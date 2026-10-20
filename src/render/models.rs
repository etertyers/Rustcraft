//! Mob meshes from their box models, rebuilt every frame, following 1.0.0's
//! `ModelRenderer`, `ModelBox` and `RenderLiving`: each part rotates about
//! its pivot (with walking, head-turning and zombie-arm animation), the
//! model is turned to the mob's facing, tinted red when hurt and tipped
//! over when dead.

use glam::{Mat3, Vec3};

use crate::{
    entity::{
        arrow::Arrow,
        minecart::Minecart,
        mob::{DEATH_TICKS, Mob, angle_difference},
        mobs::{MobDef, Part, PartBox, Role},
    },
    world::mesh::Mesh,
};

/// Model pixels per block.
const PIXELS: f32 = 16.0;
/// Model y of the feet.
const FEET_Y: f32 = 24.0;

/// Adds `mob`'s body to `mesh`, `alpha` of the way between ticks, lit by
/// (sky, block) `light`.
pub fn add_mob(mesh: &mut Mesh, mob: &Mob, def: &MobDef, alpha: f32, light: (u8, u8)) {
    add_mob_model(mesh, mob, def, &def.model, alpha, light, [1.0; 3]);
}

/// The 16 dye colors a sheep's wool takes, in wool order (modern
/// `DyeColor` texture colors).
pub const DYE_COLORS: [[f32; 3]; 16] = [
    [0.976, 0.996, 0.996],
    [0.976, 0.502, 0.114],
    [0.780, 0.306, 0.741],
    [0.227, 0.702, 0.855],
    [0.996, 0.847, 0.239],
    [0.502, 0.780, 0.122],
    [0.953, 0.545, 0.667],
    [0.278, 0.310, 0.322],
    [0.616, 0.616, 0.592],
    [0.086, 0.612, 0.612],
    [0.537, 0.196, 0.722],
    [0.235, 0.267, 0.667],
    [0.514, 0.329, 0.196],
    [0.369, 0.486, 0.086],
    [0.690, 0.180, 0.149],
    [0.114, 0.114, 0.129],
];

/// What a model needs to know about the mob it's posed for.
struct Pose {
    swing: f32,
    amount: f32,
    head_yaw: f32,
    pitch: f32,
    time: f32,
    aggressive: bool,
    airborne: bool,
    sitting: bool,
}

/// Adds one of `def`'s models (its body or a layer) posed for `mob`,
/// multiplying its colors by `tint`.
pub fn add_mob_model(
    mesh: &mut Mesh,
    mob: &Mob,
    def: &MobDef,
    model: &crate::entity::mobs::Model,
    alpha: f32,
    light: (u8, u8),
    tint: [f32; 3],
) {
    let (feet, yaw, limb_amount) = mob.pose(alpha);
    let head_yaw = angle_difference(mob.head_yaw, yaw);
    let pose = Pose {
        swing: mob.limb_swing,
        amount: limb_amount,
        head_yaw,
        pitch: mob.pitch,
        time: mob.age as f32 + alpha,
        aggressive: mob.aggressive,
        airborne: !mob.body.on_ground,
        sitting: mob.data.sitting,
    };
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
    // A hurt mob flashes red; a creeper swells and blinks white.
    let mut color = if mob.hurt_time > 0 {
        [1.0, 0.6, 0.6]
    } else {
        [1.0; 3]
    };
    let mut scale = Vec3::splat(def.scale);
    if mob.data.size > 0 {
        scale *= f32::from(mob.data.size);
    }
    // A baby villager is half size.
    if mob.data.villager.as_ref().is_some_and(|v| v.baby > 0) {
        scale *= 0.5;
    }
    let swell = mob.swell_amount(alpha);
    if swell > 0.0 {
        let wobble = 1.0 + (swell * 100.0).sin() * swell * 0.01;
        let puff = swell * swell * swell * swell;
        scale *= Vec3::new(
            (1.0 + puff * 0.4) * wobble,
            (1.0 + puff * 0.1) / wobble,
            (1.0 + puff * 0.4) * wobble,
        );
        if ((swell * 10.0) as i32) % 2 == 0 {
            color = color.map(|c| c + 1.5);
        }
    }
    let color = [color[0] * tint[0], color[1] * tint[1], color[2] * tint[2]];
    let light = [light.0 as f32, light.1 as f32];
    let place = |model: Vec3| -> [f32; 3] {
        // Model space is world space turned 180° about z, in pixels, with
        // the feet at y 24.
        let local = Vec3::new(-model.x, FEET_Y - model.y, model.z) / PIXELS * scale;
        (feet.as_vec3() + turn * local).to_array()
    };
    for part in &model.parts {
        let (rx, ry, rz, pivot) = animate(part, &pose);
        // 1.0.0 rotates a part about z, then y, then x (applied x first).
        let rotation =
            Mat3::from_rotation_z(rz) * Mat3::from_rotation_y(ry) * Mat3::from_rotation_x(rx);
        let main = PartBox {
            uv: part.uv,
            from: part.from,
            size: part.size,
        };
        for b in std::iter::once(&main).chain(&part.extra) {
            add_box(
                mesh,
                model.texture_size,
                b,
                BoxStyle {
                    mirror: part.mirror,
                    grow: part.grow,
                },
                |corner| place(pivot + rotation * corner),
                color,
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

/// The minecart's parts (modern `MinecartModel`): the floor and four
/// walls as (box origin, size, texture offset, pivot, turn about x, turn
/// about y), in model pixels with y pointing down.
/// One of the cart's boxes.
struct CartPart {
    from: [f32; 3],
    size: [f32; 3],
    uv: (f32, f32),
    pivot: [f32; 3],
    pitch: f32,
    yaw: f32,
}

const CART_PARTS: [CartPart; 5] = {
    use std::f32::consts::{FRAC_PI_2, PI};
    const fn wall(pivot: [f32; 3], yaw: f32) -> CartPart {
        CartPart {
            from: [-8.0, -9.0, -1.0],
            size: [16.0, 8.0, 2.0],
            uv: (0.0, 0.0),
            pivot,
            pitch: 0.0,
            yaw,
        }
    }
    [
        CartPart {
            from: [-10.0, -8.0, -1.0],
            size: [20.0, 16.0, 2.0],
            uv: (0.0, 10.0),
            pivot: [0.0, 4.0, 0.0],
            pitch: FRAC_PI_2,
            yaw: 0.0,
        },
        wall([-9.0, 4.0, 0.0], 3.0 * FRAC_PI_2),
        wall([9.0, 4.0, 0.0], FRAC_PI_2),
        wall([0.0, 4.0, -7.0], PI),
        wall([0.0, 4.0, 7.0], 0.0),
    ]
};

/// Adds a minecart's shell (modern `MinecartRenderer`), turned to face
/// along its rail and drawn from minecart.png.
pub fn add_minecart(mesh: &mut Mesh, cart: &Minecart, alpha: f32, light: (u8, u8)) {
    let at = cart.between_ticks(alpha as f64).as_vec3();
    let turn = Mat3::from_rotation_y(std::f32::consts::PI - cart.yaw);
    // The shaking after a hit tilts it a little.
    let shake = if cart.wobble > 0 {
        (cart.wobble as f32 - alpha).max(0.0).sin() * 0.1
    } else {
        0.0
    };
    let tilt = Mat3::from_rotation_x(shake);
    let light = [light.0 as f32, light.1 as f32];
    for CartPart {
        from,
        size,
        uv,
        pivot,
        pitch,
        yaw,
    } in CART_PARTS
    {
        let part = PartBox {
            uv,
            from: (from[0], from[1], from[2]),
            size: (size[0], size[1], size[2]),
        };
        let rotation = Mat3::from_rotation_y(yaw) * Mat3::from_rotation_x(pitch);
        add_box(
            mesh,
            (64.0, 32.0),
            &part,
            BoxStyle {
                mirror: false,
                grow: 0.0,
            },
            |corner| {
                let model = Vec3::from(pivot) + rotation * corner;
                let local = Vec3::new(-model.x, -model.y, model.z) / PIXELS + Vec3::Y * 0.375;
                (at + tilt * (turn * local)).to_array()
            },
            [1.0; 3],
            light,
        );
    }
}

/// A part's rotation this frame about x, y and z, and where it pivots
/// (1.0.0's `setRotationAngles`).
fn animate(part: &Part, pose: &Pose) -> (f32, f32, f32, Vec3) {
    use std::f32::consts::{FRAC_PI_2, FRAC_PI_4, PI};
    let pivot = Vec3::from(part.pivot);
    let stride = pose.swing * 0.6662;
    let (rest_x, rest_y, rest_z) = (part.rest, part.rest_y, part.rest_z);
    let amount = pose.amount;
    let flap = if pose.airborne {
        (pose.time * 1.1).sin().abs() * 1.0 + 0.4
    } else {
        0.0
    };
    match part.role {
        Role::None => (rest_x, rest_y, rest_z, pivot),
        Role::Head => (rest_x + pose.pitch, rest_y + pose.head_yaw, rest_z, pivot),
        Role::LegA => (
            if pose.sitting {
                rest_x
            } else {
                stride.cos() * 1.4 * amount
            },
            rest_y,
            rest_z,
            pivot,
        ),
        Role::LegB => (
            if pose.sitting {
                rest_x
            } else {
                (stride + PI).cos() * 1.4 * amount
            },
            rest_y,
            rest_z,
            pivot,
        ),
        Role::ArmA => ((stride + PI).cos() * amount, rest_y, rest_z, pivot),
        Role::ArmB => (stride.cos() * amount, rest_y, rest_z, pivot),
        Role::Reaching => (
            -FRAC_PI_2 + (pose.time * 0.067).sin() * 0.05,
            rest_y,
            rest_z,
            pivot,
        ),
        // Arms held out in front while hunting, else swinging.
        Role::Aim => {
            if pose.aggressive {
                (-FRAC_PI_2 + pose.pitch, rest_y, rest_z, pivot)
            } else {
                (stride.cos() * amount, rest_y, rest_z, pivot)
            }
        }
        Role::WingA => (rest_x, rest_y, rest_z + wing_beat(pose, flap), pivot),
        Role::WingB => (rest_x, rest_y, rest_z - wing_beat(pose, flap), pivot),
        Role::Tail => (rest_x, rest_y + stride.cos() * 1.4 * amount, rest_z, pivot),
        Role::SpiderLeg(i) => {
            let i = usize::from(i.min(7));
            let f = FRAC_PI_4;
            let g = FRAC_PI_4 / 2.0;
            let yaws = [f, -f, g, -g, -g, g, -f, f];
            let rolls = [-f, f, -f * 0.74, f * 0.74, -f * 0.74, f * 0.74, -f, f];
            let phase = [0.0, PI, FRAC_PI_2, PI * 1.5][i / 2];
            let wiggle = -((pose.swing * 0.6662 * 2.0 + phase).cos() * 0.4) * amount;
            let lift = -((pose.swing * 0.6662 + phase).sin().abs() * 0.4) * amount;
            let sign = if i % 2 == 0 { 1.0 } else { -1.0 };
            (
                rest_x,
                yaws[i] + wiggle * sign,
                rolls[i] + lift * sign,
                pivot,
            )
        }
        Role::Tentacle(i) => {
            let angle = f32::from(i) * PI * 2.0 / 8.0;
            let swing = pose.time * 0.2 + f32::from(i);
            (
                swing.sin() * 0.5 + 0.2,
                -angle + FRAC_PI_2,
                rest_z,
                Vec3::new(angle.cos() * 5.0, pivot.y, angle.sin() * 5.0),
            )
        }
    }
}

/// How far a wing is raised: bees and phantoms beat all the time, birds
/// only when off the ground.
fn wing_beat(pose: &Pose, flap: f32) -> f32 {
    if pose.aggressive && !pose.airborne {
        0.0
    } else {
        flap
    }
}

/// How a box is built beyond its size and texture position.
#[derive(Debug, Clone, Copy)]
pub struct BoxStyle {
    /// Swaps the box's left and right, for mirrored limbs.
    pub mirror: bool,
    /// Pixels the box is grown on every side without changing its
    /// texture layout (a skin's hat and sleeve layers).
    pub grow: f32,
}

/// Adds one box with Minecraft's box texture layout: the top and bottom
/// along the upper strip, then the four sides below them.
pub fn add_box(
    mesh: &mut Mesh,
    texture_size: (f32, f32),
    b: &PartBox,
    style: BoxStyle,
    place: impl Fn(Vec3) -> [f32; 3],
    tint: [f32; 3],
    light: [f32; 2],
) {
    let BoxStyle { mirror, grow } = style;
    let (u, v) = b.uv;
    let (w, h, d) = b.size;
    let (fx, fy, fz) = b.from;
    let (mut x1, mut x2) = (fx - grow, fx + w + grow);
    let (y1, y2) = (fy - grow, fy + h + grow);
    let (z1, z2) = (fz - grow, fz + d + grow);
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
    let (tw, th) = texture_size;
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
            mob.stats.health = 0.0;
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

    #[test]
    fn a_minecart_is_a_floor_and_four_walls_long_along_its_rail() {
        use crate::entity::minecart::{CartKind, Minecart};
        let mut cart = Minecart::new(CartKind::Rideable, DVec3::new(0.5, 64.0, 0.5));
        let mut mesh = Mesh::default();
        add_minecart(&mut mesh, &cart, 1.0, (15, 0));
        assert_eq!(mesh.indices.len() / 6, 5 * 6);
        let (min, max) = bounds(&mesh);
        assert!(min.y >= 64.0 && max.y < 64.8, "{min} {max}");
        // Heading east (yaw 0) it is 20 pixels long along x, 16 across.
        assert!((max.x - min.x - 1.25).abs() < 1e-3, "{}", max.x - min.x);
        assert!((max.z - min.z - 1.0).abs() < 1e-3, "{}", max.z - min.z);
        cart.yaw = std::f32::consts::FRAC_PI_2;
        let mut turned = Mesh::default();
        add_minecart(&mut turned, &cart, 1.0, (15, 0));
        let (min, max) = bounds(&turned);
        assert!((max.z - min.z - 1.25).abs() < 1e-3, "now along z");
    }
}
