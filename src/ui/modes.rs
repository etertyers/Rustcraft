//! The game-mode switcher (F3+F4) and the spectator menu (number keys),
//! drawn over the world (M43).

use egui::{Color32, FontId, Painter, Rect, pos2};

use super::gui;
use crate::player::mode::GameMode;

/// F3+F4's four modes in a row, the chosen one lit. Releasing F3 applies it.
pub fn paint_switcher(painter: &Painter, full: Rect, chosen: usize) {
    let scale = gui::gui_scale(full);
    let (w, h, gap) = (52.0 * scale, 26.0 * scale, 4.0 * scale);
    let total = 4.0 * w + 3.0 * gap;
    let left = full.center().x - total / 2.0;
    let top = full.center().y - 40.0 * scale;
    let font = FontId::proportional(8.0 * scale);
    let title = painter.layout_no_wrap(
        format!("{}  [ F4 ] Next", GameMode::ALL[chosen].name()),
        font.clone(),
        Color32::WHITE,
    );
    painter.galley(
        pos2(full.center().x - title.size().x / 2.0, top - 14.0 * scale),
        title,
        Color32::WHITE,
    );
    for (i, mode) in GameMode::ALL.iter().enumerate() {
        let rect = Rect::from_min_size(pos2(left + i as f32 * (w + gap), top), egui::vec2(w, h));
        let lit = i == chosen;
        painter.rect_filled(
            rect,
            0.0,
            if lit {
                Color32::from_rgb(90, 90, 90)
            } else {
                Color32::from_black_alpha(160)
            },
        );
        if lit {
            painter.rect_stroke(
                rect,
                0.0,
                egui::Stroke::new(scale, Color32::WHITE),
                egui::StrokeKind::Inside,
            );
        }
        let name = painter.layout_no_wrap(mode.name().to_owned(), font.clone(), Color32::WHITE);
        painter.galley(rect.center() - name.size() / 2.0, name, Color32::WHITE);
    }
}

/// The spectator menu: "Teleport to Player", one numbered row per player.
pub fn paint_spectator_menu(painter: &Painter, full: Rect, title: &str, players: &[String]) {
    let scale = gui::gui_scale(full);
    let font = FontId::proportional(8.0 * scale);
    let width = 120.0 * scale;
    let row = 12.0 * scale;
    let top = full.center().y - (players.len().max(1) as f32 * row) / 2.0 - 16.0 * scale;
    let left = full.center().x - width / 2.0;
    let head = painter.layout_no_wrap(
        format!("{title} (press a number, \u{2190}/\u{2192} switches)"),
        font.clone(),
        Color32::WHITE,
    );
    painter.rect_filled(
        Rect::from_min_size(
            pos2(left, top),
            egui::vec2(width, row * (players.len().max(1) + 1) as f32),
        ),
        0.0,
        Color32::from_black_alpha(170),
    );
    painter.galley(
        pos2(full.center().x - head.size().x / 2.0, top + 2.0 * scale),
        head,
        Color32::WHITE,
    );
    if players.is_empty() {
        let none =
            painter.layout_no_wrap("Nobody to go to".to_owned(), font.clone(), Color32::GRAY);
        painter.galley(
            pos2(left + 4.0 * scale, top + row + 2.0 * scale),
            none,
            Color32::GRAY,
        );
    }
    for (i, name) in players.iter().enumerate().take(9) {
        let line =
            painter.layout_no_wrap(format!("{}  {name}", i + 1), font.clone(), Color32::WHITE);
        painter.galley(
            pos2(left + 4.0 * scale, top + (i + 1) as f32 * row + 2.0 * scale),
            line,
            Color32::WHITE,
        );
    }
}
