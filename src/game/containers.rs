//! Screens with slots beyond the inventory, following 1.0.0's
//! `ContainerPlayer`, `ContainerWorkbench` and `ContainerChest`: the
//! inventory's 2×2 crafting grid and the crafting table's 3×3, whose items
//! are thrown out when the screen closes, and chests, whose slots are kept
//! in the world.

use glam::IVec3;

use super::Game;
use crate::{
    inventory::{
        player::{HOTBAR_SLOTS, SLOTS},
        slots::{self, Click, SlotRef},
        stack::ItemStack,
    },
    world::block_entity::BlockEntity,
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
    /// The chest at this block.
    Chest(IVec3),
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

    /// Opens the chest at `at`, giving it its slots the first time. A solid
    /// block on top keeps it shut (1.0.0).
    pub(super) fn open_chest(&mut self, at: IVec3) {
        let above = self.chunks.world().block(at.x, at.y + 1, at.z);
        if above.is_some_and(|block| {
            self.items
                .blocks
                .get(block.id())
                .is_some_and(|def| def.is_opaque_cube())
        }) {
            return;
        }
        if !matches!(self.chunks.block_entity(at), Some(BlockEntity::Chest(_))) {
            self.chunks.set_block_entity(at, Some(BlockEntity::chest()));
        }
        self.container = Some(Container::Chest(at));
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

    /// The open block's slots (a chest's).
    fn block_slots(&self) -> Option<&[Option<ItemStack>]> {
        let Some(Container::Chest(at)) = self.container else {
            return None;
        };
        self.chunks.block_entity(at).map(BlockEntity::slots)
    }

    fn block_slots_mut(&mut self) -> Option<&mut [Option<ItemStack>]> {
        let Some(Container::Chest(at)) = self.container else {
            return None;
        };
        self.chunks.block_entity_mut(at).map(BlockEntity::slots_mut)
    }

    /// The stack in a slot of the open screen.
    pub fn slot_stack(&self, slot: SlotRef) -> Option<ItemStack> {
        match slot {
            SlotRef::Inventory(index) => self.inventory.slot(index).copied(),
            SlotRef::Grid(index) => match &self.container {
                Some(Container::Crafting { grid, .. }) => grid.get(index).copied().flatten(),
                _ => None,
            },
            SlotRef::Result => self.crafting_result(),
            SlotRef::Block(index) => self.block_slots()?.get(index).copied().flatten(),
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
            SlotRef::Block(index) if click == Click::Shift => {
                // Into the inventory, hotbar end first (1.0.0).
                let Some(stack) = self
                    .block_slots_mut()
                    .and_then(|s| s.get_mut(index)?.take())
                else {
                    return;
                };
                let rest = self.inventory.merge(stack, player_slots().rev(), &items);
                if let Some(cell) = self.block_slots_mut().and_then(|s| s.get_mut(index)) {
                    *cell = rest;
                }
            }
            SlotRef::Block(index) => {
                let mut cursor = self.cursor.take();
                if let Some(cell) = self.block_slots_mut().and_then(|s| s.get_mut(index)) {
                    slots::click(cell, click, &mut cursor, &items);
                }
                self.cursor = cursor;
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

    /// Shift-click on the player's slots: into an open chest, or else
    /// between the hotbar and the rest.
    fn transfer_from_inventory(&mut self, index: usize) {
        let items = self.items.clone();
        if let Some(Container::Chest(_)) = self.container {
            let Some(stack) = self.inventory.slot(index).copied() else {
                return;
            };
            let Some(chest) = self.block_slots_mut() else {
                return;
            };
            let rest = slots::merge(chest, 0..chest.len(), stack, &items);
            self.inventory.set(index, rest);
            return;
        }
        let mut cursor = None;
        self.inventory
            .click(index, Click::Shift, &mut cursor, &items);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use glam::DVec3;

    use crate::{
        game::tests::{flat_game, load_player_chunk, registries},
        inventory::stack::Items,
        level::WorldDir,
        player::mode::GameMode,
        world::{block::BlockState, mesh::BlockModels, save::TempDir},
    };

    fn grid_of(game: &Game) -> Vec<Option<ItemStack>> {
        match &game.container {
            Some(Container::Crafting { grid, .. }) => grid.clone(),
            _ => panic!("no crafting grid open"),
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
    fn a_chest_keeps_its_items_across_saving_and_spills_them_when_broken() {
        let saves = TempDir::new("chest");
        let (items, models) = registries();
        let open = |items: &Items, models: &Arc<BlockModels>| {
            let dir = WorldDir::new(saves.path().join("Chest Test"));
            let mut game = Game::open(dir, items.clone(), models.clone()).unwrap();
            load_player_chunk(&mut game);
            game
        };
        Game::create(
            saves.path(),
            "Chest Test",
            7,
            GameMode::Survival,
            items.clone(),
            models.clone(),
        )
        .unwrap();
        let mut game = open(&items, &models);
        let at = (game.body.position() + DVec3::new(2.0, 0.0, 0.0))
            .floor()
            .as_ivec3();
        game.chunks.set_blocks(&[(at, BlockState::new(54, 3))]);
        game.open_chest(at);
        assert_eq!(game.container, Some(Container::Chest(at)));
        game.inventory.set(4, Some(ItemStack::new(265, 10)));
        game.inventory
            .set(20, Some(ItemStack::with_damage(278, 1, 9)));
        game.click_slot(SlotRef::Inventory(4), Click::Shift);
        game.click_slot(SlotRef::Inventory(20), Click::Left);
        game.click_slot(SlotRef::Block(13), Click::Left);
        assert_eq!(
            game.slot_stack(SlotRef::Block(0)),
            Some(ItemStack::new(265, 10))
        );
        assert_eq!(game.inventory.slot(4), None);
        game.close_container();
        assert!(game.entities.is_empty(), "a chest's items aren't thrown");
        game.save().unwrap();
        drop(game);

        let mut game = open(&items, &models);
        let held = game.chunks.block_entity(at).map(BlockEntity::contents);
        assert_eq!(
            held,
            Some(vec![
                ItemStack::new(265, 10),
                ItemStack::with_damage(278, 1, 9)
            ])
        );
        // Shift-clicking takes a stack back out, hotbar end first.
        game.open_chest(at);
        game.click_slot(SlotRef::Block(0), Click::Shift);
        assert_eq!(game.inventory.slot(8), Some(&ItemStack::new(265, 10)));
        game.close_container();
        game.break_block(at);
        assert_eq!(game.chunks.block_entity(at), None);
        let spilled: Vec<u16> = game.entities.iter().map(|item| item.stack.id).collect();
        assert!(spilled.contains(&278), "the pickaxe spills: {spilled:?}");
        assert!(spilled.contains(&54), "and the chest drops itself");
    }

    #[test]
    fn a_solid_block_on_top_keeps_a_chest_shut() {
        let mut game = flat_game(GameMode::Survival);
        let at = glam::IVec3::new(2, 64, 0);
        game.chunks.set_blocks(&[
            (at, BlockState::new(54, 3)),
            (at + glam::IVec3::Y, BlockState::new(1, 0)),
        ]);
        game.open_chest(at);
        assert_eq!(game.container, None);
        game.chunks
            .set_blocks(&[(at + glam::IVec3::Y, BlockState::new(50, 5))]);
        game.open_chest(at);
        assert_eq!(
            game.container,
            Some(Container::Chest(at)),
            "a torch is fine"
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
