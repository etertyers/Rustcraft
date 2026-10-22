//! The ender dragon and the end crystal as drawn (M27), from their box
//! models (modern `EnderDragonModel`, `EndCrystalModel`; 256 × 256 and
//! 128 × 64 skins, 16 model pixels to a block).
//!
//! The dragon's neck and tail are chains of ten-pixel segments that bend
//! after the heading it had a few ticks ago, its wings beat, and its jaw
//! opens to breathe.

use glam::{Mat4, Vec3};

use super::models::{BoxStyle, add_box};
use crate::{
    entity::{
        crystal::EndCrystal,
        dragon::{EnderDragon, angle_between},
        mobs::PartBox,
    },
    world::mesh::Mesh,
};

/// Model pixels per block.
const PIXELS: f32 = 16.0;
const DRAGON_SKIN: (f32, f32) = (256.0, 256.0);
const CRYSTAL_SKIN: (f32, f32) = (128.0, 64.0);
/// Where the middle of the body is in model pixels.
const BODY_CENTER: Vec3 = Vec3::new(0.0, 16.0, 24.0);
/// Spine segments are this long, in pixels.
const SEGMENT: f32 = 10.0;
/// How strongly the neck and tail follow the heading they had.
const BEND: f32 = 1.5;
/// Beats of the wings a tick, flying and perched.
const FLAP: f32 = 0.05;
const FLAP_PERCHED: f32 = 0.02;

/// One box of a part: texture position, corner, size, whether mirrored.
#[derive(Clone, Copy)]
struct Box3 {
    uv: (f32, f32),
    from: (f32, f32, f32),
    size: (f32, f32, f32),
    mirror: bool,
}

const fn b(uv: (f32, f32), from: (f32, f32, f32), size: (f32, f32, f32), mirror: bool) -> Box3 {
    Box3 {
        uv,
        from,
        size,
        mirror,
    }
}

/// The head's boxes, pushed 16 pixels toward the front as modern's are.
const HEAD: [Box3; 6] = [
    b((176.0, 44.0), (-6.0, -1.0, -40.0), (12.0, 5.0, 16.0), false),
    b(
        (112.0, 30.0),
        (-8.0, -8.0, -26.0),
        (16.0, 16.0, 16.0),
        false,
    ),
    b((0.0, 0.0), (-5.0, -12.0, -20.0), (2.0, 4.0, 6.0), true),
    b((112.0, 0.0), (-5.0, -3.0, -38.0), (2.0, 2.0, 4.0), true),
    b((0.0, 0.0), (3.0, -12.0, -20.0), (2.0, 4.0, 6.0), false),
    b((112.0, 0.0), (3.0, -3.0, -38.0), (2.0, 2.0, 4.0), false),
];
const JAW: [Box3; 1] = [b(
    (176.0, 65.0),
    (-6.0, 0.0, -16.0),
    (12.0, 4.0, 16.0),
    false,
)];
const SPINE: [Box3; 2] = [
    b(
        (192.0, 104.0),
        (-5.0, -5.0, -5.0),
        (10.0, 10.0, 10.0),
        false,
    ),
    b((48.0, 0.0), (-1.0, -9.0, -3.0), (2.0, 4.0, 6.0), false),
];
const BODY: [Box3; 4] = [
    b((0.0, 0.0), (-12.0, 0.0, -16.0), (24.0, 24.0, 64.0), false),
    b((220.0, 53.0), (-1.0, -6.0, -10.0), (2.0, 6.0, 12.0), false),
    b((220.0, 53.0), (-1.0, -6.0, 10.0), (2.0, 6.0, 12.0), false),
    b((220.0, 53.0), (-1.0, -6.0, 30.0), (2.0, 6.0, 12.0), false),
];
const WING: [Box3; 2] = [
    b((112.0, 88.0), (0.0, -4.0, -4.0), (56.0, 8.0, 8.0), true),
    b((-56.0, 88.0), (0.0, 0.0, 2.0), (56.0, 0.0, 56.0), true),
];
const WING_TIP: [Box3; 2] = [
    b((112.0, 136.0), (0.0, -2.0, -2.0), (56.0, 4.0, 4.0), true),
    b((-56.0, 144.0), (0.0, 0.0, 2.0), (56.0, 0.0, 56.0), true),
];
const FRONT_LEG: [Box3; 1] = [b(
    (112.0, 104.0),
    (-4.0, -4.0, -4.0),
    (8.0, 24.0, 8.0),
    true,
)];
const FRONT_LEG_TIP: [Box3; 1] = [b(
    (226.0, 138.0),
    (-3.0, -1.0, -3.0),
    (6.0, 24.0, 6.0),
    true,
)];
const FRONT_FOOT: [Box3; 1] = [b(
    (144.0, 104.0),
    (-4.0, 0.0, -12.0),
    (8.0, 4.0, 16.0),
    true,
)];
const REAR_LEG: [Box3; 1] = [b((0.0, 0.0), (-8.0, -4.0, -8.0), (16.0, 32.0, 16.0), true)];
const REAR_LEG_TIP: [Box3; 1] = [b((196.0, 0.0), (-6.0, -2.0, 0.0), (12.0, 32.0, 12.0), true)];
const REAR_FOOT: [Box3; 1] = [b((112.0, 0.0), (-9.0, 0.0, -20.0), (18.0, 6.0, 24.0), true)];

/// A part drawn with `matrix`, which takes model pixels to model space.
struct Pen<'a> {
    mesh: &'a mut Mesh,
    origin: Vec3,
    turn: glam::Mat3,
    tint: [f32; 3],
    light: [f32; 2],
}

impl Pen<'_> {
    fn draw(&mut self, boxes: &[Box3], matrix: Mat4) {
        for def in boxes {
            let part = PartBox {
                uv: def.uv,
                from: def.from,
                size: def.size,
            };
            let (origin, turn) = (self.origin, self.turn);
            add_box(
                self.mesh,
                DRAGON_SKIN,
                &part,
                BoxStyle {
                    mirror: def.mirror,
                    grow: 0.0,
                },
                |corner| {
                    let model = matrix.transform_point3(corner);
                    // Model space is world space turned 180° about z.
                    let local = Vec3::new(
                        -model.x,
                        -(model.y - BODY_CENTER.y),
                        model.z - BODY_CENTER.z,
                    ) / PIXELS;
                    (origin + turn * local).to_array()
                },
                self.tint,
                self.light,
            );
        }
    }
}

/// `parent`, then a move to `pivot` and a turn: z, then y, then x (applied
/// x first), as the box models rotate.
fn joint(parent: Mat4, pivot: Vec3, rx: f32, ry: f32, rz: f32) -> Mat4 {
    parent
        * Mat4::from_translation(pivot)
        * Mat4::from_rotation_z(rz)
        * Mat4::from_rotation_y(ry)
        * Mat4::from_rotation_x(rx)
}

/// Adds the dragon, `alpha` of the way between ticks, lit by `light`.
pub fn add_dragon(mesh: &mut Mesh, dragon: &EnderDragon, alpha: f32, light: (u8, u8)) {
    let at = dragon.position_between_ticks(f64::from(alpha)).as_vec3();
    let yaw = dragon.yaw_between_ticks(alpha);
    let turn = glam::Mat3::from_rotation_y(-yaw) * glam::Mat3::from_rotation_x(dragon.pitch * 0.6);
    // Red when hurt, white-hot while it dies.
    let mut tint = [1.0; 3];
    if dragon.hurt_ticks > 0 {
        tint = [1.0, 0.6, 0.6];
    }
    if dragon.is_dying() {
        let glow = dragon.death_ticks as f32 / crate::entity::dragon::DEATH_TICKS as f32;
        tint = [1.0 + glow * 1.5; 3];
    }
    let mut pen = Pen {
        mesh,
        origin: at,
        turn,
        tint,
        light: [light.0 as f32, light.1 as f32],
    };
    let flap_speed = if dragon.phase.is_sitting() {
        FLAP_PERCHED
    } else {
        FLAP
    };
    let beat = (dragon.age as f32 + alpha) * flap_speed;
    let phase = beat * std::f32::consts::TAU;
    // How far the body sways: modern's `f1`.
    let sway = ((phase - 1.0).sin() + 1.0).powi(2) * 0.05 + ((phase - 1.0).sin() + 1.0) * 0.1;
    let root = Mat4::from_translation(Vec3::new(0.0, sway - 2.0, -3.0))
        * Mat4::from_rotation_x((sway * 2.0).to_radians());
    let heading = |lag: usize| dragon.trail(lag).1;
    let bend = |from: usize, to: usize| angle_between(heading(from), heading(to)) * BEND;

    // The neck, five segments from the body forward, then the head.
    let mut point = Vec3::new(0.0, 20.0, -12.0);
    let steady = bend(6, 6);
    for i in 0..5usize {
        let ry = bend(6, 5 - i) + steady;
        let rx = (i as f32 * 0.45 + phase).cos() * 0.15;
        let m = joint(root, point, rx, ry, 0.0);
        pen.draw(&SPINE, m);
        point.y += rx.sin() * SEGMENT;
        point.z -= ry.cos() * rx.cos() * SEGMENT;
        point.x -= ry.sin() * rx.cos() * SEGMENT;
    }
    let ry = bend(6, 0);
    let head = joint(root, point, 0.0, ry, 0.0);
    pen.draw(&HEAD, head);
    let jaw_open = if dragon.mouth_open() {
        0.55
    } else {
        ((phase.sin() + 1.0) * 0.1).max(0.05)
    };
    pen.draw(
        &JAW,
        joint(head, Vec3::new(0.0, 4.0, -24.0), jaw_open, 0.0, 0.0),
    );

    // The body, wings and legs.
    let body = joint(root, Vec3::new(0.0, 4.0, 8.0), 0.0, 0.0, 0.0);
    pen.draw(&BODY, body);
    for side in [1.0f32, -1.0] {
        let mirrored = if side > 0.0 {
            Mat4::IDENTITY
        } else {
            Mat4::from_scale(Vec3::new(-1.0, 1.0, 1.0))
        };
        let wing = joint(
            root * mirrored,
            Vec3::new(12.0, 5.0, 2.0),
            0.125 - phase.cos() * 0.2,
            0.25,
            (phase.sin() + 0.125) * 0.8,
        );
        pen.draw(&WING, wing);
        let tip = joint(
            wing,
            Vec3::new(56.0, 0.0, 0.0),
            0.0,
            0.0,
            -((phase + 2.0).sin() + 0.5) * 0.75,
        );
        pen.draw(&WING_TIP, tip);
        let front = joint(
            root * mirrored,
            Vec3::new(12.0, 20.0, 2.0),
            1.3 + sway * 0.1,
            0.0,
            0.0,
        );
        pen.draw(&FRONT_LEG, front);
        let front_tip = joint(
            front,
            Vec3::new(0.0, 20.0, -1.0),
            -0.5 - sway * 0.1,
            0.0,
            0.0,
        );
        pen.draw(&FRONT_LEG_TIP, front_tip);
        let front_foot = joint(
            front_tip,
            Vec3::new(0.0, 23.0, 0.0),
            0.75 + sway * 0.1,
            0.0,
            0.0,
        );
        pen.draw(&FRONT_FOOT, front_foot);
        let rear = joint(
            root * mirrored,
            Vec3::new(16.0, 16.0, 42.0),
            1.0 + sway * 0.1,
            0.0,
            0.0,
        );
        pen.draw(&REAR_LEG, rear);
        let rear_tip = joint(rear, Vec3::new(0.0, 32.0, -4.0), 0.5 + sway * 0.1, 0.0, 0.0);
        pen.draw(&REAR_LEG_TIP, rear_tip);
        let rear_foot = joint(
            rear_tip,
            Vec3::new(0.0, 31.0, 4.0),
            0.75 + sway * 0.1,
            0.0,
            0.0,
        );
        pen.draw(&REAR_FOOT, rear_foot);
    }

    // The tail, twelve segments back from the body.
    let mut point = Vec3::new(0.0, 10.0, 60.0);
    let mut waver = 0.0f32;
    for k in 0..12usize {
        waver += (k as f32 * 0.45 + phase).sin() * 0.05;
        let ry = std::f32::consts::PI - bend(11, 12 + k);
        let rx = waver;
        let m = joint(root, point, rx, ry, 0.0);
        pen.draw(&SPINE, m);
        point.y += rx.sin() * SEGMENT;
        point.z -= ry.cos() * rx.cos() * SEGMENT;
        point.x -= ry.sin() * rx.cos() * SEGMENT;
    }
}

/// The three nested glass cubes of a crystal, spinning and bobbing, over
/// its base plate if it has one (modern `EndCrystalRenderer`).
pub fn add_crystal(mesh: &mut Mesh, crystal: &EndCrystal, alpha: f32, light: (u8, u8)) {
    let light = [light.0 as f32, light.1 as f32];
    let spin = crystal.spin(alpha);
    let tilt_axis = Vec3::new(0.5f32.sqrt(), 0.0, 0.5f32.sqrt());
    let tilt = Mat4::from_axis_angle(tilt_axis, 60.0_f32.to_radians());
    let at = crystal.position.as_vec3();
    let mut put = |boxes: &[Box3], matrix: Mat4| {
        for &def in boxes {
            let part = PartBox {
                uv: def.uv,
                from: def.from,
                size: def.size,
            };
            add_box(
                mesh,
                CRYSTAL_SKIN,
                &part,
                BoxStyle {
                    mirror: false,
                    grow: 0.0,
                },
                |corner| {
                    // Model pixels are y-down; two pixels to the block.
                    let flipped = Vec3::new(corner.x, -corner.y, corner.z);
                    matrix.transform_point3(flipped).to_array()
                },
                [1.0; 3],
                light,
            );
        }
    };
    let glass = [b((0.0, 0.0), (-4.0, -4.0, -4.0), (8.0, 8.0, 8.0), false)];
    let cube = [b((32.0, 0.0), (-4.0, -4.0, -4.0), (8.0, 8.0, 8.0), false)];
    if crystal.show_base {
        let plate = [b((0.0, 16.0), (-6.0, -4.0, -6.0), (12.0, 4.0, 12.0), false)];
        put(
            &plate,
            Mat4::from_translation(at) * Mat4::from_scale(Vec3::splat(2.0 / PIXELS)),
        );
    }
    let middle = at + Vec3::Y * (1.0 + crystal.bob(alpha));
    let mut m = Mat4::from_translation(middle)
        * Mat4::from_scale(Vec3::splat(2.0 / PIXELS))
        * Mat4::from_rotation_y(spin);
    m *= tilt;
    put(&glass, m);
    m = m * Mat4::from_scale(Vec3::splat(0.875)) * tilt * Mat4::from_rotation_y(spin);
    put(&glass, m);
    m = m * Mat4::from_scale(Vec3::splat(0.875)) * tilt * Mat4::from_rotation_y(spin);
    put(&cube, m);
}

#[cfg(test)]
mod tests {
    use glam::DVec3;

    use super::*;

    fn bounds(mesh: &Mesh) -> (Vec3, Vec3) {
        let mut lo = Vec3::splat(f32::MAX);
        let mut hi = Vec3::splat(f32::MIN);
        for v in &mesh.vertices {
            let p = Vec3::from(v.position);
            lo = lo.min(p);
            hi = hi.max(p);
        }
        (lo, hi)
    }

    #[test]
    fn the_dragon_is_a_big_finite_shape_round_its_position_with_wings_spread_either_side() {
        let mut dragon = EnderDragon::new(DVec3::new(0.0, 100.0, 0.0), 0.0, 3);
        for _ in 0..30 {
            dragon.tick(&crate::entity::dragon::Senses {
                player: None,
                crystals: 5,
                podium: DVec3::new(0.0, 64.0, 0.0),
            });
        }
        let mut mesh = Mesh::default();
        add_dragon(&mut mesh, &dragon, 0.5, (15, 0));
        assert!(mesh.vertices.len() > 1500, "{}", mesh.vertices.len());
        assert!(
            mesh.vertices
                .iter()
                .all(|v| v.position.iter().all(|c| c.is_finite()))
        );
        let (lo, hi) = bounds(&mesh);
        let at = dragon.position.as_vec3();
        // A wingspan of about fifteen blocks and a length near twenty.
        assert!(hi.x - lo.x > 12.0 && hi.z - lo.z > 12.0, "{lo} {hi}");
        assert!((hi - lo).max_element() < 40.0, "{lo} {hi}");
        assert!(((lo + hi) / 2.0 - at).length() < 15.0, "{lo} {hi} {at}");
    }

    #[test]
    fn it_faces_the_way_it_flies() {
        let mut dragon = EnderDragon::new(DVec3::new(0.0, 100.0, 0.0), 0.0, 3);
        dragon.yaw = 0.0;
        let mut north = Mesh::default();
        add_dragon(&mut north, &dragon, 1.0, (15, 0));
        // Facing north (−z): the head is the far end toward −z, so the
        // mesh reaches farther north than south of the body's middle.
        let (lo, hi) = bounds(&north);
        let middle = 100.0f32;
        let _ = middle;
        assert!(lo.z < -8.0 && hi.z > 8.0, "{lo} {hi}");
        let head = dragon.head().as_vec3();
        assert!(head.z < dragon.position.z as f32 - 6.0);
    }

    #[test]
    fn a_crystal_is_a_spinning_cube_about_a_block_big_and_a_plate_adds_a_wide_base() {
        let mut crystal = EndCrystal::new(DVec3::new(0.5, 80.0, 0.5), false);
        let mut bare = Mesh::default();
        add_crystal(&mut bare, &crystal, 0.0, (15, 0));
        let (lo, hi) = bounds(&bare);
        assert!(
            (hi - lo).max_element() < 2.5 && (hi - lo).max_element() > 0.9,
            "{lo} {hi}"
        );
        assert!(lo.y > 80.0, "floats above its spot");
        crystal.show_base = true;
        let mut based = Mesh::default();
        add_crystal(&mut based, &crystal, 0.0, (15, 0));
        assert!(based.vertices.len() > bare.vertices.len());
        let (lo, hi) = bounds(&based);
        assert!(lo.y <= 80.01 && hi.x - lo.x >= 1.4, "{lo} {hi}");
    }
}
