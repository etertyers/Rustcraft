//! Screens with slots beyond the inventory, following 1.0.0's
//! `ContainerPlayer` and `ContainerWorkbench`: the inventory's 2×2 crafting
//! grid and the crafting table's 3×3, whose items are thrown out when the
//! screen closes.

use super::Game;
use crate::inventory::{
    player::{HOTBAR_SLOTS, SLOTS},
    slots::{self, Click, SlotRef},
    stack::ItemStack,
};

/// What's open besides the inventory.
#[derive(Debug, Clone, PartialEq)]
pub enum Container {
    /// A `size` × `size` crafting grid, row by row: 2 in the inventory, 3
    /// at a crafting table.
    Crafting {
        size: usize,
        grid: Vec<Option<ItemStack>>,
    },
}

impl Container {
    fn crafting(size: usize) -> Self {
        Self::Crafting {
            size,
            grid: vec![None; size * size],
        }
    }
}

/// The player's slots in 1.0.0's container order: the main inventory top
/// row first, then the hotbar.
fn player_slots() -> impl DoubleEndedIterator<Item = usize> + Clone {
    (HOTBAR_SLOTS..SLOTS).chain(0..HOTBAR_SLOTS)
}

impl Game {
    /// Opens the inventory screen's crafting grid (survival only; creative's
    /// screen has none).
    pub fn open_inventory(&mut self) {
        self.container = (!self.mode.has_every_item()).then(|| Container::crafting(2));
    }

    /// Opens a crafting table's 3×3 grid.
    pub(super) fn open_crafting_table(&mut self) {
        self.container = Some(Container::crafting(3));
        self.release_input();
    }

    /// Closes whatever is open, throwing out the crafting grid and the
    /// stack held on the mouse (1.0.0's `onCraftGuiClosed`).
    pub fn close_container(&mut self) {
        let mut thrown: Vec<ItemStack> = self.cursor.take().into_iter().collect();
        if let Some(Container::Crafting { grid, .. }) = self.container.take() {
            thrown.extend(grid.into_iter().flatten());
        }
        for stack in thrown {
            self.throw(stack);
        }
    }

    /// What the crafting grid makes, if one is open.
    pub fn crafting_result(&self) -> Option<ItemStack> {
        let Some(Container::Crafting { size, grid }) = &self.container else {
            return None;
        };
        self.items.recipes.craft(grid, *size)
    }

    /// The stack in a slot of the open screen.
    pub fn slot_stack(&self, slot: SlotRef) -> Option<ItemStack> {
        match (slot, &self.container) {
            (SlotRef::Inventory(index), _) => self.inventory.slot(index).copied(),
            (SlotRef::Grid(index), Some(Container::Crafting { grid, .. })) => {
                grid.get(index).copied().flatten()
            }
            (SlotRef::Result, _) => self.crafting_result(),
            (SlotRef::Grid(_), None) => None,
        }
    }

    /// Clicks a slot of the open screen.
    pub fn click_slot(&mut self, slot: SlotRef, click: Click) {
        let items = self.items.clone();
        match slot {
            SlotRef::Inventory(index) if click == Click::Shift => {
                self.transfer_from_inventory(index)
            }
            SlotRef::Inventory(index) => {
                self.inventory.click(index, click, &mut self.cursor, &items);
            }
            SlotRef::Grid(index) => {
                let Some(Container::Crafting { grid, .. }) = &mut self.container else {
                    return;
                };
                let Some(cell) = grid.get_mut(index) else {
                    return;
                };
                if click == Click::Shift {
                    if let Some(stack) = cell.take() {
                        *cell = self.inventory.merge(stack, player_slots(), &items);
                    }
                } else {
                    slots::click(cell, click, &mut self.cursor, &items);
                }
            }
            SlotRef::Result if click == Click::Shift => {
                // Crafts as many as fit, each into the inventory hotbar end
                // first (1.0.0 merges results in reverse), until the grid
                // makes something else.
                let first = self.crafting_result();
                while let Some(result) = self.crafting_result()
                    && Some(result) == first
                {
                    let mut trial = self.inventory.clone();
                    if trial.merge(result, player_slots().rev(), &items).is_some() {
                        break;
                    }
                    self.inventory = trial;
                    self.use_up_grid();
                }
            }
            SlotRef::Result => {
                if let Some(result) = self.crafting_result()
                    && slots::take_onto_cursor(result, &mut self.cursor, &items)
                {
                    self.use_up_grid();
                }
            }
        }
    }

    /// Takes one item from every filled grid cell after crafting.
    fn use_up_grid(&mut self) {
        let Some(Container::Crafting { grid, .. }) = &mut self.container else {
            return;
        };
        for cell in grid.iter_mut() {
            if let Some(stack) = cell {
                stack.count -= 1;
                if stack.count == 0 {
                    *cell = None;
                }
            }
        }
    }

    /// Shift-click on the player's slots: between the hotbar and the rest.
    fn transfer_from_inventory(&mut self, index: usize) {
        let items = self.items.clone();
        let mut cursor = None;
        self.inventory
            .click(index, Click::Shift, &mut cursor, &items);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{game::tests::flat_game, player::mode::GameMode, world::block::BlockState};

    fn grid_of(game: &Game) -> Vec<Option<ItemStack>> {
        match &game.container {
            Some(Container::Crafting { grid, .. }) => grid.clone(),
            None => panic!("nothing open"),
        }
    }

    #[test]
    fn the_inventory_grid_crafts_planks_one_click_at_a_time() {
        let mut game = flat_game(GameMode::Survival);
        game.open_inventory();
        game.inventory.set(0, Some(ItemStack::new(17, 3)));
        game.click_slot(SlotRef::Inventory(0), Click::Left);
        game.click_slot(SlotRef::Grid(3), Click::Left);
        assert_eq!(game.slot_stack(SlotRef::Result), Some(ItemStack::new(5, 4)));
        game.click_slot(SlotRef::Result, Click::Left);
        game.click_slot(SlotRef::Result, Click::Left);
        assert_eq!(game.cursor, Some(ItemStack::new(5, 8)));
        assert_eq!(grid_of(&game)[3], Some(ItemStack::new(17, 1)));
        // Holding something else, the result can't be taken.
        game.click_slot(SlotRef::Inventory(5), Click::Left);
        game.inventory.set(6, Some(ItemStack::new(3, 1)));
        game.click_slot(SlotRef::Inventory(6), Click::Left);
        game.click_slot(SlotRef::Result, Click::Left);
        assert_eq!(game.cursor, Some(ItemStack::new(3, 1)));
        assert_eq!(grid_of(&game)[3], Some(ItemStack::new(17, 1)));
    }

    #[test]
    fn shift_clicking_the_result_crafts_all_it_can() {
        let mut game = flat_game(GameMode::Survival);
        game.open_crafting_table();
        // Sticks: two planks stacked, 10 and 7 of them, make 7 × 4.
        game.container = Some(Container::Crafting {
            size: 3,
            grid: vec![
                None,
                Some(ItemStack::new(5, 10)),
                None,
                None,
                Some(ItemStack::new(5, 7)),
                None,
                None,
                None,
                None,
            ],
        });
        game.click_slot(SlotRef::Result, Click::Shift);
        assert_eq!(game.inventory.slot(8), Some(&ItemStack::new(280, 28)));
        assert_eq!(grid_of(&game)[1], Some(ItemStack::new(5, 3)));
        assert_eq!(grid_of(&game)[4], None);
        assert_eq!(game.slot_stack(SlotRef::Result), None);
    }

    #[test]
    fn closing_throws_out_the_grid_and_the_held_stack() {
        let mut game = flat_game(GameMode::Survival);
        game.open_inventory();
        game.inventory.set(0, Some(ItemStack::new(4, 5)));
        game.click_slot(SlotRef::Inventory(0), Click::Right);
        game.click_slot(SlotRef::Grid(0), Click::Right);
        game.click_slot(SlotRef::Grid(1), Click::Right);
        assert_eq!(game.cursor, Some(ItemStack::new(4, 1)));
        game.close_container();
        assert_eq!(game.container, None);
        assert_eq!(game.cursor, None);
        let thrown: u8 = game.entities.iter().map(|item| item.stack.count).sum();
        assert_eq!(thrown, 3);
        assert_eq!(game.inventory.slot(0), Some(&ItemStack::new(4, 2)));
    }

    #[test]
    fn right_clicking_a_crafting_table_opens_it_even_holding_a_block() {
        let mut game = flat_game(GameMode::Survival);
        game.chunks
            .set_blocks(&[(glam::IVec3::new(0, 63, 0), BlockState::new(58, 0))]);
        game.inventory.set(0, Some(ItemStack::new(3, 5)));
        game.advance(0.0);
        game.place_clicked = true;
        game.tick();
        assert_eq!(game.container, Some(Container::crafting(3)));
        assert_eq!(
            game.inventory.slot(0),
            Some(&ItemStack::new(3, 5)),
            "nothing placed"
        );
    }

    #[test]
    fn creative_has_no_crafting_grid() {
        let mut game = flat_game(GameMode::Creative);
        game.open_inventory();
        assert_eq!(game.container, None);
        assert_eq!(game.slot_stack(SlotRef::Result), None);
    }
}
