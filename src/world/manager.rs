//! Streams chunks in and out around the player, generating and meshing them
//! on worker threads.
//!
//! Each frame `update` collects finished work, unloads far chunks and hands
//! out new jobs nearest-first. The main thread never waits on a worker.
//!
//! Mesh jobs also compute the chunk's light (see `light`). Block edits
//! remesh the chunks whose blocks or faces changed right away on the main
//! thread, so the change shows the same frame; other chunks within light
//! range are relit in the background. A mesh job that was already running
//! for an edited chunk is discarded when it returns.
//!
//! With a region store, workers load saved chunks instead of generating
//! them, and `save` writes the chunks changed since the last save.

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    thread::JoinHandle,
};

use glam::IVec3;

use crossbeam_channel::{Receiver, Sender, TryRecvError};
use log::error;

use crate::config::{CHUNK_SIZE, WORLD_HEIGHT};

use super::{
    World,
    block::BlockState,
    block_entity::BlockEntity,
    chunk::{Chunk, ChunkPos, LocalPos},
    light::{ChunkLight, light_chunk},
    mesh::{BlockModels, ChunkArea, Mesh, mesh_chunk},
    save::{RegionStore, SaveError},
    worldgen::{flat, terrain::Terrain},
};

/// Where new chunks come from.
pub enum ChunkSource {
    Terrain(Box<Terrain>),
    Flat,
    /// Fixed chunks (the block gallery); anywhere else is empty.
    Fixed(HashMap<ChunkPos, Chunk>),
}

impl ChunkSource {
    fn generate(&self, pos: ChunkPos) -> Chunk {
        match self {
            ChunkSource::Terrain(terrain) => terrain.generate(pos),
            ChunkSource::Flat => flat::flat_chunk(pos),
            ChunkSource::Fixed(chunks) => {
                chunks.get(&pos).cloned().unwrap_or_else(|| Chunk::new(pos))
            }
        }
    }

    pub fn seed(&self) -> Option<u64> {
        match self {
            ChunkSource::Terrain(terrain) => Some(terrain.seed()),
            _ => None,
        }
    }

    /// The saved chunk if there is one, else a newly generated one.
    fn load_or_generate(&self, store: Option<&RegionStore>, pos: ChunkPos) -> Chunk {
        match store.map(|store| store.load(pos)) {
            Some(Ok(Some(chunk))) => chunk,
            Some(Err(err)) => {
                error!("can't load chunk {pos:?}, generating it instead: {err}");
                self.generate(pos)
            }
            Some(Ok(None)) | None => self.generate(pos),
        }
    }
}

enum Job {
    Generate(ChunkPos),
    /// The chunk's edit revision when the job was sent.
    Mesh(ChunkPos, ChunkArea, u32),
}

enum Done {
    Generated(Box<Chunk>),
    Meshed(ChunkPos, Mesh, ChunkLight, u32),
}

/// What changed this frame, for the renderer.
#[derive(Default)]
pub struct Changes {
    pub meshes: Vec<(ChunkPos, Mesh)>,
    pub unloaded: Vec<ChunkPos>,
    /// Chunks that entered the world (generated, loaded or brought back).
    pub loaded: Vec<ChunkPos>,
}

pub struct ChunkManager {
    world: World,
    source: Arc<ChunkSource>,
    store: Option<Arc<RegionStore>>,
    models: Arc<BlockModels>,
    /// `None` only while shutting down.
    jobs: Option<Sender<Job>>,
    done: Receiver<Done>,
    workers: Vec<JoinHandle<()>>,
    generating: HashSet<ChunkPos>,
    meshing: HashSet<ChunkPos>,
    meshed: HashSet<ChunkPos>,
    /// Chunks within this distance (in chunks, per axis) are meshed and drawn.
    radius: i32,
    /// Offsets within `radius + 1`, nearest first.
    offsets: Vec<(i32, i32)>,
    max_in_flight: usize,
    /// Bumped on every edit so stale mesh jobs can be recognized.
    revisions: HashMap<ChunkPos, u32>,
    /// Chunks changed since they were last saved (every changed chunk when
    /// there's no store). They're kept in memory when unloaded, since
    /// loading or regenerating them would lose the changes.
    unsaved: HashSet<ChunkPos>,
    unloaded_edits: HashMap<ChunkPos, Arc<Chunk>>,
    /// Meshes rebuilt by edits, handed out by the next `update`.
    remeshed: Vec<(ChunkPos, Mesh)>,
    /// Drawn chunks whose light changed, waiting for a background remesh.
    relight: HashSet<ChunkPos>,
    /// Light of every drawn chunk.
    lights: HashMap<ChunkPos, ChunkLight>,
}

/// The chunk holding block `at` and its position there, if `at` is inside
/// the world's height.
fn local_pos(at: IVec3) -> Option<(ChunkPos, LocalPos)> {
    if !(0..WORLD_HEIGHT as i32).contains(&at.y) {
        return None;
    }
    let size = CHUNK_SIZE as i32;
    let local = [at.x.rem_euclid(size), at.y, at.z.rem_euclid(size)].map(|v| v as u8);
    Some((ChunkPos::of_block(at.x, at.z), local))
}

/// Farthest a light change reaches (a level-15 source fades out after 14 blocks).
const LIGHT_REACH: i32 = 14;

/// Finished jobs collected per frame, so a burst can't stall one frame.
const MAX_RESULTS_PER_FRAME: usize = 64;

impl ChunkManager {
    /// Starts `workers` threads. Chunks are generated one ring beyond
    /// `radius` so border chunks can be meshed against their neighbors.
    pub fn new(source: ChunkSource, models: Arc<BlockModels>, radius: i32, workers: usize) -> Self {
        Self::start(source, None, models, radius, workers)
    }

    /// Like `new`, but saved chunks come from `store`, and `save` writes there.
    pub fn with_store(
        source: ChunkSource,
        store: RegionStore,
        models: Arc<BlockModels>,
        radius: i32,
        workers: usize,
    ) -> Self {
        Self::start(source, Some(store), models, radius, workers)
    }

    fn start(
        source: ChunkSource,
        store: Option<RegionStore>,
        models: Arc<BlockModels>,
        radius: i32,
        workers: usize,
    ) -> Self {
        let source = Arc::new(source);
        let store = store.map(Arc::new);
        let (job_tx, job_rx) = crossbeam_channel::unbounded::<Job>();
        let (done_tx, done_rx) = crossbeam_channel::unbounded();
        let workers = (0..workers.max(1))
            .map(|i| {
                let (jobs, done, source, store, worker_models) = (
                    job_rx.clone(),
                    done_tx.clone(),
                    source.clone(),
                    store.clone(),
                    models.clone(),
                );
                let models = worker_models;
                std::thread::Builder::new()
                    .name(format!("chunk-worker-{i}"))
                    .spawn(move || {
                        for job in jobs {
                            let result = match job {
                                Job::Generate(pos) => Done::Generated(Box::new(
                                    source.load_or_generate(store.as_deref(), pos),
                                )),
                                Job::Mesh(pos, area, revision) => {
                                    let light = light_chunk(&models, &area);
                                    let mesh = mesh_chunk(&models, &area, &light);
                                    Done::Meshed(pos, mesh, light, revision)
                                }
                            };
                            if done.send(result).is_err() {
                                break;
                            }
                        }
                    })
                    .expect("spawning a chunk worker thread")
            })
            .collect::<Vec<_>>();

        let reach = radius + 1;
        let mut offsets: Vec<(i32, i32)> = (-reach..=reach)
            .flat_map(|dz| (-reach..=reach).map(move |dx| (dx, dz)))
            .collect();
        offsets.sort_by_key(|&(dx, dz)| dx * dx + dz * dz);

        Self {
            world: World::default(),
            source,
            store,
            models,
            jobs: Some(job_tx),
            done: done_rx,
            max_in_flight: workers.len() * 2,
            workers,
            generating: HashSet::new(),
            meshing: HashSet::new(),
            meshed: HashSet::new(),
            radius,
            offsets,
            revisions: HashMap::new(),
            unsaved: HashSet::new(),
            unloaded_edits: HashMap::new(),
            remeshed: Vec::new(),
            relight: HashSet::new(),
            lights: HashMap::new(),
        }
    }

    pub fn world(&self) -> &World {
        &self.world
    }

    pub fn seed(&self) -> Option<u64> {
        self.source.seed()
    }

    fn in_flight(&self) -> usize {
        self.generating.len() + self.meshing.len()
    }

    /// Chebyshev distance in chunks.
    fn distance(a: ChunkPos, b: ChunkPos) -> i32 {
        (a.x - b.x).abs().max((a.z - b.z).abs())
    }

    /// Collects finished work, unloads far chunks and queues new jobs around `center`.
    pub fn update(&mut self, center: ChunkPos) -> Changes {
        let mut changes = Changes {
            meshes: std::mem::take(&mut self.remeshed),
            unloaded: Vec::new(),
            loaded: Vec::new(),
        };

        for _ in 0..MAX_RESULTS_PER_FRAME {
            match self.done.try_recv() {
                Ok(Done::Generated(chunk)) => {
                    self.generating.remove(&chunk.pos);
                    if Self::distance(chunk.pos, center) <= self.radius + 1 {
                        changes.loaded.push(chunk.pos);
                        self.world.insert(*chunk);
                    }
                }
                Ok(Done::Meshed(pos, mesh, light, revision)) => {
                    self.meshing.remove(&pos);
                    // An edit since the job started makes it stale; it's requeued.
                    if self.world.get(pos).is_some() && revision == self.revision(pos) {
                        self.meshed.insert(pos);
                        self.relight.remove(&pos);
                        self.lights.insert(pos, light);
                        changes.meshes.push((pos, mesh));
                    }
                }
                Err(TryRecvError::Empty | TryRecvError::Disconnected) => break,
            }
        }

        // Keep one extra ring loaded so walking back and forth across a
        // border doesn't regenerate chunks.
        let keep = self.radius + 2;
        let far: Vec<ChunkPos> = self
            .world
            .positions()
            .filter(|&pos| Self::distance(pos, center) > keep)
            .collect();
        for pos in far {
            if let Some(chunk) = self.world.remove(pos)
                && self.unsaved.contains(&pos)
            {
                self.unloaded_edits.insert(pos, chunk);
            }
            if self.meshed.remove(&pos) {
                changes.unloaded.push(pos);
            }
            self.relight.remove(&pos);
            self.lights.remove(&pos);
        }
        // Meshes outside the draw radius go too (their chunk stays loaded).
        let hidden: Vec<ChunkPos> = self
            .meshed
            .iter()
            .copied()
            .filter(|&pos| Self::distance(pos, center) > self.radius)
            .collect();
        for pos in hidden {
            self.meshed.remove(&pos);
            self.relight.remove(&pos);
            self.lights.remove(&pos);
            changes.unloaded.push(pos);
        }

        let Some(jobs) = self.jobs.clone() else {
            return changes;
        };
        for &(dx, dz) in &self.offsets {
            if self.in_flight() >= self.max_in_flight {
                break;
            }
            let pos = center.offset(dx, dz);
            if self.world.get(pos).is_none() {
                if let Some(chunk) = self.unloaded_edits.remove(&pos) {
                    changes.loaded.push(pos);
                    self.world.insert_shared(chunk);
                } else if self.generating.insert(pos) {
                    let _ = jobs.send(Job::Generate(pos));
                }
            } else if dx.abs().max(dz.abs()) <= self.radius
                && (!self.meshed.contains(&pos) || self.relight.contains(&pos))
                && !self.meshing.contains(&pos)
                && self.neighbors_loaded(pos)
                && let Some(area) = self.world.area(pos)
            {
                self.meshing.insert(pos);
                let _ = jobs.send(Job::Mesh(pos, area, self.revision(pos)));
            }
        }
        changes
    }

    fn revision(&self, pos: ChunkPos) -> u32 {
        self.revisions.get(&pos).copied().unwrap_or(0)
    }

    /// (sky, block) light at a world position, if its chunk is drawn.
    pub fn light_at(&self, x: i32, y: i32, z: i32) -> Option<(u8, u8)> {
        let size = CHUNK_SIZE as i32;
        let light = self.lights.get(&ChunkPos::of_block(x, z))?;
        Some(light.get(x.rem_euclid(size), y, z.rem_euclid(size)))
    }

    /// Applies block edits in loaded chunks (others are skipped). Each
    /// touched chunk, plus the neighbor across any border an edit sits on,
    /// is remeshed now; chunks only within light range are relit soon after.
    pub fn set_blocks(&mut self, edits: &[(IVec3, BlockState)]) {
        let size = CHUNK_SIZE as i32;
        let mut touched = HashSet::new();
        let mut lit = HashSet::new();
        for &(at, block) in edits {
            if !(0..WORLD_HEIGHT as i32).contains(&at.y) {
                continue;
            }
            let pos = ChunkPos::of_block(at.x, at.z);
            let Some(chunk) = self.world.get_mut(pos) else {
                continue;
            };
            let (x, z) = (at.x.rem_euclid(size), at.z.rem_euclid(size));
            chunk.set(x as usize, at.y as usize, z as usize, block);
            self.unsaved.insert(pos);
            touched.insert(pos);
            let low = ChunkPos::of_block(at.x - LIGHT_REACH, at.z - LIGHT_REACH);
            let high = ChunkPos::of_block(at.x + LIGHT_REACH, at.z + LIGHT_REACH);
            for cz in low.z..=high.z {
                for cx in low.x..=high.x {
                    lit.insert(ChunkPos::new(cx, cz));
                }
            }
            for (edge, dx, dz) in [
                (x == 0, -1, 0),
                (x == size - 1, 1, 0),
                (z == 0, 0, -1),
                (z == size - 1, 0, 1),
            ] {
                if edge {
                    touched.insert(pos.offset(dx, dz));
                }
            }
        }
        for &pos in &touched {
            *self.revisions.entry(pos).or_default() += 1;
            // Chunks not drawn yet get meshed by `update` as usual.
            if self.meshed.contains(&pos)
                && let Some(area) = self.world.area(pos)
            {
                let light = light_chunk(&self.models, &area);
                self.remeshed
                    .push((pos, mesh_chunk(&self.models, &area, &light)));
                self.lights.insert(pos, light);
                self.relight.remove(&pos);
            }
        }
        for pos in lit.difference(&touched) {
            *self.revisions.entry(*pos).or_default() += 1;
            if self.meshed.contains(pos) {
                self.relight.insert(*pos);
            }
        }
    }

    /// The block entity at `at`, if its chunk is loaded and it has one.
    pub fn block_entity(&self, at: IVec3) -> Option<&BlockEntity> {
        let (pos, local) = local_pos(at)?;
        self.world.get(pos)?.block_entity(local)
    }

    /// The block entity at `at` for changing; its chunk will be saved.
    pub fn block_entity_mut(&mut self, at: IVec3) -> Option<&mut BlockEntity> {
        let (pos, local) = local_pos(at)?;
        let chunk = self.world.get_mut(pos)?;
        chunk.block_entity(local)?;
        self.unsaved.insert(pos);
        chunk.block_entity_mut(local)
    }

    /// Puts or removes the block entity at `at` in a loaded chunk, returning
    /// the one there before.
    pub fn set_block_entity(
        &mut self,
        at: IVec3,
        entity: Option<BlockEntity>,
    ) -> Option<BlockEntity> {
        let (pos, local) = local_pos(at)?;
        let chunk = self.world.get_mut(pos)?;
        self.unsaved.insert(pos);
        chunk.set_block_entity(local, entity)
    }

    /// Writes every chunk changed since the last save to the store, and
    /// returns how many there were. Without a store this does nothing.
    pub fn save(&mut self) -> Result<usize, SaveError> {
        let Some(store) = &self.store else {
            return Ok(0);
        };
        let chunks: Vec<&Chunk> = self
            .unsaved
            .iter()
            .filter_map(|pos| {
                let chunk = self.world.get(*pos).or(self.unloaded_edits.get(pos))?;
                Some(&**chunk)
            })
            .collect();
        let count = chunks.len();
        store.save(chunks)?;
        // Saved chunks that aren't loaded will be read back from the store.
        for pos in self.unsaved.drain() {
            self.unloaded_edits.remove(&pos);
        }
        Ok(count)
    }

    fn neighbors_loaded(&self, pos: ChunkPos) -> bool {
        (-1..=1).all(|dz| (-1..=1).all(|dx| self.world.get(pos.offset(dx, dz)).is_some()))
    }
}

impl Drop for ChunkManager {
    fn drop(&mut self) {
        // Closing the job channel ends each worker's loop.
        self.jobs = None;
        for worker in self.workers.drain(..) {
            let _ = worker.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::{biome::BiomeColors, block::BlockRegistry, save::TempDir};
    use std::time::{Duration, Instant};

    fn models() -> Arc<BlockModels> {
        let registry = BlockRegistry::load(&crate::assets::dir().join("data/blocks.ron")).unwrap();
        Arc::new(BlockModels::new(
            &registry,
            |_| Some([0.0; 4]),
            [0.0; 4],
            BiomeColors::uniform(),
        ))
    }

    /// Calls `update` until `done` says so, collecting every change.
    fn run_until(
        manager: &mut ChunkManager,
        center: ChunkPos,
        done: impl Fn(&ChunkManager, &[ChunkPos]) -> bool,
    ) -> (Vec<ChunkPos>, Vec<ChunkPos>) {
        let (mut meshed, mut unloaded) = (Vec::new(), Vec::new());
        let deadline = Instant::now() + Duration::from_secs(20);
        while !done(manager, &meshed) {
            assert!(Instant::now() < deadline, "chunks never finished loading");
            let changes = manager.update(center);
            meshed.extend(changes.meshes.into_iter().map(|(pos, _)| pos));
            unloaded.extend(changes.unloaded);
            std::thread::sleep(Duration::from_millis(1));
        }
        (meshed, unloaded)
    }

    #[test]
    fn loads_and_meshes_everything_within_the_radius_once() {
        let mut manager = ChunkManager::new(ChunkSource::Flat, models(), 2, 3);
        let center = ChunkPos::new(10, -4);
        let (meshed, _) = run_until(&mut manager, center, |m, meshed| {
            meshed.len() >= 25 && m.in_flight() == 0
        });
        let unique: HashSet<_> = meshed.iter().copied().collect();
        assert_eq!(meshed.len(), 25, "a chunk was meshed twice");
        assert!(
            unique
                .iter()
                .all(|&pos| ChunkManager::distance(pos, center) <= 2)
        );
        // Generated one ring further than meshed, for neighbor culling.
        assert_eq!(manager.world().len(), 49);
    }

    #[test]
    fn nearest_chunks_are_meshed_first() {
        let mut manager = ChunkManager::new(ChunkSource::Flat, models(), 4, 1);
        let center = ChunkPos::new(0, 0);
        let (meshed, _) = run_until(&mut manager, center, |_, meshed| meshed.len() >= 81);
        let first_ring: Vec<i32> = meshed[..9]
            .iter()
            .map(|&p| ChunkManager::distance(p, center))
            .collect();
        assert!(first_ring.iter().all(|&d| d <= 1), "{first_ring:?}");
        assert_eq!(ChunkManager::distance(*meshed.last().unwrap(), center), 4);
    }

    #[test]
    fn moving_away_unloads_old_chunks() {
        let mut manager = ChunkManager::new(ChunkSource::Flat, models(), 1, 2);
        run_until(&mut manager, ChunkPos::new(0, 0), |m, meshed| {
            meshed.len() >= 9 && m.in_flight() == 0
        });
        let far = ChunkPos::new(20, 0);
        let (_, unloaded) = run_until(&mut manager, far, |m, meshed| {
            meshed.len() >= 9 && m.in_flight() == 0
        });
        assert_eq!(unloaded.len(), 9, "every old mesh is removed once");
        assert!(
            manager
                .world()
                .positions()
                .all(|pos| ChunkManager::distance(pos, far) <= 3)
        );
    }

    #[test]
    fn generated_terrain_streams_in() {
        let source = ChunkSource::Terrain(Box::new(Terrain::new(42)));
        let mut manager = ChunkManager::new(source, models(), 2, 4);
        let (meshed, _) = run_until(&mut manager, ChunkPos::new(0, 0), |_, meshed| {
            meshed.len() >= 25
        });
        assert_eq!(meshed.len(), 25);
        assert_eq!(manager.seed(), Some(42));
    }

    /// Loads a 3×3 flat area around the origin (radius 1) and drains it.
    fn loaded_flat() -> ChunkManager {
        let mut manager = ChunkManager::new(ChunkSource::Flat, models(), 1, 2);
        run_until(&mut manager, ChunkPos::new(0, 0), |m, meshed| {
            meshed.len() >= 9 && m.in_flight() == 0
        });
        manager
    }

    const STONE: BlockState = BlockState::new(1, 0);
    const TORCH: BlockState = BlockState::new(50, 0);

    #[test]
    fn edits_remesh_the_chunk_and_border_neighbors() {
        let mut manager = loaded_flat();
        manager.set_blocks(&[(IVec3::new(5, 70, 5), STONE)]);
        assert_eq!(manager.world().block(5, 70, 5), Some(STONE));
        let changes = manager.update(ChunkPos::new(0, 0));
        let remeshed: Vec<ChunkPos> = changes.meshes.iter().map(|(pos, _)| *pos).collect();
        assert_eq!(remeshed, [ChunkPos::new(0, 0)]);
        // Let the background relight of the neighbors finish.
        run_until(&mut manager, ChunkPos::new(0, 0), |m, _| {
            m.in_flight() == 0 && m.relight.is_empty()
        });

        // A corner block touches both neighbors across its edges.
        manager.set_blocks(&[(IVec3::new(0, 70, 15), STONE)]);
        let changes = manager.update(ChunkPos::new(0, 0));
        let remeshed: HashSet<ChunkPos> = changes.meshes.iter().map(|(pos, _)| *pos).collect();
        let expected = [(0, 0), (-1, 0), (0, 1)].map(|(x, z)| ChunkPos::new(x, z));
        assert_eq!(remeshed, expected.into_iter().collect());
    }

    #[test]
    fn edits_outside_loaded_chunks_or_the_world_are_ignored() {
        let mut manager = loaded_flat();
        manager.set_blocks(&[
            (IVec3::new(500, 70, 0), STONE),
            (IVec3::new(0, 128, 0), STONE),
            (IVec3::new(0, -1, 0), STONE),
        ]);
        assert!(manager.update(ChunkPos::new(0, 0)).meshes.is_empty());
    }

    #[test]
    fn edited_chunks_survive_unloading() {
        let mut manager = loaded_flat();
        manager.set_blocks(&[(IVec3::new(3, 70, 3), STONE)]);
        let far = ChunkPos::new(30, 0);
        run_until(&mut manager, far, |m, meshed| {
            meshed.len() >= 9 && m.in_flight() == 0
        });
        assert!(manager.world().get(ChunkPos::new(0, 0)).is_none());
        run_until(&mut manager, ChunkPos::new(0, 0), |m, meshed| {
            meshed.len() >= 9 && m.in_flight() == 0
        });
        assert_eq!(manager.world().block(3, 70, 3), Some(STONE));
        // Unedited chunks are regenerated, not kept.
        assert!(manager.unloaded_edits.is_empty());
    }

    fn flat_with_store(dir: &std::path::Path) -> ChunkManager {
        let store = RegionStore::new(dir);
        let mut manager = ChunkManager::with_store(ChunkSource::Flat, store, models(), 1, 2);
        run_until(&mut manager, ChunkPos::new(0, 0), |m, meshed| {
            meshed.len() >= 9 && m.in_flight() == 0
        });
        manager
    }

    #[test]
    fn saved_edits_load_in_a_new_manager() {
        let dir = TempDir::new("manager-save");
        let mut manager = flat_with_store(dir.path());
        manager.set_blocks(&[
            (IVec3::new(3, 70, 3), STONE),
            (IVec3::new(17, 70, -1), TORCH),
        ]);
        assert_eq!(manager.save().unwrap(), 2);
        assert_eq!(manager.save().unwrap(), 0, "nothing changed since");
        drop(manager);

        // As after quitting and relaunching.
        let manager = flat_with_store(dir.path());
        assert_eq!(manager.world().block(3, 70, 3), Some(STONE));
        assert_eq!(manager.world().block(17, 70, -1), Some(TORCH));
        assert_eq!(manager.world().block(4, 70, 3), Some(BlockState::AIR));
    }

    #[test]
    fn saved_chunks_leave_memory_and_come_back_from_the_store() {
        let dir = TempDir::new("manager-unload");
        let mut manager = flat_with_store(dir.path());
        manager.set_blocks(&[(IVec3::new(3, 70, 3), STONE)]);
        let (home, far) = (ChunkPos::new(0, 0), ChunkPos::new(30, 0));
        run_until(&mut manager, far, |m, meshed| {
            meshed.len() >= 9 && m.in_flight() == 0
        });
        assert!(
            manager.unloaded_edits.contains_key(&home),
            "unsaved, so kept"
        );
        assert_eq!(manager.save().unwrap(), 1);
        assert!(manager.unloaded_edits.is_empty());
        run_until(&mut manager, home, |m, meshed| {
            meshed.len() >= 9 && m.in_flight() == 0
        });
        assert_eq!(manager.world().block(3, 70, 3), Some(STONE));
    }

    #[test]
    fn without_a_store_saving_does_nothing() {
        let mut manager = loaded_flat();
        manager.set_blocks(&[(IVec3::new(3, 70, 3), STONE)]);
        assert_eq!(manager.save().unwrap(), 0);
        assert!(manager.unsaved.contains(&ChunkPos::new(0, 0)));
    }

    #[test]
    fn mesh_jobs_started_before_an_edit_are_discarded() {
        let mut manager = ChunkManager::new(ChunkSource::Flat, models(), 1, 1);
        let center = ChunkPos::new(0, 0);
        // Load everything but stop before the center is meshed.
        let deadline = Instant::now() + Duration::from_secs(20);
        while !manager.meshing.contains(&center) {
            assert!(Instant::now() < deadline);
            manager.update(center);
            std::thread::sleep(Duration::from_millis(1));
        }
        manager.set_blocks(&[(IVec3::new(4, 70, 4), STONE)]);
        let mut delivered = Vec::new();
        while !(manager.meshed.contains(&center) && manager.in_flight() == 0) {
            assert!(Instant::now() < deadline);
            let changes = manager.update(center);
            delivered.extend(changes.meshes.into_iter().filter(|(pos, _)| *pos == center));
            std::thread::sleep(Duration::from_millis(1));
        }
        // Only the requeued job's mesh, with the new block, reached the renderer.
        let area = manager.world().area(center).unwrap();
        let fresh = mesh_chunk(&manager.models, &area, &light_chunk(&manager.models, &area));
        assert_eq!(delivered.len(), 1);
        assert_eq!(delivered[0].1, fresh);
    }

    /// M5 "done when": a sealed cave crossing a chunk border is dark, a torch
    /// placed in it lights the far side of the border, and breaking the torch
    /// makes it dark again.
    #[test]
    fn torches_light_caves_across_chunk_borders_and_breaking_them_undoes_it() {
        let mut chunks = HashMap::new();
        for cz in -2..=2 {
            for cx in -2..=2 {
                let pos = ChunkPos::new(cx, cz);
                let mut chunk = Chunk::new(pos);
                for y in 0..64 {
                    for z in 0..16 {
                        for x in 0..16 {
                            chunk.set(x, y, z, STONE);
                        }
                    }
                }
                chunks.insert(pos, chunk);
            }
        }
        // Tunnel at y 30 from x 8 to 23, crossing the border at x 16.
        for x in 8..24 {
            let chunk = chunks.get_mut(&ChunkPos::of_block(x, 5)).unwrap();
            chunk.set(x.rem_euclid(16) as usize, 30, 5, BlockState::AIR);
        }
        let mut manager = ChunkManager::new(ChunkSource::Fixed(chunks), models(), 1, 2);
        let center = ChunkPos::new(0, 0);
        let settle = |manager: &mut ChunkManager| {
            run_until(manager, center, |m, _| {
                m.meshed.len() >= 9 && m.in_flight() == 0 && m.relight.is_empty()
            });
        };
        settle(&mut manager);
        assert_eq!(
            manager.light_at(17, 30, 5),
            Some((0, 0)),
            "cave starts dark"
        );
        assert_eq!(manager.light_at(5, 70, 5), Some((15, 0)), "open sky");

        manager.set_blocks(&[(IVec3::new(14, 30, 5), TORCH)]);
        // The torch's own chunk relights at once.
        assert_eq!(manager.light_at(15, 30, 5), Some((0, 13)));
        settle(&mut manager);
        assert_eq!(manager.light_at(17, 30, 5), Some((0, 11)));
        assert_eq!(manager.light_at(23, 30, 5), Some((0, 5)));

        manager.set_blocks(&[(IVec3::new(14, 30, 5), BlockState::AIR)]);
        settle(&mut manager);
        for x in 8..24 {
            assert_eq!(manager.light_at(x, 30, 5), Some((0, 0)), "x = {x}");
        }
    }

    /// M3 "no stalls" check: fly in a straight line at fly speed with the real
    /// render distance and terrain, and measure the main thread's time in
    /// `update` and whether chunks near the camera are ever missing.
    /// Slow, so run it by hand: `cargo test --release -- --ignored flight`.
    #[test]
    #[ignore]
    fn flight_at_fly_speed_never_stalls() {
        use crate::config::{FLY_SPEED, RENDER_DISTANCE};
        let workers = std::thread::available_parallelism()
            .map_or(2, |n| n.get().saturating_sub(1))
            .max(1);
        let source = ChunkSource::Terrain(Box::new(Terrain::new(42)));
        let mut manager = ChunkManager::new(source, models(), RENDER_DISTANCE, workers);
        let mut meshed: HashSet<ChunkPos> = HashSet::new();
        // Let the spawn area load first, as the game does behind the sky.
        let start = ChunkPos::new(0, 0);
        run_until(&mut manager, start, |m, _| {
            m.meshed.len() >= 361 && m.in_flight() == 0
        });
        meshed.extend(manager.meshed.iter().copied());

        let started = Instant::now();
        let (mut worst_update, mut worst_missing, mut frames) = (Duration::ZERO, 0, 0);
        while started.elapsed() < Duration::from_secs(20) {
            let x = (started.elapsed().as_secs_f32() * FLY_SPEED) as i32;
            let center = ChunkPos::of_block(x, 0);
            let t = Instant::now();
            let changes = manager.update(center);
            worst_update = worst_update.max(t.elapsed());
            meshed.extend(changes.meshes.iter().map(|(pos, _)| *pos));
            for pos in changes.unloaded {
                meshed.remove(&pos);
            }
            // Chunks within 4 of the camera must always be there.
            let missing = (-4..=4)
                .flat_map(|dz| (-4..=4).map(move |dx| center.offset(dx, dz)))
                .filter(|pos| !meshed.contains(pos))
                .count();
            worst_missing = worst_missing.max(missing);
            frames += 1;
            std::thread::sleep(Duration::from_micros(16_600));
        }
        eprintln!(
            "{workers} workers, {frames} frames: worst update {worst_update:?}, worst missing nearby chunks {worst_missing}"
        );
        assert_eq!(worst_missing, 0);
        assert!(
            worst_update < Duration::from_millis(4),
            "update took {worst_update:?}"
        );
    }
}
