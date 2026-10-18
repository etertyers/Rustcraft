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
        props::{Prop, attach_face, axis, bed_part, door_half, half, hinge, slab_type},
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
    /// Time of day in ticks (6000 is noon), and the game mode; `None`
    /// keeps a saved world's.
    pub time: Option<u64>,
    pub mode: Option<GameMode>,
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
        time: Some(6000),
        mode: Some(GameMode::Creative),
        overlay: false,
        stage: nothing,
    },
    Shot {
        name: "m14-block-states",
        about: "stairs (corners worked out from their neighbors), logs on each axis, slabs, doors, trapdoors, torches, levers and rails",
        world: Some(WorldKind::Flat),
        view: Some(([7.0, 68.0, -4.5], std::f32::consts::PI, -0.55)),
        time: Some(6000),
        mode: Some(GameMode::Creative),
        overlay: false,
        stage: block_states,
    },
    Shot {
        name: "m14-block-states-2",
        about: "fences, panes and walls joined up, gates, snow layers, cake, chest, beds, carpets, anvils and more",
        world: Some(WorldKind::Flat),
        view: Some(([7.0, 68.0, 6.0], std::f32::consts::PI, -0.6)),
        time: Some(6000),
        mode: Some(GameMode::Creative),
        overlay: false,
        stage: block_states,
    },
    Shot {
        name: "m14-v1-world-upgraded",
        about: "a v1.0 world (open with --world) after its one-time upgrade, as the player left it",
        world: None,
        view: None,
        time: None,
        mode: None,
        overlay: true,
        stage: nothing,
    },
    Shot {
        name: "m14-build-limit",
        about: "a pillar built from the ground to Y 255, the new top of the world",
        world: Some(WorldKind::Flat),
        view: Some(([-15.5, 222.0, 15.5], 0.785, 0.32)),
        time: Some(6000),
        mode: Some(GameMode::Creative),
        overlay: true,
        stage: build_limit,
    },
    Shot {
        name: "m14-slab-and-torch",
        about: "standing on a bottom slab (feet at Y 64.5), the crosshair on a torch's small outline",
        world: Some(WorldKind::Flat),
        view: Some(([0.5, 64.5, -1.5], 0.0, -1.07)),
        time: Some(6000),
        mode: Some(GameMode::Survival),
        overlay: true,
        stage: slab_and_torch,
    },
    Shot {
        name: "m14-falling-sand",
        about: "a sand pillar falling into the hole dug out under it",
        world: Some(WorldKind::Flat),
        view: Some(([3.5, 65.0, 0.5], -0.6, -0.15)),
        time: Some(6000),
        mode: Some(GameMode::Creative),
        overlay: false,
        stage: falling_sand,
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
    // Corners, worked out from their neighbors: stairs facing north with
    // west-facing stairs behind (an outer corner) and in front (inner).
    let north = facing(COBBLESTONE_STAIRS, Face::North);
    let west = facing(COBBLESTONE_STAIRS, Face::West);
    edits.extend([
        (at(11, 64, 0), north),
        (at(11, 64, -1), west),
        (at(14, 64, 0), north),
        (at(14, 64, 1), west),
    ]);
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
    // They join each other as they go down.
    for x in 0..3 {
        edits.push((at(x, 64, 8), BlockState::of(OAK_FENCE)));
        edits.push((at(x + 4, 64, 8), BlockState::of(GLASS_PANE)));
        edits.push((at(x + 8, 64, 8), BlockState::of(IRON_BARS)));
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
    // Row 14: carpets, walls joining in an L, anvils, red sand, concrete
    // powder.
    for (i, id) in [WHITE_CARPET, RED_CARPET, BLUE_CARPET, LIME_CARPET]
        .into_iter()
        .enumerate()
    {
        edits.push((at(i as i32, 64, 14), BlockState::of(id)));
    }
    for (x, z) in [(5, 14), (6, 14), (7, 14), (7, 15)] {
        edits.push((at(x, 64, z), BlockState::of(COBBLESTONE_WALL)));
    }
    edits.push((at(9, 64, 14), facing(ANVIL, Face::East)));
    edits.push((at(10, 64, 14), facing(DAMAGED_ANVIL, Face::North)));
    edits.push((at(12, 64, 14), BlockState::of(RED_SAND)));
    edits.push((at(13, 64, 14), BlockState::of(ORANGE_CONCRETE_POWDER)));
    game.set_blocks(&edits);
}

/// Stone from the ground to the top of the world, banded with wool every
/// 32 blocks (orange at 128, v1.0's limit) and glowstone at Y 255.
fn build_limit(game: &mut Game) {
    let mut edits = line(at(0, 64, 0), IVec3::Y, 192, BlockState::of(STONE));
    for y in (96..255).step_by(32) {
        let band = if y == 128 { ORANGE_WOOL } else { WHITE_WOOL };
        edits.push((at(0, y, 0), BlockState::of(band)));
    }
    edits.push((at(0, 255, 0), BlockState::of(GLOWSTONE)));
    game.set_blocks(&edits);
}

/// A path of bottom slabs to stand on, and a torch on the ground ahead.
fn slab_and_torch(game: &mut Game) {
    let slab = BlockState::of(OAK_SLAB);
    let mut edits = line(at(0, 64, 0), IVec3::NEG_Z, 3, slab);
    edits.push((at(0, 64, -3), BlockState::of(TORCH)));
    edits.push((at(1, 64, -3), BlockState::of(DANDELION)));
    game.set_blocks(&edits);
}

/// Sand stacked three high on the grass, then the ground under it dug out
/// three deep, a few ticks into its fall.
fn falling_sand(game: &mut Game) {
    let sand = BlockState::of(SAND);
    game.set_blocks(&line(at(0, 64, -4), IVec3::Y, 3, sand));
    game.set_blocks(&line(at(0, 63, -4), IVec3::NEG_Y, 3, BlockState::AIR));
    game.run_ticks(9);
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
