//! Mesh for a single unit cube, used until chunk meshing exists (M2).

use bytemuck::{Pod, Zeroable};

use crate::world::Face;

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub struct Vertex {
    pub position: [f32; 3],
    pub uv: [f32; 2],
    /// Tint × face shade in sRGB space, multiplied with the texture color.
    pub color: [f32; 3],
}

impl Vertex {
    const ATTRIBUTES: [wgpu::VertexAttribute; 3] =
        wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x2, 2 => Float32x3];

    pub fn layout() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &Self::ATTRIBUTES,
        }
    }
}

/// Corners of a unit cube face, counter-clockwise seen from outside,
/// starting at the texture's bottom-left.
fn corners(face: Face) -> [[f32; 3]; 4] {
    match face {
        Face::Top => [[0., 1., 1.], [1., 1., 1.], [1., 1., 0.], [0., 1., 0.]],
        Face::Bottom => [[0., 0., 0.], [1., 0., 0.], [1., 0., 1.], [0., 0., 1.]],
        Face::North => [[1., 0., 0.], [0., 0., 0.], [0., 1., 0.], [1., 1., 0.]],
        Face::South => [[0., 0., 1.], [1., 0., 1.], [1., 1., 1.], [0., 1., 1.]],
        Face::East => [[1., 0., 1.], [1., 0., 0.], [1., 1., 0.], [1., 1., 1.]],
        Face::West => [[0., 0., 0.], [0., 0., 1.], [0., 1., 1.], [0., 1., 0.]],
    }
}

/// How one face of a cube is drawn.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FaceTexture {
    /// Atlas UV rect `[u0, v0, u1, v1]`, v0 at the top.
    pub uv: [f32; 4],
    /// sRGB color multiplied with the texture (white for none).
    pub tint: [f32; 3],
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct Mesh {
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u16>,
}

/// Builds a cube with its minimum corner at `origin`, textured per face by `texture`.
pub fn cube_mesh(origin: [f32; 3], texture: impl Fn(Face) -> FaceTexture) -> Mesh {
    let mut vertices = Vec::with_capacity(24);
    let mut indices = Vec::with_capacity(36);
    for face in Face::ALL {
        let base = vertices.len() as u16;
        let FaceTexture {
            uv: [u0, v0, u1, v1],
            tint,
        } = texture(face);
        let color = tint.map(|c| c * face.shade());
        let uvs = [[u0, v1], [u1, v1], [u1, v0], [u0, v0]];
        for (corner, uv) in corners(face).into_iter().zip(uvs) {
            vertices.push(Vertex {
                position: [
                    origin[0] + corner[0],
                    origin[1] + corner[1],
                    origin[2] + corner[2],
                ],
                uv,
                color,
            });
        }
        indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    Mesh { vertices, indices }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Vec3;

    const PLAIN: FaceTexture = FaceTexture {
        uv: [0.0, 0.0, 1.0, 1.0],
        tint: [1.0; 3],
    };

    #[test]
    fn cube_has_six_quads() {
        let Mesh { vertices, indices } = cube_mesh([0.0; 3], |_| PLAIN);
        assert_eq!(vertices.len(), 24);
        assert_eq!(indices.len(), 36);
    }

    #[test]
    fn faces_get_their_own_uvs_and_shaded_tint() {
        let mesh = cube_mesh([0.0; 3], |face| FaceTexture {
            uv: if face == Face::Top {
                [0.5, 0.5, 1.0, 1.0]
            } else {
                [0.0, 0.0, 0.5, 0.5]
            },
            tint: [0.5, 1.0, 1.0],
        });
        let top = &mesh.vertices[0..4];
        assert!(top.iter().all(|v| v.position[1] == 1.0 && v.uv[0] >= 0.5));
        let bottom = &mesh.vertices[4..8];
        assert!(
            bottom
                .iter()
                .all(|v| v.uv[0] <= 0.5 && v.color == [0.25, 0.5, 0.5])
        );
    }

    #[test]
    fn side_textures_are_upright() {
        // The texture's top edge (v0) must be on the face's top edge (y = 1).
        let mesh = cube_mesh([0.0; 3], |_| PLAIN);
        for face in mesh.vertices[8..].chunks(4) {
            for v in face {
                assert_eq!(v.uv[1] == 0.0, v.position[1] == 1.0, "{v:?}");
            }
        }
    }

    #[test]
    fn every_triangle_winds_counter_clockwise_from_outside() {
        let Mesh { vertices, indices } = cube_mesh([0.0; 3], |_| PLAIN);
        let center = Vec3::splat(0.5);
        for tri in indices.chunks(3) {
            let [a, b, c] = [0, 1, 2].map(|i| Vec3::from(vertices[tri[i] as usize].position));
            let normal = (b - a).cross(c - a);
            let outward = (a + b + c) / 3.0 - center;
            assert!(normal.dot(outward) > 0.0, "triangle {tri:?} faces inward");
        }
    }
}
