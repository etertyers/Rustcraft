//! Random ticks (M39, used by crops from M40): every tick, in each loaded
//! chunk near a player, `randomTickSpeed` random blocks of every 16³
//! section are picked, and the blocks that want them are ticked.

use glam::IVec3;

use super::Game;
use crate::{
    config,
    world::{
        block::BlockState,
        chunk::{ChunkPos, SECTION_COUNT, SECTION_SIZE},
        fast_hash::FastSet,
    },
};

/// How far from a player (in chunks) blocks get random ticks.
pub const RANDOM_TICK_DISTANCE: i32 = 7;

impl Game {
    /// The blocks that get a random tick this tick, with their states.
    pub(super) fn pick_random_ticks(&mut self) -> Vec<(IVec3, BlockState)> {
        let speed = self.rules.random_tick_speed;
        if speed == 0 {
            return Vec::new();
        }
        let mut near: FastSet<ChunkPos> = FastSet::default();
        for index in 0..self.player_count() {
            if self.player_at(index).dimension != self.dimension {
                continue;
            }
            let feet = self.feet_of(index);
            let center = ChunkPos::of_block(feet.x.floor() as i32, feet.z.floor() as i32);
            for dx in -RANDOM_TICK_DISTANCE..=RANDOM_TICK_DISTANCE {
                for dz in -RANDOM_TICK_DISTANCE..=RANDOM_TICK_DISTANCE {
                    near.insert(center.offset(dx, dz));
                }
            }
        }
        let mut picked = Vec::new();
        let size = SECTION_SIZE as i32;
        for pos in near {
            let Some(chunk) = self.chunks.world().get(pos) else {
                continue;
            };
            let (min_x, min_z) = pos.min_block();
            for section in 0..SECTION_COUNT {
                let Some(blocks) = chunk.section(section) else {
                    continue;
                };
                for _ in 0..speed {
                    let r = self.rng.next_u64();
                    let i = (r % blocks.len() as u64) as usize;
                    let state = blocks[i];
                    if state.def().random_ticks {
                        let (x, z, y) = (
                            i as i32 % size,
                            i as i32 / size % size,
                            i as i32 / (size * size),
                        );
                        picked.push((
                            IVec3::new(
                                min_x + x,
                                config::MIN_Y + section as i32 * size + y,
                                min_z + z,
                            ),
                            state,
                        ));
                    }
                }
            }
        }
        picked
    }

    /// Ticks the blocks `pick_random_ticks` chose.
    pub(super) fn tick_random_blocks(&mut self) {
        for (at, state) in self.pick_random_ticks() {
            // The block may have changed since it was picked.
            if self.chunks.world().block(at.x, at.y, at.z) != Some(state) {
                continue;
            }
            self.random_tick_block(at, state);
        }
    }

    /// One block's random tick.
    fn random_tick_block(&mut self, at: IVec3, state: BlockState) {
        self.mod_block_event(crate::mods::lua::BlockEvent::RandomTick, state, at);
    }
}
