// The Trailer look's last pass (M54): the HDR frame onto the screen. For
// now it copies the colors as they are, so Trailer looks like Vanilla
// until M55 adds the effects. The frame is sRGB, so linear values go in.

@group(0) @binding(0) var hdr: texture_2d<f32>;

struct PostOutput {
    @builtin(position) clip_position: vec4<f32>,
};

// One triangle covering the screen.
@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> PostOutput {
    let xy = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u)) * 2.0 - 1.0;
    var out: PostOutput;
    out.clip_position = vec4<f32>(xy, 0.0, 1.0);
    return out;
}

@fragment
fn fs_main(in: PostOutput) -> @location(0) vec4<f32> {
    let color = textureLoad(hdr, vec2<i32>(in.clip_position.xy), 0);
    return vec4<f32>(color.rgb, 1.0);
}
