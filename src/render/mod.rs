//! wgpu setup and frame rendering.

pub mod atlas;
pub mod boat;
pub mod camera;
pub mod crack;
pub mod dragon;
pub mod entities;
pub mod frustum;
pub mod hand;
pub mod held;
pub mod icons;
pub mod lightmap;
pub mod models;
mod outline;
pub mod player;
pub mod vertex;

use std::sync::Arc;

use anyhow::{Context, Result, bail};
use glam::{IVec3, Vec2, Vec3};
use image::RgbaImage;
use wgpu::util::DeviceExt;
use winit::{dpi::PhysicalSize, window::Window};

use crate::{
    config::{CHUNK_SIZE, SKY_COLOR, WORLD_HEIGHT},
    world::{chunk::ChunkPos, fast_hash::FastMap, mesh::Mesh},
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
    inverse_sky_view_proj: [[f32; 4]; 4],
    camera: [f32; 4],
    fog_color: [f32; 4],
    fog: [f32; 4],
    sky_color: [f32; 4],
    sunrise: [f32; 4],
    extra: [f32; 4],
    lighting: [f32; 4],
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
    /// Whether there's a sky with a sun, moon, stars and clouds (not in
    /// the Nether: there it's all fog).
    pub sky: bool,
    /// The Brightness option, 0 (Moody) to 1 (Bright).
    pub brightness: f32,
    /// The dimension's ambient light, lifting the dark (modern
    /// `DimensionType.ambientLight`: 0.1 in the Nether).
    pub ambient: f32,
}

/// The sky images from the packs' `environment/` folder.
pub struct SkyImages {
    pub sun: RgbaImage,
    pub moon: RgbaImage,
    pub clouds: RgbaImage,
    /// The enchantment glint's pattern (`misc/enchanted_glint_item`).
    pub glint: RgbaImage,
}

/// Name of the glint pattern.
pub const GLINT_TEXTURE: &str = "misc/enchanted_glint_item";

/// Added to the sky light of an enchanted item's vertices to make the
/// shader shimmer them (real sky light never gets near it).
pub const GLINT_FLAG: u8 = 64;

impl SkyImages {
    pub fn load(res: &crate::pack::Resources) -> Result<Self> {
        let load = |name: &str| {
            res.image(name)
                .with_context(|| format!("no texture {name}"))
        };
        Ok(Self {
            sun: load("environment/sun")?,
            moon: load("environment/moon_phases")?,
            clouds: load("environment/clouds")?,
            glint: res.image(GLINT_TEXTURE).unwrap_or_else(|| {
                log::warn!("no layer has {GLINT_TEXTURE}; enchanted items won't glint");
                RgbaImage::from_pixel(1, 1, image::Rgba([0, 0, 0, 0]))
            }),
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
    /// The atlas generation its UVs point into.
    atlas: u32,
}

pub struct Renderer {
    surface: wgpu::Surface<'static>,
    /// Draw into `offscreen` when the window has no frame to give (it's
    /// hidden or the screen is locked), waiting for the GPU, so `--bench`
    /// still measures real frames.
    offscreen_fallback: bool,
    offscreen: Option<wgpu::Texture>,
    /// The last frame went to `offscreen`.
    drew_offscreen: bool,
    /// Where to save the next frame as a PNG (for `--shot`).
    capture: Option<std::path::PathBuf>,
    /// The Clouds option isn't off.
    clouds: bool,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    depth_view: wgpu::TextureView,
    pipeline: wgpu::RenderPipeline,
    translucent_pipeline: wgpu::RenderPipeline,
    mob_pipeline: wgpu::RenderPipeline,
    crack_pipeline: wgpu::RenderPipeline,
    /// The crack over the block being mined, if any.
    crack: Option<ChunkBuffers>,
    /// One texture per mob kind.
    mob_bind_groups: Vec<wgpu::BindGroup>,
    /// This frame's mob meshes, by kind.
    mobs: Vec<(usize, ChunkBuffers)>,
    sky_pipeline: wgpu::RenderPipeline,
    sun_bind_group: wgpu::BindGroup,
    moon_bind_group: wgpu::BindGroup,
    cloud_pipeline: wgpu::RenderPipeline,
    cloud_bind_group: wgpu::BindGroup,
    glint_bind_group: wgpu::BindGroup,
    globals_buffer: wgpu::Buffer,
    globals_bind_group: wgpu::BindGroup,
    /// The same, for the first-person hand's own projection.
    hand_globals_buffer: wgpu::Buffer,
    hand_globals_bind_group: wgpu::BindGroup,
    /// The hand's arm (drawn with the skin, the last mob texture) and item.
    hand_arm: Option<ChunkBuffers>,
    hand_item: Option<ChunkBuffers>,
    texture_layout: wgpu::BindGroupLayout,
    /// The atlas, kept to copy animation frames into.
    atlas_texture: wgpu::Texture,
    atlas_bind_group: wgpu::BindGroup,
    /// Bumped each time the packs change the atlas.
    atlas_generation: u32,
    /// Earlier atlases, kept while chunks meshed against them wait to be
    /// remeshed, so they don't flash wrong textures.
    old_atlases: Vec<(u32, wgpu::BindGroup)>,
    chunks: FastMap<ChunkPos, ChunkBuffers>,
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
        let hand_globals_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("hand globals"),
            size: std::mem::size_of::<Globals>() as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let hand_globals_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("hand globals"),
            layout: &globals_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: hand_globals_buffer.as_entire_binding(),
            }],
        });

        let texture_layout = create_texture_layout(&device);
        let (atlas_texture, atlas_bind_group) =
            create_texture(&device, &queue, &texture_layout, "atlas", atlas.image());
        let sun_bind_group =
            create_texture_bind_group(&device, &queue, &texture_layout, "sun", &sky.sun);
        let moon_bind_group =
            create_texture_bind_group(&device, &queue, &texture_layout, "moon", &sky.moon);
        let cloud_bind_group =
            create_texture_bind_group(&device, &queue, &texture_layout, "clouds", &sky.clouds);
        let glint_bind_group =
            create_texture_bind_group(&device, &queue, &texture_layout, "glint", &sky.glint);
        let atlas_layout = &texture_layout;

        let shader = world_shader!(device, "shader.wgsl");
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("blocks"),
            // The glint pattern is group 2 (the sky borrows it for the moon).
            bind_group_layouts: &[
                Some(&globals_layout),
                Some(atlas_layout),
                Some(atlas_layout),
            ],
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
        let crack_pipeline =
            create_crack_pipeline(&device, &pipeline_layout, &shader, config.format);
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
            offscreen_fallback: false,
            offscreen: None,
            capture: None,
            clouds: true,
            drew_offscreen: false,
            device,
            queue,
            config,
            depth_view,
            pipeline,
            translucent_pipeline,
            mob_pipeline,
            crack_pipeline,
            crack: None,
            mob_bind_groups,
            mobs: Vec::new(),
            sky_pipeline,
            sun_bind_group,
            moon_bind_group,
            cloud_pipeline,
            cloud_bind_group,
            glint_bind_group,
            globals_buffer,
            globals_bind_group,
            hand_globals_buffer,
            hand_globals_bind_group,
            hand_arm: None,
            hand_item: None,
            texture_layout,
            atlas_texture,
            atlas_bind_group,
            atlas_generation: 0,
            old_atlases: Vec::new(),
            chunks: FastMap::default(),
            entities: None,
            chunks_drawn: 0,
            outline,
            egui_renderer,
        })
    }

    /// Swaps in new textures after the resource packs change. Chunk meshes
    /// made against the old atlas must be rebuilt.
    pub fn set_textures(&mut self, atlas: &Atlas, sky: &SkyImages, mob_textures: &[RgbaImage]) {
        let (device, queue, layout) = (&self.device, &self.queue, &self.texture_layout);
        let (texture, bind_group) = create_texture(device, queue, layout, "atlas", atlas.image());
        self.atlas_texture = texture;
        let old = std::mem::replace(&mut self.atlas_bind_group, bind_group);
        self.old_atlases.push((self.atlas_generation, old));
        self.atlas_generation += 1;
        self.sun_bind_group = create_texture_bind_group(device, queue, layout, "sun", &sky.sun);
        self.moon_bind_group = create_texture_bind_group(device, queue, layout, "moon", &sky.moon);
        self.cloud_bind_group =
            create_texture_bind_group(device, queue, layout, "clouds", &sky.clouds);
        self.mob_bind_groups = mob_textures
            .iter()
            .map(|image| create_texture_bind_group(device, queue, layout, "mob", image))
            .collect();
    }

    /// The atlas bind group for buffers made against generation `atlas`.
    fn atlas_for(&self, atlas: u32) -> &wgpu::BindGroup {
        self.old_atlases
            .iter()
            .find(|(generation, _)| *generation == atlas)
            .map_or(&self.atlas_bind_group, |(_, group)| group)
    }

    /// Copies animation frames into the atlas: (x, y, picture) per tile.
    pub fn update_atlas(&self, tiles: &[(u32, u32, RgbaImage)]) {
        for (x, y, picture) in tiles {
            self.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &self.atlas_texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d { x: *x, y: *y, z: 0 },
                    aspect: wgpu::TextureAspect::All,
                },
                picture.as_raw(),
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(4 * picture.width()),
                    rows_per_image: Some(picture.height()),
                },
                wgpu::Extent3d {
                    width: picture.width(),
                    height: picture.height(),
                    depth_or_array_layers: 1,
                },
            );
        }
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
        let mut old = std::mem::take(&mut self.mobs);
        self.mobs = meshes
            .iter()
            .filter(|(_, mesh)| !mesh.is_empty())
            .map(|(kind, mesh)| {
                let reuse = old
                    .iter()
                    .position(|(k, _)| k == kind)
                    .map(|i| old.swap_remove(i).1);
                (*kind, self.rewrite(reuse, "mob", mesh))
            })
            .collect();
    }

    /// Replaces the first-person hand drawn from the next frame on: the arm
    /// (textured with the skin, the last mob texture) and the held item
    /// (textured with the atlas), in camera space.
    pub fn set_hand(&mut self, arm: Option<&Mesh>, item: Option<&Mesh>) {
        let (old_arm, old_item) = (self.hand_arm.take(), self.hand_item.take());
        self.hand_arm = arm
            .filter(|mesh| !mesh.is_empty())
            .map(|mesh| self.rewrite(old_arm, "hand arm", mesh));
        self.hand_item = item
            .filter(|mesh| !mesh.is_empty())
            .map(|mesh| self.rewrite(old_item, "hand item", mesh));
    }

    /// Replaces the mining crack drawn from the next frame on.
    pub fn set_crack(&mut self, mesh: Option<&Mesh>) {
        let old = self.crack.take();
        self.crack = mesh.map(|mesh| self.rewrite(old, "crack", mesh));
    }

    /// Replaces the dropped-item mesh drawn from the next frame on.
    pub fn set_entities(&mut self, mesh: &Mesh) {
        let old = self.entities.take();
        self.entities = (!mesh.is_empty()).then(|| self.rewrite(old, "entity", mesh));
    }

    /// Buffers for a mesh that changes every frame: `old`'s, written over,
    /// if they're big enough, or new ones with room to grow.
    fn rewrite(&self, old: Option<ChunkBuffers>, label: &str, mesh: &Mesh) -> ChunkBuffers {
        let vertices: &[u8] = bytemuck::cast_slice(&mesh.vertices);
        let mut indices = Vec::with_capacity(mesh.indices.len() + mesh.translucent.len());
        indices.extend_from_slice(&mesh.indices);
        indices.extend_from_slice(&mesh.translucent);
        let indices: &[u8] = bytemuck::cast_slice(&indices);
        let fits = |buffer: &wgpu::Buffer, bytes: &[u8]| buffer.size() >= bytes.len() as u64;
        let (vertex_buffer, index_buffer) = match old {
            Some(old) if fits(&old.vertices, vertices) && fits(&old.indices, indices) => {
                (old.vertices, old.indices)
            }
            _ => {
                let buffer = |size: usize, usage| {
                    self.device.create_buffer(&wgpu::BufferDescriptor {
                        label: Some(label),
                        size: (size.next_power_of_two() as u64).max(256),
                        usage: usage | wgpu::BufferUsages::COPY_DST,
                        mapped_at_creation: false,
                    })
                };
                (
                    buffer(vertices.len(), wgpu::BufferUsages::VERTEX),
                    buffer(indices.len(), wgpu::BufferUsages::INDEX),
                )
            }
        };
        self.queue.write_buffer(&vertex_buffer, 0, vertices);
        self.queue.write_buffer(&index_buffer, 0, indices);
        ChunkBuffers {
            vertices: vertex_buffer,
            indices: index_buffer,
            opaque: mesh.indices.len() as u32,
            translucent: mesh.translucent.len() as u32,
            atlas: self.atlas_generation,
        }
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
            atlas: self.atlas_generation,
        }
    }

    fn draw_clouds(&self, pass: &mut wgpu::RenderPass<'_>) {
        pass.set_pipeline(&self.cloud_pipeline);
        pass.set_bind_group(1, &self.cloud_bind_group, &[]);
        pass.draw(0..6, 0..1);
    }

    /// Outlines `block`'s boxes from the next frame on, or nothing.
    pub fn set_outline(&mut self, target: Option<(IVec3, Vec<crate::world::shape::Cuboid>)>) {
        self.outline.set_target(target);
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

    /// Forgets the world's chunks, items and mobs (leaving a world).
    pub fn clear_world(&mut self) {
        self.chunks.clear();
        self.entities = None;
        self.crack = None;
        self.mobs.clear();
        self.outline.set_target(None);
    }

    /// The Clouds option: off, or drawn (Fast and Fancy are both flat).
    pub fn set_clouds(&mut self, clouds: bool) {
        self.clouds = clouds;
    }

    /// Waits for the display's refresh (vsync) or not.
    pub fn set_vsync(&mut self, vsync: bool) {
        self.config.present_mode = if vsync {
            wgpu::PresentMode::AutoVsync
        } else {
            wgpu::PresentMode::AutoNoVsync
        };
        self.surface.configure(&self.device, &self.config);
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

    /// Draws into a texture instead of the window when it has no frame
    /// to give (for `--bench`).
    pub fn set_offscreen_fallback(&mut self, on: bool) {
        self.offscreen_fallback = on;
    }

    /// Whether the last frame was drawn off screen.
    pub fn drew_offscreen(&self) -> bool {
        self.drew_offscreen
    }

    /// Draws the next frame off screen and saves it to `path` as a PNG
    /// instead of showing it.
    pub fn capture_next_frame(&mut self, path: std::path::PathBuf) {
        self.capture = Some(path);
    }

    /// A texture like the window's frames that can be copied out.
    fn capture_target(&self) -> wgpu::Texture {
        self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("capture"),
            size: wgpu::Extent3d {
                width: self.config.width,
                height: self.config.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.config.format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[self.config.format.remove_srgb_suffix()],
        })
    }

    /// Queues a copy of `texture` into a buffer to read back; returns the
    /// buffer and its padded row length in bytes.
    fn copy_out(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        texture: &wgpu::Texture,
    ) -> (wgpu::Buffer, u32) {
        let (width, height) = (self.config.width, self.config.height);
        let padded = (width * 4).div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
            * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("capture readback"),
            size: (padded * height) as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        encoder.copy_texture_to_buffer(
            texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded),
                    rows_per_image: Some(height),
                },
            },
            texture.size(),
        );
        (buffer, padded)
    }

    /// Waits for a copied frame and writes it as a PNG.
    fn save_capture(
        &self,
        buffer: &wgpu::Buffer,
        padded: u32,
        path: &std::path::Path,
    ) -> Result<()> {
        let slice = buffer.slice(..);
        slice.map_async(wgpu::MapMode::Read, |_| {});
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .context("waiting for the captured frame")?;
        let (width, height) = (self.config.width, self.config.height);
        let data = slice
            .get_mapped_range()
            .map_err(|err| anyhow::anyhow!("reading the captured frame: {err:?}"))?;
        let bgra = matches!(
            self.config.format,
            wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb
        );
        let mut pixels = Vec::with_capacity((width * height * 4) as usize);
        for row in data.chunks(padded as usize).take(height as usize) {
            for px in row[..(width * 4) as usize].chunks(4) {
                let [a, b, c, _] = [px[0], px[1], px[2], px[3]];
                let (r, g, b) = if bgra { (c, b, a) } else { (a, b, c) };
                pixels.extend_from_slice(&[r, g, b, 255]);
            }
        }
        drop(data);
        buffer.unmap();
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        image::RgbaImage::from_raw(width, height, pixels)
            .context("captured frame size")?
            .save(path)
            .with_context(|| format!("saving {}", path.display()))?;
        log::info!("saved {}", path.display());
        Ok(())
    }

    /// A texture like the window's frames, for drawing off screen.
    fn offscreen_target(&mut self) -> wgpu::Texture {
        let size = wgpu::Extent3d {
            width: self.config.width,
            height: self.config.height,
            depth_or_array_layers: 1,
        };
        if let Some(texture) = &self.offscreen
            && texture.size() == size
        {
            return texture.clone();
        }
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("offscreen"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.config.format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[self.config.format.remove_srgb_suffix()],
        });
        self.offscreen = Some(texture.clone());
        texture
    }

    fn draw(&mut self, camera: &Camera, environment: &Environment, ui: &UiPaint) -> Result<bool> {
        let capture = self.capture.take();
        let frame = match self.surface.get_current_texture() {
            _ if capture.is_some() => None,
            wgpu::CurrentSurfaceTexture::Success(frame)
            | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => Some(frame),
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded
                if self.offscreen_fallback =>
            {
                None
            }
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => {
                return Ok(false);
            }
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                self.surface.configure(&self.device, &self.config);
                return Ok(false);
            }
            wgpu::CurrentSurfaceTexture::Validation => bail!("surface validation error"),
        };
        self.drew_offscreen = frame.is_none();
        let target = match (&frame, &capture) {
            (Some(frame), _) => frame.texture.clone(),
            (None, Some(_)) => self.capture_target(),
            (None, None) => self.offscreen_target(),
        };
        let view = target.create_view(&wgpu::TextureViewDescriptor::default());
        let ui_view = target.create_view(&wgpu::TextureViewDescriptor {
            format: Some(self.config.format.remove_srgb_suffix()),
            ..Default::default()
        });

        let view_proj = camera.view_proj(self.aspect());
        let frustum = Frustum::from_view_proj(view_proj);
        if !self.old_atlases.is_empty() {
            let chunks = &self.chunks;
            self.old_atlases
                .retain(|(generation, _)| chunks.values().any(|c| c.atlas == *generation));
        }
        let frame_globals = globals(
            view_proj,
            camera.sky_view_proj(self.aspect()),
            camera.position,
            environment,
        );
        self.queue
            .write_buffer(&self.globals_buffer, 0, bytemuck::bytes_of(&frame_globals));
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
            pass.set_bind_group(2, &self.glint_bind_group, &[]);
            // Chunks in view, nearest first: opaque faces draw front to back,
            // translucent ones back to front.
            let mut visible: Vec<(f32, &ChunkBuffers)> = self
                .chunks
                .iter()
                .filter_map(|(pos, chunk)| {
                    let (x, z) = pos.min_block();
                    let min = Vec3::new(x as f32, crate::config::MIN_Y as f32, z as f32);
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
            let mut bound = self.atlas_generation;
            for (_, chunk) in &visible {
                if chunk.atlas != bound {
                    bound = chunk.atlas;
                    pass.set_bind_group(1, self.atlas_for(bound), &[]);
                }
                draw_range(&mut pass, chunk, 0..chunk.opaque);
            }
            pass.set_bind_group(1, &self.atlas_bind_group, &[]);
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
            if let Some(crack) = &self.crack {
                pass.set_pipeline(&self.crack_pipeline);
                draw_range(&mut pass, crack, 0..crack.opaque);
            }
            // Clouds and water both blend: draw whichever is farther first.
            let above_clouds = camera.position.y > environment.cloud_height;
            if above_clouds && environment.sky && self.clouds {
                self.draw_clouds(&mut pass);
            }
            pass.set_pipeline(&self.translucent_pipeline);
            pass.set_bind_group(1, &self.atlas_bind_group, &[]);
            // Entities' shadows, under the water drawn next.
            if let Some(entities) = &self.entities {
                let translucent = entities.opaque..entities.opaque + entities.translucent;
                draw_range(&mut pass, entities, translucent);
            }
            let mut bound = self.atlas_generation;
            for (_, chunk) in visible.iter().rev() {
                if chunk.atlas != bound {
                    bound = chunk.atlas;
                    pass.set_bind_group(1, self.atlas_for(bound), &[]);
                }
                draw_range(
                    &mut pass,
                    chunk,
                    chunk.opaque..chunk.opaque + chunk.translucent,
                );
            }
            if !above_clouds && environment.sky && self.clouds {
                self.draw_clouds(&mut pass);
            }
            if outline > 0 {
                self.outline.draw(&mut pass, outline);
            }
        }

        if self.hand_arm.is_some() || self.hand_item.is_some() {
            // The hand is drawn over the world with its own 70° projection,
            // a fresh depth buffer and no fog.
            let hand_env = Environment {
                fog_start: 1.0e6,
                fog_end: 2.0e6,
                fog_density: 0.0,
                ..*environment
            };
            let hand_proj = hand::hand_projection(self.aspect());
            let hand = globals(hand_proj, hand_proj, Vec3::ZERO, &hand_env);
            self.queue
                .write_buffer(&self.hand_globals_buffer, 0, bytemuck::bytes_of(&hand));
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("hand"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
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
            pass.set_bind_group(0, &self.hand_globals_bind_group, &[]);
            pass.set_bind_group(2, &self.glint_bind_group, &[]);
            pass.set_pipeline(&self.mob_pipeline);
            if let Some(arm) = &self.hand_arm
                && let Some(skin) = self.mob_bind_groups.last()
            {
                pass.set_bind_group(1, skin, &[]);
                draw_range(&mut pass, arm, 0..arm.opaque);
            }
            if let Some(item) = &self.hand_item {
                pass.set_bind_group(1, &self.atlas_bind_group, &[]);
                draw_range(&mut pass, item, 0..item.opaque);
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
        let readback = capture
            .as_ref()
            .map(|_| self.copy_out(&mut encoder, &target));
        self.queue
            .submit(ui_commands.into_iter().chain([encoder.finish()]));
        if let (Some(path), Some((buffer, padded))) = (capture, readback) {
            self.save_capture(&buffer, padded, &path)?;
        }
        match frame {
            Some(frame) => self.queue.present(frame),
            None => {
                self.device
                    .poll(wgpu::PollType::wait_indefinitely())
                    .context("waiting for an offscreen frame")?;
            }
        }
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

/// `sky_view_proj` is the view and projection without the camera's position.
fn globals(
    view_proj: glam::Mat4,
    sky_view_proj: glam::Mat4,
    camera: Vec3,
    env: &Environment,
) -> Globals {
    let (sunrise, strength) = env.sunrise.unwrap_or((Vec3::ZERO, 0.0));
    Globals {
        view_proj: view_proj.to_cols_array_2d(),
        inverse_view_proj: view_proj.inverse().to_cols_array_2d(),
        inverse_sky_view_proj: sky_view_proj.inverse().to_cols_array_2d(),
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
        extra: [
            env.moon_phase as f32,
            env.cloud_height,
            if env.sky { 0.0 } else { 1.0 },
            env.ambient,
        ],
        lighting: [env.brightness, 0.0, 0.0, 0.0],
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
    create_texture(device, queue, layout, label, image).1
}

/// A texture holding `image` that can be written to later, and a bind
/// group sampling it.
fn create_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    layout: &wgpu::BindGroupLayout,
    label: &str,
    image: &RgbaImage,
) -> (wgpu::Texture, wgpu::BindGroup) {
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
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
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
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
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
    });
    (texture, bind_group)
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

/// The mining crack: multiplies into the opaque world (src × dst twice),
/// depth-tested a little toward the camera so it shows on the block's
/// faces, without writing depth.
fn create_crack_pipeline(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    shader: &wgpu::ShaderModule,
    format: wgpu::TextureFormat,
) -> wgpu::RenderPipeline {
    let multiply = wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::Dst,
        dst_factor: wgpu::BlendFactor::Src,
        operation: wgpu::BlendOperation::Add,
    };
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("crack"),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("vs_main"),
            compilation_options: Default::default(),
            buffers: &[Some(vertex::Vertex::layout())],
        },
        primitive: wgpu::PrimitiveState {
            cull_mode: Some(wgpu::Face::Back),
            ..Default::default()
        },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: DEPTH_FORMAT,
            depth_write_enabled: Some(false),
            depth_compare: Some(wgpu::CompareFunction::LessEqual),
            stencil: Default::default(),
            bias: wgpu::DepthBiasState {
                constant: -2,
                slope_scale: -1.0,
                clamp: 0.0,
            },
        }),
        multisample: Default::default(),
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some("fs_crack"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: Some(wgpu::BlendState {
                    color: multiply,
                    alpha: wgpu::BlendComponent::OVER,
                }),
                write_mask: wgpu::ColorWrites::COLOR,
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
