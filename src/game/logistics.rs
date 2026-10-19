//! Moving items between blocks, following modern Minecraft: droppers and
//! dispensers fire (an arrow flies, TNT lights, a bucket pours, a dropper
//! hands its item to a container in front, anything else drops), and
//! hoppers pull from the container or items above and push into the
//! container they face, one item every 8 ticks.

use glam::{DVec3, IVec3};

use super::Game;
use crate::{
    entity::{arrow::Arrow, item::ItemEntity},
    inventory::{
        furnace,
        item::{Bucket, ItemId},
        slots,
        stack::ItemStack,
    },
    player::collision::Aabb,
    world::{
        Face,
        block::BlockState,
        block_entity::{BlockEntity, HOPPER_COOLDOWN},
        blocks, fluid,
        props::Prop,
    },
};

/// Dispensed arrows fly at this share of a full bow's speed (modern
/// projectile power 1.1 against a bow's 3.0).
const ARROW_POWER: f32 = 1.1 / 3.0;
/// How far a hopper reaches up for dropped items.
const SUCK_HEIGHT: f64 = 2.0;
const SUCK_BOTTOM: f64 = 11.0 / 16.0;

fn step(face: Face) -> IVec3 {
    IVec3::from_array(face.normal())
}

impl Game {
    /// What kind of container is at `cell`, if any.
    fn holds_items(&self, cell: IVec3) -> bool {
        self.chunks
            .world()
            .block(cell.x, cell.y, cell.z)
            .is_some_and(|state| {
                matches!(
                    state.id(),
                    blocks::CHEST
                        | blocks::FURNACE
                        | blocks::DISPENSER
                        | blocks::DROPPER
                        | blocks::HOPPER
                ) || state.def().shape == crate::world::block::Shape::Chest
            })
    }

    /// Puts `stack` into the container at `cell`, entering through its
    /// `side` face. Returns what didn't fit (the whole stack if `cell`
    /// isn't a container or won't take it).
    pub(super) fn insert_into(
        &mut self,
        cell: IVec3,
        side: Face,
        stack: ItemStack,
    ) -> Option<ItemStack> {
        if !self.holds_items(cell) {
            return Some(stack);
        }
        let Some(block) = self.chunks.world().block(cell.x, cell.y, cell.z) else {
            return Some(stack);
        };
        if self.chunks.block_entity(cell).is_none() {
            let made = match block.id() {
                blocks::DISPENSER | blocks::DROPPER => BlockEntity::dispenser(),
                blocks::HOPPER => BlockEntity::Hopper(Default::default()),
                blocks::FURNACE => BlockEntity::Furnace(Default::default()),
                _ => BlockEntity::chest(),
            };
            self.chunks.set_block_entity(cell, Some(made));
        }
        let items = self.items.clone();
        let Some(entity) = self.chunks.block_entity_mut(cell) else {
            return Some(stack);
        };
        let is_furnace = matches!(entity, BlockEntity::Furnace(_));
        let slots = entity.slots_mut();
        if is_furnace {
            // From above into the input; from the sides into the fuel.
            let slot = match side {
                Face::Top => furnace::INPUT,
                Face::Bottom => return Some(stack),
                _ => furnace::FUEL,
            };
            if slot == furnace::FUEL && items.recipes.burn_ticks(stack.id) == 0 {
                return Some(stack);
            }
            if slot == furnace::INPUT && items.recipes.smelt(&stack).is_none() {
                return Some(stack);
            }
            return slots::merge(slots, slot..=slot, stack, &items);
        }
        let len = slots.len();
        slots::merge(slots, 0..len, stack, &items)
    }

    /// Takes one item from the container at `cell` for a hopper under it:
    /// the first slot with something (a furnace gives only its output).
    pub(super) fn take_one_from(&mut self, cell: IVec3) -> Option<(usize, ItemStack)> {
        if !self.holds_items(cell) {
            return None;
        }
        let entity = self.chunks.block_entity(cell)?;
        let range = if matches!(entity, BlockEntity::Furnace(_)) {
            furnace::OUTPUT..furnace::OUTPUT + 1
        } else {
            0..entity.slots().len()
        };
        range.into_iter().find_map(|i| {
            entity.slots()[i]
                .clone()
                .map(|stack| (i, ItemStack { count: 1, ..stack }))
        })
    }

    pub(super) fn remove_one(&mut self, cell: IVec3, slot: usize) {
        let Some(entity) = self.chunks.block_entity_mut(cell) else {
            return;
        };
        let cell = &mut entity.slots_mut()[slot];
        if let Some(stack) = cell {
            stack.count -= 1;
            if stack.count == 0 {
                *cell = None;
            }
        }
    }

    /// Fires the dispenser or dropper at `at`.
    pub(super) fn dispense(&mut self, at: IVec3) {
        let Some(block) = self.chunks.world().block(at.x, at.y, at.z) else {
            return;
        };
        if !matches!(block.id(), blocks::DISPENSER | blocks::DROPPER) {
            return;
        }
        self.ensure_entity(at);
        let Some(entity) = self.chunks.block_entity(at) else {
            return;
        };
        let filled: Vec<usize> = (0..entity.slots().len())
            .filter(|&i| entity.slots()[i].is_some())
            .collect();
        if filled.is_empty() {
            return;
        }
        let slot = filled[self.rng.below(filled.len() as u32) as usize];
        let Some(stack) = entity.slots()[slot].clone() else {
            return;
        };
        let facing = block.face(Prop::FacingAll);
        let front = at + step(facing);
        let one = ItemStack { count: 1, ..stack };
        let dropper = block.is(blocks::DROPPER);
        let used = if dropper {
            self.drop_or_hand_over(front, facing, one)
        } else {
            self.dispense_item(front, facing, one)
        };
        match used {
            // Used up, or the bucket changed.
            Some(Used::Gone) => self.remove_one(at, slot),
            Some(Used::Became(item)) => {
                if let Some(entity) = self.chunks.block_entity_mut(at) {
                    entity.slots_mut()[slot] = Some(ItemStack::with_damage(item, 1, 0));
                }
            }
            None => {}
        }
    }

    /// A dropper's way: hand the item to a container in front, else drop it.
    fn drop_or_hand_over(&mut self, front: IVec3, facing: Face, one: ItemStack) -> Option<Used> {
        if self.holds_items(front) {
            // A full container takes nothing and the dropper keeps it.
            return self
                .insert_into(front, facing.opposite(), one)
                .is_none()
                .then_some(Used::Gone);
        }
        self.throw_out(front, facing, one);
        Some(Used::Gone)
    }

    /// A dispenser's way: arrows fly, TNT lights, buckets pour and scoop,
    /// everything else drops.
    fn dispense_item(&mut self, front: IVec3, facing: Face, one: ItemStack) -> Option<Used> {
        let spot = front.as_dvec3() + DVec3::splat(0.5);
        if one.id == crate::inventory::items::ARROW {
            let forward = DVec3::from_array(facing.normal().map(f64::from));
            let rng = &mut self.rng;
            let mut random = || rng.next_f64();
            let arrow = Arrow::shoot(
                spot - forward * 0.3 + DVec3::Y * 0.1,
                forward,
                ARROW_POWER,
                DVec3::ZERO,
                true,
                &mut random,
            );
            self.arrows.push(arrow);
            return Some(Used::Gone);
        }
        if let Some(kind) = super::minecarts::kind_of(one.id)
            && self.place_cart(front, kind)
        {
            return Some(Used::Gone);
        }
        if self.items.places(&one).is_some_and(|b| b.is(blocks::TNT)) {
            self.prime_tnt(front);
            return Some(Used::Gone);
        }
        if let Some(bucket) = self.items.bucket(one.id) {
            let world = self.chunks.world();
            let there = world.block(front.x, front.y, front.z)?;
            return match bucket {
                Bucket::Holds(fluid) => {
                    if !there.is_air()
                        && !there.def().is_replaceable(there)
                        && there.def().fluid.is_none()
                    {
                        return self.throw_out(front, facing, one).then_some(Used::Gone);
                    }
                    self.edit(&[(front, fluid::at_level(fluid, 0))]);
                    self.items.bucket_item(Bucket::Empty).map(Used::Became)
                }
                Bucket::Empty => {
                    let source = there.def().fluid.filter(|_| fluid::level(there) == 0)?;
                    self.edit(&[(front, BlockState::AIR)]);
                    self.items
                        .bucket_item(Bucket::Holds(source))
                        .map(Used::Became)
                }
            };
        }
        self.throw_out(front, facing, one);
        Some(Used::Gone)
    }

    /// Drops `one` in front of a dispenser or dropper, flying out.
    fn throw_out(&mut self, front: IVec3, facing: Face, one: ItemStack) -> bool {
        let dir = DVec3::from_array(facing.normal().map(f64::from));
        let y_offset = if matches!(facing, Face::Top | Face::Bottom) {
            0.125
        } else {
            0.15625
        };
        let origin = front.as_dvec3() + DVec3::splat(0.5) - dir * 0.3 - DVec3::Y * y_offset;
        let rng = &mut self.rng;
        let speed = 0.2 + 0.1 * rng.next_f64();
        let mut gauss = || (rng.next_f64() - rng.next_f64()) * 0.0075 * 6.0;
        let velocity = dir * speed + DVec3::new(gauss(), gauss(), gauss());
        let hover = rng.next_f64() as f32 * std::f32::consts::TAU;
        self.entities
            .push(ItemEntity::new(one, origin, velocity, hover));
        true
    }

    /// One tick of every hopper in the loaded chunks.
    pub(super) fn tick_hoppers(&mut self) {
        let world = self.chunks.world();
        let mut hoppers = Vec::new();
        for pos in world.positions() {
            let Some(chunk) = world.get(pos) else {
                continue;
            };
            let (min_x, min_z) = pos.min_block();
            for ([x, y, z], entity) in chunk.block_entities() {
                if matches!(entity, BlockEntity::Hopper(_)) {
                    hoppers.push(IVec3::new(min_x + x as i32, y as i32, min_z + z as i32));
                }
            }
        }
        for at in hoppers {
            self.tick_hopper(at);
        }
    }

    fn tick_hopper(&mut self, at: IVec3) {
        let Some(state) = self.chunks.world().block(at.x, at.y, at.z) else {
            return;
        };
        let Some(BlockEntity::Hopper(hopper)) = self.chunks.block_entity_mut(at) else {
            return;
        };
        if hopper.cooldown > 0 {
            hopper.cooldown -= 1;
            return;
        }
        if !state.is(blocks::HOPPER) || state.bool(Prop::Powered) {
            return;
        }
        let moved = self.hopper_push(at, state) | self.hopper_pull(at);
        if moved && let Some(BlockEntity::Hopper(hopper)) = self.chunks.block_entity_mut(at) {
            hopper.cooldown = HOPPER_COOLDOWN;
        }
    }

    /// Pushes one item into the container the hopper faces.
    fn hopper_push(&mut self, at: IVec3, state: BlockState) -> bool {
        let facing = state.face(Prop::FacingAll);
        let target = at + step(facing);
        let Some(BlockEntity::Hopper(hopper)) = self.chunks.block_entity(at) else {
            return false;
        };
        let Some((slot, stack)) = hopper
            .slots
            .iter()
            .enumerate()
            .find_map(|(i, s)| s.clone().map(|s| (i, ItemStack { count: 1, ..s })))
        else {
            return false;
        };
        if self.insert_into(target, facing.opposite(), stack).is_some() {
            return false;
        }
        self.remove_one(at, slot);
        true
    }

    /// Pulls one item from the container above, or else the dropped items
    /// above.
    fn hopper_pull(&mut self, at: IVec3) -> bool {
        let above = at + IVec3::Y;
        let full = |game: &Game| {
            let Some(BlockEntity::Hopper(h)) = game.chunks.block_entity(at) else {
                return true;
            };
            h.slots.iter().all(|s| {
                s.as_ref()
                    .is_some_and(|s| s.count >= game.items.max_stack(s.id))
            })
        };
        if full(self) {
            return false;
        }
        if let Some((slot, one)) = self.take_one_from(above)
            && self.insert_into(at, Face::Top, one).is_none()
        {
            self.remove_one(above, slot);
            return true;
        }
        // Items lying above.
        let suck = Aabb::new(
            at.as_dvec3() + DVec3::new(0.0, SUCK_BOTTOM, 0.0),
            at.as_dvec3() + DVec3::new(1.0, SUCK_HEIGHT, 1.0),
        );
        let mut moved = false;
        let mut index = 0;
        while index < self.entities.len() {
            let item = &self.entities[index];
            let touching =
                item.bounds().min.cmplt(suck.max).all() && item.bounds().max.cmpgt(suck.min).all();
            if !touching || item.pickup_delayed() {
                index += 1;
                continue;
            }
            let stack = item.stack.clone();
            match self.insert_into(at, Face::Top, stack.clone()) {
                None => {
                    self.entities.remove(index);
                    moved = true;
                }
                Some(rest) => {
                    if rest.count != stack.count {
                        self.entities[index].stack = rest;
                        moved = true;
                    }
                    index += 1;
                }
            }
        }
        moved
    }

    /// Gives dispensers, droppers and hoppers their slots as they're placed.
    pub(super) fn equip_placed(&mut self, edits: &[(IVec3, BlockState)]) {
        for &(at, state) in edits {
            if matches!(
                state.id(),
                blocks::DISPENSER | blocks::DROPPER | blocks::HOPPER
            ) {
                self.ensure_entity(at);
            }
        }
    }
}

/// What dispensing did to the item.
enum Used {
    /// One item used up.
    Gone,
    /// The item became another (a bucket filled or emptied).
    Became(ItemId),
}

#[cfg(test)]
mod tests {
    use glam::IVec3;

    use super::*;
    use crate::{
        game::tests::flat_game,
        inventory::items,
        player::mode::GameMode,
        world::{block::BlockState, blocks},
    };

    const Y: i32 = 64;

    fn at(x: i32, z: i32) -> IVec3 {
        IVec3::new(x, Y, z)
    }

    fn facing(block: crate::world::block::BlockId, face: Face) -> BlockState {
        BlockState::of(block).with_face(Prop::FacingAll, face)
    }

    fn count_in(game: &Game, cell: IVec3, item: ItemId) -> u32 {
        game.chunks.block_entity(cell).map_or(0, |entity| {
            entity
                .slots()
                .iter()
                .flatten()
                .filter(|s| s.id == item)
                .map(|s| u32::from(s.count))
                .sum()
        })
    }

    fn fill(game: &mut Game, cell: IVec3, stack: ItemStack) {
        game.ensure_entity(cell);
        game.chunks.block_entity_mut(cell).unwrap().slots_mut()[0] = Some(stack);
    }

    #[test]
    fn a_powered_dropper_hands_one_item_to_a_chest_or_drops_it() {
        let mut game = flat_game(GameMode::Creative);
        let z = -12;
        game.set_blocks(&[
            (at(5, z), facing(blocks::DROPPER, Face::East)),
            (at(6, z), BlockState::of(blocks::CHEST)),
        ]);
        fill(&mut game, at(5, z), ItemStack::new(items::COBBLESTONE, 3));
        game.set_blocks(&[(at(5, z + 1), BlockState::of(blocks::REDSTONE_BLOCK))]);
        game.run_ticks(8);
        assert_eq!(count_in(&game, at(6, z), items::COBBLESTONE), 1);
        assert_eq!(count_in(&game, at(5, z), items::COBBLESTONE), 2);
        // Pulsing again once more.
        game.set_blocks(&[(at(5, z + 1), BlockState::AIR)]);
        game.run_ticks(2);
        game.set_blocks(&[(at(5, z + 1), BlockState::of(blocks::REDSTONE_BLOCK))]);
        game.run_ticks(8);
        assert_eq!(count_in(&game, at(6, z), items::COBBLESTONE), 2);
        // Facing open air it throws the item out.
        game.entities.clear();
        let z = -8;
        game.set_blocks(&[(at(5, z), facing(blocks::DROPPER, Face::East))]);
        fill(&mut game, at(5, z), ItemStack::new(items::DIRT, 2));
        game.set_blocks(&[(at(5, z + 1), BlockState::of(blocks::REDSTONE_BLOCK))]);
        game.run_ticks(8);
        assert_eq!(game.entities.len(), 1);
        assert!(game.entities[0].velocity.x > 0.1, "flying east");
        assert_eq!(count_in(&game, at(5, z), items::DIRT), 1);
    }

    #[test]
    fn a_dispenser_shoots_arrows_and_lights_tnt() {
        let mut game = flat_game(GameMode::Creative);
        let z = -4;
        game.set_blocks(&[(at(5, z), facing(blocks::DISPENSER, Face::East))]);
        fill(&mut game, at(5, z), ItemStack::new(items::ARROW, 4));
        game.set_blocks(&[(at(5, z + 1), BlockState::of(blocks::REDSTONE_BLOCK))]);
        game.run_ticks(8);
        assert_eq!(game.arrows.len(), 1);
        assert!(game.arrows[0].velocity.x > 0.5);
        assert_eq!(count_in(&game, at(5, z), items::ARROW), 3);
        let z = 0;
        game.set_blocks(&[(at(9, z), facing(blocks::DISPENSER, Face::East))]);
        fill(&mut game, at(9, z), ItemStack::new(items::TNT, 1));
        game.set_blocks(&[(at(9, z + 1), BlockState::of(blocks::REDSTONE_BLOCK))]);
        game.run_ticks(8);
        assert_eq!(game.primed.len(), 1, "lit in front of the dispenser");
    }

    #[test]
    fn a_dispenser_pours_and_scoops_water() {
        let mut game = flat_game(GameMode::Creative);
        let z = 6;
        game.set_blocks(&[(at(5, z), facing(blocks::DISPENSER, Face::East))]);
        fill(&mut game, at(5, z), ItemStack::new(items::WATER_BUCKET, 1));
        game.set_blocks(&[(at(5, z + 1), BlockState::of(blocks::REDSTONE_BLOCK))]);
        game.run_ticks(8);
        assert_eq!(
            game.chunks.world().block(6, Y, z).unwrap().id(),
            blocks::WATER
        );
        assert_eq!(
            count_in(&game, at(5, z), items::BUCKET),
            1,
            "now an empty bucket"
        );
        game.set_blocks(&[(at(5, z + 1), BlockState::AIR)]);
        game.run_ticks(2);
        game.set_blocks(&[(at(5, z + 1), BlockState::of(blocks::REDSTONE_BLOCK))]);
        game.run_ticks(8);
        assert_eq!(
            count_in(&game, at(5, z), items::WATER_BUCKET),
            1,
            "and scooped it back"
        );
    }

    #[test]
    fn a_hopper_moves_items_down_a_chain_one_every_eight_ticks_and_locks_when_powered() {
        let mut game = flat_game(GameMode::Creative);
        let z = 12;
        // Chest above, hopper facing down, chest below... on the ground: put
        // the lower chest in the grass layer.
        game.set_blocks(&[
            (IVec3::new(5, Y + 2, z), BlockState::of(blocks::CHEST)),
            (
                IVec3::new(5, Y + 1, z),
                facing(blocks::HOPPER, Face::Bottom),
            ),
            (IVec3::new(5, Y, z), BlockState::of(blocks::CHEST)),
        ]);
        game.ensure_entity(IVec3::new(5, Y + 1, z));
        game.set_blocks(&[]);
        game.insert_into(
            IVec3::new(5, Y + 2, z),
            Face::Top,
            ItemStack::new(items::COBBLESTONE, 5),
        );
        game.run_ticks(1);
        let low = IVec3::new(5, Y, z);
        assert_eq!(
            count_in(&game, low, items::COBBLESTONE),
            0,
            "it takes one in first"
        );
        game.run_ticks(9);
        assert_eq!(
            count_in(&game, low, items::COBBLESTONE),
            1,
            "and passes it on 8 ticks later"
        );
        game.run_ticks(9);
        assert_eq!(count_in(&game, low, items::COBBLESTONE), 2);
        game.run_ticks(40);
        assert_eq!(count_in(&game, low, items::COBBLESTONE), 5, "five in all");
        // Power locks it.
        game.insert_into(
            IVec3::new(5, Y + 2, z),
            Face::Top,
            ItemStack::new(items::DIRT, 2),
        );
        game.set_blocks(&[(
            IVec3::new(6, Y + 1, z),
            BlockState::of(blocks::REDSTONE_BLOCK),
        )]);
        game.run_ticks(30);
        assert_eq!(count_in(&game, low, items::DIRT), 0, "locked");
        game.set_blocks(&[(IVec3::new(6, Y + 1, z), BlockState::AIR)]);
        game.run_ticks(30);
        assert_eq!(count_in(&game, low, items::DIRT), 2);
    }

    #[test]
    fn a_hopper_picks_up_dropped_items_and_feeds_a_furnace() {
        let mut game = flat_game(GameMode::Creative);
        let z = 18;
        let hopper = IVec3::new(8, Y, z);
        game.set_blocks(&[
            (hopper, facing(blocks::HOPPER, Face::East)),
            (IVec3::new(9, Y, z), BlockState::of(blocks::FURNACE)),
        ]);
        game.entities.push(ItemEntity::new(
            ItemStack::new(items::COAL, 3),
            hopper.as_dvec3() + DVec3::new(0.5, 1.2, 0.5),
            DVec3::ZERO,
            0.0,
        ));
        game.run_ticks(30);
        assert!(game.entities.is_empty(), "sucked in");
        // Sideways into a furnace means its fuel slot.
        let fuel = game
            .chunks
            .block_entity(IVec3::new(9, Y, z))
            .unwrap()
            .slots()[furnace::FUEL]
            .clone();
        assert_eq!(fuel.map(|s| s.id), Some(items::COAL));
    }

    #[test]
    fn a_dispenser_puts_a_minecart_on_the_rail_in_front_of_it() {
        let mut game = flat_game(GameMode::Creative);
        let z = 24;
        game.set_blocks(&[
            (at(5, z), facing(blocks::DISPENSER, Face::East)),
            (at(6, z), BlockState::of(blocks::RAIL)),
        ]);
        fill(&mut game, at(5, z), ItemStack::new(items::MINECART, 2));
        game.set_blocks(&[(at(5, z + 1), BlockState::of(blocks::REDSTONE_BLOCK))]);
        game.run_ticks(8);
        assert_eq!(game.carts.len(), 1);
        assert_eq!(count_in(&game, at(5, z), items::MINECART), 1);
    }
}
