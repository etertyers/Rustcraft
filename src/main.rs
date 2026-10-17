mod app;
mod assets;
mod config;
mod inventory;
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "the main menu opens saved worlds")
)]
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

/// `--gallery`, `--flat`, or generated terrain with `--seed <n>` (random if omitted).
fn parse_args(args: impl Iterator<Item = String>) -> Result<app::WorldKind> {
    let mut args = args.peekable();
    let mut seed = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--gallery" => return Ok(app::WorldKind::Gallery),
            "--flat" => return Ok(app::WorldKind::Flat),
            "--seed" => {
                let value = args.next().context("--seed needs a number")?;
                seed = Some(
                    value
                        .parse()
                        .with_context(|| format!("bad seed {value:?}"))?,
                );
            }
            other => {
                anyhow::bail!("unknown argument {other:?} (try --seed <n>, --flat or --gallery)")
            }
        }
    }
    let seed = seed.unwrap_or_else(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos() as u64)
    });
    Ok(app::WorldKind::Generated { seed })
}
