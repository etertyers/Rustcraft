//! The player's model, after modern Minecraft's `PlayerModel` and
//! `HumanoidModel`: a 64×64 skin on a head, body, two arms and two legs,
//! each with a second layer (hat, jacket, sleeves, pants) a little larger,
//! posed for walking, sneaking, swimming and crawling, swinging, using an
//! item and gliding. Poses use vanilla's formulas, in model pixels and
//! radians, so the numbers can be checked against the game.

use std::f32::consts::{FRAC_PI_2, FRAC_PI_4, PI};

use glam::{Mat3, Mat4, Quat, Vec3};
use image::{Rgba, RgbaImage, imageops};

use super::models::{BoxStyle, add_box};
use crate::{entity::mobs::PartBox, world::mesh::Mesh};

/// Model pixels per block.
const PIXELS: f32 = 16.0;
/// Model y of the feet.
const FEET_Y: f32 = 24.0;

/// Players are drawn at this fraction of the model's size, which makes
/// the 32-pixel model 1.875 blocks tall (vanilla's `PlayerRenderer.scale`).
pub const PLAYER_SCALE: f32 = 0.9375;

/// The skin sheet's size.
pub const SKIN_SIZE: (f32, f32) = (64.0, 64.0);

/// The skin layers' growth over the base box, in pixels.
const HAT_GROW: f32 = 0.5;
const LAYER_GROW: f32 = 0.25;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Limb {
    Head,
    Body,
    RightArm,
    LeftArm,
    RightLeg,
    LeftLeg,
}

impl Limb {
    pub const ALL: [Limb; 6] = [
        Limb::Head,
        Limb::Body,
        Limb::RightArm,
        Limb::LeftArm,
        Limb::RightLeg,
        Limb::LeftLeg,
    ];

    fn index(self) -> usize {
        self as usize
    }

    /// Where the limb pivots in the standing pose, in model pixels.
    fn pivot(self, slim: bool) -> Vec3 {
        // Slim arms are 3 pixels wide and sit half a pixel lower.
        let arm_y = if slim { 2.5 } else { 2.0 };
        match self {
            Limb::Head | Limb::Body => Vec3::ZERO,
            Limb::RightArm => Vec3::new(-5.0, arm_y, 0.0),
            Limb::LeftArm => Vec3::new(5.0, arm_y, 0.0),
            Limb::RightLeg => Vec3::new(-1.9, 12.0, 0.1),
            Limb::LeftLeg => Vec3::new(1.9, 12.0, 0.1),
        }
    }

    /// The limb's boxes: the base layer, then the outer layer over it.
    pub fn boxes(self, slim: bool) -> [(PartBox, f32); 2] {
        let b = |uv: (f32, f32), from: (f32, f32, f32), size: (f32, f32, f32)| PartBox {
            uv,
            from,
            size,
        };
        let arm_w = if slim { 3.0 } else { 4.0 };
        match self {
            Limb::Head => [
                (b((0.0, 0.0), (-4.0, -8.0, -4.0), (8.0, 8.0, 8.0)), 0.0),
                (
                    b((32.0, 0.0), (-4.0, -8.0, -4.0), (8.0, 8.0, 8.0)),
                    HAT_GROW,
                ),
            ],
            Limb::Body => [
                (b((16.0, 16.0), (-4.0, 0.0, -2.0), (8.0, 12.0, 4.0)), 0.0),
                (
                    b((16.0, 32.0), (-4.0, 0.0, -2.0), (8.0, 12.0, 4.0)),
                    LAYER_GROW,
                ),
            ],
            Limb::RightArm => {
                let from = (if slim { -2.0 } else { -3.0 }, -2.0, -2.0);
                [
                    (b((40.0, 16.0), from, (arm_w, 12.0, 4.0)), 0.0),
                    (b((40.0, 32.0), from, (arm_w, 12.0, 4.0)), LAYER_GROW),
                ]
            }
            Limb::LeftArm => [
                (b((32.0, 48.0), (-1.0, -2.0, -2.0), (arm_w, 12.0, 4.0)), 0.0),
                (
                    b((48.0, 48.0), (-1.0, -2.0, -2.0), (arm_w, 12.0, 4.0)),
                    LAYER_GROW,
                ),
            ],
            Limb::RightLeg => [
                (b((0.0, 16.0), (-2.0, 0.0, -2.0), (4.0, 12.0, 4.0)), 0.0),
                (
                    b((0.0, 32.0), (-2.0, 0.0, -2.0), (4.0, 12.0, 4.0)),
                    LAYER_GROW,
                ),
            ],
            Limb::LeftLeg => [
                (b((16.0, 48.0), (-2.0, 0.0, -2.0), (4.0, 12.0, 4.0)), 0.0),
                (
                    b((0.0, 48.0), (-2.0, 0.0, -2.0), (4.0, 12.0, 4.0)),
                    LAYER_GROW,
                ),
            ],
        }
    }
}

/// What a hand is doing with the item in it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ArmPose {
    /// Nothing held.
    #[default]
    Empty,
    /// Holding an item out in front.
    Item,
    /// Raising a shield.
    Block,
    /// Aiming a bow (both arms).
    Bow,
    /// Bringing food or a drink to the mouth.
    Eat,
}

/// Everything the pose depends on.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct PlayerPose {
    /// Walking: distance walked (in vanilla's units) and how fast.
    pub limb_swing: f32,
    pub limb_amount: f32,
    /// Ticks since the player appeared, with the partial tick.
    pub age: f32,
    /// How far the head is turned from the body (radians, positive
    /// turns toward east when facing north) and its pitch (positive
    /// looks down, as in vanilla).
    pub head_yaw: f32,
    pub head_pitch: f32,
    pub crouching: bool,
    /// How far into the swimming (or crawling) pose, 0–1.
    pub swim_amount: f32,
    /// Swimming in water follows the look pitch; crawling lies flat.
    pub in_water: bool,
    /// Ticks spent gliding with elytra, 0 when not.
    pub fall_flying: f32,
    /// Attack swing progress, 0–1 (0 when not swinging).
    pub attack: f32,
    /// Whether the left arm does the swinging.
    pub swing_left: bool,
    pub right: ArmPose,
    pub left: ArmPose,
    /// Ticks the current item has been in use.
    pub use_time: f32,
    /// Sitting in a boat, a cart or on a mob.
    pub riding: bool,
}

/// One limb's pose: its pivot in model pixels and its rotation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LimbPose {
    pub pos: Vec3,
    /// Rotation about x, y and z in radians.
    pub rot: Vec3,
}

impl LimbPose {
    /// Vanilla's `ModelPart.translateAndRotate`, in blocks: move to the
    /// pivot, then turn about z, y and x (x first).
    pub fn matrix(&self) -> Mat4 {
        Mat4::from_translation(self.pos / PIXELS)
            * Mat4::from_quat(Quat::from_euler(
                glam::EulerRot::ZYX,
                self.rot.z,
                self.rot.y,
                self.rot.x,
            ))
    }
}

/// All six limbs, indexed by `Limb`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rig(pub [LimbPose; 6]);

impl Rig {
    pub fn get(&self, limb: Limb) -> &LimbPose {
        &self.0[limb.index()]
    }
}

/// Vanilla's `quadraticArmUpdate`, the swimming stroke's easing.
fn quadratic_arm_update(f: f32) -> f32 {
    -65.0 * f + f * f
}

fn lerp(t: f32, a: f32, b: f32) -> f32 {
    a + (b - a) * t
}

/// Poses the limbs (`HumanoidModel.setupAnim` and `PlayerModel`).
pub fn rig(pose: &PlayerPose, slim: bool) -> Rig {
    let swim = pose.swim_amount;
    let mut pos = Limb::ALL.map(|limb| limb.pivot(slim));
    let mut rot = [Vec3::ZERO; 6];
    let i = |limb: Limb| limb.index();
    let (head, body, ra, la, rl, ll) = (
        i(Limb::Head),
        i(Limb::Body),
        i(Limb::RightArm),
        i(Limb::LeftArm),
        i(Limb::RightLeg),
        i(Limb::LeftLeg),
    );

    rot[head].y = pose.head_yaw;
    rot[head].x = if pose.fall_flying > 4.0 {
        -FRAC_PI_4
    } else if swim > 0.0 {
        lerp(swim, pose.head_pitch, -FRAC_PI_4)
    } else {
        pose.head_pitch
    };

    // Arms and legs swing against each other.
    let stride = pose.limb_swing * 0.6662;
    let amount = pose.limb_amount;
    rot[ra].x = (stride + PI).cos() * 2.0 * amount * 0.5;
    rot[la].x = stride.cos() * 2.0 * amount * 0.5;
    rot[rl].x = stride.cos() * 1.4 * amount;
    rot[ll].x = (stride + PI).cos() * 1.4 * amount;
    rot[rl].y = 0.005;
    rot[ll].y = -0.005;
    rot[rl].z = 0.005;
    rot[ll].z = -0.005;

    // Sitting: legs out in front, a little apart, arms forward (modern
    // `HumanoidModel.setupAnim` when riding).
    if pose.riding {
        rot[ra].x -= std::f32::consts::PI / 5.0;
        rot[la].x -= std::f32::consts::PI / 5.0;
        rot[rl] = Vec3::new(-1.413_716_7, std::f32::consts::PI / 10.0, 0.078_539_82);
        rot[ll] = Vec3::new(-1.413_716_7, -std::f32::consts::PI / 10.0, -0.078_539_82);
    }
    pose_arm(pose, pose.right, true, &mut rot, head);
    pose_arm(pose, pose.left, false, &mut rot, head);
    attack(pose, &mut pos, &mut rot);

    if pose.crouching {
        rot[body].x = 0.5;
        rot[ra].x += 0.4;
        rot[la].x += 0.4;
        pos[rl].z = 4.0;
        pos[ll].z = 4.0;
        pos[rl].y = 12.2;
        pos[ll].y = 12.2;
        pos[head].y = 4.2;
        pos[body].y = 3.2;
        let arm_y = if slim { 5.7 } else { 5.2 };
        pos[ra].y = arm_y;
        pos[la].y = arm_y;
    }

    // The arms sway a little even when still.
    for (arm, sign) in [(ra, 1.0), (la, -1.0)] {
        rot[arm].z += sign * ((pose.age * 0.09).cos() * 0.05 + 0.05);
        rot[arm].x += sign * (pose.age * 0.067).sin() * 0.05;
    }

    if swim > 0.0 {
        swimming(pose, &mut rot);
    }
    Rig(std::array::from_fn(|n| LimbPose {
        pos: pos[n],
        rot: rot[n],
    }))
}

/// `poseRightArm` / `poseLeftArm`.
fn pose_arm(pose: &PlayerPose, arm: ArmPose, right: bool, rot: &mut [Vec3; 6], head: usize) {
    let (this, other) = if right {
        (Limb::RightArm.index(), Limb::LeftArm.index())
    } else {
        (Limb::LeftArm.index(), Limb::RightArm.index())
    };
    let side = if right { 1.0 } else { -1.0 };
    match arm {
        ArmPose::Empty => rot[this].y = 0.0,
        ArmPose::Item => {
            rot[this].x = rot[this].x * 0.5 - 0.314_159_27;
            rot[this].y = 0.0;
        }
        ArmPose::Block => {
            rot[this].x = rot[this].x * 0.5 - 0.942_477_9;
            rot[this].y = -side * PI / 6.0;
        }
        ArmPose::Bow => {
            // Both arms point where the head looks; the one holding the
            // string comes in toward the face.
            let (right_y, left_y) = if right {
                (-0.1, 0.1 + 0.4)
            } else {
                (-0.1 - 0.4, 0.1)
            };
            rot[Limb::RightArm.index()].y = right_y + rot[head].y;
            rot[Limb::LeftArm.index()].y = left_y + rot[head].y;
            rot[this].x = -FRAC_PI_2 + rot[head].x;
            rot[other].x = -FRAC_PI_2 + rot[head].x;
        }
        ArmPose::Eat => {
            // Up to the mouth, bobbing with each bite.
            let bob = (pose.use_time * FRAC_PI_4).cos().abs() * 0.1;
            rot[this].x = -1.15 + bob;
            rot[this].y = -side * 0.35;
        }
    }
}

/// `setupAttackAnimation`: the body twists and the swinging arm chops.
fn attack(pose: &PlayerPose, pos: &mut [Vec3; 6], rot: &mut [Vec3; 6]) {
    let t = pose.attack;
    if t <= 0.0 {
        return;
    }
    let (body, ra, la, head) = (
        Limb::Body.index(),
        Limb::RightArm.index(),
        Limb::LeftArm.index(),
        Limb::Head.index(),
    );
    let mut twist = (t.sqrt() * 2.0 * PI).sin() * 0.2;
    if pose.swing_left {
        twist = -twist;
    }
    rot[body].y = twist;
    pos[ra].z = twist.sin() * 5.0;
    pos[ra].x = -twist.cos() * 5.0;
    pos[la].z = -twist.sin() * 5.0;
    pos[la].x = twist.cos() * 5.0;
    rot[ra].y += twist;
    rot[la].y += twist;
    rot[la].x += twist;
    let mut ease = 1.0 - t;
    ease *= ease;
    ease *= ease;
    let ease = 1.0 - ease;
    let lift = (ease * PI).sin();
    let lean = (t * PI).sin() * -(rot[head].x - 0.7) * 0.75;
    let arm = if pose.swing_left { la } else { ra };
    rot[arm].x -= lift * 1.2 + lean;
    rot[arm].y += twist * 2.0;
    rot[arm].z += (t * PI).sin() * -0.4;
}

/// The arm stroke and leg flutter of swimming (and crawling).
fn swimming(pose: &PlayerPose, rot: &mut [Vec3; 6]) {
    let swim = pose.swim_amount;
    let (ra, la, rl, ll) = (
        Limb::RightArm.index(),
        Limb::LeftArm.index(),
        Limb::RightLeg.index(),
        Limb::LeftLeg.index(),
    );
    let f = pose.limb_swing % 26.0;
    let (lx, ly, lz, rx, ry, rz);
    if f < 14.0 {
        let arc = quadratic_arm_update(f) / quadratic_arm_update(14.0);
        (lx, ly, lz) = (0.0, PI, PI + 1.870_796_4 * arc);
        (rx, ry, rz) = (0.0, PI, PI - 1.870_796_4 * arc);
    } else if f < 22.0 {
        let h = (f - 14.0) / 8.0;
        (lx, ly, lz) = (FRAC_PI_2 * h, PI, 5.012_389 - 1.870_796_4 * h);
        (rx, ry, rz) = (FRAC_PI_2 * h, PI, 1.270_796_3 + 1.870_796_4 * h);
    } else {
        let h = (f - 22.0) / 4.0;
        (lx, ly, lz) = (FRAC_PI_2 - FRAC_PI_2 * h, PI, PI);
        (rx, ry, rz) = (FRAC_PI_2 - FRAC_PI_2 * h, PI, PI);
    }
    rot[la] = Vec3::new(
        lerp(swim, rot[la].x, lx),
        lerp(swim, rot[la].y, ly),
        lerp(swim, rot[la].z, lz),
    );
    rot[ra] = Vec3::new(
        lerp(swim, rot[ra].x, rx),
        lerp(swim, rot[ra].y, ry),
        lerp(swim, rot[ra].z, rz),
    );
    rot[ll].x = lerp(
        swim,
        rot[ll].x,
        0.3 * (pose.limb_swing * 0.333_333_34 + PI).cos(),
    );
    rot[rl].x = lerp(
        swim,
        rot[rl].x,
        0.3 * (pose.limb_swing * 0.333_333_34).cos(),
    );
}

/// How the whole body is turned and moved: facing, then tipped over for
/// swimming, crawling and gliding (`PlayerRenderer.setupRotations`).
pub fn body_frame(pose: &PlayerPose, yaw: f32) -> Mat4 {
    let mut frame = Mat4::from_rotation_y(-yaw);
    if pose.fall_flying > 0.0 {
        let t = pose.fall_flying;
        let tip = (t * t / 100.0).clamp(0.0, 1.0);
        frame *= Mat4::from_rotation_x((tip * (-90.0 - pose.head_pitch.to_degrees())).to_radians());
    } else if pose.swim_amount > 0.0 {
        let target = if pose.in_water {
            -90.0 - pose.head_pitch.to_degrees()
        } else {
            -90.0
        };
        frame *= Mat4::from_rotation_x(lerp(pose.swim_amount, 0.0, target).to_radians());
        frame *= Mat4::from_translation(Vec3::new(0.0, -1.0, 0.3) * pose.swim_amount);
    }
    frame
}

/// Maps a point in model pixels (y down, x mirrored) to the world.
pub fn model_to_world(frame: &Mat4, feet: Vec3, p: Vec3) -> Vec3 {
    let local = Vec3::new(-p.x, FEET_Y - p.y, p.z) / PIXELS * PLAYER_SCALE;
    feet + frame.transform_point3(local)
}

/// The matrix taking points in model blocks (y down, as `LimbPose`
/// matrices leave them) to the world; `model_to_world` for matrices.
pub fn model_matrix(frame: &Mat4, feet: Vec3) -> Mat4 {
    Mat4::from_translation(feet)
        * *frame
        * Mat4::from_scale(Vec3::splat(PLAYER_SCALE))
        * Mat4::from_translation(Vec3::new(0.0, FEET_Y / PIXELS, 0.0))
        * Mat4::from_scale(Vec3::new(-1.0, -1.0, 1.0))
}

/// Adds the player's model at `feet`, facing `yaw`, lit by (sky, block)
/// `light` and multiplied by `tint`. `hat`, `jacket` and the other outer
/// layers are always drawn.
pub fn add_player(
    mesh: &mut Mesh,
    pose: &PlayerPose,
    slim: bool,
    feet: Vec3,
    yaw: f32,
    tint: [f32; 3],
    light: (u8, u8),
) {
    let rig = rig(pose, slim);
    let frame = body_frame(pose, yaw);
    let light = [light.0 as f32, light.1 as f32];
    for limb in Limb::ALL {
        let limb_pose = rig.get(limb);
        let rotation = Mat3::from_quat(Quat::from_euler(
            glam::EulerRot::ZYX,
            limb_pose.rot.z,
            limb_pose.rot.y,
            limb_pose.rot.x,
        ));
        for (b, grow) in limb.boxes(slim) {
            add_box(
                mesh,
                SKIN_SIZE,
                &b,
                BoxStyle {
                    mirror: false,
                    grow,
                },
                |corner| {
                    let at = limb_pose.pos + rotation * corner;
                    model_to_world(&frame, feet, at).to_array()
                },
                tint,
                light,
            );
        }
    }
}

/// A worn piece of armor as drawn: where it's worn, its row in the armor
/// sheet (`look::armor_row`), its tint (leather's dye) and whether it glints.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WornArmor {
    pub slot: crate::inventory::armor::ArmorSlot,
    pub row: usize,
    pub tint: [f32; 3],
    pub glint: bool,
}

/// The armor layers' growth over the limbs, in pixels (vanilla's
/// `OUTER_ARMOR_DEFORMATION` and `INNER_ARMOR_DEFORMATION`).
const OUTER_ARMOR_GROW: f32 = 1.0;
const INNER_ARMOR_GROW: f32 = 0.5;

/// Adds `worn` armor over the model in `pose` (modern
/// `HumanoidArmorLayer`): the helmet on the head and hat, the chestplate on
/// the body and arms, the boots on the legs, all from a 64 × 32 outer
/// layer, and the leggings on the body and legs from the inner layer. The
/// armor sheet has `rows` such layers stacked.
#[allow(clippy::too_many_arguments)]
pub fn add_armor(
    mesh: &mut Mesh,
    pose: &PlayerPose,
    slim: bool,
    feet: Vec3,
    yaw: f32,
    worn: &[WornArmor],
    rows: usize,
    light: (u8, u8),
) {
    use crate::inventory::armor::ArmorSlot;
    let rig = rig(pose, slim);
    let frame = body_frame(pose, yaw);
    let size = (64.0, 32.0 * rows as f32);
    for piece in worn {
        let v = 32.0 * piece.row as f32;
        let b = |uv: (f32, f32), from: (f32, f32, f32), size: (f32, f32, f32)| PartBox {
            uv: (uv.0, uv.1 + v),
            from,
            size,
        };
        let limb_box = |limb: Limb| match limb {
            Limb::Head => b((0.0, 0.0), (-4.0, -8.0, -4.0), (8.0, 8.0, 8.0)),
            Limb::Body => b((16.0, 16.0), (-4.0, 0.0, -2.0), (8.0, 12.0, 4.0)),
            Limb::RightArm => b((40.0, 16.0), (-3.0, -2.0, -2.0), (4.0, 12.0, 4.0)),
            Limb::LeftArm => b((40.0, 16.0), (-1.0, -2.0, -2.0), (4.0, 12.0, 4.0)),
            Limb::RightLeg | Limb::LeftLeg => b((0.0, 16.0), (-2.0, 0.0, -2.0), (4.0, 12.0, 4.0)),
        };
        let grow = if piece.slot == ArmorSlot::Legs {
            INNER_ARMOR_GROW
        } else {
            OUTER_ARMOR_GROW
        };
        let mut parts: Vec<(Limb, PartBox, f32)> = match piece.slot {
            ArmorSlot::Head => vec![(Limb::Head, limb_box(Limb::Head), grow)],
            ArmorSlot::Chest => [Limb::Body, Limb::RightArm, Limb::LeftArm]
                .map(|limb| (limb, limb_box(limb), grow))
                .to_vec(),
            ArmorSlot::Legs => [Limb::Body, Limb::RightLeg, Limb::LeftLeg]
                .map(|limb| (limb, limb_box(limb), grow))
                .to_vec(),
            ArmorSlot::Feet => [Limb::RightLeg, Limb::LeftLeg]
                .map(|limb| (limb, limb_box(limb), grow))
                .to_vec(),
        };
        if piece.slot == ArmorSlot::Head {
            // The helmet's hat part, grown half a pixel more.
            let hat = b((32.0, 0.0), (-4.0, -8.0, -4.0), (8.0, 8.0, 8.0));
            parts.push((Limb::Head, hat, grow + HAT_GROW));
        }
        let light = super::hand::glinted(light, piece.glint);
        let light = [light.0 as f32, light.1 as f32];
        for (limb, part, grow) in parts {
            let limb_pose = rig.get(limb);
            let rotation = Mat3::from_quat(Quat::from_euler(
                glam::EulerRot::ZYX,
                limb_pose.rot.z,
                limb_pose.rot.y,
                limb_pose.rot.x,
            ));
            add_box(
                mesh,
                size,
                &part,
                BoxStyle {
                    mirror: matches!(limb, Limb::LeftArm | Limb::LeftLeg),
                    grow,
                },
                |corner| {
                    let at = limb_pose.pos + rotation * corner;
                    model_to_world(&frame, feet, at).to_array()
                },
                piece.tint,
                light,
            );
        }
    }
}

/// Adds the elytra on the player's back, folded at rest and spread out
/// while gliding (modern `ElytraModel`; 64 × 32 skin), turned and tipped
/// with the body.
pub fn add_elytra(mesh: &mut Mesh, pose: &PlayerPose, feet: Vec3, yaw: f32, light: (u8, u8)) {
    let frame = body_frame(pose, yaw);
    let light = [light.0 as f32, light.1 as f32];
    let (mut x_rot, mut z_rot, mut lift) = (0.261_799_4_f32, -0.261_799_4_f32, 0.0_f32);
    if pose.fall_flying > 0.0 {
        // Spreads over the first ticks of a glide.
        let spread = (pose.fall_flying / 6.0).clamp(0.0, 1.0);
        x_rot += spread * (0.349_065_84 - x_rot);
        z_rot += spread * (-FRAC_PI_2 - z_rot);
    } else if pose.crouching {
        x_rot = 0.698_131_7;
        z_rot = -FRAC_PI_4;
        lift = 3.0;
    }
    for side in [1.0_f32, -1.0] {
        // The left wing is at +x, mirrored, and spreads the other way.
        let pivot = Vec3::new(5.0 * side, lift, 2.0);
        let m = Mat4::from_translation(pivot)
            * Mat4::from_rotation_z(z_rot * side)
            * Mat4::from_rotation_x(x_rot);
        let wing = PartBox {
            uv: (22.0, 0.0),
            from: if side > 0.0 {
                (-10.0, 0.0, 0.0)
            } else {
                (0.0, 0.0, 0.0)
            },
            size: (10.0, 20.0, 2.0),
        };
        add_box(
            mesh,
            ELYTRA_SIZE,
            &wing,
            BoxStyle {
                mirror: side < 0.0,
                grow: 1.0,
            },
            |corner| model_to_world(&frame, feet, m.transform_point3(corner)).to_array(),
            [1.0; 3],
            light,
        );
    }
}

/// The elytra's skin is 64 × 32.
const ELYTRA_SIZE: (f32, f32) = (64.0, 32.0);

/// The matrix, in model blocks, to a held item's origin in third person:
/// the hand's end, turned and moved as vanilla's `ItemInHandLayer` does.
pub fn hand_mount(rig: &Rig, right: bool, slim: bool) -> Mat4 {
    let limb = if right { Limb::RightArm } else { Limb::LeftArm };
    let mut arm = *rig.get(limb);
    if slim {
        arm.pos.x += if right { 0.5 } else { -0.5 };
    }
    arm.matrix()
        * Mat4::from_rotation_x(-FRAC_PI_2)
        * Mat4::from_rotation_y(PI)
        * Mat4::from_translation(Vec3::new(
            if right { 1.0 } else { -1.0 } / 16.0,
            0.125,
            -0.625,
        ))
}

/// The old 64×32 skin layout made over in the modern 64×64 one, and
/// every skin's body made opaque, as vanilla's `HttpTexture` does.
pub fn prepare_skin(image: RgbaImage) -> RgbaImage {
    let legacy = image.height() == 32;
    let mut skin = RgbaImage::new(64, 64);
    imageops::replace(&mut skin, &image, 0, 0);
    if legacy {
        // The left leg and arm are the right ones turned around.
        for (x, y, dx, dy, w, h) in [
            (4, 16, 16, 32, 4, 4),
            (8, 16, 16, 32, 4, 4),
            (0, 20, 24, 32, 4, 12),
            (4, 20, 16, 32, 4, 12),
            (8, 20, 8, 32, 4, 12),
            (12, 20, 16, 32, 4, 12),
            (44, 16, -8, 32, 4, 4),
            (48, 16, -8, 32, 4, 4),
            (40, 20, 0, 32, 4, 12),
            (44, 20, -8, 32, 4, 12),
            (48, 20, -16, 32, 4, 12),
            (52, 20, -8, 32, 4, 12),
        ] {
            for j in 0..h {
                for i in 0..w {
                    let px = *skin.get_pixel(x + i, y + j);
                    let to_x = (x as i32 + dx + (w as i32 - 1 - i as i32)) as u32;
                    skin.put_pixel(to_x, y + dy + j, px);
                }
            }
        }
        // A legacy skin's hat has no transparency of its own: if it has
        // none, it isn't a hat, so it's cleared.
        let hat = (32..64).flat_map(|x| (0..32).map(move |y| (x, y)));
        if hat.clone().all(|(x, y)| skin.get_pixel(x, y).0[3] >= 128) {
            for (x, y) in hat {
                skin.put_pixel(x, y, Rgba([0, 0, 0, 0]));
            }
        }
    }
    // The base layers are never see-through.
    for (x0, y0, x1, y1) in [(0, 0, 32, 16), (0, 16, 64, 32), (16, 48, 48, 64)] {
        for y in y0..y1 {
            for x in x0..x1 {
                skin.get_pixel_mut(x, y).0[3] = 255;
            }
        }
    }
    if legacy {
        // The rest of the old sheet's left half is body too.
        for y in 48..64 {
            for x in 16..48 {
                skin.get_pixel_mut(x, y).0[3] = 255;
            }
        }
    }
    skin
}

#[cfg(test)]
mod tests {
    use super::*;

    fn standing() -> PlayerPose {
        PlayerPose::default()
    }

    fn bounds(mesh: &Mesh) -> (Vec3, Vec3) {
        let points = mesh.vertices.iter().map(|v| Vec3::from(v.position));
        let min = points.clone().fold(Vec3::splat(f32::MAX), Vec3::min);
        let max = points.fold(Vec3::splat(f32::MIN), Vec3::max);
        (min, max)
    }

    fn model(pose: &PlayerPose, yaw: f32) -> Mesh {
        let mut mesh = Mesh::default();
        add_player(&mut mesh, pose, false, Vec3::ZERO, yaw, [1.0; 3], (15, 0));
        mesh
    }

    #[test]
    fn the_model_is_twelve_boxes_standing_1_875_blocks_tall() {
        let mesh = model(&standing(), 0.0);
        // Six limbs with an outer layer each, six faces a box.
        assert_eq!(mesh.indices.len() / 6, 12 * 6);
        let (min, max) = bounds(&mesh);
        // The pants layer reaches a quarter pixel below the feet.
        assert!((min.y + 0.25 / 16.0 * PLAYER_SCALE).abs() < 1e-3, "{min}");
        // The hat layer grows the head by half a pixel.
        assert!(
            (max.y - (2.0 + 0.5 / 16.0) * PLAYER_SCALE).abs() < 1e-3,
            "{max}"
        );
        assert!((1.875..1.93).contains(&max.y));
        // Shoulder to shoulder: 16 pixels of arms and body, swaying a little.
        assert!((0.9..1.1).contains(&(max.x - min.x)), "{min} {max}");
    }

    #[test]
    fn slim_arms_are_a_pixel_narrower() {
        let wide = {
            let mut m = Mesh::default();
            add_player(
                &mut m,
                &standing(),
                false,
                Vec3::ZERO,
                0.0,
                [1.0; 3],
                (15, 0),
            );
            bounds(&m)
        };
        let slim = {
            let mut m = Mesh::default();
            add_player(
                &mut m,
                &standing(),
                true,
                Vec3::ZERO,
                0.0,
                [1.0; 3],
                (15, 0),
            );
            bounds(&m)
        };
        let width = |b: (Vec3, Vec3)| b.1.x - b.0.x;
        assert!((width(wide) - width(slim) - 2.0 / 16.0 * PLAYER_SCALE).abs() < 1e-3);
    }

    #[test]
    fn the_player_faces_its_yaw() {
        // The face is the head's north side at yaw 0, and turns east with yaw.
        let face = |yaw: f32| {
            let mesh = model(&standing(), yaw);
            let head: Vec<Vec3> = mesh
                .vertices
                .iter()
                .map(|v| Vec3::from(v.position))
                .filter(|p| p.y > 1.5)
                .collect();
            let n = head.len() as f32;
            let mid = head.iter().copied().sum::<Vec3>() / n;
            (mid, bounds(&mesh))
        };
        let (_, (min, max)) = face(0.0);
        assert!(min.z < -0.2 && max.z < 0.5);
        let (_, (min, max)) = face(FRAC_PI_2);
        assert!(max.x > 0.2 && min.x > -0.5, "{min} {max}");
    }

    #[test]
    fn walking_swings_arms_against_legs_and_still_standing_hangs_straight() {
        let still = rig(&standing(), false);
        for limb in [Limb::RightLeg, Limb::LeftLeg] {
            assert!(still.get(limb).rot.x.abs() < 1e-6);
        }
        let walking = PlayerPose {
            limb_swing: 1.0,
            limb_amount: 1.0,
            ..standing()
        };
        let r = rig(&walking, false);
        let x = |limb| r.get(limb).rot.x;
        // Opposite limbs swing opposite ways, and arms against legs.
        assert!(x(Limb::RightLeg) * x(Limb::LeftLeg) < 0.0);
        assert!(x(Limb::RightArm) * x(Limb::LeftArm) < 0.0);
        assert!(x(Limb::RightArm) * x(Limb::RightLeg) < 0.0);
        // Legs swing 1.4, arms 1.0, as vanilla.
        let stride = 0.6662f32;
        assert!((x(Limb::RightLeg) - 1.4 * stride.cos()).abs() < 1e-5);
    }

    #[test]
    fn sneaking_leans_the_body_and_lowers_the_head() {
        let r = rig(
            &PlayerPose {
                crouching: true,
                ..standing()
            },
            false,
        );
        assert_eq!(r.get(Limb::Body).rot.x, 0.5);
        assert_eq!(r.get(Limb::Head).pos.y, 4.2);
        assert_eq!(r.get(Limb::RightLeg).pos.z, 4.0);
        let upright = rig(&standing(), false);
        assert_eq!(upright.get(Limb::Head).pos.y, 0.0);
        assert!(r.get(Limb::RightArm).rot.x > upright.get(Limb::RightArm).rot.x + 0.3);
    }

    #[test]
    fn swimming_lies_the_body_down_and_crawling_lies_it_flat() {
        let swimming = PlayerPose {
            swim_amount: 1.0,
            in_water: true,
            head_pitch: 0.0,
            ..standing()
        };
        let (min, max) = bounds(&model(&swimming, 0.0));
        // Lying down, stretched along the facing.
        assert!(max.y - min.y < 1.0, "{min} {max}");
        assert!(max.z - min.z > 1.5, "{min} {max}");
        // Facing north, the head leads: the body extends mostly north.
        let crawling = PlayerPose {
            in_water: false,
            ..swimming
        };
        let (cmin, cmax) = bounds(&model(&crawling, 0.0));
        assert!(cmax.y - cmin.y < 1.0 && cmax.z - cmin.z > 1.5);
        // Looking down in water points the swimmer down.
        let diving = PlayerPose {
            head_pitch: 0.8,
            ..swimming
        };
        let (dmin, dmax) = bounds(&model(&diving, 0.0));
        assert!(dmax.y - dmin.y > max.y - min.y + 0.3);
        // The stroke cycles through its three phases.
        let arms: Vec<f32> = [0.0, 10.0, 18.0, 24.0]
            .into_iter()
            .map(|swing| {
                rig(
                    &PlayerPose {
                        limb_swing: swing,
                        ..swimming
                    },
                    false,
                )
                .get(Limb::LeftArm)
                .rot
                .z
            })
            .collect();
        assert!(
            arms.windows(2).any(|w| (w[0] - w[1]).abs() > 0.1),
            "{arms:?}"
        );
    }

    #[test]
    fn the_elytra_hangs_folded_behind_the_back_and_spreads_wide_when_gliding() {
        let folded = PlayerPose::default();
        let mut mesh = Mesh::default();
        add_elytra(&mut mesh, &folded, Vec3::ZERO, 0.0, (15, 0));
        let (lo, hi) = bounds(&mesh);
        assert!(
            mesh.vertices
                .iter()
                .all(|v| v.position.iter().all(|c| c.is_finite()))
        );
        // Facing north the back is south (+z); the wings are behind the body.
        assert!(hi.z > 0.1 && lo.z > -0.2, "{lo} {hi}");
        let narrow = hi.x - lo.x;
        let gliding = PlayerPose {
            fall_flying: 20.0,
            ..PlayerPose::default()
        };
        let mut open = Mesh::default();
        add_elytra(&mut open, &gliding, Vec3::ZERO, 0.0, (15, 0));
        let (lo, hi) = bounds(&open);
        assert!(hi.x - lo.x > narrow + 0.5, "{} vs {narrow}", hi.x - lo.x);
    }

    #[test]
    fn gliding_tips_the_body_forward() {
        let gliding = PlayerPose {
            fall_flying: 20.0,
            ..standing()
        };
        let (min, max) = bounds(&model(&gliding, 0.0));
        assert!(max.y - min.y < 1.0 && max.z - min.z > 1.5, "{min} {max}");
        assert_eq!(rig(&gliding, false).get(Limb::Head).rot.x, -FRAC_PI_4);
        // Just started gliding: still nearly upright.
        let start = PlayerPose {
            fall_flying: 1.0,
            ..standing()
        };
        let (min, max) = bounds(&model(&start, 0.0));
        assert!(max.y - min.y > 1.7);
    }

    #[test]
    fn swinging_chops_the_arm_and_twists_the_body() {
        let at = |attack: f32| {
            rig(
                &PlayerPose {
                    attack,
                    ..standing()
                },
                false,
            )
        };
        assert_eq!(at(0.0).get(Limb::Body).rot.y, 0.0);
        let mid = at(0.5);
        assert!(mid.get(Limb::Body).rot.y.abs() > 0.0);
        // The right arm lifts far forward (negative x) at the top of the swing.
        let top = at(0.3);
        assert!(
            top.get(Limb::RightArm).rot.x < -0.8,
            "{:?}",
            top.get(Limb::RightArm)
        );
        assert!(top.get(Limb::LeftArm).rot.x > top.get(Limb::RightArm).rot.x);
        let left = rig(
            &PlayerPose {
                attack: 0.3,
                swing_left: true,
                ..standing()
            },
            false,
        );
        assert!(left.get(Limb::LeftArm).rot.x < -0.8);
        assert!(left.get(Limb::Body).rot.y * top.get(Limb::Body).rot.y < 0.0);
    }

    #[test]
    fn held_items_raise_the_arm_and_a_bow_aims_both() {
        let r = rig(
            &PlayerPose {
                right: ArmPose::Item,
                ..standing()
            },
            false,
        );
        assert!(r.get(Limb::RightArm).rot.x < -0.25);
        let bow = rig(
            &PlayerPose {
                right: ArmPose::Bow,
                left: ArmPose::Bow,
                head_pitch: 0.2,
                ..standing()
            },
            false,
        );
        for limb in [Limb::RightArm, Limb::LeftArm] {
            assert!((bow.get(limb).rot.x - (-FRAC_PI_2 + 0.2)).abs() < 1e-5);
        }
        let eat = rig(
            &PlayerPose {
                right: ArmPose::Eat,
                ..standing()
            },
            false,
        );
        assert!(eat.get(Limb::RightArm).rot.x < -1.0);
    }

    #[test]
    fn the_hand_mount_sits_at_the_end_of_the_arm() {
        // Standing, an empty arm hangs: the mount is near the hand, below the shoulder.
        let r = rig(&standing(), false);
        let mount = hand_mount(&r, true, false);
        let at = mount.transform_point3(Vec3::ZERO);
        let frame = body_frame(&standing(), 0.0);
        let world = model_to_world(&frame, Vec3::ZERO, at * PIXELS);
        // On the player's right (east at yaw 0), around hip height.
        assert!(world.x > 0.2 && world.x < 0.5, "{world}");
        assert!(world.y > 0.6 && world.y < 1.4, "{world}");
    }

    #[test]
    fn looking_down_tips_the_head_forward_and_looking_up_tips_it_back() {
        // The top of the head swings north (forward at yaw 0) when it nods.
        let top_z = |pitch: f32| {
            let pose = PlayerPose {
                head_pitch: pitch,
                ..standing()
            };
            let mesh = model(&pose, 0.0);
            let top: Vec<f32> = mesh
                .vertices
                .iter()
                .map(|v| Vec3::from(v.position))
                .filter(|p| p.y > 1.8)
                .map(|p| p.z)
                .collect();
            top.iter().sum::<f32>() / top.len() as f32
        };
        let level = top_z(0.0);
        assert!(top_z(0.7) < level - 0.1, "looking down tips it north");
        assert!(top_z(-0.7) > level + 0.1, "looking up tips it back");
        // And the head turns toward east with positive yaw.
        let turned = model(
            &PlayerPose {
                head_yaw: 0.8,
                ..standing()
            },
            0.0,
        );
        let east_of_neck = turned
            .vertices
            .iter()
            .map(|v| Vec3::from(v.position))
            .filter(|p| p.y > 1.75)
            .map(|p| p.x)
            .fold(f32::MIN, f32::max);
        assert!(east_of_neck > 0.3, "{east_of_neck}");
    }

    #[test]
    fn the_model_matrix_agrees_with_the_point_mapping() {
        let pose = PlayerPose {
            swim_amount: 0.7,
            in_water: true,
            head_pitch: 0.4,
            ..standing()
        };
        let frame = body_frame(&pose, 1.1);
        let feet = Vec3::new(10.0, 64.0, -3.0);
        let matrix = model_matrix(&frame, feet);
        for p in [
            Vec3::ZERO,
            Vec3::new(3.0, 20.0, -5.0),
            Vec3::new(-8.0, 5.0, 9.0),
        ] {
            let a = matrix.transform_point3(p / PIXELS);
            let b = model_to_world(&frame, feet, p);
            assert!((a - b).length() < 1e-4, "{a} {b}");
        }
    }

    #[test]
    fn legacy_skins_get_a_left_leg_and_an_opaque_body() {
        let mut old = RgbaImage::from_pixel(64, 32, Rgba([10, 20, 30, 255]));
        // Right leg's top, left to right.
        for i in 0..4 {
            old.put_pixel(4 + i, 16, Rgba([i as u8 * 50, 0, 0, 255]));
        }
        let skin = prepare_skin(old);
        assert_eq!(skin.dimensions(), (64, 64));
        // The left leg's top is the right one's, mirrored.
        assert_eq!(skin.get_pixel(20, 48).0[0], 150);
        assert_eq!(skin.get_pixel(23, 48).0[0], 0);
        // The all-opaque hat region is cleared; the body stays opaque.
        assert_eq!(skin.get_pixel(40, 8).0[3], 0);
        assert_eq!(skin.get_pixel(8, 8).0[3], 255);

        // A modern skin keeps its transparent hat and sleeves.
        let mut modern = RgbaImage::from_pixel(64, 64, Rgba([1, 2, 3, 255]));
        modern.put_pixel(40, 8, Rgba([9, 9, 9, 0]));
        modern.put_pixel(8, 8, Rgba([9, 9, 9, 0]));
        let skin = prepare_skin(modern);
        assert_eq!(skin.get_pixel(40, 8).0[3], 0);
        assert_eq!(skin.get_pixel(8, 8).0[3], 255, "base layers are opaque");
    }

    #[test]
    fn kenzos_skins_prepare() {
        let dir = crate::assets::dir().join("skins");
        for name in ["steve-skin.png", "alex.png"] {
            let image = image::open(dir.join(name)).unwrap().into_rgba8();
            let skin = prepare_skin(image);
            assert_eq!(skin.dimensions(), (64, 64), "{name}");
            assert_eq!(skin.get_pixel(10, 10).0[3], 255, "{name}: face is opaque");
        }
    }

    #[test]
    fn armor_covers_its_limbs_a_little_larger_than_the_skin() {
        use crate::inventory::armor::ArmorSlot;
        let pose = PlayerPose::default();
        let piece = |slot, row| WornArmor {
            slot,
            row,
            tint: [1.0; 3],
            glint: false,
        };
        let mut body = Mesh::default();
        add_player(&mut body, &pose, false, Vec3::ZERO, 0.0, [1.0; 3], (15, 0));
        let mut armor = Mesh::default();
        let worn = [
            piece(ArmorSlot::Head, 6),
            piece(ArmorSlot::Chest, 6),
            piece(ArmorSlot::Legs, 7),
            piece(ArmorSlot::Feet, 6),
        ];
        add_armor(
            &mut armor,
            &pose,
            false,
            Vec3::ZERO,
            0.0,
            &worn,
            18,
            (15, 0),
        );
        // Head and hat, body and arms, body and legs, legs: 10 boxes.
        assert_eq!(armor.vertices.len(), 10 * 24);
        let top = |mesh: &Mesh| {
            mesh.vertices
                .iter()
                .map(|v| v.position[1])
                .fold(f32::MIN, f32::max)
        };
        assert!(top(&armor) > top(&body), "the helmet is over the hat");
        // Every corner's texture is in its own row of the sheet.
        for v in &armor.vertices {
            let rows = v.uv[1] * 18.0;
            assert!((6.0..=8.0).contains(&rows), "{:?}", v.uv);
        }
    }
}
