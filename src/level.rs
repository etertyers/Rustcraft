//! Saved worlds: a folder per world under the saves folder, holding
//! `level.ron` (the `Level`: seed, game mode, player, time) and `region/`
//! (the chunks the player changed, see `world::save`).

use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result};
use log::warn;
use serde::{Deserialize, Serialize};

use crate::{player::mode::GameMode, world::save::RegionStore};

const LEVEL_FILE: &str = "level.ron";
const LEVEL_VERSION: u32 = 1;

/// Everything about a world except its chunks, like 1.0.0's `level.dat`.
/// Fields added later (inventory in M7, health and hunger in M9) get
/// `#[serde(default)]` so older saves still load.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Level {
    pub version: u32,
    pub name: String,
    pub seed: u64,
    pub game_mode: GameMode,
    /// Feet position new and fallen players start at.
    pub spawn: [f64; 3],
    pub player: PlayerData,
    /// Ticks since the world was created (the day/night cycle in M8 reads it).
    pub time: u64,
    /// Unix seconds when last saved, for sorting the world list.
    pub last_played: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlayerData {
    /// Feet position.
    pub position: [f64; 3],
    pub yaw: f32,
    pub pitch: f32,
}

impl Level {
    pub fn new(name: &str, seed: u64, game_mode: GameMode, spawn: [f64; 3]) -> Self {
        Self {
            version: LEVEL_VERSION,
            name: name.to_owned(),
            seed,
            game_mode,
            spawn,
            player: PlayerData {
                position: spawn,
                yaw: 0.0,
                pitch: 0.0,
            },
            time: 0,
            last_played: now(),
        }
    }
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// The saves folder: `$RUSTCRAFT_SAVES` if set, else `./saves`.
pub fn saves_dir() -> PathBuf {
    std::env::var_os("RUSTCRAFT_SAVES").map_or_else(|| PathBuf::from("saves"), PathBuf::from)
}

/// One world's folder.
#[derive(Debug, Clone)]
pub struct WorldDir {
    path: PathBuf,
}

impl WorldDir {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// Makes a new folder in `saves` named after `name` and writes `level` there.
    pub fn create(saves: &Path, level: &Level) -> Result<Self> {
        let folder = folder_name(&level.name, |name| saves.join(name).exists());
        let dir = Self::new(saves.join(folder));
        fs::create_dir_all(&dir.path)
            .with_context(|| format!("creating {}", dir.path.display()))?;
        dir.save_level(level)?;
        Ok(dir)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Whether the folder holds a world.
    pub fn exists(&self) -> bool {
        self.path.join(LEVEL_FILE).is_file()
    }

    pub fn load_level(&self) -> Result<Level> {
        let path = self.path.join(LEVEL_FILE);
        let text =
            fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
        ron::from_str(&text).with_context(|| format!("parsing {}", path.display()))
    }

    /// Writes `level` with the current time as `last_played`, through a
    /// temporary file so a crash mid-save keeps the old one.
    pub fn save_level(&self, level: &Level) -> Result<()> {
        let level = Level {
            last_played: now(),
            ..level.clone()
        };
        let text = ron::ser::to_string_pretty(&level, ron::ser::PrettyConfig::default())?;
        let path = self.path.join(LEVEL_FILE);
        let temp = path.with_extension("ron.tmp");
        fs::write(&temp, text).with_context(|| format!("writing {}", temp.display()))?;
        fs::rename(&temp, &path).with_context(|| format!("writing {}", path.display()))?;
        Ok(())
    }

    pub fn regions(&self) -> RegionStore {
        RegionStore::new(self.path.join("region"))
    }
}

/// Every world in `saves` that has a readable level, most recently played first.
pub fn list_worlds(saves: &Path) -> Vec<(WorldDir, Level)> {
    let Ok(entries) = fs::read_dir(saves) else {
        return Vec::new();
    };
    let mut worlds: Vec<(WorldDir, Level)> = entries
        .filter_map(|entry| {
            let dir = WorldDir::new(entry.ok()?.path());
            if !dir.exists() {
                return None;
            }
            match dir.load_level() {
                Ok(level) => Some((dir, level)),
                Err(err) => {
                    warn!("skipping world: {err:#}");
                    None
                }
            }
        })
        .collect();
    worlds.sort_by_key(|(_, level)| std::cmp::Reverse(level.last_played));
    worlds
}

/// 1.0.0's folder name for a world: the name with characters that aren't
/// allowed in file names replaced by `_`, plus `-` until it's unused.
pub fn folder_name(name: &str, exists: impl Fn(&str) -> bool) -> String {
    const ILLEGAL: &[char] = &[
        '/', '\n', '\r', '\t', '\0', '\u{c}', '`', '?', '*', '\\', '<', '>', '|', '"', ':',
    ];
    let mut folder: String = name
        .trim()
        .chars()
        .map(|c| if ILLEGAL.contains(&c) { '_' } else { c })
        .collect();
    if folder.is_empty() {
        folder = "World".to_owned();
    }
    while exists(&folder) {
        folder.push('-');
    }
    folder
}

/// 1.0.0's seed from the create-world seed box: a number is used as is,
/// other text is hashed like Java's `String.hashCode`, and an empty box
/// (or 0) gives a random seed.
pub fn parse_seed(text: &str, random: impl FnOnce() -> u64) -> u64 {
    let text = text.trim();
    if text.is_empty() {
        return random();
    }
    match text.parse::<i64>() {
        Ok(0) => random(),
        Ok(seed) => seed as u64,
        Err(_) => java_string_hash(text) as i64 as u64,
    }
}

fn java_string_hash(text: &str) -> i32 {
    text.encode_utf16().fold(0i32, |hash, unit| {
        hash.wrapping_mul(31).wrapping_add(unit as i32)
    })
}

/// A seed from the clock, for worlds created without one.
pub fn random_seed() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::save::TempDir;

    #[test]
    fn seeds_follow_1_0_0() {
        assert_eq!(parse_seed("42", || 7), 42);
        assert_eq!(parse_seed(" -5 ", || 7), (-5i64) as u64);
        assert_eq!(parse_seed("", || 7), 7);
        assert_eq!(parse_seed("0", || 7), 7);
        // Java's "hello world".hashCode() is 1794106052 and
        // "polygenelubricants".hashCode() is Integer.MIN_VALUE.
        assert_eq!(parse_seed("hello world", || 7), 1794106052);
        assert_eq!(java_string_hash("polygenelubricants"), i32::MIN);
        // Negative hashes keep their sign, like Java's int-to-long widening.
        assert_eq!(
            parse_seed("polygenelubricants", || 7),
            i32::MIN as i64 as u64
        );
    }

    #[test]
    fn folder_names_follow_1_0_0() {
        let none = |_: &str| false;
        assert_eq!(folder_name("New World", none), "New World");
        assert_eq!(folder_name("a/b:c?", none), "a_b_c_");
        assert_eq!(folder_name("   ", none), "World");
        let taken = ["New World", "New World-"];
        assert_eq!(
            folder_name("New World", |name| taken.contains(&name)),
            "New World--"
        );
    }

    #[test]
    fn levels_round_trip_and_list_newest_first() {
        let saves = TempDir::new("levels");
        let mut first = Level::new("First", 42, GameMode::Survival, [0.5, 70.0, 0.5]);
        first.player = PlayerData {
            position: [10.25, 64.0, -3.5],
            yaw: 1.5,
            pitch: -0.25,
        };
        first.time = 12345;
        let first_dir = WorldDir::create(saves.path(), &first).unwrap();
        let loaded = first_dir.load_level().unwrap();
        assert_eq!(loaded.player, first.player);
        assert_eq!((loaded.seed, loaded.time), (42, 12345));
        assert_eq!(loaded.game_mode, GameMode::Survival);

        // Same name, so a second folder; created later, so listed first.
        let mut second = Level::new("First", 7, GameMode::Creative, [0.0, 64.0, 0.0]);
        second.last_played = u64::MAX;
        let second_dir = WorldDir::create(saves.path(), &second).unwrap();
        assert!(second_dir.path().ends_with("First-"));
        let mut second_level = second_dir.load_level().unwrap();
        second_level.last_played = u64::MAX;
        fs::write(
            second_dir.path().join(LEVEL_FILE),
            ron::to_string(&second_level).unwrap(),
        )
        .unwrap();
        fs::create_dir(saves.path().join("not a world")).unwrap();

        let worlds = list_worlds(saves.path());
        let modes: Vec<GameMode> = worlds.iter().map(|(_, level)| level.game_mode).collect();
        assert_eq!(modes, [GameMode::Creative, GameMode::Survival]);
        assert!(list_worlds(&saves.path().join("missing")).is_empty());
    }
}
