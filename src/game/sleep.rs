//! Sleeping in beds, following 1.0.0's `BlockBed.blockActivated` and
//! `EntityPlayer.sleepInBedAt`: only at night, near the bed and with no
//! monsters close by. After 100 ticks asleep it's morning and the bed is
//! where the player respawns; getting hurt or leaving the bed earlier
//! wakes them without either.

use glam::{DVec3, IVec3};

use super::Game;
use crate::{
    entity::mobs::Ai,
    player::physics::Body,
    world::{
        World,
        block::{BlockDef, BlockRegistry, bed_direction},
        chunk::ChunkPos,
        daytime::{self, DAY_TICKS},
        props::{Prop, bed_part},
    },
};

/// Ticks asleep before morning comes (1.0.0's `sleepTimer` reaching 100).
pub const SLEEP_TICKS: u32 = 100;
/// Monsters this close to the bed (sideways, up or down) keep the player
/// awake.
const MONSTER_REACH: DVec3 = DVec3::new(8.0, 5.0, 8.0);
/// How far from the bed's head the player may be to lie down.
const BED_REACH: DVec3 = DVec3::new(3.0, 2.0, 3.0);
/// Ticks a notice stays on screen (1.0.0's chat lines last 10 seconds).
const MESSAGE_TICKS: u32 = 200;
/// A lying player's eyes above the bed's cell.
const LYING_EYE: f64 = 11.0 / 16.0;
/// A bed used outside the Overworld blows up this hard, setting fires.
const BED_BLAST: f32 = 5.0;

/// Lying in the bed whose head is at `head`.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Sleep {
    pub head: IVec3,
    pub ticks: u32,
}

impl Game {
    pub fn is_sleeping(&self) -> bool {
        self.player.sleep.is_some()
    }

    /// How dark the screen is while falling asleep, 0–1.
    pub fn sleep_fade(&self) -> Option<f32> {
        self.player
            .sleep
            .map(|sleep| (sleep.ticks as f32 / SLEEP_TICKS as f32).min(1.0))
    }

    /// Shows a notice for a while, in place of 1.0.0's chat line.
    pub(super) fn say(&mut self, text: &str) {
        self.player.message = Some((text.to_owned(), MESSAGE_TICKS));
    }

    /// Right-click on either half of the bed at `cell`.
    pub(super) fn use_bed(&mut self, cell: IVec3) {
        let Some(block) = self.chunks.world().block(cell.x, cell.y, cell.z) else {
            return;
        };
        let head = if block.get(Prop::BedPart) == bed_part::FOOT {
            cell + bed_direction(block)
        } else {
            cell
        };
        // Outside the Overworld a bed blows up (modern `BedBlock.use`).
        if self.dimension() != crate::world::dimension::Dimension::Overworld {
            let foot = head - bed_direction(block);
            let air = crate::world::block::BlockState::AIR;
            self.edit(&[(head, air), (foot, air)]);
            self.explode_with_fire(head.as_dvec3() + DVec3::splat(0.5), BED_BLAST);
            return;
        }
        let center = head.as_dvec3() + DVec3::splat(0.5);
        let away = (self.player.body.position() - center).abs();
        if away.cmpgt(BED_REACH).any() {
            return self.say("You may not rest now; the bed is too far away");
        }
        // Using a bed, day or night, makes it where the player respawns
        // (modern `ServerPlayer.startSleepInBed`).
        let newly_set = self.player.bed != Some(head);
        self.player.bed = Some(head);
        let angle = daytime::celestial_angle(self.time, 0.0);
        if daytime::is_daytime(angle) {
            return self.say(if newly_set {
                "Respawn point set"
            } else {
                "You can sleep only at night"
            });
        }
        if newly_set {
            self.say("Respawn point set");
        }
        let monster_near = self.mobs.iter().any(|mob| {
            let hostile = matches!(self.mob_defs.get(mob.kind).ai, Ai::Hostile { .. });
            hostile && !mob.is_dead() && (mob.feet() - center).abs().cmple(MONSTER_REACH).all()
        });
        if monster_near {
            return self.say("You may not rest now, there are monsters nearby");
        }
        self.player.mining = None;
        self.player.eating = None;
        self.player.drawing = None;
        self.release_input();
        self.player.body = Body::new(center.with_y(head.y as f64 + 0.5625));
        self.player.prev_feet = self.player.body.position();
        // Looking from the pillow toward the foot.
        let foot = -bed_direction(block).as_vec3();
        self.player.camera.yaw = foot.x.atan2(-foot.z);
        self.player.sleep = Some(Sleep { head, ticks: 0 });
    }

    /// One tick in bed: after `SLEEP_TICKS`, morning comes and the bed
    /// becomes the spawn point (1.0.0's `wakeAllPlayers`).
    pub(super) fn tick_sleep(&mut self) {
        let Some(sleep) = self.player.sleep.as_mut() else {
            return;
        };
        sleep.ticks = (sleep.ticks + 1).min(SLEEP_TICKS);
        // The night passes once everyone is fast asleep; until then, a
        // player who is waits in bed.
        let asleep = (0..self.player_count()).all(|i| {
            self.player_at(i)
                .sleep
                .is_some_and(|sleep| sleep.ticks >= SLEEP_TICKS)
        });
        if !asleep {
            return;
        }
        self.time = (self.time + DAY_TICKS) / DAY_TICKS * DAY_TICKS;
        self.for_each_player(Game::wake_in_the_morning);
    }

    /// The night passed: `self.player` gets up, and their bed is where
    /// they'll respawn.
    fn wake_in_the_morning(&mut self) {
        let Some(sleep) = self.player.sleep else {
            return;
        };
        self.player.bed = Some(sleep.head);
        self.note(crate::advancements::Event::Used("sleep".to_owned()));
        self.wake();
    }

    /// Gets up next to the bed.
    pub fn wake(&mut self) {
        let feet = self.standing_feet();
        if self.player.sleep.take().is_some() {
            self.player.stats.awake = 0;
            self.player.body = Body::new(feet);
            self.player.prev_feet = feet;
        }
    }

    /// Where the player's feet are, or for a sleeping player, where they'll
    /// stand on getting up (beside the bed, or on it if that's boxed in).
    pub(super) fn standing_feet(&self) -> DVec3 {
        match self.player.sleep {
            Some(sleep) => self
                .bed_exit(sleep.head)
                .unwrap_or_else(|| sleep.head.as_dvec3() + DVec3::new(0.5, 1.0, 0.5)),
            None => self.player.body.position(),
        }
    }

    /// Where to stand when leaving the bed whose head is at `head`: the
    /// nearest free cell beside it on solid ground (1.0.0's
    /// `getNearestEmptyChunkCoordinates`), or `None` if it isn't loaded,
    /// isn't a bed any more or is boxed in.
    fn bed_exit(&self, head: IVec3) -> Option<DVec3> {
        let world = self.chunks.world();
        let block = world.block(head.x, head.y, head.z)?;
        if block.def().placement != crate::world::block::Placement::Bed {
            return None;
        }
        let foot = head - bed_direction(block);
        free_spot(world, self.items.blocks, [foot, head])
    }

    /// Where the player comes back after dying: beside their bed if it's
    /// still there, or else the world spawn.
    pub(super) fn respawn_point(&mut self) -> DVec3 {
        let Some(head) = self.player.bed else {
            return self.spawn;
        };
        let pos = ChunkPos::of_block(head.x, head.z);
        if self.chunks.world().get(pos).is_none() {
            // Too far to check; stand on it and fall if it's gone.
            return head.as_dvec3() + DVec3::new(0.5, 1.0, 0.5);
        }
        match self.bed_exit(head) {
            Some(feet) => feet,
            None => {
                self.player.bed = None;
                self.say("You have no home bed or charged respawn anchor, or it was obstructed");
                self.spawn
            }
        }
    }

    /// The eye position while lying in bed.
    pub(super) fn lying_eye(&self) -> Option<DVec3> {
        let head = self.player.sleep?.head;
        Some(head.as_dvec3() + DVec3::new(0.5, LYING_EYE, 0.5))
    }
}

/// Feet position in the first cell beside the bed (around its foot, then
/// its head) with a full block underneath and room to stand.
fn free_spot(world: &World, blocks: &BlockRegistry, bed: [IVec3; 2]) -> Option<DVec3> {
    let def = |at: IVec3| blocks.get(world.block(at.x, at.y, at.z)?.id());
    for half in bed {
        for (dx, dz) in [
            (-1, 0),
            (1, 0),
            (0, -1),
            (0, 1),
            (-1, -1),
            (1, -1),
            (-1, 1),
            (1, 1),
        ] {
            let cell = half + IVec3::new(dx, 0, dz);
            if bed.contains(&cell) {
                continue;
            }
            let ground = def(cell - IVec3::Y).is_some_and(BlockDef::is_opaque_cube);
            let open = |at: IVec3| def(at).is_some_and(|def| !def.solid);
            if ground && open(cell) && open(cell + IVec3::Y) {
                return Some(cell.as_dvec3() + DVec3::new(0.5, 0.0, 0.5));
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use glam::{DVec3, IVec3};

    use super::*;
    use crate::world::blocks;
    use crate::{
        entity::mob::Mob,
        game::tests::{flat_game, mob_registry},
        player::mode::GameMode,
        world::block::BlockState,
    };

    /// A survival flat world with a bed whose head points north, its foot
    /// one block north of the player.
    fn game_with_bed(time: u64) -> (Game, IVec3) {
        let mut game = flat_game(GameMode::Survival);
        game.mob_defs = mob_registry();
        let foot = IVec3::new(0, 64, -1);
        let head = IVec3::new(0, 64, -2);
        let bed =
            BlockState::of(blocks::RED_BED).with_face(Prop::Facing, crate::world::Face::North);
        game.chunks
            .set_blocks(&[(foot, bed), (head, bed.with(Prop::BedPart, bed_part::HEAD))]);
        game.time = time;
        (game, head)
    }

    #[test]
    fn the_night_passes_only_when_every_player_is_asleep() {
        let (mut game, head) = game_with_bed(18000);
        // A second bed for the guest, a few blocks east.
        let bed =
            BlockState::of(blocks::RED_BED).with_face(Prop::Facing, crate::world::Face::North);
        let guest_head = IVec3::new(4, 64, -2);
        game.chunks.set_blocks(&[
            (IVec3::new(4, 64, -1), bed),
            (guest_head, bed.with(Prop::BedPart, bed_part::HEAD)),
        ]);
        game.others.push(crate::game::PlayerState::new(
            DVec3::new(4.5, 64.0, 0.5),
            0.0,
            0.0,
        ));
        game.use_bed(head);
        assert!(game.is_sleeping());
        game.run_ticks(200);
        assert!(game.is_sleeping(), "still waiting for the guest");
        assert!(game.time() < 24000, "still night");
        game.with_player(1, |guest| guest.use_bed(guest_head));
        game.run_ticks(120);
        assert!(game.time() >= 24000, "morning");
        assert!(
            !game.is_sleeping() && game.others[0].sleep.is_none(),
            "both up"
        );
        assert_eq!(game.others[0].bed, Some(guest_head));
    }

    #[test]
    fn a_bed_in_the_nether_blows_up_and_sets_fires() {
        let (mut game, head) = game_with_bed(18000);
        game.dimension = crate::world::dimension::Dimension::Nether;
        game.use_bed(head);
        assert!(!game.is_sleeping());
        let world = game.chunks.world();
        assert!(world.block(head.x, head.y, head.z).unwrap().is_air());
        // A crater with fire in it, and a hurt player beside it.
        assert!(world.block(head.x, head.y - 1, head.z).unwrap().is_air());
        let fires = (-5..=5)
            .flat_map(|dz| (-5..=5).flat_map(move |dx| (58..68).map(move |y| (dx, y, dz))))
            .filter(|&(dx, y, dz)| {
                world
                    .block(head.x + dx, y, head.z + dz)
                    .is_some_and(|b| b.is(blocks::FIRE))
            })
            .count();
        assert!(fires > 0, "fire in the crater");
        assert!(game.player.stats.health < 20.0);
    }

    #[test]
    fn beds_only_work_at_night_without_monsters_near() {
        let (mut game, head) = game_with_bed(6000);
        // By day a bed only sets the respawn point (modern).
        game.use_bed(head);
        assert!(!game.is_sleeping());
        assert_eq!(game.player.bed, Some(head));
        assert_eq!(
            game.player.message.as_ref().map(|(text, _)| text.as_str()),
            Some("Respawn point set")
        );
        game.use_bed(head);
        assert_eq!(
            game.player.message.as_ref().map(|(text, _)| text.as_str()),
            Some("You can sleep only at night")
        );
        let (mut game, head) = game_with_bed(18000);
        let kind = game.mob_defs.find("zombie").unwrap();
        let near = DVec3::new(6.5, 64.0, -2.5);
        game.mobs
            .push(Mob::new(kind, game.mob_defs.get(kind), near, 0.0));
        game.use_bed(head);
        assert!(!game.is_sleeping());
        assert_eq!(
            game.player.message.as_ref().map(|(text, _)| text.as_str()),
            Some("You may not rest now, there are monsters nearby")
        );
        // A pig is no reason to stay up.
        let pig = game.mob_defs.find("pig").unwrap();
        game.mobs[0] = Mob::new(pig, game.mob_defs.get(pig), near, 0.0);
        game.use_bed(head);
        assert!(game.is_sleeping());
    }

    #[test]
    fn a_night_in_bed_skips_to_morning_and_sets_the_spawn() {
        let (mut game, head) = game_with_bed(24000 * 3 + 15000);
        // Clicking the foot lies down in the head.
        game.use_bed(head + IVec3::Z);
        assert_eq!(game.player.sleep, Some(Sleep { head, ticks: 0 }));
        for _ in 0..SLEEP_TICKS - 1 {
            game.tick();
        }
        assert!(game.is_sleeping());
        assert!(game.sleep_fade().unwrap() > 0.95);
        game.tick();
        assert!(!game.is_sleeping());
        assert_eq!(game.time, 24000 * 4 + 1, "sunrise, plus the waking tick");
        assert_eq!(game.player.bed, Some(head));
        let feet = game.player.body.position();
        assert_eq!(feet.y, 64.0, "standing beside the bed");
        assert!(feet.distance(head.as_dvec3()) < 3.0, "{feet}");

        // Respawning comes back beside the bed, until it's broken.
        game.player.body = Body::new(DVec3::new(20.5, 64.0, 20.5));
        game.respawn();
        assert!(game.player.body.position().distance(head.as_dvec3()) < 3.0);
        game.break_block(head);
        game.respawn();
        assert_eq!(game.player.body.position(), game.spawn);
        assert_eq!(game.player.bed, None);
        assert_eq!(
            game.player.message.as_ref().map(|(text, _)| text.as_str()),
            Some("You have no home bed or charged respawn anchor, or it was obstructed")
        );
    }

    #[test]
    fn getting_hurt_or_leaving_wakes_without_morning() {
        let (mut game, head) = game_with_bed(18000);
        game.use_bed(head);
        game.player.stats.fire = 25;
        for _ in 0..10 {
            game.tick();
        }
        assert!(!game.is_sleeping(), "the fire woke them");
        // Lying down already set the respawn point (modern).
        assert_eq!(game.player.bed, Some(head));
        assert_eq!(game.time, 18010);

        let (mut game, head) = game_with_bed(18000);
        game.use_bed(head);
        game.tick();
        game.wake();
        assert!(!game.is_sleeping());
        assert_eq!((game.player.bed, game.time), (Some(head), 18001));
    }

    #[test]
    fn a_bed_too_far_away_says_so() {
        let (mut game, head) = game_with_bed(18000);
        game.player.body = Body::new(head.as_dvec3() + DVec3::new(6.5, 0.0, 0.5));
        game.use_bed(head);
        assert!(!game.is_sleeping());
        assert_eq!(game.player.bed, None);
        assert_eq!(
            game.player.message.as_ref().map(|(text, _)| text.as_str()),
            Some("You may not rest now; the bed is too far away")
        );
    }
}
