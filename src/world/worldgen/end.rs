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

    /// The top solid block of the column at (x, z), if it is over the
    /// island.
    pub fn top(&self, x: i32, z: i32) -> Option<i32> {
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
        Some(SURFACE_Y + (bumps - sink * sink * 6.0).round() as i32)
    }

    /// The lowest solid block of the column at (x, z) under a top of `top`.
    fn bottom(&self, x: i32, z: i32, top: i32) -> i32 {
        let (fx, fz) = (f64::from(x), f64::from(z));
        let radius = self.radius(fx, fz);
        let across = (fx.hypot(fz) / radius).min(1.0);
        let belly = DEPTH * (1.0 - across * across).sqrt();
        let jag = self
            .rough
            .get([fx / 9.0 + self.shift[0], fz / 9.0 + self.shift[2]])
            * 2.0;
        ((f64::from(SURFACE_Y) - belly + jag).round() as i32).min(top)
    }

    /// Whether (x, y, z) is end stone of the island.
    pub fn solid(&self, x: i32, y: i32, z: i32) -> bool {
        self.top(x, z)
            .is_some_and(|top| y <= top && y >= self.bottom(x, z, top))
    }

    pub fn generate(&self, pos: ChunkPos) -> Chunk {
        let mut chunk = Chunk::new(pos);
        let (x0, z0) = pos.min_block();
        for lz in 0..CHUNK_SIZE {
            for lx in 0..CHUNK_SIZE {
                chunk.set_biome(lx, lz, Biome::TheEnd);
                let (x, z) = (x0 + lx as i32, z0 + lz as i32);
                if x.abs() > 100 || z.abs() > 100 {
                    continue;
                }
                let Some(top) = self.top(x, z) else {
                    continue;
                };
                for y in self.bottom(x, z, top).max(1)..=top {
                    chunk.set(lx, y as usize, lz, END_STONE);
                }
            }
        }
        let clip = |at: IVec3, block: BlockState| {
            let (min_x, min_z) = pos.min_block();
            let (lx, lz) = (at.x - min_x, at.z - min_z);
            if (0..CHUNK_SIZE as i32).contains(&lx)
                && (0..CHUNK_SIZE as i32).contains(&lz)
                && (0..crate::config::WORLD_HEIGHT as i32).contains(&at.y)
            {
                Some((lx as usize, at.y as usize, lz as usize, block))
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
        for spike in spikes(self.seed) {
            if spike.reaches(pos) {
                put(&mut chunk, spike.blocks());
            }
        }
        if pos.x.abs() <= 1 && pos.z.abs() <= 1 {
            put(&mut chunk, podium(PODIUM, false));
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
            at.y as usize,
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
        assert_eq!(a.get(3, SURFACE_Y as usize, 3), END_STONE);
        assert_eq!(a.biome(0, 0), Biome::TheEnd);
        for y in 0..crate::config::WORLD_HEIGHT {
            assert_eq!(a.get(5, y, 9), b.get(5, y, 9));
        }
        // Far chunks are empty.
        let far = chunk_at(&end, 20, 20);
        assert!((0..crate::config::WORLD_HEIGHT).all(|y| far.get(4, y, 4).is_air()));
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
                            at.y as usize,
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
