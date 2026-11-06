//! The two looks (v1.4): **Vanilla**, drawn exactly as M53 drew it, and
//! **Trailer**, the trailer-style look built on top of it (HDR, post
//! passes, and the effects of M55–M57). The setting lives in the options
//! as `visuals`: which look is on, the Trailer quality profile, per-effect
//! overrides and each look's own resource pack list.
//!
//! (`src/look.rs` already means "everything texture-derived"; this is the
//! renderer's look.)

pub mod bloom;
pub mod compare;
pub mod frame;
pub mod fxaa;
pub mod packs;
pub mod profiler;
pub mod scale;
pub mod shadow;
pub mod shafts;
pub mod sun;
pub mod sway;
pub mod upscale;
pub mod water;

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Which look draws the frame.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum LookKind {
    /// M53's passes and shaders, untouched.
    Vanilla,
    /// The trailer look, the default from v1.4 on.
    #[default]
    Trailer,
}

impl LookKind {
    pub fn name(self) -> &'static str {
        match self {
            LookKind::Vanilla => "Vanilla",
            LookKind::Trailer => "Trailer",
        }
    }

    pub fn other(self) -> Self {
        match self {
            LookKind::Vanilla => LookKind::Trailer,
            LookKind::Trailer => LookKind::Vanilla,
        }
    }

    /// `--visuals vanilla|trailer`.
    pub fn parse(text: &str) -> Option<Self> {
        match text.to_ascii_lowercase().as_str() {
            "vanilla" => Some(LookKind::Vanilla),
            "trailer" => Some(LookKind::Trailer),
            _ => None,
        }
    }
}

/// The Trailer look's quality profile.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize,
)]
pub enum Quality {
    Low,
    #[default]
    Medium,
    High,
}

impl Quality {
    pub const ALL: [Quality; 3] = [Quality::Low, Quality::Medium, Quality::High];

    pub fn name(self) -> &'static str {
        match self {
            Quality::Low => "Low",
            Quality::Medium => "Medium",
            Quality::High => "High",
        }
    }

    /// The next one, wrapping (for the Video Settings button).
    pub fn next(self) -> Self {
        match self {
            Quality::Low => Quality::Medium,
            Quality::Medium => Quality::High,
            Quality::High => Quality::Low,
        }
    }

    /// `--quality low|medium|high`.
    pub fn parse(text: &str) -> Option<Self> {
        Quality::ALL
            .into_iter()
            .find(|q| q.name().eq_ignore_ascii_case(text))
    }

    /// The perf gate's row for this profile (`trailer-medium`).
    pub fn row(self) -> String {
        format!("trailer-{}", self.name().to_ascii_lowercase())
    }
}

/// One Trailer effect that can be turned on or off on its own (Trailer
/// Settings). Each is added by the milestone named in its doc.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Effect {
    /// M55: sun and moon shadows (cascaded shadow maps).
    Shadows,
    /// M55: shadow edges snapped to the 1/16-block texel grid.
    PixelLockedShadows,
    /// M55: mobs and players cast shadows.
    EntityShadows,
    /// M55: sunlight seen through leaves and grass.
    LeafLight,
    /// M56: screen-space reflections on water (and more on High).
    Reflections,
    /// M56: water shows what's under it, bent and tinted by depth.
    Refraction,
    /// M56: moving caustic light under water.
    Caustics,
    /// M56: shaded blocky clouds and the sun's glow.
    CloudShading,
    /// M56: clouds cast moving shadows on the ground.
    CloudShadows,
    /// M57: volumetric light shafts.
    LightShafts,
    /// M57: bright light glows.
    Bloom,
    /// M57: leaves and plants sway.
    SwayingPlants,
    /// M57: rain wets sky-lit surfaces and rings water.
    Wetness,
    /// M57: waterfall foam and splashes.
    Splashes,
    /// M57: thin height fog and dimension fog.
    HeightFog,
    /// M57: FXAA on edges.
    Fxaa,
}

impl Effect {
    pub const ALL: [Effect; 16] = [
        Effect::Shadows,
        Effect::PixelLockedShadows,
        Effect::EntityShadows,
        Effect::LeafLight,
        Effect::Reflections,
        Effect::Refraction,
        Effect::Caustics,
        Effect::CloudShading,
        Effect::CloudShadows,
        Effect::LightShafts,
        Effect::Bloom,
        Effect::SwayingPlants,
        Effect::Wetness,
        Effect::Splashes,
        Effect::HeightFog,
        Effect::Fxaa,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Effect::Shadows => "Shadows",
            Effect::PixelLockedShadows => "Pixel-Locked Shadows",
            Effect::EntityShadows => "Mob Shadows",
            Effect::LeafLight => "Light Through Leaves",
            Effect::Reflections => "Reflections",
            Effect::Refraction => "Refraction",
            Effect::Caustics => "Caustics",
            Effect::CloudShading => "Shaded Clouds",
            Effect::CloudShadows => "Cloud Shadows",
            Effect::LightShafts => "Light Shafts",
            Effect::Bloom => "Bloom",
            Effect::SwayingPlants => "Swaying Plants",
            Effect::Wetness => "Rain Wetness",
            Effect::Splashes => "Splashes",
            Effect::HeightFog => "Height Fog",
            Effect::Fxaa => "FXAA",
        }
    }

    /// Whether the profile has this effect on (before the player's
    /// override), from the milestone entries in docs/milestones/v1.4.md.
    pub fn default_for(self, quality: Quality) -> bool {
        use Quality::*;
        match self {
            Effect::EntityShadows => quality != Low,
            Effect::Reflections | Effect::Refraction => quality != Low,
            Effect::Caustics => quality == High,
            Effect::CloudShadows => quality != Low,
            Effect::LightShafts => quality != Low,
            Effect::Fxaa => quality != Low,
            _ => true,
        }
    }
}

/// The three built-in styles (v1.5): which textures, models and extras the
/// game wears. Independent of the Shaders switch (`LookKind`: Vanilla is
/// Shaders off, Trailer is Shaders on).
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize,
)]
pub enum Style {
    /// Complete vanilla: Mojang's own files only.
    Vanilla,
    /// The trailer look: Bare Bones, Barebones Better Leaves, Fresh
    /// Animations and Bare Bones x Fresh Animations over vanilla.
    #[default]
    Barebones,
    /// Patrix 32x over vanilla.
    Realistic,
}

impl Style {
    /// From fastest to slowest, as Video Settings lists them.
    pub const BY_SPEED: [Self; 3] = [Self::Barebones, Self::Vanilla, Self::Realistic];

    pub fn name(self) -> &'static str {
        match self {
            Self::Vanilla => "Vanilla",
            Self::Barebones => "Barebones",
            Self::Realistic => "Realistic",
        }
    }

    /// The name on the command line and in perf rows.
    pub fn key(self) -> &'static str {
        match self {
            Self::Vanilla => "vanilla",
            Self::Barebones => "barebones",
            Self::Realistic => "realistic",
        }
    }

    /// Shift+F12: Vanilla, then Barebones, then Realistic, then round.
    pub fn next(self) -> Self {
        match self {
            Self::Vanilla => Self::Barebones,
            Self::Barebones => Self::Realistic,
            Self::Realistic => Self::Vanilla,
        }
    }

    /// `--style vanilla|barebones|realistic`.
    pub fn parse(text: &str) -> Option<Self> {
        match text.to_ascii_lowercase().as_str() {
            "vanilla" => Some(Self::Vanilla),
            "barebones" => Some(Self::Barebones),
            "realistic" => Some(Self::Realistic),
            _ => None,
        }
    }
}

/// The perf gate's row for a style and shaders setting: `vanilla` with
/// shaders off, `barebones-medium` with them on.
pub fn row_name(style: Style, shaders: bool, quality: Quality) -> String {
    if shaders {
        format!("{}-{}", style.key(), quality.name().to_ascii_lowercase())
    } else {
        style.key().to_owned()
    }
}

/// "Realistic, Shaders on": what F12 and Shift+F12 show.
pub fn style_message(style: Style, shaders: bool) -> String {
    format!(
        "{}, Shaders {}",
        style.name(),
        if shaders { "on" } else { "off" }
    )
}

/// The `visuals` options.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Visuals {
    /// Shaders: `Trailer` is on, `Vanilla` off (v1.4's names).
    pub look: LookKind,
    pub style: Style,
    pub quality: Quality,
    /// Render Scale (M68): `scale::AUTO` or 50 to 100 percent.
    pub render_scale: u32,
    /// Effects the player turned on or off against the profile.
    pub effects: BTreeMap<Effect, bool>,
    /// v1.4's lists, one per look; moved to `style_packs` the first time
    /// v1.5 reads the options.
    pub vanilla_packs: Vec<String>,
    pub trailer_packs: Vec<String>,
    /// Each style's own extra resource packs on top of its stack, highest
    /// priority first. The options' `resource_packs` is always the list of
    /// the style that's on.
    pub style_packs: BTreeMap<Style, Vec<String>>,
    /// The options have been moved to styles (false in v1.4 files).
    pub style_set_up: bool,
    /// The frame rate last measured on this computer, by "style/off" or
    /// "style/on".
    pub measured: BTreeMap<String, f32>,
    /// The Measure button was pressed, and a measurement is running.
    #[serde(skip)]
    pub measure_requested: bool,
    #[serde(skip)]
    pub measuring: bool,
    /// The Trailer packs (`packs::TrailerPack` keys) already put on the
    /// Trailer list once, so one the player takes off stays off.
    pub offered: Vec<String>,
    /// The lists have been set up (false in options from before v1.4).
    pub packs_set_up: bool,
}

impl Visuals {
    /// Whether `effect` is on: the player's override, else the profile's.
    pub fn enabled(&self, effect: Effect) -> bool {
        self.effects
            .get(&effect)
            .copied()
            .unwrap_or_else(|| effect.default_for(self.quality))
    }

    /// Turns `effect` on or off; matching the profile clears the override.
    pub fn set(&mut self, effect: Effect, on: bool) {
        if on == effect.default_for(self.quality) {
            self.effects.remove(&effect);
        } else {
            self.effects.insert(effect, on);
        }
    }

    /// Whether Shaders are on.
    pub fn shaders(&self) -> bool {
        self.look == LookKind::Trailer
    }

    pub fn set_shaders(&mut self, on: bool) {
        self.look = if on {
            LookKind::Trailer
        } else {
            LookKind::Vanilla
        };
    }

    /// The extra pack list kept for `style`.
    pub fn packs(&self, style: Style) -> &Vec<String> {
        static NONE: Vec<String> = Vec::new();
        self.style_packs.get(&style).unwrap_or(&NONE)
    }

    pub fn packs_mut(&mut self, style: Style) -> &mut Vec<String> {
        self.style_packs.entry(style).or_default()
    }

    /// What the renderer needs this frame.
    pub fn frame(&self) -> FrameVisuals {
        // A measuring aid (M68): RUSTCRAFT_EFFECTS_OFF=bloom,fxaa turns the
        // named effects off for this run, to find what each one costs.
        let off = std::env::var("RUSTCRAFT_EFFECTS_OFF").unwrap_or_default();
        let off: Vec<String> = off
            .split(',')
            .map(|n| n.trim().to_lowercase().replace([' ', '-'], ""))
            .filter(|n| !n.is_empty())
            .collect();
        let forced_off = |e: Effect| {
            let name = e.name().to_lowercase().replace([' ', '-'], "");
            off.iter()
                .any(|n| *n == name || (n == "all" && e != Effect::Shadows))
        };
        // RUSTCRAFT_RENDER_SCALE=auto|50..100 sets the scale for a run.
        let render_scale = match std::env::var("RUSTCRAFT_RENDER_SCALE") {
            Ok(text) if text.eq_ignore_ascii_case("auto") => scale::AUTO,
            Ok(text) => text.parse().unwrap_or(self.render_scale),
            Err(_) => self.render_scale,
        };
        FrameVisuals {
            look: self.look,
            quality: self.quality,
            render_scale,
            effects: Effect::ALL.map(|e| self.enabled(e) && !forced_off(e)),
        }
    }
}

/// The passes a frame draws, in order, from its settings (the water's
/// pass only when something translucent is in view): the one place that
/// decides which effects get a pass, so turning an effect off removes it.
pub fn planned_passes(v: &FrameVisuals) -> Vec<&'static str> {
    if !v.trailer() {
        return vec!["world", "hand", "ui"];
    }
    let mut passes = Vec::new();
    if v.on(Effect::Shadows) {
        passes.push("shadow");
    }
    passes.push("world");
    if v.on(Effect::Reflections) || v.on(Effect::Refraction) {
        passes.push("water");
    }
    passes.push("hand");
    if v.on(Effect::Bloom) {
        passes.push("bloom");
    }
    if v.on(Effect::LightShafts) && shafts::settings(v.quality).is_some() {
        passes.push("shafts");
    }
    passes.push("post");
    if v.on(Effect::Fxaa) {
        passes.push("fxaa");
    }
    passes.push("ui");
    passes
}

/// The resolved settings the renderer draws a frame with.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FrameVisuals {
    pub look: LookKind,
    pub quality: Quality,
    /// Render Scale (M68): `scale::AUTO` or 50 to 100 percent.
    pub render_scale: u32,
    /// Indexed like `Effect::ALL`.
    effects: [bool; Effect::ALL.len()],
}

impl Default for FrameVisuals {
    fn default() -> Self {
        Visuals::default().frame()
    }
}

impl FrameVisuals {
    pub fn vanilla() -> Self {
        Visuals {
            look: LookKind::Vanilla,
            ..Visuals::default()
        }
        .frame()
    }

    pub fn trailer(&self) -> bool {
        self.look == LookKind::Trailer
    }

    /// Whether `effect` draws this frame (always false in Vanilla).
    pub fn on(&self, effect: Effect) -> bool {
        self.trailer() && self.effects[Effect::ALL.iter().position(|e| *e == effect).unwrap_or(0)]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profiles_and_overrides() {
        let mut v = Visuals::default();
        assert_eq!((v.look, v.quality), (LookKind::Trailer, Quality::Medium));
        assert!(v.enabled(Effect::Reflections) && !v.enabled(Effect::Caustics));
        v.set(Effect::Reflections, false);
        assert!(!v.enabled(Effect::Reflections));
        v.set(Effect::Reflections, true);
        assert!(
            v.effects.is_empty(),
            "matching the profile clears the override"
        );
        v.quality = Quality::Low;
        assert!(!v.enabled(Effect::LightShafts) && v.enabled(Effect::Shadows));
        assert!(!FrameVisuals::vanilla().on(Effect::Shadows));
        assert!(v.frame().on(Effect::Shadows));
    }

    /// Every effect has a value for each profile, from the milestone
    /// entries, and the Trailer Settings screen lists all of them.
    #[test]
    fn every_effect_has_a_low_medium_and_high_setting() {
        use Quality::*;
        let off = |q: Quality| -> Vec<Effect> {
            Effect::ALL
                .into_iter()
                .filter(|e| !e.default_for(q))
                .collect()
        };
        assert_eq!(
            off(Low),
            [
                Effect::EntityShadows,
                Effect::Reflections,
                Effect::Refraction,
                Effect::Caustics,
                Effect::CloudShadows,
                Effect::LightShafts,
                Effect::Fxaa,
            ]
        );
        assert_eq!(off(Medium), [Effect::Caustics]);
        assert!(off(High).is_empty());
        // The screen lists each by a name of its own.
        let mut names: Vec<&str> = Effect::ALL.iter().map(|e| e.name()).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), Effect::ALL.len());
        assert!(Effect::ALL.len() >= 16);
    }

    #[test]
    fn turning_an_effect_off_removes_its_pass() {
        let medium = FrameVisuals::default();
        let all = planned_passes(&medium);
        for pass in ["shadow", "water", "bloom", "shafts", "fxaa"] {
            assert!(all.contains(&pass), "{pass} on Medium: {all:?}");
        }
        let without = |effects: &[Effect]| {
            let mut v = Visuals::default();
            for e in effects {
                v.set(*e, false);
            }
            planned_passes(&v.frame())
        };
        assert!(!without(&[Effect::Shadows]).contains(&"shadow"));
        assert!(!without(&[Effect::Bloom]).contains(&"bloom"));
        assert!(!without(&[Effect::LightShafts]).contains(&"shafts"));
        assert!(!without(&[Effect::Fxaa]).contains(&"fxaa"));
        // The water's pass needs reflections or refraction: one is enough.
        assert!(without(&[Effect::Reflections]).contains(&"water"));
        assert!(!without(&[Effect::Reflections, Effect::Refraction]).contains(&"water"));
        // Vanilla draws M53's three passes only.
        assert_eq!(
            planned_passes(&FrameVisuals::vanilla()),
            ["world", "hand", "ui"]
        );
        // Low: no water pass, no shafts, no FXAA, but a smaller bloom.
        let low = Visuals {
            quality: Quality::Low,
            ..Visuals::default()
        };
        assert_eq!(
            planned_passes(&low.frame()),
            ["shadow", "world", "hand", "bloom", "post", "ui"]
        );
    }

    #[test]
    fn names_parse_back() {
        assert_eq!(LookKind::parse("Vanilla"), Some(LookKind::Vanilla));
        assert_eq!(LookKind::parse("trailer"), Some(LookKind::Trailer));
        assert_eq!(LookKind::parse("fancy"), None);
        for q in Quality::ALL {
            assert_eq!(Quality::parse(&q.name().to_lowercase()), Some(q));
        }
        assert_eq!(Quality::Medium.row(), "trailer-medium");
    }
}
