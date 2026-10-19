//! Animals and the player: milking cows, shearing and dyeing sheep, hens
//! laying eggs and drifting down, sheep regrowing wool, taming wolves and
//! cats, telling them to sit, and saddling and riding horses.

use glam::DVec3;

use super::{Game, mob_acts::WOOL};
use crate::{
    entity::{item::ItemEntity, mob::Mob, mobs::Ai},
    inventory::{item::ItemId, items, stack::ItemStack},
    player::physics::MoveInput,
    world::{block::BlockState, blocks},
};

/// A hen's eggs come every five to ten minutes (modern: 6000 + up to 6000
/// ticks).
const EGG_BASE: u32 = 6000;
const EGG_EXTRA: u32 = 6000;
/// Chance a bone or fish tames a wolf or cat.
const TAME_CHANCE: f64 = 1.0 / 3.0;
/// A tamed wolf's health.
const TAMED_WOLF_HEALTH: f32 = 20.0;
/// A falling hen's vertical speed is cut to this each tick.
const HEN_GLIDE: f64 = 0.6;
/// How high above its feet a rider's feet sit, as a share of the mob's
/// height, less a little.
const SEAT_FRACTION: f64 = 0.75;
const SEAT_DROP: f64 = 0.2;
/// A horse takes to a rider after this many ticks of riding on average
/// (modern's temper growth), and bucks in the meantime.
const TAME_RIDE_TICKS: u32 = 20;

/// The dye number of an item that dyes wool, 0–15 in wool order.
pub fn dye_color(item: ItemId) -> Option<u8> {
    const DYES: [(&str, u8); 16] = [
        ("bone_meal", 0),
        ("orange_dye", 1),
        ("magenta_dye", 2),
        ("light_blue_dye", 3),
        ("yellow_dye", 4),
        ("lime_dye", 5),
        ("pink_dye", 6),
        ("gray_dye", 7),
        ("light_gray_dye", 8),
        ("cyan_dye", 9),
        ("purple_dye", 10),
        ("lapis_lazuli", 11),
        ("cocoa_beans", 12),
        ("green_dye", 13),
        ("red_dye", 14),
        ("ink_sac", 15),
    ];
    DYES.iter()
        .find(|(name, _)| items::by_name(name) == Some(item))
        .map(|&(_, color)| color)
}

impl Game {
    /// One tick of the things animals do on their own.
    pub(super) fn tick_animals(&mut self) {
        let defs = self.mob_defs.clone();
        let mut laid = Vec::new();
        let mut regrown = Vec::new();
        for (index, mob) in self.mobs.iter_mut().enumerate() {
            if mob.is_dead() {
                continue;
            }
            let def = defs.get(mob.kind);
            match def.name.as_str() {
                "chicken" => {
                    if !mob.body.on_ground && mob.body.velocity.y < 0.0 {
                        mob.body.velocity.y *= HEN_GLIDE;
                    }
                    if mob.data.timer > 0 {
                        mob.data.timer -= 1;
                    } else {
                        mob.data.timer = EGG_BASE;
                        laid.push(index);
                    }
                }
                "sheep" if mob.data.sheared => regrown.push(index),
                _ => {}
            }
        }
        for index in laid {
            let extra = self.rng.below(EGG_EXTRA);
            let mob = &mut self.mobs[index];
            mob.data.timer += extra;
            let at = mob.feet();
            if let Some(egg) = items::by_name("egg") {
                self.entities.push(ItemEntity::new(
                    ItemStack::new(egg, 1),
                    at + DVec3::Y * 0.2,
                    DVec3::new(0.0, 0.1, 0.0),
                    0.0,
                ));
            }
        }
        // A sheared sheep standing on grass now and then eats it and grows
        // its wool back.
        for index in regrown {
            if self.rng.below(1000) != 0 {
                continue;
            }
            let cell = self.mobs[index].cell() - glam::IVec3::Y;
            let on_grass = self
                .chunks
                .world()
                .block(cell.x, cell.y, cell.z)
                .is_some_and(|b| b.is(blocks::GRASS_BLOCK));
            if on_grass {
                self.edit(&[(cell, BlockState::of(blocks::DIRT))]);
                self.mobs[index].data.sheared = false;
            }
        }
        self.tick_riding_horse();
    }

    /// Right-click on mob `index`. Returns whether something happened (so
    /// a block behind it isn't also used).
    pub(super) fn use_mob(&mut self, index: usize) -> bool {
        let Some(mob) = self.mobs.get(index) else {
            return false;
        };
        if mob.is_dead() {
            return false;
        }
        let name = self.mob_defs.get(mob.kind).name.clone();
        let held = self.held().cloned();
        let survival = self.mode.uses_up_items();
        match name.as_str() {
            "cow" => {
                if held.is_some_and(|s| s.id == items::BUCKET) {
                    self.fill_held_bucket(items::MILK_BUCKET);
                    return true;
                }
                false
            }
            "sheep" => {
                if let Some(stack) = held {
                    if stack.id == items::SHEARS && !self.mobs[index].data.sheared {
                        self.shear_sheep(index);
                        return true;
                    }
                    if let Some(color) = dye_color(stack.id)
                        && !self.mobs[index].data.sheared
                        && self.mobs[index].data.color != color
                    {
                        self.mobs[index].data.color = color;
                        if survival {
                            self.inventory.take(self.held_slot(), 1);
                        }
                        return true;
                    }
                }
                false
            }
            "wolf" | "cat" => self.use_pet(index, held, survival),
            "horse" => self.use_horse(index, held, survival),
            _ => false,
        }
    }

    fn fill_held_bucket(&mut self, full: ItemId) {
        if !self.mode.uses_up_items() {
            return;
        }
        let slot = self.held_slot();
        self.inventory
            .set(slot, Some(ItemStack::with_damage(full, 1, 0)));
    }

    /// Shears a sheep: one to three wool of its color drop, and the sheep
    /// goes bare until it eats grass.
    fn shear_sheep(&mut self, index: usize) {
        let color = usize::from(self.mobs[index].data.color) % 16;
        self.mobs[index].data.sheared = true;
        let count = 1 + self.rng.below(3) as u8;
        if let Some(wool) = items::by_name(WOOL[color]) {
            let at = self.mobs[index].cell();
            let rng = &mut self.rng;
            let mut random = || rng.next_f64() as f32;
            self.entities.push(ItemEntity::from_block(
                ItemStack::new(wool, count),
                at,
                &mut random,
            ));
        }
        if self.mode.wears_tools() {
            let slot = self.held_slot();
            self.inventory.damage_slot(slot, 1, &self.items);
        }
    }

    /// Wolves take bones and cats raw fish to tame; a tame one sits or gets
    /// up with a plain right-click.
    fn use_pet(&mut self, index: usize, held: Option<ItemStack>, survival: bool) -> bool {
        let kind = self.mobs[index].kind;
        let def = self.mob_defs.get(kind);
        let treat = match def.ai {
            Ai::Wolf { .. } => items::BONE,
            _ => items::COD,
        };
        let treat_ok = |id: ItemId| {
            id == treat || (matches!(def.ai, Ai::Cat) && Some(id) == items::by_name("salmon"))
        };
        let tamed = self.mobs[index].data.tamed;
        if let Some(stack) = &held
            && treat_ok(stack.id)
            && !tamed
        {
            if survival {
                self.inventory.take(self.held_slot(), 1);
            }
            if self.rng.next_f64() < TAME_CHANCE {
                let mob = &mut self.mobs[index];
                mob.data.tamed = true;
                mob.data.sitting = true;
                if matches!(self.mob_defs.get(kind).ai, Ai::Wolf { .. }) {
                    mob.data.variant = 1;
                    mob.stats.health = TAMED_WOLF_HEALTH;
                }
            }
            return true;
        }
        if tamed && held.as_ref().is_none_or(|s| !treat_ok(s.id)) {
            let mob = &mut self.mobs[index];
            mob.data.sitting = !mob.data.sitting;
            return true;
        }
        false
    }

    /// Sitting on a horse, or saddling a tame one.
    fn use_horse(&mut self, index: usize, held: Option<ItemStack>, survival: bool) -> bool {
        if let Some(stack) = held
            && stack.id == items::SADDLE
            && !self.mobs[index].data.saddled
            && self.mobs[index].data.tamed
        {
            self.mobs[index].data.saddled = true;
            if survival {
                self.inventory.take(self.held_slot(), 1);
            }
            return true;
        }
        if self.mounted.is_none() && self.riding.is_none() {
            self.mounted = Some(self.mobs[index].id);
            self.mobs[index].ridden = true;
            self.mobs[index].data.timer = 0;
            return true;
        }
        false
    }

    /// The mob the player sits on.
    fn mount_index(&self) -> Option<usize> {
        let id = self.mounted?;
        self.mobs.iter().position(|m| m.id == id)
    }

    /// A wild horse gets used to its rider over a few seconds, bucking
    /// until then.
    fn tick_riding_horse(&mut self) {
        let Some(index) = self.mount_index() else {
            return;
        };
        let mob = &mut self.mobs[index];
        if mob.data.tamed {
            return;
        }
        mob.data.timer += 1;
        if mob.data.timer.is_multiple_of(TAME_RIDE_TICKS) {
            mob.data.temper = mob.data.temper.saturating_add(5);
            let temper = u32::from(mob.data.temper);
            if self.rng.below(100) < temper {
                self.mobs[index].data.tamed = true;
            } else if self.rng.below(3) == 0 {
                // It throws its rider.
                self.dismount_mob(index, true);
            }
        }
    }

    /// Gets the player off the mob they ride; `thrown` hurls them a little.
    pub(super) fn dismount_mob(&mut self, index: usize, thrown: bool) {
        let mob = &mut self.mobs[index];
        mob.ridden = false;
        let feet = mob.feet() + DVec3::new(1.0, 0.1, 0.0);
        self.mounted = None;
        self.body.set_position(feet);
        self.prev_feet = feet;
        self.body.velocity = if thrown {
            DVec3::new(0.3, 0.3, 0.0)
        } else {
            DVec3::ZERO
        };
    }

    /// The player's tick while sitting on a mob.
    pub(super) fn ride_mob(&mut self) {
        let Some(index) = self.mount_index() else {
            self.mounted = None;
            return;
        };
        if self.keys.sneak || self.mobs[index].is_dead() {
            self.dismount_mob(index, false);
            return;
        }
        let mob = &self.mobs[index];
        let height = mob.size().1;
        let seat = mob.feet() + DVec3::Y * (height * SEAT_FRACTION - SEAT_DROP);
        self.body.set_position(seat);
        self.body.velocity = DVec3::ZERO;
    }

    /// Turns a ridden mob to where the player looks.
    pub(super) fn steer_ridden(&self, mob: &mut Mob) {
        mob.yaw = self.camera.yaw;
        mob.head_yaw = self.camera.yaw;
    }

    /// What the keys ask of the mob being ridden: a saddled horse goes where
    /// told and jumps; a wild one only stands bucking.
    pub(super) fn ridden_input(&self) -> MoveInput {
        let Some(index) = self.mount_index() else {
            return MoveInput::default();
        };
        let mob = &self.mobs[index];
        if !mob.data.saddled {
            return MoveInput::default();
        }
        let keys = self.keys.move_input();
        let speed = self.mob_defs.get(mob.kind).speed;
        MoveInput {
            forward: keys.forward * speed,
            strafe: keys.strafe * speed * 0.5,
            jump: keys.jump,
            ..MoveInput::default()
        }
    }
}
