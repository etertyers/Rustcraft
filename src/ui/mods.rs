//! The Mods screen (M39), from the title and pause menus: the mods found,
//! turn them on and off, set the load order, read a mod's details and the
//! errors its scripts caused. What is on is decided when the game starts,
//! so changes apply the next time Rustcraft starts.

use egui::{Button, Color32, RichText, vec2};

use super::ClickSound;
use crate::mods::{self, Config, Status};

const GRAY: Color32 = Color32::from_rgb(0xa0, 0xa0, 0xa0);
const RED: Color32 = Color32::from_rgb(0xff, 0x60, 0x60);
const GREEN: Color32 = Color32::from_rgb(0x70, 0xe0, 0x70);
const BUTTON: [f32; 2] = [200.0, 32.0];
const LIST_WIDTH: f32 = 360.0;
const DETAIL_WIDTH: f32 = 360.0;

/// One mod in the list.
struct Row {
    id: String,
    title: String,
    description: String,
    authors: String,
    dependencies: String,
    bundled: bool,
    /// What happened when the game chose its mods.
    status: Status,
}

pub struct ModsScreen {
    rows: Vec<Row>,
    selected: Option<usize>,
    config: Config,
    /// Something changed since the game started.
    changed: bool,
}

impl Default for ModsScreen {
    fn default() -> Self {
        Self::new()
    }
}

impl ModsScreen {
    pub fn new() -> Self {
        let config = Config::load();
        let mut rows: Vec<Row> = mods::active()
            .catalog
            .iter()
            .map(|entry| {
                let manifest = entry.found.manifest.as_ref();
                Row {
                    id: entry.found.id.clone(),
                    title: manifest.map_or_else(|| entry.found.id.clone(), mods::title),
                    description: manifest.map(|m| m.description.clone()).unwrap_or_default(),
                    authors: manifest.map(|m| m.authors.join(", ")).unwrap_or_default(),
                    dependencies: manifest
                        .map(|m| m.dependencies.join(", "))
                        .unwrap_or_default(),
                    bundled: entry.found.bundled,
                    status: entry.status.clone(),
                }
            })
            .collect();
        let rank = |id: &str| {
            config
                .order
                .iter()
                .position(|o| o == id)
                .unwrap_or(usize::MAX)
        };
        rows.sort_by_key(|r| rank(&r.id));
        Self {
            rows,
            selected: None,
            config,
            changed: false,
        }
    }

    fn enabled(&self, row: &Row) -> bool {
        !matches!(row.status, Status::Failed(_)) && !self.config.disabled.contains(&row.id)
    }

    fn save(&mut self) {
        self.config.order = self.rows.iter().map(|r| r.id.clone()).collect();
        self.changed = true;
        if let Err(err) = self.config.save() {
            log::warn!("saving the mods list: {err:#}");
        }
    }

    fn toggle(&mut self, i: usize) {
        let id = self.rows[i].id.clone();
        if let Some(at) = self.config.disabled.iter().position(|d| *d == id) {
            self.config.disabled.remove(at);
        } else {
            self.config.disabled.push(id);
        }
        self.save();
    }

    fn swap(&mut self, a: usize, b: usize) {
        self.rows.swap(a, b);
        self.selected = Some(b);
        self.save();
    }

    /// Draws the screen; true when Done is clicked.
    pub fn show(&mut self, ui: &mut egui::Ui) -> bool {
        ui.add_space(16.0);
        ui.label(RichText::new("Mods").size(32.0).color(Color32::WHITE));
        ui.label(
            RichText::new(
                "Mods further up load first. Changes apply the next time Rustcraft starts.",
            )
            .color(GRAY),
        );
        ui.add_space(12.0);
        let height = (ui.available_height() - 110.0).max(120.0);
        let mut pick = None;
        let mut toggle = None;
        let mut swap = None;
        ui.horizontal_top(|ui| {
            ui.add_space(
                ((ui.available_width() - LIST_WIDTH - DETAIL_WIDTH - 16.0) / 2.0).max(0.0),
            );
            ui.vertical(|ui| {
                ui.set_width(LIST_WIDTH);
                egui::ScrollArea::vertical()
                    .id_salt("mods")
                    .max_height(height)
                    .show(ui, |ui| {
                        if self.rows.is_empty() {
                            ui.label(RichText::new("No mods found in the mods folder").color(GRAY));
                        }
                        for (i, row) in self.rows.iter().enumerate() {
                            let on = self.enabled(row);
                            ui.horizontal(|ui| {
                                let text = RichText::new(&row.title).color(if on {
                                    Color32::WHITE
                                } else {
                                    GRAY
                                });
                                if ui
                                    .add_sized(
                                        [210.0, 28.0],
                                        Button::selectable(self.selected == Some(i), text),
                                    )
                                    .clicked_with_sound()
                                {
                                    pick = Some(i);
                                }
                                let label = match (&row.status, on) {
                                    (Status::Failed(_), _) => "Failed",
                                    (_, true) => "On",
                                    (_, false) => "Off",
                                };
                                let can_toggle = !matches!(row.status, Status::Failed(_));
                                if ui
                                    .add_enabled(
                                        can_toggle,
                                        Button::new(label).min_size(vec2(56.0, 28.0)),
                                    )
                                    .clicked_with_sound()
                                {
                                    toggle = Some(i);
                                }
                                if ui
                                    .add_enabled(i > 0, Button::new("▲").min_size(vec2(28.0, 28.0)))
                                    .clicked_with_sound()
                                {
                                    swap = Some((i, i - 1));
                                }
                                if ui
                                    .add_enabled(
                                        i + 1 < self.rows.len(),
                                        Button::new("▼").min_size(vec2(28.0, 28.0)),
                                    )
                                    .clicked_with_sound()
                                {
                                    swap = Some((i, i + 1));
                                }
                            });
                        }
                    });
            });
            ui.add_space(16.0);
            ui.vertical(|ui| {
                ui.set_width(DETAIL_WIDTH);
                egui::ScrollArea::vertical()
                    .id_salt("details")
                    .max_height(height)
                    .show(ui, |ui| {
                        match self.selected.and_then(|i| self.rows.get(i)) {
                            Some(row) => self.details(ui, row),
                            None => {
                                ui.label(RichText::new("Pick a mod to read about it").color(GRAY));
                            }
                        }
                    });
            });
        });
        if let Some(i) = pick {
            self.selected = Some(i);
        }
        if let Some(i) = toggle {
            self.toggle(i);
        }
        if let Some((a, b)) = swap {
            self.swap(a, b);
        }
        ui.add_space(12.0);
        if self.changed {
            ui.label(
                RichText::new("Restart Rustcraft for the changes to take effect").color(GREEN),
            );
        }
        let mut done = false;
        ui.horizontal(|ui| {
            ui.add_space(((ui.available_width() - 2.0 * BUTTON[0] - 8.0) / 2.0).max(0.0));
            if ui
                .add(Button::new("Open Mods Folder").min_size(BUTTON.into()))
                .clicked_with_sound()
            {
                open_folder();
            }
            done = ui
                .add(Button::new("Done").min_size(BUTTON.into()))
                .clicked_with_sound();
        });
        done
    }

    fn details(&self, ui: &mut egui::Ui, row: &Row) {
        ui.label(RichText::new(&row.title).size(20.0).color(Color32::WHITE));
        ui.label(
            RichText::new(format!(
                "id: {}{}",
                row.id,
                if row.bundled {
                    " (comes with Rustcraft)"
                } else {
                    ""
                }
            ))
            .color(GRAY),
        );
        if !row.authors.is_empty() {
            ui.label(RichText::new(format!("by {}", row.authors)).color(GRAY));
        }
        if !row.dependencies.is_empty() {
            ui.label(RichText::new(format!("needs: {}", row.dependencies)).color(GRAY));
        }
        ui.add_space(6.0);
        ui.label(&row.description);
        if let Status::Failed(why) = &row.status {
            ui.add_space(6.0);
            ui.colored_label(RED, format!("Not loaded: {why}"));
        }
        let errors = mods::errors_of(&row.id);
        if !errors.is_empty() {
            ui.add_space(6.0);
            ui.colored_label(RED, "Errors:");
            for error in errors.iter().rev().take(8) {
                ui.colored_label(RED, error);
            }
        }
    }
}

/// Shows the mods folder in the Finder, making it if needed.
fn open_folder() {
    let dir = crate::level::saves_dir().with_file_name("mods");
    if let Err(err) = std::fs::create_dir_all(&dir) {
        return log::warn!("creating {}: {err}", dir.display());
    }
    if let Err(err) = std::process::Command::new("open").arg(&dir).spawn() {
        log::warn!("opening {}: {err}", dir.display());
    }
}
