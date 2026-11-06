//! Drawing distant terrain (M69): one vertex and index buffer per tile.

use std::collections::HashMap;

use glam::Vec3;
use wgpu::util::DeviceExt;

use super::frustum::Frustum;
use crate::lod::data::TileKey;
use crate::lod::mesh::{LodMesh, LodVertex};

/// How far down LOD vertices near the loaded edge sit, so the real
/// terrain drawn over them never fights their depth.
pub const EDGE_BIAS: f32 = 1.5;

struct TileBuffers {
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    count: u32,
}

pub struct LodDraw {
    tiles: HashMap<TileKey, TileBuffers>,
    uniform: wgpu::Buffer,
    pub bind_group: wgpu::BindGroup,
    /// Tiles to draw, set by the game each frame.
    visible: Vec<TileKey>,
}

impl LodDraw {
    pub fn layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
        device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("lod uniform"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        })
    }

    pub fn new(device: &wgpu::Device, layout: &wgpu::BindGroupLayout) -> Self {
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("lod uniform"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("lod uniform"),
            layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform.as_entire_binding(),
            }],
        });
        Self {
            tiles: HashMap::new(),
            uniform,
            bind_group,
            visible: Vec::new(),
        }
    }

    pub fn vertex_layout() -> wgpu::VertexBufferLayout<'static> {
        const ATTRS: [wgpu::VertexAttribute; 2] =
            wgpu::vertex_attr_array![0 => Float32x3, 1 => Unorm8x4];
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<LodVertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &ATTRS,
        }
    }

    pub fn upload(&mut self, device: &wgpu::Device, key: TileKey, mesh: &LodMesh) {
        if mesh.indices.is_empty() {
            self.tiles.remove(&key);
            return;
        }
        let vertices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("lod vertices"),
            contents: bytemuck::cast_slice(&mesh.vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let indices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("lod indices"),
            contents: bytemuck::cast_slice(&mesh.indices),
            usage: wgpu::BufferUsages::INDEX,
        });
        self.tiles.insert(
            key,
            TileBuffers {
                vertices,
                indices,
                count: mesh.indices.len() as u32,
            },
        );
    }

    pub fn remove(&mut self, key: TileKey) {
        self.tiles.remove(&key);
    }

    pub fn clear(&mut self) {
        self.tiles.clear();
        self.visible.clear();
    }

    pub fn len(&self) -> usize {
        self.tiles.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tiles.is_empty()
    }

    pub fn set_visible(&mut self, keys: impl IntoIterator<Item = TileKey>) {
        self.visible.clear();
        self.visible.extend(keys);
    }

    pub fn write_uniform(&self, queue: &wgpu::Queue, edge: f32) {
        queue.write_buffer(
            &self.uniform,
            0,
            bytemuck::cast_slice(&[edge, EDGE_BIAS, 0.0, 0.0]),
        );
    }

    /// Draws the visible tiles inside the frustum. The caller has set the
    /// pipeline and group 0.
    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>, frustum: &Frustum) -> (usize, u64) {
        pass.set_bind_group(1, &self.bind_group, &[]);
        let (mut drawn, mut triangles) = (0, 0);
        for key in &self.visible {
            let Some(tile) = self.tiles.get(key) else {
                continue;
            };
            let (x, z) = key.origin();
            let span = key.span() as f32;
            let min = Vec3::new(x as f32, crate::config::MIN_Y as f32, z as f32);
            let max = Vec3::new(
                x as f32 + span,
                crate::config::MAX_Y as f32,
                z as f32 + span,
            );
            if !frustum.intersects_box(min, max) {
                continue;
            }
            pass.set_vertex_buffer(0, tile.vertices.slice(..));
            pass.set_index_buffer(tile.indices.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..tile.count, 0, 0..1);
            drawn += 1;
            triangles += u64::from(tile.count) / 3;
        }
        (drawn, triangles)
    }
}
