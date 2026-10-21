//! Numbers and pictures for judging the Nether's generator: the biome map
//! (`--nether-map`), patch sizes, and what a sample of chunks holds.

use std::collections::HashMap;

use super::*;
use crate::world::chunk::Chunk;

/// Biomes in a fixed order, each with its map color.
pub const BIOMES: [(Biome, [u8; 3]); 5] = [
    (Biome::NetherWastes, [140, 60, 60]),
    (Biome::CrimsonForest, [200, 30, 50]),
    (Biome::WarpedForest, [30, 170, 160]),
    (Biome::SoulSandValley, [150, 120, 80]),
    (Biome::BasaltDeltas, [70, 70, 90]),
];

fn index(biome: Biome) -> usize {
    BIOMES.iter().position(|&(b, _)| b == biome).unwrap_or(0)
}

/// The biomes of a square of quart columns around the origin (the same at
/// any y: the Nether's biomes are columns).
pub struct BiomeMap {
    pub size: usize,
    cells: Vec<u8>,
}

impl BiomeMap {
    /// `size` quart columns a side (4 × `size` blocks).
    pub fn sample(seed: u64, size: usize) -> Self {
        let nether = Nether::new(seed);
        let half = size as i32 / 2;
        let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
        let mut cells = vec![0u8; size * size];
        let band = size.div_ceil(threads) * size;
        std::thread::scope(|scope| {
            for (t, part) in cells.chunks_mut(band).enumerate() {
                let nether = &nether;
                scope.spawn(move || {
                    for (i, cell) in part.iter_mut().enumerate() {
                        let at = t * band + i;
                        let (qx, qz) = ((at % size) as i32, (at / size) as i32);
                        *cell = index(nether.biome((qx - half) * 4, (qz - half) * 4)) as u8;
                    }
                });
            }
        });
        Self { size, cells }
    }

    pub fn biome_at(&self, qx: usize, qz: usize) -> Biome {
        BIOMES[self.cells[qz * self.size + qx] as usize].0
    }
}

/// Share of the map and the patches (connected runs of one biome, by
/// quart columns) of each biome, areas in blocks².
pub struct BiomeStats {
    pub share: [f64; 5],
    pub patches: [Vec<usize>; 5],
}

impl BiomeStats {
    pub fn median_patch(&self, biome: Biome) -> usize {
        let mut areas = self.patches[index(biome)].clone();
        areas.sort_unstable();
        areas.get(areas.len() / 2).copied().unwrap_or(0)
    }

    /// The patch a typical block of this biome lies in: the size at which
    /// half of its area is in patches this big or bigger.
    pub fn typical_patch(&self, biome: Biome) -> usize {
        let mut areas = self.patches[index(biome)].clone();
        areas.sort_unstable_by(|a, b| b.cmp(a));
        let half = areas.iter().sum::<usize>() / 2;
        let mut sum = 0;
        for area in areas {
            sum += area;
            if sum >= half {
                return area;
            }
        }
        0
    }

    /// The share of all land in patches under `area` blocks².
    pub fn small_area_share(&self, area: usize) -> f64 {
        let all: usize = self.patches.iter().flatten().sum();
        let small: usize = self.patches.iter().flatten().filter(|&&a| a < area).sum();
        small as f64 / all.max(1) as f64
    }

    /// Patches under `area` blocks², as a share of all patches.
    pub fn small_share(&self, area: usize) -> f64 {
        let all: usize = self.patches.iter().map(Vec::len).sum();
        let small: usize = self
            .patches
            .iter()
            .map(|p| p.iter().filter(|&&a| a < area).count())
            .sum();
        small as f64 / all.max(1) as f64
    }
}

pub fn stats(map: &BiomeMap) -> BiomeStats {
    let n = map.size;
    let mut seen = vec![false; n * n];
    let mut patches: [Vec<usize>; 5] = Default::default();
    let mut count = [0usize; 5];
    let mut stack = Vec::new();
    for start in 0..n * n {
        count[map.cells[start] as usize] += 1;
        if seen[start] {
            continue;
        }
        let kind = map.cells[start];
        let mut area = 0;
        seen[start] = true;
        stack.push(start);
        while let Some(i) = stack.pop() {
            area += 1;
            let (x, z) = (i % n, i / n);
            let mut visit = |j: usize| {
                if !seen[j] && map.cells[j] == kind {
                    seen[j] = true;
                    stack.push(j);
                }
            };
            if x > 0 {
                visit(i - 1);
            }
            if x + 1 < n {
                visit(i + 1);
            }
            if z > 0 {
                visit(i - n);
            }
            if z + 1 < n {
                visit(i + n);
            }
        }
        patches[kind as usize].push(area * 16);
    }
    let share = count.map(|c| c as f64 / (n * n) as f64);
    BiomeStats { share, patches }
}

/// The map as a picture, one pixel per quart column, with a legend.
pub fn render(map: &BiomeMap) -> image::RgbaImage {
    let n = map.size as u32;
    let mut image = image::RgbaImage::new(n, n);
    for (i, pixel) in image.pixels_mut().enumerate() {
        let [r, g, b] = BIOMES[map.cells[i] as usize].1;
        *pixel = image::Rgba([r, g, b, 255]);
    }
    // Legend: a swatch per biome in the corner, in `BIOMES` order, on white.
    let swatch = (n / 64).max(6);
    for (k, &(_, [r, g, b])) in BIOMES.iter().enumerate() {
        let x0 = 4 + k as u32 * (swatch + 4);
        for y in 0..swatch + 8 {
            for x in 0..swatch + 4 {
                if x0 + x >= n || y >= n {
                    continue;
                }
                let inside = (2..swatch + 2).contains(&x) && (4..swatch + 4).contains(&y);
                let c = if inside { [r, g, b] } else { [255, 255, 255] };
                image.put_pixel(x0 + x - 2, y, image::Rgba([c[0], c[1], c[2], 255]));
            }
        }
    }
    image
}

/// What a square of chunks holds, judged from the finished chunks.
#[derive(Default)]
pub struct SampleStats {
    pub chunks: usize,
    /// Floor-to-ceiling heights of every open run between rock (or lava)
    /// and rock, above the lava sea.
    pub caverns: Vec<u8>,
    /// Vertical runs of basalt the columns / the pillars placed.
    pub columns: Vec<u8>,
    pub pillars: Vec<u8>,
    /// Lowest y a pillar reaches.
    pub lowest_pillar: i32,
    /// The highest block that is neither air nor bedrock.
    pub highest: i32,
}

/// Chunks between sampled chunks.
pub const STRIDE: i32 = 8;

fn median(values: &[u8]) -> u8 {
    let mut v = values.to_vec();
    v.sort_unstable();
    v.get(v.len() / 2).copied().unwrap_or(0)
}

impl SampleStats {
    pub fn median_cavern(&self) -> u8 {
        median(&self.caverns)
    }
    pub fn median_pillar(&self) -> u8 {
        median(&self.pillars)
    }
    pub fn median_column(&self) -> u8 {
        median(&self.columns)
    }

    fn merge(&mut self, other: Self) {
        self.chunks += other.chunks;
        self.caverns.extend(other.caverns);
        self.columns.extend(other.columns);
        self.pillars.extend(other.pillars);
        self.lowest_pillar = self.lowest_pillar.min(other.lowest_pillar);
        self.highest = self.highest.max(other.highest);
    }
}

fn is_basalt(block: BlockState) -> bool {
    block.is(blocks::BASALT)
}

/// Runs of new basalt (in `after`, not in `before`) in each column.
fn basalt_runs(before: &Chunk, after: &Chunk, mut run: impl FnMut(usize, usize, i32, u8)) {
    for lz in 0..CHUNK_SIZE {
        for lx in 0..CHUNK_SIZE {
            let mut length = 0u8;
            for y in 0..=HEIGHT as usize {
                let new = y < HEIGHT as usize
                    && is_basalt(after.get(lx, y, lz))
                    && !is_basalt(before.get(lx, y, lz));
                if new {
                    length += 1;
                } else if length > 0 {
                    run(lx, lz, y as i32 - i32::from(length), length);
                    length = 0;
                }
            }
        }
    }
}

/// Samples a grid of chunks, `[-radius, radius)` a side and `STRIDE`
/// chunks apart, around the origin on all cores. The spacing reaches all
/// five biomes, whose patches are hundreds of chunks across.
pub fn sample_chunks(nether: &Nether, radius: i32) -> SampleStats {
    let rows: Vec<i32> = (-radius..radius).collect();
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
    let parts: Vec<SampleStats> = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..threads)
            .map(|t| {
                let rows = &rows;
                scope.spawn(move || {
                    let mut stats = SampleStats {
                        lowest_pillar: HEIGHT,
                        ..Default::default()
                    };
                    for &cz in rows.iter().skip(t).step_by(threads) {
                        for cx in -radius..radius {
                            stats.merge(sample_chunk(
                                nether,
                                ChunkPos::new(cx * STRIDE, cz * STRIDE),
                            ));
                        }
                    }
                    stats
                })
            })
            .collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });
    let mut all = SampleStats {
        lowest_pillar: HEIGHT,
        ..Default::default()
    };
    for part in parts {
        all.merge(part);
    }
    all
}

fn sample_chunk(nether: &Nether, pos: ChunkPos) -> SampleStats {
    let mut stats = SampleStats {
        chunks: 1,
        lowest_pillar: HEIGHT,
        ..Default::default()
    };
    let chunk = nether.generate(pos);
    let open = |b: BlockState| !b.def().solid && !b.is(blocks::LAVA);
    for lz in 0..CHUNK_SIZE {
        for lx in 0..CHUNK_SIZE {
            let mut run = 0u8;
            for y in 1..HEIGHT as usize - 1 {
                let block = chunk.get(lx, y, lz);
                if !block.is(blocks::BEDROCK) && block != AIR {
                    stats.highest = stats.highest.max(y as i32);
                }
                if y as i32 > LAVA_LEVEL && open(block) {
                    run = run.saturating_add(1);
                } else {
                    // Rock above a run of air, with rock or lava under it.
                    if run > 0 && !open(block) && y > run as usize {
                        let floor = chunk.get(lx, y - run as usize - 1, lz);
                        if !open(floor) {
                            stats.caverns.push(run);
                        }
                    }
                    run = 0;
                }
            }
        }
    }
    let terrain = nether.generate_with(pos, &[], false);
    let columns = nether.generate_with(pos, &COLUMN_SALTS, false);
    basalt_runs(&terrain, &columns, |_, _, _, length| {
        stats.columns.push(length)
    });
    let pillars = nether.generate_with(pos, &[PILLAR_SALT], false);
    // A pillar is a run no longer than one beside it: the skirts of blocks
    // along its ends are shorter runs in the next columns.
    let mut runs = HashMap::new();
    basalt_runs(&terrain, &pillars, |lx, lz, y, length| {
        runs.insert((lx as i32, lz as i32, y), length);
    });
    for (&(lx, lz, y), &length) in &runs {
        let beaten = runs.iter().any(|(&(ox, oz, _), &other)| {
            (ox - lx).abs() + (oz - lz).abs() == 1 && other >= length
        });
        if !beaten {
            stats.pillars.push(length);
            stats.lowest_pillar = stats.lowest_pillar.min(y);
        }
    }
    stats
}

/// The printed summary of a biome map.
pub fn summary(stats: &BiomeStats) -> String {
    let mut text = String::new();
    for &(biome, _) in &BIOMES {
        let k = index(biome);
        text += &format!(
            "{:<16} {:5.1}%  {:6} patches, median {:>9} blocks², typical (by area) {:>9} blocks²\n",
            biome.name(),
            stats.share[k] * 100.0,
            stats.patches[k].len(),
            stats.median_patch(biome),
            stats.typical_patch(biome),
        );
    }
    text += &format!(
        "patches under 32 × 32 blocks: {:.1}% of patches, {:.2}% of the area\n",
        stats.small_share(32 * 32) * 100.0,
        stats.small_area_share(32 * 32) * 100.0
    );
    text
}

/// The printed summary of a chunk sample.
pub fn sample_summary(s: &SampleStats) -> String {
    let max = |v: &[u8]| v.iter().copied().max().unwrap_or(0);
    format!(
        "{} chunks sampled\ncavern height: median {}, max {}\ntallest basalt column {} (median {}), tallest basalt pillar {} (median {}, lowest y {})\nhighest non-air, non-bedrock y: {}\n",
        s.chunks,
        s.median_cavern(),
        max(&s.caverns),
        max(&s.columns),
        s.median_column(),
        max(&s.pillars),
        s.median_pillar(),
        s.lowest_pillar,
        s.highest,
    )
}

/// Everything the map tool prints for `seed`.
pub fn report(seed: u64, size: usize, chunk_radius: i32) -> (image::RgbaImage, String) {
    let map = BiomeMap::sample(seed, size);
    let stats = stats(&map);
    let mut text = summary(&stats);
    text += &sample_summary(&sample_chunks(&Nether::new(seed), chunk_radius));
    (render(&map), text)
}

/// Biome share by name, for tests.
pub fn shares(stats: &BiomeStats) -> HashMap<Biome, f64> {
    BIOMES
        .iter()
        .map(|&(b, _)| (b, stats.share[index(b)]))
        .collect()
}
