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
        blocks,
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

/// Lying in the bed whose head is at `head`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sleep {
    pub head: IVec3,
    pub ticks: u32,
}

impl Game {
    pub fn is_sleeping(&self) -> bool {
        self.sleep.is_some()
    }

    /// How dark the screen is while falling asleep, 0–1.
    pub fn sleep_fade(&self) -> Option<f32> {
        self.sleep
            .map(|sleep| (sleep.ticks as f32 / SLEEP_TICKS as f32).min(1.0))
    }

    /// Shows a notice for a while, in place of 1.0.0's chat line.
    pub(super) fn say(&mut self, text: &str) {
        self.message = Some((text.to_owned(), MESSAGE_TICKS));
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
        let angle = daytime::celestial_angle(self.time, 0.0);
        if daytime::is_daytime(angle) {
            return self.say("You can only sleep at night");
        }
        let center = head.as_dvec3() + DVec3::splat(0.5);
        let away = (self.body.position() - center).abs();
        if away.cmpgt(BED_REACH).any() {
            return;
        }
        let monster_near = self.mobs.iter().any(|mob| {
            let hostile = matches!(self.mob_defs.get(mob.kind).ai, Ai::Hostile { .. });
            hostile && !mob.is_dead() && (mob.feet() - center).abs().cmple(MONSTER_REACH).all()
        });
        if monster_near {
            return self.say("You may not rest now, there are monsters nearby");
        }
        self.mining = None;
        self.eating = None;
        self.drawing = None;
        self.release_input();
        self.body = Body::new(center.with_y(head.y as f64 + 0.5625));
        self.prev_feet = self.body.position();
        // Looking from the pillow toward the foot.
        let foot = -bed_direction(block).as_vec3();
        self.camera.yaw = foot.x.atan2(-foot.z);
        self.sleep = Some(Sleep { head, ticks: 0 });
    }

    /// One tick in bed: after `SLEEP_TICKS`, morning comes and the bed
    /// becomes the spawn point (1.0.0's `wakeAllPlayers`).
    pub(super) fn tick_sleep(&mut self) {
        let Some(sleep) = self.sleep.as_mut() else {
            return;
        };
        sleep.ticks += 1;
        if sleep.ticks < SLEEP_TICKS {
            return;
        }
        let head = sleep.head;
        self.time = (self.time + DAY_TICKS) / DAY_TICKS * DAY_TICKS;
        self.bed = Some(head);
        self.wake();
    }

    /// Gets up next to the bed.
    pub fn wake(&mut self) {
        let feet = self.standing_feet();
        if self.sleep.take().is_some() {
            self.stats.awake = 0;
            self.body = Body::new(feet);
            self.prev_feet = feet;
        }
    }

    /// Where the player's feet are, or for a sleeping player, where they'll
    /// stand on getting up (beside the bed, or on it if that's boxed in).
    pub(super) fn standing_feet(&self) -> DVec3 {
        match self.sleep {
            Some(sleep) => self
                .bed_exit(sleep.head)
                .unwrap_or_else(|| sleep.head.as_dvec3() + DVec3::new(0.5, 1.0, 0.5)),
            None => self.body.position(),
        }
    }

    /// Where to stand when leaving the bed whose head is at `head`: the
    /// nearest free cell beside it on solid ground (1.0.0's
    /// `getNearestEmptyChunkCoordinates`), or `None` if it isn't loaded,
    /// isn't a bed any more or is boxed in.
    fn bed_exit(&self, head: IVec3) -> Option<DVec3> {
        let world = self.chunks.world();
        let block = world.block(head.x, head.y, head.z)?;
        if !block.is(blocks::RED_BED) {
            return None;
        }
        let foot = head - bed_direction(block);
        free_spot(world, self.items.blocks, [foot, head])
    }

    /// Where the player comes back after dying: beside their bed if it's
    /// still there, or else the world spawn.
    pub(super) fn respawn_point(&mut self) -> DVec3 {
        let Some(head) = self.bed else {
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
                self.bed = None;
                self.say("Your home bed was missing or obstructed");
                self.spawn
            }
        }
    }

    /// The eye position while lying in bed.
    pub(super) fn lying_eye(&self) -> Option<DVec3> {
        let head = self.sleep?.head;
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
    fn beds_only_work_at_night_without_monsters_near() {
        let (mut game, head) = game_with_bed(6000);
        game.use_bed(head);
        assert!(!game.is_sleeping());
        assert_eq!(
            game.message.as_ref().map(|(text, _)| text.as_str()),
            Some("You can only sleep at night")
        );
        let (mut game, head) = game_with_bed(18000);
        let kind = game.mob_defs.find("zombie").unwrap();
        let near = DVec3::new(6.5, 64.0, -2.5);
        game.mobs
            .push(Mob::new(kind, game.mob_defs.get(kind), near, 0.0));
        game.use_bed(head);
        assert!(!game.is_sleeping());
        assert_eq!(
            game.message.as_ref().map(|(text, _)| text.as_str()),
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
        assert_eq!(game.sleep, Some(Sleep { head, ticks: 0 }));
        for _ in 0..SLEEP_TICKS - 1 {
            game.tick();
        }
        assert!(game.is_sleeping());
        assert!(game.sleep_fade().unwrap() > 0.95);
        game.tick();
        assert!(!game.is_sleeping());
        assert_eq!(game.time, 24000 * 4 + 1, "sunrise, plus the waking tick");
        assert_eq!(game.bed, Some(head));
        let feet = game.body.position();
        assert_eq!(feet.y, 64.0, "standing beside the bed");
        assert!(feet.distance(head.as_dvec3()) < 3.0, "{feet}");

        // Respawning comes back beside the bed, until it's broken.
        game.body = Body::new(DVec3::new(20.5, 64.0, 20.5));
        game.respawn();
        assert!(game.body.position().distance(head.as_dvec3()) < 3.0);
        game.break_block(head);
        game.respawn();
        assert_eq!(game.body.position(), game.spawn);
        assert_eq!(game.bed, None);
        assert_eq!(
            game.message.as_ref().map(|(text, _)| text.as_str()),
            Some("Your home bed was missing or obstructed")
        );
    }

    #[test]
    fn getting_hurt_or_leaving_wakes_without_morning_or_spawn() {
        let (mut game, head) = game_with_bed(18000);
        game.use_bed(head);
        game.stats.fire = 25;
        for _ in 0..10 {
            game.tick();
        }
        assert!(!game.is_sleeping(), "the fire woke them");
        assert_eq!(game.bed, None);
        assert_eq!(game.time, 18010);

        let (mut game, head) = game_with_bed(18000);
        game.use_bed(head);
        game.tick();
        game.wake();
        assert!(!game.is_sleeping());
        assert_eq!((game.bed, game.time), (None, 18001));
    }
}
