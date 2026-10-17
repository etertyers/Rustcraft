//! Items, stacks and the player's inventory (crafting comes in M10).

#[cfg_attr(
    not(test),
    expect(dead_code, reason = "stacks, mining and the HUD use these later in M7")
)]
pub mod item;
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "the game holds an inventory later in M7")
)]
pub mod player;
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "the game holds an inventory later in M7")
)]
pub mod stack;
