//! The Mods screen (M39, redesigned in M66), from the title and pause
//! menus. One row per file in the mods folder, with its icon, name,
//! version, a line about it, an on/off switch and a status in plain words:
//! On, Off, Needs restart, or an error with what to do about it. Files
//! that aren't Lua mods say what they are: a resource pack offers "Move to
//! Resource Packs", a data pack "Add to world...", a Java mod or a shader
//! pack says plainly that it can't run. Files dropped on the window and
//! Add Files... take the same routes, after a confirmation.
//!
//! What is on is decided when the game starts, so turning a Lua mod on or
//! off, or adding one, needs a restart; one banner says so, with a Restart
//! now button.

use std::{
    path::{Path, PathBuf},
    sync::mpsc::{Receiver, channel},
};

use egui::{Button, Color32, RichText, TextureHandle, vec2};

use super::ClickSound;
use crate::{
    mods::{
        self, Config, Status,
        kind::FileKind,
        route::{self, Advice, Fix},
    },
    options::Options,
};

const GRAY: Color32 = Color32::from_rgb(0xa0, 0xa0, 0xa0);
const RED: Color32 = Color32::from_rgb(0xff, 0x60, 0x60);
const GREEN: Color32 = Color32::from_rgb(0x70, 0xe0, 0x70);
const AMBER: Color32 = Color32::from_rgb(0xff, 0xc8, 0x50);
const BUTTON: [f32; 2] = [200.0, 32.0];
const LIST_WIDTH: f32 = 440.0;
const DETAIL_WIDTH: f32 = 340.0;
const ICON: f32 = 36.0;

/// What the screen asks the app to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModsAction {
    Done,
    /// Save and start the game again.
    Restart,
}

/// A row's status, in the words the screen shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RowStatus {
    On,
    Off,
    NeedsRestart,
    /// Why it isn't working, in one sentence with what to do.
    Error(String),
    /// Not a Lua mod: the row says what it is instead.
    NotAMod,
}

impl RowStatus {
    fn label(&self) -> &'static str {
        match self {
            Self::On => "On",
            Self::Off => "Off",
            Self::NeedsRestart => "Needs restart",
            Self::Error(_) => "Error",
            Self::NotAMod => "Not a mod",
        }
    }

    fn color(&self) -> Color32 {
        match self {
            Self::On => GREEN,
            Self::Off => GRAY,
            Self::NeedsRestart => AMBER,
            Self::Error(_) => RED,
            Self::NotAMod => AMBER,
        }
    }
}

/// One file in the list.
struct Row {
    id: String,
    title: String,
    description: String,
    authors: String,
    dependencies: Vec<String>,
    bundled: bool,
    kind: FileKind,
    /// The file or folder (for moving, showing in the Finder, the folder
    /// line).
    path: PathBuf,
    status: RowStatus,
    /// What the file is and what can be done, for rows that aren't mods.
    advice: Option<Advice>,
    icon: Option<Vec<u8>>,
    texture: Option<TextureHandle>,
    /// The keys it adds, as "name (default)".
    keys: Vec<String>,
}

/// A file waiting for the player to confirm where it goes.
struct Pending {
    path: PathBuf,
    advice: Advice,
}

/// The folders the screen works in; the real ones in the game.
#[derive(Clone)]
struct Dirs {
    mods: PathBuf,
    /// Where mods are looked for (the mods folder and the bundled one).
    search: Vec<PathBuf>,
    bundled: Option<PathBuf>,
    packs: PathBuf,
    saves: PathBuf,
}

impl Dirs {
    fn real() -> Self {
        let search = mods::mod_dirs();
        Self {
            mods: search[0].clone(),
            bundled: search.last().cloned(),
            search,
            packs: crate::pack::packs_dir(),
            saves: crate::level::saves_dir(),
        }
    }
}

pub struct ModsScreen {
    rows: Vec<Row>,
    selected: Option<String>,
    config: Config,
    dirs: Dirs,
    /// Ids that were running when the game started, to tell which changes
    /// need a restart.
    started: Vec<(String, bool)>,
    /// Write `mods.ron` (off in tests).
    persist: bool,
    /// Something that changes which mods run was done.
    restart: bool,
    /// What the last action did (green) or why it failed (red).
    notice: Option<(String, bool)>,
    pending: Vec<Pending>,
    /// The file picker is open: its answer comes here.
    picking: Option<Receiver<Vec<PathBuf>>>,
    /// Choosing a world for a data pack.
    world_choice: Option<(PathBuf, Vec<(String, PathBuf)>)>,
}

impl Default for ModsScreen {
    fn default() -> Self {
        Self::new()
    }
}

impl ModsScreen {
    pub fn new() -> Self {
        let started = mods::active()
            .catalog
            .iter()
            .map(|e| (e.found.id.clone(), e.status == Status::Active))
            .collect();
        let mut screen = Self::build(Dirs::real(), Config::load(), started, true);
        if let Some(path) = shot_fixture() {
            screen.pending.clear();
            screen.add_shot_row(path);
        }
        screen
    }

    fn build(dirs: Dirs, config: Config, started: Vec<(String, bool)>, persist: bool) -> Self {
        let mut screen = Self {
            rows: Vec::new(),
            selected: None,
            config,
            dirs,
            started,
            persist,
            restart: false,
            notice: None,
            pending: Vec::new(),
            picking: None,
            world_choice: None,
        };
        screen.rescan();
        screen
    }

    /// Reads the folders again, keeping the order and what was selected.
    fn rescan(&mut self) {
        let found = mods::discover(&self.dirs.search, self.dirs.bundled.as_deref());
        let mut rows: Vec<Row> = found.iter().map(|f| self.row_of(f)).collect();
        let rank = |id: &str| {
            self.config
                .order
                .iter()
                .position(|o| o == id)
                .unwrap_or(usize::MAX)
        };
        rows.sort_by_key(|r| rank(&r.id));
        self.rows = rows;
        if self
            .selected
            .as_ref()
            .is_some_and(|id| !self.rows.iter().any(|r| &r.id == id))
        {
            self.selected = None;
        }
    }

    fn row_of(&self, found: &mods::Found) -> Row {
        let manifest = found.manifest.as_ref();
        let path = found.source.path().to_owned();
        let advice = (found.kind != FileKind::LuaMod).then(|| route::advise_as(&path, found.kind));
        let wanted = !self.config.disabled.contains(&found.id);
        let started = self.started.iter().find(|(id, _)| *id == found.id);
        let status = if advice.is_some() {
            RowStatus::NotAMod
        } else if let Some(problem) = &found.problem {
            RowStatus::Error(problem.clone())
        } else {
            match started {
                Some((_, true)) if wanted => RowStatus::On,
                Some((_, true)) => RowStatus::NeedsRestart,
                Some((_, false)) => {
                    // It didn't start: the loader said why, or it was off.
                    match mods::active()
                        .catalog
                        .iter()
                        .find(|e| e.found.id == found.id)
                        .map(|e| &e.status)
                    {
                        Some(Status::Failed(why)) => RowStatus::Error(route::plain_problem(why)),
                        _ if wanted => RowStatus::NeedsRestart,
                        _ => RowStatus::Off,
                    }
                }
                None if wanted => RowStatus::NeedsRestart,
                None => RowStatus::Off,
            }
        };
        let keys = mods::active()
            .keys
            .iter()
            .filter(|k| k.mod_id == found.id)
            .map(|k| format!("{} ({})", k.name, k.default))
            .collect();
        Row {
            id: found.id.clone(),
            title: manifest.map_or_else(|| route::pretty_name(&found.id), mods::title),
            description: manifest.map_or_else(String::new, |m| m.description.clone()),
            authors: manifest.map(|m| m.authors.join(", ")).unwrap_or_default(),
            dependencies: manifest.map(|m| m.dependencies.clone()).unwrap_or_default(),
            bundled: found.bundled,
            kind: found.kind,
            icon: found.source.read("pack.png"),
            path,
            status,
            advice,
            texture: None,
            keys,
        }
    }

    /// For the screenshot: a Fresh Animations file as if it sat in `mods/`.
    fn add_shot_row(&mut self, path: PathBuf) {
        let kind = crate::mods::kind::detect(&path);
        let found = mods::Found {
            id: route::display_name(&path),
            manifest: None,
            source: mods::Source::Dir(path.clone()),
            bundled: false,
            problem: None,
            kind,
        };
        let row = self.row_of(&found);
        self.rows.insert(0, row);
    }

    fn wanted_on(&self, row: &Row) -> bool {
        !self.config.disabled.contains(&row.id)
            && matches!(row.status, RowStatus::On | RowStatus::NeedsRestart)
    }

    fn save(&mut self) {
        self.config.order = self
            .rows
            .iter()
            .filter(|r| r.kind == FileKind::LuaMod)
            .map(|r| r.id.clone())
            .collect();
        if self.persist
            && let Err(err) = self.config.save()
        {
            self.fail(format!("Couldn't save the mods list: {err:#}"));
        }
    }

    fn toggle(&mut self, id: &str) {
        if let Some(at) = self.config.disabled.iter().position(|d| d == id) {
            self.config.disabled.remove(at);
        } else {
            self.config.disabled.push(id.to_owned());
        }
        self.restart = true;
        self.save();
        self.rescan();
    }

    fn swap(&mut self, a: usize, b: usize) {
        self.rows.swap(a, b);
        self.selected = Some(self.rows[b].id.clone());
        self.restart = true;
        self.save();
    }

    fn succeed(&mut self, text: String) {
        self.notice = Some((text, false));
    }

    fn fail(&mut self, text: String) {
        log::warn!("mods: {text}");
        self.notice = Some((text, true));
    }

    /// Whether a restart would change something.
    pub fn needs_restart(&self) -> bool {
        self.restart
            || self
                .rows
                .iter()
                .any(|r| r.status == RowStatus::NeedsRestart)
    }

    /// A file dropped on the window: ask where it goes.
    pub fn dropped(&mut self, path: PathBuf) {
        let advice = route::advise(&path);
        self.pending.push(Pending { path, advice });
    }

    /// Does what a confirmed file's `fix` says.
    fn apply(&mut self, path: &Path, fix: &Fix, options: &mut Options) {
        let name = route::display_name(path);
        // A file already in the mods folder moves; a download is copied.
        let inside = path.parent().is_some_and(|p| same_dir(p, &self.dirs.mods));
        match fix {
            Fix::Install => match route::install_lua_mod(path, &self.dirs.mods, inside) {
                Ok(()) => {
                    self.restart = true;
                    self.succeed(format!("{name} added. Restart to use it."));
                }
                Err(err) => self.fail(format!("Couldn't add {name}: {err:#}")),
            },
            Fix::MoveToResourcePacks => {
                match route::install_resource_pack(path, &self.dirs.packs, inside) {
                    Ok(id) => {
                        turn_on_pack(options, &id);
                        self.succeed(format!(
                            "{name} is in Resource Packs and turned on for the {} style.",
                            options.visuals.style.name()
                        ));
                    }
                    Err(err) => self.fail(format!("Couldn't move {name}: {err:#}")),
                }
            }
            Fix::AddToWorld => {
                let worlds = route::worlds_in(&self.dirs.saves);
                if worlds.is_empty() {
                    self.fail(format!(
                        "Make a world first, then add {name} to it: data packs go in a world."
                    ));
                } else {
                    self.world_choice = Some((path.to_owned(), worlds));
                }
            }
            Fix::ShowInFinder => show_in_finder(path),
        }
        self.rescan();
    }

    fn add_to_world(&mut self, path: &Path, world: &Path) {
        let name = route::display_name(path);
        match route::install_data_pack(path, world) {
            Ok(_) => {
                let world_name = world
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                self.succeed(format!(
                    "{name} added to {world_name}. Turn it on there with /datapack enable."
                ));
            }
            Err(err) => self.fail(format!("Couldn't add {name}: {err:#}")),
        }
    }

    /// Opens the Mac's file picker without holding up the frame.
    fn pick_files(&mut self) {
        let (send, receive) = channel();
        self.picking = Some(receive);
        std::thread::spawn(move || {
            let _ = send.send(choose_files());
        });
    }

    /// Draws the screen.
    pub fn show(&mut self, ui: &mut egui::Ui, options: &mut Options) -> Option<ModsAction> {
        if let Some(receive) = &self.picking
            && let Ok(files) = receive.try_recv()
        {
            self.picking = None;
            for file in files {
                self.dropped(file);
            }
        }
        if self.picking.is_some() {
            ui.ctx().request_repaint();
        }
        ui.add_space(16.0);
        ui.label(RichText::new("Mods").size(32.0).color(Color32::WHITE));
        let mut action = None;
        if self.needs_restart() {
            ui.horizontal(|ui| {
                ui.add_space(((ui.available_width() - 520.0) / 2.0).max(0.0));
                ui.colored_label(AMBER, "Changes need a restart to take effect.");
                if ui
                    .add(Button::new("Restart now").min_size(vec2(120.0, 28.0)))
                    .clicked_with_sound()
                {
                    action = Some(ModsAction::Restart);
                }
            });
        } else {
            ui.label(
                RichText::new("Mods further up load first. Drop a .zip on the window to add it.")
                    .color(GRAY),
            );
        }
        if let Some((text, bad)) = &self.notice {
            ui.colored_label(if *bad { RED } else { GREEN }, text);
        }
        if self.confirm(ui, options) {
            return action;
        }
        ui.add_space(8.0);
        let height = (ui.available_height() - 90.0).max(120.0);
        let mut pick = None;
        let mut toggle = None;
        let mut swap = None;
        let mut fix: Option<(PathBuf, Fix)> = None;
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
                            ui.add_space(24.0);
                            ui.label(
                                RichText::new(
                                    "No mods yet. Drop a mod's .zip here, or click Add Files.",
                                )
                                .color(GRAY),
                            );
                        }
                        let count = self.rows.len();
                        let wanted: Vec<bool> =
                            self.rows.iter().map(|r| self.wanted_on(r)).collect();
                        for (i, &on) in wanted.iter().enumerate() {
                            let selected = self.selected.as_ref() == Some(&self.rows[i].id);
                            let row = &mut self.rows[i];
                            draw_row(
                                ui,
                                row,
                                selected,
                                on,
                                (i > 0, i + 1 < count),
                                &mut Clicks {
                                    pick: &mut pick,
                                    toggle: &mut toggle,
                                    swap: &mut swap,
                                    fix: &mut fix,
                                },
                                i,
                            );
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
                        let row = self
                            .selected
                            .as_ref()
                            .and_then(|id| self.rows.iter().find(|r| &r.id == id));
                        match row {
                            Some(row) => self.details(ui, row),
                            None => {
                                ui.label(RichText::new("Pick a row to read about it").color(GRAY));
                            }
                        }
                    });
            });
        });
        if let Some(i) = pick {
            self.selected = Some(self.rows[i].id.clone());
        }
        if let Some(i) = toggle {
            let id = self.rows[i].id.clone();
            self.toggle(&id);
        }
        if let Some((a, b)) = swap {
            self.swap(a, b);
        }
        if let Some((path, fix)) = fix {
            self.apply(&path, &fix, options);
        }
        ui.add_space(12.0);
        let mut done = false;
        ui.horizontal(|ui| {
            ui.add_space(((ui.available_width() - 3.0 * BUTTON[0] - 16.0) / 2.0).max(0.0));
            if ui
                .add_enabled(
                    self.picking.is_none(),
                    Button::new("Add Files...").min_size(BUTTON.into()),
                )
                .clicked_with_sound()
            {
                self.pick_files();
            }
            if ui
                .add(Button::new("Open Mods Folder").min_size(BUTTON.into()))
                .clicked_with_sound()
            {
                open_folder(&self.dirs.mods);
            }
            done = ui
                .add(Button::new("Done").min_size(BUTTON.into()))
                .clicked_with_sound();
        });
        if done {
            return Some(ModsAction::Done);
        }
        action
    }

    /// The confirmation for the first waiting file, or the world choice.
    /// True while one is showing (the list waits).
    fn confirm(&mut self, ui: &mut egui::Ui, options: &mut Options) -> bool {
        if let Some((path, worlds)) = &self.world_choice {
            let path = path.clone();
            let worlds = worlds.clone();
            let mut chosen = None;
            let mut cancel = false;
            ui.add_space(12.0);
            ui.label(
                RichText::new(format!(
                    "Add {} to which world?",
                    route::display_name(&path)
                ))
                .color(Color32::WHITE),
            );
            egui::ScrollArea::vertical()
                .id_salt("world-choice")
                .max_height(ui.available_height() - 120.0)
                .show(ui, |ui| {
                    for (name, dir) in &worlds {
                        if ui
                            .add(Button::new(name).min_size(vec2(320.0, 28.0)))
                            .clicked_with_sound()
                        {
                            chosen = Some(dir.clone());
                        }
                    }
                });
            if ui
                .add(Button::new("Cancel").min_size(BUTTON.into()))
                .clicked_with_sound()
            {
                cancel = true;
            }
            if let Some(dir) = chosen {
                self.world_choice = None;
                self.add_to_world(&path, &dir);
            } else if cancel {
                self.world_choice = None;
            }
            return true;
        }
        let Some(first) = self.pending.first() else {
            return false;
        };
        let (path, advice) = (first.path.clone(), first.advice.clone());
        let more = self.pending.len() - 1;
        ui.add_space(12.0);
        egui::Frame::new()
            .fill(Color32::from_rgb(0x30, 0x2a, 0x26))
            .inner_margin(12.0)
            .show(ui, |ui| {
                ui.label(RichText::new(&advice.message).color(Color32::WHITE));
                if more > 0 {
                    ui.label(RichText::new(format!("{more} more waiting")).color(GRAY));
                }
                let (yes, can) = match advice.fix {
                    Fix::Install => ("Add to Mods", true),
                    Fix::MoveToResourcePacks => ("Add to Resource Packs and turn on", true),
                    Fix::AddToWorld => ("Add to world...", true),
                    Fix::ShowInFinder => ("Show in Finder", true),
                };
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(can, Button::new(yes).min_size(vec2(260.0, 30.0)))
                        .clicked_with_sound()
                    {
                        self.pending.remove(0);
                        // A dropped or picked file is copied, never moved.
                        self.apply_dropped(&path, &advice.fix, options);
                    }
                    let skip = if advice.fix == Fix::ShowInFinder {
                        "OK"
                    } else {
                        "Cancel"
                    };
                    if ui
                        .add(Button::new(skip).min_size(vec2(100.0, 30.0)))
                        .clicked_with_sound()
                    {
                        self.pending.remove(0);
                    }
                });
            });
        true
    }

    /// `apply` for a file from outside the mods folder.
    fn apply_dropped(&mut self, path: &Path, fix: &Fix, options: &mut Options) {
        self.apply(path, fix, options);
    }

    fn details(&self, ui: &mut egui::Ui, row: &Row) {
        ui.label(RichText::new(&row.title).size(20.0).color(Color32::WHITE));
        ui.label(
            RichText::new(format!(
                "id: {}{}",
                row.id,
                if row.bundled { " (Built in)" } else { "" }
            ))
            .color(GRAY),
        );
        ui.colored_label(row.status.color(), row.status.label());
        if let Some(advice) = &row.advice {
            ui.add_space(6.0);
            ui.label(&advice.message);
        }
        if !row.authors.is_empty() {
            ui.label(RichText::new(format!("by {}", row.authors)).color(GRAY));
        }
        if !row.description.is_empty() {
            ui.add_space(6.0);
            ui.label(&row.description);
        }
        if let RowStatus::Error(why) = &row.status {
            ui.add_space(6.0);
            ui.colored_label(RED, why);
        }
        if !row.dependencies.is_empty() {
            ui.add_space(6.0);
            ui.label(RichText::new("Needs:").color(GRAY));
            for dep in &row.dependencies {
                let met = self.rows.iter().any(|r| &r.id == dep && self.wanted_on(r));
                ui.colored_label(
                    if met { GREEN } else { RED },
                    format!(
                        "{dep}: {}",
                        if met {
                            "found and on"
                        } else {
                            "missing or off"
                        }
                    ),
                );
            }
        }
        if !row.keys.is_empty() {
            ui.add_space(6.0);
            ui.label(RichText::new("Adds these keys (Options, Controls):").color(GRAY));
            for key in &row.keys {
                ui.label(key);
            }
        }
        ui.add_space(6.0);
        ui.label(RichText::new(format!("Folder: {}", row.path.display())).color(GRAY));
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

/// What a row's buttons asked for this frame.
struct Clicks<'a> {
    pick: &'a mut Option<usize>,
    toggle: &'a mut Option<usize>,
    swap: &'a mut Option<(usize, usize)>,
    fix: &'a mut Option<(PathBuf, Fix)>,
}

fn draw_row(
    ui: &mut egui::Ui,
    row: &mut Row,
    selected: bool,
    on: bool,
    (can_up, can_down): (bool, bool),
    clicks: &mut Clicks,
    i: usize,
) {
    if row.texture.is_none()
        && let Some(bytes) = row.icon.take()
        && let Ok(image) = image::load_from_memory(&bytes)
    {
        row.texture = Some(crate::startup::load_egui(
            ui.ctx(),
            &format!("mod-{}", row.id),
            &image.into_rgba8(),
            egui::TextureOptions::NEAREST,
        ));
    }
    let frame = egui::Frame::new()
        .fill(if selected {
            Color32::from_rgb(0x3a, 0x34, 0x30)
        } else {
            Color32::TRANSPARENT
        })
        .inner_margin(6.0);
    frame.show(ui, |ui| {
        ui.set_width(LIST_WIDTH - 12.0);
        ui.horizontal(|ui| {
            match &row.texture {
                Some(texture) => {
                    ui.image((texture.id(), vec2(ICON, ICON)));
                }
                None => default_icon(ui, &row.title),
            }
            ui.vertical(|ui| {
                ui.set_width(LIST_WIDTH - ICON - 40.0);
                ui.horizontal(|ui| {
                    let name = RichText::new(&row.title).color(if on || row.advice.is_some() {
                        Color32::WHITE
                    } else {
                        GRAY
                    });
                    if ui
                        .add(Button::selectable(selected, name).frame(false))
                        .clicked_with_sound()
                    {
                        *clicks.pick = Some(i);
                    }
                    if row.bundled {
                        ui.label(RichText::new("Built in").small().color(GRAY));
                    }
                });
                let line = match (&row.advice, &row.status) {
                    (Some(advice), _) => advice.message.clone(),
                    (None, RowStatus::Error(why)) => why.clone(),
                    _ => row.description.clone(),
                };
                ui.add(
                    egui::Label::new(RichText::new(line).small().color(
                        if matches!(row.status, RowStatus::Error(_)) {
                            RED
                        } else {
                            GRAY
                        },
                    ))
                    .truncate(),
                );
                ui.horizontal(|ui| {
                    if let Some(advice) = &row.advice {
                        let label = match advice.fix {
                            Fix::MoveToResourcePacks => Some("Move to Resource Packs"),
                            Fix::AddToWorld => Some("Add to world..."),
                            Fix::ShowInFinder => Some("Show in Finder"),
                            Fix::Install => None,
                        };
                        if let Some(label) = label
                            && ui
                                .add(Button::new(label).min_size(vec2(0.0, 24.0)))
                                .clicked_with_sound()
                        {
                            *clicks.fix = Some((row.path.clone(), advice.fix.clone()));
                        }
                    } else {
                        let can_toggle = !matches!(row.status, RowStatus::Error(_));
                        let text = if on { "Turn off" } else { "Turn on" };
                        if ui
                            .add_enabled(can_toggle, Button::new(text).min_size(vec2(80.0, 24.0)))
                            .clicked_with_sound()
                        {
                            *clicks.toggle = Some(i);
                        }
                        if ui
                            .add_enabled(can_up, Button::new("▲").min_size(vec2(24.0, 24.0)))
                            .clicked_with_sound()
                        {
                            *clicks.swap = Some((i, i - 1));
                        }
                        if ui
                            .add_enabled(can_down, Button::new("▼").min_size(vec2(24.0, 24.0)))
                            .clicked_with_sound()
                        {
                            *clicks.swap = Some((i, i + 1));
                        }
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.colored_label(row.status.color(), row.status.label());
                    });
                });
            });
        });
    });
}

/// The icon of a row without a pack.png: a square with its first letter.
fn default_icon(ui: &mut egui::Ui, title: &str) {
    let (rect, _) = ui.allocate_exact_size(vec2(ICON, ICON), egui::Sense::hover());
    ui.painter()
        .rect_filled(rect, 4.0, Color32::from_rgb(0x4a, 0x42, 0x3c));
    let letter = title
        .chars()
        .next()
        .map_or_else(String::new, |c| c.to_uppercase().to_string());
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        letter,
        egui::FontId::proportional(20.0),
        Color32::WHITE,
    );
}

fn same_dir(a: &Path, b: &Path) -> bool {
    a.canonicalize()
        .ok()
        .zip(b.canonicalize().ok())
        .is_some_and(|(a, b)| a == b)
}

/// Turns the resource pack `id` on at the top of this style's list.
pub fn turn_on_pack(options: &mut Options, id: &str) {
    options.resource_packs.retain(|p| p != id);
    options.resource_packs.insert(0, id.to_owned());
    options.store_packs();
}

/// Shows the mods folder in the Finder, making it if needed.
fn open_folder(dir: &Path) {
    if let Err(err) = std::fs::create_dir_all(dir) {
        return log::warn!("creating {}: {err}", dir.display());
    }
    if let Err(err) = std::process::Command::new("open").arg(dir).spawn() {
        log::warn!("opening {}: {err}", dir.display());
    }
}

/// Shows a file selected in the Finder.
fn show_in_finder(path: &Path) {
    if let Err(err) = std::process::Command::new("open")
        .arg("-R")
        .arg(path)
        .spawn()
    {
        log::warn!("showing {}: {err}", path.display());
    }
}

/// The Mac's file picker, many files; empty when cancelled.
fn choose_files() -> Vec<PathBuf> {
    let output = std::process::Command::new("osascript")
        .args([
            "-e",
            "set chosen to choose file with prompt \"Add mods, resource packs or data packs\" with multiple selections allowed",
            "-e",
            "set out to \"\"",
            "-e",
            "repeat with f in chosen",
            "-e",
            "set out to out & POSIX path of f & linefeed",
            "-e",
            "end repeat",
            "-e",
            "return out",
        ])
        .output();
    match output {
        Ok(out) if out.status.success() => String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| PathBuf::from(l.trim_end_matches('/')))
            .collect(),
        _ => Vec::new(),
    }
}

/// The `--shot mods-screen` scene shows Fresh Animations as a download
/// sitting in `mods/`; the file itself is never touched.
fn shot_fixture() -> Option<PathBuf> {
    if !std::env::args().any(|a| a == "mods-screen") {
        return None;
    }
    let dir = crate::pack::builtin_dir();
    crate::pack::find_by_words(&dir, &["freshanimations"], &["barebones"]).map(|n| dir.join(n))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{pack::tests::write_zip, world::save::TempDir};

    fn zip_at(dir: &Path, name: &str, files: &[&str]) -> PathBuf {
        std::fs::create_dir_all(dir).unwrap();
        let path = dir.join(name);
        let files: Vec<(&str, &[u8])> = files
            .iter()
            .map(|f| (*f, crate::pack::tests::MCMETA))
            .collect();
        write_zip(&path, &files);
        path
    }

    fn screen(root: &Path) -> ModsScreen {
        let dirs = Dirs {
            mods: root.join("mods"),
            search: vec![root.join("mods")],
            bundled: None,
            packs: root.join("resourcepacks"),
            saves: root.join("saves"),
        };
        std::fs::create_dir_all(&dirs.mods).unwrap();
        ModsScreen::build(dirs, Config::default(), Vec::new(), false)
    }

    fn lua_mod(root: &Path, id: &str, ron: &str) {
        let dir = root.join("mods").join(id);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("mod.ron"), ron).unwrap();
    }

    fn row<'a>(screen: &'a ModsScreen, id: &str) -> &'a Row {
        screen.rows.iter().find(|r| r.id == id).expect(id)
    }

    #[test]
    fn a_resource_pack_in_mods_is_not_a_mod_and_moves_and_turns_on() {
        let root = TempDir::new("mods-move");
        let fa = zip_at(
            &root.path().join("mods"),
            "Fresh Animations.zip",
            &["pack.mcmeta", "assets/minecraft/a.png"],
        );
        let mut screen = screen(root.path());
        let r = row(&screen, "Fresh Animations");
        assert_eq!(r.status, RowStatus::NotAMod);
        assert_eq!(
            r.advice.as_ref().unwrap().message,
            "Fresh Animations is a resource pack, not a mod"
        );
        assert_eq!(r.advice.as_ref().unwrap().fix, Fix::MoveToResourcePacks);
        let mut options = Options::default();
        let style = options.visuals.style;
        screen.apply(&fa, &Fix::MoveToResourcePacks, &mut options);
        assert!(!fa.exists());
        assert!(
            root.path()
                .join("resourcepacks/Fresh Animations.zip")
                .is_file()
        );
        // On for the current style, so the app rebuilds the look at once.
        assert_eq!(options.resource_packs, ["Fresh Animations.zip"]);
        assert_eq!(options.visuals.style, style);
        assert_eq!(
            options.visuals.packs(style),
            &vec!["Fresh Animations.zip".to_owned()]
        );
        assert!(screen.rows.iter().all(|r| r.id != "Fresh Animations"));
        assert!(!screen.notice.as_ref().unwrap().1);
    }

    #[test]
    fn java_mods_and_shader_packs_say_so_plainly() {
        let root = TempDir::new("mods-java");
        let mods = root.path().join("mods");
        zip_at(&mods, "wakes-0.7.3.jar", &["fabric.mod.json"]);
        zip_at(&mods, "sodium.jar", &["fabric.mod.json"]);
        zip_at(&mods, "bsl.zip", &["shaders/final.fsh"]);
        let screen = screen(root.path());
        assert!(
            row(&screen, "wakes-0.7.3")
                .advice
                .as_ref()
                .unwrap()
                .message
                .contains("built in: see Barebones Extras")
        );
        assert!(
            row(&screen, "sodium")
                .advice
                .as_ref()
                .unwrap()
                .message
                .contains("can't run Java mods")
        );
        let shader = row(&screen, "bsl");
        assert!(
            shader
                .advice
                .as_ref()
                .unwrap()
                .message
                .contains("not supported")
        );
        assert_eq!(shader.advice.as_ref().unwrap().fix, Fix::ShowInFinder);
    }

    #[test]
    fn a_broken_mod_ron_shows_the_plain_error() {
        let root = TempDir::new("mods-broken");
        lua_mod(root.path(), "ruby", r#"(id: "ruby", name: "Ruby")"#);
        let screen = screen(root.path());
        assert_eq!(
            row(&screen, "ruby").status,
            RowStatus::Error("mod.ron is missing a version line. Add it, then restart.".into())
        );
    }

    #[test]
    fn the_restart_banner_shows_only_when_needed() {
        let root = TempDir::new("mods-restart");
        lua_mod(
            root.path(),
            "ruby",
            r#"(id: "ruby", name: "Ruby", version: "1.0")"#,
        );
        let started = vec![("ruby".to_owned(), true)];
        let dirs = Dirs {
            mods: root.path().join("mods"),
            search: vec![root.path().join("mods")],
            bundled: None,
            packs: root.path().join("resourcepacks"),
            saves: root.path().join("saves"),
        };
        let mut screen = ModsScreen::build(dirs, Config::default(), started, false);
        assert_eq!(row(&screen, "ruby").status, RowStatus::On);
        assert!(!screen.needs_restart());
        screen.toggle("ruby");
        assert_eq!(row(&screen, "ruby").status, RowStatus::NeedsRestart);
        assert!(screen.needs_restart());
        screen.toggle("ruby");
        assert_eq!(row(&screen, "ruby").status, RowStatus::On);
        // Back to how it started, but the order was written: a restart
        // would change nothing the player can see, yet the flag stays
        // until the game restarts, which is the safe side.
        // A resource pack move doesn't need one.
        let fa = zip_at(
            &root.path().join("mods"),
            "FA.zip",
            &["pack.mcmeta", "assets/minecraft/a.png"],
        );
        let mut fresh = ModsScreen::build(
            screen.dirs.clone(),
            Config::default(),
            vec![("ruby".to_owned(), true)],
            false,
        );
        fresh.apply(&fa, &Fix::MoveToResourcePacks, &mut Options::default());
        assert!(!fresh.needs_restart());
    }

    #[test]
    fn a_new_lua_mod_needs_a_restart() {
        let root = TempDir::new("mods-new");
        let mut screen = screen(root.path());
        let src_dir = TempDir::new("mods-download");
        let zip = zip_at(src_dir.path(), "cool.zip", &["mod.ron", "main.lua"]);
        // The test mod.ron is not valid RON, but the file is a Lua mod.
        assert_eq!(route::advise(&zip).fix, Fix::Install);
        screen.apply(&zip, &Fix::Install, &mut Options::default());
        assert!(zip.exists(), "a download is copied, not moved");
        assert!(root.path().join("mods/cool.zip").is_file());
        assert!(screen.needs_restart());
    }

    #[test]
    fn dropped_files_wait_for_confirmation_and_then_route() {
        let root = TempDir::new("mods-drop");
        let mut screen = screen(root.path());
        let downloads = TempDir::new("mods-drop-src");
        let fa = zip_at(
            downloads.path(),
            "FreshAnimations_v1.zip",
            &["pack.mcmeta", "assets/minecraft/a.png"],
        );
        screen.dropped(fa.clone());
        assert_eq!(screen.pending.len(), 1);
        assert_eq!(screen.pending[0].advice.fix, Fix::MoveToResourcePacks);
        assert!(
            root.path().join("resourcepacks").read_dir().is_err(),
            "nothing happens before the player confirms"
        );
        let mut options = Options::default();
        let pending = screen.pending.remove(0);
        screen.apply(&pending.path, &pending.advice.fix, &mut options);
        assert!(fa.exists(), "a drop copies");
        assert!(
            root.path()
                .join("resourcepacks/FreshAnimations_v1.zip")
                .is_file()
        );
        assert_eq!(options.resource_packs, ["FreshAnimations_v1.zip"]);
    }

    #[test]
    fn a_data_pack_goes_into_the_chosen_world() {
        let root = TempDir::new("mods-data");
        let world = root.path().join("saves/Home");
        std::fs::create_dir_all(&world).unwrap();
        std::fs::write(world.join("level.ron"), "()").unwrap();
        let mut screen = screen(root.path());
        let pack = zip_at(
            &root.path().join("mods"),
            "tweaks.zip",
            &["pack.mcmeta", "data/x/y.json"],
        );
        let mut options = Options::default();
        screen.apply(&pack, &Fix::AddToWorld, &mut options);
        let (_, worlds) = screen.world_choice.clone().expect("asks which world");
        assert_eq!(worlds.len(), 1);
        screen.add_to_world(&pack, &worlds[0].1);
        assert!(world.join("datapacks/tweaks/pack.mcmeta").is_file());
    }

    #[test]
    fn the_real_fresh_animations_file_gets_a_move_button() {
        let dir = crate::pack::builtin_dir();
        let Some(name) = crate::pack::find_by_words(&dir, &["freshanimations"], &["barebones"])
        else {
            eprintln!("Fresh Animations isn't in assets/downloads; skipped");
            return;
        };
        let root = TempDir::new("mods-real-fa");
        std::fs::create_dir_all(root.path().join("mods")).unwrap();
        std::fs::copy(dir.join(&name), root.path().join("mods").join(&name)).unwrap();
        let screen = screen(root.path());
        let r = screen.rows.first().unwrap();
        assert_eq!(r.advice.as_ref().unwrap().fix, Fix::MoveToResourcePacks);
        assert!(
            r.advice
                .as_ref()
                .unwrap()
                .message
                .ends_with("is a resource pack, not a mod")
        );
    }
}
