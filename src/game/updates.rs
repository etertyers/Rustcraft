//! Block updates in the played world (modern Minecraft's neighbor and
//! shape updates): when blocks change, the blocks next to them that hang
//! on their neighbors (fences, stairs, dust) are worked out again, and
//! sand or gravel with nothing under it is scheduled to fall. Falling
//! blocks then fall as entities and land. Chunks upgraded from v1.0 have
//! their connections worked out once their neighbors are loaded.

use std::collections::VecDeque;

use glam::IVec3;

use super::Game;
use crate::{
    entity::{
        falling::{Fall, FallingBlock},
        item::ItemEntity,
    },
    inventory::stack::ItemStack,
    player::collision::WorldCollider,
    world::{Face, block::BlockState, chunk::ChunkPos, connect},
};

/// Ticks between a block update and a block starting to fall (modern
/// `FallingBlock.getDelayAfterPlace`).
const FALL_DELAY: u64 = 2;
/// Most blocks reworked by one round of updates, so a long chain can't
/// stall a tick.
const MAX_SHAPE_UPDATES: usize = 512;

/// Whether a falling block passes through `state` (modern
/// `FallingBlock.isFree`): air, fire, fluids and plants it knocks aside.
pub fn is_free(state: BlockState) -> bool {
    let def = state.def();
    state.is_air() || def.fluid.is_some() || def.is_replaceable(state)
}

impl Game {
    /// Updates the blocks around those at `changed`, which were just set:
    /// blocks whose state hangs on their neighbors are worked out again
    /// (and their neighbors in turn, if they changed), and blocks that
    /// fall are scheduled to.
    pub(super) fn update_neighbors(&mut self, changed: &[IVec3]) {
        let mut queue: VecDeque<IVec3> = changed.iter().copied().collect();
        let mut updates = 0;
        while let Some(at) = queue.pop_front() {
            self.harden(at);
            self.schedule_fall(at);
            for face in Face::ALL {
                let next = at + IVec3::from_array(face.normal());
                self.harden(next);
                self.schedule_fall(next);
                let world = self.chunks.world();
                let Some(state) = world.block(next.x, next.y, next.z) else {
                    continue;
                };
                if !state.def().shaped_by_neighbors() || updates >= MAX_SHAPE_UPDATES {
                    continue;
                }
                updates += 1;
                let shaped = connect::update_shape(state, next, &|p| world.block(p.x, p.y, p.z));
                if shaped != state {
                    self.chunks.set_blocks(&[(next, shaped)]);
                    self.redstone.changed(next);
                    queue.push_back(next);
                }
            }
        }
    }

    /// Concrete powder at `at` touching water turns to concrete (modern
    /// `ConcretePowderBlock.touchesLiquid`).
    fn harden(&mut self, at: IVec3) {
        let world = self.chunks.world();
        let Some(state) = world.block(at.x, at.y, at.z) else {
            return;
        };
        let Some(hard) = state.def().hardens else {
            return;
        };
        let wet = Face::ALL.iter().any(|face| {
            let next = at + IVec3::from_array(face.normal());
            world
                .block(next.x, next.y, next.z)
                .is_some_and(|b| b.def().fluid == Some(crate::world::block::Fluid::Water))
        });
        if wet {
            self.chunks.set_blocks(&[(at, BlockState::of(hard))]);
        }
    }

    /// Schedules the block at `at` to fall, if it falls and has nothing
    /// under it.
    fn schedule_fall(&mut self, at: IVec3) {
        let world = self.chunks.world();
        let Some(state) = world.block(at.x, at.y, at.z) else {
            return;
        };
        if !state.def().falls || self.falls_due.iter().any(|&(_, p)| p == at) {
            return;
        }
        let free = world
            .block(at.x, at.y - 1, at.z)
            .is_some_and(|below| is_free(below) && at.y > 0);
        if free {
            self.falls_due.push((self.time + FALL_DELAY, at));
        }
    }

    /// One tick of falling blocks: those due start falling (if they still
    /// have nothing under them), and those falling move and land.
    pub(super) fn tick_falling(&mut self) {
        let time = self.time;
        let (due, later): (Vec<_>, Vec<_>) = self
            .falls_due
            .drain(..)
            .partition(|&(when, _)| when <= time);
        self.falls_due = later;
        for (_, at) in due {
            let world = self.chunks.world();
            let Some(state) = world.block(at.x, at.y, at.z) else {
                continue;
            };
            let free = world.block(at.x, at.y - 1, at.z).is_some_and(is_free);
            if state.def().falls && free && at.y > 0 {
                self.falling.push(FallingBlock::new(state, at));
                self.edit(&[(at, BlockState::AIR)]);
            }
        }

        let mut falling = std::mem::take(&mut self.falling);
        let mut landed = Vec::new();
        falling.retain_mut(|block| {
            let collider = WorldCollider {
                world: self.chunks.world(),
                blocks: self.items.blocks,
            };
            match block.tick(&collider) {
                Fall::Falling => true,
                Fall::Landed(cell) => {
                    landed.push((block.state, Some(cell)));
                    false
                }
                Fall::Lost => {
                    landed.push((block.state, None));
                    false
                }
            }
        });
        self.falling.extend(falling);
        for (state, cell) in landed {
            self.land(state, cell);
        }
    }

    /// A falling block coming to rest in `cell`: it's placed there if the
    /// cell can take it and something holds it up, otherwise it drops as
    /// its item (modern `FallingBlockEntity.tick`).
    fn land(&mut self, state: BlockState, cell: Option<IVec3>) {
        if let Some(at) = cell {
            let world = self.chunks.world();
            let here = world.block(at.x, at.y, at.z);
            let below = world.block(at.x, at.y - 1, at.z);
            let fits = here.is_some_and(|here| here.def().is_replaceable(here));
            let held = below.is_some_and(|below| !is_free(below));
            if fits && held {
                // Concrete powder landing in water sets at once.
                let in_water = here.is_some_and(|here| {
                    here.def().fluid == Some(crate::world::block::Fluid::Water)
                });
                let placed = match state.def().hardens {
                    Some(hard) if in_water => BlockState::of(hard),
                    _ => state,
                };
                self.edit(&[(at, placed)]);
                return;
            }
        }
        let Some(item) = self.items.items.of_block(state.id()) else {
            return;
        };
        let at = cell.unwrap_or_else(|| self.body.position().floor().as_ivec3());
        let rng = &mut self.rng;
        let mut random = || rng.next_f64() as f32;
        self.entities.push(ItemEntity::from_block(
            ItemStack::new(item, 1),
            at,
            &mut random,
        ));
    }

    /// Works out the connections of upgraded v1.0 blocks in chunks whose
    /// neighbors have all loaded.
    pub(super) fn post_process(&mut self) {
        let ready: Vec<ChunkPos> = self
            .post_processing
            .iter()
            .copied()
            .filter(|&pos| self.chunks.loaded_around(pos))
            .collect();
        for pos in ready {
            self.post_processing.retain(|&p| p != pos);
            let (min_x, min_z) = pos.min_block();
            let cells: Vec<IVec3> = self
                .chunks
                .take_post_process(pos)
                .into_iter()
                .map(|[x, y, z]| IVec3::new(min_x + x as i32, y as i32, min_z + z as i32))
                .collect();
            let world = self.chunks.world();
            let edits: Vec<(IVec3, BlockState)> = cells
                .iter()
                .filter_map(|&at| {
                    let state = world.block(at.x, at.y, at.z)?;
                    let shaped = connect::update_shape(state, at, &|p| world.block(p.x, p.y, p.z));
                    (shaped != state).then_some((at, shaped))
                })
                .collect();
            if !edits.is_empty() {
                self.chunks.set_blocks_later(&edits);
            }
        }
    }

    /// Notes chunks that just loaded with blocks to join up.
    pub(super) fn queue_post_process(&mut self, chunks: &[ChunkPos]) {
        for &pos in chunks {
            let waiting = self
                .chunks
                .world()
                .get(pos)
                .is_some_and(|chunk| !chunk.post_process.is_empty());
            if waiting && !self.post_processing.contains(&pos) {
                self.post_processing.push(pos);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use glam::IVec3;

    use crate::{
        game::tests::flat_game,
        inventory::items,
        player::mode::GameMode,
        world::{
            block::BlockState,
            blocks,
            props::{Prop, slab_type},
        },
    };

    #[test]
    fn sand_above_a_dug_out_hole_falls_and_lands() {
        let mut game = flat_game(GameMode::Creative);
        // A sand pillar on a stone block, as worldgen leaves it: it stays
        // put until something updates it.
        let sand = BlockState::of(blocks::SAND);
        game.chunks.set_blocks(&[
            (IVec3::new(3, 64, 3), BlockState::of(blocks::STONE)),
            (IVec3::new(3, 65, 3), sand),
            (IVec3::new(3, 66, 3), sand),
        ]);
        game.run_ticks(10);
        assert_eq!(game.chunks.world().block(3, 65, 3), Some(sand));
        // Dig out the stone, and the grass and dirt under it.
        game.set_blocks(&[
            (IVec3::new(3, 64, 3), BlockState::AIR),
            (IVec3::new(3, 63, 3), BlockState::AIR),
            (IVec3::new(3, 62, 3), BlockState::AIR),
        ]);
        game.run_ticks(3);
        assert!(!game.falling.is_empty(), "the bottom sand started falling");
        game.run_ticks(60);
        assert!(game.falling.is_empty());
        let world = game.chunks.world();
        assert_eq!(world.block(3, 61, 3), Some(BlockState::of(blocks::DIRT)));
        assert_eq!(world.block(3, 62, 3), Some(sand), "both fell to the dirt");
        assert_eq!(world.block(3, 63, 3), Some(sand));
        assert_eq!(world.block(3, 65, 3), Some(BlockState::AIR));
    }

    #[test]
    fn sand_landing_on_a_torch_or_slab_drops_as_an_item() {
        let mut game = flat_game(GameMode::Survival);
        game.entities.clear();
        let slab = BlockState::of(blocks::OAK_SLAB).with(Prop::SlabType, slab_type::BOTTOM);
        game.chunks.set_blocks(&[
            (IVec3::new(4, 64, 4), BlockState::of(blocks::TORCH)),
            (IVec3::new(6, 64, 4), slab),
        ]);
        game.set_blocks(&[
            (IVec3::new(4, 70, 4), BlockState::of(blocks::SAND)),
            (IVec3::new(6, 70, 4), BlockState::of(blocks::GRAVEL)),
        ]);
        game.run_ticks(80);
        let world = game.chunks.world();
        assert_eq!(world.block(4, 64, 4), Some(BlockState::of(blocks::TORCH)));
        assert_eq!(world.block(6, 64, 4), Some(slab));
        let dropped: Vec<_> = game.entities.iter().map(|item| item.stack.id).collect();
        assert!(dropped.contains(&items::SAND) && dropped.contains(&items::GRAVEL));
    }

    #[test]
    fn concrete_powder_sets_beside_water_and_anvils_fall() {
        let mut game = flat_game(GameMode::Creative);
        let powder = BlockState::of(blocks::RED_CONCRETE_POWDER);
        game.set_blocks(&[(IVec3::new(2, 64, 2), powder)]);
        game.run_ticks(5);
        assert_eq!(
            game.chunks.world().block(2, 64, 2),
            Some(powder),
            "dry, it stays"
        );
        game.set_blocks(&[(IVec3::new(3, 64, 2), BlockState::of(blocks::WATER))]);
        game.run_ticks(1);
        assert_eq!(
            game.chunks.world().block(2, 64, 2),
            Some(BlockState::of(blocks::RED_CONCRETE))
        );
        // An anvil placed in the air falls like sand.
        let anvil = BlockState::of(blocks::ANVIL);
        game.set_blocks(&[(IVec3::new(6, 70, 6), anvil)]);
        game.run_ticks(60);
        assert_eq!(game.chunks.world().block(6, 64, 6), Some(anvil));
    }

    #[test]
    fn fences_join_and_part_as_blocks_beside_them_come_and_go() {
        let mut game = flat_game(GameMode::Creative);
        let fence = BlockState::of(blocks::OAK_FENCE);
        game.set_blocks(&[(IVec3::new(2, 64, 2), fence)]);
        game.set_blocks(&[(IVec3::new(3, 64, 2), BlockState::of(blocks::STONE))]);
        let joined = game.chunks.world().block(2, 64, 2).unwrap();
        assert!(joined.bool(Prop::East));
        game.set_blocks(&[(IVec3::new(3, 64, 2), BlockState::AIR)]);
        assert!(
            !game
                .chunks
                .world()
                .block(2, 64, 2)
                .unwrap()
                .bool(Prop::East)
        );
    }
}
