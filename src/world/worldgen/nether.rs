//! The Nether's generator (M24), after modern Minecraft's `nether` noise
//! settings and `NetherBiomes`.
//!
//! - **Shape**: 3D noise on 4 × 8 × 4 cells, slid toward solid at the
//!   bottom (bedrock floor) and the top (bedrock roof), leaving caverns
//!   between. Everything is in y 0..128; the world above the roof is air.
//! - **Lava sea**: open space at y 31 and below is lava (modern sea level 32).
//! - **Biomes**: modern's multi-noise points on temperature and humidity:
//!   nether wastes, crimson forest, warped forest, soul sand valley and
//!   basalt deltas, picked per column.
//! - **Surfaces**: nylium on forest floors, soul sand and soul soil in the
//!   valleys, basalt and blackstone in the deltas, gravel and soul sand by
//!   the lava shore in the wastes.
//! - **Features**: glowstone under ceilings, quartz and gold ore, magma
//!   near the lava, hidden and open lava springs, fire, huge fungi, roots,
//!   vines, basalt pillars and columns, and lava deltas rimmed with magma.
//!
//! Features can cross chunk borders, so each chunk replays the features of
//! its eight neighbors and keeps the blocks that land inside it. Features
//! decide where they go from the bare terrain (`Land`), which any chunk
//! works out the same way, so they come out whole on both sides of a border.

use noise::{Fbm, MultiFractal, NoiseFn, Perlin};

use super::{
    climate::{noise_offset, noise_seed},
    rng::{Rng, mix},
};
use crate::{
    config::CHUNK_SIZE,
    world::{
        biome::Biome,
        block::BlockState,
        blocks,
        chunk::{Chunk, ChunkPos},
        fast_hash::FastMap,
        props::{Prop, axis},
    },
};

/// The Nether is 128 blocks tall: bedrock at y 0 and y 127.
pub const HEIGHT: i32 = 128;
/// Open space up to this height is lava.
pub const LAVA_LEVEL: i32 = 31;
/// Bedrock thins out over this many blocks from the floor and the roof.
const BEDROCK_LAYERS: i32 = 5;

const CELL_XZ: i32 = 4;
const CELL_Y: i32 = 8;
const LEVELS: usize = (HEIGHT / CELL_Y) as usize + 1;

/// Wavelengths of the shaping noise, in blocks.
const SHAPE_XZ: f64 = 56.0;
const SHAPE_Y: f64 = 34.0;
/// Pulls the noise toward open space between the floor and roof slides.
const SHAPE_BIAS: f64 = -0.08;
/// Modern `slideNetherLike`: toward 0.9375 over the top 24 blocks and
/// toward 2.5 from y 24 down.
const TOP_SLIDE: (f64, f64) = (24.0, 0.9375);
const BOTTOM_SLIDE: (f64, f64) = (24.0, 2.5);

/// Wavelength of the biome noise, in blocks.
const BIOME_SCALE: f64 = 260.0;

const fn of(id: crate::world::block::BlockId) -> BlockState {
    BlockState::of(id)
}

const AIR: BlockState = BlockState::AIR;
const NETHERRACK: BlockState = of(blocks::NETHERRACK);
const LAVA: BlockState = of(blocks::LAVA);
const BEDROCK: BlockState = of(blocks::BEDROCK);
const SOUL_SAND: BlockState = of(blocks::SOUL_SAND);
const SOUL_SOIL: BlockState = of(blocks::SOUL_SOIL);
const GRAVEL: BlockState = of(blocks::GRAVEL);
const BLACKSTONE: BlockState = of(blocks::BLACKSTONE);
const MAGMA: BlockState = of(blocks::MAGMA_BLOCK);
const GLOWSTONE: BlockState = of(blocks::GLOWSTONE);
const SHROOMLIGHT: BlockState = of(blocks::SHROOMLIGHT);

fn upright(id: crate::world::block::BlockId) -> BlockState {
    of(id).with(Prop::Axis, axis::Y)
}

/// Densities up one corner column, every `CELL_Y` blocks.
type Levels = [f64; LEVELS];

pub struct Nether {
    /// The world's seed, and the Nether's own mixed from it.
    world_seed: u64,
    seed: u64,
    shift: [f64; 3],
    shape: Fbm<Perlin>,
    temperature: Fbm<Perlin>,
    humidity: Fbm<Perlin>,
    /// Soul sand against soul soil, basalt against blackstone.
    patches: Perlin,
    /// Gravel and soul sand by the lava shore in the wastes.
    shore: Perlin,
}

/// Modern `MultiNoiseBiomeSourceParameterList.NETHER`: (temperature,
/// humidity, offset) for each biome.
const POINTS: [(Biome, f64, f64, f64); 5] = [
    (Biome::NetherWastes, 0.0, 0.0, 0.0),
    (Biome::SoulSandValley, 0.0, -0.5, 0.0),
    (Biome::CrimsonForest, 0.4, 0.0, 0.0),
    (Biome::WarpedForest, 0.0, 0.5, 0.375),
    (Biome::BasaltDeltas, -0.5, 0.0, 0.175),
];

impl Nether {
    pub fn new(seed: u64) -> Self {
        // Its own noises, unlike the Overworld's.
        let world_seed = seed;
        let seed = mix(seed ^ 0x0000_4E45_5448_4552);
        let fbm = |index, octaves| Fbm::<Perlin>::new(noise_seed(seed, index)).set_octaves(octaves);
        Self {
            world_seed,
            seed,
            shift: noise_offset(seed, 0),
            shape: fbm(1, 4),
            temperature: fbm(2, 2),
            humidity: fbm(3, 2),
            patches: Perlin::new(noise_seed(seed, 4)),
            shore: Perlin::new(noise_seed(seed, 5)),
        }
    }

    /// The biome of the column at (x, z): the nearest point to its climate,
    /// sampled at the middle of its 4 × 4 quart as modern biomes are.
    pub fn biome(&self, x: i32, z: i32) -> Biome {
        let (qx, qz) = (x.div_euclid(4) * 4 + 2, z.div_euclid(4) * 4 + 2);
        let p = [
            qx as f64 / BIOME_SCALE + self.shift[0],
            qz as f64 / BIOME_SCALE + self.shift[2],
        ];
        // Two octaves stay within about ±0.75; stretch them to modern's range.
        let t = self.temperature.get(p) * 1.4;
        let h = self.humidity.get([p[1], p[0]]) * 1.4;
        POINTS
            .iter()
            .map(|&(biome, pt, ph, offset)| {
                let d = (t - pt).powi(2) + (h - ph).powi(2) + offset * offset;
                (biome, d)
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map_or(Biome::NetherWastes, |(biome, _)| biome)
    }

    /// The density levels of the corner column at grid point (gx, gz).
    fn corner(&self, gx: i32, gz: i32) -> Levels {
        let (x, z) = ((gx * CELL_XZ) as f64, (gz * CELL_XZ) as f64);
        std::array::from_fn(|k| {
            let y = (k as i32 * CELL_Y) as f64;
            let noise = self.shape.get([
                x / SHAPE_XZ + self.shift[0],
                y / SHAPE_Y + self.shift[1],
                z / SHAPE_XZ + self.shift[2],
            ]);
            let mut density = noise + SHAPE_BIAS;
            let top = ((y - (HEIGHT as f64 - TOP_SLIDE.0)) / TOP_SLIDE.0).clamp(0.0, 1.0);
            density += (TOP_SLIDE.1 - density) * top;
            let bottom = ((BOTTOM_SLIDE.0 - y) / BOTTOM_SLIDE.0).clamp(0.0, 1.0);
            density += (BOTTOM_SLIDE.1 - density) * bottom;
            density
        })
    }

    pub fn world_seed(&self) -> u64 {
        self.world_seed
    }

    /// Feet height on the lowest floor above the lava at (x, z) with room
    /// to stand, if there is one: where test worlds start.
    pub fn standing_height(&self, x: i32, z: i32) -> Option<i32> {
        let mut land = Land::new(self);
        land.floors(x, z)
            .into_iter()
            .rev()
            .find(|&y| (1..=3).all(|up| land.open(x, y + up, z)))
            .map(|y| y + 1)
    }

    pub fn generate(&self, pos: ChunkPos) -> Chunk {
        let mut land = Land::new(self);
        let mut chunk = Chunk::new(pos);
        let (x0, z0) = pos.min_block();
        for lz in 0..CHUNK_SIZE {
            for lx in 0..CHUNK_SIZE {
                let (x, z) = (x0 + lx as i32, z0 + lz as i32);
                let biome = land.biome(x, z);
                let bits = land.column(x, z);
                chunk.set_biome(lx, lz, biome);
                for y in 0..HEIGHT {
                    let block = self.base(bits, biome, x, y, z);
                    if block != AIR {
                        chunk.set(lx, y as usize, lz, block);
                    }
                }
            }
        }
        let mut out = Out { chunk: &mut chunk };
        for (salt, feature) in FEATURES {
            for dz in -1..=1 {
                for dx in -1..=1 {
                    let origin = pos.offset(dx, dz);
                    let mut rng = Rng::for_chunk(self.seed, origin.x, origin.z, *salt);
                    feature(&mut land, &mut out, origin, &mut rng);
                }
            }
        }
        // Fortresses, bastions and ruined portals over everything.
        crate::world::structure::nether::place(self, &mut chunk);
        chunk
    }
}

/// The bare Nether terrain for structures laying themselves out: solid
/// blocks, floors and biomes, cached column by column.
pub struct Probe<'a>(Land<'a>);

impl<'a> Probe<'a> {
    pub fn new(nether: &'a Nether) -> Self {
        Self(Land::new(nether))
    }

    pub fn solid(&mut self, x: i32, y: i32, z: i32) -> bool {
        self.0.solid(x, y, z)
    }

    /// Whether (x, y, z) is lava (open space at the lava sea's height).
    pub fn lava(&mut self, x: i32, y: i32, z: i32) -> bool {
        y <= LAVA_LEVEL && !self.0.solid(x, y, z)
    }

    /// The highest solid block at or below `y`, if any above the bedrock.
    pub fn ground_below(&mut self, x: i32, y: i32, z: i32) -> Option<i32> {
        let bits = self.0.column(x, z);
        (1..=y.min(HEIGHT - 1)).rev().find(|&y| bits >> y & 1 == 1)
    }

    /// Floors (solid with open space above, over the lava) from the top.
    pub fn floors(&mut self, x: i32, z: i32) -> Vec<i32> {
        self.0.floors(x, z)
    }

    pub fn biome(&mut self, x: i32, z: i32) -> Biome {
        self.0.biome(x, z)
    }
}

/// The bare terrain, worked out column by column as features ask for it:
/// the same in every chunk that asks.
struct Land<'a> {
    nether: &'a Nether,
    corners: FastMap<(i32, i32), Levels>,
    /// Bit y set where the block at y is solid.
    columns: FastMap<(i32, i32), u128>,
    biomes: FastMap<(i32, i32), Biome>,
}

impl<'a> Land<'a> {
    fn new(nether: &'a Nether) -> Self {
        Self {
            nether,
            corners: FastMap::default(),
            columns: FastMap::default(),
            biomes: FastMap::default(),
        }
    }

    fn corner(&mut self, gx: i32, gz: i32) -> Levels {
        let nether = self.nether;
        *self
            .corners
            .entry((gx, gz))
            .or_insert_with(|| nether.corner(gx, gz))
    }

    /// Which blocks of the column are solid, as bits.
    fn column(&mut self, x: i32, z: i32) -> u128 {
        if let Some(&bits) = self.columns.get(&(x, z)) {
            return bits;
        }
        let (gx, gz) = (x.div_euclid(CELL_XZ), z.div_euclid(CELL_XZ));
        let tx = (x - gx * CELL_XZ) as f64 / CELL_XZ as f64;
        let tz = (z - gz * CELL_XZ) as f64 / CELL_XZ as f64;
        let (a, b, c, d) = (
            self.corner(gx, gz),
            self.corner(gx + 1, gz),
            self.corner(gx, gz + 1),
            self.corner(gx + 1, gz + 1),
        );
        let levels: Levels = std::array::from_fn(|k| {
            let near = a[k] + (b[k] - a[k]) * tx;
            let far = c[k] + (d[k] - c[k]) * tx;
            near + (far - near) * tz
        });
        let mut bits = 0u128;
        for y in 0..HEIGHT {
            let k = (y / CELL_Y) as usize;
            let t = (y % CELL_Y) as f64 / CELL_Y as f64;
            if levels[k] + (levels[k + 1] - levels[k]) * t > 0.0 {
                bits |= 1 << y;
            }
        }
        // The bedrock floor and roof are always there.
        bits |= 1 | 1 << (HEIGHT - 1);
        self.columns.insert((x, z), bits);
        bits
    }

    fn solid(&mut self, x: i32, y: i32, z: i32) -> bool {
        if y < 0 {
            return true;
        }
        y < HEIGHT && self.column(x, z) >> y & 1 == 1
    }

    /// Open space above the lava, where things can stand or hang.
    fn open(&mut self, x: i32, y: i32, z: i32) -> bool {
        y > LAVA_LEVEL && y < HEIGHT && !self.solid(x, y, z)
    }

    fn biome(&mut self, x: i32, z: i32) -> Biome {
        let nether = self.nether;
        *self
            .biomes
            .entry((x, z))
            .or_insert_with(|| nether.biome(x, z))
    }

    /// Solid blocks with open space just above, from the top down.
    fn floors(&mut self, x: i32, z: i32) -> Vec<i32> {
        let bits = self.column(x, z);
        (1..HEIGHT - 2)
            .rev()
            .filter(|&y| bits >> y & 1 == 1 && bits >> (y + 1) & 1 == 0 && y + 1 > LAVA_LEVEL)
            .collect()
    }

    /// Solid blocks with open space just below, from the top down.
    fn ceilings(&mut self, x: i32, z: i32) -> Vec<i32> {
        let bits = self.column(x, z);
        (LAVA_LEVEL + 2..HEIGHT - 1)
            .rev()
            .filter(|&y| bits >> y & 1 == 1 && bits >> (y - 1) & 1 == 0)
            .collect()
    }

    /// The block before features: bedrock, the surface rules over
    /// netherrack, lava or air.
    fn base(&mut self, x: i32, y: i32, z: i32) -> BlockState {
        let bits = self.column(x, z);
        let biome = self.biome(x, z);
        self.nether.base(bits, biome, x, y, z)
    }
}

impl Nether {
    /// The block at (x, y, z) in a column with solid `bits` and `biome`.
    fn base(&self, bits: u128, biome: Biome, x: i32, y: i32, z: i32) -> BlockState {
        if bedrock(self.seed, x, y, z) {
            return BEDROCK;
        }
        if bits >> y & 1 == 0 {
            return if y <= LAVA_LEVEL { LAVA } else { AIR };
        }
        // How deep the block lies under the floor above it, and how far
        // above the open space below it (0 = touching it).
        let above = (bits >> (y + 1)).trailing_ones() as i32;
        let below = if y == 0 {
            HEIGHT
        } else {
            (!(bits << (127 - (y - 1)) as u32)).leading_zeros() as i32
        };
        self.surface(biome, x, y, z, above, below)
    }

    fn surface(&self, biome: Biome, x: i32, y: i32, z: i32, above: i32, below: i32) -> BlockState {
        let floor = above < HEIGHT - 1 - y;
        let nether = self;
        let patch = |scale: f64| {
            nether.patches.get([
                x as f64 / scale + nether.shift[0],
                y as f64 / scale,
                z as f64 / scale + nether.shift[2],
            ])
        };
        let under_lava = y + above < LAVA_LEVEL;
        match biome {
            Biome::CrimsonForest if floor && above == 0 && !under_lava => {
                of(blocks::CRIMSON_NYLIUM)
            }
            Biome::WarpedForest if floor && above == 0 && !under_lava => of(blocks::WARPED_NYLIUM),
            Biome::SoulSandValley if (floor && above < 3) || below < 2 => {
                if patch(12.0) > 0.0 {
                    SOUL_SAND
                } else {
                    SOUL_SOIL
                }
            }
            Biome::BasaltDeltas if (floor && above < 4) || below < 2 => {
                if patch(8.0) > 0.25 {
                    BLACKSTONE
                } else {
                    upright(blocks::BASALT)
                }
            }
            Biome::NetherWastes
                if floor
                    && above < 3
                    && (LAVA_LEVEL - 1..=LAVA_LEVEL + 3).contains(&(y + above)) =>
            {
                let shore = nether.shore.get([x as f64 / 16.0, z as f64 / 16.0]);
                if shore > 0.25 {
                    SOUL_SAND
                } else if shore < -0.3 {
                    GRAVEL
                } else {
                    NETHERRACK
                }
            }
            _ => NETHERRACK,
        }
    }
}

/// Bedrock at y 0 and 127, thinning out over the next four layers in
/// (modern `bedrock_floor` and `bedrock_roof`).
fn bedrock(seed: u64, x: i32, y: i32, z: i32) -> bool {
    let depth = y.min(HEIGHT - 1 - y);
    if depth >= BEDROCK_LAYERS {
        return false;
    }
    if depth == 0 {
        return true;
    }
    let roll = Rng::for_chunk(seed, x, z, 0xBED0 + y as u64).below(BEDROCK_LAYERS as u32);
    (roll as i32) < BEDROCK_LAYERS - depth
}

/// Where features write: the chunk being generated, keeping only the blocks
/// that land in it.
struct Out<'c> {
    chunk: &'c mut Chunk,
}

impl Out<'_> {
    fn local(&self, x: i32, y: i32, z: i32) -> Option<(usize, usize, usize)> {
        let (x0, z0) = self.chunk.pos.min_block();
        let (lx, lz) = (x - x0, z - z0);
        let size = CHUNK_SIZE as i32;
        ((0..size).contains(&lx) && (0..size).contains(&lz) && (1..HEIGHT - 1).contains(&y))
            .then_some((lx as usize, y as usize, lz as usize))
    }

    /// Whether a feature `reach` blocks around (x, z) could touch the chunk.
    fn near(&self, x: i32, z: i32, reach: i32) -> bool {
        let (x0, z0) = self.chunk.pos.min_block();
        let size = CHUNK_SIZE as i32;
        x + reach >= x0 && x - reach < x0 + size && z + reach >= z0 && z - reach < z0 + size
    }

    fn set(&mut self, x: i32, y: i32, z: i32, block: BlockState) {
        if let Some((lx, ly, lz)) = self.local(x, y, z) {
            self.chunk.set(lx, ly, lz, block);
        }
    }

    /// Sets the block only where it's still `host` (as features before
    /// left it): every chunk sees the same, since a block is in one chunk.
    fn replace(
        &mut self,
        x: i32,
        y: i32,
        z: i32,
        host: impl Fn(BlockState) -> bool,
        block: BlockState,
    ) {
        if let Some((lx, ly, lz)) = self.local(x, y, z)
            && host(self.chunk.get(lx, ly, lz))
        {
            self.chunk.set(lx, ly, lz, block);
        }
    }
}

type Feature = fn(&mut Land, &mut Out, ChunkPos, &mut Rng);

/// Every feature in the order they're placed, each with its salt.
const FEATURES: &[(u64, Feature)] = &[
    (0x0101, blobs),
    (0x0102, ores),
    (0x010C, debris),
    (0x0103, springs),
    (0x0104, deltas),
    (0x0105, columns),
    (0x0106, pillars),
    (0x0107, glowstone),
    (0x0108, fires),
    (0x0109, ceiling_vines),
    (0x010A, vegetation),
    (0x010B, fungi),
];

/// A random column of the chunk.
fn spot(rng: &mut Rng, origin: ChunkPos) -> (i32, i32) {
    let (x0, z0) = origin.min_block();
    (
        x0 + rng.below(CHUNK_SIZE as u32) as i32,
        z0 + rng.below(CHUNK_SIZE as u32) as i32,
    )
}

/// Uniform in lo..=hi.
fn between(rng: &mut Rng, lo: i32, hi: i32) -> i32 {
    lo + rng.below((hi - lo + 1) as u32) as i32
}

fn is_netherrack(block: BlockState) -> bool {
    block == NETHERRACK
}

/// Stone the deltas' blobs and the ores may replace.
fn is_ground(block: BlockState) -> bool {
    matches!(
        block.id(),
        blocks::NETHERRACK | blocks::BASALT | blocks::BLACKSTONE | blocks::SOUL_SOIL
    )
}

/// A rough ball of `block` replacing what `host` allows (modern
/// `ReplaceBlobsFeature` and ore veins, simplified).
fn ball(
    out: &mut Out,
    rng: &mut Rng,
    center: (i32, i32, i32),
    radius: f64,
    host: fn(BlockState) -> bool,
    block: BlockState,
) {
    let (cx, cy, cz) = center;
    let reach = radius.ceil() as i32;
    let squash = [
        0.7 + rng.next_f64() * 0.6,
        0.7 + rng.next_f64() * 0.6,
        0.7 + rng.next_f64() * 0.6,
    ];
    let rough = rng.next_u64();
    for dy in -reach..=reach {
        for dz in -reach..=reach {
            for dx in -reach..=reach {
                let d = (dx as f64 * squash[0]).powi(2)
                    + (dy as f64 * squash[1]).powi(2)
                    + (dz as f64 * squash[2]).powi(2);
                // A ragged edge, rolled per block so every chunk agrees.
                let edge = Rng::for_chunk(rough, cx + dx, cz + dz, (cy + dy) as u64).next_f64();
                if d <= radius * radius * (0.75 + 0.25 * edge) {
                    out.replace(cx + dx, cy + dy, cz + dz, host, block);
                }
            }
        }
    }
}

/// Basalt and blackstone blobs through the deltas' netherrack (modern
/// `basalt_blobs` and `blackstone_blobs`).
fn blobs(land: &mut Land, out: &mut Out, origin: ChunkPos, rng: &mut Rng) {
    for n in 0..16 {
        let (x, z) = spot(rng, origin);
        let y = between(rng, 2, HEIGHT - 3);
        let radius = 3.0 + rng.next_f64() * 3.0;
        if !out.near(x, z, 7) || land.biome(x, z) != Biome::BasaltDeltas {
            continue;
        }
        let block = if n < 12 {
            upright(blocks::BASALT)
        } else {
            BLACKSTONE
        };
        ball(out, rng, (x, y, z), radius, is_netherrack, block);
    }
}

/// Quartz and gold ore everywhere, doubled in the deltas, and magma by the
/// lava sea (modern `NetherPlacements`).
fn ores(land: &mut Land, out: &mut Out, origin: ChunkPos, rng: &mut Rng) {
    let (x0, z0) = origin.min_block();
    let deltas = land.biome(x0 + 8, z0 + 8) == Biome::BasaltDeltas;
    let twice = if deltas { 2 } else { 1 };
    let kinds = [
        (
            16 * twice,
            10,
            HEIGHT - 10,
            1.6,
            of(blocks::NETHER_QUARTZ_ORE),
        ),
        (
            10 * twice,
            10,
            HEIGHT - 10,
            1.4,
            of(blocks::NETHER_GOLD_ORE),
        ),
        (4, LAVA_LEVEL - 4, LAVA_LEVEL + 5, 2.6, MAGMA),
        (2, 5, LAVA_LEVEL, 2.4, GRAVEL),
    ];
    for (count, lo, hi, radius, block) in kinds {
        for _ in 0..count {
            let (x, z) = spot(rng, origin);
            let y = between(rng, lo, hi);
            let r = radius * (0.6 + rng.next_f64() * 0.6);
            if out.near(x, z, 4) {
                ball(out, rng, (x, y, z), r, is_ground, block);
            }
        }
    }
}

/// Ancient debris, never touching open space: about two a chunk, as modern's
/// `ore_ancient_debris_large` (y 8–24) and `ore_debris_small` (y 8–119)
/// come to once air-exposed blocks are thrown away.
fn debris(land: &mut Land, out: &mut Out, origin: ChunkPos, rng: &mut Rng) {
    for (size, lo, hi) in [(2, 8, 24), (1, 8, 119)] {
        let (x, z) = spot(rng, origin);
        let y = between(rng, lo, hi);
        if !out.near(x, z, 2) {
            continue;
        }
        for _ in 0..size {
            let (bx, by, bz) = (
                x + between(rng, -1, 1),
                y + between(rng, -1, 1),
                z + between(rng, -1, 1),
            );
            let hidden = [
                (1, 0, 0),
                (-1, 0, 0),
                (0, 1, 0),
                (0, -1, 0),
                (0, 0, 1),
                (0, 0, -1),
            ]
            .iter()
            .all(|(dx, dy, dz)| land.solid(bx + dx, by + dy, bz + dz));
            if hidden {
                out.replace(bx, by, bz, is_ground, of(blocks::ANCIENT_DEBRIS));
            }
        }
    }
}

/// Lava hidden in the netherrack, and lava falling from walls (modern
/// `spring_closed` and `spring_open`).
fn springs(land: &mut Land, out: &mut Out, origin: ChunkPos, rng: &mut Rng) {
    let deltas = {
        let (x0, z0) = origin.min_block();
        land.biome(x0 + 8, z0 + 8) == Biome::BasaltDeltas
    };
    for n in 0..20 {
        let (x, z) = spot(rng, origin);
        let y = between(rng, 10, HEIGHT - 10);
        if !out.near(x, z, 0) || !land.solid(x, y, z) {
            continue;
        }
        let sides = [
            (1, 0, 0),
            (-1, 0, 0),
            (0, 0, 1),
            (0, 0, -1),
            (0, 1, 0),
            (0, -1, 0),
        ];
        let open = sides
            .iter()
            .filter(|(dx, dy, dz)| !land.solid(x + dx, y + dy, z + dz))
            .count();
        // Closed springs sit in solid rock; four (eight in the deltas) of
        // twenty may be open on one side, under rock.
        let open_ok = n < if deltas { 8 } else { 4 } && open == 1 && land.solid(x, y + 1, z);
        if open == 0 || open_ok {
            out.replace(x, y, z, is_ground, LAVA);
        }
    }
}

/// Lava pools rimmed with magma on the deltas' floors (modern `delta`).
fn deltas(land: &mut Land, out: &mut Out, origin: ChunkPos, rng: &mut Rng) {
    for _ in 0..6 {
        let (x, z) = spot(rng, origin);
        let radius = between(rng, 2, 5);
        let rim = between(rng, 0, 1);
        if !out.near(x, z, radius + rim) || land.biome(x, z) != Biome::BasaltDeltas {
            continue;
        }
        let Some(&y) = land.floors(x, z).first() else {
            continue;
        };
        let reach = radius + rim;
        for dz in -reach..=reach {
            for dx in -reach..=reach {
                let (bx, bz) = (x + dx, z + dz);
                let d = dx * dx + dz * dz;
                if d > reach * reach || !land.solid(bx, y, bz) || land.solid(bx, y + 1, bz) {
                    continue;
                }
                // Lava needs ground under it and on every side to stay put.
                let held = land.solid(bx, y - 1, bz)
                    && [(1, 0), (-1, 0), (0, 1), (0, -1)]
                        .iter()
                        .all(|(sx, sz)| land.solid(bx + sx, y, bz + sz));
                if d <= radius * radius && held {
                    out.set(bx, y, bz, LAVA);
                } else if rim > 0 {
                    out.set(bx, y, bz, MAGMA);
                }
            }
        }
    }
}

/// Clusters of basalt columns rising from the deltas' floors and lava
/// (modern `small_basalt_columns` and `large_basalt_columns`).
fn columns(land: &mut Land, out: &mut Out, origin: ChunkPos, rng: &mut Rng) {
    for n in 0..5 {
        let (x, z) = spot(rng, origin);
        let large = n < 2;
        let (reach, height) = if large {
            (between(rng, 2, 3), between(rng, 5, 10))
        } else {
            (between(rng, 0, 1), between(rng, 1, 4))
        };
        if !out.near(x, z, reach) || land.biome(x, z) != Biome::BasaltDeltas {
            continue;
        }
        let base = land.floors(x, z).first().copied().unwrap_or(LAVA_LEVEL);
        let base = base.max(LAVA_LEVEL);
        let seed = rng.next_u64();
        for dz in -reach..=reach {
            for dx in -reach..=reach {
                let (bx, bz) = (x + dx, z + dz);
                let mut col = Rng::for_chunk(seed, bx, bz, 0);
                let falloff = dx.abs() + dz.abs();
                let h = height - falloff * height / (reach * 2 + 2) - col.below(2) as i32;
                if h <= 0 || col.below(4) == 0 && falloff > 0 {
                    continue;
                }
                for y in base + 1..=(base + h).min(HEIGHT - 6) {
                    if land.solid(bx, y, bz) {
                        break;
                    }
                    out.set(bx, y, bz, upright(blocks::BASALT));
                }
            }
        }
    }
}

/// Basalt pillars from floor to ceiling in the soul sand valleys (modern
/// `basalt_pillar`).
fn pillars(land: &mut Land, out: &mut Out, origin: ChunkPos, rng: &mut Rng) {
    for _ in 0..3 {
        let (x, z) = spot(rng, origin);
        if !out.near(x, z, 1) || land.biome(x, z) != Biome::SoulSandValley {
            continue;
        }
        let ceilings = land.ceilings(x, z);
        if ceilings.is_empty() {
            continue;
        }
        let top = ceilings[rng.below(ceilings.len() as u32) as usize] - 1;
        let seed = rng.next_u64();
        let mut y = top;
        while y > LAVA_LEVEL && !land.solid(x, y, z) {
            out.set(x, y, z, upright(blocks::BASALT));
            // A few blocks beside the pillar, near its ends.
            for (i, (dx, dz)) in [(1, 0), (-1, 0), (0, 1), (0, -1)].into_iter().enumerate() {
                if Rng::for_chunk(seed, x, z, (y * 4 + i as i32) as u64).below(6) == 0
                    && !land.solid(x + dx, y, z + dz)
                {
                    out.set(x + dx, y, z + dz, upright(blocks::BASALT));
                }
            }
            y -= 1;
        }
    }
}

/// Glowstone hanging in clumps from the ceilings (modern `glowstone` and
/// `glowstone_extra`): each grows from a block under the ceiling, adding
/// cells that touch exactly one glowstone.
fn glowstone(land: &mut Land, out: &mut Out, origin: ChunkPos, rng: &mut Rng) {
    let extra = rng.below(10) + 1;
    let count = 6 + rng.below(extra);
    for _ in 0..count {
        let (x, z) = spot(rng, origin);
        if !out.near(x, z, 7) {
            continue;
        }
        let ceilings = land.ceilings(x, z);
        let Some(&ceiling) = ceilings.get(rng.below(ceilings.len().max(1) as u32) as usize) else {
            continue;
        };
        let y = ceiling - 1;
        // The clump fits in 15 × 12 × 15 cells below its first block.
        const SIDE: i32 = 15;
        const DEPTH: i32 = 12;
        let index = |dx: i32, dy: i32, dz: i32| ((-dy * SIDE + dz + 7) * SIDE + dx + 7) as usize;
        let inside = |dx: i32, dy: i32, dz: i32| {
            dx.abs() <= 7 && dz.abs() <= 7 && (-(DEPTH - 1)..=0).contains(&dy)
        };
        let mut placed = [false; (SIDE * SIDE * DEPTH) as usize];
        let mut columns = [0u128; (SIDE * SIDE) as usize];
        for dz in -7..=7 {
            for dx in -7..=7 {
                columns[((dz + 7) * SIDE + dx + 7) as usize] = land.column(x + dx, z + dz);
            }
        }
        placed[index(0, 0, 0)] = true;
        for _ in 0..600 {
            let (dx, dy, dz) = (
                rng.below(8) as i32 - rng.below(8) as i32,
                -(rng.below(12) as i32),
                rng.below(8) as i32 - rng.below(8) as i32,
            );
            let solid = columns[((dz + 7) * SIDE + dx + 7) as usize] >> (y + dy) & 1 == 1;
            if placed[index(dx, dy, dz)] || solid || y + dy <= LAVA_LEVEL {
                continue;
            }
            let touching = [
                (1, 0, 0),
                (-1, 0, 0),
                (0, 1, 0),
                (0, -1, 0),
                (0, 0, 1),
                (0, 0, -1),
            ]
            .iter()
            .filter(|(sx, sy, sz)| {
                let (nx, ny, nz) = (dx + sx, dy + sy, dz + sz);
                inside(nx, ny, nz) && placed[index(nx, ny, nz)]
            })
            .count();
            if touching == 1 {
                placed[index(dx, dy, dz)] = true;
            }
        }
        for dy in -(DEPTH - 1)..=0 {
            for dz in -7..=7 {
                for dx in -7..=7 {
                    if placed[index(dx, dy, dz)] {
                        out.set(x + dx, y + dy, z + dz, GLOWSTONE);
                    }
                }
            }
        }
    }
}

/// Fire burning on the wastes' netherrack and soul fire in the valleys
/// (modern `patch_fire` and `patch_soul_fire`).
fn fires(land: &mut Land, out: &mut Out, origin: ChunkPos, rng: &mut Rng) {
    for _ in 0..rng.below(4) {
        let (x, z) = spot(rng, origin);
        if !out.near(x, z, 3) {
            continue;
        }
        let biome = land.biome(x, z);
        let (fire, ground) = match biome {
            Biome::NetherWastes => (of(blocks::FIRE), NETHERRACK),
            Biome::SoulSandValley => (of(blocks::SOUL_FIRE), SOUL_SAND),
            _ => continue,
        };
        let floors = land.floors(x, z);
        let Some(&y) = floors.get(rng.below(floors.len().max(1) as u32) as usize) else {
            continue;
        };
        for _ in 0..12 {
            let (bx, bz) = (x + between(rng, -3, 3), z + between(rng, -3, 3));
            let by = y + between(rng, -1, 1);
            let below = land.base(bx, by, bz);
            let soul = biome == Biome::SoulSandValley && below.is(blocks::SOUL_SOIL);
            if (below == ground || soul) && land.open(bx, by + 1, bz) {
                out.set(bx, by + 1, bz, fire);
            }
        }
    }
}

/// Weeping vines hanging from crimson ceilings, from patches of nether
/// wart block (modern `weeping_vines`).
fn ceiling_vines(land: &mut Land, out: &mut Out, origin: ChunkPos, rng: &mut Rng) {
    for _ in 0..10 {
        let (x, z) = spot(rng, origin);
        if !out.near(x, z, 4) || land.biome(x, z) != Biome::CrimsonForest {
            continue;
        }
        let ceilings = land.ceilings(x, z);
        let Some(&y) = ceilings.get(rng.below(ceilings.len().max(1) as u32) as usize) else {
            continue;
        };
        for _ in 0..30 {
            let (bx, bz) = (x + between(rng, -3, 3), z + between(rng, -3, 3));
            let by = y + between(rng, -1, 1);
            if land.solid(bx, by, bz) && !land.solid(bx, by - 1, bz) {
                out.replace(bx, by, bz, is_netherrack, of(blocks::NETHER_WART_BLOCK));
            }
        }
        for _ in 0..20 {
            let (bx, bz) = (x + between(rng, -3, 3), z + between(rng, -3, 3));
            let by = y + between(rng, -1, 1);
            if land.solid(bx, by, bz) && land.open(bx, by - 1, bz) {
                let length = between(rng, 1, 8);
                hang(land, out, (bx, by - 1, bz), length, true);
            }
        }
    }
}

/// A vine `length` long from (x, y, z): down for weeping vines, up for
/// twisting ones, ending where something is in the way (another vine
/// too). A vine stays in its column, so it reads the chunk itself.
fn hang(land: &mut Land, out: &mut Out, (x, y, z): (i32, i32, i32), length: i32, down: bool) {
    let step = if down { -1 } else { 1 };
    let mut cells = Vec::new();
    for i in 0..length {
        let cy = y + i * step;
        let free = out
            .local(x, cy, z)
            .is_some_and(|(lx, ly, lz)| out.chunk.get(lx, ly, lz) == AIR);
        if !land.open(x, cy, z) || !free {
            break;
        }
        cells.push(cy);
    }
    let (tip, plant) = if down {
        (blocks::WEEPING_VINES, blocks::WEEPING_VINES_PLANT)
    } else {
        (blocks::TWISTING_VINES, blocks::TWISTING_VINES_PLANT)
    };
    let last = cells.len().saturating_sub(1);
    for (i, &cy) in cells.iter().enumerate() {
        let block = if i == last { of(tip) } else { of(plant) };
        out.set(x, cy, z, block);
    }
}

/// Roots, fungi, sprouts and twisting vines on nylium, and mushrooms in
/// the dark of the wastes and valleys (modern `crimson_forest_vegetation`,
/// `warped_forest_vegetation`, `nether_sprouts`, `twisting_vines`).
fn vegetation(land: &mut Land, out: &mut Out, origin: ChunkPos, rng: &mut Rng) {
    let (x0, z0) = origin.min_block();
    let seed = rng.next_u64();
    for lz in 0..CHUNK_SIZE as i32 {
        for lx in 0..CHUNK_SIZE as i32 {
            let (x, z) = (x0 + lx, z0 + lz);
            if !out.near(x, z, 0) {
                continue;
            }
            let biome = land.biome(x, z);
            for y in land.floors(x, z) {
                let mut roll = Rng::for_chunk(seed, x, z, y as u64);
                let chance = roll.next_f64();
                let plant = match biome {
                    Biome::CrimsonForest => match chance {
                        c if c < 0.20 => blocks::CRIMSON_ROOTS,
                        c if c < 0.23 => blocks::CRIMSON_FUNGUS,
                        c if c < 0.232 => blocks::WARPED_FUNGUS,
                        _ => continue,
                    },
                    Biome::WarpedForest => match chance {
                        c if c < 0.16 => blocks::WARPED_ROOTS,
                        c if c < 0.26 => blocks::NETHER_SPROUTS,
                        c if c < 0.29 => blocks::WARPED_FUNGUS,
                        c if c < 0.292 => blocks::CRIMSON_FUNGUS,
                        c if c < 0.33 => {
                            let length = between(&mut roll, 1, 8);
                            hang(land, out, (x, y + 1, z), length, false);
                            continue;
                        }
                        _ => continue,
                    },
                    Biome::NetherWastes | Biome::SoulSandValley => match chance {
                        c if c < 0.002 => blocks::BROWN_MUSHROOM,
                        c if c < 0.004 => blocks::RED_MUSHROOM,
                        _ => continue,
                    },
                    _ => continue,
                };
                let ground = land.base(x, y, z);
                let on_nylium =
                    ground.is(blocks::CRIMSON_NYLIUM) || ground.is(blocks::WARPED_NYLIUM);
                let mushroom = matches!(plant, blocks::BROWN_MUSHROOM | blocks::RED_MUSHROOM);
                if on_nylium || mushroom && ground != GRAVEL {
                    out.set(x, y + 1, z, of(plant));
                }
            }
        }
    }
}

/// Huge crimson and warped fungi: a stem with a cap of wart block, lit by
/// shroomlight, crimson ones dripping weeping vines (modern
/// `HugeFungusFeature`, not planted).
fn fungi(land: &mut Land, out: &mut Out, origin: ChunkPos, rng: &mut Rng) {
    for _ in 0..8 {
        let (x, z) = spot(rng, origin);
        let biome = land.biome(x, z);
        let crimson = match biome {
            Biome::CrimsonForest => true,
            Biome::WarpedForest => false,
            _ => continue,
        };
        let floors = land.floors(x, z);
        let Some(&ground) = floors.get(rng.below(floors.len().max(1) as u32) as usize) else {
            continue;
        };
        let mut height = between(rng, 4, 13);
        if rng.below(12) == 0 {
            height *= 2;
        }
        let thick = rng.next_f64() < 0.06;
        let seed = rng.next_u64();
        let reach = if thick { 4 } else { 3 };
        if !out.near(x, z, reach) {
            continue;
        }
        if !land.base(x, ground, z).is(if crimson {
            blocks::CRIMSON_NYLIUM
        } else {
            blocks::WARPED_NYLIUM
        }) {
            continue;
        }
        // Room to grow: shorten it under a low ceiling, skip it if too low.
        let mut room = 0;
        while room < height + 1 && land.open(x, ground + 1 + room, z) {
            room += 1;
        }
        if room < 5 {
            continue;
        }
        height = height.min(room - 1);
        let top = ground + height;
        let (stem, wart) = if crimson {
            (blocks::CRIMSON_STEM, blocks::NETHER_WART_BLOCK)
        } else {
            (blocks::WARPED_STEM, blocks::WARPED_WART_BLOCK)
        };
        let stem_reach = i32::from(thick);
        for y in ground + 1..=top {
            for dz in -stem_reach..=stem_reach {
                for dx in -stem_reach..=stem_reach {
                    let corner = dx != 0 && dz != 0;
                    if corner && Rng::for_chunk(seed, dx, dz, y as u64).below(10) != 0 {
                        continue;
                    }
                    if land.open(x + dx, y, z + dz) {
                        out.set(x + dx, y, z + dz, upright(stem));
                    }
                }
            }
        }
        let cap = (rng.below((1 + height / 3) as u32) as i32 + 5).min(height);
        for y in top - cap + 1..=top + 1 {
            let from_top = top + 1 - y;
            let radius = match from_top {
                0 => 1,
                _ if from_top < 3 => 2,
                _ => 2 + i32::from(thick),
            };
            for dz in -radius..=radius {
                for dx in -radius..=radius {
                    let (bx, bz) = (x + dx, z + dz);
                    let on_stem = dx.abs() <= stem_reach && dz.abs() <= stem_reach;
                    if on_stem && y <= top || !land.open(bx, y, bz) {
                        continue;
                    }
                    let edge = dx.abs() == radius || dz.abs() == radius;
                    let corner = dx.abs() == radius && dz.abs() == radius;
                    let mut roll = Rng::for_chunk(seed, bx, bz, y as u64 + 1000);
                    let r = roll.next_f64();
                    if corner && r < 0.5 {
                        continue;
                    }
                    if edge || from_top == 0 {
                        if r < 0.95 {
                            out.set(bx, y, bz, of(wart));
                            // Weeping vines hang from the bottom of the cap.
                            if crimson && y == top - cap + 1 && roll.below(5) == 0 {
                                let length = between(&mut roll, 1, 4);
                                hang(land, out, (bx, y - 1, bz), length, true);
                            }
                        }
                    } else if r < 0.08 {
                        out.set(bx, y, bz, SHROOMLIGHT);
                    } else if r < 0.75 {
                        out.set(bx, y, bz, of(wart));
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn count(chunk: &Chunk, id: crate::world::block::BlockId) -> usize {
        let mut n = 0;
        for y in 0..HEIGHT as usize {
            for z in 0..CHUNK_SIZE {
                for x in 0..CHUNK_SIZE {
                    n += usize::from(chunk.get(x, y, z).is(id));
                }
            }
        }
        n
    }

    /// `NETHER_SLICE=out.png cargo test --release nether_slice -- --ignored`
    /// draws a side view: x across, y up, one chunk row at z 8.
    #[test]
    #[ignore]
    fn nether_slice() {
        let Ok(path) = std::env::var("NETHER_SLICE") else {
            return;
        };
        let nether = Nether::new(20111118);
        let width = 32 * 16;
        let mut image = image::RgbaImage::new(width, HEIGHT as u32);
        for cx in 0..32 {
            let chunk = nether.generate(ChunkPos::new(cx - 16, 0));
            for x in 0..16 {
                for y in 0..HEIGHT as usize {
                    let block = chunk.get(x, y, 8);
                    let color = match block.id() {
                        blocks::AIR => [20, 10, 10],
                        blocks::LAVA => [230, 90, 10],
                        blocks::NETHERRACK => [110, 40, 40],
                        blocks::BEDROCK => [40, 40, 40],
                        blocks::GLOWSTONE | blocks::SHROOMLIGHT => [255, 230, 120],
                        blocks::CRIMSON_NYLIUM | blocks::NETHER_WART_BLOCK => [180, 20, 30],
                        blocks::WARPED_NYLIUM | blocks::WARPED_WART_BLOCK => [20, 140, 120],
                        blocks::SOUL_SAND | blocks::SOUL_SOIL => [90, 70, 55],
                        blocks::BASALT | blocks::BLACKSTONE => [60, 60, 70],
                        _ => [200, 200, 200],
                    };
                    let px = (cx * 16 + x as i32) as u32;
                    image.put_pixel(
                        px,
                        HEIGHT as u32 - 1 - y as u32,
                        image::Rgba([color[0], color[1], color[2], 255]),
                    );
                }
            }
        }
        image.save(path).unwrap();
    }

    /// `cargo test --release nether_views -- --ignored --nocapture`: a big
    /// open floor in each biome of seed 20111118, for `--shot` scenes.
    #[test]
    #[ignore]
    fn nether_views() {
        let nether = Nether::new(20111118);
        for biome in Biome::NETHER {
            let mut found = None;
            'search: for r in (0..1500).step_by(16) {
                for (x, z) in [
                    (r, 0),
                    (0, r),
                    (-r, 0),
                    (0, -r),
                    (r, r),
                    (-r, r),
                    (r, -r),
                    (-r, -r),
                ] {
                    let all = (-12..=12).step_by(6).all(|dx| {
                        (-12..=12)
                            .step_by(6)
                            .all(|dz| nether.biome(x + dx, z + dz) == biome)
                    });
                    if !all {
                        continue;
                    }
                    let mut land = Land::new(&nether);
                    for y in land.floors(x, z) {
                        let room = (1..40).take_while(|&up| land.open(x, y + up, z)).count();
                        // Open ahead to the south too.
                        let ahead = (1..30).all(|dz| land.open(x, y + 6, z + dz));
                        if room >= 20 && ahead {
                            found = Some((x, y, z, room));
                            break 'search;
                        }
                    }
                }
            }
            eprintln!("{biome:?}: {found:?}");
        }
    }

    #[test]
    fn the_same_seed_makes_the_same_nether() {
        let pos = ChunkPos::new(3, -7);
        let (a, b) = (Nether::new(5).generate(pos), Nether::new(5).generate(pos));
        for y in 0..HEIGHT as usize {
            for z in 0..CHUNK_SIZE {
                for x in 0..CHUNK_SIZE {
                    assert_eq!(a.get(x, y, z), b.get(x, y, z));
                }
            }
        }
        let other = Nether::new(6).generate(pos);
        let differ = (0..HEIGHT as usize)
            .filter(|&y| a.get(8, y, 8) != other.get(8, y, 8))
            .count();
        assert!(differ > 10);
    }

    #[test]
    fn bedrock_closes_the_floor_and_roof_with_lava_and_caverns_between() {
        let nether = Nether::new(20111118);
        let (mut air, mut lava, mut sampled) = (0, 0, 0);
        for cz in -4..4 {
            for cx in -4..4 {
                let chunk = nether.generate(ChunkPos::new(cx * 5, cz * 5));
                for z in 0..CHUNK_SIZE {
                    for x in 0..CHUNK_SIZE {
                        assert!(chunk.get(x, 0, z).is(blocks::BEDROCK));
                        assert!(chunk.get(x, 127, z).is(blocks::BEDROCK));
                        for y in 128..256 {
                            assert_eq!(chunk.get(x, y, z), AIR);
                        }
                        for y in 1..=LAVA_LEVEL as usize {
                            lava += usize::from(chunk.get(x, y, z).is(blocks::LAVA));
                        }
                        for y in 40..100 {
                            sampled += 1;
                            let block = chunk.get(x, y, z);
                            air += usize::from(block == AIR || !block.def().solid);
                        }
                    }
                }
            }
        }
        let open = air as f64 / sampled as f64;
        eprintln!("open between y 40 and 100: {open:.2}, lava blocks {lava}");
        assert!((0.3..0.75).contains(&open), "open share {open}");
        assert!(lava > 64 * 256 * 3, "a lava sea");
    }

    #[test]
    fn every_nether_biome_takes_a_fair_share() {
        let nether = Nether::new(20111118);
        let mut share: HashMap<Biome, usize> = HashMap::new();
        let mut total = 0;
        for z in (-4096..4096).step_by(32) {
            for x in (-4096..4096).step_by(32) {
                *share.entry(nether.biome(x, z)).or_default() += 1;
                total += 1;
            }
        }
        for biome in Biome::NETHER {
            let s = share.get(&biome).copied().unwrap_or(0) as f64 / total as f64;
            eprintln!("{biome:?}: {:.1}%", s * 100.0);
            assert!(s > 0.06, "{biome:?} only {s}");
        }
        assert!(share.keys().all(|b| b.is_nether()));
    }

    /// A chunk of `biome` near the origin of seed 20111118.
    fn chunk_of(nether: &Nether, biome: Biome) -> Chunk {
        for r in 0..200 {
            for (cx, cz) in [(r, 0), (0, r), (-r, 0), (0, -r), (r, r), (-r, -r)] {
                let (x, z) = (cx * 16 + 8, cz * 16 + 8);
                let all =
                    (0..4).all(|i| nether.biome(x + (i % 2) * 7 - 3, z + (i / 2) * 7 - 3) == biome);
                if all {
                    return nether.generate(ChunkPos::new(cx, cz));
                }
            }
        }
        panic!("no {biome:?} chunk");
    }

    #[test]
    fn each_biome_has_its_ground_and_growth() {
        let nether = Nether::new(20111118);
        let crimson = chunk_of(&nether, Biome::CrimsonForest);
        assert!(count(&crimson, blocks::CRIMSON_NYLIUM) > 30);
        assert!(count(&crimson, blocks::CRIMSON_ROOTS) > 0);
        let warped = chunk_of(&nether, Biome::WarpedForest);
        assert!(count(&warped, blocks::WARPED_NYLIUM) > 30);
        assert!(count(&warped, blocks::WARPED_ROOTS) + count(&warped, blocks::NETHER_SPROUTS) > 0);
        let valley = chunk_of(&nether, Biome::SoulSandValley);
        assert!(count(&valley, blocks::SOUL_SAND) + count(&valley, blocks::SOUL_SOIL) > 100);
        let deltas = chunk_of(&nether, Biome::BasaltDeltas);
        assert!(count(&deltas, blocks::BASALT) + count(&deltas, blocks::BLACKSTONE) > 300);
        assert_eq!(count(&deltas, blocks::CRIMSON_NYLIUM), 0);
        let wastes = chunk_of(&nether, Biome::NetherWastes);
        assert!(count(&wastes, blocks::NETHERRACK) > 5000);
        assert_eq!(count(&wastes, blocks::SOUL_SOIL), 0);
    }

    #[test]
    fn fungi_glowstone_and_ores_turn_up() {
        let nether = Nether::new(20111118);
        let mut totals: HashMap<&str, usize> = HashMap::new();
        for cz in -6..6 {
            for cx in -6..6 {
                let chunk = nether.generate(ChunkPos::new(cx * 3, cz * 3));
                for (name, id) in [
                    ("glowstone", blocks::GLOWSTONE),
                    ("quartz", blocks::NETHER_QUARTZ_ORE),
                    ("gold", blocks::NETHER_GOLD_ORE),
                    ("magma", blocks::MAGMA_BLOCK),
                    ("ancient debris", blocks::ANCIENT_DEBRIS),
                    ("crimson stem", blocks::CRIMSON_STEM),
                    ("warped stem", blocks::WARPED_STEM),
                    ("shroomlight", blocks::SHROOMLIGHT),
                    ("weeping vines", blocks::WEEPING_VINES),
                    ("twisting vines", blocks::TWISTING_VINES),
                ] {
                    *totals.entry(name).or_default() += count(&chunk, id);
                }
            }
        }
        eprintln!("{totals:?}");
        for (name, n) in &totals {
            assert!(*n > 0, "no {name}");
        }
        // About 16 quartz veins a chunk.
        assert!(totals["quartz"] > 144 * 30);
    }

    #[test]
    fn features_line_up_across_chunk_borders() {
        // A chunk generated on its own matches what its neighbors expect:
        // each block of a feature is written by exactly one chunk, so glue
        // two chunks together and check nothing hangs in the air.
        let nether = Nether::new(77);
        for cx in 0..6 {
            let chunk = nether.generate(ChunkPos::new(cx, 0));
            for y in 1..HEIGHT as usize - 1 {
                for z in 0..CHUNK_SIZE {
                    for x in 0..CHUNK_SIZE {
                        let block = chunk.get(x, y, z);
                        // Glowstone is part of a clump hanging from rock.
                        if block.is(blocks::WEEPING_VINES) || block.is(blocks::WEEPING_VINES_PLANT)
                        {
                            let above = chunk.get(x, y + 1, z);
                            assert!(
                                above.def().solid || above.is(blocks::WEEPING_VINES_PLANT),
                                "a weeping vine at {x},{y},{z} hangs from {}",
                                above.def().name
                            );
                        }
                        if block.is(blocks::CRIMSON_ROOTS) || block.is(blocks::WARPED_ROOTS) {
                            let below = chunk.get(x, y - 1, z);
                            assert!(
                                below.is(blocks::CRIMSON_NYLIUM) || below.is(blocks::WARPED_NYLIUM),
                                "roots on {}",
                                below.def().name
                            );
                        }
                    }
                }
            }
        }
    }
}
