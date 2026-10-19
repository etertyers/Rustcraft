//! Player control.

pub mod collision;
pub mod experience;
#[cfg(debug_assertions)]
pub mod fly;
pub mod input;
pub mod interaction;
pub mod mode;
pub mod physics;
pub mod raycast;
#[cfg(test)]
mod scenario;
pub mod stats;
