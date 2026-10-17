//! Saving chunks: each chunk is encoded with bincode and compressed with
//! LZ4. Region files (32×32 chunks each) hold the chunks the player changed.

use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::{ErrorKind, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};

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

/// Chunks per region side, like 1.0.0's McRegion files.
pub const REGION_SIZE: i32 = 32;
const REGION_CHUNKS: usize = (REGION_SIZE * REGION_SIZE) as usize;
const REGION_MAGIC: &[u8; 4] = b"RCRG";
const REGION_VERSION: u32 = 1;
/// Magic and version, then an (offset, length) pair per chunk.
const HEADER_LEN: usize = 8 + REGION_CHUNKS * 8;

/// A folder of region files, `r.<x>.<z>.rcr`, each holding up to 32×32
/// chunks. A file is the magic and version, then `(offset, length)` for
/// every chunk slot (length 0 = not saved), then the encoded chunks, all
/// little-endian.
pub struct RegionStore {
    dir: PathBuf,
}

impl RegionStore {
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "saved worlds arrive with the session")
    )]
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    fn path(&self, pos: ChunkPos) -> PathBuf {
        let (x, z) = (pos.x.div_euclid(REGION_SIZE), pos.z.div_euclid(REGION_SIZE));
        self.dir.join(format!("r.{x}.{z}.rcr"))
    }

    fn slot(pos: ChunkPos) -> usize {
        (pos.z.rem_euclid(REGION_SIZE) * REGION_SIZE + pos.x.rem_euclid(REGION_SIZE)) as usize
    }

    /// The saved chunk at `pos`, or `None` if it was never saved.
    pub fn load(&self, pos: ChunkPos) -> Result<Option<Chunk>, SaveError> {
        let mut file = match File::open(self.path(pos)) {
            Ok(file) => file,
            Err(err) if err.kind() == ErrorKind::NotFound => return Ok(None),
            Err(err) => return Err(err.into()),
        };
        let mut header = [0; 8];
        file.read_exact(&mut header)?;
        check_header(&header)?;
        file.seek(SeekFrom::Start(8 + Self::slot(pos) as u64 * 8))?;
        let mut entry = [0; 8];
        file.read_exact(&mut entry)?;
        let (offset, len) = parse_entry(&entry);
        if len == 0 {
            return Ok(None);
        }
        file.seek(SeekFrom::Start(offset as u64))?;
        let mut blob = vec![0; len];
        file.read_exact(&mut blob)?;
        let chunk = decode_chunk(&blob)?;
        if chunk.pos != pos {
            return Err(SaveError::Corrupt(format!(
                "slot for {pos:?} holds {:?}",
                chunk.pos
            )));
        }
        Ok(Some(chunk))
    }

    /// Writes `chunks` into their region files, keeping the other chunks
    /// already saved there.
    pub fn save<'a>(&self, chunks: impl IntoIterator<Item = &'a Chunk>) -> Result<(), SaveError> {
        let mut regions: BTreeMap<PathBuf, Vec<&Chunk>> = BTreeMap::new();
        for chunk in chunks {
            regions.entry(self.path(chunk.pos)).or_default().push(chunk);
        }
        if regions.is_empty() {
            return Ok(());
        }
        fs::create_dir_all(&self.dir)?;
        for (path, chunks) in regions {
            let mut blobs = read_blobs(&path)?;
            for chunk in chunks {
                blobs[Self::slot(chunk.pos)] = Some(encode_chunk(chunk));
            }
            write_region(&path, &blobs)?;
        }
        Ok(())
    }
}

fn check_header(header: &[u8]) -> Result<(), SaveError> {
    let version = u32::from_le_bytes(header[4..8].try_into().unwrap());
    if &header[..4] != REGION_MAGIC || version != REGION_VERSION {
        return Err(SaveError::Corrupt(format!(
            "not a version {REGION_VERSION} region file"
        )));
    }
    Ok(())
}

fn parse_entry(entry: &[u8]) -> (usize, usize) {
    let offset = u32::from_le_bytes(entry[..4].try_into().unwrap());
    let len = u32::from_le_bytes(entry[4..8].try_into().unwrap());
    (offset as usize, len as usize)
}

/// Every saved chunk's encoded bytes, by slot (all `None` if there's no file).
fn read_blobs(path: &Path) -> Result<Vec<Option<Vec<u8>>>, SaveError> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(err) if err.kind() == ErrorKind::NotFound => return Ok(vec![None; REGION_CHUNKS]),
        Err(err) => return Err(err.into()),
    };
    if bytes.len() < HEADER_LEN {
        return Err(SaveError::Corrupt("truncated region header".into()));
    }
    check_header(&bytes)?;
    (0..REGION_CHUNKS)
        .map(|slot| {
            let (offset, len) = parse_entry(&bytes[8 + slot * 8..]);
            if len == 0 {
                return Ok(None);
            }
            let blob = bytes
                .get(offset..offset + len)
                .ok_or_else(|| SaveError::Corrupt(format!("slot {slot} runs past the file")))?;
            Ok(Some(blob.to_vec()))
        })
        .collect()
}

/// Writes a temporary file and renames it over the old one, so a crash
/// mid-save leaves the previous version intact.
fn write_region(path: &Path, blobs: &[Option<Vec<u8>>]) -> Result<(), SaveError> {
    let total: usize = blobs.iter().flatten().map(Vec::len).sum();
    let mut bytes = Vec::with_capacity(HEADER_LEN + total);
    bytes.extend_from_slice(REGION_MAGIC);
    bytes.extend_from_slice(&REGION_VERSION.to_le_bytes());
    let mut offset = HEADER_LEN;
    for blob in blobs {
        let len = blob.as_ref().map_or(0, Vec::len);
        bytes.extend_from_slice(&(offset as u32).to_le_bytes());
        bytes.extend_from_slice(&(len as u32).to_le_bytes());
        offset += len;
    }
    for blob in blobs.iter().flatten() {
        bytes.extend_from_slice(blob);
    }
    let temp = path.with_extension("rcr.tmp");
    let mut file = File::create(&temp)?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    fs::rename(&temp, path)?;
    Ok(())
}

/// A fresh folder under the system temp folder, deleted when dropped.
#[cfg(test)]
pub struct TempDir(PathBuf);

#[cfg(test)]
impl TempDir {
    pub fn new(name: &str) -> Self {
        use std::sync::atomic::{AtomicU32, Ordering};
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let dir =
            std::env::temp_dir().join(format!("rustcraft-test-{}-{n}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

#[cfg(test)]
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
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

    fn chunk_with_marker(pos: ChunkPos, marker: u8) -> Chunk {
        let mut chunk = Chunk::new(pos);
        chunk.set(1, 64, 2, BlockState::new(35, marker));
        chunk
    }

    #[test]
    fn region_files_round_trip_across_regions_and_negative_coordinates() {
        let dir = TempDir::new("regions");
        let positions = [
            ChunkPos::new(0, 0),
            ChunkPos::new(31, 31),
            ChunkPos::new(32, 0),
            ChunkPos::new(-1, -1),
            ChunkPos::new(-33, 5),
        ];
        let chunks: Vec<Chunk> = positions
            .iter()
            .enumerate()
            .map(|(i, &pos)| chunk_with_marker(pos, i as u8))
            .collect();
        RegionStore::new(dir.path()).save(&chunks).unwrap();

        let mut files: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        files.sort();
        assert_eq!(
            files,
            ["r.-1.-1.rcr", "r.-2.0.rcr", "r.0.0.rcr", "r.1.0.rcr"],
            "no temporary files left behind"
        );

        // A new store reads them back, as after relaunching.
        let store = RegionStore::new(dir.path());
        for chunk in &chunks {
            assert_same_chunk(&store.load(chunk.pos).unwrap().unwrap(), chunk);
        }
        assert!(store.load(ChunkPos::new(1, 0)).unwrap().is_none());
        assert!(store.load(ChunkPos::new(100, 100)).unwrap().is_none());
    }

    #[test]
    fn saving_again_replaces_one_chunk_and_keeps_its_neighbors() {
        let dir = TempDir::new("resave");
        let store = RegionStore::new(dir.path());
        let (a, b) = (ChunkPos::new(2, 3), ChunkPos::new(4, 5));
        store
            .save(&[chunk_with_marker(a, 1), chunk_with_marker(b, 2)])
            .unwrap();
        store.save(&[chunk_with_marker(a, 9)]).unwrap();
        assert_same_chunk(&store.load(a).unwrap().unwrap(), &chunk_with_marker(a, 9));
        assert_same_chunk(&store.load(b).unwrap().unwrap(), &chunk_with_marker(b, 2));
    }

    #[test]
    fn a_damaged_region_file_is_an_error() {
        let dir = TempDir::new("damaged");
        fs::write(dir.path().join("r.0.0.rcr"), b"definitely not a region").unwrap();
        let store = RegionStore::new(dir.path());
        assert!(store.load(ChunkPos::new(0, 0)).is_err());
        assert!(store.save(&[Chunk::new(ChunkPos::new(0, 0))]).is_err());
    }
}
