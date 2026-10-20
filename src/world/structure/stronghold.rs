//! Strongholds (modern `StrongholdPieces`): stone brick corridors,
//! crossings, chest corridors, prison halls and libraries joined from a
//! spiral staircase, with exactly one portal room: twelve End portal
//! frames round a 3 × 3 hole over lava, each frame holding an eye one
//! time in ten, and a silverfish spawner on its stairs.
//!
//! They stand in modern's rings around the world's center (3 in the first
//! ring, 1280–2816 blocks out, 6 in the second, 10 in the third), which
//! the eye of ender will point to (M27).

use glam::IVec3;

use super::{Bounds, Piece, build::Build};
use crate::world::{
    Face,
    block::BlockState,
    blocks,
    chunk::ChunkPos,
    worldgen::{rng::Rng, terrain::Terrain},
};

/// The rings: how many strongholds, and their distance out in blocks.
const RINGS: [(u32, f64, f64); 3] = [
    (3, 1280.0, 2816.0),
    (6, 4352.0, 5888.0),
    (10, 7424.0, 8960.0),
];
const SALT: u64 = 0x57_0E06_0001;
/// Pieces reach at most this far (blocks, across) from the start and
/// this many joins away.
const MAX_DISTANCE: i32 = 112;
const MAX_DEPTH: u32 = 8;
const MAX_PIECES: usize = 45;

/// The chunks strongholds start in, ring by ring.
pub fn positions(seed: u64) -> Vec<ChunkPos> {
    let mut rng = Rng::new(seed ^ SALT);
    let mut found = Vec::new();
    for (count, near, far) in RINGS {
        let start = rng.next_f64() * std::f64::consts::TAU;
        for i in 0..count {
            let angle = start + i as f64 * std::f64::consts::TAU / count as f64;
            let distance = near + rng.next_f64() * (far - near);
            found.push(ChunkPos::new(
                (angle.cos() * distance / 16.0).round() as i32,
                (angle.sin() * distance / 16.0).round() as i32,
            ));
        }
    }
    found
}

/// Stone bricks with mossy and cracked ones mixed in, the same every time.
fn brick(at: IVec3) -> BlockState {
    let h = (at.x as u32).wrapping_mul(73_856_093)
        ^ (at.y as u32).wrapping_mul(19_349_663)
        ^ (at.z as u32).wrapping_mul(83_492_791);
    match h.wrapping_mul(2_654_435_761) >> 28 {
        0..=1 => BlockState::of(blocks::MOSSY_STONE_BRICKS),
        2..=3 => BlockState::of(blocks::CRACKED_STONE_BRICKS),
        _ => BlockState::of(blocks::STONE_BRICKS),
    }
}

fn of(name: &str) -> BlockState {
    BlockState::parse(name).expect("a stronghold block")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Corridor,
    ChestCorridor,
    Prison,
    Crossing,
    Library,
    PortalRoom,
}

/// Where a piece may join: the first cell past the last piece's wall, at
/// floor level, and the way out.
#[derive(Debug, Clone, Copy)]
struct Exit {
    at: IVec3,
    facing: Face,
    depth: u32,
}

struct Hold<'a> {
    terrain: &'a Terrain,
    rng: &'a mut Rng,
    center: IVec3,
    taken: Vec<Bounds>,
    pieces: Vec<Piece>,
    libraries: u32,
    portal: bool,
}

/// A box `length` long from `at` along `facing`, `half` wide each side of
/// the middle and `height` high inside, floor at `at.y`.
fn shape(at: IVec3, facing: Face, length: i32, half: i32, height: i32) -> (Bounds, IVec3, IVec3) {
    let [nx, _, nz] = facing.normal();
    let dir = IVec3::new(nx, 0, nz);
    let side = IVec3::new(-nz, 0, nx);
    let a = at - side * half - IVec3::Y;
    let b = at + dir * (length - 1) + side * half + IVec3::Y * height;
    (Bounds::new(a, b), dir, side)
}

impl Hold<'_> {
    fn fits(&self, bounds: &Bounds) -> bool {
        let far = (bounds.min.x - self.center.x)
            .abs()
            .max((bounds.max.x - self.center.x).abs())
            > MAX_DISTANCE
            || (bounds.min.z - self.center.z)
                .abs()
                .max((bounds.max.z - self.center.z).abs())
                > MAX_DISTANCE;
        // Kept under the ground.
        let buried = bounds.max.y + 2 < self.terrain.height(bounds.center().x, bounds.center().z);
        !far && buried && bounds.min.y > 2 && !self.taken.iter().any(|t| t.intersects(bounds))
    }

    /// A stone brick shell over `bounds` with air inside and a doorway in
    /// the entrance wall at `at`.
    fn shell(&mut self, build: &mut Build, bounds: &Bounds, at: IVec3) {
        build.hollow(bounds.min, bounds.max, brick);
        // The doorway, two high, cut through this wall and the last
        // piece's.
        for y in 0..2 {
            build.set(at + IVec3::Y * y, BlockState::AIR);
        }
    }

    fn place(&mut self, kind: Kind, exit: Exit) -> Option<Vec<Exit>> {
        let Exit { at, facing, depth } = exit;
        let (length, half, height) = match kind {
            Kind::Corridor => (5 + 2 * self.rng.below(4) as i32, 2, 3),
            Kind::ChestCorridor => (7, 2, 3),
            Kind::Prison => (9, 3, 3),
            Kind::Crossing => (11, 5, 5),
            Kind::Library => (15, 7, 6),
            Kind::PortalRoom => (16, 5, 7),
        };
        let (bounds, dir, side) = shape(at, facing, length, half, height);
        if !self.fits(&bounds) {
            return None;
        }
        self.taken.push(bounds);
        let mut build = Build::new();
        self.shell(&mut build, &bounds, at);
        // The doorway's other half, in the last piece's wall.
        for y in 0..2 {
            build.set(at - dir + IVec3::Y * y, BlockState::AIR);
        }
        let mut exits = Vec::new();
        let end = at + dir * length;
        let next = |at: IVec3, facing: Face| Exit {
            at,
            facing,
            depth: depth + 1,
        };
        match kind {
            Kind::Corridor | Kind::ChestCorridor => {
                exits.push(next(end, facing));
                if kind == Kind::Corridor && self.rng.below(2) == 0 {
                    let middle = at + dir * (length / 2);
                    let (left, right) = (facing.rotate_ccw(), facing.rotate_cw());
                    exits.push(next(middle - side * (half + 1), left));
                    exits.push(next(middle + side * (half + 1), right));
                }
                if kind == Kind::ChestCorridor {
                    let chest_at = at + dir * 3 + side * 1;
                    let face = super::template::face_name(facing.rotate_ccw());
                    build.chest(
                        chest_at,
                        of(&format!("chest[facing={face}]")),
                        "stronghold_corridor",
                    );
                }
                if self.rng.below(3) == 0 {
                    let face = super::template::face_name(facing.rotate_cw());
                    build.set(
                        at + dir + side * -1 + IVec3::Y,
                        of(&format!("wall_torch[facing={face}]")),
                    );
                }
            }
            Kind::Prison => {
                exits.push(next(end, facing));
                // Cells behind iron bars down one side.
                for i in 1..length - 1 {
                    let wall = at + dir * i + side * 1;
                    for y in 0..3 {
                        let block = if i % 3 == 0 {
                            brick(wall)
                        } else {
                            of("iron_bars")
                        };
                        build.set(wall + IVec3::Y * y, block);
                    }
                }
            }
            Kind::Crossing => {
                exits.push(next(end, facing));
                let middle = at + dir * (length / 2);
                exits.push(next(middle - side * (half + 1), facing.rotate_ccw()));
                exits.push(next(middle + side * (half + 1), facing.rotate_cw()));
                // A pillar and a fountain-less middle; sometimes a chest.
                for y in 0..height {
                    build.set(middle + IVec3::Y * y, brick(middle + IVec3::Y * y));
                }
                build.set(middle + dir * -1 + IVec3::Y * 2, of("torch"));
                if self.rng.below(3) == 0 {
                    build.chest(middle + side * 3, of("chest"), "stronghold_crossing");
                }
            }
            Kind::Library => {
                self.libraries += 1;
                for i in 1..length - 1 {
                    for s in [-half + 1, half - 1] {
                        for y in 0..height - 1 {
                            build.set(at + dir * i + side * s + IVec3::Y * y, of("bookshelf"));
                        }
                    }
                    if i % 4 == 2 {
                        for s in -half + 3..=half - 3 {
                            if s != 0 {
                                for y in 0..3 {
                                    build.set(
                                        at + dir * i + side * s + IVec3::Y * y,
                                        of("bookshelf"),
                                    );
                                }
                            }
                        }
                    }
                    if self.rng.below(5) == 0 {
                        build.set(
                            at + dir * i + side * (half - 2) + IVec3::Y * (height - 1),
                            of("cobweb"),
                        );
                    }
                }
                build.chest(
                    at + dir * (length - 2) + side * 2,
                    of("chest"),
                    "stronghold_library",
                );
                build.chest(
                    at + dir * (length - 2) - side * 2,
                    of("chest"),
                    "stronghold_library",
                );
            }
            Kind::PortalRoom => {
                self.portal = true;
                self.portal_room(&mut build, at, dir, side);
            }
        }
        self.pieces.push(build.finish());
        Some(exits)
    }

    /// The portal room's insides: steps up to a platform, the frames round
    /// a pool of lava, iron-barred windows and the silverfish spawner.
    fn portal_room(&mut self, build: &mut Build, at: IVec3, dir: IVec3, side: IVec3) {
        // Steps up to the platform.
        for i in 1..=3 {
            for s in -1..=1 {
                for y in 0..i {
                    build.set(at + dir * i + side * s + IVec3::Y * y, brick(at + dir * i));
                }
            }
        }
        build.spawner(at + dir * 3 + side * 2, "silverfish");
        // The platform and the ring of frames, 5 × 5 round a 3 × 3 middle.
        let middle = at + dir * 10 + IVec3::Y * 3;
        for i in -3..=3 {
            for s in -3..=3 {
                for y in 0..3 {
                    build.set(
                        middle + dir * i + side * s - IVec3::Y * (y + 1),
                        brick(middle + dir * i + side * s),
                    );
                }
            }
        }
        let mut eyes = 0;
        let mut frames = Vec::new();
        for i in -2i32..=2 {
            for s in -2i32..=2 {
                let edge = i.abs() == 2 || s.abs() == 2;
                let corner = i.abs() == 2 && s.abs() == 2;
                let at = middle + dir * i + side * s;
                if !edge {
                    // Lava under where the portal goes.
                    build.set(at - IVec3::Y, of("lava"));
                    build.set(at, BlockState::AIR);
                    continue;
                }
                if corner {
                    continue;
                }
                // Frames face into the middle.
                let inward = if i == -2 {
                    dir
                } else if i == 2 {
                    -dir
                } else if s == -2 {
                    side
                } else {
                    -side
                };
                let face = match (inward.x, inward.z) {
                    (1, _) => "east",
                    (-1, _) => "west",
                    (_, 1) => "south",
                    _ => "north",
                };
                let eye = self.rng.below(10) == 0;
                eyes += eye as u32;
                frames.push(at);
                build.set(
                    at,
                    of(&format!("end_portal_frame[facing={face},eye={eye}]")),
                );
            }
        }
        // All twelve eyes in place: the portal is open.
        if eyes == 12 {
            for i in -1..=1 {
                for s in -1..=1 {
                    build.set(middle + dir * i + side * s, of("end_portal"));
                }
            }
        }
        // Windows of iron bars along the sides.
        for i in [5, 8, 11] {
            for s in [-5, 5] {
                build.set(at + dir * i + side * s + IVec3::Y * 3, of("iron_bars"));
            }
        }
    }

    /// The spiral staircase down from near the surface, then the rest.
    fn generate(&mut self) {
        let top = self.center;
        // The stair tower: 5 × 5, ten high, stairs winding down.
        let a = top - IVec3::new(2, 11, 2);
        let b = top + IVec3::new(2, 0, 2);
        let bounds = Bounds::new(a, b);
        self.taken.push(bounds);
        let mut build = Build::new();
        build.hollow(a, b, brick);
        let ring = [
            (-1, -1),
            (0, -1),
            (1, -1),
            (1, 0),
            (1, 1),
            (0, 1),
            (-1, 1),
            (-1, 0),
        ];
        for (i, y) in (a.y + 1..b.y).enumerate() {
            let (dx, dz) = ring[i % ring.len()];
            build.set(
                IVec3::new(top.x + dx, y, top.z + dz),
                brick(IVec3::new(dx, y, dz)),
            );
        }
        build.set(IVec3::new(top.x, a.y + 1, top.z), BlockState::AIR);
        self.pieces.push(build.finish());
        let floor = a.y + 1;
        let mut open = std::collections::VecDeque::from([Exit {
            at: IVec3::new(top.x, floor, b.z + 1),
            facing: Face::South,
            depth: 0,
        }]);
        for y in 0..2 {
            self.pieces.push({
                let mut door = Build::new();
                door.set(IVec3::new(top.x, floor + y, b.z), BlockState::AIR);
                door.finish()
            });
        }
        let mut dead = Vec::new();
        while let Some(exit) = open.pop_front() {
            if self.pieces.len() >= MAX_PIECES || exit.depth >= MAX_DEPTH {
                dead.push(exit);
                continue;
            }
            let kind = self.pick(exit.depth);
            match self.place(kind, exit) {
                Some(exits) => open.extend(exits),
                None => dead.push(exit),
            }
        }
        // Every stronghold has its portal room.
        if !self.portal {
            for exit in dead {
                if self.place(Kind::PortalRoom, exit).is_some() {
                    break;
                }
            }
        }
    }

    fn pick(&mut self, depth: u32) -> Kind {
        let roll = self.rng.below(100);
        match roll {
            0..=29 => Kind::Corridor,
            30..=44 => Kind::Crossing,
            45..=59 => Kind::ChestCorridor,
            60..=69 => Kind::Prison,
            70..=79 if self.libraries < 2 && depth >= 2 => Kind::Library,
            80..=89 if !self.portal && depth >= 3 => Kind::PortalRoom,
            _ => Kind::Corridor,
        }
    }
}

pub fn generate(terrain: &Terrain, start: ChunkPos, rng: &mut Rng) -> Option<Vec<Piece>> {
    let (x, z) = start.min_block();
    let (x, z) = (x + 8, z + 8);
    let top = (terrain.height(x, z) - 8).clamp(30, 60);
    let mut hold = Hold {
        terrain,
        rng,
        center: IVec3::new(x, top, z),
        taken: Vec::new(),
        pieces: Vec::new(),
        libraries: 0,
        portal: false,
    };
    hold.generate();
    Some(hold.pieces)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::structure::{Kind as StructureKind, structure_at};

    #[test]
    fn the_first_ring_has_three_strongholds_1280_to_2816_blocks_out() {
        let found = positions(42);
        assert_eq!(found.len(), 19);
        for pos in &found[..3] {
            let (x, z) = pos.min_block();
            let distance = ((x * x + z * z) as f64).sqrt();
            assert!((1200.0..2900.0).contains(&distance), "{distance}");
        }
        assert_eq!(positions(42), found);
    }

    #[test]
    fn every_stronghold_has_a_portal_room_with_twelve_frames() {
        let terrain = Terrain::new(20111118);
        for &start in positions(terrain.seed()).iter().take(3) {
            let hold = structure_at(&terrain, StructureKind::Stronghold, start).unwrap();
            let frames: usize = hold
                .pieces
                .iter()
                .flat_map(|p| &p.blocks)
                .filter(|(_, b)| b.is(blocks::END_PORTAL_FRAME))
                .count();
            assert_eq!(frames, 12, "{start:?}");
            let silverfish = hold
                .pieces
                .iter()
                .flat_map(|p| &p.spawners)
                .filter(|(_, m)| m == "silverfish")
                .count();
            assert_eq!(silverfish, 1);
            assert!(hold.pieces.len() >= 10, "{}", hold.pieces.len());
        }
    }
}
