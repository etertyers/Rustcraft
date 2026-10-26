//! The creative inventory's contents and rules: the tabs listed in
//! `assets/data/creative_tabs.ron`, the stacks each lists, search by the
//! start of any word of a name, and what clicking an item does (modern
//! `CreativeModeInventoryScreen`).

use std::sync::LazyLock;

use serde::Deserialize;

use super::{
    item::ItemId,
    items as ids,
    player::Inventory,
    potion,
    stack::{ItemStack, Items},
};

/// One tab of the table.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TabDef {
    pub title: String,
    /// The item drawn on the tab.
    pub icon: ItemId,
    /// What it lists, in order.
    pub items: Vec<ItemId>,
}

#[derive(Deserialize)]
struct Table {
    tabs: Vec<TabDef>,
}

/// The tabs that list items, in order (Search and Survival Inventory
/// aren't in the table).
pub fn tabs() -> &'static [TabDef] {
    static TABS: LazyLock<Vec<TabDef>> = LazyLock::new(|| {
        ron::from_str::<Table>(include_str!("../../assets/data/creative_tabs.ron"))
            .expect("assets/data/creative_tabs.ron is valid")
            .tabs
    });
    &TABS
}

/// The stacks `id` stands for in a tab: itself, or for enchanted books,
/// potions and splash potions each of their variants.
pub fn variants(id: ItemId) -> Vec<ItemStack> {
    if id == ids::ENCHANTED_BOOK {
        return super::enchant::ALL
            .iter()
            .flat_map(|&enchantment| {
                (1..=enchantment.max_level()).map(move |level| {
                    let mut book = ItemStack::new(ids::ENCHANTED_BOOK, 1);
                    book.enchant(enchantment, level);
                    book
                })
            })
            .collect();
    }
    if id == ids::POTION || id == ids::SPLASH_POTION {
        return (0..potion::POTIONS.len() as u16)
            .map(|n| ItemStack::with_damage(id, 1, n))
            .collect();
    }
    vec![ItemStack::with_damage(id, 1, 0)]
}

/// Whether `query` matches the start of `name` or of any word in it,
/// ignoring case: "oak" finds "Oak Planks" and "Dark Oak Log", "ak" and
/// "log oak" don't. An empty query matches everything.
pub fn matches(name: &str, query: &str) -> bool {
    let (name, query) = (name.to_lowercase(), query.trim().to_lowercase());
    if query.is_empty() {
        return true;
    }
    let mut starts = name
        .char_indices()
        .filter(|&(i, c)| {
            c.is_alphanumeric()
                && name[..i]
                    .chars()
                    .next_back()
                    .is_none_or(|before| !before.is_alphanumeric())
        })
        .map(|(i, _)| i);
    starts.any(|start| name[start..].starts_with(&query))
}

/// How the player clicked an item in a tab's grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Pick {
    Left,
    Right,
    /// Shift + left click.
    Shift,
    /// Middle click.
    Clone,
}

/// Clicking `shown` in the grid, with whatever is `cursor` held: with
/// nothing held, a click takes one; with the same item held, a left click
/// adds one and a right click puts one back; with something else held,
/// the click throws it away. Shift puts a full stack in the inventory and
/// the middle button takes a full stack onto an empty cursor.
pub fn pick(
    cursor: &mut Option<ItemStack>,
    inventory: &mut Inventory,
    shown: &ItemStack,
    pick: Pick,
    items: &Items,
) {
    let full = shown.with_count(items.max_stack(shown.id));
    match pick {
        Pick::Shift => {
            // Whatever doesn't fit is dropped, as 1.0.0's quick move does.
            inventory.add(full, items);
        }
        Pick::Clone => {
            if cursor.is_none() {
                *cursor = Some(full);
            }
        }
        Pick::Left | Pick::Right => match cursor {
            None => *cursor = Some(shown.with_count(1)),
            Some(held) if held.stacks_with(shown) => {
                if pick == Pick::Left {
                    held.count = held.count.saturating_add(1).min(full.count);
                } else {
                    held.count -= 1;
                    if held.count == 0 {
                        *cursor = None;
                    }
                }
            }
            Some(_) => *cursor = None,
        },
    }
}

/// Clicking the destroy slot: with a stack held, it is deleted; with
/// shift, everything in the inventory is (vanilla's "clear inventory").
pub fn destroy(cursor: &mut Option<ItemStack>, inventory: &mut Inventory, shift: bool) {
    if shift {
        for slot in 0..super::player::SLOTS {
            inventory.set(slot, None);
        }
    } else {
        *cursor = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inventory::{item, items};

    fn names_in(tab: &str) -> Vec<&'static str> {
        let tab = tabs().iter().find(|t| t.title == tab).expect(tab);
        tab.items.iter().map(|id| id.name()).collect()
    }

    #[test]
    fn the_tabs_follow_vanillas_order_and_none_is_empty() {
        let titles: Vec<&str> = tabs().iter().map(|t| t.title.as_str()).collect();
        assert_eq!(
            titles,
            [
                "Building Blocks",
                "Colored Blocks",
                "Natural Blocks",
                "Functional Blocks",
                "Redstone Blocks",
                "Tools & Utilities",
                "Combat",
                "Food & Drinks",
                "Ingredients",
                "Spawn Eggs"
            ]
        );
        assert!(tabs().iter().all(|t| !t.items.is_empty()));
    }

    #[test]
    fn every_registered_item_is_in_at_least_one_tab() {
        let listed: std::collections::BTreeSet<ItemId> = tabs()
            .iter()
            .flat_map(|t| t.items.iter().copied())
            .collect();
        let missing: Vec<&str> = item::registry()
            .iter()
            .filter(|def| !listed.contains(&def.id))
            .map(|def| def.name.as_str())
            .collect();
        assert!(missing.is_empty(), "in no tab: {missing:?}");
    }

    #[test]
    fn items_sit_in_their_vanilla_tabs_in_vanilla_order() {
        let building = names_in("Building Blocks");
        let position = |list: &[&str], name: &str| list.iter().position(|n| *n == name).unwrap();
        // A wood set runs log, wood, stripped, planks, stairs, slab.
        assert!(position(&building, "oak_log") < position(&building, "oak_planks"));
        assert!(position(&building, "oak_planks") < position(&building, "oak_stairs"));
        assert!(position(&building, "oak_stairs") < position(&building, "oak_slab"));
        assert!(position(&building, "oak_slab") < position(&building, "spruce_log"));
        // Colors go in dye order: white, light gray, gray, black, brown, red...
        let colored = names_in("Colored Blocks");
        assert!(position(&colored, "white_wool") < position(&colored, "light_gray_wool"));
        assert!(position(&colored, "black_wool") < position(&colored, "red_wool"));
        assert!(position(&colored, "pink_wool") < position(&colored, "white_carpet"));
        assert!(names_in("Natural Blocks").contains(&"grass_block"));
        assert!(names_in("Functional Blocks").contains(&"crafting_table"));
        assert!(names_in("Redstone Blocks").contains(&"piston"));
        assert!(names_in("Redstone Blocks").contains(&"oak_door"));
        assert!(names_in("Tools & Utilities").contains(&"diamond_pickaxe"));
        assert!(names_in("Combat").contains(&"diamond_sword"));
        assert!(names_in("Food & Drinks").contains(&"golden_apple"));
        assert!(names_in("Ingredients").contains(&"iron_ingot"));
        // Placeholders from the vanilla data are in their tabs too.
        assert!(!names_in("Spawn Eggs").contains(&"sniffer_egg"));
        assert!(names_in("Natural Blocks").contains(&"sniffer_egg"));
        assert!(names_in("Combat").contains(&"mace"));
        assert!(names_in("Tools & Utilities").contains(&"spyglass"));
        // Spawn eggs are only in the Spawn Eggs tab.
        let eggs = names_in("Spawn Eggs");
        assert!(eggs.len() > 20 && eggs.iter().all(|n| n.ends_with("_spawn_egg")));
        for tab in tabs().iter().filter(|t| t.title != "Spawn Eggs") {
            assert!(
                tab.items
                    .iter()
                    .all(|id| !id.name().ends_with("_spawn_egg"))
            );
        }
    }

    #[test]
    fn variants_stand_for_each_book_and_potion() {
        assert_eq!(variants(items::STICK).len(), 1);
        assert!(variants(items::ENCHANTED_BOOK).len() > 30);
        assert!(
            variants(items::ENCHANTED_BOOK)
                .iter()
                .all(|b| b.is_enchanted())
        );
        assert_eq!(variants(items::POTION).len(), potion::POTIONS.len());
        assert_eq!(variants(items::SPLASH_POTION).len(), potion::POTIONS.len());
    }

    #[test]
    fn search_matches_the_start_of_any_word_ignoring_case() {
        for name in ["Oak Planks", "Dark Oak Log", "oak log"] {
            assert!(matches(name, "oak"), "{name}");
            assert!(matches(name, "OAK"), "{name}");
        }
        assert!(matches("Dark Oak Log", "dark oak"));
        assert!(matches("Dark Oak Log", "o"));
        assert!(!matches("Oak Planks", "ak"));
        assert!(!matches("Dark Oak Log", "log oak"));
        assert!(!matches("Cloak", "oak"), "inside a word isn't a start");
        assert!(matches("Anything", ""));
        assert!(matches("Anything", "   "));
        // Words split at punctuation too.
        assert!(matches("Pointed-Dripstone", "dripstone"));
    }

    #[test]
    fn search_finds_oak_planks_and_dark_oak_log_among_all_items() {
        let items = Items::new();
        let found: Vec<String> = item::registry()
            .iter()
            .map(|def| items.name(&ItemStack::new(def.id, 1)))
            .filter(|name| matches(name, "oak"))
            .collect();
        assert!(found.iter().any(|n| n.eq_ignore_ascii_case("oak planks")));
        assert!(found.iter().any(|n| n.eq_ignore_ascii_case("dark oak log")));
        assert!(found.iter().all(|n| n.to_lowercase().contains("oak")));
    }

    fn setup() -> (Items, Inventory, ItemStack) {
        (
            Items::new(),
            Inventory::default(),
            ItemStack::new(items::OAK_PLANKS, 1),
        )
    }

    #[test]
    fn clicking_an_item_takes_one_then_adds_or_removes_one_like_vanilla() {
        let (items, mut inventory, planks) = setup();
        let mut cursor = None;
        pick(&mut cursor, &mut inventory, &planks, Pick::Left, &items);
        assert_eq!(cursor, Some(planks.with_count(1)));
        pick(&mut cursor, &mut inventory, &planks, Pick::Left, &items);
        pick(&mut cursor, &mut inventory, &planks, Pick::Left, &items);
        assert_eq!(cursor.as_ref().unwrap().count, 3);
        pick(&mut cursor, &mut inventory, &planks, Pick::Right, &items);
        assert_eq!(cursor.as_ref().unwrap().count, 2);
        // Up to a full stack, no further.
        for _ in 0..100 {
            pick(&mut cursor, &mut inventory, &planks, Pick::Left, &items);
        }
        assert_eq!(cursor.as_ref().unwrap().count, 64);
        // Nothing else was touched.
        assert_eq!(inventory, Inventory::default());
        // Right clicks put them back until none is left.
        for _ in 0..64 {
            pick(&mut cursor, &mut inventory, &planks, Pick::Right, &items);
        }
        assert_eq!(cursor, None);
    }

    #[test]
    fn clicking_another_item_with_a_stack_held_throws_the_held_one_away() {
        let (items, mut inventory, planks) = setup();
        let mut cursor = Some(ItemStack::new(items::STICK, 5));
        pick(&mut cursor, &mut inventory, &planks, Pick::Left, &items);
        assert_eq!(cursor, None);
    }

    #[test]
    fn shift_puts_a_full_stack_in_the_inventory_and_middle_takes_one_onto_the_cursor() {
        let (items, mut inventory, planks) = setup();
        let mut cursor = None;
        pick(&mut cursor, &mut inventory, &planks, Pick::Shift, &items);
        assert_eq!(cursor, None);
        assert_eq!(inventory.slot(0), Some(&planks.with_count(64)));
        pick(&mut cursor, &mut inventory, &planks, Pick::Shift, &items);
        assert_eq!(
            inventory.slot(1),
            Some(&planks.with_count(64)),
            "next free slot"
        );
        // A tool stacks to 1, so shift gives one.
        let pickaxe = ItemStack::new(items::DIAMOND_PICKAXE, 1);
        pick(&mut cursor, &mut inventory, &pickaxe, Pick::Shift, &items);
        assert_eq!(inventory.slot(2), Some(&pickaxe));
        // Middle click: a full stack on an empty cursor, only then.
        pick(&mut cursor, &mut inventory, &planks, Pick::Clone, &items);
        assert_eq!(cursor, Some(planks.with_count(64)));
        let before = cursor.clone();
        pick(&mut cursor, &mut inventory, &pickaxe, Pick::Clone, &items);
        assert_eq!(cursor, before);
    }

    #[test]
    fn the_destroy_slot_deletes_the_held_stack_and_with_shift_the_whole_inventory() {
        let (items, mut inventory, planks) = setup();
        inventory.add(planks.with_count(10), &items);
        inventory.set(
            crate::inventory::player::ARMOR_HEAD,
            Some(ItemStack::new(items::IRON_HELMET, 1)),
        );
        let mut cursor = Some(planks.with_count(3));
        destroy(&mut cursor, &mut inventory, false);
        assert_eq!(cursor, None);
        assert!(inventory.slot(0).is_some(), "the inventory stays");
        destroy(&mut cursor, &mut inventory, true);
        assert_eq!(inventory, Inventory::default());
    }
}
