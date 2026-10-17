//! Blocks that hold items (1.0.0's tile entities): a chest's slots, kept in
//! their chunk and saved with it.

use serde::{Deserialize, Serialize};

use crate::inventory::stack::ItemStack;

/// Slots in a chest.
pub const CHEST_SLOTS: usize = 27;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum BlockEntity {
    /// A chest's `CHEST_SLOTS` slots, row by row.
    Chest(Vec<Option<ItemStack>>),
}

impl BlockEntity {
    pub fn chest() -> Self {
        Self::Chest(vec![None; CHEST_SLOTS])
    }

    pub fn slots(&self) -> &[Option<ItemStack>] {
        match self {
            Self::Chest(slots) => slots,
        }
    }

    pub fn slots_mut(&mut self) -> &mut [Option<ItemStack>] {
        match self {
            Self::Chest(slots) => slots,
        }
    }

    /// Everything it holds, dropped when the block is broken.
    pub fn contents(&self) -> Vec<ItemStack> {
        self.slots().iter().flatten().copied().collect()
    }
}
