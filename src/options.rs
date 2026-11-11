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
    /// The look (Vanilla or Trailer), the Trailer quality and effects, and
    /// each look's resource packs (v1.4). Options from before v1.4 have
    /// none, so they get the Trailer look on first run.
    pub visuals: crate::render::visuals::Visuals,
    /// The extras (the video's mods) the player switched, per style.
    pub extras: crate::extras::Toggles,
    /// Distant Terrain (M69): LOD distance, quality and threads.
    pub distant: crate::lod::DistantTerrain,
}

/// The browser starts in Vanilla style with Shaders off: the packs aren't
/// there until the player adds them, and it is the configuration the web
/// gate measures (M71, M73).
fn default_visuals() -> crate::render::visuals::Visuals {
    let mut visuals = crate::render::visuals::Visuals::default();
    if crate::platform::WEB {
        visuals.look = crate::render::visuals::LookKind::Vanilla;
        visuals.style = crate::render::visuals::Style::Vanilla;
    }
    visuals
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
            visuals: default_visuals(),
            extras: Default::default(),
            distant: Default::default(),
        }
    }
}

impl Options {
    /// `options.ron` next to the saves folder. Tests use a file of their
    /// own, so they neither read nor change the player's settings.
    pub fn path() -> PathBuf {
        if cfg!(test) {
            return crate::platform::env::temp_dir().join(format!(
                "rustcraft-test-options-{}.ron",
                crate::platform::env::process_id()
            ));
        }
        level::saves_dir().with_file_name("options.ron")
    }

    /// Reads the options, falling back to the defaults if the file is
    /// missing or can't be read.
    pub fn load(path: &Path) -> Self {
        let text = match crate::platform::fs::read_to_string(path) {
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
            crate::platform::fs::create_dir_all(dir)
                .with_context(|| format!("creating {}", dir.display()))?;
        }
        crate::platform::fs::write(path, text)
            .with_context(|| format!("writing {}", path.display()))
    }

    /// Moves options from before v1.5 to styles the first time, and makes
    /// `resource_packs` the extra packs of the style that's on.
    ///
    /// v1.4's Vanilla look becomes the Vanilla style with Shaders off; its
    /// Trailer look becomes Barebones with Shaders on at the quality it had.
    /// Vanilla's pack list goes to Vanilla and the Trailer list to
    /// Barebones, minus the three packs Barebones now brings itself. Options
    /// from before v1.4 (the packs chosen, no looks) get Barebones with
    /// Shaders on, those packs as its extras.
    pub fn set_up_packs(&mut self, _available: &[(String, String)]) {
        use crate::render::visuals::{LookKind, Style, packs::identify};
        let v = &mut self.visuals;
        if !v.style_set_up {
            v.style_set_up = true;
            let own = |list: &[String]| -> Vec<String> {
                list.iter()
                    .filter(|name| identify(name, "").is_none())
                    .cloned()
                    .collect()
            };
            if v.packs_set_up {
                v.style = if v.look == LookKind::Vanilla {
                    Style::Vanilla
                } else {
                    Style::Barebones
                };
                v.style_packs
                    .insert(Style::Vanilla, std::mem::take(&mut v.vanilla_packs));
                let trailer = std::mem::take(&mut v.trailer_packs);
                v.style_packs.insert(Style::Barebones, own(&trailer));
            } else {
                v.style = Style::Barebones;
                v.style_packs
                    .insert(Style::Barebones, own(&self.resource_packs));
            }
            v.offered.clear();
            v.packs_set_up = true;
        } else {
            // The list being edited is the one of the style that's on.
            *v.packs_mut(v.style) = self.resource_packs.clone();
        }
        self.resource_packs = v.packs(v.style).clone();
    }

    /// Keeps the list being edited (`resource_packs`) as the style's own.
    pub fn store_packs(&mut self) {
        let style = self.visuals.style;
        *self.visuals.packs_mut(style) = self.resource_packs.clone();
    }

    /// Shaders on or off. The style, its packs and the quality stay.
    pub fn set_look(&mut self, look: crate::render::visuals::LookKind) {
        self.visuals.look = look;
    }

    /// Switches to `style`, keeping each style's own extra packs: the list
    /// being edited is stored for the old style and the new one's comes in.
    pub fn set_style(&mut self, style: crate::render::visuals::Style) {
        let v = &mut self.visuals;
        *v.packs_mut(v.style) = std::mem::take(&mut self.resource_packs);
        v.style = style;
        self.resource_packs = v.packs(style).clone();
    }

    /// The packs the look is built from, highest priority first: the
    /// player's extras for this style, then the style's own stack from the
    /// built-in packs in `builtin`.
    pub fn active_packs(&self, builtin: &std::path::Path) -> Vec<String> {
        let mut packs = self.resource_packs.clone();
        for own in crate::pack::style_stack(self.visuals.style, builtin) {
            if !packs.contains(&own) {
                packs.push(own);
            }
        }
        packs
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
    fn graphics_round_trip_and_a_new_file_gets_barebones_with_shaders_on_at_medium() {
        use crate::render::visuals::{Effect, Quality, Style};
        let dir = TempDir::new("options-graphics");
        let path = dir.path().join("options.ron");
        // A file from before v1.4: packs chosen, no looks.
        crate::platform::fs::write(
            &path,
            r#"(render_distance: 9, resource_packs: ["Ashen_1.8.3.zip", "Bare Bones 1.21.zip"])"#,
        )
        .unwrap();
        let mut options = Options::load(&path);
        options.set_up_packs(&[]);
        assert_eq!(options.visuals.style, Style::Barebones);
        assert!(options.visuals.shaders());
        assert_eq!(options.visuals.quality, Quality::Medium);
        // The packs it chose are the style's extras, minus Bare Bones,
        // which Barebones brings itself.
        assert_eq!(options.resource_packs, ["Ashen_1.8.3.zip"]);
        // Each style keeps its own extras.
        options.set_style(Style::Realistic);
        assert!(options.resource_packs.is_empty());
        options.resource_packs.push("Mine.zip".to_owned());
        options.visuals.quality = Quality::High;
        options.visuals.set(Effect::Bloom, false);
        options.save(&path).unwrap();
        let mut back = Options::load(&path);
        assert_eq!(back, options);
        back.set_up_packs(&[]);
        assert_eq!(back.resource_packs, ["Mine.zip"]);
        back.set_style(Style::Barebones);
        assert_eq!(
            back.resource_packs,
            ["Ashen_1.8.3.zip"],
            "Barebones' list kept"
        );
        assert!(!back.visuals.enabled(Effect::Bloom));
        assert_eq!(back.visuals.quality, Quality::High);
    }

    /// A v1.4 file: its Vanilla look becomes the Vanilla style with shaders
    /// off and its list; its Trailer look becomes Barebones with shaders on
    /// at the quality it had, its extra packs kept and the three Trailer
    /// packs dropped from the list (the style brings them).
    #[test]
    fn a_v1_4_file_migrates_both_looks() {
        use crate::render::visuals::{Quality, Style};
        let v14 = |look: &str| {
            format!(
                r#"(resource_packs: ["Ashen_1.8.3.zip"], visuals: (look: {look}, quality: High,
                   vanilla_packs: ["Ashen_1.8.3.zip"],
                   trailer_packs: ["FreshAnimations_v1.9.4.zip", "Bare Bones 1.21.zip", "Mine.zip"],
                   offered: ["freshanimations"], packs_set_up: true))"#
            )
        };
        let dir = TempDir::new("options-v14");
        let path = dir.path().join("options.ron");
        crate::platform::fs::write(&path, v14("Vanilla")).unwrap();
        let mut vanilla = Options::load(&path);
        vanilla.set_up_packs(&[]);
        assert_eq!(vanilla.visuals.style, Style::Vanilla);
        assert!(!vanilla.visuals.shaders());
        assert_eq!(vanilla.resource_packs, ["Ashen_1.8.3.zip"]);
        assert_eq!(vanilla.visuals.packs(Style::Barebones), &["Mine.zip"]);
        crate::platform::fs::write(&path, v14("Trailer")).unwrap();
        let mut trailer = Options::load(&path);
        trailer.set_up_packs(&[]);
        assert_eq!(trailer.visuals.style, Style::Barebones);
        assert!(trailer.visuals.shaders());
        assert_eq!(trailer.visuals.quality, Quality::High, "its old quality");
        assert_eq!(trailer.resource_packs, ["Mine.zip"]);
        assert_eq!(trailer.visuals.packs(Style::Vanilla), &["Ashen_1.8.3.zip"]);
        // A second time changes nothing.
        let before = trailer.clone();
        trailer.set_up_packs(&[]);
        assert_eq!(trailer, before);
    }

    /// Each style's stack (the built-in packs, top first) with all, some
    /// and none of its packs there, named like the real downloads.
    #[test]
    fn each_styles_stack_with_all_some_and_none_of_its_packs() {
        use crate::render::visuals::Style;
        let dir = TempDir::new("style-stacks");
        let all = [
            "Bare Bones 1.21.11.zip",
            "Bare Bones x Fresh Animations 1.10.4.zip",
            "Barebones+Leaves+Add-on+main.zip",
            "FreshAnimations_v1.11.2_26.1+.zip",
            "Patrix_26.3_32x_basic.zip",
        ];
        for name in all {
            crate::platform::fs::write(dir.path().join(name), "x").unwrap();
        }
        let stack = |s| crate::pack::style_stack(s, dir.path());
        assert!(
            stack(Style::Vanilla).is_empty(),
            "Vanilla has nothing over vanilla"
        );
        assert_eq!(
            stack(Style::Barebones),
            [
                "Bare Bones x Fresh Animations 1.10.4.zip",
                "FreshAnimations_v1.11.2_26.1+.zip",
                "Barebones+Leaves+Add-on+main.zip",
                "Bare Bones 1.21.11.zip"
            ]
        );
        assert_eq!(stack(Style::Realistic), ["Patrix_26.3_32x_basic.zip"]);
        // Some missing: the style still works, and says what is missing.
        crate::platform::fs::remove_file(dir.path().join("FreshAnimations_v1.11.2_26.1+.zip"))
            .unwrap();
        crate::platform::fs::remove_file(dir.path().join("Patrix_26.3_32x_basic.zip")).unwrap();
        assert_eq!(stack(Style::Barebones).len(), 3);
        assert!(stack(Style::Realistic).is_empty());
        let missing = crate::pack::missing_for(Style::Realistic, dir.path());
        assert_eq!(missing[0].0, "Patrix 32x");
        assert!(missing[0].1.contains("patrix-32x"));
        assert_eq!(
            crate::pack::missing_for(Style::Barebones, dir.path())[0].0,
            "Fresh Animations"
        );
        // None at all.
        let none = TempDir::new("style-stacks-none");
        for style in [Style::Vanilla, Style::Barebones, Style::Realistic] {
            assert!(crate::pack::style_stack(style, none.path()).is_empty());
        }
        assert_eq!(
            crate::pack::missing_for(Style::Barebones, none.path()).len(),
            4
        );
        // The look is built from the player's extras, then the stack.
        let options = Options {
            resource_packs: vec!["Mine.zip".to_owned()],
            ..Options::default()
        };
        assert_eq!(
            options.active_packs(dir.path()),
            [
                "Mine.zip",
                "Bare Bones x Fresh Animations 1.10.4.zip",
                "Barebones+Leaves+Add-on+main.zip",
                "Bare Bones 1.21.11.zip"
            ]
        );
    }

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
        crate::platform::fs::write(&path, "(fov: 500.0)").unwrap();
        let loaded = Options::load(&path);
        assert_eq!(loaded.fov, 110.0);
        assert_eq!(loaded.biome_blend, config::BIOME_BLEND);
        crate::platform::fs::write(&path, "(biome_blend: 20)").unwrap();
        assert_eq!(Options::load(&path).biome_blend, 7);
        assert_eq!(loaded.render_distance, config::RENDER_DISTANCE);
        crate::platform::fs::write(
            &path,
            r#"(player_name: "no spaces", gui_scale: 40, mystery: 3)"#,
        )
        .unwrap();
        let loaded = Options::load(&path);
        assert_eq!(loaded.player_name, DEFAULT_NAME);
        assert_eq!(loaded.gui_scale, MAX_GUI_SCALE);
        crate::platform::fs::write(&path, "not ron").unwrap();
        assert_eq!(Options::load(&path), Options::default());
    }
}
