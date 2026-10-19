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

pub struct Look {
    pub res: Resources,
    pub atlas: Atlas,
    pub models: Arc<BlockModels>,
    pub icons: ItemIcons,
    /// Item model sprites and display transforms (for the held item, M16).
    pub item_models: Arc<ItemModels>,
    pub sky: SkyImages,
    /// One per mob kind, in registry order, then the arrow's, then the
    /// player's skin.
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
    /// Builds the look from Kenzo's textures in `assets` with `packs` over
    /// them (highest priority first).
    pub fn build(assets: &Path, packs: Vec<Pack>, mobs: &MobRegistry) -> Result<Self> {
        let started = Instant::now();
        let names: Vec<String> = packs.iter().map(|p| p.name().to_owned()).collect();
        let res = Resources::load(assets, packs)?;
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
        };
        let sky = SkyImages::load(&res)?;
        let (skin, skin_slim) = load_skin(&res, assets);
        let mut mob_textures: Vec<RgbaImage> = mobs
            .iter()
            .map(|(_, def)| def.texture.as_str())
            .chain([ARROW_TEXTURE])
            .map(|name| {
                res.image(name).unwrap_or_else(|| {
                    warn!("no layer has {name}");
                    missing_image()
                })
            })
            .collect();
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
    fn kenzos_textures_alone_give_the_v1_look() {
        let assets = crate::assets::dir();
        let look = Look::build(&assets, Vec::new(), &mobs()).unwrap();
        assert_eq!(look.atlas.tile_size(), 16);
        // Water and lava animate from their .mcmeta files.
        assert!(look.atlas.animated() >= 2, "{}", look.atlas.animated());
        // The mobs, the arrow and the skin.
        assert_eq!(look.mob_textures.len(), mobs().iter().count() + 2);
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
