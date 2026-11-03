//! The sun's shadow maps on the GPU (M55): one depth texture array with a
//! layer per cascade, the pipeline that draws chunk meshes and mobs into
//! it, and the bind group (group 3) the Trailer block shader reads.

use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3};

use super::sun::{Light, ShadowConfig};

pub const MAX_CASCADES: usize = 3;
/// Bytes between cascades in the cascade buffer (a dynamic offset's
/// alignment on the strictest GPU).
pub const CASCADE_STRIDE: u64 = 256;
pub const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

/// Mirrors `Trailer` in trailer.wgsl.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct TrailerUniform {
    pub cascade_vp: [[[f32; 4]; 4]; MAX_CASCADES],
    pub reach: [f32; 4],
    pub texel: [f32; 4],
    pub range: [f32; 4],
    pub sun_dir: [f32; 4],
    pub sun_color: [f32; 4],
    pub params: [f32; 4],
    pub flags: [f32; 4],
    pub tint: [f32; 4],
    pub sky_tint: [f32; 4],
    pub water: [f32; 4],
    pub scene: [f32; 4],
    pub extras: [f32; 4],
    pub atmos: [f32; 4],
    pub weather: [f32; 4],
    pub shaft: [f32; 4],
}

/// The fog, wind, rain and glow settings of a frame (M57).
#[derive(Debug, Clone, Copy, Default)]
pub struct AtmosFrame {
    /// Thin fog that thickens low down; 0 is off (and under water).
    pub height_fog: f32,
    /// Extra thickness in the morning, 0–1.
    pub morning: f32,
    /// The Nether's and the End's thick fog.
    pub dimension_fog: f32,
    /// Plants and leaves sway.
    pub sway: bool,
    /// Wet ground, 0–1: rain eased over 10 s, none in snow.
    pub wetness: f32,
    pub rain_rings: bool,
    /// Glowing blocks are drawn above 1.0 so they bloom.
    pub emissive: bool,
    /// Wind, 0–1.
    pub wind: f32,
    /// The shafts: steps, how far they look, scattering per block and the
    /// size of their picture next to the frame's (0 steps: off).
    pub shafts: [f32; 4],
}

/// The water, scene copy and cloud settings of a frame (M56).
#[derive(Debug, Clone, Copy, Default)]
pub struct WaterFrame {
    /// Screen-space reflection steps; 0 reflects only the sky.
    pub ssr_steps: u32,
    /// Water shows what's under it, bent and tinted by depth.
    pub refraction: bool,
    /// The camera is under water and sunlight makes caustics.
    pub caustics: bool,
    /// From below, the surface is Snell's window.
    pub snell: bool,
    /// The screen's size in pixels.
    pub screen: [f32; 2],
    pub cloud_height: f32,
    /// Clouds are drawn this frame.
    pub clouds: bool,
    pub cloud_reflections: bool,
    pub cloud_shadows: bool,
    /// Glass and metal reflect (High).
    pub glass_metal: bool,
    /// The sun's glow and the clouds' shading.
    pub sky_effects: bool,
    /// Rain, 0–1.
    pub rain: f32,
}

/// What the Trailer shaders need from the frame's settings.
#[derive(Debug, Clone, Copy)]
pub struct SunFrame {
    pub light: Light,
    /// There's a sun or moon here (the Overworld, above water).
    pub sunlight: bool,
    /// Shadows are drawn this frame.
    pub shadows: bool,
    pub config: ShadowConfig,
    pub pixel_locked: bool,
    pub leaf_light: bool,
    pub dimension_tint: Vec3,
    pub sky_tint: Vec3,
    pub water: WaterFrame,
    pub atmos: AtmosFrame,
}

impl TrailerUniform {
    /// The uniform for a frame, with the cascade matrices last rendered.
    pub fn new(frame: &SunFrame, matrices: &[Mat4; MAX_CASCADES]) -> Self {
        let c = &frame.config;
        let n = if frame.shadows { c.cascades } else { 0 };
        let range = |i: usize| 2.0 * c.reach[i] + super::sun::TOWARD_LIGHT;
        Self {
            cascade_vp: matrices.map(|m| m.to_cols_array_2d()),
            reach: [c.reach[0], c.reach[1], c.reach[2], 0.0],
            texel: [c.texel(0), c.texel(1), c.texel(2), c.size as f32],
            range: [range(0), range(1), range(2), 0.0],
            sun_dir: frame
                .light
                .direction
                .extend(f32::from(frame.sunlight))
                .to_array(),
            sun_color: frame.light.color.extend(frame.light.strength).to_array(),
            params: [
                n as f32,
                c.taps as f32,
                f32::from(frame.pixel_locked),
                f32::from(frame.shadows),
            ],
            flags: [
                f32::from(frame.leaf_light),
                f32::from(c.soft),
                super::sun::AMBIENT_SCALE,
                super::sun::DIRECT_SCALE,
            ],
            tint: frame.dimension_tint.extend(1.0).to_array(),
            sky_tint: frame
                .sky_tint
                .extend(f32::from(frame.water.sky_effects))
                .to_array(),
            water: [
                frame.water.ssr_steps as f32,
                f32::from(frame.water.refraction),
                f32::from(frame.water.caustics),
                f32::from(frame.water.snell),
            ],
            scene: [
                frame.water.screen[0],
                frame.water.screen[1],
                frame.water.cloud_height,
                f32::from(frame.water.clouds),
            ],
            extras: [
                f32::from(frame.water.cloud_reflections),
                f32::from(frame.water.cloud_shadows),
                f32::from(frame.water.glass_metal),
                frame.water.rain,
            ],
            atmos: [
                frame.atmos.height_fog,
                frame.atmos.morning,
                frame.atmos.dimension_fog,
                f32::from(frame.atmos.sway),
            ],
            weather: [
                frame.atmos.wetness,
                f32::from(frame.atmos.rain_rings),
                f32::from(frame.atmos.emissive),
                frame.atmos.wind,
            ],
            shaft: frame.atmos.shafts,
        }
    }
}

/// The layouts and pipeline shared by every quality.
#[derive(Clone)]
pub struct ShadowPipeline {
    pub pipeline: wgpu::RenderPipeline,
    pub cascade_layout: wgpu::BindGroupLayout,
    /// Group 3 of the Trailer pipelines.
    pub read_layout: wgpu::BindGroupLayout,
    cloud_sampler: wgpu::Sampler,
    scene_sampler: wgpu::Sampler,
}

/// The textures group 3 holds besides the shadow maps.
pub struct SceneViews<'a> {
    pub clouds: &'a wgpu::TextureView,
    pub color: &'a wgpu::TextureView,
    pub depth: &'a wgpu::TextureView,
}

impl ShadowPipeline {
    /// `atlas_layout` is the texture layout (group 1, for the alpha test);
    /// `vertex` the block pipelines' vertex buffers.
    pub fn new(
        device: &wgpu::Device,
        atlas_layout: &wgpu::BindGroupLayout,
        vertex: &[Option<wgpu::VertexBufferLayout<'static>>],
    ) -> Self {
        let cascade_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("shadow cascade"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: true,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let read_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("shadow read"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Depth,
                        view_dimension: wgpu::TextureViewDimension::D2Array,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Comparison),
                    count: None,
                },
                // The clouds' picture, and its bilinear sampler.
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
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                // The opaque scene's color and depth, copied before the
                // water is drawn, and a clamping sampler for the color.
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
                        sample_type: wgpu::TextureSampleType::Depth,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 7,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let cloud_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("cloud bilinear"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let scene_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("scene"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("shadow.wgsl"),
            source: wgpu::ShaderSource::Wgsl(
                concat!(include_str!("../sway.wgsl"), include_str!("shadow.wgsl")).into(),
            ),
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("shadow"),
            bind_group_layouts: &[Some(&cascade_layout), Some(atlas_layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("shadow"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: vertex,
            },
            // Plants and leaves are seen from both sides, and so are mobs.
            primitive: Default::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Less),
                stencil: Default::default(),
                // Slope-scaled, so steep faces get more.
                bias: wgpu::DepthBiasState {
                    constant: 0,
                    slope_scale: 1.5,
                    clamp: 0.0,
                },
            }),
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[],
            }),
            multiview_mask: None,
            cache: None,
        });
        Self {
            pipeline,
            cascade_layout,
            read_layout,
            cloud_sampler,
            scene_sampler,
        }
    }
}

/// What a cascade was last drawn with, to skip drawing it again.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Drawn {
    pub matrix: Mat4,
    pub center: [f32; 2],
    pub direction: Vec3,
    /// The renderer's mesh revision: changes when any chunk does.
    pub revision: u64,
    /// Which quarter-second of wind it was drawn in (0 with sway off), so
    /// moving leaves move their shadows.
    pub phase: u32,
}

/// The shadow maps of one quality.
pub struct ShadowMaps {
    pub config: ShadowConfig,
    pub layers: Vec<wgpu::TextureView>,
    cascade_buffer: wgpu::Buffer,
    pub cascade_group: wgpu::BindGroup,
    pub uniform: wgpu::Buffer,
    /// Group 3 for the Trailer pipelines.
    pub read_group: wgpu::BindGroup,
    pub drawn: [Option<Drawn>; MAX_CASCADES],
    array: wgpu::TextureView,
    compare: wgpu::Sampler,
}

impl ShadowMaps {
    pub fn new(
        device: &wgpu::Device,
        pipeline: &ShadowPipeline,
        config: ShadowConfig,
        views: &SceneViews,
    ) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("shadow maps"),
            size: wgpu::Extent3d {
                width: config.size,
                height: config.size,
                depth_or_array_layers: MAX_CASCADES as u32,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: DEPTH_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let layers = (0..MAX_CASCADES as u32)
            .map(|layer| {
                texture.create_view(&wgpu::TextureViewDescriptor {
                    label: Some("shadow layer"),
                    dimension: Some(wgpu::TextureViewDimension::D2),
                    base_array_layer: layer,
                    array_layer_count: Some(1),
                    ..Default::default()
                })
            })
            .collect();
        let array = texture.create_view(&wgpu::TextureViewDescriptor {
            label: Some("shadow array"),
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            ..Default::default()
        });
        let cascade_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("shadow cascades"),
            size: CASCADE_STRIDE * MAX_CASCADES as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let cascade_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("shadow cascades"),
            layout: &pipeline.cascade_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &cascade_buffer,
                    offset: 0,
                    size: wgpu::BufferSize::new(80),
                }),
            }],
        });
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("trailer uniform"),
            size: std::mem::size_of::<TrailerUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("shadow compare"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            compare: Some(wgpu::CompareFunction::LessEqual),
            ..Default::default()
        });
        let read_group = read_group(device, pipeline, &uniform, &array, &sampler, views);
        Self {
            config,
            layers,
            cascade_buffer,
            cascade_group,
            uniform,
            read_group,
            drawn: [None; MAX_CASCADES],
            array,
            compare: sampler,
        }
    }

    /// Makes group 3 again, after the clouds' picture or the scene copy
    /// changed.
    pub fn rebind(&mut self, device: &wgpu::Device, pipeline: &ShadowPipeline, views: &SceneViews) {
        self.read_group = read_group(
            device,
            pipeline,
            &self.uniform,
            &self.array,
            &self.compare,
            views,
        );
    }

    /// Writes cascade `i`'s matrix and sway (time, wind, on) where the
    /// shadow pass reads them.
    pub fn write_cascade(&self, queue: &wgpu::Queue, i: usize, matrix: &Mat4, sway: [f32; 4]) {
        let mut bytes = [0u8; 80];
        bytes[..64].copy_from_slice(bytemuck::bytes_of(&matrix.to_cols_array_2d()));
        bytes[64..].copy_from_slice(bytemuck::bytes_of(&sway));
        queue.write_buffer(&self.cascade_buffer, CASCADE_STRIDE * i as u64, &bytes);
    }

    /// The matrices last drawn, identity for a cascade never drawn.
    pub fn matrices(&self) -> [Mat4; MAX_CASCADES] {
        std::array::from_fn(|i| self.drawn[i].map_or(Mat4::IDENTITY, |d| d.matrix))
    }
}

fn read_group(
    device: &wgpu::Device,
    pipeline: &ShadowPipeline,
    uniform: &wgpu::Buffer,
    array: &wgpu::TextureView,
    compare: &wgpu::Sampler,
    views: &SceneViews,
) -> wgpu::BindGroup {
    let entry = |binding, resource| wgpu::BindGroupEntry { binding, resource };
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("shadow read"),
        layout: &pipeline.read_layout,
        entries: &[
            entry(0, uniform.as_entire_binding()),
            entry(1, wgpu::BindingResource::TextureView(array)),
            entry(2, wgpu::BindingResource::Sampler(compare)),
            entry(3, wgpu::BindingResource::TextureView(views.clouds)),
            entry(4, wgpu::BindingResource::Sampler(&pipeline.cloud_sampler)),
            entry(5, wgpu::BindingResource::TextureView(views.color)),
            entry(6, wgpu::BindingResource::TextureView(views.depth)),
            entry(7, wgpu::BindingResource::Sampler(&pipeline.scene_sampler)),
        ],
    })
}

/// Whether the box `min..max` (world space) can touch a cascade's box:
/// its corners seen from the light aren't all off one side. Casters on
/// the light's side of the box count (z isn't tested on the near side).
pub fn box_in_cascade(matrix: &Mat4, min: Vec3, max: Vec3) -> bool {
    let mut outside = [true; 5];
    for i in 0..8 {
        let corner = Vec3::new(
            if i & 1 == 0 { min.x } else { max.x },
            if i & 2 == 0 { min.y } else { max.y },
            if i & 4 == 0 { min.z } else { max.z },
        );
        let p = matrix.project_point3(corner);
        outside[0] &= p.x < -1.0;
        outside[1] &= p.x > 1.0;
        outside[2] &= p.y < -1.0;
        outside[3] &= p.y > 1.0;
        outside[4] &= p.z > 1.0;
    }
    !outside.iter().any(|&o| o)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::visuals::{Quality, sun};

    #[test]
    fn the_uniform_is_what_the_shader_expects() {
        // Three matrices and nine vec4s.
        assert_eq!(std::mem::size_of::<TrailerUniform>(), 3 * 64 + 15 * 16);
        assert_eq!(std::mem::size_of::<TrailerUniform>() % 16, 0);
    }

    #[test]
    fn cull_keeps_boxes_in_the_cascade_and_drops_the_rest() {
        let light = sun::light(0.05, 0.0, true).direction;
        let center = Vec3::new(40.0, 70.0, 40.0);
        let (m, _) = sun::cascade(center, 24.0, light, 2048);
        // A chunk round the center, and one on the sun's side of it.
        assert!(box_in_cascade(
            &m,
            center - Vec3::splat(8.0),
            center + Vec3::splat(8.0)
        ));
        let up = center + light * 100.0;
        assert!(box_in_cascade(
            &m,
            up - Vec3::splat(8.0),
            up + Vec3::splat(8.0)
        ));
        // Far to the side, or past the far side of the box (away from the
        // light).
        let side = center + Vec3::new(0.0, 0.0, 300.0);
        assert!(!box_in_cascade(
            &m,
            side - Vec3::splat(8.0),
            side + Vec3::splat(8.0)
        ));
        let behind = center - light * 200.0;
        assert!(!box_in_cascade(
            &m,
            behind - Vec3::splat(8.0),
            behind + Vec3::splat(8.0)
        ));
    }

    #[test]
    fn the_uniform_has_no_cascades_without_shadows() {
        let frame = SunFrame {
            light: sun::light(0.0, 0.0, true),
            sunlight: true,
            shadows: false,
            config: ShadowConfig::of(Quality::Medium),
            pixel_locked: true,
            leaf_light: true,
            dimension_tint: Vec3::ONE,
            sky_tint: Vec3::ONE,
            water: WaterFrame::default(),
            atmos: AtmosFrame::default(),
        };
        let u = TrailerUniform::new(&frame, &[Mat4::IDENTITY; MAX_CASCADES]);
        assert_eq!(u.params[0], 0.0);
        let on = TrailerUniform::new(
            &SunFrame {
                shadows: true,
                ..frame
            },
            &[Mat4::IDENTITY; 3],
        );
        assert_eq!((on.params[0], on.params[1]), (2.0, 8.0));
    }
}
