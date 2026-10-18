use anyhow::{Context, Result};
use rustcraft::{app, bench, player};
use winit::event_loop::EventLoop;

fn main() -> Result<()> {
    env_logger::init();
    let event_loop = EventLoop::new()?;
    let (world, bench) = parse_args(std::env::args().skip(1))?;
    let mut app = app::App::new(world).with_bench(bench);
    event_loop.run_app(&mut app)?;
    app.into_result()
}

/// No arguments opens the main menu. `--world <name>` opens a saved world
/// (creating it with `--seed <n>` and `--creative` if it doesn't exist).
/// The unsaved test worlds are `--gallery`, `--flat`, and generated terrain
/// with `--seed <n>`. `--bench [seconds]` flies over generated terrain
/// and prints frame timings (see `bench`), at `--distance <chunks>` if
/// given.
fn parse_args(
    args: impl Iterator<Item = String>,
) -> Result<(Option<app::WorldKind>, Option<bench::Bench>)> {
    let mut args = args.peekable();
    let (mut seed, mut world, mut mode) = (None, None, player::mode::GameMode::Survival);
    let mut bench = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--bench" => {
                let seconds = match args.next_if(|next| !next.starts_with("--")) {
                    Some(value) => value
                        .parse()
                        .with_context(|| format!("bad bench length {value:?}"))?,
                    None => bench::DEFAULT_SECONDS,
                };
                bench
                    .get_or_insert_with(|| bench::Bench::new(seconds))
                    .seconds = seconds;
            }
            "--distance" => {
                let value = args.next().context("--distance needs a number of chunks")?;
                let distance = value
                    .parse()
                    .with_context(|| format!("bad render distance {value:?}"))?;
                bench
                    .get_or_insert_with(|| bench::Bench::new(bench::DEFAULT_SECONDS))
                    .distance = Some(distance);
            }
            "--gallery" => return Ok((Some(app::WorldKind::Gallery), None)),
            "--flat" => return Ok((Some(app::WorldKind::Flat), None)),
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
                "unknown argument {other:?} (try --world <name>, --seed <n>, --creative, --flat, --gallery, --bench [seconds] or --distance <chunks>)"
            ),
        }
    }
    if bench.is_some() {
        let seed = seed.unwrap_or(bench::SEED);
        return Ok((Some(app::WorldKind::Generated { seed }), bench));
    }
    let world = match (world, seed) {
        (Some(name), seed) => Some(app::WorldKind::Saved { name, seed, mode }),
        (None, Some(seed)) => Some(app::WorldKind::Generated { seed }),
        (None, None) => None,
    };
    Ok((world, None))
}
