//! Item definitions: an item for every block that has one (from
//! `assets/data/blocks.ron`), then the rest from `assets/data/items.ron`.
//!
//! Items are named as in modern Java Edition and numbered by their place in
//! that order (see build.rs); saves store names.

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::LazyLock,
};

use anyhow::{Context, Result, bail};
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
            .or_else(|| crate::mods::active().item_name(self.index()))
            .unwrap_or("unknown")
    }
}

thread_local! {
    static LENIENT: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Runs `read` with items of mods that aren't installed reading as
/// `UNKNOWN_ITEM` instead of failing (loading saves).
pub fn lenient<R>(read: impl FnOnce() -> R) -> R {
    let before = LENIENT.replace(true);
    let out = read();
    LENIENT.set(before);
    out
}

/// Item IDs are written by name (`"stick"` or `"minecraft:stick"`).
impl<'de> Deserialize<'de> for ItemId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let name = String::deserialize(deserializer)?;
        if let Some(id) = super::items::by_name(block::strip_namespace(&name)) {
            return Ok(id);
        }
        if LENIENT.get() && name.contains(':') && !name.starts_with("minecraft:") {
            return Ok(super::items::UNKNOWN_ITEM);
        }
        Err(serde::de::Error::custom(format!("unknown item {name:?}")))
    }
}

impl Serialize for ItemId {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(&block::qualified(self.name()))
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ItemDef {
    /// Set from the item's place in the order.
    #[serde(skip)]
    pub id: ItemId,
    pub name: String,
    /// Registered from vanilla's data with no behavior of its own
    /// (`assets/data/placeholder_items.ron`, listed in `docs/placeholders.md`).
    #[serde(default)]
    pub placeholder: bool,
    /// Its name as shown ("Baked Potato"), where the name made readable
    /// isn't enough (placeholders take theirs from vanilla's language file).
    #[serde(default)]
    pub display: Option<String>,
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
    /// A mod's script has an `on_use` for it.
    #[serde(default)]
    pub scripted: bool,
}

/// An item texture name from `items.ron` as a resource location: bare
/// names (`stick`) are under `item/`; names with a folder (`block/azalea`)
/// are kept.
pub fn texture_location(name: &str) -> std::borrow::Cow<'_, str> {
    if name.contains('/') {
        name.into()
    } else {
        format!("item/{name}").into()
    }
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
    /// Modern netherite (M25).
    Netherite,
}

impl Tier {
    /// Mining speed on blocks the tool is made for.
    pub fn efficiency(self) -> f32 {
        match self {
            Tier::Wood => 2.0,
            Tier::Stone => 4.0,
            Tier::Iron => 6.0,
            Tier::Diamond => 8.0,
            Tier::Netherite => 9.0,
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
            Tier::Netherite => 4,
        }
    }

    pub fn durability(self) -> u16 {
        match self {
            Tier::Wood => 59,
            Tier::Stone => 131,
            Tier::Iron => 250,
            Tier::Diamond => 1561,
            Tier::Netherite => 2031,
            Tier::Gold => 32,
        }
    }
}

fn default_max_stack() -> u8 {
    64
}

/// Vanilla's (1.21) most of an item in a stack, by its name: the data
/// files must agree, which a test checks.
pub fn vanilla_max_stack(name: &str) -> u8 {
    const ONE_SUFFIXES: &[&str] = &[
        "_sword",
        "_pickaxe",
        "_axe",
        "_shovel",
        "_hoe",
        "_spear",
        "_helmet",
        "_chestplate",
        "_leggings",
        "_boots",
        "_horse_armor",
        "_bucket",
        "_boat",
        "_raft",
        "_minecart",
        "_bed",
        "_shulker_box",
        "_bundle",
        "_banner_pattern",
        "_on_a_stick",
        "_stew",
        "_soup",
    ];
    const ONE: &[&str] = &[
        "minecart",
        "saddle",
        "cake",
        "potion",
        "splash_potion",
        "lingering_potion",
        "enchanted_book",
        "writable_book",
        "knowledge_book",
        "debug_stick",
        "bow",
        "crossbow",
        "trident",
        "mace",
        "fishing_rod",
        "shears",
        "flint_and_steel",
        "shield",
        "elytra",
        "totem_of_undying",
        "spyglass",
        "brush",
        "goat_horn",
        "shulker_box",
        "bundle",
        "wolf_armor",
        "turtle_helmet",
    ];
    const SIXTEEN_SUFFIXES: &[&str] = &["_sign", "_banner"];
    const SIXTEEN: &[&str] = &[
        "bucket",
        "egg",
        "blue_egg",
        "brown_egg",
        "snowball",
        "ender_pearl",
        "honey_bottle",
        "armor_stand",
        "written_book",
    ];
    if SIXTEEN.contains(&name) || SIXTEEN_SUFFIXES.iter().any(|s| name.ends_with(s)) {
        16
    } else if ONE.contains(&name)
        || name.starts_with("music_disc_")
        || ONE_SUFFIXES.iter().any(|s| name.ends_with(s))
    {
        1
    } else {
        64
    }
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
            placeholder: def.placeholder,
            display: def.display.clone(),
            texture: def.item_texture.clone(),
            max_stack: def.stack,
            durability: None,
            tool: None,
            places: Some(def.id),
            food: None,
            leftover: None,
            shoots: None,
            bucket: None,
            scripted: false,
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
        ItemRegistry::from_ron(&[
            include_str!("../../assets/data/items.ron"),
            include_str!("../../assets/data/placeholder_items.ron"),
        ])
        .expect("assets/data/items.ron and placeholder_items.ron are valid")
    });
    &REGISTRY
}

/// The mods' plain items (their `data/items.ron` and `main.lua`).
fn mod_defs() -> Result<std::collections::HashMap<String, ItemDef>> {
    let mods = crate::mods::active();
    let mut by_name = std::collections::HashMap::new();
    for active in &mods.active {
        if let Some(text) = &active.data.items {
            for def in ron::from_str::<Vec<ItemDef>>(text)
                .with_context(|| format!("{}'s data/items.ron", active.manifest.id))?
            {
                by_name.insert(def.name.clone(), def);
            }
        }
    }
    for lua in &mods.lua_items {
        let def: ItemDef = serde_json::from_value(lua.def.clone())
            .with_context(|| format!("an item of {}", lua.mod_id))?;
        by_name.insert(def.name.clone(), def);
    }
    Ok(by_name)
}

impl ItemRegistry {
    fn from_ron(texts: &[&str]) -> Result<Self> {
        let mut plain: Vec<ItemDef> = Vec::new();
        for text in texts {
            plain.extend(ron::from_str::<Vec<ItemDef>>(text)?);
        }
        let blocks = block::registry();
        let mut items = Vec::with_capacity(super::items::COUNT);
        let mut by_block = vec![None; blocks.iter().count()];
        for def in blocks
            .iter()
            .filter(|def| def.item && def.id.index() < crate::world::blocks::COUNT)
        {
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
        // The mods' items come after, in the order of the mod set's names.
        let mods = crate::mods::active();
        let mut plain_mod = mod_defs()?;
        for name in mods.item_names_all() {
            let id = ItemId(items.len() as u16);
            if let Some(def) = blocks.iter().find(|def| def.name == *name && def.item) {
                by_block[def.id.index()] = Some(id);
                items.push(ItemDef::for_block(id, def));
            } else if let Some(mut item) = plain_mod.remove(*name) {
                if item.texture.is_none() {
                    bail!("item {} has no texture", item.name);
                }
                item.id = id;
                items.push(item);
            } else {
                bail!("no definition for the item {name}");
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
            .map(|name| texture_location(name).into_owned())
            .collect()
    }

    /// The item texture locations that `exists` says are missing.
    pub fn missing_textures(&self, exists: impl Fn(&str) -> bool) -> BTreeSet<String> {
        self.missing_textures_in(exists, false)
    }

    /// Like `missing_textures`, counting the placeholder items too if
    /// `placeholders` (they get their textures from the vanilla layer).
    pub fn missing_textures_in(
        &self,
        exists: impl Fn(&str) -> bool,
        placeholders: bool,
    ) -> BTreeSet<String> {
        self.items
            .iter()
            .filter(|item| placeholders || !item.placeholder)
            .filter_map(|item| item.texture.as_deref())
            .map(|name| texture_location(name).into_owned())
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
        let base =
            crate::pack::Resources::load_with(&crate::assets::dir(), Vec::new(), false).unwrap();
        let missing = registry().missing_textures(|name| base.texture(name).is_some());
        // M21's, M24's and M25's modern items, which Kenzo's 1.12-era textures
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
            "chain",
            "netherite_axe",
            "netherite_boots",
            "netherite_chestplate",
            "netherite_helmet",
            "netherite_hoe",
            "netherite_ingot",
            "netherite_leggings",
            "netherite_pickaxe",
            "netherite_scrap",
            "netherite_shovel",
            "netherite_sword",
            "netherite_upgrade_smithing_template",
            "elytra",
            "popped_chorus_fruit",
        ]
        .map(|name| texture_location(name).into_owned())
        .into();
        assert_eq!(missing, needs_pack, "missing item textures");
    }

    #[test]
    fn every_item_stacks_as_in_vanilla() {
        let registry = registry();
        let wrong: Vec<String> = registry
            .iter()
            // Mods' items aren't vanilla's.
            .filter(|item| !item.name.contains(':'))
            .filter(|item| item.max_stack != vanilla_max_stack(&item.name))
            .map(|item| {
                format!(
                    "{} {} -> {}",
                    item.name,
                    item.max_stack,
                    vanilla_max_stack(&item.name)
                )
            })
            .collect();
        assert!(
            wrong.is_empty(),
            "stack sizes differ from vanilla:\n{}",
            wrong.join("\n")
        );
    }

    #[test]
    fn stacks_tools_and_durability_follow_1_0_0() {
        let item = |id| registry().get(id).unwrap();
        assert_eq!(item(items::STICK).max_stack, 64);
        assert_eq!(item(items::SNOWBALL).max_stack, 16);
        assert_eq!(item(items::BUCKET).max_stack, 16);
        assert_eq!(item(items::OAK_DOOR).max_stack, 64);
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
