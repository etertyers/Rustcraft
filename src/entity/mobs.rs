//! Mob definitions loaded from `assets/data/mobs.ron`: 1.0.0's stats,
//! drops, spawning rules and box models.

use std::path::Path;

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::{inventory::item::ItemId, world::block::BlockId};

/// Index of a mob kind in the registry.
pub type MobKind = usize;

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MobDef {
    pub name: String,
    /// Under `assets/textures/entity/`.
    pub texture: String,
    pub health: i32,
    pub width: f64,
    pub height: f64,
    /// Walking speed as a fraction of the player's (1.0.0's `moveSpeed`).
    pub speed: f64,
    pub ai: Ai,
    #[serde(default)]
    pub burns_in_daylight: bool,
    #[serde(default)]
    pub drops: Vec<MobDrop>,
    pub spawn: Spawn,
    pub model: Model,
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
pub enum Ai {
    /// Wanders, and runs when hurt (1.0.0's `EntityAnimal`).
    Passive,
    /// Chases and hits the player (1.0.0's `EntityMob`).
    Hostile { attack: i32 },
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MobDrop {
    pub item: ItemId,
    pub min: u8,
    pub max: u8,
    /// Dropped instead while the mob is burning (cooked porkchop).
    #[serde(default)]
    pub cooked: Option<ItemId>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub enum Spawn {
    /// At world generation, in groups on these ground blocks in the light,
    /// in a `chance` of new chunks.
    Ground {
        blocks: Vec<BlockId>,
        min_light: u8,
        chance: f32,
        group: u8,
    },
    /// Near the player wherever it's dark enough (1.0.0's monster spawning).
    Dark { max_light: u8, group: u8 },
}

/// A box model in Minecraft's model coordinates: pixels, y pointing down,
/// the feet at y 24.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Model {
    pub texture_size: (f32, f32),
    pub parts: Vec<Part>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Part {
    pub uv: (f32, f32),
    pub from: (f32, f32, f32),
    pub size: (f32, f32, f32),
    pub pivot: (f32, f32, f32),
    /// Resting rotation around x, in radians.
    #[serde(default)]
    pub rest: f32,
    #[serde(default)]
    pub role: Role,
    #[serde(default)]
    pub mirror: bool,
    /// More boxes moving with this part (the pig's snout).
    #[serde(default)]
    pub extra: Vec<PartBox>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PartBox {
    pub uv: (f32, f32),
    pub from: (f32, f32, f32),
    pub size: (f32, f32, f32),
}

/// How a part moves (1.0.0's `setRotationAngles`).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize)]
pub enum Role {
    #[default]
    None,
    /// Turns toward where the mob looks.
    Head,
    /// Legs swinging with walking, opposite each other.
    LegA,
    LegB,
    /// Arms swinging with walking, opposite the legs.
    ArmA,
    ArmB,
    /// Zombie arms: held forward, bobbing.
    Reaching,
}

pub struct MobRegistry {
    mobs: Vec<MobDef>,
}

impl MobRegistry {
    /// No mobs, for test worlds.
    pub fn empty() -> Self {
        Self { mobs: Vec::new() }
    }

    pub fn load(path: &Path) -> Result<Self> {
        let text =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let mobs = ron::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
        Ok(Self { mobs })
    }

    pub fn get(&self, kind: MobKind) -> &MobDef {
        &self.mobs[kind]
    }

    pub fn find(&self, name: &str) -> Option<MobKind> {
        self.mobs.iter().position(|mob| mob.name == name)
    }

    pub fn iter(&self) -> impl Iterator<Item = (MobKind, &MobDef)> {
        self.mobs.iter().enumerate()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pig_and_zombie_load_with_1_0_0_values() {
        let mobs = MobRegistry::load(&crate::assets::dir().join("data/mobs.ron")).unwrap();
        let pig = mobs.get(mobs.find("pig").unwrap());
        assert_eq!((pig.health, pig.width, pig.height), (10, 0.9, 0.9));
        assert_eq!(pig.ai, Ai::Passive);
        assert_eq!(
            pig.drops[0].cooked,
            Some(crate::inventory::items::COOKED_PORKCHOP)
        );
        assert_eq!(pig.model.parts.len(), 6);
        let zombie = mobs.get(mobs.find("zombie").unwrap());
        assert_eq!(zombie.ai, Ai::Hostile { attack: 4 });
        assert!(zombie.burns_in_daylight);
        assert_eq!(
            zombie.spawn,
            Spawn::Dark {
                max_light: 7,
                group: 4
            }
        );
        let textures = crate::assets::dir().join("textures/entity");
        for (_, mob) in mobs.iter() {
            assert!(textures.join(&mob.texture).is_file(), "{}", mob.texture);
        }
    }
}
