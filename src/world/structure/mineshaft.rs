//! Abandoned mineshafts (modern `MineshaftPieces`): a dirt-floored room
//! with corridors leaving it, each 3 wide and 3 high with a wooden support
//! every five blocks, rails, cobwebs, now and then a cave spider spawner
//! wrapped in webs and a chest minecart, ending in crossings that branch
//! again, eight joins deep at most.

use glam::{DVec3, IVec3};

use super::{Bounds, Piece, build::Build};
use crate::world::{
    Face,
    block::BlockState,
    chunk::ChunkPos,
    worldgen::{rng::Rng, terrain::Terrain},
};

/// Modern's chance of a mineshaft starting in a chunk.
pub const CHANCE: f64 = 0.004;
/// Joins from the room, and how far (in blocks) pieces reach from it.
const MAX_DEPTH: u32 = 8;
const MAX_DISTANCE: i32 = 80;

fn of(name: &str) -> BlockState {
    BlockState::parse(name).expect("a mineshaft block")
}

struct Shaft<'a> {
    terrain: &'a Terrain,
    rng: &'a mut Rng,
    center: IVec3,
    taken: Vec<Bounds>,
    pieces: Vec<Piece>,
}

impl Shaft<'_> {
    /// Whether a piece in `bounds` fits: near the room, not crossing
    /// another, and well under the ground.
    fn fits(&self, bounds: &Bounds) -> bool {
        let far = (bounds.min.x - self.center.x)
            .abs()
            .max((bounds.max.x - self.center.x).abs())
            > MAX_DISTANCE
            || (bounds.min.z - self.center.z)
                .abs()
                .max((bounds.max.z - self.center.z).abs())
                > MAX_DISTANCE;
        let shallow = [
            (bounds.min.x, bounds.min.z),
            (bounds.max.x, bounds.max.z),
            (bounds.min.x, bounds.max.z),
            (bounds.max.x, bounds.min.z),
        ]
        .into_iter()
        .any(|(x, z)| bounds.max.y + 4 > self.terrain.height(x, z));
        !far && !shallow && bounds.min.y > 4 && !self.taken.iter().any(|t| t.intersects(bounds))
    }

    /// The starting room, and corridors from its four sides.
    fn room(&mut self) {
        let (w, d) = (7 + self.rng.below(6) as i32, 7 + self.rng.below(6) as i32);
        let lo = self.center - IVec3::new(w / 2, 0, d / 2);
        let hi = lo + IVec3::new(w - 1, 4, d - 1);
        let mut build = Build::new();
        build.fill(lo, IVec3::new(hi.x, lo.y, hi.z), of("dirt"));
        build.fill(lo + IVec3::Y, hi, BlockState::AIR);
        self.taken.push(Bounds::new(lo, hi));
        self.pieces.push(build.finish());
        let middle = (lo + hi) / 2;
        for face in [Face::North, Face::East, Face::South, Face::West] {
            let edge = match face {
                Face::North => IVec3::new(middle.x, lo.y + 1, lo.z - 1),
                Face::South => IVec3::new(middle.x, lo.y + 1, hi.z + 1),
                Face::West => IVec3::new(lo.x - 1, lo.y + 1, middle.z),
                _ => IVec3::new(hi.x + 1, lo.y + 1, middle.z),
            };
            self.corridor(edge, face, 1);
        }
    }

    /// A corridor from `start` (the middle of its first floor block)
    /// running `facing`.
    fn corridor(&mut self, start: IVec3, facing: Face, depth: u32) {
        if depth > MAX_DEPTH {
            return;
        }
        let [nx, _, nz] = facing.normal();
        let dir = IVec3::new(nx, 0, nz);
        let side = IVec3::new(-nz, 0, nx);
        let sections = 2 + self.rng.below(4) as i32;
        let length = sections * 5;
        let end = start + dir * (length - 1);
        let bounds = Bounds::new(start - side - IVec3::Y, end + side + IVec3::Y * 3);
        if !self.fits(&bounds) {
            return;
        }
        self.taken.push(bounds);
        let mut build = Build::new();
        let planks = of("oak_planks");
        let fence = of("oak_fence");
        let rail = if nx != 0 {
            of("rail[shape=east_west]")
        } else {
            of("rail[shape=north_south]")
        };
        let rails = self.rng.below(3) != 0;
        let spider = self.rng.below(23) == 0;
        for i in 0..length {
            let at = start + dir * i;
            for s in -1..=1 {
                let floor = at + side * s;
                build.set(floor - IVec3::Y, planks);
                for y in 0..3 {
                    build.set(floor + IVec3::Y * y, BlockState::AIR);
                }
            }
            if i % 5 == 2 {
                // A support: two posts and a beam.
                for s in [-1, 1] {
                    build.set(at + side * s, fence);
                    build.set(at + side * s + IVec3::Y, fence);
                }
                for s in -1..=1 {
                    build.set(at + side * s + IVec3::Y * 2, planks);
                }
                if self.rng.below(8) == 0 {
                    let face = if nx != 0 { "south" } else { "east" };
                    build.set(
                        at + IVec3::Y + side,
                        of(&format!("wall_torch[facing={face}]")),
                    );
                }
            } else {
                if rails && self.rng.below(10) != 0 {
                    build.set(at, rail);
                }
                // Cobwebs in the corners.
                if self.rng.below(12) == 0 {
                    let s = if self.rng.below(2) == 0 { -1 } else { 1 };
                    build.set(at + side * s + IVec3::Y * 2, of("cobweb"));
                }
            }
            if self.rng.below(100) == 0 {
                build.mob(
                    at.as_dvec3() + DVec3::new(0.5, 0.0, 0.5),
                    "chest_minecart",
                    Some("abandoned_mineshaft"),
                );
            }
        }
        if spider {
            let at = start + dir * (length / 2 + 1);
            build.spawner(at, "cave_spider");
            for dz in -1..=1 {
                for dy in 0..=2 {
                    for s in -1..=1 {
                        let web = at + dir * dz + side * s + IVec3::Y * dy;
                        if web != at && self.rng.below(2) == 0 {
                            build.set(web, of("cobweb"));
                        }
                    }
                }
            }
        }
        self.pieces.push(build.finish());
        // What's at the far end: a crossing, a turn, or nothing.
        let next = end + dir;
        match self.rng.below(5) {
            0 | 1 => self.crossing(next, facing, depth + 1),
            2 => {
                let turn = if self.rng.below(2) == 0 {
                    facing.rotate_cw()
                } else {
                    facing.rotate_ccw()
                };
                self.corridor(next, turn, depth + 1);
            }
            _ => self.corridor(next, facing, depth + 1),
        }
    }

    /// A 3 × 3 crossing (sometimes two storeys tall) with corridors out of
    /// its other sides.
    fn crossing(&mut self, start: IVec3, facing: Face, depth: u32) {
        let [nx, _, nz] = facing.normal();
        let dir = IVec3::new(nx, 0, nz);
        let center = start + dir;
        let tall = self.rng.below(4) == 0;
        let height = if tall { 6 } else { 3 };
        let bounds = Bounds::new(
            center - IVec3::new(1, 1, 1),
            center + IVec3::new(1, height, 1),
        );
        if !self.fits(&bounds) {
            return;
        }
        self.taken.push(bounds);
        let mut build = Build::new();
        for dx in -1..=1 {
            for dz in -1..=1 {
                let at = center + IVec3::new(dx, 0, dz);
                build.set(at - IVec3::Y, of("oak_planks"));
                for y in 0..height {
                    build.set(at + IVec3::Y * y, BlockState::AIR);
                }
            }
        }
        if tall {
            for (dx, dz) in [(-1, -1), (1, -1), (-1, 1), (1, 1)] {
                for y in 0..height {
                    build.set(center + IVec3::new(dx, y, dz), of("oak_planks"));
                }
            }
        }
        self.pieces.push(build.finish());
        for face in [facing, facing.rotate_cw(), facing.rotate_ccw()] {
            if self.rng.below(4) != 0 {
                let [fx, _, fz] = face.normal();
                self.corridor(center + IVec3::new(fx, 0, fz) * 2, face, depth + 1);
            }
        }
    }
}

pub fn generate(terrain: &Terrain, start: ChunkPos, rng: &mut Rng) -> Option<Vec<Piece>> {
    let (x, z) = start.min_block();
    let (x, z) = (x + 8, z + 8);
    let ground = terrain.height(x, z);
    let top = (ground - 12).min(50);
    if top < 12 {
        return None;
    }
    let y = 10 + rng.below((top - 10) as u32) as i32;
    let mut shaft = Shaft {
        terrain,
        rng,
        center: IVec3::new(x, y, z),
        taken: Vec::new(),
        pieces: Vec::new(),
    };
    shaft.room();
    (shaft.pieces.len() > 1).then_some(shaft.pieces)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::structure::{Kind, structure_at};

    #[test]
    fn a_mineshaft_has_a_room_and_branching_corridors_underground() {
        let terrain = Terrain::new(20111118);
        let mut biggest = 0;
        for z in 0..6 {
            for x in 0..6 {
                if let Some(shaft) = structure_at(&terrain, Kind::Mineshaft, ChunkPos::new(x, z)) {
                    biggest = biggest.max(shaft.pieces.len());
                    for piece in &shaft.pieces {
                        let b = piece.bounds.unwrap();
                        assert!(b.max.y + 3 < terrain.height(b.min.x, b.min.z) + 4);
                    }
                }
            }
        }
        assert!(biggest >= 10, "{biggest}");
    }
}
