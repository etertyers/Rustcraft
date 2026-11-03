//! The Trailer look's sun and moon light (M55), in pure functions the
//! tests can check: where the light comes from and how strong and what
//! color it is, the cascade layout of the shadow maps per quality, how a
//! cascade's projection is snapped to whole shadow texels, the tone curve
//! and a tested copy of the shader's lighting for one face.
//!
//! The light follows the sky shader's sun path: east (+X) at sunrise, up
//! at noon, west (−X) at sunset, no tilt. The moon is opposite. Both
//! fade to nothing at the horizon, where the light switches from one to
//! the other, so shadows never pop.

use std::f32::consts::TAU;

use glam::{
    Mat4, Vec3,
    camera::rh::{proj::directx::orthographic, view::look_to_mat4},
};

use super::Quality;

/// The sun's direction (toward the sun) for a celestial angle (0 noon,
/// 0.25 sunset, 0.5 midnight, 0.75 sunrise), as `sky.wgsl` draws it.
pub fn sun_direction(angle: f32) -> Vec3 {
    let a = angle * TAU;
    Vec3::new(-a.sin(), a.cos(), 0.0)
}

/// The moonlight's strength next to full sunlight.
pub const MOON_STRENGTH: f32 = 0.16;
/// How far above (or below) the horizon the light has faded in.
pub const FADE_ELEVATION: f32 = 0.15;
/// Rain and thunder dim the sun to this share at full rain.
pub const RAIN_DIM: f32 = 0.75;

/// Daylight: warm white. Low sun: orange. Moon: dim blue. (sRGB-ish
/// multipliers on the surface's color, 0–1.)
pub const SUN_WHITE: Vec3 = Vec3::new(1.0, 0.96, 0.88);
pub const SUN_ORANGE: Vec3 = Vec3::new(1.0, 0.52, 0.22);
pub const MOON_BLUE: Vec3 = Vec3::new(0.42, 0.55, 1.0);

/// The light in the sky this frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Light {
    /// Unit vector toward the light: the sun above the horizon, else the
    /// moon.
    pub direction: Vec3,
    /// Its color times its strength (0 at the horizon).
    pub color: Vec3,
    /// 0–1: how strong it is.
    pub strength: f32,
    /// The light's height above the horizon (sine of its elevation).
    pub elevation: f32,
    /// It's the sun (not the moon) that's up.
    pub by_day: bool,
}

fn smoothstep(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn mix(a: Vec3, b: Vec3, t: f32) -> Vec3 {
    a + (b - a) * t
}

/// The light at celestial angle `angle` with `rain` (0–1) dimming it.
/// With `sunlight` false (no sun in the dimension) there is none.
pub fn light(angle: f32, rain: f32, sunlight: bool) -> Light {
    let sun = sun_direction(angle);
    let by_day = sun.y >= 0.0;
    let direction = if by_day { sun } else { -sun };
    let elevation = direction.y.max(0.0);
    let fade = smoothstep(0.0, FADE_ELEVATION, elevation);
    let rain = 1.0 - RAIN_DIM * rain.clamp(0.0, 1.0);
    let (color, strength) = if !sunlight {
        (Vec3::ZERO, 0.0)
    } else if by_day {
        let tint = mix(SUN_ORANGE, SUN_WHITE, smoothstep(0.05, 0.5, elevation));
        (tint * fade * rain, fade * rain)
    } else {
        (
            MOON_BLUE * MOON_STRENGTH * fade * rain,
            MOON_STRENGTH * fade * rain,
        )
    };
    Light {
        direction,
        color,
        strength,
        elevation,
        by_day,
    }
}

/// How strong the volumetric light shafts are (0–1.5): none when the sun
/// is below the horizon, with no sun in the dimension (the Nether, the
/// End), or by night; strongest at sunrise and sunset, when the sun is
/// low, and stronger in the mist of rain. `sunlight` is whether the
/// dimension has a sun.
pub fn shaft_strength(light: &Light, rain: f32, sunlight: bool) -> f32 {
    if !sunlight || !light.by_day || light.direction.y <= 0.0 {
        return 0.0;
    }
    let fade = smoothstep(0.0, FADE_ELEVATION, light.elevation);
    let low = 1.0 - light.elevation;
    (0.25 + 0.75 * low * low) * fade * (1.0 + 0.6 * rain.clamp(0.0, 1.0))
}

/// How much of the light reaches a face with `sky` light (0–15): none in
/// the dark, all in the open, smoothly between.
pub fn sky_factor(sky: f32) -> f32 {
    smoothstep(0.0, 15.0, sky.clamp(0.0, 15.0))
}

/// The shadow maps of one quality profile.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShadowConfig {
    pub cascades: usize,
    /// Width and height of each cascade.
    pub size: u32,
    /// The distance (blocks) each cascade reaches, near to far.
    pub reach: [f32; 3],
    /// Samples a lookup takes.
    pub taps: u32,
    /// The far cascades refresh every other frame (Medium).
    pub alternate: bool,
    /// Soft edges that widen with the distance from the caster (High).
    pub soft: bool,
}

impl ShadowConfig {
    pub fn of(quality: Quality) -> Self {
        match quality {
            Quality::Low => Self {
                cascades: 1,
                size: 1024,
                reach: [40.0, 40.0, 40.0],
                taps: 4,
                alternate: false,
                soft: false,
            },
            Quality::Medium => Self {
                cascades: 2,
                size: 2048,
                reach: [24.0, 96.0, 96.0],
                taps: 8,
                alternate: true,
                soft: false,
            },
            Quality::High => Self {
                cascades: 3,
                size: 2048,
                reach: [16.0, 64.0, 160.0],
                taps: 16,
                alternate: false,
                soft: true,
            },
        }
    }

    /// The last cascade's reach: where shadows fade out.
    pub fn end(&self) -> f32 {
        self.reach[self.cascades - 1]
    }

    /// A world-space texel of cascade `i`, in blocks.
    pub fn texel(&self, i: usize) -> f32 {
        2.0 * self.reach[i] / self.size as f32
    }
}

/// How far toward the light a cascade's box reaches past its center, so
/// tall things off to the side still cast into it.
pub const TOWARD_LIGHT: f32 = 420.0;

/// A cascade: a box round `center` (a sphere of `radius`) seen from the
/// light, its center snapped to whole texels of the `size`² map so it
/// doesn't shimmer when the camera moves. Returns the view-projection and
/// the snapped center in light space (x, y).
pub fn cascade(center: Vec3, radius: f32, direction: Vec3, size: u32) -> (Mat4, [f32; 2]) {
    let up = if direction.y.abs() > 0.99 {
        Vec3::Z
    } else {
        Vec3::Y
    };
    // Looking away from the light.
    let view = look_to_mat4(Vec3::ZERO, -direction, up);
    let c = view.transform_point3(center);
    let texel = 2.0 * radius / size as f32;
    let snapped = [(c.x / texel).round() * texel, (c.y / texel).round() * texel];
    // Light-space z grows toward the light; depth 0 is the near side.
    let near = -(c.z + radius + TOWARD_LIGHT);
    let far = -(c.z - radius);
    let proj = orthographic(-radius, radius, -radius, radius, near, far);
    let recentre = Mat4::from_translation(Vec3::new(-snapped[0], -snapped[1], 0.0));
    (proj * recentre * view, snapped)
}

/// The tone curve (M55), on luminance: the identity up to `SHOULDER`, then
/// a soft shoulder toward 1, so lit midtones are untouched and bright sun
/// doesn't clip.
pub const SHOULDER: f32 = 0.8;

pub fn tone(luminance: f32) -> f32 {
    tone_from(luminance, SHOULDER)
}

/// The curve with its shoulder at `shoulder`; at 1 it's the identity up to
/// the display's white (a mod's brightness override uses that, so nothing
/// is darker than in Vanilla).
pub fn tone_from(luminance: f32, shoulder: f32) -> f32 {
    if luminance <= shoulder {
        luminance
    } else if shoulder >= 1.0 {
        1.0
    } else {
        let room = 1.0 - shoulder;
        shoulder + room * (1.0 - (-(luminance - shoulder) / room).exp())
    }
}

/// Saturation after the curve: about +10%.
pub const SATURATION: f32 = 1.10;

pub fn luminance(c: Vec3) -> f32 {
    c.dot(Vec3::new(0.2126, 0.7152, 0.0722))
}

/// The curve and saturation on a linear color.
pub fn tone_map(c: Vec3) -> Vec3 {
    tone_map_from(c, SHOULDER)
}

pub fn tone_map_from(c: Vec3, shoulder: f32) -> Vec3 {
    let l = luminance(c);
    let scaled = if l > 1e-5 {
        c * (tone_from(l, shoulder) / l)
    } else {
        c
    };
    let l = luminance(scaled);
    (Vec3::splat(l) + (scaled - Vec3::splat(l)) * SATURATION).max(Vec3::ZERO)
}

pub fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

pub fn linear_to_srgb(c: f32) -> f32 {
    if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

/// What the ambient light is worth next to full sun and what the sun adds,
/// so an unshadowed face in full noon sun comes out about as bright as
/// Vanilla's.
pub const AMBIENT_SCALE: f32 = 0.72;
pub const DIRECT_SCALE: f32 = 0.32;

/// The shader's light on one face, as sRGB multipliers on the albedo: the
/// vanilla lightmap `vanilla` (what Vanilla multiplies by) scaled to
/// ambient, plus the sun's direct light. `n_dot_l` 0–1, `shadow` 0–1
/// (1 lit), `sky` 0–15 the face's sky light, `fullbright` a mod's
/// brightness override (nothing is darker than in Vanilla then).
pub fn face_lighting(
    vanilla: Vec3,
    sun: &Light,
    n_dot_l: f32,
    shadow: f32,
    sky: f32,
    fullbright: bool,
) -> Vec3 {
    let direct = sun.color * (n_dot_l.clamp(0.0, 1.0) * shadow * sky_factor(sky) * DIRECT_SCALE);
    let lit = vanilla * AMBIENT_SCALE + direct;
    if fullbright { lit.max(vanilla) } else { lit }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::daytime;

    fn near(a: Vec3, b: Vec3) -> bool {
        (a - b).length() < 1e-4
    }

    /// 0 is sunrise, 6000 noon, 12000 sunset, 18000 midnight (vanilla's
    /// day starts at sunrise).
    #[test]
    fn the_sun_follows_vanillas_angles() {
        let at = |ticks| sun_direction(daytime::celestial_angle(ticks, 0.0));
        // Sunrise in the east (+X), on the horizon.
        let sunrise = at(0);
        assert!(sunrise.x > 0.95 && sunrise.y.abs() < 0.3, "{sunrise:?}");
        assert!(near(at(6000), Vec3::Y));
        let sunset = at(12000);
        assert!(sunset.x < -0.95 && sunset.y.abs() < 0.3, "{sunset:?}");
        assert!(near(at(18000), Vec3::NEG_Y));
        // No tilt: the path is in the XY plane.
        for t in (0..24000).step_by(500) {
            assert_eq!(at(t).z, 0.0);
        }
        // The vanilla sunset/sunrise angles themselves.
        for (ticks, angle) in [(0, 0.75), (6000, 0.0), (12000, 0.25), (18000, 0.5)] {
            let got = daytime::celestial_angle(ticks, 0.0);
            assert!(
                (got - angle).abs() < 0.04 || (got - angle - 1.0).abs() < 0.04,
                "{ticks}: {got}"
            );
        }
    }

    #[test]
    fn the_light_switches_between_sun_and_moon_at_zero_strength() {
        let before = light(0.2499, 0.0, true);
        let after = light(0.2501, 0.0, true);
        // Over the horizon the direction flips...
        let low = |a: f32| light(a, 0.0, true);
        assert!(low(0.2).direction.y > 0.0 && low(0.3).direction.y > 0.0);
        // ...but strength is 0 there on both sides, so nothing pops.
        let flip = (0..2000)
            .map(|i| i as f32 / 2000.0)
            .collect::<Vec<_>>()
            .windows(2)
            .filter(|w| {
                light(w[0], 0.0, true)
                    .direction
                    .dot(light(w[1], 0.0, true).direction)
                    < 0.0
            })
            .count();
        assert_eq!(flip, 2, "once at sunset and once at sunrise");
        assert!(before.strength < 0.2 && after.strength < 0.2);
        let mut last = light(0.0, 0.0, true).color;
        for i in 1..=4000 {
            let now = light(i as f32 / 4000.0, 0.0, true).color;
            assert!(
                (now - last).length() < 0.02,
                "pop at {i}: {last:?} -> {now:?}"
            );
            last = now;
        }
    }

    #[test]
    fn day_is_warm_white_dusk_orange_night_dim_blue_and_rain_dims() {
        let noon = light(0.0, 0.0, true);
        assert!(noon.color.x > 0.95 && noon.color.z > 0.8 && noon.strength > 0.99);
        let dusk = light(0.235, 0.0, true);
        assert!(
            dusk.color.x > dusk.color.y * 1.3 && dusk.color.y > dusk.color.z,
            "{dusk:?}"
        );
        let night = light(0.5, 0.0, true);
        assert!(night.color.z > night.color.x * 2.0 && night.strength < 0.2);
        let storm = light(0.0, 1.0, true);
        assert!((storm.strength - (1.0 - RAIN_DIM)).abs() < 1e-5);
        assert_eq!(light(0.0, 0.0, false).strength, 0.0);
    }

    #[test]
    fn shafts_are_gone_at_night_and_in_the_nether_and_strongest_when_the_sun_is_low() {
        let strength = |angle: f32, sunlight: bool| {
            shaft_strength(&light(angle, 0.0, sunlight), 0.0, sunlight)
        };
        // Below the horizon (midnight, and just after sunset): none.
        assert_eq!(strength(0.5, true), 0.0);
        assert_eq!(strength(0.3, true), 0.0);
        // No sun in the dimension: none, at any hour.
        for a in [0.0, 0.1, 0.2, 0.7, 0.9] {
            assert_eq!(strength(a, false), 0.0, "{a}");
        }
        // Noon is weaker than a low sun; and a low sun beats a faded one on
        // the horizon.
        let noon = strength(0.0, true);
        let low = strength(0.2, true);
        assert!(noon > 0.2 && low > noon * 1.5, "{noon} {low}");
        assert!(strength(0.249, true) < low, "fades out at the horizon");
        // Rain mist makes them stronger.
        assert!(shaft_strength(&light(0.0, 1.0, true), 1.0, true) > noon * 1.3);
    }

    #[test]
    fn shadow_profiles_match_the_milestone() {
        let low = ShadowConfig::of(Quality::Low);
        assert_eq!(
            (low.cascades, low.size, low.taps, low.end()),
            (1, 1024, 4, 40.0)
        );
        let medium = ShadowConfig::of(Quality::Medium);
        assert_eq!((medium.cascades, medium.size, medium.taps), (2, 2048, 8));
        assert_eq!(&medium.reach[..2], &[24.0, 96.0]);
        assert!(medium.alternate);
        let high = ShadowConfig::of(Quality::High);
        assert_eq!(
            (high.cascades, high.taps, high.reach),
            (3, 16, [16.0, 64.0, 160.0])
        );
    }

    #[test]
    fn a_cascade_does_not_shimmer_when_the_camera_moves_less_than_a_texel() {
        let light = light(0.05, 0.0, true).direction;
        let radius = 24.0;
        let size = 2048;
        let texel = 2.0 * radius / size as f32;
        // A camera standing exactly on a snap point (in light space).
        let up = Vec3::Y;
        let view = look_to_mat4(Vec3::ZERO, -light, up);
        let start = view
            .inverse()
            .transform_point3(Vec3::new(10.0 * texel, -7.0 * texel, 3.0));
        let (base, snap) = cascade(start, radius, light, size);
        // Half a texel along the light's x and y axes: same projection.
        let right = view.inverse().transform_vector3(Vec3::X);
        let above = view.inverse().transform_vector3(Vec3::Y);
        for offset in [
            right * texel * 0.45,
            above * -texel * 0.45,
            (right + above) * texel * 0.3,
        ] {
            let (moved, snapped) = cascade(start + offset, radius, light, size);
            assert_eq!(snapped, snap);
            assert!(base.abs_diff_eq(moved, 1e-4), "projection changed");
        }
        // A whole texel moves it by exactly a texel.
        let (_, next) = cascade(start + right * texel * 1.01, radius, light, size);
        assert!((next[0] - snap[0] - texel).abs() < 1e-4);
    }

    #[test]
    fn a_cascade_holds_the_points_round_its_center() {
        let sun = light(0.07, 0.0, true).direction;
        let center = Vec3::new(120.0, 70.0, -30.0);
        let (m, _) = cascade(center, 24.0, sun, 2048);
        for d in [Vec3::X, -Vec3::X, Vec3::Z, -Vec3::Z, Vec3::Y, -Vec3::Y] {
            let p = m.project_point3(center + d * 20.0);
            assert!(p.x.abs() <= 1.0 && p.y.abs() <= 1.0, "{d:?} {p:?}");
            assert!((0.0..=1.0).contains(&p.z), "{d:?} {p:?}");
        }
        // Something between the center and the light is in front (nearer).
        let toward = m.project_point3(center + sun * 200.0).z;
        assert!(toward < m.project_point3(center).z && toward >= 0.0);
    }

    /// A closed stone room at noon has no sky light inside: the sun adds
    /// nothing anywhere in it, whatever the shadow map says.
    #[test]
    fn no_sun_where_there_is_no_sky_light() {
        let sun = light(0.0, 0.0, true);
        let torchlit = Vec3::splat(0.6);
        for n_dot_l in [0.0, 0.5, 1.0] {
            let lit = face_lighting(torchlit, &sun, n_dot_l, 1.0, 0.0, false);
            assert!(near(lit, torchlit * AMBIENT_SCALE), "{lit:?}");
        }
        assert_eq!(sky_factor(0.0), 0.0);
        assert_eq!(sky_factor(15.0), 1.0);
        assert!(sky_factor(7.0) > 0.4 && sky_factor(7.0) < 0.6);
    }

    #[test]
    fn fullbright_is_never_darker_than_vanilla() {
        let sun = light(0.0, 0.0, true);
        for sky in 0..=15 {
            for block in 0..=15 {
                // The vanilla lightmap value for the level (a stand-in
                // with its shape: rising with either light).
                let level = sky.max(block) as f32 / 15.0;
                let vanilla = Vec3::splat(level);
                for (n_dot_l, shadow) in [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.3, 0.5)] {
                    let lit = face_lighting(vanilla, &sun, n_dot_l, shadow, sky as f32, true);
                    assert!(
                        lit.cmpge(vanilla).all(),
                        "{sky} {block}: {lit:?} < {vanilla:?}"
                    );
                }
            }
        }
    }

    /// A fully sky-lit grass top at noon in full sun is within 10% of
    /// Vanilla's brightness after the tone curve.
    #[test]
    fn noon_grass_is_within_ten_percent_of_vanilla() {
        let sun = light(0.0, 0.0, true);
        // The grass-top color of the default biome tint in sRGB, and the
        // vanilla lightmap at sky 15 in the day (about 1).
        let grass = Vec3::new(0.55, 0.74, 0.28);
        let vanilla_light = Vec3::splat(0.98);
        let vanilla = grass * vanilla_light;
        let lit = face_lighting(vanilla_light, &sun, 1.0, 1.0, 15.0, false);
        let linear = (grass * lit).to_array().map(srgb_to_linear);
        let mapped = tone_map(Vec3::from_array(linear))
            .to_array()
            .map(linear_to_srgb);
        let ours = luminance(Vec3::from_array(mapped.map(srgb_to_linear)));
        let theirs = luminance(vanilla.to_array().map(srgb_to_linear).into());
        let ratio = ours / theirs;
        assert!((0.9..=1.1).contains(&ratio), "ratio {ratio}");
    }

    /// With a brightness override the curve and saturation never lower a
    /// pixel's luminance below what Vanilla would show (at most white).
    #[test]
    fn fullbright_tone_never_darkens() {
        for i in 0..=100 {
            for hue in [
                Vec3::new(1.0, 0.5, 0.2),
                Vec3::new(0.3, 1.0, 0.6),
                Vec3::ONE,
            ] {
                let c = hue * (i as f32 / 100.0);
                let out = tone_map_from(c, 1.0);
                assert!(luminance(out) >= luminance(c) - 1e-4, "{c:?} -> {out:?}");
            }
        }
    }

    #[test]
    fn the_tone_curve_is_untouched_below_the_shoulder_and_never_clips() {
        for l in [0.0, 0.1, 0.5, SHOULDER] {
            assert_eq!(tone(l), l);
        }
        assert!(tone(1.0) < 1.0 && tone(3.0) < 1.0 && tone(100.0) <= 1.0);
        let mut last = 0.0;
        for i in 0..400 {
            let t = tone(i as f32 / 100.0);
            assert!(t >= last);
            last = t;
        }
        // A gray stays gray; a color gets about 10% more saturated.
        let gray = tone_map(Vec3::splat(0.3));
        assert!(near(gray, Vec3::splat(0.3)));
        let c = tone_map(Vec3::new(0.4, 0.2, 0.1));
        let before =
            (Vec3::new(0.4, 0.2, 0.1) - Vec3::splat(luminance(Vec3::new(0.4, 0.2, 0.1)))).length();
        let after = (c - Vec3::splat(luminance(c))).length();
        assert!((after / before - SATURATION).abs() < 0.02);
    }
}
