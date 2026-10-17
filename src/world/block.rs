//! Block definitions loaded from `assets/data/blocks.ron`.
//!
//! IDs are Minecraft 1.0.0's numeric block IDs (`docs/blocks-and-items.md`).

use std::{collections::BTreeSet, path::Path};

use anyhow::{Context, Result, bail};
use serde::Deserialize;

use super::Face;
use crate::config;

pub type BlockId = u16;

/// Texture names per face. The most specific field wins:
/// a direction (`north`, …), then `top`/`bottom`, then `end` (top and bottom)
/// or `side` (the four sides), then `all`.
#[derive(Debug, Default, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Textures {
    pub all: Option<String>,
    pub end: Option<String>,
    pub side: Option<String>,
    pub top: Option<String>,
    pub bottom: Option<String>,
    pub north: Option<String>,
    pub south: Option<String>,
    pub east: Option<String>,
    pub west: Option<String>,
}

impl Textures {
    pub fn get(&self, face: Face) -> Option<&str> {
        let (specific, group) = match face {
            Face::Top => (&self.top, &self.end),
            Face::Bottom => (&self.bottom, &self.end),
            Face::North => (&self.north, &self.side),
            Face::South => (&self.south, &self.side),
            Face::East => (&self.east, &self.side),
            Face::West => (&self.west, &self.side),
        };
        specific
            .as_ref()
            .or(group.as_ref())
            .or(self.all.as_ref())
            .map(String::as_str)
    }
}

/// Color multiplied into a block's grayscale textures.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize)]
pub enum Tint {
    #[default]
    None,
    /// Biome grass color on the top face only (grass block; its side texture is pre-colored).
    GrassTop,
    /// Biome grass color on every face (tall grass, ferns).
    Grass,
    /// Biome foliage color on every face (leaves, vines).
    Foliage,
    /// A fixed 0xRRGGBB color on every face (e.g. lily pads).
    Fixed(u32),
}

impl Tint {
    /// sRGB tint for `face`, white where the tint doesn't apply.
    /// Biomes arrive in M3; until then every biome uses plains colors.
    pub fn color(self, face: Face) -> [f32; 3] {
        let rgb = match self {
            Tint::None => return [1.0; 3],
            Tint::GrassTop if face != Face::Top => return [1.0; 3],
            Tint::GrassTop | Tint::Grass => config::PLAINS_GRASS_COLOR,
            Tint::Foliage => config::PLAINS_FOLIAGE_COLOR,
            Tint::Fixed(rgb) => rgb,
        };
        [rgb >> 16, rgb >> 8, rgb].map(|c| (c & 0xFF) as f32 / 255.0)
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlockDef {
    pub id: BlockId,
    pub name: String,
    /// `None` for blocks that draw nothing (air, moving piston).
    #[serde(default)]
    pub textures: Option<Textures>,
    #[serde(default)]
    pub tint: Tint,
}

/// All block definitions, indexed by ID.
pub struct BlockRegistry {
    blocks: Vec<Option<BlockDef>>,
}

impl BlockRegistry {
    pub fn load(path: &Path) -> Result<Self> {
        let text =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        Self::from_ron(&text).with_context(|| format!("parsing {}", path.display()))
    }

    pub fn from_ron(text: &str) -> Result<Self> {
        let defs: Vec<BlockDef> = ron::from_str(text)?;
        let mut blocks: Vec<Option<BlockDef>> = Vec::new();
        for def in defs {
            let index = def.id as usize;
            if blocks.len() <= index {
                blocks.resize(index + 1, None);
            }
            if let Some(existing) = &blocks[index] {
                bail!(
                    "block ID {} used by both {} and {}",
                    def.id,
                    existing.name,
                    def.name
                );
            }
            blocks[index] = Some(def);
        }
        Ok(Self { blocks })
    }

    pub fn get(&self, id: BlockId) -> Option<&BlockDef> {
        self.blocks.get(id as usize)?.as_ref()
    }

    pub fn iter(&self) -> impl Iterator<Item = &BlockDef> {
        self.blocks.iter().flatten()
    }

    /// Texture names referenced by some block that `exists` rejects.
    pub fn missing_textures(&self, exists: impl Fn(&str) -> bool) -> BTreeSet<String> {
        self.iter()
            .filter_map(|def| def.textures.as_ref())
            .flat_map(|textures| Face::ALL.map(|face| textures.get(face)))
            .flatten()
            .filter(|name| !exists(name))
            .map(str::to_owned)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn most_specific_texture_wins() {
        let textures = Textures {
            all: Some("all".into()),
            side: Some("side".into()),
            top: Some("top".into()),
            south: Some("front".into()),
            ..Default::default()
        };
        assert_eq!(textures.get(Face::Top), Some("top"));
        assert_eq!(textures.get(Face::Bottom), Some("all"));
        assert_eq!(textures.get(Face::South), Some("front"));
        assert_eq!(textures.get(Face::East), Some("side"));
    }

    #[test]
    fn grass_top_tints_only_the_top() {
        assert_ne!(Tint::GrassTop.color(Face::Top), [1.0; 3]);
        assert_eq!(Tint::GrassTop.color(Face::North), [1.0; 3]);
        assert_eq!(
            Tint::Fixed(0xFF8000).color(Face::East),
            [1.0, 128.0 / 255.0, 0.0]
        );
    }

    #[test]
    fn duplicate_ids_are_rejected() {
        let ron = r#"[(id: 1, name: "a"), (id: 1, name: "b")]"#;
        assert!(BlockRegistry::from_ron(ron).is_err());
    }

    fn real_registry() -> BlockRegistry {
        BlockRegistry::load(&crate::assets::dir().join("data/blocks.ron")).unwrap()
    }

    #[test]
    fn every_1_0_0_block_id_is_defined() {
        let registry = real_registry();
        for id in 0..=122 {
            assert!(
                registry.get(id).is_some(),
                "block {id} missing from blocks.ron"
            );
        }
        assert_eq!(registry.iter().count(), 123);
        assert_eq!(registry.get(2).unwrap().name, "grass");
    }

    /// Keep in sync with the missing-texture list in docs/DECISIONS.md.
    #[test]
    fn only_known_block_textures_are_missing() {
        let dir = crate::assets::dir().join("textures/blocks");
        let missing =
            real_registry().missing_textures(|name| dir.join(format!("{name}.png")).is_file());
        let expected = [
            "chest_front",
            "chest_side",
            "chest_top",
            "end_portal",
            "sign",
        ];
        assert_eq!(missing, expected.map(String::from).into());
    }
}
