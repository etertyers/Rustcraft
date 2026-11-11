//! Lua mods in the browser build (M70): not available. This keeps the types
//! the game uses (`Event`, `Host`, `Runtime`, ...) so no game code needs a
//! `cfg`; `Runtime::start` always fails, so no mod ever runs, and
//! `register` adds nothing. The Mods screen says why
//! (`mods::LUA_UNAVAILABLE`).

use glam::{DVec3, IVec3};

use super::ModSet;

pub const INSTRUCTION_LIMIT: u64 = 2_000_000;
pub const MEMORY_LIMIT: usize = 64 << 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Registration,
    Server,
    Client,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PlayerInfo {
    pub index: usize,
    pub name: String,
    pub feet: DVec3,
    pub dimension: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    ServerTick {
        tick: u64,
    },
    PlayerJoin(PlayerInfo),
    PlayerLeave(PlayerInfo),
    BlockBreak {
        player: usize,
        at: IVec3,
        block: String,
    },
    BlockPlace {
        player: usize,
        at: IVec3,
        block: String,
    },
    EntityDamage {
        entity: String,
        amount: f32,
        source: String,
    },
    EntityDeath {
        entity: String,
        source: String,
    },
    Chat {
        player: usize,
        name: String,
        message: String,
    },
    ItemCrafted {
        player: usize,
        item: String,
        count: u32,
    },
}

impl Event {
    pub fn name(&self) -> &'static str {
        match self {
            Event::ServerTick { .. } => "server_tick",
            Event::PlayerJoin(_) => "player_join",
            Event::PlayerLeave(_) => "player_leave",
            Event::BlockBreak { .. } => "block_break",
            Event::BlockPlace { .. } => "block_place",
            Event::EntityDamage { .. } => "entity_damage",
            Event::EntityDeath { .. } => "entity_death",
            Event::Chat { .. } => "chat",
            Event::ItemCrafted { .. } => "item_crafted",
        }
    }
}

pub trait Host {
    fn tick(&self) -> u64;
    fn block_at(&self, at: IVec3) -> Option<String>;
    fn set_block(&mut self, at: IVec3, state: &str) -> Result<(), String>;
    fn give(&mut self, player: usize, item: &str, count: u32) -> Result<(), String>;
    fn say(&mut self, player: Option<usize>, text: &str);
    fn spawn(&mut self, kind: &str, at: DVec3) -> Result<(), String>;
    fn players(&self) -> Vec<PlayerInfo>;
    fn loot(&mut self, table: &str) -> Result<Vec<(String, u32)>, String>;
}

pub const ACTIONBAR: &str = "\u{0}actionbar";

pub trait ClientHost {
    fn hud_text(&mut self, id: &str, text: Option<String>, seconds: Option<f32>);
    fn actionbar(&mut self, text: String, ticks: u32) {
        self.hud_text(ACTIONBAR, Some(text), Some(ticks as f32 / 20.0));
    }
    fn brightness_override(&mut self, level: Option<u8>);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockEvent {
    Use,
    Place,
    Break,
    RandomTick,
}

/// Why Lua can't start here.
#[derive(Debug)]
pub struct Unavailable;

impl std::fmt::Display for Unavailable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(super::LUA_UNAVAILABLE)
    }
}

pub struct Runtime;

pub fn short(err: &Unavailable) -> String {
    err.to_string()
}

pub fn register(_set: &mut ModSet) {}

impl Runtime {
    pub fn start(_side: Side, _set: &ModSet) -> Result<Self, Unavailable> {
        Err(Unavailable)
    }
    pub fn broken(&self) -> Vec<String> {
        Vec::new()
    }
    pub fn wants(&self, _event: &str) -> bool {
        false
    }
    pub fn fire(&self, _host: &mut dyn Host, _event: &Event) {}
    pub fn tick_timers_server(&mut self, _host: &mut dyn Host) {}
    pub fn tick_timers_client(&mut self, _host: &mut dyn ClientHost) {}
    pub fn has_block_callback(&self, _name: &str, _event: BlockEvent) -> bool {
        false
    }
    pub fn block_event(
        &self,
        _host: &mut dyn Host,
        _name: &str,
        _event: BlockEvent,
        _at: IVec3,
        _player: usize,
    ) {
    }
    pub fn has_item_callback(&self, _name: &str) -> bool {
        false
    }
    pub fn item_used(
        &self,
        _host: &mut dyn Host,
        _name: &str,
        _player: usize,
        _aimed: Option<IVec3>,
    ) {
    }
    pub fn run_command(
        &self,
        _host: &mut dyn Host,
        _name: &str,
        _player: usize,
        _args: &[&str],
    ) -> Option<Result<Vec<String>, String>> {
        None
    }
    pub fn key_pressed(&self, _host: &mut dyn ClientHost, _id: &str) {}
    pub fn eval_as(
        &self,
        _host: &mut dyn Host,
        _mod_id: &str,
        _code: &str,
    ) -> Result<String, String> {
        Err(super::LUA_UNAVAILABLE.into())
    }
    pub fn eval_plain(&self, _code: &str) -> Result<String, String> {
        Err(super::LUA_UNAVAILABLE.into())
    }
}
