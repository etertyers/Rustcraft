//! Bastion remnants (modern has four layouts from templates; this is one,
//! drawn in code after them): a keep of polished blackstone bricks on the
//! lava sea's shore at y 33, standing on blackstone down to the ground,
//! three storeys tall with ramparts round the top. The bottom is the
//! treasure room (gold blocks round a magma core, the treasure chest),
//! the upper floors hold chests, and a hoglin stable leans on one side.
//! Piglins, piglin brutes and hoglins live there.
//!
//! Also the Nether's ruined portals: a broken obsidian frame with crying
//! obsidian on a blackstone and netherrack platform, with a chest.

use glam::{DVec3, IVec3};

use super::{Piece, build::Build};
use crate::world::{
    block::BlockState,
    blocks,
    chunk::ChunkPos,
    worldgen::{nether::Probe, rng::Rng},
};

/// The keep's floor (modern bastions start at y 33).
pub const BASE: i32 = 33;
/// Its footprint, half across, and storey height.
const HALF: i32 = 11;
const STOREY: i32 = 7;
const STOREYS: i32 = 3;

/// The bricks of the walls, cracked and gilded now and then, the same
/// every time.
fn brick(at: IVec3) -> BlockState {
    let h = (at.x as u32).wrapping_mul(73_856_093)
        ^ (at.y as u32).wrapping_mul(19_349_663)
        ^ (at.z as u32).wrapping_mul(83_492_791);
    match h.wrapping_mul(2_654_435_761) >> 26 {
        0 => BlockState::of(blocks::GILDED_BLACKSTONE),
        1..=10 => BlockState::of(blocks::CRACKED_POLISHED_BLACKSTONE_BRICKS),
        11..=18 => BlockState::of(blocks::BLACKSTONE),
        _ => BlockState::of(blocks::POLISHED_BLACKSTONE_BRICKS),
    }
}

/// Blackstone pillars from below `tops` down to the ground.
fn foundations(probe: &mut Probe, tops: impl IntoIterator<Item = IVec3>) -> Piece {
    let mut build = Build::new();
    for top in tops {
        let mut y = top.y;
        while y > 1 && !probe.solid(top.x, y, top.z) {
            build.set(
                IVec3::new(top.x, y, top.z),
                BlockState::of(blocks::BLACKSTONE),
            );
            y -= 1;
        }
    }
    let mut piece = build.finish();
    piece.support = true;
    piece
}

/// Lays out the bastion starting in chunk `start`.
pub fn generate(probe: &mut Probe, start: ChunkPos, rng: &mut Rng) -> Option<Vec<Piece>> {
    let (x, z) = start.min_block();
    let c = IVec3::new(x + 8, BASE, z + 8);
    let top = BASE + STOREY * STOREYS;
    let mut build = Build::new();
    let at = |dx: i32, dy: i32, dz: i32| c + IVec3::new(dx, dy, dz);
    // The shell, floors and the ramparts.
    build.hollow(
        at(-HALF, -1, -HALF),
        at(HALF, STOREY * STOREYS, HALF),
        brick,
    );
    for storey in 1..STOREYS {
        let y = storey * STOREY;
        build.fill_with(
            at(-HALF + 1, y, -HALF + 1),
            at(HALF - 1, y, HALF - 1),
            |p| {
                // A hole in the middle of each floor, to drop through.
                if (p.x - c.x).abs() <= 2 && (p.z - c.z).abs() <= 2 {
                    BlockState::AIR
                } else {
                    BlockState::of(blocks::POLISHED_BLACKSTONE)
                }
            },
        );
    }
    for dz in -HALF..=HALF {
        for dx in -HALF..=HALF {
            let edge = dx.abs() == HALF || dz.abs() == HALF;
            if edge && (dx + dz).rem_euclid(2) == 0 {
                build.set(
                    at(dx, STOREY * STOREYS + 1, dz),
                    BlockState::of(blocks::POLISHED_BLACKSTONE_BRICK_WALL),
                );
            }
        }
    }
    // Doorways on each side and windows.
    for (sx, sz) in [(1i32, 0i32), (-1, 0), (0, 1), (0, -1)] {
        for dy in 0..4 {
            for k in -1..=1 {
                let (dx, dz) = (sx * HALF + sz.abs() * k, sz * HALF + sx.abs() * k);
                build.set(at(dx, dy, dz), BlockState::AIR);
            }
        }
        for storey in 1..STOREYS {
            for k in [-6, 6] {
                let (dx, dz) = (sx * HALF + sz.abs() * k, sz * HALF + sx.abs() * k);
                let y = storey * STOREY + 2;
                build.set(at(dx, y, dz), BlockState::AIR);
                build.set(at(dx, y + 1, dz), BlockState::AIR);
            }
        }
    }
    // The treasure room: a magma core ringed with gold blocks, chains
    // hanging from the floor above, and the treasure chest.
    build.fill(
        at(-1, 0, -1),
        at(1, 0, 1),
        BlockState::of(blocks::MAGMA_BLOCK),
    );
    for (dx, dz) in [
        (-3, -3),
        (3, -3),
        (-3, 3),
        (3, 3),
        (0, -4),
        (0, 4),
        (-4, 0),
        (4, 0),
    ] {
        build.set(at(dx, 0, dz), BlockState::of(blocks::GOLD_BLOCK));
    }
    for (dx, dz) in [(-5, -5), (5, -5), (-5, 5), (5, 5)] {
        for dy in 3..STOREY {
            build.set(at(dx, dy, dz), BlockState::of(blocks::CHAIN));
        }
    }
    build.chest(
        at(0, 1, -5),
        BlockState::of(blocks::CHEST),
        "bastion_treasure",
    );
    // Chests on the upper floors.
    for (storey, (dx, dz)) in [(1, (-8, 8)), (2, (8, -8)), (2, (-8, -8))] {
        build.chest(
            at(dx, storey * STOREY + 1, dz),
            BlockState::of(blocks::CHEST),
            "bastion_other",
        );
    }
    // Who lives there.
    let mid = |dx: i32, dy: i32, dz: i32| at(dx, dy, dz).as_dvec3() + DVec3::new(0.5, 0.0, 0.5);
    for i in 0..4 + rng.below(3) as i32 {
        let storey = i % STOREYS;
        let spot = mid(-6 + (i * 5) % 12, storey * STOREY + 1, 6 - (i * 7) % 12);
        build.mob(spot, "piglin", None);
    }
    build.mob(mid(4, 1, 4), "piglin_brute", None);
    build.mob(mid(-4, 1, -4), "piglin_brute", None);
    let mut keep = build.finish();

    // The hoglin stable on the east side.
    let mut stable = Build::new();
    let (s0, s1) = (at(HALF + 1, -1, -6), at(HALF + 11, 5, 6));
    stable.hollow(s0, s1, brick);
    stable.fill(at(HALF, 0, -1), at(HALF, 2, 1), BlockState::AIR);
    stable.fill(at(HALF + 2, 0, -5), at(HALF + 10, 0, 5), BlockState::AIR);
    for dz in [-5, 5] {
        for dx in HALF + 2..=HALF + 10 {
            stable.set(
                at(dx, 0, dz),
                BlockState::of(blocks::POLISHED_BLACKSTONE_BRICK_WALL),
            );
        }
    }
    stable.chest(
        at(HALF + 10, 0, 0),
        BlockState::of(blocks::CHEST),
        "bastion_hoglin_stable",
    );
    for i in 0..2 + rng.below(2) as i32 {
        stable.mob(mid(HALF + 4 + i * 2, 0, -2 + i), "hoglin", None);
    }
    keep.compute_bounds();
    let stable = stable.finish();

    // All of it stands on blackstone.
    let mut tops = Vec::new();
    for dz in -HALF..=HALF {
        for dx in -HALF..=HALF + 11 {
            if dx > HALF && dz.abs() > 6 {
                continue;
            }
            tops.push(at(dx, -2, dz));
        }
    }
    let base = foundations(probe, tops);
    // The keep shouldn't poke through the Nether's roof.
    if top > 118 {
        return None;
    }
    Some(vec![base, keep, stable])
}

/// A ruined portal near the middle of chunk `start`: on the first floor
/// above the lava there, or on a platform on the lava sea.
pub fn ruined_portal(probe: &mut Probe, start: ChunkPos, rng: &mut Rng) -> Option<Vec<Piece>> {
    let (x, z) = start.min_block();
    let (x, z) = (x + 8, z + 8);
    // The lowest floor with room for the frame above it, or the lava sea.
    let lava = crate::world::worldgen::nether::LAVA_LEVEL;
    let mut floors = probe.floors(x, z);
    floors.reverse();
    let floor = floors
        .into_iter()
        .find(|&y| y > lava && (1..=6).all(|up| !probe.solid(x, y + up, z)))
        .unwrap_or(lava);
    let base = IVec3::new(x, floor, z);
    let along_x = rng.below(2) == 0;
    let step = if along_x { IVec3::X } else { IVec3::Z };
    let across = IVec3::ONE - step - IVec3::Y;
    let mut build = Build::new();
    // Room round the frame, then the platform: blackstone, netherrack and
    // magma.
    for a in -3..=4 {
        for b in -2..=2 {
            for up in 1..=6 {
                build.set(
                    base + step * a + across * b + IVec3::Y * up,
                    BlockState::AIR,
                );
            }
        }
    }
    for a in -3..=4 {
        for b in -2..=2 {
            let at = base + step * a + across * b;
            let roll = rng.below(10);
            let block = match roll {
                0..=4 => blocks::BLACKSTONE,
                5..=7 => blocks::NETHERRACK,
                8 => blocks::MAGMA_BLOCK,
                _ => continue,
            };
            build.set(at, BlockState::of(block));
        }
    }
    // The frame, with a fifth of it missing and some crying.
    for i in -1..=2 {
        for up in 0..=4 {
            let side = i == -1 || i == 2;
            let end = up == 0 || up == 4;
            if !side && !end {
                continue;
            }
            let at = base + step * i + IVec3::Y * (up + 1);
            let roll = rng.below(10);
            if roll < 2 {
                continue;
            }
            let block = if roll < 4 {
                blocks::CRYING_OBSIDIAN
            } else {
                blocks::OBSIDIAN
            };
            build.set(at, BlockState::of(block));
        }
    }
    build.set(
        base + across * 2 + step * 3 + IVec3::Y,
        BlockState::of(blocks::GOLD_BLOCK),
    );
    build.chest(
        base + across * -2 + IVec3::Y,
        BlockState::of(blocks::CHEST),
        "ruined_portal",
    );
    let piece = build.finish();
    let tops =
        (-3..=4).flat_map(|a| (-2..=2).map(move |b| base + step * a + across * b - IVec3::Y));
    let support = foundations(probe, tops.collect::<Vec<_>>());
    Some(vec![support, piece])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::{
        structure::{Kind, nether},
        worldgen::nether::Nether,
    };

    #[test]
    fn a_bastion_has_its_treasure_chest_gold_and_garrison() {
        let world = Nether::new(20111118);
        let found = (0..20)
            .filter_map(|i| nether::find(&world, Kind::Bastion, ChunkPos::new(0, i * 40), 60))
            .next()
            .expect("a bastion");
        let loot: Vec<&str> = found
            .pieces
            .iter()
            .flat_map(|p| &p.loot)
            .map(|(_, t)| t.as_str())
            .collect();
        assert!(loot.contains(&"bastion_treasure"));
        assert!(loot.contains(&"bastion_hoglin_stable"));
        assert_eq!(loot.iter().filter(|t| **t == "bastion_other").count(), 3);
        let mobs: Vec<&str> = found.mobs().map(|m| m.mob.as_str()).collect();
        assert!(mobs.iter().filter(|m| **m == "piglin").count() >= 4);
        assert_eq!(mobs.iter().filter(|m| **m == "piglin_brute").count(), 2);
        assert!(mobs.contains(&"hoglin"));
        let blocks: Vec<_> = found.pieces.iter().flat_map(|p| &p.blocks).collect();
        assert!(
            blocks
                .iter()
                .filter(|(_, b)| b.is(blocks::GOLD_BLOCK))
                .count()
                >= 8
        );
    }

    #[test]
    fn nether_ruined_portals_have_a_broken_frame_and_a_chest() {
        let world = Nether::new(20111118);
        let found = nether::find(&world, Kind::NetherRuinedPortal, ChunkPos::new(0, 0), 60)
            .expect("a ruined portal");
        let blocks: Vec<_> = found.pieces.iter().flat_map(|p| &p.blocks).collect();
        let frame = blocks
            .iter()
            .filter(|(_, b)| b.is(blocks::OBSIDIAN) || b.is(blocks::CRYING_OBSIDIAN))
            .count();
        assert!((6..14).contains(&frame), "{frame} frame blocks of 14");
        assert!(
            found
                .pieces
                .iter()
                .any(|p| p.loot.iter().any(|(_, t)| t == "ruined_portal"))
        );
    }
}
