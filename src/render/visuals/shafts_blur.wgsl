// Smooths the light shafts: a 4 × 4 box over the low-size picture (four
// bilinear taps), so the jitter's grain doesn't show when it's scaled up.

@group(0) @binding(0) var source: texture_2d<f32>;
@group(0) @binding(1) var linear_sampler: sampler;

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

@fragment
fn fs_main(in: Output) -> @location(0) vec4<f32> {
    let texel = 1.0 / vec2<f32>(textureDimensions(source));
    let a = textureSampleLevel(source, linear_sampler, in.uv + vec2<f32>(-1.0, -1.0) * texel, 0.0).r;
    let b = textureSampleLevel(source, linear_sampler, in.uv + vec2<f32>(1.0, -1.0) * texel, 0.0).r;
    let c = textureSampleLevel(source, linear_sampler, in.uv + vec2<f32>(-1.0, 1.0) * texel, 0.0).r;
    let d = textureSampleLevel(source, linear_sampler, in.uv + vec2<f32>(1.0, 1.0) * texel, 0.0).r;
    return vec4<f32>((a + b + c + d) * 0.25, 0.0, 0.0, 1.0);
}
