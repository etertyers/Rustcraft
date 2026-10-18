//! Player movement, following Minecraft 1.0.0's `EntityLiving.moveEntityWithHeading`
//! and `Entity.moveEntity`: per-tick acceleration and friction, gravity and
//! drag, axis-by-axis collision, step-up and sneaking at edges.

use glam::DVec3;

use super::collision::{Aabb, Collider};
use crate::config::{
    AIR_ACCELERATION, AIR_FRICTION, EYE_HEIGHT, FLY_ACCELERATION, FLY_VERTICAL_DRAG,
    FLY_VERTICAL_SPEED, GRAVITY, JUMP_VELOCITY, PLAYER_HEIGHT, PLAYER_WIDTH, SNEAK_EYE_DROP,
    SNEAK_FACTOR, SPRINT_FACTOR, SPRINT_JUMP_BOOST, STEP_HEIGHT, VERTICAL_DRAG, WALK_ACCELERATION,
};
use crate::world::block::Fluid;

/// Movement intent for one tick.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct MoveInput {
    /// +1 forward, −1 back.
    pub forward: f64,
    /// +1 right, −1 left.
    pub strafe: f64,
    pub jump: bool,
    pub sneak: bool,
    /// The sprint key; double-tapping forward also sprints, as in 1.0.0.
    pub sprint: bool,
}

/// A second forward press within this many ticks starts sprinting.
const SPRINT_DOUBLE_TAP_TICKS: u8 = 7;
/// Acceleration at full input in water or lava (1.0.0's `moveFlying` 0.02).
const SWIM_ACCELERATION: f64 = 0.02;
/// Velocity kept per tick in water and in lava.
const WATER_DRAG: f64 = 0.8;
const LAVA_DRAG: f64 = 0.5;
/// Sinking per tick in a liquid, and rising per tick holding jump.
const SWIM_SINK: f64 = 0.02;
const SWIM_UP: f64 = 0.04;
/// Speed added each tick along the water's flow (1.0.0's
/// `handleMaterialAcceleration`).
pub const CURRENT_PUSH: f64 = 0.014;
/// Upward speed when swimming into a ledge with room above it.
const LEDGE_HOP: f64 = 0.3;
/// A second jump press within this many ticks toggles creative flight.
const FLY_DOUBLE_TAP_TICKS: u8 = 7;
/// Forward input needed to start or keep sprinting.
const SPRINT_MIN_FORWARD: f64 = 0.8;
/// Every movement input is scaled by this (1.0.0 `moveForward *= 0.98`).
const INPUT_SCALE: f64 = 0.98;
/// Makes ground acceleration independent of friction at normal slipperiness
/// (1.0.0's `0.16277136 / f³`).
const GROUND_ACCELERATION_SCALE: f64 = 0.16277136;
/// Sneaking backs a move off an edge in steps this long.
const SNEAK_EDGE_STEP: f64 = 0.05;

/// The player's physical body.
#[derive(Debug, Clone, PartialEq)]
pub struct Body {
    /// The hitbox is the stored state, as in 1.0.0: deriving it from the
    /// feet position every tick would let rounding push it into a wall.
    bb: Aabb,
    /// Blocks per tick.
    pub velocity: DVec3,
    pub on_ground: bool,
    pub sneaking: bool,
    pub sprinting: bool,
    /// Double-tapping jump toggles `flying` (creative).
    pub may_fly: bool,
    pub flying: bool,
    /// False when too hungry to sprint.
    pub can_sprint: bool,
    /// Whether the last tick started a jump.
    pub jumped: bool,
    /// The liquid the body was in during the last tick.
    pub liquid: Option<Fluid>,
    collided_horizontally: bool,
    was_moving_forward: bool,
    sprint_tap_timer: u8,
    was_jumping: bool,
    fly_tap_timer: u8,
    /// Eye height above the feet when not sneaking.
    eye: f64,
}

impl Body {
    /// The player's body, with its feet at `position`.
    pub fn new(position: DVec3) -> Self {
        Self::sized(position, PLAYER_WIDTH, PLAYER_HEIGHT, EYE_HEIGHT)
    }

    /// A body of any size (mobs), with its feet at `position`.
    pub fn sized(position: DVec3, width: f64, height: f64, eye_height: f64) -> Self {
        Self {
            bb: sized_hitbox(position, width, height),
            eye: eye_height,
            velocity: DVec3::ZERO,
            on_ground: false,
            sneaking: false,
            sprinting: false,
            may_fly: false,
            flying: false,
            can_sprint: true,
            jumped: false,
            liquid: None,
            collided_horizontally: false,
            was_moving_forward: false,
            sprint_tap_timer: 0,
            was_jumping: false,
            fly_tap_timer: 0,
        }
    }

    pub fn aabb(&self) -> Aabb {
        self.bb
    }

    /// Whether the last move ran into a wall.
    pub fn collided_horizontally(&self) -> bool {
        self.collided_horizontally
    }

    /// Feet: the center of the hitbox's bottom face.
    pub fn position(&self) -> DVec3 {
        DVec3::new(
            (self.bb.min.x + self.bb.max.x) / 2.0,
            self.bb.min.y,
            (self.bb.min.z + self.bb.max.z) / 2.0,
        )
    }

    /// Moves the feet to `feet` without collision, for free flight and the
    /// bench's route.
    pub fn set_position(&mut self, feet: DVec3) {
        let size = self.bb.max - self.bb.min;
        self.bb = sized_hitbox(feet, size.x, size.y);
    }

    /// Camera height above the feet.
    pub fn eye_height(&self) -> f64 {
        if self.sneaking {
            self.eye - SNEAK_EYE_DROP
        } else {
            self.eye
        }
    }

    /// Advances one tick. `yaw` is in radians, 0 facing north (−Z).
    pub fn tick(&mut self, input: &MoveInput, yaw: f32, world: &impl Collider) {
        let mut forward = input.forward.clamp(-1.0, 1.0);
        let mut strafe = input.strafe.clamp(-1.0, 1.0);
        self.sneaking = input.sneak;
        if self.sneaking {
            forward *= SNEAK_FACTOR;
            strafe *= SNEAK_FACTOR;
        }
        self.update_sprint(forward, input.sprint);
        self.update_flight(input.jump);
        let liquid = if self.flying {
            None
        } else {
            self.find_liquid(world)
        };
        self.liquid = liquid;
        if liquid == Some(Fluid::Water) {
            self.velocity += world.current(&self.water_box()) * CURRENT_PUSH;
        }
        self.jumped = false;

        let (sin_yaw, cos_yaw) = (yaw as f64).sin_cos();
        let facing = DVec3::new(sin_yaw, 0.0, -cos_yaw);
        let right = DVec3::new(cos_yaw, 0.0, sin_yaw);
        if self.flying {
            if input.sneak {
                self.velocity.y -= FLY_VERTICAL_SPEED;
            }
            if input.jump {
                self.velocity.y += FLY_VERTICAL_SPEED;
            }
        } else if input.jump && liquid.is_some() {
            self.velocity.y += SWIM_UP;
        } else if input.jump && self.on_ground {
            self.jumped = true;
            self.velocity.y = JUMP_VELOCITY;
            if self.sprinting {
                self.velocity += facing * SPRINT_JUMP_BOOST;
            }
        }

        let friction = self.friction(world);
        let sprint = if self.sprinting { SPRINT_FACTOR } else { 1.0 };
        let acceleration = if self.flying {
            FLY_ACCELERATION
        } else if liquid.is_some() {
            SWIM_ACCELERATION
        } else if self.on_ground {
            WALK_ACCELERATION * sprint * GROUND_ACCELERATION_SCALE / friction.powi(3)
        } else {
            AIR_ACCELERATION * sprint
        };
        let (forward, strafe) = (forward * INPUT_SCALE, strafe * INPUT_SCALE);
        let length = forward.hypot(strafe);
        if length >= 0.01 {
            let scale = acceleration / length.max(1.0);
            self.velocity += (facing * forward + right * strafe) * scale;
        }

        if let Some(fluid) = liquid {
            self.swim(fluid, world);
            return;
        }
        let vertical = self.velocity.y;
        self.move_by(self.velocity, world);
        if self.flying {
            // 1.0.0 keeps 0.6 of the vertical speed and ignores gravity.
            self.velocity.y = vertical * FLY_VERTICAL_DRAG;
            if self.on_ground {
                self.flying = false;
            }
        } else {
            self.velocity.y = (self.velocity.y - GRAVITY) * VERTICAL_DRAG;
        }
        self.velocity.x *= friction;
        self.velocity.z *= friction;
    }

    /// The liquid the body is in: water reaching the hitbox shrunk by 0.4
    /// top and bottom, or lava reaching it also shrunk 0.1 sideways
    /// (1.0.0's `isInWater` and `handleLavaMovement`).
    fn find_liquid(&self, world: &impl Collider) -> Option<Fluid> {
        match world.liquid(&self.water_box()) {
            Some(Fluid::Water) => Some(Fluid::Water),
            _ => world
                .liquid(&self.shrunk(0.1))
                .filter(|&fluid| fluid == Fluid::Lava),
        }
    }

    /// The hitbox shrunk by 0.4 top and bottom and `side` sideways.
    fn shrunk(&self, side: f64) -> Aabb {
        let by = DVec3::new(side, 0.4, side);
        Aabb::new(self.bb.min + by, self.bb.max - by)
    }

    /// The part of the body water has to reach to count.
    fn water_box(&self) -> Aabb {
        self.shrunk(0.001)
    }

    /// Moves through water or lava: thick drag, slow sinking, and a hop out
    /// onto a ledge when swimming into it (1.0.0's
    /// `moveEntityWithHeading` liquid branch).
    fn swim(&mut self, fluid: Fluid, world: &impl Collider) {
        let start_y = self.bb.min.y;
        self.move_by(self.velocity, world);
        let drag = match fluid {
            Fluid::Water => WATER_DRAG,
            Fluid::Lava => LAVA_DRAG,
        };
        self.velocity *= drag;
        self.velocity.y -= SWIM_SINK;
        let climbed = self.bb.min.y - start_y;
        let ledge = DVec3::new(
            self.velocity.x,
            self.velocity.y + 0.6 - climbed,
            self.velocity.z,
        );
        let above = self.bb.offset(ledge);
        if self.collided_horizontally && !collides(&above, world) && world.liquid(&above).is_none()
        {
            self.velocity.y = LEDGE_HOP;
        }
    }

    /// Double-tap forward to sprint (1.0.0), or hold the sprint key. Sprinting
    /// stops when forward is released, while sneaking, or on hitting a wall.
    fn update_sprint(&mut self, forward: f64, sprint_key: bool) {
        self.sprint_tap_timer = self.sprint_tap_timer.saturating_sub(1);
        let moving_forward = forward >= SPRINT_MIN_FORWARD;
        let can_start = self.on_ground && moving_forward && self.can_sprint;
        if can_start && !self.was_moving_forward && !self.sprinting {
            if self.sprint_tap_timer == 0 {
                self.sprint_tap_timer = SPRINT_DOUBLE_TAP_TICKS;
            } else {
                self.sprinting = true;
                self.sprint_tap_timer = 0;
            }
        }
        if sprint_key && moving_forward && self.can_sprint {
            self.sprinting = true;
        }
        if self.sprinting && (!moving_forward || self.collided_horizontally || !self.can_sprint) {
            self.sprinting = false;
        }
        self.was_moving_forward = moving_forward;
    }

    /// Double-tapping jump toggles flight when allowed (1.0.0's
    /// `flyToggleTimer`).
    fn update_flight(&mut self, jump: bool) {
        self.fly_tap_timer = self.fly_tap_timer.saturating_sub(1);
        if !self.may_fly {
            self.flying = false;
        } else if jump && !self.was_jumping {
            if self.fly_tap_timer == 0 {
                self.fly_tap_timer = FLY_DOUBLE_TAP_TICKS;
            } else {
                self.flying = !self.flying;
                self.fly_tap_timer = 0;
            }
        }
        self.was_jumping = jump;
    }

    /// Horizontal velocity kept per tick: the block underfoot's slipperiness
    /// × 0.91 on the ground, 0.91 in the air.
    fn friction(&self, world: &impl Collider) -> f64 {
        if !self.on_ground {
            return AIR_FRICTION;
        }
        let p = self.position().floor().as_ivec3();
        let below = self.bb.min.y.floor() as i32 - 1;
        world.slipperiness(p.x, below, p.z) as f64 * AIR_FRICTION
    }

    /// Moves by `wanted`, stopping at solid blocks and stepping up ledges.
    fn move_by(&mut self, wanted: DVec3, world: &impl Collider) {
        let start = self.aabb();
        let mut wanted = wanted;
        if self.on_ground && self.sneaking {
            (wanted.x, wanted.z) = back_off_edges(&start, wanted.x, wanted.z, world);
        }
        let mut boxes = Vec::new();
        let (mut moved, mut end) = sweep(start, wanted, world, &mut boxes);

        let landing = self.on_ground || (moved.y != wanted.y && wanted.y < 0.0);
        let blocked = moved.x != wanted.x || moved.z != wanted.z;
        if landing && blocked {
            // Retry raised by the step height, then settle back down.
            let up = DVec3::new(wanted.x, STEP_HEIGHT, wanted.z);
            let (raised, raised_box) = sweep(start, up, world, &mut boxes);
            let (lowered, stepped_box) =
                sweep(raised_box, DVec3::NEG_Y * STEP_HEIGHT, world, &mut boxes);
            if raised.x.hypot(raised.z) > moved.x.hypot(moved.z) {
                moved = raised + lowered;
                end = stepped_box;
            }
        }

        self.bb = end;
        self.collided_horizontally = moved.x != wanted.x || moved.z != wanted.z;
        self.on_ground = moved.y != wanted.y && wanted.y < 0.0;
        for axis in 0..3 {
            if moved[axis] != wanted[axis] {
                self.velocity[axis] = 0.0;
            }
        }
    }
}

/// The player hitbox with its feet at `feet`.
#[cfg(test)]
pub fn hitbox(feet: DVec3) -> Aabb {
    sized_hitbox(feet, PLAYER_WIDTH, PLAYER_HEIGHT)
}

/// A `width` × `height` hitbox with its feet at `feet`.
pub fn sized_hitbox(feet: DVec3, width: f64, height: f64) -> Aabb {
    let half = width / 2.0;
    Aabb::new(
        feet - DVec3::new(half, 0.0, half),
        feet + DVec3::new(half, height, half),
    )
}

/// Moves `bb` by `d`, clipping Y first, then X, then Z, like 1.0.0.
/// Returns the distance actually moved and the moved box.
pub fn sweep(bb: Aabb, d: DVec3, world: &impl Collider, boxes: &mut Vec<Aabb>) -> (DVec3, Aabb) {
    boxes.clear();
    world.boxes(&bb.stretch(d), boxes);
    let mut bb = bb;
    let mut moved = d;
    for axis in [1, 0, 2] {
        let mut v = moved[axis];
        for block in boxes.iter() {
            v = block.clip(&bb, axis, v);
        }
        moved[axis] = v;
        let mut offset = DVec3::ZERO;
        offset[axis] = v;
        bb = bb.offset(offset);
    }
    (moved, bb)
}

/// Shortens a sneaking move so the player keeps something under their
/// feet: 1.0.0 backs off in 0.05-block steps while nothing solid is within a
/// block below the moved hitbox.
fn back_off_edges(bb: &Aabb, mut dx: f64, mut dz: f64, world: &impl Collider) -> (f64, f64) {
    let unsupported = |dx: f64, dz: f64| !collides(&bb.offset(DVec3::new(dx, -1.0, dz)), world);
    let shrink = |v: f64| {
        if v.abs() < SNEAK_EDGE_STEP {
            0.0
        } else {
            v - SNEAK_EDGE_STEP.copysign(v)
        }
    };
    while dx != 0.0 && unsupported(dx, 0.0) {
        dx = shrink(dx);
    }
    while dz != 0.0 && unsupported(0.0, dz) {
        dz = shrink(dz);
    }
    while dx != 0.0 && dz != 0.0 && unsupported(dx, dz) {
        dx = shrink(dx);
        dz = shrink(dz);
    }
    (dx, dz)
}

fn collides(bb: &Aabb, world: &impl Collider) -> bool {
    let mut boxes = Vec::new();
    world.boxes(bb, &mut boxes);
    boxes.iter().any(|b| b.intersects(bb))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::TICKS_PER_SECOND;
    use glam::IVec3;
    use std::collections::HashMap;

    /// Hand-built test worlds: full blocks, half-height blocks, ice.
    #[derive(Default)]
    struct TestWorld {
        blocks: HashMap<IVec3, Kind>,
    }

    #[derive(Clone, Copy, PartialEq)]
    enum Kind {
        Full,
        Half,
        Ice,
        Water,
    }

    impl TestWorld {
        /// A floor of `kind` covering x and z in -50..50 at y 63.
        fn floor(kind: Kind) -> Self {
            let mut world = Self::default();
            for z in -50..50 {
                for x in -50..50 {
                    world.set(x, 63, z, kind);
                }
            }
            world
        }

        fn set(&mut self, x: i32, y: i32, z: i32, kind: Kind) {
            self.blocks.insert(IVec3::new(x, y, z), kind);
        }
    }

    impl Collider for TestWorld {
        fn boxes(&self, region: &Aabb, out: &mut Vec<Aabb>) {
            let (min, max) = (region.min.floor().as_ivec3(), region.max.floor().as_ivec3());
            for (pos, &kind) in &self.blocks {
                if kind != Kind::Water && pos.cmpge(min).all() && pos.cmple(max).all() {
                    let mut bb = Aabb::block(pos.x, pos.y, pos.z);
                    if kind == Kind::Half {
                        bb.max.y -= 0.5;
                    }
                    out.push(bb);
                }
            }
        }

        fn slipperiness(&self, x: i32, y: i32, z: i32) -> f32 {
            match self.blocks.get(&IVec3::new(x, y, z)) {
                Some(Kind::Ice) => 0.98,
                _ => 0.6,
            }
        }

        fn liquid(&self, region: &Aabb) -> Option<Fluid> {
            let (min, max) = (region.min.floor().as_ivec3(), region.max.floor().as_ivec3());
            self.blocks
                .iter()
                .any(|(pos, &kind)| {
                    kind == Kind::Water
                        && pos.cmpge(min).all()
                        && pos.cmple(max).all()
                        && region.max.y >= pos.y as f64 + 8.0 / 9.0
                })
                .then_some(Fluid::Water)
        }
    }

    fn standing_at(x: f64, z: f64) -> Body {
        let mut body = Body::new(DVec3::new(x, 64.0, z));
        body.on_ground = true;
        body
    }

    const FORWARD: MoveInput = MoveInput {
        forward: 1.0,
        strafe: 0.0,
        jump: false,
        sneak: false,
        sprint: false,
    };

    /// Steady horizontal speed in blocks per second after `ticks` of `input`.
    fn top_speed(world: &TestWorld, input: MoveInput, ticks: u32) -> f64 {
        let mut body = standing_at(0.5, 0.5);
        for _ in 0..ticks {
            body.tick(&input, 0.0, world);
        }
        let before = body.position();
        body.tick(&input, 0.0, world);
        (body.position() - before).length() * TICKS_PER_SECOND as f64
    }

    #[test]
    fn walk_sprint_and_sneak_speeds_match_minecraft() {
        let world = TestWorld::floor(Kind::Full);
        let walk = top_speed(&world, FORWARD, 60);
        let sprint = top_speed(
            &world,
            MoveInput {
                sprint: true,
                ..FORWARD
            },
            60,
        );
        let sneak = top_speed(
            &world,
            MoveInput {
                sneak: true,
                ..FORWARD
            },
            60,
        );
        assert!((walk - 4.317).abs() < 0.005, "walk {walk}");
        assert!((sprint - 5.612).abs() < 0.005, "sprint {sprint}");
        assert!((sneak - 1.31).abs() < 0.03, "sneak {sneak}");
    }

    /// Diagonal input is normalized after the 0.98 input scale, so it ends
    /// up about 2% faster than straight walking, exactly like 1.0.0.
    #[test]
    fn diagonal_walking_matches_minecraft() {
        let world = TestWorld::floor(Kind::Full);
        let input = MoveInput {
            strafe: 1.0,
            ..FORWARD
        };
        let diagonal = top_speed(&world, input, 60);
        assert!((diagonal - 4.405).abs() < 0.005, "diagonal {diagonal}");
    }

    #[test]
    fn walking_follows_yaw() {
        let world = TestWorld::floor(Kind::Full);
        let mut body = standing_at(0.5, 0.5);
        for _ in 0..20 {
            body.tick(&FORWARD, std::f32::consts::FRAC_PI_2, &world);
        }
        assert!(body.position().x > 3.0, "{}", body.position());
        assert!((body.position().z - 0.5).abs() < 1e-6);
    }

    #[test]
    fn jump_reaches_about_1_25_blocks_and_lands_after_12_ticks() {
        let world = TestWorld::floor(Kind::Full);
        let mut body = standing_at(0.5, 0.5);
        let jump = MoveInput {
            jump: true,
            ..Default::default()
        };
        body.tick(&jump, 0.0, &world);
        let mut apex: f64 = 0.0;
        let mut ticks = 1;
        while !body.on_ground {
            body.tick(&MoveInput::default(), 0.0, &world);
            apex = apex.max(body.position().y - 64.0);
            ticks += 1;
            assert!(ticks < 100);
        }
        assert!((apex - 1.2522).abs() < 0.001, "apex {apex}");
        assert_eq!(ticks, 12);
        assert_eq!(body.position().y, 64.0);
    }

    #[test]
    fn falling_lands_on_the_floor() {
        let world = TestWorld::floor(Kind::Full);
        let mut body = Body::new(DVec3::new(0.5, 80.0, 0.5));
        for _ in 0..100 {
            body.tick(&MoveInput::default(), 0.0, &world);
        }
        assert!(body.on_ground);
        assert_eq!(body.position().y, 64.0);
        assert_eq!(body.velocity.y, 0.0 - GRAVITY * VERTICAL_DRAG);
    }

    #[test]
    fn walls_stop_walking_and_end_sprinting() {
        let mut world = TestWorld::floor(Kind::Full);
        for y in 64..66 {
            world.set(0, y, -3, Kind::Full);
        }
        let mut body = standing_at(0.5, 0.5);
        let sprint = MoveInput {
            sprint: true,
            ..FORWARD
        };
        for _ in 0..40 {
            body.tick(&sprint, 0.0, &world);
        }
        assert!(
            (body.position().z - (-2.0 + 0.3)).abs() < 1e-9,
            "{}",
            body.position()
        );
        assert!(!body.sprinting);
    }

    #[test]
    fn steps_up_half_blocks_but_not_full_ones() {
        let mut world = TestWorld::floor(Kind::Full);
        world.set(0, 64, -3, Kind::Half);
        let mut body = standing_at(0.5, 0.5);
        for _ in 0..30 {
            body.tick(&FORWARD, 0.0, &world);
        }
        assert!(body.position().z < -3.0, "{}", body.position());

        let mut world = TestWorld::floor(Kind::Full);
        world.set(0, 64, -3, Kind::Full);
        let mut body = standing_at(0.5, 0.5);
        for _ in 0..30 {
            body.tick(&FORWARD, 0.0, &world);
        }
        assert_eq!(body.position().y, 64.0);
        assert!(body.position().z > -2.0 + 0.29);
    }

    #[test]
    fn jumping_climbs_onto_a_block() {
        let mut world = TestWorld::floor(Kind::Full);
        for x in -2..3 {
            world.set(x, 64, -2, Kind::Full);
        }
        let mut body = standing_at(0.5, 0.5);
        let input = MoveInput {
            jump: true,
            ..FORWARD
        };
        for _ in 0..30 {
            body.tick(&input, 0.0, &world);
        }
        assert!(
            body.position().z < -1.0 && body.position().y >= 65.0,
            "{}",
            body.position()
        );
    }

    #[test]
    fn sneaking_never_walks_off_an_edge() {
        let mut world = TestWorld::default();
        world.set(0, 63, 0, Kind::Full);
        let mut body = standing_at(0.5, 0.5);
        let input = MoveInput {
            sneak: true,
            strafe: 1.0,
            ..FORWARD
        };
        for _ in 0..100 {
            body.tick(&input, 0.0, &world);
        }
        assert_eq!(body.position().y, 64.0);
        assert!(body.on_ground);
        // The hitbox still overlaps the block.
        assert!(body.position().x < 1.0 + 0.3 && body.position().z > -0.3);
        // Without sneaking, the same walk falls off.
        let mut body = standing_at(0.5, 0.5);
        for _ in 0..40 {
            body.tick(
                &MoveInput {
                    strafe: 1.0,
                    ..FORWARD
                },
                0.0,
                &world,
            );
        }
        assert!(body.position().y < 64.0);
    }

    #[test]
    fn double_tapping_forward_sprints() {
        let world = TestWorld::floor(Kind::Full);
        let mut body = standing_at(0.5, 0.5);
        body.tick(&FORWARD, 0.0, &world);
        assert!(!body.sprinting);
        body.tick(&MoveInput::default(), 0.0, &world);
        body.tick(&FORWARD, 0.0, &world);
        assert!(body.sprinting);
        // Releasing forward stops it.
        body.tick(&MoveInput::default(), 0.0, &world);
        assert!(!body.sprinting);
        // A slow second tap doesn't count.
        body.tick(&FORWARD, 0.0, &world);
        for _ in 0..10 {
            body.tick(&MoveInput::default(), 0.0, &world);
        }
        body.tick(&FORWARD, 0.0, &world);
        assert!(!body.sprinting);
    }

    #[test]
    fn sneaking_stops_sprinting_and_lowers_the_eye() {
        let world = TestWorld::floor(Kind::Full);
        let mut body = standing_at(0.5, 0.5);
        let sprint = MoveInput {
            sprint: true,
            ..FORWARD
        };
        body.tick(&sprint, 0.0, &world);
        assert!(body.sprinting);
        body.tick(
            &MoveInput {
                sneak: true,
                ..sprint
            },
            0.0,
            &world,
        );
        assert!(!body.sprinting);
        assert_eq!(body.eye_height(), EYE_HEIGHT - SNEAK_EYE_DROP);
    }

    #[test]
    fn ice_slides_further() {
        let stop_distance = |kind| {
            let world = TestWorld::floor(kind);
            let mut body = standing_at(0.5, 0.5);
            for _ in 0..40 {
                body.tick(&FORWARD, 0.0, &world);
            }
            let released = body.position();
            for _ in 0..200 {
                body.tick(&MoveInput::default(), 0.0, &world);
            }
            (body.position() - released).length()
        };
        let (stone, ice) = (stop_distance(Kind::Full), stop_distance(Kind::Ice));
        assert!(stone < 0.5, "stone {stone}");
        assert!(ice > 3.0 * stone, "ice {ice}, stone {stone}");
    }

    #[test]
    fn sprint_jumping_adds_a_forward_push() {
        let world = TestWorld::floor(Kind::Full);
        let mut body = standing_at(0.5, 0.5);
        body.sprinting = true;
        let before = body.velocity;
        let input = MoveInput {
            jump: true,
            sprint: true,
            ..FORWARD
        };
        body.tick(&input, 0.0, &world);
        // 0.2 boost plus a tick of sprint acceleration, toward north.
        assert!(body.velocity.z - before.z < -0.2 * AIR_FRICTION * 0.6);
        assert!(body.velocity.y > 0.0);
    }

    const JUMP: MoveInput = MoveInput {
        forward: 0.0,
        strafe: 0.0,
        jump: true,
        sneak: false,
        sprint: false,
    };

    /// Taps jump twice, a tick apart, as a player double-tapping Space.
    fn double_tap_jump(body: &mut Body, world: &TestWorld) {
        for input in [JUMP, MoveInput::default(), JUMP, MoveInput::default()] {
            body.tick(&input, 0.0, world);
        }
    }

    #[test]
    fn double_tapping_jump_flies_only_when_allowed() {
        let world = TestWorld::floor(Kind::Full);
        let mut survival = standing_at(0.5, 0.5);
        double_tap_jump(&mut survival, &world);
        assert!(!survival.flying);

        let mut creative = standing_at(0.5, 0.5);
        creative.may_fly = true;
        double_tap_jump(&mut creative, &world);
        assert!(creative.flying);
        // Hovering: no gravity, so once the jump dies away it stays up.
        for _ in 0..60 {
            creative.tick(&MoveInput::default(), 0.0, &world);
        }
        let height = creative.position().y;
        for _ in 0..40 {
            creative.tick(&MoveInput::default(), 0.0, &world);
        }
        assert!((creative.position().y - height).abs() < 1e-6, "hovers");
        assert!(height > 64.0);
    }

    #[test]
    fn flight_speeds_match_1_0_0() {
        let world = TestWorld::default();
        let mut body = Body::new(DVec3::new(0.5, 80.0, 0.5));
        body.may_fly = true;
        body.flying = true;
        let rise = MoveInput {
            jump: true,
            ..FORWARD
        };
        let mut start = body.position();
        for tick in 0..200 {
            if tick == 100 {
                start = body.position();
            }
            body.tick(&rise, 0.0, &world);
        }
        let per_second = (body.position() - start) / 100.0 * TICKS_PER_SECOND as f64;
        // 1.0.0: 0.05 × 0.98 / (1 − 0.91) blocks per tick forward; up, the
        // 0.15 is added before moving, so 0.15 / (1 − 0.6) per tick.
        assert!(
            (-per_second.z - 10.89).abs() < 0.01,
            "forward {}",
            -per_second.z
        );
        assert!((per_second.y - 7.5).abs() < 0.01, "up {}", per_second.y);
    }

    #[test]
    fn flying_stops_on_landing() {
        let world = TestWorld::floor(Kind::Full);
        let mut body = Body::new(DVec3::new(0.5, 66.0, 0.5));
        body.may_fly = true;
        body.flying = true;
        let down = MoveInput {
            sneak: true,
            ..MoveInput::default()
        };
        for _ in 0..40 {
            body.tick(&down, 0.0, &world);
        }
        assert!(!body.flying);
        assert_eq!(body.position().y, 64.0);
    }

    /// A pool of water 10 deep (y 54–63) over a stone floor at y 53.
    fn pool() -> TestWorld {
        let mut world = TestWorld::default();
        for z in -20..20 {
            for x in -20..20 {
                world.set(x, 53, z, Kind::Full);
                for y in 54..64 {
                    world.set(x, y, z, Kind::Water);
                }
            }
        }
        world
    }

    fn vertical_speed_after(world: &TestWorld, input: MoveInput, ticks: u32) -> f64 {
        let mut body = Body::new(DVec3::new(0.5, 58.0, 0.5));
        for _ in 0..ticks {
            body.tick(&input, 0.0, world);
        }
        body.velocity.y
    }

    #[test]
    fn sinking_and_swimming_up_match_1_0_0() {
        let world = pool();
        // 1.0.0: ×0.8 then −0.02 per tick settles at −0.1; holding jump
        // adds 0.04 first and settles at +0.06.
        let sinking = vertical_speed_after(&world, MoveInput::default(), 30);
        assert!((sinking + 0.1).abs() < 1e-3, "{sinking}");
        let rising = vertical_speed_after(&world, JUMP, 30);
        assert!((rising - 0.06).abs() < 1e-3, "{rising}");
    }

    #[test]
    fn swimming_speed_matches_1_0_0() {
        let world = pool();
        let mut body = Body::new(DVec3::new(0.5, 58.0, 0.5));
        let mut start = body.position();
        for tick in 0..100 {
            if tick == 50 {
                start = body.position();
            }
            body.tick(&FORWARD, 0.0, &world);
        }
        let speed = (start.z - body.position().z) / 50.0 * TICKS_PER_SECOND as f64;
        // 0.02 × 0.98 / (1 − 0.8) blocks per tick.
        assert!((speed - 1.96).abs() < 0.01, "{speed}");
    }

    #[test]
    fn treading_water_stays_at_the_surface_and_a_ledge_lifts_you_out() {
        let mut world = pool();
        // A stone bank north of the pool, level with the water.
        for x in -20..20 {
            for z in -100..-20 {
                world.set(x, 63, z, Kind::Full);
            }
        }
        let mut body = Body::new(DVec3::new(0.5, 62.0, -15.0));
        let swim_north = MoveInput {
            jump: true,
            ..FORWARD
        };
        for _ in 0..200 {
            body.tick(&swim_north, 0.0, &world);
        }
        assert!(
            body.position().y >= 64.0,
            "climbed out: {}",
            body.position()
        );
        assert!(
            body.position().z < -20.0,
            "onto the bank: {}",
            body.position()
        );
    }
}
