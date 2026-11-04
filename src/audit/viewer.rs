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
/// animation and company, and whether it is enlarged.
type LargeKey = (String, i32, i32, u8, bool);

pub struct Viewer {
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
            started: Instant::now(),
            loaded_in: None,
        }
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

    /// Draws the viewer. Returns true when Esc or Done closes it.
    pub fn show(&mut self, ui: &mut egui::Ui) -> bool {
        self.poll();
        let ctx = ui.ctx().clone();
        let loaded = match &self.loading {
            Loading::Ready(l) => Some(l.clone()),
            Loading::Busy(..) => {
                ui.label("Rendering thumbnails...");
                ctx.request_repaint();
                None
            }
            Loading::Failed(err) => {
                ui.label(RichText::new(format!("Can't load: {err}")).color(Color32::LIGHT_RED));
                None
            }
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
        let total = loaded.as_ref().map_or(0, |l| l.entries.len());
        ui.horizontal(|ui| {
            ui.label(RichText::new(self.model.status(total)).color(Color32::WHITE));
            if let Some(t) = self.loaded_in {
                ui.label(RichText::new(format!("(loaded in {:.1} s)", t.as_secs_f32())).small());
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
            if ui.button("Done").clicked() {
                self.model.closed = true;
            }
        });
        ui.separator();
        let height = (ui.available_height() - 8.0).max(200.0);
        ui.horizontal_top(|ui| {
            // Categories and search.
            ui.allocate_ui(vec2(150.0, height), |ui| {
                ui.vertical(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.search).hint_text("Search").desired_width(140.0));
                    for category in Category::ALL {
                        if ui.selectable_label(self.category == category, category.name()).clicked() {
                            self.category = category;
                            self.model.selected = 0;
                        }
                    }
                    ui.add_space(8.0);
                    ui.label(RichText::new("Arrows move, Enter enlarges\nB broken, K OK, U clear\nF next flagged, Esc closes").small());
                });
            });
            let Some(loaded) = loaded else { return };
            // The grid.
            let cell = THUMB as f32 + 12.0;
            ui.allocate_ui(vec2(640.0, height), |ui| {
                let columns = ((ui.available_width() / cell) as usize).max(1);
                self.model.columns = columns;
                let rows = visible.len().div_ceil(columns);
                let selected_row = self.model.selected / columns;
                egui::ScrollArea::vertical()
                    .id_salt("texture-grid")
                    .max_height(height)
                    .show_rows(ui, cell, rows, |ui, range| {
                        if ui.input(|i| {
                            i.key_pressed(EKey::ArrowDown)
                                || i.key_pressed(EKey::ArrowUp)
                                || i.key_pressed(EKey::F)
                        }) {
                            let top = selected_row as f32 * cell;
                            ui.scroll_to_rect(
                                egui::Rect::from_min_size(
                                    ui.min_rect().min + vec2(0.0, top - range.start as f32 * cell),
                                    vec2(cell, cell),
                                ),
                                None,
                            );
                        }
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
            });
            // The selected one.
            let selected = self
                .model
                .current()
                .and_then(|id| loaded.entries.iter().find(|e| e.id == id && e.category == self.category));
            ui.allocate_ui(vec2(ui.available_width(), height), |ui| {
                ui.vertical(|ui| {
                    if let Some(entry) = selected {
                        self.detail(ui, &loaded, entry);
                    }
                });
            });
        });
        self.model.closed
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

    fn detail(&mut self, ui: &mut egui::Ui, loaded: &Arc<Loaded>, entry: &Entry) {
        let ctx = ui.ctx().clone();
        ui.label(RichText::new(&entry.id).color(Color32::WHITE).strong());
        ui.label(format!("from: {}", entry.origin));
        // The large 3D view: drag to rotate.
        let size = if self.model.large { 480 } else { 256 };
        let animated = matches!(entry.subject, Subject::Mob { .. }) && self.anim != Anim::Idle;
        let time = self.started.elapsed().as_secs_f32();
        let key = (
            entry.id.clone(),
            (self.yaw * 100.0) as i32,
            (self.pitch * 100.0) as i32,
            self.anim as u8 + 10 * u8::from(self.company),
            self.model.large,
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
                size,
            );
            self.large_image = Some((key, upload(&ctx, "large", &image)));
        }
        if let Some((_, texture)) = &self.large_image {
            let response = ui.add(
                egui::Image::new((texture.id(), vec2(size as f32, size as f32)))
                    .sense(egui::Sense::drag()),
            );
            let drag = response.drag_delta();
            self.yaw += drag.x * 0.01;
            self.pitch = (self.pitch + drag.y * 0.01).clamp(-1.5, 1.5);
        }
        if animated {
            ctx.request_repaint();
        }
        ui.horizontal(|ui| match entry.subject {
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
        // The raw texture, with the model's UV boxes over it.
        if let Some(image) = raw_with_boxes(loaded, entry) {
            let again = self.raw.as_ref().is_none_or(|(id, _)| *id != entry.id);
            if again {
                self.raw = Some((entry.id.clone(), upload(&ctx, "raw", &image)));
            }
            if let Some((_, t)) = &self.raw {
                let scale = (200.0 / image.width().max(image.height()) as f32).max(1.0);
                ui.label(RichText::new("raw texture and UV boxes").small());
                ui.image((
                    t.id(),
                    vec2(image.width() as f32 * scale, image.height() as f32 * scale),
                ));
            }
        }
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
        if let Some(note) = self.model.note.as_mut() {
            ui.label("Note (Enter saves, Esc cancels):");
            let edit = ui.add(egui::TextEdit::singleline(note).desired_width(260.0));
            edit.request_focus();
        }
    }
}

impl Default for Viewer {
    fn default() -> Self {
        Self::new()
    }
}

fn upload(ctx: &egui::Context, name: &str, image: &RgbaImage) -> TextureHandle {
    let size = [image.width() as usize, image.height() as usize];
    ctx.load_texture(
        name.to_owned(),
        egui::ColorImage::from_rgba_unmultiplied(size, image.as_raw()),
        TextureOptions::NEAREST,
    )
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
            let _ = ctx.run_ui(
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
}
