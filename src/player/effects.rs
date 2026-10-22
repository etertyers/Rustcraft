//! Status effects (M26, modern `MobEffect` and `MobEffectInstance`): what
//! potions, golden apples, some food and wither skeletons give the player
//! and mobs, each with a level (amplifier 0 is level I) and ticks left.
//! `Stats` keeps the active ones and runs those that act over time
//! (regeneration, poison, wither, hunger); the game reads the rest where
//! they matter (movement, damage dealt and taken, air, fire, light).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Effect {
    Speed,
    Slowness,
    Strength,
    InstantHealth,
    InstantDamage,
    JumpBoost,
    Regeneration,
    Resistance,
    FireResistance,
    WaterBreathing,
    Invisibility,
    NightVision,
    Weakness,
    Poison,
    Wither,
    Absorption,
    Hunger,
    SlowFalling,
    /// Lifts off the ground (shulker bullets, M28).
    Levitation,
}

impl Effect {
    pub const ALL: [Effect; 19] = [
        Effect::Speed,
        Effect::Slowness,
        Effect::Strength,
        Effect::InstantHealth,
        Effect::InstantDamage,
        Effect::JumpBoost,
        Effect::Regeneration,
        Effect::Resistance,
        Effect::FireResistance,
        Effect::WaterBreathing,
        Effect::Invisibility,
        Effect::NightVision,
        Effect::Weakness,
        Effect::Poison,
        Effect::Wither,
        Effect::Absorption,
        Effect::Hunger,
        Effect::SlowFalling,
        Effect::Levitation,
    ];

    /// Modern's name (`minecraft:fire_resistance`, less the namespace),
    /// which also names its icon (`mob_effect/fire_resistance`).
    pub fn id(self) -> &'static str {
        match self {
            Effect::Speed => "speed",
            Effect::Slowness => "slowness",
            Effect::Strength => "strength",
            Effect::InstantHealth => "instant_health",
            Effect::InstantDamage => "instant_damage",
            Effect::JumpBoost => "jump_boost",
            Effect::Regeneration => "regeneration",
            Effect::Resistance => "resistance",
            Effect::FireResistance => "fire_resistance",
            Effect::WaterBreathing => "water_breathing",
            Effect::Invisibility => "invisibility",
            Effect::NightVision => "night_vision",
            Effect::Weakness => "weakness",
            Effect::Poison => "poison",
            Effect::Wither => "wither",
            Effect::Absorption => "absorption",
            Effect::Hunger => "hunger",
            Effect::SlowFalling => "slow_falling",
            Effect::Levitation => "levitation",
        }
    }

    /// As shown: "Fire Resistance".
    pub fn name(self) -> &'static str {
        match self {
            Effect::Speed => "Speed",
            Effect::Slowness => "Slowness",
            Effect::Strength => "Strength",
            Effect::InstantHealth => "Instant Health",
            Effect::InstantDamage => "Instant Damage",
            Effect::JumpBoost => "Jump Boost",
            Effect::Regeneration => "Regeneration",
            Effect::Resistance => "Resistance",
            Effect::FireResistance => "Fire Resistance",
            Effect::WaterBreathing => "Water Breathing",
            Effect::Invisibility => "Invisibility",
            Effect::NightVision => "Night Vision",
            Effect::Weakness => "Weakness",
            Effect::Poison => "Poison",
            Effect::Wither => "Wither",
            Effect::Absorption => "Absorption",
            Effect::Hunger => "Hunger",
            Effect::SlowFalling => "Slow Falling",
            Effect::Levitation => "Levitation",
        }
    }

    /// Its color (sRGB), for particles and potions (modern values).
    pub fn color(self) -> u32 {
        match self {
            Effect::Speed => 0x33EBFF,
            Effect::Slowness => 0x8BAFE0,
            Effect::Strength => 0xFFC700,
            Effect::InstantHealth => 0xF82423,
            Effect::InstantDamage => 0xA9656A,
            Effect::JumpBoost => 0xFDFF84,
            Effect::Regeneration => 0xCD5CAB,
            Effect::Resistance => 0x9146F0,
            Effect::FireResistance => 0xFF9900,
            Effect::WaterBreathing => 0x98DAC0,
            Effect::Invisibility => 0xF6F6F6,
            Effect::NightVision => 0xC2FF66,
            Effect::Weakness => 0x484D48,
            Effect::Poison => 0x87A363,
            Effect::Wither => 0x736156,
            Effect::Absorption => 0x2552A5,
            Effect::Hunger => 0x587653,
            Effect::SlowFalling => 0xF3CFB9,
            Effect::Levitation => 0xCEFFFF,
        }
    }

    /// Acts once, when given (healing and harming).
    pub fn instant(self) -> bool {
        matches!(self, Effect::InstantHealth | Effect::InstantDamage)
    }

    /// Where its icon is in the 1.12 inventory sheet's effect grid (18
    /// pixels apart from (0, 198)), for packs without modern's
    /// `mob_effect/` icons.
    pub fn legacy_icon(self) -> Option<(u32, u32)> {
        Some(match self {
            Effect::Speed => (0, 0),
            Effect::Slowness => (1, 0),
            Effect::Strength => (4, 0),
            Effect::Weakness => (5, 0),
            Effect::Poison => (6, 0),
            Effect::Regeneration => (7, 0),
            Effect::Invisibility => (0, 1),
            Effect::Hunger => (1, 1),
            Effect::JumpBoost => (2, 1),
            Effect::NightVision => (4, 1),
            Effect::Resistance => (6, 1),
            Effect::FireResistance => (7, 1),
            Effect::WaterBreathing => (0, 2),
            Effect::Wither => (1, 2),
            Effect::Absorption => (2, 2),
            _ => return None,
        })
    }
}

/// An effect working on someone: level (0 is I) and ticks left.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Active {
    pub effect: Effect,
    pub amplifier: u8,
    pub duration: u32,
}

impl Active {
    pub fn new(effect: Effect, amplifier: u8, duration: u32) -> Self {
        Self {
            effect,
            amplifier,
            duration,
        }
    }

    /// "Fire Resistance", "Strength II".
    pub fn label(&self) -> String {
        const NUMERALS: [&str; 5] = ["", " II", " III", " IV", " V"];
        let level = NUMERALS.get(self.amplifier as usize).copied().unwrap_or("");
        format!("{}{level}", self.effect.name())
    }

    /// "3:00", or "**:**" past 99 minutes.
    pub fn timer(&self) -> String {
        timer(self.duration)
    }

    /// Whether a periodic effect acts this tick (modern
    /// `isDurationEffectTick`): every `base` ticks, twice as often a level.
    fn acts_now(&self, base: u32) -> bool {
        let every = (base >> self.amplifier).max(1);
        self.duration.is_multiple_of(every)
    }
}

/// Minutes and seconds of `ticks`.
pub fn timer(ticks: u32) -> String {
    let seconds = ticks / 20;
    if seconds >= 100 * 60 {
        "**:**".to_owned()
    } else {
        format!("{}:{:02}", seconds / 60, seconds % 60)
    }
}

/// What an effect does to its bearer this tick.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct Tick {
    pub heal: f32,
    pub poison: f32,
    pub wither: f32,
    pub exhaustion: f32,
}

/// Regeneration heals every 50 ticks, poison hurts every 25, wither every
/// 40 (each twice as often a level), hunger tires 0.005 a tick a level.
const REGENERATION_EVERY: u32 = 50;
const POISON_EVERY: u32 = 25;
const WITHER_EVERY: u32 = 40;
const HUNGER_PER_TICK: f32 = 0.005;

/// The effects on someone.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Effects(pub Vec<Active>);

impl Effects {
    /// Gives `new`, keeping whichever of it and one already there is
    /// stronger, or for the same level, lasts longer (modern `update`).
    pub fn add(&mut self, new: Active) {
        match self.0.iter_mut().find(|a| a.effect == new.effect) {
            Some(old) => {
                if new.amplifier > old.amplifier
                    || (new.amplifier == old.amplifier && new.duration > old.duration)
                {
                    *old = new;
                }
            }
            None => self.0.push(new),
        }
    }

    pub fn get(&self, effect: Effect) -> Option<&Active> {
        self.0.iter().find(|a| a.effect == effect)
    }

    pub fn has(&self, effect: Effect) -> bool {
        self.get(effect).is_some()
    }

    /// The level (0 is I) of `effect`, if active.
    pub fn amplifier(&self, effect: Effect) -> Option<u8> {
        self.get(effect).map(|a| a.amplifier)
    }

    pub fn clear(&mut self) {
        self.0.clear();
    }

    pub fn remove(&mut self, effect: Effect) {
        self.0.retain(|a| a.effect != effect);
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// One tick: what the periodic effects do now, and every effect a tick
    /// nearer its end.
    pub fn tick(&mut self) -> Tick {
        let mut out = Tick::default();
        for a in &self.0 {
            match a.effect {
                Effect::Regeneration if a.acts_now(REGENERATION_EVERY) => out.heal += 1.0,
                Effect::Poison if a.acts_now(POISON_EVERY) => out.poison += 1.0,
                Effect::Wither if a.acts_now(WITHER_EVERY) => out.wither += 1.0,
                Effect::Hunger => out.exhaustion += HUNGER_PER_TICK * (a.amplifier as f32 + 1.0),
                _ => {}
            }
        }
        for a in &mut self.0 {
            a.duration = a.duration.saturating_sub(1);
        }
        self.0.retain(|a| a.duration > 0);
        out
    }

    /// Movement speed scaled by Speed (+20% a level) and Slowness (−15% a
    /// level).
    pub fn speed_factor(&self) -> f64 {
        let up = self
            .amplifier(Effect::Speed)
            .map_or(0.0, |a| 0.2 * (f64::from(a) + 1.0));
        let down = self
            .amplifier(Effect::Slowness)
            .map_or(0.0, |a| 0.15 * (f64::from(a) + 1.0));
        ((1.0 + up) * (1.0 - down)).max(0.0)
    }

    /// Damage added to a melee hit: Strength +3 a level, Weakness −4.
    pub fn attack_bonus(&self) -> f32 {
        let strength = self
            .amplifier(Effect::Strength)
            .map_or(0.0, |a| 3.0 * (f32::from(a) + 1.0));
        let weakness = self
            .amplifier(Effect::Weakness)
            .map_or(0.0, |a| 4.0 * (f32::from(a) + 1.0));
        strength - weakness
    }

    /// Extra jump speed from Jump Boost (0.1 a level), and the blocks of
    /// fall it forgives (one a level).
    pub fn jump_boost(&self) -> (f64, f32) {
        self.amplifier(Effect::JumpBoost).map_or((0.0, 0.0), |a| {
            let level = f64::from(a) + 1.0;
            (0.1 * level, level as f32)
        })
    }

    /// The share of damage Resistance takes away (20% a level).
    pub fn resistance(&self) -> f32 {
        self.amplifier(Effect::Resistance)
            .map_or(0.0, |a| (0.2 * (f32::from(a) + 1.0)).min(1.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stronger_or_longer_effect_replaces_the_old_one() {
        let mut fx = Effects::default();
        fx.add(Active::new(Effect::Speed, 0, 3600));
        fx.add(Active::new(Effect::Speed, 0, 100));
        assert_eq!(fx.get(Effect::Speed).unwrap().duration, 3600);
        fx.add(Active::new(Effect::Speed, 1, 100));
        assert_eq!(fx.get(Effect::Speed).unwrap().amplifier, 1);
        assert_eq!(fx.0.len(), 1);
    }

    #[test]
    fn periodic_effects_act_on_modern_intervals_and_run_out() {
        let mut fx = Effects::default();
        fx.add(Active::new(Effect::Regeneration, 0, 100));
        fx.add(Active::new(Effect::Poison, 1, 100));
        let (mut heal, mut poison) = (0.0, 0.0);
        for _ in 0..100 {
            let t = fx.tick();
            heal += t.heal;
            poison += t.poison;
        }
        // Regeneration I every 50 ticks, Poison II every 12.
        assert_eq!(heal, 2.0);
        assert_eq!(poison, 8.0);
        assert!(fx.is_empty());
    }

    #[test]
    fn speed_strength_and_jump_follow_modern_values() {
        let mut fx = Effects::default();
        fx.add(Active::new(Effect::Speed, 1, 100));
        assert!((fx.speed_factor() - 1.4).abs() < 1e-9);
        fx.add(Active::new(Effect::Slowness, 0, 100));
        assert!((fx.speed_factor() - 1.4 * 0.85).abs() < 1e-9);
        fx.add(Active::new(Effect::Strength, 0, 100));
        assert_eq!(fx.attack_bonus(), 3.0);
        fx.add(Active::new(Effect::JumpBoost, 1, 100));
        assert_eq!(fx.jump_boost(), (0.2, 2.0));
        assert_eq!(Active::new(Effect::FireResistance, 0, 3600).timer(), "3:00");
        assert_eq!(Active::new(Effect::Strength, 1, 1).label(), "Strength II");
    }
}
