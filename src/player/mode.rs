//! Game modes. Every survival/creative difference should ask `GameMode`
//! here rather than checking for creative in scattered places.

use serde::{Deserialize, Serialize};

/// Chosen when a world is created and fixed for its lifetime, as in 1.0.0.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GameMode {
    #[default]
    Survival,
    Creative,
    /// Can't break or place blocks (except by `can_break`/`can_place_on`).
    Adventure,
    /// Flies through blocks, unseen, touching nothing.
    Spectator,
}

impl GameMode {
    pub fn name(self) -> &'static str {
        match self {
            GameMode::Survival => "Survival",
            GameMode::Creative => "Creative",
            GameMode::Adventure => "Adventure",
            GameMode::Spectator => "Spectator",
        }
    }

    /// Vanilla's lowercase id, for `/gamemode`, `/defaultgamemode` and saves.
    pub fn id(self) -> &'static str {
        match self {
            GameMode::Survival => "survival",
            GameMode::Creative => "creative",
            GameMode::Adventure => "adventure",
            GameMode::Spectator => "spectator",
        }
    }

    /// The mode a vanilla name or number means.
    pub fn parse(word: &str) -> Option<Self> {
        Some(match word {
            "survival" | "s" | "0" => GameMode::Survival,
            "creative" | "c" | "1" => GameMode::Creative,
            "adventure" | "a" | "2" => GameMode::Adventure,
            "spectator" | "sp" | "3" => GameMode::Spectator,
            _ => return None,
        })
    }

    pub const ALL: [GameMode; 4] = [
        GameMode::Survival,
        GameMode::Creative,
        GameMode::Adventure,
        GameMode::Spectator,
    ];

    /// Blocks break on the first hit, with no mining time.
    pub fn breaks_instantly(self) -> bool {
        self == GameMode::Creative
    }

    /// Broken blocks drop items.
    pub fn drops_items(self) -> bool {
        matches!(self, GameMode::Survival | GameMode::Adventure)
    }

    /// Placing a block uses one from the stack.
    pub fn uses_up_items(self) -> bool {
        matches!(self, GameMode::Survival | GameMode::Adventure)
    }

    /// Tools lose durability.
    pub fn wears_tools(self) -> bool {
        matches!(self, GameMode::Survival | GameMode::Adventure)
    }

    /// Every block and item is to hand: E opens the creative item list,
    /// and middle-click puts the targeted block in the hotbar.
    pub fn has_every_item(self) -> bool {
        self == GameMode::Creative
    }

    /// Health and hunger matter: damage, hunger and death (survival).
    pub fn is_vulnerable(self) -> bool {
        matches!(self, GameMode::Survival | GameMode::Adventure)
    }

    /// Double-tapping jump toggles flight.
    pub fn can_fly(self) -> bool {
        matches!(self, GameMode::Creative | GameMode::Spectator)
    }

    /// Blocks can be broken and placed freely (adventure needs item
    /// components, spectators never).
    pub fn edits_world(self) -> bool {
        matches!(self, GameMode::Survival | GameMode::Creative)
    }

    /// Adventure's `can_break` / `can_place_on` rules apply.
    pub fn needs_permission_to_edit(self) -> bool {
        self == GameMode::Adventure
    }

    /// Passes through blocks and entities, and mobs ignore the player.
    pub fn is_spectator(self) -> bool {
        self == GameMode::Spectator
    }

    /// Opens doors, levers, chests and so on, hits mobs and picks things up.
    pub fn interacts(self) -> bool {
        self != GameMode::Spectator
    }

    /// The hotbar, hearts and hunger are drawn.
    pub fn shows_hotbar(self) -> bool {
        self != GameMode::Spectator
    }

    /// The other mode, for the create-world button that cycles through them.
    pub fn next(self) -> Self {
        match self {
            GameMode::Survival => GameMode::Creative,
            _ => GameMode::Survival,
        }
    }
}
