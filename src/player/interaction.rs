//! Breaking and placing blocks: which cells change, following 1.0.0's rules.

use glam::IVec3;

use super::{collision::Aabb, raycast::Hit};
use crate::{
    config::WORLD_HEIGHT,
    world::{
        Face, World,
        block::{BlockDef, BlockRegistry, BlockState, Placement},
    },
};

/// Ticks between breaks while the button is held (1.0.0's creative `blockHitDelay`).
pub const BREAK_REPEAT_TICKS: u8 = 5;
/// Ticks between placements while the button is held (1.0.0's `rightClickDelayTimer`).
pub const PLACE_REPEAT_TICKS: u8 = 4;

/// One block change.
pub type Edit = (IVec3, BlockState);

fn def_at<'a>(world: &World, blocks: &'a BlockRegistry, at: IVec3) -> Option<&'a BlockDef> {
    blocks.get(world.block(at.x, at.y, at.z)?.id())
}

/// Breaks the block at `at` (instantly, until mining exists in M7), plus
/// anything it held up: flowers or a sugar cane stack on top, torches on
/// its sides. Unbreakable blocks (bedrock) give no edits.
pub fn break_block(world: &World, blocks: &BlockRegistry, at: IVec3) -> Vec<Edit> {
    let Some(def) = def_at(world, blocks, at) else {
        return Vec::new();
    };
    if def.hardness < 0.0 || !def.is_targetable() {
        return Vec::new();
    }
    let mut edits = vec![(at, BlockState::AIR)];
    let mut next = 0;
    while let Some(&(removed, _)) = edits.get(next) {
        next += 1;
        for face in Face::ALL {
            let neighbor = removed + IVec3::from_array(face.normal());
            let Some(state) = world.block(neighbor.x, neighbor.y, neighbor.z) else {
                continue;
            };
            let held = blocks
                .get(state.id())
                .and_then(|def| def.support(state.data()))
                .is_some_and(|offset| neighbor + offset == removed);
            if held && !edits.iter().any(|&(p, _)| p == neighbor) {
                edits.push((neighbor, BlockState::AIR));
            }
        }
    }
    edits
}

/// Where `block` goes when placed against `hit`, or `None` if it can't be:
/// the cell must be in the world, loaded and replaceable, a solid block
/// can't overlap the player, and blocks that need ground need it below.
pub fn place_block(
    world: &World,
    blocks: &BlockRegistry,
    hit: &Hit,
    block: BlockState,
    player: &Aabb,
) -> Option<Edit> {
    let clicked = def_at(world, blocks, hit.block)?;
    // Clicking tall grass or snow replaces it instead of building beside it.
    let at = if clicked.replaceable {
        hit.block
    } else {
        hit.adjacent()?
    };
    if !(0..WORLD_HEIGHT as i32).contains(&at.y) {
        return None;
    }
    if !def_at(world, blocks, at)?.replaceable {
        return None;
    }
    let new = blocks.get(block.id())?;
    if new.solid && Aabb::block(at.x, at.y, at.z).intersects(player) {
        return None;
    }
    let data = match new.placement {
        // Turned toward the player by `facing_data` afterwards.
        Placement::Plain | Placement::Facing => block.data(),
        Placement::Torch => match hit.face? {
            Face::Top => 5,
            Face::East => 1,
            Face::West => 2,
            Face::South => 3,
            Face::North => 4,
            Face::Bottom => return None,
        },
    };
    if let Some(offset) = new.support(data) {
        let support = def_at(world, blocks, at + offset)?;
        // Torches need a full solid block; plants just need ground.
        let holds = match new.placement {
            Placement::Torch => support.is_opaque_cube(),
            Placement::Plain | Placement::Facing => !support.replaceable,
        };
        if !holds {
            return None;
        }
    }
    Some((at, BlockState::new(block.id(), data)))
}

/// The data value turning a `Placement::Facing` block's front toward a
/// player looking along `yaw` (radians, 0 north, turning east).
pub fn facing_data(yaw: f32) -> u8 {
    let quarter = (yaw / std::f32::consts::FRAC_PI_2).round() as i32;
    // Looking north, east, south, west puts the front south, west, north, east.
    [3, 4, 2, 5][quarter.rem_euclid(4) as usize]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        player::physics::hitbox,
        world::{
            Face,
            chunk::{Chunk, ChunkPos},
        },
    };
    use glam::DVec3;

    const STONE: BlockState = BlockState::new(1, 0);
    const BEDROCK: BlockState = BlockState::new(7, 0);
    const REEDS: BlockState = BlockState::new(83, 0);
    const TALL_GRASS: BlockState = BlockState::new(31, 1);
    const WATER: BlockState = BlockState::new(9, 0);
    const DANDELION: BlockState = BlockState::new(37, 0);

    fn blocks() -> BlockRegistry {
        BlockRegistry::load(&crate::assets::dir().join("data/blocks.ron")).unwrap()
    }

    /// One loaded chunk at the origin with a stone floor at y 63.
    fn world_with(extra: &[(IVec3, BlockState)]) -> World {
        let mut chunk = Chunk::new(ChunkPos::new(0, 0));
        for z in 0..16 {
            for x in 0..16 {
                chunk.set(x, 63, z, STONE);
            }
        }
        for &(at, block) in extra {
            chunk.set(at.x as usize, at.y as usize, at.z as usize, block);
        }
        let mut world = World::default();
        world.insert(chunk);
        world
    }

    fn hit(block: IVec3, face: Face) -> Hit {
        Hit {
            block,
            face: Some(face),
        }
    }

    /// A player standing well away from the test cells.
    fn far_player() -> Aabb {
        hitbox(DVec3::new(12.5, 64.0, 12.5))
    }

    #[test]
    fn facing_blocks_put_their_front_toward_the_player() {
        use std::f32::consts::{FRAC_PI_2, PI, TAU};
        assert_eq!(facing_data(0.0), 3, "looking north, the front faces south");
        assert_eq!(facing_data(FRAC_PI_2), 4);
        assert_eq!(facing_data(PI), 2);
        assert_eq!(facing_data(-FRAC_PI_2), 5);
        assert_eq!(facing_data(TAU - 0.2), 3);
    }

    #[test]
    fn breaking_removes_the_block_and_plants_on_it() {
        let world = world_with(&[
            (IVec3::new(2, 64, 2), REEDS),
            (IVec3::new(2, 65, 2), REEDS),
            (IVec3::new(2, 66, 2), REEDS),
        ]);
        let edits = break_block(&world, &blocks(), IVec3::new(2, 63, 2));
        let cells: Vec<i32> = edits.iter().map(|(at, _)| at.y).collect();
        assert_eq!(cells, [63, 64, 65, 66]);
        assert!(edits.iter().all(|(_, block)| block.is_air()));
    }

    #[test]
    fn bedrock_air_and_unloaded_blocks_cannot_be_broken() {
        let world = world_with(&[(IVec3::new(1, 0, 1), BEDROCK)]);
        let blocks = blocks();
        assert!(break_block(&world, &blocks, IVec3::new(1, 0, 1)).is_empty());
        assert!(break_block(&world, &blocks, IVec3::new(1, 70, 1)).is_empty());
        assert!(break_block(&world, &blocks, IVec3::new(40, 63, 1)).is_empty());
    }

    #[test]
    fn places_against_the_clicked_face() {
        let world = world_with(&[]);
        let edit = place_block(
            &world,
            &blocks(),
            &hit(IVec3::new(3, 63, 3), Face::Top),
            STONE,
            &far_player(),
        );
        assert_eq!(edit, Some((IVec3::new(3, 64, 3), STONE)));
    }

    #[test]
    fn cannot_place_inside_the_player() {
        let world = world_with(&[]);
        let player = hitbox(DVec3::new(3.5, 64.0, 3.5));
        let below_feet = hit(IVec3::new(3, 63, 3), Face::Top);
        assert_eq!(
            place_block(&world, &blocks(), &below_feet, STONE, &player),
            None
        );
        // Non-solid blocks may overlap the player.
        let world = world_with(&[]);
        let edit = place_block(&world, &blocks(), &below_feet, DANDELION, &player);
        assert_eq!(edit, Some((IVec3::new(3, 64, 3), DANDELION)));
        // Standing next to the cell is fine.
        let beside = hitbox(DVec3::new(4.3, 64.0, 3.5));
        assert!(place_block(&world, &blocks(), &below_feet, STONE, &beside).is_some());
    }

    #[test]
    fn clicking_tall_grass_replaces_it_and_water_can_be_built_into() {
        let world = world_with(&[
            (IVec3::new(5, 64, 5), TALL_GRASS),
            (IVec3::new(6, 64, 5), WATER),
            (IVec3::new(7, 64, 5), STONE),
        ]);
        let blocks = blocks();
        let player = far_player();
        let edit = place_block(
            &world,
            &blocks,
            &hit(IVec3::new(5, 64, 5), Face::Top),
            STONE,
            &player,
        );
        assert_eq!(edit, Some((IVec3::new(5, 64, 5), STONE)));
        let edit = place_block(
            &world,
            &blocks,
            &hit(IVec3::new(7, 64, 5), Face::West),
            STONE,
            &player,
        );
        assert_eq!(edit, Some((IVec3::new(6, 64, 5), STONE)));
    }

    #[test]
    fn cannot_place_into_solid_blocks_or_outside_the_world() {
        let world = world_with(&[(IVec3::new(3, 64, 3), STONE)]);
        let blocks = blocks();
        let player = far_player();
        // The cell beside the clicked face is already stone.
        let into_stone = hit(IVec3::new(3, 63, 3), Face::Top);
        assert_eq!(
            place_block(&world, &blocks, &into_stone, STONE, &player),
            None
        );
        let world = world_with(&[(IVec3::new(3, 127, 3), STONE)]);
        let above_top = hit(IVec3::new(3, 127, 3), Face::Top);
        assert_eq!(
            place_block(&world, &blocks, &above_top, STONE, &player),
            None
        );
        // Started inside a block: no face to place against.
        let inside = Hit {
            block: IVec3::new(3, 63, 3),
            face: None,
        };
        assert_eq!(place_block(&world, &blocks, &inside, STONE, &player), None);
    }

    #[test]
    fn plants_need_ground() {
        let world = world_with(&[(IVec3::new(3, 65, 3), STONE)]);
        let blocks = blocks();
        let player = far_player();
        let on_floor = hit(IVec3::new(3, 63, 3), Face::Top);
        assert!(place_block(&world, &blocks, &on_floor, DANDELION, &player).is_some());
        // Beside a floating block, over air.
        let on_side = hit(IVec3::new(3, 65, 3), Face::North);
        assert!(place_block(&world, &blocks, &on_side, STONE, &player).is_some());
        assert_eq!(
            place_block(&world, &blocks, &on_side, DANDELION, &player),
            None
        );
    }

    const TORCH: BlockState = BlockState::new(50, 0);

    #[test]
    fn torches_attach_to_floors_and_walls_like_1_0_0() {
        let world = world_with(&[(IVec3::new(3, 64, 3), STONE)]);
        let blocks = blocks();
        let player = far_player();
        let place = |face| {
            place_block(
                &world,
                &blocks,
                &hit(IVec3::new(3, 64, 3), face),
                TORCH,
                &player,
            )
        };
        assert_eq!(
            place(Face::Top),
            Some((IVec3::new(3, 65, 3), BlockState::new(50, 5)))
        );
        assert_eq!(
            place(Face::East),
            Some((IVec3::new(4, 64, 3), BlockState::new(50, 1)))
        );
        assert_eq!(
            place(Face::West),
            Some((IVec3::new(2, 64, 3), BlockState::new(50, 2)))
        );
        assert_eq!(
            place(Face::South),
            Some((IVec3::new(3, 64, 4), BlockState::new(50, 3)))
        );
        assert_eq!(
            place(Face::North),
            Some((IVec3::new(3, 64, 2), BlockState::new(50, 4)))
        );
        assert_eq!(place(Face::Bottom), None);
        // Not on glass.
        let world = world_with(&[(IVec3::new(3, 64, 3), BlockState::new(20, 0))]);
        let on_glass = hit(IVec3::new(3, 64, 3), Face::Top);
        assert_eq!(
            place_block(&world, &blocks, &on_glass, TORCH, &player),
            None
        );
    }

    #[test]
    fn breaking_a_wall_pops_its_torches_but_not_others() {
        let world = world_with(&[
            (IVec3::new(3, 64, 3), STONE),
            (IVec3::new(4, 64, 3), BlockState::new(50, 1)), // on the wall's east side
            (IVec3::new(3, 65, 3), BlockState::new(50, 5)), // on top of the wall
            (IVec3::new(3, 64, 4), BlockState::new(50, 5)), // on the floor beside it
        ]);
        let edits = break_block(&world, &blocks(), IVec3::new(3, 64, 3));
        let mut cells: Vec<IVec3> = edits.iter().map(|(at, _)| *at).collect();
        cells.sort_by_key(|c| (c.x, c.y, c.z));
        let expected = [
            IVec3::new(3, 64, 3),
            IVec3::new(3, 65, 3),
            IVec3::new(4, 64, 3),
        ];
        assert_eq!(cells, expected);
    }
}
