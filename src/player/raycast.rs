//! Voxel raycast (Amanatides–Woo DDA): finds the first block a ray hits.

use glam::{DVec3, IVec3};

use crate::world::Face;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hit {
    pub block: IVec3,
    /// The face the ray entered through; `None` if it started inside the block.
    pub face: Option<Face>,
}

impl Hit {
    /// The cell in front of the hit face, where a placed block goes.
    pub fn adjacent(&self) -> Option<IVec3> {
        self.face
            .map(|face| self.block + IVec3::from_array(face.normal()))
    }
}

/// Walks the cells along `dir` from `origin` up to `reach` blocks and
/// returns the first one `is_target` accepts.
pub fn raycast(
    origin: DVec3,
    dir: DVec3,
    reach: f64,
    mut is_target: impl FnMut(IVec3) -> bool,
) -> Option<Hit> {
    let dir = dir.normalize_or_zero();
    if dir == DVec3::ZERO {
        return None;
    }
    let mut cell = origin.floor().as_ivec3();
    if is_target(cell) {
        return Some(Hit {
            block: cell,
            face: None,
        });
    }
    let step = IVec3::from_array(dir.to_array().map(|d| {
        if d > 0.0 {
            1
        } else if d < 0.0 {
            -1
        } else {
            0
        }
    }));
    let t_delta = dir.recip().abs();
    let mut t_max = DVec3::ZERO;
    for axis in 0..3 {
        t_max[axis] = match step[axis] {
            1 => (cell[axis] as f64 + 1.0 - origin[axis]) / dir[axis],
            -1 => (origin[axis] - cell[axis] as f64) / -dir[axis],
            _ => f64::INFINITY,
        };
    }
    loop {
        let axis = if t_max.x <= t_max.y && t_max.x <= t_max.z {
            0
        } else if t_max.y <= t_max.z {
            1
        } else {
            2
        };
        if t_max[axis] > reach {
            return None;
        }
        cell[axis] += step[axis];
        t_max[axis] += t_delta[axis];
        if is_target(cell) {
            let face = match (axis, step[axis] > 0) {
                (0, true) => Face::West,
                (0, false) => Face::East,
                (1, true) => Face::Bottom,
                (1, false) => Face::Top,
                (_, true) => Face::North,
                (_, false) => Face::South,
            };
            return Some(Hit {
                block: cell,
                face: Some(face),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn only(target: IVec3) -> impl FnMut(IVec3) -> bool {
        move |cell| cell == target
    }

    #[test]
    fn hits_a_block_ahead_through_the_facing_face() {
        let target = IVec3::new(3, 64, 0);
        let hit = raycast(DVec3::new(0.5, 64.5, 0.5), DVec3::X, 5.0, only(target)).unwrap();
        assert_eq!(hit.block, target);
        assert_eq!(hit.face, Some(Face::West));
        assert_eq!(hit.adjacent(), Some(IVec3::new(2, 64, 0)));
    }

    #[test]
    fn looking_down_hits_the_top_face() {
        let target = IVec3::new(0, 63, 0);
        let hit = raycast(DVec3::new(0.5, 65.62, 0.5), DVec3::NEG_Y, 5.0, only(target)).unwrap();
        assert_eq!(hit.face, Some(Face::Top));
        assert_eq!(hit.adjacent(), Some(IVec3::new(0, 64, 0)));
    }

    #[test]
    fn respects_reach() {
        let origin = DVec3::new(0.5, 64.5, 0.5);
        // The near face of x = 5 is 4.5 away; x = 6 is 5.5 away.
        assert!(raycast(origin, DVec3::X, 5.0, only(IVec3::new(5, 64, 0))).is_some());
        assert!(raycast(origin, DVec3::X, 5.0, only(IVec3::new(6, 64, 0))).is_none());
    }

    #[test]
    fn diagonal_rays_visit_every_cell_they_cross() {
        let mut visited = Vec::new();
        let origin = DVec3::new(0.2, 0.5, 0.5);
        raycast(origin, DVec3::new(1.0, 0.0, -1.0), 3.0, |cell| {
            visited.push(cell);
            false
        });
        // Consecutive cells always share a face.
        for pair in visited.windows(2) {
            assert_eq!((pair[1] - pair[0]).abs().element_sum(), 1, "{visited:?}");
        }
        assert!(visited.contains(&IVec3::new(2, 0, -2)));
    }

    #[test]
    fn negative_directions_and_coordinates_work() {
        let target = IVec3::new(-3, 10, -7);
        let origin = DVec3::new(-0.5, 10.5, -6.5);
        let hit = raycast(origin, DVec3::NEG_X, 5.0, only(target)).unwrap();
        assert_eq!(hit.face, Some(Face::East));
        assert_eq!(hit.adjacent(), Some(IVec3::new(-2, 10, -7)));
    }

    #[test]
    fn starting_inside_a_block_hits_it_with_no_face() {
        let target = IVec3::new(0, 0, 0);
        let hit = raycast(DVec3::splat(0.5), DVec3::Z, 5.0, only(target)).unwrap();
        assert_eq!(hit.face, None);
        assert_eq!(hit.adjacent(), None);
    }
}
