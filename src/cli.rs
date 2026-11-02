//! The command line: what the flags in CLAUDE.md §3 ask for, and the
//! usage text for anything else.

use std::fmt;

use crate::{
    app::WorldKind,
    bench,
    game::view::ViewMode,
    player::mode::GameMode,
    render::visuals::{LookKind, Quality},
    shots,
};

/// The scene to capture and where to save it.
pub type ShotArgs = (&'static shots::Shot, std::path::PathBuf);

/// What the command line asked for.
pub struct Cli {
    /// `None` opens the main menu.
    pub world: Option<WorldKind>,
    pub bench: Option<bench::Bench>,
    pub shot: Option<ShotArgs>,
    pub view: ViewMode,
    /// LAN: the player name for this run, a world to join, and a port to
    /// open the world to LAN on at once (0 for any).
    pub name: Option<String>,
    pub join: Option<String>,
    pub lan: Option<u16>,
    /// `--size <w>x<h>`: the window's size in pixels (for `--shot`).
    pub size: Option<(u32, u32)>,
    /// `--visuals vanilla|trailer` and `--quality low|medium|high`: the
    /// look this run, without saving it.
    pub visuals: Option<LookKind>,
    pub quality: Option<Quality>,
    /// `--shot <name> --compare`: the scene once per look, side by side.
    pub compare: bool,
    /// `--perf-test --all-looks`: Vanilla and Trailer Low, Medium and High.
    pub all_looks: bool,
}

/// The world `--creative` makes when no `--world` names one.
pub const CREATIVE_TEST_WORLD: &str = "Creative test";

const USAGE: &str = "\
Usage: cargo run --release -- [flags]
       (the `--` matters: without it cargo reads the flags itself and fails
       with \"unexpected argument '--creative' found\")

  (no flags)            open the main menu
  --creative            make a new creative world \"Creative test\" and open it
  --world <name>        open that saved world, or create it
    --seed <n>            ...with this seed (survival unless --creative)
  --seed <n>            an unsaved world from this seed
  --flat | --gallery    unsaved test worlds (flat grass, every block)
  --nether | --end      an unsaved test world in the Nether or the End
  --view first|back|front   the view to start in (F5 cycles it)
  --pack <name>         play with this resource pack once (repeatable)
  --bench [seconds] [--distance <chunks>]   timed flight, prints fps
  --perf-test           the performance gate (writes docs/perf/<label>.md and .json); --underwater flies in the sea
    --runs <n>            ...n flights in one launch; the report has each measure's median and spread
    --label <name>        ...name the results <name> (default m<N>, with -nether, -end or -underwater)
    --row <name>          ...which baseline row the results are for (default vanilla,
                            or trailer-<quality> with --visuals trailer)
    --all-looks           ...Vanilla, then Trailer at Low, Medium and High (one results file each)
  --shot <name> [--out <file>]   capture a scripted scene (--shot list)
    --size <w>x<h>        ...at this size in pixels (default 1280x720)
    --compare             ...once per look, Vanilla left and Trailer right, in one picture
  --visuals vanilla|trailer   draw with this look this time (F12 switches in game)
  --quality low|medium|high   the Trailer look's quality this time
  --name <player>       play as this name this time (Options keeps yours)
  --join <ip:port>      join a world open to LAN
  --lan [port]          open the world to LAN as soon as it's up
  --lan-test <n>        a server with n bots walking, mining and placing
                        for 60 s; prints tick times and bandwidth
  --help                this text
Tools: --write-structures, --asset-report, --gen-placeholders,
       --biome-map and --height-map [--seed <n>] [--size <px>] [--out <file>],
       --spawn-test [--seed <n>] (one night of mob counts in plains),
       --flower-test [--seed <n>] (flowers per chunk by biome)
       --nether-map [--seed <n>] [--chunks <side>] (basalt pillars and columns by Nether biome)
       --cave-test [--seed <n>] [--chunks <n>] (the underground in numbers)
       --cave-openings [--seed <n>] [--side <chunks>] (surface openings by source)
       --cave-slice --seed <n> --x <x> --z <z> --size 256 --out <file> (a cross-section picture)
       --structure-test [--seed <n>] (20 ruined portals, 20 geodes and 5 fortresses checked)
";

/// Why the command line wasn't used: a mistake, or `--help`.
#[derive(Debug, PartialEq, Eq)]
pub struct CliError {
    /// What was wrong; `None` for `--help`.
    message: Option<String>,
}

impl CliError {
    fn wrong(message: impl Into<String>) -> Self {
        Self {
            message: Some(message.into()),
        }
    }

    /// 0 after `--help`, else 2.
    pub fn exit_code(&self) -> i32 {
        if self.message.is_some() { 2 } else { 0 }
    }
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if let Some(message) = &self.message {
            writeln!(f, "error: {message}\n")?;
        }
        f.write_str(USAGE)
    }
}

impl std::error::Error for CliError {}

/// Reads the arguments after the program name.
pub fn parse(args: impl Iterator<Item = String>) -> Result<Cli, CliError> {
    let mut args = args.peekable();
    let (mut seed, mut world, mut creative) = (None, None, false);
    let mut bench = None;
    let (mut shot, mut out) = (None, None);
    let mut test_world = None;
    let mut nether = false;
    let mut underwater = false;
    let mut end = false;
    let (mut runs, mut label, mut row, mut perf_out) = (1u32, None, None, None);
    let mut view = ViewMode::First;
    let (mut name, mut join, mut lan) = (None, None, None);
    let mut size = None;
    let (mut visuals, mut quality, mut compare, mut all_looks) = (None, None, false, false);
    fn value(
        args: &mut impl Iterator<Item = String>,
        flag: &str,
        what: &str,
    ) -> Result<String, CliError> {
        args.next()
            .ok_or_else(|| CliError::wrong(format!("{flag} needs {what}")))
    }
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-h" | "--help" => return Err(CliError { message: None }),
            "--shot" => {
                let name = value(&mut args, "--shot", "a scene name")?;
                if name == "list" {
                    for shot in shots::SHOTS {
                        println!("{:28} {}", shot.name, shot.about);
                    }
                    std::process::exit(0);
                }
                shot = Some(shots::find(&name).ok_or_else(|| {
                    CliError::wrong(format!("no scene {name:?} (try --shot list)"))
                })?);
            }
            "--out" => out = Some(value(&mut args, "--out", "a file")?.into()),
            "--compare" => compare = true,
            "--all-looks" => all_looks = true,
            "--visuals" => {
                let text = value(&mut args, "--visuals", "vanilla or trailer")?;
                visuals = Some(LookKind::parse(&text).ok_or_else(|| {
                    CliError::wrong(format!("bad visuals {text:?} (vanilla or trailer)"))
                })?);
            }
            "--quality" => {
                let text = value(&mut args, "--quality", "low, medium or high")?;
                quality = Some(Quality::parse(&text).ok_or_else(|| {
                    CliError::wrong(format!("bad quality {text:?} (low, medium or high)"))
                })?);
            }
            "--size" => {
                let text = value(&mut args, "--size", "a size like 640x360")?;
                size =
                    Some(parse_size(&text).ok_or_else(|| {
                        CliError::wrong(format!("bad size {text:?} (like 640x360)"))
                    })?);
            }
            "--bench" => {
                let seconds = match args.next_if(|next| !next.starts_with("--")) {
                    Some(text) => text
                        .parse()
                        .map_err(|_| CliError::wrong(format!("bad bench length {text:?}")))?,
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
                let text = value(&mut args, "--distance", "a number of chunks")?;
                let distance = text
                    .parse()
                    .map_err(|_| CliError::wrong(format!("bad render distance {text:?}")))?;
                bench
                    .get_or_insert_with(|| bench::Bench::new(bench::DEFAULT_SECONDS))
                    .distance = Some(distance);
            }
            "--view" => {
                let text = value(&mut args, "--view", "first, back or front")?;
                view = match text.as_str() {
                    "first" => ViewMode::First,
                    "back" => ViewMode::ThirdBack,
                    "front" => ViewMode::ThirdFront,
                    other => {
                        return Err(CliError::wrong(format!(
                            "bad view {other:?} (first, back or front)"
                        )));
                    }
                };
            }
            "--name" => {
                let text = value(&mut args, "--name", "a player name")?;
                if !crate::options::valid_name(&text) {
                    return Err(CliError::wrong(format!(
                        "bad name {text:?} (3 to 16 letters, digits or underscores)"
                    )));
                }
                name = Some(text);
            }
            "--join" => {
                join = Some(value(
                    &mut args,
                    "--join",
                    "an address like 192.168.1.5:51234",
                )?)
            }
            "--lan" => {
                lan = Some(match args.next_if(|next| !next.starts_with("--")) {
                    Some(text) => text
                        .parse()
                        .map_err(|_| CliError::wrong(format!("bad port {text:?}")))?,
                    None => 0,
                });
            }
            "--gallery" => test_world = Some(WorldKind::Gallery),
            // Read in `main`.
            "--pack" => {
                value(&mut args, "--pack", "a resource pack's name")?;
            }
            "--runs" => {
                let text = value(&mut args, "--runs", "a number")?;
                runs = text
                    .parse()
                    .ok()
                    .filter(|&n| n >= 1)
                    .ok_or_else(|| CliError::wrong(format!("bad number of runs {text:?}")))?;
            }
            "--label" => label = Some(value(&mut args, "--label", "a name")?),
            "--row" => row = Some(value(&mut args, "--row", "a name")?),
            "--perf-out" => perf_out = Some(value(&mut args, "--perf-out", "a file")?.into()),
            "--flat" => test_world = Some(WorldKind::Flat),
            "--nether" => nether = true,
            "--underwater" => underwater = true,
            "--end" => end = true,
            "--world" => world = Some(value(&mut args, "--world", "a name")?),
            "--creative" => creative = true,
            "--seed" => {
                let text = value(&mut args, "--seed", "a number")?;
                seed = Some(
                    text.parse()
                        .map_err(|_| CliError::wrong(format!("bad seed {text:?}")))?,
                );
            }
            other => {
                return Err(CliError::wrong(if other.starts_with('-') {
                    format!("unknown flag {other:?}")
                } else {
                    format!("unexpected argument {other:?}")
                }));
            }
        }
    }
    let mode = if creative {
        GameMode::Creative
    } else {
        GameMode::Survival
    };
    let shot = shot.map(|shot: &'static shots::Shot| {
        let out = out.unwrap_or_else(|| format!("docs/screenshots/{}.png", shot.name).into());
        (shot, out)
    });
    let bench_world = bench.as_mut().map(|bench| {
        use crate::world::dimension::Dimension;
        let seed = seed.unwrap_or(bench::SEED);
        let (dimension, kind) = if nether {
            (Dimension::Nether, WorldKind::Nether { seed })
        } else if end {
            (Dimension::End, WorldKind::End { seed })
        } else {
            (Dimension::Overworld, WorldKind::Generated { seed })
        };
        bench.dimension = dimension;
        bench.underwater = underwater && dimension == Dimension::Overworld;
        bench.runs = runs;
        bench.label = label;
        bench.row = row.unwrap_or_else(|| match visuals {
            Some(LookKind::Trailer) => quality.unwrap_or_default().row(),
            _ => bench.row.clone(),
        });
        bench.run_out = perf_out;
        // Each dimension's report has its own file.
        let name = bench.perf_label();
        if let Some(path) = bench.perf_report.as_mut() {
            *path = format!("docs/perf/{name}.md").into();
        }
        kind
    });
    if let Some(kind) = bench_world {
        return Ok(Cli {
            world: Some(kind),
            bench,
            shot: None,
            view,
            name,
            join: None,
            lan: None,
            size: None,
            visuals,
            quality,
            compare: false,
            all_looks,
        });
    }
    let world = match (world, seed) {
        _ if nether => Some(WorldKind::Nether {
            seed: seed.unwrap_or(bench::SEED),
        }),
        _ if end => Some(WorldKind::End {
            seed: seed.unwrap_or(bench::SEED),
        }),
        _ if test_world.is_some() => test_world,
        (Some(name), seed) => Some(WorldKind::Saved { name, seed, mode }),
        // `--creative` alone: a fresh world straight away.
        (None, None) if creative => Some(WorldKind::New {
            name: CREATIVE_TEST_WORLD.to_owned(),
            mode,
        }),
        (None, Some(seed)) => Some(WorldKind::Generated { seed }),
        (None, None) => None,
    };
    Ok(Cli {
        world,
        bench: None,
        shot,
        view,
        name,
        join,
        lan,
        size,
        visuals,
        quality,
        compare,
        all_looks: false,
    })
}

/// `640x360` as (640, 360).
fn parse_size(text: &str) -> Option<(u32, u32)> {
    let (w, h) = text.split_once('x')?;
    let size = (w.parse().ok()?, h.parse().ok()?);
    (size.0 > 0 && size.1 > 0).then_some(size)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_words(words: &[&str]) -> Result<Cli, CliError> {
        parse(words.iter().map(|w| w.to_string()))
    }

    #[test]
    fn perf_test_takes_runs_a_label_and_a_row() {
        let cli = parse_words(&[
            "--perf-test",
            "--runs",
            "3",
            "--label",
            "x",
            "--row",
            "trailer",
        ])
        .unwrap();
        let bench = cli.bench.unwrap();
        assert_eq!((bench.runs, bench.row.as_str()), (3, "trailer"));
        assert_eq!(bench.perf_report.unwrap().to_str(), Some("docs/perf/x.md"));
        let plain = parse_words(&["--perf-test", "--nether"])
            .unwrap()
            .bench
            .unwrap();
        assert_eq!((plain.runs, plain.row.as_str()), (1, "vanilla"));
        assert_eq!(
            plain.perf_label(),
            format!("m{}-nether", bench::PERF_MILESTONE)
        );
        assert!(parse_words(&["--perf-test", "--runs", "0"]).is_err());
        let trailer =
            parse_words(&["--perf-test", "--visuals", "trailer", "--quality", "high"]).unwrap();
        assert_eq!(trailer.bench.unwrap().row, "trailer-high");
        assert_eq!(trailer.visuals, Some(LookKind::Trailer));
        let shot =
            parse_words(&["--shot", "forest-noon", "--compare", "--size", "640x360"]).unwrap();
        assert!(shot.compare && shot.size == Some((640, 360)));
        assert!(parse_words(&["--visuals", "fancy"]).is_err());
        assert!(
            parse_words(&["--perf-test", "--all-looks"])
                .unwrap()
                .all_looks
        );
    }

    #[test]
    fn creative_alone_makes_a_new_creative_world_and_skips_the_menu() {
        let cli = parse_words(&["--creative"]).unwrap();
        assert_eq!(
            cli.world,
            Some(WorldKind::New {
                name: "Creative test".into(),
                mode: GameMode::Creative
            })
        );
    }

    #[test]
    fn world_opens_or_creates_that_world_in_the_chosen_mode() {
        let saved = |name: &str, seed, mode| {
            Some(WorldKind::Saved {
                name: name.into(),
                seed,
                mode,
            })
        };
        let both = parse_words(&["--world", "base", "--creative"]).unwrap();
        assert_eq!(both.world, saved("base", None, GameMode::Creative));
        // The order of the flags doesn't matter.
        let flipped = parse_words(&["--creative", "--world", "base", "--seed", "7"]).unwrap();
        assert_eq!(flipped.world, saved("base", Some(7), GameMode::Creative));
        let survival = parse_words(&["--world", "base"]).unwrap();
        assert_eq!(survival.world, saved("base", None, GameMode::Survival));
        assert!(parse_words(&[]).unwrap().world.is_none());
    }

    #[test]
    fn unknown_flags_and_stray_words_print_usage_that_explains_cargos_dashes() {
        for words in [&["--frobnicate"][..], &["creative"], &["--world"]] {
            let err = parse_words(words).err().expect("should be refused");
            assert_eq!(err.exit_code(), 2);
            let text = err.to_string();
            assert!(text.starts_with("error: "), "{text}");
            assert!(text.contains("Usage:"));
            assert!(text.contains("cargo run --release -- [flags]"));
            assert!(text.contains("unexpected argument '--creative' found"));
        }
        let help = parse_words(&["--help"]).err().unwrap();
        assert_eq!(help.exit_code(), 0);
        assert!(help.to_string().starts_with("Usage:"));
    }
}
