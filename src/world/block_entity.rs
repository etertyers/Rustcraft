//! Blocks that hold items (1.0.0's tile entities): chests and furnaces,
//! kept in their chunk and saved with it.

use serde::{Deserialize, Serialize};

use crate::inventory::{furnace::Furnace, stack::ItemStack};

/// Slots in a chest.
pub const CHEST_SLOTS: usize = 27;
/// Slots in a dispenser or dropper (3×3), and in a hopper.
pub const DISPENSER_SLOTS: usize = 9;
pub const HOPPER_SLOTS: usize = 5;
/// Ticks a hopper waits after moving an item (modern `MOVE_ITEM_SPEED`).
pub const HOPPER_COOLDOWN: u8 = 8;

/// A hopper's slots and the ticks until it moves something again.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Hopper {
    pub slots: Vec<Option<ItemStack>>,
    pub cooldown: u8,
}

impl Default for Hopper {
    fn default() -> Self {
        Self {
            slots: vec![None; HOPPER_SLOTS],
            cooldown: 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum BlockEntity {
    /// A chest's `CHEST_SLOTS` slots, row by row.
    Chest(Vec<Option<ItemStack>>),
    Furnace(Furnace),
    /// A dispenser's or dropper's nine slots.
    Dispenser(Vec<Option<ItemStack>>),
    Hopper(Hopper),
}

impl BlockEntity {
    pub fn chest() -> Self {
        Self::Chest(vec![None; CHEST_SLOTS])
    }

    pub fn dispenser() -> Self {
        Self::Dispenser(vec![None; DISPENSER_SLOTS])
    }

    pub fn slots(&self) -> &[Option<ItemStack>] {
        match self {
            Self::Chest(slots) | Self::Dispenser(slots) => slots,
            Self::Furnace(furnace) => &furnace.slots,
            Self::Hopper(hopper) => &hopper.slots,
        }
    }

    pub fn slots_mut(&mut self) -> &mut [Option<ItemStack>] {
        match self {
            Self::Chest(slots) | Self::Dispenser(slots) => slots,
            Self::Furnace(furnace) => &mut furnace.slots,
            Self::Hopper(hopper) => &mut hopper.slots,
        }
    }

    /// Everything it holds, dropped when the block is broken.
    pub fn contents(&self) -> Vec<ItemStack> {
        self.slots().iter().flatten().cloned().collect()
    }
}
