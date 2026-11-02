//! The client's side of mods (M39): the Lua state whose scripts handle key
//! presses and draw text above the hotbar or change how bright the world
//! is drawn. It runs on the player's own computer, in single player and
//! on LAN alike.

use super::lua::{ClientHost, Runtime, Side};
use crate::game::overlay::{FADE_TICKS, SHOW_TICKS, alpha};

/// Text a mod put above the hotbar.
struct Text {
    id: String,
    text: String,
    /// Ticks left, counting the fade (the last `FADE_TICKS`).
    left: f32,
}

#[derive(Default)]
struct Screen {
    texts: Vec<Text>,
    min_light: Option<u8>,
}

impl ClientHost for Screen {
    fn hud_text(&mut self, id: &str, text: Option<String>, seconds: Option<f32>) {
        self.texts.retain(|t| t.id != id);
        if let Some(text) = text {
            self.texts.push(Text {
                id: id.to_owned(),
                text,
                // Without a time: the overlay message's 3 seconds.
                left: seconds.map_or(SHOW_TICKS as f32, |s| (s * 20.0).max(0.0))
                    + FADE_TICKS as f32,
            });
        }
    }

    fn brightness_override(&mut self, level: Option<u8>) {
        self.min_light = level;
    }
}

pub struct ClientMods {
    runtime: Option<Runtime>,
    screen: Screen,
    /// Seconds not yet turned into 20 Hz ticks.
    carry: f32,
}

impl ClientMods {
    /// Starts the client's Lua if any enabled mod has a script.
    pub fn start() -> Self {
        let set = super::active();
        let runtime = if set.active.iter().any(|m| m.lua.is_some()) {
            match Runtime::start(Side::Client, set) {
                Ok(runtime) => Some(runtime),
                Err(err) => {
                    super::report_error("lua", super::lua::short(&err));
                    None
                }
            }
        } else {
            None
        };
        Self {
            runtime,
            screen: Screen::default(),
            carry: 0.0,
        }
    }

    /// A mod key (`mod:key`) was pressed.
    pub fn key_pressed(&mut self, id: &str) {
        if let Some(runtime) = &self.runtime {
            runtime.key_pressed(&mut self.screen, id);
        }
    }

    /// Advances timers and the texts' clocks by `dt` seconds.
    pub fn advance(&mut self, dt: f32) {
        self.carry += dt.min(0.25);
        while self.carry >= 0.05 {
            self.carry -= 0.05;
            if let Some(runtime) = &mut self.runtime {
                runtime.tick_timers_client(&mut self.screen);
            }
            for text in &mut self.screen.texts {
                text.left -= 1.0;
            }
            self.screen.texts.retain(|t| t.left > 0.0);
        }
    }

    /// The lines to draw above the hotbar, oldest first, with how opaque
    /// each is (0–1).
    pub fn lines(&self) -> Vec<(String, f32)> {
        self.screen
            .texts
            .iter()
            .map(|t| (t.text.clone(), alpha(t.left)))
            .collect()
    }

    /// The light level the world is lit at least as much as (0 is off).
    pub fn min_light(&self) -> f32 {
        f32::from(self.screen.min_light.unwrap_or(0))
    }

    /// Back to normal, as when leaving a world.
    pub fn reset(&mut self) {
        self.screen = Screen::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mods() -> ClientMods {
        ClientMods {
            runtime: None,
            screen: Screen::default(),
            carry: 0.0,
        }
    }

    /// Runs `ticks` 20 Hz ticks.
    fn run(mods: &mut ClientMods, ticks: u32) {
        for _ in 0..ticks {
            mods.advance(0.05);
        }
    }

    #[test]
    fn every_text_fades_after_its_time_and_one_without_a_time_lasts_sixty_ticks() {
        let mut mods = mods();
        mods.screen.hud_text("a", Some("short".into()), Some(0.5));
        mods.screen.hud_text("b", Some("no time".into()), None);
        assert_eq!(mods.lines().len(), 2);
        run(&mut mods, 10);
        assert_eq!(
            mods.lines().len(),
            2,
            "short is at full opacity until it fades"
        );
        run(&mut mods, 20);
        let names: Vec<_> = mods.lines().into_iter().map(|l| l.0).collect();
        assert_eq!(
            names,
            ["no time"],
            "short is gone after its time plus the fade"
        );
        run(&mut mods, 30);
        assert_eq!(mods.lines()[0].1, 1.0, "full opacity for its sixty ticks");
        run(&mut mods, 1);
        assert!(mods.lines()[0].1 < 1.0, "fading");
        run(&mut mods, 19);
        assert!(mods.lines().is_empty(), "gone after 80 ticks");
        mods.screen.hud_text("c", Some("x".into()), None);
        mods.screen.hud_text("c", None, None);
        assert!(mods.lines().is_empty());
    }

    #[test]
    fn a_new_action_bar_message_replaces_the_old_and_is_gone_after_eighty_ticks() {
        let mut mods = mods();
        mods.screen.actionbar("Fullbright: ON".into(), 60);
        run(&mut mods, 30);
        mods.screen.actionbar("Fullbright: OFF".into(), 60);
        let lines = mods.lines();
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0], ("Fullbright: OFF".to_owned(), 1.0));
        run(&mut mods, 79);
        assert_eq!(mods.lines().len(), 1, "still fading at 79");
        run(&mut mods, 1);
        assert!(mods.lines().is_empty(), "gone at 80");
    }

    #[test]
    fn the_bundled_fullbright_shows_on_then_off_and_both_fade_away() {
        // The real mod, loaded by the real loader (tests run with the
        // bundled mods).
        let mut mods = ClientMods::start();
        assert!(mods.runtime.is_some(), "fullbright's script runs");
        let shown = |mods: &ClientMods| mods.lines();
        mods.key_pressed("fullbright:toggle");
        assert_eq!(shown(&mods), [("Fullbright: ON".to_owned(), 1.0)]);
        assert_eq!(mods.min_light(), 15.0);
        run(&mut mods, 80);
        assert!(shown(&mods).is_empty(), "ON fades away on its own");
        assert_eq!(mods.min_light(), 15.0, "the light stays on");
        mods.key_pressed("fullbright:toggle");
        assert_eq!(shown(&mods), [("Fullbright: OFF".to_owned(), 1.0)]);
        assert_eq!(mods.min_light(), 0.0);
        run(&mut mods, 80);
        assert!(shown(&mods).is_empty(), "OFF fades away too");
        // Pressed twice in a row, the second replaces the first.
        mods.key_pressed("fullbright:toggle");
        mods.key_pressed("fullbright:toggle");
        assert_eq!(shown(&mods), [("Fullbright: OFF".to_owned(), 1.0)]);
    }
}
