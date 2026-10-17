//! Free-fly camera controller. A dev tool: from M4 it exists only in debug builds.

use glam::Vec3;
use winit::keyboard::KeyCode;

use crate::{config, render::camera::Camera};

/// Movement keys currently held.
#[derive(Debug, Default, Clone, Copy)]
pub struct FlyInput {
    forward: bool,
    back: bool,
    left: bool,
    right: bool,
    up: bool,
    down: bool,
}

impl FlyInput {
    /// Records a key press or release. Returns false if the key isn't a movement key.
    pub fn set_key(&mut self, key: KeyCode, pressed: bool) -> bool {
        let slot = match key {
            KeyCode::KeyW => &mut self.forward,
            KeyCode::KeyS => &mut self.back,
            KeyCode::KeyA => &mut self.left,
            KeyCode::KeyD => &mut self.right,
            KeyCode::Space => &mut self.up,
            KeyCode::ShiftLeft | KeyCode::ShiftRight => &mut self.down,
            _ => return false,
        };
        *slot = pressed;
        true
    }

    /// Releases every key, e.g. when the window loses focus.
    pub fn clear(&mut self) {
        *self = Self::default();
    }

    /// Unit direction (or zero) in world space. WASD move horizontally
    /// relative to yaw only, like Minecraft's creative flight.
    fn direction(&self, yaw: f32) -> Vec3 {
        let axis = |pos: bool, neg: bool| pos as i32 as f32 - neg as i32 as f32;
        let (sin_yaw, cos_yaw) = yaw.sin_cos();
        let forward = Vec3::new(sin_yaw, 0.0, -cos_yaw);
        let right = Vec3::new(cos_yaw, 0.0, sin_yaw);
        let dir = forward * axis(self.forward, self.back)
            + right * axis(self.right, self.left)
            + Vec3::Y * axis(self.up, self.down);
        dir.normalize_or_zero()
    }
}

/// Moves the camera for `dt` seconds of held keys.
pub fn fly(camera: &mut Camera, input: &FlyInput, dt: f32) {
    camera.position += input.direction(camera.yaw) * config::FLY_SPEED * dt;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::FRAC_PI_2;

    fn held(keys: &[KeyCode]) -> FlyInput {
        let mut input = FlyInput::default();
        for &key in keys {
            assert!(input.set_key(key, true));
        }
        input
    }

    #[test]
    fn w_moves_along_yaw_ignoring_pitch() {
        let mut camera = Camera::new(Vec3::ZERO, FRAC_PI_2, 1.0);
        fly(&mut camera, &held(&[KeyCode::KeyW]), 1.0);
        assert!(
            camera
                .position
                .abs_diff_eq(Vec3::X * config::FLY_SPEED, 1e-4)
        );
    }

    #[test]
    fn diagonal_is_not_faster() {
        let mut camera = Camera::new(Vec3::ZERO, 0.0, 0.0);
        fly(&mut camera, &held(&[KeyCode::KeyW, KeyCode::KeyD]), 1.0);
        assert!((camera.position.length() - config::FLY_SPEED).abs() < 1e-4);
    }

    #[test]
    fn space_and_shift_move_vertically_and_cancel() {
        let mut camera = Camera::new(Vec3::ZERO, 0.0, 0.0);
        fly(&mut camera, &held(&[KeyCode::ShiftLeft]), 0.5);
        assert!((camera.position.y + config::FLY_SPEED * 0.5).abs() < 1e-4);
        let before = camera.position;
        fly(
            &mut camera,
            &held(&[KeyCode::Space, KeyCode::ShiftLeft]),
            1.0,
        );
        assert_eq!(camera.position, before);
    }

    #[test]
    fn non_movement_keys_are_ignored() {
        assert!(!FlyInput::default().set_key(KeyCode::KeyQ, true));
    }
}
