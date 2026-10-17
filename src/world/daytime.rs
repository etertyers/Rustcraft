//! The day/night cycle, following 1.0.0: a day is 24000 ticks (20
//! minutes), starting at sunrise. These functions turn the time into the
//! sun's position, how bright the sky is, and the sky, fog and sunrise
//! colors (1.0.0's `WorldProvider.calculateCelestialAngle`,
//! `World.getSkyColor`, `getFogColor`, `calcSunriseSunsetColors`,
//! `getStarBrightness` and the sun brightness used by the lightmap).

use std::f32::consts::{PI, TAU};

use glam::Vec3;

/// Ticks in a full day.
pub const DAY_TICKS: u64 = 24000;

/// Where the sun is: 0 at noon, 0.25 at sunset, 0.5 at midnight, 0.75 at
/// sunrise. Days last a little longer than nights, as in 1.0.0.
pub fn celestial_angle(time: u64, partial: f32) -> f32 {
    let mut f = ((time % DAY_TICKS) as f32 + partial) / DAY_TICKS as f32 - 0.25;
    if f < 0.0 {
        f += 1.0;
    }
    if f > 1.0 {
        f -= 1.0;
    }
    let eased = 1.0 - ((f * PI).cos() + 1.0) / 2.0;
    f + (eased - f) / 3.0
}

/// Unit vector toward the sun: up at noon, west (−X) at sunset, east at
/// sunrise. The moon is opposite. (sky.wgsl computes the same.)
#[cfg(test)]
pub fn sun_direction(angle: f32) -> Vec3 {
    let a = angle * TAU;
    Vec3::new(-a.sin(), a.cos(), 0.0)
}

/// Daylight, 0.2 (night) to 1 (day): scales sky light in the lightmap.
pub fn sun_brightness(angle: f32) -> f32 {
    let dark = (1.0 - ((angle * TAU).cos() * 2.0 + 0.2)).clamp(0.0, 1.0);
    (1.0 - dark) * 0.8 + 0.2
}

/// Sky light lost to the time of day: 0 by day, 11 at night (1.0.0's
/// `skylightSubtracted`). Mob spawning and burning use the reduced level.
pub fn sky_light_subtracted(angle: f32) -> u8 {
    let dark = (1.0 - ((angle * TAU).cos() * 2.0 + 0.5)).clamp(0.0, 1.0);
    (dark * 11.0) as u8
}

/// 1.0.0's day: while the sky is losing less than 4 light levels.
pub fn is_daytime(angle: f32) -> bool {
    sky_light_subtracted(angle) < 4
}

/// 1.0.0's light level to brightness table (0.0 dark to 1.0 bright).
pub fn brightness(level: u8) -> f32 {
    let dark = 1.0 - level.min(15) as f32 / 15.0;
    (1.0 - dark) / (dark * 3.0 + 1.0)
}

/// How much daylight colors the sky and fog, 0 (night) to 1 (day).
fn sky_light(angle: f32) -> f32 {
    ((angle * TAU).cos() * 2.0 + 0.5).clamp(0.0, 1.0)
}

/// Sky color overhead for a biome at `temperature` (sRGB, 0–1).
pub fn sky_color(angle: f32, temperature: f32) -> Vec3 {
    biome_sky_color(temperature) * sky_light(angle)
}

/// 1.0.0's `BiomeGenBase.getSkyColorByTemp`: bluer when cold.
fn biome_sky_color(temperature: f32) -> Vec3 {
    let t = (temperature / 3.0).clamp(-1.0, 1.0);
    hsb_to_rgb(0.622_222_2 - t * 0.05, 0.5 + t * 0.1, 1.0)
}

/// Fog color near the horizon (sRGB, 0–1), before mixing in the sky color.
pub fn fog_color(angle: f32) -> Vec3 {
    let light = sky_light(angle);
    Vec3::new(
        0.752_941_2 * (light * 0.94 + 0.06),
        0.847_058_8 * (light * 0.94 + 0.06),
        light * 0.91 + 0.09,
    )
}

/// The sunrise or sunset glow near the horizon: color and strength, or
/// `None` while the sun is well up or down.
pub fn sunrise_color(angle: f32) -> Option<(Vec3, f32)> {
    const SPREAD: f32 = 0.4;
    let height = (angle * TAU).cos();
    if height.abs() > SPREAD {
        return None;
    }
    let f = height / SPREAD * 0.5 + 0.5;
    let alpha = 1.0 - (1.0 - (f * PI).sin()) * 0.99;
    Some((
        Vec3::new(f * 0.3 + 0.7, f * f * 0.7 + 0.2, 0.2),
        alpha * alpha,
    ))
}

/// How bright the stars are, 0 by day to 0.5 at night.
pub fn star_brightness(angle: f32) -> f32 {
    let f = (1.0 - ((angle * TAU).cos() * 2.0 + 0.75)).clamp(0.0, 1.0);
    f * f * 0.5
}

/// Clock time like "day 1, 06:00": tick 0 is 6 in the morning of day 1.
pub fn clock(time: u64) -> String {
    let day = time / DAY_TICKS + 1;
    let ticks = (time + 6000) % DAY_TICKS;
    let (hours, minutes) = (ticks / 1000, ticks % 1000 * 60 / 1000);
    format!("day {day}, {hours:02}:{minutes:02}")
}

/// Moon phase 0–7 (0 full), one per day.
pub fn moon_phase(time: u64) -> u32 {
    ((time / DAY_TICKS) % 8) as u32
}

/// Java's `Color.HSBtoRGB`, as 0–1 floats.
fn hsb_to_rgb(hue: f32, saturation: f32, brightness: f32) -> Vec3 {
    if saturation == 0.0 {
        return Vec3::splat(brightness);
    }
    let h = (hue - hue.floor()) * 6.0;
    let f = h - h.floor();
    let p = brightness * (1.0 - saturation);
    let q = brightness * (1.0 - saturation * f);
    let t = brightness * (1.0 - saturation * (1.0 - f));
    let (r, g, b) = match h as u32 {
        0 => (brightness, t, p),
        1 => (q, brightness, p),
        2 => (p, brightness, t),
        3 => (p, q, brightness),
        4 => (t, p, brightness),
        _ => (brightness, p, q),
    };
    // Java rounds each channel to a byte.
    Vec3::new(r, g, b).map(|c| (c * 255.0 + 0.5).floor() / 255.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-3
    }

    #[test]
    fn the_sun_is_up_from_tick_0_to_12000_and_highest_at_6000() {
        // The eased angle puts the sun a little above the horizon at both ends.
        let morning = sun_direction(celestial_angle(0, 0.0));
        assert!(
            morning.x > 0.95 && morning.y > 0.0,
            "east, just risen: {morning}"
        );
        let evening = sun_direction(celestial_angle(12000, 0.0));
        assert!(
            evening.x < -0.95 && evening.y > 0.0,
            "west, about to set: {evening}"
        );
        assert!(close(celestial_angle(6000, 0.0), 0.0), "noon");
        assert!(close(celestial_angle(18000, 0.0), 0.5), "midnight");
        assert!(
            close(celestial_angle(30000, 0.0), 0.0),
            "every day the same"
        );
        assert!(sun_direction(celestial_angle(18000, 0.0)).y < -0.99);
    }

    #[test]
    fn brightness_and_colors_follow_1_0_0() {
        let (noon, midnight) = (0.0, 0.5);
        assert_eq!(sun_brightness(noon), 1.0);
        assert!(close(sun_brightness(midnight), 0.2));
        assert_eq!(star_brightness(noon), 0.0);
        assert_eq!(star_brightness(midnight), 0.5);
        // Plains (temperature 0.8) noon sky: HSB(0.6089, 0.5267, 1) through
        // Java's HSBtoRGB.
        let sky = sky_color(noon, 0.8) * 255.0;
        assert_eq!(sky.round(), Vec3::new(121.0, 167.0, 255.0));
        assert_eq!(sky_color(midnight, 0.8), Vec3::ZERO);
        let fog = fog_color(noon);
        assert!(close(fog.x, 0.7529) && close(fog.z, 1.0));
        assert!(sunrise_color(noon).is_none());
        let (glow, alpha) = sunrise_color(0.25).unwrap();
        assert!(alpha > 0.9 && glow.x > glow.z, "orange at sunset");
        assert_eq!(clock(0), "day 1, 06:00");
        assert_eq!(clock(6000), "day 1, 12:00");
        assert_eq!(clock(18500), "day 1, 00:30");
        assert_eq!(clock(DAY_TICKS + 12000), "day 2, 18:00");
        assert_eq!(sky_light_subtracted(noon), 0);
        assert_eq!(sky_light_subtracted(midnight), 11);
        assert!(is_daytime(noon) && !is_daytime(midnight));
        assert_eq!(brightness(15), 1.0);
        assert_eq!(brightness(0), 0.0);
        assert_eq!(moon_phase(0), 0);
        assert_eq!(moon_phase(DAY_TICKS * 9), 1);
    }
}
