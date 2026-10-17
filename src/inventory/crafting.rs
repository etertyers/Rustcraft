//! Recipes from `assets/data/recipes.ron` and matching a crafting grid
//! against them, following 1.0.0's `CraftingManager`, `ShapedRecipes` and
//! `ShapelessRecipes`; plus smelting recipes and furnace fuels.

use std::{collections::BTreeMap, path::Path};

use anyhow::{Context, Result, bail};
use serde::Deserialize;

use super::{
    item::{FIRST_ITEM_ID, ItemId},
    stack::ItemStack,
};

/// One cell of a recipe: an item, with a data value or (if left out) any
/// data for blocks and 0 for items.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ingredient {
    pub item: ItemId,
    #[serde(default)]
    pub data: Option<u16>,
}

impl Ingredient {
    fn matches(&self, stack: &ItemStack) -> bool {
        if stack.id != self.item {
            return false;
        }
        match self.data {
            Some(data) => stack.damage == data,
            None if self.item < FIRST_ITEM_ID => true,
            None => stack.damage == 0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Output {
    pub item: ItemId,
    pub count: u8,
    #[serde(default)]
    pub data: u16,
}

impl Output {
    pub fn stack(&self) -> ItemStack {
        ItemStack::with_damage(self.item, self.count, self.data)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ShapedDef {
    result: Output,
    pattern: Vec<String>,
    key: BTreeMap<char, Ingredient>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ShapelessDef {
    result: Output,
    ingredients: Vec<Ingredient>,
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Smelting {
    pub input: Ingredient,
    pub result: Output,
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct Fuel {
    item: ItemId,
    ticks: u32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RecipesFile {
    shaped: Vec<ShapedDef>,
    shapeless: Vec<ShapelessDef>,
    smelting: Vec<Smelting>,
    fuels: Vec<Fuel>,
}

/// A shaped recipe as a width × height grid of optional ingredients.
#[derive(Debug, Clone, PartialEq)]
struct Shaped {
    width: usize,
    height: usize,
    cells: Vec<Option<Ingredient>>,
    result: Output,
}

#[derive(Default)]
pub struct Recipes {
    shaped: Vec<Shaped>,
    shapeless: Vec<(Vec<Ingredient>, Output)>,
    smelting: Vec<Smelting>,
    fuels: Vec<Fuel>,
}

impl Recipes {
    pub fn load(path: &Path) -> Result<Self> {
        let text =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        Self::from_ron(&text).with_context(|| format!("parsing {}", path.display()))
    }

    pub fn from_ron(text: &str) -> Result<Self> {
        let file: RecipesFile = ron::from_str(text)?;
        let mut shaped = Vec::new();
        for def in file.shaped {
            let height = def.pattern.len();
            let width = def
                .pattern
                .iter()
                .map(|row| row.chars().count())
                .max()
                .unwrap_or(0);
            let mut cells = Vec::with_capacity(width * height);
            for row in &def.pattern {
                let mut chars: Vec<char> = row.chars().collect();
                chars.resize(width, ' ');
                for c in chars {
                    if c == ' ' {
                        cells.push(None);
                    } else {
                        let Some(&ingredient) = def.key.get(&c) else {
                            bail!("recipe for item {} uses unknown key {c:?}", def.result.item);
                        };
                        cells.push(Some(ingredient));
                    }
                }
            }
            shaped.push(Shaped {
                width,
                height,
                cells,
                result: def.result,
            });
        }
        Ok(Self {
            shaped,
            shapeless: file
                .shapeless
                .into_iter()
                .map(|def| (def.ingredients, def.result))
                .collect(),
            smelting: file.smelting,
            fuels: file.fuels,
        })
    }

    /// What a `size` × `size` grid (row by row) crafts, if anything.
    pub fn craft(&self, grid: &[Option<ItemStack>], size: usize) -> Option<ItemStack> {
        // The filled part of the grid.
        let filled = |x: usize, y: usize| grid[y * size + x].is_some();
        let columns: Vec<usize> = (0..size)
            .filter(|&x| (0..size).any(|y| filled(x, y)))
            .collect();
        let rows: Vec<usize> = (0..size)
            .filter(|&y| (0..size).any(|x| filled(x, y)))
            .collect();
        let (&left, &right) = (columns.first()?, columns.last()?);
        let (&top, &bottom) = (rows.first()?, rows.last()?);
        let (width, height) = (right - left + 1, bottom - top + 1);
        let cell = |x: usize, y: usize| grid[(top + y) * size + left + x].as_ref();
        for recipe in &self.shaped {
            if recipe.width != width || recipe.height != height {
                continue;
            }
            for mirrored in [false, true] {
                let fits = (0..height).all(|y| {
                    (0..width).all(|x| {
                        let rx = if mirrored { width - 1 - x } else { x };
                        match (&recipe.cells[y * width + rx], cell(x, y)) {
                            (None, None) => true,
                            (Some(ingredient), Some(stack)) => ingredient.matches(stack),
                            _ => false,
                        }
                    })
                });
                if fits {
                    return Some(recipe.result.stack());
                }
            }
        }
        let stacks: Vec<&ItemStack> = grid.iter().flatten().collect();
        self.shapeless.iter().find_map(|(ingredients, result)| {
            shapeless_matches(ingredients, &stacks).then(|| result.stack())
        })
    }

    /// What smelting `stack` makes, if it can be smelted.
    pub fn smelt(&self, stack: &ItemStack) -> Option<ItemStack> {
        self.smelting
            .iter()
            .find(|recipe| recipe.input.matches(stack))
            .map(|recipe| recipe.result.stack())
    }

    /// Ticks one `item` burns in a furnace (0 if it isn't a fuel).
    pub fn burn_ticks(&self, item: ItemId) -> u32 {
        self.fuels
            .iter()
            .find(|fuel| fuel.item == item)
            .map_or(0, |fuel| fuel.ticks)
    }
}

/// Each ingredient matched by a different stack, with none left over.
fn shapeless_matches(ingredients: &[Ingredient], stacks: &[&ItemStack]) -> bool {
    if ingredients.len() != stacks.len() {
        return false;
    }
    let mut used = vec![false; stacks.len()];
    ingredients.iter().all(|ingredient| {
        let found = (0..stacks.len()).find(|&i| !used[i] && ingredient.matches(stacks[i]));
        found.map(|i| used[i] = true).is_some()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn recipes() -> Recipes {
        Recipes::load(&crate::assets::dir().join("data/recipes.ron")).unwrap()
    }

    fn grid(size: usize, cells: &[(usize, usize, u16, u16)]) -> Vec<Option<ItemStack>> {
        let mut grid = vec![None; size * size];
        for &(x, y, id, data) in cells {
            grid[y * size + x] = Some(ItemStack::with_damage(id, 1, data));
        }
        grid
    }

    #[test]
    fn planks_from_any_log_anywhere_in_either_grid() {
        let recipes = recipes();
        let birch = grid(2, &[(1, 1, 17, 2)]);
        assert_eq!(recipes.craft(&birch, 2), Some(ItemStack::new(5, 4)));
        let corner = grid(3, &[(0, 2, 17, 0)]);
        assert_eq!(recipes.craft(&corner, 3), Some(ItemStack::new(5, 4)));
        assert_eq!(recipes.craft(&grid(2, &[]), 2), None);
    }

    #[test]
    fn shaped_recipes_need_their_shape_and_may_be_mirrored() {
        let recipes = recipes();
        // A wooden pickaxe: planks across the top, sticks down the middle.
        let pickaxe = grid(
            3,
            &[
                (0, 0, 5, 0),
                (1, 0, 5, 0),
                (2, 0, 5, 0),
                (1, 1, 280, 0),
                (1, 2, 280, 0),
            ],
        );
        assert_eq!(recipes.craft(&pickaxe, 3), Some(ItemStack::new(270, 1)));
        // The axe and its mirror image both work.
        let axe = grid(
            3,
            &[
                (0, 0, 4, 0),
                (1, 0, 4, 0),
                (0, 1, 4, 0),
                (1, 1, 280, 0),
                (1, 2, 280, 0),
            ],
        );
        let mirrored = grid(
            3,
            &[
                (1, 0, 4, 0),
                (2, 0, 4, 0),
                (2, 1, 4, 0),
                (1, 1, 280, 0),
                (1, 2, 280, 0),
            ],
        );
        assert_eq!(recipes.craft(&axe, 3), Some(ItemStack::new(275, 1)));
        assert_eq!(recipes.craft(&mirrored, 3), Some(ItemStack::new(275, 1)));
        // Sticks need to be stacked vertically, not side by side.
        let sideways = grid(2, &[(0, 0, 5, 0), (1, 0, 5, 0)]);
        assert_eq!(recipes.craft(&sideways, 2), None);
        // A 3×3 recipe doesn't fit the 2×2 grid.
        let table = grid(2, &[(0, 0, 5, 0), (1, 0, 5, 0), (0, 1, 5, 0), (1, 1, 5, 0)]);
        assert_eq!(recipes.craft(&table, 2), Some(ItemStack::new(58, 1)));
    }

    #[test]
    fn item_data_values_matter_but_block_variants_dont() {
        let recipes = recipes();
        // Charcoal torches; a dye in place of coal makes nothing.
        let charcoal = grid(2, &[(0, 0, 263, 1), (0, 1, 280, 0)]);
        assert_eq!(recipes.craft(&charcoal, 2), Some(ItemStack::new(50, 4)));
        let dye = grid(2, &[(0, 0, 351, 1), (0, 1, 280, 0)]);
        assert_eq!(recipes.craft(&dye, 2), None);
        // A bed from mixed wool colors.
        let bed = grid(
            3,
            &[
                (0, 0, 35, 14),
                (1, 0, 35, 0),
                (2, 0, 35, 4),
                (0, 1, 5, 0),
                (1, 1, 5, 0),
                (2, 1, 5, 0),
            ],
        );
        assert_eq!(recipes.craft(&bed, 3), Some(ItemStack::new(355, 1)));
    }

    #[test]
    fn planks_make_wooden_slabs_not_stone_ones() {
        let recipes = recipes();
        let row = grid(3, &[(0, 1, 5, 0), (1, 1, 5, 0), (2, 1, 5, 0)]);
        assert_eq!(recipes.craft(&row, 3), Some(ItemStack::new(126, 3)));
        assert_eq!(recipes.burn_ticks(126), 150);
    }

    #[test]
    fn shapeless_recipes_take_any_arrangement() {
        let recipes = recipes();
        let stew = grid(3, &[(2, 2, 281, 0), (0, 1, 40, 0), (1, 0, 39, 0)]);
        assert_eq!(recipes.craft(&stew, 3), Some(ItemStack::new(282, 1)));
        // Red dye on white wool makes red wool (data 14).
        let red = grid(2, &[(1, 1, 351, 1), (0, 0, 35, 0)]);
        assert_eq!(
            recipes.craft(&red, 2),
            Some(ItemStack::with_damage(35, 1, 14))
        );
        // An extra item spoils it.
        let spoiled = grid(
            3,
            &[(2, 2, 281, 0), (0, 1, 40, 0), (1, 0, 39, 0), (0, 0, 3, 0)],
        );
        assert_eq!(recipes.craft(&spoiled, 3), None);
    }

    #[test]
    fn smelting_and_fuels_follow_1_0_0() {
        let recipes = recipes();
        assert_eq!(
            recipes.smelt(&ItemStack::new(15, 1)),
            Some(ItemStack::new(265, 1))
        );
        assert_eq!(
            recipes.smelt(&ItemStack::with_damage(17, 1, 2)),
            Some(ItemStack::with_damage(263, 1, 1))
        );
        assert_eq!(recipes.smelt(&ItemStack::new(3, 1)), None);
        assert_eq!(recipes.burn_ticks(263), 1600);
        assert_eq!(recipes.burn_ticks(5), 300);
        assert_eq!(recipes.burn_ticks(280), 100);
        assert_eq!(recipes.burn_ticks(1), 0);
    }

    #[test]
    fn every_recipe_names_real_items() {
        let assets = crate::assets::dir();
        let items = crate::inventory::stack::Items::new(
            crate::world::block::BlockRegistry::load(&assets.join("data/blocks.ron"))
                .unwrap()
                .into(),
            crate::inventory::item::ItemRegistry::load(&assets.join("data/items.ron"))
                .unwrap()
                .into(),
        );
        let recipes = recipes();
        let mut ids: Vec<ItemId> = Vec::new();
        for recipe in &recipes.shaped {
            ids.push(recipe.result.item);
            ids.extend(recipe.cells.iter().flatten().map(|i| i.item));
        }
        for (ingredients, result) in &recipes.shapeless {
            ids.push(result.item);
            ids.extend(ingredients.iter().map(|i| i.item));
        }
        for id in ids {
            assert!(items.exists(id), "recipe uses unknown item {id}");
        }
        assert!(recipes.shaped.len() > 90);
    }
}
