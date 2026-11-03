// Light shafts (M57): from each pixel's ray through the world, march
// through the sun's shadow maps and add up the light that reaches the
// steps, so sunlight shows as beams through leaves, cave mouths and
// windows. Quarter size (Medium) or half size (High), jittered, then
// blurred and brought up to size by the post pass.
// (Prepended with globals.wgsl and trailer_common.wgsl.)

@group(1) @binding(0) var unused_atlas: texture_2d<f32>;
@group(1) @binding(1) var unused_sampler: sampler;
@group(2) @binding(0) var unused_glint: texture_2d<f32>;
@group(2) @binding(1) var unused_glint_sampler: sampler;

struct Output {
    @builtin(position) clip_position: vec4<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> Output {
    let xy = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u)) * 2.0 - 1.0;
    var out: Output;
    out.clip_position = vec4<f32>(xy, 0.0, 1.0);
    return out;
}

// Whether the sun reaches `p`: 1 lit, 0 in shadow, from the cascade that
// covers it (1 beyond them all).
fn lit_at(p: vec3<f32>) -> f32 {
    let count = u32(trailer.params.x);
    let distance_to = distance(p, globals.camera.xyz);
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
    let clip = trailer.cascade_vp[cascade] * vec4<f32>(p, 1.0);
    let uv = vec2<f32>(clip.x * 0.5 + 0.5, 0.5 - clip.y * 0.5);
    let depth = clip.z - 0.05 / trailer.range[cascade];
    return textureSampleCompareLevel(shadow_map, shadow_sampler, uv, i32(cascade), depth);
}

@fragment
fn fs_main(in: Output) -> @location(0) vec4<f32> {
    // x: steps; y: the farthest it looks; z: scattering per block; w: how
    // many pixels of the frame one pixel here covers.
    let steps = u32(trailer.shaft.x);
    let scale = trailer.shaft.w;
    let size = trailer.scene.xy;
    let uv = (in.clip_position.xy * scale + 0.5 * scale) / size;
    let depth = textureLoad(scene_depth, vec2<i32>(min(uv, vec2<f32>(0.9999)) * size), 0);
    let eye = globals.camera.xyz;
    let end = world_from_depth(uv, select(depth, 0.99999, depth >= 1.0));
    let to = end - eye;
    let dir = normalize(to);
    let reach = min(length(to), trailer.shaft.y);
    // Jitter along the ray, per pixel and frame, to hide the steps.
    let jitter = fract(52.9829189 * fract(dot(in.clip_position.xy + globals.camera.w * 5.588, vec2<f32>(0.06711056, 0.00583715))));
    var lit = 0.0;
    for (var i = 0u; i < steps; i = i + 1u) {
        let t = (f32(i) + jitter) / f32(steps) * reach;
        let p = eye + dir * t;
        // Less of it high up in the air.
        let thin = exp(-max(p.y - 70.0, 0.0) * 0.02);
        lit = lit + lit_at(p) * thin;
    }
    // Light scatters forward: brightest looking toward it.
    let toward = max(dot(dir, trailer.sun_dir.xyz), 0.0);
    let phase = 0.3 + 0.7 * toward * toward * toward;
    let amount = lit / f32(steps) * reach * trailer.shaft.z * phase;
    return vec4<f32>(amount, 0.0, 0.0, 1.0);
}
