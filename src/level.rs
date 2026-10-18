//! Saved worlds: a folder per world under the saves folder, holding
//! `level.ron` (the `Level`: seed, game mode, player, time) and each
//! dimension's region files (the chunks the player changed, see
//! `world::save`): the Overworld's in `region/`, as in Minecraft.
//!
//! v1.0 worlds (level version 1) store 1.0.0's numeric IDs. They open
//! after a one-time upgrade (`WorldDir::migrate_v1`) that keeps a copy of
//! the old files in `backup-v1.0/`.

use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result};
use log::warn;
use serde::{Deserialize, Serialize};

use crate::{
    inventory::player::Inventory,
    player::{mode::GameMode, stats::Stats},
    world::{dimension::Dimension, legacy, save::RegionStore},
};

const LEVEL_FILE: &str = "level.ron";
/// 2 since v1.1 (blocks and items by name); 1 was v1.0.
pub const LEVEL_VERSION: u32 = 2;
/// Where an upgraded v1.0 world's original files are kept.
const BACKUP_FOLDER: &str = "backup-v1.0";

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
    /// Animals (added in M9).
    #[serde(default)]
    pub mobs: Vec<SavedMob>,
    /// Chunks whose animals have been placed (added in M9).
    #[serde(default)]
    pub populated: Vec<(i32, i32)>,
    /// The dimension the player is in (added in v1.1).
    #[serde(default)]
    pub dimension: Dimension,
}

/// A mob as saved in level.ron.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SavedMob {
    pub name: String,
    pub position: [f64; 3],
    pub yaw: f32,
    pub health: i32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlayerData {
    /// Feet position.
    pub position: [f64; 3],
    pub yaw: f32,
    pub pitch: f32,
    /// Added in M7.
    #[serde(default)]
    pub inventory: Inventory,
    /// Creative flight (added in M7).
    #[serde(default)]
    pub flying: bool,
    /// Health, hunger, air and fire (added in M9).
    #[serde(default)]
    pub stats: Stats,
    /// The head of the bed the player last slept in, where they respawn
    /// (added in M10).
    #[serde(default)]
    pub bed: Option<[i32; 3]>,
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
                inventory: Inventory::default(),
                flying: false,
                stats: Stats::default(),
                bed: None,
            },
            time: 0,
            last_played: now(),
            mobs: Vec::new(),
            populated: Vec::new(),
            dimension: Dimension::Overworld,
        }
    }
}

/// Reads a `level.ron`, upgrading a v1.0 one's item stacks in memory (its
/// version stays 1 until `WorldDir::migrate_v1` has upgraded its chunks).
fn read_level(text: &str) -> Result<Level> {
    let value: ron::Value = ron::from_str(text)?;
    let version = match &value {
        ron::Value::Map(map) => map.iter().find_map(|(key, value)| match (key, value) {
            (ron::Value::String(key), number) if key == "version" => {
                number.clone().into_rust::<u32>().ok()
            }
            _ => None,
        }),
        _ => None,
    };
    match version.unwrap_or(0) {
        1 => Ok(ron::from_str(&legacy::upgrade_level(text)?)?),
        LEVEL_VERSION => Ok(ron::from_str(text)?),
        other => anyhow::bail!("level version {other} is newer than this game"),
    }
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// The saves folder: `$RUSTCRAFT_SAVES` if set, else
/// `~/Library/Application Support/Rustcraft/saves` in the app bundle, else
/// `./saves`. Options are kept beside it.
pub fn saves_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("RUSTCRAFT_SAVES") {
        return dir.into();
    }
    match crate::assets::support_dir().filter(|_| crate::assets::in_app_bundle()) {
        Some(support) => support.join("saves"),
        None => PathBuf::from("saves"),
    }
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
        read_level(&text).with_context(|| format!("parsing {}", path.display()))
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

    /// The region files of `dimension`.
    pub fn regions(&self, dimension: Dimension) -> RegionStore {
        RegionStore::new(self.path.join(dimension.region_folder()))
    }

    /// Upgrades a v1.0 world (`level`, read by `load_level`) to today's
    /// format, once: copies `level.ron` and `region/` into `backup-v1.0/`,
    /// rewrites every region file with modern block names, then saves the
    /// level as version 2. If it's cut short it carries on next time:
    /// region files already upgraded are left alone and the level is
    /// written last.
    pub fn migrate_v1(&self, level: &mut Level) -> Result<()> {
        if level.version >= LEVEL_VERSION {
            return Ok(());
        }
        log::info!("upgrading {:?} from v1.0", level.name);
        let backup = self.path.join(BACKUP_FOLDER);
        if !backup.exists() {
            let temp = self.path.join(format!("{BACKUP_FOLDER}.tmp"));
            let _ = fs::remove_dir_all(&temp);
            fs::create_dir_all(&temp)?;
            fs::copy(self.path.join(LEVEL_FILE), temp.join(LEVEL_FILE))?;
            let region = self.path.join("region");
            if region.is_dir() {
                copy_dir(&region, &temp.join("region"))?;
            }
            fs::rename(&temp, &backup)?;
        }
        let chunks = self
            .regions(Dimension::Overworld)
            .upgrade_v1(legacy::decode_chunk)
            .context("upgrading the chunks")?;
        level.version = LEVEL_VERSION;
        level.dimension = Dimension::Overworld;
        self.save_level(level)?;
        log::info!("upgraded {chunks} chunks; the v1.0 files are in {BACKUP_FOLDER}/");
        Ok(())
    }
}

/// Copies the files directly in `from` into a new folder `to`.
fn copy_dir(from: &Path, to: &Path) -> Result<()> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        if entry.file_type()?.is_file() {
            fs::copy(entry.path(), to.join(entry.file_name()))?;
        }
    }
    Ok(())
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
            bed: Some([3, 64, -7]),
            ..first.player
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

    #[test]
    fn a_v1_world_is_upgraded_once_and_keeps_its_blocks_items_and_player() {
        use crate::{
            inventory::{items, stack::ItemStack},
            world::{
                Face,
                block::BlockState,
                blocks,
                chunk::ChunkPos,
                legacy::v1,
                props::Prop,
                save::{REGION_CHUNKS, RegionStore, write_region_version},
            },
        };
        let saves = TempDir::new("v1-world");
        let dir = saves.path().join("Old World");
        fs::create_dir_all(dir.join("region")).unwrap();
        fs::write(
            dir.join(LEVEL_FILE),
            r#"(version: 1, name: "Old World", seed: 5, game_mode: Survival, spawn: (0.5, 64.0, 0.5),
               player: (position: (3.5, 64.0, 3.5), yaw: 0.5, pitch: 0.0,
                   inventory: (slots: [Some((id: 278, count: 1, damage: 40)), Some((id: 351, count: 7, damage: 4))], selected: 1)),
               time: 1234, last_played: 0)"#,
        )
        .unwrap();
        // A stone floor; a chest facing south holding a worn pickaxe and red
        // wool; a torch on the wall of a stone block; a lit furnace facing
        // east; ripe wheat on wet farmland; two fence posts side by side.
        let mut floor: Vec<_> = (0..16)
            .flat_map(|x| (0..16).map(move |z| (x, 63, z, 1u16, 0u8)))
            .collect();
        floor.extend([
            (2, 64, 2, 54, 3),
            (4, 64, 5, 1, 0),
            (5, 64, 5, 50, 1),
            (7, 64, 2, 62, 5),
            (9, 63, 9, 60, 7),
            (9, 64, 9, 59, 7),
            (8, 64, 12, 85, 0),
            (9, 64, 12, 85, 0),
            (3, 127, 3, 4, 0),
        ]);
        let chest = v1::Entity::Chest(
            (0..27)
                .map(|i| match i {
                    0 => Some(v1::Stack {
                        id: 278,
                        count: 1,
                        damage: 40,
                    }),
                    5 => Some(v1::Stack {
                        id: 35,
                        count: 5,
                        damage: 14,
                    }),
                    _ => None,
                })
                .collect(),
        );
        let mut blobs = vec![None; REGION_CHUNKS];
        blobs[RegionStore::slot(ChunkPos::new(0, 0))] = Some(v1::chunk(
            ChunkPos::new(0, 0),
            &floor,
            vec![([2, 64, 2], chest)],
        ));
        write_region_version(&dir.join("region/r.0.0.rcr"), &blobs, 1).unwrap();

        // Listing reads it without changing anything.
        let worlds = list_worlds(saves.path());
        assert_eq!(worlds.len(), 1);
        assert_eq!(worlds[0].1.version, 1);
        let world = WorldDir::new(&dir);
        let mut level = world.load_level().unwrap();
        world.migrate_v1(&mut level).unwrap();
        assert_eq!(level.version, LEVEL_VERSION);
        assert!(dir.join("backup-v1.0/level.ron").is_file());
        assert!(dir.join("backup-v1.0/region/r.0.0.rcr").is_file());

        let reopened = world.load_level().unwrap();
        assert_eq!(reopened.version, LEVEL_VERSION);
        assert_eq!(reopened.time, 1234);
        assert_eq!(reopened.player.position, [3.5, 64.0, 3.5]);
        let inventory = &reopened.player.inventory;
        assert_eq!(
            inventory.slot(0),
            Some(&ItemStack::with_damage(items::DIAMOND_PICKAXE, 1, 40))
        );
        assert_eq!(
            inventory.slot(1),
            Some(&ItemStack::new(items::LAPIS_LAZULI, 7))
        );

        let chunk = world
            .regions(Dimension::Overworld)
            .load(ChunkPos::new(0, 0))
            .unwrap()
            .unwrap();
        assert_eq!(chunk.get(0, 63, 0), BlockState::of(blocks::STONE));
        // v1.0's top layer stays; above it, new air.
        assert_eq!(chunk.get(3, 127, 3), BlockState::of(blocks::COBBLESTONE));
        assert!(chunk.get(3, 128, 3).is_air() && chunk.section_is_empty(8));
        assert_eq!(
            chunk.get(2, 64, 2),
            BlockState::of(blocks::CHEST).with_face(Prop::Facing, Face::South)
        );
        assert_eq!(
            chunk.get(5, 64, 5),
            BlockState::of(blocks::WALL_TORCH).with_face(Prop::Facing, Face::East)
        );
        let furnace = chunk.get(7, 64, 2);
        assert!(furnace.is(blocks::FURNACE) && furnace.bool(Prop::Lit));
        assert_eq!(furnace.face(Prop::Facing), Face::East);
        assert_eq!(chunk.get(9, 64, 9).number(Prop::Age7), 7);
        assert_eq!(chunk.get(9, 63, 9).number(Prop::Moisture), 7);
        let held: Vec<_> = chunk.block_entity([2, 64, 2]).unwrap().contents();
        assert_eq!(
            held,
            [
                ItemStack::with_damage(items::DIAMOND_PICKAXE, 1, 40),
                ItemStack::new(items::RED_WOOL, 5)
            ]
        );
        // The fences' connections are worked out once their neighbors load.
        assert!(chunk.post_process.contains(&[8, 64, 12]));
        assert!(chunk.post_process.contains(&[9, 64, 12]));

        // A second open changes nothing.
        let backup = fs::read(dir.join("backup-v1.0/level.ron")).unwrap();
        let mut level = world.load_level().unwrap();
        world.migrate_v1(&mut level).unwrap();
        assert_eq!(fs::read(dir.join("backup-v1.0/level.ron")).unwrap(), backup);
    }

    /// Upgrades copies of real v1.0 worlds: point `RUSTCRAFT_V1_SAVES` at
    /// a folder of them (they're changed in place) and run with `--ignored`.
    #[test]
    #[ignore]
    fn real_v1_worlds_upgrade_and_every_chunk_loads() {
        use crate::world::chunk::ChunkPos;
        let saves = std::env::var_os("RUSTCRAFT_V1_SAVES").expect("RUSTCRAFT_V1_SAVES");
        let worlds = list_worlds(Path::new(&saves));
        assert!(!worlds.is_empty());
        for (dir, mut level) in worlds {
            dir.migrate_v1(&mut level).unwrap();
            let store = dir.regions(Dimension::Overworld);
            let mut chunks = 0;
            let Ok(files) = fs::read_dir(dir.path().join("region")) else {
                continue;
            };
            for entry in files {
                let name = entry.unwrap().file_name().into_string().unwrap();
                let parts: Vec<&str> = name.split('.').collect();
                let (rx, rz): (i32, i32) = (parts[1].parse().unwrap(), parts[2].parse().unwrap());
                for slot in 0..32 * 32 {
                    let pos = ChunkPos::new(rx * 32 + slot % 32, rz * 32 + slot / 32);
                    if store.load(pos).unwrap().is_some() {
                        chunks += 1;
                    }
                }
            }
            println!("{}: {chunks} chunks upgraded", level.name);
            assert!(dir.load_level().unwrap().version == LEVEL_VERSION);
        }
    }

    #[test]
    fn m6_levels_without_an_inventory_still_load() {
        let saves = TempDir::new("old-level");
        let dir = saves.path().join("Old");
        fs::create_dir(&dir).unwrap();
        fs::write(
            dir.join(LEVEL_FILE),
            r#"(version: 1, name: "Old", seed: 5, game_mode: Survival, spawn: (0.5, 70.0, 0.5),
               player: (position: (1.0, 70.0, 2.0), yaw: 0.0, pitch: 0.0), time: 10, last_played: 0)"#,
        )
        .unwrap();
        let level = WorldDir::new(dir).load_level().unwrap();
        assert_eq!(level.player.inventory, Inventory::default());
        assert!(!level.player.flying);
    }
}
