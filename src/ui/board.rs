//! The scoreboard's displays (M44): the sidebar, bossbars, titles and the
//! action bar, drawn over the world.

use egui::{Align2, Color32, FontId, Painter, Pos2, Rect, pos2, vec2};

use super::{font, gui};
use crate::game::board::{BarView, Sidebar, TitleState};

/// The width of a `§`-coded text at `size`.
fn width_of(painter: &Painter, text: &str, size: f32) -> f32 {
    painter
        .layout_job(font::job(text, size, Color32::WHITE, false))
        .size()
        .x
}

/// The sidebar on the right: a title band over the rows, names white on
/// the left and scores (red) on the right.
pub fn paint_sidebar(painter: &Painter, full: Rect, sidebar: &Sidebar) {
    let scale = gui::gui_scale(full);
    let size = 8.0 * scale;
    let row = 9.0 * scale;
    let rows = &sidebar.rows;
    let mut width = width_of(painter, &sidebar.title, size);
    for (name, score) in rows {
        let w = width_of(painter, name, size)
            + if score.is_empty() {
                0.0
            } else {
                size + width_of(painter, score, size)
            };
        width = width.max(w);
    }
    let height = row * rows.len() as f32;
    let right = full.right() - 3.0 * scale;
    let left = right - width - 2.0 * scale;
    let bottom = full.center().y + height / 3.0 + row;
    let top = bottom - height;
    painter.rect_filled(
        Rect::from_min_max(
            pos2(left - scale, top - row - scale),
            pos2(right + scale, top - scale),
        ),
        0.0,
        Color32::from_rgba_unmultiplied(0x50, 0x50, 0x50, 0x60),
    );
    painter.rect_filled(
        Rect::from_min_max(pos2(left - scale, top - scale), pos2(right + scale, bottom)),
        0.0,
        Color32::from_black_alpha(0x66),
    );
    font::shadowed(
        painter,
        pos2((left + right) / 2.0, top - row),
        Align2::CENTER_TOP,
        &sidebar.title,
        Color32::WHITE,
        size,
    );
    for (i, (name, score)) in rows.iter().enumerate() {
        let y = top + row * i as f32;
        font::shadowed(
            painter,
            pos2(left, y),
            Align2::LEFT_TOP,
            name,
            Color32::WHITE,
            size,
        );
        if !score.is_empty() {
            font::shadowed(
                painter,
                pos2(right, y),
                Align2::RIGHT_TOP,
                score,
                Color32::from_rgb(0xFF, 0x55, 0x55),
                size,
            );
        }
    }
}

fn bar_color(name: &str) -> Color32 {
    match name {
        "pink" => Color32::from_rgb(0xF0, 0x6E, 0xB8),
        "blue" => Color32::from_rgb(0x4B, 0x7B, 0xE8),
        "red" => Color32::from_rgb(0xE8, 0x3A, 0x3A),
        "green" => Color32::from_rgb(0x4C, 0xC8, 0x4C),
        "yellow" => Color32::from_rgb(0xF0, 0xD8, 0x3A),
        "purple" => Color32::from_rgb(0x8E, 0x4C, 0xD0),
        _ => Color32::WHITE,
    }
}

/// Bossbars along the top, centered: the name above a 182-wide bar.
pub fn paint_bars(painter: &Painter, full: Rect, bars: &[BarView]) {
    let scale = gui::gui_scale(full);
    for (i, bar) in bars.iter().take(9).enumerate() {
        let width = 182.0 * scale;
        let left = full.center().x - width / 2.0;
        let top = (12.0 + 19.0 * i as f32) * scale;
        font::shadowed(
            painter,
            pos2(full.center().x, top - 9.0 * scale),
            Align2::CENTER_TOP,
            &bar.name,
            Color32::WHITE,
            8.0 * scale,
        );
        let back = Rect::from_min_size(pos2(left, top), vec2(width, 5.0 * scale));
        painter.rect_filled(back, 0.0, Color32::from_rgb(0x40, 0x40, 0x40));
        let share = (bar.value as f32 / bar.max.max(1) as f32).clamp(0.0, 1.0);
        let fill = Rect::from_min_size(back.min, vec2(width * share, 5.0 * scale));
        painter.rect_filled(fill, 0.0, bar_color(&bar.color));
        let notches = match bar.style.as_str() {
            "notched_6" => 6,
            "notched_10" => 10,
            "notched_12" => 12,
            "notched_20" => 20,
            _ => 0,
        };
        for n in 1..notches {
            let x = left + width * n as f32 / notches as f32;
            painter.rect_filled(
                Rect::from_min_size(pos2(x, top), vec2(scale, 5.0 * scale)),
                0.0,
                Color32::BLACK,
            );
        }
    }
}

/// `/title`: the title at four times the text size and the subtitle at
/// twice, fading with the state; and the action bar above the hotbar.
pub fn paint_titles(painter: &Painter, full: Rect, title: &TitleState, alpha: Option<f32>) {
    let scale = gui::gui_scale(full);
    if let Some(alpha) = alpha {
        let color = Color32::from_white_alpha((alpha.clamp(0.0, 1.0) * 255.0) as u8);
        if !title.title.is_empty() {
            font::shadowed(
                painter,
                pos2(full.center().x, full.center().y - 20.0 * scale),
                Align2::CENTER_CENTER,
                &title.title,
                color,
                8.0 * scale * 4.0,
            );
        }
        if !title.subtitle.is_empty() {
            font::shadowed(
                painter,
                pos2(full.center().x, full.center().y + 10.0 * scale),
                Align2::CENTER_CENTER,
                &title.subtitle,
                color,
                8.0 * scale * 2.0,
            );
        }
    }
    if title.actionbar_left > 0 && !title.actionbar.is_empty() {
        let fade = (title.actionbar_left as f32 / 10.0).min(1.0);
        let alpha = (fade * 255.0) as u8;
        // "Now Playing" cycles round the rainbow (`Mth.hsvToRgb(t / 50, 0.7, 0.6)`).
        let color = if title.actionbar_rainbow {
            let rgb = crate::game::jukebox::hsv(title.actionbar_left as f32 / 50.0, 0.7, 0.6);
            Color32::from_rgba_unmultiplied((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8, alpha)
        } else {
            Color32::from_white_alpha(alpha)
        };
        font::shadowed(
            painter,
            pos2(full.center().x, full.bottom() - 68.0 * scale),
            Align2::CENTER_BOTTOM,
            &title.actionbar,
            color,
            8.0 * scale,
        );
    }
}

/// A name tag line: the (dressed) name and, below, its below-name score.
pub fn paint_tag(painter: &Painter, center: Pos2, name: &str, below: Option<&str>, size: f32) {
    let width = width_of(painter, name, size);
    let rect = Rect::from_center_size(center, vec2(width, size * 1.125)).expand(size / 8.0);
    painter.rect_filled(rect, 0.0, Color32::from_black_alpha(64));
    font::shadowed(
        painter,
        rect.center(),
        Align2::CENTER_CENTER,
        name,
        Color32::WHITE,
        size,
    );
    if let Some(below) = below {
        let at = pos2(center.x, rect.bottom() + size * 0.6);
        font::shadowed(
            painter,
            at,
            Align2::CENTER_CENTER,
            below,
            Color32::WHITE,
            size,
        );
    }
}

#[allow(dead_code)]
fn unused(_: FontId) {}
