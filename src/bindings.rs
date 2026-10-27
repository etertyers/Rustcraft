//! Key bindings: every action the player can bind, its category and vanilla
//! default, and the keys and mouse buttons it can be bound to, named the way
//! vanilla's `options.txt` names them (`key.forward` → `key.keyboard.w`).

use std::collections::BTreeMap;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use winit::{event::MouseButton, keyboard::KeyCode};

/// A key or mouse button.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Input {
    Key(KeyCode),
    Mouse(MouseButton),
}

/// The Key Binds screen's headings, in order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Category {
    Movement,
    Gameplay,
    Inventory,
    Creative,
    Multiplayer,
    Misc,
    /// The key bindings mods register (M39).
    Mods,
    Debug,
}

impl Category {
    pub const ALL: [Category; 8] = [
        Category::Movement,
        Category::Gameplay,
        Category::Inventory,
        Category::Creative,
        Category::Multiplayer,
        Category::Misc,
        Category::Mods,
        Category::Debug,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Category::Movement => "Movement",
            Category::Gameplay => "Gameplay",
            Category::Inventory => "Inventory",
            Category::Creative => "Creative Mode",
            Category::Multiplayer => "Multiplayer",
            Category::Misc => "Miscellaneous",
            Category::Mods => "Mods",
            Category::Debug => "Debug",
        }
    }
}

macro_rules! actions {
    ($($action:ident, $id:literal, $name:literal, $category:ident, $default:expr;)*) => {
        /// Something a key or mouse button does.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
        pub enum Action {
            $($action,)*
        }

        impl Action {
            /// Every action, in the Key Binds screen's order within each
            /// category.
            pub const ALL: &[Action] = &[$(Action::$action,)*];

            /// Vanilla's name for it in `options.txt`.
            fn base_id(self) -> &'static str {
                match self {
                    $(Action::$action => $id,)*
                }
            }

            /// The Key Binds screen's label.
            fn base_name(self) -> &'static str {
                match self {
                    $(Action::$action => $name,)*
                }
            }

            pub fn category(self) -> Category {
                match self {
                    $(Action::$action => Category::$category,)*
                }
            }

            /// Vanilla's key for it.
            fn base_default(self) -> Option<Input> {
                use KeyCode as K;
                use MouseButton as M;
                #[allow(unused_imports)]
                use Input::{Key, Mouse};
                match self {
                    $(Action::$action => $default,)*
                }
            }
        }
    };
}

actions! {
    Jump, "key.jump", "Jump", Movement, Some(Key(K::Space));
    Sneak, "key.sneak", "Sneak", Movement, Some(Key(K::ShiftLeft));
    Sprint, "key.sprint", "Sprint", Movement, Some(Key(K::ControlLeft));
    Left, "key.left", "Strafe Left", Movement, Some(Key(K::KeyA));
    Right, "key.right", "Strafe Right", Movement, Some(Key(K::KeyD));
    Back, "key.back", "Walk Backwards", Movement, Some(Key(K::KeyS));
    Forward, "key.forward", "Walk Forwards", Movement, Some(Key(K::KeyW));
    Attack, "key.attack", "Attack/Destroy", Gameplay, Some(Mouse(M::Left));
    PickBlock, "key.pickItem", "Pick Block", Gameplay, Some(Mouse(M::Middle));
    Use, "key.use", "Use Item/Place Block", Gameplay, Some(Mouse(M::Right));
    Drop, "key.drop", "Drop Selected Item", Inventory, Some(Key(K::KeyQ));
    Hotbar1, "key.hotbar.1", "Hotbar Slot 1", Inventory, Some(Key(K::Digit1));
    Hotbar2, "key.hotbar.2", "Hotbar Slot 2", Inventory, Some(Key(K::Digit2));
    Hotbar3, "key.hotbar.3", "Hotbar Slot 3", Inventory, Some(Key(K::Digit3));
    Hotbar4, "key.hotbar.4", "Hotbar Slot 4", Inventory, Some(Key(K::Digit4));
    Hotbar5, "key.hotbar.5", "Hotbar Slot 5", Inventory, Some(Key(K::Digit5));
    Hotbar6, "key.hotbar.6", "Hotbar Slot 6", Inventory, Some(Key(K::Digit6));
    Hotbar7, "key.hotbar.7", "Hotbar Slot 7", Inventory, Some(Key(K::Digit7));
    Hotbar8, "key.hotbar.8", "Hotbar Slot 8", Inventory, Some(Key(K::Digit8));
    Hotbar9, "key.hotbar.9", "Hotbar Slot 9", Inventory, Some(Key(K::Digit9));
    Inventory, "key.inventory", "Open/Close Inventory", Inventory, Some(Key(K::KeyE));
    SwapHands, "key.swapOffhand", "Swap Item With Offhand", Inventory, Some(Key(K::KeyF));
    CaptureCorner1, "key.capture.corner1", "Mark Capture Corner 1", Creative, Some(Key(K::F8));
    CaptureCorner2, "key.capture.corner2", "Mark Capture Corner 2", Creative, Some(Key(K::F9));
    CaptureSave, "key.capture.save", "Save Capture", Creative, Some(Key(K::F10));
    Chat, "key.chat", "Open Chat", Multiplayer, Some(Key(K::KeyT));
    PlayerList, "key.playerlist", "List Players", Multiplayer, Some(Key(K::Tab));
    Command, "key.command", "Open Command", Multiplayer, Some(Key(K::Slash));
    Advancements, "key.advancements", "Advancements", Misc, Some(Key(K::KeyL));
    Screenshot, "key.screenshot", "Take Screenshot", Misc, Some(Key(K::F2));
    Perspective, "key.togglePerspective", "Toggle Perspective", Misc, Some(Key(K::F5));
    Fullscreen, "key.fullscreen", "Toggle Fullscreen", Misc, Some(Key(K::F11));
    HideGui, "key.hideGui", "Hide GUI", Misc, Some(Key(K::F1));
    DebugOverlay, "key.debug.overlay", "Debug Screen", Misc, Some(Key(K::F3));
    DevFlight, "key.debug.flight", "Toggle Dev Flight", Debug, Some(Key(K::F4));
    DevKit, "key.debug.kit", "Give Dev Kit", Debug, Some(Key(K::F6));
    SkipHour, "key.debug.skipHour", "Skip an Hour", Debug, Some(Key(K::F7));
    Mod1, "key.mod.1", "Mod Key 1", Mods, None;
    Mod2, "key.mod.2", "Mod Key 2", Mods, None;
    Mod3, "key.mod.3", "Mod Key 3", Mods, None;
    Mod4, "key.mod.4", "Mod Key 4", Mods, None;
    Mod5, "key.mod.5", "Mod Key 5", Mods, None;
    Mod6, "key.mod.6", "Mod Key 6", Mods, None;
    Mod7, "key.mod.7", "Mod Key 7", Mods, None;
    Mod8, "key.mod.8", "Mod Key 8", Mods, None;
}

/// How many keys mods can register (the Mod1…Mod8 actions).
pub const MOD_KEYS: usize = 8;

impl Action {
    /// Which of the mods' keys this is (0 for `Mod1`).
    pub fn mod_slot(self) -> Option<usize> {
        const SLOTS: [Action; MOD_KEYS] = [
            Action::Mod1,
            Action::Mod2,
            Action::Mod3,
            Action::Mod4,
            Action::Mod5,
            Action::Mod6,
            Action::Mod7,
            Action::Mod8,
        ];
        SLOTS.iter().position(|&a| a == self)
    }

    /// The `mod:key` id of the mod key bound to this action, if some mod
    /// registered one for its slot.
    pub fn mod_key_id(self) -> Option<&'static str> {
        crate::mods::active().key_ids.get(self.mod_slot()?).copied()
    }

    /// Vanilla's name for it in `options.txt`; a mod key is
    /// `key.mod.<mod>:<key>`, so it keeps its binding when mods change.
    pub fn id(self) -> &'static str {
        match self.mod_slot() {
            Some(slot) => crate::mods::active()
                .key_option_ids
                .get(slot)
                .copied()
                .unwrap_or(self.base_id()),
            None => self.base_id(),
        }
    }

    /// The Key Binds screen's label.
    pub fn name(self) -> &'static str {
        match self.mod_slot() {
            Some(slot) => crate::mods::active()
                .key_names
                .get(slot)
                .copied()
                .unwrap_or(self.base_name()),
            None => self.base_name(),
        }
    }

    /// Vanilla's key for it (a mod key: the one its mod asks for).
    pub fn default_input(self) -> Option<Input> {
        match self.mod_slot() {
            Some(slot) => crate::mods::active()
                .keys
                .get(slot)
                .and_then(|k| key_from_label(&k.default)),
            None => self.base_default(),
        }
    }

    /// The hotbar slot (0–8) a Hotbar Slot action picks.
    pub fn hotbar_slot(self) -> Option<usize> {
        const SLOTS: [Action; 9] = [
            Action::Hotbar1,
            Action::Hotbar2,
            Action::Hotbar3,
            Action::Hotbar4,
            Action::Hotbar5,
            Action::Hotbar6,
            Action::Hotbar7,
            Action::Hotbar8,
            Action::Hotbar9,
        ];
        SLOTS.iter().position(|&a| a == self)
    }

    pub fn from_id(id: &str) -> Option<Action> {
        Action::ALL.iter().copied().find(|a| a.id() == id)
    }

    /// Shown on the Key Binds screen: the Debug keys only do anything in a
    /// debug build.
    pub fn listed(self) -> bool {
        match self.mod_slot() {
            Some(slot) => slot < crate::mods::active().keys.len(),
            None => self.category() != Category::Debug || cfg!(debug_assertions),
        }
    }
}

/// Every action's key; an action missing from the map has its default, and
/// `None` is unbound.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Bindings {
    changed: BTreeMap<Action, Option<Input>>,
}

impl Bindings {
    pub fn get(&self, action: Action) -> Option<Input> {
        match self.changed.get(&action) {
            Some(input) => *input,
            None => action.default_input(),
        }
    }

    pub fn set(&mut self, action: Action, input: Option<Input>) {
        if input == action.default_input() {
            self.changed.remove(&action);
        } else {
            self.changed.insert(action, input);
        }
    }

    pub fn reset(&mut self, action: Action) {
        self.changed.remove(&action);
    }

    pub fn reset_all(&mut self) {
        self.changed.clear();
    }

    pub fn is_default(&self, action: Action) -> bool {
        !self.changed.contains_key(&action)
    }

    pub fn all_default(&self) -> bool {
        self.changed.is_empty()
    }

    /// Every action `input` does (more than one when keys conflict; like
    /// vanilla, they all happen).
    pub fn actions(&self, input: Input) -> impl Iterator<Item = Action> + '_ {
        Action::ALL
            .iter()
            .copied()
            .filter(move |&a| self.get(a) == Some(input))
    }

    /// Whether another listed action shares `action`'s key, which the Key
    /// Binds screen shows in red.
    pub fn conflicts(&self, action: Action) -> bool {
        let Some(input) = self.get(action) else {
            return false;
        };
        Action::ALL
            .iter()
            .any(|&other| other != action && other.listed() && self.get(other) == Some(input))
    }
}

impl Serialize for Bindings {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let map: BTreeMap<&str, String> = Action::ALL
            .iter()
            .map(|&a| (a.id(), self.get(a).map_or(UNBOUND.to_owned(), input_id)))
            .collect();
        map.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Bindings {
    /// Unknown actions and keys are skipped, so older and newer files load.
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let map = BTreeMap::<String, String>::deserialize(deserializer)?;
        let mut bindings = Bindings::default();
        for (action, input) in map {
            let Some(action) = Action::from_id(&action) else {
                continue;
            };
            if input == UNBOUND {
                bindings.set(action, None);
            } else if let Some(input) = input_from_id(&input) {
                bindings.set(action, Some(input));
            }
        }
        Ok(bindings)
    }
}

/// Vanilla's name for no key.
const UNBOUND: &str = "key.keyboard.unknown";

/// Keys with vanilla's `options.txt` id and the
/// name the Key Binds screen shows.
const KEYS: &[(KeyCode, &str, &str)] = {
    use KeyCode as K;
    &[
        (K::KeyA, "key.keyboard.a", "A"),
        (K::KeyB, "key.keyboard.b", "B"),
        (K::KeyC, "key.keyboard.c", "C"),
        (K::KeyD, "key.keyboard.d", "D"),
        (K::KeyE, "key.keyboard.e", "E"),
        (K::KeyF, "key.keyboard.f", "F"),
        (K::KeyG, "key.keyboard.g", "G"),
        (K::KeyH, "key.keyboard.h", "H"),
        (K::KeyI, "key.keyboard.i", "I"),
        (K::KeyJ, "key.keyboard.j", "J"),
        (K::KeyK, "key.keyboard.k", "K"),
        (K::KeyL, "key.keyboard.l", "L"),
        (K::KeyM, "key.keyboard.m", "M"),
        (K::KeyN, "key.keyboard.n", "N"),
        (K::KeyO, "key.keyboard.o", "O"),
        (K::KeyP, "key.keyboard.p", "P"),
        (K::KeyQ, "key.keyboard.q", "Q"),
        (K::KeyR, "key.keyboard.r", "R"),
        (K::KeyS, "key.keyboard.s", "S"),
        (K::KeyT, "key.keyboard.t", "T"),
        (K::KeyU, "key.keyboard.u", "U"),
        (K::KeyV, "key.keyboard.v", "V"),
        (K::KeyW, "key.keyboard.w", "W"),
        (K::KeyX, "key.keyboard.x", "X"),
        (K::KeyY, "key.keyboard.y", "Y"),
        (K::KeyZ, "key.keyboard.z", "Z"),
        (K::Digit0, "key.keyboard.0", "0"),
        (K::Digit1, "key.keyboard.1", "1"),
        (K::Digit2, "key.keyboard.2", "2"),
        (K::Digit3, "key.keyboard.3", "3"),
        (K::Digit4, "key.keyboard.4", "4"),
        (K::Digit5, "key.keyboard.5", "5"),
        (K::Digit6, "key.keyboard.6", "6"),
        (K::Digit7, "key.keyboard.7", "7"),
        (K::Digit8, "key.keyboard.8", "8"),
        (K::Digit9, "key.keyboard.9", "9"),
        (K::F1, "key.keyboard.f1", "F1"),
        (K::F2, "key.keyboard.f2", "F2"),
        (K::F3, "key.keyboard.f3", "F3"),
        (K::F4, "key.keyboard.f4", "F4"),
        (K::F5, "key.keyboard.f5", "F5"),
        (K::F6, "key.keyboard.f6", "F6"),
        (K::F7, "key.keyboard.f7", "F7"),
        (K::F8, "key.keyboard.f8", "F8"),
        (K::F9, "key.keyboard.f9", "F9"),
        (K::F10, "key.keyboard.f10", "F10"),
        (K::F11, "key.keyboard.f11", "F11"),
        (K::F12, "key.keyboard.f12", "F12"),
        (K::F13, "key.keyboard.f13", "F13"),
        (K::F14, "key.keyboard.f14", "F14"),
        (K::F15, "key.keyboard.f15", "F15"),
        (K::F16, "key.keyboard.f16", "F16"),
        (K::F17, "key.keyboard.f17", "F17"),
        (K::F18, "key.keyboard.f18", "F18"),
        (K::F19, "key.keyboard.f19", "F19"),
        (K::F20, "key.keyboard.f20", "F20"),
        (K::Numpad0, "key.keyboard.keypad.0", "Keypad 0"),
        (K::Numpad1, "key.keyboard.keypad.1", "Keypad 1"),
        (K::Numpad2, "key.keyboard.keypad.2", "Keypad 2"),
        (K::Numpad3, "key.keyboard.keypad.3", "Keypad 3"),
        (K::Numpad4, "key.keyboard.keypad.4", "Keypad 4"),
        (K::Numpad5, "key.keyboard.keypad.5", "Keypad 5"),
        (K::Numpad6, "key.keyboard.keypad.6", "Keypad 6"),
        (K::Numpad7, "key.keyboard.keypad.7", "Keypad 7"),
        (K::Numpad8, "key.keyboard.keypad.8", "Keypad 8"),
        (K::Numpad9, "key.keyboard.keypad.9", "Keypad 9"),
        (K::NumpadAdd, "key.keyboard.keypad.add", "Keypad +"),
        (
            K::NumpadDecimal,
            "key.keyboard.keypad.decimal",
            "Keypad Decimal",
        ),
        (K::NumpadEnter, "key.keyboard.keypad.enter", "Keypad Enter"),
        (K::NumpadEqual, "key.keyboard.keypad.equal", "Keypad ="),
        (
            K::NumpadMultiply,
            "key.keyboard.keypad.multiply",
            "Keypad *",
        ),
        (K::NumpadDivide, "key.keyboard.keypad.divide", "Keypad /"),
        (
            K::NumpadSubtract,
            "key.keyboard.keypad.subtract",
            "Keypad -",
        ),
        (K::ArrowDown, "key.keyboard.down", "Down Arrow"),
        (K::ArrowLeft, "key.keyboard.left", "Left Arrow"),
        (K::ArrowRight, "key.keyboard.right", "Right Arrow"),
        (K::ArrowUp, "key.keyboard.up", "Up Arrow"),
        (K::Quote, "key.keyboard.apostrophe", "'"),
        (K::Backslash, "key.keyboard.backslash", "\\"),
        (K::Comma, "key.keyboard.comma", ","),
        (K::Equal, "key.keyboard.equal", "="),
        (K::Backquote, "key.keyboard.grave.accent", "`"),
        (K::BracketLeft, "key.keyboard.left.bracket", "["),
        (K::Minus, "key.keyboard.minus", "-"),
        (K::Period, "key.keyboard.period", "."),
        (K::BracketRight, "key.keyboard.right.bracket", "]"),
        (K::Semicolon, "key.keyboard.semicolon", ";"),
        (K::Slash, "key.keyboard.slash", "/"),
        (K::Space, "key.keyboard.space", "Space"),
        (K::Tab, "key.keyboard.tab", "Tab"),
        (K::AltLeft, "key.keyboard.left.alt", "Left Option"),
        (K::ControlLeft, "key.keyboard.left.control", "Left Control"),
        (K::ShiftLeft, "key.keyboard.left.shift", "Left Shift"),
        (K::SuperLeft, "key.keyboard.left.win", "Left Cmd"),
        (K::AltRight, "key.keyboard.right.alt", "Right Option"),
        (
            K::ControlRight,
            "key.keyboard.right.control",
            "Right Control",
        ),
        (K::ShiftRight, "key.keyboard.right.shift", "Right Shift"),
        (K::SuperRight, "key.keyboard.right.win", "Right Cmd"),
        (K::Enter, "key.keyboard.enter", "Enter"),
        (K::Escape, "key.keyboard.escape", "Escape"),
        (K::Backspace, "key.keyboard.backspace", "Backspace"),
        (K::Delete, "key.keyboard.delete", "Delete"),
        (K::End, "key.keyboard.end", "End"),
        (K::Home, "key.keyboard.home", "Home"),
        (K::Insert, "key.keyboard.insert", "Insert"),
        (K::PageDown, "key.keyboard.page.down", "Page Down"),
        (K::PageUp, "key.keyboard.page.up", "Page Up"),
        (K::CapsLock, "key.keyboard.caps.lock", "Caps Lock"),
        (K::Pause, "key.keyboard.pause", "Pause"),
        (K::ScrollLock, "key.keyboard.scroll.lock", "Scroll Lock"),
        (K::NumLock, "key.keyboard.num.lock", "Num Lock"),
        (K::PrintScreen, "key.keyboard.print.screen", "Print Screen"),
        (K::ContextMenu, "key.keyboard.menu", "Menu"),
        (K::IntlBackslash, "key.keyboard.world.1", "World 1"),
    ]
};

/// Vanilla's id for a key or mouse button.
pub fn input_id(input: Input) -> String {
    match input {
        Input::Key(key) => KEYS
            .iter()
            .find(|(k, ..)| *k == key)
            .map_or(UNBOUND, |(_, id, _)| id)
            .to_owned(),
        Input::Mouse(button) => match button {
            MouseButton::Left => "key.mouse.left".to_owned(),
            MouseButton::Right => "key.mouse.right".to_owned(),
            MouseButton::Middle => "key.mouse.middle".to_owned(),
            MouseButton::Back => "key.mouse.4".to_owned(),
            MouseButton::Forward => "key.mouse.5".to_owned(),
            MouseButton::Other(n) => format!("key.mouse.{}", n + 1),
        },
    }
}

/// A key by the name the Key Binds screen shows ("G", "F7", "Keypad 1").
pub fn key_from_label(label: &str) -> Option<Input> {
    KEYS.iter()
        .find(|(_, _, name)| name.eq_ignore_ascii_case(label))
        .map(|(k, ..)| Input::Key(*k))
}

pub fn input_from_id(id: &str) -> Option<Input> {
    if id.starts_with("key.keyboard.") {
        return KEYS
            .iter()
            .find(|(_, k, _)| *k == id)
            .map(|(k, ..)| Input::Key(*k));
    }
    let button = match id.strip_prefix("key.mouse.")? {
        "left" => MouseButton::Left,
        "right" => MouseButton::Right,
        "middle" => MouseButton::Middle,
        "4" => MouseButton::Back,
        "5" => MouseButton::Forward,
        n => MouseButton::Other(n.parse::<u16>().ok()?.checked_sub(1)?),
    };
    Some(Input::Mouse(button))
}

/// The Key Binds screen's name for a key or button.
pub fn input_name(input: Option<Input>) -> String {
    match input {
        None => "Not Bound".to_owned(),
        Some(Input::Key(key)) => KEYS
            .iter()
            .find(|(k, ..)| *k == key)
            .map_or_else(|| format!("{key:?}"), |(.., name)| (*name).to_owned()),
        Some(Input::Mouse(button)) => match button {
            MouseButton::Left => "Left Button".to_owned(),
            MouseButton::Right => "Right Button".to_owned(),
            MouseButton::Middle => "Middle Button".to_owned(),
            MouseButton::Back => "Button 4".to_owned(),
            MouseButton::Forward => "Button 5".to_owned(),
            MouseButton::Other(n) => format!("Button {}", n + 1),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_vanillas() {
        let keys = Bindings::default();
        let key = |a| keys.get(a);
        assert_eq!(key(Action::Forward), Some(Input::Key(KeyCode::KeyW)));
        assert_eq!(key(Action::Sneak), Some(Input::Key(KeyCode::ShiftLeft)));
        assert_eq!(key(Action::Sprint), Some(Input::Key(KeyCode::ControlLeft)));
        assert_eq!(key(Action::Attack), Some(Input::Mouse(MouseButton::Left)));
        assert_eq!(key(Action::Chat), Some(Input::Key(KeyCode::KeyT)));
        assert_eq!(key(Action::Command), Some(Input::Key(KeyCode::Slash)));
        assert_eq!(key(Action::Hotbar9), Some(Input::Key(KeyCode::Digit9)));
        assert_eq!(Action::Hotbar9.hotbar_slot(), Some(8));
        // No two listed defaults share a key.
        assert!(Action::ALL.iter().all(|&a| !keys.conflicts(a)), "{keys:?}");
    }

    #[test]
    fn every_key_and_button_round_trips_through_its_vanilla_id() {
        for (key, ..) in KEYS {
            let input = Input::Key(*key);
            assert_eq!(input_from_id(&input_id(input)), Some(input), "{key:?}");
        }
        for button in [
            MouseButton::Left,
            MouseButton::Right,
            MouseButton::Middle,
            MouseButton::Back,
            MouseButton::Forward,
            MouseButton::Other(7),
        ] {
            let input = Input::Mouse(button);
            assert_eq!(input_from_id(&input_id(input)), Some(input));
        }
        assert_eq!(input_id(Input::Key(KeyCode::KeyW)), "key.keyboard.w");
        assert_eq!(input_id(Input::Mouse(MouseButton::Other(7))), "key.mouse.8");
        assert_eq!(input_from_id("key.keyboard.nonsense"), None);
        assert_eq!(input_name(None), "Not Bound");
        assert_eq!(
            input_name(Some(Input::Key(KeyCode::ShiftLeft))),
            "Left Shift"
        );
    }

    #[test]
    fn rebinding_conflicts_and_resetting() {
        let mut keys = Bindings::default();
        keys.set(Action::Jump, Some(Input::Key(KeyCode::KeyW)));
        assert!(keys.conflicts(Action::Jump) && keys.conflicts(Action::Forward));
        let both: Vec<_> = keys.actions(Input::Key(KeyCode::KeyW)).collect();
        assert_eq!(both, [Action::Jump, Action::Forward]);
        assert_eq!(keys.actions(Input::Key(KeyCode::Space)).count(), 0);
        keys.set(Action::Forward, None);
        assert!(!keys.conflicts(Action::Jump));
        assert_eq!(keys.get(Action::Forward), None);
        keys.reset(Action::Forward);
        assert!(keys.is_default(Action::Forward) && !keys.all_default());
        // Setting a key back to its default forgets the change.
        keys.set(Action::Jump, Some(Input::Key(KeyCode::Space)));
        assert!(keys.all_default());
        keys.set(Action::Drop, None);
        keys.reset_all();
        assert_eq!(keys, Bindings::default());
    }

    #[test]
    fn bindings_save_as_vanilla_ids_and_skip_what_they_dont_know() {
        let mut keys = Bindings::default();
        keys.set(Action::Jump, Some(Input::Mouse(MouseButton::Back)));
        keys.set(Action::Drop, None);
        let text = ron::to_string(&keys).unwrap();
        assert!(text.contains(r#""key.jump":"key.mouse.4""#), "{text}");
        assert!(
            text.contains(r#""key.drop":"key.keyboard.unknown""#),
            "{text}"
        );
        assert_eq!(ron::from_str::<Bindings>(&text).unwrap(), keys);
        let odd = r#"{"key.jump": "key.keyboard.g", "key.socialInteractions": "key.keyboard.p", "key.sneak": "key.keyboard.what"}"#;
        let loaded: Bindings = ron::from_str(odd).unwrap();
        assert_eq!(loaded.get(Action::Jump), Some(Input::Key(KeyCode::KeyG)));
        assert!(loaded.is_default(Action::Sneak));
    }
}
