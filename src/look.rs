//! Everything the game draws that comes from the resource packs: the
//! atlas, block models, item icons, sky, mob textures and GUI images.
//! Built at startup and again whenever the selected packs change.

use std::{path::Path, sync::Arc, time::Instant};

use anyhow::Result;
use image::RgbaImage;
use log::{info, warn};

use crate::{
    entity::{effects::SWEEP_FRAMES, mobs::MobRegistry},
    inventory::item,
    pack::{
        self, Pack, Resources,
        model::{BlockOverrides, ItemModels},
    },
    render::{
        SkyImages,
        atlas::{Atlas, MISSING_TILE},
        crack,
        entities::{self, EffectUvs},
        icons::ItemIcons,
        player::prepare_skin,
    },
    world::{
        biome::BiomeColors,
        block::{self, texture_location},
        mesh::BlockModels,
    },
};

/// The arrow's texture, drawn after the mob kinds' textures.
pub const ARROW_TEXTURE: &str = "entity/projectiles/arrow";
/// The minecart's texture, after the arrow's.
pub const MINECART_TEXTURE: &str = "entity/minecart";
/// The ender dragon's and the end crystal's, after the minecart's.
pub const DRAGON_TEXTURE: &str = "entity/enderdragon/dragon";
pub const CRYSTAL_TEXTURE: &str = "entity/endercrystal/endercrystal";
/// The elytra's, after the crystal's.
pub const ELYTRA_TEXTURE: &str = "entity/elytra";

/// After the mob kinds' textures come the arrow's, the cart's, the
/// dragon's, the crystal's and the elytra's (0–4), then each wood's boat
/// and chest boat, then the armor sheet and last the skin.
pub const EXTRA_BOATS: usize = 5;
pub const BOAT_TEXTURES: usize = crate::entity::boat::WOODS.len() * 2;
pub const EXTRA_ARMOR: usize = EXTRA_BOATS + BOAT_TEXTURES;
pub const EXTRA_SKIN: usize = EXTRA_ARMOR + 1;

/// The texture of a boat kind, counted from `EXTRA_BOATS`.
pub fn boat_texture(kind: crate::entity::boat::BoatKind) -> usize {
    kind.wood * 2 + usize::from(kind.chest)
}

/// The armor materials in the armor sheet, two rows each (the outer layer,
/// then the leggings' inner one): (26.x name, older `models/armor` name).
pub const ARMOR_MATERIALS: [(&str, &str); 9] = [
    ("leather", "leather"),
    ("leather_overlay", "leather"),
    ("chainmail", "chainmail"),
    ("iron", "iron"),
    ("gold", "gold"),
    ("diamond", "diamond"),
    ("netherite", "netherite"),
    ("turtle_scute", "turtle"),
    ("copper", "copper"),
];

/// How many layers are in the armor sheet, two for each material.
pub const ARMOR_LAYERS: usize = ARMOR_MATERIALS.len() * 2;

/// The armor sheet is a grid of layers this many across: one tall column
/// of 18 would be 2304 pixels with a 32x pack, past what egui and some GPUs
/// take.
pub const ARMOR_COLUMNS: usize = 4;

/// The armor sheet's grid: (columns, rows).
pub const fn armor_grid() -> (usize, usize) {
    (ARMOR_COLUMNS, ARMOR_LAYERS.div_ceil(ARMOR_COLUMNS))
}

/// The armor sheet's cell, as (column, row), for `material`'s layer (0
/// outer, 1 inner).
pub fn armor_row(material: &str, layer: usize) -> Option<(usize, usize)> {
    let index = ARMOR_MATERIALS.iter().position(|(m, _)| *m == material)? * 2 + layer;
    Some((index % ARMOR_COLUMNS, index / ARMOR_COLUMNS))
}

/// Every armor layer texture in one picture: a grid of cells of 64 × 32
/// (or the packs' larger size), `ARMOR_COLUMNS` across, one for each
/// material's layer in `ARMOR_MATERIALS` order; a layer no pack has is left
/// clear.
pub fn armor_sheet(res: &Resources) -> RgbaImage {
    let layers: Vec<Option<RgbaImage>> = ARMOR_MATERIALS
        .iter()
        .flat_map(|&(new, old)| {
            let overlay = new.strip_suffix("_overlay").is_some();
            [("humanoid", 1), ("humanoid_leggings", 2)].map(|(dir, n)| {
                let old = if overlay {
                    format!("models/armor/{old}_layer_{n}_overlay")
                } else {
                    format!("models/armor/{old}_layer_{n}")
                };
                let new = format!("entity/equipment/{dir}/{new}");
                res.texture_named(&[&new, &old]).map(|t| t.first_frame())
            })
        })
        .collect();
    let width = layers
        .iter()
        .flatten()
        .map(RgbaImage::width)
        .max()
        .unwrap_or(64);
    let row = width / 2;
    let (columns, rows) = armor_grid();
    let mut sheet = RgbaImage::new(width * columns as u32, row * rows as u32);
    for (i, layer) in layers.iter().enumerate() {
        if let Some(image) = layer {
            let image =
                image::imageops::resize(image, width, row, image::imageops::FilterType::Nearest);
            let (x, y) = ((i % columns) as i64, (i / columns) as i64);
            image::imageops::replace(&mut sheet, &image, x * i64::from(width), y * i64::from(row));
        }
    }
    sheet
}

/// The skin, armor sheet and elytra, for the figure in the inventory.
#[derive(Clone, Copy)]
pub struct FigureImages<'a> {
    pub skin: &'a RgbaImage,
    pub armor: &'a RgbaImage,
    pub elytra: &'a RgbaImage,
}

pub struct Look {
    pub res: Resources,
    pub atlas: Atlas,
    pub models: Arc<BlockModels>,
    pub icons: ItemIcons,
    /// Item model sprites and display transforms (for the held item, M16).
    pub item_models: Arc<ItemModels>,
    pub sky: SkyImages,
    /// One per mob kind, in registry order, then the arrow's, the cart's,
    /// the dragon's, the crystal's, the elytra's, the armor sheet and last
    /// the player's skin.
    pub mob_textures: Vec<RgbaImage>,
    /// The packs' custom entity models, made the active ones when this
    /// look is installed.
    pub cem: crate::cem::Registry,
    /// Whether the skin has slim (3 pixel) arms.
    pub skin_slim: bool,
    /// Atlas rects of the mining crack's stages.
    pub crack_uvs: Vec<[f32; 4]>,
    /// Where the combat effects are in the atlas.
    pub effect_uvs: EffectUvs,
}

/// Opens the packs named in `ids` (highest priority first) from `dir`;
/// ones that are gone or broken are skipped with a warning.
pub fn open_packs(dir: &Path, ids: &[String]) -> Vec<Pack> {
    open_packs_in(dir, &pack::builtin_dir(), ids)
}

/// Like [`open_packs`], with the built-in packs' folder given.
pub fn open_packs_in(dir: &Path, builtin: &Path, ids: &[String]) -> Vec<Pack> {
    ids.iter()
        .filter_map(|id| match Pack::open(&pack::find(dir, builtin, id)) {
            Ok(pack) => Some(pack),
            Err(err) => {
                warn!("skipping resource pack {id:?}: {err:#}");
                None
            }
        })
        .collect()
}

impl Look {
    /// The pictures the player's figure in the inventory is drawn with.
    pub fn figure(&self) -> FigureImages<'_> {
        let n = self.mob_textures.len();
        FigureImages {
            skin: self.skin(),
            armor: self.armor_sheet(),
            elytra: &self.mob_textures[n - 3],
        }
    }

    /// Every armor layer, the mob texture before the skin.
    pub fn armor_sheet(&self) -> &RgbaImage {
        &self.mob_textures[self.mob_textures.len() - 2]
    }

    /// The player's skin, the last of the mob textures.
    pub fn skin(&self) -> &RgbaImage {
        self.mob_textures.last().expect("the skin is loaded last")
    }

    /// Builds the look from Kenzo's textures in `assets` with `packs` over
    /// them (highest priority first).
    pub fn build(assets: &Path, packs: Vec<Pack>, mobs: &MobRegistry) -> Result<Self> {
        Self::build_with(assets, packs, mobs, true)
    }

    /// Like `build`; `vanilla` says whether the downloaded vanilla assets
    /// are the lowest layer.
    pub fn build_with(
        assets: &Path,
        packs: Vec<Pack>,
        mobs: &MobRegistry,
        vanilla: bool,
    ) -> Result<Self> {
        let started = Instant::now();
        let names: Vec<String> = packs.iter().map(|p| p.name().to_owned()).collect();
        let res = Resources::load_with(assets, packs, vanilla)?;
        let blocks = block::registry();
        let items = item::registry();

        let overrides = BlockOverrides::load(&res, blocks);
        let item_models = ItemModels::load(
            &res,
            items.iter().map(|item| {
                let drawn_as_block = item.texture.is_none() && item.places.is_some();
                (item.name.as_str(), drawn_as_block, item.tool.is_some())
            }),
        );
        let mut textures = blocks.texture_locations();
        textures.extend(items.texture_locations());
        textures.extend(crate::render::icons::BOW_PULLING.map(str::to_owned));
        textures.extend(crate::render::icons::potion_textures());
        textures.extend(
            overrides
                .textures()
                .map(|name| texture_location(name).into_owned()),
        );
        textures.extend(
            items
                .iter()
                .filter_map(|item| item_models.texture(&item.name))
                .map(str::to_owned),
        );
        textures.extend((0..crack::STAGES).map(crack::texture));
        textures.insert(entities::CRIT_TEXTURE.to_owned());
        textures.insert(entities::SHADOW_TEXTURE.to_owned());
        textures.extend((0..SWEEP_FRAMES).map(entities::sweep_texture));
        textures.extend((0..entities::ORB_ICONS).map(entities::orb_texture));
        textures.extend(entities::END_TEXTURES.map(str::to_owned));
        textures.insert(entities::HOOK_TEXTURE.to_owned());
        textures.insert(entities::LINE_TEXTURE.to_owned());
        let atlas = Atlas::build(&res, textures);

        let colors = BiomeColors::load(&res).unwrap_or_else(|err| {
            warn!("{err:#}; using default biome colors");
            BiomeColors::uniform()
        });
        let models = Arc::new(BlockModels::with_packs(
            blocks,
            |name| atlas.get(name).map(|tile| atlas.uv_rect(tile)),
            atlas.uv_rect(MISSING_TILE),
            colors,
            &overrides,
        ));
        let icons = ItemIcons::new(&atlas, models.clone(), items, &item_models);
        let crack_uvs = (0..crack::STAGES)
            .map(|i| atlas.uv_or_missing(&crack::texture(i)))
            .collect();
        let effect_uvs = EffectUvs {
            crit: atlas.uv_or_missing(entities::CRIT_TEXTURE),
            shadow: atlas.uv_or_missing(entities::SHADOW_TEXTURE),
            sweep: std::array::from_fn(|i| atlas.uv_or_missing(&entities::sweep_texture(i))),
            orb: std::array::from_fn(|i| atlas.uv_or_missing(&entities::orb_texture(i))),
            dragon_fireball: atlas.uv_or_missing(entities::DRAGON_FIREBALL_TEXTURE),
            crystal_beam: atlas.uv_or_missing(entities::CRYSTAL_BEAM_TEXTURE),
            gateway_beam: atlas.uv_or_missing(entities::GATEWAY_BEAM_TEXTURE),
            hook: atlas.uv_or_missing(entities::HOOK_TEXTURE),
            line: atlas.uv_or_missing(entities::LINE_TEXTURE),
        };
        let sky = SkyImages::load(&res)?;
        let (skin, skin_slim) = load_skin(&res, assets);
        let mut mob_textures: Vec<RgbaImage> = mobs
            .textures_sized()
            .into_iter()
            .map(|(name, size)| (name, Some(size)))
            .chain(
                [
                    ARROW_TEXTURE,
                    MINECART_TEXTURE,
                    DRAGON_TEXTURE,
                    CRYSTAL_TEXTURE,
                    ELYTRA_TEXTURE,
                ]
                .map(|name| (name, None)),
            )
            .map(|(name, size)| mob_image(&res, name, size))
            .collect();
        for wood in crate::entity::boat::WOODS {
            mob_textures.push(mob_image(
                &res,
                &format!("entity/boat/{wood}"),
                Some((128, 64)),
            ));
            mob_textures.push(mob_image(
                &res,
                &format!("entity/chest_boat/{wood}"),
                Some((128, 128)),
            ));
        }
        // Custom entity models (a pack's `optifine/cem`, M58) bring their
        // own texture, worn in the plain variant's place.
        let mob_names: Vec<&str> = mobs.iter().map(|(_, def)| def.name.as_str()).collect();
        let cem = crate::cem::Registry::load(&|path| res.bytes(path), &mob_names);
        for (name, model) in &cem.models {
            let Some(kind) = mobs.find(name) else {
                continue;
            };
            match &model.texture {
                Some(path) => {
                    if let Some(image) = cem_texture(&res, path) {
                        mob_textures[mobs.texture_slot(kind, 0)] = image;
                    }
                }
                // No texture of its own: the mob's usual one, but the one
                // that fits this model's UVs (the pack's `textureSize`, not
                // the size the built-in boxes declare).
                None if !model.baby => {
                    let size = (model.texture_size.0 as u32, model.texture_size.1 as u32);
                    if let Some(image) = mob_texture(&res, &mobs.get(kind).texture, Some(size)) {
                        mob_textures[mobs.texture_slot(kind, 0)] = image;
                    }
                }
                None => {}
            }
        }
        mob_textures.push(armor_sheet(&res));
        mob_textures.push(skin);
        info!(
            "resources: {} over the defaults, {} blocks re-modeled, built in {} ms",
            if names.is_empty() {
                "no packs".to_owned()
            } else {
                names.join(", ")
            },
            overrides.len(),
            started.elapsed().as_millis()
        );
        Ok(Self {
            cem,
            res,
            atlas,
            models,
            icons,
            item_models: Arc::new(item_models),
            sky,
            mob_textures,
            skin_slim,
            crack_uvs,
            effect_uvs,
        })
    }

    /// Builds the look with the packs named in `ids` from the packs folder.
    pub fn with_packs(assets: &Path, ids: &[String], mobs: &MobRegistry) -> Result<Self> {
        Self::build(assets, open_packs(&pack::packs_dir(), ids), mobs)
    }
}

/// The player's skin: one in a resource pack's `entity/player/` (wide
/// Steve before slim Alex, the highest pack first), else the one in
/// `assets/skins/`. Returns the 64×64 skin and whether its arms are slim.
pub fn load_skin(res: &Resources, assets: &Path) -> (RgbaImage, bool) {
    const NAMES: [(&str, bool); 2] = [
        ("entity/player/wide/steve", false),
        ("entity/player/slim/alex", true),
    ];
    for layer in (1..res.layers().len()).rev() {
        for (name, slim) in NAMES {
            if let Some(texture) = res.texture_in(layer, name) {
                return (prepare_skin(texture.image), slim);
            }
        }
    }
    let dir = assets.join("skins");
    for (file, slim) in [("steve-skin.png", false), ("alex.png", true)] {
        match image::open(dir.join(file)) {
            Ok(image) => return (prepare_skin(image.into_rgba8()), slim),
            Err(err) => warn!("no skin {file}: {err}"),
        }
    }
    (missing_image(), false)
}

/// A mob's texture from the highest layer that has it. One from the vanilla
/// fallback must be the size the hand-built model is laid out for (newer
/// versions change some layouts, and a different size means different
/// UVs), else it is skipped with a log line, and `None` is returned.
pub fn mob_texture(res: &Resources, name: &str, size: Option<(u32, u32)>) -> Option<RgbaImage> {
    let Some(found) = size else {
        return res.texture(name).map(|t| t.first_frame());
    };
    // The first layer, from the top, whose texture fits the model's UVs:
    // the vanilla fallback and Kenzo's base layer must be exactly the
    // size (an older layout is a different size), a pack's may be any
    // multiple of it (an HD pack).
    let fits = |layer: usize, dims: (u32, u32)| {
        if layer == 0 {
            dims == found
        } else {
            // A pack's or vanilla's may be any whole multiple (an HD pack,
            // vanilla's ghast at twice its old size): same shape.
            dims.0 * found.1 == dims.1 * found.0
        }
    };
    let mut first = None;
    for layer in res.priority() {
        let Some(texture) = res.texture_in(layer, name) else {
            continue;
        };
        let dims = texture.image.dimensions();
        if fits(layer, dims) {
            return Some(texture.first_frame());
        }
        warn!(
            "skipping {name} from layer {layer}: it is {}×{} but the model wants {}×{}",
            dims.0, dims.1, found.0, found.1
        );
        first.get_or_insert(texture);
    }
    // Vanilla's file is in a 26.x layout the built-in boxes don't have (the
    // rabbit's): Rustcraft's own old-layout texture goes with them.
    if let Some(own) = res.own_texture(name)
        && own.image.dimensions() == found
    {
        static REPORTED: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());
        if let Ok(mut reported) = REPORTED.lock()
            && !reported.iter().any(|n| n == name)
        {
            info!("{name}: vanilla's layout isn't modeled, using Rustcraft's own texture");
            reported.push(name.to_owned());
        }
        return Some(own.first_frame());
    }
    if first.is_none() {
        warn!("no layer has {name}");
    }
    // Nothing fits the layout: a vanilla one is skipped (it's never the
    // base look), any other is used as is, as before.
    first
        .filter(|texture| !texture.vanilla)
        .map(|t| t.first_frame())
}

/// `mob_texture`, or the checkerboard.
fn mob_image(res: &Resources, name: &str, size: Option<(u32, u32)>) -> RgbaImage {
    mob_texture(res, name, size).unwrap_or_else(missing_image)
}

/// A custom model's texture: `path` as the `.jem` says it, in the
/// models' folder or as a resource location.
fn cem_texture(res: &Resources, path: &str) -> Option<RgbaImage> {
    let path = path.trim_start_matches("./");
    let bytes = res
        .bytes(&format!("optifine/cem/{path}"))
        .or_else(|| res.bytes(path))
        .or_else(|| res.bytes(&format!("textures/{path}")))?;
    image::load_from_memory(&bytes).ok().map(|i| i.to_rgba8())
}

/// A small magenta and black checkerboard for a missing mob texture.
fn missing_image() -> RgbaImage {
    RgbaImage::from_fn(16, 16, |x, y| {
        if (x / 8 + y / 8).is_multiple_of(2) {
            image::Rgba([248, 0, 248, 255])
        } else {
            image::Rgba([0, 0, 0, 255])
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        pack::tests::{ASHEN, PRIME, kenzos_pack},
        world::{block::BlockState, blocks},
    };

    fn mobs() -> MobRegistry {
        MobRegistry::load(&crate::assets::dir().join("data/mobs.ron")).unwrap()
    }

    #[test]
    fn a_vanilla_mob_texture_of_the_wrong_size_is_skipped() {
        let dir = crate::world::save::TempDir::new("vanilla-mobs");
        let root = dir.path().join("vanilla/assets/minecraft/textures/entity");
        std::fs::create_dir_all(&root).unwrap();
        for (name, w, h) in [("pig", 64, 32), ("horse", 128, 128)] {
            RgbaImage::from_pixel(w, h, image::Rgba([9, 9, 9, 255]))
                .save(root.join(format!("{name}.png")))
                .unwrap();
        }
        let res = Resources::new(
            Pack::base(dir.path()),
            Vec::new(),
            std::collections::HashMap::new(),
        )
        .with_vanilla(Pack::vanilla(&dir.path().join("vanilla")).unwrap());
        // The model is laid out for 64×32: that size is taken, another isn't.
        assert_eq!(
            mob_image(&res, "entity/pig", Some((64, 32))).dimensions(),
            (64, 32)
        );
        assert_eq!(
            mob_image(&res, "entity/pig", Some((64, 64))),
            missing_image()
        );
        assert_eq!(
            mob_image(&res, "entity/horse", None).dimensions(),
            (128, 128)
        );
        assert_eq!(
            mob_image(&res, "entity/cow", Some((64, 32))),
            missing_image()
        );
    }

    #[test]
    fn a_base_texture_in_an_older_layout_gives_way_to_the_vanilla_one_that_fits() {
        let dir = crate::world::save::TempDir::new("base-horse");
        let base = dir.path().join("textures/entity");
        let vanilla = dir.path().join("vanilla/assets/minecraft/textures/entity");
        std::fs::create_dir_all(&base).unwrap();
        std::fs::create_dir_all(&vanilla).unwrap();
        let save = |root: &std::path::Path, name: &str, size: u32| {
            RgbaImage::from_pixel(size, size, image::Rgba([9, 9, 9, 255]))
                .save(root.join(format!("{name}.png")))
                .unwrap();
        };
        // Kenzo's horse is 128×128, vanilla 1.21's 64×64; his cow has no
        // vanilla twin.
        save(&base, "horse", 128);
        save(&vanilla, "horse", 64);
        save(&base, "cow", 128);
        let res = Resources::new(
            Pack::base(dir.path()),
            Vec::new(),
            std::collections::HashMap::new(),
        )
        .with_vanilla(Pack::vanilla(&dir.path().join("vanilla")).unwrap());
        assert_eq!(
            mob_image(&res, "entity/horse", Some((64, 64))).dimensions(),
            (64, 64),
            "the horse model is laid out for 64×64"
        );
        assert_eq!(
            mob_image(&res, "entity/cow", Some((64, 64))).dimensions(),
            (128, 128),
            "nothing else fits, so the base look stays"
        );
    }

    #[test]
    fn the_horse_and_its_overlays_load_in_the_64_by_64_layout_when_vanilla_is_there() {
        let assets = crate::assets::dir();
        if !assets.join("vanilla").is_dir() {
            return;
        }
        let mobs = mobs();
        let look = Look::build_with(&assets, Vec::new(), &mobs, true).unwrap();
        let kind = mobs.find("horse").unwrap();
        let horse = mobs.get(kind);
        assert_eq!(
            look.mob_textures[mobs.texture_slot(kind, 0)].dimensions(),
            (64, 64)
        );
        for variant in 0..=horse.variants.len() as u8 {
            assert_eq!(
                look.mob_textures[mobs.texture_slot(kind, variant)].dimensions(),
                (64, 64)
            );
        }
        for (i, layer) in horse.layers.iter().enumerate() {
            assert_eq!(
                look.mob_textures[mobs.layer_slot(kind, i)].dimensions(),
                (64, 64),
                "{}",
                layer.texture
            );
        }
    }

    #[test]
    fn kenzos_textures_alone_give_the_v1_look() {
        let assets = crate::assets::dir();
        let look = Look::build_with(&assets, Vec::new(), &mobs(), false).unwrap();
        assert_eq!(look.atlas.tile_size(), 16);
        // Water and lava animate from their .mcmeta files.
        assert!(look.atlas.animated() >= 2, "{}", look.atlas.animated());
        // The mobs, then the arrow, cart, dragon, crystal, elytra, armor and skin.
        assert_eq!(
            look.mob_textures.len(),
            mobs().texture_count() + EXTRA_SKIN + 1
        );
        // Kenzo's 1.12 armor layers fill the sheet's rows.
        let sheet = look.armor_sheet();
        let (columns, rows) = armor_grid();
        let cell = sheet.width() / columns as u32;
        assert_eq!(sheet.height(), cell / 2 * rows as u32);
        let row = cell / 2;
        let (iron_x, iron_y) = armor_row("iron", 0).unwrap();
        let opaque = (0..row)
            .flat_map(|y| (0..cell).map(move |x| (x, y)))
            .filter(|&(x, y)| {
                sheet.get_pixel(iron_x as u32 * cell + x, iron_y as u32 * row + y)[3] > 0
            })
            .count();
        assert!(opaque > 100, "iron's outer layer is there: {opaque}");
        assert!(look.atlas.has("block/grass_block_top"));
        assert!(look.atlas.has("block/destroy_stage_9"));
        // Concrete has no texture without a pack.
        let concrete = look.models.get(BlockState::of(blocks::WHITE_CONCRETE));
        assert_eq!(concrete.faces[0].uv, look.atlas.uv_rect(MISSING_TILE));
    }

    #[test]
    fn a_16x_and_a_32x_pack_each_retexture_and_fall_back() {
        let assets = crate::assets::dir();
        for (name, tile) in [(ASHEN, 16), (PRIME, 32)] {
            let Some(pack) = kenzos_pack(name) else {
                continue;
            };
            let look = Look::build(&assets, vec![pack], &mobs()).unwrap();
            assert_eq!(look.atlas.tile_size(), tile, "{name}");
            // Blocks and items come from the pack...
            for texture in [
                "block/stone",
                "block/dirt",
                "block/oak_planks",
                "item/stick",
            ] {
                assert_eq!(
                    look.res.texture(texture).unwrap().layer,
                    1,
                    "{name}: {texture}"
                );
            }
            let concrete = look.models.get(BlockState::of(blocks::WHITE_CONCRETE));
            assert_ne!(
                concrete.faces[0].uv,
                look.atlas.uv_rect(MISSING_TILE),
                "{name}: the pack brings concrete"
            );
            // ...and what it lacks (the v1.0 bed's block faces) falls back.
            assert_eq!(look.res.texture("block/bed_feet_end").unwrap().layer, 0);
            assert!(look.atlas.has("block/bed_feet_end"));
        }
    }

    #[test]
    fn mobs_retexture_with_a_pack_that_has_them_and_fall_back_without() {
        let assets = crate::assets::dir();
        let mobs = mobs();
        let base = Look::build(&assets, Vec::new(), &mobs).unwrap();
        // Ashen has the pig under 1.20's name, pig/pig.png.
        let (Some(ashen), Some(prime)) = (kenzos_pack(ASHEN), kenzos_pack(PRIME)) else {
            return;
        };
        let look = Look::build(&assets, vec![ashen], &mobs).unwrap();
        assert_eq!(
            look.res.texture("entity/pig/temperate_pig").unwrap().layer,
            1
        );
        assert_ne!(look.mob_textures[0], base.mob_textures[0]);
        // Prime has no pig or zombie: Kenzo's stay.
        let look = Look::build(&assets, vec![prime], &mobs).unwrap();
        assert_eq!(look.mob_textures[0], base.mob_textures[0]);
        assert_eq!(look.mob_textures[1], base.mob_textures[1]);
    }

    #[test]
    fn the_skin_comes_from_assets_unless_a_pack_brings_one() {
        let assets = crate::assets::dir();
        let look = Look::build(&assets, Vec::new(), &mobs()).unwrap();
        let skin = look.mob_textures.last().unwrap();
        assert_eq!(skin.dimensions(), (64, 64));
        assert!(!look.skin_slim);
        let steve = prepare_skin(
            image::open(assets.join("skins/steve-skin.png"))
                .unwrap()
                .into_rgba8(),
        );
        assert_eq!(skin, &steve);

        // Kenzo's packs have no player skin: his stays.
        if let Some(ashen) = kenzos_pack(ASHEN) {
            let look = Look::build(&assets, vec![ashen], &mobs()).unwrap();
            assert_eq!(look.mob_textures.last().unwrap(), &steve);
        }

        // A pack with a slim skin overrides it, as an old 64×32 one too.
        let dir = crate::world::save::TempDir::new("skin-pack");
        let mut old = image::RgbaImage::from_pixel(64, 32, image::Rgba([7, 8, 9, 255]));
        old.put_pixel(40, 8, image::Rgba([1, 1, 1, 0]));
        let mut bytes = Vec::new();
        old.write_to(
            &mut std::io::Cursor::new(&mut bytes),
            image::ImageFormat::Png,
        )
        .unwrap();
        let zip = dir.path().join("skin.zip");
        crate::pack::tests::write_zip(
            &zip,
            &[
                ("pack.mcmeta", crate::pack::tests::MCMETA),
                (
                    "assets/minecraft/textures/entity/player/slim/alex.png",
                    &bytes,
                ),
            ],
        );
        let pack = Pack::open(&zip).unwrap();
        let look = Look::build(&assets, vec![pack], &mobs()).unwrap();
        assert!(look.skin_slim);
        let skin = look.mob_textures.last().unwrap();
        assert_eq!(skin.dimensions(), (64, 64));
        assert_eq!(skin.get_pixel(8, 8).0, [7, 8, 9, 255]);
        // The legacy left leg was made from the right one.
        assert_eq!(skin.get_pixel(20, 48).0, [7, 8, 9, 255]);
    }

    #[test]
    fn combat_effects_have_pictures_in_every_look() {
        let assets = crate::assets::dir();
        let mut looks = vec![Look::build(&assets, Vec::new(), &mobs()).unwrap()];
        for name in [ASHEN, PRIME] {
            if let Some(pack) = kenzos_pack(name) {
                looks.push(Look::build(&assets, vec![pack], &mobs()).unwrap());
            }
        }
        for look in looks {
            let uvs = look.effect_uvs;
            assert_ne!(uvs.crit, look.atlas.uv_rect(MISSING_TILE));
            assert!(
                uvs.sweep
                    .iter()
                    .all(|&uv| uv != look.atlas.uv_rect(MISSING_TILE))
            );
        }
    }

    #[test]
    fn missing_packs_are_skipped() {
        let dir = crate::world::save::TempDir::new("open-packs");
        let packs = open_packs(dir.path(), &["gone.zip".to_owned()]);
        assert!(packs.is_empty());
    }

    fn png(width: u32, height: u32, color: [u8; 4]) -> Vec<u8> {
        let image = RgbaImage::from_pixel(width, height, image::Rgba(color));
        let mut bytes = Vec::new();
        image
            .write_to(
                &mut std::io::Cursor::new(&mut bytes),
                image::ImageFormat::Png,
            )
            .unwrap();
        bytes
    }

    /// A custom model without a texture of its own gets the mob's texture
    /// that fits the model's `textureSize` (Fresh Animations' pig is 64×64
    /// where the built-in boxes declared 64×32, so its texture used to be
    /// refused and an older, wrong-sized one drawn).
    #[test]
    fn a_custom_model_gets_the_texture_that_fits_its_texture_size() {
        use crate::pack::tests::{MCMETA, write_zip};
        let dir = crate::world::save::TempDir::new("cem-texture-size");
        let zip = dir.path().join("fa.zip");
        write_zip(
            &zip,
            &[
                ("pack.mcmeta", MCMETA),
                (
                    "assets/minecraft/optifine/cem/pig.jem",
                    br#"{"textureSize": [128, 64], "models": [{"part": "body", "id": "body", "boxes": [{"coordinates": [-4, -4, -4, 8, 8, 8], "textureOffset": [0, 0]}]}]}"#,
                ),
                (
                    "assets/minecraft/textures/entity/pig/pig_temperate.png",
                    &png(128, 64, [200, 100, 100, 255]),
                ),
            ],
        );
        let mobs = mobs();
        let look = Look::build(
            &crate::assets::dir(),
            vec![Pack::open(&zip).unwrap()],
            &mobs,
        )
        .unwrap();
        let pig = mobs.find("pig").unwrap();
        let texture = &look.mob_textures[mobs.texture_slot(pig, 0)];
        assert_eq!(texture.dimensions(), (128, 64));
        assert_eq!(texture.get_pixel(3, 3).0, [200, 100, 100, 255]);
    }

    /// 26.3's pig and cow keep their old layout in the top half of a 64×64
    /// canvas, and the bee is 64×64: the declared sizes are those, so the
    /// vanilla file (not an older one from Rustcraft's own textures) is used.
    #[test]
    fn mobs_whose_canvas_grew_declare_it_and_use_the_vanilla_file() {
        let assets = crate::assets::dir();
        if !assets.join("vanilla/assets/minecraft").is_dir() {
            eprintln!("skipping: no assets/vanilla");
            return;
        }
        let mobs = mobs();
        let look = Look::build(&assets, Vec::new(), &mobs).unwrap();
        for name in ["pig", "cow", "bee"] {
            let kind = mobs.find(name).unwrap();
            let texture = &look.mob_textures[mobs.texture_slot(kind, 0)];
            assert_eq!(texture.dimensions(), (64, 64), "{name}");
        }
    }

    /// Every mob's textures (and its variants' and layers') are in the
    /// vanilla layer under 26.3's names, found through the name table, so a
    /// build that has only the vanilla layer draws every mob.
    #[test]
    fn every_mob_texture_resolves_in_the_vanilla_layer_alone() {
        let assets = crate::assets::dir();
        if !assets.join("vanilla/assets/minecraft").is_dir() {
            eprintln!("skipping: no assets/vanilla");
            return;
        }
        let res = Resources::load(&assets, Vec::new()).unwrap();
        let mobs = mobs();
        let missing: Vec<&str> = mobs
            .textures()
            .into_iter()
            .filter(|name| res.texture_in(res.vanilla_layer(), name).is_none())
            .collect();
        assert!(missing.is_empty(), "not in vanilla: {missing:?}");
    }

    /// The Vanilla style shows only vanilla's files: every block and item
    /// texture comes from `assets/vanilla`, except the ones vanilla has no
    /// file for (Rustcraft's own), which are few and listed.
    #[test]
    fn the_vanilla_style_resolves_every_texture_to_vanilla_or_a_listed_rustcraft_only_file() {
        let assets = crate::assets::dir();
        if !assets.join("vanilla/assets/minecraft").is_dir() {
            eprintln!("skipping: no assets/vanilla");
            return;
        }
        let look = Look::build(&assets, Vec::new(), &mobs()).unwrap();
        let mut own = Vec::new();
        let mut names = std::collections::BTreeSet::new();
        names.extend(crate::world::block::registry().texture_locations());
        names.extend(crate::inventory::item::registry().texture_locations());
        for name in &names {
            let Some(texture) = look.res.texture(name) else {
                continue;
            };
            if !texture.vanilla {
                own.push(name.clone());
            }
        }
        // Vanilla 26.3 has no file under these names (beds, chests and the
        // like are entities there), so Rustcraft's own fill in.
        let listed = [
            "block/bed_feet_end",
            "block/bed_feet_side",
            "block/bed_feet_top",
            "block/bed_head_end",
            "block/bed_head_side",
            "block/bed_head_top",
            "block/chest_front",
            "block/chest_side",
            "block/chest_top",
            "block/dropper_front_horizontal",
            "block/end_gateway",
            "block/end_portal",
            "block/piston_top_normal",
            "block/redstone_lamp_off",
            "item/bed",
            "item/map_empty",
            "item/shield",
        ];
        assert_eq!(own, listed, "textures not from vanilla");
        // And none comes from a pack: no pack was asked for.
        assert!(
            look.res
                .layers()
                .iter()
                .skip(1)
                .all(|pack| pack.builtin || !pack.id.is_empty())
                && look.cem.models.is_empty(),
            "no custom entity models in Vanilla"
        );
    }

    /// In Barebones, every texture Bare Bones has resolves to one of the
    /// style's own packs, never to vanilla or to Rustcraft's own.
    #[test]
    fn in_barebones_every_bare_bones_texture_resolves_to_the_barebones_stack() {
        let assets = crate::assets::dir();
        let dir = assets.join("downloads/resourcepacks");
        let stack = crate::pack::style_stack(crate::render::visuals::Style::Barebones, &dir);
        let Some(bare) =
            crate::pack::find_by_words(&dir, &["barebones"], &["freshanimations", "leaves"])
        else {
            eprintln!("skipping: no Bare Bones in {}", dir.display());
            return;
        };
        if !assets.join("vanilla/assets/minecraft").is_dir() {
            eprintln!("skipping: no assets/vanilla");
            return;
        }
        let look = Look::build(
            &assets,
            open_packs_in(&crate::pack::packs_dir(), &dir, &stack),
            &mobs(),
        )
        .unwrap();
        let pack = Pack::open(&dir.join(&bare)).unwrap();
        let mut checked = 0;
        for name in crate::world::block::registry().texture_locations() {
            if !pack.has(&format!("textures/{name}.png")) {
                continue;
            }
            checked += 1;
            let texture = look.res.texture(&name).unwrap();
            assert!(
                !texture.vanilla && texture.layer != 0,
                "{name} comes from {}",
                origin_name(&look, &texture)
            );
        }
        assert!(checked > 100, "only {checked} block textures checked");
    }

    fn origin_name(look: &Look, texture: &crate::pack::Texture) -> String {
        if texture.vanilla {
            "vanilla".to_owned()
        } else if texture.layer == 0 {
            "Rustcraft's own".to_owned()
        } else {
            look.res.layers()[texture.layer].name().to_owned()
        }
    }

    /// Going Vanilla, Barebones, Realistic and round again leaves nothing of
    /// the other styles: the packs asked for are each style's own, and the
    /// same style builds the same textures.
    #[test]
    fn switching_styles_back_and_forth_leaves_nothing_of_the_others() {
        use crate::render::visuals::Style;
        let assets = crate::assets::dir();
        if !assets.join("vanilla/assets/minecraft").is_dir() {
            eprintln!("skipping: no assets/vanilla");
            return;
        }
        let dir = assets.join("downloads/resourcepacks");
        let mut options = crate::options::Options::default();
        let mut atlases: std::collections::BTreeMap<&str, Vec<RgbaImage>> = Default::default();
        for style in [
            Style::Vanilla,
            Style::Barebones,
            Style::Realistic,
            Style::Vanilla,
            Style::Barebones,
            Style::Realistic,
        ] {
            options.set_style(style);
            let packs = options.active_packs(&dir);
            // Only this style's own packs (and the player's extras: none).
            assert_eq!(packs, crate::pack::style_stack(style, &dir), "{style:?}");
            let look = Look::build(
                &assets,
                open_packs_in(&crate::pack::packs_dir(), &dir, &packs),
                &mobs(),
            )
            .unwrap();
            atlases
                .entry(style.key())
                .or_default()
                .push(look.atlas.image().clone());
        }
        for (style, built) in &atlases {
            assert_eq!(
                built[0], built[1],
                "{style} builds the same atlas every time"
            );
        }
        // A style with packs looks different from Vanilla (when they're there).
        if !crate::pack::style_stack(Style::Barebones, &dir).is_empty() {
            assert_ne!(atlases["vanilla"][0], atlases["barebones"][0]);
        }
    }
}
