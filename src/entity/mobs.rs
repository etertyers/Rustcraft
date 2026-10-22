//! Mob definitions loaded from `assets/data/mobs.ron`: 1.0.0's stats,
//! drops, spawning rules and box models.

use std::path::Path;

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::{
    inventory::item::ItemId,
    world::{biome::Biome, block::BlockId},
};

/// Index of a mob kind in the registry.
pub type MobKind = usize;

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MobDef {
    pub name: String,
    /// Its texture's resource location (`entity/pig/temperate_pig`).
    pub texture: String,
    pub health: i32,
    pub width: f64,
    pub height: f64,
    /// Walking speed as a fraction of the player's (1.0.0's `moveSpeed`).
    pub speed: f64,
    pub ai: Ai,
    #[serde(default)]
    pub burns_in_daylight: bool,
    /// Walking speed while angry, if it differs (an enderman's 0.45 against
    /// 0.3 calm, in this table's units).
    #[serde(default)]
    pub angry_speed: Option<f64>,
    /// Teleports away from a projectile instead of being hit (endermen).
    #[serde(default)]
    pub dodges: bool,
    /// What it holds in its main hand when it spawns (a skeleton's bow).
    #[serde(default)]
    pub holds: Option<ItemId>,
    /// How far it notices a player it can see, and how far its paths
    /// reach (modern `FOLLOW_RANGE`: 35 for the zombie family, else 16).
    #[serde(default = "follow_range")]
    pub follow_range: f64,
    #[serde(default)]
    pub drops: Vec<MobDrop>,
    pub spawn: Spawn,
    pub model: Model,
    /// Other textures one of these mobs may wear instead (cat and horse
    /// coats, a wolf's tame and angry faces), picked by the mob's variant.
    #[serde(default)]
    pub variants: Vec<String>,
    /// Further models drawn over it, each with its own texture (a sheep's
    /// wool).
    #[serde(default)]
    pub layers: Vec<Layer>,
    /// The model is drawn at this size.
    #[serde(default = "one")]
    pub scale: f32,
    /// Climbs walls it walks into (spiders).
    #[serde(default)]
    pub climbs: bool,
    /// Lives in water: swims in all three directions and cannot breathe
    /// out of it.
    #[serde(default)]
    pub swims: bool,
    /// Flies (bees, phantoms).
    #[serde(default)]
    pub flies: bool,
    /// Takes damage from water and rain (endermen).
    #[serde(default)]
    pub hurt_by_water: bool,
    /// Takes no fall damage.
    #[serde(default)]
    pub no_fall: bool,
    /// Fire and lava don't hurt it (the Nether's mobs, M24).
    #[serde(default)]
    pub fire_immune: bool,
    /// Walks on lava as on ground (striders).
    #[serde(default)]
    pub walks_on_lava: bool,
    /// Eye height as a fraction of its height.
    #[serde(default = "eye_fraction")]
    pub eye: f64,
}

fn follow_range() -> f64 {
    16.0
}

fn one() -> f32 {
    1.0
}

fn eye_fraction() -> f64 {
    0.85
}

/// A second model drawn over a mob's body.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Layer {
    pub texture: String,
    /// Its own model, or (with `like_body`) the body's, grown.
    #[serde(default)]
    pub model: Model,
    /// Drawn with the body's model, every box grown by this many pixels
    /// (horse markings, saddle and armor, laid out like the body).
    #[serde(default)]
    pub like_body: Option<f32>,
    /// Only drawn on a horse with these markings (1–4) or this armor (1–4).
    #[serde(default)]
    pub only_marking: Option<u8>,
    #[serde(default)]
    pub only_armor: Option<u8>,
    /// Whether it takes the mob's color (a dyed sheep's wool).
    #[serde(default)]
    pub dyed: bool,
    /// Hidden when the mob is sheared.
    #[serde(default)]
    pub wool: bool,
    /// Glows (an enderman's eyes) rather than being lit by the world.
    #[serde(default)]
    pub glow: bool,
    /// Only drawn while the mob has this variant (a tamed wolf's collar).
    #[serde(default)]
    pub only_variant: Option<u8>,
    /// Only drawn while the mob is saddled (a strider's saddle).
    #[serde(default)]
    pub saddle: bool,
}

/// How a mob decides what to do (1.0.0's `EntityCreature` family, and
/// modern Minecraft's goals for the later ones).
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
pub enum Ai {
    /// Wanders, and runs when hurt (1.0.0's `EntityAnimal`).
    Passive,
    /// Chases and hits the player (1.0.0's `EntityMob`).
    Hostile { attack: f32 },
    /// Leaves the player alone until hit, then chases and bites; golems
    /// go after monsters too.
    Neutral { attack: f32 },
    /// Hostile in the dark, neutral in the light; leaps (spiders).
    Spider { attack: f32 },
    /// Keeps its distance and shoots arrows (skeletons).
    Archer { interval: u32 },
    /// Swells beside the player and blows up (creepers).
    Creeper { power: f32 },
    /// Angered by being looked at; teleports and carries blocks.
    Enderman { attack: f32 },
    /// Hops after the player (slimes).
    Slime,
    /// Keeps away and throws potions (witches).
    Witch,
    /// Circles overhead and dives (phantoms).
    Phantom { attack: f32 },
    /// Swims about in water (squid, fish).
    Swimmer,
    /// Tameable; hostile to what it's angry at, follows its owner.
    Wolf { attack: f32 },
    /// Tameable; sits and follows; scares creepers.
    Cat,
    /// Rideable once saddled; gentle until tamed.
    Horse,
    /// Hops about.
    Rabbit,
    /// Flies about flowers, stings once when angered.
    Bee { attack: f32 },
    /// Wanders at dusk and hunts small animals.
    Fox { attack: f32 },
    /// Wanders and keeps to the village.
    Villager,
    /// Floats about and shoots fireballs at the player (ghasts).
    Ghast,
    /// Neutral until hurt, then angry with every zombified piglin around.
    ZombifiedPiglin { attack: f32 },
    /// Hostile unless the player wears gold; takes gold to barter.
    Piglin { attack: f32 },
    /// Hostile; tosses what it hits into the air.
    Hoglin { attack: f32 },
    /// Walks the lava; rideable once saddled, steered with a warped
    /// fungus on a stick.
    Strider,
    /// Hovers near the player, burns up and shoots three small fireballs
    /// (M25).
    Blaze,
    /// Sits in its shell on the spot, opens up with the player in sight and
    /// shoots homing bullets that lift whoever they hit (M28).
    Shulker,
}

impl Ai {
    /// Mobs that stay in the world when the player is far away, are saved
    /// and don't count against the monster cap.
    pub fn is_creature(self) -> bool {
        !self.is_monster()
    }

    /// Walkers that go after the player within their `follow_range`.
    pub fn has_follow_range(self) -> bool {
        matches!(
            self,
            Ai::Hostile { .. }
                | Ai::Spider { .. }
                | Ai::Archer { .. }
                | Ai::Creeper { .. }
                | Ai::ZombifiedPiglin { .. }
                | Ai::Hoglin { .. }
        )
    }

    /// Mobs that spawn in the dark and despawn.
    pub fn is_monster(self) -> bool {
        matches!(
            self,
            Ai::Hostile { .. }
                | Ai::Spider { .. }
                | Ai::Archer { .. }
                | Ai::Creeper { .. }
                | Ai::Enderman { .. }
                | Ai::Slime
                | Ai::Witch
                | Ai::Phantom { .. }
                | Ai::Ghast
                | Ai::ZombifiedPiglin { .. }
                | Ai::Piglin { .. }
                | Ai::Hoglin { .. }
                | Ai::Blaze
                | Ai::Shulker
        )
    }
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
    /// The chance it drops at all, 0–1 (default always).
    #[serde(default = "one")]
    pub chance: f32,
}

/// Where a mob spawns.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub enum Spawn {
    /// Never by itself (spawned by other means).
    Never,
    /// At world generation, in groups on these ground blocks in the light,
    /// in a `chance` of new chunks, in these biomes (all if empty).
    Ground {
        blocks: Vec<BlockId>,
        min_light: u8,
        chance: f32,
        group: u8,
        /// Its share of the biome's animals, against the other kinds.
        #[serde(default = "hundred")]
        weight: u32,
        #[serde(default)]
        biomes: Vec<Biome>,
    },
    /// Near the player wherever it's dark enough (1.0.0's monster
    /// spawning), in these biomes (all if empty); `weight` shares out the
    /// cap among kinds.
    Dark {
        max_light: u8,
        group: u8,
        #[serde(default)]
        biomes: Vec<Biome>,
        #[serde(default = "hundred")]
        weight: u32,
        /// Nether biomes it spawns in too, each with a weight and group
        /// (skeletons in soul sand valleys, endermen); `biomes` are only
        /// ever the Overworld's.
        #[serde(default)]
        nether: Vec<(Biome, u32, u8)>,
    },
    /// Near the player in the Nether wherever it's dark enough, in these
    /// biomes, each with a weight and group (M24).
    Nether {
        max_light: u8,
        biomes: Vec<(Biome, u32, u8)>,
    },
    /// On the lava sea near the player, in these biomes (striders).
    Lava { group: u8, biomes: Vec<Biome> },
    /// Only inside Nether fortresses, by weight (blazes, wither
    /// skeletons; M25).
    Fortress { weight: u32, group: u8 },
    /// In water at generation and near the player, in these biomes.
    Water {
        chance: f32,
        group: u8,
        #[serde(default)]
        biomes: Vec<Biome>,
        /// Needs the open sky above (squid) or any depth (fish).
        #[serde(default)]
        min_depth: u8,
    },
    /// In dark places deep underground in "slime chunks" (a tenth of
    /// chunks picked by the world seed), below this height.
    SlimeChunk { below: i32 },
    /// In the open sky above a player who has not slept for three days.
    Phantom,
}

fn hundred() -> u32 {
    100
}

/// A box model in Minecraft's model coordinates: pixels, y pointing down,
/// the feet at y 24.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
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
    /// Resting rotation around y and z.
    #[serde(default)]
    pub rest_y: f32,
    #[serde(default)]
    pub rest_z: f32,
    #[serde(default)]
    pub role: Role,
    #[serde(default)]
    pub mirror: bool,
    /// More boxes moving with this part (the pig's snout).
    #[serde(default)]
    pub extra: Vec<PartBox>,
    /// Pixels every box of the part is grown on each side (a layer drawn
    /// over a body, a robe).
    #[serde(default)]
    pub grow: f32,
    /// What the mob holds in its main hand is drawn at the bottom of this
    /// part (its right arm).
    #[serde(default)]
    pub hand: bool,
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
    /// Wings beating with the mob's age (chickens, phantoms, bees); the
    /// right one is mirrored.
    WingA,
    WingB,
    /// A shulker's lid: raised while it's open (M28).
    Lid,
    /// One of a spider's eight legs, 0–7.
    SpiderLeg(u8),
    /// One of a squid's eight tentacles, 0–7.
    Tentacle(u8),
    /// A wolf's tail, wagging.
    Tail,
    /// Held raised while the mob is attacking (skeleton arms with a bow,
    /// witch nose twitching): drawn up when the mob is aggressive.
    Aim,
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
        let mut mobs: Vec<MobDef> =
            ron::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
        for mob in &mut mobs {
            for layer in &mut mob.layers {
                if let Some(grow) = layer.like_body {
                    layer.model = Model {
                        texture_size: mob.model.texture_size,
                        parts: mob
                            .model
                            .parts
                            .iter()
                            .map(|part| Part {
                                grow: part.grow + grow,
                                hand: false,
                                ..part.clone()
                            })
                            .collect(),
                    };
                }
            }
        }
        Ok(Self { mobs })
    }

    pub fn get(&self, kind: MobKind) -> &MobDef {
        &self.mobs[kind]
    }

    /// Every mob's name.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.mobs.iter().map(|m| m.name.as_str())
    }

    pub fn find(&self, name: &str) -> Option<MobKind> {
        self.mobs.iter().position(|mob| mob.name == name)
    }

    pub fn iter(&self) -> impl Iterator<Item = (MobKind, &MobDef)> {
        self.mobs.iter().enumerate()
    }

    /// Every texture the mobs use, in the order the renderer binds them:
    /// each kind's own, then its variants, then its layers'.
    pub fn textures(&self) -> Vec<&str> {
        let mut names = Vec::new();
        for mob in &self.mobs {
            names.push(mob.texture.as_str());
            names.extend(mob.variants.iter().map(String::as_str));
            names.extend(mob.layers.iter().map(|l| l.texture.as_str()));
        }
        names
    }

    /// `textures`, each with the texture size its model is built for.
    pub fn textures_sized(&self) -> Vec<(&str, (u32, u32))> {
        let size = |model: &Model| (model.texture_size.0 as u32, model.texture_size.1 as u32);
        let mut names = Vec::new();
        for mob in &self.mobs {
            names.push((mob.texture.as_str(), size(&mob.model)));
            names.extend(mob.variants.iter().map(|v| (v.as_str(), size(&mob.model))));
            names.extend(
                mob.layers
                    .iter()
                    .map(|l| (l.texture.as_str(), size(&l.model))),
            );
        }
        names
    }

    /// How many textures `textures` lists: the arrow's is the next slot.
    pub fn texture_count(&self) -> usize {
        self.mobs
            .iter()
            .map(|m| 1 + m.variants.len() + m.layers.len())
            .sum()
    }

    /// The renderer's slot for `kind`'s texture with variant `variant`
    /// (0 is the plain one).
    pub fn texture_slot(&self, kind: MobKind, variant: u8) -> usize {
        let before: usize = self.mobs[..kind]
            .iter()
            .map(|m| 1 + m.variants.len() + m.layers.len())
            .sum();
        let own = &self.mobs[kind];
        before + (variant as usize).min(own.variants.len())
    }

    /// The slot of layer `layer` of `kind`.
    pub fn layer_slot(&self, kind: MobKind, layer: usize) -> usize {
        let before: usize = self.mobs[..kind]
            .iter()
            .map(|m| 1 + m.variants.len() + m.layers.len())
            .sum();
        before + 1 + self.mobs[kind].variants.len() + layer
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Mobs whose textures Kenzo's 1.12-era textures don't have; a resource
    /// pack brings them.
    const NO_BASE_TEXTURE: [&str; 11] = [
        "shulker",
        "bee",
        "fox",
        "cod",
        "salmon",
        "phantom",
        "zombified_piglin",
        "piglin",
        "hoglin",
        "strider",
        "piglin_brute",
    ];

    #[test]
    fn mobs_load_with_modern_values() {
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
        assert_eq!(zombie.ai, Ai::Hostile { attack: 3.0 });
        assert!(zombie.burns_in_daylight);
        assert!(matches!(
            zombie.spawn,
            Spawn::Dark {
                max_light: 7,
                group: 4,
                ..
            }
        ));
        assert_eq!(zombie.height, 1.95);
        let creeper = mobs.get(mobs.find("creeper").unwrap());
        assert_eq!(creeper.ai, Ai::Creeper { power: 3.0 });
        let names: Vec<_> = mobs.iter().map(|(_, m)| m.name.as_str()).collect();
        for want in [
            "skeleton",
            "creeper",
            "spider",
            "cave_spider",
            "enderman",
            "slime",
            "witch",
            "drowned",
            "husk",
            "stray",
            "phantom",
            "cow",
            "sheep",
            "chicken",
            "wolf",
            "cat",
            "horse",
            "rabbit",
            "squid",
            "cod",
            "salmon",
            "bee",
            "fox",
            "villager",
            "iron_golem",
        ] {
            assert!(names.contains(&want), "missing {want}");
        }
        let mut unique = names.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), names.len(), "names are unique");
    }

    #[test]
    fn every_texture_the_mobs_use_is_there_but_the_few_a_pack_must_bring() {
        let mobs = MobRegistry::load(&crate::assets::dir().join("data/mobs.ron")).unwrap();
        let base =
            crate::pack::Resources::load_with(&crate::assets::dir(), Vec::new(), false).unwrap();
        for (_, mob) in mobs.iter() {
            let own = std::iter::once(&mob.texture)
                .chain(&mob.variants)
                .chain(mob.layers.iter().map(|l| &l.texture));
            for texture in own {
                // Armor and saddles of the newer layout come from the
                // vanilla fallback only.
                if texture.starts_with("entity/equipment/") {
                    continue;
                }
                let missing = base.texture(texture).is_none();
                assert_eq!(
                    missing,
                    NO_BASE_TEXTURE.contains(&mob.name.as_str()),
                    "{} texture {texture}",
                    mob.name
                );
            }
        }
    }

    #[test]
    fn texture_slots_follow_the_order_of_the_list() {
        let mobs = MobRegistry::load(&crate::assets::dir().join("data/mobs.ron")).unwrap();
        let names = mobs.textures();
        assert_eq!(names.len(), mobs.texture_count());
        for (kind, mob) in mobs.iter() {
            assert_eq!(names[mobs.texture_slot(kind, 0)], mob.texture);
            for (v, variant) in mob.variants.iter().enumerate() {
                assert_eq!(names[mobs.texture_slot(kind, v as u8 + 1)], variant);
            }
            for (l, layer) in mob.layers.iter().enumerate() {
                assert_eq!(names[mobs.layer_slot(kind, l)], layer.texture);
            }
        }
    }

    #[test]
    fn the_horse_model_fits_vanillas_64_by_64_layout_and_its_layers_grow_on_the_body() {
        let mobs = MobRegistry::load(&crate::assets::dir().join("data/mobs.ron")).unwrap();
        let horse = mobs.get(mobs.find("horse").unwrap());
        let (width, height) = horse.model.texture_size;
        assert_eq!((width, height), (64.0, 64.0));
        for part in &horse.model.parts {
            for (uv, size) in std::iter::once((part.uv, part.size))
                .chain(part.extra.iter().map(|b| (b.uv, b.size)))
            {
                let right = uv.0 + 2.0 * (size.0 + size.2);
                let bottom = uv.1 + size.2 + size.1;
                assert!(right <= width && bottom <= height, "{uv:?} {size:?}");
            }
        }
        // Markings (1–4), armor (1–4) and the saddle are the body's boxes,
        // grown, with their own textures.
        let marks: Vec<_> = horse.layers.iter().filter_map(|l| l.only_marking).collect();
        let armor: Vec<_> = horse.layers.iter().filter_map(|l| l.only_armor).collect();
        assert_eq!((marks, armor), (vec![1, 2, 3, 4], vec![1, 2, 3, 4]));
        assert!(horse.layers.iter().any(|l| l.saddle));
        for layer in &horse.layers {
            assert_eq!(layer.model.parts.len(), horse.model.parts.len());
            assert_eq!(layer.model.texture_size, horse.model.texture_size);
            assert!(layer.model.parts.iter().all(|p| p.grow > 0.0));
        }
    }
}
