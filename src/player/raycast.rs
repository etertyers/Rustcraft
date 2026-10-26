//! Voxel raycast (Amanatides–Woo DDA): finds the first block a ray hits,
//! either any cell a test accepts or, for the crosshair, the first block
//! outline box it actually passes through.

use glam::{DVec3, IVec3};

use crate::world::{Face, shape::Cuboid};

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Hit {
    pub block: IVec3,
    /// The face the ray entered through; `None` if it started inside the block.
    pub face: Option<Face>,
    /// Where the ray hit.
    pub point: DVec3,
}

impl Hit {
    /// The cell in front of the hit face, where a placed block goes.
    pub fn adjacent(&self) -> Option<IVec3> {
        self.face
            .map(|face| self.block + IVec3::from_array(face.normal()))
    }

    /// How high up its block the ray hit, 0–1 (top or bottom halves pick
    /// how slabs and stairs are placed).
    pub fn height_in_block(&self) -> f64 {
        self.point.y - self.block.y as f64
    }
}

/// The face a ray crossing a boundary along `axis` in direction `step` enters.
fn entered(axis: usize, step: i32) -> Face {
    match (axis, step > 0) {
        (0, true) => Face::West,
        (0, false) => Face::East,
        (1, true) => Face::Bottom,
        (1, false) => Face::Top,
        (_, true) => Face::North,
        (_, false) => Face::South,
    }
}

/// Walks the cells along `dir` (normalized) from `origin` up to `reach`
/// blocks, calling `visit(cell, face entered, distance entered)` until it
/// returns a hit.
fn walk(
    origin: DVec3,
    dir: DVec3,
    reach: f64,
    mut visit: impl FnMut(IVec3, Option<Face>, f64) -> Option<Hit>,
) -> Option<Hit> {
    let mut cell = origin.floor().as_ivec3();
    if let Some(hit) = visit(cell, None, 0.0) {
        return Some(hit);
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
        let t = t_max[axis];
        if t > reach {
            return None;
        }
        cell[axis] += step[axis];
        t_max[axis] += t_delta[axis];
        if let Some(hit) = visit(cell, Some(entered(axis, step[axis])), t) {
            return Some(hit);
        }
    }
}

/// Walks the cells along `dir` from `origin` up to `reach` blocks and
/// returns the first one `is_target` accepts, hit where the ray enters it.
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
    walk(origin, dir, reach, |cell, face, t| {
        is_target(cell).then(|| Hit {
            block: cell,
            face,
            point: origin + dir * t,
        })
    })
}

/// The first block whose boxes (`boxes(cell)`, in the cell, such as its
/// outline) the ray from `origin` along `dir` passes through within
/// `reach`, and the face of the box it entered.
pub fn raycast_boxes(
    origin: DVec3,
    dir: DVec3,
    reach: f64,
    mut boxes: impl FnMut(IVec3) -> &'static [Cuboid],
) -> Option<Hit> {
    let dir = dir.normalize_or_zero();
    if dir == DVec3::ZERO {
        return None;
    }
    walk(origin, dir, reach, |cell, _, _| {
        let corner = cell.as_dvec3();
        boxes(cell)
            .iter()
            .filter_map(|b| {
                let min = corner + DVec3::from(b.min.map(f64::from));
                let max = corner + DVec3::from(b.max.map(f64::from));
                ray_box(origin, dir, min, max)
            })
            .filter(|&(t, _)| t <= reach)
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(t, face)| Hit {
                block: cell,
                face,
                point: origin + dir * t,
            })
    })
}

/// Where a ray from `origin` along `dir` first meets the box `min`–`max`
/// (slab method): its distance and the face it enters through, or `None`
/// for the face if it starts inside.
fn ray_box(origin: DVec3, dir: DVec3, min: DVec3, max: DVec3) -> Option<(f64, Option<Face>)> {
    let mut near = f64::NEG_INFINITY;
    let mut far = f64::INFINITY;
    let mut face = None;
    for axis in 0..3 {
        if dir[axis] == 0.0 {
            if origin[axis] < min[axis] || origin[axis] > max[axis] {
                return None;
            }
            continue;
        }
        let (a, b) = (
            (min[axis] - origin[axis]) / dir[axis],
            (max[axis] - origin[axis]) / dir[axis],
        );
        let (t0, t1) = (a.min(b), a.max(b));
        if t0 > near {
            near = t0;
            face = Some(entered(axis, if dir[axis] > 0.0 { 1 } else { -1 }));
        }
        far = far.min(t1);
    }
    if near > far || far < 0.0 {
        return None;
    }
    if near < 0.0 {
        return Some((0.0, None));
    }
    Some((near, face))
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
    fn box_rays_hit_the_real_shape_and_pass_beside_it() {
        // A bottom slab at (0, 64, 2): a ray over it misses, one into its
        // side hits at the right height, and one from above hits its top.
        let slab = Cuboid::px(0.0, 0.0, 0.0, 16.0, 8.0, 16.0);
        let slabs: &'static [Cuboid] = Box::leak(Box::new([slab]));
        let at = IVec3::new(0, 64, 2);
        let boxes = |cell: IVec3| if cell == at { slabs } else { &[][..] };
        let over = raycast_boxes(DVec3::new(0.5, 64.75, 0.5), DVec3::Z, 5.0, boxes);
        assert_eq!(over, None, "passes over the slab");
        let side = raycast_boxes(DVec3::new(0.5, 64.25, 0.5), DVec3::Z, 5.0, boxes).unwrap();
        assert_eq!((side.block, side.face), (at, Some(Face::North)));
        assert!((side.point.z - 2.0).abs() < 1e-9);
        assert!((side.height_in_block() - 0.25).abs() < 1e-9);
        let down = DVec3::new(0.0, -1.0, 1.0).normalize();
        let top = raycast_boxes(DVec3::new(0.5, 65.5, 1.5), down, 5.0, boxes).unwrap();
        assert_eq!(top.face, Some(Face::Top));
        assert!((top.point.y - 64.5).abs() < 1e-9);
    }

    #[test]
    fn starting_inside_a_block_hits_it_with_no_face() {
        let target = IVec3::new(0, 0, 0);
        let hit = raycast(DVec3::splat(0.5), DVec3::Z, 5.0, only(target)).unwrap();
        assert_eq!(hit.face, None);
        assert_eq!(hit.adjacent(), None);
    }
}
