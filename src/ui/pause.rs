//! The pause menu Escape opens in a world (1.0.0's GuiIngameMenu: Back to
//! Game, Options..., Save and Quit to Title), opening the Options screens
//! it shares with the title screen.

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
}

/// The pause menu, or its Options page, or the Resource Packs screen.
#[derive(Default)]
pub struct PauseMenu {
    pub options: Option<OptionsScreen>,
    pub packs: Option<PackScreen>,
}

impl PauseMenu {
    /// Escape: from Resource Packs back to Options without applying, up
    /// the Options pages to the menu, from the menu back to the game.
    pub fn back(&mut self) -> Option<PauseAction> {
        if self.packs.take().is_some() {
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
        };
        assert_eq!(pause.back(), None);
        assert!(pause.packs.is_none() && pause.options.is_some());
        assert_eq!(pause.back(), None);
        assert!(pause.options.is_none());
        assert_eq!(pause.back(), Some(PauseAction::Resume));
    }
}
