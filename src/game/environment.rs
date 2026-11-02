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
/// The Brightness option's default (vanilla's 50%); the game sets the
/// player's own each frame.
pub const DEFAULT_BRIGHTNESS: f32 = 0.5;
/// Exponential fog in lava, and its color (1.0.0).
const LAVA_FOG: (f32, Vec3) = (2.0, Vec3::new(0.6, 0.1, 0.0));
/// Modern underwater fog: linear from 8 blocks behind the eyes to 96
/// ahead, times the water vision (at least a quarter).
const WATER_FOG_START: f32 = -8.0;
const WATER_FOG_END: f32 = 96.0;
/// Ticks under water until the eyes are used to it (modern
/// `LocalPlayer.waterVisionTime`), and how fast that fades above water.
pub const WATER_VISION_TICKS: u32 = 600;
pub const WATER_VISION_FADE: u32 = 10;

/// How used to the water the eyes are, 0–1 (modern `getWaterVision`).
pub fn water_vision(ticks: u32) -> f32 {
    if ticks >= WATER_VISION_TICKS {
        return 1.0;
    }
    let early = (ticks as f32 / 100.0).clamp(0.0, 1.0);
    let late = if ticks < 100 {
        0.0
    } else {
        ((ticks - 100) as f32 / 500.0).clamp(0.0, 1.0)
    };
    early * 0.6 + late * 0.4
}

/// How far underwater fog reaches in `biome` once the eyes have been
/// `ticks` under water (modern `FogRenderer`, `FogType.WATER`): 96 blocks
/// times the water vision (at least a quarter), 15% less in swamps. Only
/// the render distance cuts it short, never light or time of day.
pub fn water_fog_end(biome: Biome, ticks: u32) -> f32 {
    let swamp = matches!(biome, Biome::Swamp | Biome::MangroveSwamp);
    WATER_FOG_END * water_vision(ticks).max(0.25) * if swamp { 0.85 } else { 1.0 }
}

/// How bright the light at the eyes is, 0–1: the brighter of the sky light
/// (dimmed by the sun) and the block light, or a mod's brightness override
/// (Fullbright) if brighter, as a share of full light.
pub fn eye_brightness(sky: u8, block: u8, sun: f32, min_light: f32) -> f32 {
    (f32::from(sky) * sun)
        .max(f32::from(block))
        .max(min_light)
        .clamp(0.0, 15.0)
        / 15.0
}

/// The color of fog under water in `biome`: the biome's water fog color
/// scaled by the light at the eyes (`brightness`), and lifted toward its
/// full strength by `lift` (0–1): Night Vision's scale or the water vision
/// as the eyes adjust, whichever is more, as vanilla does. Time of day and
/// depth reach the fog only through this color, never its distance.
pub fn water_fog_color(biome: Biome, brightness: f32, lift: f32) -> Vec3 {
    let c = biome.water_fog_color();
    let color = Vec3::new(
        (c >> 16 & 0xFF) as f32,
        (c >> 8 & 0xFF) as f32,
        (c & 0xFF) as f32,
    ) / 255.0;
    let boost = 1.0 / color.max_element().max(1.0 / 255.0);
    let color = color * brightness.clamp(0.0, 1.0);
    color * (1.0 - lift) + color * boost * lift
}

/// How opaque the full-screen underwater overlay (`misc/underwater.png`)
/// is: vanilla's 0.1 times the light at the eyes (0–1).
pub fn underwater_overlay_alpha(brightness: f32) -> f32 {
    0.1 * brightness.clamp(0.0, 1.0)
}

impl Game {
    /// The liquid the camera is in, if any (what the client draws).
    pub fn eye_in(&self) -> Option<Fluid> {
        self.fluid_at(self.player.camera.position.as_dvec3())
    }

    /// The overlay over the screen under water: its opacity (0.1 times the
    /// light at the eyes) and where the texture sits, sliding with where the
    /// head turns; `None` with the eyes out of water.
    pub fn underwater_overlay(&self, min_light: f32) -> Option<(f32, [f32; 2])> {
        if self.eye_in() != Some(Fluid::Water) {
            return None;
        }
        let eye = self.player.camera.position.floor().as_ivec3();
        let (sky, block) = self.chunks.light_at(eye.x, eye.y, eye.z).unwrap_or((15, 0));
        let sun = self.environment_with(min_light).sun_brightness;
        let alpha = underwater_overlay_alpha(eye_brightness(sky, block, sun, min_light));
        let camera = &self.player.camera;
        let offset = [
            -camera.yaw.to_degrees() / 64.0,
            camera.pitch.to_degrees() / 64.0,
        ];
        Some((alpha, offset))
    }

    /// The liquid the player's eyes are in, from where their body is: the
    /// server has no camera for a player (it stays where they spawned), so
    /// anything it decides must use this.
    pub fn eyes_in(&self) -> Option<Fluid> {
        let body = &self.player.body;
        self.fluid_at(body.position() + glam::DVec3::Y * body.eye_height())
    }

    /// The sound event music is taken from where the player is: the credits'
    /// poem, the End (with its own track for the dragon fight), the Nether's
    /// biomes, creative mode, under water, else the game's tracks.
    pub fn music_event(&self) -> Option<&'static str> {
        if self.player.credits.is_some() {
            return Some("music.credits");
        }
        Some(match self.dimension {
            Dimension::End if self.dragon.is_some() => "music.dragon",
            Dimension::End => "music.end",
            Dimension::Nether => {
                let p = self.player.camera.position.floor().as_ivec3();
                match self.chunks.world().biome_at(p.x, p.z).unwrap_or_default() {
                    Biome::CrimsonForest => "music.nether.crimson_forest",
                    Biome::SoulSandValley => "music.nether.soul_sand_valley",
                    Biome::BasaltDeltas => "music.nether.basalt_deltas",
                    // The warped forest has no tracks of its own.
                    _ => "music.nether.nether_wastes",
                }
            }
            Dimension::Overworld if self.eye_in() == Some(Fluid::Water) => "music.under_water",
            Dimension::Overworld if self.player.mode == crate::player::mode::GameMode::Creative => {
                "music.creative"
            }
            Dimension::Overworld => "music.game",
        })
    }

    pub fn environment(&self) -> Environment {
        self.environment_with(0.0)
    }

    /// The environment to draw, with a mod's brightness override of
    /// `min_light` (Fullbright) lighting the fog as well as the blocks.
    pub fn environment_with(&self, min_light: f32) -> Environment {
        let p = self.player.camera.position.floor().as_ivec3();
        let biome = self.chunks.world().biome_at(p.x, p.z).unwrap_or_default();
        let view = (self.chunks.radius() * CHUNK_SIZE as i32) as f32;
        let mut environment = match self.dimension {
            Dimension::Nether => nether(
                self.day_time(),
                self.tick_alpha() as f32,
                biome,
                view,
                self.eye_in(),
            ),
            Dimension::End => end(
                self.day_time(),
                self.tick_alpha() as f32,
                view,
                self.eye_in(),
            ),
            Dimension::Overworld => self.overworld_environment(biome, view),
        };
        // Night vision lights everything, flickering in its last 10 s
        // (modern `GameRenderer.getNightVisionScale`).
        let night_vision = self
            .player
            .stats
            .effects
            .get(crate::player::effects::Effect::NightVision)
            .map_or(0.0, |active| {
                let d = active.duration as f32;
                if d > 200.0 {
                    1.0
                } else {
                    0.7 + 0.3 * (d * std::f32::consts::PI * 0.2).sin()
                }
            });
        environment.ambient = environment.ambient.max(night_vision);
        environment.min_light = min_light;
        if self.eye_in() == Some(Fluid::Water) {
            let eye = self.player.camera.position.floor().as_ivec3();
            let (sky, block) = self.chunks.light_at(eye.x, eye.y, eye.z).unwrap_or((15, 0));
            let brightness = eye_brightness(
                sky,
                block,
                environment.sun_brightness,
                environment.min_light,
            );
            let lift = night_vision.max(water_vision(self.player.water_vision));
            let color = water_fog_color(biome, brightness, lift);
            environment.fog_color = color;
            environment.sky_color = color;
            environment.fog_density = 0.0;
            environment.fog_start = WATER_FOG_START;
            environment.fog_end = water_fog_end(biome, self.player.water_vision).min(view);
            environment.sky = false;
        }
        environment
    }

    fn overworld_environment(&self, biome: Biome, view: f32) -> Environment {
        let mut environment = environment(
            self.day_time(),
            self.tick_alpha() as f32,
            biome,
            self.player.camera.forward(),
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
        // Water's fog is set by `Game::environment`.
        None | Some(Fluid::Water) => (0.0, fog_color),
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
        brightness: DEFAULT_BRIGHTNESS,
        min_light: 0.0,
        glint: [1.0, 1.0],
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
        // Water's fog is set by `Game::environment`.
        None | Some(Fluid::Water) => (0.0, fog),
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
        brightness: DEFAULT_BRIGHTNESS,
        min_light: 0.0,
        glint: [1.0, 1.0],
    }
}

/// The End's: no sun, moon, stars or clouds, a near-black purple sky and
/// fog (modern `EndEffects`, fog 0x181318), and bright light (modern's
/// `forceBrightLightmap`, here a high ambient light) since the End has no
/// sky light. `view` is the render distance in blocks.
pub fn end(time: u64, partial: f32, view: f32, eye: Option<Fluid>) -> Environment {
    let dark = Vec3::new(0x18 as f32, 0x13 as f32, 0x18 as f32) / 255.0;
    let (fog_density, fog_color) = match eye {
        None | Some(Fluid::Water) => (0.0, dark),
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
        brightness: DEFAULT_BRIGHTNESS,
        min_light: 0.0,
        glint: [1.0, 1.0],
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
    fn under_water_the_fog_reaches_24_blocks_at_first_then_58_then_96_and_swamps_15_percent_less() {
        assert_eq!(water_vision(0), 0.0);
        assert!((water_vision(100) - 0.6).abs() < 1e-6);
        assert_eq!(water_vision(600), 1.0);
        assert_eq!(water_fog_end(Biome::Ocean, 0), 24.0);
        assert!((water_fog_end(Biome::Ocean, 100) - 57.6).abs() < 1e-4);
        assert_eq!(water_fog_end(Biome::Ocean, 600), 96.0);
        assert_eq!(water_fog_end(Biome::Ocean, 5000), 96.0);
        assert!((water_fog_end(Biome::Swamp, 600) - 81.6).abs() < 1e-4);
        assert!((water_fog_end(Biome::MangroveSwamp, 600) - 81.6).abs() < 1e-4);
        assert!((water_fog_end(Biome::Swamp, 0) - 20.4).abs() < 1e-4);
        // The color is the biome's own, scaled only by the light at the eyes.
        let full = water_fog_color(Biome::Ocean, 1.0, 0.0);
        assert!((full - Vec3::new(5.0, 5.0, 51.0) / 255.0).length() < 1e-6);
        let dim = water_fog_color(Biome::Ocean, 0.25, 0.0);
        assert!((dim - full * 0.25).length() < 1e-6);
        assert_eq!(water_fog_color(Biome::Ocean, 0.0, 0.0), Vec3::ZERO);
        // Night Vision lifts it toward full strength.
        let lit = water_fog_color(Biome::Ocean, 1.0, 1.0);
        assert!((lit.z - 1.0).abs() < 1e-6 && lit.x < 0.11);
        let (swamp, _) = (water_fog_color(Biome::Swamp, 1.0, 0.0), ());
        assert!(swamp.x > swamp.z, "swamps are murky green-brown");
        let lava = environment(6000, 0.0, Biome::Plains, Vec3::NEG_Z, Some(Fluid::Lava));
        assert_eq!(lava.fog_density, 2.0);
    }

    #[test]
    fn the_eye_brightness_follows_the_light_level_and_fullbright_lights_it_fully() {
        assert_eq!(eye_brightness(15, 0, 1.0, 0.0), 1.0);
        assert_eq!(eye_brightness(0, 0, 1.0, 0.0), 0.0);
        assert!((eye_brightness(10, 0, 1.0, 0.0) - 10.0 / 15.0).abs() < 1e-6);
        assert!(eye_brightness(15, 0, 0.2, 0.0) < eye_brightness(15, 0, 1.0, 0.0));
        assert_eq!(eye_brightness(0, 0, 1.0, 15.0), 1.0, "Fullbright");
        assert_eq!(
            eye_brightness(0, 12, 1.0, 0.0),
            eye_brightness(12, 0, 1.0, 0.0)
        );
    }

    #[test]
    fn the_underwater_overlay_is_at_most_a_tenth_opaque_and_scales_with_the_light() {
        for brightness in [0.0, 0.25, 0.5, 1.0, 3.0] {
            assert!(underwater_overlay_alpha(brightness) <= 0.1 + 1e-6);
        }
        assert_eq!(underwater_overlay_alpha(1.0), 0.1);
        assert_eq!(underwater_overlay_alpha(0.0), 0.0);
        assert!((underwater_overlay_alpha(0.5) - 0.05).abs() < 1e-6);
    }

    #[test]
    fn time_of_day_and_depth_change_the_fog_color_but_never_how_far_it_reaches() {
        use crate::game::tests::flat_game;
        use crate::player::mode::GameMode;
        let mut game = flat_game(GameMode::Creative);
        let water: Vec<_> = (-3..=3)
            .flat_map(|x| {
                (-3..=3).flat_map(move |z| {
                    (50..=70).map(move |y| {
                        (
                            glam::IVec3::new(x, y, z),
                            crate::world::block::BlockState::of(crate::world::blocks::WATER),
                        )
                    })
                })
            })
            .collect();
        game.chunks.set_blocks(&water);
        game.chunks
            .load_now(crate::world::chunk::ChunkPos::new(0, 0));
        game.player.water_vision = 300;
        game.chunks.set_radius(9);
        let reach = |game: &mut Game, time: u64, y: f64| {
            game.time = time;
            game.teleport(glam::DVec3::new(0.5, y, 0.5), 0.0, 0.0);
            let env = game.environment_with(0.0);
            (env.fog_end, env.fog_color)
        };
        let noon = reach(&mut game, 6000, 60.0);
        let midnight = reach(&mut game, 18000, 60.0);
        let deep = reach(&mut game, 6000, 52.0);
        assert!(noon.0 > 24.0 && noon.0 <= 96.0);
        assert_eq!(noon.0, midnight.0, "midnight reaches as far as noon");
        assert_eq!(noon.0, deep.0, "depth doesn't change how far");
        assert!(
            midnight.1.length() <= noon.1.length(),
            "the color is what darkens"
        );
        let fullbright = game.environment_with(15.0);
        assert_eq!(fullbright.fog_end, noon.0);
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
