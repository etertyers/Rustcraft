//! The Options screens, shared by the title screen and the pause menu, after
//! modern vanilla's: the main page (FOV, Difficulty, player name and the
//! buttons to the others), Skin Customization, Music & Sounds, Video
//! Settings, Controls with Mouse Settings and Key Binds, and Accessibility
//! Settings. Changes go straight into `Options`.

use egui::{Button, Color32, Rect, Response, RichText, Sense, Stroke, StrokeKind, Ui, vec2};

use super::ClickSound;
use crate::{
    bindings::{Action, Bindings, Category, Input, input_name},
    level::Difficulty,
    options::{
        BIOME_BLENDS, Clouds, FOVS, FRAMERATES, Graphics, MAX_GUI_SCALE, MainHand, NAME_LENGTHS,
        Options, Particles, RENDER_DISTANCES, SENSITIVITIES, UNLIMITED_FRAMERATE, valid_name,
    },
};

/// Vanilla's 150 × 20 option buttons and the 310-wide ones, at twice the
/// size like the rest of the menus.
const HALF: [f32; 2] = [200.0, 30.0];
const WIDE: [f32; 2] = [408.0, 30.0];
const GAP: f32 = 8.0;
const GRAY: Color32 = Color32::from_rgb(0xa0, 0xa0, 0xa0);
const RED: Color32 = Color32::from_rgb(0xff, 0x55, 0x55);
const YELLOW: Color32 = Color32::from_rgb(0xff, 0xff, 0x55);

/// One of the Options screens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Main,
    Skin,
    Sounds,
    Video,
    Controls,
    Mouse,
    KeyBinds,
    Accessibility,
}

impl Page {
    /// Where Done and Escape go: Mouse Settings and Key Binds back to
    /// Controls, the rest back to the main page, which leaves.
    fn parent(self) -> Option<Page> {
        match self {
            Page::Main => None,
            Page::Mouse | Page::KeyBinds => Some(Page::Controls),
            _ => Some(Page::Main),
        }
    }

    fn title(self) -> &'static str {
        match self {
            Page::Main => "Options",
            Page::Skin => "Skin Customization",
            Page::Sounds => "Music & Sound Options",
            Page::Video => "Video Settings",
            Page::Controls => "Controls",
            Page::Mouse => "Mouse Settings",
            Page::KeyBinds => "Key Binds",
            Page::Accessibility => "Accessibility Settings",
        }
    }
}

/// What the Options screens asked their caller for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OptionsAction {
    /// Done on the main page: leave Options.
    Done,
    ResourcePacks,
}

/// The open Options screen and the key bind being changed.
#[derive(Debug)]
pub struct OptionsScreen {
    pub page: Page,
    /// The action whose key the next key or mouse button press sets.
    waiting: Option<Action>,
    /// Clicks are ignored while the press that set a key is still being
    /// let go of, so it doesn't click the button under the mouse.
    muted: bool,
    unmute_after_frame: bool,
    /// The Player Name box's text, kept even while it isn't a valid name.
    name: Option<String>,
}

impl Default for OptionsScreen {
    fn default() -> Self {
        Self::new(Page::Main)
    }
}

impl OptionsScreen {
    pub fn new(page: Page) -> Self {
        Self {
            page,
            waiting: None,
            muted: false,
            unmute_after_frame: false,
            name: None,
        }
    }

    /// Escape: stops waiting for a key, or goes up a page. Returns true
    /// when it leaves the main page.
    pub fn back(&mut self) -> bool {
        if self.waiting.take().is_some() {
            return false;
        }
        match self.page.parent() {
            Some(parent) => {
                self.page = parent;
                false
            }
            None => true,
        }
    }

    /// Waits for the next key or mouse button press to set `action`'s key.
    pub fn wait_for(&mut self, action: Action) {
        self.waiting = Some(action);
    }

    /// The action waiting for a key on the Key Binds screen.
    pub fn waiting(&self) -> Option<Action> {
        self.waiting
    }

    /// Sets the waiting action's key (`None` unbinds it, which Escape
    /// does). A mouse button stays muted until it is let go.
    pub fn bind(&mut self, keys: &mut Bindings, input: Option<Input>) {
        let Some(action) = self.waiting.take() else {
            return;
        };
        keys.set(action, input);
        self.muted = true;
        self.unmute_after_frame = !matches!(input, Some(Input::Mouse(_)));
    }

    /// A mouse button was let go.
    pub fn mouse_released(&mut self) {
        if self.muted {
            self.unmute_after_frame = true;
        }
    }

    fn clicked(&self, response: &Response) -> bool {
        !self.muted && response.clicked_with_sound()
    }

    /// Draws the current page. `difficulty` is the world's, when there is
    /// one.
    pub fn show(
        &mut self,
        ui: &mut Ui,
        options: &mut Options,
        difficulty: Option<&mut Difficulty>,
    ) -> Option<OptionsAction> {
        heading(ui, self.page.title());
        let action = match self.page {
            Page::Main => self.main(ui, options, difficulty),
            Page::Skin => {
                self.skin(ui, options);
                None
            }
            Page::Sounds => {
                self.sounds(ui, options);
                None
            }
            Page::Video => {
                self.video(ui, options);
                None
            }
            Page::Controls => {
                self.controls(ui, options);
                None
            }
            Page::Mouse => {
                self.mouse(ui, options);
                None
            }
            Page::KeyBinds => {
                self.key_binds(ui, &mut options.keys);
                None
            }
            Page::Accessibility => {
                self.accessibility(ui, options);
                None
            }
        };
        if self.page != Page::KeyBinds {
            ui.add_space(16.0);
            let done = ui.add_sized(WIDE, Button::new("Done"));
            if self.clicked(&done) {
                match self.page.parent() {
                    Some(parent) => self.page = parent,
                    None => return Some(OptionsAction::Done),
                }
            }
        }
        if self.unmute_after_frame {
            self.muted = false;
            self.unmute_after_frame = false;
        }
        action
    }

    fn main(
        &mut self,
        ui: &mut Ui,
        options: &mut Options,
        difficulty: Option<&mut Difficulty>,
    ) -> Option<OptionsAction> {
        let mut action = None;
        pairs(ui, "options-main", |ui, cell| {
            cell(ui);
            range_slider(ui, HALF, &mut options.fov, FOVS, 1.0, |v| {
                format!("FOV: {}", fov_name(v))
            });
            cell(ui);
            match difficulty {
                Some(difficulty) => {
                    let text = format!("Difficulty: {}", difficulty.name());
                    let response = ui.add_sized(HALF, Button::new(text));
                    if self.clicked(&response) {
                        *difficulty = difficulty.next();
                    }
                }
                None => {
                    ui.add_enabled(false, Button::new("Difficulty").min_size(HALF.into()))
                        .on_disabled_hover_text("Set in a world");
                }
            }
        });
        ui.add_space(GAP);
        // The name other players see on LAN; only a valid name is kept.
        let name = self.name.get_or_insert_with(|| options.player_name.clone());
        ui.label(RichText::new("Player Name").color(GRAY));
        ui.add_sized(
            [WIDE[0], 24.0],
            egui::TextEdit::singleline(name).char_limit(*NAME_LENGTHS.end()),
        );
        if valid_name(name) {
            options.player_name = name.clone();
        } else {
            ui.label(
                RichText::new("3 to 16 letters, digits or underscores")
                    .color(RED)
                    .size(14.0),
            );
        }
        ui.add_space(GAP);
        let pages = [
            ("Skin Customization...", Some(Page::Skin)),
            ("Music & Sounds...", Some(Page::Sounds)),
            ("Video Settings...", Some(Page::Video)),
            ("Controls...", Some(Page::Controls)),
            ("Chat Settings...", None),
            ("Resource Packs...", None),
            ("Accessibility Settings...", Some(Page::Accessibility)),
        ];
        pairs(ui, "options-pages", |ui, cell| {
            for (label, page) in pages {
                cell(ui);
                let chat = label.starts_with("Chat");
                let response = ui.add_enabled(!chat, Button::new(label).min_size(HALF.into()));
                if chat {
                    response.on_disabled_hover_text("Chat arrives with commands");
                } else if self.clicked(&response) {
                    match page {
                        Some(page) => self.page = page,
                        None => action = Some(OptionsAction::ResourcePacks),
                    }
                }
            }
        });
        action
    }

    fn skin(&mut self, ui: &mut Ui, options: &mut Options) {
        let skin = &mut options.skin;
        pairs(ui, "options-skin", |ui, cell| {
            for (label, on) in [
                ("Jacket", &mut skin.jacket),
                ("Left Sleeve", &mut skin.left_sleeve),
                ("Right Sleeve", &mut skin.right_sleeve),
                ("Left Pants Leg", &mut skin.left_pants),
                ("Right Pants Leg", &mut skin.right_pants),
                ("Hat", &mut skin.hat),
            ] {
                cell(ui);
                self.toggle(ui, label, on);
            }
            cell(ui);
            let hand = match options.main_hand {
                MainHand::Left => "Left",
                MainHand::Right => "Right",
            };
            let response = ui.add_sized(HALF, Button::new(format!("Main Hand: {hand}")));
            if self.clicked(&response) {
                options.main_hand = match options.main_hand {
                    MainHand::Left => MainHand::Right,
                    MainHand::Right => MainHand::Left,
                };
            }
        });
    }

    fn sounds(&mut self, ui: &mut Ui, options: &mut Options) {
        range_slider(ui, WIDE, &mut options.volume, 0.0..=1.0, 0.01, |v| {
            format!("Master Volume: {}", volume_name(v))
        });
        ui.add_space(GAP);
        let v = &mut options.volumes;
        pairs(ui, "options-sounds", |ui, cell| {
            for (label, value) in [
                ("Music", &mut v.music),
                ("Jukebox/Note Blocks", &mut v.records),
                ("Weather", &mut v.weather),
                ("Blocks", &mut v.blocks),
                ("Hostile Creatures", &mut v.hostile),
                ("Friendly Creatures", &mut v.neutral),
                ("Players", &mut v.players),
                ("Ambient/Environment", &mut v.ambient),
                ("Voice/Speech", &mut v.voice),
            ] {
                cell(ui);
                range_slider(ui, HALF, value, 0.0..=1.0, 0.01, |v| {
                    format!("{label}: {}", volume_name(v))
                });
            }
        });
    }

    fn video(&mut self, ui: &mut Ui, options: &mut Options) {
        let height = list_height(ui);
        egui::ScrollArea::vertical()
            .max_height(height)
            .show(ui, |ui| {
                pairs(ui, "options-video", |ui, cell| {
                    cell(ui);
                    let graphics = match options.graphics {
                        Graphics::Fast => "Fast",
                        Graphics::Fancy => "Fancy",
                    };
                    let response = ui.add_sized(HALF, Button::new(format!("Graphics: {graphics}")));
                    if self.clicked(&response) {
                        options.graphics = match options.graphics {
                            Graphics::Fast => Graphics::Fancy,
                            Graphics::Fancy => Graphics::Fast,
                        };
                    }
                    cell(ui);
                    int_slider(ui, &mut options.render_distance, RENDER_DISTANCES, |v| {
                        format!("Render Distance: {v} chunks")
                    });
                    cell(ui);
                    self.toggle(ui, "Smooth Lighting", &mut options.smooth_lighting);
                    cell(ui);
                    let mut rate = options.max_framerate as i32;
                    let range = *FRAMERATES.start() as i32..=*FRAMERATES.end() as i32;
                    if int_slider(ui, &mut rate, range, |v| {
                        if v as u32 >= UNLIMITED_FRAMERATE {
                            "Max Framerate: Unlimited".to_owned()
                        } else {
                            format!("Max Framerate: {v} fps")
                        }
                    }) {
                        // Vanilla's slider moves in steps of 10.
                        options.max_framerate = (rate as u32).div_ceil(10) * 10;
                    }
                    cell(ui);
                    self.toggle(ui, "VSync", &mut options.vsync);
                    cell(ui);
                    self.toggle(ui, "View Bobbing", &mut options.view_bobbing);
                    cell(ui);
                    let mut scale = options.gui_scale as i32;
                    if int_slider(ui, &mut scale, 0..=MAX_GUI_SCALE as i32, |v| match v {
                        0 => "GUI Scale: Auto".to_owned(),
                        v => format!("GUI Scale: {v}"),
                    }) {
                        options.gui_scale = scale as u32;
                    }
                    cell(ui);
                    self.toggle(ui, "Fullscreen", &mut options.fullscreen);
                    cell(ui);
                    range_slider(ui, HALF, &mut options.brightness, 0.0..=1.0, 0.01, |v| {
                        format!("Brightness: {}", brightness_name(v))
                    });
                    cell(ui);
                    let clouds = match options.clouds {
                        Clouds::Off => "OFF",
                        Clouds::Fast => "Fast",
                        Clouds::Fancy => "Fancy",
                    };
                    let response = ui.add_sized(HALF, Button::new(format!("Clouds: {clouds}")));
                    if self.clicked(&response) {
                        options.clouds = match options.clouds {
                            Clouds::Fancy => Clouds::Fast,
                            Clouds::Fast => Clouds::Off,
                            Clouds::Off => Clouds::Fancy,
                        };
                    }
                    cell(ui);
                    let particles = match options.particles {
                        Particles::All => "All",
                        Particles::Decreased => "Decreased",
                        Particles::Minimal => "Minimal",
                    };
                    let response =
                        ui.add_sized(HALF, Button::new(format!("Particles: {particles}")));
                    if self.clicked(&response) {
                        options.particles = match options.particles {
                            Particles::All => Particles::Decreased,
                            Particles::Decreased => Particles::Minimal,
                            Particles::Minimal => Particles::All,
                        };
                    }
                    cell(ui);
                    self.toggle(ui, "Entity Shadows", &mut options.entity_shadows);
                    cell(ui);
                    int_slider(ui, &mut options.biome_blend, BIOME_BLENDS, |v| {
                        format!("Biome Blend: {}", blend_name(v))
                    });
                });
            });
    }

    fn controls(&mut self, ui: &mut Ui, options: &mut Options) {
        pairs(ui, "options-controls", |ui, cell| {
            cell(ui);
            let response = ui.add_sized(HALF, Button::new("Mouse Settings..."));
            if self.clicked(&response) {
                self.page = Page::Mouse;
            }
            cell(ui);
            let response = ui.add_sized(HALF, Button::new("Key Binds..."));
            if self.clicked(&response) {
                self.page = Page::KeyBinds;
            }
            cell(ui);
            self.hold_or_toggle(ui, "Sneak", &mut options.toggle_sneak);
            cell(ui);
            self.hold_or_toggle(ui, "Sprint", &mut options.toggle_sprint);
            cell(ui);
            self.toggle(ui, "Auto-Jump", &mut options.auto_jump);
        });
    }

    fn mouse(&mut self, ui: &mut Ui, options: &mut Options) {
        pairs(ui, "options-mouse", |ui, cell| {
            cell(ui);
            range_slider(
                ui,
                HALF,
                &mut options.sensitivity,
                SENSITIVITIES,
                0.01,
                |v| format!("Sensitivity: {}", sensitivity_name(v)),
            );
            cell(ui);
            self.toggle(ui, "Invert Mouse", &mut options.invert_y);
        });
    }

    fn accessibility(&mut self, ui: &mut Ui, options: &mut Options) {
        pairs(ui, "options-accessibility", |ui, cell| {
            cell(ui);
            self.toggle(ui, "Auto-Jump", &mut options.auto_jump);
            cell(ui);
            self.toggle(ui, "View Bobbing", &mut options.view_bobbing);
            cell(ui);
            self.hold_or_toggle(ui, "Sneak", &mut options.toggle_sneak);
            cell(ui);
            self.hold_or_toggle(ui, "Sprint", &mut options.toggle_sprint);
            for (label, value) in [
                ("FOV Effects", &mut options.fov_effects),
                ("Damage Tilt", &mut options.damage_tilt),
                ("Glint Speed", &mut options.glint_speed),
                ("Glint Strength", &mut options.glint_strength),
            ] {
                cell(ui);
                range_slider(ui, HALF, value, 0.0..=1.0, 0.01, |v| {
                    format!("{label}: {}", volume_name(v))
                });
            }
        });
    }

    /// Every listed action by category: its name, its key (red when
    /// another action shares it, `> key <` while waiting for a new one) and
    /// Reset; then Reset Keys and Done.
    fn key_binds(&mut self, ui: &mut Ui, keys: &mut Bindings) {
        let height = list_height(ui);
        egui::ScrollArea::vertical()
            .max_height(height)
            .show(ui, |ui| {
                for category in Category::ALL {
                    let actions: Vec<Action> = Action::ALL
                        .iter()
                        .copied()
                        .filter(|a| a.category() == category && a.listed())
                        .collect();
                    if actions.is_empty() {
                        continue;
                    }
                    ui.add_space(GAP);
                    ui.label(
                        RichText::new(category.name())
                            .color(Color32::WHITE)
                            .strong(),
                    );
                    for action in actions {
                        ui.horizontal(|ui| {
                            ui.add_space((ui.available_width() - 470.0).max(0.0) / 2.0);
                            ui.add_sized(
                                [220.0, 26.0],
                                egui::Label::new(
                                    RichText::new(action.name()).color(Color32::WHITE),
                                ),
                            );
                            let name = input_name(keys.get(action));
                            let text = if self.waiting == Some(action) {
                                RichText::new(format!("> {name} <")).color(YELLOW)
                            } else if keys.conflicts(action) {
                                RichText::new(name).color(RED)
                            } else {
                                RichText::new(name)
                            };
                            let key = ui.add_sized([150.0, 26.0], Button::new(text));
                            if self.clicked(&key) {
                                self.wait_for(action);
                            }
                            let reset = ui.add_enabled(
                                !keys.is_default(action),
                                Button::new("Reset").min_size(vec2(90.0, 26.0)),
                            );
                            if self.clicked(&reset) {
                                keys.reset(action);
                                self.waiting = None;
                            }
                        });
                    }
                }
            });
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            ui.add_space((ui.available_width() - 2.0 * HALF[0] - GAP).max(0.0) / 2.0);
            let reset = ui.add_enabled(
                !keys.all_default(),
                Button::new("Reset Keys").min_size(HALF.into()),
            );
            if self.clicked(&reset) {
                keys.reset_all();
                self.waiting = None;
            }
            let done = ui.add(Button::new("Done").min_size(HALF.into()));
            if self.clicked(&done) {
                self.waiting = None;
                self.page = Page::Controls;
            }
        });
    }

    fn toggle(&self, ui: &mut Ui, label: &str, on: &mut bool) {
        let text = format!("{label}: {}", if *on { "ON" } else { "OFF" });
        let response = ui.add_sized(HALF, Button::new(text));
        if self.clicked(&response) {
            *on = !*on;
        }
    }

    fn hold_or_toggle(&self, ui: &mut Ui, label: &str, toggle: &mut bool) {
        let text = format!("{label}: {}", if *toggle { "Toggle" } else { "Hold" });
        let response = ui.add_sized(HALF, Button::new(text));
        if self.clicked(&response) {
            *toggle = !*toggle;
        }
    }
}

/// Lays widgets out two to a row, centered: `cell` starts the next one.
fn pairs(ui: &mut Ui, id: &str, add: impl FnOnce(&mut Ui, &mut dyn FnMut(&mut Ui))) {
    let width = 2.0 * HALF[0] + GAP;
    ui.horizontal(|ui| {
        ui.add_space((ui.available_width() - width).max(0.0) / 2.0);
        egui::Grid::new(id)
            .num_columns(2)
            .spacing([GAP, GAP])
            .show(ui, |ui| {
                let mut count = 0;
                let mut cell = |ui: &mut Ui| {
                    if count > 0 && count % 2 == 0 {
                        ui.end_row();
                    }
                    count += 1;
                };
                add(ui, &mut cell);
            });
    });
}

/// Vanilla's option slider: a button-sized track with the label on it and a
/// handle that follows the mouse. `value` is 0–1; returns true if it moved.
fn slider(ui: &mut Ui, size: [f32; 2], text: &str, value: &mut f32) -> bool {
    let (rect, response) = ui.allocate_exact_size(size.into(), Sense::click_and_drag());
    const HANDLE: f32 = 12.0;
    let mut changed = false;
    if (response.dragged() || response.clicked())
        && let Some(pointer) = response.interact_pointer_pos()
    {
        let fraction =
            ((pointer.x - rect.left() - HANDLE / 2.0) / (rect.width() - HANDLE)).clamp(0.0, 1.0);
        if fraction != *value {
            *value = fraction;
            changed = true;
        }
    }
    let visuals = ui.style().interact(&response);
    let painter = ui.painter();
    let track = ui.visuals().widgets.noninteractive;
    painter.rect(
        rect,
        2.0,
        track.bg_fill.gamma_multiply(0.6),
        Stroke::new(1.0, Color32::BLACK),
        StrokeKind::Inside,
    );
    let x = rect.left() + *value * (rect.width() - HANDLE);
    let handle = Rect::from_min_size(egui::pos2(x, rect.top()), vec2(HANDLE, rect.height()));
    painter.rect(
        handle,
        2.0,
        visuals.weak_bg_fill,
        visuals.bg_stroke,
        StrokeKind::Inside,
    );
    painter.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        text,
        egui::TextStyle::Button.resolve(ui.style()),
        Color32::WHITE,
    );
    changed
}

/// A slider over `range` in steps of `step`, labelled by `label`.
fn range_slider(
    ui: &mut Ui,
    size: [f32; 2],
    value: &mut f32,
    range: std::ops::RangeInclusive<f32>,
    step: f32,
    label: impl Fn(f32) -> String,
) -> bool {
    let (lo, hi) = (*range.start(), *range.end());
    let mut fraction = (*value - lo) / (hi - lo);
    if slider(ui, size, &label(*value), &mut fraction) {
        let raw = lo + fraction * (hi - lo);
        *value = ((raw / step).round() * step).clamp(lo, hi);
        true
    } else {
        false
    }
}

fn int_slider(
    ui: &mut Ui,
    value: &mut i32,
    range: std::ops::RangeInclusive<i32>,
    label: impl Fn(i32) -> String,
) -> bool {
    let (lo, hi) = (*range.start(), *range.end());
    let mut fraction = (*value - lo) as f32 / (hi - lo) as f32;
    if slider(ui, HALF, &label(*value), &mut fraction) {
        let new = lo + (fraction * (hi - lo) as f32).round() as i32;
        let changed = new != *value;
        *value = new;
        changed
    } else {
        false
    }
}

/// The height of a page's scrolling list: the screen less the heading
/// and the buttons under it. (The pause menu's area sizes itself to its
/// contents, so the space left in it can't be used.)
fn list_height(ui: &Ui) -> f32 {
    (ui.ctx().content_rect().height() - 220.0).max(120.0)
}

fn heading(ui: &mut Ui, text: &str) {
    ui.add_space(16.0);
    ui.label(RichText::new(text).size(32.0).color(Color32::WHITE));
    ui.add_space(16.0);
}

/// Vanilla's names for the Brightness slider's ends.
pub fn brightness_name(brightness: f32) -> String {
    match (brightness * 100.0).round() as i32 {
        0 => "Moody".to_owned(),
        100 => "Bright".to_owned(),
        percent => format!("{percent}%"),
    }
}

/// Vanilla's names for the ends and middle of the FOV slider.
pub fn fov_name(fov: f32) -> String {
    match fov.round() as i32 {
        70 => "Normal".to_owned(),
        110 => "Quake Pro".to_owned(),
        degrees => degrees.to_string(),
    }
}

/// Modern's names for the biome blend sizes: "OFF", then "3x3" up to
/// "15x15".
pub fn blend_name(radius: i32) -> String {
    match radius {
        0 => "OFF".to_owned(),
        r => format!("{0}x{0}", 2 * r + 1),
    }
}

pub fn percent(fraction: f32) -> String {
    format!("{}%", (fraction * 100.0).round() as i32)
}

/// A volume or effect slider's value: "OFF" at zero, else a percentage.
fn volume_name(fraction: f32) -> String {
    if fraction <= 0.0 {
        "OFF".to_owned()
    } else {
        percent(fraction)
    }
}

/// Vanilla's names for the sensitivity slider's ends.
fn sensitivity_name(sensitivity: f32) -> String {
    match (sensitivity * 100.0).round() as i32 {
        0 => "*yawn*".to_owned(),
        200 => "HYPERSPEED!!!".to_owned(),
        _ => percent(sensitivity),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use winit::keyboard::KeyCode;

    #[test]
    fn labels_read_like_vanilla() {
        assert_eq!(fov_name(70.0), "Normal");
        assert_eq!(fov_name(110.0), "Quake Pro");
        assert_eq!(fov_name(85.4), "85");
        assert_eq!(percent(1.0), "100%");
        assert_eq!(percent(0.25), "25%");
        assert_eq!(blend_name(0), "OFF");
        assert_eq!(blend_name(2), "5x5");
        assert_eq!(blend_name(7), "15x15");
        assert_eq!(volume_name(0.0), "OFF");
        assert_eq!(sensitivity_name(2.0), "HYPERSPEED!!!");
        assert_eq!(sensitivity_name(0.0), "*yawn*");
        assert_eq!(brightness_name(0.0), "Moody");
    }

    #[test]
    fn escape_and_done_go_up_a_page_at_a_time() {
        let mut screen = OptionsScreen::new(Page::KeyBinds);
        screen.waiting = Some(Action::Jump);
        assert!(!screen.back(), "the first Escape only stops waiting");
        assert_eq!((screen.page, screen.waiting), (Page::KeyBinds, None));
        assert!(!screen.back());
        assert_eq!(screen.page, Page::Controls);
        assert!(!screen.back());
        assert_eq!(screen.page, Page::Main);
        assert!(screen.back(), "Escape on the main page leaves");
    }

    #[test]
    fn binding_sets_the_waiting_key_and_mutes_the_mouse_until_let_go() {
        let mut keys = Bindings::default();
        let mut screen = OptionsScreen::new(Page::KeyBinds);
        screen.waiting = Some(Action::Jump);
        screen.bind(
            &mut keys,
            Some(Input::Mouse(winit::event::MouseButton::Back)),
        );
        assert_eq!(
            keys.get(Action::Jump),
            Some(Input::Mouse(winit::event::MouseButton::Back))
        );
        assert!(screen.muted && !screen.unmute_after_frame);
        screen.mouse_released();
        assert!(screen.unmute_after_frame);
        // Escape unbinds; with nothing waiting, keys are left alone.
        screen.waiting = Some(Action::Drop);
        screen.bind(&mut keys, None);
        assert_eq!(keys.get(Action::Drop), None);
        screen.bind(&mut keys, Some(Input::Key(KeyCode::KeyZ)));
        assert_eq!(keys.actions(Input::Key(KeyCode::KeyZ)).count(), 0);
    }
}
