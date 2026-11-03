// The Trailer look's block shader (M55): the same textures, tints, light
// levels and fog as shader.wgsl, with the sun and moon added on top: a
// directional light with cascaded shadow maps, sunlight seen through
// leaves, and the dimension's ambient tint. Draws into the HDR target;
// the tone curve is in the post pass. (Prepended with globals.wgsl.)

@group(1) @binding(0) var atlas_texture: texture_2d<f32>;
@group(1) @binding(1) var atlas_sampler: sampler;
@group(2) @binding(0) var glint_texture: texture_2d<f32>;
@group(2) @binding(1) var glint_sampler: sampler;

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
    // rgb: the light's color times its strength.
    sun_color: vec4<f32>,
    // x: cascades (0: no shadows); y: PCF taps; z: pixel-locked; w: hide
    // the entity shadow blobs.
    params: vec4<f32>,
    // x: sunlight through leaves; y: soft edges widening with distance;
    // z: ambient scale in the sun; w: direct scale.
    flags: vec4<f32>,
    // The dimension's ambient tint.
    tint: vec4<f32>,
    // The sky's tint on ambient light under the open sky.
    sky_tint: vec4<f32>,
};
@group(3) @binding(0) var<uniform> trailer: Trailer;
@group(3) @binding(1) var shadow_map: texture_depth_2d_array;
@group(3) @binding(2) var shadow_sampler: sampler_comparison;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) color: vec3<f32>,
    @location(3) light: vec2<f32>,
    @location(4) face: u32,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec3<f32>,
    @location(2) light: vec2<f32>,
    @location(3) world: vec3<f32>,
    @location(4) @interpolate(flat) face: u32,
};

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    out.clip_position = globals.view_proj * vec4<f32>(in.position, 1.0);
    out.uv = in.uv;
    out.color = in.color;
    out.light = in.light;
    out.world = in.position;
    out.face = in.face;
    return out;
}

fn brightness(level: f32) -> f32 {
    let dark = 1.0 - level / 15.0;
    let curve = (1.0 - dark) / (dark * 3.0 + 1.0);
    let ambient = globals.extra.w;
    return curve + ambient * (1.0 - curve);
}

fn not_gamma(x: vec3<f32>) -> vec3<f32> {
    let n = vec3<f32>(1.0) - x;
    return vec3<f32>(1.0) - n * n * n * n;
}

// Vanilla's lightmap, as shader.wgsl has it.
fn lightmap(raw_light: vec2<f32>) -> vec3<f32> {
    var light = raw_light;
    if (globals.lighting.w > 0.0) {
        light = max(raw_light, vec2<f32>(globals.lighting.w));
    }
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
    let gray = vec3<f32>(0.75);
    let lit = clamp(mix(sky + torch, gray, 0.04), vec3<f32>(0.0), vec3<f32>(1.0));
    let bright = mix(lit, not_gamma(lit), globals.lighting.x);
    return clamp(mix(bright, gray, 0.04), vec3<f32>(0.0), vec3<f32>(1.0));
}

const GLINT_FLAG: f32 = 64.0;
const SOLID_FLAG: f32 = 32.0;
const SHADOW_FLAG: f32 = 128.0;

fn glint(world: vec3<f32>) -> vec3<f32> {
    let seconds = globals.camera.w / 20.0 * globals.lighting.y;
    let p = vec2<f32>(world.x + world.z, world.y * 1.4) * 8.0;
    let c = 0.98480775;
    let s = 0.17364818;
    let rotated = vec2<f32>(p.x * c - p.y * s, p.x * s + p.y * c);
    let scroll = vec2<f32>(seconds / 3.0, seconds / 4.873);
    let texel = textureSample(glint_texture, glint_sampler, rotated + scroll);
    return texel.rgb * vec3<f32>(0.5, 0.25, 0.8) * globals.lighting.z;
}

// ---- the face attribute (render/vertex.rs) ----

// The face normal: octahedral, 8 bits a side.
fn face_normal(face: u32) -> vec3<f32> {
    var x = f32(face & 255u) / 255.0 * 2.0 - 1.0;
    var y = f32((face >> 8u) & 255u) / 255.0 * 2.0 - 1.0;
    let z = 1.0 - abs(x) - abs(y);
    let t = max(-z, 0.0);
    x = x + select(t, -t, x >= 0.0);
    y = y + select(t, -t, y >= 0.0);
    return normalize(vec3<f32>(x, y, z));
}

fn face_class(face: u32) -> u32 {
    return (face >> 16u) & 255u;
}

fn face_solid(face: u32) -> bool {
    return ((face >> 24u) & 4u) != 0u;
}

// Leaves, plants, crops and vines let the sun through.
fn translucent_class(material: u32) -> bool {
    return material >= 1u && material <= 4u;
}

// ---- shadows ----

const DISC = array<vec2<f32>, 16>(
    vec2<f32>(-0.94, -0.40), vec2<f32>(0.95, -0.77), vec2<f32>(-0.09, -0.93),
    vec2<f32>(0.34, 0.29), vec2<f32>(-0.92, 0.46), vec2<f32>(-0.82, -0.88),
    vec2<f32>(0.62, 0.98), vec2<f32>(-0.38, 0.28), vec2<f32>(0.18, -0.30),
    vec2<f32>(0.97, 0.12), vec2<f32>(-0.34, 0.89), vec2<f32>(0.46, -0.99),
    vec2<f32>(-0.60, -0.10), vec2<f32>(0.77, 0.53), vec2<f32>(-0.16, 0.60),
    vec2<f32>(0.10, -0.62),
);

fn hash2(p: vec3<f32>) -> f32 {
    return fract(sin(dot(p, vec3<f32>(12.9898, 78.233, 37.719))) * 43758.5453);
}

// How lit `world` is (1 in light, 0 in shadow) by the cascade that covers
// it, fading out at the end of the last one.
fn shadow_at(world: vec3<f32>, normal: vec3<f32>) -> f32 {
    let count = u32(trailer.params.x);
    if count == 0u {
        return 1.0;
    }
    let distance_to = distance(world, globals.camera.xyz);
    var cascade = count;
    for (var i = 0u; i < count; i = i + 1u) {
        if distance_to < trailer.reach[i] * 0.92 {
            cascade = i;
            break;
        }
    }
    if cascade >= count {
        return 1.0;
    }
    let end = trailer.reach[count - 1u] * 0.92;
    let fade = smoothstep(end * 0.85, end, distance_to);
    // Pixel-locked: the lookup sits at the middle of the 1/16-block texel
    // of the face, so shadow edges are crisp steps that match the
    // textures' pixels.
    var p = world;
    if trailer.params.z > 0.5 {
        let a = abs(normal);
        let q = floor(world * 16.0) + 0.5;
        p = vec3<f32>(
            select(q.x / 16.0, world.x, a.x > 0.6),
            select(q.y / 16.0, world.y, a.y > 0.6),
            select(q.z / 16.0, world.z, a.z > 0.6),
        );
        // Nudge off the face so its own plane isn't the occluder.
        p = p + normal * 0.002;
    }
    let texel = trailer.texel[cascade];
    p = p + normal * texel * 1.5;
    let clip = trailer.cascade_vp[cascade] * vec4<f32>(p, 1.0);
    let uv = vec2<f32>(clip.x * 0.5 + 0.5, 0.5 - clip.y * 0.5);
    let range = trailer.range[cascade];
    let depth = clip.z - 0.03 / range;
    let size = trailer.texel.w;
    let layer = i32(cascade);
    let taps = u32(trailer.params.y);
    var radius = 1.4;
    if trailer.flags.y > 0.5 {
        // Soft edges: average the depth of what blocks the light, and
        // widen the blur with how far the occluder is.
        var blockers = 0.0;
        var average = 0.0;
        for (var i = 0u; i < 8u; i = i + 1u) {
            let at = uv + DISC[i * 2u] * 6.0 / size;
            let stored = textureLoad(
                shadow_map,
                vec2<i32>(clamp(at, vec2<f32>(0.0), vec2<f32>(0.9999)) * size),
                layer,
                0,
            );
            if stored < depth {
                blockers = blockers + 1.0;
                average = average + stored;
            }
        }
        if blockers > 0.0 {
            let gap = (depth - average / blockers) * range;
            radius = clamp(1.0 + gap * 0.5 / texel, 1.0, 7.0);
        }
    }
    let spin = hash2(floor(p * 16.0)) * 6.2831853;
    let sn = sin(spin);
    let cs = cos(spin);
    var lit = 0.0;
    for (var i = 0u; i < taps; i = i + 1u) {
        let d = DISC[i];
        let offset = vec2<f32>(d.x * cs - d.y * sn, d.x * sn + d.y * cs) * radius / size;
        lit = lit + textureSampleCompareLevel(shadow_map, shadow_sampler, uv + offset, layer, depth);
    }
    lit = lit / f32(taps);
    return mix(lit, 1.0, fade);
}

// The light on a face: Vanilla's lightmap as the ambient, tinted by the
// sky and the dimension, plus the sun's or moon's direct light where the
// face has sky light and isn't in shadow. Mirrors sun::face_lighting.
fn face_lighting(
    world: vec3<f32>,
    face: u32,
    sky: f32,
    vanilla: vec3<f32>,
) -> vec3<f32> {
    // No sun or moon here (the Nether, the End, under water, the hand):
    // Vanilla's light with the dimension's tint.
    if trailer.sun_dir.w < 0.5 || globals.extra.z > 0.5 {
        return vanilla * trailer.tint.rgb;
    }
    var n = face_normal(face);
    // Seen from behind (plants, mobs): turn the normal to the viewer.
    if dot(n, globals.camera.xyz - world) < 0.0 {
        n = -n;
    }
    let material = face_class(face);
    let l = trailer.sun_dir.xyz;
    var n_dot_l = dot(n, l);
    if translucent_class(material) && trailer.flags.x > 0.5 {
        // Wrapped light, and some from behind: the sun shines through.
        let front = clamp((n_dot_l + 0.4) / 1.4, 0.0, 1.0);
        let back = clamp((-n_dot_l + 0.4) / 1.4, 0.0, 1.0) * 0.6;
        n_dot_l = max(front, back);
    } else {
        n_dot_l = max(n_dot_l, 0.0);
    }
    var shadow = 1.0;
    if n_dot_l > 0.0 {
        shadow = shadow_at(world, n);
    }
    let sky_factor = smoothstep(0.0, 15.0, clamp(sky, 0.0, 15.0));
    let sky_tint = mix(vec3<f32>(1.0), trailer.sky_tint.rgb, sky_factor);
    let ambient = vanilla * sky_tint * trailer.flags.z;
    let direct = trailer.sun_color.rgb * (n_dot_l * shadow * sky_factor * trailer.flags.w);
    var lit = ambient + direct;
    if globals.lighting.w > 0.0 {
        // A mod's brightness override: nothing darker than in Vanilla.
        lit = max(lit, vanilla);
    }
    return lit;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let texel = textureSample(atlas_texture, atlas_sampler, in.uv);
    var light = in.light;
    var solid = face_solid(in.face);
    if light.y >= SOLID_FLAG * 0.5 {
        light.y -= SOLID_FLAG;
        solid = true;
    }
    if texel.a < 0.5 && !solid {
        discard;
    }
    var shine = vec3<f32>(0.0);
    if light.x >= GLINT_FLAG * 0.5 {
        light.x -= GLINT_FLAG;
        shine = glint(in.world);
    }
    let lit = face_lighting(in.world, in.face, light.x, lightmap(light));
    let color = linear_to_srgb(texel.rgb) * in.color * lit + shine;
    let visible = fog_visibility(distance(in.world, globals.camera.xyz));
    let fogged = mix(globals.fog_color.rgb, color, visible);
    return vec4<f32>(srgb_to_linear(fogged), 1.0);
}

@fragment
fn fs_crack(in: VertexOutput) -> @location(0) vec4<f32> {
    let texel = textureSample(atlas_texture, atlas_sampler, in.uv);
    if texel.a < 0.1 {
        discard;
    }
    let shade = mix(vec3<f32>(0.5), linear_to_srgb(texel.rgb), texel.a);
    return vec4<f32>(shade, 1.0);
}

@fragment
fn fs_translucent(in: VertexOutput) -> @location(0) vec4<f32> {
    let texel = textureSample(atlas_texture, atlas_sampler, in.uv);
    if texel.a < 0.01 {
        discard;
    }
    if in.light.x >= SHADOW_FLAG * 0.5 {
        // Vanilla's round entity shadow blob: gone when real shadows are on.
        if trailer.params.w > 0.5 {
            discard;
        }
        return vec4<f32>(texel.rgb, texel.a * in.color.r);
    }
    let lit = face_lighting(in.world, in.face, in.light.x, lightmap(in.light));
    let color = linear_to_srgb(texel.rgb) * in.color * lit;
    let visible = fog_visibility(distance(in.world, globals.camera.xyz));
    let fogged = mix(globals.fog_color.rgb, color, visible);
    return vec4<f32>(srgb_to_linear(fogged), texel.a);
}
