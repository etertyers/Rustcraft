//! Seeded terrain: climate noise picks the biome, and height, surface layers,
//! water and bedrock follow from it.
//!
//! Every function here is pure (it depends only on the seed and coordinates),
//! so any chunk can be generated on its own, in any order, on any thread, and
//! features near a chunk border can ask about columns in the next chunk.

use std::collections::HashMap;

use noise::{Fbm, MultiFractal, NoiseFn, Perlin};

use super::rng::{Rng, mix};
use crate::{
    config::{CHUNK_SIZE, SEA_LEVEL, WORLD_HEIGHT},
    world::{
        biome::Biome,
        block::BlockState,
        chunk::{Chunk, ChunkPos},
    },
};

pub const STONE: BlockState = BlockState::new(1, 0);
pub const GRASS: BlockState = BlockState::new(2, 0);
pub const DIRT: BlockState = BlockState::new(3, 0);
pub const BEDROCK: BlockState = BlockState::new(7, 0);
pub const WATER: BlockState = BlockState::new(9, 0);
pub const SAND: BlockState = BlockState::new(12, 0);
pub const GRAVEL: BlockState = BlockState::new(13, 0);
pub const SANDSTONE: BlockState = BlockState::new(24, 0);
pub const LAVA: BlockState = BlockState::new(11, 0);

/// Caves are where two 3D noise fields are both near zero; this is the
/// squared distance from zero that counts (wider = bigger tunnels).
const CAVE_WIDTH: f64 = 0.0045;
/// Air carved at or below this height becomes lava, as in 1.0.0.
const LAVA_LEVEL: i32 = 10;
/// Caves stay this far below the floor of any water.
const CAVE_WATER_MARGIN: i32 = 3;

/// Continent noise below this is ocean.
const OCEAN_THRESHOLD: f64 = -0.16;
/// Blocks of depth per unit of continent noise below the threshold.
const OCEAN_SLOPE: f64 = 90.0;
const MAX_OCEAN_DEPTH: f64 = 24.0;
/// Blocks of rise per unit of continent noise above the threshold (inland is higher).
const INLAND_SLOPE: f64 = 14.0;

/// Climate thresholds: hot and dry is desert, humid is forest, the rest plains.
const DESERT_TEMPERATURE: f64 = 0.0;
const DESERT_HUMIDITY: f64 = 0.0;
const FOREST_HUMIDITY: f64 = 0.1;
/// Half-width of the climate band over which terrain shape blends between biomes.
const BLEND: f64 = 0.08;

/// Per-biome terrain shape: height above sea level and hill amplitude in blocks.
struct Shape {
    base: f64,
    hills: f64,
}

const PLAINS_SHAPE: Shape = Shape {
    base: 3.0,
    hills: 4.0,
};
const FOREST_SHAPE: Shape = Shape {
    base: 4.0,
    hills: 10.0,
};
const DESERT_SHAPE: Shape = Shape {
    base: 3.0,
    hills: 5.0,
};

/// Everything about a column that doesn't depend on caves or features.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Column {
    pub biome: Biome,
    /// Y of the topmost solid block.
    pub height: i32,
    /// The block on top (grass, sand, dirt or gravel).
    pub top: BlockState,
}

pub struct Terrain {
    seed: u64,
    continent: Fbm<Perlin>,
    temperature: Fbm<Perlin>,
    humidity: Fbm<Perlin>,
    hills: Fbm<Perlin>,
    /// Varies surface layer depth and beach/gravel patches.
    surface: Perlin,
    cave_a: Perlin,
    cave_b: Perlin,
}

/// A 32-bit noise seed derived from the world seed, different for each `index`.
fn noise_seed(seed: u64, index: u64) -> u32 {
    mix(seed ^ mix(index)) as u32
}

fn smoothstep(edge0: f64, edge1: f64, x: f64) -> f64 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

impl Terrain {
    pub fn new(seed: u64) -> Self {
        let fbm = |index, octaves, frequency| {
            Fbm::<Perlin>::new(noise_seed(seed, index))
                .set_octaves(octaves)
                .set_frequency(frequency)
        };
        Self {
            seed,
            continent: fbm(1, 4, 1.0 / 500.0),
            temperature: fbm(2, 3, 1.0 / 300.0),
            humidity: fbm(3, 3, 1.0 / 260.0),
            hills: fbm(4, 4, 1.0 / 90.0),
            surface: Perlin::new(noise_seed(seed, 5)),
            cave_a: Perlin::new(noise_seed(seed, 6)),
            cave_b: Perlin::new(noise_seed(seed, 7)),
        }
    }

    pub fn seed(&self) -> u64 {
        self.seed
    }

    fn climate(&self, x: i32, z: i32) -> (f64, f64, f64) {
        let p = [x as f64, z as f64];
        (
            self.continent.get(p),
            self.temperature.get(p),
            self.humidity.get(p),
        )
    }

    fn classify(continent: f64, temperature: f64, humidity: f64) -> Biome {
        if continent < OCEAN_THRESHOLD {
            Biome::Ocean
        } else if temperature > DESERT_TEMPERATURE && humidity < DESERT_HUMIDITY {
            Biome::Desert
        } else if humidity > FOREST_HUMIDITY {
            Biome::Forest
        } else {
            Biome::Plains
        }
    }

    /// Height of the topmost solid block. Biome shapes are mixed by smooth
    /// weights around the climate thresholds, so borders never step.
    fn height(&self, x: i32, z: i32, continent: f64, temperature: f64, humidity: f64) -> i32 {
        let forest = smoothstep(FOREST_HUMIDITY - BLEND, FOREST_HUMIDITY + BLEND, humidity);
        let desert = smoothstep(
            DESERT_TEMPERATURE - BLEND,
            DESERT_TEMPERATURE + BLEND,
            temperature,
        ) * smoothstep(DESERT_HUMIDITY + BLEND, DESERT_HUMIDITY - BLEND, humidity);
        let mix_shape = |f: fn(&Shape) -> f64| {
            let land = f(&PLAINS_SHAPE) + (f(&FOREST_SHAPE) - f(&PLAINS_SHAPE)) * forest;
            land + (f(&DESERT_SHAPE) - land) * desert
        };
        // 0 deep in the ocean, 1 on land; flattens hills under water.
        let land = smoothstep(OCEAN_THRESHOLD - 0.06, OCEAN_THRESHOLD + 0.02, continent);
        let shelf = if continent < OCEAN_THRESHOLD {
            ((continent - OCEAN_THRESHOLD) * OCEAN_SLOPE).max(-MAX_OCEAN_DEPTH)
        } else {
            (continent - OCEAN_THRESHOLD) * INLAND_SLOPE
        };
        let hills = self.hills.get([x as f64, z as f64]) * mix_shape(|s| s.hills);
        let height =
            SEA_LEVEL as f64 + shelf + mix_shape(|s| s.base) * land + hills * (0.25 + 0.75 * land);
        height.round().clamp(8.0, WORLD_HEIGHT as f64 - 24.0) as i32
    }

    /// Noise in -1..1 for surface details at a column.
    fn surface_noise(&self, x: i32, z: i32, scale: f64, offset: f64) -> f64 {
        self.surface
            .get([x as f64 / scale + offset, z as f64 / scale - offset])
    }

    pub fn column(&self, x: i32, z: i32) -> Column {
        let (c, t, h) = self.climate(x, z);
        let biome = Self::classify(c, t, h);
        let height = self.height(x, z, c, t, h);
        let patch = self.surface_noise(x, z, 12.0, 0.5);
        let top = if biome == Biome::Desert {
            SAND
        } else if height < SEA_LEVEL - 1 {
            // Underwater floors are sand, gravel or dirt in patches.
            if patch > 0.25 {
                GRAVEL
            } else if patch > -0.35 || biome == Biome::Ocean {
                SAND
            } else {
                DIRT
            }
        } else if height <= SEA_LEVEL + 1 && patch > -0.2 {
            // Beaches.
            SAND
        } else {
            GRASS
        };
        Column { biome, height, top }
    }

    /// Whether a cave runs through (x, y, z), ignoring what block is there.
    pub fn is_cave(&self, x: i32, y: i32, z: i32) -> bool {
        if y < 1 {
            return false;
        }
        // Tunnels are stretched sideways: caves wind more than they climb.
        let p = [x as f64 / 48.0, y as f64 / 24.0, z as f64 / 48.0];
        let a = self.cave_a.get(p);
        let b = self.cave_b.get(p);
        a * a + b * b < CAVE_WIDTH
    }

    /// Carves caves into a filled chunk, leaving bedrock, water and the
    /// ground just under water alone.
    fn carve_caves(&self, chunk: &mut Chunk) {
        let (min_x, min_z) = chunk.pos.min_block();
        for z in 0..CHUNK_SIZE {
            for x in 0..CHUNK_SIZE {
                let (wx, wz) = (min_x + x as i32, min_z + z as i32);
                let column = self.column(wx, wz);
                let top = if column.height < SEA_LEVEL {
                    column.height - CAVE_WATER_MARGIN
                } else {
                    column.height
                };
                for y in 1..=top {
                    let block = chunk.get(x, y as usize, z);
                    if block == BEDROCK || !self.is_cave(wx, y, wz) {
                        continue;
                    }
                    let carved = if y <= LAVA_LEVEL {
                        LAVA
                    } else {
                        BlockState::AIR
                    };
                    chunk.set(x, y as usize, z, carved);
                }
            }
        }
    }

    /// A land column near the origin to spawn on: (x, y of the block above the
    /// surface, z). Searches outward in rings, like 1.0.0's spawn search.
    pub fn spawn_point(&self) -> (i32, i32, i32) {
        const STEP: i32 = 8;
        let mut chunks: HashMap<ChunkPos, Chunk> = HashMap::new();
        for ring in 0..256 {
            for i in -ring..=ring {
                for (x, z) in [(i, -ring), (i, ring), (-ring, i), (ring, i)] {
                    let (x, z) = (x * STEP, z * STEP);
                    let column = self.column(x, z);
                    if column.biome == Biome::Ocean || column.height < SEA_LEVEL {
                        continue;
                    }
                    // Check the generated column too: a cave can carve the
                    // surface away, and a tree or plant can stand on it.
                    let chunk = chunks
                        .entry(ChunkPos::of_block(x, z))
                        .or_insert_with_key(|&pos| self.generate(pos));
                    let (lx, lz) = (x.rem_euclid(16) as usize, z.rem_euclid(16) as usize);
                    let y = column.height as usize;
                    if chunk.get(lx, y, lz) == column.top
                        && chunk.get(lx, y + 1, lz).is_air()
                        && chunk.get(lx, y + 2, lz).is_air()
                    {
                        return (x, column.height + 1, z);
                    }
                }
            }
        }
        (0, WORLD_HEIGHT as i32, 0)
    }

    /// Fills a chunk with stone, surface layers, water and bedrock (no caves or features).
    pub fn fill(&self, chunk: &mut Chunk) {
        let (min_x, min_z) = chunk.pos.min_block();
        let mut bedrock_rng = Rng::for_chunk(self.seed, chunk.pos.x, chunk.pos.z, 0xBED);
        for z in 0..CHUNK_SIZE {
            for x in 0..CHUNK_SIZE {
                let (wx, wz) = (min_x + x as i32, min_z + z as i32);
                let column = self.column(wx, wz);
                chunk.set_biome(x, z, column.biome);
                let depth = 3 + (self.surface_noise(wx, wz, 8.0, 7.3) * 1.5 + 1.5) as i32;
                let (filler, under) = match column.top {
                    SAND if column.biome == Biome::Desert => (SAND, Some(SANDSTONE)),
                    SAND | GRAVEL => (column.top, None),
                    _ => (DIRT, None),
                };
                for y in 1..=column.height {
                    let below_top = column.height - y;
                    let block = if below_top == 0 {
                        column.top
                    } else if below_top < depth {
                        filler
                    } else if below_top < depth + 3
                        && let Some(under) = under
                    {
                        under
                    } else {
                        STONE
                    };
                    chunk.set(x, y as usize, z, block);
                }
                for y in column.height + 1..=SEA_LEVEL {
                    chunk.set(x, y as usize, z, WATER);
                }
                // A bedrock floor, ragged for four blocks like 1.0.0.
                chunk.set(x, 0, z, BEDROCK);
                for y in 1..5 {
                    if (bedrock_rng.below(5) as usize) >= y {
                        chunk.set(x, y, z, BEDROCK);
                    }
                }
            }
        }
    }

    /// Generates a chunk: terrain, caves, ores, trees and plants.
    pub fn generate(&self, pos: ChunkPos) -> Chunk {
        let mut chunk = Chunk::new(pos);
        self.fill(&mut chunk);
        self.carve_caves(&mut chunk);
        super::ores::populate(self, &mut chunk);
        super::features::populate(self, &mut chunk);
        chunk
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn same_seed_same_chunk_different_seed_different_chunk() {
        let pos = ChunkPos::new(3, -7);
        let a = Terrain::new(1).generate(pos);
        let b = Terrain::new(1).generate(pos);
        let c = Terrain::new(2).generate(pos);
        let blocks = |chunk: &Chunk| -> Vec<BlockState> {
            (0..WORLD_HEIGHT)
                .flat_map(|y| (0..256).map(move |i| (i % 16, y, i / 16)))
                .map(|(x, y, z)| chunk.get(x, y, z))
                .collect()
        };
        assert_eq!(blocks(&a), blocks(&b));
        assert_ne!(blocks(&a), blocks(&c));
    }

    /// M3 "done when": flying in one direction for 2 minutes (about 1,300
    /// blocks at fly speed) passes all four biomes.
    #[test]
    fn a_two_minute_flight_crosses_all_four_biomes() {
        let mut hits = 0;
        let trials = 40;
        for seed in 0..trials {
            let terrain = Terrain::new(seed);
            let mut rng = Rng::new(seed);
            let (x0, z0) = (
                rng.below(20_000) as i32 - 10_000,
                rng.below(20_000) as i32 - 10_000,
            );
            let angle = rng.next_f64() * std::f64::consts::TAU;
            let biomes: HashSet<Biome> = (0..1300)
                .step_by(4)
                .map(|d| {
                    let x = x0 + (angle.cos() * d as f64) as i32;
                    let z = z0 + (angle.sin() * d as f64) as i32;
                    terrain.column(x, z).biome
                })
                .collect();
            if biomes.len() == 4 {
                hits += 1;
            }
        }
        // Not every random line can cross every biome (39/40 did when tuned).
        assert!(
            hits * 10 >= trials * 9,
            "only {hits}/{trials} flights saw all four biomes"
        );
    }

    #[test]
    fn biome_mix_is_reasonable() {
        let terrain = Terrain::new(12345);
        let mut counts = [0usize; 4];
        for z in (-4000..4000).step_by(64) {
            for x in (-4000..4000).step_by(64) {
                counts[terrain.column(x, z).biome as usize] += 1;
            }
        }
        let total: usize = counts.iter().sum();
        for (biome, count) in Biome::ALL.iter().zip(counts) {
            let share = count as f64 / total as f64;
            assert!((0.08..0.55).contains(&share), "{biome:?} covers {share:.2}");
        }
    }

    #[test]
    fn borders_are_continuous() {
        // Neighboring columns never differ by more than a few blocks, even
        // across biome borders.
        let terrain = Terrain::new(99);
        let mut steepest = 0;
        for z in (-2000..2000).step_by(7) {
            let mut previous = terrain.column(-2000, z).height;
            for x in -1999..2000 {
                let height = terrain.column(x, z).height;
                steepest = steepest.max((height - previous).abs());
                previous = height;
            }
        }
        assert!(
            steepest <= 4,
            "a {steepest}-block cliff between neighboring columns"
        );
    }

    #[test]
    fn oceans_are_water_down_to_the_floor() {
        let terrain = Terrain::new(5);
        let (x, z) = (-6000..6000)
            .step_by(16)
            .flat_map(|x| (-6000..6000).step_by(16).map(move |z| (x, z)))
            .find(|&(x, z)| terrain.column(x, z).height < SEA_LEVEL - 5)
            .expect("no ocean found");
        let chunk = terrain.generate(ChunkPos::of_block(x, z));
        let (lx, lz) = (x.rem_euclid(16) as usize, z.rem_euclid(16) as usize);
        let column = terrain.column(x, z);
        assert_eq!(chunk.get(lx, SEA_LEVEL as usize, lz), WATER);
        assert_eq!(chunk.get(lx, column.height as usize + 1, lz), WATER);
        assert_eq!(chunk.get(lx, column.height as usize, lz), column.top);
        assert!(chunk.get(lx, SEA_LEVEL as usize + 1, lz).is_air());
    }

    #[test]
    fn spawn_is_on_open_dry_land() {
        // Seed 42 used to spawn where a cave had carved away the surface.
        for seed in (0..10).chain([42]) {
            let terrain = Terrain::new(seed);
            let (x, y, z) = terrain.spawn_point();
            let column = terrain.column(x, z);
            assert_ne!(column.biome, Biome::Ocean);
            assert_eq!(y, column.height + 1);
            assert!(column.height >= SEA_LEVEL);
            let chunk = terrain.generate(ChunkPos::of_block(x, z));
            let (lx, lz) = (x.rem_euclid(16) as usize, z.rem_euclid(16) as usize);
            let y = y as usize;
            assert_eq!(chunk.get(lx, y - 1, lz), column.top, "seed {seed}");
            assert!(chunk.get(lx, y, lz).is_air(), "seed {seed}");
            assert!(chunk.get(lx, y + 1, lz).is_air(), "seed {seed}");
        }
    }

    #[test]
    fn caves_carve_land_but_keep_bedrock_and_ocean_floors() {
        let terrain = Terrain::new(8);
        let (mut air_below_surface, mut lava) = (0, 0);
        for cz in -4..4 {
            for cx in -4..4 {
                let chunk = terrain.generate(ChunkPos::new(cx, cz));
                let (min_x, min_z) = chunk.pos.min_block();
                for z in 0..16 {
                    for x in 0..16 {
                        assert_eq!(chunk.get(x, 0, z), BEDROCK);
                        let column = terrain.column(min_x + x as i32, min_z + z as i32);
                        if column.height < SEA_LEVEL {
                            let floor = chunk.get(x, column.height as usize, z);
                            assert!(!floor.is_air(), "hole under water at {x},{z}");
                        }
                        for y in 1..column.height as usize {
                            match chunk.get(x, y, z) {
                                b if b.is_air() => air_below_surface += 1,
                                LAVA => {
                                    assert!(y as i32 <= LAVA_LEVEL);
                                    lava += 1;
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }
        }
        assert!(
            air_below_surface > 1000,
            "only {air_below_surface} cave blocks in 64 chunks"
        );
        assert!(lava > 0, "no lava in deep caves");
    }

    #[test]
    fn surface_layers_and_bedrock() {
        let terrain = Terrain::new(77);
        let chunk = terrain.generate(ChunkPos::new(0, 0));
        for z in 0..16 {
            for x in 0..16 {
                assert_eq!(chunk.get(x, 0, z), BEDROCK);
                let column = terrain.column(x as i32, z as i32);
                assert_eq!(chunk.biome(x, z), column.biome);
                if terrain.is_cave(x as i32, column.height, z as i32)
                    || terrain.is_cave(x as i32, column.height - 1, z as i32)
                {
                    continue;
                }
                let top = chunk.get(x, column.height as usize, z);
                // Trees turn the grass under their trunk to dirt.
                assert!(top == column.top || top == DIRT, "{top:?} at {x},{z}");
                if column.top == GRASS {
                    assert_eq!(chunk.get(x, column.height as usize - 1, z), DIRT);
                }
            }
        }
    }

    #[test]
    fn deserts_have_sand_over_sandstone() {
        let terrain = Terrain::new(3);
        let (x, z) = (-8000..8000)
            .step_by(32)
            .flat_map(|x| (-8000..8000).step_by(32).map(move |z| (x, z)))
            .find(|&(x, z)| {
                let c = terrain.column(x, z);
                c.biome == Biome::Desert
                    && c.height > SEA_LEVEL + 2
                    && (c.height - 10..=c.height).all(|y| !terrain.is_cave(x, y, z))
            })
            .expect("no desert found");
        let chunk = terrain.generate(ChunkPos::of_block(x, z));
        let (lx, lz) = (x.rem_euclid(16) as usize, z.rem_euclid(16) as usize);
        let blocks: Vec<BlockState> = (0..WORLD_HEIGHT).map(|y| chunk.get(lx, y, lz)).collect();
        let top = terrain.column(x, z).height as usize;
        assert_eq!(blocks[top], SAND);
        assert!(blocks[..top].contains(&SANDSTONE));
    }
}
