//! The biome map debug tool (`--biome-map`): surface biomes over a square
//! of the world, one pixel per 4×4 column (quart), with statistics to judge
//! the layout by: each biome's share, its patch sizes, and snowy biomes
//! touching hot ones.

use std::collections::BTreeMap;

use image::{Rgba, RgbaImage};

use super::{biome_source, climate::ClimateNoise};
use crate::world::biome::Biome;

/// Surface biomes on the quart grid of a square centered on the origin.
pub struct BiomeMap {
    /// Quarts across (and down).
    pub side: usize,
    pub biomes: Vec<Biome>,
}

impl BiomeMap {
    /// The biomes of `side` × `side` quarts (4 blocks each) centered on the
    /// origin, sampled on every core.
    pub fn sample(seed: u64, side: usize) -> Self {
        let noise = ClimateNoise::new(seed);
        let half = (side / 2) as i32;
        let mut biomes = vec![Biome::Ocean; side * side];
        let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
        let rows_each = side.div_ceil(threads);
        std::thread::scope(|scope| {
            for (n, chunk) in biomes.chunks_mut(rows_each * side).enumerate() {
                let noise = &noise;
                scope.spawn(move || {
                    for (k, out) in chunk.iter_mut().enumerate() {
                        let (i, j) = ((k % side) as i32, (n * rows_each + k / side) as i32);
                        *out = biome_source::surface(&noise.sample((i - half) * 4, (j - half) * 4));
                    }
                });
            }
        });
        Self { side, biomes }
    }

    fn at(&self, i: usize, j: usize) -> Biome {
        self.biomes[j * self.side + i]
    }
}

/// Snowy or frozen biomes, which shouldn't touch `hot` ones.
pub fn snowy(biome: Biome) -> bool {
    matches!(
        biome,
        Biome::SnowyPlains
            | Biome::SnowyTaiga
            | Biome::IceSpikes
            | Biome::FrozenOcean
            | Biome::DeepFrozenOcean
    )
}

pub fn hot(biome: Biome) -> bool {
    matches!(
        biome,
        Biome::Desert | Biome::Badlands | Biome::Savanna | Biome::Jungle | Biome::BambooJungle
    )
}

/// Oceans, by name.
pub fn ocean(biome: Biome) -> bool {
    biome.name().contains("Ocean")
}

/// Biomes that are lines by nature (rivers, shores), left out of the
/// patch-size check.
pub fn linear(biome: Biome) -> bool {
    matches!(biome, Biome::River | Biome::Beach | Biome::StonyShore)
}

#[derive(Debug, Clone, Default)]
pub struct BiomeStats {
    /// Share of the area, 0–1.
    pub share: BTreeMap<&'static str, f64>,
    /// Mean connected patch size in blocks², and the patch size the median
    /// block of the biome lies in (area-weighted median).
    pub mean_patch: BTreeMap<&'static str, f64>,
    pub median_patch: BTreeMap<&'static str, f64>,
    /// Pairs of side-by-side quarts that are snowy and hot.
    pub bad_neighbors: usize,
    pub ocean_share: f64,
    /// The area-weighted median patch size over every land biome that
    /// isn't a line, in blocks².
    pub land_median_patch: f64,
}

pub fn stats(map: &BiomeMap) -> BiomeStats {
    let side = map.side;
    let total = (side * side) as f64;
    let mut stats = BiomeStats::default();
    let mut bad = 0;
    for j in 0..side {
        for i in 0..side {
            let here = map.at(i, j);
            *stats.share.entry(here.name()).or_default() += 1.0 / total;
            for (ni, nj) in [(i + 1, j), (i, j + 1)] {
                if ni < side && nj < side {
                    let there = map.at(ni, nj);
                    if (snowy(here) && hot(there)) || (hot(here) && snowy(there)) {
                        bad += 1;
                    }
                }
            }
        }
    }
    stats.bad_neighbors = bad;
    stats.ocean_share = map.biomes.iter().filter(|b| ocean(**b)).count() as f64 / total;
    // Connected patches (4-way), by flood fill.
    let mut seen = vec![false; side * side];
    let mut patches: BTreeMap<&'static str, Vec<usize>> = BTreeMap::new();
    let mut land: Vec<usize> = Vec::new();
    let mut stack = Vec::new();
    for start in 0..side * side {
        if seen[start] {
            continue;
        }
        let biome = map.biomes[start];
        seen[start] = true;
        stack.push(start);
        let mut size = 0;
        while let Some(k) = stack.pop() {
            size += 1;
            let (i, j) = (k % side, k / side);
            let mut visit = |ni: usize, nj: usize| {
                let n = nj * side + ni;
                if !seen[n] && map.biomes[n] == biome {
                    seen[n] = true;
                    stack.push(n);
                }
            };
            if i > 0 {
                visit(i - 1, j);
            }
            if i + 1 < side {
                visit(i + 1, j);
            }
            if j > 0 {
                visit(i, j - 1);
            }
            if j + 1 < side {
                visit(i, j + 1);
            }
        }
        patches.entry(biome.name()).or_default().push(size);
        if !ocean(biome) && !linear(biome) {
            land.push(size);
        }
    }
    let weighted_median = |sizes: &mut Vec<usize>| -> f64 {
        sizes.sort_unstable();
        let total: usize = sizes.iter().sum();
        let mut acc = 0;
        for &s in sizes.iter() {
            acc += s;
            if acc * 2 >= total {
                return s as f64 * 16.0;
            }
        }
        0.0
    };
    for (name, sizes) in &mut patches {
        let mean = sizes.iter().sum::<usize>() as f64 / sizes.len() as f64 * 16.0;
        stats.mean_patch.insert(name, mean);
        stats.median_patch.insert(name, weighted_median(sizes));
    }
    stats.land_median_patch = weighted_median(&mut land);
    stats
}

/// A fixed color for each biome on the map.
pub fn color(biome: Biome) -> [u8; 3] {
    use Biome::*;
    match biome {
        Ocean => [0, 0, 112],
        DeepOcean => [0, 0, 48],
        FrozenOcean => [112, 112, 214],
        DeepFrozenOcean => [64, 64, 144],
        ColdOcean => [32, 32, 112],
        DeepColdOcean => [32, 32, 56],
        LukewarmOcean => [0, 0, 144],
        DeepLukewarmOcean => [0, 0, 64],
        WarmOcean => [0, 0, 172],
        Plains => [141, 179, 96],
        Forest => [5, 102, 33],
        Desert => [250, 148, 24],
        Beach => [250, 222, 85],
        StonyShore => [162, 162, 132],
        River => [0, 0, 255],
        Taiga => [11, 102, 89],
        SnowyPlains => [255, 255, 255],
        SnowyTaiga => [49, 85, 74],
        IceSpikes => [180, 220, 220],
        Jungle => [83, 123, 9],
        BambooJungle => [118, 142, 20],
        Swamp => [7, 249, 178],
        MangroveSwamp => [44, 204, 142],
        Savanna => [189, 178, 95],
        Badlands => [217, 69, 21],
        BirchForest => [48, 116, 68],
        DarkForest => [64, 81, 26],
        WindsweptHills => [96, 96, 96],
        Meadow => [131, 187, 109],
        Grove => [71, 114, 108],
        SnowySlopes => [196, 196, 196],
        JaggedPeaks => [220, 220, 200],
        FrozenPeaks => [176, 179, 206],
        CherryGrove => [255, 145, 200],
        MushroomFields => [255, 0, 255],
        LushCaves => [40, 200, 40],
        DripstoneCaves => [120, 90, 60],
        NetherWastes => [191, 59, 59],
        CrimsonForest => [221, 8, 8],
        WarpedForest => [73, 144, 123],
        SoulSandValley => [94, 56, 48],
        BasaltDeltas => [64, 54, 54],
        TheEnd => [128, 122, 140],
    }
}

/// The map as an image, with a legend of the biomes on it down the
/// right-hand side (a swatch per biome, in the order of their share).
pub fn render(map: &BiomeMap, stats: &BiomeStats) -> RgbaImage {
    const LEGEND: u32 = 24;
    let side = map.side as u32;
    let mut on_map: Vec<(&str, f64)> = stats.share.iter().map(|(n, s)| (*n, *s)).collect();
    on_map.sort_by(|a, b| b.1.total_cmp(&a.1));
    let height = side.max(on_map.len() as u32 * LEGEND);
    let mut image = RgbaImage::from_pixel(side + LEGEND * 2, height, Rgba([20, 20, 20, 255]));
    for j in 0..side {
        for i in 0..side {
            let [r, g, b] = color(map.at(i as usize, j as usize));
            image.put_pixel(i, j, Rgba([r, g, b, 255]));
        }
    }
    for (row, (name, _)) in on_map.iter().enumerate() {
        let Some(biome) = Biome::ALL.into_iter().find(|b| b.name() == *name) else {
            continue;
        };
        let [r, g, b] = color(biome);
        for y in row as u32 * LEGEND + 4..(row as u32 + 1) * LEGEND - 4 {
            for x in side + 8..side + LEGEND * 2 - 8 {
                image.put_pixel(x, y, Rgba([r, g, b, 255]));
            }
        }
    }
    image
}

/// The text summary the tool prints.
pub fn summary(stats: &BiomeStats) -> String {
    let mut rows: Vec<_> = stats.share.iter().collect();
    rows.sort_by(|a, b| b.1.total_cmp(a.1));
    let mut text =
        String::from("biome                  share   mean patch   median patch (blocks²)\n");
    for (name, share) in rows {
        text += &format!(
            "{name:<22} {:>5.1}%   {:>10.0}   {:>12.0}\n",
            share * 100.0,
            stats.mean_patch[name],
            stats.median_patch[name]
        );
    }
    let missing: Vec<&str> = Biome::ALL
        .into_iter()
        .filter(|b| !matches!(b, Biome::LushCaves | Biome::DripstoneCaves))
        .map(Biome::name)
        .filter(|n| !stats.share.contains_key(n))
        .collect();
    text += &format!("missing: {missing:?}\n");
    text += &format!(
        "oceans {:.1}%, land median patch {:.0} blocks² ({:.0}² blocks), snowy beside hot: {}\n",
        stats.ocean_share * 100.0,
        stats.land_median_patch,
        stats.land_median_patch.sqrt(),
        stats.bad_neighbors
    );
    text
}

/// Surface heights and biomes on the quart grid, for the height map.
pub struct HeightMap {
    pub side: usize,
    pub heights: Vec<i32>,
    pub biomes: Vec<Biome>,
}

impl HeightMap {
    /// `side` × `side` quarts centered on the origin, on every core.
    pub fn sample(seed: u64, side: usize) -> Self {
        let terrain = super::terrain::Terrain::new(seed);
        let half = (side / 2) as i32;
        let mut cells = vec![(0, Biome::Ocean); side * side];
        let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
        let rows_each = side.div_ceil(threads);
        std::thread::scope(|scope| {
            for (n, chunk) in cells.chunks_mut(rows_each * side).enumerate() {
                let terrain = &terrain;
                scope.spawn(move || {
                    for (k, out) in chunk.iter_mut().enumerate() {
                        let (i, j) = ((k % side) as i32, (n * rows_each + k / side) as i32);
                        *out = terrain.corner_surface(i - half, j - half);
                    }
                });
            }
        });
        Self {
            side,
            heights: cells.iter().map(|c| c.0).collect(),
            biomes: cells.iter().map(|c| c.1).collect(),
        }
    }

    /// Min, mean and max surface Y per biome, most common first.
    pub fn stats(&self) -> Vec<(&'static str, i32, f64, i32, usize)> {
        let mut by: BTreeMap<&'static str, (i32, i64, i32, usize)> = BTreeMap::new();
        for (&h, b) in self.heights.iter().zip(&self.biomes) {
            let e = by.entry(b.name()).or_insert((i32::MAX, 0, i32::MIN, 0));
            e.0 = e.0.min(h);
            e.1 += i64::from(h);
            e.2 = e.2.max(h);
            e.3 += 1;
        }
        let mut rows: Vec<_> = by
            .into_iter()
            .map(|(n, (lo, sum, hi, count))| (n, lo, sum as f64 / count as f64, hi, count))
            .collect();
        rows.sort_by_key(|row| std::cmp::Reverse(row.4));
        rows
    }

    /// Biome colors shaded by the slope (light from the north-west), with
    /// water darkened by its depth.
    pub fn render(&self) -> RgbaImage {
        let side = self.side as u32;
        let at = |i: i64, j: i64| {
            let (i, j) = (i.clamp(0, side as i64 - 1), j.clamp(0, side as i64 - 1));
            self.heights[j as usize * self.side + i as usize]
        };
        RgbaImage::from_fn(side, side, |i, j| {
            let (i64i, i64j) = (i as i64, j as i64);
            let h = at(i64i, i64j);
            let dx = f64::from(at(i64i + 1, i64j) - at(i64i - 1, i64j)) / 8.0;
            let dz = f64::from(at(i64i, i64j + 1) - at(i64i, i64j - 1)) / 8.0;
            let shade = (1.0 - 0.6 * (dx + dz)).clamp(0.35, 1.5);
            let [r, g, b] = color(self.biomes[j as usize * self.side + i as usize]);
            let depth = if h < crate::config::SEA_LEVEL {
                1.0 - (f64::from(crate::config::SEA_LEVEL - h) / 40.0).min(0.75)
            } else {
                1.0
            };
            let k = |c: u8| (f64::from(c) * shade * depth).clamp(0.0, 255.0) as u8;
            Rgba([k(r), k(g), k(b), 255])
        })
    }
}

/// The height map's printed table.
pub fn height_summary(rows: &[(&'static str, i32, f64, i32, usize)]) -> String {
    let mut text = String::from("biome                  min Y   mean Y   max Y\n");
    for (name, lo, mean, hi, _) in rows {
        text += &format!("{name:<22} {lo:>5}   {mean:>6.1}   {hi:>5}\n");
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_seed_gives_the_same_grid_and_another_seed_another() {
        let a = BiomeMap::sample(42, 256);
        let b = BiomeMap::sample(42, 256);
        let c = BiomeMap::sample(43, 256);
        assert_eq!(a.biomes, b.biomes);
        assert_ne!(a.biomes, c.biomes);
    }

    #[test]
    fn a_4096_quart_square_has_every_biome_in_sane_places() {
        // 4096 × 4096 quarts: 16384 blocks across.
        let map = BiomeMap::sample(20111118, 4096);
        let stats = stats(&map);
        eprintln!("{}", summary(&stats));
        assert_eq!(stats.bad_neighbors, 0, "snowy beside hot");
        for biome in Biome::ALL {
            if !biome.is_cave() && !biome.is_nether() && !biome.is_end() {
                assert!(stats.share.contains_key(biome.name()), "no {biome:?}");
            }
        }
        assert!(
            (0.25..=0.45).contains(&stats.ocean_share),
            "oceans {}",
            stats.ocean_share
        );
        assert!(
            stats.land_median_patch >= 128.0 * 128.0,
            "confetti: {}",
            stats.land_median_patch
        );
    }
}
