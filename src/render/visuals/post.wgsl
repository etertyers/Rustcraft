// The Trailer look's last pass: the HDR frame onto the screen with the
// tone curve (identity up to the shoulder, then a soft roll-off, on
// luminance so hues hold), about +10% saturation and a soft warm tint at
// sunrise and sunset. Mirrors sun::tone_map. The frame is sRGB, so linear
// values go out.

struct Post {
    // x: saturation; y: shoulder start; z: sunrise tint strength.
    params: vec4<f32>,
    tint: vec4<f32>,
};
@group(0) @binding(0) var hdr: texture_2d<f32>;
@group(0) @binding(1) var<uniform> post: Post;

struct PostOutput {
    @builtin(position) clip_position: vec4<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> PostOutput {
    let xy = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u)) * 2.0 - 1.0;
    var out: PostOutput;
    out.clip_position = vec4<f32>(xy, 0.0, 1.0);
    return out;
}

fn luminance(c: vec3<f32>) -> f32 {
    return dot(c, vec3<f32>(0.2126, 0.7152, 0.0722));
}

fn tone(l: f32) -> f32 {
    let shoulder = post.params.y;
    if l <= shoulder {
        return l;
    }
    if shoulder >= 1.0 {
        return 1.0;
    }
    let room = 1.0 - shoulder;
    return shoulder + room * (1.0 - exp(-(l - shoulder) / room));
}

@fragment
fn fs_main(in: PostOutput) -> @location(0) vec4<f32> {
    var color = textureLoad(hdr, vec2<i32>(in.clip_position.xy), 0).rgb;
    color = color * mix(vec3<f32>(1.0), post.tint.rgb, post.params.z);
    let l = luminance(color);
    if l > 1e-5 {
        color = color * (tone(l) / l);
    }
    let l2 = luminance(color);
    color = max(vec3<f32>(l2) + (color - vec3<f32>(l2)) * post.params.x, vec3<f32>(0.0));
    return vec4<f32>(color, 1.0);
}
