//! The End's generator (M27): the main island, the obsidian pillars with
//! their crystals' bedrock, the podium the exit portal stands on, and the
//! End gateways' towers.
//!
//! - **Island**: one disc of end stone about 150 blocks across, flat on top
//!   at y 63 and rounded underneath, with a ragged edge. The rest is void
//!   (modern's outer islands start at M28).
//! - **Pillars** (modern `SpikeFeature`): ten obsidian columns round the
//!   island, 42 blocks from the middle, 2–5 radius and 76–103 tall (which
//!   one is tallest depends on the seed). Each ends in bedrock with a fire
//!   on it and an end crystal above; two are caged in iron bars.
//! - **Podium** (modern `EndPodiumFeature`): a bedrock bowl round the
//!   island's middle with a pillar and four torches. It is a hole until the
//!   dragon dies and a portal after.
//!
//! The island is worked out from the position alone, so any chunk draws its
//! part the same way; pillars are written clipped to the chunk.

use glam::{DVec3, IVec3};
use noise::{NoiseFn, Perlin};

use super::{
    climate::{noise_offset, noise_seed},
    rng::{Rng, mix},
};
use crate::{
    config::CHUNK_SIZE,
    world::{
        Face,
        biome::Biome,
        block::BlockState,
        blocks,
        chunk::{Chunk, ChunkPos},
        props::Prop,
    },
};

/// The top block of the main island, and the podium's level.
pub const SURFACE_Y: i32 = 63;
/// Where the exit portal stands: the island's middle.
pub const PODIUM: IVec3 = IVec3::new(0, SURFACE_Y, 0);
/// The island's usual radius and how far its edge wanders.
const RADIUS: f64 = 74.0;
const RAGGED: f64 = 9.0;
/// How deep the island is under its middle.
const DEPTH: f64 = 38.0;
/// How far from the middle the pillars stand.
const SPIKE_RING: f64 = 42.0;
/// Where a player arriving from the Overworld stands (modern
/// `END_SPAWN_POINT`), on a 5 × 5 obsidian platform one block down.
pub const PLATFORM: IVec3 = IVec3::new(100, 50, 0);
/// Where the dragon's gateways stand (modern: 96 out, y 75).
const GATEWAY_RING: f64 = 96.0;
const GATEWAY_Y: i32 = 75;
pub const GATEWAYS: usize = 20;

/// Outer islands start this far from the middle (modern's start around
/// 1000; 20 blocks of void to 600 keeps the first ones within reach of a
/// gateway's 1024).
pub const OUTER_START: f64 = 600.0;
/// Chorus plants per chunk on the outer islands, at most (modern's 0–4).
const CHORUS_PER_CHUNK: u32 = 4;
const CHORUS_SALT: u64 = 0x43_484F_5255;
/// The outer islands' grid, in blocks, and the chance in 100 a cell has
/// one.
const CELL: i32 = 96;
const ISLAND_CHANCE: u64 = 60;

/// One outer island: its middle column, its radius and its flat top.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Island {
    center: (i32, i32),
    radius: i32,
    base: i32,
}

const END_STONE: BlockState = BlockState::of(blocks::END_STONE);
const OBSIDIAN: BlockState = BlockState::of(blocks::OBSIDIAN);
const BEDROCK: BlockState = BlockState::of(blocks::BEDROCK);

pub struct End {
    seed: u64,
    edge: Perlin,
    rough: Perlin,
    shift: [f64; 3],
}

impl End {
    pub fn new(seed: u64) -> Self {
        let own = mix(seed ^ 0x0000_454E_4400_0001);
        Self {
            seed,
            edge: Perlin::new(noise_seed(own, 1)),
            rough: Perlin::new(noise_seed(own, 2)),
            shift: noise_offset(own, 0),
        }
    }

    pub fn world_seed(&self) -> u64 {
        self.seed
    }

    /// How far the island reaches from the middle at the heading of
    /// (x, z).
    fn radius(&self, x: f64, z: f64) -> f64 {
        let d = x.hypot(z).max(1.0);
        let around = self
            .edge
            .get([x / d * 1.4 + self.shift[0], z / d * 1.4 + self.shift[1]]);
        let wobble = self
            .rough
            .get([x / 26.0 + self.shift[1], z / 26.0 + self.shift[2]]);
        RADIUS + around * RAGGED + wobble * 4.0
    }

    /// The main island's top and bottom blocks in the column at (x, z).
    fn main_span(&self, x: i32, z: i32) -> Option<(i32, i32)> {
        if x.abs() > 100 || z.abs() > 100 {
            return None;
        }
        let (fx, fz) = (f64::from(x), f64::from(z));
        let d = fx.hypot(fz);
        let radius = self.radius(fx, fz);
        if d >= radius {
            return None;
        }
        // Flat out to 14 blocks from the edge, then the rim sinks.
        let sink = ((d - (radius - 14.0)) / 14.0).max(0.0);
        let bumps = if d < 8.0 {
            0.0
        } else {
            self.rough
                .get([fx / 11.0 + self.shift[2], fz / 11.0 + self.shift[0]])
                * 1.2
        };
        let top = SURFACE_Y + (bumps - sink * sink * 6.0).round() as i32;
        let across = (d / radius).min(1.0);
        let belly = DEPTH * (1.0 - across * across).sqrt();
        let jag = self
            .rough
            .get([fx / 9.0 + self.shift[0], fz / 9.0 + self.shift[2]])
            * 2.0;
        let bottom = ((f64::from(SURFACE_Y) - belly + jag).round() as i32).min(top);
        Some((top, bottom))
    }

    /// The island of the outer cell (`cx`, `cz`), if it has one: a disc
    /// 12–43 blocks wide at its edge, a flat top between y 56 and 75.
    fn island_in(&self, cx: i32, cz: i32) -> Option<Island> {
        let h =
            mix(self.seed ^ 0x4F55_5445_5200 ^ mix((cx as u32 as u64) << 32 | cz as u32 as u64));
        if h % 100 >= ISLAND_CHANCE {
            return None;
        }
        let pick = |shift: u32, n: u64| (mix(h >> shift ^ 0x9E37) % n) as i32;
        let center = (
            cx * CELL + 20 + pick(3, (CELL - 40) as u64),
            cz * CELL + 20 + pick(7, (CELL - 40) as u64),
        );
        let reach = f64::from(center.0).hypot(f64::from(center.1));
        let radius = 12 + pick(11, 32);
        (reach >= OUTER_START).then_some(Island {
            center,
            radius,
            base: 56 + pick(17, 20),
        })
    }

    /// The outer islands whose discs could reach (x, z).
    fn islands_near(&self, x: i32, z: i32) -> Vec<Island> {
        if f64::from(x).hypot(f64::from(z)) < OUTER_START - 60.0 {
            return Vec::new();
        }
        let (cx, cz) = (x.div_euclid(CELL), z.div_euclid(CELL));
        let mut found = Vec::new();
        for dz in -1..=1 {
            for dx in -1..=1 {
                found.extend(self.island_in(cx + dx, cz + dz));
            }
        }
        found
    }

    /// The top and bottom blocks of every island's column at (x, z): the
    /// main island's, then the outer ones'.
    fn spans(&self, x: i32, z: i32) -> Vec<(i32, i32)> {
        let mut spans: Vec<(i32, i32)> = self.main_span(x, z).into_iter().collect();
        let (fx, fz) = (f64::from(x), f64::from(z));
        for island in self.islands_near(x, z) {
            let (dx, dz) = (
                fx - f64::from(island.center.0),
                fz - f64::from(island.center.1),
            );
            let wobble = self
                .rough
                .get([fx / 13.0 + self.shift[1], fz / 13.0 + self.shift[2]])
                * 3.0;
            let radius = f64::from(island.radius) + wobble;
            let d = dx.hypot(dz);
            if d >= radius {
                continue;
            }
            let sink = ((d - (radius - 6.0)) / 6.0).max(0.0);
            let top = island.base - (sink * sink * 3.0).round() as i32;
            let across = d / radius;
            let belly = radius * 0.5 * (1.0 - across * across).sqrt();
            let bottom = ((f64::from(island.base) - belly).round() as i32).min(top);
            spans.push((top, bottom));
        }
        spans
    }

    /// The top solid block of the column at (x, z), if it is over an
    /// island.
    pub fn top(&self, x: i32, z: i32) -> Option<i32> {
        self.spans(x, z).into_iter().map(|(top, _)| top).max()
    }

    /// Whether (x, y, z) is end stone of an island.
    pub fn solid(&self, x: i32, y: i32, z: i32) -> bool {
        self.spans(x, z)
            .into_iter()
            .any(|(top, bottom)| y <= top && y >= bottom)
    }

    /// The outer island whose middle is nearest (x, z), with its top: where
    /// a gateway leads.
    pub fn island_near(&self, x: i32, z: i32) -> Option<(IVec3, i32)> {
        let (cx, cz) = (x.div_euclid(CELL), z.div_euclid(CELL));
        let mut best: Option<(f64, IVec3, i32)> = None;
        for dz in -4..=4 {
            for dx in -4..=4 {
                let Some(island) = self.island_in(cx + dx, cz + dz) else {
                    continue;
                };
                // Small islands are a poor welcome.
                if island.radius < 16 {
                    continue;
                }
                let d = f64::from(island.center.0 - x).hypot(f64::from(island.center.1 - z));
                if best.is_none_or(|(b, _, _)| d < b) {
                    best = Some((
                        d,
                        IVec3::new(island.center.0, island.base, island.center.1),
                        island.radius,
                    ));
                }
            }
        }
        best.map(|(_, at, radius)| (at, radius))
    }

    /// Every outer island of at least `radius` blocks whose middle is in
    /// the square of `size` blocks from (`x`, `z`), for placing cities.
    pub fn big_islands_in(&self, x: i32, z: i32, size: i32, radius: i32) -> Vec<(IVec3, i32)> {
        let mut found = Vec::new();
        for cz in z.div_euclid(CELL)..=(z + size).div_euclid(CELL) {
            for cx in x.div_euclid(CELL)..=(x + size).div_euclid(CELL) {
                if let Some(island) = self.island_in(cx, cz)
                    && island.radius >= radius
                    && (x..x + size).contains(&island.center.0)
                    && (z..z + size).contains(&island.center.1)
                {
                    found.push((
                        IVec3::new(island.center.0, island.base, island.center.1),
                        island.radius,
                    ));
                }
            }
        }
        found
    }

    /// The chorus plants rooted in chunk `pos` (none to four of them, on the
    /// outer islands' end stone), grown whole, so a chunk can draw the part of
    /// a neighbor's plant that leans over it.
    fn chorus_plants(&self, pos: ChunkPos) -> Vec<(IVec3, BlockState)> {
        let mut rng = Rng::for_chunk(self.seed, pos.x, pos.z, CHORUS_SALT);
        let (x0, z0) = pos.min_block();
        let mut blocks = Vec::new();
        for _ in 0..rng.below(CHORUS_PER_CHUNK + 1) {
            let (x, z) = (
                x0 + rng.below(CHUNK_SIZE as u32) as i32,
                z0 + rng.below(CHUNK_SIZE as u32) as i32,
            );
            if f64::from(x).hypot(f64::from(z)) < OUTER_START - 60.0 {
                continue;
            }
            let Some(top) = self.top(x, z) else {
                continue;
            };
            let mut growth = Rng::new(mix(self.seed
                ^ CHORUS_SALT
                ^ mix((x as u32 as u64) << 32 | z as u32 as u64)));
            blocks.extend(crate::world::chorus::grow_plant(
                IVec3::new(x, top + 1, z),
                &mut growth,
                &|at| self.solid(at.x, at.y, at.z),
            ));
        }
        blocks
    }

    pub fn generate(&self, pos: ChunkPos) -> Chunk {
        let mut chunk = Chunk::new(pos);
        let (x0, z0) = pos.min_block();
        for lz in 0..CHUNK_SIZE {
            for lx in 0..CHUNK_SIZE {
                chunk.set_biome(lx, lz, Biome::TheEnd);
                let (x, z) = (x0 + lx as i32, z0 + lz as i32);
                let spans = self.spans(x, z);
                let Some(_) = spans.iter().max_by_key(|(top, _)| *top) else {
                    continue;
                };
                for (top, bottom) in spans {
                    for y in bottom.max(1)..=top {
                        chunk.set(lx, y, lz, END_STONE);
                    }
                }
            }
        }
        let clip = |at: IVec3, block: BlockState| {
            let (min_x, min_z) = pos.min_block();
            let (lx, lz) = (at.x - min_x, at.z - min_z);
            if (0..CHUNK_SIZE as i32).contains(&lx)
                && (0..CHUNK_SIZE as i32).contains(&lz)
                && crate::world::chunk::in_height(at.y)
            {
                Some((lx as usize, at.y, lz as usize, block))
            } else {
                None
            }
        };
        let put = |chunk: &mut Chunk, edits: Vec<(IVec3, BlockState)>| {
            for (at, block) in edits {
                if let Some((x, y, z, block)) = clip(at, block) {
                    chunk.set(x, y, z, block);
                }
            }
        };
        // Plants lean at most 8 blocks, so only the neighbors' reach this chunk.
        if f64::from(x0).hypot(f64::from(z0)) > OUTER_START - 200.0 {
            for dz in -1..=1 {
                for dx in -1..=1 {
                    put(&mut chunk, self.chorus_plants(pos.offset(dx, dz)));
                }
            }
        }
        for spike in spikes(self.seed) {
            if spike.reaches(pos) {
                put(&mut chunk, spike.blocks());
            }
        }
        if pos.x.abs() <= 1 && pos.z.abs() <= 1 {
            put(&mut chunk, podium(PODIUM, false));
        }
        // End cities and their ships on the outer islands.
        if f64::from(x0).hypot(f64::from(z0)) > OUTER_START - 200.0 {
            crate::world::structure::end::place(self, &mut chunk);
        }
        chunk
    }
}

/// One obsidian pillar (modern `SpikeFeature.EndSpike`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Spike {
    pub x: i32,
    pub z: i32,
    pub radius: i32,
    /// Where the bedrock on top is; the crystal floats just above.
    pub height: i32,
    pub guarded: bool,
}

/// The ten pillars of a world: modern's, handed out round the ring in an
/// order the seed shuffles.
pub fn spikes(seed: u64) -> Vec<Spike> {
    let mut order: Vec<i32> = (0..10).collect();
    let mut rng = Rng::new(mix(seed ^ 0x0000_5350_494B_4553));
    for i in (1..order.len()).rev() {
        order.swap(i, rng.below(i as u32 + 1) as usize);
    }
    (0..10)
        .map(|slot| {
            let i = order[slot];
            let angle = f64::from(slot as i32) * std::f64::consts::TAU / 10.0;
            Spike {
                x: (SPIKE_RING * angle.cos()).floor() as i32,
                z: (SPIKE_RING * angle.sin()).floor() as i32,
                radius: 2 + i / 3,
                height: 76 + i * 3,
                guarded: i == 1 || i == 2,
            }
        })
        .collect()
}

impl Spike {
    /// Where its crystal floats.
    pub fn crystal(&self) -> DVec3 {
        DVec3::new(
            f64::from(self.x) + 0.5,
            f64::from(self.height) + 1.0,
            f64::from(self.z) + 0.5,
        )
    }

    /// Whether any of it lies in `chunk` (its cage reaches two blocks out).
    fn reaches(&self, chunk: ChunkPos) -> bool {
        let (x0, z0) = chunk.min_block();
        let reach = self.radius.max(2);
        self.x + reach >= x0
            && self.x - reach < x0 + CHUNK_SIZE as i32
            && self.z + reach >= z0
            && self.z - reach < z0 + CHUNK_SIZE as i32
    }

    /// The pillar, its bedrock top and fire, and the cage if it has one.
    pub fn blocks(&self) -> Vec<(IVec3, BlockState)> {
        let r = self.radius;
        let mut out = Vec::new();
        for dz in -r..=r {
            for dx in -r..=r {
                if dx * dx + dz * dz > r * r + 1 {
                    continue;
                }
                for y in 1..self.height {
                    out.push((IVec3::new(self.x + dx, y, self.z + dz), OBSIDIAN));
                }
                // The air round the top, in case the island rises there.
                for y in self.height + 1..self.height + 10 {
                    out.push((IVec3::new(self.x + dx, y, self.z + dz), BlockState::AIR));
                }
            }
        }
        out.push((IVec3::new(self.x, self.height, self.z), BEDROCK));
        out.push((
            IVec3::new(self.x, self.height + 1, self.z),
            BlockState::of(blocks::FIRE),
        ));
        if self.guarded {
            for dz in -2i32..=2 {
                for dx in -2i32..=2 {
                    for dy in 0..=3 {
                        if dx.abs() != 2 && dz.abs() != 2 && dy != 3 {
                            continue;
                        }
                        let bars = BlockState::of(blocks::IRON_BARS)
                            .with_bool(Prop::West, dx == -2)
                            .with_bool(Prop::East, dx == 2)
                            .with_bool(Prop::North, dz == -2)
                            .with_bool(Prop::South, dz == 2);
                        out.push((
                            IVec3::new(self.x + dx, self.height + 1 + dy, self.z + dz),
                            bars,
                        ));
                    }
                }
            }
        }
        out
    }
}

/// The podium round `origin` (modern `EndPodiumFeature`): a bedrock bowl
/// over end stone with a bedrock pillar and four torches; the bowl's
/// floor is air, or the End portal once `active`.
pub fn podium(origin: IVec3, active: bool) -> Vec<(IVec3, BlockState)> {
    let mut out = Vec::new();
    for dy in -1..=32 {
        for dz in -4..=4 {
            for dx in -4..=4 {
                let d2 = dx * dx + dy * dy + dz * dz;
                let inner = d2 * 4 < 25;
                if !inner && d2 * 4 >= 49 {
                    continue;
                }
                let block = if dy < 0 {
                    if inner { BEDROCK } else { END_STONE }
                } else if dy > 0 {
                    BlockState::AIR
                } else if !inner {
                    BEDROCK
                } else if active {
                    BlockState::of(blocks::END_PORTAL)
                } else {
                    BlockState::AIR
                };
                out.push((origin + IVec3::new(dx, dy, dz), block));
            }
        }
    }
    for dy in 0..4 {
        out.push((origin + IVec3::Y * dy, BEDROCK));
    }
    for face in [Face::North, Face::South, Face::East, Face::West] {
        let [nx, _, nz] = face.normal();
        out.push((
            origin + IVec3::new(nx, 2, nz),
            BlockState::of(blocks::WALL_TORCH).with_face(Prop::Facing, face),
        ));
    }
    out
}

/// The obsidian platform arrivals stand on and the air cleared over it
/// (modern `ServerLevel.makeObsidianPlatform`).
pub fn platform() -> Vec<(IVec3, BlockState)> {
    let mut out = Vec::new();
    for dz in -2..=2 {
        for dx in -2..=2 {
            for dy in -1..=2 {
                let block = if dy < 0 { OBSIDIAN } else { BlockState::AIR };
                out.push((PLATFORM + IVec3::new(dx, dy, dz), block));
            }
        }
    }
    out
}

/// The towers of bedrock round an End gateway at `at`: the gateway between
/// two bedrock blocks above and two below.
pub fn gateway(at: IVec3) -> Vec<(IVec3, BlockState)> {
    let mut out = vec![(at, BlockState::of(blocks::END_GATEWAY))];
    for dy in [-2, -1, 1, 2] {
        out.push((at + IVec3::Y * dy, BEDROCK));
    }
    out
}

/// The gateway standing on an outer island at `at` (the block the player
/// walks into, on the island's top): a bedrock block under it and two over.
pub fn exit_gateway(at: IVec3) -> Vec<(IVec3, BlockState)> {
    let mut out = vec![(at, BlockState::of(blocks::END_GATEWAY))];
    out.push((at - IVec3::Y, BEDROCK));
    out.push((at + IVec3::Y, BEDROCK));
    out.push((at + IVec3::Y * 2, BEDROCK));
    out
}

/// Where the `index`th gateway (the order the seed shuffles) stands.
pub fn gateway_position(seed: u64, index: usize) -> IVec3 {
    let mut order: Vec<usize> = (0..GATEWAYS).collect();
    let mut rng = Rng::new(mix(seed ^ 0x0000_4741_5445_5753));
    for i in (1..order.len()).rev() {
        order.swap(i, rng.below(i as u32 + 1) as usize);
    }
    let slot = order[index % GATEWAYS];
    let angle = slot as f64 * std::f64::consts::TAU / GATEWAYS as f64;
    IVec3::new(
        (GATEWAY_RING * angle.cos()).floor() as i32,
        GATEWAY_Y,
        (GATEWAY_RING * angle.sin()).floor() as i32,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chunk_at(end: &End, x: i32, z: i32) -> Chunk {
        end.generate(ChunkPos::new(x, z))
    }

    fn block(end: &End, at: IVec3) -> BlockState {
        let chunk = chunk_at(end, at.x.div_euclid(16), at.z.div_euclid(16));
        chunk.get(
            at.x.rem_euclid(16) as usize,
            at.y,
            at.z.rem_euclid(16) as usize,
        )
    }

    #[test]
    fn the_island_is_flat_on_top_round_underneath_and_void_beyond() {
        let end = End::new(1);
        assert_eq!(end.top(0, 0), Some(SURFACE_Y));
        assert!((SURFACE_Y - 2..=SURFACE_Y + 2).contains(&end.top(20, 10).unwrap()));
        assert!(end.solid(0, SURFACE_Y, 0));
        assert!(!end.solid(0, SURFACE_Y + 1, 0));
        // Under the middle it goes a good way down; under the rim barely.
        assert!(end.solid(0, SURFACE_Y - 30, 0));
        assert!(!end.solid(0, 5, 0));
        assert!(end.top(150, 0).is_none() && end.top(0, -140).is_none());
        // The edge is within 60-90 blocks whichever way you look.
        for step in 0..16 {
            let a = f64::from(step) * std::f64::consts::TAU / 16.0;
            let edge = (0..120)
                .find(|d| {
                    end.top(
                        (f64::from(*d) * a.cos()) as i32,
                        (f64::from(*d) * a.sin()) as i32,
                    )
                    .is_none()
                })
                .unwrap();
            assert!((55..=95).contains(&edge), "{edge} at {a}");
        }
    }

    #[test]
    fn chunks_are_end_stone_with_the_end_biome_and_the_same_every_time() {
        let end = End::new(7);
        let a = chunk_at(&end, 2, 1);
        let b = chunk_at(&end, 2, 1);
        assert_eq!(a.get(3, SURFACE_Y, 3), END_STONE);
        assert_eq!(a.biome(0, 0), Biome::TheEnd);
        for y in crate::config::MIN_Y..crate::config::MAX_Y {
            assert_eq!(a.get(5, y, 9), b.get(5, y, 9));
        }
        // Far chunks are empty.
        let far = chunk_at(&end, 20, 20);
        assert!((crate::config::MIN_Y..crate::config::MAX_Y).all(|y| far.get(4, y, 4).is_air()));
    }

    #[test]
    fn ten_pillars_ring_the_island_with_modern_sizes_and_two_cages() {
        let spikes = spikes(99);
        assert_eq!(spikes.len(), 10);
        let mut heights: Vec<i32> = spikes.iter().map(|s| s.height).collect();
        heights.sort();
        assert_eq!(heights, (0..10).map(|i| 76 + i * 3).collect::<Vec<_>>());
        for s in &spikes {
            assert_eq!(s.radius, 2 + (s.height - 76) / 9);
            let d = f64::from(s.x).hypot(f64::from(s.z));
            assert!((40.0..=43.0).contains(&d), "{d}");
        }
        assert_eq!(spikes.iter().filter(|s| s.guarded).count(), 2);
        // The shuffle depends on the seed.
        assert_ne!(spikes, super::spikes(100));
        assert_eq!(spikes, super::spikes(99));
    }

    #[test]
    fn a_pillar_is_obsidian_to_a_bedrock_top_with_fire_and_a_cage_when_guarded() {
        let end = End::new(5);
        for spike in spikes(5) {
            let top = IVec3::new(spike.x, spike.height, spike.z);
            assert_eq!(block(&end, top), BEDROCK);
            assert_eq!(block(&end, top + IVec3::Y), BlockState::of(blocks::FIRE));
            assert_eq!(block(&end, top - IVec3::Y), OBSIDIAN);
            assert_eq!(block(&end, top - IVec3::Y * 40), OBSIDIAN);
            assert!(
                block(&end, top + IVec3::Y * 2).is_air(),
                "room for the crystal"
            );
            let bars = block(&end, top + IVec3::new(2, 1, 0));
            assert_eq!(bars.is(blocks::IRON_BARS), spike.guarded, "{spike:?}");
            // Wider than its radius is open.
            assert!(block(&end, top + IVec3::new(spike.radius + 3, 0, 0)).is_air());
        }
    }

    #[test]
    fn pillars_come_out_whole_across_chunk_borders() {
        let end = End::new(11);
        let big = spikes(11).into_iter().max_by_key(|s| s.radius).unwrap();
        let expected: std::collections::HashSet<IVec3> = big.blocks().iter().map(|b| b.0).collect();
        let mut seen = 0;
        for cx in (big.x - 8).div_euclid(16)..=(big.x + 8).div_euclid(16) {
            for cz in (big.z - 8).div_euclid(16)..=(big.z + 8).div_euclid(16) {
                let chunk = chunk_at(&end, cx, cz);
                for at in &expected {
                    if at.x.div_euclid(16) == cx
                        && at.z.div_euclid(16) == cz
                        && chunk.get(
                            at.x.rem_euclid(16) as usize,
                            at.y,
                            at.z.rem_euclid(16) as usize,
                        ) == OBSIDIAN
                    {
                        seen += 1;
                    }
                }
            }
        }
        let obsidian = big.blocks().iter().filter(|b| b.1 == OBSIDIAN).count();
        assert_eq!(seen, obsidian);
    }

    #[test]
    fn the_podium_is_a_hole_in_a_bedrock_bowl_until_active() {
        let end = End::new(3);
        let origin = PODIUM;
        // Generated inactive: bedrock pillar, torches, a hole beside it.
        assert_eq!(block(&end, origin), BEDROCK);
        assert_eq!(block(&end, origin + IVec3::Y * 3), BEDROCK);
        assert_eq!(block(&end, origin + IVec3::new(1, 0, 0)), BlockState::AIR);
        assert_eq!(block(&end, origin + IVec3::new(0, -1, 1)), BEDROCK);
        assert_eq!(block(&end, origin + IVec3::new(3, 0, 0)), BEDROCK);
        assert!(block(&end, origin + IVec3::new(1, 2, 0)).is(blocks::WALL_TORCH));
        assert!(block(&end, origin + IVec3::new(4, 0, 0)).is(blocks::END_STONE));
        // Active: the hole's floor is the portal.
        let active = podium(origin, true);
        let portal = active
            .iter()
            .filter(|(_, b)| b.is(blocks::END_PORTAL))
            .count();
        assert!((12..=21).contains(&portal), "{portal}");
        assert!(
            active
                .iter()
                .all(|(at, b)| !b.is(blocks::END_PORTAL) || at.y == origin.y)
        );
    }

    #[test]
    fn the_arrival_platform_is_obsidian_with_room_over_it() {
        let platform = platform();
        assert_eq!(platform.len(), 100);
        let obsidian = platform.iter().filter(|(_, b)| *b == OBSIDIAN).count();
        assert_eq!(obsidian, 25);
        assert!(
            platform
                .iter()
                .all(|(at, b)| (b.is_air()) == (at.y >= PLATFORM.y))
        );
    }

    #[test]
    fn outer_islands_start_beyond_600_blocks_and_every_direction_has_some_to_land_on() {
        let end = End::new(5);
        // None between the main island and the outer ring.
        for d in (150..560).step_by(10) {
            for a in 0..12 {
                let t = f64::from(a) * std::f64::consts::TAU / 12.0;
                assert!(
                    end.top(
                        (f64::from(d) * t.cos()) as i32,
                        (f64::from(d) * t.sin()) as i32
                    )
                    .is_none()
                );
            }
        }
        // Around a gateway's 1024 there's always one within a few hundred blocks.
        for slot in 0..GATEWAYS {
            let t = slot as f64 * std::f64::consts::TAU / GATEWAYS as f64;
            let (x, z) = ((1024.0 * t.cos()) as i32, (1024.0 * t.sin()) as i32);
            let (at, radius) = end.island_near(x, z).expect("an island");
            assert!(radius >= 16);
            let near = f64::from(at.x - x).hypot(f64::from(at.z - z));
            assert!(near < 450.0, "{near} from {x},{z}");
            assert!(end.solid(at.x, at.y, at.z) && end.top(at.x, at.z).unwrap() >= at.y - 1);
            assert!(!end.solid(at.x, at.y + 1, at.z));
        }
    }

    #[test]
    fn outer_islands_are_the_same_whichever_chunk_draws_them_and_grow_chorus_plants() {
        let end = End::new(5);
        let (at, radius) = end.island_near(1024, 0).unwrap();
        assert_eq!(end.island_near(1024, 0), Some((at, radius)));
        let pos = ChunkPos::of_block(at.x, at.z);
        let chunk = end.generate(pos);
        let again = end.generate(pos);
        let local = (
            (at.x.rem_euclid(16)) as usize,
            (at.z.rem_euclid(16)) as usize,
        );
        assert_eq!(chunk.get(local.0, at.y, local.1), END_STONE);
        for y in crate::config::MIN_Y..crate::config::MAX_Y {
            assert_eq!(
                chunk.get(local.0, y, local.1),
                again.get(local.0, y, local.1)
            );
        }
        // Somewhere on the island's chunks a chorus plant stands on end stone.
        let mut plants = 0;
        for dz in -2..=2 {
            for dx in -2..=2 {
                let chunk = end.generate(pos.offset(dx, dz));
                for z in 0..16 {
                    for x in 0..16 {
                        for y in crate::config::MIN_Y + 1..crate::config::MAX_Y {
                            if chunk.get(x, y, z).is(blocks::CHORUS_PLANT)
                                && chunk.get(x, y - 1, z).is(blocks::END_STONE)
                            {
                                plants += 1;
                            }
                        }
                    }
                }
            }
        }
        assert!(plants >= 1, "{plants}");
    }

    #[test]
    fn gateways_are_a_ring_of_twenty_at_96_blocks_in_the_seeds_order() {
        let positions: Vec<IVec3> = (0..GATEWAYS).map(|i| gateway_position(8, i)).collect();
        let distinct: std::collections::HashSet<_> = positions.iter().collect();
        assert_eq!(distinct.len(), GATEWAYS);
        for p in &positions {
            let d = f64::from(p.x).hypot(f64::from(p.z));
            assert!((94.0..=97.5).contains(&d), "{d}");
            assert_eq!(p.y, 75);
        }
        assert_ne!(positions[0], gateway_position(9, 0));
        let tower = gateway(positions[0]);
        assert!(tower[0].1.is(blocks::END_GATEWAY));
        assert_eq!(tower.len(), 5);
    }
}
