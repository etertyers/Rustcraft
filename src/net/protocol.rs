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
    /// Each client tick: the movement keys held and where the player
    /// looks, numbered so the server can say which it has played.
    Input(Input),
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
    /// Where the player is while flying with the debug flight key (F4),
    /// which moves them between ticks.
    Fly { feet: glam::DVec3 },
    /// Leaving.
    Disconnect,
}

/// One tick of a player's movement input.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Input {
    pub seq: u32,
    pub keys: crate::player::input::MoveKeys,
    pub yaw: f32,
    pub pitch: f32,
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
    /// Joined; the world's seed (for F3 and `/seed`).
    Welcome { seed: Option<u64> },
    /// Not joined, and why (a full world, a taken name, another version).
    Refused(String),
    /// A chunk to show, new or sent again (its block entities changed).
    Chunk(Arc<Chunk>),
    /// Blocks that changed in chunks the client has, remeshed at once when
    /// `urgent` (a player's own edits) or in the background (fluids, fire).
    Blocks {
        edits: Vec<(glam::IVec3, crate::world::block::BlockState)>,
        urgent: bool,
    },
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
    /// The last `Input` played before this tick ended (0 before any).
    pub ack: u32,
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
