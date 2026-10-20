//! The Nether's structures (M25): fortresses and bastion remnants, which
//! share modern's "nether complexes" placement (one candidate every 27
//! chunks, kept 4 apart; two in five are fortresses and the rest bastions,
//! which don't stand in basalt deltas), and ruined portals (one every 25
//! chunks, kept 10 apart).
//!
//! As in the Overworld, each structure lays itself out from the seed
//! alone, asking only the bare terrain (`worldgen::nether::Probe`), and
//! each chunk writes the part of it inside.

use std::{cell::RefCell, collections::HashMap, sync::Arc};

use glam::IVec3;

use super::{Kind, MobSpawn, Structure, bastion, fortress, write};
use crate::world::{
    biome::Biome,
    chunk::{Chunk, ChunkPos},
    worldgen::{
        nether::{Nether, Probe},
        rng::Rng,
    },
};

const KINDS: [Kind; 3] = [Kind::Fortress, Kind::Bastion, Kind::NetherRuinedPortal];

/// Two parts in five of nether complexes are fortresses (modern weights 2
/// and 3).
const FORTRESS_SHARE: f64 = 0.4;

/// The kind of complex that may start at `start`, for one of the two.
fn complex_at(nether: &Nether, start: ChunkPos) -> Kind {
    let mut rng = Rng::for_chunk(nether.world_seed(), start.x, start.z, 0xC0_4E7E);
    if rng.next_f64() < FORTRESS_SHARE {
        Kind::Fortress
    } else {
        Kind::Bastion
    }
}

/// The chunks within `kind`'s reach of `pos` where one of them starts.
fn starts_near(nether: &Nether, kind: Kind, pos: ChunkPos) -> Vec<ChunkPos> {
    let reach = kind.reach();
    let spread = kind.spread();
    let region = |c: i32| c.div_euclid(spread.spacing);
    let mut found = Vec::new();
    for rz in region(pos.z - reach)..=region(pos.z + reach) {
        for rx in region(pos.x - reach)..=region(pos.x + reach) {
            let start = spread.candidate(nether.world_seed(), kind.salt(), rx, rz);
            let near = (start.x - pos.x).abs() <= reach && (start.z - pos.z).abs() <= reach;
            let ours = match kind {
                Kind::Fortress | Kind::Bastion => complex_at(nether, start) == kind,
                _ => true,
            };
            if near && ours {
                found.push(start);
            }
        }
    }
    found
}

fn generate(nether: &Nether, kind: Kind, start: ChunkPos) -> Option<Structure> {
    let mut probe = Probe::new(nether);
    let (x, z) = start.min_block();
    let biome = probe.biome(x + 8, z + 8);
    let mut rng = Rng::for_chunk(nether.world_seed(), start.x, start.z, kind.salt() ^ 0x5EED);
    let pieces = match kind {
        Kind::Fortress => fortress::generate(&mut probe, start, &mut rng)?,
        Kind::Bastion if biome != Biome::BasaltDeltas => {
            bastion::generate(&mut probe, start, &mut rng)?
        }
        Kind::NetherRuinedPortal => bastion::ruined_portal(&mut probe, start, &mut rng)?,
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

/// Where the nearest structure of `kind` starts within `radius` chunks of
/// `pos`, for tests and screenshots.
pub fn find(nether: &Nether, kind: Kind, pos: ChunkPos, radius: i32) -> Option<Arc<Structure>> {
    let spacing = kind.spread().spacing;
    let mut best: Option<(i32, Arc<Structure>)> = None;
    let region = |c: i32| c.div_euclid(spacing);
    for rz in region(pos.z - radius)..=region(pos.z + radius) {
        for rx in region(pos.x - radius)..=region(pos.x + radius) {
            let start = kind
                .spread()
                .candidate(nether.world_seed(), kind.salt(), rx, rz);
            let ours = match kind {
                Kind::Fortress | Kind::Bastion => complex_at(nether, start) == kind,
                _ => true,
            };
            if !ours {
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
