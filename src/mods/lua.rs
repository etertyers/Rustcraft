//! The Lua side of mods: a sandboxed Lua 5.4 per side (the server's, the
//! client's, and one used only while the game starts to collect what
//! `main.lua` registers). Each mod runs in its own environment with an
//! `rc` table of the API; the functions that reach the world exist only
//! during a call, handed to the mod by the `Host` the game passes in.

use std::{
    cell::RefCell,
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
};

use glam::{DVec3, IVec3};
use log::info;
use mlua::{
    Function, HookTriggers, Lua, LuaOptions, LuaSerdeExt, MultiValue, Scope, StdLib, Table, Value,
    VmState,
};

use super::{CommandSpec, KeySpec, LuaDef, ModSet, report_error};

/// Lua instructions one call (an event, a command, a timer) may run
/// before it is stopped as a runaway.
pub const INSTRUCTION_LIMIT: u64 = 2_000_000;
/// How often the limit is checked.
const HOOK_EVERY: u32 = 1000;
/// Memory one side's Lua may use.
pub const MEMORY_LIMIT: usize = 64 << 20;
/// The longest a mod's error text is kept.
const ERROR_LEN: usize = 300;

/// What a Lua state is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    /// Run once at startup to collect the blocks, items, commands and
    /// keys that `main.lua` registers; nothing else works yet.
    Registration,
    Server,
    Client,
}

impl Side {
    fn name(self) -> &'static str {
        match self {
            Side::Registration => "registration",
            Side::Server => "server",
            Side::Client => "client",
        }
    }
}

/// A player, as a mod sees them.
#[derive(Debug, Clone, PartialEq)]
pub struct PlayerInfo {
    pub index: usize,
    pub name: String,
    pub feet: DVec3,
    pub dimension: String,
}

/// Something that happened, for `rc.on(name, handler)`.
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

    fn table(&self, lua: &Lua) -> mlua::Result<Table> {
        let t = lua.create_table()?;
        let at = |t: &Table, p: IVec3| -> mlua::Result<()> {
            t.set("x", p.x)?;
            t.set("y", p.y)?;
            t.set("z", p.z)
        };
        match self {
            Event::ServerTick { tick } => t.set("tick", *tick)?,
            Event::PlayerJoin(p) | Event::PlayerLeave(p) => {
                t.set("player", p.index)?;
                t.set("name", p.name.as_str())?;
            }
            Event::BlockBreak {
                player,
                at: p,
                block,
            }
            | Event::BlockPlace {
                player,
                at: p,
                block,
            } => {
                t.set("player", *player)?;
                t.set("block", block.as_str())?;
                at(&t, *p)?;
            }
            Event::EntityDamage {
                entity,
                amount,
                source,
            } => {
                t.set("entity", entity.as_str())?;
                t.set("amount", *amount)?;
                t.set("source", source.as_str())?;
            }
            Event::EntityDeath { entity, source } => {
                t.set("entity", entity.as_str())?;
                t.set("source", source.as_str())?;
            }
            Event::Chat {
                player,
                name,
                message,
            } => {
                t.set("player", *player)?;
                t.set("name", name.as_str())?;
                t.set("message", message.as_str())?;
            }
            Event::ItemCrafted {
                player,
                item,
                count,
            } => {
                t.set("player", *player)?;
                t.set("item", item.as_str())?;
                t.set("count", *count)?;
            }
        }
        Ok(t)
    }
}

/// The world as a mod on the server may touch it. Players are numbered
/// from 0, the host first. Failures are the text of a Lua error.
pub trait Host {
    fn tick(&self) -> u64;
    /// The block's state text (`minecraft:stone`), `None` if not loaded.
    fn block_at(&self, at: IVec3) -> Option<String>;
    fn set_block(&mut self, at: IVec3, state: &str) -> Result<(), String>;
    fn give(&mut self, player: usize, item: &str, count: u32) -> Result<(), String>;
    /// A chat line for one player, or everyone.
    fn say(&mut self, player: Option<usize>, text: &str);
    fn spawn(&mut self, kind: &str, at: DVec3) -> Result<(), String>;
    fn players(&self) -> Vec<PlayerInfo>;
    /// Rolls a loot table: (item, count).
    fn loot(&mut self, table: &str) -> Result<Vec<(String, u32)>, String>;
}

/// The client as a mod may touch it.
pub trait ClientHost {
    /// Text above the hotbar. `seconds` of `None` keeps it until it is
    /// set to `None` text.
    fn hud_text(&mut self, id: &str, text: Option<String>, seconds: Option<f32>);
    /// Lights every block at least this much (0–15); `None` is off.
    fn brightness_override(&mut self, level: Option<u8>);
}

/// Which of a block's callbacks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockEvent {
    Use,
    Place,
    Break,
    RandomTick,
}

impl BlockEvent {
    fn key(self) -> &'static str {
        match self {
            BlockEvent::Use => "on_use",
            BlockEvent::Place => "on_place",
            BlockEvent::Break => "on_break",
            BlockEvent::RandomTick => "on_random_tick",
        }
    }
}

#[derive(Default)]
struct Callbacks {
    mod_id: String,
    funcs: HashMap<&'static str, Function>,
}

struct Timer {
    id: u64,
    mod_id: String,
    /// The tick it fires on.
    due: u64,
    delay: u64,
    repeats: bool,
    func: Function,
}

#[derive(Default)]
struct Shared {
    handlers: HashMap<String, Vec<(String, Function)>>,
    blocks: HashMap<String, Callbacks>,
    items: HashMap<String, Callbacks>,
    commands: HashMap<String, (String, Function)>,
    keys: HashMap<String, (String, Function)>,
    timers: Vec<Timer>,
    next_timer: u64,
    /// The tick the timers count from.
    now: u64,
    /// What `main.lua` registered (Registration side only).
    blocks_defined: Vec<LuaDef>,
    items_defined: Vec<LuaDef>,
    commands_defined: Vec<CommandSpec>,
    keys_defined: Vec<KeySpec>,
    /// Mods whose `main.lua` failed.
    broken: Vec<String>,
}

/// One side's Lua with every mod's `main.lua` loaded.
pub struct Runtime {
    lua: Lua,
    side: Side,
    shared: Arc<Mutex<Shared>>,
    rc: HashMap<String, Table>,
    /// Each mod's global environment.
    envs: HashMap<String, Table>,
    budget: Arc<AtomicU64>,
    ticks: u64,
}

/// Keeps the first line of an error and caps its length.
pub fn short(err: &mlua::Error) -> String {
    let text = err.to_string();
    let mut line = text
        .lines()
        .find(|l| !l.trim().is_empty() && !l.starts_with("stack traceback"))
        .unwrap_or("error")
        .trim()
        .to_owned();
    // mlua wraps callback errors as "runtime error: ..."; keep the cause.
    if let Some(rest) = line.strip_prefix("runtime error: ") {
        line = rest.to_owned();
    }
    if line.len() > ERROR_LEN {
        line.truncate(ERROR_LEN);
        line.push('…');
    }
    line
}

fn fail(text: impl std::fmt::Display) -> mlua::Error {
    mlua::Error::runtime(text.to_string())
}

/// Runs every active mod's `main.lua` in a registration-only Lua state and
/// adds what they register to `set`: block and item names first, for the
/// registries to be built from.
pub fn register(set: &mut ModSet) {
    if set.active.iter().all(|m| m.lua.is_none()) {
        return;
    }
    let runtime = match Runtime::start(Side::Registration, set) {
        Ok(runtime) => runtime,
        Err(err) => {
            report_error("lua", format!("Lua failed to start: {}", short(&err)));
            return;
        }
    };
    let mut shared = runtime.shared.lock().unwrap_or_else(|e| e.into_inner());
    let blocks = std::mem::take(&mut shared.blocks_defined);
    let items = std::mem::take(&mut shared.items_defined);
    set.commands = std::mem::take(&mut shared.commands_defined);
    set.keys = std::mem::take(&mut shared.keys_defined);
    drop(shared);
    for block in blocks {
        let name = block.def["name"].as_str().unwrap_or_default().to_owned();
        let has_item = block
            .def
            .get("item")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        let added = set.add_block_name(&name).and_then(|()| {
            if has_item {
                set.add_item_name(&name)
            } else {
                Ok(())
            }
        });
        match added {
            Ok(()) => set.lua_blocks.push(block),
            Err(err) => report_error(&block.mod_id, format!("{err:#}")),
        }
    }
    for item in items {
        let name = item.def["name"].as_str().unwrap_or_default().to_owned();
        match set.add_item_name(&name) {
            Ok(()) => set.lua_items.push(item),
            Err(err) => report_error(&item.mod_id, format!("{err:#}")),
        }
    }
}

impl Runtime {
    /// A sandboxed Lua for `side` with the active mods of `set` loaded.
    pub fn start(side: Side, set: &ModSet) -> mlua::Result<Self> {
        let lua = Lua::new_with(
            StdLib::TABLE | StdLib::STRING | StdLib::MATH | StdLib::UTF8 | StdLib::COROUTINE,
            LuaOptions::default(),
        )?;
        lua.set_memory_limit(MEMORY_LIMIT)?;
        let budget = Arc::new(AtomicU64::new(0));
        let counted = budget.clone();
        lua.set_global_hook(
            HookTriggers {
                every_nth_instruction: Some(HOOK_EVERY),
                ..Default::default()
            },
            move |_, _| {
                if counted.fetch_add(u64::from(HOOK_EVERY), Ordering::Relaxed) >= INSTRUCTION_LIMIT
                {
                    Err(fail("the mod ran too long (instruction limit reached)"))
                } else {
                    Ok(VmState::Continue)
                }
            },
        )?;
        let globals = lua.globals();
        for name in [
            "dofile",
            "loadfile",
            "collectgarbage",
            "os",
            "io",
            "debug",
            "package",
            "require",
            "print",
        ] {
            globals.set(name, Value::Nil)?;
        }
        let mut runtime = Self {
            lua,
            side,
            shared: Arc::new(Mutex::new(Shared::default())),
            rc: HashMap::new(),
            envs: HashMap::new(),
            budget,
            ticks: 0,
        };
        runtime.load_mods(set)?;
        Ok(runtime)
    }

    fn load_mods(&mut self, set: &ModSet) -> mlua::Result<()> {
        for active in &set.active {
            let Some(source) = &active.lua else {
                continue;
            };
            let id = active.manifest.id.clone();
            let env = self.lua.create_table()?;
            let meta = self.lua.create_table()?;
            meta.set("__index", self.lua.globals())?;
            env.set_metatable(Some(meta))?;
            let rc = self.build_rc(&id, &active.manifest.version)?;
            env.set("rc", rc.clone())?;
            self.install_require(&env, active)?;
            let log_id = id.clone();
            env.set(
                "print",
                self.lua.create_function(move |_, args: MultiValue| {
                    let line: Vec<String> = args.iter().map(|v| format!("{v:?}")).collect();
                    info!("[mod {log_id}] {}", line.join(" "));
                    Ok(())
                })?,
            )?;
            self.rc.insert(id.clone(), rc);
            self.envs.insert(id.clone(), env.clone());
            self.budget.store(0, Ordering::Relaxed);
            let loaded = self
                .lua
                .load(source.as_str())
                .set_name(format!("@{id}/main.lua"))
                .set_environment(env)
                .exec();
            if let Err(err) = loaded {
                report_error(&id, short(&err));
                self.shared
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .broken
                    .push(id);
            }
        }
        Ok(())
    }

    /// `require("name")` loads `name.lua` from the mod's own folder and
    /// nowhere else.
    fn install_require(&self, env: &Table, active: &super::Active) -> mlua::Result<()> {
        let loaded = self.lua.create_table()?;
        let source = active.source.clone();
        let mod_id = active.manifest.id.clone();
        let env_for_chunks = env.clone();
        let require = self.lua.create_function(move |lua, name: String| {
            let valid = !name.is_empty()
                && !name.starts_with('/')
                && !name.contains("..")
                && !name.contains('\\')
                && name
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || "_./-".contains(c));
            if !valid {
                return Err(fail(format!("require: bad module name {name:?}")));
            }
            let cached: Value = loaded.get(name.as_str())?;
            if !matches!(cached, Value::Nil) {
                return Ok(cached);
            }
            let path = format!("{}.lua", name.replace('.', "/"));
            let Some(text) = source.text(&path) else {
                return Err(fail(format!("require: no file {path} in the mod {mod_id}")));
            };
            let value: Value = lua
                .load(text)
                .set_name(format!("@{mod_id}/{path}"))
                .set_environment(env_for_chunks.clone())
                .eval()?;
            let value = if matches!(value, Value::Nil) {
                Value::Boolean(true)
            } else {
                value
            };
            loaded.set(name.as_str(), value.clone())?;
            Ok(value)
        })?;
        env.set("require", require)
    }

    /// The `rc` table for one mod: everything that doesn't need the world.
    fn build_rc(&self, id: &str, version: &str) -> mlua::Result<Table> {
        let lua = &self.lua;
        let rc = lua.create_table()?;
        rc.set("mod_id", id)?;
        rc.set("version", version)?;
        rc.set("game_version", super::GAME_VERSION)?;
        rc.set("side", self.side.name())?;
        rc.set("is_server", self.side == Side::Server)?;
        rc.set("is_client", self.side == Side::Client)?;
        let log_id = id.to_owned();
        rc.set(
            "log",
            lua.create_function(move |_, args: MultiValue| {
                let line: Vec<String> = args
                    .iter()
                    .map(|v| match v {
                        Value::String(s) => s.to_string_lossy().to_string(),
                        other => format!("{other:?}"),
                    })
                    .collect();
                info!("[mod {log_id}] {}", line.join(" "));
                Ok(())
            })?,
        )?;
        let (side, shared) = (self.side, self.shared.clone());
        let owner = id.to_owned();

        // rc.on(event, handler)
        let (s, o) = (shared.clone(), owner.clone());
        rc.set(
            "on",
            lua.create_function(move |_, (event, func): (String, Function)| {
                if side != Side::Registration {
                    s.lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .handlers
                        .entry(event)
                        .or_default()
                        .push((o.clone(), func));
                }
                Ok(())
            })?,
        )?;

        // rc.after(ticks, f) / rc.every(ticks, f) -> id; rc.cancel(id)
        for (name, repeats) in [("after", false), ("every", true)] {
            let (s, o) = (shared.clone(), owner.clone());
            rc.set(
                name,
                lua.create_function(move |_, (ticks, func): (u64, Function)| {
                    if side == Side::Registration {
                        return Ok(0);
                    }
                    if ticks == 0 {
                        return Err(fail("the delay must be at least 1 tick"));
                    }
                    let mut s = s.lock().unwrap_or_else(|e| e.into_inner());
                    s.next_timer += 1;
                    let id = s.next_timer;
                    let due = s.now + ticks;
                    s.timers.push(Timer {
                        id,
                        mod_id: o.clone(),
                        due,
                        delay: ticks,
                        repeats,
                        func,
                    });
                    Ok(id)
                })?,
            )?;
        }
        let s = shared.clone();
        rc.set(
            "cancel",
            lua.create_function(move |_, id: u64| {
                s.lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .timers
                    .retain(|t| t.id != id);
                Ok(())
            })?,
        )?;

        // Registration: blocks, items, commands, keys.
        let (s, o) = (shared.clone(), owner.clone());
        rc.set(
            "register_block",
            lua.create_function(move |lua, (name, def): (String, Table)| {
                register_thing(lua, &s, &o, side, true, name, def)
            })?,
        )?;
        let (s, o) = (shared.clone(), owner.clone());
        rc.set(
            "register_item",
            lua.create_function(move |lua, (name, def): (String, Table)| {
                register_thing(lua, &s, &o, side, false, name, def)
            })?,
        )?;
        let (s, o) = (shared.clone(), owner.clone());
        rc.set(
            "register_command",
            lua.create_function(move |_, (name, def): (String, Table)| {
                let run: Function = def.get("run")?;
                let mut s = s.lock().unwrap_or_else(|e| e.into_inner());
                if side == Side::Registration {
                    s.commands_defined.push(CommandSpec {
                        mod_id: o.clone(),
                        usage: def
                            .get::<Option<String>>("usage")?
                            .unwrap_or_else(|| format!("/{name}")),
                        cheat: def.get::<Option<bool>>("cheat")?.unwrap_or(false),
                        host: def.get::<Option<bool>>("host")?.unwrap_or(false),
                        name: name.clone(),
                    });
                }
                s.commands.insert(name, (o.clone(), run));
                Ok(())
            })?,
        )?;
        let (s, o) = (shared.clone(), owner.clone());
        rc.set(
            "register_key",
            lua.create_function(move |_, (key, def): (String, Table)| {
                let full = if key.contains(':') {
                    key
                } else {
                    format!("{o}:{key}")
                };
                let mut s = s.lock().unwrap_or_else(|e| e.into_inner());
                if side == Side::Registration {
                    s.keys_defined.push(KeySpec {
                        mod_id: o.clone(),
                        id: full.clone(),
                        name: def
                            .get::<Option<String>>("name")?
                            .unwrap_or_else(|| full.clone()),
                        default: def.get::<Option<String>>("default")?.unwrap_or_default(),
                    });
                }
                if let Some(press) = def.get::<Option<Function>>("on_press")? {
                    s.keys.insert(full, (o.clone(), press));
                }
                Ok(())
            })?,
        )?;
        Ok(rc)
    }

    fn call_server(
        &self,
        host: &mut dyn Host,
        mod_id: &str,
        func: &Function,
        args: impl mlua::IntoLuaMulti,
    ) -> mlua::Result<MultiValue> {
        let Some(rc) = self.rc.get(mod_id) else {
            return Err(fail(format!("no mod {mod_id} is running")));
        };
        self.budget.store(0, Ordering::Relaxed);
        let cell = RefCell::new(host);
        self.lua.scope(|scope| {
            install_server_api(&self.lua, scope, &cell, rc)?;
            func.call(args)
        })
    }

    fn call_client(
        &self,
        host: &mut dyn ClientHost,
        mod_id: &str,
        func: &Function,
        args: impl mlua::IntoLuaMulti,
    ) -> mlua::Result<MultiValue> {
        let Some(rc) = self.rc.get(mod_id) else {
            return Err(fail(format!("no mod {mod_id} is running")));
        };
        self.budget.store(0, Ordering::Relaxed);
        let cell = RefCell::new(host);
        self.lua.scope(|scope| {
            install_client_api(&self.lua, scope, &cell, rc)?;
            func.call(args)
        })
    }

    /// Mods whose `main.lua` failed to load.
    pub fn broken(&self) -> Vec<String> {
        self.shared
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .broken
            .clone()
    }

    /// Whether any mod handles `event` or has block callbacks (cheap
    /// checks for the game to skip building events nobody wants).
    pub fn wants(&self, event: &str) -> bool {
        self.shared
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .handlers
            .get(event)
            .is_some_and(|h| !h.is_empty())
    }

    /// Tells every mod's handlers of `event`.
    pub fn fire(&self, host: &mut dyn Host, event: &Event) {
        let handlers: Vec<(String, Function)> = {
            let shared = self.shared.lock().unwrap_or_else(|e| e.into_inner());
            match shared.handlers.get(event.name()) {
                Some(list) => list.clone(),
                None => return,
            }
        };
        for (mod_id, func) in handlers {
            let result = event
                .table(&self.lua)
                .and_then(|table| self.call_server(host, &mod_id, &func, table));
            if let Err(err) = result {
                report_error(&mod_id, format!("{}: {}", event.name(), short(&err)));
            }
        }
    }

    /// Runs the timers due this tick.
    pub fn tick_timers_server(&mut self, host: &mut dyn Host) {
        self.ticks += 1;
        for (mod_id, func) in self.due_timers() {
            if let Err(err) = self.call_server(host, &mod_id, &func, ()) {
                report_error(&mod_id, format!("timer: {}", short(&err)));
            }
        }
    }

    pub fn tick_timers_client(&mut self, host: &mut dyn ClientHost) {
        self.ticks += 1;
        for (mod_id, func) in self.due_timers() {
            if let Err(err) = self.call_client(host, &mod_id, &func, ()) {
                report_error(&mod_id, format!("timer: {}", short(&err)));
            }
        }
    }

    fn due_timers(&self) -> Vec<(String, Function)> {
        let now = self.ticks;
        let mut shared = self.shared.lock().unwrap_or_else(|e| e.into_inner());
        shared.now = now;
        let mut due = Vec::new();
        shared.timers.retain_mut(|timer| {
            if timer.due > now {
                return true;
            }
            due.push((timer.mod_id.clone(), timer.func.clone()));
            if timer.repeats {
                timer.due = now + timer.delay;
            }
            timer.repeats
        });
        due
    }

    /// Whether mod block `name` has a callback for `event`.
    pub fn has_block_callback(&self, name: &str, event: BlockEvent) -> bool {
        self.shared
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .blocks
            .get(name)
            .is_some_and(|c| c.funcs.contains_key(event.key()))
    }

    /// Calls a mod block's callback with a table `{x, y, z, player, block}`.
    pub fn block_event(
        &self,
        host: &mut dyn Host,
        name: &str,
        event: BlockEvent,
        at: IVec3,
        player: usize,
    ) {
        let found = {
            let shared = self.shared.lock().unwrap_or_else(|e| e.into_inner());
            shared.blocks.get(name).and_then(|c| {
                c.funcs
                    .get(event.key())
                    .map(|f| (c.mod_id.clone(), f.clone()))
            })
        };
        let Some((mod_id, func)) = found else {
            return;
        };
        let result = (|| {
            let t = self.lua.create_table()?;
            t.set("x", at.x)?;
            t.set("y", at.y)?;
            t.set("z", at.z)?;
            t.set("player", player)?;
            t.set("block", name)?;
            self.call_server(host, &mod_id, &func, t)
        })();
        if let Err(err) = result {
            report_error(&mod_id, format!("{name} {}: {}", event.key(), short(&err)));
        }
    }

    /// Whether mod item `name` has an `on_use`.
    pub fn has_item_callback(&self, name: &str) -> bool {
        self.shared
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .items
            .get(name)
            .is_some_and(|c| c.funcs.contains_key("on_use"))
    }

    /// Calls a mod item's `on_use` with `{player, item, x, y, z}` (the
    /// block aimed at, if any).
    pub fn item_used(&self, host: &mut dyn Host, name: &str, player: usize, aimed: Option<IVec3>) {
        let found = {
            let shared = self.shared.lock().unwrap_or_else(|e| e.into_inner());
            shared
                .items
                .get(name)
                .and_then(|c| c.funcs.get("on_use").map(|f| (c.mod_id.clone(), f.clone())))
        };
        let Some((mod_id, func)) = found else {
            return;
        };
        let result = (|| {
            let t = self.lua.create_table()?;
            t.set("player", player)?;
            t.set("item", name)?;
            if let Some(at) = aimed {
                t.set("x", at.x)?;
                t.set("y", at.y)?;
                t.set("z", at.z)?;
            }
            self.call_server(host, &mod_id, &func, t)
        })();
        if let Err(err) = result {
            report_error(&mod_id, format!("{name} on_use: {}", short(&err)));
        }
    }

    /// Runs the mod command `name`: `Some` of its lines or error, `None`
    /// if no mod registered it. The handler gets `(ctx, args)`; it
    /// returns a string or a list of strings to say, or `nil, "error"`.
    pub fn run_command(
        &self,
        host: &mut dyn Host,
        name: &str,
        player: usize,
        args: &[&str],
    ) -> Option<Result<Vec<String>, String>> {
        let (mod_id, func) = {
            let shared = self.shared.lock().unwrap_or_else(|e| e.into_inner());
            shared.commands.get(name).cloned()?
        };
        let result = (|| {
            let ctx = self.lua.create_table()?;
            ctx.set("player", player)?;
            ctx.set("command", name)?;
            let list = self.lua.create_table()?;
            for (i, arg) in args.iter().enumerate() {
                list.set(i + 1, *arg)?;
            }
            self.call_server(host, &mod_id, &func, (ctx, list))
        })();
        Some(match result {
            Err(err) => {
                let text = short(&err);
                report_error(&mod_id, format!("/{name}: {text}"));
                Err(format!("Command failed: {text}"))
            }
            Ok(values) => {
                let mut values = values.into_iter();
                match (values.next(), values.next()) {
                    (None | Some(Value::Nil), Some(Value::String(why))) => {
                        Err(why.to_string_lossy().to_string())
                    }
                    (Some(Value::String(line)), _) => Ok(vec![line.to_string_lossy().to_string()]),
                    (Some(Value::Table(lines)), _) => Ok(lines
                        .sequence_values::<String>()
                        .filter_map(Result::ok)
                        .collect()),
                    _ => Ok(Vec::new()),
                }
            }
        })
    }

    /// Calls the `on_press` of key binding `id` (`mod:key`).
    pub fn key_pressed(&self, host: &mut dyn ClientHost, id: &str) {
        let found = {
            let shared = self.shared.lock().unwrap_or_else(|e| e.into_inner());
            shared.keys.get(id).cloned()
        };
        let Some((mod_id, func)) = found else {
            return;
        };
        if let Err(err) = self.call_client(host, &mod_id, &func, ()) {
            report_error(&mod_id, format!("key {id}: {}", short(&err)));
        }
    }

    /// Runs a chunk of Lua as mod `mod_id` on the server (tests and
    /// tooling); its return values come back as text.
    pub fn eval_as(&self, host: &mut dyn Host, mod_id: &str, code: &str) -> Result<String, String> {
        let func = self
            .lua
            .load(code)
            .set_name("=eval")
            .into_function()
            .map_err(|e| short(&e))?;
        let env = self
            .envs
            .get(mod_id)
            .ok_or_else(|| format!("no mod {mod_id} is running"))?
            .clone();
        func.set_environment(env).map_err(|e| short(&e))?;
        let values = self
            .call_server(host, mod_id, &func, ())
            .map_err(|e| short(&e))?;
        Ok(values
            .iter()
            .map(|v| match v {
                Value::String(s) => s.to_string_lossy().to_string(),
                other => format!("{other:?}"),
            })
            .collect::<Vec<_>>()
            .join(","))
    }

    /// Runs an expression in the sandbox without any mod (tests): the
    /// instruction limit and the missing libraries apply.
    pub fn eval_plain(&self, code: &str) -> Result<String, String> {
        self.budget.store(0, Ordering::Relaxed);
        self.lua
            .load(code)
            .eval::<Value>()
            .map(|v| match v {
                Value::String(s) => s.to_string_lossy().to_string(),
                other => format!("{other:?}"),
            })
            .map_err(|e| short(&e))
    }
}

/// `rc.register_block(name, def)` / `rc.register_item(name, def)`: during
/// registration the definition (less its callbacks) is kept as JSON for
/// the registries; later only the callbacks are attached.
fn register_thing(
    lua: &Lua,
    shared: &Arc<Mutex<Shared>>,
    mod_id: &str,
    side: Side,
    is_block: bool,
    name: String,
    def: Table,
) -> mlua::Result<()> {
    let full = if name.contains(':') {
        name
    } else {
        format!("{mod_id}:{name}")
    };
    if !full.starts_with(&format!("{mod_id}:")) {
        return Err(fail(format!(
            "{full}: a mod may only register names in its own namespace {mod_id}:"
        )));
    }
    let callback_keys: &[&'static str] = if is_block {
        &["on_use", "on_place", "on_break", "on_random_tick"]
    } else {
        &["on_use"]
    };
    let mut callbacks = Callbacks {
        mod_id: mod_id.to_owned(),
        funcs: HashMap::new(),
    };
    for &key in callback_keys {
        if let Some(func) = def.get::<Option<Function>>(key)? {
            callbacks.funcs.insert(key, func);
            def.set(key, Value::Nil)?;
        }
    }
    let kind = if is_block { "block" } else { "item" };
    let mut shared = shared.lock().unwrap_or_else(|e| e.into_inner());
    if side == Side::Registration {
        let mut json: serde_json::Value = lua
            .from_value(Value::Table(def))
            .map_err(|e| fail(format!("{kind} {full}: {e}")))?;
        let serde_json::Value::Object(fields) = &mut json else {
            return Err(fail(format!(
                "{kind} {full}: the definition must be a table of fields"
            )));
        };
        fields.insert("name".into(), full.clone().into());
        let scripted = callbacks
            .funcs
            .keys()
            .any(|k| matches!(*k, "on_use" | "on_place" | "on_break"));
        if scripted {
            fields.insert("scripted".into(), true.into());
        }
        if is_block {
            fields.entry("hardness").or_insert(1.0.into());
            if callbacks.funcs.contains_key("on_random_tick") {
                fields.insert("random_ticks".into(), true.into());
            }
        } else {
            fields.entry("texture").or_insert(
                full.split_once(':')
                    .map_or(full.as_str(), |(_, n)| n)
                    .into(),
            );
        }
        let entry = LuaDef {
            mod_id: mod_id.to_owned(),
            def: json,
        };
        if is_block {
            shared.blocks_defined.push(entry);
        } else {
            shared.items_defined.push(entry);
        }
    } else {
        let exists = if is_block {
            super::active().block_by_name(&full).is_some()
        } else {
            super::active().item_by_name(&full).is_some()
        };
        if !exists {
            return Err(fail(format!(
                "{kind} {full} was not registered while the game started: register at the top of main.lua, not later"
            )));
        }
    }
    if !callbacks.funcs.is_empty() {
        if is_block {
            shared.blocks.insert(full, callbacks);
        } else {
            shared.items.insert(full, callbacks);
        }
    }
    Ok(())
}

fn at(x: i32, y: i32, z: i32) -> IVec3 {
    IVec3::new(x, y, z)
}

/// Puts the world-reaching functions in `rc` for one call.
fn install_server_api<'scope, 'env>(
    lua: &Lua,
    scope: &'scope Scope<'scope, 'env>,
    host: &'env RefCell<&mut dyn Host>,
    rc: &Table,
) -> mlua::Result<()> {
    let _ = lua;
    rc.set(
        "tick",
        scope.create_function(move |_, ()| Ok(host.borrow().tick()))?,
    )?;
    rc.set(
        "get_block",
        scope.create_function(move |_, (x, y, z): (i32, i32, i32)| {
            Ok(host.borrow().block_at(at(x, y, z)))
        })?,
    )?;
    rc.set(
        "set_block",
        scope.create_function(move |_, (x, y, z, state): (i32, i32, i32, String)| {
            host.borrow_mut()
                .set_block(at(x, y, z), &state)
                .map_err(fail)
        })?,
    )?;
    rc.set(
        "give",
        scope.create_function(
            move |_, (player, item, count): (usize, String, Option<u32>)| {
                host.borrow_mut()
                    .give(player, &item, count.unwrap_or(1))
                    .map_err(fail)
            },
        )?,
    )?;
    rc.set(
        "say",
        scope.create_function(move |_, (text, player): (String, Option<usize>)| {
            host.borrow_mut().say(player, &text);
            Ok(())
        })?,
    )?;
    rc.set(
        "spawn",
        scope.create_function(move |_, (kind, x, y, z): (String, f64, f64, f64)| {
            host.borrow_mut()
                .spawn(&kind, DVec3::new(x, y, z))
                .map_err(fail)
        })?,
    )?;
    rc.set(
        "players",
        scope.create_function(move |lua, ()| {
            let list = lua.create_table()?;
            for (i, p) in host.borrow().players().into_iter().enumerate() {
                let t = lua.create_table()?;
                t.set("index", p.index)?;
                t.set("name", p.name)?;
                t.set("x", p.feet.x)?;
                t.set("y", p.feet.y)?;
                t.set("z", p.feet.z)?;
                t.set("dimension", p.dimension)?;
                list.set(i + 1, t)?;
            }
            Ok(list)
        })?,
    )?;
    rc.set(
        "loot",
        scope.create_function(move |lua, table: String| {
            let rolled = host.borrow_mut().loot(&table).map_err(fail)?;
            let list = lua.create_table()?;
            for (i, (item, count)) in rolled.into_iter().enumerate() {
                let t = lua.create_table()?;
                t.set("item", item)?;
                t.set("count", count)?;
                list.set(i + 1, t)?;
            }
            Ok(list)
        })?,
    )?;
    Ok(())
}

/// Puts `rc.client` (HUD text and the brightness override) in `rc` for
/// one call.
fn install_client_api<'scope, 'env>(
    lua: &Lua,
    scope: &'scope Scope<'scope, 'env>,
    host: &'env RefCell<&mut dyn ClientHost>,
    rc: &Table,
) -> mlua::Result<()> {
    let client = lua.create_table()?;
    client.set(
        "hud_text",
        scope.create_function(
            move |_, (id, text, seconds): (String, Option<String>, Option<f32>)| {
                host.borrow_mut().hud_text(&id, text, seconds);
                Ok(())
            },
        )?,
    )?;
    client.set(
        "set_brightness_override",
        scope.create_function(move |_, level: Option<u8>| {
            if level.is_some_and(|l| l > 15) {
                return Err(fail("the brightness override is a light level, 0 to 15"));
            }
            host.borrow_mut().brightness_override(level);
            Ok(())
        })?,
    )?;
    rc.set("client", client)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mods::{Active, Data, Manifest, Source};

    pub(crate) fn mod_with(id: &str, lua: &str) -> Active {
        Active {
            manifest: Manifest {
                id: id.to_owned(),
                name: id.to_owned(),
                version: "1.0".into(),
                authors: vec![],
                description: String::new(),
                rustcraft: String::new(),
                dependencies: vec![],
                client_only: false,
            },
            source: Source::Dir("/nonexistent".into()),
            data: Data::default(),
            lua: Some(lua.to_owned()),
        }
    }

    fn set_of(mods: Vec<Active>) -> ModSet {
        ModSet {
            active: mods,
            ..Default::default()
        }
    }

    #[derive(Default)]
    struct Fake {
        blocks: HashMap<IVec3, String>,
        said: Vec<String>,
        given: Vec<(usize, String, u32)>,
    }

    impl Host for Fake {
        fn tick(&self) -> u64 {
            7
        }
        fn block_at(&self, at: IVec3) -> Option<String> {
            self.blocks.get(&at).cloned()
        }
        fn set_block(&mut self, at: IVec3, state: &str) -> Result<(), String> {
            if state == "nonsense" {
                return Err(format!("unknown block {state}"));
            }
            self.blocks.insert(at, state.to_owned());
            Ok(())
        }
        fn give(&mut self, player: usize, item: &str, count: u32) -> Result<(), String> {
            self.given.push((player, item.to_owned(), count));
            Ok(())
        }
        fn say(&mut self, _: Option<usize>, text: &str) {
            self.said.push(text.to_owned());
        }
        fn spawn(&mut self, _: &str, _: DVec3) -> Result<(), String> {
            Ok(())
        }
        fn players(&self) -> Vec<PlayerInfo> {
            vec![PlayerInfo {
                index: 0,
                name: "Steve".into(),
                feet: DVec3::new(1.0, 2.0, 3.0),
                dimension: "overworld".into(),
            }]
        }
        fn loot(&mut self, _: &str) -> Result<Vec<(String, u32)>, String> {
            Ok(vec![("minecraft:stick".into(), 2)])
        }
    }

    fn server(code: &str) -> Runtime {
        let set = set_of(vec![mod_with("t", code)]);
        Runtime::start(Side::Server, &set).unwrap()
    }

    #[test]
    fn the_sandbox_has_no_os_io_or_require_outside_the_mod() {
        let rt = server("");
        for name in ["os", "io", "debug", "package", "dofile", "loadfile"] {
            assert_eq!(
                rt.eval_plain(&format!("return type({name})")).unwrap(),
                "nil",
                "{name}"
            );
        }
        assert_eq!(
            rt.eval_plain("return type(string.format) .. type(math.floor)")
                .unwrap(),
            "functionfunction"
        );
        // A mod can't climb out of its folder with require.
        let mut host = Fake::default();
        let bad = rt.eval_as(&mut host, "t", "return require('../secret')");
        assert!(bad.unwrap_err().contains("bad module name"));
    }

    #[test]
    fn a_runaway_loop_is_stopped() {
        let rt = server("");
        let err = rt.eval_plain("while true do end").unwrap_err();
        assert!(err.contains("ran too long"), "{err}");
        // The state still works afterwards.
        assert_eq!(rt.eval_plain("return 1 + 1").unwrap(), "Integer(2)");
        // Coroutines can't hide a loop from the limit.
        let err = rt
            .eval_plain("return coroutine.wrap(function() while true do end end)()")
            .unwrap_err();
        assert!(err.contains("ran too long"), "{err}");
    }

    #[test]
    fn errors_are_caught_and_reported_without_a_panic() {
        let rt = server("rc.on('server_tick', function() error('boom') end)");
        let mut host = Fake::default();
        rt.fire(&mut host, &Event::ServerTick { tick: 1 });
        assert!(
            crate::mods::errors_of("t")
                .iter()
                .any(|e| e.contains("boom")),
            "{:?}",
            crate::mods::errors_of("t")
        );
    }

    #[test]
    fn a_main_lua_that_fails_is_marked_broken() {
        let rt = server("local x = nil; x.y = 1");
        assert_eq!(rt.broken(), ["t"]);
    }

    #[test]
    fn handlers_see_events_and_reach_the_world() {
        let rt = server(
            "rc.on('block_break', function(e)
                rc.set_block(e.x, e.y + 1, e.z, 'minecraft:gold_block')
                rc.say(e.block .. ' broken by ' .. rc.players()[1].name)
                rc.give(e.player, 'minecraft:diamond', 3)
            end)",
        );
        let mut host = Fake::default();
        rt.fire(
            &mut host,
            &Event::BlockBreak {
                player: 0,
                at: IVec3::new(1, 2, 3),
                block: "minecraft:stone".into(),
            },
        );
        assert_eq!(host.blocks[&IVec3::new(1, 3, 3)], "minecraft:gold_block");
        assert_eq!(host.said, ["minecraft:stone broken by Steve"]);
        assert_eq!(host.given, [(0, "minecraft:diamond".to_owned(), 3)]);
    }

    #[test]
    fn world_functions_only_work_during_a_call() {
        let rt = server("saved = rc.get_block; rc.on('server_tick', function() end)");
        let mut host = Fake::default();
        // `saved` points at a function that expired with its call.
        let out = rt.eval_as(
            &mut host,
            "t",
            "return pcall(function() return rc.get_block(0,0,0) end)",
        );
        assert!(out.is_ok(), "{out:?}");
    }

    #[test]
    fn commands_return_lines_or_an_error() {
        let rt = server(
            "rc.register_command('hi', { run = function(ctx, args)
                if args[1] == 'bad' then return nil, 'no way' end
                return 'hello ' .. (args[1] or 'you') .. ' #' .. ctx.player
            end })",
        );
        let mut host = Fake::default();
        assert_eq!(
            rt.run_command(&mut host, "hi", 2, &["Bob"]),
            Some(Ok(vec!["hello Bob #2".to_owned()]))
        );
        assert_eq!(
            rt.run_command(&mut host, "hi", 0, &["bad"]),
            Some(Err("no way".to_owned()))
        );
        assert_eq!(rt.run_command(&mut host, "nope", 0, &[]), None);
    }

    #[test]
    fn timers_fire_after_their_delay_and_repeat() {
        let rt = server(
            "n = 0; rc.every(2, function() rc.say('tick') end); rc.after(3, function() rc.say('once') end)",
        );
        let mut rt = rt;
        let mut host = Fake::default();
        for _ in 0..6 {
            rt.tick_timers_server(&mut host);
        }
        assert_eq!(host.said.iter().filter(|s| *s == "tick").count(), 3);
        assert_eq!(host.said.iter().filter(|s| *s == "once").count(), 1);
    }

    #[test]
    fn require_loads_from_the_mod_only() {
        let dir = crate::world::save::TempDir::new("mod-require");
        std::fs::write(dir.path().join("helper.lua"), "return { answer = 42 }").unwrap();
        let mut active = mod_with("t", "answer = require('helper').answer");
        active.source = Source::Dir(dir.path().to_owned());
        let set = set_of(vec![active]);
        let rt = Runtime::start(Side::Server, &set).unwrap();
        assert!(rt.broken().is_empty(), "{:?}", crate::mods::errors_of("t"));
        let mut host = Fake::default();
        assert_eq!(
            rt.eval_as(&mut host, "t", "return tostring(answer)")
                .unwrap(),
            "42"
        );
    }

    #[test]
    fn registration_collects_names_and_attaches_nothing_else() {
        let mut set = set_of(vec![mod_with(
            "gems",
            "rc.register_block('gem_block', { hardness = 3, textures = { all = 'gem_block' }, on_use = function() end })
             rc.register_item('gem', { texture = 'gem' })
             rc.register_command('gems', { usage = '/gems', run = function() end })
             rc.register_key('toggle', { name = 'Toggle', default = 'G', on_press = function() end })",
        )]);
        register(&mut set);
        assert_eq!(set.block_names_all(), ["gems:gem_block"]);
        assert_eq!(set.item_names_all(), ["gems:gem_block", "gems:gem"]);
        assert_eq!(set.commands[0].name, "gems");
        assert_eq!(set.keys[0].id, "gems:toggle");
        assert_eq!(set.lua_blocks[0].def["hardness"], 3);
    }

    #[test]
    fn a_mod_can_not_register_in_another_namespace() {
        let mut set = set_of(vec![mod_with(
            "a",
            "rc.register_item('b:thing', { texture = 'x' })",
        )]);
        register(&mut set);
        assert!(set.item_names_all().is_empty());
        assert!(
            crate::mods::errors_of("a")
                .iter()
                .any(|e| e.contains("own namespace"))
        );
    }

    #[test]
    fn client_functions_set_text_and_brightness() {
        struct Screen {
            text: Vec<(String, Option<String>)>,
            level: Option<u8>,
        }
        impl ClientHost for Screen {
            fn hud_text(&mut self, id: &str, text: Option<String>, _: Option<f32>) {
                self.text.push((id.to_owned(), text));
            }
            fn brightness_override(&mut self, level: Option<u8>) {
                self.level = level;
            }
        }
        let set = set_of(vec![mod_with(
            "fb",
            "on = false
             rc.register_key('g', { on_press = function()
                on = not on
                rc.client.set_brightness_override(on and 15 or nil)
                rc.client.hud_text('fb', on and 'Fullbright: ON' or nil)
             end })",
        )]);
        let rt = Runtime::start(Side::Client, &set).unwrap();
        let mut screen = Screen {
            text: vec![],
            level: None,
        };
        rt.key_pressed(&mut screen, "fb:g");
        assert_eq!(screen.level, Some(15));
        assert_eq!(
            screen.text.last().unwrap().1.as_deref(),
            Some("Fullbright: ON")
        );
        rt.key_pressed(&mut screen, "fb:g");
        assert_eq!(screen.level, None);
    }
}
