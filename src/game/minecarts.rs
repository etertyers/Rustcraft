//! Minecarts in the played world: placing them on rails, riding, hitting
//! them to break them, chest and hopper carts' contents, TNT carts, and
//! the activator and detector rails they roll over.

use glam::{DVec3, IVec3};

use super::{Game, redstone::Presser};
use crate::{
    entity::{
        item::ItemEntity,
        minecart::{CartEvent, CartKind, HOPPER_COOLDOWN, Minecart},
    },
    inventory::{items, slots, stack::ItemStack},
    player::{
        collision::{Aabb, WorldCollider},
        raycast::Hit,
    },
    world::{blocks, props::Prop, rails},
};

/// Carts closer than this (centers) push each other apart.
const PUSH_DISTANCE: f64 = 0.98;
/// How far past a hopper cart's box it reaches sideways for items.
const SUCK_MARGIN: f64 = 0.25;

/// The item a cart of `kind` is placed from and drops as.
pub fn item_of(kind: CartKind) -> crate::inventory::item::ItemId {
    match kind {
        CartKind::Rideable => items::MINECART,
        CartKind::Chest => items::CHEST_MINECART,
        CartKind::Hopper => items::HOPPER_MINECART,
        CartKind::Tnt => items::TNT_MINECART,
    }
}

/// The kind of cart an item places.
pub fn kind_of(item: crate::inventory::item::ItemId) -> Option<CartKind> {
    Some(match item {
        items::MINECART => CartKind::Rideable,
        items::CHEST_MINECART => CartKind::Chest,
        items::HOPPER_MINECART => CartKind::Hopper,
        items::TNT_MINECART => CartKind::Tnt,
        _ => return None,
    })
}

impl Game {
    /// Puts a cart of `kind` on the rail at `cell`, if that is one.
    pub(super) fn place_cart(&mut self, cell: IVec3, kind: CartKind) -> bool {
        let Some(state) = self.chunks.world().block(cell.x, cell.y, cell.z) else {
            return false;
        };
        if !rails::is_rail(state) {
            return false;
        }
        let rise = if (2..=5).contains(&rails::shape_of(state)) {
            0.5
        } else {
            0.0
        };
        let at = DVec3::new(
            cell.x as f64 + 0.5,
            cell.y as f64 + 0.0625 + rise,
            cell.z as f64 + 0.5,
        );
        self.carts.push(Minecart::new(kind, at));
        true
    }

    /// The cart under the crosshair within reach, if it's nearer than the
    /// targeted block.
    pub(super) fn target_cart(&self, eye: DVec3, block: Option<&Hit>) -> Option<usize> {
        let dir = self.camera.forward().as_dvec3();
        let block_distance = block.map_or(f64::MAX, |hit| {
            (hit.block.as_dvec3() + DVec3::splat(0.5)).distance(eye) - 0.5
        });
        self.carts
            .iter()
            .enumerate()
            .filter_map(|(i, cart)| {
                let distance = cart
                    .aabb()
                    .ray_hit(eye, dir, crate::config::combat::REACH)?;
                Some((i, distance))
            })
            .filter(|&(_, distance)| distance < block_distance)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(i, _)| i)
    }

    /// Whether the player is in a cart, on a mob or in a boat.
    pub fn is_riding(&self) -> bool {
        self.riding.is_some() || self.mounted.is_some() || self.boat.is_some()
    }

    /// Right-click on a cart: sit in a plain one, open a chest or hopper
    /// one.
    pub(super) fn use_cart(&mut self, index: usize) {
        let Some(cart) = self.carts.get(index) else {
            return;
        };
        match cart.kind {
            CartKind::Rideable => {
                if self.riding.is_none() && !self.carts[index].rider {
                    self.carts[index].rider = true;
                    self.riding = Some(index);
                }
            }
            CartKind::Chest | CartKind::Hopper => self.open_cart(index),
            CartKind::Tnt => {}
        }
    }

    /// A punch (or swing) of `damage` at a cart. Creative breaks it at
    /// once; otherwise it takes a few hits.
    pub(super) fn hit_cart(&mut self, index: usize, damage: f32) {
        let Some(cart) = self.carts.get_mut(index) else {
            return;
        };
        let breaks = cart.hurt(damage) || !self.mode.drops_items();
        if breaks {
            self.break_cart(index);
        }
    }

    /// Removes a cart, dropping it and what it holds.
    pub(super) fn break_cart(&mut self, index: usize) {
        if index >= self.carts.len() {
            return;
        }
        let cart = self.carts.remove(index);
        match self.riding {
            Some(r) if r == index => self.dismount(&cart),
            Some(r) if r > index => self.riding = Some(r - 1),
            _ => {}
        }
        self.close_if_cart_gone();
        let rng = &mut self.rng;
        let mut random = || rng.next_f64() as f32;
        let center = cart.position + DVec3::Y * 0.35;
        let mut dropped: Vec<ItemStack> = cart.slots.iter().flatten().cloned().collect();
        if self.mode.drops_items() {
            dropped.push(ItemStack::new(item_of(cart.kind), 1));
        }
        for stack in dropped {
            self.entities.push(ItemEntity::new(
                stack,
                center,
                DVec3::new(
                    (random() as f64 - 0.5) * 0.2,
                    0.2,
                    (random() as f64 - 0.5) * 0.2,
                ),
                random() * std::f32::consts::TAU,
            ));
        }
    }

    /// Gets the player out of `cart`, onto a free spot beside it.
    fn dismount(&mut self, cart: &Minecart) {
        self.riding = None;
        let world = self.chunks.world();
        let collider = WorldCollider {
            world,
            blocks: self.items.blocks,
        };
        let spots = [
            DVec3::new(1.0, 0.0, 0.0),
            DVec3::new(-1.0, 0.0, 0.0),
            DVec3::new(0.0, 0.0, 1.0),
            DVec3::new(0.0, 0.0, -1.0),
            DVec3::new(0.0, 0.8, 0.0),
        ];
        let size = self.body.aabb();
        let half = (size.max - size.min) / 2.0;
        let free = |feet: DVec3| {
            let aabb = Aabb::new(
                feet - DVec3::new(half.x, 0.0, half.z),
                feet + DVec3::new(half.x, size.max.y - size.min.y, half.z),
            );
            let mut boxes = Vec::new();
            crate::player::collision::Collider::boxes(&collider, &aabb, &mut boxes);
            boxes.is_empty()
        };
        let feet = spots
            .iter()
            .map(|offset| cart.position + *offset)
            .find(|&p| free(p))
            .unwrap_or(cart.position + DVec3::Y * 0.8);
        self.body.set_position(feet);
        self.prev_feet = feet;
        self.body.velocity = DVec3::ZERO;
    }

    /// The player's tick while sitting in a cart: carried along, and out
    /// again with sneak.
    pub(super) fn ride_cart(&mut self) {
        let Some(index) = self.riding else {
            return;
        };
        let Some(cart) = self.carts.get(index) else {
            self.riding = None;
            return;
        };
        if self.keys.sneak {
            let cart = self.carts[index].clone();
            self.carts[index].rider = false;
            self.dismount(&cart);
            return;
        }
        let speed = cart.velocity.length();
        if speed > 0.05 && self.time.is_multiple_of(8) {
            crate::audio::play_here_with(
                "entity.minecart.riding",
                (speed * 2.0).min(0.9) as f32,
                1.0,
            );
        }
        self.body.set_position(cart.position);
        self.body.velocity = DVec3::ZERO;
    }

    /// The push a rider's keys give the cart: along where they look.
    fn rider_push(&self) -> DVec3 {
        let input = self.keys.move_input();
        let forward = DVec3::new(
            self.camera.yaw.sin() as f64,
            0.0,
            -self.camera.yaw.cos() as f64,
        );
        let right = DVec3::new(
            self.camera.yaw.cos() as f64,
            0.0,
            self.camera.yaw.sin() as f64,
        );
        (forward * input.forward + right * input.strafe) * 0.1
    }

    /// Boxes of carts, which press detector rails and wooden plates.
    pub(super) fn rail_pressers(&self) -> Vec<(Aabb, Presser)> {
        self.carts
            .iter()
            .map(|c| (c.aabb(), Presser::Cart))
            .collect()
    }

    /// One tick of every cart.
    pub(super) fn tick_carts(&mut self) {
        if self.carts.is_empty() {
            return;
        }
        let mut carts = std::mem::take(&mut self.carts);
        let push = self.rider_push();
        let mut blasts = Vec::new();
        let mut ejected = None;
        for (index, cart) in carts.iter_mut().enumerate() {
            let world = self.chunks.world();
            let collider = WorldCollider {
                world,
                blocks: self.items.blocks,
            };
            // Activator rails: a powered one lights a TNT cart, shakes a
            // rider out and shuts a hopper cart.
            let activated = cart
                .rail_under(world)
                .is_some_and(|(_, s)| s.is(blocks::ACTIVATOR_RAIL) && s.bool(Prop::Powered));
            if activated {
                match cart.kind {
                    CartKind::Tnt => cart.ignite(),
                    CartKind::Rideable if cart.rider => {
                        cart.rider = false;
                        if self.riding == Some(index) {
                            self.riding = None;
                            ejected = Some(index);
                        }
                    }
                    CartKind::Hopper => cart.enabled = false,
                    _ => {}
                }
            } else if cart.kind == CartKind::Hopper {
                cart.enabled = true;
            }
            let rider_push = if cart.rider { push } else { DVec3::ZERO };
            if let Some(CartEvent::Explode(power)) = cart.tick(world, &collider, rider_push) {
                blasts.push((index, cart.position + DVec3::Y * 0.35, power));
            }
        }
        // Carts shove one another apart.
        for i in 0..carts.len() {
            for j in i + 1..carts.len() {
                let away = carts[j].position - carts[i].position;
                let flat = DVec3::new(away.x, 0.0, away.z);
                let d = flat.length();
                if d < PUSH_DISTANCE
                    && d > 0.01
                    && (carts[i].position.y - carts[j].position.y).abs() < 0.7
                {
                    let push = flat / d * ((PUSH_DISTANCE - d) * 0.05);
                    carts[i].velocity -= push;
                    carts[j].velocity += push;
                }
            }
        }
        self.carts = carts;
        if let Some(index) = ejected {
            let cart = self.carts[index].clone();
            self.dismount(&cart);
        }
        // Blow up the TNT carts that went off, last first so indices hold.
        for (index, center, power) in blasts.into_iter().rev() {
            if index < self.carts.len() {
                self.carts.remove(index);
                match self.riding {
                    Some(r) if r == index => self.riding = None,
                    Some(r) if r > index => self.riding = Some(r - 1),
                    _ => {}
                }
            }
            self.explode(center, power);
        }
        self.carts.retain(|c| c.position.y > -64.0);
        self.tick_hopper_carts();
        self.close_if_cart_gone();
    }

    /// Hopper carts pull from the container above them and items around.
    fn tick_hopper_carts(&mut self) {
        for index in 0..self.carts.len() {
            let cart = &self.carts[index];
            if cart.kind != CartKind::Hopper || !cart.enabled || cart.cooldown > 0 {
                continue;
            }
            let above = cart.position.floor().as_ivec3() + IVec3::Y;
            let mut moved = false;
            if let Some((slot, one)) = self.take_one_from(above) {
                let items = self.items.clone();
                let slots = &mut self.carts[index].slots;
                let len = slots.len();
                if slots::merge(slots, 0..len, one, &items).is_none() {
                    self.remove_one(above, slot);
                    moved = true;
                }
            }
            if !moved {
                let suck = self.carts[index].aabb().grow(SUCK_MARGIN);
                let mut i = 0;
                while i < self.entities.len() {
                    let item = &self.entities[i];
                    let b = item.bounds();
                    if !(b.min.cmplt(suck.max).all() && b.max.cmpgt(suck.min).all())
                        || item.pickup_delayed()
                    {
                        i += 1;
                        continue;
                    }
                    let stack = item.stack.clone();
                    let items = self.items.clone();
                    let slots = &mut self.carts[index].slots;
                    let len = slots.len();
                    match slots::merge(slots, 0..len, stack.clone(), &items) {
                        None => {
                            self.entities.remove(i);
                            moved = true;
                        }
                        Some(rest) => {
                            if rest.count != stack.count {
                                self.entities[i].stack = rest;
                                moved = true;
                            }
                            i += 1;
                        }
                    }
                }
            }
            if moved {
                self.carts[index].cooldown = HOPPER_COOLDOWN;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use glam::{DVec3, IVec3};

    use super::*;
    use crate::{
        game::tests::flat_game,
        player::mode::GameMode,
        world::{
            block::BlockState,
            props::{Prop, rail_shape},
        },
    };

    const Y: i32 = 64;

    fn ew_rail(id: crate::world::block::BlockId) -> BlockState {
        let state = BlockState::of(id);
        if state.has(Prop::RailShape) {
            state.with(Prop::RailShape, rail_shape::EAST_WEST)
        } else {
            state.with(Prop::StraightRailShape, rail_shape::EAST_WEST)
        }
    }

    fn track(game: &mut Game, z: i32, from: i32, to: i32, id: crate::world::block::BlockId) {
        crate::game::tests::wait_for_chunk(game, from, z);
        crate::game::tests::wait_for_chunk(game, to, z);
        let cells: Vec<_> = (from..=to)
            .map(|x| (IVec3::new(x, Y, z), ew_rail(id)))
            .collect();
        game.set_blocks(&cells);
    }

    #[test]
    fn a_powered_line_carries_a_cart_past_a_detector_that_lights_a_lamp() {
        let mut game = flat_game(GameMode::Creative);
        let z = -10;
        track(&mut game, z, 2, 3, blocks::RAIL);
        track(&mut game, z, 4, 11, blocks::POWERED_RAIL);
        track(&mut game, z, 12, 12, blocks::DETECTOR_RAIL);
        track(&mut game, z, 13, 20, blocks::RAIL);
        game.set_blocks(&[
            (
                IVec3::new(4, Y, z + 1),
                BlockState::of(blocks::REDSTONE_BLOCK),
            ),
            (
                IVec3::new(12, Y, z + 1),
                BlockState::of(blocks::REDSTONE_LAMP),
            ),
            (IVec3::new(21, Y, z), BlockState::of(blocks::STONE)),
        ]);
        game.run_ticks(3);
        assert!(
            game.chunks
                .world()
                .block(11, Y, z)
                .unwrap()
                .bool(Prop::Powered),
            "power runs along the line"
        );
        assert!(game.place_cart(IVec3::new(3, Y, z), CartKind::Rideable));
        game.carts[0].velocity.x = 0.05;
        let mut lit_at = None;
        let lamp = IVec3::new(12, Y, z + 1);
        for tick in 0..120 {
            game.run_ticks(1);
            let lit = game
                .chunks
                .world()
                .block(lamp.x, lamp.y, lamp.z)
                .unwrap()
                .bool(Prop::Lit);
            if lit && lit_at.is_none() {
                lit_at = Some(tick);
            }
        }
        assert!(
            lit_at.is_some(),
            "the detector lit the lamp as the cart passed"
        );
        assert!(
            game.carts[0].position.x > 12.5,
            "{:?}",
            game.carts[0].position
        );
        let lamp_state = game.chunks.world().block(lamp.x, lamp.y, lamp.z).unwrap();
        assert!(
            !lamp_state.bool(Prop::Lit),
            "and it went out once the cart was gone on"
        );
    }

    #[test]
    fn carts_go_on_rails_only_and_the_player_rides_one_until_sneaking() {
        let mut game = flat_game(GameMode::Survival);
        let z = -14;
        track(&mut game, z, 2, 14, blocks::RAIL);
        assert!(!game.place_cart(IVec3::new(5, Y + 3, z), CartKind::Rideable));
        assert!(game.place_cart(IVec3::new(5, Y, z), CartKind::Rideable));
        game.use_cart(0);
        assert!(game.is_riding());
        game.run_ticks(2);
        let cart = game.carts[0].position;
        assert!(
            (game.body.position() - cart).length() < 0.01,
            "carried with it"
        );
        game.carts[0].velocity.x = 0.3;
        game.run_ticks(10);
        assert!(game.body.position().x > 6.0, "{}", game.body.position().x);
        game.keys.sneak = true;
        game.run_ticks(1);
        game.keys.sneak = false;
        assert!(!game.is_riding());
        assert!((game.body.position().x - game.carts[0].position.x).abs() < 2.0);
    }

    #[test]
    fn hitting_a_chest_cart_breaks_it_and_drops_it_and_its_contents() {
        let mut game = flat_game(GameMode::Survival);
        game.entities.clear();
        let z = -16;
        track(&mut game, z, 2, 6, blocks::RAIL);
        assert!(game.place_cart(IVec3::new(4, Y, z), CartKind::Chest));
        game.carts[0].slots[3] = Some(ItemStack::new(items::DIRT, 5));
        game.use_cart(0);
        assert!(matches!(
            game.container,
            Some(crate::game::Container::Cart(0))
        ));
        game.close_container();
        for _ in 0..4 {
            game.hit_cart(0, 1.0);
        }
        assert_eq!(game.carts.len(), 1, "four punches aren't enough");
        game.hit_cart(0, 1.0);
        assert!(game.carts.is_empty());
        let dropped: Vec<_> = game.entities.iter().map(|e| e.stack.id).collect();
        assert!(dropped.contains(&items::CHEST_MINECART) && dropped.contains(&items::DIRT));
    }

    #[test]
    fn an_activator_rail_lights_a_tnt_cart_and_shuts_a_hopper_cart() {
        let mut game = flat_game(GameMode::Creative);
        let z = -18;
        track(&mut game, z, 2, 8, blocks::ACTIVATOR_RAIL);
        game.set_blocks(&[(
            IVec3::new(5, Y, z + 1),
            BlockState::of(blocks::REDSTONE_BLOCK),
        )]);
        game.run_ticks(3);
        assert!(game.place_cart(IVec3::new(5, Y, z), CartKind::Tnt));
        assert!(game.place_cart(IVec3::new(7, Y, z), CartKind::Hopper));
        game.run_ticks(2);
        assert!(game.carts[0].fuse >= 0, "lit");
        assert!(!game.carts[1].enabled, "the hopper cart is switched off");
        game.run_ticks(100);
        assert!(
            game.carts.iter().all(|c| c.kind != CartKind::Tnt),
            "it blew up"
        );
    }

    #[test]
    fn a_hopper_cart_collects_what_lies_above_it() {
        let mut game = flat_game(GameMode::Creative);
        let z = -20;
        track(&mut game, z, 2, 8, blocks::RAIL);
        assert!(game.place_cart(IVec3::new(5, Y, z), CartKind::Hopper));
        game.entities.push(ItemEntity::new(
            ItemStack::new(items::COAL, 2),
            DVec3::new(5.5, 64.4, z as f64 + 0.5),
            DVec3::ZERO,
            0.0,
        ));
        game.run_ticks(10);
        assert!(game.entities.is_empty());
        assert_eq!(game.carts[0].slots[0], Some(ItemStack::new(items::COAL, 2)));
    }
}
