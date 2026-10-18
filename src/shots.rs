//! `--shot <name>`: scripted scenes for checking a milestone. The game
//! opens the scene's world (or the one named with `--world`), puts the
//! player where the scene says, waits until every chunk in view is loaded
//! and meshed, stages the scene (placing blocks, running ticks), and saves
//! a frame as a PNG (`docs/screenshots/<name>.png`, or `--out <file>`).
//! Game time stands still between those steps, so a scene looks the same
//! every run.

use glam::IVec3;

use crate::{
    app::WorldKind,
    game::Game,
    player::mode::GameMode,
    world::{
        Face,
        block::{BlockId, BlockState},
        blocks::*,
        props::{
            Prop, attach_face, axis, bed_part, door_half, half, hinge, slab_type, stairs_shape,
        },
    },
};

pub struct Shot {
    pub name: &'static str,
    /// What the screenshot shows.
    pub about: &'static str,
    /// The world, unless the command line names one.
    pub world: Option<WorldKind>,
    /// Feet, yaw and pitch to view the scene from; `None` keeps the
    /// player where the world left them.
    pub view: Option<([f64; 3], f32, f32)>,
    /// Time of day in ticks (6000 is noon).
    pub time: u64,
    pub mode: GameMode,
    /// Shows the F3 overlay.
    pub overlay: bool,
    /// Places the scene's blocks and runs its ticks, once the chunks
    /// around are ready.
    pub stage: fn(&mut Game),
}

/// Every scene, by milestone.
pub static SHOTS: &[Shot] = &[
    Shot {
        name: "m14-gallery",
        about: "every block in its default state and each variant",
        world: Some(WorldKind::Gallery),
        view: Some(([22.5, 64.0, -4.0], std::f32::consts::PI, -0.3)),
        time: 6000,
        mode: GameMode::Creative,
        overlay: false,
        stage: nothing,
    },
    Shot {
        name: "m14-block-states",
        about: "stairs, logs, slabs, doors, trapdoors, torches and more in modern block states",
        world: Some(WorldKind::Flat),
        view: Some(([7.0, 69.0, -4.5], std::f32::consts::PI, -0.62)),
        time: 6000,
        mode: GameMode::Creative,
        overlay: false,
        stage: block_states,
    },
    Shot {
        name: "dev-rows-closeup",
        about: "a close look at the rows behind the stairs",
        world: Some(WorldKind::Flat),
        view: Some(([6.5, 67.5, 1.0], std::f32::consts::PI, -0.6)),
        time: 6000,
        mode: GameMode::Creative,
        overlay: false,
        stage: block_states,
    },
    Shot {
        name: "dev-stairs-closeup",
        about: "a close look at stairs (for checking shading)",
        world: Some(WorldKind::Flat),
        view: Some(([11.5, 66.0, -2.5], std::f32::consts::PI - 0.3, -0.45)),
        time: 6000,
        mode: GameMode::Creative,
        overlay: false,
        stage: block_states,
    },
];

fn at(x: i32, y: i32, z: i32) -> IVec3 {
    IVec3::new(x, y, z)
}

fn facing(id: BlockId, face: Face) -> BlockState {
    BlockState::of(id).with_face(Prop::Facing, face)
}

const SIDES: [Face; 4] = [Face::North, Face::East, Face::South, Face::West];

/// Rows of blocks in their states on the flat world's grass (y 64).
fn block_states(game: &mut Game) {
    let mut edits = Vec::new();
    // Row 0: stairs facing each way, upside down, and the four corners.
    for (i, &face) in SIDES.iter().enumerate() {
        let stairs = facing(OAK_STAIRS, face);
        edits.push((at(i as i32, 64, 0), stairs));
        edits.push((at(i as i32 + 5, 64, 0), stairs.with(Prop::Half, half::TOP)));
    }
    let corners = [
        stairs_shape::OUTER_LEFT,
        stairs_shape::OUTER_RIGHT,
        stairs_shape::INNER_LEFT,
        stairs_shape::INNER_RIGHT,
    ];
    for (i, shape) in corners.into_iter().enumerate() {
        let stairs = facing(COBBLESTONE_STAIRS, Face::North).with(Prop::StairsShape, shape);
        edits.push((at(i as i32 + 10, 64, 0), stairs));
    }
    // Row 2: logs along each axis, slabs on the bottom, top and doubled.
    for (i, a) in [axis::Y, axis::X, axis::Z].into_iter().enumerate() {
        edits.push((
            at(i as i32, 64, 2),
            BlockState::of(OAK_LOG).with(Prop::Axis, a),
        ));
        edits.push((
            at(i as i32 + 3, 64, 2),
            BlockState::of(BIRCH_LOG).with(Prop::Axis, a),
        ));
    }
    for (i, t) in [slab_type::BOTTOM, slab_type::TOP, slab_type::DOUBLE]
        .into_iter()
        .enumerate()
    {
        edits.push((
            at(i as i32 + 7, 64, 2),
            BlockState::of(OAK_SLAB).with(Prop::SlabType, t),
        ));
        edits.push((
            at(i as i32 + 10, 64, 2),
            BlockState::of(STONE_BRICK_SLAB).with(Prop::SlabType, t),
        ));
    }
    // Row 4: doors closed, open on either hinge; trapdoors.
    let door = |x: i32, id, open: bool, side: u8| {
        let lower = facing(id, Face::North)
            .with_bool(Prop::Open, open)
            .with(Prop::Hinge, side);
        [
            (at(x, 64, 4), lower),
            (
                at(x, 65, 4),
                lower.with(Prop::DoubleBlockHalf, door_half::UPPER),
            ),
        ]
    };
    edits.extend(door(0, OAK_DOOR, false, hinge::LEFT));
    edits.extend(door(2, OAK_DOOR, true, hinge::LEFT));
    edits.extend(door(4, OAK_DOOR, true, hinge::RIGHT));
    edits.extend(door(6, IRON_DOOR, false, hinge::LEFT));
    let trapdoor = facing(OAK_TRAPDOOR, Face::North);
    edits.push((at(8, 64, 4), trapdoor));
    edits.push((at(9, 64, 4), trapdoor.with(Prop::Half, half::TOP)));
    edits.push((at(10, 64, 4), trapdoor.with_bool(Prop::Open, true)));
    edits.push((
        at(11, 64, 4),
        facing(OAK_TRAPDOOR, Face::East).with_bool(Prop::Open, true),
    ));
    // Row 6: a stone post with a torch on each side, floor torches,
    // levers, a button and plates, and rails.
    edits.push((at(1, 64, 6), BlockState::of(STONE)));
    for face in SIDES {
        let beside = at(1, 64, 6) + IVec3::from_array(face.normal());
        edits.push((beside, facing(WALL_TORCH, face)));
    }
    edits.push((at(4, 64, 6), BlockState::of(TORCH)));
    edits.push((at(5, 64, 6), BlockState::of(REDSTONE_TORCH)));
    edits.push((
        at(6, 64, 6),
        facing(LEVER, Face::North).with(Prop::AttachFace, attach_face::FLOOR),
    ));
    edits.push((at(7, 64, 6), BlockState::of(STONE)));
    edits.push((at(7, 64, 5), facing(STONE_BUTTON, Face::North)));
    edits.push((at(8, 64, 6), BlockState::of(STONE_PRESSURE_PLATE)));
    edits.push((at(9, 64, 6), BlockState::of(RAIL)));
    edits.push((at(10, 64, 6), BlockState::of(RAIL).with(Prop::RailShape, 6)));
    edits.push((
        at(11, 64, 6),
        BlockState::of(POWERED_RAIL).with_bool(Prop::Powered, true),
    ));
    // Row 8: fences, panes and bars joined in lines, a gate shut and open.
    for x in 0..3 {
        let mut fence = BlockState::of(OAK_FENCE);
        if x > 0 {
            fence = fence.with_bool(Prop::West, true);
        }
        if x < 2 {
            fence = fence.with_bool(Prop::East, true);
        }
        edits.push((at(x, 64, 8), fence));
        edits.push((at(x + 4, 64, 8), with_sides(GLASS_PANE, fence)));
        edits.push((at(x + 8, 64, 8), with_sides(IRON_BARS, fence)));
    }
    edits.push((at(12, 64, 8), facing(OAK_FENCE_GATE, Face::South)));
    edits.push((
        at(13, 64, 8),
        facing(OAK_FENCE_GATE, Face::South).with_bool(Prop::Open, true),
    ));
    // Row 10: snow layers, cake, chest, farmland, cactus, bed, and the rest.
    for layers in 1..=8 {
        edits.push((
            at(layers as i32 - 1, 64, 10),
            BlockState::of(SNOW).with_number(Prop::Layers, layers),
        ));
    }
    edits.push((
        at(8, 64, 10),
        BlockState::of(CAKE).with_number(Prop::Bites, 3),
    ));
    edits.push((at(9, 64, 10), facing(CHEST, Face::North)));
    edits.push((
        at(10, 63, 10),
        BlockState::of(FARMLAND).with_number(Prop::Moisture, 7),
    ));
    edits.push((
        at(10, 64, 10),
        BlockState::of(WHEAT).with_number(Prop::Age7, 7),
    ));
    edits.push((at(11, 64, 10), BlockState::of(CACTUS)));
    edits.push((at(12, 64, 10), facing(LADDER, Face::North)));
    edits.push((at(12, 64, 11), BlockState::of(STONE)));
    edits.push((at(13, 64, 10), BlockState::of(ENCHANTING_TABLE)));
    edits.push((
        at(14, 64, 10),
        facing(END_PORTAL_FRAME, Face::North).with_bool(Prop::Eye, true),
    ));
    let bed = facing(RED_BED, Face::East);
    edits.push((at(0, 64, 12), bed));
    edits.push((at(1, 64, 12), bed.with(Prop::BedPart, bed_part::HEAD)));
    edits.push((at(3, 64, 12), BlockState::of(BREWING_STAND)));
    edits.push((at(4, 64, 12), BlockState::of(CAULDRON)));
    edits.push((at(5, 64, 12), facing(REPEATER, Face::North)));
    edits.push((at(6, 64, 12), BlockState::of(DRAGON_EGG)));
    edits.push((
        at(7, 64, 12),
        BlockState::of(REDSTONE_WIRE).with_number(Prop::Power, 15),
    ));
    edits.push((at(8, 64, 12), BlockState::of(STONE)));
    edits.push((
        at(8, 64, 13),
        BlockState::of(VINE).with_bool(Prop::North, true),
    ));
    game.set_blocks(&edits);
}

/// `id` connected on the same sides as `like`.
fn with_sides(id: BlockId, like: BlockState) -> BlockState {
    SIDES.iter().fold(BlockState::of(id), |state, &face| {
        state.with_bool(Prop::side(face), like.bool(Prop::side(face)))
    })
}

pub fn find(name: &str) -> Option<&'static Shot> {
    SHOTS.iter().find(|shot| shot.name == name)
}

/// A scene that places nothing.
pub fn nothing(_: &mut Game) {}

/// `count` blocks in a line from `from` along `step`.
pub fn line(from: IVec3, step: IVec3, count: i32, block: BlockState) -> Vec<(IVec3, BlockState)> {
    (0..count).map(|i| (from + step * i, block)).collect()
}
