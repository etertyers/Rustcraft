//! On-screen UI drawn with egui: the main menu, the HUD and the F3 debug overlay.

mod fps;
pub mod glint;
pub mod gui;
pub mod menu;
pub mod packs;
pub mod pause;
pub mod screens;

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
    /// (sky, block) light where the player stands, once its chunk is drawn.
    pub light: Option<(u8, u8)>,
    pub mode: GameMode,
    /// Game time in ticks.
    pub time: u64,
}

/// Always-on HUD pieces.
pub struct Hud {
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
        res: &crate::pack::Resources,
    ) -> anyhow::Result<Self> {
        let ctx = egui::Context::default();
        let textures = GuiTextures::load(&ctx, atlas, res)?;
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
        res: &crate::pack::Resources,
    ) -> anyhow::Result<()> {
        self.textures = GuiTextures::load(&self.ctx, atlas, res)?;
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
    pub fn frame(
        &mut self,
        window: &Window,
        camera: &Camera,
        stats: &DebugStats,
        hud: &Hud,
        screen: Option<(&mut screens::Screen, &screens::ScreenView)>,
        pause: Option<(&mut pause::PauseMenu, &mut Options)>,
    ) -> (UiPaint, Vec<screens::ScreenAction>) {
        let input = self.state.take_egui_input(window);
        let visible = self.visible;
        let text = debug_text(self.fps.fps(), camera, stats);
        let textures = &self.textures;
        let mut actions = Vec::new();
        let mut screen = screen;
        let mut pause = pause;
        let output = self.ctx.run_ui(input, |ui| {
            let full = ui.ctx().content_rect();
            if hud.crosshair {
                draw_crosshair(ui);
                if let Some(indicator) = hud.attack {
                    gui::paint_attack_indicator(ui.painter(), full, indicator, textures);
                }
            }
            gui::paint_hotbar(ui.painter(), full, &hud.hotbar, hud.selected, textures);
            if let Some(offhand) = &hud.offhand {
                gui::paint_offhand(ui.painter(), full, offhand, textures);
            }
            if let Some(survival) = &hud.survival {
                gui::paint_survival(ui.painter(), full, survival, textures);
            }
            if visible {
                show_overlay(ui.ctx(), &text);
            }
            if let Some((screen, view)) = &mut screen {
                actions = screens::show(ui.ctx(), screen, view, textures);
            }
            if let Some(message) = &hud.message {
                paint_message(ui.painter(), full, message);
            }
            if let Some(message) = &hud.death
                && show_death_screen(ui.ctx(), message)
            {
                actions.push(screens::ScreenAction::Respawn);
            }
            if let Some((menu, options)) = &mut pause
                && let Some(action) = menu.show(ui.ctx(), options)
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
        (self.paint(output), actions)
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
         Biome: {}\n\
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
                respawn = ui.add(button).clicked();
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
            leave = ui.add(button).clicked();
        });
    leave
}

/// A notice above the hotbar, where 1.0.0's chat shows (bottom left).
fn paint_message(painter: &egui::Painter, full: egui::Rect, text: &str) {
    let scale = gui::gui_scale(full);
    let font = egui::FontId::proportional(8.0 * scale);
    let galley = painter.layout_no_wrap(text.to_owned(), font, Color32::WHITE);
    let at = egui::pos2(full.left() + 2.0 * scale, full.bottom() - 48.0 * scale);
    let rect = egui::Rect::from_min_size(at, galley.size()).expand(scale);
    painter.rect_filled(rect, 0.0, Color32::from_black_alpha(128));
    painter.galley(at, galley, Color32::WHITE);
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
            light: Some((4, 11)),
            mode: GameMode::Creative,
            time: 6000,
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
        assert!(text.contains("Light: 11 (4 sky, 11 block)"), "{text}");
        assert!(text.contains("Seed: 42"), "{text}");
        assert!(text.contains("Mode: Creative"), "{text}");
        assert!(text.contains("Time: day 1, 12:00"), "{text}");
    }
}
