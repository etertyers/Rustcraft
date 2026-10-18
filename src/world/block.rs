//! Block definitions loaded from `assets/data/blocks.ron`.
//!
//! IDs are Minecraft 1.0.0's numeric block IDs (`docs/blocks-and-items.md`).

use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

use anyhow::{Context, Result, bail};
use glam::IVec3;
use serde::Deserialize;

use super::{
    Face,
    biome::rgb,
    harvest::{Drops, Harvest},
};
use crate::inventory::item::ToolKind;

pub type BlockId = u16;

/// Number of distinct data values (1.0.0 stores 4 bits per block).
pub const DATA_VALUES: usize = 16;

/// A block in the world: its ID plus a 4-bit data value (wool color, log type,
/// crop stage...), packed into 16 bits like 1.0.0's 12-bit ID + 4-bit data.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BlockState(u16);

impl BlockState {
    pub const AIR: BlockState = BlockState(0);

    pub const fn new(id: BlockId, data: u8) -> Self {
        Self((id & 0x0FFF) | ((data as u16 & 0xF) << 12))
    }

    pub const fn id(self) -> BlockId {
        self.0 & 0x0FFF
    }

    pub const fn data(self) -> u8 {
        (self.0 >> 12) as u8
    }

    pub const fn is_air(self) -> bool {
        self.id() == 0
    }

    /// The packed 16 bits, for saving.
    pub const fn to_bits(self) -> u16 {
        self.0
    }

    pub const fn from_bits(bits: u16) -> Self {
        Self(bits)
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
            Tint::Fixed(hex) => FaceTint::Rgb(rgb(hex)),
        }
    }
}

/// A block's model. Only `None`, `Cube`, `Cross` and `Crop` are built so far;
/// every other shape draws as a full cube placeholder (see docs/DECISIONS.md).
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
    EndPortal,
    EndPortalFrame,
    DragonEgg,
}

/// How placing a block sets its data value.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize)]
pub enum Placement {
    /// Keeps the data value it was placed with.
    #[default]
    Plain,
    /// Attaches to the clicked face like a 1.0.0 torch: data 1–4 for walls
    /// (attached to the block to the west, east, north, south), 5 for floors.
    /// Can't hang from ceilings.
    Torch,
    /// Turns its front (the `south` texture) toward the player, like
    /// 1.0.0's furnace: data 2–5 for a front facing north, south, west, east.
    Facing,
    /// A two-block bed on top of a block (1.0.0's `ItemBed`): the foot where
    /// placed and the head one block further the way the player looks.
    /// Data 0–3 is that way (south, west, north, east); the head adds 8.
    Bed,
    /// A slab, which turns the same slab under it into `double` (keeping
    /// the data value) instead of stacking on it (1.0.0's `BlockStep`).
    Slab { double: BlockId },
    /// A two-block door on top of a full block (1.0.0's `ItemDoor`): data
    /// 0–3 is its direction, 4 is open and 8 the upper half.
    Door,
}

/// The way from a bed's foot to its head for direction `dir` (0–3: south,
/// west, north, east, as in 1.0.0).
pub fn bed_direction(dir: u8) -> IVec3 {
    match dir & 3 {
        0 => IVec3::Z,
        1 => IVec3::NEG_X,
        2 => IVec3::NEG_Z,
        _ => IVec3::X,
    }
}

/// A door's thickness (1.0.0's 0.1875).
const DOOR_THICKNESS: f32 = 3.0 / 16.0;

/// Which side of its cell a door covers, 0–3 (1.0.0's `getState`): its
/// direction less one while closed, its direction while open.
pub fn door_side(data: u8) -> u8 {
    if data & 4 == 0 {
        (data + 3) & 3
    } else {
        data & 3
    }
}

/// A door's slab within its cell, as (min, max) (1.0.0's
/// `setDoorRotation`): along the north, east, south or west side.
pub fn door_box(data: u8) -> ([f32; 3], [f32; 3]) {
    let t = DOOR_THICKNESS;
    match door_side(data) {
        0 => ([0.0, 0.0, 0.0], [1.0, 1.0, t]),
        1 => ([1.0 - t, 0.0, 0.0], [1.0, 1.0, 1.0]),
        2 => ([0.0, 0.0, 1.0 - t], [1.0, 1.0, 1.0]),
        _ => ([0.0, 0.0, 0.0], [t, 1.0, 1.0]),
    }
}

/// What right-clicking a block does (1.0.0's `blockActivated`).
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
}

/// The liquid a block is (flowing or still).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
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
    /// Partly see-through (water, ice, portal). Drawn as cutout until the
    /// blended pass exists (M8).
    Translucent,
}

/// Overrides for one data value.
#[derive(Debug, Default, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Variant {
    pub name: Option<String>,
    pub textures: Option<Textures>,
    pub tint: Option<Tint>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlockDef {
    pub id: BlockId,
    pub name: String,
    /// Mining hardness from 1.0.0; negative means unbreakable.
    pub hardness: f32,
    #[serde(default)]
    pub shape: Shape,
    #[serde(default)]
    pub layer: RenderLayer,
    /// Whether entities collide with it.
    #[serde(default = "default_true")]
    pub solid: bool,
    /// Light emitted, 0–15.
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
    /// Placing a block may overwrite it, and clicking it places into its
    /// cell (air, fluids, fire, tall grass, dead bushes, snow layers, vines).
    #[serde(default)]
    pub replaceable: bool,
    /// Pops off when the block below it is removed (plants, crops, rails,
    /// pressure plates, redstone, snow layers, standing signs).
    #[serde(default)]
    pub needs_ground: bool,
    #[serde(default)]
    pub placement: Placement,
    /// What right-clicking it does, if anything.
    #[serde(default)]
    pub activate: Option<Activate>,
    /// Liquids swim, slow mining and color the fog.
    #[serde(default)]
    pub fluid: Option<Fluid>,
    /// Holds back water and lava without being solid to walk into (signs,
    /// pressure plates, cobwebs, sugar cane: 1.0.0's solid materials and
    /// `blockBlocksFlow` list). Solid blocks always do.
    #[serde(default)]
    pub blocks_flow: bool,
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
    /// Per-data-value overrides; data values not listed use the defaults above.
    #[serde(default)]
    pub variants: BTreeMap<u8, Variant>,
}

fn default_true() -> bool {
    true
}

fn default_slipperiness() -> f32 {
    0.6
}

impl BlockDef {
    pub fn textures_for(&self, data: u8) -> Option<&Textures> {
        self.variants
            .get(&data)
            .and_then(|v| v.textures.as_ref())
            .or(self.textures.as_ref())
    }

    pub fn tint_for(&self, data: u8) -> Tint {
        self.variants
            .get(&data)
            .and_then(|v| v.tint)
            .unwrap_or(self.tint)
    }

    pub fn light_opacity(&self) -> u8 {
        self.opacity
            .unwrap_or(if self.is_opaque_cube() { 15 } else { 0 })
    }

    /// Offset to the block holding this one up, if it pops off without it.
    pub fn support(&self, data: u8) -> Option<IVec3> {
        match self.placement {
            Placement::Torch => Some(match data {
                1 => IVec3::NEG_X,
                2 => IVec3::X,
                3 => IVec3::NEG_Z,
                4 => IVec3::Z,
                _ => IVec3::NEG_Y,
            }),
            Placement::Plain | Placement::Facing | Placement::Slab { .. } => {
                self.needs_ground.then_some(IVec3::NEG_Y)
            }
            // The lower half stands on the ground, the upper on the lower.
            Placement::Door => Some(IVec3::NEG_Y),
            // Each half goes with the other.
            Placement::Bed => {
                let toward_head = bed_direction(data & 3);
                Some(if data & 8 == 0 {
                    toward_head
                } else {
                    -toward_head
                })
            }
        }
    }

    /// Every neighbor whose removal takes this block with it: what holds
    /// it up, plus a door's upper half for its lower half.
    pub fn held_by(&self, data: u8) -> Vec<IVec3> {
        let mut by: Vec<IVec3> = self.support(data).into_iter().collect();
        if self.placement == Placement::Door && data & 8 == 0 {
            by.push(IVec3::Y);
        }
        by
    }

    /// The part of its cell entities bump into, as (min, max) within the
    /// cell, or `None` if they pass through (1.0.0's collision bounds; a
    /// bed is 9/16 tall and a door a 3/16 slab on one side).
    pub fn collision_box(&self, data: u8) -> Option<([f32; 3], [f32; 3])> {
        if !self.solid {
            return None;
        }
        Some(match self.shape {
            Shape::Bed => ([0.0; 3], [1.0, 9.0 / 16.0, 1.0]),
            Shape::Slab => ([0.0; 3], [1.0, 0.5, 1.0]),
            Shape::Door => door_box(data),
            _ => ([0.0; 3], [1.0; 3]),
        })
    }

    /// The face its front texture is on, for blocks placed facing the player.
    pub fn front(&self, data: u8) -> Option<Face> {
        if self.placement != Placement::Facing {
            return None;
        }
        Some(match data {
            2 => Face::North,
            4 => Face::West,
            5 => Face::East,
            _ => Face::South,
        })
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
}

/// All block definitions, indexed by ID.
pub struct BlockRegistry {
    blocks: Vec<Option<BlockDef>>,
}

impl BlockRegistry {
    pub fn load(path: &Path) -> Result<Self> {
        let text =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        Self::from_ron(&text).with_context(|| format!("parsing {}", path.display()))
    }

    pub fn from_ron(text: &str) -> Result<Self> {
        let defs: Vec<BlockDef> = ron::from_str(text)?;
        let mut blocks: Vec<Option<BlockDef>> = Vec::new();
        for def in defs {
            let index = def.id as usize;
            if let Some(&data) = def.variants.keys().find(|&&d| d as usize >= DATA_VALUES) {
                bail!("block {} has out-of-range data value {data}", def.name);
            }
            if def.opacity.is_some_and(|o| o > 15) {
                bail!("block {} has opacity above 15", def.name);
            }
            if def.light > 15 {
                bail!("block {} has light level {} above 15", def.name, def.light);
            }
            if blocks.len() <= index {
                blocks.resize(index + 1, None);
            }
            if let Some(existing) = &blocks[index] {
                bail!(
                    "block ID {} used by both {} and {}",
                    def.id,
                    existing.name,
                    def.name
                );
            }
            blocks[index] = Some(def);
        }
        Ok(Self { blocks })
    }

    pub fn get(&self, id: BlockId) -> Option<&BlockDef> {
        self.blocks.get(id as usize)?.as_ref()
    }

    /// A readable name like "planks" or "wool (orange)"; unknown IDs show the number.
    pub fn display_name(&self, block: BlockState) -> String {
        let Some(def) = self.get(block.id()) else {
            return format!("block {}", block.id());
        };
        let base = def.name.replace('_', " ");
        match def
            .variants
            .get(&block.data())
            .and_then(|v| v.name.as_deref())
        {
            Some(variant) => format!("{base} ({})", variant.replace('_', " ")),
            None => base,
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = &BlockDef> {
        self.blocks.iter().flatten()
    }

    /// Texture names referenced by some block or variant that `exists` rejects.
    pub fn missing_textures(&self, exists: impl Fn(&str) -> bool) -> BTreeSet<String> {
        self.iter()
            .flat_map(|def| {
                def.textures
                    .iter()
                    .chain(def.variants.values().filter_map(|v| v.textures.as_ref()))
            })
            .flat_map(|textures| Face::ALL.map(|face| textures.get(face)))
            .flatten()
            .filter(|name| !exists(name))
            .map(str::to_owned)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_names_are_readable() {
        let registry = BlockRegistry::load(&crate::assets::dir().join("data/blocks.ron")).unwrap();
        assert_eq!(registry.display_name(BlockState::new(98, 0)), "stonebrick");
        assert_eq!(
            registry.display_name(BlockState::new(35, 1)),
            "wool (orange)"
        );
        assert_eq!(
            registry.display_name(BlockState::new(31, 1)),
            "tallgrass (grass)"
        );
        assert_eq!(
            registry.display_name(BlockState::new(4000, 0)),
            "block 4000"
        );
    }

    #[test]
    fn block_state_packs_id_and_data() {
        let state = BlockState::new(35, 14);
        assert_eq!((state.id(), state.data()), (35, 14));
        assert_eq!(BlockState::new(4095, 15).id(), 4095);
        assert!(BlockState::AIR.is_air());
        assert!(!state.is_air());
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
    fn variants_override_textures_and_tint() {
        let ron = r#"#![enable(implicit_some)]
            [(id: 18, name: "leaves", hardness: 0.2, textures: (all: "oak"), tint: Foliage,
              variants: {2: (textures: (all: "birch"), tint: Fixed(0x80A755))})]"#;
        let registry = BlockRegistry::from_ron(ron).unwrap();
        let leaves = registry.get(18).unwrap();
        assert_eq!(leaves.textures_for(0).unwrap().get(Face::Top), Some("oak"));
        assert_eq!(
            leaves.textures_for(2).unwrap().get(Face::Top),
            Some("birch")
        );
        assert_eq!(leaves.tint_for(0), Tint::Foliage);
        assert_eq!(leaves.tint_for(2), Tint::Fixed(0x80A755));
    }

    #[test]
    fn invalid_definitions_are_rejected() {
        let duplicate = r#"[(id: 1, name: "a", hardness: 1.0), (id: 1, name: "b", hardness: 1.0)]"#;
        assert!(BlockRegistry::from_ron(duplicate).is_err());
        let bad_data = r#"[(id: 1, name: "a", hardness: 1.0, variants: {16: ()})]"#;
        assert!(BlockRegistry::from_ron(bad_data).is_err());
        let bad_light = r#"[(id: 1, name: "a", hardness: 1.0, light: 16)]"#;
        assert!(BlockRegistry::from_ron(bad_light).is_err());
    }

    fn real_registry() -> BlockRegistry {
        BlockRegistry::load(&crate::assets::dir().join("data/blocks.ron")).unwrap()
    }

    #[test]
    fn every_1_0_0_block_id_is_defined() {
        let registry = real_registry();
        for id in 0..=122 {
            assert!(
                registry.get(id).is_some(),
                "block {id} missing from blocks.ron"
            );
        }
        // Plus the wooden slabs from 1.3 (125, 126).
        assert_eq!(registry.iter().count(), 125);
        assert_eq!(registry.get(126).unwrap().name, "wooden_slab");
        assert_eq!(registry.get(2).unwrap().name, "grass");
    }

    #[test]
    fn spot_check_1_0_0_properties() {
        let registry = real_registry();
        let block = |id| registry.get(id).unwrap();
        assert_eq!(block(1).hardness, 1.5);
        assert!(block(7).hardness < 0.0, "bedrock is unbreakable");
        assert_eq!(block(49).hardness, 50.0);
        assert_eq!(block(50).light, 14);
        assert_eq!(block(89).light, 15);
        assert!(block(1).is_opaque_cube());
        assert!(!block(20).is_opaque_cube(), "glass is see-through");
        assert!(!block(37).solid, "flowers have no collision");
        assert_eq!(block(37).shape, Shape::Cross);
        assert_eq!(block(0).shape, Shape::None);
    }

    #[test]
    fn doors_cover_one_side_and_swing_a_quarter_when_opened() {
        let t = 3.0 / 16.0;
        // Direction 3 (placed looking north) closes along the south side.
        assert_eq!(door_box(3), ([0.0, 0.0, 1.0 - t], [1.0, 1.0, 1.0]));
        // Open, it swings to the west side; the upper half matches.
        assert_eq!(door_box(3 | 4), ([0.0; 3], [t, 1.0, 1.0]));
        assert_eq!(door_box(3 | 4 | 8), door_box(3 | 4));
        let blocks = real_registry();
        let door = blocks.get(64).unwrap();
        assert_eq!(door.collision_box(0), Some(door_box(0)));
        let bed = blocks.get(26).unwrap();
        assert_eq!(bed.collision_box(8).unwrap().1[1], 9.0 / 16.0);
        assert_eq!(blocks.get(37).unwrap().collision_box(0), None);
        assert_eq!(door.held_by(0), [IVec3::NEG_Y, IVec3::Y]);
        assert_eq!(door.held_by(8), [IVec3::NEG_Y]);
    }

    #[test]
    fn furnaces_and_chests_turn_their_front_by_data() {
        let blocks = real_registry();
        let furnace = blocks.get(61).unwrap();
        assert_eq!(furnace.front(2), Some(Face::North));
        assert_eq!(furnace.front(5), Some(Face::East));
        assert_eq!(furnace.front(0), Some(Face::South), "the item's front");
        assert_eq!(blocks.get(54).unwrap().front(4), Some(Face::West));
        assert_eq!(blocks.get(1).unwrap().front(2), None);
    }

    /// Keep in sync with the missing-texture list in docs/DECISIONS.md.
    #[test]
    fn only_known_block_textures_are_missing() {
        let dir = crate::assets::dir().join("textures/blocks");
        let missing =
            real_registry().missing_textures(|name| dir.join(format!("{name}.png")).is_file());
        let expected = ["end_portal", "sign"];
        assert_eq!(missing, expected.map(String::from).into());
    }
}
