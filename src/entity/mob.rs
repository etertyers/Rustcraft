//! A living mob: its body, health and what it's doing, following 1.0.0's
//! `EntityLiving`. Decisions come from `ai`; this file moves the body,
//! takes hits and drops loot.

use glam::{DVec3, IVec3};

use super::{
    ai::AiState,
    mobs::{MobDef, MobKind},
};
use serde::{Deserialize, Serialize};

use crate::{
    inventory::stack::ItemStack,
    player::{
        collision::Collider,
        physics::{Body, MoveInput},
        stats::{DamageSource, Stats, Surroundings},
    },
    world::block::{BlockState, Fluid},
};

/// Ticks a hit makes a mob flash red (1.0.0's `maxHurtTime`).
const HURT_FLASH: u32 = 10;
/// Ticks a dead mob lies on its side before it's removed.
pub const DEATH_TICKS: u32 = 20;
/// 1.0.0's `knockBack` strength.
const KNOCKBACK: f64 = 0.4;
/// What sets one mob apart from another of its kind: coat, dye, whether it
/// is tame, sheared, saddled or angry, and a few counters.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct MobData {
    /// Which of the kind's textures it wears (0 is the plain one).
    #[serde(default)]
    pub variant: u8,
    /// A sheep's dye, 0–15 in the order of the 16 wool colors.
    #[serde(default)]
    pub color: u8,
    #[serde(default)]
    pub sheared: bool,
    #[serde(default)]
    pub tamed: bool,
    #[serde(default)]
    pub sitting: bool,
    #[serde(default)]
    pub saddled: bool,
    /// A slime's size: 1, 2 or 4 (0 for the kinds that have none).
    #[serde(default)]
    pub size: u8,
    /// Ticks until a hen lays, a sheep regrows its wool, and so on.
    #[serde(default)]
    pub timer: u32,
    /// Taming progress for a horse (temper) or similar.
    #[serde(default)]
    pub temper: u8,
    /// Ticks left in love mode after being fed breeding food.
    #[serde(default)]
    pub love: u32,
    /// A villager's job, trades, bed and so on (added in M23).
    #[serde(default)]
    pub villager: Option<Box<super::villager::VillagerData>>,
}

/// A kill within this many ticks of the player's last hit drops
/// experience (modern `lastHurtByPlayerTime`).
const PLAYER_HIT_TICKS: u32 = 100;

#[derive(Debug, Clone)]
pub struct Mob {
    /// Tells it from the others for as long as the game runs (0 until the
    /// game adds it).
    pub id: u64,
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
    /// Its drops have been dropped (once, however it died).
    pub looted: bool,
    pub data: MobData,
    /// The block an enderman holds.
    pub carrying: Option<BlockState>,
    /// A creeper's swelling, 0–30, and whether a mob has its arms up.
    pub swell: i32,
    prev_swell: i32,
    pub aggressive: bool,
    /// The mob the player rides, if it's one.
    pub ridden: bool,
    /// Ticks left in which a death counts as the player's kill (it
    /// drops experience only then).
    pub player_hit: u32,
    /// Fire and lava don't hurt it (from its kind).
    pub fire_immune: bool,
    /// What the sound code has already heard of it.
    pub heard: Heard,
}

/// The health, death and stride of a mob as the sound code last saw them,
/// to tell what happened since.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Heard {
    pub health: f32,
    pub dead: bool,
    pub stride: f32,
    /// A creeper's swelling and an enderman's anger.
    pub swell: i32,
    pub aggressive: bool,
}

impl Mob {
    pub fn new(kind: MobKind, def: &MobDef, feet: DVec3, yaw: f32) -> Self {
        Self::with_data(kind, def, feet, yaw, MobData::default())
    }

    /// A mob with the given traits; a slime's size sets its hitbox and
    /// health.
    pub fn with_data(kind: MobKind, def: &MobDef, feet: DVec3, yaw: f32, data: MobData) -> Self {
        let size = f64::from(data.size.max(1));
        let (width, height) = if data.size > 0 {
            (def.width * size, def.height * size)
        } else {
            (def.width, def.height)
        };
        let health = if data.size > 0 {
            (data.size.max(1) as i32).pow(2)
        } else {
            def.health
        };
        let body = Body::sized(feet, width, height, height * def.eye);
        Self {
            id: 0,
            kind,
            body,
            prev_feet: feet,
            yaw,
            prev_yaw: yaw,
            head_yaw: yaw,
            pitch: 0.0,
            stats: Stats::with_health(health as f32),
            hurt_time: 0,
            death_time: 0,
            age: 0,
            limb_swing: 0.0,
            limb_amount: 0.0,
            prev_limb_amount: 0.0,
            ai: AiState::default(),
            looted: false,
            data,
            carrying: None,
            swell: 0,
            prev_swell: 0,
            aggressive: false,
            ridden: false,
            player_hit: 0,
            fire_immune: def.fire_immune,
            heard: Heard {
                health: health as f32,
                ..Heard::default()
            },
        }
    }

    /// Forgets the path it was following.
    pub fn clear_path(&mut self) {
        self.ai.clear_path();
    }

    /// A creeper's swelling `alpha` of the way from the last tick, 0–1.
    pub fn swell_amount(&self, alpha: f32) -> f32 {
        let swell = self.prev_swell as f32 + (self.swell - self.prev_swell) as f32 * alpha;
        (swell / 28.0).clamp(0.0, 1.0)
    }

    /// Its width and height now.
    pub fn size(&self) -> (f64, f64) {
        let a = self.body.aabb();
        (a.max.x - a.min.x, a.max.y - a.min.y)
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
    pub fn hurt(&mut self, amount: f32, attacker: Option<DVec3>, source: DamageSource) -> bool {
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

    /// Hurts it with the world rather than an attacker (water on an
    /// enderman, air for a squid): no flinching or fleeing.
    /// Marks a hit by the player, whose kill drops experience.
    pub fn hit_by_player(&mut self) {
        self.player_hit = PLAYER_HIT_TICKS;
    }

    pub fn hurt_by_world(&mut self, amount: f32, source: DamageSource) {
        if !self.is_dead() && self.stats.damage(amount, source) {
            self.hurt_time = HURT_FLASH;
        }
    }

    /// Pushes the mob away from `from`.
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
        self.player_hit = self.player_hit.saturating_sub(1);
        self.prev_swell = self.swell;
        self.age += 1;
        if self.is_dead() {
            self.death_time += 1;
            // Dead mobs stop walking but still fall.
            self.body.tick(&MoveInput::default(), self.yaw, world);
            return;
        }
        self.body.tick(input, self.yaw, world);
        let moved = self.feet() - self.prev_feet;
        // The Nether's mobs shrug off fire and lava.
        let burns = !self.fire_immune;
        if !burns {
            self.stats.fire = 0;
        }
        let around = Surroundings {
            on_ground: self.body.on_ground,
            fell: -moved.y as f32,
            in_water: self.body.liquid == Some(Fluid::Water),
            in_lava: burns && self.body.liquid == Some(Fluid::Lava),
            in_fire: burns && world.in_fire(&self.body.aabb()),
            eyes_in_water,
            flying: false,
            no_fall: false,
            on_magma: false,
        };
        self.stats.tick(&around, true);
        // 1.0.0's limb swing: faster walking, bigger strides.
        let speed = (moved.x.hypot(moved.z) * 4.0).min(1.0) as f32;
        self.limb_amount += (speed - self.limb_amount) * 0.4;
        self.limb_swing += self.limb_amount;
    }

    /// What it drops on death: each drop 0–2 (or its range), cooked if it
    /// died burning.
    pub fn drops(
        &self,
        def: &MobDef,
        looting: u8,
        random: &mut impl FnMut() -> f32,
    ) -> Vec<ItemStack> {
        // Only the smallest slimes drop slimeballs, and only the bigger
        // magma cubes magma cream.
        let small = self.data.size <= 1;
        if self.data.size > 0 && small == (def.name == "magma_cube") {
            return Vec::new();
        }
        let mut stacks = Vec::new();
        for drop in &def.drops {
            // Looting makes the rarer drops likelier, by a point a level.
            let chance = drop.chance + 0.01 * f32::from(looting);
            if drop.chance < 1.0 && random() >= chance {
                continue;
            }
            let range = (drop.max - drop.min + 1) as f32;
            let mut count = drop.min + ((random() * range) as u8).min(drop.max - drop.min);
            // And the common ones come in bigger numbers: up to `looting`
            // more.
            if looting > 0 && drop.chance >= 1.0 && drop.min != drop.max {
                count += (random() * f32::from(looting + 1)) as u8;
            }
            let item = match drop.cooked {
                Some(cooked) if self.stats.fire > 0 => cooked,
                _ => drop.item,
            };
            if count > 0 {
                stacks.push(ItemStack::with_damage(item, count, 0));
            }
        }
        stacks
    }
}

/// Knockback away from `from` for any body hit there (modern
/// `LivingEntity.knockback` with the usual 0.4).
pub fn knock_back(body: &mut Body, from: DVec3) {
    let away = body.position() - from;
    push(body, KNOCKBACK, DVec3::new(away.x, 0.0, away.z));
}

/// Modern `LivingEntity.knockback`: halves the horizontal speed and adds
/// `strength` along `toward` (horizontal; a zero vector pushes a hair east),
/// and lifts a body on the ground by `strength`, up to 0.4.
pub fn push(body: &mut Body, strength: f64, toward: DVec3) {
    let mut dir = DVec3::new(toward.x, 0.0, toward.z);
    if dir.length_squared() < 1e-8 {
        dir = DVec3::X;
    }
    let dir = dir.normalize() * strength;
    body.velocity.x = body.velocity.x / 2.0 + dir.x;
    body.velocity.z = body.velocity.z / 2.0 + dir.z;
    if body.on_ground {
        body.velocity.y = (body.velocity.y / 2.0 + strength).min(0.4);
    }
}

/// `a − b` wrapped into −π..π.
pub fn angle_difference(a: f32, b: f32) -> f32 {
    use std::f32::consts::{PI, TAU};
    (a - b + PI).rem_euclid(TAU) - PI
}
