//! On-screen UI drawn with egui. For now only the F3 debug overlay.

mod fps;

use std::time::Instant;

use egui::{Align2, Color32, Frame, RichText};
use winit::{event::WindowEvent, window::Window};

use crate::render::{UiPaint, camera::Camera};
use fps::FpsCounter;

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

    /// Builds this frame's UI.
    pub fn frame(&mut self, window: &Window, camera: &Camera) -> UiPaint {
        let input = self.state.take_egui_input(window);
        let visible = self.visible;
        let text = debug_text(self.fps.fps(), camera);
        let output = self.ctx.run_ui(input, |ui| {
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

fn debug_text(fps: f32, camera: &Camera) -> String {
    let p = camera.position;
    let block = p.floor();
    format!(
        "Rustcraft\n\
         {fps:.0} fps\n\
         XYZ: {:.3} / {:.3} / {:.3}\n\
         Block: {} {} {}\n\
         Facing: {} ({:.1} / {:.1})",
        p.x,
        p.y,
        p.z,
        block.x,
        block.y,
        block.z,
        camera.facing().describe(),
        camera.yaw.to_degrees(),
        camera.pitch.to_degrees(),
    )
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
        let text = debug_text(59.6, &camera);
        assert!(text.contains("60 fps"), "{text}");
        assert!(text.contains("XYZ: -0.500 / 64.250 / 3.000"), "{text}");
        assert!(text.contains("Block: -1 64 3"), "{text}");
        assert!(
            text.contains("Facing: north (Towards negative Z)"),
            "{text}"
        );
    }
}
