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
pub mod visuals;

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
    /// A light level (0–15) every block is lit at least as much as: a
    /// mod's brightness override (Fullbright). 0 is off.
    pub min_light: f32,
    /// Glint Speed and Glint Strength as multiples of vanilla's defaults.
    pub glint: [f32; 2],
    /// The dimension's ambient light, lifting the dark (modern
    /// `DimensionType.ambientLight`: 0.1 in the Nether).
    pub ambient: f32,
    /// Rain or thunder, 0 or 1 (the Trailer look eases it over 10 s).
    pub rain: f32,
    /// Whether the dimension has a sun and moon to light and shade with
    /// (the Overworld: yes; the Nether and the End: no).
    pub sunlight: bool,
    /// What the Trailer look tints the ambient light with: warm in the
    /// Nether, a faint purple in the End, none elsewhere.
    pub ambient_tint: Vec3,
    /// Precipitation here is snow (a cold biome at the camera): rain
    /// doesn't wet the ground.
    pub snow: bool,
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

/// A Trailer shader: globals.wgsl, trailer_common.wgsl, then `body`.
macro_rules! trailer_shader {
    ($device:expr, $file:literal) => {
        $device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some($file),
            source: wgpu::ShaderSource::Wgsl(
                concat!(
                    include_str!("globals.wgsl"),
                    include_str!("trailer_common.wgsl"),
                    include_str!("sway.wgsl"),
                    include_str!($file)
                )
                .into(),
            ),
        })
    };
}

/// A chunk's mesh on the GPU.
/// A mesh on the GPU: opaque indices first, then translucent ones.
struct ChunkBuffers {
    vertices: wgpu::Buffer,
    /// The face attribute per vertex (slot 1), read only by the Trailer
    /// look's pipelines.
    faces: wgpu::Buffer,
    indices: wgpu::Buffer,
    opaque: u32,
    translucent: u32,
    /// The atlas generation its UVs point into.
    atlas: u32,
    /// The triangles with sky light on some vertex, as a second index
    /// buffer for the shadow maps: what's deep underground casts no
    /// shadow on anything the sun reaches. `None` for meshes drawn by the
    /// frame (mobs, items) and chunks with none.
    shadow_indices: Option<(wgpu::Buffer, u32)>,
    /// The lowest and highest glowing face: bloom has something to do
    /// when one is near the camera's height.
    glow_y: Option<(f32, f32)>,
    /// The lowest and highest vertex, for culling against a shadow map.
    y_range: (f32, f32),
}

/// Where the CPU's time went in one frame of `Renderer::render` (M68):
/// waiting for the next drawable, encoding the passes, submitting them, and
/// presenting (or, off screen, waiting for the GPU).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct CpuSplit {
    pub acquire: std::time::Duration,
    pub encode: std::time::Duration,
    pub submit: std::time::Duration,
    pub present: std::time::Duration,
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
    /// Where the CPU's time went in the last frame (M68).
    cpu: CpuSplit,
    /// Where to save the next frame as a PNG (for `--shot`).
    capture: Option<std::path::PathBuf>,
    /// The Clouds option isn't off.
    clouds: bool,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    depth_texture: wgpu::Texture,
    depth_view: wgpu::TextureView,
    /// The Vanilla look's pipelines (M53's, drawing into the surface) and
    /// the Trailer look's (the same shaders into the HDR target, reading
    /// the face buffer). Both are made at startup so switching is instant.
    vanilla: Pipelines,
    trailer: Pipelines,
    /// The Trailer look's targets and post passes (made on first use and
    /// on resize).
    trailer_frame: visuals::frame::TrailerFrame,
    /// Which look draws, and its settings.
    visuals: visuals::FrameVisuals,
    /// Pass timings, if the GPU has timestamp queries.
    profiler: Option<visuals::profiler::GpuProfiler>,
    /// The sun's shadow maps for the Trailer look's quality.
    shadow_pipeline: visuals::shadow::ShadowPipeline,
    shadows: visuals::shadow::ShadowMaps,
    /// Bumped whenever a chunk mesh or the atlas changes, so cached
    /// cascades know to be drawn again.
    mesh_revision: u64,
    frames: u64,
    /// Rain eased over 10 s, and the game time it was last eased at.
    rain: f32,
    rain_time: f32,
    /// Triangles drawn last frame: the main view and the shadow maps.
    triangles: (u64, u64),
    /// The crack over the block being mined, if any.
    crack: Option<ChunkBuffers>,
    /// One texture per mob kind.
    mob_bind_groups: Vec<wgpu::BindGroup>,
    /// Other players' skins, drawn by slots after the mob textures.
    guest_skins: Vec<wgpu::BindGroup>,
    /// This frame's mob meshes, by kind.
    mobs: Vec<(usize, ChunkBuffers)>,
    sun_bind_group: wgpu::BindGroup,
    moon_bind_group: wgpu::BindGroup,
    cloud_bind_group: wgpu::BindGroup,
    cloud_texture: wgpu::Texture,
    /// The opaque scene copied for the water, and whether the frame's
    /// world is drawn in two passes to take it.
    scene: SceneCopies,
    split_world: bool,
    /// A degenerate triangle drawn through every Trailer pipeline on the
    /// first Trailer frame, so none compiles in the middle of play (the
    /// first water in view cost a 50 ms frame).
    warm_mesh: ChunkBuffers,
    warmed: std::cell::Cell<bool>,
    /// The Trailer look's bloom, light shafts and FXAA, and which of the
    /// shafts and FXAA are drawn this frame (set by `prepare_sun`).
    bloom: visuals::bloom::Bloom,
    shafts: visuals::shafts::Shafts,
    fxaa: visuals::fxaa::Fxaa,
    shafts_active: bool,
    fxaa_active: bool,
    /// The light shafts' color times their strength.
    shaft_color: [f32; 3],
    /// The depth and HDR textures can be copied from (see `create_depth_view`).
    copy_usage: bool,
    /// The size the depth buffer and the world's targets are drawn at
    /// (Render Scale, M68): the window's, or a share of it with shaders on.
    scene_size: (u32, u32),
    /// The present modes the surface offers (for vsync on and off).
    present_modes: Vec<wgpu::PresentMode>,
    /// Stretches the world to the window when Render Scale is under 100%.
    upscale: visuals::upscale::Upscale,
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
    /// The atlas was over the GPU's limit and went up shrunk, so animated
    /// tiles can't be written into it.
    atlas_scaled: bool,
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
    /// How long making the Trailer look's pipelines took at startup.
    trailer_startup: std::time::Duration,
}

/// The pipelines that draw the world into one kind of target. The Vanilla
/// set is exactly M53's; the Trailer set draws the same shaders into the
/// HDR target with the face buffer bound.
struct Pipelines {
    blocks: wgpu::RenderPipeline,
    translucent: wgpu::RenderPipeline,
    /// The Trailer look's cheaper water for Low (no scene copy).
    translucent_low: Option<wgpu::RenderPipeline>,
    mob: wgpu::RenderPipeline,
    /// Depth only, for a boat's water mask.
    mask: wgpu::RenderPipeline,
    crack: wgpu::RenderPipeline,
    sky: wgpu::RenderPipeline,
    clouds: wgpu::RenderPipeline,
    /// Binds the face buffer (slot 1) with each mesh.
    faces: bool,
}

/// The layouts every pipeline set shares.
struct Layouts {
    globals: wgpu::BindGroupLayout,
    texture: wgpu::BindGroupLayout,
    blocks: wgpu::PipelineLayout,
    /// The Trailer look's block pipelines also read the sun and shadows
    /// (group 3).
    trailer_blocks: wgpu::PipelineLayout,
    shadow: visuals::shadow::ShadowPipeline,
}

impl Pipelines {
    /// Every world pipeline drawing into `format`; `faces` adds the face
    /// buffer to the block pipelines' vertex buffers.
    fn new(
        device: &wgpu::Device,
        layouts: &Layouts,
        shaders: &Shaders,
        format: wgpu::TextureFormat,
        faces: bool,
    ) -> Self {
        // The Trailer set reads the sun and shadows; Vanilla's is M53's.
        let (blocks, shader) = if faces {
            (&layouts.trailer_blocks, &shaders.trailer)
        } else {
            (&layouts.blocks, &shaders.blocks)
        };
        Self {
            blocks: create_block_pipeline(device, blocks, shader, format, false, true, faces),
            translucent: create_block_pipeline(device, blocks, shader, format, true, true, faces),
            translucent_low: faces.then(|| {
                create_block_pipeline_entry(
                    device,
                    blocks,
                    shader,
                    format,
                    (true, true, faces),
                    Some("fs_translucent_low"),
                )
            }),
            // Mob boxes are drawn from both sides, so face winding doesn't matter.
            mob: create_block_pipeline(device, blocks, shader, format, false, false, faces),
            mask: create_mask_pipeline(device, blocks, shader, format, faces),
            crack: create_crack_pipeline(device, blocks, shader, format, faces),
            sky: create_sky_pipeline(
                device,
                layouts,
                if faces {
                    &shaders.trailer_sky
                } else {
                    &shaders.sky
                },
                format,
                faces,
            ),
            clouds: create_cloud_pipeline(
                device,
                layouts,
                if faces {
                    &shaders.trailer_clouds
                } else {
                    &shaders.clouds
                },
                format,
                faces,
            ),
            faces,
        }
    }
}

/// The world shaders, compiled once for both looks.
struct Shaders {
    blocks: wgpu::ShaderModule,
    trailer: wgpu::ShaderModule,
    trailer_sky: wgpu::ShaderModule,
    trailer_clouds: wgpu::ShaderModule,
    sky: wgpu::ShaderModule,
    clouds: wgpu::ShaderModule,
}

impl Shaders {
    fn new(device: &wgpu::Device) -> Self {
        Self {
            blocks: world_shader!(device, "shader.wgsl"),
            trailer: trailer_shader!(device, "trailer.wgsl"),
            trailer_sky: trailer_shader!(device, "trailer_sky.wgsl"),
            trailer_clouds: trailer_shader!(device, "trailer_clouds.wgsl"),
            sky: world_shader!(device, "sky.wgsl"),
            clouds: world_shader!(device, "clouds.wgsl"),
        }
    }
}

impl Layouts {
    fn new(device: &wgpu::Device) -> Self {
        let globals = create_globals_layout(device);
        let texture = create_texture_layout(device);
        let blocks = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("blocks"),
            // The glint pattern is group 2 (the sky borrows it for the moon).
            bind_group_layouts: &[Some(&globals), Some(&texture), Some(&texture)],
            immediate_size: 0,
        });
        let shadow = visuals::shadow::ShadowPipeline::new(device, &texture, &vertex_buffers(true));
        let trailer_blocks = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("trailer blocks"),
            bind_group_layouts: &[
                Some(&globals),
                Some(&texture),
                Some(&texture),
                Some(&shadow.read_layout),
            ],
            immediate_size: 0,
        });
        Self {
            globals,
            texture,
            blocks,
            trailer_blocks,
            shadow,
        }
    }
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
        // Timestamps for the GPU profiler, where the GPU has them.
        let features =
            adapter.features() & (wgpu::Features::TIMESTAMP_QUERY | visuals::frame::HDR_FEATURE);
        log::info!(
            "GPU timestamp queries: {}",
            if features.is_empty() {
                "no (the profiler is off)"
            } else {
                "yes"
            }
        );
        // The adapter's own texture limit (wgpu's default asks for 8192),
        // which the GUI is told too.
        let max_side = adapter.limits().max_texture_dimension_2d;
        let required_limits = wgpu::Limits {
            max_texture_dimension_2d: max_side,
            ..wgpu::Limits::default()
        };
        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
                label: Some("rustcraft device"),
                required_features: features,
                required_limits,
                ..Default::default()
            }))?;
        let device_limit = device.limits().max_texture_dimension_2d;
        crate::startup::set_gpu_limit(device_limit);

        let mut config = surface
            .get_default_config(&adapter, size.width.max(1), size.height.max(1))
            .context("surface is not supported by the GPU adapter")?;
        // Prefer an sRGB format so colors are gamma-correct.
        let caps = surface.get_capabilities(&adapter);
        if let Some(format) = caps.formats.iter().find(|f| f.is_srgb()) {
            config.format = *format;
        }
        let present_modes = caps.present_modes.clone();
        config.present_mode = choose_present_mode(true, &present_modes);
        config.desired_maximum_frame_latency = MAX_FRAME_LATENCY;
        // egui blends in gamma space, so it draws through a non-sRGB view of the frame.
        config.view_formats = vec![config.format.remove_srgb_suffix()];
        surface.configure(&device, &config);
        let (depth_texture, depth_view) =
            create_depth_view(&device, (config.width, config.height), false);

        let layouts = Layouts::new(&device);
        let globals_layout = &layouts.globals;
        let globals_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("globals"),
            size: std::mem::size_of::<Globals>() as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let globals_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("globals"),
            layout: globals_layout,
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
            layout: globals_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: hand_globals_buffer.as_entire_binding(),
            }],
        });

        let texture_layout = layouts.texture.clone();
        let (atlas_texture, atlas_bind_group) =
            create_texture(&device, &queue, &texture_layout, "atlas", atlas.image());
        let sun_bind_group =
            create_texture_bind_group(&device, &queue, &texture_layout, "sun", &sky.sun);
        let moon_bind_group =
            create_texture_bind_group(&device, &queue, &texture_layout, "moon", &sky.moon);
        let (cloud_texture, cloud_bind_group) =
            create_texture(&device, &queue, &texture_layout, "clouds", &sky.clouds);
        let glint_bind_group =
            create_texture_bind_group(&device, &queue, &texture_layout, "glint", &sky.glint);
        let shaders = Shaders::new(&device);
        let hdr_format = visuals::frame::hdr_format(device.features());
        log::info!("Trailer look draws in {hdr_format:?}");
        let vanilla = Pipelines::new(&device, &layouts, &shaders, config.format, false);
        // The Trailer look's pipelines are made now too, so F12 never waits
        // for them; this is what they add to startup.
        let started = std::time::Instant::now();
        let trailer = Pipelines::new(
            &device,
            &layouts,
            &shaders,
            visuals::frame::hdr_format(device.features()),
            true,
        );
        let trailer_frame = visuals::frame::TrailerFrame::new(&device, &queue, config.format);
        let trailer_startup = started.elapsed();
        log::info!(
            "Trailer look's pipelines made in {:.1} ms",
            trailer_startup.as_secs_f64() * 1000.0
        );
        let profiler = visuals::profiler::GpuProfiler::new(&device, &queue);
        let mob_bind_groups = mob_textures
            .iter()
            .map(|image| create_texture_bind_group(&device, &queue, &texture_layout, "mob", image))
            .collect();

        let outline = Outline::new(
            &device,
            globals_layout,
            [config.format, hdr_format],
            DEPTH_FORMAT,
        );
        let egui_renderer = egui_wgpu::Renderer::new(
            &device,
            config.format.remove_srgb_suffix(),
            egui_wgpu::RendererOptions::default(),
        );

        let warm_mesh = {
            let buffer = |contents: &[u8], usage| {
                device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("warm"),
                    contents,
                    usage,
                })
            };
            ChunkBuffers {
                vertices: buffer(
                    bytemuck::cast_slice(&[<vertex::Vertex as bytemuck::Zeroable>::zeroed(); 3]),
                    wgpu::BufferUsages::VERTEX,
                ),
                faces: buffer(bytemuck::cast_slice(&[0u32; 3]), wgpu::BufferUsages::VERTEX),
                indices: buffer(
                    bytemuck::cast_slice(&[0u32, 1, 2]),
                    wgpu::BufferUsages::INDEX,
                ),
                opaque: 3,
                translucent: 0,
                atlas: 0,
                shadow_indices: None,
                glow_y: None,
                y_range: (0.0, 0.0),
            }
        };
        let bloom = visuals::bloom::Bloom::new(&device, hdr_format);
        let shafts = visuals::shafts::Shafts::new(
            &device,
            &layouts.trailer_blocks,
            &trailer_shader!(device, "visuals/shafts.wgsl"),
        );
        let fxaa = visuals::fxaa::Fxaa::new(&device, &queue, config.format);
        let upscale = visuals::upscale::Upscale::new(&device, config.format);
        let scene_size = (config.width, config.height);
        visuals::scale::set_window(config.width, config.height);
        let scene = SceneCopies::new(&device, hdr_format, 1, 1);
        let cloud_view = cloud_texture.create_view(&Default::default());
        let shadows = visuals::shadow::ShadowMaps::new(
            &device,
            &layouts.shadow,
            visuals::sun::ShadowConfig::of(visuals::Quality::Medium),
            &visuals::shadow::SceneViews {
                clouds: &cloud_view,
                color: &scene.color_view,
                depth: &scene.depth_view,
            },
        );
        Ok(Self {
            surface,
            offscreen_fallback: false,
            offscreen: None,
            capture: None,
            clouds: true,
            drew_offscreen: false,
            cpu: CpuSplit::default(),
            device,
            queue,
            config,
            depth_texture,
            depth_view,
            vanilla,
            trailer,
            trailer_frame,
            visuals: visuals::FrameVisuals::default(),
            profiler,
            shadows,
            shadow_pipeline: layouts.shadow.clone(),
            mesh_revision: 0,
            frames: 0,
            rain: 0.0,
            rain_time: 0.0,
            triangles: (0, 0),
            crack: None,
            mob_bind_groups,
            guest_skins: Vec::new(),
            mobs: Vec::new(),
            sun_bind_group,
            moon_bind_group,
            cloud_bind_group,
            cloud_texture,
            scene,
            split_world: false,
            warm_mesh,
            warmed: std::cell::Cell::new(false),
            bloom,
            shafts,
            fxaa,
            shafts_active: false,
            fxaa_active: false,
            shaft_color: [0.0; 3],
            copy_usage: false,
            scene_size,
            present_modes,
            upscale,
            glint_bind_group,
            globals_buffer,
            globals_bind_group,
            hand_globals_buffer,
            hand_globals_bind_group,
            hand_arm: None,
            hand_item: None,
            texture_layout,
            atlas_texture,
            atlas_scaled: crate::startup::shrink_factor(
                atlas.image().width(),
                atlas.image().height(),
                device_limit,
            ) > 1,
            atlas_bind_group,
            atlas_generation: 0,
            old_atlases: Vec::new(),
            chunks: FastMap::default(),
            entities: None,
            chunks_drawn: 0,
            outline,
            egui_renderer,
            trailer_startup,
        })
    }

    /// Makes group 3 of the Trailer pipelines again, after the clouds'
    /// picture or the scene copy changed.
    fn rebind_trailer(&mut self) {
        let cloud_view = self.cloud_texture.create_view(&Default::default());
        self.shadows.rebind(
            &self.device,
            &self.shadow_pipeline,
            &visuals::shadow::SceneViews {
                clouds: &cloud_view,
                color: &self.scene.color_view,
                depth: &self.scene.depth_view,
            },
        );
    }

    /// Which look draws from the next frame on.
    pub fn set_visuals(&mut self, visuals: visuals::FrameVisuals) {
        self.visuals = visuals;
    }

    /// How long making the Trailer look's pipelines took at startup.
    pub fn trailer_startup(&self) -> std::time::Duration {
        self.trailer_startup
    }

    /// The GPU profiler, if the GPU has timestamp queries.
    pub fn profiler(&mut self) -> Option<&mut visuals::profiler::GpuProfiler> {
        self.profiler.as_mut()
    }

    /// Swaps in new textures after the resource packs change. Chunk meshes
    /// made against the old atlas must be rebuilt.
    pub fn set_textures(&mut self, atlas: &Atlas, sky: &SkyImages, mob_textures: &[RgbaImage]) {
        self.mesh_revision += 1;
        let (device, queue, layout) = (&self.device, &self.queue, &self.texture_layout);
        let (texture, bind_group) = create_texture(device, queue, layout, "atlas", atlas.image());
        self.atlas_texture = texture;
        self.atlas_scaled = crate::startup::shrink_factor(
            atlas.image().width(),
            atlas.image().height(),
            self.device.limits().max_texture_dimension_2d,
        ) > 1;
        let old = std::mem::replace(&mut self.atlas_bind_group, bind_group);
        self.old_atlases.push((self.atlas_generation, old));
        self.atlas_generation += 1;
        self.sun_bind_group = create_texture_bind_group(device, queue, layout, "sun", &sky.sun);
        self.moon_bind_group = create_texture_bind_group(device, queue, layout, "moon", &sky.moon);
        let (cloud_texture, cloud_bind_group) =
            create_texture(device, queue, layout, "clouds", &sky.clouds);
        self.cloud_texture = cloud_texture;
        self.cloud_bind_group = cloud_bind_group;
        self.mob_bind_groups = mob_textures
            .iter()
            .map(|image| create_texture_bind_group(device, queue, layout, "mob", image))
            .collect();
        self.rebind_trailer();
    }

    /// Other players' skins, in the order of their slots.
    pub fn set_guest_skins(&mut self, skins: &[RgbaImage]) {
        let (device, queue, layout) = (&self.device, &self.queue, &self.texture_layout);
        self.guest_skins = skins
            .iter()
            .map(|image| create_texture_bind_group(device, queue, layout, "skin", image))
            .collect();
    }

    /// The mesh slot drawn with guest skin `index`.
    pub fn guest_skin_slot(&self, index: usize) -> usize {
        self.mob_bind_groups.len() + index
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
        if self.atlas_scaled {
            return;
        }
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
        self.mesh_revision += 1;
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
        let faces: &[u8] = bytemuck::cast_slice(&mesh.faces);
        let mut indices = Vec::with_capacity(mesh.indices.len() + mesh.translucent.len());
        indices.extend_from_slice(&mesh.indices);
        indices.extend_from_slice(&mesh.translucent);
        let indices: &[u8] = bytemuck::cast_slice(&indices);
        let fits = |buffer: &wgpu::Buffer, bytes: &[u8]| buffer.size() >= bytes.len() as u64;
        let buffer = |size: usize, usage| {
            self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: (size.next_power_of_two() as u64).max(256),
                usage: usage | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        };
        let (vertex_buffer, face_buffer, index_buffer) = match old {
            Some(old)
                if fits(&old.vertices, vertices)
                    && fits(&old.faces, faces)
                    && fits(&old.indices, indices) =>
            {
                (old.vertices, old.faces, old.indices)
            }
            _ => (
                buffer(vertices.len(), wgpu::BufferUsages::VERTEX),
                buffer(faces.len(), wgpu::BufferUsages::VERTEX),
                buffer(indices.len(), wgpu::BufferUsages::INDEX),
            ),
        };
        self.queue.write_buffer(&vertex_buffer, 0, vertices);
        self.queue.write_buffer(&face_buffer, 0, faces);
        self.queue.write_buffer(&index_buffer, 0, indices);
        ChunkBuffers {
            vertices: vertex_buffer,
            faces: face_buffer,
            indices: index_buffer,
            opaque: mesh.indices.len() as u32,
            translucent: mesh.translucent.len() as u32,
            atlas: self.atlas_generation,
            shadow_indices: None,
            glow_y: None,
            y_range: (0.0, 0.0),
        }
    }

    /// The shadow maps' index buffer for a chunk mesh, if it has any
    /// triangle the sun reaches.
    fn shadow_indices(&self, mesh: &Mesh) -> Option<(wgpu::Buffer, u32)> {
        let lit = &mesh.shadow_indices;
        (!lit.is_empty()).then(|| {
            let buffer = self
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("shadow indices"),
                    contents: bytemuck::cast_slice(lit),
                    usage: wgpu::BufferUsages::INDEX,
                });
            (buffer, lit.len() as u32)
        })
    }

    fn buffers(&self, label: &str, mesh: &Mesh) -> ChunkBuffers {
        let vertices = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(label),
                contents: bytemuck::cast_slice(&mesh.vertices),
                usage: wgpu::BufferUsages::VERTEX,
            });
        debug_assert_eq!(mesh.faces.len(), mesh.vertices.len());
        let faces = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(label),
                contents: bytemuck::cast_slice(&mesh.faces),
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
            faces,
            indices,
            opaque: mesh.indices.len() as u32,
            translucent: mesh.translucent.len() as u32,
            atlas: self.atlas_generation,
            shadow_indices: self.shadow_indices(mesh),
            glow_y: mesh.glow_y,
            y_range: mesh.y_range,
        }
    }

    /// Whether anything brighter than white could be on screen: the sun
    /// (or its glint on water) in front of the camera, or a chunk in view
    /// with glowing blocks. When nothing is, bloom would add nothing, so
    /// its passes are skipped (the picture is the same).
    fn something_glows(
        &self,
        camera: &Camera,
        environment: &Environment,
        frustum: &Frustum,
    ) -> bool {
        let sun = visuals::sun::sun_direction(environment.celestial_angle);
        if environment.sky
            && environment.sunlight
            && sun.y > -0.05
            && camera.forward().dot(sun) > 0.55
        {
            return true;
        }
        // A glow far above or below the camera is hidden by the ground.
        let reach = 64.0;
        self.chunks.iter().any(|(pos, chunk)| {
            let (x, z) = pos.min_block();
            chunk.glow_y.is_some_and(|(lo, hi)| {
                hi > camera.position.y - reach && lo < camera.position.y + reach
            }) && frustum.intersects_box(
                Vec3::new(x as f32, chunk.y_range.0, z as f32),
                Vec3::new(
                    x as f32 + CHUNK_SIZE as f32,
                    chunk.y_range.1,
                    z as f32 + CHUNK_SIZE as f32,
                ),
            )
        })
    }

    /// Whether a chunk in view has water, ice or glass to draw: the world
    /// is only split into two passes (and the scene copied) when so.
    fn any_translucent_in_view(&self, camera: &Camera, frustum: &Frustum) -> bool {
        let _ = camera;
        self.chunks.iter().any(|(pos, chunk)| {
            let (x, z) = pos.min_block();
            let min = Vec3::new(x as f32, chunk.y_range.0, z as f32);
            let max = Vec3::new(
                x as f32 + CHUNK_SIZE as f32,
                chunk.y_range.1,
                z as f32 + CHUNK_SIZE as f32,
            );
            chunk.translucent > 0 && frustum.intersects_box(min, max)
        })
    }

    fn draw_sky(&self, pass: &mut wgpu::RenderPass<'_>, set: &Pipelines) {
        pass.set_pipeline(&set.sky);
        pass.set_bind_group(1, &self.sun_bind_group, &[]);
        pass.set_bind_group(2, &self.moon_bind_group, &[]);
        pass.draw(0..3, 0..1);
    }

    fn draw_clouds(&self, pass: &mut wgpu::RenderPass<'_>, set: &Pipelines) {
        pass.set_pipeline(&set.clouds);
        pass.set_bind_group(1, &self.cloud_bind_group, &[]);
        pass.draw(0..6, 0..1);
    }

    /// Outlines `block`'s boxes from the next frame on, or nothing.
    pub fn set_outline(&mut self, target: Option<(IVec3, Vec<crate::world::shape::Cuboid>)>) {
        self.outline.set_target(target);
    }

    pub fn remove_chunk(&mut self, pos: ChunkPos) {
        self.mesh_revision += 1;
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
        self.mesh_revision += 1;
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
        self.config.present_mode = choose_present_mode(vsync, &self.present_modes);
        self.surface.configure(&self.device, &self.config);
    }

    pub fn resize(&mut self, size: PhysicalSize<u32>) {
        if size.width == 0 || size.height == 0 {
            return;
        }
        self.config.width = size.width;
        self.config.height = size.height;
        visuals::scale::set_window(size.width, size.height);
        self.surface.configure(&self.device, &self.config);
        self.scene_size = self.scene_size_for(self.visuals.trailer());
        (self.depth_texture, self.depth_view) =
            create_depth_view(&self.device, self.scene_size, self.copy_usage);
    }

    /// The size the world is drawn at: the window's, or with shaders on a
    /// share of it (Render Scale, M68).
    fn scene_size_for(&self, shaders: bool) -> (u32, u32) {
        visuals::scale::sizes(
            (self.config.width, self.config.height),
            shaders,
            self.visuals.render_scale,
        )
        .scene
    }

    /// The share of the window the world is drawn at, in percent (100 with
    /// shaders off).
    pub fn render_percent(&self) -> u32 {
        let (w, h) = (self.config.width, self.config.height);
        if self.visuals.trailer() {
            visuals::scale::percent(self.visuals.render_scale, w, h)
        } else {
            100
        }
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

    /// What the surface was set up with, for the log and `--perf-test`:
    /// the drawable against the window's size and scale, the present mode and
    /// frame latency, and the display's refresh rate as winit reports it.
    pub fn display_report(&self, window: &Window) -> String {
        let refresh = window
            .current_monitor()
            .and_then(|m| m.refresh_rate_millihertz())
            .map_or_else(
                || "unknown".to_owned(),
                |mhz| format!("{:.1} Hz", f64::from(mhz) / 1000.0),
            );
        let inner = window.inner_size();
        format!(
            "display: drawable {}x{}, window {}x{} pixels at scale factor {}, refresh {refresh}; \
             present mode {:?} (offered {:?}), frame latency {}, render scale {}%",
            self.config.width,
            self.config.height,
            inner.width,
            inner.height,
            window.scale_factor(),
            self.config.present_mode,
            self.present_modes,
            self.config.desired_maximum_frame_latency,
            self.render_percent(),
        )
    }

    /// Where the CPU's time went in the last frame.
    pub fn cpu_split(&self) -> CpuSplit {
        self.cpu
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
        let started = std::time::Instant::now();
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
        let acquired = std::time::Instant::now();
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
        // The profiler is taken out while its timestamp writes are borrowed.
        let mut profiler = self.profiler.take();
        if let Some(profiler) = profiler.as_mut() {
            profiler.begin_frame();
        }
        let trailer = self.visuals.trailer();
        // The water reads a copy of the opaque scene when it reflects or
        // refracts: only then do the depth and HDR textures need the flag.
        let plan = visuals::planned_passes(&self.visuals);
        let copy = trailer
            && (plan.contains(&"water") || plan.contains(&"shafts") || plan.contains(&"fxaa"));
        let scene = self.scene_size_for(trailer);
        if copy != self.copy_usage || scene != self.scene_size {
            self.copy_usage = copy;
            self.scene_size = scene;
            (self.depth_texture, self.depth_view) = create_depth_view(&self.device, scene, copy);
        }
        // Under 100% the world is drawn small and stretched to the window.
        let scaled = trailer && scene != (self.config.width, self.config.height);
        if trailer {
            self.trailer_frame
                .prepare(&self.device, scene.0, scene.1, copy);
            self.trailer_frame.prepare_ldr(
                &self.device,
                self.config.format,
                plan.contains(&"fxaa") || scaled,
                plan.contains(&"fxaa") && scaled,
            );
        }
        // The sun's light and shadow maps (before the world is drawn).
        let sun_frame = trailer.then(|| self.prepare_sun(environment));
        let mut shadow_triangles = 0;
        if let Some(sun_frame) = &sun_frame {
            shadow_triangles =
                self.draw_shadows(&mut encoder, profiler.as_mut(), camera, sun_frame);
        }
        let set = if trailer {
            &self.trailer
        } else {
            &self.vanilla
        };
        // Vanilla draws straight into the frame, as M53 did; Trailer into
        // its HDR target, then post passes into the frame.
        let world_view = match (trailer, self.trailer_frame.hdr_view()) {
            (true, Some(hdr)) => hdr,
            _ => &view,
        };
        let clear = if trailer {
            let [r, g, b] = SKY_COLOR.map(srgb_to_linear);
            wgpu::Color { r, g, b, a: 1.0 }
        } else {
            clear_color(self.config.format)
        };
        let (drawn, triangles) = self.draw_world(
            &mut encoder,
            world_view,
            set,
            clear,
            profiler.as_mut(),
            camera,
            environment,
            &frustum,
            outline,
        );
        self.chunks_drawn = drawn;
        self.triangles = (triangles, shadow_triangles);
        if capture.is_some() {
            log::info!(
                "triangles drawn: {triangles} in the main view, {shadow_triangles} in the shadow maps"
            );
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
            let timestamps = profiler.as_mut().and_then(|p| p.pass("hand"));
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("hand"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: world_view,
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
                timestamp_writes: timestamps,
                ..Default::default()
            });
            pass.set_bind_group(0, &self.hand_globals_bind_group, &[]);
            pass.set_bind_group(2, &self.glint_bind_group, &[]);
            if set.faces {
                pass.set_bind_group(3, &self.shadows.read_group, &[]);
            }
            pass.set_pipeline(&set.mob);
            if let Some(arm) = &self.hand_arm
                && let Some(skin) = self.mob_bind_groups.last()
            {
                pass.set_bind_group(1, skin, &[]);
                draw_range(&mut pass, arm, 0..arm.opaque, set.faces);
            }
            if let Some(item) = &self.hand_item {
                pass.set_bind_group(1, &self.atlas_bind_group, &[]);
                draw_range(&mut pass, item, 0..item.opaque, set.faces);
            }
        }

        if trailer {
            let size = scene;
            // Glow round what's brighter than white.
            let mut bloom: Vec<&wgpu::TextureView> = Vec::new();
            if plan.contains(&"bloom")
                && self.something_glows(camera, environment, &frustum)
                && let Some(hdr) = self.trailer_frame.hdr_view()
            {
                self.bloom.prepare(
                    &self.device,
                    self.trailer_frame.format(),
                    hdr,
                    size,
                    visuals::bloom::levels(self.visuals.quality),
                    self.trailer_frame.generation(),
                );
                self.bloom.run(&mut encoder, profiler.as_mut());
                bloom = self.bloom.levels();
            }
            // Light shafts, if the sun is up and they're on.
            let mut shafts = None;
            if self.shafts_active
                && let Some((_, scale)) = visuals::shafts::settings(self.visuals.quality)
            {
                self.shafts.prepare(&self.device, size, scale);
                self.shafts.run(
                    &mut encoder,
                    [
                        &self.globals_bind_group,
                        &self.atlas_bind_group,
                        &self.glint_bind_group,
                        &self.shadows.read_group,
                    ],
                    profiler.as_mut(),
                );
                shafts = self.shafts.result();
            }
            let tint = environment
                .sunrise
                .map_or(0.0, |(_, strength)| strength * 0.4);
            let mut params = visuals::frame::PostParams::new(tint, environment.min_light > 0.0);
            if !bloom.is_empty() {
                params = params.with_bloom(visuals::bloom::STRENGTH, bloom.len());
            }
            if shafts.is_some() {
                params = params.with_shafts(self.shaft_color);
            }
            // With FXAA, or a Render Scale under 100%, the picture goes to
            // a texture first.
            let target = match (self.fxaa_active || scaled, self.trailer_frame.ldr_view()) {
                (true, Some(ldr)) => ldr,
                _ => &view,
            };
            self.trailer_frame.post(
                &self.device,
                &self.queue,
                &mut encoder,
                target,
                &params,
                &bloom,
                shafts,
                profiler.as_mut(),
            );
            if self.fxaa_active
                && let Some(ldr) = self.trailer_frame.ldr_view()
            {
                let timestamps = profiler.as_mut().and_then(|p| p.pass("fxaa"));
                let out = if scaled {
                    self.trailer_frame.ldr_b_view().unwrap_or(&view)
                } else {
                    &view
                };
                self.fxaa.run(
                    &self.device,
                    &mut encoder,
                    ldr,
                    &self.scene.depth_view,
                    out,
                    timestamps,
                );
            }
            // The world, drawn small, stretched to the window; the UI
            // follows at full resolution.
            if scaled {
                let source = if self.fxaa_active {
                    self.trailer_frame.ldr_b_view()
                } else {
                    self.trailer_frame.ldr_view()
                };
                if let Some(source) = source {
                    let timestamps = profiler.as_mut().and_then(|p| p.pass("upscale"));
                    self.upscale
                        .run(&self.device, &mut encoder, source, &view, timestamps);
                }
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
            let timestamps = profiler.as_mut().and_then(|p| p.pass("ui"));
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
                    timestamp_writes: timestamps,
                    ..Default::default()
                })
                .forget_lifetime();
            self.egui_renderer
                .render(&mut pass, &ui.primitives, &screen);
        }
        if let Some(profiler) = profiler.as_mut() {
            profiler.end_frame(&mut encoder);
        }
        let readback = capture
            .as_ref()
            .map(|_| self.copy_out(&mut encoder, &target));
        let encoded = std::time::Instant::now();
        self.queue
            .submit(ui_commands.into_iter().chain([encoder.finish()]));
        if let Some(profiler) = profiler.as_mut() {
            profiler.after_submit();
        }
        let submitted = std::time::Instant::now();
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
        if let Some(profiler) = profiler.as_mut() {
            profiler.collect(&self.device);
        }
        self.profiler = profiler;
        self.cpu = CpuSplit {
            acquire: acquired - started,
            encode: encoded - acquired,
            submit: submitted - encoded,
            present: submitted.elapsed(),
        };
        Ok(true)
    }

    /// The world pass: sky, chunks, items, mobs, the crack, clouds, water
    /// and glass, the outline, drawn into `view` with `set`'s pipelines.
    /// Returns the chunks drawn.
    #[allow(clippy::too_many_arguments)]
    fn draw_world(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        set: &Pipelines,
        clear: wgpu::Color,
        mut profiler: Option<&mut visuals::profiler::GpuProfiler>,
        camera: &Camera,
        environment: &Environment,
        frustum: &Frustum,
        outline: u32,
    ) -> (usize, u64) {
        let faces = set.faces;
        // The Trailer water reads a copy of what's been drawn so far: the
        // world is drawn in two passes, with the copy between them.
        let split = faces && self.split_world && self.any_translucent_in_view(camera, frustum);
        // The shafts and FXAA read the opaque scene's depth: kept, and
        // copied after the world, before the hand clears it.
        let copy_depth = faces && (self.shafts_active || self.fxaa_active);
        let timestamps = profiler.as_mut().and_then(|p| p.pass("world"));
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("world"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(clear),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &self.depth_view,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: if split || copy_depth {
                        wgpu::StoreOp::Store
                    } else {
                        wgpu::StoreOp::Discard
                    },
                }),
                stencil_ops: None,
            }),
            timestamp_writes: timestamps,
            ..Default::default()
        });
        pass.set_bind_group(0, &self.globals_bind_group, &[]);
        if faces {
            pass.set_bind_group(3, &self.shadows.read_group, &[]);
        }
        // Vanilla draws the sky first; Trailer's goes after the opaque
        // things, where it shades only what they leave uncovered.
        if !faces {
            self.draw_sky(&mut pass, set);
        }
        pass.set_pipeline(&set.blocks);
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
        let triangles: u64 = visible
            .iter()
            .map(|(_, c)| u64::from(c.opaque + c.translucent) / 3)
            .sum();
        let mut bound = self.atlas_generation;
        for (_, chunk) in &visible {
            if chunk.atlas != bound {
                bound = chunk.atlas;
                pass.set_bind_group(1, self.atlas_for(bound), &[]);
            }
            draw_range(&mut pass, chunk, 0..chunk.opaque, faces);
        }
        pass.set_bind_group(1, &self.atlas_bind_group, &[]);
        if let Some(entities) = &self.entities {
            draw_range(&mut pass, entities, 0..entities.opaque, faces);
        }
        if !self.mobs.is_empty() {
            pass.set_pipeline(&set.mob);
            for (kind, mesh) in &self.mobs {
                // Past the mob textures come other players' skins.
                let group = match self.mob_bind_groups.get(*kind) {
                    Some(group) => group,
                    None => match self.guest_skins.get(*kind - self.mob_bind_groups.len()) {
                        Some(group) => group,
                        None => continue,
                    },
                };
                pass.set_bind_group(1, group, &[]);
                draw_range(&mut pass, mesh, 0..mesh.opaque, faces);
            }
            // Vanilla's water mask: a boat's inside, in depth only, so the
            // water drawn next stops at its rim.
            pass.set_pipeline(&set.mask);
            for (kind, mesh) in &self.mobs {
                if mesh.translucent == 0 {
                    continue;
                }
                if let Some(group) = self.mob_bind_groups.get(*kind) {
                    pass.set_bind_group(1, group, &[]);
                    let range = mesh.opaque..mesh.opaque + mesh.translucent;
                    draw_range(&mut pass, mesh, range, faces);
                }
            }
            pass.set_bind_group(1, &self.atlas_bind_group, &[]);
        }
        if let Some(crack) = &self.crack {
            pass.set_pipeline(&set.crack);
            draw_range(&mut pass, crack, 0..crack.opaque, faces);
        }
        if faces {
            self.draw_sky(&mut pass, set);
            pass.set_bind_group(1, &self.atlas_bind_group, &[]);
            pass.set_bind_group(2, &self.glint_bind_group, &[]);
            if !self.warmed.replace(true) {
                // Zero-area, so nothing shows; each pipeline is used once.
                for pipeline in [Some(&set.translucent), set.translucent_low.as_ref()]
                    .into_iter()
                    .flatten()
                    .chain([&set.blocks, &set.mob, &set.mask, &set.crack])
                {
                    pass.set_pipeline(pipeline);
                    draw_range(&mut pass, &self.warm_mesh, 0..3, true);
                }
                self.draw_clouds(&mut pass, set);
            }
        }
        // The water's own pass: after a copy of the opaque scene.
        let mut pass = if split {
            drop(pass);
            let size = wgpu::Extent3d {
                width: self.scene.size.0,
                height: self.scene.size.1,
                depth_or_array_layers: 1,
            };
            if let Some(hdr) = self.trailer_frame.hdr_texture() {
                encoder.copy_texture_to_texture(
                    hdr.as_image_copy(),
                    self.scene.color.as_image_copy(),
                    size,
                );
                encoder.copy_texture_to_texture(
                    self.depth_texture.as_image_copy(),
                    self.scene.depth.as_image_copy(),
                    size,
                );
            }
            let timestamps = profiler.as_mut().and_then(|p| p.pass("water"));
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("water"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
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
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Discard,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: timestamps,
                ..Default::default()
            });
            pass.set_bind_group(0, &self.globals_bind_group, &[]);
            pass.set_bind_group(2, &self.glint_bind_group, &[]);
            pass.set_bind_group(3, &self.shadows.read_group, &[]);
            pass
        } else {
            pass
        };
        // Clouds and water both blend: draw whichever is farther first.
        let above_clouds = camera.position.y > environment.cloud_height;
        if above_clouds && environment.sky && self.clouds {
            self.draw_clouds(&mut pass, set);
        }
        // Low's water has no scene copy to read: a lighter shader.
        let translucent = match &set.translucent_low {
            Some(low) if !self.split_world => low,
            _ => &set.translucent,
        };
        pass.set_pipeline(translucent);
        pass.set_bind_group(1, &self.atlas_bind_group, &[]);
        // Entities' shadows, under the water drawn next.
        if let Some(entities) = &self.entities {
            let translucent = entities.opaque..entities.opaque + entities.translucent;
            draw_range(&mut pass, entities, translucent, faces);
        }
        let mut bound = self.atlas_generation;
        for (_, chunk) in visible.iter().rev() {
            if chunk.atlas != bound {
                bound = chunk.atlas;
                pass.set_bind_group(1, self.atlas_for(bound), &[]);
            }
            let range = chunk.opaque..chunk.opaque + chunk.translucent;
            draw_range(&mut pass, chunk, range, faces);
        }
        if !above_clouds && environment.sky && self.clouds {
            self.draw_clouds(&mut pass, set);
        }
        if outline > 0 {
            self.outline.draw(&mut pass, outline, faces);
        }
        drop(pass);
        if copy_depth && !split {
            encoder.copy_texture_to_texture(
                self.depth_texture.as_image_copy(),
                self.scene.depth.as_image_copy(),
                wgpu::Extent3d {
                    width: self.scene.size.0,
                    height: self.scene.size.1,
                    depth_or_array_layers: 1,
                },
            );
        }
        (visible.len(), triangles)
    }

    /// The sun's light for the frame, the shadow maps' size for the
    /// quality and the water and cloud settings; eases rain over 10 s, and
    /// makes the scene copy when the water needs one.
    fn prepare_sun(&mut self, environment: &Environment) -> visuals::shadow::SunFrame {
        use visuals::{Effect, Quality, shadow::WaterFrame, sun};
        // Rain fades in and out over 10 s (200 ticks) of game time.
        let elapsed = (environment.time - self.rain_time).clamp(0.0, 40.0);
        self.rain_time = environment.time;
        let step = elapsed / 200.0;
        self.rain += (environment.rain - self.rain).clamp(-step, step);
        let visuals = self.visuals;
        let on = |effect| visuals.on(effect);
        let quality = visuals.quality;
        let config = sun::ShadowConfig::of(quality);
        // Under water (the sky is off but the dimension has a sun) the
        // sun only shows as caustics, and only on High.
        let underwater = environment.sunlight && !environment.sky;
        let caustics = underwater && on(Effect::Caustics);
        let shafts_cfg = visuals::shafts::settings(quality).filter(|_| on(Effect::LightShafts));
        // Shafts under water come from the surface: the sun's shadow maps
        // are drawn for them too.
        let sunlight =
            environment.sunlight && (environment.sky || caustics || shafts_cfg.is_some());
        let light = sun::light(environment.celestial_angle, self.rain, sunlight);
        let shaft_strength = sun::shaft_strength(&light, self.rain, sunlight);
        self.shafts_active = shafts_cfg.is_some() && shaft_strength > 0.01;
        self.fxaa_active = on(Effect::Fxaa);
        self.shaft_color = if underwater {
            // The surface's light in the water's own color (M51's fog).
            let fog = environment.fog_color;
            (fog * shaft_strength * 1.2).to_array()
        } else {
            (light.color / light.strength.max(0.01) * shaft_strength * 0.9).to_array()
        };
        let shadows = on(Effect::Shadows) && sunlight && light.strength > 0.02;
        // The water reads a copy of the opaque scene on Medium and High.
        let ssr_steps = match quality {
            Quality::Low => 0,
            Quality::Medium => 12,
            Quality::High => 32,
        } * u32::from(on(Effect::Reflections));
        let refraction = on(Effect::Refraction);
        self.split_world = ssr_steps > 0 || refraction;
        let size = self.scene_size_for(true);
        let mut rebind = self.shadows.config != config;
        let need_scene = self.split_world || self.shafts_active || self.fxaa_active;
        if need_scene && self.scene.size != size {
            let format = self.trailer_frame.format();
            self.scene = SceneCopies::new(&self.device, format, size.0, size.1);
            rebind = true;
        }
        if self.shadows.config != config {
            let cloud_view = self.cloud_texture.create_view(&Default::default());
            self.shadows = visuals::shadow::ShadowMaps::new(
                &self.device,
                &self.shadow_pipeline,
                config,
                &visuals::shadow::SceneViews {
                    clouds: &cloud_view,
                    color: &self.scene.color_view,
                    depth: &self.scene.depth_view,
                },
            );
        } else if rebind {
            self.rebind_trailer();
        }
        let clouds = self.clouds && environment.sky;
        let not_low = quality != Quality::Low;
        // The sky's color tints the ambient light under the open sky.
        let sky = environment.sky_color;
        let sky_tint = Vec3::ONE.lerp(sky / sky.max_element().max(0.01), 0.3);
        visuals::shadow::SunFrame {
            light,
            sunlight,
            shadows,
            config,
            pixel_locked: on(Effect::PixelLockedShadows),
            leaf_light: on(Effect::LeafLight),
            dimension_tint: environment.ambient_tint,
            sky_tint,
            atmos: {
                let mut atmos = self.atmos_frame(environment);
                if let (true, Some((steps, scale))) = (self.shafts_active, shafts_cfg) {
                    atmos.shafts = [
                        steps as f32,
                        visuals::shafts::REACH,
                        visuals::shafts::SCATTER,
                        scale as f32,
                    ];
                }
                atmos
            },
            water: WaterFrame {
                ssr_steps: if self.split_world { ssr_steps } else { 0 },
                refraction: self.split_world && refraction,
                caustics,
                snell: quality == Quality::High && underwater && on(Effect::Caustics),
                screen: [size.0 as f32, size.1 as f32],
                cloud_height: environment.cloud_height,
                clouds,
                cloud_reflections: clouds && not_low,
                cloud_shadows: clouds && on(Effect::CloudShadows),
                glass_metal: quality == Quality::High && on(Effect::Reflections),
                sky_effects: on(Effect::CloudShading),
                rain: self.rain,
            },
        }
    }

    /// The fog, wind, rain and glow settings of the frame.
    fn atmos_frame(&self, environment: &Environment) -> visuals::shadow::AtmosFrame {
        use visuals::Effect;
        let on = |effect| self.visuals.on(effect);
        // Under water the fog is M51's alone.
        let underwater = environment.fog_start < 0.0 || environment.fog_density > 0.0;
        let overworld_sky = environment.sunlight && environment.sky;
        // Thickest as the sun rises (the morning, sun in the east and low).
        let sun = visuals::sun::sun_direction(environment.celestial_angle);
        let morning = if sun.x > 0.0 {
            (1.0 - ((sun.y - 0.2).abs() / 0.5)).clamp(0.0, 1.0)
        } else {
            0.0
        };
        visuals::shadow::AtmosFrame {
            height_fog: if on(Effect::HeightFog) && overworld_sky && !underwater {
                0.0016
            } else {
                0.0
            },
            morning,
            dimension_fog: if on(Effect::HeightFog) && !environment.sunlight && !underwater {
                if environment.ambient > 0.05 && environment.ambient < 0.2 {
                    // The Nether: thick and warm, in the biome's fog color.
                    0.011
                } else {
                    // The End: a dark purple.
                    0.005
                }
            } else {
                0.0
            },
            sway: on(Effect::SwayingPlants),
            wetness: if on(Effect::Wetness) && !environment.snow && overworld_sky {
                self.rain
            } else {
                0.0
            },
            rain_rings: on(Effect::Wetness) && self.visuals.quality != visuals::Quality::Low,
            emissive: self.visuals.trailer(),
            wind: self.rain,
            shafts: [0.0; 4],
        }
    }

    /// Draws each cascade that needs it (the light moved more than 0.05°,
    /// the camera a shadow texel, a chunk changed, or mobs cast into it),
    /// then writes the frame's uniform. Returns the triangles drawn.
    fn draw_shadows(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        mut profiler: Option<&mut visuals::profiler::GpuProfiler>,
        camera: &Camera,
        frame: &visuals::shadow::SunFrame,
    ) -> u64 {
        use visuals::{Effect, shadow, sun};
        let mut triangles = 0;
        let mut to_draw = Vec::new();
        // Swaying leaves move their shadows: draw again every quarter
        // second of wind.
        let phase = if frame.atmos.sway {
            (self.rain_time / 5.0) as u32 + 1
        } else {
            0
        };
        let sway = [
            self.rain_time,
            frame.atmos.wind,
            f32::from(frame.atmos.sway),
            0.0,
        ];
        if frame.shadows {
            let config = frame.config;
            let mobs_cast = self.visuals.on(Effect::EntityShadows);
            // 0.05°.
            let same_light = 0.05f32.to_radians().cos();
            for i in 0..config.cascades {
                let (matrix, center) = sun::cascade(
                    camera.position,
                    config.reach[i],
                    frame.light.direction,
                    config.size,
                );
                let drawn = self.shadows.drawn[i];
                let unchanged = drawn.is_some_and(|d| {
                    d.direction.dot(frame.light.direction) > same_light
                        && d.center == center
                        && d.revision == self.mesh_revision
                        && d.phase == phase
                });
                // Mobs move every frame: the near cascade is drawn again.
                let skip = unchanged && !(i == 0 && mobs_cast);
                // Medium draws its far cascade every other frame.
                let waiting = config.alternate && i > 0 && drawn.is_some() && self.frames % 2 == 1;
                if skip || waiting {
                    continue;
                }
                self.shadows.write_cascade(&self.queue, i, &matrix, sway);
                self.shadows.drawn[i] = Some(shadow::Drawn {
                    matrix,
                    center,
                    direction: frame.light.direction,
                    revision: self.mesh_revision,
                    phase,
                });
                to_draw.push(i);
            }
        }
        self.frames += 1;
        for &i in &to_draw {
            let Some(drawn) = self.shadows.drawn[i] else {
                continue;
            };
            let timestamps = profiler.as_mut().and_then(|p| p.pass("shadow"));
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("shadow"),
                color_attachments: &[],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.shadows.layers[i],
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: timestamps,
                ..Default::default()
            });
            pass.set_pipeline(&self.shadow_pipeline.pipeline);
            pass.set_bind_group(
                0,
                &self.shadows.cascade_group,
                &[(shadow::CASCADE_STRIDE * i as u64) as u32],
            );
            pass.set_bind_group(1, &self.atlas_bind_group, &[]);
            let mut bound = self.atlas_generation;
            for (pos, chunk) in &self.chunks {
                let Some((indices, count)) = &chunk.shadow_indices else {
                    continue;
                };
                let (x, z) = pos.min_block();
                let min = Vec3::new(x as f32, chunk.y_range.0, z as f32);
                let max = Vec3::new(
                    x as f32 + CHUNK_SIZE as f32,
                    chunk.y_range.1,
                    z as f32 + CHUNK_SIZE as f32,
                );
                if !shadow::box_in_cascade(&drawn.matrix, min, max) {
                    continue;
                }
                if chunk.atlas != bound {
                    bound = chunk.atlas;
                    pass.set_bind_group(1, self.atlas_for(bound), &[]);
                }
                triangles += u64::from(*count) / 3;
                pass.set_vertex_buffer(0, chunk.vertices.slice(..));
                pass.set_vertex_buffer(1, chunk.faces.slice(..));
                pass.set_index_buffer(indices.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..*count, 0, 0..1);
            }
            if i == 0 && self.visuals.on(Effect::EntityShadows) {
                for (kind, mesh) in &self.mobs {
                    let group = match self.mob_bind_groups.get(*kind) {
                        Some(group) => group,
                        None => match self.guest_skins.get(*kind - self.mob_bind_groups.len()) {
                            Some(group) => group,
                            None => continue,
                        },
                    };
                    pass.set_bind_group(1, group, &[]);
                    triangles += u64::from(mesh.opaque) / 3;
                    draw_range(&mut pass, mesh, 0..mesh.opaque, true);
                }
            }
        }
        let uniform = shadow::TrailerUniform::new(frame, &self.shadows.matrices());
        self.queue
            .write_buffer(&self.shadows.uniform, 0, bytemuck::bytes_of(&uniform));
        triangles
    }
}

/// Draws `range` of a mesh's indices, if it isn't empty.
/// With `faces`, the face buffer is bound too (the Trailer pipelines).
fn draw_range(
    pass: &mut wgpu::RenderPass<'_>,
    mesh: &ChunkBuffers,
    range: std::ops::Range<u32>,
    faces: bool,
) {
    if range.is_empty() {
        return;
    }
    pass.set_vertex_buffer(0, mesh.vertices.slice(..));
    if faces {
        pass.set_vertex_buffer(1, mesh.faces.slice(..));
    }
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
        lighting: [env.brightness, env.glint[0], env.glint[1], env.min_light],
    }
}

/// The block pipelines' vertex buffers: the vertices, and for the
/// Trailer look the face buffer too.
fn vertex_buffers(faces: bool) -> Vec<Option<wgpu::VertexBufferLayout<'static>>> {
    let mut buffers = vec![Some(vertex::Vertex::layout())];
    if faces {
        buffers.push(Some(vertex::face_layout()));
    }
    buffers
}

/// The per-frame uniforms.
fn create_globals_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
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
    })
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
    // Over the device's limit, the picture is shrunk by a whole number
    // (a warning names it); wgpu would panic otherwise.
    let image = crate::startup::fit_gpu(label, image, device.limits().max_texture_dimension_2d);
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
    faces: bool,
) -> wgpu::RenderPipeline {
    create_block_pipeline_entry(
        device,
        layout,
        shader,
        format,
        (translucent, cull_back, faces),
        None,
    )
}

/// `create_block_pipeline` with `(translucent, cull_back, faces)` and a
/// fragment entry point other than the usual one.
fn create_block_pipeline_entry(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    shader: &wgpu::ShaderModule,
    format: wgpu::TextureFormat,
    (translucent, cull_back, faces): (bool, bool, bool),
    entry_override: Option<&'static str>,
) -> wgpu::RenderPipeline {
    let (label, entry, blend) = if translucent {
        (
            "translucent blocks",
            entry_override.unwrap_or("fs_translucent"),
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
            buffers: &vertex_buffers(faces),
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

/// A boat's water mask: depth, no color, drawn from both sides.
fn create_mask_pipeline(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    shader: &wgpu::ShaderModule,
    format: wgpu::TextureFormat,
    faces: bool,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("water mask"),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("vs_main"),
            compilation_options: Default::default(),
            buffers: &vertex_buffers(faces),
        },
        primitive: Default::default(),
        depth_stencil: Some(wgpu::DepthStencilState {
            format: DEPTH_FORMAT,
            depth_write_enabled: Some(true),
            depth_compare: Some(wgpu::CompareFunction::Less),
            stencil: Default::default(),
            bias: Default::default(),
        }),
        multisample: Default::default(),
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some("fs_main"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: None,
                write_mask: wgpu::ColorWrites::empty(),
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
    faces: bool,
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
            buffers: &vertex_buffers(faces),
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
    layouts: &Layouts,
    shader: &wgpu::ShaderModule,
    format: wgpu::TextureFormat,
    trailer: bool,
) -> wgpu::RenderPipeline {
    // The Trailer clouds read the cloud picture and the sun in group 3.
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("clouds"),
        bind_group_layouts: &if trailer {
            vec![
                Some(&layouts.globals),
                Some(&layouts.texture),
                Some(&layouts.texture),
                Some(&layouts.shadow.read_layout),
            ]
        } else {
            vec![Some(&layouts.globals), Some(&layouts.texture)]
        },
        immediate_size: 0,
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("clouds"),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: shader,
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
            module: shader,
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
    layouts: &Layouts,
    shader: &wgpu::ShaderModule,
    format: wgpu::TextureFormat,
    trailer: bool,
) -> wgpu::RenderPipeline {
    let mut groups = vec![
        Some(&layouts.globals),
        Some(&layouts.texture),
        Some(&layouts.texture),
    ];
    if trailer {
        groups.push(Some(&layouts.shadow.read_layout));
    }
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("sky"),
        bind_group_layouts: &groups,
        immediate_size: 0,
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("sky"),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("vs_main"),
            compilation_options: Default::default(),
            buffers: &[],
        },
        primitive: Default::default(),
        depth_stencil: Some(wgpu::DepthStencilState {
            format: DEPTH_FORMAT,
            depth_write_enabled: Some(false),
            // The Trailer sky is drawn after the world, at the far plane.
            depth_compare: Some(if trailer {
                wgpu::CompareFunction::LessEqual
            } else {
                wgpu::CompareFunction::Always
            }),
            stencil: Default::default(),
            bias: Default::default(),
        }),
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

/// Frames the GPU may be behind the CPU (wgpu's `desired_maximum_frame_latency`):
/// 2 keeps the CPU a frame ahead of the display without queueing more
/// (M68).
pub const MAX_FRAME_LATENCY: u32 = 2;

/// The present mode for vsync on or off from what the surface offers:
/// vsync on waits for the display's refresh (Fifo, which runs at the
/// display's full rate, 120 Hz on a ProMotion screen); off takes
/// Immediate, else Mailbox, and falls back to Fifo.
pub fn choose_present_mode(vsync: bool, offered: &[wgpu::PresentMode]) -> wgpu::PresentMode {
    use wgpu::PresentMode::*;
    if vsync {
        return Fifo;
    }
    [Immediate, Mailbox]
        .into_iter()
        .find(|mode| offered.contains(mode))
        .unwrap_or(Fifo)
}

/// The depth buffer; with `copy` it can be copied out for the water
/// (Trailer, Medium and High). The flag costs speed on Apple GPUs, so
/// it's only set while a frame needs the copy.
fn create_depth_view(
    device: &wgpu::Device,
    (width, height): (u32, u32),
    copy: bool,
) -> (wgpu::Texture, wgpu::TextureView) {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("depth"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: DEPTH_FORMAT,
        usage: if copy {
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC
        } else {
            wgpu::TextureUsages::RENDER_ATTACHMENT
        },
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    (texture, view)
}

/// A copy of the opaque scene's color and depth, taken before the water
/// is drawn (Medium and High): what refraction and screen-space
/// reflections read.
struct SceneCopies {
    color: wgpu::Texture,
    color_view: wgpu::TextureView,
    depth: wgpu::Texture,
    depth_view: wgpu::TextureView,
    size: (u32, u32),
}

impl SceneCopies {
    fn new(device: &wgpu::Device, format: wgpu::TextureFormat, width: u32, height: u32) -> Self {
        let size = wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        };
        let make = |label, format| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size,
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            })
        };
        let color = make("scene color", format);
        let depth = make("scene depth", DEPTH_FORMAT);
        Self {
            color_view: color.create_view(&Default::default()),
            depth_view: depth.create_view(&Default::default()),
            color,
            depth,
            size: (width, height),
        }
    }
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

    /// No blocking readback on the main thread (M68): the GPU profiler polls
    /// without waiting and reads results frames later; the only waits in the
    /// renderer are for a screenshot's copy and for a frame drawn off screen
    /// (when the window can't take one).
    #[test]
    fn nothing_but_captures_and_offscreen_frames_waits_for_the_gpu() {
        let profiler = include_str!("visuals/profiler.rs");
        assert!(
            !profiler.contains("wait_indefinitely"),
            "the profiler waits"
        );
        assert!(!profiler.contains("PollType::Wait"), "the profiler waits");
        assert!(profiler.contains("PollType::Poll"));
        let renderer = include_str!("mod.rs");
        // The two waits: `save_capture` and the offscreen branch of `draw`.
        let production = renderer.split("#[cfg(test)]").next().unwrap();
        assert_eq!(
            production.matches("wait_indefinitely").count(),
            2,
            "a new wait in the renderer: is it a capture or an offscreen frame?"
        );
        for file in [
            "visuals/bloom.rs",
            "visuals/shafts.rs",
            "visuals/water.rs",
            "visuals/shadow.rs",
        ] {
            let text = match file {
                "visuals/bloom.rs" => include_str!("visuals/bloom.rs"),
                "visuals/shafts.rs" => include_str!("visuals/shafts.rs"),
                "visuals/water.rs" => include_str!("visuals/water.rs"),
                _ => include_str!("visuals/shadow.rs"),
            };
            assert!(!text.contains("map_async"), "{file} reads back");
        }
    }

    #[test]
    fn vsync_off_takes_immediate_when_the_surface_has_it() {
        use wgpu::PresentMode::*;
        assert_eq!(choose_present_mode(false, &[Fifo, Immediate]), Immediate);
        assert_eq!(choose_present_mode(false, &[Fifo, Mailbox]), Mailbox);
        assert_eq!(choose_present_mode(false, &[Fifo]), Fifo);
        assert_eq!(choose_present_mode(false, &[]), Fifo);
        // Vsync on is always Fifo: the display's own rate, whatever it is.
        assert_eq!(choose_present_mode(true, &[Fifo, Immediate, Mailbox]), Fifo);
        assert_eq!(MAX_FRAME_LATENCY, 2);
    }

    /// A device with no window, or None where there's no GPU (CI).
    pub(crate) fn headless_device() -> Option<(wgpu::Device, wgpu::Queue)> {
        let instance =
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
        let adapter = pollster::block_on(instance.request_adapter(&Default::default())).ok()?;
        pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            required_features: adapter.features() & visuals::frame::HDR_FEATURE,
            ..Default::default()
        }))
        .ok()
    }

    /// The Trailer look's pipelines are made at startup (so F12 never
    /// waits); they must add less than 300 ms. Also checks every world
    /// shader compiles for both targets.
    #[test]
    fn trailer_pipelines_add_under_300_ms_to_startup() {
        let Some((device, _queue)) = headless_device() else {
            eprintln!("skipped: no GPU");
            return;
        };
        let layouts = Layouts::new(&device);
        let shaders = Shaders::new(&device);
        let format = wgpu::TextureFormat::Bgra8UnormSrgb;
        let _vanilla = Pipelines::new(&device, &layouts, &shaders, format, false);
        // The best of three, so other tests hogging the CPU don't fail it.
        let took = (0..3)
            .map(|_| {
                let started = std::time::Instant::now();
                let _trailer = Pipelines::new(
                    &device,
                    &layouts,
                    &shaders,
                    visuals::frame::hdr_format(device.features()),
                    true,
                );
                let _frame = visuals::frame::TrailerFrame::new(&device, &_queue, format);
                started.elapsed()
            })
            .min()
            .unwrap();
        println!(
            "Trailer pipelines made in {:.1} ms",
            took.as_secs_f64() * 1000.0
        );
        assert!(took < std::time::Duration::from_millis(300), "{took:?}");
    }

    /// M51's underwater fog (24 blocks at first, 57.6 after 100 ticks,
    /// 96 after 600) reaches the shaders unchanged: both looks read the
    /// same globals, and the Trailer shaders have no fog of their own.
    #[test]
    fn m51_fog_is_the_same_in_the_trailer_look() {
        use crate::game::environment::{environment, water_fog_end};
        use crate::world::biome::Biome;
        for (ticks, end) in [(0, 24.0), (100, 57.6), (600, 96.0)] {
            let mut env = environment(6000, 0.0, Biome::Ocean, Vec3::Z, None);
            env.fog_end = water_fog_end(Biome::Ocean, ticks);
            let g = globals(glam::Mat4::IDENTITY, glam::Mat4::IDENTITY, Vec3::ZERO, &env);
            assert!((g.fog[0] - end).abs() < 1e-3, "{ticks}: {}", g.fog[0]);
        }
        // The Trailer shaders use `fog_visibility` and add no fog formula.
        for source in [
            include_str!("trailer.wgsl"),
            include_str!("trailer_sky.wgsl"),
            include_str!("trailer_clouds.wgsl"),
            include_str!("trailer_common.wgsl"),
        ] {
            assert!(!source.contains("fog_density"), "a fog formula of its own");
        }
    }

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
