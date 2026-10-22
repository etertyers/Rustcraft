//! Player movement, after modern Minecraft's `LivingEntity.travel`,
//! `Player` and `Entity.move`: per-tick acceleration and friction, gravity
//! and drag, axis-by-axis collision, step-up, standing, sneaking and
//! crawling poses with their own hitboxes, swimming, climbing, and blocks
//! that slow, slide or bounce. Values are per tick (20 ticks a second).

use glam::DVec3;

use super::collision::{Aabb, Collider};
use crate::config::{
    AIR_ACCELERATION, AIR_FRICTION, CROUCH_EYE_HEIGHT, CROUCH_HEIGHT, EYE_HEIGHT, FLY_ACCELERATION,
    FLY_VERTICAL_DRAG, FLY_VERTICAL_SPEED, GRAVITY, JUMP_VELOCITY, PLAYER_HEIGHT, PLAYER_WIDTH,
    SNEAK_FACTOR, SPRINT_FACTOR, SPRINT_JUMP_BOOST, STEP_HEIGHT, SWIM_EYE_HEIGHT, SWIM_HEIGHT,
    VERTICAL_DRAG, WALK_ACCELERATION,
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
    /// The sprint key; double-tapping forward also sprints.
    pub sprint: bool,
    /// Where the player looks vertically: the sine of the pitch, +1 straight
    /// up. Swimming goes where the player looks.
    pub look_y: f64,
    /// Eating, drinking or drawing a bow slows the player to a fifth.
    pub using_item: bool,
}

impl MoveInput {
    /// No keys held, looking level.
    pub const IDLE: Self = Self {
        forward: 0.0,
        strafe: 0.0,
        jump: false,
        sneak: false,
        sprint: false,
        look_y: 0.0,
        using_item: false,
    };
}

/// How the body is held: it decides the hitbox and the eyes.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum Pose {
    #[default]
    Standing,
    /// Sneaking: shorter, slower, and kept from walking off edges.
    Crouching,
    /// Swimming or crawling: a 0.6-block-tall body lying along the facing.
    Swimming,
}

/// A second forward press within this many ticks starts sprinting.
const SPRINT_DOUBLE_TAP_TICKS: u8 = 7;
/// Acceleration at full input in water or lava (modern `moveRelative`
/// speed for fluids).
const SWIM_ACCELERATION: f64 = 0.02;
/// Velocity kept per tick in water (sprint-swimming keeps more) and in lava.
const WATER_DRAG: f64 = 0.8;
const WATER_SPRINT_DRAG: f64 = 0.9;
const WATER_VERTICAL_DRAG: f64 = 0.8;
const LAVA_DRAG: f64 = 0.5;
/// Sinking per tick in water (a sixteenth of gravity, unless sprinting)
/// and in lava (a quarter).
const WATER_SINK: f64 = GRAVITY / 16.0;
const LAVA_SINK: f64 = GRAVITY / 4.0;
/// Gravity while falling under Slow Falling.
const SLOW_FALL_GRAVITY: f64 = 0.01;
/// Upward speed added each tick holding jump in a fluid.
const SWIM_UP: f64 = 0.04;
/// Fluid this deep (in blocks, over the feet) swims on jumping; shallower
/// and standing, jumping is an ordinary jump.
const FLUID_JUMP_THRESHOLD: f64 = 0.4;
/// Speed added each tick along the water's flow (modern
/// `updateFluidHeightAndDoFluidPushing`).
pub const CURRENT_PUSH: f64 = 0.014;
/// Upward speed when swimming into a ledge with room above it.
const LEDGE_HOP: f64 = 0.3;
/// A second jump press within this many ticks toggles creative flight.
const FLY_DOUBLE_TAP_TICKS: u8 = 7;
/// Forward input needed to start or keep sprinting on land.
const SPRINT_MIN_FORWARD: f64 = 0.8;
/// Any forward input at all, which is all sprinting under water needs.
const FORWARD_IMPULSE: f64 = 1.0e-5;
/// Every movement input is scaled by this (`moveForward *= 0.98`).
const INPUT_SCALE: f64 = 0.98;
/// Using an item scales the input by this.
const USING_ITEM_SCALE: f64 = 0.2;
/// Sneaking backs a move off an edge in steps this long.
const SNEAK_EDGE_STEP: f64 = 0.05;
/// Most a climbing body moves sideways or falls, per tick, and the upward
/// speed that sets when it pushes against the ladder or jumps.
const CLIMB_SIDEWAYS: f64 = 0.15;
const CLIMB_FALL: f64 = 0.15;
const CLIMB_UP: f64 = 0.2;
/// Walking on a bouncy block slows by 0.4 plus a fifth of the fall speed.
const BOUNCY_SLOWDOWN: f64 = 0.4;
const BOUNCY_SLOWDOWN_PER_SPEED: f64 = 0.2;
/// How far under the feet the block that affects movement is.
const BELOW_FEET: f64 = 0.500_001;
/// Water and lava are felt through a hitbox shrunk by this.
const FLUID_MARGIN: f64 = 0.001;

/// The player's physical body.
#[derive(Debug, Clone, PartialEq)]
pub struct Body {
    /// The hitbox is the stored state: deriving it from the feet position
    /// every tick would let rounding push it into a wall.
    bb: Aabb,
    /// Blocks per tick.
    pub velocity: DVec3,
    pub on_ground: bool,
    /// Whether sneak is held (and the body isn't flying).
    pub sneaking: bool,
    pub sprinting: bool,
    /// Sprinting in water with the head under (or having been): the body
    /// lies flat and goes where it looks.
    pub swimming: bool,
    /// Double-tapping jump toggles `flying` (creative).
    pub may_fly: bool,
    pub flying: bool,
    /// False when too hungry to sprint.
    pub can_sprint: bool,
    /// Flies freely whatever the ground (flying and swimming mobs).
    pub free_flight: bool,
    /// Whether the last tick started a jump.
    pub jumped: bool,
    /// The liquid the body was in during the last tick.
    pub liquid: Option<Fluid>,
    /// Whether the last tick landed on a bouncy block and bounced.
    pub bounced: bool,
    /// Whether the body is in a ladder or vine.
    pub on_climbable: bool,
    /// What status effects do to moving (M26): walking scaled by Speed and
    /// Slowness, jumps raised by Jump Boost, falls slowed by Slow Falling.
    pub speed_factor: f64,
    pub jump_bonus: f64,
    pub slow_falling: bool,
    /// Gliding on elytra (M28): a lying 0.6-block body steered by where
    /// it looks, which Game starts and stops.
    pub fall_flying: bool,
    /// How hard the last glide tick hit a wall: modern's kinetic energy,
    /// `(speed lost) × 10 − 3`, when above 0.
    pub glide_impact: f64,
    pose: Pose,
    /// Whether this is the player, who has poses (mobs stay standing).
    has_poses: bool,
    collided_horizontally: bool,
    was_moving_forward: bool,
    sprint_tap_timer: u8,
    was_jumping: bool,
    fly_tap_timer: u8,
    /// Height of the hitbox and of the eyes when standing.
    height: f64,
    eye: f64,
}

impl Body {
    /// The player's body, with its feet at `position`.
    pub fn new(position: DVec3) -> Self {
        let mut body = Self::sized(position, PLAYER_WIDTH, PLAYER_HEIGHT, EYE_HEIGHT);
        body.has_poses = true;
        body
    }

    /// A body of any size (mobs), with its feet at `position`.
    pub fn sized(position: DVec3, width: f64, height: f64, eye_height: f64) -> Self {
        Self {
            bb: sized_hitbox(position, width, height),
            height,
            eye: eye_height,
            velocity: DVec3::ZERO,
            on_ground: false,
            sneaking: false,
            sprinting: false,
            swimming: false,
            may_fly: false,
            flying: false,
            can_sprint: true,
            free_flight: false,
            jumped: false,
            liquid: None,
            bounced: false,
            on_climbable: false,
            pose: Pose::Standing,
            has_poses: false,
            collided_horizontally: false,
            was_moving_forward: false,
            sprint_tap_timer: 0,
            was_jumping: false,
            fly_tap_timer: 0,
            speed_factor: 1.0,
            jump_bonus: 0.0,
            slow_falling: false,
            fall_flying: false,
            glide_impact: 0.0,
        }
    }

    pub fn aabb(&self) -> Aabb {
        self.bb
    }

    pub fn pose(&self) -> Pose {
        self.pose
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

    /// Camera height above the feet in the current pose.
    pub fn eye_height(&self) -> f64 {
        match self.pose {
            Pose::Standing => self.eye,
            Pose::Crouching => CROUCH_EYE_HEIGHT,
            Pose::Swimming => SWIM_EYE_HEIGHT,
        }
    }

    /// Hitbox height in `pose`.
    fn height_in(&self, pose: Pose) -> f64 {
        match pose {
            Pose::Standing => self.height,
            Pose::Crouching => CROUCH_HEIGHT,
            Pose::Swimming => SWIM_HEIGHT,
        }
    }

    /// Whether the body moves at a crouch's pace: crouching, or crawling
    /// out of the water.
    pub fn moving_slowly(&self) -> bool {
        self.pose == Pose::Crouching || (self.pose == Pose::Swimming && !self.swimming)
    }

    /// The hitbox the body would have in `pose`, feet staying put.
    fn box_in(&self, pose: Pose) -> Aabb {
        let mut bb = self.bb;
        bb.max.y = bb.min.y + self.height_in(pose);
        bb
    }

    /// The cell whose block the body stands on, for friction, bouncing and
    /// the second look at speed and jump factors.
    fn cell_below(&self) -> (i32, i32, i32) {
        let p = self.position();
        (
            p.x.floor() as i32,
            (self.bb.min.y - BELOW_FEET).floor() as i32,
            p.z.floor() as i32,
        )
    }

    /// The cell the feet are in.
    fn feet_cell(&self) -> (i32, i32, i32) {
        let p = self.position();
        (p.x.floor() as i32, p.y.floor() as i32, p.z.floor() as i32)
    }

    /// `factor` of the block at the feet, or of the block below if that is
    /// plain (modern `getBlockSpeedFactor` and `getBlockJumpFactor`).
    fn block_factor(&self, factor: impl Fn(i32, i32, i32) -> f32) -> f64 {
        let (x, y, z) = self.feet_cell();
        let here = factor(x, y, z) as f64;
        if here != 1.0 {
            return here;
        }
        let (x, y, z) = self.cell_below();
        factor(x, y, z) as f64
    }

    /// Advances one tick. `yaw` is in radians, 0 facing north (−Z).
    pub fn tick(&mut self, input: &MoveInput, yaw: f32, world: &impl Collider) {
        if self.free_flight {
            self.may_fly = true;
            self.flying = true;
        } else {
            self.update_flight(input.jump);
        }
        self.sneaking = input.sneak && !self.flying;
        if self.fall_flying && (self.flying || self.on_ground || self.liquid.is_some()) {
            self.fall_flying = false;
        }
        self.glide_impact = 0.0;
        let eyes_in_water = self.eyes_in_water(world);
        self.update_swimming(eyes_in_water);
        self.update_pose(world);
        if self.fall_flying {
            self.glide(yaw, input.look_y, world);
            return;
        }

        let mut forward = input.forward.clamp(-1.0, 1.0);
        let mut strafe = input.strafe.clamp(-1.0, 1.0);
        if self.moving_slowly() {
            forward *= SNEAK_FACTOR;
            strafe *= SNEAK_FACTOR;
        }
        if input.using_item {
            forward *= USING_ITEM_SCALE;
            strafe *= USING_ITEM_SCALE;
        }
        self.update_sprint(forward, input, eyes_in_water);

        let liquid = if self.flying {
            None
        } else {
            self.find_liquid(world)
        };
        self.liquid = liquid;
        let region = self.bb.grow(-FLUID_MARGIN);
        let depth = liquid.map_or(0.0, |fluid| world.fluid_depth(&region, fluid));
        if liquid == Some(Fluid::Water) {
            self.velocity += world.current(&region) * CURRENT_PUSH;
        }
        self.jumped = false;
        self.bounced = false;

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
        } else if input.jump {
            let swims_up = liquid.is_some() && (!self.on_ground || depth > FLUID_JUMP_THRESHOLD);
            if swims_up {
                self.velocity.y += SWIM_UP;
            } else if self.on_ground {
                self.jumped = true;
                let power = JUMP_VELOCITY * self.block_factor(|x, y, z| world.jump_factor(x, y, z))
                    + self.jump_bonus;
                self.velocity.y = self.velocity.y.max(power);
                if self.sprinting {
                    self.velocity += facing * SPRINT_JUMP_BOOST;
                }
            }
        }

        let friction = self.friction(world);
        let sprint = if self.sprinting { SPRINT_FACTOR } else { 1.0 };
        let acceleration = if self.flying {
            FLY_ACCELERATION
        } else if liquid.is_some() {
            SWIM_ACCELERATION
        } else if self.on_ground {
            // Independent of friction at normal slipperiness: the speed
            // attribute times 0.216 over the block's friction cubed.
            WALK_ACCELERATION * sprint * self.speed_factor * 0.216
                / block_friction(friction).powi(3)
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
            self.swim(fluid, input, world);
            return;
        }
        self.walk_or_fly(input, friction, world);
    }

    /// One tick of gliding (modern `LivingEntity.updateFallFlying`'s travel):
    /// gravity eased by the lift of the look angle, diving trades height
    /// for speed, climbing trades speed for height, and the heading
    /// follows the look.
    fn glide(&mut self, yaw: f32, look_y: f64, world: &impl Collider) {
        self.liquid = self.find_liquid(world);
        let (sin_yaw, cos_yaw) = (yaw as f64).sin_cos();
        let cos_pitch = (1.0 - look_y * look_y).max(0.0).sqrt();
        let look = DVec3::new(sin_yaw * cos_pitch, look_y, -cos_yaw * cos_pitch);
        let flat_look = cos_pitch;
        let flat_speed = self.velocity.x.hypot(self.velocity.z);
        let lift = cos_pitch * cos_pitch;
        let mut v = self.velocity;
        v.y += GRAVITY * (-1.0 + lift * 0.75);
        if v.y < 0.0 && flat_look > 0.0 {
            let d = v.y * -0.1 * lift;
            v += DVec3::new(look.x * d / flat_look, d, look.z * d / flat_look);
        }
        if look_y > 0.0 && flat_look > 0.0 {
            let d = flat_speed * look_y * 0.04;
            v += DVec3::new(-look.x * d / flat_look, d * 3.2, -look.z * d / flat_look);
        }
        if flat_look > 0.0 {
            v.x += (look.x / flat_look * flat_speed - v.x) * 0.1;
            v.z += (look.z / flat_look * flat_speed - v.z) * 0.1;
        }
        self.velocity = DVec3::new(v.x * 0.99, v.y * 0.98, v.z * 0.99);
        self.move_by(self.velocity, world);
        if self.collided_horizontally {
            let after = self.velocity.x.hypot(self.velocity.z);
            self.glide_impact = ((flat_speed - after) * 10.0 - 3.0).max(0.0);
        }
        if self.on_ground {
            self.fall_flying = false;
        }
    }

    /// Ground, air, ladders and flight: one tick of the rest of
    /// `travel`.
    fn walk_or_fly(&mut self, input: &MoveInput, friction: f64, world: &impl Collider) {
        self.on_climbable = !self.flying && self.in_climbable(world);
        if self.on_climbable {
            // A ladder holds a body to a crawl: slow sideways, slow falling,
            // and not at all while sneaking.
            self.velocity.x = self.velocity.x.clamp(-CLIMB_SIDEWAYS, CLIMB_SIDEWAYS);
            self.velocity.z = self.velocity.z.clamp(-CLIMB_SIDEWAYS, CLIMB_SIDEWAYS);
            self.velocity.y = self.velocity.y.max(-CLIMB_FALL);
            if self.velocity.y < 0.0 && self.sneaking {
                self.velocity.y = 0.0;
            }
        }
        let vertical = self.velocity.y;
        self.move_by(self.velocity, world);
        if self.on_ground && !self.flying {
            self.land(vertical, world);
        }
        if self.in_climbable(world) && !self.flying && (self.collided_horizontally || input.jump) {
            self.velocity.y = CLIMB_UP;
        }
        // The block underfoot or around the feet slows the walker.
        let factor = self.block_factor(|x, y, z| world.speed_factor(x, y, z));
        self.velocity.x *= factor;
        self.velocity.z *= factor;
        if self.flying {
            // Flight keeps 0.6 of the vertical speed and ignores gravity.
            self.velocity.y = vertical * FLY_VERTICAL_DRAG;
            if self.on_ground && !self.free_flight {
                self.flying = false;
            }
        } else {
            // Slow Falling: modern's 0.01 gravity while falling.
            let gravity = if self.slow_falling && self.velocity.y <= 0.0 {
                SLOW_FALL_GRAVITY
            } else {
                GRAVITY
            };
            self.velocity.y = (self.velocity.y - gravity) * VERTICAL_DRAG;
        }
        self.velocity.x *= friction;
        self.velocity.z *= friction;
    }

    /// What touching down does: a bouncy block throws a body back up
    /// (unless sneaking) and slows walking across it.
    fn land(&mut self, fall_speed: f64, world: &impl Collider) {
        let (x, y, z) = self.cell_below();
        if !world.bouncy(x, y, z) || self.sneaking {
            return;
        }
        if fall_speed < 0.0 {
            self.velocity.y = -fall_speed;
            self.bounced = true;
        } else if fall_speed.abs() < 0.1 {
            // Walking over it drags.
            let slow = BOUNCY_SLOWDOWN + fall_speed.abs() * BOUNCY_SLOWDOWN_PER_SPEED;
            self.velocity.x *= slow;
            self.velocity.z *= slow;
        }
    }

    /// Whether the body is inside a ladder or vine.
    fn in_climbable(&self, world: &impl Collider) -> bool {
        let (x, y, z) = self.feet_cell();
        world.climbable(x, y, z)
    }

    /// Whether the eyes are under water.
    fn eyes_in_water(&self, world: &impl Collider) -> bool {
        // Modern `updateFluidOnEyes` looks a ninth of a block below the eyes.
        let eyes = self.position() + DVec3::Y * (self.eye_height() - 1.0 / 9.0);
        world.fluid_at(eyes) == Some(Fluid::Water)
    }

    /// Sprinting in water starts swimming once the head is under, and
    /// keeps going while in water (modern `updateSwimming`).
    fn update_swimming(&mut self, eyes_in_water: bool) {
        if self.flying || !self.has_poses {
            self.swimming = false;
            return;
        }
        let in_water = self.liquid == Some(Fluid::Water);
        self.swimming = if self.swimming {
            self.sprinting && in_water
        } else {
            self.sprinting && eyes_in_water && in_water
        };
    }

    /// Chooses the pose the body can fit in (modern `updatePlayerPose`):
    /// swimming if swimming, crouching if sneaking, else standing; if that
    /// doesn't fit, crouching, and failing that, crawling.
    fn update_pose(&mut self, world: &impl Collider) {
        if !self.has_poses {
            return;
        }
        let fits = |pose: Pose| !collides(&self.box_in(pose), world);
        if !fits(Pose::Swimming) {
            return;
        }
        let wanted = if self.swimming || self.fall_flying {
            Pose::Swimming
        } else if self.sneaking {
            Pose::Crouching
        } else {
            Pose::Standing
        };
        let pose = if fits(wanted) {
            wanted
        } else if fits(Pose::Crouching) {
            Pose::Crouching
        } else {
            Pose::Swimming
        };
        self.bb = self.box_in(pose);
        self.pose = pose;
    }

    /// The liquid the body is in: any the hitbox, shrunk a hair, touches
    /// (water first). Standing in a puddle counts.
    fn find_liquid(&self, world: &impl Collider) -> Option<Fluid> {
        world.liquid(&self.bb.grow(-FLUID_MARGIN))
    }

    /// Moves through water or lava: thick drag, slow sinking, swimming where
    /// the player looks, and a hop out onto a ledge when swimming into it.
    fn swim(&mut self, fluid: Fluid, input: &MoveInput, world: &impl Collider) {
        if self.swimming && fluid == Fluid::Water {
            // A swimmer's vertical speed eases toward where they look.
            let look = input.look_y;
            let rate = if look < -0.2 { 0.085 } else { 0.06 };
            let head = self.position() + DVec3::Y * (self.height_in(self.pose) - 0.1);
            let surfacing = world.fluid_at(head).is_some();
            if look <= 0.0 || input.jump || surfacing {
                self.velocity.y += (look - self.velocity.y) * rate;
            }
        }
        let start_y = self.bb.min.y;
        self.move_by(self.velocity, world);
        self.on_climbable = self.in_climbable(world);
        if self.collided_horizontally && self.on_climbable {
            self.velocity.y = CLIMB_UP;
        }
        let (drag, vertical_drag) = match fluid {
            Fluid::Water if self.sprinting => (WATER_SPRINT_DRAG, WATER_VERTICAL_DRAG),
            Fluid::Water => (WATER_DRAG, WATER_VERTICAL_DRAG),
            Fluid::Lava => (LAVA_DRAG, LAVA_DRAG),
        };
        self.velocity.x *= drag;
        self.velocity.z *= drag;
        self.velocity.y *= vertical_drag;
        match fluid {
            // A sprinting swimmer doesn't sink.
            Fluid::Water if !self.sprinting => self.velocity.y -= WATER_SINK,
            Fluid::Water => {}
            Fluid::Lava => self.velocity.y -= LAVA_SINK,
        }
        let climbed = self.bb.min.y - start_y;
        let ledge = DVec3::new(
            self.velocity.x,
            self.velocity.y + 0.6 - climbed,
            self.velocity.z,
        );
        let above = self.bb.offset(ledge);
        if self.collided_horizontally
            && !collides(&above, world)
            && world.liquid(&above.grow(-FLUID_MARGIN)).is_none()
        {
            self.velocity.y = LEDGE_HOP;
        }
    }

    /// Double-tap forward to sprint, or hold the sprint key. Sprinting stops
    /// when forward is released, while sneaking or crawling, on hitting a
    /// wall, when too hungry, and in water unless the head is under.
    fn update_sprint(&mut self, forward: f64, input: &MoveInput, eyes_in_water: bool) {
        self.sprint_tap_timer = self.sprint_tap_timer.saturating_sub(1);
        let enough = if eyes_in_water {
            forward > FORWARD_IMPULSE
        } else {
            forward >= SPRINT_MIN_FORWARD
        };
        let can_start = (self.on_ground || eyes_in_water)
            && enough
            && self.can_sprint
            && !self.moving_slowly()
            && !input.using_item;
        if can_start && !self.was_moving_forward && !self.sprinting {
            if self.sprint_tap_timer == 0 {
                self.sprint_tap_timer = SPRINT_DOUBLE_TAP_TICKS;
            } else {
                self.sprinting = true;
                self.sprint_tap_timer = 0;
            }
        }
        if input.sprint && can_start {
            self.sprinting = true;
        }
        let in_water_not_under = self.liquid == Some(Fluid::Water) && !eyes_in_water;
        if self.sprinting
            && (forward <= FORWARD_IMPULSE
                || self.moving_slowly()
                || self.collided_horizontally
                || !self.can_sprint
                || in_water_not_under)
        {
            self.sprinting = false;
        }
        self.was_moving_forward = enough;
    }

    /// Double-tapping jump toggles flight when allowed (`flyToggleTimer`).
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
        let (x, y, z) = self.cell_below();
        world.slipperiness(x, y, z) as f64 * AIR_FRICTION
    }

    /// Moves by `wanted`, stopping at solid blocks and stepping up ledges.
    fn move_by(&mut self, wanted: DVec3, world: &impl Collider) {
        let start = self.aabb();
        let mut wanted = wanted;
        if self.on_ground && self.sneaking && wanted.y <= 0.0 {
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

/// The block's own friction (slipperiness) from the friction a tick keeps,
/// which includes the air's 0.91.
fn block_friction(kept: f64) -> f64 {
    kept / AIR_FRICTION
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
/// feet: backs off in 0.05-block steps while nothing solid is within a step
/// height (0.6) below the moved hitbox (modern `maybeBackOffFromEdge`).
fn back_off_edges(bb: &Aabb, mut dx: f64, mut dz: f64, world: &impl Collider) -> (f64, f64) {
    let unsupported =
        |dx: f64, dz: f64| !collides(&bb.offset(DVec3::new(dx, -STEP_HEIGHT, dz)), world);
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
        /// 0.6 high, the tallest step the player takes.
        Step,
        /// The top half of a block, a low ceiling.
        TopHalf,
        Ice,
        Water,
        /// Not solid; climbable.
        Ladder,
        /// Soul sand: walking speed ×0.4.
        Soul,
        /// Honey: walking ×0.4, jumping ×0.5.
        Honey,
        /// Slime: bounces, sticky to walk on, slippery 0.8.
        Slime,
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
                let solid = !matches!(kind, Kind::Water | Kind::Ladder);
                if solid && pos.cmpge(min).all() && pos.cmple(max).all() {
                    let mut bb = Aabb::block(pos.x, pos.y, pos.z);
                    match kind {
                        Kind::Half => bb.max.y -= 0.5,
                        Kind::Step => bb.max.y -= 0.4,
                        Kind::TopHalf => bb.min.y += 0.5,
                        _ => {}
                    }
                    out.push(bb);
                }
            }
        }

        fn slipperiness(&self, x: i32, y: i32, z: i32) -> f32 {
            match self.blocks.get(&IVec3::new(x, y, z)) {
                Some(Kind::Ice) => 0.98,
                Some(Kind::Slime) => 0.8,
                _ => 0.6,
            }
        }

        fn speed_factor(&self, x: i32, y: i32, z: i32) -> f32 {
            match self.blocks.get(&IVec3::new(x, y, z)) {
                Some(Kind::Soul | Kind::Honey) => 0.4,
                _ => 1.0,
            }
        }

        fn jump_factor(&self, x: i32, y: i32, z: i32) -> f32 {
            match self.blocks.get(&IVec3::new(x, y, z)) {
                Some(Kind::Honey) => 0.5,
                _ => 1.0,
            }
        }

        fn climbable(&self, x: i32, y: i32, z: i32) -> bool {
            self.blocks.get(&IVec3::new(x, y, z)) == Some(&Kind::Ladder)
        }

        fn bouncy(&self, x: i32, y: i32, z: i32) -> bool {
            self.blocks.get(&IVec3::new(x, y, z)) == Some(&Kind::Slime)
        }

        fn liquid(&self, region: &Aabb) -> Option<Fluid> {
            (self.water_depth(region) > 0.0).then_some(Fluid::Water)
        }

        fn fluid_depth(&self, region: &Aabb, fluid: Fluid) -> f64 {
            if fluid == Fluid::Water {
                self.water_depth(region)
            } else {
                0.0
            }
        }

        fn fluid_at(&self, point: DVec3) -> Option<Fluid> {
            let cell = point.floor().as_ivec3();
            let full = self.blocks.get(&(cell + IVec3::Y)) == Some(&Kind::Water);
            let surface = cell.y as f64 + if full { 1.0 } else { 8.0 / 9.0 };
            (self.blocks.get(&cell) == Some(&Kind::Water) && point.y < surface)
                .then_some(Fluid::Water)
        }
    }

    impl TestWorld {
        /// How far above the bottom of `region` the highest water it
        /// touches reaches.
        fn water_depth(&self, region: &Aabb) -> f64 {
            let (min, max) = (region.min.floor().as_ivec3(), region.max.floor().as_ivec3());
            self.blocks
                .iter()
                .filter(|&(pos, &kind)| {
                    kind == Kind::Water && pos.cmpge(min).all() && pos.cmple(max).all()
                })
                .map(|(pos, _)| pos.y as f64 + 8.0 / 9.0 - region.min.y)
                .filter(|&depth| depth >= 0.0)
                .fold(0.0, f64::max)
        }
    }

    fn standing_at(x: f64, z: f64) -> Body {
        let mut body = Body::new(DVec3::new(x, 64.0, z));
        body.on_ground = true;
        body
    }

    const FORWARD: MoveInput = MoveInput {
        forward: 1.0,
        ..MoveInput::IDLE
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
        assert_eq!(body.eye_height(), CROUCH_EYE_HEIGHT);
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
        jump: true,
        ..MoveInput::IDLE
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
    fn sinking_and_swimming_up_match_modern_minecraft() {
        let world = pool();
        // ×0.8 then −0.005 (gravity/16) per tick settles at −0.025; holding
        // jump adds 0.04 first and settles at 0.8 × (v + 0.04) − 0.005 = +0.135.
        let sinking = vertical_speed_after(&world, MoveInput::default(), 30);
        assert!((sinking + 0.025).abs() < 1e-3, "{sinking}");
        let rising = vertical_speed_after(&world, JUMP, 30);
        assert!((rising - 0.135).abs() < 1e-3, "{rising}");
    }

    #[test]
    fn swimming_speed_matches_modern_minecraft() {
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

    /// Steady horizontal speed in blocks per second, over a tick after `ticks`.
    fn steady(world: &TestWorld, input: MoveInput, ticks: u32, start: Body) -> f64 {
        let mut body = start;
        for _ in 0..ticks {
            body.tick(&input, 0.0, world);
        }
        let before = body.position();
        body.tick(&input, 0.0, world);
        let moved = body.position() - before;
        moved.x.hypot(moved.z) * TICKS_PER_SECOND as f64
    }

    #[test]
    fn modern_poses_have_their_own_hitboxes_and_eyes() {
        let world = TestWorld::floor(Kind::Full);
        let mut body = standing_at(0.5, 0.5);
        let height = |body: &Body| body.aabb().max.y - body.aabb().min.y;
        body.tick(&MoveInput::IDLE, 0.0, &world);
        assert!((height(&body) - 1.8).abs() < 1e-9 && body.eye_height() == 1.62);
        let sneak = MoveInput {
            sneak: true,
            ..MoveInput::IDLE
        };
        body.tick(&sneak, 0.0, &world);
        assert_eq!(body.pose(), Pose::Crouching);
        assert!((height(&body) - 1.5).abs() < 1e-9 && body.eye_height() == 1.27);
        // The feet and the width stay put.
        assert_eq!(body.position(), DVec3::new(0.5, 64.0, 0.5));
        assert!((body.aabb().max.x - body.aabb().min.x - 0.6).abs() < 1e-12);
        body.tick(&MoveInput::IDLE, 0.0, &world);
        assert_eq!(body.pose(), Pose::Standing);
        assert!((height(&body) - 1.8).abs() < 1e-9);
    }

    #[test]
    fn a_low_ceiling_keeps_you_crouched_and_a_gap_makes_you_crawl() {
        let sneak = MoveInput {
            sneak: true,
            ..MoveInput::IDLE
        };
        // A ceiling 1.5 above the floor fits a crouch but not a stand.
        let mut world = TestWorld::floor(Kind::Full);
        world.set(0, 65, 0, Kind::TopHalf);
        let mut body = standing_at(0.5, 0.5);
        body.tick(&sneak, 0.0, &world);
        assert_eq!(body.pose(), Pose::Crouching);
        body.tick(&MoveInput::IDLE, 0.0, &world);
        assert_eq!(body.pose(), Pose::Crouching, "no room to stand");

        // A full block over one gap: 1 block high, only crawling fits.
        let mut world = TestWorld::floor(Kind::Full);
        world.set(0, 65, 0, Kind::Full);
        let mut body = standing_at(0.5, 0.5);
        body.tick(&MoveInput::IDLE, 0.0, &world);
        assert_eq!(body.pose(), Pose::Swimming);
        assert!((body.aabb().max.y - body.aabb().min.y - 0.6).abs() < 1e-12);
        assert_eq!(body.eye_height(), 0.4);
        assert!(!body.swimming, "crawling is not swimming");
        assert!(body.moving_slowly());
        // Crawling is as slow as sneaking, and you stand up once clear.
        let mut crawl_world = TestWorld::floor(Kind::Full);
        for z in -3..=0 {
            crawl_world.set(0, 65, z, Kind::Full);
        }
        let speed = steady(&crawl_world, FORWARD, 60, standing_at(0.5, 0.5));
        assert!((speed - 1.31).abs() < 0.03, "crawl {speed}");
        let mut body = standing_at(0.5, 0.5);
        for _ in 0..80 {
            body.tick(&FORWARD, 0.0, &crawl_world);
        }
        assert_eq!(
            body.pose(),
            Pose::Standing,
            "out of the tunnel at z {}",
            body.position().z
        );
    }

    #[test]
    fn sneaking_slows_to_a_third_and_walks_down_a_step_but_not_off_a_cliff() {
        let world = TestWorld::floor(Kind::Full);
        let sneak = MoveInput {
            sneak: true,
            ..FORWARD
        };
        let speed = steady(&world, sneak, 60, standing_at(0.5, 0.5));
        assert!((speed - 1.31).abs() < 0.03, "{speed}");

        // The floor drops by 0.5 ahead (a half slab): within a step, so it's fine.
        let mut world = TestWorld::floor(Kind::Full);
        for z in -50..-3 {
            world.set(0, 63, z, Kind::Half);
        }
        let mut body = standing_at(0.5, 0.5);
        for _ in 0..80 {
            body.tick(&sneak, 0.0, &world);
        }
        assert!(
            body.position().z < -4.0,
            "walked down the step: {}",
            body.position()
        );
        assert!(body.position().y <= 63.5 + 1e-9);

        // A one-block drop stops a sneaking player at the edge.
        let mut world = TestWorld::floor(Kind::Full);
        for z in -50..-3 {
            world.blocks.remove(&IVec3::new(0, 63, z));
        }
        let mut body = standing_at(0.5, 0.5);
        for _ in 0..80 {
            body.tick(&sneak, 0.0, &world);
        }
        assert!(
            body.position().z > -3.5 && body.on_ground,
            "{}",
            body.position()
        );
    }

    #[test]
    fn steps_are_six_tenths_of_a_block() {
        assert_eq!(STEP_HEIGHT, 0.6);
        let mut world = TestWorld::floor(Kind::Full);
        world.set(0, 64, -3, Kind::Step);
        let mut body = standing_at(0.5, 0.5);
        for _ in 0..30 {
            body.tick(&FORWARD, 0.0, &world);
            if body.position().z < -2.5 {
                break;
            }
        }
        assert!(body.position().z < -2.5, "{}", body.position());
        assert!(
            (body.position().y - 64.6).abs() < 1e-9,
            "stepped up 0.6: {}",
            body.position()
        );
    }

    #[test]
    fn sprinting_starts_and_stops_by_modern_rules() {
        let world = TestWorld::floor(Kind::Full);
        let key = MoveInput {
            sprint: true,
            ..FORWARD
        };
        // The sprint key starts it; letting go of forward stops it.
        let mut body = standing_at(0.5, 0.5);
        body.tick(&key, 0.0, &world);
        assert!(body.sprinting);
        body.tick(&MoveInput::IDLE, 0.0, &world);
        assert!(!body.sprinting);
        // Too hungry (food 6 or less) can't start or keep it.
        let mut hungry = standing_at(0.5, 0.5);
        hungry.can_sprint = false;
        hungry.tick(&key, 0.0, &world);
        assert!(!hungry.sprinting);
        let mut body = standing_at(0.5, 0.5);
        body.tick(&key, 0.0, &world);
        body.can_sprint = false;
        body.tick(&key, 0.0, &world);
        assert!(!body.sprinting);
        // Sneaking stops it, and eating won't let it start.
        let mut body = standing_at(0.5, 0.5);
        body.tick(&key, 0.0, &world);
        body.tick(&MoveInput { sneak: true, ..key }, 0.0, &world);
        assert!(!body.sprinting);
        let mut body = standing_at(0.5, 0.5);
        body.tick(
            &MoveInput {
                using_item: true,
                ..key
            },
            0.0,
            &world,
        );
        assert!(!body.sprinting);
        // Strafing alone doesn't count.
        let mut body = standing_at(0.5, 0.5);
        body.tick(
            &MoveInput {
                strafe: 1.0,
                sprint: true,
                ..MoveInput::IDLE
            },
            0.0,
            &world,
        );
        assert!(!body.sprinting);
    }

    #[test]
    fn eating_slows_you_to_a_fifth() {
        let world = TestWorld::floor(Kind::Full);
        let using = MoveInput {
            using_item: true,
            ..FORWARD
        };
        let slow = steady(&world, using, 120, standing_at(0.5, 0.5));
        assert!((slow - 4.317 * 0.2).abs() < 0.02, "{slow}");
    }

    #[test]
    fn sprint_jumping_carries_further_than_walking_jumps() {
        let world = TestWorld::floor(Kind::Full);
        let jump_distance = |sprint: bool| {
            let mut body = standing_at(0.5, 0.5);
            let input = MoveInput {
                sprint,
                jump: true,
                ..FORWARD
            };
            for _ in 0..30 {
                body.tick(&input, 0.0, &world);
            }
            // Run the first jump's whole arc, then see how far it went.
            0.5 - body.position().z
        };
        assert!(jump_distance(true) > jump_distance(false) * 1.2);
    }

    /// A pool of water over a stone floor, 10 deep, and a body in it.
    fn deep_water() -> TestWorld {
        let mut world = TestWorld::default();
        for z in -50..50 {
            for x in -50..50 {
                world.set(x, 53, z, Kind::Full);
                for y in 54..64 {
                    world.set(x, y, z, Kind::Water);
                }
            }
        }
        world
    }

    #[test]
    fn sprinting_with_your_head_under_water_swims_flat_and_fast() {
        let world = deep_water();
        let mut body = Body::new(DVec3::new(0.5, 58.0, 0.5));
        body.can_sprint = true;
        let dive = MoveInput {
            sprint: true,
            ..FORWARD
        };
        for _ in 0..40 {
            body.tick(&dive, 0.0, &world);
        }
        assert!(body.swimming && body.sprinting);
        assert_eq!(body.pose(), Pose::Swimming);
        assert!((body.aabb().max.y - body.aabb().min.y - 0.6).abs() < 1e-12);
        // 0.02 × 0.98 / (1 − 0.9) blocks a tick, against 1.96 b/s walking.
        let speed = steady(&world, dive, 40, body.clone());
        assert!((speed - 3.92).abs() < 0.05, "{speed}");
        let wading = steady(&world, FORWARD, 60, Body::new(DVec3::new(0.5, 58.0, 0.5)));
        assert!((wading - 1.96).abs() < 0.03, "{wading}");
    }

    #[test]
    fn a_swimmer_goes_where_they_look() {
        let world = deep_water();
        let swim = |look: f64| {
            let mut body = Body::new(DVec3::new(0.5, 58.0, 0.5));
            let input = MoveInput {
                sprint: true,
                look_y: look,
                ..FORWARD
            };
            for _ in 0..30 {
                body.tick(&input, 0.0, &world);
            }
            (body.position().y - 58.0, body.swimming)
        };
        let (up, swimming) = swim(0.7);
        assert!(swimming && up > 3.0, "{up}");
        let (down, _) = swim(-0.7);
        assert!(down < -2.0, "{down}");
        let (level, _) = swim(0.0);
        assert!(level.abs() < 1.0, "{level}");
    }

    #[test]
    fn sprinting_at_the_surface_ends_in_a_wade() {
        let world = deep_water();
        // Head out of the water: a sprint can't start in it, and an ongoing
        // one stops.
        let mut body = Body::new(DVec3::new(0.5, 62.5, 0.5));
        body.sprinting = true;
        body.tick(&FORWARD, 0.0, &world);
        body.tick(&FORWARD, 0.0, &world);
        assert!(!body.sprinting && !body.swimming);
    }

    #[test]
    fn jumping_in_shallow_water_hops_but_in_deep_water_swims_up() {
        // Water one cell deep over the floor: 0.89 high, past the 0.4 threshold,
        // so jumping swims. A thin layer (a flowing edge) is an ordinary jump.
        let mut world = TestWorld::floor(Kind::Full);
        world.set(0, 64, 0, Kind::Water);
        let mut body = standing_at(0.5, 0.5);
        body.tick(&JUMP, 0.0, &world);
        assert!(!body.jumped, "deep enough to swim");
        assert!(body.velocity.y > 0.0 && body.velocity.y < 0.1);
    }

    #[test]
    fn ladders_climb_at_2_35_blocks_a_second_and_slide_at_3() {
        let mut world = TestWorld::floor(Kind::Full);
        // A wall with a ladder on its face, 20 high.
        for y in 64..84 {
            world.set(0, y, -1, Kind::Full);
            world.set(0, y, 0, Kind::Ladder);
        }
        let mut body = standing_at(0.5, 0.5);
        let climb = MoveInput {
            forward: 1.0,
            ..MoveInput::IDLE
        };
        for _ in 0..20 {
            body.tick(&climb, 0.0, &world);
        }
        let before = body.position().y;
        for _ in 0..20 {
            body.tick(&climb, 0.0, &world);
        }
        let speed = (body.position().y - before) / 20.0 * TICKS_PER_SECOND as f64;
        // 0.1176 blocks a tick once the push of 0.2 meets gravity.
        assert!((speed - 2.352).abs() < 0.05, "climbing {speed}");
        assert!(body.on_climbable);

        // Letting go slides down at no more than 0.15 a tick (3 b/s).
        for _ in 0..10 {
            body.tick(&MoveInput::IDLE, 0.0, &world);
        }
        let before = body.position().y;
        for _ in 0..10 {
            body.tick(&MoveInput::IDLE, 0.0, &world);
        }
        let slide = (before - body.position().y) / 10.0 * TICKS_PER_SECOND as f64;
        assert!((slide - 3.0).abs() < 0.1, "sliding {slide}");

        // Sneaking holds you on the ladder.
        let held = body.position().y;
        let sneak = MoveInput {
            sneak: true,
            ..MoveInput::IDLE
        };
        for _ in 0..20 {
            body.tick(&sneak, 0.0, &world);
        }
        assert!(
            (body.position().y - held).abs() < 0.2,
            "{} {held}",
            body.position().y
        );
        // Jumping climbs too, even without pushing into the wall.
        let jump = MoveInput {
            jump: true,
            ..MoveInput::IDLE
        };
        let before = body.position().y;
        for _ in 0..20 {
            body.tick(&jump, 0.0, &world);
        }
        assert!(body.position().y > before + 1.5);
    }

    #[test]
    fn soul_sand_and_honey_slow_walking_and_honey_halves_jumps() {
        let normal = steady(
            &TestWorld::floor(Kind::Full),
            FORWARD,
            80,
            standing_at(0.5, 0.5),
        );
        let soul = steady(
            &TestWorld::floor(Kind::Soul),
            FORWARD,
            80,
            standing_at(0.5, 0.5),
        );
        // 0.4 of the velocity is kept each tick: 0.098 / (1 − 0.4 × 0.546) − 0.098 ... = 2.51 b/s.
        assert!((soul - 2.51).abs() < 0.05, "soul sand {soul}");
        assert!(soul < normal * 0.6);
        let honey = steady(
            &TestWorld::floor(Kind::Honey),
            FORWARD,
            80,
            standing_at(0.5, 0.5),
        );
        assert!((honey - soul).abs() < 1e-6);

        let jump_speed = |kind| {
            let world = TestWorld::floor(kind);
            let mut body = standing_at(0.5, 0.5);
            body.tick(&JUMP, 0.0, &world);
            body.velocity.y
        };
        // The first tick's velocity after gravity and drag.
        let plain = jump_speed(Kind::Full);
        let sticky = jump_speed(Kind::Honey);
        assert!(
            (plain - (0.42 - GRAVITY) * VERTICAL_DRAG).abs() < 1e-9,
            "{plain}"
        );
        assert!(
            (sticky - (0.21 - GRAVITY) * VERTICAL_DRAG).abs() < 1e-9,
            "{sticky}"
        );
    }

    #[test]
    fn slime_bounces_unless_you_sneak() {
        let world = TestWorld::floor(Kind::Slime);
        let mut body = Body::new(DVec3::new(0.5, 70.0, 0.5));
        let mut fall = 0.0;
        for _ in 0..60 {
            fall = body.velocity.y;
            body.tick(&MoveInput::IDLE, 0.0, &world);
            if body.bounced {
                break;
            }
        }
        assert!(body.bounced, "never bounced");
        // Thrown back up as fast as it came down (then a tick of gravity).
        assert!(fall < -0.5);
        assert!(
            (body.velocity.y - (-fall - GRAVITY) * VERTICAL_DRAG).abs() < 1e-9,
            "{} {fall}",
            body.velocity.y
        );

        let mut sneaker = Body::new(DVec3::new(0.5, 70.0, 0.5));
        let sneak = MoveInput {
            sneak: true,
            ..MoveInput::IDLE
        };
        for _ in 0..60 {
            sneaker.tick(&sneak, 0.0, &world);
        }
        assert!(!sneaker.bounced && sneaker.on_ground);
        assert!(sneaker.velocity.y.abs() < 0.1);
    }

    #[test]
    fn ice_and_slime_slip_by_their_slipperiness() {
        let stop_distance = |kind| {
            let world = TestWorld::floor(kind);
            let mut body = standing_at(0.5, 0.5);
            for _ in 0..40 {
                body.tick(&FORWARD, 0.0, &world);
            }
            let released = body.position();
            for _ in 0..300 {
                body.tick(&MoveInput::IDLE, 0.0, &world);
            }
            (body.position() - released).length()
        };
        let (stone, slime, ice) = (
            stop_distance(Kind::Full),
            stop_distance(Kind::Slime),
            stop_distance(Kind::Ice),
        );
        assert!(stone < slime && slime < ice, "{stone} {slime} {ice}");
    }
}
