//! Seeded terrain: modern climate noise (see `climate`) shapes the land and
//! picks the biome, and height, surface layers, water, deepslate, caves and
//! bedrock follow from it.
//!
//! Every function here is pure (it depends only on the seed and coordinates),
//! so any chunk can be generated on its own, in any order, on any thread, and
//! features near a chunk border can ask about columns in the next chunk.

use std::collections::HashMap;

use noise::{Fbm, MultiFractal, NoiseFn, Perlin};

use super::{
    biome_source,
    caves::{CARVER_REACH, Caves, worms},
    climate::{Climate, ClimateNoise, Lay, noise_offset, noise_seed},
    rng::{Rng, mix},
    terrain_shape::Shaper,
};
use crate::{
    config::{CHUNK_SIZE, MAX_Y, MIN_Y, SEA_LEVEL},
    world::{
        biome::Biome,
        block::BlockState,
        blocks,
        chunk::{Chunk, ChunkPos},
    },
};

pub const STONE: BlockState = BlockState::of(blocks::STONE);
pub const GRASS: BlockState = BlockState::of(blocks::GRASS_BLOCK);
pub const DIRT: BlockState = BlockState::of(blocks::DIRT);
pub const BEDROCK: BlockState = BlockState::of(blocks::BEDROCK);
pub const WATER: BlockState = BlockState::of(blocks::WATER);
pub const SAND: BlockState = BlockState::of(blocks::SAND);
pub const RED_SAND: BlockState = BlockState::of(blocks::RED_SAND);
pub const GRAVEL: BlockState = BlockState::of(blocks::GRAVEL);
pub const SANDSTONE: BlockState = BlockState::of(blocks::SANDSTONE);
pub const LAVA: BlockState = BlockState::of(blocks::LAVA);
pub const DEEPSLATE: BlockState = BlockState::of(blocks::DEEPSLATE);
pub const PODZOL: BlockState = BlockState::of(blocks::PODZOL);
pub const MYCELIUM: BlockState = BlockState::of(blocks::MYCELIUM);
pub const MUD: BlockState = BlockState::of(blocks::MUD);
pub const CLAY: BlockState = BlockState::of(blocks::CLAY);
pub const SNOW_BLOCK: BlockState = BlockState::of(blocks::SNOW_BLOCK);
pub const PACKED_ICE: BlockState = BlockState::of(blocks::PACKED_ICE);
pub const TERRACOTTA: BlockState = BlockState::of(blocks::TERRACOTTA);
pub const COARSE_DIRT: BlockState = BlockState::of(blocks::COARSE_DIRT);
pub const CALCITE: BlockState = BlockState::of(blocks::CALCITE);
pub const ICE: BlockState = BlockState::of(blocks::ICE);
/// A rise of this many blocks to a neighbor bares the stone.
const STEEP: i32 = 4;

/// Caves stay this far below the floor of any water.
const CAVE_WATER_MARGIN: i32 = 4;
/// Caves stop this far under the surface, except at cave entrances.
const CAVE_ROOF: i32 = 3;
/// Cave noise is sampled every `CAVE_CELL_XZ` blocks across and
/// `CAVE_CELL_Y` up, and blended in between.
const CAVE_CELL_XZ: i32 = 4;
const CAVE_CELL_Y: i32 = 4;
/// Deepslate replaces stone below Y 0, blending into it over the eight
/// blocks above (modern `deepslate` vertical gradient).
pub const DEEPSLATE_LEVEL: i32 = 0;
const DEEPSLATE_BLEND: i32 = 8;
/// Lowest height a cave biome reaches.
const CAVE_BIOME_FLOOR: i32 = MIN_Y + 4;
/// The ragged bedrock floor: always at the bottom, thinning out over the
/// four blocks above (modern −64 to −60).
const BEDROCK_RAGGED: i32 = 4;

/// Everything about a column that doesn't depend on caves or features.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Column {
    pub biome: Biome,
    /// Y of the topmost solid block.
    pub height: i32,
    /// The block on top (grass, sand, snow...).
    pub top: BlockState,
    /// Whether caves may break through the surface here.
    pub entrance: bool,
    /// The cave biome deep underneath, if any.
    pub cave: Option<Biome>,
    /// Steepest drop to a neighboring column, in blocks.
    pub slope: i32,
    /// The preliminary surface (the shape's offset) at the quart's corner.
    pub offset: f64,
}

impl Column {
    /// Whether plants and trees can grow on it: grass (or podzol) that no
    /// cave cuts away.
    pub fn grassy(&self) -> bool {
        matches!(self.top.id(), blocks::GRASS_BLOCK | blocks::PODZOL) && !self.entrance
    }
}

pub struct Terrain {
    seed: u64,
    /// Coordinate shifts for the detail noise (see `noise_offset`).
    shift: [f64; 3],
    climate: ClimateNoise,
    shaper: Shaper,
    /// The 3D noise that shapes the land around its offset.
    base: Fbm<Perlin>,
    /// Sharp ridges added on peaks.
    jagged: Perlin,
    /// Varies surface layer depth and beach/gravel patches.
    surface: Perlin,
    entrances: Perlin,
    caves: Caves,
    /// Badlands terracotta bands, by height.
    bands: [BlockState; BAND_COUNT],
}

pub fn smoothstep(edge0: f64, edge1: f64, x: f64) -> f64 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}

/// Density is sampled at the corners of 4 × 8 × 4 cells and blended in
/// between (modern `NoiseChunk`).
const CELL_XZ: i32 = 4;
const CELL_Y: i32 = 8;
/// Corners up a column, every 8 blocks from Y 0 to 256; the land's shape
/// lives up there, and everything below Y 0 is solid.
const LEVELS: usize = 256 / CELL_Y as usize + 1;
/// The most the 3D noise moves density; it fades out from `NOISE_FADE`
/// up to `NOISE_TOP`, so nothing floats up there.
const NOISE_AMPLITUDE: f64 = 0.6;
const NOISE_FADE: f64 = 180.0;
const NOISE_TOP: f64 = 200.0;
/// Highest the surface is pulled to, and nothing solid from `CAP` up.
const MAX_TARGET: f64 = 240.0;
const CAP: i32 = 248;

/// Densities up one corner column, every `CELL_Y` blocks.
type Levels = [f64; LEVELS];

/// Corner columns on the `CELL_XZ` grid, and their shapes.
struct Corners {
    gx0: i32,
    gz0: i32,
    row: usize,
    levels: Vec<Levels>,
    offsets: Vec<f64>,
    /// The climate at each corner (what its quart's biome comes from).
    climates: Vec<Climate>,
}

impl Corners {
    fn index(&self, gx: i32, gz: i32) -> usize {
        (gz - self.gz0) as usize * self.row + (gx - self.gx0) as usize
    }

    /// The column at (x, z): its density levels blended from the four
    /// corners round it.
    fn levels(&self, x: i32, z: i32) -> Levels {
        let (gx, gz) = (x.div_euclid(CELL_XZ), z.div_euclid(CELL_XZ));
        let (tx, tz) = (
            (x - gx * CELL_XZ) as f64 / CELL_XZ as f64,
            (z - gz * CELL_XZ) as f64 / CELL_XZ as f64,
        );
        let i = self.index(gx, gz);
        let (a, b, c, d) = (
            &self.levels[i],
            &self.levels[i + 1],
            &self.levels[i + self.row],
            &self.levels[i + self.row + 1],
        );
        std::array::from_fn(|k| lerp(lerp(a[k], b[k], tx), lerp(c[k], d[k], tx), tz))
    }

    /// The offset of the corner at the quart holding (x, z).
    fn offset(&self, x: i32, z: i32) -> f64 {
        self.offsets[self.index(x.div_euclid(CELL_XZ), z.div_euclid(CELL_XZ))]
    }
}

/// The density at height `y` from a column's levels.
fn density(levels: &Levels, y: i32) -> f64 {
    let k = (y / CELL_Y) as usize;
    if k + 1 >= LEVELS {
        return levels[LEVELS - 1];
    }
    lerp(
        levels[k],
        levels[k + 1],
        (y % CELL_Y) as f64 / CELL_Y as f64,
    )
}

/// The topmost solid height in a column (0 if none).
fn top_of(levels: &Levels) -> i32 {
    for k in (0..LEVELS - 1).rev() {
        if levels[k].max(levels[k + 1]) <= 0.0 {
            continue;
        }
        for y in (k as i32 * CELL_Y..(k as i32 + 1) * CELL_Y).rev() {
            if density(levels, y) > 0.0 {
                return y;
            }
        }
    }
    0
}

const BAND_COUNT: usize = 64;

impl Terrain {
    pub fn new(seed: u64) -> Self {
        let fbm = |index, octaves, wavelength: f64| {
            Fbm::<Perlin>::new(noise_seed(seed, index))
                .set_octaves(octaves)
                .set_frequency(1.0 / wavelength)
        };
        Self {
            seed,
            shift: noise_offset(seed, 0),
            climate: ClimateNoise::new(seed),
            shaper: Shaper::new(),
            base: fbm(4, 3, 90.0),
            jagged: Perlin::new(noise_seed(seed, 14)),
            surface: Perlin::new(noise_seed(seed, 5)),
            entrances: Perlin::new(noise_seed(seed, 16)),
            caves: Caves::new(seed),
            bands: badlands_bands(seed),
        }
    }

    pub fn seed(&self) -> u64 {
        self.seed
    }

    /// The climate at a column.
    pub fn climate(&self, x: i32, z: i32) -> Climate {
        self.climate.at(x, z)
    }

    /// The density levels of the corner column at grid point (gx, gz)
    /// (block x = gx × 4), and its offset: `(offset − y) × factor`, pulled
    /// toward the offset plus jagged peaks, moved by the 3D noise where it
    /// could matter.
    fn corner(&self, gx: i32, gz: i32, c: &Climate) -> (Levels, f64) {
        let (x, z) = (gx * CELL_XZ, gz * CELL_XZ);
        let shape = self.shaper.shape(c);
        let ridge = 1.0
            - self
                .jagged
                .get([
                    x as f64 / 24.0 + self.shift[0],
                    z as f64 / 24.0 + self.shift[2],
                ])
                .abs();
        let target = (shape.offset + shape.jaggedness * ridge * ridge).min(MAX_TARGET);
        let levels = std::array::from_fn(|k| {
            let y = k as i32 * CELL_Y;
            if y >= CAP {
                return -1.0;
            }
            let pulled = (target - y as f64) * shape.factor;
            let amplitude = NOISE_AMPLITUDE * smoothstep(NOISE_TOP, NOISE_FADE, y as f64);
            // Far from the surface the noise can't change the sign.
            if pulled.abs() >= amplitude {
                return pulled;
            }
            let noise = self.base.get([
                x as f64 + self.shift[0],
                y as f64 * 1.5 + self.shift[1],
                z as f64 + self.shift[2],
            ]);
            pulled + amplitude * noise.clamp(-1.0, 1.0)
        });
        (levels, shape.offset)
    }

    /// The corners covering blocks `x0..=x1`, `z0..=z1`.
    fn corners(&self, x0: i32, z0: i32, x1: i32, z1: i32) -> Corners {
        let (gx0, gz0) = (x0.div_euclid(CELL_XZ), z0.div_euclid(CELL_XZ));
        let (gx1, gz1) = (x1.div_euclid(CELL_XZ) + 1, z1.div_euclid(CELL_XZ) + 1);
        let row = (gx1 - gx0 + 1) as usize;
        let mut levels = Vec::with_capacity(row * (gz1 - gz0 + 1) as usize);
        let mut offsets = Vec::with_capacity(levels.capacity());
        let mut climates = Vec::with_capacity(levels.capacity());
        for gz in gz0..=gz1 {
            for gx in gx0..=gx1 {
                let c = self.climate.sample(gx * CELL_XZ, gz * CELL_XZ);
                let (l, o) = self.corner(gx, gz, &c);
                levels.push(l);
                offsets.push(o);
                climates.push(c);
            }
        }
        Corners {
            gx0,
            gz0,
            row,
            levels,
            offsets,
            climates,
        }
    }

    /// How the land lies at one column.
    fn lay(&self, x: i32, z: i32) -> Lay {
        let corners = self.corners(x, z, x, z);
        Lay {
            height: top_of(&corners.levels(x, z)),
            offset: corners.offset(x, z),
        }
    }

    /// The surface height and biome at the corner column of grid point
    /// (gx, gz) (block x = gx × 4), for maps.
    pub fn corner_surface(&self, gx: i32, gz: i32) -> (i32, Biome) {
        let c = self.climate.sample(gx * CELL_XZ, gz * CELL_XZ);
        let (levels, _) = self.corner(gx, gz, &c);
        (top_of(&levels), biome_source::surface(&c))
    }

    /// The preliminary surface (the shape's offset) at the quart holding
    /// (x, z).
    pub fn offset(&self, x: i32, z: i32) -> f64 {
        let (gx, gz) = (x.div_euclid(CELL_XZ), z.div_euclid(CELL_XZ));
        let c = self.climate.sample(gx * CELL_XZ, gz * CELL_XZ);
        self.shaper.shape(&c).offset
    }

    /// The surface biome of the quart holding (x, z), straight from the
    /// climate.
    pub fn biome_at(&self, x: i32, z: i32) -> Biome {
        super::biome_source::surface(&self.climate.quart(x, z))
    }

    /// Height of the topmost solid block.
    pub fn height(&self, x: i32, z: i32) -> i32 {
        self.lay(x, z).height
    }

    /// Noise in -1..1 for surface details at a column.
    fn surface_noise(&self, x: i32, z: i32, scale: f64, offset: f64) -> f64 {
        self.surface.get([
            (x as f64 + self.shift[0]) / scale + offset,
            (z as f64 + self.shift[2]) / scale - offset,
        ])
    }

    /// The column at (x, z), working out its neighbors' heights for the slope.
    pub fn column(&self, x: i32, z: i32) -> Column {
        let around = [(1, 0), (-1, 0), (0, 1), (0, -1)].map(|(dx, dz)| self.height(x + dx, z + dz));
        let quart = self.climate.quart(x, z);
        self.column_with(x, z, quart, self.lay(x, z), around)
    }

    /// The column at (x, z) with its quart's climate `quart` (which picks
    /// the biome) and lay `lay`, given its four neighbors' heights.
    fn column_with(&self, x: i32, z: i32, quart: Climate, lay: Lay, around: [i32; 4]) -> Column {
        let biome = biome_source::surface(&quart);
        let slope = around
            .iter()
            .map(|&h| (lay.height - h).abs())
            .max()
            .unwrap_or(0);
        let top = self.top(x, z, biome, lay.height, slope);
        let entrance = self.entrances.get([
            (x as f64 + self.shift[0]) / 23.0,
            (z as f64 + self.shift[2]) / 23.0,
        ]) > 0.55;
        // The cave biome under the quart, at any depth in its range.
        let cave = biome_source::cave(&quart, biome_source::CAVE_DEPTH.0);
        Column {
            biome,
            height: lay.height,
            top,
            entrance,
            cave,
            slope,
            offset: lay.offset,
        }
    }

    /// The block on top of a column: modern's surface rules, in order.
    /// Under water: gravel where cold or deep, sand where warm, clay in
    /// patches. Then each biome's own ground; steep slopes (4 blocks or
    /// more to a neighbor) bare their stone, and above Y 200 grass gives
    /// way to stone or snow.
    fn top(&self, x: i32, z: i32, biome: Biome, height: i32, slope: i32) -> BlockState {
        let patch = self.surface_noise(x, z, 12.0, 0.5);
        let steep = slope >= STEEP;
        let underwater = height < SEA_LEVEL;
        let wet = matches!(biome, Biome::River | Biome::Swamp | Biome::MangroveSwamp);
        if underwater && !wet {
            let deep = height < SEA_LEVEL - 14;
            let cold = biome.climate().0 < 0.3;
            return if matches!(biome, Biome::Beach | Biome::Desert) && !deep {
                SAND
            } else if patch < -0.6 && !deep {
                CLAY
            } else if cold || deep || patch > 0.45 {
                GRAVEL
            } else {
                SAND
            };
        }
        // Low, wet ground in swamps holds pools.
        let pool = wet && patch > 0.45 && (SEA_LEVEL - 1..=SEA_LEVEL).contains(&height);
        match biome {
            Biome::River if underwater => {
                if patch > 0.3 {
                    GRAVEL
                } else if patch < -0.45 {
                    CLAY
                } else {
                    SAND
                }
            }
            Biome::Swamp | Biome::MangroveSwamp if pool => WATER,
            Biome::MangroveSwamp => MUD,
            Biome::Desert | Biome::Beach => SAND,
            Biome::StonyShore if patch > 0.3 => GRAVEL,
            Biome::StonyShore => STONE,
            Biome::Badlands if steep || height > 100 => self.band(x, height, z),
            Biome::Badlands => RED_SAND,
            Biome::MushroomFields => MYCELIUM,
            Biome::FrozenPeaks if steep && patch > 0.0 => PACKED_ICE,
            Biome::FrozenPeaks if patch > 0.55 => ICE,
            Biome::FrozenPeaks | Biome::JaggedPeaks | Biome::SnowySlopes if !steep => SNOW_BLOCK,
            Biome::FrozenPeaks | Biome::JaggedPeaks | Biome::SnowySlopes => STONE,
            // Modern's stony peaks (here windswept hills up high): stone
            // and calcite.
            Biome::WindsweptHills if height > 150 && patch > 0.15 => CALCITE,
            Biome::WindsweptHills if height > 150 || steep => STONE,
            Biome::WindsweptHills if patch > 0.45 => GRAVEL,
            _ if height > 200 => {
                if biome.is_cold_at(height) {
                    SNOW_BLOCK
                } else {
                    STONE
                }
            }
            _ if steep => STONE,
            Biome::Taiga | Biome::SnowyTaiga | Biome::BambooJungle if patch > 0.4 => PODZOL,
            Biome::Savanna if patch > 0.55 => COARSE_DIRT,
            _ if height <= SEA_LEVEL + 1 && patch > -0.2 && !wet => SAND,
            _ => GRASS,
        }
    }

    /// A badlands terracotta band at a height (modern `BandsTerracotta`,
    /// gently tilted across the land).
    fn band(&self, x: i32, y: i32, z: i32) -> BlockState {
        let tilt = (self.surface_noise(x, z, 60.0, 3.1) * 2.0).round() as i32;
        self.bands[(y + tilt).rem_euclid(BAND_COUNT as i32) as usize]
    }

    /// The columns of a chunk and a margin around it, for generating it
    /// and replaying its neighbors' trees and veins.
    pub fn area(&self, center: ChunkPos) -> Area {
        let (cx, cz) = center.min_block();
        let (min_x, min_z) = (cx - Area::MARGIN, cz - Area::MARGIN);
        let side = Area::SIDE;
        // Density corners covering one column more each way, for the slopes.
        let corners = self.corners(min_x - 1, min_z - 1, min_x + side, min_z + side);
        let wide = (side + 2) as usize;
        let mut lays = Vec::with_capacity(wide * wide);
        for z in min_z - 1..min_z + side + 1 {
            for x in min_x - 1..min_x + side + 1 {
                lays.push(Lay {
                    height: top_of(&corners.levels(x, z)),
                    offset: corners.offset(x, z),
                });
            }
        }
        let mut columns = Vec::with_capacity((side * side) as usize);
        for z in 0..side {
            for x in 0..side {
                let i = (z as usize + 1) * wide + x as usize + 1;
                let around = [i + 1, i - 1, i + wide, i - wide].map(|j| lays[j].height);
                let (wx, wz) = (min_x + x, min_z + z);
                let quart =
                    corners.climates[corners.index(wx.div_euclid(CELL_XZ), wz.div_euclid(CELL_XZ))];
                columns.push(self.column_with(wx, wz, quart, lays[i], around));
            }
        }
        Area {
            min_x,
            min_z,
            columns,
            corners,
        }
    }

    /// The noise caves' field at a point (carves where positive).
    fn cave_noise(&self, x: i32, y: i32, z: i32) -> f64 {
        self.caves.density(x, y, z)
    }

    /// Whether a cave runs through (x, y, z), ignoring what block is there.
    /// Cave noise is blended between samples on a coarse grid, exactly as
    /// `carve_caves` does.
    pub fn is_cave(&self, x: i32, y: i32, z: i32) -> bool {
        if y <= MIN_Y + BEDROCK_RAGGED {
            return false;
        }
        let (cx, cy, cz) = (
            x.div_euclid(CAVE_CELL_XZ) * CAVE_CELL_XZ,
            y.div_euclid(CAVE_CELL_Y) * CAVE_CELL_Y,
            z.div_euclid(CAVE_CELL_XZ) * CAVE_CELL_XZ,
        );
        let corner = |dx: i32, dy: i32, dz: i32| {
            self.cave_noise(
                cx + dx * CAVE_CELL_XZ,
                cy + dy * CAVE_CELL_Y,
                cz + dz * CAVE_CELL_XZ,
            )
        };
        let c = [
            [
                [corner(0, 0, 0), corner(1, 0, 0)],
                [corner(0, 0, 1), corner(1, 0, 1)],
            ],
            [
                [corner(0, 1, 0), corner(1, 1, 0)],
                [corner(0, 1, 1), corner(1, 1, 1)],
            ],
        ];
        let t = (
            (x - cx) as f64 / CAVE_CELL_XZ as f64,
            (y - cy) as f64 / CAVE_CELL_Y as f64,
            (z - cz) as f64 / CAVE_CELL_XZ as f64,
        );
        trilinear(&c, t) > 0.0
    }

    /// The highest block caves may carve in a column.
    fn cave_top(column: &Column) -> i32 {
        if column.height < SEA_LEVEL {
            column.height - CAVE_WATER_MARGIN
        } else if column.entrance {
            column.height
        } else {
            column.height - CAVE_ROOF
        }
    }

    /// Carves caves into a filled chunk: the noise caves, then the worm
    /// tunnels and ravines of the chunks around. Each carved block takes
    /// what its aquifer holds; bedrock and the ground just under water are
    /// left alone.
    fn carve_caves(&self, chunk: &mut Chunk, area: &Area) {
        let (min_x, min_z) = chunk.pos.min_block();
        let top = (0..CHUNK_SIZE * CHUNK_SIZE)
            .map(|i| {
                let column = area.get(min_x + (i % 16) as i32, min_z + (i / 16) as i32);
                Self::cave_top(column)
            })
            .max()
            .unwrap_or(MIN_Y)
            .max(MIN_Y + 1);
        // Noise on a coarse grid over the chunk, from the bottom of the
        // world up to the highest top.
        let nx = CHUNK_SIZE as i32 / CAVE_CELL_XZ + 1;
        let ny = (top - MIN_Y) / CAVE_CELL_Y + 2;
        let mut grid = vec![0.0; (nx * nx * ny) as usize];
        let index = |i: i32, j: i32, k: i32| ((j * nx + k) * nx + i) as usize;
        for j in 0..ny {
            for k in 0..nx {
                for i in 0..nx {
                    grid[index(i, j, k)] = self.cave_noise(
                        min_x + i * CAVE_CELL_XZ,
                        MIN_Y + j * CAVE_CELL_Y,
                        min_z + k * CAVE_CELL_XZ,
                    );
                }
            }
        }
        for z in 0..CHUNK_SIZE {
            for x in 0..CHUNK_SIZE {
                let column = area.get(min_x + x as i32, min_z + z as i32);
                let (i, k) = (x as i32 / CAVE_CELL_XZ, z as i32 / CAVE_CELL_XZ);
                let tx = (x as i32 % CAVE_CELL_XZ) as f64 / CAVE_CELL_XZ as f64;
                let tz = (z as i32 % CAVE_CELL_XZ) as f64 / CAVE_CELL_XZ as f64;
                for y in MIN_Y + 1..=Self::cave_top(column) {
                    let up = y - MIN_Y;
                    let j = up / CAVE_CELL_Y;
                    let ty = (up % CAVE_CELL_Y) as f64 / CAVE_CELL_Y as f64;
                    let c = [
                        [
                            [grid[index(i, j, k)], grid[index(i + 1, j, k)]],
                            [grid[index(i, j, k + 1)], grid[index(i + 1, j, k + 1)]],
                        ],
                        [
                            [grid[index(i, j + 1, k)], grid[index(i + 1, j + 1, k)]],
                            [
                                grid[index(i, j + 1, k + 1)],
                                grid[index(i + 1, j + 1, k + 1)],
                            ],
                        ],
                    ];
                    if trilinear(&c, (tx, ty, tz)) <= 0.0 {
                        continue;
                    }
                    self.carve(chunk, x, y, z, column.height);
                }
            }
        }
        self.carve_worms(chunk, area);
    }

    /// Carves one block of a cave, if it isn't bedrock, with what the
    /// aquifer holds there.
    fn carve(&self, chunk: &mut Chunk, x: usize, y: i32, z: usize, surface: i32) {
        if chunk.get(x, y, z) == BEDROCK {
            return;
        }
        let (min_x, min_z) = chunk.pos.min_block();
        let (wx, wz) = (min_x + x as i32, min_z + z as i32);
        if let Some(block) = self.caves.fill(wx, y, wz, surface).block() {
            chunk.set(x, y, z, block);
        }
    }

    /// The worm tunnels and ravines that reach this chunk, from every
    /// chunk within `CARVER_REACH`.
    fn carve_worms(&self, chunk: &mut Chunk, area: &Area) {
        let pos = chunk.pos;
        let (min_x, min_z) = pos.min_block();
        let (max_x, max_z) = (min_x + 16, min_z + 16);
        for sz in pos.z - CARVER_REACH..=pos.z + CARVER_REACH {
            for sx in pos.x - CARVER_REACH..=pos.x + CARVER_REACH {
                for worm in worms(self.seed, sx, sz) {
                    for &([px, py, pz], across, up) in &worm.steps {
                        if px + across < min_x as f64
                            || px - across >= max_x as f64
                            || pz + across < min_z as f64
                            || pz - across >= max_z as f64
                        {
                            continue;
                        }
                        let x0 = ((px - across).floor() as i32).max(min_x);
                        let x1 = ((px + across).ceil() as i32).min(max_x - 1);
                        let z0 = ((pz - across).floor() as i32).max(min_z);
                        let z1 = ((pz + across).ceil() as i32).min(max_z - 1);
                        let y0 = ((py - up).floor() as i32).max(MIN_Y + 1);
                        let y1 = (py + up).ceil() as i32;
                        for wz in z0..=z1 {
                            for wx in x0..=x1 {
                                let column = area.get(wx, wz);
                                let top = Self::cave_top(column).min(y1);
                                let dx = (wx as f64 + 0.5 - px) / across;
                                let dz = (wz as f64 + 0.5 - pz) / across;
                                if dx * dx + dz * dz >= 1.0 {
                                    continue;
                                }
                                for y in y0..=top {
                                    let dy = (y as f64 + 0.5 - py) / up;
                                    // A flatter floor, as modern carvers have.
                                    if dy < -0.7 || dx * dx + dy * dy + dz * dz >= 1.0 {
                                        continue;
                                    }
                                    let (lx, lz) = ((wx - min_x) as usize, (wz - min_z) as usize);
                                    self.carve(chunk, lx, y, lz, column.height);
                                }
                            }
                        }
                    }
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
                    if column.biome.is_ocean()
                        || column.biome == Biome::River
                        || column.height < SEA_LEVEL
                    {
                        continue;
                    }
                    // Check the generated column too: a cave can carve the
                    // surface away, and a tree or plant can stand on it.
                    let chunk = chunks
                        .entry(ChunkPos::of_block(x, z))
                        .or_insert_with_key(|&pos| self.generate(pos));
                    let (lx, lz) = (x.rem_euclid(16) as usize, z.rem_euclid(16) as usize);
                    let y = column.height;
                    let ground = chunk.get(lx, y, lz);
                    if ground.def().solid
                        && ground.def().fluid.is_none()
                        && chunk.get(lx, y + 1, lz).def().replaceable
                        && chunk.get(lx, y + 2, lz).is_air()
                    {
                        return (x, column.height + 1, z);
                    }
                }
            }
        }
        (0, MAX_Y, 0)
    }

    /// Fills a chunk with stone, surface layers, water and bedrock (no caves or features).
    pub fn fill(&self, chunk: &mut Chunk) {
        let area = self.area(chunk.pos);
        self.fill_from(chunk, &area);
    }

    fn fill_from(&self, chunk: &mut Chunk, area: &Area) {
        let (min_x, min_z) = chunk.pos.min_block();
        let mut rng = Rng::for_chunk(self.seed, chunk.pos.x, chunk.pos.z, 0xBED);
        for z in 0..CHUNK_SIZE {
            for x in 0..CHUNK_SIZE {
                let (wx, wz) = (min_x + x as i32, min_z + z as i32);
                let column = *area.get(wx, wz);
                chunk.set_biome(x, z, column.biome);
                // Cave biomes fill the quart's 4×4×4 cells whose depth
                // (1/128 a block under the surface) is in range, from the
                // column at the quart's corner.
                if let Some(cave) = column.cave
                    && x % 4 == 0
                    && z % 4 == 0
                {
                    let (lo, hi) = biome_source::CAVE_DEPTH;
                    let surface = column.offset.round() as i32;
                    let top = surface - (lo * 128.0).ceil() as i32;
                    let bottom = (surface - (hi * 128.0) as i32).max(CAVE_BIOME_FLOOR);
                    for y in (bottom..=top).step_by(4) {
                        chunk.set_cave_biome(x, y, z, Some(cave));
                    }
                }
                let depth = 3 + (self.surface_noise(wx, wz, 8.0, 7.3) * 1.5 + 1.5) as i32;
                // Solid wherever the blended density is above zero; the
                // surface layers follow the topmost solid block, and
                // overhangs leave air under them.
                let levels = area.corners.levels(wx, wz);
                for y in MIN_Y + 1..=column.height {
                    if y < 0 || density(&levels, y) > 0.0 {
                        let block = self.layer(&column, wx, y, wz, depth, &mut rng);
                        chunk.set(x, y, z, block);
                    }
                }
                for y in column.height + 1..=SEA_LEVEL {
                    chunk.set(x, y, z, WATER);
                }
                // A bedrock floor at the bottom, ragged for four blocks.
                chunk.set(x, MIN_Y, z, BEDROCK);
                for up in 1..=BEDROCK_RAGGED {
                    if rng.below(BEDROCK_RAGGED as u32 + 1) as i32 >= up {
                        chunk.set(x, MIN_Y + up, z, BEDROCK);
                    }
                }
            }
        }
    }

    /// The block at height `y` in a column: the top, the layers under it,
    /// then stone, or deepslate low down.
    fn layer(
        &self,
        column: &Column,
        x: i32,
        y: i32,
        z: i32,
        depth: i32,
        rng: &mut Rng,
    ) -> BlockState {
        let below_top = column.height - y;
        let top = column.top;
        if below_top == 0 {
            return top;
        }
        let filler = match top.id() {
            blocks::GRASS_BLOCK | blocks::PODZOL | blocks::MYCELIUM | blocks::COARSE_DIRT => {
                Some((DIRT, depth))
            }
            // Beaches and deserts: 3 or 4 sand, then sandstone.
            blocks::SAND => Some((SAND, 3 + depth % 2)),
            // Pools in swamps lie on their mud or dirt.
            blocks::WATER if column.biome == Biome::MangroveSwamp => Some((MUD, depth)),
            blocks::WATER => Some((DIRT, depth)),
            blocks::RED_SAND => Some((RED_SAND, 2)),
            blocks::GRAVEL | blocks::CLAY | blocks::MUD | blocks::PACKED_ICE | blocks::ICE => {
                Some((top, depth.min(3)))
            }
            blocks::SNOW_BLOCK => Some((SNOW_BLOCK, 2)),
            _ => None,
        };
        let mut used = 0;
        if let Some((block, n)) = filler {
            if below_top < n {
                return block;
            }
            used = n;
        }
        // Sand lies on sandstone; badlands on bands of terracotta.
        if top == SAND && column.biome != Biome::River && below_top < used + 3 {
            return SANDSTONE;
        }
        if column.biome == Biome::Badlands && (top.id() != blocks::STONE) && y > SEA_LEVEL - 8 {
            return self.band(x, y, z);
        }
        if y < DEEPSLATE_LEVEL
            || (y < DEEPSLATE_LEVEL + DEEPSLATE_BLEND
                && (rng.below(DEEPSLATE_BLEND as u32) as i32) >= y - DEEPSLATE_LEVEL)
        {
            DEEPSLATE
        } else {
            STONE
        }
    }

    /// Gives a chunk saved before M34 (Y 0 to 255) the depth new chunks
    /// have: everything below Y 0 as this world generates it now, and the
    /// old bedrock floor at Y 0–4 replaced by what lies there now (modern
    /// 1.18's upgrade of old worlds). Every block from Y 0 up stays.
    pub fn deepen(&self, chunk: &mut Chunk) {
        let fresh = self.generate(chunk.pos);
        for z in 0..CHUNK_SIZE {
            for x in 0..CHUNK_SIZE {
                for y in MIN_Y..0 {
                    chunk.set(x, y, z, fresh.get(x, y, z));
                    if y % 4 == 0 && x % 4 == 0 && z % 4 == 0 {
                        let cave = Some(fresh.biome_at(x, y, z)).filter(|b| b.is_cave());
                        chunk.set_cave_biome(x, y, z, cave);
                    }
                }
                for y in 0..=BEDROCK_RAGGED {
                    if chunk.get(x, y, z) == BEDROCK {
                        chunk.set(x, y, z, fresh.get(x, y, z));
                    }
                }
            }
        }
        for (at, entity) in fresh.block_entities() {
            if at[1] < 0 {
                chunk.set_block_entity(at, Some(entity.clone()));
            }
        }
        chunk.shallow = false;
    }

    /// Generates a chunk: terrain, caves, ores, underground features, trees
    /// and plants.
    pub fn generate(&self, pos: ChunkPos) -> Chunk {
        let area = self.area(pos);
        let mut chunk = Chunk::new(pos);
        self.fill_from(&mut chunk, &area);
        self.carve_caves(&mut chunk, &area);
        super::ores::populate(self, &area, &mut chunk);
        super::underground::populate(self, &area, &mut chunk);
        let keep_out = crate::world::structure::place(self, &mut chunk);
        super::features::populate(self, &area, &mut chunk, &keep_out);
        chunk
    }
}

/// Trilinear blend of a cell's corners, indexed `[y][z][x]`.
fn trilinear(c: &[[[f64; 2]; 2]; 2], (tx, ty, tz): (f64, f64, f64)) -> f64 {
    let x = |y: usize, z: usize| lerp(c[y][z][0], c[y][z][1], tx);
    let z = |y: usize| lerp(x(y, 0), x(y, 1), tz);
    lerp(z(0), z(1), ty)
}

/// Modern badlands bands: orange terracotta with bands of plain, yellow,
/// brown, red, white and light gray, placed by the seed.
fn badlands_bands(seed: u64) -> [BlockState; BAND_COUNT] {
    let of = BlockState::of;
    let mut bands = [of(blocks::ORANGE_TERRACOTTA); BAND_COUNT];
    let mut rng = Rng::new(mix(seed ^ 0xBAD));
    let mut i = 0;
    while i < BAND_COUNT {
        i += 1 + rng.below(5) as usize;
        if i < BAND_COUNT {
            bands[i] = TERRACOTTA;
        }
    }
    for (block, count, width) in [
        (blocks::YELLOW_TERRACOTTA, 3, 1),
        (blocks::BROWN_TERRACOTTA, 3, 2),
        (blocks::RED_TERRACOTTA, 3, 1),
        (blocks::WHITE_TERRACOTTA, 2, 2),
        (blocks::LIGHT_GRAY_TERRACOTTA, 2, 1),
    ] {
        for _ in 0..count {
            let start = rng.below(BAND_COUNT as u32) as usize;
            for k in 0..width + rng.below(2) as usize {
                bands[(start + k) % BAND_COUNT] = of(block);
            }
        }
    }
    bands
}

/// The columns of a chunk and a margin around it: as far as any tree,
/// ice spike or vein from beyond can reach into the chunk.
pub struct Area {
    min_x: i32,
    min_z: i32,
    columns: Vec<Column>,
    /// The density corners, for filling the chunk.
    corners: Corners,
}

impl Area {
    /// Every column of the eight chunks around, plus one for the far
    /// corner of a 2 × 2 trunk on their edge: trees are planned from the
    /// chunk they stand in.
    pub const MARGIN: i32 = CHUNK_SIZE as i32 + 1;
    const SIDE: i32 = CHUNK_SIZE as i32 + 2 * Self::MARGIN;

    /// The column at (x, z), which must be in the chunk or next to it.
    pub fn get(&self, x: i32, z: i32) -> &Column {
        let (i, j) = (x - self.min_x, z - self.min_z);
        debug_assert!(
            (0..Self::SIDE).contains(&i) && (0..Self::SIDE).contains(&j),
            "column {x},{z} is outside the area"
        );
        &self.columns[(j * Self::SIDE + i) as usize]
    }

    /// The column at (x, z) if it's in the area.
    pub fn try_get(&self, x: i32, z: i32) -> Option<&Column> {
        let (i, j) = (x - self.min_x, z - self.min_z);
        ((0..Self::SIDE).contains(&i) && (0..Self::SIDE).contains(&j))
            .then(|| &self.columns[(j * Self::SIDE + i) as usize])
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::collections::{HashMap, HashSet};

    fn local(x: i32, z: i32) -> (usize, usize) {
        (x.rem_euclid(16) as usize, z.rem_euclid(16) as usize)
    }

    /// The first column (on a coarse spiral from the origin) that `pick`
    /// accepts.
    pub(crate) fn find(
        terrain: &Terrain,
        what: &str,
        pick: impl Fn(&Column) -> bool,
    ) -> (i32, i32) {
        (0..4000)
            .flat_map(|r: i32| {
                let n = r.max(1) * 8;
                (0..n).map(move |k| {
                    let a = k as f64 / n as f64 * std::f64::consts::TAU;
                    (
                        (a.cos() * r as f64 * 24.0) as i32,
                        (a.sin() * r as f64 * 24.0) as i32,
                    )
                })
            })
            .find(|&(x, z)| pick(&terrain.column(x, z)))
            .unwrap_or_else(|| panic!("no {what} found"))
    }

    #[test]
    fn same_seed_same_chunk_different_seed_different_chunk() {
        let pos = ChunkPos::new(3, -7);
        let a = Terrain::new(1).generate(pos);
        let b = Terrain::new(1).generate(pos);
        let c = Terrain::new(2).generate(pos);
        let blocks = |chunk: &Chunk| -> Vec<BlockState> {
            (crate::config::MIN_Y..crate::config::MAX_Y)
                .flat_map(|y| (0..256).map(move |i| (i % 16, y, i / 16)))
                .map(|(x, y, z)| chunk.get(x, y, z))
                .collect()
        };
        assert_eq!(blocks(&a), blocks(&b));
        assert_ne!(blocks(&a), blocks(&c));
    }

    #[test]
    fn the_area_agrees_with_single_columns() {
        let terrain = Terrain::new(77);
        for pos in [
            ChunkPos::new(0, 0),
            ChunkPos::new(-31, 17),
            ChunkPos::new(140, -260),
        ] {
            let area = terrain.area(pos);
            let (x0, z0) = pos.min_block();
            for (dx, dz) in [(0, 0), (15, 15), (-12, 7), (27, -12), (8, 3)] {
                let (x, z) = (x0 + dx, z0 + dz);
                assert_eq!(*area.get(x, z), terrain.column(x, z), "{x},{z}");
            }
        }
    }

    /// M21 "done when": a seed shows every new biome.
    #[test]
    fn every_surface_biome_appears_within_reach() {
        for seed in [20111118, 1] {
            let terrain = Terrain::new(seed);
            let mut seen = HashSet::new();
            for z in (-6000..6000).step_by(48) {
                for x in (-6000..6000).step_by(48) {
                    seen.insert(terrain.column(x, z).biome);
                }
            }
            let missing: Vec<_> = Biome::ALL
                .iter()
                .filter(|b| !b.is_cave() && !b.is_nether() && !b.is_end() && !seen.contains(b))
                .collect();
            assert!(missing.is_empty(), "seed {seed} lacks {missing:?}");
        }
    }

    #[test]
    fn biome_shares_are_reasonable() {
        let terrain = Terrain::new(12345);
        let mut counts: HashMap<Biome, usize> = HashMap::new();
        for z in (-8000..8000).step_by(80) {
            for x in (-8000..8000).step_by(80) {
                *counts.entry(terrain.column(x, z).biome).or_insert(0) += 1;
            }
        }
        let total: usize = counts.values().sum();
        let share = |n: usize| n as f64 / total as f64;
        for (biome, &n) in &counts {
            assert!(share(n) < 0.12, "{biome:?} covers {:.2}", share(n));
        }
        let ocean: usize = counts
            .iter()
            .filter(|(b, _)| b.is_ocean())
            .map(|(_, n)| n)
            .sum();
        assert!(
            (0.25..0.5).contains(&share(ocean)),
            "oceans cover {:.2}",
            share(ocean)
        );
    }

    /// Surface heights and biomes on the quart grid of a square
    /// `side` quarts across round the origin.
    fn quarts(terrain: &Terrain, side: i32) -> Vec<(i32, Biome)> {
        (-side / 2..side / 2)
            .flat_map(|j| (-side / 2..side / 2).map(move |i| (i, j)))
            .map(|(i, j)| terrain.corner_surface(i * 3, j * 3))
            .collect()
    }

    #[test]
    fn oceans_sit_low_peaks_high_and_nothing_reaches_250() {
        let terrain = Terrain::new(20111118);
        let cells = quarts(&terrain, 1200);
        let mean = |pick: fn(Biome) -> bool| {
            let hs: Vec<f64> = cells
                .iter()
                .filter(|c| pick(c.1))
                .map(|c| f64::from(c.0))
                .collect();
            (hs.iter().sum::<f64>() / hs.len().max(1) as f64, hs.len())
        };
        for &(h, biome) in &cells {
            if biome.is_ocean() {
                assert!(h < 60, "{biome:?} at {h}");
            }
            assert!(h <= 250, "{h}");
        }
        let (peaks, n) = mean(|b| matches!(b, Biome::JaggedPeaks | Biome::FrozenPeaks));
        assert!(n > 50 && peaks > 150.0, "peaks average {peaks} over {n}");
        let mut heights: Vec<i32> = cells.iter().map(|c| c.0).collect();
        heights.sort();
        assert!(
            heights[heights.len() / 50] < 45,
            "deep oceans only reach {}",
            heights[heights.len() / 50]
        );
    }

    #[test]
    fn rivers_hold_water_to_sea_level_over_a_low_bed() {
        let terrain = Terrain::new(20111118);
        let mut checked = 0;
        'search: for j in -400..400 {
            for i in -400..400 {
                let (x, z) = (i * 12, j * 12);
                if terrain.biome_at(x, z) != Biome::River || terrain.height(x, z) >= SEA_LEVEL {
                    continue;
                }
                let chunk = terrain.generate(ChunkPos::of_block(x, z));
                let (lx, lz) = local(x, z);
                let column = terrain.column(x, z);
                assert!(column.height <= 61, "river bed at {}", column.height);
                assert_eq!(chunk.get(lx, SEA_LEVEL, lz), WATER);
                assert!(chunk.get(lx, SEA_LEVEL + 1, lz).is_air());
                checked += 1;
                if checked == 20 {
                    break 'search;
                }
            }
        }
        assert_eq!(checked, 20);
    }

    #[test]
    fn nothing_floats_above_200() {
        let terrain = Terrain::new(15);
        let mut high = 0;
        for j in -6..6 {
            for i in -6..6 {
                // The terrain alone (trees overhang, caves are carved).
                let mut chunk = Chunk::new(ChunkPos::new(i * 7, j * 7));
                terrain.fill(&mut chunk);
                for z in 0..16 {
                    for x in 0..16 {
                        // From 200 up, every column is solid from 200 to its
                        // top: no air under solid ground.
                        let solid = |y: i32| chunk.get(x, y, z).def().solid;
                        let top = (200..crate::config::MAX_Y).rev().find(|&y| solid(y));
                        if let Some(top) = top {
                            high += 1;
                            for y in 200..top {
                                let b = chunk.get(x, y, z);
                                assert!(!b.is_air(), "a gap at {y} under {top}");
                            }
                        }
                    }
                }
            }
        }
        assert!(high > 0, "nothing reached 200 to check");
    }

    #[test]
    fn lowlands_are_smooth_and_mountains_steep_but_continuous() {
        let terrain = Terrain::new(99);
        let (mut lowland, mut anywhere) = (0, 0);
        for z in (-3000..3000).step_by(61) {
            let mut previous = terrain.height(-3000, z);
            for x in -2999..3000 {
                let lay = terrain.lay(x, z);
                let step = (lay.height - previous).abs();
                anywhere = anywhere.max(step);
                // Lowlands (worn smooth: erosion -0.2 and up, pulled to
                // under 90), not counting the sea floor far under water.
                let worn = terrain.climate.quart(x, z).erosion >= -0.2;
                if worn && lay.offset < 90.0 && lay.height > SEA_LEVEL - 12 {
                    lowland = lowland.max(step);
                }
                previous = lay.height;
            }
        }
        // Worn land holds to its offset; mountains are left to the 3D
        // noise, whose overhangs make the top jump further.
        assert!(lowland <= 7, "a {lowland}-block step in the lowlands");
        assert!(anywhere <= 16, "a {anywhere}-block cliff");
    }

    #[test]
    fn oceans_are_water_down_to_the_floor() {
        let terrain = Terrain::new(5);
        let (x, z) = find(&terrain, "ocean", |c| {
            c.biome.is_ocean() && c.height < SEA_LEVEL - 5
        });
        let chunk = terrain.generate(ChunkPos::of_block(x, z));
        let (lx, lz) = local(x, z);
        let column = terrain.column(x, z);
        assert!(matches!(
            chunk.get(lx, SEA_LEVEL, lz).id(),
            blocks::WATER | blocks::ICE | blocks::PACKED_ICE
        ));
        assert_eq!(chunk.get(lx, column.height + 1, lz), WATER);
        assert_eq!(chunk.get(lx, column.height, lz), column.top);
    }

    #[test]
    fn rivers_carry_water_below_the_land_around() {
        let terrain = Terrain::new(5);
        let (x, z) = find(&terrain, "river", |c| {
            c.biome == Biome::River && c.height < SEA_LEVEL - 1
        });
        let chunk = terrain.generate(ChunkPos::of_block(x, z));
        let (lx, lz) = local(x, z);
        let top = chunk.get(lx, SEA_LEVEL, lz).id();
        assert!(matches!(top, blocks::WATER | blocks::ICE), "{top:?}");
    }

    #[test]
    fn spawn_is_on_open_dry_land() {
        for seed in (0..6).chain([42, 20111118]) {
            let terrain = Terrain::new(seed);
            let (x, y, z) = terrain.spawn_point();
            let column = terrain.column(x, z);
            assert!(!column.biome.is_ocean(), "seed {seed}");
            assert_eq!(y, column.height + 1);
            assert!(column.height >= SEA_LEVEL);
            let chunk = terrain.generate(ChunkPos::of_block(x, z));
            let (lx, lz) = local(x, z);
            let y = y as usize;
            assert!(chunk.get(lx, (y - 1) as i32, lz).def().solid, "seed {seed}");
            assert!(chunk.get(lx, (y + 1) as i32, lz).is_air(), "seed {seed}");
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
                        assert_eq!(chunk.get(x, MIN_Y, z), BEDROCK);
                        let column = terrain.column(min_x + x as i32, min_z + z as i32);
                        if column.height < SEA_LEVEL {
                            let floor = chunk.get(x, column.height, z);
                            assert!(!floor.is_air(), "hole under water at {x},{z}");
                        }
                        for y in MIN_Y + 1..column.height {
                            match chunk.get(x, y, z) {
                                b if b.is_air() => air_below_surface += 1,
                                // Deep caves fill with lava (and a few pools sit higher).
                                LAVA if y <= super::super::caves::LAVA_LEVEL => lava += 1,
                                _ => {}
                            }
                        }
                    }
                }
            }
        }
        assert!(
            air_below_surface > 2000,
            "only {air_below_surface} cave blocks in 64 chunks"
        );
        assert!(lava > 0, "no lava in deep caves");
    }

    #[test]
    fn is_cave_matches_the_carved_chunk() {
        let terrain = Terrain::new(8);
        let pos = ChunkPos::new(2, -3);
        let chunk = terrain.generate(pos);
        let (min_x, min_z) = pos.min_block();
        // Rock left where the noise opens a cave is an aquifer's wall;
        // it stays a small share.
        let (mut checked, mut walls) = (0, 0);
        for (x, z) in [(3usize, 4usize), (10, 12), (15, 0)] {
            for y in -50..40 {
                let block = chunk.get(x, y, z);
                if matches!(block.id(), blocks::STONE | blocks::DEEPSLATE) {
                    walls += usize::from(terrain.is_cave(min_x + x as i32, y, min_z + z as i32));
                    checked += 1;
                }
            }
        }
        assert!(checked > 60);
        assert!(walls * 10 <= checked, "{walls} of {checked}");
    }

    #[test]
    fn deepslate_lies_below_stone() {
        let terrain = Terrain::new(3);
        let chunk = terrain.generate(ChunkPos::new(5, 5));
        let count = |range: std::ops::Range<i32>, block: BlockState| {
            range
                .flat_map(|y| (0..256).map(move |i| (i % 16, y, i / 16)))
                .filter(|&(x, y, z)| chunk.get(x, y, z) == block)
                .count()
        };
        // All deepslate below Y 0, blending into stone up to 8.
        assert_eq!(
            count(DEEPSLATE_LEVEL + DEEPSLATE_BLEND..MAX_Y, DEEPSLATE),
            0
        );
        assert_eq!(count(MIN_Y + 5..DEEPSLATE_LEVEL, STONE), 0);
        assert!(count(-50..-10, DEEPSLATE) > 1000);
        assert!(count(0..8, DEEPSLATE) > 0 && count(0..8, STONE) > 0);
    }

    /// Generates the chunk holding (x, z) and returns its column there.
    fn column_blocks(terrain: &Terrain, x: i32, z: i32) -> Vec<BlockState> {
        let chunk = terrain.generate(ChunkPos::of_block(x, z));
        let (lx, lz) = local(x, z);
        (crate::config::MIN_Y..crate::config::MAX_Y)
            .map(|y| chunk.get(lx, y, lz))
            .collect()
    }

    /// An open column (no cave entrance) of `biome`, not on a slope.
    fn open(terrain: &Terrain, biome: Biome) -> (i32, i32, Column) {
        let (x, z) = find(terrain, biome.name(), |c| {
            c.biome == biome && !c.entrance && c.slope <= 1 && c.height > SEA_LEVEL
        });
        (x, z, terrain.column(x, z))
    }

    #[test]
    fn biomes_have_their_own_ground() {
        let terrain = Terrain::new(20111118);
        for (biome, top) in [
            (Biome::Desert, SAND),
            (Biome::MushroomFields, MYCELIUM),
            (Biome::MangroveSwamp, MUD),
            (Biome::JaggedPeaks, SNOW_BLOCK),
            (Biome::Plains, GRASS),
        ] {
            let (x, z, column) = open(&terrain, biome);
            assert_eq!(column.top, top, "{biome:?}");
            let blocks = column_blocks(&terrain, x, z);
            let h = (column.height - crate::config::MIN_Y) as usize;
            if column.top == SAND {
                assert!(
                    blocks[..h].contains(&SANDSTONE),
                    "sandstone under {biome:?}"
                );
            }
            assert!(
                blocks[h].id() == top.id() || blocks[h].id() == blocks::DIRT,
                "{biome:?}: {:?}",
                blocks[h]
            );
        }
        // Badlands: red sand on bands of terracotta.
        let (x, z, column) = open(&terrain, Biome::Badlands);
        let blocks = column_blocks(&terrain, x, z);
        let h = (column.height - crate::config::MIN_Y) as usize;
        assert!(
            blocks[h - 10..h]
                .iter()
                .any(|b| b.id().name().ends_with("terracotta"))
        );
        // Cold biomes: snow on top, snowy grass.
        let (x, z, column) = open(&terrain, Biome::SnowyPlains);
        let blocks = column_blocks(&terrain, x, z);
        let h = (column.height - crate::config::MIN_Y) as usize;
        assert_eq!(blocks[h + 1].id(), blocks::SNOW, "{:?}", &blocks[h..h + 3]);
        assert!(blocks[h].bool(crate::world::props::Prop::Snowy));
        // Frozen oceans freeze over.
        let (x, z) = find(&terrain, "frozen ocean", |c| c.biome == Biome::FrozenOcean);
        let blocks = column_blocks(&terrain, x, z);
        assert!(matches!(
            blocks[(SEA_LEVEL - crate::config::MIN_Y) as usize].id(),
            blocks::ICE | blocks::PACKED_ICE | blocks::SNOW_BLOCK
        ));
    }

    #[test]
    fn cave_biomes_are_stored_deep_under_their_columns() {
        let terrain = Terrain::new(31);
        let (x, z) = find(&terrain, "lush caves", |c| {
            c.cave == Some(Biome::LushCaves) && c.height > 80
        });
        let column = terrain.column(x, z);
        let chunk = terrain.generate(ChunkPos::of_block(x, z));
        let (lx, lz) = local(x, z);
        // Halfway down the cave band (depth 0.2 to 0.9 under the quart's
        // corner).
        let deep = terrain.height(x.div_euclid(4) * 4, z.div_euclid(4) * 4) - 64;
        assert_eq!(
            chunk.biome_at(lx, (deep as usize) as i32, lz),
            Biome::LushCaves
        );
        assert_eq!(
            chunk.biome_at(lx, (column.height as usize) as i32, lz),
            column.biome
        );
    }

    /// Prints biome shares, heights and slopes over a big square:
    /// `cargo test --release survey -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn survey() {
        for seed in [1u64, 12345, 20111118] {
            let terrain = Terrain::new(seed);
            let mut counts: HashMap<Biome, usize> = HashMap::new();
            let mut heights = Vec::new();
            let mut first: HashMap<Biome, (i32, i32)> = HashMap::new();
            let step = 32;
            for z in (-8000..8000).step_by(step) {
                for x in (-8000..8000).step_by(step) {
                    let c = terrain.column(x, z);
                    *counts.entry(c.biome).or_insert(0) += 1;
                    heights.push(c.height);
                    let d = x.abs().max(z.abs());
                    first
                        .entry(c.biome)
                        .and_modify(|p| {
                            if d < p.0.abs().max(p.1.abs()) {
                                *p = (x, z)
                            }
                        })
                        .or_insert((x, z));
                }
            }
            let total: usize = counts.values().sum();
            let mut list: Vec<_> = counts.iter().collect();
            list.sort_by_key(|(_, n)| std::cmp::Reverse(**n));
            println!("seed {seed}:");
            for (biome, n) in &list {
                println!(
                    "  {:20} {:5.2}%  nearest {:?}",
                    biome.name(),
                    **n as f64 * 100.0 / total as f64,
                    first[biome]
                );
            }
            let missing: Vec<_> = Biome::ALL
                .iter()
                .filter(|b| !b.is_cave() && !counts.contains_key(b))
                .collect();
            println!("  missing: {missing:?}");
            heights.sort();
            let q = |p: f64| heights[((heights.len() - 1) as f64 * p) as usize];
            println!(
                "  heights q01 {} q10 {} q50 {} q90 {} q99 {} max {}",
                q(0.01),
                q(0.1),
                q(0.5),
                q(0.9),
                q(0.99),
                heights.last().unwrap()
            );
            // Steepness along lines.
            let mut steps = vec![0usize; 64];
            for z in (-2000..2000).step_by(97) {
                let mut prev = terrain.height(-2000, z);
                for x in -1999..2000 {
                    let h = terrain.height(x, z);
                    steps[((h - prev).unsigned_abs() as usize).min(63)] += 1;
                    if (h - prev).abs() > 9 {
                        println!(
                            "    step {} at {x},{z}: {:?} offset {:.1}",
                            h - prev,
                            terrain.column(x, z).biome,
                            terrain.lay(x, z).offset,
                        );
                    }
                    prev = h;
                }
            }
            let big: Vec<_> = steps
                .iter()
                .enumerate()
                .filter(|(i, n)| *i > 3 && **n > 0)
                .collect();
            println!("  steps >3: {big:?}");
        }
    }
}

#[cfg(test)]
mod timing {
    use super::*;
    use std::time::Instant;

    /// `cargo test --release timing -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn stages() {
        let terrain = Terrain::new(20111118);
        let n = 300;
        let mut t = [0.0f64; 6];
        for i in 0..n {
            let pos = ChunkPos::new(i % 20, i / 20 + 3);
            let s = Instant::now();
            let area = terrain.area(pos);
            t[0] += s.elapsed().as_secs_f64();
            let mut chunk = Chunk::new(pos);
            let s = Instant::now();
            terrain.fill_from(&mut chunk, &area);
            t[1] += s.elapsed().as_secs_f64();
            let s = Instant::now();
            terrain.carve_caves(&mut chunk, &area);
            t[2] += s.elapsed().as_secs_f64();
            let s = Instant::now();
            super::super::ores::populate(&terrain, &area, &mut chunk);
            t[3] += s.elapsed().as_secs_f64();
            let s = Instant::now();
            super::super::underground::populate(&terrain, &area, &mut chunk);
            t[4] += s.elapsed().as_secs_f64();
            let s = Instant::now();
            super::super::features::populate(&terrain, &area, &mut chunk, &[]);
            t[5] += s.elapsed().as_secs_f64();
        }
        for (name, v) in ["area", "fill", "caves", "ores", "underground", "features"]
            .iter()
            .zip(t)
        {
            println!("{name:12} {:.3} ms", v * 1000.0 / n as f64);
        }
    }

    /// Share of land columns (out of `n`) whose surface block a cave has
    /// carved away: openings to the sky.
    fn openings(seed: u64) -> (usize, usize) {
        let terrain = Terrain::new(seed);
        let (mut open, mut land) = (0, 0);
        for cz in -6..6 {
            for cx in -6..6 {
                let chunk = terrain.generate(ChunkPos::new(cx, cz));
                let (min_x, min_z) = chunk.pos.min_block();
                for z in 0..16 {
                    for x in 0..16 {
                        let column = terrain.column(min_x + x as i32, min_z + z as i32);
                        if column.height <= SEA_LEVEL {
                            continue;
                        }
                        land += 1;
                        let top = chunk.get(x, column.height, z);
                        let below = chunk.get(x, column.height - 2, z);
                        open += usize::from(top.is_air() && below.is_air());
                    }
                }
            }
        }
        (open, land)
    }

    #[test]
    fn caves_open_to_the_surface_only_rarely() {
        let (mut open, mut land) = (0, 0);
        for seed in [15, 3, 20111118] {
            let (o, l) = openings(seed);
            eprintln!("seed {seed}: openings {o} of {l}");
            open += o;
            land += l;
        }
        assert!(open > 0, "no cave reaches the surface");
        assert!(open * 100 < land, "{open} of {land} columns open");
    }
}
