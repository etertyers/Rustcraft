//! Meshes for dropped items, rebuilt every frame, following 1.0.0's
//! `RenderItem`: blocks are spinning quarter-size cubes, other items are
//! half-size pictures turned toward the camera, and both bob up and down.

use glam::{DVec3, Vec3};

use super::{
    camera::Camera,
    icons::{IconFace, ItemIcon, ItemIcons},
};
use crate::{entity::item::ItemEntity, world::mesh::Mesh};

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
