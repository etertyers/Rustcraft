// Bloom (M57): bright HDR pixels glow softly. The chain halves the frame
// down to a few small levels (the first keeping only what's brighter than
// the threshold, so an ordinary daylit scene gets no haze), then adds each
// level back into the one above it with a soft tent filter. Mirrors
// bloom.rs's `prefilter`.

struct Params {
    // x, y: one texel of the input; z: the threshold (linear luminance);
    // w: 1 for the first level, which filters by the threshold.
    values: vec4<f32>,
};
@group(0) @binding(0) var source: texture_2d<f32>;
@group(0) @binding(1) var linear_sampler: sampler;
@group(0) @binding(2) var<uniform> params: Params;

struct Output {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> Output {
    let xy = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u)) * 2.0 - 1.0;
    var out: Output;
    out.clip_position = vec4<f32>(xy, 0.0, 1.0);
    out.uv = vec2<f32>(xy.x * 0.5 + 0.5, 0.5 - xy.y * 0.5);
    return out;
}

fn luminance(c: vec3<f32>) -> f32 {
    return dot(c, vec3<f32>(0.2126, 0.7152, 0.0722));
}

// What of `c` is above the threshold (all of its color, scaled).
fn prefilter(c: vec3<f32>) -> vec3<f32> {
    let l = luminance(c);
    return c * (max(l - params.values.z, 0.0) / max(l, 1e-4));
}

fn tap(uv: vec2<f32>, offset: vec2<f32>) -> vec3<f32> {
    let c = textureSampleLevel(source, linear_sampler, uv + offset * params.values.xy, 0.0).rgb;
    if params.values.w > 0.5 {
        return prefilter(c);
    }
    return c;
}

// Half size: four bilinear taps cover a 4 × 4 box.
@fragment
fn fs_down(in: Output) -> @location(0) vec4<f32> {
    let c = (tap(in.uv, vec2<f32>(-1.0, -1.0))
        + tap(in.uv, vec2<f32>(1.0, -1.0))
        + tap(in.uv, vec2<f32>(-1.0, 1.0))
        + tap(in.uv, vec2<f32>(1.0, 1.0))) * 0.25;
    return vec4<f32>(c, 1.0);
}
