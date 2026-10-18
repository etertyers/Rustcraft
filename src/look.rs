//! Everything the game draws that comes from the resource packs: the
//! atlas, block models, item icons, sky, mob textures and GUI images.
//! Built at startup and again whenever the selected packs change.

use std::{path::Path, sync::Arc, time::Instant};

use anyhow::Result;
use image::RgbaImage;
use log::{info, warn};

use crate::{
    entity::mobs::MobRegistry,
    inventory::item,
    pack::{
        self, Pack, Resources,
        model::{BlockOverrides, ItemModels},
    },
    render::{
        SkyImages,
        atlas::{Atlas, MISSING_TILE},
        crack,
        icons::ItemIcons,
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
    /// One per mob kind, in registry order, then the arrow's.
    pub mob_textures: Vec<RgbaImage>,
    /// Atlas rects of the mining crack's stages.
    pub crack_uvs: Vec<[f32; 4]>,
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
        let sky = SkyImages::load(&res)?;
        let mob_textures = mobs
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
            crack_uvs,
        })
    }

    /// Builds the look with the packs named in `ids` from the packs folder.
    pub fn with_packs(assets: &Path, ids: &[String], mobs: &MobRegistry) -> Result<Self> {
        Self::build(assets, open_packs(&pack::packs_dir(), ids), mobs)
    }
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
    use crate::world::{block::BlockState, blocks};

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
        assert_eq!(look.mob_textures.len(), mobs().iter().count() + 1);
        assert!(look.atlas.has("block/grass_block_top"));
        assert!(look.atlas.has("block/destroy_stage_9"));
        // Concrete has no texture without a pack.
        let concrete = look.models.get(BlockState::of(blocks::WHITE_CONCRETE));
        assert_eq!(concrete.faces[0].uv, look.atlas.uv_rect(MISSING_TILE));
    }

    #[test]
    fn a_16x_and_a_32x_pack_each_retexture_and_fall_back() {
        let assets = crate::assets::dir();
        for (name, tile) in [("Ashen_1.8.3.zip", 16), ("Prime's HD Textures (FREE)", 32)] {
            let pack = Pack::open(&assets.join(name)).unwrap();
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
        let ashen = Pack::open(&assets.join("Ashen_1.8.3.zip")).unwrap();
        let look = Look::build(&assets, vec![ashen], &mobs).unwrap();
        assert_eq!(
            look.res.texture("entity/pig/temperate_pig").unwrap().layer,
            1
        );
        assert_ne!(look.mob_textures[0], base.mob_textures[0]);
        // Prime has no pig or zombie: Kenzo's stay.
        let prime = Pack::open(&assets.join("Prime's HD Textures (FREE)")).unwrap();
        let look = Look::build(&assets, vec![prime], &mobs).unwrap();
        assert_eq!(look.mob_textures[0], base.mob_textures[0]);
        assert_eq!(look.mob_textures[1], base.mob_textures[1]);
    }

    #[test]
    fn missing_packs_are_skipped() {
        let dir = crate::world::save::TempDir::new("open-packs");
        let packs = open_packs(dir.path(), &["gone.zip".to_owned()]);
        assert!(packs.is_empty());
    }
}
