//! Minecarts, following modern Minecraft's `AbstractMinecart` on rails:
//! a cart follows the rail it is on, speeds up going down slopes and on
//! powered rails, slows with 4% friction, tops out at 0.4 blocks a tick,
//! and off rails it slides and falls. The rideable cart, the chest cart,
//! the hopper cart and the TNT cart share all of that.

use glam::{DVec3, IVec3};

use crate::{
    inventory::stack::ItemStack,
    player::collision::{Aabb, Collider},
    world::{
        World,
        block::BlockState,
        blocks,
        props::Prop,
        rails::{is_rail, shape_of},
    },
};

/// Fastest a cart goes along rails, blocks per tick.
pub const MAX_SPEED: f64 = 0.4;
const GRAVITY: f64 = 0.04;
const WIDTH: f64 = 0.98;
const HEIGHT: f64 = 0.7;
/// Where on the rail's cell a cart sits, above its floor.
const RAIL_LIFT: f64 = 0.0625;
/// Damage a hit adds is the hit's damage times this; over `BREAK_AT`
/// breaks it.
pub const HIT_DAMAGE: f32 = 10.0;
const BREAK_AT: f32 = 40.0;
/// Ticks a lit TNT cart burns.
pub const TNT_FUSE: i32 = 80;
/// A hopper cart's pause after taking something.
pub const HOPPER_COOLDOWN: u8 = 4;
/// Slots of the chest and hopper carts.
pub const CHEST_SLOTS: usize = 27;
pub const HOPPER_SLOTS: usize = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CartKind {
    Rideable,
    Chest,
    Hopper,
    Tnt,
}

impl CartKind {
    pub fn slot_count(self) -> usize {
        match self {
            CartKind::Chest => CHEST_SLOTS,
            CartKind::Hopper => HOPPER_SLOTS,
            _ => 0,
        }
    }
}

/// What a cart did this tick that the game must carry out.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CartEvent {
    /// A TNT cart blows with this power.
    Explode(f32),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Minecart {
    pub kind: CartKind,
    /// The middle of the bottom of the box.
    pub position: DVec3,
    pub previous: DVec3,
    pub velocity: DVec3,
    /// Direction of travel on the ground plane, radians (`atan2(dz, dx)`).
    pub yaw: f32,
    pub on_rail: bool,
    pub on_ground: bool,
    /// Whether it ran into a block sideways this tick.
    pub bumped: bool,
    /// Its squared speed along the ground going into the last move.
    impact: f64,
    /// The slots of a chest or hopper cart.
    pub slots: Vec<Option<ItemStack>>,
    /// Damage taken, which breaks it over 40.
    pub damage: f32,
    /// Ticks of hit shaking left.
    pub wobble: u8,
    /// Whether the player sits in it.
    pub rider: bool,
    /// Ticks until a lit TNT cart blows; negative if unlit.
    pub fuse: i32,
    /// A hopper cart's pause, and whether an activator rail turned it off.
    pub cooldown: u8,
    pub enabled: bool,
}

/// The ends of each rail shape as offsets (x, y, z) from the rail's
/// middle, in modern `AbstractMinecart.EXITS` order.
fn exits(shape: u8) -> [IVec3; 2] {
    let v = |x, y, z| IVec3::new(x, y, z);
    match shape {
        0 => [v(0, 0, -1), v(0, 0, 1)],
        1 => [v(-1, 0, 0), v(1, 0, 0)],
        2 => [v(-1, 0, 0), v(1, 1, 0)],
        3 => [v(-1, 1, 0), v(1, 0, 0)],
        4 => [v(0, 1, -1), v(0, 0, 1)],
        5 => [v(0, 0, -1), v(0, 1, 1)],
        6 => [v(0, 0, 1), v(1, 0, 0)],
        7 => [v(0, 0, 1), v(-1, 0, 0)],
        8 => [v(0, 0, -1), v(-1, 0, 0)],
        _ => [v(0, 0, -1), v(1, 0, 0)],
    }
}

impl Minecart {
    pub fn new(kind: CartKind, position: DVec3) -> Self {
        Self {
            kind,
            position,
            previous: position,
            velocity: DVec3::ZERO,
            yaw: 0.0,
            on_rail: false,
            on_ground: false,
            bumped: false,
            impact: 0.0,
            slots: vec![None; kind.slot_count()],
            damage: 0.0,
            wobble: 0,
            rider: false,
            fuse: -1,
            cooldown: 0,
            enabled: true,
        }
    }

    pub fn aabb(&self) -> Aabb {
        let half = WIDTH / 2.0;
        Aabb::new(
            self.position - DVec3::new(half, 0.0, half),
            self.position + DVec3::new(half, HEIGHT, half),
        )
    }

    pub fn between_ticks(&self, alpha: f64) -> DVec3 {
        self.previous.lerp(self.position, alpha)
    }

    /// The cell of the rail under the cart and its state: the cell below
    /// first, as modern Minecraft looks.
    pub fn rail_under(&self, world: &World) -> Option<(IVec3, BlockState)> {
        let cell = self.position.floor().as_ivec3();
        [cell - IVec3::Y, cell].into_iter().find_map(|p| {
            let state = world.block(p.x, p.y, p.z)?;
            is_rail(state).then_some((p, state))
        })
    }

    /// Hits it for `amount`. Returns true if that breaks it.
    pub fn hurt(&mut self, amount: f32) -> bool {
        self.damage += amount * HIT_DAMAGE;
        self.wobble = 10;
        self.damage > BREAK_AT
    }

    /// Lights a TNT cart.
    pub fn ignite(&mut self) {
        if self.kind == CartKind::Tnt && self.fuse < 0 {
            self.fuse = TNT_FUSE;
        }
    }

    /// The point on the rail at `cell` closest to where the cart is (modern
    /// `getPos`): the rail's line, with the cart's height along it.
    fn point_on_rail(&self, cell: IVec3, shape: u8, at: DVec3) -> DVec3 {
        let [first, second] = exits(shape);
        let (x, y, z) = (cell.x as f64, cell.y as f64, cell.z as f64);
        let a = DVec3::new(
            x + 0.5 + first.x as f64 * 0.5,
            y + RAIL_LIFT + first.y as f64 * 0.5,
            z + 0.5 + first.z as f64 * 0.5,
        );
        let b = DVec3::new(
            x + 0.5 + second.x as f64 * 0.5,
            y + RAIL_LIFT + second.y as f64 * 0.5,
            z + 0.5 + second.z as f64 * 0.5,
        );
        let (dx, dz) = (b.x - a.x, b.z - a.z);
        let rise = (b.y - a.y) * 2.0;
        let t = if dx == 0.0 {
            at.z - z
        } else if dz == 0.0 {
            at.x - x
        } else {
            ((at.x - a.x) * dx + (at.z - a.z) * dz) * 2.0
        };
        let mut y = a.y + rise * t;
        if rise < 0.0 {
            y += 1.0;
        } else if rise > 0.0 {
            y += 0.5;
        }
        DVec3::new(a.x + dx * t, y, a.z + dz * t)
    }

    /// One tick. `push` is a rider's push along the ground. Returns what
    /// the game has to do about it.
    pub fn tick(
        &mut self,
        world: &World,
        collider: &impl Collider,
        push: DVec3,
    ) -> Option<CartEvent> {
        self.previous = self.position;
        self.wobble = self.wobble.saturating_sub(1);
        self.damage = (self.damage - 0.2).max(0.0);
        self.cooldown = self.cooldown.saturating_sub(1);
        let mut event = None;
        if self.kind == CartKind::Tnt && self.fuse >= 0 {
            self.fuse -= 1;
            if self.fuse < 0 {
                event = Some(CartEvent::Explode(self.blast_power()));
            }
        }
        self.velocity.y -= GRAVITY;
        match self.rail_under(world) {
            Some((cell, state)) => {
                self.on_rail = true;
                self.follow(world, collider, cell, state, push);
            }
            None => {
                self.on_rail = false;
                self.come_off_rails(collider);
            }
        }
        let moved = self.position - self.previous;
        if moved.x * moved.x + moved.z * moved.z > 0.001 {
            let mut yaw = moved.z.atan2(moved.x) as f32;
            // A cart looks the same from either end; keep its yaw within
            // a half turn so it doesn't spin about when it reverses.
            if yaw > std::f32::consts::FRAC_PI_2 {
                yaw -= std::f32::consts::PI;
            } else if yaw <= -std::f32::consts::FRAC_PI_2 {
                yaw += std::f32::consts::PI;
            }
            self.yaw = yaw;
        }
        // A TNT cart that hits something hard blows up.
        if self.kind == CartKind::Tnt && event.is_none() && self.bumped && self.impact >= 0.01 {
            self.fuse = -1;
            event = Some(CartEvent::Explode(self.blast_power()));
        }
        event
    }

    /// The blast of a TNT cart: 4 and more the faster it goes.
    fn blast_power(&self) -> f32 {
        let speed = (self.velocity.x * self.velocity.x + self.velocity.z * self.velocity.z)
            .sqrt()
            .min(5.0) as f32;
        4.0 + 0.75 * speed
    }

    fn follow(
        &mut self,
        world: &World,
        collider: &impl Collider,
        cell: IVec3,
        state: BlockState,
        push: DVec3,
    ) {
        let shape = shape_of(state);
        let before = self.point_on_rail(cell, shape, self.position);
        let mut y = cell.y as f64;
        let mut brakes = false;
        let mut boosts = false;
        if state.is(blocks::POWERED_RAIL) {
            boosts = state.bool(Prop::Powered);
            brakes = !boosts;
        }
        let slope = 0.0078125;
        match shape {
            2 => {
                self.velocity.x -= slope;
                y += 1.0;
            }
            3 => {
                self.velocity.x += slope;
                y += 1.0;
            }
            4 => {
                self.velocity.z += slope;
                y += 1.0;
            }
            5 => {
                self.velocity.z -= slope;
                y += 1.0;
            }
            _ => {}
        }
        let [first, second] = exits(shape);
        let mut dx = f64::from(second.x - first.x);
        let mut dz = f64::from(second.z - first.z);
        let length = (dx * dx + dz * dz).sqrt();
        if self.velocity.x * dx + self.velocity.z * dz < 0.0 {
            dx = -dx;
            dz = -dz;
        }
        let speed = self.velocity.x.hypot(self.velocity.z).min(2.0);
        self.velocity.x = speed * dx / length;
        self.velocity.z = speed * dz / length;
        // A rider's own push gets a slow cart going.
        if self.rider {
            let own = push.x * push.x + push.z * push.z;
            let now = self.velocity.x * self.velocity.x + self.velocity.z * self.velocity.z;
            if own > 1.0e-4 && now < 0.01 {
                self.velocity.x += push.x * 0.1;
                self.velocity.z += push.z * 0.1;
                brakes = false;
            }
        }
        if brakes {
            // An unpowered powered rail stops a slow cart and slows others.
            if self.velocity.x.hypot(self.velocity.z) < 0.03 {
                self.velocity = DVec3::ZERO;
            } else {
                self.velocity.x *= 0.5;
                self.velocity.z *= 0.5;
                self.velocity.y = 0.0;
            }
        }
        let (cx, cz) = (cell.x as f64 + 0.5, cell.z as f64 + 0.5);
        let from = DVec3::new(cx + first.x as f64 * 0.5, 0.0, cz + first.z as f64 * 0.5);
        let to = DVec3::new(cx + second.x as f64 * 0.5, 0.0, cz + second.z as f64 * 0.5);
        let (ax, az) = (to.x - from.x, to.z - from.z);
        let t = if ax == 0.0 {
            self.position.z - cell.z as f64
        } else if az == 0.0 {
            self.position.x - cell.x as f64
        } else {
            ((self.position.x - from.x) * ax + (self.position.z - from.z) * az) * 2.0
        };
        self.position = DVec3::new(from.x + ax * t, y, from.z + az * t);
        let factor = if self.rider { 0.75 } else { 1.0 };
        let wanted = DVec3::new(
            (factor * self.velocity.x).clamp(-MAX_SPEED, MAX_SPEED),
            0.0,
            (factor * self.velocity.z).clamp(-MAX_SPEED, MAX_SPEED),
        );
        self.slide(collider, wanted);
        // Reaching the end of a slope lifts the cart a block.
        let (fx, fz) = (
            self.position.x.floor() as i32 - cell.x,
            self.position.z.floor() as i32 - cell.z,
        );
        if first.y != 0 && fx == first.x && fz == first.z {
            self.position.y += f64::from(first.y);
        } else if second.y != 0 && fx == second.x && fz == second.z {
            self.position.y += f64::from(second.y);
        }
        let drag = if self.rider { 0.997 } else { 0.96 };
        self.velocity.x *= drag;
        self.velocity.z *= drag;
        self.velocity.y = 0.0;
        let after = self.point_on_rail(cell, shape, self.position);
        let climb = (before.y - after.y) * 0.05;
        let h = self.velocity.x.hypot(self.velocity.z);
        if h > 0.0 {
            let scale = (h + climb) / h;
            self.velocity.x *= scale;
            self.velocity.z *= scale;
        }
        self.position.y = after.y;
        let (nx, nz) = (
            self.position.x.floor() as i32,
            self.position.z.floor() as i32,
        );
        if nx != cell.x || nz != cell.z {
            let h = self.velocity.x.hypot(self.velocity.z);
            self.velocity.x = h * f64::from(nx - cell.x);
            self.velocity.z = h * f64::from(nz - cell.z);
        }
        if boosts {
            let h = self.velocity.x.hypot(self.velocity.z);
            if h > 0.01 {
                self.velocity.x += self.velocity.x / h * 0.06;
                self.velocity.z += self.velocity.z / h * 0.06;
            } else {
                // Standing still on a powered rail, a solid block at one
                // end pushes it away from that end.
                let solid = |p: IVec3| {
                    world
                        .block(p.x, p.y, p.z)
                        .is_some_and(crate::world::redstone::conductor)
                };
                match shape {
                    1 => {
                        if solid(cell + IVec3::NEG_X) {
                            self.velocity.x = 0.02;
                        } else if solid(cell + IVec3::X) {
                            self.velocity.x = -0.02;
                        }
                    }
                    0 => {
                        if solid(cell + IVec3::NEG_Z) {
                            self.velocity.z = 0.02;
                        } else if solid(cell + IVec3::Z) {
                            self.velocity.z = -0.02;
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    fn come_off_rails(&mut self, collider: &impl Collider) {
        self.velocity.x = self.velocity.x.clamp(-MAX_SPEED, MAX_SPEED);
        self.velocity.z = self.velocity.z.clamp(-MAX_SPEED, MAX_SPEED);
        if self.on_ground {
            self.velocity *= 0.5;
        }
        let wanted = self.velocity;
        self.slide(collider, wanted);
        if !self.on_ground {
            self.velocity *= 0.95;
        }
    }

    /// Moves by `wanted`, y first, stopping at blocks.
    fn slide(&mut self, collider: &impl Collider, wanted: DVec3) {
        self.impact = wanted.x * wanted.x + wanted.z * wanted.z;
        self.on_ground = false;
        self.bumped = false;
        for axis in [1, 0, 2] {
            let d = wanted[axis];
            if d == 0.0 {
                continue;
            }
            let aabb = self.aabb();
            let mut stretch = DVec3::ZERO;
            stretch[axis] = d;
            let mut boxes = Vec::new();
            collider.boxes(&aabb.stretch(stretch), &mut boxes);
            let moved = boxes.iter().fold(d, |m, b| b.clip(&aabb, axis, m));
            self.position[axis] += moved;
            if moved != d {
                if axis == 1 {
                    self.on_ground = d < 0.0;
                } else {
                    self.bumped = true;
                }
                self.velocity[axis] = 0.0;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        player::collision::WorldCollider,
        world::{
            World,
            chunk::{Chunk, ChunkPos},
            props::rail_shape,
        },
    };

    fn rail(shape: u8) -> BlockState {
        BlockState::of(blocks::RAIL).with(Prop::RailShape, shape)
    }

    fn powered(shape: u8, on: bool) -> BlockState {
        BlockState::of(blocks::POWERED_RAIL)
            .with(Prop::StraightRailShape, shape)
            .with_bool(Prop::Powered, on)
    }

    /// Stone floor at y=63 with `blocks` set on top.
    fn world_with(cells: &[(IVec3, BlockState)]) -> World {
        let mut world = World::default();
        let mut chunk = Chunk::new(ChunkPos::new(0, 0));
        for x in 0..16 {
            for z in 0..16 {
                chunk.set(x, 63, z, BlockState::of(blocks::STONE));
            }
        }
        for &(at, state) in cells {
            chunk.set(at.x as usize, at.y, at.z as usize, state);
        }
        world.insert(chunk);
        world
    }

    fn run(cart: &mut Minecart, world: &World, ticks: usize) {
        let collider = WorldCollider {
            world,
            blocks: crate::world::block::registry(),
        };
        for _ in 0..ticks {
            cart.tick(world, &collider, DVec3::ZERO);
        }
    }

    fn line(shape: u8, from: i32, to: i32, z: i32) -> Vec<(IVec3, BlockState)> {
        (from..=to)
            .map(|x| (IVec3::new(x, 64, z), rail(shape)))
            .collect()
    }

    #[test]
    fn a_pushed_cart_rolls_along_flat_rail_and_slows_by_four_percent_a_tick() {
        let world = world_with(&line(rail_shape::EAST_WEST, 1, 14, 5));
        let mut cart = Minecart::new(CartKind::Rideable, DVec3::new(2.5, 64.0, 5.5));
        cart.velocity.x = 0.3;
        run(&mut cart, &world, 1);
        let after_one = cart.velocity.x;
        assert!((after_one - 0.3 * 0.96).abs() < 1e-9, "{after_one}");
        run(&mut cart, &world, 10);
        assert!(cart.position.x > 4.0);
        assert!((cart.position.z - 5.5).abs() < 1e-9, "stays on the line");
        assert!(
            (cart.position.y - 64.0625).abs() < 1e-6,
            "{}",
            cart.position.y
        );
        assert!(cart.on_rail);
    }

    #[test]
    fn it_follows_a_corner_and_a_slope() {
        let mut cells = line(rail_shape::EAST_WEST, 1, 5, 5);
        cells.push((IVec3::new(6, 64, 5), rail(rail_shape::SOUTH_WEST)));
        cells.extend((6..=9).map(|z| (IVec3::new(6, 64, z), rail(rail_shape::NORTH_SOUTH))));
        let world = world_with(&cells);
        let mut cart = Minecart::new(CartKind::Rideable, DVec3::new(2.5, 64.0, 5.5));
        cart.velocity.x = 0.4;
        run(&mut cart, &world, 25);
        assert!(
            cart.position.z > 6.5,
            "turned south at the corner: {:?}",
            cart.position
        );
        assert!((cart.position.x - 6.5).abs() < 0.2);
        // Rolling down a slope speeds a cart up from rest.
        let slope = vec![
            (IVec3::new(4, 64, 8), rail(rail_shape::ASCENDING_EAST)),
            (IVec3::new(3, 64, 8), rail(rail_shape::EAST_WEST)),
            (IVec3::new(2, 64, 8), rail(rail_shape::EAST_WEST)),
        ];
        let world = world_with(&slope);
        let mut cart = Minecart::new(CartKind::Rideable, DVec3::new(4.5, 64.5, 8.5));
        run(&mut cart, &world, 20);
        assert!(
            cart.velocity.x < -0.05,
            "rolled west downhill: {}",
            cart.velocity.x
        );
        assert!(cart.position.x < 4.0);
    }

    #[test]
    fn powered_rails_boost_unpowered_ones_stop_and_a_bump_stops_it_dead() {
        let mut cells = line(rail_shape::EAST_WEST, 1, 3, 2);
        cells
            .extend((4..=12).map(|x| (IVec3::new(x, 64, 2), powered(rail_shape::EAST_WEST, true))));
        let world = world_with(&cells);
        let mut cart = Minecart::new(CartKind::Rideable, DVec3::new(4.5, 64.0, 2.5));
        cart.velocity.x = 0.1;
        run(&mut cart, &world, 18);
        assert!(cart.velocity.x > 0.25, "boosted: {}", cart.velocity.x);
        let mut cells = line(rail_shape::EAST_WEST, 1, 3, 2);
        cells.extend(
            (4..=12).map(|x| (IVec3::new(x, 64, 2), powered(rail_shape::EAST_WEST, false))),
        );
        let world = world_with(&cells);
        let mut cart = Minecart::new(CartKind::Rideable, DVec3::new(4.5, 64.0, 2.5));
        cart.velocity.x = 0.3;
        run(&mut cart, &world, 30);
        assert!(cart.velocity.x < 0.01, "braked: {}", cart.velocity.x);
        // A dead end: it runs into stone and stops.
        let mut cells = line(rail_shape::EAST_WEST, 1, 6, 2);
        cells.push((IVec3::new(7, 64, 2), BlockState::of(blocks::STONE)));
        let world = world_with(&cells);
        let mut cart = Minecart::new(CartKind::Rideable, DVec3::new(2.5, 64.0, 2.5));
        cart.velocity.x = 0.4;
        run(&mut cart, &world, 30);
        assert!(cart.position.x < 7.0 - 0.49 + 1e-6, "{}", cart.position.x);
    }

    #[test]
    fn off_the_rails_it_falls_and_slides_to_a_stop_and_tnt_carts_blow_on_a_hard_hit() {
        let world = world_with(&[]);
        let mut cart = Minecart::new(CartKind::Rideable, DVec3::new(5.5, 70.0, 5.5));
        cart.velocity.x = 0.2;
        run(&mut cart, &world, 40);
        assert!(!cart.on_rail);
        assert!(
            (cart.position.y - 64.0).abs() < 1e-6,
            "on the floor: {}",
            cart.position.y
        );
        assert!(cart.velocity.x < 0.01);
        let cells = vec![(IVec3::new(7, 64, 5), BlockState::of(blocks::STONE))];
        let world = world_with(&cells);
        let collider = crate::player::collision::WorldCollider {
            world: &world,
            blocks: crate::world::block::registry(),
        };
        let mut tnt = Minecart::new(CartKind::Tnt, DVec3::new(6.0, 64.0, 5.5));
        tnt.velocity.x = 0.4;
        let mut blast = None;
        for _ in 0..20 {
            blast = blast.or(tnt.tick(&world, &collider, DVec3::ZERO));
        }
        assert!(
            matches!(blast, Some(CartEvent::Explode(p)) if p >= 4.0),
            "{blast:?}"
        );
        // Lit, it burns for four seconds.
        let mut lit = Minecart::new(CartKind::Tnt, DVec3::new(2.5, 64.0, 12.5));
        lit.ignite();
        let mut ticks = 0;
        loop {
            ticks += 1;
            if lit.tick(&world, &collider, DVec3::ZERO).is_some() {
                break;
            }
            assert!(ticks < 200);
        }
        assert_eq!(ticks, 81);
    }

    #[test]
    fn hits_add_up_to_breaking_it() {
        let mut cart = Minecart::new(CartKind::Rideable, DVec3::ZERO);
        assert!(!cart.hurt(1.0));
        assert!(!cart.hurt(1.0));
        assert!(!cart.hurt(1.0));
        assert!(cart.hurt(1.5), "40 is passed after about four punches");
        let mut sword = Minecart::new(CartKind::Rideable, DVec3::ZERO);
        assert!(!sword.hurt(4.0));
        assert!(sword.hurt(4.0), "two sword hits");
    }
}
