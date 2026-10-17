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
    collided_horizontally: bool,
    was_moving_forward: bool,
    sprint_tap_timer: u8,
    was_jumping: bool,
    fly_tap_timer: u8,
}

impl Body {
    pub fn new(position: DVec3) -> Self {
        Self {
            bb: hitbox(position),
            velocity: DVec3::ZERO,
            on_ground: false,
            sneaking: false,
            sprinting: false,
            may_fly: false,
            flying: false,
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

    /// Feet: the center of the hitbox's bottom face.
    pub fn position(&self) -> DVec3 {
        DVec3::new(
            (self.bb.min.x + self.bb.max.x) / 2.0,
            self.bb.min.y,
            (self.bb.min.z + self.bb.max.z) / 2.0,
        )
    }

    /// Moves the feet to `feet` without collision, for free flight.
    #[cfg(debug_assertions)]
    pub fn set_position(&mut self, feet: DVec3) {
        self.bb = hitbox(feet);
    }

    /// Camera height above the feet.
    pub fn eye_height(&self) -> f64 {
        if self.sneaking {
            EYE_HEIGHT - SNEAK_EYE_DROP
        } else {
            EYE_HEIGHT
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
        } else if input.jump && self.on_ground {
            self.velocity.y = JUMP_VELOCITY;
            if self.sprinting {
                self.velocity += facing * SPRINT_JUMP_BOOST;
            }
        }

        let friction = self.friction(world);
        let sprint = if self.sprinting { SPRINT_FACTOR } else { 1.0 };
        let acceleration = if self.flying {
            FLY_ACCELERATION
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

    /// Double-tap forward to sprint (1.0.0), or hold the sprint key. Sprinting
    /// stops when forward is released, while sneaking, or on hitting a wall.
    fn update_sprint(&mut self, forward: f64, sprint_key: bool) {
        self.sprint_tap_timer = self.sprint_tap_timer.saturating_sub(1);
        let moving_forward = forward >= SPRINT_MIN_FORWARD;
        if self.on_ground && moving_forward && !self.was_moving_forward && !self.sprinting {
            if self.sprint_tap_timer == 0 {
                self.sprint_tap_timer = SPRINT_DOUBLE_TAP_TICKS;
            } else {
                self.sprinting = true;
                self.sprint_tap_timer = 0;
            }
        }
        if sprint_key && moving_forward {
            self.sprinting = true;
        }
        if self.sprinting && (!moving_forward || self.collided_horizontally) {
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
pub fn hitbox(feet: DVec3) -> Aabb {
    let half = PLAYER_WIDTH / 2.0;
    Aabb::new(
        feet - DVec3::new(half, 0.0, half),
        feet + DVec3::new(half, PLAYER_HEIGHT, half),
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
                if pos.cmpge(min).all() && pos.cmple(max).all() {
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
}
