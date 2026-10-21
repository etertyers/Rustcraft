//! The Nether's structures (M25): fortresses and bastion remnants, which
//! share modern's "nether complexes" placement, ruined portals, and nether
//! fossils. Every spacing, separation, salt, weight and biome list is in the
//! tables below.
//!
//! Each region of `spacing` × `spacing` chunks has one candidate chunk, from
//! the world seed, the region and the salt alone. A complex candidate picks
//! fortress or bastion by weight and, if the biome there refuses the pick,
//! tries the other, so every region gets exactly one complex.
//!
//! As in the Overworld, each structure lays itself out from the seed
//! alone, asking only the bare terrain (`worldgen::nether::Probe`), and
//! each chunk writes the part of it inside. A chunk finds the structures
//! reaching it by checking the candidates around it, so nothing depends on
//! which chunks were generated first.

use std::{cell::RefCell, collections::HashMap, sync::Arc};

use glam::IVec3;

use super::{Kind, MobSpawn, Structure, bastion, fortress, fossil, write};
use crate::world::{
    biome::Biome,
    chunk::{Chunk, ChunkPos},
    worldgen::{
        nether::{Nether, Probe},
        rng::Rng,
    },
};

/// Modern's `random_spread` placement of a structure set.
pub struct Placement {
    pub spacing: i32,
    pub separation: i32,
    pub salt: u64,
}

impl Placement {
    pub(super) const fn spread(&self) -> super::Spread {
        super::Spread::new(self.spacing, self.separation)
    }
}

/// Fortresses and bastion remnants (`nether_complexes`).
pub const COMPLEXES: Placement = Placement {
    spacing: 27,
    separation: 4,
    salt: 30_084_232,
};
/// Ruined portals (modern's one `ruined_portals` set, here in blackstone).
pub const RUINED_PORTALS: Placement = Placement {
    spacing: 40,
    separation: 15,
    salt: 34_222_645,
};
pub const FOSSILS: Placement = Placement {
    spacing: 2,
    separation: 1,
    salt: 14_357_921,
};

const ALL_BIOMES: [Biome; 5] = Biome::NETHER;

/// One kind of nether complex: its pick weight and where it may start.
pub struct Complex {
    pub kind: Kind,
    pub weight: u32,
    pub biomes: &'static [Biome],
}

pub const COMPLEX_KINDS: [Complex; 2] = [
    Complex {
        kind: Kind::Fortress,
        weight: 2,
        biomes: &ALL_BIOMES,
    },
    Complex {
        kind: Kind::Bastion,
        weight: 3,
        biomes: &[
            Biome::NetherWastes,
            Biome::SoulSandValley,
            Biome::CrimsonForest,
            Biome::WarpedForest,
        ],
    },
];

pub const FOSSIL_BIOMES: [Biome; 1] = [Biome::SoulSandValley];

const KINDS: [Kind; 4] = [
    Kind::Fortress,
    Kind::Bastion,
    Kind::NetherRuinedPortal,
    Kind::NetherFossil,
];

/// How far into its chunk a bastion's start lies.
const BASTION_OFFSET: i32 = 8;

/// The kind of complex that starts at `start`: a weighted pick, then the
/// other if the biome at the first's start refuses it. None if both do.
fn complex_at(nether: &Nether, start: ChunkPos) -> Option<Kind> {
    let mut rng = Rng::for_chunk(nether.world_seed(), start.x, start.z, 0xC0_4E7E);
    let mut left: Vec<&Complex> = COMPLEX_KINDS.iter().collect();
    while !left.is_empty() {
        let total: u32 = left.iter().map(|c| c.weight).sum();
        let mut roll = rng.below(total);
        let i = left
            .iter()
            .position(|c| {
                let hit = roll < c.weight;
                roll = roll.saturating_sub(c.weight);
                hit
            })
            .unwrap_or(0);
        let pick = left.remove(i);
        let (x, z) = start.min_block();
        let offset = if pick.kind == Kind::Fortress {
            fortress::START_OFFSET
        } else {
            BASTION_OFFSET
        };
        if pick.biomes.contains(&nether.biome(x + offset, z + offset)) {
            return Some(pick.kind);
        }
    }
    None
}

/// Whether the candidate at `start` is a structure of `kind`.
fn ours(nether: &Nether, kind: Kind, start: ChunkPos) -> bool {
    match kind {
        Kind::Fortress | Kind::Bastion => complex_at(nether, start) == Some(kind),
        Kind::NetherFossil => {
            let (x, z) = start.min_block();
            FOSSIL_BIOMES.contains(&nether.biome(x + 8, z + 8))
        }
        _ => true,
    }
}

/// The candidate chunk of region (rx, rz) for `kind`'s placement.
pub fn candidate(nether: &Nether, kind: Kind, rx: i32, rz: i32) -> ChunkPos {
    kind.spread()
        .candidate(nether.world_seed(), kind.salt(), rx, rz)
}

/// The chunks within `kind`'s reach of `pos` where one of them starts.
fn starts_near(nether: &Nether, kind: Kind, pos: ChunkPos) -> Vec<ChunkPos> {
    let reach = kind.reach();
    let spacing = kind.spread().spacing;
    let region = |c: i32| c.div_euclid(spacing);
    let mut found = Vec::new();
    for rz in region(pos.z - reach)..=region(pos.z + reach) {
        for rx in region(pos.x - reach)..=region(pos.x + reach) {
            let start = candidate(nether, kind, rx, rz);
            let near = (start.x - pos.x).abs() <= reach && (start.z - pos.z).abs() <= reach;
            if near && ours(nether, kind, start) {
                found.push(start);
            }
        }
    }
    found
}

/// Where the structure of `kind` starting in chunk `start` has its start
/// piece (a fortress's and a bastion's are fixed by the tables above; the
/// others' y is wherever their floor is).
pub fn start_block(nether: &Nether, kind: Kind, start: ChunkPos) -> IVec3 {
    let (x, z) = start.min_block();
    match kind {
        Kind::Fortress => {
            let mut rng =
                Rng::for_chunk(nether.world_seed(), start.x, start.z, kind.salt() ^ 0x5EED);
            let o = fortress::START_OFFSET;
            IVec3::new(x + o, fortress::deck(&mut rng), z + o)
        }
        _ => IVec3::new(x + BASTION_OFFSET, bastion::BASE, z + BASTION_OFFSET),
    }
}

fn generate(nether: &Nether, kind: Kind, start: ChunkPos) -> Option<Structure> {
    let mut probe = Probe::new(nether);
    let mut rng = Rng::for_chunk(nether.world_seed(), start.x, start.z, kind.salt() ^ 0x5EED);
    let pieces = match kind {
        Kind::Fortress => fortress::generate(&mut probe, start, &mut rng)?,
        Kind::Bastion => bastion::generate(&mut probe, start, &mut rng)?,
        Kind::NetherRuinedPortal => bastion::ruined_portal(&mut probe, start, &mut rng)?,
        Kind::NetherFossil => fossil::generate(&mut probe, start, &mut rng)?,
        _ => return None,
    };
    Structure::new(kind, start, pieces)
}

type CacheKey = (u64, Kind, ChunkPos);

thread_local! {
    static CACHE: RefCell<HashMap<CacheKey, Option<Arc<Structure>>>> = RefCell::new(HashMap::new());
}

const CACHE_SIZE: usize = 256;

/// The structure of `kind` starting in `start`, laid out once per thread.
pub fn structure_at(nether: &Nether, kind: Kind, start: ChunkPos) -> Option<Arc<Structure>> {
    let key = (nether.world_seed(), kind, start);
    if let Some(found) = CACHE.with(|cache| cache.borrow().get(&key).cloned()) {
        return found;
    }
    let made = generate(nether, kind, start).map(Arc::new);
    CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        if cache.len() >= CACHE_SIZE {
            cache.clear();
        }
        cache.insert(key, made.clone());
    });
    made
}

/// Every Nether structure with a piece reaching into chunk `pos`.
pub fn near(nether: &Nether, pos: ChunkPos) -> Vec<Arc<Structure>> {
    let mut found = Vec::new();
    for kind in KINDS {
        for start in starts_near(nether, kind, pos) {
            if let Some(structure) = structure_at(nether, kind, start)
                && structure.bounds.touches_chunk(pos)
            {
                found.push(structure);
            }
        }
    }
    found
}

/// Writes the Nether structures' parts inside `chunk`.
pub fn place(nether: &Nether, chunk: &mut Chunk) {
    let structures = near(nether, chunk.pos);
    if !structures.is_empty() {
        write(&structures, nether.world_seed(), chunk);
    }
}

/// Mobs the structures put in chunk `pos` when it first loads (a
/// bastion's piglins, brutes and hoglins).
pub fn spawns_in(nether: &Nether, pos: ChunkPos) -> Vec<MobSpawn> {
    let mut spawns = Vec::new();
    for structure in near(nether, pos) {
        for mob in structure.mobs() {
            let at = ChunkPos::of_block(mob.at.x.floor() as i32, mob.at.z.floor() as i32);
            if at == pos {
                spawns.push(mob.clone());
            }
        }
    }
    spawns
}

/// The kind of structure whose pieces (not their pillars) hold `at`, for
/// what spawns there.
pub fn kind_at(nether: &Nether, at: IVec3) -> Option<Kind> {
    near(nether, ChunkPos::of_block(at.x, at.z))
        .into_iter()
        .find(|s| {
            s.pieces
                .iter()
                .filter(|p| !p.support)
                .filter_map(|p| p.bounds)
                .any(|b| b.contains(at))
        })
        .map(|s| s.kind)
}

/// A structure that starts in a surveyed area.
#[derive(Debug, Clone, Copy)]
pub struct Found {
    pub kind: Kind,
    pub start: ChunkPos,
    /// Its start piece: fixed for a fortress or a bastion, the lowest block
    /// of its pieces for the rest.
    pub at: IVec3,
}

/// Every structure of `kinds` starting in the chunks `lo..=hi`.
pub fn survey(nether: &Nether, kinds: &[Kind], lo: ChunkPos, hi: ChunkPos) -> Vec<Found> {
    let mut found = Vec::new();
    for &kind in kinds {
        let spacing = kind.spread().spacing;
        for rz in lo.z.div_euclid(spacing)..=hi.z.div_euclid(spacing) {
            for rx in lo.x.div_euclid(spacing)..=hi.x.div_euclid(spacing) {
                let start = candidate(nether, kind, rx, rz);
                if !(lo.x..=hi.x).contains(&start.x)
                    || !(lo.z..=hi.z).contains(&start.z)
                    || !ours(nether, kind, start)
                {
                    continue;
                }
                let Some(structure) = structure_at(nether, kind, start) else {
                    continue;
                };
                let at = match kind {
                    Kind::Fortress | Kind::Bastion => start_block(nether, kind, start),
                    _ => {
                        let low = structure
                            .pieces
                            .iter()
                            .filter(|p| !p.support)
                            .filter_map(|p| p.bounds)
                            .map(|b| b.min.y)
                            .min()
                            .unwrap_or(0);
                        let (x, z) = start.min_block();
                        IVec3::new(x + 8, low, z + 8)
                    }
                };
                found.push(Found { kind, start, at });
            }
        }
    }
    found
}

/// Where the nearest structure of `kind` starts within `radius` chunks of
/// `pos`, for tests and screenshots.
pub fn find(nether: &Nether, kind: Kind, pos: ChunkPos, radius: i32) -> Option<Arc<Structure>> {
    let spacing = kind.spread().spacing;
    let mut best: Option<(i32, Arc<Structure>)> = None;
    let region = |c: i32| c.div_euclid(spacing);
    for rz in region(pos.z - radius)..=region(pos.z + radius) {
        for rx in region(pos.x - radius)..=region(pos.x + radius) {
            let start = candidate(nether, kind, rx, rz);
            if !ours(nether, kind, start) {
                continue;
            }
            let d = (start.x - pos.x).abs().max((start.z - pos.z).abs());
            if d > radius || best.as_ref().is_some_and(|(b, _)| *b <= d) {
                continue;
            }
            if let Some(found) = structure_at(nether, kind, start) {
                best = Some((d, found));
            }
        }
    }
    best.map(|(_, s)| s)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `cargo test --release structure_chunk_time -- --ignored --nocapture`:
    /// how long Nether chunks over a fortress and a bastion take.
    #[test]
    #[ignore]
    fn structure_chunk_time() {
        let nether = Nether::new(20111118);
        for kind in [Kind::Fortress, Kind::Bastion] {
            let found = find(&nether, kind, ChunkPos::new(0, 0), 80).unwrap();
            let (lo, hi) = (
                ChunkPos::of_block(found.bounds.min.x, found.bounds.min.z),
                ChunkPos::of_block(found.bounds.max.x, found.bounds.max.z),
            );
            // A fresh thread, so the first chunk lays the structure out.
            let n = nether.world_seed();
            let took = std::thread::spawn(move || {
                let nether = Nether::new(n);
                let started = std::time::Instant::now();
                let mut count = 0;
                for z in lo.z..=hi.z {
                    for x in lo.x..=hi.x {
                        nether.generate(ChunkPos::new(x, z));
                        count += 1;
                    }
                }
                (count, started.elapsed())
            })
            .join()
            .unwrap();
            eprintln!(
                "{kind:?}: {} chunks, {:.2} ms each",
                took.0,
                took.1.as_secs_f64() * 1000.0 / took.0 as f64
            );
        }
    }

    /// `cargo test --release nether_structure_spots -- --ignored --nocapture`:
    /// where the nearest of each starts on seed 20111118, for screenshots.
    #[test]
    #[ignore]
    fn nether_structure_spots() {
        let nether = Nether::new(20111118);
        for kind in KINDS {
            let Some(found) = find(&nether, kind, ChunkPos::new(0, 0), 80) else {
                eprintln!("{kind:?}: none");
                continue;
            };
            let spawner = found
                .pieces
                .iter()
                .find_map(|p| p.spawners.first().map(|s| s.0));
            eprintln!(
                "{kind:?}: start {:?}, bounds {:?}..{:?}, spawner {spawner:?}",
                found.start, found.bounds.min, found.bounds.max
            );
        }
    }

    /// A seed whose first hundred regions have 39% fortresses: the share
    /// swings with how much of the area is deltas (bastions can't start in
    /// them, so a fortress takes their place).
    const SEED: u64 = 1;

    /// The hundred regions around the origin (4320 blocks across).
    fn regions() -> impl Iterator<Item = (i32, i32)> {
        (-5..5).flat_map(|rz| (-5..5).map(move |rx| (rx, rz)))
    }

    #[test]
    fn every_region_gets_exactly_one_complex_never_a_bastion_in_the_deltas() {
        let nether = Nether::new(SEED);
        let mut fortresses = 0;
        for (rx, rz) in regions() {
            let start = candidate(&nether, Kind::Fortress, rx, rz);
            let kind = complex_at(&nether, start).expect("a complex");
            // Only the pick that stands is "ours"; the other isn't.
            let other = if kind == Kind::Fortress {
                Kind::Bastion
            } else {
                Kind::Fortress
            };
            assert!(ours(&nether, kind, start) && !ours(&nether, other, start));
            assert!(
                structure_at(&nether, kind, start).is_some(),
                "{kind:?} at {start:?}"
            );
            if kind == Kind::Fortress {
                fortresses += 1;
            } else {
                let (x, z) = start.min_block();
                assert_ne!(nether.biome(x + 8, z + 8), Biome::BasaltDeltas);
            }
        }
        assert!(
            (38..=50).contains(&fortresses),
            "{fortresses} fortresses of 100"
        );
    }

    #[test]
    fn complexes_keep_apart_and_ruined_portals_come_one_to_a_region() {
        let nether = Nether::new(SEED);
        let starts: Vec<ChunkPos> = regions()
            .map(|(rx, rz)| candidate(&nether, Kind::Fortress, rx, rz))
            .collect();
        for (i, a) in starts.iter().enumerate() {
            for b in &starts[i + 1..] {
                assert!(
                    (a.x - b.x).abs() >= 4 || (a.z - b.z).abs() >= 4,
                    "{a:?} {b:?}"
                );
            }
        }
        // Adjacent regions are four apart along the axis they touch on.
        for (rx, rz) in regions() {
            let a = candidate(&nether, Kind::Fortress, rx, rz);
            assert!(candidate(&nether, Kind::Fortress, rx + 1, rz).x - a.x >= 4);
            assert!(candidate(&nether, Kind::Fortress, rx, rz + 1).z - a.z >= 4);
        }
        let spacing = RUINED_PORTALS.spacing;
        for rz in -3..3 {
            for rx in -3..3 {
                let start = candidate(&nether, Kind::NetherRuinedPortal, rx, rz);
                assert_eq!(
                    (start.x.div_euclid(spacing), start.z.div_euclid(spacing)),
                    (rx, rz)
                );
                assert!(structure_at(&nether, Kind::NetherRuinedPortal, start).is_some());
            }
        }
        let found = survey(
            &nether,
            &[Kind::NetherRuinedPortal],
            ChunkPos::new(-3 * spacing, -3 * spacing),
            ChunkPos::new(3 * spacing - 1, 3 * spacing - 1),
        );
        assert_eq!(found.len(), 36);
    }

    #[test]
    fn starts_and_blocks_do_not_depend_on_the_seed_run_or_load_order() {
        let (a, b) = (Nether::new(SEED), Nether::new(SEED));
        for (rx, rz) in regions() {
            let start = candidate(&a, Kind::Fortress, rx, rz);
            assert_eq!(start, candidate(&b, Kind::Fortress, rx, rz));
            assert_eq!(complex_at(&a, start), complex_at(&b, start));
        }
        assert_ne!(
            candidate(&a, Kind::Fortress, 0, 0),
            candidate(&Nether::new(SEED + 1), Kind::Fortress, 0, 0)
        );
        // The chunks of a fortress, generated forward and backward on fresh
        // threads (each with its own layouts cached).
        let found = find(&a, Kind::Fortress, ChunkPos::new(0, 0), 60).unwrap();
        let (lo, hi) = (
            ChunkPos::of_block(found.bounds.min.x, found.bounds.min.z),
            ChunkPos::of_block(found.bounds.max.x, found.bounds.max.z),
        );
        let mut chunks: Vec<ChunkPos> = (lo.z..=hi.z)
            .flat_map(|z| (lo.x..=hi.x).map(move |x| ChunkPos::new(x, z)))
            .collect();
        chunks.truncate(24);
        let run = |order: Vec<ChunkPos>| {
            std::thread::spawn(move || {
                let nether = Nether::new(SEED);
                let mut made: Vec<_> = order.iter().map(|&p| (p, nether.generate(p))).collect();
                made.sort_by_key(|(p, _)| (p.x, p.z));
                made.into_iter()
                    .map(|(_, c)| {
                        (0..128)
                            .flat_map(|y| {
                                (0..16).flat_map(move |z| (0..16).map(move |x| (x, y, z)))
                            })
                            .map(|(x, y, z)| c.get(x, y, z))
                            .collect::<Vec<_>>()
                    })
                    .collect::<Vec<_>>()
            })
            .join()
            .unwrap()
        };
        let forward = run(chunks.clone());
        chunks.reverse();
        assert!(forward == run(chunks), "load order changed the blocks");
    }

    #[test]
    fn fortresses_sit_between_y_48_and_70_and_bastions_start_at_33() {
        let nether = Nether::new(SEED);
        let (mut fortresses, mut bastions) = (0, 0);
        let found = survey(
            &nether,
            &[Kind::Fortress, Kind::Bastion],
            ChunkPos::new(-135, -135),
            ChunkPos::new(134, 134),
        );
        for f in found {
            let (sx, sz) = (
                f.start.min_block().0 + fortress::START_OFFSET,
                f.start.min_block().1 + fortress::START_OFFSET,
            );
            match f.kind {
                Kind::Fortress => {
                    fortresses += 1;
                    assert!((48..=70).contains(&f.at.y), "{:?}", f.at);
                    let s = structure_at(&nether, f.kind, f.start).unwrap();
                    let reach = (s.bounds.min.x - sx)
                        .abs()
                        .max((s.bounds.max.x - sx).abs())
                        .max((s.bounds.min.z - sz).abs())
                        .max((s.bounds.max.z - sz).abs());
                    assert!(reach <= 112, "a fortress reaching {reach} blocks");
                }
                _ => {
                    bastions += 1;
                    assert_eq!(f.at.y, 33);
                }
            }
        }
        assert!(fortresses > 20 && bastions > 20);
    }

    #[test]
    fn fossils_are_bone_in_valleys_between_y_32_and_125() {
        let nether = Nether::new(SEED);
        let found = survey(
            &nether,
            &[Kind::NetherFossil],
            ChunkPos::new(-100, -100),
            ChunkPos::new(99, 99),
        );
        assert!(found.len() > 50, "{} fossils", found.len());
        for f in found.iter().take(100) {
            let s = structure_at(&nether, f.kind, f.start).unwrap();
            for piece in &s.pieces {
                for (at, block) in &piece.blocks {
                    assert!(block.is(crate::world::blocks::BONE_BLOCK));
                    assert!((32..=125 + 5).contains(&at.y), "{at}");
                }
            }
            let (x, z) = (f.at.x, f.at.z);
            let _ = (x, z);
            let (x0, z0) = f.start.min_block();
            let any_valley = (0..16).any(|d| nether.biome(x0 + d, z0 + d) == Biome::SoulSandValley);
            assert!(any_valley);
        }
    }
}
