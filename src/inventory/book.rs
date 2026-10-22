//! The recipe book: every crafting recipe as the book lists it (no
//! unlocking), which of them the player's items make, which fit the
//! inventory's 2 × 2 grid, and moving the ingredients of one into a grid.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::{
    crafting::{Ingredient, Output, Recipes},
    item::{self, ItemId},
    player::{Inventory, MAIN_SLOTS},
    stack::{ItemStack, Items},
};

/// A tab of the book (the book's own grouping, as 1.21's crafting
/// categories).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Category {
    #[default]
    All,
    ToolsCombat,
    Building,
    /// Food and everything else.
    Misc,
    Redstone,
}

impl Category {
    pub const ALL: [Category; 5] = [
        Category::All,
        Category::ToolsCombat,
        Category::Building,
        Category::Misc,
        Category::Redstone,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Category::All => "All",
            Category::ToolsCombat => "Tools & Combat",
            Category::Building => "Building Blocks",
            Category::Misc => "Misc",
            Category::Redstone => "Redstone",
        }
    }
}

/// What the book remembers about a world: whether it is open and how it
/// filters (saved in the level).
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settings {
    pub open: bool,
    /// Show only what the player can craft now.
    pub craftable_only: bool,
    pub category: Category,
}

/// One recipe as the book shows it.
#[derive(Debug, Clone)]
pub struct BookRecipe {
    pub result: Output,
    pub width: usize,
    pub height: usize,
    /// Shaped recipes keep their layout; shapeless ones fill the grid in
    /// order.
    pub shaped: bool,
    /// `width` × `height` cells, row by row (every cell filled when
    /// shapeless).
    pub cells: Vec<Option<Ingredient>>,
    /// Never `Category::All`.
    pub category: Category,
}

/// How many of each item the player has.
type Counts = BTreeMap<ItemId, u32>;

fn counts(inventory: &Inventory, grid: &[Option<ItemStack>]) -> Counts {
    let mut have = Counts::new();
    let held = (0..MAIN_SLOTS)
        .filter_map(|i| inventory.slot(i))
        .chain(grid.iter().flatten());
    for stack in held {
        *have.entry(stack.id).or_default() += stack.count as u32;
    }
    have
}

impl BookRecipe {
    /// Whether the recipe fits a `size` × `size` grid.
    pub fn fits(&self, size: usize) -> bool {
        if self.shaped {
            self.width <= size && self.height <= size
        } else {
            self.cells.len() <= size * size
        }
    }

    /// Each ingredient with the grid cell (row by row in a `size` × `size`
    /// grid) it goes in: shaped recipes from the top-left corner,
    /// shapeless ones in order.
    pub fn placements(&self, size: usize) -> Vec<(usize, &Ingredient)> {
        let mut out = Vec::new();
        for (i, cell) in self.cells.iter().enumerate() {
            let Some(ingredient) = cell else { continue };
            let at = if self.shaped {
                (i / self.width) * size + i % self.width
            } else {
                i
            };
            out.push((at, ingredient));
        }
        out
    }

    /// The item each ingredient would use from `have`, taking the one the
    /// player has most of, or `None` if something is missing.
    fn choose(&self, have: &Counts) -> Option<Vec<ItemId>> {
        let mut left = have.clone();
        let needed: Vec<&Ingredient> = self.cells.iter().flatten().collect();
        // Ingredients with the fewest choices first, so a broad one doesn't
        // take what a narrow one needs.
        let mut order: Vec<usize> = (0..needed.len()).collect();
        order.sort_by_key(|&i| needed[i].items().len());
        let mut chosen = vec![ItemId::default(); needed.len()];
        for i in order {
            let best = needed[i]
                .items()
                .iter()
                .copied()
                .filter(|id| left.get(id).copied().unwrap_or(0) > 0)
                .max_by_key(|id| left[id])?;
            *left.get_mut(&best)? -= 1;
            chosen[i] = best;
        }
        Some(chosen)
    }

    /// Whether `inventory` (and what is already in `grid`) has the
    /// ingredients for one.
    pub fn craftable(&self, inventory: &Inventory, grid: &[Option<ItemStack>]) -> bool {
        self.choose(&counts(inventory, grid)).is_some()
    }
}

/// Every crafting recipe, in the order of the recipe file.
pub fn all(recipes: &Recipes, items: &Items) -> Vec<BookRecipe> {
    recipes
        .crafting()
        .map(|(result, width, height, shaped, cells)| BookRecipe {
            category: category_of(items, result.item),
            result,
            width,
            height,
            shaped,
            cells,
        })
        .collect()
}

/// The tab a recipe's result belongs in.
fn category_of(items: &Items, result: ItemId) -> Category {
    let name = result.name();
    let has = |words: &[&str]| words.iter().any(|w| name.contains(w));
    let ends = |words: &[&str]| words.iter().any(|w| name.ends_with(w));
    if ends(&[
        "_door",
        "_trapdoor",
        "_fence_gate",
        "_button",
        "_pressure_plate",
        "_rail",
    ]) || has(&[
        "redstone",
        "repeater",
        "comparator",
        "piston",
        "observer",
        "dispenser",
        "dropper",
        "hopper",
        "lever",
        "tnt",
        "note_block",
        "daylight_detector",
        "tripwire",
        "target",
        "lightning_rod",
        "jukebox",
        "lectern",
        "crafter",
        "bell",
        "copper_bulb",
    ]) && !has(&["redstone_ore"])
    {
        return Category::Redstone;
    }
    if ends(&[
        "_sword",
        "_pickaxe",
        "_axe",
        "_shovel",
        "_hoe",
        "_helmet",
        "_chestplate",
        "_leggings",
        "_boots",
        "_spear",
    ]) || matches!(
        name,
        "bow"
            | "crossbow"
            | "arrow"
            | "shield"
            | "fishing_rod"
            | "flint_and_steel"
            | "shears"
            | "compass"
            | "clock"
            | "spyglass"
            | "brush"
            | "trident"
            | "mace"
            | "carrot_on_a_stick"
            | "warped_fungus_on_a_stick"
            | "spectral_arrow"
            | "bucket"
    ) {
        return Category::ToolsCombat;
    }
    let is_block = item::registry()
        .get(result)
        .is_some_and(|def| def.texture.is_none())
        && items.item(result).is_some();
    if is_block
        && !has(&[
            "_bed", "chest", "furnace", "barrel", "sign", "banner", "candle", "shulker",
        ])
    {
        Category::Building
    } else {
        Category::Misc
    }
}

/// Moves the ingredients of `recipe` from the inventory into `grid` (a
/// `size` × `size` crafting grid): what was in the grid goes back to the
/// inventory first, then each cell gets one of its ingredient, or with
/// `max` as many sets as the items and stack sizes allow. Changes nothing
/// and returns false if the recipe doesn't fit, can't be made or the
/// grid's old contents don't fit back.
pub fn place(
    recipe: &BookRecipe,
    size: usize,
    inventory: &mut Inventory,
    grid: &mut [Option<ItemStack>],
    items: &Items,
    max: bool,
) -> bool {
    if !recipe.fits(size) {
        return false;
    }
    let (mut inv, mut cells) = (inventory.clone(), grid.to_vec());
    for cell in cells.iter_mut() {
        if let Some(stack) = cell.take()
            && inv.add(stack, items).is_some()
        {
            return false;
        }
    }
    let Some(chosen) = recipe.choose(&counts(&inv, &cells)) else {
        return false;
    };
    // Items used per set, and how many sets the player can fill.
    let mut per_set: BTreeMap<ItemId, u32> = BTreeMap::new();
    for &id in &chosen {
        *per_set.entry(id).or_default() += 1;
    }
    let have = counts(&inv, &cells);
    let mut sets = if max { u32::MAX } else { 1 };
    for (&id, &uses) in &per_set {
        sets = sets.min(have[&id] / uses).min(items.max_stack(id) as u32);
    }
    if sets == 0 {
        return false;
    }
    let placements = recipe.placements(size);
    for (&(at, _), &id) in placements.iter().zip(&chosen) {
        let mut wanted = sets;
        let mut taken = 0u32;
        for slot in 0..MAIN_SLOTS {
            if wanted == 0 {
                break;
            }
            if inv.slot(slot).is_some_and(|s| s.id == id)
                && let Some(part) = inv.take(slot, wanted.min(255) as u8)
            {
                wanted -= part.count as u32;
                taken += part.count as u32;
            }
        }
        cells[at] = Some(ItemStack::new(id, taken as u8));
    }
    *inventory = inv;
    grid.clone_from_slice(&cells);
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inventory::items;

    fn recipes() -> (Items, Vec<BookRecipe>) {
        let recipes = Recipes::load(&crate::assets::dir().join("data/recipes.ron")).unwrap();
        let items = Items::new().with_recipes(std::sync::Arc::new(recipes));
        let book = all(&items.recipes, &items);
        (items, book)
    }

    fn making(book: &[BookRecipe], item: ItemId) -> &BookRecipe {
        book.iter()
            .find(|r| r.result.item == item)
            .unwrap_or_else(|| panic!("no recipe for {}", item.name()))
    }

    #[test]
    fn the_book_lists_every_recipe_in_a_tab_and_the_inventory_only_those_that_fit() {
        let (_, book) = recipes();
        assert!(book.len() > 300, "{} recipes", book.len());
        assert!(book.iter().all(|r| r.category != Category::All));
        for category in &Category::ALL[1..] {
            assert!(book.iter().any(|r| r.category == *category), "{category:?}");
        }
        let small: Vec<&str> = book
            .iter()
            .filter(|r| r.fits(2))
            .map(|r| r.result.item.name())
            .collect();
        for fits in [
            "oak_planks",
            "stick",
            "crafting_table",
            "torch",
            "white_dye",
        ] {
            assert!(small.contains(&fits), "{fits} fits 2x2");
        }
        for needs_table in ["wooden_pickaxe", "chest", "furnace", "bow", "piston"] {
            assert!(
                !small.contains(&needs_table),
                "{needs_table} needs the table"
            );
        }
        // A shapeless recipe with five ingredients doesn't fit either.
        assert!(book.iter().filter(|r| !r.shaped).any(|r| !r.fits(2)));
    }

    #[test]
    fn recipes_sort_into_the_books_tabs() {
        let (items, book) = recipes();
        let category = |id| making(&book, id).category;
        assert_eq!(category(items::IRON_PICKAXE), Category::ToolsCombat);
        assert_eq!(category(items::DIAMOND_SWORD), Category::ToolsCombat);
        assert_eq!(category(items::OAK_PLANKS), Category::Building);
        assert_eq!(category(items::BREAD), Category::Misc);
        assert_eq!(category(items::STICK), Category::Misc);
        assert_eq!(category(items::PISTON), Category::Redstone);
        assert_eq!(category(items::OAK_DOOR), Category::Redstone);
        let _ = items;
    }

    fn inventory_with(items: &Items, stacks: &[ItemStack]) -> Inventory {
        let mut inventory = Inventory::default();
        for stack in stacks {
            assert!(inventory.add(stack.clone(), items).is_none());
        }
        inventory
    }

    #[test]
    fn a_recipe_is_craftable_when_the_inventory_holds_its_ingredients() {
        let (items, book) = recipes();
        let sticks = making(&book, items::STICK);
        let none = vec![None; 4];
        assert!(!sticks.craftable(&Inventory::default(), &none));
        let one = inventory_with(&items, &[ItemStack::new(items::OAK_PLANKS, 1)]);
        assert!(!sticks.craftable(&one, &none), "needs two planks");
        let two = inventory_with(&items, &[ItemStack::new(items::OAK_PLANKS, 2)]);
        assert!(sticks.craftable(&two, &none));
        // Any wood's planks do: one oak and one birch.
        let mixed = inventory_with(
            &items,
            &[
                ItemStack::new(items::OAK_PLANKS, 1),
                ItemStack::new(items::BIRCH_PLANKS, 1),
            ],
        );
        assert!(sticks.craftable(&mixed, &none));
        // What is already in the grid counts too.
        let mut grid = vec![None; 4];
        grid[0] = Some(ItemStack::new(items::OAK_PLANKS, 2));
        assert!(sticks.craftable(&Inventory::default(), &grid));
    }

    #[test]
    fn placing_a_recipe_moves_its_ingredients_into_the_grid() {
        let (items, book) = recipes();
        let sticks = making(&book, items::STICK);
        let mut inventory = inventory_with(&items, &[ItemStack::new(items::OAK_PLANKS, 5)]);
        let mut grid = vec![None; 4];
        assert!(place(sticks, 2, &mut inventory, &mut grid, &items, false));
        assert_eq!(grid[0], Some(ItemStack::new(items::OAK_PLANKS, 1)));
        assert_eq!(grid[2], Some(ItemStack::new(items::OAK_PLANKS, 1)));
        assert!(grid[1].is_none() && grid[3].is_none());
        assert_eq!(
            inventory.slot(0),
            Some(&ItemStack::new(items::OAK_PLANKS, 3))
        );
        assert_eq!(
            items.recipes.craft(&grid, 2),
            Some(ItemStack::new(items::STICK, 4))
        );
        // Placing again first puts the grid's planks back, then takes two.
        assert!(place(sticks, 2, &mut inventory, &mut grid, &items, false));
        assert_eq!(
            inventory.slot(0),
            Some(&ItemStack::new(items::OAK_PLANKS, 3))
        );
    }

    #[test]
    fn shift_fills_as_many_sets_as_possible() {
        let (items, book) = recipes();
        let sticks = making(&book, items::STICK);
        // 64 planks: two cells hold 32 each (sets of two planks).
        let mut inventory = inventory_with(&items, &[ItemStack::new(items::OAK_PLANKS, 64)]);
        let mut grid = vec![None; 4];
        assert!(place(sticks, 2, &mut inventory, &mut grid, &items, true));
        assert_eq!(grid[0], Some(ItemStack::new(items::OAK_PLANKS, 32)));
        assert_eq!(grid[2], Some(ItemStack::new(items::OAK_PLANKS, 32)));
        assert_eq!(inventory.slot(0), None);
        // Limited by what there is, and by one stack per cell.
        let mut inventory = inventory_with(
            &items,
            &[
                ItemStack::new(items::OAK_PLANKS, 64),
                ItemStack::new(items::OAK_PLANKS, 64),
                ItemStack::new(items::OAK_PLANKS, 64),
            ],
        );
        let mut grid = vec![None; 4];
        assert!(place(sticks, 2, &mut inventory, &mut grid, &items, true));
        assert_eq!(grid[0].as_ref().unwrap().count, 64);
        assert_eq!(grid[2].as_ref().unwrap().count, 64);
        assert_eq!(
            inventory.slot(2),
            Some(&ItemStack::new(items::OAK_PLANKS, 64))
        );
    }

    #[test]
    fn a_recipe_you_cannot_make_or_that_does_not_fit_changes_nothing() {
        let (items, book) = recipes();
        let pickaxe = making(&book, items::WOODEN_PICKAXE);
        let mut inventory = inventory_with(
            &items,
            &[
                ItemStack::new(items::OAK_PLANKS, 3),
                ItemStack::new(items::STICK, 1),
            ],
        );
        let (before, mut grid) = (inventory.clone(), vec![None; 9]);
        // One stick is one short.
        assert!(!place(pickaxe, 3, &mut inventory, &mut grid, &items, false));
        assert_eq!(inventory, before);
        assert!(grid.iter().all(Option::is_none));
        // The 3 × 3 recipe doesn't fit the 2 × 2 grid even with the items.
        inventory.add(ItemStack::new(items::STICK, 4), &items);
        let before = inventory.clone();
        assert!(!place(
            pickaxe,
            2,
            &mut inventory,
            &mut vec![None; 4],
            &items,
            false
        ));
        assert_eq!(inventory, before);
        // And with them, the 3 × 3 grid takes it, shaped as the recipe is.
        assert!(place(pickaxe, 3, &mut inventory, &mut grid, &items, false));
        assert_eq!(
            items.recipes.craft(&grid, 3),
            Some(ItemStack::new(items::WOODEN_PICKAXE, 1))
        );
    }
}
