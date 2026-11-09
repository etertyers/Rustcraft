//! The server's side of Distant Terrain: workers that make tiles from the
//! terrain (or the store), keep real chunks' columns to overlay on them,
//! and hand out the tiles' compressed bytes.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use crossbeam_channel::{Receiver, Sender};

use super::Quality;
use super::colors::ColorTable;
use super::data::{Sample, TileKey};
use super::sample::{chunk_columns, fast_tile, patch_from_columns};
use super::store::LodStore;
use crate::world::chunk::{Chunk, ChunkPos};
use crate::world::worldgen::terrain::Terrain;

/// Most real chunks' columns kept to fill tiles made later.
const COLUMN_CAP: usize = 6000;
/// Finished tiles kept for the next client that asks.
const READY_CAP: usize = 512;
/// Stored tiles at these levels (up to 8 blocks per sample) are patched
/// when a real chunk arrives.
const PATCH_LEVELS: u8 = 3;

type Columns = Arc<Mutex<HashMap<ChunkPos, Arc<Vec<Sample>>>>>;

enum Job {
    Make(TileKey),
    Chunk(Arc<Chunk>),
}

pub struct LodMaker {
    jobs: Sender<Job>,
    done: Receiver<(TileKey, Arc<Vec<u8>>)>,
    store: Arc<Mutex<LodStore>>,
    columns: Columns,
    pending: HashSet<TileKey>,
    ready: HashMap<TileKey, Arc<Vec<u8>>>,
    order: Vec<TileKey>,
}

impl LodMaker {
    /// Workers make tiles of `terrain`'s world. `colors` only classifies
    /// blocks here; the colors are the clients'.
    pub fn new(
        terrain: Arc<Terrain>,
        colors: Arc<ColorTable>,
        threads: usize,
        store: LodStore,
    ) -> Self {
        let (jobs, job_rx) = crossbeam_channel::unbounded::<Job>();
        let (done_tx, done) = crossbeam_channel::unbounded();
        let store = Arc::new(Mutex::new(store));
        let columns: Columns = Arc::new(Mutex::new(HashMap::new()));
        for n in 0..threads.max(1) {
            let (job_rx, done_tx) = (job_rx.clone(), done_tx.clone());
            let (terrain, colors) = (terrain.clone(), colors.clone());
            let (store, columns) = (store.clone(), columns.clone());
            std::thread::Builder::new()
                .name(format!("lod-make-{n}"))
                .spawn(move || {
                    super::manager::background();
                    while let Ok(job) = job_rx.recv() {
                        match job {
                            Job::Make(key) => {
                                let bytes = make(&terrain, key, &store, &columns);
                                if done_tx.send((key, Arc::new(bytes))).is_err() {
                                    break;
                                }
                            }
                            Job::Chunk(chunk) => see(&chunk, &colors, &store, &columns),
                        }
                    }
                })
                .expect("spawn an LOD maker");
        }
        Self {
            jobs,
            done,
            store,
            columns,
            pending: HashSet::new(),
            ready: HashMap::new(),
            order: Vec::new(),
        }
    }

    /// Starts making `key` unless it is made or being made.
    pub fn request(&mut self, key: TileKey) {
        if !self.ready.contains_key(&key) && self.pending.insert(key) {
            let _ = self.jobs.send(Job::Make(key));
        }
    }

    /// Takes in the tiles workers finished.
    pub fn poll(&mut self) {
        while let Ok((key, bytes)) = self.done.try_recv() {
            self.pending.remove(&key);
            if self.ready.insert(key, bytes).is_none() {
                self.order.push(key);
            }
            while self.order.len() > READY_CAP {
                let old = self.order.remove(0);
                self.ready.remove(&old);
            }
        }
    }

    /// The tile's compressed bytes, once made.
    pub fn get(&self, key: TileKey) -> Option<Arc<Vec<u8>>> {
        self.ready.get(&key).cloned()
    }

    pub fn busy(&self) -> usize {
        self.pending.len()
    }

    /// A real chunk generated or loaded.
    pub fn observe(&self, chunk: Arc<Chunk>) {
        let _ = self.jobs.send(Job::Chunk(chunk));
    }

    pub fn disk_usage(&self) -> u64 {
        self.store.lock().map_or(0, |s| s.usage())
    }

    pub fn flush(&self) {
        if let Ok(mut store) = self.store.lock() {
            let _ = store.flush();
        }
    }

    /// Columns of real chunks held (for the tests).
    pub fn known_chunks(&self) -> usize {
        self.columns.lock().map_or(0, |c| c.len())
    }
}

/// A tile from the store, or made fresh with real chunks' columns laid
/// over the terrain's.
fn make(terrain: &Terrain, key: TileKey, store: &Mutex<LodStore>, columns: &Columns) -> Vec<u8> {
    if let Some(tile) = store.lock().ok().and_then(|mut s| s.get(key)) {
        return tile.encode();
    }
    let mut tile = fast_tile(terrain, key, Quality::High);
    let known: Vec<(ChunkPos, Arc<Vec<Sample>>)> = columns
        .lock()
        .map(|c| c.iter().map(|(p, v)| (*p, v.clone())).collect())
        .unwrap_or_default();
    for (pos, cols) in known {
        let (x, z) = pos.min_block();
        if TileKey::containing(key.level, x, z) == key {
            patch_from_columns(&mut tile, pos, &cols);
        }
    }
    if let Ok(mut store) = store.lock() {
        store.put(&tile);
    }
    tile.encode()
}

/// A real chunk: keep its columns, and patch the fine tiles already saved.
fn see(chunk: &Chunk, colors: &ColorTable, store: &Mutex<LodStore>, columns: &Columns) {
    let cols = chunk_columns(chunk, colors, Quality::High);
    let (x, z) = chunk.pos.min_block();
    if let Ok(mut store) = store.lock() {
        for level in 0..=PATCH_LEVELS {
            let key = TileKey::containing(level, x, z);
            if let Some(mut tile) = store.get(key) {
                patch_from_columns(&mut tile, chunk.pos, &cols);
                store.put(&tile);
            }
        }
    }
    if let Ok(mut known) = columns.lock() {
        if known.len() >= COLUMN_CAP
            && let Some(old) = known.keys().next().copied()
        {
            known.remove(&old);
        }
        known.insert(chunk.pos, Arc::new(cols));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lod::data::Tile;
    use crate::world::biome::BiomeColors;

    fn wait(maker: &mut LodMaker, key: TileKey) -> Arc<Vec<u8>> {
        for _ in 0..4000 {
            maker.poll();
            if let Some(bytes) = maker.get(key) {
                return bytes;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        panic!("tile {key:?} never came");
    }

    fn maker(dir: &std::path::Path) -> LodMaker {
        let store = LodStore::open(dir, u64::MAX).unwrap();
        LodMaker::new(
            Arc::new(Terrain::new(12345)),
            Arc::new(ColorTable::new(None, BiomeColors::uniform())),
            2,
            store,
        )
    }

    fn temp(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("rustcraft-lod-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn tiles_are_made_saved_and_reloaded() {
        let dir = temp("maker");
        let key = TileKey::new(2, 0, 0);
        let mut first = maker(&dir);
        first.request(key);
        let bytes = wait(&mut first, key);
        let tile = Tile::decode(&bytes).expect("a good tile");
        assert_eq!(tile.key, key);
        assert!(tile.samples.iter().all(|s| s.count > 0));
        first.flush();
        assert!(first.disk_usage() > 0);
        drop(first);
        // A second maker reads the same bytes back from disk.
        let mut again = maker(&dir);
        again.request(key);
        assert_eq!(*wait(&mut again, key), *bytes);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_real_chunk_replaces_the_fast_samples_in_tiles_made_later() {
        let dir = temp("maker-real");
        let terrain = Terrain::new(12345);
        let chunk = Arc::new(terrain.generate(ChunkPos::new(1, 1)));
        let mut m = maker(&dir);
        m.observe(chunk.clone());
        for _ in 0..2000 {
            if m.known_chunks() == 1 {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let key = TileKey::containing(0, 16, 16);
        m.request(key);
        let tile = Tile::decode(&wait(&mut m, key)).unwrap();
        let real = tile.samples.iter().filter(|s| s.real).count();
        assert_eq!(real, 256, "the chunk's 16×16 columns");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
