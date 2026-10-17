//! Mob meshes from their box models, rebuilt every frame, following 1.0.0's
//! `ModelRenderer`, `ModelBox` and `RenderLiving`: each part rotates about
//! its pivot (with walking, head-turning and zombie-arm animation), the
//! model is turned to the mob's facing, tinted red when hurt and tipped
//! over when dead.

use glam::{Mat3, Vec3};

use crate::{
    entity::{
        arrow::Arrow,
        mob::{DEATH_TICKS, Mob, angle_difference},
        mobs::{MobDef, Model, Part, PartBox, Role},
    },
    world::mesh::Mesh,
};

/// Model pixels per block.
const PIXELS: f32 = 16.0;
/// Model y of the feet.
const FEET_Y: f32 = 24.0;

/// Adds `mob`'s model to `mesh`, `alpha` of the way between ticks, lit by
/// (sky, block) `light`.
pub fn add_mob(mesh: &mut Mesh, mob: &Mob, def: &MobDef, alpha: f32, light: (u8, u8)) {
    let (feet, yaw, limb_amount) = mob.pose(alpha);
    let swing = mob.limb_swing;
    let head_yaw = angle_difference(mob.head_yaw, yaw);
    let time = mob.age as f32 + alpha;
    // Dead mobs tip over onto their side over 20 ticks (1.0.0's rotateCorpse).
    let death = if mob.is_dead() {
        let f = ((mob.death_time as f32 + alpha - 1.0) / DEATH_TICKS as f32 * 1.6)
            .max(0.0)
            .sqrt()
            .min(1.0);
        f * std::f32::consts::FRAC_PI_2
    } else {
        0.0
    };
    let turn = Mat3::from_rotation_y(-yaw) * Mat3::from_rotation_z(death);
    let tint = if mob.hurt_time > 0 {
        [1.0, 0.6, 0.6]
    } else {
        [1.0; 3]
    };
    let light = [light.0 as f32, light.1 as f32];
    let place = |model: Vec3| -> [f32; 3] {
        // Model space is world space turned 180° about z, in pixels, with
        // the feet at y 24.
        let local = Vec3::new(-model.x, FEET_Y - model.y, model.z) / PIXELS;
        (feet.as_vec3() + turn * local).to_array()
    };
    for part in &def.model.parts {
        let (pitch, turn_y) = animate(part, swing, limb_amount, head_yaw, mob.pitch, time);
        // 1.0.0 rotates a part about z, then y, then x (applied x first).
        let rotation = Mat3::from_rotation_y(turn_y) * Mat3::from_rotation_x(pitch);
        let pivot = Vec3::from(part.pivot);
        let main = PartBox {
            uv: part.uv,
            from: part.from,
            size: part.size,
        };
        for b in std::iter::once(&main).chain(&part.extra) {
            add_box(
                mesh,
                &def.model,
                b,
                part.mirror,
                |corner| place(pivot + rotation * corner),
                tint,
                light,
            );
        }
    }
}

/// Arrow size: 1.0.0 draws 16 model pixels at 0.05625 blocks each.
const ARROW_SCALE: f32 = 0.05625;

/// Adds an arrow (1.0.0's `RenderArrow`): two crossed shaft planes and a
/// crossed tail, from arrow.png.
pub fn add_arrow(mesh: &mut Mesh, arrow: &Arrow, alpha: f32, light: (u8, u8)) {
    let center = arrow.position_between_ticks(alpha as f64).as_vec3();
    let forward = arrow.heading.as_vec3().normalize_or(Vec3::NEG_Z);
    let helper = if forward.y.abs() > 0.9 {
        Vec3::X
    } else {
        Vec3::Y
    };
    let side = forward.cross(helper).normalize();
    let up = side.cross(forward);
    let place = |x: f32, y: f32, z: f32| {
        (center + (forward * x + up * y + side * z) * ARROW_SCALE).to_array()
    };
    let light = [light.0 as f32, light.1 as f32];
    let color = [1.0; 3];
    let shaft = [0.0, 0.0, 0.5, 5.0 / 32.0];
    let tail = [0.0, 5.0 / 32.0, 5.0 / 32.0, 10.0 / 32.0];
    // The shaft in two crossed planes along the arrow, then the fletching
    // across its back end.
    mesh.quad(
        [
            place(-8.0, -2.0, 0.0),
            place(8.0, -2.0, 0.0),
            place(8.0, 2.0, 0.0),
            place(-8.0, 2.0, 0.0),
        ],
        shaft,
        [color; 4],
        [light; 4],
    );
    mesh.quad(
        [
            place(-8.0, 0.0, -2.0),
            place(8.0, 0.0, -2.0),
            place(8.0, 0.0, 2.0),
            place(-8.0, 0.0, 2.0),
        ],
        shaft,
        [color; 4],
        [light; 4],
    );
    mesh.quad(
        [
            place(-7.0, -2.0, -2.0),
            place(-7.0, -2.0, 2.0),
            place(-7.0, 2.0, 2.0),
            place(-7.0, 2.0, -2.0),
        ],
        tail,
        [color; 4],
        [light; 4],
    );
}

/// A part's rotation this frame about x and y (1.0.0's
/// `setRotationAngles`).
fn animate(
    part: &Part,
    swing: f32,
    amount: f32,
    head_yaw: f32,
    pitch: f32,
    time: f32,
) -> (f32, f32) {
    use std::f32::consts::PI;
    let stride = swing * 0.6662;
    let x = match part.role {
        Role::None => part.rest,
        Role::Head => part.rest + pitch,
        Role::LegA => stride.cos() * 1.4 * amount,
        Role::LegB => (stride + PI).cos() * 1.4 * amount,
        Role::ArmA => (stride + PI).cos() * amount,
        Role::ArmB => stride.cos() * amount,
        Role::Reaching => -PI / 2.0 + (time * 0.067).sin() * 0.05,
    };
    let y = if part.role == Role::Head {
        head_yaw
    } else {
        0.0
    };
    (x, y)
}

/// Adds one box with Minecraft's box texture layout: the top and bottom
/// along the upper strip, then the four sides below them.
fn add_box(
    mesh: &mut Mesh,
    model: &Model,
    b: &PartBox,
    mirror: bool,
    place: impl Fn(Vec3) -> [f32; 3],
    tint: [f32; 3],
    light: [f32; 2],
) {
    let (u, v) = b.uv;
    let (w, h, d) = b.size;
    let (mut x1, y1, z1) = b.from;
    let mut x2 = x1 + w;
    let (y2, z2) = (y1 + h, z1 + d);
    if mirror {
        std::mem::swap(&mut x1, &mut x2);
    }
    let corner = |x: f32, y: f32, z: f32| Vec3::new(x, y, z);
    let v0 = corner(x1, y1, z1);
    let v1 = corner(x2, y1, z1);
    let v2 = corner(x2, y2, z1);
    let v3 = corner(x1, y2, z1);
    let v4 = corner(x1, y1, z2);
    let v5 = corner(x2, y1, z2);
    let v6 = corner(x2, y2, z2);
    let v7 = corner(x1, y2, z2);
    let (tw, th) = model.texture_size;
    // 1.0.0's ModelBox faces: corners and texture rect (u1, v1, u2, v2).
    let faces = [
        (
            [v5, v1, v2, v6],
            [u + d + w, v + d, u + d + w + d, v + d + h],
            0.6,
        ),
        ([v0, v4, v7, v3], [u, v + d, u + d, v + d + h], 0.6),
        ([v5, v4, v0, v1], [u + d, v, u + d + w, v + d], 1.0),
        ([v2, v3, v7, v6], [u + d + w, v + d, u + d + w + w, v], 0.5),
        ([v1, v0, v3, v2], [u + d, v + d, u + d + w, v + d + h], 0.8),
        (
            [v4, v5, v6, v7],
            [u + d + w + d, v + d, u + d + w + d + w, v + d + h],
            0.8,
        ),
    ];
    for (corners, [a, b2, c, e], shade) in faces {
        // Minecraft's quad corners carry uvs (u2,v1), (u1,v1), (u1,v2),
        // (u2,v2); in this mesh's bottom-left, bottom-right, top-right,
        // top-left order that's corners 2, 3, 0, 1 over the rect.
        let [m0, m1, m2, m3] = corners;
        let uv = [a / tw, b2 / th, c / tw, e / th];
        let color = tint.map(|t| t * shade);
        mesh.quad(
            [place(m2), place(m3), place(m0), place(m1)],
            uv,
            [color; 4],
            [light; 4],
        );
    }
}

#[cfg(test)]
mod tests {
    use glam::DVec3;

    use super::*;
    use crate::entity::mobs::MobRegistry;

    fn mesh_of(name: &str, yaw: f32, dead: bool) -> Mesh {
        let mobs = MobRegistry::load(&crate::assets::dir().join("data/mobs.ron")).unwrap();
        let kind = mobs.find(name).unwrap();
        let def = mobs.get(kind);
        let mut mob = Mob::new(kind, def, DVec3::new(0.5, 64.0, 0.5), yaw);
        mob.head_yaw = yaw;
        if dead {
            mob.stats.health = 0;
            mob.death_time = DEATH_TICKS;
        }
        let mut mesh = Mesh::default();
        add_mob(&mut mesh, &mob, def, 1.0, (15, 0));
        mesh
    }

    fn bounds(mesh: &Mesh) -> (Vec3, Vec3) {
        let points = mesh.vertices.iter().map(|v| Vec3::from(v.position));
        let min = points.clone().fold(Vec3::splat(f32::MAX), Vec3::min);
        let max = points.fold(Vec3::splat(f32::MIN), Vec3::max);
        (min, max)
    }

    #[test]
    fn models_have_their_boxes_and_stand_on_their_feet() {
        let pig = mesh_of("pig", 0.0, false);
        assert_eq!(pig.indices.len() / 6, 7 * 6, "head, snout, body, four legs");
        let zombie = mesh_of("zombie", 0.0, false);
        assert_eq!(zombie.indices.len() / 6, 6 * 6);
        let (min, max) = bounds(&zombie);
        assert!((min.y - 64.0).abs() < 1e-4, "feet on the ground: {min}");
        assert!((max.y - 66.0).abs() < 1e-4, "two blocks tall: {max}");
    }

    #[test]
    fn the_pig_faces_its_yaw_and_dead_mobs_lie_down() {
        // The snout reaches furthest forward.
        let north = bounds(&mesh_of("pig", 0.0, false));
        assert!(north.0.z < 0.5 - 0.5, "snout to the north: {:?}", north);
        let east = bounds(&mesh_of("pig", std::f32::consts::FRAC_PI_2, false));
        assert!(east.1.x > 0.5 + 0.5, "snout to the east: {:?}", east);
        let (_, top) = bounds(&mesh_of("zombie", 0.0, true));
        assert!(top.y < 64.0 + 0.7, "lying down: {top}");
    }
}
