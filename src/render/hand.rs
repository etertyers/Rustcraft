//! The first-person hand, after modern Minecraft's `ItemInHandRenderer`:
//! the bare right arm, or the item in it, swung, dipped when the item
//! changes, lagging behind the mouse, shaken by walking, raised to the
//! mouth to eat and drawn back to shoot. Meshes are in camera space (x
//! right, y up, −z forward) and drawn with their own 70° projection.

use std::f32::consts::PI;

use glam::{Mat4, Vec3};

use super::{
    camera::ViewBob,
    held::{HeldGeometry, display_matrix},
    models::{BoxStyle, add_box},
    player::{Limb, PlayerPose, SKIN_SIZE, rig},
};
use crate::{game::view::HandUse, pack::model::Transform, world::mesh::Mesh};

/// The hand is drawn with this field of view whatever the setting.
pub const HAND_FOV_DEGREES: f32 = 70.0;

/// The item in the hand: its geometry and the model's
/// `firstperson_righthand` display transform.
#[derive(Debug, Clone, Copy)]
pub struct HandItem<'a> {
    pub geometry: &'a HeldGeometry,
    pub display: Transform,
    /// Enchanted: the shader makes it shimmer.
    pub glint: bool,
}

/// Everything the hand's pose depends on.
#[derive(Debug, Clone, Copy)]
pub struct HandInput<'a> {
    /// The left hand (the off-hand) instead of the right: everything
    /// is mirrored.
    pub left: bool,
    /// Swing progress, 0–1.
    pub swing: f32,
    /// How far the hand is lowered, 0 when raised.
    pub equip: f32,
    /// How far the hand trails the mouse, in degrees: (pitch, yaw).
    pub lag: (f32, f32),
    pub bob: ViewBob,
    pub using: HandUse,
    pub item: Option<HandItem<'a>>,
    pub slim: bool,
    /// (sky, block) light on the hand.
    pub light: (u8, u8),
}

/// The hand's meshes: the arm (textured with the skin) and the item
/// (textured with the atlas).
#[derive(Debug, Default)]
pub struct HandMeshes {
    pub arm: Mesh,
    pub item: Mesh,
}

fn rot_x(degrees: f32) -> Mat4 {
    Mat4::from_rotation_x(degrees.to_radians())
}
fn rot_y(degrees: f32) -> Mat4 {
    Mat4::from_rotation_y(degrees.to_radians())
}
fn rot_z(degrees: f32) -> Mat4 {
    Mat4::from_rotation_z(degrees.to_radians())
}
fn shift(x: f32, y: f32, z: f32) -> Mat4 {
    Mat4::from_translation(Vec3::new(x, y, z))
}

/// `applyItemArmTransform`: the item's resting place at the lower right.
fn item_arm_transform(equip: f32, i: f32) -> Mat4 {
    shift(i * 0.56, -0.52 + equip * -0.6, -0.72)
}

/// `applyItemArmAttackTransform`: the chop of a swing.
fn item_attack_transform(swing: f32, i: f32) -> Mat4 {
    let f = (swing * swing * PI).sin();
    let g = (swing.sqrt() * PI).sin();
    rot_y(i * (45.0 + f * -20.0)) * rot_z(i * g * -20.0) * rot_x(g * -80.0) * rot_y(i * -45.0)
}

/// `applyEatTransform`: the food comes up to the mouth and bobs.
fn eat_transform(remaining: f32, total: f32, side: f32) -> Mat4 {
    let f = remaining + 1.0;
    let g = f / total;
    let bob = if g < 0.8 {
        (f / 4.0 * PI).cos().abs() * 0.1
    } else {
        0.0
    };
    let i = 1.0 - g.max(0.0).powi(27);
    shift(0.0, bob, 0.0)
        * shift(side * i * 0.6, i * -0.5, 0.0)
        * rot_y(side * i * 90.0)
        * rot_x(i * 10.0)
        * rot_z(side * i * 30.0)
}

/// The bow pulling back as it's drawn.
fn bow_transform(drawn: f32, i: f32) -> Mat4 {
    let mut m = shift(i * -0.278_568_2, 0.183_443_87, 0.157_315_31)
        * rot_x(-13.935)
        * rot_y(i * 35.3)
        * rot_z(i * -9.785);
    let g = drawn / 20.0;
    let g = ((g * g + g * 2.0) / 3.0).min(1.0);
    if g > 0.1 {
        let h = ((drawn - 0.1) * 1.3).sin();
        let k = h * (g - 0.1);
        m *= shift(0.0, k * 0.004, 0.0);
    }
    m * shift(0.0, 0.0, g * 0.04)
        * Mat4::from_scale(Vec3::new(1.0, 1.0, 1.0 + g * 0.2))
        * rot_y(45.0)
}

/// The pose of the hand for `input`, as a transform of camera space.
pub fn hand_pose(input: &HandInput) -> Mat4 {
    let swing = input.swing;
    let i = if input.left { -1.0 } else { 1.0 };
    let root = input.bob.matrix() * rot_x(-input.lag.0) * rot_y(input.lag.1);
    if input.item.is_none() {
        // The bare arm (`renderPlayerArm`).
        let g = swing.sqrt();
        let h = -0.3 * (g * PI).sin();
        let k = 0.4 * (g * 2.0 * PI).sin();
        let j = -0.4 * (swing * PI).sin();
        let m = (swing * swing * PI).sin();
        let l = (g * PI).sin();
        return root
            * shift(
                i * (h + 0.640_000_05),
                k - 0.6 + input.equip * -0.6,
                j - 0.719_999_97,
            )
            * rot_y(i * 45.0)
            * rot_y(i * l * 70.0)
            * rot_z(i * m * -20.0)
            * shift(-i, 3.6, 3.5)
            * rot_z(i * 120.0)
            * rot_x(200.0)
            * rot_y(i * -135.0)
            * shift(i * 5.6, 0.0, 0.0);
    }
    match input.using {
        HandUse::Eat { remaining, total } => {
            root * eat_transform(remaining, total, i) * item_arm_transform(input.equip, i)
        }
        HandUse::Bow { drawn, .. } => {
            root * item_arm_transform(input.equip, i) * bow_transform(drawn, i)
        }
        HandUse::None => {
            let f = -0.4 * (swing.sqrt() * PI).sin();
            let g = 0.2 * (swing.sqrt() * PI * 2.0).sin();
            let h = -0.2 * (swing * PI).sin();
            root * shift(i * f, g, h)
                * item_arm_transform(input.equip, i)
                * item_attack_transform(swing, i)
        }
    }
}

/// `light` with the shader's flag for enchanted items added when `on`.
pub fn glinted(light: (u8, u8), on: bool) -> (u8, u8) {
    if on {
        (light.0 + super::GLINT_FLAG, light.1)
    } else {
        light
    }
}

/// Builds the hand for one frame.
pub fn build(input: &HandInput) -> HandMeshes {
    let pose = hand_pose(input);
    let mut meshes = HandMeshes::default();
    match input.item {
        Some(item) => {
            let matrix = pose * display_matrix(&item.display, input.left);
            item.geometry.add_to(
                &mut meshes.item,
                &matrix,
                [1.0; 3],
                glinted(input.light, item.glint),
            );
        }
        None => add_arm(&mut meshes.arm, &pose, input),
    }
    meshes
}

/// The right arm and its sleeve in the hand's pose. The arm is built
/// standing still with no swing, as vanilla's `renderHand` does.
fn add_arm(mesh: &mut Mesh, pose: &Mat4, input: &HandInput) {
    let which = if input.left {
        Limb::LeftArm
    } else {
        Limb::RightArm
    };
    let mut limb = *rig(&PlayerPose::default(), input.slim).get(which);
    limb.rot.x = 0.0;
    let arm = limb.matrix();
    let light = [input.light.0 as f32, input.light.1 as f32];
    for (b, grow) in which.boxes(input.slim) {
        add_box(
            mesh,
            SKIN_SIZE,
            &b,
            BoxStyle {
                mirror: false,
                grow,
            },
            |corner| {
                // Model pixels in blocks, y down as the model has it.
                let in_model = arm.transform_point3(corner / 16.0);
                pose.transform_point3(in_model).to_array()
            },
            [1.0; 3],
            light,
        );
    }
}

/// The projection the hand is drawn with.
pub fn hand_projection(aspect: f32) -> Mat4 {
    glam::camera::rh::proj::directx::perspective(
        HAND_FOV_DEGREES.to_radians(),
        aspect,
        crate::config::Z_NEAR,
        crate::config::Z_FAR,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::{held::HeldGeometry, icons::IconFace};

    fn input<'a>(item: Option<HandItem<'a>>) -> HandInput<'a> {
        HandInput {
            left: false,
            swing: 0.0,
            equip: 0.0,
            lag: (0.0, 0.0),
            bob: ViewBob::NONE,
            using: HandUse::None,
            item,
            slim: false,
            light: (15, 0),
        }
    }

    fn sword() -> HeldGeometry {
        HeldGeometry::cube(
            IconFace {
                uv: [0.0; 4],
                color: [1.0; 3],
            },
            IconFace {
                uv: [0.0; 4],
                color: [1.0; 3],
            },
            IconFace {
                uv: [0.0; 4],
                color: [1.0; 3],
            },
        )
    }

    fn display() -> Transform {
        // Vanilla's `item/handheld` firstperson_righthand.
        Transform {
            rotation: [0.0, -90.0, 25.0],
            translation: [1.13, 3.2, 1.13],
            scale: [0.68; 3],
        }
    }

    fn bounds(mesh: &Mesh) -> (Vec3, Vec3) {
        let points = mesh.vertices.iter().map(|v| Vec3::from(v.position));
        let min = points.clone().fold(Vec3::splat(f32::MAX), Vec3::min);
        let max = points.fold(Vec3::splat(f32::MIN), Vec3::max);
        (min, max)
    }

    /// Where camera-space `p` lands on screen, −1..1 each way.
    fn ndc(p: Vec3) -> Vec3 {
        let clip = hand_projection(16.0 / 9.0) * p.extend(1.0);
        clip.truncate() / clip.w
    }

    #[test]
    fn the_bare_arm_comes_in_from_the_lower_right_and_points_ahead() {
        let meshes = build(&input(None));
        assert!(meshes.item.is_empty());
        // Two boxes (arm and sleeve), six faces each.
        assert_eq!(meshes.arm.indices.len() / 6, 12);
        let (min, max) = bounds(&meshes.arm);
        // In front of the camera, to the right and below the middle.
        assert!(max.z < 0.0, "{min} {max}");
        assert!(min.x > -0.1 && max.x > 0.3, "{min} {max}");
        assert!(max.y < 0.1, "{min} {max}");
        // The near end runs off the bottom-right of the screen; the fist
        // is in the lower right quarter.
        let corners = [
            min,
            max,
            Vec3::new(min.x, max.y, min.z),
            Vec3::new(max.x, min.y, max.z),
        ];
        let far = corners
            .iter()
            .map(|&c| ndc(c))
            .fold(Vec3::splat(f32::MIN), |a, b| a.max(b));
        assert!(far.x > 0.3, "reaches the right side: {far}");
        let near = corners
            .iter()
            .map(|&c| ndc(c))
            .fold(Vec3::splat(f32::MAX), |a, b| a.min(b));
        assert!(near.y < -0.5, "reaches the bottom: {near}");
        assert!(near.x > -0.2, "stays on the right: {near}");
    }

    #[test]
    fn the_left_hand_mirrors_the_right() {
        let geometry = sword();
        let item = HandItem {
            geometry: &geometry,
            display: Transform {
                rotation: [0.0, 0.0, 0.0],
                translation: [0.0; 3],
                scale: [0.5; 3],
            },
            glint: false,
        };
        for held in [None, Some(item)] {
            for swing in [0.0, 0.3, 0.7] {
                let right = build(&HandInput {
                    swing,
                    ..input(held)
                });
                let left = build(&HandInput {
                    left: true,
                    swing,
                    ..input(held)
                });
                let (r, l) = if held.is_some() {
                    (bounds(&right.item), bounds(&left.item))
                } else {
                    (bounds(&right.arm), bounds(&left.arm))
                };
                // Mirrored across the middle of the screen.
                assert!((r.0.y - l.0.y).abs() < 0.05 && (r.1.y - l.1.y).abs() < 0.05);
                assert!((r.0.z - l.0.z).abs() < 0.05 && (r.1.z - l.1.z).abs() < 0.05);
                assert!(
                    (r.0.x + l.1.x).abs() < 0.05 && (r.1.x + l.0.x).abs() < 0.05,
                    "held {} swing {swing}: {r:?} {l:?}",
                    held.is_some()
                );
            }
        }
    }

    #[test]
    fn a_held_item_rests_at_the_lower_right() {
        let geometry = sword();
        let item = HandItem {
            geometry: &geometry,
            display: display(),
            glint: false,
        };
        let meshes = build(&input(Some(item)));
        assert!(meshes.arm.is_empty(), "an item replaces the bare arm");
        let (min, max) = bounds(&meshes.item);
        let middle = (min + max) / 2.0;
        assert!((middle.x - 0.56).abs() < 0.25, "{middle}");
        assert!((middle.y + 0.52).abs() < 0.3, "{middle}");
        assert!((middle.z + 0.72).abs() < 0.3, "{middle}");
        let screen = ndc(middle);
        assert!(
            screen.x > 0.2 && screen.y < -0.1 && screen.y > -1.0,
            "{screen}"
        );
    }

    #[test]
    fn lowering_the_hand_drops_the_item_by_six_tenths() {
        let geometry = sword();
        let item = HandItem {
            geometry: &geometry,
            display: display(),
            glint: false,
        };
        let raised = bounds(&build(&input(Some(item))).item);
        let lowered = bounds(
            &build(&HandInput {
                equip: 1.0,
                ..input(Some(item))
            })
            .item,
        );
        assert!(((raised.0.y - lowered.0.y) - 0.6).abs() < 1e-4);
        assert!((raised.0.x - lowered.0.x).abs() < 1e-4);
    }

    #[test]
    fn swinging_moves_the_item_and_arm_and_returns_to_rest() {
        let geometry = sword();
        let item = HandItem {
            geometry: &geometry,
            display: display(),
            glint: false,
        };
        for held in [Some(item), None] {
            let at = |swing: f32| {
                let m = build(&HandInput {
                    swing,
                    ..input(held)
                });
                bounds(if held.is_some() { &m.item } else { &m.arm })
            };
            let rest = at(0.0);
            let mid = at(0.5);
            assert!(
                (mid.0 - rest.0).length() > 0.1,
                "swinging moves it: {rest:?} {mid:?}"
            );
            let end = at(1.0);
            assert!(
                (end.0 - rest.0).length() < 1e-3 && (end.1 - rest.1).length() < 1e-3,
                "ends back at rest: {rest:?} {end:?}"
            );
        }
    }

    #[test]
    fn eating_brings_the_food_up_toward_the_middle_of_the_screen() {
        let geometry = sword();
        let item = HandItem {
            geometry: &geometry,
            display: display(),
            glint: false,
        };
        let at = |remaining: f32| {
            let m = build(&HandInput {
                using: HandUse::Eat {
                    remaining,
                    total: 32.0,
                },
                ..input(Some(item))
            });
            let (min, max) = bounds(&m.item);
            (min + max) / 2.0
        };
        let start = at(31.0);
        let end = at(2.0);
        // It comes in from the right to the middle of the screen, still
        // in the lower half, where the mouth is.
        assert!(end.x < start.x - 0.4 && end.x.abs() < 0.2, "{start} {end}");
        let screen = ndc(end);
        assert!(screen.x.abs() < 0.25 && screen.y < 0.0, "{screen}");
    }

    #[test]
    fn drawing_a_bow_pulls_it_back_toward_the_player() {
        let geometry = sword();
        let item = HandItem {
            geometry: &geometry,
            display: display(),
            glint: false,
        };
        let at = |drawn: f32| {
            let m = build(&HandInput {
                using: HandUse::Bow { drawn, full: 20.0 },
                ..input(Some(item))
            });
            let (min, max) = bounds(&m.item);
            (min + max) / 2.0
        };
        let relaxed = at(0.0);
        let full = at(20.0);
        assert!(full.z > relaxed.z, "{relaxed} {full}");
        assert!((full.z - relaxed.z) < 0.2);
    }

    #[test]
    fn the_hand_lags_and_shakes_with_the_view() {
        let geometry = sword();
        let item = HandItem {
            geometry: &geometry,
            display: display(),
            glint: false,
        };
        let rest = bounds(&build(&input(Some(item))).item);
        // The camera has turned right: the hand trails to the left.
        let lagging = bounds(
            &build(&HandInput {
                lag: (0.0, 5.0),
                ..input(Some(item))
            })
            .item,
        );
        assert!(lagging.0.x < rest.0.x - 0.02, "{rest:?} {lagging:?}");
        // The camera has tipped up: the hand trails below.
        let looking_up = bounds(
            &build(&HandInput {
                lag: (5.0, 0.0),
                ..input(Some(item))
            })
            .item,
        );
        assert!(looking_up.0.y < rest.0.y - 0.02);
        let bobbing = bounds(
            &build(&HandInput {
                bob: ViewBob {
                    sideways: 0.02,
                    down: -0.03,
                    roll: 0.02,
                    pitch: 0.02,
                },
                ..input(Some(item))
            })
            .item,
        );
        assert!((bobbing.0 - rest.0).length() > 0.01);
    }
}
