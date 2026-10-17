struct Globals {
    view_proj: mat4x4<f32>,
};

@group(0) @binding(0) var<uniform> globals: Globals;
@group(1) @binding(0) var atlas_texture: texture_2d<f32>;
@group(1) @binding(1) var atlas_sampler: sampler;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) color: vec3<f32>,
    @location(3) light: vec2<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec3<f32>,
    @location(2) light: vec2<f32>,
};

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    out.clip_position = globals.view_proj * vec4<f32>(in.position, 1.0);
    out.uv = in.uv;
    out.color = in.color;
    out.light = in.light;
    return out;
}

// 1.0.0's light level to brightness curve (`lightBrightnessTable`, overworld).
fn brightness(level: f32) -> f32 {
    let dark = 1.0 - level / 15.0;
    return (1.0 - dark) / (dark * 3.0 + 1.0);
}

// 1.0.0's lightmap (`EntityRenderer.updateLightmap`) at full daylight with the
// "Moody" brightness setting: white sky light plus warm torch light, never
// fully black. The day/night cycle (M8) will scale the sky part.
fn lightmap(light: vec2<f32>) -> vec3<f32> {
    let sky = brightness(light.x);
    let block = brightness(light.y) * 1.5;
    let torch = vec3<f32>(
        block,
        block * ((block * 0.6 + 0.4) * 0.6 + 0.4),
        block * (block * block * 0.6 + 0.4),
    );
    return min((vec3<f32>(sky) + torch) * 0.96 + 0.03, vec3<f32>(1.0));
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let texel = textureSample(atlas_texture, atlas_sampler, in.uv);
    // Cutout transparency (leaves, plants); blended transparency comes in M8.
    if texel.a < 0.5 {
        discard;
    }
    // The vertex color is sRGB like Minecraft's tint and shade factors; the
    // texel is already linear, so linearize the color before multiplying.
    let color = in.color * lightmap(in.light);
    return vec4<f32>(texel.rgb * pow(color, vec3<f32>(2.2)), 1.0);
}
