//! Breaking and placing blocks: which cells change, with 1.0.0's rules
//! and modern block states.

use glam::IVec3;

use super::{collision::Aabb, raycast::Hit};
use crate::{
    config::WORLD_HEIGHT,
    world::{
        Face, World,
        block::{BlockDef, BlockRegistry, BlockState, Placement},
        props::{Prop, axis, bed_part, door_half, hinge, slab_type},
    },
};

/// Ticks between breaks while the button is held (1.0.0's creative `blockHitDelay`).
pub const BREAK_REPEAT_TICKS: u8 = 5;
/// Ticks between placements while the button is held (1.0.0's `rightClickDelayTimer`).
pub const PLACE_REPEAT_TICKS: u8 = 4;

/// One block change.
pub type Edit = (IVec3, BlockState);

fn state_at(world: &World, at: IVec3) -> Option<BlockState> {
    world.block(at.x, at.y, at.z)
}

fn def_at<'a>(world: &World, blocks: &'a BlockRegistry, at: IVec3) -> Option<&'a BlockDef> {
    blocks.get(state_at(world, at)?.id())
}

/// Breaks the block at `at`, plus anything it held up: flowers or a sugar
/// cane stack on top, torches on its sides. Unbreakable blocks (bedrock)
/// give no edits.
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
            let Some(state) = state_at(world, neighbor) else {
                continue;
            };
            let held = state
                .def()
                .held_by(state)
                .iter()
                .any(|&offset| neighbor + offset == removed);
            if held && !edits.iter().any(|&(p, _)| p == neighbor) {
                edits.push((neighbor, BlockState::AIR));
            }
        }
    }
    edits
}

/// Where `block` goes when placed against `hit`, and in what state, or
/// `None` if it can't be: the cell must be in the world, loaded and
/// replaceable, a solid block can't overlap the player, and blocks that
/// need ground need it below. Beds, doors and slabs are finished by
/// `place_bed`, `place_door` and `place_slab`.
pub fn place_block(
    world: &World,
    blocks: &BlockRegistry,
    hit: &Hit,
    block: BlockState,
    player: &Aabb,
) -> Option<Edit> {
    let clicked = state_at(world, hit.block)?;
    // Clicking tall grass or snow replaces it instead of building beside it.
    let at = if clicked.def().is_replaceable(clicked) {
        hit.block
    } else {
        hit.adjacent()?
    };
    if !(0..WORLD_HEIGHT as i32).contains(&at.y) {
        return None;
    }
    let there = state_at(world, at)?;
    if !there.def().is_replaceable(there) {
        return None;
    }
    let new = blocks.get(block.id())?;
    if new.solid && Aabb::block(at.x, at.y, at.z).intersects(player) {
        return None;
    }
    let state = match new.placement {
        Placement::Torch { wall } => match hit.face? {
            Face::Top => block,
            Face::Bottom => return None,
            // On a wall, facing away from it.
            side => BlockState::of(wall)
                .with_face(Prop::Facing, side)
                .with_bool(Prop::Lit, block.bool(Prop::Lit)),
        },
        Placement::Axis => block.with(
            Prop::Axis,
            match hit.face {
                Some(Face::East | Face::West) => axis::X,
                Some(Face::North | Face::South) => axis::Z,
                _ => axis::Y,
            },
        ),
        // Turned toward the player, or made into two halves, afterwards.
        Placement::Plain
        | Placement::Facing
        | Placement::Bed
        | Placement::Door
        | Placement::Slab => block,
    };
    let def = state.def();
    if !matches!(def.placement, Placement::Bed | Placement::Door)
        && let Some(offset) = def.support(state)
    {
        let support = def_at(world, blocks, at + offset)?;
        // Torches need a full solid block; plants just need ground.
        let holds = if def.shape == crate::world::block::Shape::Torch {
            support.is_opaque_cube()
        } else {
            !support.replaceable
        };
        if !holds {
            return None;
        }
    }
    Some((at, state))
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
    // The head goes the way the player looks.
    let facing = Face::from_yaw(yaw);
    let head = foot + IVec3::from_array(facing.normal());
    for cell in [foot, head] {
        if !state_at(world, cell).is_some_and(BlockState::is_air)
            || !def_at(world, blocks, cell - IVec3::Y).is_some_and(BlockDef::is_opaque_cube)
            || Aabb::block(cell.x, cell.y, cell.z).intersects(player)
        {
            return None;
        }
    }
    let foot_state = bed
        .with_face(Prop::Facing, facing)
        .with(Prop::BedPart, bed_part::FOOT);
    Some([
        (foot, foot_state),
        (head, foot_state.with(Prop::BedPart, bed_part::HEAD)),
    ])
}

/// A door's two halves for `lower` (from `place_block`) and a player
/// looking along `yaw`: only on top of a full block, into two replaceable
/// cells clear of the player, facing the way the player looks. Its hinge
/// goes beside the solid side, or beside a door already there, so pairs
/// open apart (1.0.0's `ItemDoor.placeDoorBlock`).
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
    let above = state_at(world, upper)?;
    if !def_at(world, blocks, lower - IVec3::Y)?.is_opaque_cube()
        || !above.def().is_replaceable(above)
        || Aabb::block(upper.x, upper.y, upper.z).intersects(player)
    {
        return None;
    }
    let facing = Face::from_yaw(yaw);
    // The player's right.
    let right = IVec3::from_array(facing.rotate_cw().normal());
    let count = |offset: IVec3, test: &dyn Fn(BlockState) -> bool| {
        [lower, upper]
            .iter()
            .filter(|&&cell| state_at(world, cell + offset).is_some_and(test))
            .count()
    };
    let solid = |block: BlockState| block.def().is_opaque_cube();
    let is_door = |block: BlockState| block.id() == door.id();
    let (door_left, door_right) = (count(-right, &is_door) > 0, count(right, &is_door) > 0);
    let side = if (door_left && !door_right) || count(right, &solid) > count(-right, &solid) {
        hinge::RIGHT
    } else {
        hinge::LEFT
    };
    let lower_state = door
        .with_face(Prop::Facing, facing)
        .with(Prop::Hinge, side)
        .with(Prop::DoubleBlockHalf, door_half::LOWER);
    Some([
        (lower, lower_state),
        (
            upper,
            lower_state.with(Prop::DoubleBlockHalf, door_half::UPPER),
        ),
    ])
}

/// A slab placed at `at` onto the same slab (a bottom slab of the same
/// block) joins it into a double slab instead (1.0.0's
/// `BlockStep.onBlockAdded`).
pub fn place_slab(world: &World, (at, slab): Edit) -> Edit {
    let below = at - IVec3::Y;
    match state_at(world, below) {
        Some(under)
            if under.id() == slab.id() && under.get(Prop::SlabType) == slab_type::BOTTOM =>
        {
            (below, under.with(Prop::SlabType, slab_type::DOUBLE))
        }
        _ => (at, slab),
    }
}

/// The way a `Placement::Facing` block's front faces for a player looking
/// along `yaw` (radians, 0 north, turning east): toward the player.
pub fn facing_toward(yaw: f32) -> Face {
    Face::from_yaw(yaw).opposite()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        player::physics::hitbox,
        world::{
            Face, blocks,
            chunk::{Chunk, ChunkPos},
        },
    };
    use glam::DVec3;

    const STONE: BlockState = BlockState::of(blocks::STONE);
    const BEDROCK: BlockState = BlockState::of(blocks::BEDROCK);
    const SUGAR_CANE: BlockState = BlockState::of(blocks::SUGAR_CANE);
    const SHORT_GRASS: BlockState = BlockState::of(blocks::SHORT_GRASS);
    const WATER: BlockState = BlockState::of(blocks::WATER);
    const DANDELION: BlockState = BlockState::of(blocks::DANDELION);
    const TORCH: BlockState = BlockState::of(blocks::TORCH);

    fn blocks() -> &'static BlockRegistry {
        crate::world::block::registry()
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

    fn set(world: &mut World, edits: &[Edit]) {
        for &(at, block) in edits {
            let pos = ChunkPos::of_block(at.x, at.z);
            world
                .get_mut(pos)
                .unwrap()
                .set(at.x as usize, at.y as usize, at.z as usize, block);
        }
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
        assert_eq!(
            facing_toward(0.0),
            Face::South,
            "looking north, the front faces south"
        );
        assert_eq!(facing_toward(FRAC_PI_2), Face::West);
        assert_eq!(facing_toward(PI), Face::North);
        assert_eq!(facing_toward(-FRAC_PI_2), Face::East);
        assert_eq!(facing_toward(TAU - 0.2), Face::South);
    }

    #[test]
    fn beds_go_on_top_of_solid_blocks_with_the_head_ahead() {
        use std::f32::consts::FRAC_PI_2;
        let bed = BlockState::of(blocks::RED_BED);
        let world = world_with(&[(IVec3::new(5, 64, 3), STONE)]);
        let blocks = blocks();
        let on_top = hit(IVec3::new(5, 63, 5), Face::Top);
        let foot = place_block(&world, blocks, &on_top, bed, &far_player()).unwrap();
        // Looking east: the head goes east.
        let halves = place_bed(&world, blocks, &on_top, foot, FRAC_PI_2, &far_player());
        let east = bed.with_face(Prop::Facing, Face::East);
        assert_eq!(
            halves,
            Some([
                (IVec3::new(5, 64, 5), east),
                (
                    IVec3::new(6, 64, 5),
                    east.with(Prop::BedPart, bed_part::HEAD)
                ),
            ])
        );
        // Looking north, the head's cell is taken by stone.
        let blocked = hit(IVec3::new(5, 63, 4), Face::Top);
        let foot = place_block(&world, blocks, &blocked, bed, &far_player()).unwrap();
        assert_eq!(
            place_bed(&world, blocks, &blocked, foot, 0.0, &far_player()),
            None
        );
        // Only on top of blocks.
        let side = hit(IVec3::new(5, 64, 3), Face::East);
        let foot = place_block(&world, blocks, &side, bed, &far_player()).unwrap();
        assert_eq!(
            place_bed(&world, blocks, &side, foot, 0.0, &far_player()),
            None
        );
        // Breaking either half takes the other.
        let mut world = world;
        set(&mut world, &halves.unwrap());
        let broken = break_block(&world, blocks, IVec3::new(6, 64, 5));
        assert_eq!(broken.len(), 2);
    }

    #[test]
    fn doors_stand_on_full_blocks_and_hinge_against_walls() {
        let door = BlockState::of(blocks::OAK_DOOR);
        let mut world = world_with(&[(IVec3::new(4, 64, 8), STONE), (IVec3::new(4, 65, 8), STONE)]);
        let blocks = blocks();
        let place = |world: &World, x: i32, yaw: f32| {
            let on_top = hit(IVec3::new(x, 63, 8), Face::Top);
            let lower = place_block(world, blocks, &on_top, door, &far_player())?;
            place_door(world, blocks, &on_top, lower, yaw, &far_player())
        };
        // Looking north: facing north, hinged on the left.
        let north = door.with_face(Prop::Facing, Face::North);
        assert_eq!(
            place(&world, 8, 0.0),
            Some([
                (IVec3::new(8, 64, 8), north),
                (
                    IVec3::new(8, 65, 8),
                    north.with(Prop::DoubleBlockHalf, door_half::UPPER)
                ),
            ])
        );
        // With a wall to the east (on its right), it hinges on the right.
        let [(_, lower), _] = place(&world, 3, 0.0).unwrap();
        assert_eq!(lower, north.with(Prop::Hinge, hinge::RIGHT));
        // Breaking the ground takes both halves; so does breaking either.
        let [(at, lower), (above, upper)] = place(&world, 8, 0.0).unwrap();
        set(&mut world, &[(at, lower), (above, upper)]);
        assert_eq!(break_block(&world, blocks, at - IVec3::Y).len(), 3);
        assert_eq!(break_block(&world, blocks, above).len(), 2);
        assert_eq!(break_block(&world, blocks, at).len(), 2);
        // No room under the world's ceiling or on a side face.
        let side = hit(IVec3::new(4, 64, 8), Face::East);
        let lower = place_block(&world, blocks, &side, door, &far_player()).unwrap();
        assert_eq!(
            place_door(&world, blocks, &side, lower, 0.0, &far_player()),
            None
        );
    }

    #[test]
    fn a_slab_on_the_same_slab_makes_a_double_slab() {
        let oak = BlockState::of(blocks::OAK_SLAB);
        let world = world_with(&[(IVec3::new(2, 64, 2), oak)]);
        assert_eq!(
            place_slab(&world, (IVec3::new(2, 65, 2), oak)),
            (
                IVec3::new(2, 64, 2),
                oak.with(Prop::SlabType, slab_type::DOUBLE)
            )
        );
        // A different slab just stacks.
        let brick = BlockState::of(blocks::BRICK_SLAB);
        assert_eq!(
            place_slab(&world, (IVec3::new(2, 65, 2), brick)),
            (IVec3::new(2, 65, 2), brick)
        );
    }

    #[test]
    fn logs_lie_along_the_clicked_face() {
        let world = world_with(&[(IVec3::new(3, 64, 3), STONE)]);
        let log = BlockState::of(blocks::OAK_LOG);
        let place = |face| {
            place_block(
                &world,
                blocks(),
                &hit(IVec3::new(3, 64, 3), face),
                log,
                &far_player(),
            )
            .unwrap()
            .1
            .get(Prop::Axis)
        };
        assert_eq!(place(Face::Top), axis::Y);
        assert_eq!(place(Face::East), axis::X);
        assert_eq!(place(Face::North), axis::Z);
    }

    #[test]
    fn breaking_removes_the_block_and_plants_on_it() {
        let world = world_with(&[
            (IVec3::new(2, 64, 2), SUGAR_CANE),
            (IVec3::new(2, 65, 2), SUGAR_CANE),
            (IVec3::new(2, 66, 2), SUGAR_CANE),
        ]);
        let edits = break_block(&world, blocks(), IVec3::new(2, 63, 2));
        let cells: Vec<i32> = edits.iter().map(|(at, _)| at.y).collect();
        assert_eq!(cells, [63, 64, 65, 66]);
        assert!(edits.iter().all(|(_, block)| block.is_air()));
    }

    #[test]
    fn bedrock_air_and_unloaded_blocks_cannot_be_broken() {
        let world = world_with(&[(IVec3::new(1, 0, 1), BEDROCK)]);
        let blocks = blocks();
        assert!(break_block(&world, blocks, IVec3::new(1, 0, 1)).is_empty());
        assert!(break_block(&world, blocks, IVec3::new(1, 70, 1)).is_empty());
        assert!(break_block(&world, blocks, IVec3::new(40, 63, 1)).is_empty());
    }

    #[test]
    fn places_against_the_clicked_face() {
        let world = world_with(&[]);
        let edit = place_block(
            &world,
            blocks(),
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
            place_block(&world, blocks(), &below_feet, STONE, &player),
            None
        );
        // Non-solid blocks may overlap the player.
        let edit = place_block(&world, blocks(), &below_feet, DANDELION, &player);
        assert_eq!(edit, Some((IVec3::new(3, 64, 3), DANDELION)));
        // Standing next to the cell is fine.
        let beside = hitbox(DVec3::new(4.3, 64.0, 3.5));
        assert!(place_block(&world, blocks(), &below_feet, STONE, &beside).is_some());
    }

    #[test]
    fn clicking_tall_grass_replaces_it_and_water_can_be_built_into() {
        let world = world_with(&[
            (IVec3::new(5, 64, 5), SHORT_GRASS),
            (IVec3::new(6, 64, 5), WATER),
            (IVec3::new(7, 64, 5), STONE),
        ]);
        let blocks = blocks();
        let player = far_player();
        let edit = place_block(
            &world,
            blocks,
            &hit(IVec3::new(5, 64, 5), Face::Top),
            STONE,
            &player,
        );
        assert_eq!(edit, Some((IVec3::new(5, 64, 5), STONE)));
        let edit = place_block(
            &world,
            blocks,
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
            place_block(&world, blocks, &into_stone, STONE, &player),
            None
        );
        let top = WORLD_HEIGHT as i32 - 1;
        let world = world_with(&[(IVec3::new(3, top, 3), STONE)]);
        let above_top = hit(IVec3::new(3, top, 3), Face::Top);
        assert_eq!(
            place_block(&world, blocks, &above_top, STONE, &player),
            None
        );
        // Started inside a block: no face to place against.
        let inside = Hit {
            block: IVec3::new(3, 63, 3),
            face: None,
        };
        assert_eq!(place_block(&world, blocks, &inside, STONE, &player), None);
    }

    #[test]
    fn plants_need_ground() {
        let world = world_with(&[(IVec3::new(3, 65, 3), STONE)]);
        let blocks = blocks();
        let player = far_player();
        let on_floor = hit(IVec3::new(3, 63, 3), Face::Top);
        assert!(place_block(&world, blocks, &on_floor, DANDELION, &player).is_some());
        // Beside a floating block, over air.
        let on_side = hit(IVec3::new(3, 65, 3), Face::North);
        assert!(place_block(&world, blocks, &on_side, STONE, &player).is_some());
        assert_eq!(
            place_block(&world, blocks, &on_side, DANDELION, &player),
            None
        );
    }

    fn wall_torch(facing: Face) -> BlockState {
        BlockState::of(blocks::WALL_TORCH).with_face(Prop::Facing, facing)
    }

    #[test]
    fn torches_stand_on_floors_and_hang_on_walls() {
        let world = world_with(&[(IVec3::new(3, 64, 3), STONE)]);
        let blocks = blocks();
        let player = far_player();
        let place = |face| {
            place_block(
                &world,
                blocks,
                &hit(IVec3::new(3, 64, 3), face),
                TORCH,
                &player,
            )
        };
        assert_eq!(place(Face::Top), Some((IVec3::new(3, 65, 3), TORCH)));
        assert_eq!(
            place(Face::East),
            Some((IVec3::new(4, 64, 3), wall_torch(Face::East)))
        );
        assert_eq!(
            place(Face::West),
            Some((IVec3::new(2, 64, 3), wall_torch(Face::West)))
        );
        assert_eq!(
            place(Face::South),
            Some((IVec3::new(3, 64, 4), wall_torch(Face::South)))
        );
        assert_eq!(
            place(Face::North),
            Some((IVec3::new(3, 64, 2), wall_torch(Face::North)))
        );
        assert_eq!(place(Face::Bottom), None);
        // Not on glass.
        let world = world_with(&[(IVec3::new(3, 64, 3), BlockState::of(blocks::GLASS))]);
        let on_glass = hit(IVec3::new(3, 64, 3), Face::Top);
        assert_eq!(place_block(&world, blocks, &on_glass, TORCH, &player), None);
        // A redstone torch hangs as a redstone wall torch, still lit.
        let world = world_with(&[(IVec3::new(3, 64, 3), STONE)]);
        let redstone = BlockState::of(blocks::REDSTONE_TORCH);
        let (_, hung) = place_block(
            &world,
            blocks,
            &hit(IVec3::new(3, 64, 3), Face::East),
            redstone,
            &player,
        )
        .unwrap();
        assert!(hung.is(blocks::REDSTONE_WALL_TORCH) && hung.bool(Prop::Lit));
    }

    #[test]
    fn breaking_a_wall_pops_its_torches_but_not_others() {
        let world = world_with(&[
            (IVec3::new(3, 64, 3), STONE),
            (IVec3::new(4, 64, 3), wall_torch(Face::East)), // on the wall's east side
            (IVec3::new(3, 65, 3), TORCH),                  // on top of the wall
            (IVec3::new(3, 64, 4), TORCH),                  // on the floor beside it
        ]);
        let edits = break_block(&world, blocks(), IVec3::new(3, 64, 3));
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
