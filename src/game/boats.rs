//! Boats in play: placing one on water (or land), getting in (two seats,
//! one in a chest boat; mobs that bump an empty seat get pushed in),
//! rowing with the movement keys, leaving with sneak, opening a chest
//! boat with sneak and use, and punching one until it breaks into its
//! item (and a chest boat's contents).

use glam::DVec3;

use super::{Container, Game};
use crate::{
    entity::{
        boat::{Boat, BoatKind, Passenger, Rowing},
        item::ItemEntity,
    },
    inventory::stack::ItemStack,
    player::{
        collision::{Aabb, Collider, WorldCollider},
        raycast::{self, Hit},
    },
    world::block::Fluid,
};

/// How far the player's view may turn from the boat's heading (modern
/// `Boat.clampRotation`: 105°).
const LOOK_LIMIT: f32 = 105.0 * std::f32::consts::PI / 180.0;

/// Mobs that never get in a boat: the water animals.
const SWIMMERS: [&str; 7] = [
    "squid",
    "glow_squid",
    "cod",
    "salmon",
    "tropical_fish",
    "pufferfish",
    "dolphin",
];

impl Game {
    /// The boat the player sits in.
    pub fn boat_ridden(&self) -> Option<&Boat> {
        let id = self.boat?;
        self.boats.iter().find(|b| b.id == id)
    }

    /// Boats in the player's dimension.
    pub fn boats_here(&self) -> impl Iterator<Item = &Boat> {
        let here = self.dimension;
        self.boats.iter().filter(move |b| b.dimension == here)
    }

    /// The boat under the crosshair, if it's nearer than the targeted block.
    pub(super) fn target_boat(&self, eye: DVec3, block: Option<&Hit>) -> Option<usize> {
        let dir = self.camera.forward().as_dvec3();
        let block_distance = block.map_or(f64::MAX, |hit| hit.point.distance(eye));
        self.boats
            .iter()
            .enumerate()
            .filter(|(_, b)| b.dimension == self.dimension && Some(b.id) != self.boat)
            .filter_map(|(i, b)| {
                let distance = b.aabb().ray_hit(eye, dir, crate::config::combat::REACH)?;
                (distance < block_distance).then_some((i, distance))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(i, _)| i)
    }

    /// Whether the held item is a boat.
    pub(super) fn holding_boat(&self) -> bool {
        self.held()
            .is_some_and(|stack| BoatKind::of_item(stack.id.name()).is_some())
    }

    /// Puts the held boat where the player looks: on the water's surface,
    /// or on a block (modern `BoatItem.use`). Returns whether it did.
    pub(super) fn place_boat(&mut self, eye: DVec3) -> bool {
        let Some(kind) = self
            .held()
            .and_then(|stack| BoatKind::of_item(stack.id.name()))
        else {
            return false;
        };
        let world = self.chunks.world();
        let dir = self.camera.forward().as_dvec3();
        let stops = |at: glam::IVec3| {
            world.block(at.x, at.y, at.z).is_some_and(|b| {
                let def = b.def();
                def.is_targetable() || def.fluid.is_some()
            })
        };
        let Some(hit) = raycast::raycast(eye, dir, crate::config::REACH, stops) else {
            return false;
        };
        let state = world.block(hit.block.x, hit.block.y, hit.block.z);
        let on_water = state.is_some_and(|b| b.def().fluid == Some(Fluid::Water));
        let at = if on_water {
            let top = crate::entity::boat::water_top(world, hit.block)
                .map_or(hit.point.y, |(top, _)| top);
            DVec3::new(hit.point.x, top, hit.point.z)
        } else if hit.face == Some(crate::world::Face::Top) {
            hit.point
        } else {
            return false;
        };
        // No room if blocks are in the way.
        let bb = crate::entity::boat::aabb_at(at).grow(-0.1);
        let collider = WorldCollider {
            world,
            blocks: self.items.blocks,
        };
        let mut boxes = Vec::new();
        collider.boxes(&bb, &mut boxes);
        if boxes.iter().any(|b| b.intersects(&bb)) {
            return false;
        }
        self.next_boat_id += 1;
        let mut boat = Boat::new(self.next_boat_id, kind, at, self.camera.yaw);
        boat.dimension = self.dimension;
        self.boats.push(boat);
        if self.mode.uses_up_items() {
            self.inventory.take(self.held_slot(), 1);
        }
        true
    }

    /// Right-click on a boat: sit in it, or with sneak open a chest boat.
    pub(super) fn use_boat(&mut self, index: usize) {
        let Some(boat) = self.boats.get(index) else {
            return;
        };
        if self.keys.sneak && boat.kind.chest {
            self.container = Some(Container::Boat(boat.id));
            self.release_input();
            return;
        }
        if self.boat.is_some() || self.is_riding() || !boat.has_room() {
            return;
        }
        let id = boat.id;
        self.boats[index].passengers.push(Passenger::Player);
        self.boat = Some(id);
        self.body.velocity = DVec3::ZERO;
    }

    /// A punch at a boat: it breaks after enough hits (at once in creative).
    pub(super) fn hit_boat(&mut self, index: usize, damage: f32) {
        let Some(boat) = self.boats.get_mut(index) else {
            return;
        };
        if boat.hurt(damage.max(1.0)) || !self.mode.drops_items() {
            self.break_boat(index);
        }
    }

    /// Removes a boat, dropping it and a chest boat's contents.
    pub(super) fn break_boat(&mut self, index: usize) {
        if index >= self.boats.len() {
            return;
        }
        let boat = self.boats.remove(index);
        if self.boat == Some(boat.id) {
            self.boat = None;
            self.step_out(&boat);
        }
        if matches!(self.container, Some(Container::Boat(id)) if id == boat.id) {
            self.close_container();
        }
        let rng = &mut self.rng;
        let mut random = || rng.next_f64() as f32;
        let center = boat.position + DVec3::Y * 0.3;
        let mut dropped: Vec<ItemStack> = boat.slots.iter().flatten().cloned().collect();
        if self.mode.drops_items()
            && let Some(id) = crate::inventory::items::by_name(&boat.kind.item_name())
        {
            dropped.push(ItemStack::new(id, 1));
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

    /// The player's tick in a boat: sneak gets out.
    pub(super) fn ride_boat(&mut self) {
        let Some(index) = self
            .boat
            .and_then(|id| self.boats.iter().position(|b| b.id == id))
        else {
            self.boat = None;
            return;
        };
        if self.keys.sneak {
            let boat = self.boats[index].clone();
            self.boats[index]
                .passengers
                .retain(|p| *p != Passenger::Player);
            self.boat = None;
            self.step_out(&boat);
            return;
        }
        self.body.velocity = DVec3::ZERO;
    }

    /// Stands the player on a free spot beside `boat` (or on top of it).
    fn step_out(&mut self, boat: &Boat) {
        let world = self.chunks.world();
        let collider = WorldCollider {
            world,
            blocks: self.items.blocks,
        };
        let size = self.body.aabb();
        let half = (size.max - size.min) / 2.0;
        let free = |feet: DVec3| {
            let aabb = Aabb::new(
                feet - DVec3::new(half.x, 0.0, half.z),
                feet + DVec3::new(half.x, size.max.y - size.min.y, half.z),
            );
            let mut boxes = Vec::new();
            collider.boxes(&aabb, &mut boxes);
            !boxes.iter().any(|b| b.intersects(&aabb))
        };
        let side = DVec3::new(boat.forward().z, 0.0, -boat.forward().x);
        let feet = [
            side * 1.2,
            -side * 1.2,
            boat.forward() * 1.4,
            -boat.forward() * 1.4,
        ]
        .into_iter()
        .map(|offset| boat.position + offset)
        .find(|&p| free(p))
        .unwrap_or(boat.position + DVec3::Y * crate::entity::boat::HEIGHT);
        self.body.set_position(feet);
        self.prev_feet = feet;
        self.body.velocity = DVec3::ZERO;
    }

    /// One tick of every boat here: the player rows the one they sit in
    /// front of, mobs bump into empty seats, and passengers ride along.
    pub(super) fn tick_boats(&mut self) {
        if self.boats.is_empty() {
            return;
        }
        let keys = &self.keys;
        let rowing = Rowing {
            forward: keys.forward,
            back: keys.back,
            left: keys.left,
            right: keys.right,
        };
        let mut ejected = Vec::new();
        for index in 0..self.boats.len() {
            if self.boats[index].dimension != self.dimension {
                continue;
            }
            let world = self.chunks.world();
            let collider = WorldCollider {
                world,
                blocks: self.items.blocks,
            };
            let boat = &mut self.boats[index];
            let rower = boat.passengers.first() == Some(&Passenger::Player);
            let sunk = boat.tick(world, &collider, rower.then_some(rowing));
            if sunk && !boat.passengers.is_empty() {
                ejected.push(index);
            }
        }
        for index in ejected {
            let boat = self.boats[index].clone();
            self.boats[index].passengers.clear();
            if self.boat == Some(boat.id) {
                self.boat = None;
                self.step_out(&boat);
            }
        }
        self.board_mobs();
        self.carry_passengers();
    }

    /// Mobs that bump into a boat with a free seat get in (modern
    /// `Boat.push`): any but players, water animals and those too wide.
    fn board_mobs(&mut self) {
        let riding: Vec<u64> = self
            .boats
            .iter()
            .flat_map(|b| &b.passengers)
            .filter_map(|p| match p {
                Passenger::Mob(id) => Some(*id),
                Passenger::Player => None,
            })
            .collect();
        for index in 0..self.boats.len() {
            if self.boats[index].dimension != self.dimension || !self.boats[index].has_room() {
                continue;
            }
            let reach = self.boats[index].aabb().grow(0.2);
            let joining = self.mobs.iter().find(|mob| {
                let def = self.mob_defs.get(mob.kind);
                !mob.is_dead()
                    && !riding.contains(&mob.id)
                    && def.width < crate::entity::boat::WIDTH
                    && !SWIMMERS.contains(&def.name.as_str())
                    && mob.body.aabb().intersects(&reach)
            });
            if let Some(mob) = joining {
                let id = mob.id;
                self.boats[index].passengers.push(Passenger::Mob(id));
                return;
            }
        }
    }

    /// Sits every passenger in their seat, turns the rower's view with the
    /// boat, and lets go of mobs that died or wandered off.
    fn carry_passengers(&mut self) {
        let player_boat = self.boat;
        for index in 0..self.boats.len() {
            let alive = |id: u64| self.mobs.iter().any(|m| m.id == id && !m.is_dead());
            let boat = &mut self.boats[index];
            boat.passengers.retain(|p| match p {
                Passenger::Mob(id) => alive(*id),
                Passenger::Player => player_boat == Some(boat.id),
            });
            let boat = self.boats[index].clone();
            for (seat, passenger) in boat.passengers.iter().enumerate() {
                match *passenger {
                    Passenger::Player => {
                        let feet = boat.seat(seat, false);
                        self.body.set_position(feet);
                        self.body.velocity = DVec3::ZERO;
                        self.camera.yaw += boat.yaw - boat.prev_yaw;
                        let off = wrap(self.camera.yaw - boat.yaw);
                        self.camera.yaw = boat.yaw + off.clamp(-LOOK_LIMIT, LOOK_LIMIT);
                    }
                    Passenger::Mob(id) => {
                        if let Some(mob) = self.mobs.iter_mut().find(|m| m.id == id) {
                            mob.body.set_position(boat.seat(seat, true));
                            mob.body.velocity = DVec3::ZERO;
                        }
                    }
                }
            }
        }
    }

    /// The boats as saved.
    pub(super) fn saved_boats(&self) -> Vec<crate::entity::boat::SavedBoat> {
        self.boats.iter().map(Boat::saved).collect()
    }

    /// Puts saved boats back.
    pub(super) fn load_boats(&mut self, saved: &[crate::entity::boat::SavedBoat]) {
        for s in saved {
            self.next_boat_id += 1;
            if let Some(boat) = Boat::from_saved(self.next_boat_id, s) {
                self.boats.push(boat);
            }
        }
    }
}

/// An angle brought into −π..π.
fn wrap(angle: f32) -> f32 {
    use std::f32::consts::{PI, TAU};
    (angle + PI).rem_euclid(TAU) - PI
}

#[cfg(test)]
mod tests {
    use glam::{DVec3, IVec3};

    use super::*;
    use crate::{
        game::tests::flat_game,
        inventory::{items, stack::ItemStack},
        player::mode::GameMode,
        world::{block::BlockState, blocks},
    };

    /// A survival game with a pool of still water at y 63 just ahead.
    fn pool() -> Game {
        let mut game = flat_game(GameMode::Survival);
        game.mob_defs = crate::game::tests::mob_registry();
        let water: Vec<_> = (-4..5)
            .flat_map(|x| {
                (-14..0).map(move |z| (IVec3::new(x, 63, z), BlockState::of(blocks::WATER)))
            })
            .collect();
        game.chunks.set_blocks(&water);
        game
    }

    fn put_boat(game: &mut Game, item: &str) -> usize {
        game.inventory
            .set(0, Some(ItemStack::new(items::by_name(item).unwrap(), 1)));
        game.inventory.selected = 0;
        game.camera.yaw = 0.0;
        game.camera.pitch = -0.6;
        let eye = game.body.position() + DVec3::Y * game.body.eye_height();
        assert!(game.place_boat(eye), "placed");
        game.boats.len() - 1
    }

    #[test]
    fn a_boat_goes_on_the_water_and_is_used_up() {
        let mut game = pool();
        let index = put_boat(&mut game, "oak_boat");
        let boat = &game.boats[index];
        assert!((boat.position.y - (63.0 + 8.0 / 9.0)).abs() < 1e-9);
        assert!(game.inventory.slot(0).is_none());
        assert_eq!(boat.kind.item_name(), "oak_boat");
    }

    #[test]
    fn rowing_carries_the_player_and_sneak_gets_out() {
        let mut game = pool();
        let index = put_boat(&mut game, "spruce_boat");
        game.use_boat(index);
        assert!(game.boat_ridden().is_some());
        let start = game.boats[index].position;
        game.keys.forward = true;
        for _ in 0..30 {
            game.tick();
        }
        game.keys.forward = false;
        let boat = game.boat_ridden().unwrap().clone();
        assert!(
            boat.position.z < start.z - 3.0,
            "rowed north: {start} → {}",
            boat.position
        );
        assert!(
            game.body.position().distance(boat.position) < 1.0,
            "the player rides along"
        );
        game.keys.sneak = true;
        game.tick();
        game.keys.sneak = false;
        assert!(game.boat_ridden().is_none());
        assert!(
            game.body.position().distance(boat.position) > 0.9,
            "stepped out"
        );
    }

    #[test]
    fn punching_breaks_it_into_its_item_and_its_chests_contents() {
        let mut game = pool();
        let index = put_boat(&mut game, "birch_chest_boat");
        game.boats[index].slots[3] = Some(ItemStack::new(items::DIAMOND, 4));
        for _ in 0..4 {
            game.hit_boat(index, 1.0);
        }
        assert_eq!(game.boats.len(), 1);
        game.hit_boat(index, 1.0);
        assert!(game.boats.is_empty());
        let dropped: Vec<_> = game.entities.iter().map(|e| e.stack.id).collect();
        assert!(dropped.contains(&items::DIAMOND));
        assert!(dropped.contains(&items::by_name("birch_chest_boat").unwrap()));
    }

    #[test]
    fn a_chest_boat_opens_with_sneak_and_seats_one() {
        let mut game = pool();
        let index = put_boat(&mut game, "oak_chest_boat");
        game.keys.sneak = true;
        game.use_boat(index);
        assert!(matches!(game.container, Some(Container::Boat(_))));
        game.close_container();
        game.keys.sneak = false;
        game.use_boat(index);
        assert!(!game.boats[index].has_room(), "one seat");
    }

    #[test]
    fn a_pig_bumping_an_empty_boat_gets_in_and_boats_save() {
        let mut game = pool();
        let index = put_boat(&mut game, "oak_boat");
        let at = game.boats[index].position;
        let kind = game.mob_defs.find("pig").unwrap();
        let pig = crate::entity::mob::Mob::new(kind, game.mob_defs.get(kind), at, 0.0);
        game.add_mob(pig);
        game.tick();
        assert!(matches!(
            game.boats[index].passengers[..],
            [Passenger::Mob(_)]
        ));
        let saved = game.saved_boats();
        assert_eq!(saved.len(), 1);
        let mut other = flat_game(GameMode::Survival);
        other.load_boats(&saved);
        assert_eq!(other.boats[0].kind, game.boats[index].kind);
    }
}
