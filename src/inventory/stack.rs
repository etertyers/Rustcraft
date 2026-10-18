//! Item stacks, and lookups across blocks and items (blocks are items too).

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use super::{
    crafting::Recipes,
    item::{self, Bucket, ItemDef, ItemId, ItemRegistry, Tool},
};
use crate::world::block::{self, BlockRegistry, BlockState};

/// Some of one item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ItemStack {
    pub id: ItemId,
    pub count: u8,
    /// For items that wear out, the damage taken.
    pub damage: u16,
}

impl ItemStack {
    /// Undamaged.
    pub fn new(id: ItemId, count: u8) -> Self {
        Self {
            id,
            count,
            damage: 0,
        }
    }

    pub fn with_damage(id: ItemId, count: u8, damage: u16) -> Self {
        Self { id, count, damage }
    }

    /// Whether the two can share a slot (same item and damage).
    pub fn stacks_with(&self, other: &ItemStack) -> bool {
        self.id == other.id && self.damage == other.damage
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
