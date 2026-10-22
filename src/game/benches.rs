//! The enchanting table, the anvil and the grindstone: what their screens
//! offer, and what taking an offer or a result does.

use glam::{DVec3, IVec3};

use super::{Bench, BenchKind, Container, Game};
use crate::{
    inventory::{
        anvil::{self, AnvilResult, Refusal},
        enchant::{self, MAX_BOOKSHELF_POWER, Offer},
        grindstone, items,
        slots::{self, SlotRef},
        smithing,
        stack::ItemStack,
    },
    player::stats::DamageSource,
    world::{block::BlockState, blocks, props::Prop},
};

/// The chance an anvil takes damage when it does its work.
const ANVIL_DAMAGE_CHANCE: f64 = 0.12;

/// Where bookshelves count for a table: the ring two blocks out, a block
/// low and high, with nothing solid in the way (modern
/// `EnchantingTableBlock.BOOKSHELF_OFFSETS`).
fn bookshelf_offsets() -> impl Iterator<Item = IVec3> {
    (-2..=2).flat_map(|x| {
        (0..=1).flat_map(move |y| {
            (-2..=2).filter_map(move |z| {
                (i32::abs(x) == 2 || i32::abs(z) == 2).then_some(IVec3::new(x, y, z))
            })
        })
    })
}

impl Game {
    /// Opens the screen of the enchanting table, anvil or grindstone at
    /// `at`.
    pub(super) fn open_bench(&mut self, kind: BenchKind, at: IVec3) {
        if kind == BenchKind::Enchanting && self.stats.enchant_seed == 0 {
            self.stats.enchant_seed = (self.rng.next_u64() as u32).max(1);
        }
        self.container = Some(Container::Bench(Bench {
            kind,
            at,
            slots: vec![
                None;
                match kind {
                    BenchKind::Smithing => 3,
                    BenchKind::Stonecutter => 1,
                    _ => 2,
                }
            ],
            name: String::new(),
            choice: 0,
        }));
        self.release_input();
    }

    /// The open bench, if there is one.
    pub fn bench(&self) -> Option<&Bench> {
        match &self.container {
            Some(Container::Bench(bench)) => Some(bench),
            _ => None,
        }
    }

    /// Whether `slot` takes `stack`: the table's lapis slot wants lapis.
    pub(super) fn slot_accepts(&self, slot: SlotRef, stack: &ItemStack) -> bool {
        match (self.bench(), slot) {
            (Some(bench), SlotRef::Grid(1)) if bench.kind == BenchKind::Enchanting => {
                stack.id == items::LAPIS_LAZULI
            }
            _ => true,
        }
    }

    /// What the open stonecutter can cut its input into.
    pub fn stonecutter_cuts(&self) -> Vec<crate::inventory::crafting::Output> {
        let Some(bench) = self.bench().filter(|b| b.kind == BenchKind::Stonecutter) else {
            return Vec::new();
        };
        bench
            .slots
            .first()
            .and_then(Option::as_ref)
            .map_or_else(Vec::new, |input| self.items.recipes.stonecutting(input.id))
    }

    /// Chooses cut `index` on the open stonecutter, if it's there.
    pub fn choose_cut(&mut self, index: usize) {
        if index < self.stonecutter_cuts().len()
            && let Some(Container::Bench(bench)) = &mut self.container
        {
            bench.choice = index;
            crate::audio::play_here("ui.stonecutter.select_recipe");
        }
    }

    /// Whether levels and lapis cost nothing (creative).
    fn bench_is_free(&self) -> bool {
        self.mode.has_every_item()
    }

    /// Types in the anvil's name box.
    pub fn set_anvil_name(&mut self, name: String) {
        if let Some(Container::Bench(bench)) = &mut self.container
            && bench.kind == BenchKind::Anvil
        {
            bench.name = name;
        }
    }

    /// Bookshelves around the table at `at`, up to 15.
    pub fn bookshelf_power(&self, at: IVec3) -> u32 {
        let world = self.chunks.world();
        let blocks_def = self.items.blocks;
        let clear = |cell: IVec3| {
            world.block(cell.x, cell.y, cell.z).is_none_or(|block| {
                block.is(blocks::AIR) || blocks_def.get(block.id()).is_some_and(|d| d.replaceable)
            })
        };
        let shelves = bookshelf_offsets()
            .filter(|&offset| {
                let shelf = at + offset;
                world
                    .block(shelf.x, shelf.y, shelf.z)
                    .is_some_and(|block| block.is(blocks::BOOKSHELF))
                    && clear(at + IVec3::new(offset.x / 2, offset.y, offset.z / 2))
            })
            .count() as u32;
        shelves.min(MAX_BOOKSHELF_POWER)
    }

    /// The table's three offers for the item in its slot.
    pub fn enchant_offers(&self) -> [Option<Offer>; 3] {
        let Some(bench) = self.bench().filter(|b| b.kind == BenchKind::Enchanting) else {
            return [None, None, None];
        };
        let Some(stack) = bench.slots.first().and_then(Option::as_ref) else {
            return [None, None, None];
        };
        // Books come in stacks; one at a time is enchanted. Anything else
        // stacked can't be.
        if stack.is_enchanted()
            || stack.id == items::ENCHANTED_BOOK
            || (stack.count != 1 && stack.id != items::BOOK)
        {
            return [None, None, None];
        }
        let power = self.bookshelf_power(bench.at);
        enchant::offers(&self.items, stack.id, power, self.stats.enchant_seed)
    }

    /// Lapis in the table's lapis slot.
    pub fn lapis_count(&self) -> u32 {
        self.bench()
            .and_then(|bench| bench.slots.get(1)?.as_ref())
            .map_or(0, |stack| u32::from(stack.count))
    }

    /// Takes offer `button` (0–2): spends lapis and levels, enchants the
    /// item and picks a new seed. Returns whether it happened.
    pub fn enchant_item(&mut self, button: usize) -> bool {
        let price = enchant::offer_price(button);
        let free = self.bench_is_free();
        let Some(offer) = self.enchant_offers().get(button).cloned().flatten() else {
            return false;
        };
        let level = self.stats.experience.level;
        if !free && (self.lapis_count() < price || level < price || level < offer.requires) {
            return false;
        }
        let Some(Container::Bench(bench)) = &mut self.container else {
            return false;
        };
        let Some(mut item) = bench.slots[0].take() else {
            return false;
        };
        let mut spare = None;
        if item.id == items::BOOK {
            // One book of a stack becomes the enchanted book; the rest
            // stay in the slot.
            if item.count > 1 {
                spare = Some(item.with_count(item.count - 1));
            }
            item = ItemStack::new(items::ENCHANTED_BOOK, 1);
        }
        for &(enchantment, level) in &offer.enchants {
            item.enchant(enchantment, level);
        }
        match spare {
            Some(rest) => {
                bench.slots[0] = Some(rest);
                let items = self.items.clone();
                if let Some(left) = self.inventory.add(item, &items) {
                    self.throw(left);
                }
            }
            None => bench.slots[0] = Some(item),
        }
        let Some(Container::Bench(bench)) = &mut self.container else {
            return false;
        };
        if !free {
            if let Some(lapis) = bench.slots[1].as_mut() {
                lapis.count -= price as u8;
                if lapis.count == 0 {
                    bench.slots[1] = None;
                }
            }
            self.stats.experience.spend_levels(price);
        }
        self.stats.enchant_seed = (self.rng.next_u64() as u32).max(1);
        if let Some(at) = self.bench().map(|bench| bench.at) {
            crate::audio::play(
                "block.enchantment_table.use",
                at.as_dvec3() + DVec3::splat(0.5),
            );
        }
        self.note(crate::advancements::Event::Used("enchant".to_owned()));
        true
    }

    /// What the open anvil would make, or why not.
    pub fn anvil_outcome(&self) -> Option<Result<AnvilResult, Refusal>> {
        let bench = self.bench().filter(|b| b.kind == BenchKind::Anvil)?;
        let left = bench.slots.first()?.as_ref()?;
        let right = bench.slots.get(1).and_then(Option::as_ref);
        Some(anvil::combine(
            &self.items,
            left,
            right,
            &bench.name,
            self.bench_is_free(),
        ))
    }

    /// What the open anvil or grindstone makes from its slots.
    pub(super) fn bench_result(&self) -> Option<ItemStack> {
        let bench = self.bench()?;
        match bench.kind {
            BenchKind::Enchanting => None,
            BenchKind::Anvil => {
                // Shown even when the levels aren't there to take it.
                Some(self.anvil_outcome()?.ok()?.output)
            }
            BenchKind::Grindstone => {
                let top = bench.slots.first().and_then(Option::as_ref);
                let bottom = bench.slots.get(1).and_then(Option::as_ref);
                grindstone::grind(&self.items, top, bottom).map(|made| made.output)
            }
            BenchKind::Smithing => {
                let slot = |i: usize| bench.slots.get(i).and_then(Option::as_ref);
                smithing::upgrade(&self.items, slot(0), slot(1), slot(2))
            }
            BenchKind::Stonecutter => {
                let cuts = self.stonecutter_cuts();
                cuts.get(bench.choice)
                    .map(crate::inventory::crafting::Output::stack)
            }
        }
    }

    /// Takes the open anvil's or grindstone's result onto the mouse (or,
    /// with `shift`, into the inventory), using up what went into it.
    pub(super) fn take_bench_result(&mut self, shift: bool) {
        let Some(kind) = self.bench().map(|bench| bench.kind) else {
            return;
        };
        let items = self.items.clone();
        let free = self.bench_is_free();
        let (output, cost, right_used, xp) = match kind {
            BenchKind::Enchanting => return,
            BenchKind::Anvil => {
                let Some(Ok(made)) = self.anvil_outcome() else {
                    return;
                };
                if !free && self.stats.experience.level < made.cost {
                    return;
                }
                (made.output, made.cost, made.right_used, 0)
            }
            BenchKind::Grindstone => {
                let Some(bench) = self.bench() else {
                    return;
                };
                let top = bench.slots.first().and_then(Option::as_ref);
                let bottom = bench.slots.get(1).and_then(Option::as_ref);
                let Some(made) = grindstone::grind(&items, top, bottom) else {
                    return;
                };
                let xp = made.experience(&mut self.rng);
                (made.output, 0, u8::MAX, xp)
            }
            BenchKind::Smithing | BenchKind::Stonecutter => {
                let Some(output) = self.bench_result() else {
                    return;
                };
                (output, 0, 1, 0)
            }
        };
        if let Some(at) = self.bench().map(|bench| bench.at) {
            let sound = match kind {
                BenchKind::Anvil => "block.anvil.use",
                BenchKind::Grindstone => "block.grindstone.use",
                BenchKind::Smithing => "block.smithing_table.use",
                BenchKind::Stonecutter => "ui.stonecutter.take_result",
                BenchKind::Enchanting => "block.enchantment_table.use",
            };
            crate::audio::play(sound, at.as_dvec3() + DVec3::splat(0.5));
        }
        if shift {
            if self
                .inventory
                .merge(
                    output.clone(),
                    0..crate::inventory::player::MAIN_SLOTS,
                    &items,
                )
                .is_some()
            {
                return;
            }
        } else if !slots::take_onto_cursor(output, &mut self.cursor, &items) {
            return;
        }
        if let Some(Container::Bench(bench)) = &mut self.container
            && kind == BenchKind::Smithing
        {
            // One template and one ingot are used up with the base.
            for i in [0, 2] {
                if let Some(stack) = bench.slots[i].as_mut() {
                    stack.count -= 1;
                    if stack.count == 0 {
                        bench.slots[i] = None;
                    }
                }
            }
            bench.slots[1] = None;
        } else if let Some(Container::Bench(bench)) = &mut self.container
            && kind == BenchKind::Stonecutter
        {
            // One of the input is cut.
            if let Some(stack) = bench.slots[0].as_mut() {
                stack.count -= 1;
                if stack.count == 0 {
                    bench.slots[0] = None;
                }
            }
        } else if let Some(Container::Bench(bench)) = &mut self.container {
            bench.slots[0] = None;
            if let Some(right) = bench.slots[1].as_mut() {
                right.count = right.count.saturating_sub(right_used);
                if right.count == 0 {
                    bench.slots[1] = None;
                }
            }
            bench.name.clear();
        }
        if !free {
            self.stats.experience.spend_levels(cost);
        }
        if xp > 0 {
            let at = self.body.position() + DVec3::Y * 0.5;
            self.spawn_xp(xp, at);
        }
        if kind == BenchKind::Anvil && !free && self.rng.next_f64() < ANVIL_DAMAGE_CHANCE {
            self.wear_anvil();
        }
    }

    /// A falling anvil landing in `at` after dropping `fallen` blocks:
    /// everything in the cell takes two points for each block past one
    /// (up to 40), and the anvil may chip. Returns the anvil it settles as.
    pub(super) fn anvil_lands(&mut self, state: BlockState, at: IVec3, fallen: f64) -> BlockState {
        let distance = (fallen - 1.0).ceil();
        if distance <= 0.0 {
            return state;
        }
        crate::audio::play("block.anvil.land", at.as_dvec3() + DVec3::splat(0.5));
        let damage = (distance * 2.0).floor().min(40.0) as f32;
        let cell = crate::player::collision::Aabb::new(at.as_dvec3(), at.as_dvec3() + DVec3::ONE);
        if self.mode.is_vulnerable() && self.body.aabb().intersects(&cell) {
            self.stats.damage(damage, DamageSource::Anvil);
        }
        for mob in &mut self.mobs {
            if !mob.is_dead() && mob.body.aabb().intersects(&cell) {
                mob.hurt(damage, None, DamageSource::Anvil);
            }
        }
        if self.rng.next_f64() < 0.05 + distance * 0.05 {
            let facing = state.face(Prop::Facing);
            return match state.id() {
                blocks::ANVIL => {
                    BlockState::of(blocks::CHIPPED_ANVIL).with_face(Prop::Facing, facing)
                }
                blocks::CHIPPED_ANVIL => {
                    BlockState::of(blocks::DAMAGED_ANVIL).with_face(Prop::Facing, facing)
                }
                _ => state,
            };
        }
        state
    }

    /// Chips the anvil being used, and breaks it once it's damaged.
    pub(super) fn wear_anvil(&mut self) {
        let Some(at) = self.bench().map(|bench| bench.at) else {
            return;
        };
        let Some(block) = self.chunks.world().block(at.x, at.y, at.z) else {
            return;
        };
        let facing = block.face(Prop::Facing);
        let worn = match block.id() {
            blocks::ANVIL => BlockState::of(blocks::CHIPPED_ANVIL).with_face(Prop::Facing, facing),
            blocks::CHIPPED_ANVIL => {
                BlockState::of(blocks::DAMAGED_ANVIL).with_face(Prop::Facing, facing)
            }
            blocks::DAMAGED_ANVIL => BlockState::AIR,
            _ => return,
        };
        self.edit(&[(at, worn)]);
        if worn == BlockState::AIR {
            crate::audio::play("block.anvil.destroy", at.as_dvec3() + DVec3::splat(0.5));
            self.close_container();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        game::tests::flat_game,
        inventory::{
            enchant::Enchantment,
            slots::{Click, SlotRef},
        },
        player::{experience::points_to_reach, mode::GameMode},
    };

    const TABLE: IVec3 = IVec3::new(4, 64, 0);

    /// A table with `shelves` bookshelves around it, filled from the ring's
    /// low row first (each with air between it and the table).
    fn table_with(game: &mut Game, shelves: usize) {
        let mut edits = vec![(TABLE, BlockState::of(blocks::ENCHANTING_TABLE))];
        for offset in bookshelf_offsets().take(shelves) {
            edits.push((TABLE + offset, BlockState::of(blocks::BOOKSHELF)));
        }
        game.chunks.set_blocks(&edits);
    }

    /// Puts `stack` into the bench's slot `index` by clicking, with it on
    /// the mouse.
    fn put(game: &mut Game, index: usize, stack: ItemStack) {
        game.cursor = Some(stack);
        game.click_slot(SlotRef::Grid(index), Click::Left);
    }

    fn give_levels(game: &mut Game, levels: u32) {
        game.stats.experience.add(points_to_reach(levels));
    }

    #[test]
    fn bookshelves_count_when_the_gap_to_the_table_is_clear() {
        let mut game = flat_game(GameMode::Survival);
        table_with(&mut game, 15);
        assert_eq!(game.bookshelf_power(TABLE), 15);
        // A block in the gap cuts a shelf off.
        let gap = TABLE + IVec3::new(-1, 0, 0);
        game.chunks
            .set_blocks(&[(gap, BlockState::of(blocks::STONE))]);
        assert!(game.bookshelf_power(TABLE) < 15);
        // The ring has 32 spots (16 columns, 2 high) but the power stops
        // at 15.
        table_with(&mut game, 32);
        assert_eq!(game.bookshelf_power(TABLE), 15);
        assert_eq!(bookshelf_offsets().count(), 32);
    }

    #[test]
    fn a_table_enchants_a_diamond_pickaxe_with_efficiency() {
        let mut game = flat_game(GameMode::Survival);
        table_with(&mut game, 15);
        game.open_bench(BenchKind::Enchanting, TABLE);
        // Find a player seed whose top offer has Efficiency, as one would
        // by trying the table again after each enchanting.
        let pickaxe = ItemStack::new(items::DIAMOND_PICKAXE, 1);
        put(&mut game, 0, pickaxe);
        put(&mut game, 1, ItemStack::new(items::LAPIS_LAZULI, 3));
        assert_eq!(game.lapis_count(), 3);
        let seed = (1..5000u32)
            .find(|&seed| {
                game.stats.enchant_seed = seed;
                game.enchant_offers()[2].as_ref().is_some_and(|offer| {
                    offer
                        .enchants
                        .iter()
                        .any(|(e, _)| *e == Enchantment::Efficiency)
                })
            })
            .expect("some seed offers Efficiency");
        game.stats.enchant_seed = seed;
        let offer = game.enchant_offers()[2].clone().unwrap();
        assert_eq!(offer.requires, 30);

        // Too few levels: nothing happens.
        give_levels(&mut game, 29);
        assert!(!game.enchant_item(2));
        assert_eq!(game.lapis_count(), 3);
        // Enough levels: 3 lapis and 3 levels are spent.
        give_levels(&mut game, 33);
        let before = game.stats.experience.level;
        assert!(game.enchant_item(2));
        assert_eq!(game.stats.experience.level, before - 3);
        assert_eq!(game.lapis_count(), 0);
        let bench = game.bench().unwrap();
        let done = bench.slots[0].as_ref().unwrap();
        assert!(done.level_of(Enchantment::Efficiency) >= 1, "{done:?}");
        // The seed moved on, and an enchanted item takes no more.
        assert_ne!(game.stats.enchant_seed, seed);
        assert!(game.enchant_offers().iter().all(Option::is_none));
    }

    #[test]
    fn a_book_becomes_an_enchanted_book() {
        let mut game = flat_game(GameMode::Survival);
        table_with(&mut game, 15);
        game.open_bench(BenchKind::Enchanting, TABLE);
        if let Some(Container::Bench(bench)) = &mut game.container {
            bench.slots[0] = Some(ItemStack::new(items::BOOK, 1));
            bench.slots[1] = Some(ItemStack::new(items::LAPIS_LAZULI, 3));
        }
        give_levels(&mut game, 30);
        assert!(game.enchant_item(1));
        let book = game.bench().unwrap().slots[0].clone().unwrap();
        assert_eq!(book.id, items::ENCHANTED_BOOK);
        // The table drops one of the list for a book, so a book never holds
        // as many as the same offer on a tool could.
        assert!(!book.enchantments().is_empty(), "{book:?}");
    }

    #[test]
    fn creative_players_enchant_for_free() {
        let mut game = flat_game(GameMode::Creative);
        table_with(&mut game, 0);
        game.open_bench(BenchKind::Enchanting, TABLE);
        if let Some(Container::Bench(bench)) = &mut game.container {
            bench.slots[0] = Some(ItemStack::new(items::DIAMOND_SWORD, 1));
        }
        assert!(game.enchant_offers()[0].is_some());
        assert!(game.enchant_item(0));
        assert!(
            game.bench().unwrap().slots[0]
                .as_ref()
                .unwrap()
                .is_enchanted()
        );
    }

    #[test]
    fn the_lapis_slot_takes_only_lapis_and_closing_gives_things_back() {
        let mut game = flat_game(GameMode::Survival);
        game.open_bench(BenchKind::Enchanting, TABLE);
        game.cursor = Some(ItemStack::new(items::DIRT, 5));
        game.click_slot(SlotRef::Grid(1), Click::Left);
        assert_eq!(game.slot_stack(SlotRef::Grid(1)), None);
        game.click_slot(SlotRef::Grid(0), Click::Left);
        assert_eq!(
            game.slot_stack(SlotRef::Grid(0)),
            Some(ItemStack::new(items::DIRT, 5))
        );
        game.close_container();
        assert_eq!(
            game.inventory.slot(0),
            Some(&ItemStack::new(items::DIRT, 5)),
            "back in the pack"
        );
    }

    fn enchanted_book(enchantment: Enchantment, level: u8) -> ItemStack {
        let mut book = ItemStack::new(items::ENCHANTED_BOOK, 1);
        book.enchant(enchantment, level);
        book
    }

    #[test]
    fn two_enchanted_books_combine_on_an_anvil_for_levels() {
        let mut game = flat_game(GameMode::Survival);
        let anvil = IVec3::new(4, 64, 3);
        game.chunks
            .set_blocks(&[(anvil, BlockState::of(blocks::ANVIL))]);
        game.open_bench(BenchKind::Anvil, anvil);
        if let Some(Container::Bench(bench)) = &mut game.container {
            bench.slots[0] = Some(enchanted_book(Enchantment::Efficiency, 3));
            bench.slots[1] = Some(enchanted_book(Enchantment::Efficiency, 3));
        }
        let shown = game.slot_stack(SlotRef::Result).expect("a result shows");
        assert_eq!(shown.level_of(Enchantment::Efficiency), 4);
        // Without the 4 levels it can't be taken.
        give_levels(&mut game, 3);
        game.click_slot(SlotRef::Result, Click::Left);
        assert_eq!(game.cursor, None);
        give_levels(&mut game, 10);
        let before = game.stats.experience.level;
        game.click_slot(SlotRef::Result, Click::Left);
        assert_eq!(
            game.cursor
                .as_ref()
                .map(|c| c.level_of(Enchantment::Efficiency)),
            Some(4)
        );
        assert_eq!(game.stats.experience.level, before - 4);
        assert_eq!(game.slot_stack(SlotRef::Grid(0)), None);
        assert_eq!(game.slot_stack(SlotRef::Grid(1)), None);
    }

    #[test]
    fn an_anvil_renames_and_repairs_and_sometimes_chips() {
        let mut game = flat_game(GameMode::Survival);
        let anvil = IVec3::new(4, 64, 3);
        game.chunks
            .set_blocks(&[(anvil, BlockState::of(blocks::ANVIL))]);
        game.open_bench(BenchKind::Anvil, anvil);
        let worn = ItemStack::with_damage(items::DIAMOND_PICKAXE, 1, 1000);
        if let Some(Container::Bench(bench)) = &mut game.container {
            bench.slots[0] = Some(worn);
            bench.slots[1] = Some(ItemStack::new(items::DIAMOND, 2));
        }
        game.set_anvil_name("Digger".into());
        let shown = game.slot_stack(SlotRef::Result).unwrap();
        assert_eq!(shown.custom_name(), Some("Digger"));
        assert_eq!(shown.damage, 1000 - 2 * 390);
        give_levels(&mut game, 5);
        game.click_slot(SlotRef::Result, Click::Shift);
        assert!(
            game.inventory.find(items::DIAMOND_PICKAXE).is_some(),
            "shift-click sends it to the pack"
        );
        // Both diamonds went in; the bench is empty and the name cleared.
        assert!(game.bench().unwrap().slots.iter().all(Option::is_none));
        assert_eq!(game.bench().unwrap().name, "");
        // Used a lot, an anvil chips, then breaks.
        for _ in 0..200 {
            if game
                .chunks
                .world()
                .block(anvil.x, anvil.y, anvil.z)
                .unwrap()
                .is(blocks::AIR)
            {
                break;
            }
            game.wear_anvil();
        }
        let left = game
            .chunks
            .world()
            .block(anvil.x, anvil.y, anvil.z)
            .unwrap();
        assert!(left.is(blocks::AIR), "wore out");
    }

    #[test]
    fn a_grindstone_strips_enchantments_and_pays_experience() {
        let mut game = flat_game(GameMode::Survival);
        let stone = IVec3::new(4, 64, 3);
        game.chunks
            .set_blocks(&[(stone, BlockState::of(blocks::GRINDSTONE))]);
        game.open_bench(BenchKind::Grindstone, stone);
        let mut pickaxe = ItemStack::new(items::DIAMOND_PICKAXE, 1);
        pickaxe.enchant(Enchantment::Efficiency, 5);
        pickaxe.enchant(Enchantment::Mending, 1);
        if let Some(Container::Bench(bench)) = &mut game.container {
            bench.slots[0] = Some(pickaxe);
        }
        let shown = game.slot_stack(SlotRef::Result).unwrap();
        assert!(!shown.is_enchanted());
        game.click_slot(SlotRef::Result, Click::Left);
        assert_eq!(
            game.cursor.as_ref().map(ItemStack::is_enchanted),
            Some(false)
        );
        let paid: u32 = game.orbs.iter().map(|orb| orb.value).sum();
        // Efficiency V (41) and Mending (34) at least: 38 + up to 37 more.
        assert!((38..=75).contains(&paid), "{paid}");
        assert!(game.bench().unwrap().slots.iter().all(Option::is_none));
    }

    #[test]
    fn a_stonecutter_offers_cuts_of_its_input_and_makes_the_chosen_one_a_block_at_a_time() {
        let mut game = flat_game(GameMode::Survival);
        let cutter = IVec3::new(4, 64, 3);
        game.chunks
            .set_blocks(&[(cutter, BlockState::of(blocks::STONECUTTER))]);
        game.open_bench(BenchKind::Stonecutter, cutter);
        assert!(game.stonecutter_cuts().is_empty());
        assert!(game.slot_stack(SlotRef::Result).is_none());
        if let Some(Container::Bench(bench)) = &mut game.container {
            bench.slots[0] = Some(ItemStack::new(items::COBBLESTONE, 3));
        }
        let cuts: Vec<_> = game
            .stonecutter_cuts()
            .iter()
            .map(|o| (o.item, o.count))
            .collect();
        assert_eq!(
            cuts,
            [
                (items::COBBLESTONE_SLAB, 2),
                (items::COBBLESTONE_STAIRS, 1),
                (items::COBBLESTONE_WALL, 1)
            ]
        );
        // The first is chosen to begin with; choosing another changes the result.
        assert_eq!(
            game.slot_stack(SlotRef::Result),
            Some(ItemStack::new(items::COBBLESTONE_SLAB, 2))
        );
        game.choose_cut(2);
        game.choose_cut(9);
        assert_eq!(
            game.slot_stack(SlotRef::Result),
            Some(ItemStack::new(items::COBBLESTONE_WALL, 1))
        );
        game.click_slot(SlotRef::Result, Click::Left);
        assert_eq!(
            game.cursor,
            Some(ItemStack::new(items::COBBLESTONE_WALL, 1))
        );
        assert_eq!(game.bench().unwrap().slots[0].as_ref().unwrap().count, 2);
        // A second one stacks onto the cursor; the last one empties the slot.
        game.click_slot(SlotRef::Result, Click::Left);
        game.click_slot(SlotRef::Result, Click::Left);
        assert_eq!(game.cursor.as_ref().unwrap().count, 3);
        assert!(game.bench().unwrap().slots[0].is_none());
        assert!(game.slot_stack(SlotRef::Result).is_none());
    }

    #[test]
    fn a_smithing_table_upgrades_a_diamond_sword_to_netherite() {
        let mut game = flat_game(GameMode::Survival);
        let table = IVec3::new(4, 64, 3);
        game.chunks
            .set_blocks(&[(table, BlockState::of(blocks::SMITHING_TABLE))]);
        game.open_bench(BenchKind::Smithing, table);
        let mut sword = ItemStack::with_damage(items::DIAMOND_SWORD, 1, 40);
        sword.enchant(Enchantment::Sharpness, 3);
        if let Some(Container::Bench(bench)) = &mut game.container {
            bench.slots[0] = Some(ItemStack::new(
                items::NETHERITE_UPGRADE_SMITHING_TEMPLATE,
                2,
            ));
            bench.slots[1] = Some(sword);
            bench.slots[2] = Some(ItemStack::new(items::NETHERITE_INGOT, 1));
        }
        assert_eq!(
            game.slot_stack(SlotRef::Result).map(|s| s.id),
            Some(items::NETHERITE_SWORD)
        );
        game.click_slot(SlotRef::Result, Click::Left);
        let made = game.cursor.clone().unwrap();
        assert_eq!((made.id, made.damage), (items::NETHERITE_SWORD, 40));
        assert_eq!(made.level_of(Enchantment::Sharpness), 3);
        let slots = &game.bench().unwrap().slots;
        assert_eq!(
            slots[0].as_ref().map(|s| s.count),
            Some(1),
            "one template used"
        );
        assert!(slots[1].is_none() && slots[2].is_none());
    }

    #[test]
    fn a_falling_anvil_hurts_what_it_lands_on_and_its_damage_grows_with_the_drop() {
        use crate::{entity::mob::Mob, game::tests::mob_registry};
        let mut game = flat_game(GameMode::Survival);
        game.mob_defs = mob_registry();
        let kind = game.mob_defs.find("cow").unwrap();
        let cow = Mob::new(
            kind,
            game.mob_defs.get(kind),
            DVec3::new(6.5, 64.0, 6.5),
            0.0,
        );
        let health = cow.stats.health;
        game.mobs.push(cow);
        let high = IVec3::new(6, 68, 6);
        game.chunks
            .set_blocks(&[(high, BlockState::of(blocks::ANVIL))]);
        game.falls_due.push((game.time, high));
        for _ in 0..30 {
            game.tick();
        }
        let hurt = health - game.mobs[0].stats.health;
        // About 4 blocks: ceil(4 − 1) × 2 = 6 points at most from here
        // (the cow may have wandered or dodged the landing).
        assert!(hurt == 0.0 || (1.0..=8.0).contains(&hurt), "{hurt}");
        // The numbers behind it: a 20-block drop does 38, never past 40.
        let state = BlockState::of(blocks::ANVIL);
        let before = game.stats.health;
        game.body.set_position(DVec3::new(0.5, 64.0, 0.5));
        game.anvil_lands(state, IVec3::new(0, 64, 0), 20.0);
        assert_eq!(before - game.stats.health.max(0.0), 20.0f32.min(before));
        // A drop of one block or less does nothing.
        let mut other = flat_game(GameMode::Survival);
        let before = other.stats.health;
        other.anvil_lands(state, IVec3::new(0, 64, 0), 1.0);
        assert_eq!(other.stats.health, before);
    }

    #[test]
    fn a_stack_of_books_is_enchanted_one_book_at_a_time() {
        let mut game = flat_game(GameMode::Survival);
        table_with(&mut game, 15);
        game.open_bench(BenchKind::Enchanting, TABLE);
        if let Some(Container::Bench(bench)) = &mut game.container {
            bench.slots[0] = Some(ItemStack::new(items::BOOK, 5));
            bench.slots[1] = Some(ItemStack::new(items::LAPIS_LAZULI, 3));
        }
        give_levels(&mut game, 30);
        assert!(game.enchant_item(0));
        let bench = game.bench().unwrap();
        assert_eq!(bench.slots[0], Some(ItemStack::new(items::BOOK, 4)));
        let slot = game
            .inventory
            .find(items::ENCHANTED_BOOK)
            .expect("the book");
        assert!(game.inventory.slot(slot).unwrap().is_enchanted());
        // A stack of anything else isn't enchantable.
        if let Some(Container::Bench(bench)) = &mut game.container {
            bench.slots[0] = Some(ItemStack::new(items::STICK, 4));
        }
        assert!(game.enchant_offers().iter().all(Option::is_none));
    }
}
