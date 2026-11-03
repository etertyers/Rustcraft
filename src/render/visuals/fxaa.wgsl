// FXAA (M57), tuned for pixel art: only the edges between different
// things (where the depth jumps) are smoothed, along the edge. Inside a
// face, where the textures' pixels meet, nothing is touched, so the
// textures stay sharp. Reads the tone-mapped frame and the opaque scene's
// depth.

struct Params {
    // x: the near plane; y: the far plane; z: how big a jump in distance
    // (as a share of it) counts as an edge.
    values: vec4<f32>,
};
@group(0) @binding(0) var frame: texture_2d<f32>;
@group(0) @binding(1) var scene_depth: texture_depth_2d;
@group(0) @binding(2) var linear_sampler: sampler;
@group(0) @binding(3) var<uniform> params: Params;

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

fn luma(c: vec3<f32>) -> f32 {
    return dot(c, vec3<f32>(0.299, 0.587, 0.114));
}

// Distance in front of the camera for a depth value.
fn linear_depth(d: f32) -> f32 {
    return params.values.x * params.values.y / (params.values.y - d * (params.values.y - params.values.x));
}

@fragment
fn fs_main(in: Output) -> @location(0) vec4<f32> {
    let px = vec2<i32>(in.clip_position.xy);
    let size = vec2<f32>(textureDimensions(frame));
    let center = textureLoad(frame, px, 0);
    let z = linear_depth(textureLoad(scene_depth, px, 0));
    let dx = vec2<i32>(1, 0);
    let dy = vec2<i32>(0, 1);
    let zr = linear_depth(textureLoad(scene_depth, px + dx, 0));
    let zl = linear_depth(textureLoad(scene_depth, px - dx, 0));
    let zu = linear_depth(textureLoad(scene_depth, px - dy, 0));
    let zd = linear_depth(textureLoad(scene_depth, px + dy, 0));
    let jump = max(max(abs(zr - z), abs(zl - z)), max(abs(zu - z), abs(zd - z)));
    if jump < params.values.z * z {
        return center;
    }
    // An edge.
    let l = luma(center.rgb);
    let lr = luma(textureLoad(frame, px + dx, 0).rgb);
    let ll = luma(textureLoad(frame, px - dx, 0).rgb);
    let lu = luma(textureLoad(frame, px - dy, 0).rgb);
    let ld = luma(textureLoad(frame, px + dy, 0).rgb);
    let gradient = vec2<f32>(lr - ll, ld - lu);
    if length(gradient) < 0.02 {
        return center;
    }
    // Blend across the edge, along the steepest change of light: a
    // staircase's steps soften into a smooth slope.
    let along = normalize(gradient);
    let uv = (in.clip_position.xy) / size;
    let a = textureSampleLevel(frame, linear_sampler, uv + along * 0.5 / size, 0.0);
    let b = textureSampleLevel(frame, linear_sampler, uv - along * 0.5 / size, 0.0);
    let c = textureSampleLevel(frame, linear_sampler, uv + along * 1.5 / size, 0.0);
    let d = textureSampleLevel(frame, linear_sampler, uv - along * 1.5 / size, 0.0);
    return mix(center, (a + b) * 0.35 + (c + d) * 0.15, 0.75);
}
