//! Structure templates: pieces stored as data files under
//! `assets/structures/` (`village/plains/houses/small_house.ron`), so any
//! of them can be replaced by one captured in the game.
//!
//! A template is drawn in layers, bottom first; each layer is rows from
//! north to south, each row a string of letters from west to east. `key`
//! says what each letter is; a space leaves the world as it is. The
//! bottom layer sits at ground level (it replaces the top block). Jigsaw
//! connectors say where other pieces join, which way they face out of the
//! piece, and which pool (a folder of templates) the joining piece comes
//! from.

use std::{
    collections::{BTreeMap, HashMap},
    path::Path,
    sync::LazyLock,
};

use anyhow::{Context, Result, bail};
use glam::{DVec3, IVec3};
use serde::{Deserialize, Serialize};

use super::{MobSpawn, Piece};
use crate::world::{Face, block::BlockState};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Template {
    /// Width (x, west to east), height (y) and depth (z, north to south).
    pub size: [i32; 3],
    /// How likely the pool picks it against the others in its folder.
    #[serde(default = "one")]
    pub weight: u32,
    /// What each letter stands for. A space is left as the world has it.
    pub key: BTreeMap<char, BlockState>,
    /// Bottom layer first; rows north to south; letters west to east.
    pub layers: Vec<Vec<String>>,
    /// Chests (or dispensers) and their loot tables.
    #[serde(default)]
    pub chests: Vec<([i32; 3], String)>,
    /// Spawners and the mob they make.
    #[serde(default)]
    pub spawners: Vec<([i32; 3], String)>,
    /// Mobs put in the world when the chunk first loads: where, which, and
    /// a villager's profession.
    #[serde(default)]
    pub mobs: Vec<([f64; 3], String, Option<String>)>,
    #[serde(default)]
    pub jigsaws: Vec<Jigsaw>,
    /// Fills under the bottom layer down to the ground.
    #[serde(default)]
    pub foundation: Option<BlockState>,
    /// The bottom layer is laid into the ground's top wherever the ground
    /// is (paths), instead of at one height.
    #[serde(default)]
    pub on_surface: bool,
}

fn one() -> u32 {
    1
}

/// Where another piece joins.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Jigsaw {
    pub at: [i32; 3],
    /// north, east, south or west: the way out of this piece.
    pub facing: String,
    /// The folder the joining piece comes from (`village/plains/houses`).
    pub pool: String,
}

pub fn face_named(name: &str) -> Option<Face> {
    Some(match name {
        "north" => Face::North,
        "east" => Face::East,
        "south" => Face::South,
        "west" => Face::West,
        _ => return None,
    })
}

pub fn face_name(face: Face) -> &'static str {
    match face {
        Face::North => "north",
        Face::East => "east",
        Face::South => "south",
        Face::West => "west",
        Face::Top => "up",
        Face::Bottom => "down",
    }
}

/// A connector in world space.
#[derive(Debug, Clone, PartialEq)]
pub struct PlacedJigsaw {
    pub at: IVec3,
    pub facing: Face,
    pub pool: String,
}

impl Template {
    /// Checks the layers fit `size` and use only letters in `key`.
    pub fn validate(&self) -> Result<()> {
        let [w, h, d] = self.size;
        if self.layers.len() != h as usize {
            bail!("{} layers for a height of {h}", self.layers.len());
        }
        for (y, layer) in self.layers.iter().enumerate() {
            if layer.len() != d as usize {
                bail!("layer {y} has {} rows, not {d}", layer.len());
            }
            for (z, row) in layer.iter().enumerate() {
                if row.chars().count() != w as usize {
                    bail!("layer {y} row {z} is {} long, not {w}", row.chars().count());
                }
                if let Some(c) = row.chars().find(|c| *c != ' ' && !self.key.contains_key(c)) {
                    bail!("layer {y} row {z} uses {c:?}, which isn't in the key");
                }
            }
        }
        for jigsaw in &self.jigsaws {
            if face_named(&jigsaw.facing).is_none() {
                bail!("jigsaw facing {:?}", jigsaw.facing);
            }
        }
        Ok(())
    }

    /// The size after `turns` quarter turns clockwise.
    pub fn turned_size(&self, turns: u8) -> [i32; 3] {
        let [w, h, d] = self.size;
        if turns % 2 == 1 { [d, h, w] } else { [w, h, d] }
    }

    /// Where local (x, z) lands after `turns` quarter turns, in a footprint
    /// starting at the origin.
    pub fn turn(&self, x: i32, z: i32, turns: u8) -> (i32, i32) {
        let [w, _, d] = self.size;
        match turns % 4 {
            0 => (x, z),
            1 => (d - 1 - z, x),
            2 => (w - 1 - x, d - 1 - z),
            _ => (z, w - 1 - x),
        }
    }

    fn turn_f(&self, x: f64, z: f64, turns: u8) -> (f64, f64) {
        let [w, _, d] = self.size;
        let (w, d) = (w as f64, d as f64);
        match turns % 4 {
            0 => (x, z),
            1 => (d - z, x),
            2 => (w - x, d - z),
            _ => (z, w - x),
        }
    }

    /// The connectors placed at `origin` (the turned footprint's
    /// north-west bottom corner) after `turns`.
    pub fn jigsaws_at(&self, origin: IVec3, turns: u8) -> Vec<PlacedJigsaw> {
        self.jigsaws
            .iter()
            .map(|j| {
                let (x, z) = self.turn(j.at[0], j.at[2], turns);
                let mut facing = face_named(&j.facing).unwrap_or(Face::North);
                for _ in 0..turns % 4 {
                    facing = facing.rotate_cw();
                }
                PlacedJigsaw {
                    at: origin + IVec3::new(x, j.at[1], z),
                    facing,
                    pool: j.pool.clone(),
                }
            })
            .collect()
    }

    /// The piece this makes at `origin` after `turns` quarter turns.
    pub fn place(&self, origin: IVec3, turns: u8) -> Piece {
        let mut piece = Piece::default();
        for (y, layer) in self.layers.iter().enumerate() {
            for (z, row) in layer.iter().enumerate() {
                for (x, c) in row.chars().enumerate() {
                    let Some(&block) = self.key.get(&c) else {
                        continue;
                    };
                    let (tx, tz) = self.turn(x as i32, z as i32, turns);
                    let at = origin + IVec3::new(tx, y as i32, tz);
                    let block = block.rotated(turns);
                    if self.on_surface && y == 0 {
                        piece.surface.push((at.x, at.z, block));
                        continue;
                    }
                    piece.blocks.push((at, block));
                    if y == 0
                        && let Some(foundation) = self.foundation
                    {
                        piece.foundations.push((at, foundation));
                    }
                }
            }
        }
        let local = |at: [i32; 3]| {
            let (x, z) = self.turn(at[0], at[2], turns);
            origin + IVec3::new(x, at[1], z)
        };
        piece.loot = self
            .chests
            .iter()
            .map(|(at, table)| (local(*at), table.clone()))
            .collect();
        piece.spawners = self
            .spawners
            .iter()
            .map(|(at, mob)| (local(*at), mob.clone()))
            .collect();
        piece.mobs = self
            .mobs
            .iter()
            .map(|(at, mob, tag)| {
                let (x, z) = self.turn_f(at[0], at[2], turns);
                MobSpawn {
                    mob: mob.clone(),
                    at: origin.as_dvec3() + DVec3::new(x, at[1], z),
                    tag: tag.clone(),
                }
            })
            .collect();
        piece.compute_bounds();
        piece
    }
}

/// Every template under `assets/structures`, by name (its path without
/// `.ron`), and the pools (folders) they belong to.
pub struct Registry {
    pub templates: HashMap<String, Template>,
    pub pools: HashMap<String, Vec<String>>,
}

impl Registry {
    pub fn load(dir: &Path) -> Result<Self> {
        let mut registry = Self {
            templates: HashMap::new(),
            pools: HashMap::new(),
        };
        if dir.is_dir() {
            registry.load_dir(dir, dir)?;
        }
        for names in registry.pools.values_mut() {
            names.sort();
        }
        Ok(registry)
    }

    fn load_dir(&mut self, root: &Path, dir: &Path) -> Result<()> {
        let mut entries: Vec<_> = std::fs::read_dir(dir)?
            .flatten()
            .map(|e| e.path())
            .collect();
        entries.sort();
        for path in entries {
            if path.is_dir() {
                self.load_dir(root, &path)?;
                continue;
            }
            if path.extension().is_none_or(|ext| ext != "ron") {
                continue;
            }
            let text = std::fs::read_to_string(&path)?;
            let template: Template =
                ron::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
            template
                .validate()
                .with_context(|| format!("checking {}", path.display()))?;
            let name = path
                .strip_prefix(root)?
                .with_extension("")
                .to_string_lossy()
                .replace('\\', "/");
            let pool = name
                .rsplit_once('/')
                .map_or("", |(pool, _)| pool)
                .to_owned();
            self.pools.entry(pool).or_default().push(name.clone());
            self.templates.insert(name, template);
        }
        Ok(())
    }

    pub fn get(&self, name: &str) -> Option<&Template> {
        self.templates.get(name)
    }

    /// The templates in pool `name`.
    pub fn pool(&self, name: &str) -> &[String] {
        self.pools.get(name).map_or(&[], Vec::as_slice)
    }
}

/// The game's templates, from `assets/structures`.
pub fn registry() -> &'static Registry {
    static REGISTRY: LazyLock<Registry> = LazyLock::new(|| {
        let dir = crate::assets::dir().join("structures");
        Registry::load(&dir).unwrap_or_else(|err| {
            log::error!("structure templates: {err:#}");
            Registry {
                templates: HashMap::new(),
                pools: HashMap::new(),
            }
        })
    });
    &REGISTRY
}

/// Draws a template in local coordinates, for templates made in code
/// and for the capture tool.
#[derive(Debug, Clone, Default)]
pub struct Sketch {
    pub size: [i32; 3],
    blocks: BTreeMap<(i32, i32, i32), BlockState>,
    pub chests: Vec<([i32; 3], String)>,
    pub spawners: Vec<([i32; 3], String)>,
    pub mobs: Vec<([f64; 3], String, Option<String>)>,
    pub jigsaws: Vec<Jigsaw>,
    pub foundation: Option<BlockState>,
    pub on_surface: bool,
    pub weight: u32,
}

impl Sketch {
    pub fn new(size: [i32; 3]) -> Self {
        Self {
            size,
            weight: 1,
            ..Default::default()
        }
    }

    pub fn inside(&self, x: i32, y: i32, z: i32) -> bool {
        let [w, h, d] = self.size;
        (0..w).contains(&x) && (0..h).contains(&y) && (0..d).contains(&z)
    }

    pub fn set(&mut self, x: i32, y: i32, z: i32, block: BlockState) {
        if self.inside(x, y, z) {
            self.blocks.insert((x, y, z), block);
        }
    }

    pub fn get(&self, x: i32, y: i32, z: i32) -> Option<BlockState> {
        self.blocks.get(&(x, y, z)).copied()
    }

    /// Forgets the block at (x, y, z): the world shows through.
    pub fn clear(&mut self, x: i32, y: i32, z: i32) {
        self.blocks.remove(&(x, y, z));
    }

    /// Fills the box between two corners.
    pub fn fill(&mut self, a: [i32; 3], b: [i32; 3], block: BlockState) {
        for y in a[1].min(b[1])..=a[1].max(b[1]) {
            for z in a[2].min(b[2])..=a[2].max(b[2]) {
                for x in a[0].min(b[0])..=a[0].max(b[0]) {
                    self.set(x, y, z, block);
                }
            }
        }
    }

    /// The box's sides of `wall`, its inside air.
    pub fn hollow(&mut self, a: [i32; 3], b: [i32; 3], wall: BlockState) {
        self.fill(a, b, wall);
        let (lo, hi) = (
            [a[0].min(b[0]), a[1].min(b[1]), a[2].min(b[2])],
            [a[0].max(b[0]), a[1].max(b[1]), a[2].max(b[2])],
        );
        self.fill(
            [lo[0] + 1, lo[1] + 1, lo[2] + 1],
            [hi[0] - 1, hi[1] - 1, hi[2] - 1],
            BlockState::AIR,
        );
    }

    pub fn chest(&mut self, x: i32, y: i32, z: i32, block: BlockState, table: &str) {
        self.set(x, y, z, block);
        self.chests.push(([x, y, z], table.to_owned()));
    }

    pub fn spawner(&mut self, x: i32, y: i32, z: i32, mob: &str) {
        self.set(x, y, z, BlockState::of(crate::world::blocks::SPAWNER));
        self.spawners.push(([x, y, z], mob.to_owned()));
    }

    pub fn mob(&mut self, x: f64, y: f64, z: f64, mob: &str, tag: Option<&str>) {
        self.mobs
            .push(([x, y, z], mob.to_owned(), tag.map(str::to_owned)));
    }

    pub fn jigsaw(&mut self, x: i32, y: i32, z: i32, facing: Face, pool: &str) {
        self.jigsaws.push(Jigsaw {
            at: [x, y, z],
            facing: face_name(facing).to_owned(),
            pool: pool.to_owned(),
        });
    }

    /// The template, with a letter for each different block.
    pub fn template(&self) -> Template {
        const LETTERS: &str = "#abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789@$%&*+=-~^<>?!|/()[]{};:,'\"_`";
        let mut key: BTreeMap<char, BlockState> = BTreeMap::new();
        let mut letter_of: HashMap<BlockState, char> = HashMap::new();
        let mut letters = LETTERS.chars();
        // Air is always '.'.
        let mut letter = |block: BlockState, key: &mut BTreeMap<char, BlockState>| -> char {
            if block.is_air() {
                key.insert('.', BlockState::AIR);
                return '.';
            }
            *letter_of.entry(block).or_insert_with(|| {
                let c = letters.next().expect("fewer than 90 kinds of block");
                key.insert(c, block);
                c
            })
        };
        let [w, h, d] = self.size;
        let layers = (0..h)
            .map(|y| {
                (0..d)
                    .map(|z| {
                        (0..w)
                            .map(|x| match self.get(x, y, z) {
                                Some(block) => letter(block, &mut key),
                                None => ' ',
                            })
                            .collect()
                    })
                    .collect()
            })
            .collect();
        Template {
            size: self.size,
            weight: self.weight,
            key,
            layers,
            chests: self.chests.clone(),
            spawners: self.spawners.clone(),
            mobs: self.mobs.clone(),
            jigsaws: self.jigsaws.clone(),
            foundation: self.foundation,
            on_surface: self.on_surface,
        }
    }
}

/// Writes `template` as RON to `path`.
pub fn write(template: &Template, path: &Path) -> Result<()> {
    let config = ron::ser::PrettyConfig::new()
        .depth_limit(2)
        .struct_names(false)
        .separate_tuple_members(false);
    let text = ron::ser::to_string_pretty(template, config)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, text + "\n")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::blocks;

    fn little_house() -> Template {
        let mut sketch = Sketch::new([3, 2, 4]);
        sketch.fill([0, 0, 0], [2, 0, 3], BlockState::of(blocks::COBBLESTONE));
        sketch.set(
            1,
            1,
            0,
            BlockState::parse("oak_stairs[facing=north]").unwrap(),
        );
        sketch.chest(2, 1, 3, BlockState::of(blocks::CHEST), "village_house");
        sketch.jigsaw(1, 0, 0, Face::North, "village/plains/streets");
        sketch.template()
    }

    #[test]
    fn a_sketch_round_trips_through_ron() {
        let template = little_house();
        template.validate().unwrap();
        let text = ron::to_string(&template).unwrap();
        let back: Template = ron::from_str(&text).unwrap();
        assert_eq!(back, template);
    }

    #[test]
    fn turning_a_template_turns_its_blocks_and_connectors() {
        let template = little_house();
        let origin = IVec3::new(100, 64, 200);
        // A quarter turn: 3 wide by 4 deep becomes 4 wide by 3 deep.
        assert_eq!(template.turned_size(1), [4, 2, 3]);
        let piece = template.place(origin, 1);
        let bounds = piece.bounds.unwrap();
        assert_eq!(bounds.min, origin);
        assert_eq!(bounds.max, origin + IVec3::new(3, 1, 2));
        // The stair at the north edge now faces east at the east edge.
        let stair = piece
            .blocks
            .iter()
            .find(|(_, b)| b.is(blocks::OAK_STAIRS))
            .unwrap();
        assert_eq!(stair.0, origin + IVec3::new(3, 1, 1));
        assert_eq!(stair.1.face(crate::world::props::Prop::Facing), Face::East);
        let jigsaw = &template.jigsaws_at(origin, 1)[0];
        assert_eq!(
            (jigsaw.at, jigsaw.facing),
            (origin + IVec3::new(3, 0, 1), Face::East)
        );
        // The chest moved with its loot.
        assert_eq!(piece.loot[0].0, origin + IVec3::new(0, 1, 2));
        // Every turn keeps every block inside the turned footprint.
        for turns in 0..4 {
            let [w, h, d] = template.turned_size(turns);
            let piece = template.place(IVec3::ZERO, turns);
            for (at, _) in &piece.blocks {
                assert!(at.x < w && at.y < h && at.z < d && at.min_element() >= 0);
            }
        }
    }

    #[test]
    fn bad_templates_are_caught() {
        let mut template = little_house();
        template.layers[0][0].push('#');
        assert!(template.validate().is_err());
        let mut template = little_house();
        template.layers[1][0] = "Q  ".into();
        assert!(template.validate().is_err());
    }
}
