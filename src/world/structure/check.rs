//! `--structure-test`: generates a sample of ruined portals, geodes and
//! Nether fortresses and checks each against M33's rules, printing a line
//! per structure.
//!
//! - A ruined portal's cleared inside is open (air or water) in the
//!   generated world, its chest is there, and its box isn't sealed in
//!   rock: some cell just outside it is open too.
//! - A geode (one worldgen didn't cancel) has no more than
//!   `GEODE_INVALID_THRESHOLD` of its shell points open.
//! - A fortress has at least one blaze spawner, standing where blazes
//!   and wither skeletons spawn on their own.

use std::collections::HashMap;

use glam::IVec3;

use super::{Kind, nether, structure_at};
use crate::world::{
    block::BlockState,
    blocks,
    chunk::{Chunk, ChunkPos},
    worldgen::{
        nether::Nether,
        terrain::Terrain,
        underground::{GEODE_INVALID_THRESHOLD, geode_at},
    },
};

/// One structure's result.
#[derive(Debug, Clone)]
pub struct Check {
    pub what: String,
    pub at: IVec3,
    pub pass: bool,
    pub detail: String,
}

/// Generated chunks, made as they're asked for.
struct Chunks<'a> {
    terrain: &'a Terrain,
    made: HashMap<ChunkPos, Chunk>,
}

impl Chunks<'_> {
    fn block(&mut self, at: IVec3) -> BlockState {
        let pos = ChunkPos::of_block(at.x, at.z);
        let chunk = self
            .made
            .entry(pos)
            .or_insert_with(|| self.terrain.generate(pos));
        let (x0, z0) = pos.min_block();
        // Below the world is as good as bedrock; above it, air.
        if at.y < 0 {
            return BlockState::of(blocks::BEDROCK);
        }
        if at.y >= crate::config::WORLD_HEIGHT as i32 {
            return BlockState::AIR;
        }
        chunk.get((at.x - x0) as usize, at.y as usize, (at.z - z0) as usize)
    }
}

/// Air, fluid, or something you walk through or over (plants, a snow
/// layer).
fn open(block: BlockState) -> bool {
    block.is_air() || block.def().fluid.is_some() || !block.def().solid || block.is(blocks::SNOW)
}

/// The first `count` ruined portals of `seed` and how they fare.
pub fn ruined_portals(seed: u64, count: usize) -> Vec<Check> {
    let terrain = Terrain::new(seed);
    let mut chunks = Chunks {
        terrain: &terrain,
        made: HashMap::new(),
    };
    let kind = Kind::RuinedPortal;
    let spread = kind.spread();
    let mut out = Vec::new();
    'search: for ring in 0..40i32 {
        for rz in -ring..=ring {
            for rx in -ring..=ring {
                if rx.abs() != ring && rz.abs() != ring {
                    continue;
                }
                let start = spread.candidate(seed, kind.salt(), rx, rz);
                let Some(portal) = structure_at(&terrain, kind, start) else {
                    continue;
                };
                out.push(check_portal(&mut chunks, &terrain, &portal));
                if out.len() >= count {
                    break 'search;
                }
            }
        }
    }
    out
}

fn check_portal(chunks: &mut Chunks, terrain: &Terrain, portal: &super::Structure) -> Check {
    let piece = &portal.pieces[0];
    let cleared: Vec<IVec3> = piece
        .blocks
        .iter()
        .filter(|(_, b)| b.is_air() || b.is(blocks::WATER))
        .map(|&(p, _)| p)
        .collect();
    let blocked_cells: Vec<BlockState> = cleared
        .iter()
        .map(|&p| chunks.block(p))
        .filter(|&b| !open(b))
        .collect();
    let blocked = blocked_cells.len();
    let mut names: Vec<&str> = blocked_cells
        .iter()
        .map(|b| b.def().name.as_str())
        .collect();
    names.sort_unstable();
    names.dedup();
    let chest = piece
        .loot
        .first()
        .is_some_and(|&(p, _)| chunks.block(p).is(blocks::CHEST));
    let bounds = piece.bounds.expect("a placed piece has bounds");
    // Just outside the box: its sides and top.
    let mut outside = Vec::new();
    for y in bounds.min.y + 1..=bounds.max.y + 1 {
        for x in bounds.min.x - 1..=bounds.max.x + 1 {
            for z in bounds.min.z - 1..=bounds.max.z + 1 {
                let inside = (bounds.min.x..=bounds.max.x).contains(&x)
                    && (bounds.min.z..=bounds.max.z).contains(&z)
                    && y <= bounds.max.y;
                if !inside {
                    outside.push(IVec3::new(x, y, z));
                }
            }
        }
    }
    let exposed = outside.iter().filter(|&&p| open(chunks.block(p))).count();
    let center = bounds.center();
    let biome = terrain.column(center.x, center.z).biome;
    let height = terrain.height(center.x, center.z);
    let kind = placement_name(bounds.min.y, height, biome);
    let pass = blocked == 0 && chest && exposed > 0;
    Check {
        what: format!("ruined portal ({kind}, {})", biome.name()),
        at: bounds.min,
        pass,
        detail: format!(
            "{} cleared cells, {blocked} blocked{}; chest {}; {exposed} open cells around",
            cleared.len(),
            if names.is_empty() {
                String::new()
            } else {
                format!(" (by {})", names.join(", "))
            },
            if chest { "there" } else { "missing" }
        ),
    }
}

/// A guess at the placement from where the floor sits.
fn placement_name(floor: i32, surface: i32, biome: crate::world::biome::Biome) -> &'static str {
    if floor < surface - 10 {
        "underground"
    } else if floor < surface - 2 {
        if biome == crate::world::biome::Biome::Desert {
            "partly buried"
        } else {
            "in a mountain"
        }
    } else if floor < crate::config::SEA_LEVEL - 1 {
        "ocean floor"
    } else {
        "land surface"
    }
}

/// The first `count` geodes of `seed` (spiralling out from the origin's
/// chunk), and how many were cancelled on the way.
pub fn geodes(seed: u64, count: usize) -> (Vec<Check>, usize) {
    let terrain = Terrain::new(seed);
    let mut chunks = Chunks {
        terrain: &terrain,
        made: HashMap::new(),
    };
    let mut out = Vec::new();
    let mut cancelled = 0;
    'search: for ring in 0..60i32 {
        for z in -ring..=ring {
            for x in -ring..=ring {
                if x.abs() != ring && z.abs() != ring {
                    continue;
                }
                let pos = ChunkPos::new(x, z);
                if !rolls_geode(&terrain, pos) {
                    continue;
                }
                let Some(geode) = geode_at(&terrain, pos) else {
                    cancelled += 1;
                    continue;
                };
                let points: Vec<IVec3> = geode.shell().collect();
                let open_points = points.iter().filter(|&&p| open(chunks.block(p))).count();
                let basalt = points
                    .iter()
                    .filter(|&&p| chunks.block(p).is(blocks::SMOOTH_BASALT))
                    .count();
                out.push(Check {
                    what: "geode".to_owned(),
                    at: geode.center,
                    pass: open_points <= GEODE_INVALID_THRESHOLD,
                    detail: format!(
                        "{open_points} of {} shell points open (at most {GEODE_INVALID_THRESHOLD}), {basalt} smooth basalt",
                        points.len()
                    ),
                });
                if out.len() >= count {
                    break 'search;
                }
            }
        }
    }
    (out, cancelled)
}

/// Whether chunk `pos` rolls a geode before the cancel check.
fn rolls_geode(terrain: &Terrain, pos: ChunkPos) -> bool {
    crate::world::worldgen::underground::rolls_geode(terrain, pos)
}

/// The first `count` fortresses of `seed`.
pub fn fortresses(seed: u64, count: usize) -> Vec<Check> {
    let world = Nether::new(seed);
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    'search: for ring in 0..20i32 {
        for j in -ring..=ring {
            for i in -ring..=ring {
                if i.abs() != ring && j.abs() != ring {
                    continue;
                }
                let Some(found) =
                    nether::find(&world, Kind::Fortress, ChunkPos::new(i * 30, j * 30), 30)
                else {
                    continue;
                };
                if !seen.insert(found.start) {
                    continue;
                }
                let spawners: Vec<IVec3> = found
                    .pieces
                    .iter()
                    .flat_map(|p| &p.spawners)
                    .filter(|(_, m)| m == "blaze")
                    .map(|&(p, _)| p)
                    .collect();
                // Blazes and wither skeletons spawn where the fortress is.
                let spawning = spawners
                    .iter()
                    .filter(|&&p| nether::kind_at(&world, p + IVec3::Y) == Some(Kind::Fortress))
                    .count();
                out.push(Check {
                    what: "fortress".to_owned(),
                    at: found.bounds.min,
                    pass: !spawners.is_empty() && spawning == spawners.len(),
                    detail: format!(
                        "{} blaze spawners, {spawning} inside the fortress's spawning bounds",
                        spawners.len()
                    ),
                });
                if out.len() >= count {
                    break 'search;
                }
            }
        }
    }
    out
}

/// Runs every check and prints them. Returns whether all passed.
pub fn run(seed: u64) -> bool {
    let portals = ruined_portals(seed, 20);
    let (geodes, cancelled) = geodes(seed, 20);
    let forts = fortresses(seed, 5);
    let mut all = true;
    for (title, checks) in [
        ("Ruined portals", &portals),
        ("Geodes", &geodes),
        ("Nether fortresses", &forts),
    ] {
        let passed = checks.iter().filter(|c| c.pass).count();
        println!("{title}: {passed}/{} pass", checks.len());
        for c in checks.iter() {
            println!(
                "  {} {} at {} {} {}: {}",
                if c.pass { "PASS" } else { "FAIL" },
                c.what,
                c.at.x,
                c.at.y,
                c.at.z,
                c.detail
            );
        }
        all &= passed == checks.len();
    }
    println!("({cancelled} geodes cancelled on the way for an open shell)");
    println!("{}", if all { "all pass" } else { "SOME FAIL" });
    all
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sampled_ruined_portals_are_open_inside() {
        let checks = ruined_portals(20111118, 8);
        assert_eq!(checks.len(), 8);
        for c in &checks {
            assert!(c.pass, "{} at {}: {}", c.what, c.at, c.detail);
        }
    }

    #[test]
    fn sampled_geodes_are_set_in_rock() {
        let (checks, _) = geodes(20111118, 6);
        assert_eq!(checks.len(), 6);
        for c in &checks {
            assert!(c.pass, "{} at {}: {}", c.what, c.at, c.detail);
        }
    }
}
