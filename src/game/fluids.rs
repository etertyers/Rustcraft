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
    /// Sets blocks the player changed, waking fluids and fires next to
    /// them. Their chunks are remeshed at once.
    pub(super) fn edit(&mut self, edits: &[(IVec3, BlockState)]) {
        self.edit_sounds(edits);
        self.chunks.set_blocks(edits);
        self.wake_around(edits);
    }

    /// Sets blocks the world changed on its own, remeshing their chunks in
    /// the background.
    pub(super) fn edit_later(&mut self, edits: &[(IVec3, BlockState)]) {
        self.edit_sounds(edits);
        self.chunks.set_blocks_later(edits);
        self.wake_around(edits);
    }

    fn wake_around(&mut self, edits: &[(IVec3, BlockState)]) {
        self.equip_placed(edits);
        for &(at, _) in edits {
            self.fluids.changed(at);
            self.fires.changed(at);
            self.redstone.changed(at);
        }
        let cells: Vec<IVec3> = edits.iter().map(|&(at, _)| at).collect();
        self.update_neighbors(&cells);
    }

    /// Whether the selected item is a bucket.
    pub(super) fn holding_bucket(&self) -> bool {
        self.player
            .inventory
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
            .player
            .inventory
            .selected_stack()
            .and_then(|stack| self.items.bucket(stack.id))
        else {
            return;
        };
        let world = self.chunks.world();
        let blocks = self.items.blocks;
        let def_at = |at: IVec3| blocks.get(world.block(at.x, at.y, at.z)?.id());
        let (edit, after) = match bucket {
            Bucket::Empty => {
                let dir = self.player.camera.forward().as_dvec3();
                let source = |at: IVec3| {
                    let Some(block) = world.block(at.x, at.y, at.z) else {
                        return false;
                    };
                    blocks.get(block.id()).is_some_and(|def| {
                        def.is_targetable() || (def.fluid.is_some() && fluid::level(block) == 0)
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
                // Water poured in the Nether boils away.
                let source = if self.evaporates(fluid) {
                    world.block(at.x, at.y, at.z).unwrap_or(BlockState::AIR)
                } else {
                    fluid::at_level(fluid, 0)
                };
                ((at, source), Bucket::Empty)
            }
        };
        let sound = match (bucket, after) {
            (Bucket::Empty, Bucket::Holds(crate::world::block::Fluid::Lava)) => {
                "item.bucket.fill_lava"
            }
            (Bucket::Empty, _) => "item.bucket.fill",
            (_, _) if bucket == Bucket::Holds(crate::world::block::Fluid::Lava) => {
                "item.bucket.empty_lava"
            }
            _ => "item.bucket.empty",
        };
        crate::audio::play(sound, edit.0.as_dvec3() + DVec3::splat(0.5));
        self.edit(&[edit]);
        let keeps = after == Bucket::Empty && !self.player.mode.uses_up_items();
        if !keeps && let Some(id) = self.items.bucket_item(after) {
            // One bucket of a stack fills; the full one goes beside the
            // rest (modern `ItemUtils.createFilledResult`).
            let slot = self.player.inventory.selected;
            let got = ItemStack::with_damage(id, 1, 0);
            let many = self
                .player
                .inventory
                .slot(slot)
                .is_some_and(|s| s.count > 1);
            if many && self.player.mode.uses_up_items() {
                self.player.inventory.take(slot, 1);
                if let Some(rest) = self.player.inventory.add(got, &self.items) {
                    self.throw(rest);
                }
            } else {
                self.player.inventory.set(slot, Some(got));
            }
        }
    }

    /// Whether `fluid` boils away when poured here: water in the Nether
    /// (modern `DimensionType.ultraWarm`).
    pub(super) fn evaporates(&self, fluid: crate::world::block::Fluid) -> bool {
        fluid == crate::world::block::Fluid::Water
            && self.dimension() == crate::world::dimension::Dimension::Nether
    }

    /// One tick of flowing water and lava.
    pub(super) fn tick_fluids(&mut self) {
        let rng = &mut self.rng;
        let mut random = |n: u32| rng.below(n);
        let outcome = self.fluids.tick(
            self.chunks.world(),
            self.items.blocks,
            self.time,
            &mut random,
        );
        if outcome.edits.is_empty() {
            return;
        }
        // Washed-away blocks drop whatever breaking them by hand would.
        for (at, block) in outcome.washed {
            let rng = &mut self.rng;
            let mut random = || rng.next_f64() as f32;
            for stack in harvest::drops(block, None, &mut random) {
                self.entities
                    .push(ItemEntity::from_block(stack, at, &mut random));
            }
        }
        // Lava meeting water hisses as it turns to stone.
        for &(at, new) in &outcome.edits {
            let was_lava = self
                .chunks
                .world()
                .block(at.x, at.y, at.z)
                .is_some_and(|old| old.def().fluid == Some(crate::world::block::Fluid::Lava));
            if was_lava && new.def().fluid.is_none() {
                crate::audio::play_at(
                    "block.lava.extinguish",
                    at.as_dvec3() + DVec3::splat(0.5),
                    0.5,
                    2.6 + self.rng.next_f64() as f32 * 0.8,
                );
            }
        }
        self.chunks.set_blocks_later(&outcome.edits);
        for &(at, _) in &outcome.edits {
            self.fires.changed(at);
            self.redstone.changed(at);
        }
        // Fluid flowing past wakes sand and reshapes what it touches.
        let cells: Vec<IVec3> = outcome.edits.iter().map(|&(at, _)| at).collect();
        self.update_neighbors(&cells);
    }

    /// One tick of fire, and random ticks for the lava in chunks within
    /// `RANDOM_TICK_CHUNKS` of the player.
    pub(super) fn tick_fires(&mut self) {
        // `doFireTick` off: fire neither spreads nor burns out, and lava
        // lights nothing.
        if !self.rules.do_fire_tick {
            return;
        }
        let reach = RANDOM_TICK_CHUNKS;
        let mut near: Vec<ChunkPos> = self
            .player_chunks()
            .into_iter()
            .flat_map(|center| {
                (-reach..=reach)
                    .flat_map(move |dz| (-reach..=reach).map(move |dx| center.offset(dx, dz)))
            })
            .collect();
        near.sort_by_key(|pos| (pos.x, pos.z));
        near.dedup();
        let random_ticks = self.rules.random_tick_speed;
        let rng = &mut self.rng;
        let mut random = |n: u32| rng.below(n);
        let edits = self.fires.tick(
            self.chunks.world(),
            self.items.blocks,
            self.time,
            &near,
            random_ticks,
            &mut random,
        );
        if !edits.is_empty() {
            // TNT that burns is lit instead.
            let burnt: Vec<IVec3> = edits
                .iter()
                .filter(|&&(at, new)| {
                    !new.is(crate::world::blocks::TNT)
                        && self
                            .chunks
                            .world()
                            .block(at.x, at.y, at.z)
                            .is_some_and(|old| old.is(crate::world::blocks::TNT))
                })
                .map(|&(at, _)| at)
                .collect();
            self.edit_later(&edits);
            for at in burnt {
                self.prime_tnt(at);
            }
        }
    }

    /// Flint and steel on the face of the block looked at: lights a fire in
    /// the air beside it if fire can stay there, wearing the flint by one.
    pub(super) fn strike_flint(&mut self, hit: &crate::player::raycast::Hit) {
        let clicked = self
            .chunks
            .world()
            .block(hit.block.x, hit.block.y, hit.block.z);
        if clicked.is_some_and(|block| block.is(crate::world::blocks::TNT)) {
            // Lighting TNT primes it instead of making fire.
            self.edit(&[(hit.block, BlockState::AIR)]);
            self.prime_tnt(hit.block);
            self.finish_flint();
            return;
        }
        let Some(at) = hit.adjacent() else {
            return;
        };
        // Fire in an obsidian frame lights it as a portal.
        if self.light_portal(at) {
            self.finish_flint();
            return;
        }
        let rng = &mut self.rng;
        let mut random = |n: u32| rng.below(n);
        let Some(edit) = self.fires.ignite(
            self.chunks.world(),
            self.items.blocks,
            at,
            self.time,
            &mut random,
        ) else {
            return;
        };
        self.edit(&[edit]);
        self.finish_flint();
    }

    /// Swings and wears the flint after it lit something; a fire charge
    /// is used up instead.
    fn finish_flint(&mut self) {
        self.player.view.swing();
        let charge = self
            .held()
            .is_some_and(|s| s.id == crate::inventory::items::FIRE_CHARGE);
        crate::audio::play_at(
            if charge {
                "item.firecharge.use"
            } else {
                "item.flintandsteel.use"
            },
            self.player.body.position(),
            1.0,
            0.8 + self.rng.next_f64() as f32 * 0.4,
        );
        let slot = self.player.inventory.hand_slot(self.player.use_hand);
        let charge = self
            .player
            .inventory
            .slot(slot)
            .is_some_and(|stack| stack.id == crate::inventory::items::FIRE_CHARGE);
        if charge {
            if self.player.mode.uses_up_items() {
                self.player.inventory.take(slot, 1);
            }
        } else if self.player.mode.wears_tools() {
            self.player.inventory.damage_slot(slot, 1, &self.items);
        }
        self.player.place_cooldown = crate::player::interaction::PLACE_REPEAT_TICKS;
    }

    /// Schedules the flowing fluid and fire in chunks that just loaded.
    pub(super) fn schedule_fluids(&mut self, chunks: &[ChunkPos]) {
        for &pos in chunks {
            if let Some(chunk) = self.chunks.world().get(pos) {
                self.fluids.chunk_loaded(chunk, self.time);
                self.fires.chunk_loaded(chunk, self.time);
                self.redstone.chunk_loaded(chunk, self.time);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use glam::{DVec3, IVec3};

    use super::*;
    use crate::{
        game::tests::flat_game,
        inventory::{item::ItemId, items, stack::ItemStack},
        player::mode::GameMode,
        player::physics::Body,
        world::blocks,
    };

    const GLASS: BlockState = BlockState::of(blocks::GLASS);
    const SOURCE: BlockState = BlockState::of(blocks::WATER);

    /// The level of the water at (x, 64, z), if there's water there.
    fn water_level(game: &Game, x: i32, z: i32) -> Option<u8> {
        let block = game.chunks.world().block(x, 64, z)?;
        block.is(blocks::WATER).then(|| fluid::level(block))
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
        assert_eq!(water_level(&game, 4, 4), Some(0), "a source in its box");
        assert_eq!(water_level(&game, 6, 4), None);
        game.break_block(source + IVec3::X);
        for _ in 0..40 {
            game.tick();
        }
        assert!(water_level(&game, 6, 4).is_some(), "out through the gap");
    }

    #[test]
    fn the_current_carries_the_player_and_items_downstream() {
        let mut game = flat_game(GameMode::Survival);
        game.edit(&[(IVec3::new(5, 64, 0), SOURCE)]);
        for _ in 0..100 {
            game.tick();
        }
        assert!(water_level(&game, 0, 0).is_some());
        let start = DVec3::new(0.5, 64.0, 0.5);
        game.player.body = Body::new(start);
        game.player.prev_feet = start;
        game.entities.clear();
        game.entities.push(ItemEntity::new(
            ItemStack::new(items::COBBLESTONE, 1),
            DVec3::new(0.5, 64.2, 2.5),
            DVec3::ZERO,
            0.0,
        ));
        for _ in 0..20 {
            game.tick();
        }
        // Away from the source, which is east.
        assert!(
            game.player.body.position().x < start.x - 0.3,
            "{}",
            game.player.body.position()
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
            (lava, BlockState::of(blocks::LAVA)),
            (fire, BlockState::of(blocks::FIRE)),
        ]);
        game.entities.clear();
        for x in [3.5, -2.5, 0.5] {
            game.entities.push(ItemEntity::new(
                ItemStack::new(items::COBBLESTONE, 1),
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
        game.player.body = Body::new(feet);
        game.player.prev_feet = feet;
        let health = game.player.stats.health;
        game.tick();
        assert_eq!(game.player.stats.health, health - 1.0);
        assert!(game.player.stats.fire >= 150);
    }

    /// Right-clicks once with `id` in the first hotbar slot, standing at
    /// the origin looking north and down at `pitch`: −0.51 looks through
    /// (0, 64, −2) to the ground beyond, −0.68 through it to the ground
    /// below it.
    fn click_with(game: &mut Game, id: ItemId, pitch: f32) {
        let feet = DVec3::new(0.5, 64.0, 0.5);
        game.player.body = Body::new(feet);
        game.player.prev_feet = feet;
        game.player.inventory.set(0, Some(ItemStack::new(id, 1)));
        game.player.inventory.selected = 0;
        game.player.camera.yaw = 0.0;
        game.player.camera.pitch = pitch;
        game.advance(0.0);
        game.player.place_clicked = true;
        game.tick();
    }

    #[test]
    fn one_bucket_of_a_stack_fills_and_the_rest_stay() {
        let mut game = flat_game(GameMode::Survival);
        game.edit(&[(IVec3::new(0, 64, -2), BlockState::of(blocks::WATER))]);
        game.player
            .inventory
            .set(0, Some(ItemStack::new(items::BUCKET, 16)));
        game.player.inventory.selected = 0;
        game.player.camera.yaw = 0.0;
        game.player.camera.pitch = -0.51;
        game.advance(0.0);
        game.player.place_clicked = true;
        game.tick();
        assert_eq!(
            game.player.inventory.slot(0),
            Some(&ItemStack::new(items::BUCKET, 15))
        );
        assert!(game.player.inventory.find(items::WATER_BUCKET).is_some());
    }

    #[test]
    fn buckets_scoop_up_and_pour_out_sources() {
        let mut game = flat_game(GameMode::Survival);
        let at = IVec3::new(0, 64, -2);
        game.edit(&[(at, BlockState::of(blocks::WATER))]);
        click_with(&mut game, items::BUCKET, -0.51);
        assert_eq!(game.chunks.world().block(0, 64, -2), Some(BlockState::AIR));
        assert_eq!(
            game.player.inventory.slot(0),
            Some(&ItemStack::new(items::WATER_BUCKET, 1))
        );

        // Poured onto the ground just beyond.
        click_with(&mut game, items::WATER_BUCKET, -0.51);
        let poured = game.chunks.world().block(0, 64, -3).unwrap();
        assert_eq!(poured, SOURCE);
        assert_eq!(
            game.player.inventory.slot(0),
            Some(&ItemStack::new(items::BUCKET, 1))
        );
        for _ in 0..60 {
            game.tick();
        }
        assert!(water_level(&game, 0, -6).is_some(), "it spread");

        // Flowing water can't be scooped up; the bucket passes through it.
        assert_eq!(water_level(&game, 0, -2), Some(1));
        click_with(&mut game, items::BUCKET, -0.68);
        assert_eq!(
            game.player.inventory.slot(0),
            Some(&ItemStack::new(items::BUCKET, 1))
        );

        // Lava in creative: pouring keeps the bucket full, and scooping
        // fills an empty one.
        let mut game = flat_game(GameMode::Creative);
        click_with(&mut game, items::LAVA_BUCKET, -0.51);
        let poured = game.chunks.world().block(0, 64, -3).unwrap();
        assert_eq!(poured.id(), blocks::LAVA);
        assert_eq!(
            game.player.inventory.slot(0),
            Some(&ItemStack::new(items::LAVA_BUCKET, 1))
        );
        game.edit(&[(at, BlockState::of(blocks::LAVA))]);
        click_with(&mut game, items::BUCKET, -0.51);
        assert_eq!(game.chunks.world().block(0, 64, -2), Some(BlockState::AIR));
        assert_eq!(
            game.player.inventory.slot(0),
            Some(&ItemStack::new(items::LAVA_BUCKET, 1))
        );
    }
}
