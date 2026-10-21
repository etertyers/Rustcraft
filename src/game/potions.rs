//! Potions and effects in play (M26): drinking a potion (it leaves a glass
//! bottle) or milk (it clears every effect), throwing splash potions and
//! their burst over everyone near, the effects food gives (golden apples,
//! rotten flesh, spider eyes, raw chicken, pufferfish), and the swirl of
//! particles round whoever has an effect.

use glam::DVec3;

use super::Game;
use crate::{
    entity::{
        particle::Particle,
        thrown::{ThrownPotion, splash_strength},
    },
    inventory::{items, potion, stack::ItemStack},
    player::{
        collision::Aabb,
        effects::{Active, Effect},
    },
};

/// Effects some food gives: (item, effect, amplifier, ticks, chance) —
/// modern `Foods`.
const FOOD_EFFECTS: [(&str, Effect, u8, u32, f64); 7] = [
    ("golden_apple", Effect::Regeneration, 1, 100, 1.0),
    ("golden_apple", Effect::Absorption, 0, 2400, 1.0),
    ("rotten_flesh", Effect::Hunger, 0, 600, 0.8),
    ("spider_eye", Effect::Poison, 0, 100, 1.0),
    ("chicken", Effect::Hunger, 0, 600, 0.3),
    ("pufferfish", Effect::Poison, 1, 1200, 1.0),
    ("pufferfish", Effect::Hunger, 2, 300, 1.0),
];

/// Ticks between swirl particles round someone with an effect.
const SWIRL_EVERY: u64 = 4;

impl Game {
    /// Whether `id` is drunk rather than eaten: potions and milk.
    pub(super) fn is_drink(id: crate::inventory::item::ItemId) -> bool {
        matches!(id, items::POTION | items::MILK_BUCKET)
    }

    /// The end of drinking `stack` from `slot`: its effects (or, for milk,
    /// none at all), and the bottle or bucket back.
    pub(super) fn finish_drinking(&mut self, slot: usize, stack: &ItemStack) {
        let vulnerable = self.mode.is_vulnerable();
        let empty = if stack.id == items::MILK_BUCKET {
            self.stats.effects.clear();
            self.stats.absorption = 0.0;
            items::BUCKET
        } else {
            if let Some(potion) = potion::of(stack) {
                for effect in potion::effects(potion) {
                    self.stats.apply(effect, false, vulnerable);
                }
            }
            items::GLASS_BOTTLE
        };
        if !self.mode.uses_up_items() {
            return;
        }
        self.inventory.take(slot, 1);
        let left = ItemStack::new(empty, 1);
        if self.inventory.slot(slot).is_none() {
            self.inventory.set(slot, Some(left));
        } else if let Some(rest) = self.inventory.add(left, &self.items) {
            self.throw(rest);
        }
    }

    /// Right-click on a cauldron with a water bucket (fills it), an empty
    /// bucket (takes a full one's water), a glass bottle (takes a third) or
    /// a water bottle (adds a third). Returns whether anything happened.
    pub(super) fn use_cauldron(&mut self, cell: glam::IVec3) -> bool {
        use crate::world::{block::BlockState, blocks, props::Prop};
        let Some(block) = self.chunks.world().block(cell.x, cell.y, cell.z) else {
            return false;
        };
        let level = if block.is(blocks::WATER_CAULDRON) {
            block.number(Prop::LevelCauldron)
        } else if block.is(blocks::CAULDRON) {
            0
        } else {
            return false;
        };
        let Some(held) = self.held().cloned() else {
            return false;
        };
        let water = potion::of(&held).is_some_and(|p| p.id == "water") && held.id == items::POTION;
        let (new_level, gives) = match held.id {
            items::WATER_BUCKET if level < 3 => (3, items::BUCKET),
            items::BUCKET if level == 3 => (0, items::WATER_BUCKET),
            items::GLASS_BOTTLE if level > 0 => (level - 1, items::POTION),
            items::POTION if water && level < 3 => (level + 1, items::GLASS_BOTTLE),
            _ => return false,
        };
        let state = if new_level == 0 {
            BlockState::of(blocks::CAULDRON)
        } else {
            BlockState::of(blocks::WATER_CAULDRON).with_number(Prop::LevelCauldron, new_level)
        };
        self.edit(&[(cell, state)]);
        self.swap_held(ItemStack::new(gives, 1));
        true
    }

    /// Fills the held glass bottle from the water source looked at.
    pub(super) fn fill_bottle(&mut self, eye: DVec3) -> bool {
        let world = self.chunks.world();
        let dir = self.camera.forward().as_dvec3();
        let water = |at: glam::IVec3| {
            world
                .block(at.x, at.y, at.z)
                .is_some_and(|b| b.def().is_targetable() || b.is(crate::world::blocks::WATER))
        };
        let Some(hit) = crate::player::raycast::raycast(eye, dir, crate::config::REACH, water)
        else {
            return false;
        };
        let is_water = world
            .block(hit.block.x, hit.block.y, hit.block.z)
            .is_some_and(|b| b.is(crate::world::blocks::WATER));
        if !is_water {
            return false;
        }
        self.swap_held(ItemStack::new(items::POTION, 1));
        true
    }

    /// Uses up one of the held item for `got` (a bottle filled, a bucket
    /// emptied), into the hand if it's free, the inventory if not.
    fn swap_held(&mut self, got: ItemStack) {
        if !self.mode.uses_up_items() {
            if got.id == items::POTION && self.inventory.add(got.clone(), &self.items).is_some() {
                self.throw(got);
            }
            return;
        }
        let slot = self.held_slot();
        self.inventory.take(slot, 1);
        if self.inventory.slot(slot).is_none() {
            self.inventory.set(slot, Some(got));
        } else if let Some(rest) = self.inventory.add(got, &self.items) {
            self.throw(rest);
        }
    }

    /// The effects eating `food` gives.
    pub(super) fn food_effects(&mut self, food: crate::inventory::item::ItemId) {
        let vulnerable = self.mode.is_vulnerable();
        for &(name, effect, amplifier, duration, chance) in &FOOD_EFFECTS {
            if items::by_name(name) == Some(food) && self.rng.next_f64() < chance {
                self.stats
                    .apply(Active::new(effect, amplifier, duration), false, vulnerable);
            }
        }
    }

    /// Throws the splash potion in `slot` the way the player looks.
    pub(super) fn throw_potion(&mut self, slot: usize) {
        let Some(stack) = self.inventory.slot(slot).cloned() else {
            return;
        };
        let eye = self.body.position() + DVec3::Y * self.body.eye_height();
        let thrown = ThrownPotion::throw(
            stack.with_count(1),
            eye,
            self.camera.yaw,
            self.camera.pitch,
            self.body.velocity,
        );
        self.thrown.push(thrown);
        if self.mode.uses_up_items() {
            self.inventory.take(slot, 1);
        }
        self.view.swing();
    }

    /// One tick of thrown potions, bursting those that hit something.
    pub(super) fn tick_thrown(&mut self) {
        let mut flying = std::mem::take(&mut self.thrown);
        let player = self.body.aabb();
        let mut bursts = Vec::new();
        flying.retain_mut(|potion| {
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
                Aabb::block(hit.block.x, hit.block.y, hit.block.z).ray_hit(from, dir, reach)
            };
            match potion.tick(block_hit, &self.mobs, &player) {
                Some(at) => {
                    bursts.push((at, potion.stack.clone()));
                    false
                }
                None => potion.position.y > -64.0,
            }
        });
        self.thrown = flying;
        for (at, stack) in bursts {
            self.splash(at, &stack);
        }
    }

    /// A splash potion bursting at `at`: everyone within four blocks gets
    /// its effects, weaker and shorter the farther from the middle.
    pub(super) fn splash(&mut self, at: DVec3, stack: &ItemStack) {
        let Some(potion) = potion::of(stack) else {
            return;
        };
        let effects = potion::effects(potion);
        let scaled = |effect: Active, strength: f64| -> Option<Active> {
            if effect.effect.instant() {
                // Instant ones are scaled as they act (below).
                return Some(effect);
            }
            let ticks = (strength * f64::from(effect.duration) + 0.5) as u32;
            (ticks > 20).then_some(Active {
                duration: ticks,
                ..effect
            })
        };
        let vulnerable = self.mode.is_vulnerable();
        if let Some(strength) = splash_strength(at, &self.body.aabb()) {
            for &effect in &effects {
                if let Some(effect) = scaled(effect, strength) {
                    apply_scaled(&mut self.stats, effect, strength, false, vulnerable);
                }
            }
        }
        for mob in &mut self.mobs {
            if mob.is_dead() {
                continue;
            }
            let Some(strength) = splash_strength(at, &mob.body.aabb()) else {
                continue;
            };
            let undead = super::gear::kind_of(&self.mob_defs.get(mob.kind).name)
                == super::gear::Kind::Undead;
            for &effect in &effects {
                if let Some(effect) = scaled(effect, strength) {
                    let before = mob.stats.health;
                    apply_scaled(&mut mob.stats, effect, strength, undead, true);
                    if mob.stats.health < before {
                        mob.hit_by_player();
                    }
                }
            }
        }
        // Glass and the potion's color fly out.
        let color = potion.effects.first().map_or(0x385DC6, |e| e.0.color());
        let rng = &mut self.rng;
        let mut random = || rng.next_f64() as f32;
        let burst: Vec<Particle> = (0..16)
            .map(|_| Particle::colored(at, color, &mut random))
            .collect();
        self.add_particles(burst);
    }

    /// Swirls of each effect's color round the player and mobs with one.
    pub(super) fn swirl_effects(&mut self) {
        if !self.time.is_multiple_of(SWIRL_EVERY) {
            return;
        }
        let mut swirls = Vec::new();
        let mut around = |stats: &crate::player::stats::Stats,
                          aabb: Aabb,
                          rng: &mut crate::world::worldgen::rng::Rng| {
            let Some(active) = stats.effects.0.first() else {
                return;
            };
            if active.effect == Effect::Invisibility {
                return;
            }
            let mut random = || rng.next_f64() as f32;
            let size = aabb.max - aabb.min;
            let at = aabb.min
                + DVec3::new(
                    f64::from(random()) * size.x,
                    f64::from(random()) * size.y,
                    f64::from(random()) * size.z,
                );
            swirls.push(Particle::colored(at, active.effect.color(), &mut random));
        };
        let in_third_person = self.view_mode() != super::view::ViewMode::First;
        if in_third_person {
            around(&self.stats, self.body.aabb(), &mut self.rng);
        }
        for mob in &self.mobs {
            if !mob.is_dead() && !mob.stats.effects.is_empty() {
                around(&mob.stats, mob.body.aabb(), &mut self.rng);
            }
        }
        self.add_particles(swirls);
    }
}

/// Gives `effect` at `strength` (0–1): an instant one heals or hurts that
/// share of its full amount (modern `applyInstantenousEffect`).
fn apply_scaled(
    stats: &mut crate::player::stats::Stats,
    effect: Active,
    strength: f64,
    undead: bool,
    vulnerable: bool,
) {
    if !effect.effect.instant() {
        stats.apply(effect, undead, vulnerable);
        return;
    }
    let level = 1u32 << effect.amplifier.min(4);
    let heals = (effect.effect == Effect::InstantHealth) != undead;
    if heals {
        stats.heal((strength * f64::from(4 * level)) as f32);
    } else if vulnerable {
        let amount = (strength * f64::from(6 * level) + 0.5) as f32;
        stats.damage(amount, crate::player::stats::DamageSource::Magic);
    }
}

#[cfg(test)]
mod tests {
    use glam::IVec3;

    use super::*;
    use crate::{
        game::tests::{flat_game, mob_registry},
        player::mode::GameMode,
        world::{block::BlockState, blocks},
    };

    fn drink(game: &mut Game, stack: ItemStack) {
        game.inventory.set(0, Some(stack));
        game.inventory.selected = 0;
        game.placing = true;
        for _ in 0..40 {
            game.tick();
        }
        game.placing = false;
    }

    #[test]
    fn a_fire_resistance_potion_keeps_lava_off_for_three_minutes_until_milk() {
        let mut game = flat_game(GameMode::Survival);
        drink(&mut game, potion::stack(items::POTION, "fire_resistance"));
        let active = game
            .stats
            .effects
            .get(Effect::FireResistance)
            .copied()
            .expect("drunk");
        assert!(active.duration > 3500 && active.duration <= 3600);
        assert_eq!(
            game.inventory.slot(0).map(|s| s.id),
            Some(items::GLASS_BOTTLE)
        );
        // In lava, unhurt.
        let feet = game.body.position().floor().as_ivec3();
        game.chunks.set_blocks(&[
            (feet, BlockState::of(blocks::LAVA)),
            (feet + IVec3::Y, BlockState::of(blocks::LAVA)),
        ]);
        for _ in 0..60 {
            game.tick();
        }
        assert_eq!(game.stats.health, 20.0);
        game.chunks
            .set_blocks(&[(feet, BlockState::AIR), (feet + IVec3::Y, BlockState::AIR)]);
        game.stats.fire = 0;
        drink(&mut game, ItemStack::new(items::MILK_BUCKET, 1));
        assert!(game.stats.effects.is_empty(), "milk clears it");
        assert_eq!(game.inventory.slot(0).map(|s| s.id), Some(items::BUCKET));
    }

    #[test]
    fn a_splash_of_healing_heals_the_player_and_hurts_the_undead() {
        let mut game = flat_game(GameMode::Survival);
        game.mob_defs = mob_registry();
        game.stats.health = 10.0;
        let at = game.body.position() + DVec3::new(1.0, 0.0, 0.0);
        game.summon("zombie", at, 0.0);
        game.splash(
            at + DVec3::Y * 0.5,
            &potion::stack(items::SPLASH_POTION, "healing"),
        );
        assert!(game.stats.health > 10.0, "healed: {}", game.stats.health);
        assert!(game.mobs[0].stats.health < 20.0, "the zombie is hurt");
    }

    #[test]
    fn a_thrown_splash_potion_lands_and_bursts() {
        let mut game = flat_game(GameMode::Survival);
        game.camera.pitch = -1.2;
        game.inventory
            .set(0, Some(potion::stack(items::SPLASH_POTION, "swiftness")));
        game.inventory.selected = 0;
        game.place_clicked = true;
        game.tick();
        assert_eq!(game.thrown.len(), 1);
        assert!(game.inventory.slot(0).is_none());
        for _ in 0..60 {
            game.tick();
        }
        assert!(game.thrown.is_empty());
        assert!(
            game.stats.effects.has(Effect::Speed),
            "splashed at their feet"
        );
    }

    #[test]
    fn a_golden_apple_gives_regeneration_and_absorption() {
        let mut game = flat_game(GameMode::Survival);
        game.stats.food = 10;
        drink_like_food(&mut game, items::GOLDEN_APPLE);
        assert_eq!(game.stats.effects.amplifier(Effect::Regeneration), Some(1));
        assert!(game.stats.effects.has(Effect::Absorption));
        assert_eq!(game.stats.absorption, 4.0);
    }

    #[test]
    fn a_stand_brews_fire_resistance_from_a_cauldron_filled_bottle() {
        use crate::{game::Container, inventory::brewing};
        let mut game = flat_game(GameMode::Survival);
        let cauldron = IVec3::new(0, 64, -2);
        game.chunks
            .set_blocks(&[(cauldron, BlockState::of(blocks::CAULDRON))]);
        game.inventory
            .set(0, Some(ItemStack::new(items::WATER_BUCKET, 1)));
        game.inventory.selected = 0;
        assert!(game.use_cauldron(cauldron));
        assert_eq!(game.inventory.slot(0).map(|s| s.id), Some(items::BUCKET));
        game.inventory
            .set(0, Some(ItemStack::new(items::GLASS_BOTTLE, 3)));
        for _ in 0..3 {
            assert!(game.use_cauldron(cauldron));
        }
        assert!(
            game.chunks
                .world()
                .block(0, 64, -2)
                .unwrap()
                .is(blocks::CAULDRON),
            "emptied"
        );
        let bottles: u8 = (0..36)
            .filter_map(|i| game.inventory.slot(i))
            .filter(|s| potion::of(s).is_some_and(|p| p.id == "water"))
            .map(|s| s.count)
            .sum();
        assert_eq!(bottles, 3);

        let stand = IVec3::new(2, 64, 0);
        game.chunks
            .set_blocks(&[(stand, BlockState::of(blocks::BREWING_STAND))]);
        game.open_brewing(stand);
        let put = |game: &mut Game, slot: usize, stack: ItemStack| {
            if let Some(slots) = game.block_slots_mut() {
                slots[slot] = Some(stack);
            }
        };
        for i in brewing::BOTTLES {
            put(&mut game, i, potion::stack(items::POTION, "water"));
        }
        put(
            &mut game,
            brewing::INGREDIENT,
            ItemStack::new(items::NETHER_WART, 1),
        );
        put(
            &mut game,
            brewing::FUEL,
            ItemStack::new(items::BLAZE_POWDER, 1),
        );
        for _ in 0..402 {
            game.tick();
        }
        put(
            &mut game,
            brewing::INGREDIENT,
            ItemStack::new(items::MAGMA_CREAM, 1),
        );
        for _ in 0..402 {
            game.tick();
        }
        assert!(matches!(game.container, Some(Container::Brewing(_))));
        let slots = game.block_slots().unwrap().to_vec();
        for i in brewing::BOTTLES {
            assert_eq!(
                potion::of(slots[i].as_ref().unwrap()).unwrap().id,
                "fire_resistance"
            );
        }
        let shown = game.chunks.world().block(2, 64, 0).unwrap();
        assert!(
            shown.bool(crate::world::props::Prop::HasBottle1),
            "bottles on the stand"
        );
    }

    #[test]
    fn swiftness_speeds_walking_up_a_fifth_and_night_vision_lights_the_dark() {
        let walk = |speed: bool| {
            let mut game = flat_game(GameMode::Survival);
            if speed {
                game.stats
                    .apply(Active::new(Effect::Speed, 0, 600), false, true);
            }
            game.camera.pitch = 0.0;
            game.on_key(winit::keyboard::KeyCode::KeyW, true, false, true);
            let start = game.body.position();
            for _ in 0..60 {
                game.tick();
            }
            (game.body.position() - start).length()
        };
        let ratio = walk(true) / walk(false);
        assert!((ratio - 1.2).abs() < 0.03, "{ratio}");

        let mut game = flat_game(GameMode::Survival);
        game.time = 18000;
        assert!(game.environment().ambient < 0.5);
        game.stats
            .apply(Active::new(Effect::NightVision, 0, 3600), false, true);
        assert_eq!(game.environment().ambient, 1.0);
    }

    #[test]
    fn a_wither_skeleton_withers_what_it_hits() {
        let mut game = flat_game(GameMode::Survival);
        game.mob_defs = mob_registry();
        game.summon(
            "wither_skeleton",
            game.body.position() + DVec3::new(1.2, 0.0, 0.0),
            0.0,
        );
        for _ in 0..60 {
            game.tick();
            if game.stats.effects.has(Effect::Wither) {
                break;
            }
        }
        assert!(game.stats.effects.has(Effect::Wither));
    }

    fn drink_like_food(game: &mut Game, food: crate::inventory::item::ItemId) {
        drink(game, ItemStack::new(food, 1));
    }
}
