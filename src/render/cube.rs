//! Mesh for a single unit cube, used until chunk meshing exists (M2).

use bytemuck::{Pod, Zeroable};

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub struct Vertex {
    pub position: [f32; 3],
    pub color: [f32; 3],
}

impl Vertex {
    const ATTRIBUTES: [wgpu::VertexAttribute; 2] =
        wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3];

    pub fn layout() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &Self::ATTRIBUTES,
        }
    }
}

/// A cube face: its outward normal and corners, counter-clockwise seen from outside.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Face {
    Top,
    Bottom,
    North,
    South,
    East,
    West,
}

impl Face {
    pub const ALL: [Face; 6] = [
        Face::Top,
        Face::Bottom,
        Face::North,
        Face::South,
        Face::East,
        Face::West,
    ];

    /// Corners of the unit cube face, counter-clockwise from outside,
    /// starting at the texture's bottom-left.
    fn corners(self) -> [[f32; 3]; 4] {
        match self {
            Face::Top => [[0., 1., 1.], [1., 1., 1.], [1., 1., 0.], [0., 1., 0.]],
            Face::Bottom => [[0., 0., 0.], [1., 0., 0.], [1., 0., 1.], [0., 0., 1.]],
            Face::North => [[1., 0., 0.], [0., 0., 0.], [0., 1., 0.], [1., 1., 0.]],
            Face::South => [[0., 0., 1.], [1., 0., 1.], [1., 1., 1.], [0., 1., 1.]],
            Face::East => [[1., 0., 1.], [1., 0., 0.], [1., 1., 0.], [1., 1., 1.]],
            Face::West => [[0., 0., 0.], [0., 0., 1.], [0., 1., 1.], [0., 1., 0.]],
        }
    }

    /// Minecraft's fixed directional shading: top 1.0, bottom 0.5, Z faces 0.8, X faces 0.6.
    pub fn shade(self) -> f32 {
        match self {
            Face::Top => 1.0,
            Face::Bottom => 0.5,
            Face::North | Face::South => 0.8,
            Face::East | Face::West => 0.6,
        }
    }
}

/// Builds a cube with its minimum corner at `origin`. Returns vertices and indices.
pub fn cube_mesh(origin: [f32; 3]) -> (Vec<Vertex>, Vec<u16>) {
    let mut vertices = Vec::with_capacity(24);
    let mut indices = Vec::with_capacity(36);
    for face in Face::ALL {
        let base = vertices.len() as u16;
        let s = face.shade();
        for corner in face.corners() {
            vertices.push(Vertex {
                position: [
                    origin[0] + corner[0],
                    origin[1] + corner[1],
                    origin[2] + corner[2],
                ],
                color: [s, s, s],
            });
        }
        indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    (vertices, indices)
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Vec3;

    #[test]
    fn cube_has_six_quads() {
        let (vertices, indices) = cube_mesh([0.0; 3]);
        assert_eq!(vertices.len(), 24);
        assert_eq!(indices.len(), 36);
    }

    #[test]
    fn every_triangle_winds_counter_clockwise_from_outside() {
        let (vertices, indices) = cube_mesh([0.0; 3]);
        let center = Vec3::splat(0.5);
        for tri in indices.chunks(3) {
            let [a, b, c] = [0, 1, 2].map(|i| Vec3::from(vertices[tri[i] as usize].position));
            let normal = (b - a).cross(c - a);
            let outward = (a + b + c) / 3.0 - center;
            assert!(normal.dot(outward) > 0.0, "triangle {tri:?} faces inward");
        }
    }
}
