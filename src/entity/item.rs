//! Dropped items, following 1.0.0's `EntityItem`: they fall, slide and
//! bounce, can be picked up after a short delay, and vanish after five
//! minutes.

use glam::{DVec3, IVec3};

use crate::{
    inventory::{
        player::Inventory,
        stack::{ItemStack, Items},
    },
    player::{
        collision::{Aabb, Collider},
        physics::{CURRENT_PUSH, sweep},
    },
};

/// Edge length of the item's box.
const SIZE: f64 = 0.25;
/// Per tick, as in 1.0.0.
const GRAVITY: f64 = 0.04;
const DRAG: f64 = 0.98;
/// Vertical speed kept, reversed, on hitting the ground.
const BOUNCE: f64 = -0.5;
/// Ticks before it disappears: 5 minutes.
const LIFETIME: u32 = 6000;
/// Ticks before a broken block's drop can be picked up.
const BLOCK_DROP_DELAY: u32 = 10;
/// Ticks before an item the player threw can be picked back up.
const THROW_DELAY: u32 = 40;
/// An item's health (1.0.0's `EntityItem.health`), and what lava and fire
/// take from it each tick.
const HEALTH: i32 = 5;
const LAVA_DAMAGE: i32 = 4;
const FIRE_DAMAGE: i32 = 1;
/// Items this close sideways to the player's box are picked up
/// (1.0.0's `boundingBox.expand(1, 0, 1)`).
const PICKUP_REACH: f64 = 1.0;

#[derive(Debug, Clone, PartialEq)]
pub struct ItemEntity {
    pub stack: ItemStack,
    bb: Aabb,
    /// Center at the previous tick, for smooth drawing between ticks.
    prev_center: DVec3,
    pub velocity: DVec3,
    pub on_ground: bool,
    /// Ticks since it was dropped.
    pub age: u32,
    pickup_delay: u32,
    /// Random phase for the spin and bob when drawn (1.0.0's `hoverStart`).
    pub hover: f32,
    /// Burns away at 0.
    health: i32,
}

impl ItemEntity {
    /// An item centered on `center`.
    pub fn new(stack: ItemStack, center: DVec3, velocity: DVec3, hover: f32) -> Self {
        let half = DVec3::splat(SIZE / 2.0);
        Self {
            stack,
            bb: Aabb::new(center - half, center + half),
            prev_center: center,
            velocity,
            on_ground: false,
            age: 0,
            pickup_delay: 0,
            hover,
            health: HEALTH,
        }
    }

    /// A drop from the block at `cell`: somewhere in its middle, popping up
    /// (1.0.0's `dropBlockAsItem_do` and `EntityItem` constructor).
    pub fn from_block(stack: ItemStack, cell: IVec3, random: &mut impl FnMut() -> f32) -> Self {
        let spread = 0.7;
        let mut offset = || random() as f64 * spread + (1.0 - spread) / 2.0;
        let center = cell.as_dvec3() + DVec3::new(offset(), offset(), offset());
        let velocity = DVec3::new(
            random() as f64 * 0.2 - 0.1,
            0.2,
            random() as f64 * 0.2 - 0.1,
        );
        let mut item = Self::new(stack, center, velocity, random() * std::f32::consts::TAU);
        item.pickup_delay = BLOCK_DROP_DELAY;
        item
    }

    /// Thrown by the player from `eye` along `forward` (a unit vector), as
    /// 1.0.0's `dropPlayerItemWithRandomChoice` does.
    pub fn thrown(
        stack: ItemStack,
        eye: DVec3,
        forward: DVec3,
        random: &mut impl FnMut() -> f32,
    ) -> Self {
        let center = eye - DVec3::Y * 0.3;
        let mut velocity = forward * 0.3 + DVec3::Y * 0.1;
        let angle = random() as f64 * std::f64::consts::TAU;
        let spread = 0.02 * random() as f64;
        velocity += DVec3::new(
            angle.cos() * spread,
            (random() - random()) as f64 * 0.1,
            angle.sin() * spread,
        );
        let mut item = Self::new(stack, center, velocity, random() * std::f32::consts::TAU);
        item.pickup_delay = THROW_DELAY;
        item
    }

    /// Dropped at death: tossed from the eyes in a random direction
    /// (1.0.0's `dropPlayerItemWithRandomChoice` with random choice).
    pub fn scattered(stack: ItemStack, eye: DVec3, random: &mut impl FnMut() -> f32) -> Self {
        let speed = random() as f64 * 0.5;
        let angle = random() as f64 * std::f64::consts::TAU;
        let velocity = DVec3::new(-angle.sin() * speed, 0.2, angle.cos() * speed);
        let mut item = Self::new(
            stack,
            eye - DVec3::Y * 0.3,
            velocity,
            random() * std::f32::consts::TAU,
        );
        item.pickup_delay = THROW_DELAY;
        item
    }

    /// The box it fills.
    pub fn bounds(&self) -> Aabb {
        self.bb
    }

    /// Moves it by `by`, as a piston does.
    pub fn shift(&mut self, by: DVec3) {
        self.bb = self.bb.offset(by);
        self.prev_center += by;
    }

    pub fn center(&self) -> DVec3 {
        (self.bb.min + self.bb.max) / 2.0
    }

    /// Center `alpha` of the way from the previous tick to this one.
    pub fn center_between_ticks(&self, alpha: f64) -> DVec3 {
        self.prev_center.lerp(self.center(), alpha)
    }

    pub fn expired(&self) -> bool {
        self.age >= LIFETIME || self.stack.count == 0
    }

    /// One tick of 1.0.0's `EntityItem.onUpdate`, burning in lava and fire.
    pub fn tick(&mut self, world: &impl Collider) {
        self.prev_center = self.center();
        // Water carries items along while their box touches it.
        let region = self.bb.grow(-0.001);
        self.velocity += world.current(&region) * CURRENT_PUSH;
        self.velocity.y -= GRAVITY;
        let wanted = self.velocity;
        let (moved, bb) = sweep(self.bb, wanted, world, &mut Vec::new());
        self.bb = bb;
        self.on_ground = moved.y != wanted.y && wanted.y < 0.0;
        for axis in 0..3 {
            if moved[axis] != wanted[axis] {
                self.velocity[axis] = 0.0;
            }
        }
        let mut friction = DRAG;
        if self.on_ground {
            let below = (self.center() - DVec3::Y * (SIZE / 2.0 + 0.01)).floor();
            let (x, y, z) = (below.x as i32, below.y as i32, below.z as i32);
            friction = world.slipperiness(x, y, z) as f64 * DRAG;
        }
        self.velocity *= DVec3::new(friction, DRAG, friction);
        if self.on_ground {
            self.velocity.y *= BOUNCE;
        }
        self.age += 1;
        self.pickup_delay = self.pickup_delay.saturating_sub(1);
        // Lava and fire burn it up.
        if world.in_lava(&self.bb) {
            self.health -= LAVA_DAMAGE;
        } else if world.in_fire(&self.bb) {
            self.health -= FIRE_DAMAGE;
        }
        if self.health <= 0 {
            self.stack.count = 0;
        }
    }

    /// Moves as much of the stack as fits into `inventory` if the item is
    /// close to the player's box `player` and its delay is over. Returns
    /// true if anything was picked up.
    pub fn pick_up(&mut self, player: &Aabb, inventory: &mut Inventory, items: &Items) -> bool {
        let reach = Aabb::new(
            player.min - DVec3::new(PICKUP_REACH, 0.0, PICKUP_REACH),
            player.max + DVec3::new(PICKUP_REACH, 0.0, PICKUP_REACH),
        );
        if self.pickup_delay > 0 || self.stack.count == 0 || !reach.intersects(&self.bb) {
            return false;
        }
        let before = self.stack.count;
        self.stack.count = inventory
            .add(self.stack, items)
            .map_or(0, |rest| rest.count);
        self.stack.count < before
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inventory::items;
    use crate::player::collision::WorldCollider;
    use crate::{
        player::physics::hitbox,
        world::{World, chunk::ChunkPos, worldgen::flat::flat_chunk},
    };

    struct Scene {
        world: World,
        items: Items,
    }

    impl Scene {
        /// A flat world with grass at y 63.
        fn new() -> Self {
            let mut world = World::default();
            for z in -1..=1 {
                for x in -1..=1 {
                    world.insert(flat_chunk(ChunkPos::new(x, z)));
                }
            }
            Self {
                world,
                items: Items::new(),
            }
        }

        fn run(&self, item: &mut ItemEntity, ticks: u32) {
            let collider = WorldCollider {
                world: &self.world,
                blocks: self.items.blocks,
            };
            for _ in 0..ticks {
                item.tick(&collider);
            }
        }
    }

    const COBBLESTONE: ItemStack = ItemStack {
        id: items::COBBLESTONE,
        count: 1,
        damage: 0,
    };

    #[test]
    fn a_block_drop_pops_up_falls_and_comes_to_rest_on_the_ground() {
        let scene = Scene::new();
        let mut random = || 0.5;
        // Dropped from the grass block itself, just broken.
        let mut item = ItemEntity::from_block(COBBLESTONE, IVec3::new(0, 64, 0), &mut random);
        let start = item.center();
        assert!((start - DVec3::new(0.5, 64.5, 0.5)).length() < 1e-9);
        scene.run(&mut item, 3);
        assert!(item.center().y > start.y, "it pops up first");
        scene.run(&mut item, 60);
        assert!(item.on_ground);
        assert!(
            (item.center().y - (64.0 + SIZE / 2.0)).abs() < 1e-9,
            "resting on the grass"
        );
        assert!(item.velocity.length() < 1e-3);
    }

    #[test]
    fn items_slide_to_a_stop() {
        let scene = Scene::new();
        let mut item = ItemEntity::new(
            COBBLESTONE,
            DVec3::new(0.5, 64.125, 0.5),
            DVec3::new(0.3, 0.0, 0.0),
            0.0,
        );
        scene.run(&mut item, 40);
        let x = item.center().x;
        assert!(x > 0.6 && x < 1.5, "slid a short way: {x}");
        assert!(item.velocity.x.abs() < 1e-6);
    }

    #[test]
    fn pickup_waits_for_the_delay_and_takes_what_fits() {
        let scene = Scene::new();
        let player = hitbox(DVec3::new(0.5, 64.0, 0.5));
        let mut inventory = Inventory::default();
        let mut random = || 0.5;
        let stack = ItemStack::new(items::COBBLESTONE, 10);
        let mut item = ItemEntity::from_block(stack, IVec3::new(1, 64, 0), &mut random);
        assert!(
            !item.pick_up(&player, &mut inventory, &scene.items),
            "too soon"
        );
        scene.run(&mut item, BLOCK_DROP_DELAY);
        assert!(item.pick_up(&player, &mut inventory, &scene.items));
        assert!(item.expired());
        assert_eq!(inventory.slot(0), Some(&stack));

        // A full inventory takes only part of a stack.
        for i in 1..36 {
            inventory.set(i, Some(ItemStack::new(items::DIRT, 64)));
        }
        inventory.set(0, Some(ItemStack::new(items::COBBLESTONE, 60)));
        let mut item = ItemEntity::new(stack, DVec3::new(0.5, 64.2, 0.5), DVec3::ZERO, 0.0);
        assert!(item.pick_up(&player, &mut inventory, &scene.items));
        assert_eq!((item.stack.count, item.expired()), (6, false));
    }

    #[test]
    fn far_items_are_not_picked_up_and_old_ones_vanish() {
        let scene = Scene::new();
        let player = hitbox(DVec3::new(0.5, 64.0, 0.5));
        let mut inventory = Inventory::default();
        let mut item = ItemEntity::new(COBBLESTONE, DVec3::new(3.0, 64.2, 0.5), DVec3::ZERO, 0.0);
        assert!(!item.pick_up(&player, &mut inventory, &scene.items));
        scene.run(&mut item, LIFETIME - 1);
        assert!(!item.expired());
        scene.run(&mut item, 1);
        assert!(item.expired());
    }

    #[test]
    fn thrown_items_fly_forward_and_wait_longer() {
        let scene = Scene::new();
        let eye = DVec3::new(0.5, 65.62, 0.5);
        let mut random = || 0.5;
        let mut item = ItemEntity::thrown(COBBLESTONE, eye, DVec3::NEG_Z, &mut random);
        let player = hitbox(DVec3::new(0.5, 64.0, 0.5));
        let mut inventory = Inventory::default();
        scene.run(&mut item, BLOCK_DROP_DELAY);
        assert!(!item.pick_up(&player, &mut inventory, &scene.items));
        scene.run(&mut item, 30);
        assert!(item.center().z < -1.0, "landed in front: {}", item.center());
    }
}
