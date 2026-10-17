//! Item stacks, and lookups across blocks and items (blocks are items too).

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use super::{
    crafting::Recipes,
    item::{FIRST_ITEM_ID, ItemDef, ItemId, ItemRegistry, Tool},
};
use crate::world::block::{BlockRegistry, BlockState};

/// Some of one item, like 1.0.0's `ItemStack`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ItemStack {
    pub id: ItemId,
    pub count: u8,
    /// The data value (block variant, dye color), or for items that wear
    /// out, the damage taken.
    pub damage: u16,
}

impl ItemStack {
    /// Undamaged, data value 0 (used by the debug dev kit and tests).
    #[cfg(any(debug_assertions, test))]
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

    /// Whether the two can share a slot (same item and data value).
    pub fn stacks_with(&self, other: &ItemStack) -> bool {
        self.id == other.id && self.damage == other.damage
    }
}

/// Item facts for any ID, block or item.
#[derive(Clone)]
pub struct Items {
    pub blocks: Arc<BlockRegistry>,
    pub items: Arc<ItemRegistry>,
    pub recipes: Arc<Recipes>,
}

impl Items {
    /// Without recipes; see `with_recipes`.
    pub fn new(blocks: Arc<BlockRegistry>, items: Arc<ItemRegistry>) -> Self {
        Self {
            blocks,
            items,
            recipes: Arc::new(Recipes::default()),
        }
    }

    pub fn with_recipes(self, recipes: Arc<Recipes>) -> Self {
        Self { recipes, ..self }
    }

    /// The item definition, for IDs that aren't blocks.
    pub fn item(&self, id: ItemId) -> Option<&ItemDef> {
        if id < FIRST_ITEM_ID {
            return None;
        }
        self.items.get(id)
    }

    /// Whether `id` names a block or item that exists.
    pub fn exists(&self, id: ItemId) -> bool {
        self.item(id).is_some() || (id < FIRST_ITEM_ID && self.blocks.get(id).is_some())
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

    /// The block this stack places, if any: blocks place themselves (with
    /// the stack's data value), some items place a block.
    pub fn places(&self, stack: &ItemStack) -> Option<BlockState> {
        if stack.id < FIRST_ITEM_ID {
            let def = self.blocks.get(stack.id)?;
            let data = if def.variants.is_empty() {
                0
            } else {
                stack.damage as u8
            };
            return Some(BlockState::new(stack.id, data));
        }
        self.item(stack.id)?
            .places
            .map(|block| BlockState::new(block, 0))
    }

    /// Display name, e.g. "stone" or "lapis_lazuli".
    pub fn name(&self, stack: &ItemStack) -> String {
        if stack.id < FIRST_ITEM_ID {
            return self
                .blocks
                .display_name(BlockState::new(stack.id, stack.damage as u8));
        }
        match self.item(stack.id) {
            Some(item) if item.max_damage().is_some() => item.name.clone(),
            Some(item) => item.variant(stack.damage as u8).0.to_owned(),
            None => format!("unknown item {}", stack.id),
        }
    }
}
