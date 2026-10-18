//! Mining: how fast a block breaks with a given tool, whether it drops
//! anything, and what, following 1.0.0's `Block.blockStrength`,
//! `canHarvestBlock` and `idDropped`/`quantityDropped`.

use serde::Deserialize;

use super::{
    block::{BlockDef, BlockState, Condition},
    props::{Prop, slab_type},
};
use crate::inventory::{
    item::{self, ItemId, Tier, Tool, ToolKind},
    stack::ItemStack,
};

/// The tool a block needs to drop anything (1.0.0's rock, iron, snow and
/// cobweb materials).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Harvest {
    pub tools: Vec<ToolKind>,
    /// Lowest tier that works (by harvest level, so gold counts as wood).
    #[serde(default = "lowest_tier")]
    pub tier: Tier,
}

fn lowest_tier() -> Tier {
    Tier::Wood
}

/// What a block drops when harvested.
#[derive(Debug, Default, Clone, PartialEq, Deserialize)]
pub enum Drops {
    /// Its item (two from a double slab).
    #[default]
    Itself,
    Nothing,
    /// Every drop in the list that rolls successfully.
    All(Vec<Drop>),
    /// The first drop in the list that rolls successfully.
    OneOf(Vec<Drop>),
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Drop {
    pub item: ItemId,
    /// Between `min` and `max` items (`max` defaults to `min`).
    #[serde(default = "one")]
    pub min: u8,
    #[serde(default)]
    pub max: Option<u8>,
    #[serde(default = "always")]
    pub chance: f32,
    /// Only from states meeting this condition (`age=7` for ripe wheat).
    #[serde(default)]
    pub when: String,
    /// `when`, parsed when the registry loads.
    #[serde(skip)]
    pub condition: Condition,
    /// Only when broken with this tool (leaves with shears).
    #[serde(default)]
    pub tool: Option<ToolKind>,
    /// Never when broken with this tool.
    #[serde(default)]
    pub not_tool: Option<ToolKind>,
}

fn one() -> u8 {
    1
}

fn always() -> f32 {
    1.0
}

impl Drop {
    fn roll(
        &self,
        state: BlockState,
        tool: Option<ToolKind>,
        random: &mut impl FnMut() -> f32,
    ) -> Option<ItemStack> {
        if !state.matches(&self.condition)
            || self.tool.is_some_and(|needed| tool != Some(needed))
            || self.not_tool.is_some_and(|banned| tool == Some(banned))
            || random() >= self.chance
        {
            return None;
        }
        let max = self.max.unwrap_or(self.min).max(self.min);
        let extra = (random() * (max - self.min + 1) as f32) as u8;
        let count = (self.min + extra).min(max);
        (count > 0).then(|| ItemStack::with_damage(self.item, count, 0))
    }
}

/// Whether breaking `block` with `tool` drops anything.
pub fn can_harvest(block: &BlockDef, tool: Option<&Tool>) -> bool {
    let Some(harvest) = &block.harvest else {
        return true;
    };
    tool.is_some_and(|tool| {
        harvest.tools.contains(&tool.kind)
            && tool.tier.map_or(0, Tier::level) >= harvest.tier.level()
    })
}

/// How fast `tool` (or a bare hand) mines `block`, before penalties.
fn tool_speed(block: &BlockDef, tool: Option<&Tool>) -> f32 {
    let Some(tool) = tool else {
        return 1.0;
    };
    if let Some(&speed) = tool.speeds.get(&block.id) {
        return speed;
    }
    match tool.tier {
        Some(tier) if block.tool == Some(tool.kind) => tier.efficiency(),
        _ => tool.base_speed,
    }
}

/// Fraction of `block` broken per tick (1.0.0's `Block.blockStrength`); it
/// breaks once the total reaches 1. Mining is 5× slower in water and
/// again off the ground, unless the block can't be harvested anyway.
pub fn progress_per_tick(
    block: &BlockDef,
    tool: Option<&Tool>,
    on_ground: bool,
    in_water: bool,
) -> f32 {
    if block.hardness < 0.0 {
        return 0.0;
    }
    if !can_harvest(block, tool) {
        return 1.0 / block.hardness / 100.0;
    }
    let mut speed = tool_speed(block, tool);
    if in_water {
        speed /= 5.0;
    }
    if !on_ground {
        speed /= 5.0;
    }
    speed / block.hardness / 30.0
}

/// What breaking `state` drops, assuming it was harvested. `random`
/// returns numbers in 0..1.
pub fn drops(
    state: BlockState,
    tool: Option<ToolKind>,
    random: &mut impl FnMut() -> f32,
) -> Vec<ItemStack> {
    let block = state.def();
    match &block.drops {
        Drops::Itself => {
            let Some(item) = item::registry().of_block(block.id) else {
                return Vec::new();
            };
            let double =
                state.has(Prop::SlabType) && state.get(Prop::SlabType) == slab_type::DOUBLE;
            vec![ItemStack::with_damage(item, if double { 2 } else { 1 }, 0)]
        }
        Drops::Nothing => Vec::new(),
        Drops::All(list) => list
            .iter()
            .filter_map(|drop| drop.roll(state, tool, random))
            .collect(),
        Drops::OneOf(list) => list
            .iter()
            .find_map(|drop| drop.roll(state, tool, random))
            .into_iter()
            .collect(),
    }
}

impl Drops {
    /// The drops in the list, for parsing their conditions.
    pub fn list_mut(&mut self) -> &mut [Drop] {
        match self {
            Drops::All(list) | Drops::OneOf(list) => list,
            Drops::Itself | Drops::Nothing => &mut [],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        inventory::{items, stack::Items},
        world::{block::BlockId, blocks},
    };

    fn items() -> Items {
        Items::new()
    }

    /// Ticks to break `block` with item `tool` (`None` = bare hand), standing on
    /// dry ground: 1 / progress per tick, rounded up. (The game sums the
    /// progress in floats like 1.0.0, which sometimes takes one tick more.)
    fn ticks(items: &Items, block: BlockId, tool: Option<ItemId>) -> u32 {
        let def = items.blocks.def(block);
        let per_tick = progress_per_tick(def, tool.and_then(|t| items.tool(t)), true, false);
        (1.0 / per_tick).ceil() as u32
    }

    #[test]
    fn mining_times_match_1_0_0() {
        let items = items();
        // Seconds from the Minecraft wiki's 1.0-era breaking time table.
        assert_eq!(
            ticks(&items, blocks::STONE, None),
            150,
            "stone by hand: 7.5 s"
        );
        assert_eq!(
            ticks(&items, blocks::STONE, Some(items::WOODEN_PICKAXE)),
            23,
            "stone, wooden pickaxe: 1.15 s"
        );
        assert_eq!(
            ticks(&items, blocks::STONE, Some(items::DIAMOND_PICKAXE)),
            6,
            "stone, diamond pickaxe: 0.3 s"
        );
        assert_eq!(
            ticks(&items, blocks::STONE, Some(items::GOLDEN_PICKAXE)),
            4,
            "stone, golden pickaxe: 0.2 s"
        );
        assert_eq!(
            ticks(&items, blocks::DIRT, None),
            15,
            "dirt by hand: 0.75 s"
        );
        assert_eq!(
            ticks(&items, blocks::DIRT, Some(items::WOODEN_SHOVEL)),
            8,
            "dirt, wooden shovel: 0.4 s"
        );
        assert_eq!(ticks(&items, blocks::OAK_LOG, None), 60, "log by hand: 3 s");
        assert_eq!(
            ticks(&items, blocks::OAK_LOG, Some(items::STONE_AXE)),
            15,
            "log, stone axe: 0.75 s"
        );
        assert_eq!(
            ticks(&items, blocks::OBSIDIAN, Some(items::DIAMOND_PICKAXE)),
            188,
            "obsidian, diamond pickaxe: 9.4 s"
        );
        assert_eq!(
            ticks(&items, blocks::OBSIDIAN, Some(items::IRON_PICKAXE)),
            5000,
            "obsidian, iron pickaxe: can't harvest, 250 s"
        );
        assert_eq!(
            ticks(&items, blocks::COBWEB, Some(items::IRON_SWORD)),
            8,
            "cobweb, sword: 0.4 s"
        );
        assert_eq!(
            ticks(&items, blocks::OAK_LEAVES, Some(items::SHEARS)),
            1,
            "leaves, shears: instant"
        );
        // Swords are a little faster on everything.
        assert_eq!(ticks(&items, blocks::DIRT, Some(items::IRON_SWORD)), 10);
        // Instant blocks and unbreakable ones.
        assert!(progress_per_tick(items.blocks.def(blocks::SHORT_GRASS), None, true, false) >= 1.0);
        assert_eq!(
            progress_per_tick(items.blocks.def(blocks::BEDROCK), None, true, false),
            0.0
        );
    }

    #[test]
    fn water_and_jumping_slow_mining_five_times_each() {
        let items = items();
        let stone = items.blocks.def(blocks::STONE);
        let pickaxe = items.tool(items::WOODEN_PICKAXE);
        let dry = progress_per_tick(stone, pickaxe, true, false);
        let close = |a: f32, b: f32| (a - b).abs() < 1e-9;
        assert!(close(
            progress_per_tick(stone, pickaxe, false, false),
            dry / 5.0
        ));
        assert!(close(
            progress_per_tick(stone, pickaxe, false, true),
            dry / 25.0
        ));
        // Without the right tool there's no penalty, as in 1.0.0.
        let by_hand = progress_per_tick(stone, None, true, false);
        assert_eq!(progress_per_tick(stone, None, false, true), by_hand);
    }

    #[test]
    fn harvesting_needs_the_right_tool_and_tier() {
        let items = items();
        let harvests = |block: BlockId, tool: Option<ItemId>| {
            can_harvest(items.blocks.def(block), tool.and_then(|t| items.tool(t)))
        };
        assert!(
            !harvests(blocks::STONE, None),
            "stone by hand drops nothing"
        );
        assert!(harvests(blocks::STONE, Some(items::WOODEN_PICKAXE)));
        assert!(
            !harvests(blocks::STONE, Some(items::WOODEN_SHOVEL)),
            "not with a shovel"
        );
        assert!(
            !harvests(blocks::IRON_ORE, Some(items::WOODEN_PICKAXE)),
            "iron ore needs stone"
        );
        assert!(harvests(blocks::IRON_ORE, Some(items::STONE_PICKAXE)));
        assert!(
            !harvests(blocks::DIAMOND_ORE, Some(items::STONE_PICKAXE)),
            "diamond ore needs iron"
        );
        assert!(harvests(blocks::DIAMOND_ORE, Some(items::IRON_PICKAXE)));
        assert!(
            !harvests(blocks::GOLD_ORE, Some(items::GOLDEN_PICKAXE)),
            "gold mines like wood"
        );
        assert!(
            !harvests(blocks::OBSIDIAN, Some(items::IRON_PICKAXE)),
            "obsidian needs diamond"
        );
        assert!(harvests(blocks::OBSIDIAN, Some(items::DIAMOND_PICKAXE)));
        assert!(harvests(blocks::DIRT, None), "dirt drops by hand");
        assert!(
            !harvests(blocks::SNOW_BLOCK, None)
                && harvests(blocks::SNOW_BLOCK, Some(items::WOODEN_SHOVEL)),
            "snow needs a shovel"
        );
        assert!(
            !harvests(blocks::COBWEB, None)
                && harvests(blocks::COBWEB, Some(items::WOODEN_SWORD))
                && harvests(blocks::COBWEB, Some(items::SHEARS))
        );
    }

    /// Drops with a fixed sequence of "random" numbers.
    fn drops_with(state: BlockState, tool: Option<ToolKind>, rolls: &[f32]) -> Vec<(ItemId, u8)> {
        let mut rolls = rolls.iter().copied().cycle();
        drops(state, tool, &mut || rolls.next().unwrap())
            .iter()
            .map(|s| (s.id, s.count))
            .collect()
    }

    #[test]
    fn drops_follow_1_0_0() {
        let d = |block, rolls: &[f32]| drops_with(BlockState::of(block), None, rolls);
        assert_eq!(
            d(blocks::STONE, &[0.0]),
            [(items::COBBLESTONE, 1)],
            "stone → cobblestone"
        );
        assert_eq!(
            d(blocks::GRASS_BLOCK, &[0.0]),
            [(items::DIRT, 1)],
            "grass → dirt"
        );
        assert_eq!(d(blocks::DIRT, &[0.0]), [(items::DIRT, 1)], "dirt → itself");
        assert_eq!(d(blocks::RED_WOOL, &[0.0]), [(items::RED_WOOL, 1)]);
        assert_eq!(d(blocks::BIRCH_LOG, &[0.0]), [(items::BIRCH_LOG, 1)]);
        assert_eq!(d(blocks::GLASS, &[0.0]), [], "glass breaks into nothing");
        assert_eq!(
            d(blocks::COAL_ORE, &[0.0]),
            [(items::COAL, 1)],
            "coal ore → coal"
        );
        assert_eq!(
            d(blocks::LAPIS_ORE, &[0.0, 0.99]),
            [(items::LAPIS_LAZULI, 8)],
            "lapis: 4–8"
        );
        assert_eq!(
            d(blocks::LAPIS_ORE, &[0.0, 0.0]),
            [(items::LAPIS_LAZULI, 4)]
        );
        assert_eq!(
            d(blocks::REDSTONE_ORE, &[0.0, 0.99]),
            [(items::REDSTONE, 5)],
            "redstone: 4–5"
        );
        assert_eq!(
            d(blocks::GRAVEL, &[0.05]),
            [(items::FLINT, 1)],
            "gravel: flint 1 in 10"
        );
        assert_eq!(
            d(blocks::GRAVEL, &[0.5]),
            [(items::GRAVEL, 1)],
            "otherwise gravel"
        );
        assert_eq!(
            d(blocks::SPRUCE_LEAVES, &[0.01]),
            [(items::SPRUCE_SAPLING, 1)],
            "spruce leaves → spruce sapling"
        );
        assert_eq!(d(blocks::SPRUCE_LEAVES, &[0.5]), [], "only 1 in 20");
        let wheat = |age| BlockState::of(blocks::WHEAT).with_number(Prop::Age7, age);
        assert_eq!(
            drops_with(wheat(7), None, &[0.0]),
            [(items::WHEAT, 1), (items::WHEAT_SEEDS, 1)],
            "ripe wheat"
        );
        assert_eq!(
            drops_with(wheat(3), None, &[0.0]),
            [(items::WHEAT_SEEDS, 1)],
            "unripe: just seeds"
        );
        let double = BlockState::of(blocks::BRICK_SLAB).with(Prop::SlabType, slab_type::DOUBLE);
        assert_eq!(
            drops_with(double, None, &[0.0]),
            [(items::BRICK_SLAB, 2)],
            "double slab → two slabs"
        );
        let sheared = drops_with(
            BlockState::of(blocks::BIRCH_LEAVES),
            Some(ToolKind::Shears),
            &[0.9],
        );
        assert_eq!(
            sheared,
            [(items::BIRCH_LEAVES, 1)],
            "shears keep the leaves"
        );
        assert_eq!(d(blocks::WALL_TORCH, &[0.0]), [(items::TORCH, 1)]);
        assert_eq!(d(blocks::WATER, &[0.0]), []);
    }
}
