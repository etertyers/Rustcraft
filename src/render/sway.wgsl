// Plants and leaves moving in the wind (M57), by world position and game
// time. Mirrors render/visuals/sway.rs. Used by the Trailer block shader
// and the shadow pass, so shadows move with what casts them.

// How far each kind moves at most, in blocks.
const SWAY_LEAVES: f32 = 0.0625;
const SWAY_PLANT: f32 = 0.1875;
const SWAY_VINE: f32 = 0.09375;

// The offset of a vertex at `position` of a face with attribute `face`.
// `wind` is 0-1 (rain and thunder raise it). Grass, flowers, ferns, crops
// and saplings bend at the top only (the bottom stays put); leaves move
// all over; vines and hanging roots swing at the free, lower end.
fn sway_offset(position: vec3<f32>, face: u32, ticks: f32, wind: f32) -> vec3<f32> {
    let material = (face >> 16u) & 255u;
    let top = (((face >> 24u) & 1u) != 0u);
    var reach = 0.0;
    if material == 1u {
        reach = SWAY_LEAVES;
    } else if (material == 2u || material == 3u) && top {
        reach = SWAY_PLANT;
    } else if material == 4u && !top {
        reach = SWAY_VINE;
    }
    if reach == 0.0 {
        return vec3<f32>(0.0);
    }
    let t = ticks / 20.0;
    let phase = position.x * 0.9 + position.z * 1.3 + position.y * 0.45;
    let strength = reach * (0.55 + 0.45 * clamp(wind, 0.0, 1.0)) * 0.70710678;
    let dx = sin(t * (1.6 + wind * 1.4) + phase);
    let dz = sin(t * (1.3 + wind * 1.2) + phase * 1.3 + 1.7);
    return vec3<f32>(dx * strength, 0.0, dz * strength);
}
