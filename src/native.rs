use anyhow::{Context, Result};
use rustcraft::{app, cli};
use winit::event_loop::EventLoop;

pub fn main() -> Result<()> {
    rustcraft::platform::logging::init();
    // Chooses the mods before any block or item registry is built.
    rustcraft::mods::init();
    // Writes the built-in structure templates to assets/structures (for
    // after changing their designs in code); overwrites what's there.
    if std::env::args().any(|arg| arg == "--write-structures") {
        let dir = rustcraft::assets::dir().join("structures");
        let count = rustcraft::world::structure::designs::write_all(&dir)?;
        println!("wrote {count} templates to {}", dir.display());
        return Ok(());
    }
    // Prints what the asset layers cover: textures and sounds still missing.
    if std::env::args().any(|arg| arg == "--asset-report") {
        let report = rustcraft::assets::report(&rustcraft::assets::dir())?;
        print!("{}", report.text);
        // `--exit`: a failing status when the build is incomplete.
        if std::env::args().any(|arg| arg == "--exit") && !report.problems.is_empty() {
            for problem in &report.problems {
                eprintln!("asset problem: {problem}");
            }
            rustcraft::platform::env::exit(1);
        }
        return Ok(());
    }
    // `--texture-audit [--mode <mode>]`: contact sheets and a report of every
    // block, item, mob and sprite (M60); `--texture-marks` prints Kenzo's marks.
    if std::env::args().any(|arg| arg == "--texture-audit") {
        let args: Vec<String> = std::env::args().collect();
        let mode = args
            .iter()
            .position(|a| a == "--mode")
            .and_then(|i| args.get(i + 1))
            .map_or("vanilla", String::as_str);
        let entry = args
            .iter()
            .position(|a| a == "--entry")
            .and_then(|i| args.get(i + 1))
            .map(String::as_str);
        return rustcraft::audit::cli(mode, entry);
    }
    if std::env::args().any(|arg| arg == "--texture-marks") {
        print!(
            "{}",
            rustcraft::audit::marks::Marks::load(&rustcraft::audit::marks::marks_path()).table()
        );
        return Ok(());
    }
    // Rewrites the placeholder blocks and items from vanilla's data in
    // assets/vanilla (see src/placeholders.rs) and docs/placeholders.md.
    if std::env::args().any(|arg| arg == "--gen-placeholders") {
        let docs = rustcraft::assets::repo_root().join("docs");
        let found = rustcraft::placeholders::generate(&rustcraft::assets::dir(), &docs)?;
        println!(
            "wrote {} placeholder blocks and {} items",
            found.blocks.len(),
            found.items.len()
        );
        for line in &found.unresolved {
            println!("left out: {line}");
        }
        return Ok(());
    }
    // `--biome-map --seed <n> --size <pixels> --out <file>`: the surface
    // biomes, one pixel per 4×4 column, and their statistics.
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|arg| arg == "--flower-test") {
        let seed = args
            .iter()
            .position(|arg| arg == "--seed")
            .and_then(|i| args.get(i + 1))
            .map_or(Ok(12345), |s| s.parse())
            .context("--seed needs a number")?;
        rustcraft::world::worldgen::flower_stats::print_stats(seed, 400);
        return Ok(());
    }
    // `--cave-test [--seed <n>] [--chunks <n>]` and `--cave-slice --seed <n>
    // --x <x> --z <z> --size 256 --out <file>`: the underground in numbers
    // and in a picture.
    let number = |flag: &str, default: i64| -> Result<i64> {
        args.iter()
            .position(|arg| arg == flag)
            .and_then(|i| args.get(i + 1))
            .map_or(Ok(default), |s| s.parse())
            .with_context(|| format!("{flag} needs a number"))
    };
    // `--nether-map [--seed <n>] [--chunks <side>]`: basalt pillars and
    // column clusters per chunk, by biome.
    if args.iter().any(|arg| arg == "--nether-map") {
        let seed = number("--seed", 12345)? as u64;
        rustcraft::world::worldgen::nether_stats::print_stats(seed, number("--chunks", 64)? as i32);
        return Ok(());
    }
    // `--cave-openings [--seed <n>] [--side <chunks>]`: the surface openings
    // the caves make, per 1,000 × 1,000 blocks, by source (M53).
    if args.iter().any(|arg| arg == "--cave-openings") {
        let seed = number("--seed", 12345)? as u64;
        if args.iter().any(|arg| arg == "--time") {
            rustcraft::world::worldgen::opening_stats::time_generation(
                seed,
                number("--side", 16)? as i32,
            );
            return Ok(());
        }
        rustcraft::world::worldgen::opening_stats::print_stats(seed, number("--side", 63)? as i32);
        return Ok(());
    }
    // `--grass-test [--seed <n>] [--side <chunks>]`: the bare dirt the
    // surface leaves exposed, by cause, and the openings (M67).
    if args.iter().any(|arg| arg == "--grass-test") {
        let seed = number("--seed", 12345)? as u64;
        rustcraft::world::worldgen::grass_stats::print_stats(seed, number("--side", 63)? as i32);
        return Ok(());
    }
    if args.iter().any(|arg| arg == "--cave-test") {
        let seed = number("--seed", 12345)? as u64;
        rustcraft::world::worldgen::cave_stats::print_stats(
            seed,
            number("--chunks", 400)? as usize,
        );
        return Ok(());
    }
    if args.iter().any(|arg| arg == "--cave-slice") {
        let out = args
            .iter()
            .position(|arg| arg == "--out")
            .and_then(|i| args.get(i + 1))
            .context("--out needs a file")?;
        rustcraft::world::worldgen::cave_stats::write_slice(
            number("--seed", 12345)? as u64,
            number("--x", 0)? as i32,
            number("--z", 0)? as i32,
            number("--size", 256)? as u32,
            std::path::Path::new(out),
        )?;
        println!("wrote {out}");
        return Ok(());
    }
    if let Some(i) = args.iter().position(|arg| arg == "--lan-test") {
        let bots = args
            .get(i + 1)
            .map_or(Ok(8), |n| n.parse())
            .context("--lan-test needs a number of bots")?;
        let report = rustcraft::lantest::run(bots)?;
        println!("{report}");
        return Ok(());
    }
    if args.iter().any(|arg| arg == "--structure-test") {
        let seed = args
            .iter()
            .position(|arg| arg == "--seed")
            .and_then(|i| args.get(i + 1))
            .map_or(Ok(20111118), |s| s.parse())
            .context("--seed needs a number")?;
        let pass = rustcraft::world::structure::check::run(seed);
        rustcraft::platform::env::exit(if pass { 0 } else { 1 });
    }
    if args.iter().any(|arg| arg == "--spawn-test") {
        let seed = args
            .iter()
            .position(|arg| arg == "--seed")
            .and_then(|i| args.get(i + 1))
            .map_or(Ok(12345), |s| s.parse())
            .context("--seed needs a number")?;
        return rustcraft::game::spawn_test::run(seed);
    }
    if args.iter().any(|arg| arg == "--biome-map") {
        return biome_map(&args);
    }
    if args.iter().any(|arg| arg == "--height-map") {
        return height_map(&args);
    }
    let cli = match cli::parse(std::env::args().skip(1)) {
        Ok(cli) => cli,
        Err(err) => {
            eprint!("{err}");
            rustcraft::platform::env::exit(err.exit_code());
        }
    };
    // `--perf-test --all-looks`: each look and quality in turn.
    if cli.all_looks && cli.bench.as_ref().is_some_and(|b| b.perf_report.is_some()) {
        return rustcraft::perf::run_all_looks(&args);
    }
    // `--shot <name> --compare`: the scene once per look, side by side.
    if let Some(spec) = &cli.compare
        && let Some((_, out)) = &cli.shot
    {
        return rustcraft::render::visuals::compare::run(&args, spec, out, cli.quality);
    }
    // `--perf-test --runs <n>` flies n times in child processes.
    if let Some(bench) = cli
        .bench
        .as_ref()
        .filter(|b| b.perf_report.is_some() && b.runs > 1)
    {
        return rustcraft::perf::run_many(bench, &args);
    }
    let event_loop = EventLoop::new()?;
    let mut app = app::App::new(cli.world)
        .with_bench(cli.bench)
        .with_view(cli.view)
        .with_lan(cli.name, cli.join, cli.lan)
        .with_size(cli.size)
        .with_graphics(cli.style, cli.shaders, cli.quality);
    // `--pack <file or folder>` (repeatable): play or shoot with these
    // resource packs instead of the chosen ones, leaving options.ron be.
    let packs: Vec<String> = args
        .windows(2)
        .filter(|pair| pair[0] == "--pack")
        .map(|pair| pair[1].clone())
        .collect();
    if !packs.is_empty() {
        app = app.with_packs(packs);
    }
    if let Some((shot, out)) = cli.shot {
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
    let started = rustcraft::platform::time::Instant::now();
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
    let started = rustcraft::platform::time::Instant::now();
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
