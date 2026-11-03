//! Bloom (M57): a chain of levels (5 on Medium and High, 4 on Low; a
//! quarter of the frame, then each half the one before) the HDR frame is
//! filtered down through and the post pass adds them up again, so
//! light brighter than 1.0 — the sun, glowing blocks, lava, fire, glints
//! on water — spreads a soft glow. The result is the first level, which
//! the post pass adds to the picture.

use bytemuck::{Pod, Zeroable};

use super::{Quality, profiler::GpuProfiler};

/// Light dimmer than this (linear luminance) never blooms, so a plain
/// daylit scene gets no haze.
pub const THRESHOLD: f32 = 1.0;
/// How much of the glow the post pass adds.
pub const STRENGTH: f32 = 0.55;
/// What each level, largest first, counts for in the sum the post pass
/// makes (an even fall-off: each level 80% of the one above).
pub const WEIGHTS: [f32; 5] = [1.0, 0.8, 0.64, 0.51, 0.41];

/// Levels in the chain for a quality: 5 on Medium and High, 4 on Low.
pub fn levels(quality: Quality) -> usize {
    if quality == Quality::Low { 4 } else { 5 }
}

/// What the first level keeps of a linear color: the part of it above
/// `THRESHOLD` (a tested copy of the shader's `prefilter`).
pub fn prefilter(color: [f32; 3]) -> [f32; 3] {
    let l = 0.2126 * color[0] + 0.7152 * color[1] + 0.0722 * color[2];
    let keep = (l - THRESHOLD).max(0.0) / l.max(1e-4);
    color.map(|c| c * keep)
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Params {
    values: [f32; 4],
}

struct Level {
    view: wgpu::TextureView,
    size: (u32, u32),
    // Keeps the texture alive.
    _texture: wgpu::Texture,
}

pub struct Bloom {
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    down: wgpu::RenderPipeline,
    levels: Vec<Level>,
    /// `down[i]` reads the level above (the HDR frame for i = 0) and
    /// writes level i. The post pass reads every level itself, so there
    /// are no passes back up: each level is scaled by `WEIGHTS` there.
    down_groups: Vec<wgpu::BindGroup>,
    /// Frame size, level count and HDR texture generation it was built for.
    built: Option<((u32, u32), usize, u64)>,
}

/// One full-screen pass of the chain.
fn run_pass(
    encoder: &mut wgpu::CommandEncoder,
    target: &wgpu::TextureView,
    pipeline: &wgpu::RenderPipeline,
    group: &wgpu::BindGroup,
    load: wgpu::LoadOp<wgpu::Color>,
    timestamps: Option<wgpu::RenderPassTimestampWrites<'_>>,
) {
    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("bloom"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: target,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load,
                store: wgpu::StoreOp::Store,
            },
        })],
        timestamp_writes: timestamps,
        ..Default::default()
    });
    pass.set_pipeline(pipeline);
    pass.set_bind_group(0, group, &[]);
    pass.draw(0..3, 0..1);
}

impl Bloom {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("bloom"),
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
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("bloom"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let shader = device.create_shader_module(wgpu::include_wgsl!("bloom.wgsl"));
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("bloom"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let make = |entry: &str| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("bloom"),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_main"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                primitive: Default::default(),
                depth_stencil: None,
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(entry),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        Self {
            down: make("fs_down"),
            layout,
            sampler,
            levels: Vec::new(),
            down_groups: Vec::new(),
            built: None,
        }
    }

    /// The levels, once `run` has drawn them, largest first.
    pub fn levels(&self) -> Vec<&wgpu::TextureView> {
        self.levels.iter().map(|l| &l.view).collect()
    }

    /// Makes the levels for a frame of `size` (the HDR texture `hdr`,
    /// generation `generation`), if they aren't there.
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        hdr: &wgpu::TextureView,
        size: (u32, u32),
        count: usize,
        generation: u64,
    ) {
        if self.built == Some((size, count, generation)) {
            return;
        }
        self.built = Some((size, count, generation));
        self.levels.clear();
        let (mut w, mut h) = size;
        for i in 0..count {
            // The first level is a quarter of the frame (reading the whole
            // frame costs the most); each after it half the one before.
            let shrink = if i == 0 { 4 } else { 2 };
            (w, h) = ((w / shrink).max(1), (h / shrink).max(1));
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("bloom level"),
                size: wgpu::Extent3d {
                    width: w,
                    height: h,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            self.levels.push(Level {
                view: texture.create_view(&Default::default()),
                size: (w, h),
                _texture: texture,
            });
        }
        let group = |input: &wgpu::TextureView, input_size: (u32, u32), first: bool| {
            // The first level's four taps sit two texels out, to cover the
            // quarter-size step.
            let reach = if first { 2.0 } else { 1.0 };
            let params = Params {
                values: [
                    reach / input_size.0 as f32,
                    reach / input_size.1 as f32,
                    THRESHOLD,
                    f32::from(first),
                ],
            };
            let buffer = wgpu::util::DeviceExt::create_buffer_init(
                device,
                &wgpu::util::BufferInitDescriptor {
                    label: Some("bloom params"),
                    contents: bytemuck::bytes_of(&params),
                    usage: wgpu::BufferUsages::UNIFORM,
                },
            );
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("bloom"),
                layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(input),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&self.sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: buffer.as_entire_binding(),
                    },
                ],
            })
        };
        self.down_groups = (0..count)
            .map(|i| {
                if i == 0 {
                    group(hdr, size, true)
                } else {
                    group(&self.levels[i - 1].view, self.levels[i - 1].size, false)
                }
            })
            .collect();
    }

    /// Filters the HDR frame down through the levels: a pass each.
    pub fn run(&self, encoder: &mut wgpu::CommandEncoder, mut profiler: Option<&mut GpuProfiler>) {
        for i in 0..self.levels.len() {
            let ts = profiler.as_mut().and_then(|p| p.pass("bloom"));
            run_pass(
                encoder,
                &self.levels[i].view,
                &self.down,
                &self.down_groups[i],
                wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                ts,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plain_daylit_scene_gets_no_bloom() {
        // Noon grass, stone, sky, snow: linear values below 1 keep nothing,
        // so the post pass's picture is unchanged (well within 2%).
        for color in [
            [0.18, 0.35, 0.08],
            [0.3, 0.3, 0.3],
            [0.2, 0.4, 0.9],
            [0.95, 0.95, 0.95],
            [1.0, 1.0, 1.0],
        ] {
            assert_eq!(prefilter(color), [0.0; 3], "{color:?}");
        }
    }

    #[test]
    fn light_above_one_blooms_in_its_own_color() {
        let lava = prefilter([3.0, 1.2, 0.3]);
        assert!(lava[0] > 1.0 && lava[0] > lava[1] && lava[1] > lava[2]);
        let l = |c: [f32; 3]| 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
        // Exactly what's above the threshold is kept.
        assert!((l(lava) - (l([3.0, 1.2, 0.3]) - THRESHOLD)).abs() < 1e-4);
    }

    #[test]
    fn the_chain_is_five_levels_on_medium_and_high_and_four_on_low() {
        assert_eq!(levels(Quality::Low), 4);
        assert_eq!(levels(Quality::Medium), 5);
        assert_eq!(levels(Quality::High), 5);
    }
}
