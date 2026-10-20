//! Each block state's shapes, as boxes in its cell (modern Minecraft's
//! `VoxelShape`s): the outline the crosshair targets and outlines, and the
//! collision boxes entities bump into. A torch is only as big as it looks
//! and has no collision; a slab is half a block; a fence's collision
//! reaches 1.5 blocks up so it can't be jumped. The tables are built once
//! for every state.

use std::{ops::Range, sync::LazyLock};

use super::{
    Face,
    block::{BlockState, Shape},
    blocks,
    props::{Prop, attach_face, half, slab_type, stairs_shape, thickness, wall_side},
};

/// A box in block units, relative to its cell's minimum corner. Collision
/// boxes may reach outside the cell (fences are 1.5 tall).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Cuboid {
    pub min: [f32; 3],
    pub max: [f32; 3],
}

impl Cuboid {
    pub const FULL: Cuboid = Cuboid {
        min: [0.0; 3],
        max: [1.0; 3],
    };

    /// A box given in pixels (sixteenths), like Minecraft's `Block.box`.
    pub const fn px(x0: f32, y0: f32, z0: f32, x1: f32, y1: f32, z1: f32) -> Self {
        Self {
            min: [x0 / 16.0, y0 / 16.0, z0 / 16.0],
            max: [x1 / 16.0, y1 / 16.0, z1 / 16.0],
        }
    }

    /// This box turned a quarter clockwise round the cell's vertical
    /// middle `turns` times (north → east), as block models rotate.
    pub fn rotated(self, turns: u8) -> Self {
        let mut b = self;
        for _ in 0..turns % 4 {
            // (x, z) → (1 - z, x)
            b = Cuboid {
                min: [1.0 - b.max[2], b.min[1], b.min[0]],
                max: [1.0 - b.min[2], b.max[1], b.max[0]],
            };
        }
        b
    }

    /// Flipped top to bottom within the cell.
    pub fn flipped(self) -> Self {
        Cuboid {
            min: [self.min[0], 1.0 - self.max[1], self.min[2]],
            max: [self.max[0], 1.0 - self.min[1], self.max[2]],
        }
    }

    /// The smallest box holding all of `boxes`.
    pub fn bounds(boxes: &[Cuboid]) -> Option<Cuboid> {
        let first = *boxes.first()?;
        Some(boxes.iter().fold(first, |a, b| Cuboid {
            min: [0, 1, 2].map(|i| a.min[i].min(b.min[i])),
            max: [0, 1, 2].map(|i| a.max[i].max(b.max[i])),
        }))
    }
}

/// Quarter turns clockwise from north to `face`.
fn turns(face: Face) -> u8 {
    match face {
        Face::East => 1,
        Face::South => 2,
        Face::West => 3,
        _ => 0,
    }
}

/// Every state's outline and collision boxes, by `BlockRegistry::state_index`.
pub struct Shapes {
    boxes: Vec<Cuboid>,
    outline: Vec<Range<u32>>,
    collision: Vec<Range<u32>>,
}

/// The shapes of every block state.
pub fn shapes() -> &'static Shapes {
    static SHAPES: LazyLock<Shapes> = LazyLock::new(|| {
        let registry = super::block::registry();
        let mut shapes = Shapes {
            boxes: Vec::new(),
            outline: Vec::with_capacity(registry.state_count()),
            collision: Vec::with_capacity(registry.state_count()),
        };
        for state in registry.states() {
            let outline = outline_of(state);
            let collision = collision_of(state, &outline);
            for (boxes, ranges) in [
                (outline, &mut shapes.outline),
                (collision, &mut shapes.collision),
            ] {
                let start = shapes.boxes.len() as u32;
                shapes.boxes.extend(boxes);
                ranges.push(start..shapes.boxes.len() as u32);
            }
        }
        shapes
    });
    &SHAPES
}

impl Shapes {
    /// The boxes the crosshair targets and the outline traces.
    pub fn outline(&self, state: BlockState) -> &[Cuboid] {
        let range = &self.outline[super::block::registry().state_index(state)];
        &self.boxes[range.start as usize..range.end as usize]
    }

    /// The boxes entities collide with (empty for flowers, torches...).
    pub fn collision(&self, state: BlockState) -> &[Cuboid] {
        let range = &self.collision[super::block::registry().state_index(state)];
        &self.boxes[range.start as usize..range.end as usize]
    }
}

/// A post with arms toward its connected sides (fences, panes, walls):
/// Minecraft's `CrossCollisionBlock` shapes with a post `post` pixels
/// either side of the middle, arms `arm` either side, and heights.
fn cross(
    post: f32,
    arm: f32,
    post_height: f32,
    arm_height: impl Fn(Face) -> Option<f32>,
    with_post: bool,
) -> Vec<Cuboid> {
    let (p0, p1) = (8.0 - post, 8.0 + post);
    let (a0, a1) = (8.0 - arm, 8.0 + arm);
    let mut boxes = Vec::new();
    if with_post {
        boxes.push(Cuboid::px(p0, 0.0, p0, p1, post_height, p1));
    }
    for face in [Face::North, Face::East, Face::South, Face::West] {
        let Some(h) = arm_height(face) else {
            continue;
        };
        boxes.push(match face {
            Face::North => Cuboid::px(a0, 0.0, 0.0, a1, h, a1),
            Face::South => Cuboid::px(a0, 0.0, a0, a1, h, 16.0),
            Face::West => Cuboid::px(0.0, 0.0, a0, a1, h, a1),
            _ => Cuboid::px(a0, 0.0, a0, 16.0, h, a1),
        });
    }
    boxes
}

/// Which sides of a fence or pane connect.
fn connected(state: BlockState, face: Face) -> bool {
    state.bool(Prop::side(face))
}

/// Whether a wall shows its post: when `up`, or with no sides to stand on.
fn wall_post(state: BlockState) -> bool {
    state.bool(Prop::Up)
        || [Face::North, Face::East, Face::South, Face::West]
            .iter()
            .all(|&face| state.get(Prop::wall_side(face)) == wall_side::NONE)
}

/// A wall side's height in pixels, `low` or `tall`, or `None` if open.
fn wall_height(state: BlockState, face: Face, low: f32, tall: f32) -> Option<f32> {
    match state.get(Prop::wall_side(face)) {
        wall_side::LOW => Some(low),
        wall_side::TALL => Some(tall),
        _ => None,
    }
}

/// The stair's upper part: quarters of the cell's top half (or bottom
/// half for upside-down stairs) by its facing and corner shape.
fn stairs(state: BlockState) -> Vec<Cuboid> {
    let facing = state.face(Prop::Facing);
    let shape = state.get(Prop::StairsShape);
    // Quarters as seen facing north: the back (north) half and its corners.
    let nw = Cuboid::px(0.0, 8.0, 0.0, 8.0, 16.0, 8.0);
    let ne = Cuboid::px(8.0, 8.0, 0.0, 16.0, 16.0, 8.0);
    let sw = Cuboid::px(0.0, 8.0, 8.0, 8.0, 16.0, 16.0);
    let se = Cuboid::px(8.0, 8.0, 8.0, 16.0, 16.0, 16.0);
    let quarters: Vec<Cuboid> = match shape {
        stairs_shape::OUTER_LEFT => vec![nw],
        stairs_shape::OUTER_RIGHT => vec![ne],
        stairs_shape::INNER_LEFT => vec![nw, ne, sw],
        stairs_shape::INNER_RIGHT => vec![nw, ne, se],
        _ => vec![Cuboid::px(0.0, 8.0, 0.0, 16.0, 16.0, 8.0)],
    };
    let mut boxes = vec![Cuboid::px(0.0, 0.0, 0.0, 16.0, 8.0, 16.0)];
    boxes.extend(quarters.into_iter().map(|q| q.rotated(turns(facing))));
    if state.get(Prop::Half) == half::TOP {
        boxes = boxes.into_iter().map(Cuboid::flipped).collect();
    }
    boxes
}

/// The outline of `state`: what the crosshair hits and the box traced
/// round it, following modern Minecraft's block shapes.
fn outline_of(state: BlockState) -> Vec<Cuboid> {
    let def = state.def();
    let facing = || state.face(Prop::Facing);
    let one = |b: Cuboid| vec![b];
    match def.shape {
        Shape::None | Shape::Liquid => Vec::new(),
        Shape::Cube => one(Cuboid::FULL),
        Shape::Cross => one(match state.kind() {
            blocks::POPPY => Cuboid::px(5.0, 0.0, 5.0, 11.0, 10.0, 11.0),
            blocks::BROWN_MUSHROOM | blocks::RED_MUSHROOM => {
                Cuboid::px(5.0, 0.0, 5.0, 11.0, 6.0, 11.0)
            }
            blocks::OAK_SAPLING => Cuboid::px(2.0, 0.0, 2.0, 14.0, 12.0, 14.0),
            blocks::TALL_GRASS
            | blocks::LARGE_FERN
            | blocks::SUNFLOWER
            | blocks::LILAC
            | blocks::ROSE_BUSH
            | blocks::PEONY => Cuboid::px(2.0, 0.0, 2.0, 14.0, 16.0, 14.0),
            blocks::CAVE_VINES | blocks::CAVE_VINES_PLANT => {
                Cuboid::px(1.0, 0.0, 1.0, 15.0, 16.0, 15.0)
            }
            blocks::POINTED_DRIPSTONE => match state.get(Prop::Thickness) {
                thickness::TIP_MERGE | thickness::TIP => {
                    Cuboid::px(5.0, 0.0, 5.0, 11.0, 11.0, 11.0)
                }
                thickness::FRUSTUM => Cuboid::px(4.0, 0.0, 4.0, 12.0, 16.0, 12.0),
                thickness::MIDDLE => Cuboid::px(3.0, 0.0, 3.0, 13.0, 16.0, 13.0),
                _ => Cuboid::px(2.0, 0.0, 2.0, 14.0, 16.0, 14.0),
            },
            blocks::SMALL_AMETHYST_BUD => Cuboid::px(4.0, 0.0, 4.0, 12.0, 3.0, 12.0),
            blocks::MEDIUM_AMETHYST_BUD => Cuboid::px(3.0, 0.0, 3.0, 13.0, 4.0, 13.0),
            blocks::LARGE_AMETHYST_BUD => Cuboid::px(3.0, 0.0, 3.0, 13.0, 5.0, 13.0),
            blocks::AMETHYST_CLUSTER => Cuboid::px(3.0, 0.0, 3.0, 13.0, 7.0, 13.0),
            blocks::SUGAR_CANE => Cuboid::px(2.0, 0.0, 2.0, 14.0, 16.0, 14.0),
            blocks::COBWEB => Cuboid::FULL,
            _ => Cuboid::px(2.0, 0.0, 2.0, 14.0, 13.0, 14.0),
        }),
        Shape::Crop => one(match state.id() {
            blocks::PUMPKIN_STEM | blocks::MELON_STEM => {
                let h = 2.0 + 2.0 * state.number(Prop::Age7) as f32;
                Cuboid::px(7.0, 0.0, 7.0, 9.0, h, 9.0)
            }
            blocks::NETHER_WART => {
                let h = [5.0, 8.0, 11.0, 14.0][state.number(Prop::Age3) as usize];
                Cuboid::px(0.0, 0.0, 0.0, 16.0, h, 16.0)
            }
            _ => {
                let h = 2.0 + 2.0 * state.number(Prop::Age7) as f32;
                Cuboid::px(0.0, 0.0, 0.0, 16.0, h, 16.0)
            }
        }),
        Shape::Slab => one(match state.get(Prop::SlabType) {
            slab_type::BOTTOM => Cuboid::px(0.0, 0.0, 0.0, 16.0, 8.0, 16.0),
            slab_type::TOP => Cuboid::px(0.0, 8.0, 0.0, 16.0, 16.0, 16.0),
            _ => Cuboid::FULL,
        }),
        Shape::Stairs => stairs(state),
        Shape::Fence => cross(
            2.0,
            2.0,
            16.0,
            |f| connected(state, f).then_some(16.0),
            true,
        ),
        Shape::Pane => cross(
            1.0,
            1.0,
            16.0,
            |f| connected(state, f).then_some(16.0),
            true,
        ),
        Shape::Wall => cross(
            4.0,
            3.0,
            16.0,
            |f| wall_height(state, f, 14.0, 16.0),
            wall_post(state),
        ),
        Shape::FenceGate => {
            let low = if state.bool(Prop::InWall) { 3.0 } else { 0.0 };
            let open = state.bool(Prop::Open);
            let along_x = matches!(facing(), Face::North | Face::South);
            if open {
                // The open gate's two halves stand against the posts.
                let side = Cuboid::px(0.0, 0.0, 6.0, 2.0, 16.0 - low, 10.0);
                let other = Cuboid::px(14.0, 0.0, 6.0, 16.0, 16.0 - low, 10.0);
                let boxes = vec![side, other];
                if along_x {
                    boxes
                } else {
                    boxes.into_iter().map(|b| b.rotated(1)).collect()
                }
            } else {
                one(if along_x {
                    Cuboid::px(0.0, 0.0, 6.0, 16.0, 16.0 - low, 10.0)
                } else {
                    Cuboid::px(6.0, 0.0, 0.0, 10.0, 16.0 - low, 16.0)
                })
            }
        }
        Shape::Door => {
            let (min, max) = super::block::door_box(state);
            one(Cuboid { min, max })
        }
        Shape::Trapdoor => one(if state.bool(Prop::Open) {
            Cuboid::px(0.0, 0.0, 13.0, 16.0, 16.0, 16.0).rotated(turns(facing()))
        } else if state.get(Prop::Half) == half::TOP {
            Cuboid::px(0.0, 13.0, 0.0, 16.0, 16.0, 16.0)
        } else {
            Cuboid::px(0.0, 0.0, 0.0, 16.0, 3.0, 16.0)
        }),
        Shape::Torch => one(if state.has(Prop::Facing) {
            // On a wall, leaning out from it: modern `WallTorchBlock`.
            Cuboid::px(5.5, 3.0, 11.0, 10.5, 13.0, 16.0).rotated(turns(facing()))
        } else {
            Cuboid::px(6.0, 0.0, 6.0, 10.0, 10.0, 10.0)
        }),
        Shape::Rail => {
            let shape = if state.has(Prop::RailShape) {
                state.get(Prop::RailShape)
            } else {
                state.get(Prop::StraightRailShape)
            };
            // Shapes 2–5 climb a block.
            let ascending = (2..=5).contains(&shape);
            one(Cuboid::px(
                0.0,
                0.0,
                0.0,
                16.0,
                if ascending { 8.0 } else { 2.0 },
                16.0,
            ))
        }
        Shape::Ladder => one(Cuboid::px(0.0, 0.0, 13.0, 16.0, 16.0, 16.0).rotated(turns(facing()))),
        Shape::Lever => one(attached(
            state,
            Cuboid::px(5.0, 4.0, 10.0, 11.0, 12.0, 16.0),
            Cuboid::px(5.0, 0.0, 4.0, 11.0, 6.0, 12.0),
        )),
        Shape::Button => {
            let depth = if state.bool(Prop::Powered) { 1.0 } else { 2.0 };
            one(attached(
                state,
                Cuboid::px(5.0, 6.0, 16.0 - depth, 11.0, 10.0, 16.0),
                Cuboid::px(5.0, 0.0, 6.0, 11.0, depth, 10.0),
            ))
        }
        Shape::PressurePlate => {
            let h = if state.bool(Prop::Powered) { 0.5 } else { 1.0 };
            one(Cuboid::px(1.0, 0.0, 1.0, 15.0, h, 15.0))
        }
        Shape::Repeater => one(Cuboid::px(0.0, 0.0, 0.0, 16.0, 2.0, 16.0)),
        Shape::RedstoneWire => one(Cuboid::px(3.0, 0.0, 3.0, 13.0, 1.0, 13.0)),
        Shape::SnowLayer => {
            let h = 2.0 * state.number(Prop::Layers) as f32;
            one(Cuboid::px(0.0, 0.0, 0.0, 16.0, h, 16.0))
        }
        Shape::Carpet => one(Cuboid::px(0.0, 0.0, 0.0, 16.0, 1.0, 16.0)),
        Shape::Vine => {
            let mut boxes: Vec<Cuboid> = [Face::North, Face::East, Face::South, Face::West]
                .into_iter()
                .filter(|&f| state.bool(Prop::side(f)))
                .map(|f| Cuboid::px(0.0, 0.0, 0.0, 16.0, 16.0, 1.0).rotated(turns(f)))
                .collect();
            if state.bool(Prop::Up) {
                boxes.push(Cuboid::px(0.0, 15.0, 0.0, 16.0, 16.0, 16.0));
            }
            if boxes.is_empty() {
                boxes.push(Cuboid::px(0.0, 15.0, 0.0, 16.0, 16.0, 16.0));
            }
            boxes
        }
        Shape::LilyPad => one(Cuboid::px(1.0, 0.0, 1.0, 15.0, 1.5, 15.0)),
        Shape::Cactus => one(Cuboid::px(1.0, 0.0, 1.0, 15.0, 16.0, 15.0)),
        Shape::Bed => one(Cuboid::px(0.0, 0.0, 0.0, 16.0, 9.0, 16.0)),
        Shape::Cake => {
            let bites = state.number(Prop::Bites) as f32;
            one(Cuboid::px(1.0 + 2.0 * bites, 0.0, 1.0, 15.0, 8.0, 15.0))
        }
        Shape::Chest => one(Cuboid::px(1.0, 0.0, 1.0, 15.0, 14.0, 15.0)),
        Shape::Sign => one(if state.has(Prop::Facing) {
            Cuboid::px(0.0, 4.5, 14.0, 16.0, 12.5, 16.0).rotated(turns(facing()))
        } else {
            Cuboid::px(4.0, 0.0, 4.0, 12.0, 16.0, 12.0)
        }),
        Shape::Farmland => one(Cuboid::px(0.0, 0.0, 0.0, 16.0, 15.0, 16.0)),
        Shape::PistonHead => {
            // A plate on the facing side and an arm back through the cell,
            // as for a head facing north.
            let plate = Cuboid::px(0.0, 0.0, 0.0, 16.0, 16.0, 4.0);
            let arm = Cuboid::px(6.0, 6.0, 4.0, 10.0, 10.0, 16.0);
            facing_all(state, vec![plate, arm])
        }
        Shape::Fire => one(Cuboid::px(0.0, 0.0, 0.0, 16.0, 1.0, 16.0)),
        Shape::BrewingStand => vec![
            Cuboid::px(1.0, 0.0, 1.0, 15.0, 2.0, 15.0),
            Cuboid::px(7.0, 0.0, 7.0, 9.0, 14.0, 9.0),
        ],
        Shape::Cauldron => one(Cuboid::FULL),
        Shape::Hopper => one(Cuboid::FULL),
        Shape::EnchantingTable => one(Cuboid::px(0.0, 0.0, 0.0, 16.0, 12.0, 16.0)),
        Shape::EndPortal => one(Cuboid::px(0.0, 6.0, 0.0, 16.0, 12.0, 16.0)),
        Shape::EndPortalFrame => {
            let mut boxes = vec![Cuboid::px(0.0, 0.0, 0.0, 16.0, 13.0, 16.0)];
            if state.bool(Prop::Eye) {
                boxes.push(Cuboid::px(4.0, 13.0, 4.0, 12.0, 16.0, 12.0));
            }
            boxes
        }
        Shape::DragonEgg => one(Cuboid::px(1.0, 0.0, 1.0, 15.0, 16.0, 15.0)),
        Shape::Bamboo => one(Cuboid::px(6.5, 0.0, 6.5, 9.5, 16.0, 9.5)),
        Shape::Lectern => vec![
            Cuboid::px(0.0, 0.0, 0.0, 16.0, 2.0, 16.0),
            Cuboid::px(4.0, 2.0, 4.0, 12.0, 13.0, 12.0),
            Cuboid::px(0.0, 13.0, 0.0, 16.0, 16.0, 16.0),
        ],
        Shape::Lantern => vec![
            Cuboid::px(5.0, 0.0, 5.0, 11.0, 7.0, 11.0),
            Cuboid::px(6.0, 7.0, 6.0, 10.0, 9.0, 10.0),
        ],
        Shape::Bell => {
            // Minecraft's floor bell: a beam across two posts along x,
            // turned for north/south.
            let boxes = vec![
                Cuboid::px(0.0, 0.0, 6.0, 2.0, 16.0, 10.0),
                Cuboid::px(14.0, 0.0, 6.0, 16.0, 16.0, 10.0),
                Cuboid::px(2.0, 13.0, 7.0, 14.0, 15.0, 9.0),
                Cuboid::px(5.0, 6.0, 5.0, 11.0, 13.0, 11.0),
                Cuboid::px(4.0, 4.0, 4.0, 12.0, 6.0, 12.0),
            ];
            match facing() {
                Face::North | Face::South => boxes.into_iter().map(|b| b.rotated(1)).collect(),
                _ => boxes,
            }
        }
        Shape::Grindstone => {
            // Minecraft's grindstone on the floor: a wheel between two
            // posts on legs, turning on the facing's axis.
            let boxes = vec![
                Cuboid::px(2.0, 0.0, 6.0, 4.0, 7.0, 10.0),
                Cuboid::px(12.0, 0.0, 6.0, 14.0, 7.0, 10.0),
                Cuboid::px(2.0, 7.0, 5.0, 4.0, 13.0, 11.0),
                Cuboid::px(12.0, 7.0, 5.0, 14.0, 13.0, 11.0),
                Cuboid::px(4.0, 4.0, 2.0, 12.0, 16.0, 14.0),
            ];
            match facing() {
                Face::North | Face::South => boxes.into_iter().map(|b| b.rotated(1)).collect(),
                _ => boxes,
            }
        }
        Shape::Anvil => {
            // Minecraft's anvil, lying along x; turned for north/south.
            let boxes = vec![
                Cuboid::px(2.0, 0.0, 2.0, 14.0, 4.0, 14.0),
                Cuboid::px(3.0, 4.0, 4.0, 13.0, 5.0, 12.0),
                Cuboid::px(4.0, 5.0, 6.0, 12.0, 10.0, 10.0),
                Cuboid::px(0.0, 10.0, 3.0, 16.0, 16.0, 13.0),
            ];
            match facing() {
                Face::North | Face::South => boxes.into_iter().map(|b| b.rotated(1)).collect(),
                _ => boxes,
            }
        }
    }
}

/// The boxes `state` is drawn as, for shapes drawn as boxes (mostly its
/// outline); `None` for shapes the mesher draws its own way: cubes, plants
/// and crops, torches, fire, fluids, beds, doors, cactus and flat things
/// (rails, ladders, vines, lily pads, redstone dust).
pub fn render_boxes(state: BlockState) -> Option<Vec<Cuboid>> {
    let def = state.def();
    let facing = || state.face(Prop::Facing);
    Some(match def.shape {
        Shape::None
        | Shape::Cube
        | Shape::Cross
        | Shape::Crop
        | Shape::Bed
        | Shape::Door
        | Shape::Torch
        | Shape::Fire
        | Shape::Liquid
        | Shape::Rail
        | Shape::Ladder
        | Shape::Vine
        | Shape::LilyPad
        | Shape::RedstoneWire
        | Shape::Cactus => return None,
        // A post with two rails to each neighbor it joins.
        Shape::Fence => {
            let mut boxes = vec![Cuboid::px(6.0, 0.0, 6.0, 10.0, 16.0, 10.0)];
            for face in [Face::North, Face::East, Face::South, Face::West] {
                if connected(state, face) {
                    boxes.push(Cuboid::px(7.0, 12.0, 0.0, 9.0, 15.0, 6.0).rotated(turns(face)));
                    boxes.push(Cuboid::px(7.0, 6.0, 0.0, 9.0, 9.0, 6.0).rotated(turns(face)));
                }
            }
            boxes
        }
        // Two posts and two bars, swung open to one side.
        Shape::FenceGate => {
            let low = if state.bool(Prop::InWall) { 3.0 } else { 0.0 };
            let mut boxes = vec![
                Cuboid::px(0.0, 5.0 - low, 7.0, 2.0, 16.0 - low, 9.0),
                Cuboid::px(14.0, 5.0 - low, 7.0, 16.0, 16.0 - low, 9.0),
            ];
            if state.bool(Prop::Open) {
                for x in [0.0, 14.0] {
                    boxes.push(Cuboid::px(x, 6.0 - low, 9.0, x + 2.0, 9.0 - low, 15.0));
                    boxes.push(Cuboid::px(x, 12.0 - low, 9.0, x + 2.0, 15.0 - low, 15.0));
                }
            } else {
                boxes.push(Cuboid::px(2.0, 6.0 - low, 7.0, 14.0, 9.0 - low, 9.0));
                boxes.push(Cuboid::px(2.0, 12.0 - low, 7.0, 14.0, 15.0 - low, 9.0));
            }
            // The boxes above are for a gate facing south, opening to the
            // south (away from the player who placed it); turn them.
            let t = (turns(facing()) + 2) % 4;
            boxes.into_iter().map(|b| b.rotated(t)).collect()
        }
        // The base and its handle.
        Shape::Lever => {
            let base = attached(
                state,
                Cuboid::px(5.0, 4.0, 13.0, 11.0, 12.0, 16.0),
                Cuboid::px(5.0, 0.0, 4.0, 11.0, 3.0, 12.0),
            );
            let handle = attached(
                state,
                Cuboid::px(7.0, 7.0, 6.0, 9.0, 9.0, 13.0),
                Cuboid::px(7.0, 3.0, 7.0, 9.0, 13.0, 9.0),
            );
            vec![base, handle]
        }
        // The slab with its two torches.
        Shape::Repeater => {
            let mut boxes = vec![Cuboid::px(0.0, 0.0, 0.0, 16.0, 2.0, 16.0)];
            for z in [2.0, 8.0] {
                boxes.push(Cuboid::px(7.0, 2.0, z, 9.0, 7.0, z + 2.0).rotated(turns(facing())));
            }
            boxes
        }
        Shape::Cauldron | Shape::Hopper => collision_of(state, &[]),
        _ => outline_of(state),
    })
}

/// A lever or button's box: on a wall (`wall`, as for one facing north,
/// on the south face of the block behind it), on the floor (`floor`, as
/// for one facing north) or the ceiling (the floor box flipped).
fn attached(state: BlockState, wall: Cuboid, floor: Cuboid) -> Cuboid {
    let t = turns(state.face(Prop::Facing));
    match state.get(Prop::AttachFace) {
        attach_face::WALL => wall.rotated(t),
        attach_face::CEILING => floor.rotated(t).flipped(),
        _ => floor.rotated(t),
    }
}

/// Boxes drawn for a block facing north, turned to face any of six ways.
fn facing_all(state: BlockState, boxes: Vec<Cuboid>) -> Vec<Cuboid> {
    let face = state.face(Prop::FacingAll);
    boxes
        .into_iter()
        .map(|b| match face {
            // Facing up: the north side goes to the top.
            Face::Top => Cuboid {
                min: [b.min[0], 1.0 - b.max[2], b.min[1]],
                max: [b.max[0], 1.0 - b.min[2], b.max[1]],
            },
            Face::Bottom => Cuboid {
                min: [b.min[0], b.min[2], b.min[1]],
                max: [b.max[0], b.max[2], b.max[1]],
            },
            horizontal => b.rotated(turns(horizontal)),
        })
        .collect()
}

/// The collision boxes of `state` (`outline` is its outline): nothing for
/// blocks entities walk through, taller than the outline for fences, gates
/// and walls, lower for snow layers and cactus, the outline otherwise.
fn collision_of(state: BlockState, outline: &[Cuboid]) -> Vec<Cuboid> {
    let def = state.def();
    if !def.solid {
        return Vec::new();
    }
    match def.shape {
        Shape::Fence => cross(
            2.0,
            2.0,
            24.0,
            |f| connected(state, f).then_some(24.0),
            true,
        ),
        Shape::Wall => cross(
            4.0,
            3.0,
            24.0,
            |f| wall_height(state, f, 24.0, 24.0),
            wall_post(state),
        ),
        Shape::FenceGate => {
            if state.bool(Prop::Open) {
                Vec::new()
            } else {
                let along_x = matches!(state.face(Prop::Facing), Face::North | Face::South);
                vec![if along_x {
                    Cuboid::px(0.0, 0.0, 6.0, 16.0, 24.0, 10.0)
                } else {
                    Cuboid::px(6.0, 0.0, 0.0, 10.0, 24.0, 16.0)
                }]
            }
        }
        // A snow layer collides one layer lower than it looks.
        Shape::SnowLayer => {
            let h = 2.0 * (state.number(Prop::Layers) as f32 - 1.0);
            if h > 0.0 {
                vec![Cuboid::px(0.0, 0.0, 0.0, 16.0, h, 16.0)]
            } else {
                Vec::new()
            }
        }
        Shape::Cactus => vec![Cuboid::px(1.0, 0.0, 1.0, 15.0, 15.0, 15.0)],
        // A funnel: a bowl of walls over a narrow neck.
        Shape::Hopper => vec![
            Cuboid::px(0.0, 10.0, 0.0, 16.0, 11.0, 16.0),
            Cuboid::px(0.0, 11.0, 0.0, 16.0, 16.0, 2.0),
            Cuboid::px(0.0, 11.0, 14.0, 16.0, 16.0, 16.0),
            Cuboid::px(0.0, 11.0, 2.0, 2.0, 16.0, 14.0),
            Cuboid::px(14.0, 11.0, 2.0, 16.0, 16.0, 14.0),
            Cuboid::px(4.0, 4.0, 4.0, 12.0, 10.0, 12.0),
            Cuboid::px(6.0, 0.0, 6.0, 10.0, 4.0, 10.0),
        ],
        // Hollow: walls round a floor, so things can sit inside.
        Shape::Cauldron => vec![
            Cuboid::px(0.0, 0.0, 0.0, 16.0, 4.0, 16.0),
            Cuboid::px(0.0, 4.0, 0.0, 16.0, 16.0, 2.0),
            Cuboid::px(0.0, 4.0, 14.0, 16.0, 16.0, 16.0),
            Cuboid::px(0.0, 4.0, 2.0, 2.0, 16.0, 14.0),
            Cuboid::px(14.0, 4.0, 2.0, 16.0, 16.0, 14.0),
        ],
        _ if state.is(blocks::SOUL_SAND) => vec![Cuboid::px(0.0, 0.0, 0.0, 16.0, 14.0, 16.0)],
        _ => outline.to_vec(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::props::{hinge, stairs_shape};

    fn outline(state: BlockState) -> Vec<Cuboid> {
        shapes().outline(state).to_vec()
    }

    fn collision(state: BlockState) -> Vec<Cuboid> {
        shapes().collision(state).to_vec()
    }

    fn top(boxes: &[Cuboid]) -> f32 {
        Cuboid::bounds(boxes).map_or(0.0, |b| b.max[1])
    }

    #[test]
    fn plants_and_torches_are_small_and_walk_through() {
        for id in [
            blocks::TORCH,
            blocks::DANDELION,
            blocks::OAK_SAPLING,
            blocks::SHORT_GRASS,
        ] {
            let state = BlockState::of(id);
            assert!(collision(state).is_empty(), "{}", id.name());
            let b = Cuboid::bounds(&outline(state)).unwrap();
            assert!(b.max[1] < 1.0 && b.min[0] > 0.0, "{}: {b:?}", id.name());
        }
        let torch = outline(BlockState::of(blocks::TORCH))[0];
        assert_eq!(torch, Cuboid::px(6.0, 0.0, 6.0, 10.0, 10.0, 10.0));
        // A wall torch leans out from its wall: facing east, from the west side.
        let wall = BlockState::of(blocks::WALL_TORCH).with_face(Prop::Facing, Face::East);
        assert_eq!(outline(wall)[0], Cuboid::px(0.0, 3.0, 5.5, 5.0, 13.0, 10.5));
    }

    #[test]
    fn slabs_are_half_blocks_on_the_bottom_or_top() {
        let slab = BlockState::of(blocks::OAK_SLAB);
        assert_eq!(
            collision(slab),
            [Cuboid::px(0.0, 0.0, 0.0, 16.0, 8.0, 16.0)]
        );
        let top_slab = slab.with(Prop::SlabType, slab_type::TOP);
        assert_eq!(
            collision(top_slab),
            [Cuboid::px(0.0, 8.0, 0.0, 16.0, 16.0, 16.0)]
        );
        let double = slab.with(Prop::SlabType, slab_type::DOUBLE);
        assert_eq!(collision(double), [Cuboid::FULL]);
    }

    #[test]
    fn stairs_have_a_step_toward_their_facing_and_corners() {
        let stairs = BlockState::of(blocks::OAK_STAIRS).with_face(Prop::Facing, Face::East);
        let boxes = outline(stairs);
        assert_eq!(boxes.len(), 2);
        assert_eq!(boxes[0], Cuboid::px(0.0, 0.0, 0.0, 16.0, 8.0, 16.0));
        assert_eq!(
            boxes[1],
            Cuboid::px(8.0, 8.0, 0.0, 16.0, 16.0, 16.0),
            "the east half"
        );
        // Upside down, the slab is on top and the step below.
        let flipped = stairs.with(Prop::Half, half::TOP);
        assert_eq!(
            outline(flipped)[0],
            Cuboid::px(0.0, 8.0, 0.0, 16.0, 16.0, 16.0)
        );
        // An outer corner keeps one quarter, an inner one three.
        let outer = stairs.with(Prop::StairsShape, stairs_shape::OUTER_LEFT);
        assert_eq!(outline(outer).len(), 2);
        let inner = stairs.with(Prop::StairsShape, stairs_shape::INNER_RIGHT);
        assert_eq!(outline(inner).len(), 4);
    }

    #[test]
    fn fences_and_walls_are_1_5_blocks_tall_to_bump_into() {
        let fence = BlockState::of(blocks::OAK_FENCE).with_bool(Prop::North, true);
        assert_eq!(top(&outline(fence)), 1.0);
        assert_eq!(top(&collision(fence)), 1.5);
        assert_eq!(collision(fence).len(), 2, "the post and its north arm");
        let pane = BlockState::of(blocks::GLASS_PANE);
        assert_eq!(outline(pane), [Cuboid::px(7.0, 0.0, 7.0, 9.0, 16.0, 9.0)]);
    }

    #[test]
    fn doors_trapdoors_and_ladders_are_thin() {
        let door = BlockState::of(blocks::OAK_DOOR).with_face(Prop::Facing, Face::North);
        let t = 3.0 / 16.0;
        assert_eq!(collision(door)[0].min[2], 1.0 - t);
        let right = door
            .with_bool(Prop::Open, true)
            .with(Prop::Hinge, hinge::RIGHT);
        assert_eq!(collision(right)[0].min[0], 1.0 - t);
        let trapdoor = BlockState::of(blocks::OAK_TRAPDOOR);
        assert_eq!(top(&collision(trapdoor)), 3.0 / 16.0);
        let ladder = BlockState::of(blocks::LADDER).with_face(Prop::Facing, Face::North);
        assert_eq!(
            collision(ladder),
            [Cuboid::px(0.0, 0.0, 13.0, 16.0, 16.0, 16.0)]
        );
    }

    #[test]
    fn snow_cakes_chests_and_farmland_follow_minecraft() {
        let snow = |layers| BlockState::of(blocks::SNOW).with_number(Prop::Layers, layers);
        assert!(collision(snow(1)).is_empty(), "one layer is walked through");
        assert_eq!(top(&collision(snow(4))), 6.0 / 16.0);
        assert_eq!(top(&outline(snow(4))), 8.0 / 16.0);
        let cake = BlockState::of(blocks::CAKE).with_number(Prop::Bites, 3);
        assert_eq!(outline(cake)[0].min[0], 7.0 / 16.0);
        assert_eq!(top(&outline(BlockState::of(blocks::CHEST))), 14.0 / 16.0);
        assert_eq!(
            top(&collision(BlockState::of(blocks::FARMLAND))),
            15.0 / 16.0
        );
        assert_eq!(
            top(&collision(BlockState::of(blocks::SOUL_SAND))),
            14.0 / 16.0
        );
        assert_eq!(top(&outline(BlockState::of(blocks::SOUL_SAND))), 1.0);
    }

    #[test]
    fn rotation_turns_north_into_east() {
        let north = Cuboid::px(0.0, 0.0, 0.0, 16.0, 16.0, 3.0);
        assert_eq!(
            north.rotated(1),
            Cuboid::px(13.0, 0.0, 0.0, 16.0, 16.0, 16.0)
        );
        assert_eq!(
            north.rotated(2),
            Cuboid::px(0.0, 0.0, 13.0, 16.0, 16.0, 16.0)
        );
        assert_eq!(north.rotated(4), north);
    }

    #[test]
    fn every_state_has_shapes() {
        let registry = crate::world::block::registry();
        for state in registry.states() {
            let def = state.def();
            let outline = shapes().outline(state);
            assert_eq!(
                outline.is_empty(),
                !def.is_targetable(),
                "{state} outline {outline:?}"
            );
            if !def.solid {
                assert!(shapes().collision(state).is_empty(), "{state}");
            }
        }
    }
}
