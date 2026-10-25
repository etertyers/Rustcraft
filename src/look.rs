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

/// The armor sheet's row for `material`'s layer (0 outer, 1 inner).
pub fn armor_row(material: &str, layer: usize) -> Option<usize> {
    let index = ARMOR_MATERIALS.iter().position(|(m, _)| *m == material)?;
    Some(index * 2 + layer)
}

/// Every armor layer texture stacked into one picture, a row of 64 × 32
/// (or the packs' larger size) for each material's layer in
/// `ARMOR_MATERIALS` order; a layer no pack has is left clear.
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
    let mut sheet = RgbaImage::new(width, row * layers.len() as u32);
    for (i, layer) in layers.iter().enumerate() {
        if let Some(image) = layer {
            let image =
                image::imageops::resize(image, width, row, image::imageops::FilterType::Nearest);
            image::imageops::replace(&mut sheet, &image, 0, i64::from(row) * i as i64);
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
    ids.iter()
        .filter_map(|id| match Pack::open(&dir.join(id)) {
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
        textures.extend((0..SWEEP_FRAMES).map(entities::sweep_texture));
        textures.extend((0..entities::ORB_ICONS).map(entities::orb_texture));
        textures.extend(entities::END_TEXTURES.map(str::to_owned));
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
            sweep: std::array::from_fn(|i| atlas.uv_or_missing(&entities::sweep_texture(i))),
            orb: std::array::from_fn(|i| atlas.uv_or_missing(&entities::orb_texture(i))),
            dragon_fireball: atlas.uv_or_missing(entities::DRAGON_FIREBALL_TEXTURE),
            crystal_beam: atlas.uv_or_missing(entities::CRYSTAL_BEAM_TEXTURE),
            gateway_beam: atlas.uv_or_missing(entities::GATEWAY_BEAM_TEXTURE),
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
        if layer == res.vanilla_layer() || layer == 0 {
            dims == found
        } else {
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
        assert_eq!(sheet.height(), sheet.width() / 2 * 18);
        let row = sheet.width() / 2;
        let iron = armor_row("iron", 0).unwrap() as u32;
        let opaque = (0..row)
            .flat_map(|y| (0..sheet.width()).map(move |x| (x, y)))
            .filter(|&(x, y)| sheet.get_pixel(x, iron * row + y)[3] > 0)
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
}
