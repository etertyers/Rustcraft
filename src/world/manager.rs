//! Streams chunks in and out around the player, generating and meshing them
//! on worker threads.
//!
//! Each frame `update` collects finished work, unloads far chunks and hands
//! out new jobs nearest-first. The main thread never waits on a worker.

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    thread::JoinHandle,
};

use crossbeam_channel::{Receiver, Sender, TryRecvError};

use super::{
    World,
    chunk::{Chunk, ChunkPos},
    mesh::{BlockModels, ChunkArea, Mesh, mesh_chunk},
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
}

enum Job {
    Generate(ChunkPos),
    Mesh(ChunkPos, ChunkArea),
}

enum Done {
    Generated(Box<Chunk>),
    Meshed(ChunkPos, Mesh),
}

/// What changed this frame, for the renderer.
#[derive(Default)]
pub struct Changes {
    pub meshes: Vec<(ChunkPos, Mesh)>,
    pub unloaded: Vec<ChunkPos>,
}

pub struct ChunkManager {
    world: World,
    source: Arc<ChunkSource>,
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
}

/// Finished jobs collected per frame, so a burst can't stall one frame.
const MAX_RESULTS_PER_FRAME: usize = 64;

impl ChunkManager {
    /// Starts `workers` threads. Chunks are generated one ring beyond
    /// `radius` so border chunks can be meshed against their neighbors.
    pub fn new(source: ChunkSource, models: Arc<BlockModels>, radius: i32, workers: usize) -> Self {
        let source = Arc::new(source);
        let (job_tx, job_rx) = crossbeam_channel::unbounded::<Job>();
        let (done_tx, done_rx) = crossbeam_channel::unbounded();
        let workers = (0..workers.max(1))
            .map(|i| {
                let (jobs, done, source, models) = (
                    job_rx.clone(),
                    done_tx.clone(),
                    source.clone(),
                    models.clone(),
                );
                std::thread::Builder::new()
                    .name(format!("chunk-worker-{i}"))
                    .spawn(move || {
                        for job in jobs {
                            let result = match job {
                                Job::Generate(pos) => {
                                    Done::Generated(Box::new(source.generate(pos)))
                                }
                                Job::Mesh(pos, area) => {
                                    Done::Meshed(pos, mesh_chunk(&models, &area))
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
            jobs: Some(job_tx),
            done: done_rx,
            max_in_flight: workers.len() * 2,
            workers,
            generating: HashSet::new(),
            meshing: HashSet::new(),
            meshed: HashSet::new(),
            radius,
            offsets,
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
        let mut changes = Changes::default();

        for _ in 0..MAX_RESULTS_PER_FRAME {
            match self.done.try_recv() {
                Ok(Done::Generated(chunk)) => {
                    self.generating.remove(&chunk.pos);
                    if Self::distance(chunk.pos, center) <= self.radius + 1 {
                        self.world.insert(*chunk);
                    }
                }
                Ok(Done::Meshed(pos, mesh)) => {
                    self.meshing.remove(&pos);
                    if self.world.get(pos).is_some() {
                        self.meshed.insert(pos);
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
            self.world.remove(pos);
            if self.meshed.remove(&pos) {
                changes.unloaded.push(pos);
            }
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
                if self.generating.insert(pos) {
                    let _ = jobs.send(Job::Generate(pos));
                }
            } else if dx.abs().max(dz.abs()) <= self.radius
                && !self.meshed.contains(&pos)
                && !self.meshing.contains(&pos)
                && self.neighbors_loaded(pos)
                && let Some(area) = self.world.area(pos)
            {
                self.meshing.insert(pos);
                let _ = jobs.send(Job::Mesh(pos, area));
            }
        }
        changes
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
    use crate::world::{biome::BiomeColors, block::BlockRegistry};
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
