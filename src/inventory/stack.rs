//! Item stacks, and lookups across blocks and items (blocks are items too).

use std::{fmt, sync::Arc};

use serde::{
    Deserialize, Deserializer, Serialize, Serializer,
    de::{MapAccess, SeqAccess, Visitor},
    ser::SerializeStruct,
};

use super::{
    crafting::Recipes,
    enchant::{Enchanted, Enchantment},
    item::{self, Bucket, ItemDef, ItemId, ItemRegistry, Tool},
};
use crate::world::block::{self, BlockRegistry, BlockState};

/// Some of one item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemStack {
    pub id: ItemId,
    pub count: u8,
    /// For items that wear out, the damage taken.
    pub damage: u16,
    /// Enchantments, a custom name and the anvil's work penalty, which
    /// most stacks don't have.
    pub extra: Option<Box<Extra>>,
}

/// What a stack carries besides its item, count and damage.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Extra {
    pub enchants: Vec<Enchanted>,
    /// Set by renaming at an anvil.
    pub name: Option<String>,
    /// Times the anvil has worked on it (modern `RepairCost` is 2^n − 1).
    pub work: u8,
}

impl ItemStack {
    /// Undamaged.
    pub fn new(id: ItemId, count: u8) -> Self {
        Self::with_damage(id, count, 0)
    }

    pub fn with_damage(id: ItemId, count: u8, damage: u16) -> Self {
        Self {
            id,
            count,
            damage,
            extra: None,
        }
    }

    /// Whether the two can share a slot (same item, damage, enchantments
    /// and name).
    pub fn stacks_with(&self, other: &ItemStack) -> bool {
        self.id == other.id && self.damage == other.damage && self.extra == other.extra
    }

    /// The same item with a different count.
    pub fn with_count(&self, count: u8) -> Self {
        Self {
            count,
            ..self.clone()
        }
    }

    pub fn enchantments(&self) -> &[Enchanted] {
        self.extra.as_ref().map_or(&[], |extra| &extra.enchants)
    }

    pub fn is_enchanted(&self) -> bool {
        !self.enchantments().is_empty()
    }

    /// The level of `enchantment` on the stack, 0 if it isn't there.
    pub fn level_of(&self, enchantment: Enchantment) -> u8 {
        self.enchantments()
            .iter()
            .find(|(e, _)| *e == enchantment)
            .map_or(0, |&(_, level)| level)
    }

    /// Puts on `enchantment` at `level`, replacing a lower or equal level.
    pub fn enchant(&mut self, enchantment: Enchantment, level: u8) {
        let extra = self.extra.get_or_insert_with(Default::default);
        match extra.enchants.iter_mut().find(|(e, _)| *e == enchantment) {
            Some(entry) => entry.1 = level,
            None => extra.enchants.push((enchantment, level)),
        }
    }

    pub fn clear_enchantments(&mut self) {
        if let Some(extra) = self.extra.as_mut() {
            extra.enchants.clear();
        }
        self.tidy();
    }

    pub fn custom_name(&self) -> Option<&str> {
        self.extra.as_ref()?.name.as_deref()
    }

    pub fn set_custom_name(&mut self, name: Option<String>) {
        if name.is_some() {
            self.extra.get_or_insert_with(Default::default).name = name;
        } else if let Some(extra) = self.extra.as_mut() {
            extra.name = None;
        }
        self.tidy();
    }

    /// How many times an anvil has worked on the stack.
    pub fn work(&self) -> u8 {
        self.extra.as_ref().map_or(0, |extra| extra.work)
    }

    pub fn set_work(&mut self, work: u8) {
        if work > 0 {
            self.extra.get_or_insert_with(Default::default).work = work;
        } else if let Some(extra) = self.extra.as_mut() {
            extra.work = 0;
        }
        self.tidy();
    }

    /// Drops `extra` once it holds nothing, so plain stacks compare and
    /// save as plain.
    fn tidy(&mut self) {
        if self.extra.as_deref() == Some(&Extra::default()) {
            self.extra = None;
        }
    }
}

/// Set in a saved stack's count when it has an `extra` after it. Saves
/// made before enchanting (bincode, with no length for the fields) read
/// back unchanged.
const HAS_EXTRA: u8 = 0x80;

impl Serialize for ItemStack {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut out =
            serializer.serialize_struct("ItemStack", 3 + self.extra.is_some() as usize)?;
        out.serialize_field("id", &self.id)?;
        let flag = if self.extra.is_some() { HAS_EXTRA } else { 0 };
        out.serialize_field("count", &(self.count | flag))?;
        out.serialize_field("damage", &self.damage)?;
        if let Some(extra) = &self.extra {
            out.serialize_field("extra", extra)?;
        }
        out.end()
    }
}

impl<'de> Deserialize<'de> for ItemStack {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct StackVisitor;

        impl<'de> Visitor<'de> for StackVisitor {
            type Value = ItemStack;

            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("an item stack")
            }

            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<ItemStack, A::Error> {
                let missing = |field| serde::de::Error::missing_field(field);
                let id = seq.next_element()?.ok_or_else(|| missing("id"))?;
                let count: u8 = seq.next_element()?.ok_or_else(|| missing("count"))?;
                let damage = seq.next_element()?.ok_or_else(|| missing("damage"))?;
                let extra = if count & HAS_EXTRA != 0 {
                    Some(Box::new(
                        seq.next_element()?.ok_or_else(|| missing("extra"))?,
                    ))
                } else {
                    None
                };
                Ok(ItemStack {
                    id,
                    count: count & !HAS_EXTRA,
                    damage,
                    extra,
                })
            }

            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<ItemStack, A::Error> {
                let (mut id, mut count, mut damage, mut extra) = (None, None, None, None);
                while let Some(key) = map.next_key::<String>()? {
                    match key.as_str() {
                        "id" => id = Some(map.next_value()?),
                        "count" => count = Some(map.next_value::<u8>()?),
                        "damage" => damage = Some(map.next_value()?),
                        "extra" => extra = Some(Box::new(map.next_value::<Extra>()?)),
                        _ => {
                            map.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                let missing = |field| serde::de::Error::missing_field(field);
                Ok(ItemStack {
                    id: id.ok_or_else(|| missing("id"))?,
                    count: count.ok_or_else(|| missing("count"))? & !HAS_EXTRA,
                    damage: damage.unwrap_or(0),
                    extra,
                })
            }
        }

        deserializer.deserialize_struct(
            "ItemStack",
            &["id", "count", "damage", "extra"],
            StackVisitor,
        )
    }
}

/// Item facts, and the blocks and recipes behind them.
#[derive(Clone)]
pub struct Items {
    pub blocks: &'static BlockRegistry,
    pub items: &'static ItemRegistry,
    pub recipes: Arc<Recipes>,
}

impl Default for Items {
    fn default() -> Self {
        Self::new()
    }
}

impl Items {
    /// Without recipes; see `with_recipes`.
    pub fn new() -> Self {
        Self {
            blocks: block::registry(),
            items: item::registry(),
            recipes: Arc::new(Recipes::default()),
        }
    }

    pub fn with_recipes(self, recipes: Arc<Recipes>) -> Self {
        Self { recipes, ..self }
    }

    pub fn item(&self, id: ItemId) -> Option<&ItemDef> {
        self.items.get(id)
    }

    /// Whether `id` names an item that exists.
    pub fn exists(&self, id: ItemId) -> bool {
        self.item(id).is_some()
    }

    pub fn max_stack(&self, id: ItemId) -> u8 {
        self.item(id).map_or(64, |item| item.max_stack)
    }

    /// Uses before the item breaks, for items that wear out.
    pub fn max_damage(&self, id: ItemId) -> Option<u16> {
        self.item(id)?.max_damage()
    }

    pub fn tool(&self, id: ItemId) -> Option<&Tool> {
        self.item(id)?.tool.as_ref()
    }

    /// What the item is as a bucket, if it is one.
    pub fn bucket(&self, id: ItemId) -> Option<Bucket> {
        self.item(id)?.bucket
    }

    /// The bucket item holding `bucket`'s contents.
    pub fn bucket_item(&self, bucket: Bucket) -> Option<ItemId> {
        self.items
            .iter()
            .find(|item| item.bucket == Some(bucket))
            .map(|item| item.id)
    }

    /// The block this stack places, if any, in the state a player places it.
    pub fn places(&self, stack: &ItemStack) -> Option<BlockState> {
        let block = self.item(stack.id)?.places?;
        Some(self.blocks.def(block).placed_state())
    }

    /// Display name, e.g. "stone" or "lapis lazuli".
    pub fn name(&self, stack: &ItemStack) -> String {
        match self.item(stack.id) {
            Some(item) => item.name.replace('_', " "),
            None => format!("unknown item {}", stack.id.index()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inventory::items;

    fn bincode_config() -> impl bincode::config::Config {
        bincode::config::standard().with_fixed_int_encoding()
    }

    fn enchanted() -> ItemStack {
        let mut pickaxe = ItemStack::with_damage(items::DIAMOND_PICKAXE, 1, 12);
        pickaxe.enchant(Enchantment::Efficiency, 3);
        pickaxe.enchant(Enchantment::Unbreaking, 2);
        pickaxe.set_custom_name(Some("Digger".into()));
        pickaxe.set_work(2);
        pickaxe
    }

    #[test]
    fn an_old_stack_in_a_save_reads_back_unchanged() {
        // v1.0's ItemStack was id, count and damage, in that order.
        let old = (items::STONE, 37u8, 0u16);
        let bytes = bincode::serde::encode_to_vec(old, bincode_config()).unwrap();
        let (stack, used): (ItemStack, usize) =
            bincode::serde::decode_from_slice(&bytes, bincode_config()).unwrap();
        assert_eq!(used, bytes.len());
        assert_eq!(stack, ItemStack::new(items::STONE, 37));
        // And a plain stack still writes exactly that.
        let again = bincode::serde::encode_to_vec(&stack, bincode_config()).unwrap();
        assert_eq!(again, bytes);
    }

    #[test]
    fn enchanted_stacks_round_trip_in_bincode_and_ron() {
        let stack = enchanted();
        let bytes = bincode::serde::encode_to_vec(&stack, bincode_config()).unwrap();
        let (back, used): (ItemStack, usize) =
            bincode::serde::decode_from_slice(&bytes, bincode_config()).unwrap();
        assert_eq!((back, used), (stack.clone(), bytes.len()));
        let text = ron::to_string(&stack).unwrap();
        assert_eq!(ron::from_str::<ItemStack>(&text).unwrap(), stack);
    }

    #[test]
    fn a_list_of_mixed_stacks_stays_in_step() {
        let slots = vec![
            Some(ItemStack::new(items::DIRT, 3)),
            Some(enchanted()),
            None,
            Some(ItemStack::new(items::COAL, 9)),
        ];
        let bytes = bincode::serde::encode_to_vec(&slots, bincode_config()).unwrap();
        let (back, _): (Vec<Option<ItemStack>>, usize) =
            bincode::serde::decode_from_slice(&bytes, bincode_config()).unwrap();
        assert_eq!(back, slots);
    }

    #[test]
    fn a_saved_ron_stack_without_extras_still_loads() {
        let text = r#"(id: "minecraft:stick", count: 4, damage: 0)"#;
        assert_eq!(
            ron::from_str::<ItemStack>(text).unwrap(),
            ItemStack::new(items::STICK, 4)
        );
    }

    #[test]
    fn enchantments_keep_stacks_apart_and_tidy_up() {
        let plain = ItemStack::with_damage(items::DIAMOND_PICKAXE, 1, 12);
        assert!(!plain.stacks_with(&enchanted()));
        let mut stack = enchanted();
        stack.clear_enchantments();
        stack.set_custom_name(None);
        stack.set_work(0);
        assert_eq!(stack, plain, "nothing left over");
        assert_eq!(enchanted().level_of(Enchantment::Efficiency), 3);
        assert_eq!(enchanted().level_of(Enchantment::Fortune), 0);
    }
}
