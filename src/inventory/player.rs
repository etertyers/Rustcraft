//! The player's 36 slots (hotbar first) and 1.0.0's rules for picking up
//! items and clicking slots with a stack held on the cursor.

use serde::{Deserialize, Serialize};

use super::stack::{ItemStack, Items};

pub const HOTBAR_SLOTS: usize = 9;
pub const SLOTS: usize = 36;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Inventory {
    /// `SLOTS` long: 0–8 are the hotbar, 9–35 the rest.
    slots: Vec<Option<ItemStack>>,
    /// Selected hotbar slot.
    pub selected: usize,
}

/// A mouse click on an inventory slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Click {
    Left,
    Right,
    /// Shift + left click: move the stack between the hotbar and the rest.
    Shift,
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

    /// Fixes a loaded inventory: the right number of slots, a valid
    /// selection, and no stacks of items that don't exist or are empty.
    pub fn repair(&mut self, items: &Items) {
        self.slots.resize(SLOTS, None);
        self.selected = self.selected.min(HOTBAR_SLOTS - 1);
        for slot in &mut self.slots {
            if slot.is_some_and(|stack| stack.count == 0 || !items.exists(stack.id)) {
                *slot = None;
            }
        }
    }

    /// Picks up `stack`, filling matching stacks first, then empty slots,
    /// both hotbar first (1.0.0's `addItemStackToInventory`). Returns what
    /// didn't fit.
    pub fn add(&mut self, stack: ItemStack, items: &Items) -> Option<ItemStack> {
        self.merge_into(stack, 0..SLOTS, items)
    }

    /// Moves `stack` into slots in `range`: onto matching stacks, then into
    /// the first empty slots. Returns what didn't fit.
    fn merge_into(
        &mut self,
        mut stack: ItemStack,
        range: std::ops::Range<usize>,
        items: &Items,
    ) -> Option<ItemStack> {
        let max = items.max_stack(stack.id);
        for slot in self.slots[range.clone()].iter_mut().flatten() {
            if slot.stacks_with(&stack) && slot.count < max {
                let moved = stack.count.min(max - slot.count);
                slot.count += moved;
                stack.count -= moved;
                if stack.count == 0 {
                    return None;
                }
            }
        }
        for slot in &mut self.slots[range] {
            if slot.is_none() {
                let moved = stack.count.min(max);
                *slot = Some(ItemStack {
                    count: moved,
                    ..stack
                });
                stack.count -= moved;
                if stack.count == 0 {
                    return None;
                }
            }
        }
        Some(stack)
    }

    /// Takes up to `count` items from `index`.
    pub fn take(&mut self, index: usize, count: u8) -> Option<ItemStack> {
        let slot = self.slots[index].as_mut()?;
        let taken = count.min(slot.count);
        slot.count -= taken;
        let stack = ItemStack {
            count: taken,
            ..*slot
        };
        if slot.count == 0 {
            self.slots[index] = None;
        }
        Some(stack)
    }

    /// Wears the selected item by `amount`. Returns true if it broke.
    pub fn damage_selected(&mut self, amount: u16, items: &Items) -> bool {
        let Some(stack) = self.slots[self.selected].as_mut() else {
            return false;
        };
        let Some(max) = items.max_damage(stack.id) else {
            return false;
        };
        stack.damage = stack.damage.saturating_add(amount);
        // 1.0.0 breaks the item once damage exceeds its durability.
        if stack.damage > max {
            self.slots[self.selected] = None;
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
        let slot = &mut self.slots[index];
        match (slot.as_mut(), cursor.as_mut()) {
            (None, None) => {}
            // Pick up everything, or half rounded up.
            (Some(stack), None) => {
                let taken = match click {
                    Click::Right => stack.count.div_ceil(2),
                    _ => stack.count,
                };
                *cursor = Some(ItemStack {
                    count: taken,
                    ..*stack
                });
                stack.count -= taken;
                if stack.count == 0 {
                    *slot = None;
                }
            }
            // Put down everything that fits, or one.
            (None, Some(held)) => {
                let put = match click {
                    Click::Right => 1,
                    _ => held.count.min(items.max_stack(held.id)),
                };
                *slot = Some(ItemStack {
                    count: put,
                    ..*held
                });
                held.count -= put;
                if held.count == 0 {
                    *cursor = None;
                }
            }
            // Add onto a matching stack.
            (Some(stack), Some(held)) if stack.stacks_with(held) => {
                let room = items.max_stack(stack.id).saturating_sub(stack.count);
                let put = match click {
                    Click::Right => 1.min(room),
                    _ => held.count.min(room),
                };
                stack.count += put;
                held.count -= put;
                if held.count == 0 {
                    *cursor = None;
                }
            }
            // Different items swap.
            (Some(_), Some(_)) => std::mem::swap(slot, cursor),
        }
    }

    /// Shift-click: moves a hotbar stack into the rest of the inventory,
    /// and anything else into the hotbar (1.0.0's `transferStackInSlot`).
    fn transfer(&mut self, index: usize, items: &Items) {
        let Some(stack) = self.slots[index].take() else {
            return;
        };
        let target = if index < HOTBAR_SLOTS {
            HOTBAR_SLOTS..SLOTS
        } else {
            0..HOTBAR_SLOTS
        };
        self.slots[index] = self.merge_into(stack, target, items);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::{inventory::item::ItemRegistry, world::block::BlockRegistry};

    pub fn items() -> Items {
        let assets = crate::assets::dir();
        Items::new(
            Arc::new(BlockRegistry::load(&assets.join("data/blocks.ron")).unwrap()),
            Arc::new(ItemRegistry::load(&assets.join("data/items.ron")).unwrap()),
        )
    }

    const STONE: u16 = 1;
    const DIRT: u16 = 3;
    const SNOWBALL: u16 = 332;
    const PICKAXE: u16 = 270;

    fn counts(inventory: &Inventory) -> Vec<(usize, u16, u8)> {
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
        for i in 0..SLOTS {
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
    fn data_values_keep_stacks_apart() {
        let items = items();
        let mut inventory = Inventory::default();
        inventory.add(ItemStack::with_damage(35, 5, 14), &items);
        inventory.add(ItemStack::with_damage(35, 5, 1), &items);
        assert_eq!(inventory.slot(0).unwrap().damage, 14);
        assert_eq!(inventory.slot(1).unwrap().damage, 1);
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
            (inventory.slot(0), cursor),
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
