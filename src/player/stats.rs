//! Health, hunger, air, fire and falling, following 1.0.0's `FoodStats`,
//! `EntityLiving.attackEntityFrom` and `Entity.onEntityUpdate`. The game
//! feeds in what happened each tick; these rules turn it into damage and
//! healing.

use serde::{Deserialize, Serialize};

pub const MAX_HEALTH: i32 = 20;
pub const MAX_FOOD: i32 = 20;
/// Saturation of a fresh player (1.0.0).
pub const START_SATURATION: f32 = 5.0;
/// Ticks of air: 15 seconds under water.
pub const MAX_AIR: i32 = 300;

/// Exhaustion that costs one saturation point (or one food point).
const EXHAUSTION_PER_POINT: f32 = 4.0;
/// Ticks between healing (food ≥ 18) or starving (food 0) by one point.
const FOOD_TIMER_TICKS: u32 = 80;
/// Food needed to heal over time.
const HEALING_FOOD: i32 = 18;
/// Starving stops at this much health on normal difficulty.
const STARVING_FLOOR: i32 = 1;
/// After a hit, the player can't be hurt again for 10 ticks, except by a
/// bigger hit, which only deals the difference (1.0.0's `heartsLife`).
const HURT_COOLDOWN: u32 = 20;
/// Falls longer than this hurt, one point per block more (1.0.0).
const SAFE_FALL: f32 = 3.0;
/// Air left when drowning starts to hurt, and how much it hurts.
const DROWNING_AIR: i32 = -20;
const DROWNING_DAMAGE: i32 = 2;
/// Touching lava hurts this much and sets the player burning for 15 s.
const LAVA_DAMAGE: i32 = 4;
const LAVA_FIRE_TICKS: i32 = 300;
/// Burning hurts this much once a second.
const FIRE_DAMAGE: i32 = 1;
/// Exhaustion added by taking damage (1.0.0's `DamageSource.hungerDamage`).
const DAMAGE_EXHAUSTION: f32 = 0.3;

/// What hurt the player, for the death message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DamageSource {
    Fall,
    Drowning,
    Lava,
    Fire,
    Starving,
    Mob,
    Arrow,
}

impl DamageSource {
    /// 1.0.0's death messages.
    pub fn death_message(self) -> &'static str {
        match self {
            DamageSource::Fall => "hit the ground too hard",
            DamageSource::Drowning => "drowned",
            DamageSource::Lava => "tried to swim in lava",
            DamageSource::Fire => "burned to death",
            DamageSource::Starving => "starved to death",
            DamageSource::Mob => "was slain",
            DamageSource::Arrow => "was shot",
        }
    }
}

/// What the player was in and doing this tick.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct Surroundings {
    pub on_ground: bool,
    /// Blocks moved down this tick (negative while rising).
    pub fell: f32,
    /// The body is in water or lava.
    pub in_water: bool,
    pub in_lava: bool,
    /// The eyes are under water.
    pub eyes_in_water: bool,
    /// Flying in creative, which never builds up a fall.
    pub flying: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Stats {
    pub health: i32,
    pub food: i32,
    pub saturation: f32,
    pub exhaustion: f32,
    food_timer: u32,
    pub air: i32,
    /// Ticks left burning.
    pub fire: i32,
    /// Blocks fallen since last standing on something.
    pub fall_distance: f32,
    /// Ticks left of the hurt cooldown, and the hit that started it.
    hurt_cooldown: u32,
    last_damage: i32,
    /// What last hurt the player.
    #[serde(skip)]
    pub last_source: Option<DamageSource>,
}

impl Default for Stats {
    fn default() -> Self {
        Self {
            health: MAX_HEALTH,
            food: MAX_FOOD,
            saturation: START_SATURATION,
            exhaustion: 0.0,
            food_timer: 0,
            air: MAX_AIR,
            fire: 0,
            fall_distance: 0.0,
            hurt_cooldown: 0,
            last_damage: 0,
            last_source: None,
        }
    }
}

impl Stats {
    pub fn is_dead(&self) -> bool {
        self.health <= 0
    }

    /// Sprinting needs more than 3 drumsticks (1.0.0).
    pub fn can_sprint(&self) -> bool {
        self.food > 6
    }

    pub fn can_eat(&self) -> bool {
        self.food < MAX_FOOD
    }

    /// Tiring actions: walking, jumping, mining, fighting, being hurt.
    pub fn add_exhaustion(&mut self, amount: f32) {
        self.exhaustion = (self.exhaustion + amount).min(40.0);
    }

    /// 1.0.0's `FoodStats.addStats`: food up to 20, saturation up to the
    /// food level.
    pub fn eat(&mut self, food: i32, saturation: f32) {
        self.food = (self.food + food).min(MAX_FOOD);
        self.saturation = (self.saturation + food as f32 * saturation * 2.0).min(self.food as f32);
    }

    pub fn heal(&mut self, amount: i32) {
        if !self.is_dead() {
            self.health = (self.health + amount).min(MAX_HEALTH);
        }
    }

    /// Stats for a mob with `health` points.
    pub fn with_health(health: i32) -> Self {
        Self {
            health,
            ..Self::default()
        }
    }

    /// Hurts the player, honoring the hurt cooldown. Returns true for a
    /// fresh hit (which knocks back and flashes), false if it was absorbed
    /// or only topped up the last one.
    pub fn damage(&mut self, amount: i32, source: DamageSource) -> bool {
        if self.is_dead() || amount <= 0 {
            return false;
        }
        let fresh = if self.hurt_cooldown > HURT_COOLDOWN / 2 {
            if amount <= self.last_damage {
                return false;
            }
            self.health -= amount - self.last_damage;
            false
        } else {
            self.hurt_cooldown = HURT_COOLDOWN;
            self.health -= amount;
            true
        };
        self.last_damage = amount;
        self.last_source = Some(source);
        self.add_exhaustion(DAMAGE_EXHAUSTION);
        fresh
    }

    /// One tick of falling, drowning and burning. `vulnerable` is false in
    /// creative: nothing hurts. Mobs use this too; hunger is `tick_food`.
    pub fn tick(&mut self, around: &Surroundings, vulnerable: bool) {
        self.hurt_cooldown = self.hurt_cooldown.saturating_sub(1);
        self.tick_fall(around, vulnerable);
        if around.eyes_in_water && vulnerable {
            self.air -= 1;
            if self.air <= DROWNING_AIR {
                self.air = 0;
                self.damage(DROWNING_DAMAGE, DamageSource::Drowning);
            }
        } else {
            self.air = MAX_AIR;
        }
        if around.in_water {
            self.fire = 0;
        }
        if around.in_lava && vulnerable {
            self.damage(LAVA_DAMAGE, DamageSource::Lava);
            self.fire = LAVA_FIRE_TICKS;
        }
        if self.fire > 0 {
            if self.fire % 20 == 0 && vulnerable {
                self.damage(FIRE_DAMAGE, DamageSource::Fire);
            }
            self.fire -= 1;
        }
    }

    /// 1.0.0's `updateFallState`: a fall hurts on landing; water, lava and
    /// flight cancel it.
    fn tick_fall(&mut self, around: &Surroundings, vulnerable: bool) {
        if around.in_water || around.flying {
            self.fall_distance = 0.0;
        } else if around.in_lava {
            self.fall_distance *= 0.5;
        }
        if around.on_ground {
            let damage = (self.fall_distance - SAFE_FALL).ceil() as i32;
            if damage > 0 && vulnerable {
                self.damage(damage, DamageSource::Fall);
            }
            self.fall_distance = 0.0;
        } else if around.fell > 0.0 {
            self.fall_distance += around.fell;
        }
    }

    /// A tick of hunger: 1.0.0's `FoodStats.onUpdate` on normal difficulty.
    pub fn tick_food(&mut self) {
        if self.exhaustion > EXHAUSTION_PER_POINT {
            self.exhaustion -= EXHAUSTION_PER_POINT;
            if self.saturation > 0.0 {
                self.saturation = (self.saturation - 1.0).max(0.0);
            } else {
                self.food = (self.food - 1).max(0);
            }
        }
        if self.food >= HEALING_FOOD && self.health < MAX_HEALTH {
            self.food_timer += 1;
            if self.food_timer >= FOOD_TIMER_TICKS {
                self.heal(1);
                self.food_timer = 0;
            }
        } else if self.food <= 0 {
            self.food_timer += 1;
            if self.food_timer >= FOOD_TIMER_TICKS {
                if self.health > STARVING_FLOOR {
                    self.damage(1, DamageSource::Starving);
                }
                self.food_timer = 0;
            }
        } else {
            self.food_timer = 0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GROUND: Surroundings = Surroundings {
        on_ground: true,
        fell: 0.0,
        in_water: false,
        in_lava: false,
        eyes_in_water: false,
        flying: false,
    };
    const AIR: Surroundings = Surroundings {
        on_ground: false,
        ..GROUND
    };

    fn fall(stats: &mut Stats, blocks: f32) {
        stats.tick(
            &Surroundings {
                fell: blocks,
                ..AIR
            },
            true,
        );
        stats.tick(&GROUND, true);
    }

    #[test]
    fn falls_hurt_one_point_per_block_past_three() {
        for (height, damage) in [(3.0, 0), (3.5, 1), (4.0, 1), (10.0, 7), (23.0, 20)] {
            let mut stats = Stats::default();
            fall(&mut stats, height);
            assert_eq!(MAX_HEALTH - stats.health, damage, "{height} blocks");
        }
        let mut stats = Stats::default();
        fall(&mut stats, 23.0);
        assert!(stats.is_dead());
        assert_eq!(stats.last_source, Some(DamageSource::Fall));
    }

    #[test]
    fn landing_in_water_or_flying_is_safe_and_creative_never_hurts() {
        let mut stats = Stats::default();
        stats.tick(&Surroundings { fell: 30.0, ..AIR }, true);
        stats.tick(
            &Surroundings {
                in_water: true,
                ..GROUND
            },
            true,
        );
        assert_eq!(stats.health, MAX_HEALTH);
        let mut creative = Stats::default();
        creative.tick(&Surroundings { fell: 30.0, ..AIR }, false);
        creative.tick(&GROUND, false);
        creative.tick(
            &Surroundings {
                in_lava: true,
                ..GROUND
            },
            false,
        );
        assert_eq!(creative.health, MAX_HEALTH);
    }

    #[test]
    fn drowning_starts_after_15_seconds_then_hurts_2_each_second() {
        let mut stats = Stats::default();
        let under = Surroundings {
            eyes_in_water: true,
            in_water: true,
            ..GROUND
        };
        for _ in 0..(MAX_AIR - DROWNING_AIR - 1) {
            stats.tick(&under, true);
        }
        assert_eq!(stats.health, MAX_HEALTH, "still holding breath");
        stats.tick(&under, true);
        assert_eq!(stats.health, MAX_HEALTH - 2);
        // Air refills at once at the surface.
        stats.tick(&GROUND, true);
        assert_eq!(stats.air, MAX_AIR);
    }

    #[test]
    fn lava_hurts_4_and_sets_you_on_fire_until_water() {
        let mut stats = Stats::default();
        stats.tick(
            &Surroundings {
                in_lava: true,
                ..GROUND
            },
            true,
        );
        assert_eq!(stats.health, MAX_HEALTH - 4);
        assert_eq!(stats.fire, LAVA_FIRE_TICKS - 1);
        // Out of the lava it keeps burning, 1 a second.
        for _ in 0..40 {
            stats.tick(&GROUND, true);
        }
        assert!(stats.health <= MAX_HEALTH - 5);
        stats.tick(
            &Surroundings {
                in_water: true,
                ..GROUND
            },
            true,
        );
        assert_eq!(stats.fire, 0);
    }

    #[test]
    fn the_hurt_cooldown_absorbs_smaller_hits_and_tops_up_bigger_ones() {
        let mut stats = Stats::default();
        assert!(stats.damage(4, DamageSource::Mob));
        assert!(!stats.damage(3, DamageSource::Mob), "smaller: absorbed");
        assert_eq!(stats.health, 16);
        assert!(
            !stats.damage(6, DamageSource::Mob),
            "bigger: only the difference"
        );
        assert_eq!(stats.health, 14);
        for _ in 0..10 {
            stats.tick(&GROUND, true);
        }
        assert!(
            stats.damage(1, DamageSource::Mob),
            "fresh hit after 10 ticks"
        );
    }

    #[test]
    fn food_heals_when_high_and_starves_when_empty() {
        let mut stats = Stats {
            health: 10,
            ..Stats::default()
        };
        for _ in 0..FOOD_TIMER_TICKS {
            stats.tick_food();
        }
        assert_eq!(stats.health, 11, "one point per 4 seconds at 18+ food");

        let mut starving = Stats {
            food: 0,
            saturation: 0.0,
            health: 3,
            ..Stats::default()
        };
        for _ in 0..FOOD_TIMER_TICKS * 5 {
            starving.tick(&GROUND, true);
            starving.tick_food();
        }
        assert_eq!(
            starving.health, STARVING_FLOOR,
            "normal difficulty stops at half a heart"
        );
    }

    #[test]
    fn exhaustion_eats_saturation_before_food_and_eating_refills_both() {
        let mut stats = Stats::default();
        stats.add_exhaustion(4.1);
        stats.tick_food();
        assert_eq!((stats.food, stats.saturation), (20, 4.0));
        stats.saturation = 0.0;
        stats.add_exhaustion(4.1);
        stats.tick_food();
        assert_eq!(stats.food, 19);
        // Cooked porkchop: 8 food, 0.8 saturation modifier.
        stats.food = 10;
        stats.eat(8, 0.8);
        assert_eq!(stats.food, 18);
        assert!((stats.saturation - 12.8).abs() < 1e-4);
        assert!(!Stats::default().can_eat());
        assert!(
            !Stats {
                food: 6,
                ..Stats::default()
            }
            .can_sprint()
        );
    }
}
