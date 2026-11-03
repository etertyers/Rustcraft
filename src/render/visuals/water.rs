//! Water, ice and glass in the Trailer look (M56), as pure functions the
//! tests can check; trailer.wgsl has the same formulas. Waves move the top
//! of still water a little and tilt its normal; a Fresnel term mixes in
//! reflections; deep water absorbs toward the biome's color; and from
//! below the surface is a window onto the sky with total reflection
//! outside it.

use glam::{Vec2, Vec3};

/// The most a still water surface moves up or down, in blocks.
pub const WAVE_AMPLITUDE: f32 = 0.05;
/// Water's reflectance looking straight down (Schlick's F0).
pub const WATER_F0: f32 = 0.02;
/// Ice and glass reflect a little more.
pub const GLASS_F0: f32 = 0.04;
/// Water's index of refraction.
pub const WATER_IOR: f32 = 1.333;
/// How fast water absorbs light toward its own color: per block of
/// water between the surface and what's under it.
pub const ABSORPTION: f32 = 0.30;

/// The surface's height offset at world `x`, `z` and game `ticks`, in
/// blocks, between −`WAVE_AMPLITUDE` and +`WAVE_AMPLITUDE`: three slow
/// sine waves going different ways. Zero is the flat surface.
pub fn wave_height(x: f32, z: f32, ticks: f32) -> f32 {
    let t = ticks / 20.0;
    let a = (x * 0.9 + z * 0.4 + t * 1.3).sin();
    let b = (x * -0.5 + z * 1.1 - t * 1.0).sin();
    let c = (x * 1.7 - z * 1.3 + t * 1.9).sin();
    WAVE_AMPLITUDE * (a * 0.45 + b * 0.35 + c * 0.2)
}

/// The slope of the surface (d height / dx, d height / dz), for tilting
/// the normal: the height's derivative plus a finer ripple that moves
/// the shading without moving the surface.
pub fn wave_slope(x: f32, z: f32, ticks: f32) -> Vec2 {
    let t = ticks / 20.0;
    let da = (x * 0.9 + z * 0.4 + t * 1.3).cos();
    let db = (x * -0.5 + z * 1.1 - t * 1.0).cos();
    let dc = (x * 1.7 - z * 1.3 + t * 1.9).cos();
    let big = Vec2::new(
        0.9 * da * 0.45 - 0.5 * db * 0.35 + 1.7 * dc * 0.2,
        0.4 * da * 0.45 + 1.1 * db * 0.35 - 1.3 * dc * 0.2,
    ) * WAVE_AMPLITUDE;
    // Two scrolling ripple layers (the shader's noise, here as sines).
    let r1 = (x * 5.1 + z * 3.3 + t * 2.1).cos() * 0.5;
    let r2 = (x * -3.7 + z * 5.9 - t * 1.7).cos() * 0.5;
    big + Vec2::new(r1 * 5.1 - r2 * 3.7, r1 * 3.3 + r2 * 5.9) * 0.004
}

/// The surface normal from its slope.
pub fn wave_normal(x: f32, z: f32, ticks: f32) -> Vec3 {
    let s = wave_slope(x, z, ticks);
    Vec3::new(-s.x, 1.0, -s.y).normalize()
}

/// Schlick's approximation: how much of the light is reflected seen at
/// `cos_theta` (the cosine of the angle from the normal) from a surface
/// whose reflectance straight on is `f0`.
pub fn fresnel(cos_theta: f32, f0: f32) -> f32 {
    f0 + (1.0 - f0) * (1.0 - cos_theta.clamp(0.0, 1.0)).powi(5)
}

/// How much of the water's own color shows over what's under it, for
/// `depth` blocks of water: 0 at the surface, toward 1 in the deep.
pub fn depth_tint(depth: f32) -> f32 {
    1.0 - (-ABSORPTION * depth.max(0.0)).exp()
}

/// How much of the sky a reflection shows with `sky` light (0–15) at the
/// surface: none in a cave, all in the open.
pub fn reflection_sky_weight(sky: f32) -> f32 {
    let t = ((sky - 1.0) / 14.0).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Seen from under the surface at `cos_theta` from the normal: the
/// direction's cosine in the air above if light gets out (Snell's
/// window), or `None` for total internal reflection.
pub fn snell_window(cos_theta: f32) -> Option<f32> {
    let sin_in = (1.0 - cos_theta * cos_theta).max(0.0).sqrt();
    let sin_out = sin_in * WATER_IOR;
    (sin_out < 1.0).then(|| (1.0 - sin_out * sin_out).sqrt())
}

/// The sun's glint on the water: a sharp highlight where the reflected
/// view direction `reflected` meets the light `light`, 0 with the light
/// below the horizon, dimmed by `rain` (0–1).
pub fn sun_glint(reflected: Vec3, light: Vec3, rain: f32) -> f32 {
    if light.y <= 0.0 {
        return 0.0;
    }
    reflected.dot(light).max(0.0).powf(1500.0) * 40.0 * (1.0 - rain.clamp(0.0, 1.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresnel_is_f0_looking_down_and_near_one_at_a_grazing_angle() {
        assert!((fresnel(1.0, WATER_F0) - 0.02).abs() < 1e-6);
        assert!(fresnel(0.05, WATER_F0) > 0.7);
        // Grazing: 1° above the surface.
        let grazing = fresnel(1.0f32.to_radians().sin(), WATER_F0);
        assert!(grazing > 0.9, "{grazing}");
        let mut last = 0.0;
        for i in (0..=100).rev() {
            let f = fresnel(i as f32 / 100.0, WATER_F0);
            assert!(f >= last - 1e-6);
            last = f;
        }
    }

    #[test]
    fn the_waves_never_move_the_surface_more_than_a_twentieth_of_a_block() {
        let mut highest: f32 = 0.0;
        for x in (-200..200).step_by(7) {
            for z in (-200..200).step_by(11) {
                for ticks in (0..4000).step_by(37) {
                    let h = wave_height(x as f32 * 0.37, z as f32 * 0.41, ticks as f32);
                    highest = highest.max(h.abs());
                }
            }
        }
        assert!(highest <= WAVE_AMPLITUDE + 1e-6, "{highest}");
        // And they do move it.
        assert!(highest > WAVE_AMPLITUDE * 0.5, "{highest}");
        // The normal stays near straight up.
        let n = wave_normal(3.0, 4.0, 100.0);
        assert!(n.y > 0.97 && (n.length() - 1.0).abs() < 1e-5, "{n:?}");
    }

    #[test]
    fn the_wave_slope_is_the_height_s_derivative() {
        let (x, z, t) = (4.2, -7.7, 123.0);
        let e = 1e-3;
        let dh_dx = (wave_height(x + e, z, t) - wave_height(x - e, z, t)) / (2.0 * e);
        let dh_dz = (wave_height(x, z + e, t) - wave_height(x, z - e, t)) / (2.0 * e);
        // The slope also holds the fine ripple, small next to the swell's.
        let s = wave_slope(x, z, t);
        assert!(
            (s.x - dh_dx).abs() < 0.12 && (s.y - dh_dz).abs() < 0.12,
            "{s:?} {dh_dx} {dh_dz}"
        );
    }

    #[test]
    fn deep_water_tints_more_than_shallow_and_none_at_the_surface() {
        assert_eq!(depth_tint(0.0), 0.0);
        let mut last = 0.0;
        for depth in [0.25, 0.5, 1.0, 2.0, 5.0, 10.0, 30.0] {
            let tint = depth_tint(depth);
            assert!(tint > last, "{depth}");
            last = tint;
        }
        assert!(depth_tint(1.0) < 0.35, "shallow water is clear");
        assert!(depth_tint(15.0) > 0.95, "deep water is dark");
        assert_eq!(depth_tint(-3.0), 0.0);
    }

    #[test]
    fn a_cave_lake_reflects_no_sky() {
        assert_eq!(reflection_sky_weight(0.0), 0.0);
        assert_eq!(reflection_sky_weight(1.0), 0.0);
        assert_eq!(reflection_sky_weight(15.0), 1.0);
        assert!(reflection_sky_weight(8.0) > 0.3 && reflection_sky_weight(8.0) < 0.7);
    }

    #[test]
    fn from_below_there_is_a_window_of_about_49_degrees() {
        // Straight up looks out.
        assert!((snell_window(1.0).unwrap() - 1.0).abs() < 1e-6);
        // The critical angle is asin(1 / 1.333) = 48.6°.
        let critical = (1.0 / WATER_IOR).asin();
        assert!(snell_window((critical - 0.01).cos()).is_some());
        assert!(snell_window((critical + 0.01).cos()).is_none());
        assert!(snell_window(0.2).is_none());
    }

    #[test]
    fn the_glint_is_sharp_dim_in_rain_and_gone_below_the_horizon() {
        let light = Vec3::new(0.6, 0.8, 0.0);
        assert!(sun_glint(light, light, 0.0) > 10.0);
        // A few degrees off: nothing.
        let off = Vec3::new(0.5, 0.86, 0.0).normalize();
        assert!(sun_glint(off, light, 0.0) < 0.5);
        assert!(sun_glint(light, light, 1.0) == 0.0);
        let below = Vec3::new(0.6, -0.8, 0.0);
        assert_eq!(sun_glint(below, below, 0.0), 0.0);
    }
}
