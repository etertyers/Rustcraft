// The Trailer look's clouds (M56): Vanilla's layout, height and speed (one
// layer, 12 blocks a texel, drifting west, 80% opaque), shaded: lit tops
// with a brighter rim toward the sun and a darker edge away from it, a
// darker underside seen from below, and a warm edge at sunrise and
// sunset. (Prepended with globals.wgsl and trailer_common.wgsl.)

const ALPHA: f32 = 0.8;

struct CloudOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) world: vec3<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> CloudOutput {
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, -1.0), vec2<f32>(1.0, 1.0),
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, 1.0), vec2<f32>(-1.0, 1.0),
    );
    let reach = globals.fog.x;
    let xz = globals.camera.xz + corners[index] * reach;
    let world = vec3<f32>(xz.x, globals.extra.y, xz.y);
    var out: CloudOutput;
    out.clip_position = globals.view_proj * vec4<f32>(world, 1.0);
    out.world = world;
    return out;
}

// The cloud at texel `cell` (wrapping): 1 or 0.
fn cell_cloud(cell: vec2<f32>) -> f32 {
    let dims = vec2<f32>(textureDimensions(cloud_texture));
    let wrapped = vec2<i32>(cell - floor(cell / dims) * dims);
    return select(0.0, 1.0, textureLoad(cloud_texture, wrapped, 0).a >= 0.5);
}

@fragment
fn fs_main(in: CloudOutput) -> @location(0) vec4<f32> {
    let dims = vec2<f32>(textureDimensions(cloud_texture));
    let size = BLOCKS_PER_CLOUD_TEXEL * dims.x;
    let drift = globals.camera.w * CLOUD_DRIFT_PER_TICK;
    let at = vec2<f32>(in.world.x + drift, in.world.z) / size * dims;
    let cell = floor(at);
    if cell_cloud(cell) < 0.5 {
        discard;
    }
    var color = cloud_tint();
    if trailer.sky_tint.w > 0.5 {
        let f = at - cell;
        let sun = trailer.sun_dir.xyz;
        var toward = vec2<f32>(0.0);
        if length(sun.xz) > 0.01 {
            toward = normalize(sun.xz);
        }
        var shade = 1.0;
        var rim = 0.0;
        // Each side with no cloud beyond it is an edge: lit and bright on
        // the sun's side, dark on the other.
        let sides = array<vec2<f32>, 4>(
            vec2<f32>(1.0, 0.0), vec2<f32>(-1.0, 0.0), vec2<f32>(0.0, 1.0), vec2<f32>(0.0, -1.0),
        );
        for (var i = 0; i < 4; i = i + 1) {
            let side = sides[i];
            if cell_cloud(cell + side) < 0.5 {
                // 1 at the edge, 0 a third of a cell in.
                let along = select(f.x, f.y, side.y != 0.0);
                let positive = side.x > 0.0 || side.y > 0.0;
                let edge_distance = select(along, 1.0 - along, positive);
                let closeness = 1.0 - smoothstep(0.0, 0.33, edge_distance);
                if dot(side, toward) > 0.0 {
                    rim = rim + closeness;
                } else {
                    shade = shade - 0.20 * closeness;
                }
            }
        }
        // Seen from under, the bottoms are in shadow.
        if globals.camera.y < globals.extra.y {
            shade = shade * 0.74;
        }
        let strength = trailer.sun_color.w;
        color = color * shade;
        color = color + vec3<f32>(0.10) * rim * strength;
        // A warm edge at sunrise and sunset.
        color = color + globals.sunrise.rgb * globals.sunrise.a * rim * 0.8;
    }
    let visible = fog_visibility(distance(in.world, globals.camera.xyz));
    let fogged = mix(globals.fog_color.rgb, color, visible);
    return vec4<f32>(srgb_to_linear(fogged), ALPHA);
}
