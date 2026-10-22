//! The ender dragon (M27, after modern `EnderDragon` and its phases).
//!
//! It circles the island in a holding pattern, sometimes strafing the
//! player with fireballs, and with no crystals left lands on the fountain
//! to breathe and bite before taking off again. Crystals within 32 blocks
//! heal it; it dies in a 200-tick blaze of light after its health runs out.
//!
//! This is the pure part: where it flies and what it wants to do. The game
//! (`game::dragon`) carries out what it does to the world.

use std::collections::VecDeque;

use glam::DVec3;
use serde::{Deserialize, Serialize};

use crate::{player::collision::Aabb, world::worldgen::rng::Rng};

pub const MAX_HEALTH: f32 = 200.0;
/// Experience for the first kill and every one after (modern).
pub const FIRST_KILL_XP: u32 = 12_000;
pub const REPEAT_KILL_XP: u32 = 500;
/// Ticks the death takes.
pub const DEATH_TICKS: u32 = 200;
/// A dying dragon flies at this speed to the exit portal, and starts dying
/// where it is if it hasn't got there in 10 seconds.
const DEATH_FLIGHT_SPEED: f64 = 0.9;
const DEATH_FLIGHT_LIMIT: u32 = 200;
/// It has arrived this near (sideways, and up or down).
const DEATH_ARRIVED: (f64, f64) = (2.0, 4.0);
/// While dying it rises this much a tick and turns this much (radians).
const DEATH_RISE: f64 = 0.1;
const DEATH_SPIN: f32 = 0.12;
/// Bursts of light break out round it from this tick on.
pub const DEATH_BURSTS_FROM: u32 = 180;
/// Experience orbs drop from this tick on (every fifth).
pub const DEATH_XP_FROM: u32 = 154;
/// Ticks between the heals a crystal gives, and how far away one reaches.
pub const HEAL_EVERY: u32 = 10;
pub const HEAL_REACH: f64 = 32.0;
/// Sitting, a quarter of its health in damage sends it flying again.
const SIT_DAMAGE_LIMIT: f32 = MAX_HEALTH * 0.25;
/// Ticks of positions and headings kept for the tail to follow.
const HISTORY: usize = 64;

/// Flight, in blocks a tick.
const FLY_SPEED: f64 = 0.5;
const STRAFE_SPEED: f64 = 0.6;
const CHARGE_SPEED: f64 = 0.95;
/// How fast it can turn, radians a tick.
const TURN: f32 = 0.06;
/// How quickly its velocity follows where it wants to go.
const EASE: f64 = 0.1;

/// Where the holding pattern runs: radii, and heights above the fountain.
const RING: [f64; 2] = [48.0, 62.0];
const ORBIT_STEP: f64 = std::f64::consts::TAU / 12.0;
const ARRIVED: f64 = 12.0;
const ORBIT_BASE: f64 = 14.0;
const ORBIT_SWING: f64 = 24.0;

/// Where its body sits over the fountain's top while perched.
const PERCH_RISE: f64 = 5.0;
/// Ticks perched before it flies off, and the stages of sitting.
const PERCH_TICKS: u32 = 600;
const SCAN_TICKS: u32 = 100;
const FLAME_TICKS: u32 = 100;
const FLAME_AT: u32 = 60;
const BITE_TICKS: u32 = 40;
/// The player is bitten from this close while it sits.
pub const BITE_REACH: f64 = 12.0;

/// Fireballs: the windup, the distance it shoots from, how long a pass lasts.
const WINDUP: u32 = 12;
const SHOOT_RANGE: f64 = 70.0;
const STRAFE_TICKS: u32 = 140;
pub const FIREBALL_SPEED: f64 = 0.9;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Phase {
    /// Circling the island.
    HoldingPattern,
    /// Flying at the player, shooting fireballs.
    StrafePlayer,
    /// Heading for the fountain.
    LandingApproach,
    /// Coming down onto it.
    Landing,
    /// Sitting on it, breathing.
    SittingFlaming,
    /// Sitting, looking round.
    SittingScanning,
    /// Sitting, biting at what comes near.
    SittingAttacking,
    /// Lifting off the fountain.
    Takeoff,
    /// Flying straight at where the player was.
    ChargingPlayer,
    /// Out of health: flying to the point over the exit portal, still
    /// dangerous to survival players on the way.
    DeathFlight,
    /// The 200-tick death sequence: rising, spinning, beams, bursts.
    Dying,
}

impl Phase {
    pub fn is_sitting(self) -> bool {
        matches!(
            self,
            Phase::SittingFlaming | Phase::SittingScanning | Phase::SittingAttacking
        )
    }
}

/// What the dragon can tell about the world this tick.
#[derive(Debug, Clone, Copy)]
pub struct Senses {
    /// Where the player's feet are, if they're in the End and alive.
    pub player: Option<DVec3>,
    /// How many crystals are left.
    pub crystals: usize,
    /// The top of the fountain, where the portal level is.
    pub podium: DVec3,
    /// The point above the highest block of the exit portal's column, where
    /// the dragon dies.
    pub exit: DVec3,
}

/// What the dragon does that the world has to carry out.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Act {
    /// A fireball shot from `from` along `dir`.
    Fireball { from: DVec3, dir: DVec3 },
    /// A cloud of breath laid at `at`.
    Breath { at: DVec3 },
    /// A bite at `at`, reaching `BITE_REACH`.
    Bite { at: DVec3 },
    /// A burst of light somewhere on a dying dragon.
    Burst { at: DVec3 },
    /// The last tick of dying: it's gone.
    Died,
}

/// The parts it can be hit on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Part {
    Head,
    Neck,
    Body,
    Tail,
    Wing,
}

/// A dragon in the game, as saved.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SavedDragon {
    pub position: [f64; 3],
    pub yaw: f32,
    pub health: f32,
    pub phase: Phase,
}

#[derive(Debug, Clone)]
pub struct EnderDragon {
    pub position: DVec3,
    pub prev_position: DVec3,
    /// Radians, the way the camera counts: it faces (sin, 0, −cos).
    pub yaw: f32,
    pub prev_yaw: f32,
    pub pitch: f32,
    pub velocity: DVec3,
    pub health: f32,
    pub phase: Phase,
    /// Ticks in this phase.
    pub phase_ticks: u32,
    /// Ticks since it landed.
    pub perched: u32,
    /// Where it flies to, and where it is on its ring.
    target: DVec3,
    orbit: f64,
    arrivals: u32,
    /// Fireball windup, and whether this pass has shot yet.
    windup: u32,
    shot: bool,
    sitting_damage: f32,
    pub death_ticks: u32,
    /// Ticks since it was last hurt, for the red flash.
    pub hurt_ticks: u32,
    pub age: u32,
    /// Where it has been, newest first.
    history: VecDeque<(DVec3, f32)>,
    rng: Rng,
}

impl EnderDragon {
    /// A healthy dragon at `at`, starting its first lap.
    pub fn new(at: DVec3, yaw: f32, seed: u64) -> Self {
        let mut dragon = Self {
            position: at,
            prev_position: at,
            yaw,
            prev_yaw: yaw,
            pitch: 0.0,
            velocity: DVec3::ZERO,
            health: MAX_HEALTH,
            phase: Phase::HoldingPattern,
            phase_ticks: 0,
            perched: 0,
            target: at,
            orbit: f64::from(yaw),
            arrivals: 0,
            windup: 0,
            shot: false,
            sitting_damage: 0.0,
            death_ticks: 0,
            hurt_ticks: 0,
            age: 0,
            history: VecDeque::new(),
            rng: Rng::new(seed),
        };
        dragon.history.push_back((at, yaw));
        dragon
    }

    pub fn restore(saved: SavedDragon, seed: u64) -> Self {
        let mut dragon = Self::new(DVec3::from_array(saved.position), saved.yaw, seed);
        dragon.health = saved.health;
        dragon.phase = if saved.phase == Phase::Dying {
            Phase::Dying
        } else {
            Phase::HoldingPattern
        };
        dragon
    }

    pub fn save(&self) -> SavedDragon {
        SavedDragon {
            position: self.position.to_array(),
            yaw: self.yaw,
            health: self.health,
            phase: self.phase,
        }
    }

    /// Out of health: on its way to die or dying.
    pub fn is_dying(&self) -> bool {
        matches!(self.phase, Phase::DeathFlight | Phase::Dying)
    }

    /// In the 200-tick death sequence itself.
    pub fn in_death_sequence(&self) -> bool {
        self.phase == Phase::Dying
    }

    /// The way it faces.
    pub fn facing(&self) -> DVec3 {
        facing(self.yaw)
    }

    /// Where its mouth is.
    pub fn head(&self) -> DVec3 {
        self.position + self.facing() * 8.0 + DVec3::Y * 1.0
    }

    /// Whether its jaws are open: breathing, or about to shoot.
    pub fn mouth_open(&self) -> bool {
        self.phase == Phase::SittingFlaming
            || (self.phase == Phase::StrafePlayer && self.windup > 0)
            || self.phase == Phase::SittingAttacking
    }

    /// Hurts it. Hits anywhere but the head deal a quarter and a point at
    /// most; it only takes blows from the player's hand, arrows and
    /// crystals. Returns whether it was hurt.
    pub fn hurt(&mut self, part: Part, amount: f32) -> bool {
        if self.is_dying() {
            return false;
        }
        let amount = if part == Part::Head {
            amount
        } else {
            amount / 4.0 + amount.min(1.0)
        };
        if amount < 0.01 {
            return false;
        }
        self.health -= amount;
        self.hurt_ticks = 10;
        if self.health <= 0.0 {
            self.health = 0.0;
            self.set_phase(Phase::DeathFlight);
            return true;
        }
        if self.phase.is_sitting() {
            self.sitting_damage += amount;
            if self.sitting_damage > SIT_DAMAGE_LIMIT {
                self.sitting_damage = 0.0;
                self.set_phase(Phase::Takeoff);
            }
        }
        true
    }

    /// A crystal's heal.
    pub fn heal(&mut self, amount: f32) {
        if !self.is_dying() {
            self.health = (self.health + amount).min(MAX_HEALTH);
        }
    }

    fn set_phase(&mut self, phase: Phase) {
        self.phase = phase;
        self.phase_ticks = 0;
        self.windup = 0;
        self.shot = false;
        if phase.is_sitting() && !matches!(phase, Phase::SittingScanning) {
            // Stays perched; the counters carry on.
        }
    }

    fn roll(&mut self) -> f64 {
        self.rng.next_f64()
    }

    /// One tick; what it did that the world must carry out.
    pub fn tick(&mut self, senses: &Senses) -> Vec<Act> {
        let mut acts = Vec::new();
        self.prev_position = self.position;
        self.prev_yaw = self.yaw;
        self.age += 1;
        self.phase_ticks += 1;
        self.hurt_ticks = self.hurt_ticks.saturating_sub(1);
        match self.phase {
            Phase::HoldingPattern => self.hold(senses),
            Phase::StrafePlayer => self.strafe(senses, &mut acts),
            Phase::LandingApproach => self.approach(senses),
            Phase::Landing => self.land(senses),
            Phase::SittingScanning => self.scan(senses),
            Phase::SittingFlaming => self.flame(senses, &mut acts),
            Phase::SittingAttacking => self.bite(senses, &mut acts),
            Phase::Takeoff => self.take_off(senses),
            Phase::ChargingPlayer => self.charge(senses),
            Phase::DeathFlight => self.death_flight(senses),
            Phase::Dying => self.die(&mut acts),
        }
        if self.phase.is_sitting() {
            self.perched += 1;
        } else if !matches!(self.phase, Phase::Landing) {
            self.perched = 0;
        }
        self.history.push_front((self.position, self.yaw));
        self.history.truncate(HISTORY);
        acts
    }

    /// Eases the velocity toward `speed` along the way to `target`, and
    /// turns to face the way it goes.
    fn steer(&mut self, target: DVec3, speed: f64) {
        let to = target - self.position;
        let dir = to.normalize_or_zero();
        self.velocity += (dir * speed - self.velocity) * EASE;
        self.position += self.velocity;
        let flat = self.velocity.x.hypot(self.velocity.z);
        if flat > 0.05 {
            let goal = self.velocity.x.atan2(-self.velocity.z) as f32;
            self.yaw += angle_between(self.yaw, goal).clamp(-TURN, TURN);
            self.pitch = self.velocity.y.atan2(flat) as f32;
        }
    }

    /// Turns on the spot toward `at`.
    fn face(&mut self, at: DVec3) {
        let to = at - self.position;
        if to.x.hypot(to.z) > 0.5 {
            let goal = to.x.atan2(-to.z) as f32;
            self.yaw += angle_between(self.yaw, goal).clamp(-TURN, TURN);
        }
    }

    fn podium_perch(senses: &Senses) -> DVec3 {
        senses.podium + DVec3::Y * PERCH_RISE
    }

    // ---- the phases ----

    fn ring_point(&self, senses: &Senses) -> DVec3 {
        let r = RING[(self.arrivals % 2) as usize];
        let rise = ORBIT_BASE + ORBIT_SWING * (0.5 + 0.5 * (self.orbit * 1.7).sin());
        DVec3::new(
            senses.podium.x + self.orbit.cos() * r,
            senses.podium.y + rise + 8.0,
            senses.podium.z + self.orbit.sin() * r,
        )
    }

    fn hold(&mut self, senses: &Senses) {
        if self.phase_ticks == 1 || self.target.distance(self.position) < 1e-6 {
            self.target = self.ring_point(senses);
        }
        self.steer(self.target, FLY_SPEED);
        let flat = (self.target.x - self.position.x).hypot(self.target.z - self.position.z);
        if flat >= ARRIVED {
            return;
        }
        self.arrivals += 1;
        self.orbit += ORBIT_STEP;
        // Modern `DragonHoldingPatternPhase`: with no crystals it lands one
        // arrival in eight; otherwise one in (crystals + 3) strafes.
        if senses.crystals == 0 && self.roll() < 1.0 / 8.0 {
            self.set_phase(Phase::LandingApproach);
            return;
        }
        if senses.player.is_some() && self.roll() < 1.0 / (senses.crystals as f64 + 3.0) {
            self.set_phase(Phase::StrafePlayer);
            return;
        }
        self.target = self.ring_point(senses);
    }

    fn strafe(&mut self, senses: &Senses, acts: &mut Vec<Act>) {
        let Some(player) = senses.player else {
            self.back_to_pattern(senses);
            return;
        };
        let aim = player + DVec3::Y * 1.6;
        // Flies at a point above the player, so it passes over.
        self.steer(aim + DVec3::Y * 6.0, STRAFE_SPEED);
        let head = self.head();
        let flat = (aim.x - self.position.x).hypot(aim.z - self.position.z);
        let ahead = (aim - self.position).normalize_or_zero().dot(self.facing()) > 0.6;
        if !self.shot && flat < SHOOT_RANGE && ahead {
            self.windup += 1;
            if self.windup >= WINDUP {
                let dir = (aim - head).normalize_or_zero();
                acts.push(Act::Fireball { from: head, dir });
                self.shot = true;
                self.windup = 0;
            }
        }
        if (self.shot && flat < 16.0) || self.phase_ticks > STRAFE_TICKS {
            self.back_to_pattern(senses);
        }
    }

    fn back_to_pattern(&mut self, senses: &Senses) {
        self.set_phase(Phase::HoldingPattern);
        self.arrivals += 1;
        self.target = self.ring_point(senses);
    }

    fn approach(&mut self, senses: &Senses) {
        // Heads for a point 30 blocks out over the island, level with the
        // fountain's top, on the side it's flying from.
        if self.phase_ticks == 1 {
            let from = self.position - senses.podium;
            let away = DVec3::new(from.x, 0.0, from.z).normalize_or_zero();
            self.target = senses.podium + away * 30.0 + DVec3::Y * 14.0;
        }
        self.steer(self.target, FLY_SPEED);
        if self.position.distance(self.target) < 8.0 || self.phase_ticks > 200 {
            self.set_phase(Phase::Landing);
        }
    }

    fn land(&mut self, senses: &Senses) {
        let perch = Self::podium_perch(senses);
        let left = perch - self.position;
        let speed = (left.length() * 0.12 + 0.08).min(FLY_SPEED);
        self.steer(perch, speed);
        if left.length() < 1.5 || self.phase_ticks > 300 {
            self.position = perch;
            self.velocity = DVec3::ZERO;
            self.pitch = 0.0;
            self.set_phase(Phase::SittingScanning);
        }
    }

    fn sit_still(&mut self, senses: &Senses) {
        self.velocity = DVec3::ZERO;
        self.pitch = 0.0;
        self.position = Self::podium_perch(senses);
        if let Some(player) = senses.player {
            self.face(player);
        }
    }

    fn scan(&mut self, senses: &Senses) {
        self.sit_still(senses);
        if self.perched >= PERCH_TICKS {
            self.set_phase(Phase::Takeoff);
            return;
        }
        let near = senses
            .player
            .is_some_and(|p| (p.x - self.position.x).hypot(p.z - self.position.z) < BITE_REACH);
        if near {
            self.set_phase(Phase::SittingAttacking);
        } else if self.phase_ticks >= SCAN_TICKS {
            self.set_phase(Phase::SittingFlaming);
        }
    }

    fn flame(&mut self, senses: &Senses, acts: &mut Vec<Act>) {
        self.sit_still(senses);
        if self.phase_ticks == FLAME_AT {
            let ahead = self.position + self.facing() * 10.0;
            let at = DVec3::new(ahead.x, senses.podium.y + 1.0, ahead.z);
            acts.push(Act::Breath { at });
        }
        if self.phase_ticks >= FLAME_TICKS {
            self.set_phase(Phase::SittingScanning);
        }
    }

    fn bite(&mut self, senses: &Senses, acts: &mut Vec<Act>) {
        self.sit_still(senses);
        if self.phase_ticks == 10 || self.phase_ticks == 25 {
            acts.push(Act::Bite { at: self.head() });
        }
        if self.phase_ticks >= BITE_TICKS {
            self.set_phase(Phase::SittingScanning);
        }
    }

    fn take_off(&mut self, senses: &Senses) {
        if self.phase_ticks == 1 {
            let side = self.facing();
            self.target = senses.podium + side * 20.0 + DVec3::Y * 40.0;
        }
        self.steer(self.target, FLY_SPEED);
        if self.position.y > senses.podium.y + 30.0 || self.phase_ticks > 120 {
            if self.roll() < 1.0 / 3.0 && senses.player.is_some() {
                self.set_phase(Phase::ChargingPlayer);
            } else {
                self.back_to_pattern(senses);
            }
        }
    }

    fn charge(&mut self, senses: &Senses) {
        if self.phase_ticks == 1 {
            self.target = senses.player.unwrap_or(senses.podium) + DVec3::Y * 1.0;
        }
        self.steer(self.target, CHARGE_SPEED);
        if self.position.distance(self.target) < 10.0 || self.phase_ticks > 60 {
            self.back_to_pattern(senses);
        }
    }

    /// Flies straight to the point over the exit portal, then starts the
    /// death sequence there (or where it is, if that takes over 10 seconds).
    fn death_flight(&mut self, senses: &Senses) {
        self.steer(senses.exit, DEATH_FLIGHT_SPEED);
        let to = senses.exit - self.position;
        let there = to.x.hypot(to.z) < DEATH_ARRIVED.0 && to.y.abs() < DEATH_ARRIVED.1;
        if there || self.phase_ticks > DEATH_FLIGHT_LIMIT {
            self.velocity = DVec3::ZERO;
            self.set_phase(Phase::Dying);
        }
    }

    fn die(&mut self, acts: &mut Vec<Act>) {
        self.death_ticks += 1;
        // Rising and spinning.
        self.position.y += DEATH_RISE;
        self.yaw += DEATH_SPIN;
        // Bursts of light round its body over the last twenty ticks.
        if self.death_ticks >= DEATH_BURSTS_FROM {
            let spread = DVec3::new(
                self.roll() * 2.0 - 1.0,
                self.roll() * 2.0 - 1.0,
                self.roll() * 2.0 - 1.0,
            ) * 4.0;
            acts.push(Act::Burst {
                at: self.position + spread,
            });
        }
        if self.death_ticks >= DEATH_TICKS {
            acts.push(Act::Died);
        }
    }

    // ---- where its parts are ----

    /// Where it was `lag` ticks ago and the way it faced.
    fn then(&self, lag: usize) -> (DVec3, f32) {
        self.history
            .get(lag)
            .or(self.history.back())
            .copied()
            .unwrap_or((self.position, self.yaw))
    }

    /// The hitboxes of its parts (modern `DragonPart`s, with a bigger head
    /// than modern's 1 × 1 block).
    pub fn parts(&self) -> Vec<(Part, Aabb)> {
        let boxed = |center: DVec3, w: f64, h: f64| {
            Aabb::new(
                center - DVec3::new(w / 2.0, h / 2.0, w / 2.0),
                center + DVec3::new(w / 2.0, h / 2.0, w / 2.0),
            )
        };
        let f = self.facing();
        let right = DVec3::new(f64::from(self.yaw).cos(), 0.0, f64::from(self.yaw).sin());
        let lift = DVec3::Y * f64::from(self.pitch).sin() * 3.0;
        let mut parts = vec![
            (
                Part::Head,
                boxed(self.position + f * 8.0 + DVec3::Y + lift, 2.5, 2.5),
            ),
            (
                Part::Neck,
                boxed(
                    self.position + f * 5.0 + DVec3::Y * 0.5 + lift * 0.6,
                    4.0,
                    4.0,
                ),
            ),
            (Part::Body, boxed(self.position, 7.0, 7.0)),
        ];
        for i in 1..=3usize {
            let (at, yaw) = self.then(i * 4);
            let behind = facing(yaw) * -(2.5 * i as f64);
            parts.push((Part::Tail, boxed(at + behind, 3.0, 3.0)));
        }
        for side in [-1.0, 1.0] {
            parts.push((
                Part::Wing,
                boxed(
                    self.position + right * side * 9.0 + DVec3::Y * 1.5,
                    7.0,
                    2.0,
                ),
            ));
        }
        parts
    }

    /// The first part a ray from `origin` along `dir` meets within `reach`,
    /// and how far along.
    pub fn ray_hit(&self, origin: DVec3, dir: DVec3, reach: f64) -> Option<(Part, f64)> {
        self.parts()
            .into_iter()
            .filter_map(|(part, bb)| Some((part, bb.ray_hit(origin, dir, reach)?)))
            .min_by(|a, b| a.1.total_cmp(&b.1))
    }

    pub fn position_between_ticks(&self, alpha: f64) -> DVec3 {
        self.prev_position.lerp(self.position, alpha)
    }

    pub fn yaw_between_ticks(&self, alpha: f32) -> f32 {
        self.prev_yaw + angle_between(self.prev_yaw, self.yaw) * alpha
    }

    /// The dragon's position and heading `lag` ticks ago, for drawing its
    /// neck and tail along its path.
    pub fn trail(&self, lag: usize) -> (DVec3, f32) {
        self.then(lag)
    }

    /// Experience for the kill.
    pub fn kill_xp(first: bool) -> u32 {
        if first { FIRST_KILL_XP } else { REPEAT_KILL_XP }
    }
}

/// The unit vector a heading `yaw` faces.
pub fn facing(yaw: f32) -> DVec3 {
    let yaw = f64::from(yaw);
    DVec3::new(yaw.sin(), 0.0, -yaw.cos())
}

/// The shortest turn from heading `from` to `to`.
pub fn angle_between(from: f32, to: f32) -> f32 {
    let tau = std::f32::consts::TAU;
    let mut d = (to - from) % tau;
    if d > std::f32::consts::PI {
        d -= tau;
    } else if d < -std::f32::consts::PI {
        d += tau;
    }
    d
}

#[cfg(test)]
mod tests {
    use super::*;

    const PODIUM: DVec3 = DVec3::new(0.0, 64.0, 0.0);
    const EXIT: DVec3 = DVec3::new(0.5, 70.0, 0.5);

    fn senses(player: Option<DVec3>, crystals: usize) -> Senses {
        Senses {
            player,
            crystals,
            podium: PODIUM,
            exit: EXIT,
        }
    }

    fn start() -> EnderDragon {
        EnderDragon::new(DVec3::new(0.0, 128.0, 0.0), 0.0, 7)
    }

    #[test]
    fn it_circles_the_island_at_a_steady_height_and_never_leaves_it() {
        let mut dragon = start();
        let mut angles = std::collections::BTreeSet::new();
        for _ in 0..4000 {
            dragon.tick(&senses(None, 10));
            assert!(dragon.position.is_finite());
            let flat = dragon.position.x.hypot(dragon.position.z);
            assert!(flat < 90.0, "{flat}");
            assert!(
                (60.0..130.0).contains(&dragon.position.y),
                "{}",
                dragon.position.y
            );
            angles.insert(((dragon.position.z.atan2(dragon.position.x) + 4.0) * 2.0) as i32);
        }
        assert_eq!(
            dragon.phase,
            Phase::HoldingPattern,
            "no player: no strafing"
        );
        assert!(
            angles.len() >= 10,
            "went all the way round: {}",
            angles.len()
        );
    }

    #[test]
    fn with_a_player_about_it_strafes_and_fires_toward_them() {
        let mut dragon = start();
        let player = DVec3::new(20.0, 64.0, 0.0);
        let mut fired = None;
        let mut strafed = false;
        for _ in 0..6000 {
            for act in dragon.tick(&senses(Some(player), 2)) {
                if let Act::Fireball { from, dir } = act {
                    fired.get_or_insert((from, dir));
                }
            }
            strafed |= dragon.phase == Phase::StrafePlayer;
            if fired.is_some() {
                break;
            }
        }
        assert!(strafed);
        let (from, dir) = fired.expect("a fireball");
        assert!((dir.length() - 1.0).abs() < 1e-9);
        // Aimed at the player.
        let to_player = (player + DVec3::Y * 1.6 - from).normalize();
        assert!(dir.dot(to_player) > 0.99);
        assert_eq!(from, dragon.head());
    }

    #[test]
    fn with_no_crystals_it_lands_sits_breathes_bites_and_takes_off_again() {
        let mut dragon = start();
        let near = DVec3::new(40.0, 64.0, 0.0);
        let mut seen = std::collections::HashSet::new();
        let mut breathed = false;
        for _ in 0..12000 {
            // The player stands far off, then comes close once it's sitting.
            let player = if dragon.phase.is_sitting() && dragon.perched > 250 {
                DVec3::new(6.0, 64.0, 0.0)
            } else {
                near
            };
            for act in dragon.tick(&senses(Some(player), 0)) {
                breathed |= matches!(act, Act::Breath { .. });
            }
            seen.insert(dragon.phase);
            if seen.contains(&Phase::Takeoff) && dragon.phase == Phase::HoldingPattern {
                break;
            }
        }
        for phase in [
            Phase::LandingApproach,
            Phase::Landing,
            Phase::SittingScanning,
            Phase::SittingFlaming,
            Phase::SittingAttacking,
            Phase::Takeoff,
        ] {
            assert!(seen.contains(&phase), "{phase:?} in {seen:?}");
        }
        assert!(breathed);
    }

    #[test]
    fn crystals_keep_it_from_landing() {
        let mut dragon = start();
        for _ in 0..8000 {
            dragon.tick(&senses(None, 3));
            assert!(!dragon.phase.is_sitting());
            assert_ne!(dragon.phase, Phase::LandingApproach);
        }
    }

    #[test]
    fn it_sits_over_the_fountain_when_it_lands() {
        let mut dragon = start();
        for _ in 0..12000 {
            dragon.tick(&senses(None, 0));
            if dragon.phase == Phase::SittingScanning {
                break;
            }
        }
        assert_eq!(dragon.phase, Phase::SittingScanning);
        assert_eq!(dragon.position, PODIUM + DVec3::Y * PERCH_RISE);
        assert_eq!(dragon.velocity, DVec3::ZERO);
    }

    #[test]
    fn hits_off_the_head_count_for_a_quarter_and_a_point() {
        let mut dragon = start();
        dragon.hurt(Part::Head, 8.0);
        assert_eq!(dragon.health, MAX_HEALTH - 8.0);
        dragon.hurt(Part::Body, 8.0);
        assert_eq!(dragon.health, MAX_HEALTH - 8.0 - 3.0, "8/4 + 1");
        dragon.hurt(Part::Wing, 0.5);
        assert_eq!(dragon.health, MAX_HEALTH - 11.0 - 0.625);
        assert!(dragon.hurt_ticks > 0);
    }

    #[test]
    fn a_quarter_of_its_health_in_damage_while_sitting_sends_it_off() {
        let mut dragon = start();
        dragon.set_phase(Phase::SittingScanning);
        dragon.hurt(Part::Head, 30.0);
        assert!(dragon.phase.is_sitting());
        dragon.hurt(Part::Head, 30.0);
        assert_eq!(dragon.phase, Phase::Takeoff);
        let mut flying = start();
        flying.hurt(Part::Head, 100.0);
        assert_eq!(flying.phase, Phase::HoldingPattern);
    }

    #[test]
    fn crystals_heal_it_up_to_full_and_no_further() {
        let mut dragon = start();
        dragon.hurt(Part::Head, 3.0);
        dragon.heal(1.0);
        assert_eq!(dragon.health, MAX_HEALTH - 2.0);
        dragon.heal(50.0);
        assert_eq!(dragon.health, MAX_HEALTH);
    }

    #[test]
    fn it_flies_to_the_exit_portal_then_dies_over_two_hundred_ticks() {
        let mut dragon = start();
        dragon.position = DVec3::new(80.0, 100.0, 0.0);
        assert!(dragon.hurt(Part::Head, 500.0));
        assert_eq!(dragon.phase, Phase::DeathFlight);
        assert_eq!(dragon.health, 0.0);
        assert!(!dragon.hurt(Part::Head, 5.0), "can't be hurt meanwhile");
        // It flies, doing nothing else, until it's over the portal.
        let mut flight = 0;
        while dragon.phase == Phase::DeathFlight {
            let acts = dragon.tick(&senses(Some(dragon.position), 0));
            assert!(acts.is_empty(), "no attacks while it flies: {acts:?}");
            flight += 1;
            assert!(flight <= 201, "arrives within 10 seconds");
        }
        assert_eq!(dragon.phase, Phase::Dying);
        let (dx, dz) = (dragon.position.x - EXIT.x, dragon.position.z - EXIT.z);
        assert!(dx.hypot(dz) < 3.0, "over the portal: {}", dragon.position);
        // Then the 200 ticks: rising a tenth a tick, turning, bursts over
        // the last twenty, and gone at the end.
        let (y0, yaw0) = (dragon.position.y, dragon.yaw);
        let mut bursts = 0;
        let mut died_at = None;
        let mut first_burst = None;
        for tick in 1..=DEATH_TICKS + 5 {
            for act in dragon.tick(&senses(None, 0)) {
                match act {
                    Act::Burst { .. } => {
                        bursts += 1;
                        first_burst.get_or_insert(tick);
                    }
                    Act::Died => {
                        died_at.get_or_insert(tick);
                    }
                    _ => {}
                }
            }
        }
        assert_eq!(died_at, Some(DEATH_TICKS));
        assert_eq!(first_burst, Some(DEATH_BURSTS_FROM));
        assert_eq!(bursts, (DEATH_TICKS + 5 - DEATH_BURSTS_FROM + 1) as usize);
        assert!(
            (dragon.position.y - y0 - 0.1 * f64::from(DEATH_TICKS + 5)).abs() < 1e-6,
            "rose a tenth a tick"
        );
        assert!(dragon.yaw != yaw0, "spun");
    }

    #[test]
    fn a_dragon_that_cannot_reach_the_portal_in_10_seconds_dies_where_it_is() {
        let mut dragon = start();
        dragon.position = DVec3::new(900.0, 100.0, 0.0);
        dragon.hurt(Part::Head, 500.0);
        for _ in 0..205 {
            dragon.tick(&senses(None, 0));
        }
        assert_eq!(dragon.phase, Phase::Dying);
        assert!(dragon.position.x > 600.0, "{}", dragon.position);
    }

    #[test]
    fn rays_find_the_nearest_part_and_miss_the_air() {
        let mut dragon = start();
        dragon.position = DVec3::new(0.0, 80.0, 0.0);
        dragon.yaw = 0.0;
        let from = DVec3::new(0.0, 80.0, 30.0);
        let (part, distance) = dragon.ray_hit(from, DVec3::NEG_Z, 40.0).unwrap();
        // Facing north, the head is nearest to the south... which is the tail.
        assert!(matches!(part, Part::Tail | Part::Body));
        assert!(distance > 15.0);
        let from_front = DVec3::new(0.0, 81.0, -30.0);
        let (front, _) = dragon.ray_hit(from_front, DVec3::Z, 40.0).unwrap();
        assert_eq!(front, Part::Head);
        assert!(dragon.ray_hit(from, DVec3::Y, 40.0).is_none());
    }

    #[test]
    fn the_wings_spread_either_side_and_the_tail_trails_its_path() {
        let mut dragon = start();
        dragon.yaw = 0.0;
        for _ in 0..40 {
            dragon.tick(&senses(None, 10));
        }
        let parts = dragon.parts();
        let wings: Vec<_> = parts.iter().filter(|(p, _)| *p == Part::Wing).collect();
        assert_eq!(wings.len(), 2);
        let (a, b) = (wings[0].1, wings[1].1);
        assert!(((a.min + a.max) / 2.0).distance((b.min + b.max) / 2.0) > 12.0);
        assert_eq!(parts.iter().filter(|(p, _)| *p == Part::Tail).count(), 3);
    }

    #[test]
    fn a_saved_dragon_comes_back_where_and_as_it_was() {
        let mut dragon = start();
        dragon.hurt(Part::Head, 40.0);
        dragon.position = DVec3::new(10.0, 90.0, -5.0);
        let back = EnderDragon::restore(dragon.save(), 1);
        assert_eq!(back.position, dragon.position);
        assert_eq!(back.health, 160.0);
        assert_eq!(back.phase, Phase::HoldingPattern);
    }

    #[test]
    fn angles_turn_the_short_way() {
        assert!((angle_between(0.1, -0.1) + 0.2).abs() < 1e-6);
        assert!(angle_between(3.0, -3.0) > 0.0, "through pi, not back round");
        assert!(angle_between(0.0, std::f32::consts::TAU).abs() < 1e-5);
    }
}
