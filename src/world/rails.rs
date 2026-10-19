//! Joining rails up, after modern Minecraft's `RailState`: a rail placed
//! beside another turns to meet it, up a slope if the other is a step
//! higher and round a corner if two others are at right angles, and the
//! rails it meets turn to meet it back. Powered, detector and activator
//! rails only run straight.

use glam::IVec3;

use super::{
    Face,
    block::BlockState,
    blocks,
    props::{Prop, rail_shape},
};

/// The four directions a rail leaves its cell, in the order ties are
/// settled.
const SIDES: [Face; 4] = [Face::North, Face::South, Face::West, Face::East];

/// Whether `state` is any kind of rail.
pub fn is_rail(state: BlockState) -> bool {
    matches!(
        state.id(),
        blocks::RAIL | blocks::POWERED_RAIL | blocks::DETECTOR_RAIL | blocks::ACTIVATOR_RAIL
    )
}

/// The rail's shape, 0–9 (`props::rail_shape`).
pub fn shape_of(state: BlockState) -> u8 {
    if state.has(Prop::RailShape) {
        state.get(Prop::RailShape)
    } else {
        state.get(Prop::StraightRailShape)
    }
}

fn with_shape(state: BlockState, shape: u8) -> BlockState {
    if state.has(Prop::RailShape) {
        state.with(Prop::RailShape, shape)
    } else {
        state.with(Prop::StraightRailShape, shape)
    }
}

/// Whether the rail can bend (only plain rails do).
fn curves(state: BlockState) -> bool {
    state.has(Prop::RailShape)
}

fn step(face: Face) -> IVec3 {
    IVec3::from_array(face.normal())
}

/// The two ends of a rail shape as (direction, rises): which sides it
/// leaves by and whether that end is a block higher.
pub fn ends(shape: u8) -> [(Face, bool); 2] {
    use Face::*;
    match shape {
        rail_shape::NORTH_SOUTH => [(North, false), (South, false)],
        rail_shape::EAST_WEST => [(West, false), (East, false)],
        rail_shape::ASCENDING_EAST => [(West, false), (East, true)],
        rail_shape::ASCENDING_WEST => [(West, true), (East, false)],
        rail_shape::ASCENDING_NORTH => [(North, true), (South, false)],
        rail_shape::ASCENDING_SOUTH => [(North, false), (South, true)],
        rail_shape::SOUTH_EAST => [(South, false), (East, false)],
        rail_shape::SOUTH_WEST => [(South, false), (West, false)],
        rail_shape::NORTH_WEST => [(North, false), (West, false)],
        _ => [(North, false), (East, false)],
    }
}

/// The shape running between two sides, flat.
fn flat_between(a: Face, b: Face, curved: bool) -> Option<u8> {
    use Face::*;
    let has = |x: Face, y: Face| (a == x && b == y) || (a == y && b == x);
    if has(North, South) {
        Some(rail_shape::NORTH_SOUTH)
    } else if has(East, West) {
        Some(rail_shape::EAST_WEST)
    } else if !curved {
        None
    } else if has(South, East) {
        Some(rail_shape::SOUTH_EAST)
    } else if has(South, West) {
        Some(rail_shape::SOUTH_WEST)
    } else if has(North, West) {
        Some(rail_shape::NORTH_WEST)
    } else if has(North, East) {
        Some(rail_shape::NORTH_EAST)
    } else {
        None
    }
}

/// Where a rail beside `at` toward `side` is, if there is one that could
/// join `at`: level, a step down or a step up.
fn neighbor(
    get: &impl Fn(IVec3) -> Option<BlockState>,
    at: IVec3,
    side: Face,
) -> Option<(IVec3, BlockState)> {
    let beside = at + step(side);
    [beside, beside - IVec3::Y, beside + IVec3::Y]
        .into_iter()
        .filter_map(|p| Some((p, get(p)?)))
        .find(|&(p, state)| {
            is_rail(state) && {
                // It must have an end free or already pointing back.
                let back = side.opposite();
                let points_back = ends(shape_of(state)).iter().any(|&(face, _)| face == back);
                points_back || free_ends(get, p, state) > 0
            }
        })
}

/// How many of a rail's ends have no rail joined to them.
fn free_ends(get: &impl Fn(IVec3) -> Option<BlockState>, at: IVec3, state: BlockState) -> usize {
    ends(shape_of(state))
        .iter()
        .filter(|&&(face, up)| {
            let beside = at + step(face) + if up { IVec3::Y } else { IVec3::ZERO };
            [beside, beside - IVec3::Y]
                .into_iter()
                .all(|p| !get(p).is_some_and(is_rail))
        })
        .count()
}

/// The shape the rail `state` at `at` takes to meet the rails around it.
/// `None` for a rail with nothing to meet (it keeps its shape).
pub fn shape_toward(
    get: &impl Fn(IVec3) -> Option<BlockState>,
    at: IVec3,
    state: BlockState,
) -> Option<u8> {
    let meets: Vec<(Face, IVec3)> = SIDES
        .iter()
        .filter_map(|&side| neighbor(get, at, side).map(|(p, _)| (side, p)))
        .collect();
    let curved = curves(state);
    // Two to meet: through, or round a corner. A rail meeting three
    // prefers a straight run.
    let pair = meets.iter().enumerate().find_map(|(i, &(a, _))| {
        meets[i + 1..]
            .iter()
            .find_map(|&(b, _)| flat_between(a, b, curved).map(|shape| (a, b, shape)))
    });
    let (shape, toward) = match (pair, meets.first()) {
        (Some((a, b, shape)), _) => (shape, [a, b].to_vec()),
        (None, Some(&(side, _))) => {
            let straight = if matches!(side, Face::North | Face::South) {
                rail_shape::NORTH_SOUTH
            } else {
                rail_shape::EAST_WEST
            };
            (straight, vec![side])
        }
        _ => return None,
    };
    // A straight run with a rail a step up at one end climbs toward it.
    if shape <= rail_shape::EAST_WEST {
        for side in toward {
            let beside = at + step(side);
            if get(beside + IVec3::Y).is_some_and(is_rail) {
                return Some(match side {
                    Face::North => rail_shape::ASCENDING_NORTH,
                    Face::South => rail_shape::ASCENDING_SOUTH,
                    Face::East => rail_shape::ASCENDING_EAST,
                    _ => rail_shape::ASCENDING_WEST,
                });
            }
        }
    }
    Some(shape)
}

/// Placing `state` at `at`: its own shape, and the rails it joins turning
/// to meet it. Returns every changed block.
pub fn place(
    get: &impl Fn(IVec3) -> Option<BlockState>,
    at: IVec3,
    state: BlockState,
) -> Vec<(IVec3, BlockState)> {
    let with_us = |p: IVec3| if p == at { Some(state) } else { get(p) };
    let own = shape_toward(&with_us, at, state)
        .map(|shape| with_shape(state, shape))
        .unwrap_or(state);
    let mut edits = vec![(at, own)];
    let with_us = |p: IVec3| if p == at { Some(own) } else { get(p) };
    for side in SIDES {
        let Some((p, other)) = neighbor(get, at, side) else {
            continue;
        };
        // Only rails with a free end turn to meet it.
        if free_ends(get, p, other) == 0 && !joined_toward(other, p, at) {
            continue;
        }
        if let Some(shape) = shape_toward(&with_us, p, other)
            && shape != shape_of(other)
        {
            edits.push((p, with_shape(other, shape)));
        }
    }
    edits
}

/// Whether the rail `other` at `p` already runs toward `at`.
fn joined_toward(other: BlockState, p: IVec3, at: IVec3) -> bool {
    ends(shape_of(other)).iter().any(|&(face, up)| {
        let beside = p + step(face);
        beside == at || beside + IVec3::Y == at && up || beside - IVec3::Y == at
    })
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    fn rail(shape: u8) -> BlockState {
        BlockState::of(blocks::RAIL).with(Prop::RailShape, shape)
    }

    fn world(rails: &[(IVec3, BlockState)]) -> impl Fn(IVec3) -> Option<BlockState> + use<> {
        let map: HashMap<IVec3, BlockState> = rails.iter().copied().collect();
        move |p| Some(map.get(&p).copied().unwrap_or(BlockState::AIR))
    }

    #[test]
    fn a_lone_rail_keeps_its_shape_and_one_neighbor_straightens_it() {
        let at = IVec3::new(0, 64, 0);
        let get = world(&[]);
        assert_eq!(
            place(&get, at, rail(rail_shape::EAST_WEST)),
            vec![(at, rail(rail_shape::EAST_WEST))]
        );
        let get = world(&[(at + IVec3::Z, rail(rail_shape::NORTH_SOUTH))]);
        let edits = place(&get, at, rail(rail_shape::EAST_WEST));
        assert_eq!(edits[0].1, rail(rail_shape::NORTH_SOUTH));
    }

    #[test]
    fn two_rails_at_right_angles_make_a_corner() {
        let at = IVec3::new(0, 64, 0);
        let get = world(&[
            (at + IVec3::Z, rail(rail_shape::NORTH_SOUTH)),
            (at + IVec3::X, rail(rail_shape::EAST_WEST)),
        ]);
        let edits = place(&get, at, rail(rail_shape::NORTH_SOUTH));
        assert_eq!(edits[0].1, rail(rail_shape::SOUTH_EAST));
    }

    #[test]
    fn a_rail_beside_one_a_step_up_climbs_toward_it() {
        let at = IVec3::new(0, 64, 0);
        let get = world(&[(at + IVec3::X + IVec3::Y, rail(rail_shape::EAST_WEST))]);
        let edits = place(&get, at, rail(rail_shape::EAST_WEST));
        assert_eq!(edits[0].1, rail(rail_shape::ASCENDING_EAST));
    }

    #[test]
    fn powered_rails_never_curve() {
        let at = IVec3::new(0, 64, 0);
        let powered = BlockState::of(blocks::POWERED_RAIL);
        let get = world(&[
            (at + IVec3::Z, rail(rail_shape::NORTH_SOUTH)),
            (at + IVec3::X, rail(rail_shape::EAST_WEST)),
        ]);
        let edits = place(&get, at, powered);
        let shape = shape_of(edits[0].1);
        assert!(shape <= rail_shape::ASCENDING_SOUTH, "{shape}");
    }

    #[test]
    fn the_rail_met_turns_to_meet_the_new_one() {
        // A lone east-west rail, then a rail placed north of it: the old
        // one, with a free end, bends toward the new one only if it must;
        // a rail with two ends already used stays.
        let a = IVec3::new(0, 64, 0);
        let get = world(&[(a, rail(rail_shape::EAST_WEST))]);
        let edits = place(&get, a + IVec3::NEG_Z, rail(rail_shape::EAST_WEST));
        assert_eq!(edits[0].1, rail(rail_shape::NORTH_SOUTH));
        assert!(
            edits
                .iter()
                .any(|&(p, s)| p == a && s == rail(rail_shape::NORTH_SOUTH))
        );
        let b = IVec3::new(0, 64, 0);
        let get = world(&[
            (b, rail(rail_shape::EAST_WEST)),
            (b + IVec3::X, rail(rail_shape::EAST_WEST)),
            (b + IVec3::NEG_X, rail(rail_shape::EAST_WEST)),
        ]);
        let edits = place(&get, b + IVec3::NEG_Z, rail(rail_shape::EAST_WEST));
        assert!(!edits.iter().any(|&(p, _)| p == b), "a joined rail stays");
    }
}
