//! Mob meshes from their box models, rebuilt every frame, following 1.0.0's
//! `ModelRenderer`, `ModelBox` and `RenderLiving`: each part rotates about
//! its pivot (with walking, head-turning and zombie-arm animation), the
//! model is turned to the mob's facing, tinted red when hurt and tipped
//! over when dead.

use glam::{Mat3, Mat4, Vec3};

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

/// Where a mob's main hand is, as a matrix from item space to the world
/// (vanilla's `ItemInHandLayer`: out to the end of the arm, laid along it,
/// then a nudge to the palm), `alpha` of the way between ticks. `None` if
/// the model has no hand.
pub fn hand_mount(mob: &Mob, def: &MobDef, alpha: f32) -> Option<Mat4> {
    let part = def.model.parts.iter().find(|part| part.hand)?;
    let (feet, yaw, _) = mob.pose(alpha);
    let pose = Pose::of(mob, def, alpha, angle_difference(mob.head_yaw, yaw));
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
    let mut scale = def.scale;
    if mob.data.size > 0 {
        scale *= f32::from(mob.data.size);
    }
    let (rx, ry, rz, pivot) = animate(part, &pose);
    let rotation =
        Mat3::from_rotation_z(rz) * Mat3::from_rotation_y(ry) * Mat3::from_rotation_x(rx);
    // Model pixels become blocks with y pointing down from the feet at
    // 1.5 blocks, as `place` does for the body.
    let to_world = Mat4::from_translation(feet.as_vec3())
        * Mat4::from_mat3(turn)
        * Mat4::from_scale(Vec3::new(-scale, -scale, scale))
        * Mat4::from_translation(Vec3::new(0.0, -FEET_Y / PIXELS, 0.0));
    let arm = Mat4::from_translation(pivot / PIXELS) * Mat4::from_mat3(rotation);
    let palm = Mat4::from_rotation_x(-std::f32::consts::FRAC_PI_2)
        * Mat4::from_rotation_y(std::f32::consts::PI)
        * Mat4::from_translation(Vec3::new(1.0 / 16.0, 0.125, -0.625));
    Some(to_world * arm * palm)
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

/// The names vanilla's models give a mob's parts, worked out from their
/// roles and order (mobs.ron has none): `head`, `body`, the legs and
/// arms of a humanoid or a four-legged walker, wings, a tail, a spider's
/// `leg1` to `leg8`. A custom model that `attach`es to a part is
/// matched by these.
pub fn vanilla_part_names(model: &crate::entity::mobs::Model) -> Vec<String> {
    let humanoid = model
        .parts
        .iter()
        .any(|p| matches!(p.role, Role::ArmA | Role::ArmB | Role::Reaching | Role::Aim));
    let (mut legs, mut arms, mut bodies) = (0, 0, 0);
    model
        .parts
        .iter()
        .enumerate()
        .map(|(i, part)| match part.role {
            Role::Head => "head".to_owned(),
            Role::None => {
                bodies += 1;
                if bodies == 1 {
                    "body".to_owned()
                } else {
                    format!("part{i}")
                }
            }
            Role::LegA | Role::LegB if humanoid => if part.role == Role::LegA {
                "right_leg"
            } else {
                "left_leg"
            }
            .to_owned(),
            Role::LegA | Role::LegB | Role::CatLeg(_) => {
                legs += 1;
                [
                    "right_hind_leg",
                    "left_hind_leg",
                    "right_front_leg",
                    "left_front_leg",
                ]
                .get(legs - 1)
                .map_or_else(|| format!("leg{legs}"), |n| (*n).to_owned())
            }
            Role::ArmA | Role::ArmB | Role::Reaching | Role::Aim => {
                arms += 1;
                if arms == 1 { "right_arm" } else { "left_arm" }.to_owned()
            }
            Role::WingA => "right_wing".to_owned(),
            Role::WingB => "left_wing".to_owned(),
            Role::Lid => "lid".to_owned(),
            Role::Tail => "tail".to_owned(),
            Role::SpiderLeg(n) => format!("leg{}", n + 1),
            Role::Tentacle(n) | Role::GhastTentacle(n) => format!("tentacle{}", n + 1),
            Role::BlazeRod(n) | Role::Slice(n) | Role::Segment(n) | Role::Hop(n) => {
                format!("part{i}_{n}")
            }
        })
        .collect()
}

/// What a model needs to know about the mob it's posed for.
struct Pose {
    swing: f32,
    amount: f32,
    head_yaw: f32,
    pitch: f32,
    time: f32,
    aggressive: bool,
    sitting: bool,
    /// Wing angle this frame (chickens, bees, phantoms).
    wing: f32,
    /// A squid's tentacle angle this frame.
    tentacle: f32,
    /// Largest swing of arms and legs.
    limit: f32,
    /// A slime's squash this frame.
    squish: f32,
}

impl Pose {
    fn of(mob: &Mob, def: &MobDef, alpha: f32, head_yaw: f32) -> Self {
        let (swing, amount) = mob.limbs(alpha);
        let time = mob.age as f32 + alpha;
        Self {
            swing,
            amount,
            head_yaw,
            pitch: mob.pitch,
            time,
            aggressive: mob.aggressive,
            sitting: mob.data.sitting,
            wing: wing_angle(mob, &def.name, time, alpha),
            tentacle: tentacle_angle(time + mob.id as f32 * 7.0),
            limit: def.limb_limit.unwrap_or(f32::MAX),
            squish: mob.prev_squish + (mob.squish - mob.prev_squish) * alpha,
        }
    }
}

/// How far a wing is raised, as vanilla's models have it: a chicken by its
/// flapping state, a bee by its age while off the ground, a phantom by its
/// age.
fn wing_angle(mob: &Mob, name: &str, time: f32, alpha: f32) -> f32 {
    use std::f32::consts::PI;
    match name {
        "chicken" => {
            let flap = mob.prev_flap + (mob.flap - mob.prev_flap) * alpha;
            let speed = mob.prev_flap_speed + (mob.flap_speed - mob.prev_flap_speed) * alpha;
            (flap.sin() + 1.0) * speed
        }
        "bee" if !mob.body.on_ground => (time * 2.1).cos() * PI * 0.15,
        "bee" => 0.0,
        "phantom" => {
            ((time + mob.id as f32 * 3.0) * 7.448_451_f32.to_radians()).cos()
                * 16.0_f32.to_radians()
        }
        _ => 0.0,
    }
}

/// A squid's tentacle swing: a pulse, then still (vanilla's
/// `tentacleMovement`).
fn tentacle_angle(time: f32) -> f32 {
    use std::f32::consts::PI;
    let movement = (time * 0.04) % (2.0 * PI);
    if movement < PI {
        let t = movement / PI;
        (t * t * PI).sin() * PI * 0.25
    } else {
        0.0
    }
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
    // A custom entity model (a resource pack's, M58) draws in place of the
    // mob's own body; its layers (wool, saddles) are left out.
    let custom = std::ptr::eq(model, &def.model)
        .then(crate::cem::active)
        .flatten()
        .and_then(|registry| registry.find(&def.name, mob.data.age < 0).cloned())
        .filter(|_| mob.data.variant == 0);
    add_mob_model_using(mesh, mob, def, model, alpha, light, tint, custom);
}

/// `add_mob_model` with the custom model to draw the body with, if any.
#[allow(clippy::too_many_arguments)]
pub fn add_mob_model_using(
    mesh: &mut Mesh,
    mob: &Mob,
    def: &MobDef,
    model: &crate::entity::mobs::Model,
    alpha: f32,
    light: (u8, u8),
    tint: [f32; 3],
    custom: Option<std::sync::Arc<crate::cem::CemModel>>,
) {
    let (feet, yaw, _) = mob.pose(alpha);
    let head_yaw = angle_difference(mob.head_yaw, yaw);
    let pose = Pose::of(mob, def, alpha, head_yaw);
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
    // A baby villager or animal is half size.
    let baby = mob.data.age < 0 || mob.data.villager.as_ref().is_some_and(|v| v.baby > 0);
    // A pack's own baby model is already small.
    let own_baby = baby && custom.as_ref().is_some_and(|m| m.baby);
    if baby && !own_baby {
        scale *= 0.5;
    }
    if mob.data.size > 0 {
        // SlimeRenderer.scale: squashed wide on landing, tall in the air.
        let size = f32::from(mob.data.size.max(1));
        let g = 1.0 / (pose.squish / (size * 0.5 + 1.0) + 1.0);
        scale *= Vec3::new(g, 1.0 / g, g);
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
    let attached: Vec<String> = custom
        .as_ref()
        .map(|cem| {
            cem.parts
                .iter()
                .filter(|p| p.attach)
                .map(|p| p.vanilla.clone().unwrap_or_else(|| p.name.clone()))
                .collect()
        })
        .unwrap_or_default();
    let names = custom.is_some().then(|| vanilla_part_names(model));
    if let Some(cem) = &custom {
        let (frame, counter) = crate::cem::frame();
        let entity = crate::cem::vars::EntityVars::of(mob, def.health as f32, &frame);
        crate::cem::with_state(mob.id, &def.name, cem, &entity, counter, |runtime| {
            crate::cem::pose::add_parts(mesh, cem, runtime, place, color, light);
        });
    }
    for (index, part) in model.parts.iter().enumerate() {
        // Replaced by the custom model, unless it attaches to this part.
        if let Some(names) = &names
            && !attached.contains(&names[index])
        {
            continue;
        }
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
    let limit = pose.limit;
    match part.role {
        Role::None => (rest_x, rest_y, rest_z, pivot),
        Role::Head => (rest_x + pose.pitch, rest_y + pose.head_yaw, rest_z, pivot),
        Role::LegA => (
            if pose.sitting {
                rest_x
            } else {
                (stride.cos() * 1.4 * amount).clamp(-limit, limit)
            },
            rest_y,
            rest_z,
            pivot,
        ),
        Role::LegB => (
            if pose.sitting {
                rest_x
            } else {
                ((stride + PI).cos() * 1.4 * amount).clamp(-limit, limit)
            },
            rest_y,
            rest_z,
            pivot,
        ),
        Role::GhastTentacle(i) => (
            0.2 * (pose.time * 0.3 + f32::from(i)).sin() + 0.4,
            rest_y,
            rest_z,
            pivot,
        ),
        Role::BlazeRod(i) => {
            // Vanilla's BlazeModel: three rings of four rods, bobbing and
            // turning with the age.
            let i = f32::from(i);
            let ring = (i / 4.0).floor();
            let k = i % 4.0;
            let t = pose.time;
            let (angle, radius, y) = match ring as u8 {
                0 => (
                    t * PI * -0.1 + k * FRAC_PI_2,
                    9.0,
                    -2.0 + ((k * 2.0 + t) * 0.25).cos(),
                ),
                1 => (
                    FRAC_PI_4 + t * PI * 0.03 + k * FRAC_PI_2,
                    7.0,
                    2.0 + (((k + 4.0) * 2.0 + t) * 0.25).cos(),
                ),
                _ => (
                    0.471_238_9 + t * PI * -0.05 + k * FRAC_PI_2,
                    5.0,
                    11.0 + (((k + 8.0) * 1.5 + t) * 0.5).cos(),
                ),
            };
            (
                rest_x,
                rest_y,
                rest_z,
                Vec3::new(angle.cos() * radius, y, angle.sin() * radius),
            )
        }
        Role::Slice(i) => (
            rest_x,
            rest_y,
            rest_z,
            pivot - Vec3::Y * ((4.0 - f32::from(i)) * pose.squish.max(0.0) * 1.7),
        ),
        Role::Segment(i) => {
            let i = f32::from(i);
            let phase = pose.time * 0.9 + i * 0.15 * PI;
            let away = (i - 2.0).abs();
            (
                rest_x,
                rest_y + phase.cos() * PI * 0.05 * (1.0 + away),
                rest_z,
                pivot + Vec3::X * (phase.sin() * PI * 0.2 * away),
            )
        }
        Role::Hop(i) => {
            // Both legs of a pair move together; the front pair opposes.
            let swing = stride.cos() * amount;
            (
                rest_x + if i == 0 { swing } else { -swing * 0.6 },
                rest_y,
                rest_z,
                pivot,
            )
        }
        Role::CatLeg(i) => {
            // Vanilla's ocelot gait: 1.0 × speed, hind left and right a
            // little apart, the front pair opposite.
            let phase = [0.0, 0.3, PI + 0.3, PI][usize::from(i.min(3))];
            let angle = if pose.sitting {
                rest_x
            } else {
                (stride + phase).cos() * amount
            };
            (angle, rest_y, rest_z, pivot)
        }
        Role::ArmA => (
            ((stride + PI).cos() * amount).clamp(-limit, limit),
            rest_y,
            rest_z,
            pivot,
        ),
        Role::ArmB => (
            (stride.cos() * amount).clamp(-limit, limit),
            rest_y,
            rest_z,
            pivot,
        ),
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
        Role::Lid => (
            rest_x,
            rest_y,
            rest_z,
            pivot - Vec3::Y * if pose.aggressive { 10.0 } else { 0.0 },
        ),
        Role::WingA => (rest_x, rest_y, rest_z + pose.wing, pivot),
        Role::WingB => (rest_x, rest_y, rest_z - pose.wing, pivot),
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
            (
                pose.tentacle,
                -angle + FRAC_PI_2,
                rest_z,
                Vec3::new(angle.cos() * 5.0, pivot.y, angle.sin() * 5.0),
            )
        }
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

    /// A custom pig written for these tests: a head that attaches to the
    /// vanilla head, and a body that replaces the vanilla body, all
    /// swinging with the limb swing.
    const CUSTOM_PIG: &str = r#"{
        "textureSize": [64, 32],
        "models": [
            {"part": "head", "id": "head", "attach": true, "invertAxis": "xy", "translate": [0, -12, 6],
             "boxes": [{"coordinates": [-1, -1, -1, 2, 2, 2], "textureOffset": [0, 0]}],
             "animations": [{"head.rx": "sin(age * 0.1) * 0.2"}]},
            {"part": "body", "id": "body", "invertAxis": "xy", "translate": [0, -11, -2],
             "boxes": [{"coordinates": [-5, -8, -8, 10, 16, 8], "textureOffset": [28, 8]}]}
        ]
    }"#;

    fn custom(text: &str) -> std::sync::Arc<crate::cem::CemModel> {
        std::sync::Arc::new(
            crate::cem::model::parse(text, &|_| None, &mut |_| {}).expect("a model"),
        )
    }

    fn mesh_using(name: &str, model: Option<std::sync::Arc<crate::cem::CemModel>>) -> Mesh {
        let mobs = MobRegistry::load(&crate::assets::dir().join("data/mobs.ron")).unwrap();
        let kind = mobs.find(name).unwrap();
        let def = mobs.get(kind);
        let mob = Mob::new(kind, def, DVec3::new(0.5, 64.0, 0.5), 0.0);
        let mut mesh = Mesh::default();
        add_mob_model_using(
            &mut mesh,
            &mob,
            def,
            &def.model,
            1.0,
            (15, 0),
            [1.0; 3],
            model,
        );
        mesh
    }

    #[test]
    fn vanilla_parts_have_the_names_a_custom_model_attaches_to() {
        let mobs = MobRegistry::load(&crate::assets::dir().join("data/mobs.ron")).unwrap();
        let names = |name: &str| vanilla_part_names(&mobs.get(mobs.find(name).unwrap()).model);
        assert_eq!(
            names("pig"),
            [
                "head",
                "body",
                "right_hind_leg",
                "left_hind_leg",
                "right_front_leg",
                "left_front_leg"
            ]
        );
        let zombie = names("zombie");
        for name in [
            "head",
            "body",
            "right_arm",
            "left_arm",
            "right_leg",
            "left_leg",
        ] {
            assert!(zombie.iter().any(|n| n == name), "{name} in {zombie:?}");
        }
    }

    #[test]
    fn a_custom_model_replaces_the_body_attaches_where_asked_and_other_mobs_are_unchanged() {
        let plain = mesh_using("pig", None);
        assert_eq!(plain.indices.len() / 6, 7 * 6);
        let with = mesh_using("pig", Some(custom(CUSTOM_PIG)));
        // The vanilla head and snout stay (the head is attached to), and
        // the custom model adds a head box and a body: 2 + 1 + 1 boxes.
        assert_eq!(
            with.indices.len() / 6,
            4 * 6,
            "attached head keeps its two boxes"
        );
        assert_ne!(with, plain);
        // A mob the pack has no model for looks as before: the same mob
        // drawn through the public entry point (which finds no model for
        // it) and with none given is the same mesh.
        let mobs = MobRegistry::load(&crate::assets::dir().join("data/mobs.ron")).unwrap();
        let kind = mobs.find("zombie").unwrap();
        let def = mobs.get(kind);
        let mob = Mob::new(kind, def, DVec3::new(0.5, 64.0, 0.5), 0.0);
        let (mut a, mut b) = (Mesh::default(), Mesh::default());
        add_mob_model(&mut a, &mob, def, &def.model, 1.0, (15, 0), [1.0; 3]);
        add_mob_model_using(&mut b, &mob, def, &def.model, 1.0, (15, 0), [1.0; 3], None);
        assert_eq!(a.vertices.len(), b.vertices.len());
        for (i, (x, y)) in a.vertices.iter().zip(&b.vertices).enumerate() {
            assert_eq!(x, y, "vertex {i}");
        }
        assert_eq!(a.indices, b.indices);
        assert_eq!(a.indices.len() / 6, 6 * 6);
        // A model with no attach replaces every vanilla part.
        let replaced = custom(
            r#"{"textureSize": [64, 32], "models": [{"id": "all", "boxes": [{"coordinates": [0, 0, 0, 4, 4, 4], "textureOffset": [0, 0]}]}]}"#,
        );
        assert_eq!(mesh_using("pig", Some(replaced)).indices.len() / 6, 6);
    }

    /// 100 animated mobs on screen: building their meshes and running
    /// their animations takes at most 1.0 ms a frame on the M1 (release).
    #[test]
    fn a_hundred_animated_mobs_take_under_a_millisecond_a_frame() {
        let mobs = MobRegistry::load(&crate::assets::dir().join("data/mobs.ron")).unwrap();
        let kind = mobs.find("pig").unwrap();
        let def = mobs.get(kind);
        // A model of a dozen parts with the kinds of animation lines Fresh
        // Animations uses.
        let mut parts = String::new();
        for i in 0..12 {
            if i > 0 {
                parts.push(',');
            }
            parts.push_str(&format!(
                r#"{{"id": "p{i}", "translate": [0, {y}, 0], "boxes": [{{"coordinates": [-1, 0, -1, 2, 3, 2], "textureOffset": [0, 0]}}],
                   "animations": [{{"var.s{i}": "sin(age * 0.1 + {i}) * limb_speed", "p{i}.rx": "if(is_hurt, 0.3, cos(limb_swing * 0.6662 + {i}) * 1.4 * limb_speed + var.s{i} * 0.1)", "p{i}.ry": "torad(clamp(head_yaw, -30, 30)) * 0.5"}}]}}"#,
                y = i as f32 * 0.5
            ));
        }
        let model = custom(&format!(
            r#"{{"textureSize": [64, 32], "models": [{parts}]}}"#
        ));
        let herd: Vec<Mob> = (0..100)
            .map(|i| {
                let mut mob = Mob::new(kind, def, DVec3::new(i as f64, 64.0, 0.5), 0.0);
                mob.id = 1000 + i as u64;
                mob.limb_swing = i as f32 * 0.3;
                mob
            })
            .collect();
        let frame = |n: u64| {
            crate::cem::begin_frame(n, 0.5, DVec3::ZERO);
            let mut mesh = Mesh::default();
            for mob in &herd {
                add_mob_model_using(
                    &mut mesh,
                    mob,
                    def,
                    &def.model,
                    0.5,
                    (15, 0),
                    [1.0; 3],
                    Some(model.clone()),
                );
            }
            mesh
        };
        for n in 0..20 {
            frame(n); // warm up
        }
        let started = std::time::Instant::now();
        let frames = 200;
        let mut quads = 0;
        for n in 0..frames {
            quads = frame(100 + n).indices.len() / 6;
        }
        let each = started.elapsed().as_secs_f64() * 1000.0 / frames as f64;
        println!("100 animated mobs: {each:.3} ms a frame ({quads} quads)");
        assert_eq!(quads, 100 * 12 * 6);
        // Debug test builds are slower than the release the milestone
        // measures (1.0 ms on the M1).
        // What the same 100 mobs cost with the plain models, for scale.
        let started = std::time::Instant::now();
        for n in 0..frames {
            let mut mesh = Mesh::default();
            for mob in &herd {
                add_mob_model_using(
                    &mut mesh,
                    mob,
                    def,
                    &def.model,
                    0.5,
                    (15, 0),
                    [1.0; 3],
                    None,
                );
            }
            assert!(mesh.indices.len() / 6 > 0 && n < frames);
        }
        let plain = started.elapsed().as_secs_f64() * 1000.0 / frames as f64;
        println!("the same 100 mobs with the plain pig model: {plain:.3} ms a frame");
        let allowed = if cfg!(debug_assertions) { 12.0 } else { 1.0 };
        assert!(each < allowed, "{each:.3} ms a frame");
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

    /// How many times a frame animates each mob's model (Step 0 / M58 fix).
    #[test]
    fn a_mob_is_animated_once_a_frame_layers_included() {
        let dir = crate::assets::dir().join("downloads/resourcepacks");
        let Some(path) = std::fs::read_dir(&dir).ok().and_then(|e| {
            e.filter_map(|e| e.ok().map(|e| e.path()))
                .find(|p| p.to_string_lossy().contains("FreshAnimations"))
        }) else {
            eprintln!("skipping: no Fresh Animations pack");
            return;
        };
        let pack = crate::pack::Pack::open(&path).unwrap();
        crate::cem::install(crate::cem::Registry::load(
            &|p| pack.read(p),
            &["pig", "sheep"],
        ));
        let mobs = MobRegistry::load(&crate::assets::dir().join("data/mobs.ron")).unwrap();
        let count = |name: &str, setup: &dyn Fn(&mut Mob)| -> u32 {
            let kind = mobs.find(name).unwrap();
            let def = mobs.get(kind);
            let mut mob = Mob::new(kind, def, DVec3::new(0.5, 64.0, 0.5), 0.0);
            setup(&mut mob);
            crate::cem::begin_frame(100, 0.5, DVec3::ZERO);
            crate::cem::ANIMATED.with(|n| n.set(0));
            let mut mesh = Mesh::default();
            add_mob(&mut mesh, &mob, def, 0.5, (15, 0));
            for layer in &def.layers {
                if (layer.wool && mob.data.sheared) || (layer.saddle && !mob.data.saddled) {
                    continue;
                }
                add_mob_model(&mut mesh, &mob, def, &layer.model, 0.5, (15, 0), [1.0; 3]);
            }
            crate::cem::ANIMATED.with(|n| n.get())
        };
        let plain_pig = count("pig", &|_| {});
        let saddled_pig = count("pig", &|m| m.data.saddled = true);
        let wool_sheep = count("sheep", &|_| {});
        println!(
            "with_state per frame: pig {plain_pig}, saddled pig {saddled_pig}, sheep with wool {wool_sheep}"
        );
        crate::cem::install(crate::cem::Registry::default());
        assert_eq!((plain_pig, saddled_pig, wool_sheep), (1, 1, 1));
    }

    fn mob_of(name: &str) -> (Mob, &'static MobDef) {
        let mobs: &'static MobRegistry = Box::leak(Box::new(
            MobRegistry::load(&crate::assets::dir().join("data/mobs.ron")).unwrap(),
        ));
        let kind = mobs.find(name).unwrap();
        let def = mobs.get(kind);
        (Mob::new(kind, def, DVec3::new(0.5, 64.0, 0.5), 0.0), def)
    }

    #[test]
    fn the_walk_position_moves_smoothly_between_ticks_like_vanilla() {
        let (mut mob, _) = mob_of("cow");
        mob.limb_swing = 10.0;
        mob.limb_amount = 0.5;
        let (start, _) = mob.limbs(0.0);
        let (end, speed) = mob.limbs(1.0);
        assert!((end - 10.0).abs() < 1e-6 && (start - 9.5).abs() < 1e-6);
        assert!((speed - 0.5).abs() < 1e-6);
        // Babies take three times the steps; the speed never passes 1.
        mob.data.age = -100;
        assert!((mob.limbs(1.0).0 - 30.0).abs() < 1e-5);
        mob.limb_amount = 3.0;
        assert_eq!(mob.limbs(1.0).1, 1.0);
    }

    #[test]
    fn an_endermans_limbs_stop_at_0_4_and_a_cats_front_and_hind_legs_oppose() {
        let (mut mob, def) = mob_of("enderman");
        mob.limb_swing = 0.0;
        mob.limb_amount = 1.0;
        let pose = Pose::of(&mob, def, 1.0, 0.0);
        let leg = def
            .model
            .parts
            .iter()
            .find(|p| p.role == Role::LegA)
            .unwrap();
        assert!((animate(leg, &pose).0 - 0.4).abs() < 1e-6);
        let (mut cat, def) = mob_of("cat");
        cat.limb_amount = 0.5;
        let pose = Pose::of(&cat, def, 1.0, 0.0);
        let angle = |i| {
            let part = def
                .model
                .parts
                .iter()
                .find(|p| p.role == Role::CatLeg(i))
                .unwrap();
            animate(part, &pose).0
        };
        // Peak 1.0 × speed, not the 1.4 of the other walkers.
        assert!(angle(0).abs() <= 0.5 + 1e-6 && (angle(0) - 0.5).abs() < 1e-6);
        assert!(angle(2) < 0.0 && angle(3) < 0.0);
    }

    #[test]
    fn a_chicken_flaps_only_after_being_airborne_and_settles_on_the_ground() {
        let (mut chicken, def) = mob_of("chicken");
        let wing = |m: &Mob| Pose::of(m, def, 1.0, 0.0).wing;
        assert_eq!(wing(&chicken), 0.0);
        let grid = crate::entity::path::tests::Grid::default();
        // Falling with nothing below: the flapping builds.
        chicken.body.on_ground = false;
        for _ in 0..6 {
            chicken.tick(&crate::player::physics::MoveInput::default(), &grid, false);
        }
        assert!(chicken.flap_speed > 0.9, "{}", chicken.flap_speed);
        // Back on a floor the flapping dies away within a few ticks.
        let (mut chicken, _) = mob_of("chicken");
        chicken.flap_speed = 1.0;
        let floor = crate::entity::path::tests::Grid::floor();
        for _ in 0..8 {
            chicken.tick(&crate::player::physics::MoveInput::default(), &floor, false);
        }
        assert_eq!(chicken.flap_speed, 0.0);
        assert!(wing(&chicken).abs() < 1e-6);
    }

    #[test]
    fn a_squids_tentacles_pulse_then_rest() {
        assert_eq!(tentacle_angle(0.0), 0.0);
        let peak = (0..2000)
            .map(|t| tentacle_angle(t as f32 * 0.5))
            .fold(0.0f32, f32::max);
        assert!(peak > 0.5 && peak <= std::f32::consts::FRAC_PI_4 + 1e-4);
        // The second half of the cycle is still.
        assert_eq!(tentacle_angle(std::f32::consts::PI * 1.5 / 0.04), 0.0);
    }

    fn role_part(def: &MobDef, role: Role) -> &Part {
        def.model.parts.iter().find(|p| p.role == role).unwrap()
    }

    #[test]
    fn blaze_rods_circle_ghast_tentacles_sway_and_silverfish_crawl() {
        let (mut blaze, def) = mob_of("blaze");
        blaze.age = 17;
        let pose = Pose::of(&blaze, def, 0.0, 0.0);
        for i in 0..12 {
            let (.., pivot) = animate(role_part(def, Role::BlazeRod(i)), &pose);
            let radius = pivot.x.hypot(pivot.z);
            assert!(
                [9.0, 7.0, 5.0].iter().any(|r| (radius - r).abs() < 1e-3),
                "{i}: {radius}"
            );
        }
        let (ghast, def) = mob_of("ghast");
        let pose = Pose::of(&ghast, def, 0.0, 0.0);
        let x = animate(role_part(def, Role::GhastTentacle(0)), &pose).0;
        assert!((0.2..=0.6).contains(&x));
        let (mut fish, def) = mob_of("silverfish");
        let mut ends = |age| {
            fish.age = age;
            let pose = Pose::of(&fish, def, 0.0, 0.0);
            (
                animate(role_part(def, Role::Segment(2)), &pose).3.x,
                animate(role_part(def, Role::Segment(6)), &pose).3.x,
            )
        };
        // The middle segment (2) holds still sideways; the tail sways.
        let (a, b) = (ends(5), ends(9));
        assert!(a.0.abs() < 1e-6 && a.1.abs() > 0.1);
        assert_ne!(a.1, b.1);
    }

    #[test]
    fn a_slime_squashes_on_landing_stretches_in_the_air_and_a_rabbits_legs_hop_together() {
        let (mut slime, _) = mob_of("slime");
        slime.data.size = 2;
        let floor = crate::entity::path::tests::Grid::floor();
        let none = crate::player::physics::MoveInput::default();
        // Dropped from a little above the floor: it lands and squashes.
        slime.body.on_ground = false;
        slime.tick(&none, &floor, false);
        for _ in 0..12 {
            slime.tick(&none, &floor, false);
        }
        assert!(slime.squish.abs() > 1e-3 || slime.prev_squish.abs() > 1e-3);
        let (mut rabbit, def) = mob_of("rabbit");
        rabbit.limb_amount = 0.5;
        let pose = Pose::of(&rabbit, def, 1.0, 0.0);
        let rear = animate(role_part(def, Role::Hop(0)), &pose).0;
        let front = animate(role_part(def, Role::Hop(1)), &pose).0;
        assert!(rear * front <= 0.0 && rear != 0.0);
    }
}
