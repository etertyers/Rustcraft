//! The Trailer look's frame (M54): the world is drawn into an HDR target
//! (`HDR_FORMAT`) with the same depth buffer, then post passes draw it into
//! the window's frame, and the UI goes on top, untouched, as in Vanilla.

use super::profiler::GpuProfiler;

/// The Trailer look's world target.
pub const HDR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

/// One texture the post passes read and write.
struct Target {
    view: wgpu::TextureView,
    /// Reads it in the next pass.
    bind_group: wgpu::BindGroup,
}

pub struct TrailerFrame {
    /// Reads one texture with `textureLoad`.
    input_layout: wgpu::BindGroupLayout,
    post: wgpu::RenderPipeline,
    /// The HDR target, at the frame's size; made on first use.
    hdr: Option<Target>,
    size: (u32, u32),
}

impl TrailerFrame {
    /// `format`: the window's frame.
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let input_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("post input"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            }],
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
            post,
            hdr: None,
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
            format: HDR_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&Default::default());
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("hdr input"),
            layout: &self.input_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&view),
            }],
        });
        self.hdr = Some(Target { view, bind_group });
    }

    /// Where the world is drawn, once `prepare` has run.
    pub fn hdr_view(&self) -> Option<&wgpu::TextureView> {
        self.hdr.as_ref().map(|t| &t.view)
    }

    /// The post passes, from the HDR target into `frame`.
    pub fn post(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        frame: &wgpu::TextureView,
        profiler: Option<&mut GpuProfiler>,
    ) {
        let Some(hdr) = &self.hdr else {
            return;
        };
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
