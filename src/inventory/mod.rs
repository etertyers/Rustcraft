//! Items, stacks, the player's inventory and crafting.

#[cfg_attr(
    not(test),
    expect(dead_code, reason = "crafting screens use this later in M10")
)]
pub mod crafting;
pub mod item;
pub mod player;
pub mod stack;
