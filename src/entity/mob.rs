//! A living mob: its body, health and what it's doing, following 1.0.0's
//! `EntityLiving`. Decisions come from `ai`; this file moves the body,
//! takes hits and drops loot.

use glam::{DVec3, IVec3};

use super::{
    ai::AiState,
    mobs::{MobDef, MobKind},
};
use crate::{
    inventory::stack::ItemStack,
    player::{
        collision::Collider,
        physics::{Body, MoveInput},
        stats::{DamageSource, Stats, Surroundings},
    },
    world::block::Fluid,
};

/// Ticks a hit makes a mob flash red (1.0.0's `maxHurtTime`).
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "combat and mob rendering come next")
)]
const HURT_FLASH: u32 = 10;
/// Ticks a dead mob lies on its side before it's removed.
pub const DEATH_TICKS: u32 = 20;
/// 1.0.0's `knockBack` strength.
const KNOCKBACK: f64 = 0.4;
/// Eye height as a fraction of a mob's height (1.0.0's `getEyeHeight`).
const EYE_FRACTION: f64 = 0.85;

#[derive(Debug, Clone)]
pub struct Mob {
    pub kind: MobKind,
    pub body: Body,
    prev_feet: DVec3,
    /// Facing, radians, 0 north (−Z), turning toward east (+X).
    pub yaw: f32,
    prev_yaw: f32,
    /// Where the head looks, radians (absolute yaw and pitch).
    pub head_yaw: f32,
    pub pitch: f32,
    pub stats: Stats,
    /// Ticks left flashing red after a hit.
    pub hurt_time: u32,
    /// Ticks since dying.
    pub death_time: u32,
    /// Ticks lived, for despawning (reset while the player is near).
    pub age: u32,
    /// Walking animation: distance walked and how fast (0–1).
    pub limb_swing: f32,
    pub limb_amount: f32,
    prev_limb_amount: f32,
    pub ai: AiState,
}

impl Mob {
    pub fn new(kind: MobKind, def: &MobDef, feet: DVec3, yaw: f32) -> Self {
        let body = Body::sized(feet, def.width, def.height, def.height * EYE_FRACTION);
        Self {
            kind,
            body,
            prev_feet: feet,
            yaw,
            prev_yaw: yaw,
            head_yaw: yaw,
            pitch: 0.0,
            stats: Stats::with_health(def.health),
            hurt_time: 0,
            death_time: 0,
            age: 0,
            limb_swing: 0.0,
            limb_amount: 0.0,
            prev_limb_amount: 0.0,
            ai: AiState::default(),
        }
    }

    pub fn feet(&self) -> DVec3 {
        self.body.position()
    }

    pub fn eye(&self) -> DVec3 {
        self.feet() + DVec3::Y * self.body.eye_height()
    }

    /// The block cell its feet are in.
    pub fn cell(&self) -> IVec3 {
        (self.feet() + DVec3::Y * 0.5).floor().as_ivec3()
    }

    pub fn is_dead(&self) -> bool {
        self.stats.is_dead()
    }

    /// Dead long enough to remove.
    pub fn is_gone(&self) -> bool {
        self.death_time >= DEATH_TICKS
    }

    /// Feet, facing and leg swing `alpha` of the way from the last tick.
    pub fn pose(&self, alpha: f32) -> (DVec3, f32, f32) {
        let feet = self.prev_feet.lerp(self.feet(), alpha as f64);
        let yaw = self.prev_yaw + angle_difference(self.yaw, self.prev_yaw) * alpha;
        let amount = self.prev_limb_amount + (self.limb_amount - self.prev_limb_amount) * alpha;
        (feet, yaw, amount)
    }

    /// Hits the mob for `amount` from `attacker` (a position, for
    /// knockback). Returns true for a fresh hit; the AI reacts to it.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "combat and mob rendering come next")
    )]
    pub fn hurt(&mut self, amount: i32, attacker: Option<DVec3>, source: DamageSource) -> bool {
        if self.is_dead() {
            return false;
        }
        let fresh = self.stats.damage(amount, source);
        if fresh {
            self.hurt_time = HURT_FLASH;
            self.ai.on_hurt();
            if let Some(from) = attacker {
                self.knock_back(from);
            }
        }
        fresh
    }

    /// 1.0.0's `knockBack`: half the speed, then pushed away and up.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "combat and mob rendering come next")
    )]
    pub fn knock_back(&mut self, from: DVec3) {
        knock_back(&mut self.body, from);
    }

    /// One tick of movement and status. `fire`: whether daylight set it
    /// burning this tick.
    pub fn tick(&mut self, input: &MoveInput, world: &impl Collider, eyes_in_water: bool) {
        self.prev_feet = self.feet();
        self.prev_yaw = self.yaw;
        self.prev_limb_amount = self.limb_amount;
        self.hurt_time = self.hurt_time.saturating_sub(1);
        self.age += 1;
        if self.is_dead() {
            self.death_time += 1;
            // Dead mobs stop walking but still fall.
            self.body.tick(&MoveInput::default(), self.yaw, world);
            return;
        }
        self.body.tick(input, self.yaw, world);
        let moved = self.feet() - self.prev_feet;
        let around = Surroundings {
            on_ground: self.body.on_ground,
            fell: -moved.y as f32,
            in_water: self.body.liquid == Some(Fluid::Water),
            in_lava: self.body.liquid == Some(Fluid::Lava),
            eyes_in_water,
            flying: false,
        };
        self.stats.tick(&around, true);
        // 1.0.0's limb swing: faster walking, bigger strides.
        let speed = (moved.x.hypot(moved.z) * 4.0).min(1.0) as f32;
        self.limb_amount += (speed - self.limb_amount) * 0.4;
        self.limb_swing += self.limb_amount;
    }

    /// What it drops on death: each drop 0–2 (or its range), cooked if it
    /// died burning.
    pub fn drops(&self, def: &MobDef, random: &mut impl FnMut() -> f32) -> Vec<ItemStack> {
        def.drops
            .iter()
            .filter_map(|drop| {
                let range = (drop.max - drop.min + 1) as f32;
                let count = drop.min + ((random() * range) as u8).min(drop.max - drop.min);
                let item = match drop.cooked {
                    Some(cooked) if self.stats.fire > 0 => cooked,
                    _ => drop.item,
                };
                (count > 0).then(|| ItemStack::with_damage(item, count, 0))
            })
            .collect()
    }
}

/// 1.0.0's `EntityLiving.knockBack` for any body hit from `from`.
pub fn knock_back(body: &mut Body, from: DVec3) {
    let away = body.position() - from;
    let mut push = DVec3::new(away.x, 0.0, away.z);
    if push.length_squared() < 1e-8 {
        push = DVec3::new(0.01, 0.0, 0.0);
    }
    let push = push.normalize() * KNOCKBACK;
    body.velocity /= 2.0;
    body.velocity.x += push.x;
    body.velocity.z += push.z;
    body.velocity.y = (body.velocity.y + KNOCKBACK).min(KNOCKBACK);
}

/// `a − b` wrapped into −π..π.
pub fn angle_difference(a: f32, b: f32) -> f32 {
    use std::f32::consts::{PI, TAU};
    (a - b + PI).rem_euclid(TAU) - PI
}
