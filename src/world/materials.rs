//! Material classes (M54): what the Trailer look's shaders treat each
//! block state as, from `assets/data/materials.ron` (never from match
//! arms). Packed into every vertex with the face normal
//! (`render::vertex::pack_face`).

use std::sync::LazyLock;

use serde::Deserialize;

use super::block::{BlockState, Condition, registry};

/// A block's material class. The numbers are what the shaders read.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[repr(u8)]
pub enum Class {
    #[default]
    Opaque = 0,
    CutoutLeaves = 1,
    /// Grass, flowers, ferns, saplings.
    CutoutPlant = 2,
    Crops = 3,
    /// Vines and hanging plants.
    Vines = 4,
    Water = 5,
    Lava = 6,
    Ice = 7,
    /// Glass and stained glass.
    Glass = 8,
    Emissive = 9,
    /// Metal-ish blocks that shine a little.
    Metal = 10,
    /// Mobs, players, dropped items and the hand (not from the file).
    Entity = 11,
}

impl Class {
    pub const ALL: [Class; 12] = [
        Class::Opaque,
        Class::CutoutLeaves,
        Class::CutoutPlant,
        Class::Crops,
        Class::Vines,
        Class::Water,
        Class::Lava,
        Class::Ice,
        Class::Glass,
        Class::Emissive,
        Class::Metal,
        Class::Entity,
    ];
}

/// A block state's class and glow (0–15).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Material {
    pub class: Class,
    pub glow: u8,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Rule {
    blocks: Vec<String>,
    #[serde(default)]
    when: String,
    class: Class,
    #[serde(default)]
    glow: u8,
}

const FILE: &str = include_str!("../../assets/data/materials.ron");

/// Whether `pattern` (a name, `#acts_as`, or `*` at either end) covers
/// block `name` acting as `kind`.
fn covers(pattern: &str, name: &str, kind: &str) -> bool {
    if let Some(of) = pattern.strip_prefix('#') {
        return kind == of || name == of;
    }
    match (pattern.strip_prefix('*'), pattern.strip_suffix('*')) {
        (Some(suffix), _) => name.ends_with(suffix),
        (_, Some(prefix)) => name.starts_with(prefix),
        _ => name == pattern,
    }
}

/// Every state's material, indexed by `BlockRegistry::state_index`.
static TABLE: LazyLock<Vec<Material>> = LazyLock::new(|| {
    let rules: Vec<Rule> = ron::from_str(FILE).expect("assets/data/materials.ron");
    let blocks = registry();
    let mut table = vec![Material::default(); blocks.state_count()];
    // Each rule's condition, parsed per block (properties differ).
    for state in blocks.states() {
        let id = state.id();
        let (name, kind) = (id.name(), state.kind().name());
        let found = rules.iter().find(|rule| {
            rule.blocks.iter().any(|p| covers(p, name, kind))
                && (rule.when.is_empty()
                    || Condition::parse(&rule.when, blocks.layout(id), name)
                        .is_ok_and(|c| state.matches(&c)))
        });
        if let Some(rule) = found {
            table[blocks.state_index(state)] = Material {
                class: rule.class,
                glow: rule.glow.min(15),
            };
        }
    }
    table
});

/// The material of a block state.
pub fn of(state: BlockState) -> Material {
    TABLE
        .get(registry().state_index(state))
        .copied()
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::{blocks::*, props::Prop};

    #[test]
    fn classes_come_from_the_file() {
        let class = |id| of(BlockState::of(id)).class;
        assert_eq!(class(STONE), Class::Opaque);
        assert_eq!(class(OAK_LEAVES), Class::CutoutLeaves);
        assert_eq!(class(BIRCH_LEAVES), Class::CutoutLeaves);
        assert_eq!(class(SHORT_GRASS), Class::CutoutPlant);
        assert_eq!(class(RED_TULIP), Class::CutoutPlant);
        assert_eq!(class(WHEAT), Class::Crops);
        assert_eq!(class(VINE), Class::Vines);
        assert_eq!(class(WATER), Class::Water);
        assert_eq!(class(ICE), Class::Ice);
        assert_eq!(class(GLASS), Class::Glass);
        assert_eq!(class(GOLD_BLOCK), Class::Metal);
        assert_eq!(
            of(BlockState::of(LAVA)),
            Material {
                class: Class::Lava,
                glow: 15
            }
        );
        assert_eq!(of(BlockState::of(GLOWSTONE)).glow, 15);
        // Lit only when lit.
        let lamp = BlockState::of(REDSTONE_LAMP);
        assert_eq!(class(REDSTONE_LAMP), Class::Opaque);
        assert_eq!(of(lamp.with_bool(Prop::Lit, true)).class, Class::Emissive);
    }

    #[test]
    fn patterns() {
        assert!(covers("*_leaves", "jungle_leaves", "oak_leaves"));
        assert!(covers("#poppy", "allium", "poppy"));
        assert!(covers("cut_*", "cut_copper", "cut_copper"));
        assert!(!covers("glass", "glass_pane", "glass_pane"));
    }
}
