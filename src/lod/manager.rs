//! The LOD manager: picks the tiles to draw around the viewer, has worker
//! threads make them (from the store, or the fast path) and mesh them, and
//! hands finished meshes to the renderer.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use crossbeam_channel::{Receiver, Sender};

use super::colors::ColorTable;
use super::data::{Sample, Tile, TileKey};
use super::mesh::{self, Hole, LodMesh, Neighborhood};
use super::sample::{chunk_columns, patch_from_columns};
use super::store::LodStore;
use super::{CHUNK_BLOCKS, MAX_LEVEL, Quality, level_for};
use crate::world::chunk::{Chunk, ChunkPos};

/// Chunks overlap the LOD by this many chunks inside the loaded edge, so
/// the seam is covered by real terrain drawn over it.
pub const OVERLAP_CHUNKS: i32 = 2;
/// Tiles asked of the server and not yet answered, at most.
const MAX_ASKED: usize = 48;

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

enum Job {
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
    pub loaded_from_disk: u64,
}

pub struct LodManager {
    settings: LodSettings,
    store: Arc<Mutex<LodStore>>,
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
}

impl LodManager {
    /// A manager for one viewer: tiles come from the store or, when it
    /// has none, from the server (`take_wanted`, `insert_tile`); its workers
    /// build meshes.
    pub fn new(settings: LodSettings, colors: Arc<ColorTable>, store: LodStore) -> Self {
        Self::build(settings, colors, store)
    }

    fn build(settings: LodSettings, colors: Arc<ColorTable>, store: LodStore) -> Self {
        let (jobs, job_rx) = crossbeam_channel::unbounded::<Job>();
        let (done_tx, done) = crossbeam_channel::unbounded::<Done>();
        let store = Arc::new(Mutex::new(store));
        for n in 0..settings.worker_count() {
            let (job_rx, done_tx) = (job_rx.clone(), done_tx.clone());
            let colors = colors.clone();
            std::thread::Builder::new()
                .name(format!("lod-{n}"))
                .spawn(move || {
                    background();
                    while let Ok(job) = job_rx.recv() {
                        let out = work(job, &colors);
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

    /// Bytes of LOD data in memory: tiles and meshes.
    pub fn memory(&self) -> u64 {
        let tiles: usize = self.tiles.values().map(|t| t.memory()).sum();
        let meshes: usize = self.shown.values().map(|s| s.bytes).sum();
        (tiles + meshes) as u64
    }

    /// Forgets everything held in memory (Off): the store keeps its files.
    pub fn clear(&mut self) -> Vec<TileKey> {
        let gone: Vec<TileKey> = self.shown.keys().copied().collect();
        self.tiles.clear();
        self.asked.clear();
        self.shown.clear();
        self.selected.clear();
        self.last_view = None;
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

    /// A real chunk arrived: its columns replace the server's in the tiles
    /// held now.
    pub fn observe(&mut self, chunk: Arc<Chunk>) {
        self.in_flight += 1;
        let _ = self.jobs.send(Job::Columns(chunk));
    }

    /// Tiles the server should send, nearest `eye` first: the ones
    /// selected and not in memory or on disk.
    pub fn outstanding(&self, eye: (f32, f32)) -> Vec<TileKey> {
        let mut keys: Vec<TileKey> = self.asked.iter().copied().collect();
        keys.sort_by(|a, b| distance(eye, *a).total_cmp(&distance(eye, *b)));
        keys
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
        let budget = self.settings.worker_count() * 3;
        for key in order {
            if self.in_flight >= budget {
                break;
            }
            if !self.tiles.contains_key(&key) {
                if self.asked.len() < MAX_ASKED && self.asked.insert(key) {
                    // The cache on disk first, then the server.
                    let found = self.store.lock().ok().and_then(|mut s| s.get(key));
                    if let Some(tile) = found {
                        self.stats.loaded_from_disk += 1;
                        self.accept(tile);
                        self.asked.remove(&key);
                    }
                }
                continue;
            }
            // Wait for same-level neighbors that are selected and coming.
            let around = neighbors(key);
            let coming = around
                .iter()
                .any(|k| self.selected.contains(k) && !self.tiles.contains_key(k));
            if coming {
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

fn work(job: Job, colors: &ColorTable) -> Option<Done> {
    match job {
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
            mesh: mesh::build(&near, hole, quality, colors),
        }),
        Job::Columns(chunk) => {
            let cols = chunk_columns(&chunk, colors, Quality::High);
            Some(Done::Columns(chunk.pos, cols))
        }
    }
}

/// Runs the thread at low priority, so LOD building doesn't compete with
/// the game.
pub(super) fn background() {
    #[cfg(target_os = "macos")]
    // SAFETY: sets the calling thread's own quality-of-service class.
    unsafe {
        libc::pthread_set_qos_class_self_np(libc::qos_class_t::QOS_CLASS_UTILITY, 0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lod::data::Tile;

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
    fn tiles_arrive_get_meshed_and_off_frees_the_memory() {
        use crate::lod::maker::LodMaker;
        use crate::world::biome::BiomeColors;
        use crate::world::worldgen::terrain::Terrain;
        let dir = std::env::temp_dir().join(format!("rustcraft-lod-view-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let colors = Arc::new(ColorTable::new(None, BiomeColors::uniform()));
        let mut maker = LodMaker::new(
            Arc::new(Terrain::new(12345)),
            colors.clone(),
            2,
            LodStore::open(&dir.join("server"), u64::MAX).unwrap(),
        );
        let store = LodStore::open(&dir.join("client"), u64::MAX).unwrap();
        let mut lod = LodManager::new(settings(40), colors, store);
        let (eye, center) = ((8.0, 8.0), ChunkPos::new(0, 0));
        let mut uploads = 0;
        for _ in 0..4000 {
            uploads += lod.update(eye, center, 9).uploads.len();
            // The server's side, in this process.
            maker.poll();
            for key in lod.outstanding(eye) {
                maker.request(key);
                if let Some(bytes) = maker.get(key) {
                    lod.insert_tile(Tile::decode(&bytes).unwrap());
                }
            }
            if !lod.selected().is_empty() && lod.selected().len() == lod.visible().count() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let stats = lod.stats();
        assert!(uploads > 0 && stats.triangles > 1000, "{stats:?}");
        assert!(stats.memory > 0);
        // Off drops everything held in memory; the files stay.
        lod.flush();
        let gone = lod.clear();
        assert!(!gone.is_empty());
        assert_eq!(lod.memory(), 0);
        assert!(lod.disk_usage() > 0);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
