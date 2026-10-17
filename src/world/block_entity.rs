//! Blocks that hold items (1.0.0's tile entities): chests and furnaces,
//! kept in their chunk and saved with it.

use serde::{Deserialize, Serialize};

use crate::inventory::{furnace::Furnace, stack::ItemStack};

/// Slots in a chest.
pub const CHEST_SLOTS: usize = 27;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum BlockEntity {
    /// A chest's `CHEST_SLOTS` slots, row by row.
    Chest(Vec<Option<ItemStack>>),
    Furnace(Furnace),
}

impl BlockEntity {
    pub fn chest() -> Self {
        Self::Chest(vec![None; CHEST_SLOTS])
    }

    pub fn slots(&self) -> &[Option<ItemStack>] {
        match self {
            Self::Chest(slots) => slots,
            Self::Furnace(furnace) => &furnace.slots,
        }
    }

    pub fn slots_mut(&mut self) -> &mut [Option<ItemStack>] {
        match self {
            Self::Chest(slots) => slots,
            Self::Furnace(furnace) => &mut furnace.slots,
        }
    }

    /// Everything it holds, dropped when the block is broken.
    pub fn contents(&self) -> Vec<ItemStack> {
        self.slots().iter().flatten().copied().collect()
    }
}
