//! A one-line text box like vanilla's `EditBox`: a cursor, a selection,
//! backspace and delete, select-all, paste and a character limit. It
//! edits from egui's raw events so the game draws it in its own font.

use egui::{Event, Key};

#[derive(Debug, Clone, Default, PartialEq)]
pub struct TextField {
    pub text: String,
    /// Where the cursor is, in characters.
    pub cursor: usize,
    /// The other end of the selection, if one is held.
    pub anchor: Option<usize>,
    pub limit: usize,
}

impl TextField {
    pub fn new(text: &str, limit: usize) -> Self {
        let text: String = text.chars().take(limit).collect();
        let cursor = text.chars().count();
        TextField {
            text,
            cursor,
            anchor: None,
            limit,
        }
    }

    fn len(&self) -> usize {
        self.text.chars().count()
    }

    fn byte(&self, at: usize) -> usize {
        self.text
            .char_indices()
            .nth(at)
            .map_or(self.text.len(), |(i, _)| i)
    }

    /// The selected range, in characters, if it isn't empty.
    pub fn selection(&self) -> Option<(usize, usize)> {
        let anchor = self.anchor?;
        (anchor != self.cursor).then(|| (anchor.min(self.cursor), anchor.max(self.cursor)))
    }

    fn delete_selection(&mut self) -> bool {
        let Some((from, to)) = self.selection() else {
            return false;
        };
        let (a, b) = (self.byte(from), self.byte(to));
        self.text.replace_range(a..b, "");
        self.cursor = from;
        self.anchor = None;
        true
    }

    /// Types `typed` at the cursor over any selection, as much as fits.
    pub fn insert(&mut self, typed: &str) {
        self.delete_selection();
        let room = self.limit.saturating_sub(self.len());
        let typed: String = typed
            .chars()
            .filter(|&c| !c.is_control() && c != '§')
            .take(room)
            .collect();
        let at = self.byte(self.cursor);
        self.text.insert_str(at, &typed);
        self.cursor += typed.chars().count();
    }

    pub fn backspace(&mut self) {
        if !self.delete_selection() && self.cursor > 0 {
            let (a, b) = (self.byte(self.cursor - 1), self.byte(self.cursor));
            self.text.replace_range(a..b, "");
            self.cursor -= 1;
        }
    }

    pub fn delete(&mut self) {
        if !self.delete_selection() && self.cursor < self.len() {
            let (a, b) = (self.byte(self.cursor), self.byte(self.cursor + 1));
            self.text.replace_range(a..b, "");
        }
    }

    pub fn select_all(&mut self) {
        self.anchor = Some(0);
        self.cursor = self.len();
    }

    /// Moves the cursor, keeping (shift) or dropping the selection.
    fn move_to(&mut self, at: usize, shift: bool) {
        if shift {
            self.anchor.get_or_insert(self.cursor);
        } else {
            self.anchor = None;
        }
        self.cursor = at.min(self.len());
    }

    /// Applies a frame's typing. Returns whether the text changed.
    pub fn edit(&mut self, events: &[Event]) -> bool {
        let before = self.text.clone();
        for event in events {
            match event {
                Event::Text(typed) | Event::Paste(typed) => self.insert(typed),
                Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } => {
                    let shift = modifiers.shift;
                    match key {
                        Key::Backspace => self.backspace(),
                        Key::Delete => self.delete(),
                        Key::A if modifiers.command => self.select_all(),
                        Key::ArrowLeft if self.selection().is_some() && !shift => {
                            let (from, _) = self.selection().unwrap();
                            self.move_to(from, false);
                        }
                        Key::ArrowRight if self.selection().is_some() && !shift => {
                            let (_, to) = self.selection().unwrap();
                            self.move_to(to, false);
                        }
                        Key::ArrowLeft => self.move_to(self.cursor.saturating_sub(1), shift),
                        Key::ArrowRight => self.move_to(self.cursor + 1, shift),
                        Key::Home | Key::ArrowUp => self.move_to(0, shift),
                        Key::End | Key::ArrowDown => self.move_to(self.len(), shift),
                        _ => {}
                    }
                }
                _ => {}
            }
        }
        self.text != before
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(key: Key, command: bool, shift: bool) -> Event {
        Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers {
                shift,
                command,
                mac_cmd: command,
                ..Default::default()
            },
        }
    }

    #[test]
    fn typing_backspace_and_the_cursor() {
        let mut field = TextField::new("Pick", 50);
        assert_eq!(field.cursor, 4);
        assert!(field.edit(&[Event::Text("axe".into())]));
        assert_eq!(field.text, "Pickaxe");
        field.edit(&vec![key(Key::ArrowLeft, false, false); 3]);
        field.edit(&[Event::Text("-".into())]);
        assert_eq!(field.text, "Pick-axe");
        field.edit(&[key(Key::Backspace, false, false)]);
        assert_eq!((field.text.as_str(), field.cursor), ("Pickaxe", 4));
        field.edit(&[key(Key::Delete, false, false)]);
        assert_eq!(field.text, "Pickxe");
        assert!(!field.edit(&[key(Key::ArrowRight, false, false)]));
    }

    #[test]
    fn select_all_then_type_or_paste_replaces_everything() {
        let mut field = TextField::new("Diamond Pickaxe", 50);
        field.edit(&[key(Key::A, true, false), Event::Text("D".into())]);
        assert_eq!(field.text, "D");
        field.edit(&[key(Key::A, true, false), Event::Paste("Digger".into())]);
        assert_eq!((field.text.as_str(), field.cursor), ("Digger", 6));
        field.edit(&[
            key(Key::Home, false, false),
            key(Key::ArrowRight, false, true),
            key(Key::ArrowRight, false, true),
            key(Key::Backspace, false, false),
        ]);
        assert_eq!(field.text, "gger");
    }

    #[test]
    fn the_limit_is_fifty_characters() {
        let mut field = TextField::new("", 50);
        field.edit(&[Event::Paste("x".repeat(70))]);
        assert_eq!(field.text.chars().count(), 50);
        field.edit(&[Event::Text("y".into())]);
        assert_eq!(field.text.chars().count(), 50);
        assert!(!field.text.contains('y'));
        assert_eq!(TextField::new(&"z".repeat(60), 50).text.len(), 50);
    }

    #[test]
    fn control_characters_are_not_typed() {
        let mut field = TextField::new("", 50);
        field.edit(&[Event::Text("a\u{8}\tb".into())]);
        assert_eq!(field.text, "ab");
    }
}
