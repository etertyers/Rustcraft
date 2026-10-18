//! Rustcraft: a single-player Minecraft clone. The library holds the whole
//! game so benchmarks (`benches/`) can reach it; `main.rs` only parses the
//! command line and runs the app.

pub mod app;
pub mod assets;
pub mod bench;
pub mod config;
pub mod entity;
pub mod game;
pub mod inventory;
pub mod level;
pub mod options;
pub mod player;
pub mod render;
pub mod ui;
pub mod world;
