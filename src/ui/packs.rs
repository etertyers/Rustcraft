//! The Resource Packs screen, after modern Minecraft's: the packs in the
//! `resourcepacks/` folder on the left, the selected ones on the right
//! with the highest priority on top and the defaults always at the bottom.
//! Done applies the selection; the game reloads its textures in place.

use std::{
    path::PathBuf,
    time::{Duration, Instant, SystemTime},
};

use super::ClickSound;
use egui::{Button, Color32, RichText, TextureHandle, vec2};
use log::warn;

use crate::pack::{self, Compatibility, Pack};

const GRAY: Color32 = Color32::from_rgb(0xa0, 0xa0, 0xa0);
const WARNING: Color32 = Color32::from_rgb(0xff, 0x80, 0x80);
const BUTTON: [f32; 2] = [200.0, 32.0];
const COLUMN_WIDTH: f32 = 330.0;
const ICON_SIZE: f32 = 32.0;
/// How often the folder is checked for added or removed packs.
const RESCAN_EVERY: Duration = Duration::from_secs(1);

/// One pack in a list.
struct Entry {
    id: String,
    name: String,
    description: String,
    compatibility: Compatibility,
    /// `pack.png`, decoded when first drawn.
    icon: Option<Vec<u8>>,
    texture: Option<TextureHandle>,
}

impl Entry {
    fn of(pack: Pack) -> Self {
        Self {
            name: pack.name().to_owned(),
            description: pack.info.description.clone(),
            compatibility: pack.info.compatibility(),
            icon: pack.icon,
            id: pack.id,
            texture: None,
        }
    }
}

pub struct PackScreen {
    dir: PathBuf,
    available: Vec<Entry>,
    /// Highest priority first.
    selected: Vec<Entry>,
    /// The folder's modification time when last read, and when it was checked.
    scanned: Option<SystemTime>,
    checked: Instant,
}

impl PackScreen {
    /// Lists the packs in `dir`, with `enabled` (highest priority first)
    /// selected. Selected packs that are gone are dropped.
    pub fn new(dir: PathBuf, enabled: &[String]) -> Self {
        let mut screen = Self {
            dir,
            available: Vec::new(),
            selected: Vec::new(),
            scanned: None,
            checked: Instant::now(),
        };
        screen.rescan(enabled);
        screen
    }

    /// Reads the folder again, keeping the current selection and order.
    fn rescan(&mut self, enabled: &[String]) {
        self.scanned = modified(&self.dir);
        let mut packs: Vec<Entry> = pack::list(&self.dir).into_iter().map(Entry::of).collect();
        self.selected = enabled
            .iter()
            .filter_map(|id| {
                let at = packs.iter().position(|p| &p.id == id)?;
                Some(packs.remove(at))
            })
            .collect();
        self.available = packs;
    }

    /// The selected packs' ids, highest priority first.
    pub fn selection(&self) -> Vec<String> {
        self.selected.iter().map(|p| p.id.clone()).collect()
    }

    /// Moves available pack `i` to the top of the selection.
    fn select(&mut self, i: usize) {
        if i < self.available.len() {
            let entry = self.available.remove(i);
            self.selected.insert(0, entry);
        }
    }

    /// Moves selected pack `i` back to the available list, in name order.
    fn deselect(&mut self, i: usize) {
        if i < self.selected.len() {
            let entry = self.selected.remove(i);
            let at = self
                .available
                .partition_point(|p| p.name.to_lowercase() < entry.name.to_lowercase());
            self.available.insert(at, entry);
        }
    }

    /// Raises selected pack `i` one place in priority.
    fn raise(&mut self, i: usize) {
        if i > 0 && i < self.selected.len() {
            self.selected.swap(i - 1, i);
        }
    }

    /// Lowers selected pack `i` one place in priority.
    fn lower(&mut self, i: usize) {
        if i + 1 < self.selected.len() {
            self.selected.swap(i, i + 1);
        }
    }

    /// Draws the screen. Returns the selection when Done is clicked.
    pub fn show(&mut self, ui: &mut egui::Ui) -> Option<Vec<String>> {
        if self.checked.elapsed() >= RESCAN_EVERY {
            self.checked = Instant::now();
            if modified(&self.dir) != self.scanned {
                let selection = self.selection();
                self.rescan(&selection);
            }
        }
        ui.add_space(16.0);
        ui.label(
            RichText::new("Select Resource Packs")
                .size(28.0)
                .color(Color32::WHITE),
        );
        ui.label(RichText::new("Packs on top override the ones below them").color(GRAY));
        ui.add_space(12.0);
        let list_height = (ui.available_height() - 110.0).max(120.0);
        let mut click = None;
        ui.horizontal_top(|ui| {
            ui.add_space(((ui.available_width() - 2.0 * COLUMN_WIDTH - 16.0) / 2.0).max(0.0));
            ui.vertical(|ui| {
                ui.set_width(COLUMN_WIDTH);
                ui.label(RichText::new("Available").strong().color(Color32::WHITE));
                egui::ScrollArea::vertical()
                    .id_salt("available")
                    .max_height(list_height)
                    .show(ui, |ui| {
                        if self.available.is_empty() {
                            ui.label(
                                RichText::new("Put packs in the resource pack folder").color(GRAY),
                            );
                        }
                        for (i, entry) in self.available.iter_mut().enumerate() {
                            row(ui, entry, &[("Add ▶", Click::Select(i))], &mut click);
                        }
                    });
            });
            ui.add_space(16.0);
            ui.vertical(|ui| {
                ui.set_width(COLUMN_WIDTH);
                ui.label(RichText::new("Selected").strong().color(Color32::WHITE));
                egui::ScrollArea::vertical()
                    .id_salt("selected")
                    .max_height(list_height)
                    .show(ui, |ui| {
                        let last = self.selected.len().saturating_sub(1);
                        for (i, entry) in self.selected.iter_mut().enumerate() {
                            let mut buttons = vec![("◀ Remove", Click::Deselect(i))];
                            if i > 0 {
                                buttons.push(("▲", Click::Raise(i)));
                            }
                            if i < last {
                                buttons.push(("▼", Click::Lower(i)));
                            }
                            row(ui, entry, &buttons, &mut click);
                        }
                        ui.add_space(4.0);
                        ui.label(RichText::new("Default").color(Color32::WHITE));
                        ui.label(RichText::new("The default look of Rustcraft").color(GRAY));
                    });
            });
        });
        match click {
            Some(Click::Select(i)) => self.select(i),
            Some(Click::Deselect(i)) => self.deselect(i),
            Some(Click::Raise(i)) => self.raise(i),
            Some(Click::Lower(i)) => self.lower(i),
            None => {}
        }
        ui.add_space(12.0);
        let mut done = false;
        ui.horizontal(|ui| {
            ui.add_space(((ui.available_width() - 2.0 * BUTTON[0] - 8.0) / 2.0).max(0.0));
            if ui
                .add(Button::new("Open Pack Folder").min_size(BUTTON.into()))
                .clicked_with_sound()
            {
                self.open_folder();
            }
            done = ui
                .add(Button::new("Done").min_size(BUTTON.into()))
                .clicked_with_sound();
        });
        done.then(|| self.selection())
    }

    /// Shows the packs folder in the Finder, making it if needed.
    fn open_folder(&self) {
        if let Err(err) = std::fs::create_dir_all(&self.dir) {
            return warn!("creating {}: {err}", self.dir.display());
        }
        if let Err(err) = std::process::Command::new("open").arg(&self.dir).spawn() {
            warn!("opening {}: {err}", self.dir.display());
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum Click {
    Select(usize),
    Deselect(usize),
    Raise(usize),
    Lower(usize),
}

/// One pack's row: icon, name, description, a warning if it was made for
/// another version, and its buttons.
fn row(ui: &mut egui::Ui, entry: &mut Entry, buttons: &[(&str, Click)], click: &mut Option<Click>) {
    if entry.texture.is_none()
        && let Some(bytes) = entry.icon.take()
    {
        match image::load_from_memory(&bytes) {
            Ok(image) => {
                let image = image.into_rgba8();
                let size = [image.width() as usize, image.height() as usize];
                let pixels = egui::ColorImage::from_rgba_unmultiplied(size, image.as_raw());
                entry.texture = Some(ui.ctx().load_texture(
                    format!("pack-{}", entry.id),
                    pixels,
                    egui::TextureOptions::NEAREST,
                ));
            }
            Err(err) => warn!("pack.png of {}: {err}", entry.name),
        }
    }
    ui.horizontal(|ui| {
        match &entry.texture {
            Some(texture) => {
                ui.image((texture.id(), vec2(ICON_SIZE, ICON_SIZE)));
            }
            None => {
                ui.add_space(ICON_SIZE + ui.spacing().item_spacing.x);
            }
        }
        ui.vertical(|ui| {
            ui.label(RichText::new(&entry.name).color(Color32::WHITE));
            let first_line = entry.description.lines().next().unwrap_or_default();
            ui.label(RichText::new(first_line).small().color(GRAY));
            match entry.compatibility {
                Compatibility::Ok => {}
                Compatibility::Older => {
                    ui.label(
                        RichText::new("Made for an older version")
                            .small()
                            .color(WARNING),
                    );
                }
                Compatibility::Newer => {
                    ui.label(
                        RichText::new("Made for a newer version")
                            .small()
                            .color(WARNING),
                    );
                }
            }
            ui.horizontal(|ui| {
                for &(text, action) in buttons {
                    if ui.small_button(text).clicked_with_sound() {
                        *click = Some(action);
                    }
                }
            });
        });
    });
    ui.separator();
}

/// When `dir` last changed, if it exists.
fn modified(dir: &std::path::Path) -> Option<SystemTime> {
    std::fs::metadata(dir).and_then(|m| m.modified()).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{pack::tests::*, world::save::TempDir};

    fn screen_with(names: &[&str], enabled: &[&str]) -> (TempDir, PackScreen) {
        let dir = TempDir::new("pack-screen");
        for name in names {
            write_zip(&dir.path().join(name), &[("pack.mcmeta", MCMETA)]);
        }
        let enabled: Vec<String> = enabled.iter().map(|s| s.to_string()).collect();
        let screen = PackScreen::new(dir.path().to_owned(), &enabled);
        (dir, screen)
    }

    fn names(entries: &[Entry]) -> Vec<&str> {
        entries.iter().map(|e| e.name.as_str()).collect()
    }

    #[test]
    fn opens_with_the_saved_selection_in_order() {
        let (_dir, screen) = screen_with(
            &["a.zip", "b.zip", "c.zip"],
            &["c.zip", "gone.zip", "a.zip"],
        );
        assert_eq!(screen.selection(), ["c.zip", "a.zip"]);
        assert_eq!(names(&screen.available), ["b"]);
    }

    #[test]
    fn selecting_reordering_and_removing_packs() {
        let (_dir, mut screen) = screen_with(&["a.zip", "b.zip", "c.zip"], &[]);
        assert_eq!(names(&screen.available), ["a", "b", "c"]);
        // New picks go on top.
        screen.select(2);
        screen.select(0);
        assert_eq!(screen.selection(), ["a.zip", "c.zip"]);
        screen.lower(0);
        assert_eq!(screen.selection(), ["c.zip", "a.zip"]);
        screen.raise(1);
        assert_eq!(screen.selection(), ["a.zip", "c.zip"]);
        // Out-of-range moves do nothing.
        screen.raise(0);
        screen.lower(1);
        screen.select(9);
        assert_eq!(screen.selection(), ["a.zip", "c.zip"]);
        // Removed packs go back in name order.
        screen.deselect(0);
        assert_eq!(names(&screen.available), ["a", "b"]);
        assert_eq!(screen.selection(), ["c.zip"]);
    }

    #[test]
    fn rescanning_keeps_the_selection() {
        let (dir, mut screen) = screen_with(&["a.zip", "b.zip"], &["b.zip"]);
        write_zip(&dir.path().join("new.zip"), &[("pack.mcmeta", MCMETA)]);
        std::fs::remove_file(dir.path().join("a.zip")).unwrap();
        let selection = screen.selection();
        screen.rescan(&selection);
        assert_eq!(screen.selection(), ["b.zip"]);
        assert_eq!(names(&screen.available), ["new"]);
    }
}
