//! The crack over a block being mined (1.0.0's `drawBlockBreaking`): the
//! block's box drawn with one of the ten `destroy_stage` textures, which
//! the crack pipeline multiplies into what's already on screen.

use glam::Vec3;

use crate::world::mesh::Mesh;

/// Stages of cracking, `destroy_stage_0` to `destroy_stage_9`.
pub const STAGES: usize = 10;

/// The crack sits this far outside the block so it isn't hidden by it.
const GROW: f32 = 0.002;

/// Which crack texture shows at mining `progress` (0–1).
pub fn stage(progress: f32) -> usize {
    ((progress * STAGES as f32) as usize).min(STAGES - 1)
}

/// The six faces of the box from `min` to `max`, textured with `uv`.
pub fn crack_mesh(min: Vec3, max: Vec3, uv: [f32; 4]) -> Mesh {
    let (a, b) = (min - Vec3::splat(GROW), max + Vec3::splat(GROW));
    let corner = |x: bool, y: bool, z: bool| {
        [
            if x { b.x } else { a.x },
            if y { b.y } else { a.y },
            if z { b.z } else { a.z },
        ]
    };
    // Each face counter-clockwise from outside, bottom-left first.
    let faces = [
        [(0, 1, 1), (1, 1, 1), (1, 1, 0), (0, 1, 0)],
        [(0, 0, 0), (1, 0, 0), (1, 0, 1), (0, 0, 1)],
        [(1, 0, 0), (0, 0, 0), (0, 1, 0), (1, 1, 0)],
        [(0, 0, 1), (1, 0, 1), (1, 1, 1), (0, 1, 1)],
        [(1, 0, 1), (1, 0, 0), (1, 1, 0), (1, 1, 1)],
        [(0, 0, 0), (0, 0, 1), (0, 1, 1), (0, 1, 0)],
    ];
    let mut mesh = Mesh::default();
    for face in faces {
        let corners = face.map(|(x, y, z)| corner(x == 1, y == 1, z == 1));
        mesh.quad(corners, uv, [[1.0; 3]; 4], [[15.0, 15.0]; 4]);
    }
    mesh
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_crack_grows_through_ten_stages_and_wraps_the_block() {
        assert_eq!(stage(0.0), 0);
        assert_eq!(stage(0.55), 5);
        assert_eq!(stage(0.999), 9);
        assert_eq!(stage(1.0), 9);
        let mesh = crack_mesh(
            Vec3::new(2.0, 64.0, 3.0),
            Vec3::new(3.0, 65.0, 4.0),
            [0.0; 4],
        );
        assert_eq!(mesh.indices.len(), 6 * 6);
        let lowest = mesh
            .vertices
            .iter()
            .map(|v| v.position[1])
            .fold(f32::MAX, f32::min);
        assert!(lowest < 64.0 && lowest > 63.99);
    }
}
