//! wgpu setup and frame rendering.

pub mod atlas;
pub mod camera;
pub mod entities;
pub mod frustum;
pub mod icons;
pub mod models;
mod outline;
pub mod vertex;

use std::{collections::HashMap, sync::Arc};

use anyhow::{Context, Result, bail};
use glam::{IVec3, Vec2, Vec3};
use image::RgbaImage;
use wgpu::util::DeviceExt;
use winit::{dpi::PhysicalSize, window::Window};

use crate::{
    config::{CHUNK_SIZE, SKY_COLOR, WORLD_HEIGHT},
    world::{chunk::ChunkPos, mesh::Mesh},
};
use atlas::Atlas;
use camera::Camera;
use frustum::Frustum;
use outline::Outline;

/// One frame of tessellated UI from egui, drawn over the world.
pub struct UiPaint {
    pub primitives: Vec<egui::ClippedPrimitive>,
    pub textures: egui::TexturesDelta,
    pub pixels_per_point: f32,
}

const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

/// Mirrors `Globals` in globals.wgsl.
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Globals {
    view_proj: [[f32; 4]; 4],
    inverse_view_proj: [[f32; 4]; 4],
    camera: [f32; 4],
    fog_color: [f32; 4],
    fog: [f32; 4],
    sky_color: [f32; 4],
    sunrise: [f32; 4],
    extra: [f32; 4],
}

/// The sky, light and fog for one frame. Colors are sRGB, 0–1.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Environment {
    /// Game time in ticks, with the fraction since the last tick.
    pub time: f32,
    /// 1.0.0's celestial angle: 0 noon, 0.5 midnight.
    pub celestial_angle: f32,
    pub sky_color: Vec3,
    pub fog_color: Vec3,
    /// Linear fog between these distances...
    pub fog_start: f32,
    pub fog_end: f32,
    /// ...or exponential fog with this density, if positive (under water).
    pub fog_density: f32,
    /// Scales sky light in the lightmap, 0.2–1.
    pub sun_brightness: f32,
    pub star_brightness: f32,
    pub moon_phase: u32,
    /// The sunrise or sunset glow and its strength.
    pub sunrise: Option<(Vec3, f32)>,
    pub cloud_height: f32,
}

/// The sky images from the texture pack's `environment/` folder.
pub struct SkyImages {
    pub sun: RgbaImage,
    pub moon: RgbaImage,
    pub clouds: RgbaImage,
}

impl SkyImages {
    pub fn load(dir: &std::path::Path) -> Result<Self> {
        let load = |name: &str| -> Result<RgbaImage> {
            let path = dir.join(name);
            Ok(image::open(&path)
                .with_context(|| format!("loading {}", path.display()))?
                .into_rgba8())
        };
        Ok(Self {
            sun: load("sun.png")?,
            moon: load("moon_phases.png")?,
            clouds: load("clouds.png")?,
        })
    }
}

/// A shader made of globals.wgsl followed by `body`.
macro_rules! world_shader {
    ($device:expr, $file:literal) => {
        $device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some($file),
            source: wgpu::ShaderSource::Wgsl(
                concat!(include_str!("globals.wgsl"), include_str!($file)).into(),
            ),
        })
    };
}

/// A chunk's mesh on the GPU.
/// A mesh on the GPU: opaque indices first, then translucent ones.
struct ChunkBuffers {
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    opaque: u32,
    translucent: u32,
}

pub struct Renderer {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    depth_view: wgpu::TextureView,
    pipeline: wgpu::RenderPipeline,
    translucent_pipeline: wgpu::RenderPipeline,
    mob_pipeline: wgpu::RenderPipeline,
    /// One texture per mob kind.
    mob_bind_groups: Vec<wgpu::BindGroup>,
    /// This frame's mob meshes, by kind.
    mobs: Vec<(usize, ChunkBuffers)>,
    sky_pipeline: wgpu::RenderPipeline,
    sun_bind_group: wgpu::BindGroup,
    moon_bind_group: wgpu::BindGroup,
    cloud_pipeline: wgpu::RenderPipeline,
    cloud_bind_group: wgpu::BindGroup,
    globals_buffer: wgpu::Buffer,
    globals_bind_group: wgpu::BindGroup,
    atlas_bind_group: wgpu::BindGroup,
    chunks: HashMap<ChunkPos, ChunkBuffers>,
    /// Dropped items, rebuilt every frame.
    entities: Option<ChunkBuffers>,
    /// Chunks that passed frustum culling in the last frame.
    chunks_drawn: usize,
    outline: Outline,
    egui_renderer: egui_wgpu::Renderer,
}

impl Renderer {
    pub fn new(
        window: Arc<Window>,
        atlas: &Atlas,
        sky: &SkyImages,
        mob_textures: &[RgbaImage],
    ) -> Result<Self> {
        let size = window.inner_size();
        let instance =
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
        let surface = instance.create_surface(window)?;
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(&surface),
            ..Default::default()
        }))?;
        log::info!("GPU adapter: {:?}", adapter.get_info());
        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
                label: Some("rustcraft device"),
                ..Default::default()
            }))?;

        let mut config = surface
            .get_default_config(&adapter, size.width.max(1), size.height.max(1))
            .context("surface is not supported by the GPU adapter")?;
        // Prefer an sRGB format so colors are gamma-correct.
        let caps = surface.get_capabilities(&adapter);
        if let Some(format) = caps.formats.iter().find(|f| f.is_srgb()) {
            config.format = *format;
        }
        config.present_mode = wgpu::PresentMode::AutoVsync;
        // egui blends in gamma space, so it draws through a non-sRGB view of the frame.
        config.view_formats = vec![config.format.remove_srgb_suffix()];
        surface.configure(&device, &config);
        let depth_view = create_depth_view(&device, &config);

        let globals_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("globals"),
            size: std::mem::size_of::<Globals>() as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let globals_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("globals"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let globals_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("globals"),
            layout: &globals_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: globals_buffer.as_entire_binding(),
            }],
        });

        let texture_layout = create_texture_layout(&device);
        let atlas_bind_group =
            create_texture_bind_group(&device, &queue, &texture_layout, "atlas", atlas.image());
        let sun_bind_group =
            create_texture_bind_group(&device, &queue, &texture_layout, "sun", &sky.sun);
        let moon_bind_group =
            create_texture_bind_group(&device, &queue, &texture_layout, "moon", &sky.moon);
        let cloud_bind_group =
            create_texture_bind_group(&device, &queue, &texture_layout, "clouds", &sky.clouds);
        let atlas_layout = &texture_layout;

        let shader = world_shader!(device, "shader.wgsl");
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("blocks"),
            bind_group_layouts: &[Some(&globals_layout), Some(atlas_layout)],
            immediate_size: 0,
        });
        let pipeline = create_block_pipeline(
            &device,
            &pipeline_layout,
            &shader,
            config.format,
            false,
            true,
        );
        let translucent_pipeline = create_block_pipeline(
            &device,
            &pipeline_layout,
            &shader,
            config.format,
            true,
            true,
        );
        // Mob boxes are drawn from both sides, so face winding doesn't matter.
        let mob_pipeline = create_block_pipeline(
            &device,
            &pipeline_layout,
            &shader,
            config.format,
            false,
            false,
        );
        let mob_bind_groups = mob_textures
            .iter()
            .map(|image| create_texture_bind_group(&device, &queue, &texture_layout, "mob", image))
            .collect();

        let sky_pipeline =
            create_sky_pipeline(&device, &globals_layout, &texture_layout, config.format);
        let cloud_pipeline =
            create_cloud_pipeline(&device, &globals_layout, &texture_layout, config.format);
        let outline = Outline::new(&device, &globals_layout, config.format, DEPTH_FORMAT);
        let egui_renderer = egui_wgpu::Renderer::new(
            &device,
            config.format.remove_srgb_suffix(),
            egui_wgpu::RendererOptions::default(),
        );

        Ok(Self {
            surface,
            device,
            queue,
            config,
            depth_view,
            pipeline,
            translucent_pipeline,
            mob_pipeline,
            mob_bind_groups,
            mobs: Vec::new(),
            sky_pipeline,
            sun_bind_group,
            moon_bind_group,
            cloud_pipeline,
            cloud_bind_group,
            globals_buffer,
            globals_bind_group,
            atlas_bind_group,
            chunks: HashMap::new(),
            entities: None,
            chunks_drawn: 0,
            outline,
            egui_renderer,
        })
    }

    /// Uploads a chunk's mesh, replacing any previous one. Empty meshes just remove it.
    pub fn upload_chunk(&mut self, pos: ChunkPos, mesh: &Mesh) {
        if mesh.is_empty() {
            self.chunks.remove(&pos);
            return;
        }
        let buffers = self.buffers("chunk", mesh);
        self.chunks.insert(pos, buffers);
    }

    /// Replaces the mob meshes (one per kind that's present) drawn from
    /// the next frame on.
    pub fn set_mobs(&mut self, meshes: &[(usize, Mesh)]) {
        self.mobs = meshes
            .iter()
            .filter(|(_, mesh)| !mesh.is_empty())
            .map(|(kind, mesh)| (*kind, self.buffers("mob", mesh)))
            .collect();
    }

    /// Replaces the dropped-item mesh drawn from the next frame on.
    pub fn set_entities(&mut self, mesh: &Mesh) {
        self.entities = (!mesh.is_empty()).then(|| self.buffers("entity", mesh));
    }

    fn buffers(&self, label: &str, mesh: &Mesh) -> ChunkBuffers {
        let vertices = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(label),
                contents: bytemuck::cast_slice(&mesh.vertices),
                usage: wgpu::BufferUsages::VERTEX,
            });
        let all: Vec<u32> = mesh
            .indices
            .iter()
            .chain(&mesh.translucent)
            .copied()
            .collect();
        let indices = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(label),
                contents: bytemuck::cast_slice(&all),
                usage: wgpu::BufferUsages::INDEX,
            });
        ChunkBuffers {
            vertices,
            indices,
            opaque: mesh.indices.len() as u32,
            translucent: mesh.translucent.len() as u32,
        }
    }

    fn draw_clouds(&self, pass: &mut wgpu::RenderPass<'_>) {
        pass.set_pipeline(&self.cloud_pipeline);
        pass.set_bind_group(1, &self.cloud_bind_group, &[]);
        pass.draw(0..6, 0..1);
    }

    /// Outlines `block` from the next frame on, or nothing.
    pub fn set_outline(&mut self, block: Option<IVec3>) {
        self.outline.set_target(block);
    }

    pub fn remove_chunk(&mut self, pos: ChunkPos) {
        self.chunks.remove(&pos);
    }

    /// Chunk meshes drawn in the last frame, after frustum culling.
    pub fn chunks_drawn(&self) -> usize {
        self.chunks_drawn
    }

    pub fn aspect(&self) -> f32 {
        self.config.width as f32 / self.config.height as f32
    }

    pub fn resize(&mut self, size: PhysicalSize<u32>) {
        if size.width == 0 || size.height == 0 {
            return;
        }
        self.config.width = size.width;
        self.config.height = size.height;
        self.surface.configure(&self.device, &self.config);
        self.depth_view = create_depth_view(&self.device, &self.config);
    }

    /// Draws a frame. Returns false if the surface wasn't ready and the frame was skipped.
    pub fn render(
        &mut self,
        camera: &Camera,
        environment: &Environment,
        mut ui: UiPaint,
    ) -> Result<bool> {
        // Upload new egui textures (e.g. the font atlas) even if this frame is skipped.
        for (id, deltas) in &ui.textures.set {
            for delta in deltas {
                self.egui_renderer
                    .update_texture(&self.device, &self.queue, *id, delta);
            }
        }
        let result = self.draw(camera, environment, &ui);
        for id in &ui.textures.free {
            self.egui_renderer.free_texture(id);
        }
        ui.textures.clear();
        result
    }

    fn draw(&mut self, camera: &Camera, environment: &Environment, ui: &UiPaint) -> Result<bool> {
        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame)
            | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => frame,
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => {
                return Ok(false);
            }
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                self.surface.configure(&self.device, &self.config);
                return Ok(false);
            }
            wgpu::CurrentSurfaceTexture::Validation => bail!("surface validation error"),
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let ui_view = frame.texture.create_view(&wgpu::TextureViewDescriptor {
            format: Some(self.config.format.remove_srgb_suffix()),
            ..Default::default()
        });

        let view_proj = camera.view_proj(self.aspect());
        let frustum = Frustum::from_view_proj(view_proj);
        let globals = globals(view_proj, camera.position, environment);
        self.queue
            .write_buffer(&self.globals_buffer, 0, bytemuck::bytes_of(&globals));
        let outline = self.outline.prepare(&self.queue);

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("world"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(clear_color(self.config.format)),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Discard,
                    }),
                    stencil_ops: None,
                }),
                ..Default::default()
            });
            pass.set_bind_group(0, &self.globals_bind_group, &[]);
            pass.set_pipeline(&self.sky_pipeline);
            pass.set_bind_group(1, &self.sun_bind_group, &[]);
            pass.set_bind_group(2, &self.moon_bind_group, &[]);
            pass.draw(0..3, 0..1);
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(1, &self.atlas_bind_group, &[]);
            // Chunks in view, nearest first: opaque faces draw front to back,
            // translucent ones back to front.
            let mut visible: Vec<(f32, &ChunkBuffers)> = self
                .chunks
                .iter()
                .filter_map(|(pos, chunk)| {
                    let (x, z) = pos.min_block();
                    let min = Vec3::new(x as f32, 0.0, z as f32);
                    let size = Vec3::new(CHUNK_SIZE as f32, WORLD_HEIGHT as f32, CHUNK_SIZE as f32);
                    let center = min + size / 2.0;
                    let distance =
                        Vec2::new(center.x - camera.position.x, center.z - camera.position.z);
                    frustum
                        .intersects_box(min, min + size)
                        .then_some((distance.length_squared(), chunk))
                })
                .collect();
            visible.sort_by(|a, b| a.0.total_cmp(&b.0));
            self.chunks_drawn = visible.len();
            for (_, chunk) in &visible {
                draw_range(&mut pass, chunk, 0..chunk.opaque);
            }
            if let Some(entities) = &self.entities {
                draw_range(&mut pass, entities, 0..entities.opaque);
            }
            if !self.mobs.is_empty() {
                pass.set_pipeline(&self.mob_pipeline);
                for (kind, mesh) in &self.mobs {
                    pass.set_bind_group(1, &self.mob_bind_groups[*kind], &[]);
                    draw_range(&mut pass, mesh, 0..mesh.opaque);
                }
                pass.set_bind_group(1, &self.atlas_bind_group, &[]);
            }
            // Clouds and water both blend: draw whichever is farther first.
            let above_clouds = camera.position.y > environment.cloud_height;
            if above_clouds {
                self.draw_clouds(&mut pass);
            }
            pass.set_pipeline(&self.translucent_pipeline);
            pass.set_bind_group(1, &self.atlas_bind_group, &[]);
            for (_, chunk) in visible.iter().rev() {
                draw_range(
                    &mut pass,
                    chunk,
                    chunk.opaque..chunk.opaque + chunk.translucent,
                );
            }
            if !above_clouds {
                self.draw_clouds(&mut pass);
            }
            if outline {
                self.outline.draw(&mut pass);
            }
        }

        let screen = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [self.config.width, self.config.height],
            pixels_per_point: ui.pixels_per_point,
        };
        let ui_commands = self.egui_renderer.update_buffers(
            &self.device,
            &self.queue,
            &mut encoder,
            &ui.primitives,
            &screen,
        );
        {
            let mut pass = encoder
                .begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("ui"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &ui_view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Load,
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    ..Default::default()
                })
                .forget_lifetime();
            self.egui_renderer
                .render(&mut pass, &ui.primitives, &screen);
        }
        self.queue
            .submit(ui_commands.into_iter().chain([encoder.finish()]));
        self.queue.present(frame);
        Ok(true)
    }
}

/// Draws `range` of a mesh's indices, if it isn't empty.
fn draw_range(pass: &mut wgpu::RenderPass<'_>, mesh: &ChunkBuffers, range: std::ops::Range<u32>) {
    if range.is_empty() {
        return;
    }
    pass.set_vertex_buffer(0, mesh.vertices.slice(..));
    pass.set_index_buffer(mesh.indices.slice(..), wgpu::IndexFormat::Uint32);
    pass.draw_indexed(range, 0, 0..1);
}

fn globals(view_proj: glam::Mat4, camera: Vec3, env: &Environment) -> Globals {
    let (sunrise, strength) = env.sunrise.unwrap_or((Vec3::ZERO, 0.0));
    Globals {
        view_proj: view_proj.to_cols_array_2d(),
        inverse_view_proj: view_proj.inverse().to_cols_array_2d(),
        camera: camera.extend(env.time).to_array(),
        fog_color: env.fog_color.extend(env.fog_start).to_array(),
        fog: [
            env.fog_end,
            env.fog_density,
            env.sun_brightness,
            env.star_brightness,
        ],
        sky_color: env.sky_color.extend(env.celestial_angle).to_array(),
        sunrise: sunrise.extend(strength).to_array(),
        extra: [env.moon_phase as f32, env.cloud_height, 0.0, 0.0],
    }
}

/// A texture and sampler, for the atlas and the sky images.
fn create_texture_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("texture"),
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
    })
}

/// Uploads an sRGB texture with nearest filtering, no mipmaps and
/// repeating edges, like Minecraft 1.0.0.
fn create_texture_bind_group(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    layout: &wgpu::BindGroupLayout,
    label: &str,
    image: &RgbaImage,
) -> wgpu::BindGroup {
    let texture = device.create_texture_with_data(
        queue,
        &wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width: image.width(),
                height: image.height(),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        },
        wgpu::util::TextureDataOrder::LayerMajor,
        image.as_raw(),
    );
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some(label),
        address_mode_u: wgpu::AddressMode::Repeat,
        address_mode_v: wgpu::AddressMode::Repeat,
        mag_filter: wgpu::FilterMode::Nearest,
        min_filter: wgpu::FilterMode::Nearest,
        ..Default::default()
    });
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some(label),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(&sampler),
            },
        ],
    })
}

/// Block faces: opaque and cutout ones write depth; translucent ones blend
/// over what's behind them and don't.
fn create_block_pipeline(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    shader: &wgpu::ShaderModule,
    format: wgpu::TextureFormat,
    translucent: bool,
    cull_back: bool,
) -> wgpu::RenderPipeline {
    let (label, entry, blend) = if translucent {
        (
            "translucent blocks",
            "fs_translucent",
            Some(wgpu::BlendState::ALPHA_BLENDING),
        )
    } else {
        ("blocks", "fs_main", None)
    };
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("vs_main"),
            compilation_options: Default::default(),
            buffers: &[Some(vertex::Vertex::layout())],
        },
        primitive: wgpu::PrimitiveState {
            cull_mode: cull_back.then_some(wgpu::Face::Back),
            ..Default::default()
        },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: DEPTH_FORMAT,
            depth_write_enabled: Some(!translucent),
            depth_compare: Some(wgpu::CompareFunction::Less),
            stencil: Default::default(),
            bias: Default::default(),
        }),
        multisample: Default::default(),
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some(entry),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend,
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    })
}

/// Draws the cloud layer: blended, seen from both sides, depth-tested but
/// not written.
fn create_cloud_pipeline(
    device: &wgpu::Device,
    globals_layout: &wgpu::BindGroupLayout,
    texture_layout: &wgpu::BindGroupLayout,
    format: wgpu::TextureFormat,
) -> wgpu::RenderPipeline {
    let shader = world_shader!(device, "clouds.wgsl");
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("clouds"),
        bind_group_layouts: &[Some(globals_layout), Some(texture_layout)],
        immediate_size: 0,
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("clouds"),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            compilation_options: Default::default(),
            buffers: &[],
        },
        primitive: Default::default(),
        depth_stencil: Some(wgpu::DepthStencilState {
            format: DEPTH_FORMAT,
            depth_write_enabled: Some(false),
            depth_compare: Some(wgpu::CompareFunction::Less),
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
    })
}

/// Draws the sky behind everything: no depth test or write.
fn create_sky_pipeline(
    device: &wgpu::Device,
    globals_layout: &wgpu::BindGroupLayout,
    texture_layout: &wgpu::BindGroupLayout,
    format: wgpu::TextureFormat,
) -> wgpu::RenderPipeline {
    let shader = world_shader!(device, "sky.wgsl");
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("sky"),
        bind_group_layouts: &[
            Some(globals_layout),
            Some(texture_layout),
            Some(texture_layout),
        ],
        immediate_size: 0,
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("sky"),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            compilation_options: Default::default(),
            buffers: &[],
        },
        primitive: Default::default(),
        depth_stencil: Some(wgpu::DepthStencilState {
            format: DEPTH_FORMAT,
            depth_write_enabled: Some(false),
            depth_compare: Some(wgpu::CompareFunction::Always),
            stencil: Default::default(),
            bias: Default::default(),
        }),
        multisample: Default::default(),
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            compilation_options: Default::default(),
            targets: &[Some(format.into())],
        }),
        multiview_mask: None,
        cache: None,
    })
}

fn create_depth_view(
    device: &wgpu::Device,
    config: &wgpu::SurfaceConfiguration,
) -> wgpu::TextureView {
    device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some("depth"),
            size: wgpu::Extent3d {
                width: config.width,
                height: config.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: DEPTH_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        })
        .create_view(&wgpu::TextureViewDescriptor::default())
}

/// The sky color as a clear value for `format`. sRGB surfaces expect linear input.
fn clear_color(format: wgpu::TextureFormat) -> wgpu::Color {
    let [r, g, b] = if format.is_srgb() {
        SKY_COLOR.map(srgb_to_linear)
    } else {
        SKY_COLOR
    };
    wgpu::Color { r, g, b, a: 1.0 }
}

fn srgb_to_linear(c: f64) -> f64 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn srgb_to_linear_endpoints_and_midpoint() {
        assert_eq!(srgb_to_linear(0.0), 0.0);
        assert!((srgb_to_linear(1.0) - 1.0).abs() < 1e-12);
        // sRGB 0.5 is about 21.4% linear light.
        assert!((srgb_to_linear(0.5) - 0.214).abs() < 1e-3);
    }

    #[test]
    fn clear_color_matches_surface_encoding() {
        let srgb = clear_color(wgpu::TextureFormat::Bgra8UnormSrgb);
        let linear = clear_color(wgpu::TextureFormat::Bgra8Unorm);
        assert_eq!(linear.r, SKY_COLOR[0]);
        assert!(srgb.r < linear.r);
        assert_eq!(srgb.b, 1.0);
    }
}
