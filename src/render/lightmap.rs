//! The lightmap (modern `LightTexture.updateLightTexture`): how bright
//! each sky and block light level draws. `shader.wgsl`'s `lightmap` is the
//! same formula on the GPU; this copy is for tests.

/// A light level's brightness: `f / (4 − 3f)` with `f = level / 15`,
/// lifted by the dimension's ambient light.
pub fn curve(level: f32, ambient: f32) -> f32 {
    let f = level / 15.0;
    let curved = f / (4.0 - 3.0 * f);
    curved + ambient * (1.0 - curved)
}

/// Vanilla's `notGamma`: the brighter curve the Brightness option blends
/// toward.
pub fn not_gamma(x: f32) -> f32 {
    let n = 1.0 - x;
    1.0 - n * n * n * n
}

fn mix(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    std::array::from_fn(|i| a[i] + (b[i] - a[i]) * t)
}

/// The sRGB color light (`sky`, `block`) gives: block light warm and
/// flickering about 1.5×, sky light scaled by the sun (bluish at night),
/// a little gray mixed in, then blended toward the brighter curve by
/// `brightness` (the option: 0 Moody, 0.5 the default, 1 Bright).
pub fn color(sky: f32, block: f32, sun: f32, ambient: f32, brightness: f32) -> [f32; 3] {
    let sky_level = curve(sky, ambient) * (sun * 0.95 + 0.05);
    let night = sun * 0.65 + 0.35;
    let b = curve(block, ambient) * 1.5;
    let torch = [
        b,
        b * ((b * 0.6 + 0.4) * 0.6 + 0.4),
        b * (b * b * 0.6 + 0.4),
    ];
    let lit = [
        torch[0] + sky_level * night,
        torch[1] + sky_level * night,
        torch[2] + sky_level,
    ];
    let gray = [0.75; 3];
    let lit = mix(lit, gray, 0.04).map(|c| c.clamp(0.0, 1.0));
    let bright = lit.map(not_gamma);
    mix(mix(lit, bright, brightness), gray, 0.04).map(|c| c.clamp(0.0, 1.0))
}

/// The color a water surface is drawn in before its alpha: the texture's
/// average (0–1 sRGB) times the biome's tint, the top face's full shade
/// (1.0) and the lightmap for `sky` and `block` light. `world::mesh`
/// gives a surface the light above it, so the sea at noon is lit by sky
/// light 15 however deep it is.
pub fn water_surface(
    texture: [f32; 3],
    tint: [f32; 3],
    (sky, block): (f32, f32),
    sun: f32,
    ambient: f32,
    brightness: f32,
) -> [f32; 3] {
    let light = color(sky, block, sun, ambient, brightness);
    std::array::from_fn(|i| texture[i] * tint[i] * light[i])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_water_surface_at_noon_is_the_same_color_at_any_depth() {
        // The mesher gives a surface the brighter of its own and the sky's
        // light, so what differs with depth (the water's own light) never
        // reaches the color.
        let surface = |own: u8| {
            let (sky, block) = crate::world::mesh::fluid_light((own, 0), (15, 0)).into();
            water_surface(
                [0.69; 3],
                [0.247, 0.463, 0.894],
                (sky, block),
                1.0,
                0.0,
                0.5,
            )
        };
        assert_eq!(surface(14), surface(1));
        let color = surface(14);
        // Vanilla's gray texture (0.69) in the default ocean tint.
        assert!((color[2] - 0.69 * 0.894).abs() < 0.03, "{color:?}");
        assert!(color[2] > color[1] && color[1] > color[0]);
    }

    #[test]
    fn levels_follow_vanillas_curve() {
        assert_eq!(curve(0.0, 0.0), 0.0);
        assert_eq!(curve(15.0, 0.0), 1.0);
        // f = 0.5: 0.5 / 2.5.
        assert!((curve(7.5, 0.0) - 0.2).abs() < 1e-6);
        for level in 1..15 {
            assert!(curve(level as f32, 0.0) > curve(level as f32 - 1.0, 0.0));
        }
        assert!(
            (curve(0.0, 0.1) - 0.1).abs() < 1e-6,
            "the Nether's ambient light"
        );
    }

    #[test]
    fn torch_light_is_warm_and_the_brightness_option_lifts_the_dark() {
        // Four blocks from a torch (10) at night with no sky; right by it
        // the light is all but white.
        let moody = color(0.0, 10.0, 0.2, 0.0, 0.0);
        let normal = color(0.0, 10.0, 0.2, 0.0, 0.5);
        let bright = color(0.0, 10.0, 0.2, 0.0, 1.0);
        assert!(color(0.0, 14.0, 0.2, 0.0, 0.5).iter().all(|&c| c > 0.95));
        assert!(moody[0] > moody[2], "warm: more red than blue {moody:?}");
        assert!(normal[0] >= moody[0] && bright[0] >= normal[0]);
        // Seven blocks away (level 7) the option matters most.
        let far = |b| color(0.0, 7.0, 0.2, 0.0, b)[1];
        assert!(far(0.5) > far(0.0) * 1.5, "{} {}", far(0.0), far(0.5));
        assert!(far(1.0) > far(0.5));
        // Never quite black: 4% of 0.75 gray, twice, at Moody.
        assert!((color(0.0, 0.0, 0.0, 0.0, 0.0)[0] - 0.0588).abs() < 1e-3);
        // Full daylight is white at any brightness.
        assert!(color(15.0, 0.0, 1.0, 0.0, 0.5).iter().all(|&c| c > 0.95));
    }

    #[test]
    fn not_gamma_lifts_the_dark_end_most() {
        assert_eq!(not_gamma(0.0), 0.0);
        assert_eq!(not_gamma(1.0), 1.0);
        assert!((not_gamma(0.2) - (1.0 - 0.8f32.powi(4))).abs() < 1e-6);
    }
}
