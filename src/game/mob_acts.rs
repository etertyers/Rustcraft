//! What mobs do besides walking, carried out in the world: striking the
//! player or other mobs, loosing arrows, creepers going off, spiders
//! leaping, endermen teleporting and moving blocks, and what falls from a
//! mob that dies (its drops, a sheep's wool, a slime's brood).

use glam::{DVec3, IVec3};

use super::{
    Game,
    combat::{Outcome, Strike},
    mobs::WorldTerrain,
};
use crate::{
    entity::{
        ai::{BlockAction, Intent, Teleport, Victim},
        arrow::Arrow,
        item::ItemEntity,
        mob::{DEATH_TICKS, Mob, MobData, knock_back, push},
        mobs::Ai,
        path::Terrain,
    },
    inventory::{enchant::Enchantment, items, stack::ItemStack},
    player::stats::DamageSource,
    world::block::BlockState,
};

/// Arrows from skeletons fly at this share of a full bow's speed (modern
/// 1.6 blocks a tick against a bow's 3).
const MOB_ARROW_POWER: f32 = 1.6 / 3.0;
/// Witches' missiles are slower.
const WITCH_POWER: f32 = 0.5;
/// Blocks an arrow arcs up per block of distance, so shots carry (modern
/// adds 0.2 per block).
const ARROW_LOB: f64 = 0.2;
/// Endermen look for a block within this far, and for a place to
/// teleport within this far.
const TAKE_REACH: i32 = 2;
const TELEPORT_RANGE: f64 = 32.0;
/// Wool colors by dye number, in vanilla's order.
pub const WOOL: [&str; 16] = [
    "white_wool",
    "orange_wool",
    "magenta_wool",
    "light_blue_wool",
    "yellow_wool",
    "lime_wool",
    "pink_wool",
    "gray_wool",
    "light_gray_wool",
    "cyan_wool",
    "purple_wool",
    "blue_wool",
    "brown_wool",
    "green_wool",
    "red_wool",
    "black_wool",
];
/// Blocks an enderman may pick up (modern `#enderman_holdable`, as far as
/// they exist here).
const HOLDABLE: [&str; 15] = [
    "grass_block",
    "dirt",
    "sand",
    "red_sand",
    "gravel",
    "clay",
    "dandelion",
    "poppy",
    "brown_mushroom",
    "red_mushroom",
    "tnt",
    "cactus",
    "pumpkin",
    "melon",
    "mycelium",
];

impl Game {
    /// Carries out what each mob decided this tick (`pending` is by index
    /// into `mobs`).
    pub(super) fn apply_intents(
        &mut self,
        mobs: &mut [Mob],
        pending: Vec<(usize, Intent)>,
        player_feet: DVec3,
    ) {
        for (index, intent) in pending {
            if mobs[index].is_dead() {
                continue;
            }
            let feet = mobs[index].feet();
            if let Some(leap) = intent.leap {
                mobs[index].body.velocity = leap;
            }
            if let Some(hit) = intent.hit {
                match hit.victim {
                    Victim::Player => self.mob_strikes_player(
                        &mut mobs[index],
                        hit.damage,
                        player_feet,
                        intent.toss,
                    ),
                    Victim::Mob(victim) => {
                        if victim < mobs.len() && victim != index {
                            mobs[victim].hurt(hit.damage, Some(feet), DamageSource::Mob);
                        }
                    }
                }
            }
            if let Some(shot) = intent.shot {
                self.mob_shoots(&mobs[index], shot.from, shot.at);
            }
            if let Some(shot) = intent.fireball {
                self.loose_fireball(shot.from, shot.at);
            }
            if let Some(tp) = intent.teleport {
                self.teleport_mob(&mut mobs[index], tp);
            }
            if let Some(action) = intent.block {
                self.enderman_block(&mut mobs[index], action);
            }
            if let Some(power) = intent.explode {
                let mob = &mut mobs[index];
                mob.looted = true;
                mob.stats.health = 0.0;
                mob.death_time = DEATH_TICKS;
                let center = mob.feet() + DVec3::Y * (mob.size().1 / 2.0);
                self.explode(center, power);
            }
        }
    }

    /// `toss`: the blow throws the player up too (a hoglin's, modern
    /// `HoglinBase.throwTarget`).
    fn mob_strikes_player(&mut self, mob: &mut Mob, damage: f32, player_feet: DVec3, toss: bool) {
        let strike = Strike {
            amount: damage,
            source: DamageSource::Mob,
            from: Some(mob.feet()),
            ranged: false,
            axe: false,
        };
        match self.hurt_player(strike) {
            Outcome::Hurt { fresh: true } => {
                knock_back(&mut self.body, mob.feet());
                if toss {
                    let lift = 0.25 + self.rng.next_f64() * 0.25;
                    self.body.velocity.y = self.body.velocity.y.max(0.0) + lift;
                }
                // Tame wolves take up the fight against whatever hit.
                self.threat_id = mob.id;
                self.threat_ticks = 100;
            }
            // A shield shoves the attacker back.
            Outcome::Blocked => {
                let away = mob.feet() - player_feet;
                push(&mut mob.body, crate::config::combat::SHIELD_KNOCKBACK, away);
            }
            _ => {}
        }
    }

    /// An arrow from a mob's eyes toward `at`, lobbed a little to carry.
    fn mob_shoots(&mut self, mob: &Mob, from: DVec3, at: DVec3) {
        let delta = at - from;
        let flat = delta.x.hypot(delta.z);
        let aim = (delta + DVec3::Y * flat * ARROW_LOB).normalize_or_zero();
        if aim == DVec3::ZERO {
            return;
        }
        let power = if matches!(self.mob_defs.get(mob.kind).ai, Ai::Witch) {
            WITCH_POWER
        } else {
            MOB_ARROW_POWER
        };
        let rng = &mut self.rng;
        let mut random = || rng.next_f64();
        let mut arrow = Arrow::shoot(from, aim, power, DVec3::ZERO, false, &mut random);
        arrow.hostile = true;
        arrow.critical = false;
        self.arrows.push(arrow);
    }

    /// Moves `mob` to a spot it can stand on, within reach of where it
    /// asked for.
    fn teleport_mob(&mut self, mob: &mut Mob, kind: Teleport) {
        let feet = mob.feet();
        for _ in 0..16 {
            let (x, y, z) = match kind {
                Teleport::Random => (
                    feet.x + (self.rng.next_f64() - 0.5) * TELEPORT_RANGE * 2.0,
                    feet.y + (self.rng.next_f64() - 0.5) * 16.0,
                    feet.z + (self.rng.next_f64() - 0.5) * TELEPORT_RANGE * 2.0,
                ),
                Teleport::Near(point) => (
                    point.x + (self.rng.next_f64() - 0.5) * 16.0,
                    point.y + (self.rng.next_f64() - 0.5) * 6.0,
                    point.z + (self.rng.next_f64() - 0.5) * 16.0,
                ),
            };
            let mut cell = DVec3::new(x, y, z).floor().as_ivec3();
            let terrain = WorldTerrain {
                world: self.chunks.world(),
                blocks: self.items.blocks,
            };
            // Drop to the ground below.
            let mut dropped = 0;
            while !terrain.water(cell)
                && terrain.passable(cell - IVec3::Y)
                && cell.y > 1
                && dropped < 32
            {
                cell -= IVec3::Y;
                dropped += 1;
            }
            let ground = !terrain.passable(cell - IVec3::Y);
            let free = (0..3).all(|dy| {
                let c = cell + IVec3::Y * dy;
                terrain.passable(c) && !terrain.water(c) && !terrain.lava(c)
            });
            if ground && free {
                mob.body
                    .set_position(cell.as_dvec3() + DVec3::new(0.5, 0.0, 0.5));
                mob.body.velocity = DVec3::ZERO;
                mob.clear_path();
                return;
            }
        }
    }

    /// An enderman takes a block it stands near, or sets down the one it
    /// holds.
    fn enderman_block(&mut self, mob: &mut Mob, action: BlockAction) {
        let feet = mob.feet().floor().as_ivec3();
        let reach = TAKE_REACH * 2 + 1;
        let cell = IVec3::new(
            feet.x - TAKE_REACH + self.rng.below(reach as u32) as i32,
            feet.y + self.rng.below(3) as i32,
            feet.z - TAKE_REACH + self.rng.below(reach as u32) as i32,
        );
        let world = self.chunks.world();
        let Some(here) = world.block(cell.x, cell.y, cell.z) else {
            return;
        };
        let below = world.block(cell.x, cell.y - 1, cell.z);
        match action {
            BlockAction::Take => {
                if HOLDABLE.contains(&here.id().name()) {
                    mob.carrying = Some(here);
                    self.edit(&[(cell, BlockState::AIR)]);
                }
            }
            BlockAction::Place => {
                let Some(held) = mob.carrying else {
                    return;
                };
                let solid_below = below.is_some_and(|b| b.def().solid);
                if here.is_air() && solid_below {
                    self.edit(&[(cell, held)]);
                    mob.carrying = None;
                }
            }
        }
    }

    /// Everything a dead mob leaves: its drops, a sheep's wool, the block
    /// an enderman held, and a slime's smaller slimes.
    pub(super) fn on_mob_death(&mut self, mob: &Mob) {
        let def = self.mob_defs.get(mob.kind);
        let cell = mob.cell();
        // Looting counts when the player made the kill.
        let looting = if mob.player_hit > 0 {
            self.held_level(Enchantment::Looting)
        } else {
            0
        };
        let mut stacks = {
            let rng = &mut self.rng;
            let mut random = || rng.next_f64() as f32;
            mob.drops(def, looting, &mut random)
        };
        if def.name == "sheep"
            && !mob.data.sheared
            && let Some(wool) = items::by_name(WOOL[usize::from(mob.data.color) % 16])
        {
            stacks.push(ItemStack::new(wool, 1));
        }
        if let Some(block) = mob.carrying
            && let Some(item) = self.items.items.of_block(block.id())
        {
            stacks.push(ItemStack::new(item, 1));
        }
        let rng = &mut self.rng;
        let mut random = || rng.next_f64() as f32;
        for stack in stacks {
            self.entities
                .push(ItemEntity::from_block(stack, cell, &mut random));
        }
        let points = self.mob_xp(mob);
        self.spawn_xp(points, mob.feet() + DVec3::Y * (mob.size().1 / 2.0));
        // Slimes break into smaller ones.
        if mob.data.size > 1 {
            let size = mob.data.size / 2;
            let count = 2 + self.rng.below(3);
            for i in 0..count {
                let offset = DVec3::new(
                    (f64::from(i % 2) - 0.5) * f64::from(mob.data.size) / 4.0,
                    0.5,
                    (f64::from(i / 2) - 0.5) * f64::from(mob.data.size) / 4.0,
                );
                let data = MobData {
                    size,
                    ..Default::default()
                };
                let yaw = self.rng.next_f64() as f32 * std::f32::consts::TAU;
                let child = Mob::with_data(
                    mob.kind,
                    self.mob_defs.get(mob.kind),
                    mob.feet() + offset,
                    yaw,
                    data,
                );
                self.add_mob(child);
            }
        }
    }
}
