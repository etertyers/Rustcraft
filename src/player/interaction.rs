//! Breaking and placing blocks: which cells change, with 1.0.0's rules
//! and modern block states.

use glam::{DVec3, IVec3};

use super::{collision::Aabb, raycast::Hit};
use crate::{
    config::WORLD_HEIGHT,
    world::{
        Face, World,
        block::{BlockDef, BlockRegistry, BlockState, Placement, Shape},
        connect::{self, sturdy},
        props::{Prop, attach_face, axis, bed_part, door_half, half, hinge, rail_shape, slab_type},
        rails, shape,
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

/// Whether the cell holding `there` can take `block` placed into it:
/// anything replaceable (air, tall grass, water), or the same slab or
/// snow layer that `block` joins (modern `canBeReplaced`). `clicked` is
/// whether `there` is the block clicked, rather than the cell beside it.
fn takes(there: BlockState, block: BlockState, hit: &Hit, clicked: bool) -> bool {
    let def = there.def();
    if there.id() == block.id() {
        match def.placement {
            Placement::Slab if there.get(Prop::SlabType) != slab_type::DOUBLE => {
                if !clicked {
                    return true;
                }
                let upper = hit.height_in_block() > 0.5;
                return match (there.get(Prop::SlabType), hit.face) {
                    (slab_type::BOTTOM, Some(face)) => {
                        face == Face::Top || (upper && face.is_horizontal())
                    }
                    (_, Some(face)) => face == Face::Bottom || (!upper && face.is_horizontal()),
                    _ => false,
                };
            }
            Placement::Layers if there.number(Prop::Layers) < 8 => {
                return !clicked || hit.face == Some(Face::Top);
            }
            _ => {}
        }
    }
    def.is_replaceable(there)
}

/// What placing `block` (an item's block, in its placed state) against
/// `hit` changes, or `None` if it can't go there: the cell must be in the
/// world, loaded and able to take it, blocks that collide can't overlap
/// the player (`player`, looking along `yaw`), and blocks that hang on
/// something need it there. Beds and doors take two cells. Blocks whose
/// state hangs on their neighbors (fences, stairs) are joined up.
pub fn place_block(
    world: &World,
    blocks: &BlockRegistry,
    hit: &Hit,
    block: BlockState,
    player: &Aabb,
    yaw: f32,
) -> Option<Vec<Edit>> {
    place_block_looking(world, blocks, hit, block, player, yaw, 0.0)
}

/// As `place_block`, for a player looking along `yaw` and `pitch`, which
/// blocks that face up or down (pistons, droppers) turn by.
pub fn place_block_looking(
    world: &World,
    blocks: &BlockRegistry,
    hit: &Hit,
    block: BlockState,
    player: &Aabb,
    yaw: f32,
    pitch: f32,
) -> Option<Vec<Edit>> {
    let clicked = state_at(world, hit.block)?;
    // Clicking tall grass, or a slab this one completes, places into it
    // instead of building beside it.
    let (at, there) = if takes(clicked, block, hit, true) {
        (hit.block, clicked)
    } else {
        let at = hit.adjacent()?;
        let there = state_at(world, at)?;
        if !takes(there, block, hit, false) {
            return None;
        }
        (at, there)
    };
    if !(0..WORLD_HEIGHT as i32).contains(&at.y) {
        return None;
    }
    let looking = Face::from_yaw(yaw);
    let face = hit.face?;
    // A side clicked on its top half.
    let upper = hit.height_in_block() > 0.5;
    let def = blocks.get(block.id())?;
    let state = match def.placement {
        Placement::Plain => block,
        Placement::Torch { wall } => match face {
            Face::Top => block,
            Face::Bottom => return None,
            // On a wall, facing away from it.
            side => BlockState::of(wall)
                .with_face(Prop::Facing, side)
                .with_bool(Prop::Lit, block.bool(Prop::Lit)),
        },
        Placement::Facing => {
            if block.has(Prop::FacingAll) {
                block.with_face(Prop::FacingAll, facing_toward_looking(yaw, pitch))
            } else {
                block.with_face(Prop::Facing, facing_toward(yaw))
            }
        }
        Placement::LookAll => block.with_face(Prop::FacingAll, facing_toward_looking(yaw, pitch)),
        // The spout points into the clicked block; from below, it points down.
        Placement::Hopper => block.with_face(
            Prop::FacingAll,
            match face {
                Face::Bottom => Face::Top,
                side => side,
            }
            .opposite(),
        ),
        Placement::Look | Placement::Bed | Placement::Door => {
            block.with_face(Prop::Facing, looking)
        }
        Placement::LookClockwise => block.with_face(Prop::Facing, looking.rotate_cw()),
        Placement::Axis => block.with(
            Prop::Axis,
            match face {
                Face::East | Face::West => axis::X,
                Face::North | Face::South => axis::Z,
                _ => axis::Y,
            },
        ),
        Placement::Slab => {
            let kind = if there.id() == block.id() {
                slab_type::DOUBLE
            } else {
                match face {
                    Face::Bottom => slab_type::TOP,
                    Face::Top => slab_type::BOTTOM,
                    _ if upper => slab_type::TOP,
                    _ => slab_type::BOTTOM,
                }
            };
            block.with(Prop::SlabType, kind)
        }
        Placement::Stairs => {
            let top = face == Face::Bottom || (face != Face::Top && upper);
            block
                .with_face(Prop::Facing, looking)
                .with(Prop::Half, if top { half::TOP } else { half::BOTTOM })
        }
        Placement::Trapdoor => {
            if face.is_horizontal() && hit.block != at {
                block
                    .with_face(Prop::Facing, face)
                    .with(Prop::Half, if upper { half::TOP } else { half::BOTTOM })
            } else {
                let top = face == Face::Bottom;
                block
                    .with_face(Prop::Facing, looking.opposite())
                    .with(Prop::Half, if top { half::TOP } else { half::BOTTOM })
            }
        }
        Placement::Wall => {
            // On the wall clicked, or failing that the nearest wall the
            // player faces (modern `LadderBlock`), facing away from it.
            let mut toward = Vec::with_capacity(4);
            if face.is_horizontal() {
                toward.push(face.opposite());
            }
            toward.extend(horizontal_by_look(yaw));
            let wall = toward.into_iter().find(|&d| {
                state_at(world, at + IVec3::from_array(d.normal())).is_some_and(sturdy)
            })?;
            block.with_face(Prop::Facing, wall.opposite())
        }
        Placement::Attached => match face {
            Face::Top => block
                .with(Prop::AttachFace, attach_face::FLOOR)
                .with_face(Prop::Facing, looking),
            Face::Bottom => block
                .with(Prop::AttachFace, attach_face::CEILING)
                .with_face(Prop::Facing, looking),
            side => block
                .with(Prop::AttachFace, attach_face::WALL)
                .with_face(Prop::Facing, side),
        },
        Placement::Vine => {
            // On the clicked block's side or underside, or failing that the
            // nearest side the player faces, or the block above (modern
            // `VineBlock`); never on the ground.
            let base = if there.id() == block.id() {
                there
            } else {
                block
            };
            let mut toward = Vec::with_capacity(6);
            if face != Face::Top {
                toward.push(face.opposite());
            }
            toward.extend(horizontal_by_look(yaw));
            toward.push(Face::Top);
            let side = toward.into_iter().find(|&d| {
                !base.bool(Prop::side(d))
                    && state_at(world, at + IVec3::from_array(d.normal())).is_some_and(sturdy)
            })?;
            base.with_bool(Prop::side(side), true)
        }
        Placement::Layers => {
            if there.id() == block.id() {
                there.with_number(Prop::Layers, there.number(Prop::Layers) + 1)
            } else {
                block
            }
        }
        Placement::Rail => {
            let along_x = matches!(looking, Face::East | Face::West);
            if block.has(Prop::RailShape) {
                block.with(
                    Prop::RailShape,
                    if along_x {
                        rail_shape::EAST_WEST
                    } else {
                        rail_shape::NORTH_SOUTH
                    },
                )
            } else {
                block.with(
                    Prop::StraightRailShape,
                    if along_x {
                        rail_shape::EAST_WEST
                    } else {
                        rail_shape::NORTH_SOUTH
                    },
                )
            }
        }
    };
    let new = state.def();
    // Whatever it hangs on must be there to hold it.
    if !matches!(new.placement, Placement::Bed | Placement::Door)
        && let Some(offset) = new.support(state)
    {
        let support = state_at(world, at + offset)?;
        if !holds(new.shape, support) {
            return None;
        }
    }
    if collides(state, at, player) {
        return None;
    }
    let edits = match new.placement {
        Placement::Bed => place_bed(world, blocks, hit, (at, state), player)?.to_vec(),
        Placement::Door => place_door(world, blocks, hit, (at, state), player)?.to_vec(),
        Placement::Rail => rails::place(&|p| state_at(world, p), at, state),
        _ => vec![(at, state)],
    };
    // Join fences, stairs and dust to what's around them.
    let get = |p: IVec3| {
        edits
            .iter()
            .find(|&&(at, _)| at == p)
            .map(|&(_, block)| block)
            .or_else(|| state_at(world, p))
    };
    Some(
        edits
            .iter()
            .map(|&(at, block)| (at, connect::update_shape(block, at, &get)))
            .collect(),
    )
}

/// Whether `support` can hold up a block of `shape` hanging on it: a full
/// block for torches, ladders, levers and buttons, anything not replaceable
/// (not air, water or plants) for the rest.
pub fn holds(shape: Shape, support: BlockState) -> bool {
    match shape {
        Shape::Torch | Shape::Ladder | Shape::Lever | Shape::Button => sturdy(support),
        _ => !support.def().replaceable,
    }
}

/// The four sides, the one the player looks most toward first (modern
/// `Direction.orderedByNearest`, horizontally).
fn horizontal_by_look(yaw: f32) -> [Face; 4] {
    let look = (yaw.sin(), -yaw.cos());
    let toward = |face: &Face| {
        let [x, _, z] = face.normal();
        x as f32 * look.0 + z as f32 * look.1
    };
    let mut faces = [Face::North, Face::East, Face::South, Face::West];
    faces.sort_by(|a, b| toward(b).total_cmp(&toward(a)));
    faces
}

/// Whether `state` at `at` would collide with `player`.
fn collides(state: BlockState, at: IVec3, player: &Aabb) -> bool {
    let corner = at.as_dvec3();
    shape::shapes().collision(state).iter().any(|b| {
        Aabb::new(
            corner + DVec3::from(b.min.map(f64::from)),
            corner + DVec3::from(b.max.map(f64::from)),
        )
        .intersects(player)
    })
}

/// A bed's two halves for `foot` (from `place_block`, facing the way the
/// player looks): only on top of a block, into empty cells above full
/// solid blocks, clear of the player (1.0.0's `ItemBed.onItemUse`).
fn place_bed(
    world: &World,
    blocks: &BlockRegistry,
    hit: &Hit,
    (foot, bed): Edit,
    player: &Aabb,
) -> Option<[Edit; 2]> {
    if hit.face != Some(Face::Top) {
        return None;
    }
    let head = foot + IVec3::from_array(bed.face(Prop::Facing).normal());
    for cell in [foot, head] {
        if !state_at(world, cell).is_some_and(BlockState::is_air)
            || !def_at(world, blocks, cell - IVec3::Y).is_some_and(BlockDef::is_opaque_cube)
            || Aabb::block(cell.x, cell.y, cell.z).intersects(player)
        {
            return None;
        }
    }
    let foot_state = bed.with(Prop::BedPart, bed_part::FOOT);
    Some([
        (foot, foot_state),
        (head, foot_state.with(Prop::BedPart, bed_part::HEAD)),
    ])
}

/// A door's two halves for `lower` (from `place_block`, facing the way the
/// player looks): only on top of a full block, into two replaceable cells
/// clear of the player. Its hinge goes beside the solid side, or beside a
/// door already there, so pairs open apart (1.0.0's
/// `ItemDoor.placeDoorBlock`).
fn place_door(
    world: &World,
    blocks: &BlockRegistry,
    hit: &Hit,
    (lower, door): Edit,
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
    // The player's right.
    let right = IVec3::from_array(door.face(Prop::Facing).rotate_cw().normal());
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

/// The way a `Placement::Facing` block's front faces for a player looking
/// along `yaw` (radians, 0 north, turning east): toward the player.
pub fn facing_toward(yaw: f32) -> Face {
    Face::from_yaw(yaw).opposite()
}

/// As `facing_toward`, but also up or down when the player looks steeply
/// that way (modern `getNearestLookingDirection().getOpposite()`).
pub fn facing_toward_looking(yaw: f32, pitch: f32) -> Face {
    let (sin_yaw, cos_yaw) = yaw.sin_cos();
    let flat = pitch.cos();
    let (x, z) = (sin_yaw * flat, -cos_yaw * flat);
    let y = pitch.sin();
    let looking = if y.abs() > x.abs().max(z.abs()) {
        if y > 0.0 { Face::Top } else { Face::Bottom }
    } else {
        Face::from_yaw(yaw)
    };
    looking.opposite()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        player::physics::hitbox,
        world::{
            Face, blocks,
            chunk::{Chunk, ChunkPos},
            props::stairs_shape,
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

    /// A hit on `face` of `block`, `height` up it (0.5 is the middle).
    fn hit_at(block: IVec3, face: Face, height: f64) -> Hit {
        let center = block.as_dvec3() + DVec3::splat(0.5);
        let mut point = center + IVec3::from_array(face.normal()).as_dvec3() * 0.5;
        if face.is_horizontal() {
            point.y = block.y as f64 + height;
        }
        Hit {
            block,
            face: Some(face),
            point,
        }
    }

    /// A hit on the middle of `face` of `block`.
    fn hit(block: IVec3, face: Face) -> Hit {
        hit_at(block, face, 0.4)
    }

    /// A player standing well away from the test cells.
    fn far_player() -> Aabb {
        hitbox(DVec3::new(12.5, 64.0, 12.5))
    }

    /// Places `block` against `hit` looking along `yaw`, by a far player.
    fn place(world: &World, hit: &Hit, block: BlockState, yaw: f32) -> Option<Vec<Edit>> {
        place_block(world, blocks(), hit, block, &far_player(), yaw)
    }

    /// The one cell a placement changes.
    fn place_one(world: &World, hit: &Hit, block: BlockState) -> Option<Edit> {
        let edits = place(world, hit, block, 0.0)?;
        assert_eq!(edits.len(), 1);
        Some(edits[0])
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
        let world = world_with(&[]);
        let furnace = BlockState::of(blocks::FURNACE);
        let (_, placed) =
            place_one(&world, &hit(IVec3::new(3, 63, 3), Face::Top), furnace).unwrap();
        assert_eq!(placed.face(Prop::Facing), Face::South);
    }

    #[test]
    fn beds_go_on_top_of_solid_blocks_with_the_head_ahead() {
        use std::f32::consts::FRAC_PI_2;
        let bed = BlockState::of(blocks::RED_BED);
        let world = world_with(&[(IVec3::new(5, 64, 3), STONE)]);
        let on_top = hit(IVec3::new(5, 63, 5), Face::Top);
        // Looking east: the head goes east.
        let halves = place(&world, &on_top, bed, FRAC_PI_2).unwrap();
        let east = bed.with_face(Prop::Facing, Face::East);
        assert_eq!(
            halves,
            [
                (IVec3::new(5, 64, 5), east),
                (
                    IVec3::new(6, 64, 5),
                    east.with(Prop::BedPart, bed_part::HEAD)
                ),
            ]
        );
        // Looking north, the head's cell is taken by stone.
        assert_eq!(
            place(&world, &hit(IVec3::new(5, 63, 4), Face::Top), bed, 0.0),
            None
        );
        // Only on top of blocks.
        assert_eq!(
            place(&world, &hit(IVec3::new(5, 64, 3), Face::East), bed, 0.0),
            None
        );
        // Breaking either half takes the other.
        let mut world = world;
        set(&mut world, &halves);
        let broken = break_block(&world, blocks(), IVec3::new(6, 64, 5));
        assert_eq!(broken.len(), 2);
    }

    #[test]
    fn doors_stand_on_full_blocks_and_hinge_against_walls() {
        let door = BlockState::of(blocks::OAK_DOOR);
        let mut world = world_with(&[(IVec3::new(4, 64, 8), STONE), (IVec3::new(4, 65, 8), STONE)]);
        let put = |world: &World, x: i32, yaw: f32| {
            place(world, &hit(IVec3::new(x, 63, 8), Face::Top), door, yaw)
        };
        // Looking north: facing north, hinged on the left.
        let north = door.with_face(Prop::Facing, Face::North);
        assert_eq!(
            put(&world, 8, 0.0),
            Some(vec![
                (IVec3::new(8, 64, 8), north),
                (
                    IVec3::new(8, 65, 8),
                    north.with(Prop::DoubleBlockHalf, door_half::UPPER)
                ),
            ])
        );
        // With a wall to the east (on its right), it hinges on the right.
        let lower = put(&world, 3, 0.0).unwrap()[0].1;
        assert_eq!(lower, north.with(Prop::Hinge, hinge::RIGHT));
        // Breaking the ground takes both halves; so does breaking either.
        let halves = put(&world, 8, 0.0).unwrap();
        let (at, above) = (halves[0].0, halves[1].0);
        set(&mut world, &halves);
        assert_eq!(break_block(&world, blocks(), at - IVec3::Y).len(), 3);
        assert_eq!(break_block(&world, blocks(), above).len(), 2);
        assert_eq!(break_block(&world, blocks(), at).len(), 2);
        // Not on a side face.
        assert_eq!(
            place(&world, &hit(IVec3::new(4, 64, 8), Face::East), door, 0.0),
            None
        );
    }

    #[test]
    fn slabs_go_on_the_half_clicked_and_join_into_double_slabs() {
        let oak = BlockState::of(blocks::OAK_SLAB);
        let world = world_with(&[(IVec3::new(4, 64, 4), STONE)]);
        let kind = |hit: Hit| place_one(&world, &hit, oak).map(|(_, s)| s.get(Prop::SlabType));
        // On the ground: the bottom half. Under a block: the top half.
        assert_eq!(
            kind(hit(IVec3::new(2, 63, 2), Face::Top)),
            Some(slab_type::BOTTOM)
        );
        assert_eq!(
            kind(hit(IVec3::new(4, 64, 4), Face::Bottom)),
            None,
            "under it is stone"
        );
        // On a side: by which half was clicked.
        assert_eq!(
            kind(hit_at(IVec3::new(4, 64, 4), Face::East, 0.25)),
            Some(slab_type::BOTTOM)
        );
        assert_eq!(
            kind(hit_at(IVec3::new(4, 64, 4), Face::East, 0.75)),
            Some(slab_type::TOP)
        );
        // Clicking the top of a bottom slab fills it in.
        let world = world_with(&[(IVec3::new(2, 64, 2), oak)]);
        assert_eq!(
            place_one(&world, &hit(IVec3::new(2, 64, 2), Face::Top), oak),
            Some((
                IVec3::new(2, 64, 2),
                oak.with(Prop::SlabType, slab_type::DOUBLE)
            ))
        );
        // A different slab goes on top instead.
        let brick = BlockState::of(blocks::BRICK_SLAB);
        assert_eq!(
            place_one(&world, &hit(IVec3::new(2, 64, 2), Face::Top), brick),
            Some((IVec3::new(2, 65, 2), brick))
        );
    }

    #[test]
    fn stairs_face_the_way_the_player_looks_and_turn_upside_down_up_high() {
        use std::f32::consts::FRAC_PI_2;
        let stairs = BlockState::of(blocks::OAK_STAIRS);
        let world = world_with(&[(IVec3::new(4, 64, 4), STONE)]);
        let (_, placed) = place(
            &world,
            &hit(IVec3::new(2, 63, 2), Face::Top),
            stairs,
            FRAC_PI_2,
        )
        .unwrap()[0];
        assert_eq!(placed.face(Prop::Facing), Face::East);
        assert_eq!(placed.get(Prop::Half), half::BOTTOM);
        let (_, high) = place(
            &world,
            &hit_at(IVec3::new(4, 64, 4), Face::West, 0.8),
            stairs,
            0.0,
        )
        .unwrap()[0];
        assert_eq!(high.get(Prop::Half), half::TOP);
        // Beside stairs turned the other way, it makes a corner.
        let west = stairs.with_face(Prop::Facing, Face::West);
        let world = world_with(&[(IVec3::new(2, 64, 1), west)]);
        let (_, corner) =
            place(&world, &hit(IVec3::new(2, 63, 2), Face::Top), stairs, 0.0).unwrap()[0];
        assert_eq!(corner.get(Prop::StairsShape), stairs_shape::OUTER_LEFT);
    }

    #[test]
    fn fences_join_what_they_are_placed_beside() {
        let fence = BlockState::of(blocks::OAK_FENCE);
        let world = world_with(&[(IVec3::new(3, 64, 3), fence), (IVec3::new(5, 64, 4), STONE)]);
        let (_, placed) = place_one(&world, &hit(IVec3::new(4, 63, 3), Face::Top), fence).unwrap();
        assert!(placed.bool(Prop::West), "joins the fence");
        assert!(!placed.bool(Prop::East) && !placed.bool(Prop::North));
    }

    #[test]
    fn levers_buttons_ladders_and_trapdoors_attach_to_the_clicked_face() {
        let world = world_with(&[(IVec3::new(4, 64, 4), STONE)]);
        let stone = IVec3::new(4, 64, 4);
        let button = BlockState::of(blocks::STONE_BUTTON);
        let (at, wall) = place_one(&world, &hit(stone, Face::South), button).unwrap();
        assert_eq!(at, IVec3::new(4, 64, 5));
        assert_eq!(wall.get(Prop::AttachFace), attach_face::WALL);
        assert_eq!(wall.face(Prop::Facing), Face::South);
        let lever = BlockState::of(blocks::LEVER);
        let (_, floor) = place_one(&world, &hit(stone, Face::Top), lever).unwrap();
        assert_eq!(floor.get(Prop::AttachFace), attach_face::FLOOR);
        // Ladders go only on sides, facing out.
        let ladder = BlockState::of(blocks::LADDER);
        let (_, hung) = place_one(&world, &hit(stone, Face::West), ladder).unwrap();
        assert_eq!(hung.face(Prop::Facing), Face::West);
        assert_eq!(place_one(&world, &hit(stone, Face::Top), ladder), None);
        // A trapdoor on the top half of a side hangs from the top.
        let trapdoor = BlockState::of(blocks::OAK_TRAPDOOR);
        let (_, high) = place_one(&world, &hit_at(stone, Face::North, 0.8), trapdoor).unwrap();
        assert_eq!(high.get(Prop::Half), half::TOP);
        assert_eq!(high.face(Prop::Facing), Face::North);
    }

    #[test]
    fn ladders_and_vines_find_the_wall_you_face_when_you_click_the_floor() {
        use std::f32::consts::PI;
        let wall = IVec3::new(4, 64, 4);
        let floor = IVec3::new(4, 63, 5);
        let world = world_with(&[(wall, STONE), (floor, STONE)]);
        let ladder = BlockState::of(blocks::LADDER);
        let vine = BlockState::of(blocks::VINE);
        // Clicking the floor in front of a wall hangs it on the wall,
        // whichever way the player looks.
        for yaw in [0.0, PI / 2.0, PI] {
            let edits = place(&world, &hit(floor, Face::Top), ladder, yaw).unwrap();
            assert_eq!(edits[0].0, IVec3::new(4, 64, 5));
            assert_eq!(edits[0].1.face(Prop::Facing), Face::South, "yaw {yaw}");
            let edits = place(&world, &hit(floor, Face::Top), vine, yaw).unwrap();
            assert!(edits[0].1.bool(Prop::North), "yaw {yaw}");
        }
        // Out in the open there's nothing to hang on.
        let open = world_with(&[(floor, STONE)]);
        assert_eq!(place(&open, &hit(floor, Face::Top), ladder, 0.0), None);
        assert_eq!(place(&open, &hit(floor, Face::Top), vine, 0.0), None);
        // A vine clicked onto a block's underside hangs from it.
        let overhang = IVec3::new(8, 66, 8);
        let ceiling = world_with(&[(overhang, STONE)]);
        let (at, hanging) = place_one(&ceiling, &hit(overhang, Face::Bottom), vine).unwrap();
        assert_eq!(at, IVec3::new(8, 65, 8));
        assert!(hanging.bool(Prop::Up));
        // A ladder clicked onto a side still goes on that side first.
        let (_, hung) = place_one(&world, &hit(wall, Face::East), ladder).unwrap();
        assert_eq!(hung.face(Prop::Facing), Face::East);
    }

    #[test]
    fn a_ladder_wont_go_where_the_player_stands() {
        let wall = IVec3::new(4, 64, 4);
        let world = world_with(&[(wall, STONE)]);
        // Pressed against the wall, the ladder's plate would be inside you.
        let hugging = hitbox(DVec3::new(4.5, 64.0, 5.3));
        let ladder = BlockState::of(blocks::LADDER);
        assert_eq!(
            place_block(
                &world,
                blocks(),
                &hit(wall, Face::South),
                ladder,
                &hugging,
                0.0
            ),
            None
        );
        // A step back, it fits.
        let back = hitbox(DVec3::new(4.5, 64.0, 5.6));
        assert!(
            place_block(
                &world,
                blocks(),
                &hit(wall, Face::South),
                ladder,
                &back,
                0.0
            )
            .is_some()
        );
    }

    #[test]
    fn snow_stacks_layers_up_to_a_full_block() {
        let snow = BlockState::of(blocks::SNOW);
        let three = snow.with_number(Prop::Layers, 3);
        let world = world_with(&[(IVec3::new(2, 64, 2), three)]);
        let (at, more) = place_one(&world, &hit(IVec3::new(2, 64, 2), Face::Top), snow).unwrap();
        assert_eq!((at, more.number(Prop::Layers)), (IVec3::new(2, 64, 2), 4));
        // Stone can't go into three layers of snow, only on top.
        assert_eq!(
            place_one(&world, &hit(IVec3::new(2, 64, 2), Face::Top), STONE).map(|(at, _)| at),
            Some(IVec3::new(2, 65, 2))
        );
    }

    #[test]
    fn logs_lie_along_the_clicked_face() {
        let world = world_with(&[(IVec3::new(3, 64, 3), STONE)]);
        let log = BlockState::of(blocks::OAK_LOG);
        let axis_of = |face| {
            place_one(&world, &hit(IVec3::new(3, 64, 3), face), log)
                .unwrap()
                .1
                .get(Prop::Axis)
        };
        assert_eq!(axis_of(Face::Top), axis::Y);
        assert_eq!(axis_of(Face::East), axis::X);
        assert_eq!(axis_of(Face::North), axis::Z);
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
        let edit = place_one(&world, &hit(IVec3::new(3, 63, 3), Face::Top), STONE);
        assert_eq!(edit, Some((IVec3::new(3, 64, 3), STONE)));
    }

    #[test]
    fn only_blocks_that_collide_are_kept_out_of_the_player() {
        let world = world_with(&[]);
        let player = hitbox(DVec3::new(3.5, 64.0, 3.5));
        let below_feet = hit(IVec3::new(3, 63, 3), Face::Top);
        let put =
            |block, player: &Aabb| place_block(&world, blocks(), &below_feet, block, player, 0.0);
        assert_eq!(put(STONE, &player), None);
        // Flowers, torches and a layer of snow don't collide.
        assert!(put(DANDELION, &player).is_some());
        assert!(put(TORCH, &player).is_some());
        assert!(put(BlockState::of(blocks::SNOW), &player).is_some());
        // Standing next to the cell is fine.
        let beside = hitbox(DVec3::new(4.3, 64.0, 3.5));
        assert!(put(STONE, &beside).is_some());
    }

    #[test]
    fn clicking_tall_grass_replaces_it_and_water_can_be_built_into() {
        let world = world_with(&[
            (IVec3::new(5, 64, 5), SHORT_GRASS),
            (IVec3::new(6, 64, 5), WATER),
            (IVec3::new(7, 64, 5), STONE),
        ]);
        let edit = place_one(&world, &hit(IVec3::new(5, 64, 5), Face::Top), STONE);
        assert_eq!(edit, Some((IVec3::new(5, 64, 5), STONE)));
        let edit = place_one(&world, &hit(IVec3::new(7, 64, 5), Face::West), STONE);
        assert_eq!(edit, Some((IVec3::new(6, 64, 5), STONE)));
    }

    #[test]
    fn cannot_place_into_solid_blocks_or_outside_the_world() {
        let world = world_with(&[(IVec3::new(3, 64, 3), STONE)]);
        assert_eq!(
            place_one(&world, &hit(IVec3::new(3, 63, 3), Face::Top), STONE),
            None
        );
        let top = WORLD_HEIGHT as i32 - 1;
        let world = world_with(&[(IVec3::new(3, top, 3), STONE)]);
        assert_eq!(
            place_one(&world, &hit(IVec3::new(3, top, 3), Face::Top), STONE),
            None
        );
        // The top layer itself takes blocks.
        let world = world_with(&[(IVec3::new(3, top - 1, 3), STONE)]);
        assert_eq!(
            place_one(&world, &hit(IVec3::new(3, top - 1, 3), Face::Top), STONE),
            Some((IVec3::new(3, top, 3), STONE))
        );
        // Started inside a block: no face to place against.
        let inside = Hit {
            block: IVec3::new(3, 63, 3),
            face: None,
            point: DVec3::new(3.5, 63.5, 3.5),
        };
        assert_eq!(place_one(&world, &inside, STONE), None);
    }

    #[test]
    fn plants_need_ground() {
        let world = world_with(&[(IVec3::new(3, 65, 3), STONE)]);
        assert!(place_one(&world, &hit(IVec3::new(3, 63, 3), Face::Top), DANDELION).is_some());
        // Beside a floating block, over air.
        let on_side = hit(IVec3::new(3, 65, 3), Face::North);
        assert!(place_one(&world, &on_side, STONE).is_some());
        assert_eq!(place_one(&world, &on_side, DANDELION), None);
    }

    fn wall_torch(facing: Face) -> BlockState {
        BlockState::of(blocks::WALL_TORCH).with_face(Prop::Facing, facing)
    }

    #[test]
    fn torches_stand_on_floors_and_hang_on_walls() {
        let world = world_with(&[(IVec3::new(3, 64, 3), STONE)]);
        let put = |face| place_one(&world, &hit(IVec3::new(3, 64, 3), face), TORCH);
        assert_eq!(put(Face::Top), Some((IVec3::new(3, 65, 3), TORCH)));
        assert_eq!(
            put(Face::East),
            Some((IVec3::new(4, 64, 3), wall_torch(Face::East)))
        );
        assert_eq!(
            put(Face::West),
            Some((IVec3::new(2, 64, 3), wall_torch(Face::West)))
        );
        assert_eq!(
            put(Face::South),
            Some((IVec3::new(3, 64, 4), wall_torch(Face::South)))
        );
        assert_eq!(
            put(Face::North),
            Some((IVec3::new(3, 64, 2), wall_torch(Face::North)))
        );
        assert_eq!(put(Face::Bottom), None);
        // Glass is a full block to hang on in modern Minecraft, but not leaves.
        let leaves = world_with(&[(IVec3::new(3, 64, 3), BlockState::of(blocks::OAK_LEAVES))]);
        assert_eq!(
            place_one(&leaves, &hit(IVec3::new(3, 64, 3), Face::Top), TORCH),
            None
        );
        // A redstone torch hangs as a redstone wall torch, still lit.
        let redstone = BlockState::of(blocks::REDSTONE_TORCH);
        let (_, hung) =
            place_one(&world, &hit(IVec3::new(3, 64, 3), Face::East), redstone).unwrap();
        assert!(hung.is(blocks::REDSTONE_WALL_TORCH) && hung.bool(Prop::Lit));
    }

    #[test]
    fn breaking_a_wall_pops_what_hangs_on_it_but_not_others() {
        let ladder = BlockState::of(blocks::LADDER).with_face(Prop::Facing, Face::North);
        let world = world_with(&[
            (IVec3::new(3, 64, 3), STONE),
            (IVec3::new(4, 64, 3), wall_torch(Face::East)), // on the wall's east side
            (IVec3::new(3, 65, 3), TORCH),                  // on top of the wall
            (IVec3::new(3, 64, 4), TORCH),                  // on the floor beside it
            (IVec3::new(3, 64, 2), ladder),                 // on its north side
        ]);
        let edits = break_block(&world, blocks(), IVec3::new(3, 64, 3));
        let mut cells: Vec<IVec3> = edits.iter().map(|(at, _)| *at).collect();
        cells.sort_by_key(|c| (c.x, c.y, c.z));
        let expected = [
            IVec3::new(3, 64, 2),
            IVec3::new(3, 64, 3),
            IVec3::new(3, 65, 3),
            IVec3::new(4, 64, 3),
        ];
        assert_eq!(cells, expected);
    }
}
