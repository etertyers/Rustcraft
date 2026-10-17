//! Items and (from M7) stacks, inventory and crafting.

#[cfg_attr(
    not(test),
    expect(dead_code, reason = "stacks, mining and the HUD use these later in M7")
)]
pub mod item;
