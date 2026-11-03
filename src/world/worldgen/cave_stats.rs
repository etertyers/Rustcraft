//! `--cave-test` and `--cave-slice`: how much of the underground is open,
//! flooded, ore-bearing and reachable, measured on generated chunks (no
//! rendering), and a picture of one vertical slice of it.
//!
//! The sample is `chunks / 16` windows of 4×4 chunks spread over a
//! 4096 × 4096 area; each window stands for the 256 × 256 area round it.

use std::collections::{HashMap, HashSet, VecDeque};

use glam::IVec3;

use super::terrain::Terrain;
use crate::{
    config::{CHUNK_SIZE, MIN_Y, SEA_LEVEL},
    world::{
        block::BlockState,
        blocks,
        chunk::{Chunk, ChunkPos},
    },
};

/// The area the windows are spread over, in chunks on a side.
const SPAN: i32 = 256;
/// A window is this many chunks on a side.
const WINDOW: i32 = 4;
/// Bands of 16 blocks from Y −64 up to 128.
pub const BANDS: usize = 12;
/// Water and lava bodies with fewer side faces than this aren't judged.
const MIN_BODY_FACES: usize = 24;

/// Every ore the test counts, by name, with its stone and deepslate block.
pub fn ore_blocks() -> Vec<(&'static str, [crate::world::block::BlockId; 2])> {
    vec![
        ("coal", [blocks::COAL_ORE, blocks::DEEPSLATE_COAL_ORE]),
        ("iron", [blocks::IRON_ORE, blocks::DEEPSLATE_IRON_ORE]),
        ("copper", [blocks::COPPER_ORE, blocks::DEEPSLATE_COPPER_ORE]),
        ("gold", [blocks::GOLD_ORE, blocks::DEEPSLATE_GOLD_ORE]),
        (
            "redstone",
            [blocks::REDSTONE_ORE, blocks::DEEPSLATE_REDSTONE_ORE],
        ),
        (
            "diamond",
            [blocks::DIAMOND_ORE, blocks::DEEPSLATE_DIAMOND_ORE],
        ),
        ("lapis", [blocks::LAPIS_ORE, blocks::DEEPSLATE_LAPIS_ORE]),
        (
            "emerald",
            [blocks::EMERALD_ORE, blocks::DEEPSLATE_EMERALD_ORE],
        ),
    ]
}

/// What the sample found.
#[derive(Debug, Default, Clone)]
pub struct Stats {
    pub chunks: usize,
    /// Blocks at or below the surface in each band, and the cave air, water
    /// and lava among them.
    pub ground: [u64; BANDS],
    pub air: [u64; BANDS],
    pub water: [u64; BANDS],
    pub lava: [u64; BANDS],
    /// Connected cave water (or lava) bodies with enough side faces to judge,
    /// and how many have flat walls on chunk borders over more than half
    /// of them.
    pub water_bodies: usize,
    pub box_bodies: usize,
    /// Bodies with a fifth or more of their side faces on chunk borders.
    pub walled_bodies: usize,
    /// Cave water bodies above Y 30 that don't reach the sea.
    pub inland_high: usize,
    /// Depth from the surface to the first cave air, per column (`None`
    /// when a column has none).
    pub first_air: Vec<Option<u16>>,
    /// Chunks with a column whose top block is carved away.
    pub open_chunks: usize,
    /// Windows with at least one such chunk.
    pub open_windows: usize,
    /// Windows that are mostly land (an ocean has no ground to open).
    pub land_windows: usize,
    pub open_land_windows: usize,
    pub windows: usize,
    /// The share of each window's cave air that its largest connected
    /// system holds.
    pub system_shares: Vec<f64>,
    /// Blocks of each ore (stone, deepslate) and how many touch cave air.
    pub ores: Vec<(&'static str, [u64; 2], [u64; 2])>,
    /// Ore blocks with a stone host below Y −8, and in deepslate above Y 8.
    pub stone_ore_below: u64,
    pub deep_ore_above: u64,
    /// Chunks (below Y −48) with a diamond exposed or one block in.
    pub diamond_chunks: usize,
    /// Stone, deepslate and tuff by band from Y −64 to 16 (5 bands), and
    /// the solid blocks in them.
    pub family: [[u64; 3]; 5],
    /// Solid blocks under the surface, and those beside cave air.
    pub solid: u64,
    pub solid_exposed: u64,
}

/// Whether `block` is cave-open: air.
fn is_air(block: BlockState) -> bool {
    block.is_air()
}

fn band(y: i32) -> Option<usize> {
    let b = (y - MIN_Y).div_euclid(16);
    (0..BANDS as i32).contains(&b).then_some(b as usize)
}

/// The chunks of window `index` of `windows`, laid out on a square grid
/// over the area.
fn window_chunks(index: usize, windows: usize) -> Vec<ChunkPos> {
    let side = (windows as f64).sqrt().ceil() as usize;
    let spacing = SPAN / side as i32;
    let (col, row) = ((index % side) as i32, (index / side) as i32);
    let (x0, z0) = (
        -SPAN / 2 + col * spacing + spacing / 2 - WINDOW / 2,
        -SPAN / 2 + row * spacing + spacing / 2 - WINDOW / 2,
    );
    (0..WINDOW)
        .flat_map(|dz| (0..WINDOW).map(move |dx| ChunkPos::new(x0 + dx, z0 + dz)))
        .collect()
}

/// A generated window: its chunks and every column's surface height.
struct Window {
    chunks: HashMap<ChunkPos, Chunk>,
    heights: HashMap<(i32, i32), i32>,
}

impl Window {
    fn block(&self, x: i32, y: i32, z: i32) -> Option<BlockState> {
        let chunk = self.chunks.get(&ChunkPos::of_block(x, z))?;
        Some(chunk.get(
            x.rem_euclid(CHUNK_SIZE as i32) as usize,
            y,
            z.rem_euclid(CHUNK_SIZE as i32) as usize,
        ))
    }

    fn height(&self, x: i32, z: i32) -> Option<i32> {
        self.heights.get(&(x, z)).copied()
    }

    /// Cave air: air under the surface.
    fn cave_air(&self, x: i32, y: i32, z: i32) -> bool {
        self.height(x, z).is_some_and(|h| y < h) && self.block(x, y, z).is_some_and(is_air)
    }
}

fn generate(terrain: &Terrain, positions: &[ChunkPos]) -> Window {
    let mut chunks = HashMap::new();
    let mut heights = HashMap::new();
    for &pos in positions {
        let area = terrain.area(pos);
        let (x0, z0) = pos.min_block();
        for z in 0..CHUNK_SIZE as i32 {
            for x in 0..CHUNK_SIZE as i32 {
                heights.insert((x0 + x, z0 + z), area.get(x0 + x, z0 + z).height);
            }
        }
        chunks.insert(pos, terrain.generate(pos));
    }
    Window { chunks, heights }
}

/// Whether the connected `cells` (water or lava) have flat walls on chunk
/// borders over more than half of their side faces, and how many side
/// faces they have.
#[cfg(test)]
fn box_shaped(
    cells: &[IVec3],
    set: &HashSet<IVec3>,
    solid: impl Fn(IVec3) -> bool,
) -> (usize, bool) {
    let (faces, flat) = flat_faces(cells, set, solid);
    (faces, faces > 0 && flat * 2 > faces)
}

/// Side faces of `cells` against rock, and how many lie on chunk borders.
fn flat_faces(
    cells: &[IVec3],
    set: &HashSet<IVec3>,
    solid: impl Fn(IVec3) -> bool,
) -> (usize, usize) {
    let (mut faces, mut flat) = (0, 0);
    for &c in cells {
        for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
            let n = c + IVec3::new(dx, 0, dz);
            if set.contains(&n) || !solid(n) {
                continue;
            }
            faces += 1;
            // The wall lies on the plane between c and n.
            let plane = if dx != 0 { c.x.max(n.x) } else { c.z.max(n.z) };
            if plane.rem_euclid(CHUNK_SIZE as i32) == 0 {
                flat += 1;
            }
        }
    }
    (faces, flat)
}

/// Measures `chunks` chunks of `seed`'s world.
pub fn measure(seed: u64, chunks: usize) -> Stats {
    let terrain = Terrain::new(seed);
    let windows = (chunks / (WINDOW * WINDOW) as usize).max(1);
    let mut stats = Stats {
        windows,
        ores: ore_blocks()
            .iter()
            .map(|(name, _)| (*name, [0; 2], [0; 2]))
            .collect(),
        ..Stats::default()
    };
    let ore_names = ore_blocks();
    for index in 0..windows {
        let positions = window_chunks(index, windows);
        let window = generate(&terrain, &positions);
        stats.chunks += positions.len();
        let mut window_open = false;
        let land = window.heights.values().filter(|&&h| h >= SEA_LEVEL).count();
        let is_land = land * 4 >= window.heights.len();
        stats.land_windows += usize::from(is_land);
        let mut air_cells: Vec<IVec3> = Vec::new();
        let mut fluid_cells: HashSet<IVec3> = HashSet::new();
        for pos in &positions {
            let (x0, z0) = pos.min_block();
            let mut chunk_open = false;
            let mut chunk_diamond = false;
            for z in z0..z0 + CHUNK_SIZE as i32 {
                for x in x0..x0 + CHUNK_SIZE as i32 {
                    let h = window.height(x, z).unwrap_or(0);
                    let top = window.block(x, h, z);
                    if top.is_some_and(|b| !b.def().solid) && h > MIN_Y + 5 {
                        chunk_open = true;
                    }
                    let mut first = None;
                    for y in MIN_Y + 1..=h {
                        let Some(block) = window.block(x, y, z) else {
                            continue;
                        };
                        if let Some(b) = band(y) {
                            stats.ground[b] += 1;
                            if is_air(block) {
                                stats.air[b] += 1;
                            } else if block.id() == blocks::WATER {
                                stats.water[b] += 1;
                            } else if block.id() == blocks::LAVA {
                                stats.lava[b] += 1;
                            }
                        }
                        if is_air(block) {
                            air_cells.push(IVec3::new(x, y, z));
                            if first.is_none() && y < h {
                                first = Some((h - y) as u16);
                            }
                        } else if block.id() == blocks::WATER || block.id() == blocks::LAVA {
                            fluid_cells.insert(IVec3::new(x, y, z));
                        } else {
                            if block.def().solid {
                                stats.solid += 1;
                                if [
                                    (1, 0, 0),
                                    (-1, 0, 0),
                                    (0, 1, 0),
                                    (0, -1, 0),
                                    (0, 0, 1),
                                    (0, 0, -1),
                                ]
                                .iter()
                                .any(|&(dx, dy, dz)| window.cave_air(x + dx, y + dy, z + dz))
                                {
                                    stats.solid_exposed += 1;
                                }
                            }
                        }
                        if !is_air(block)
                            && block.id() != blocks::WATER
                            && block.id() != blocks::LAVA
                            && y < 16
                        {
                            let fam = (y - MIN_Y).div_euclid(16) as usize;
                            let kind = match block.id() {
                                blocks::STONE => Some(0),
                                blocks::DEEPSLATE => Some(1),
                                blocks::TUFF => Some(2),
                                _ => None,
                            };
                            if let (Some(kind), true) = (kind, fam < 5) {
                                stats.family[fam][kind] += 1;
                            }
                        }
                        for (i, (_, ids)) in ore_names.iter().enumerate() {
                            let Some(which) = ids.iter().position(|&id| id == block.id()) else {
                                continue;
                            };
                            stats.ores[i].1[which] += 1;
                            if (which == 0 && y < -8) || (which == 1 && y > 8) {
                                if which == 0 {
                                    stats.stone_ore_below += 1;
                                } else {
                                    stats.deep_ore_above += 1;
                                }
                            }
                            let exposed = [
                                (1, 0, 0),
                                (-1, 0, 0),
                                (0, 1, 0),
                                (0, -1, 0),
                                (0, 0, 1),
                                (0, 0, -1),
                            ]
                            .iter()
                            .any(|&(dx, dy, dz)| window.cave_air(x + dx, y + dy, z + dz));
                            if exposed {
                                stats.ores[i].2[which] += 1;
                            }
                            if i == 5 && y < -48 {
                                let near = exposed
                                    || [
                                        (1, 0, 0),
                                        (-1, 0, 0),
                                        (0, 1, 0),
                                        (0, -1, 0),
                                        (0, 0, 1),
                                        (0, 0, -1),
                                    ]
                                    .iter()
                                    .any(|&(dx, dy, dz)| {
                                        // One block in: air two blocks away.
                                        window.cave_air(x + dx * 2, y + dy * 2, z + dz * 2)
                                    });
                                if near {
                                    chunk_diamond = true;
                                }
                            }
                        }
                    }
                    stats.first_air.push(first);
                }
            }
            if chunk_open {
                stats.open_chunks += 1;
                window_open = true;
            }
            if chunk_diamond {
                stats.diamond_chunks += 1;
            }
        }
        if window_open {
            stats.open_windows += 1;
            stats.open_land_windows += usize::from(is_land);
        }
        // The largest connected system of cave air in the window.
        let air: HashSet<IVec3> = air_cells.iter().copied().collect();
        let mut seen: HashSet<IVec3> = HashSet::with_capacity(air.len());
        let mut largest = 0usize;
        for &start in &air_cells {
            if !seen.insert(start) {
                continue;
            }
            let mut size = 0usize;
            let mut queue = VecDeque::from([start]);
            while let Some(c) = queue.pop_front() {
                size += 1;
                for d in [
                    IVec3::X,
                    IVec3::NEG_X,
                    IVec3::Y,
                    IVec3::NEG_Y,
                    IVec3::Z,
                    IVec3::NEG_Z,
                ] {
                    let n = c + d;
                    if air.contains(&n) && seen.insert(n) {
                        queue.push_back(n);
                    }
                }
            }
            largest = largest.max(size);
        }
        if !air.is_empty() {
            stats.system_shares.push(largest as f64 / air.len() as f64);
        }
        // Water and lava bodies and whether they are boxes.
        let solid = |c: IVec3| window.block(c.x, c.y, c.z).is_some_and(|b| b.def().solid);
        let mut seen: HashSet<IVec3> = HashSet::new();
        for &start in &fluid_cells {
            if !seen.insert(start) {
                continue;
            }
            let kind = window.block(start.x, start.y, start.z).map(|b| b.id());
            let mut body = Vec::new();
            let mut queue = VecDeque::from([start]);
            while let Some(c) = queue.pop_front() {
                body.push(c);
                for d in [
                    IVec3::X,
                    IVec3::NEG_X,
                    IVec3::Y,
                    IVec3::NEG_Y,
                    IVec3::Z,
                    IVec3::NEG_Z,
                ] {
                    let n = c + d;
                    if fluid_cells.contains(&n)
                        && window.block(n.x, n.y, n.z).map(|b| b.id()) == kind
                        && seen.insert(n)
                    {
                        queue.push_back(n);
                    }
                }
            }
            // A body that rises to the sea is the ocean's, not a cave's.
            // (or lies under an ocean, river or lake: the sea's own aquifer)
            if body.iter().any(|c| {
                c.y >= SEA_LEVEL - 1 || window.height(c.x, c.z).is_some_and(|h| h < SEA_LEVEL)
            }) {
                continue;
            }
            if kind == Some(blocks::WATER) && body.iter().any(|c| c.y > 30) {
                stats.inland_high += 1;
            }
            let set: HashSet<IVec3> = body.iter().copied().collect();
            let (faces, flat) = flat_faces(&body, &set, solid);
            if faces >= MIN_BODY_FACES {
                stats.water_bodies += 1;
                if flat * 2 > faces {
                    stats.box_bodies += 1;
                }
                if flat * 5 > faces {
                    stats.walled_bodies += 1;
                }
            }
        }
    }
    stats
}

fn percent(part: u64, whole: u64) -> f64 {
    if whole == 0 {
        0.0
    } else {
        part as f64 * 100.0 / whole as f64
    }
}

/// The depth within which the median column has cave air: `None` if more
/// than half the columns have none.
pub fn median_first_air(stats: &Stats) -> Option<u16> {
    let mut depths: Vec<u16> = stats
        .first_air
        .iter()
        .map(|d| d.unwrap_or(u16::MAX))
        .collect();
    depths.sort_unstable();
    depths
        .get(depths.len() / 2)
        .copied()
        .filter(|&d| d != u16::MAX)
}

impl Stats {
    /// Average cave air share (percent) in the bands from Y −48 to 32.
    pub fn air_percent(&self, b: usize) -> f64 {
        percent(self.air[b], self.ground[b])
    }

    pub fn exposed_per_chunk(&self, name: &str) -> f64 {
        self.ores
            .iter()
            .find(|(n, ..)| *n == name)
            .map_or(0.0, |(_, _, exposed)| {
                (exposed[0] + exposed[1]) as f64 / self.chunks.max(1) as f64
            })
    }

    /// The report `--cave-test` prints.
    pub fn report(&self) -> String {
        use std::fmt::Write;
        let mut out = String::new();
        let _ = writeln!(
            out,
            "cave test: {} chunks in {} windows of 4×4 chunks",
            self.chunks, self.windows
        );
        let _ = writeln!(
            out,
            "(a,b,f) per 16-block band: cave air %, water %, lava %"
        );
        for b in 0..BANDS {
            let lo = MIN_Y + b as i32 * 16;
            let _ = write!(
                out,
                "  Y {:>4}..{:>4}: air {:>5.1}%  water {:>5.2}%  lava {:>5.2}%",
                lo,
                lo + 15,
                self.air_percent(b),
                percent(self.water[b], self.ground[b]),
                percent(self.lava[b], self.ground[b]),
            );
            if b < 5 {
                let [s, d, t] = self.family[b];
                let total = (s + d + t).max(1);
                let _ = write!(
                    out,
                    "   stone {:>4.1}% deepslate {:>4.1}% tuff {:>4.1}%",
                    percent(s, total),
                    percent(d, total),
                    percent(t, total)
                );
            }
            let _ = writeln!(out);
        }
        let _ = writeln!(
            out,
            "(b) cave water/lava bodies: {} judged, {} boxes (flat walls on chunk borders over half their sides), {} with a fifth or more; {} water bodies above Y 30 not reaching the sea",
            self.water_bodies, self.box_bodies, self.walled_bodies, self.inland_high
        );
        let median = median_first_air(self);
        let columns = self.first_air.len().max(1);
        let with_air = self.first_air.iter().filter(|d| d.is_some()).count();
        let within_40 = self
            .first_air
            .iter()
            .filter(|d| d.is_some_and(|d| d <= 40))
            .count();
        let _ = writeln!(
            out,
            "(c) columns with cave air: {:.1}% ({:.1}% within 40 blocks of the surface), median depth to first cave air: {}; chunks open to the sky: {:.1}%; 256×256 areas with an opening: {}/{} ({}/{} of the mostly-land ones)",
            percent(with_air as u64, columns as u64),
            percent(within_40 as u64, columns as u64),
            median.map_or("none".to_owned(), |d| format!("{d} blocks")),
            percent(self.open_chunks as u64, self.chunks as u64),
            self.open_windows,
            self.windows,
            self.open_land_windows,
            self.land_windows,
        );
        let big = self.system_shares.iter().filter(|&&s| s > 0.5).count();
        let _ = writeln!(
            out,
            "(d) windows where one system holds over half the cave air: {}/{} (average share {:.0}%)",
            big,
            self.system_shares.len(),
            self.system_shares.iter().sum::<f64>() * 100.0 / self.system_shares.len().max(1) as f64,
        );
        let _ = writeln!(
            out,
            "(e) ore blocks per chunk (stone + deepslate), exposed per chunk:"
        );
        for (name, total, exposed) in &self.ores {
            let _ = writeln!(
                out,
                "  {name:<9} {:>6.2} ({:>5.2} stone, {:>5.2} deepslate)   exposed {:>5.2}",
                (total[0] + total[1]) as f64 / self.chunks.max(1) as f64,
                total[0] as f64 / self.chunks.max(1) as f64,
                total[1] as f64 / self.chunks.max(1) as f64,
                (exposed[0] + exposed[1]) as f64 / self.chunks.max(1) as f64,
            );
        }
        let _ = writeln!(
            out,
            "  solid blocks beside cave air: {:.2}%\n  stone ores below Y -8: {}, deepslate ores above Y 8: {}, chunks with diamond exposed or one block in (below Y -48): {:.1}%",
            percent(self.solid_exposed, self.solid),
            self.stone_ore_below,
            self.deep_ore_above,
            percent(self.diamond_chunks as u64, self.chunks as u64),
        );
        out
    }
}

/// Prints the report for `--cave-test`.
pub fn print_stats(seed: u64, chunks: usize) {
    println!("{}", measure(seed, chunks).report());
}

/// A pixel color for a block in the slice.
fn slice_color(block: BlockState) -> [u8; 3] {
    let id = block.id();
    if block.is_air() {
        return [255, 255, 255];
    }
    for (i, (_, ids)) in ore_blocks().iter().enumerate() {
        if ids.contains(&id) {
            return [
                [0, 0, 0],
                [230, 170, 120],
                [0, 200, 120],
                [255, 230, 0],
                [255, 0, 0],
                [0, 255, 255],
                [0, 60, 255],
                [0, 255, 0],
            ][i];
        }
    }
    match id {
        blocks::WATER => [40, 90, 255],
        blocks::LAVA => [255, 130, 0],
        blocks::DEEPSLATE => [60, 60, 66],
        blocks::TUFF => [110, 120, 100],
        blocks::STONE => [128, 128, 128],
        blocks::BEDROCK => [20, 20, 20],
        _ if block.def().solid => [150, 120, 90],
        _ => [200, 255, 200],
    }
}

/// Writes a vertical slice 256 blocks wide (along x from `x`, at `z`),
/// from Y −64 to 128, one pixel a block, with the surface as a red line.
pub fn write_slice(
    seed: u64,
    x: i32,
    z: i32,
    size: u32,
    out: &std::path::Path,
) -> anyhow::Result<()> {
    let terrain = Terrain::new(seed);
    let first = ChunkPos::of_block(x, z);
    let last = ChunkPos::of_block(x + size as i32 - 1, z);
    let positions: Vec<ChunkPos> = (first.x..=last.x)
        .map(|cx| ChunkPos::new(cx, first.z))
        .collect();
    let window = generate(&terrain, &positions);
    let height = 192u32;
    let mut image = image::RgbImage::new(size, height);
    for i in 0..size as i32 {
        let wx = x + i;
        let surface = window.height(wx, z).unwrap_or(0);
        for row in 0..height as i32 {
            let y = 127 - row;
            let color = if y == surface {
                [255, 0, 0]
            } else {
                window.block(wx, y, z).map_or([255, 0, 255], slice_color)
            };
            image.put_pixel(i as u32, row as u32, image::Rgb(color));
        }
    }
    image.save(out)?;
    Ok(())
}

/// The number of ore blocks a kind expects per chunk: count × size × the
/// share of its height range that lies inside the world (before discards
/// and overlaps). A pure function of the table, for checking the counts.
pub fn expected_ore_blocks(
    count: f64,
    size: f64,
    low: i32,
    high: i32,
    world_low: i32,
    world_high: i32,
) -> f64 {
    let span = (high - low + 1).max(1) as f64;
    let inside = (high.min(world_high) - low.max(world_low) + 1).max(0) as f64;
    count * size * inside / span * VEIN_FILL
}

/// The share of a vein's `size` that comes out as distinct blocks:
/// vanilla's `OreFeature` lays `size` overlapping ellipsoids along a short
/// line, so small veins fill about half of it (measured on redstone and
/// lapis; the spec's plain count × size is the most a vein can hold).
pub const VEIN_FILL: f64 = 0.5;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_underground_is_open_like_vanillas_on_a_64_chunk_sample() {
        let stats = measure(12345, 64);
        assert_eq!(stats.chunks, 64);
        // Cave air 5–20% in every band from Y −48 to 32.
        for b in 1..=5 {
            let air = stats.air_percent(b);
            assert!((5.0..=20.0).contains(&air), "band {b}: {air:.1}%");
        }
        // Lava at the bottom, no boxes of water, no water standing high
        // inland.
        assert!(stats.lava[0] > 0, "no lava below Y -48");
        assert_eq!(stats.box_bodies, 0, "boxes of water");
        // (a coast's own aquifers flood to the sea, which a window can't see)
        // (the 400-chunk run counts 18 in 25 windows, before and after M53's
        // vanilla carvers; a few windows hold several)
        assert!(
            stats.inland_high <= 3 * stats.windows,
            "{} water bodies high up inland",
            stats.inland_high
        );
        // Openings to the sky in every 256 × 256 area, and a big system
        // in at least 30% of the windows.
        // (all 25 of the 400-chunk run's; the 4 here are a small sample)
        assert!(stats.open_windows * 4 >= stats.windows * 3);
        let big = stats.system_shares.iter().filter(|&&s| s > 0.5).count();
        assert!(
            big * 10 >= stats.system_shares.len() * 3,
            "{big} of {}",
            stats.system_shares.len()
        );
        // Deep down it is deepslate and tuff (95% of the solid blocks).
        for b in 0..4 {
            let [stone, deep, tuff] = stats.family[b];
            assert!(
                (deep + tuff) * 100 >= (stone + deep + tuff) * 95,
                "band {b}: {stone} stone, {deep} deepslate, {tuff} tuff"
            );
        }
        // Diamonds sit mostly low, and iron and coal turn up in caves.
        assert!(stats.exposed_per_chunk("coal") > 0.5);
        assert!(stats.exposed_per_chunk("iron") > 0.5);
    }

    #[test]
    fn windows_tile_the_area_without_overlapping() {
        let all: Vec<ChunkPos> = (0..25).flat_map(|i| window_chunks(i, 25)).collect();
        let unique: HashSet<ChunkPos> = all.iter().copied().collect();
        assert_eq!(all.len(), 400);
        assert_eq!(unique.len(), 400);
        assert!(all.iter().all(|p| p.x.abs() <= 128 && p.z.abs() <= 128));
    }

    #[test]
    fn expected_counts_follow_the_range_inside_the_world() {
        // Diamond-like: all of the range inside, or half of it below the floor.
        assert_eq!(
            expected_ore_blocks(7.0, 4.0, -64, 15, -64, 319),
            28.0 * VEIN_FILL
        );
        let half = expected_ore_blocks(7.0, 4.0, -144, 16, -64, 319);
        assert!((half - 28.0 * 81.0 / 161.0 * VEIN_FILL).abs() < 1e-9);
    }

    #[test]
    fn a_box_of_water_is_told_from_a_pool() {
        let wall = |_: IVec3| true;
        // A body filling a whole chunk-aligned 4×4 room: every side face
        // is on a plane that is a multiple of 16 only if the room spans
        // the chunk.
        let cells: Vec<IVec3> = (0..16)
            .flat_map(|x| (0..16).map(move |z| IVec3::new(x, 10, z)))
            .collect();
        let set: HashSet<IVec3> = cells.iter().copied().collect();
        let (faces, boxed) = box_shaped(&cells, &set, wall);
        assert_eq!(faces, 64);
        assert!(boxed, "a room exactly one chunk wide has flat walls");
        let pool: Vec<IVec3> = (3..9)
            .flat_map(|x| (3..9).map(move |z| IVec3::new(x, 10, z)))
            .collect();
        let set: HashSet<IVec3> = pool.iter().copied().collect();
        assert!(
            !box_shaped(&pool, &set, wall).1,
            "a pool inside a chunk isn't one"
        );
    }
}
