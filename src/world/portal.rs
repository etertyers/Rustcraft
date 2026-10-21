//! Nether portal shapes (modern `PortalShape`): finding an obsidian frame
//! around a cell, and the portal a frame holds.
//!
//! A frame is a rectangle of obsidian around an empty inside 2 to 21 blocks
//! wide and 3 to 21 tall, standing along x or z. Its corners don't matter.
//! The inside may hold air, fire or portal blocks.

use glam::IVec3;

use super::{
    block::BlockState,
    blocks,
    props::{Prop, axis},
};

/// Smallest and largest inside of a frame.
pub const MIN_WIDTH: i32 = 2;
pub const MAX_WIDTH: i32 = 21;
pub const MIN_HEIGHT: i32 = 3;
pub const MAX_HEIGHT: i32 = 21;

/// The inside of a frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Shape {
    /// `axis::X` or `axis::Z`: the way it's wide.
    pub axis: u8,
    /// The lowest inside cell on the frame's negative side.
    pub corner: IVec3,
    pub width: i32,
    pub height: i32,
}

impl Shape {
    /// One step along the frame's width.
    pub fn along(&self) -> IVec3 {
        along(self.axis)
    }

    /// Every inside cell, bottom row first.
    pub fn cells(&self) -> impl Iterator<Item = IVec3> + '_ {
        (0..self.height).flat_map(move |y| {
            (0..self.width).map(move |i| self.corner + self.along() * i + IVec3::Y * y)
        })
    }

    /// The portal block that fills it.
    pub fn portal(&self) -> BlockState {
        BlockState::of(blocks::NETHER_PORTAL).with(Prop::HorizontalAxis, horizontal(self.axis))
    }

    /// Where a player coming out of it stands: the middle of its bottom
    /// row.
    pub fn exit(&self) -> glam::DVec3 {
        let middle = self.along().as_dvec3() * (self.width as f64 / 2.0);
        let across = (IVec3::ONE - self.along() - IVec3::Y).as_dvec3() * 0.5;
        self.corner.as_dvec3() + middle + across
    }
}

/// The `HorizontalAxis` value for an axis (x is 0, z is 1).
fn horizontal(axis: u8) -> u8 {
    u8::from(axis == axis::Z)
}

/// The axis of a portal block.
pub fn axis_of(portal: BlockState) -> u8 {
    if portal.get(Prop::HorizontalAxis) == 1 {
        axis::Z
    } else {
        axis::X
    }
}

fn along(axis: u8) -> IVec3 {
    if axis == axis::Z { IVec3::Z } else { IVec3::X }
}

fn is_frame(state: Option<BlockState>) -> bool {
    state.is_some_and(|s| s.is(blocks::OBSIDIAN))
}

/// Air, fire or portal: what a frame's inside may hold.
fn is_empty(state: Option<BlockState>) -> bool {
    state.is_some_and(|s| {
        s.is_air() || s.is(blocks::FIRE) || s.is(blocks::SOUL_FIRE) || s.is(blocks::NETHER_PORTAL)
    })
}

/// The frame around `at` standing along `axis`, if there's a whole one.
pub fn find(at: IVec3, axis: u8, get: &impl Fn(IVec3) -> Option<BlockState>) -> Option<Shape> {
    if !is_empty(get(at)) {
        return None;
    }
    let step = along(axis);
    // Down to the bottom row, then along it to the negative side.
    let mut bottom = at;
    let mut fell = 0;
    while fell < MAX_HEIGHT && is_empty(get(bottom - IVec3::Y)) {
        bottom -= IVec3::Y;
        fell += 1;
    }
    let left = distance_to_side(bottom, -step, get)? - 1;
    let corner = bottom - step * left;
    let width = distance_to_side(corner, step, get)?;
    if !(MIN_WIDTH..=MAX_WIDTH).contains(&width) {
        return None;
    }
    let height = height(corner, step, width, get)?;
    Some(Shape {
        axis,
        corner,
        width,
        height,
    })
}

/// Either way round: x first, as modern does.
pub fn find_any(at: IVec3, get: &impl Fn(IVec3) -> Option<BlockState>) -> Option<Shape> {
    find(at, axis::X, get).or_else(|| find(at, axis::Z, get))
}

/// Steps from `from` along `dir` over empty cells with obsidian under
/// them, up to the obsidian side: modern `getDistanceUntilEdgeAboveFrame`.
fn distance_to_side(
    from: IVec3,
    dir: IVec3,
    get: &impl Fn(IVec3) -> Option<BlockState>,
) -> Option<i32> {
    for i in 0..=MAX_WIDTH {
        let cell = from + dir * i;
        let state = get(cell);
        if !is_empty(state) {
            return is_frame(state).then_some(i);
        }
        if !is_frame(get(cell - IVec3::Y)) {
            return None;
        }
    }
    None
}

/// How tall the inside is: rows of empty cells with obsidian at both
/// ends, under a whole row of obsidian.
fn height(
    corner: IVec3,
    step: IVec3,
    width: i32,
    get: &impl Fn(IVec3) -> Option<BlockState>,
) -> Option<i32> {
    let mut height = 0;
    'rows: while height <= MAX_HEIGHT {
        let row = corner + IVec3::Y * height;
        if !is_frame(get(row - step)) || !is_frame(get(row + step * width)) {
            break;
        }
        for i in 0..width {
            if !is_empty(get(row + step * i)) {
                break 'rows;
            }
        }
        height += 1;
    }
    let top = corner + IVec3::Y * height;
    let roofed = (0..width).all(|i| is_frame(get(top + step * i)));
    ((MIN_HEIGHT..=MAX_HEIGHT).contains(&height) && roofed).then_some(height)
}

/// Whether the portal block at `at` still has what it needs beside it in
/// its own plane: portal or obsidian along its axis and above and below
/// (modern `NetherPortalBlock.updateShape`, cell by cell).
pub fn holds(at: IVec3, portal: BlockState, get: &impl Fn(IVec3) -> Option<BlockState>) -> bool {
    let step = along(axis_of(portal));
    [step, -step, IVec3::Y, IVec3::NEG_Y]
        .iter()
        .all(|&d| get(at + d).is_none_or(|s| s.is(blocks::NETHER_PORTAL) || s.is(blocks::OBSIDIAN)))
}

/// The obsidian and portal blocks of a new 2 × 3 portal whose inside
/// starts at `corner` along `axis` (modern `PortalForcer.createPortal`).
pub fn build(corner: IVec3, axis: u8) -> Vec<(IVec3, BlockState)> {
    let shape = Shape {
        axis,
        corner,
        width: MIN_WIDTH,
        height: MIN_HEIGHT,
    };
    let step = shape.along();
    let obsidian = BlockState::of(blocks::OBSIDIAN);
    let mut blocks = Vec::new();
    for y in -1..=MIN_HEIGHT {
        for i in -1..=MIN_WIDTH {
            let cell = corner + step * i + IVec3::Y * y;
            let edge = i == -1 || i == MIN_WIDTH || y == -1 || y == MIN_HEIGHT;
            blocks.push((cell, if edge { obsidian } else { shape.portal() }));
        }
    }
    blocks
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// A frame of obsidian with an inside `width` × `height` from `corner`.
    fn frame(corner: IVec3, step: IVec3, width: i32, height: i32) -> HashMap<IVec3, BlockState> {
        let mut world = HashMap::new();
        for y in -1..=height {
            for i in -1..=width {
                let edge = i == -1 || i == width || y == -1 || y == height;
                let corner_block = (i == -1 || i == width) && (y == -1 || y == height);
                if edge && !corner_block {
                    world.insert(
                        corner + step * i + IVec3::Y * y,
                        BlockState::of(blocks::OBSIDIAN),
                    );
                }
            }
        }
        world
    }

    fn getter(world: &HashMap<IVec3, BlockState>) -> impl Fn(IVec3) -> Option<BlockState> + '_ {
        |p| Some(world.get(&p).copied().unwrap_or(BlockState::AIR))
    }

    #[test]
    fn a_frame_of_any_valid_size_is_found_from_any_inside_cell() {
        for (width, height) in [(2, 3), (4, 5), (21, 21), (3, 10)] {
            for (axis_value, step) in [(axis::X, IVec3::X), (axis::Z, IVec3::Z)] {
                let corner = IVec3::new(10, 64, -5);
                let world = frame(corner, step, width, height);
                let get = getter(&world);
                let inside = corner + step * (width - 1) + IVec3::Y * (height - 1);
                let shape = find_any(inside, &get).expect("a frame");
                assert_eq!(shape.axis, axis_value);
                assert_eq!(
                    (shape.corner, shape.width, shape.height),
                    (corner, width, height)
                );
                assert_eq!(shape.cells().count() as i32, width * height);
            }
        }
    }

    #[test]
    fn too_small_too_big_or_broken_frames_hold_nothing() {
        let corner = IVec3::new(0, 64, 0);
        for (width, height) in [(1, 3), (2, 2), (22, 3), (2, 22)] {
            let world = frame(corner, IVec3::X, width, height);
            assert!(
                find_any(corner, &getter(&world)).is_none(),
                "{width}×{height}"
            );
        }
        let mut world = frame(corner, IVec3::X, 2, 3);
        world.remove(&(corner + IVec3::new(-1, 1, 0)));
        assert!(
            find_any(corner, &getter(&world)).is_none(),
            "a gap in the side"
        );
        let mut world = frame(corner, IVec3::X, 2, 3);
        world.insert(corner + IVec3::Y, BlockState::of(blocks::STONE));
        assert!(
            find_any(corner, &getter(&world)).is_none(),
            "something inside"
        );
        // Fire inside is fine (it's what lights it).
        let mut world = frame(corner, IVec3::X, 2, 3);
        world.insert(corner, BlockState::of(blocks::FIRE));
        assert!(find_any(corner + IVec3::X, &getter(&world)).is_some());
    }

    #[test]
    fn a_built_portal_is_a_whole_frame_and_breaks_without_it() {
        let corner = IVec3::new(5, 70, 5);
        let world: HashMap<_, _> = build(corner, axis::Z).into_iter().collect();
        let get = getter(&world);
        let shape = find_any(corner + IVec3::Y, &get).unwrap();
        assert_eq!((shape.axis, shape.width, shape.height), (axis::Z, 2, 3));
        let portal = world[&corner];
        assert!(portal.is(blocks::NETHER_PORTAL));
        assert!(holds(corner, portal, &get));
        let mut broken = world.clone();
        broken.insert(corner - IVec3::Y, BlockState::AIR);
        assert!(!holds(corner, portal, &getter(&broken)));
        // Standing in the middle of the bottom row.
        let exit = shape.exit();
        assert_eq!(exit, glam::DVec3::new(5.5, 70.0, 6.0));
    }
}
