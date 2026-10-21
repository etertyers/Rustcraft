//! Nether fossils (modern `nether_fossil`): a spine and ribs of bone blocks
//! on a soul sand valley floor, between y 32 and y 125. Modern picks one of
//! fourteen templates; this draws one simple skeleton at a random length.

use glam::IVec3;

use super::{Piece, build::Build};
use crate::world::{
    biome::Biome,
    block::BlockState,
    blocks,
    chunk::ChunkPos,
    props::{Prop, axis},
    worldgen::{nether::Probe, rng::Rng},
};

/// The y range of the air block a fossil lies in.
const LOW: i32 = 32;
const HIGH: i32 = 125;
/// Room above the floor a fossil needs.
const ROOM: i32 = 6;

fn bone(axis: u8) -> BlockState {
    BlockState::of(blocks::BONE_BLOCK).with(Prop::Axis, axis)
}

/// Lays out the fossil starting in chunk `start`, if its spot is a valley.
pub fn generate(probe: &mut Probe, start: ChunkPos, rng: &mut Rng) -> Option<Vec<Piece>> {
    let (x0, z0) = start.min_block();
    let (x, z) = (x0 + rng.below(16) as i32, z0 + rng.below(16) as i32);
    if probe.biome(x, z) != Biome::SoulSandValley {
        return None;
    }
    let floors: Vec<i32> = probe
        .floors(x, z)
        .into_iter()
        .filter(|&y| {
            (LOW..=HIGH).contains(&(y + 1)) && (1..=ROOM).all(|up| !probe.solid(x, y + up, z))
        })
        .collect();
    let floor = *floors.get(rng.below(floors.len().max(1) as u32) as usize)?;
    let along_x = rng.below(2) == 0;
    let (along, across, spine) = if along_x {
        (IVec3::X, IVec3::Z, axis::X)
    } else {
        (IVec3::Z, IVec3::X, axis::Z)
    };
    let length = 6 + rng.below(5) as i32;
    let base = IVec3::new(x, floor + 1, z);
    let mut build = Build::new();
    for i in 0..length {
        build.set(base + along * i, bone(spine));
    }
    // Ribs arching over the spine every other block.
    for i in (1..length - 1).step_by(2) {
        for side in [-1, 1] {
            for (out, up) in [(1, 0), (2, 1), (2, 2), (1, 3)] {
                let at = base + along * i + across * (side * out) + IVec3::Y * up;
                build.set(at, bone(if up == 1 || up == 2 { axis::Y } else { axis::X }));
            }
        }
        build.set(base + along * i + IVec3::Y * 3, bone(axis::Y));
    }
    Some(vec![build.finish()])
}
