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
    /// A monster spawner (added in M23, so it comes last for old saves).
    Spawner(Spawner),
    /// A brewing stand (M26).
    BrewingStand(crate::inventory::brewing::BrewingStand),
    /// A command block's command and state (M45).
    CommandBlock(CommandBlock),
    /// A jukebox and its disc (M47).
    Jukebox(Jukebox),
}

/// A jukebox: one slot for a disc, and the game tick its song ends at
/// (0 when nothing plays). A song doesn't resume after a reload; the disc
/// stays in.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Jukebox {
    pub slots: Vec<Option<ItemStack>>,
    pub ends: u64,
}

impl Jukebox {
    pub fn new() -> Self {
        Self {
            slots: vec![None],
            ends: 0,
        }
    }
}

/// What a command block keeps (its mode is its block, `conditional` is in its state).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CommandBlock {
    pub command: String,
    /// Always Active (runs without redstone).
    pub auto: bool,
    /// Redstone reaches it now.
    pub powered: bool,
    /// An impulse block has been triggered and runs on its next turn.
    pub pending: bool,
    /// The first line its command said last time, without codes.
    pub last_output: String,
    pub track_output: bool,
    /// What the command returned the last time it ran.
    pub success: i32,
}

impl Default for CommandBlock {
    fn default() -> Self {
        Self {
            command: String::new(),
            auto: false,
            powered: false,
            pending: false,
            last_output: String::new(),
            track_output: true,
            success: 0,
        }
    }
}

/// What a spawner makes, and the ticks until it next tries.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Spawner {
    /// The mob's name, as in `mobs.ron`.
    pub mob: String,
    pub delay: u32,
}

impl Spawner {
    /// A new spawner waits 20 ticks before its first try (modern).
    pub fn new(mob: &str) -> Self {
        Self {
            mob: mob.to_owned(),
            delay: 20,
        }
    }
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
            Self::BrewingStand(stand) => &stand.slots,
            Self::Jukebox(jukebox) => &jukebox.slots,
            Self::Spawner(_) | Self::CommandBlock(_) => &[],
        }
    }

    pub fn slots_mut(&mut self) -> &mut [Option<ItemStack>] {
        match self {
            Self::Chest(slots) | Self::Dispenser(slots) => slots,
            Self::Furnace(furnace) => &mut furnace.slots,
            Self::Hopper(hopper) => &mut hopper.slots,
            Self::BrewingStand(stand) => &mut stand.slots,
            Self::Jukebox(jukebox) => &mut jukebox.slots,
            Self::Spawner(_) | Self::CommandBlock(_) => &mut [],
        }
    }

    /// Everything it holds, dropped when the block is broken.
    pub fn contents(&self) -> Vec<ItemStack> {
        self.slots().iter().flatten().cloned().collect()
    }
}
