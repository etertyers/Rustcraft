//! Player control.

pub mod collision;
#[cfg(debug_assertions)]
pub mod fly;
pub mod input;
pub mod interaction;
pub mod mode;
pub mod physics;
pub mod raycast;
#[cfg(test)]
mod scenario;
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "the game applies these later in M9")
)]
pub mod stats;
