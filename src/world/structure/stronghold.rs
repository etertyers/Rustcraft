//! Strongholds (modern `StrongholdPieces`, M52): a spiral staircase down
//! from the start, then pieces joined door to door by a weighted random
//! walk (straight corridors, left and right turns, stairs, prison halls,
//! five-way crossings, square rooms with a fountain, a pillar or a storage
//! loft, chest corridors and libraries), up to 50 deep and 112 blocks from
//! the start, with exactly one portal room: twelve End portal frames
//! round a 3 × 3 pool of lava, each frame holding an eye one time in ten,
//! and a silverfish spawner on its stairs. The whole thing is moved down
//! until its top is at least ten blocks under sea level.
//!
//! They stand in modern's concentric rings round the world's center (3,
//! 6, 10, 15, 21, 28, 36 and 9 of them, 128 in all), each moved to the
//! nearest land chunk within 112 blocks; the eye of ender and `/locate`
//! both read `positions`.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex, OnceLock},
};

use glam::IVec3;

use super::{Bounds, Piece, build::Build};
use crate::{
    config::SEA_LEVEL,
    world::{
        biome::Biome,
        block::BlockState,
        blocks,
        chunk::ChunkPos,
        worldgen::{rng::Rng, terrain::Terrain},
    },
};

/// Modern's `stronghold` placement: rings `DISTANCE` chunks apart, the
/// first with `RING_START` strongholds, `COUNT` in all.
const DISTANCE: f64 = 32.0;
const RING_START: u32 = 3;
pub const COUNT: u32 = 128;
const SALT: u64 = 0x57_0E06_0001;
/// A stronghold moves to a land chunk within this many chunks (112 blocks).
const BIOME_SEARCH: i32 = 7;
/// How far the search for land goes when there is none that near.
const FAR_SEARCH: i32 = 96;
/// Pieces stay this far (blocks) from the start, and this many doors deep.
pub const MAX_DISTANCE: i32 = 112;
const MAX_DEPTH: u32 = 50;
const MAX_PIECES: usize = 50;

/// Whether a stronghold may stand under `biome`: land that isn't a beach,
/// river or mushroom field.
pub fn suits(biome: Biome) -> bool {
    !(biome.is_ocean()
        || biome.is_nether()
        || biome.is_end()
        || biome.is_cave()
        || matches!(biome, Biome::River | Biome::Beach | Biome::MushroomFields))
}

/// The chunks strongholds start in, ring by ring, worked out once per
/// world from the seed (modern `ConcentricRingsStructurePlacement`).
pub fn positions(seed: u64) -> Vec<ChunkPos> {
    static CACHE: OnceLock<Mutex<HashMap<u64, Arc<Vec<ChunkPos>>>>> = OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    if let Some(found) = cache.lock().unwrap_or_else(|e| e.into_inner()).get(&seed) {
        return found.to_vec();
    }
    let found = Arc::new(compute(&Terrain::new(seed), seed));
    cache
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(seed, found.clone());
    found.to_vec()
}

/// The nearest chunk to `(cx, cz)` within `BIOME_SEARCH` whose biome suits
/// a stronghold. Modern keeps the chunk when there is none (a stronghold
/// can then stand under an ocean); here the search goes on outward, up to
/// `FAR_SEARCH` chunks, so none does.
fn settle(terrain: &Terrain, cx: i32, cz: i32) -> ChunkPos {
    let suits_at = |x: i32, z: i32| suits(terrain.biome_at(x * 16 + 8, z * 16 + 8));
    if suits_at(cx, cz) {
        return ChunkPos::new(cx, cz);
    }
    let mut best: Option<(i32, ChunkPos)> = None;
    for dz in -BIOME_SEARCH..=BIOME_SEARCH {
        for dx in -BIOME_SEARCH..=BIOME_SEARCH {
            let d = dx * dx + dz * dz;
            if d <= BIOME_SEARCH * BIOME_SEARCH
                && best.is_none_or(|(b, _)| d < b)
                && suits_at(cx + dx, cz + dz)
            {
                best = Some((d, ChunkPos::new(cx + dx, cz + dz)));
            }
        }
    }
    if let Some((_, pos)) = best {
        return pos;
    }
    for ring in BIOME_SEARCH + 1..=FAR_SEARCH {
        for dz in -ring..=ring {
            for dx in -ring..=ring {
                if dx.abs().max(dz.abs()) != ring {
                    continue;
                }
                let d = dx * dx + dz * dz;
                if best.is_none_or(|(b, _)| d < b) && suits_at(cx + dx, cz + dz) {
                    best = Some((d, ChunkPos::new(cx + dx, cz + dz)));
                }
            }
        }
        if let Some((_, pos)) = best {
            return pos;
        }
    }
    ChunkPos::new(cx, cz)
}

fn compute(terrain: &Terrain, seed: u64) -> Vec<ChunkPos> {
    let mut rng = Rng::new(seed ^ SALT);
    let mut found = Vec::new();
    let mut angle = rng.next_f64() * std::f64::consts::TAU;
    let (mut ring, mut in_ring, mut spread) = (0u32, 0u32, RING_START);
    for i in 0..COUNT {
        let distance = 4.0 * DISTANCE
            + DISTANCE * f64::from(ring) * 6.0
            + (rng.next_f64() - 0.5) * DISTANCE * 2.5;
        let x = (angle.cos() * distance).round() as i32;
        let z = (angle.sin() * distance).round() as i32;
        found.push(settle(terrain, x, z));
        angle += std::f64::consts::TAU / f64::from(spread);
        in_ring += 1;
        if in_ring == spread {
            ring += 1;
            in_ring = 0;
            spread += 2 * spread / (ring + 1);
            spread = spread.min(COUNT - (i + 1));
            angle += rng.next_f64() * std::f64::consts::TAU;
        }
    }
    found
}

/// Stone bricks: 20% cracked, 30% mossy, 5% infested (modern
/// `SmartStoneSelector`), the same every time at one spot.
pub fn brick(at: IVec3) -> BlockState {
    let h = (at.x as u32).wrapping_mul(73_856_093)
        ^ (at.y as u32).wrapping_mul(19_349_663)
        ^ (at.z as u32).wrapping_mul(83_492_791);
    match (h.wrapping_mul(2_654_435_761) >> 8) % 20 {
        0..=3 => BlockState::of(blocks::CRACKED_STONE_BRICKS),
        4..=9 => BlockState::of(blocks::MOSSY_STONE_BRICKS),
        10 => of("infested_stone_bricks"),
        _ => BlockState::of(blocks::STONE_BRICKS),
    }
}

fn of(name: &str) -> BlockState {
    BlockState::parse(name).unwrap_or_else(|err| panic!("a stronghold block {name}: {err}"))
}

/// The compass name of a horizontal direction.
fn name(dir: IVec3) -> &'static str {
    match (dir.x.signum(), dir.z.signum()) {
        (0, -1) => "north",
        (1, 0) => "east",
        (0, 1) => "south",
        _ => "west",
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Straight,
    Prison,
    LeftTurn,
    RightTurn,
    Room,
    StraightStairs,
    Spiral,
    Five,
    ChestCorridor,
    Library,
    Portal,
}

/// How often each piece is picked, how many a stronghold may have and from
/// which depth (modern `PIECE_WEIGHTS`).
const WEIGHTS: [(Kind, u32, u32, u32); 10] = [
    (Kind::Straight, 40, u32::MAX, 0),
    (Kind::Prison, 5, 5, 0),
    (Kind::LeftTurn, 20, u32::MAX, 0),
    (Kind::RightTurn, 20, u32::MAX, 0),
    (Kind::Room, 10, 6, 0),
    (Kind::StraightStairs, 5, 5, 0),
    (Kind::Spiral, 5, 5, 0),
    (Kind::Five, 5, 4, 0),
    (Kind::ChestCorridor, 5, 4, 0),
    (Kind::Library, 10, 2, 4),
];
/// The portal room: weight 20, one only, from depth 5.
const PORTAL: (Kind, u32, u32, u32) = (Kind::Portal, 20, 1, 5);

impl Kind {
    /// Half the width, the height, the length and the height of the door's
    /// feet above the floor bricks.
    fn size(self, tall: bool) -> (i32, i32, i32, i32) {
        match self {
            Kind::Straight | Kind::ChestCorridor => (2, 5, 7, 1),
            Kind::LeftTurn | Kind::RightTurn => (2, 5, 5, 1),
            Kind::Prison => (4, 5, 11, 1),
            Kind::Room => (5, 7, 11, 1),
            Kind::StraightStairs => (2, 11, 8, 6),
            Kind::Spiral => (2, 11, 5, 7),
            Kind::Five => (5, 9, 11, 1),
            Kind::Library if tall => (6, 11, 15, 1),
            Kind::Library => (6, 6, 11, 1),
            Kind::Portal => (5, 8, 16, 1),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Door {
    Opening,
    Wood,
    Grates,
    Iron,
}

/// A piece's box: where the door's feet are, which way it runs.
#[derive(Debug, Clone, Copy)]
struct Frame {
    /// The floor bricks under the middle of the front wall.
    origin: IVec3,
    dir: IVec3,
    half: i32,
    height: i32,
    length: i32,
}

impl Frame {
    fn right(&self) -> IVec3 {
        IVec3::new(-self.dir.z, 0, self.dir.x)
    }

    /// `right` blocks to the right, `up` above the floor bricks, `ahead`
    /// in from the front wall.
    fn at(&self, right: i32, up: i32, ahead: i32) -> IVec3 {
        self.origin + self.right() * right + IVec3::Y * up + self.dir * ahead
    }

    fn bounds(&self) -> Bounds {
        Bounds::new(
            self.at(-self.half, 0, 0),
            self.at(self.half, self.height - 1, self.length - 1),
        )
    }
}

#[derive(Debug, Clone, Copy)]
struct Placed {
    kind: Kind,
    frame: Frame,
    door: Door,
    tall: bool,
    depth: u32,
    start: bool,
}

/// Where a child may start: its door's feet, which way it runs, and a
/// piece it must be.
struct Exit {
    cell: IVec3,
    dir: IVec3,
    imposed: Option<Kind>,
}

struct Hold<'a> {
    rng: &'a mut Rng,
    start: IVec3,
    placed: Vec<Placed>,
    pending: Vec<usize>,
    counts: Vec<(Kind, u32)>,
    last: Option<Kind>,
    portal: bool,
}

impl Hold<'_> {
    fn count(&self, kind: Kind) -> u32 {
        self.counts
            .iter()
            .find(|(k, _)| *k == kind)
            .map_or(0, |(_, n)| *n)
    }

    fn may_place(&self, entry: (Kind, u32, u32, u32), depth: u32) -> bool {
        self.count(entry.0) < entry.2 && depth >= entry.3
    }

    fn random_door(&mut self) -> Door {
        match self.rng.below(5) {
            2 => Door::Wood,
            3 => Door::Grates,
            4 => Door::Iron,
            _ => Door::Opening,
        }
    }

    /// A piece of `kind` entered at `exit`, if it fits.
    fn fit(&mut self, kind: Kind, exit: &Exit, depth: u32) -> Option<Placed> {
        let tries: &[bool] = if kind == Kind::Library {
            &[true, false]
        } else {
            &[false]
        };
        for &tall in tries {
            let (half, height, length, entry) = kind.size(tall);
            let frame = Frame {
                origin: exit.cell - IVec3::Y * entry,
                dir: exit.dir,
                half,
                height,
                length,
            };
            let b = frame.bounds();
            let near = |v: i32, s: i32| (v - s).abs() <= MAX_DISTANCE;
            let inside = near(b.min.x, self.start.x)
                && near(b.max.x, self.start.x)
                && near(b.min.z, self.start.z)
                && near(b.max.z, self.start.z);
            if inside && !self.placed.iter().any(|p| p.frame.bounds().intersects(&b)) {
                let door = self.random_door();
                return Some(Placed {
                    kind,
                    frame,
                    door,
                    tall,
                    depth,
                    start: false,
                });
            }
        }
        None
    }

    /// Modern `generatePiece`: up to five weighted picks, each trying the
    /// chosen piece and then the ones after it in the table.
    fn child(&mut self, exit: &Exit, depth: u32) -> Option<Placed> {
        if depth > MAX_DEPTH || self.placed.len() >= MAX_PIECES {
            return None;
        }
        if let Some(kind) = exit.imposed {
            return self.fit(kind, exit, depth);
        }
        let table: Vec<_> = WEIGHTS
            .iter()
            .copied()
            .chain((!self.portal).then_some(PORTAL))
            .collect();
        for _ in 0..5 {
            let total: u32 = table
                .iter()
                .filter(|e| self.may_place(**e, depth))
                .map(|e| e.1)
                .sum();
            if total == 0 {
                return None;
            }
            let mut roll = self.rng.below(total) as i64;
            let mut chosen = None;
            for (i, entry) in table.iter().enumerate() {
                if self.may_place(*entry, depth) {
                    roll -= i64::from(entry.1);
                    if roll < 0 {
                        chosen = Some(i);
                        break;
                    }
                }
            }
            let Some(first) = chosen else { continue };
            for entry in &table[first..] {
                if !self.may_place(*entry, depth) || Some(entry.0) == self.last {
                    continue;
                }
                if let Some(piece) = self.fit(entry.0, exit, depth) {
                    return Some(piece);
                }
            }
        }
        None
    }

    /// The doors out of a piece.
    fn exits(&mut self, p: &Placed) -> Vec<Exit> {
        let f = p.frame;
        let (left, right) = (-f.right(), f.right());
        let forward = || Exit {
            cell: f.at(0, 1, f.length),
            dir: f.dir,
            imposed: None,
        };
        let side = |dir: IVec3, ahead: i32| Exit {
            cell: f.at(0, 1, ahead) + dir * (f.half + 1),
            dir,
            imposed: None,
        };
        match p.kind {
            Kind::Straight => {
                let mut out = vec![forward()];
                if self.rng.below(2) == 0 {
                    out.push(side(left, 2));
                }
                if self.rng.below(2) == 0 {
                    out.push(side(right, 2));
                }
                out
            }
            Kind::LeftTurn => vec![side(left, 2)],
            Kind::RightTurn => vec![side(right, 2)],
            Kind::Room => vec![forward(), side(left, 5), side(right, 5)],
            Kind::Five => vec![
                forward(),
                side(left, 3),
                side(right, 3),
                side(left, 7),
                side(right, 7),
            ],
            // The staircase lets out on its left, at the bottom; the first
            // thing from the start is always a five-way crossing.
            Kind::Spiral => {
                let mut exit = side(left, 2);
                exit.imposed = p.start.then_some(Kind::Five);
                vec![exit]
            }
            Kind::Library | Kind::Portal => Vec::new(),
            Kind::Prison | Kind::StraightStairs | Kind::ChestCorridor => vec![forward()],
        }
    }

    /// Lays the stronghold out from its spiral staircase.
    fn generate(&mut self, top: IVec3) {
        let dir = match self.rng.below(4) {
            0 => IVec3::NEG_Z,
            1 => IVec3::X,
            2 => IVec3::Z,
            _ => IVec3::NEG_X,
        };
        let (half, height, length, entry) = Kind::Spiral.size(false);
        let frame = Frame {
            origin: top - IVec3::Y * entry,
            dir,
            half,
            height,
            length,
        };
        self.start = frame.bounds().min;
        self.placed.push(Placed {
            kind: Kind::Spiral,
            frame,
            door: Door::Opening,
            tall: false,
            depth: 0,
            start: true,
        });
        self.pending.push(0);
        while !self.pending.is_empty() {
            let pick = self.rng.below(self.pending.len() as u32) as usize;
            let index = self.pending.swap_remove(pick);
            let parent = self.placed[index];
            for exit in self.exits(&parent) {
                if let Some(child) = self.child(&exit, parent.depth + 1) {
                    self.last = Some(child.kind);
                    match self.counts.iter_mut().find(|(k, _)| *k == child.kind) {
                        Some((_, n)) => *n += 1,
                        None => self.counts.push((child.kind, 1)),
                    }
                    if child.kind == Kind::Portal {
                        self.portal = true;
                    }
                    self.placed.push(child);
                    self.pending.push(self.placed.len() - 1);
                }
            }
        }
    }
}

/// A wall torch at `(right, up, ahead)` shining toward `toward`.
fn torch(build: &mut Build, f: &Frame, at: (i32, i32, i32), toward: IVec3) {
    build.set(
        f.at(at.0, at.1, at.2),
        of(&format!("wall_torch[facing={}]", name(toward))),
    );
}

/// The doorway in the front wall at the door's feet `y`: opening, wooden
/// door, iron bars, or an iron door with a button each side.
fn door(build: &mut Build, f: &Frame, kind: Door, y: i32) {
    // The doorway also goes through the wall of the piece it joins.
    for dy in 0..2 {
        build.set(f.at(0, y + dy, -1), BlockState::AIR);
    }
    let facing = name(f.dir);
    match kind {
        Door::Opening => {
            for dy in 0..2 {
                build.set(f.at(0, y + dy, 0), BlockState::AIR);
            }
        }
        Door::Wood | Door::Iron => {
            let (block, other) = if kind == Door::Wood {
                ("oak_door", "oak_door")
            } else {
                ("iron_door", "iron_door")
            };
            build.set(
                f.at(0, y, 0),
                of(&format!("{block}[facing={facing},half=lower]")),
            );
            build.set(
                f.at(0, y + 1, 0),
                of(&format!("{other}[facing={facing},half=upper]")),
            );
            if kind == Door::Iron {
                build.set(
                    f.at(1, y + 1, 1),
                    of(&format!("stone_button[face=wall,facing={facing}]")),
                );
                let back = name(-f.dir);
                build.set(
                    f.at(1, y + 1, -2),
                    of(&format!("stone_button[face=wall,facing={back}]")),
                );
            }
        }
        Door::Grates => {
            for dy in 0..2 {
                build.set(f.at(0, y + dy, 0), of("iron_bars"));
            }
        }
    }
}

impl Hold<'_> {
    fn build_piece(&mut self, p: &Placed) -> Piece {
        let f = p.frame;
        let mut build = Build::new();
        let (a, b) = (
            f.at(-f.half, 0, 0),
            f.at(f.half, f.height - 1, f.length - 1),
        );
        build.hollow(a, b, brick);
        let (_, _, _, entry) = p.kind.size(p.tall);
        if !p.start {
            door(&mut build, &f, p.door, entry);
        }
        let (right, left) = (f.right(), -f.right());
        match p.kind {
            Kind::Straight | Kind::LeftTurn | Kind::RightTurn => {
                if self.rng.below(5) == 0 {
                    torch(&mut build, &f, (f.half - 1, 3, 1), left);
                }
            }
            Kind::ChestCorridor => {
                build.chest(
                    f.at(1, 1, 3),
                    of(&format!("chest[facing={}]", name(left))),
                    "stronghold_corridor",
                );
                build.set(f.at(1, 2, 3), brick(f.at(1, 2, 3)));
                torch(&mut build, &f, (-1, 3, 3), right);
            }
            Kind::Prison => self.prison(&mut build, &f),
            Kind::Room => self.room(&mut build, &f),
            Kind::Five => self.five(&mut build, &f),
            Kind::StraightStairs => stairs(&mut build, &f),
            Kind::Spiral => spiral(&mut build, &f),
            Kind::Library => self.library(&mut build, &f, p.tall),
            Kind::Portal => self.portal_room(&mut build, &f),
        }
        build.finish()
    }

    fn prison(&mut self, build: &mut Build, f: &Frame) {
        for s in [-1, 1] {
            for ahead in 1..=9 {
                for y in 1..=3 {
                    let at = f.at(2 * s, y, ahead);
                    let block = match ahead % 3 {
                        0 => brick(at),
                        1 if y < 3 => {
                            let (face, half) =
                                (name(f.right() * -s), if y == 1 { "lower" } else { "upper" });
                            of(&format!("iron_door[facing={face},half={half}]"))
                        }
                        _ => of("iron_bars"),
                    };
                    build.set(at, block);
                }
                if ahead % 3 == 0 {
                    for y in 1..=3 {
                        build.set(f.at(3 * s, y, ahead), brick(f.at(3 * s, y, ahead)));
                    }
                }
            }
            torch(build, f, (s, 3, 5), -f.right() * s);
        }
    }

    fn room(&mut self, build: &mut Build, f: &Frame) {
        let (right, left) = (f.right(), -f.right());
        match self.rng.below(3) {
            0 => {
                // A fountain: a brick basin on a pillar, water on top.
                for r in -1..=1 {
                    for a in 4..=6 {
                        for y in 1..=2 {
                            build.set(f.at(r, y, a), brick(f.at(r, y, a)));
                        }
                        if r != 0 || a != 5 {
                            build.set(f.at(r, 3, a), brick(f.at(r, 3, a)));
                        }
                    }
                }
                build.set(f.at(0, 3, 5), of("water[level=0]"));
                for (u, a, face) in [(-2, 5, left), (2, 5, right), (0, 3, -f.dir), (0, 7, f.dir)] {
                    torch(build, f, (u, 2, a), face);
                }
            }
            1 => {
                // A pillar with torches round it.
                for y in 1..=5 {
                    build.set(f.at(0, y, 5), brick(f.at(0, y, 5)));
                }
                for (u, a, face) in [(-1, 5, left), (1, 5, right), (0, 4, -f.dir), (0, 6, f.dir)] {
                    torch(build, f, (u, 3, a), face);
                }
            }
            _ => {
                // A storage loft along the left wall, up a ladder, with two
                // chests.
                for u in -4..=-2 {
                    for a in 3..=7 {
                        build.set(f.at(u, 4, a), of("oak_planks"));
                    }
                }
                for y in 1..=5 {
                    build.set(
                        f.at(-4, y, 2),
                        of(&format!("ladder[facing={}]", name(right))),
                    );
                }
                for a in [4, 6] {
                    build.chest(
                        f.at(-3, 5, a),
                        of(&format!("chest[facing={}]", name(right))),
                        "stronghold_crossing",
                    );
                }
            }
        }
    }

    fn five(&mut self, build: &mut Build, f: &Frame) {
        for (u, a) in [(-2, 3), (2, 3), (-2, 7), (2, 7)] {
            for y in 1..=7 {
                build.set(f.at(u, y, a), brick(f.at(u, y, a)));
            }
            torch(
                build,
                f,
                (u, 3, if a == 3 { a - 1 } else { a + 1 }),
                if a == 3 { -f.dir } else { f.dir },
            );
        }
        if self.rng.below(2) == 0 {
            build.chest(
                f.at(3, 1, 5),
                of(&format!("chest[facing={}]", name(-f.right()))),
                "stronghold_crossing",
            );
        }
    }

    fn library(&mut self, build: &mut Build, f: &Frame, tall: bool) {
        let (right, left) = (f.right(), -f.right());
        let last = f.length - 2;
        for ahead in 1..=last {
            for u in [-5, 5] {
                for y in 1..=4 {
                    build.set(f.at(u, y, ahead), of("bookshelf"));
                }
            }
            if ahead % 3 == 0 && ahead < last {
                for side in [-1, 1] {
                    for u in 2..=4 {
                        for y in 1..=3 {
                            build.set(f.at(u * side, y, ahead), of("bookshelf"));
                        }
                    }
                }
            }
            if self.rng.below(4) == 0 {
                let u = self.rng.below(9) as i32 - 4;
                build.set(f.at(u, f.height - 2, ahead), of("cobweb"));
            }
        }
        let chest = |build: &mut Build, u: i32, y: i32, ahead: i32, face: IVec3| {
            build.chest(
                f.at(u, y, ahead),
                of(&format!("chest[facing={}]", name(face))),
                "stronghold_library",
            );
        };
        chest(build, 3, 1, last, left);
        if tall {
            // A gallery round the second floor, a ladder up to it and a
            // chandelier hanging in the middle.
            for ahead in 1..=last {
                for u in [-5, -4, 4, 5] {
                    if (u, ahead) != (4, 1) {
                        build.set(f.at(u, 6, ahead), of("oak_planks"));
                    }
                }
                for u in [-5, 5] {
                    for y in 7..=9 {
                        build.set(f.at(u, y, ahead), of("bookshelf"));
                    }
                }
            }
            for y in 1..=6 {
                build.set(
                    f.at(4, y, 1),
                    of(&format!("ladder[facing={}]", name(f.dir))),
                );
            }
            chest(build, -4, 7, last, right);
            let mid = f.length / 2;
            for y in f.height - 4..=f.height - 2 {
                build.set(f.at(0, y, mid), of("oak_fence"));
            }
            for (u, a) in [(-1, mid), (1, mid), (0, mid - 1), (0, mid + 1)] {
                build.set(f.at(u, f.height - 4, a), of("oak_fence"));
                build.set(f.at(u, f.height - 3, a), of("torch"));
            }
        } else {
            chest(build, -3, 1, last, right);
        }
    }

    /// The portal room: stairs up to a platform, lava under the portal,
    /// the twelve frames, iron-barred windows and the silverfish spawner.
    fn portal_room(&mut self, build: &mut Build, f: &Frame) {
        // Steps up to the platform, three high.
        for (a, y) in [(4, 1), (5, 2), (6, 3)] {
            for u in -1..=1 {
                for fill in 1..y {
                    build.set(f.at(u, fill, a), brick(f.at(u, fill, a)));
                }
                build.set(
                    f.at(u, y, a),
                    of(&format!(
                        "stone_brick_stairs[facing={},half=bottom]",
                        name(f.dir)
                    )),
                );
            }
        }
        build.spawner(f.at(1, 3, 5), "silverfish");
        // The platform: 7 × 7, its top at 3, a pool of lava under the middle.
        for u in -3..=3 {
            for a in 7..=13 {
                for y in 1..=3 {
                    build.set(f.at(u, y, a), brick(f.at(u, y, a)));
                }
            }
        }
        // Lava also lies level with the platform along both sides of it and
        // behind it (on brick), dammed from the walkway by a brick.
        for u in -4i32..=4 {
            for a in 7..=14 {
                if u.abs() == 4 || a == 14 {
                    for y in 1..=2 {
                        build.set(f.at(u, y, a), brick(f.at(u, y, a)));
                    }
                    build.set(f.at(u, 3, a), of("lava[level=0]"));
                }
            }
        }
        for u in [-4, 4] {
            build.set(f.at(u, 3, 6), brick(f.at(u, 3, 6)));
        }
        let mid = 10;
        let mut eyes = 0;
        for i in -2i32..=2 {
            for s in -2i32..=2 {
                let (u, a) = (s, mid + i);
                let (edge, corner) = (i.abs() == 2 || s.abs() == 2, i.abs() == 2 && s.abs() == 2);
                if !edge {
                    build.set(f.at(u, 3, a), of("lava[level=0]"));
                    continue;
                }
                if corner {
                    continue;
                }
                let inward = if i == -2 {
                    f.dir
                } else if i == 2 {
                    -f.dir
                } else if s == -2 {
                    f.right()
                } else {
                    -f.right()
                };
                let eye = self.rng.below(10) == 0;
                eyes += u32::from(eye);
                build.set(
                    f.at(u, 4, a),
                    of(&format!(
                        "end_portal_frame[facing={},eye={eye}]",
                        name(inward)
                    )),
                );
            }
        }
        // All twelve eyes in place: the portal is open from the start.
        if eyes == 12 {
            for i in -1..=1 {
                for s in -1..=1 {
                    build.set(f.at(s, 4, mid + i), of("end_portal"));
                }
            }
        }
        // Windows of iron bars along the sides.
        for a in [5, 8, 11] {
            for u in [-5, 5] {
                for y in 3..=4 {
                    build.set(f.at(u, y, a), of("iron_bars"));
                }
            }
        }
        for u in [-4, 4] {
            for a in [2, 6, 9, 12] {
                let toward = if u < 0 { f.right() } else { -f.right() };
                torch(build, f, (u, 3, a), toward);
            }
        }
    }
}

/// Straight stairs down: five steps from the door's height to the floor.
fn stairs(build: &mut Build, f: &Frame) {
    for ahead in 1..=6 {
        let row = 6 - ahead;
        for u in -1..=1 {
            for y in 1..row {
                build.set(f.at(u, y, ahead), brick(f.at(u, y, ahead)));
            }
            if row >= 1 {
                build.set(
                    f.at(u, row, ahead),
                    of(&format!(
                        "stone_brick_stairs[facing={},half=bottom]",
                        name(-f.dir)
                    )),
                );
            }
        }
    }
    torch(build, f, (1, 8, 1), -f.right());
}

/// A spiral staircase round a pillar, six steps down, then the floor.
fn spiral(build: &mut Build, f: &Frame) {
    let ring = [
        (0, 1),
        (1, 1),
        (1, 2),
        (1, 3),
        (0, 3),
        (-1, 3),
        (-1, 2),
        (-1, 1),
    ];
    for y in 1..=9 {
        build.set(f.at(0, y, 2), brick(f.at(0, y, 2)));
    }
    for (i, &(u, a)) in ring.iter().enumerate().take(6) {
        let row = 6 - i as i32;
        for y in 1..row {
            build.set(f.at(u, y, a), brick(f.at(u, y, a)));
        }
        let face = if i == 0 {
            f.dir
        } else {
            let prev = ring[i - 1];
            f.right() * (prev.0 - u) + f.dir * (prev.1 - a)
        };
        build.set(
            f.at(u, row, a),
            of(&format!(
                "stone_brick_stairs[facing={},half=bottom]",
                name(face)
            )),
        );
    }
}

/// Lays out the stronghold whose start chunk is `start`, trying again
/// until it has a portal room.
pub fn generate(_terrain: &Terrain, start: ChunkPos, rng: &mut Rng) -> Option<Vec<Piece>> {
    let (x, z) = start.min_block();
    let top = IVec3::new(x + 8, 120, z + 8);
    for _ in 0..200 {
        let mut hold = Hold {
            rng: &mut *rng,
            start: top,
            placed: Vec::new(),
            pending: Vec::new(),
            counts: Vec::new(),
            last: None,
            portal: false,
        };
        hold.generate(top);
        if !hold.portal {
            continue;
        }
        let placed = std::mem::take(&mut hold.placed);
        let mut pieces: Vec<Piece> = placed.iter().map(|p| hold.build_piece(p)).collect();
        // Moved down until the top is ten under the sea, anywhere the
        // walls still clear the bottom of the world.
        let high = placed.iter().map(|p| p.frame.bounds().max.y).max()?;
        let low = placed.iter().map(|p| p.frame.bounds().min.y).min()?;
        let span = high - low + 1;
        let (sea, floor) = (SEA_LEVEL - 10, 11 + span);
        let target = if floor < sea {
            floor + hold.rng.below((sea - floor) as u32) as i32
        } else {
            sea
        };
        let shift = IVec3::Y * (target - high);
        for piece in &mut pieces {
            for (at, _) in &mut piece.blocks {
                *at += shift;
            }
            for (at, _) in &mut piece.loot {
                *at += shift;
            }
            for (at, _) in &mut piece.spawners {
                *at += shift;
            }
            piece.compute_bounds();
        }
        return Some(pieces);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::structure::{Kind as StructureKind, structure_at};

    #[test]
    fn the_rings_hold_3_6_10_15_21_28_36_and_9_and_the_first_is_1280_to_2816_out() {
        let found = positions(12345);
        assert_eq!(found.len(), 128);
        let distance = |pos: &ChunkPos| {
            let (x, z) = pos.min_block();
            f64::from(x).hypot(f64::from(z))
        };
        for pos in &found[..3] {
            assert!(
                (1280.0..=2816.0).contains(&distance(pos)),
                "{}",
                distance(pos)
            );
        }
        // Ring 1 (the second) is 4,352–5,888 blocks out, give or take the
        // 112 blocks a stronghold may move to find land.
        for pos in &found[3..9] {
            assert!(
                (4240.0..=6000.0).contains(&distance(pos)),
                "{}",
                distance(pos)
            );
        }
        assert_eq!(positions(12345), found, "cached and the same");
        assert_ne!(positions(54321), found);
    }

    #[test]
    fn none_stands_in_the_ocean_or_on_a_beach_or_river() {
        let seed = 12345;
        let terrain = Terrain::new(seed);
        for pos in positions(seed) {
            let (x, z) = pos.min_block();
            let biome = terrain.biome_at(x + 8, z + 8);
            assert!(suits(biome), "{pos:?} is in {biome:?}");
        }
    }

    #[test]
    fn every_block_name_the_generator_uses_parses() {
        for text in [
            "oak_door[facing=north,half=lower]",
            "oak_door[facing=north,half=upper]",
            "iron_door[facing=east,half=lower]",
            "iron_door[facing=east,half=upper]",
            "stone_button[face=wall,facing=south]",
            "stone_brick_stairs[facing=west,half=bottom]",
            "ladder[facing=east]",
            "chest[facing=north]",
            "wall_torch[facing=west]",
            "water[level=0]",
            "lava[level=0]",
            "end_portal_frame[facing=south,eye=true]",
            "infested_stone_bricks",
            "oak_fence",
        ] {
            assert!(BlockState::parse(text).is_ok(), "{text}");
        }
    }

    #[test]
    fn a_stronghold_has_one_portal_room_with_twelve_inward_frames_under_the_sea() {
        let terrain = Terrain::new(12345);
        for &start in positions(terrain.seed()).iter().take(4) {
            let hold = structure_at(&terrain, StructureKind::Stronghold, start).unwrap();
            let frames: Vec<_> = hold
                .pieces
                .iter()
                .flat_map(|p| &p.blocks)
                .filter(|(_, b)| b.is(blocks::END_PORTAL_FRAME))
                .collect();
            assert_eq!(frames.len(), 12, "{start:?}");
            let spawners = hold
                .pieces
                .iter()
                .flat_map(|p| &p.spawners)
                .filter(|(_, m)| m == "silverfish")
                .count();
            assert_eq!(spawners, 1);
            assert!(
                hold.bounds.max.y <= SEA_LEVEL - 10,
                "top {}",
                hold.bounds.max.y
            );
            assert!(hold.bounds.min.y >= 10, "bottom {}", hold.bounds.min.y);
            let (cx, cz) = start.min_block();
            let (cx, cz) = (cx + 8, cz + 8);
            for piece in &hold.pieces {
                let b = piece.bounds.unwrap();
                for v in [b.min, b.max] {
                    assert!(
                        (v.x - cx).abs() <= MAX_DISTANCE + 8
                            && (v.z - cz).abs() <= MAX_DISTANCE + 8,
                        "{v} too far from {cx} {cz}"
                    );
                }
            }
            assert!(
                hold.pieces.len() >= 8 && hold.pieces.len() <= MAX_PIECES,
                "{}",
                hold.pieces.len()
            );
        }
    }

    /// The twelve frames of one stronghold, as (position, facing, eye).
    fn frames_of(
        hold: &crate::world::structure::Structure,
    ) -> Vec<(IVec3, crate::world::Face, bool)> {
        use crate::world::props::Prop;
        hold.pieces
            .iter()
            .flat_map(|p| &p.blocks)
            .filter(|(_, b)| b.is(blocks::END_PORTAL_FRAME))
            .map(|&(at, b)| (at, b.face(Prop::Facing), b.bool(Prop::Eye)))
            .collect()
    }

    #[test]
    fn the_frames_form_a_five_by_five_ring_without_corners_all_facing_in() {
        let terrain = Terrain::new(12345);
        for &start in positions(terrain.seed()).iter().take(6) {
            let hold = structure_at(&terrain, StructureKind::Stronghold, start).unwrap();
            let frames = frames_of(&hold);
            assert_eq!(frames.len(), 12);
            let sum: IVec3 = frames.iter().map(|f| f.0).sum();
            let center = sum.as_vec3() / 12.0;
            // The ring M27's portal code looks for: filling every frame
            // with an eye lights it.
            let middle = IVec3::new(
                center.x.round() as i32,
                frames[0].0.y,
                center.z.round() as i32,
            );
            let mut want = crate::world::end_portal::ring(middle);
            let mut have: Vec<_> = frames.iter().map(|f| (f.0, f.1)).collect();
            want.sort_by_key(|(at, _)| (at.x, at.z));
            have.sort_by_key(|(at, _)| (at.x, at.z));
            assert_eq!(have, want, "{start:?}");
            let lit = |at: IVec3| {
                frames.iter().find(|f| f.0 == at).map(|f| {
                    crate::world::block::BlockState::of(blocks::END_PORTAL_FRAME)
                        .with_face(crate::world::props::Prop::Facing, f.1)
                        .with_bool(crate::world::props::Prop::Eye, true)
                })
            };
            assert_eq!(
                crate::world::end_portal::find_lit(frames[0].0, &lit),
                Some(middle)
            );
            // All on one level, 3 to a side of a square 5 across.
            assert!(frames.iter().all(|f| f.0.y == frames[0].0.y));
            for (at, face, _) in &frames {
                let n = face.normal();
                let inward = glam::Vec3::new(n[0] as f32, 0.0, n[2] as f32);
                let to_center = center - at.as_vec3();
                assert!(
                    inward.dot(to_center) > 1.0,
                    "{at} faces {face:?} away from {center}"
                );
                let (dx, dz) = (
                    (at.x as f32 - center.x).abs(),
                    (at.z as f32 - center.z).abs(),
                );
                assert!(
                    (dx.max(dz) - 2.0).abs() < 0.51 && dx.min(dz) < 1.6,
                    "{dx} {dz}"
                );
            }
        }
    }

    #[test]
    fn about_one_frame_in_ten_already_holds_an_eye() {
        let terrain = Terrain::new(12345);
        let (mut frames, mut eyes) = (0, 0);
        for &start in positions(terrain.seed()).iter().take(90) {
            let hold = structure_at(&terrain, StructureKind::Stronghold, start).unwrap();
            for (_, _, eye) in frames_of(&hold) {
                frames += 1;
                eyes += usize::from(eye);
            }
        }
        assert!(frames >= 1000, "{frames}");
        let share = eyes as f64 / frames as f64;
        assert!(
            (0.07..=0.13).contains(&share),
            "{eyes} of {frames}: {share}"
        );
    }

    /// Where to stand for the portal-room screenshot, for each of the
    /// first strongholds (`cargo test portal_room_view -- --ignored --nocapture`).
    #[test]
    #[ignore]
    fn portal_room_view() {
        let terrain = Terrain::new(12345);
        for (i, &start) in positions(12345).iter().take(4).enumerate() {
            let hold = structure_at(&terrain, StructureKind::Stronghold, start).unwrap();
            let frames = frames_of(&hold);
            let sum: IVec3 = frames.iter().map(|f| f.0).sum();
            let center = sum.as_vec3() / 12.0;
            let (_, face, _) = frames
                .iter()
                .find(|(at, face, _)| {
                    let n = face.normal();
                    let behind = glam::Vec3::new(-n[0] as f32, 0.0, -n[2] as f32);
                    (at.as_vec3() - center - behind * 2.0).length() < 0.6
                })
                .unwrap();
            let n = face.normal();
            let feet = center + glam::Vec3::new(-n[0] as f32, 0.0, -n[2] as f32) * 8.0
                - glam::Vec3::Y * 3.0;
            let yaw = match (n[0], n[2]) {
                (0, -1) => 0.0,
                (1, 0) => std::f32::consts::FRAC_PI_2,
                (0, 1) => std::f32::consts::PI,
                _ => -std::f32::consts::FRAC_PI_2,
            };
            println!(
                "VIEW {i}: stronghold {start:?} frames at y {} center {center}; feet [{:.1}, {:.1}, {:.1}] yaw {yaw}",
                frames[0].0.y, feet.x, feet.y, feet.z
            );
        }
    }
}
