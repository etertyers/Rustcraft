//! Held movement keys.

use winit::keyboard::KeyCode;

use super::physics::MoveInput;

/// Movement keys currently held.
#[derive(Debug, Default, Clone, Copy)]
pub struct MoveKeys {
    pub forward: bool,
    pub back: bool,
    pub left: bool,
    pub right: bool,
    pub jump: bool,
    pub sneak: bool,
    pub sprint: bool,
}

impl MoveKeys {
    /// Records a key press or release. Returns false if the key isn't a movement key.
    pub fn set_key(&mut self, key: KeyCode, pressed: bool) -> bool {
        let slot = match key {
            KeyCode::KeyW => &mut self.forward,
            KeyCode::KeyS => &mut self.back,
            KeyCode::KeyA => &mut self.left,
            KeyCode::KeyD => &mut self.right,
            KeyCode::Space => &mut self.jump,
            KeyCode::ShiftLeft | KeyCode::ShiftRight => &mut self.sneak,
            KeyCode::ControlLeft => &mut self.sprint,
            _ => return false,
        };
        *slot = pressed;
        true
    }

    /// Releases every key, e.g. when the window loses focus.
    pub fn clear(&mut self) {
        *self = Self::default();
    }

    pub fn move_input(&self) -> MoveInput {
        MoveInput {
            forward: axis(self.forward, self.back),
            strafe: axis(self.right, self.left),
            jump: self.jump,
            sneak: self.sneak,
            sprint: self.sprint,
            ..MoveInput::default()
        }
    }
}

/// +1, −1 or 0 (both or neither held).
pub fn axis(pos: bool, neg: bool) -> f64 {
    pos as i32 as f64 - neg as i32 as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opposite_keys_cancel() {
        let mut keys = MoveKeys::default();
        keys.set_key(KeyCode::KeyW, true);
        keys.set_key(KeyCode::KeyS, true);
        keys.set_key(KeyCode::KeyA, true);
        let input = keys.move_input();
        assert_eq!((input.forward, input.strafe), (0.0, -1.0));
    }

    #[test]
    fn non_movement_keys_are_ignored() {
        assert!(!MoveKeys::default().set_key(KeyCode::KeyQ, true));
    }
}
