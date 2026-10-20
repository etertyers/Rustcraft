//! What a piston can move, following modern Minecraft's
//! `PistonStructureResolver` (without slime or honey): a line of up to 12
//! blocks in front of an extending piston, or the one block a sticky
//! piston pulls back. Blocks that would be crushed (plants, torches, rails,
//! fluids) are destroyed instead, and obsidian, extended pistons and
//! anything that holds items won't move.

use glam::IVec3;

use super::{
    Face,
    block::{BlockState, Shape},
    blocks,
    props::Prop,
};
use crate::config::WORLD_HEIGHT;

/// Most blocks one piston pushes (modern `PistonBaseBlock.MAX_PUSH_DEPTH`).
pub const PUSH_LIMIT: usize = 12;

/// How a block answers being pushed (modern `PushReaction`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reaction {
    Normal,
    /// Breaks and drops as an item.
    Destroy,
    /// Can't be moved at all.
    Block,
}

/// What a piston move comes to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    /// The blocks to move one step along `direction`, nearest the piston
    /// first.
    pub push: Vec<IVec3>,
    /// The blocks in the way that break.
    pub destroy: Vec<IVec3>,
    pub direction: Face,
}

fn step(face: Face) -> IVec3 {
    IVec3::from_array(face.normal())
}

/// How `state` reacts to a piston.
pub fn reaction(state: BlockState) -> Reaction {
    let def = state.def();
    if def.fluid.is_some() {
        return Reaction::Destroy;
    }
    if matches!(
        state.id().name(),
        "obsidian"
            | "crying_obsidian"
            | "enchanting_table"
            | "end_portal_frame"
            | "end_portal"
            | "nether_portal"
            | "ender_chest"
            | "spawner"
            | "piston_head"
            | "moving_piston"
            | "respawn_anchor"
    ) {
        return Reaction::Block;
    }
    let crushed = matches!(
        def.shape,
        Shape::Cross
            | Shape::Crop
            | Shape::Torch
            | Shape::Rail
            | Shape::Lever
            | Shape::Button
            | Shape::PressurePlate
            | Shape::Repeater
            | Shape::RedstoneWire
            | Shape::Vine
            | Shape::LilyPad
            | Shape::Fire
            | Shape::SnowLayer
            | Shape::Cake
            | Shape::Door
            | Shape::Ladder
            | Shape::Bed
            | Shape::Sign
    );
    if crushed || (def.replaceable && !def.solid) {
        Reaction::Destroy
    } else {
        Reaction::Normal
    }
}

/// Whether `state` holds items or other data that can't move.
fn has_block_entity(state: BlockState) -> bool {
    matches!(
        state.def().shape,
        Shape::Chest | Shape::Sign | Shape::Bed | Shape::BrewingStand | Shape::EnchantingTable
    ) || matches!(
        state.id().name(),
        "furnace"
            | "blast_furnace"
            | "smoker"
            | "barrel"
            | "lectern"
            | "dispenser"
            | "dropper"
            | "hopper"
            | "jukebox"
            | "beacon"
            | "spawner"
    )
}

/// Whether a piston can move `state` at `at` toward `move_direction`
/// (modern `PistonBaseBlock.isPushable`). Blocks that break only count
/// when `destroy_blocks` says so.
pub fn pushable(state: BlockState, at: IVec3, move_direction: Face, destroy_blocks: bool) -> bool {
    if !(0..WORLD_HEIGHT as i32).contains(&at.y) {
        return false;
    }
    if state.is_air() {
        return true;
    }
    if (move_direction == Face::Bottom && at.y == 0)
        || (move_direction == Face::Top && at.y == WORLD_HEIGHT as i32 - 1)
    {
        return false;
    }
    if matches!(state.id(), blocks::PISTON | blocks::STICKY_PISTON) {
        if state.bool(Prop::Extended) {
            return false;
        }
    } else if state.def().hardness < 0.0 {
        return false;
    }
    let allowed = match reaction(state) {
        Reaction::Block => false,
        Reaction::Destroy => destroy_blocks,
        Reaction::Normal => true,
    };
    allowed && !has_block_entity(state)
}

/// Works out what a piston at `piston` facing `facing` moves when it
/// `extending` or (if sticky) pulls back. `get` reads blocks; `None` for
/// unloaded ones stops the move. Returns `None` if the move is blocked.
pub fn resolve(
    get: &impl Fn(IVec3) -> Option<BlockState>,
    piston: IVec3,
    facing: Face,
    extending: bool,
) -> Option<Plan> {
    let direction = if extending { facing } else { facing.opposite() };
    let start = if extending {
        piston + step(facing)
    } else {
        piston + step(facing) * 2
    };
    let mut plan = Plan {
        push: Vec::new(),
        destroy: Vec::new(),
        direction,
    };
    let first = get(start)?;
    if !pushable(first, start, direction, false) {
        // A pushable-only-if-destroyed block in the way just breaks; so
        // does anything else a retracting piston has nothing to pull.
        if extending && reaction(first) == Reaction::Destroy {
            plan.destroy.push(start);
            return Some(plan);
        }
        return if extending { None } else { Some(plan) };
    }
    if first.is_air() {
        return Some(plan);
    }
    plan.push.push(start);
    let mut at = start;
    loop {
        at += step(direction);
        if at == piston {
            return None;
        }
        let state = get(at)?;
        if state.is_air() {
            return Some(plan);
        }
        if !pushable(state, at, direction, true) {
            return None;
        }
        if reaction(state) == Reaction::Destroy {
            plan.destroy.push(at);
            return Some(plan);
        }
        if plan.push.len() >= PUSH_LIMIT {
            return None;
        }
        plan.push.push(at);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::block::BlockState;

    fn get_in(cells: &[(IVec3, BlockState)]) -> impl Fn(IVec3) -> Option<BlockState> + '_ {
        move |at| {
            Some(
                cells
                    .iter()
                    .find(|&&(p, _)| p == at)
                    .map_or(BlockState::AIR, |&(_, s)| s),
            )
        }
    }

    fn row(count: i32, state: BlockState) -> Vec<(IVec3, BlockState)> {
        (1..=count).map(|i| (IVec3::new(i, 0, 0), state)).collect()
    }

    #[test]
    fn a_piston_pushes_up_to_twelve_blocks_and_not_thirteen() {
        let stone = BlockState::of(blocks::STONE);
        let twelve = row(12, stone);
        let plan = resolve(&get_in(&twelve), IVec3::ZERO, Face::East, true).unwrap();
        assert_eq!(plan.push.len(), 12);
        assert_eq!(plan.push[0], IVec3::new(1, 0, 0), "nearest first");
        let thirteen = row(13, stone);
        assert_eq!(
            resolve(&get_in(&thirteen), IVec3::ZERO, Face::East, true),
            None
        );
    }

    #[test]
    fn obsidian_and_extended_pistons_block_the_push() {
        let obsidian = row(1, BlockState::of(blocks::OBSIDIAN));
        assert_eq!(
            resolve(&get_in(&obsidian), IVec3::ZERO, Face::East, true),
            None
        );
        let extended = BlockState::of(blocks::PISTON)
            .with_bool(Prop::Extended, true)
            .with_face(Prop::FacingAll, Face::East);
        let cells = row(1, extended);
        assert_eq!(
            resolve(&get_in(&cells), IVec3::ZERO, Face::East, true),
            None
        );
        // An unextended one moves.
        let cells = row(1, BlockState::of(blocks::PISTON));
        assert!(resolve(&get_in(&cells), IVec3::ZERO, Face::East, true).is_some());
    }

    #[test]
    fn plants_and_torches_in_the_way_are_destroyed_and_end_the_line() {
        let stone = BlockState::of(blocks::STONE);
        let mut cells = row(2, stone);
        cells.push((IVec3::new(3, 0, 0), BlockState::of(blocks::TORCH)));
        cells.push((IVec3::new(4, 0, 0), stone));
        let plan = resolve(&get_in(&cells), IVec3::ZERO, Face::East, true).unwrap();
        assert_eq!(plan.push, vec![IVec3::new(1, 0, 0), IVec3::new(2, 0, 0)]);
        assert_eq!(plan.destroy, vec![IVec3::new(3, 0, 0)]);
    }

    #[test]
    fn chests_and_furnaces_cannot_be_pushed() {
        for block in [blocks::CHEST, blocks::FURNACE, blocks::HOPPER] {
            let cells = row(1, BlockState::of(block));
            assert_eq!(
                resolve(&get_in(&cells), IVec3::ZERO, Face::East, true),
                None
            );
        }
    }

    #[test]
    fn a_sticky_piston_pulls_the_block_two_cells_out_toward_itself() {
        let stone = BlockState::of(blocks::STONE);
        let cells = vec![(IVec3::new(2, 0, 0), stone)];
        let plan = resolve(&get_in(&cells), IVec3::ZERO, Face::East, false).unwrap();
        assert_eq!(plan.push, vec![IVec3::new(2, 0, 0)]);
        assert_eq!(plan.direction, Face::West);
        // Nothing there: nothing to pull.
        let plan = resolve(&get_in(&[]), IVec3::ZERO, Face::East, false).unwrap();
        assert!(plan.push.is_empty());
    }

    #[test]
    fn nothing_moves_off_the_top_or_bottom_of_the_world() {
        let stone = BlockState::of(blocks::STONE);
        assert!(!pushable(stone, IVec3::new(0, 0, 0), Face::Bottom, false));
        let top = WORLD_HEIGHT as i32 - 1;
        assert!(!pushable(stone, IVec3::new(0, top, 0), Face::Top, false));
        assert!(pushable(stone, IVec3::new(0, 10, 0), Face::Top, false));
    }
}
