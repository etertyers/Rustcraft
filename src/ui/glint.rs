//! The enchantment glint on item icons in the hotbar and screens: a
//! purple pattern that scrolls over the item, added to its colors. Flat
//! items get an overlay cut to their own shape, made fresh ten times a
//! second; blocks draw the pattern over each of their three faces.
//! (Held and dropped items glint in the shader instead.)

use std::{cell::RefCell, collections::HashMap};

use egui::{Color32, ColorImage, Mesh, Painter, Pos2, Rect, Shape, TextureHandle, TextureOptions};
use image::RgbaImage;

use crate::render::icons::ItemIcon;

/// The pattern is tinted by this much of each color (the game's
/// `color(0.5, 0.25, 0.8)`).
const TINT: [f32; 3] = [0.5, 0.25, 0.8];
/// The pattern runs at 10° and repeats 8 times across an icon.
const ROTATION_DEGREES: f32 = 10.0;
const REPEATS: f32 = 8.0;
/// Seconds the pattern takes to scroll one repeat in each direction.
const SCROLL_SECONDS: [f64; 2] = [3.0, 4.873];
/// Overlays are redrawn this often.
const FRAMES_PER_SECOND: f64 = 10.0;

struct State {
    atlas: RgbaImage,
    pattern: RgbaImage,
    /// The pattern as an additive, repeating texture for blocks.
    texture: TextureHandle,
    /// The frame the overlays were drawn for.
    frame: u64,
    overlays: HashMap<[u32; 4], TextureHandle>,
}

thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
}

/// Sets up the glint with the atlas the icons are cut from and the
/// pattern (none: nothing glints).
pub fn install(ctx: &egui::Context, atlas: &RgbaImage, pattern: Option<RgbaImage>) {
    let state = pattern.map(|pattern| {
        let size = [pattern.width() as usize, pattern.height() as usize];
        let bytes: Vec<u8> = pattern
            .pixels()
            .flat_map(|p| [p[0], p[1], p[2], 0])
            .collect();
        let image = ColorImage::from_rgba_premultiplied(size, &bytes);
        State {
            atlas: atlas.clone(),
            texture: ctx.load_texture("glint", image, TextureOptions::NEAREST_REPEAT),
            pattern,
            frame: 0,
            overlays: HashMap::new(),
        }
    });
    STATE.with(|cell| *cell.borrow_mut() = state);
}

/// Where the pattern is at `p`, a point of the icon from 0 to 1 across
/// and down, `seconds` into the game: in repeats of the pattern.
pub fn pattern_uv(p: [f32; 2], seconds: f64) -> [f32; 2] {
    let (sin, cos) = ROTATION_DEGREES.to_radians().sin_cos();
    let (x, y) = (p[0] * REPEATS, p[1] * REPEATS);
    [
        x * cos - y * sin + (seconds / SCROLL_SECONDS[0]).fract() as f32,
        x * sin + y * cos + (seconds / SCROLL_SECONDS[1]).fract() as f32,
    ]
}

/// The tinted pattern's color at `uv` (wrapping), 0–1 per channel.
pub fn sample(pattern: &RgbaImage, uv: [f32; 2]) -> [f32; 3] {
    let (w, h) = pattern.dimensions();
    let x = (uv[0].rem_euclid(1.0) * w as f32) as u32 % w;
    let y = (uv[1].rem_euclid(1.0) * h as f32) as u32 % h;
    let texel = pattern.get_pixel(x, y);
    std::array::from_fn(|i| f32::from(texel[i]) / 255.0 * TINT[i])
}

/// An icon's overlay: the pattern wherever `tile` is opaque, as additive
/// premultiplied colors (alpha 0), `seconds` into the game.
pub fn overlay(tile: &RgbaImage, pattern: &RgbaImage, seconds: f64) -> ColorImage {
    let (w, h) = tile.dimensions();
    let mut bytes = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let alpha = f32::from(tile.get_pixel(x, y)[3]) / 255.0;
            let uv = pattern_uv(
                [(x as f32 + 0.5) / w as f32, (y as f32 + 0.5) / h as f32],
                seconds,
            );
            let [r, g, b] = sample(pattern, uv).map(|c| (c * alpha * 255.0).round() as u8);
            bytes.extend([r, g, b, 0]);
        }
    }
    ColorImage::from_rgba_premultiplied([w as usize, h as usize], &bytes)
}

/// Paints the pattern over `mesh`'s triangles (already projected onto the
/// screen), scrolling across the rectangle they cover.
pub fn paint_mesh(painter: &Painter, mut mesh: Mesh) {
    STATE.with(|cell| {
        let state = cell.borrow();
        let Some(state) = state.as_ref() else {
            return;
        };
        let Some(rect) = mesh
            .vertices
            .iter()
            .map(|v| Rect::from_pos(v.pos))
            .reduce(Rect::union)
        else {
            return;
        };
        let seconds = painter.ctx().input(|input| input.time);
        let side = rect.width().max(rect.height()).max(1.0);
        let tint = Color32::from_rgba_premultiplied(
            (TINT[0] * 255.0) as u8,
            (TINT[1] * 255.0) as u8,
            (TINT[2] * 255.0) as u8,
            0,
        );
        mesh.texture_id = state.texture.id();
        for v in &mut mesh.vertices {
            let p = [(v.pos.x - rect.min.x) / side, (v.pos.y - rect.min.y) / side];
            let [u, w] = pattern_uv(p, seconds);
            v.uv = Pos2::new(u, w);
            v.color = tint;
        }
        painter.add(Shape::mesh(mesh));
    });
}

/// Paints the glint over an icon drawn in `rect`.
pub fn paint(painter: &Painter, rect: Rect, icon: &ItemIcon) {
    STATE.with(|cell| {
        let mut state = cell.borrow_mut();
        let Some(state) = state.as_mut() else {
            return;
        };
        let seconds = painter.ctx().input(|input| input.time);
        let frame = (seconds * FRAMES_PER_SECOND) as u64;
        if frame != state.frame {
            state.frame = frame;
            state.overlays.clear();
        }
        match icon {
            ItemIcon::Flat(face) => {
                let (aw, ah) = state.atlas.dimensions();
                let [u0, v0, u1, v1] = face.uv;
                let (x0, y0) = (
                    (u0 * aw as f32).round() as u32,
                    (v0 * ah as f32).round() as u32,
                );
                let (x1, y1) = (
                    (u1 * aw as f32).round() as u32,
                    (v1 * ah as f32).round() as u32,
                );
                if x1 <= x0 || y1 <= y0 || x1 > aw || y1 > ah {
                    return;
                }
                let key = [x0, y0, x1, y1];
                if !state.overlays.contains_key(&key) {
                    let tile = image::imageops::crop_imm(&state.atlas, x0, y0, x1 - x0, y1 - y0)
                        .to_image();
                    let image = overlay(&tile, &state.pattern, seconds);
                    let name = format!("glint {x0} {y0}");
                    let handle = painter
                        .ctx()
                        .load_texture(name, image, TextureOptions::NEAREST);
                    state.overlays.insert(key, handle);
                }
                painter.image(
                    state.overlays[&key].id(),
                    rect,
                    Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                    Color32::WHITE,
                );
            }
            ItemIcon::Cube { .. } => {
                // The same three faces `paint_icon` drew, with the pattern.
                let (c, w, h) = (rect.center(), rect.width() * 0.45, rect.height() * 0.5);
                let top = Pos2::new(c.x, c.y - h);
                let left = Pos2::new(c.x - w, c.y - h / 2.0);
                let right = Pos2::new(c.x + w, c.y - h / 2.0);
                let bottom = Pos2::new(c.x, c.y + h);
                let low_left = Pos2::new(c.x - w, c.y + h / 2.0);
                let low_right = Pos2::new(c.x + w, c.y + h / 2.0);
                let mut mesh = Mesh::with_texture(state.texture.id());
                let tint = Color32::from_rgba_premultiplied(
                    (TINT[0] * 255.0) as u8,
                    (TINT[1] * 255.0) as u8,
                    (TINT[2] * 255.0) as u8,
                    255,
                );
                let size = rect.size();
                for corners in [
                    [left, top, right, c],
                    [left, c, bottom, low_left],
                    [c, right, low_right, bottom],
                ] {
                    let base = mesh.vertices.len() as u32;
                    for pos in corners {
                        let p = [(pos.x - rect.min.x) / size.x, (pos.y - rect.min.y) / size.y];
                        let [u, v] = pattern_uv(p, seconds);
                        mesh.vertices.push(egui::epaint::Vertex {
                            pos,
                            uv: Pos2::new(u, v),
                            color: tint,
                        });
                    }
                    mesh.indices
                        .extend([base, base + 1, base + 2, base, base + 2, base + 3]);
                }
                painter.add(Shape::mesh(mesh));
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stripes() -> RgbaImage {
        RgbaImage::from_fn(8, 8, |x, _| {
            if x % 2 == 0 {
                image::Rgba([255, 255, 255, 255])
            } else {
                image::Rgba([0, 0, 0, 255])
            }
        })
    }

    #[test]
    fn the_overlay_covers_only_the_opaque_pixels_and_adds_light() {
        let tile = RgbaImage::from_fn(16, 16, |x, _| {
            image::Rgba([200, 10, 10, if x < 8 { 255 } else { 0 }])
        });
        let image = overlay(&tile, &stripes(), 0.0);
        let at = |x: usize, y: usize| image.pixels[y * 16 + x];
        // Additive: alpha is always 0.
        assert!(image.pixels.iter().all(|p| p.a() == 0));
        // Nothing where the item is clear.
        assert!(
            (8..16)
                .all(|x| (0..16).all(|y| (at(x, y).r(), at(x, y).g(), at(x, y).b()) == (0, 0, 0)))
        );
        // Some purple-ish light on the item, blue strongest.
        let lit: Vec<_> = (0..8)
            .flat_map(|x| (0..16).map(move |y| (x, y)))
            .map(|(x, y)| at(x, y))
            .filter(|p| p.r() > 0 || p.b() > 0)
            .collect();
        assert!(!lit.is_empty());
        assert!(lit.iter().all(|p| p.b() >= p.r() && p.r() >= p.g()));
    }

    #[test]
    fn the_pattern_scrolls_over_time_and_repeats() {
        let pattern = stripes();
        let a = overlay(
            &RgbaImage::from_pixel(16, 16, image::Rgba([1, 1, 1, 255])),
            &pattern,
            0.0,
        );
        let b = overlay(
            &RgbaImage::from_pixel(16, 16, image::Rgba([1, 1, 1, 255])),
            &pattern,
            0.7,
        );
        assert_ne!(a.pixels, b.pixels);
        // After the longest scroll period it is back where it began.
        let [u0, v0] = pattern_uv([0.3, 0.6], 0.0);
        let [u1, _] = pattern_uv([0.3, 0.6], 3.0);
        assert!((u0 - u1).abs() < 1e-4);
        let _ = v0;
        // Sampling wraps.
        assert_eq!(sample(&pattern, [0.1, 0.1]), sample(&pattern, [5.1, -3.9]));
    }
}
