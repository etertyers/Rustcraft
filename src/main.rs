mod app;
mod assets;
mod config;
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "the game spawns dropped items later in M7")
)]
mod entity;
mod game;
mod inventory;
mod level;
mod player;
mod render;
mod ui;
mod world;

use anyhow::{Context, Result};
use winit::event_loop::EventLoop;

fn main() -> Result<()> {
    env_logger::init();
    let event_loop = EventLoop::new()?;
    let world = parse_args(std::env::args().skip(1))?;
    let mut app = app::App::new(world);
    event_loop.run_app(&mut app)?;
    app.into_result()
}

/// No arguments opens the main menu. `--world <name>` opens a saved world
/// (creating it with `--seed <n>` and `--creative` if it doesn't exist).
/// The unsaved test worlds are `--gallery`, `--flat`, and generated terrain
/// with `--seed <n>`.
fn parse_args(args: impl Iterator<Item = String>) -> Result<Option<app::WorldKind>> {
    let mut args = args.peekable();
    let (mut seed, mut world, mut mode) = (None, None, player::mode::GameMode::Survival);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--gallery" => return Ok(Some(app::WorldKind::Gallery)),
            "--flat" => return Ok(Some(app::WorldKind::Flat)),
            "--world" => world = Some(args.next().context("--world needs a name")?),
            "--creative" => mode = player::mode::GameMode::Creative,
            "--seed" => {
                let value = args.next().context("--seed needs a number")?;
                seed = Some(
                    value
                        .parse()
                        .with_context(|| format!("bad seed {value:?}"))?,
                );
            }
            other => anyhow::bail!(
                "unknown argument {other:?} (try --world <name>, --seed <n>, --creative, --flat or --gallery)"
            ),
        }
    }
    Ok(match (world, seed) {
        (Some(name), seed) => Some(app::WorldKind::Saved { name, seed, mode }),
        (None, Some(seed)) => Some(app::WorldKind::Generated { seed }),
        (None, None) => None,
    })
}
