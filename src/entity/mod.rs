//! Things in the world that aren't blocks: dropped items and mobs.

#[cfg_attr(
    not(test),
    expect(dead_code, reason = "the game runs mobs later in M9")
)]
pub mod ai;
pub mod item;
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "the game runs mobs later in M9")
)]
pub mod mob;
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "the game spawns mobs later in M9")
)]
pub mod mobs;
#[cfg_attr(not(test), expect(dead_code, reason = "mob AI uses this later in M9"))]
pub mod path;
