// The Trailer look's block shader (M55): the same textures, tints, light
// levels and fog as shader.wgsl, with the sun and moon added on top: a
// directional light with cascaded shadow maps, sunlight seen through
// leaves, and the dimension's ambient tint. Draws into the HDR target;
// the tone curve is in the post pass. (Prepended with globals.wgsl.)

@group(1) @binding(0) var atlas_texture: texture_2d<f32>;
@group(1) @binding(1) var atlas_sampler: sampler;
@group(2) @binding(0) var glint_texture: texture_2d<f32>;
@group(2) @binding(1) var glint_sampler: sampler;

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
    var position = in.position;
    // The flat top of still water rises and falls a little (face flag 2).
    if ((in.face >> 24u) & 2u) != 0u {
        position.y = position.y + wave_height(position.x, position.z, globals.camera.w);
    }
    out.clip_position = globals.view_proj * vec4<f32>(position, 1.0);
    out.uv = in.uv;
    out.color = in.color;
    out.light = in.light;
    out.world = position;
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

fn face_material(face: u32) -> u32 {
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
    receive_shadows: bool,
) -> vec3<f32> {
    // No sun or moon here (the Nether, the End, the hand, under water
    // without caustics): Vanilla's light with the dimension's tint. The
    // hand is drawn with a fog start no real fog has.
    let is_hand = globals.fog_color.w > 100000.0;
    let underwater = trailer.water.z > 0.5;
    if trailer.sun_dir.w < 0.5 || is_hand || (globals.extra.z > 0.5 && !underwater) {
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
    // Water, ice and glass don't take shadows: the lookups would only
    // cost, over a whole sea.
    if n_dot_l > 0.0 && receive_shadows {
        shadow = shadow_at(world, n);
        // Clouds' soft shadows: the cloud layer along the way to the sun.
        if trailer.extras.y > 0.5 && trailer.scene.w > 0.5 && world.y < trailer.scene.z {
            let t = (trailer.scene.z - world.y) / max(l.y, 0.05);
            shadow = shadow * (1.0 - 0.55 * cloud_alpha(world.xz + l.xz * t));
        }
    }
    let sky_factor = smoothstep(0.0, 15.0, clamp(sky, 0.0, 15.0));
    let sky_tint = mix(vec3<f32>(1.0), trailer.sky_tint.rgb, sky_factor);
    let ambient = vanilla * sky_tint * trailer.flags.z;
    var sun_color = trailer.sun_color.rgb;
    if underwater {
        // Sunlight through the water: blue-green, in moving patterns.
        let veins = caustic(world.xz * 0.9 + world.y * 0.3, globals.camera.w / 20.0);
        sun_color = sun_color * vec3<f32>(0.45, 0.85, 1.0) * (0.3 + 2.4 * veins);
    }
    let direct = sun_color * (n_dot_l * shadow * sky_factor * trailer.flags.w);
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
    let lit = face_lighting(in.world, in.face, light.x, lightmap(light), true);
    var color = linear_to_srgb(texel.rgb) * in.color * lit + shine;
    // Metal shines a little at High: the sky's color in its sheen at a
    // glancing angle (cheap: no sky lookup in the block shader).
    if trailer.extras.z > 0.5 && face_material(in.face) == 10u {
        let view = normalize(in.world - globals.camera.xyz);
        let n = face_normal(in.face);
        let sheen = fresnel(abs(dot(view, n)), 0.08) * reflection_sky_weight(light.x);
        color = mix(color, globals.sky_color.rgb * lit, sheen * 0.6);
    }
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

// Water seen from above: the sky and the shore reflected by Fresnel, the
// scene under it seen through the waves (bent, and tinted by depth), the
// sun's glint. Returns linear color and alpha.
fn water_above(in: VertexOutput, texel: vec4<f32>, lit: vec3<f32>) -> vec4<f32> {
    let eye = globals.camera.xyz;
    let to_surface = in.world - eye;
    let distance_to = length(to_surface);
    let view = to_surface / distance_to;
    let slope = wave_slope(in.world.x, in.world.z, globals.camera.w);
    let n = normalize(vec3<f32>(-slope.x, 1.0, -slope.y));
    let cos_theta = max(dot(-view, n), 0.0);
    let f = fresnel(cos_theta, WATER_F0) * reflection_sky_weight(in.light.x);
    let reflected = reflect(view, n);
    // The water's own color: its texture and the biome's tint, lit.
    let body = srgb_to_linear(linear_to_srgb(texel.rgb) * in.color * lit);
    var reflection = srgb_to_linear(sky_reflection(in.world, reflected)) * reflection_sky_weight(in.light.x);
    var ssr_weight = 0.0;
    let pixel = in.clip_position.xy;
    if trailer.water.x > 0.5 {
        let jitter = hash2(vec3<f32>(pixel, globals.camera.w));
        let hit = screen_reflection(in.world, reflected, u32(trailer.water.x), jitter);
        ssr_weight = hit.w;
        reflection = mix(reflection, hit.rgb, hit.w);
    }
    // What's under the surface.
    var under = body;
    var alpha = max(texel.a, f);
    if trailer.water.y > 0.5 {
        var uv = pixel / trailer.scene.xy;
        let bend = n.xz * 0.35 / (1.0 + distance_to * 0.04);
        var shifted = clamp(uv + bend * 0.05, vec2<f32>(0.001), vec2<f32>(0.999));
        var d = textureLoad(scene_depth, vec2<i32>(shifted * trailer.scene.xy), 0);
        // Something in front of the water (a boat, a mob): don't bend.
        if d < in.clip_position.z {
            shifted = uv;
            d = textureLoad(scene_depth, vec2<i32>(uv * trailer.scene.xy), 0);
        }
        var thickness = 40.0;
        if d < 1.0 {
            thickness = max(distance(world_from_depth(shifted, d), eye) - distance_to, 0.0);
        }
        let scene = textureSampleLevel(scene_color, scene_sampler, shifted, 0.0).rgb;
        let deep = body * vec3<f32>(0.55, 0.7, 0.85);
        under = mix(scene, deep, depth_tint(thickness * 0.8));
        alpha = 1.0;
    }
    var color = mix(under, reflection, f);
    // The sun's glint on the waves.
    if trailer.sun_dir.w > 0.5 {
        let glint = sun_glint(reflected, trailer.sun_dir.xyz);
        color = color + trailer.sun_color.rgb * glint * reflection_sky_weight(in.light.x);
    }
    return vec4<f32>(color, alpha);
}

// Water seen from under the surface (High): a window onto the sky inside
// the critical angle, the dim water outside it.
fn water_below(in: VertexOutput, lit: vec3<f32>) -> vec4<f32> {
    let view = normalize(in.world - globals.camera.xyz);
    let cos_theta = abs(view.y);
    let fog = srgb_to_linear(globals.fog_color.rgb);
    let sin_in = sqrt(max(1.0 - cos_theta * cos_theta, 0.0));
    let sin_out = sin_in * WATER_IOR;
    if sin_out < 1.0 {
        let out_dir = normalize(vec3<f32>(view.x * WATER_IOR, sqrt(1.0 - sin_out * sin_out), view.z * WATER_IOR));
        let sky = srgb_to_linear(sky_gradient(out_dir));
        return vec4<f32>(mix(sky * 0.9, fog, 0.25), 1.0);
    }
    // Total internal reflection: the water's own dimness.
    return vec4<f32>(fog * 0.7, 1.0);
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
    let lit = face_lighting(in.world, in.face, in.light.x, lightmap(in.light), false);
    let material = face_material(in.face);
    let n = face_normal(in.face);
    let visible = fog_visibility(distance(in.world, globals.camera.xyz));
    let fog = srgb_to_linear(globals.fog_color.rgb);
    // Water's surface (not its sides): reflections, refraction, glints.
    if material == 5u && abs(n.y) > 0.9 {
        let underside = n.y < 0.0;
        if !underside {
            let water = water_above(in, texel, lit);
            return vec4<f32>(mix(fog, water.rgb, visible), water.a);
        }
        if trailer.water.w > 0.5 {
            let water = water_below(in, lit);
            return vec4<f32>(mix(fog, water.rgb, visible), water.a);
        }
    }
    var color = srgb_to_linear(linear_to_srgb(texel.rgb) * in.color * lit);
    var alpha = texel.a;
    // Ice and glass catch the sky: a little at the glancing angles, the
    // sun's glint on ice.
    if material == 7u || material == 8u {
        let view = normalize(in.world - globals.camera.xyz);
        var facing = n;
        if dot(facing, view) > 0.0 {
            facing = -facing;
        }
        let f = fresnel(dot(-view, facing), GLASS_F0) * reflection_sky_weight(in.light.x);
        let reflected = reflect(view, facing);
        var reflection = srgb_to_linear(sky_reflection(in.world, reflected));
        // Ice reflects what's on screen from Medium; glass from High.
        if trailer.water.x > 0.5 && (material == 7u || trailer.extras.z > 0.5) {
            let hit = screen_reflection(in.world, reflected, u32(trailer.water.x), hash2(vec3<f32>(in.clip_position.xy, 1.0)));
            reflection = mix(reflection, hit.rgb, hit.w);
        }
        color = mix(color, reflection, f);
        alpha = max(alpha, f);
        if material == 7u && trailer.sun_dir.w > 0.5 {
            color = color + trailer.sun_color.rgb * sun_glint(reflected, trailer.sun_dir.xyz) * 0.12;
        }
    }
    return vec4<f32>(mix(fog, color, visible), alpha);
}

// Low: water with no scene copy. Cheap waves (the slow swell's slope), the
// sky reflected by Fresnel and the sun's glint, over Vanilla's blending.
@fragment
fn fs_translucent_low(in: VertexOutput) -> @location(0) vec4<f32> {
    let texel = textureSample(atlas_texture, atlas_sampler, in.uv);
    if texel.a < 0.01 {
        discard;
    }
    if in.light.x >= SHADOW_FLAG * 0.5 {
        if trailer.params.w > 0.5 {
            discard;
        }
        return vec4<f32>(texel.rgb, texel.a * in.color.r);
    }
    let lit = face_lighting(in.world, in.face, in.light.x, lightmap(in.light), false);
    let visible = fog_visibility(distance(in.world, globals.camera.xyz));
    let fog = srgb_to_linear(globals.fog_color.rgb);
    var color = srgb_to_linear(linear_to_srgb(texel.rgb) * in.color * lit);
    var alpha = texel.a;
    let n = face_normal(in.face);
    let material = face_material(in.face);
    if material == 5u && n.y > 0.9 {
        let view = normalize(in.world - globals.camera.xyz);
        let t = globals.camera.w / 20.0;
        let slope = vec2<f32>(cos(in.world.x * 0.9 + in.world.z * 0.4 + t * 1.3), cos(in.world.z * 1.1 - in.world.x * 0.5 - t)) * 0.03;
        let wave = normalize(vec3<f32>(-slope.x, 1.0, -slope.y));
        let reflected = reflect(view, wave);
        let weight = reflection_sky_weight(in.light.x);
        let f = fresnel(max(dot(-view, wave), 0.0), WATER_F0) * weight;
        let sky = srgb_to_linear(sky_gradient(reflected));
        color = mix(color, sky, f);
        alpha = max(alpha, f);
        if trailer.sun_dir.w > 0.5 {
            color = color + trailer.sun_color.rgb * sun_glint(reflected, trailer.sun_dir.xyz) * weight;
        }
    }
    return vec4<f32>(mix(fog, color, visible), alpha);
}
