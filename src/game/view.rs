//! How the player looks to the camera: the swing animation, the held
//! item's equip dip and lag behind the mouse, view bobbing, the body
//! turning after the head and the walking animation. These follow modern
//! Minecraft's `LivingEntity`, `LocalPlayer` and `ItemInHandRenderer` and
//! run on the game's ticks; the renderer reads them with the partial tick.
//! Also the F5 views: first person, third person behind and in front.

use std::f32::consts::{PI, TAU};

use crate::bindings::Action;
use glam::{DVec3, IVec3, Vec3};

use super::Game;
use crate::{
    inventory::{item::ItemId, player::Hand},
    player::raycast,
    render::{
        camera::{Camera, ViewBob},
        player::{ArmPose, PlayerPose},
    },
};

/// Ticks a swing takes (vanilla's `getCurrentSwingDuration`, no haste).
pub const SWING_TICKS: i32 = 6;
/// How far behind or in front of the head the third-person camera sits.
pub const THIRD_PERSON_DISTANCE: f64 = 4.0;
/// Third-person clipping probes the eye's corners this far out.
const CLIP_MARGIN: f64 = 0.1;
/// How long the view shakes after a hit (vanilla's `hurtDuration`).
const HURT_TICKS: u32 = 10;
/// Ticks of drawing a bow at which it's fully drawn.
const BOW_FULL_DRAW: f32 = 20.0;
/// Ticks of eating before the food is eaten.
const EAT_TICKS: f32 = 32.0;
/// The view widens by this much sprinting (the speed attribute's 1.3,
/// halfway to 1) and flying; a fully drawn bow narrows it by 15%.
const SPRINTING_FOV: f32 = 1.15;
const FLYING_FOV: f32 = 1.1;
const BOW_ZOOM: f32 = 0.15;
/// The view and the eyes ease to where they're going by this much a tick.
const FOV_EASE: f32 = 0.5;
const EYE_EASE: f32 = 0.5;
/// Swimming eases in and out by this much a tick.
const SWIM_STEP: f32 = 0.09;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum ViewMode {
    /// Through the player's eyes, with the hand.
    #[default]
    First,
    /// Behind the player's head, looking the same way.
    ThirdBack,
    /// In front of the player, looking back at them.
    ThirdFront,
}

impl ViewMode {
    /// F5: first person, then behind, then in front, then around again.
    pub fn next(self) -> Self {
        match self {
            Self::First => Self::ThirdBack,
            Self::ThirdBack => Self::ThirdFront,
            Self::ThirdFront => Self::First,
        }
    }
}

/// What a tick tells the view about the player.
#[derive(Debug, Clone, Copy)]
pub struct TickInfo {
    /// How far the feet moved this tick.
    pub moved: DVec3,
    pub on_ground: bool,
    /// Where the player looks (radians; pitch positive looks up).
    pub yaw: f32,
    pub pitch: f32,
    /// Swimming (sprinting in water) or crawling.
    pub swimming: bool,
    /// The item type in the selected slot, and in the off-hand.
    pub item: Option<ItemId>,
    pub offhand: Option<ItemId>,
    /// Sprinting, and flying in creative: both widen the view.
    pub sprinting: bool,
    pub flying: bool,
    /// Ticks a bow has been drawn, which narrows the view.
    pub bow_ticks: Option<u32>,
    /// Where the eyes are above the feet now (the pose's).
    pub eye_height: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ViewState {
    pub mode: ViewMode,
    pub bobbing: bool,
    age: u32,
    swinging: bool,
    swing_time: i32,
    prev_attack: f32,
    attack: f32,
    prev_hand_height: f32,
    hand_height: f32,
    hand_item: Option<ItemId>,
    prev_off_height: f32,
    off_height: f32,
    off_item: Option<ItemId>,
    prev_x_bob: f32,
    x_bob: f32,
    prev_y_bob: f32,
    y_bob: f32,
    prev_walk_dist: f32,
    walk_dist: f32,
    prev_bob: f32,
    bob: f32,
    prev_body_yaw: f32,
    body_yaw: f32,
    prev_limb_amount: f32,
    limb_amount: f32,
    limb_swing: f32,
    prev_swim: f32,
    swim: f32,
    prev_fov: f32,
    fov: f32,
    prev_eye: f32,
    eye: f32,
}

/// An angle wrapped to −π..π.
fn wrap(angle: f32) -> f32 {
    (angle + PI).rem_euclid(TAU) - PI
}

fn lerp(alpha: f32, a: f32, b: f32) -> f32 {
    a + (b - a) * alpha
}

impl ViewState {
    pub fn new(yaw: f32, pitch: f32, item: Option<ItemId>) -> Self {
        Self {
            mode: ViewMode::First,
            bobbing: true,
            age: 0,
            swinging: false,
            swing_time: 0,
            prev_attack: 0.0,
            attack: 0.0,
            prev_hand_height: 1.0,
            hand_height: 1.0,
            hand_item: item,
            prev_off_height: 1.0,
            off_height: 1.0,
            off_item: None,
            prev_x_bob: pitch.to_degrees(),
            x_bob: pitch.to_degrees(),
            prev_y_bob: yaw.to_degrees(),
            y_bob: yaw.to_degrees(),
            prev_walk_dist: 0.0,
            walk_dist: 0.0,
            prev_bob: 0.0,
            bob: 0.0,
            prev_body_yaw: yaw,
            body_yaw: yaw,
            prev_limb_amount: 0.0,
            limb_amount: 0.0,
            limb_swing: 0.0,
            prev_swim: 0.0,
            swim: 0.0,
            prev_fov: 1.0,
            fov: 1.0,
            prev_eye: crate::config::EYE_HEIGHT as f32,
            eye: crate::config::EYE_HEIGHT as f32,
        }
    }

    /// Starts a swing (vanilla's `LivingEntity.swing`): not while one is
    /// under way and less than half done.
    pub fn swing(&mut self) {
        if !self.swinging || self.swing_time >= SWING_TICKS / 2 || self.swing_time < 0 {
            self.swing_time = -1;
            self.swinging = true;
        }
    }

    /// Whether the arm is swinging.
    pub fn is_swinging(&self) -> bool {
        self.swinging
    }

    /// One game tick.
    pub fn tick(&mut self, t: &TickInfo) {
        self.age += 1;

        // The swing: time counts up, and `attack` is how far through it.
        self.prev_attack = self.attack;
        if self.swinging {
            self.swing_time += 1;
            if self.swing_time >= SWING_TICKS {
                self.swing_time = 0;
                self.swinging = false;
            }
        } else {
            self.swing_time = 0;
        }
        self.attack = self.swing_time as f32 / SWING_TICKS as f32;

        // The hand drops when the item changes and comes back up.
        self.prev_hand_height = self.hand_height;
        let wanted = if self.hand_item == t.item { 1.0 } else { 0.0 };
        self.hand_height += (wanted - self.hand_height).clamp(-0.4, 0.4);
        if self.hand_height < 0.1 {
            self.hand_item = t.item;
        }

        // The off-hand dips and rises the same way.
        self.prev_off_height = self.off_height;
        let wanted = if self.off_item == t.offhand { 1.0 } else { 0.0 };
        self.off_height += (wanted - self.off_height).clamp(-0.4, 0.4);
        if self.off_height < 0.1 {
            self.off_item = t.offhand;
        }

        // The hand lags behind where the mouse points.
        self.prev_x_bob = self.x_bob;
        self.prev_y_bob = self.y_bob;
        self.x_bob += (t.pitch.to_degrees() - self.x_bob) * 0.5;
        self.y_bob += wrap((t.yaw.to_degrees() - self.y_bob).to_radians()).to_degrees() * 0.5;

        // Walking: distance for the bob's rhythm and how strong it is.
        let walked = (t.moved.x * t.moved.x + t.moved.z * t.moved.z).sqrt() as f32;
        self.prev_walk_dist = self.walk_dist;
        self.walk_dist += walked * 0.6;
        self.prev_bob = self.bob;
        let target = if t.on_ground && !t.swimming {
            walked.min(0.1)
        } else {
            0.0
        };
        self.bob += (target - self.bob) * 0.4;

        // The legs: swing faster the faster the player moves.
        self.prev_limb_amount = self.limb_amount;
        self.limb_amount += ((walked * 4.0).min(1.0) - self.limb_amount) * 0.4;
        self.limb_swing += self.limb_amount;

        self.tick_body_yaw(t, walked);

        self.tick_fov(t);
        self.prev_eye = self.eye;
        self.eye += (t.eye_height as f32 - self.eye) * EYE_EASE;

        self.prev_swim = self.swim;
        self.swim = if t.swimming {
            (self.swim + SWIM_STEP).min(1.0)
        } else {
            (self.swim - SWIM_STEP).max(0.0)
        };
    }

    /// The view widens when sprinting or flying and narrows drawing a bow,
    /// easing toward it by half each tick (modern `getFieldOfViewModifier`).
    fn tick_fov(&mut self, t: &TickInfo) {
        let mut target = 1.0;
        if t.flying {
            target *= FLYING_FOV;
        }
        if t.sprinting {
            target *= SPRINTING_FOV;
        }
        if let Some(ticks) = t.bow_ticks {
            let drawn = (ticks as f32 / BOW_FULL_DRAW).min(1.0);
            target *= 1.0 - drawn * drawn * BOW_ZOOM;
        }
        self.prev_fov = self.fov;
        self.fov += (target - self.fov) * FOV_EASE;
    }

    /// What the field of view is multiplied by, `alpha` of the way between ticks.
    pub fn fov_modifier(&self, alpha: f32) -> f32 {
        lerp(alpha, self.prev_fov, self.fov)
    }

    /// The camera's height above the feet, easing between poses.
    pub fn eye_height(&self, alpha: f32) -> f32 {
        lerp(alpha, self.prev_eye, self.eye)
    }

    /// The body turns after the head (vanilla's `tickHeadTurn`): toward
    /// where it's walking, and never more than 75° from the head.
    fn tick_body_yaw(&mut self, t: &TickInfo, walked: f32) {
        self.prev_body_yaw = self.body_yaw;
        let mut target = self.body_yaw;
        if walked * walked > 0.0025 {
            let heading = (t.moved.x as f32).atan2(-t.moved.z as f32);
            let away = wrap(t.yaw - heading).abs();
            // Backing up keeps the body facing forward.
            target = if away > 95f32.to_radians() && away < 265f32.to_radians() {
                heading + PI
            } else {
                heading
            };
        }
        if self.attack > 0.0 {
            target = t.yaw;
        }
        self.body_yaw += wrap(target - self.body_yaw) * 0.3;
        let mut turn = wrap(t.yaw - self.body_yaw);
        let limit = 75f32.to_radians();
        turn = turn.clamp(-limit, limit);
        self.body_yaw = t.yaw - turn;
        if turn * turn > 50f32.to_radians().powi(2) {
            self.body_yaw += turn * 0.2;
        }
    }

    /// How far the player walked in the last tick (vanilla's walk
    /// distance, 0.6 a block) and the walking speed it is compared with
    /// (the movement speed attribute, 0.1).
    pub fn walk_step(&self) -> (f32, f32) {
        (self.walk_dist - self.prev_walk_dist, 0.1)
    }

    /// The swing's progress, 0–1, `alpha` of the way between ticks
    /// (vanilla's `getAttackAnim`).
    pub fn attack_anim(&self, alpha: f32) -> f32 {
        let mut step = self.attack - self.prev_attack;
        if step < 0.0 {
            step += 1.0;
        }
        self.prev_attack + step * alpha
    }

    /// How far the hand is lowered, 0 when raised: 0–1.
    pub fn equip_progress(&self, alpha: f32) -> f32 {
        1.0 - lerp(alpha, self.prev_hand_height, self.hand_height)
    }

    /// How far the off-hand is lowered, 0 when raised.
    pub fn offhand_equip_progress(&self, alpha: f32) -> f32 {
        1.0 - lerp(alpha, self.prev_off_height, self.off_height)
    }

    /// The item the off-hand shows, which lags while it dips.
    pub fn offhand_item(&self) -> Option<ItemId> {
        self.off_item
    }

    /// The item the hand shows, which lags the selection while the hand dips.
    pub fn hand_item(&self) -> Option<ItemId> {
        self.hand_item
    }

    /// The hand's drift behind the mouse in degrees: (pitch, yaw) of the
    /// camera minus the lagging values, times the hand's 0.1 factor.
    pub fn hand_lag(&self, alpha: f32, yaw: f32, pitch: f32) -> (f32, f32) {
        let x = lerp(alpha, self.prev_x_bob, self.x_bob);
        let y = self.prev_y_bob
            + wrap((self.y_bob - self.prev_y_bob).to_radians()).to_degrees() * alpha;
        let dy = wrap((yaw.to_degrees() - y).to_radians()).to_degrees();
        ((pitch.to_degrees() - x) * 0.1, dy * 0.1)
    }

    /// The shake of walking, for the camera and the hand.
    pub fn view_bob(&self, alpha: f32) -> ViewBob {
        if !self.bobbing {
            return ViewBob::NONE;
        }
        let step = self.walk_dist - self.prev_walk_dist;
        let phase = -(self.walk_dist + step * alpha);
        let strength = lerp(alpha, self.prev_bob, self.bob);
        let (sin, cos) = ((phase * PI).sin(), (phase * PI).cos());
        ViewBob {
            sideways: sin * strength * 0.5,
            down: -(cos * strength).abs(),
            roll: (sin * strength * 3.0).to_radians(),
            pitch: (((phase * PI - 0.2).cos() * strength).abs() * 5.0).to_radians(),
        }
    }

    /// The body's facing, `alpha` of the way between ticks.
    pub fn body_yaw(&self, alpha: f32) -> f32 {
        self.prev_body_yaw + wrap(self.body_yaw - self.prev_body_yaw) * alpha
    }

    /// Distance walked and its speed, for the legs.
    pub fn limbs(&self, alpha: f32) -> (f32, f32) {
        let amount = lerp(alpha, self.prev_limb_amount, self.limb_amount);
        (self.limb_swing - self.limb_amount * (1.0 - alpha), amount)
    }

    pub fn swim_amount(&self, alpha: f32) -> f32 {
        lerp(alpha, self.prev_swim, self.swim)
    }

    pub fn age(&self, alpha: f32) -> f32 {
        self.age as f32 + alpha
    }
}

impl Game {
    /// The player's view settings.
    pub fn view(&self) -> &ViewState {
        &self.view
    }

    /// Chooses the view directly (for `--view`).
    pub fn set_view_mode(&mut self, mode: ViewMode) {
        self.view.mode = mode;
    }

    pub fn view_mode(&self) -> ViewMode {
        self.view.mode
    }

    /// Starts a swing and runs `ticks` of it (for `--shot` scenes).
    pub fn swing_for_shot(&mut self, ticks: u32) {
        self.view.swing();
        self.run_ticks(ticks + 1);
    }

    /// Holds sneak for a moment (for `--shot` scenes).
    pub fn sneak_for_shot(&mut self) {
        self.keys.set_key(Action::Sneak, true);
        self.run_ticks(3);
    }

    /// F5.
    pub fn cycle_view(&mut self) {
        self.view.mode = self.view.mode.next();
    }

    /// Feeds the view one tick of the player's movement.
    pub(super) fn tick_view(&mut self) {
        let info = TickInfo {
            moved: self.body.position() - self.prev_feet,
            on_ground: self.body.on_ground,
            yaw: self.camera.yaw,
            pitch: self.camera.pitch,
            swimming: self.body.pose() == crate::player::physics::Pose::Swimming,
            item: self.inventory.selected_stack().map(|stack| stack.id),
            offhand: self
                .inventory
                .slot(crate::inventory::player::OFFHAND)
                .map(|stack| stack.id),
            sprinting: self.body.sprinting,
            flying: self.body.flying,
            bow_ticks: self.drawing,
            eye_height: self.body.eye_height(),
        };
        self.view.tick(&info);
    }

    /// Swimming: sprinting in water after putting the head under (modern
    /// `isSwimming`).
    pub fn is_swimming(&self) -> bool {
        self.body.swimming
    }

    /// The view's roll just after a hit (vanilla `bobHurt`), scaled by
    /// Damage Tilt: up to 14° over half a second.
    pub fn hurt_tilt(&self, alpha: f32) -> f32 {
        let ticks = self.stats.hurt_ticks();
        if ticks == 0 || self.stats.is_dead() {
            return 0.0;
        }
        let f = ((ticks as f32 - alpha) / HURT_TICKS as f32).max(0.0);
        -((f.powi(4) * PI).sin() * 14.0 * self.damage_tilt).to_radians()
    }

    /// The camera drawing the frame: the player's eyes, or pulled back
    /// (or around in front) for F5, shaking with each step.
    pub fn view_camera(&self) -> Camera {
        let alpha = self.tick_alpha() as f32;
        let mut camera = self.camera;
        camera.bob = self.view.view_bob(alpha);
        camera.bob.roll += self.hurt_tilt(alpha);
        // FOV Effects scales how far sprinting, flying and potions move it.
        let modifier = 1.0 + (self.view.fov_modifier(alpha) - 1.0) * self.fov_effects;
        camera.fov = (camera.fov * modifier).clamp(1.0, 170.0);
        let flip = match self.view.mode {
            ViewMode::First => return camera,
            ViewMode::ThirdBack => false,
            ViewMode::ThirdFront => true,
        };
        // Looking back at the player turns the camera around.
        if flip {
            camera.yaw = (camera.yaw + PI).rem_euclid(TAU);
            camera.pitch = -camera.pitch;
        }
        camera.bob = ViewBob::NONE;
        let eye = self.camera.position.as_dvec3();
        let back = -camera.forward().as_dvec3();
        let distance = self.clip_distance(eye, back, THIRD_PERSON_DISTANCE);
        camera.position = (eye + back * distance).as_vec3();
        camera
    }

    /// How far the camera can back off from `eye` along `dir` before a
    /// block gets in the way: the shortest of eight rays from the corners
    /// of a small box around the eye (vanilla's `getMaxZoom`).
    pub fn clip_distance(&self, eye: DVec3, dir: DVec3, wanted: f64) -> f64 {
        let world = self.chunks.world();
        let shapes = crate::world::shape::shapes();
        // The blocks' real shapes, so a ladder you're on or a slab beside
        // you only stops the camera where its boxes are.
        let boxes = |cell: IVec3| {
            world
                .block(cell.x, cell.y, cell.z)
                .map_or(&[][..], |block| shapes.collision(block))
        };
        let mut distance = wanted;
        for i in 0..8 {
            let offset = DVec3::new(
                if i & 1 == 0 {
                    -CLIP_MARGIN
                } else {
                    CLIP_MARGIN
                },
                if i & 2 == 0 {
                    -CLIP_MARGIN
                } else {
                    CLIP_MARGIN
                },
                if i & 4 == 0 {
                    -CLIP_MARGIN
                } else {
                    CLIP_MARGIN
                },
            );
            let start = eye + offset;
            if let Some(hit) = raycast::raycast_boxes(start, dir, wanted + CLIP_MARGIN, boxes) {
                distance = distance.min(hit.point.distance(start));
            }
        }
        distance
    }

    /// The player as the model draws it this frame.
    pub fn player_pose(&self) -> PlayerPose {
        let alpha = self.tick_alpha() as f32;
        let view = &self.view;
        let body_yaw = view.body_yaw(alpha);
        let (limb_swing, limb_amount) = view.limbs(alpha);
        let eating = self.eating.map(|(_, ticks)| ticks as f32);
        let drawing = self.drawing.is_some();
        let blocking = self.is_blocking();
        let arm = |hand: Hand| {
            let holding = self.inventory.hand_stack(hand).is_some();
            if drawing && self.drawing_hand == hand {
                ArmPose::Bow
            } else if blocking && self.shield_hand == Some(hand) {
                ArmPose::Block
            } else if eating.is_some() && self.use_hand == hand {
                ArmPose::Eat
            } else if holding {
                ArmPose::Item
            } else {
                ArmPose::Empty
            }
        };
        let (main, off) = (arm(Hand::Main), arm(Hand::Off));
        // A left-handed player holds the main hand's item on the left.
        let (right, left) = if self.left_handed {
            (off, main)
        } else {
            (main, off)
        };
        PlayerPose {
            limb_swing,
            limb_amount,
            age: view.age(alpha),
            head_yaw: wrap(self.camera.yaw - body_yaw),
            head_pitch: -self.camera.pitch,
            crouching: self.body.pose() == crate::player::physics::Pose::Crouching,
            swim_amount: view.swim_amount(alpha),
            in_water: self.body.liquid == Some(crate::world::block::Fluid::Water),
            fall_flying: if self.body.fall_flying {
                self.glide_ticks() as f32 + alpha
            } else {
                0.0
            },
            attack: view.attack_anim(alpha),
            swing_left: self.left_handed,
            riding: self.is_riding(),
            right: if drawing { ArmPose::Bow } else { right },
            left: if drawing { ArmPose::Bow } else { left },
            use_time: eating.map_or(0.0, |t| t + alpha),
        }
    }

    /// The body's facing and feet, `alpha` of the way between ticks.
    pub fn player_body(&self) -> (Vec3, f32) {
        let alpha = self.tick_alpha();
        let feet = self.prev_feet.lerp(self.body.position(), alpha);
        (feet.as_vec3(), self.view.body_yaw(alpha as f32))
    }

    /// The hand's use of the selected item: eating (ticks left and the
    /// total) or drawing a bow (ticks drawn, 0–1 scale), for the
    /// first-person hand.
    pub fn use_progress(&self) -> HandUse {
        let alpha = self.tick_alpha() as f32;
        if let Some(ticks) = self.drawing {
            return HandUse::Bow {
                drawn: ticks as f32 + alpha,
                full: BOW_FULL_DRAW,
            };
        }
        if let Some((_, ticks)) = self.eating {
            return HandUse::Eat {
                remaining: EAT_TICKS - ticks as f32 - alpha,
                total: EAT_TICKS,
            };
        }
        HandUse::None
    }
}

/// What the main hand is using the item for.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum HandUse {
    None,
    /// Eating or drinking: ticks left and in all.
    Eat {
        remaining: f32,
        total: f32,
    },
    /// Drawing a bow: ticks drawn so far and for a full draw.
    Bow {
        drawn: f32,
        full: f32,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn walking(dx: f64, dz: f64) -> TickInfo {
        TickInfo {
            moved: DVec3::new(dx, 0.0, dz),
            on_ground: true,
            yaw: 0.0,
            pitch: 0.0,
            swimming: false,
            item: None,
            offhand: None,
            sprinting: false,
            flying: false,
            bow_ticks: None,
            eye_height: 1.62,
        }
    }

    #[test]
    fn a_swing_takes_six_ticks_and_cannot_restart_early() {
        let mut view = ViewState::new(0.0, 0.0, None);
        let still = walking(0.0, 0.0);
        assert_eq!(view.attack_anim(0.5), 0.0);
        view.swing();
        let mut progress = Vec::new();
        for _ in 0..8 {
            view.tick(&still);
            progress.push(view.attack);
        }
        // swing_time counts -1.. then 0..: 0, 1/6, ... 5/6, then done.
        assert_eq!(progress[0], 0.0);
        assert!((progress[1] - 1.0 / 6.0).abs() < 1e-6);
        assert!((progress[5] - 5.0 / 6.0).abs() < 1e-6);
        assert_eq!(progress[6], 0.0);
        assert!(!view.is_swinging());

        // Swinging again before the half-way mark does nothing...
        view.swing();
        view.tick(&still);
        view.tick(&still);
        let before = view.swing_time;
        view.swing();
        assert_eq!(view.swing_time, before);
        // ...but after it, the swing starts over.
        for _ in 0..2 {
            view.tick(&still);
        }
        view.swing();
        assert_eq!(view.swing_time, -1);
    }

    #[test]
    fn attack_progress_interpolates_and_wraps_at_the_end() {
        let mut view = ViewState::new(0.0, 0.0, None);
        view.prev_attack = 5.0 / 6.0;
        view.attack = 0.0;
        // Passing the end carries on into the next lap instead of jumping back.
        assert!((view.attack_anim(0.5) - (5.0 / 6.0 + 0.5 / 6.0)).abs() < 1e-6);
        view.prev_attack = 0.25;
        view.attack = 0.5;
        assert!((view.attack_anim(0.5) - 0.375).abs() < 1e-6);
    }

    #[test]
    fn changing_item_dips_the_hand_then_raises_it_with_the_new_one() {
        let sword = Some(ItemId::default());
        let mut view = ViewState::new(0.0, 0.0, sword);
        let none = TickInfo {
            item: None,
            ..walking(0.0, 0.0)
        };
        assert_eq!(view.equip_progress(1.0), 0.0);
        // Switching: it drops by at most 0.4 a tick...
        view.tick(&none);
        assert!((view.hand_height - 0.6).abs() < 1e-6);
        assert_eq!(view.hand_item(), sword, "still shows the old item");
        view.tick(&none);
        view.tick(&none);
        assert!(view.hand_height < 0.1);
        // ...and once it's low, the new item takes its place and rises.
        assert_eq!(view.hand_item(), None);
        view.tick(&none);
        view.tick(&none);
        view.tick(&none);
        assert_eq!(view.hand_height, 1.0);
        assert_eq!(view.equip_progress(1.0), 0.0);
        // The same item, worn down, doesn't dip.
        let mut view = ViewState::new(0.0, 0.0, sword);
        let same = TickInfo {
            item: sword,
            ..walking(0.0, 0.0)
        };
        view.tick(&same);
        assert_eq!(view.hand_height, 1.0);
    }

    #[test]
    fn the_hand_lags_behind_the_mouse() {
        let mut view = ViewState::new(0.0, 0.0, None);
        let turned = TickInfo {
            yaw: 90f32.to_radians(),
            pitch: 30f32.to_radians(),
            ..walking(0.0, 0.0)
        };
        view.tick(&turned);
        // Halfway there after a tick: a quarter-turn lag of 45° × 0.1.
        let (pitch, yaw) = view.hand_lag(1.0, turned.yaw, turned.pitch);
        assert!((pitch - 1.5).abs() < 1e-3, "{pitch}");
        assert!((yaw - 4.5).abs() < 1e-3, "{yaw}");
        for _ in 0..20 {
            view.tick(&turned);
        }
        let (pitch, yaw) = view.hand_lag(1.0, turned.yaw, turned.pitch);
        assert!(pitch.abs() < 1e-3 && yaw.abs() < 1e-3);
        // Across the 0/360 seam the lag takes the short way round.
        let mut view = ViewState::new(359f32.to_radians(), 0.0, None);
        let over = TickInfo {
            yaw: 1f32.to_radians(),
            ..walking(0.0, 0.0)
        };
        view.tick(&over);
        let (_, yaw) = view.hand_lag(1.0, over.yaw, 0.0);
        assert!(yaw.abs() < 0.2, "{yaw}");
    }

    #[test]
    fn walking_bobs_the_view_and_standing_still_does_not() {
        let mut view = ViewState::new(0.0, 0.0, None);
        assert_eq!(view.view_bob(0.5), ViewBob::NONE);
        for _ in 0..10 {
            view.tick(&walking(0.0, -0.2159));
        }
        let bob = view.view_bob(0.5);
        assert_ne!(bob, ViewBob::NONE);
        // The shake is a few hundredths of a block and a degree or two.
        assert!(
            bob.sideways.abs() < 0.05 && bob.down <= 0.0 && bob.down > -0.1,
            "{bob:?}"
        );
        // It wears off when the player stops, or leaves the ground.
        for _ in 0..30 {
            view.tick(&walking(0.0, 0.0));
        }
        assert!(view.view_bob(0.5).down > -0.001);
        view.bobbing = false;
        view.tick(&walking(0.0, -0.2));
        assert_eq!(view.view_bob(0.5), ViewBob::NONE);
        let mut flying = ViewState::new(0.0, 0.0, None);
        for _ in 0..10 {
            flying.tick(&TickInfo {
                on_ground: false,
                ..walking(0.0, -0.2159)
            });
        }
        assert!(flying.bob.abs() < 1e-6);
    }

    #[test]
    fn the_legs_swing_with_speed() {
        let mut slow = ViewState::new(0.0, 0.0, None);
        let mut fast = ViewState::new(0.0, 0.0, None);
        for _ in 0..20 {
            slow.tick(&walking(0.0, -0.05));
            fast.tick(&walking(0.0, -0.2159));
        }
        assert!(fast.limb_amount > slow.limb_amount);
        assert!(fast.limb_swing > slow.limb_swing);
        // A full stride is about 0.86 of the way to the cap.
        assert!(
            (fast.limb_amount - 0.8636).abs() < 0.01,
            "{}",
            fast.limb_amount
        );
        let (swing, amount) = fast.limbs(1.0);
        assert!((swing - fast.limb_swing).abs() < 1e-6 && (amount - fast.limb_amount).abs() < 1e-6);
        // Standing, they settle.
        for _ in 0..30 {
            fast.tick(&walking(0.0, 0.0));
        }
        assert!(fast.limb_amount < 0.01);
    }

    #[test]
    fn the_body_follows_the_head_but_never_too_far_behind() {
        let mut view = ViewState::new(0.0, 0.0, None);
        // Turn the head to the east while standing: the body follows only
        // far enough to stay within 75° of it.
        let east = TickInfo {
            yaw: 120f32.to_radians(),
            ..walking(0.0, 0.0)
        };
        for _ in 0..40 {
            view.tick(&east);
        }
        let gap = wrap(east.yaw - view.body_yaw).to_degrees();
        assert!(gap.abs() <= 75.5, "{gap}");
        // Walking pulls the body round to face the way it goes.
        let walk_east = TickInfo {
            yaw: 90f32.to_radians(),
            ..walking(0.2, 0.0)
        };
        let mut view = ViewState::new(0.0, 0.0, None);
        for _ in 0..40 {
            view.tick(&walk_east);
        }
        assert!(wrap(view.body_yaw - 90f32.to_radians()).abs() < 0.05);
        // Backing up keeps facing forward.
        let backing = TickInfo {
            yaw: 0.0,
            ..walking(0.0, 0.2)
        };
        let mut view = ViewState::new(0.0, 0.0, None);
        for _ in 0..40 {
            view.tick(&backing);
        }
        assert!(wrap(view.body_yaw).abs() < 0.05, "{}", view.body_yaw);
    }

    #[test]
    fn swimming_eases_in_and_out() {
        let mut view = ViewState::new(0.0, 0.0, None);
        let swim = TickInfo {
            swimming: true,
            ..walking(0.0, -0.1)
        };
        for _ in 0..5 {
            view.tick(&swim);
        }
        assert!((view.swim_amount(1.0) - 0.45).abs() < 1e-5);
        for _ in 0..20 {
            view.tick(&swim);
        }
        assert_eq!(view.swim_amount(1.0), 1.0);
        for _ in 0..20 {
            view.tick(&walking(0.0, 0.0));
        }
        assert_eq!(view.swim_amount(1.0), 0.0);
    }

    #[test]
    fn the_third_person_camera_backs_off_four_blocks_and_stops_at_walls() {
        use crate::{
            game::tests::flat_game,
            player::mode::GameMode,
            world::{block::BlockState, blocks},
        };
        let mut game = flat_game(GameMode::Survival);
        game.camera.pitch = 0.0;
        game.advance(0.0);
        let eye = game.camera.position;
        // First person is the eyes themselves.
        assert_eq!(game.view_camera().position, eye);

        // Behind: four blocks back along the look direction (facing north,
        // that's south).
        game.cycle_view();
        let behind = game.view_camera();
        assert!(
            (behind.position - eye - Vec3::new(0.0, 0.0, 4.0)).length() < 1e-4,
            "{eye} {}",
            behind.position
        );
        assert_eq!(behind.yaw, game.camera.yaw);
        assert_eq!(behind.pitch, game.camera.pitch);

        // A wall two blocks behind pulls the camera in front of it.
        let wall: Vec<_> = (-2..=3)
            .flat_map(|x| {
                (63..=68).map(move |y| (IVec3::new(x, y, 2), BlockState::of(blocks::STONE)))
            })
            .collect();
        game.set_blocks(&wall);
        let pulled = game.view_camera();
        let distance = (pulled.position - eye).length();
        assert!(distance > 1.2 && distance < 1.5, "{distance}");
        assert!(pulled.position.z < 2.0, "{}", pulled.position);

        // In front, looking back at the player.
        game.cycle_view();
        let front = game.view_camera();
        assert!((front.position - eye - Vec3::new(0.0, 0.0, -4.0)).length() < 1e-4);
        assert!((front.forward() + game.camera.forward()).length() < 1e-4);

        // And around to first person again.
        game.cycle_view();
        assert_eq!(game.view().mode, ViewMode::First);
        assert_eq!(game.view_camera().position, eye);
    }

    #[test]
    fn a_ladder_youre_on_doesnt_pull_the_camera_into_your_head() {
        use crate::{
            game::tests::flat_game,
            player::mode::GameMode,
            world::{Face, block::BlockState, blocks, props::Prop},
        };
        let mut game = flat_game(GameMode::Survival);
        game.camera.pitch = 0.0;
        // A wall just north of the player with a ladder in the player's cell,
        // and a slab behind them.
        let ladder = BlockState::of(blocks::LADDER).with_face(Prop::Facing, Face::South);
        game.set_blocks(&[
            (IVec3::new(0, 65, -1), BlockState::of(blocks::STONE)),
            (IVec3::new(0, 65, 0), ladder),
            (IVec3::new(0, 64, 2), BlockState::of(blocks::OAK_SLAB)),
        ]);
        game.advance(0.0);
        let eye = game.camera.position;
        game.set_view_mode(ViewMode::ThirdBack);
        let behind = game.view_camera();
        assert!(
            ((behind.position - eye).length() - 4.0).abs() < 1e-3,
            "free behind: {}",
            (behind.position - eye).length()
        );
        // In front, the ladder's plate is the first thing in the way, not
        // the ladder's whole cell.
        game.set_view_mode(ViewMode::ThirdFront);
        let front = (game.view_camera().position - eye).length();
        assert!(front > 0.1, "{front}");
    }

    #[test]
    fn damage_tilts_the_view_and_fov_effects_scale_the_zoom() {
        use crate::{
            game::tests::flat_game,
            player::{mode::GameMode, stats::DamageSource},
        };
        let mut game = flat_game(GameMode::Survival);
        assert_eq!(game.hurt_tilt(0.0), 0.0);
        game.stats.damage(2.0, DamageSource::Mob);
        game.run_ticks(2);
        let tilt = game.hurt_tilt(0.0);
        assert!(tilt < -1f32.to_radians(), "tilted {tilt}");
        game.damage_tilt = 0.0;
        assert_eq!(game.hurt_tilt(0.0), 0.0);
        game.run_ticks(12);
        game.damage_tilt = 1.0;
        assert_eq!(game.hurt_tilt(0.0), 0.0, "over after half a second");

        game.view.fov = 1.15;
        game.view.prev_fov = 1.15;
        let fov = |game: &Game| game.view_camera().fov;
        let base = game.camera.fov;
        assert!((fov(&game) - base * 1.15).abs() < 1e-3);
        game.fov_effects = 0.0;
        assert!((fov(&game) - base).abs() < 1e-3);
        game.fov_effects = 0.5;
        assert!((fov(&game) - base * 1.075).abs() < 1e-3);
    }

    #[test]
    fn a_left_handed_player_holds_and_swings_on_the_left() {
        use crate::{
            game::tests::flat_game,
            inventory::{items, stack::ItemStack},
            player::mode::GameMode,
        };
        let mut game = flat_game(GameMode::Survival);
        game.inventory
            .set(0, Some(ItemStack::new(items::IRON_SWORD, 1)));
        let pose = game.player_pose();
        assert_eq!((pose.right, pose.left), (ArmPose::Item, ArmPose::Empty));
        game.left_handed = true;
        let pose = game.player_pose();
        assert_eq!((pose.right, pose.left), (ArmPose::Empty, ArmPose::Item));
        assert!(pose.swing_left);
    }

    #[test]
    fn the_pose_follows_what_the_player_does() {
        use crate::{game::tests::flat_game, player::mode::GameMode};
        let mut game = flat_game(GameMode::Survival);
        let pose = game.player_pose();
        assert!(!pose.crouching && pose.swim_amount == 0.0 && pose.attack == 0.0);
        assert_eq!(pose.right, ArmPose::Empty);
        // Looking straight down: the head pitches down (vanilla positive).
        assert!((pose.head_pitch - std::f32::consts::FRAC_PI_2).abs() < 0.01);

        game.sneak_for_shot();
        assert!(game.player_pose().crouching);
        game.keys.clear();
        game.run_ticks(3);

        game.view.swing();
        game.run_ticks(3);
        assert!(game.player_pose().attack > 0.0);
        game.run_ticks(10);
        assert_eq!(game.player_pose().attack, 0.0);

        game.inventory.set(
            0,
            Some(crate::inventory::stack::ItemStack::new(
                crate::inventory::items::IRON_SWORD,
                1,
            )),
        );
        game.inventory.selected = 0;
        assert_eq!(game.player_pose().right, ArmPose::Item);
    }

    #[test]
    fn left_clicks_and_placing_swing_the_arm() {
        use crate::{game::tests::flat_game, player::mode::GameMode};
        let mut game = flat_game(GameMode::Creative);
        assert!(!game.view().is_swinging());
        // A click that hits nothing still swings.
        game.camera.pitch = 0.5;
        game.on_action(Action::Attack, true, false, true);
        game.on_action(Action::Attack, false, false, true);
        game.run_ticks(1);
        assert!(game.view().is_swinging());
        game.run_ticks(8);
        assert!(!game.view().is_swinging());
        // Placing a block swings.
        // Looking at the ground a couple of blocks ahead.
        game.camera.pitch = -0.6;
        game.advance(0.0);
        game.inventory.set(
            0,
            Some(crate::inventory::stack::ItemStack::new(
                crate::inventory::items::STONE,
                4,
            )),
        );
        game.inventory.selected = 0;
        game.on_action(Action::Use, true, false, true);
        game.on_action(Action::Use, false, false, true);
        game.run_ticks(1);
        assert!(game.view().is_swinging());
    }

    #[test]
    fn the_view_widens_when_sprinting_and_flying_and_narrows_drawing_a_bow() {
        let mut view = ViewState::new(0.0, 0.0, None);
        assert_eq!(view.fov_modifier(1.0), 1.0);
        let sprint = TickInfo {
            sprinting: true,
            ..walking(0.0, -0.2)
        };
        for _ in 0..10 {
            view.tick(&sprint);
        }
        assert!((view.fov_modifier(1.0) - 1.15).abs() < 0.01);
        // Easing: half way there after one tick.
        let mut view = ViewState::new(0.0, 0.0, None);
        view.tick(&sprint);
        assert!((view.fov_modifier(1.0) - 1.075).abs() < 1e-5);
        assert!((view.fov_modifier(0.0) - 1.0).abs() < 1e-5);
        // Back to normal when sprinting stops.
        for _ in 0..10 {
            view.tick(&walking(0.0, -0.2));
        }
        assert!((view.fov_modifier(1.0) - 1.0).abs() < 0.01);
        let mut view = ViewState::new(0.0, 0.0, None);
        let fly = TickInfo {
            flying: true,
            sprinting: true,
            ..walking(0.0, -0.2)
        };
        for _ in 0..10 {
            view.tick(&fly);
        }
        assert!((view.fov_modifier(1.0) - 1.15 * 1.1).abs() < 0.01);
        // A bow drawn fully pulls the view in by 15%.
        let mut view = ViewState::new(0.0, 0.0, None);
        for _ in 0..10 {
            view.tick(&TickInfo {
                bow_ticks: Some(20),
                ..walking(0.0, 0.0)
            });
        }
        assert!((view.fov_modifier(1.0) - 0.85).abs() < 0.01);
    }

    #[test]
    fn the_camera_eases_between_eye_heights() {
        let mut view = ViewState::new(0.0, 0.0, None);
        assert_eq!(view.eye_height(1.0), 1.62);
        let crouched = TickInfo {
            eye_height: 1.27,
            ..walking(0.0, 0.0)
        };
        view.tick(&crouched);
        // Half way down after a tick, then closer and closer.
        assert!((view.eye_height(1.0) - 1.445).abs() < 1e-5);
        for _ in 0..8 {
            view.tick(&crouched);
        }
        assert!((view.eye_height(1.0) - 1.27).abs() < 0.002);
        assert!((view.eye_height(0.0) - view.eye_height(1.0)).abs() < 0.002);
    }

    #[test]
    fn sprinting_widens_the_game_view_and_sneaking_lowers_the_camera() {
        use crate::{game::tests::flat_game, player::mode::GameMode};
        let mut game = flat_game(GameMode::Survival);
        game.camera.pitch = 0.0;
        let base = game.view_camera().fov;
        game.on_action(Action::Forward, true, false, true);
        game.on_action(Action::Sprint, true, false, true);
        game.run_ticks(15);
        game.advance(0.0);
        assert!(game.body.sprinting);
        let fov = game.view_camera().fov;
        assert!((fov / base - 1.15).abs() < 0.02, "{base} -> {fov}");
        game.on_action(Action::Forward, false, false, true);
        game.on_action(Action::Sprint, false, false, true);
        game.run_ticks(20);
        game.advance(0.0);
        assert!((game.view_camera().fov / base - 1.0).abs() < 0.01);

        // Sneaking: the hitbox and eyes drop, the camera follows.
        let standing_eye = game.camera.position.y - game.body.position().y as f32;
        assert!((standing_eye - 1.62).abs() < 0.01, "{standing_eye}");
        game.on_action(Action::Sneak, true, false, true);
        game.run_ticks(10);
        game.advance(0.0);
        let sneaking_eye = game.camera.position.y - game.body.position().y as f32;
        assert!((sneaking_eye - 1.27).abs() < 0.01, "{sneaking_eye}");
        assert!(game.player_pose().crouching);
    }

    #[test]
    fn f5_cycles_the_three_views() {
        let mut mode = ViewMode::First;
        let mut seen = Vec::new();
        for _ in 0..4 {
            seen.push(mode);
            mode = mode.next();
        }
        assert_eq!(
            seen,
            [
                ViewMode::First,
                ViewMode::ThirdBack,
                ViewMode::ThirdFront,
                ViewMode::First
            ]
        );
    }
}
