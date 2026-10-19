//! Recipes from `assets/data/recipes.ron` and matching a crafting grid
//! against them, following 1.0.0's `CraftingManager`, `ShapedRecipes` and
//! `ShapelessRecipes`; plus smelting recipes and furnace fuels.

use std::{collections::BTreeMap, path::Path};

use anyhow::{Context, Result, bail};
use serde::Deserialize;

use super::{item::ItemId, stack::ItemStack};

/// One cell of a recipe as written: an item, or a tag naming a group of
/// items (any log, any wool, as 1.0.0 matched any data value).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct IngredientDef {
    #[serde(default)]
    item: Option<ItemId>,
    #[serde(default)]
    tag: Option<String>,
}

/// One cell of a recipe: the items that fit it.
#[derive(Debug, Clone, PartialEq)]
pub struct Ingredient(Vec<ItemId>);

impl Ingredient {
    fn resolve(def: &IngredientDef, tags: &BTreeMap<String, Vec<ItemId>>) -> Result<Self> {
        match (def.item, &def.tag) {
            (Some(item), None) => Ok(Self(vec![item])),
            (None, Some(tag)) => match tags.get(tag) {
                Some(items) => Ok(Self(items.clone())),
                None => bail!("unknown tag {tag:?}"),
            },
            _ => bail!("an ingredient needs an item or a tag: {def:?}"),
        }
    }

    fn matches(&self, stack: &ItemStack) -> bool {
        self.0.contains(&stack.id)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Output {
    pub item: ItemId,
    pub count: u8,
}

impl Output {
    pub fn stack(&self) -> ItemStack {
        ItemStack::with_damage(self.item, self.count, 0)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ShapedDef {
    result: Output,
    pattern: Vec<String>,
    key: BTreeMap<char, IngredientDef>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ShapelessDef {
    result: Output,
    ingredients: Vec<IngredientDef>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SmeltingDef {
    input: IngredientDef,
    result: Output,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Smelting {
    pub input: Ingredient,
    pub result: Output,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FuelDef {
    #[serde(default)]
    item: Option<ItemId>,
    #[serde(default)]
    tag: Option<String>,
    ticks: u32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RecipesFile {
    /// Groups of items recipes can name (modern Minecraft's item tags).
    tags: BTreeMap<String, Vec<ItemId>>,
    shaped: Vec<ShapedDef>,
    shapeless: Vec<ShapelessDef>,
    smelting: Vec<SmeltingDef>,
    fuels: Vec<FuelDef>,
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
    fuels: Vec<(Ingredient, u32)>,
}

impl Recipes {
    pub fn load(path: &Path) -> Result<Self> {
        let text =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        Self::from_ron(&text).with_context(|| format!("parsing {}", path.display()))
    }

    pub fn from_ron(text: &str) -> Result<Self> {
        let file: RecipesFile = ron::from_str(text)?;
        let tags = &file.tags;
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
                        let Some(ingredient) = def.key.get(&c) else {
                            bail!(
                                "recipe for {} uses unknown key {c:?}",
                                def.result.item.name()
                            );
                        };
                        cells.push(Some(Ingredient::resolve(ingredient, tags)?));
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
        let shapeless = file
            .shapeless
            .iter()
            .map(|def| {
                let ingredients = def
                    .ingredients
                    .iter()
                    .map(|i| Ingredient::resolve(i, tags))
                    .collect::<Result<_>>()?;
                Ok((ingredients, def.result))
            })
            .collect::<Result<_>>()?;
        let smelting = file
            .smelting
            .iter()
            .map(|def| {
                Ok(Smelting {
                    input: Ingredient::resolve(&def.input, tags)?,
                    result: def.result,
                })
            })
            .collect::<Result<_>>()?;
        let fuels = file
            .fuels
            .iter()
            .map(|def| {
                let fuel = IngredientDef {
                    item: def.item,
                    tag: def.tag.clone(),
                };
                Ok((Ingredient::resolve(&fuel, tags)?, def.ticks))
            })
            .collect::<Result<_>>()?;
        Ok(Self {
            shaped,
            shapeless,
            smelting,
            fuels,
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
            .find(|(fuel, _)| fuel.0.contains(&item))
            .map_or(0, |&(_, ticks)| ticks)
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
    use crate::inventory::items;

    fn recipes() -> Recipes {
        Recipes::load(&crate::assets::dir().join("data/recipes.ron")).unwrap()
    }

    fn grid(size: usize, cells: &[(usize, usize, ItemId)]) -> Vec<Option<ItemStack>> {
        let mut grid = vec![None; size * size];
        for &(x, y, id) in cells {
            grid[y * size + x] = Some(ItemStack::new(id, 1));
        }
        grid
    }

    #[test]
    fn planks_from_any_log_anywhere_in_either_grid() {
        let recipes = recipes();
        // Each log makes its own planks (modern).
        let birch = grid(2, &[(1, 1, items::BIRCH_LOG)]);
        assert_eq!(
            recipes.craft(&birch, 2),
            Some(ItemStack::new(items::BIRCH_PLANKS, 4))
        );
        let stripped = grid(2, &[(0, 0, items::STRIPPED_MANGROVE_WOOD)]);
        assert_eq!(
            recipes.craft(&stripped, 2),
            Some(ItemStack::new(items::MANGROVE_PLANKS, 4))
        );
        let corner = grid(3, &[(0, 2, items::OAK_LOG)]);
        assert_eq!(
            recipes.craft(&corner, 3),
            Some(ItemStack::new(items::OAK_PLANKS, 4))
        );
        assert_eq!(recipes.craft(&grid(2, &[]), 2), None);
    }

    #[test]
    fn shaped_recipes_need_their_shape_and_may_be_mirrored() {
        let recipes = recipes();
        let (planks, stick, cobble) = (items::OAK_PLANKS, items::STICK, items::COBBLESTONE);
        // A wooden pickaxe: planks across the top, sticks down the middle.
        let pickaxe = grid(
            3,
            &[
                (0, 0, planks),
                (1, 0, planks),
                (2, 0, planks),
                (1, 1, stick),
                (1, 2, stick),
            ],
        );
        assert_eq!(
            recipes.craft(&pickaxe, 3),
            Some(ItemStack::new(items::WOODEN_PICKAXE, 1))
        );
        // The axe and its mirror image both work.
        let axe = grid(
            3,
            &[
                (0, 0, cobble),
                (1, 0, cobble),
                (0, 1, cobble),
                (1, 1, stick),
                (1, 2, stick),
            ],
        );
        let mirrored = grid(
            3,
            &[
                (1, 0, cobble),
                (2, 0, cobble),
                (2, 1, cobble),
                (1, 1, stick),
                (1, 2, stick),
            ],
        );
        assert_eq!(
            recipes.craft(&axe, 3),
            Some(ItemStack::new(items::STONE_AXE, 1))
        );
        assert_eq!(
            recipes.craft(&mirrored, 3),
            Some(ItemStack::new(items::STONE_AXE, 1))
        );
        // Sticks need to be stacked vertically, not side by side.
        let sideways = grid(2, &[(0, 0, planks), (1, 0, planks)]);
        assert_eq!(
            recipes.craft(&sideways, 2),
            Some(ItemStack::new(items::OAK_PRESSURE_PLATE, 1)),
            "side by side they are a pressure plate"
        );
        let table = grid(
            2,
            &[
                (0, 0, planks),
                (1, 0, planks),
                (0, 1, planks),
                (1, 1, planks),
            ],
        );
        assert_eq!(
            recipes.craft(&table, 2),
            Some(ItemStack::new(items::CRAFTING_TABLE, 1))
        );
    }

    #[test]
    fn charcoal_works_like_coal_and_any_wool_makes_a_bed() {
        let recipes = recipes();
        let charcoal = grid(2, &[(0, 0, items::CHARCOAL), (0, 1, items::STICK)]);
        assert_eq!(
            recipes.craft(&charcoal, 2),
            Some(ItemStack::new(items::TORCH, 4))
        );
        let dye = grid(2, &[(0, 0, items::RED_DYE), (0, 1, items::STICK)]);
        assert_eq!(recipes.craft(&dye, 2), None);
        // A bed from mixed wool colors.
        let planks = items::OAK_PLANKS;
        let bed = grid(
            3,
            &[
                (0, 0, items::RED_WOOL),
                (1, 0, items::WHITE_WOOL),
                (2, 0, items::YELLOW_WOOL),
                (0, 1, planks),
                (1, 1, planks),
                (2, 1, planks),
            ],
        );
        assert_eq!(
            recipes.craft(&bed, 3),
            Some(ItemStack::new(items::RED_BED, 1))
        );
    }

    #[test]
    fn a_shield_takes_six_planks_and_an_iron_ingot() {
        let recipes = recipes();
        let planks = items::OAK_PLANKS;
        let shield = grid(
            3,
            &[
                (0, 0, planks),
                (1, 0, items::IRON_INGOT),
                (2, 0, planks),
                (0, 1, planks),
                (1, 1, planks),
                (2, 1, planks),
                (1, 2, planks),
            ],
        );
        assert_eq!(
            recipes.craft(&shield, 3),
            Some(ItemStack::new(items::SHIELD, 1))
        );
        // The iron in the wrong place makes nothing.
        let wrong = grid(
            3,
            &[
                (0, 0, planks),
                (1, 0, planks),
                (2, 0, planks),
                (0, 1, items::IRON_INGOT),
                (1, 1, planks),
                (2, 1, planks),
                (1, 2, planks),
            ],
        );
        assert_eq!(recipes.craft(&wrong, 3), None);
        assert_eq!(items::SHIELD.name(), "shield");
    }

    #[test]
    fn planks_make_wooden_slabs_not_stone_ones() {
        let recipes = recipes();
        let planks = items::OAK_PLANKS;
        let row = grid(3, &[(0, 1, planks), (1, 1, planks), (2, 1, planks)]);
        assert_eq!(
            recipes.craft(&row, 3),
            Some(ItemStack::new(items::OAK_SLAB, 6))
        );
        assert_eq!(recipes.burn_ticks(items::OAK_SLAB), 150);
        assert_eq!(recipes.burn_ticks(items::CHERRY_SLAB), 150);
        // Sticks, tables and tools take any planks.
        let cherry = items::CHERRY_PLANKS;
        let column = grid(3, &[(1, 0, cherry), (1, 1, cherry)]);
        assert_eq!(
            recipes.craft(&column, 3),
            Some(ItemStack::new(items::STICK, 4))
        );
    }

    #[test]
    fn every_wood_makes_its_own_set() {
        let recipes = recipes();
        let item = |name: &str| items::by_name(name).unwrap_or_else(|| panic!("no {name}"));
        for wood in [
            "oak", "spruce", "birch", "jungle", "acacia", "dark_oak", "mangrove", "cherry",
            "bamboo",
        ] {
            let planks = item(&format!("{wood}_planks"));
            let stick = items::STICK;
            let door = grid(
                3,
                &[0, 1, 2].map(|y| [(0, y, planks), (1, y, planks)]).concat(),
            );
            let fence = grid(
                3,
                &[(0, 0, planks), (1, 0, stick), (2, 0, planks)]
                    .into_iter()
                    .chain([(0, 1, planks), (1, 1, stick), (2, 1, planks)])
                    .collect::<Vec<_>>(),
            );
            for (grid, part, count) in [(door, "door", 3), (fence, "fence", 3)] {
                assert_eq!(
                    recipes.craft(&grid, 3),
                    Some(ItemStack::new(item(&format!("{wood}_{part}")), count)),
                    "{wood} {part}"
                );
            }
            assert_eq!(recipes.burn_ticks(planks), 300, "{wood}");
        }
    }

    #[test]
    fn shapeless_recipes_take_any_arrangement() {
        let recipes = recipes();
        let (bowl, red, brown) = (items::BOWL, items::RED_MUSHROOM, items::BROWN_MUSHROOM);
        let stew = grid(3, &[(2, 2, bowl), (0, 1, red), (1, 0, brown)]);
        assert_eq!(
            recipes.craft(&stew, 3),
            Some(ItemStack::new(items::MUSHROOM_STEW, 1))
        );
        let red_wool = grid(2, &[(1, 1, items::RED_DYE), (0, 0, items::WHITE_WOOL)]);
        assert_eq!(
            recipes.craft(&red_wool, 2),
            Some(ItemStack::new(items::RED_WOOL, 1))
        );
        // An extra item spoils it.
        let spoiled = grid(
            3,
            &[
                (2, 2, bowl),
                (0, 1, red),
                (1, 0, brown),
                (0, 0, items::DIRT),
            ],
        );
        assert_eq!(recipes.craft(&spoiled, 3), None);
    }

    #[test]
    fn carpets_walls_concrete_powder_and_anvils_are_craftable() {
        let recipes = recipes();
        let wool = items::LIME_WOOL;
        let carpet = grid(2, &[(0, 1, wool), (1, 1, wool)]);
        assert_eq!(
            recipes.craft(&carpet, 2),
            Some(ItemStack::new(items::LIME_CARPET, 3))
        );
        let cobble = items::COBBLESTONE;
        let wall: Vec<_> = (0..6).map(|i| (i % 3, i / 3, cobble)).collect();
        assert_eq!(
            recipes.craft(&grid(3, &wall), 3),
            Some(ItemStack::new(items::COBBLESTONE_WALL, 6))
        );
        let mut powder: Vec<_> = (0..4).map(|i| (i % 3, i / 3, items::SAND)).collect();
        powder.extend((4..8).map(|i| (i % 3, i / 3, items::GRAVEL)));
        powder.push((2, 2, items::RED_DYE));
        assert_eq!(
            recipes.craft(&grid(3, &powder), 3),
            Some(ItemStack::new(items::RED_CONCRETE_POWDER, 8))
        );
        let (block, ingot) = (items::IRON_BLOCK, items::IRON_INGOT);
        let anvil = grid(
            3,
            &[
                (0, 0, block),
                (1, 0, block),
                (2, 0, block),
                (1, 1, ingot),
                (0, 2, ingot),
                (1, 2, ingot),
                (2, 2, ingot),
            ],
        );
        assert_eq!(
            recipes.craft(&anvil, 3),
            Some(ItemStack::new(items::ANVIL, 1))
        );
    }

    #[test]
    fn smelting_and_fuels_follow_1_0_0() {
        let recipes = recipes();
        assert_eq!(
            recipes.smelt(&ItemStack::new(items::IRON_ORE, 1)),
            Some(ItemStack::new(items::IRON_INGOT, 1))
        );
        assert_eq!(
            recipes.smelt(&ItemStack::new(items::BIRCH_LOG, 1)),
            Some(ItemStack::new(items::CHARCOAL, 1))
        );
        assert_eq!(recipes.smelt(&ItemStack::new(items::DIRT, 1)), None);
        assert_eq!(recipes.burn_ticks(items::COAL), 1600);
        assert_eq!(recipes.burn_ticks(items::CHARCOAL), 1600);
        assert_eq!(recipes.burn_ticks(items::SPRUCE_LOG), 300);
        assert_eq!(recipes.burn_ticks(items::OAK_PLANKS), 300);
        assert_eq!(recipes.burn_ticks(items::STICK), 100);
        assert_eq!(recipes.burn_ticks(items::STONE), 0);
        assert!(recipes.shaped.len() > 90);
    }

    #[test]
    fn redstone_rails_and_minecarts_have_their_modern_recipes() {
        let recipes = recipes();
        let (iron, stick, cobble) = (items::IRON_INGOT, items::STICK, items::COBBLESTONE);
        let redstone = items::REDSTONE;
        let piston = grid(
            3,
            &[
                (0, 0, items::OAK_PLANKS),
                (1, 0, items::OAK_PLANKS),
                (2, 0, items::OAK_PLANKS),
                (0, 1, cobble),
                (1, 1, iron),
                (2, 1, cobble),
                (0, 2, cobble),
                (1, 2, redstone),
                (2, 2, cobble),
            ],
        );
        assert_eq!(
            recipes.craft(&piston, 3),
            Some(ItemStack::new(items::PISTON, 1))
        );
        let rails = grid(
            3,
            &[
                (0, 0, iron),
                (2, 0, iron),
                (0, 1, iron),
                (1, 1, stick),
                (2, 1, iron),
                (0, 2, iron),
                (2, 2, iron),
            ],
        );
        assert_eq!(
            recipes.craft(&rails, 3),
            Some(ItemStack::new(items::RAIL, 16))
        );
        let cart = grid(
            3,
            &[
                (0, 1, iron),
                (2, 1, iron),
                (0, 2, iron),
                (1, 2, iron),
                (2, 2, iron),
            ],
        );
        assert_eq!(
            recipes.craft(&cart, 3),
            Some(ItemStack::new(items::MINECART, 1))
        );
        // Shapeless: a chest and a minecart anywhere.
        let chest_cart = grid(2, &[(1, 0, items::MINECART), (0, 1, items::CHEST)]);
        assert_eq!(
            recipes.craft(&chest_cart, 2),
            Some(ItemStack::new(items::CHEST_MINECART, 1))
        );
        let nine = grid(2, &[(0, 0, items::REDSTONE_BLOCK)]);
        assert_eq!(
            recipes.craft(&nine, 2),
            Some(ItemStack::new(items::REDSTONE, 9))
        );
    }
}
