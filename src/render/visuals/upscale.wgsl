// Render Scale's upscale (M68): the world drawn at a share of the window's
// size is stretched to the window with bilinear filtering, then sharpened
// by how far each pixel is from its four neighbors, kept inside their range
// so edges don't ring. The UI is drawn after this, at full resolution.

@group(0) @binding(0) var frame: texture_2d<f32>;
@group(0) @binding(1) var linear_sampler: sampler;

struct Output {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> Output {
    let uv = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
    var out: Output;
    out.clip_position = vec4<f32>(uv * 2.0 - 1.0, 0.0, 1.0);
    out.uv = vec2<f32>(uv.x, 1.0 - uv.y);
    return out;
}

const SHARPNESS: f32 = 0.4;

@fragment
fn fs_main(in: Output) -> @location(0) vec4<f32> {
    let texel = 1.0 / vec2<f32>(textureDimensions(frame));
    let c = textureSampleLevel(frame, linear_sampler, in.uv, 0.0).rgb;
    let n = textureSampleLevel(frame, linear_sampler, in.uv + vec2<f32>(0.0, -texel.y), 0.0).rgb;
    let s = textureSampleLevel(frame, linear_sampler, in.uv + vec2<f32>(0.0, texel.y), 0.0).rgb;
    let e = textureSampleLevel(frame, linear_sampler, in.uv + vec2<f32>(texel.x, 0.0), 0.0).rgb;
    let w = textureSampleLevel(frame, linear_sampler, in.uv + vec2<f32>(-texel.x, 0.0), 0.0).rgb;
    let lo = min(c, min(min(n, s), min(e, w)));
    let hi = max(c, max(max(n, s), max(e, w)));
    let blur = (n + s + e + w) * 0.25;
    let sharp = clamp(c + (c - blur) * SHARPNESS, lo, hi);
    return vec4<f32>(sharp, 1.0);
}
