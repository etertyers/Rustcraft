// Blocks and dropped items: textured, lit by the lightmap, fogged.
// (Prepended with globals.wgsl.)

@group(1) @binding(0) var atlas_texture: texture_2d<f32>;
@group(1) @binding(1) var atlas_sampler: sampler;
// The enchantment glint's pattern, repeating.
@group(2) @binding(0) var glint_texture: texture_2d<f32>;
@group(2) @binding(1) var glint_sampler: sampler;

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
    @location(3) world: vec3<f32>,
};

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    out.clip_position = globals.view_proj * vec4<f32>(in.position, 1.0);
    out.uv = in.uv;
    out.color = in.color;
    out.light = in.light;
    out.world = in.position;
    return out;
}

// 1.0.0's light level to brightness curve (`lightBrightnessTable`, overworld),
// lifted by the dimension's ambient light as modern `getBrightness` is.
fn brightness(level: f32) -> f32 {
    let dark = 1.0 - level / 15.0;
    let curve = (1.0 - dark) / (dark * 3.0 + 1.0);
    let ambient = globals.extra.w;
    return curve + ambient * (1.0 - curve);
}

// 1.0.0's lightmap (`EntityRenderer.updateLightmap`) with the "Moody"
// brightness setting: sky light scaled by the time of day (and bluish at
// night), warm torch light, never fully black.
fn lightmap(light: vec2<f32>) -> vec3<f32> {
    let sun = globals.fog.z;
    let sky_level = brightness(light.x) * (sun * 0.95 + 0.05);
    let night = sun * 0.65 + 0.35;
    let sky = vec3<f32>(sky_level * night, sky_level * night, sky_level);
    let block = brightness(light.y) * 1.5;
    let torch = vec3<f32>(
        block,
        block * ((block * 0.6 + 0.4) * 0.6 + 0.4),
        block * (block * block * 0.6 + 0.4),
    );
    return min((sky + torch) * 0.96 + 0.03, vec3<f32>(1.0));
}

// Enchanted items have 64 added to their sky light (see GLINT_FLAG).
const GLINT_FLAG: f32 = 64.0;

// The glint pattern at a point of an enchanted item: stripes scrolling
// diagonally at 10°, tinted purple. Tiled across the item's world position
// (a block is 16 repeats).
fn glint(world: vec3<f32>) -> vec3<f32> {
    let seconds = globals.camera.w / 20.0;
    let p = vec2<f32>(world.x + world.z, world.y * 1.4) * 8.0;
    let c = 0.98480775;
    let s = 0.17364818;
    let rotated = vec2<f32>(p.x * c - p.y * s, p.x * s + p.y * c);
    let scroll = vec2<f32>(seconds / 3.0, seconds / 4.873);
    let texel = textureSample(glint_texture, glint_sampler, rotated + scroll);
    return texel.rgb * vec3<f32>(0.5, 0.25, 0.8);
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let texel = textureSample(atlas_texture, atlas_sampler, in.uv);
    // Cutout transparency (leaves, plants); water, ice and glass use
    // fs_translucent.
    if texel.a < 0.5 {
        discard;
    }
    var light = in.light;
    var shine = vec3<f32>(0.0);
    if light.x >= GLINT_FLAG * 0.5 {
        light.x -= GLINT_FLAG;
        shine = glint(in.world);
    }
    // Minecraft multiplies the texture by its tint, shade and lightmap in
    // sRGB values, so do the same: back to sRGB, multiply, then linear again
    // for the sRGB target. Multiplying in linear space crushes the darkest
    // light levels to pure black.
    let color = linear_to_srgb(texel.rgb) * in.color * lightmap(light) + shine;
    let visible = fog_visibility(distance(in.world, globals.camera.xyz));
    let fogged = mix(globals.fog_color.rgb, color, visible);
    return vec4<f32>(srgb_to_linear(fogged), 1.0);
}

// The crack over a block being mined: its color multiplies what's on
// screen twice over (1.0.0's DST_COLOR × SRC_COLOR blending), so mid-gray
// leaves it alone and darker pixels darken it. The texture's sRGB values
// are used as they are, since blending happens on linear values.
@fragment
fn fs_crack(in: VertexOutput) -> @location(0) vec4<f32> {
    let texel = textureSample(atlas_texture, atlas_sampler, in.uv);
    if texel.a < 0.1 {
        discard;
    }
    let shade = mix(vec3<f32>(0.5), linear_to_srgb(texel.rgb), texel.a);
    return vec4<f32>(shade, 1.0);
}

// Translucent faces (water, ice, glass): the texture's own alpha, blended.
@fragment
fn fs_translucent(in: VertexOutput) -> @location(0) vec4<f32> {
    let texel = textureSample(atlas_texture, atlas_sampler, in.uv);
    if texel.a < 0.01 {
        discard;
    }
    let color = linear_to_srgb(texel.rgb) * in.color * lightmap(in.light);
    let visible = fog_visibility(distance(in.world, globals.camera.xyz));
    let fogged = mix(globals.fog_color.rgb, color, visible);
    return vec4<f32>(srgb_to_linear(fogged), texel.a);
}
