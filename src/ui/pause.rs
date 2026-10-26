//! The pause menu Escape opens in a world (vanilla's: Back to Game,
//! Options..., Open to LAN, Save and Quit to Title), opening the Options
//! screens it shares with the title screen and the Open to LAN screen.

use egui::{Align2, Button, Color32, RichText};

use super::ClickSound;
use super::options::{OptionsAction, OptionsScreen};
use super::packs::PackScreen;
use crate::{level::Difficulty, options::Options};

const BUTTON: [f32; 2] = [260.0, 32.0];

/// What the player chose in the pause menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum PauseAction {
    Resume,
    SaveAndQuit,
    /// Start LAN World: guests play in `mode`, may cheat or not, on `port`
    /// (any free one if `None`).
    OpenToLan {
        mode: crate::player::mode::GameMode,
        cheats: bool,
        port: Option<u16>,
    },
}

/// The pause menu, or its Options page, or the Resource Packs screen.
#[derive(Default)]
pub struct PauseMenu {
    pub options: Option<OptionsScreen>,
    pub packs: Option<PackScreen>,
    pub lan: Option<LanScreen>,
    /// Whether this world can be opened to LAN (the host's, not yet open),
    /// set by the caller.
    pub can_open_lan: bool,
    /// The world allows cheats, so Open to LAN's Allow Cheats starts on.
    pub world_cheats: bool,
}

/// Open to LAN's choices (vanilla's `ShareToLanScreen`).
#[derive(Debug, Clone, PartialEq)]
pub struct LanScreen {
    pub mode: crate::player::mode::GameMode,
    pub cheats: bool,
    pub port: String,
}

impl PauseMenu {
    /// Escape: from Resource Packs back to Options without applying, up
    /// the Options pages to the menu, from the menu back to the game.
    pub fn back(&mut self) -> Option<PauseAction> {
        if self.packs.take().is_some() || self.lan.take().is_some() {
            None
        } else if let Some(options) = self.options.as_mut() {
            if options.back() {
                self.options = None;
            }
            None
        } else {
            Some(PauseAction::Resume)
        }
    }

    /// Draws the menu over the darkened game; changes go straight into
    /// `options`.
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        options: &mut Options,
        difficulty: &mut Difficulty,
    ) -> Option<PauseAction> {
        let full = ctx.content_rect();
        let painter = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Background,
            egui::Id::unique("pause"),
        ));
        painter.rect_filled(full, 0.0, Color32::from_black_alpha(140));
        let mut action = None;
        egui::Area::new(egui::Id::unique("pause-menu"))
            .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
            .fade_in(false)
            .show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    if let Some(packs) = self.packs.as_mut() {
                        if let Some(selection) = packs.show(ui) {
                            options.resource_packs = selection;
                            self.packs = None;
                        }
                        return;
                    }
                    if let Some(screen) = self.options.as_mut() {
                        match screen.show(ui, options, Some(difficulty)) {
                            Some(OptionsAction::Done) => self.options = None,
                            Some(OptionsAction::ResourcePacks) => {
                                self.packs = Some(PackScreen::new(
                                    crate::pack::packs_dir(),
                                    &options.resource_packs,
                                ));
                            }
                            None => {}
                        }
                        return;
                    }
                    if let Some(lan) = self.lan.as_mut() {
                        match lan_screen(ui, lan) {
                            Some(LanChoice::Start(open)) => {
                                action = Some(open);
                                self.lan = None;
                            }
                            Some(LanChoice::Cancel) => self.lan = None,
                            None => {}
                        }
                        return;
                    }
                    heading(ui, "Game menu");
                    if ui
                        .add_sized(BUTTON, Button::new("Back to Game"))
                        .clicked_with_sound()
                    {
                        action = Some(PauseAction::Resume);
                    }
                    ui.add_space(8.0);
                    if ui
                        .add_sized(BUTTON, Button::new("Options..."))
                        .clicked_with_sound()
                    {
                        self.options = Some(OptionsScreen::default());
                    }
                    ui.add_space(8.0);
                    let lan = ui.add_enabled(
                        self.can_open_lan,
                        Button::new("Open to LAN").min_size(BUTTON.into()),
                    );
                    if lan.clicked_with_sound() {
                        self.lan = Some(LanScreen {
                            mode: crate::player::mode::GameMode::Survival,
                            cheats: self.world_cheats,
                            port: String::new(),
                        });
                    }
                    ui.add_space(8.0);
                    if ui
                        .add_sized(BUTTON, Button::new("Save and Quit to Title"))
                        .clicked_with_sound()
                    {
                        action = Some(PauseAction::SaveAndQuit);
                    }
                });
            });
        action
    }
}

enum LanChoice {
    Start(PauseAction),
    Cancel,
}

/// Open to LAN: the other players' game mode, Allow Cheats, the port, and
/// Start LAN World or Cancel.
fn lan_screen(ui: &mut egui::Ui, lan: &mut LanScreen) -> Option<LanChoice> {
    use crate::player::mode::GameMode;
    heading(ui, "Open to LAN");
    let mode = format!("Game Mode: {}", lan.mode.name());
    if ui.add_sized(BUTTON, Button::new(mode)).clicked_with_sound() {
        lan.mode = match lan.mode {
            GameMode::Survival => GameMode::Creative,
            GameMode::Creative => GameMode::Survival,
        };
    }
    ui.add_space(8.0);
    let cheats = format!("Allow Cheats: {}", if lan.cheats { "ON" } else { "OFF" });
    if ui
        .add_sized(BUTTON, Button::new(cheats))
        .clicked_with_sound()
    {
        lan.cheats = !lan.cheats;
    }
    ui.add_space(8.0);
    ui.label(RichText::new("Port Number").color(Color32::from_rgb(0xa0, 0xa0, 0xa0)));
    ui.add_sized(
        [BUTTON[0], 24.0],
        egui::TextEdit::singleline(&mut lan.port)
            .hint_text("any free port")
            .char_limit(5),
    );
    let port = match lan.port.trim() {
        "" => Some(None),
        text => text.parse::<u16>().ok().filter(|&p| p >= 1024).map(Some),
    };
    if port.is_none() {
        ui.label(
            RichText::new("A port from 1024 to 65535").color(Color32::from_rgb(0xff, 0x55, 0x55)),
        );
    }
    ui.add_space(16.0);
    let start = ui
        .add_enabled(
            port.is_some(),
            Button::new("Start LAN World").min_size(BUTTON.into()),
        )
        .clicked_with_sound();
    ui.add_space(8.0);
    if ui
        .add_sized(BUTTON, Button::new("Cancel"))
        .clicked_with_sound()
    {
        return Some(LanChoice::Cancel);
    }
    match (start, port) {
        (true, Some(port)) => Some(LanChoice::Start(PauseAction::OpenToLan {
            mode: lan.mode,
            cheats: lan.cheats,
            port,
        })),
        _ => None,
    }
}

fn heading(ui: &mut egui::Ui, text: &str) {
    ui.add_space(16.0);
    ui.label(RichText::new(text).size(32.0).color(Color32::WHITE));
    ui.add_space(16.0);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escape_steps_back_out_of_packs_and_options_then_resumes() {
        let dir = crate::world::save::TempDir::new("pause-packs");
        let mut pause = PauseMenu {
            options: Some(OptionsScreen::default()),
            packs: Some(PackScreen::new(dir.path().to_owned(), &[])),
            ..PauseMenu::default()
        };
        assert_eq!(pause.back(), None);
        assert!(pause.packs.is_none() && pause.options.is_some());
        assert_eq!(pause.back(), None);
        assert!(pause.options.is_none());
        assert_eq!(pause.back(), Some(PauseAction::Resume));
    }
}
