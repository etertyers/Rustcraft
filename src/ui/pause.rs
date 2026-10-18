//! The pause menu Escape opens in a world (1.0.0's GuiIngameMenu: Back to
//! Game, Options..., Save and Quit to Title) and the Options screen it
//! shares with the title screen (1.0.0's GuiOptions and GuiVideoSettings,
//! cut down to the settings this game has).

use egui::{Align2, Button, Color32, RichText, Slider};

use crate::options::{FOVS, Options, RENDER_DISTANCES, SENSITIVITIES};

const BUTTON: [f32; 2] = [260.0, 32.0];

/// What the player chose in the pause menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PauseAction {
    Resume,
    SaveAndQuit,
}

/// The pause menu, or its Options page.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct PauseMenu {
    pub options_open: bool,
}

impl PauseMenu {
    /// Escape: from Options back to the menu, from the menu back to the game.
    pub fn back(&mut self) -> Option<PauseAction> {
        if self.options_open {
            self.options_open = false;
            None
        } else {
            Some(PauseAction::Resume)
        }
    }

    /// Draws the menu over the darkened game; changes go straight into
    /// `options`.
    pub fn show(&mut self, ctx: &egui::Context, options: &mut Options) -> Option<PauseAction> {
        let full = ctx.content_rect();
        let painter = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Background,
            egui::Id::unique("pause"),
        ));
        painter.rect_filled(full, 0.0, Color32::from_black_alpha(140));
        let mut action = None;
        egui::Area::new(egui::Id::unique("pause-menu"))
            .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    if self.options_open {
                        if options_page(ui, options) {
                            self.options_open = false;
                        }
                        return;
                    }
                    heading(ui, "Game menu");
                    if ui.add_sized(BUTTON, Button::new("Back to Game")).clicked() {
                        action = Some(PauseAction::Resume);
                    }
                    ui.add_space(8.0);
                    if ui.add_sized(BUTTON, Button::new("Options...")).clicked() {
                        self.options_open = true;
                    }
                    ui.add_space(8.0);
                    if ui
                        .add_sized(BUTTON, Button::new("Save and Quit to Title"))
                        .clicked()
                    {
                        action = Some(PauseAction::SaveAndQuit);
                    }
                });
            });
        action
    }
}

/// The Options page: sliders for render distance, field of view, mouse
/// sensitivity and volume, a vsync toggle and Done. Returns true when Done
/// is clicked.
pub fn options_page(ui: &mut egui::Ui, options: &mut Options) -> bool {
    heading(ui, "Options");
    let width = BUTTON[0];
    ui.spacing_mut().slider_width = width;
    let row = |ui: &mut egui::Ui, label: String| {
        ui.label(RichText::new(label).color(Color32::WHITE));
    };
    row(
        ui,
        format!("Render Distance: {} chunks", options.render_distance),
    );
    ui.add(Slider::new(&mut options.render_distance, RENDER_DISTANCES).show_value(false));
    ui.add_space(8.0);
    row(ui, format!("FOV: {}", fov_name(options.fov)));
    ui.add(
        Slider::new(&mut options.fov, FOVS)
            .step_by(1.0)
            .show_value(false),
    );
    ui.add_space(8.0);
    row(ui, format!("Sensitivity: {}", percent(options.sensitivity)));
    ui.add(
        Slider::new(&mut options.sensitivity, SENSITIVITIES)
            .step_by(0.01)
            .show_value(false),
    );
    ui.add_space(8.0);
    row(
        ui,
        format!("Volume: {} (no sound yet)", percent(options.volume)),
    );
    ui.add(
        Slider::new(&mut options.volume, 0.0..=1.0)
            .step_by(0.01)
            .show_value(false),
    );
    ui.add_space(8.0);
    let vsync = format!("VSync: {}", if options.vsync { "ON" } else { "OFF" });
    if ui.add_sized(BUTTON, Button::new(vsync)).clicked() {
        options.vsync = !options.vsync;
    }
    ui.add_space(16.0);
    ui.add_sized(BUTTON, Button::new("Done")).clicked()
}

/// 1.0.0's names for the ends and middle of the FOV slider.
fn fov_name(fov: f32) -> String {
    match fov.round() as i32 {
        70 => "Normal".to_owned(),
        110 => "Quake Pro".to_owned(),
        degrees => degrees.to_string(),
    }
}

fn percent(fraction: f32) -> String {
    format!("{}%", (fraction * 100.0).round() as i32)
}

fn heading(ui: &mut egui::Ui, text: &str) {
    ui.add_space(16.0);
    ui.label(RichText::new(text).size(28.0).color(Color32::WHITE));
    ui.add_space(16.0);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escape_steps_back_out_of_options_then_resumes() {
        let mut pause = PauseMenu { options_open: true };
        assert_eq!(pause.back(), None);
        assert!(!pause.options_open);
        assert_eq!(pause.back(), Some(PauseAction::Resume));
    }

    #[test]
    fn labels_read_like_1_0_0() {
        assert_eq!(fov_name(70.0), "Normal");
        assert_eq!(fov_name(110.0), "Quake Pro");
        assert_eq!(fov_name(85.4), "85");
        assert_eq!(percent(1.0), "100%");
        assert_eq!(percent(0.25), "25%");
    }
}
