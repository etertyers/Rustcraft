// The sun's shadow maps (M55): chunk meshes and mobs drawn from the
// light into a depth map, cutting out leaves' and plants' see-through
// pixels so light falls through their holes. Fast graphics' solid leaves
// (face flag 4) cast whole.

struct Cascade {
    view_proj: mat4x4<f32>,
};
@group(0) @binding(0) var<uniform> cascade: Cascade;
@group(1) @binding(0) var atlas_texture: texture_2d<f32>;
@group(1) @binding(1) var atlas_sampler: sampler;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(4) face: u32,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) @interpolate(flat) face: u32,
};

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    out.clip_position = cascade.view_proj * vec4<f32>(in.position, 1.0);
    out.uv = in.uv;
    out.face = in.face;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) {
    if ((in.face >> 24u) & 4u) != 0u {
        return;
    }
    let texel = textureSample(atlas_texture, atlas_sampler, in.uv);
    if texel.a < 0.5 {
        discard;
    }
}
