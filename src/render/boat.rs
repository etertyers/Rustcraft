//! Boats and rafts as modern `BoatModel`, `ChestBoatModel`, `RaftModel`
//! and `ChestRaftModel` draw them (128 × 64 textures, 128 × 128 with a
//! chest), placed as `BoatRenderer` places them, with the paddles swinging
//! while they row.

use std::f32::consts::{FRAC_PI_2, PI};

use glam::{Mat3, Quat, Vec3};

use super::models::{BoxStyle, add_box};
use crate::{
    entity::{boat::Boat, mobs::PartBox},
    world::mesh::Mesh,
};

/// Model pixels per block.
const PIXELS: f32 = 16.0;

/// One box of a model part: texture corner, start and size in pixels.
type Cube = ((f32, f32), [f32; 3], [f32; 3]);

/// A model part: its boxes, pivot and rotation about x, y and z.
struct Part {
    cubes: &'static [Cube],
    pivot: [f32; 3],
    rotation: [f32; 3],
}

const BOAT: [Part; 5] = [
    // Bottom.
    Part {
        cubes: &[((0.0, 0.0), [-14.0, -9.0, -3.0], [28.0, 16.0, 3.0])],
        pivot: [0.0, 3.0, 1.0],
        rotation: [FRAC_PI_2, 0.0, 0.0],
    },
    // Back.
    Part {
        cubes: &[((0.0, 19.0), [-13.0, -7.0, -1.0], [18.0, 6.0, 2.0])],
        pivot: [-15.0, 4.0, 4.0],
        rotation: [0.0, PI * 1.5, 0.0],
    },
    // Front.
    Part {
        cubes: &[((0.0, 27.0), [-8.0, -7.0, -1.0], [16.0, 6.0, 2.0])],
        pivot: [15.0, 4.0, 0.0],
        rotation: [0.0, FRAC_PI_2, 0.0],
    },
    // Right.
    Part {
        cubes: &[((0.0, 35.0), [-14.0, -7.0, -1.0], [28.0, 6.0, 2.0])],
        pivot: [0.0, 4.0, -9.0],
        rotation: [0.0, PI, 0.0],
    },
    // Left.
    Part {
        cubes: &[((0.0, 43.0), [-14.0, -7.0, -1.0], [28.0, 6.0, 2.0])],
        pivot: [0.0, 4.0, 9.0],
        rotation: [0.0, 0.0, 0.0],
    },
];

const RAFT: [Part; 1] = [Part {
    cubes: &[
        ((0.0, 0.0), [-14.0, -11.0, -4.0], [28.0, 20.0, 4.0]),
        ((0.0, 0.0), [-14.0, -9.0, -8.0], [28.0, 16.0, 4.0]),
    ],
    pivot: [0.0, -2.1, 1.0],
    rotation: [1.570_796_4, 0.0, 0.0],
}];

/// The paddles' boxes (texture corner, then the blade's), left then
/// right, and their pivots, for a boat and a raft.
const BOAT_PADDLES: [((f32, f32), [f32; 3]); 2] = [
    ((62.0, 0.0), [3.0, -5.0, 9.0]),
    ((62.0, 20.0), [3.0, -5.0, -9.0]),
];
const RAFT_PADDLES: [((f32, f32), [f32; 3]); 2] = [
    ((0.0, 24.0), [3.0, -4.0, 9.0]),
    ((40.0, 24.0), [3.0, -4.0, -9.0]),
];

/// The chest's bottom, lid and lock (texture corner, size and where each
/// sits), and how much lower a raft's sit.
const CHEST: [Cube; 3] = [
    ((0.0, 76.0), [12.0, 8.0, 12.0], [-2.0, -5.0, -6.0]),
    ((0.0, 59.0), [12.0, 4.0, 12.0], [-2.0, -9.0, -6.0]),
    ((0.0, 59.0), [2.0, 4.0, 1.0], [-1.0, -6.0, -1.0]),
];
const RAFT_CHEST_DROP: f32 = -5.1;

/// The texture size: 128 × 64, or 128 × 128 with a chest.
pub fn texture_size(chest: bool) -> (f32, f32) {
    if chest { (128.0, 128.0) } else { (128.0, 64.0) }
}

/// A paddle's turn about x and y `rowing` radians into its stroke (modern
/// `BoatModel.animatePaddle`).
pub fn paddle_angles(rowing: f32, right: bool) -> (f32, f32) {
    let lerp = |a: f32, b: f32, t: f32| a + (b - a) * t;
    let x = lerp(-PI / 3.0, -PI / 12.0, ((-rowing).sin() + 1.0) / 2.0);
    let y = lerp(-PI / 4.0, PI / 4.0, ((-rowing + 1.0).sin() + 1.0) / 2.0);
    (x, if right { PI - y } else { y })
}

/// Adds `boat` between ticks (`alpha`), lit by `light`, to `mesh`.
pub fn add_boat(mesh: &mut Mesh, boat: &Boat, alpha: f32, light: (u8, u8)) {
    let at = boat
        .prev_position
        .lerp(boat.position, alpha as f64)
        .as_vec3();
    let yaw = boat.prev_yaw + (boat.yaw - boat.prev_yaw) * alpha;
    let hurt = (boat.hurt_time - alpha).max(0.0);
    let shake = if hurt > 0.0 {
        (hurt.sin() * hurt * boat.damage.max(0.0) / 10.0 * boat.hurt_dir).to_radians()
    } else {
        0.0
    };
    // `BoatRenderer`: up 0.375, turned (vanilla's 180° − yaw is −yaw in
    // the camera's terms), shaken, flipped and turned a quarter.
    let frame = Mat3::from_rotation_y(-yaw)
        * Mat3::from_rotation_x(shake)
        * Mat3::from_diagonal(Vec3::new(-1.0, -1.0, 1.0))
        * Mat3::from_rotation_y(FRAC_PI_2);
    let size = texture_size(boat.kind.chest);
    let light = [light.0 as f32, light.1 as f32];
    let mut add =
        |uv: (f32, f32), from: [f32; 3], extent: [f32; 3], pivot: [f32; 3], rot: [f32; 3]| {
            let turn = Mat3::from_quat(Quat::from_euler(
                glam::EulerRot::ZYX,
                rot[2],
                rot[1],
                rot[0],
            ));
            let part = PartBox {
                uv,
                from: (from[0], from[1], from[2]),
                size: (extent[0], extent[1], extent[2]),
            };
            add_box(
                mesh,
                size,
                &part,
                BoxStyle {
                    mirror: false,
                    grow: 0.0,
                },
                |corner| {
                    let model = Vec3::from(pivot) + turn * corner;
                    (at + Vec3::Y * 0.375 + frame * (model / PIXELS)).to_array()
                },
                [1.0; 3],
                light,
            );
        };
    let raft = boat.kind.is_raft();
    let hull: &[Part] = if raft { &RAFT } else { &BOAT };
    for part in hull {
        for &(uv, from, extent) in part.cubes {
            add(uv, from, extent, part.pivot, part.rotation);
        }
    }
    let paddles = if raft { RAFT_PADDLES } else { BOAT_PADDLES };
    for (side, (uv, pivot)) in paddles.into_iter().enumerate() {
        let (x, y) = paddle_angles(boat.paddles[side], side == 1);
        let rot = [x, y, PI / 16.0];
        let blade_x = if side == 1 { 0.001 } else { -1.001 };
        add(uv, [-1.0, 0.0, -5.0], [2.0, 2.0, 18.0], pivot, rot);
        add(uv, [blade_x, -3.0, 8.0], [1.0, 6.0, 7.0], pivot, rot);
    }
    if boat.kind.chest {
        let drop = if raft { RAFT_CHEST_DROP } else { 0.0 };
        for (uv, extent, [x, y, z]) in CHEST {
            add(
                uv,
                [0.0; 3],
                extent,
                [x, y + drop, z],
                [0.0, -FRAC_PI_2, 0.0],
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use glam::DVec3;

    use super::*;
    use crate::entity::boat::BoatKind;

    /// The box around the hull: the first five boxes.
    fn bounds(mesh: &Mesh) -> (Vec3, Vec3) {
        mesh.vertices[..5 * 24].iter().fold(
            (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)),
            |(lo, hi), v| {
                let p = Vec3::from(v.position);
                (lo.min(p), hi.max(p))
            },
        )
    }

    #[test]
    fn a_boat_is_long_along_its_heading_and_sits_on_its_position() {
        let boat = Boat::new(1, BoatKind::of_item("oak_boat").unwrap(), DVec3::ZERO, 0.0);
        let mut mesh = Mesh::default();
        add_boat(&mut mesh, &boat, 1.0, (15, 0));
        // Hull 5 boxes, paddles 4: 9 boxes of 24 corners.
        assert_eq!(mesh.vertices.len(), 9 * 24);
        let (lo, hi) = bounds(&mesh);
        // Facing north, the hull runs 28 pixels along z and 20 across x.
        let (long, wide) = (hi.z - lo.z, hi.x - lo.x);
        assert!(long > 1.7 && wide < long, "{long} × {wide}");
        assert!(lo.y > -0.3 && lo.y < 0.1, "bottom at {}", lo.y);
        // Turned east, the long side runs along x.
        let east = Boat::new(1, boat.kind, DVec3::ZERO, FRAC_PI_2);
        let mut mesh = Mesh::default();
        add_boat(&mut mesh, &east, 1.0, (15, 0));
        let (lo, hi) = bounds(&mesh);
        assert!(hi.x - lo.x > hi.z - lo.z);
    }

    #[test]
    fn chest_boats_and_rafts_have_their_own_parts() {
        let chest = Boat::new(
            1,
            BoatKind::of_item("birch_chest_boat").unwrap(),
            DVec3::ZERO,
            0.0,
        );
        let mut mesh = Mesh::default();
        add_boat(&mut mesh, &chest, 1.0, (15, 0));
        assert_eq!(mesh.vertices.len(), 12 * 24);
        let raft = Boat::new(
            1,
            BoatKind::of_item("bamboo_raft").unwrap(),
            DVec3::ZERO,
            0.0,
        );
        let mut mesh = Mesh::default();
        add_boat(&mut mesh, &raft, 1.0, (15, 0));
        assert_eq!(mesh.vertices.len(), 6 * 24);
    }

    #[test]
    fn paddles_swing_through_their_stroke() {
        let (x0, y0) = paddle_angles(0.0, false);
        let (x1, y1) = paddle_angles(PI / 2.0, false);
        assert!((x0 - x1).abs() > 0.1 && (y0 - y1).abs() > 0.1);
        let (_, right) = paddle_angles(0.0, true);
        assert!((right - (PI - y0)).abs() < 1e-6);
    }
}
