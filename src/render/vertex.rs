//! The vertex format shared by the mesher and the block shader.

use bytemuck::{Pod, Zeroable};

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub struct Vertex {
    pub position: [f32; 3],
    pub uv: [f32; 2],
    /// Tint × face shade × ambient occlusion in sRGB space, multiplied with the texture color.
    pub color: [f32; 3],
    /// Sky and block light, 0–15, interpolated across the face.
    pub light: [f32; 2],
}

impl Vertex {
    const ATTRIBUTES: [wgpu::VertexAttribute; 4] = wgpu::vertex_attr_array![
        0 => Float32x3,
        1 => Float32x2,
        2 => Float32x3,
        3 => Float32x2
    ];

    pub fn layout() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &Self::ATTRIBUTES,
        }
    }
}

/// What the Trailer look's shaders know about a vertex's face (M54), packed
/// into one u32 kept beside the vertices (`Mesh::faces`, a second vertex
/// buffer, so `Vertex` and the Vanilla look's pipelines stay as M53 had
/// them): bits 0–15 the face normal (octahedral, 8 bits a side), 16–23
/// the material class, 24–31 flags (`FLAG_*`, and the emissive strength
/// 0–15 in the top four bits).
pub const FACE_ATTRIBUTE: [wgpu::VertexAttribute; 1] = wgpu::vertex_attr_array![4 => Uint32];

/// The face buffer's layout (slot 1).
pub fn face_layout() -> wgpu::VertexBufferLayout<'static> {
    wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<u32>() as wgpu::BufferAddress,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &FACE_ATTRIBUTE,
    }
}

/// The vertex is at the top of its block (plants bend there; the bottom
/// stays put).
pub const FLAG_TOP: u8 = 1;
/// The top face of still water (waves move it).
pub const FLAG_STILL_WATER_TOP: u8 = 2;
/// Fast graphics' solid leaves.
pub const FLAG_SOLID: u8 = 4;

/// Normal, class and flags in one u32.
pub fn pack_face(normal: [f32; 3], class: u8, flags: u8) -> u32 {
    let [x, y] = octahedral(normal);
    let q = |v: f32| ((v * 0.5 + 0.5) * 255.0).round().clamp(0.0, 255.0) as u32;
    q(x) | (q(y) << 8) | (u32::from(class) << 16) | (u32::from(flags) << 24)
}

/// The normal (unit length), class and flags back.
pub fn unpack_face(packed: u32) -> ([f32; 3], u8, u8) {
    let d = |bits: u32| (bits & 0xff) as f32 / 255.0 * 2.0 - 1.0;
    let normal = from_octahedral([d(packed), d(packed >> 8)]);
    (normal, (packed >> 16) as u8, (packed >> 24) as u8)
}

/// A unit vector folded onto the octahedron and flattened to [-1, 1]².
fn octahedral(n: [f32; 3]) -> [f32; 2] {
    let sum = n[0].abs() + n[1].abs() + n[2].abs();
    if sum == 0.0 {
        return [0.0, 1.0];
    }
    let (x, y, z) = (n[0] / sum, n[1] / sum, n[2] / sum);
    if z >= 0.0 {
        [x, y]
    } else {
        let sx = if x >= 0.0 { 1.0 } else { -1.0 };
        let sy = if y >= 0.0 { 1.0 } else { -1.0 };
        [(1.0 - y.abs()) * sx, (1.0 - x.abs()) * sy]
    }
}

fn from_octahedral([x, y]: [f32; 2]) -> [f32; 3] {
    let z = 1.0 - x.abs() - y.abs();
    let t = (-z).max(0.0);
    let x = x + if x >= 0.0 { -t } else { t };
    let y = y + if y >= 0.0 { -t } else { t };
    let len = (x * x + y * y + z * z).sqrt();
    [x / len, y / len, z / len]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::materials::Class;

    #[test]
    fn every_face_direction_and_class_round_trips() {
        let s = std::f32::consts::FRAC_1_SQRT_2;
        let normals = [
            [1.0, 0.0, 0.0],
            [-1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, -1.0, 0.0],
            [0.0, 0.0, 1.0],
            [0.0, 0.0, -1.0],
            // Cross plants' diagonal planes.
            [s, 0.0, s],
            [-s, 0.0, s],
            [s, 0.0, -s],
            [-s, 0.0, -s],
        ];
        for class in Class::ALL {
            for flags in [
                0,
                FLAG_TOP,
                FLAG_STILL_WATER_TOP | FLAG_SOLID,
                0xf0 | FLAG_TOP,
            ] {
                for n in normals {
                    let (back, c, f) = unpack_face(pack_face(n, class as u8, flags));
                    let dot = back[0] * n[0] + back[1] * n[1] + back[2] * n[2];
                    assert!(dot > 0.999, "{n:?} came back as {back:?}");
                    assert_eq!((c, f), (class as u8, flags));
                }
            }
        }
    }
}
