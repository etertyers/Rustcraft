//! The player's 36 slots (hotbar first) and 1.0.0's rules for picking up
//! items and clicking slots with a stack held on the cursor.

use serde::{Deserialize, Serialize};

pub use super::slots::Click;
use super::{
    item::ItemId,
    slots,
    stack::{ItemStack, Items},
};

pub const HOTBAR_SLOTS: usize = 9;
/// The hotbar and the rows above it.
pub const MAIN_SLOTS: usize = 36;
/// The off-hand slot, after them (modern Minecraft's slot 40).
pub const OFFHAND: usize = 36;
/// The armor slots, after the off-hand: boots, leggings, chestplate and
/// helmet (modern's slots 36–39 in this order).
pub const ARMOR_FEET: usize = 37;
pub const ARMOR_LEGS: usize = 38;
pub const ARMOR_CHEST: usize = 39;
pub const ARMOR_HEAD: usize = 40;
pub const SLOTS: usize = 41;

/// A hand: items are used from the main hand, or the off-hand when the
/// main hand has nothing to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum Hand {
    #[default]
    Main,
    Off,
}

impl Hand {
    pub fn opposite(self) -> Self {
        match self {
            Hand::Main => Hand::Off,
            Hand::Off => Hand::Main,
        }
    }
}

/// Whether `index` is one of the four armor slots.
pub fn is_armor_slot(index: usize) -> bool {
    (ARMOR_FEET..=ARMOR_HEAD).contains(&index)
}

/// The armor slot `stack` goes in, if it's armor.
pub fn armor_slot_of(items: &Items, stack: &ItemStack) -> Option<usize> {
    let def = items.item(stack.id)?;
    super::armor::armor_of(&def.name).map(|armor| armor.slot.index())
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Inventory {
    /// `SLOTS` long: 0–8 are the hotbar, 9–35 the rest, 36 the off-hand and
    /// 37–40 the armor.
    slots: Vec<Option<ItemStack>>,
    /// Selected hotbar slot.
    pub selected: usize,
}

impl Default for Inventory {
    fn default() -> Self {
        Self {
            slots: vec![None; SLOTS],
            selected: 0,
        }
    }
}

impl Inventory {
    pub fn slot(&self, index: usize) -> Option<&ItemStack> {
        self.slots[index].as_ref()
    }

    pub fn set(&mut self, index: usize, stack: Option<ItemStack>) {
        self.slots[index] = stack;
    }

    pub fn selected_stack(&self) -> Option<&ItemStack> {
        self.slot(self.selected)
    }

    /// The slot a hand's item is in.
    pub fn hand_slot(&self, hand: Hand) -> usize {
        match hand {
            Hand::Main => self.selected,
            Hand::Off => OFFHAND,
        }
    }

    /// What `hand` holds.
    pub fn hand_stack(&self, hand: Hand) -> Option<&ItemStack> {
        self.slot(self.hand_slot(hand))
    }

    /// Swaps the main hand's item with the off-hand's (the F key).
    pub fn swap_hands(&mut self) {
        self.slots.swap(self.selected, OFFHAND);
    }

    /// Fixes a loaded inventory: the right number of slots, a valid
    /// selection, and no stacks of items that don't exist or are empty.
    pub fn repair(&mut self, items: &Items) {
        self.slots.resize(SLOTS, None);
        self.selected = self.selected.min(HOTBAR_SLOTS - 1);
        for slot in &mut self.slots {
            if slot
                .as_ref()
                .is_some_and(|stack| stack.count == 0 || !items.exists(stack.id))
            {
                *slot = None;
            }
        }
    }

    /// Picks up `stack`, filling matching stacks first, then empty slots,
    /// both hotbar first (1.0.0's `addItemStackToInventory`). Returns what
    /// didn't fit.
    pub fn add(&mut self, stack: ItemStack, items: &Items) -> Option<ItemStack> {
        self.merge(stack, 0..MAIN_SLOTS, items)
    }

    /// Moves `stack` into the slots at the indices in `order` (matching
    /// stacks first, then empty slots). Returns what didn't fit.
    pub fn merge(
        &mut self,
        stack: ItemStack,
        order: impl Iterator<Item = usize> + Clone,
        items: &Items,
    ) -> Option<ItemStack> {
        slots::merge(&mut self.slots, order, stack, items)
    }

    /// Takes up to `count` items from `index`.
    pub fn take(&mut self, index: usize, count: u8) -> Option<ItemStack> {
        let slot = self.slots[index].as_mut()?;
        let taken = count.min(slot.count);
        slot.count -= taken;
        let stack = ItemStack {
            count: taken,
            ..slot.clone()
        };
        if slot.count == 0 {
            self.slots[index] = None;
        }
        Some(stack)
    }

    /// The first slot holding item `id`, if any.
    pub fn find(&self, id: ItemId) -> Option<usize> {
        self.slots
            .iter()
            .position(|slot| slot.as_ref().is_some_and(|stack| stack.id == id))
    }

    /// Wears the selected item by `amount`. Returns true if it broke.
    pub fn damage_selected(&mut self, amount: u16, items: &Items) -> bool {
        self.damage_slot(self.selected, amount, items)
    }

    /// Wears the item in a hand by `amount`. Returns true if it broke.
    pub fn damage_hand(&mut self, hand: Hand, amount: u16, items: &Items) -> bool {
        self.damage_slot(self.hand_slot(hand), amount, items)
    }

    /// Wears the item in slot `index` by `amount`. Returns true if it broke.
    pub fn damage_slot(&mut self, index: usize, amount: u16, items: &Items) -> bool {
        let Some(stack) = self.slots[index].as_mut() else {
            return false;
        };
        let Some(max) = items.max_damage(stack.id) else {
            return false;
        };
        stack.damage = stack.damage.saturating_add(amount);
        // 1.0.0 breaks the item once damage exceeds its durability.
        if stack.damage > max {
            self.slots[index] = None;
            return true;
        }
        false
    }

    /// Clicks slot `index` with `cursor` held, following 1.0.0's
    /// `Container.slotClick`.
    pub fn click(
        &mut self,
        index: usize,
        click: Click,
        cursor: &mut Option<ItemStack>,
        items: &Items,
    ) {
        if click == Click::Shift {
            return self.transfer(index, items);
        }
        // An armor slot takes only its own kind of piece.
        if is_armor_slot(index)
            && cursor
                .as_ref()
                .is_some_and(|held| armor_slot_of(items, held) != Some(index))
        {
            return;
        }
        slots::click(&mut self.slots[index], click, cursor, items);
    }

    /// Puts the armor in slot `from` on, swapping with what is worn there
    /// (right-click with it in hand). Returns whether it was armor.
    pub fn equip(&mut self, from: usize, items: &Items) -> bool {
        let Some(slot) = self.slots[from]
            .as_ref()
            .and_then(|stack| armor_slot_of(items, stack))
        else {
            return false;
        };
        self.slots.swap(from, slot);
        true
    }

    /// Shift-click: moves a hotbar stack into the rest of the inventory,
    /// and anything else into the hotbar (1.0.0's `transferStackInSlot`).
    fn transfer(&mut self, index: usize, items: &Items) {
        let Some(stack) = self.slots[index].take() else {
            return;
        };
        // Armor goes on if its place is free; worn armor comes off.
        if index < MAIN_SLOTS
            && let Some(slot) = armor_slot_of(items, &stack)
            && self.slots[slot].is_none()
        {
            self.slots[slot] = Some(stack);
            return;
        }
        let target = if is_armor_slot(index) {
            0..MAIN_SLOTS
        } else if index < HOTBAR_SLOTS {
            HOTBAR_SLOTS..MAIN_SLOTS
        } else if index == OFFHAND {
            0..MAIN_SLOTS
        } else {
            0..HOTBAR_SLOTS
        };
        self.slots[index] = self.merge(stack, target, items);
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::inventory::items;

    pub fn items() -> Items {
        Items::new()
    }

    const STONE: ItemId = items::STONE;
    const DIRT: ItemId = items::DIRT;
    const SNOWBALL: ItemId = items::SNOWBALL;
    const PICKAXE: ItemId = items::WOODEN_PICKAXE;

    fn counts(inventory: &Inventory) -> Vec<(usize, ItemId, u8)> {
        (0..SLOTS)
            .filter_map(|i| inventory.slot(i).map(|s| (i, s.id, s.count)))
            .collect()
    }

    #[test]
    fn picking_up_fills_matching_stacks_then_empty_slots_hotbar_first() {
        let items = items();
        let mut inventory = Inventory::default();
        inventory.set(4, Some(ItemStack::new(STONE, 60)));
        inventory.set(20, Some(ItemStack::new(STONE, 63)));
        assert_eq!(inventory.add(ItemStack::new(STONE, 10), &items), None);
        assert_eq!(
            counts(&inventory),
            [(0, STONE, 5), (4, STONE, 64), (20, STONE, 64)]
        );
        // Snowballs stack to 16.
        assert_eq!(inventory.add(ItemStack::new(SNOWBALL, 20), &items), None);
        assert_eq!(inventory.slot(1).unwrap().count, 16);
        assert_eq!(inventory.slot(2).unwrap().count, 4);
        // Tools never stack.
        inventory.add(ItemStack::new(PICKAXE, 1), &items);
        inventory.add(ItemStack::new(PICKAXE, 1), &items);
        assert_eq!(inventory.slot(3).unwrap().id, PICKAXE);
        assert_eq!(inventory.slot(5).unwrap().id, PICKAXE);
    }

    #[test]
    fn a_full_inventory_returns_the_rest() {
        let items = items();
        let mut inventory = Inventory::default();
        for i in 0..MAIN_SLOTS {
            inventory.set(i, Some(ItemStack::new(DIRT, 63)));
        }
        let rest = inventory.add(ItemStack::new(DIRT, 40), &items);
        assert_eq!(rest, Some(ItemStack::new(DIRT, 4)));
        assert_eq!(
            inventory.add(ItemStack::new(STONE, 1), &items),
            Some(ItemStack::new(STONE, 1))
        );
    }

    #[test]
    fn the_off_hand_swaps_with_the_main_hand_and_isnt_filled_by_pickups() {
        let items = items();
        let mut inventory = Inventory::default();
        inventory.set(2, Some(ItemStack::new(PICKAXE, 1)));
        inventory.set(OFFHAND, Some(ItemStack::new(STONE, 5)));
        inventory.selected = 2;
        assert_eq!(inventory.hand_stack(Hand::Main).unwrap().id, PICKAXE);
        assert_eq!(inventory.hand_stack(Hand::Off).unwrap().id, STONE);
        inventory.swap_hands();
        assert_eq!(inventory.hand_stack(Hand::Main).unwrap().id, STONE);
        assert_eq!(inventory.hand_stack(Hand::Off).unwrap().id, PICKAXE);
        // Swapping with an empty hand moves the item.
        inventory.set(OFFHAND, None);
        inventory.swap_hands();
        assert!(inventory.hand_stack(Hand::Main).is_none());
        assert_eq!(inventory.hand_stack(Hand::Off).unwrap().id, STONE);
        // Picking up never lands in the off-hand, even with the rest full.
        let mut full = Inventory::default();
        for i in 0..MAIN_SLOTS {
            full.set(i, Some(ItemStack::new(DIRT, 64)));
        }
        assert!(full.add(ItemStack::new(STONE, 1), &items).is_some());
        assert!(full.slot(OFFHAND).is_none());
        // Shift-clicking the off-hand sends it to the main slots.
        inventory.click(OFFHAND, Click::Shift, &mut None, &items);
        assert!(inventory.slot(OFFHAND).is_none());
        assert_eq!(inventory.slot(0).unwrap().id, STONE);
        // Wearing an off-hand item breaks it at its durability.
        let mut worn = Inventory::default();
        worn.set(OFFHAND, Some(ItemStack::new(PICKAXE, 1)));
        assert!(!worn.damage_hand(Hand::Off, 30, &items));
        assert!(worn.damage_hand(Hand::Off, 30, &items));
        assert!(worn.slot(OFFHAND).is_none());
        // Old 36-slot saves grow the extra slot.
        let mut old = Inventory::default();
        old.slots.truncate(MAIN_SLOTS);
        old.repair(&items);
        assert_eq!(old.slots.len(), SLOTS);
    }

    #[test]
    fn different_colors_keep_stacks_apart() {
        let items = items();
        let mut inventory = Inventory::default();
        inventory.add(ItemStack::new(items::RED_WOOL, 5), &items);
        inventory.add(ItemStack::new(items::ORANGE_WOOL, 5), &items);
        assert_eq!(inventory.slot(0).unwrap().id, items::RED_WOOL);
        assert_eq!(inventory.slot(1).unwrap().id, items::ORANGE_WOOL);
    }

    #[test]
    fn left_click_picks_up_puts_down_merges_and_swaps() {
        let items = items();
        let mut inventory = Inventory::default();
        let mut cursor = None;
        inventory.set(0, Some(ItemStack::new(STONE, 40)));
        inventory.set(1, Some(ItemStack::new(STONE, 30)));
        inventory.set(2, Some(ItemStack::new(DIRT, 7)));

        inventory.click(0, Click::Left, &mut cursor, &items);
        assert_eq!(
            (inventory.slot(0), cursor.clone()),
            (None, Some(ItemStack::new(STONE, 40)))
        );
        // Merging fills slot 1 to 64 and keeps the rest held.
        inventory.click(1, Click::Left, &mut cursor, &items);
        assert_eq!(inventory.slot(1).unwrap().count, 64);
        assert_eq!(cursor, Some(ItemStack::new(STONE, 6)));
        // A different item swaps with the cursor.
        inventory.click(2, Click::Left, &mut cursor, &items);
        assert_eq!(inventory.slot(2), Some(&ItemStack::new(STONE, 6)));
        assert_eq!(cursor, Some(ItemStack::new(DIRT, 7)));
        inventory.click(5, Click::Left, &mut cursor, &items);
        assert_eq!((inventory.slot(5).unwrap().count, cursor), (7, None));
    }

    #[test]
    fn right_click_takes_half_and_puts_one() {
        let items = items();
        let mut inventory = Inventory::default();
        let mut cursor = None;
        inventory.set(0, Some(ItemStack::new(STONE, 7)));
        inventory.click(0, Click::Right, &mut cursor, &items);
        assert_eq!(cursor, Some(ItemStack::new(STONE, 4)));
        assert_eq!(inventory.slot(0).unwrap().count, 3);
        inventory.click(0, Click::Right, &mut cursor, &items);
        inventory.click(9, Click::Right, &mut cursor, &items);
        assert_eq!(inventory.slot(0).unwrap().count, 4);
        assert_eq!(inventory.slot(9).unwrap().count, 1);
        assert_eq!(cursor, Some(ItemStack::new(STONE, 2)));
    }

    #[test]
    fn shift_click_moves_between_hotbar_and_the_rest() {
        let items = items();
        let mut inventory = Inventory::default();
        let mut cursor = None;
        inventory.set(3, Some(ItemStack::new(STONE, 10)));
        inventory.set(30, Some(ItemStack::new(STONE, 60)));
        inventory.click(3, Click::Shift, &mut cursor, &items);
        assert_eq!(counts(&inventory), [(9, STONE, 6), (30, STONE, 64)]);
        inventory.click(30, Click::Shift, &mut cursor, &items);
        assert_eq!(counts(&inventory), [(0, STONE, 64), (9, STONE, 6)]);
        assert_eq!(cursor, None);
    }

    #[test]
    fn tools_break_once_damage_passes_durability() {
        let items = items();
        let mut inventory = Inventory::default();
        inventory.set(0, Some(ItemStack::with_damage(PICKAXE, 1, 57)));
        assert!(!inventory.damage_selected(1, &items));
        assert_eq!(inventory.selected_stack().unwrap().damage, 58);
        // Damage 59 still works: it breaks once damage passes the durability,
        // so a wooden pickaxe lasts 60 blocks, as in 1.0.0.
        assert!(!inventory.damage_selected(1, &items));
        assert!(inventory.damage_selected(1, &items));
        assert_eq!(inventory.selected_stack(), None);
        // Items without durability don't wear.
        inventory.set(0, Some(ItemStack::new(STONE, 1)));
        assert!(!inventory.damage_selected(5, &items));
        assert_eq!(inventory.selected_stack().unwrap().damage, 0);
    }
}
