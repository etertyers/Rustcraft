//! Moving between dimensions (M24): the Overworld, the Nether and the End.
//!
//! Each dimension has its own chunk manager, region files, animals and
//! populated chunks. Every player is in a dimension of their own choosing
//! (M37): each dimension with a player in it is live, its world held in a
//! `Realm` while another's is in the game's fields, and ticked in turn with
//! the players in it. A dimension nobody is in is parked: its chunks saved,
//! its manager emptied and kept with its animals, dropped items, carts and
//! scheduled updates, so coming back picks them up again.

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
        worldgen::{end::End, nether::Nether, terrain::Terrain},
    },
};

/// A live dimension's world while another's is in the game's fields.
macro_rules! realm {
    ($($field:ident: $ty:ty,)*) => {
        pub(super) struct Realm {
            $($field: $ty,)*
        }

        impl Game {
            /// Trades the game's dimension fields for `realm`'s.
            fn swap_realm(&mut self, realm: &mut Realm) {
                $(std::mem::swap(&mut self.$field, &mut realm.$field);)*
            }
        }
    };
}

realm! {
    chunks: ChunkManager,
    mobs: Vec<crate::entity::mob::Mob>,
    stashed_mobs: crate::world::fast_hash::FastMap<ChunkPos, Vec<crate::entity::mob::Mob>>,
    populated: FastSet<ChunkPos>,
    entities: Vec<ItemEntity>,
    orbs: Vec<XpOrb>,
    carts: Vec<Minecart>,
    fluids: Fluids,
    fires: Fires,
    redstone: Redstone,
    arrows: Vec<crate::entity::arrow::Arrow>,
    fireballs: Vec<crate::entity::fireball::Fireball>,
    thrown: Vec<crate::entity::thrown::ThrownPotion>,
    eyes: Vec<crate::entity::eye::EyeOfEnder>,
    bullets: Vec<crate::entity::bullet::ShulkerBullet>,
    dragon_fireballs: Vec<crate::entity::breath::DragonFireball>,
    breath: Vec<crate::entity::breath::BreathCloud>,
    particles: Vec<crate::entity::particle::Particle>,
    effects: Vec<crate::entity::effects::Effect>,
    falling: Vec<crate::entity::falling::FallingBlock>,
    falls_due: Vec<(u64, glam::IVec3)>,
    primed: Vec<crate::entity::tnt::PrimedTnt>,
    post_processing: Vec<ChunkPos>,
}

/// Where a player lands in the dimension they go to.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum Arrival {
    At(DVec3),
    /// By the portal nearest `target` (made if there's none), facing along
    /// its axis.
    Portal(glam::IVec3, u8),
    /// On the End's arrival platform, rebuilt.
    EndPlatform,
    /// Beside their bed or at the world spawn.
    Home,
}

/// A dimension nobody is in, as it was left.
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
        self.player.dimension = dimension;
        self.fluids.warm = dimension == Dimension::Nether;
        if dimension == Dimension::End {
            self.start_fight();
        }
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
    /// Sends `self.player` to `to`, arriving as `arrival` says. In the
    /// middle of a tick they go when it ends, so the dimension they leave
    /// finishes its tick first.
    pub(super) fn change_dimension(&mut self, to: Dimension, arrival: Arrival) {
        if to == self.player.dimension {
            self.arrive(arrival);
            return;
        }
        self.close_container();
        if self.is_sleeping() {
            self.wake();
        }
        if let Some(index) = self.mount_index() {
            self.dismount_mob(index, false);
        }
        self.player.riding = None;
        self.player.mining = None;
        self.player.eating = None;
        self.player.drawing = None;
        self.player.target = None;
        self.player.travel = Some((to, arrival));
        if !self.ticking && self.swapped == 0 {
            self.finish_travels();
        }
    }

    /// Moves the players who went through a portal this tick, then parks
    /// the dimensions left empty.
    pub(super) fn finish_travels(&mut self) {
        let mut went = false;
        for index in 0..self.player_count() {
            let Some((to, arrival)) = self.player_at(index).travel else {
                continue;
            };
            went = true;
            self.enter_realm(to);
            self.with_player(index, |game| {
                game.player.travel = None;
                game.player.dimension = to;
                game.arrive(arrival);
                game.player.arriving = Some(0);
                game.note_dimension(to);
            });
            if index == 0 {
                self.dimension_changed = true;
            }
        }
        self.enter_realm(self.player.dimension);
        let empty: Vec<Dimension> = self
            .live
            .keys()
            .copied()
            .filter(|&d| (0..self.player_count()).all(|i| self.player_at(i).dimension != d))
            .collect();
        for dimension in empty {
            self.park(dimension);
        }
        if went && let Err(err) = self.save() {
            error!("saving after going to another dimension: {err}");
        }
    }

    /// Puts `self.player` where `arrival` says in the dimension they're
    /// now in.
    fn arrive(&mut self, arrival: Arrival) {
        match arrival {
            Arrival::At(feet) => self.place_player(feet),
            Arrival::Portal(target, axis) => {
                let exit = self.portal_near(target, axis);
                self.place_player(exit);
            }
            Arrival::EndPlatform => self.arrive_in_the_end(),
            Arrival::Home => {
                let feet = self.respawn_point();
                self.place_player(feet);
            }
        }
    }

    /// Puts `dimension`'s world in the game's fields (waking it if it was
    /// parked), keeping the one there live.
    pub fn enter_realm(&mut self, dimension: Dimension) {
        if dimension == self.dimension {
            return;
        }
        let (mut realm, mobs) = match self.live.remove(&dimension) {
            Some(realm) => (realm, Vec::new()),
            None => self.wake_realm(dimension),
        };
        self.swap_realm(&mut realm);
        self.live.insert(self.dimension, realm);
        self.dimension = dimension;
        self.restore_mobs(&mobs);
    }

    /// A parked (or new) dimension's world, and its saved animals.
    fn wake_realm(&mut self, dimension: Dimension) -> (Realm, Vec<crate::level::SavedMob>) {
        let (radius, models, blend) = (
            self.chunks.radius(),
            self.chunks.models().clone(),
            self.chunks.blend(),
        );
        let mut away = self.away.remove(&dimension).unwrap_or_default();
        let mut chunks = match away.chunks.take() {
            Some(mut chunks) => {
                chunks.set_radius(radius);
                chunks.set_models(models);
                chunks
            }
            None => {
                let source = self.source_for(dimension);
                match &self.saved {
                    Some(saved) => ChunkManager::with_store(
                        source,
                        saved.dir.regions(dimension),
                        models,
                        radius,
                        super::chunk_workers(),
                    ),
                    None => ChunkManager::new(source, models, radius, super::chunk_workers()),
                }
            }
        };
        chunks.set_blend(blend);
        // A server's chunks stay a server's in every dimension.
        if self.chunks.serves() && !chunks.serves() {
            chunks.serve();
        }
        let mut fluids = away.fluids;
        fluids.warm = dimension == Dimension::Nether;
        let realm = Realm {
            chunks,
            mobs: Vec::new(),
            stashed_mobs: Default::default(),
            populated: away.populated,
            entities: away.entities,
            orbs: away.orbs,
            carts: away.carts,
            fluids,
            fires: away.fires,
            redstone: away.redstone,
            arrows: Vec::new(),
            fireballs: Vec::new(),
            thrown: Vec::new(),
            eyes: Vec::new(),
            bullets: Vec::new(),
            dragon_fireballs: Vec::new(),
            breath: Vec::new(),
            particles: Vec::new(),
            effects: Vec::new(),
            falling: Vec::new(),
            falls_due: Vec::new(),
            primed: Vec::new(),
            post_processing: Vec::new(),
        };
        (realm, away.mobs)
    }

    /// Parks live `dimension` (not the one in the game's fields): its
    /// chunks saved and unloaded, its animals and things kept for later.
    fn park(&mut self, dimension: Dimension) {
        let back = self.dimension;
        if dimension == back {
            return;
        }
        self.enter_realm(dimension);
        if let Err(err) = self.chunks.save() {
            error!("saving the chunks of {}: {err}", dimension.name());
        }
        let mut left = self.away.remove(&dimension).unwrap_or_default();
        left.mobs = self.saved_mobs();
        left.populated = std::mem::take(&mut self.populated);
        left.entities = std::mem::take(&mut self.entities);
        left.orbs = std::mem::take(&mut self.orbs);
        left.carts = std::mem::take(&mut self.carts);
        left.fluids = std::mem::take(&mut self.fluids);
        left.fires = std::mem::take(&mut self.fires);
        left.redstone = std::mem::take(&mut self.redstone);
        self.enter_realm(back);
        if let Some(mut realm) = self.live.remove(&dimension) {
            realm.chunks.unload_all();
            left.chunks = Some(realm.chunks);
        }
        self.away.insert(dimension, left);
    }

    /// Players in the dimension whose world is in the game's fields.
    pub fn players_here(&self) -> Vec<usize> {
        (0..self.player_count())
            .filter(|&i| self.player_at(i).dimension == self.dimension)
            .collect()
    }

    /// The dimensions loaded now: the one in the game's fields and the
    /// other live ones.
    pub fn live_dimensions(&self) -> Vec<Dimension> {
        std::iter::once(self.dimension)
            .chain(self.live.keys().copied())
            .collect()
    }

    /// The dimensions with players in them, the host's first.
    pub fn dimensions_with_players(&self) -> Vec<Dimension> {
        let mut dimensions = Vec::new();
        for i in 0..self.player_count() {
            let d = self.player_at(i).dimension;
            if !dimensions.contains(&d) {
                dimensions.push(d);
            }
        }
        dimensions
    }

    /// Puts the player's feet at `feet`, standing still.
    pub(super) fn place_player(&mut self, feet: DVec3) {
        self.player.body.set_position(feet);
        self.player.body.velocity = DVec3::ZERO;
        self.player.prev_feet = feet;
        self.player.stats.fall_distance = 0.0;
    }

    /// The animals and populated chunks of the dimensions not in the
    /// game's fields, for saving: the parked ones as they were left, the
    /// live ones as they are (their chunks saved too).
    pub(super) fn away_for_saving(&mut self) -> (Vec<SavedMob>, Vec<(Dimension, i32, i32)>) {
        let mut mobs = Vec::new();
        let mut populated = Vec::new();
        let back = self.dimension;
        let live: Vec<Dimension> = self.live.keys().copied().collect();
        for dimension in live {
            self.enter_realm(dimension);
            if let Err(err) = self.chunks.save() {
                error!("saving the chunks of {}: {err}", dimension.name());
            }
            mobs.extend(self.saved_mobs());
            populated.extend(self.populated.iter().map(|pos| (dimension, pos.x, pos.z)));
        }
        self.enter_realm(back);
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
        self.player.arriving.is_some()
    }

    /// Ends the wait once the chunks around the player are drawn, or after
    /// ten seconds whatever happens.
    pub(super) fn tick_arrival(&mut self) {
        let Some(ticks) = self.player.arriving else {
            return;
        };
        let feet = self.player.body.position();
        let center = ChunkPos::of_block(feet.x.floor() as i32, feet.z.floor() as i32);
        if self.chunks.ready_around(center, 1) || ticks >= ARRIVAL_TIMEOUT {
            self.player.arriving = None;
        } else {
            self.player.arriving = Some(ticks + 1);
        }
    }
}

/// Longest the game waits for the chunks around an arriving player.
const ARRIVAL_TIMEOUT: u32 = 10 * crate::config::TICKS_PER_SECOND;

/// Where `dimension`'s new chunks come from for a world with `seed`.
pub fn source_for(dimension: Dimension, seed: u64) -> ChunkSource {
    match dimension {
        Dimension::Nether => ChunkSource::Nether(Box::new(Nether::new(seed))),
        Dimension::End => ChunkSource::End(Box::new(End::new(seed))),
        Dimension::Overworld => ChunkSource::Terrain(Box::new(Terrain::new(seed))),
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
    fn players_can_be_in_different_dimensions_at_once() {
        let mut game = flat_game(GameMode::Survival);
        game.others.push(crate::game::PlayerState::new(
            DVec3::new(4.5, 64.0, 0.5),
            0.0,
            0.0,
        ));
        let nether_feet = DVec3::new(0.5, 70.0, 0.5);
        game.with_player(1, |guest| {
            guest.change_dimension(Dimension::Nether, Arrival::At(nether_feet));
        });
        // Another player's travel waits for the end of the tick.
        assert_eq!(game.others[0].dimension, Dimension::Overworld);
        game.run_ticks(1);
        assert_eq!(game.others[0].dimension, Dimension::Nether);
        assert_eq!(game.player.dimension, Dimension::Overworld);
        assert_eq!(
            game.dimension(),
            Dimension::Overworld,
            "the host's world is in front"
        );
        assert_eq!(
            game.dimensions_with_players(),
            [Dimension::Overworld, Dimension::Nether]
        );
        // The Nether loads around the guest while the host's world stays.
        game.enter_realm(Dimension::Nether);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while game.chunks.world().get(ChunkPos::new(0, 0)).is_none() {
            assert!(
                std::time::Instant::now() < deadline,
                "the Nether never loaded"
            );
            game.chunks.update(ChunkPos::new(0, 0));
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        game.enter_realm(Dimension::Overworld);
        assert!(
            game.chunks.world().get(ChunkPos::new(0, 0)).is_some(),
            "the Overworld's"
        );
        let time = game.time();
        game.run_ticks(40);
        assert_eq!(game.time(), time + 40, "one clock for both");
        assert_eq!(
            game.player.body.position().y,
            64.0,
            "the host stands on the grass"
        );
        // Back home, the Nether is parked again.
        game.with_player(1, |guest| {
            guest.change_dimension(
                Dimension::Overworld,
                Arrival::At(DVec3::new(4.5, 64.0, 0.5)),
            );
        });
        game.run_ticks(1);
        assert_eq!(game.live_dimensions(), [Dimension::Overworld]);
        assert!(game.away.contains_key(&Dimension::Nether));
    }

    #[test]
    fn a_servers_chunks_serve_in_the_dimension_it_goes_to() {
        let mut game = crate::game::tests::flat_game(GameMode::Survival);
        game.chunks.serve();
        game.change_dimension(Dimension::Nether, Arrival::At(DVec3::new(0.5, 70.0, 0.5)));
        assert!(game.chunks.serves());
        game.change_dimension(
            Dimension::Overworld,
            Arrival::At(DVec3::new(0.5, 70.0, 0.5)),
        );
        assert!(game.chunks.serves(), "and in the one it comes back to");
    }

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
            game.player
                .inventory
                .set(0, Some(ItemStack::new(fluid_bucket, 1)));
            game.player.camera.pitch = -0.6;
            game.advance(0.0);
            let at = game.player.target.unwrap().adjacent().unwrap();
            let eye = game.player.body.position() + DVec3::Y * game.player.body.eye_height();
            game.use_bucket(eye);
            let block = game.chunks.world().block(at.x, at.y, at.z).unwrap();
            match stays {
                Some(id) => assert!(block.is(id), "{dimension:?}"),
                None => assert!(block.is_air(), "boiled away"),
            }
            assert_eq!(
                game.player.inventory.slot(0).unwrap().id,
                items::BUCKET,
                "emptied"
            );
        }
    }

    #[test]
    fn a_world_left_in_the_nether_reopens_there_with_both_sides_kept() {
        use crate::{
            game::tests::{load_player_chunk, registries},
            level::WorldDir,
            world::{block::BlockState, save::TempDir},
        };
        use glam::IVec3;
        let saves = TempDir::new("nether");
        let (items, models) = registries();
        let mut game = Game::create(
            saves.path(),
            "Nether Test",
            42,
            GameMode::Creative,
            items.clone(),
            models.clone(),
        )
        .unwrap()
        .with_mobs(crate::game::tests::mob_registry());
        load_player_chunk(&mut game);
        let home = game.player.body.position();
        let mark = home.floor().as_ivec3() + IVec3::new(0, 3, 0);
        game.chunks
            .set_blocks(&[(mark, BlockState::of(blocks::GOLD_BLOCK))]);
        let pig = game
            .summon("pig", home + DVec3::new(2.0, 0.0, 0.0), 0.0)
            .unwrap()
            .kind;
        game.portals.push((Dimension::Overworld, [1, 2, 3]));

        let there = DVec3::new(10.5, 70.0, 10.5);
        game.change_dimension(Dimension::Nether, Arrival::At(there));
        load_player_chunk(&mut game);
        let ember = IVec3::new(10, 75, 10);
        game.chunks
            .set_blocks(&[(ember, BlockState::of(blocks::GLOWSTONE))]);
        game.save().unwrap();
        drop(game);

        let dir = WorldDir::new(saves.path().join("Nether Test"));
        let mut game = Game::open(dir, items, models)
            .unwrap()
            .with_mobs(crate::game::tests::mob_registry());
        assert_eq!(game.dimension(), Dimension::Nether);
        assert_eq!(game.player.body.position(), there);
        assert!(game.fluids.warm);
        assert_eq!(game.portals, vec![(Dimension::Overworld, [1, 2, 3])]);
        load_player_chunk(&mut game);
        let block = |game: &Game, at: IVec3| game.chunks.world().block(at.x, at.y, at.z);
        assert_eq!(block(&game, ember), Some(BlockState::of(blocks::GLOWSTONE)));
        assert!(game.saved_mobs().is_empty(), "the pig stayed home");

        game.change_dimension(Dimension::Overworld, Arrival::At(home));
        load_player_chunk(&mut game);
        assert_eq!(block(&game, mark), Some(BlockState::of(blocks::GOLD_BLOCK)));
        let mobs = game.saved_mobs();
        assert_eq!(mobs.len(), 1);
        assert_eq!(game.mob_defs.find(&mobs[0].name), Some(pig));
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
        game.change_dimension(Dimension::Nether, Arrival::At(DVec3::new(0.5, 80.0, 0.5)));
        assert!(game.mobs.is_empty() && game.saved_mobs().is_empty());
        let (away, _) = game.away_for_saving();
        assert_eq!(away.len(), 1);
        assert_eq!(away[0].dimension, Dimension::Overworld);
        game.change_dimension(
            Dimension::Overworld,
            Arrival::At(DVec3::new(0.5, 64.0, 0.5)),
        );
        // Back in the stash, waiting for its chunk.
        assert_eq!(game.saved_mobs().len(), 1);
        assert!(game.away_for_saving().0.is_empty());
    }
}
