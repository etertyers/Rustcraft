//! On-screen UI drawn with egui. For now only the F3 debug overlay.

mod fps;

use std::time::Instant;

use egui::{Align2, Color32, Frame, RichText};
use winit::{event::WindowEvent, window::Window};

use crate::{
    render::{UiPaint, camera::Camera},
    world::biome::Biome,
};
use fps::FpsCounter;

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
}

/// Always-on HUD pieces.
pub struct Hud {
    /// Shown while the cursor is grabbed for play.
    pub crosshair: bool,
    /// The selected hotbar block, shown at the bottom until the real hotbar (M7).
    pub selected: String,
}

pub struct DebugUi {
    ctx: egui::Context,
    state: egui_winit::State,
    fps: FpsCounter,
    /// Whether the F3 overlay is shown.
    visible: bool,
}

impl DebugUi {
    pub fn new(window: &Window) -> Self {
        let ctx = egui::Context::default();
        let state = egui_winit::State::new(
            ctx.clone(),
            egui::ViewportId::ROOT,
            window,
            Some(window.scale_factor() as f32),
            None,
            None,
        );
        Self {
            ctx,
            state,
            fps: FpsCounter::new(Instant::now()),
            visible: false,
        }
    }

    /// Feeds a window event to egui so it tracks size and scale.
    pub fn on_window_event(&mut self, window: &Window, event: &WindowEvent) {
        // The overlay isn't interactive, so whether egui consumed the event doesn't matter.
        let _ = self.state.on_window_event(window, event);
    }

    pub fn toggle(&mut self) {
        self.visible = !self.visible;
    }

    /// Counts a frame that reached the screen, for the FPS readout.
    pub fn frame_presented(&mut self, now: Instant) {
        self.fps.tick(now);
    }

    /// Builds this frame's UI: the crosshair (while playing) and the F3 overlay.
    pub fn frame(
        &mut self,
        window: &Window,
        camera: &Camera,
        stats: &DebugStats,
        hud: &Hud,
    ) -> UiPaint {
        let input = self.state.take_egui_input(window);
        let visible = self.visible;
        let text = debug_text(self.fps.fps(), camera, stats);
        let output = self.ctx.run_ui(input, |ui| {
            if hud.crosshair {
                draw_crosshair(ui);
            }
            show_selected(ui.ctx(), &hud.selected);
            if visible {
                show_overlay(ui.ctx(), &text);
            }
        });
        // egui's platform output (cursor icon, clipboard) is ignored: applying
        // it would unhide the cursor while it's grabbed for mouse look.
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
         Seed: {}",
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

fn show_selected(ctx: &egui::Context, text: &str) {
    egui::Area::new(egui::Id::unique("selected"))
        .anchor(Align2::CENTER_BOTTOM, [0.0, -24.0])
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
    }
}
