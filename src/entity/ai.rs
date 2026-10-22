//! What mobs decide each tick, after 1.0.0's `EntityCreature`,
//! `EntityAnimal` and `EntityMob` and modern Minecraft's goals for the
//! later kinds. Every mob wants one thing a tick (`Goal`); which depends on
//! its `Ai`: passive mobs wander and run when hurt, hostile ones chase and
//! hit what they can see and reach, archers hold off and shoot, creepers
//! swell, and so on. Steering (paths, turning, jumping) is shared.

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
/// Turning speed, radians per tick (1.0.0's 30°).
const TURN: f32 = std::f32::consts::PI / 6.0;
/// Wandering picks the best of this many spots within these offsets.
const WANDER_TRIES: usize = 10;
const WANDER_SPREAD: (i32, i32) = (6, 3);
const WANDER_DISTANCE: f32 = 10.0;
/// Ticks a neutral mob stays angry after being hurt.
const ANGER_TICKS: u32 = 400;
/// An archer wants to be between these distances, and shoots within the
/// larger (modern `RangedBowAttackGoal`).
const ARCHER_RANGE: f64 = 15.0;
/// A bow takes this long to draw.
pub const DRAW_TICKS: u32 = 20;
/// Creepers start swelling this close and swell for this long.
const SWELL_RANGE: f64 = 3.0;
const SWELL_STOP_RANGE: f64 = 7.0;
pub const SWELL_TICKS: i32 = 30;
/// A tamed mob follows from this far and stops this near.
const FOLLOW_START: f64 = 10.0;
const FOLLOW_STOP: f64 = 2.0;
/// Hostile mobs a golem or wolf goes after from this far.
const THREAT_RANGE: f64 = 16.0;
/// Ghasts notice the player this far off, charge a fireball this long
/// (showing the shooting face from halfway), and wait this long after
/// (modern `GhastShootFireballGoal`: 64 blocks, 20 ticks, −40).
const GHAST_RANGE: f64 = 64.0;
const GHAST_CHARGE: i32 = 20;
const GHAST_REST: i32 = 40;
/// A ghast drifts to a spot this far off in each direction (modern
/// `RandomFloatAroundGoal`).
const GHAST_DRIFT: f64 = 16.0;
/// Blazes notice the player within 48 blocks; burn up for 60 ticks, loose
/// three small fireballs 6 ticks apart and rest 100 (modern
/// `BlazeAttackGoal`), and strike 6 at arm's length.
const BLAZE_RANGE: f64 = 48.0;
const BLAZE_CHARGE: i32 = 60;
const BLAZE_VOLLEY: i32 = 6;
const BLAZE_REST: i32 = 100;
const BLAZE_HIT: f32 = 6.0;
/// Phantoms circle this far from the player and this high above.
const ORBIT_RADIUS: f64 = 16.0;
const ORBIT_HEIGHT: f64 = 15.0;

#[derive(Debug, Default, Clone)]
pub struct AiState {
    path: Vec<IVec3>,
    /// Hostile: chasing its target.
    pub chasing: bool,
    /// Passive: ticks left running away.
    fleeing: u32,
    attack_cooldown: u32,
    hurt: bool,
    /// Neutral: ticks of rage left after being hurt.
    pub angry: u32,
    /// Archers: ticks until the next shot, ticks the bow has been drawn,
    /// how long the target has been in sight (negative once lost), and
    /// strafing.
    shoot_cooldown: i32,
    pub draw: u32,
    see_time: i32,
    strafe_time: i32,
    strafe_clockwise: bool,
    strafe_back: bool,
    /// Hoppers (slimes, rabbits): ticks until the next hop.
    hop_delay: u32,
    leap_cooldown: u32,
    /// Phantoms: angle round the player and ticks left in a dive.
    orbit: f32,
    dive: u32,
    /// Flyers and swimmers: where they're headed.
    destination: Option<DVec3>,
    /// Ghasts: ticks charging a fireball (negative while resting after).
    charge: i32,
}

impl AiState {
    pub fn on_hurt(&mut self) {
        self.hurt = true;
    }

    pub fn clear_path(&mut self) {
        self.path.clear();
    }

    /// Makes a neutral mob angry (without a hit, as an enderman's stare
    /// does).
    pub fn enrage(&mut self) {
        self.angry = ANGER_TICKS;
        self.chasing = true;
    }
}

/// The player, as a mob sees them.
#[derive(Debug, Clone, Copy)]
pub struct Target {
    pub feet: DVec3,
    pub eye: DVec3,
    pub aabb: Aabb,
    /// A clear line of sight from the mob's eyes.
    pub visible: bool,
    /// Looking at the mob's head (what angers an enderman).
    pub stares: bool,
}

/// Another mob nearby, as a mob sees it.
#[derive(Debug, Clone, Copy)]
pub struct Other {
    pub index: usize,
    pub feet: DVec3,
    pub eye: DVec3,
    /// A monster (golems and tame wolves go after these).
    pub monster: bool,
    pub creeper: bool,
    /// Chickens and rabbits, which foxes hunt.
    pub prey: bool,
    pub cat: bool,
    pub visible: bool,
    pub width: f64,
}

/// What the surroundings are like for this mob.
#[derive(Debug, Clone, Copy, Default)]
pub struct Env {
    /// How bright it is where the mob stands, 0–1.
    pub brightness: f32,
    pub day: bool,
    /// The cell the mob is in is water.
    pub in_water: bool,
}

/// What a mob can find out about the world.
pub struct View<'a, T: Terrain> {
    pub terrain: &'a T,
    /// The player, if this mob may go after them (survival and alive).
    pub player: Option<Target>,
    /// How much the mob likes a spot to wander to (grass for animals,
    /// darkness for monsters).
    pub weight: &'a dyn Fn(IVec3) -> f32,
    pub env: Env,
    pub others: &'a [Other],
    /// Where the player is, even if they can't be attacked: whom tame mobs
    /// follow.
    pub owner: Option<DVec3>,
    /// The mob (an index into `others`) that hurt or was hurt by the
    /// player: tame wolves help.
    pub threat: Option<usize>,
    /// Where a villager's day sends it (its job site, the bell, its bed).
    pub errand: Option<DVec3>,
}

/// Who a mob's blow lands on.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Victim {
    Player,
    Mob(usize),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Strike {
    pub victim: Victim,
    pub damage: f32,
}

/// A missile an archer or witch looses.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Shot {
    pub from: DVec3,
    pub at: DVec3,
}

/// What an enderman does with the block it stands near.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockAction {
    Take,
    Place,
}

/// Where an enderman or a mob in trouble teleports.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Teleport {
    /// To a random spot within 32 blocks.
    Random,
    /// To a spot near this point.
    Near(DVec3),
}

/// What the mob does this tick.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct Intent {
    pub input: MoveInput,
    /// A blow struck this tick.
    pub hit: Option<Strike>,
    pub shot: Option<Shot>,
    /// A creeper going off, with its power.
    pub explode: Option<f32>,
    /// A ghast's fireball.
    pub fireball: Option<Shot>,
    /// A blaze's small fireball.
    pub small_fireball: Option<Shot>,
    /// A shulker's homing bullet (M28).
    pub bullet: Option<Shot>,
    /// The blow (`hit`) throws its victim up (hoglins).
    pub toss: bool,
    /// A spider's leap: velocity to set.
    pub leap: Option<DVec3>,
    pub teleport: Option<Teleport>,
    pub block: Option<BlockAction>,
    /// Vertical speed when flying or swimming (−1 to 1).
    pub rise: f64,
}

/// What a mob wants this tick.
#[derive(Debug, Clone, Copy)]
enum Goal {
    /// Wander as usual.
    Idle,
    /// Path to a point and face it.
    Chase(DVec3),
    /// Stand and face a point.
    Face(DVec3),
    /// Run from a point.
    Flee(DVec3),
    /// Hold off from a point: move sideways round it, and back or in.
    Strafe {
        around: DVec3,
        clockwise: bool,
        back: bool,
    },
    /// Move straight at a point, through the air or water.
    Direct(DVec3),
    /// Do nothing but look at a point (a mob swelling or sitting).
    Stand(Option<DVec3>),
}

/// How far `from` is from `target` for a melee blow: the mob's feet to the
/// target's feet (modern `getAttackReachSqr`).
fn reaches(
    def: &MobDef,
    from: DVec3,
    size: (f64, f64),
    target_feet: DVec3,
    target_width: f64,
) -> bool {
    let reach_sq = (size.0 * 2.0).powi(2) + target_width;
    from.distance_squared(target_feet) <= reach_sq && (def.height > 0.0)
}

/// Decides a mob's tick, turning it to face where it's going.
pub fn think<T: Terrain>(
    mob: &mut Mob,
    def: &MobDef,
    view: &View<T>,
    random: &mut impl FnMut() -> f32,
) -> Intent {
    let mut intent = Intent::default();
    mob.aggressive = false;
    if mob.is_dead() {
        return intent;
    }
    let height = mob.size().1.ceil() as i32;
    let state = &mut mob.ai;
    state.fleeing = state.fleeing.saturating_sub(1);
    state.attack_cooldown = state.attack_cooldown.saturating_sub(1);
    state.angry = state.angry.saturating_sub(1);
    state.leap_cooldown = state.leap_cooldown.saturating_sub(1);
    state.hop_delay = state.hop_delay.saturating_sub(1);
    state.shoot_cooldown -= 1;
    let was_hurt = std::mem::take(&mut state.hurt);
    if was_hurt {
        match def.ai {
            Ai::Hostile { .. }
            | Ai::Creeper { .. }
            | Ai::Witch
            | Ai::Archer { .. }
            | Ai::Ghast
            | Ai::Shulker
            | Ai::Blaze => {
                state.chasing = view.player.is_some();
            }
            Ai::Passive
            | Ai::Rabbit
            | Ai::Horse
            | Ai::Swimmer
            | Ai::Villager
            | Ai::Cat
            | Ai::Strider
            | Ai::Fox { .. } => {
                state.fleeing = FLEE_TICKS;
                state.path.clear();
            }
            Ai::Neutral { .. }
            | Ai::Spider { .. }
            | Ai::Wolf { .. }
            | Ai::Bee { .. }
            | Ai::Enderman { .. }
            | Ai::Phantom { .. }
            | Ai::ZombifiedPiglin { .. }
            | Ai::Piglin { .. }
            | Ai::Hoglin { .. }
            | Ai::Slime => {
                state.angry = ANGER_TICKS;
                state.chasing = view.player.is_some();
            }
        }
    }

    let goal = match def.ai {
        Ai::Villager => match view.errand {
            // Walk there, then stay put.
            Some(errand) if mob.feet().distance(errand) > 1.5 => Goal::Chase(errand),
            Some(errand) => Goal::Stand(Some(errand + DVec3::Y)),
            None => Goal::Idle,
        },
        Ai::Passive | Ai::Horse => Goal::Idle,
        Ai::Rabbit => Goal::Idle,
        Ai::Hostile { attack } => melee(mob, def, view, attack, true, &mut intent),
        Ai::Neutral { attack } => {
            let angry = mob.ai.angry > 0;
            if angry {
                melee(mob, def, view, attack, true, &mut intent)
            } else {
                golem_or_idle(mob, def, view, attack, &mut intent)
            }
        }
        Ai::Spider { attack } => {
            // Hostile in the dark, leaving the player alone in the light
            // unless hurt.
            let dark = view.env.brightness < 0.5;
            let angry = mob.ai.angry > 0;
            if dark || angry {
                let goal = melee(mob, def, view, attack, true, &mut intent);
                spider_leap(mob, view, &mut intent);
                goal
            } else {
                mob.ai.chasing = false;
                Goal::Idle
            }
        }
        Ai::Archer { interval } => archer(mob, view, interval, random, &mut intent),
        Ai::Creeper { power } => creeper(mob, view, power, &mut intent),
        Ai::Enderman { attack } => enderman(mob, def, view, attack, random, &mut intent),
        Ai::Slime => slime(mob, def, view, &mut intent),
        Ai::Witch => witch(mob, view, &mut intent),
        Ai::Phantom { attack } => phantom(mob, def, view, attack, &mut intent),
        Ai::Swimmer => swimmer(mob, view, random),
        Ai::Wolf { attack } => wolf(mob, def, view, attack, &mut intent),
        Ai::Cat => cat(mob, view),
        Ai::Bee { attack } => bee(mob, def, view, attack, random, &mut intent),
        Ai::Fox { attack } => fox(mob, def, view, attack, &mut intent),
        Ai::Ghast => ghast(mob, view, random, &mut intent),
        Ai::ZombifiedPiglin { attack } => {
            if mob.ai.angry > 0 {
                melee(mob, def, view, attack, true, &mut intent)
            } else {
                mob.ai.chasing = false;
                Goal::Idle
            }
        }
        Ai::Piglin { attack } => piglin(mob, def, view, attack, &mut intent),
        Ai::Hoglin { attack } => {
            let goal = melee(mob, def, view, attack, true, &mut intent);
            intent.toss = intent.hit.is_some();
            goal
        }
        Ai::Strider => Goal::Idle,
        Ai::Blaze => blaze(mob, def, view, random, &mut intent),
        Ai::Shulker => shulker(mob, view, &mut intent),
    };

    steer(mob, def, view, goal, height, random, &mut intent);
    intent
}

/// Chases the player while it can see them and strikes when in reach and
/// in the open (modern `MeleeAttackGoal`, which needs a line of sight).
fn melee<T: Terrain>(
    mob: &mut Mob,
    def: &MobDef,
    view: &View<T>,
    attack: f32,
    needs_sight: bool,
    intent: &mut Intent,
) -> Goal {
    let feet = mob.feet();
    let size = mob.size();
    let state = &mut mob.ai;
    let Some(target) = &view.player else {
        state.chasing = false;
        return Goal::Idle;
    };
    let distance = target.feet.distance(feet);
    if distance < NOTICE_RANGE && target.visible {
        state.chasing = true;
    } else if distance > GIVE_UP_RANGE {
        state.chasing = false;
    }
    if !state.chasing {
        return Goal::Idle;
    }
    mob.aggressive = true;
    let target_width = target.aabb.max.x - target.aabb.min.x;
    let in_reach = reaches(def, feet, size, target.feet, target_width);
    if mob.ai.attack_cooldown == 0 && in_reach && (target.visible || !needs_sight) {
        intent.hit = Some(Strike {
            victim: Victim::Player,
            damage: attack,
        });
        mob.ai.attack_cooldown = ATTACK_COOLDOWN;
    }
    Goal::Chase(target.feet)
}

/// An iron golem goes after monsters near it; anything else neutral waits.
fn golem_or_idle<T: Terrain>(
    mob: &mut Mob,
    def: &MobDef,
    view: &View<T>,
    attack: f32,
    intent: &mut Intent,
) -> Goal {
    if def.name != "iron_golem" {
        return Goal::Idle;
    }
    let feet = mob.feet();
    let size = mob.size();
    let nearest = view
        .others
        .iter()
        .filter(|o| o.monster && o.visible && o.feet.distance(feet) < THREAT_RANGE)
        .min_by(|a, b| {
            a.feet
                .distance_squared(feet)
                .total_cmp(&b.feet.distance_squared(feet))
        });
    let Some(other) = nearest else {
        return Goal::Idle;
    };
    mob.aggressive = true;
    if mob.ai.attack_cooldown == 0 && reaches(def, feet, size, other.feet, other.width) {
        intent.hit = Some(Strike {
            victim: Victim::Mob(other.index),
            damage: attack,
        });
        mob.ai.attack_cooldown = ATTACK_COOLDOWN;
    }
    Goal::Chase(other.feet)
}

/// A spider leaps at a target a few blocks off (modern `LeapAtTargetGoal`).
fn spider_leap<T: Terrain>(mob: &mut Mob, view: &View<T>, intent: &mut Intent) {
    let Some(target) = &view.player else {
        return;
    };
    if !mob.ai.chasing || !mob.body.on_ground || mob.ai.leap_cooldown > 0 {
        return;
    }
    let away = target.feet - mob.feet();
    let flat = DVec3::new(away.x, 0.0, away.z);
    let d = flat.length();
    if (2.0..6.0).contains(&d) {
        let v = flat / d * 0.4 + mob.body.velocity * 0.2;
        intent.leap = Some(DVec3::new(v.x, 0.4, v.z));
        mob.ai.leap_cooldown = 20;
    }
}

/// Skeletons: close in until they see the target, then hold off between a
/// quarter and three quarters of the range strafing, drawing the bow for a
/// second and shooting every two (modern `RangedBowAttackGoal`).
fn archer<T: Terrain>(
    mob: &mut Mob,
    view: &View<T>,
    interval: u32,
    random: &mut impl FnMut() -> f32,
    intent: &mut Intent,
) -> Goal {
    let feet = mob.feet();
    let state = &mut mob.ai;
    let Some(target) = &view.player else {
        state.chasing = false;
        state.draw = 0;
        return Goal::Idle;
    };
    let distance = target.feet.distance(feet);
    if distance < NOTICE_RANGE + 8.0 && target.visible {
        state.chasing = true;
    } else if distance > GIVE_UP_RANGE {
        state.chasing = false;
    }
    if !state.chasing {
        state.draw = 0;
        return Goal::Idle;
    }
    let in_range = distance <= ARCHER_RANGE;
    if target.visible {
        state.see_time = (state.see_time + 1).max(1);
    } else {
        state.see_time = (state.see_time - 1).min(-1);
    }
    let goal = if in_range && state.see_time >= 20 {
        state.strafe_time += 1;
        if state.strafe_time >= 20 {
            if random() < 0.3 {
                state.strafe_clockwise = !state.strafe_clockwise;
            }
            if random() < 0.3 {
                state.strafe_back = !state.strafe_back;
            }
            state.strafe_time = 0;
        }
        if distance > ARCHER_RANGE * 0.75 {
            state.strafe_back = false;
        } else if distance < ARCHER_RANGE * 0.25 {
            state.strafe_back = true;
        }
        Goal::Strafe {
            around: target.feet,
            clockwise: state.strafe_clockwise,
            back: state.strafe_back,
        }
    } else {
        state.strafe_time = -1;
        Goal::Chase(target.feet)
    };
    mob.aggressive = true;
    // Drawing and loosing.
    let state = &mut mob.ai;
    if state.draw > 0 {
        if !target.visible && state.see_time < -60 {
            state.draw = 0;
        } else if target.visible {
            state.draw += 1;
            if state.draw >= DRAW_TICKS {
                state.draw = 0;
                state.shoot_cooldown = interval as i32;
                intent.shot = Some(Shot {
                    from: mob.eye(),
                    at: target.feet + (target.aabb.max - target.aabb.min) * DVec3::Y / 3.0,
                });
            }
        }
    } else if state.shoot_cooldown <= 0 && state.see_time >= -60 && in_range {
        state.draw = 1;
    }
    goal
}

/// Creepers: swell while the player is within 3 blocks (and keep swelling
/// to 7), go off at 30 ticks, back off when the player goes (modern
/// `SwellGoal`); otherwise chase without a blow.
fn creeper<T: Terrain>(mob: &mut Mob, view: &View<T>, power: f32, intent: &mut Intent) -> Goal {
    let feet = mob.feet();
    // A cat in sight sends it running.
    if let Some(cat) = view
        .others
        .iter()
        .find(|o| o.cat && o.feet.distance(feet) < 6.0)
    {
        mob.swell = (mob.swell - 1).max(0);
        return Goal::Flee(cat.feet);
    }
    let Some(target) = &view.player else {
        mob.swell = (mob.swell - 1).max(0);
        mob.ai.chasing = false;
        return Goal::Idle;
    };
    let distance = target.feet.distance(feet);
    if distance < NOTICE_RANGE && target.visible {
        mob.ai.chasing = true;
    } else if distance > GIVE_UP_RANGE {
        mob.ai.chasing = false;
    }
    let swelling = mob.swell > 0;
    let close = distance < SWELL_RANGE && target.visible;
    let holding = swelling && distance < SWELL_STOP_RANGE && target.visible;
    if close || holding {
        mob.swell += 1;
        mob.aggressive = true;
        if mob.swell >= SWELL_TICKS {
            intent.explode = Some(power);
        }
        return Goal::Stand(Some(target.feet));
    }
    mob.swell = (mob.swell - 1).max(0);
    if mob.ai.chasing {
        Goal::Chase(target.feet)
    } else {
        Goal::Idle
    }
}

/// Endermen: calm until looked at in the head, then they chase and hit for
/// 7, teleporting to keep up; water hurts them and they teleport out of
/// it; at times they pick up and put down blocks.
fn enderman<T: Terrain>(
    mob: &mut Mob,
    def: &MobDef,
    view: &View<T>,
    attack: f32,
    random: &mut impl FnMut() -> f32,
    intent: &mut Intent,
) -> Goal {
    if let Some(target) = &view.player
        && target.stares
        && mob.ai.angry == 0
    {
        mob.ai.enrage();
    }
    if view.env.in_water {
        intent.teleport = Some(Teleport::Random);
    }
    if mob.ai.angry == 0 {
        mob.ai.chasing = false;
        // Idle: now and then move a block about, and drift about.
        if random() < 1.0 / 20.0 && mob.carrying.is_none() {
            intent.block = Some(BlockAction::Take);
        } else if random() < 1.0 / 2000.0 && mob.carrying.is_some() {
            intent.block = Some(BlockAction::Place);
        }
        if random() < 1.0 / 400.0 && !view.env.day {
            intent.teleport = Some(Teleport::Random);
        }
        return Goal::Idle;
    }
    let Some(target) = &view.player else {
        mob.ai.angry = 0;
        return Goal::Idle;
    };
    mob.ai.chasing = true;
    mob.aggressive = true;
    let distance = target.feet.distance(mob.feet());
    if (distance > 16.0 && random() < 1.0 / 30.0) || (!target.visible && random() < 1.0 / 60.0) {
        intent.teleport = Some(Teleport::Near(target.feet));
    }
    let size = mob.size();
    let width = target.aabb.max.x - target.aabb.min.x;
    if mob.ai.attack_cooldown == 0
        && target.visible
        && reaches(def, mob.feet(), size, target.feet, width)
    {
        intent.hit = Some(Strike {
            victim: Victim::Player,
            damage: attack,
        });
        mob.ai.attack_cooldown = ATTACK_COOLDOWN;
    }
    Goal::Chase(target.feet)
}

/// Slimes hop after a player they can see, and a hop that lands on them
/// hurts (a size of 1 doesn't).
fn slime<T: Terrain>(mob: &mut Mob, def: &MobDef, view: &View<T>, intent: &mut Intent) -> Goal {
    let size = mob.data.size.max(1);
    let feet = mob.feet();
    let body = mob.size();
    let Some(target) = &view.player else {
        mob.ai.chasing = false;
        return Goal::Idle;
    };
    let distance = target.feet.distance(feet);
    if distance < NOTICE_RANGE && target.visible {
        mob.ai.chasing = true;
    } else if distance > GIVE_UP_RANGE {
        mob.ai.chasing = false;
    }
    if !mob.ai.chasing {
        return Goal::Idle;
    }
    mob.aggressive = true;
    let width = target.aabb.max.x - target.aabb.min.x;
    if size > 1
        && mob.ai.attack_cooldown == 0
        && target.visible
        && reaches(def, feet, (body.0 * 1.4, body.1), target.feet, width)
    {
        intent.hit = Some(Strike {
            victim: Victim::Player,
            damage: f32::from(size),
        });
        mob.ai.attack_cooldown = ATTACK_COOLDOWN;
    }
    Goal::Direct(target.feet)
}

/// Witches stay about ten blocks off and pelt the player.
fn witch<T: Terrain>(mob: &mut Mob, view: &View<T>, intent: &mut Intent) -> Goal {
    let feet = mob.feet();
    let state = &mut mob.ai;
    let Some(target) = &view.player else {
        state.chasing = false;
        return Goal::Idle;
    };
    let distance = target.feet.distance(feet);
    if distance < NOTICE_RANGE && target.visible {
        state.chasing = true;
    } else if distance > GIVE_UP_RANGE {
        state.chasing = false;
    }
    if !state.chasing {
        return Goal::Idle;
    }
    mob.aggressive = true;
    if state.shoot_cooldown <= 0 && target.visible && distance < 10.0 {
        state.shoot_cooldown = 60;
        intent.shot = Some(Shot {
            from: mob.eye(),
            at: target.eye,
        });
    }
    if distance < 5.0 {
        Goal::Flee(target.feet)
    } else if distance > 9.0 {
        Goal::Chase(target.feet)
    } else {
        Goal::Face(target.feet)
    }
}

/// Phantoms circle the player high up, now and then diving to strike.
fn phantom<T: Terrain>(
    mob: &mut Mob,
    def: &MobDef,
    view: &View<T>,
    attack: f32,
    intent: &mut Intent,
) -> Goal {
    let feet = mob.feet();
    let size = mob.size();
    let state = &mut mob.ai;
    let Some(target) = &view.player else {
        return Goal::Idle;
    };
    state.orbit += 0.06;
    if state.dive > 0 {
        state.dive -= 1;
        mob.aggressive = true;
        let aim = target.eye;
        let width = target.aabb.max.x - target.aabb.min.x;
        if mob.ai.attack_cooldown == 0
            && reaches(def, feet, (size.0 * 1.5, size.1), target.feet, width + 1.0)
        {
            intent.hit = Some(Strike {
                victim: Victim::Player,
                damage: attack,
            });
            mob.ai.attack_cooldown = ATTACK_COOLDOWN;
            mob.ai.dive = 0;
        }
        return Goal::Direct(aim);
    }
    if state.attack_cooldown == 0 && state.orbit.sin() > 0.98 && target.visible {
        state.dive = 60;
    }
    let angle = f64::from(state.orbit);
    let around = target.feet
        + DVec3::new(
            angle.cos() * ORBIT_RADIUS,
            ORBIT_HEIGHT,
            angle.sin() * ORBIT_RADIUS,
        );
    Goal::Direct(around)
}

/// Ghasts: with the player in sight within 64 blocks, hover facing them,
/// charge for a second (showing the shooting face from halfway) and loose
/// a fireball, then rest two seconds; otherwise drift to a random spot up
/// to 16 blocks off now and then.
fn ghast<T: Terrain>(
    mob: &mut Mob,
    view: &View<T>,
    random: &mut impl FnMut() -> f32,
    intent: &mut Intent,
) -> Goal {
    let feet = mob.feet();
    let eye = mob.eye();
    let target = view
        .player
        .filter(|t| t.visible && t.eye.distance(eye) < GHAST_RANGE);
    let state = &mut mob.ai;
    let goal = if let Some(target) = target {
        state.chasing = true;
        state.charge += 1;
        if state.charge >= GHAST_CHARGE {
            let middle = (target.aabb.min + target.aabb.max) / 2.0;
            intent.fireball = Some(Shot {
                from: eye,
                at: middle,
            });
            state.charge = -GHAST_REST;
        }
        Goal::Stand(Some(target.eye))
    } else {
        state.chasing = false;
        if state.charge > 0 {
            state.charge -= 1;
        }
        let arrived = state
            .destination
            .is_none_or(|to| to.distance(feet) < 1.0 || to.distance(feet) > 60.0);
        if arrived || random() < 0.01 {
            let mut offset = || f64::from(random() * 2.0 - 1.0) * GHAST_DRIFT;
            state.destination = Some(feet + DVec3::new(offset(), offset(), offset()));
        }
        state.destination.map_or(Goal::Idle, Goal::Direct)
    };
    mob.aggressive = mob.ai.charge > GHAST_CHARGE / 2;
    mob.data.variant = u8::from(mob.aggressive);
    goal
}

/// Blazes: with the player in sight within 48 blocks, strike them at
/// arm's length or hang back and shoot: burn up for three seconds, then
/// loose three small fireballs, a little scattered, and rest five seconds.
/// They drift to just above the player's eyes.
fn blaze<T: Terrain>(
    mob: &mut Mob,
    def: &MobDef,
    view: &View<T>,
    random: &mut impl FnMut() -> f32,
    intent: &mut Intent,
) -> Goal {
    let (feet, eye, size) = (mob.feet(), mob.eye(), mob.size());
    let Some(target) = view
        .player
        .filter(|t| t.visible && t.eye.distance(eye) < BLAZE_RANGE)
    else {
        mob.ai.chasing = false;
        mob.ai.charge = mob.ai.charge.min(0);
        return Goal::Idle;
    };
    let state = &mut mob.ai;
    state.chasing = true;
    let width = target.aabb.max.x - target.aabb.min.x;
    if reaches(def, feet, size, target.feet, width) {
        if state.attack_cooldown == 0 {
            intent.hit = Some(Strike {
                victim: Victim::Player,
                damage: BLAZE_HIT,
            });
            state.attack_cooldown = ATTACK_COOLDOWN;
        }
        return Goal::Direct(target.feet);
    }
    state.charge += 1;
    let volley = state.charge - BLAZE_CHARGE;
    if volley >= 0 && volley % BLAZE_VOLLEY == 0 {
        let distance = target.eye.distance(eye);
        let spread = distance.sqrt() * 0.5;
        let mut jitter = || f64::from(random() - random()) * spread * 0.2;
        let middle = (target.aabb.min + target.aabb.max) / 2.0;
        intent.small_fireball = Some(Shot {
            from: feet + DVec3::Y * (size.1 / 2.0),
            at: middle + DVec3::new(jitter(), 0.0, jitter()),
        });
        if volley >= BLAZE_VOLLEY * 2 {
            state.charge = -BLAZE_REST;
        }
    }
    mob.aggressive = mob.ai.charge > 0;
    let above = target.eye + DVec3::Y;
    if above.distance(eye) > 4.0 {
        Goal::Direct(above)
    } else {
        Goal::Stand(Some(target.eye))
    }
}

/// Shulkers: with the player in sight within 20 blocks, open the shell
/// after half a second and send a bullet after them every two seconds
/// while it is open; with nobody about, close up again. They never move.
fn shulker<T: Terrain>(mob: &mut Mob, view: &View<T>, intent: &mut Intent) -> Goal {
    let eye = mob.eye();
    let target = view
        .player
        .filter(|t| t.visible && t.eye.distance(eye) < SHULKER_RANGE);
    let Some(target) = target else {
        mob.ai.chasing = false;
        mob.ai.charge = mob.ai.charge.min(0) - 1;
        mob.ai.charge = mob.ai.charge.max(-SHULKER_REST);
        return Goal::Idle;
    };
    mob.ai.chasing = true;
    mob.ai.charge = mob.ai.charge.max(0) + 1;
    mob.aggressive = mob.ai.charge > SHULKER_OPEN;
    if mob.ai.charge > SHULKER_OPEN && (mob.ai.charge - SHULKER_OPEN) % SHULKER_VOLLEY == 1 {
        intent.bullet = Some(Shot {
            from: eye,
            at: (target.aabb.min + target.aabb.max) / 2.0,
        });
    }
    Goal::Stand(Some(target.eye))
}

/// How far a shulker sees, ticks before it opens, and between bullets.
const SHULKER_RANGE: f64 = 20.0;
const SHULKER_OPEN: i32 = 10;
const SHULKER_VOLLEY: i32 = 40;
const SHULKER_REST: i32 = 200;

/// Piglins: while admiring gold they stand still; otherwise they go after
/// the player (modern `PiglinAi` spares players wearing gold, but armor
/// can't be worn here yet).
fn piglin<T: Terrain>(
    mob: &mut Mob,
    def: &MobDef,
    view: &View<T>,
    attack: f32,
    intent: &mut Intent,
) -> Goal {
    if mob.data.timer > 0 {
        mob.ai.chasing = false;
        return Goal::Stand(view.player.map(|t| t.eye));
    }
    melee(mob, def, view, attack, true, intent)
}

/// Squid and fish swim about the water they're in; out of it they flop.
fn swimmer<T: Terrain>(mob: &mut Mob, view: &View<T>, random: &mut impl FnMut() -> f32) -> Goal {
    let feet = mob.feet();
    let state = &mut mob.ai;
    if state.fleeing > 0
        && let Some(from) = state.destination
    {
        return Goal::Flee(from);
    }
    let arrived = state
        .destination
        .is_none_or(|d| d.distance(feet) < 1.0 || random() < 1.0 / 100.0);
    if arrived && view.env.in_water {
        let offset = DVec3::new(
            (random() as f64 - 0.5) * 12.0,
            (random() as f64 - 0.5) * 4.0,
            (random() as f64 - 0.5) * 12.0,
        );
        let spot = feet + offset;
        let cell = spot.floor().as_ivec3();
        state.destination = view.terrain.water(cell).then_some(spot);
    }
    match state.destination {
        Some(spot) if view.env.in_water => Goal::Direct(spot),
        _ => Goal::Idle,
    }
}

/// Wolves: wild ones leave the player alone unless hurt; tame ones follow
/// the player, sit when told and go after the player's enemies.
fn wolf<T: Terrain>(
    mob: &mut Mob,
    def: &MobDef,
    view: &View<T>,
    attack: f32,
    intent: &mut Intent,
) -> Goal {
    let feet = mob.feet();
    let size = mob.size();
    if mob.data.tamed {
        if mob.data.sitting {
            return Goal::Stand(None);
        }
        if let Some(index) = view.threat
            && let Some(other) = view.others.iter().find(|o| o.index == index)
            && other.feet.distance(feet) < THREAT_RANGE * 2.0
        {
            mob.aggressive = true;
            if mob.ai.attack_cooldown == 0 && reaches(def, feet, size, other.feet, other.width) {
                intent.hit = Some(Strike {
                    victim: Victim::Mob(other.index),
                    damage: attack,
                });
                mob.ai.attack_cooldown = ATTACK_COOLDOWN;
            }
            return Goal::Chase(other.feet);
        }
        if let Some(owner) = view.owner {
            let d = owner.distance(feet);
            if d > FOLLOW_START {
                return Goal::Chase(owner);
            }
            if d > FOLLOW_STOP && mob.ai.path.len() > 1 {
                return Goal::Chase(owner);
            }
        }
        return Goal::Idle;
    }
    // Wild: neutral toward the player, but not toward sheep... keep it simple.
    if mob.ai.angry > 0 {
        let goal = melee(mob, def, view, attack, true, intent);
        mob.data.variant = 2;
        return goal;
    }
    mob.data.variant = 0;
    Goal::Idle
}

/// Cats follow a tame owner and sit when told; wild ones keep away from
/// the player.
fn cat<T: Terrain>(mob: &mut Mob, view: &View<T>) -> Goal {
    let feet = mob.feet();
    if mob.data.tamed {
        if mob.data.sitting {
            return Goal::Stand(None);
        }
        if let Some(owner) = view.owner
            && owner.distance(feet) > FOLLOW_START
        {
            return Goal::Chase(owner);
        }
        return Goal::Idle;
    }
    if let Some(player) = view.owner
        && player.distance(feet) < 6.0
    {
        return Goal::Flee(player);
    }
    Goal::Idle
}

/// Bees fly about at random and sting a player they're angry at.
fn bee<T: Terrain>(
    mob: &mut Mob,
    def: &MobDef,
    view: &View<T>,
    attack: f32,
    random: &mut impl FnMut() -> f32,
    intent: &mut Intent,
) -> Goal {
    let feet = mob.feet();
    let size = mob.size();
    if mob.ai.angry > 0
        && let Some(target) = &view.player
    {
        mob.aggressive = true;
        let width = target.aabb.max.x - target.aabb.min.x;
        if mob.ai.attack_cooldown == 0 && reaches(def, feet, size, target.feet, width + 0.5) {
            intent.hit = Some(Strike {
                victim: Victim::Player,
                damage: attack,
            });
            mob.ai.attack_cooldown = ATTACK_COOLDOWN;
        }
        return Goal::Direct(target.eye);
    }
    let state = &mut mob.ai;
    if state.destination.is_none_or(|d| d.distance(feet) < 1.0) || random() < 1.0 / 100.0 {
        let offset = DVec3::new(
            (random() as f64 - 0.5) * 16.0,
            (random() as f64 - 0.4) * 6.0,
            (random() as f64 - 0.5) * 16.0,
        );
        let spot = feet + offset;
        state.destination = view
            .terrain
            .passable(spot.floor().as_ivec3())
            .then_some(spot);
    }
    state.destination.map_or(Goal::Idle, Goal::Direct)
}

/// Foxes hunt chickens and rabbits they see, and keep away from the player.
fn fox<T: Terrain>(
    mob: &mut Mob,
    def: &MobDef,
    view: &View<T>,
    attack: f32,
    intent: &mut Intent,
) -> Goal {
    let feet = mob.feet();
    let size = mob.size();
    if let Some(prey) = view
        .others
        .iter()
        .filter(|o| o.prey && o.visible && o.feet.distance(feet) < 12.0)
        .min_by(|a, b| {
            a.feet
                .distance_squared(feet)
                .total_cmp(&b.feet.distance_squared(feet))
        })
    {
        mob.aggressive = true;
        if mob.ai.attack_cooldown == 0 && reaches(def, feet, size, prey.feet, prey.width) {
            intent.hit = Some(Strike {
                victim: Victim::Mob(prey.index),
                damage: attack,
            });
            mob.ai.attack_cooldown = ATTACK_COOLDOWN;
        }
        return Goal::Chase(prey.feet);
    }
    if let Some(player) = view.owner
        && player.distance(feet) < 8.0
    {
        return Goal::Flee(player);
    }
    Goal::Idle
}

/// Turns a goal into movement: paths toward it, turns, jumps.
fn steer<T: Terrain>(
    mob: &mut Mob,
    def: &MobDef,
    view: &View<T>,
    goal: Goal,
    height: i32,
    random: &mut impl FnMut() -> f32,
    intent: &mut Intent,
) {
    let feet = mob.feet();
    let cell = mob.cell();
    let mut face = None;
    let mut direct = None;
    let mut flee_from = None;
    let mut hold = false;
    let mut strafe: Option<(f64, f64)> = None;
    match goal {
        Goal::Idle => {}
        Goal::Chase(point) => {
            face = Some(point);
            let state = &mut mob.ai;
            if mob.body.liquid.is_some() && def.ai.is_monster() {
                direct = Some(point);
            } else if state.path.is_empty() || random() < 0.05 {
                let goal = (point + DVec3::Y * 0.5).floor().as_ivec3();
                state.path = find_path(view.terrain, cell, goal, height, NOTICE_RANGE as f32);
            }
        }
        Goal::Face(point) => face = Some(point),
        Goal::Flee(point) => {
            flee_from = Some(point);
            face = Some(feet + (feet - point));
        }
        Goal::Strafe {
            around,
            clockwise,
            back,
        } => {
            face = Some(around);
            mob.ai.path.clear();
            let side = if clockwise { 0.5 } else { -0.5 };
            strafe = Some((if back { -0.5 } else { 0.5 }, side));
        }
        Goal::Direct(point) => {
            direct = Some(point);
            face = Some(point);
        }
        Goal::Stand(point) => {
            hold = true;
            face = point;
            mob.ai.path.clear();
        }
    }

    let idle = matches!(goal, Goal::Idle);
    if idle && !mob.data.sitting {
        let state = &mut mob.ai;
        let wants = state.path.is_empty() && random() < 1.0 / 180.0;
        let restless = random() < 1.0 / 120.0;
        let running = state.fleeing > 0 && state.path.is_empty();
        if (wants || restless || running) && !def.flies && !def.swims {
            wander(mob, height, view, random);
        }
    }
    if let Some(from) = flee_from {
        let away = feet + (feet - from).with_y(0.0).normalize_or_zero() * 8.0;
        let goal_cell = away.floor().as_ivec3();
        if mob.ai.path.is_empty() || random() < 0.05 {
            mob.ai.path = find_path(view.terrain, cell, goal_cell, height, WANDER_DISTANCE);
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
    let speed = if state.fleeing > 0 || flee_from.is_some() {
        def.speed * 2.0
    } else {
        def.speed
    };
    let mut heading = None;
    if hold {
        // Stand still.
    } else if let Some((forward, side)) = strafe {
        intent.input.forward = forward * speed * 2.0;
        intent.input.strafe = side * speed * 2.0;
    } else if let Some(point) = direct {
        let to = point - feet;
        if to.length() > 0.4 {
            intent.input.forward = speed;
            heading = Some(point);
            intent.rise = (to.y / to.length() * 1.5).clamp(-1.0, 1.0);
            if def.flies || def.swims {
                intent.input.jump = intent.rise > 0.2;
                intent.input.sneak = intent.rise < -0.2;
            } else {
                // Hops: on the ground only when it's time.
                let hopper = matches!(def.ai, Ai::Slime);
                if hopper {
                    if mob.body.on_ground {
                        if mob.ai.hop_delay == 0 {
                            intent.input.jump = true;
                            mob.ai.hop_delay = (random() * 20.0) as u32 + 10;
                        } else {
                            intent.input.forward = 0.0;
                        }
                    }
                } else {
                    intent.input.jump = to.y > 0.5;
                }
            }
        }
    } else if let Some(next) = state.path.first() {
        let center = next.as_dvec3() + DVec3::new(0.5, 0.0, 0.5);
        heading = Some(center);
        intent.input.forward = speed;
        intent.input.jump = next.y > (mob.body.aabb().min.y + 0.5).floor() as i32;
        // Rabbits hop rather than walk.
        if matches!(def.ai, Ai::Rabbit) {
            intent.input.jump = mob.body.on_ground && random() < 0.3;
        }
    } else if let Some(target) = face
        && (target - feet).with_y(0.0).length() > 1.0
        && !matches!(goal, Goal::Face(_) | Goal::Flee(_))
    {
        intent.input.forward = speed;
    }
    if let Some(toward) = face.or(heading) {
        let wanted = yaw_toward(feet, toward);
        mob.yaw += angle_difference(wanted, mob.yaw).clamp(-TURN, TURN);
    }
    mob.head_yaw = face.map_or(mob.yaw, |target| yaw_toward(feet, target));
    if mob.body.liquid.is_some() && !def.swims && random() < 0.8 {
        intent.input.jump = true;
    }
    if intent.input.forward > 0.0 && mob.body.collided_horizontally() {
        intent.input.jump = true;
    }
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

    pub(crate) fn registry() -> MobRegistry {
        MobRegistry::load(&crate::assets::dir().join("data/mobs.ron")).unwrap()
    }

    /// Runs `ticks` ticks of AI and movement; returns every intent.
    pub(crate) fn run_intents(
        mob: &mut Mob,
        def: &MobDef,
        grid: &Grid,
        player: Option<Target>,
        env: Env,
        others: &[Other],
        ticks: u32,
    ) -> Vec<Intent> {
        let mut seed = 0.37f32;
        let mut random = || {
            seed = (seed * 997.0 + 0.123).fract();
            seed
        };
        let weight = |_: IVec3| 0.0;
        let mut out = Vec::new();
        for _ in 0..ticks {
            let view = View {
                terrain: grid,
                player,
                weight: &weight,
                env,
                others,
                owner: player.map(|p| p.feet),
                threat: None,
                errand: None,
            };
            let intent = think(mob, def, &view, &mut random);
            mob.tick(&intent.input, grid, false);
            out.push(intent);
        }
        out
    }

    /// Runs ticks; returns the damage dealt to the player.
    fn run(mob: &mut Mob, def: &MobDef, grid: &Grid, player: Option<Target>, ticks: u32) -> f32 {
        run_intents(mob, def, grid, player, Env::default(), &[], ticks)
            .iter()
            .filter_map(|i| i.hit)
            .filter(|s| s.victim == Victim::Player)
            .map(|s| s.damage)
            .sum()
    }

    pub(crate) fn player_at(feet: DVec3) -> Target {
        Target {
            feet,
            eye: feet + DVec3::Y * 1.62,
            aabb: sized_hitbox(feet, 0.6, 1.8),
            visible: true,
            stares: false,
        }
    }

    #[test]
    fn a_zombie_chases_a_visible_player_and_hits_for_3_each_second() {
        let mobs = registry();
        let kind = mobs.find("zombie").unwrap();
        let def = mobs.get(kind);
        let grid = Grid::floor();
        let mut zombie = Mob::new(kind, def, DVec3::new(0.5, 64.0, 0.5), 0.0);
        let player = player_at(DVec3::new(8.5, 64.0, 0.5));
        let dealt = run(&mut zombie, def, &grid, Some(player), 200);
        assert!(
            zombie.feet().distance(player.feet) < 1.6,
            "caught up: {}",
            zombie.feet()
        );
        // Once in reach it hits every 20 ticks.
        assert!(dealt >= 3.0 * 4.0, "dealt {dealt}");
        assert_eq!(dealt % 3.0, 0.0);
    }

    #[test]
    fn a_zombie_ignores_a_player_it_may_not_attack() {
        let mobs = registry();
        let kind = mobs.find("zombie").unwrap();
        let def = mobs.get(kind);
        let mut zombie = Mob::new(kind, def, DVec3::new(0.5, 64.0, 0.5), 0.0);
        assert_eq!(run(&mut zombie, def, &Grid::floor(), None, 200), 0.0);
        assert!(!zombie.ai.chasing);
    }

    #[test]
    fn a_zombie_cannot_hit_through_a_wall_or_a_ceiling() {
        let mobs = registry();
        let kind = mobs.find("zombie").unwrap();
        let def = mobs.get(kind);
        let grid = Grid::floor();
        // The mob is within reach but has no clear line to the player.
        let mut zombie = Mob::new(kind, def, DVec3::new(0.5, 64.0, 0.5), 0.0);
        let mut player = player_at(DVec3::new(1.2, 64.0, 0.5));
        player.visible = false;
        assert_eq!(run(&mut zombie, def, &grid, Some(player), 100), 0.0);
        // The same, in the open.
        let mut zombie = Mob::new(kind, def, DVec3::new(0.5, 64.0, 0.5), 0.0);
        player.visible = true;
        assert!(run(&mut zombie, def, &grid, Some(player), 100) >= 3.0);
        // Too far to reach by a hair is out of reach too: farther than
        // (2 × width)² + the player's width apart.
        let mut zombie = Mob::new(kind, def, DVec3::new(0.5, 64.0, 0.5), 0.0);
        let near = player_at(DVec3::new(0.5, 64.0, 2.0));
        let intents = run_intents(&mut zombie, def, &grid, Some(near), Env::default(), &[], 1);
        assert!(intents[0].hit.is_none(), "1.5 blocks away is out of reach");
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
            1.0,
            Some(start + DVec3::X),
            crate::player::stats::DamageSource::Mob,
        );
        run(&mut pig, def, &grid, None, 40);
        assert!(pig.feet().distance(start) > 3.0, "ran off: {}", pig.feet());
        assert_eq!(pig.stats.health, 9.0);
    }
}
