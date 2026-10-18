//! The wireframe boxes around the block the player is looking at: its
//! outline shape, which may be several boxes (stairs, fences).

use glam::{IVec3, Vec3};

use crate::world::shape::Cuboid;

/// Minecraft grows the outline by this much so it sits just outside the block.
const GROW: f32 = 0.002;

/// 12 edges, two vertices each.
const BOX_VERTICES: u32 = 24;
/// Most boxes outlined at once (a wall post with four sides is five).
const MAX_BOXES: usize = 8;

pub struct Outline {
    pipeline: wgpu::RenderPipeline,
    vertices: wgpu::Buffer,
    /// The block outlined in the next frame, and its outline boxes.
    target: Option<(IVec3, Vec<Cuboid>)>,
}

impl Outline {
    pub fn new(
        device: &wgpu::Device,
        globals_layout: &wgpu::BindGroupLayout,
        format: wgpu::TextureFormat,
        depth_format: wgpu::TextureFormat,
    ) -> Self {
        let shader = device.create_shader_module(wgpu::include_wgsl!("outline.wgsl"));
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("outline"),
            bind_group_layouts: &[Some(globals_layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("outline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<[f32; 3]>() as wgpu::BufferAddress,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x3],
                })],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::LineList,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: depth_format,
                depth_write_enabled: Some(false),
                depth_compare: Some(wgpu::CompareFunction::LessEqual),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let vertices = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("outline vertices"),
            size: (BOX_VERTICES as usize * MAX_BOXES * std::mem::size_of::<[f32; 3]>())
                as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self {
            pipeline,
            vertices,
            target: None,
        }
    }

    pub fn set_target(&mut self, target: Option<(IVec3, Vec<Cuboid>)>) {
        self.target = target;
    }

    /// Uploads this frame's boxes; returns how many vertices to draw (0 if
    /// there's nothing to outline).
    pub fn prepare(&self, queue: &wgpu::Queue) -> u32 {
        let Some((block, boxes)) = &self.target else {
            return 0;
        };
        let corner = block.as_vec3();
        let vertices: Vec<[f32; 3]> = boxes
            .iter()
            .take(MAX_BOXES)
            .flat_map(|b| {
                let min = corner + Vec3::from(b.min) - GROW;
                let max = corner + Vec3::from(b.max) + GROW;
                box_edges(min, max)
            })
            .collect();
        queue.write_buffer(&self.vertices, 0, bytemuck::cast_slice(&vertices));
        vertices.len() as u32
    }

    /// Draws `vertices` uploaded by `prepare`. Bind group 0 must hold the globals.
    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>, vertices: u32) {
        pass.set_pipeline(&self.pipeline);
        pass.set_vertex_buffer(0, self.vertices.slice(..));
        pass.draw(0..vertices, 0..1);
    }
}

/// Line-list vertices for the 12 edges of a box.
fn box_edges(min: Vec3, max: Vec3) -> [[f32; 3]; BOX_VERTICES as usize] {
    let corner = |i: usize| {
        [
            if i & 1 == 0 { min.x } else { max.x },
            if i & 2 == 0 { min.y } else { max.y },
            if i & 4 == 0 { min.z } else { max.z },
        ]
    };
    let mut out = [[0.0; 3]; BOX_VERTICES as usize];
    let mut n = 0;
    for a in 0..8 {
        // Each edge joins corners that differ in one bit; list it from the lower one.
        for bit in [1, 2, 4] {
            if a & bit == 0 {
                out[n] = corner(a);
                out[n + 1] = corner(a | bit);
                n += 2;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn box_edges_are_the_twelve_unit_edges() {
        let edges = box_edges(Vec3::ZERO, Vec3::ONE);
        for pair in edges.chunks(2) {
            let (a, b) = (Vec3::from(pair[0]), Vec3::from(pair[1]));
            assert_eq!((b - a).length(), 1.0, "{a} -> {b}");
        }
        let unique: std::collections::HashSet<_> =
            edges.chunks(2).map(|pair| format!("{:?}", pair)).collect();
        assert_eq!(unique.len(), 12);
    }
}
