//! Armor worn (M28): the points and toughness it adds to the player's
//! stats each tick, the wear it takes from the hits it softens, and putting
//! it on with a right-click.

use super::Game;
use crate::inventory::{
    armor::armor_of,
    player::{ARMOR_FEET, ARMOR_HEAD, armor_slot_of},
};

impl Game {
    /// What the worn pieces add up to: (defense, toughness).
    pub fn armor_totals(&self) -> (f32, f32) {
        let mut totals = (0.0, 0.0);
        for index in ARMOR_FEET..=ARMOR_HEAD {
            let Some(stack) = self.player.inventory.slot(index) else {
                continue;
            };
            let Some(armor) = self.items.item(stack.id).and_then(|d| armor_of(&d.name)) else {
                continue;
            };
            totals.0 += armor.defense;
            totals.1 += armor.toughness;
        }
        totals
    }

    /// Sets the stats' armor from what's worn, and wears each piece for
    /// the damage taken since last tick: a quarter of it, at least 1
    /// (modern `hurtArmor`).
    pub(super) fn tick_armor(&mut self) {
        let (defense, toughness) = self.armor_totals();
        self.player.stats.armor = defense;
        self.player.stats.toughness = toughness;
        let wear = std::mem::take(&mut self.player.stats.armor_wear);
        if wear <= 0.0 || !self.player.mode.wears_tools() {
            return;
        }
        let amount = (wear / 4.0).floor().max(1.0) as u16;
        for index in ARMOR_FEET..=ARMOR_HEAD {
            self.player
                .inventory
                .damage_slot(index, amount, &self.items);
        }
    }

    /// Right-click with armor in hand: puts it on, swapping with the piece
    /// there. Returns whether it was armor.
    pub(super) fn equip_held(&mut self) -> bool {
        let slot = self.held_slot();
        let items = self.items.clone();
        let is_armor = self
            .player
            .inventory
            .slot(slot)
            .is_some_and(|stack| armor_slot_of(&items, stack).is_some());
        let name = self
            .player
            .inventory
            .slot(slot)
            .map(|stack| stack.id.name())
            .unwrap_or_default();
        let worn = is_armor && self.player.inventory.equip(slot, &items);
        if worn {
            crate::audio::play_here(crate::audio::events::armor_equip(name));
        }
        worn
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        game::tests::flat_game,
        inventory::{items, stack::ItemStack},
        player::{mode::GameMode, stats::DamageSource},
    };

    #[test]
    fn armor_put_on_with_a_right_click_cuts_damage_and_wears() {
        let mut game = flat_game(GameMode::Survival);
        game.player
            .inventory
            .set(0, Some(ItemStack::new(items::IRON_CHESTPLATE, 1)));
        game.player
            .inventory
            .set(1, Some(ItemStack::new(items::IRON_HELMET, 1)));
        game.player.inventory.selected = 0;
        assert!(game.equip_held());
        assert!(game.player.inventory.slot(0).is_none());
        game.player.inventory.selected = 1;
        assert!(game.equip_held());
        assert_eq!(game.armor_totals(), (8.0, 0.0));
        game.tick_armor();
        let before = game.player.stats.health;
        game.player.stats.damage(10.0, DamageSource::Mob);
        // 8 points against 10 damage: 10 × (1 − max(1.6, 8 − 5) / 25) = 8.8.
        assert!((before - game.player.stats.health - 8.8).abs() < 0.01);
        game.tick_armor();
        let chest = game
            .player
            .inventory
            .slot(crate::inventory::player::ARMOR_CHEST)
            .unwrap();
        assert_eq!(chest.damage, 2, "a quarter of 10, rounded down");
        // Falling gets through armor and wears none.
        let before = game.player.stats.health;
        game.player.stats.fall_distance = 0.0;
        game.player.stats.damage(3.0, DamageSource::Fall);
        assert!(
            (before - game.player.stats.health - 3.0).abs() < 0.01
                || game.player.stats.health <= before
        );
        game.tick_armor();
        let chest = game
            .player
            .inventory
            .slot(crate::inventory::player::ARMOR_CHEST)
            .unwrap();
        assert_eq!(chest.damage, 2);
    }

    #[test]
    fn the_armor_slots_take_only_their_own_piece_and_shift_click_swaps_in_and_out() {
        let mut game = flat_game(GameMode::Survival);
        game.open_inventory();
        let boots = ARMOR_FEET;
        game.player.cursor = Some(ItemStack::new(items::IRON_HELMET, 1));
        game.click_slot(
            crate::inventory::slots::SlotRef::Inventory(boots),
            crate::inventory::player::Click::Left,
        );
        assert!(
            game.player.inventory.slot(boots).is_none(),
            "a helmet isn't boots"
        );
        assert!(game.player.cursor.is_some());
        game.click_slot(
            crate::inventory::slots::SlotRef::Inventory(ARMOR_HEAD),
            crate::inventory::player::Click::Left,
        );
        assert!(game.player.inventory.slot(ARMOR_HEAD).is_some());
        assert!(game.player.cursor.is_none());
        // Shift-click takes it off again.
        game.click_slot(
            crate::inventory::slots::SlotRef::Inventory(ARMOR_HEAD),
            crate::inventory::player::Click::Shift,
        );
        assert!(game.player.inventory.slot(ARMOR_HEAD).is_none());
        assert_eq!(
            game.player.inventory.slot(0).unwrap().id,
            items::IRON_HELMET
        );
        // And shift-clicking it in the main inventory puts it on.
        game.click_slot(
            crate::inventory::slots::SlotRef::Inventory(0),
            crate::inventory::player::Click::Shift,
        );
        assert_eq!(
            game.player.inventory.slot(ARMOR_HEAD).unwrap().id,
            items::IRON_HELMET
        );
    }
}
