//! The Trailer look's frame (M54): the world is drawn into an HDR target
//! (`hdr_format`) with the same depth buffer, then post passes draw it into
//! the window's frame, and the UI goes on top, untouched, as in Vanilla.

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
    view: wgpu::TextureView,
    /// Reads it (and the post uniform) in the next pass.
    bind_group: wgpu::BindGroup,
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
        }
    }
}

pub struct TrailerFrame {
    /// Reads one texture with `textureLoad`.
    input_layout: wgpu::BindGroupLayout,
    post_uniform: wgpu::Buffer,
    post: wgpu::RenderPipeline,
    /// The HDR target, at the frame's size; made on first use.
    hdr: Option<Target>,
    hdr_format: wgpu::TextureFormat,
    size: (u32, u32),
}

impl TrailerFrame {
    /// `format`: the window's frame.
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
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
        Self {
            input_layout,
            post_uniform,
            post,
            hdr: None,
            hdr_format,
            size: (0, 0),
        }
    }

    /// Makes the targets for a frame of `width` × `height`, if they aren't.
    pub fn prepare(&mut self, device: &wgpu::Device, width: u32, height: u32) {
        if self.hdr.is_some() && self.size == (width, height) {
            return;
        }
        self.size = (width, height);
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
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&Default::default());
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("hdr input"),
            layout: &self.input_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: self.post_uniform.as_entire_binding(),
                },
            ],
        });
        self.hdr = Some(Target { view, bind_group });
    }

    /// The format of the world target.
    pub fn format(&self) -> wgpu::TextureFormat {
        self.hdr_format
    }

    /// Where the world is drawn, once `prepare` has run.
    pub fn hdr_view(&self) -> Option<&wgpu::TextureView> {
        self.hdr.as_ref().map(|t| &t.view)
    }

    /// The post passes, from the HDR target into `frame`.
    pub fn post(
        &self,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        frame: &wgpu::TextureView,
        params: &PostParams,
        profiler: Option<&mut GpuProfiler>,
    ) {
        let Some(hdr) = &self.hdr else {
            return;
        };
        queue.write_buffer(&self.post_uniform, 0, bytemuck::bytes_of(params));
        let timestamps = profiler.and_then(|p| p.pass("post"));
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("post"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: frame,
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
        pass.set_bind_group(0, &hdr.bind_group, &[]);
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
