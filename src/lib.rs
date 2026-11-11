//! Rustcraft: a single-player Minecraft clone. The library holds the whole
//! game so benchmarks (`benches/`) can reach it; `main.rs` only parses the
//! command line and runs the app.

pub mod advancements;
pub mod app;
pub mod assets;
pub mod audio;
pub mod audit;
pub mod bench;
pub mod bindings;
pub mod cem;
pub mod cli;
pub mod client;
pub mod config;
pub mod credits;
pub mod entity;
pub mod extras;
pub mod figure;
pub mod game;
pub mod inventory;
pub mod lantest;
pub mod level;
pub mod lod;
pub mod look;
pub mod mods;
pub mod net;
pub mod options;
pub mod pack;
pub mod perf;
pub mod placeholders;
pub mod player;
pub mod render;
pub mod server;
#[cfg(not(target_arch = "wasm32"))]
pub mod setup;
pub mod shots;
pub mod startup;
#[cfg(test)]
pub(crate) mod testutil;
pub mod ui;
pub mod world;
pub mod worldlist;
