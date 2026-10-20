use anyhow::{Context, Result};
use rustcraft::{app, bench, game::view::ViewMode, player, shots};
use winit::event_loop::EventLoop;

fn main() -> Result<()> {
    env_logger::init();
    // Writes the built-in structure templates to assets/structures (for
    // after changing their designs in code); overwrites what's there.
    if std::env::args().any(|arg| arg == "--write-structures") {
        let dir = rustcraft::assets::dir().join("structures");
        let count = rustcraft::world::structure::designs::write_all(&dir)?;
        println!("wrote {count} templates to {}", dir.display());
        return Ok(());
    }
    // `--biome-map --seed <n> --size <pixels> --out <file>`: the surface
    // biomes, one pixel per 4×4 column, and their statistics.
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|arg| arg == "--biome-map") {
        return biome_map(&args);
    }
    if args.iter().any(|arg| arg == "--height-map") {
        return height_map(&args);
    }
    let event_loop = EventLoop::new()?;
    let (world, bench, shot, view) = parse_args(std::env::args().skip(1))?;
    let mut app = app::App::new(world).with_bench(bench).with_view(view);
    if let Some((shot, out)) = shot {
        app = app.with_shot(shot, out);
    }
    event_loop.run_app(&mut app)?;
    app.into_result()
}

/// Writes the biome map and prints its statistics.
fn biome_map(args: &[String]) -> Result<()> {
    use rustcraft::world::worldgen::biome_map::{BiomeMap, render, stats, summary};
    let value = |flag: &str| {
        args.iter()
            .position(|arg| arg == flag)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    let seed = value("--seed")
        .map_or(Ok(0), |s| s.parse())
        .context("--seed needs a number")?;
    let size = value("--size")
        .map_or(Ok(4096), |s| s.parse())
        .context("--size needs a number of pixels")?;
    let out = value("--out").unwrap_or_else(|| "biome-map.png".into());
    let started = std::time::Instant::now();
    let map = BiomeMap::sample(seed, size);
    let stats = stats(&map);
    render(&map, &stats).save(&out)?;
    println!(
        "seed {seed}, {size} × {size} pixels ({} blocks across), {:.1} s → {out}",
        size * 4,
        started.elapsed().as_secs_f64()
    );
    print!("{}", summary(&stats));
    Ok(())
}

/// `--height-map --seed <n> --size <pixels> --out <file>`: the surface
/// hillshaded in biome colors, and min/mean/max surface Y per biome.
fn height_map(args: &[String]) -> Result<()> {
    use rustcraft::world::worldgen::biome_map::{HeightMap, height_summary};
    let value = |flag: &str| {
        args.iter()
            .position(|arg| arg == flag)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    let seed = value("--seed")
        .map_or(Ok(0), |s| s.parse())
        .context("--seed needs a number")?;
    let size = value("--size")
        .map_or(Ok(2048), |s| s.parse())
        .context("--size needs a number of pixels")?;
    let out = value("--out").unwrap_or_else(|| "docs/screenshots/m21-height-map.png".into());
    let started = std::time::Instant::now();
    let map = HeightMap::sample(seed, size);
    map.render().save(&out)?;
    println!(
        "seed {seed}, {size} × {size} pixels ({} blocks across), {:.1} s → {out}",
        size * 4,
        started.elapsed().as_secs_f64()
    );
    print!("{}", height_summary(&map.stats()));
    Ok(())
}

/// The scene to capture and where to save it.
type ShotArgs = (&'static shots::Shot, std::path::PathBuf);

/// What the command line asked for: the world, a bench, a scene to
/// capture and the view to start in.
type Parsed = (
    Option<app::WorldKind>,
    Option<bench::Bench>,
    Option<ShotArgs>,
    ViewMode,
);

/// No arguments opens the main menu. `--world <name>` opens a saved world
/// (creating it with `--seed <n>` and `--creative` if it doesn't exist).
/// The unsaved test worlds are `--gallery`, `--flat`, and generated terrain
/// with `--seed <n>`. `--bench [seconds]` flies over generated terrain
/// and prints frame timings (see `bench`), at `--distance <chunks>` if
/// given. `--shot <name> [--out <file>]` captures a scene (see `shots`);
/// `--shot list` lists them. `--view first|back|front` starts in that view
/// (F5 cycles them).
fn parse_args(args: impl Iterator<Item = String>) -> Result<Parsed> {
    let mut args = args.peekable();
    let (mut seed, mut world, mut mode) = (None, None, player::mode::GameMode::Survival);
    let mut bench = None;
    let (mut shot, mut out) = (None, None);
    let mut test_world = None;
    let mut view = ViewMode::First;
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
            "--perf-test" => {
                let mut perf = bench::Bench::new(bench::PERF_SECONDS);
                perf.distance = Some(bench::PERF_DISTANCE);
                perf.perf_report = Some(format!("docs/perf/m{}.md", bench::PERF_MILESTONE).into());
                bench = Some(perf);
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
            "--view" => {
                let value = args.next().context("--view needs first, back or front")?;
                view = match value.as_str() {
                    "first" => ViewMode::First,
                    "back" => ViewMode::ThirdBack,
                    "front" => ViewMode::ThirdFront,
                    other => anyhow::bail!("bad view {other:?} (first, back or front)"),
                };
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
        return Ok((Some(app::WorldKind::Generated { seed }), bench, None, view));
    }
    let world = match (world, seed) {
        _ if test_world.is_some() => test_world,
        (Some(name), seed) => Some(app::WorldKind::Saved { name, seed, mode }),
        (None, Some(seed)) => Some(app::WorldKind::Generated { seed }),
        (None, None) => None,
    };
    Ok((world, None, shot, view))
}
