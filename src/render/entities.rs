//! Meshes for dropped items, rebuilt every frame, following 1.0.0's
//! `RenderItem`: blocks are spinning quarter-size cubes, other items are
//! half-size pictures turned toward the camera, and both bob up and down.

use glam::{DVec3, IVec3, Vec3};

use super::{
    camera::Camera,
    icons::{IconFace, ItemIcon, ItemIcons},
};
use crate::{
    entity::{
        effects::{Effect, EffectKind, SWEEP_FRAMES},
        falling::FallingBlock,
        item::ItemEntity,
        minecart::{CartKind, Minecart},
        orb::XpOrb,
        particle::Particle,
        tnt::PrimedTnt,
    },
    world::{
        Face,
        block::BlockState,
        blocks,
        mesh::{BlockModels, Mesh, face_corners},
    },
};

const CUBE_HALF: f32 = 0.125;
const SPRITE_HALF_WIDTH: f32 = 0.25;
/// The sprite reaches this far below and above its center.
const SPRITE_BELOW: f32 = 0.125;
const SPRITE_ABOVE: f32 = 0.375;

/// Face shading like the chunk mesher's: top, bottom, north/south, east/west.
const SHADE_TOP: f32 = 1.0;
const SHADE_BOTTOM: f32 = 0.5;
const SHADE_Z: f32 = 0.8;
const SHADE_X: f32 = 0.6;

/// `alpha` is how far the frame is between the last two ticks, and
/// `light_at` gives (sky, block) light at a point.
pub fn item_mesh(
    entities: &[ItemEntity],
    icons: &ItemIcons,
    camera: &Camera,
    alpha: f64,
    light_at: impl Fn(DVec3) -> (u8, u8),
) -> Mesh {
    let mut mesh = Mesh::default();
    for item in entities {
        let center = item.center_between_ticks(alpha);
        let (sky, block) = light_at(center);
        let (sky, block) = super::hand::glinted((sky, block), item.stack.is_enchanted());
        let light = [sky as f32, block as f32];
        let time = item.age as f32 + alpha as f32;
        let bob = ((time / 10.0 + item.hover).sin() * 0.1 + 0.1) as f64;
        let origin = (center + DVec3::Y * bob).as_vec3();
        match icons.icon(&item.stack) {
            ItemIcon::Cube { top, front, side } => {
                let spin = time / 20.0 + item.hover;
                add_cube(&mut mesh, origin, spin, [top, front, side], light);
            }
            ItemIcon::Flat(face) => add_sprite(&mut mesh, origin, camera.yaw, face, light),
        }
    }
    mesh
}

/// Adds ghasts' fireballs: the fire charge's picture facing the camera,
/// three times an item's size and fully lit (modern renders large
/// fireballs as their item, scaled up).
pub fn add_fireballs(
    mesh: &mut Mesh,
    fireballs: &[crate::entity::fireball::Fireball],
    icons: &ItemIcons,
    camera: &Camera,
    alpha: f64,
) {
    let stack = crate::inventory::stack::ItemStack::new(crate::inventory::items::FIRE_CHARGE, 1);
    let ItemIcon::Flat(face) = icons.icon(&stack) else {
        return;
    };
    for ball in fireballs {
        let center = ball.position_between_ticks(alpha).as_vec3();
        let half = if ball.small {
            SMALL_FIREBALL_HALF
        } else {
            FIREBALL_HALF
        };
        let right = Vec3::new(camera.yaw.cos(), 0.0, camera.yaw.sin()) * half;
        let up = Vec3::Y * half;
        let corners = [
            center - right - up,
            center + right - up,
            center + right + up,
            center - right + up,
        ];
        mesh.quad(
            corners.map(|p| p.to_array()),
            face.uv,
            [face.color; 4],
            [[15.0, 15.0]; 4],
        );
    }
}

/// Adds eyes of ender in flight: the item's picture facing the camera.
pub fn add_eyes(
    mesh: &mut Mesh,
    eyes: &[crate::entity::eye::EyeOfEnder],
    icons: &ItemIcons,
    camera: &Camera,
    alpha: f64,
) {
    let stack = crate::inventory::stack::ItemStack::new(crate::inventory::items::ENDER_EYE, 1);
    let ItemIcon::Flat(face) = icons.icon(&stack) else {
        return;
    };
    let right = Vec3::new(camera.yaw.cos(), 0.0, camera.yaw.sin()) * EYE_HALF;
    let up = Vec3::Y * EYE_HALF;
    for eye in eyes {
        let center = eye.position_between_ticks(alpha).as_vec3();
        mesh.quad(
            [
                center - right - up,
                center + right - up,
                center + right + up,
                center - right + up,
            ]
            .map(|p| p.to_array()),
            face.uv,
            [face.color; 4],
            [[15.0, 15.0]; 4],
        );
    }
}

/// Half an eye of ender's drawn width.
const EYE_HALF: f32 = 0.2;

/// Adds the dragon's fireballs: its picture facing the camera, fully lit.
pub fn add_dragon_fireballs(
    mesh: &mut Mesh,
    balls: &[crate::entity::breath::DragonFireball],
    uvs: &EffectUvs,
    camera: &Camera,
    alpha: f64,
) {
    let right = Vec3::new(camera.yaw.cos(), 0.0, camera.yaw.sin()) * DRAGON_FIREBALL_HALF;
    let up = Vec3::Y * DRAGON_FIREBALL_HALF;
    for ball in balls {
        let center = ball.position_between_ticks(alpha).as_vec3();
        mesh.quad(
            [
                center - right - up,
                center + right - up,
                center + right + up,
                center - right + up,
            ]
            .map(|p| p.to_array()),
            uvs.dragon_fireball,
            [[1.0; 3]; 4],
            [[15.0, 15.0]; 4],
        );
    }
}

/// Adds shulkers' bullets: small pale sparks facing the camera.
pub fn add_shulker_bullets(
    mesh: &mut Mesh,
    bullets: &[crate::entity::bullet::ShulkerBullet],
    uvs: &EffectUvs,
    camera: &Camera,
    alpha: f64,
) {
    let right = Vec3::new(camera.yaw.cos(), 0.0, camera.yaw.sin()) * BULLET_HALF;
    let up = Vec3::Y * BULLET_HALF;
    for bullet in bullets {
        let center = bullet.position_between_ticks(alpha).as_vec3();
        mesh.quad(
            [
                center - right - up,
                center + right - up,
                center + right + up,
                center - right + up,
            ]
            .map(|p| p.to_array()),
            uvs.dragon_fireball,
            [[0.9, 0.85, 1.0]; 4],
            [[15.0, 15.0]; 4],
        );
    }
}

/// Half the width a shulker's bullet is drawn.
const BULLET_HALF: f32 = 0.2;

/// Half the width a dragon fireball is drawn.
const DRAGON_FIREBALL_HALF: f32 = 0.9;

/// Adds the beams from crystals to the dragon they heal: ribbons turned
/// to face the camera, a picture of the beam every two blocks.
pub fn add_crystal_beams(
    mesh: &mut Mesh,
    crystals: &[crate::entity::crystal::EndCrystal],
    uvs: &EffectUvs,
    camera: &Camera,
    alpha: f64,
) {
    let eye = camera.position;
    for crystal in crystals {
        let Some(to) = crystal.beam_to else {
            continue;
        };
        let from = (crystal.center() + DVec3::Y * f64::from(crystal.bob(alpha as f32))).as_vec3();
        let to = to.as_vec3();
        let span = to - from;
        let length = span.length();
        if length < 0.5 {
            continue;
        }
        let dir = span / length;
        let toward_camera = eye - (from + to) / 2.0;
        let side = dir.cross(toward_camera).normalize_or_zero() * BEAM_HALF;
        let segments = (length / BEAM_SEGMENT).ceil().max(1.0) as usize;
        for i in 0..segments {
            let a = from + dir * (i as f32 * BEAM_SEGMENT);
            let b = from + dir * ((i + 1) as f32 * BEAM_SEGMENT).min(length);
            mesh.quad(
                [a - side, a + side, b + side, b - side].map(|p| p.to_array()),
                uvs.crystal_beam,
                [[1.0; 3]; 4],
                [[15.0, 15.0]; 4],
            );
        }
    }
}

/// Half the width of a crystal's beam, and how long one picture of it is.
const BEAM_HALF: f32 = 0.35;
const BEAM_SEGMENT: f32 = 2.0;

/// Adds the beams of light that stream out of a dying dragon's body and
/// grow over its 200 ticks of death (modern `DragonDeathPhase`'s rays):
/// ribbons turned to the camera, up to sixty in the last tick.
pub fn add_death_beams(
    mesh: &mut Mesh,
    dragon: &crate::entity::dragon::EnderDragon,
    uvs: &EffectUvs,
    camera: &Camera,
    alpha: f64,
) {
    if !dragon.in_death_sequence() {
        return;
    }
    let f = ((f64::from(dragon.death_ticks) + alpha)
        / f64::from(crate::entity::dragon::DEATH_TICKS))
    .min(1.0) as f32;
    let count = ((f + f * f) / 2.0 * 60.0) as usize;
    let from = dragon.position_between_ticks(alpha).as_vec3() + Vec3::Y * 2.0;
    // Each ray has its own direction and length, fixed by its number.
    let hash = |i: usize, k: f32| {
        ((i as f32 * 12.9898 + k * 78.233).sin() * 43758.547)
            .fract()
            .abs()
    };
    for i in 0..count {
        let dir = Vec3::new(
            hash(i, 1.0) * 2.0 - 1.0,
            hash(i, 2.0) * 2.0 - 1.0,
            hash(i, 3.0) * 2.0 - 1.0,
        )
        .normalize_or(Vec3::Y);
        let length = (hash(i, 4.0) * 20.0 + 5.0 + hash(i, 5.0) * 2.0) * f;
        let to = from + dir * length;
        let toward_camera = camera.position - (from + to) / 2.0;
        let side = dir.cross(toward_camera).normalize_or_zero() * DEATH_BEAM_HALF * f.max(0.3);
        mesh.quad(
            [from - side, from + side, to + side, to - side].map(|p| p.to_array()),
            uvs.crystal_beam,
            [[1.0; 3]; 4],
            [[15.0, 15.0]; 4],
        );
    }
}

/// Half the width of a ray of a dying dragon's light.
const DEATH_BEAM_HALF: f32 = 0.4;

/// Adds the pillar of light over each opened End gateway: four sides, from
/// the gateway up to the top of the world.
pub fn add_gateway_beams(mesh: &mut Mesh, gateways: &[IVec3], uvs: &EffectUvs, camera: &Camera) {
    let top = crate::config::WORLD_HEIGHT as f32;
    for at in gateways {
        let center = Vec3::new(at.x as f32 + 0.5, 0.0, at.z as f32 + 0.5);
        // Skip beams on the far side of the world.
        if (center - camera.position).length() > 400.0 {
            continue;
        }
        let bottom = at.y as f32 + 0.5;
        let h = GATEWAY_BEAM_HALF;
        let corners = [
            Vec3::new(-h, 0.0, -h),
            Vec3::new(h, 0.0, -h),
            Vec3::new(h, 0.0, h),
            Vec3::new(-h, 0.0, h),
        ];
        for i in 0..4 {
            let (p, q) = (corners[i], corners[(i + 1) % 4]);
            let at_y = |c: Vec3, y: f32| center + c + Vec3::Y * y;
            let mut quad = [at_y(q, bottom), at_y(p, bottom), at_y(p, top), at_y(q, top)];
            // Wound to face away from the middle.
            let normal = (quad[1] - quad[0]).cross(quad[2] - quad[1]);
            let outward = (quad[0] + quad[2]) / 2.0 - Vec3::new(center.x, quad[0].y, center.z);
            if normal.dot(outward) < 0.0 {
                quad = [quad[1], quad[0], quad[3], quad[2]];
            }
            mesh.quad(
                quad.map(|c| c.to_array()),
                uvs.gateway_beam,
                [[1.0; 3]; 4],
                [[15.0, 15.0]; 4],
            );
        }
    }
}

/// Half the width of a gateway's beam.
const GATEWAY_BEAM_HALF: f32 = 0.3;

/// Half a fireball's drawn width, and a blaze's small one's.
const FIREBALL_HALF: f32 = 0.75;
const SMALL_FIREBALL_HALF: f32 = 0.25;

/// Adds falling blocks: whole blocks with their faces' textures, lit by
/// the light where they are.
pub fn add_falling_blocks(
    mesh: &mut Mesh,
    blocks: &[FallingBlock],
    models: &BlockModels,
    alpha: f64,
    light_at: impl Fn(DVec3) -> (u8, u8),
) {
    for block in blocks {
        let bottom = block.between_ticks(alpha);
        let (sky, lit) = light_at(bottom + DVec3::Y * 0.5);
        let light = [sky as f32, lit as f32];
        let corner = (bottom - DVec3::new(0.5, 0.0, 0.5)).as_vec3();
        let model = models.get(block.state);
        for face in Face::ALL {
            let texture = model.faces[face.index()];
            let color = models.icon_color(texture.tint).map(|c| c * face.shade());
            let corners = face_corners(face).map(|c| (corner + Vec3::from(c)).to_array());
            mesh.quad(corners, texture.uv, [color; 4], [light; 4]);
        }
    }
}

/// Adds the block a chest, hopper or TNT minecart carries, three
/// quarters size, turned with the cart.
pub fn add_cart_blocks(
    mesh: &mut Mesh,
    carts: &[Minecart],
    models: &BlockModels,
    alpha: f64,
    light_at: impl Fn(DVec3) -> (u8, u8),
) {
    for cart in carts {
        let id = match cart.kind {
            CartKind::Chest => blocks::CHEST,
            CartKind::Hopper => blocks::HOPPER,
            CartKind::Tnt => blocks::TNT,
            CartKind::Rideable => continue,
        };
        let model = models.get(BlockState::of(id));
        let base = cart.between_ticks(alpha);
        let (sky, lit) = light_at(base + DVec3::Y * 0.5);
        let light = [sky as f32, lit as f32];
        let middle = (base + DVec3::Y * 0.47).as_vec3();
        let turn = glam::Mat3::from_rotation_y(std::f32::consts::PI - cart.yaw);
        let flash = if cart.kind == CartKind::Tnt && cart.fuse >= 0 && (cart.fuse / 5) % 2 == 0 {
            2.5
        } else {
            1.0
        };
        for face in Face::ALL {
            let texture = model.faces[face.index()];
            let color = models
                .icon_color(texture.tint)
                .map(|c| c * face.shade() * flash);
            let corners = face_corners(face)
                .map(|c| (middle + turn * ((Vec3::from(c) - Vec3::splat(0.5)) * 0.75)).to_array());
            mesh.quad(corners, texture.uv, [color; 4], [light; 4]);
        }
    }
}

/// Adds lit TNT: the block, swelling and flashing white as it burns down.
pub fn add_primed_tnt(
    mesh: &mut Mesh,
    primed: &[PrimedTnt],
    models: &BlockModels,
    alpha: f64,
    light_at: impl Fn(DVec3) -> (u8, u8),
) {
    let model = models.get(BlockState::of(blocks::TNT));
    for tnt in primed {
        let bottom = tnt.between_ticks(alpha);
        let (sky, lit) = light_at(bottom + DVec3::Y * 0.5);
        let light = [sky as f32, lit as f32];
        let scale = tnt.scale(alpha as f32);
        let middle = (bottom + DVec3::Y * 0.5).as_vec3();
        let flash = if tnt.flashing() { 2.5 } else { 1.0 };
        for face in Face::ALL {
            let texture = model.faces[face.index()];
            let color = models
                .icon_color(texture.tint)
                .map(|c| c * face.shade() * flash);
            let corners = face_corners(face)
                .map(|c| (middle + (Vec3::from(c) - Vec3::splat(0.5)) * scale).to_array());
            mesh.quad(corners, texture.uv, [color; 4], [light; 4]);
        }
    }
}

/// 1.0.0 darkens digging particles to 60% (then tints them as their block,
/// except grass, whose particles come from its untinted bottom).
const PARTICLE_SHADE: f32 = 0.6;

/// Adds block-breaking particles: squares facing the camera, each showing
/// a quarter-size window of its block face's texture.
pub fn add_particles(
    mesh: &mut Mesh,
    particles: &[Particle],
    models: &BlockModels,
    camera: &Camera,
    alpha: f64,
    light_at: impl Fn(DVec3) -> (u8, u8),
) {
    let right = Vec3::new(camera.yaw.cos(), 0.0, camera.yaw.sin());
    let up = right.cross(camera.forward()).normalize();
    for particle in particles {
        let center = particle.position_between_ticks(alpha);
        let (sky, block) = light_at(center);
        let texture = models.get(particle.block).faces[particle.face.index()];
        let color = particle
            .tint
            .unwrap_or_else(|| models.icon_color(texture.tint))
            .map(|c| c * PARTICLE_SHADE);
        let [u0, v0, u1, v1] = texture.uv;
        let [wu, wv] = particle.window;
        let (du, dv) = (u1 - u0, v1 - v0);
        let uv = [
            u0 + du * wu,
            v0 + dv * wv,
            u0 + du * (wu + 0.25),
            v0 + dv * (wv + 0.25),
        ];
        let (r, u) = (right * particle.half_width, up * particle.half_width);
        let c = center.as_vec3();
        let corners = [c - r - u, c + r - u, c + r + u, c - r + u];
        mesh.quad(
            corners.map(|p| p.to_array()),
            uv,
            [color; 4],
            [[sky as f32, block as f32]; 4],
        );
    }
}

/// Where the combat effects' pictures are in the atlas.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct EffectUvs {
    pub crit: [f32; 4],
    pub sweep: [[f32; 4]; SWEEP_FRAMES],
    /// The experience orb's 11 pictures, small to big.
    pub orb: [[f32; 4]; ORB_ICONS],
    /// The dragon's fireball, the crystal's beam and the gateway's beam.
    pub dragon_fireball: [f32; 4],
    pub crystal_beam: [f32; 4],
    pub gateway_beam: [f32; 4],
}

/// The End's pictures that are drawn from the atlas.
pub const DRAGON_FIREBALL_TEXTURE: &str = "item/dragon_fireball";
pub const CRYSTAL_BEAM_TEXTURE: &str = "entity/endercrystal/endercrystal_beam";
pub const GATEWAY_BEAM_TEXTURE: &str = "entity/end_gateway_beam";
pub const END_TEXTURES: [&str; 3] = [
    DRAGON_FIREBALL_TEXTURE,
    CRYSTAL_BEAM_TEXTURE,
    GATEWAY_BEAM_TEXTURE,
];

pub const ORB_ICONS: usize = 11;

pub fn orb_texture(icon: usize) -> String {
    format!("entity/xp_orb_{icon}")
}

/// The textures the effects use, as atlas names.
pub const CRIT_TEXTURE: &str = "particle/critical_hit";
pub fn sweep_texture(frame: usize) -> String {
    format!("particle/sweep_{frame}")
}

/// Adds critical-hit stars and sweep arcs: squares facing the camera.
pub fn add_effects(
    mesh: &mut Mesh,
    effects: &[Effect],
    uvs: &EffectUvs,
    camera: &Camera,
    alpha: f64,
    light_at: impl Fn(DVec3) -> (u8, u8),
) {
    let right = Vec3::new(camera.yaw.cos(), 0.0, camera.yaw.sin());
    let up = right.cross(camera.forward()).normalize();
    for effect in effects {
        let center = effect.position_between_ticks(alpha);
        let (sky, block) = light_at(center);
        // Stars glow; the arc is lit by the world.
        let (uv, light) = match effect.kind {
            EffectKind::Crit => (uvs.crit, [15.0, 15.0]),
            EffectKind::Sweep { .. } => (uvs.sweep[effect.frame()], [sky as f32, block as f32]),
        };
        let (r, u) = (right * effect.half_width, up * effect.half_width);
        let c = center.as_vec3();
        mesh.quad(
            [c - r - u, c + r - u, c + r + u, c - r + u].map(|p| p.to_array()),
            uv,
            [[1.0; 3]; 4],
            [light; 4],
        );
    }
}

/// Adds experience orbs: pictures facing the camera, pulsing from yellow
/// to green as modern `ExperienceOrbRenderer` does.
pub fn add_orbs(
    mesh: &mut Mesh,
    orbs: &[XpOrb],
    uvs: &EffectUvs,
    camera: &Camera,
    alpha: f64,
    light_at: impl Fn(DVec3) -> (u8, u8),
) {
    let right = Vec3::new(camera.yaw.cos(), 0.0, camera.yaw.sin());
    let up = right.cross(camera.forward()).normalize();
    for orb in orbs {
        let center = orb.center_between_ticks(alpha);
        let (sky, block) = light_at(center);
        let pulse = (orb.age as f32 + alpha as f32 + orb.phase * 10.0) / 4.0;
        let tint = [
            (pulse.sin() + 1.0) * 0.5,
            1.0,
            ((pulse + 4.0 * std::f32::consts::FRAC_PI_3).sin() + 1.0) * 0.1,
        ];
        let half = ORB_SIZE / 2.0;
        let (r, u) = (right * half, up * half);
        let c = (center + DVec3::Y * 0.1).as_vec3();
        mesh.quad(
            [c - r - u, c + r - u, c + r + u, c - r + u].map(|p| p.to_array()),
            uvs.orb[orb.icon()],
            [tint; 4],
            // Orbs glow a little: seven more block light, as vanilla does.
            [[sky as f32, (block + 7).min(15) as f32]; 4],
        );
    }
}

/// An orb is this wide, in blocks.
const ORB_SIZE: f32 = 0.4;

fn add_cube(
    mesh: &mut Mesh,
    origin: Vec3,
    spin: f32,
    [top, front, side]: [IconFace; 3],
    light: [f32; 2],
) {
    let h = CUBE_HALF;
    let (sin, cos) = spin.sin_cos();
    let place = |[x, y, z]: [f32; 3]| {
        let p = origin + Vec3::new(x * cos - z * sin, y, x * sin + z * cos);
        p.to_array()
    };
    // Corners per face: bottom-left, bottom-right, top-right, top-left as
    // seen from outside.
    let faces = [
        (
            [[-h, h, h], [h, h, h], [h, h, -h], [-h, h, -h]],
            top,
            SHADE_TOP,
        ),
        (
            [[-h, -h, -h], [h, -h, -h], [h, -h, h], [-h, -h, h]],
            top,
            SHADE_BOTTOM,
        ),
        (
            [[-h, -h, h], [h, -h, h], [h, h, h], [-h, h, h]],
            front,
            SHADE_Z,
        ),
        (
            [[h, -h, -h], [-h, -h, -h], [-h, h, -h], [h, h, -h]],
            front,
            SHADE_Z,
        ),
        (
            [[h, -h, h], [h, -h, -h], [h, h, -h], [h, h, h]],
            side,
            SHADE_X,
        ),
        (
            [[-h, -h, -h], [-h, -h, h], [-h, h, h], [-h, h, -h]],
            side,
            SHADE_X,
        ),
    ];
    for (corners, face, shade) in faces {
        let color = face.color.map(|c| c * shade);
        mesh.quad(corners.map(place), face.uv, [color; 4], [light; 4]);
    }
}

fn add_sprite(mesh: &mut Mesh, origin: Vec3, yaw: f32, face: IconFace, light: [f32; 2]) {
    // The camera's right, so the picture faces it.
    let right = Vec3::new(yaw.cos(), 0.0, yaw.sin()) * SPRITE_HALF_WIDTH;
    let (below, above) = (Vec3::Y * SPRITE_BELOW, Vec3::Y * SPRITE_ABOVE);
    let corners = [
        origin - right - below,
        origin + right - below,
        origin + right + above,
        origin - right + above,
    ];
    mesh.quad(
        corners.map(|p| p.to_array()),
        face.uv,
        [face.color; 4],
        [light; 4],
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::blocks;

    #[test]
    fn particles_face_the_camera_even_looking_up_or_down() {
        use crate::world::{Face, biome::BiomeColors, block::BlockState};
        let blocks = crate::world::block::registry();
        let models = BlockModels::new(
            blocks,
            |_| None,
            [0.0, 0.0, 1.0, 1.0],
            BiomeColors::uniform(),
        );
        let mut random = || 0.5;
        let chip = Particle::chip(
            glam::IVec3::new(0, 64, 0),
            Face::Top,
            BlockState::of(blocks::STONE),
            &mut random,
        );
        for (yaw, pitch) in [(0.0f32, 0.0f32), (1.0, 0.8), (2.5, -1.2), (-2.0, 0.3)] {
            let camera = Camera::new(Vec3::new(0.5, 70.0, 0.5), yaw, pitch);
            let mut mesh = Mesh::default();
            add_particles(
                &mut mesh,
                std::slice::from_ref(&chip),
                &models,
                &camera,
                1.0,
                |_| (15, 0),
            );
            let p = |i: usize| Vec3::from_array(mesh.vertices[i].position);
            let normal = (p(1) - p(0)).cross(p(3) - p(0)).normalize();
            assert!(normal.dot(camera.forward()) < -0.99, "{yaw} {pitch}");
            // A quarter of the texture, darkened to 60%.
            let (u0, u1) = (mesh.vertices[0].uv[0], mesh.vertices[1].uv[0]);
            assert!(((u1 - u0) - 0.25).abs() < 1e-6);
            assert_eq!(mesh.vertices[0].color, [0.6; 3]);
        }
    }

    #[test]
    fn gateway_beams_face_outward_on_all_four_sides() {
        let uvs = EffectUvs::default();
        let camera = Camera::new(Vec3::new(0.0, 80.0, 0.0), 0.0, 0.0);
        let mut mesh = Mesh::default();
        let at = IVec3::new(20, 75, 20);
        add_gateway_beams(&mut mesh, &[at], &uvs, &camera);
        assert_eq!(mesh.indices.len(), 4 * 6);
        let center = Vec3::new(20.5, 100.0, 20.5);
        for quad in mesh.vertices.chunks(4) {
            let p: Vec<Vec3> = quad.iter().map(|v| Vec3::from(v.position)).collect();
            let normal = (p[1] - p[0]).cross(p[2] - p[1]);
            let out = (p[0] + p[2]) / 2.0 - center;
            assert!(normal.dot(Vec3::new(out.x, 0.0, out.z)) > 0.0);
        }
    }

    #[test]
    fn crystal_beams_run_from_the_crystal_to_the_dragon_and_face_the_camera() {
        let uvs = EffectUvs::default();
        let camera = Camera::new(Vec3::new(0.0, 70.0, 30.0), 0.0, 0.0);
        let mut crystal =
            crate::entity::crystal::EndCrystal::new(DVec3::new(0.0, 80.0, 0.0), false);
        let mut mesh = Mesh::default();
        add_crystal_beams(
            &mut mesh,
            std::slice::from_ref(&crystal),
            &uvs,
            &camera,
            0.0,
        );
        assert!(mesh.vertices.is_empty(), "no beam without a dragon to heal");
        crystal.beam_to = Some(DVec3::new(0.0, 80.0, -10.0));
        add_crystal_beams(
            &mut mesh,
            std::slice::from_ref(&crystal),
            &uvs,
            &camera,
            0.0,
        );
        assert!(mesh.vertices.len() >= 4);
        for quad in mesh.vertices.chunks(4) {
            let p: Vec<Vec3> = quad.iter().map(|v| Vec3::from(v.position)).collect();
            let normal = (p[1] - p[0]).cross(p[2] - p[1]);
            assert!(normal.dot(camera.position - p[0]) > 0.0, "faces the camera");
        }
    }

    #[test]
    fn sprites_face_the_camera() {
        let face = IconFace {
            uv: [0.0, 0.0, 1.0, 1.0],
            color: [1.0; 3],
        };
        for yaw in [0.0f32, 1.0, 2.5, -2.0] {
            let mut mesh = Mesh::default();
            add_sprite(&mut mesh, Vec3::ZERO, yaw, face, [15.0, 0.0]);
            let p = |i: usize| Vec3::from_array(mesh.vertices[i].position);
            let normal = (p(1) - p(0)).cross(p(3) - p(0)).normalize();
            let forward = Vec3::new(yaw.sin(), 0.0, -yaw.cos());
            assert!(
                normal.dot(forward) < -0.99,
                "front faces back at the camera"
            );
        }
    }

    #[test]
    fn cube_faces_point_outward() {
        let face = IconFace {
            uv: [0.0, 0.0, 1.0, 1.0],
            color: [1.0; 3],
        };
        let mut mesh = Mesh::default();
        add_cube(&mut mesh, Vec3::ZERO, 0.7, [face; 3], [15.0, 0.0]);
        for quad in mesh.vertices.chunks(4) {
            let p = |i: usize| Vec3::from_array(quad[i].position);
            let normal = (p(1) - p(0)).cross(p(3) - p(0));
            let middle = (p(0) + p(2)) / 2.0;
            assert!(normal.dot(middle) > 0.0);
        }
        assert_eq!(mesh.vertices.len(), 24);
    }
}
