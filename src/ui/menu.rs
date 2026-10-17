//! The main menu, following 1.0.0's screens: the title screen
//! (Singleplayer, Quit Game), Select World (each world's mode and when it
//! was last played) and Create New World (name, seed, game mode).

use std::{
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use egui::{Align, Button, Color32, Frame, Layout, RichText};

use crate::{
    level::{self, Level, WorldDir},
    options::Options,
    player::mode::GameMode,
};

/// What the player chose.
pub enum MenuAction {
    Quit,
    Open(WorldDir),
    Create {
        name: String,
        seed: u64,
        mode: GameMode,
    },
}

enum Screen {
    Title,
    Options,
    SelectWorld,
    CreateWorld,
}

pub struct Menu {
    saves: PathBuf,
    screen: Screen,
    worlds: Vec<(WorldDir, Level)>,
    selected: Option<usize>,
    name: String,
    seed: String,
    mode: GameMode,
    /// Why the last world failed to open, shown until the next choice.
    pub error: Option<String>,
}

const BUTTON: [f32; 2] = [260.0, 32.0];
const BACKGROUND: Color32 = Color32::from_rgb(0x2a, 0x22, 0x1b);
const GRAY: Color32 = Color32::from_rgb(0xa0, 0xa0, 0xa0);

impl Menu {
    pub fn new(saves: PathBuf) -> Self {
        Self {
            saves,
            screen: Screen::Title,
            worlds: Vec::new(),
            selected: None,
            name: String::new(),
            seed: String::new(),
            mode: GameMode::Survival,
            error: None,
        }
    }

    pub fn show(&mut self, ui: &mut egui::Ui, options: &mut Options) -> Option<MenuAction> {
        let mut action = None;
        egui::CentralPanel::default()
            .frame(Frame::new().fill(BACKGROUND).inner_margin(24.0))
            .show(ui, |ui| {
                ui.vertical_centered(|ui| {
                    action = match self.screen {
                        Screen::Title => self.title(ui),
                        Screen::Options => {
                            if super::pause::options_page(ui, options) {
                                self.screen = Screen::Title;
                            }
                            None
                        }
                        Screen::SelectWorld => self.select_world(ui),
                        Screen::CreateWorld => self.create_world(ui),
                    };
                    if let Some(error) = &self.error {
                        ui.add_space(12.0);
                        ui.colored_label(Color32::from_rgb(0xff, 0x60, 0x60), error);
                    }
                });
            });
        if action.is_some() {
            self.error = None;
        }
        action
    }

    fn title(&mut self, ui: &mut egui::Ui) -> Option<MenuAction> {
        ui.add_space(ui.available_height() * 0.2);
        ui.label(
            RichText::new("Rustcraft")
                .size(56.0)
                .strong()
                .color(Color32::WHITE),
        );
        ui.add_space(48.0);
        if ui.add_sized(BUTTON, Button::new("Singleplayer")).clicked() {
            self.worlds = level::list_worlds(&self.saves);
            self.selected = None;
            self.screen = Screen::SelectWorld;
        }
        ui.add_space(8.0);
        if ui.add_sized(BUTTON, Button::new("Options...")).clicked() {
            self.screen = Screen::Options;
        }
        ui.add_space(8.0);
        ui.add_sized(BUTTON, Button::new("Quit Game"))
            .clicked()
            .then_some(MenuAction::Quit)
    }

    fn select_world(&mut self, ui: &mut egui::Ui) -> Option<MenuAction> {
        heading(ui, "Select World");
        let mut play = None;
        let list_height = (ui.available_height() - 120.0).max(80.0);
        egui::ScrollArea::vertical()
            .max_height(list_height)
            .max_width(420.0)
            .show(ui, |ui| {
                if self.worlds.is_empty() {
                    ui.label(RichText::new("No worlds yet").color(GRAY));
                }
                let now = unix_now();
                for (i, (dir, level)) in self.worlds.iter().enumerate() {
                    let folder = dir
                        .path()
                        .file_name()
                        .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
                    let text = format!(
                        "{}\n{folder} ({})\n{} Mode",
                        level.name,
                        played_ago(level.last_played, now),
                        level.game_mode.name()
                    );
                    let row = ui.add_sized(
                        [400.0, 56.0],
                        Button::selectable(self.selected == Some(i), text),
                    );
                    if row.clicked() {
                        self.selected = Some(i);
                    }
                    if row.double_clicked() {
                        play = Some(i);
                    }
                }
            });
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            ui.add_space((ui.available_width() - 2.0 * BUTTON[0] - 8.0).max(0.0) / 2.0);
            let can_play = self.selected.is_some();
            if ui
                .add_enabled(
                    can_play,
                    Button::new("Play Selected World").min_size(BUTTON.into()),
                )
                .clicked()
            {
                play = self.selected;
            }
            if ui
                .add(Button::new("Create New World").min_size(BUTTON.into()))
                .clicked()
            {
                self.name = "New World".to_owned();
                self.seed.clear();
                self.mode = GameMode::Survival;
                self.screen = Screen::CreateWorld;
            }
        });
        ui.add_space(8.0);
        if ui.add_sized(BUTTON, Button::new("Cancel")).clicked() {
            self.screen = Screen::Title;
        }
        let i = play?;
        Some(MenuAction::Open(self.worlds[i].0.clone()))
    }

    fn create_world(&mut self, ui: &mut egui::Ui) -> Option<MenuAction> {
        heading(ui, "Create New World");
        let field = |ui: &mut egui::Ui, label: &str, text: &mut String| {
            ui.with_layout(Layout::top_down(Align::Min), |ui| {
                ui.set_max_width(BUTTON[0] * 1.5);
                ui.label(RichText::new(label).color(GRAY));
                ui.add(egui::TextEdit::singleline(text).desired_width(f32::INFINITY));
            });
        };
        field(ui, "World Name", &mut self.name);
        let folder = level::folder_name(&self.name, |name| self.saves.join(name).exists());
        ui.label(RichText::new(format!("Will be saved in: {folder}")).color(GRAY));
        ui.add_space(12.0);
        field(ui, "Seed for the World Generator", &mut self.seed);
        ui.label(RichText::new("Leave blank for a random seed").color(GRAY));
        ui.add_space(12.0);
        let mode = format!("Game Mode: {}", self.mode.name());
        if ui.add_sized(BUTTON, Button::new(mode)).clicked() {
            self.mode = self.mode.next();
        }
        ui.label(RichText::new(mode_description(self.mode)).color(GRAY));
        ui.add_space(24.0);
        let create = ui
            .add_sized(BUTTON, Button::new("Create New World"))
            .clicked();
        ui.add_space(8.0);
        if ui.add_sized(BUTTON, Button::new("Cancel")).clicked() {
            self.screen = Screen::SelectWorld;
        }
        create.then(|| MenuAction::Create {
            name: self.name.trim().to_owned(),
            seed: level::parse_seed(&self.seed, level::random_seed),
            mode: self.mode,
        })
    }
}

fn heading(ui: &mut egui::Ui, text: &str) {
    ui.add_space(16.0);
    ui.label(RichText::new(text).size(28.0).color(Color32::WHITE));
    ui.add_space(16.0);
}

/// 1.0.0's description under the game mode button.
fn mode_description(mode: GameMode) -> &'static str {
    match mode {
        GameMode::Survival => "Search for resources, crafting, gain levels, health and hunger",
        GameMode::Creative => "Unlimited resources, free flying and destroy blocks instantly",
    }
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// When a world was last played, relative to `now` (both Unix seconds).
fn played_ago(last_played: u64, now: u64) -> String {
    let seconds = now.saturating_sub(last_played);
    let (count, unit) = match seconds {
        0..60 => return "just now".to_owned(),
        60..3600 => (seconds / 60, "minute"),
        3600..86400 => (seconds / 3600, "hour"),
        _ => (seconds / 86400, "day"),
    };
    let plural = if count == 1 { "" } else { "s" };
    format!("{count} {unit}{plural} ago")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn last_played_reads_naturally() {
        let now = 1_000_000;
        assert_eq!(played_ago(now - 5, now), "just now");
        assert_eq!(played_ago(now + 30, now), "just now");
        assert_eq!(played_ago(now - 60, now), "1 minute ago");
        assert_eq!(played_ago(now - 7200, now), "2 hours ago");
        assert_eq!(played_ago(now - 3 * 86400, now), "3 days ago");
    }
}
