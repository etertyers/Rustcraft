//! Farming (M40): crops growing on random ticks at vanilla's rates,
//! farmland, hoes, bone meal and the composter. Everything runs on the
//! server and reaches clients as block edits.

use glam::{DVec3, IVec3};

use super::Game;
use crate::{
    config::farming as cfg,
    entity::{item::ItemEntity, particle::Particle},
    inventory::{
        item::{ItemId, ToolKind},
        items,
    },
    player::raycast::Hit,
    world::{
        block::{BlockId, BlockState},
        blocks,
        props::Prop,
    },
};

const SIDES: [IVec3; 4] = [IVec3::X, IVec3::NEG_X, IVec3::Z, IVec3::NEG_Z];

/// Vanilla's `CropBlock.getGrowthSpeed`: 1, plus each farmland under or
/// beside the crop (3 if wet, a quarter of that for the eight around),
/// halved if the same crop stands in both directions along a row or on
/// a diagonal. A crop then grows when `random(25 / speed + 1) == 0`.
pub fn growth_speed(get: &impl Fn(IVec3) -> Option<BlockState>, at: IVec3, crop: BlockId) -> f32 {
    let below = at - IVec3::Y;
    let mut speed = 1.0;
    for dx in -1..=1 {
        for dz in -1..=1 {
            let mut g = 0.0;
            if let Some(b) = get(below + IVec3::new(dx, 0, dz))
                && b.is(blocks::FARMLAND)
            {
                g = if b.number(Prop::Moisture) > 0 {
                    3.0
                } else {
                    1.0
                };
            }
            if dx != 0 || dz != 0 {
                g /= 4.0;
            }
            speed += g;
        }
    }
    let same = |p: IVec3| get(p).is_some_and(|b| b.is(crop));
    let row_x = same(at + IVec3::X) || same(at - IVec3::X);
    let row_z = same(at + IVec3::Z) || same(at - IVec3::Z);
    let diagonal = [(1, 1), (1, -1), (-1, 1), (-1, -1)]
        .into_iter()
        .any(|(x, z)| same(at + IVec3::new(x, 0, z)));
    if (row_x && row_z) || diagonal {
        speed /= 2.0;
    }
    speed
}

/// How likely an item is to raise a composter a level (vanilla's table).
pub fn compost_chance(name: &str) -> Option<f32> {
    let n = crate::world::block::strip_namespace(name);
    let leaves = n.ends_with("_leaves");
    let sapling = n.ends_with("_sapling") || n == "mangrove_propagule";
    let flower = matches!(
        n,
        "dandelion"
            | "poppy"
            | "blue_orchid"
            | "allium"
            | "azure_bluet"
            | "red_tulip"
            | "orange_tulip"
            | "white_tulip"
            | "pink_tulip"
            | "oxeye_daisy"
            | "cornflower"
            | "lily_of_the_valley"
            | "sunflower"
            | "lilac"
            | "rose_bush"
            | "peony"
            | "wither_rose"
            | "torchflower"
            | "pink_petals"
            | "spore_blossom"
    );
    Some(match n {
        _ if leaves || sapling => 0.3,
        "beetroot_seeds" | "short_grass" | "grass" | "kelp" | "melon_seeds" | "pumpkin_seeds"
        | "sweet_berries" | "glow_berries" | "seagrass" | "wheat_seeds" | "moss_carpet"
        | "hanging_roots" => 0.3,
        "cactus" | "dried_kelp_block" | "glow_lichen" | "melon_slice" | "nether_sprouts"
        | "sugar_cane" | "tall_grass" | "twisting_vines" | "vine" | "weeping_vines" => 0.5,
        "apple" | "beetroot" | "carrot" | "cocoa_beans" | "fern" | "large_fern" | "lily_pad"
        | "melon" | "moss_block" | "brown_mushroom" | "red_mushroom" | "nether_wart" | "potato"
        | "pumpkin" | "carved_pumpkin" | "sea_pickle" | "shroomlight" | "crimson_fungus"
        | "warped_fungus" | "crimson_roots" | "warped_roots" | "wheat" | "bamboo" => 0.65,
        _ if flower => 0.65,
        "baked_potato"
        | "bread"
        | "cookie"
        | "hay_block"
        | "brown_mushroom_block"
        | "red_mushroom_block"
        | "mushroom_stem"
        | "nether_wart_block"
        | "warped_wart_block"
        | "flowering_azalea" => 0.85,
        "cake" | "pumpkin_pie" => 1.0,
        _ => return None,
    })
}

fn named(name: &str) -> Option<BlockId> {
    blocks::by_name(name)
}

/// A tree's shape for a sapling: (offset, is log). Oak and birch are
/// 4–6 tall, spruce is a cone, jungle is tall, acacia leans, dark oak
/// is a thick trunk (needs four saplings).
fn tree_shape(species: &str, rng: &mut crate::world::worldgen::rng::Rng) -> Vec<(IVec3, bool)> {
    let mut out = Vec::new();
    let blob = |out: &mut Vec<(IVec3, bool)>, y: i32, radius: i32, corners: bool| {
        for dx in -radius..=radius {
            for dz in -radius..=radius {
                if !corners && dx.abs() == radius && dz.abs() == radius && radius > 1 {
                    continue;
                }
                out.push((IVec3::new(dx, y, dz), false));
            }
        }
    };
    match species {
        "spruce" => {
            let h = 6 + rng.below(3) as i32;
            for y in 0..h {
                out.push((IVec3::new(0, y, 0), true));
            }
            for (i, y) in (2..=h).rev().enumerate() {
                blob(&mut out, y, (i as i32 / 2).min(2), false);
            }
        }
        "jungle" => {
            let h = 8 + rng.below(5) as i32;
            for y in 0..h {
                out.push((IVec3::new(0, y, 0), true));
            }
            for y in h - 3..=h {
                blob(&mut out, y, if y == h { 1 } else { 2 }, false);
            }
        }
        "acacia" => {
            let h = 5 + rng.below(2) as i32;
            for y in 0..h {
                out.push((IVec3::new(0, y, 0), true));
            }
            out.push((IVec3::new(1, h, 0), true));
            for y in h..=h + 1 {
                for dx in -1..=3 {
                    for dz in -2..=2 {
                        if !(dx == 1 && dz == 0 && y == h) {
                            out.push((IVec3::new(dx, y, dz), false));
                        }
                    }
                }
            }
        }
        _ => {
            let h = 4 + rng.below(3) as i32;
            for y in 0..h {
                out.push((IVec3::new(0, y, 0), true));
            }
            blob(&mut out, h - 2, 2, false);
            blob(&mut out, h - 1, 2, false);
            blob(&mut out, h, 1, true);
            blob(&mut out, h + 1, 1, false);
        }
    }
    // Leaves never replace logs of the same tree.
    let logs: Vec<IVec3> = out
        .iter()
        .filter(|(_, log)| *log)
        .map(|(p, _)| *p)
        .collect();
    out.retain(|(p, log)| *log || !logs.contains(p));
    out
}

impl Game {
    /// A random tick for a block that grows. Returns whether it was one.
    pub(super) fn farm_random_tick(&mut self, at: IVec3, state: BlockState) -> bool {
        let name = state.def().name.as_str();
        match name {
            "wheat" | "carrots" | "potatoes" => self.grow_crop(at, state, Prop::Age7, 7, 1),
            "beetroots" => self.grow_crop(at, state, Prop::Age3, 3, 3),
            "melon_stem" | "pumpkin_stem" => self.grow_stem(at, state),
            "sugar_cane" | "cactus" => self.grow_stalk(at, state),
            "bamboo" => self.grow_bamboo(at),
            "sweet_berry_bush" => {
                if self.crop_light(at) && self.rng.below(5) == 0 {
                    self.age_up(at, state, Prop::Age3, 3, 1);
                }
            }
            "cocoa" => {
                if self.rng.below(5) == 0 {
                    self.age_up(at, state, Prop::Age2, 2, 1);
                }
            }
            "nether_wart" => {
                if self.rng.below(10) == 0 {
                    self.age_up(at, state, Prop::Age3, 3, 1);
                }
            }
            "kelp" => {
                if self.rng.below(100) < 14 {
                    self.grow_kelp(at, state);
                }
            }
            "farmland" => self.tick_farmland(at, state),
            "red_mushroom" | "brown_mushroom" => self.spread_mushroom(at, state),
            n if n.ends_with("_sapling") => self.tick_sapling(at, state),
            _ => return false,
        }
        true
    }

    /// Light a crop at `at` gets: the greater of block light and sky light
    /// less the dark of the hour.
    fn crop_light(&self, at: IVec3) -> bool {
        let Some((sky, block)) = self.chunks.light_at(at.x, at.y, at.z) else {
            return false;
        };
        let dark = crate::world::daytime::sky_light_subtracted(self.celestial_angle());
        sky.saturating_sub(dark).max(block) >= cfg::CROP_LIGHT
    }

    fn age_up(&mut self, at: IVec3, state: BlockState, prop: Prop, max: u8, by: u8) -> bool {
        let age = state.number(prop);
        if age >= max {
            return false;
        }
        self.edit(&[(at, state.with_number(prop, (age + by).min(max)))]);
        true
    }

    fn grow_crop(&mut self, at: IVec3, state: BlockState, prop: Prop, max: u8, one_in: u32) {
        if state.number(prop) >= max || !self.crop_light(at) {
            return;
        }
        let world = self.chunks.world();
        let speed = growth_speed(&|p| world.block(p.x, p.y, p.z), at, state.id());
        if self.rng.below((25.0 / speed) as u32 + 1) == 0 && self.rng.below(one_in) == 0 {
            self.age_up(at, state, prop, max, 1);
        }
    }

    /// Stems grow like crops; ripe, they put their fruit on a free
    /// dirt, grass or farmland block beside them, and start over.
    fn grow_stem(&mut self, at: IVec3, state: BlockState) {
        if !self.crop_light(at) {
            return;
        }
        let age = state.number(Prop::Age7);
        if age < 7 {
            self.grow_crop(at, state, Prop::Age7, 7, 1);
            return;
        }
        if self.rng.below(4) != 0 {
            return;
        }
        let fruit = if state.is(blocks::MELON_STEM) {
            "melon"
        } else {
            "pumpkin"
        };
        let Some(fruit) = named(fruit) else { return };
        let side = SIDES[self.rng.below(4) as usize];
        let spot = at + side;
        let world = self.chunks.world();
        let free = world
            .block(spot.x, spot.y, spot.z)
            .is_some_and(BlockState::is_air);
        let soil = world.block(spot.x, spot.y - 1, spot.z).is_some_and(|b| {
            b.is(blocks::FARMLAND)
                || [
                    blocks::DIRT,
                    blocks::GRASS_BLOCK,
                    blocks::COARSE_DIRT,
                    blocks::PODZOL,
                    blocks::MYCELIUM,
                    blocks::ROOTED_DIRT,
                    blocks::MOSS_BLOCK,
                    blocks::MUD,
                ]
                .iter()
                .any(|&id| b.is(id))
        });
        if free && soil {
            self.edit(&[
                (spot, BlockState::of(fruit)),
                (at, state.with_number(Prop::Age7, 0)),
            ]);
        }
    }

    /// Sugar cane and cactus: up to three tall, a tick in sixteen grows
    /// a block on top (their age counts to 15).
    fn grow_stalk(&mut self, at: IVec3, state: BlockState) {
        let world = self.chunks.world();
        let above = at + IVec3::Y;
        if !world
            .block(above.x, above.y, above.z)
            .is_some_and(BlockState::is_air)
        {
            return;
        }
        let mut height = 1;
        while world
            .block(at.x, at.y - height, at.z)
            .is_some_and(|b| b.is(state.id()))
        {
            height += 1;
        }
        if height >= cfg::STALK_HEIGHT {
            return;
        }
        let age = state.number(Prop::Age15);
        if age == 15 {
            self.edit(&[
                (above, BlockState::of(state.id())),
                (at, state.with_number(Prop::Age15, 0)),
            ]);
        } else {
            self.edit(&[(at, state.with_number(Prop::Age15, age + 1))]);
        }
    }

    /// Bamboo: a third of the ticks with light, up to 12–16 tall.
    fn grow_bamboo(&mut self, at: IVec3) {
        let above = at + IVec3::Y;
        let world = self.chunks.world();
        if !world
            .block(above.x, above.y, above.z)
            .is_some_and(BlockState::is_air)
            || !self.crop_light(above)
            || self.rng.below(3) != 0
        {
            return;
        }
        let (mut base, mut top) = (at, at);
        while world
            .block(base.x, base.y - 1, base.z)
            .is_some_and(|b| b.is(blocks::BAMBOO))
        {
            base.y -= 1;
        }
        while world
            .block(top.x, top.y + 1, top.z)
            .is_some_and(|b| b.is(blocks::BAMBOO))
        {
            top.y += 1;
        }
        let limit = 12
            + (crate::world::worldgen::rng::mix((base.x as u64) << 32 ^ base.z as u64) % 5) as i32;
        if top == at && top.y - base.y + 1 < limit {
            self.edit(&[(above, BlockState::of(blocks::BAMBOO))]);
        }
    }

    fn grow_kelp(&mut self, at: IVec3, state: BlockState) {
        let above = at + IVec3::Y;
        let age = state.number(Prop::Age25);
        let world = self.chunks.world();
        let water = world
            .block(above.x, above.y, above.z)
            .is_some_and(|b| b.is(blocks::WATER));
        if age < 25 && water {
            let mut kelp = BlockState::of(blocks::KELP).with_bool(Prop::Waterlogged, true);
            kelp = kelp.with_number(Prop::Age25, age + 1);
            self.edit(&[(above, kelp)]);
        }
    }

    /// Farmland: wet if water lies within four blocks on its level or
    /// the one above; dries one step a tick; with no crop and no water it
    /// goes back to dirt.
    fn tick_farmland(&mut self, at: IVec3, state: BlockState) {
        let world = self.chunks.world();
        let wet = (-4..=4).any(|dx| {
            (-4..=4).any(|dz| {
                (0..=1).any(|dy| {
                    world
                        .block(at.x + dx, at.y + dy, at.z + dz)
                        .is_some_and(|b| b.is(blocks::WATER))
                })
            })
        });
        let moisture = state.number(Prop::Moisture);
        let above = world.block(at.x, at.y + 1, at.z);
        let planted = above.is_some_and(|b| b.def().soil.is_some() || b.def().random_ticks);
        if above.is_some_and(|b| b.def().solid && b.def().is_opaque_cube()) {
            self.edit(&[(at, BlockState::of(blocks::DIRT))]);
        } else if wet {
            if moisture < 7 {
                self.edit(&[(at, state.with_number(Prop::Moisture, 7))]);
            }
        } else if moisture > 0 {
            self.edit(&[(at, state.with_number(Prop::Moisture, moisture - 1))]);
        } else if !planted {
            self.edit(&[(at, BlockState::of(blocks::DIRT))]);
        }
    }

    /// Something of `weight` landed on the block under `feet` after a
    /// fall: farmland may be trampled (more likely the further).
    pub(super) fn trample(&mut self, feet: DVec3, fall: f32) {
        let below = IVec3::new(
            feet.x.floor() as i32,
            (feet.y - 0.2).floor() as i32,
            feet.z.floor() as i32,
        );
        let Some(block) = self.chunks.world().block(below.x, below.y, below.z) else {
            return;
        };
        if block.is(blocks::FARMLAND) && self.rng.next_f64() < f64::from(fall - 0.5) {
            self.edit(&[(below, BlockState::of(blocks::DIRT))]);
        }
    }

    /// Mushrooms spread to a nearby dark spot unless five or more grow
    /// within 4 blocks.
    fn spread_mushroom(&mut self, at: IVec3, state: BlockState) {
        if self.rng.below(25) != 0 {
            return;
        }
        let world = self.chunks.world();
        let mut count = 0;
        for x in -4..=4 {
            for y in -1..=1 {
                for z in -4..=4 {
                    if world
                        .block(at.x + x, at.y + y, at.z + z)
                        .is_some_and(|b| b.is(state.id()))
                    {
                        count += 1;
                    }
                }
            }
        }
        if count > 4 {
            return;
        }
        let mut spot = at;
        for _ in 0..2 {
            spot += IVec3::new(
                self.rng.below(3) as i32 - 1,
                self.rng.below(3) as i32 - 1,
                self.rng.below(3) as i32 - 1,
            );
        }
        let world = self.chunks.world();
        let dark = self
            .chunks
            .light_at(spot.x, spot.y, spot.z)
            .is_some_and(|(sky, block)| sky.max(block) < 13);
        let free = world
            .block(spot.x, spot.y, spot.z)
            .is_some_and(BlockState::is_air);
        let solid = world
            .block(spot.x, spot.y - 1, spot.z)
            .is_some_and(|b| b.def().is_opaque_cube());
        if dark && free && solid {
            self.edit(&[(spot, BlockState::of(state.id()))]);
        }
    }

    /// Saplings: one tick in seven with light, first a stage, then a tree.
    fn tick_sapling(&mut self, at: IVec3, state: BlockState) {
        if !self.crop_light(at + IVec3::Y) || self.rng.below(7) != 0 {
            return;
        }
        if state.number(Prop::Stage) == 0 {
            self.edit(&[(at, state.with_number(Prop::Stage, 1))]);
        } else {
            self.grow_tree(at, state);
        }
    }

    /// Replaces the sapling at `at` with its tree if there is room.
    pub(super) fn grow_tree(&mut self, at: IVec3, state: BlockState) -> bool {
        let name = state.def().name.clone();
        let species = name
            .trim_end_matches("_sapling")
            .trim_end_matches("_propagule");
        let species = match species {
            "oak" | "birch" | "spruce" | "jungle" | "acacia" | "cherry" => species,
            _ => return false,
        };
        let (Some(log), Some(leaves)) = (
            named(&format!("{species}_log")),
            named(&format!("{species}_leaves")),
        ) else {
            return false;
        };
        let mut rng = crate::world::worldgen::rng::Rng::new(self.rng.next_u64());
        let shape = tree_shape(species, &mut rng);
        let world = self.chunks.world();
        let room = shape.iter().all(|(p, _)| {
            let c = at + *p;
            world.block(c.x, c.y, c.z).is_some_and(|b| {
                b.is_air()
                    || b.def().replaceable
                    || b.def().name.ends_with("_leaves")
                    || *p == IVec3::ZERO
            })
        });
        if !room {
            return false;
        }
        let mut edits = Vec::new();
        for (p, is_log) in shape {
            let state = if is_log {
                BlockState::of(log)
            } else {
                BlockState::of(leaves)
            };
            edits.push((at + p, state));
        }
        self.edit(&edits);
        true
    }

    // --- the player's hands ---

    /// Right-click on a block with a hoe, bone meal or a planting item.
    /// Returns whether it did something.
    pub(super) fn farm_use_item(&mut self, hit: &Hit) -> bool {
        let Some(stack) = self.held().cloned() else {
            return false;
        };
        let target = hit.block;
        let Some(block) = self.chunks.world().block(target.x, target.y, target.z) else {
            return false;
        };
        let above = target + IVec3::Y;
        let above_air = self
            .chunks
            .world()
            .block(above.x, above.y, above.z)
            .is_some_and(BlockState::is_air);
        // Hoes till.
        if self
            .items
            .tool(stack.id)
            .is_some_and(|t| t.kind == ToolKind::Hoe)
        {
            let till = |id| block.is(id);
            let new = if till(blocks::GRASS_BLOCK) || till(blocks::DIRT) || till(blocks::DIRT_PATH)
            {
                above_air.then(|| BlockState::of(blocks::FARMLAND))
            } else if till(blocks::COARSE_DIRT) {
                above_air.then(|| BlockState::of(blocks::DIRT))
            } else if till(blocks::ROOTED_DIRT) {
                Some(BlockState::of(blocks::DIRT))
            } else {
                None
            };
            let Some(new) = new else { return false };
            self.edit(&[(target, new)]);
            crate::audio::play("item.hoe.till", target.as_dvec3() + DVec3::splat(0.5));
            if block.is(blocks::ROOTED_DIRT)
                && let Some(roots) = items::by_name("hanging_roots")
            {
                let mut rnd = || self.rng.next_f64() as f32;
                let drop = ItemEntity::from_block(
                    crate::inventory::stack::ItemStack::new(roots, 1),
                    target,
                    &mut rnd,
                );
                self.entities.push(drop);
            }
            if self.player.mode.wears_tools() {
                self.wear_selected(1);
            }
            return true;
        }
        if stack.id == items::BONE_MEAL {
            if self.bone_meal(target, block) {
                if self.player.mode.uses_up_items() {
                    self.player.inventory.take(self.held_slot(), 1);
                }
                return true;
            }
            return false;
        }
        // Cocoa goes on the side of a jungle log.
        if stack.id == items::COCOA_BEANS
            && let Some(face) = hit.face
            && !matches!(face, crate::world::Face::Top | crate::world::Face::Bottom)
            && block.def().name == "jungle_log"
        {
            let spot = target + IVec3::from_array(face.normal());
            if self
                .chunks
                .world()
                .block(spot.x, spot.y, spot.z)
                .is_some_and(BlockState::is_air)
            {
                self.edit(&[(spot, BlockState::of(blocks::COCOA))]);
                if self.player.mode.uses_up_items() {
                    self.player.inventory.take(self.held_slot(), 1);
                }
                return true;
            }
            return false;
        }
        // A composter takes what it can compost.
        if block.is(blocks::COMPOSTER) {
            return self.fill_composter(target, block, stack.id);
        }
        false
    }

    /// Right-click a block by itself: berries off a bush, bone meal out of
    /// a full composter.
    pub(super) fn farm_use_block(&mut self, cell: IVec3) -> bool {
        let Some(block) = self.chunks.world().block(cell.x, cell.y, cell.z) else {
            return false;
        };
        if block.is(blocks::SWEET_BERRY_BUSH) {
            let age = block.number(Prop::Age3);
            if age < 2 {
                return false;
            }
            let count = 1 + self.rng.below(2) + u32::from(age == 3);
            if let Some(berries) = items::by_name("sweet_berries") {
                let mut rnd = || self.rng.next_f64() as f32;
                let drop = ItemEntity::from_block(
                    crate::inventory::stack::ItemStack::new(berries, count as u8),
                    cell,
                    &mut rnd,
                );
                self.entities.push(drop);
            }
            self.edit(&[(cell, block.with_number(Prop::Age3, 1))]);
            return true;
        }
        if block.is(blocks::COMPOSTER) && block.number(Prop::Level) == 8 {
            let mut rnd = || self.rng.next_f64() as f32;
            let drop = ItemEntity::from_block(
                crate::inventory::stack::ItemStack::new(items::BONE_MEAL, 1),
                cell,
                &mut rnd,
            );
            self.entities.push(drop);
            self.edit(&[(cell, block.with_number(Prop::Level, 0))]);
            return true;
        }
        false
    }

    fn fill_composter(&mut self, cell: IVec3, block: BlockState, item: ItemId) -> bool {
        let level = block.number(Prop::Level);
        let Some(chance) = compost_chance(item.name()) else {
            return false;
        };
        if level >= 7 {
            return false;
        }
        if self.player.mode.uses_up_items() {
            self.player.inventory.take(self.held_slot(), 1);
        }
        if level == 0 || self.rng.next_f64() < f64::from(chance) {
            let next = level + 1;
            self.edit(&[(cell, block.with_number(Prop::Level, next))]);
            if next == 7 {
                self.farm_due
                    .push((self.time + cfg::COMPOSTER_READY_TICKS, cell));
            }
        }
        true
    }

    /// Composters that filled up become ready after a second.
    pub(super) fn tick_farm_due(&mut self) {
        if self.farm_due.is_empty() {
            return;
        }
        let now = self.time;
        let (due, later): (Vec<_>, Vec<_>) = std::mem::take(&mut self.farm_due)
            .into_iter()
            .partition(|(t, _)| *t <= now);
        self.farm_due = later;
        for (_, cell) in due {
            if let Some(b) = self.chunks.world().block(cell.x, cell.y, cell.z)
                && b.is(blocks::COMPOSTER)
                && b.number(Prop::Level) == 7
            {
                self.edit(&[(cell, b.with_number(Prop::Level, 8))]);
            }
        }
    }

    /// Bone meal on `cell`: crops, saplings and the like. True if used.
    pub(super) fn bone_meal(&mut self, cell: IVec3, block: BlockState) -> bool {
        let name = block.def().name.clone();
        let (lo, hi) = cfg::BONE_MEAL_STAGES;
        let by = (lo + self.rng.below(hi - lo + 1)) as u8;
        let used = match name.as_str() {
            "wheat" | "carrots" | "potatoes" | "melon_stem" | "pumpkin_stem" => {
                self.age_up(cell, block, Prop::Age7, 7, by)
            }
            "beetroots" => self.age_up(cell, block, Prop::Age3, 3, 1),
            "sweet_berry_bush" => self.age_up(cell, block, Prop::Age3, 3, 1),
            "cocoa" => self.age_up(cell, block, Prop::Age2, 2, 1),
            "sugar_cane" | "cactus" => {
                let mut top = cell;
                while self
                    .chunks
                    .world()
                    .block(top.x, top.y + 1, top.z)
                    .is_some_and(|b| b.is(block.id()))
                {
                    top.y += 1;
                }
                let above = top + IVec3::Y;
                let free = self
                    .chunks
                    .world()
                    .block(above.x, above.y, above.z)
                    .is_some_and(BlockState::is_air);
                if free && (top.y - cell.y) < 2 {
                    self.edit(&[(above, BlockState::of(block.id()))]);
                }
                free
            }
            "bamboo" => {
                let above = cell + IVec3::Y;
                let free = self
                    .chunks
                    .world()
                    .block(above.x, above.y, above.z)
                    .is_some_and(BlockState::is_air);
                if free {
                    self.edit(&[(above, BlockState::of(blocks::BAMBOO))]);
                }
                free
            }
            n if n.ends_with("_sapling") => {
                if self.rng.below(100) < 45 {
                    if block.number(Prop::Stage) == 0 {
                        self.edit(&[(cell, block.with_number(Prop::Stage, 1))]);
                    } else {
                        self.grow_tree(cell, block);
                    }
                }
                true
            }
            "grass_block" => {
                self.spread_grass(cell);
                true
            }
            _ => false,
        };
        if used {
            self.green_sparkles(cell);
        }
        used
    }

    /// Short grass, now and then a flower, on grass within three blocks.
    fn spread_grass(&mut self, cell: IVec3) {
        let (Some(grass), Some(flower)) =
            (named("short_grass").or(named("grass")), named("dandelion"))
        else {
            return;
        };
        let mut edits = Vec::new();
        for _ in 0..24 {
            let spot = cell
                + IVec3::new(
                    self.rng.below(7) as i32 - 3,
                    0,
                    self.rng.below(7) as i32 - 3,
                );
            let world = self.chunks.world();
            let on_grass = world
                .block(spot.x, spot.y, spot.z)
                .is_some_and(|b| b.is(blocks::GRASS_BLOCK));
            let free = world
                .block(spot.x, spot.y + 1, spot.z)
                .is_some_and(BlockState::is_air);
            if on_grass
                && free
                && !edits
                    .iter()
                    .any(|(p, _): &(IVec3, BlockState)| *p == spot + IVec3::Y)
            {
                let plant = if self.rng.below(8) == 0 {
                    flower
                } else {
                    grass
                };
                edits.push((spot + IVec3::Y, BlockState::of(plant)));
            }
        }
        self.edit(&edits);
    }

    fn green_sparkles(&mut self, cell: IVec3) {
        let rng = &mut self.rng;
        let mut random = || rng.next_f64() as f32;
        let burst = Particle::burst(cell, BlockState::of(blocks::LIME_WOOL), &mut random);
        self.add_particles(burst);
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use crate::{
        game::tests::{flat_game, mob_registry},
        inventory::stack::ItemStack,
        player::mode::GameMode,
        world::Face,
    };

    fn block(name: &str) -> BlockState {
        BlockState::of(blocks::by_name(name).unwrap())
    }

    fn at(x: i32, y: i32, z: i32) -> IVec3 {
        IVec3::new(x, y, z)
    }

    /// A noon game whose chunks have been lit, so crops see light.
    fn lit_game() -> Game {
        let mut game = flat_game(GameMode::Survival).with_mobs(mob_registry());
        game.time = 6000;
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        while game.chunks.light_at(3, 64, 3).is_none() {
            assert!(std::time::Instant::now() < deadline, "no light");
            game.chunks.update(crate::world::chunk::ChunkPos::new(0, 0));
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        game
    }

    fn top_hit(cell: IVec3) -> Hit {
        Hit {
            block: cell,
            face: Some(Face::Top),
            point: cell.as_dvec3() + DVec3::new(0.5, 1.0, 0.5),
        }
    }

    fn hold(game: &mut Game, name: &str) {
        let id = items::by_name(name).unwrap();
        game.player
            .inventory
            .set(game.player.inventory.selected, Some(ItemStack::new(id, 4)));
    }

    fn cell(game: &Game, p: IVec3) -> BlockState {
        game.chunks.world().block(p.x, p.y, p.z).unwrap()
    }

    #[test]
    fn growth_speed_follows_the_vanilla_formula() {
        let wheat = blocks::WHEAT;
        let farm =
            |moisture| BlockState::of(blocks::FARMLAND).with_number(Prop::Moisture, moisture);
        let field = |ground: BlockState, extra: &[(IVec3, BlockState)]| {
            let mut map: HashMap<IVec3, BlockState> = HashMap::new();
            for dx in -1..=1 {
                for dz in -1..=1 {
                    map.insert(at(dx, -1, dz), ground);
                }
            }
            map.extend(extra.iter().copied());
            move |p: IVec3| Some(map.get(&p).copied().unwrap_or(BlockState::AIR))
        };
        // Dry farmland all round: 1 + 1 + 8 × 1/4.
        assert_eq!(growth_speed(&field(farm(0), &[]), at(0, 0, 0), wheat), 4.0);
        // Wet: 1 + 3 + 8 × 3/4.
        assert_eq!(growth_speed(&field(farm(7), &[]), at(0, 0, 0), wheat), 10.0);
        // No farmland: 1.
        assert_eq!(
            growth_speed(
                &field(BlockState::of(blocks::DIRT), &[]),
                at(0, 0, 0),
                wheat
            ),
            1.0
        );
        // The same crop on both sides along one row and across: halved.
        let crop = BlockState::of(wheat);
        let rows = field(farm(7), &[(at(1, 0, 0), crop), (at(0, 0, 1), crop)]);
        assert_eq!(growth_speed(&rows, at(0, 0, 0), wheat), 5.0);
        // A neat row (neighbors on one axis only) isn't penalised.
        let row = field(farm(7), &[(at(1, 0, 0), crop), (at(-1, 0, 0), crop)]);
        assert_eq!(growth_speed(&row, at(0, 0, 0), wheat), 10.0);
    }

    #[test]
    fn a_hoe_tills_grass_dirt_and_paths_and_wears() {
        let mut game = lit_game();
        let spot = at(3, 63, 3);
        hold(&mut game, "iron_hoe");
        game.player.inventory.set(
            game.player.inventory.selected,
            Some(ItemStack::new(items::by_name("iron_hoe").unwrap(), 1)),
        );
        assert!(game.farm_use_item(&top_hit(spot)));
        assert!(cell(&game, spot).is(blocks::FARMLAND));
        // Coarse dirt turns to dirt, and dirt under a block stays.
        game.edit(&[
            (at(5, 63, 3), block("coarse_dirt")),
            (at(7, 63, 3), block("dirt")),
            (at(7, 64, 3), block("stone")),
        ]);
        assert!(game.farm_use_item(&top_hit(at(5, 63, 3))));
        assert!(cell(&game, at(5, 63, 3)).is(blocks::DIRT));
        assert!(
            !game.farm_use_item(&top_hit(at(7, 63, 3))),
            "something sits on it"
        );
        assert!(
            game.player.inventory.selected_stack().unwrap().damage > 0,
            "the hoe wore"
        );
    }

    #[test]
    fn wheat_needs_farmland_grows_on_random_ticks_in_light_and_stops_when_ripe() {
        let mut game = lit_game();
        let farm = BlockState::of(blocks::FARMLAND).with_number(Prop::Moisture, 7);
        let spot = at(3, 64, 3);
        game.edit(&[(at(3, 63, 3), farm), (spot, block("wheat"))]);
        // Seeds can't be planted on grass, only farmland.
        hold(&mut game, "wheat_seeds");
        assert!(
            !BlockState::of(blocks::WHEAT)
                .def()
                .soil
                .unwrap()
                .allows(cell(&game, at(6, 63, 6)))
        );
        for _ in 0..3000 {
            let state = cell(&game, spot);
            game.farm_random_tick(spot, state);
        }
        assert_eq!(
            cell(&game, spot).number(Prop::Age7),
            7,
            "ripe after plenty of ticks"
        );
        // In the dark it doesn't grow.
        let mut night = lit_game();
        night.time = 18000;
        night.edit(&[(at(3, 63, 3), farm), (spot, block("wheat"))]);
        for _ in 0..3000 {
            let state = cell(&night, spot);
            night.farm_random_tick(spot, state);
        }
        assert_eq!(
            cell(&night, spot).number(Prop::Age7),
            0,
            "no growth at night in the open"
        );
    }

    #[test]
    fn farmland_dries_without_water_turns_to_dirt_and_is_trampled_by_falls() {
        let mut game = lit_game();
        let spot = at(3, 63, 3);
        let wet = BlockState::of(blocks::FARMLAND).with_number(Prop::Moisture, 7);
        game.edit(&[(spot, wet)]);
        for _ in 0..8 {
            let s = cell(&game, spot);
            game.tick_farmland(spot, s);
        }
        assert!(
            cell(&game, spot).is(blocks::DIRT),
            "no water, no crop: dirt"
        );
        // Water within four blocks keeps it wet.
        game.edit(&[
            (spot, BlockState::of(blocks::FARMLAND)),
            (at(3, 63, 7), BlockState::of(blocks::WATER)),
        ]);
        let s = cell(&game, spot);
        game.tick_farmland(spot, s);
        assert_eq!(cell(&game, spot).number(Prop::Moisture), 7);
        // A long fall tramples it; a short hop doesn't.
        game.trample(spot.as_dvec3() + DVec3::new(0.5, 1.0, 0.5), 0.4);
        assert!(cell(&game, spot).is(blocks::FARMLAND));
        game.trample(spot.as_dvec3() + DVec3::new(0.5, 1.0, 0.5), 9.0);
        assert!(cell(&game, spot).is(blocks::DIRT));
    }

    #[test]
    fn bone_meal_advances_crops_and_the_composter_makes_it() {
        let mut game = lit_game();
        let farm = BlockState::of(blocks::FARMLAND).with_number(Prop::Moisture, 7);
        game.edit(&[(at(3, 63, 3), farm), (at(3, 64, 3), block("wheat"))]);
        hold(&mut game, "bone_meal");
        assert!(game.farm_use_item(&top_hit(at(3, 64, 3))));
        let age = cell(&game, at(3, 64, 3)).number(Prop::Age7);
        assert!((2..=5).contains(&age), "{age}");
        assert_eq!(game.player.inventory.selected_stack().unwrap().count, 3);
        // A composter: wheat seeds (30%) until level 7, then bone meal.
        game.edit(&[(at(6, 64, 6), block("composter"))]);
        hold(&mut game, "wheat_seeds");
        for _ in 0..300 {
            game.player.inventory.set(
                game.player.inventory.selected,
                Some(ItemStack::new(items::by_name("wheat_seeds").unwrap(), 1)),
            );
            game.farm_use_item(&top_hit(at(6, 64, 6)));
            if cell(&game, at(6, 64, 6)).number(Prop::Level) == 7 {
                break;
            }
        }
        assert_eq!(cell(&game, at(6, 64, 6)).number(Prop::Level), 7);
        game.run_ticks(25);
        assert_eq!(
            cell(&game, at(6, 64, 6)).number(Prop::Level),
            8,
            "ready a second later"
        );
        assert!(game.farm_use_block(at(6, 64, 6)));
        assert_eq!(cell(&game, at(6, 64, 6)).number(Prop::Level), 0);
        assert!(game.entities.iter().any(|e| e.stack.id == items::BONE_MEAL));
    }

    #[test]
    fn compost_chances_are_vanillas() {
        assert_eq!(compost_chance("wheat_seeds"), Some(0.3));
        assert_eq!(compost_chance("oak_leaves"), Some(0.3));
        assert_eq!(compost_chance("melon_slice"), Some(0.5));
        assert_eq!(compost_chance("carrot"), Some(0.65));
        assert_eq!(compost_chance("bread"), Some(0.85));
        assert_eq!(compost_chance("cake"), Some(1.0));
        assert_eq!(compost_chance("stone"), None);
    }

    #[test]
    fn sugar_cane_and_cactus_grow_three_tall_and_bamboo_to_its_limit() {
        let mut game = lit_game();
        game.edit(&[
            (at(3, 63, 3), block("sand")),
            (at(3, 64, 3), block("cactus")),
            (at(6, 63, 6), block("dirt")),
            (at(6, 64, 6), block("bamboo")),
        ]);
        for _ in 0..4000 {
            for p in [
                at(3, 64, 3),
                at(3, 65, 3),
                at(3, 66, 3),
                at(6, 64, 6),
                at(6, 65, 6),
                at(6, 66, 6),
            ] {
                let s = cell(&game, p);
                if s.is(blocks::CACTUS) || s.is(blocks::BAMBOO) {
                    game.farm_random_tick(p, s);
                }
            }
        }
        assert!(
            cell(&game, at(3, 66, 3)).is(blocks::CACTUS) && cell(&game, at(3, 67, 3)).is_air(),
            "cactus stops at three"
        );
        let top = (64..90)
            .take_while(|&y| cell(&game, at(6, y, 6)).is(blocks::BAMBOO))
            .count();
        assert!(top > 3, "bamboo grew: {top}");
    }

    #[test]
    fn two_animals_in_love_make_a_baby_that_grows_up() {
        let mut game = lit_game();
        for x in [2.5, 3.5] {
            let cow = game.summon("cow", DVec3::new(x, 64.0, 3.5), 0.0).unwrap();
            cow.data.love = 600;
        }
        let before = game.mobs.len();
        game.run_ticks(60);
        assert_eq!(game.mobs.len(), before + 1, "a calf");
        let baby = game.mobs.iter().find(|m| m.data.age < 0).expect("a baby");
        assert!(
            baby.data.age <= -cfg::BABY_TICKS + 60,
            "a fresh calf has nearly its full time left"
        );
        let parents: Vec<_> = game.mobs.iter().filter(|m| m.data.age >= 0).collect();
        assert!(
            parents
                .iter()
                .all(|m| m.data.love == 0 && m.data.breed_cooldown > 5000),
            "five minutes before again"
        );
        // After its time a baby is grown.
        game.mobs
            .iter_mut()
            .filter(|m| m.data.age < 0)
            .for_each(|m| m.data.age = -2);
        game.run_ticks(3);
        assert!(game.mobs.iter().all(|m| m.data.age >= 0), "grown up");
    }

    #[test]
    fn feeding_sets_love_only_with_the_right_food_and_follows_the_player() {
        let mut game = lit_game();
        let cow = game.summon("cow", DVec3::new(6.5, 64.0, 3.5), 0.0).unwrap();
        cow.data.love = 0;
        let idx = game.mobs.len() - 1;
        hold(&mut game, "carrot");
        assert!(!game.use_mob(idx), "a cow doesn't want carrots");
        hold(&mut game, "wheat");
        assert!(game.use_mob(idx));
        assert!(game.mobs[idx].data.love > 0);
        // A baby fed wheat grows up sooner.
        game.mobs[idx].data.love = 0;
        game.mobs[idx].data.age = -10000;
        assert!(game.use_mob(idx));
        assert_eq!(game.mobs[idx].data.age, -9000);
    }

    #[test]
    fn a_sapling_grows_into_a_tree_when_there_is_room() {
        let mut game = lit_game();
        game.edit(&[(
            at(4, 64, 4),
            block("oak_sapling").with_number(Prop::Stage, 1),
        )]);
        assert!(game.grow_tree(at(4, 64, 4), cell(&game, at(4, 64, 4))));
        assert!(
            cell(&game, at(4, 64, 4)).is(blocks::OAK_LOG)
                && cell(&game, at(4, 67, 4)).is(blocks::OAK_LOG)
        );
        assert!(
            (66..=71).any(|y| cell(&game, at(5, y, 4)).def().name.ends_with("leaves")),
            "a crown of leaves"
        );
        // A stone slab roof blocks it.
        game.edit(&[
            (at(10, 64, 10), block("oak_sapling")),
            (at(10, 66, 10), block("stone")),
        ]);
        assert!(!game.grow_tree(at(10, 64, 10), cell(&game, at(10, 64, 10))));
    }
}
