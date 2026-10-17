//! Game modes. Every survival/creative difference should ask `GameMode`
//! here rather than checking for creative in scattered places.

use serde::{Deserialize, Serialize};

/// Chosen when a world is created and fixed for its lifetime, as in 1.0.0.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GameMode {
    #[default]
    Survival,
    Creative,
}

impl GameMode {
    pub fn name(self) -> &'static str {
        match self {
            GameMode::Survival => "Survival",
            GameMode::Creative => "Creative",
        }
    }

    /// Blocks break on the first hit, with no mining time.
    pub fn breaks_instantly(self) -> bool {
        self == GameMode::Creative
    }

    /// Broken blocks drop items.
    pub fn drops_items(self) -> bool {
        self == GameMode::Survival
    }

    /// Placing a block uses one from the stack.
    pub fn uses_up_items(self) -> bool {
        self == GameMode::Survival
    }

    /// Tools lose durability.
    pub fn wears_tools(self) -> bool {
        self == GameMode::Survival
    }

    /// Every block and item is to hand: E opens the creative item list,
    /// and middle-click puts the targeted block in the hotbar.
    pub fn has_every_item(self) -> bool {
        self == GameMode::Creative
    }

    /// Double-tapping jump toggles flight.
    pub fn can_fly(self) -> bool {
        self == GameMode::Creative
    }

    /// The other mode, for the create-world button that cycles through them.
    pub fn next(self) -> Self {
        match self {
            GameMode::Survival => GameMode::Creative,
            GameMode::Creative => GameMode::Survival,
        }
    }
}
