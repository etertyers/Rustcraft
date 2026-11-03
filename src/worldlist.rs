//! The Select World list: what each world shows (name, game mode, last
//! played, size on disk, icon), read on a background thread so clicking
//! Singleplayer never waits on the saves folder.
//!
//! Each world keeps those few facts in a small `world.meta.ron` beside its
//! `level.ron`, rewritten whenever the level is saved. A world without one
//! (or whose level was written after it) gets one made from the head of
//! `level.ron`; nothing here opens region files or reads the player.

use std::{
    fs,
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver, TryRecvError},
    thread,
    time::Instant,
};

use image::RgbaImage;
use log::{info, warn};
use serde::{Deserialize, Serialize};

use crate::{level::WorldDir, player::mode::GameMode};

const META_FILE: &str = "world.meta.ron";
const LEVEL_FILE: &str = "level.ron";
const ICON_FILE: &str = "icon.png";

/// What the world list needs from a world, as saved in `world.meta.ron`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Meta {
    pub name: String,
    pub game_mode: GameMode,
    /// Unix seconds.
    pub last_played: u64,
    /// The (id, version) of the mods it was last played with (M39).
    #[serde(default)]
    pub mods: Vec<(String, String)>,
}

/// One row of the list.
pub struct WorldInfo {
    pub dir: WorldDir,
    pub meta: Meta,
    /// Bytes in the world's files (its folder and the folders in it).
    pub size: u64,
    pub icon: Option<RgbaImage>,
}

/// Writes `meta` for `dir`, through a temporary file like the level.
pub fn write_meta(dir: &Path, meta: &Meta) {
    let write = || -> anyhow::Result<()> {
        let text = ron::ser::to_string_pretty(meta, ron::ser::PrettyConfig::default())?;
        let path = dir.join(META_FILE);
        let temp = path.with_extension("ron.tmp");
        fs::write(&temp, text)?;
        fs::rename(&temp, &path)?;
        Ok(())
    };
    if let Err(err) = write() {
        warn!("writing {META_FILE} in {}: {err:#}", dir.display());
    }
}

/// The world's facts: its `world.meta.ron` if that is at least as new as
/// the level, else the head of `level.ron` (written back as a new meta).
pub fn read_meta(dir: &Path) -> Option<Meta> {
    let modified = |name: &str| fs::metadata(dir.join(name)).and_then(|m| m.modified()).ok();
    let (meta_time, level_time) = (modified(META_FILE), modified(LEVEL_FILE));
    if let (Some(meta_time), Some(level_time)) = (meta_time, level_time)
        && meta_time >= level_time
        && let Ok(text) = fs::read_to_string(dir.join(META_FILE))
        && let Ok(meta) = ron::from_str(&text)
    {
        return Some(meta);
    }
    let text = fs::read_to_string(dir.join(LEVEL_FILE)).ok()?;
    let meta = scan_level_head(&text).or_else(|| {
        // Not the pretty-printed layout: the slow way, ignoring the rest.
        ron::from_str(&text)
            .inspect_err(|err| warn!("skipping world {}: {err}", dir.display()))
            .ok()
    })?;
    write_meta(dir, &meta);
    Some(meta)
}

/// Finds `name`, `game_mode` and `last_played` among the fields of a
/// pretty-printed `level.ron` (the ones at one indent) without parsing the
/// rest, which can be a megabyte of populated chunks.
fn scan_level_head(text: &str) -> Option<Meta> {
    let (mut name, mut mode, mut played) = (None, None, None);
    for line in text.lines() {
        let Some(field) = line.strip_prefix("    ") else {
            continue;
        };
        let Some((key, value)) = field.split_once(": ") else {
            continue;
        };
        let value = value.trim_end().trim_end_matches(',');
        match key {
            "name" => name = ron::from_str(value).ok(),
            "game_mode" => mode = ron::from_str(value).ok(),
            "last_played" => played = value.parse().ok(),
            _ => {}
        }
        if name.is_some() && mode.is_some() && played.is_some() {
            break;
        }
    }
    Some(Meta {
        name: name?,
        game_mode: mode?,
        last_played: played?,
        mods: Vec::new(),
    })
}

/// Bytes in the files of `dir` and one level of folders below it
/// (`region/`, `DIM-1/`, ...), from their directory entries alone.
fn size_on_disk(dir: &Path) -> u64 {
    let Ok(entries) = fs::read_dir(dir) else {
        return 0;
    };
    entries
        .flatten()
        .map(|entry| match entry.metadata() {
            Ok(m) if m.is_dir() => fs::read_dir(entry.path()).map_or(0, |inner| {
                inner
                    .flatten()
                    .filter_map(|e| e.metadata().ok())
                    .filter(|m| m.is_file())
                    .map(|m| m.len())
                    .sum()
            }),
            Ok(m) => m.len(),
            Err(_) => 0,
        })
        .sum()
}

fn read_info(path: PathBuf) -> Option<WorldInfo> {
    let dir = WorldDir::new(&path);
    if !dir.exists() {
        return None;
    }
    let meta = read_meta(&path)?;
    let icon = image::open(path.join(ICON_FILE)).ok().map(|i| i.to_rgba8());
    Some(WorldInfo {
        size: size_on_disk(&path),
        icon,
        dir,
        meta,
    })
}

/// A scan of the saves folder running on its own thread; worlds arrive
/// through `poll` as they are read.
pub struct WorldScan {
    rx: Receiver<WorldInfo>,
    finished: bool,
}

impl WorldScan {
    /// Starts reading `saves` and returns at once.
    pub fn start(saves: PathBuf) -> Self {
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let started = Instant::now();
            let mut count = 0;
            for entry in fs::read_dir(&saves).into_iter().flatten().flatten() {
                if let Some(info) = read_info(entry.path()) {
                    count += 1;
                    if tx.send(info).is_err() {
                        return;
                    }
                }
            }
            info!("read {count} worlds' metadata in {:?}", started.elapsed());
        });
        Self {
            rx,
            finished: false,
        }
    }

    /// Moves the worlds read since the last call into `out`; true once
    /// every world has arrived.
    pub fn poll(&mut self, out: &mut Vec<WorldInfo>) -> bool {
        while !self.finished {
            match self.rx.try_recv() {
                Ok(info) => out.push(info),
                Err(TryRecvError::Empty) => return false,
                Err(TryRecvError::Disconnected) => self.finished = true,
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{level::Level, world::save::TempDir};
    use std::time::Duration;

    fn collect(saves: &Path) -> Vec<WorldInfo> {
        let mut scan = WorldScan::start(saves.to_owned());
        let mut worlds = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(20);
        while !scan.poll(&mut worlds) {
            assert!(Instant::now() < deadline, "the scan never finished");
            thread::sleep(Duration::from_millis(1));
        }
        worlds
    }

    #[test]
    fn a_hundred_saves_list_without_the_main_thread_waiting_or_loading_chunks() {
        let saves = TempDir::new("hundred");
        for i in 0..100 {
            let level = Level::new(&format!("World {i}"), i, GameMode::Survival, [0.0; 3]);
            let dir = WorldDir::create(saves.path(), &level).unwrap();
            // Region files that would fail to load if anything opened them.
            let region = dir.path().join("region");
            fs::create_dir_all(&region).unwrap();
            fs::write(region.join("r.0.0.bin"), vec![0xFF; 4096]).unwrap();
            // Half of them are from before the metadata file existed.
            if i % 2 == 0 {
                fs::remove_file(dir.path().join(META_FILE)).unwrap();
            }
        }
        let main_thread = Instant::now();
        let mut scan = WorldScan::start(saves.path().to_owned());
        let mut worlds = Vec::new();
        scan.poll(&mut worlds);
        let waited = main_thread.elapsed();
        assert!(waited < Duration::from_millis(50), "took {waited:?}");

        let deadline = Instant::now() + Duration::from_secs(20);
        while !scan.poll(&mut worlds) {
            assert!(Instant::now() < deadline, "the scan never finished");
            thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(worlds.len(), 100);
        assert!(worlds.iter().all(|w| w.size >= 4096));
        // The missing ones got a meta file written.
        assert!(
            fs::read_dir(saves.path()).unwrap().all(|e| e
                .unwrap()
                .path()
                .join(META_FILE)
                .is_file())
        );
    }

    #[test]
    fn the_head_of_a_level_gives_its_facts_and_a_stale_meta_is_replaced() {
        let saves = TempDir::new("head");
        let mut level = Level::new("My \"Home\"", 1, GameMode::Creative, [0.0; 3]);
        level.player.position = [1.0, 2.0, 3.0];
        let dir = WorldDir::create(saves.path(), &level).unwrap();
        let meta = read_meta(dir.path()).unwrap();
        assert_eq!(meta.name, "My \"Home\"");
        assert_eq!(meta.game_mode, GameMode::Creative);

        // A level written after the meta wins, even in another layout.
        level.name = "Renamed".into();
        std::thread::sleep(Duration::from_millis(20));
        fs::write(dir.path().join(LEVEL_FILE), ron::to_string(&level).unwrap()).unwrap();
        assert_eq!(read_meta(dir.path()).unwrap().name, "Renamed");
        assert_eq!(collect(saves.path()).len(), 1);
        assert!(collect(&saves.path().join("missing")).is_empty());
    }
}
