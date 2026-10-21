//! Item definitions: an item for every block that has one (from
//! `assets/data/blocks.ron`), then the rest from `assets/data/items.ron`.
//!
//! Items are named as in modern Java Edition and numbered by their place in
//! that order (see build.rs); saves store names.

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::LazyLock,
};

use anyhow::{Result, bail};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::world::block::{self, BlockId, Fluid};

/// An item's runtime ID. The named constants are in `inventory::items`.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ItemId(pub(crate) u16);

impl ItemId {
    pub const fn index(self) -> usize {
        self.0 as usize
    }

    /// The item's name without the namespace, e.g. `diamond_pickaxe`.
    pub fn name(self) -> &'static str {
        super::items::NAMES
            .get(self.index())
            .copied()
            .unwrap_or("unknown")
    }
}

/// Item IDs are written by name (`"stick"` or `"minecraft:stick"`).
impl<'de> Deserialize<'de> for ItemId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let name = String::deserialize(deserializer)?;
        super::items::by_name(block::strip_namespace(&name))
            .ok_or_else(|| serde::de::Error::custom(format!("unknown item {name:?}")))
    }
}

impl Serialize for ItemId {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(&format_args!("{}:{}", block::NAMESPACE, self.name()))
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ItemDef {
    /// Set from the item's place in the order.
    #[serde(skip)]
    pub id: ItemId,
    pub name: String,
    /// Texture name under `textures/item/`; `None` for block items drawn
    /// as their block.
    pub texture: Option<String>,
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
    pub food: Option<Food>,
    /// Item left in the hand after using this one (stew leaves a bowl).
    #[serde(default)]
    pub leftover: Option<ItemId>,
    /// Ammunition this shoots, drawn by holding right-click (a bow).
    #[serde(default)]
    pub shoots: Option<ItemId>,
    /// A bucket, and what it holds.
    #[serde(default)]
    pub bucket: Option<Bucket>,
}

/// What a bucket holds (1.0.0's `ItemBucket.isFull`): an empty one picks
/// up a source block, a full one pours one out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub enum Bucket {
    Empty,
    Holds(Fluid),
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
    /// Damage with a full swing, bare hands included (modern
    /// `Attributes.ATTACK_DAMAGE`: 1 plus the tool's bonus).
    #[serde(default = "default_attack")]
    pub attack: f32,
    /// Swings a second at full strength (modern `ATTACK_SPEED`: 4 bare
    /// handed and for items that aren't weapons).
    #[serde(default = "default_attack_speed")]
    pub attack_speed: f32,
    /// Durability lost per hit (1.0.0: swords 1, other tools 2).
    #[serde(default = "default_hit_wear")]
    pub hit_wear: u16,
}

fn default_attack() -> f32 {
    1.0
}

fn default_attack_speed() -> f32 {
    4.0
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

    /// The item for block `def`: placing it, stacking as the block says,
    /// and showing `item_texture` if it has one.
    fn for_block(id: ItemId, def: &block::BlockDef) -> Self {
        Self {
            id,
            name: def.name.clone(),
            texture: def.item_texture.clone(),
            max_stack: def.stack,
            durability: None,
            tool: None,
            places: Some(def.id),
            food: None,
            leftover: None,
            shoots: None,
            bucket: None,
        }
    }
}

pub struct ItemRegistry {
    items: Vec<ItemDef>,
    /// Each block's item, if it has one.
    by_block: Vec<Option<ItemId>>,
}

/// The registry for the blocks' items and `assets/data/items.ron`, built
/// into the game (its names are compiled in as constants, see build.rs).
pub fn registry() -> &'static ItemRegistry {
    static REGISTRY: LazyLock<ItemRegistry> = LazyLock::new(|| {
        ItemRegistry::from_ron(include_str!("../../assets/data/items.ron"))
            .expect("assets/data/items.ron is valid")
    });
    &REGISTRY
}

impl ItemRegistry {
    fn from_ron(text: &str) -> Result<Self> {
        let plain: Vec<ItemDef> = ron::from_str(text)?;
        let blocks = block::registry();
        let mut items = Vec::with_capacity(super::items::COUNT);
        let mut by_block = vec![None; blocks.iter().count()];
        for def in blocks.iter().filter(|def| def.item) {
            let id = ItemId(items.len() as u16);
            by_block[def.id.index()] = Some(id);
            items.push(ItemDef::for_block(id, def));
        }
        for mut item in plain {
            if item.texture.is_none() {
                bail!("item {} has no texture", item.name);
            }
            item.id = ItemId(items.len() as u16);
            items.push(item);
        }
        if items.len() != super::items::COUNT {
            bail!(
                "there are {} items but the game was built with {}",
                items.len(),
                super::items::COUNT
            );
        }
        for item in &items {
            if super::items::NAMES[item.id.index()] != item.name {
                bail!(
                    "item {} is {} but was built as {}",
                    item.id.index(),
                    item.name,
                    super::items::NAMES[item.id.index()]
                );
            }
        }
        Ok(Self { items, by_block })
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn get(&self, id: ItemId) -> Option<&ItemDef> {
        self.items.get(id.index())
    }

    /// The item of a block, if it has one (water and wall torches don't).
    pub fn of_block(&self, block: BlockId) -> Option<ItemId> {
        self.by_block.get(block.index()).copied().flatten()
    }

    pub fn iter(&self) -> impl Iterator<Item = &ItemDef> {
        self.items.iter()
    }

    /// Texture names referenced by some item that `exists` rejects.
    /// Every item texture location (`item/stick`).
    pub fn texture_locations(&self) -> BTreeSet<String> {
        self.items
            .iter()
            .filter_map(|item| item.texture.as_deref())
            .map(|name| format!("item/{name}"))
            .collect()
    }

    /// The item texture locations that `exists` says are missing.
    pub fn missing_textures(&self, exists: impl Fn(&str) -> bool) -> BTreeSet<String> {
        self.texture_locations()
            .into_iter()
            .filter(|name| !exists(name))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{inventory::items, world::blocks};

    #[test]
    fn blocks_have_items_unless_they_say_not() {
        let registry = registry();
        assert_eq!(registry.of_block(blocks::STONE), Some(items::STONE));
        assert_eq!(registry.of_block(blocks::WATER), None);
        assert_eq!(registry.of_block(blocks::WALL_TORCH), None);
        assert_eq!(
            registry.get(items::STONE).unwrap().places,
            Some(blocks::STONE)
        );
        assert_eq!(items::DIAMOND_PICKAXE.name(), "diamond_pickaxe");
        // The 1.0.0 items, less those that became block items, plus
        // charcoal and the dyes split out of their data values.
        let plain = registry
            .iter()
            .filter(|item| item.texture.is_some() && item.places.is_none());
        assert!(plain.count() > 100);
    }

    #[test]
    fn every_item_texture_exists() {
        let base = crate::pack::Resources::load(&crate::assets::dir(), Vec::new()).unwrap();
        let missing = registry().missing_textures(|name| base.texture(name).is_some());
        // M21's and M24's modern items, which Kenzo's 1.12-era textures
        // predate; a resource pack brings them.
        let needs_pack: BTreeSet<String> = [
            "amethyst_shard",
            "bamboo_door",
            "cherry_door",
            "copper_ingot",
            "crimson_door",
            "glow_berries",
            "mangrove_door",
            "raw_copper",
            "warped_door",
            "warped_fungus_on_a_stick",
        ]
        .map(|name| format!("item/{name}"))
        .into();
        assert_eq!(missing, needs_pack, "missing item textures");
    }

    #[test]
    fn stacks_tools_and_durability_follow_1_0_0() {
        let item = |id| registry().get(id).unwrap();
        assert_eq!(item(items::STICK).max_stack, 64);
        assert_eq!(item(items::SNOWBALL).max_stack, 16);
        assert_eq!(item(items::BUCKET).max_stack, 1);
        assert_eq!(item(items::OAK_DOOR).max_stack, 1);
        assert_eq!(item(items::WOODEN_PICKAXE).max_damage(), Some(59));
        assert_eq!(item(items::DIAMOND_PICKAXE).max_damage(), Some(1561));
        assert_eq!(item(items::GOLDEN_SHOVEL).max_damage(), Some(32));
        assert_eq!(item(items::SHEARS).max_damage(), Some(238));
        assert_eq!(item(items::DIAMOND_CHESTPLATE).max_damage(), Some(528));
        assert_eq!(item(items::STICK).max_damage(), None);
        let sword = item(items::DIAMOND_SWORD).tool.as_ref().unwrap();
        assert_eq!(
            (sword.kind, sword.base_speed, sword.wear),
            (ToolKind::Sword, 1.5, 2)
        );
        assert_eq!(sword.speeds.get(&blocks::COBWEB), Some(&15.0));
        assert_eq!(
            item(items::STONE_PICKAXE).tool.as_ref().unwrap().tier,
            Some(Tier::Stone)
        );
        assert_eq!(item(items::WHEAT_SEEDS).places, Some(blocks::WHEAT));
        assert_eq!(item(items::MUSHROOM_STEW).leftover, Some(items::BOWL));
        assert_eq!(item(items::BOW).shoots, Some(items::ARROW));
        assert_eq!(item(items::OAK_DOOR).texture.as_deref(), Some("oak_door"));
    }

    #[test]
    fn items_save_by_name() {
        let text = ron::to_string(&items::LAPIS_LAZULI).unwrap();
        assert_eq!(text, "\"minecraft:lapis_lazuli\"");
        assert_eq!(ron::from_str::<ItemId>(&text).unwrap(), items::LAPIS_LAZULI);
        assert_eq!(ron::from_str::<ItemId>("\"stick\"").unwrap(), items::STICK);
        assert!(ron::from_str::<ItemId>("\"no_such_item\"").is_err());
    }
}
