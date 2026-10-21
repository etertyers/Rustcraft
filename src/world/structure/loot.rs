//! What the chests in structures hold: modern Java Edition's loot tables
//! cut down to the items Rustcraft has.

use crate::{
    inventory::{
        enchant, items,
        stack::{ItemStack, Items},
    },
    world::{block_entity::CHEST_SLOTS, worldgen::rng::Rng},
};

/// One thing a pool can give: `weight` against the pool's others, `min`
/// to `max` of it, enchanted at `enchant` levels of power if set (a
/// `book` becomes an enchanted book).
#[derive(Debug, Clone, Copy)]
pub struct Entry {
    pub item: &'static str,
    pub weight: u32,
    pub min: u8,
    pub max: u8,
    pub enchant: Option<i32>,
}

const fn e(item: &'static str, weight: u32, min: u8, max: u8) -> Entry {
    Entry {
        item,
        weight,
        min,
        max,
        enchant: None,
    }
}

const fn ench(item: &'static str, weight: u32, level: i32) -> Entry {
    Entry {
        item,
        weight,
        min: 1,
        max: 1,
        enchant: Some(level),
    }
}

/// Nine parts in ten of a pool giving nothing.
const NOTHING_9: Entry = e("", 9, 1, 1);

/// Rolls `rolls` times (a range) into `entries`.
pub struct Pool {
    pub rolls: (u32, u32),
    pub entries: Vec<Entry>,
}

fn pool(min: u32, max: u32, entries: Vec<Entry>) -> Pool {
    Pool {
        rolls: (min, max),
        entries,
    }
}

/// The pools of the table called `name`.
pub fn table(name: &str) -> Option<Vec<Pool>> {
    Some(match name {
        "simple_dungeon" => vec![
            pool(
                1,
                3,
                vec![
                    e("saddle", 20, 1, 1),
                    e("golden_apple", 15, 1, 1),
                    e("music_disc_cat", 15, 1, 1),
                    e("music_disc_blocks", 15, 1, 1),
                    ench("book", 10, 30),
                ],
            ),
            pool(
                1,
                4,
                vec![
                    e("iron_ingot", 10, 1, 4),
                    e("gold_ingot", 5, 1, 4),
                    e("bread", 20, 1, 1),
                    e("wheat", 20, 1, 4),
                    e("bucket", 10, 1, 1),
                    e("redstone", 15, 1, 4),
                    e("coal", 15, 1, 4),
                    e("melon_seeds", 10, 2, 4),
                    e("pumpkin_seeds", 10, 2, 4),
                ],
            ),
            pool(
                3,
                3,
                vec![
                    e("bone", 10, 1, 8),
                    e("gunpowder", 10, 1, 8),
                    e("rotten_flesh", 10, 1, 8),
                    e("string", 10, 1, 8),
                ],
            ),
        ],
        "abandoned_mineshaft" => vec![
            pool(
                1,
                1,
                vec![
                    e("golden_apple", 20, 1, 1),
                    ench("book", 10, 30),
                    e("iron_pickaxe", 5, 1, 1),
                ],
            ),
            pool(
                2,
                4,
                vec![
                    e("iron_ingot", 10, 1, 5),
                    e("gold_ingot", 5, 1, 3),
                    e("redstone", 5, 4, 9),
                    e("lapis_lazuli", 5, 4, 9),
                    e("diamond", 3, 1, 2),
                    e("coal", 10, 3, 8),
                    e("bread", 15, 1, 3),
                    e("glow_berries", 15, 3, 6),
                    e("melon_seeds", 10, 2, 4),
                    e("pumpkin_seeds", 10, 2, 4),
                ],
            ),
            pool(
                3,
                3,
                vec![
                    e("rail", 20, 4, 8),
                    e("powered_rail", 5, 1, 4),
                    e("detector_rail", 5, 1, 4),
                    e("activator_rail", 5, 1, 4),
                    e("torch", 15, 1, 16),
                ],
            ),
        ],
        "desert_pyramid" => vec![
            pool(
                2,
                4,
                vec![
                    e("diamond", 5, 1, 3),
                    e("iron_ingot", 15, 1, 5),
                    e("gold_ingot", 15, 2, 7),
                    e("emerald", 15, 1, 3),
                    e("bone", 25, 4, 6),
                    e("spider_eye", 25, 1, 3),
                    e("rotten_flesh", 25, 3, 7),
                    e("saddle", 20, 1, 1),
                    e("golden_apple", 20, 1, 1),
                    ench("book", 20, 30),
                ],
            ),
            pool(
                4,
                4,
                vec![
                    e("bone", 10, 1, 8),
                    e("gunpowder", 10, 1, 8),
                    e("rotten_flesh", 10, 1, 8),
                    e("string", 10, 1, 8),
                    e("sand", 10, 1, 8),
                ],
            ),
        ],
        "jungle_temple" => vec![pool(
            2,
            6,
            vec![
                e("diamond", 3, 1, 3),
                e("iron_ingot", 10, 1, 5),
                e("gold_ingot", 15, 2, 7),
                e("emerald", 2, 1, 3),
                e("bone", 20, 4, 6),
                e("rotten_flesh", 16, 3, 7),
                e("saddle", 3, 1, 1),
                ench("book", 1, 30),
                e("bamboo", 15, 1, 3),
            ],
        )],
        "jungle_temple_dispenser" => vec![pool(1, 2, vec![e("arrow", 30, 2, 7)])],
        "stronghold_corridor" => vec![pool(
            2,
            3,
            vec![
                e("ender_pearl", 10, 1, 1),
                e("diamond", 3, 1, 3),
                e("iron_ingot", 10, 1, 5),
                e("gold_ingot", 5, 1, 3),
                e("redstone", 5, 4, 9),
                e("bread", 15, 1, 3),
                e("apple", 15, 1, 3),
                e("iron_pickaxe", 5, 1, 1),
                e("iron_sword", 5, 1, 1),
                e("iron_chestplate", 5, 1, 1),
                e("iron_helmet", 5, 1, 1),
                e("iron_leggings", 5, 1, 1),
                e("iron_boots", 5, 1, 1),
                e("golden_apple", 1, 1, 1),
                e("saddle", 1, 1, 1),
                ench("book", 1, 30),
            ],
        )],
        "stronghold_crossing" => vec![pool(
            1,
            4,
            vec![
                e("iron_ingot", 10, 1, 5),
                e("gold_ingot", 5, 1, 3),
                e("redstone", 5, 4, 9),
                e("coal", 10, 3, 8),
                e("bread", 15, 1, 3),
                e("apple", 15, 1, 3),
                e("iron_pickaxe", 1, 1, 1),
                ench("book", 1, 30),
            ],
        )],
        "stronghold_library" => vec![pool(
            2,
            10,
            vec![
                e("book", 20, 1, 3),
                e("paper", 20, 2, 7),
                e("compass", 1, 1, 1),
                ench("book", 10, 30),
            ],
        )],
        "shipwreck_supply" => vec![pool(
            3,
            10,
            vec![
                e("paper", 8, 1, 12),
                e("wheat", 7, 8, 21),
                e("coal", 6, 2, 8),
                e("rotten_flesh", 5, 5, 24),
                e("gunpowder", 3, 1, 5),
                e("carved_pumpkin", 2, 1, 3),
                ench("leather_helmet", 3, 15),
                ench("leather_chestplate", 3, 15),
                ench("leather_leggings", 3, 15),
                ench("leather_boots", 3, 15),
                ench("book", 1, 30),
            ],
        )],
        "shipwreck_treasure" => vec![
            pool(
                3,
                6,
                vec![
                    e("iron_ingot", 90, 1, 5),
                    e("gold_ingot", 10, 1, 5),
                    e("emerald", 40, 1, 5),
                    e("diamond", 5, 1, 1),
                ],
            ),
            pool(
                2,
                5,
                vec![e("gold_nugget", 50, 1, 10), e("lapis_lazuli", 20, 1, 10)],
            ),
        ],
        "shipwreck_map" => vec![pool(
            1,
            3,
            vec![
                e("paper", 20, 1, 10),
                e("feather", 10, 1, 5),
                e("book", 5, 1, 5),
                e("compass", 10, 1, 1),
                e("clock", 1, 1, 1),
            ],
        )],
        "ruined_portal" => vec![pool(
            4,
            8,
            vec![
                e("obsidian", 40, 1, 2),
                e("flint", 40, 1, 4),
                e("flint_and_steel", 40, 1, 1),
                e("golden_apple", 15, 1, 1),
                e("gold_nugget", 15, 4, 24),
                ench("golden_sword", 15, 15),
                ench("golden_axe", 15, 15),
                ench("golden_hoe", 15, 15),
                ench("golden_shovel", 15, 15),
                ench("golden_pickaxe", 15, 15),
                ench("golden_boots", 15, 15),
                ench("golden_helmet", 15, 15),
                ench("golden_chestplate", 15, 15),
                ench("golden_leggings", 15, 15),
                e("glistering_melon_slice", 5, 4, 12),
                e("clock", 5, 1, 1),
                e("gold_ingot", 5, 2, 8),
                e("bell", 1, 1, 1),
                e("gold_block", 1, 1, 2),
            ],
        )],
        "village_house" => vec![pool(
            3,
            8,
            vec![
                e("gold_nugget", 1, 1, 3),
                e("dandelion", 2, 1, 1),
                e("poppy", 1, 1, 1),
                e("bread", 10, 1, 4),
                e("apple", 10, 1, 5),
                e("book", 1, 1, 1),
                e("feather", 1, 1, 1),
                e("emerald", 2, 1, 4),
                e("oak_sapling", 5, 1, 2),
                e("wheat_seeds", 5, 2, 6),
            ],
        )],
        "village_weaponsmith" => vec![pool(
            3,
            8,
            vec![
                e("diamond", 3, 1, 3),
                e("iron_ingot", 10, 1, 5),
                e("gold_ingot", 5, 1, 3),
                e("bread", 15, 1, 3),
                e("apple", 15, 1, 3),
                e("iron_pickaxe", 5, 1, 1),
                e("iron_sword", 5, 1, 1),
                e("iron_chestplate", 5, 1, 1),
                e("iron_helmet", 5, 1, 1),
                e("iron_leggings", 5, 1, 1),
                e("iron_boots", 5, 1, 1),
                e("obsidian", 5, 3, 7),
                e("oak_sapling", 5, 3, 7),
                e("saddle", 3, 1, 1),
            ],
        )],
        "village_toolsmith" => vec![pool(
            3,
            8,
            vec![
                e("diamond", 1, 1, 3),
                e("iron_ingot", 5, 1, 5),
                e("gold_ingot", 1, 1, 3),
                e("bread", 15, 1, 3),
                e("iron_pickaxe", 5, 1, 1),
                e("coal", 1, 1, 3),
                e("stick", 20, 1, 3),
                e("iron_shovel", 5, 1, 1),
            ],
        )],
        "village_armorer" => vec![pool(
            1,
            5,
            vec![
                e("iron_ingot", 2, 1, 3),
                e("bread", 4, 1, 4),
                e("iron_helmet", 1, 1, 1),
                e("emerald", 1, 1, 1),
            ],
        )],
        "village_cleric" => vec![pool(
            1,
            5,
            vec![
                e("redstone", 1, 1, 4),
                e("bread", 7, 1, 4),
                e("rotten_flesh", 7, 1, 4),
                e("lapis_lazuli", 1, 1, 4),
                e("gold_ingot", 1, 1, 4),
            ],
        )],
        "village_butcher" => vec![pool(
            1,
            5,
            vec![
                e("emerald", 1, 1, 1),
                e("porkchop", 6, 1, 3),
                e("wheat", 6, 1, 3),
                e("beef", 6, 1, 3),
                e("mutton", 6, 1, 3),
                e("coal", 3, 1, 3),
            ],
        )],
        // M25: modern's Nether tables, less horse armor, crossbows,
        // spectral arrows, golden carrots and lodestones (not in yet).
        "nether_bridge" => vec![pool(
            2,
            4,
            vec![
                e("diamond", 5, 1, 3),
                e("iron_ingot", 5, 1, 5),
                e("gold_ingot", 15, 1, 3),
                e("golden_sword", 5, 1, 1),
                e("golden_chestplate", 5, 1, 1),
                e("flint_and_steel", 5, 1, 1),
                e("nether_wart", 5, 3, 7),
                e("saddle", 10, 1, 1),
                e("obsidian", 2, 2, 4),
            ],
        )],
        "bastion_treasure" => vec![
            pool(
                3,
                3,
                vec![
                    e("netherite_ingot", 15, 1, 1),
                    e("ancient_debris", 10, 1, 1),
                    e("netherite_scrap", 8, 1, 1),
                    e("ancient_debris", 4, 2, 2),
                    ench("diamond_sword", 6, 30),
                    ench("diamond_chestplate", 6, 30),
                    ench("diamond_helmet", 6, 30),
                    ench("diamond_leggings", 6, 30),
                    ench("diamond_boots", 6, 30),
                    e("diamond", 5, 2, 6),
                    e("golden_apple", 2, 1, 1),
                ],
            ),
            pool(
                3,
                4,
                vec![
                    e("diamond", 1, 1, 1),
                    e("gold_block", 1, 2, 5),
                    e("gold_ingot", 1, 3, 9),
                    e("iron_ingot", 1, 3, 9),
                    e("crying_obsidian", 1, 3, 5),
                    e("quartz", 1, 8, 23),
                    e("gilded_blackstone", 1, 5, 15),
                    e("magma_cream", 1, 3, 8),
                ],
            ),
            pool(
                1,
                1,
                vec![e("netherite_upgrade_smithing_template", 1, 1, 1)],
            ),
        ],
        "bastion_other" => vec![
            pool(
                1,
                1,
                vec![
                    ench("diamond_pickaxe", 6, 30),
                    ench("diamond_shovel", 6, 30),
                    e("netherite_scrap", 4, 1, 1),
                    e("ancient_debris", 12, 1, 1),
                    e("golden_apple", 5, 1, 1),
                    e("golden_boots", 6, 1, 1),
                    e("golden_sword", 6, 1, 1),
                    e("golden_axe", 6, 1, 1),
                    ench("book", 5, 10),
                ],
            ),
            pool(
                2,
                2,
                vec![
                    e("iron_nugget", 1, 2, 6),
                    e("gold_nugget", 1, 2, 6),
                    e("gilded_blackstone", 1, 1, 5),
                    e("string", 1, 3, 6),
                    e("leather", 1, 1, 3),
                    e("arrow", 1, 5, 17),
                    e("iron_ingot", 1, 2, 6),
                    e("gold_ingot", 1, 2, 6),
                    e("chain", 1, 2, 10),
                    e("cooked_porkchop", 1, 2, 6),
                    e("magma_cream", 1, 2, 6),
                    e("crying_obsidian", 1, 1, 5),
                    e("obsidian", 1, 4, 6),
                ],
            ),
            // One time in ten (the treasure room's always has one).
            pool(
                1,
                1,
                vec![e("netherite_upgrade_smithing_template", 1, 1, 1), NOTHING_9],
            ),
        ],
        "bastion_hoglin_stable" => vec![
            pool(
                1,
                1,
                vec![
                    ench("diamond_shovel", 15, 30),
                    ench("diamond_pickaxe", 12, 30),
                    e("netherite_scrap", 8, 1, 1),
                    e("ancient_debris", 12, 1, 1),
                    e("ancient_debris", 5, 2, 2),
                    e("saddle", 12, 1, 1),
                    e("gold_block", 16, 2, 4),
                    e("golden_axe", 10, 1, 1),
                ],
            ),
            pool(
                3,
                4,
                vec![
                    e("glowstone", 3, 2, 5),
                    e("gold_ingot", 2, 3, 6),
                    e("soul_sand", 2, 2, 7),
                    e("crimson_nylium", 2, 2, 7),
                    e("porkchop", 2, 2, 5),
                    e("cooked_porkchop", 2, 2, 7),
                    e("crimson_fungus", 2, 2, 7),
                    e("leather", 1, 1, 3),
                    e("arrow", 1, 5, 17),
                    e("string", 1, 3, 8),
                ],
            ),
            // One time in ten (the treasure room's always has one).
            pool(
                1,
                1,
                vec![e("netherite_upgrade_smithing_template", 1, 1, 1), NOTHING_9],
            ),
        ],
        _ => return None,
    })
}

/// Rolls the table called `name` into a chest's slots, each stack in a
/// random empty slot. Unknown tables (or names) give nothing.
pub fn fill(name: &str, rng: &mut Rng) -> Vec<Option<ItemStack>> {
    let mut slots = vec![None; CHEST_SLOTS];
    let Some(pools) = table(name) else {
        log::warn!("no loot table {name:?}");
        return slots;
    };
    let items = Items::new();
    for pool in &pools {
        let (lo, hi) = pool.rolls;
        let rolls = lo + rng.below(hi - lo + 1);
        let total: u32 = pool.entries.iter().map(|entry| entry.weight).sum();
        for _ in 0..rolls {
            let mut pick = rng.below(total);
            let Some(entry) = pool.entries.iter().find(|entry| {
                let hit = pick < entry.weight;
                pick = pick.saturating_sub(entry.weight);
                hit
            }) else {
                continue;
            };
            let Some(id) = items::by_name(entry.item) else {
                continue;
            };
            let count = entry.min + rng.below(u32::from(entry.max - entry.min) + 1) as u8;
            let mut stack = ItemStack::new(id, count.min(items.max_stack(id)));
            if let Some(level) = entry.enchant {
                let enchants = enchant::random_for(&items, id, level, rng);
                if id == items::BOOK {
                    stack = ItemStack::new(items::ENCHANTED_BOOK, 1);
                    // A book gets just one.
                    if let Some(&(e, l)) = enchants.first() {
                        stack.enchant(e, l);
                    }
                } else {
                    for (e, l) in enchants {
                        stack.enchant(e, l);
                    }
                }
            }
            let empty: Vec<usize> = (0..CHEST_SLOTS).filter(|&i| slots[i].is_none()).collect();
            if empty.is_empty() {
                break;
            }
            slots[empty[rng.below(empty.len() as u32) as usize]] = Some(stack);
        }
    }
    slots
}

/// Every table, for checking.
pub const TABLES: &[&str] = &[
    "simple_dungeon",
    "abandoned_mineshaft",
    "desert_pyramid",
    "jungle_temple",
    "jungle_temple_dispenser",
    "stronghold_corridor",
    "stronghold_crossing",
    "stronghold_library",
    "shipwreck_supply",
    "shipwreck_treasure",
    "shipwreck_map",
    "ruined_portal",
    "nether_bridge",
    "bastion_treasure",
    "bastion_other",
    "bastion_hoglin_stable",
    "village_house",
    "village_weaponsmith",
    "village_toolsmith",
    "village_armorer",
    "village_cleric",
    "village_butcher",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_item_in_every_table_exists() {
        for name in TABLES {
            for pool in table(name).unwrap() {
                for entry in pool
                    .entries
                    .into_iter()
                    .filter(|entry| !entry.item.is_empty())
                {
                    assert!(
                        items::by_name(entry.item).is_some(),
                        "{name}: no item {}",
                        entry.item
                    );
                    assert!(entry.min <= entry.max && entry.weight > 0);
                }
            }
        }
    }

    #[test]
    fn a_dungeon_chest_fills_with_its_pools() {
        let mut rng = Rng::new(3);
        for _ in 0..50 {
            let slots = fill("simple_dungeon", &mut rng);
            let stacks: Vec<_> = slots.iter().flatten().collect();
            // 1–3 + 1–4 + 3 rolls.
            assert!((5..=10).contains(&stacks.len()), "{}", stacks.len());
        }
    }

    #[test]
    fn enchanted_entries_come_enchanted() {
        let mut rng = Rng::new(9);
        let mut books = 0;
        for _ in 0..200 {
            for stack in fill("stronghold_library", &mut rng).into_iter().flatten() {
                if stack.id == items::ENCHANTED_BOOK {
                    assert_eq!(stack.enchantments().len(), 1);
                    books += 1;
                }
            }
        }
        assert!(books > 50, "{books}");
    }
}
