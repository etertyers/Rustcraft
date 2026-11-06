//! LOD storage: compressed region files in `saves/<world>/lod/`.
//!
//! A region file holds the tiles of one level in an 8×8 block of tile
//! coordinates. Its modification time is its last use (reading touches it),
//! and when the folder passes its cap the oldest-used files go first.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use super::data::{Tile, TileKey};

/// Tiles along a side of a region.
const REGION_TILES: i32 = 8;
const MAGIC: &[u8; 4] = b"LOD1";
/// Regions kept decoded in memory at once.
const CACHE_REGIONS: usize = 16;

type RegionKey = (u8, i32, i32);

fn region_of(key: TileKey) -> RegionKey {
    (
        key.level,
        key.tx.div_euclid(REGION_TILES),
        key.tz.div_euclid(REGION_TILES),
    )
}

fn region_path(dir: &Path, (level, rx, rz): RegionKey) -> PathBuf {
    dir.join(format!("r.{level}.{rx}.{rz}.lod"))
}

/// The raw (still compressed) tiles of one region.
type Region = HashMap<(i32, i32), Vec<u8>>;

pub struct LodStore {
    dir: PathBuf,
    cap: u64,
    cache: HashMap<RegionKey, Region>,
    /// Least recently used first.
    order: Vec<RegionKey>,
    dirty: HashSet<RegionKey>,
    touched: HashSet<RegionKey>,
}

impl LodStore {
    pub fn open(dir: &Path, cap: u64) -> io::Result<Self> {
        fs::create_dir_all(dir)?;
        Ok(Self {
            dir: dir.to_path_buf(),
            cap,
            cache: HashMap::new(),
            order: Vec::new(),
            dirty: HashSet::new(),
            touched: HashSet::new(),
        })
    }

    fn path(&self, region: RegionKey) -> PathBuf {
        region_path(&self.dir, region)
    }

    pub fn set_cap(&mut self, cap: u64) {
        self.cap = cap;
    }

    fn load(&mut self, region: RegionKey) -> &mut Region {
        if !self.cache.contains_key(&region) {
            let data = fs::read(self.path(region))
                .ok()
                .and_then(|bytes| decode_region(&bytes))
                .unwrap_or_default();
            if self.cache.len() >= CACHE_REGIONS {
                // Drop the least recently used region that is saved.
                if let Some(at) = self.order.iter().position(|r| !self.dirty.contains(r)) {
                    let old = self.order.remove(at);
                    self.cache.remove(&old);
                }
            }
            self.cache.insert(region, data);
        }
        self.order.retain(|r| *r != region);
        self.order.push(region);
        self.cache.get_mut(&region).expect("just inserted")
    }

    /// The tile saved for `key`, if any (and not damaged).
    pub fn get(&mut self, key: TileKey) -> Option<Tile> {
        let region = region_of(key);
        let tile = self
            .load(region)
            .get(&(key.tx, key.tz))
            .and_then(|bytes| Tile::decode(bytes))?;
        self.touched.insert(region);
        Some(tile)
    }

    pub fn contains(&mut self, key: TileKey) -> bool {
        let region = region_of(key);
        self.load(region).contains_key(&(key.tx, key.tz))
    }

    pub fn put(&mut self, tile: &Tile) {
        let region = region_of(tile.key);
        let bytes = tile.encode();
        self.load(region).insert((tile.key.tx, tile.key.tz), bytes);
        self.dirty.insert(region);
    }

    /// Writes changed regions, marks read ones as used, and removes the
    /// oldest-used files while the folder is over its cap.
    pub fn flush(&mut self) -> io::Result<()> {
        let dirty: Vec<RegionKey> = self.dirty.drain().collect();
        for region in dirty {
            let bytes = encode_region(&self.cache[&region]);
            let path = self.path(region);
            let tmp = path.with_extension("tmp");
            fs::File::create(&tmp)?.write_all(&bytes)?;
            fs::rename(&tmp, &path)?;
        }
        for region in self.touched.drain().collect::<Vec<_>>() {
            if let Ok(file) = fs::File::options().write(true).open(self.path(region)) {
                let _ = file.set_modified(SystemTime::now());
            }
        }
        self.enforce_cap()
    }

    /// Bytes the files take.
    pub fn usage(&self) -> u64 {
        files(&self.dir).iter().map(|f| f.1).sum()
    }

    fn enforce_cap(&mut self) -> io::Result<()> {
        let mut list = files(&self.dir);
        let mut total: u64 = list.iter().map(|f| f.1).sum();
        list.sort_by_key(|f| f.2);
        for (path, size, _) in list {
            if total <= self.cap {
                break;
            }
            fs::remove_file(&path)?;
            total -= size;
            let dir = self.dir.clone();
            let same = |region: &RegionKey| region_path(&dir, *region) == path;
            self.cache.retain(|region, _| !same(region));
            self.order.retain(|region| !same(region));
        }
        Ok(())
    }

    /// Deletes every file (the world's "reset distant terrain").
    pub fn clear(&mut self) -> io::Result<()> {
        self.cache.clear();
        self.order.clear();
        self.dirty.clear();
        self.touched.clear();
        for (path, _, _) in files(&self.dir) {
            fs::remove_file(path)?;
        }
        Ok(())
    }
}

/// Region files with their sizes and last-used times.
fn files(dir: &Path) -> Vec<(PathBuf, u64, SystemTime)> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .filter_map(Result::ok)
        .filter(|e| e.path().extension().is_some_and(|x| x == "lod"))
        .filter_map(|e| {
            let meta = e.metadata().ok()?;
            Some((e.path(), meta.len(), meta.modified().ok()?))
        })
        .collect()
}

fn encode_region(region: &Region) -> Vec<u8> {
    let mut out = MAGIC.to_vec();
    out.extend((region.len() as u32).to_le_bytes());
    let mut keys: Vec<_> = region.keys().copied().collect();
    keys.sort_unstable();
    for (tx, tz) in keys {
        let bytes = &region[&(tx, tz)];
        out.extend(tx.to_le_bytes());
        out.extend(tz.to_le_bytes());
        out.extend((bytes.len() as u32).to_le_bytes());
        out.extend(bytes);
    }
    out
}

fn decode_region(bytes: &[u8]) -> Option<Region> {
    if bytes.get(..4)? != MAGIC {
        return None;
    }
    let count = u32::from_le_bytes(bytes.get(4..8)?.try_into().ok()?);
    let mut at = 8;
    let mut region = Region::new();
    for _ in 0..count {
        let head = bytes.get(at..at + 12)?;
        let tx = i32::from_le_bytes(head[0..4].try_into().ok()?);
        let tz = i32::from_le_bytes(head[4..8].try_into().ok()?);
        let len = u32::from_le_bytes(head[8..12].try_into().ok()?) as usize;
        at += 12;
        region.insert((tx, tz), bytes.get(at..at + len)?.to_vec());
        at += len;
    }
    Some(region)
}

#[cfg(test)]
mod tests {
    use super::super::data::tests::sample_tile;
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("rustcraft-lod-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    fn tile_at(level: u8, tx: i32, tz: i32) -> Tile {
        let mut tile = sample_tile();
        tile.key = TileKey::new(level, tx, tz);
        tile
    }

    #[test]
    fn tiles_round_trip_through_region_files() {
        let dir = temp("round");
        let mut store = LodStore::open(&dir, u64::MAX).unwrap();
        let tiles = [tile_at(0, 0, 0), tile_at(0, -1, 9), tile_at(3, 5, -20)];
        for t in &tiles {
            store.put(t);
        }
        store.flush().unwrap();
        let mut again = LodStore::open(&dir, u64::MAX).unwrap();
        for t in &tiles {
            assert_eq!(again.get(t.key).as_ref(), Some(t));
        }
        assert!(again.get(TileKey::new(0, 1, 1)).is_none());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_damaged_region_reads_as_empty() {
        let dir = temp("damaged");
        let mut store = LodStore::open(&dir, u64::MAX).unwrap();
        store.put(&tile_at(0, 0, 0));
        store.flush().unwrap();
        fs::write(dir.join("r.0.0.0.lod"), b"junk").unwrap();
        let mut again = LodStore::open(&dir, u64::MAX).unwrap();
        assert!(again.get(TileKey::new(0, 0, 0)).is_none());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_cap_removes_the_oldest_used_files_first() {
        let dir = temp("cap");
        let mut store = LodStore::open(&dir, u64::MAX).unwrap();
        // Three regions of one level.
        for rx in 0..3 {
            store.put(&tile_at(0, rx * REGION_TILES, 0));
            store.flush().unwrap();
            std::thread::sleep(std::time::Duration::from_millis(30));
        }
        let size = store.usage();
        assert!(size > 0);
        // Use region 0 again, so region 1 is now the oldest.
        assert!(store.get(TileKey::new(0, 0, 0)).is_some());
        store.flush().unwrap();
        store.set_cap(size * 2 / 3 + 1);
        store.flush().unwrap();
        assert!(store.usage() <= size * 2 / 3 + 1);
        assert!(!dir.join("r.0.1.0.lod").exists(), "oldest-used goes first");
        assert!(dir.join("r.0.0.0.lod").exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn clear_empties_the_folder() {
        let dir = temp("clear");
        let mut store = LodStore::open(&dir, u64::MAX).unwrap();
        store.put(&tile_at(1, 0, 0));
        store.flush().unwrap();
        store.clear().unwrap();
        assert_eq!(store.usage(), 0);
        assert!(store.get(TileKey::new(1, 0, 0)).is_none());
        let _ = fs::remove_dir_all(&dir);
    }
}
