//! Block state properties, named and valued as in modern Java Edition's
//! `BlockStateProperties` (`facing=north`, `half=bottom`, `age=7`...).
//! Each block lists the properties it has in `blocks.ron`; a `BlockState`
//! holds one value of each.

use serde::Deserialize;

use super::Face;

/// A block state property. Several share a name but not their values, as
/// in Minecraft: `facing` is horizontal for stairs but six-way for
/// pistons, `half` is top/bottom for stairs but upper/lower for doors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
pub enum Prop {
    /// `facing`: the four horizontal directions.
    Facing,
    /// `facing`: all six directions.
    FacingAll,
    /// `axis`: x, y or z (logs).
    Axis,
    /// `axis`: x or z (nether portals).
    HorizontalAxis,
    /// `half`: top or bottom (stairs, trapdoors).
    Half,
    /// `half`: upper or lower (doors).
    DoubleBlockHalf,
    /// `part`: head or foot (beds).
    BedPart,
    /// `type`: top, bottom or double (slabs).
    SlabType,
    /// `type`: single, left or right (chests).
    ChestType,
    /// `type`: normal or sticky (piston heads).
    PistonType,
    /// `shape` of stairs: straight or a corner.
    StairsShape,
    /// `shape` of a rail, curves included.
    RailShape,
    /// `shape` of a powered, detector or activator rail (no curves).
    StraightRailShape,
    /// `hinge`: left or right (doors).
    Hinge,
    /// `face`: floor, wall or ceiling (levers, buttons).
    AttachFace,
    Open,
    Powered,
    Waterlogged,
    Lit,
    Snowy,
    Occupied,
    Extended,
    Short,
    Locked,
    Triggered,
    Eye,
    Persistent,
    Unstable,
    HasRecord,
    InWall,
    /// Connections on each side (fences, panes, vines, fire, mushroom blocks).
    Up,
    Down,
    North,
    East,
    South,
    West,
    HasBottle0,
    HasBottle1,
    HasBottle2,
    /// A wall's side: none, low or tall.
    WallNorth,
    WallEast,
    WallSouth,
    WallWest,
    /// Redstone dust's side: up, side or none.
    WireNorth,
    WireEast,
    WireSouth,
    WireWest,
    Age1,
    Age2,
    Age3,
    Age5,
    Age7,
    Age15,
    Age25,
    /// `level`: fluid level, 0 a source.
    Level,
    /// `level` of water in a cauldron, 1–3.
    LevelCauldron,
    Power,
    Rotation,
    Layers,
    Bites,
    Moisture,
    Delay,
    /// `distance` from leaves to a log, 1–7.
    Distance,
    Stage,
    Note,
    Instrument,
}

const BOOL: &[&str] = &["false", "true"];
const HORIZONTAL: &[&str] = &["north", "south", "west", "east"];
const ALL_DIRECTIONS: &[&str] = &["north", "east", "south", "west", "up", "down"];
const NUMBERS: &[&str] = &[
    "0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "10", "11", "12", "13", "14", "15", "16",
    "17", "18", "19", "20", "21", "22", "23", "24", "25",
];
const WALL_SIDE: &[&str] = &["none", "low", "tall"];
const WIRE_SIDE: &[&str] = &["up", "side", "none"];

/// The faces `Prop::Facing`'s values name, in order.
const HORIZONTAL_FACES: [Face; 4] = [Face::North, Face::South, Face::West, Face::East];
/// The faces `Prop::FacingAll`'s values name, in order.
const ALL_FACES: [Face; 6] = [
    Face::North,
    Face::East,
    Face::South,
    Face::West,
    Face::Top,
    Face::Bottom,
];

impl Prop {
    /// The name written in block states, e.g. `facing`.
    pub fn name(self) -> &'static str {
        use Prop::*;
        match self {
            Facing | FacingAll => "facing",
            Axis | HorizontalAxis => "axis",
            Half | DoubleBlockHalf => "half",
            BedPart => "part",
            SlabType | ChestType | PistonType => "type",
            StairsShape | RailShape | StraightRailShape => "shape",
            Hinge => "hinge",
            AttachFace => "face",
            Open => "open",
            Powered => "powered",
            Waterlogged => "waterlogged",
            Lit => "lit",
            Snowy => "snowy",
            Occupied => "occupied",
            Extended => "extended",
            Short => "short",
            Locked => "locked",
            Triggered => "triggered",
            Eye => "eye",
            Persistent => "persistent",
            Unstable => "unstable",
            HasRecord => "has_record",
            InWall => "in_wall",
            Up => "up",
            Down => "down",
            North | WallNorth | WireNorth => "north",
            East | WallEast | WireEast => "east",
            South | WallSouth | WireSouth => "south",
            West | WallWest | WireWest => "west",
            HasBottle0 => "has_bottle_0",
            HasBottle1 => "has_bottle_1",
            HasBottle2 => "has_bottle_2",
            Age1 | Age2 | Age3 | Age5 | Age7 | Age15 | Age25 => "age",
            Level | LevelCauldron => "level",
            Power => "power",
            Rotation => "rotation",
            Layers => "layers",
            Bites => "bites",
            Moisture => "moisture",
            Delay => "delay",
            Distance => "distance",
            Stage => "stage",
            Note => "note",
            Instrument => "instrument",
        }
    }

    /// Every value, in Minecraft's order.
    pub fn values(self) -> &'static [&'static str] {
        use Prop::*;
        match self {
            Facing => HORIZONTAL,
            FacingAll => ALL_DIRECTIONS,
            Axis => &["x", "y", "z"],
            HorizontalAxis => &["x", "z"],
            Half => &["top", "bottom"],
            DoubleBlockHalf => &["upper", "lower"],
            BedPart => &["head", "foot"],
            SlabType => &["top", "bottom", "double"],
            ChestType => &["single", "left", "right"],
            PistonType => &["normal", "sticky"],
            StairsShape => &[
                "straight",
                "inner_left",
                "inner_right",
                "outer_left",
                "outer_right",
            ],
            RailShape => &[
                "north_south",
                "east_west",
                "ascending_east",
                "ascending_west",
                "ascending_north",
                "ascending_south",
                "south_east",
                "south_west",
                "north_west",
                "north_east",
            ],
            StraightRailShape => &[
                "north_south",
                "east_west",
                "ascending_east",
                "ascending_west",
                "ascending_north",
                "ascending_south",
            ],
            Hinge => &["left", "right"],
            AttachFace => &["floor", "wall", "ceiling"],
            WallNorth | WallEast | WallSouth | WallWest => WALL_SIDE,
            WireNorth | WireEast | WireSouth | WireWest => WIRE_SIDE,
            Age1 => &NUMBERS[..2],
            Age2 => &NUMBERS[..3],
            Age3 => &NUMBERS[..4],
            Age5 => &NUMBERS[..6],
            Age7 => &NUMBERS[..8],
            Age15 => &NUMBERS[..16],
            Age25 => &NUMBERS[..26],
            Level => &NUMBERS[..16],
            LevelCauldron => &NUMBERS[1..4],
            Power => &NUMBERS[..16],
            Rotation => &NUMBERS[..16],
            Layers => &NUMBERS[1..9],
            Bites => &NUMBERS[..7],
            Moisture => &NUMBERS[..8],
            Delay => &NUMBERS[1..5],
            Distance => &NUMBERS[1..8],
            Stage => &NUMBERS[..2],
            Note => &NUMBERS[..25],
            Instrument => &[
                "harp",
                "basedrum",
                "snare",
                "hat",
                "bass",
                "flute",
                "bell",
                "guitar",
                "chime",
                "xylophone",
                "iron_xylophone",
                "cow_bell",
                "didgeridoo",
                "bit",
                "banjo",
                "pling",
            ],
            _ => BOOL,
        }
    }

    /// The value index blocks start with unless `blocks.ron` says
    /// otherwise: Minecraft's usual default (`facing=north`, `half=bottom`,
    /// `axis=y`, `face=wall`, sides `none`, numbers at their lowest except
    /// leaf `distance` 7, booleans false).
    pub fn default_index(self) -> u8 {
        use Prop::*;
        match self {
            Axis | Half | DoubleBlockHalf | BedPart | SlabType | AttachFace => 1,
            WallNorth | WallEast | WallSouth | WallWest => 0,
            WireNorth | WireEast | WireSouth | WireWest => 2,
            Distance => 6,
            _ => 0,
        }
    }

    /// The index of `value`, if it's one of this property's values.
    pub fn index_of(self, value: &str) -> Option<u8> {
        self.values()
            .iter()
            .position(|&v| v == value)
            .map(|i| i as u8)
    }

    /// The direction a `facing` value names.
    pub fn face(self, index: u8) -> Face {
        match self {
            Prop::FacingAll => ALL_FACES[index as usize],
            _ => HORIZONTAL_FACES[index as usize],
        }
    }

    /// The value naming `face`, for `facing` properties; `None` for a
    /// vertical face on a horizontal property.
    pub fn index_of_face(self, face: Face) -> Option<u8> {
        let faces: &[Face] = match self {
            Prop::FacingAll => &ALL_FACES,
            _ => &HORIZONTAL_FACES,
        };
        faces.iter().position(|&f| f == face).map(|i| i as u8)
    }

    /// The number a numeric value stands for (`layers` index 0 is 1).
    pub fn number(self, index: u8) -> u8 {
        self.values()[index as usize].parse().unwrap_or(index)
    }

    /// The index of numeric value `number`, if it's in range.
    pub fn index_of_number(self, number: u8) -> Option<u8> {
        let first: u8 = self.values()[0].parse().ok()?;
        let index = number.checked_sub(first)?;
        ((index as usize) < self.values().len()).then_some(index)
    }

    /// The property for the side of a fence, pane, vine or fire facing `face`.
    pub fn side(face: Face) -> Prop {
        match face {
            Face::North => Prop::North,
            Face::East => Prop::East,
            Face::South => Prop::South,
            Face::West => Prop::West,
            Face::Top => Prop::Up,
            Face::Bottom => Prop::Down,
        }
    }

    /// The property for a wall's side toward horizontal `face`.
    pub fn wall_side(face: Face) -> Prop {
        match face {
            Face::North => Prop::WallNorth,
            Face::East => Prop::WallEast,
            Face::South => Prop::WallSouth,
            _ => Prop::WallWest,
        }
    }

    /// The property for redstone dust's side toward horizontal `face`.
    pub fn wire_side(face: Face) -> Prop {
        match face {
            Face::North => Prop::WireNorth,
            Face::East => Prop::WireEast,
            Face::South => Prop::WireSouth,
            _ => Prop::WireWest,
        }
    }
}

/// Values of `Prop::Half`.
pub mod half {
    pub const TOP: u8 = 0;
    pub const BOTTOM: u8 = 1;
}

/// Values of `Prop::DoubleBlockHalf`.
pub mod door_half {
    pub const UPPER: u8 = 0;
    pub const LOWER: u8 = 1;
}

/// Values of `Prop::BedPart`.
pub mod bed_part {
    pub const HEAD: u8 = 0;
    pub const FOOT: u8 = 1;
}

/// Values of `Prop::SlabType`.
pub mod slab_type {
    pub const TOP: u8 = 0;
    pub const BOTTOM: u8 = 1;
    pub const DOUBLE: u8 = 2;
}

/// Values of `Prop::StairsShape`.
pub mod stairs_shape {
    pub const STRAIGHT: u8 = 0;
    pub const INNER_LEFT: u8 = 1;
    pub const INNER_RIGHT: u8 = 2;
    pub const OUTER_LEFT: u8 = 3;
    pub const OUTER_RIGHT: u8 = 4;
}

/// Values of `Prop::Hinge`.
pub mod hinge {
    pub const LEFT: u8 = 0;
    pub const RIGHT: u8 = 1;
}

/// Values of `Prop::AttachFace`.
pub mod attach_face {
    pub const FLOOR: u8 = 0;
    pub const WALL: u8 = 1;
    pub const CEILING: u8 = 2;
}

/// Values of `Prop::Axis`.
pub mod axis {
    pub const X: u8 = 0;
    pub const Y: u8 = 1;
    pub const Z: u8 = 2;
}

/// Values of the `Prop::Wall*` sides.
pub mod wall_side {
    pub const NONE: u8 = 0;
    pub const LOW: u8 = 1;
    pub const TALL: u8 = 2;
}

/// Values of the `Prop::Wire*` sides.
pub mod wire_side {
    pub const UP: u8 = 0;
    pub const SIDE: u8 = 1;
    pub const NONE: u8 = 2;
}

/// Values of `Prop::RailShape` (and the first six of `StraightRailShape`).
pub mod rail_shape {
    pub const NORTH_SOUTH: u8 = 0;
    pub const EAST_WEST: u8 = 1;
    pub const ASCENDING_EAST: u8 = 2;
    pub const ASCENDING_WEST: u8 = 3;
    pub const ASCENDING_NORTH: u8 = 4;
    pub const ASCENDING_SOUTH: u8 = 5;
    pub const SOUTH_EAST: u8 = 6;
    pub const SOUTH_WEST: u8 = 7;
    pub const NORTH_WEST: u8 = 8;
    pub const NORTH_EAST: u8 = 9;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_and_numbers_follow_minecraft() {
        assert_eq!(
            Prop::Axis.values()[Prop::Axis.default_index() as usize],
            "y"
        );
        assert_eq!(Prop::Distance.number(Prop::Distance.default_index()), 7);
        assert_eq!(
            Prop::AttachFace.values()[Prop::AttachFace.default_index() as usize],
            "wall"
        );
        assert_eq!(Prop::Layers.number(0), 1);
        assert_eq!(Prop::Layers.index_of_number(8), Some(7));
        assert_eq!(Prop::Layers.index_of_number(0), None);
        assert_eq!(Prop::Age7.index_of_number(8), None);
        assert_eq!(Prop::LevelCauldron.values(), ["1", "2", "3"]);
    }

    #[test]
    fn facing_values_round_trip_through_faces() {
        for prop in [Prop::Facing, Prop::FacingAll] {
            for index in 0..prop.values().len() as u8 {
                assert_eq!(prop.index_of_face(prop.face(index)), Some(index));
            }
        }
        assert_eq!(Prop::Facing.index_of_face(Face::Top), None);
        assert_eq!(Prop::FacingAll.face(4), Face::Top);
        assert_eq!(Prop::Facing.values()[0], "north");
    }
}
