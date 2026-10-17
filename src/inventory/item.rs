//! Item definitions loaded from `assets/data/items.ron`.
//!
//! IDs are Minecraft 1.0.0's numeric item IDs (`docs/blocks-and-items.md`).

use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

use anyhow::{Context, Result, bail};
use serde::Deserialize;

use crate::world::block::BlockId;

pub type ItemId = u16;

/// Items with IDs below this are blocks (1.0.0's `ItemBlock`s).
pub const FIRST_ITEM_ID: ItemId = 256;

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ItemDef {
    pub id: ItemId,
    pub name: String,
    /// File stem in `assets/textures/items/`.
    pub texture: String,
    #[serde(default = "default_max_stack")]
    pub max_stack: u8,
    /// Uses before breaking, for items without a tool tier.
    #[serde(default)]
    pub durability: Option<u16>,
    #[serde(default)]
    pub tool: Option<Tool>,
    /// Block placed by right-clicking with this item.
    #[serde(default)]
    pub places: Option<BlockId>,
    #[serde(default)]
    pub variants: BTreeMap<u8, ItemVariant>,
    #[serde(default)]
    pub food: Option<Food>,
    /// Item left in the hand after using this one (stew leaves a bowl).
    #[serde(default)]
    pub leftover: Option<ItemId>,
    /// Ammunition this shoots, drawn by holding right-click (a bow).
    #[serde(default)]
    pub shoots: Option<ItemId>,
}

/// What eating restores (1.0.0's `ItemFood`).
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Food {
    pub hunger: i32,
    /// Saturation gained is hunger × this × 2.
    pub saturation: f32,
    /// Can be eaten with a full hunger bar (golden apples).
    #[serde(default)]
    pub always_edible: bool,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ItemVariant {
    pub name: String,
    pub texture: String,
}

/// How an item mines blocks.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Tool {
    pub kind: ToolKind,
    #[serde(default)]
    pub tier: Option<Tier>,
    /// Mining speed on blocks the tool isn't made for.
    #[serde(default = "default_base_speed")]
    pub base_speed: f32,
    /// Mining speed on particular blocks, overriding everything else.
    #[serde(default)]
    pub speeds: BTreeMap<BlockId, f32>,
    /// Durability lost per block broken.
    #[serde(default = "default_wear")]
    pub wear: u16,
    /// Damage dealt hitting a mob (1.0.0's `getDamageVsEntity`).
    #[serde(default = "default_attack")]
    pub attack: i32,
    /// Durability lost per hit (1.0.0: swords 1, other tools 2).
    #[serde(default = "default_hit_wear")]
    pub hit_wear: u16,
}

fn default_attack() -> i32 {
    1
}

fn default_hit_wear() -> u16 {
    2
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub enum ToolKind {
    Pickaxe,
    Shovel,
    Axe,
    Sword,
    Hoe,
    Shears,
}

/// Tool materials, with 1.0.0's `EnumToolMaterial` values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
pub enum Tier {
    Wood,
    Gold,
    Stone,
    Iron,
    Diamond,
}

impl Tier {
    /// Mining speed on blocks the tool is made for.
    pub fn efficiency(self) -> f32 {
        match self {
            Tier::Wood => 2.0,
            Tier::Stone => 4.0,
            Tier::Iron => 6.0,
            Tier::Diamond => 8.0,
            Tier::Gold => 12.0,
        }
    }

    /// Harvest level: what the tool can mine (gold mines like wood).
    pub fn level(self) -> u8 {
        match self {
            Tier::Wood | Tier::Gold => 0,
            Tier::Stone => 1,
            Tier::Iron => 2,
            Tier::Diamond => 3,
        }
    }

    pub fn durability(self) -> u16 {
        match self {
            Tier::Wood => 59,
            Tier::Stone => 131,
            Tier::Iron => 250,
            Tier::Diamond => 1561,
            Tier::Gold => 32,
        }
    }
}

fn default_max_stack() -> u8 {
    64
}

fn default_base_speed() -> f32 {
    1.0
}

fn default_wear() -> u16 {
    1
}

impl ItemDef {
    /// Uses before breaking, if the item wears out.
    pub fn max_damage(&self) -> Option<u16> {
        self.durability
            .or_else(|| self.tool.as_ref()?.tier.map(Tier::durability))
    }

    /// Name and texture for a data value (dye color, charcoal).
    pub fn variant(&self, data: u8) -> (&str, &str) {
        self.variants
            .get(&data)
            .map_or((&self.name, &self.texture), |v| (&v.name, &v.texture))
    }
}

pub struct ItemRegistry {
    items: Vec<ItemDef>,
}

impl ItemRegistry {
    pub fn load(path: &Path) -> Result<Self> {
        let text =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        Self::from_ron(&text).with_context(|| format!("parsing {}", path.display()))
    }

    pub fn from_ron(text: &str) -> Result<Self> {
        let mut items: Vec<ItemDef> = ron::from_str(text)?;
        items.sort_by_key(|item| item.id);
        if let Some(pair) = items.windows(2).find(|pair| pair[0].id == pair[1].id) {
            bail!(
                "item ID {} used by both {} and {}",
                pair[0].id,
                pair[0].name,
                pair[1].name
            );
        }
        Ok(Self { items })
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn get(&self, id: ItemId) -> Option<&ItemDef> {
        let index = self.items.binary_search_by_key(&id, |item| item.id).ok()?;
        Some(&self.items[index])
    }

    pub fn iter(&self) -> impl Iterator<Item = &ItemDef> {
        self.items.iter()
    }

    /// Texture names referenced by some item that `exists` rejects.
    pub fn missing_textures(&self, exists: impl Fn(&str) -> bool) -> BTreeSet<String> {
        self.items
            .iter()
            .flat_map(|item| {
                std::iter::once(item.texture.as_str())
                    .chain(item.variants.values().map(|v| v.texture.as_str()))
            })
            .filter(|name| !exists(name))
            .map(str::to_owned)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn real_registry() -> ItemRegistry {
        ItemRegistry::load(&crate::assets::dir().join("data/items.ron")).unwrap()
    }

    #[test]
    fn every_1_0_0_item_id_is_defined() {
        let registry = real_registry();
        for id in (256..=382).chain(2256..=2266) {
            assert!(
                registry.get(id).is_some(),
                "item {id} missing from items.ron"
            );
        }
        assert_eq!(registry.len(), 127 + 11);
        assert_eq!(registry.get(278).unwrap().name, "diamond_pickaxe");
    }

    #[test]
    fn every_item_texture_exists() {
        let dir = crate::assets::dir().join("textures/items");
        let missing =
            real_registry().missing_textures(|name| dir.join(format!("{name}.png")).is_file());
        assert!(missing.is_empty(), "missing item textures: {missing:?}");
    }

    #[test]
    fn stacks_tools_and_durability_follow_1_0_0() {
        let registry = real_registry();
        let item = |id| registry.get(id).unwrap();
        assert_eq!(item(280).max_stack, 64, "stick");
        assert_eq!(item(332).max_stack, 16, "snowball");
        assert_eq!(item(325).max_stack, 1, "bucket");
        assert_eq!(item(270).max_damage(), Some(59), "wooden pickaxe");
        assert_eq!(item(278).max_damage(), Some(1561), "diamond pickaxe");
        assert_eq!(item(284).max_damage(), Some(32), "golden shovel");
        assert_eq!(item(359).max_damage(), Some(238), "shears");
        assert_eq!(item(311).max_damage(), Some(528), "diamond chestplate");
        assert_eq!(item(280).max_damage(), None);
        let sword = item(276).tool.as_ref().unwrap();
        assert_eq!(
            (sword.kind, sword.base_speed, sword.wear),
            (ToolKind::Sword, 1.5, 2)
        );
        assert_eq!(item(274).tool.as_ref().unwrap().tier, Some(Tier::Stone));
        assert_eq!(item(351).variant(4), ("lapis_lazuli", "dye_powder_blue"));
        assert_eq!(item(263).variant(1).0, "charcoal");
        assert_eq!(item(295).places, Some(59), "seeds plant wheat");
    }

    #[test]
    fn duplicate_ids_are_rejected() {
        let ron = r#"[(id: 256, name: "a", texture: "a"), (id: 256, name: "b", texture: "b")]"#;
        assert!(ItemRegistry::from_ron(ron).is_err());
    }
}
