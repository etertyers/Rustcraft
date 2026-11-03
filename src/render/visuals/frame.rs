//! The Trailer look's frame (M54): the world is drawn into an HDR target
//! (`hdr_format`) with the same depth buffer, then post passes draw it into
//! the window's frame, and the UI goes on top, untouched, as in Vanilla.

use wgpu::util::DeviceExt;

use super::profiler::GpuProfiler;

/// The Trailer look's world target when the GPU can't render to the
/// small format: 16-bit floats a channel (8 bytes a pixel).
pub const HDR_FORMAT_WIDE: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
/// Preferred: 11/11/10-bit floats in 4 bytes a pixel. Half the memory
/// traffic of `HDR_FORMAT_WIDE` on a tile GPU, and enough range and
/// precision for the shading (no alpha is needed).
pub const HDR_FORMAT_SMALL: wgpu::TextureFormat = wgpu::TextureFormat::Rg11b10Ufloat;

/// The features to ask the device for to use the small format.
pub const HDR_FEATURE: wgpu::Features = wgpu::Features::RG11B10UFLOAT_RENDERABLE;

/// The HDR format a device with `features` draws the Trailer look in.
pub fn hdr_format(features: wgpu::Features) -> wgpu::TextureFormat {
    if features.contains(HDR_FEATURE) {
        HDR_FORMAT_SMALL
    } else {
        HDR_FORMAT_WIDE
    }
}

/// One texture the post passes read and write.
struct Target {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
}

/// Mirrors `Post` in post.wgsl.
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct PostParams {
    /// x: saturation; y: where the tone curve's shoulder starts; z: the
    /// strength of the sunrise tint.
    pub params: [f32; 4],
    /// The sunrise and sunset tint.
    pub tint: [f32; 4],
    /// x: how much of the bloom is added; the weights of its first four
    /// levels come with `bloom_weights`.
    pub bloom: [f32; 4],
    /// The weights of the bloom's levels (the fifth is `bloom.y`).
    pub bloom_weights: [f32; 4],
    /// rgb: the light shafts' color times their strength; w: 1 when they
    /// are drawn.
    pub shafts: [f32; 4],
}

impl PostParams {
    /// The tone curve and saturation, and a warm tint of `sunrise`
    /// strength (0–1) at sunrise and sunset. With `fullbright` (a mod's
    /// brightness override) the curve is the identity and there's no tint,
    /// so nothing is darker than in Vanilla.
    pub fn new(sunrise: f32, fullbright: bool) -> Self {
        let (shoulder, sunrise) = if fullbright {
            (1.0, 0.0)
        } else {
            (super::sun::SHOULDER, sunrise.clamp(0.0, 1.0))
        };
        Self {
            params: [super::sun::SATURATION, shoulder, sunrise, 0.0],
            tint: [1.05, 0.97, 0.86, 1.0],
            bloom: [0.0; 4],
            bloom_weights: [0.0; 4],
            shafts: [0.0; 4],
        }
    }

    pub fn with_bloom(mut self, strength: f32, levels: usize) -> Self {
        let weight = |i: usize| {
            if i < levels {
                super::bloom::WEIGHTS[i]
            } else {
                0.0
            }
        };
        self.bloom = [strength, weight(4), 0.0, 0.0];
        self.bloom_weights = [weight(0), weight(1), weight(2), weight(3)];
        self
    }

    pub fn with_shafts(mut self, color: [f32; 3]) -> Self {
        self.shafts = [color[0], color[1], color[2], 1.0];
        self
    }
}

pub struct TrailerFrame {
    /// Reads one texture with `textureLoad`.
    input_layout: wgpu::BindGroupLayout,
    post_uniform: wgpu::Buffer,
    post: wgpu::RenderPipeline,
    /// The HDR target, at the frame's size; made on first use.
    hdr: Option<Target>,
    /// Counts the HDR target's remakes, for what was built against it.
    generation: u64,
    /// The tone-mapped picture, for FXAA to read (when FXAA is on).
    ldr: Option<(wgpu::Texture, wgpu::TextureView)>,
    ldr_size: (u32, u32),
    sampler: wgpu::Sampler,
    /// A black 1 × 1, for a bloom or shafts that isn't drawn.
    dummy: wgpu::TextureView,
    hdr_format: wgpu::TextureFormat,
    /// The target can be copied from (with the water reading the scene).
    copyable: bool,
    size: (u32, u32),
}

impl TrailerFrame {
    /// `format`: the window's frame.
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        let hdr_format = hdr_format(device.features());
        let input_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("post input"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                // The bloom's five levels, the light shafts, and a sampler
                // for them.
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 5,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 6,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 7,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 8,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let post_uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("post"),
            size: std::mem::size_of::<PostParams>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let shader = device.create_shader_module(wgpu::include_wgsl!("post.wgsl"));
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("post"),
            bind_group_layouts: &[Some(&input_layout)],
            immediate_size: 0,
        });
        let post = fullscreen_pipeline(device, &layout, &shader, "post", format);
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("post"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let dummy = device
            .create_texture_with_data(
                queue,
                &wgpu::TextureDescriptor {
                    label: Some("post dummy"),
                    size: wgpu::Extent3d {
                        width: 1,
                        height: 1,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                },
                wgpu::util::TextureDataOrder::LayerMajor,
                &[0, 0, 0, 255],
            )
            .create_view(&Default::default());
        Self {
            input_layout,
            post_uniform,
            post,
            hdr: None,
            generation: 0,
            ldr: None,
            ldr_size: (0, 0),
            sampler,
            dummy,
            hdr_format,
            copyable: false,
            size: (0, 0),
        }
    }

    /// Makes the targets for a frame of `width` × `height`, if they aren't.
    pub fn prepare(&mut self, device: &wgpu::Device, width: u32, height: u32, copy: bool) {
        if self.hdr.is_some() && self.size == (width, height) && self.copyable == copy {
            return;
        }
        self.size = (width, height);
        self.copyable = copy;
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("hdr"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.hdr_format,
            usage: if copy {
                wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING
                    | wgpu::TextureUsages::COPY_SRC
            } else {
                wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING
            },
            view_formats: &[],
        });
        let view = texture.create_view(&Default::default());
        self.generation += 1;
        self.hdr = Some(Target { texture, view });
    }

    /// The tone-mapped picture's texture (made, or dropped when `on` is
    /// false), for FXAA to read.
    pub fn prepare_ldr(&mut self, device: &wgpu::Device, format: wgpu::TextureFormat, on: bool) {
        if !on {
            self.ldr = None;
            return;
        }
        if self.ldr.is_some() && self.ldr_size == self.size {
            return;
        }
        self.ldr_size = self.size;
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("ldr"),
            size: wgpu::Extent3d {
                width: self.size.0,
                height: self.size.1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&Default::default());
        self.ldr = Some((texture, view));
    }

    /// The tone-mapped picture, when FXAA is on.
    pub fn ldr_view(&self) -> Option<&wgpu::TextureView> {
        self.ldr.as_ref().map(|(_, v)| v)
    }

    /// Changes whenever the HDR target is made again.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// The format of the world target.
    pub fn format(&self) -> wgpu::TextureFormat {
        self.hdr_format
    }

    /// The HDR target as a texture, to copy the opaque scene out of.
    pub fn hdr_texture(&self) -> Option<&wgpu::Texture> {
        self.hdr.as_ref().map(|t| &t.texture)
    }

    /// Where the world is drawn, once `prepare` has run.
    pub fn hdr_view(&self) -> Option<&wgpu::TextureView> {
        self.hdr.as_ref().map(|t| &t.view)
    }

    /// The post pass: the HDR target (with the bloom and the light
    /// shafts when given) tone-mapped into `target`.
    #[allow(clippy::too_many_arguments)]
    pub fn post(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        params: &PostParams,
        bloom: &[&wgpu::TextureView],
        shafts: Option<&wgpu::TextureView>,
        profiler: Option<&mut GpuProfiler>,
    ) {
        let Some(hdr) = &self.hdr else {
            return;
        };
        queue.write_buffer(&self.post_uniform, 0, bytemuck::bytes_of(params));
        // A bloom level that isn't drawn (Low has four) is black.
        let level = |i: usize| bloom.get(i).copied().unwrap_or(&self.dummy);
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("post"),
            layout: &self.input_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&hdr.view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: self.post_uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(level(0)),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(level(1)),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::TextureView(level(2)),
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: wgpu::BindingResource::TextureView(level(3)),
                },
                wgpu::BindGroupEntry {
                    binding: 6,
                    resource: wgpu::BindingResource::TextureView(level(4)),
                },
                wgpu::BindGroupEntry {
                    binding: 7,
                    resource: wgpu::BindingResource::TextureView(shafts.unwrap_or(&self.dummy)),
                },
                wgpu::BindGroupEntry {
                    binding: 8,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        });
        let timestamps = profiler.and_then(|p| p.pass("post"));
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("post"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            timestamp_writes: timestamps,
            ..Default::default()
        });
        pass.set_pipeline(&self.post);
        pass.set_bind_group(0, &group, &[]);
        pass.draw(0..3, 0..1);
    }
}

/// A pipeline drawing one full-screen triangle into `format`, no depth.
pub fn fullscreen_pipeline(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    shader: &wgpu::ShaderModule,
    label: &str,
    format: wgpu::TextureFormat,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("vs_main"),
            compilation_options: Default::default(),
            buffers: &[],
        },
        primitive: Default::default(),
        depth_stencil: None,
        multisample: Default::default(),
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some("fs_main"),
            compilation_options: Default::default(),
            targets: &[Some(format.into())],
        }),
        multiview_mask: None,
        cache: None,
    })
}
