//! Using the selected item on the world: mining (with 1.0.0's breaking
//! times, drops and tool wear in survival, instantly in creative), placing
//! blocks from the stack, and throwing items.

use glam::{DVec3, IVec3};

use super::{BenchKind, Game};
use crate::{
    entity::{
        arrow::{self, Arrow, ArrowEvent},
        item::ItemEntity,
        particle::{MAX_PARTICLES, Particle},
    },
    inventory::{
        enchant::Enchantment,
        item::Tool,
        items,
        player::{HOTBAR_SLOTS, Hand},
        stack::ItemStack,
    },
    player::{
        interaction::{self, BREAK_REPEAT_TICKS, PLACE_REPEAT_TICKS},
        raycast::Hit,
        stats::DamageSource,
    },
    world::{
        block::{Activate, Fluid},
        harvest,
        props::{Prop, door_half},
    },
};

/// Exhaustion per block broken (modern `Block.playerDestroy`).
const BREAK_EXHAUSTION: f32 = 0.005;
/// Ticks of holding right-click to eat (1.0.0's `getMaxItemUseDuration`).
const EAT_TICKS: u32 = 32;
impl Game {
    /// Ticks a bow has been drawn, with the part of a tick.
    pub fn bow_ticks(&self) -> Option<f32> {
        self.drawing
            .map(|ticks| ticks as f32 + self.tick_alpha() as f32)
    }

    /// Eating or drawing a bow, which slows the player to a fifth.
    pub(super) fn using_item(&self) -> bool {
        self.eating.is_some() || self.drawing.is_some() || self.shield_hand.is_some()
    }

    /// The F key: swaps what the hands hold, ending whatever was in use.
    pub fn swap_hands(&mut self) {
        self.eating = None;
        self.drawing = None;
        self.shield_hand = None;
        self.shield_ticks = 0;
        self.inventory.swap_hands();
    }

    /// What the hand being used holds.
    pub(super) fn held(&self) -> Option<&ItemStack> {
        self.inventory.hand_stack(self.use_hand)
    }

    /// The slot of the hand being used.
    pub(super) fn held_slot(&self) -> usize {
        self.inventory.hand_slot(self.use_hand)
    }

    /// Whether `hand`'s item has something to do on a right-click now:
    /// raising a shield, drawing a bow, eating, pouring a bucket, striking
    /// flint, or placing a block against something looked at.
    fn hand_uses(&self, hand: Hand, hit: Option<&Hit>) -> bool {
        let Some(stack) = self.inventory.hand_stack(hand) else {
            return false;
        };
        let Some(item) = self.items.item(stack.id) else {
            return false;
        };
        if stack.id == items::SHIELD {
            return true;
        }
        if hand == Hand::Main && (item.bucket.is_some() || item.shoots.is_some()) {
            return true;
        }
        if Self::is_drink(stack.id)
            || matches!(
                stack.id,
                items::SPLASH_POTION
                    | items::GLASS_BOTTLE
                    | items::ENDER_EYE
                    | items::FIREWORK_ROCKET
            )
            || crate::inventory::player::armor_slot_of(&self.items, stack).is_some()
        {
            return true;
        }
        if let Some(food) = item.food {
            return self.mode.is_vulnerable() && (food.always_edible || self.stats.can_eat());
        }
        hit.is_some()
            && (matches!(
                stack.id,
                items::FLINT_AND_STEEL | items::FIRE_CHARGE | items::END_CRYSTAL
            ) || self.items.places(stack).is_some())
    }

    /// Whether `hand` holds something that shoots.
    fn is_bow(&self, hand: Hand) -> bool {
        self.inventory
            .hand_stack(hand)
            .and_then(|stack| self.items.item(stack.id))
            .is_some_and(|item| item.shoots.is_some())
    }

    /// The hand a right-click uses: the main hand if it has something to do,
    /// else the off-hand.
    fn pick_use_hand(&self, hit: Option<&Hit>) -> Option<Hand> {
        [Hand::Main, Hand::Off]
            .into_iter()
            .find(|&hand| self.hand_uses(hand, hit))
    }

    /// The block being mined and how far along it is (0–1), for the crack.
    pub fn mining_progress(&self) -> Option<(IVec3, f32)> {
        self.mining
    }

    /// The tool in the selected slot, if it's a tool.
    pub(super) fn selected_tool(&self) -> Option<&Tool> {
        let stack = self.inventory.selected_stack()?;
        self.items.tool(stack.id)
    }

    /// Breaks or places once per tick while a button is held.
    pub(super) fn use_hands(&mut self) {
        self.break_cooldown = self.break_cooldown.saturating_sub(1);
        self.place_cooldown = self.place_cooldown.saturating_sub(1);
        let wants_break = self.breaking || self.break_clicked;
        let using = self.placing || self.place_clicked;
        let eye = self.body.position() + DVec3::Y * self.body.eye_height();
        let hit = self.find_target(eye);
        let candidate = using.then(|| self.pick_use_hand(hit.as_ref())).flatten();
        // A shield stays up while its hand holds right-click, even while
        // the other hand attacks.
        self.keep_blocking(candidate);
        let hand = candidate.filter(|_| !wants_break);
        let bow_hand = hand.filter(|&h| self.is_bow(h));
        self.use_hand = match (bow_hand, self.drawing) {
            (Some(h), _) => h,
            (None, Some(_)) => self.drawing_hand,
            (None, None) => hand.unwrap_or(Hand::Main),
        };
        if self.keep_drawing(bow_hand.is_some()) {
            self.break_clicked = false;
            self.place_clicked = false;
            self.mining = None;
            return;
        }
        if let Some(h) = hand {
            self.use_hand = h;
        }
        if hand.is_some() && self.keep_eating() {
            self.break_clicked = false;
            self.place_clicked = false;
            self.mining = None;
            return;
        }
        // Buckets and bottles in a cauldron, bottles filled from water.
        if self.place_clicked && !wants_break && hit.is_some_and(|h| self.use_cauldron(h.block)) {
            self.view.swing();
            self.place_clicked = false;
            return;
        }
        if hand.is_some()
            && self.place_clicked
            && self.held().is_some_and(|s| s.id == items::GLASS_BOTTLE)
        {
            self.place_clicked = false;
            if self.fill_bottle(eye) {
                self.view.swing();
            }
            return;
        }
        // A splash potion is thrown, once per click.
        if hand.is_some()
            && self.place_clicked
            && self.held().is_some_and(|s| s.id == items::SPLASH_POTION)
        {
            let slot = self.held_slot();
            self.throw_potion(slot);
            self.break_clicked = false;
            self.place_clicked = false;
            return;
        }
        // A firework rocket boosts a glide, once per click.
        if hand.is_some()
            && self.place_clicked
            && self.held().is_some_and(|s| s.id == items::FIREWORK_ROCKET)
        {
            self.use_rocket();
            self.break_clicked = false;
            self.place_clicked = false;
            return;
        }
        // Armor is put on, once per click.
        if hand.is_some() && self.place_clicked && self.equip_held() {
            self.view.swing();
            self.break_clicked = false;
            self.place_clicked = false;
            return;
        }
        // An eye of ender is set in a frame or thrown, once per click.
        if hand.is_some()
            && self.place_clicked
            && self.held().is_some_and(|s| s.id == items::ENDER_EYE)
        {
            // A block with something to do (a chest, a door) takes the click.
            let used = hit.is_some_and(|hit| self.activate(hit.block, true));
            if !used {
                self.use_eye(hit);
            }
            self.break_clicked = false;
            self.place_clicked = false;
            return;
        }
        self.eating = None;
        if let Some(h) = hand {
            self.use_hand = h;
        }
        let wants_place = using && self.place_cooldown == 0;
        let place_pressed = self.place_clicked;
        let clicked = self.break_clicked;
        if clicked {
            // Every left click swings, whatever it hits.
            self.view.swing();
        }
        self.break_clicked = false;
        self.place_clicked = false;
        // Right-clicking an animal: milk, shears, dye, a bone, a saddle, a seat.
        if using
            && place_pressed
            && !wants_break
            && let Some(mob) = self.target_mob(eye, hit.as_ref())
            && self.use_mob(mob)
        {
            self.view.swing();
            return;
        }
        // A punched fireball flies back.
        if clicked && self.deflect_fireball(eye) {
            self.mining = None;
            return;
        }
        // A crystal or the dragon in front of the targeted block is hit.
        if clicked && let Some(target) = self.target_end(eye, hit.as_ref()) {
            self.mining = None;
            self.attack_end(target);
            return;
        }
        // A mob in front of the targeted block is hit instead, once per click.
        if let Some(mob) = self.target_mob(eye, hit.as_ref())
            && (wants_break || self.mining.is_none())
        {
            self.mining = None;
            if clicked {
                self.attack(mob);
            }
            return;
        }
        // A cart in front of the targeted block: punched to break, or
        // used (sat in, opened).
        if let Some(cart) = self.target_cart(eye, hit.as_ref()) {
            if clicked {
                self.mining = None;
                self.attack_ticker = 0;
                let damage = self.attack_damage();
                self.hit_cart(cart, damage);
                return;
            }
            if using && place_pressed && !wants_break {
                self.use_cart(cart);
                self.view.swing();
                return;
            }
        }
        if !wants_break || hit.is_none() || self.shield_hand.is_some() {
            // Letting go or looking away starts the block over (1.0.0); a
            // raised shield keeps both hands busy.
            self.mining = None;
        }
        if self.shield_hand.is_some() && wants_break {
            if clicked && hit.is_none() {
                self.swing_at_air();
            }
            return;
        }
        if using && !wants_break && self.use_hand == Hand::Main && self.holding_bucket() {
            // A bucket works on a fresh click, unless it's on a chest or
            // door, which opens as usual.
            let used = hit.is_some_and(|hit| self.activate(hit.block, place_pressed));
            if !used && place_pressed {
                self.use_bucket(eye);
            }
            return;
        }
        let Some(hit) = hit else {
            if clicked {
                // A swing at nothing wastes the charge.
                self.swing_at_air();
            }
            return;
        };
        if wants_break {
            self.view.swing();
            self.mine(hit.block);
            // Chips fly off the face while it's being mined.
            if self.mining.is_some()
                && let Some(face) = hit.face
                && let Some(block) =
                    self.chunks
                        .world()
                        .block(hit.block.x, hit.block.y, hit.block.z)
            {
                let rng = &mut self.rng;
                let mut random = || rng.next_f64() as f32;
                let chip = Particle::chip(hit.block, face, block, &mut random);
                self.add_particles([chip]);
            }
        } else if using {
            if self.activate(hit.block, place_pressed) {
                if place_pressed {
                    self.view.swing();
                }
            } else if wants_place {
                if self.held().is_some_and(|stack| {
                    matches!(stack.id, items::FLINT_AND_STEEL | items::FIRE_CHARGE)
                }) {
                    self.strike_flint(&hit);
                } else if self.holding_crystal() {
                    if self.place_crystal(&hit) {
                        self.place_cooldown = PLACE_REPEAT_TICKS;
                    }
                } else if let Some(kind) = self
                    .held()
                    .and_then(|stack| super::minecarts::kind_of(stack.id))
                {
                    // A minecart goes on a rail, not beside it.
                    if self.place_cart(hit.block, kind) {
                        self.view.swing();
                        if self.mode.uses_up_items() {
                            self.inventory.take(self.held_slot(), 1);
                        }
                        self.place_cooldown = PLACE_REPEAT_TICKS;
                    }
                } else {
                    self.place(&hit);
                }
            }
        }
    }

    /// One tick of mining `cell`; breaks it once the progress reaches 1.
    fn mine(&mut self, cell: IVec3) {
        if self.break_cooldown > 0 {
            return;
        }
        let world = self.chunks.world();
        let Some(def) = world
            .block(cell.x, cell.y, cell.z)
            .and_then(|block| self.items.blocks.get(block.id()))
        else {
            return;
        };
        let progress = if self.mode.breaks_instantly() {
            1.0
        } else {
            let in_water = self.eye_in() == Some(Fluid::Water);
            let per_tick = harvest::progress_per_tick_with(
                def,
                self.selected_tool(),
                self.body.on_ground,
                in_water,
                self.held_level(Enchantment::Efficiency),
            );
            let so_far = match self.mining {
                Some((mining, progress)) if mining == cell => progress,
                _ => 0.0,
            };
            so_far + per_tick
        };
        if progress < 1.0 {
            self.mining = Some((cell, progress));
            return;
        }
        self.mining = None;
        self.break_cooldown = BREAK_REPEAT_TICKS;
        self.break_block(cell);
    }

    /// Removes the block at `cell` and whatever it held up, dropping items
    /// and wearing the tool as the game mode says.
    pub(super) fn break_block(&mut self, cell: IVec3) {
        let world = self.chunks.world();
        let edits = interaction::break_block(world, self.items.blocks, cell);
        if edits.is_empty() {
            return;
        }
        if let Some(broken) = world.block(cell.x, cell.y, cell.z) {
            let rng = &mut self.rng;
            let mut random = || rng.next_f64() as f32;
            let burst = Particle::burst(cell, broken, &mut random);
            self.add_particles(burst);
        }
        let world = self.chunks.world();
        if self.mode.drops_items() {
            let mut mined = Vec::new();
            let fortune = self.held_level(Enchantment::Fortune);
            let silk = self.held_level(Enchantment::SilkTouch) > 0;
            let tool = self
                .inventory
                .selected_stack()
                .and_then(|stack| self.items.tool(stack.id));
            let rng = &mut self.rng;
            let mut random = || rng.next_f64() as f32;
            for &(at, _) in &edits {
                let Some(block) = world.block(at.x, at.y, at.z) else {
                    continue;
                };
                // The mined block needs the right tool; blocks it held up
                // (torches, flowers) always drop.
                if at == cell && !harvest::can_harvest(block.def(), tool) {
                    continue;
                }
                let kind = tool.map(|tool| tool.kind);
                for stack in harvest::drops_with(block, kind, fortune, silk, &mut random) {
                    self.entities
                        .push(ItemEntity::from_block(stack, at, &mut random));
                }
                if at == cell {
                    mined.push(block);
                }
            }
            for block in mined {
                // Silk Touch takes the ore whole and leaves its experience.
                let points = if silk { 0 } else { self.ore_xp(block) };
                self.spawn_xp(points, cell.as_dvec3() + DVec3::splat(0.5));
            }
        }
        // A chest or furnace spills what it held, in every game mode (1.0.0).
        for &(at, _) in &edits {
            let broken = self.chunks.world().block(at.x, at.y, at.z);
            let entity = self.chunks.set_block_entity(at, None);
            let rng = &mut self.rng;
            let mut random = || rng.next_f64() as f32;
            // A shulker box comes out whole, with what it held inside; an
            // empty one drops in survival only.
            if let Some(block) = broken
                && block.def().name.ends_with("shulker_box")
                && let Some(item) = self.items.items.of_block(block.id())
            {
                let mut stack = ItemStack::new(item, 1);
                if let Some(entity) = &entity {
                    stack.set_contents(entity.slots().to_vec());
                }
                if self.mode.drops_items() || !stack.contents().is_empty() {
                    self.entities
                        .push(ItemEntity::from_block(stack, at, &mut random));
                }
                continue;
            }
            let Some(entity) = entity else {
                continue;
            };
            for stack in entity.contents() {
                self.entities
                    .push(ItemEntity::from_block(stack, at, &mut random));
            }
        }
        if self.mode.is_vulnerable() {
            self.stats.add_exhaustion(BREAK_EXHAUSTION);
        }
        if self.mode.wears_tools()
            && let Some(wear) = self.selected_tool().map(|tool| tool.wear)
        {
            self.wear_selected(wear);
        }
        self.edit(&edits);
    }

    /// Draws a bow while `using` (right-click held) with ammunition, and
    /// shoots on release. Returns true while drawing.
    fn keep_drawing(&mut self, using: bool) -> bool {
        let ammo = self
            .held()
            .and_then(|stack| self.items.item(stack.id))
            .and_then(|item| item.shoots);
        let Some(ammo) = ammo else {
            self.drawing = None;
            return false;
        };
        let has_ammo = !self.mode.uses_up_items() || self.find_ammo(ammo).is_some();
        if using && has_ammo {
            self.drawing_hand = self.use_hand;
            self.drawing = Some(self.drawing.map_or(1, |ticks| ticks + 1));
            return true;
        }
        if let Some(ticks) = self.drawing.take()
            && let Some(power) = arrow::bow_power(ticks)
        {
            self.shoot_arrow(power, ammo);
        }
        false
    }

    /// The slot holding ammunition: the off-hand first, then the pack.
    fn find_ammo(&self, ammo: crate::inventory::item::ItemId) -> Option<usize> {
        if self
            .inventory
            .slot(crate::inventory::player::OFFHAND)
            .is_some_and(|stack| stack.id == ammo)
        {
            return Some(crate::inventory::player::OFFHAND);
        }
        self.inventory.find(ammo)
    }

    /// Looses an arrow at `power` (0–1) from the bow in hand.
    pub(super) fn shoot_arrow(&mut self, power: f32, ammo: crate::inventory::item::ItemId) {
        let eye = self.body.position() + DVec3::Y * self.body.eye_height();
        let forward = self.camera.forward().as_dvec3();
        let pickup = self.mode.uses_up_items();
        // The arrow carries the shooter's own motion, up and down only in the air.
        let mut carried = self.body.velocity;
        if self.body.on_ground {
            carried.y = 0.0;
        }
        let level = |e: Enchantment| self.held().map_or(0, |bow| bow.level_of(e));
        let (draw, punch, flame, infinity) = (
            level(Enchantment::Power),
            level(Enchantment::Punch),
            level(Enchantment::Flame) > 0,
            level(Enchantment::Infinity) > 0,
        );
        let rng = &mut self.rng;
        let mut random = || rng.next_f64();
        self.arrows.push(
            Arrow::shoot(
                eye,
                forward,
                power,
                carried,
                pickup && !infinity,
                &mut random,
            )
            .enchanted(draw, punch, flame),
        );
        // Infinity shoots without using up a plain arrow.
        if pickup
            && !infinity
            && let Some(slot) = self.find_ammo(ammo)
        {
            self.inventory.take(slot, 1);
        }
        if self.mode.wears_tools() {
            let hand = self.use_hand;
            self.wear_hand(hand, 1);
        }
    }

    /// Moves arrows: hits on mobs, sticking in blocks, picking them up.
    pub(super) fn tick_arrows(&mut self) {
        let mut arrows = std::mem::take(&mut self.arrows);
        let player = self.body.aabb();
        let mut hits = Vec::new();
        let mut end_hits = Vec::new();
        for arrow in &mut arrows {
            let world = self.chunks.world();
            let blocks = self.items.blocks;
            let solid = |cell: glam::IVec3| {
                world
                    .block(cell.x, cell.y, cell.z)
                    .and_then(|block| blocks.get(block.id()))
                    .is_none_or(|def| def.solid)
            };
            let block_hit = |from: DVec3, dir: DVec3, reach: f64| {
                let hit = crate::player::raycast::raycast(from, dir, reach, solid)?;
                crate::player::collision::Aabb::block(hit.block.x, hit.block.y, hit.block.z)
                    .ray_hit(from, dir, reach)
            };
            let in_water = self.fluid_at(arrow.position) == Some(Fluid::Water);
            // One loosed from the bow that meets a crystal or the dragon
            // before any block hits it.
            let end_hit = (!arrow.hostile && !arrow.stuck)
                .then(|| {
                    let speed = arrow.velocity.length();
                    let wall = (speed > 1e-9)
                        .then(|| block_hit(arrow.position, arrow.velocity / speed, speed))
                        .flatten();
                    self.arrow_hits_end(arrow.position, arrow.velocity, wall)
                })
                .flatten();
            let rng = &mut self.rng;
            let mut random = || rng.next_f64() as f32;
            let from = arrow.position;
            if let Some(target) = end_hit {
                let damage = arrow.hit_other(&mut random);
                end_hits.push((target, damage));
                continue;
            }
            let at_player = (arrow.hostile && !self.stats.is_dead()).then_some(&player);
            match arrow.tick(block_hit, in_water, &self.mobs, at_player, &mut random) {
                Some(ArrowEvent::HitMob {
                    index,
                    damage,
                    punch,
                    flame,
                }) => {
                    self.mobs[index].hurt(damage, Some(from), DamageSource::Arrow);
                    if punch > 0 {
                        crate::entity::mob::push(
                            &mut self.mobs[index].body,
                            crate::config::combat::PUNCH_PER_LEVEL * f64::from(punch),
                            arrow.heading,
                        );
                    }
                    if flame {
                        let stats = &mut self.mobs[index].stats;
                        stats.fire = stats.fire.max(crate::config::combat::FLAME_TICKS);
                    }
                    if !arrow.hostile {
                        self.mobs[index].hit_by_player();
                        self.rally(index);
                    }
                }
                Some(ArrowEvent::HitPlayer { damage }) => hits.push((damage, from)),
                None => {}
            }
        }
        for (target, damage) in end_hits {
            self.arrow_strikes_end(target, damage);
        }
        for (damage, from) in hits {
            if self.mode.is_vulnerable() {
                let strike = super::combat::Strike {
                    amount: damage,
                    source: DamageSource::Arrow,
                    from: Some(from),
                    ranged: true,
                    axe: false,
                };
                self.hurt_player(strike);
            }
        }
        arrows.retain(|arrow| {
            if arrow.can_pick_up(&player)
                && self
                    .inventory
                    .add(ItemStack::new(items::ARROW, 1), &self.items)
                    .is_none()
            {
                return false;
            }
            !arrow.expired()
        });
        self.arrows = arrows;
    }

    /// A tick of eating the selected food while right-click is held.
    /// Returns false if the selected item can't be eaten now.
    fn keep_eating(&mut self) -> bool {
        let slot = self.held_slot();
        let Some(stack) = self.held().cloned() else {
            return false;
        };
        let drink = Self::is_drink(stack.id);
        let food = self.items.item(stack.id).and_then(|item| item.food);
        if !drink {
            let Some(food) = food else {
                return false;
            };
            let hungry = food.always_edible || self.stats.can_eat();
            if !self.mode.is_vulnerable() || !hungry {
                return false;
            }
        }
        let ticks = match self.eating {
            Some((eating_slot, ticks)) if eating_slot == slot => ticks + 1,
            _ => 1,
        };
        if ticks < EAT_TICKS {
            self.eating = Some((slot, ticks));
            return true;
        }
        self.eating = None;
        if drink {
            self.finish_drinking(slot, &stack);
            return true;
        }
        let Some(food) = food else {
            return false;
        };
        self.inventory.take(slot, 1);
        self.stats.eat(food.hunger, food.saturation);
        self.food_effects(stack.id);
        if stack.id == items::CHORUS_FRUIT {
            self.chorus_teleport();
        }
        if let Some(leftover) = self.items.item(stack.id).and_then(|item| item.leftover) {
            let bowl = ItemStack::with_damage(leftover, 1, 0);
            if self.inventory.slot(slot).is_none() {
                self.inventory.set(slot, Some(bowl));
            } else if let Some(rest) = self.inventory.add(bowl, &self.items) {
                self.throw(rest);
            }
        }
        true
    }

    /// The living mob under the crosshair within reach, if it's nearer
    /// than the targeted block.
    pub(super) fn target_mob(&self, eye: DVec3, block: Option<&Hit>) -> Option<usize> {
        let dir = self.camera.forward().as_dvec3();
        let block_distance = block.map_or(f64::MAX, |hit| {
            (hit.block.as_dvec3() + DVec3::splat(0.5)).distance(eye) - 0.5
        });
        self.mobs
            .iter()
            .enumerate()
            .filter(|(_, mob)| !mob.is_dead())
            .filter_map(|(i, mob)| {
                let distance = mob
                    .body
                    .aabb()
                    .grow(crate::config::combat::TARGET_GROW)
                    .ray_hit(eye, dir, crate::config::combat::REACH)?;
                Some((i, distance))
            })
            .filter(|&(_, distance)| distance < block_distance)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(i, _)| i)
    }

    /// Right-clicks the block at `cell` (1.0.0's `blockActivated`): opens a
    /// crafting table and so on. Sneaking with something in hand places
    /// against it instead. Only a fresh press (`pressed`) uses the block, as
    /// in modern Minecraft: holding the button doesn't open and shut a door
    /// over and over. Returns whether the block is usable, so nothing gets
    /// placed against it.
    fn activate(&mut self, cell: IVec3, pressed: bool) -> bool {
        if self.keys.sneak && self.inventory.selected_stack().is_some() {
            return false;
        }
        let Some(action) = self
            .chunks
            .world()
            .block(cell.x, cell.y, cell.z)
            .and_then(|block| self.items.blocks.get(block.id()))
            .and_then(|def| def.activate)
        else {
            return false;
        };
        if !pressed {
            return true;
        }
        match action {
            Activate::Crafting => self.open_crafting_table(),
            Activate::Chest => self.open_chest(cell),
            Activate::Furnace => self.open_furnace(cell),
            Activate::Bed => self.use_bed(cell),
            Activate::Door => self.toggle_door(cell),
            Activate::Dispenser => {
                let dropper = self
                    .chunks
                    .world()
                    .block(cell.x, cell.y, cell.z)
                    .is_some_and(|block| block.is(crate::world::blocks::DROPPER));
                self.open_dispenser(cell, dropper);
            }
            Activate::Hopper => self.open_hopper(cell),
            Activate::Enchanting => self.open_bench(BenchKind::Enchanting, cell),
            Activate::Anvil => self.open_bench(BenchKind::Anvil, cell),
            Activate::Grindstone => self.open_bench(BenchKind::Grindstone, cell),
            Activate::Smithing => self.open_bench(BenchKind::Smithing, cell),
            Activate::Brewing => self.open_brewing(cell),
            Activate::Redstone => {
                self.use_redstone_block(cell);
            }
            Activate::Open => {
                if let Some(block) = self.chunks.world().block(cell.x, cell.y, cell.z) {
                    let open = !block.bool(Prop::Open);
                    self.edit(&[(cell, block.with_bool(Prop::Open, open))]);
                }
            }
        }
        true
    }

    /// Opens or closes the door at `cell`, both halves (1.0.0's
    /// `BlockDoor.blockActivated`).
    fn toggle_door(&mut self, cell: IVec3) {
        let world = self.chunks.world();
        let Some(block) = world.block(cell.x, cell.y, cell.z) else {
            return;
        };
        let lower = if block.get(Prop::DoubleBlockHalf) == door_half::LOWER {
            cell
        } else {
            cell - IVec3::Y
        };
        let open = !block.bool(Prop::Open);
        let edits: Vec<_> = [lower, lower + IVec3::Y]
            .into_iter()
            .filter_map(|at| {
                let half = world.block(at.x, at.y, at.z)?;
                (half.id() == block.id()).then(|| (at, half.with_bool(Prop::Open, open)))
            })
            .collect();
        self.edit(&edits);
    }

    /// Places the selected stack's block against the clicked face.
    fn place(&mut self, hit: &Hit) {
        let Some(stack) = self.held().cloned() else {
            return;
        };
        let Some(block) = self.items.places(&stack) else {
            return;
        };
        let world = self.chunks.world();
        let player = self.body.aabb();
        let yaw = self.camera.yaw;
        let Some(edits) = interaction::place_block_looking(
            world,
            self.items.blocks,
            hit,
            block,
            &player,
            yaw,
            self.camera.pitch,
        ) else {
            return;
        };
        self.edit(&edits);
        // A shulker box put down keeps what the item held.
        if !stack.contents().is_empty()
            && let Some(&(at, _)) = edits.iter().find(|(_, state)| state.id() == block.id())
        {
            let mut slots = stack.contents().to_vec();
            slots.resize(crate::world::block_entity::CHEST_SLOTS, None);
            self.chunks.set_block_entity(
                at,
                Some(crate::world::block_entity::BlockEntity::Chest(slots)),
            );
        }
        self.view.swing();
        if self.mode.uses_up_items() {
            self.inventory.take(self.held_slot(), 1);
        }
        self.place_cooldown = PLACE_REPEAT_TICKS;
    }

    /// Middle click (1.0.0's pick block): selects the hotbar slot holding
    /// the targeted block's item, or with every item to hand, puts a stack
    /// of it in the selected slot.
    pub fn pick_block(&mut self) {
        let Some(hit) = self.target else {
            return;
        };
        let Some(block) = self
            .chunks
            .world()
            .block(hit.block.x, hit.block.y, hit.block.z)
        else {
            return;
        };
        let Some(item) = self.items.items.of_block(block.id()) else {
            return;
        };
        let stack = ItemStack::with_damage(item, 1, 0);
        let in_hotbar = (0..HOTBAR_SLOTS).find(|&i| {
            self.inventory
                .slot(i)
                .is_some_and(|held| held.stacks_with(&stack))
        });
        if let Some(slot) = in_hotbar {
            self.inventory.selected = slot;
        } else if self.mode.has_every_item() {
            let full = ItemStack {
                count: self.items.max_stack(stack.id),
                ..stack
            };
            self.inventory.set(self.inventory.selected, Some(full));
        }
    }

    /// A dev tool for debug builds (F6), a shortcut past crafting for
    /// testing: one pickaxe of each tier, iron shovel, axe and sword,
    /// shears and torches.
    #[cfg(debug_assertions)]
    pub fn give_dev_kit(&mut self) {
        const KIT: [(crate::inventory::item::ItemId, u8); 10] = [
            (items::WOODEN_PICKAXE, 1),
            (items::STONE_PICKAXE, 1),
            (items::IRON_PICKAXE, 1),
            (items::GOLDEN_PICKAXE, 1),
            (items::DIAMOND_PICKAXE, 1),
            (items::IRON_SHOVEL, 1),
            (items::IRON_AXE, 1),
            (items::IRON_SWORD, 1),
            (items::SHEARS, 1),
            (items::TORCH, 64),
        ];
        for (id, count) in KIT {
            if let Some(rest) = self.inventory.add(ItemStack::new(id, count), &self.items) {
                self.throw(rest);
            }
        }
        log::info!("dev kit given");
    }

    /// Adds particles, dropping the oldest past 1.0.0's limit.
    pub(super) fn add_particles(&mut self, particles: impl IntoIterator<Item = Particle>) {
        self.particles.extend(particles);
        let over = self.particles.len().saturating_sub(MAX_PARTICLES);
        self.particles.drain(..over);
    }

    /// Moves block-breaking particles and removes the faded ones.
    pub(super) fn tick_particles(&mut self) {
        let collider = crate::player::collision::WorldCollider {
            world: self.chunks.world(),
            blocks: self.items.blocks,
        };
        for particle in &mut self.particles {
            particle.tick(&collider);
        }
        self.particles.retain(|particle| !particle.expired());
    }

    /// Throws one of the selected item forward (1.0.0's Q).
    pub fn throw_selected(&mut self) {
        let Some(stack) = self.inventory.take(self.inventory.selected, 1) else {
            return;
        };
        self.throw(stack);
    }

    /// Throws `stack` from the player's eyes.
    pub fn throw(&mut self, stack: ItemStack) {
        let eye = self.body.position() + DVec3::Y * self.body.eye_height();
        let forward = self.camera.forward().as_dvec3();
        let rng = &mut self.rng;
        let mut random = || rng.next_f64() as f32;
        self.entities
            .push(ItemEntity::thrown(stack, eye, forward, &mut random));
    }

    /// Moves dropped items and lets the player pick them up.
    pub(super) fn tick_items(&mut self) {
        let collider = crate::player::collision::WorldCollider {
            world: self.chunks.world(),
            blocks: self.items.blocks,
        };
        let player = self.body.aabb();
        let alive = !self.stats.is_dead();
        for item in &mut self.entities {
            item.tick(&collider);
            if alive {
                item.pick_up(&player, &mut self.inventory, &self.items);
            }
        }
        self.entities.retain(|item| !item.expired());
        self.tick_orbs();
    }
}
