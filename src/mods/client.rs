//! The client's side of mods (M39): the Lua state whose scripts handle key
//! presses and draw text above the hotbar or change how bright the world
//! is drawn. It runs on the player's own computer, in single player and
//! on LAN alike.

use super::lua::{ClientHost, Runtime, Side};

/// Text a mod put above the hotbar.
struct Text {
    id: String,
    text: String,
    /// Seconds left; `None` stays until the mod clears it.
    left: Option<f32>,
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
                left: seconds,
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
                if let Some(left) = &mut text.left {
                    *left -= 0.05;
                }
            }
            self.screen.texts.retain(|t| t.left.is_none_or(|l| l > 0.0));
        }
    }

    /// The lines to draw above the hotbar, oldest first.
    pub fn lines(&self) -> Vec<String> {
        self.screen.texts.iter().map(|t| t.text.clone()).collect()
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

    #[test]
    fn texts_expire_and_persistent_ones_stay() {
        let mut mods = ClientMods {
            runtime: None,
            screen: Screen::default(),
            carry: 0.0,
        };
        mods.screen.hud_text("a", Some("short".into()), Some(0.5));
        mods.screen.hud_text("b", Some("stays".into()), None);
        assert_eq!(mods.lines().len(), 2);
        mods.advance(0.2);
        mods.advance(0.2);
        assert_eq!(mods.lines().len(), 2);
        mods.advance(0.2);
        assert_eq!(mods.lines(), ["stays"]);
        mods.screen.hud_text("b", None, None);
        assert!(mods.lines().is_empty());
    }
}
