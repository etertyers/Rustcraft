//! Drawing Minecraft-style GUI with the texture pack's `gui/` images:
//! the GUI scale, item icons (flat pictures or little isometric cubes),
//! stack counts and durability bars, and the hotbar.

use std::path::Path;

use anyhow::{Context, Result};
use egui::{Color32, Mesh, Painter, Pos2, Rect, Shape, TextureHandle, TextureId, pos2, vec2};
use image::RgbaImage;

use crate::{
    inventory::stack::{ItemStack, Items},
    render::icons::{IconFace, ItemIcon, ItemIcons},
};

/// What one slot holds, ready to draw.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SlotView {
    pub icon: ItemIcon,
    pub count: u8,
    /// Durability left, 0–1, once a tool or armor piece has been used.
    pub durability: Option<f32>,
}

impl SlotView {
    pub fn of(stack: &ItemStack, icons: &ItemIcons, items: &Items) -> Self {
        let durability = items
            .max_damage(stack.id)
            .filter(|_| stack.damage > 0)
            .map(|max| 1.0 - stack.damage as f32 / max as f32);
        Self {
            icon: icons.icon(stack),
            count: stack.count,
            durability,
        }
    }
}

/// GUI images loaded into egui.
pub struct GuiTextures {
    pub atlas: TextureHandle,
    pub widgets: TextureHandle,
    pub inventory: TextureHandle,
    pub creative: TextureHandle,
    pub tabs: TextureHandle,
    pub icons: TextureHandle,
}

/// GUI images are 256×256 sheets.
const SHEET: f32 = 256.0;

impl GuiTextures {
    pub fn load(ctx: &egui::Context, atlas: &RgbaImage, gui_dir: &Path) -> Result<Self> {
        let load = |name: &str, image: &RgbaImage| {
            let size = [image.width() as usize, image.height() as usize];
            let pixels = egui::ColorImage::from_rgba_unmultiplied(size, image.as_raw());
            ctx.load_texture(name, pixels, egui::TextureOptions::NEAREST)
        };
        let file = |path: &str| -> Result<TextureHandle> {
            let full = gui_dir.join(path);
            let image = image::open(&full)
                .with_context(|| format!("loading {}", full.display()))?
                .into_rgba8();
            Ok(load(path, &image))
        };
        Ok(Self {
            atlas: load("atlas", atlas),
            widgets: file("widgets.png")?,
            inventory: file("container/inventory.png")?,
            creative: file("container/creative_inventory/tab_items.png")?,
            tabs: file("container/creative_inventory/tabs.png")?,
            icons: file("icons.png")?,
        })
    }
}

/// Minecraft's "auto" GUI scale: the largest whole number of screen points
/// per GUI pixel that leaves at least 320×240 GUI pixels.
pub fn gui_scale(screen: Rect) -> f32 {
    let fit = (screen.width() / 320.0).min(screen.height() / 240.0);
    fit.floor().max(1.0)
}

/// Draws the `size` region of a 256×256 GUI sheet starting at `from`
/// (both in sheet pixels) with its top-left at `at`, `scale` points per pixel.
pub fn sheet_image(
    painter: &Painter,
    texture: TextureId,
    from: [f32; 2],
    size: [f32; 2],
    at: Pos2,
    scale: f32,
) {
    let uv = Rect::from_min_size(
        pos2(from[0] / SHEET, from[1] / SHEET),
        vec2(size[0] / SHEET, size[1] / SHEET),
    );
    let rect = Rect::from_min_size(at, vec2(size[0], size[1]) * scale);
    painter.image(texture, rect, uv, Color32::WHITE);
}

fn color(rgb: [f32; 3], shade: f32) -> Color32 {
    let c = |v: f32| (v * shade * 255.0).round().clamp(0.0, 255.0) as u8;
    Color32::from_rgb(c(rgb[0]), c(rgb[1]), c(rgb[2]))
}

fn uv_rect(face: &IconFace) -> Rect {
    let [u0, v0, u1, v1] = face.uv;
    Rect::from_min_max(pos2(u0, v0), pos2(u1, v1))
}

/// Draws an item icon filling `rect` (a 16×16 GUI-pixel square).
pub fn paint_icon(painter: &Painter, rect: Rect, icon: &ItemIcon, atlas: TextureId) {
    match icon {
        ItemIcon::Flat(face) => {
            painter.image(atlas, rect, uv_rect(face), color(face.color, 1.0));
        }
        ItemIcon::Cube { top, front, side } => {
            // An isometric cube: top, left (front) and right (side) faces,
            // shaded like 1.0.0's inventory blocks.
            let (c, w, h) = (rect.center(), rect.width() * 0.45, rect.height() * 0.5);
            let top_point = pos2(c.x, c.y - h);
            let left = pos2(c.x - w, c.y - h / 2.0);
            let right = pos2(c.x + w, c.y - h / 2.0);
            let bottom = pos2(c.x, c.y + h);
            let low_left = pos2(c.x - w, c.y + h / 2.0);
            let low_right = pos2(c.x + w, c.y + h / 2.0);
            let mut mesh = Mesh::with_texture(atlas);
            // Corners in UV order: top-left, top-right, bottom-right, bottom-left.
            let mut quad = |corners: [Pos2; 4], face: &IconFace, shade: f32| {
                let uv = uv_rect(face);
                let uvs = [
                    uv.left_top(),
                    uv.right_top(),
                    uv.right_bottom(),
                    uv.left_bottom(),
                ];
                let base = mesh.vertices.len() as u32;
                for (pos, uv) in corners.into_iter().zip(uvs) {
                    mesh.vertices.push(egui::epaint::Vertex {
                        pos,
                        uv,
                        color: color(face.color, shade),
                    });
                }
                mesh.indices
                    .extend([base, base + 1, base + 2, base, base + 2, base + 3]);
            };
            quad([left, top_point, right, c], top, 1.0);
            quad([left, c, bottom, low_left], front, 0.8);
            quad([c, right, low_right, bottom], side, 0.6);
            painter.add(Shape::mesh(mesh));
        }
    }
}

/// Draws a slot's icon, its count (if more than one) and its durability
/// bar (once worn), at `at` (the slot's top-left) with `scale`.
pub fn paint_slot(painter: &Painter, at: Pos2, slot: &SlotView, scale: f32, atlas: TextureId) {
    let rect = Rect::from_min_size(at, vec2(16.0, 16.0) * scale);
    paint_icon(painter, rect, &slot.icon, atlas);
    if let Some(left) = slot.durability {
        // 1.0.0: a 13-pixel bar on black, green to red as it wears.
        let bar = |x: f32, width: f32, height: f32, color: Color32| {
            let min = at + vec2(2.0 + x, 13.0) * scale;
            painter.rect_filled(
                Rect::from_min_size(min, vec2(width, height) * scale),
                0.0,
                color,
            );
        };
        bar(0.0, 13.0, 2.0, Color32::BLACK);
        let shade = (left.clamp(0.0, 1.0) * 255.0) as u8;
        bar(
            0.0,
            (13.0 * left).round(),
            1.0,
            Color32::from_rgb(255 - shade, shade, 0),
        );
    }
    if slot.count > 1 {
        let font = egui::FontId::monospace(8.0 * scale);
        let corner = at + vec2(17.0, 17.5) * scale;
        let text = slot.count.to_string();
        painter.text(
            corner + vec2(scale, scale),
            egui::Align2::RIGHT_BOTTOM,
            &text,
            font.clone(),
            Color32::from_gray(63),
        );
        painter.text(
            corner,
            egui::Align2::RIGHT_BOTTOM,
            &text,
            font,
            Color32::WHITE,
        );
    }
}

/// Survival numbers for the HUD.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SurvivalHud {
    pub health: i32,
    pub food: i32,
    /// Air while the eyes are under water.
    pub air: Option<i32>,
}

/// Hearts, drumsticks and air bubbles above the hotbar, from icons.png,
/// as in 1.0.0's `GuiIngame`: each icon is two points, full or half.
pub fn paint_survival(painter: &Painter, screen: Rect, hud: &SurvivalHud, textures: &GuiTextures) {
    let scale = gui_scale(screen);
    let icons = textures.icons.id();
    let left = screen.center().x - 91.0 * scale;
    let right = screen.center().x + 91.0 * scale;
    let row = screen.bottom() - 39.0 * scale;
    let icon = |from: [f32; 2], x: f32, y: f32| {
        sheet_image(painter, icons, from, [9.0, 9.0], pos2(x, y), scale);
    };
    for i in 0..10 {
        let points = i * 2;
        // Hearts fill from the left, drumsticks from the right.
        let heart_x = left + i as f32 * 8.0 * scale;
        icon([16.0, 0.0], heart_x, row);
        if hud.health > points + 1 {
            icon([52.0, 0.0], heart_x, row);
        } else if hud.health == points + 1 {
            icon([61.0, 0.0], heart_x, row);
        }
        let food_x = right - (i as f32 * 8.0 + 9.0) * scale;
        icon([16.0, 27.0], food_x, row);
        if hud.food > points + 1 {
            icon([52.0, 27.0], food_x, row);
        } else if hud.food == points + 1 {
            icon([61.0, 27.0], food_x, row);
        }
    }
    if let Some(air) = hud.air {
        let full = ((air - 2) as f32 * 10.0 / 300.0).ceil().max(0.0) as i32;
        let popping = (air as f32 * 10.0 / 300.0).ceil() as i32 - full;
        for i in 0..full + popping {
            let x = right - (i as f32 * 8.0 + 9.0) * scale;
            let from = if i < full { [16.0, 18.0] } else { [25.0, 18.0] };
            icon(from, x, row - 10.0 * scale);
        }
    }
}

/// The hotbar at the bottom of the screen, as in 1.0.0's `GuiIngame`.
pub fn paint_hotbar(
    painter: &Painter,
    screen: Rect,
    slots: &[Option<SlotView>],
    selected: usize,
    textures: &GuiTextures,
) {
    let scale = gui_scale(screen);
    let at = pos2(
        screen.center().x - 91.0 * scale,
        screen.bottom() - 22.0 * scale,
    );
    sheet_image(
        painter,
        textures.widgets.id(),
        [0.0, 0.0],
        [182.0, 22.0],
        at,
        scale,
    );
    let highlight = at + vec2(-1.0 + selected as f32 * 20.0, -1.0) * scale;
    sheet_image(
        painter,
        textures.widgets.id(),
        [0.0, 22.0],
        [24.0, 24.0],
        highlight,
        scale,
    );
    for (i, slot) in slots.iter().enumerate() {
        if let Some(slot) = slot {
            let slot_at = at + vec2(3.0 + i as f32 * 20.0, 3.0) * scale;
            paint_slot(painter, slot_at, slot, scale, textures.atlas.id());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gui_scale_is_minecrafts_auto_scale() {
        let screen = |w: f32, h: f32| Rect::from_min_size(Pos2::ZERO, vec2(w, h));
        assert_eq!(gui_scale(screen(1920.0, 1080.0)), 4.0);
        assert_eq!(gui_scale(screen(1280.0, 720.0)), 3.0);
        assert_eq!(gui_scale(screen(854.0, 480.0)), 2.0);
        assert_eq!(gui_scale(screen(300.0, 200.0)), 1.0);
    }
}
