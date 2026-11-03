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
pub mod shadow;
pub mod shafts;
pub mod sun;
pub mod sway;
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

/// The `visuals` options.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Visuals {
    pub look: LookKind,
    pub quality: Quality,
    /// Effects the player turned on or off against the profile.
    pub effects: BTreeMap<Effect, bool>,
    /// Each look's resource packs, highest priority first. The options'
    /// `resource_packs` is always the list of the look that's on.
    pub vanilla_packs: Vec<String>,
    pub trailer_packs: Vec<String>,
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

    /// The pack list kept for `look`.
    pub fn packs(&self, look: LookKind) -> &Vec<String> {
        match look {
            LookKind::Vanilla => &self.vanilla_packs,
            LookKind::Trailer => &self.trailer_packs,
        }
    }

    pub fn packs_mut(&mut self, look: LookKind) -> &mut Vec<String> {
        match look {
            LookKind::Vanilla => &mut self.vanilla_packs,
            LookKind::Trailer => &mut self.trailer_packs,
        }
    }

    /// What the renderer needs this frame.
    pub fn frame(&self) -> FrameVisuals {
        FrameVisuals {
            look: self.look,
            quality: self.quality,
            effects: Effect::ALL.map(|e| self.enabled(e)),
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
