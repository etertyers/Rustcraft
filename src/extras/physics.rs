//! Sound Physics (M65), after Sound Physics Remastered: sounds take on the
//! space they're in. Rays from the listener, averaged over time, give the
//! size of the room (four reverb sizes, so caves echo and fields don't); a
//! ray from the source to the listener counts the blocks between them and
//! muffles the sound; distance takes the highs off; and being underwater
//! muffles everything.
//!
//! The work runs on a background thread from a cached snapshot of the
//! blocks around the listener, never per sound on the main thread. The main
//! thread only sends a job and picks up the last answer.

use std::sync::{Arc, Mutex};

use crossbeam_channel::{Receiver, Sender, bounded};
use glam::DVec3;

/// What the effects know of the blocks: whether one stops sound.
pub trait Solids: Send + Sync {
    fn solid(&self, x: i32, y: i32, z: i32) -> bool;
}

/// How far a ray looks for a wall, in blocks.
pub const RAY_RANGE: f64 = 40.0;
/// Rays cast around the listener.
const RAYS: usize = 24;

/// The four reverb sizes' weights (0–1 each): small, medium, large, huge.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Reverb(pub [f32; 4]);

impl Reverb {
    /// How much reverb in all.
    pub fn total(self) -> f32 {
        self.0.iter().sum()
    }

    fn lerp(self, other: Reverb, t: f32) -> Reverb {
        Reverb(std::array::from_fn(|i| {
            self.0[i] + (other.0[i] - self.0[i]) * t
        }))
    }
}

/// Where a ray from `from` along `dir` first meets a solid block, within
/// `RAY_RANGE`.
fn march(solids: &dyn Solids, from: DVec3, dir: DVec3) -> Option<f64> {
    let mut d = 0.5;
    while d < RAY_RANGE {
        let p = from + dir * d;
        if solids.solid(p.x.floor() as i32, p.y.floor() as i32, p.z.floor() as i32) {
            return Some(d);
        }
        d += 0.5;
    }
    None
}

/// Directions spread evenly over a sphere (a golden-angle spiral).
fn directions() -> [DVec3; RAYS] {
    std::array::from_fn(|i| {
        let y = 1.0 - 2.0 * (i as f64 + 0.5) / RAYS as f64;
        let r = (1.0 - y * y).sqrt();
        let a = i as f64 * 2.399_963_229_728_653;
        DVec3::new(a.cos() * r, y, a.sin() * r)
    })
}

/// The reverb of the space around `at` right now: the share of rays that hit
/// a wall says how enclosed it is, and how far they went says how big. A
/// field's rays escape, so it has none; a closed room's all hit, so it
/// has a lot, in the size bucket of its walls' distance.
pub fn reverb_around(solids: &dyn Solids, at: DVec3) -> Reverb {
    let mut hits = Vec::new();
    for dir in directions() {
        if let Some(d) = march(solids, at, dir) {
            hits.push(d);
        }
    }
    if hits.is_empty() {
        return Reverb::default();
    }
    let enclosed = hits.len() as f32 / RAYS as f32;
    let mean = (hits.iter().sum::<f64>() / hits.len() as f64) as f32;
    // Smooth buckets: triangles centered at 2, 5, 11 and 24 blocks.
    let tri = |center: f32, width: f32| (1.0 - (mean - center).abs() / width).clamp(0.0, 1.0);
    let raw = [
        tri(2.0, 3.0),
        tri(5.0, 4.0),
        tri(11.0, 8.0),
        tri(24.0, 14.0),
    ];
    let norm = raw.iter().sum::<f32>().max(1e-3);
    // Only a mostly enclosed space echoes.
    let strength = ((enclosed - 0.4) / 0.6).clamp(0.0, 1.0);
    Reverb(raw.map(|w| w / norm * strength))
}

/// Solid blocks between `source` and `listener`, along the straight line.
pub fn blocks_between(solids: &dyn Solids, source: DVec3, listener: DVec3) -> u32 {
    let delta = listener - source;
    let length = delta.length();
    if length < 1.0 {
        return 0;
    }
    let dir = delta / length;
    let mut count = 0;
    let mut last = None;
    let mut d = 0.5;
    while d < length - 0.5 {
        let p = source + dir * d;
        let cell = (p.x.floor() as i32, p.y.floor() as i32, p.z.floor() as i32);
        if last != Some(cell) {
            last = Some(cell);
            if solids.solid(cell.0, cell.1, cell.2) {
                count += 1;
            }
        }
        d += 0.4;
    }
    count
}

/// How a sound is changed on its way to the listener.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Shaping {
    /// Multiplies the loudness.
    pub gain: f32,
    /// Low-pass cutoff in hertz (20 000 is open).
    pub cutoff: f32,
    pub reverb: Reverb,
}

impl Shaping {
    pub const OPEN: Shaping = Shaping {
        gain: 1.0,
        cutoff: 20_000.0,
        reverb: Reverb([0.0; 4]),
    };
}

/// A sound from `source` heard at `listener`: walls between them muffle it
/// (each block takes about a fifth of the loudness and half the highs),
/// distance takes the highs off (air absorption), and the room's reverb is
/// sent along.
pub fn shape(
    solids: &dyn Solids,
    source: DVec3,
    listener: DVec3,
    room: Reverb,
    underwater: bool,
) -> Shaping {
    let walls = blocks_between(solids, source, listener).min(8);
    let distance = source.distance(listener) as f32;
    let mut gain = 0.8_f32.powi(walls as i32);
    let mut cutoff = 20_000.0 * 0.5_f32.powi(walls as i32) / (1.0 + distance / 30.0);
    if underwater {
        gain *= 0.7;
        cutoff = cutoff.min(1_000.0);
    }
    Shaping {
        gain,
        cutoff: cutoff.clamp(350.0, 20_000.0),
        reverb: room,
    }
}

/// A job for the worker: the blocks as they were, where the listener is,
/// and the sounds' sources to shape.
struct Job {
    world: Arc<dyn Solids>,
    listener: DVec3,
    underwater: bool,
    sources: Vec<DVec3>,
}

/// The worker's answer to a job.
#[derive(Debug, Clone, Default)]
pub struct Answer {
    pub shapings: Vec<Shaping>,
    pub room: Reverb,
    /// How long the job took, for the cost report.
    pub micros: u32,
}

/// The background thread and its mailboxes.
pub struct Physics {
    jobs: Sender<Job>,
    answers: Receiver<Answer>,
}

impl Physics {
    pub fn spawn() -> Self {
        let (jobs, job_rx) = bounded::<Job>(2);
        let (answer_tx, answers) = bounded::<Answer>(2);
        std::thread::Builder::new()
            .name("sound physics".to_owned())
            .spawn(move || {
                let mut room = Reverb::default();
                while let Ok(job) = job_rx.recv() {
                    let started = std::time::Instant::now();
                    // The room is averaged over time, so it doesn't flicker.
                    let now = reverb_around(&*job.world, job.listener);
                    room = room.lerp(now, 0.3);
                    let shapings = job
                        .sources
                        .iter()
                        .map(|&s| shape(&*job.world, s, job.listener, room, job.underwater))
                        .collect();
                    let _ = answer_tx.try_send(Answer {
                        shapings,
                        room,
                        micros: started.elapsed().as_micros() as u32,
                    });
                }
            })
            .ok();
        Self { jobs, answers }
    }

    /// Sends a job, unless the worker is busy (then the last answer stands).
    pub fn submit(
        &self,
        world: Arc<dyn Solids>,
        listener: DVec3,
        underwater: bool,
        sources: Vec<DVec3>,
    ) -> bool {
        self.jobs
            .try_send(Job {
                world,
                listener,
                underwater,
                sources,
            })
            .is_ok()
    }

    /// The newest answer waiting, if any.
    pub fn latest(&self) -> Option<Answer> {
        let mut newest = None;
        while let Ok(answer) = self.answers.try_recv() {
            newest = Some(answer);
        }
        newest
    }
}

/// A block lookup over a few chunks' `Arc`s, cheap to clone and take to the
/// worker: the snapshot of the blocks around the listener.
pub struct ChunkSnapshot {
    pub chunks:
        std::collections::HashMap<crate::world::chunk::ChunkPos, Arc<crate::world::chunk::Chunk>>,
}

impl ChunkSnapshot {
    /// The chunks within two of the player's.
    pub fn around(game: &crate::game::Game) -> Self {
        let center = crate::world::chunk::ChunkPos::of_block(
            game.player.camera.position.x.floor() as i32,
            game.player.camera.position.z.floor() as i32,
        );
        let world = game.chunks.world();
        let mut chunks = std::collections::HashMap::new();
        for dx in -2..=2 {
            for dz in -2..=2 {
                let pos = crate::world::chunk::ChunkPos::new(center.x + dx, center.z + dz);
                if let Some(chunk) = world.get(pos) {
                    chunks.insert(pos, chunk.clone());
                }
            }
        }
        Self { chunks }
    }
}

impl Solids for ChunkSnapshot {
    fn solid(&self, x: i32, y: i32, z: i32) -> bool {
        let size = crate::config::CHUNK_SIZE as i32;
        let Some(chunk) = self
            .chunks
            .get(&crate::world::chunk::ChunkPos::of_block(x, z))
        else {
            return false;
        };
        let block = chunk.get(
            x.rem_euclid(size) as usize,
            y.clamp(crate::config::MIN_Y, crate::config::MAX_Y - 1),
            z.rem_euclid(size) as usize,
        );
        !block.is_air() && block.def().solid && block.def().fluid.is_none()
    }
}

/// Shared between the app and the audio engine: the newest snapshot.
pub type SharedWorld = Arc<Mutex<Option<Arc<dyn Solids>>>>;

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[derive(Default)]
    struct Blocks(HashSet<(i32, i32, i32)>);

    impl Solids for Blocks {
        fn solid(&self, x: i32, y: i32, z: i32) -> bool {
            self.0.contains(&(x, y, z)) || y < 0
        }
    }

    /// A hollow stone cube `half` wide each way around (0, 10, 0).
    fn room(half: i32) -> Blocks {
        let mut b = Blocks::default();
        for x in -half - 1..=half + 1 {
            for y in 10 - half - 1..=10 + half + 1 {
                for z in -half - 1..=half + 1 {
                    let edge = x.abs() > half || z.abs() > half || (y - 10).abs() > half;
                    if edge {
                        b.0.insert((x, y, z));
                    }
                }
            }
        }
        b
    }

    #[test]
    fn a_closed_9_by_9_by_9_room_has_far_more_reverb_than_an_open_field() {
        let field = Blocks::default();
        let at = DVec3::new(0.5, 10.5, 0.5);
        let open = reverb_around(&field, at).total();
        // A field (nothing above the floor at y 0): only the ground in view.
        let walled = reverb_around(&room(4), at).total();
        assert!(open < 0.1, "a field echoes {open}");
        assert!(walled > 0.9, "a closed room echoes {walled}");
        assert!(walled > open * 5.0);
    }

    #[test]
    fn a_big_cave_lands_in_a_bigger_reverb_than_a_closet() {
        let at = DVec3::new(0.5, 10.5, 0.5);
        let small = reverb_around(&room(2), at);
        let big = reverb_around(&room(16), at);
        let weight = |r: Reverb| {
            r.0.iter()
                .enumerate()
                .map(|(i, w)| i as f32 * w)
                .sum::<f32>()
                / r.total()
        };
        assert!(weight(big) > weight(small) + 1.0, "{small:?} {big:?}");
    }

    #[test]
    fn a_wall_between_source_and_listener_lowers_the_volume_and_the_highs() {
        let mut world = Blocks::default();
        let (source, listener) = (DVec3::new(0.5, 5.5, 0.5), DVec3::new(10.5, 5.5, 0.5));
        let clear = shape(&world, source, listener, Reverb::default(), false);
        for y in 0..12 {
            for z in -3..4 {
                world.0.insert((5, y, z));
            }
        }
        let walled = shape(&world, source, listener, Reverb::default(), false);
        assert!(
            walled.gain < clear.gain * 0.9,
            "{} vs {}",
            walled.gain,
            clear.gain
        );
        assert!(walled.cutoff < clear.cutoff * 0.6);
        // Underwater everything is muffled.
        let wet = shape(
            &Blocks::default(),
            source,
            listener,
            Reverb::default(),
            true,
        );
        assert!(wet.cutoff <= 1_000.0 && wet.gain < 1.0);
        // Far away loses the highs even with nothing in the way.
        let far = shape(
            &Blocks::default(),
            source,
            source + DVec3::X * 60.0,
            Reverb::default(),
            false,
        );
        assert!(far.cutoff < clear.cutoff);
    }

    #[test]
    fn the_worker_answers_off_the_main_thread_and_fast() {
        let physics = Physics::spawn();
        let world: Arc<dyn Solids> = Arc::new(room(8));
        let at = DVec3::new(0.5, 10.5, 0.5);
        assert!(physics.submit(world.clone(), at, false, vec![at + DVec3::X; 50]));
        let started = std::time::Instant::now();
        let answer = loop {
            if let Some(a) = physics.latest() {
                break a;
            }
            assert!(started.elapsed().as_secs() < 5, "no answer");
            std::thread::sleep(std::time::Duration::from_millis(1));
        };
        assert_eq!(answer.shapings.len(), 50);
        assert!(answer.room.total() > 0.0);
        // Under 2 ms of one core for the update with 50 sounds (release).
        let allowed = if cfg!(debug_assertions) {
            40_000
        } else {
            2_000
        };
        assert!(answer.micros < allowed, "{} us", answer.micros);
    }

    #[test]
    fn the_main_thread_share_is_a_send_and_a_receive() {
        let physics = Physics::spawn();
        let world: Arc<dyn Solids> = Arc::new(room(8));
        let at = DVec3::new(0.5, 10.5, 0.5);
        let started = crate::testutil::thread_cpu_time();
        for _ in 0..200 {
            physics.submit(world.clone(), at, false, vec![at; 20]);
            let _ = physics.latest();
        }
        let per = (crate::testutil::thread_cpu_time() - started).as_secs_f64() * 1000.0 / 200.0;
        let allowed = if cfg!(debug_assertions) { 3.0 } else { 0.3 };
        assert!(per < allowed, "{per:.4} ms of main-thread time a frame");
    }
}
