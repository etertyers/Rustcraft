//! The sky, light and fog to draw each frame with, from the time of day,
//! the biome, where the player looks and whether their eyes are under water
//! (1.0.0's `EntityRenderer.updateFogColor` and `setupFog`).

use glam::Vec3;

use super::Game;
use crate::{
    config::{CHUNK_SIZE, CLOUD_HEIGHT, RENDER_DISTANCE},
    render::Environment,
    world::{biome::Biome, block::Fluid, daytime, dimension::Dimension},
};

/// 1.0.0 mixes this much of the sky color into the fog at its "far" render
/// distance: 1 − (1/4)^0.25.
const SKY_IN_FOG: f32 = 0.292_893_2;
/// Linear fog starts at this fraction of the render distance (1.0.0).
const FOG_START: f32 = 0.25;
/// Exponential fog under water and in lava, and their colors (1.0.0).
const WATER_FOG: (f32, Vec3) = (0.1, Vec3::new(0.02, 0.02, 0.2));
const LAVA_FOG: (f32, Vec3) = (2.0, Vec3::new(0.6, 0.1, 0.0));

impl Game {
    /// The liquid the camera is in, if any.
    pub fn eye_in(&self) -> Option<Fluid> {
        self.fluid_at(self.camera.position.as_dvec3())
    }

    /// The sound event music is taken from where the player is: the credits'
    /// poem, the End (with its own track for the dragon fight), the Nether's
    /// biomes, creative mode, under water, else the game's tracks.
    pub fn music_event(&self) -> Option<&'static str> {
        if self.credits.is_some() {
            return Some("music.credits");
        }
        Some(match self.dimension {
            Dimension::End if self.dragon.is_some() => "music.dragon",
            Dimension::End => "music.end",
            Dimension::Nether => {
                let p = self.camera.position.floor().as_ivec3();
                match self.chunks.world().biome_at(p.x, p.z).unwrap_or_default() {
                    Biome::CrimsonForest => "music.nether.crimson_forest",
                    Biome::SoulSandValley => "music.nether.soul_sand_valley",
                    Biome::BasaltDeltas => "music.nether.basalt_deltas",
                    // The warped forest has no tracks of its own.
                    _ => "music.nether.nether_wastes",
                }
            }
            Dimension::Overworld if self.eye_in() == Some(Fluid::Water) => "music.under_water",
            Dimension::Overworld if self.mode == crate::player::mode::GameMode::Creative => {
                "music.creative"
            }
            Dimension::Overworld => "music.game",
        })
    }

    pub fn environment(&self) -> Environment {
        let p = self.camera.position.floor().as_ivec3();
        let biome = self.chunks.world().biome_at(p.x, p.z).unwrap_or_default();
        let view = (self.chunks.radius() * CHUNK_SIZE as i32) as f32;
        let mut environment = match self.dimension {
            Dimension::Nether => nether(
                self.time,
                self.tick_alpha() as f32,
                biome,
                view,
                self.eye_in(),
            ),
            Dimension::End => end(self.time, self.tick_alpha() as f32, view, self.eye_in()),
            Dimension::Overworld => self.overworld_environment(biome, view),
        };
        // Night vision lights everything, flickering in its last 10 s
        // (modern `GameRenderer.getNightVisionScale`).
        if let Some(active) = self
            .stats
            .effects
            .get(crate::player::effects::Effect::NightVision)
        {
            let d = active.duration as f32;
            let strength = if d > 200.0 {
                1.0
            } else {
                0.7 + 0.3 * (d * std::f32::consts::PI * 0.2).sin()
            };
            environment.ambient = environment.ambient.max(strength);
        }
        environment
    }

    fn overworld_environment(&self, biome: Biome, view: f32) -> Environment {
        let mut environment = environment(
            self.time,
            self.tick_alpha() as f32,
            biome,
            self.camera.forward(),
            self.eye_in(),
        );
        // Fog ends at the chosen render distance.
        let fog_end = view;
        environment.fog_start *= fog_end / environment.fog_end;
        environment.fog_end = fog_end;
        environment
    }
}

/// The environment at game time `time` (plus `partial` of a tick).
pub fn environment(
    time: u64,
    partial: f32,
    biome: Biome,
    look: Vec3,
    eye: Option<Fluid>,
) -> Environment {
    let angle = daytime::celestial_angle(time, partial);
    let sky_color = daytime::sky_color(angle, biome.climate().0);
    let mut fog_color = daytime::fog_color(angle);
    fog_color += (sky_color - fog_color) * SKY_IN_FOG;
    let sunrise = daytime::sunrise_color(angle);
    if let Some((glow, strength)) = sunrise {
        // Looking toward the rising or setting sun tints the fog.
        let sun_side = if (angle * std::f32::consts::TAU).sin() > 0.0 {
            Vec3::NEG_X
        } else {
            Vec3::X
        };
        let facing = look.dot(sun_side).max(0.0) * strength;
        fog_color += (glow - fog_color) * facing;
    }
    let fog_end = (RENDER_DISTANCE * CHUNK_SIZE as i32) as f32;
    let (fog_density, fog_color) = match eye {
        None => (0.0, fog_color),
        Some(Fluid::Water) => WATER_FOG,
        Some(Fluid::Lava) => LAVA_FOG,
    };
    Environment {
        time: time as f32 + partial,
        celestial_angle: angle,
        sky_color,
        fog_color,
        fog_start: fog_end * FOG_START,
        fog_end,
        fog_density,
        sun_brightness: daytime::sun_brightness(angle),
        star_brightness: daytime::star_brightness(angle),
        moon_phase: daytime::moon_phase(time),
        sunrise,
        cloud_height: CLOUD_HEIGHT,
        sky: true,
        ambient: 0.0,
    }
}

/// The Nether's: no sky, no sun, thick fog the color of the biome, and
/// a little ambient light (modern `NetherEffects`, `FogRenderer`'s thick
/// fog, and the Nether's 0.1 ambient light). `view` is the render
/// distance in blocks.
pub fn nether(time: u64, partial: f32, biome: Biome, view: f32, eye: Option<Fluid>) -> Environment {
    let fog = biome.nether_fog().unwrap_or(0x330808);
    let fog = Vec3::new(
        (fog >> 16 & 0xFF) as f32,
        (fog >> 8 & 0xFF) as f32,
        (fog & 0xFF) as f32,
    ) / 255.0;
    let (fog_density, fog_color) = match eye {
        None => (0.0, fog),
        Some(Fluid::Water) => WATER_FOG,
        Some(Fluid::Lava) => LAVA_FOG,
    };
    Environment {
        time: time as f32 + partial,
        celestial_angle: 0.0,
        sky_color: fog_color,
        fog_color,
        fog_start: view * NETHER_FOG_START,
        fog_end: view.min(NETHER_FOG_REACH) * NETHER_FOG_END,
        fog_density,
        sun_brightness: 0.0,
        star_brightness: 0.0,
        moon_phase: 0,
        sunrise: None,
        cloud_height: CLOUD_HEIGHT,
        sky: false,
        ambient: NETHER_AMBIENT,
    }
}

/// The End's: no sun, moon, stars or clouds, a near-black purple sky and
/// fog (modern `EndEffects`, fog 0x181318), and bright light (modern's
/// `forceBrightLightmap`, here a high ambient light) since the End has no
/// sky light. `view` is the render distance in blocks.
pub fn end(time: u64, partial: f32, view: f32, eye: Option<Fluid>) -> Environment {
    let dark = Vec3::new(0x18 as f32, 0x13 as f32, 0x18 as f32) / 255.0;
    let (fog_density, fog_color) = match eye {
        None => (0.0, dark),
        Some(Fluid::Water) => WATER_FOG,
        Some(Fluid::Lava) => LAVA_FOG,
    };
    Environment {
        time: time as f32 + partial,
        celestial_angle: 0.0,
        sky_color: dark,
        fog_color,
        fog_start: view * FOG_START,
        fog_end: view,
        fog_density,
        sun_brightness: 0.0,
        star_brightness: 0.0,
        moon_phase: 0,
        sunrise: None,
        cloud_height: CLOUD_HEIGHT,
        sky: false,
        ambient: END_AMBIENT,
    }
}

/// How much the End's dark is lifted: its blocks are lit as if by a soft
/// glow from everywhere, though it has no sky light.
const END_AMBIENT: f32 = 0.6;

/// Modern thick fog: from 5% of the render distance to half of it (at
/// most 192 blocks).
const NETHER_FOG_START: f32 = 0.05;
const NETHER_FOG_END: f32 = 0.5;
const NETHER_FOG_REACH: f32 = 192.0;
/// Modern `DimensionType` ambient light of the Nether.
const NETHER_AMBIENT: f32 = 0.1;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noon_is_bright_and_midnight_dark() {
        let noon = environment(6000, 0.0, Biome::Plains, Vec3::NEG_Z, None);
        assert_eq!(noon.sun_brightness, 1.0);
        assert_eq!(noon.star_brightness, 0.0);
        assert!(noon.sky_color.z > 0.9);
        assert_eq!((noon.fog_start, noon.fog_end), (36.0, 144.0));
        let midnight = environment(18000, 0.0, Biome::Plains, Vec3::NEG_Z, None);
        assert!(midnight.sun_brightness < 0.21);
        assert_eq!(midnight.sky_color, Vec3::ZERO);
        assert!(midnight.fog_color.length() < 0.2);
    }

    #[test]
    fn under_water_the_fog_is_thick_and_blue() {
        let env = environment(6000, 0.0, Biome::Ocean, Vec3::NEG_Z, Some(Fluid::Water));
        assert_eq!((env.fog_density, env.fog_color), WATER_FOG);
        let lava = environment(6000, 0.0, Biome::Plains, Vec3::NEG_Z, Some(Fluid::Lava));
        assert_eq!(lava.fog_density, 2.0);
    }

    #[test]
    fn the_nether_has_thick_fog_the_color_of_its_biome_and_no_sky() {
        let wastes = nether(6000, 0.0, Biome::NetherWastes, 144.0, None);
        assert!(!wastes.sky);
        assert_eq!(wastes.ambient, 0.1);
        assert!((wastes.fog_start - 7.2).abs() < 1e-4);
        assert_eq!(wastes.fog_end, 72.0);
        assert_eq!(wastes.fog_color, wastes.sky_color);
        assert!(wastes.fog_color.x > wastes.fog_color.z, "reddish");
        let valley = nether(6000, 0.0, Biome::SoulSandValley, 144.0, None);
        assert!(valley.fog_color.z > valley.fog_color.x, "bluish");
        assert!(environment(6000, 0.0, Biome::Plains, Vec3::NEG_Z, None).sky);
    }

    #[test]
    fn the_end_has_no_sky_a_dark_purple_fog_and_bright_light() {
        let end = end(6000, 0.0, 144.0, None);
        assert!(!end.sky);
        assert_eq!(end.sun_brightness, 0.0);
        assert!(end.fog_color.length() < 0.2);
        assert!(end.fog_color.z >= end.fog_color.y, "purplish");
        assert!(end.ambient > 0.4);
        assert_eq!(end.fog_end, 144.0);
    }

    #[test]
    fn looking_at_the_sunset_tints_the_fog() {
        // About 12700 is sunset: the sun at the western horizon.
        let west = environment(12700, 0.0, Biome::Plains, Vec3::NEG_X, None);
        let east = environment(12700, 0.0, Biome::Plains, Vec3::X, None);
        assert!(west.sunrise.is_some());
        assert!(
            west.fog_color.x > east.fog_color.x + 0.1,
            "{} vs {}",
            west.fog_color,
            east.fog_color
        );
    }
}
