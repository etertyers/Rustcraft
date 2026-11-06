//! The LOD manager: picks the tiles to draw around the viewer, has worker
//! threads make them (from the store, or the fast path) and mesh them, and
//! hands finished meshes to the renderer.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use crossbeam_channel::{Receiver, Sender};

use super::colors::ColorTable;
use super::data::{Sample, Tile, TileKey};
use super::mesh::{self, Hole, LodMesh, Neighborhood};
use super::sample::{chunk_columns, fast_tile, patch_from_columns};
use super::store::LodStore;
use super::{CHUNK_BLOCKS, MAX_LEVEL, Quality, level_for};
use crate::world::chunk::{Chunk, ChunkPos};
use crate::world::worldgen::terrain::Terrain;

/// Chunks overlap the LOD by this many chunks inside the loaded edge, so
/// the seam is covered by real terrain drawn over it.
pub const OVERLAP_CHUNKS: i32 = 2;
/// Most real chunks' columns kept to fill tiles made later.
const COLUMN_CAP: usize = 6000;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LodSettings {
    /// LOD distance in chunks.
    pub distance: i32,
    pub quality: Quality,
    /// Worker threads; 0 is Auto (cores − 2, at least 1).
    pub threads: usize,
}

impl LodSettings {
    pub fn worker_count(&self) -> usize {
        if self.threads > 0 {
            return self.threads;
        }
        std::thread::available_parallelism()
            .map_or(2, |n| n.get())
            .saturating_sub(2)
            .max(1)
    }
}

type Columns = Arc<Mutex<HashMap<ChunkPos, Arc<Vec<Sample>>>>>;

enum Job {
    Make(TileKey),
    Mesh {
        key: TileKey,
        epoch: u64,
        near: Box<Neighborhood>,
        hole: Option<Hole>,
        quality: Quality,
    },
    Columns(Arc<Chunk>),
}

enum Done {
    Tile(Tile, bool),
    Mesh {
        key: TileKey,
        epoch: u64,
        hole: Option<Hole>,
        mesh: LodMesh,
    },
    Columns(ChunkPos, Vec<Sample>),
}

struct Shown {
    epoch: u64,
    /// The hole and quality the current (or pending) mesh was made with.
    hole: Option<Hole>,
    quality: Quality,
    /// Whether a mesh job is running for `epoch`.
    pending: bool,
    /// Whether a mesh reached the renderer.
    uploaded: bool,
    triangles: usize,
    bytes: usize,
}

/// What the renderer should do after an update.
#[derive(Default)]
pub struct Frame {
    pub uploads: Vec<(TileKey, LodMesh)>,
    pub removed: Vec<TileKey>,
}

/// Counters for the F3 screen, the tests and the perf notes.
#[derive(Debug, Default, Clone, Copy)]
pub struct Stats {
    pub tiles: usize,
    pub meshes: usize,
    pub triangles: usize,
    pub memory: u64,
    pub in_flight: usize,
    pub made: u64,
    pub loaded_from_disk: u64,
}

pub struct LodManager {
    settings: LodSettings,
    store: Arc<Mutex<LodStore>>,
    columns: Columns,
    jobs: Sender<Job>,
    done: Receiver<Done>,
    tiles: HashMap<TileKey, Arc<Tile>>,
    asked: HashSet<TileKey>,
    shown: HashMap<TileKey, Shown>,
    selected: Vec<TileKey>,
    last_view: Option<(i32, i32, i32, LodSettings)>,
    dirty: bool,
    in_flight: usize,
    next_epoch: u64,
    stats: Stats,
    /// Tiles a client could not build itself and asked its server for.
    remote: bool,
    wanted_remote: Vec<TileKey>,
}

impl LodManager {
    /// A manager whose workers make tiles from `terrain`.
    pub fn new(
        settings: LodSettings,
        terrain: Arc<Terrain>,
        colors: Arc<ColorTable>,
        store: LodStore,
    ) -> Self {
        Self::build(settings, Some(terrain), colors, store)
    }

    /// A client's manager: no terrain, tiles arrive from the server
    /// (`take_wanted`, `insert_tile`).
    pub fn remote(settings: LodSettings, colors: Arc<ColorTable>, store: LodStore) -> Self {
        Self::build(settings, None, colors, store)
    }

    fn build(
        settings: LodSettings,
        terrain: Option<Arc<Terrain>>,
        colors: Arc<ColorTable>,
        store: LodStore,
    ) -> Self {
        let (jobs, job_rx) = crossbeam_channel::unbounded::<Job>();
        let (done_tx, done) = crossbeam_channel::unbounded::<Done>();
        let store = Arc::new(Mutex::new(store));
        let columns: Columns = Arc::new(Mutex::new(HashMap::new()));
        for n in 0..settings.worker_count() {
            let (job_rx, done_tx) = (job_rx.clone(), done_tx.clone());
            let (terrain, colors) = (terrain.clone(), colors.clone());
            let (store, columns) = (store.clone(), columns.clone());
            std::thread::Builder::new()
                .name(format!("lod-{n}"))
                .spawn(move || {
                    background();
                    while let Ok(job) = job_rx.recv() {
                        let out = work(job, terrain.as_deref(), &colors, &store, &columns);
                        if let Some(out) = out
                            && done_tx.send(out).is_err()
                        {
                            break;
                        }
                    }
                })
                .expect("spawn an LOD worker");
        }
        Self {
            settings,
            store,
            columns,
            jobs,
            done,
            tiles: HashMap::new(),
            asked: HashSet::new(),
            shown: HashMap::new(),
            selected: Vec::new(),
            last_view: None,
            dirty: true,
            in_flight: 0,
            next_epoch: 1,
            stats: Stats::default(),
            remote: terrain.is_none(),
            wanted_remote: Vec::new(),
        }
    }

    pub fn settings(&self) -> LodSettings {
        self.settings
    }

    pub fn set_settings(&mut self, settings: LodSettings) {
        if settings.quality != self.settings.quality {
            // Slices and the finest level changed: meshes are stale.
            for shown in self.shown.values_mut() {
                shown.quality = settings.quality;
                shown.epoch = 0;
            }
        }
        self.settings = settings;
        self.dirty = true;
    }

    pub fn stats(&self) -> Stats {
        let mut stats = self.stats;
        stats.tiles = self.tiles.len();
        stats.meshes = self.shown.values().filter(|s| s.uploaded).count();
        stats.triangles = self.shown.values().map(|s| s.triangles).sum();
        stats.in_flight = self.in_flight;
        stats.memory = self.memory();
        stats
    }

    /// Bytes of LOD data in memory: tiles, kept columns and meshes.
    pub fn memory(&self) -> u64 {
        let tiles: usize = self.tiles.values().map(|t| t.memory()).sum();
        let cols = self.columns.lock().map_or(0, |c| c.len())
            * crate::config::CHUNK_SIZE
            * crate::config::CHUNK_SIZE
            * std::mem::size_of::<Sample>();
        let meshes: usize = self.shown.values().map(|s| s.bytes).sum();
        (tiles + cols + meshes) as u64
    }

    /// Forgets everything held in memory (Off): the store keeps its files.
    pub fn clear(&mut self) -> Vec<TileKey> {
        let gone: Vec<TileKey> = self.shown.keys().copied().collect();
        self.tiles.clear();
        self.asked.clear();
        self.shown.clear();
        self.selected.clear();
        self.last_view = None;
        self.wanted_remote.clear();
        if let Ok(mut columns) = self.columns.lock() {
            columns.clear();
        }
        gone
    }

    pub fn disk_usage(&self) -> u64 {
        self.store.lock().map_or(0, |s| s.usage())
    }

    pub fn flush(&self) {
        if let Ok(mut store) = self.store.lock() {
            let _ = store.flush();
        }
    }

    /// A real chunk was generated or loaded: its columns replace the fast
    /// ones in tiles made now and later.
    pub fn observe(&mut self, chunk: Arc<Chunk>) {
        if self.remote {
            return;
        }
        self.in_flight += 1;
        let _ = self.jobs.send(Job::Columns(chunk));
    }

    /// Tiles a client wants its server to send (cleared by the call).
    pub fn take_wanted(&mut self) -> Vec<TileKey> {
        std::mem::take(&mut self.wanted_remote)
    }

    /// A tile that arrived from the server.
    pub fn insert_tile(&mut self, tile: Tile) {
        let key = tile.key;
        if let Ok(mut store) = self.store.lock() {
            store.put(&tile);
        }
        self.accept(tile);
        self.asked.remove(&key);
    }

    fn accept(&mut self, tile: Tile) {
        let key = tile.key;
        self.tiles.insert(key, Arc::new(tile));
        self.touch(key);
    }

    /// Tiles around `key` need new meshes.
    fn touch(&mut self, key: TileKey) {
        let (tx, tz) = (key.tx, key.tz);
        for (dx, dz) in [(0, 0), (-1, 0), (1, 0), (0, -1), (0, 1)] {
            let k = TileKey::new(key.level, tx + dx, tz + dz);
            if let Some(shown) = self.shown.get_mut(&k) {
                shown.epoch = 0;
            }
        }
        self.dirty = true;
    }

    /// Advances one frame. `eye` is the viewer in blocks; `loaded` the
    /// radius of the loaded chunks and `center` the viewer's chunk.
    pub fn update(&mut self, eye: (f32, f32), center: ChunkPos, loaded: i32) -> Frame {
        let mut frame = Frame::default();
        self.drain(&mut frame);
        let view = (center.x, center.z, loaded, self.settings);
        if self.last_view != Some(view) {
            self.last_view = Some(view);
            self.dirty = true;
        }
        if self.dirty {
            self.dirty = false;
            let hole = hole_for(center, loaded);
            self.selected = select(eye, loaded, self.settings, hole);
            let keep: HashSet<TileKey> = self.selected.iter().copied().collect();
            let stale: Vec<TileKey> = self
                .shown
                .keys()
                .filter(|k| !keep.contains(k))
                .copied()
                .collect();
            for key in stale {
                if self.shown.remove(&key).is_some_and(|s| s.uploaded) {
                    frame.removed.push(key);
                }
            }
            self.tiles.retain(|k, _| keep.contains(k));
            self.asked.retain(|k| keep.contains(k));
        }
        let hole = hole_for(center, loaded);
        // Nearest first.
        let mut order = self.selected.clone();
        order.sort_by(|a, b| {
            let da = distance(eye, *a);
            let db = distance(eye, *b);
            da.total_cmp(&db)
        });
        let budget = if self.remote {
            64
        } else {
            self.settings.worker_count() * 3
        };
        for key in order {
            if self.in_flight >= budget {
                break;
            }
            if !self.tiles.contains_key(&key) {
                if self.asked.insert(key) {
                    if self.remote {
                        self.wanted_remote.push(key);
                        // Disk first.
                        let found = self.store.lock().ok().and_then(|mut s| s.get(key));
                        if let Some(tile) = found {
                            self.accept(tile);
                            self.stats.loaded_from_disk += 1;
                            self.asked.remove(&key);
                            self.wanted_remote.retain(|k| *k != key);
                        }
                    } else {
                        self.in_flight += 1;
                        let _ = self.jobs.send(Job::Make(key));
                    }
                }
                continue;
            }
            // Wait for same-level neighbors that are selected and coming.
            let around = neighbors(key);
            let coming = around
                .iter()
                .any(|k| self.selected.contains(k) && !self.tiles.contains_key(k));
            if coming && !self.remote {
                continue;
            }
            let wanted_hole = hole.filter(|h| h.touches(key));
            let quality = self.settings.quality;
            let shown = self.shown.entry(key).or_insert(Shown {
                epoch: 0,
                hole: None,
                quality,
                pending: false,
                uploaded: false,
                triangles: 0,
                bytes: 0,
            });
            let fresh = shown.epoch != 0 && shown.hole == wanted_hole && shown.quality == quality;
            if fresh || shown.pending {
                if shown.pending && shown.hole != wanted_hole {
                    // The running job is for another hole: queue again.
                } else {
                    continue;
                }
            }
            self.next_epoch += 1;
            shown.epoch = self.next_epoch;
            shown.hole = wanted_hole;
            shown.quality = quality;
            shown.pending = true;
            let epoch = shown.epoch;
            let near = Neighborhood {
                tile: self.tiles[&key].clone(),
                around: around.map(|k| self.tiles.get(&k).cloned()),
            };
            self.in_flight += 1;
            let _ = self.jobs.send(Job::Mesh {
                key,
                epoch,
                near: Box::new(near),
                hole: wanted_hole,
                quality,
            });
        }
        frame
    }

    fn drain(&mut self, frame: &mut Frame) {
        while let Ok(done) = self.done.try_recv() {
            self.in_flight = self.in_flight.saturating_sub(1);
            match done {
                Done::Tile(tile, from_disk) => {
                    self.stats.made += 1;
                    self.stats.loaded_from_disk += u64::from(from_disk);
                    let key = tile.key;
                    self.asked.remove(&key);
                    if self.selected.contains(&key) {
                        self.accept(tile);
                    }
                }
                Done::Mesh {
                    key,
                    epoch,
                    hole,
                    mesh,
                } => {
                    let Some(shown) = self.shown.get_mut(&key) else {
                        continue;
                    };
                    if shown.epoch != epoch {
                        shown.pending = false;
                        continue;
                    }
                    shown.pending = false;
                    shown.hole = hole;
                    shown.uploaded = true;
                    shown.triangles = mesh.triangles();
                    shown.bytes = mesh.bytes();
                    frame.uploads.push((key, mesh));
                }
                Done::Columns(pos, columns) => {
                    self.patch_resident(pos, &columns);
                }
            }
        }
    }

    /// A real chunk's columns into the tiles held in memory.
    fn patch_resident(&mut self, pos: ChunkPos, columns: &[Sample]) {
        let keys: Vec<TileKey> = (0..=MAX_LEVEL)
            .map(|level| {
                let (x, z) = pos.min_block();
                TileKey::containing(level, x, z)
            })
            .filter(|k| self.tiles.contains_key(k))
            .collect();
        for key in keys {
            if let Some(tile) = self.tiles.get_mut(&key) {
                let tile = Arc::make_mut(tile);
                if patch_from_columns(tile, pos, columns) > 0
                    && let Ok(mut store) = self.store.lock()
                {
                    store.put(tile);
                }
            }
            self.touch(key);
        }
    }

    /// The tiles to draw now, nearest first.
    pub fn visible(&self) -> impl Iterator<Item = TileKey> + '_ {
        self.selected
            .iter()
            .copied()
            .filter(|k| self.shown.get(k).is_some_and(|s| s.uploaded))
    }

    pub fn selected(&self) -> &[TileKey] {
        &self.selected
    }
}

fn neighbors(key: TileKey) -> [TileKey; 4] {
    [(-1, 0), (1, 0), (0, -1), (0, 1)]
        .map(|(dx, dz)| TileKey::new(key.level, key.tx + dx, key.tz + dz))
}

fn distance(eye: (f32, f32), key: TileKey) -> f32 {
    let c = key.center();
    (c.0 - eye.0).hypot(c.1 - eye.1)
}

/// The blocks the LOD leaves to the loaded chunks: the loaded square, less
/// `OVERLAP_CHUNKS` on each side.
pub fn hole_for(center: ChunkPos, loaded: i32) -> Option<Hole> {
    let r = (loaded - OVERLAP_CHUNKS).max(0);
    if r == 0 {
        return None;
    }
    Some(Hole {
        min: ((center.x - r) * CHUNK_BLOCKS, (center.z - r) * CHUNK_BLOCKS),
        max: (
            (center.x + r + 1) * CHUNK_BLOCKS,
            (center.z + r + 1) * CHUNK_BLOCKS,
        ),
    })
}

/// The tiles that cover the ring from the loaded area out to the LOD
/// distance, as a quadtree: each tile is split until it is no coarser than
/// the level its distance allows. Tiles wholly inside the hole are left
/// out.
pub fn select(
    eye: (f32, f32),
    loaded: i32,
    settings: LodSettings,
    hole: Option<Hole>,
) -> Vec<TileKey> {
    let reach = (settings.distance * CHUNK_BLOCKS) as f32;
    let near = (loaded * CHUNK_BLOCKS) as f32;
    let max_span = (super::TILE_SAMPLES as i32) << MAX_LEVEL;
    let (lo_x, hi_x) = (
        ((eye.0 - reach) / max_span as f32).floor() as i32,
        ((eye.0 + reach) / max_span as f32).floor() as i32,
    );
    let (lo_z, hi_z) = (
        ((eye.1 - reach) / max_span as f32).floor() as i32,
        ((eye.1 + reach) / max_span as f32).floor() as i32,
    );
    let mut out = Vec::new();
    let mut stack: Vec<TileKey> = (lo_x..=hi_x)
        .flat_map(|tx| (lo_z..=hi_z).map(move |tz| TileKey::new(MAX_LEVEL, tx, tz)))
        .collect();
    while let Some(key) = stack.pop() {
        let (ox, oz) = key.origin();
        let span = key.span() as f32;
        // Distance from the eye to the tile's nearest point (square norm).
        let dx = (ox as f32 - eye.0).max(eye.0 - (ox as f32 + span)).max(0.0);
        let dz = (oz as f32 - eye.1).max(eye.1 - (oz as f32 + span)).max(0.0);
        let d = dx.max(dz);
        if d >= reach {
            continue;
        }
        if let Some(h) = hole
            && ox >= h.min.0
            && oz >= h.min.1
            && ox + key.span() <= h.max.0
            && oz + key.span() <= h.max.1
        {
            continue;
        }
        let allowed = level_for(d, near, settings.quality);
        if key.level <= allowed {
            out.push(key);
        } else {
            for (cx, cz) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                stack.push(TileKey::new(
                    key.level - 1,
                    key.tx * 2 + cx,
                    key.tz * 2 + cz,
                ));
            }
        }
    }
    out
}

fn work(
    job: Job,
    terrain: Option<&Terrain>,
    colors: &ColorTable,
    store: &Mutex<LodStore>,
    columns: &Columns,
) -> Option<Done> {
    match job {
        Job::Make(key) => {
            let saved = store.lock().ok().and_then(|mut s| s.get(key));
            if let Some(tile) = saved {
                return Some(Done::Tile(tile, true));
            }
            let terrain = terrain?;
            let mut tile = fast_tile(terrain, colors, key, Quality::High);
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
            Some(Done::Tile(tile, false))
        }
        Job::Mesh {
            key,
            epoch,
            near,
            hole,
            quality,
        } => Some(Done::Mesh {
            key,
            epoch,
            hole,
            mesh: mesh::build(&near, hole, quality),
        }),
        Job::Columns(chunk) => {
            let cols = chunk_columns(&chunk, colors, Quality::High);
            if let Ok(mut known) = columns.lock() {
                if known.len() >= COLUMN_CAP
                    && let Some(old) = known.keys().next().copied()
                {
                    known.remove(&old);
                }
                known.insert(chunk.pos, Arc::new(cols.clone()));
            }
            Some(Done::Columns(chunk.pos, cols))
        }
    }
}

/// Runs the thread at low priority, so LOD building doesn't compete with
/// the game.
fn background() {
    #[cfg(target_os = "macos")]
    // SAFETY: sets the calling thread's own quality-of-service class.
    unsafe {
        libc::pthread_set_qos_class_self_np(libc::qos_class_t::QOS_CLASS_UTILITY, 0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(distance: i32) -> LodSettings {
        LodSettings {
            distance,
            quality: Quality::Medium,
            threads: 2,
        }
    }

    #[test]
    fn selected_tiles_never_overlap_and_leave_no_gap_outside_the_hole() {
        let eye = (5.0, -300.0);
        let center = ChunkPos::of_block(5, -300);
        let hole = hole_for(center, 9);
        let tiles = select(eye, 9, settings(128), hole);
        assert!(tiles.len() > 20);
        // Sample the plane on a grid: each point inside the distance and
        // outside the hole is in exactly one tile; inside the hole in none
        // (wholly-inside tiles are skipped).
        let reach = 128.0 * 16.0;
        let mut checked = 0;
        for xi in -60..60 {
            for zi in -60..60 {
                let (x, z) = (eye.0 + xi as f32 * 31.7, eye.1 + zi as f32 * 31.7);
                if (x - eye.0).abs() >= reach - 40.0 || (z - eye.1).abs() >= reach - 40.0 {
                    continue;
                }
                let (bx, bz) = (x.floor() as i32, z.floor() as i32);
                let n = tiles
                    .iter()
                    .filter(|t| {
                        let (ox, oz) = t.origin();
                        (ox..ox + t.span()).contains(&bx) && (oz..oz + t.span()).contains(&bz)
                    })
                    .count();
                assert!(n <= 1, "{bx},{bz} is in {n} tiles");
                let h = hole.unwrap();
                let far_from_hole = bx < h.min.0 - 2048
                    || bx >= h.max.0 + 2048
                    || bz < h.min.1 - 2048
                    || bz >= h.max.1 + 2048;
                if far_from_hole {
                    assert_eq!(n, 1, "gap at {bx},{bz}");
                }
                checked += 1;
            }
        }
        assert!(checked > 1000);
    }

    #[test]
    fn levels_get_coarser_with_distance() {
        let eye = (0.0, 0.0);
        let tiles = select(eye, 9, settings(256), hole_for(ChunkPos::new(0, 0), 9));
        let level_at = |d: f32| {
            tiles
                .iter()
                .filter(|t| {
                    let c = t.center();
                    (c.0.abs().max(c.1.abs()) - d).abs() < t.span() as f32 / 2.0
                })
                .map(|t| t.level)
                .min()
        };
        let near = level_at(200.0).unwrap();
        let far = level_at(3000.0).unwrap();
        assert!(near < far, "near {near}, far {far}");
        assert!(
            tiles
                .iter()
                .all(|t| t.level >= Quality::Medium.finest_level())
        );
    }

    #[test]
    fn the_hole_leaves_the_loaded_chunks_to_real_terrain() {
        let hole = hole_for(ChunkPos::new(3, -4), 9).unwrap();
        // Loaded chunks 3±9 are covered by real terrain; the hole stops two
        // chunks inside so real terrain overlaps the LOD.
        assert_eq!(hole.min, ((3 - 7) * 16, (-4 - 7) * 16));
        assert_eq!(hole.max, ((3 + 8) * 16, (-4 + 8) * 16));
        assert!(hole_for(ChunkPos::new(0, 0), 2).is_none());
    }

    #[test]
    fn tiles_are_made_meshed_saved_and_freed_when_off() {
        use crate::world::biome::BiomeColors;
        let dir = std::env::temp_dir().join(format!("rustcraft-lod-mgr-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let terrain = Arc::new(Terrain::new(12345));
        let colors = Arc::new(ColorTable::new(None, BiomeColors::uniform()));
        let store = LodStore::open(&dir, u64::MAX).unwrap();
        let mut lod = LodManager::new(settings(40), terrain.clone(), colors.clone(), store);
        let center = ChunkPos::new(0, 0);
        let mut uploaded = 0;
        for _ in 0..4000 {
            let frame = lod.update((8.0, 8.0), center, 9);
            uploaded += frame.uploads.len();
            if lod.selected().len() == lod.visible().count() && !lod.selected().is_empty() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let stats = lod.stats();
        assert!(uploaded > 0 && stats.triangles > 1000, "{stats:?}");
        assert!(stats.memory > 0);
        lod.flush();
        assert!(lod.disk_usage() > 0);
        // Off frees the memory; the files stay.
        let gone = lod.clear();
        assert!(!gone.is_empty());
        assert_eq!(lod.memory(), 0);
        assert!(lod.disk_usage() > 0);
        drop(lod);
        // A new manager loads the saved tiles instead of making them.
        let store = LodStore::open(&dir, u64::MAX).unwrap();
        let mut again = LodManager::new(settings(40), terrain, colors, store);
        for _ in 0..4000 {
            again.update((8.0, 8.0), center, 9);
            if again.selected().len() == again.visible().count() && !again.selected().is_empty() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(again.stats().loaded_from_disk > 0, "{:?}", again.stats());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
