//! The names of sound events that depend on what makes them: a block's
//! sounds come from its sound group (`block.stone.break`), a mob's from its
//! kind (`entity.zombie.hurt`). Events with a fixed name are written where
//! they are played, as `audio::play("block.chest.open", at)`.

/// The sound group of a block, from its name: `stone` for stone,
/// `wood`, `gravel`, `grass`, `sand`, `glass`, `wool`, `metal`... Every
/// group has `break`, `step`, `place`, `hit` and `fall` events in vanilla's
/// table (checked by a test).
pub fn block_group(name: &str) -> &'static str {
    let has = |part: &str| name.contains(part);
    let ends = |part: &str| name.ends_with(part);
    let starts = |part: &str| name.starts_with(part);
    let wood_kind = [
        "oak", "spruce", "birch", "jungle", "acacia", "mangrove", "pale_oak",
    ]
    .iter()
    .any(|kind| starts(kind));
    // Specific blocks and families first, the general stone last.
    match name {
        "ladder" => return "ladder",
        "anvil" | "chipped_anvil" | "damaged_anvil" => return "anvil",
        "scaffolding" => return "scaffolding",
        "cobweb" => return "cobweb",
        "slime_block" => return "slime_block",
        "honey_block" => return "honey_block",
        "sponge" => return "sponge",
        "wet_sponge" => return "wet_sponge",
        "snow" | "snow_block" => return "snow",
        "powder_snow" => return "powder_snow",
        "bone_block" => return "bone_block",
        "soul_sand" => return "soul_sand",
        "soul_soil" => return "soul_soil",
        "netherrack" => return "netherrack",
        "nether_wart_block" | "warped_wart_block" | "shroomlight_block" => return "wart_block",
        "shroomlight" => return "shroomlight",
        "nether_wart" => return "nether_wart",
        "ancient_debris" => return "ancient_debris",
        "netherite_block" => return "netherite_block",
        "gilded_blackstone" => return "gilded_blackstone",
        "lodestone" => return "lodestone",
        "calcite" => return "calcite",
        "tuff" | "tuff_slab" | "tuff_stairs" | "tuff_wall" => return "tuff",
        "polished_tuff" | "polished_tuff_slab" | "polished_tuff_stairs" | "polished_tuff_wall" => {
            return "polished_tuff";
        }
        "moss_block" => return "moss",
        "moss_carpet" | "pale_moss_carpet" => return "moss_carpet",
        "pale_moss_block" => return "moss",
        "mud" => return "mud",
        "packed_mud" => return "packed_mud",
        "rooted_dirt" => return "rooted_dirt",
        "hanging_roots" => return "hanging_roots",
        "spore_blossom" => return "spore_blossom",
        "azalea" | "potted_azalea_bush" => return "azalea",
        "flowering_azalea" | "potted_flowering_azalea_bush" => return "flowering_azalea",
        "big_dripleaf" | "big_dripleaf_stem" => return "big_dripleaf",
        "small_dripleaf" => return "small_dripleaf",
        "sculk" => return "sculk",
        "sculk_vein" => return "sculk_vein",
        "sculk_sensor" | "calibrated_sculk_sensor" => return "sculk_sensor",
        "sculk_catalyst" => return "sculk_catalyst",
        "sculk_shrieker" => return "sculk_shrieker",
        "pointed_dripstone" => return "pointed_dripstone",
        "dripstone_block" => return "dripstone_block",
        "amethyst_block" | "budding_amethyst" => return "amethyst_block",
        "amethyst_cluster" => return "amethyst_cluster",
        "large_amethyst_bud" => return "large_amethyst_bud",
        "medium_amethyst_bud" => return "medium_amethyst_bud",
        "small_amethyst_bud" => return "small_amethyst_bud",
        "sweet_berry_bush" => return "sweet_berry_bush",
        "vine" | "glow_lichen" => return "vine",
        "weeping_vines" | "weeping_vines_plant" => return "weeping_vines",
        "twisting_vines" | "twisting_vines_plant" => return "twisting_vines",
        "cave_vines" | "cave_vines_plant" => return "cave_vines",
        "nether_sprouts" => return "nether_sprouts",
        "frogspawn" => return "frogspawn",
        "mangrove_roots" => return "mangrove_roots",
        "muddy_mangrove_roots" => return "muddy_mangrove_roots",
        "spawner" => return "spawner",
        "trial_spawner" => return "trial_spawner",
        "vault" => return "vault",
        "decorated_pot" => return "decorated_pot",
        "chiseled_bookshelf" => return "chiseled_bookshelf",
        "pink_petals" | "wildflowers" => return "pink_petals",
        "cherry_leaves" => return "cherry_leaves",
        "cherry_sapling" => return "cherry_sapling",
        "bamboo" | "bamboo_sapling" => return "bamboo_sapling",
        "azalea_leaves" | "flowering_azalea_leaves" => return "azalea_leaves",
        "dried_ghast" => return "dried_ghast",
        "heavy_core" => return "heavy_core",
        "leaf_litter" => return "leaf_litter",
        "cactus_flower" => return "cactus_flower",
        "resin_block" => return "resin",
        "chain" | "iron_chain" => return "chain",
        "lantern" | "soul_lantern" => return "lantern",
        "copper_lantern" => return "lantern",
        "iron_block"
        | "gold_block"
        | "diamond_block"
        | "emerald_block"
        | "lapis_block"
        | "raw_iron_block"
        | "raw_gold_block"
        | "iron_bars"
        | "hopper"
        | "cauldron"
        | "rail"
        | "powered_rail"
        | "detector_rail"
        | "activator_rail"
        | "iron_door"
        | "iron_trapdoor"
        | "heavy_weighted_pressure_plate"
        | "light_weighted_pressure_plate"
        | "brewing_stand"
        | "bell"
        | "lightning_rod"
        | "beacon_base" => return "metal",
        "red_sand" | "sand" => return "sand",
        "suspicious_sand" => return "suspicious_sand",
        "suspicious_gravel" => return "suspicious_gravel",
        "gravel" | "dirt" | "coarse_dirt" | "podzol" | "farmland" | "dirt_path" | "clay"
        | "grass_path" => return "gravel",
        "grass_block" | "mycelium" | "hay_block" | "short_grass" | "tall_grass" | "fern"
        | "large_fern" | "tnt" => return "grass",
        "cactus" | "cake" | "carpet" => return "wool",
        "pumpkin" | "carved_pumpkin" | "jack_o_lantern" | "melon" => return "wood",
        _ => {}
    }
    if has("candle") && !has("cake") {
        return "candle";
    }
    if has("glass") || has("ice") || name == "sea_lantern" || name == "beacon" {
        return "glass";
    }
    if has("wool") || has("carpet") || has("banner") || has("bed") {
        return if has("bed") && !has("wool") {
            "wood"
        } else {
            "wool"
        };
    }
    if has("coral") {
        return if has("fan") || ends("coral") {
            "grass"
        } else {
            "coral_block"
        };
    }
    if has("nylium") {
        return "nylium";
    }
    if has("roots") && (starts("crimson") || starts("warped")) {
        return "roots";
    }
    if has("fungus") && !has("shelf") {
        return "fungus";
    }
    if ends("_stem") || ends("_hyphae") || ends("_stem_top") {
        return if starts("crimson") || starts("warped") {
            "stem"
        } else {
            "wood"
        };
    }
    if starts("crimson") || starts("warped") {
        return "nether_wood";
    }
    if starts("cherry") {
        return "cherry_wood";
    }
    if starts("bamboo") {
        return if has("mosaic") || has("planks") || has("block") || has("door") || has("fence") {
            "bamboo_wood"
        } else {
            "wood"
        };
    }
    if has("leaves") {
        return "grass";
    }
    if has("sapling")
        || has("flower")
        || has("tulip")
        || has("rose")
        || has("poppy")
        || has("dandelion")
        || has("orchid")
        || has("allium")
        || has("bluet")
        || has("daisy")
        || has("cornflower")
        || has("lily")
        || has("peony")
        || has("lilac")
        || has("sunflower")
        || has("torchflower")
        || has("pitcher")
        || has("wheat")
        || has("carrots")
        || has("potatoes")
        || has("beetroots")
        || has("stem")
        || has("seagrass")
        || has("kelp")
        || has("bush")
        || has("mushroom") && !has("block")
    {
        return "grass";
    }
    if has("copper") {
        return "copper";
    }
    if has("deepslate") {
        return if has("tile") {
            "deepslate_tiles"
        } else if has("brick") {
            "deepslate_bricks"
        } else if has("polished") || has("chiseled") {
            "polished_deepslate"
        } else {
            "deepslate"
        };
    }
    if has("mud_brick") {
        return "mud_bricks";
    }
    if has("nether_brick") || has("red_nether") {
        return "nether_bricks";
    }
    if has("basalt") {
        return "basalt";
    }
    if has("nether_gold_ore") {
        return "nether_gold_ore";
    }
    if has("quartz_ore") {
        return "nether_ore";
    }
    if has("netherrack") {
        return "netherrack";
    }
    if has("froglight") {
        return "froglight";
    }
    if has("sulfur") || has("cinnabar") || has("resin") {
        return if has("resin_brick") {
            "resin_bricks"
        } else {
            "stone"
        };
    }
    if wood_kind
        || has("planks")
        || has("log")
        || has("wood")
        || has("chest")
        || has("barrel")
        || has("bookshelf")
        || has("crafting_table")
        || has("door")
        || has("trapdoor")
        || has("fence")
        || has("sign")
        || has("button") && !has("stone") && !has("polished")
        || has("pressure_plate") && !has("stone") && !has("polished")
        || has("note_block")
        || has("jukebox")
        || has("lectern")
        || has("loom")
        || has("cartography")
        || has("fletching")
        || has("smithing_table")
        || has("composter")
        || has("beehive")
        || has("bee_nest")
        || has("shelf")
        || has("torch")
        || has("campfire")
        || has("daylight")
        || has("target")
        || has("boat")
        || has("trapped")
    {
        return "wood";
    }
    "stone"
}

/// The group that stands in for one that has no event for a verb in
/// vanilla's table (a nether wart has no step of its own).
fn stand_in(group: &str, verb: &str) -> &'static str {
    match (group, verb) {
        ("bamboo_sapling", _) => "bamboo",
        ("cactus_flower" | "sweet_berry_bush", _) => "grass",
        ("fungus", _) => "grass",
        ("large_amethyst_bud" | "medium_amethyst_bud" | "small_amethyst_bud", _) => {
            "amethyst_cluster"
        }
        ("twisting_vines", _) => "weeping_vines",
        ("nether_wart", _) => "wart_block",
        ("dried_ghast" | "resin", _) => "stone",
        _ => "stone",
    }
}

/// `block.<group>.<verb>` for a block's name: `break`, `step`, `place`,
/// `hit` or `fall`.
pub fn block_event(name: &str, verb: &str) -> String {
    let group = block_group(name);
    let gap = match group {
        "bamboo_sapling" => matches!(verb, "step" | "fall"),
        "cactus_flower" | "sweet_berry_bush" => matches!(verb, "step" | "hit" | "fall"),
        "fungus" => matches!(verb, "hit" | "fall"),
        "large_amethyst_bud" | "medium_amethyst_bud" | "small_amethyst_bud" => {
            matches!(verb, "step" | "hit" | "fall")
        }
        "twisting_vines" => true,
        "nether_wart" => true,
        "dried_ghast" | "resin" => verb == "hit",
        _ => false,
    };
    let group = if gap { stand_in(group, verb) } else { group };
    format!("block.{group}.{verb}")
}

/// The mob whose sounds a mob makes (cave spiders chitter like spiders).
fn mob_voice(name: &str) -> &str {
    match name {
        "cave_spider" => "spider",
        other => other,
    }
}

/// `entity.<mob>.<verb>` (`ambient`, `hurt`, `death`, `step`).
pub fn mob_event(name: &str, verb: &str) -> String {
    format!("entity.{}.{verb}", mob_voice(name))
}

/// The event a piece of armor makes when put on, by its material.
pub fn armor_equip(item: &str) -> &'static str {
    match item.split('_').next().unwrap_or_default() {
        "leather" => "item.armor.equip_leather",
        "chainmail" => "item.armor.equip_chain",
        "iron" => "item.armor.equip_iron",
        "golden" => "item.armor.equip_gold",
        "diamond" => "item.armor.equip_diamond",
        "netherite" => "item.armor.equip_netherite",
        "copper" => "item.armor.equip_copper",
        "turtle" => "item.armor.equip_turtle",
        "elytra" => "item.armor.equip_elytra",
        _ => "item.armor.equip_generic",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{audio::build_table, pack::Resources, world::block};

    fn merged() -> Option<crate::audio::SoundTable> {
        let assets = crate::assets::dir();
        if !assets
            .join("vanilla/assets/minecraft/sounds.json")
            .is_file()
        {
            return None;
        }
        Some(build_table(&Resources::load(&assets, Vec::new()).unwrap()))
    }

    #[test]
    fn blocks_fall_into_the_groups_a_player_expects() {
        for (name, group) in [
            ("stone", "stone"),
            ("cobblestone", "stone"),
            ("oak_planks", "wood"),
            ("dark_oak_log", "wood"),
            ("spruce_trapdoor", "wood"),
            ("crafting_table", "wood"),
            ("dirt", "gravel"),
            ("gravel", "gravel"),
            ("grass_block", "grass"),
            ("dandelion", "grass"),
            ("oak_leaves", "grass"),
            ("sand", "sand"),
            ("glass", "glass"),
            ("blue_stained_glass_pane", "glass"),
            ("ice", "glass"),
            ("white_wool", "wool"),
            ("red_carpet", "wool"),
            ("iron_block", "metal"),
            ("anvil", "anvil"),
            ("netherrack", "netherrack"),
            ("soul_sand", "soul_sand"),
            ("crimson_planks", "nether_wood"),
            ("cherry_planks", "cherry_wood"),
            ("deepslate_bricks", "deepslate_bricks"),
            ("copper_block", "copper"),
            ("obsidian", "stone"),
            ("snow_block", "snow"),
        ] {
            assert_eq!(block_group(name), group, "{name}");
        }
        assert_eq!(block_event("stone", "break"), "block.stone.break");
        assert_eq!(mob_event("cave_spider", "hurt"), "entity.spider.hurt");
        assert_eq!(armor_equip("iron_helmet"), "item.armor.equip_iron");
        assert_eq!(armor_equip("golden_boots"), "item.armor.equip_gold");
    }

    #[test]
    fn every_block_and_placeholder_has_all_five_sounds_in_the_merged_table() {
        let Some(table) = merged() else { return };
        let mut missing = Vec::new();
        for def in block::registry().iter() {
            for verb in ["break", "step", "place", "hit", "fall"] {
                let event = block_event(&def.name, verb);
                if !table.contains(&event) {
                    missing.push(format!("{} -> {event}", def.name));
                }
            }
        }
        assert!(missing.is_empty(), "{} missing: {missing:?}", missing.len());
    }
}

#[cfg(test)]
mod played {
    use std::{collections::BTreeSet, fs, path::Path};

    use super::*;
    use crate::{audio::build_table, pack::Resources};

    /// Every `audio::play...("event.name", ...)` literal in the source.
    fn literal_events(dir: &Path, found: &mut BTreeSet<String>) {
        for entry in fs::read_dir(dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                literal_events(&path, found);
            } else if path.extension().is_some_and(|e| e == "rs") {
                let text = fs::read_to_string(&path).unwrap();
                let mut rest = text.as_str();
                while let Some(i) = rest.find("audio::play") {
                    rest = &rest[i + "audio::play".len()..];
                    // The call's first argument, if it is a string literal.
                    let Some(open) = rest.find('(') else { break };
                    let args = rest[open + 1..].trim_start();
                    if let Some(quoted) = args.strip_prefix('"')
                        && let Some(end) = quoted.find('"')
                    {
                        let name = &quoted[..end];
                        if name.chars().all(|c| {
                            c.is_ascii_lowercase() || c.is_ascii_digit() || c == '.' || c == '_'
                        }) && name.contains('.')
                        {
                            found.insert(name.to_owned());
                        }
                    }
                }
            }
        }
    }

    /// Every event spelled out in a `match` arm or constant that is then
    /// handed to `audio::play` (door, button, note and hurt sounds).
    fn named_events(dir: &Path, found: &mut BTreeSet<String>) {
        for entry in fs::read_dir(dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                named_events(&path, found);
            } else if path.extension().is_some_and(|e| e == "rs") {
                // The table's own tests use made-up events.
                if path.ends_with("audio/table.rs") {
                    continue;
                }
                let text = fs::read_to_string(&path).unwrap();
                for line in text.lines() {
                    let line = line.trim();
                    if line.starts_with("//") {
                        continue;
                    }
                    for prefix in [
                        "\"block.",
                        "\"entity.",
                        "\"item.",
                        "\"ui.",
                        "\"ambient.",
                        "\"music",
                    ] {
                        let mut rest = line;
                        while let Some(i) = rest.find(prefix) {
                            let quoted = &rest[i + 1..];
                            if let Some(end) = quoted.find('"') {
                                let name = &quoted[..end];
                                if name.chars().all(|c| {
                                    c.is_ascii_lowercase()
                                        || c.is_ascii_digit()
                                        || c == '.'
                                        || c == '_'
                                }) && !name.ends_with('.')
                                {
                                    found.insert(name.to_owned());
                                }
                            }
                            rest = &rest[i + 1..];
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn every_sound_event_the_code_plays_is_in_the_merged_table() {
        let assets = crate::assets::dir();
        if !assets
            .join("vanilla/assets/minecraft/sounds.json")
            .is_file()
        {
            return;
        }
        let table = build_table(&Resources::load(&assets, Vec::new()).unwrap());
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut events = BTreeSet::new();
        literal_events(&src, &mut events);
        named_events(&src, &mut events);
        // Not events: partial names used to build others, and test strings.
        events.retain(|e| e.matches('.').count() >= 2 && !e.ends_with('.'));
        assert!(events.len() > 100, "found only {} events", events.len());
        let missing: Vec<&String> = events.iter().filter(|e| !table.contains(e)).collect();
        assert!(missing.is_empty(), "not in the sound table: {missing:?}");
    }

    #[test]
    fn every_mob_has_a_hurt_and_death_sound_and_most_an_ambient_one() {
        let assets = crate::assets::dir();
        if !assets
            .join("vanilla/assets/minecraft/sounds.json")
            .is_file()
        {
            return;
        }
        let table = build_table(&Resources::load(&assets, Vec::new()).unwrap());
        let mobs = crate::entity::mobs::MobRegistry::load(&assets.join("data/mobs.ron")).unwrap();
        let mut quiet = Vec::new();
        for name in mobs.names() {
            for verb in ["hurt", "death"] {
                let event = mob_event(name, verb);
                if !table.contains(&event) {
                    quiet.push(event);
                }
            }
        }
        assert!(quiet.is_empty(), "no sound for {quiet:?}");
    }
}
