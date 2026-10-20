//! Moving between dimensions (M24): the Overworld and the Nether, later
//! the End.
//!
//! Each dimension has its own chunk manager, region files, animals and
//! populated chunks. Going to another dimension saves the chunks of the one
//! left, empties its manager and parks it with that dimension's animals,
//! dropped items, carts and scheduled updates, so coming back picks them up
//! again. Only the dimension the player is in is loaded.

use glam::DVec3;
use log::error;

use super::Game;
use crate::{
    entity::{item::ItemEntity, minecart::Minecart, orb::XpOrb},
    level::SavedMob,
    world::{
        chunk::ChunkPos,
        dimension::Dimension,
        fast_hash::FastSet,
        fire::Fires,
        fluid::Fluids,
        manager::{ChunkManager, ChunkSource},
        redstone::Redstone,
        worldgen::{nether::Nether, terrain::Terrain},
    },
};

/// A dimension the player isn't in, as it was left.
#[derive(Default)]
pub(super) struct Away {
    /// Its chunk manager, emptied; `None` until the player has been there
    /// this session.
    pub chunks: Option<ChunkManager>,
    pub mobs: Vec<SavedMob>,
    pub populated: FastSet<ChunkPos>,
    pub entities: Vec<ItemEntity>,
    pub orbs: Vec<XpOrb>,
    pub carts: Vec<Minecart>,
    pub fluids: Fluids,
    pub fires: Fires,
    pub redstone: Redstone,
}

impl Game {
    /// The dimension the player is in.
    pub fn dimension(&self) -> Dimension {
        self.dimension
    }

    /// A test world that starts in `dimension` (its chunks must come from
    /// there).
    pub fn starting_in(mut self, dimension: Dimension) -> Self {
        self.dimension = dimension;
        self.fluids.warm = dimension == Dimension::Nether;
        self
    }

    /// Whether the player went to another dimension since the last call,
    /// so the renderer drops the chunks it drew.
    pub fn take_dimension_change(&mut self) -> bool {
        std::mem::take(&mut self.dimension_changed)
    }

    /// Where `dimension`'s new chunks come from in this world.
    pub(super) fn source_for(&self, dimension: Dimension) -> ChunkSource {
        let seed = self.chunks.seed().unwrap_or(0);
        source_for(dimension, seed)
    }

    /// Takes the player to `to` with their feet at `feet`. What they ride
    /// stays behind, as does any screen they had open. The caller saves
    /// once the player is where they'll stay.
    pub(super) fn change_dimension(&mut self, to: Dimension, feet: DVec3) {
        let from = self.dimension;
        if to == from {
            self.place_player(feet);
            return;
        }
        self.close_container();
        if self.is_sleeping() {
            self.wake();
        }
        if let Some(index) = self.mount_index() {
            self.dismount_mob(index, false);
        }
        self.riding = None;
        self.mining = None;
        self.eating = None;
        self.drawing = None;
        self.target = None;

        // Leave: keep the old dimension's chunks saved and its things parked.
        if let Err(err) = self.chunks.save() {
            error!("saving the chunks of {}: {err}", from.name());
        }
        let mut left = self.away.remove(&from).unwrap_or_default();
        left.mobs = self.saved_mobs();
        left.populated = std::mem::take(&mut self.populated);
        left.entities = std::mem::take(&mut self.entities);
        left.orbs = std::mem::take(&mut self.orbs);
        left.carts = std::mem::take(&mut self.carts);
        left.fluids = std::mem::take(&mut self.fluids);
        left.fires = std::mem::take(&mut self.fires);
        left.redstone = std::mem::take(&mut self.redstone);
        self.mobs.clear();
        self.stashed_mobs.clear();
        self.arrows.clear();
        self.fireballs.clear();
        self.particles.clear();
        self.effects.clear();
        self.falling.clear();
        self.falls_due.clear();
        self.primed.clear();
        self.post_processing.clear();

        // Arrive: the new dimension's manager, fresh or as it was left.
        let (radius, models, blend) = (
            self.chunks.radius(),
            self.chunks.models().clone(),
            self.chunks.blend(),
        );
        let mut arrived = self.away.remove(&to).unwrap_or_default();
        let mut chunks = match arrived.chunks.take() {
            Some(mut chunks) => {
                chunks.set_radius(radius);
                chunks.set_models(models);
                chunks
            }
            None => {
                let source = self.source_for(to);
                match &self.saved {
                    Some(saved) => ChunkManager::with_store(
                        source,
                        saved.dir.regions(to),
                        models,
                        radius,
                        super::chunk_workers(),
                    ),
                    None => ChunkManager::new(source, models, radius, super::chunk_workers()),
                }
            }
        };
        chunks.set_blend(blend);
        let mut old = std::mem::replace(&mut self.chunks, chunks);
        old.unload_all();
        left.chunks = Some(old);
        self.away.insert(from, left);

        self.dimension = to;
        self.populated = arrived.populated;
        self.entities = arrived.entities;
        self.orbs = arrived.orbs;
        self.carts = arrived.carts;
        self.fluids = arrived.fluids;
        self.fluids.warm = to == Dimension::Nether;
        self.fires = arrived.fires;
        self.redstone = arrived.redstone;
        self.restore_mobs(&arrived.mobs);
        self.place_player(feet);
        self.dimension_changed = true;
        self.arriving = Some(0);
    }

    /// Puts the player's feet at `feet`, standing still.
    pub(super) fn place_player(&mut self, feet: DVec3) {
        self.body.set_position(feet);
        self.body.velocity = DVec3::ZERO;
        self.prev_feet = feet;
        self.stats.fall_distance = 0.0;
    }

    /// The animals and populated chunks of the dimensions the player isn't
    /// in, for saving.
    pub(super) fn away_for_saving(&self) -> (Vec<SavedMob>, Vec<(Dimension, i32, i32)>) {
        let mut mobs = Vec::new();
        let mut populated = Vec::new();
        for (&dimension, away) in &self.away {
            mobs.extend(away.mobs.iter().cloned());
            populated.extend(away.populated.iter().map(|pos| (dimension, pos.x, pos.z)));
        }
        (mobs, populated)
    }

    /// Parks the saved animals and populated chunks of the dimensions the
    /// player isn't in, as `open` reads them.
    pub(super) fn restore_away(&mut self, mobs: &[SavedMob], populated: &[(Dimension, i32, i32)]) {
        for mob in mobs.iter().filter(|mob| mob.dimension != self.dimension) {
            self.away
                .entry(mob.dimension)
                .or_default()
                .mobs
                .push(mob.clone());
        }
        for &(dimension, x, z) in populated {
            if dimension != self.dimension {
                self.away
                    .entry(dimension)
                    .or_default()
                    .populated
                    .insert(ChunkPos::new(x, z));
            }
        }
    }

    /// While the chunks around a player who just arrived load, the game
    /// waits; `Some(ticks)` counts how long it has.
    pub fn is_arriving(&self) -> bool {
        self.arriving.is_some()
    }

    /// Ends the wait once the chunks around the player are drawn, or after
    /// ten seconds whatever happens.
    pub(super) fn tick_arrival(&mut self) {
        let Some(ticks) = self.arriving else {
            return;
        };
        let feet = self.body.position();
        let center = ChunkPos::of_block(feet.x.floor() as i32, feet.z.floor() as i32);
        if self.chunks.ready_around(center, 1) || ticks >= ARRIVAL_TIMEOUT {
            self.arriving = None;
        } else {
            self.arriving = Some(ticks + 1);
        }
    }
}

/// Longest the game waits for the chunks around an arriving player.
const ARRIVAL_TIMEOUT: u32 = 10 * crate::config::TICKS_PER_SECOND;

/// Where `dimension`'s new chunks come from for a world with `seed`.
pub fn source_for(dimension: Dimension, seed: u64) -> ChunkSource {
    match dimension {
        Dimension::Nether => ChunkSource::Nether(Box::new(Nether::new(seed))),
        // The End comes in M27; until then it's the Overworld's land.
        Dimension::Overworld | Dimension::End => ChunkSource::Terrain(Box::new(Terrain::new(seed))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        game::tests::flat_game,
        inventory::{items, stack::ItemStack},
        player::mode::GameMode,
        world::blocks,
    };

    #[test]
    fn water_poured_in_the_nether_boils_away_but_lava_doesnt() {
        for (dimension, fluid_bucket, stays) in [
            (Dimension::Nether, items::WATER_BUCKET, None),
            (
                Dimension::Overworld,
                items::WATER_BUCKET,
                Some(blocks::WATER),
            ),
            (Dimension::Nether, items::LAVA_BUCKET, Some(blocks::LAVA)),
        ] {
            let mut game = flat_game(GameMode::Survival);
            game.dimension = dimension;
            game.inventory.set(0, Some(ItemStack::new(fluid_bucket, 1)));
            game.camera.pitch = -0.6;
            game.advance(0.0);
            let at = game.target.unwrap().adjacent().unwrap();
            let eye = game.body.position() + DVec3::Y * game.body.eye_height();
            game.use_bucket(eye);
            let block = game.chunks.world().block(at.x, at.y, at.z).unwrap();
            match stays {
                Some(id) => assert!(block.is(id), "{dimension:?}"),
                None => assert!(block.is_air(), "boiled away"),
            }
            assert_eq!(game.inventory.slot(0).unwrap().id, items::BUCKET, "emptied");
        }
    }

    #[test]
    fn animals_stay_in_their_dimension() {
        let mut game = flat_game(GameMode::Creative);
        game.mob_defs = crate::game::tests::mob_registry();
        let pig = game.mob_defs.find("pig").unwrap();
        let feet = DVec3::new(3.5, 64.0, 3.5);
        let mob = game.new_mob(pig, feet, 0.0);
        game.add_mob(mob);
        assert_eq!(game.saved_mobs().len(), 1);
        game.change_dimension(Dimension::Nether, DVec3::new(0.5, 80.0, 0.5));
        assert!(game.mobs.is_empty() && game.saved_mobs().is_empty());
        let (away, _) = game.away_for_saving();
        assert_eq!(away.len(), 1);
        assert_eq!(away[0].dimension, Dimension::Overworld);
        game.change_dimension(Dimension::Overworld, DVec3::new(0.5, 64.0, 0.5));
        // Back in the stash, waiting for its chunk.
        assert_eq!(game.saved_mobs().len(), 1);
        assert!(game.away_for_saving().0.is_empty());
    }
}
