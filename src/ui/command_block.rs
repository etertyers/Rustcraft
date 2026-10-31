//! The command block editor (M45): the command with Tab completion, the
//! previous output, a track-output toggle, and buttons for the mode,
//! conditional and redstone settings; Done and Cancel.

use egui::{Align2, Color32, Event, Key, Pos2, Rect, vec2};

use super::{font, gui, text_field::TextField};
use crate::game::command_blocks::Edit;

/// What the editor asks for.
#[derive(Debug, Clone, PartialEq)]
pub enum EditorAction {
    Done(Edit),
    Cancel,
    /// Tab: what can finish this command line (no `/`).
    Complete(String),
}

#[derive(Debug)]
pub struct CommandBlockEditor {
    pub edit: Edit,
    input: TextField,
    completions: Vec<String>,
    completed: Option<usize>,
}

impl CommandBlockEditor {
    pub fn new(edit: Edit) -> Self {
        let input = TextField::new(&edit.command, 32_500);
        Self {
            edit,
            input,
            completions: Vec::new(),
            completed: None,
        }
    }

    pub fn command(&self) -> &str {
        &self.input.text
    }

    /// The server's completions for the line as it was: the first goes in.
    pub fn completions(&mut self, options: Vec<String>) {
        self.completions = options;
        self.completed = None;
        self.cycle();
    }

    fn cycle(&mut self) {
        if self.completions.is_empty() {
            return;
        }
        let next = self
            .completed
            .map_or(0, |i| (i + 1) % self.completions.len());
        self.completed = Some(next);
        let mut text = self.input.text.clone();
        let cut = text.rfind(' ').map_or(0, |i| i + 1);
        let slash = usize::from(cut == 0 && text.starts_with('/'));
        text.truncate(cut.max(slash));
        text.push_str(&self.completions[next]);
        self.input = TextField::new(&text, 32_500);
    }

    fn finished(&self) -> Edit {
        Edit {
            command: self.input.text.clone(),
            ..self.edit.clone()
        }
    }

    /// Draws the editor over the game and reads its keys and clicks.
    pub fn show(&mut self, ctx: &egui::Context) -> Option<EditorAction> {
        let full = ctx.content_rect();
        let scale = gui::gui_scale(full);
        let mut action = None;
        // Keys: Enter is Done, Escape Cancel, Tab completes.
        let events = ctx.input(|i| i.events.clone());
        let mut typing = Vec::new();
        for event in &events {
            if let Event::Text(t) | Event::Paste(t) = event {
                typing.push(Event::Text(t.replace('§', "")));
                continue;
            }
            let Event::Key {
                key, pressed: true, ..
            } = event
            else {
                continue;
            };
            match key {
                Key::Enter => action = Some(EditorAction::Done(self.finished())),
                Key::Escape => action = Some(EditorAction::Cancel),
                Key::Tab => {
                    if self.completed.is_some() {
                        self.cycle();
                    } else {
                        let line = self.input.text.trim_start_matches('/').to_owned();
                        action = Some(EditorAction::Complete(line));
                    }
                }
                _ => typing.push(event.clone()),
            }
        }
        if self.input.edit(&typing) {
            self.completions.clear();
            self.completed = None;
        }
        let painter = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::unique("command_block_editor"),
        ));
        painter.rect_filled(full, 0.0, Color32::from_black_alpha(110));
        let width = 280.0 * scale;
        let left = full.center().x - width / 2.0;
        let mut y = full.top() + 28.0 * scale;
        let white = Color32::WHITE;
        let gray = Color32::from_rgb(0xA0, 0xA0, 0xA0);
        font::shadowed(
            &painter,
            Pos2::new(full.center().x, y),
            Align2::CENTER_TOP,
            "Set Console Command for Minecart/Block",
            white,
            8.0 * scale,
        );
        y += 18.0 * scale;
        font::shadowed(
            &painter,
            Pos2::new(left, y),
            Align2::LEFT_TOP,
            "Console Command Input",
            gray,
            8.0 * scale,
        );
        y += 11.0 * scale;
        // The command box, with its cursor and the end of a long line in view.
        let boxed = Rect::from_min_size(Pos2::new(left, y), vec2(width, 16.0 * scale));
        painter.rect_filled(boxed, 0.0, Color32::BLACK);
        painter.rect_stroke(
            boxed,
            0.0,
            egui::Stroke::new(scale, Color32::from_rgb(0xA0, 0xA0, 0xA0)),
            egui::StrokeKind::Inside,
        );
        let chars: Vec<char> = self.input.text.chars().collect();
        let cursor = self.input.cursor.min(chars.len());
        let text_width = |s: &str| {
            painter
                .layout_job(font::job(s, 8.0 * scale, white, false))
                .size()
                .x
        };
        let mut first = 0;
        while first < cursor
            && text_width(&chars[first..cursor].iter().collect::<String>()) > width - 8.0 * scale
        {
            first += 1;
        }
        let shown: String = chars[first..].iter().collect();
        let at = Pos2::new(boxed.left() + 3.0 * scale, boxed.top() + 4.0 * scale);
        font::shadowed(
            &painter,
            at,
            Align2::LEFT_TOP,
            &shown,
            Color32::from_rgb(0xE0, 0xE0, 0xE0),
            8.0 * scale,
        );
        let blink = ((ctx.input(|i| i.time) * 20.0 / 6.0) as u64).is_multiple_of(2);
        ctx.request_repaint();
        if blink {
            let x = at.x + text_width(&chars[first..cursor].iter().collect::<String>());
            painter.rect_filled(
                Rect::from_min_size(Pos2::new(x, at.y - scale), vec2(scale, 10.0 * scale)),
                0.0,
                Color32::from_rgb(0xE0, 0xE0, 0xE0),
            );
        }
        y += 22.0 * scale;
        font::shadowed(
            &painter,
            Pos2::new(left, y),
            Align2::LEFT_TOP,
            "Previous Output",
            gray,
            8.0 * scale,
        );
        y += 11.0 * scale;
        let output = Rect::from_min_size(Pos2::new(left, y), vec2(width, 16.0 * scale));
        painter.rect_filled(output, 0.0, Color32::from_rgb(0x20, 0x20, 0x20));
        painter.rect_stroke(
            output,
            0.0,
            egui::Stroke::new(scale, Color32::from_rgb(0x60, 0x60, 0x60)),
            egui::StrokeKind::Inside,
        );
        font::shadowed(
            &painter,
            Pos2::new(output.left() + 3.0 * scale, output.top() + 4.0 * scale),
            Align2::LEFT_TOP,
            &self.edit.last_output,
            Color32::from_rgb(0xC0, 0xC0, 0xC0),
            8.0 * scale,
        );
        y += 26.0 * scale;
        // The setting buttons.
        let pointer = ctx.input(|i| i.pointer.hover_pos());
        let clicked = ctx.input(|i| i.pointer.primary_clicked());
        let button = |label: &str, rect: Rect, painter: &egui::Painter| -> bool {
            let hot = pointer.is_some_and(|p| rect.contains(p));
            painter.rect_filled(
                rect,
                0.0,
                if hot {
                    Color32::from_rgb(0x70, 0x70, 0x90)
                } else {
                    Color32::from_rgb(0x50, 0x50, 0x50)
                },
            );
            painter.rect_stroke(
                rect,
                0.0,
                egui::Stroke::new(scale, Color32::BLACK),
                egui::StrokeKind::Inside,
            );
            font::shadowed(
                painter,
                rect.center(),
                Align2::CENTER_CENTER,
                label,
                white,
                8.0 * scale,
            );
            hot && clicked
        };
        let half = (width - 4.0 * scale) / 2.0;
        let row = |y: f32, i: f32| {
            Rect::from_min_size(
                Pos2::new(left + i * (half + 4.0 * scale), y),
                vec2(half, 16.0 * scale),
            )
        };
        if button(
            &format!("Mode: {}", self.edit.mode.label()),
            row(y, 0.0),
            &painter,
        ) {
            self.edit.mode = self.edit.mode.next();
        }
        if button(
            if self.edit.conditional {
                "Conditional"
            } else {
                "Unconditional"
            },
            row(y, 1.0),
            &painter,
        ) {
            self.edit.conditional = !self.edit.conditional;
        }
        y += 20.0 * scale;
        if button(
            if self.edit.auto {
                "Always Active"
            } else {
                "Needs Redstone"
            },
            row(y, 0.0),
            &painter,
        ) {
            self.edit.auto = !self.edit.auto;
        }
        if button(
            &format!(
                "Track Output: {}",
                if self.edit.track_output { "ON" } else { "OFF" }
            ),
            row(y, 1.0),
            &painter,
        ) {
            self.edit.track_output = !self.edit.track_output;
        }
        y += 28.0 * scale;
        if button("Done", row(y, 0.0), &painter) {
            action = Some(EditorAction::Done(self.finished()));
        }
        if button("Cancel", row(y, 1.0), &painter) {
            action = Some(EditorAction::Cancel);
        }
        action
    }
}
