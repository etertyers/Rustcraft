//! Breaking and placing blocks: which cells change, following 1.0.0's rules.

use glam::IVec3;

use super::{collision::Aabb, raycast::Hit};
use crate::{
    config::WORLD_HEIGHT,
    world::{
        Face, World,
        block::{BlockDef, BlockId, BlockRegistry, BlockState, Placement, bed_direction},
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
            let held = blocks.get(state.id()).is_some_and(|def| {
                def.held_by(state.data())
                    .iter()
                    .any(|&offset| neighbor + offset == removed)
            });
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
        // Turned toward the player by `facing_data`, or made into two
        // halves by `place_bed` or `place_door`, afterwards.
        Placement::Plain
        | Placement::Facing
        | Placement::Bed
        | Placement::Door
        | Placement::Slab { .. } => block.data(),
        Placement::Torch => match hit.face? {
            Face::Top => 5,
            Face::East => 1,
            Face::West => 2,
            Face::South => 3,
            Face::North => 4,
            Face::Bottom => return None,
        },
    };
    if !matches!(new.placement, Placement::Bed | Placement::Door)
        && let Some(offset) = new.support(data)
    {
        let support = def_at(world, blocks, at + offset)?;
        // Torches need a full solid block; plants just need ground.
        let holds = match new.placement {
            Placement::Torch => support.is_opaque_cube(),
            Placement::Plain
            | Placement::Facing
            | Placement::Bed
            | Placement::Door
            | Placement::Slab { .. } => !support.replaceable,
        };
        if !holds {
            return None;
        }
    }
    Some((at, BlockState::new(block.id(), data)))
}

/// A bed's two halves for `foot` (from `place_block`) and a player looking
/// along `yaw`: only on top of a block, into empty cells above full solid
/// blocks, clear of the player (1.0.0's `ItemBed.onItemUse`).
pub fn place_bed(
    world: &World,
    blocks: &BlockRegistry,
    hit: &Hit,
    (foot, bed): Edit,
    yaw: f32,
    player: &Aabb,
) -> Option<[Edit; 2]> {
    if hit.face != Some(Face::Top) {
        return None;
    }
    let quarter = (yaw / std::f32::consts::FRAC_PI_2).round() as i32;
    // Looking north, east, south, west puts the head north, east, south, west.
    let dir = [2, 3, 0, 1][quarter.rem_euclid(4) as usize];
    let head = foot + bed_direction(dir);
    for cell in [foot, head] {
        if !world
            .block(cell.x, cell.y, cell.z)
            .is_some_and(BlockState::is_air)
            || !def_at(world, blocks, cell - IVec3::Y).is_some_and(BlockDef::is_opaque_cube)
            || Aabb::block(cell.x, cell.y, cell.z).intersects(player)
        {
            return None;
        }
    }
    Some([
        (foot, BlockState::new(bed.id(), dir)),
        (head, BlockState::new(bed.id(), dir | 8)),
    ])
}

/// A door's two halves for `lower` (from `place_block`) and a player
/// looking along `yaw`: only on top of a full block, into two replaceable
/// cells clear of the player. Its hinge goes beside the solid side, or
/// beside a door already there, so pairs open apart (1.0.0's
/// `ItemDoor.placeDoorBlock`).
pub fn place_door(
    world: &World,
    blocks: &BlockRegistry,
    hit: &Hit,
    (lower, door): Edit,
    yaw: f32,
    player: &Aabb,
) -> Option<[Edit; 2]> {
    if hit.face != Some(Face::Top) || lower.y + 1 >= WORLD_HEIGHT as i32 {
        return None;
    }
    let upper = lower + IVec3::Y;
    if !def_at(world, blocks, lower - IVec3::Y)?.is_opaque_cube()
        || !def_at(world, blocks, upper)?.replaceable
        || Aabb::block(upper.x, upper.y, upper.z).intersects(player)
    {
        return None;
    }
    let quarter = (yaw / std::f32::consts::FRAC_PI_2).round() as i32;
    // Looking north, east, south, west gives direction 3, 0, 1, 2.
    let mut dir = (quarter + 3).rem_euclid(4) as u8;
    let side = [IVec3::Z, IVec3::NEG_X, IVec3::NEG_Z, IVec3::X][dir as usize];
    let count = |offset: IVec3, test: &dyn Fn(BlockState) -> bool| {
        [lower, upper]
            .iter()
            .filter(|&&cell| {
                let at = cell + offset;
                world.block(at.x, at.y, at.z).is_some_and(test)
            })
            .count()
    };
    let solid = |block: BlockState| blocks.get(block.id()).is_some_and(BlockDef::is_opaque_cube);
    let is_door = |block: BlockState| block.id() == door.id();
    let (door_left, door_right) = (count(-side, &is_door) > 0, count(side, &is_door) > 0);
    if (door_left && !door_right) || count(side, &solid) > count(-side, &solid) {
        // Hinged on the other side: turned back a quarter and "open".
        dir = ((dir + 3) & 3) | 4;
    }
    Some([
        (lower, BlockState::new(door.id(), dir)),
        (upper, BlockState::new(door.id(), dir | 8)),
    ])
}

/// A slab placed at `at` onto the same slab (same block and data value)
/// joins it into the `double` block instead (1.0.0's
/// `BlockStep.onBlockAdded`).
pub fn place_slab(world: &World, (at, slab): Edit, double: BlockId) -> Edit {
    let below = at - IVec3::Y;
    match world.block(below.x, below.y, below.z) {
        Some(under) if under == slab => (below, BlockState::new(double, slab.data())),
        _ => (at, slab),
    }
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
    fn beds_go_on_top_of_solid_blocks_with_the_head_ahead() {
        use std::f32::consts::FRAC_PI_2;
        const BED: BlockState = BlockState::new(26, 0);
        let world = world_with(&[(IVec3::new(5, 64, 3), STONE)]);
        let blocks = blocks();
        let on_top = hit(IVec3::new(5, 63, 5), Face::Top);
        let foot = place_block(&world, &blocks, &on_top, BED, &far_player()).unwrap();
        // Looking east: the head goes east (direction 3).
        let halves = place_bed(&world, &blocks, &on_top, foot, FRAC_PI_2, &far_player());
        assert_eq!(
            halves,
            Some([
                (IVec3::new(5, 64, 5), BlockState::new(26, 3)),
                (IVec3::new(6, 64, 5), BlockState::new(26, 3 | 8)),
            ])
        );
        // Looking north, the head's cell is taken by stone.
        let blocked = hit(IVec3::new(5, 63, 4), Face::Top);
        let foot = place_block(&world, &blocks, &blocked, BED, &far_player()).unwrap();
        assert_eq!(
            place_bed(&world, &blocks, &blocked, foot, 0.0, &far_player()),
            None
        );
        // Only on top of blocks.
        let side = hit(IVec3::new(5, 64, 3), Face::East);
        let foot = place_block(&world, &blocks, &side, BED, &far_player()).unwrap();
        assert_eq!(
            place_bed(&world, &blocks, &side, foot, 0.0, &far_player()),
            None
        );
        // Breaking either half takes the other.
        let mut world = world;
        for (at, block) in halves.unwrap() {
            let pos = ChunkPos::of_block(at.x, at.z);
            world
                .get_mut(pos)
                .unwrap()
                .set(at.x as usize, at.y as usize, at.z as usize, block);
        }
        let broken = break_block(&world, &blocks, IVec3::new(6, 64, 5));
        assert_eq!(broken.len(), 2);
    }

    #[test]
    fn doors_stand_on_full_blocks_and_hinge_against_walls() {
        const DOOR: BlockState = BlockState::new(64, 0);
        let mut world = world_with(&[(IVec3::new(4, 64, 8), STONE), (IVec3::new(4, 65, 8), STONE)]);
        let blocks = blocks();
        let place = |world: &World, x: i32, yaw: f32| {
            let on_top = hit(IVec3::new(x, 63, 8), Face::Top);
            let lower = place_block(world, &blocks, &on_top, DOOR, &far_player())?;
            place_door(world, &blocks, &on_top, lower, yaw, &far_player())
        };
        // Looking north: direction 3, the upper half adding 8.
        assert_eq!(
            place(&world, 8, 0.0),
            Some([
                (IVec3::new(8, 64, 8), BlockState::new(64, 3)),
                (IVec3::new(8, 65, 8), BlockState::new(64, 3 | 8)),
            ])
        );
        // With a wall to the east (on its right), it hinges the other way:
        // a quarter back and marked open.
        let [(_, lower), _] = place(&world, 3, 0.0).unwrap();
        assert_eq!(lower, BlockState::new(64, 2 | 4));
        // Breaking the ground takes both halves; so does breaking either.
        let [(at, lower), (above, upper)] = place(&world, 8, 0.0).unwrap();
        for (cell, block) in [(at, lower), (above, upper)] {
            let pos = ChunkPos::of_block(cell.x, cell.z);
            world.get_mut(pos).unwrap().set(
                cell.x as usize,
                cell.y as usize,
                cell.z as usize,
                block,
            );
        }
        assert_eq!(break_block(&world, &blocks, at - IVec3::Y).len(), 3);
        assert_eq!(break_block(&world, &blocks, above).len(), 2);
        assert_eq!(break_block(&world, &blocks, at).len(), 2);
        // No room under the world's ceiling or on a side face.
        let side = hit(IVec3::new(4, 64, 8), Face::East);
        let lower = place_block(&world, &blocks, &side, DOOR, &far_player()).unwrap();
        assert_eq!(
            place_door(&world, &blocks, &side, lower, 0.0, &far_player()),
            None
        );
    }

    #[test]
    fn a_slab_on_the_same_slab_makes_a_double_slab() {
        let wooden = BlockState::new(126, 0);
        let world = world_with(&[(IVec3::new(2, 64, 2), wooden)]);
        assert_eq!(
            place_slab(&world, (IVec3::new(2, 65, 2), wooden), 125),
            (IVec3::new(2, 64, 2), BlockState::new(125, 0))
        );
        // Different slabs, or a different variant, just stack.
        let brick = BlockState::new(44, 4);
        assert_eq!(
            place_slab(&world, (IVec3::new(2, 65, 2), brick), 43),
            (IVec3::new(2, 65, 2), brick)
        );
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
