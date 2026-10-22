//! Rustcraft: a single-player Minecraft clone. The library holds the whole
//! game so benchmarks (`benches/`) can reach it; `main.rs` only parses the
//! command line and runs the app.

pub mod advancements;
pub mod app;
pub mod assets;
pub mod audio;
pub mod bench;
pub mod config;
pub mod credits;
pub mod entity;
pub mod figure;
pub mod game;
pub mod inventory;
pub mod level;
pub mod look;
pub mod options;
pub mod pack;
pub mod placeholders;
pub mod player;
pub mod render;
pub mod shots;
pub mod ui;
pub mod world;
