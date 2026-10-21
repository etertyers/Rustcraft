//! Structures: villages, desert and jungle temples, shipwrecks, ruined
//! portals, dungeons, mineshafts and strongholds; in the Nether (M25),
//! fortresses, bastion remnants and ruined portals (see `nether`).
//!
//! Each kind decides from the world seed alone where it may start (one
//! candidate per region of chunks, as modern `RandomSpreadStructurePlacement`
//! does) and lays itself out as pieces of blocks in world space, asking only
//! the pure terrain functions. A chunk being generated finds the structures
//! whose pieces reach it and writes the part inside itself, so the result
//! doesn't depend on which chunks are loaded or in what order. Layouts are
//! cached per worker thread, since every chunk near a structure asks for it.
//!
//! Villages and the surface structures are built from templates
//! (`assets/structures/**/*.ron`, see `template`) joined jigsaw-style;
//! dungeons, mineshafts and strongholds are laid out in code.

pub mod bastion;
pub mod build;
pub mod designs;
pub mod dungeon;
pub mod fortress;
pub mod fossil;
pub mod jigsaw;
pub mod loot;
pub mod mineshaft;
pub mod nether;
pub mod stronghold;
pub mod surface;
pub mod template;
pub mod village;

use std::{cell::RefCell, collections::HashMap, sync::Arc};

use glam::{DVec3, IVec3};

use super::{
    block::BlockState,
    block_entity::{BlockEntity, Spawner},
    blocks,
    chunk::{Chunk, ChunkPos},
    worldgen::{rng::Rng, terrain::Terrain},
};
use crate::config::{CHUNK_SIZE, WORLD_HEIGHT};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    Village,
    DesertTemple,
    JungleTemple,
    Shipwreck,
    RuinedPortal,
    Dungeon,
    Mineshaft,
    Stronghold,
    /// The Nether's (M25), placed by `nether` rather than `near`.
    Fortress,
    Bastion,
    NetherRuinedPortal,
    NetherFossil,
}

impl Kind {
    pub const ALL: [Kind; 8] = [
        Kind::Village,
        Kind::DesertTemple,
        Kind::JungleTemple,
        Kind::Shipwreck,
        Kind::RuinedPortal,
        Kind::Dungeon,
        Kind::Mineshaft,
        Kind::Stronghold,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Kind::Village => "village",
            Kind::DesertTemple => "desert temple",
            Kind::JungleTemple => "jungle temple",
            Kind::Shipwreck => "shipwreck",
            Kind::RuinedPortal => "ruined portal",
            Kind::Dungeon => "dungeon",
            Kind::Mineshaft => "mineshaft",
            Kind::Stronghold => "stronghold",
            Kind::Fortress => "nether fortress",
            Kind::Bastion => "bastion remnant",
            Kind::NetherRuinedPortal => "ruined portal (Nether)",
            Kind::NetherFossil => "nether fossil",
        }
    }

    /// Farthest any block reaches from the start chunk, in chunks.
    fn reach(self) -> i32 {
        match self {
            Kind::Village => 7,
            Kind::DesertTemple | Kind::JungleTemple => 2,
            Kind::Shipwreck | Kind::RuinedPortal => 2,
            Kind::Dungeon => 1,
            Kind::Mineshaft => 6,
            Kind::Stronghold => 8,
            Kind::Fortress => 8,
            Kind::Bastion => 4,
            Kind::NetherRuinedPortal => 2,
            Kind::NetherFossil => 1,
        }
    }

    /// Whether trees and plants keep out of it (it stands on the surface).
    fn on_surface(self) -> bool {
        !matches!(
            self,
            Kind::Dungeon
                | Kind::Mineshaft
                | Kind::Stronghold
                | Kind::Fortress
                | Kind::Bastion
                | Kind::NetherFossil
        )
    }

    fn salt(self) -> u64 {
        match self {
            Kind::Village => 10_387_312,
            Kind::DesertTemple => 14_357_617,
            Kind::JungleTemple => 14_357_619,
            Kind::Shipwreck => 165_745_295,
            Kind::RuinedPortal => 34_222_645,
            Kind::Dungeon => 0xD0_6E07,
            Kind::Mineshaft => 0x313E,
            Kind::Stronghold => 0x57_0E06,
            Kind::Fortress | Kind::Bastion => nether::COMPLEXES.salt,
            Kind::NetherRuinedPortal => nether::RUINED_PORTALS.salt,
            Kind::NetherFossil => nether::FOSSILS.salt,
        }
    }

    /// Lays out the structure that starts in chunk `start`, if one does.
    fn generate(self, terrain: &Terrain, start: ChunkPos) -> Option<Structure> {
        let mut rng = Rng::for_chunk(terrain.seed(), start.x, start.z, self.salt() ^ 0x5EED);
        let pieces = match self {
            Kind::Village => village::generate(terrain, start, &mut rng)?,
            Kind::DesertTemple | Kind::JungleTemple | Kind::Shipwreck | Kind::RuinedPortal => {
                surface::generate(self, terrain, start, &mut rng)?
            }
            Kind::Dungeon => dungeon::generate(terrain, start, &mut rng)?,
            Kind::Mineshaft => mineshaft::generate(terrain, start, &mut rng)?,
            Kind::Stronghold => stronghold::generate(terrain, start, &mut rng)?,
            Kind::Fortress | Kind::Bastion | Kind::NetherRuinedPortal | Kind::NetherFossil => {
                return None;
            }
        };
        Structure::new(self, start, pieces)
    }

    /// The chunks within `reach` of `pos` where one of these may start.
    fn starts_near(self, seed: u64, pos: ChunkPos) -> Vec<ChunkPos> {
        let reach = self.reach();
        let near =
            |start: &ChunkPos| (start.x - pos.x).abs() <= reach && (start.z - pos.z).abs() <= reach;
        match self {
            Kind::Stronghold => stronghold::positions(seed)
                .into_iter()
                .filter(near)
                .collect(),
            Kind::Dungeon | Kind::Mineshaft => {
                let chance = if self == Kind::Dungeon {
                    dungeon::CHANCE
                } else {
                    mineshaft::CHANCE
                };
                let mut found = Vec::new();
                for z in pos.z - reach..=pos.z + reach {
                    for x in pos.x - reach..=pos.x + reach {
                        let mut rng = Rng::for_chunk(seed, x, z, self.salt());
                        if rng.next_f64() < chance {
                            found.push(ChunkPos::new(x, z));
                        }
                    }
                }
                found
            }
            _ => {
                let spread = self.spread();
                let region = |c: i32| c.div_euclid(spread.spacing);
                let mut found = Vec::new();
                for rz in region(pos.z - reach)..=region(pos.z + reach) {
                    for rx in region(pos.x - reach)..=region(pos.x + reach) {
                        let start = spread.candidate(seed, self.salt(), rx, rz);
                        if near(&start) {
                            found.push(start);
                        }
                    }
                }
                found
            }
        }
    }

    pub(super) fn spread(self) -> Spread {
        match self {
            Kind::Village => Spread::new(34, 8),
            Kind::DesertTemple | Kind::JungleTemple => Spread::new(32, 8),
            Kind::Shipwreck => Spread::new(24, 4),
            Kind::RuinedPortal => Spread::new(40, 15),
            Kind::NetherRuinedPortal => nether::RUINED_PORTALS.spread(),
            Kind::Fortress | Kind::Bastion => nether::COMPLEXES.spread(),
            Kind::NetherFossil => nether::FOSSILS.spread(),
            Kind::Dungeon | Kind::Mineshaft | Kind::Stronghold => Spread::new(1, 0),
        }
    }
}

/// Modern `RandomSpreadStructurePlacement`: one candidate per region of
/// `spacing` × `spacing` chunks, kept `separation` chunks from the next
/// region's.
#[derive(Debug, Clone, Copy)]
pub(super) struct Spread {
    spacing: i32,
    separation: i32,
}

impl Spread {
    const fn new(spacing: i32, separation: i32) -> Self {
        Self {
            spacing,
            separation,
        }
    }

    pub(super) fn candidate(self, seed: u64, salt: u64, rx: i32, rz: i32) -> ChunkPos {
        let mut rng = Rng::for_chunk(seed, rx, rz, salt);
        let room = (self.spacing - self.separation).max(1) as u32;
        ChunkPos::new(
            rx * self.spacing + rng.below(room) as i32,
            rz * self.spacing + rng.below(room) as i32,
        )
    }
}

/// An inclusive box of blocks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bounds {
    pub min: IVec3,
    pub max: IVec3,
}

impl Bounds {
    pub fn new(a: IVec3, b: IVec3) -> Self {
        Self {
            min: a.min(b),
            max: a.max(b),
        }
    }

    pub fn union(self, other: Bounds) -> Self {
        Self::new(self.min.min(other.min), self.max.max(other.max))
    }

    pub fn intersects(&self, other: &Bounds) -> bool {
        self.min.cmple(other.max).all() && self.max.cmpge(other.min).all()
    }

    /// Overlap seen from above, ignoring height.
    pub fn intersects_xz(&self, other: &Bounds) -> bool {
        self.min.x <= other.max.x
            && self.max.x >= other.min.x
            && self.min.z <= other.max.z
            && self.max.z >= other.min.z
    }

    pub fn contains(&self, p: IVec3) -> bool {
        p.cmpge(self.min).all() && p.cmple(self.max).all()
    }

    pub fn contains_column(&self, x: i32, z: i32) -> bool {
        (self.min.x..=self.max.x).contains(&x) && (self.min.z..=self.max.z).contains(&z)
    }

    pub fn grow(self, by: i32) -> Self {
        Self::new(self.min - IVec3::splat(by), self.max + IVec3::splat(by))
    }

    pub fn touches_chunk(&self, pos: ChunkPos) -> bool {
        let (x, z) = pos.min_block();
        let size = CHUNK_SIZE as i32 - 1;
        self.min.x <= x + size && self.max.x >= x && self.min.z <= z + size && self.max.z >= z
    }

    pub fn center(&self) -> IVec3 {
        (self.min + self.max) / 2
    }
}

/// A mob to put in the world when its chunk first loads.
#[derive(Debug, Clone, PartialEq)]
pub struct MobSpawn {
    /// The mob's name as in `mobs.ron`, or `chest_minecart`.
    pub mob: String,
    pub at: DVec3,
    /// A villager's profession, or a chest minecart's loot table.
    pub tag: Option<String>,
}

/// One piece of a structure, in world space.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Piece {
    pub bounds: Option<Bounds>,
    pub blocks: Vec<(IVec3, BlockState)>,
    /// Columns filled downward from just below here, until they meet the
    /// ground, so a building on a slope doesn't float.
    pub foundations: Vec<(IVec3, BlockState)>,
    /// Blocks laid into the top of a column (paths): x, z and the block.
    pub surface: Vec<(i32, i32, BlockState)>,
    /// Chests (already among the blocks) and their loot tables.
    pub loot: Vec<(IVec3, String)>,
    /// Spawners (already among the blocks) and what they make.
    pub spawners: Vec<(IVec3, String)>,
    pub mobs: Vec<MobSpawn>,
    /// Pillars holding a piece up: not part of it for what spawns there.
    pub support: bool,
}

impl Piece {
    /// The box around everything the piece writes.
    pub fn compute_bounds(&mut self) {
        let points = self
            .blocks
            .iter()
            .map(|&(p, _)| p)
            .chain(self.foundations.iter().map(|&(p, _)| p))
            .chain(self.surface.iter().map(|&(x, z, _)| IVec3::new(x, 0, z)));
        self.bounds = points.fold(None, |acc: Option<Bounds>, p| {
            let one = Bounds::new(p, p);
            Some(acc.map_or(one, |b| b.union(one)))
        });
        // Foundations reach down and paths sit at any height.
        if let Some(b) = self.bounds.as_mut()
            && (!self.foundations.is_empty() || !self.surface.is_empty())
        {
            b.min.y = 0;
            b.max.y = b
                .max
                .y
                .max(WORLD_HEIGHT as i32 - 1)
                .min(WORLD_HEIGHT as i32 - 1);
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Structure {
    pub kind: Kind,
    pub start: ChunkPos,
    pub bounds: Bounds,
    pub pieces: Vec<Piece>,
}

impl Structure {
    pub(super) fn new(kind: Kind, start: ChunkPos, mut pieces: Vec<Piece>) -> Option<Self> {
        for piece in &mut pieces {
            if piece.bounds.is_none() {
                piece.compute_bounds();
            }
        }
        let bounds = pieces
            .iter()
            .filter_map(|piece| piece.bounds)
            .reduce(Bounds::union)?;
        Some(Self {
            kind,
            start,
            bounds,
            pieces,
        })
    }

    /// Every mob the structure puts in the world.
    pub fn mobs(&self) -> impl Iterator<Item = &MobSpawn> {
        self.pieces.iter().flat_map(|piece| &piece.mobs)
    }
}

type CacheKey = (u64, Kind, ChunkPos);

thread_local! {
    static CACHE: RefCell<HashMap<CacheKey, Option<Arc<Structure>>>> = RefCell::new(HashMap::new());
}

/// Most layouts kept per thread before the cache starts over.
const CACHE_SIZE: usize = 512;

/// The structure of `kind` starting in `start`, laid out once per thread.
pub fn structure_at(terrain: &Terrain, kind: Kind, start: ChunkPos) -> Option<Arc<Structure>> {
    let key = (terrain.seed(), kind, start);
    if let Some(found) = CACHE.with(|cache| cache.borrow().get(&key).cloned()) {
        return found;
    }
    let made = kind.generate(terrain, start).map(Arc::new);
    CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        if cache.len() >= CACHE_SIZE {
            cache.clear();
        }
        cache.insert(key, made.clone());
    });
    made
}

/// Every structure with a piece reaching into chunk `pos`.
pub fn near(terrain: &Terrain, pos: ChunkPos) -> Vec<Arc<Structure>> {
    let mut found = Vec::new();
    for kind in Kind::ALL {
        for start in kind.starts_near(terrain.seed(), pos) {
            if let Some(structure) = structure_at(terrain, kind, start)
                && structure.bounds.touches_chunk(pos)
            {
                found.push(structure);
            }
        }
    }
    found
}

/// The structure of any kind whose bounds hold `at`, if one does.
pub fn containing(terrain: &Terrain, at: IVec3) -> Option<Arc<Structure>> {
    let pos = ChunkPos::of_block(at.x, at.z);
    near(terrain, pos).into_iter().find(|s| {
        s.pieces
            .iter()
            .filter_map(|p| p.bounds)
            .any(|b| b.contains(at))
    })
}

/// Mobs the structures put in chunk `pos`, for when it first loads.
pub fn spawns_in(terrain: &Terrain, pos: ChunkPos) -> Vec<MobSpawn> {
    let mut spawns = Vec::new();
    for structure in near(terrain, pos) {
        for mob in structure.mobs() {
            let at = ChunkPos::of_block(mob.at.x.floor() as i32, mob.at.z.floor() as i32);
            if at == pos {
                spawns.push(mob.clone());
            }
        }
    }
    spawns
}

/// Writes the parts of every structure inside `chunk`, returning the boxes
/// trees and plants must keep out of.
pub fn place(terrain: &Terrain, chunk: &mut Chunk) -> Vec<Bounds> {
    let structures = near(terrain, chunk.pos);
    write(&structures, terrain.seed(), chunk)
}

/// Writes the parts of `structures` inside `chunk` (loot rolled from
/// `seed`), returning the boxes trees and plants must keep out of.
pub(super) fn write(structures: &[Arc<Structure>], seed: u64, chunk: &mut Chunk) -> Vec<Bounds> {
    let mut keep_out = Vec::new();
    let pieces: Vec<&Piece> = structures
        .iter()
        .flat_map(|s| s.pieces.iter())
        .filter(|p| p.bounds.is_some_and(|b| b.touches_chunk(chunk.pos)))
        .collect();
    for s in structures {
        if s.kind.on_surface() {
            keep_out.extend(s.pieces.iter().filter_map(|p| p.bounds));
        }
    }
    let mut writer = Writer::new(chunk);
    // Paths first, then what buildings stand on, then the buildings.
    for piece in &pieces {
        for &(x, z, block) in &piece.surface {
            writer.surface(x, z, block);
        }
    }
    for piece in &pieces {
        for &(top, block) in &piece.foundations {
            writer.foundation(top, block);
        }
    }
    for piece in &pieces {
        for &(at, block) in &piece.blocks {
            writer.set(at, block);
        }
        for (at, table) in &piece.loot {
            if let Some(local) = writer.local(*at) {
                let mut rng = Rng::for_chunk(seed, at.x, at.z, at.y as u64 ^ 0x1007);
                let slots = loot::fill(table, &mut rng);
                let entity = if writer.get(*at).is_some_and(|b| b.is(blocks::DISPENSER)) {
                    let mut nine = slots;
                    nine.truncate(super::block_entity::DISPENSER_SLOTS);
                    BlockEntity::Dispenser(nine)
                } else {
                    BlockEntity::Chest(slots)
                };
                writer.chunk.set_block_entity(local, Some(entity));
            }
        }
        for (at, mob) in &piece.spawners {
            if let Some(local) = writer.local(*at) {
                writer
                    .chunk
                    .set_block_entity(local, Some(BlockEntity::Spawner(Spawner::new(mob))));
            }
        }
    }
    keep_out
}

/// Writes world-space blocks into one chunk, dropping the rest.
struct Writer<'a> {
    chunk: &'a mut Chunk,
    min_x: i32,
    min_z: i32,
}

impl<'a> Writer<'a> {
    fn new(chunk: &'a mut Chunk) -> Self {
        let (min_x, min_z) = chunk.pos.min_block();
        Self {
            chunk,
            min_x,
            min_z,
        }
    }

    fn local(&self, at: IVec3) -> Option<[u8; 3]> {
        let (x, z) = (at.x - self.min_x, at.z - self.min_z);
        let size = CHUNK_SIZE as i32;
        ((0..size).contains(&x)
            && (0..size).contains(&z)
            && (0..WORLD_HEIGHT as i32).contains(&at.y))
        .then_some([x as u8, at.y as u8, z as u8])
    }

    fn get(&self, at: IVec3) -> Option<BlockState> {
        let [x, y, z] = self.local(at)?;
        Some(self.chunk.get(x as usize, y as usize, z as usize))
    }

    fn set(&mut self, at: IVec3, block: BlockState) {
        if let Some([x, y, z]) = self.local(at) {
            self.chunk.set(x as usize, y as usize, z as usize, block);
            // A block entity left from the terrain isn't this block's.
            self.chunk.set_block_entity([x, y, z], None);
            // Fences, panes, walls and stairs join their neighbors once
            // the chunks around are loaded.
            if block.def().shaped_by_neighbors() {
                self.chunk.post_process.push([x, y, z]);
            }
        }
    }

    /// Fills the column below `top` with `block` down to solid ground (at
    /// most 24 blocks).
    fn foundation(&mut self, top: IVec3, block: BlockState) {
        for y in (top.y - 24..top.y).rev() {
            let at = IVec3::new(top.x, y, top.z);
            let Some(here) = self.get(at) else {
                return;
            };
            if is_ground(here) {
                return;
            }
            self.set(at, block);
        }
    }

    /// Lays `block` into the top of column (x, z): in place of the
    /// surface block, clearing the plant on it. Water gets the block too,
    /// as a bridge.
    fn surface(&mut self, x: i32, z: i32, block: BlockState) {
        let Some(top) = (1..WORLD_HEIGHT as i32).rev().find(|&y| {
            self.get(IVec3::new(x, y, z))
                .is_some_and(|b| !b.is_air() && !is_plant(b))
        }) else {
            return;
        };
        let at = IVec3::new(x, top, z);
        self.set(at, block);
        let above = at + IVec3::Y;
        if self.get(above).is_some_and(is_plant) {
            self.set(above, BlockState::AIR);
            if self.get(above + IVec3::Y).is_some_and(is_plant) {
                self.set(above + IVec3::Y, BlockState::AIR);
            }
        }
    }
}

/// Solid ground a foundation stops at.
fn is_ground(block: BlockState) -> bool {
    !block.is_air() && block.def().solid && !is_plant(block) && block.def().fluid.is_none()
}

/// Plants, leaves and snow layers: what paths clear away.
fn is_plant(block: BlockState) -> bool {
    use super::block::Shape;
    let def = block.def();
    matches!(def.shape, Shape::Cross | Shape::Crop | Shape::SnowLayer)
        || def.name.ends_with("_leaves")
        || block.is(blocks::SNOW)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spread_candidates_stay_in_their_region_apart_from_the_next() {
        let spread = Spread::new(34, 8);
        for rz in -3..3 {
            for rx in -3..3 {
                let c = spread.candidate(42, 7, rx, rz);
                assert!((rx * 34..rx * 34 + 26).contains(&c.x), "{c:?}");
                assert!((rz * 34..rz * 34 + 26).contains(&c.z), "{c:?}");
            }
        }
        // The same seed always picks the same chunks.
        assert_eq!(spread.candidate(42, 7, 1, 1), spread.candidate(42, 7, 1, 1));
        assert_ne!(spread.candidate(42, 7, 1, 1), spread.candidate(43, 7, 1, 1));
    }

    #[test]
    fn bounds_overlap_and_touch_chunks() {
        let a = Bounds::new(IVec3::new(0, 60, 0), IVec3::new(10, 70, 10));
        let b = Bounds::new(IVec3::new(10, 0, 10), IVec3::new(20, 5, 20));
        assert!(a.intersects_xz(&b));
        assert!(!a.intersects(&b));
        assert!(a.touches_chunk(ChunkPos::new(0, 0)));
        assert!(!a.touches_chunk(ChunkPos::new(1, 0)));
        assert!(b.touches_chunk(ChunkPos::new(1, 1)));
    }
}
