//! Screens with slots beyond the inventory, following 1.0.0's
//! `ContainerPlayer`, `ContainerWorkbench`, `ContainerChest` and
//! `ContainerFurnace`: the inventory's 2×2 crafting grid and the crafting
//! table's 3×3, whose items are thrown out when the screen closes, and
//! chests and furnaces, whose slots are kept in the world. Furnaces burn on
//! in every loaded chunk.

use glam::IVec3;

use super::Game;
use crate::{
    inventory::{
        furnace::{self, Furnace},
        player::{HOTBAR_SLOTS, SLOTS},
        slots::{self, Click, SlotRef},
        stack::ItemStack,
    },
    world::{block_entity::BlockEntity, blocks, props::Prop},
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
    /// The furnace at this block.
    Furnace(IVec3),
    /// The dispenser or dropper at this block (`true` for a dropper).
    Dispenser(IVec3, bool),
    /// The hopper at this block.
    Hopper(IVec3),
    /// The chest or hopper minecart with this index.
    Cart(usize),
    /// An enchanting table, anvil or grindstone, with the stacks put in it.
    Bench(Bench),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BenchKind {
    Enchanting,
    Anvil,
    Grindstone,
}

/// The screen of an enchanting table, anvil or grindstone. Its input
/// slots are `SlotRef::Grid`; the anvil's and grindstone's output is
/// `SlotRef::Result`, worked out from the inputs.
#[derive(Debug, Clone, PartialEq)]
pub struct Bench {
    pub kind: BenchKind,
    pub at: IVec3,
    pub slots: Vec<Option<ItemStack>>,
    /// What's typed in the anvil's name box.
    pub name: String,
}

impl Container {
    /// The block whose slots this shows, if it shows a block's.
    pub fn block(&self) -> Option<IVec3> {
        match *self {
            Self::Crafting { .. } | Self::Cart(_) | Self::Bench(_) => None,
            Self::Chest(at) | Self::Furnace(at) | Self::Dispenser(at, _) | Self::Hopper(at) => {
                Some(at)
            }
        }
    }

    /// The grid-like slots (`SlotRef::Grid`) of a crafting grid or bench.
    pub(super) fn grid(&self) -> Option<&[Option<ItemStack>]> {
        match self {
            Self::Crafting { grid, .. } => Some(grid),
            Self::Bench(bench) => Some(&bench.slots),
            _ => None,
        }
    }

    pub(super) fn grid_mut(&mut self) -> Option<&mut [Option<ItemStack>]> {
        match self {
            Self::Crafting { grid, .. } => Some(grid),
            Self::Bench(bench) => Some(&mut bench.slots),
            _ => None,
        }
    }

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

    /// Opens the furnace at `at`, giving it its slots the first time.
    pub(super) fn open_furnace(&mut self, at: IVec3) {
        if !matches!(self.chunks.block_entity(at), Some(BlockEntity::Furnace(_))) {
            let furnace = BlockEntity::Furnace(Furnace::default());
            self.chunks.set_block_entity(at, Some(furnace));
        }
        self.container = Some(Container::Furnace(at));
        self.release_input();
    }

    /// The open furnace's fuel left and cooking progress (each 0–1).
    pub fn furnace_progress(&self) -> Option<(f32, f32)> {
        let Some(Container::Furnace(at)) = self.container else {
            return None;
        };
        match self.chunks.block_entity(at)? {
            BlockEntity::Furnace(furnace) => Some((furnace.fuel_left(), furnace.progress())),
            _ => None,
        }
    }

    /// Runs a tick of every furnace in the loaded chunks that has something
    /// to do, lighting or putting out its block.
    pub(super) fn tick_furnaces(&mut self) {
        let world = self.chunks.world();
        let mut active = Vec::new();
        for pos in world.positions() {
            let Some(chunk) = world.get(pos) else {
                continue;
            };
            let (min_x, min_z) = pos.min_block();
            for ([x, y, z], entity) in chunk.block_entities() {
                if let BlockEntity::Furnace(furnace) = entity
                    && !furnace.is_idle(&self.items)
                {
                    active.push(IVec3::new(min_x + x as i32, y as i32, min_z + z as i32));
                }
            }
        }
        for at in active {
            let Some(BlockEntity::Furnace(furnace)) = self.chunks.block_entity_mut(at) else {
                continue;
            };
            if !furnace.tick(&self.items) {
                continue;
            }
            let lit = furnace.is_burning();
            let Some(block) = self.chunks.world().block(at.x, at.y, at.z) else {
                continue;
            };
            if block.is(blocks::FURNACE) && block.bool(Prop::Lit) != lit {
                self.chunks
                    .set_blocks(&[(at, block.with_bool(Prop::Lit, lit))]);
            }
        }
    }

    /// Closes whatever is open, throwing out the crafting grid and the
    /// stack held on the mouse (1.0.0's `onCraftGuiClosed`).
    pub fn close_container(&mut self) {
        let mut thrown: Vec<ItemStack> = self.cursor.take().into_iter().collect();
        match self.container.take() {
            Some(Container::Crafting { grid, .. }) => thrown.extend(grid.into_iter().flatten()),
            // A bench gives back what was put in it, into the inventory
            // if it fits.
            Some(Container::Bench(bench)) => {
                for stack in bench.slots.into_iter().flatten() {
                    let items = self.items.clone();
                    thrown.extend(self.inventory.add(stack, &items));
                }
            }
            _ => {}
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

    /// The block whose slots are open (a chest, furnace, dispenser or
    /// hopper).
    fn open_block(&self) -> Option<IVec3> {
        self.container.as_ref().and_then(Container::block)
    }

    /// Opens the dispenser or dropper at `at`.
    pub(super) fn open_dispenser(&mut self, at: IVec3, dropper: bool) {
        self.ensure_entity(at);
        self.container = Some(Container::Dispenser(at, dropper));
        self.release_input();
    }

    /// Opens the hopper at `at`.
    pub(super) fn open_hopper(&mut self, at: IVec3) {
        self.ensure_entity(at);
        self.container = Some(Container::Hopper(at));
        self.release_input();
    }

    /// Gives the dispenser, dropper or hopper at `at` its slots if it has
    /// none yet.
    pub(super) fn ensure_entity(&mut self, at: IVec3) {
        let Some(block) = self.chunks.world().block(at.x, at.y, at.z) else {
            return;
        };
        let wanted = match block.id() {
            blocks::DISPENSER | blocks::DROPPER => BlockEntity::dispenser(),
            blocks::HOPPER => BlockEntity::Hopper(Default::default()),
            _ => return,
        };
        if self.chunks.block_entity(at).is_none() {
            self.chunks.set_block_entity(at, Some(wanted));
        }
    }

    /// Opens the chest or hopper minecart `index`.
    pub(super) fn open_cart(&mut self, index: usize) {
        self.container = Some(Container::Cart(index));
        self.release_input();
    }

    /// Closes a cart's screen when the cart is gone.
    pub(super) fn close_if_cart_gone(&mut self) {
        if let Some(Container::Cart(index)) = self.container
            && index >= self.carts.len()
        {
            self.close_container();
        }
    }

    fn block_slots(&self) -> Option<&[Option<ItemStack>]> {
        if let Some(Container::Cart(index)) = self.container {
            return self.carts.get(index).map(|cart| cart.slots.as_slice());
        }
        let at = self.open_block()?;
        self.chunks.block_entity(at).map(BlockEntity::slots)
    }

    fn block_slots_mut(&mut self) -> Option<&mut [Option<ItemStack>]> {
        if let Some(Container::Cart(index)) = self.container {
            return self
                .carts
                .get_mut(index)
                .map(|cart| cart.slots.as_mut_slice());
        }
        let at = self.open_block()?;
        self.chunks.block_entity_mut(at).map(BlockEntity::slots_mut)
    }

    /// The stack in a slot of the open screen.
    pub fn slot_stack(&self, slot: SlotRef) -> Option<ItemStack> {
        match slot {
            SlotRef::Inventory(index) => self.inventory.slot(index).cloned(),
            SlotRef::Grid(index) => match &self.container {
                Some(container) => container.grid()?.get(index).cloned().flatten(),
                None => None,
            },
            SlotRef::Result => self.crafting_result().or_else(|| self.bench_result()),
            SlotRef::Block(index) => self.block_slots()?.get(index).cloned().flatten(),
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
                // The enchanting table's lapis slot takes only lapis.
                if click != Click::Shift
                    && self
                        .cursor
                        .as_ref()
                        .is_some_and(|held| !self.slot_accepts(slot, held))
                {
                    return;
                }
                let Some(grid) = self.container.as_mut().and_then(Container::grid_mut) else {
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
            SlotRef::Result if matches!(self.container, Some(Container::Bench(_))) => {
                self.take_bench_result(click == Click::Shift);
            }
            SlotRef::Result if click == Click::Shift => {
                // Crafts as many as fit, each into the inventory hotbar end
                // first (1.0.0 merges results in reverse), until the grid
                // makes something else.
                let first = self.crafting_result();
                while let Some(result) = self.crafting_result()
                    && Some(&result) == first.as_ref()
                {
                    let mut trial = self.inventory.clone();
                    if trial
                        .merge(result.clone(), player_slots().rev(), &items)
                        .is_some()
                    {
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
                let output_before = self.furnace_output_count();
                let Some(stack) = self
                    .block_slots_mut()
                    .and_then(|s| s.get_mut(index)?.take())
                else {
                    return;
                };
                // Into the inventory: a furnace's input and fuel from the top
                // row, anything else hotbar end first (1.0.0).
                let from_furnace_input = matches!(self.container, Some(Container::Furnace(_)))
                    && index != furnace::OUTPUT;
                let rest = if from_furnace_input {
                    self.inventory.merge(stack, player_slots(), &items)
                } else {
                    self.inventory.merge(stack, player_slots().rev(), &items)
                };
                if let Some(cell) = self.block_slots_mut().and_then(|s| s.get_mut(index)) {
                    *cell = rest;
                }
                if index == furnace::OUTPUT {
                    self.pay_furnace_xp(output_before);
                }
            }
            SlotRef::Block(index) => {
                let output = matches!(self.container, Some(Container::Furnace(_)))
                    && index == furnace::OUTPUT;
                let output_before = self.furnace_output_count();
                let mut cursor = self.cursor.take();
                if let Some(cell) = self.block_slots_mut().and_then(|s| s.get_mut(index)) {
                    if output {
                        slots::click_take_only(cell, click, &mut cursor, &items);
                    } else {
                        slots::click(cell, click, &mut cursor, &items);
                    }
                }
                self.cursor = cursor;
                if output {
                    self.pay_furnace_xp(output_before);
                }
            }
        }
    }

    /// How many items sit in the open furnace's output.
    fn furnace_output_count(&self) -> u8 {
        match self.container {
            Some(Container::Furnace(_)) => self
                .block_slots()
                .and_then(|slots| slots.get(furnace::OUTPUT)?.as_ref())
                .map_or(0, |stack| stack.count),
            _ => 0,
        }
    }

    /// Takes something out of a furnace's output pays out the experience
    /// it has stored, as orbs at the player's feet.
    fn pay_furnace_xp(&mut self, output_before: u8) {
        if self.furnace_output_count() >= output_before {
            return;
        }
        let Some(Container::Furnace(at)) = self.container else {
            return;
        };
        let roll = self.rng.next_f64() as f32;
        let Some(BlockEntity::Furnace(furnace)) = self.chunks.block_entity_mut(at) else {
            return;
        };
        let points = furnace.take_xp(roll);
        let feet = self.body.position();
        self.spawn_xp(points, feet + glam::DVec3::Y * 0.5);
    }

    /// Whether `slot` is on the open screen and takes items put into it
    /// (not a result or a furnace's output).
    fn takes_items(&self, slot: SlotRef) -> bool {
        match slot {
            SlotRef::Inventory(index) => index < SLOTS,
            SlotRef::Grid(index) => self
                .container
                .as_ref()
                .and_then(Container::grid)
                .is_some_and(|grid| index < grid.len()),
            SlotRef::Result => false,
            SlotRef::Block(index) => {
                let output = matches!(self.container, Some(Container::Furnace(_)))
                    && index == furnace::OUTPUT;
                !output && self.block_slots().is_some_and(|slots| index < slots.len())
            }
        }
    }

    /// Puts `stack` in a slot that `takes_items`.
    fn set_slot(&mut self, slot: SlotRef, stack: Option<ItemStack>) {
        match slot {
            SlotRef::Inventory(index) => self.inventory.set(index, stack),
            SlotRef::Grid(index) => {
                if let Some(grid) = self.container.as_mut().and_then(Container::grid_mut) {
                    grid[index] = stack;
                }
            }
            SlotRef::Block(index) => {
                if let Some(cell) = self.block_slots_mut().and_then(|s| s.get_mut(index)) {
                    *cell = stack;
                }
            }
            SlotRef::Result => {}
        }
    }

    /// Drops the held stack across `slots` as a modern Minecraft drag: a
    /// left drag shares it evenly, a right drag puts one in each. Slots
    /// holding something else, or full, are skipped, and no more slots are
    /// used than there are items; what's left stays held.
    pub fn drag_across(&mut self, slots: &[SlotRef], click: Click) {
        let Some(held) = self.cursor.clone() else {
            return;
        };
        let max = self.items.max_stack(held.id);
        let mut targets: Vec<SlotRef> = Vec::new();
        for &slot in slots {
            let fits = self.takes_items(slot)
                && self
                    .slot_stack(slot)
                    .is_none_or(|stack| stack.stacks_with(&held) && stack.count < max);
            if fits && !targets.contains(&slot) && targets.len() < held.count as usize {
                targets.push(slot);
            }
        }
        if targets.is_empty() {
            return;
        }
        let each = match click {
            Click::Right => 1,
            _ => held.count / targets.len() as u8,
        };
        let mut left = held.count;
        for slot in targets {
            let there = self.slot_stack(slot).map_or(0, |stack| stack.count);
            let put = each.min(max - there);
            self.set_slot(
                slot,
                Some(ItemStack {
                    count: there + put,
                    ..held.clone()
                }),
            );
            left -= put;
        }
        self.cursor = (left > 0).then_some(ItemStack {
            count: left,
            ..held.clone()
        });
    }

    /// Double-click: fills the held stack from matching stacks on the open
    /// screen, part stacks first (modern Minecraft's pick-up-all). Results
    /// aren't taken from.
    pub fn collect_onto_cursor(&mut self) {
        let Some(mut held) = self.cursor.clone() else {
            return;
        };
        let max = self.items.max_stack(held.id);
        let mut slots: Vec<SlotRef> = match &self.container {
            Some(container @ (Container::Crafting { .. } | Container::Bench(_))) => {
                let count = container.grid().map_or(0, <[_]>::len);
                (0..count).map(SlotRef::Grid).collect()
            }
            Some(
                Container::Chest(_)
                | Container::Furnace(_)
                | Container::Dispenser(..)
                | Container::Hopper(_)
                | Container::Cart(_),
            ) => {
                let count = self.block_slots().map_or(0, <[_]>::len);
                (0..count).map(SlotRef::Block).collect()
            }
            None => Vec::new(),
        };
        slots.extend((0..SLOTS).map(SlotRef::Inventory));
        for full_stacks in [false, true] {
            for &slot in &slots {
                if held.count >= max {
                    break;
                }
                let Some(stack) = self.slot_stack(slot) else {
                    continue;
                };
                if !stack.stacks_with(&held) || (stack.count >= max) != full_stacks {
                    continue;
                }
                let taken = stack.count.min(max - held.count);
                held.count += taken;
                let rest = stack.count - taken;
                self.set_slot(
                    slot,
                    (rest > 0).then_some(ItemStack {
                        count: rest,
                        ..stack
                    }),
                );
            }
        }
        self.cursor = Some(held);
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
    /// between the hotbar and the rest (1.0.0's furnace doesn't sort items
    /// into its slots).
    fn transfer_from_inventory(&mut self, index: usize) {
        let items = self.items.clone();
        if let Some(
            Container::Chest(_)
            | Container::Dispenser(..)
            | Container::Hopper(_)
            | Container::Cart(_),
        ) = self.container
        {
            let Some(stack) = self.inventory.slot(index).cloned() else {
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
    use crate::inventory::items;
    use crate::world::blocks;
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
        game.inventory
            .set(0, Some(ItemStack::new(items::OAK_LOG, 3)));
        game.click_slot(SlotRef::Inventory(0), Click::Left);
        game.click_slot(SlotRef::Grid(3), Click::Left);
        assert_eq!(
            game.slot_stack(SlotRef::Result),
            Some(ItemStack::new(items::OAK_PLANKS, 4))
        );
        game.click_slot(SlotRef::Result, Click::Left);
        game.click_slot(SlotRef::Result, Click::Left);
        assert_eq!(game.cursor, Some(ItemStack::new(items::OAK_PLANKS, 8)));
        assert_eq!(grid_of(&game)[3], Some(ItemStack::new(items::OAK_LOG, 1)));
        // Holding something else, the result can't be taken.
        game.click_slot(SlotRef::Inventory(5), Click::Left);
        game.inventory.set(6, Some(ItemStack::new(items::DIRT, 1)));
        game.click_slot(SlotRef::Inventory(6), Click::Left);
        game.click_slot(SlotRef::Result, Click::Left);
        assert_eq!(game.cursor, Some(ItemStack::new(items::DIRT, 1)));
        assert_eq!(grid_of(&game)[3], Some(ItemStack::new(items::OAK_LOG, 1)));
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
                Some(ItemStack::new(items::OAK_PLANKS, 10)),
                None,
                None,
                Some(ItemStack::new(items::OAK_PLANKS, 7)),
                None,
                None,
                None,
                None,
            ],
        });
        game.click_slot(SlotRef::Result, Click::Shift);
        assert_eq!(
            game.inventory.slot(8),
            Some(&ItemStack::new(items::STICK, 28))
        );
        assert_eq!(
            grid_of(&game)[1],
            Some(ItemStack::new(items::OAK_PLANKS, 3))
        );
        assert_eq!(grid_of(&game)[4], None);
        // What planks are left over make a button, which isn't sticks.
        assert_eq!(
            game.slot_stack(SlotRef::Result),
            Some(ItemStack::new(items::OAK_BUTTON, 1))
        );
    }

    #[test]
    fn closing_throws_out_the_grid_and_the_held_stack() {
        let mut game = flat_game(GameMode::Survival);
        game.open_inventory();
        game.inventory
            .set(0, Some(ItemStack::new(items::COBBLESTONE, 5)));
        game.click_slot(SlotRef::Inventory(0), Click::Right);
        game.click_slot(SlotRef::Grid(0), Click::Right);
        game.click_slot(SlotRef::Grid(1), Click::Right);
        assert_eq!(game.cursor, Some(ItemStack::new(items::COBBLESTONE, 1)));
        game.close_container();
        assert_eq!(game.container, None);
        assert_eq!(game.cursor, None);
        let thrown: u8 = game.entities.iter().map(|item| item.stack.count).sum();
        assert_eq!(thrown, 3);
        assert_eq!(
            game.inventory.slot(0),
            Some(&ItemStack::new(items::COBBLESTONE, 2))
        );
    }

    #[test]
    fn right_clicking_a_crafting_table_opens_it_even_holding_a_block() {
        let mut game = flat_game(GameMode::Survival);
        game.chunks.set_blocks(&[(
            glam::IVec3::new(0, 63, 0),
            BlockState::of(blocks::CRAFTING_TABLE),
        )]);
        game.inventory.set(0, Some(ItemStack::new(items::DIRT, 5)));
        game.advance(0.0);
        game.place_clicked = true;
        game.tick();
        assert_eq!(game.container, Some(Container::crafting(3)));
        assert_eq!(
            game.inventory.slot(0),
            Some(&ItemStack::new(items::DIRT, 5)),
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
        game.chunks
            .set_blocks(&[(at, BlockState::of(blocks::CHEST))]);
        game.open_chest(at);
        assert_eq!(game.container, Some(Container::Chest(at)));
        game.inventory
            .set(4, Some(ItemStack::new(items::IRON_INGOT, 10)));
        game.inventory.set(
            20,
            Some(ItemStack::with_damage(items::DIAMOND_PICKAXE, 1, 9)),
        );
        game.click_slot(SlotRef::Inventory(4), Click::Shift);
        game.click_slot(SlotRef::Inventory(20), Click::Left);
        game.click_slot(SlotRef::Block(13), Click::Left);
        assert_eq!(
            game.slot_stack(SlotRef::Block(0)),
            Some(ItemStack::new(items::IRON_INGOT, 10))
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
                ItemStack::new(items::IRON_INGOT, 10),
                ItemStack::with_damage(items::DIAMOND_PICKAXE, 1, 9)
            ])
        );
        // Shift-clicking takes a stack back out, hotbar end first.
        game.open_chest(at);
        game.click_slot(SlotRef::Block(0), Click::Shift);
        assert_eq!(
            game.inventory.slot(8),
            Some(&ItemStack::new(items::IRON_INGOT, 10))
        );
        game.close_container();
        game.break_block(at);
        assert_eq!(game.chunks.block_entity(at), None);
        let spilled: Vec<_> = game.entities.iter().map(|item| item.stack.id).collect();
        assert!(
            spilled.contains(&items::DIAMOND_PICKAXE),
            "the pickaxe spills: {spilled:?}"
        );
        assert!(
            spilled.contains(&items::CHEST),
            "and the chest drops itself"
        );
    }

    #[test]
    fn a_solid_block_on_top_keeps_a_chest_shut() {
        let mut game = flat_game(GameMode::Survival);
        let at = glam::IVec3::new(2, 64, 0);
        game.chunks.set_blocks(&[
            (at, BlockState::of(blocks::CHEST)),
            (at + glam::IVec3::Y, BlockState::of(blocks::STONE)),
        ]);
        game.open_chest(at);
        assert_eq!(game.container, None);
        game.chunks
            .set_blocks(&[(at + glam::IVec3::Y, BlockState::of(blocks::TORCH))]);
        game.open_chest(at);
        assert_eq!(
            game.container,
            Some(Container::Chest(at)),
            "a torch is fine"
        );
    }

    #[test]
    fn a_furnace_lights_smelts_with_its_screen_closed_and_goes_out() {
        let mut game = flat_game(GameMode::Survival);
        let at = glam::IVec3::new(2, 64, 0);
        let block = |game: &Game| game.chunks.world().block(at.x, at.y, at.z).unwrap();
        let furnace =
            BlockState::of(blocks::FURNACE).with_face(Prop::Facing, crate::world::Face::West);
        game.chunks.set_blocks(&[(at, furnace)]);
        game.open_furnace(at);
        game.inventory
            .set(0, Some(ItemStack::new(items::IRON_ORE, 2)));
        game.inventory
            .set(1, Some(ItemStack::new(items::OAK_PLANKS, 1)));
        game.click_slot(SlotRef::Inventory(0), Click::Left);
        game.click_slot(SlotRef::Block(furnace::INPUT), Click::Left);
        game.click_slot(SlotRef::Inventory(1), Click::Left);
        // The output only gives.
        game.click_slot(SlotRef::Block(furnace::OUTPUT), Click::Left);
        assert_eq!(game.slot_stack(SlotRef::Block(furnace::OUTPUT)), None);
        game.click_slot(SlotRef::Block(furnace::FUEL), Click::Left);
        game.close_container();
        game.tick();
        assert_eq!(
            block(&game),
            furnace.with_bool(Prop::Lit, true),
            "lit, facing kept"
        );
        // A plank burns 300 ticks: one ingot, most of a second, then out.
        for _ in 0..300 {
            game.tick();
        }
        assert_eq!(block(&game), furnace);
        game.open_furnace(at);
        assert_eq!(
            game.slot_stack(SlotRef::Block(furnace::OUTPUT)),
            Some(ItemStack::new(items::IRON_INGOT, 1))
        );
        assert_eq!(
            game.furnace_progress(),
            Some((0.0, 0.0)),
            "half-done ore starts over"
        );
        game.click_slot(SlotRef::Block(furnace::OUTPUT), Click::Shift);
        assert_eq!(
            game.inventory.slot(8),
            Some(&ItemStack::new(items::IRON_INGOT, 1))
        );
    }

    #[test]
    fn dragging_shares_a_stack_evenly_or_one_each() {
        let mut game = flat_game(GameMode::Survival);
        game.open_inventory();
        game.inventory
            .set(9, Some(ItemStack::new(items::COBBLESTONE, 62)));
        game.inventory.set(11, Some(ItemStack::new(items::DIRT, 1)));
        game.cursor = Some(ItemStack::new(items::COBBLESTONE, 10));
        // Left across four slots: dirt's slot is skipped, so 10 / 3 each.
        let slots = [9, 10, 11, 12].map(SlotRef::Inventory);
        game.drag_across(&slots, Click::Left);
        assert_eq!(
            game.inventory.slot(9),
            Some(&ItemStack::new(items::COBBLESTONE, 64)),
            "capped at 64"
        );
        assert_eq!(
            game.inventory.slot(10),
            Some(&ItemStack::new(items::COBBLESTONE, 3))
        );
        assert_eq!(
            game.inventory.slot(12),
            Some(&ItemStack::new(items::COBBLESTONE, 3))
        );
        assert_eq!(
            game.cursor,
            Some(ItemStack::new(items::COBBLESTONE, 2)),
            "the rest stays held"
        );
        // Right across three slots, holding only two: one each in two.
        let slots = [
            SlotRef::Grid(0),
            SlotRef::Grid(1),
            SlotRef::Result,
            SlotRef::Grid(2),
        ];
        game.drag_across(&slots, Click::Right);
        assert_eq!(
            game.slot_stack(SlotRef::Grid(0)),
            Some(ItemStack::new(items::COBBLESTONE, 1))
        );
        assert_eq!(
            game.slot_stack(SlotRef::Grid(1)),
            Some(ItemStack::new(items::COBBLESTONE, 1))
        );
        assert_eq!(game.slot_stack(SlotRef::Grid(2)), None);
        assert_eq!(game.cursor, None);
    }

    #[test]
    fn double_clicking_gathers_part_stacks_first() {
        let mut game = flat_game(GameMode::Survival);
        game.open_inventory();
        game.cursor = Some(ItemStack::new(items::OAK_PLANKS, 10));
        game.inventory
            .set(0, Some(ItemStack::new(items::OAK_PLANKS, 64)));
        game.inventory
            .set(20, Some(ItemStack::new(items::OAK_PLANKS, 30)));
        game.inventory
            .set(21, Some(ItemStack::new(items::DIRT, 30)));
        game.container = Some(Container::Crafting {
            size: 2,
            grid: vec![Some(ItemStack::new(items::OAK_PLANKS, 5)), None, None, None],
        });
        game.collect_onto_cursor();
        assert_eq!(game.cursor, Some(ItemStack::new(items::OAK_PLANKS, 64)));
        assert_eq!(game.slot_stack(SlotRef::Grid(0)), None);
        assert_eq!(game.inventory.slot(20), None);
        assert_eq!(
            game.inventory.slot(0),
            Some(&ItemStack::new(items::OAK_PLANKS, 45)),
            "full stack last"
        );
        assert_eq!(
            game.inventory.slot(21),
            Some(&ItemStack::new(items::DIRT, 30))
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
