use anyhow::{Context, Result};
use rustcraft::{app, bench, player, shots};
use winit::event_loop::EventLoop;

fn main() -> Result<()> {
    env_logger::init();
    let event_loop = EventLoop::new()?;
    let (world, bench, shot) = parse_args(std::env::args().skip(1))?;
    let mut app = app::App::new(world).with_bench(bench);
    if let Some((shot, out)) = shot {
        app = app.with_shot(shot, out);
    }
    event_loop.run_app(&mut app)?;
    app.into_result()
}

/// The scene to capture and where to save it.
type ShotArgs = (&'static shots::Shot, std::path::PathBuf);

/// No arguments opens the main menu. `--world <name>` opens a saved world
/// (creating it with `--seed <n>` and `--creative` if it doesn't exist).
/// The unsaved test worlds are `--gallery`, `--flat`, and generated terrain
/// with `--seed <n>`. `--bench [seconds]` flies over generated terrain
/// and prints frame timings (see `bench`), at `--distance <chunks>` if
/// given. `--shot <name> [--out <file>]` captures a scene (see `shots`);
/// `--shot list` lists them.
fn parse_args(
    args: impl Iterator<Item = String>,
) -> Result<(
    Option<app::WorldKind>,
    Option<bench::Bench>,
    Option<ShotArgs>,
)> {
    let mut args = args.peekable();
    let (mut seed, mut world, mut mode) = (None, None, player::mode::GameMode::Survival);
    let mut bench = None;
    let (mut shot, mut out) = (None, None);
    let mut test_world = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--shot" => {
                let name = args.next().context("--shot needs a scene name")?;
                if name == "list" {
                    for shot in shots::SHOTS {
                        println!("{:28} {}", shot.name, shot.about);
                    }
                    std::process::exit(0);
                }
                shot = Some(
                    shots::find(&name)
                        .with_context(|| format!("no scene {name:?} (try --shot list)"))?,
                );
            }
            "--out" => out = Some(args.next().context("--out needs a file")?.into()),
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
            "--gallery" => test_world = Some(app::WorldKind::Gallery),
            "--flat" => test_world = Some(app::WorldKind::Flat),
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
                "unknown argument {other:?} (try --world <name>, --seed <n>, --creative, --flat, --gallery, --bench [seconds], --distance <chunks> or --shot <name>)"
            ),
        }
    }
    let shot = shot.map(|shot: &'static shots::Shot| {
        let out = out.unwrap_or_else(|| format!("docs/screenshots/{}.png", shot.name).into());
        (shot, out)
    });
    if bench.is_some() {
        let seed = seed.unwrap_or(bench::SEED);
        return Ok((Some(app::WorldKind::Generated { seed }), bench, None));
    }
    let world = match (world, seed) {
        _ if test_world.is_some() => test_world,
        (Some(name), seed) => Some(app::WorldKind::Saved { name, seed, mode }),
        (None, Some(seed)) => Some(app::WorldKind::Generated { seed }),
        (None, None) => None,
    };
    Ok((world, None, shot))
}
