// The sky behind everything: the sky color fading into fog toward the
// horizon, the sunrise/sunset glow, stars, the sun and the moon, following
// 1.0.0's RenderGlobal.renderSky. (Prepended with globals.wgsl.)

@group(1) @binding(0) var sun_texture: texture_2d<f32>;
@group(1) @binding(1) var sun_sampler: sampler;
@group(2) @binding(0) var moon_texture: texture_2d<f32>;
@group(2) @binding(1) var moon_sampler: sampler;

struct SkyOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) ndc: vec2<f32>,
};

// One triangle covering the screen.
@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> SkyOutput {
    let xy = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u)) * 2.0 - 1.0;
    var out: SkyOutput;
    out.clip_position = vec4<f32>(xy, 0.0, 1.0);
    out.ndc = xy;
    return out;
}

const PI: f32 = 3.14159265;
// 1.0.0 draws the sky color on a plane this far above the camera, so fog
// thickens it toward the horizon.
const SKY_PLANE_HEIGHT: f32 = 16.0;
// Sun and moon half-sizes at their distance of 100 blocks.
const SUN_SIZE: f32 = 0.30;
const MOON_SIZE: f32 = 0.20;

fn hash3(p: vec3<f32>) -> f32 {
    return fract(sin(dot(p, vec3<f32>(127.1, 311.7, 74.7))) * 43758.5453);
}

// `dir` turned back by the sky's rotation, so stars move with the sun.
fn celestial(dir: vec3<f32>) -> vec3<f32> {
    let a = globals.sky_color.w * 2.0 * PI;
    let turned = vec3<f32>(dir.z, dir.y, -dir.x);
    return vec3<f32>(
        turned.x,
        turned.y * cos(a) + turned.z * sin(a),
        -turned.y * sin(a) + turned.z * cos(a),
    );
}

fn stars(dir: vec3<f32>) -> f32 {
    let p = celestial(dir) * 90.0;
    let cell = floor(p);
    if hash3(cell) < 0.996 {
        return 0.0;
    }
    let center = cell + 0.5 + (vec3<f32>(hash3(cell + 1.0), hash3(cell + 2.0), hash3(cell + 3.0)) - 0.5) * 0.5;
    let size = 0.12 + hash3(cell + 4.0) * 0.12;
    return step(length(p - center), size);
}

// Where `dir` hits a square of half-size `size` facing the camera along
// `toward`, as 0–1 texture coordinates, or a negative value if it misses.
fn billboard(dir: vec3<f32>, toward: vec3<f32>, size: f32) -> vec2<f32> {
    let along = dot(dir, toward);
    if along <= 0.0 {
        return vec2<f32>(-1.0);
    }
    let across = vec3<f32>(0.0, 0.0, 1.0);
    let up = cross(across, toward);
    let local = vec2<f32>(dot(dir, up), dot(dir, across)) / along / size;
    if abs(local.x) > 1.0 || abs(local.y) > 1.0 {
        return vec2<f32>(-1.0);
    }
    return local * 0.5 + 0.5;
}

@fragment
fn fs_main(in: SkyOutput) -> @location(0) vec4<f32> {
    // The view ray through this pixel, from a matrix that turns but doesn't
    // move: from a point on the near plane to one about 50 blocks out, so
    // the walking bob's shift can't swing it.
    let near = globals.inverse_sky_view_proj * vec4<f32>(in.ndc, 0.0, 1.0);
    let far = globals.inverse_sky_view_proj * vec4<f32>(in.ndc, 0.999, 1.0);
    let dir = normalize(far.xyz / far.w - near.xyz / near.w);

    // Without a sky (the Nether) it's fog all round.
    if globals.extra.z > 0.5 {
        return vec4<f32>(srgb_to_linear(globals.fog_color.rgb), 1.0);
    }

    // Sky color overhead, fog toward and below the horizon.
    var color = globals.fog_color.rgb;
    if dir.y > 0.0 {
        let visible = fog_visibility(SKY_PLANE_HEIGHT / dir.y);
        color = mix(globals.fog_color.rgb, globals.sky_color.rgb, visible);
    }

    // The sunrise or sunset glow, on the horizon on the sun's side.
    let a = globals.sky_color.w * 2.0 * PI;
    let sun = vec3<f32>(-sin(a), cos(a), 0.0);
    let flat_dir = normalize(vec3<f32>(dir.x, 0.0, dir.z) + vec3<f32>(0.0, 0.0, 1e-5));
    let toward_sun = max(dot(flat_dir, vec3<f32>(sign(sun.x), 0.0, 0.0)), 0.0);
    let near_horizon = 1.0 - smoothstep(0.0, 0.45, abs(dir.y - 0.05));
    let glow = globals.sunrise.a * toward_sun * toward_sun * near_horizon;
    color = mix(color, globals.sunrise.rgb, glow);

    // Stars, the sun and the moon add light, as in 1.0.0; they don't
    // show below the horizon or through underwater fog.
    if dir.y > 0.0 && globals.fog.y == 0.0 {
        color += vec3<f32>(stars(dir) * globals.fog.w);
        let sun_uv = billboard(dir, sun, SUN_SIZE);
        if sun_uv.x >= 0.0 {
            color += textureSampleLevel(sun_texture, sun_sampler, sun_uv, 0.0).rgb;
        }
        let moon_uv = billboard(dir, -sun, MOON_SIZE);
        if moon_uv.x >= 0.0 {
            let phase = u32(globals.extra.x);
            let cell = vec2<f32>(f32(phase % 4u), f32((phase / 4u) % 2u));
            let uv = (cell + moon_uv) / vec2<f32>(4.0, 2.0);
            color += textureSampleLevel(moon_texture, moon_sampler, uv, 0.0).rgb;
        }
    }
    return vec4<f32>(srgb_to_linear(min(color, vec3<f32>(1.0))), 1.0);
}
