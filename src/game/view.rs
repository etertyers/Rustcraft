//! How the player looks to the camera: the swing animation, the held
//! item's equip dip and lag behind the mouse, view bobbing, the body
//! turning after the head and the walking animation. These follow modern
//! Minecraft's `LivingEntity`, `LocalPlayer` and `ItemInHandRenderer` and
//! run on the game's ticks; the renderer reads them with the partial tick.
//! Also the F5 views: first person, third person behind and in front.

use std::f32::consts::{PI, TAU};

use glam::{DVec3, IVec3, Vec3};
use winit::keyboard::KeyCode;

use super::Game;
use crate::{
    inventory::item::ItemId,
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
/// Ticks of drawing a bow at which it's fully drawn.
const BOW_FULL_DRAW: f32 = 20.0;
/// Ticks of eating before the food is eaten.
const EAT_TICKS: f32 = 32.0;
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
    /// The item type in the selected slot.
    pub item: Option<ItemId>,
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

        self.prev_swim = self.swim;
        self.swim = if t.swimming {
            (self.swim + SWIM_STEP).min(1.0)
        } else {
            (self.swim - SWIM_STEP).max(0.0)
        };
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

    /// Starts a swing and runs `ticks` of it (for `--shot` scenes).
    pub fn swing_for_shot(&mut self, ticks: u32) {
        self.view.swing();
        self.run_ticks(ticks + 1);
    }

    /// Holds sneak for a moment (for `--shot` scenes).
    pub fn sneak_for_shot(&mut self) {
        self.keys.set_key(KeyCode::ShiftLeft, true);
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
            swimming: self.is_swimming(),
            item: self.inventory.selected_stack().map(|stack| stack.id),
        };
        self.view.tick(&info);
    }

    /// Swimming: sprinting in water, with the head under (modern
    /// `isSwimming`). Crawling joins this with M17.
    pub fn is_swimming(&self) -> bool {
        self.body.sprinting && self.body.liquid == Some(crate::world::block::Fluid::Water)
    }

    /// The camera drawing the frame: the player's eyes, or pulled back
    /// (or around in front) for F5, shaking with each step.
    pub fn view_camera(&self) -> Camera {
        let alpha = self.tick_alpha() as f32;
        let mut camera = self.camera;
        camera.bob = self.view.view_bob(alpha);
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
        let blocks = self.items.blocks;
        let blocking = |cell: IVec3| {
            world
                .block(cell.x, cell.y, cell.z)
                .and_then(|block| blocks.get(block.id()))
                .is_some_and(|def| def.solid)
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
            if let Some(hit) = raycast::raycast(start, dir, wanted + CLIP_MARGIN, blocking) {
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
        let holding = self.inventory.selected_stack().is_some();
        let main = if drawing {
            ArmPose::Bow
        } else if eating.is_some() {
            ArmPose::Eat
        } else if holding {
            ArmPose::Item
        } else {
            ArmPose::Empty
        };
        PlayerPose {
            limb_swing,
            limb_amount,
            age: view.age(alpha),
            head_yaw: wrap(self.camera.yaw - body_yaw),
            head_pitch: -self.camera.pitch,
            crouching: self.body.sneaking,
            swim_amount: view.swim_amount(alpha),
            in_water: self.body.liquid.is_some(),
            fall_flying: 0.0,
            attack: view.attack_anim(alpha),
            swing_left: false,
            right: main,
            left: if drawing {
                ArmPose::Bow
            } else {
                ArmPose::Empty
            },
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
