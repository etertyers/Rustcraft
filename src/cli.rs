//! The command line: what the flags in CLAUDE.md §3 ask for, and the
//! usage text for anything else.

use std::fmt;

use crate::{app::WorldKind, bench, game::view::ViewMode, player::mode::GameMode, shots};

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
  --perf-test           the performance gate (writes docs/perf/m<N>.md)
  --shot <name> [--out <file>]   capture a scripted scene (--shot list)
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
    let mut end = false;
    let mut view = ViewMode::First;
    let (mut name, mut join, mut lan) = (None, None, None);
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
            "--flat" => test_world = Some(WorldKind::Flat),
            "--nether" => nether = true,
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
        // Each dimension's report has its own file.
        if let Some(path) = bench.perf_report.as_mut() {
            let suffix = match dimension {
                Dimension::Overworld => "",
                Dimension::Nether => "-nether",
                Dimension::End => "-end",
            };
            if !suffix.is_empty() {
                *path = format!("docs/perf/m{}{suffix}.md", bench::PERF_MILESTONE).into();
            }
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
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_words(words: &[&str]) -> Result<Cli, CliError> {
        parse(words.iter().map(|w| w.to_string()))
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
