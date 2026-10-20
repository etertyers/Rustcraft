//! Ore and stone veins placed like modern Minecraft's (`OrePlacements`),
//! with their heights fitted to our world: modern y -64..63 is squeezed into
//! our y 0..63 and everything from sea level up keeps its height.
//!
//! A vein can cross a chunk border, so each chunk replays the veins of its
//! eight neighbors too and keeps only the blocks that land inside itself.

use std::{f64::consts::PI, sync::LazyLock};

use super::{
    rng::Rng,
    terrain::{Area, DEEPSLATE, DIRT, GRAVEL, STONE, Terrain},
};
use crate::{
    config::{CHUNK_SIZE, SEA_LEVEL, WORLD_HEIGHT},
    world::{
        biome::Biome,
        block::BlockState,
        blocks,
        chunk::{Chunk, ChunkPos},
    },
};

const fn of(id: crate::world::block::BlockId) -> BlockState {
    BlockState::of(id)
}

/// Our height for a modern height: below sea level, modern -64..63 maps
/// onto 0..63; from sea level up the height is the same.
pub const fn ours(modern: i32) -> i32 {
    if modern >= SEA_LEVEL {
        modern
    } else {
        // Round to the nearest block, staying under sea level.
        let y = ((modern + 64) * SEA_LEVEL * 2 + 127).div_euclid(127 * 2);
        if y > SEA_LEVEL - 1 { SEA_LEVEL - 1 } else { y }
    }
}

/// How the centers of a kind's veins spread up and down (modern
/// `HeightRangePlacement`), in our heights.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Spread {
    /// Evenly over `low..=high`.
    Uniform { low: i32, high: i32 },
    /// Most at `peak`, thinning to none at `low` and `high` (modern
    /// `trapezoid` with no plateau). The ends may lie outside the world.
    Triangle { low: i32, peak: i32, high: i32 },
}

impl Spread {
    fn uniform(low: i32, high: i32) -> Self {
        Spread::Uniform {
            low: ours(low),
            high: ours(high),
        }
    }

    fn triangle(low: i32, high: i32) -> Self {
        Spread::Triangle {
            low: ours(low),
            peak: ours((low + high) / 2),
            high: ours(high),
        }
    }

    fn sample(self, rng: &mut Rng) -> i32 {
        match self {
            Spread::Uniform { low, high } => low + rng.below((high - low + 1) as u32) as i32,
            Spread::Triangle { low, peak, high } => {
                // Sum of two uniforms over each half, as modern's trapezoid.
                let half_low = peak - low;
                let half_high = high - peak;
                let a = rng.below(half_low.max(1) as u32 + 1) as i32;
                let b = rng.below(half_high.max(1) as u32 + 1) as i32;
                low + a + b
            }
        }
    }

    /// Lowest and highest center.
    pub fn range(self) -> (i32, i32) {
        match self {
            Spread::Uniform { low, high } | Spread::Triangle { low, high, .. } => (low, high),
        }
    }
}

/// Where a kind of vein may form.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Place {
    Anywhere,
    /// Only where the column's biome is one of these (emeralds).
    Biomes(&'static [Biome]),
    /// Only where the cave biome beneath is this one (dripstone copper).
    Cave(Biome),
}

/// One kind of vein, as modern `OreConfiguration` plus its placement.
#[derive(Debug, Clone, Copy)]
pub struct VeinKind {
    pub name: &'static str,
    /// Placed in stone (and the stones that count as stone).
    pub block: BlockState,
    /// Placed in deepslate and tuff instead, if different.
    pub deep: BlockState,
    /// Veins attempted per chunk; below 1, the chance of one.
    pub count: f32,
    /// Blocks per vein (before overlaps and the wrong stone are skipped).
    pub size: u32,
    pub spread: Spread,
    /// Chance a block of the vein beside air isn't placed (modern
    /// `discardChanceOnAirExposure`).
    pub air_discard: f32,
    pub place: Place,
}

const MOUNTAINS: &[Biome] = &[
    Biome::WindsweptHills,
    Biome::Meadow,
    Biome::Grove,
    Biome::SnowySlopes,
    Biome::JaggedPeaks,
    Biome::FrozenPeaks,
    Biome::CherryGrove,
];

/// Modern's ore list (1.21 `OrePlacements`) in its order, then the stone
/// blobs. Heights are modern ones; `Spread` fits them to our world.
pub fn kinds() -> Vec<VeinKind> {
    let vein = |name, block, deep, count, size, spread, air_discard, place| VeinKind {
        name,
        block,
        deep,
        count,
        size,
        spread,
        air_discard,
        place,
    };
    let (u, t) = (Spread::uniform, Spread::triangle);
    let any = Place::Anywhere;
    let coal = (of(blocks::COAL_ORE), of(blocks::DEEPSLATE_COAL_ORE));
    let iron = (of(blocks::IRON_ORE), of(blocks::DEEPSLATE_IRON_ORE));
    let gold = (of(blocks::GOLD_ORE), of(blocks::DEEPSLATE_GOLD_ORE));
    let redstone = (of(blocks::REDSTONE_ORE), of(blocks::DEEPSLATE_REDSTONE_ORE));
    let diamond = (of(blocks::DIAMOND_ORE), of(blocks::DEEPSLATE_DIAMOND_ORE));
    let lapis = (of(blocks::LAPIS_ORE), of(blocks::DEEPSLATE_LAPIS_ORE));
    let copper = (of(blocks::COPPER_ORE), of(blocks::DEEPSLATE_COPPER_ORE));
    let emerald = (of(blocks::EMERALD_ORE), of(blocks::DEEPSLATE_EMERALD_ORE));
    let same = |b: BlockState| (b, b);
    vec![
        vein("dirt", DIRT, DIRT, 7.0, 33, u(0, 160), 0.0, any),
        vein("gravel", GRAVEL, GRAVEL, 14.0, 33, u(-64, 255), 0.0, any),
        vein(
            "granite",
            same(of(blocks::GRANITE)).0,
            of(blocks::GRANITE),
            2.0,
            64,
            u(0, 60),
            0.0,
            any,
        ),
        vein(
            "diorite",
            of(blocks::DIORITE),
            of(blocks::DIORITE),
            2.0,
            64,
            u(0, 60),
            0.0,
            any,
        ),
        vein(
            "andesite",
            of(blocks::ANDESITE),
            of(blocks::ANDESITE),
            2.0,
            64,
            u(0, 60),
            0.0,
            any,
        ),
        vein(
            "tuff",
            of(blocks::TUFF),
            of(blocks::TUFF),
            2.0,
            64,
            u(-64, 0),
            0.0,
            any,
        ),
        vein(
            "coal (upper)",
            coal.0,
            coal.1,
            30.0,
            17,
            u(136, 255),
            0.0,
            any,
        ),
        vein(
            "coal (lower)",
            coal.0,
            coal.1,
            20.0,
            17,
            t(0, 192),
            0.5,
            any,
        ),
        vein(
            "iron (upper)",
            iron.0,
            iron.1,
            90.0,
            9,
            t(80, 384),
            0.0,
            any,
        ),
        vein(
            "iron (middle)",
            iron.0,
            iron.1,
            10.0,
            9,
            t(-24, 56),
            0.0,
            any,
        ),
        vein(
            "iron (small)",
            iron.0,
            iron.1,
            10.0,
            4,
            u(-64, 72),
            0.0,
            any,
        ),
        vein("gold", gold.0, gold.1, 4.0, 9, t(-64, 32), 0.5, any),
        vein(
            "gold (lower)",
            gold.0,
            gold.1,
            0.5,
            9,
            u(-64, -48),
            0.5,
            any,
        ),
        vein(
            "gold (badlands)",
            gold.0,
            gold.1,
            50.0,
            9,
            u(32, 255),
            0.0,
            Place::Biomes(&[Biome::Badlands]),
        ),
        vein(
            "redstone",
            redstone.0,
            redstone.1,
            4.0,
            8,
            u(-64, 15),
            0.0,
            any,
        ),
        vein(
            "redstone (lower)",
            redstone.0,
            redstone.1,
            8.0,
            8,
            t(-96, -32),
            0.0,
            any,
        ),
        vein(
            "diamond",
            diamond.0,
            diamond.1,
            7.0,
            4,
            t(-144, 16),
            0.5,
            any,
        ),
        vein(
            "diamond (large)",
            diamond.0,
            diamond.1,
            1.0 / 9.0,
            12,
            t(-144, 16),
            0.7,
            any,
        ),
        vein(
            "diamond (buried)",
            diamond.0,
            diamond.1,
            4.0,
            8,
            t(-144, 16),
            1.0,
            any,
        ),
        vein("lapis", lapis.0, lapis.1, 2.0, 7, t(-32, 32), 0.0, any),
        vein(
            "lapis (buried)",
            lapis.0,
            lapis.1,
            4.0,
            7,
            u(-64, 64),
            1.0,
            any,
        ),
        vein(
            "copper",
            copper.0,
            copper.1,
            16.0,
            10,
            t(-16, 112),
            0.0,
            any,
        ),
        vein(
            "copper (dripstone)",
            copper.0,
            copper.1,
            16.0,
            20,
            t(-16, 112),
            0.0,
            Place::Cave(Biome::DripstoneCaves),
        ),
        vein(
            "emerald",
            emerald.0,
            emerald.1,
            100.0,
            3,
            t(-16, 480),
            0.0,
            Place::Biomes(MOUNTAINS),
        ),
    ]
}

const ORE_SALT: u64 = 0x04E5;

/// Whether ore may replace `block`: stone and the stones like it (modern
/// `stone_ore_replaceables`) or deepslate and tuff (`deepslate_ore_replaceables`).
fn host(block: BlockState) -> Option<bool> {
    match block.id() {
        blocks::STONE | blocks::GRANITE | blocks::DIORITE | blocks::ANDESITE => Some(false),
        blocks::DEEPSLATE | blocks::TUFF => Some(true),
        _ => None,
    }
}

/// Adds veins from this chunk and its neighbors to `chunk`.
static KINDS: LazyLock<Vec<VeinKind>> = LazyLock::new(kinds);

pub fn populate(terrain: &Terrain, area: &Area, chunk: &mut Chunk) {
    let center = chunk.pos;
    let kinds = &*KINDS;
    for dz in -1..=1 {
        for dx in -1..=1 {
            let source = center.offset(dx, dz);
            let mut rng = Rng::for_chunk(terrain.seed(), source.x, source.z, ORE_SALT);
            for kind in kinds {
                let whole = kind.count.floor() as u32;
                let extra = (rng.next_f64() as f32) < kind.count.fract();
                for _ in 0..whole + extra as u32 {
                    // Each vein draws from its own numbers, so skipping one
                    // that can't reach this chunk changes no other.
                    let mut vein_rng = Rng::new(rng.next_u64());
                    vein(&mut vein_rng, source, kind, area, chunk);
                }
            }
        }
    }
}

/// One vein, following modern `OreFeature`: a line of overlapping
/// ellipsoids whose radius swells in the middle.
fn vein(rng: &mut Rng, source: ChunkPos, kind: &VeinKind, area: &Area, chunk: &mut Chunk) {
    let (min_x, min_z) = source.min_block();
    let x = min_x + rng.below(CHUNK_SIZE as u32) as i32;
    let z = min_z + rng.below(CHUNK_SIZE as u32) as i32;
    let y = kind.spread.sample(rng);
    if !(0..WORLD_HEIGHT as i32).contains(&y) {
        return;
    }
    let size = kind.size as f64;
    // A vein reaches at most this far from its center: an eighth of its
    // size along its line, plus the widest ellipsoid's half.
    let reach = size * 3.0 / 16.0 + 1.0;
    let (chunk_x, chunk_z) = chunk.pos.min_block();
    let (cx, cz) = (chunk_x as f64, chunk_z as f64);
    let (fx, fz) = (x as f64 + 0.5, z as f64 + 0.5);
    if fx + reach < cx || fx - reach > cx + 16.0 || fz + reach < cz || fz - reach > cz + 16.0 {
        return;
    }
    // The biggest blobs can start past the area's margin; only veins that
    // may go anywhere are that big.
    if let Some(column) = area.try_get(x, z) {
        // Nothing to replace high above the ground (the land around can
        // rise a little higher than the column the vein starts over).
        if y as f64 - reach > (column.height + 8) as f64 {
            return;
        }
        match kind.place {
            Place::Anywhere => {}
            Place::Biomes(biomes) if biomes.contains(&column.biome) => {}
            Place::Cave(biome) if column.cave == Some(biome) => {}
            _ => return,
        }
    } else if kind.place != Place::Anywhere {
        return;
    }
    let angle = rng.next_f64() * PI;
    let (sin, cos) = angle.sin_cos();
    let (x0, x1) = (fx + sin * size / 8.0, fx - sin * size / 8.0);
    let (z0, z1) = (fz + cos * size / 8.0, fz - cos * size / 8.0);
    let y0 = y as f64 + rng.below(3) as f64 - 1.0;
    let y1 = y as f64 + rng.below(3) as f64 - 1.0;
    // Air exposure is rolled per block from its position, so the vein comes
    // out the same in every chunk it crosses.
    let discard_seed = rng.next_u64();

    for step in 0..kind.size {
        let t = step as f64 / size;
        let (px, py, pz) = (x0 + (x1 - x0) * t, y0 + (y1 - y0) * t, z0 + (z1 - z0) * t);
        let spread = rng.next_f64() * size / 16.0;
        let radius = ((PI * t).sin() + 1.0) * spread + 1.0;
        let half = radius / 2.0;
        // The ellipsoid's cells, clipped to this chunk.
        let x_range = ((px - half).floor() as i32).max(chunk_x)
            ..=((px + half).floor() as i32).min(chunk_x + 15);
        let y_range = ((py - half).floor() as i32).max(1)
            ..=((py + half).floor() as i32).min(WORLD_HEIGHT as i32 - 2);
        let z_range = ((pz - half).floor() as i32).max(chunk_z)
            ..=((pz + half).floor() as i32).min(chunk_z + 15);
        if x_range.is_empty() || z_range.is_empty() || y_range.is_empty() {
            continue;
        }
        let inv = 1.0 / half;
        for bx in x_range {
            let dx = (bx as f64 + 0.5 - px) * inv;
            for by in y_range.clone() {
                let dy = (by as f64 + 0.5 - py) * inv;
                let dxy = dx * dx + dy * dy;
                if dxy >= 1.0 {
                    continue;
                }
                for bz in z_range.clone() {
                    let dz = (bz as f64 + 0.5 - pz) * inv;
                    if dxy + dz * dz >= 1.0 {
                        continue;
                    }
                    let (lx, ly, lz) = (
                        (bx - chunk_x) as usize,
                        by as usize,
                        (bz - chunk_z) as usize,
                    );
                    let here = chunk.get(lx, ly, lz);
                    let Some(deep) = host(here) else {
                        continue;
                    };
                    let block = if deep { kind.deep } else { kind.block };
                    if block == here {
                        continue;
                    }
                    if kind.air_discard > 0.0
                        && touches_air(chunk, lx, ly, lz)
                        && (Rng::for_chunk(discard_seed, bx, bz, by as u64).next_f64() as f32)
                            < kind.air_discard
                    {
                        continue;
                    }
                    chunk.set(lx, ly, lz, block);
                }
            }
        }
    }
}

/// Whether a block in the chunk has air (or water or lava: a cave) beside
/// it. Cells past the chunk's edge count as solid.
fn touches_air(chunk: &Chunk, x: usize, y: usize, z: usize) -> bool {
    let open = |b: BlockState| b.is_air() || b.def().fluid.is_some();
    let (x, y, z) = (x as i32, y as i32, z as i32);
    [
        (1, 0, 0),
        (-1, 0, 0),
        (0, 1, 0),
        (0, -1, 0),
        (0, 0, 1),
        (0, 0, -1),
    ]
    .iter()
    .any(|&(dx, dy, dz)| {
        let (nx, ny, nz) = (x + dx, y + dy, z + dz);
        (0..16).contains(&nx)
            && (0..16).contains(&nz)
            && (0..WORLD_HEIGHT as i32).contains(&ny)
            && open(chunk.get(nx as usize, ny as usize, nz as usize))
    })
}

/// Whether `block` is stone or deepslate, which ores and blobs replace.
pub fn is_stone(block: BlockState) -> bool {
    block == STONE || block == DEEPSLATE
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn heights_squeeze_below_sea_level_and_keep_above() {
        assert_eq!(ours(-64), 0);
        assert_eq!(ours(0), 32);
        assert_eq!(ours(16), 40);
        assert_eq!(ours(62), 62);
        assert_eq!(ours(63), 63);
        assert_eq!(ours(232), 232);
    }

    #[test]
    fn spreads_stay_in_range_and_triangles_peak_in_the_middle() {
        let mut rng = Rng::new(5);
        let spread = Spread::triangle(-32, 32);
        let mut counts = HashMap::new();
        for _ in 0..20_000 {
            let y = spread.sample(&mut rng);
            let (low, high) = spread.range();
            assert!((low..=high).contains(&y));
            *counts.entry(y).or_insert(0) += 1;
        }
        let at = |y| counts.get(&y).copied().unwrap_or(0);
        // Lapis: our 16..48, most at 32.
        assert!(at(32) > at(20) && at(32) > at(44));
        assert!(at(32) > 3 * at(18));
    }

    /// Counts of each block by height over a square of chunks, plus the
    /// biome of each ore block's column.
    fn survey(seed: u64, radius: i32) -> (HashMap<BlockState, Vec<usize>>, Vec<Biome>) {
        let terrain = Terrain::new(seed);
        let mut counts: HashMap<BlockState, Vec<usize>> = HashMap::new();
        let mut emerald_biomes = Vec::new();
        for cz in -radius..radius {
            for cx in -radius..radius {
                let chunk = terrain.generate(ChunkPos::new(cx, cz));
                for y in 0..WORLD_HEIGHT {
                    for z in 0..16 {
                        for x in 0..16 {
                            let block = chunk.get(x, y, z);
                            counts.entry(block).or_insert_with(|| vec![0; WORLD_HEIGHT])[y] += 1;
                            if matches!(
                                block.id(),
                                blocks::EMERALD_ORE | blocks::DEEPSLATE_EMERALD_ORE
                            ) {
                                emerald_biomes.push(chunk.biome(x, z));
                            }
                        }
                    }
                }
            }
        }
        (counts, emerald_biomes)
    }

    #[test]
    fn ores_follow_the_modern_table() {
        // Seed 15 has hills and mountains round the origin, where coal and
        // emeralds show.
        let (counts, emerald_biomes) = survey(15, 4);
        let total =
            |ids: &[crate::world::block::BlockId], range: std::ops::Range<usize>| -> usize {
                ids.iter()
                    .filter_map(|&id| counts.get(&of(id)))
                    .map(|by_y| by_y[range.clone()].iter().sum::<usize>())
                    .sum()
            };
        let diamond = [blocks::DIAMOND_ORE, blocks::DEEPSLATE_DIAMOND_ORE];
        let redstone = [blocks::REDSTONE_ORE, blocks::DEEPSLATE_REDSTONE_ORE];
        let lapis = [blocks::LAPIS_ORE, blocks::DEEPSLATE_LAPIS_ORE];
        let gold = [blocks::GOLD_ORE, blocks::DEEPSLATE_GOLD_ORE];
        let coal = [blocks::COAL_ORE, blocks::DEEPSLATE_COAL_ORE];
        let copper = [blocks::COPPER_ORE, blocks::DEEPSLATE_COPPER_ORE];
        let iron = [blocks::IRON_ORE, blocks::DEEPSLATE_IRON_ORE];
        let all = 0..WORLD_HEIGHT;
        // Diamonds sit near the bottom: none above our 42 (modern 16, plus a
        // vein's reach), thinning upward (the triangle peaks at our 0, so
        // three quarters of it lies below 20 before bedrock and caves).
        assert_eq!(total(&diamond, 43..WORLD_HEIGHT), 0);
        let (low, mid, high) = (
            total(&diamond, 0..14),
            total(&diamond, 14..28),
            total(&diamond, 28..43),
        );
        assert!(
            low > mid && mid > high,
            "diamonds by height: {low} {mid} {high}"
        );
        assert!(total(&diamond, all.clone()) > 0);
        // Redstone too, below our 42.
        assert_eq!(total(&redstone, 43..WORLD_HEIGHT), 0);
        // Lapis peaks around our 32 (modern 0).
        assert!(total(&lapis, 24..40) > total(&lapis, 44..66));
        // Gold below our 52, except in badlands.
        assert!(total(&gold, 0..52) > 0);
        // Coal is common high up, absent from the deepest layers.
        assert!(total(&coal, 64..WORLD_HEIGHT) > total(&coal, 0..40));
        assert!(total(&coal, 0..30) == 0);
        // Copper is mid-depth (our 24..112).
        assert!(total(&copper, 40..80) > 0);
        assert_eq!(total(&copper, 0..20), 0);
        // Iron is common deep down.
        assert!(total(&iron, 20..64) > 0);
        // Deepslate forms only below our 36, stone forms only above 31.
        for id in [blocks::DEEPSLATE_DIAMOND_ORE, blocks::DEEPSLATE_IRON_ORE] {
            if let Some(by_y) = counts.get(&of(id)) {
                assert_eq!(by_y[40..].iter().sum::<usize>(), 0, "{}", id.name());
            }
        }
        assert_eq!(total(&[blocks::DIAMOND_ORE], 0..28), 0);
        // Emeralds only in mountain biomes.
        for biome in &emerald_biomes {
            assert!(MOUNTAINS.contains(biome), "emerald in {biome:?}");
        }
    }

    #[test]
    fn exposed_diamonds_are_rarer_than_buried_ones() {
        // Air exposure discards: count diamonds with air beside them.
        let terrain = Terrain::new(8);
        let (mut exposed, mut buried) = (0, 0);
        for cz in -4..4 {
            for cx in -4..4 {
                let chunk = terrain.generate(ChunkPos::new(cx, cz));
                for y in 1..60 {
                    for z in 1..15 {
                        for x in 1..15 {
                            let id = chunk.get(x, y, z).id();
                            if matches!(id, blocks::DIAMOND_ORE | blocks::DEEPSLATE_DIAMOND_ORE) {
                                if touches_air(&chunk, x, y, z) {
                                    exposed += 1;
                                } else {
                                    buried += 1;
                                }
                            }
                        }
                    }
                }
            }
        }
        assert!(buried > 0);
        assert!(exposed * 4 < buried, "{exposed} exposed, {buried} buried");
    }

    #[test]
    fn veins_continue_across_chunk_borders() {
        // Generating a chunk alone or after its neighbor gives the same
        // blocks, so a vein from a neighbor shows up either way.
        let terrain = Terrain::new(4);
        let alone = terrain.generate(ChunkPos::new(1, 1));
        let _neighbor = terrain.generate(ChunkPos::new(0, 1));
        let again = terrain.generate(ChunkPos::new(1, 1));
        for y in 0..WORLD_HEIGHT {
            for z in 0..16 {
                for x in 0..16 {
                    assert_eq!(alone.get(x, y, z), again.get(x, y, z));
                }
            }
        }
    }
}

#[cfg(test)]
mod timing {
    use super::*;
    use std::time::Instant;

    #[test]
    #[ignore]
    fn kinds_cost() {
        let terrain = Terrain::new(20111118);
        let all = kinds();
        let mut cost = vec![0.0f64; all.len()];
        let n = 200;
        for i in 0..n {
            let pos = ChunkPos::new(i % 20, i / 20 + 3);
            let area = terrain.area(pos);
            let mut chunk = Chunk::new(pos);
            terrain.fill(&mut chunk);
            for dz in -1..=1 {
                for dx in -1..=1 {
                    let source = pos.offset(dx, dz);
                    let mut rng = Rng::for_chunk(terrain.seed(), source.x, source.z, ORE_SALT);
                    for (k, kind) in all.iter().enumerate() {
                        let whole = kind.count.floor() as u32;
                        let extra = (rng.next_f64() as f32) < kind.count.fract();
                        let s = Instant::now();
                        for _ in 0..whole + extra as u32 {
                            let mut vein_rng = Rng::new(rng.next_u64());
                            vein(&mut vein_rng, source, kind, &area, &mut chunk);
                        }
                        cost[k] += s.elapsed().as_secs_f64();
                    }
                }
            }
        }
        for (kind, c) in all.iter().zip(cost) {
            println!("{:20} {:.3} ms", kind.name, c * 1000.0 / n as f64);
        }
    }
}
