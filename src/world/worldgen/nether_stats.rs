//! `--nether-map`: how many basalt pillars and column clusters the Nether's
//! chunks get, by biome, and how tall they stand.

use std::collections::BTreeMap;

use super::nether::{Nether, Placement, log_placements, take_placements};
use crate::{
    config::CHUNK_SIZE,
    world::{biome::Biome, blocks, chunk::ChunkPos},
};

/// Placements and tall basalt in the Nether chunks of a square sample.
#[derive(Debug, Default)]
pub struct Stats {
    pub chunks: usize,
    /// Chunks whose middle column is each biome.
    pub chunks_in: BTreeMap<&'static str, usize>,
    pub placed: Vec<Placement>,
    /// Basalt runs at least this tall that stand free in the generated
    /// chunks, by biome of their column: (pillar-like, column-like).
    pub runs: BTreeMap<&'static str, (usize, usize)>,
}

/// Generates `side` × `side` chunks round the origin and counts.
pub fn measure(seed: u64, side: i32) -> Stats {
    let nether = Nether::new(seed);
    let mut stats = Stats::default();
    log_placements();
    for cz in -side / 2..side - side / 2 {
        for cx in -side / 2..side - side / 2 {
            let pos = ChunkPos::new(cx, cz);
            let chunk = nether.generate(pos);
            stats.chunks += 1;
            *stats
                .chunks_in
                .entry(chunk.biome_at(8, 64, 8).name())
                .or_default() += 1;
            for z in 0..CHUNK_SIZE {
                for x in 0..CHUNK_SIZE {
                    let mut y = 1;
                    while y < 127 {
                        if chunk.get(x, y, z).id() != blocks::BASALT {
                            y += 1;
                            continue;
                        }
                        let start = y;
                        while y < 127 && chunk.get(x, y, z).id() == blocks::BASALT {
                            y += 1;
                        }
                        let (low, high) = (chunk.get(x, start - 1, z), chunk.get(x, y, z));
                        // A run with solid rock above it that has air or
                        // lava below hangs: a pillar; one standing on rock
                        // or lava under air is a column.
                        let length = y - start;
                        let entry = stats
                            .runs
                            .entry(chunk.biome_at(x, y, z).name())
                            .or_default();
                        if length >= 13 && high.def().solid && !low.def().solid {
                            entry.0 += 1;
                        } else if (1..=12).contains(&length) && high.is_air() {
                            entry.1 += 1;
                        }
                    }
                }
            }
        }
    }
    stats.placed = take_placements();
    stats
}

fn median(values: &mut [i32]) -> i32 {
    values.sort_unstable();
    values.get(values.len() / 2).copied().unwrap_or(0)
}

impl Stats {
    pub fn report(&self) -> String {
        use std::fmt::Write;
        let mut out = String::new();
        let _ = writeln!(out, "nether map: {} chunks", self.chunks);
        let mut biomes: Vec<&str> = self.chunks_in.keys().copied().collect();
        for p in &self.placed {
            if !biomes.contains(&p.biome.name()) {
                biomes.push(p.biome.name());
            }
        }
        for name in biomes {
            let of = |pillar: bool| -> Vec<i32> {
                self.placed
                    .iter()
                    .filter(|p| p.pillar == pillar && p.biome.name() == name)
                    .map(|p| p.height)
                    .collect()
            };
            let (mut pillars, mut columns) = (of(true), of(false));
            let chunks = self.chunks_in.get(name).copied().unwrap_or(0).max(1) as f64;
            let (tall, low) = self.runs.get(name).copied().unwrap_or((0, 0));
            let _ = writeln!(
                out,
                "  {name:<14} chunks {:>4}  pillars {:>6.2}/chunk (median {:>3}, max {:>3} tall)  column clusters {:>6.2}/chunk (median {:>2}, max {:>2})  free basalt runs: {tall} tall, {low} short",
                self.chunks_in.get(name).copied().unwrap_or(0),
                pillars.len() as f64 / chunks,
                median(&mut pillars),
                pillars.iter().max().copied().unwrap_or(0),
                columns.len() as f64 / chunks,
                median(&mut columns),
                columns.iter().max().copied().unwrap_or(0),
            );
        }
        let pillars_out = self
            .placed
            .iter()
            .filter(|p| p.pillar && p.biome != Biome::SoulSandValley)
            .count();
        let columns_out = self
            .placed
            .iter()
            .filter(|p| !p.pillar && p.biome != Biome::BasaltDeltas)
            .count();
        let _ = writeln!(
            out,
            "  outside their biome: {pillars_out} pillars, {columns_out} column clusters"
        );
        out
    }
}

pub fn print_stats(seed: u64, side: i32) {
    println!("{}", measure(seed, side).report());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pillars_stand_in_soul_sand_valleys_and_columns_in_basalt_deltas_only() {
        let stats = measure(12345, 64);
        assert_eq!(stats.chunks, 64 * 64);
        let (mut pillars, mut columns) = (0, 0);
        let (mut valley_pillars, mut valley_chunks) = (0usize, 0usize);
        for p in &stats.placed {
            if p.pillar {
                pillars += 1;
                assert_eq!(p.biome, Biome::SoulSandValley, "a pillar in {:?}", p.biome);
                valley_pillars += 1;
            } else {
                columns += 1;
                assert_eq!(p.biome, Biome::BasaltDeltas, "a column in {:?}", p.biome);
                assert!(p.height <= 10, "a column {} tall", p.height);
            }
        }
        valley_chunks += stats
            .chunks_in
            .get("Soul Sand Valley")
            .copied()
            .unwrap_or(0);
        assert!(
            pillars > 0 && columns > 1000,
            "{pillars} pillars, {columns} clusters"
        );
        // At most ten pillars a valley chunk on average, none through lava.
        assert!(
            valley_pillars <= valley_chunks * 10,
            "{valley_pillars} in {valley_chunks}"
        );
        for p in stats.placed.iter().filter(|p| p.pillar) {
            assert!(
                p.bottom > crate::world::worldgen::nether::LAVA_LEVEL,
                "a pillar to {}",
                p.bottom
            );
        }
    }
}
