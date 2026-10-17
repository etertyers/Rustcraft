//! Mining: how fast a block breaks with a given tool, whether it drops
//! anything, and what, following 1.0.0's `Block.blockStrength`,
//! `canHarvestBlock` and `idDropped`/`quantityDropped`.

use serde::Deserialize;

use super::block::BlockDef;
use crate::inventory::{
    item::{ItemId, Tier, Tool, ToolKind},
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
    /// One of itself, keeping its data value if it has variants.
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
    #[serde(default)]
    pub data: u16,
    /// Use the block's data value instead of `data` (leaves → sapling type).
    #[serde(default)]
    pub keep_data: bool,
    /// Between `min` and `max` items (`max` defaults to `min`).
    #[serde(default = "one")]
    pub min: u8,
    #[serde(default)]
    pub max: Option<u8>,
    #[serde(default = "always")]
    pub chance: f32,
    /// Only from blocks with at least this data value (ripe crops).
    #[serde(default)]
    pub min_data: u8,
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
        data: u8,
        tool: Option<ToolKind>,
        random: &mut impl FnMut() -> f32,
    ) -> Option<ItemStack> {
        if data < self.min_data
            || self.tool.is_some_and(|needed| tool != Some(needed))
            || self.not_tool.is_some_and(|banned| tool == Some(banned))
            || random() >= self.chance
        {
            return None;
        }
        let max = self.max.unwrap_or(self.min).max(self.min);
        let extra = (random() * (max - self.min + 1) as f32) as u8;
        let count = (self.min + extra).min(max);
        let data = if self.keep_data {
            data as u16
        } else {
            self.data
        };
        (count > 0).then(|| ItemStack::with_damage(self.item, count, data))
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

/// What breaking `block` (with data value `data`) drops, assuming it was
/// harvested. `random` returns numbers in 0..1.
pub fn drops(
    block: &BlockDef,
    data: u8,
    tool: Option<ToolKind>,
    random: &mut impl FnMut() -> f32,
) -> Vec<ItemStack> {
    match &block.drops {
        Drops::Itself => {
            let data = if block.variants.is_empty() { 0 } else { data };
            vec![ItemStack::with_damage(block.id, 1, data as u16)]
        }
        Drops::Nothing => Vec::new(),
        Drops::All(list) => list
            .iter()
            .filter_map(|drop| drop.roll(data, tool, random))
            .collect(),
        Drops::OneOf(list) => list
            .iter()
            .find_map(|drop| drop.roll(data, tool, random))
            .into_iter()
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        inventory::{item::ItemRegistry, stack::Items},
        world::block::BlockRegistry,
    };

    fn items() -> Items {
        let assets = crate::assets::dir();
        Items::new(
            BlockRegistry::load(&assets.join("data/blocks.ron"))
                .unwrap()
                .into(),
            ItemRegistry::load(&assets.join("data/items.ron"))
                .unwrap()
                .into(),
        )
    }

    /// Ticks to break `block` with item `tool` (0 = bare hand), standing on
    /// dry ground: 1 / progress per tick, rounded up. (The game sums the
    /// progress in floats like 1.0.0, which sometimes takes one tick more.)
    fn ticks(items: &Items, block: u16, tool: u16) -> u32 {
        let def = items.blocks.get(block).unwrap();
        let per_tick = progress_per_tick(def, items.tool(tool), true, false);
        (1.0 / per_tick).ceil() as u32
    }

    #[test]
    fn mining_times_match_1_0_0() {
        let items = items();
        // Seconds from the Minecraft wiki's 1.0-era breaking time table.
        assert_eq!(ticks(&items, 1, 0), 150, "stone by hand: 7.5 s");
        assert_eq!(ticks(&items, 1, 270), 23, "stone, wooden pickaxe: 1.15 s");
        assert_eq!(ticks(&items, 1, 278), 6, "stone, diamond pickaxe: 0.3 s");
        assert_eq!(ticks(&items, 1, 285), 4, "stone, golden pickaxe: 0.2 s");
        assert_eq!(ticks(&items, 3, 0), 15, "dirt by hand: 0.75 s");
        assert_eq!(ticks(&items, 3, 269), 8, "dirt, wooden shovel: 0.4 s");
        assert_eq!(ticks(&items, 17, 0), 60, "log by hand: 3 s");
        assert_eq!(ticks(&items, 17, 275), 15, "log, stone axe: 0.75 s");
        assert_eq!(
            ticks(&items, 49, 278),
            188,
            "obsidian, diamond pickaxe: 9.4 s"
        );
        assert_eq!(
            ticks(&items, 49, 257),
            5000,
            "obsidian, iron pickaxe: can't harvest, 250 s"
        );
        assert_eq!(ticks(&items, 30, 267), 8, "cobweb, sword: 0.4 s");
        assert_eq!(ticks(&items, 18, 359), 1, "leaves, shears: instant");
        // Swords are a little faster on everything.
        assert_eq!(ticks(&items, 3, 267), 10);
        // Instant blocks and unbreakable ones.
        assert!(progress_per_tick(items.blocks.get(31).unwrap(), None, true, false) >= 1.0);
        assert_eq!(
            progress_per_tick(items.blocks.get(7).unwrap(), None, true, false),
            0.0
        );
    }

    #[test]
    fn water_and_jumping_slow_mining_five_times_each() {
        let items = items();
        let stone = items.blocks.get(1).unwrap();
        let pickaxe = items.tool(270);
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
        let harvests =
            |block: u16, tool: u16| can_harvest(items.blocks.get(block).unwrap(), items.tool(tool));
        assert!(!harvests(1, 0), "stone by hand drops nothing");
        assert!(harvests(1, 270));
        assert!(!harvests(1, 269), "not with a shovel");
        assert!(!harvests(15, 270), "iron ore needs stone");
        assert!(harvests(15, 274));
        assert!(!harvests(56, 274), "diamond ore needs iron");
        assert!(harvests(56, 257));
        assert!(!harvests(14, 285), "gold mines like wood");
        assert!(!harvests(49, 257), "obsidian needs diamond");
        assert!(harvests(49, 278));
        assert!(harvests(3, 0), "dirt drops by hand");
        assert!(!harvests(80, 0) && harvests(80, 269), "snow needs a shovel");
        assert!(!harvests(30, 0) && harvests(30, 268) && harvests(30, 359));
    }

    /// Drops with a fixed sequence of "random" numbers.
    fn drops_with(
        items: &Items,
        block: u16,
        data: u8,
        tool: Option<ToolKind>,
        rolls: &[f32],
    ) -> Vec<(u16, u8, u16)> {
        let mut rolls = rolls.iter().copied().cycle();
        drops(items.blocks.get(block).unwrap(), data, tool, &mut || {
            rolls.next().unwrap()
        })
        .iter()
        .map(|s| (s.id, s.count, s.damage))
        .collect()
    }

    #[test]
    fn drops_follow_1_0_0() {
        let items = items();
        let d = |block, data, rolls: &[f32]| drops_with(&items, block, data, None, rolls);
        assert_eq!(d(1, 0, &[0.0]), [(4, 1, 0)], "stone → cobblestone");
        assert_eq!(d(2, 0, &[0.0]), [(3, 1, 0)], "grass → dirt");
        assert_eq!(d(3, 0, &[0.0]), [(3, 1, 0)], "dirt → itself");
        assert_eq!(d(35, 14, &[0.0]), [(35, 1, 14)], "wool keeps its color");
        assert_eq!(d(17, 2, &[0.0]), [(17, 1, 2)], "birch log stays birch");
        assert_eq!(d(20, 0, &[0.0]), [], "glass breaks into nothing");
        assert_eq!(d(16, 0, &[0.0]), [(263, 1, 0)], "coal ore → coal");
        assert_eq!(
            d(21, 0, &[0.0, 0.99]),
            [(351, 8, 4)],
            "lapis: 4–8 lapis dye"
        );
        assert_eq!(d(21, 0, &[0.0, 0.0]), [(351, 4, 4)]);
        assert_eq!(d(73, 0, &[0.0, 0.99]), [(331, 5, 0)], "redstone: 4–5");
        assert_eq!(d(13, 0, &[0.05]), [(318, 1, 0)], "gravel: flint 1 in 10");
        assert_eq!(d(13, 0, &[0.5]), [(13, 1, 0)], "otherwise gravel");
        assert_eq!(
            d(18, 1, &[0.01]),
            [(6, 1, 1)],
            "spruce leaves → spruce sapling"
        );
        assert_eq!(d(18, 1, &[0.5]), [], "only 1 in 20");
        assert_eq!(d(59, 7, &[0.0]), [(296, 1, 0), (295, 1, 0)], "ripe wheat");
        assert_eq!(d(59, 3, &[0.0]), [(295, 1, 0)], "unripe wheat: just seeds");
        assert_eq!(d(43, 2, &[0.0]), [(44, 2, 2)], "double slab → two slabs");
        let sheared = drops_with(&items, 18, 2, Some(ToolKind::Shears), &[0.9]);
        assert_eq!(sheared, [(18, 1, 2)], "shears keep the leaves");
    }
}
