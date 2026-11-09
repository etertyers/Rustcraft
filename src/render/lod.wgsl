// Distant terrain (M69): LOD meshes drawn after the sky and before the
// loaded chunks, lit by the sun, fogged like blocks.

struct Lod {
    // x: the loaded edge (blocks from the viewer, Chebyshev); y: how far
    // down LOD vertices near that edge sit so real terrain covers them.
    edge: vec4<f32>,
};

@group(1) @binding(0) var<uniform> lod: Lod;

struct VertexInput {
    @location(0) pos: vec3<f32>,
    @location(1) rgba: vec4<f32>,
};

struct VertexOutput {
    @builtin(position) clip: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) color: vec3<f32>,
    @location(2) shade: f32,
};

const Z_NEAR: f32 = 0.05;
const Z_FAR: f32 = 1000.0;
// Past this distance (view depth) the real projection would clip LOD, so
// its depth is squeezed monotonically toward 1 instead.
const W_SQUEEZE: f32 = 900.0;

fn face_shade(face: u32) -> f32 {
    switch face {
        case 0u: { return 1.0; }
        case 1u, 2u: { return 0.8; }
        default: { return 0.6; }
    }
}

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    var pos = in.pos;
    let eye = globals.camera.xyz;
    let reach = max(abs(pos.x - eye.x), abs(pos.z - eye.z));
    if reach < lod.edge.x + 64.0 {
        pos.y -= lod.edge.y;
    }
    var clip = globals.view_proj * vec4<f32>(pos, 1.0);
    if clip.w > W_SQUEEZE {
        let at_squeeze = Z_FAR / (Z_FAR - Z_NEAR) * (1.0 - Z_NEAR / W_SQUEEZE);
        let depth = 1.0 - (1.0 - at_squeeze) * (W_SQUEEZE / clip.w);
        clip.z = depth * clip.w;
    }
    var out: VertexOutput;
    out.clip = clip;
    out.world = pos;
    out.color = in.rgba.rgb;
    let face = u32(round(in.rgba.a * 255.0)) >> 4u;
    out.shade = face_shade(face);
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    // The lightmap's sky light scales with the sun; never fully black.
    let sun = clamp((globals.fog.z - 0.2) / 0.8, 0.0, 1.0);
    var light = mix(0.16, 1.0, sun);
    light = max(light, globals.lighting.w / 15.0);
    light = max(light, globals.extra.w);
    let color = in.color * in.shade * light;
    let visible = fog_visibility(distance(in.world, globals.camera.xyz));
    let fogged = mix(globals.fog_color.rgb, color, visible);
    return vec4<f32>(srgb_to_linear(fogged), 1.0);
}
