//! The player's settings (vanilla's `Options`), kept in `options.ron`
//! beside the saves folder: everything on the Options screens, the key
//! bindings and the selected resource packs. Settings a file doesn't have
//! keep their defaults and ones the game doesn't know are skipped, so
//! older and newer files load.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use log::warn;
use serde::{Deserialize, Serialize};

use crate::{bindings::Bindings, config, level};

/// Render distance range in chunks: 1.0.0's Tiny (2) to Far (16).
pub const RENDER_DISTANCES: std::ops::RangeInclusive<i32> = 2..=16;
/// Biome blend radius range: off to modern's 15 × 15.
pub const BIOME_BLENDS: std::ops::RangeInclusive<i32> = 0..=7;
/// Field of view range in degrees: 1.0.0's slider ran from 30 to "Quake
/// Pro" at 110, with 70 as "Normal".
pub const FOVS: std::ops::RangeInclusive<f32> = 30.0..=110.0;
/// Mouse sensitivity range, as a fraction of the default (1.0.0's 0–200%).
pub const SENSITIVITIES: std::ops::RangeInclusive<f32> = 0.0..=2.0;
/// Max Framerate's range in frames a second; the top is "Unlimited".
pub const FRAMERATES: std::ops::RangeInclusive<u32> = 10..=260;
pub const UNLIMITED_FRAMERATE: u32 = 260;
/// The largest GUI Scale offered (0 is Auto).
pub const MAX_GUI_SCALE: u32 = 6;
/// Vanilla's player names: 3 to 16 letters, digits and underscores.
pub const NAME_LENGTHS: std::ops::RangeInclusive<usize> = 3..=16;
pub const DEFAULT_NAME: &str = "Player";

/// Fast draws leaves solid; Fancy sees through them.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Graphics {
    Fast,
    #[default]
    Fancy,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Clouds {
    Off,
    Fast,
    #[default]
    Fancy,
}

/// How many particles are made: all, about a third fewer, or only the
/// ones that matter.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Particles {
    #[default]
    All,
    Decreased,
    Minimal,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum MainHand {
    Left,
    #[default]
    Right,
}

/// Music & Sounds' sliders below Master Volume, 0–1 each (vanilla's
/// `SoundSource`s).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Volumes {
    pub music: f32,
    pub records: f32,
    pub weather: f32,
    pub blocks: f32,
    pub hostile: f32,
    pub neutral: f32,
    pub players: f32,
    pub ambient: f32,
    pub voice: f32,
}

impl Default for Volumes {
    fn default() -> Self {
        Self {
            music: 1.0,
            records: 1.0,
            weather: 1.0,
            blocks: 1.0,
            hostile: 1.0,
            neutral: 1.0,
            players: 1.0,
            ambient: 1.0,
            voice: 1.0,
        }
    }
}

impl Volumes {
    fn clamped(self) -> Self {
        let c = |v: f32| v.clamp(0.0, 1.0);
        Self {
            music: c(self.music),
            records: c(self.records),
            weather: c(self.weather),
            blocks: c(self.blocks),
            hostile: c(self.hostile),
            neutral: c(self.neutral),
            players: c(self.players),
            ambient: c(self.ambient),
            voice: c(self.voice),
        }
    }
}

/// Skin Customization: which of the skin's outer layers are worn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SkinParts {
    pub jacket: bool,
    pub left_sleeve: bool,
    pub right_sleeve: bool,
    pub left_pants: bool,
    pub right_pants: bool,
    pub hat: bool,
}

impl Default for SkinParts {
    fn default() -> Self {
        Self {
            jacket: true,
            left_sleeve: true,
            right_sleeve: true,
            left_pants: true,
            right_pants: true,
            hat: true,
        }
    }
}

/// Whether `name` is a name vanilla would take.
pub fn valid_name(name: &str) -> bool {
    NAME_LENGTHS.contains(&name.chars().count())
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Options {
    /// Chunks drawn in each direction.
    pub render_distance: i32,
    /// Columns each way that grass and foliage colors average over (0 is
    /// off).
    pub biome_blend: i32,
    /// Vertical field of view in degrees.
    pub fov: f32,
    /// Mouse look speed, 1 for the default.
    pub sensitivity: f32,
    /// Sound volume 0–1: the master volume of sounds and music.
    pub volume: f32,
    pub vsync: bool,
    /// Whether the view and hand shake with each step.
    pub view_bobbing: bool,
    /// The Brightness option, 0 (Moody) to 1 (Bright); vanilla's 50% by
    /// default.
    pub brightness: f32,
    /// Selected resource packs (file or folder names in `resourcepacks/`),
    /// highest priority first.
    pub resource_packs: Vec<String>,
    /// Every action's key or mouse button.
    pub keys: Bindings,
    /// The name other players see on LAN (M37).
    pub player_name: String,
    /// Chat Settings (M38).
    pub chat: crate::ui::chat::ChatSettings,
    /// Walking into a one-block step jumps it (vanilla's Auto-Jump, off by
    /// default).
    pub auto_jump: bool,
    /// Sneak and Sprint switch on and off with a press instead of being
    /// held.
    pub toggle_sneak: bool,
    pub toggle_sprint: bool,
    pub invert_y: bool,
    pub volumes: Volumes,
    pub graphics: Graphics,
    /// Light and ambient occlusion blend across faces; off, each face has
    /// the light of the block in front of it.
    pub smooth_lighting: bool,
    pub max_framerate: u32,
    /// GUI pixels' size in points; 0 picks the largest that fits (Auto).
    pub gui_scale: u32,
    pub fullscreen: bool,
    pub clouds: Clouds,
    pub particles: Particles,
    pub entity_shadows: bool,
    pub skin: SkinParts,
    pub main_hand: MainHand,
    /// How much sprinting, flying and potions widen or narrow the view,
    /// 0–1.
    pub fov_effects: f32,
    /// How much the view tilts when hurt, 0–1.
    pub damage_tilt: f32,
    /// The enchantment glint's speed and brightness, 0–1 (vanilla's
    /// defaults are 0.5 and 0.75).
    pub glint_speed: f32,
    pub glint_strength: f32,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            render_distance: config::RENDER_DISTANCE,
            biome_blend: config::BIOME_BLEND,
            fov: config::FOV_Y_DEGREES,
            sensitivity: 1.0,
            volume: 1.0,
            vsync: true,
            view_bobbing: true,
            brightness: crate::game::environment::DEFAULT_BRIGHTNESS,
            resource_packs: Vec::new(),
            keys: Bindings::default(),
            player_name: DEFAULT_NAME.to_owned(),
            chat: Default::default(),
            auto_jump: false,
            toggle_sneak: false,
            toggle_sprint: false,
            invert_y: false,
            volumes: Volumes::default(),
            graphics: Graphics::default(),
            smooth_lighting: true,
            max_framerate: 120,
            gui_scale: 0,
            fullscreen: false,
            clouds: Clouds::default(),
            particles: Particles::default(),
            entity_shadows: true,
            skin: SkinParts::default(),
            main_hand: MainHand::default(),
            fov_effects: 1.0,
            damage_tilt: 1.0,
            glint_speed: 0.5,
            glint_strength: 0.75,
        }
    }
}

impl Options {
    /// `options.ron` next to the saves folder. Tests use a file of their
    /// own, so they neither read nor change the player's settings.
    pub fn path() -> PathBuf {
        if cfg!(test) {
            return std::env::temp_dir()
                .join(format!("rustcraft-test-options-{}.ron", std::process::id()));
        }
        level::saves_dir().with_file_name("options.ron")
    }

    /// Reads the options, falling back to the defaults if the file is
    /// missing or can't be read.
    pub fn load(path: &Path) -> Self {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Self::default(),
            Err(err) => {
                warn!("reading {}: {err}; using default options", path.display());
                return Self::default();
            }
        };
        match ron::from_str::<Self>(&text) {
            Ok(options) => options.clamped(),
            Err(err) => {
                warn!("parsing {}: {err}; using default options", path.display());
                Self::default()
            }
        }
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let text = ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default())?;
        if let Some(dir) = path.parent().filter(|dir| !dir.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        }
        std::fs::write(path, text).with_context(|| format!("writing {}", path.display()))
    }

    /// Every value inside its slider's range.
    pub fn clamped(self) -> Self {
        Self {
            render_distance: self
                .render_distance
                .clamp(*RENDER_DISTANCES.start(), *RENDER_DISTANCES.end()),
            biome_blend: self
                .biome_blend
                .clamp(*BIOME_BLENDS.start(), *BIOME_BLENDS.end()),
            fov: self.fov.clamp(*FOVS.start(), *FOVS.end()),
            sensitivity: self
                .sensitivity
                .clamp(*SENSITIVITIES.start(), *SENSITIVITIES.end()),
            volume: self.volume.clamp(0.0, 1.0),
            vsync: self.vsync,
            view_bobbing: self.view_bobbing,
            brightness: self.brightness.clamp(0.0, 1.0),
            resource_packs: self.resource_packs,
            keys: self.keys,
            chat: self.chat.clamped(),
            player_name: if valid_name(&self.player_name) {
                self.player_name
            } else {
                DEFAULT_NAME.to_owned()
            },
            volumes: self.volumes.clamped(),
            max_framerate: self
                .max_framerate
                .clamp(*FRAMERATES.start(), *FRAMERATES.end()),
            gui_scale: self.gui_scale.min(MAX_GUI_SCALE),
            fov_effects: self.fov_effects.clamp(0.0, 1.0),
            damage_tilt: self.damage_tilt.clamp(0.0, 1.0),
            glint_speed: self.glint_speed.clamp(0.0, 1.0),
            glint_strength: self.glint_strength.clamp(0.0, 1.0),
            ..self
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::save::TempDir;

    #[test]
    fn names_are_vanillas() {
        assert!(valid_name("Kenzo") && valid_name("a_b") && valid_name("Sixteen_letters_"));
        assert!(!valid_name("ab") && !valid_name("Seventeen_letters") && !valid_name("Ke nzo"));
        assert!(!valid_name("Kénzo"));
    }

    #[test]
    fn options_round_trip_and_bad_files_fall_back_to_defaults() {
        let dir = TempDir::new("options");
        let path = dir.path().join("options.ron");
        assert_eq!(Options::load(&path), Options::default());
        let options = Options {
            render_distance: 12,
            biome_blend: 5,
            fov: 90.0,
            sensitivity: 1.5,
            volume: 0.25,
            vsync: false,
            view_bobbing: false,
            brightness: 0.8,
            resource_packs: vec!["High.zip".to_owned(), "Low".to_owned()],
            keys: {
                let mut keys = Bindings::default();
                keys.set(
                    crate::bindings::Action::Jump,
                    Some(crate::bindings::Input::Key(winit::keyboard::KeyCode::KeyJ)),
                );
                keys
            },
            player_name: "Kenzo".to_owned(),
            auto_jump: true,
            invert_y: true,
            volumes: Volumes {
                hostile: 0.3,
                ..Volumes::default()
            },
            graphics: Graphics::Fast,
            max_framerate: 60,
            gui_scale: 3,
            clouds: Clouds::Off,
            particles: Particles::Minimal,
            skin: SkinParts {
                hat: false,
                ..SkinParts::default()
            },
            main_hand: MainHand::Left,
            glint_strength: 0.2,
            ..Options::default()
        };
        options.save(&path).unwrap();
        assert_eq!(Options::load(&path), options);
        // Older files without a setting keep its default; values out of
        // range are pulled back in.
        std::fs::write(&path, "(fov: 500.0)").unwrap();
        let loaded = Options::load(&path);
        assert_eq!(loaded.fov, 110.0);
        assert_eq!(loaded.biome_blend, config::BIOME_BLEND);
        std::fs::write(&path, "(biome_blend: 20)").unwrap();
        assert_eq!(Options::load(&path).biome_blend, 7);
        assert_eq!(loaded.render_distance, config::RENDER_DISTANCE);
        std::fs::write(
            &path,
            r#"(player_name: "no spaces", gui_scale: 40, mystery: 3)"#,
        )
        .unwrap();
        let loaded = Options::load(&path);
        assert_eq!(loaded.player_name, DEFAULT_NAME);
        assert_eq!(loaded.gui_scale, MAX_GUI_SCALE);
        std::fs::write(&path, "not ron").unwrap();
        assert_eq!(Options::load(&path), Options::default());
    }
}
