//! The End portal's frame (modern `EndPortalFrameBlock.getOrCreatePortalShape`):
//! twelve frames in a ring round a 3 × 3 middle, three along each side
//! with the corners left empty, each facing into the middle and holding an
//! eye of ender. The last eye placed lights the portal.

use glam::IVec3;

use super::{Face, block::BlockState, blocks, props::Prop};

/// The frames round `center`, with the way each one must face: into the
/// middle.
pub fn ring(center: IVec3) -> Vec<(IVec3, Face)> {
    let mut frames = Vec::with_capacity(12);
    for dz in -2..=2i32 {
        for dx in -2..=2i32 {
            let edge = dx.abs() == 2 || dz.abs() == 2;
            let corner = dx.abs() == 2 && dz.abs() == 2;
            if !edge || corner {
                continue;
            }
            let face = if dx == -2 {
                Face::East
            } else if dx == 2 {
                Face::West
            } else if dz == -2 {
                Face::South
            } else {
                Face::North
            };
            frames.push((center + IVec3::new(dx, 0, dz), face));
        }
    }
    frames
}

/// The nine cells the portal fills.
pub fn middle(center: IVec3) -> [IVec3; 9] {
    std::array::from_fn(|i| center + IVec3::new(i as i32 % 3 - 1, 0, i as i32 / 3 - 1))
}

/// Whether `state` is a frame facing `face` holding an eye.
fn is_filled(state: Option<BlockState>, face: Face) -> bool {
    state.is_some_and(|s| {
        s.is(blocks::END_PORTAL_FRAME) && s.bool(Prop::Eye) && s.face(Prop::Facing) == face
    })
}

/// The middle of the complete ring that has the frame at `at` in it, if
/// every one of its twelve frames holds an eye.
pub fn find_lit(at: IVec3, get: &impl Fn(IVec3) -> Option<BlockState>) -> Option<IVec3> {
    for dz in -2..=2i32 {
        for dx in -2..=2i32 {
            let center = at + IVec3::new(dx, 0, dz);
            let frames = ring(center);
            if frames.iter().any(|&(cell, _)| cell == at)
                && frames
                    .iter()
                    .all(|&(cell, face)| is_filled(get(cell), face))
            {
                return Some(center);
            }
        }
    }
    None
}

/// Whether the cell at `at` is a portal block.
pub fn is_portal(state: BlockState) -> bool {
    state.is(blocks::END_PORTAL)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    fn frame(face: Face, eye: bool) -> BlockState {
        BlockState::of(blocks::END_PORTAL_FRAME)
            .with_face(Prop::Facing, face)
            .with_bool(Prop::Eye, eye)
    }

    fn world(center: IVec3, eyes: bool) -> HashMap<IVec3, BlockState> {
        ring(center)
            .into_iter()
            .map(|(cell, face)| (cell, frame(face, eyes)))
            .collect()
    }

    #[test]
    fn a_ring_is_twelve_frames_facing_the_middle_without_corners() {
        let center = IVec3::new(10, 40, -7);
        let frames = ring(center);
        assert_eq!(frames.len(), 12);
        assert!(frames.iter().all(|(cell, _)| cell.y == 40));
        for (cell, face) in &frames {
            let [nx, _, nz] = face.normal();
            let step = *cell + IVec3::new(nx, 0, nz);
            let d = (step - center).abs();
            assert!(d.max_element() < (cell - center).abs().max_element());
        }
        assert!(
            !frames
                .iter()
                .any(|(c, _)| (c.x - center.x).abs() == 2 && (c.z - center.z).abs() == 2)
        );
        assert_eq!(middle(center).len(), 9);
        assert!(middle(center).contains(&center));
    }

    #[test]
    fn the_twelfth_eye_lights_the_portal_and_eleven_do_not() {
        let center = IVec3::new(0, 30, 0);
        let mut blocks = world(center, true);
        let get = |at: IVec3| blocks.get(&at).copied();
        let any = ring(center)[5].0;
        assert_eq!(find_lit(any, &get), Some(center));
        // Take one eye out.
        let (cell, face) = ring(center)[7];
        blocks.insert(cell, frame(face, false));
        let get = |at: IVec3| blocks.get(&at).copied();
        assert_eq!(find_lit(any, &get), None);
        // A frame turned the wrong way doesn't count, eye or not.
        blocks.insert(cell, frame(face.opposite(), true));
        let get = |at: IVec3| blocks.get(&at).copied();
        assert_eq!(find_lit(any, &get), None);
    }

    #[test]
    fn it_doesnt_matter_which_frame_is_asked_about() {
        let center = IVec3::new(-30, 12, 44);
        let blocks = world(center, true);
        let get = |at: IVec3| blocks.get(&at).copied();
        for (cell, _) in ring(center) {
            assert_eq!(find_lit(cell, &get), Some(center));
        }
        assert_eq!(find_lit(center, &get), None, "the middle is no frame");
    }
}
