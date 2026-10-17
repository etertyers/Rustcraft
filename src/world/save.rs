//! Saving chunks: each chunk is encoded with bincode and compressed with
//! LZ4. Region files (32×32 chunks each) hold the chunks the player changed.

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::config::CHUNK_SIZE;

use super::{
    biome::Biome,
    block::BlockState,
    chunk::{Chunk, ChunkPos, SECTION_COUNT, SECTION_VOLUME},
};

#[derive(Debug, Error)]
pub enum SaveError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("corrupt save data: {0}")]
    Corrupt(String),
}

/// A chunk as stored on disk. All-air sections are left out.
#[derive(Serialize, Deserialize)]
struct ChunkData {
    x: i32,
    z: i32,
    sections: Vec<SectionData>,
    /// `Biome::id` per column, `z * 16 + x`.
    biomes: Vec<u8>,
}

#[derive(Serialize, Deserialize)]
struct SectionData {
    /// 0 = bottom.
    index: u8,
    /// `BlockState` bits in `y, z, x` order.
    blocks: Vec<u16>,
}

fn bincode_config() -> impl bincode::config::Config {
    // Fixed-size integers compress better than varints.
    bincode::config::standard().with_fixed_int_encoding()
}

/// Encodes and compresses a chunk.
pub fn encode_chunk(chunk: &Chunk) -> Vec<u8> {
    let sections = (0..SECTION_COUNT)
        .filter_map(|index| {
            let blocks = chunk.section(index)?;
            Some(SectionData {
                index: index as u8,
                blocks: blocks.iter().map(|block| block.to_bits()).collect(),
            })
        })
        .collect();
    let biomes = (0..CHUNK_SIZE * CHUNK_SIZE)
        .map(|i| chunk.biome(i % CHUNK_SIZE, i / CHUNK_SIZE).id())
        .collect();
    let data = ChunkData {
        x: chunk.pos.x,
        z: chunk.pos.z,
        sections,
        biomes,
    };
    let bytes = bincode::serde::encode_to_vec(&data, bincode_config())
        .expect("encoding to memory can't fail");
    lz4_flex::compress_prepend_size(&bytes)
}

/// Decompresses and decodes a chunk, checking every field.
pub fn decode_chunk(bytes: &[u8]) -> Result<Chunk, SaveError> {
    let corrupt = |what: String| SaveError::Corrupt(what);
    let bytes = lz4_flex::decompress_size_prepended(bytes)
        .map_err(|err| corrupt(format!("chunk decompression: {err}")))?;
    let (data, _): (ChunkData, usize) = bincode::serde::decode_from_slice(&bytes, bincode_config())
        .map_err(|err| corrupt(format!("chunk decoding: {err}")))?;
    let mut chunk = Chunk::new(ChunkPos::new(data.x, data.z));
    for section in data.sections {
        let index = section.index as usize;
        if index >= SECTION_COUNT || section.blocks.len() != SECTION_VOLUME {
            return Err(corrupt(format!("bad section {index}")));
        }
        let blocks: Vec<BlockState> = section
            .blocks
            .into_iter()
            .map(BlockState::from_bits)
            .collect();
        chunk.set_section(index, &blocks);
    }
    if data.biomes.len() != CHUNK_SIZE * CHUNK_SIZE {
        return Err(corrupt(format!("{} biomes", data.biomes.len())));
    }
    for (i, &id) in data.biomes.iter().enumerate() {
        let biome = Biome::from_id(id).ok_or_else(|| corrupt(format!("biome id {id}")))?;
        chunk.set_biome(i % CHUNK_SIZE, i / CHUNK_SIZE, biome);
    }
    Ok(chunk)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{config::WORLD_HEIGHT, world::worldgen::terrain::Terrain};

    pub(super) fn assert_same_chunk(a: &Chunk, b: &Chunk) {
        assert_eq!(a.pos, b.pos);
        for y in 0..WORLD_HEIGHT {
            for z in 0..CHUNK_SIZE {
                for x in 0..CHUNK_SIZE {
                    assert_eq!(a.get(x, y, z), b.get(x, y, z), "block at {x} {y} {z}");
                }
            }
        }
        for index in 0..SECTION_COUNT {
            assert_eq!(a.section_is_empty(index), b.section_is_empty(index));
        }
        for z in 0..CHUNK_SIZE {
            for x in 0..CHUNK_SIZE {
                assert_eq!(a.biome(x, z), b.biome(x, z));
            }
        }
    }

    #[test]
    fn generated_chunk_round_trips() {
        let terrain = Terrain::new(42);
        for pos in [ChunkPos::new(0, 0), ChunkPos::new(-7, 13)] {
            let chunk = terrain.generate(pos);
            let bytes = encode_chunk(&chunk);
            assert_same_chunk(&decode_chunk(&bytes).unwrap(), &chunk);
            // 64 KiB of block data compresses well.
            assert!(bytes.len() < 16 * 1024, "{} bytes", bytes.len());
        }
    }

    #[test]
    fn data_values_biomes_and_empty_sections_round_trip() {
        let mut chunk = Chunk::new(ChunkPos::new(3, -2));
        chunk.set(0, 0, 0, BlockState::new(35, 14));
        chunk.set(15, 127, 15, BlockState::new(50, 5));
        chunk.set_biome(4, 9, Biome::Desert);
        chunk.set_biome(15, 15, Biome::Ocean);
        let decoded = decode_chunk(&encode_chunk(&chunk)).unwrap();
        assert_same_chunk(&decoded, &chunk);
        assert!(decoded.section_is_empty(3));
    }

    #[test]
    fn garbage_is_an_error_not_a_panic() {
        assert!(matches!(
            decode_chunk(b"not a chunk"),
            Err(SaveError::Corrupt(_))
        ));
        let mut bytes = encode_chunk(&Chunk::new(ChunkPos::new(0, 0)));
        let last = bytes.len() - 1;
        bytes.truncate(last);
        assert!(decode_chunk(&bytes).is_err());
    }
}
