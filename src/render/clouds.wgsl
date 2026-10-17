// 1.0.0's "fast" clouds: one flat layer textured with clouds.png, 12
// blocks per texel, drifting west, 80% opaque, dimmed at night and fogged.
// (Prepended with globals.wgsl.)

@group(1) @binding(0) var cloud_texture: texture_2d<f32>;
@group(1) @binding(1) var cloud_sampler: sampler;

const BLOCKS_PER_TEXEL: f32 = 12.0;
const DRIFT_PER_TICK: f32 = 0.03;
const ALPHA: f32 = 0.8;

struct CloudOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) world: vec3<f32>,
};

// A square under the fog's reach, centered on the camera.
@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> CloudOutput {
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, -1.0), vec2<f32>(1.0, 1.0),
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, 1.0), vec2<f32>(-1.0, 1.0),
    );
    let reach = globals.fog.x;
    let xz = globals.camera.xz + corners[index] * reach;
    let world = vec3<f32>(xz.x, globals.extra.y, xz.y);
    var out: CloudOutput;
    out.clip_position = globals.view_proj * vec4<f32>(world, 1.0);
    out.world = world;
    return out;
}

@fragment
fn fs_main(in: CloudOutput) -> @location(0) vec4<f32> {
    let size = BLOCKS_PER_TEXEL * f32(textureDimensions(cloud_texture).x);
    let drift = globals.camera.w * DRIFT_PER_TICK;
    let uv = vec2<f32>(in.world.x + drift, in.world.z) / size;
    let texel = textureSample(cloud_texture, cloud_sampler, uv);
    if texel.a < 0.5 {
        discard;
    }
    // 1.0.0's World.drawClouds color.
    let a = globals.sky_color.w * 2.0 * 3.14159265;
    let light = clamp(cos(a) * 2.0 + 0.5, 0.0, 1.0);
    let tint = vec3<f32>(light * 0.9 + 0.1, light * 0.9 + 0.1, light * 0.85 + 0.15);
    let color = linear_to_srgb(texel.rgb) * tint;
    let visible = fog_visibility(distance(in.world, globals.camera.xyz));
    let fogged = mix(globals.fog_color.rgb, color, visible);
    return vec4<f32>(srgb_to_linear(fogged), ALPHA);
}
