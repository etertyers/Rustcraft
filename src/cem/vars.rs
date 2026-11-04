//! The entity variables CEM animations read (OptiFine's, and the ones
//! Entity Model Features adds): what a mob is doing, as numbers. The ones
//! Rustcraft can't know read as 0 or false and are listed once in the log.

use glam::DVec3;

use crate::entity::mob::{Mob, angle_difference};

/// `time` wraps after this many ticks (2 h 40 min of play): f32 keeps 1/64
/// of a tick below 2^18 = 262,144 ticks, so animations stay smooth.
pub const CLOCK_PERIOD: u64 = 24_000 * 8;

/// `time` and `day_time` for a whole number of ticks and the fraction.
pub fn clock(ticks: u64, alpha: f32) -> (f32, f32) {
    (
        (ticks % CLOCK_PERIOD) as f32 + alpha,
        (ticks % 24_000) as f32 + alpha,
    )
}

/// Every variable's name, in slot order.
pub const NAMES: [&str; 48] = [
    "limb_swing",
    "limb_speed",
    "age",
    "head_yaw",
    "head_pitch",
    "pos_x",
    "pos_y",
    "pos_z",
    "rot_x",
    "rot_y",
    "health",
    "max_health",
    "hurt_time",
    "death_time",
    "swing_progress",
    "move_forward",
    "move_strafing",
    "is_on_ground",
    "is_in_water",
    "is_in_lava",
    "is_riding",
    "is_ridden",
    "is_burning",
    "is_hurt",
    "is_aggressive",
    "is_child",
    "is_sneaking",
    "is_sprinting",
    "is_wet",
    "is_alive",
    "id",
    "player_pos_x",
    "player_pos_y",
    "player_pos_z",
    "time",
    "day_time",
    "frame_time",
    "frame_counter",
    "is_invisible",
    "is_glowing",
    "is_in_ocean",
    "fall_distance",
    "is_sitting",
    "is_sleeping",
    "dimension",
    "swing_progress_off",
    "anger_time",
    "variant",
];

pub const COUNT: usize = NAMES.len();

/// The slot of a variable name.
pub fn slot(name: &str) -> Option<u8> {
    NAMES.iter().position(|n| *n == name).map(|i| i as u8)
}

/// Names that exist in OptiFine or EMF but that Rustcraft has no value
/// for: they read as 0 and the log says so once.
pub fn known_but_unsupported(name: &str) -> bool {
    matches!(
        name,
        "is_swimming"
            | "is_flying"
            | "is_on_fire"
            | "is_jumping"
            | "is_spectator"
            | "is_tamed"
            | "is_leashed"
            | "is_pregnant"
            | "has_target"
            | "target_distance"
            | "speed"
            | "speed_x"
            | "speed_y"
            | "speed_z"
            | "item_count"
            | "light_block"
            | "light_sky"
    )
}

/// The values for a frame, indexed like `NAMES`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EntityVars(pub [f32; COUNT]);

impl Default for EntityVars {
    fn default() -> Self {
        Self([0.0; COUNT])
    }
}

/// What the frame knows besides the mob.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Frame {
    /// Whole game ticks since the world began. Kept apart from the
    /// fraction (`alpha`) so no precision is lost in an old world.
    pub ticks: u64,
    /// Seconds since the last frame.
    pub frame_time: f32,
    pub frame_counter: u32,
    /// Where the player is.
    pub player: DVec3,
    /// `alpha` of the way between ticks.
    pub alpha: f32,
}

impl EntityVars {
    /// The variables of `mob` at `frame`. Everything comes from the mob's
    /// synced state and the tick, so two clients agree.
    pub fn of(mob: &Mob, max_health: f32, frame: &Frame) -> Self {
        let mut v = [0.0f32; COUNT];
        let (feet, yaw, limb_amount) = mob.pose(frame.alpha);
        let set = |v: &mut [f32; COUNT], name: &str, value: f32| {
            if let Some(i) = slot(name) {
                v[usize::from(i)] = value;
            }
        };
        let flag = |b: bool| f32::from(b);
        set(&mut v, "limb_swing", mob.limb_swing);
        set(&mut v, "limb_speed", limb_amount);
        set(&mut v, "age", mob.age as f32 + frame.alpha);
        // Degrees, turned the way OptiFine's are (positive to the left).
        set(
            &mut v,
            "head_yaw",
            -angle_difference(mob.head_yaw, yaw).to_degrees(),
        );
        set(&mut v, "head_pitch", mob.pitch.to_degrees());
        set(&mut v, "pos_x", feet.x as f32);
        set(&mut v, "pos_y", feet.y as f32);
        set(&mut v, "pos_z", feet.z as f32);
        set(&mut v, "rot_x", mob.pitch.to_degrees());
        set(&mut v, "rot_y", -yaw.to_degrees());
        set(&mut v, "health", mob.stats.health);
        set(&mut v, "max_health", max_health);
        set(&mut v, "hurt_time", mob.hurt_time as f32);
        set(&mut v, "death_time", mob.death_time as f32);
        set(&mut v, "is_on_ground", flag(mob.body.on_ground));
        set(
            &mut v,
            "is_in_water",
            flag(mob.body.liquid == Some(crate::world::block::Fluid::Water)),
        );
        set(
            &mut v,
            "is_in_lava",
            flag(mob.body.liquid == Some(crate::world::block::Fluid::Lava)),
        );
        set(&mut v, "is_hurt", flag(mob.hurt_time > 0));
        set(&mut v, "is_aggressive", flag(mob.aggressive));
        set(&mut v, "is_child", flag(mob.data.age < 0));
        set(&mut v, "is_sneaking", flag(mob.body.sneaking));
        set(&mut v, "is_sprinting", flag(mob.body.sprinting));
        set(
            &mut v,
            "is_wet",
            flag(mob.body.liquid == Some(crate::world::block::Fluid::Water)),
        );
        set(&mut v, "is_alive", flag(!mob.is_dead()));
        set(&mut v, "is_sitting", flag(mob.data.sitting));
        set(&mut v, "id", (mob.id % 100_000) as f32);
        set(&mut v, "player_pos_x", frame.player.x as f32);
        set(&mut v, "player_pos_y", frame.player.y as f32);
        set(&mut v, "player_pos_z", frame.player.z as f32);
        // `time` wraps every `CLOCK_PERIOD` ticks before it becomes an f32
        // (which is only exact to 1/8 tick past a million); `day_time` is
        // exact because 24000 divides the period.
        let (time, day_time) = clock(frame.ticks, frame.alpha);
        set(&mut v, "time", time);
        set(&mut v, "day_time", day_time);
        set(&mut v, "frame_time", frame.frame_time);
        set(&mut v, "frame_counter", frame.frame_counter as f32);
        set(&mut v, "variant", f32::from(mob.data.variant));
        Self(v)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_name_has_its_own_slot() {
        let mut names = NAMES.to_vec();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), COUNT);
        assert_eq!(slot("limb_swing"), Some(0));
        assert_eq!(slot("is_alive"), Some(29));
        assert_eq!(slot("nonsense"), None);
        assert!(known_but_unsupported("is_swimming"));
        assert!(!known_but_unsupported("limb_swing"));
    }
}
