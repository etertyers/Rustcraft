//! The messages between a server and its clients. Single player sends them
//! through in-process channels as they are; LAN (M37) encodes them.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

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
pub const PROTOCOL_VERSION: u32 = 2;

/// What a client tells the server.
#[derive(Debug, Clone, Serialize, Deserialize)]
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
    /// The player's skin as a PNG, and whether its arms are slim (sent
    /// once, on joining).
    Skin { png: Vec<u8>, slim: bool },
    /// A chat message, or a command if it starts with `/` (M38).
    Chat(String),
    /// Tab in chat: what can finish this command line (without its `/`).
    Complete(String),
    /// Leaving.
    Disconnect,
}

/// One tick of a player's movement input.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Input {
    pub seq: u32,
    pub keys: crate::player::input::MoveKeys,
    pub yaw: f32,
    pub pitch: f32,
}

/// The options that change how the server plays a player.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
#[derive(Serialize, Deserialize)]
pub enum ServerMsg {
    /// Joined; the world's seed (for F3 and `/seed`).
    Welcome { seed: Option<u64> },
    /// Not joined, and why (a full world, a taken name, another version).
    Refused(String),
    /// A chunk to show, new or sent again (its block entities changed).
    Chunk(#[serde(with = "chunk_bytes")] Arc<Chunk>),
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
    /// Everyone playing, the host first (sent as players come and go, for
    /// the Tab list).
    Players(Vec<String>),
    /// Another player's skin (each once, as they join or this player does).
    Skin {
        name: String,
        png: Vec<u8>,
        slim: bool,
    },
    /// A line for the chat, with `§` codes; `command` when it answers this
    /// player's own command (shown even with chat set to Commands Only).
    Chat { text: String, command: bool },
    /// The answer to a `Complete`: what the last word could be.
    Completions(Vec<String>),
    /// The server is going away, or sent this player off.
    Disconnect(String),
}

/// One tick of the world for one player: their own state and everything
/// they can see move.
#[derive(Serialize, Deserialize)]
pub struct Snapshot {
    /// The last `Input` played before this tick ended (0 before any).
    pub ack: u32,
    pub time: u64,
    pub day_offset: i64,
    pub difficulty: Difficulty,
    pub rules: GameRules,
    pub player: PlayerState,
    pub entities: Entities,
    /// The other players in sight.
    pub players: Vec<PlayerView>,
}

/// Another player as others see them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlayerView {
    pub name: String,
    /// Where they stood a tick ago and now, to draw them between.
    pub prev_feet: glam::DVec3,
    pub feet: glam::DVec3,
    /// Which way their body faces.
    pub yaw: f32,
    pub pose: crate::render::player::PlayerPose,
    pub wearer: crate::figure::Wearer,
    pub invisible: bool,
}

/// Everything that moves in the player's dimension.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
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

/// Chunks travel in their save format (their own compact encoding).
mod chunk_bytes {
    use std::sync::Arc;

    use serde::{Deserialize, Deserializer, Serializer, de::Error};

    use crate::world::{chunk::Chunk, save};

    pub fn serialize<S: Serializer>(chunk: &Arc<Chunk>, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_bytes(&save::encode_chunk(chunk))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Arc<Chunk>, D::Error> {
        let bytes = Vec::<u8>::deserialize(deserializer)?;
        save::decode_chunk(&bytes)
            .map(Arc::new)
            .map_err(|err| D::Error::custom(format!("{err}")))
    }
}
