//! The messages between a server and its clients. Single player sends them
//! through in-process channels as they are; LAN (M37) encodes them.

use std::sync::Arc;

use crate::{
    bindings::Action,
    game::PlayerState,
    level::{Difficulty, GameRules},
    ui::screens::ScreenAction,
    world::{
        chunk::{Chunk, ChunkPos},
        dimension::Dimension,
    },
};

/// Bumped whenever a message changes, so mismatched games refuse each
/// other instead of misreading.
pub const PROTOCOL_VERSION: u32 = 1;

/// What a client tells the server.
#[derive(Debug, Clone)]
pub enum ClientMsg {
    /// The first message: who is joining, and which protocol they speak.
    Login { version: u32, name: String },
    /// Settings the server plays the player by (sent on joining and when
    /// they change).
    Settings(PlayerSettings),
    /// Where the player looks, sent every tick it changes.
    Look { yaw: f32, pitch: f32 },
    /// A bound key or button pressed or let go.
    Action {
        action: Action,
        pressed: bool,
        repeat: bool,
        grabbed: bool,
    },
    /// The mouse wheel moved the hotbar to this slot.
    SelectSlot(usize),
    /// Something done on an open screen.
    Screen(ScreenAction),
    /// The inventory key opened the inventory.
    OpenInventory,
    /// The open screen closed (Escape or the inventory key).
    CloseScreen,
    /// Escape during the credits.
    FinishCredits,
    /// The mouse left the game: every held key and button lets go.
    ReleaseInput,
    /// Leaving.
    Disconnect,
}

/// The options that change how the server plays a player.
#[derive(Debug, Clone, PartialEq)]
pub struct PlayerSettings {
    /// Chunks sent around the player in each direction.
    pub view_distance: i32,
    pub auto_jump: bool,
    pub toggle_sneak: bool,
    pub toggle_sprint: bool,
    pub left_handed: bool,
    pub skin_layers: crate::render::player::OuterLayers,
    pub particles: crate::options::Particles,
}

/// What the server tells a client.
pub enum ServerMsg {
    /// Joined; the world's seed (for F3 and `/seed`) and its dimension.
    Welcome { seed: Option<u64> },
    /// Not joined, and why (a full world, a taken name, another version).
    Refused(String),
    /// A chunk to show, new or changed. `urgent` ones (a player's own
    /// edits) are remeshed at once; the rest in the background.
    Chunk { chunk: Arc<Chunk>, urgent: bool },
    /// A chunk out of the player's view.
    Unload(ChunkPos),
    /// The player went to another dimension: every chunk is forgotten.
    Dimension(Dimension),
    /// The world after a tick, as the player sees it.
    Snapshot(Box<Snapshot>),
    /// Sounds to play, already filtered to the ones the player hears.
    Sounds(Vec<crate::audio::Cue>),
    /// The server is going away, or sent this player off.
    Disconnect(String),
}

/// One tick of the world for one player: their own state and everything
/// they can see move.
pub struct Snapshot {
    pub time: u64,
    pub difficulty: Difficulty,
    pub rules: GameRules,
    pub player: PlayerState,
    pub entities: Entities,
}

/// Everything that moves in the player's dimension.
#[derive(Debug, Clone, Default)]
pub struct Entities {
    pub mobs: Vec<crate::entity::mob::Mob>,
    pub items: Vec<crate::entity::item::ItemEntity>,
    pub orbs: Vec<crate::entity::orb::XpOrb>,
    pub particles: Vec<crate::entity::particle::Particle>,
    pub effects: Vec<crate::entity::effects::Effect>,
    pub arrows: Vec<crate::entity::arrow::Arrow>,
    pub fireballs: Vec<crate::entity::fireball::Fireball>,
    pub bullets: Vec<crate::entity::bullet::ShulkerBullet>,
    pub thrown: Vec<crate::entity::thrown::ThrownPotion>,
    pub falling: Vec<crate::entity::falling::FallingBlock>,
    pub carts: Vec<crate::entity::minecart::Minecart>,
    pub boats: Vec<crate::entity::boat::Boat>,
    pub primed: Vec<crate::entity::tnt::PrimedTnt>,
    pub eyes: Vec<crate::entity::eye::EyeOfEnder>,
    pub dragon: Option<crate::entity::dragon::EnderDragon>,
    pub crystals: Vec<crate::entity::crystal::EndCrystal>,
    pub dragon_fireballs: Vec<crate::entity::breath::DragonFireball>,
    pub breath: Vec<crate::entity::breath::BreathCloud>,
}
