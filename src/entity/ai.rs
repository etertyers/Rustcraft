//! What mobs decide each tick, after 1.0.0's `EntityCreature`,
//! `EntityAnimal` and `EntityMob`: passive mobs wander and run when hurt;
//! hostile mobs chase a player they can see and hit them up close. A new
//! kind of behavior is a new `Ai` variant handled here.

use glam::{DVec3, IVec3};

use super::{
    mob::{Mob, angle_difference},
    mobs::{Ai, MobDef},
    path::{Terrain, find_path},
};
use crate::player::{collision::Aabb, physics::MoveInput};

/// Ticks a hurt passive mob runs for (1.0.0's `fleeingTick`).
const FLEE_TICKS: u32 = 60;
/// Ticks between a hostile mob's hits (1.0.0's `attackTime`).
const ATTACK_COOLDOWN: u32 = 20;
/// A hostile mob notices a player it can see this close.
const NOTICE_RANGE: f64 = 16.0;
/// ...and gives up beyond this.
const GIVE_UP_RANGE: f64 = 32.0;
/// Hits land within this distance (1.0.0's `attackEntity`).
const ATTACK_RANGE: f64 = 2.0;
/// Turning speed, radians per tick (1.0.0's 30°).
const TURN: f32 = std::f32::consts::PI / 6.0;
/// Wandering picks the best of this many spots within these offsets.
const WANDER_TRIES: usize = 10;
const WANDER_SPREAD: (i32, i32) = (6, 3);
const WANDER_DISTANCE: f32 = 10.0;

#[derive(Debug, Default, Clone)]
pub struct AiState {
    path: Vec<IVec3>,
    /// Hostile: chasing the player.
    pub chasing: bool,
    /// Passive: ticks left running away.
    fleeing: u32,
    attack_cooldown: u32,
    hurt: bool,
}

impl AiState {
    pub fn on_hurt(&mut self) {
        self.hurt = true;
    }
}

/// The player, as a mob sees them.
#[derive(Debug, Clone, Copy)]
pub struct Target {
    pub feet: DVec3,
    pub aabb: Aabb,
    /// A clear line of sight from the mob's eyes.
    pub visible: bool,
}

/// What a mob can find out about the world.
pub struct View<'a, T: Terrain> {
    pub terrain: &'a T,
    /// The player, if this mob may go after them (survival and alive).
    pub player: Option<Target>,
    /// How much the mob likes a spot to wander to (grass for animals,
    /// darkness for monsters).
    pub weight: &'a dyn Fn(IVec3) -> f32,
}

/// What the mob does this tick.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct Intent {
    pub input: MoveInput,
    /// Damage dealt to the player this tick.
    pub hit: Option<i32>,
}

/// Decides a mob's tick, turning it to face where it's going.
pub fn think<T: Terrain>(
    mob: &mut Mob,
    def: &MobDef,
    view: &View<T>,
    random: &mut impl FnMut() -> f32,
) -> Intent {
    let mut intent = Intent::default();
    if mob.is_dead() {
        return intent;
    }
    let height = def.height.ceil() as i32;
    let feet = mob.feet();
    let cell = mob.cell();
    let body = mob.body.aabb();
    let state = &mut mob.ai;
    state.fleeing = state.fleeing.saturating_sub(1);
    state.attack_cooldown = state.attack_cooldown.saturating_sub(1);
    if std::mem::take(&mut state.hurt) {
        match def.ai {
            Ai::Passive => {
                state.fleeing = FLEE_TICKS;
                state.path.clear();
            }
            Ai::Hostile { .. } => state.chasing = view.player.is_some(),
        }
    }

    let mut face = None;
    if let Ai::Hostile { attack } = def.ai {
        match &view.player {
            Some(target) => {
                let distance = target.feet.distance(feet);
                if distance < NOTICE_RANGE && target.visible {
                    state.chasing = true;
                } else if distance > GIVE_UP_RANGE {
                    state.chasing = false;
                }
                if state.chasing {
                    face = Some(target.feet);
                    let level = target.aabb.max.y > body.min.y && target.aabb.min.y < body.max.y;
                    if state.attack_cooldown == 0 && distance < ATTACK_RANGE && level {
                        intent.hit = Some(attack);
                        state.attack_cooldown = ATTACK_COOLDOWN;
                    }
                    if state.path.is_empty() || random() < 0.05 {
                        let goal = (target.feet + DVec3::Y * 0.5).floor().as_ivec3();
                        state.path =
                            find_path(view.terrain, cell, goal, height, NOTICE_RANGE as f32);
                    }
                }
            }
            None => state.chasing = false,
        }
    }

    if !state.chasing {
        let idle = state.path.is_empty() && random() < 1.0 / 180.0;
        let restless = random() < 1.0 / 120.0;
        let running = state.fleeing > 0 && state.path.is_empty();
        if idle || restless || running {
            wander(mob, height, view, random);
        }
    }

    // Follow the path, skipping waypoints already close (1.0.0 skips any
    // within twice the mob's width).
    let state = &mut mob.ai;
    let reach = (def.width * 2.0).max(0.5);
    while let Some(next) = state.path.first() {
        let center = next.as_dvec3() + DVec3::new(0.5, 0.0, 0.5);
        if (center - feet).with_y(0.0).length() >= reach {
            break;
        }
        state.path.remove(0);
    }
    let speed = if state.fleeing > 0 {
        def.speed * 2.0
    } else {
        def.speed
    };
    let mut heading = None;
    if let Some(next) = state.path.first() {
        let center = next.as_dvec3() + DVec3::new(0.5, 0.0, 0.5);
        heading = Some(center);
        intent.input.forward = speed;
        intent.input.jump = next.y > (mob.body.aabb().min.y + 0.5).floor() as i32;
    } else if let Some(target) = face
        && (target - feet).with_y(0.0).length() > 1.0
    {
        intent.input.forward = speed;
    }
    if let Some(toward) = face.or(heading) {
        let wanted = yaw_toward(feet, toward);
        mob.yaw += angle_difference(wanted, mob.yaw).clamp(-TURN, TURN);
    }
    mob.head_yaw = face.map_or(mob.yaw, |target| yaw_toward(feet, target));
    if mob.body.liquid.is_some() && random() < 0.8 {
        intent.input.jump = true;
    }
    if intent.input.forward > 0.0 && mob.body.collided_horizontally() {
        intent.input.jump = true;
    }
    intent
}

/// Picks the best-liked of a few nearby spots and paths to it (1.0.0's
/// `updateWanderPath`).
fn wander<T: Terrain>(
    mob: &mut Mob,
    height: i32,
    view: &View<T>,
    random: &mut impl FnMut() -> f32,
) {
    let start = mob.cell();
    let (across, up) = WANDER_SPREAD;
    let mut offset = |spread: i32| (random() * (spread * 2 + 1) as f32) as i32 - spread;
    let mut best: Option<(f32, IVec3)> = None;
    for _ in 0..WANDER_TRIES {
        let spot = start + IVec3::new(offset(across), offset(up), offset(across));
        let weight = (view.weight)(spot);
        if best.is_none_or(|(known, _)| weight > known) {
            best = Some((weight, spot));
        }
    }
    if let Some((_, spot)) = best {
        mob.ai.path = find_path(view.terrain, start, spot, height, WANDER_DISTANCE);
    }
}

/// Yaw from `from` looking at `to` (0 north, toward east).
fn yaw_toward(from: DVec3, to: DVec3) -> f32 {
    let d = to - from;
    d.x.atan2(-d.z) as f32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        entity::{mobs::MobRegistry, path::tests::Grid},
        player::{collision::Collider, physics::sized_hitbox},
    };

    impl Collider for Grid {
        fn boxes(&self, region: &Aabb, out: &mut Vec<Aabb>) {
            let (min, max) = (region.min.floor().as_ivec3(), region.max.floor().as_ivec3());
            for cell in &self.solid {
                if cell.cmpge(min).all() && cell.cmple(max).all() {
                    out.push(Aabb::block(cell.x, cell.y, cell.z));
                }
            }
        }
        fn slipperiness(&self, _: i32, _: i32, _: i32) -> f32 {
            0.6
        }
    }

    fn registry() -> MobRegistry {
        MobRegistry::load(&crate::assets::dir().join("data/mobs.ron")).unwrap()
    }

    /// Runs `ticks` ticks of AI and movement; returns damage dealt to the player.
    fn run(mob: &mut Mob, def: &MobDef, grid: &Grid, player: Option<Target>, ticks: u32) -> i32 {
        let mut seed = 0.37f32;
        let mut random = || {
            seed = (seed * 997.0 + 0.123).fract();
            seed
        };
        let weight = |_: IVec3| 0.0;
        let mut dealt = 0;
        for _ in 0..ticks {
            let view = View {
                terrain: grid,
                player,
                weight: &weight,
            };
            let intent = think(mob, def, &view, &mut random);
            dealt += intent.hit.unwrap_or(0);
            mob.tick(&intent.input, grid, false);
        }
        dealt
    }

    fn player_at(feet: DVec3) -> Target {
        Target {
            feet,
            aabb: sized_hitbox(feet, 0.6, 1.8),
            visible: true,
        }
    }

    #[test]
    fn a_zombie_chases_a_visible_player_and_hits_for_4_each_second() {
        let mobs = registry();
        let kind = mobs.find("zombie").unwrap();
        let def = mobs.get(kind);
        let grid = Grid::floor();
        let mut zombie = Mob::new(kind, def, DVec3::new(0.5, 64.0, 0.5), 0.0);
        let player = player_at(DVec3::new(8.5, 64.0, 0.5));
        let dealt = run(&mut zombie, def, &grid, Some(player), 200);
        assert!(
            zombie.feet().distance(player.feet) < ATTACK_RANGE,
            "caught up: {}",
            zombie.feet()
        );
        // Once in reach it hits every 20 ticks.
        assert!(dealt >= 4 * 4, "dealt {dealt}");
        assert_eq!(dealt % 4, 0);
    }

    #[test]
    fn a_zombie_ignores_a_player_it_may_not_attack() {
        let mobs = registry();
        let kind = mobs.find("zombie").unwrap();
        let def = mobs.get(kind);
        let mut zombie = Mob::new(kind, def, DVec3::new(0.5, 64.0, 0.5), 0.0);
        assert_eq!(run(&mut zombie, def, &Grid::floor(), None, 200), 0);
        assert!(!zombie.ai.chasing);
    }

    #[test]
    fn a_hurt_pig_runs_twice_as_fast() {
        let mobs = registry();
        let kind = mobs.find("pig").unwrap();
        let def = mobs.get(kind);
        let grid = Grid::floor();
        let start = DVec3::new(0.5, 64.0, 0.5);
        let mut pig = Mob::new(kind, def, start, 0.0);
        pig.hurt(
            1,
            Some(start + DVec3::X),
            crate::player::stats::DamageSource::Mob,
        );
        run(&mut pig, def, &grid, None, 40);
        assert!(pig.feet().distance(start) > 3.0, "ran off: {}", pig.feet());
        assert_eq!(pig.stats.health, 9);
    }
}
