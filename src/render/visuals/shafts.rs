//! Light shafts (M57): the march through the shadow maps (quarter size on
//! Medium, half size on High, 8 or 16 steps) into a small picture, a 4 × 4
//! blur, and the picture the post pass scales up and adds. The march reads
//! the opaque scene's depth (a copy) and the Trailer uniform's shadow
//! maps, so it draws with the same group 3 as the blocks.

use super::{Quality, profiler::GpuProfiler};

/// Steps of the march and the frame's pixels one of its pixels covers.
pub fn settings(quality: Quality) -> Option<(u32, u32)> {
    match quality {
        Quality::Low => None,
        Quality::Medium => Some((8, 4)),
        Quality::High => Some((16, 2)),
    }
}

/// The farthest a shaft looks, in blocks, and its scattering per block.
pub const REACH: f32 = 96.0;
pub const SCATTER: f32 = 0.0045;

pub const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R16Float;

struct Picture {
    view: wgpu::TextureView,
    _texture: wgpu::Texture,
}

pub struct Shafts {
    march: wgpu::RenderPipeline,
    blur: wgpu::RenderPipeline,
    blur_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    march_picture: Option<Picture>,
    blur_picture: Option<Picture>,
    blur_group: Option<wgpu::BindGroup>,
    size: (u32, u32),
}

impl Shafts {
    /// `march_layout` is the Trailer block pipelines' layout (globals,
    /// two textures, group 3); `march_shader` the shafts shader.
    pub fn new(
        device: &wgpu::Device,
        march_layout: &wgpu::PipelineLayout,
        march_shader: &wgpu::ShaderModule,
    ) -> Self {
        let blur_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("shafts blur"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let blur_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("shafts blur"),
            bind_group_layouts: &[Some(&blur_layout)],
            immediate_size: 0,
        });
        let blur_shader = device.create_shader_module(wgpu::include_wgsl!("shafts_blur.wgsl"));
        let make = |layout: &wgpu::PipelineLayout, shader: &wgpu::ShaderModule| {
            super::frame::fullscreen_pipeline(device, layout, shader, "shafts", FORMAT)
        };
        Self {
            march: make(march_layout, march_shader),
            blur: make(&blur_pipeline_layout, &blur_shader),
            blur_layout,
            sampler: device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("shafts"),
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                ..Default::default()
            }),
            march_picture: None,
            blur_picture: None,
            blur_group: None,
            size: (0, 0),
        }
    }

    /// The smoothed picture, once `run` has drawn it.
    pub fn result(&self) -> Option<&wgpu::TextureView> {
        self.blur_picture.as_ref().map(|p| &p.view)
    }

    /// The pictures for a frame of `frame` pixels, each `scale` times
    /// smaller; made again when the size changes.
    pub fn prepare(&mut self, device: &wgpu::Device, frame: (u32, u32), scale: u32) {
        let size = ((frame.0 / scale).max(1), (frame.1 / scale).max(1));
        if self.size == size && self.march_picture.is_some() {
            return;
        }
        self.size = size;
        let make = || {
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("shafts"),
                size: wgpu::Extent3d {
                    width: size.0,
                    height: size.1,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            Picture {
                view: texture.create_view(&Default::default()),
                _texture: texture,
            }
        };
        let march = make();
        self.blur_group = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("shafts blur"),
            layout: &self.blur_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&march.view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        }));
        self.march_picture = Some(march);
        self.blur_picture = Some(make());
    }

    /// The march, then the blur. `groups` are the block pipelines' groups
    /// 0 to 3 (globals, atlas, glint, the sun and scene).
    pub fn run(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        groups: [&wgpu::BindGroup; 4],
        mut profiler: Option<&mut GpuProfiler>,
    ) {
        let (Some(march), Some(blur), Some(blur_group)) =
            (&self.march_picture, &self.blur_picture, &self.blur_group)
        else {
            return;
        };
        let ts = profiler.as_mut().and_then(|p| p.pass("shafts"));
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("shafts"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &march.view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                timestamp_writes: ts,
                ..Default::default()
            });
            pass.set_pipeline(&self.march);
            for (i, group) in groups.iter().enumerate() {
                pass.set_bind_group(i as u32, *group, &[]);
            }
            pass.draw(0..3, 0..1);
        }
        let ts = profiler.as_mut().and_then(|p| p.pass("shafts"));
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("shafts blur"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &blur.view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            timestamp_writes: ts,
            ..Default::default()
        });
        pass.set_pipeline(&self.blur);
        pass.set_bind_group(0, blur_group, &[]);
        pass.draw(0..3, 0..1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn low_has_no_shafts_medium_a_quarter_size_with_8_steps_high_half_with_16() {
        assert_eq!(settings(Quality::Low), None);
        assert_eq!(settings(Quality::Medium), Some((8, 4)));
        assert_eq!(settings(Quality::High), Some((16, 2)));
    }
}
