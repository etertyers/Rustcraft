//! Water, lava and fire in the played world: the game's block edits wake
//! the fluids and fires around them, scheduled updates run each tick, what
//! water washes away drops as items, and lava near the player starts
//! fires.

use glam::{DVec3, IVec3};

use super::Game;
use crate::{
    config,
    entity::item::ItemEntity,
    inventory::{item::Bucket, stack::ItemStack},
    player::raycast,
    world::{block::BlockState, chunk::ChunkPos, fluid, harvest},
};

/// Chunks around the player, each way, whose blocks get random ticks
/// (modern Minecraft ticks those within 128 blocks).
const RANDOM_TICK_CHUNKS: i32 = 8;

impl Game {
    /// Sets blocks, waking fluids and fires next to them.
    pub(super) fn edit(&mut self, edits: &[(IVec3, BlockState)]) {
        self.chunks.set_blocks(edits);
        for &(at, _) in edits {
            self.fluids.changed(at);
            self.fires.changed(at);
        }
    }

    /// Whether the selected item is a bucket.
    pub(super) fn holding_bucket(&self) -> bool {
        self.inventory
            .selected_stack()
            .is_some_and(|stack| self.items.bucket(stack.id).is_some())
    }

    /// Right-click with a bucket (1.0.0's `ItemBucket.onItemRightClick`):
    /// an empty one scoops up the first source block in reach, a full one
    /// pours a source onto the face of the block looked at, into air or
    /// anything fluid washes away. Scooping fills the bucket; pouring
    /// empties it in survival, but creative keeps it full (as modern
    /// Minecraft does).
    pub(super) fn use_bucket(&mut self, eye: DVec3) {
        let Some(bucket) = self
            .inventory
            .selected_stack()
            .and_then(|stack| self.items.bucket(stack.id))
        else {
            return;
        };
        let world = self.chunks.world();
        let blocks = &self.items.blocks;
        let def_at = |at: IVec3| blocks.get(world.block(at.x, at.y, at.z)?.id());
        let (edit, after) = match bucket {
            Bucket::Empty => {
                let dir = self.camera.forward().as_dvec3();
                let source = |at: IVec3| {
                    let Some(block) = world.block(at.x, at.y, at.z) else {
                        return false;
                    };
                    blocks.get(block.id()).is_some_and(|def| {
                        def.is_targetable() || (def.fluid.is_some() && block.data() == 0)
                    })
                };
                let Some(hit) = raycast::raycast(eye, dir, config::REACH, source) else {
                    return;
                };
                let Some(fluid) = def_at(hit.block).and_then(|def| def.fluid) else {
                    return;
                };
                ((hit.block, BlockState::AIR), Bucket::Holds(fluid))
            }
            Bucket::Holds(fluid) => {
                let Some(at) = self.find_target(eye).and_then(|hit| hit.adjacent()) else {
                    return;
                };
                if def_at(at).is_none_or(|def| def.stops_fluid()) {
                    return;
                }
                let source = BlockState::new(fluid::flowing(fluid), 0);
                ((at, source), Bucket::Empty)
            }
        };
        self.edit(&[edit]);
        let keeps = after == Bucket::Empty && !self.mode.uses_up_items();
        if !keeps && let Some(id) = self.items.bucket_item(after) {
            let slot = self.inventory.selected;
            self.inventory
                .set(slot, Some(ItemStack::with_damage(id, 1, 0)));
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
        for &(at, _) in &outcome.edits {
            self.fires.changed(at);
        }
    }

    /// One tick of fire, and random ticks for the lava in chunks within
    /// `RANDOM_TICK_CHUNKS` of the player.
    pub(super) fn tick_fires(&mut self) {
        let feet = self.body.position().floor().as_ivec3();
        let center = ChunkPos::of_block(feet.x, feet.z);
        let reach = RANDOM_TICK_CHUNKS;
        let near: Vec<_> = (-reach..=reach)
            .flat_map(|dz| (-reach..=reach).map(move |dx| center.offset(dx, dz)))
            .collect();
        let rng = &mut self.rng;
        let mut random = |n: u32| rng.below(n);
        let edits = self.fires.tick(
            self.chunks.world(),
            &self.items.blocks,
            self.time,
            &near,
            &mut random,
        );
        if !edits.is_empty() {
            self.edit(&edits);
        }
    }

    /// Schedules the flowing fluid and fire in chunks that just loaded.
    pub(super) fn schedule_fluids(&mut self, chunks: &[ChunkPos]) {
        for &pos in chunks {
            if let Some(chunk) = self.chunks.world().get(pos) {
                self.fluids.chunk_loaded(chunk, self.time);
                self.fires.chunk_loaded(chunk, self.time);
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

    #[test]
    fn lava_and_fire_burn_up_items_and_fire_hurts_the_player() {
        let mut game = flat_game(GameMode::Survival);
        let lava = IVec3::new(3, 64, 3);
        let fire = IVec3::new(-3, 64, 3);
        game.edit(&[
            (lava, BlockState::new(11, 0)),
            (fire, BlockState::new(crate::world::fire::FIRE, 0)),
        ]);
        game.entities.clear();
        for x in [3.5, -2.5, 0.5] {
            game.entities.push(ItemEntity::new(
                ItemStack::new(4, 1),
                DVec3::new(x, 64.2, 3.5),
                DVec3::ZERO,
                0.0,
            ));
        }
        for _ in 0..6 {
            game.tick();
        }
        let left: Vec<_> = game.entities.iter().map(|item| item.center().x).collect();
        assert_eq!(left.len(), 1, "only the item on bare ground is left");
        assert!((left[0] - 0.5).abs() < 0.1);

        // Standing in the fire hurts and sets the player alight.
        let feet = DVec3::new(-2.5, 64.0, 3.5);
        game.body = Body::new(feet);
        game.prev_feet = feet;
        let health = game.stats.health;
        game.tick();
        assert_eq!(game.stats.health, health - 1);
        assert!(game.stats.fire >= 150);
    }

    /// Right-clicks once with `id` in the first hotbar slot, standing at
    /// the origin looking north and down at `pitch`: −0.51 looks through
    /// (0, 64, −2) to the ground beyond, −0.68 through it to the ground
    /// below it.
    fn click_with(game: &mut Game, id: u16, pitch: f32) {
        let feet = DVec3::new(0.5, 64.0, 0.5);
        game.body = Body::new(feet);
        game.prev_feet = feet;
        game.inventory.set(0, Some(ItemStack::new(id, 1)));
        game.inventory.selected = 0;
        game.camera.yaw = 0.0;
        game.camera.pitch = pitch;
        game.advance(0.0);
        game.place_clicked = true;
        game.tick();
    }

    #[test]
    fn buckets_scoop_up_and_pour_out_sources() {
        let mut game = flat_game(GameMode::Survival);
        let at = IVec3::new(0, 64, -2);
        game.edit(&[(at, BlockState::new(9, 0))]);
        click_with(&mut game, 325, -0.51);
        assert_eq!(game.chunks.world().block(0, 64, -2), Some(BlockState::AIR));
        assert_eq!(game.inventory.slot(0), Some(&ItemStack::new(326, 1)));

        // Poured onto the ground just beyond.
        click_with(&mut game, 326, -0.51);
        let poured = game.chunks.world().block(0, 64, -3).unwrap();
        assert_eq!((poured.id(), poured.data()), (8, 0));
        assert_eq!(game.inventory.slot(0), Some(&ItemStack::new(325, 1)));
        for _ in 0..60 {
            game.tick();
        }
        assert_eq!(water_id(&game, 0, -6), Some(9), "it spread");

        // Flowing water can't be scooped up; the bucket passes through it.
        assert_eq!(
            game.chunks.world().block(0, 64, -2),
            Some(BlockState::new(9, 1))
        );
        click_with(&mut game, 325, -0.68);
        assert_eq!(game.inventory.slot(0), Some(&ItemStack::new(325, 1)));

        // Lava in creative: pouring keeps the bucket full, and scooping
        // fills an empty one.
        let mut game = flat_game(GameMode::Creative);
        click_with(&mut game, 327, -0.51);
        let poured = game.chunks.world().block(0, 64, -3).unwrap();
        assert_eq!(poured.id(), 10);
        assert_eq!(game.inventory.slot(0), Some(&ItemStack::new(327, 1)));
        game.edit(&[(at, BlockState::new(11, 0))]);
        click_with(&mut game, 325, -0.51);
        assert_eq!(game.chunks.world().block(0, 64, -2), Some(BlockState::AIR));
        assert_eq!(game.inventory.slot(0), Some(&ItemStack::new(327, 1)));
    }
}
