//! Dungeons (modern `MonsterRoomFeature`): a small cobblestone room with
//! a mossy floor, a spawner of zombies, skeletons or spiders, and one or
//! two chests against the walls. A room is only built where its floor and
//! ceiling are solid and a cave breaks into its walls in one to five
//! places.

use glam::IVec3;

use super::{Piece, build::Build};
use crate::world::{
    block::BlockState,
    blocks,
    chunk::ChunkPos,
    worldgen::{rng::Rng, terrain::Terrain},
};

use crate::config::MIN_Y;

/// The share of chunks that try for a dungeon.
pub const CHANCE: f64 = 0.4;
/// Spots tried in such a chunk.
const TRIES: u32 = 8;

/// Whether the cell is rock (under the surface, not a cave).
pub fn solid(terrain: &Terrain, at: IVec3) -> bool {
    at.y > MIN_Y + 4 && at.y <= terrain.height(at.x, at.z) && !terrain.is_cave(at.x, at.y, at.z)
}

pub fn generate(terrain: &Terrain, start: ChunkPos, rng: &mut Rng) -> Option<Vec<Piece>> {
    let (x0, z0) = start.min_block();
    // The ground's height once for the chunk: a room stays well under it.
    let ground = terrain.height(x0 + 8, z0 + 8);
    for _ in 0..TRIES {
        let center = IVec3::new(
            x0 + rng.below(16) as i32,
            // Modern `monster_room` and `monster_room_deep`: from just
            // over the bedrock up under the ground.
            MIN_Y + 6 + rng.below(160) as i32,
            z0 + rng.below(16) as i32,
        );
        let (rx, rz) = (2 + rng.below(2) as i32, 2 + rng.below(2) as i32);
        if center.y + 5 + 12 > ground {
            continue;
        }
        if let Some(piece) = room(terrain, center, rx, rz, rng) {
            return Some(vec![piece]);
        }
    }
    None
}

/// Rock here (the caller keeps rooms 12 blocks under the ground, so only
/// caves matter).
fn rock(terrain: &Terrain, at: IVec3) -> bool {
    at.y > MIN_Y + 4 && !terrain.is_cave(at.x, at.y, at.z)
}

fn room(terrain: &Terrain, c: IVec3, rx: i32, rz: i32, rng: &mut Rng) -> Option<Piece> {
    let (lo, hi) = (
        c + IVec3::new(-rx - 1, -1, -rz - 1),
        c + IVec3::new(rx + 1, 4, rz + 1),
    );
    // Openings first: most spots have no cave near, and that's cheap to
    // see. An opening is a cave at the wall's foot with room above it.
    let mut openings = 0;
    for x in lo.x..=hi.x {
        for z in lo.z..=hi.z {
            let wall = x == lo.x || x == hi.x || z == lo.z || z == hi.z;
            if wall
                && !rock(terrain, IVec3::new(x, c.y, z))
                && !rock(terrain, IVec3::new(x, c.y + 1, z))
            {
                openings += 1;
                if openings > 5 {
                    return None;
                }
            }
        }
    }
    if openings == 0 {
        return None;
    }
    // Floor and ceiling must be rock.
    for x in lo.x..=hi.x {
        for z in lo.z..=hi.z {
            if !rock(terrain, IVec3::new(x, lo.y, z)) || !rock(terrain, IVec3::new(x, hi.y, z)) {
                return None;
            }
        }
    }
    let mut build = Build::new();
    let cobble = BlockState::of(blocks::COBBLESTONE);
    let mossy = BlockState::of(blocks::MOSSY_COBBLESTONE);
    let mut moss = Rng::new(rng.next_u64());
    for y in lo.y..=hi.y {
        for z in lo.z..=hi.z {
            for x in lo.x..=hi.x {
                let at = IVec3::new(x, y, z);
                let shell =
                    x == lo.x || x == hi.x || z == lo.z || z == hi.z || y == lo.y || y == hi.y;
                let block = if !shell {
                    BlockState::AIR
                } else if y == lo.y && moss.below(4) != 0 {
                    mossy
                } else if y == lo.y || y == hi.y {
                    cobble
                } else if !rock(terrain, at) {
                    // Leave the openings the cave made.
                    continue;
                } else {
                    cobble
                };
                build.set(at, block);
            }
        }
    }
    let mob = match rng.below(4) {
        0 | 1 => "zombie",
        2 => "skeleton",
        _ => "spider",
    };
    build.spawner(c, mob);
    // One or two chests, each against a wall.
    let chests = 1 + rng.below(2);
    let mut placed: Vec<IVec3> = Vec::new();
    for _ in 0..chests {
        let along = |span: i32, rng: &mut Rng| -span + rng.below((2 * span + 1) as u32) as i32;
        let at = match rng.below(4) {
            0 => c + IVec3::new(-rx, 0, along(rz, rng)),
            1 => c + IVec3::new(rx, 0, along(rz, rng)),
            2 => c + IVec3::new(along(rx, rng), 0, -rz),
            _ => c + IVec3::new(along(rx, rng), 0, rz),
        };
        if at != c && !placed.contains(&at) {
            placed.push(at);
            build.chest(at, BlockState::of(blocks::CHEST), "simple_dungeon");
        }
    }
    Some(build.finish())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::structure::{Kind, structure_at};

    #[test]
    fn dungeons_are_rooms_with_a_spawner_and_chests_underground() {
        let terrain = Terrain::new(20111118);
        let mut found = 0;
        for z in -12..12 {
            for x in -12..12 {
                let Some(dungeon) = structure_at(&terrain, Kind::Dungeon, ChunkPos::new(x, z))
                else {
                    continue;
                };
                found += 1;
                let piece = &dungeon.pieces[0];
                assert_eq!(piece.spawners.len(), 1);
                assert!(["zombie", "skeleton", "spider"].contains(&piece.spawners[0].1.as_str()));
                assert!(!piece.loot.is_empty() && piece.loot.len() <= 2);
                let (at, _) = piece.spawners[0];
                assert!(at.y < terrain.height(at.x, at.z), "underground");
            }
        }
        // Rare but not unheard of: modern has a few per dozen chunks.
        assert!(found >= 5, "{found}");
    }
}
