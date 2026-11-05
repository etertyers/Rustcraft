//! The video's mods, rebuilt natively (M63–M65): client-side extras with
//! one toggle each in Video Settings → Extras. They're on by default in
//! Barebones and Realistic and off in Vanilla, and each can be switched in
//! any style. None changes what the server decides.

pub mod ambient;
pub mod footsteps;
pub mod fx;
pub mod jar;
pub mod live;
pub mod physics;

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use std::sync::atomic::{AtomicBool, Ordering};

use crate::render::visuals::Style;

static PLAYER_ANIMATIONS: AtomicBool = AtomicBool::new(true);

/// Whether the player-animation extra is on (set each frame from the
/// options; the model reads it).
pub fn player_animations() -> bool {
    PLAYER_ANIMATIONS.load(Ordering::Relaxed)
}

pub fn set_player_animations(on: bool) {
    PLAYER_ANIMATIONS.store(on, Ordering::Relaxed);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Extra {
    /// Not Enough Animations (M63).
    PlayerAnimations,
    /// Visuality.
    VisualParticles,
    /// Cave Dust.
    CaveDust,
    /// Particle Rain.
    RainParticles,
    /// Make Bubbles Pop.
    BubblePop,
    /// Wakes.
    Wakes,
    /// Presence Footsteps.
    FootstepsByMaterial,
    /// Sound Physics Remastered.
    SoundPhysics,
    /// Ambient Sounds.
    AmbientSounds,
}

impl Extra {
    pub const ALL: [Extra; 9] = [
        Extra::PlayerAnimations,
        Extra::VisualParticles,
        Extra::CaveDust,
        Extra::RainParticles,
        Extra::BubblePop,
        Extra::Wakes,
        Extra::FootstepsByMaterial,
        Extra::SoundPhysics,
        Extra::AmbientSounds,
    ];

    /// The toggle's label, named after what it does.
    pub fn name(self) -> &'static str {
        match self {
            Extra::PlayerAnimations => "Player Animations",
            Extra::VisualParticles => "Small Particles",
            Extra::CaveDust => "Cave Dust",
            Extra::RainParticles => "Rain and Snow Particles",
            Extra::BubblePop => "Rising, Popping Bubbles",
            Extra::Wakes => "Water Wakes",
            Extra::FootstepsByMaterial => "Footsteps by Material",
            Extra::SoundPhysics => "Sound Physics",
            Extra::AmbientSounds => "Ambient Sounds",
        }
    }

    /// The tooltip: what it does and the mod it's built from.
    pub fn tooltip(self) -> &'static str {
        match self {
            Extra::PlayerAnimations => {
                "Players eat, climb, row, ride, sleep and hold maps with their whole body. \
                 (Not Enough Animations)"
            }
            Extra::VisualParticles => {
                "Sparkles on ores, feathers, bones and blobs when mobs are hit, rings on water \
                 in rain. (Visuality)"
            }
            Extra::CaveDust => "Slow dust motes in dark caves, more the deeper you go. (Cave Dust)",
            Extra::RainParticles => {
                "Rain and snow as particles that splash, ripple and stop at roofs, and dust in \
                 deserts. (Particle Rain)"
            }
            Extra::BubblePop => "Bubbles rise to the surface and pop there. (Make Bubbles Pop)",
            Extra::Wakes => {
                "Wakes behind boats, swimmers and mobs, and splashes when something falls into \
                 water. (Wakes)"
            }
            Extra::FootstepsByMaterial => {
                "Every foot makes its own sound from the block it lands on, layered, for the \
                 player and walking mobs. (Presence Footsteps)"
            }
            Extra::SoundPhysics => {
                "Echo in caves and rooms, muffling through walls, dull distant sounds and \
                 muffling under water. (Sound Physics Remastered)"
            }
            Extra::AmbientSounds => {
                "Birds, crickets, wind, water and cave sounds that fade in and out with the \
                 place and the time. (Ambient Sounds)"
            }
        }
    }

    fn key(self) -> &'static str {
        match self {
            Extra::PlayerAnimations => "player_animations",
            Extra::VisualParticles => "visual_particles",
            Extra::CaveDust => "cave_dust",
            Extra::RainParticles => "rain_particles",
            Extra::BubblePop => "bubble_pop",
            Extra::Wakes => "wakes",
            Extra::FootstepsByMaterial => "footsteps",
            Extra::SoundPhysics => "sound_physics",
            Extra::AmbientSounds => "ambient_sounds",
        }
    }
}

/// What the player switched, per style. Anything not switched follows the
/// style's default: on in Barebones and Realistic, off in Vanilla.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Toggles {
    /// `"barebones:cave_dust"` → on or off.
    overrides: BTreeMap<String, bool>,
    /// Every extra on or off for this run (`--extras on|off` in a
    /// performance run); never saved.
    #[serde(skip)]
    pub force: Option<bool>,
}

impl Toggles {
    fn slot(style: Style, extra: Extra) -> String {
        format!("{}:{}", style.name().to_lowercase(), extra.key())
    }

    pub fn default_for(style: Style) -> bool {
        style != Style::Vanilla
    }

    pub fn enabled(&self, style: Style, extra: Extra) -> bool {
        if let Some(on) = self.force {
            return on;
        }
        self.overrides
            .get(&Self::slot(style, extra))
            .copied()
            .unwrap_or_else(|| Self::default_for(style))
    }

    /// Whether the player changed it from the style's default.
    pub fn changed(&self, style: Style, extra: Extra) -> bool {
        self.overrides.contains_key(&Self::slot(style, extra))
    }

    pub fn set(&mut self, style: Style, extra: Extra, on: bool) {
        let slot = Self::slot(style, extra);
        if on == Self::default_for(style) {
            self.overrides.remove(&slot);
        } else {
            self.overrides.insert(slot, on);
        }
    }
}

/// What a client sees that the effects care about: everything that moves
/// through water (the player, mobs, boats) and mobs hurt since the last
/// call (`hurt` remembers each mob's hurt flash a tick ago).
pub fn bodies_and_hits(
    game: &crate::game::Game,
    hurt: &mut std::collections::HashMap<u64, u32>,
) -> (Vec<fx::Body>, Vec<fx::Hit>) {
    use fx::{Body, BodyKind, Hit};
    let in_water = |feet: glam::DVec3| {
        let cell = (feet + glam::DVec3::Y * 0.2).floor().as_ivec3();
        game.chunks
            .world()
            .block(cell.x, cell.y, cell.z)
            .is_some_and(|b| b.is(crate::world::blocks::WATER))
    };
    let me = game.player.body_state();
    let mut bodies = vec![Body {
        id: u64::MAX,
        kind: BodyKind::Player,
        feet: me.position(),
        velocity: me.velocity,
        width: 0.6,
        height: 1.8,
        yaw: game.player.camera.yaw,
        in_water: me.liquid == Some(crate::world::block::Fluid::Water),
    }];
    let mut hits = Vec::new();
    let mut seen = std::collections::HashMap::new();
    for mob in &game.mobs {
        let def = game.mob_defs().get(mob.kind);
        bodies.push(Body {
            id: mob.id,
            kind: BodyKind::Mob,
            feet: mob.feet(),
            velocity: mob.body.velocity,
            width: def.width,
            height: def.height,
            yaw: mob.yaw,
            in_water: in_water(mob.feet()),
        });
        seen.insert(mob.id, mob.hurt_time);
        let before = hurt.get(&mob.id).copied().unwrap_or(0);
        if mob.hurt_time > before {
            hits.push(Hit {
                at: mob.feet() + glam::DVec3::Y * def.height * 0.6,
                mob: def.name.clone(),
                size: mob.data.size,
            });
        }
    }
    *hurt = seen;
    for boat in &game.boats {
        bodies.push(Body {
            id: boat.id | (1 << 62),
            kind: BodyKind::Boat,
            feet: boat.position,
            velocity: boat.velocity,
            width: 1.4,
            height: 0.6,
            yaw: boat.yaw,
            in_water: in_water(boat.position),
        });
    }
    (bodies, hits)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extras_follow_the_style_until_switched_and_each_style_keeps_its_own() {
        let mut toggles = Toggles::default();
        for extra in Extra::ALL {
            assert!(toggles.enabled(Style::Barebones, extra), "{extra:?}");
            assert!(toggles.enabled(Style::Realistic, extra));
            assert!(!toggles.enabled(Style::Vanilla, extra));
        }
        toggles.set(Style::Vanilla, Extra::Wakes, true);
        toggles.set(Style::Barebones, Extra::CaveDust, false);
        assert!(toggles.enabled(Style::Vanilla, Extra::Wakes));
        assert!(!toggles.enabled(Style::Barebones, Extra::CaveDust));
        assert!(toggles.enabled(Style::Realistic, Extra::CaveDust));
        assert!(toggles.changed(Style::Vanilla, Extra::Wakes));
        // Back to the default forgets the override.
        toggles.set(Style::Vanilla, Extra::Wakes, false);
        assert!(!toggles.changed(Style::Vanilla, Extra::Wakes));
        // It round-trips through the options file's format.
        let text = ron::to_string(&toggles).unwrap();
        assert_eq!(ron::from_str::<Toggles>(&text).unwrap(), toggles);
    }
}
