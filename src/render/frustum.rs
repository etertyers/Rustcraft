//! View-frustum culling of axis-aligned boxes.

use glam::{Mat4, Vec3, Vec4};

/// The six planes of a view frustum, each `(normal, distance)` with the
/// normal pointing inward.
#[derive(Debug, Clone, Copy)]
pub struct Frustum {
    planes: [Vec4; 6],
}

impl Frustum {
    /// Extracts planes from a view-projection matrix with wgpu's 0..1 depth range
    /// (Gribb–Hartmann).
    pub fn from_view_proj(m: Mat4) -> Self {
        let [r0, r1, r2, r3] = [0, 1, 2, 3].map(|i| m.row(i));
        let planes =
            [r3 + r0, r3 - r0, r3 + r1, r3 - r1, r2, r3 - r2].map(|p| p / p.truncate().length());
        Self { planes }
    }

    /// Whether any part of the box `min..max` may be visible.
    pub fn intersects_box(&self, min: Vec3, max: Vec3) -> bool {
        self.planes.iter().all(|plane| {
            let normal = plane.truncate();
            // The box corner furthest along the plane normal.
            let corner = Vec3::select(normal.cmpge(Vec3::ZERO), max, min);
            normal.dot(corner) + plane.w >= 0.0
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::camera::Camera;

    fn frustum() -> Frustum {
        // At the origin looking north (-Z).
        Frustum::from_view_proj(Camera::new(Vec3::ZERO, 0.0, 0.0).view_proj(1.0))
    }

    #[test]
    fn box_ahead_is_visible_and_behind_is_not() {
        let f = frustum();
        assert!(f.intersects_box(Vec3::new(-1.0, -1.0, -11.0), Vec3::new(1.0, 1.0, -9.0)));
        assert!(!f.intersects_box(Vec3::new(-1.0, -1.0, 9.0), Vec3::new(1.0, 1.0, 11.0)));
    }

    #[test]
    fn boxes_off_to_the_side_or_too_far_are_culled() {
        let f = frustum();
        assert!(!f.intersects_box(Vec3::new(50.0, -1.0, -11.0), Vec3::new(52.0, 1.0, -9.0)));
        assert!(!f.intersects_box(Vec3::new(-1.0, -1.0, -2000.0), Vec3::new(1.0, 1.0, -1990.0)));
    }

    #[test]
    fn box_around_the_camera_is_visible() {
        assert!(frustum().intersects_box(Vec3::splat(-8.0), Vec3::splat(8.0)));
    }
}
