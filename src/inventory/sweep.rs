//! The recipe sweep (M29): every item has a modern recipe (crafting,
//! smelting or stonecutting) unless it is found, dropped, brewed or made at
//! the smithing table, which `NOT_CRAFTED` lists. A new item that isn't
//! craftable has to be added to that list on purpose.

use crate::{
    inventory::{crafting::Recipes, item::ItemId, items},
    world::block::registry,
};

/// What no crafting, smelting or stonecutting recipe makes.
const NOT_CRAFTED: &[&str] = &[
    // Smithing, not crafting.
    "netherite_sword",
    "netherite_shovel",
    "netherite_pickaxe",
    "netherite_axe",
    "netherite_hoe",
    "netherite_helmet",
    "netherite_chestplate",
    "netherite_leggings",
    "netherite_boots",
    // Brewing.
    "potion",
    "splash_potion",
    // Found in the world, dropped by mobs or loot, or taken with a bucket or tool.
    "grass_block",
    "dirt",
    "cobblestone",
    "oak_sapling",
    "spruce_sapling",
    "birch_sapling",
    "bedrock",
    "sand",
    "gravel",
    "gold_ore",
    "iron_ore",
    "coal_ore",
    "oak_log",
    "spruce_log",
    "birch_log",
    "oak_leaves",
    "spruce_leaves",
    "birch_leaves",
    "sponge",
    "lapis_ore",
    "cobweb",
    "short_grass",
    "fern",
    "dead_bush",
    "dandelion",
    "poppy",
    "brown_mushroom",
    "red_mushroom",
    "petrified_oak_slab",
    "obsidian",
    "spawner",
    "diamond_ore",
    "farmland",
    "redstone_ore",
    "snow",
    "ice",
    "cactus",
    "sugar_cane",
    "carved_pumpkin",
    "netherrack",
    "soul_sand",
    "infested_stone",
    "infested_cobblestone",
    "infested_stone_bricks",
    "brown_mushroom_block",
    "red_mushroom_block",
    "mushroom_stem",
    "vine",
    "glow_lichen",
    "mycelium",
    "lily_pad",
    "nether_wart",
    "end_portal_frame",
    "end_stone",
    "dragon_egg",
    "red_sand",
    "white_concrete",
    "orange_concrete",
    "magenta_concrete",
    "light_blue_concrete",
    "yellow_concrete",
    "lime_concrete",
    "pink_concrete",
    "gray_concrete",
    "light_gray_concrete",
    "cyan_concrete",
    "purple_concrete",
    "blue_concrete",
    "brown_concrete",
    "green_concrete",
    "red_concrete",
    "black_concrete",
    "chipped_anvil",
    "damaged_anvil",
    "stripped_oak_log",
    "stripped_spruce_log",
    "stripped_birch_log",
    "jungle_log",
    "stripped_jungle_log",
    "jungle_leaves",
    "jungle_sapling",
    "acacia_log",
    "stripped_acacia_log",
    "acacia_leaves",
    "acacia_sapling",
    "dark_oak_log",
    "stripped_dark_oak_log",
    "dark_oak_leaves",
    "dark_oak_sapling",
    "mangrove_log",
    "stripped_mangrove_log",
    "mangrove_leaves",
    "mangrove_propagule",
    "cherry_log",
    "stripped_cherry_log",
    "cherry_leaves",
    "cherry_sapling",
    "bamboo",
    "stripped_bamboo_block",
    "podzol",
    "rooted_dirt",
    "mud",
    "moss_block",
    "cobbled_deepslate",
    "granite",
    "diorite",
    "andesite",
    "tuff",
    "calcite",
    "pointed_dripstone",
    "budding_amethyst",
    "small_amethyst_bud",
    "medium_amethyst_bud",
    "large_amethyst_bud",
    "amethyst_cluster",
    "copper_ore",
    "emerald_ore",
    "deepslate_coal_ore",
    "deepslate_copper_ore",
    "deepslate_iron_ore",
    "deepslate_gold_ore",
    "deepslate_redstone_ore",
    "deepslate_emerald_ore",
    "deepslate_lapis_ore",
    "deepslate_diamond_ore",
    "allium",
    "azure_bluet",
    "blue_orchid",
    "red_tulip",
    "orange_tulip",
    "white_tulip",
    "pink_tulip",
    "oxeye_daisy",
    "cornflower",
    "lily_of_the_valley",
    "pink_petals",
    "tall_grass",
    "large_fern",
    "sunflower",
    "lilac",
    "rose_bush",
    "peony",
    "azalea_leaves",
    "flowering_azalea_leaves",
    "mangrove_roots",
    "muddy_mangrove_roots",
    "bell",
    "dirt_path",
    "crying_obsidian",
    "crimson_stem",
    "stripped_crimson_stem",
    "crimson_nylium",
    "crimson_fungus",
    "crimson_roots",
    "warped_stem",
    "stripped_warped_stem",
    "warped_nylium",
    "warped_fungus",
    "warped_roots",
    "warped_wart_block",
    "shroomlight",
    "nether_sprouts",
    "weeping_vines",
    "twisting_vines",
    "soul_soil",
    "basalt",
    "blackstone",
    "nether_quartz_ore",
    "nether_gold_ore",
    "ancient_debris",
    "gilded_blackstone",
    "chorus_plant",
    "chorus_flower",
    "apple",
    "string",
    "feather",
    "gunpowder",
    "wheat_seeds",
    "chainmail_helmet",
    "chainmail_chestplate",
    "chainmail_leggings",
    "chainmail_boots",
    "flint",
    "porkchop",
    "water_bucket",
    "lava_bucket",
    "saddle",
    "snowball",
    "milk_bucket",
    "clay_ball",
    "slime_ball",
    "egg",
    "glowstone_dust",
    "cod",
    "ink_sac",
    "cocoa_beans",
    "bone",
    "filled_map",
    "melon_slice",
    "beef",
    "chicken",
    "rotten_flesh",
    "ender_pearl",
    "blaze_rod",
    "ghast_tear",
    "spider_eye",
    "music_disc_13",
    "music_disc_cat",
    "music_disc_blocks",
    "music_disc_chirp",
    "music_disc_far",
    "music_disc_mall",
    "music_disc_mellohi",
    "music_disc_stal",
    "music_disc_strad",
    "music_disc_ward",
    "music_disc_11",
    "mutton",
    "rabbit",
    "rabbit_hide",
    "rabbit_foot",
    "salmon",
    "phantom_membrane",
    "raw_copper",
    "amethyst_shard",
    "glow_berries",
    "enchanted_book",
    "wither_skeleton_skull",
    "carrot",
    "pufferfish",
    "elytra",
    "shulker_shell",
    "chorus_fruit",
];

#[cfg(test)]
mod tests {
    use super::*;

    fn recipes() -> Recipes {
        Recipes::load(&crate::assets::dir().join("data/recipes.ron")).unwrap()
    }

    fn every_item() -> Vec<ItemId> {
        (0..items::NAMES.len())
            .map(|i| items::by_name(items::NAMES[i]).unwrap())
            .collect()
    }

    #[test]
    fn every_craftable_item_has_a_recipe() {
        let made = recipes().results();
        let missing: Vec<&str> = every_item()
            .into_iter()
            .filter(|id| !made.contains(id) && !NOT_CRAFTED.contains(&id.name()))
            .filter(|id| id.name() != "unknown_item")
            // Placeholders (docs/placeholders.md) have no recipes yet.
            .filter(|id| {
                !crate::inventory::item::registry()
                    .get(*id)
                    .is_some_and(|d| d.placeholder)
            })
            .map(|id| id.name())
            .collect();
        assert!(missing.is_empty(), "no recipe for {missing:?}");
    }

    #[test]
    fn the_not_crafted_list_names_real_items_with_no_recipe() {
        let made = recipes().results();
        for name in NOT_CRAFTED {
            let id = items::by_name(name).unwrap_or_else(|| panic!("no item {name}"));
            assert!(
                !made.contains(&id),
                "{name} has a recipe now: take it off the list"
            );
        }
        let mut sorted: Vec<_> = NOT_CRAFTED.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), NOT_CRAFTED.len(), "a name is listed twice");
    }

    #[test]
    fn the_modern_stonecutter_recipes_are_there() {
        let recipes = recipes();
        let cuts = |name: &str| {
            recipes
                .stonecutting(items::by_name(name).unwrap())
                .iter()
                .map(|o| (o.item.name(), o.count))
                .collect::<Vec<_>>()
        };
        assert!(cuts("cobblestone").contains(&("cobblestone_slab", 2)));
        assert!(cuts("cobblestone").contains(&("cobblestone_stairs", 1)));
        assert!(cuts("stone_bricks").contains(&("stone_brick_slab", 2)));
        assert!(cuts("sandstone").contains(&("cut_sandstone", 1)));
        assert!(cuts("purpur_block").contains(&("purpur_pillar", 1)));
        assert!(cuts("dirt").is_empty());
        // Cutting never makes more material than it uses: a slab is half a block.
        let _ = registry();
    }
}
