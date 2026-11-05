//! `--grass-test` (M67): how much bare dirt the surface leaves exposed, and
//! what exposed it.
//!
//! Exposed dirt is a `dirt` block with nothing above it but air, leaves and
//! plants (what the sky sees of it). Vanilla leaves almost none, because
//! its surface rules put grass on every floor near the surface and its
//! carvers turn the dirt under a carved grass block back into grass.
//! `examples/vanilla_dirt.rs` counts the same things in a world Mojang's
//! own server generated.
//!
//! Causes, from the chunk after each generation step:
//! - **noise cave**: exposed after the terrain step, with the land alone
//!   solid in the block above (a density cave or entrance took what was
//!   there);
//! - **surface rule**: exposed after the terrain step with the land open
//!   above it anyway (the rules chose dirt for the top);
//! - **carver**: exposed only after the worm carvers ran;
//! - **biome**: coarse dirt, podzol and rooted dirt, which vanilla puts on
//!   top in some biomes (not counted as dirt).

use std::collections::{HashMap, HashSet, VecDeque};

use super::terrain::Terrain;
use crate::{
    config::{CHUNK_SIZE, MAX_Y, MIN_Y},
    world::{blocks, chunk::Chunk, chunk::ChunkPos},
};

/// Whether the sky sees through a block for this count: air, leaves and
/// the plants that grow on the ground.
pub fn sees_through(name: &str) -> bool {
    let n = name.strip_prefix("minecraft:").unwrap_or(name);
    n.ends_with("air")
        || n.ends_with("leaves")
        || matches!(
            n,
            "short_grass"
                | "tall_grass"
                | "fern"
                | "large_fern"
                | "dead_bush"
                | "vine"
                | "snow"
                | "sweet_berry_bush"
                | "poppy"
                | "dandelion"
                | "azure_bluet"
                | "oxeye_daisy"
                | "cornflower"
                | "lily_of_the_valley"
                | "blue_orchid"
                | "allium"
                | "red_tulip"
                | "orange_tulip"
                | "white_tulip"
                | "pink_tulip"
                | "lilac"
                | "rose_bush"
                | "peony"
                | "sunflower"
                | "brown_mushroom"
                | "red_mushroom"
                | "bamboo"
                | "sugar_cane"
                | "cactus"
                | "moss_carpet"
                | "pale_moss_carpet"
                | "leaf_litter"
                | "bush"
                | "firefly_bush"
                | "wildflowers"
                | "pink_petals"
                | "cherry_leaves"
                | "azalea"
                | "flowering_azalea"
        )
}

/// What the sky sees first in a column of a generated chunk: the top block
/// that isn't see-through, with its Y.
fn top_of(chunk: &Chunk, x: usize, z: usize) -> Option<(i32, crate::world::block::BlockState)> {
    for y in (MIN_Y..MAX_Y).rev() {
        let block = chunk.get(x, y, z);
        if !sees_through(&block.def().name) {
            return Some((y, block));
        }
    }
    None
}

/// A pit: a column whose top is at least `PIT_DEPTH` blocks lower than each
/// of the four columns `PIT_REACH` blocks away along x and z. A stand-in
/// for "a cave opening or ravine bottom" that needs nothing but the
/// finished blocks, so the same count can be made in a world Mojang's
/// server generated. It is a statistic to compare, not a target.
pub const PIT_DEPTH: i32 = 5;
pub const PIT_REACH: i32 = 5;

/// The columns of `tops` (sky-facing top Y by column) that are pits.
pub fn pit_columns(tops: &HashMap<(i32, i32), i32>) -> HashSet<(i32, i32)> {
    tops.iter()
        .filter(|&(&(x, z), &y)| {
            [(1, 0), (-1, 0), (0, 1), (0, -1)].iter().all(|&(dx, dz)| {
                tops.get(&(x + dx * PIT_REACH, z + dz * PIT_REACH))
                    .is_some_and(|&n| n >= y + PIT_DEPTH)
            })
        })
        .map(|(&at, _)| at)
        .collect()
}

/// The counts for one area.
#[derive(Debug, Default, Clone)]
pub struct Dirt {
    pub area: u64,
    /// Columns whose sky-facing top block is exposed dirt, by cause.
    pub noise: HashSet<(i32, i32)>,
    pub surface_rule: HashSet<(i32, i32)>,
    pub carver: HashSet<(i32, i32)>,
    /// Exposed coarse dirt, podzol and rooted dirt.
    pub biome: HashSet<(i32, i32)>,
    /// Exposed dirt of the finished chunk (trees and plants in place), the
    /// number to put next to vanilla's.
    pub final_dirt: HashSet<(i32, i32)>,
    /// The sky-facing top Y of every column of the finished chunks.
    pub tops: HashMap<(i32, i32), i32>,
}

impl Dirt {
    pub fn total_blocks(&self) -> usize {
        self.noise.len() + self.surface_rule.len() + self.carver.len()
    }
}

/// Connected groups (8 neighbors) in a set of columns, as their sizes.
pub fn patches(columns: &HashSet<(i32, i32)>) -> Vec<usize> {
    let mut seen: HashSet<(i32, i32)> = HashSet::new();
    let mut sizes = Vec::new();
    for &start in columns {
        if !seen.insert(start) {
            continue;
        }
        let mut size = 1;
        let mut queue = VecDeque::from([start]);
        while let Some((x, z)) = queue.pop_front() {
            for dz in -1..=1 {
                for dx in -1..=1 {
                    let n = (x + dx, z + dz);
                    if columns.contains(&n) && seen.insert(n) {
                        size += 1;
                        queue.push_back(n);
                    }
                }
            }
        }
        sizes.push(size);
    }
    sizes
}

/// Measures `side` × `side` chunks round 0, 0.
pub fn measure(seed: u64, side: i32) -> Dirt {
    let terrain = Terrain::new(seed);
    let first = -side / 2;
    let positions: Vec<ChunkPos> = (first..first + side)
        .flat_map(|z| (first..first + side).map(move |x| ChunkPos::new(x, z)))
        .collect();
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
    let per = positions.len().div_ceil(threads);
    let mut total = Dirt {
        area: (side as u64 * CHUNK_SIZE as u64).pow(2),
        ..Dirt::default()
    };
    std::thread::scope(|scope| {
        let handles: Vec<_> = positions
            .chunks(per)
            .map(|chunk_positions| {
                let terrain = &terrain;
                scope.spawn(move || {
                    let mut part = Dirt::default();
                    for &pos in chunk_positions {
                        let terrain_only = terrain.generate_until(pos, false);
                        let carved = terrain.generate_until(pos, true);
                        let full = terrain.generate(pos);
                        let (x0, z0) = pos.min_block();
                        for z in 0..CHUNK_SIZE {
                            for x in 0..CHUNK_SIZE {
                                let at = (x0 + x as i32, z0 + z as i32);
                                let dirt = |chunk: &Chunk| {
                                    top_of(chunk, x, z).filter(|(_, b)| b.id() == blocks::DIRT)
                                };
                                if let Some((y, block)) = top_of(&full, x, z) {
                                    part.tops.insert(at, y);
                                    match block.id() {
                                        blocks::DIRT => {
                                            part.final_dirt.insert(at);
                                        }
                                        blocks::COARSE_DIRT
                                        | blocks::PODZOL
                                        | blocks::ROOTED_DIRT => {
                                            part.biome.insert(at);
                                        }
                                        _ => {}
                                    }
                                }
                                if let Some((y, _)) = dirt(&terrain_only) {
                                    // The land alone solid above it: a cave
                                    // took what covered it.
                                    if terrain.land_density_at(at.0, y + 1, at.1) > 0.0 {
                                        part.noise.insert(at);
                                    } else {
                                        part.surface_rule.insert(at);
                                    }
                                } else if let Some((y, _)) = dirt(&carved) {
                                    part.carver.insert(at);
                                    if std::env::var_os("M67_DEBUG").is_some() && part.carver.len() < 12 {
                                        let area = terrain.area(pos);
                                        let c = area.get(at.0, at.1);
                                        let name = |ch: &Chunk, yy: i32| ch.get(x, yy, z).def().name.clone();
                                        eprintln!("CARVER {at:?} y {y} height {} top {} biome {:?}: terrain-only {} {} {} | carved {} {} {}", c.height, c.top.def().name, c.biome, name(&terrain_only, y + 1), name(&terrain_only, y), name(&terrain_only, y - 1), name(&carved, y + 1), name(&carved, y), name(&carved, y - 1));
                                    }
                                }
                            }
                        }
                    }
                    part
                })
            })
            .collect();
        for handle in handles {
            let part = handle.join().expect("a chunk thread");
            total.noise.extend(part.noise);
            total.surface_rule.extend(part.surface_rule);
            total.carver.extend(part.carver);
            total.biome.extend(part.biome);
            total.final_dirt.extend(part.final_dirt);
            total.tops.extend(part.tops);
        }
    });
    total
}

impl Dirt {
    pub fn report(&self) -> String {
        let per = |n: usize| n as f64 * 1_000_000.0 / self.area as f64;
        let row = |name: &str, set: &HashSet<(i32, i32)>| {
            let sizes = patches(set);
            let biggest = sizes.iter().max().copied().unwrap_or(0);
            format!(
                "  {name:<44} {:>8.1} blocks  {:>7.1} patches  (largest {biggest})\n",
                per(set.len()),
                per(sizes.len()),
            )
        };
        let mut out = format!(
            "exposed dirt per 1,000 × 1,000 blocks ({} blocks² measured), by cause:\n",
            self.area
        );
        out.push_str(&row("a noise cave or entrance cut the grass", &self.noise));
        out.push_str(&row("a carver cut it", &self.carver));
        out.push_str(&row("a surface rule put dirt on top", &self.surface_rule));
        out.push_str(&row(
            "biome ground (coarse dirt, podzol, rooted)",
            &self.biome,
        ));
        out.push_str(&row(
            "dirt of the finished chunks (to compare)",
            &self.final_dirt,
        ));
        out.push_str(&pit_report(&self.tops, self.area));
        out
    }
}

/// The pit line of a report: columns, and groups of them, per km².
pub fn pit_report(tops: &HashMap<(i32, i32), i32>, area: u64) -> String {
    let pits = pit_columns(tops);
    let groups = patches(&pits).len();
    let per = |n: usize| n as f64 * 1_000_000.0 / area as f64;
    format!(
        "pits (top {PIT_DEPTH}+ blocks under all four columns {PIT_REACH} away): {:.1} columns, {:.1} groups per 1,000 × 1,000; {:.3}% of columns\n",
        per(pits.len()),
        per(groups),
        pits.len() as f64 * 100.0 / tops.len().max(1) as f64
    )
}

/// Prints `--grass-test`: the exposed dirt, then the M53 openings.
pub fn print_stats(seed: u64, side: i32) {
    println!("{}", measure(seed, side).report());
    println!("{}", super::opening_stats::measure(seed, side).report());
}
