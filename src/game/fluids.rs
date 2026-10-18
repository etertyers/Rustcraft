//! Water and lava in the played world: the game's block edits wake the
//! fluids around them, scheduled fluid updates run each tick, and what
//! water washes away drops as items.

use glam::IVec3;

use super::Game;
use crate::{
    entity::item::ItemEntity,
    world::{block::BlockState, chunk::ChunkPos, harvest},
};

impl Game {
    /// Sets blocks the player changed, waking fluids next to them.
    pub(super) fn edit(&mut self, edits: &[(IVec3, BlockState)]) {
        self.chunks.set_blocks(edits);
        for &(at, _) in edits {
            self.fluids.changed(at);
        }
    }

    /// One tick of flowing water and lava.
    pub(super) fn tick_fluids(&mut self) {
        let rng = &mut self.rng;
        let mut random = |n: u32| rng.below(n);
        let outcome = self.fluids.tick(
            self.chunks.world(),
            &self.items.blocks,
            self.time,
            &mut random,
        );
        if outcome.edits.is_empty() {
            return;
        }
        // Washed-away blocks drop whatever breaking them by hand would.
        for (at, block) in outcome.washed {
            let Some(def) = self.items.blocks.get(block.id()) else {
                continue;
            };
            let rng = &mut self.rng;
            let mut random = || rng.next_f64() as f32;
            for stack in harvest::drops(def, block.data(), None, &mut random) {
                self.entities
                    .push(ItemEntity::from_block(stack, at, &mut random));
            }
        }
        self.chunks.set_blocks(&outcome.edits);
    }

    /// Schedules the flowing fluid in chunks that just loaded.
    pub(super) fn schedule_fluids(&mut self, chunks: &[ChunkPos]) {
        for &pos in chunks {
            if let Some(chunk) = self.chunks.world().get(pos) {
                self.fluids
                    .chunk_loaded(chunk, &self.items.blocks, self.time);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use glam::{DVec3, IVec3};

    use super::*;
    use crate::{
        game::tests::flat_game, inventory::stack::ItemStack, player::mode::GameMode,
        player::physics::Body,
    };

    const GLASS: BlockState = BlockState::new(20, 0);
    const SOURCE: BlockState = BlockState::new(8, 0);

    fn water_id(game: &Game, x: i32, z: i32) -> Option<u16> {
        let block = game.chunks.world().block(x, 64, z)?;
        matches!(block.id(), 8 | 9).then_some(block.id())
    }

    #[test]
    fn breaking_a_dam_lets_the_water_out() {
        let mut game = flat_game(GameMode::Creative);
        let source = IVec3::new(4, 64, 4);
        let walls: Vec<_> = [IVec3::X, IVec3::NEG_X, IVec3::Z, IVec3::NEG_Z]
            .map(|side| (source + side, GLASS))
            .into();
        game.edit(&walls);
        game.edit(&[(source, SOURCE)]);
        for _ in 0..40 {
            game.tick();
        }
        assert_eq!(water_id(&game, 4, 4), Some(9), "settled in its box");
        assert_eq!(water_id(&game, 6, 4), None);
        game.break_block(source + IVec3::X);
        for _ in 0..40 {
            game.tick();
        }
        assert!(water_id(&game, 6, 4).is_some(), "out through the gap");
    }

    #[test]
    fn the_current_carries_the_player_and_items_downstream() {
        let mut game = flat_game(GameMode::Survival);
        game.edit(&[(IVec3::new(5, 64, 0), SOURCE)]);
        for _ in 0..100 {
            game.tick();
        }
        assert!(water_id(&game, 0, 0).is_some());
        let start = DVec3::new(0.5, 64.0, 0.5);
        game.body = Body::new(start);
        game.prev_feet = start;
        game.entities.clear();
        game.entities.push(ItemEntity::new(
            ItemStack::new(4, 1),
            DVec3::new(0.5, 64.2, 2.5),
            DVec3::ZERO,
            0.0,
        ));
        for _ in 0..20 {
            game.tick();
        }
        // Away from the source, which is east.
        assert!(
            game.body.position().x < start.x - 0.3,
            "{}",
            game.body.position()
        );
        assert!(
            game.entities[0].center().x < 0.2,
            "{}",
            game.entities[0].center()
        );
    }
}
