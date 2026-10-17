//! Free-fly camera controller: a dev tool that exists only in debug builds.

use glam::DVec3;

use super::input::{MoveKeys, axis};
use crate::config;

/// Unit direction (or zero) in world space. WASD move horizontally relative
/// to yaw only, like Minecraft's creative flight; Space and Shift move up and down.
fn direction(keys: &MoveKeys, yaw: f32) -> DVec3 {
    let (sin_yaw, cos_yaw) = (yaw as f64).sin_cos();
    let forward = DVec3::new(sin_yaw, 0.0, -cos_yaw);
    let right = DVec3::new(cos_yaw, 0.0, sin_yaw);
    let dir = forward * axis(keys.forward, keys.back)
        + right * axis(keys.right, keys.left)
        + DVec3::Y * axis(keys.jump, keys.sneak);
    dir.normalize_or_zero()
}

/// Moves `position` for `dt` seconds of held keys.
pub fn fly(position: &mut DVec3, yaw: f32, keys: &MoveKeys, dt: f32) {
    *position += direction(keys, yaw) * (config::FLY_SPEED * dt) as f64;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::FRAC_PI_2;
    use winit::keyboard::KeyCode;

    fn held(keys: &[KeyCode]) -> MoveKeys {
        let mut held = MoveKeys::default();
        for &key in keys {
            assert!(held.set_key(key, true));
        }
        held
    }

    const SPEED: f64 = config::FLY_SPEED as f64;

    #[test]
    fn w_moves_along_yaw() {
        let mut position = DVec3::ZERO;
        fly(&mut position, FRAC_PI_2, &held(&[KeyCode::KeyW]), 1.0);
        assert!(position.abs_diff_eq(DVec3::X * SPEED, 1e-4));
    }

    #[test]
    fn diagonal_is_not_faster() {
        let mut position = DVec3::ZERO;
        fly(
            &mut position,
            0.0,
            &held(&[KeyCode::KeyW, KeyCode::KeyD]),
            1.0,
        );
        assert!((position.length() - SPEED).abs() < 1e-4);
    }

    #[test]
    fn space_and_shift_move_vertically_and_cancel() {
        let mut position = DVec3::ZERO;
        fly(&mut position, 0.0, &held(&[KeyCode::ShiftLeft]), 0.5);
        assert!((position.y + SPEED * 0.5).abs() < 1e-4);
        let before = position;
        let both = held(&[KeyCode::Space, KeyCode::ShiftLeft]);
        fly(&mut position, 0.0, &both, 1.0);
        assert_eq!(position, before);
    }
}
