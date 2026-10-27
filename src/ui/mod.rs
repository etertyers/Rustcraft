//! On-screen UI drawn with egui: the main menu, the HUD and the F3 debug overlay.

pub mod chat;
pub mod creative;
pub mod doll;
pub mod font;
mod fps;
pub mod geometry;
pub mod glint;
pub mod gui;
pub mod menu;
pub mod options;
pub mod packs;
pub mod pause;
pub mod recipe_book;
pub mod screens;
pub mod stack;
pub mod text_field;

use std::time::Instant;

use egui::{Align2, Color32, Frame, RichText};
use winit::{event::WindowEvent, window::Window};

use crate::{
    options::Options,
    player::mode::GameMode,
    render::{UiPaint, camera::Camera},
    world::{biome::Biome, daytime},
};
use fps::FpsCounter;
use gui::{GuiTextures, SlotView, SurvivalHud};
use menu::{Menu, MenuAction};

/// A click on an interface button, with the click sound.
pub trait ClickSound {
    fn clicked_with_sound(&self) -> bool;
}

impl ClickSound for egui::Response {
    fn clicked_with_sound(&self) -> bool {
        let clicked = self.clicked();
        if clicked {
            crate::audio::play_here("ui.button.click");
        }
        clicked
    }
}

/// World and renderer numbers shown in the F3 overlay.
#[derive(Debug, Default, Clone, Copy)]
pub struct DebugStats {
    /// Chunk meshes drawn after frustum culling.
    pub chunks_drawn: usize,
    /// Chunks in memory.
    pub chunks_loaded: usize,
    /// Biome at the camera, if its chunk is loaded.
    pub biome: Option<Biome>,
    /// World seed, for generated worlds.
    pub seed: Option<u64>,
    /// The climate parameters where the player stands, in a generated world.
    pub climate: Option<crate::world::worldgen::climate::Climate>,
    /// (sky, block) light where the player stands, once its chunk is drawn.
    pub light: Option<(u8, u8)>,
    pub mode: GameMode,
    /// Game time in ticks.
    pub time: u64,
    pub dimension: crate::world::dimension::Dimension,
}

/// Always-on HUD pieces.
pub struct Hud {
    /// Hide GUI (F1): no crosshair, hotbar, hearts or effects.
    pub hidden: bool,
    /// Other players' names over their heads: where on screen (0–1 across
    /// and down), and how far off (nearer drawn bigger).
    pub name_tags: Vec<NameTag>,
    /// Everyone playing, while List Players (Tab) is held.
    pub player_list: Option<Vec<String>>,
    /// Shown while the cursor is grabbed for play.
    pub crosshair: bool,
    /// The swing's recharge under the crosshair.
    pub attack: Option<gui::AttackIndicator>,
    pub hotbar: Vec<Option<SlotView>>,
    /// What the off-hand holds.
    pub offhand: Option<SlotView>,
    pub selected: usize,
    /// Hearts and food, in survival.
    pub survival: Option<SurvivalHud>,
    /// Set while dead: how the player died.
    pub death: Option<String>,
    /// Set while in bed: how far asleep, 0–1.
    pub sleep: Option<f32>,
    /// A notice shown above the hotbar.
    pub message: Option<String>,
    /// Lines mods draw above the hotbar, centered (Fullbright's "ON").
    pub mod_lines: Vec<String>,
    /// How strongly a portal's swirl covers the screen, 0–1.
    pub portal: f32,
    /// Set while the chunks around a player who changed dimension load.
    pub loading: bool,
    /// The player's status effects (M26).
    pub effects: Vec<EffectView>,
    /// The dragon's health as a fraction, for the boss bar (M27).
    pub boss: Option<f32>,
    /// Ticks the poem and credits have rolled, while they do (M27).
    pub credits: Option<u32>,
    /// Advancement toasts, newest last (M29).
    pub toasts: Vec<ToastView>,
    /// The advancements screen, while it is open.
    pub advancements: Option<AdvView>,
}

/// A player's name over their head.
#[derive(Debug, Clone, PartialEq)]
pub struct NameTag {
    pub name: String,
    pub at: egui::Pos2,
    pub distance: f32,
}

/// An advancement just made, as its toast shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct ToastView {
    pub header: &'static str,
    pub title: String,
    pub icon: Option<SlotView>,
    pub frame: crate::advancements::Frame,
    /// 0 when it has just slid out of the screen's edge, 1 fully in.
    pub slide: f32,
}

/// One tab along the top of the advancements screen.
#[derive(Debug, Clone, PartialEq)]
pub struct TabView {
    pub title: &'static str,
    pub icon: Option<SlotView>,
    pub done: usize,
    pub total: usize,
}

/// A node of the open tab's tree.
#[derive(Debug, Clone, PartialEq)]
pub struct NodeView {
    pub column: u32,
    pub row: f32,
    pub icon: Option<SlotView>,
    pub frame: crate::advancements::Frame,
    pub title: String,
    pub description: String,
    pub done: bool,
    /// Its parent isn't done yet.
    pub locked: bool,
    /// Criteria met and how many there are.
    pub met: (usize, usize),
    /// Index in `nodes` of its parent.
    pub parent: Option<usize>,
}

/// The advancements screen: the tabs, which is open and its nodes.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AdvView {
    pub tabs: Vec<TabView>,
    pub current: usize,
    pub nodes: Vec<NodeView>,
}

/// A status effect as the HUD shows it: its icon (by place in
/// `Effect::ALL`), color and time left.
#[derive(Debug, Clone, PartialEq)]
pub struct EffectView {
    pub index: usize,
    pub color: u32,
    pub timer: String,
    /// In its last ten seconds the icon blinks: hidden this frame.
    pub blinked: bool,
}

pub struct DebugUi {
    ctx: egui::Context,
    textures: GuiTextures,
    state: egui_winit::State,
    fps: FpsCounter,
    /// Whether the F3 overlay is shown.
    visible: bool,
}

impl DebugUi {
    /// `atlas` is the block and item texture atlas, for item icons; GUI
    /// images come from `res`.
    pub fn new(
        window: &Window,
        atlas: &image::RgbaImage,
        figure: crate::look::FigureImages,
        res: &crate::pack::Resources,
    ) -> anyhow::Result<Self> {
        let ctx = egui::Context::default();
        let textures = GuiTextures::load(&ctx, atlas, figure, res)?;
        font::install(&ctx, res);
        let state = egui_winit::State::new(
            ctx.clone(),
            egui::ViewportId::ROOT,
            window,
            Some(window.scale_factor() as f32),
            None,
            None,
        );
        Ok(Self {
            ctx,
            textures,
            state,
            fps: FpsCounter::new(Instant::now()),
            visible: false,
        })
    }

    /// Reloads the GUI images after the resource packs change.
    pub fn set_textures(
        &mut self,
        atlas: &image::RgbaImage,
        figure: crate::look::FigureImages,
        res: &crate::pack::Resources,
    ) -> anyhow::Result<()> {
        self.textures = GuiTextures::load(&self.ctx, atlas, figure, res)?;
        font::install(&self.ctx, res);
        Ok(())
    }

    /// The egui context, for loading pack icons.
    pub fn context(&self) -> &egui::Context {
        &self.ctx
    }

    /// Feeds a window event to egui so it tracks size and scale.
    pub fn on_window_event(&mut self, window: &Window, event: &WindowEvent) {
        // The overlay isn't interactive, so whether egui consumed the event doesn't matter.
        let _ = self.state.on_window_event(window, event);
    }

    /// Moves egui's pointer to `pos` (in points), for when the real cursor
    /// was put there without a mouse event.
    pub fn point_at(&mut self, pos: egui::Pos2) {
        self.state
            .egui_input_mut()
            .events
            .push(egui::Event::PointerMoved(pos));
    }

    pub fn toggle(&mut self) {
        self.visible = !self.visible;
    }

    pub fn is_visible(&self) -> bool {
        self.visible
    }

    /// Counts a frame that reached the screen, for the FPS readout.
    pub fn frame_presented(&mut self, now: Instant) {
        self.fps.tick(now);
    }

    /// Builds this frame's UI: the crosshair (while playing), the hotbar,
    /// the F3 overlay and any open inventory screen, returning what was
    /// clicked on the screen.
    #[allow(clippy::too_many_arguments)]
    pub fn frame(
        &mut self,
        window: &Window,
        camera: &Camera,
        stats: &DebugStats,
        hud: &Hud,
        screen: Option<(&mut screens::Screen, &screens::ScreenView)>,
        pause: Option<(
            &mut pause::PauseMenu,
            &mut Options,
            &mut crate::level::Difficulty,
        )>,
        chat: Option<(&mut chat::Chat, &chat::ChatSettings)>,
    ) -> (
        UiPaint,
        Vec<screens::ScreenAction>,
        Option<chat::ChatAction>,
    ) {
        let input = self.state.take_egui_input(window);
        let visible = self.visible;
        let text = debug_text(self.fps.fps(), camera, stats);
        let textures = &self.textures;
        let mut actions = Vec::new();
        let mut screen = screen;
        let mut pause = pause;
        let mut chat = chat;
        let mut chat_action = None;
        let output = self.ctx.run_ui(input, |ui| {
            let full = ui.ctx().content_rect();
            if hud.crosshair && !hud.hidden {
                draw_crosshair(ui);
                if let Some(indicator) = hud.attack {
                    gui::paint_attack_indicator(ui.painter(), full, indicator, textures);
                }
            }
            if !hud.hidden {
                gui::paint_hotbar(ui.painter(), full, &hud.hotbar, hud.selected, textures);
            }
            if let Some(offhand) = &hud.offhand
                && !hud.hidden
            {
                gui::paint_offhand(ui.painter(), full, offhand, textures);
            }
            if let Some(survival) = &hud.survival
                && !hud.hidden
            {
                gui::paint_survival(ui.painter(), full, survival, textures);
            }
            if visible {
                show_overlay(ui.ctx(), &text);
            }
            if let Some((screen, view)) = &mut screen {
                actions = screens::show(ui.ctx(), screen, view, textures);
            }
            if hud.portal > 0.0 {
                paint_portal(ui.ctx(), hud.portal);
            }
            if !hud.effects.is_empty() && !hud.hidden {
                paint_effects(ui.painter(), full, &hud.effects, textures);
            }
            if !hud.hidden {
                paint_name_tags(ui.painter(), full, &hud.name_tags);
            }
            if let Some((chat, settings)) = &mut chat {
                chat_action = chat.show(ui.ctx(), settings);
            }
            if let Some(names) = &hud.player_list {
                paint_player_list(ui.painter(), full, names);
            }
            if let Some(message) = &hud.message {
                paint_message(ui.painter(), full, message);
            }
            if !hud.mod_lines.is_empty() && !hud.hidden {
                paint_mod_lines(ui.painter(), full, &hud.mod_lines);
            }
            if let Some(health) = hud.boss {
                paint_boss_bar(ui.painter(), full, "Ender Dragon", health);
            }
            if !hud.toasts.is_empty() {
                paint_toasts(ui.painter(), full, &hud.toasts, textures);
            }
            if let Some(view) = &hud.advancements
                && let Some(tab) = show_advancements(ui.ctx(), view, textures)
            {
                actions.push(screens::ScreenAction::AdvTab(tab));
            }
            if hud.loading {
                show_loading_screen(ui.ctx());
            }
            if let Some(ticks) = hud.credits {
                show_credits(ui.ctx(), ticks);
            }
            if let Some(message) = &hud.death
                && show_death_screen(ui.ctx(), message)
            {
                actions.push(screens::ScreenAction::Respawn);
            }
            if let Some((menu, options, difficulty)) = &mut pause
                && let Some(action) = menu.show(ui.ctx(), options, difficulty)
            {
                actions.push(screens::ScreenAction::Pause(action));
            }
            if let Some(asleep) = hud.sleep
                && show_sleep_screen(ui.ctx(), asleep)
            {
                actions.push(screens::ScreenAction::LeaveBed);
            }
        });
        // egui's platform output (cursor icon, clipboard) is ignored: applying
        // it would unhide the cursor while it's grabbed for mouse look.
        (self.paint(output), actions, chat_action)
    }

    /// Builds this frame's main menu and returns what the player chose.
    pub fn menu_frame(
        &mut self,
        window: &Window,
        menu: &mut Menu,
        options: &mut Options,
    ) -> (UiPaint, Option<MenuAction>) {
        let input = self.state.take_egui_input(window);
        let mut action = None;
        let output = self.ctx.run_ui(input, |ui| action = menu.show(ui, options));
        // The cursor is free in the menu, so let egui set it and use the clipboard.
        self.state
            .handle_platform_output(window, output.platform_output.clone());
        (self.paint(output), action)
    }

    fn paint(&self, output: egui::FullOutput) -> UiPaint {
        UiPaint {
            primitives: self.ctx.tessellate(output.shapes, output.pixels_per_point),
            textures: output.textures_delta,
            pixels_per_point: output.pixels_per_point,
        }
    }
}

fn debug_text(fps: f32, camera: &Camera, stats: &DebugStats) -> String {
    let p = camera.position;
    let block = p.floor();
    format!(
        "Rustcraft\n\
         {fps:.0} fps, chunks: {} drawn, {} loaded\n\
         XYZ: {:.3} / {:.3} / {:.3}\n\
         Block: {} {} {}\n\
         Facing: {} ({:.1} / {:.1})\n\
         Biome: {} ({})\n\
         {}\n\
         Light: {}\n\
         Seed: {}\n\
         Mode: {}\n\
         Time: {}",
        stats.chunks_drawn,
        stats.chunks_loaded,
        p.x,
        p.y,
        p.z,
        block.x,
        block.y,
        block.z,
        camera.facing().describe(),
        camera.yaw.to_degrees(),
        camera.pitch.to_degrees(),
        stats.biome.map_or("-", Biome::name),
        stats.dimension.name(),
        stats.climate.map_or_else(
            || "Climate: -".to_owned(),
            |c| format!(
                "T {:.2} H {:.2} C {:.2} E {:.2} W {:.2} PV {:.2}",
                c.temperature,
                c.humidity,
                c.continent,
                c.erosion,
                c.weirdness,
                c.peaks()
            )
        ),
        stats.light.map_or_else(
            || "-".to_owned(),
            |(sky, block)| format!("{} ({sky} sky, {block} block)", sky.max(block))
        ),
        stats.seed.map_or_else(|| "-".to_owned(), |s| s.to_string()),
        stats.mode.name(),
        daytime::clock(stats.time),
    )
}

/// A small white plus at the screen center, outlined so it shows on bright blocks too.
fn draw_crosshair(ui: &egui::Ui) {
    const HALF: f32 = 5.0;
    let center = ui.ctx().content_rect().center();
    let painter = ui.painter();
    for (width, color) in [(3.0, Color32::from_black_alpha(140)), (1.5, Color32::WHITE)] {
        let stroke = egui::Stroke::new(width, color);
        painter.line_segment(
            [
                center - egui::vec2(HALF, 0.0),
                center + egui::vec2(HALF, 0.0),
            ],
            stroke,
        );
        painter.line_segment(
            [
                center - egui::vec2(0.0, HALF),
                center + egui::vec2(0.0, HALF),
            ],
            stroke,
        );
    }
}

/// Status effects in a row from the top-right corner, each its icon in a
/// dark frame with the time left under it (modern draws the icons there;
/// the inventory screen lists the times, shown here at once instead). An
/// effect in its last ten seconds blinks.
fn paint_effects(
    painter: &egui::Painter,
    full: egui::Rect,
    effects: &[EffectView],
    textures: &gui::GuiTextures,
) {
    let scale = gui::gui_scale(full);
    for (i, effect) in effects.iter().enumerate() {
        let corner = egui::pos2(
            full.right() - (i as f32 + 1.0) * 26.0 * scale,
            full.top() + 2.0 * scale,
        );
        let frame_rect = egui::Rect::from_min_size(corner, egui::vec2(24.0, 32.0) * scale);
        painter.rect_filled(
            frame_rect,
            2.0 * scale,
            Color32::from_rgba_unmultiplied(0, 0, 0, 120),
        );
        let icon = egui::Rect::from_min_size(
            corner + egui::vec2(3.0, 3.0) * scale,
            egui::vec2(18.0, 18.0) * scale,
        );
        if !effect.blinked {
            match textures.effects.get(effect.index).and_then(Option::as_ref) {
                Some(texture) => {
                    let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
                    painter.image(texture.id(), icon, uv, Color32::WHITE);
                }
                None => {
                    let c = effect.color;
                    let color = Color32::from_rgb((c >> 16) as u8, (c >> 8) as u8, c as u8);
                    painter.rect_filled(icon.shrink(3.0 * scale), 2.0 * scale, color);
                }
            }
        }
        painter.text(
            egui::pos2(frame_rect.center().x, icon.bottom() + 1.0 * scale),
            Align2::CENTER_TOP,
            &effect.timer,
            egui::FontId::proportional(8.0 * scale),
            Color32::WHITE,
        );
    }
}

/// The swirl over the screen while standing in a Nether portal: modern
/// draws the portal texture over everything, fading in as the wait goes
/// on; this tints the screen its purple.
fn paint_portal(ctx: &egui::Context, strength: f32) {
    let full = ctx.content_rect();
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Background,
        egui::Id::unique("portal"),
    ));
    // Modern squares the strength so it starts faint.
    let alpha = (strength * strength * 0.8 + 0.2 * strength) * 200.0;
    painter.rect_filled(
        full,
        0.0,
        Color32::from_rgba_unmultiplied(110, 30, 170, alpha as u8),
    );
}

/// The advancement toasts stacked down the top right corner (modern
/// `ToastComponent`): a dark card with the header in yellow, the title
/// under it and the icon at the left.
fn paint_toasts(
    painter: &egui::Painter,
    full: egui::Rect,
    toasts: &[ToastView],
    tex: &GuiTextures,
) {
    let scale = gui::gui_scale(full);
    let (w, h) = (160.0 * scale, 32.0 * scale);
    for (i, toast) in toasts.iter().rev().take(5).enumerate() {
        let x = full.right() - w * toast.slide.clamp(0.0, 1.0);
        let top = 4.0 * scale + i as f32 * (h + 2.0 * scale);
        let card = egui::Rect::from_min_size(egui::pos2(x, top), egui::vec2(w, h));
        painter.rect_filled(card, 0.0, Color32::from_rgb(34, 34, 34));
        painter.rect_stroke(
            card,
            0.0,
            egui::Stroke::new(scale, frame_color(toast.frame)),
            egui::StrokeKind::Inside,
        );
        painter.text(
            card.min + egui::vec2(30.0, 7.0) * scale,
            Align2::LEFT_TOP,
            toast.header,
            egui::FontId::proportional(8.0 * scale),
            if toast.frame == crate::advancements::Frame::Challenge {
                Color32::from_rgb(255, 85, 255)
            } else {
                Color32::from_rgb(255, 255, 85)
            },
        );
        painter.text(
            card.min + egui::vec2(30.0, 18.0) * scale,
            Align2::LEFT_TOP,
            &toast.title,
            egui::FontId::proportional(8.0 * scale),
            Color32::WHITE,
        );
        if let Some(icon) = &toast.icon {
            gui::paint_slot(
                painter,
                card.min + egui::vec2(8.0, 8.0) * scale,
                icon,
                scale,
                tex.atlas.id(),
            );
        }
    }
}

/// The frame's color: gray for a task, gold for a goal, purple for a
/// challenge.
fn frame_color(frame: crate::advancements::Frame) -> Color32 {
    match frame {
        crate::advancements::Frame::Task => Color32::from_rgb(160, 160, 160),
        crate::advancements::Frame::Goal => Color32::from_rgb(230, 190, 60),
        crate::advancements::Frame::Challenge => Color32::from_rgb(170, 70, 210),
    }
}

/// The advancements screen (L): tabs along the top with what's done in
/// each, the open tab's tree of framed icons joined by lines, and a card
/// for the one under the mouse. Returns a tab the player clicked.
fn show_advancements(ctx: &egui::Context, view: &AdvView, tex: &GuiTextures) -> Option<usize> {
    let full = ctx.content_rect();
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Foreground,
        egui::Id::unique("advancements"),
    ));
    painter.rect_filled(full, 0.0, Color32::from_black_alpha(200));
    let scale = gui::gui_scale(full);
    let pointer = ctx.input(|i| i.pointer.hover_pos());
    let pressed = ctx.input(|i| i.pointer.primary_pressed());
    // The window grows to hold the tree (nodes 28 across and 26 down), as far
    // as the screen allows.
    let columns = view.nodes.iter().map(|n| n.column).max().unwrap_or(0) as f32;
    let rows = view.nodes.iter().map(|n| n.row).fold(0.0_f32, f32::max);
    let width = (columns * 28.0 + 22.0 + 28.0)
        .max(252.0)
        .min(full.width() / scale - 16.0);
    let height = (rows * 26.0 + 22.0 + 46.0)
        .max(140.0)
        .min(full.height() / scale - 64.0);
    let window = egui::Rect::from_center_size(
        full.center() + egui::vec2(0.0, 12.0 * scale),
        egui::vec2(width, height) * scale,
    );
    painter.rect_filled(window, 0.0, Color32::from_rgb(28, 24, 36));
    painter.rect_stroke(
        window,
        0.0,
        egui::Stroke::new(scale, Color32::from_rgb(90, 80, 110)),
        egui::StrokeKind::Outside,
    );
    let mut clicked = None;
    // The tabs, left to right above the window.
    for (i, tab) in view.tabs.iter().enumerate() {
        let rect = egui::Rect::from_min_size(
            window.min + egui::vec2(i as f32 * 29.0, -29.0) * scale,
            egui::vec2(28.0, 28.0) * scale,
        );
        let selected = i == view.current;
        painter.rect_filled(
            rect,
            0.0,
            if selected {
                Color32::from_rgb(70, 60, 90)
            } else {
                Color32::from_rgb(40, 36, 52)
            },
        );
        if let Some(icon) = &tab.icon {
            gui::paint_slot(
                &painter,
                rect.min + egui::vec2(6.0, 6.0) * scale,
                icon,
                scale,
                tex.atlas.id(),
            );
        }
        if pointer.is_some_and(|p| rect.contains(p)) {
            painter.rect_filled(rect, 0.0, Color32::from_white_alpha(40));
            if pressed {
                clicked = Some(i);
            }
        }
    }
    if let Some(tab) = view.tabs.get(view.current) {
        painter.text(
            window.min + egui::vec2(8.0, 6.0) * scale,
            Align2::LEFT_TOP,
            format!("{}  {}/{}", tab.title, tab.done, tab.total),
            egui::FontId::proportional(8.0 * scale),
            Color32::WHITE,
        );
    }
    // The tree, nodes 28 pixels across and 27 down, squeezed to fit.
    let step_x = (28.0_f32).min((width - 50.0) / columns.max(1.0));
    let step_y = (26.0_f32).min((height - 46.0 - 22.0) / rows.max(1.0));
    let at = |n: &NodeView| {
        window.min + egui::vec2(14.0 + n.column as f32 * step_x, 30.0 + n.row * step_y) * scale
    };
    for node in &view.nodes {
        if let Some(parent) = node.parent.and_then(|p| view.nodes.get(p)) {
            let (a, b) = (
                at(parent) + egui::vec2(11.0, 11.0) * scale,
                at(node) + egui::vec2(11.0, 11.0) * scale,
            );
            let color = if node.done {
                Color32::from_rgb(230, 230, 230)
            } else {
                Color32::from_rgb(80, 70, 100)
            };
            painter.line_segment(
                [a, egui::pos2(b.x - (b.x - a.x) / 2.0, a.y)],
                egui::Stroke::new(2.0 * scale, color),
            );
            painter.line_segment(
                [
                    egui::pos2(b.x - (b.x - a.x) / 2.0, a.y),
                    egui::pos2(b.x - (b.x - a.x) / 2.0, b.y),
                ],
                egui::Stroke::new(2.0 * scale, color),
            );
            painter.line_segment(
                [egui::pos2(b.x - (b.x - a.x) / 2.0, b.y), b],
                egui::Stroke::new(2.0 * scale, color),
            );
        }
    }
    let mut hovered = None;
    for (i, node) in view.nodes.iter().enumerate() {
        let rect = egui::Rect::from_min_size(at(node), egui::vec2(22.0, 22.0) * scale);
        let color = if node.done {
            frame_color(node.frame)
        } else if node.locked {
            Color32::from_rgb(50, 46, 60)
        } else {
            Color32::from_rgb(90, 84, 110)
        };
        painter.rect_filled(rect, 0.0, Color32::from_rgb(20, 18, 26));
        painter.rect_stroke(
            rect,
            0.0,
            egui::Stroke::new(2.0 * scale, color),
            egui::StrokeKind::Inside,
        );
        if let Some(icon) = &node.icon {
            gui::paint_slot(
                &painter,
                rect.min + egui::vec2(3.0, 3.0) * scale,
                icon,
                scale,
                tex.atlas.id(),
            );
        }
        if !node.done {
            painter.rect_filled(
                rect,
                0.0,
                Color32::from_black_alpha(if node.locked { 150 } else { 90 }),
            );
        }
        if pointer.is_some_and(|p| rect.contains(p)) {
            hovered = Some(i);
        }
    }
    if let Some(node) = hovered.and_then(|i| view.nodes.get(i)) {
        let text = format!(
            "{}\n{}{}",
            node.title,
            node.description,
            if node.met.1 > 1 {
                format!("\n{}/{}", node.met.0, node.met.1)
            } else {
                String::new()
            }
        );
        if let Some(p) = pointer {
            let galley = painter.layout(
                text,
                egui::FontId::proportional(8.0 * scale),
                Color32::WHITE,
                160.0 * scale,
            );
            let card = egui::Rect::from_min_size(
                p + egui::vec2(8.0, 8.0) * scale,
                galley.size() + egui::vec2(8.0, 6.0) * scale,
            );
            painter.rect_filled(card, 0.0, Color32::from_rgb(16, 10, 24));
            painter.rect_stroke(
                card,
                0.0,
                egui::Stroke::new(scale, frame_color(node.frame)),
                egui::StrokeKind::Inside,
            );
            painter.galley(
                card.min + egui::vec2(4.0, 3.0) * scale,
                galley,
                Color32::WHITE,
            );
        }
    }
    painter.text(
        egui::pos2(full.center().x, window.bottom() + 6.0 * scale),
        Align2::CENTER_TOP,
        "L or Esc to close",
        egui::FontId::proportional(8.0 * scale),
        Color32::from_gray(120),
    );
    clicked
}

/// A boss's name over a bar across the top of the screen (modern's boss
/// bar: purple for the dragon), `health` of it full.
fn paint_boss_bar(painter: &egui::Painter, full: egui::Rect, name: &str, health: f32) {
    let scale = gui::gui_scale(full);
    let width = 182.0 * scale;
    let left = full.center().x - width / 2.0;
    let top = 12.0 * scale;
    painter.text(
        egui::pos2(full.center().x, top - 2.0 * scale),
        Align2::CENTER_BOTTOM,
        name,
        egui::FontId::proportional(8.0 * scale),
        Color32::WHITE,
    );
    let bar = egui::Rect::from_min_size(egui::pos2(left, top), egui::vec2(width, 5.0 * scale));
    painter.rect_filled(bar.expand(scale), 0.0, Color32::from_rgb(20, 8, 30));
    painter.rect_filled(bar, 0.0, Color32::from_rgb(58, 30, 78));
    let filled = egui::Rect::from_min_size(
        bar.min,
        egui::vec2(width * health.clamp(0.0, 1.0), 5.0 * scale),
    );
    painter.rect_filled(filled, 0.0, Color32::from_rgb(168, 60, 214));
    // Notches every tenth, as the notched style has.
    for tenth in 1..10 {
        let x = left + width * tenth as f32 / 10.0;
        painter.line_segment(
            [egui::pos2(x, bar.top()), egui::pos2(x, bar.bottom())],
            egui::Stroke::new(scale.max(1.0) * 0.5, Color32::from_rgb(20, 8, 30)),
        );
    }
}

/// The poem and credits: white-on-black lines scrolling up the screen
/// (modern's `WinScreen`). `ticks` is how long they have rolled.
fn show_credits(ctx: &egui::Context, ticks: u32) {
    use crate::credits::{LINE_TICKS, LINES, Line};
    let full = ctx.content_rect();
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Foreground,
        egui::Id::unique("credits"),
    ));
    painter.rect_filled(full, 0.0, Color32::BLACK);
    let scale = gui::gui_scale(full);
    let step = 12.0 * scale;
    // Line `i` enters at the bottom at tick `i * LINE_TICKS` and climbs a
    // line's height every `LINE_TICKS`.
    let rolled = ticks as f32 / LINE_TICKS as f32 * step;
    for (i, line) in LINES.iter().enumerate() {
        let y = full.bottom() + i as f32 * step - rolled;
        if y < -step || y > full.bottom() + step {
            continue;
        }
        let (text, size, color) = match *line {
            Line::Poem(text) => (text, 8.0, Color32::from_rgb(170, 205, 170)),
            Line::Heading(text) => (text, 16.0, Color32::from_rgb(255, 214, 100)),
            Line::Credit(text) => (text, 8.0, Color32::WHITE),
            Line::Gap => continue,
        };
        painter.text(
            egui::pos2(full.center().x, y),
            Align2::CENTER_TOP,
            text,
            egui::FontId::proportional(size * scale),
            color,
        );
    }
    painter.text(
        egui::pos2(full.right() - 4.0 * scale, full.bottom() - 4.0 * scale),
        Align2::RIGHT_BOTTOM,
        "Esc to skip",
        egui::FontId::proportional(8.0 * scale),
        Color32::from_rgb(110, 110, 110),
    );
}

/// Shown while the chunks around a player who just went through a portal
/// load (modern "Loading terrain...").
fn show_loading_screen(ctx: &egui::Context) {
    let full = ctx.content_rect();
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Foreground,
        egui::Id::unique("loading"),
    ));
    painter.rect_filled(full, 0.0, Color32::from_rgb(40, 28, 20));
    let scale = gui::gui_scale(full);
    painter.text(
        full.center(),
        Align2::CENTER_CENTER,
        "Loading terrain...",
        egui::FontId::proportional(8.0 * scale),
        Color32::WHITE,
    );
}

/// 1.0.0's death screen: the world tinted red, "You died!" and a Respawn
/// button. Returns true when Respawn is clicked.
fn show_death_screen(ctx: &egui::Context, message: &str) -> bool {
    let full = ctx.content_rect();
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Background,
        egui::Id::unique("death"),
    ));
    painter.rect_filled(full, 0.0, Color32::from_rgba_unmultiplied(128, 0, 0, 110));
    let scale = gui::gui_scale(full);
    let mut respawn = false;
    egui::Area::new(egui::Id::unique("death-screen"))
        .anchor(Align2::CENTER_CENTER, [0.0, -20.0 * scale])
        .show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.label(
                    RichText::new("You died!")
                        .size(20.0 * scale)
                        .color(Color32::WHITE),
                );
                ui.add_space(4.0 * scale);
                ui.label(
                    RichText::new(message)
                        .size(7.0 * scale)
                        .color(Color32::from_gray(220)),
                );
                ui.add_space(16.0 * scale);
                let button = egui::Button::new(RichText::new("Respawn").size(8.0 * scale))
                    .min_size(egui::vec2(200.0, 20.0) * scale);
                respawn = ui.add(button).clicked_with_sound();
            });
        });
    respawn
}

/// 1.0.0's GuiSleepMP: the screen darkens as the player falls asleep,
/// with a Leave Bed button. Returns true when it's clicked.
fn show_sleep_screen(ctx: &egui::Context, asleep: f32) -> bool {
    let full = ctx.content_rect();
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Background,
        egui::Id::unique("sleep"),
    ));
    let alpha = (220.0 * asleep) as u8;
    painter.rect_filled(
        full,
        0.0,
        Color32::from_rgba_unmultiplied(16, 16, 32, alpha),
    );
    let scale = gui::gui_scale(full);
    let mut leave = false;
    egui::Area::new(egui::Id::unique("sleep-screen"))
        .anchor(Align2::CENTER_BOTTOM, [0.0, -20.0 * scale])
        .show(ctx, |ui| {
            let button = egui::Button::new(RichText::new("Leave Bed").size(8.0 * scale))
                .min_size(egui::vec2(200.0, 20.0) * scale);
            leave = ui.add(button).clicked_with_sound();
        });
    leave
}

/// A notice above the hotbar, where 1.0.0's chat shows (bottom left).
/// The Tab list: everyone playing, in a column at the top middle, on a
/// dark panel (vanilla's `PlayerTabOverlay`, one column).
fn paint_player_list(painter: &egui::Painter, full: egui::Rect, names: &[String]) {
    let scale = gui::gui_scale(full);
    let font = egui::FontId::proportional(8.0 * scale);
    let galleys: Vec<_> = names
        .iter()
        .map(|name| painter.layout_no_wrap(name.clone(), font.clone(), Color32::WHITE))
        .collect();
    let width = galleys
        .iter()
        .map(|g| g.size().x)
        .fold(80.0 * scale, f32::max);
    let row = 9.0 * scale;
    let top = full.top() + 10.0 * scale;
    let left = full.center().x - width / 2.0;
    let panel = egui::Rect::from_min_size(
        egui::pos2(left - scale, top - scale),
        egui::vec2(width + 2.0 * scale, row * names.len() as f32 + 2.0 * scale),
    );
    painter.rect_filled(panel, 0.0, Color32::from_black_alpha(128));
    for (i, galley) in galleys.into_iter().enumerate() {
        let y = top + row * i as f32;
        painter.rect_filled(
            egui::Rect::from_min_size(egui::pos2(left, y), egui::vec2(width, 8.0 * scale)),
            0.0,
            Color32::from_white_alpha(32),
        );
        painter.galley(egui::pos2(left, y), galley, Color32::WHITE);
    }
}

/// Names over players' heads, on a dark band, smaller further off.
fn paint_name_tags(painter: &egui::Painter, full: egui::Rect, tags: &[NameTag]) {
    let scale = gui::gui_scale(full);
    for tag in tags {
        let size = (8.0 * scale * (8.0 / tag.distance.max(8.0)))
            .max(8.0)
            .round();
        let font = egui::FontId::proportional(size);
        let galley = painter.layout_no_wrap(tag.name.clone(), font, Color32::WHITE);
        let center = egui::pos2(
            full.left() + tag.at.x * full.width(),
            full.top() + tag.at.y * full.height(),
        );
        let at = center - galley.size() / 2.0;
        let rect = egui::Rect::from_min_size(at, galley.size()).expand(size / 8.0);
        painter.rect_filled(rect, 0.0, Color32::from_black_alpha(64));
        painter.galley(at, galley, Color32::WHITE);
    }
}

fn paint_message(painter: &egui::Painter, full: egui::Rect, text: &str) {
    let scale = gui::gui_scale(full);
    let font = egui::FontId::proportional(8.0 * scale);
    let galley = painter.layout_no_wrap(text.to_owned(), font, Color32::WHITE);
    let at = egui::pos2(full.left() + 2.0 * scale, full.bottom() - 48.0 * scale);
    let rect = egui::Rect::from_min_size(at, galley.size()).expand(scale);
    painter.rect_filled(rect, 0.0, Color32::from_black_alpha(128));
    painter.galley(at, galley, Color32::WHITE);
}

/// What mods write above the hotbar: centered, the newest nearest it.
fn paint_mod_lines(painter: &egui::Painter, full: egui::Rect, lines: &[String]) {
    let scale = gui::gui_scale(full);
    let font = egui::FontId::proportional(8.0 * scale);
    for (i, line) in lines.iter().rev().enumerate() {
        let galley = painter.layout_no_wrap(line.clone(), font.clone(), Color32::WHITE);
        let y = full.bottom() - (72.0 + 11.0 * i as f32) * scale;
        let at = egui::pos2(full.center().x - galley.size().x / 2.0, y);
        let rect = egui::Rect::from_min_size(at, galley.size()).expand(scale);
        painter.rect_filled(rect, 0.0, Color32::from_black_alpha(128));
        painter.galley(at, galley, Color32::WHITE);
    }
}

fn show_overlay(ctx: &egui::Context, text: &str) {
    egui::Area::new(egui::Id::unique("f3"))
        .anchor(Align2::LEFT_TOP, [4.0, 4.0])
        .interactable(false)
        .show(ctx, |ui| {
            Frame::new()
                .fill(Color32::from_black_alpha(128))
                .inner_margin(4.0)
                .show(ui, |ui| {
                    ui.label(RichText::new(text).monospace().color(Color32::WHITE));
                });
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Vec3;

    #[test]
    fn debug_text_shows_position_block_and_facing() {
        let camera = Camera::new(Vec3::new(-0.5, 64.25, 3.0), 0.0, 0.0);
        let stats = DebugStats {
            chunks_drawn: 12,
            chunks_loaded: 81,
            biome: Some(Biome::Forest),
            seed: Some(42),
            climate: Some(crate::world::worldgen::climate::Climate {
                temperature: 0.5,
                weirdness: 0.0,
                ..Default::default()
            }),
            light: Some((4, 11)),
            mode: GameMode::Creative,
            time: 6000,
            dimension: Default::default(),
        };
        let text = debug_text(59.6, &camera, &stats);
        assert!(
            text.contains("60 fps, chunks: 12 drawn, 81 loaded"),
            "{text}"
        );
        assert!(text.contains("XYZ: -0.500 / 64.250 / 3.000"), "{text}");
        assert!(text.contains("Block: -1 64 3"), "{text}");
        assert!(
            text.contains("Facing: north (Towards negative Z)"),
            "{text}"
        );
        assert!(text.contains("Biome: Forest"), "{text}");
        assert!(
            text.contains("T 0.50 H 0.00 C 0.00 E 0.00 W 0.00 PV -1.00"),
            "{text}"
        );
        assert!(text.contains("Light: 11 (4 sky, 11 block)"), "{text}");
        assert!(text.contains("Seed: 42"), "{text}");
        assert!(text.contains("Mode: Creative"), "{text}");
        assert!(text.contains("Time: day 1, 12:00"), "{text}");
    }
}
