//! What enchantments on the item in hand do: faster mining, slower wear,
//! harder and fiercer blows, and bow shots.

use super::Game;
use crate::{
    config::combat as c,
    inventory::{enchant::Enchantment, player::Hand},
};

/// What kind of creature a mob is, for Smite and Bane of Arthropods.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Undead,
    Arthropod,
    Other,
}

/// The kind of mob called `name`.
pub fn kind_of(name: &str) -> Kind {
    match name {
        "zombie" | "husk" | "drowned" | "skeleton" | "stray" | "phantom" => Kind::Undead,
        "spider" | "cave_spider" | "bee" | "silverfish" | "endermite" => Kind::Arthropod,
        _ => Kind::Other,
    }
}

/// Damage the main hand's enchantments add to a full swing at a `kind`
/// (modern `EnchantmentHelper.getDamageBonus`).
pub fn damage_bonus(sharpness: u8, smite: u8, bane: u8, kind: Kind) -> f32 {
    let mut bonus = 0.0;
    if sharpness > 0 {
        bonus += c::SHARPNESS_BASE + c::SHARPNESS_PER_LEVEL * f32::from(sharpness - 1);
    }
    if kind == Kind::Undead {
        bonus += c::SMITE_PER_LEVEL * f32::from(smite);
    }
    if kind == Kind::Arthropod {
        bonus += c::BANE_PER_LEVEL * f32::from(bane);
    }
    bonus
}

impl Game {
    /// The level of `enchantment` on the main hand's item.
    pub(super) fn held_level(&self, enchantment: Enchantment) -> u8 {
        self.inventory
            .selected_stack()
            .map_or(0, |stack| stack.level_of(enchantment))
    }

    /// Damage points an item with Unbreaking `level` really takes when it
    /// would take `amount`: each point is skipped with chance
    /// level/(level + 1).
    fn after_unbreaking(&mut self, level: u8, amount: u16) -> u16 {
        if level == 0 {
            return amount;
        }
        let keep = 1.0 / (f64::from(level) + 1.0);
        (0..amount).filter(|_| self.rng.next_f64() < keep).count() as u16
    }

    /// Wears the main hand's item, as tools do mining and fighting.
    /// Returns true if it broke.
    pub(super) fn wear_selected(&mut self, amount: u16) -> bool {
        let hand = Hand::Main;
        self.wear_hand(hand, amount)
    }

    /// Wears the item in `hand`, with Unbreaking. Returns true if it broke.
    pub(super) fn wear_hand(&mut self, hand: Hand, amount: u16) -> bool {
        let level = self
            .inventory
            .hand_stack(hand)
            .map_or(0, |stack| stack.level_of(Enchantment::Unbreaking));
        let amount = self.after_unbreaking(level, amount);
        amount > 0 && self.inventory.damage_hand(hand, amount, &self.items)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        game::tests::flat_game,
        inventory::{items, stack::ItemStack},
        player::mode::GameMode,
    };

    #[test]
    fn damage_bonuses_follow_modern_values() {
        assert_eq!(damage_bonus(1, 0, 0, Kind::Other), 1.0);
        assert_eq!(damage_bonus(5, 0, 0, Kind::Other), 3.0);
        // Smite and Bane only against their kind.
        assert_eq!(damage_bonus(0, 5, 0, Kind::Undead), 12.5);
        assert_eq!(damage_bonus(0, 5, 0, Kind::Other), 0.0);
        assert_eq!(damage_bonus(0, 0, 3, Kind::Arthropod), 7.5);
        assert_eq!(damage_bonus(0, 0, 3, Kind::Undead), 0.0);
        assert_eq!(kind_of("skeleton"), Kind::Undead);
        assert_eq!(kind_of("cave_spider"), Kind::Arthropod);
        assert_eq!(kind_of("pig"), Kind::Other);
    }

    #[test]
    fn unbreaking_cuts_wear_by_level_over_level_plus_one() {
        let mut game = flat_game(GameMode::Survival);
        let mut pickaxe = ItemStack::new(items::DIAMOND_PICKAXE, 1);
        pickaxe.enchant(Enchantment::Unbreaking, 3);
        let slot = game.inventory.selected;
        game.inventory.set(slot, Some(pickaxe));
        for _ in 0..4000 {
            game.wear_selected(1);
        }
        let worn = game.inventory.selected_stack().unwrap().damage;
        // A quarter of 4000, give or take chance.
        assert!((850..1150).contains(&worn), "{worn}");
        // Without it every point counts.
        game.inventory
            .set(slot, Some(ItemStack::new(items::DIAMOND_PICKAXE, 1)));
        for _ in 0..100 {
            game.wear_selected(1);
        }
        assert_eq!(game.inventory.selected_stack().unwrap().damage, 100);
    }
}

#[cfg(test)]
mod effect_tests {
    use glam::DVec3;

    use super::*;
    use crate::{
        entity::mob::Mob,
        game::tests::{flat_game, mob_registry},
        inventory::{items, stack::ItemStack},
        player::mode::GameMode,
        world::{block::BlockState, blocks, harvest},
    };

    fn enchanted(item: crate::inventory::item::ItemId, e: &[(Enchantment, u8)]) -> ItemStack {
        let mut stack = ItemStack::new(item, 1);
        for &(enchantment, level) in e {
            stack.enchant(enchantment, level);
        }
        stack
    }

    #[test]
    fn efficiency_adds_its_level_squared_plus_one_to_the_mining_speed() {
        let items = crate::inventory::stack::Items::new();
        let stone = items.blocks.def(blocks::STONE);
        let pickaxe = items.tool(items::DIAMOND_PICKAXE);
        let plain = harvest::progress_per_tick(stone, pickaxe, true, false);
        // Speed 8 on stone, plus 26 for Efficiency V: 34 over 8.
        let fast = harvest::progress_per_tick_with(stone, pickaxe, true, false, 5);
        assert!((fast / plain - 34.0 / 8.0).abs() < 1e-4, "{fast} {plain}");
        assert_eq!((1.0 / fast).ceil(), 2.0, "stone breaks in two ticks");
        // A bare hand (speed 1) gets nothing.
        let hand = harvest::progress_per_tick(stone, None, true, false);
        assert_eq!(
            harvest::progress_per_tick_with(stone, None, true, false, 5),
            hand
        );
    }

    #[test]
    fn fortune_multiplies_diamonds_and_adds_lapis() {
        let mut state = 7u32;
        let mut random = move || {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (state >> 8) as f32 / (1u32 << 24) as f32
        };
        let diamond = BlockState::of(blocks::DIAMOND_ORE);
        let total = |fortune, random: &mut dyn FnMut() -> f32| -> u32 {
            (0..4000)
                .map(|_| {
                    let mut roll = || random();
                    harvest::drops_with(
                        diamond,
                        Some(ToolKindShim::PICKAXE),
                        fortune,
                        false,
                        &mut roll,
                    )
                    .iter()
                    .map(|s| u32::from(s.count))
                    .sum::<u32>()
                })
                .sum()
        };
        assert_eq!(total(0, &mut random), 4000);
        let mean = f64::from(total(3, &mut random)) / 4000.0;
        assert!(
            (2.05..2.35).contains(&mean),
            "Fortune III averages 2.2: {mean}"
        );
        let lapis = BlockState::of(blocks::LAPIS_ORE);
        let mut roll = || random();
        let plain = harvest::drops_with(lapis, Some(ToolKindShim::PICKAXE), 0, false, &mut roll);
        assert!(plain[0].count >= 4);
    }

    struct ToolKindShim;
    impl ToolKindShim {
        const PICKAXE: crate::inventory::item::ToolKind = crate::inventory::item::ToolKind::Pickaxe;
    }

    #[test]
    fn silk_touch_takes_ores_glass_and_stone_as_themselves() {
        let mut random = || 0.5;
        let mut silk = |block| {
            harvest::drops_with(
                BlockState::of(block),
                Some(crate::inventory::item::ToolKind::Pickaxe),
                0,
                true,
                &mut random,
            )
        };
        assert_eq!(
            silk(blocks::DIAMOND_ORE),
            vec![ItemStack::new(items::DIAMOND_ORE, 1)]
        );
        assert_eq!(silk(blocks::STONE), vec![ItemStack::new(items::STONE, 1)]);
        assert_eq!(silk(blocks::GLASS), vec![ItemStack::new(items::GLASS, 1)]);
        // Dirt drops dirt anyway; cobblestone is not special.
        assert_eq!(silk(blocks::DIRT), vec![ItemStack::new(items::DIRT, 1)]);
    }

    #[test]
    fn mining_with_silk_touch_in_the_game_gives_the_ore_and_no_experience() {
        let mut game = flat_game(GameMode::Survival);
        let cell = glam::IVec3::new(2, 64, 0);
        game.chunks
            .set_blocks(&[(cell, BlockState::of(blocks::DIAMOND_ORE))]);
        let slot = game.inventory.selected;
        game.inventory.set(
            slot,
            Some(enchanted(
                items::DIAMOND_PICKAXE,
                &[(Enchantment::SilkTouch, 1)],
            )),
        );
        game.break_block(cell);
        assert_eq!(game.entities.len(), 1);
        assert_eq!(game.entities[0].stack.id, items::DIAMOND_ORE);
        assert!(game.orbs.is_empty(), "silk touch leaves the experience");
        // Without it: a diamond and some experience.
        let cell = glam::IVec3::new(3, 64, 0);
        game.entities.clear();
        game.chunks
            .set_blocks(&[(cell, BlockState::of(blocks::DIAMOND_ORE))]);
        game.inventory
            .set(slot, Some(ItemStack::new(items::DIAMOND_PICKAXE, 1)));
        game.break_block(cell);
        assert_eq!(game.entities[0].stack.id, items::DIAMOND);
        assert!(game.orbs.iter().map(|o| o.value).sum::<u32>() >= 3);
    }

    #[test]
    fn looting_makes_a_zombies_drops_bigger_and_rarer_ones_likelier() {
        let defs = mob_registry();
        let kind = defs.find("zombie").unwrap();
        let zombie = Mob::new(kind, defs.get(kind), DVec3::ZERO, 0.0);
        let mut state = 99u32;
        let mut random = move || {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (state >> 8) as f32 / (1u32 << 24) as f32
        };
        let mut total = |looting| {
            (0..4000)
                .flat_map(|_| zombie.drops(defs.get(kind), looting, &mut random))
                .map(|s| u32::from(s.count))
                .sum::<u32>()
        };
        let (plain, looted) = (total(0), total(3));
        assert!(looted as f32 > plain as f32 * 1.4, "{plain} {looted}");
    }

    #[test]
    fn a_bow_with_power_punch_flame_and_infinity_shoots_better_arrows_for_free() {
        let mut game = flat_game(GameMode::Survival);
        let bow = enchanted(
            items::BOW,
            &[
                (Enchantment::Power, 5),
                (Enchantment::Punch, 2),
                (Enchantment::Flame, 1),
                (Enchantment::Infinity, 1),
            ],
        );
        let slot = game.inventory.selected;
        game.inventory.set(slot, Some(bow));
        game.inventory.set(8, Some(ItemStack::new(items::ARROW, 3)));
        game.shoot_arrow(1.0, items::ARROW);
        let arrow = &game.arrows[0];
        assert_eq!((arrow.power, arrow.punch, arrow.flame), (5, 2, true));
        assert!(!arrow.pickup, "infinity arrows vanish");
        assert_eq!(
            game.inventory.slot(8),
            Some(&ItemStack::new(items::ARROW, 3)),
            "no arrow used"
        );
        // A plain bow uses one and leaves one to pick up.
        game.inventory
            .set(slot, Some(ItemStack::new(items::BOW, 1)));
        game.shoot_arrow(1.0, items::ARROW);
        assert_eq!(game.inventory.slot(8).map(|s| s.count), Some(2));
        assert!(game.arrows[1].pickup);
    }
}
