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

    /// The other mode, for the create-world button that cycles through them.
    pub fn next(self) -> Self {
        match self {
            GameMode::Survival => GameMode::Creative,
            GameMode::Creative => GameMode::Survival,
        }
    }
}
