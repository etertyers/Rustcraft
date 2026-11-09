//! The sound extras as the app runs them (M65): once per game tick they
//! look at what this client sees and make footsteps for every foot on the
//! ground; every couple of seconds they work out which ambient beds fit the
//! place; and every half second they snapshot the blocks around the
//! listener for Sound Physics. Everything is client-side.

use std::{collections::HashMap, sync::Arc};

use glam::DVec3;

use super::{
    ambient::{Ambient, Bed, Dimension, Situation},
    footsteps::{Footsteps, Locomotion, StepCue, StepEvent, Stride},
    physics::{ChunkSnapshot, Solids},
};
use crate::game::Game;

/// Which sound extras are on.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SoundOn {
    pub footsteps: bool,
    pub physics: bool,
    pub ambient: bool,
}

/// One walker's footfall memory.
#[derive(Debug, Default, Clone, Copy)]
struct Walker {
    stride: Stride,
    was_on_ground: bool,
    fall: f64,
    last_y: f64,
    climbed: f64,
}

#[derive(Default)]
pub struct Live {
    footsteps: Option<Footsteps>,
    ambient: Option<Ambient>,
    loaded: bool,
    walkers: HashMap<u64, Walker>,
    /// Footsteps waiting out a delay: (game tick due, sound, where).
    pending: Vec<(u64, StepCue, DVec3)>,
    seed: u64,
    snapshot: Option<Arc<dyn Solids>>,
    snapshot_tick: Option<u64>,
    beds: Vec<Bed>,
    beds_tick: Option<u64>,
    last_tick: Option<u64>,
}

impl Live {
    fn load(&mut self) {
        if self.loaded {
            return;
        }
        self.loaded = true;
        let dir = crate::assets::dir().join("downloads/mods");
        self.footsteps = Footsteps::load(&dir).filter(|f| !f.is_empty());
        self.ambient = Ambient::load(&dir).filter(|a| !a.is_empty());
        log::info!(
            "sound extras: footsteps {}, ambient sounds {}",
            if self.footsteps.is_some() {
                "loaded"
            } else {
                "missing"
            },
            if self.ambient.is_some() {
                "loaded"
            } else {
                "missing"
            },
        );
    }

    fn random(&mut self) -> f32 {
        self.seed = self
            .seed
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        ((self.seed >> 40) as f32) / (1u64 << 24) as f32
    }

    /// Whether vanilla's footsteps should be muted: only when ours play.
    pub fn footsteps_ready(&self, on: SoundOn) -> bool {
        on.footsteps && self.footsteps.is_some()
    }

    /// The beds that fit where the player is now.
    pub fn beds(&self) -> &[Bed] {
        &self.beds
    }

    /// The blocks around the listener, for Sound Physics.
    pub fn snapshot(&self) -> Option<Arc<dyn Solids>> {
        self.snapshot.clone()
    }

    /// One game tick.
    pub fn tick(&mut self, game: &Game, on: SoundOn) {
        self.load();
        let tick = game.time();
        if self.last_tick == Some(tick) {
            return;
        }
        self.last_tick = Some(tick);
        if on.footsteps {
            self.footsteps_tick(game, tick);
        } else {
            self.walkers.clear();
            self.pending.clear();
        }
        if on.physics && self.snapshot_tick.is_none_or(|t| tick >= t + 10) {
            self.snapshot_tick = Some(tick);
            self.snapshot = Some(Arc::new(ChunkSnapshot::around(game)));
        } else if !on.physics {
            self.snapshot = None;
            self.snapshot_tick = None;
        }
        if on.ambient && self.beds_tick.is_none_or(|t| tick >= t + 40) {
            self.beds_tick = Some(tick);
            if let Some(ambient) = &self.ambient {
                self.beds = ambient.beds(&situation(game));
            }
        } else if !on.ambient {
            self.beds.clear();
            self.beds_tick = None;
        }
    }

    fn footsteps_tick(&mut self, game: &Game, tick: u64) {
        let Some(data) = self.footsteps.take() else {
            return;
        };
        // Delayed parts of earlier steps.
        let due: Vec<_> = self
            .pending
            .iter()
            .filter(|(t, ..)| *t <= tick)
            .cloned()
            .collect();
        self.pending.retain(|(t, ..)| *t > tick);
        for (_, cue, at) in due {
            crate::audio::play_at(cue.event, at, cue.volume, cue.pitch);
        }
        let world = game.chunks.world();
        let block_under = |at: DVec3| {
            let cell = (at - DVec3::Y * 0.2).floor().as_ivec3();
            let state = world.block(cell.x, cell.y, cell.z)?;
            Some(state)
        };
        let materials = |this: &Self, at: DVec3| -> Vec<String> {
            let _ = this;
            let Some(state) = block_under(at) else {
                return Vec::new();
            };
            let name = state.def().name.clone();
            let step = crate::audio::events::block_event(&name, "step");
            data.materials(&name, &step)
        };
        let mut made: Vec<(StepCue, DVec3)> = Vec::new();
        let mut random = {
            let mut s = self.seed ^ tick.wrapping_mul(0x9E37_79B9);
            move || {
                s = s
                    .wrapping_mul(6_364_136_223_846_793_005)
                    .wrapping_add(1_442_695_040_888_963_407);
                ((s >> 40) as f32) / (1u64 << 24) as f32
            }
        };
        let mut seen = Vec::new();
        // The player.
        {
            let me = game.player.body_state();
            let pose = game.player_pose();
            let w = self.walkers.entry(u64::MAX).or_default();
            seen.push(u64::MAX);
            let on_ground = me.on_ground && me.liquid.is_none() && !me.flying;
            let feet = me.position();
            let amount = pose.limb_amount;
            let sneaking = me.sneaking;
            let mut events: Vec<StepEvent> = Vec::new();
            if on_ground {
                if !w.was_on_ground && w.fall < -0.3 {
                    events.push(StepEvent::Land);
                }
                for _ in 0..w.stride.steps(pose.limb_swing, 2) {
                    events.push(StepEvent::for_pace(amount, sneaking));
                }
            } else {
                w.stride.steps(pose.limb_swing, 2);
            }
            // Climbing: a step for every block or so up or down.
            if me.on_climbable && !on_ground {
                w.climbed += (feet.y - w.last_y).abs();
                if w.climbed > 0.6 {
                    w.climbed = 0.0;
                    events.push(StepEvent::Walk);
                }
            }
            w.was_on_ground = on_ground;
            w.fall = me.velocity.y;
            w.last_y = feet.y;
            let mats = materials(self, feet);
            for event in events {
                let quiet = if sneaking { 0.4 } else { 1.0 };
                for mut cue in data.cues(&mats, event, &mut random) {
                    cue.volume *= quiet;
                    made.push((cue, feet));
                }
            }
        }
        // Mobs that walk.
        for mob in &game.mobs {
            let def = game.mob_defs().get(mob.kind);
            let feet_count = match data.locomotion(&def.name) {
                Locomotion::None => continue,
                l => l.feet(),
            };
            let w = self.walkers.entry(mob.id).or_default();
            seen.push(mob.id);
            let on_ground = mob.body.on_ground && mob.body.liquid.is_none() && !mob.is_dead();
            let (swing, amount) = mob.limbs(1.0);
            let mut events = Vec::new();
            if on_ground {
                if !w.was_on_ground && w.fall < -0.3 {
                    events.push(StepEvent::Land);
                }
                for _ in 0..w.stride.steps(swing, feet_count) {
                    events.push(StepEvent::for_pace(amount, false));
                }
            } else {
                w.stride.steps(swing, feet_count);
            }
            w.was_on_ground = on_ground;
            w.fall = mob.body.velocity.y;
            if events.is_empty() {
                continue;
            }
            let feet = mob.feet();
            let mats = materials(self, feet);
            let scale = if mob.data.age < 0 { 0.6 } else { 1.0 };
            for event in events {
                for mut cue in data.cues(&mats, event, &mut random) {
                    cue.volume *= scale * 0.8;
                    made.push((cue, feet));
                }
            }
        }
        self.walkers.retain(|id, _| seen.contains(id));
        self.footsteps = Some(data);
        for (cue, at) in made {
            if cue.delay_ticks == 0 {
                crate::audio::play_at(cue.event, at, cue.volume, cue.pitch);
            } else {
                self.pending
                    .push((tick + u64::from(cue.delay_ticks), cue, at));
            }
        }
        let _ = self.random();
    }
}

/// What the ambient conditions look at, from the game.
pub fn situation(game: &Game) -> Situation {
    let eye = game.player.camera.position.as_dvec3();
    let cell = eye.floor().as_ivec3();
    let world = game.chunks.world();
    let biome = world
        .biome_at_block(cell.x, cell.y, cell.z)
        .unwrap_or(crate::world::biome::Biome::Plains);
    let (sky, _) = game
        .chunks
        .light_at(cell.x, cell.y, cell.z)
        .unwrap_or((15, 0));
    // The ground under the player: the highest solid block in the column.
    let mut ground = cell.y;
    let mut y = (cell.y + 64).min(crate::config::MAX_Y - 1);
    while y > crate::config::MIN_Y {
        if world
            .block(cell.x, y, cell.z)
            .is_some_and(|b| !b.is_air() && b.def().solid && b.def().fluid.is_none())
        {
            ground = y;
            break;
        }
        y -= 1;
    }
    // Water over the head, in blocks.
    let mut depth: f32 = 0.0;
    if game.eye_in() == Some(crate::world::block::Fluid::Water) {
        depth = 0.5;
        let mut y = cell.y + 1;
        while world
            .block(cell.x, y, cell.z)
            .is_some_and(|b| b.is(crate::world::blocks::WATER))
            && depth < 30.0
        {
            depth += 1.0;
            y += 1;
        }
        depth = depth.max(0.5);
    }
    let day = game.day_time() % 24_000;
    use crate::game::weather::Weather;
    Situation {
        dimension: match game.dimension() {
            crate::world::dimension::Dimension::Overworld => Dimension::Overworld,
            crate::world::dimension::Dimension::Nether => Dimension::Nether,
            _ => Dimension::End,
        },
        biome: format!("{biome:?}").to_lowercase(),
        temperature: biome.temperature_at(cell.y),
        hour: (day as f32 / 1000.0 + 6.0) % 24.0,
        y: eye.y as f32,
        relative_height: (cell.y - ground) as f32,
        sky_light: f32::from(sky),
        sky: f32::from(sky) / 15.0,
        underwater: depth,
        raining: game.weather != Weather::Clear,
        thunder: game.weather == Weather::Thunder,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{audio, bindings::Action, game::tests::flat_game, player::mode::GameMode};

    fn all_on() -> SoundOn {
        SoundOn {
            footsteps: true,
            physics: true,
            ambient: true,
        }
    }

    #[test]
    fn walking_on_grass_queues_material_footsteps_foot_by_foot() {
        if Footsteps::load(&crate::assets::dir().join("downloads/mods")).is_none() {
            eprintln!("skipped: no Presence Footsteps jar");
            return;
        }
        let mut game = flat_game(GameMode::Survival);
        let mut live = Live::default();
        audio::drain();
        game.run_ticks(10);
        live.tick(&game, all_on());
        audio::drain();
        game.player.keys.set_key(Action::Forward, true);
        let mut cues = Vec::new();
        for _ in 0..80 {
            game.run_ticks(1);
            live.tick(&game, all_on());
            cues.extend(audio::drain());
        }
        let steps: Vec<_> = cues
            .iter()
            .filter(|c| c.event.starts_with("pf_presence."))
            .collect();
        assert!(
            steps.len() >= 6,
            "{} footstep sounds in 80 ticks",
            steps.len()
        );
        assert!(steps.iter().all(|c| c.at.is_some()), "each at its foot");
        // Layered, on grass: more than one kind of sound.
        let kinds: std::collections::HashSet<_> = steps.iter().map(|c| &c.event).collect();
        assert!(kinds.len() >= 2, "{kinds:?}");
    }

    #[test]
    fn with_every_toggle_off_nothing_is_added_or_muted() {
        let mut game = flat_game(GameMode::Survival);
        let mut live = Live::default();
        game.player.keys.set_key(Action::Forward, true);
        audio::drain();
        let off = SoundOn::default();
        for _ in 0..40 {
            game.run_ticks(1);
            live.tick(&game, off);
        }
        assert!(live.beds().is_empty());
        assert!(live.snapshot().is_none());
        assert!(!live.footsteps_ready(off));
        assert!(
            !audio::drain()
                .iter()
                .any(|c| c.event.starts_with("pf_presence."))
        );
        // Vanilla's steps survive the filter exactly when muting is off.
        let step = audio::Cue {
            event: "block.grass.step".into(),
            at: None,
            volume: 1.0,
            pitch: 1.0,
        };
        assert!(audio::keeps(&step, false));
        assert!(!audio::keeps(&step, true));
        let other = audio::Cue {
            event: "block.grass.break".into(),
            ..step
        };
        assert!(audio::keeps(&other, true) && audio::keeps(&other, false));
    }
}
