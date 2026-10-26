// Per-frame values shared by the world shaders (prepended to each).

struct Globals {
    view_proj: mat4x4<f32>,
    inverse_view_proj: mat4x4<f32>,
    // The inverse of the view and projection without the camera's position,
    // for directions (the sky).
    inverse_sky_view_proj: mat4x4<f32>,
    // xyz: camera position; w: game time in ticks.
    camera: vec4<f32>,
    // rgb: fog color (sRGB); w: linear fog start distance.
    fog_color: vec4<f32>,
    // x: linear fog end; y: exponential fog density (0 means linear fog);
    // z: sun brightness for the lightmap; w: star brightness.
    fog: vec4<f32>,
    // rgb: sky color overhead (sRGB); w: celestial angle.
    sky_color: vec4<f32>,
    // rgb: sunrise or sunset glow (sRGB); a: its strength.
    sunrise: vec4<f32>,
    // x: moon phase 0–7; y: cloud height; z: 1 where there's no sky (the
    // Nether); w: the dimension's ambient light.
    extra: vec4<f32>,
    // x: the Brightness option, 0 (Moody) to 1 (Bright); y and z: Glint
    // Speed and Glint Strength as multiples of vanilla's defaults.
    lighting: vec4<f32>,
};

@group(0) @binding(0) var<uniform> globals: Globals;

// How much of something `distance` away still shows through the fog, 0–1:
// linear between the fog start and end, or exponential under water.
fn fog_visibility(distance: f32) -> f32 {
    if globals.fog.y > 0.0 {
        return exp(-globals.fog.y * distance);
    }
    let start = globals.fog_color.w;
    return clamp((globals.fog.x - distance) / (globals.fog.x - start), 0.0, 1.0);
}

fn linear_to_srgb(c: vec3<f32>) -> vec3<f32> {
    let curve = 1.055 * pow(c, vec3<f32>(1.0 / 2.4)) - 0.055;
    return select(curve, c * 12.92, c <= vec3<f32>(0.0031308));
}

fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    let curve = pow((c + 0.055) / 1.055, vec3<f32>(2.4));
    return select(curve, c / 12.92, c <= vec3<f32>(0.04045));
}
