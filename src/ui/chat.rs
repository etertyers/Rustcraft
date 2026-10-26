//! Chat (M38), as vanilla draws it: the last lines over the bottom left of
//! the screen, fading ten seconds after they arrive while chat is closed;
//! opened with T (or / for a command), a box to type in with the sent
//! history on ↑ and ↓, the mouse wheel to scroll back through the last 100
//! lines, and Tab to complete a command. Text uses the Minecraft font and
//! its `§` codes.

use std::collections::VecDeque;

use egui::{Color32, Event, Key, Pos2, Rect, vec2};

use super::{font, gui};

/// Lines kept, and sent messages remembered.
const KEPT: usize = 100;
/// Seconds a line shows while chat is closed, the last of them fading.
const SHOWN: f64 = 10.0;
const FADE: f64 = 1.0;
/// Lines shown while chat is closed and while it's open (vanilla's 10 and
/// 20 at the default height).
const CLOSED_LINES: usize = 10;
const OPEN_LINES: usize = 20;
/// The longest message vanilla sends.
pub const MAX_MESSAGE: usize = 256;

/// Chat Settings (Options): who sees chat, and how it looks.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct ChatSettings {
    pub visibility: Visibility,
    /// Text opacity, 0.1–1.
    pub opacity: f32,
    /// The dark band behind the lines, 0–1 (vanilla's default 0.5).
    pub background: f32,
    /// Text size as a fraction of normal, 0.5–1.
    pub scale: f32,
    /// Width in GUI pixels, 40–320.
    pub width: f32,
    /// Extra space between lines, 0–1 of a line.
    pub spacing: f32,
}

impl Default for ChatSettings {
    fn default() -> Self {
        Self {
            visibility: Visibility::Shown,
            opacity: 1.0,
            background: 0.5,
            scale: 1.0,
            width: 320.0,
            spacing: 0.0,
        }
    }
}

impl ChatSettings {
    /// Every value inside its slider's range.
    pub fn clamped(self) -> Self {
        Self {
            visibility: self.visibility,
            opacity: self.opacity.clamp(0.1, 1.0),
            background: self.background.clamp(0.0, 1.0),
            scale: self.scale.clamp(0.5, 1.0),
            width: self.width.clamp(40.0, 320.0),
            spacing: self.spacing.clamp(0.0, 1.0),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Visibility {
    #[default]
    Shown,
    /// Only the answers to commands.
    CommandsOnly,
    Hidden,
}

impl Visibility {
    pub fn name(self) -> &'static str {
        match self {
            Visibility::Shown => "Shown",
            Visibility::CommandsOnly => "Commands Only",
            Visibility::Hidden => "Hidden",
        }
    }

    pub fn next(self) -> Self {
        match self {
            Visibility::Shown => Visibility::CommandsOnly,
            Visibility::CommandsOnly => Visibility::Hidden,
            Visibility::Hidden => Visibility::Shown,
        }
    }
}

/// A line of chat and when it came.
#[derive(Debug, Clone, PartialEq)]
struct Line {
    text: String,
    /// An answer to this player's own command (shown even with Commands
    /// Only).
    command: bool,
    at: f64,
}

/// What the chat box asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChatAction {
    /// Enter: send this message or command.
    Send(String),
    /// Tab: what can follow this command line.
    Complete(String),
}

#[derive(Debug, Default)]
pub struct Chat {
    lines: VecDeque<Line>,
    /// Typing, and what's in the box.
    open: bool,
    input: String,
    /// Sent messages, newest last, and the one ↑/↓ is on.
    sent: Vec<String>,
    recalled: Option<usize>,
    /// Lines scrolled back from the newest.
    scroll: usize,
    /// The server's completions for the line as it was, and the one Tab
    /// put in.
    completions: Vec<String>,
    completed: Option<usize>,
    /// Focus the box on the next frame.
    focus: bool,
}

impl Chat {
    /// A line arrives (a command's answer if `command`).
    pub fn push(&mut self, text: String, command: bool, now: f64) {
        self.lines.push_back(Line { text, command, at: now });
        while self.lines.len() > KEPT {
            self.lines.pop_front();
        }
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Opens the box with `start` in it ("" for T, "/" for /).
    pub fn open(&mut self, start: &str) {
        self.open = true;
        self.input = start.to_owned();
        self.recalled = None;
        self.scroll = 0;
        self.completions.clear();
        self.completed = None;
        self.focus = true;
    }

    pub fn close(&mut self) {
        self.open = false;
        self.input.clear();
        self.scroll = 0;
    }

    /// The server's answer to a Tab: completions of the line.
    pub fn completions(&mut self, options: Vec<String>) {
        self.completions = options;
        self.completed = None;
        self.cycle_completion();
    }

    /// Puts the next completion in the box's last word.
    fn cycle_completion(&mut self) {
        if self.completions.is_empty() {
            return;
        }
        let next = self.completed.map_or(0, |i| (i + 1) % self.completions.len());
        self.completed = Some(next);
        let cut = self.input.rfind(' ').map_or(1.min(self.input.len()), |i| i + 1);
        self.input.truncate(cut);
        self.input.push_str(&self.completions[next]);
    }

    /// ↑ (back) or ↓ (forward) through what was sent.
    fn recall(&mut self, back: bool) {
        if self.sent.is_empty() {
            return;
        }
        let index = match (self.recalled, back) {
            (None, true) => self.sent.len() - 1,
            (None, false) => return,
            (Some(i), true) => i.saturating_sub(1),
            (Some(i), false) if i + 1 < self.sent.len() => i + 1,
            (Some(_), false) => {
                self.recalled = None;
                self.input.clear();
                return;
            }
        };
        self.recalled = Some(index);
        self.input = self.sent[index].clone();
    }

    /// Takes the keys for the open box: Enter sends, Tab completes, ↑/↓
    /// recall. The text itself goes through the text box.
    fn keys(&mut self, ctx: &egui::Context) -> Option<ChatAction> {
        let events = ctx.input(|i| i.events.clone());
        let mut action = None;
        for event in events {
            let Event::Key {
                key, pressed: true, ..
            } = event
            else {
                continue;
            };
            match key {
                Key::Enter => {
                    let text: String = self.input.trim().chars().take(MAX_MESSAGE).collect();
                    if !text.is_empty() {
                        if self.sent.last() != Some(&text) {
                            self.sent.push(text.clone());
                            if self.sent.len() > KEPT {
                                self.sent.remove(0);
                            }
                        }
                        action = Some(ChatAction::Send(text));
                    }
                    self.close();
                }
                Key::Tab if self.input.starts_with('/') => {
                    if self.completed.is_some() {
                        self.cycle_completion();
                    } else {
                        action = Some(ChatAction::Complete(self.input.clone()));
                    }
                }
                Key::ArrowUp => self.recall(true),
                Key::ArrowDown => self.recall(false),
                _ => {}
            }
        }
        action
    }

    /// The lines to draw now, newest last: the recent ones while closed,
    /// with how faded each is, or a scrolled page of them while open.
    fn visible(&self, now: f64, settings: &ChatSettings) -> Vec<(&Line, f32)> {
        let shown = |line: &&Line| match settings.visibility {
            Visibility::Shown => true,
            Visibility::CommandsOnly => line.command,
            Visibility::Hidden => false,
        };
        if self.open {
            let page: Vec<&Line> = self.lines.iter().rev().filter(shown).skip(self.scroll).take(OPEN_LINES).collect();
            page.into_iter().rev().map(|line| (line, 1.0)).collect()
        } else {
            let recent: Vec<&Line> = self.lines.iter().rev().filter(shown).take(CLOSED_LINES).collect();
            recent
                .into_iter()
                .rev()
                .filter_map(|line| {
                    let age = now - line.at;
                    (age < SHOWN).then(|| (line, ((SHOWN - age) / FADE).min(1.0) as f32))
                })
                .collect()
        }
    }

    /// Draws the chat over the bottom left; while open, the box too.
    /// Returns what was asked for.
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        settings: &ChatSettings,
        now: f64,
    ) -> Option<ChatAction> {
        let full = ctx.content_rect();
        let scale = gui::gui_scale(full);
        let size = (8.0 * scale * settings.scale).round().max(8.0);
        let row = size * (9.0 / 8.0) * (1.0 + settings.spacing);
        let width = settings.width * scale;
        let left = full.left() + 2.0 * scale;
        let bottom = full.bottom() - 40.0 * scale;
        let mut action = None;
        if self.open {
            action = self.keys(ctx);
            let scrolled = ctx.input(|i| i.smooth_scroll_delta.y);
            if scrolled > 0.0 {
                self.scroll = (self.scroll + 1).min(self.lines.len().saturating_sub(1));
            } else if scrolled < 0.0 {
                self.scroll = self.scroll.saturating_sub(1);
            }
        }
        let painter = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::new("chat"),
        ));
        let lines = self.visible(now, settings);
        for (i, (line, fade)) in lines.iter().rev().enumerate() {
            let y = bottom - row * (i + 1) as f32;
            let band = Rect::from_min_size(Pos2::new(left - scale, y), vec2(width, row));
            let back = (settings.background * fade * 255.0) as u8;
            painter.rect_filled(band, 0.0, Color32::from_black_alpha(back));
            let alpha = (settings.opacity * fade * 255.0) as u8;
            let color = Color32::from_rgba_unmultiplied(255, 255, 255, alpha);
            font::shadowed(
                &painter,
                Pos2::new(left, y + (row - size * 9.0 / 8.0) / 2.0),
                egui::Align2::LEFT_TOP,
                &line.text,
                color,
                size,
            );
        }
        if self.open {
            let box_rect = Rect::from_min_max(
                Pos2::new(left - scale, full.bottom() - 14.0 * scale),
                Pos2::new(full.right() - 2.0 * scale, full.bottom() - 2.0 * scale),
            );
            egui::Area::new(egui::Id::new("chat-box"))
                .fixed_pos(box_rect.min)
                .order(egui::Order::Foreground)
                .show(ctx, |ui| {
                    painter.rect_filled(box_rect, 0.0, Color32::from_black_alpha(128));
                    let edit = egui::TextEdit::singleline(&mut self.input)
                        .char_limit(MAX_MESSAGE)
                        .frame(false)
                        .text_color(Color32::WHITE)
                        .font(egui::FontId::proportional(size))
                        .desired_width(box_rect.width());
                    let response = ui.add(edit);
                    if self.focus {
                        response.request_focus();
                        self.focus = false;
                    }
                    if response.changed() {
                        self.completions.clear();
                        self.completed = None;
                    }
                });
        }
        action
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closed_chat_shows_the_last_ten_lines_fading_after_ten_seconds() {
        let mut chat = Chat::default();
        for i in 0..15 {
            chat.push(format!("line {i}"), false, f64::from(i));
        }
        let settings = ChatSettings::default();
        let seen = chat.visible(14.0, &settings);
        assert_eq!(seen.len(), 10);
        assert_eq!(seen.last().unwrap().0.text, "line 14");
        // Line 5 came at 5 s: at 14.5 s it's half faded, at 15 s gone.
        let at = |chat: &Chat, now: f64| {
            chat.visible(now, &settings)
                .iter()
                .find(|(line, _)| line.text == "line 5")
                .map(|&(_, fade)| fade)
        };
        assert_eq!(at(&chat, 14.5), Some(0.5));
        assert_eq!(at(&chat, 15.0), None);
        // Open, every line shows again, scrolled back with the wheel.
        chat.open("");
        assert_eq!(chat.visible(1000.0, &settings).len(), 15);
        chat.scroll = 3;
        assert_eq!(chat.visible(1000.0, &settings).last().unwrap().0.text, "line 11");
    }

    #[test]
    fn a_hundred_lines_are_kept_and_commands_only_hides_the_rest() {
        let mut chat = Chat::default();
        for i in 0..120 {
            chat.push(format!("{i}"), i % 2 == 0, 0.0);
        }
        assert_eq!(chat.lines.len(), KEPT);
        assert_eq!(chat.lines.front().unwrap().text, "20");
        let settings = ChatSettings {
            visibility: Visibility::CommandsOnly,
            ..ChatSettings::default()
        };
        assert!(chat.visible(1.0, &settings).iter().all(|(line, _)| line.command));
        let hidden = ChatSettings {
            visibility: Visibility::Hidden,
            ..ChatSettings::default()
        };
        assert!(chat.visible(1.0, &hidden).is_empty());
    }

    #[test]
    fn up_and_down_walk_through_what_was_sent_and_tab_cycles_completions() {
        let mut chat = Chat::default();
        chat.sent = vec!["hello".into(), "/time set day".into()];
        chat.open("");
        chat.recall(true);
        assert_eq!(chat.input, "/time set day");
        chat.recall(true);
        assert_eq!(chat.input, "hello");
        chat.recall(false);
        assert_eq!(chat.input, "/time set day");
        chat.recall(false);
        assert_eq!(chat.input, "", "past the newest is an empty box");
        chat.open("/gam");
        chat.completions(vec!["gamemode".into(), "gamerule".into()]);
        assert_eq!(chat.input, "/gamemode");
        chat.cycle_completion();
        assert_eq!(chat.input, "/gamerule");
        chat.cycle_completion();
        assert_eq!(chat.input, "/gamemode");
        chat.open("/give Kenzo dia");
        chat.completions(vec!["diamond".into(), "diamond_sword".into()]);
        assert_eq!(chat.input, "/give Kenzo diamond");
    }
}
