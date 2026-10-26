//! The way to the End (M27): eyes of ender set in portal frames or thrown
//! toward the nearest stronghold, the portal they light, and travel through
//! it both ways (modern `EnderEyeItem`, `EndPortalBlock`).
//!
//! Stepping into an End portal in the Overworld sends the player to the
//! obsidian platform at (100, 50, 0) at once. The portal in the End, over
//! the island's middle, sends them to their Overworld spawn; the first time
//! the poem and credits play before that.

use glam::{DVec3, IVec3};

use super::Game;
use crate::{
    entity::{
        eye::{EyeOfEnder, Landing},
        item::ItemEntity,
        particle::Particle,
    },
    inventory::{items, stack::ItemStack},
    player::raycast::Hit,
    world::{
        block::BlockState, blocks, chunk::ChunkPos, dimension::Dimension, end_portal, props::Prop,
        structure::stronghold, worldgen::end,
    },
};

/// Chance in five that a thrown eye breaks.
const BREAK_IN: u32 = 5;
/// Color of the specks a breaking eye scatters.
const EYE_SPECKS: u32 = 0x4CC5A0;

impl Game {
    /// Right-click with an eye of ender: sets it in the frame looked at, or
    /// throws it.
    pub(super) fn use_eye(&mut self, hit: Option<Hit>) {
        let frame = hit.and_then(|hit| {
            let state = self
                .chunks
                .world()
                .block(hit.block.x, hit.block.y, hit.block.z)?;
            (state.is(blocks::END_PORTAL_FRAME) && !state.bool(Prop::Eye))
                .then_some((hit.block, state))
        });
        match frame {
            Some((cell, state)) => self.place_eye(cell, state),
            None => self.throw_eye(),
        }
    }

    /// Sets the held eye in the empty frame at `cell`; the twelfth lights
    /// the portal.
    fn place_eye(&mut self, cell: IVec3, frame: BlockState) {
        self.edit(&[(cell, frame.with_bool(Prop::Eye, true))]);
        crate::audio::play(
            "block.end_portal_frame.fill",
            cell.as_dvec3() + DVec3::splat(0.5),
        );
        if self.player.mode.uses_up_items() {
            self.player.inventory.take(self.held_slot(), 1);
        }
        self.player.view.swing();
        let world = self.chunks.world();
        let get = |at: IVec3| world.block(at.x, at.y, at.z);
        if let Some(center) = end_portal::find_lit(cell, &get) {
            let portal = BlockState::of(blocks::END_PORTAL);
            let edits: Vec<_> = end_portal::middle(center)
                .into_iter()
                .map(|at| (at, portal))
                .collect();
            self.edit(&edits);
            crate::audio::play_here_with("block.end_portal.spawn", 1.0, 1.0);
        }
    }

    /// The block column of the stronghold nearest `from`, as modern's eye
    /// finds it (the start of its first piece).
    pub(super) fn nearest_stronghold(&self, from: DVec3) -> Option<(f64, f64)> {
        let seed = self.chunks.seed()?;
        stronghold::positions(seed)
            .into_iter()
            .map(|chunk| (f64::from(chunk.x * 16 + 8), f64::from(chunk.z * 16 + 8)))
            .min_by(|a, b| {
                let da = (a.0 - from.x).hypot(a.1 - from.z);
                let db = (b.0 - from.x).hypot(b.1 - from.z);
                da.total_cmp(&db)
            })
    }

    /// Throws the held eye toward the nearest stronghold. In another
    /// dimension there is nothing to find, so it stays in the hand.
    fn throw_eye(&mut self) {
        if self.dimension != Dimension::Overworld {
            return;
        }
        let from = self.player.body.position()
            + DVec3::Y * (self.player.body.aabb().max.y - self.player.body.position().y) / 2.0;
        let Some(toward) = self.nearest_stronghold(from) else {
            return;
        };
        let survives = self.rng.below(BREAK_IN) != 0;
        crate::audio::play_at("entity.ender_eye.launch", from, 1.0, 0.8);
        self.eyes.push(EyeOfEnder::throw(from, toward, survives));
        if self.player.mode.uses_up_items() {
            self.player.inventory.take(self.held_slot(), 1);
        }
        self.player.view.swing();
    }

    /// One tick of the eyes in flight: those that finish drop as items or
    /// break.
    pub(super) fn tick_eyes(&mut self) {
        let mut landed = Vec::new();
        self.eyes.retain_mut(|eye| match eye.tick() {
            Some(landing) => {
                landed.push(landing);
                false
            }
            None => true,
        });
        for landing in landed {
            match landing {
                Landing::Dropped(at) => {
                    let hover = self.rng.next_f64() as f32 * std::f32::consts::TAU;
                    self.entities.push(ItemEntity::new(
                        ItemStack::new(items::ENDER_EYE, 1),
                        at,
                        DVec3::ZERO,
                        hover,
                    ));
                }
                Landing::Broke(at) => {
                    crate::audio::play("entity.ender_eye.death", at);
                    let rng = &mut self.rng;
                    let mut random = || rng.next_f64() as f32;
                    let specks: Vec<_> = (0..12)
                        .map(|_| Particle::colored(at, EYE_SPECKS, &mut random))
                        .collect();
                    self.add_particles(specks);
                }
            }
        }
    }

    /// Whether the player's hitbox touches an End portal.
    fn in_end_portal(&self) -> bool {
        let bb = self.player.body.aabb();
        let min = bb.min.floor().as_ivec3();
        let max = (bb.max - DVec3::splat(1e-7)).floor().as_ivec3();
        let world = self.chunks.world();
        (min.y..=max.y).any(|y| {
            (min.z..=max.z).any(|z| {
                (min.x..=max.x).any(|x| world.block(x, y, z).is_some_and(end_portal::is_portal))
            })
        })
    }

    /// Standing in an End portal takes the player through it.
    pub(super) fn tick_end_portal(&mut self) {
        if self.is_riding() || self.player.credits.is_some() || !self.in_end_portal() {
            return;
        }
        match self.dimension {
            Dimension::Overworld => self.enter_end(),
            Dimension::End => self.leave_end(),
            Dimension::Nether => {}
        }
    }

    /// To the End's arrival platform, which is rebuilt each time.
    pub(super) fn enter_end(&mut self) {
        self.change_dimension(Dimension::End, super::dimensions::Arrival::EndPlatform);
    }

    /// Arriving in the End: on the platform, facing the island, and the
    /// fight begun if it hasn't.
    pub(super) fn arrive_in_the_end(&mut self) {
        let feet = end::PLATFORM.as_dvec3() + DVec3::new(0.5, 0.0, 0.5);
        self.place_player(feet);
        let platform = end::platform();
        // The platform reaches into neighbor chunks; they must be there to
        // take it.
        for (at, _) in &platform {
            self.chunks.load_now(ChunkPos::of_block(at.x, at.z));
        }
        self.edit(&platform);
        // Facing the island, as modern's arrival does.
        self.player.camera.yaw = 0.0_f32.to_radians() - std::f32::consts::FRAC_PI_2;
        self.player.camera.pitch = 0.0;
        self.start_fight();
    }

    /// Through the exit portal: the poem and credits the first time, then
    /// home.
    pub(super) fn leave_end(&mut self) {
        if !self.end.credits_seen {
            self.end.credits_seen = true;
            self.player.credits = Some(0);
            return;
        }
        self.go_home();
    }

    /// Back to the Overworld spawn or bed, the player's health and things
    /// as they were.
    pub(super) fn go_home(&mut self) {
        self.change_dimension(Dimension::Overworld, super::dimensions::Arrival::Home);
    }

    /// Starts the credits `ticks` in, for a `--shot` scene.
    pub fn stage_credits(&mut self, ticks: u32) {
        self.player.credits = Some(ticks);
    }

    /// Ticks the credits are on screen for, or `None` when they aren't.
    pub fn credits_ticks(&self) -> Option<u32> {
        self.player.credits
    }

    /// Ends the credits (they run out or are skipped): home.
    pub fn finish_credits(&mut self) {
        if self.player.credits.take().is_some() {
            self.go_home();
        }
    }

    /// One tick of the credits rolling.
    pub(super) fn tick_credits(&mut self) {
        let Some(ticks) = self.player.credits else {
            return;
        };
        if ticks >= crate::credits::TICKS {
            self.finish_credits();
        } else {
            self.player.credits = Some(ticks + 1);
        }
    }
}
