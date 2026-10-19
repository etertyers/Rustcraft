//! Meshes for dropped items, rebuilt every frame, following 1.0.0's
//! `RenderItem`: blocks are spinning quarter-size cubes, other items are
//! half-size pictures turned toward the camera, and both bob up and down.

use glam::{DVec3, Vec3};

use super::{
    camera::Camera,
    icons::{IconFace, ItemIcon, ItemIcons},
};
use crate::{
    entity::{
        effects::{Effect, EffectKind, SWEEP_FRAMES},
        falling::FallingBlock,
        item::ItemEntity,
        particle::Particle,
    },
    world::{
        Face,
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
        let color = models.icon_color(texture.tint).map(|c| c * PARTICLE_SHADE);
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
