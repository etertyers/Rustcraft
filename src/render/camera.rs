//! Perspective camera with yaw/pitch look.
//!
//! Axes follow Minecraft: +Y up, -Z north, +X east, +Z south, -X west.

use std::f32::consts::{FRAC_PI_2, TAU};

use glam::{
    Mat4, Vec3,
    camera::rh::{proj::directx::perspective, view::look_to_mat4},
};

use crate::config;

/// Keeps the camera from flipping over at straight up/down.
const MAX_PITCH: f32 = FRAC_PI_2 - 0.001;

/// The shake of walking, applied to the view (vanilla's `bobView`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ViewBob {
    /// Sideways and downward shift in blocks.
    pub sideways: f32,
    pub down: f32,
    /// Roll and pitch in radians.
    pub roll: f32,
    pub pitch: f32,
}

impl ViewBob {
    pub const NONE: Self = Self {
        sideways: 0.0,
        down: 0.0,
        roll: 0.0,
        pitch: 0.0,
    };

    /// The shake as a transform of camera space: shift, then roll, then
    /// pitch, as vanilla applies them to the view.
    pub fn matrix(&self) -> Mat4 {
        Mat4::from_translation(Vec3::new(self.sideways, self.down, 0.0))
            * Mat4::from_rotation_z(self.roll)
            * Mat4::from_rotation_x(self.pitch)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Camera {
    /// Eye position in world space.
    pub position: Vec3,
    /// Radians. 0 looks north (-Z); positive turns right, toward east (+X).
    pub yaw: f32,
    /// Radians. Positive looks up.
    pub pitch: f32,
    /// Vertical field of view in degrees.
    pub fov: f32,
    /// The shake of walking.
    pub bob: ViewBob,
}

impl Camera {
    pub fn new(position: Vec3, yaw: f32, pitch: f32) -> Self {
        let mut camera = Self {
            position,
            yaw: 0.0,
            pitch: 0.0,
            fov: config::FOV_Y_DEGREES,
            bob: ViewBob::NONE,
        };
        camera.rotate(yaw, pitch);
        camera
    }

    /// Turns by the given yaw and pitch deltas (radians), clamping pitch.
    pub fn rotate(&mut self, d_yaw: f32, d_pitch: f32) {
        self.yaw = (self.yaw + d_yaw).rem_euclid(TAU);
        self.pitch = (self.pitch + d_pitch).clamp(-MAX_PITCH, MAX_PITCH);
    }

    /// Unit vector the camera looks along.
    pub fn forward(&self) -> Vec3 {
        let (sin_yaw, cos_yaw) = self.yaw.sin_cos();
        let (sin_pitch, cos_pitch) = self.pitch.sin_cos();
        Vec3::new(sin_yaw * cos_pitch, sin_pitch, -cos_yaw * cos_pitch)
    }

    /// Nearest compass direction the camera faces horizontally.
    pub fn facing(&self) -> Facing {
        match (self.yaw / FRAC_PI_2).round() as u32 % 4 {
            0 => Facing::North,
            1 => Facing::East,
            2 => Facing::South,
            _ => Facing::West,
        }
    }

    pub fn view_proj(&self, aspect: f32) -> Mat4 {
        let view = look_to_mat4(self.position, self.forward(), Vec3::Y);
        // wgpu clip space matches DirectX: depth 0..1, Y up.
        let proj = perspective(self.fov.to_radians(), aspect, config::Z_NEAR, config::Z_FAR);
        proj * self.bob.matrix() * view
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Facing {
    North,
    East,
    South,
    West,
}

impl Facing {
    /// Minecraft's F3 wording, e.g. "north (Towards negative Z)".
    pub fn describe(self) -> &'static str {
        match self {
            Facing::North => "north (Towards negative Z)",
            Facing::East => "east (Towards positive X)",
            Facing::South => "south (Towards positive Z)",
            Facing::West => "west (Towards negative X)",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_vec_eq(a: Vec3, b: Vec3) {
        assert!(a.abs_diff_eq(b, 1e-5), "{a} != {b}");
    }

    #[test]
    fn yaw_zero_looks_north_and_quarter_turn_looks_east() {
        assert_vec_eq(Camera::new(Vec3::ZERO, 0.0, 0.0).forward(), Vec3::NEG_Z);
        assert_vec_eq(Camera::new(Vec3::ZERO, FRAC_PI_2, 0.0).forward(), Vec3::X);
    }

    #[test]
    fn facing_rounds_to_the_nearest_direction() {
        let facing = |degrees: f32| Camera::new(Vec3::ZERO, degrees.to_radians(), 0.0).facing();
        assert_eq!(facing(0.0), Facing::North);
        assert_eq!(facing(30.0), Facing::North);
        assert_eq!(facing(60.0), Facing::East);
        assert_eq!(facing(180.0), Facing::South);
        assert_eq!(facing(-90.0), Facing::West);
        assert_eq!(facing(350.0), Facing::North);
    }

    #[test]
    fn pitch_is_clamped_and_yaw_wraps() {
        let mut camera = Camera::new(Vec3::ZERO, 0.0, 0.0);
        camera.rotate(-0.5, 10.0);
        assert_eq!(camera.pitch, MAX_PITCH);
        assert!((0.0..TAU).contains(&camera.yaw));
        assert!(camera.forward().y > 0.99);
    }

    #[test]
    fn point_in_front_projects_to_screen_center() {
        let camera = Camera::new(Vec3::new(0.0, 64.0, 0.0), 0.0, 0.0);
        let clip = camera.view_proj(16.0 / 9.0) * glam::Vec4::new(0.0, 64.0, -10.0, 1.0);
        let ndc = clip.truncate() / clip.w;
        assert!(ndc.x.abs() < 1e-5 && ndc.y.abs() < 1e-5);
        assert!((0.0..1.0).contains(&ndc.z));
    }
}
