use anyhow::{Context, Result};
use rustcraft::{app, cli};
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
    // Prints what the asset layers cover: textures and sounds still missing.
    if std::env::args().any(|arg| arg == "--asset-report") {
        print!("{}", rustcraft::assets::report(&rustcraft::assets::dir())?);
        return Ok(());
    }
    // Rewrites the placeholder blocks and items from vanilla's data in
    // assets/vanilla (see src/placeholders.rs) and docs/placeholders.md.
    if std::env::args().any(|arg| arg == "--gen-placeholders") {
        let docs = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("docs");
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
    if args.iter().any(|arg| arg == "--structure-test") {
        let seed = args
            .iter()
            .position(|arg| arg == "--seed")
            .and_then(|i| args.get(i + 1))
            .map_or(Ok(20111118), |s| s.parse())
            .context("--seed needs a number")?;
        let pass = rustcraft::world::structure::check::run(seed);
        std::process::exit(if pass { 0 } else { 1 });
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
            std::process::exit(err.exit_code());
        }
    };
    let event_loop = EventLoop::new()?;
    let mut app = app::App::new(cli.world)
        .with_bench(cli.bench)
        .with_view(cli.view);
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
