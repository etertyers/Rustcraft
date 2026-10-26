//! Block definitions loaded from `assets/data/blocks.ron`, and block states.
//!
//! Blocks are named as in modern Java Edition (`minecraft:oak_log`) and
//! numbered by their place in the file; the numbers are runtime IDs only
//! (saves store names). A `BlockState` is a block plus a value for each of
//! its properties (`facing=east`, `half=top`...), packed so that all zeros
//! is the block's default state.

use std::{fmt, sync::LazyLock};

use anyhow::{Result, bail};
use glam::IVec3;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::{
    Face,
    biome::rgb,
    harvest::{Drops, Harvest},
    props::Prop,
};
use crate::inventory::item::ToolKind;

/// A block's runtime ID: its index in `blocks.ron`. The named constants
/// are in `world::blocks`.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BlockId(pub(crate) u16);

impl BlockId {
    pub const fn index(self) -> usize {
        self.0 as usize
    }

    /// The block's name without the namespace, e.g. `oak_log`.
    pub fn name(self) -> &'static str {
        super::blocks::NAMES
            .get(self.index())
            .copied()
            .unwrap_or("unknown")
    }
}

/// Block IDs are written by name (`"oak_log"`, or `"minecraft:oak_log"`).
impl<'de> Deserialize<'de> for BlockId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let name = String::deserialize(deserializer)?;
        super::blocks::by_name(strip_namespace(&name))
            .ok_or_else(|| serde::de::Error::custom(format!("unknown block {name:?}")))
    }
}

impl Serialize for BlockId {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(&format_args!("{NAMESPACE}:{}", self.name()))
    }
}

/// The namespace every block and item is in.
pub const NAMESPACE: &str = "minecraft";

/// `name` without a leading `minecraft:`.
pub fn strip_namespace(name: &str) -> &str {
    name.strip_prefix("minecraft:").unwrap_or(name)
}

/// A block in the world: its ID plus a value for each of its properties.
/// The property values are packed into `props` as mixed-radix digits, each
/// counted from the block's default value, so `props == 0` is the default
/// state.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BlockState {
    id: BlockId,
    props: u16,
}

impl BlockState {
    pub const AIR: BlockState = BlockState::of(super::blocks::AIR);

    /// The default state of block `id`.
    pub const fn of(id: BlockId) -> Self {
        Self { id, props: 0 }
    }

    pub const fn id(self) -> BlockId {
        self.id
    }

    pub const fn is_air(self) -> bool {
        self.id.0 == super::blocks::AIR.0
    }

    /// Whether this is a state of block `id`.
    pub const fn is(self, id: BlockId) -> bool {
        self.id.0 == id.0
    }

    /// The packed property values, for tables indexed by state.
    pub const fn props(self) -> u16 {
        self.props
    }

    pub fn def(self) -> &'static BlockDef {
        registry().def(self.id)
    }

    /// The block whose behavior this one shares (`BlockDef::acts_as`), or
    /// its own: what code that names blocks matches on.
    pub fn kind(self) -> BlockId {
        self.def().acts_as.unwrap_or(self.id)
    }

    /// Whether the block has property `prop`.
    pub fn has(self, prop: Prop) -> bool {
        registry().layout(self.id).slot(prop).is_some()
    }

    /// The index of `prop`'s value (into `prop.values()`). Blocks without
    /// the property read as its default.
    pub fn get(self, prop: Prop) -> u8 {
        let layout = registry().layout(self.id);
        match layout.slot(prop) {
            Some(slot) => layout.read(self.props, slot),
            None => prop.default_index(),
        }
    }

    /// This state with `prop` set to value `index`; unchanged if the block
    /// doesn't have the property.
    pub fn with(self, prop: Prop, index: u8) -> Self {
        let layout = registry().layout(self.id);
        match layout.slot(prop) {
            Some(slot) => Self {
                props: layout.write(self.props, slot, index),
                ..self
            },
            None => self,
        }
    }

    pub fn bool(self, prop: Prop) -> bool {
        self.get(prop) == 1
    }

    pub fn with_bool(self, prop: Prop, value: bool) -> Self {
        self.with(prop, value as u8)
    }

    /// A numeric property's value (`age`, `layers`...).
    pub fn number(self, prop: Prop) -> u8 {
        prop.number(self.get(prop))
    }

    /// This state with a numeric property set, clamped to its range.
    pub fn with_number(self, prop: Prop, number: u8) -> Self {
        let last = prop.values().len() as u8 - 1;
        let index = prop
            .index_of_number(number)
            .unwrap_or(if number == 0 { 0 } else { last });
        self.with(prop, index)
    }

    /// The direction a `facing` property points.
    pub fn face(self, prop: Prop) -> Face {
        prop.face(self.get(prop))
    }

    /// This state facing `face`; unchanged if `prop` can't point that way.
    pub fn with_face(self, prop: Prop, face: Face) -> Self {
        match prop.index_of_face(face) {
            Some(index) => self.with(prop, index),
            None => self,
        }
    }

    /// This state turned `turns` quarter turns clockwise seen from above,
    /// as a structure piece is: facings, axes, rail shapes and the sides a
    /// fence or wall joins turn with it.
    pub fn rotated(self, turns: u8) -> Self {
        let turns = turns % 4;
        if turns == 0 {
            return self;
        }
        let mut state = self;
        for &prop in &registry().layout(self.id).props {
            state = match prop {
                Prop::Facing | Prop::FacingAll => {
                    let mut face = self.face(prop);
                    for _ in 0..turns {
                        face = face.rotate_cw();
                    }
                    state.with_face(prop, face)
                }
                Prop::Axis | Prop::HorizontalAxis if turns % 2 == 1 => {
                    let swapped = match prop.values()[self.get(prop) as usize] {
                        "x" => "z",
                        "z" => "x",
                        other => other,
                    };
                    state.with(prop, prop.index_of(swapped).unwrap_or(self.get(prop)))
                }
                Prop::RailShape | Prop::StraightRailShape => {
                    let mut name = prop.values()[self.get(prop) as usize];
                    for _ in 0..turns {
                        name = rail_turned(name);
                    }
                    state.with(prop, prop.index_of(name).unwrap_or(self.get(prop)))
                }
                _ => state,
            };
        }
        // Sides that join: the value on each side moves to the next side
        // clockwise.
        for sides in [
            [Prop::North, Prop::East, Prop::South, Prop::West],
            [
                Prop::WallNorth,
                Prop::WallEast,
                Prop::WallSouth,
                Prop::WallWest,
            ],
        ] {
            if !self.has(sides[0]) {
                continue;
            }
            for (i, &side) in sides.iter().enumerate() {
                let to = sides[(i + turns as usize) % 4];
                state = state.with(to, self.get(side));
            }
        }
        state
    }

    /// Whether this state meets `condition`.
    pub fn matches(self, condition: &Condition) -> bool {
        let layout = registry().layout(self.id);
        condition
            .0
            .iter()
            .all(|&(slot, allowed)| allowed & (1 << layout.read(self.props, slot)) != 0)
    }

    /// Reads a state written as by `Display`: `minecraft:oak_stairs[facing=east]`.
    /// The namespace is optional and properties left out keep their default.
    pub fn parse(text: &str) -> Result<Self> {
        let (name, props) = match text.split_once('[') {
            Some((name, rest)) => {
                let Some(props) = rest.strip_suffix(']') else {
                    bail!("block state {text:?} is missing its ']'");
                };
                (name, props)
            }
            None => (text, ""),
        };
        let Some(id) = super::blocks::by_name(strip_namespace(name)) else {
            bail!("unknown block {name:?}");
        };
        let mut state = Self::of(id);
        let layout = registry().layout(id);
        for pair in props.split(',').filter(|pair| !pair.is_empty()) {
            let Some((key, value)) = pair.split_once('=') else {
                bail!("bad property {pair:?} in {text:?}");
            };
            let Some(slot) = layout.slot_named(key) else {
                bail!("{name} has no property {key:?}");
            };
            let prop = layout.props[slot];
            let Some(index) = prop.index_of(value) else {
                bail!("{name}'s {key} can't be {value:?}");
            };
            state.props = layout.write(state.props, slot, index);
        }
        Ok(state)
    }
}

/// `minecraft:name[prop=value,...]`, with every property, in the order
/// `blocks.ron` lists them.
impl fmt::Display for BlockState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{NAMESPACE}:{}", self.id.name())?;
        let layout = registry().layout(self.id);
        if layout.props.is_empty() {
            return Ok(());
        }
        f.write_str("[")?;
        for (slot, prop) in layout.props.iter().enumerate() {
            if slot > 0 {
                f.write_str(",")?;
            }
            let value = prop.values()[layout.read(self.props, slot) as usize];
            write!(f, "{}={value}", prop.name())?;
        }
        f.write_str("]")
    }
}

/// States are saved as their `Display` text.
impl Serialize for BlockState {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for BlockState {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Self::parse(&text).map_err(serde::de::Error::custom)
    }
}

/// A rail shape a quarter turn clockwise.
fn rail_turned(shape: &'static str) -> &'static str {
    match shape {
        "north_south" => "east_west",
        "east_west" => "north_south",
        "ascending_north" => "ascending_east",
        "ascending_east" => "ascending_south",
        "ascending_south" => "ascending_west",
        "ascending_west" => "ascending_north",
        "north_east" => "south_east",
        "south_east" => "south_west",
        "south_west" => "north_west",
        "north_west" => "north_east",
        other => other,
    }
}

/// How a block's properties are packed into `BlockState::props`.
#[derive(Debug, Clone, Default)]
pub struct Layout {
    pub props: Vec<Prop>,
    /// Per property: its default value's index and the number of values.
    defaults: Vec<u8>,
    sizes: Vec<u8>,
    strides: Vec<u16>,
    /// The number of states (the product of `sizes`).
    pub states: u32,
}

impl Layout {
    fn new(props: Vec<Prop>, defaults: Vec<u8>) -> Self {
        let sizes: Vec<u8> = props.iter().map(|p| p.values().len() as u8).collect();
        let mut strides = Vec::with_capacity(props.len());
        let mut states = 1u32;
        for &size in &sizes {
            strides.push(states as u16);
            states *= size as u32;
        }
        Self {
            props,
            defaults,
            sizes,
            strides,
            states,
        }
    }

    pub fn slot(&self, prop: Prop) -> Option<usize> {
        self.props.iter().position(|&p| p == prop)
    }

    fn slot_named(&self, name: &str) -> Option<usize> {
        self.props.iter().position(|p| p.name() == name)
    }

    fn read(&self, props: u16, slot: usize) -> u8 {
        let size = self.sizes[slot] as u16;
        let digit = (props / self.strides[slot]) % size;
        ((digit + self.defaults[slot] as u16) % size) as u8
    }

    fn write(&self, props: u16, slot: usize, index: u8) -> u16 {
        let size = self.sizes[slot] as u16;
        let stride = self.strides[slot];
        let old = (props / stride) % size;
        let digit = (index as u16 % size + size - self.defaults[slot] as u16) % size;
        props - old * stride + digit * stride
    }

    /// The packed props of every state, in order.
    fn all_states(&self) -> impl Iterator<Item = u16> + use<> {
        0..self.states as u16
    }
}

/// A test on a block's state, written `facing=east,half=top` (every pair
/// must hold) with `|` between alternative values (`shape=north_east|south_east`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Condition(Vec<(usize, u32)>);

impl Condition {
    pub fn parse(text: &str, layout: &Layout, block: &str) -> Result<Self> {
        let mut tests = Vec::new();
        for pair in text.split(',').filter(|pair| !pair.is_empty()) {
            let Some((key, values)) = pair.split_once('=') else {
                bail!("bad condition {pair:?} on {block}");
            };
            let Some(slot) = layout.slot_named(key) else {
                bail!("{block} has no property {key:?}");
            };
            let prop = layout.props[slot];
            let mut allowed = 0u32;
            for value in values.split('|') {
                let Some(index) = prop.index_of(value) else {
                    bail!("{block}'s {key} can't be {value:?}");
                };
                allowed |= 1 << index;
            }
            tests.push((slot, allowed));
        }
        Ok(Self(tests))
    }
}

/// A texture name from `blocks.ron` as a resource location: bare names
/// (`stone`) are under `block/`; names with a folder (from pack models)
/// are kept.
pub fn texture_location(name: &str) -> std::borrow::Cow<'_, str> {
    if name.contains('/') {
        name.into()
    } else {
        format!("block/{name}").into()
    }
}

/// Texture names per face. The most specific field wins:
/// a direction (`north`, …), then `top`/`bottom`, then `end` (top and bottom)
/// or `side` (the four sides), then `all`.
#[derive(Debug, Default, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Textures {
    pub all: Option<String>,
    pub end: Option<String>,
    pub side: Option<String>,
    pub top: Option<String>,
    pub bottom: Option<String>,
    pub north: Option<String>,
    pub south: Option<String>,
    pub east: Option<String>,
    pub west: Option<String>,
}

impl Textures {
    pub fn get(&self, face: Face) -> Option<&str> {
        let (specific, group) = match face {
            Face::Top => (&self.top, &self.end),
            Face::Bottom => (&self.bottom, &self.end),
            Face::North => (&self.north, &self.side),
            Face::South => (&self.south, &self.side),
            Face::East => (&self.east, &self.side),
            Face::West => (&self.west, &self.side),
        };
        specific
            .as_ref()
            .or(group.as_ref())
            .or(self.all.as_ref())
            .map(String::as_str)
    }

    /// These textures with every field `over` sets replaced.
    fn overlaid(&self, over: &Textures) -> Textures {
        let pick = |base: &Option<String>, over: &Option<String>| over.clone().or(base.clone());
        Textures {
            all: pick(&self.all, &over.all),
            end: pick(&self.end, &over.end),
            side: pick(&self.side, &over.side),
            top: pick(&self.top, &over.top),
            bottom: pick(&self.bottom, &over.bottom),
            north: pick(&self.north, &over.north),
            south: pick(&self.south, &over.south),
            east: pick(&self.east, &over.east),
            west: pick(&self.west, &over.west),
        }
    }
}

/// Color multiplied into a block's grayscale textures.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize)]
pub enum Tint {
    #[default]
    None,
    /// Biome grass color on the top face only (grass block; its side texture is pre-colored).
    GrassTop,
    /// Biome grass color on every face (tall grass, ferns).
    Grass,
    /// Biome foliage color on every face (leaves, vines).
    Foliage,
    /// Biome water color on every face (water, unless its texture is
    /// already colored).
    Water,
    /// A fixed 0xRRGGBB color on every face (e.g. lily pads).
    Fixed(u32),
}

/// The tint on one face, before the biome is known.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FaceTint {
    None,
    /// The biome's grass color.
    Grass,
    /// The biome's foliage color.
    Foliage,
    /// The biome's water color.
    Water,
    /// A fixed sRGB color.
    Rgb([f32; 3]),
}

impl Tint {
    /// How `face` is tinted.
    pub fn for_face(self, face: Face) -> FaceTint {
        match self {
            Tint::None => FaceTint::None,
            Tint::GrassTop if face != Face::Top => FaceTint::None,
            Tint::GrassTop | Tint::Grass => FaceTint::Grass,
            Tint::Foliage => FaceTint::Foliage,
            Tint::Water => FaceTint::Water,
            Tint::Fixed(hex) => FaceTint::Rgb(rgb(hex)),
        }
    }
}

/// A block's model, which also picks its outline and collision boxes (see
/// `world::shape`).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize)]
pub enum Shape {
    /// Draws nothing (air, moving piston).
    None,
    #[default]
    Cube,
    /// Two crossed diagonal planes (flowers, saplings, tall grass).
    Cross,
    /// Four planes in a # pattern (wheat, stems, nether wart).
    Crop,
    Liquid,
    Slab,
    Stairs,
    Fence,
    FenceGate,
    Pane,
    /// Cobblestone walls: a post with low or tall sides.
    Wall,
    /// A 1/16-high sheet (carpets).
    Carpet,
    Anvil,
    /// A wheel on two legs.
    Grindstone,
    /// A base, a post and a sloped desk.
    Lectern,
    /// A bell hanging from a beam between two posts.
    Bell,
    /// A small lantern standing on the ground.
    Lantern,
    Door,
    Trapdoor,
    Torch,
    Rail,
    Ladder,
    Lever,
    Button,
    PressurePlate,
    Repeater,
    RedstoneWire,
    SnowLayer,
    Vine,
    LilyPad,
    Cactus,
    Bed,
    Cake,
    Chest,
    Sign,
    Farmland,
    PistonHead,
    Fire,
    BrewingStand,
    Cauldron,
    EnchantingTable,
    Hopper,
    EndPortal,
    EndPortalFrame,
    DragonEgg,
    /// A 3×3-pixel stalk (bamboo).
    Bamboo,
    /// Drawn from the boxes in `BlockDef::model` (placeholder blocks).
    Model,
}

/// How placing a block picks its state (modern `getStateForPlacement`).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize)]
pub enum Placement {
    /// Keeps the item's state.
    #[default]
    Plain,
    /// A torch on the floor, or `wall` (facing away from the wall) on the
    /// side of a block. Can't hang from ceilings.
    Torch { wall: BlockId },
    /// Turns its front (`facing`) toward the player, like a furnace.
    Facing,
    /// Faces the way the player looks (fence gates).
    Look,
    /// Its front toward the player, up or down too (pistons).
    LookAll,
    /// Faces a quarter turn clockwise from the way the player looks (anvils).
    LookClockwise,
    /// A log, lying along the axis of the clicked face.
    Axis,
    /// A two-block bed on top of a block: the foot where placed and the
    /// head one block further the way the player looks.
    Bed,
    /// A slab on the bottom or top half of its cell, by where it was
    /// clicked, or joining the same slab there into a double one.
    Slab,
    /// A two-block door on top of a full block.
    Door,
    /// Stairs facing the way the player looks, upside down if placed
    /// against the top half of a block.
    Stairs,
    /// A trapdoor against the clicked side, on its top or bottom half.
    Trapdoor,
    /// On the side of a block, facing out from it (ladders).
    Wall,
    /// On the floor, ceiling or side of a block (levers and buttons).
    Attached,
    /// Vines, on the side or underside of the clicked block.
    Vine,
    /// Snow layers, stacking onto the same block up to 8.
    Layers,
    /// Rails, along the way the player looks.
    Rail,
    /// Hoppers, spout toward the clicked block (down if clicked from below).
    Hopper,
    /// Two-block plants (tall grass, sunflowers): the upper half goes above.
    Tall,
}

/// The way from a bed's foot to its head.
pub fn bed_direction(state: BlockState) -> IVec3 {
    IVec3::from_array(state.face(Prop::Facing).normal())
}

/// A door's thickness (3/16).
const DOOR_THICKNESS: f32 = 3.0 / 16.0;

/// The side of its cell a door covers: closed, the side opposite its
/// `facing` (nearest the player who placed it); open, a quarter turn
/// round its hinge (modern `DoorBlock.getShape`).
pub fn door_side(state: BlockState) -> Face {
    let facing = state.face(Prop::Facing);
    if !state.bool(Prop::Open) {
        facing.opposite()
    } else if state.get(Prop::Hinge) == super::props::hinge::LEFT {
        facing.rotate_ccw()
    } else {
        facing.rotate_cw()
    }
}

/// A door's slab within its cell, as (min, max).
pub fn door_box(state: BlockState) -> ([f32; 3], [f32; 3]) {
    let t = DOOR_THICKNESS;
    match door_side(state) {
        Face::North => ([0.0, 0.0, 0.0], [1.0, 1.0, t]),
        Face::East => ([1.0 - t, 0.0, 0.0], [1.0, 1.0, 1.0]),
        Face::South => ([0.0, 0.0, 1.0 - t], [1.0, 1.0, 1.0]),
        _ => ([0.0, 0.0, 0.0], [t, 1.0, 1.0]),
    }
}

/// What right-clicking a block does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub enum Activate {
    /// Opens a 3×3 crafting grid.
    Crafting,
    /// Opens its 27 slots, unless a solid block sits on top.
    Chest,
    /// Opens its input, fuel and output slots.
    Furnace,
    /// Sleeps in it.
    Bed,
    /// Opens or closes it.
    Door,
    /// Opens or closes it, a single block (trapdoors, fence gates).
    Open,
    /// Opens its nine slots (dispensers and droppers).
    Dispenser,
    /// Opens its five slots.
    Hopper,
    /// Opens the enchanting screen.
    Enchanting,
    /// Opens the anvil screen.
    Anvil,
    /// Opens the grindstone screen.
    Grindstone,
    /// Opens the smithing screen (M25).
    Smithing,
    /// Opens the brewing stand (M26).
    Brewing,
    /// Opens the stonecutter's screen (M29).
    Stonecutter,
    /// Flips, presses or tunes it (levers, buttons, repeaters, note blocks).
    Redstone,
}

/// How a block burns (modern `FireBlock.setFlammable`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Burns {
    /// How readily fire spreads into air next to it.
    pub encourage: u8,
    /// How readily it catches and burns away, out of 300 a fire tick.
    pub catch: u8,
}

/// The liquid a block is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, serde::Serialize)]
pub enum Fluid {
    Water,
    Lava,
}

/// How a block's pixels are drawn.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize)]
pub enum RenderLayer {
    #[default]
    Opaque,
    /// Pixels are fully opaque or fully transparent (leaves, glass, plants).
    Cutout,
    /// Partly see-through (water, ice, portal).
    Translucent,
}

/// One box of a block's model: a model element of vanilla's JSON.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelPart {
    /// Corners in pixels (sixteenths of a block) from the cell's minimum.
    pub from: (f32, f32, f32),
    pub to: (f32, f32, f32),
    pub textures: Textures,
}

impl ModelPart {
    /// The box, with flat elements (vanilla draws planes) given a half
    /// pixel of thickness so the crosshair can hit them.
    pub fn cuboid(&self) -> crate::world::shape::Cuboid {
        const MIN_THICKNESS: f32 = 0.5;
        let grow = |from: f32, to: f32| {
            if to - from >= MIN_THICKNESS {
                (from, to)
            } else if to + MIN_THICKNESS <= 16.0 {
                (from, from + MIN_THICKNESS)
            } else {
                (to - MIN_THICKNESS, to)
            }
        };
        let (x0, x1) = grow(self.from.0, self.to.0);
        let (y0, y1) = grow(self.from.1, self.to.1);
        let (z0, z1) = grow(self.from.2, self.to.2);
        crate::world::shape::Cuboid::px(x0, y0, z0, x1, y1, z1)
    }
}

/// Overrides for the states meeting a condition.
#[derive(Debug, Default, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Variant {
    /// Replaces the boxes it is drawn from (the End portal frame's eye).
    pub model: Option<Vec<ModelPart>>,
    /// Replaces the faces it names.
    pub textures: Option<Textures>,
    pub tint: Option<Tint>,
    pub light: Option<u8>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlockDef {
    /// Set from the block's place in the file.
    #[serde(skip)]
    pub id: BlockId,
    /// Modern Minecraft's name, without the namespace.
    pub name: String,
    /// A block registered from vanilla's data with no behavior of its
    /// own (`assets/data/placeholder_blocks.ron`, listed in
    /// `docs/placeholders.md`).
    #[serde(default)]
    pub placeholder: bool,
    /// Its name as shown ("Andesite Slab"), where the name made readable
    /// isn't enough (placeholders take theirs from vanilla's language file).
    #[serde(default)]
    pub display: Option<String>,
    /// Boxes it is drawn from, each with its own textures (placeholder
    /// models, the End portal frame's eye). Empty for most blocks.
    #[serde(default)]
    pub model: Vec<ModelPart>,
    /// Mining hardness from 1.0.0; negative means unbreakable.
    pub hardness: f32,
    /// Its properties, in the order states list them.
    #[serde(default)]
    pub props: Vec<Prop>,
    /// Default values unlike the properties' usual ones, as a condition
    /// (`lit=true`).
    #[serde(default)]
    pub defaults: String,
    /// Values a player-placed block gets (`persistent=true` for leaves).
    #[serde(default)]
    pub placed: String,
    /// Whether it has an item of its own (read by build.rs).
    #[serde(default = "default_true")]
    pub item: bool,
    /// The texture in `textures/item/` its item shows, instead of the
    /// block drawn as an icon.
    #[serde(default)]
    pub item_texture: Option<String>,
    /// Most of its item in a stack.
    #[serde(default = "default_stack")]
    pub stack: u8,
    #[serde(default)]
    pub shape: Shape,
    #[serde(default)]
    pub layer: RenderLayer,
    /// Whether entities collide with it.
    #[serde(default = "default_true")]
    pub solid: bool,
    /// Light emitted, 0–15 (variants may change it).
    #[serde(default)]
    pub light: u8,
    /// Light lost passing through, 0–15 (1.0.0's `lightOpacity`, with 255 as
    /// 15). Defaults to 15 for opaque cubes and 0 for everything else.
    #[serde(default)]
    pub opacity: Option<u8>,
    /// Hide faces between two blocks of this type (glass, water, ice).
    #[serde(default)]
    pub cull_same: bool,
    /// Ground friction: 0.6 for most blocks, 0.98 for ice (1.0.0 values).
    #[serde(default = "default_slipperiness")]
    pub slipperiness: f32,
    /// Walking speed while in or on it, as a fraction (modern
    /// `Block.speedFactor`: soul sand and honey 0.4).
    #[serde(default = "default_factor")]
    pub speed_factor: f32,
    /// Jump height while on it (modern `jumpFactor`: honey 0.5).
    #[serde(default = "default_factor")]
    pub jump_factor: f32,
    /// Ladders and vines: bodies in it climb.
    #[serde(default)]
    pub climbable: bool,
    /// Landing on it bounces the player back up (slime).
    #[serde(default)]
    pub bouncy: bool,
    /// Placing a block may overwrite it, and clicking it places into its
    /// cell (air, fluids, fire, tall grass, dead bushes, snow layers, vines).
    #[serde(default)]
    pub replaceable: bool,
    /// Pops off when the block below it is removed (plants, crops, rails,
    /// pressure plates, redstone, snow layers, standing signs).
    #[serde(default)]
    pub needs_ground: bool,
    /// Pops off without a block above to hang from (weeping vines).
    #[serde(default)]
    pub hangs: bool,
    #[serde(default)]
    pub placement: Placement,
    /// What right-clicking it does, if anything.
    #[serde(default)]
    pub activate: Option<Activate>,
    /// Falls when there's nothing under it (sand, gravel), after a block
    /// update: being placed, or a neighbor changing.
    #[serde(default)]
    pub falls: bool,
    /// The block it turns into touching water (concrete powder).
    #[serde(default)]
    pub hardens: Option<BlockId>,
    /// A block whose behavior it shares where code names blocks: every
    /// wooden button presses like the oak button, every kind of leaves
    /// acts like oak leaves.
    #[serde(default)]
    pub acts_as: Option<BlockId>,
    /// Liquids swim, slow mining and color the fog.
    #[serde(default)]
    pub fluid: Option<Fluid>,
    /// Holds back water and lava without being solid to walk into (signs,
    /// pressure plates, cobwebs, sugar cane: 1.0.0's solid materials and
    /// `blockBlocksFlow` list). Solid blocks always do.
    #[serde(default)]
    pub blocks_flow: bool,
    /// How fire treats it; `None` doesn't burn.
    #[serde(default)]
    pub burns: Option<Burns>,
    /// The tool that mines it fast (1.0.0's effective tool lists and
    /// materials).
    #[serde(default)]
    pub tool: Option<ToolKind>,
    /// The tool it needs to drop anything; `None` drops for anything.
    #[serde(default)]
    pub harvest: Option<Harvest>,
    #[serde(default)]
    pub drops: Drops,
    #[serde(default)]
    pub textures: Option<Textures>,
    #[serde(default)]
    pub tint: Tint,
    /// Overrides for states meeting a condition, applied in order.
    #[serde(default)]
    pub variants: Vec<(String, Variant)>,
}

fn default_true() -> bool {
    true
}

fn default_stack() -> u8 {
    64
}

fn default_slipperiness() -> f32 {
    0.6
}

fn default_factor() -> f32 {
    1.0
}

impl BlockDef {
    /// The variants `state` meets.
    fn variants_of(&self, state: BlockState) -> impl Iterator<Item = &Variant> {
        let conditions = registry().conditions(self.id);
        self.variants
            .iter()
            .zip(conditions)
            .filter(move |(_, condition)| state.matches(condition))
            .map(|((_, variant), _)| variant)
    }

    /// Whether `state` meets the condition of variant number `variant`.
    pub fn shows_variant(&self, state: BlockState, variant: usize) -> bool {
        registry()
            .conditions(self.id)
            .get(variant)
            .is_some_and(|condition| state.matches(condition))
    }

    /// Every set of textures the block names: its own, its variants', and
    /// those of its model's boxes.
    pub fn all_textures(&self) -> impl Iterator<Item = &Textures> {
        let parts = self.model.iter().map(|p| &p.textures);
        let variants = self.variants.iter().flat_map(|(_, v)| {
            v.textures
                .iter()
                .chain(v.model.iter().flatten().map(|p| &p.textures))
        });
        self.textures.iter().chain(variants).chain(parts)
    }

    pub fn textures_for(&self, state: BlockState) -> Option<Textures> {
        let mut textures = self.textures.clone();
        for over in self.variants_of(state).filter_map(|v| v.textures.as_ref()) {
            textures = Some(textures.unwrap_or_default().overlaid(over));
        }
        textures
    }

    /// The boxes `state` is drawn from, if it has a model of its own.
    pub fn parts_for(&self, state: BlockState) -> &[ModelPart] {
        self.variants_of(state)
            .filter_map(|v| v.model.as_deref())
            .last()
            .unwrap_or(&self.model)
    }

    pub fn tint_for(&self, state: BlockState) -> Tint {
        self.variants_of(state)
            .filter_map(|v| v.tint)
            .last()
            .unwrap_or(self.tint)
    }

    /// Light `state` gives off, 0–15.
    pub fn light_for(&self, state: BlockState) -> u8 {
        self.variants_of(state)
            .filter_map(|v| v.light)
            .last()
            .unwrap_or(self.light)
    }

    pub fn light_opacity(&self) -> u8 {
        self.opacity
            .unwrap_or(if self.is_opaque_cube() { 15 } else { 0 })
    }

    /// Light `state` stops: as `light_opacity`, except that a double slab
    /// is a full opaque block.
    pub fn light_opacity_for(&self, state: BlockState) -> u8 {
        self.opacity.unwrap_or(if self.is_opaque_cube_state(state) {
            15
        } else {
            0
        })
    }

    /// Offset to the block holding this one up, if it pops off without it.
    pub fn support(&self, state: BlockState) -> Option<IVec3> {
        let behind = || IVec3::from_array(state.face(Prop::Facing).opposite().normal());
        match self.shape {
            // A wall torch hangs on the block behind it.
            Shape::Torch if state.has(Prop::Facing) => Some(behind()),
            Shape::Torch => Some(IVec3::NEG_Y),
            Shape::Ladder => Some(behind()),
            Shape::Lever | Shape::Button => Some(match state.get(Prop::AttachFace) {
                super::props::attach_face::FLOOR => IVec3::NEG_Y,
                super::props::attach_face::CEILING => IVec3::Y,
                _ => behind(),
            }),
            // The lower half stands on the ground, the upper on the lower.
            Shape::Door => Some(IVec3::NEG_Y),
            // Each half goes with the other.
            Shape::Bed => {
                let toward_head = bed_direction(state);
                Some(
                    if state.get(Prop::BedPart) == super::props::bed_part::FOOT {
                        toward_head
                    } else {
                        -toward_head
                    },
                )
            }
            _ if self.hangs => Some(IVec3::Y),
            _ => self.needs_ground.then_some(IVec3::NEG_Y),
        }
    }

    /// Every neighbor whose removal takes this block with it: what holds
    /// it up, plus the upper half of a door or tall plant for its lower half.
    pub fn held_by(&self, state: BlockState) -> Vec<IVec3> {
        let mut by: Vec<IVec3> = self.support(state).into_iter().collect();
        if matches!(self.placement, Placement::Door | Placement::Tall)
            && state.get(Prop::DoubleBlockHalf) == super::props::door_half::LOWER
        {
            by.push(IVec3::Y);
        }
        by
    }

    /// The face its front texture is on, for blocks placed facing the player.
    pub fn front(&self, state: BlockState) -> Option<Face> {
        if self.placement != Placement::Facing {
            return None;
        }
        let face = state.face(if state.has(Prop::FacingAll) {
            Prop::FacingAll
        } else {
            Prop::Facing
        });
        // The front of a dispenser facing up or down is drawn by a variant.
        matches!(face, Face::North | Face::South | Face::East | Face::West).then_some(face)
    }

    /// Whether the crosshair can select it (everything but air and fluids).
    pub fn is_targetable(&self) -> bool {
        !matches!(self.shape, Shape::None | Shape::Liquid)
    }

    /// Whether water and lava stop at it rather than wash it away.
    pub fn stops_fluid(&self) -> bool {
        self.fluid.is_none() && (self.solid || self.blocks_flow)
    }

    /// A full opaque cube hides the faces of whatever touches it.
    pub fn is_opaque_cube(&self) -> bool {
        self.shape == Shape::Cube && self.layer == RenderLayer::Opaque
    }

    /// Whether `state` fills its whole cell with opaque faces (a double
    /// slab does, a single one doesn't).
    pub fn is_opaque_cube_state(&self, state: BlockState) -> bool {
        self.is_opaque_cube()
            || (self.shape == Shape::Slab
                && self.layer == RenderLayer::Opaque
                && state.get(Prop::SlabType) == super::props::slab_type::DOUBLE)
    }

    /// Whether its state hangs on its neighbors and is worked out again
    /// when they change: fence, pane, wall and vine connections, stair corners,
    /// redstone dust, fence gates in walls and snowy grass.
    pub fn shaped_by_neighbors(&self) -> bool {
        matches!(
            self.shape,
            Shape::Fence
                | Shape::Pane
                | Shape::Wall
                | Shape::Stairs
                | Shape::RedstoneWire
                | Shape::Vine
                | Shape::FenceGate
        ) || self.props.contains(&Prop::Snowy)
            || self.id == super::blocks::CHORUS_PLANT
    }

    /// Whether placing something may overwrite `state`: a snow layer only
    /// while it's one layer thin.
    pub fn is_replaceable(&self, state: BlockState) -> bool {
        self.replaceable && (!state.has(Prop::Layers) || state.number(Prop::Layers) == 1)
    }

    /// The state a player places: the default, with `placed` applied.
    pub fn placed_state(&self) -> BlockState {
        registry().placed[self.id.index()]
    }
}

/// All block definitions, indexed by ID.
pub struct BlockRegistry {
    blocks: Vec<BlockDef>,
    layouts: Vec<Layout>,
    /// Parsed `variants` conditions per block.
    conditions: Vec<Vec<Condition>>,
    /// Each block's first state in dense per-state tables.
    bases: Vec<u32>,
    states: u32,
    /// Each block's player-placed state.
    placed: Vec<BlockState>,
}

/// The registry for `assets/data/blocks.ron`, built into the game (its
/// names are compiled in as constants, see build.rs).
pub fn registry() -> &'static BlockRegistry {
    static REGISTRY: LazyLock<BlockRegistry> = LazyLock::new(|| {
        BlockRegistry::from_ron(&[
            include_str!("../../assets/data/blocks.ron"),
            include_str!("../../assets/data/placeholder_blocks.ron"),
        ])
        .expect("assets/data/blocks.ron and placeholder_blocks.ron are valid")
    });
    &REGISTRY
}

impl BlockRegistry {
    fn from_ron(texts: &[&str]) -> Result<Self> {
        let mut blocks: Vec<BlockDef> = Vec::new();
        for text in texts {
            blocks.extend(ron::from_str::<Vec<BlockDef>>(text)?);
        }
        if blocks.len() != super::blocks::COUNT {
            bail!(
                "blocks.ron has {} blocks but the game was built with {}",
                blocks.len(),
                super::blocks::COUNT
            );
        }
        let mut layouts = Vec::with_capacity(blocks.len());
        let mut conditions = Vec::with_capacity(blocks.len());
        let mut bases = Vec::with_capacity(blocks.len());
        let mut placed = Vec::with_capacity(blocks.len());
        let mut states = 0u32;
        for (index, def) in blocks.iter_mut().enumerate() {
            def.id = BlockId(index as u16);
            if super::blocks::NAMES[index] != def.name {
                bail!(
                    "block {index} is {} but was built as {}",
                    def.name,
                    super::blocks::NAMES[index]
                );
            }
            if def.light > 15 {
                bail!("block {} has light level {} above 15", def.name, def.light);
            }
            if def.opacity.is_some_and(|o| o > 15) {
                bail!("block {} has opacity above 15", def.name);
            }
            let mut layout = Layout::new(
                def.props.clone(),
                def.props.iter().map(|p| p.default_index()).collect(),
            );
            if layout.states > u16::MAX as u32 {
                bail!("block {} has too many states", def.name);
            }
            // Defaults unlike the usual ones.
            let given = Condition::parse(&def.defaults, &layout, &def.name)?;
            for &(slot, allowed) in &given.0 {
                layout.defaults[slot] = allowed.trailing_zeros() as u8;
            }
            for drop in def.drops.list_mut() {
                drop.condition = Condition::parse(&drop.when, &layout, &def.name)?;
            }
            let parsed = def
                .variants
                .iter()
                .map(|(condition, _)| Condition::parse(condition, &layout, &def.name))
                .collect::<Result<Vec<_>>>()?;
            let mut placed_props = 0;
            for &(slot, allowed) in &Condition::parse(&def.placed, &layout, &def.name)?.0 {
                placed_props = layout.write(placed_props, slot, allowed.trailing_zeros() as u8);
            }
            placed.push(BlockState {
                id: def.id,
                props: placed_props,
            });
            bases.push(states);
            states += layout.states;
            layouts.push(layout);
            conditions.push(parsed);
        }
        let registry = Self {
            blocks,
            layouts,
            conditions,
            bases,
            states,
            placed,
        };
        Ok(registry)
    }

    pub fn get(&self, id: BlockId) -> Option<&BlockDef> {
        self.blocks.get(id.index())
    }

    /// The definition of a block that exists (every `BlockId` does).
    pub fn def(&self, id: BlockId) -> &BlockDef {
        &self.blocks[id.index()]
    }

    pub fn layout(&self, id: BlockId) -> &Layout {
        &self.layouts[id.index()]
    }

    fn conditions(&self, id: BlockId) -> &[Condition] {
        &self.conditions[id.index()]
    }

    /// The number of states of all blocks together.
    pub fn state_count(&self) -> usize {
        self.states as usize
    }

    /// `state`'s position in dense per-state tables, 0..`state_count`.
    pub fn state_index(&self, state: BlockState) -> usize {
        (self.bases[state.id.index()] + state.props as u32) as usize
    }

    /// Every state of every block, in `state_index` order.
    pub fn states(&self) -> impl Iterator<Item = BlockState> + '_ {
        self.blocks.iter().flat_map(|def| {
            self.layout(def.id)
                .all_states()
                .map(move |props| BlockState { id: def.id, props })
        })
    }

    /// A readable name like "oak planks" or "orange wool".
    pub fn display_name(&self, block: BlockState) -> String {
        let def = self.def(block.id);
        def.display
            .clone()
            .unwrap_or_else(|| def.name.replace('_', " "))
    }

    pub fn iter(&self) -> impl Iterator<Item = &BlockDef> {
        self.blocks.iter()
    }

    /// Texture names referenced by some block or variant that `exists` rejects.
    /// Every texture location (`block/stone`) the blocks name.
    pub fn texture_locations(&self) -> std::collections::BTreeSet<String> {
        self.iter()
            .flat_map(BlockDef::all_textures)
            .flat_map(|textures| Face::ALL.map(|face| textures.get(face)))
            .flatten()
            .map(|name| texture_location(name).into_owned())
            .collect()
    }

    /// The texture locations the blocks name that `exists` says are missing.
    pub fn missing_textures(
        &self,
        exists: impl Fn(&str) -> bool,
    ) -> std::collections::BTreeSet<String> {
        self.missing_textures_in(exists, false)
    }

    /// Like `missing_textures`, counting the placeholder blocks too if
    /// `placeholders` (they get their textures from the vanilla layer).
    pub fn missing_textures_in(
        &self,
        exists: impl Fn(&str) -> bool,
        placeholders: bool,
    ) -> std::collections::BTreeSet<String> {
        self.iter()
            .filter(|def| placeholders || !def.placeholder)
            .flat_map(BlockDef::all_textures)
            .flat_map(|textures| Face::ALL.map(|face| textures.get(face)))
            .flatten()
            .map(texture_location)
            .filter(|name| !exists(name))
            .map(|name| name.into_owned())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::{blocks, props::Prop};

    #[test]
    fn display_names_are_readable() {
        let registry = registry();
        assert_eq!(
            registry.display_name(BlockState::of(blocks::STONE_BRICKS)),
            "stone bricks"
        );
        assert_eq!(
            registry.display_name(BlockState::of(blocks::ORANGE_WOOL)),
            "orange wool"
        );
    }

    #[test]
    fn states_pack_property_values_with_the_default_at_zero() {
        let stairs = BlockState::of(blocks::OAK_STAIRS);
        assert_eq!(stairs.face(Prop::Facing), Face::North);
        assert_eq!(stairs.get(Prop::Half), super::super::props::half::BOTTOM);
        let turned = stairs
            .with_face(Prop::Facing, Face::East)
            .with(Prop::Half, super::super::props::half::TOP);
        assert_eq!(turned.face(Prop::Facing), Face::East);
        assert_eq!(turned.get(Prop::Half), super::super::props::half::TOP);
        assert!(!turned.bool(Prop::Waterlogged));
        assert_ne!(turned, stairs);
        // Properties a block doesn't have read as their default and can't be set.
        let stone = BlockState::of(blocks::STONE);
        assert_eq!(stone.with(Prop::Facing, 3), stone);
        assert!(!stone.has(Prop::Facing));
        // A log stands upright by default.
        let log = BlockState::of(blocks::OAK_LOG);
        assert_eq!(log.get(Prop::Axis), super::super::props::axis::Y);
    }

    #[test]
    fn states_print_and_parse_like_minecraft() {
        let state = BlockState::of(blocks::OAK_STAIRS)
            .with_face(Prop::Facing, Face::East)
            .with(Prop::Half, super::super::props::half::TOP);
        let text = state.to_string();
        assert_eq!(
            text,
            "minecraft:oak_stairs[facing=east,half=top,shape=straight,waterlogged=false]"
        );
        assert_eq!(BlockState::parse(&text).unwrap(), state);
        assert_eq!(
            BlockState::parse("oak_stairs[half=top,facing=east]").unwrap(),
            state
        );
        assert_eq!(
            BlockState::parse("minecraft:stone").unwrap().to_string(),
            "minecraft:stone"
        );
        assert!(BlockState::parse("minecraft:stone[facing=north]").is_err());
        assert!(BlockState::parse("minecraft:oak_stairs[facing=up]").is_err());
        assert!(BlockState::parse("minecraft:no_such_block").is_err());
    }

    #[test]
    fn every_state_round_trips_through_its_text() {
        let registry = registry();
        let mut count = 0;
        for state in registry.states() {
            assert_eq!(BlockState::parse(&state.to_string()).unwrap(), state);
            count += 1;
        }
        assert_eq!(count, registry.state_count());
    }

    #[test]
    fn defaults_unlike_the_usual_ones_come_first() {
        // Redstone torches start lit.
        assert!(BlockState::of(blocks::REDSTONE_TORCH).bool(Prop::Lit));
        // Mushroom blocks show their skin on every side.
        let cap = BlockState::of(blocks::BROWN_MUSHROOM_BLOCK);
        assert!(Face::ALL.iter().all(|&face| cap.bool(Prop::side(face))));
        // Leaves placed by a player don't decay.
        let leaves = registry().def(blocks::OAK_LEAVES).placed_state();
        assert!(leaves.bool(Prop::Persistent));
        assert!(!BlockState::of(blocks::OAK_LEAVES).bool(Prop::Persistent));
    }

    #[test]
    fn most_specific_texture_wins() {
        let textures = Textures {
            all: Some("all".into()),
            side: Some("side".into()),
            top: Some("top".into()),
            south: Some("front".into()),
            ..Default::default()
        };
        assert_eq!(textures.get(Face::Top), Some("top"));
        assert_eq!(textures.get(Face::Bottom), Some("all"));
        assert_eq!(textures.get(Face::South), Some("front"));
        assert_eq!(textures.get(Face::East), Some("side"));
    }

    #[test]
    fn grass_top_tints_only_the_top() {
        assert_eq!(Tint::GrassTop.for_face(Face::Top), FaceTint::Grass);
        assert_eq!(Tint::GrassTop.for_face(Face::North), FaceTint::None);
        assert_eq!(Tint::Grass.for_face(Face::North), FaceTint::Grass);
        assert_eq!(
            Tint::Fixed(0xFF8000).for_face(Face::East),
            FaceTint::Rgb([1.0, 128.0 / 255.0, 0.0])
        );
    }

    #[test]
    fn variants_change_textures_and_light_by_state() {
        let wheat = registry().def(blocks::WHEAT);
        let ripe = BlockState::of(blocks::WHEAT).with_number(Prop::Age7, 7);
        assert_eq!(
            wheat.textures_for(ripe).unwrap().get(Face::North),
            Some("wheat_stage7")
        );
        let furnace = registry().def(blocks::FURNACE);
        let lit = BlockState::of(blocks::FURNACE).with_bool(Prop::Lit, true);
        assert_eq!(furnace.light_for(lit), 13);
        assert_eq!(furnace.light_for(BlockState::of(blocks::FURNACE)), 0);
        assert_eq!(
            furnace.textures_for(lit).unwrap().get(Face::South),
            Some("furnace_front_on")
        );
        let grass = registry().def(blocks::GRASS_BLOCK);
        let snowy = BlockState::of(blocks::GRASS_BLOCK).with_bool(Prop::Snowy, true);
        assert_eq!(grass.tint_for(snowy), Tint::None);
        assert_eq!(
            grass.textures_for(snowy).unwrap().get(Face::Bottom),
            Some("dirt")
        );
    }

    #[test]
    fn spot_check_properties() {
        let block = |id| registry().def(id);
        assert_eq!(block(blocks::STONE).hardness, 1.5);
        assert!(
            block(blocks::BEDROCK).hardness < 0.0,
            "bedrock is unbreakable"
        );
        assert_eq!(block(blocks::OBSIDIAN).hardness, 50.0);
        assert_eq!(block(blocks::TORCH).light, 14);
        assert_eq!(block(blocks::GLOWSTONE).light, 15);
        assert!(block(blocks::STONE).is_opaque_cube());
        assert!(
            !block(blocks::GLASS).is_opaque_cube(),
            "glass is see-through"
        );
        assert!(!block(blocks::DANDELION).solid, "flowers have no collision");
        assert_eq!(block(blocks::DANDELION).shape, Shape::Cross);
        assert_eq!(block(blocks::AIR).shape, Shape::None);
        assert_eq!(BlockState::AIR.id(), blocks::AIR);
    }

    #[test]
    fn doors_cover_one_side_and_swing_a_quarter_round_their_hinge() {
        let t = 3.0 / 16.0;
        let door = BlockState::of(blocks::OAK_DOOR).with_face(Prop::Facing, Face::North);
        // Placed looking north, it closes along the south side, nearest the player.
        assert_eq!(door_box(door), ([0.0, 0.0, 1.0 - t], [1.0, 1.0, 1.0]));
        // Open with the hinge on the left, it swings to the west side.
        let open = door.with_bool(Prop::Open, true);
        assert_eq!(door_box(open), ([0.0; 3], [t, 1.0, 1.0]));
        let right = open.with(Prop::Hinge, super::super::props::hinge::RIGHT);
        assert_eq!(door_box(right), ([1.0 - t, 0.0, 0.0], [1.0, 1.0, 1.0]));
        let def = registry().def(blocks::OAK_DOOR);
        let upper = door.with(Prop::DoubleBlockHalf, super::super::props::door_half::UPPER);
        assert_eq!(def.held_by(door), [IVec3::NEG_Y, IVec3::Y]);
        assert_eq!(def.held_by(upper), [IVec3::NEG_Y]);
    }

    #[test]
    fn furnaces_and_chests_turn_their_front_by_facing() {
        let furnace = registry().def(blocks::FURNACE);
        let facing = |face| BlockState::of(blocks::FURNACE).with_face(Prop::Facing, face);
        assert_eq!(furnace.front(facing(Face::North)), Some(Face::North));
        assert_eq!(furnace.front(facing(Face::East)), Some(Face::East));
        assert_eq!(
            registry()
                .def(blocks::STONE)
                .front(BlockState::of(blocks::STONE)),
            None
        );
    }

    /// Keep in sync with the missing-texture list in docs/DECISIONS.md.
    #[test]
    fn only_known_block_textures_are_missing() {
        let base =
            crate::pack::Resources::load_with(&crate::assets::dir(), Vec::new(), false).unwrap();
        let missing = registry().missing_textures(|name| base.texture(name).is_some());
        // The eye of the End portal frame is only in vanilla's and the
        // packs' textures (the sign borrows the oak planks).
        let mut expected = vec!["block/end_portal_frame_eye".to_owned()];
        // Concrete isn't in Kenzo's textures; a resource pack brings it.
        for color in [
            "white",
            "orange",
            "magenta",
            "light_blue",
            "yellow",
            "lime",
            "pink",
            "gray",
            "light_gray",
            "cyan",
            "purple",
            "blue",
            "brown",
            "green",
            "red",
            "black",
        ] {
            expected.push(format!("block/{color}_concrete"));
            expected.push(format!("block/{color}_concrete_powder"));
        }
        // M21's modern blocks, which Kenzo's 1.12-era textures predate.
        for name in [
            "amethyst_block",
            "amethyst_cluster",
            "azalea_leaves",
            "bamboo_block",
            "bamboo_block_top",
            "bamboo_door_bottom",
            "bamboo_door_top",
            "bamboo_mosaic",
            "bamboo_planks",
            "bamboo_stalk",
            "bamboo_trapdoor",
            "blue_ice",
            "budding_amethyst",
            "calcite",
            "cave_vines",
            "cave_vines_lit",
            "cave_vines_plant",
            "cave_vines_plant_lit",
            "cherry_door_bottom",
            "cherry_door_top",
            "cherry_leaves",
            "cherry_log",
            "cherry_log_top",
            "cherry_planks",
            "cherry_sapling",
            "cherry_trapdoor",
            "cobbled_deepslate",
            "copper_block",
            "copper_ore",
            "cornflower",
            "deepslate",
            "deepslate_coal_ore",
            "deepslate_copper_ore",
            "deepslate_diamond_ore",
            "deepslate_emerald_ore",
            "deepslate_gold_ore",
            "deepslate_iron_ore",
            "deepslate_lapis_ore",
            "deepslate_redstone_ore",
            "deepslate_top",
            "dripstone_block",
            "flowering_azalea_leaves",
            "large_amethyst_bud",
            "mangrove_door_bottom",
            "mangrove_door_top",
            "mangrove_leaves",
            "mangrove_log",
            "mangrove_log_top",
            "mangrove_planks",
            "mangrove_propagule",
            "mangrove_roots_side",
            "mangrove_roots_top",
            "mangrove_trapdoor",
            "barrel_bottom",
            "barrel_side",
            "barrel_top",
            "blast_furnace_front",
            "blast_furnace_front_on",
            "blast_furnace_side",
            "blast_furnace_top",
            "composter_bottom",
            "composter_side",
            "composter_top",
            "crying_obsidian",
            "lantern",
            "lectern_front",
            "lectern_sides",
            "lectern_top",
            "smithing_table_bottom",
            "smithing_table_front",
            "smithing_table_side",
            "smithing_table_top",
            "smoker_bottom",
            "smoker_front",
            "smoker_front_on",
            "smoker_side",
            "smoker_top",
            "grindstone_round",
            "grindstone_side",
            "medium_amethyst_bud",
            "moss_block",
            "mud",
            "mud_bricks",
            "muddy_mangrove_roots_side",
            "muddy_mangrove_roots_top",
            "packed_mud",
            "pink_petals",
            "pointed_dripstone_down_base",
            "pointed_dripstone_down_frustum",
            "pointed_dripstone_down_middle",
            "pointed_dripstone_down_tip",
            "pointed_dripstone_down_tip_merge",
            "pointed_dripstone_up_base",
            "pointed_dripstone_up_frustum",
            "pointed_dripstone_up_middle",
            "pointed_dripstone_up_tip",
            "pointed_dripstone_up_tip_merge",
            "rooted_dirt",
            "small_amethyst_bud",
            "smooth_basalt",
            "stripped_acacia_log",
            "stripped_acacia_log_top",
            "stripped_bamboo_block",
            "stripped_bamboo_block_top",
            "stripped_birch_log",
            "stripped_birch_log_top",
            "stripped_cherry_log",
            "stripped_cherry_log_top",
            "stripped_dark_oak_log",
            "stripped_dark_oak_log_top",
            "stripped_jungle_log",
            "stripped_jungle_log_top",
            "stripped_mangrove_log",
            "stripped_mangrove_log_top",
            "stripped_oak_log",
            "stripped_oak_log_top",
            "stripped_spruce_log",
            "stripped_spruce_log_top",
            "tuff",
        ] {
            expected.push(format!("block/{name}"));
        }
        // M25's fortress, bastion and netherite blocks.
        for name in [
            "ancient_debris_side",
            "ancient_debris_top",
            "chain",
            "chiseled_nether_bricks",
            "chiseled_polished_blackstone",
            "cracked_nether_bricks",
            "cracked_polished_blackstone_bricks",
            "gilded_blackstone",
            "glow_lichen",
            "netherite_block",
            "polished_blackstone",
            "polished_blackstone_bricks",
        ] {
            expected.push(format!("block/{name}"));
        }
        // M24's Nether blocks from 1.16 on.
        for name in [
            "basalt_side",
            "basalt_top",
            "blackstone",
            "blackstone_top",
            "crimson_door_bottom",
            "crimson_door_top",
            "crimson_fungus",
            "crimson_nylium",
            "crimson_nylium_side",
            "crimson_planks",
            "crimson_roots",
            "crimson_stem",
            "crimson_stem_top",
            "crimson_trapdoor",
            "nether_gold_ore",
            "nether_sprouts",
            "polished_basalt_side",
            "polished_basalt_top",
            "shroomlight",
            "soul_fire_0",
            "soul_lantern",
            "soul_soil",
            "soul_torch",
            "stripped_crimson_stem",
            "stripped_crimson_stem_top",
            "stripped_warped_stem",
            "stripped_warped_stem_top",
            "twisting_vines",
            "twisting_vines_plant",
            "warped_door_bottom",
            "warped_door_top",
            "warped_fungus",
            "warped_nylium",
            "warped_nylium_side",
            "warped_planks",
            "warped_roots",
            "warped_stem",
            "warped_stem_top",
            "warped_trapdoor",
            "warped_wart_block",
            "weeping_vines",
            "weeping_vines_plant",
            "purpur_block",
            "purpur_pillar",
            "purpur_pillar_top",
            "stonecutter_top",
            "stonecutter_bottom",
            "stonecutter_side",
            "loom_top",
            "loom_bottom",
            "loom_side",
            "loom_front",
            "cartography_table_top",
            "cartography_table_side1",
            "cartography_table_side2",
            "cartography_table_side3",
            "fletching_table_top",
            "fletching_table_front",
            "fletching_table_side",
        ] {
            expected.push(format!("block/{name}"));
        }
        assert_eq!(missing, expected.into_iter().collect());
    }

    #[test]
    fn states_turn_with_a_structure() {
        let parse = |text| BlockState::parse(text).unwrap();
        assert_eq!(
            parse("oak_stairs[facing=north,half=top]").rotated(1),
            parse("oak_stairs[facing=east,half=top]")
        );
        assert_eq!(
            parse("oak_log[axis=x]").rotated(1),
            parse("oak_log[axis=z]")
        );
        assert_eq!(
            parse("oak_log[axis=y]").rotated(3),
            parse("oak_log[axis=y]")
        );
        assert_eq!(
            parse("rail[shape=north_east]").rotated(2),
            parse("rail[shape=south_west]")
        );
        assert_eq!(
            parse("oak_fence[north=true,east=false]").rotated(1),
            parse("oak_fence[east=true,north=false]")
        );
        assert_eq!(parse("stone").rotated(1), parse("stone"));
        // Four turns come back around.
        let door = parse("oak_door[facing=west,half=upper,hinge=right]");
        assert_eq!(door.rotated(1).rotated(1).rotated(1).rotated(1), door);
    }
}
