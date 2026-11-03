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
    block_entity::BlockEntity,
    chunk::{CAVE_CELLS, Chunk, ChunkPos, LocalPos, NO_CAVE, SECTION_COUNT, SECTION_VOLUME},
    fast_hash::FastMap,
};

#[derive(Debug, Error)]
pub enum SaveError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("corrupt save data: {0}")]
    Corrupt(String),
}

/// The chunk format: 4 reaches down to Y −64 (M34): sections count from
/// there and block positions hold a world Y; 3 adds cave biomes (M21); 2
/// stores blocks by name (v1.1); 1 was v1.0's numeric 1.0.0 IDs, which
/// `legacy` upgrades.
pub const CHUNK_VERSION: u32 = 4;

/// A block position in a chunk before version 4: x, y (from 0) and z.
type OldPos = [u8; 3];

/// Sections and cave biome cells a chunk from before version 4 moves up:
/// its Y 0 is section 4 now.
const OLD_SECTION_SHIFT: usize =
    (-crate::config::MIN_Y) as usize / crate::world::chunk::SECTION_SIZE;
const OLD_CAVE_LAYERS: usize = 256 / 4;

/// A chunk as stored on disk. All-air sections are left out.
#[derive(Serialize, Deserialize)]
struct ChunkData {
    version: u32,
    x: i32,
    z: i32,
    sections: Vec<SectionData>,
    /// `Biome::id` per column, `z * 16 + x`.
    biomes: Vec<u8>,
    /// Chests and furnaces, by position in the chunk.
    block_entities: Vec<(LocalPos, BlockEntity)>,
    /// `Chunk::post_process`.
    post_process: Vec<LocalPos>,
    /// `Chunk::cave_biomes`: `CAVE_CELLS` biome ids, or empty for none.
    cave_biomes: Vec<u8>,
}

/// Version 3: Y 0 to 255, positions as bytes.
#[derive(Deserialize)]
struct ChunkDataV3 {
    version: u32,
    x: i32,
    z: i32,
    sections: Vec<SectionData>,
    biomes: Vec<u8>,
    block_entities: Vec<(OldPos, BlockEntity)>,
    post_process: Vec<OldPos>,
    cave_biomes: Vec<u8>,
}

/// Version 2, which is version 3 without cave biomes.
#[derive(Deserialize)]
struct ChunkDataV2 {
    version: u32,
    x: i32,
    z: i32,
    sections: Vec<SectionData>,
    biomes: Vec<u8>,
    block_entities: Vec<(OldPos, BlockEntity)>,
    post_process: Vec<OldPos>,
}

impl From<ChunkDataV2> for ChunkDataV3 {
    fn from(old: ChunkDataV2) -> Self {
        Self {
            version: old.version,
            x: old.x,
            z: old.z,
            sections: old.sections,
            biomes: old.biomes,
            block_entities: old.block_entities,
            post_process: old.post_process,
            cave_biomes: Vec::new(),
        }
    }
}

impl From<ChunkDataV3> for ChunkData {
    /// Moves an old chunk's sections, cave biomes and positions to where
    /// Y 0 is now; everything stays at the same height.
    fn from(old: ChunkDataV3) -> Self {
        let pos = |[x, y, z]: OldPos| [x as i16, y as i16, z as i16];
        let mut cave_biomes = Vec::new();
        if old.cave_biomes.len() == 16 * OLD_CAVE_LAYERS {
            cave_biomes = vec![NO_CAVE; CAVE_CELLS];
            let below = 16 * (-crate::config::MIN_Y as usize / 4);
            cave_biomes[below..below + old.cave_biomes.len()].copy_from_slice(&old.cave_biomes);
        }
        Self {
            version: old.version,
            x: old.x,
            z: old.z,
            sections: old
                .sections
                .into_iter()
                .map(|s| SectionData {
                    index: s.index + OLD_SECTION_SHIFT as u8,
                    ..s
                })
                .collect(),
            biomes: old.biomes,
            block_entities: old
                .block_entities
                .into_iter()
                .map(|(at, entity)| (pos(at), entity))
                .collect(),
            post_process: old.post_process.into_iter().map(pos).collect(),
            cave_biomes,
        }
    }
}

/// A section's blocks as indices into its own list of states, which are
/// saved by name (`minecraft:oak_stairs[facing=east,...]`), so runtime
/// IDs can change between versions.
#[derive(Serialize, Deserialize)]
struct SectionData {
    /// 0 = bottom.
    index: u8,
    palette: Vec<String>,
    /// Palette indices in `y, z, x` order.
    blocks: Vec<u16>,
}

fn bincode_config() -> impl bincode::config::Config {
    // Fixed-size integers compress better than varints.
    bincode::config::standard().with_fixed_int_encoding()
}

/// A section's palette (state texts) and indices. `lost` holds the real
/// states of unknown blocks, by `section * 4096 + index`.
fn palettize(
    blocks: &[BlockState],
    section: usize,
    lost: &std::collections::BTreeMap<u32, String>,
) -> (Vec<String>, Vec<u16>) {
    let mut palette: Vec<String> = Vec::new();
    let mut lookup: FastMap<BlockState, u16> = FastMap::default();
    let mut lost_lookup: std::collections::HashMap<&str, u16> = Default::default();
    let base = (section * SECTION_VOLUME) as u32;
    let indices = blocks
        .iter()
        .enumerate()
        .map(|(i, &block)| {
            if !lost.is_empty()
                && block.is(crate::world::blocks::UNKNOWN_BLOCK)
                && let Some(text) = lost.get(&(base + i as u32))
            {
                return *lost_lookup.entry(text).or_insert_with(|| {
                    palette.push(text.clone());
                    (palette.len() - 1) as u16
                });
            }
            *lookup.entry(block).or_insert_with(|| {
                palette.push(block.to_string());
                (palette.len() - 1) as u16
            })
        })
        .collect();
    (palette, indices)
}

/// Reads a palette entry. A block of a mod that is not installed (or
/// whose properties changed) becomes `UNKNOWN_BLOCK`, and its text is
/// returned to be kept; anything else wrong is an error.
fn parse_palette_entry(text: &str) -> Result<(BlockState, Option<String>), String> {
    match BlockState::parse(text) {
        Ok(state) => Ok((state, None)),
        Err(err) => {
            let name = text.split('[').next().unwrap_or(text);
            if name.contains(':') && !name.starts_with("minecraft:") {
                Ok((
                    BlockState::of(crate::world::blocks::UNKNOWN_BLOCK),
                    Some(text.to_owned()),
                ))
            } else {
                Err(format!("{err:#}"))
            }
        }
    }
}

/// Encodes and compresses a chunk.
pub fn encode_chunk(chunk: &Chunk) -> Vec<u8> {
    let sections = (0..SECTION_COUNT)
        .filter_map(|index| {
            let (palette, blocks) = palettize(chunk.section(index)?, index, &chunk.lost);
            Some(SectionData {
                index: index as u8,
                palette,
                blocks,
            })
        })
        .collect();
    let biomes = (0..CHUNK_SIZE * CHUNK_SIZE)
        .map(|i| chunk.biome(i % CHUNK_SIZE, i / CHUNK_SIZE).id())
        .collect();
    let data = ChunkData {
        version: CHUNK_VERSION,
        x: chunk.pos.x,
        z: chunk.pos.z,
        sections,
        biomes,
        block_entities: chunk
            .block_entities()
            .map(|(at, entity)| (at, entity.clone()))
            .collect(),
        post_process: chunk.post_process.clone(),
        cave_biomes: chunk
            .cave_biomes()
            .map_or(Vec::new(), |cells| cells.to_vec()),
    };
    let bytes = bincode::serde::encode_to_vec(&data, bincode_config())
        .expect("encoding to memory can't fail");
    lz4_flex::compress_prepend_size(&bytes)
}

/// Decompresses and decodes a chunk, checking every field. Items of mods
/// that aren't installed, in its chests, read as unknown items.
pub fn decode_chunk(bytes: &[u8]) -> Result<Chunk, SaveError> {
    crate::inventory::item::lenient(|| decode_chunk_strict(bytes))
}

fn decode_chunk_strict(bytes: &[u8]) -> Result<Chunk, SaveError> {
    let corrupt = |what: String| SaveError::Corrupt(what);
    let bytes = lz4_flex::decompress_size_prepended(bytes)
        .map_err(|err| corrupt(format!("chunk decompression: {err}")))?;
    // The version comes first, as a fixed-size little-endian u32.
    let version = bytes
        .first_chunk::<4>()
        .map(|b| u32::from_le_bytes(*b))
        .ok_or_else(|| corrupt("empty chunk".into()))?;
    let decoding = |err| corrupt(format!("chunk decoding: {err}"));
    let data: ChunkData = match version {
        CHUNK_VERSION => {
            bincode::serde::decode_from_slice(&bytes, bincode_config())
                .map_err(decoding)?
                .0
        }
        3 => bincode::serde::decode_from_slice::<ChunkDataV3, _>(&bytes, bincode_config())
            .map_err(decoding)?
            .0
            .into(),
        2 => ChunkDataV3::from(
            bincode::serde::decode_from_slice::<ChunkDataV2, _>(&bytes, bincode_config())
                .map_err(decoding)?
                .0,
        )
        .into(),
        _ => return Err(corrupt(format!("chunk version {version}"))),
    };
    let mut chunk = Chunk::new(ChunkPos::new(data.x, data.z));
    for section in data.sections {
        let index = section.index as usize;
        if index >= SECTION_COUNT || section.blocks.len() != SECTION_VOLUME {
            return Err(corrupt(format!("bad section {index}")));
        }
        let palette = section
            .palette
            .iter()
            .map(|text| parse_palette_entry(text))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|why| corrupt(format!("section {index}'s palette: {why}")))?;
        let blocks = section
            .blocks
            .iter()
            .map(|&i| palette.get(i as usize))
            .collect::<Option<Vec<_>>>()
            .ok_or_else(|| corrupt(format!("section {index} indexes past its palette")))?;
        for (i, (_, lost)) in blocks.iter().enumerate() {
            if let Some(text) = lost {
                chunk
                    .lost
                    .insert((index * SECTION_VOLUME + i) as u32, text.clone());
            }
        }
        let states: Vec<BlockState> = blocks.iter().map(|(state, _)| *state).collect();
        chunk.set_section(index, &states);
    }
    if data.biomes.len() != CHUNK_SIZE * CHUNK_SIZE {
        return Err(corrupt(format!("{} biomes", data.biomes.len())));
    }
    for (i, &id) in data.biomes.iter().enumerate() {
        let biome = Biome::from_id(id).ok_or_else(|| corrupt(format!("biome id {id}")))?;
        chunk.set_biome(i % CHUNK_SIZE, i / CHUNK_SIZE, biome);
    }
    match data.cave_biomes.len() {
        0 => {}
        CAVE_CELLS => {
            for (i, &id) in data.cave_biomes.iter().enumerate() {
                let biome = match id {
                    NO_CAVE => None,
                    _ => Some(Biome::from_id(id).ok_or_else(|| corrupt(format!("biome id {id}")))?),
                };
                let (x, z, y) = (i % 4 * 4, i / 4 % 4 * 4, i / 16 * 4);
                chunk.set_cave_biome(x, crate::config::MIN_Y + y as i32, z, biome);
            }
        }
        n => return Err(corrupt(format!("{n} cave biomes"))),
    }
    for (at, entity) in data.block_entities {
        let [x, y, z] = at;
        let inside = |v: i16| (0..CHUNK_SIZE as i16).contains(&v);
        if !inside(x) || !inside(z) || !crate::world::chunk::in_height(y.into()) {
            return Err(corrupt(format!("block entity at {at:?}")));
        }
        chunk.set_block_entity(at, Some(entity));
    }
    chunk.post_process = data.post_process;
    // A chunk made before M34 has nothing under Y 0 yet.
    chunk.shallow = version < 4;
    Ok(chunk)
}

/// Chunks per region side, like 1.0.0's McRegion files.
pub const REGION_SIZE: i32 = 32;
pub(crate) const REGION_CHUNKS: usize = (REGION_SIZE * REGION_SIZE) as usize;
const REGION_MAGIC: &[u8; 4] = b"RCRG";
/// 2 since v1.1 (chunks inside are `CHUNK_VERSION` 2 or 3).
const REGION_VERSION: u32 = 2;
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
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    fn path(&self, pos: ChunkPos) -> PathBuf {
        let (x, z) = (pos.x.div_euclid(REGION_SIZE), pos.z.div_euclid(REGION_SIZE));
        self.dir.join(format!("r.{x}.{z}.rcr"))
    }

    pub(crate) fn slot(pos: ChunkPos) -> usize {
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

    /// Rewrites every region file still in the v1.0 format, decoding its
    /// chunks with `decode_v1` and saving them in today's. Returns how many
    /// chunks were upgraded.
    pub fn upgrade_v1(
        &self,
        decode_v1: impl Fn(&[u8]) -> Result<Chunk, SaveError>,
    ) -> Result<usize, SaveError> {
        let entries = match fs::read_dir(&self.dir) {
            Ok(entries) => entries,
            Err(err) if err.kind() == ErrorKind::NotFound => return Ok(0),
            Err(err) => return Err(err.into()),
        };
        let mut upgraded = 0;
        for entry in entries {
            let path = entry?.path();
            if path.extension().is_none_or(|ext| ext != "rcr") {
                continue;
            }
            let (version, blobs) = read_region(&path)?;
            if version != 1 {
                continue;
            }
            let blobs = blobs
                .into_iter()
                .map(|blob| {
                    blob.map(|old| {
                        upgraded += 1;
                        decode_v1(&old).map(|chunk| encode_chunk(&chunk))
                    })
                    .transpose()
                })
                .collect::<Result<Vec<_>, _>>()?;
            write_region(&path, &blobs)?;
        }
        Ok(upgraded)
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

/// The region file version in `header`, if it's a region file.
fn header_version(header: &[u8]) -> Result<u32, SaveError> {
    if &header[..4] != REGION_MAGIC {
        return Err(SaveError::Corrupt("not a region file".into()));
    }
    Ok(u32::from_le_bytes(header[4..8].try_into().unwrap()))
}

fn check_header(header: &[u8]) -> Result<(), SaveError> {
    match header_version(header)? {
        REGION_VERSION => Ok(()),
        1 => Err(SaveError::Corrupt(
            "a v1.0 region file (the world needs upgrading first)".into(),
        )),
        version => Err(SaveError::Corrupt(format!(
            "region file version {version}, not {REGION_VERSION}"
        ))),
    }
}

fn parse_entry(entry: &[u8]) -> (usize, usize) {
    let offset = u32::from_le_bytes(entry[..4].try_into().unwrap());
    let len = u32::from_le_bytes(entry[4..8].try_into().unwrap());
    (offset as usize, len as usize)
}

/// Each slot's encoded chunk, if one is saved there.
type Blobs = Vec<Option<Vec<u8>>>;

/// Every saved chunk's encoded bytes, by slot (all `None` if there's no file).
fn read_blobs(path: &Path) -> Result<Blobs, SaveError> {
    if !path.exists() {
        return Ok(vec![None; REGION_CHUNKS]);
    }
    let (version, blobs) = read_region(path)?;
    if version != REGION_VERSION {
        check_header(&[&REGION_MAGIC[..], &version.to_le_bytes()].concat())?;
    }
    Ok(blobs)
}

/// A region file's version and every saved chunk's encoded bytes, by slot.
fn read_region(path: &Path) -> Result<(u32, Blobs), SaveError> {
    let bytes = fs::read(path)?;
    if bytes.len() < HEADER_LEN {
        return Err(SaveError::Corrupt("truncated region header".into()));
    }
    let version = header_version(&bytes)?;
    let blobs = (0..REGION_CHUNKS)
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
        .collect::<Result<_, SaveError>>()?;
    Ok((version, blobs))
}

/// Writes a temporary file and renames it over the old one, so a crash
/// mid-save leaves the previous version intact.
fn write_region(path: &Path, blobs: &[Option<Vec<u8>>]) -> Result<(), SaveError> {
    write_region_version(path, blobs, REGION_VERSION)
}

/// `write_region` with any version number (tests write v1.0 files).
pub(crate) fn write_region_version(
    path: &Path,
    blobs: &[Option<Vec<u8>>],
    version: u32,
) -> Result<(), SaveError> {
    let total: usize = blobs.iter().flatten().map(Vec::len).sum();
    let mut bytes = Vec::with_capacity(HEADER_LEN + total);
    bytes.extend_from_slice(REGION_MAGIC);
    bytes.extend_from_slice(&version.to_le_bytes());
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
    use crate::world::{Face, blocks, props::Prop, worldgen::terrain::Terrain};

    pub(super) fn assert_same_chunk(a: &Chunk, b: &Chunk) {
        assert_eq!(a.pos, b.pos);
        for y in crate::config::MIN_Y..crate::config::MAX_Y {
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
        assert_eq!(a.cave_biomes(), b.cave_biomes());
        assert!(a.block_entities().eq(b.block_entities()));
    }

    #[test]
    fn cave_biomes_round_trip() {
        let mut chunk = Chunk::new(ChunkPos::new(0, 0));
        chunk.set_biome(5, 5, Biome::Forest);
        chunk.set_cave_biome(5, 20, 5, Some(Biome::LushCaves));
        chunk.set_cave_biome(12, 200, 1, Some(Biome::DripstoneCaves));
        let decoded = decode_chunk(&encode_chunk(&chunk)).unwrap();
        assert_same_chunk(&decoded, &chunk);
        assert_eq!(decoded.biome_at(4, 23, 7), Biome::LushCaves);
        assert_eq!(decoded.biome_at(5, 24, 5), Biome::Forest);
        assert_eq!(decoded.biome_at(13, 201, 2), Biome::DripstoneCaves);
    }

    #[test]
    fn version_2_chunks_still_load() {
        #[derive(Serialize)]
        struct V2 {
            version: u32,
            x: i32,
            z: i32,
            sections: Vec<SectionData>,
            biomes: Vec<u8>,
            block_entities: Vec<(LocalPos, BlockEntity)>,
            post_process: Vec<LocalPos>,
        }
        let stone = BlockState::of(blocks::STONE);
        let old = V2 {
            version: 2,
            x: 4,
            z: -1,
            sections: vec![SectionData {
                index: 3,
                palette: vec![stone.to_string()],
                blocks: vec![0; SECTION_VOLUME],
            }],
            biomes: vec![Biome::Desert.id(); CHUNK_SIZE * CHUNK_SIZE],
            block_entities: Vec::new(),
            post_process: Vec::new(),
        };
        let bytes = bincode::serde::encode_to_vec(&old, bincode_config()).unwrap();
        let chunk = decode_chunk(&lz4_flex::compress_prepend_size(&bytes)).unwrap();
        assert_eq!(chunk.pos, ChunkPos::new(4, -1));
        assert_eq!(chunk.get(3, 50, 9), stone);
        assert_eq!(chunk.biome_at(3, 50, 9), Biome::Desert);
        assert!(chunk.cave_biomes().is_none());
    }

    /// The M34 migration: a version 3 chunk (Y 0 to 255) keeps every block
    /// and chest at its height, gets the Overworld's new depth under it in
    /// place of its bedrock floor, and saves as version 4.
    #[test]
    fn version_3_chunks_keep_their_blocks_and_get_new_depth() {
        #[derive(Serialize)]
        struct V3 {
            version: u32,
            x: i32,
            z: i32,
            sections: Vec<SectionData>,
            biomes: Vec<u8>,
            block_entities: Vec<([u8; 3], BlockEntity)>,
            post_process: Vec<[u8; 3]>,
            cave_biomes: Vec<u8>,
        }
        let (stone, bedrock, chest) = (
            BlockState::of(blocks::STONE),
            BlockState::of(blocks::BEDROCK),
            BlockState::of(blocks::CHEST),
        );
        // Section 0 (Y 0–15): bedrock at the bottom, stone above; a chest
        // at Y 70 in section 4.
        let mut bottom = vec![1u16; SECTION_VOLUME];
        bottom[..256].fill(0);
        let mut upper = vec![0u16; SECTION_VOLUME];
        upper[(6 * 16 + 2) * 16 + 2] = 1;
        let old = V3 {
            version: 3,
            x: 2,
            z: 3,
            sections: vec![
                SectionData {
                    index: 0,
                    palette: vec![bedrock.to_string(), stone.to_string()],
                    blocks: bottom,
                },
                SectionData {
                    index: 4,
                    palette: vec![BlockState::AIR.to_string(), chest.to_string()],
                    blocks: upper,
                },
            ],
            biomes: vec![Biome::Plains.id(); CHUNK_SIZE * CHUNK_SIZE],
            block_entities: vec![([2, 70, 2], BlockEntity::Chest(vec![None; 27]))],
            post_process: vec![[1, 20, 1]],
            cave_biomes: Vec::new(),
        };
        let bytes = bincode::serde::encode_to_vec(&old, bincode_config()).unwrap();
        let mut chunk = decode_chunk(&lz4_flex::compress_prepend_size(&bytes)).unwrap();
        assert!(chunk.shallow);
        assert_eq!(chunk.get(5, 0, 5), bedrock);
        assert_eq!(chunk.get(5, 10, 5), stone);
        assert_eq!(chunk.get(2, 70, 2), chest);
        assert!(chunk.block_entity([2, 70, 2]).is_some());
        assert_eq!(chunk.post_process, vec![[1, 20, 1]]);
        assert!(chunk.get(5, -10, 5).is_air(), "nothing below 0 yet");

        let terrain = crate::world::worldgen::terrain::Terrain::new(9);
        terrain.deepen(&mut chunk);
        assert!(!chunk.shallow);
        assert_eq!(chunk.get(2, 70, 2), chest);
        assert_eq!(chunk.get(5, 10, 5), stone);
        assert_eq!(chunk.get(5, crate::config::MIN_Y, 5), bedrock);
        assert!(
            (0..16).all(|x| chunk.get(x, 0, 7) != bedrock),
            "the old floor is gone"
        );
        let deep = (crate::config::MIN_Y + 5..0)
            .filter(|&y| chunk.get(8, y, 8).is(blocks::DEEPSLATE))
            .count();
        assert!(deep > 20, "{deep} deepslate under the old floor");

        // Saved again, it's a version 4 chunk that reads back the same.
        let again = decode_chunk(&encode_chunk(&chunk)).unwrap();
        assert!(!again.shallow);
        assert_same_chunk(&again, &chunk);
    }

    #[test]
    fn chests_round_trip() {
        let mut chunk = Chunk::new(ChunkPos::new(1, 1));
        let chest = BlockState::of(blocks::CHEST).with_face(Prop::Facing, Face::South);
        chunk.set(2, 70, 3, chest);
        let mut entity = BlockEntity::chest();
        entity.slots_mut()[26] = Some(crate::inventory::stack::ItemStack::with_damage(
            crate::inventory::items::DIAMOND_PICKAXE,
            1,
            40,
        ));
        chunk.set_block_entity([2, 70, 3], Some(entity));
        assert_same_chunk(&decode_chunk(&encode_chunk(&chunk)).unwrap(), &chunk);
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
        chunk.set(0, 0, 0, BlockState::of(blocks::RED_WOOL));
        chunk.set(15, 127, 15, BlockState::of(blocks::TORCH));
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
        let wheat = BlockState::of(blocks::WHEAT).with_number(Prop::Age7, marker);
        chunk.set(1, 64, 2, wheat);
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
