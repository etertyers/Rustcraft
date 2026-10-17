//! Player control.

pub mod collision;
#[cfg(debug_assertions)]
pub mod fly;
pub mod input;
pub mod interaction;
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "the main menu opens saved worlds")
)]
pub mod mode;
pub mod physics;
pub mod raycast;
#[cfg(test)]
mod scenario;
