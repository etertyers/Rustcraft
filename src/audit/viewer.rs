//! The Texture Viewer (M60): Kenzo's quick check. Categories and a search
//! box on the left, a grid of thumbnails in the middle (a red corner on
//! what a detector flagged, a check or cross on what Kenzo marked), and
//! the selected one large on the right with its raw texture and UV boxes.
//! Arrow keys move, Enter enlarges, B marks broken (with a note), K marks
//! OK, U clears, F jumps to the next flagged one, Esc closes. Marks are
//! saved at once to `texture-marks.ron`.

use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, mpsc},
    time::Instant,
};

use egui::{Color32, Key as EKey, RichText, TextureHandle, TextureOptions, vec2};
use image::RgbaImage;

use super::{
    Anim, Category, Entry, Subject,
    marks::{self, Marks, Verdict},
    raster::View,
    sheet::THUMB,
};
use crate::{entity::mobs::MobRegistry, look::Look};

/// The keys the viewer reacts to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Left,
    Right,
    Up,
    Down,
    Enter,
    B,
    K,
    U,
    F,
    Esc,
}

/// What the keys act on: the visible entries, the selection and the marks.
/// No drawing, so the keys can be tested.
pub struct Model {
    /// The visible entries' ids and whether a detector flagged each.
    pub items: Vec<(String, bool)>,
    pub selected: usize,
    pub columns: usize,
    pub marks: Marks,
    path: Option<PathBuf>,
    /// The texture set marks are made in.
    pub set: String,
    /// The note being typed for a broken mark.
    pub note: Option<String>,
    pub large: bool,
    pub closed: bool,
}

impl Model {
    pub fn new(set: &str, marks: Marks, path: Option<PathBuf>) -> Self {
        Self {
            items: Vec::new(),
            selected: 0,
            columns: 8,
            marks,
            path,
            set: set.to_owned(),
            note: None,
            large: false,
            closed: false,
        }
    }

    /// Where the marks are saved (tests use a temp file).
    pub fn set_path(&mut self, path: PathBuf) {
        // The marks are the new file's, not the player's.
        self.marks = Marks::load(&path);
        self.path = Some(path);
    }

    pub fn current(&self) -> Option<&str> {
        self.items.get(self.selected).map(|(id, _)| id.as_str())
    }

    fn save(&self) {
        if let Some(path) = &self.path
            && let Err(err) = self.marks.save(path)
        {
            log::warn!("can't save {}: {err}", path.display());
        }
    }

    fn mark(&mut self, verdict: Verdict, note: &str) {
        if let Some(id) = self.current().map(str::to_owned) {
            self.marks
                .mark(&id, &self.set, verdict, note, &marks::today());
            self.save();
        }
    }

    /// Saves the typed note as a broken mark.
    pub fn commit_note(&mut self) {
        if let Some(note) = self.note.take() {
            self.mark(Verdict::Broken, &note);
        }
    }

    /// "N broken, M OK, K unchecked" over the visible entries' set.
    pub fn status(&self, total: usize) -> String {
        let (broken, ok) = self.marks.counts(&self.set);
        format!(
            "{broken} broken, {ok} OK, {} unchecked",
            total.saturating_sub(broken + ok)
        )
    }

    pub fn key(&mut self, key: Key) {
        if self.note.is_some() {
            match key {
                Key::Enter => self.commit_note(),
                Key::Esc => self.note = None,
                _ => {}
            }
            return;
        }
        let n = self.items.len();
        let cols = self.columns.max(1);
        match key {
            Key::Left => self.selected = self.selected.saturating_sub(1),
            Key::Right => self.selected = (self.selected + 1).min(n.saturating_sub(1)),
            Key::Up => self.selected = self.selected.saturating_sub(cols),
            Key::Down => self.selected = (self.selected + cols).min(n.saturating_sub(1)),
            Key::Enter => self.large = !self.large,
            Key::B => {
                if self.current().is_some() {
                    let old = self
                        .current()
                        .and_then(|id| self.marks.get(id, &self.set))
                        .map(|m| m.note.clone())
                        .unwrap_or_default();
                    self.note = Some(old);
                }
            }
            Key::K => self.mark(Verdict::Ok, ""),
            Key::U => {
                if let Some(id) = self.current().map(str::to_owned) {
                    self.marks.clear(&id, &self.set);
                    self.save();
                }
            }
            Key::F => {
                let start = self.selected + 1;
                if let Some(at) = (0..n)
                    .map(|i| (start + i) % n)
                    .find(|&i| self.items[i].1 && i != self.selected)
                {
                    self.selected = at;
                }
            }
            Key::Esc => {
                if self.large {
                    self.large = false;
                } else {
                    self.closed = true;
                }
            }
        }
    }
}

/// What the background thread built for a texture set.
pub struct Loaded {
    pub look: Look,
    pub mobs: MobRegistry,
    pub entries: Vec<Entry>,
}

enum Loading {
    Busy(mpsc::Receiver<Result<Loaded, String>>, Instant),
    Ready(Arc<Loaded>),
    Failed(String),
}

fn load_in_background(set: &str) -> Loading {
    let (tx, rx) = mpsc::channel();
    let set = set.to_owned();
    std::thread::spawn(move || {
        let result = (|| -> anyhow::Result<Loaded> {
            let assets = crate::assets::dir();
            let stack = super::stack(&set, &assets.join("downloads/resourcepacks"))
                .ok_or_else(|| anyhow::anyhow!("no texture set {set:?}"))?;
            let mobs = MobRegistry::load(&assets.join("data/mobs.ron"))?;
            let look = super::build_look(&assets, &stack, &mobs)?;
            let (blocks, items, rest) = std::thread::scope(|scope| {
                let blocks = scope.spawn(|| super::block_entries(&look, None));
                let items = scope.spawn(|| super::item_entries(&look));
                let rest = {
                    let mut v = super::mob_entries(&look, &mobs);
                    for (category, folders) in [
                        (Category::Gui, &["gui"][..]),
                        (Category::Particles, &["particle"]),
                        (Category::Environment, &["environment"]),
                    ] {
                        v.extend(super::flat_entries(&look, &assets, category, folders));
                    }
                    v
                };
                (
                    blocks.join().unwrap_or_default(),
                    items.join().unwrap_or_default(),
                    rest,
                )
            });
            let mut entries = blocks;
            entries.extend(items);
            entries.extend(rest);
            Ok(Loaded {
                look,
                mobs,
                entries,
            })
        })();
        let _ = tx.send(result.map_err(|e| format!("{e:#}")));
    });
    Loading::Busy(rx, Instant::now())
}

/// What the large view was drawn for: the id, the angles in hundredths, the
/// animation and company, and its size in pixels.
type LargeKey = (String, i32, i32, u8, u32);

/// Where each region was drawn in the last frame (points), for the layout
/// test.
#[derive(Debug, Default, Clone)]
pub struct Report {
    pub categories: Option<egui::Rect>,
    pub grid: Option<egui::Rect>,
    /// What the grid shows (its scroll area's visible part).
    pub grid_view: Option<egui::Rect>,
    /// The selected thumbnail, if it was drawn.
    pub selected: Option<egui::Rect>,
    /// The width the thumbnails have (the grid less its scroll bar).
    pub grid_inner_width: f32,
    pub large: Option<egui::Rect>,
    pub raw: Option<egui::Rect>,
    pub note: Option<egui::Rect>,
    pub columns: usize,
}

pub struct Viewer {
    pub report: Report,
    pub model: Model,
    loading: Loading,
    pub sets: Vec<String>,
    category: Category,
    search: String,
    thumbs: HashMap<(String, usize), TextureHandle>,
    large_image: Option<(LargeKey, TextureHandle)>,
    raw: Option<(String, TextureHandle)>,
    yaw: f32,
    pitch: f32,
    anim: Anim,
    company: bool,
    /// The large view's zoom (1 to 8) and the picture point, as a fraction
    /// of the whole, at its top left corner.
    zoom: f32,
    zoom_at: egui::Vec2,
    /// What made the grid last scroll, and where it is scrolled to.
    grid_seen: Option<(usize, usize, Category, usize, String)>,
    grid_offset: f32,
    started: Instant,
    /// How long the set took to load, once it has.
    pub loaded_in: Option<std::time::Duration>,
}

impl Viewer {
    pub fn new() -> Self {
        let assets = crate::assets::dir();
        let dir = assets.join("downloads/resourcepacks");
        let sets: Vec<String> = ["vanilla", "barebones", "realistic"]
            .into_iter()
            .filter(|name| {
                super::stack(name, &dir).is_some_and(|s| *name == "vanilla" || !s.packs.is_empty())
            })
            .map(str::to_owned)
            .collect();
        let set = sets
            .first()
            .cloned()
            .unwrap_or_else(|| "vanilla".to_owned());
        Self {
            report: Report::default(),
            model: Model::new(
                &set,
                Marks::load(&marks::marks_path()),
                Some(marks::marks_path()),
            ),
            loading: load_in_background(&set),
            sets,
            category: Category::Blocks,
            search: String::new(),
            thumbs: HashMap::new(),
            large_image: None,
            raw: None,
            yaw: View::THREE_QUARTER.yaw,
            pitch: View::THREE_QUARTER.pitch,
            anim: Anim::Idle,
            company: false,
            zoom: 1.0,
            zoom_at: egui::Vec2::ZERO,
            grid_seen: None,
            grid_offset: 0.0,
            started: Instant::now(),
            loaded_in: None,
        }
    }

    /// Shows `category` filtered by `search` (the shot and the tests).
    pub fn focus(&mut self, category: Category, search: &str) {
        self.category = category;
        self.search = search.to_owned();
        self.model.selected = 0;
    }

    /// Esc, from the game's own key handling: cancels a note, shrinks the
    /// large view, then closes. Returns true when the viewer is closed.
    pub fn escape(&mut self) -> bool {
        self.model.key(Key::Esc);
        self.model.closed
    }

    fn switch_set(&mut self, set: &str) {
        let keep = self.model.current().map(str::to_owned);
        self.model.set = set.to_owned();
        self.thumbs.clear();
        self.large_image = None;
        self.raw = None;
        self.loading = load_in_background(set);
        self.started = Instant::now();
        self.loaded_in = None;
        if let Some(id) = keep {
            self.search.clear();
            self.model.note = None;
            let _ = id;
        }
    }

    fn poll(&mut self) {
        if let Loading::Busy(rx, started) = &self.loading
            && let Ok(result) = rx.try_recv()
        {
            self.loaded_in = Some(started.elapsed());
            self.loading = match result {
                Ok(loaded) => Loading::Ready(Arc::new(loaded)),
                Err(err) => Loading::Failed(err),
            };
        }
    }

    /// Draws the viewer inside the room `ui` has left, and nowhere else.
    /// Returns true when Esc or Done closes it.
    pub fn show(&mut self, ui: &mut egui::Ui) -> bool {
        self.poll();
        self.report = Report::default();
        let ctx = ui.ctx().clone();
        let avail = ui.available_rect_before_wrap();
        ui.allocate_rect(avail, egui::Sense::hover());
        let loaded = match &self.loading {
            Loading::Ready(l) => Some(l.clone()),
            Loading::Busy(..) => {
                ctx.request_repaint();
                None
            }
            Loading::Failed(_) => None,
        };
        // The visible entries.
        let search = self.search.to_lowercase();
        let visible: Vec<usize> = loaded
            .as_ref()
            .map(|l| {
                l.entries
                    .iter()
                    .enumerate()
                    .filter(|(_, e)| {
                        e.category == self.category
                            && (search.is_empty() || e.id.to_lowercase().contains(&search))
                    })
                    .map(|(i, _)| i)
                    .collect()
            })
            .unwrap_or_default();
        if let Some(l) = &loaded {
            let ids: Vec<(String, bool)> = visible
                .iter()
                .map(|&i| (l.entries[i].id.clone(), !l.entries[i].flags.is_empty()))
                .collect();
            if ids != self.model.items {
                let keep = self.model.current().map(str::to_owned);
                self.model.items = ids;
                self.model.selected = keep
                    .and_then(|id| self.model.items.iter().position(|(i, _)| *i == id))
                    .unwrap_or(0);
            }
        }
        // Keys, unless a text box has them.
        if !ctx.egui_wants_keyboard_input() || self.model.note.is_some() {
            let keys = [
                (EKey::ArrowLeft, Key::Left),
                (EKey::ArrowRight, Key::Right),
                (EKey::ArrowUp, Key::Up),
                (EKey::ArrowDown, Key::Down),
                (EKey::Enter, Key::Enter),
                (EKey::B, Key::B),
                (EKey::K, Key::K),
                (EKey::U, Key::U),
                (EKey::F, Key::F),
            ];
            let typing = self.model.note.is_some();
            for (egui_key, key) in keys {
                let pressed = ui.input(|i| i.key_pressed(egui_key));
                // While a note is typed only Enter and Esc count.
                if pressed && (!typing || matches!(key, Key::Enter | Key::Esc)) {
                    self.model.key(key);
                }
            }
        }
        let regions = regions(avail, self.model.large, self.model.note.is_some());
        let total = loaded.as_ref().map_or(0, |l| l.entries.len());
        self.top_bar(ui, regions.top, total, regions.categories.is_none());
        if let Some(rect) = regions.note {
            self.note_strip(ui, rect);
        }
        if let Some(rect) = regions.categories {
            self.report.categories = Some(rect);
            self.category_list(ui, rect);
        }
        let Some(loaded) = loaded else {
            let text = match &self.loading {
                Loading::Failed(err) => {
                    RichText::new(format!("Can't load: {err}")).color(Color32::LIGHT_RED)
                }
                _ => RichText::new("Rendering thumbnails..."),
            };
            ui.scope_builder(egui::UiBuilder::new().max_rect(regions.grid), |ui| {
                ui.label(text)
            });
            return self.model.closed;
        };
        let selected = self.model.current().and_then(|id| {
            loaded
                .entries
                .iter()
                .find(|e| e.id == id && e.category == self.category)
        });
        if self.model.large {
            if let Some(entry) = selected {
                self.report.grid = None;
                self.large_area(ui, regions.grid, &loaded, entry);
            }
        } else {
            self.report.grid = Some(regions.grid);
            self.grid(ui, regions.grid, &loaded, &visible);
            if let Some(entry) = selected {
                self.detail(ui, regions.detail, &loaded, entry);
            }
        }
        self.model.closed
    }

    /// Status, the texture set, and Done; in a small window also the
    /// category list and search.
    fn top_bar(&mut self, ui: &mut egui::Ui, rect: egui::Rect, total: usize, compact: bool) {
        ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
            ui.set_clip_rect(rect.intersect(ui.clip_rect()));
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(self.model.status(total)).color(Color32::WHITE));
                if let Some(t) = self.loaded_in {
                    ui.label(
                        RichText::new(format!("(loaded in {:.1} s)", t.as_secs_f32())).small(),
                    );
                }
                egui::ComboBox::from_id_salt("texture-set")
                    .selected_text(format!("Texture set: {}", self.model.set))
                    .show_ui(ui, |ui| {
                        for set in self.sets.clone() {
                            if ui.selectable_label(self.model.set == set, &set).clicked() {
                                self.switch_set(&set);
                            }
                        }
                    });
                if compact {
                    egui::ComboBox::from_id_salt("categories")
                        .selected_text("Categories")
                        .show_ui(ui, |ui| {
                            for category in Category::ALL {
                                if ui
                                    .selectable_label(self.category == category, category.name())
                                    .clicked()
                                {
                                    self.category = category;
                                    self.model.selected = 0;
                                }
                            }
                        });
                    ui.add(
                        egui::TextEdit::singleline(&mut self.search)
                            .hint_text("Search")
                            .desired_width(120.0),
                    );
                }
                if ui.button("Done").clicked() {
                    self.model.closed = true;
                }
            });
        });
    }

    /// The note box, in a strip under the top bar so it is always on screen.
    fn note_strip(&mut self, ui: &mut egui::Ui, rect: egui::Rect) {
        let id = self.model.current().unwrap_or("").to_owned();
        let Some(note) = self.model.note.as_mut() else {
            return;
        };
        let mut edit_rect = None;
        ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
            ui.set_clip_rect(rect.intersect(ui.clip_rect()));
            ui.horizontal(|ui| {
                ui.label(format!("Note for {id} (Enter saves, Esc cancels)"));
                let room = (rect.right() - ui.cursor().left()).max(80.0) - 8.0;
                let edit = ui.add(egui::TextEdit::singleline(note).desired_width(room));
                edit.request_focus();
                edit_rect = Some(edit.rect);
            });
        });
        self.report.note = edit_rect;
    }

    /// The search box, the categories and the key hints.
    fn category_list(&mut self, ui: &mut egui::Ui, rect: egui::Rect) {
        ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
            ui.set_clip_rect(rect.intersect(ui.clip_rect()));
            ui.vertical(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut self.search)
                        .hint_text("Search")
                        .desired_width(rect.width() - 8.0),
                );
                for category in Category::ALL {
                    if ui
                        .selectable_label(self.category == category, category.name())
                        .clicked()
                    {
                        self.category = category;
                        self.model.selected = 0;
                    }
                }
                ui.add_space(8.0);
                ui.label(
                    RichText::new(
                        "Arrows move\nEnter enlarges\nB broken, K OK\nU clear, F next flagged\nEsc closes",
                    )
                    .small(),
                );
            });
        });
    }

    /// The thumbnails, scrolled so the selected one stays in view.
    fn grid(
        &mut self,
        ui: &mut egui::Ui,
        rect: egui::Rect,
        loaded: &Arc<Loaded>,
        visible: &[usize],
    ) {
        ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
            ui.set_clip_rect(rect.intersect(ui.clip_rect()));
            ui.spacing_mut().item_spacing = vec2(GRID_SPACING, GRID_SPACING);
            let cell = THUMB as f32 + 8.0;
            let stride = cell + GRID_SPACING;
            // The room for thumbnails is the grid's width less its scroll bar.
            let inner = rect.width() - ui.spacing().scroll.allocated_width();
            let columns = (((inner + GRID_SPACING) / stride) as usize).max(1);
            self.model.columns = columns;
            self.report.columns = columns;
            self.report.grid_inner_width = inner;
            let rows = visible.len().div_ceil(columns);
            // Scroll after anything that moves the selection, the category
            // or the search, a resize, or a reload.
            let seen = (
                self.model.selected,
                visible.len(),
                self.category,
                columns,
                self.search.clone(),
            );
            let moved = self.grid_seen.as_ref() != Some(&seen);
            self.grid_seen = Some(seen);
            let mut area = egui::ScrollArea::vertical()
                .id_salt("texture-grid")
                .max_height(rect.height())
                .auto_shrink([false, false]);
            if moved && !visible.is_empty() {
                let top = (self.model.selected / columns) as f32 * stride;
                let view = rect.height();
                let mut offset = self.grid_offset;
                if top < offset {
                    offset = top;
                } else if top + cell > offset + view {
                    offset = top + cell - view;
                }
                area = area.vertical_scroll_offset(offset.max(0.0));
            }
            let output = area.show_rows(ui, cell, rows, |ui, range| {
                for row in range {
                    ui.horizontal(|ui| {
                        for col in 0..columns {
                            let at = row * columns + col;
                            let Some(&index) = visible.get(at) else { break };
                            self.thumbnail(ui, &loaded.entries[index], at);
                        }
                    });
                }
            });
            self.grid_offset = output.state.clamped_offset().y;
            self.report.grid_view = Some(output.inner_rect);
        });
    }

    fn thumbnail(&mut self, ui: &mut egui::Ui, entry: &Entry, at: usize) {
        let view = 0;
        let ctx = ui.ctx().clone();
        let texture = self
            .thumbs
            .entry((entry.id.clone(), view))
            .or_insert_with(|| upload(&ctx, &entry.id, &entry.views[view]))
            .clone();
        let size = THUMB as f32 + 8.0;
        let (rect, response) = ui.allocate_exact_size(vec2(size, size), egui::Sense::click());
        let selected = at == self.model.selected;
        if selected {
            self.report.selected = Some(rect);
        }
        let painter = ui.painter();
        painter.rect_filled(rect, 2.0, Color32::from_gray(30));
        if selected {
            painter.rect_stroke(
                rect,
                2.0,
                egui::Stroke::new(2.0, Color32::WHITE),
                egui::StrokeKind::Inside,
            );
        }
        painter.image(
            texture.id(),
            rect.shrink(4.0),
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            Color32::WHITE,
        );
        if !entry.flags.is_empty() {
            let corner = [
                rect.right_top(),
                rect.right_top() + vec2(-12.0, 0.0),
                rect.right_top() + vec2(0.0, 12.0),
            ];
            painter.add(egui::Shape::convex_polygon(
                corner.to_vec(),
                Color32::RED,
                egui::Stroke::NONE,
            ));
        }
        match self
            .model
            .marks
            .get(&entry.id, &self.model.set)
            .map(|m| m.verdict)
        {
            Some(Verdict::Ok) => {
                painter.text(
                    rect.left_top() + vec2(3.0, 1.0),
                    egui::Align2::LEFT_TOP,
                    "✔",
                    egui::FontId::proportional(14.0),
                    Color32::GREEN,
                );
            }
            Some(Verdict::Broken) => {
                painter.text(
                    rect.left_top() + vec2(3.0, 1.0),
                    egui::Align2::LEFT_TOP,
                    "✘",
                    egui::FontId::proportional(14.0),
                    Color32::LIGHT_RED,
                );
            }
            None => {}
        }
        if response.clicked() {
            self.model.selected = at;
        }
        response.on_hover_text(&entry.id);
    }

    /// The right-hand column: the selected one's name, a large view sized
    /// to the column, its controls, the raw texture and the flags, all in
    /// a scroll area.
    fn detail(&mut self, ui: &mut egui::Ui, rect: egui::Rect, loaded: &Arc<Loaded>, entry: &Entry) {
        ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
            ui.set_clip_rect(rect.intersect(ui.clip_rect()));
            egui::ScrollArea::vertical()
                .id_salt("texture-detail")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let width =
                        (rect.width() - ui.spacing().scroll.allocated_width() - 8.0).max(64.0);
                    ui.set_max_width(width);
                    ui.label(RichText::new(&entry.id).color(Color32::WHITE).strong());
                    ui.label(format!("from: {}", entry.origin));
                    let side = width.min(rect.height() * 0.55).max(64.0).floor();
                    self.large_view(ui, loaded, entry, side, 512);
                    self.controls(ui, entry);
                    self.raw_texture(ui, loaded, entry, width);
                    self.flags_and_mark(ui, entry);
                });
        });
    }

    /// Enter: the large view over the grid and the detail column, the
    /// largest square that fits, with the raw texture beside it if there is
    /// room and below it (scrolling) if not.
    fn large_area(
        &mut self,
        ui: &mut egui::Ui,
        rect: egui::Rect,
        loaded: &Arc<Loaded>,
        entry: &Entry,
    ) {
        ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
            ui.set_clip_rect(rect.intersect(ui.clip_rect()));
            let bar = ui.spacing().scroll.allocated_width();
            // Title line, then the square.
            let header = 40.0;
            let side = (rect.width() - bar)
                .min(rect.height() - header)
                .max(64.0)
                .floor();
            let beside = rect.width() - side - bar - 16.0 >= RAW_BESIDE_MIN;
            egui::ScrollArea::both()
                .id_salt("texture-large")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.label(RichText::new(&entry.id).color(Color32::WHITE).strong());
                    ui.label(format!("from: {}", entry.origin));
                    if beside {
                        ui.horizontal_top(|ui| {
                            ui.vertical(|ui| {
                                self.large_view(ui, loaded, entry, side, 1024);
                            });
                            ui.vertical(|ui| {
                                let w = rect.width() - side - bar - 24.0;
                                ui.set_max_width(w);
                                self.controls(ui, entry);
                                self.raw_texture(ui, loaded, entry, w);
                                self.flags_and_mark(ui, entry);
                            });
                        });
                    } else {
                        let w = rect.width() - bar - 8.0;
                        self.large_view(ui, loaded, entry, side.min(w), 1024);
                        self.controls(ui, entry);
                        self.raw_texture(ui, loaded, entry, w);
                        self.flags_and_mark(ui, entry);
                    }
                });
        });
    }

    /// The 3D view, `side` points square, drawn at that size in physical
    /// pixels (at most `cap`). Drag rotates; the wheel or a pinch zooms from
    /// 1× to 8× around the pointer, clipped to the box; a double click
    /// resets both. Zooming never changes the layout.
    fn large_view(
        &mut self,
        ui: &mut egui::Ui,
        loaded: &Arc<Loaded>,
        entry: &Entry,
        side: f32,
        cap: u32,
    ) {
        let ctx = ui.ctx().clone();
        let pixels = ((side * ctx.pixels_per_point()).round() as u32).clamp(64, cap);
        let animated = matches!(entry.subject, Subject::Mob { .. }) && self.anim != Anim::Idle;
        let time = self.started.elapsed().as_secs_f32();
        let key = (
            entry.id.clone(),
            (self.yaw * 100.0) as i32,
            (self.pitch * 100.0) as i32,
            self.anim as u8 + 10 * u8::from(self.company),
            pixels,
        );
        if animated || self.large_image.as_ref().is_none_or(|(k, _)| *k != key) {
            let image = render_large(
                loaded,
                entry,
                self.yaw,
                self.pitch,
                self.anim,
                self.company,
                time,
                pixels,
            );
            self.large_image = Some((key, upload(&ctx, "large", &image)));
        }
        let (rect, response) =
            ui.allocate_exact_size(vec2(side, side), egui::Sense::click_and_drag());
        self.report.large = Some(rect);
        if response.double_clicked() {
            self.zoom = 1.0;
            self.zoom_at = egui::Vec2::ZERO;
            self.yaw = View::THREE_QUARTER.yaw;
            self.pitch = View::THREE_QUARTER.pitch;
        }
        let drag = response.drag_delta();
        self.yaw += drag.x * 0.01;
        self.pitch = (self.pitch + drag.y * 0.01).clamp(-1.5, 1.5);
        if response.hovered() {
            let (scroll, pinch) = ui.input(|i| (i.smooth_scroll_delta.y, i.zoom_delta()));
            let factor = (scroll * 0.005).exp() * pinch;
            if factor != 1.0
                && let Some(pointer) = response.hover_pos()
            {
                let old = self.zoom;
                let new = (old * factor).clamp(1.0, 8.0);
                // The picture point under the pointer stays under it.
                let fraction = (pointer - rect.min) / rect.size();
                let at = self.zoom_at + fraction / old;
                self.zoom = new;
                self.zoom_at = at - fraction / new;
                let room = 1.0 - 1.0 / new;
                self.zoom_at = self.zoom_at.clamp(egui::Vec2::ZERO, vec2(room, room));
            }
        }
        if let Some((_, texture)) = &self.large_image {
            let size = 1.0 / self.zoom;
            let uv = egui::Rect::from_min_size(
                egui::pos2(self.zoom_at.x, self.zoom_at.y),
                vec2(size, size),
            );
            ui.painter()
                .with_clip_rect(rect)
                .image(texture.id(), rect, uv, Color32::WHITE);
        }
        if animated {
            ctx.request_repaint();
        }
    }

    /// The animation buttons of a mob or the "next to grass" box of a block.
    fn controls(&mut self, ui: &mut egui::Ui, entry: &Entry) {
        ui.horizontal_wrapped(|ui| match entry.subject {
            Subject::Mob { .. } => {
                for anim in Anim::ALL {
                    if ui
                        .selectable_label(self.anim == anim, anim.name())
                        .clicked()
                    {
                        self.anim = anim;
                    }
                }
            }
            Subject::Block(_) => {
                ui.checkbox(&mut self.company, "Next to grass and stone");
            }
            _ => {}
        });
    }

    /// The raw texture with the model's UV boxes over it, scaled to fit
    /// `width` by whole numbers where it can (so its pixels stay sharp) and
    /// never wider than `width`.
    fn raw_texture(&mut self, ui: &mut egui::Ui, loaded: &Arc<Loaded>, entry: &Entry, width: f32) {
        let ctx = ui.ctx().clone();
        let Some(image) = raw_with_boxes(loaded, entry) else {
            return;
        };
        if self.raw.as_ref().is_none_or(|(id, _)| *id != entry.id) {
            self.raw = Some((entry.id.clone(), upload(&ctx, "raw", &image)));
        }
        let Some((_, t)) = &self.raw else { return };
        let longest = image.width().max(image.height()) as f32;
        let fit = width / longest;
        let scale = if fit >= 1.0 { fit.floor() } else { fit };
        ui.label(RichText::new("raw texture and UV boxes").small());
        let response = ui.image((
            t.id(),
            vec2(image.width() as f32 * scale, image.height() as f32 * scale),
        ));
        // What shows: the part the scroll area hasn't clipped away.
        self.report.raw = Some(response.rect.intersect(ui.clip_rect()));
    }

    fn flags_and_mark(&mut self, ui: &mut egui::Ui, entry: &Entry) {
        for flag in &entry.flags {
            ui.label(
                RichText::new(format!(
                    "[{}] {} {}",
                    flag.detector.name(),
                    flag.part,
                    flag.detail
                ))
                .small()
                .color(Color32::LIGHT_RED),
            );
        }
        if let Some(mark) = self.model.marks.get(&entry.id, &self.model.set) {
            ui.label(format!(
                "marked {:?} {} {}",
                mark.verdict, mark.date, mark.note
            ));
        }
    }
}

/// Space between thumbnails.
const GRID_SPACING: f32 = 4.0;
/// The raw texture goes beside the enlarged view if at least this much
/// width is left over.
const RAW_BESIDE_MIN: f32 = 150.0;
const BAR: f32 = 28.0;
const GAP: f32 = 8.0;

/// Where each region of the viewer goes inside `avail`, from the room
/// there is, never from fixed sizes: a top bar; the category list (hidden
/// behind a dropdown in the top bar when the window is small); a detail
/// column of about 35% of the width (260 points to half); the grid gets the
/// rest. Enlarged, the grid and the detail column are one region.
#[derive(Debug, Clone, Copy)]
pub struct Regions {
    pub top: egui::Rect,
    pub note: Option<egui::Rect>,
    pub categories: Option<egui::Rect>,
    pub grid: egui::Rect,
    pub detail: egui::Rect,
}

pub fn regions(avail: egui::Rect, enlarged: bool, note: bool) -> Regions {
    use egui::{Rect, pos2};
    let compact = avail.width() < 700.0 || avail.height() < 300.0;
    let top_h = if compact { BAR * 2.0 } else { BAR };
    let top = Rect::from_min_size(avail.min, vec2(avail.width(), top_h.min(avail.height())));
    let mut y = top.bottom() + 4.0;
    let note_rect = note.then(|| {
        let r = Rect::from_min_size(pos2(avail.left(), y), vec2(avail.width(), BAR));
        y = r.bottom() + 4.0;
        r
    });
    let body = Rect::from_min_max(pos2(avail.left(), y.min(avail.bottom())), avail.max);
    let cat_w = if compact {
        0.0
    } else if avail.width() < 900.0 {
        110.0
    } else {
        160.0
    };
    let categories = (!compact).then(|| Rect::from_min_size(body.min, vec2(cat_w, body.height())));
    let left = body.left() + if compact { 0.0 } else { cat_w + GAP };
    let rest = Rect::from_min_max(pos2(left.min(body.right()), body.top()), body.max);
    let detail_w = (avail.width() * 0.35)
        .clamp(260.0, (avail.width() * 0.5).max(260.0))
        .min(rest.width() * 0.6);
    let grid = Rect::from_min_max(rest.min, pos2(rest.right() - detail_w - GAP, rest.bottom()));
    let detail = Rect::from_min_max(pos2(rest.right() - detail_w, rest.top()), rest.max);
    if enlarged {
        return Regions {
            top,
            note: note_rect,
            categories,
            grid: rest,
            detail: rest,
        };
    }
    Regions {
        top,
        note: note_rect,
        categories,
        grid,
        detail,
    }
}

impl Default for Viewer {
    fn default() -> Self {
        Self::new()
    }
}

fn upload(ctx: &egui::Context, name: &str, image: &RgbaImage) -> TextureHandle {
    crate::startup::load_egui(ctx, name, image, TextureOptions::NEAREST)
}

/// The large view of `entry`.
#[allow(clippy::too_many_arguments)]
pub fn render_large(
    loaded: &Loaded,
    entry: &Entry,
    yaw: f32,
    pitch: f32,
    anim: Anim,
    company: bool,
    time: f32,
    size: u32,
) -> RgbaImage {
    let view = View { yaw, pitch };
    match &entry.subject {
        Subject::Mob {
            kind,
            variant,
            baby,
        } => {
            let mesh = super::mob_mesh_posed(&loaded.mobs, *kind, *variant, *baby, anim, time);
            let slot = loaded.mobs.texture_slot(*kind, *variant);
            super::raster::render(
                &mesh,
                &loaded.look.mob_textures[slot],
                view,
                size,
                super::BACKGROUND,
                None,
            )
            .0
        }
        Subject::Block(state) if company => {
            let mesh = super::block_mesh_in_company(&loaded.look, *state);
            super::raster::render(
                &mesh,
                loaded.look.atlas.image(),
                view,
                size,
                super::BACKGROUND,
                Some(
                    loaded
                        .look
                        .atlas
                        .uv_rect(crate::render::atlas::MISSING_TILE),
                ),
            )
            .0
        }
        subject => super::render_subject(&loaded.look, &loaded.mobs, subject, view, size).0,
    }
}

/// The entry's raw texture; for a mob, with every UV box drawn on it.
pub fn raw_with_boxes(loaded: &Loaded, entry: &Entry) -> Option<RgbaImage> {
    match &entry.subject {
        Subject::Mob {
            kind,
            variant,
            baby,
        } => {
            let mesh = super::mob_mesh(&loaded.mobs, *kind, *variant, *baby);
            let slot = loaded.mobs.texture_slot(*kind, *variant);
            Some(super::uv_overlay(
                &loaded.look.mob_textures[slot],
                &mesh,
                256,
            ))
        }
        Subject::Flat(name) => loaded.look.res.image(name),
        Subject::Item(id) => {
            let def = crate::inventory::item::registry().get(*id)?;
            let name =
                crate::inventory::item::texture_location(def.texture.as_deref()?).into_owned();
            loaded.look.res.image(&name)
        }
        Subject::Block(state) => {
            let def = crate::world::block::registry().get(state.kind())?;
            let name = def.all_textures().find_map(|t| {
                [crate::world::Face::Top, crate::world::Face::South]
                    .into_iter()
                    .find_map(|f| {
                        t.get(f)
                            .map(|n| crate::world::block::texture_location(n).into_owned())
                    })
            })?;
            loaded.look.res.image(&name)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model() -> (crate::world::save::TempDir, Model) {
        let dir = crate::world::save::TempDir::new("viewer-marks");
        let path = dir.path().join("texture-marks.ron");
        let mut m = Model::new("vanilla", Marks::default(), Some(path));
        m.columns = 3;
        m.items = (0..7)
            .map(|i| (format!("block/b{i}"), i == 2 || i == 5))
            .collect();
        (dir, m)
    }

    #[test]
    fn arrows_move_and_stop_at_the_ends() {
        let (_d, mut m) = model();
        m.key(Key::Right);
        m.key(Key::Right);
        assert_eq!(m.selected, 2);
        m.key(Key::Down);
        assert_eq!(m.selected, 5);
        m.key(Key::Down);
        assert_eq!(m.selected, 6, "stops at the last one");
        m.key(Key::Up);
        m.key(Key::Up);
        m.key(Key::Up);
        assert_eq!(m.selected, 0);
        m.key(Key::Left);
        assert_eq!(m.selected, 0);
    }

    #[test]
    fn k_u_and_b_change_the_marks_and_save_them_at_once() {
        let (dir, mut m) = model();
        m.key(Key::K);
        assert_eq!(
            m.marks.get("block/b0", "vanilla").unwrap().verdict,
            Verdict::Ok
        );
        // The file already has it.
        let on_disk = Marks::load(&dir.path().join("texture-marks.ron"));
        assert_eq!(on_disk.counts("vanilla"), (0, 1));
        m.key(Key::Right);
        // B opens the note box; typing then Enter saves a broken mark.
        m.key(Key::B);
        assert!(m.note.is_some());
        m.note = Some("legs inside out".to_owned());
        m.key(Key::Enter);
        assert!(m.note.is_none());
        let mark = m.marks.get("block/b1", "vanilla").unwrap();
        assert_eq!(
            (mark.verdict, mark.note.as_str()),
            (Verdict::Broken, "legs inside out")
        );
        assert_eq!(
            Marks::load(&dir.path().join("texture-marks.ron")).counts("vanilla"),
            (1, 1)
        );
        // Esc cancels a note without marking.
        m.key(Key::Right);
        m.key(Key::B);
        m.key(Key::Esc);
        assert!(m.note.is_none() && m.marks.get("block/b2", "vanilla").is_none());
        assert!(!m.closed, "Esc in the note box doesn't close the viewer");
        // U clears.
        m.key(Key::Left);
        m.key(Key::U);
        assert!(m.marks.get("block/b1", "vanilla").is_none());
        assert_eq!(m.status(7), "0 broken, 1 OK, 6 unchecked");
    }

    #[test]
    fn f_jumps_to_the_next_flagged_one_and_wraps() {
        let (_d, mut m) = model();
        m.key(Key::F);
        assert_eq!(m.selected, 2);
        m.key(Key::F);
        assert_eq!(m.selected, 5);
        m.key(Key::F);
        assert_eq!(m.selected, 2, "wraps round");
    }

    #[test]
    fn enter_enlarges_and_esc_shrinks_then_closes() {
        let (_d, mut m) = model();
        m.key(Key::Enter);
        assert!(m.large);
        m.key(Key::Esc);
        assert!(!m.large && !m.closed);
        m.key(Key::Esc);
        assert!(m.closed);
    }

    #[test]
    fn marks_belong_to_the_texture_set() {
        let (_d, mut m) = model();
        m.key(Key::K);
        m.set = "barebones".to_owned();
        assert!(m.marks.get("block/b0", "barebones").is_none());
        assert_eq!(m.status(7), "0 broken, 0 OK, 7 unchecked");
    }

    /// The real viewer, loaded with the vanilla set, lays out and draws
    /// frames for every category, and a selected mob gets its large view
    /// and raw texture with UV boxes.
    #[test]
    fn the_viewer_draws_every_category_headless() {
        if !crate::assets::dir()
            .join("vanilla/assets/minecraft")
            .is_dir()
        {
            eprintln!("skipping: no assets/vanilla");
            return;
        }
        let mut viewer = Viewer::new();
        // Marks go to a temp file, not the player's.
        let dir = crate::world::save::TempDir::new("viewer-draw");
        viewer.model.set_path(dir.path().join("texture-marks.ron"));
        let ctx = egui::Context::default();
        let frame = |viewer: &mut Viewer| {
            let mut out = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1400.0, 900.0),
                    )),
                    ..Default::default()
                },
                |ui| {
                    viewer.show(ui);
                },
            );
            out.textures_delta.clear();
        };
        let started = std::time::Instant::now();
        while viewer.loaded_in.is_none() {
            frame(&mut viewer);
            assert!(started.elapsed().as_secs() < 60, "the set never loaded");
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        for category in Category::ALL {
            viewer.category = category;
            frame(&mut viewer);
            frame(&mut viewer);
            assert!(!viewer.model.items.is_empty(), "{category:?} is empty");
        }
        // A mob: select it, mark it, see the large view and the raw texture.
        viewer.category = Category::Mobs;
        viewer.search = "pig".to_owned();
        frame(&mut viewer);
        frame(&mut viewer);
        assert_eq!(viewer.model.current(), Some("mob/pig"));
        viewer.model.key(Key::B);
        viewer.model.note = Some("legs".to_owned());
        viewer.model.key(Key::Enter);
        frame(&mut viewer);
        assert_eq!(viewer.model.marks.counts("vanilla"), (1, 0));
        assert!(viewer.large_image.is_some() && viewer.raw.is_some());
    }

    const SIZES: [(f32, f32); 5] = [
        (854.0, 480.0),
        (1280.0, 720.0),
        (1440.0, 900.0),
        (1512.0, 982.0),
        (1920.0, 1080.0),
    ];

    /// A viewer in a host (the title menu or the pause menu) that can be
    /// drawn headless.
    struct Host {
        title: bool,
        ctx: egui::Context,
        menu: crate::ui::menu::Menu,
        pause: crate::ui::pause::PauseMenu,
        options: crate::options::Options,
        difficulty: crate::level::Difficulty,
        _dir: crate::world::save::TempDir,
    }

    impl Host {
        fn new(title: bool) -> Self {
            let dir = crate::world::save::TempDir::new("viewer-host");
            let mut menu = crate::ui::menu::Menu::new(dir.path().join("saves"));
            let mut pause = crate::ui::pause::PauseMenu::default();
            if title {
                menu.open_texture_viewer();
            } else {
                pause.viewer = Some(Box::default());
            }
            let mut host = Self {
                title,
                ctx: egui::Context::default(),
                menu,
                pause,
                options: crate::options::Options::default(),
                difficulty: crate::level::Difficulty::default(),
                _dir: dir,
            };
            let path = host._dir.path().join("texture-marks.ron");
            host.viewer().model.set_path(path);
            let started = std::time::Instant::now();
            while host.viewer().loaded_in.is_none() {
                host.frame(vec2(1280.0, 720.0), 1.0);
                assert!(started.elapsed().as_secs() < 90, "never loaded");
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            host
        }

        fn viewer(&mut self) -> &mut Viewer {
            if self.title {
                self.menu.viewer_mut().unwrap()
            } else {
                self.pause.viewer_mut().unwrap()
            }
        }

        /// One frame at `size` points and `ppp` pixels per point.
        fn frame(&mut self, size: egui::Vec2, ppp: f32) {
            self.ctx.set_pixels_per_point(ppp);
            let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
            let mut out = self.ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(screen),
                    ..Default::default()
                },
                |ui| {
                    if self.title {
                        self.menu.show(ui, &mut self.options);
                    } else {
                        self.pause
                            .show(ui.ctx(), &mut self.options, &mut self.difficulty);
                    }
                },
            );
            out.textures_delta.clear();
        }

        fn frames(&mut self, size: egui::Vec2, ppp: f32) {
            for _ in 0..3 {
                self.frame(size, ppp);
            }
        }
    }

    /// Draws the viewer in each host at each size and scale, a mob
    /// selected, normal, with the note box open and enlarged, and returns
    /// what is wrong: any region off the screen, or a grid whose columns
    /// don't fill its width.
    fn layout_report(print: bool) -> Vec<String> {
        let mut bad = Vec::new();
        for title in [true, false] {
            let mut host = Host::new(title);
            let name = if title { "title" } else { "pause" };
            for ppp in [1.0f32, 2.0] {
                for (w, h) in SIZES {
                    let size = vec2(w, h);
                    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
                    for state in ["normal", "note", "enlarged"] {
                        let v = host.viewer();
                        v.model.large = false;
                        v.model.note = None;
                        v.focus(Category::Mobs, "pig");
                        host.frames(size, ppp);
                        let v = host.viewer();
                        match state {
                            "note" => v.model.key(Key::B),
                            "enlarged" => v.model.key(Key::Enter),
                            _ => {}
                        }
                        host.frames(size, ppp);
                        let r = host.viewer().report.clone();
                        let label = format!("{name} {w}x{h} @{ppp} {state}");
                        let mut line = format!("{label}:");
                        let wants_note = state == "note";
                        for (region, rect) in [
                            ("categories", r.categories),
                            ("grid", r.grid),
                            ("large", r.large),
                            ("raw", r.raw),
                            ("note", r.note),
                        ] {
                            let Some(rect) = rect else {
                                let expected = match region {
                                    "note" => wants_note,
                                    "grid" => state != "enlarged",
                                    "categories" => false,
                                    _ => true,
                                };
                                if expected {
                                    line += &format!(" {region}=MISSING");
                                    bad.push(format!("{label}: {region} missing"));
                                }
                                continue;
                            };
                            let inside = screen.contains_rect(rect.shrink(0.5));
                            line += &format!(
                                " {region}=r{:.0}/b{:.0}{}",
                                rect.right(),
                                rect.bottom(),
                                if inside { "" } else { " OUT" }
                            );
                            if !inside {
                                bad.push(format!("{label}: {region} {rect:?} outside {screen:?}"));
                            }
                        }
                        if let (Some(grid), "normal") = (r.grid, state) {
                            let stride = THUMB as f32 + 8.0 + 4.0;
                            let fit = ((r.grid_inner_width + 4.0) / stride) as usize;
                            line += &format!(" columns={} (fit {fit})", r.columns);
                            if r.columns != fit.max(1)
                                || r.columns as f32 * stride - 4.0 > grid.width()
                            {
                                bad.push(format!(
                                    "{label}: {} columns don't fill the grid",
                                    r.columns
                                ));
                            }
                        }
                        if print {
                            println!("{line}");
                        }
                    }
                }
            }
        }
        bad
    }

    /// Arrows to the last entry leave its thumbnail inside the grid.
    #[test]
    fn the_selected_thumbnail_stays_in_the_grid_after_arrows() {
        if !crate::assets::dir()
            .join("vanilla/assets/minecraft")
            .is_dir()
        {
            eprintln!("skipping: no assets/vanilla");
            return;
        }
        let mut host = Host::new(true);
        let size = vec2(1280.0, 720.0);
        host.viewer().focus(Category::Blocks, "");
        host.frames(size, 2.0);
        let last = host.viewer().model.items.len() - 1;
        assert!(last > 100, "a long list to scroll");
        let columns = host.viewer().model.columns;
        // Down to the last row, one frame per key, then to the end.
        while host.viewer().model.selected + columns <= last {
            host.viewer().model.key(Key::Down);
            host.frame(size, 2.0);
            check_selected_visible(&mut host);
        }
        while host.viewer().model.selected < last {
            host.viewer().model.key(Key::Right);
            host.frame(size, 2.0);
        }
        host.frames(size, 2.0);
        assert_eq!(host.viewer().model.selected, last);
        check_selected_visible(&mut host);
        // And back up with F and Up.
        while host.viewer().model.selected >= columns {
            host.viewer().model.key(Key::Up);
            host.frame(size, 2.0);
        }
        host.frames(size, 2.0);
        check_selected_visible(&mut host);
    }

    fn check_selected_visible(host: &mut Host) {
        let r = host.viewer().report.clone();
        let (Some(selected), Some(view)) = (r.selected, r.grid_view) else {
            panic!("the selected thumbnail wasn't drawn: {r:?}");
        };
        assert!(
            view.contains_rect(selected.shrink(0.5)),
            "selected {selected:?} not inside the grid {view:?}"
        );
    }

    #[test]
    fn every_region_is_on_screen_at_every_size() {
        if !crate::assets::dir()
            .join("vanilla/assets/minecraft")
            .is_dir()
        {
            eprintln!("skipping: no assets/vanilla");
            return;
        }
        let bad = layout_report(true);
        assert!(
            bad.is_empty(),
            "{} regions off screen:\n{}",
            bad.len(),
            bad.join("\n")
        );
    }
}
