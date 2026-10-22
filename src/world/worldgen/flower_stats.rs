//! How many flowers a biome's chunks grow (`--flower-test`, and the tests
//! that hold plains to vanilla's density).

use super::terrain::Terrain;
use crate::world::{biome::Biome, block::BlockId, blocks, chunk::ChunkPos};

/// One-block flowers.
const SMALL: [BlockId; 13] = [
    blocks::DANDELION,
    blocks::POPPY,
    blocks::AZURE_BLUET,
    blocks::OXEYE_DAISY,
    blocks::CORNFLOWER,
    blocks::RED_TULIP,
    blocks::ORANGE_TULIP,
    blocks::WHITE_TULIP,
    blocks::PINK_TULIP,
    blocks::ALLIUM,
    blocks::LILY_OF_THE_VALLEY,
    blocks::BLUE_ORCHID,
    blocks::PINK_PETALS,
];
/// Two-block flowers (each counts once).
const TALL: [BlockId; 4] = [
    blocks::LILAC,
    blocks::ROSE_BUSH,
    blocks::PEONY,
    blocks::SUNFLOWER,
];

/// The flowers in each of `count` chunks whose middle column is `biome`,
/// taken in a spiral from `near` outward.
pub fn flowers_per_chunk(terrain: &Terrain, biome: Biome, count: usize) -> Vec<u32> {
    let mut found = Vec::with_capacity(count);
    let mut ring = 0i32;
    while found.len() < count && ring < 400 {
        for i in -ring..=ring {
            for (x, z) in [(i, -ring), (i, ring), (-ring, i), (ring, i)] {
                if found.len() >= count || (ring > 0 && x.abs() != ring && z.abs() != ring) {
                    continue;
                }
                let pos = ChunkPos::new(x * 3, z * 3);
                let (min_x, min_z) = pos.min_block();
                if terrain.biome_at(min_x + 8, min_z + 8) != biome {
                    continue;
                }
                found.push(count_flowers(&terrain.generate(pos)));
            }
        }
        ring += 1;
    }
    found
}

fn count_flowers(chunk: &crate::world::chunk::Chunk) -> u32 {
    let (mut small, mut tall) = (0, 0);
    for y in 56..140 {
        for z in 0..16 {
            for x in 0..16 {
                let id = chunk.get(x, y, z).id();
                small += u32::from(SMALL.contains(&id));
                tall += u32::from(TALL.contains(&id));
            }
        }
    }
    small + tall / 2
}

/// Prints the average flowers per chunk and the share of chunks with none,
/// for every biome that grows flowers.
pub fn print_stats(seed: u64, chunks: usize) {
    let terrain = Terrain::new(seed);
    println!("seed {seed}, {chunks} chunks per biome");
    println!("biome                 flowers/chunk  empty chunks");
    for biome in [
        Biome::Plains,
        Biome::Meadow,
        Biome::Forest,
        Biome::BirchForest,
        Biome::DarkForest,
        Biome::Jungle,
        Biome::Savanna,
        Biome::Swamp,
        Biome::WindsweptHills,
        Biome::CherryGrove,
    ] {
        let counts = flowers_per_chunk(&terrain, biome, chunks);
        if counts.is_empty() {
            continue;
        }
        let total: u32 = counts.iter().sum();
        let empty = counts.iter().filter(|&&c| c == 0).count();
        println!(
            "{:<21} {:>13.2}  {:>11.0}%   ({} chunks)",
            format!("{biome:?}"),
            f64::from(total) / counts.len() as f64,
            100.0 * empty as f64 / counts.len() as f64,
            counts.len(),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plains_flowers_are_rare_and_clustered_like_vanilla() {
        let terrain = Terrain::new(12345);
        let counts = flowers_per_chunk(&terrain, Biome::Plains, 1000);
        assert_eq!(counts.len(), 1000);
        let mean = f64::from(counts.iter().sum::<u32>()) / 1000.0;
        let empty = counts.iter().filter(|&&c| c == 0).count();
        assert!((0.5..=4.0).contains(&mean), "{mean} flowers per chunk");
        assert!(empty >= 700, "only {empty} of 1000 chunks have none");
    }

    #[test]
    fn forests_are_no_denser_than_plains() {
        let terrain = Terrain::new(12345);
        for biome in [Biome::Forest, Biome::BirchForest] {
            let counts = flowers_per_chunk(&terrain, biome, 200);
            let mean = f64::from(counts.iter().sum::<u32>()) / counts.len() as f64;
            assert!(mean < 4.0, "{biome:?}: {mean} flowers per chunk");
        }
    }
}
