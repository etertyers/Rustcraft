//! `--cave-openings` (M53): how many openings the caves make in the
//! surface, per 1,000 × 1,000 blocks, and what makes them.
//!
//! An opening is a connected group (8 neighbors) of land columns whose top
//! block is carved away with air under it, counted once. Its source is the
//! cave part that is open most often at the group's top two blocks: the
//! entrance blob, the 3D spaghetti tunnel inside the entrances, the cheese
//! and layer caves breaking through, the 2D spaghetti, noodles, the cave
//! carver or the canyon carver.

use std::collections::{HashMap, HashSet, VecDeque};

use glam::IVec3;

use super::terrain::Terrain;
use crate::{
    config::{CHUNK_SIZE, MIN_Y, SEA_LEVEL},
    world::chunk::ChunkPos,
};

/// The things that can open the ground, in the order ties go to.
pub const SOURCES: [&str; 7] = [
    "entrance noise",
    "spaghetti_3d (in the entrances)",
    "cheese breaking through",
    "spaghetti_2d",
    "noodle",
    "cave carver",
    "canyon carver",
];

#[derive(Debug, Default)]
pub struct Openings {
    /// Land columns looked at, and those with their top carved.
    pub columns: u64,
    pub open_columns: u64,
    pub openings: u64,
    /// Openings by their main source (`SOURCES`), and how many each source
    /// has a hand in.
    pub by_source: [u64; 7],
    pub touched: [u64; 7],
    pub area: u64,
}

impl Openings {
    pub fn report(&self) -> String {
        let per = |n: u64| n as f64 * 1_000_000.0 / self.area as f64;
        let mut out = format!(
            "surface openings: {} in {} blocks² of land and sea = {:.1} per 1,000 × 1,000 blocks\n\
             open surface columns: {} of {} land columns = {:.3}%\n",
            self.openings,
            self.area,
            per(self.openings),
            self.open_columns,
            self.columns,
            self.open_columns as f64 * 100.0 / self.columns.max(1) as f64,
        );
        out.push_str(
            "by main source (per 1,000 × 1,000), and openings each source has a hand in:\n",
        );
        for (i, name) in SOURCES.iter().enumerate() {
            out.push_str(&format!(
                "  {name:<34} {:>7.1}   touches {:>7.1}\n",
                per(self.by_source[i]),
                per(self.touched[i]),
            ));
        }
        out
    }
}

/// A part of the caves counts toward an opening when it is open or nearly:
/// the density is blended between the corners of its cell, and
/// `5 × entrances` only has to fall under the land's density (a few tenths)
/// at a corner to pull the top block under zero.
const NEAR: f64 = 0.2;

/// How many of the eight corners of the 4 × 8 × 4 cell holding (x, h, z)
/// each cave part is open at (the density is blended between them), plus
/// the carvers at the top two blocks, per `SOURCES`.
fn claims(terrain: &Terrain, kinds: &HashMap<IVec3, u8>, x: i32, h: i32, z: i32) -> [u32; 7] {
    let mut counts = [0u32; 7];
    let (x0, z0) = (x.div_euclid(4) * 4, z.div_euclid(4) * 4);
    let y0 = MIN_Y + (h - MIN_Y).div_euclid(8) * 8;
    for dy in [0, 8] {
        for dz in [0, 4] {
            for dx in [0, 4] {
                let t = terrain.caves().terms(
                    f64::from(x0 + dx),
                    f64::from(y0 + dy),
                    f64::from(z0 + dz),
                );
                for i in 0..5 {
                    counts[i] += u32::from(t[i] <= NEAR);
                }
            }
        }
    }
    for y in [h, h - 1] {
        let worm = kinds.get(&IVec3::new(x, y, z)).copied().unwrap_or(0);
        counts[5] += u32::from(worm & 1 != 0) * 4;
        counts[6] += u32::from(worm & 2 != 0) * 4;
    }
    counts
}

/// Generates `side` × `side` chunks round 0, 0 and counts their openings.
pub fn measure(seed: u64, side: i32) -> Openings {
    let terrain = Terrain::new(seed);
    let first = -side / 2;
    let positions: Vec<ChunkPos> = (first..first + side)
        .flat_map(|z| (first..first + side).map(move |x| ChunkPos::new(x, z)))
        .collect();
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
    let per = positions.len().div_ceil(threads);
    // Each thread works out the open columns of its chunks: (x, h, z).
    let mut open: Vec<(i32, i32, i32)> = Vec::new();
    let mut columns = 0u64;
    std::thread::scope(|scope| {
        let handles: Vec<_> = positions
            .chunks(per)
            .map(|chunk_positions| {
                let terrain = &terrain;
                scope.spawn(move || {
                    let (mut found, mut land) = (Vec::new(), 0u64);
                    for &pos in chunk_positions {
                        let area = terrain.area(pos);
                        let chunk = terrain.generate(pos);
                        let (x0, z0) = pos.min_block();
                        for z in 0..CHUNK_SIZE {
                            for x in 0..CHUNK_SIZE {
                                let (wx, wz) = (x0 + x as i32, z0 + z as i32);
                                let h = area.get(wx, wz).height;
                                if h <= SEA_LEVEL || h <= MIN_Y + 5 {
                                    continue;
                                }
                                land += 1;
                                let top = chunk.get(x, h, z);
                                let below = chunk.get(x, h - 1, z);
                                // Air the caves made: the land alone would
                                // be solid there (terrain roughness isn't
                                // a cave).
                                if top.is_air()
                                    && below.is_air()
                                    && terrain.land_density_at(wx, h, wz) > 0.0
                                {
                                    found.push((wx, h, wz));
                                }
                            }
                        }
                    }
                    (found, land)
                })
            })
            .collect();
        for handle in handles {
            let (found, land) = handle.join().expect("a chunk thread");
            open.extend(found);
            columns += land;
        }
    });
    // Group the open columns (8 neighbors).
    let index: HashMap<(i32, i32), i32> = open.iter().map(|&(x, h, z)| ((x, z), h)).collect();
    let mut seen: HashSet<(i32, i32)> = HashSet::new();
    let mut groups: Vec<Vec<(i32, i32, i32)>> = Vec::new();
    for &(x, h, z) in &open {
        if !seen.insert((x, z)) {
            continue;
        }
        let mut group = vec![(x, h, z)];
        let mut queue = VecDeque::from([(x, z)]);
        while let Some((cx, cz)) = queue.pop_front() {
            for dz in -1..=1 {
                for dx in -1..=1 {
                    let n = (cx + dx, cz + dz);
                    if let Some(&nh) = index.get(&n)
                        && seen.insert(n)
                    {
                        group.push((n.0, nh, n.1));
                        queue.push_back(n);
                    }
                }
            }
        }
        groups.push(group);
    }
    // Which carvers run through each group's top cells.
    let mut cells: Vec<IVec3> = Vec::new();
    for group in &groups {
        for &(x, h, z) in group {
            cells.push(IVec3::new(x, h, z));
            cells.push(IVec3::new(x, h - 1, z));
        }
    }
    let kinds: HashMap<IVec3, u8> = cells
        .chunks(4096)
        .flat_map(|batch| {
            terrain
                .worm_kinds(batch)
                .into_iter()
                .zip(batch.iter().copied())
                .map(|(k, c)| (c, k))
                .collect::<Vec<_>>()
        })
        .collect();
    let mut stats = Openings {
        columns,
        open_columns: open.len() as u64,
        openings: groups.len() as u64,
        area: (side as u64 * CHUNK_SIZE as u64).pow(2),
        ..Openings::default()
    };
    for group in &groups {
        let mut total = [0u32; 7];
        for &(x, h, z) in group {
            let c = claims(&terrain, &kinds, x, h, z);
            for i in 0..7 {
                total[i] += c[i];
            }
        }
        let best = (0..7).max_by_key(|&i| (total[i], 7 - i)).unwrap_or(0);
        if total[best] == 0 && std::env::var_os("M53_DEBUG").is_some() {
            let (x, h, z) = group[0];
            let chunk = terrain.generate(ChunkPos::of_block(x, z));
            let at = |y: i32| {
                chunk
                    .get(x.rem_euclid(16) as usize, y, z.rem_euclid(16) as usize)
                    .def()
                    .name
                    .clone()
            };
            eprintln!(
                "UNEXPLAINED {x} {h} {z}: density {:.4} land {:.4} blocks {} {} {} {} (size {})",
                terrain.density_at(x, h, z),
                terrain.land_density_at(x, h, z),
                at(h + 1),
                at(h),
                at(h - 1),
                at(h - 2),
                group.len()
            );
        }
        if total[best] > 0 {
            stats.by_source[best] += 1;
        }
        for (touched, &claimed) in stats.touched.iter_mut().zip(&total) {
            *touched += u64::from(claimed > 0);
        }
    }
    stats
}

/// Prints the report for `--cave-openings`.
pub fn print_stats(seed: u64, side: i32) {
    println!("{}", measure(seed, side).report());
}

/// Times `terrain.area` and `terrain.generate` over `side` × `side` chunks
/// on one thread (`--cave-openings --time`).
pub fn time_generation(seed: u64, side: i32) {
    let terrain = Terrain::new(seed);
    let first = -side / 2;
    let (mut areas, mut chunks) = (std::time::Duration::ZERO, std::time::Duration::ZERO);
    for z in first..first + side {
        for x in first..first + side {
            let pos = ChunkPos::new(x, z);
            let t = std::time::Instant::now();
            let _ = terrain.area(pos);
            areas += t.elapsed();
            let t = std::time::Instant::now();
            let _ = terrain.generate(pos);
            chunks += t.elapsed();
        }
    }
    let n = f64::from(side * side);
    println!(
        "{} chunks on one thread: area {:.2} ms each, generate {:.2} ms each",
        side * side,
        areas.as_secs_f64() * 1000.0 / n,
        chunks.as_secs_f64() * 1000.0 / n
    );
}
