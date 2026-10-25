//! The dimensions a world has. Only the Overworld is played so far; the
//! Nether (M24) and the End (M27) get their own chunks and region files.

use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Dimension {
    #[default]
    Overworld,
    Nether,
    End,
}

impl Dimension {
    pub const ALL: [Dimension; 3] = [Dimension::Overworld, Dimension::Nether, Dimension::End];

    /// The lowest block and one past the highest you can build at: the
    /// Overworld's modern −64 to 319 (M34), the Nether's and End's 0 to 255.
    pub fn min_y(self) -> i32 {
        match self {
            Dimension::Overworld => crate::config::MIN_Y,
            Dimension::Nether | Dimension::End => 0,
        }
    }

    pub fn max_y(self) -> i32 {
        match self {
            Dimension::Overworld => crate::config::MAX_Y,
            Dimension::Nether | Dimension::End => 256,
        }
    }

    /// Modern Minecraft's name for it.
    pub fn name(self) -> &'static str {
        match self {
            Dimension::Overworld => "minecraft:overworld",
            Dimension::Nether => "minecraft:the_nether",
            Dimension::End => "minecraft:the_end",
        }
    }

    /// Its region files' folder inside a world folder, laid out as in
    /// Minecraft: the Overworld's at the top, the others in `DIM-1` and
    /// `DIM1`.
    pub fn region_folder(self) -> &'static str {
        match self {
            Dimension::Overworld => "region",
            Dimension::Nether => "DIM-1/region",
            Dimension::End => "DIM1/region",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_dimension_has_its_own_region_folder() {
        let folders: std::collections::BTreeSet<_> =
            Dimension::ALL.iter().map(|d| d.region_folder()).collect();
        assert_eq!(folders.len(), 3);
        assert_eq!(Dimension::default().region_folder(), "region");
        assert_eq!(Dimension::Nether.name(), "minecraft:the_nether");
    }
}
