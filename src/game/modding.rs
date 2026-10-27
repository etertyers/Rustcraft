//! Mods on the server (M39): the Lua runtime a world runs, the world as
//! the mods' `Host`, and the events and callbacks the rest of the game
//! tells it about. A client's copy of the world never runs mods.

use glam::{DVec3, IVec3};

use super::{Game, sync::Role};
use crate::{
    inventory::{item::ItemId, stack::ItemStack},
    mods::lua::{BlockEvent, Event, Host, PlayerInfo, Runtime, Side},
    world::block::BlockState,
};

/// Where the world's mod runtime is.
#[derive(Default)]
pub(super) enum ModState {
    /// Not looked at yet: started with the first event.
    #[default]
    Unstarted,
    Running(Box<Runtime>),
    /// No mod has a script, or Lua could not start.
    Off,
}

/// The game as the mods' `Host`.
pub(super) struct GameHost<'a>(pub &'a mut Game);

impl Host for GameHost<'_> {
    fn tick(&self) -> u64 {
        self.0.time
    }

    fn block_at(&self, at: IVec3) -> Option<String> {
        self.0
            .chunks
            .world()
            .block(at.x, at.y, at.z)
            .map(|b| b.to_string())
    }

    fn set_block(&mut self, at: IVec3, state: &str) -> Result<(), String> {
        let state = BlockState::parse(state).map_err(|e| format!("{e:#}"))?;
        if self.0.chunks.world().block(at.x, at.y, at.z).is_none() {
            return Err(format!("{}, {}, {} is not loaded", at.x, at.y, at.z));
        }
        self.0.edit(&[(at, state)]);
        Ok(())
    }

    fn give(&mut self, player: usize, item: &str, count: u32) -> Result<(), String> {
        let id = crate::inventory::items::by_name(crate::world::block::strip_namespace(item))
            .ok_or_else(|| format!("unknown item {item}"))?;
        if player >= self.0.player_count() {
            return Err(format!("no player {player}"));
        }
        self.0.give_to(player, id, count);
        Ok(())
    }

    fn say(&mut self, player: Option<usize>, text: &str) {
        match player {
            Some(index) => self.0.chat_to.push((index, text.to_owned())),
            None => self.0.chat_out.push(text.to_owned()),
        }
    }

    fn spawn(&mut self, kind: &str, at: DVec3) -> Result<(), String> {
        let kind = crate::world::block::strip_namespace(kind);
        self.0
            .summon(kind, at, 0.0)
            .map(|_| ())
            .ok_or_else(|| format!("unknown entity {kind}"))
    }

    fn players(&self) -> Vec<PlayerInfo> {
        (0..self.0.player_count())
            .map(|index| {
                let p = self.0.player_at(index);
                PlayerInfo {
                    index,
                    name: p.name.clone(),
                    feet: self.0.feet_of(index),
                    dimension: Game::dimension_slug(p.dimension),
                }
            })
            .collect()
    }

    fn loot(&mut self, table: &str) -> Result<Vec<(String, u32)>, String> {
        if crate::world::structure::loot::table(table).is_none() {
            return Err(format!("no loot table {table}"));
        }
        let slots = crate::world::structure::loot::fill(table, &mut self.0.rng);
        Ok(slots
            .into_iter()
            .flatten()
            .map(|s| {
                (
                    crate::world::block::qualified(s.id.name()).into_owned(),
                    u32::from(s.count),
                )
            })
            .collect())
    }
}

impl Game {
    /// Takes the runtime out to call it, starting it the first time. The
    /// caller puts it back with `restore_mods`.
    fn take_mods(&mut self) -> Option<Box<Runtime>> {
        if self.role != Role::Server {
            return None;
        }
        match std::mem::replace(&mut self.mods, ModState::Off) {
            ModState::Running(runtime) => Some(runtime),
            ModState::Off => None,
            ModState::Unstarted => {
                let set = crate::mods::active();
                if set.active.iter().all(|m| m.lua.is_none()) {
                    return None;
                }
                match Runtime::start(Side::Server, set) {
                    Ok(runtime) => Some(Box::new(runtime)),
                    Err(err) => {
                        crate::mods::report_error("lua", crate::mods::lua::short(&err));
                        None
                    }
                }
            }
        }
    }

    fn restore_mods(&mut self, runtime: Box<Runtime>) {
        self.mods = ModState::Running(runtime);
    }

    /// Whether a mod listens for `event` (so it isn't built for nothing).
    fn mods_want(&self, event: &str) -> bool {
        matches!(&self.mods, ModState::Running(rt) if rt.wants(event))
            || matches!(self.mods, ModState::Unstarted)
                && self.role == Role::Server
                && !crate::mods::active().active.is_empty()
    }

    /// Tells the mods' handlers about `event`.
    pub fn mod_event(&mut self, event: Event) {
        if !self.mods_want(event.name()) {
            return;
        }
        let Some(runtime) = self.take_mods() else {
            return;
        };
        runtime.fire(&mut GameHost(self), &event);
        self.restore_mods(runtime);
    }

    /// The names of the blocks and the item here with callbacks are the
    /// mods' (`mod:name`).
    pub(super) fn mod_block_event(&mut self, event: BlockEvent, block: BlockState, at: IVec3) {
        if !block.def().scripted && !block.def().random_ticks {
            return;
        }
        let Some(runtime) = self.take_mods() else {
            return;
        };
        let name = block.def().name.clone();
        runtime.block_event(&mut GameHost(self), &name, event, at, 0);
        self.restore_mods(runtime);
    }

    /// Like `mod_block_event` for the player at `self.player` using,
    /// placing or breaking it.
    pub(super) fn mod_block_by_player(&mut self, event: BlockEvent, block: BlockState, at: IVec3) {
        if !block.def().scripted {
            return;
        }
        let index = self.current_player_index();
        let Some(runtime) = self.take_mods() else {
            return;
        };
        let name = block.def().name.clone();
        runtime.block_event(&mut GameHost(self), &name, event, at, index);
        self.restore_mods(runtime);
    }

    /// Whether the block at `at` has a mod's `on_use`, and calls it.
    pub(super) fn mod_use_block(&mut self, at: IVec3) -> bool {
        let Some(block) = self.chunks.world().block(at.x, at.y, at.z) else {
            return false;
        };
        if !block.def().scripted {
            return false;
        }
        let name = block.def().name.clone();
        let index = self.current_player_index();
        let Some(runtime) = self.take_mods() else {
            return false;
        };
        let has = runtime.has_block_callback(&name, BlockEvent::Use);
        if has {
            runtime.block_event(&mut GameHost(self), &name, BlockEvent::Use, at, index);
        }
        self.restore_mods(runtime);
        has
    }

    /// Calls the `on_use` of the mod item `id`, if it has one.
    pub(super) fn mod_use_item(&mut self, id: ItemId, aimed: Option<IVec3>) -> bool {
        let Some(def) = self.items.item(id) else {
            return false;
        };
        if !def.scripted {
            return false;
        }
        let name = def.name.clone();
        let index = self.current_player_index();
        let Some(runtime) = self.take_mods() else {
            return false;
        };
        let has = runtime.has_item_callback(&name);
        if has {
            runtime.item_used(&mut GameHost(self), &name, index, aimed);
        }
        self.restore_mods(runtime);
        has
    }

    /// A mod command, if one is called `name`.
    pub(super) fn run_mod_command(
        &mut self,
        name: &str,
        index: usize,
        args: &[&str],
    ) -> Option<Result<Vec<String>, String>> {
        let runtime = self.take_mods()?;
        let out = runtime.run_command(&mut GameHost(self), name, index, args);
        self.restore_mods(runtime);
        out
    }

    /// The server tick: the `server_tick` event and the timers.
    pub(super) fn tick_mods(&mut self) {
        let Some(mut runtime) = self.take_mods() else {
            return;
        };
        let tick = self.time;
        if runtime.wants("server_tick") {
            runtime.fire(&mut GameHost(self), &Event::ServerTick { tick });
        }
        runtime.tick_timers_server(&mut GameHost(self));
        self.restore_mods(runtime);
    }

    /// A player took `result` out of a crafting grid.
    pub(super) fn mod_crafted(&mut self, result: &ItemStack) {
        let (player, count) = (self.turn, u32::from(result.count));
        let item = crate::world::block::qualified(result.id.name()).into_owned();
        self.mod_event(Event::ItemCrafted {
            player,
            item,
            count,
        });
    }

    /// The index of the player whose turn it is (0 outside LAN).
    fn current_player_index(&self) -> usize {
        self.turn
    }

    /// Gives `count` of `id` to player `index`, throwing what doesn't fit.
    pub fn give_to(&mut self, index: usize, id: ItemId, count: u32) {
        let items = self.items().clone();
        self.with_player(index, |g| {
            let max = u32::from(items.max_stack(id));
            let mut left = count;
            while left > 0 {
                let take = left.min(max);
                left -= take;
                if let Some(rest) = g
                    .player
                    .inventory
                    .add(ItemStack::new(id, take as u8), &items)
                {
                    g.throw(rest);
                }
            }
        });
    }

    /// The mods' view of player `index`.
    pub fn player_info(&self, index: usize) -> PlayerInfo {
        let p = self.player_at(index);
        PlayerInfo {
            index,
            name: p.name.clone(),
            feet: self.feet_of(index),
            dimension: Self::dimension_slug(p.dimension),
        }
    }
}
