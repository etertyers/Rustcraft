//! Animals and the player: milking cows, shearing and dyeing sheep, hens
//! laying eggs and drifting down, sheep regrowing wool, taming wolves and
//! cats, telling them to sit, and saddling and riding horses.

use crate::config::farming as cfg;
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

/// What each animal is fed to breed it (vanilla's foods).
const BREEDING_FOOD: [(&str, &[&str]); 9] = [
    ("cow", &["wheat"]),
    ("sheep", &["wheat"]),
    ("pig", &["carrot", "potato", "beetroot"]),
    (
        "chicken",
        &[
            "wheat_seeds",
            "beetroot_seeds",
            "melon_seeds",
            "pumpkin_seeds",
        ],
    ),
    ("rabbit", &["dandelion", "carrot", "golden_carrot"]),
    (
        "wolf",
        &[
            "beef",
            "porkchop",
            "chicken",
            "mutton",
            "rabbit",
            "cooked_beef",
            "cooked_porkchop",
            "cooked_chicken",
            "cooked_mutton",
            "cooked_rabbit",
            "rotten_flesh",
        ],
    ),
    ("cat", &["cod", "salmon"]),
    ("horse", &["golden_apple", "golden_carrot"]),
    (
        "bee",
        &[
            "dandelion",
            "poppy",
            "blue_orchid",
            "allium",
            "azure_bluet",
            "red_tulip",
            "orange_tulip",
            "white_tulip",
            "pink_tulip",
            "oxeye_daisy",
            "cornflower",
            "lily_of_the_valley",
            "sunflower",
            "lilac",
            "rose_bush",
            "peony",
        ],
    ),
];

/// The fox's food, apart: a tuple array with ten rows would hold it too.
const FOX_FOOD: &str = "sweet_berries";

/// Whether `item` is the breeding food of the mob called `mob`.
fn is_breeding_food(mob: &str, item: ItemId) -> bool {
    if mob == "fox" {
        return items::by_name(FOX_FOOD) == Some(item);
    }
    BREEDING_FOOD
        .iter()
        .filter(|(name, _)| *name == mob)
        .flat_map(|(_, foods)| foods.iter())
        .any(|food| items::by_name(food) == Some(item))
}

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
        self.mate_animals();
        let defs = self.mob_defs.clone();
        let mut laid = Vec::new();
        let mut regrown = Vec::new();
        for (index, mob) in self.mobs.iter_mut().enumerate() {
            if mob.is_dead() {
                continue;
            }
            let def = defs.get(mob.kind);
            mob.data.love = mob.data.love.saturating_sub(1);
            mob.data.breed_cooldown = mob.data.breed_cooldown.saturating_sub(1);
            if mob.data.age < 0 {
                mob.data.age += 1;
            }
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
            crate::audio::play_at(
                "entity.chicken.egg",
                at,
                1.0,
                (self.rng.next_f64() as f32 - self.rng.next_f64() as f32) * 0.2 + 1.0,
            );
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
    /// Animals in love walk to a partner and make a baby; animals follow
    /// a player holding their food.
    fn mate_animals(&mut self) {
        let defs = self.mob_defs.clone();
        let players: Vec<(DVec3, Option<ItemId>)> = (0..self.player_count())
            .filter(|&i| self.player_at(i).dimension == self.dimension)
            .map(|i| {
                let p = self.player_at(i);
                (self.feet_of(i), p.inventory.selected_stack().map(|s| s.id))
            })
            .collect();
        let mut pairs = Vec::new();
        for i in 0..self.mobs.len() {
            let (kind, feet, love, adult, ready) = {
                let m = &self.mobs[i];
                (
                    m.kind,
                    m.feet(),
                    m.data.love,
                    m.data.age >= 0,
                    m.data.breed_cooldown == 0,
                )
            };
            if self.mobs[i].is_dead() || defs.get(kind).name == "villager" {
                continue;
            }
            let name = defs.get(kind).name.as_str();
            let mut toward = None;
            if love > 0 && adult && ready {
                let partner = (0..self.mobs.len())
                    .filter(|&j| j != i)
                    .filter(|&j| {
                        let o = &self.mobs[j];
                        o.kind == kind && o.data.love > 0 && o.data.age >= 0 && !o.is_dead()
                    })
                    .map(|j| (j, self.mobs[j].feet()))
                    .filter(|(_, f)| f.distance(feet) <= cfg::MATE_RANGE)
                    .min_by(|a, b| a.1.distance(feet).total_cmp(&b.1.distance(feet)));
                if let Some((j, there)) = partner {
                    if there.distance(feet) < 1.5 {
                        if i < j {
                            pairs.push((i, j));
                        }
                    } else {
                        toward = Some(there);
                    }
                }
            }
            if toward.is_none() {
                toward = players
                    .iter()
                    .filter(|(_, held)| held.is_some_and(|h| is_breeding_food(name, h)))
                    .map(|(p, _)| *p)
                    .filter(|p| p.distance(feet) < cfg::MATE_RANGE && p.distance(feet) > 2.0)
                    .min_by(|a, b| a.distance(feet).total_cmp(&b.distance(feet)));
            }
            if let Some(there) = toward {
                let d = (there - feet).with_y(0.0).normalize_or_zero();
                let mob = &mut self.mobs[i];
                mob.body.velocity.x += d.x * 0.02;
                mob.body.velocity.z += d.z * 0.02;
                mob.yaw = d.x.atan2(-d.z) as f32;
            }
        }
        for (a, b) in pairs {
            self.make_baby(a, b);
        }
    }

    /// The pair's baby, their cooldown and a little experience.
    fn make_baby(&mut self, a: usize, b: usize) {
        let name = self.mob_defs.get(self.mobs[a].kind).name.clone();
        let (fa, fb) = (self.mobs[a].feet(), self.mobs[b].feet());
        let mid = (fa + fb) / 2.0;
        for i in [a, b] {
            self.mobs[i].data.love = 0;
            self.mobs[i].data.breed_cooldown = cfg::BREED_COOLDOWN;
        }
        let variant = self.mobs[a].data.variant;
        if let Some(baby) = self.summon(&name, mid, 0.0) {
            baby.data.age = -cfg::BABY_TICKS;
            baby.data.variant = variant;
        }
        let points = 1 + self.rng.below(7);
        self.spawn_xp(points, mid);
    }

    pub(super) fn use_mob(&mut self, index: usize) -> bool {
        let Some(mob) = self.mobs.get(index) else {
            return false;
        };
        if mob.is_dead() {
            return false;
        }
        let name = self.mob_defs.get(mob.kind).name.clone();
        let held = self.held().cloned();
        let survival = self.player.mode.uses_up_items();
        // Breeding food goes to the animal, not into the player's mouth.
        // (A wolf has to be tamed first.)
        if let Some(stack) = &held
            && is_breeding_food(&name, stack.id)
            && (name != "wolf" || self.mobs[index].data.tamed)
        {
            let data = &mut self.mobs[index].data;
            if data.age < 0 {
                // A baby grows up sooner: a tenth of what is left off.
                data.age += (-data.age as f32 * cfg::FEED_SPEEDUP) as i32;
            } else if data.love == 0 && data.breed_cooldown == 0 {
                data.love = cfg::LOVE_TICKS;
            } else {
                return false;
            }
            if survival {
                self.player.inventory.take(self.held_slot(), 1);
            }
            return true;
        }
        match name.as_str() {
            "villager" => self.talk_to_villager(index),
            "cow" => {
                if held.is_some_and(|s| s.id == items::BUCKET) {
                    crate::audio::play("entity.cow.milk", self.mobs[index].feet());
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
                            self.player.inventory.take(self.held_slot(), 1);
                        }
                        return true;
                    }
                }
                false
            }
            "cod" | "salmon" => {
                // A bucket scoops the fish up (Tactical Fishing).
                if held.is_some_and(|s| s.id == items::BUCKET)
                    && let Some(full) = items::by_name(&format!("{name}_bucket"))
                {
                    self.fill_held_bucket(full);
                    self.mobs.remove(index);
                    self.note(crate::advancements::Event::Used("fish_bucket".to_owned()));
                    return true;
                }
                false
            }
            "wolf" | "cat" => self.use_pet(index, held, survival),
            "horse" => self.use_horse(index, held, survival),
            "strider" => self.use_strider(index, held),
            _ => false,
        }
    }

    fn fill_held_bucket(&mut self, full: ItemId) {
        if !self.player.mode.uses_up_items() {
            return;
        }
        let slot = self.held_slot();
        self.player
            .inventory
            .set(slot, Some(ItemStack::with_damage(full, 1, 0)));
    }

    /// Shears a sheep: one to three wool of its color drop, and the sheep
    /// goes bare until it eats grass.
    fn shear_sheep(&mut self, index: usize) {
        let color = usize::from(self.mobs[index].data.color) % 16;
        self.mobs[index].data.sheared = true;
        crate::audio::play("entity.sheep.shear", self.mobs[index].feet());
        self.note(crate::advancements::Event::Used("shear".to_owned()));
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
        if self.player.mode.wears_tools() {
            let slot = self.held_slot();
            self.player.inventory.damage_slot(slot, 1, &self.items);
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
                self.player.inventory.take(self.held_slot(), 1);
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
            if self.mobs[index].data.tamed {
                self.note(crate::advancements::Event::Used("tame".to_owned()));
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
        // Horse armor goes on a tame horse that has none.
        if let Some(stack) = &held {
            let armor = [
                items::LEATHER_HORSE_ARMOR,
                items::IRON_HORSE_ARMOR,
                items::GOLDEN_HORSE_ARMOR,
                items::DIAMOND_HORSE_ARMOR,
            ]
            .iter()
            .position(|&id| id == stack.id);
            if let Some(kind) = armor
                && self.mobs[index].data.tamed
                && self.mobs[index].data.armor == 0
            {
                self.mobs[index].data.armor = kind as u8 + 1;
                crate::audio::play("entity.horse.armor", self.mobs[index].feet());
                if survival {
                    self.player.inventory.take(self.held_slot(), 1);
                }
                return true;
            }
        }
        if let Some(stack) = held
            && stack.id == items::SADDLE
            && !self.mobs[index].data.saddled
            && self.mobs[index].data.tamed
        {
            self.mobs[index].data.saddled = true;
            crate::audio::play("entity.horse.saddle", self.mobs[index].feet());
            if survival {
                self.player.inventory.take(self.held_slot(), 1);
            }
            return true;
        }
        if self.player.mounted.is_none() && self.player.riding.is_none() {
            self.player.mounted = Some(self.mobs[index].id);
            self.mobs[index].ridden = true;
            self.mobs[index].data.timer = 0;
            return true;
        }
        false
    }

    /// The mob the player sits on.
    pub(super) fn mount_index(&self) -> Option<usize> {
        let id = self.player.mounted?;
        self.mobs.iter().position(|m| m.id == id)
    }

    /// A wild horse gets used to its rider over a few seconds, bucking
    /// until then.
    fn tick_riding_horse(&mut self) {
        let Some(index) = self.mount_index() else {
            return;
        };
        let horse = matches!(self.mob_defs.get(self.mobs[index].kind).ai, Ai::Horse);
        let mob = &mut self.mobs[index];
        // Only a wild horse bucks; pigs and striders carry anyone.
        if mob.data.tamed || !horse {
            return;
        }
        mob.data.timer += 1;
        if mob.data.timer.is_multiple_of(TAME_RIDE_TICKS) {
            mob.data.temper = mob.data.temper.saturating_add(5);
            let temper = u32::from(mob.data.temper);
            if self.rng.below(100) < temper {
                self.mobs[index].data.tamed = true;
                self.note(crate::advancements::Event::Used("tame".to_owned()));
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
        self.player.mounted = None;
        self.player.body.set_position(feet);
        self.player.prev_feet = feet;
        self.player.body.velocity = if thrown {
            DVec3::new(0.3, 0.3, 0.0)
        } else {
            DVec3::ZERO
        };
    }

    /// The player's tick while sitting on a mob.
    pub(super) fn ride_mob(&mut self) {
        let Some(index) = self.mount_index() else {
            self.player.mounted = None;
            return;
        };
        if self.player.keys.sneak || self.mobs[index].is_dead() {
            self.dismount_mob(index, false);
            return;
        }
        let mob = &self.mobs[index];
        let height = mob.size().1;
        let seat = mob.feet() + DVec3::Y * (height * SEAT_FRACTION - SEAT_DROP);
        self.player.body.set_position(seat);
        self.player.body.velocity = DVec3::ZERO;
    }

    /// Turns a ridden mob to where the player looks.
    pub(super) fn steer_ridden(&self, mob: &mut Mob) {
        mob.yaw = self.player.camera.yaw;
        mob.head_yaw = self.player.camera.yaw;
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
        let keys = self.player.keys.move_input();
        let def = self.mob_defs.get(mob.kind);
        let speed = def.speed;
        // A strider walks where its rider looks, keys or not.
        if matches!(def.ai, crate::entity::mobs::Ai::Strider) {
            return MoveInput {
                forward: speed,
                ..MoveInput::default()
            };
        }
        MoveInput {
            forward: keys.forward * speed,
            strafe: keys.strafe * speed * 0.5,
            jump: keys.jump,
            ..MoveInput::default()
        }
    }
}
