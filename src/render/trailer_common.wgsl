// What every Trailer shader shares (prepended after globals.wgsl): the
// frame's sun, shadow and water uniform, the group 3 bindings, and the
// sky and cloud functions the sky pass, the clouds pass and the water's
// reflections all use.

// Mirrors `TrailerUniform` in render/visuals/shadow.rs.
struct Trailer {
    cascade_vp: array<mat4x4<f32>, 3>,
    // Blocks each cascade reaches.
    reach: vec4<f32>,
    // A shadow texel in blocks per cascade; w: the map's size in texels.
    texel: vec4<f32>,
    // The depth range of each cascade's box in blocks.
    range: vec4<f32>,
    // xyz: toward the light; w: 1 where there's a sun or moon.
    sun_dir: vec4<f32>,
    // rgb: the light's color times its strength; w: its strength.
    sun_color: vec4<f32>,
    // x: cascades (0: no shadows); y: PCF taps; z: pixel-locked; w: hide
    // the entity shadow blobs.
    params: vec4<f32>,
    // x: sunlight through leaves; y: soft edges widening with distance;
    // z: ambient scale in the sun; w: direct scale.
    flags: vec4<f32>,
    // The dimension's ambient tint.
    tint: vec4<f32>,
    // rgb: the sky's tint on ambient light under the open sky; w: 1 when
    // the sun's glow and the clouds' shading are on.
    sky_tint: vec4<f32>,
    // x: screen-space reflection steps (0: sky only); y: refraction on;
    // z: caustics (we're under water); w: Snell's window on.
    water: vec4<f32>,
    // x, y: the screen's size in pixels; z: the clouds' height; w: 1 when
    // clouds are drawn.
    scene: vec4<f32>,
    // x: clouds in reflections; y: cloud shadows; z: glass and metal
    // reflect; w: rain, 0-1.
    extras: vec4<f32>,
    // x: height-fog density (0: off); y: more of it in the morning, 0-1;
    // z: the Nether's and the End's fog density; w: plants and leaves sway.
    atmos: vec4<f32>,
    // x: wet ground, 0-1; y: rain rings on water; z: glowing blocks are
    // drawn above 1.0 to bloom; w: wind, 0-1.
    weather: vec4<f32>,
    // The light shafts: x: steps; y: the farthest they look; z: scattering
    // per block; w: pixels of the frame one pixel of theirs covers.
    shaft: vec4<f32>,
};
@group(3) @binding(0) var<uniform> trailer: Trailer;
@group(3) @binding(1) var shadow_map: texture_depth_2d_array;
@group(3) @binding(2) var shadow_sampler: sampler_comparison;
// The clouds' picture (alpha is the cloud), bilinear and repeating for
// soft shadows; the opaque scene's color and depth, copied before the
// water is drawn.
@group(3) @binding(3) var cloud_texture: texture_2d<f32>;
@group(3) @binding(4) var cloud_sampler: sampler;
@group(3) @binding(5) var scene_color: texture_2d<f32>;
@group(3) @binding(6) var scene_depth: texture_depth_2d;
@group(3) @binding(7) var scene_sampler: sampler;

const PI: f32 = 3.14159265;
const BLOCKS_PER_CLOUD_TEXEL: f32 = 12.0;
const CLOUD_DRIFT_PER_TICK: f32 = 0.03;

// A cheap hash of a point, 0-1, with no sin (interleaved gradient noise).
fn hash2(p: vec3<f32>) -> f32 {
    let q = fract(dot(p, vec3<f32>(0.06711056, 0.00583715, 0.0137)));
    return fract(52.9829189 * q);
}

// ---- clouds ----

// The cloud (0-1, soft) over world x, z, drifting west.
fn cloud_alpha(xz: vec2<f32>) -> f32 {
    let size = BLOCKS_PER_CLOUD_TEXEL * f32(textureDimensions(cloud_texture).x);
    let drift = globals.camera.w * CLOUD_DRIFT_PER_TICK;
    let uv = vec2<f32>(xz.x + drift, xz.y) / size;
    return textureSampleLevel(cloud_texture, cloud_sampler, uv, 0.0).a;
}

// 1.0.0's cloud color by time of day (sRGB).
fn cloud_tint() -> vec3<f32> {
    let a = globals.sky_color.w * 2.0 * PI;
    let light = clamp(cos(a) * 2.0 + 0.5, 0.0, 1.0);
    return vec3<f32>(light * 0.9 + 0.1, light * 0.9 + 0.1, light * 0.85 + 0.15);
}

// ---- sky ----

// 1.0.0 draws the sky color on a plane this far above the camera, so fog
// thickens it toward the horizon.
const SKY_PLANE_HEIGHT: f32 = 16.0;

// The sky color seen along `dir` (sRGB): Vanilla's sky and sunrise glow,
// with a deeper blue overhead, a brighter haze at the horizon that
// matches the fog, and a soft glow round the sun or moon.
fn sky_gradient(dir: vec3<f32>) -> vec3<f32> {
    var color = globals.fog_color.rgb;
    if dir.y > 0.0 {
        let visible = fog_visibility(SKY_PLANE_HEIGHT / dir.y);
        color = mix(globals.fog_color.rgb, globals.sky_color.rgb, visible);
        color = mix(color, color * vec3<f32>(0.84, 0.90, 1.05), smoothstep(0.3, 1.0, dir.y) * 0.85);
    }
    // The sunrise or sunset glow, on the horizon on the sun's side.
    let a = globals.sky_color.w * 2.0 * PI;
    let sun = vec3<f32>(-sin(a), cos(a), 0.0);
    let flat_dir = normalize(vec3<f32>(dir.x, 0.0, dir.z) + vec3<f32>(0.0, 0.0, 1e-5));
    let toward_sun = max(dot(flat_dir, vec3<f32>(sign(sun.x), 0.0, 0.0)), 0.0);
    let near_horizon = 1.0 - smoothstep(0.0, 0.45, abs(dir.y - 0.05));
    let glow = globals.sunrise.a * toward_sun * toward_sun * near_horizon;
    color = mix(color, globals.sunrise.rgb, glow);
    if trailer.sun_dir.w > 0.5 && trailer.sky_tint.w > 0.5 {
        // Haze at the horizon, in the fog's color so the two meet.
        let haze = exp(-abs(dir.y) * 7.0) * 0.18;
        color = mix(color, min(globals.fog_color.rgb * 1.12 + vec3<f32>(0.03), vec3<f32>(1.0)), haze);
        // Light round the sun (or moon).
        // d^40 and d^8 by squaring (pow is slow in a full-screen shader).
        let d = max(dot(dir, trailer.sun_dir.xyz), 0.0);
        let d2 = d * d;
        let d4 = d2 * d2;
        let d8 = d4 * d4;
        let d16 = d8 * d8;
        let d32 = d16 * d16;
        color = color + trailer.sun_color.rgb * (d32 * d8 * 0.40 + d8 * 0.08);
    }
    return color;
}

// What a reflection from `origin` along `dir` sees of the sky (sRGB):
// the gradient, and on Medium and High the clouds.
fn sky_reflection(origin: vec3<f32>, dir: vec3<f32>) -> vec3<f32> {
    var color = sky_gradient(dir);
    if trailer.extras.x > 0.5 && trailer.scene.w > 0.5 && dir.y > 0.02 {
        let t = (trailer.scene.z - origin.y) / dir.y;
        if t > 0.0 && t < 3000.0 {
            let p = origin.xz + dir.xz * t;
            let cloud = cloud_alpha(p) * smoothstep(0.02, 0.2, dir.y) * 0.8;
            color = mix(color, cloud_tint(), cloud);
        }
    }
    return color;
}

// ---- water ----

// Mirrors render/visuals/water.rs.
const WAVE_AMPLITUDE: f32 = 0.05;
const WATER_F0: f32 = 0.02;
const GLASS_F0: f32 = 0.04;
const WATER_IOR: f32 = 1.333;
const ABSORPTION: f32 = 0.30;

fn wave_height(x: f32, z: f32, ticks: f32) -> f32 {
    let t = ticks / 20.0;
    let a = sin(x * 0.9 + z * 0.4 + t * 1.3);
    let b = sin(x * -0.5 + z * 1.1 - t * 1.0);
    let c = sin(x * 1.7 - z * 1.3 + t * 1.9);
    return WAVE_AMPLITUDE * (a * 0.45 + b * 0.35 + c * 0.2);
}

fn wave_slope(x: f32, z: f32, ticks: f32) -> vec2<f32> {
    let t = ticks / 20.0;
    let da = cos(x * 0.9 + z * 0.4 + t * 1.3);
    let db = cos(x * -0.5 + z * 1.1 - t * 1.0);
    let dc = cos(x * 1.7 - z * 1.3 + t * 1.9);
    let big = vec2<f32>(
        0.9 * da * 0.45 - 0.5 * db * 0.35 + 1.7 * dc * 0.2,
        0.4 * da * 0.45 + 1.1 * db * 0.35 - 1.3 * dc * 0.2,
    ) * WAVE_AMPLITUDE;
    let r1 = cos(x * 5.1 + z * 3.3 + t * 2.1) * 0.5;
    let r2 = cos(x * -3.7 + z * 5.9 - t * 1.7) * 0.5;
    return big + vec2<f32>(r1 * 5.1 - r2 * 3.7, r1 * 3.3 + r2 * 5.9) * 0.004;
}

fn fresnel(cos_theta: f32, f0: f32) -> f32 {
    let m = 1.0 - clamp(cos_theta, 0.0, 1.0);
    let m2 = m * m;
    return f0 + (1.0 - f0) * m2 * m2 * m;
}

fn depth_tint(depth: f32) -> f32 {
    return 1.0 - exp(-ABSORPTION * max(depth, 0.0));
}

fn reflection_sky_weight(sky: f32) -> f32 {
    let t = clamp((sky - 1.0) / 14.0, 0.0, 1.0);
    return t * t * (3.0 - 2.0 * t);
}

// The sun's glint: a sharp highlight where the reflected direction meets
// the light.
fn sun_glint(reflected: vec3<f32>, light: vec3<f32>) -> f32 {
    // Nothing but a tiny cone round the light: skip the pow elsewhere.
    let d = dot(reflected, light);
    if light.y <= 0.0 || d < 0.995 {
        return 0.0;
    }
    return pow(d, 1500.0) * 40.0 * (1.0 - clamp(trailer.extras.w, 0.0, 1.0));
}

// Moving light patterns under water: bright veins where two sine
// lattices cross.
fn caustic(p: vec2<f32>, t: f32) -> f32 {
    let a = sin(p.x * 2.1 + t * 1.1 + sin(p.y * 1.7 + t * 0.7));
    let b = sin(p.y * 2.3 - t * 0.9 + sin(p.x * 1.9 - t * 0.6));
    let c = 1.0 - abs(a * b);
    let c2 = c * c;
    return c2 * c2;
}

// ---- the scene copy (Medium and High) ----

fn world_from_depth(uv: vec2<f32>, depth: f32) -> vec3<f32> {
    let ndc = vec4<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, depth, 1.0);
    let w = globals.inverse_view_proj * ndc;
    return w.xyz / w.w;
}

// A ray from `origin` along `dir` marched against the opaque scene's
// depth: rgb (linear) of what it hits in the scene copy and, in w, how
// sure the hit is (0: nothing, the ray left the screen or the sky).
fn screen_reflection(origin: vec3<f32>, dir: vec3<f32>, steps: u32, jitter: f32) -> vec4<f32> {
    let size = trailer.scene.xy;
    let eye = globals.camera.xyz;
    var growth = 1.35;
    if steps >= 32u {
        growth = 1.15;
    }
    var t = 0.25 + jitter * 0.35;
    var step_length = 0.55;
    var last = 0.0;
    var hit_t = -1.0;
    for (var i = 0u; i < steps; i = i + 1u) {
        let p = origin + dir * t;
        let clip = globals.view_proj * vec4<f32>(p, 1.0);
        if clip.w <= 0.0 {
            break;
        }
        let ndc = clip.xyz / clip.w;
        let uv = vec2<f32>(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5);
        if uv.x < 0.0 || uv.y < 0.0 || uv.x > 1.0 || uv.y > 1.0 {
            break;
        }
        let d = textureLoad(scene_depth, vec2<i32>(uv * size), 0);
        if d < 1.0 {
            let gap = distance(p, eye) - distance(world_from_depth(uv, d), eye);
            if gap > 0.0 && gap < 0.6 + t * 0.15 {
                hit_t = t;
                break;
            }
        }
        last = t;
        t = t + step_length;
        step_length = step_length * growth;
    }
    if hit_t < 0.0 {
        return vec4<f32>(0.0);
    }
    // Refine the hit: bisect between the last miss and the hit.
    var lo = last;
    var hi = hit_t;
    for (var i = 0; i < 4; i = i + 1) {
        let mid = (lo + hi) * 0.5;
        let p = origin + dir * mid;
        let clip = globals.view_proj * vec4<f32>(p, 1.0);
        let ndc = clip.xyz / clip.w;
        let uv = vec2<f32>(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5);
        let d = textureLoad(scene_depth, vec2<i32>(clamp(uv, vec2<f32>(0.0), vec2<f32>(0.999)) * size), 0);
        if distance(p, eye) > distance(world_from_depth(uv, d), eye) {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    let p = origin + dir * hi;
    let clip = globals.view_proj * vec4<f32>(p, 1.0);
    let ndc = clip.xyz / clip.w;
    let uv = clamp(vec2<f32>(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5), vec2<f32>(0.0), vec2<f32>(1.0));
    let edge = min(min(uv.x, 1.0 - uv.x), min(uv.y, 1.0 - uv.y));
    let color = textureSampleLevel(scene_color, scene_sampler, uv, 0.0).rgb;
    return vec4<f32>(color, smoothstep(0.0, 0.08, edge));
}

// ---- fog ----

// Thin height fog (thicker low down, in the morning, over the sea) and the
// Nether's and the End's thick fog, mixed over what Vanilla's fog left.
// Never shortens Vanilla's fog distances: it only adds haze, and is off
// under water (M51's fog is alone there). `color` is linear.
fn extra_fog(color: vec3<f32>, world: vec3<f32>) -> vec3<f32> {
    var amount = 0.0;
    let distance_to = distance(world, globals.camera.xyz);
    if trailer.atmos.x > 0.0 {
        let low = exp(-max(world.y - 64.0, 0.0) * 0.035);
        amount = 1.0 - exp(-distance_to * trailer.atmos.x * low * (1.0 + 1.6 * trailer.atmos.y));
    }
    if trailer.atmos.z > 0.0 {
        amount = max(amount, 1.0 - exp(-distance_to * trailer.atmos.z));
    }
    return mix(color, srgb_to_linear(globals.fog_color.rgb), amount);
}
