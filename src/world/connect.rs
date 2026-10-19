//! Block states that hang on their neighbors, worked out the way modern
//! Minecraft's `Block.updateShape` does when a block is placed and when
//! something beside it changes: fence, pane and wall connections, stair
//! corners, redstone dust sides, vines hanging from the block above, snowy
//! grass and fence gates set into walls.

use glam::IVec3;

use super::{
    Face,
    block::{BlockState, Shape},
    blocks,
    props::{Prop, stairs_shape, wall_side, wire_side},
};

const SIDES: [Face; 4] = [Face::North, Face::East, Face::South, Face::West];

fn offset(face: Face) -> IVec3 {
    IVec3::from_array(face.normal())
}

/// Whether `state` has a full, solid face to attach to (modern
/// `isFaceSturdy` for a full block), less the blocks fences and walls
/// never join (leaves, pumpkins and melons).
pub fn sturdy(state: BlockState) -> bool {
    let def = state.def();
    let full = def.shape == Shape::Cube || def.is_opaque_cube_state(state);
    full && def.solid
        && !matches!(
            state.id(),
            blocks::OAK_LEAVES
                | blocks::SPRUCE_LEAVES
                | blocks::BIRCH_LEAVES
                | blocks::CARVED_PUMPKIN
                | blocks::JACK_O_LANTERN
                | blocks::MELON
        )
}

/// Whether a fence gate faces across `face` (its gap runs along the
/// line through `face`), so a fence or wall that way joins it.
fn gate_in_line(gate: BlockState, face: Face) -> bool {
    if gate.def().shape != Shape::FenceGate {
        return false;
    }
    let facing = gate.face(Prop::Facing);
    facing.rotate_cw() == face || facing.rotate_ccw() == face
}

/// Wooden fences join wooden fences, and nether brick fences their own.
fn fences_join(a: BlockState, b: BlockState) -> bool {
    let nether = |s: BlockState| s.is(blocks::NETHER_BRICK_FENCE);
    b.def().shape == Shape::Fence && nether(a) == nether(b)
}

fn is_wall(state: BlockState) -> bool {
    state.def().shape == Shape::Wall
}

fn is_stairs(state: BlockState) -> bool {
    state.def().shape == Shape::Stairs
}

/// `state` at `at` reworked from its neighbors, read through `get` (which
/// gives `None` for unloaded cells; those count as air). Blocks that don't
/// hang on their neighbors come back unchanged.
pub fn update_shape(
    state: BlockState,
    at: IVec3,
    get: &impl Fn(IVec3) -> Option<BlockState>,
) -> BlockState {
    let def = state.def();
    let near = |face: Face| get(at + offset(face)).unwrap_or(BlockState::AIR);
    match def.shape {
        Shape::Fence => SIDES.iter().fold(state, |s, &face| {
            let other = near(face);
            let joins = fences_join(state, other) || gate_in_line(other, face) || sturdy(other);
            s.with_bool(Prop::side(face), joins)
        }),
        Shape::Pane => SIDES.iter().fold(state, |s, &face| {
            let other = near(face);
            let joins = other.def().shape == Shape::Pane || is_wall(other) || sturdy(other);
            s.with_bool(Prop::side(face), joins)
        }),
        Shape::Wall => wall(state, &near),
        Shape::Stairs => state.with(Prop::StairsShape, stairs(state, &near)),
        Shape::RedstoneWire => wire(state, at, get),
        Shape::Vine => state.with_bool(Prop::Up, sturdy(near(Face::Top))),
        Shape::FenceGate => {
            let facing = state.face(Prop::Facing);
            let walled = is_wall(near(facing.rotate_cw())) || is_wall(near(facing.rotate_ccw()));
            state.with_bool(Prop::InWall, walled)
        }
        _ if state.has(Prop::Snowy) => {
            let above = near(Face::Top);
            state.with_bool(
                Prop::Snowy,
                above.is(blocks::SNOW) || above.is(blocks::SNOW_BLOCK),
            )
        }
        _ => state,
    }
}

/// A wall's sides (low, or tall under a full block) and whether its post
/// shows: everywhere but the middle of a straight run (modern
/// `WallBlock.updateShape`, simplified).
fn wall(state: BlockState, near: &impl Fn(Face) -> BlockState) -> BlockState {
    let above = near(Face::Top);
    let tall = sturdy(above) || is_wall(above);
    let mut state = state;
    let mut joined = [false; 4];
    for (i, &face) in SIDES.iter().enumerate() {
        let other = near(face);
        joined[i] = is_wall(other)
            || other.def().shape == Shape::Pane
            || gate_in_line(other, face)
            || sturdy(other);
        let side = match (joined[i], tall) {
            (false, _) => wall_side::NONE,
            (true, true) => wall_side::TALL,
            (true, false) => wall_side::LOW,
        };
        state = state.with(Prop::wall_side(face), side);
    }
    let [n, e, s, w] = joined;
    let straight = (n && s && !e && !w) || (e && w && !n && !s);
    let post_above = is_wall(above) && above.bool(Prop::Up);
    state.with_bool(Prop::Up, !straight || post_above)
}

/// A stair's corner shape from the stairs behind and in front of it
/// (modern `StairBlock.getStairsShape`).
fn stairs(state: BlockState, near: &impl Fn(Face) -> BlockState) -> u8 {
    let facing = state.face(Prop::Facing);
    let level = state.get(Prop::Half);
    let same_level = |other: BlockState| is_stairs(other) && other.get(Prop::Half) == level;
    // A neighbor this way doesn't stop the corner unless it's stairs
    // facing and set the same as this one.
    let can_turn = |face: Face| {
        let other = near(face);
        !is_stairs(other) || other.face(Prop::Facing) != facing || other.get(Prop::Half) != level
    };
    let back = near(facing);
    if same_level(back) {
        let turned = back.face(Prop::Facing);
        if turned.is_horizontal()
            && (turned == facing.rotate_cw() || turned == facing.rotate_ccw())
            && can_turn(turned.opposite())
        {
            return if turned == facing.rotate_ccw() {
                stairs_shape::OUTER_LEFT
            } else {
                stairs_shape::OUTER_RIGHT
            };
        }
    }
    let front = near(facing.opposite());
    if same_level(front) {
        let turned = front.face(Prop::Facing);
        if (turned == facing.rotate_cw() || turned == facing.rotate_ccw()) && can_turn(turned) {
            return if turned == facing.rotate_ccw() {
                stairs_shape::INNER_LEFT
            } else {
                stairs_shape::INNER_RIGHT
            };
        }
    }
    stairs_shape::STRAIGHT
}

/// Whether redstone dust joins `other` beside it: more dust, or a part
/// that gives or takes power from that side.
fn powers(other: BlockState, toward: Face) -> bool {
    match other.id() {
        blocks::REDSTONE_WIRE
        | blocks::REDSTONE_TORCH
        | blocks::REDSTONE_WALL_TORCH
        | blocks::LEVER
        | blocks::STONE_BUTTON
        | blocks::OAK_BUTTON
        | blocks::REDSTONE_BLOCK
        | blocks::STONE_PRESSURE_PLATE
        | blocks::OAK_PRESSURE_PLATE
        | blocks::DETECTOR_RAIL => true,
        // A repeater only along its line.
        blocks::REPEATER => {
            let facing = other.face(Prop::Facing);
            facing == toward || facing == toward.opposite()
        }
        _ => false,
    }
}

/// Redstone dust's sides (modern `RedStoneWireBlock.getConnectionState`,
/// simplified): `side` toward dust or a redstone part beside it or dust a
/// step down, `up` toward dust a step up a solid block; lone dust is a
/// cross, and a single connection runs straight through.
fn wire(state: BlockState, at: IVec3, get: &impl Fn(IVec3) -> Option<BlockState>) -> BlockState {
    let read = |p: IVec3| get(p).unwrap_or(BlockState::AIR);
    let above_open = !sturdy(read(at + IVec3::Y));
    let mut sides = [wire_side::NONE; 4];
    for (i, &face) in SIDES.iter().enumerate() {
        let beside = at + offset(face);
        let next = read(beside);
        sides[i] = if powers(next, face) {
            wire_side::SIDE
        } else if sturdy(next) {
            if above_open && read(beside + IVec3::Y).is(blocks::REDSTONE_WIRE) {
                wire_side::UP
            } else {
                wire_side::NONE
            }
        } else if read(beside - IVec3::Y).is(blocks::REDSTONE_WIRE) {
            wire_side::SIDE
        } else {
            wire_side::NONE
        };
    }
    let joined: Vec<usize> = (0..4).filter(|&i| sides[i] != wire_side::NONE).collect();
    match joined.as_slice() {
        [] => sides = [wire_side::SIDE; 4],
        &[only] => sides[(only + 2) % 4] = wire_side::SIDE,
        _ => {}
    }
    SIDES.iter().zip(sides).fold(state, |s, (&face, side)| {
        s.with(Prop::wire_side(face), side)
    })
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use crate::world::props::half;

    /// Works out the block at the origin among `around` (offset, state).
    fn shaped(state: BlockState, around: &[(IVec3, BlockState)]) -> BlockState {
        let map: HashMap<IVec3, BlockState> = around.iter().copied().collect();
        update_shape(state, IVec3::ZERO, &|p| {
            Some(map.get(&p).copied().unwrap_or(BlockState::AIR))
        })
    }

    fn of(id: super::super::block::BlockId) -> BlockState {
        BlockState::of(id)
    }

    #[test]
    fn fences_join_fences_gates_and_solid_blocks_but_not_leaves() {
        let fence = of(blocks::OAK_FENCE);
        let gate = of(blocks::OAK_FENCE_GATE).with_face(Prop::Facing, Face::North);
        let state = shaped(
            fence,
            &[
                (IVec3::X, fence),
                (IVec3::NEG_X, gate),
                (IVec3::Z, of(blocks::STONE)),
                (IVec3::NEG_Z, of(blocks::OAK_LEAVES)),
            ],
        );
        assert!(state.bool(Prop::East) && state.bool(Prop::West) && state.bool(Prop::South));
        assert!(!state.bool(Prop::North), "not leaves");
        // A nether brick fence keeps to its own kind.
        let nether = shaped(of(blocks::NETHER_BRICK_FENCE), &[(IVec3::X, fence)]);
        assert!(!nether.bool(Prop::East));
        // A gate facing east runs north–south, so it doesn't join from the west.
        let side_gate = of(blocks::OAK_FENCE_GATE).with_face(Prop::Facing, Face::East);
        assert!(!shaped(fence, &[(IVec3::NEG_X, side_gate)]).bool(Prop::West));
    }

    #[test]
    fn panes_join_panes_and_solid_blocks() {
        let pane = of(blocks::GLASS_PANE);
        let state = shaped(
            pane,
            &[
                (IVec3::X, of(blocks::IRON_BARS)),
                (IVec3::NEG_X, of(blocks::GLASS)),
            ],
        );
        assert!(state.bool(Prop::East) && state.bool(Prop::West));
        assert!(!state.bool(Prop::North));
    }

    #[test]
    fn stairs_turn_corners_like_minecraft() {
        let north = of(blocks::OAK_STAIRS).with_face(Prop::Facing, Face::North);
        let west = of(blocks::OAK_STAIRS).with_face(Prop::Facing, Face::West);
        let east = of(blocks::OAK_STAIRS).with_face(Prop::Facing, Face::East);
        // A west-facing stair behind (north of) a north-facing one: an outer corner.
        assert_eq!(
            shaped(north, &[(IVec3::NEG_Z, west)]).get(Prop::StairsShape),
            stairs_shape::OUTER_LEFT
        );
        assert_eq!(
            shaped(north, &[(IVec3::NEG_Z, east)]).get(Prop::StairsShape),
            stairs_shape::OUTER_RIGHT
        );
        // In front (south): an inner corner.
        assert_eq!(
            shaped(north, &[(IVec3::Z, west)]).get(Prop::StairsShape),
            stairs_shape::INNER_LEFT
        );
        // Stairs of the other half don't join.
        let upside_down = west.with(Prop::Half, half::TOP);
        assert_eq!(
            shaped(north, &[(IVec3::Z, upside_down)]).get(Prop::StairsShape),
            stairs_shape::STRAIGHT
        );
        assert_eq!(
            shaped(north, &[]).get(Prop::StairsShape),
            stairs_shape::STRAIGHT
        );
    }

    #[test]
    fn redstone_dust_joins_dust_and_parts_and_lone_dust_is_a_cross() {
        let wire = of(blocks::REDSTONE_WIRE);
        let lone = shaped(wire, &[]);
        assert!(
            SIDES
                .iter()
                .all(|&f| lone.get(Prop::wire_side(f)) == wire_side::SIDE)
        );
        // One neighbor: a straight line through.
        let line = shaped(wire, &[(IVec3::X, wire)]);
        assert_eq!(line.get(Prop::WireEast), wire_side::SIDE);
        assert_eq!(line.get(Prop::WireWest), wire_side::SIDE);
        assert_eq!(line.get(Prop::WireNorth), wire_side::NONE);
        // Up a step onto a block.
        let step = shaped(
            wire,
            &[
                (IVec3::NEG_Z, of(blocks::STONE)),
                (IVec3::new(0, 1, -1), wire),
                (IVec3::X, of(blocks::REDSTONE_TORCH)),
            ],
        );
        assert_eq!(step.get(Prop::WireNorth), wire_side::UP);
        assert_eq!(step.get(Prop::WireEast), wire_side::SIDE);
    }

    #[test]
    fn grass_goes_snowy_under_snow_and_vines_hang_from_blocks_above() {
        let grass = of(blocks::GRASS_BLOCK);
        assert!(shaped(grass, &[(IVec3::Y, of(blocks::SNOW))]).bool(Prop::Snowy));
        assert!(!shaped(grass, &[(IVec3::Y, of(blocks::STONE))]).bool(Prop::Snowy));
        let vine = of(blocks::VINE).with_bool(Prop::North, true);
        assert!(shaped(vine, &[(IVec3::Y, of(blocks::STONE))]).bool(Prop::Up));
        assert!(!shaped(vine, &[]).bool(Prop::Up));
    }

    #[test]
    fn other_blocks_are_left_alone() {
        let stone = of(blocks::STONE);
        assert_eq!(shaped(stone, &[(IVec3::X, of(blocks::OAK_FENCE))]), stone);
    }
}
