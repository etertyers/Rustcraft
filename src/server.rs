//! The server: the world and every player in it, ticking 20 times a second
//! and telling each connected client what they see. Single player runs one
//! in the same process with a single client, so there's one way to play.

use std::{collections::VecDeque, sync::Arc};

use glam::DVec3;
use log::{info, warn};

use crate::game::commands::Reply;

use crate::{
    audio::{self, Cue},
    config,
    game::{Game, PlayerState},
    inventory::book::BookRecipe,
    net::{
        protocol::{ClientMsg, Input, PROTOCOL_VERSION, ServerMsg},
        transport::Connection,
    },
    world::{chunk::ChunkPos, dimension::Dimension},
};

/// The most players a world takes (vanilla LAN's default).
pub const MAX_PLAYERS: usize = 8;
/// Positioned sounds farther than this from a player aren't sent to them.
const HEARING: f64 = 64.0;
/// Inputs a client may get ahead by before the oldest are dropped.
const MAX_QUEUED_INPUTS: usize = 4;

pub type ServerConnection = Box<dyn Connection<ServerMsg, ClientMsg>>;

/// A world open to LAN.
struct Lan {
    listener: crate::net::lan::Listener,
    guest_mode: crate::player::mode::GameMode,
    cheats: bool,
}

/// A connection that hasn't logged in yet.
struct Pending {
    conn: ServerConnection,
}

/// A logged-in player's connection and what it has been told.
struct Remote {
    conn: ServerConnection,
    /// Movement input waiting for the player's next turns, and the last
    /// one played.
    inputs: VecDeque<Input>,
    ack: u32,
    /// Chunks the client has, and how far it sees.
    sent: crate::world::fast_hash::FastSet<ChunkPos>,
    view_distance: i32,
    /// The dimension its chunks are in.
    dimension: Dimension,
    /// The player's skin (PNG) and arm width, once sent.
    skin: Option<(Vec<u8>, bool)>,
}

pub struct Server {
    pub game: Game,
    /// Players in the same order as the game's: the host first, then
    /// `game.others`.
    remotes: Vec<Remote>,
    pending: Vec<Pending>,
    /// Every recipe in the book's order, for placing one from the book.
    book: Arc<Vec<BookRecipe>>,
    /// Open to LAN: the port listened on, and the guests' game mode and
    /// whether they may cheat (M38's commands).
    lan: Option<Lan>,
    /// A client changed something on a screen since the last snapshot, so
    /// one goes out even without a tick (the held stack follows the mouse
    /// at once).
    changed: bool,
    /// Players `/kick` sent away, who leave after the messages are read.
    kicked: Vec<usize>,
    /// `/stop` was run: whoever runs the server should save and close.
    pub stopping: bool,
}

impl Server {
    pub fn new(mut game: Game) -> Self {
        game.chunks.serve();
        game.keep_sounds();
        let book = Arc::new(crate::inventory::book::all(
            &game.items().recipes,
            game.items(),
        ));
        Self {
            game,
            remotes: Vec::new(),
            pending: Vec::new(),
            book,
            changed: false,
            lan: None,
            kicked: Vec::new(),
            stopping: false,
        }
    }

    /// Opens the world to LAN on `port` (0 for any free one); guests play
    /// in `guest_mode`. Returns the port.
    pub fn open_to_lan(
        &mut self,
        port: u16,
        guest_mode: crate::player::mode::GameMode,
        cheats: bool,
    ) -> std::io::Result<u16> {
        let world = self.game.world_name().unwrap_or_else(|| "World".to_owned());
        let motd = format!("{} - {world}", self.game.player.name);
        let listener = crate::net::lan::Listener::open(port, motd)?;
        let port = listener.port;
        self.game.guests_cheat = cheats;
        self.lan = Some(Lan {
            listener,
            guest_mode,
            cheats,
        });
        Ok(port)
    }

    /// Tells every guest the world is closing (the host saved and quit);
    /// the caller saves the world, everyone in it included.
    pub fn shut_down(&mut self) {
        for remote in self.remotes.iter_mut().skip(1) {
            remote.conn.send(ServerMsg::Disconnect(
                "The host closed the world".to_owned(),
            ));
        }
        self.lan = None;
    }

    /// The port the world is open to LAN on, if it is.
    pub fn lan_port(&self) -> Option<u16> {
        self.lan.as_ref().map(|lan| lan.listener.port)
    }

    /// Whether guests may use cheat commands (Open to LAN's Allow Cheats).
    pub fn cheats(&self) -> bool {
        self.lan.as_ref().is_some_and(|lan| lan.cheats)
    }

    /// Whether player `index` may cheat: the host in a world made with
    /// Allow Cheats, and everyone once Open to LAN allows it.
    fn may_cheat(&self, index: usize) -> bool {
        self.cheats()
            || (index == 0 && self.game.cheats)
            || self
                .game
                .admin
                .ops
                .contains(&self.game.player_at(index).name)
    }

    /// Sends a chat line to one player.
    fn tell(&mut self, index: usize, text: String, command: bool) {
        if let Some(remote) = self.remotes.get_mut(index) {
            remote.conn.send(ServerMsg::Chat { text, command });
        }
    }

    /// Sends a chat line to everyone.
    fn broadcast(&mut self, text: String) {
        info!("[chat] {text}");
        for remote in &mut self.remotes {
            remote.conn.send(ServerMsg::Chat {
                text: text.clone(),
                command: false,
            });
        }
    }

    /// A player's chat line: said to everyone, or run as a command.
    fn chat(&mut self, index: usize, line: &str) {
        // As vanilla, `§` isn't allowed in what players type.
        let line: String = crate::ui::chat::strip_codes(line)
            .chars()
            .take(crate::ui::chat::MAX_MESSAGE)
            .collect();
        let line = line.trim();
        if line.is_empty() {
            return;
        }
        let Some(command) = line.strip_prefix('/') else {
            let name = self.game.player_at(index).name.clone();
            self.game.mod_event(crate::mods::lua::Event::Chat {
                player: index,
                name: name.clone(),
                message: line.to_owned(),
            });
            let shown = self.game.board.dressed(&name);
            self.broadcast(format!("<{shown}> {line}"));
            return;
        };
        let cheats = self.may_cheat(index);
        let replies = self.game.run_command(index, command, cheats, index == 0);
        self.changed = true;
        self.route(Some(index), replies);
    }

    /// Delivers what a command said: to its sender (`Some`), or, for the
    /// server's own functions and command blocks (`None`), to those it names.
    fn route(&mut self, sender: Option<usize>, replies: Vec<Reply>) {
        let index = sender.unwrap_or(0);
        for reply in replies {
            match reply {
                Reply::Sender(_) if sender.is_none() => {}
                Reply::Sender(text) => self.tell(index, text, true),
                Reply::Player(to, text) => self.tell(to, text, false),
                Reply::Everyone(text) => self.broadcast(text),
                Reply::Rich {
                    to,
                    text,
                    click,
                    hover,
                } => {
                    if let Some(remote) = self.remotes.get_mut(to) {
                        remote.conn.send(ServerMsg::RichChat { text, click, hover });
                    }
                }
                Reply::Publish(_) if self.lan.is_some() => self.tell(
                    index,
                    "§cThe world is already published to LAN".to_owned(),
                    true,
                ),
                Reply::Publish(port) => {
                    let mode = self.game.default_mode.unwrap_or(self.game.player.mode);
                    match self.open_to_lan(port, mode, false) {
                        Ok(port) => {
                            self.tell(index, format!("Local game hosted on port {port}"), true)
                        }
                        Err(err) => {
                            self.tell(index, format!("§cFailed to start the server: {err}"), true)
                        }
                    }
                }
                Reply::Stop => self.stopping = true,
                Reply::Kick(who, reason) => {
                    if let Some(remote) = self.remotes.get_mut(who).filter(|_| who > 0) {
                        remote.conn.send(ServerMsg::Disconnect(reason));
                        self.kicked.push(who);
                    }
                }
            }
        }
    }

    /// Sends the game's own chat lines (deaths, advancements) to everyone.
    fn send_game_chat(&mut self) {
        let said = std::mem::take(&mut self.game.replies_out);
        if !said.is_empty() {
            self.route(None, said);
        }
        for line in std::mem::take(&mut self.game.chat_out) {
            self.broadcast(line);
        }
        for (to, line) in std::mem::take(&mut self.game.chat_to) {
            self.tell(to, line, false);
        }
    }

    /// A new connection; it joins when its login arrives.
    pub fn connect(&mut self, conn: ServerConnection) {
        self.pending.push(Pending { conn });
    }

    /// Players connected, the host first.
    pub fn player_count(&self) -> usize {
        self.remotes.len()
    }

    /// Reads what clients sent, runs the ticks that are due and tells each
    /// client the result.
    pub fn advance(&mut self, dt: f32) {
        self.accept();
        self.read_messages();
        // Lines from between ticks (a scene being set up) go out too.
        self.send_game_chat();
        // Chunks load around every player, in each one's dimension.
        for dimension in self.game.dimensions_with_players() {
            self.game.enter_realm(dimension);
            let centers = self.game.player_chunks();
            let changes = self.game.chunks.update_around(&centers);
            self.game.on_chunks_loaded(&changes.loaded);
        }
        self.home();
        let ticks = self.game.clock(dt);
        for _ in 0..ticks {
            self.tick();
        }
        // Chunks that finished loading since the last tick go out at once.
        self.send_chunks();
        // A stopped world (paused, or a scene being set up) still shows its
        // changes, and a screen's click shows before the next tick.
        if ticks == 0 && (dt == 0.0 || self.changed) {
            self.publish(Vec::new());
        }
        self.changed = false;
    }

    /// One tick: each player's next input, the world's tick, then what
    /// each player sees.
    fn tick(&mut self) {
        for (index, remote) in self.remotes.iter_mut().enumerate() {
            if let Some(input) = remote.inputs.pop_front() {
                remote.ack = input.seq;
                self.game.with_player(index, |game| game.take_input(&input));
            }
        }
        // Sounds queued before the tick are the client's own (this thread
        // also runs the host's client); only the tick's are the world's.
        let earlier = audio::drain();
        self.game.step();
        self.send_game_chat();
        let heard = audio::drain();
        for cue in earlier {
            audio::requeue(cue);
        }
        self.send_chunks();
        self.publish(heard);
    }

    /// Sends each player their snapshot and the sounds they hear.
    fn publish(&mut self, heard: Vec<Cue>) {
        let views: Vec<_> = (0..self.game.player_count())
            .map(|index| {
                let view = self.game.with_player(index, |game| game.player_view());
                (self.game.player_at(index).dimension, view)
            })
            .collect();
        let records = std::mem::take(&mut self.game.record_events);
        for index in 0..self.remotes.len() {
            let ack = self.remotes[index].ack;
            // What's in the chunks the client has, in their dimension.
            let dimension = self.game.player_at(index).dimension;
            self.game.enter_realm(dimension);
            let range = f64::from((self.remotes[index].view_distance + 1) * 16);
            let sees_spectators = self.game.player_at(index).mode.is_spectator();
            let here: Vec<_> = views
                .iter()
                .filter(|(d, view)| *d == dimension && (sees_spectators || !view.spectator))
                .map(|(_, view)| view.clone())
                .collect();
            let snapshot = self
                .game
                .with_player(index, |game| game.snapshot(ack, range, here));
            let feet = self.feet(index);
            self.remotes[index]
                .conn
                .send(ServerMsg::Snapshot(Box::new(snapshot)));
            // Jukebox songs starting within earshot, and any stopping.
            for event in &records {
                let middle = event.at.as_dvec3() + glam::DVec3::splat(0.5);
                let near = middle.distance(feet) <= crate::game::jukebox::HEARING_RANGE;
                if event.dimension == dimension && (event.song.is_none() || near) {
                    self.remotes[index].conn.send(ServerMsg::Record {
                        at: event.at,
                        song: event.song.clone(),
                    });
                }
            }
            // Sounds near them, and their own.
            let own = self
                .game
                .with_player(index, |game| std::mem::take(&mut game.player.heard));
            let sounds: Vec<Cue> = heard
                .iter()
                .filter(|cue| cue.at.is_some_and(|at| at.distance(feet) < HEARING))
                .cloned()
                .chain(own)
                .collect();
            if !sounds.is_empty() {
                self.remotes[index].conn.send(ServerMsg::Sounds(sounds));
            }
        }
        self.home();
    }

    /// Puts the host's dimension back in the game's fields.
    fn home(&mut self) {
        let dimension = self.game.player.dimension;
        self.game.enter_realm(dimension);
    }

    /// Takes in logins: a player with a free name joins, the first as the
    /// host (the world's own player), the rest as guests.
    fn accept(&mut self) {
        if let Some(lan) = &self.lan {
            for stream in lan.listener.arrivals() {
                match crate::net::transport::Tcp::new(stream) {
                    Ok(conn) => self.pending.push(Pending {
                        conn: Box::new(conn),
                    }),
                    Err(err) => warn!("a LAN connection failed: {err}"),
                }
            }
        }
        let mut still = Vec::new();
        for mut pending in std::mem::take(&mut self.pending) {
            let Some(msg) = pending.conn.try_recv() else {
                if pending.conn.is_open() {
                    still.push(pending);
                }
                continue;
            };
            let ClientMsg::Login {
                version,
                name,
                mods,
            } = msg
            else {
                continue;
            };
            let refusal = if version != PROTOCOL_VERSION {
                Some(format!(
                    "Incompatible version: the server runs protocol {PROTOCOL_VERSION}, you {version}"
                ))
            } else if let Some(lines) = self.mod_mismatch(&mods) {
                Some(format!(
                    "Your mods are not the host's:\n{}",
                    lines.join("\n")
                ))
            } else if !crate::options::valid_name(&name) {
                Some("Names are 3 to 16 letters, digits or underscores".to_owned())
            } else if self.remotes.len() >= MAX_PLAYERS {
                Some("The world is full".to_owned())
            } else if self.names().any(|taken| taken == name) {
                Some(format!("{name} is already playing"))
            } else {
                self.game.admin.refusal(&name)
            };
            if let Some(reason) = refusal {
                info!("refused {name}: {reason}");
                pending.conn.send(ServerMsg::Refused(reason));
                continue;
            }
            self.join(pending.conn, name);
        }
        self.pending.extend(still);
    }

    /// What differs between a guest's mods and this server's, if
    /// anything: the guest must have the same ones (not client-only).
    fn mod_mismatch(&self, theirs: &[(String, String)]) -> Option<Vec<String>> {
        let ours = crate::mods::active().required();
        let mut lines = crate::mods::differences(&ours, theirs);
        if lines.is_empty() {
            lines.extend(crate::mods::order_difference(&ours, theirs));
        }
        (!lines.is_empty()).then_some(lines)
    }

    fn names(&self) -> impl Iterator<Item = &str> {
        let host = (!self.remotes.is_empty()).then_some(self.game.player.name.as_str());
        host.into_iter()
            .chain(self.game.others.iter().map(|p| p.name.as_str()))
    }

    fn join(&mut self, mut conn: ServerConnection, name: String) {
        // The host plays the world's own player; a guest starts at the
        // spawn. Either way, a player who has been here before is put back
        // as they left.
        let index = self.remotes.len();
        if index == 0 {
            self.game.player.name = name.clone();
        } else {
            let spawn = self.game.spawn_point();
            let mut player = PlayerState::new(spawn, 0.0, 0.0);
            player.name = name.clone();
            player.mode = self.game.default_mode.unwrap_or_else(|| {
                self.lan
                    .as_ref()
                    .map_or(self.game.player.mode, |lan| lan.guest_mode)
            });
            self.game.others.push(player);
        }
        self.game.with_player(index, Game::restore_player);
        info!("{name} joined");
        conn.send(ServerMsg::Welcome {
            seed: self.game.chunks.seed(),
        });
        let dimension = self.game.dimension();
        conn.send(ServerMsg::Dimension(dimension));
        self.remotes.push(Remote {
            conn,
            inputs: VecDeque::new(),
            ack: 0,
            sent: Default::default(),
            view_distance: config::RENDER_DISTANCE,
            dimension,
            skin: None,
        });
        self.announce(format!("{name} joined the game"));
        let player = self.game.player_info(index);
        self.game
            .mod_event(crate::mods::lua::Event::PlayerJoin(player));
        // The newcomer sees everyone's skins.
        let index = self.remotes.len() - 1;
        for other in 0..index {
            if let Some((png, slim)) = self.remotes[other].skin.clone() {
                let name = self.game.player_at(other).name.clone();
                self.remotes[index]
                    .conn
                    .send(ServerMsg::Skin { name, png, slim });
            }
        }
    }

    /// Tells everyone in chat `text` (in yellow, as vanilla's join and
    /// leave lines) and who is playing now.
    fn announce(&mut self, text: String) {
        self.broadcast(format!("§e{text}"));
        let names: Vec<String> = (0..self.game.player_count())
            .map(|i| self.game.player_at(i).name.clone())
            .collect();
        for remote in &mut self.remotes {
            remote.conn.send(ServerMsg::Players(names.clone()));
        }
    }

    /// Plays each client's messages for its player.
    fn read_messages(&mut self) {
        let mut gone = Vec::new();
        for index in 0..self.remotes.len() {
            while let Some(msg) = self.remotes[index].conn.try_recv() {
                if let ClientMsg::Disconnect = msg {
                    gone.push(index);
                    break;
                }
                self.handle(index, msg);
            }
            if !self.remotes[index].conn.is_open() && !gone.contains(&index) {
                gone.push(index);
            }
        }
        gone.append(&mut self.kicked);
        gone.sort_unstable();
        gone.dedup();
        for index in gone.into_iter().rev() {
            self.leave(index);
        }
        self.home();
    }

    fn handle(&mut self, index: usize, msg: ClientMsg) {
        let book = self.book.clone();
        // What a player does happens in their dimension.
        let dimension = self.game.player_at(index).dimension;
        self.game.enter_realm(dimension);
        match msg {
            ClientMsg::Login { .. } | ClientMsg::Disconnect => {}
            ClientMsg::Skin { png, slim } => {
                let name = self.game.player_at(index).name.clone();
                for (other, remote) in self.remotes.iter_mut().enumerate() {
                    if other != index {
                        remote.conn.send(ServerMsg::Skin {
                            name: name.clone(),
                            png: png.clone(),
                            slim,
                        });
                    }
                }
                self.remotes[index].skin = Some((png, slim));
            }
            ClientMsg::Settings(settings) => {
                self.remotes[index].view_distance = settings.view_distance;
                self.game
                    .with_player(index, |game| game.take_settings(&settings));
                let widest = self.remotes.iter().map(|r| r.view_distance).max();
                if let Some(radius) = widest {
                    self.game.chunks.set_radius(radius);
                }
            }
            ClientMsg::Input(input) => {
                let inputs = &mut self.remotes[index].inputs;
                inputs.push_back(input);
                while inputs.len() > MAX_QUEUED_INPUTS {
                    inputs.pop_front();
                }
            }
            ClientMsg::Action {
                action,
                pressed,
                repeat,
                grabbed,
            } => self.game.with_player(index, |game| {
                game.on_action(action, pressed, repeat, grabbed);
            }),
            ClientMsg::SelectSlot(slot) => self.game.with_player(index, |game| {
                game.player.inventory.selected =
                    slot.min(crate::inventory::player::HOTBAR_SLOTS - 1);
            }),
            ClientMsg::Screen(action) => {
                self.changed = true;
                self.game
                    .with_player(index, |game| game.apply_screen_action(action, &book));
            }
            ClientMsg::OpenInventory => {
                self.changed = true;
                self.game.with_player(index, Game::open_inventory);
            }
            ClientMsg::CloseScreen => {
                self.changed = true;
                self.game.with_player(index, Game::close_container);
            }
            ClientMsg::FinishCredits => self.game.with_player(index, Game::finish_credits),
            ClientMsg::ReleaseInput => self.game.with_player(index, Game::release_input),
            ClientMsg::Fly { feet } => self.game.with_player(index, |game| game.fly_to(feet)),
            ClientMsg::Chat(line) => self.chat(index, &line),
            ClientMsg::SpectatorTeleport(name) => {
                self.game.spectator_teleport(index, &name);
            }
            ClientMsg::CommandBlock(edit) => {
                self.game
                    .with_player(index, |g| g.finish_command_block_edit(edit));
            }
            ClientMsg::SpectatorTeleportTeam(team) => {
                self.game.spectator_teleport_team(index, &team);
            }
            ClientMsg::Complete(line) => {
                let cheats = self.may_cheat(index);
                let options = self.game.complete_command(&line, cheats, index == 0);
                self.remotes[index]
                    .conn
                    .send(ServerMsg::Completions(options));
            }
        }
    }

    /// A guest left (the host leaving ends the server, which its owner
    /// saves and drops).
    fn leave(&mut self, index: usize) {
        let remote = self.remotes.remove(index);
        drop(remote);
        if index == 0 {
            warn!("the host left");
            return;
        }
        if let Err(err) = self.game.with_player(index, |game| game.save_player()) {
            warn!("saving a leaving player: {err:#}");
        }
        let info = self.game.player_info(index);
        self.game
            .mod_event(crate::mods::lua::Event::PlayerLeave(info));
        let player = self.game.others.remove(index - 1);
        info!("{} left", player.name);
        self.announce(format!("{} left the game", player.name));
    }

    /// Sends every client the chunks around it it doesn't have, the edits
    /// in the ones it has, and takes away the ones out of its view.
    fn send_chunks(&mut self) {
        let dimensions: Vec<Dimension> = self.game.live_dimensions();
        for dimension in dimensions {
            self.game.enter_realm(dimension);
            self.send_chunks_here(dimension);
        }
        self.home();
    }

    /// `send_chunks` for the players in `dimension`, whose world is in the
    /// game's fields.
    fn send_chunks_here(&mut self, dimension: Dimension) {
        let (edits, rewritten) = self.game.chunks.take_edits();
        for index in 0..self.remotes.len() {
            if self.game.player_at(index).dimension != dimension {
                continue;
            }
            let center = {
                let feet = self.feet(index);
                ChunkPos::of_block(feet.x.floor() as i32, feet.z.floor() as i32)
            };
            let world = self.game.chunks.world();
            let remote = &mut self.remotes[index];
            if remote.dimension != dimension {
                remote.dimension = dimension;
                remote.sent.clear();
                remote.conn.send(ServerMsg::Dimension(dimension));
            }
            for urgent in [true, false] {
                let mine: Vec<_> = edits
                    .iter()
                    .filter(|(at, _, now)| {
                        *now == urgent && remote.sent.contains(&ChunkPos::of_block(at.x, at.z))
                    })
                    .map(|&(at, block, _)| (at, block))
                    .collect();
                if !mine.is_empty() {
                    remote.conn.send(ServerMsg::Blocks {
                        edits: mine,
                        urgent,
                    });
                }
            }
            for pos in &rewritten {
                if remote.sent.contains(pos)
                    && let Some(chunk) = world.get(*pos)
                {
                    remote.conn.send(ServerMsg::Chunk(chunk.clone()));
                }
            }
            // One ring past the view, so the client can mesh its border.
            let reach = remote.view_distance + 1;
            let far: Vec<ChunkPos> = remote
                .sent
                .iter()
                .copied()
                .filter(|pos| (pos.x - center.x).abs().max((pos.z - center.z).abs()) > reach)
                .collect();
            for pos in far {
                remote.sent.remove(&pos);
                remote.conn.send(ServerMsg::Unload(pos));
            }
            for dz in -reach..=reach {
                for dx in -reach..=reach {
                    let pos = center.offset(dx, dz);
                    if !remote.sent.contains(&pos)
                        && let Some(chunk) = world.get(pos)
                    {
                        remote.sent.insert(pos);
                        remote.conn.send(ServerMsg::Chunk(chunk.clone()));
                    }
                }
            }
        }
    }

    fn feet(&self, index: usize) -> DVec3 {
        self.game.feet_of(index)
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use glam::IVec3;

    use super::*;
    use crate::{
        bindings::Action,
        client::Client,
        game::tests::{mob_registry, registries},
        inventory::{items, stack::ItemStack},
        net::{protocol::PlayerSettings, transport::local_pair},
        player::mode::GameMode,
        world::{
            blocks,
            manager::{ChunkManager, ChunkSource},
        },
    };

    fn settings() -> PlayerSettings {
        PlayerSettings {
            view_distance: 2,
            auto_jump: false,
            toggle_sneak: false,
            toggle_sprint: false,
            left_handed: false,
            skin_layers: crate::render::player::ALL_LAYERS,
            particles: crate::options::Particles::All,
        }
    }

    /// A flat creative world on a server, and clients for `names`.
    fn world_with(names: &[&str]) -> (Server, Vec<(Client, Game)>) {
        let (items, models) = registries();
        let mobs = mob_registry();
        let chunks = ChunkManager::new(ChunkSource::Flat, models.clone(), 2, 2);
        let feet = DVec3::new(0.5, 64.0, 0.5);
        let mut game =
            Game::new(chunks, items.clone(), feet, feet, 0.0, 0.0).with_mobs(mobs.clone());
        game.player.mode = GameMode::Creative;
        let mut server = Server::new(game);
        let clients = names
            .iter()
            .map(|name| {
                let (server_end, client_end) = local_pair();
                server.connect(Box::new(server_end));
                let client = Client::join(Box::new(client_end), name, settings());
                let game = Game::new_client(items.clone(), models.clone(), mobs.clone(), 2, 1);
                (client, game)
            })
            .collect();
        (server, clients)
    }

    /// Runs frames of every client and the server until `done`.
    fn run_until(
        server: &mut Server,
        clients: &mut [(Client, Game)],
        done: impl Fn(&Server, &[(Client, Game)]) -> bool,
    ) {
        let deadline = Instant::now() + Duration::from_secs(30);
        while !done(server, clients) {
            assert!(
                Instant::now() < deadline,
                "never happened: {} players; server {:?}; clients {:?}",
                server.player_count(),
                block_at(&server.game, IVec3::new(0, 63, 0)),
                clients
                    .iter()
                    .map(|(c, g)| (
                        c.welcomed,
                        g.chunks.world().len(),
                        block_at(g, IVec3::new(0, 63, 0))
                    ))
                    .collect::<Vec<_>>()
            );
            let dt = 0.05;
            for (client, game) in clients.iter_mut() {
                client.advance(game, dt);
            }
            server.advance(dt);
            for (client, game) in clients.iter_mut() {
                client.receive(game);
                game.chunks.update(ChunkPos::new(0, 0));
            }
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    fn block_at(game: &Game, at: IVec3) -> Option<crate::world::block::BlockState> {
        game.chunks.world().block(at.x, at.y, at.z)
    }

    #[test]
    fn two_clients_build_and_break_and_everyone_sees_the_same_blocks() {
        let (mut server, mut clients) = world_with(&["Alice", "Bob"]);
        let grass = |at: IVec3| {
            move |game: &Game| block_at(game, at).is_some_and(|b| b.is(blocks::GRASS_BLOCK))
        };
        run_until(&mut server, &mut clients, |server, clients| {
            server.player_count() == 2
                && clients
                    .iter()
                    .all(|(_, game)| grass(IVec3::new(3, 63, 3))(game))
        });
        assert_eq!(server.game.player.name, "Alice", "the first in is the host");
        assert_eq!(server.game.others[0].name, "Bob");
        // Alice stands at (3, 3) looking straight down; Bob just south of
        // the block at (0, 63, 0), looking down at its top.
        server
            .game
            .with_player(0, |game| game.fly_to(DVec3::new(3.5, 64.0, 3.5)));
        server
            .game
            .with_player(1, |game| game.fly_to(DVec3::new(0.5, 64.0, 1.5)));
        clients[0].1.player.camera.pitch = -std::f32::consts::FRAC_PI_2;
        clients[1].1.player.camera.pitch = -70f32.to_radians();
        let press = |client: &mut Client, action, pressed| {
            client.send(ClientMsg::Action {
                action,
                pressed,
                repeat: false,
                grabbed: true,
            });
        };
        // Alice punches out the grass under her (creative breaks at once).
        press(&mut clients[0].0, Action::Attack, true);
        let gone = |game: &Game| block_at(game, IVec3::new(3, 63, 3)).is_some_and(|b| b.is_air());
        run_until(&mut server, &mut clients, |server, clients| {
            gone(&server.game) && clients.iter().all(|(_, game)| gone(game))
        });
        press(&mut clients[0].0, Action::Attack, false);
        // Bob puts stone on the block in front of him.
        server.game.others[0]
            .inventory
            .set(0, Some(ItemStack::new(items::STONE, 64)));
        press(&mut clients[1].0, Action::Use, true);
        let stone =
            |game: &Game| block_at(game, IVec3::new(0, 64, 0)).is_some_and(|b| b.is(blocks::STONE));
        run_until(&mut server, &mut clients, |server, clients| {
            stone(&server.game) && clients.iter().all(|(_, game)| stone(game))
        });
        press(&mut clients[1].0, Action::Use, false);
        // Each client's player is where the server has them.
        for (index, (_, game)) in clients.iter().enumerate() {
            let feet = game.player.body_state().position();
            assert!(feet.distance(server.game.feet_of(index)) < 1e-6, "{feet}");
        }
        // And every chunk a client has agrees with the server's, block for
        // block.
        for (_, game) in &clients {
            for pos in game.chunks.world().positions() {
                let ours = game.chunks.world().get(pos).unwrap();
                let theirs = server
                    .game
                    .chunks
                    .world()
                    .get(pos)
                    .expect("the server has it");
                for y in 60..68 {
                    for z in 0..16 {
                        for x in 0..16 {
                            assert_eq!(
                                ours.get(x, y, z),
                                theirs.get(x, y, z),
                                "{pos:?} {x} {y} {z}"
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn joining_a_stopped_world_puts_the_camera_with_the_player() {
        let (mut server, mut clients) = world_with(&["Alice"]);
        server.game.fly_to(DVec3::new(-300.5, 80.0, 41.5));
        let (client, game) = &mut clients[0];
        for _ in 0..3 {
            client.advance(game, 0.0);
            server.advance(0.0);
            client.receive(game);
        }
        let eye = game.player.camera.position.as_dvec3();
        assert!(eye.distance(DVec3::new(-300.5, 81.62, 41.5)) < 0.5, "{eye}");
    }

    #[test]
    fn a_guest_joins_over_tcp_and_plays_in_the_lan_game_mode() {
        let (mut server, mut clients) = world_with(&["Alice"]);
        let port = server
            .open_to_lan(0, GameMode::Survival, false)
            .expect("open to LAN");
        assert_eq!(server.lan_port(), Some(port));
        let stream = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
        let conn = crate::net::transport::Tcp::new(stream).unwrap();
        let (items, models) = registries();
        let guest = Client::join(Box::new(conn), "Bob", settings());
        clients.push((guest, Game::new_client(items, models, mob_registry(), 2, 1)));
        run_until(&mut server, &mut clients, |server, clients| {
            let (bob, game) = &clients[1];
            server.player_count() == 2
                && bob.welcomed
                && block_at(game, IVec3::new(0, 63, 0)).is_some_and(|b| b.is(blocks::GRASS_BLOCK))
                && game.player.name == "Bob"
        });
        assert_eq!(
            server.game.others[0].mode,
            GameMode::Survival,
            "the LAN mode"
        );
        assert_eq!(
            server.game.player.mode,
            GameMode::Creative,
            "the host keeps theirs"
        );
        assert_eq!(clients[1].0.players, ["Alice", "Bob"], "the Tab list");
        run_until(&mut server, &mut clients, |_, c| {
            heard(&c[0].0, "§eBob joined the game")
        });
        // Leaving takes Bob out of the world.
        clients.pop();
        run_until(&mut server, &mut clients, |server, c| {
            server.player_count() == 1 && heard(&c[0].0, "§eBob left the game")
        });
        assert!(server.game.others.is_empty());
    }

    #[test]
    fn players_see_each_other_and_each_others_skins() {
        let (mut server, mut clients) = world_with(&["Alice", "Bob"]);
        clients[0].0.send(ClientMsg::Skin {
            png: vec![1, 2, 3],
            slim: true,
        });
        run_until(&mut server, &mut clients, |server, _| {
            server.player_count() == 2
        });
        server
            .game
            .with_player(1, |game| game.fly_to(DVec3::new(3.5, 64.0, 0.5)));
        run_until(&mut server, &mut clients, |_, clients| {
            clients[0]
                .1
                .remote_players
                .iter()
                .any(|v| v.name == "Bob" && v.feet.x > 3.0)
                && clients[1]
                    .1
                    .remote_players
                    .iter()
                    .any(|v| v.name == "Alice")
                && !clients[1].0.skins.is_empty()
        });
        let bob = clients[0]
            .1
            .remote_players
            .iter()
            .find(|v| v.name == "Bob")
            .unwrap();
        assert!(
            bob.feet.distance(DVec3::new(3.5, 64.0, 0.5)) < 0.01,
            "{} {}",
            bob.feet,
            server.game.feet_of(1)
        );
        assert!(
            clients[0]
                .1
                .remote_players
                .iter()
                .all(|v| v.name != "Alice"),
            "not yourself"
        );
        assert_eq!(
            clients[1].0.skins[0],
            ("Alice".to_owned(), vec![1, 2, 3], true)
        );
        // Someone joining later gets the skins already sent.
        let (server_end, client_end) = local_pair();
        server.connect(Box::new(server_end));
        let (items, models) = registries();
        let carol = Client::join(Box::new(client_end), "Carol", settings());
        clients.push((carol, Game::new_client(items, models, mob_registry(), 2, 1)));
        run_until(&mut server, &mut clients, |_, clients| {
            !clients[2].0.skins.is_empty()
        });
        assert_eq!(clients[2].0.skins[0].0, "Alice");
    }

    #[test]
    fn a_taken_name_and_another_version_are_refused() {
        let (mut server, mut clients) = world_with(&["Alice", "Alice"]);
        run_until(&mut server, &mut clients, |_, clients| {
            clients[1].0.gone.is_some()
        });
        assert_eq!(
            clients[1].0.gone.as_deref(),
            Some("Alice is already playing")
        );
        assert!(clients[0].0.gone.is_none() && clients[0].0.welcomed);
        let (server_end, mut client_end) = local_pair();
        server.connect(Box::new(server_end));
        client_end.send(ClientMsg::Login {
            version: PROTOCOL_VERSION + 1,
            name: "Carol".to_owned(),
            mods: Vec::new(),
        });
        server.advance(0.0);
        assert!(matches!(client_end.try_recv(), Some(ServerMsg::Refused(_))));
        assert_eq!(server.player_count(), 1);
    }

    fn heard(client: &Client, text: &str) -> bool {
        client.chat.iter().any(|(line, _)| line == text)
    }

    #[test]
    fn two_players_chat_see_deaths_and_cheats_follow_the_world() {
        let (mut server, mut clients) = world_with(&["Kenzo", "Alex"]);
        run_until(&mut server, &mut clients, |s, c| {
            s.player_count() == 2 && c.iter().all(|(client, _)| client.welcomed)
        });
        run_until(&mut server, &mut clients, |_, c| {
            heard(&c[0].0, "§eAlex joined the game")
        });
        clients[1]
            .0
            .send(ClientMsg::Chat("hello §cthere".to_owned()));
        run_until(&mut server, &mut clients, |_, c| {
            c.iter()
                .all(|(client, _)| heard(client, "<Alex> hello there"))
        });
        // Cheats are off: a cheat command is refused, a harmless one runs.
        clients[1]
            .0
            .send(ClientMsg::Chat("/give @s diamond".to_owned()));
        clients[1].0.send(ClientMsg::Chat("/list".to_owned()));
        run_until(&mut server, &mut clients, |_, c| {
            heard(
                &c[1].0,
                "There are 2 of a max of 8 players online: Kenzo, Alex",
            )
        });
        assert!(heard(
            &clients[1].0,
            "§cYou do not have permission to use this command"
        ));
        assert!(
            clients[1]
                .0
                .chat
                .iter()
                .all(|(line, command)| !line.contains("max of") || *command)
        );
        assert!(!heard(
            &clients[0].0,
            "There are 2 of a max of 8 players online: Kenzo, Alex"
        ));
        // A world made with Allow Cheats: the host may cheat (guests only
        // once Open to LAN allows it).
        server.game.cheats = true;
        assert!(server.may_cheat(0) && !server.may_cheat(1));
        clients[0]
            .0
            .send(ClientMsg::Chat("/gamemode survival Alex".to_owned()));
        clients[0].0.send(ClientMsg::Chat("/kill Alex".to_owned()));
        run_until(&mut server, &mut clients, |_, c| {
            c.iter().all(|(client, _)| heard(client, "Alex was killed"))
        });
        // Tab asks the server.
        clients[0].0.send(ClientMsg::Complete("gamer".to_owned()));
        run_until(&mut server, &mut clients, |_, c| {
            c[0].0.completions.is_some()
        });
        assert_eq!(
            clients[0].0.completions.as_deref(),
            Some(&["gamerule".to_owned()][..])
        );
    }

    #[test]
    fn spectators_are_unseen_spectate_and_teleport_the_same_for_every_client() {
        let (mut server, mut clients) = world_with(&["Alice", "Bob"]);
        server.game.cheats = true;
        run_until(&mut server, &mut clients, |s, c| {
            s.player_count() == 2 && c.iter().all(|(client, _)| client.welcomed)
        });
        // Alice, the host, can see Bob until he turns to spectator.
        run_until(&mut server, &mut clients, |_, c| {
            c[0].1.remote_players.iter().any(|v| v.name == "Bob")
        });
        clients[0]
            .0
            .send(ClientMsg::Chat("/gamemode spectator Bob".to_owned()));
        run_until(&mut server, &mut clients, |s, c| {
            s.game.others[0].mode == GameMode::Spectator
                && c[1].1.player.mode == GameMode::Spectator
                && !c[0].1.remote_players.iter().any(|v| v.name == "Bob")
        });
        // Bob sees Alice; Alice, a survivor, doesn't see Bob; two
        // spectators see each other.
        clients[0]
            .0
            .send(ClientMsg::Chat("/gamemode spectator Alice".to_owned()));
        run_until(&mut server, &mut clients, |_, c| {
            c[0].1.player.mode == GameMode::Spectator
                && c[0].1.remote_players.iter().any(|v| v.name == "Bob")
                && c[1].1.remote_players.iter().any(|v| v.name == "Alice")
        });
        // Alice sets Bob to spectate her: his eyes follow hers on both screens.
        server
            .game
            .with_player(0, |game| game.fly_to(DVec3::new(6.5, 70.0, 6.5)));
        clients[0]
            .0
            .send(ClientMsg::Chat("/spectate Alice Bob".to_owned()));
        run_until(&mut server, &mut clients, |s, c| {
            let alice = s.game.feet_of(0);
            heard(&c[0].0, "Now spectating Alice")
                && (s.game.feet_of(1) - alice).length() < 3.0
                && (c[1].1.feet_of(0) - alice).length() < 3.0
        });
        // Letting go (a new mode clears it) and picking Alice in the menu.
        clients[0]
            .0
            .send(ClientMsg::Chat("/gamemode spectator Bob".to_owned()));
        server
            .game
            .with_player(0, |game| game.fly_to(DVec3::new(-8.5, 70.0, -8.5)));
        clients[1]
            .0
            .send(ClientMsg::SpectatorTeleport("Alice".to_owned()));
        run_until(&mut server, &mut clients, |s, c| {
            let alice = s.game.feet_of(0);
            (s.game.feet_of(1) - alice).length() < 1.5 && (c[1].1.feet_of(0) - alice).length() < 1.5
        });
        // Not a spectator, not allowed.
        clients[0]
            .0
            .send(ClientMsg::Chat("/gamemode survival Bob".to_owned()));
        run_until(&mut server, &mut clients, |s, _| {
            s.game.others[0].mode == GameMode::Survival
        });
        server
            .game
            .with_player(0, |game| game.fly_to(DVec3::new(12.5, 70.0, 12.5)));
        clients[1]
            .0
            .send(ClientMsg::SpectatorTeleport("Alice".to_owned()));
        for _ in 0..20 {
            server.advance(0.05);
        }
        assert!((server.game.feet_of(1) - server.game.feet_of(0)).length() > 5.0);
    }

    #[test]
    fn scoreboard_teams_titles_bars_and_rich_chat_reach_every_client() {
        let (mut server, mut clients) = world_with(&["Alice", "Bob"]);
        server.game.cheats = true;
        run_until(&mut server, &mut clients, |s, c| {
            s.player_count() == 2 && c.iter().all(|(client, _)| client.welcomed)
        });
        for line in [
            "/scoreboard objectives add kills dummy {\"text\":\"Kills\"}",
            "/scoreboard objectives setdisplay sidebar kills",
            "/scoreboard objectives setdisplay list kills",
            "/scoreboard players set Alice kills 3",
            "/scoreboard players set Bob kills 8",
            "/team add red",
            "/team modify red color red",
            "/team join red Alice",
            "/bossbar add boss {\"text\":\"Dragon\"}",
            "/bossbar set boss players @a",
            "/bossbar set boss max 200",
            "/bossbar set boss value 50",
            "/title Bob title {\"text\":\"Hello Bob\"}",
            "/tellraw Bob {\"text\":\"Click me\",\"clickEvent\":{\"action\":\"run_command\",\"value\":\"/list\"}}",
        ] {
            clients[0].0.send(ClientMsg::Chat(line.to_owned()));
        }
        run_until(&mut server, &mut clients, |_, c| {
            c.iter().all(|(_, g)| {
                let v = &g.board_view;
                v.sidebar.as_ref().is_some_and(|s| {
                    s.title == "§rKills"
                        && s.rows
                            .first()
                            .is_some_and(|(n, score)| n == "Bob" && score == "§c8")
                        && s.rows.iter().any(|(n, _)| n == "§cAlice§r")
                }) && v.bars.len() == 1
                    && v.bars[0].value == 50
                    && v.bars[0].max == 200
                    && v.dressed
                        .iter()
                        .any(|(n, d)| n == "Alice" && d == "§cAlice§r")
                    && v.list.len() == 2
            }) && c[1].1.player.title.title == "§rHello Bob"
                && c[1].0.chat_events.values().any(|(click, _)| {
                    click
                        .as_ref()
                        .is_some_and(|(a, v)| a == "run_command" && v == "/list")
                })
        });
        assert!(
            clients[0].1.player.title.title.is_empty(),
            "only Bob got the title"
        );
        // Chat shows the team's colored name, to everyone.
        clients[0].0.send(ClientMsg::Chat("hello".to_owned()));
        run_until(&mut server, &mut clients, |_, c| {
            c.iter()
                .all(|(client, _)| heard(client, "<§cAlice§r> hello"))
        });
        // A team message goes to its members only.
        clients[0]
            .0
            .send(ClientMsg::Chat("/teammsg team only".to_owned()));
        run_until(&mut server, &mut clients, |_, c| {
            heard(&c[0].0, "-> red <§cAlice§r> team only")
        });
        assert!(!c_heard_team(&clients[1].0));
        // A spectator's menu can go to a team's member.
        server
            .game
            .with_player(0, |game| game.fly_to(DVec3::new(9.5, 70.0, 9.5)));
        clients[0]
            .0
            .send(ClientMsg::Chat("/gamemode spectator Bob".to_owned()));
        run_until(&mut server, &mut clients, |s, _| {
            s.game.others[0].mode == GameMode::Spectator
        });
        clients[1]
            .0
            .send(ClientMsg::SpectatorTeleportTeam("red".to_owned()));
        run_until(&mut server, &mut clients, |s, _| {
            (s.game.feet_of(1) - s.game.feet_of(0)).length() < 1.5
        });
    }

    fn c_heard_team(client: &Client) -> bool {
        client.chat.iter().any(|(l, _)| l.contains("team only"))
    }

    #[test]
    fn a_command_block_chain_and_a_pack_function_play_out_for_both_clients() {
        use crate::game::command_blocks::{Edit, Mode};
        let (mut server, mut clients) = world_with(&["Alice", "Bob"]);
        server.game.cheats = true;
        run_until(&mut server, &mut clients, |s, c| {
            s.player_count() == 2 && c.iter().all(|(client, _)| client.welcomed)
        });
        // A data pack with a tick function counting ticks.
        let root = std::env::temp_dir().join(format!("rc-m45-server-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let pack = root.join("datapacks/p");
        std::fs::create_dir_all(pack.join("data/demo/function")).unwrap();
        std::fs::create_dir_all(pack.join("data/minecraft/tags/function")).unwrap();
        std::fs::write(
            pack.join("pack.mcmeta"),
            r#"{"pack":{"pack_format":48,"description":"p"}}"#,
        )
        .unwrap();
        std::fs::write(
            pack.join("data/demo/function/t.mcfunction"),
            "scoreboard players add #ticks n 1\n",
        )
        .unwrap();
        std::fs::write(
            pack.join("data/minecraft/tags/function/tick.json"),
            r#"{"values":["demo:t"]}"#,
        )
        .unwrap();
        server.game.world_dir_for_packs = Some(root.clone());
        server.game.reload_packs();
        for line in [
            "/scoreboard objectives add n dummy {\"text\":\"Count\"}",
            "/scoreboard objectives setdisplay sidebar n",
        ] {
            clients[0].0.send(ClientMsg::Chat(line.to_owned()));
        }
        // Alice's block, set up through the editor's message; Bob's is refused.
        run_until(&mut server, &mut clients, |s, c| {
            block_at(&s.game, IVec3::new(4, 70, 4)).is_some()
                && c.iter()
                    .all(|(_, g)| block_at(g, IVec3::new(4, 70, 4)).is_some())
        });
        let base = IVec3::new(4, 70, 4);
        let facing = crate::world::Face::East;
        let kinds = [
            (Mode::Repeat, "scoreboard players add #cmd n 1"),
            (Mode::Chain, "scoreboard players add #chain n 1"),
        ];
        for (i, (mode, _)) in kinds.iter().enumerate() {
            let state = crate::world::block::BlockState::of(mode.block())
                .with_face(crate::world::props::Prop::FacingAll, facing);
            server
                .game
                .set_blocks(&[(base + IVec3::X * i as i32, state)]);
        }
        for (i, (mode, command)) in kinds.iter().enumerate() {
            let edit = Edit {
                at: (base + IVec3::X * i as i32).to_array(),
                command: (*command).to_owned(),
                mode: *mode,
                conditional: false,
                auto: true,
                track_output: true,
                last_output: String::new(),
            };
            clients[0]
                .0
                .send(ClientMsg::CommandBlock(Some(edit.clone())));
            if i == 0 {
                let mut hostile = edit;
                hostile.command = "kill @a".to_owned();
                clients[1].0.send(ClientMsg::CommandBlock(Some(hostile)));
            }
        }
        run_until(&mut server, &mut clients, |_, c| {
            c.iter().all(|(_, g)| {
                g.board_view.sidebar.as_ref().is_some_and(|s| {
                    let has = |name: &str| s.rows.iter().any(|(n, v)| n == name && v.len() > 2);
                    has("#cmd") && has("#chain") && has("#ticks")
                })
            })
        });
        let cmd = server.game.board.score("n", "#cmd").unwrap();
        let chain = server.game.board.score("n", "#chain").unwrap();
        assert!(
            (cmd - chain).abs() <= 1,
            "the chain runs with its repeater: {cmd} {chain}"
        );
        assert!(
            server.game.board.score("n", "#ticks").unwrap() > 0,
            "the pack's tick function ran"
        );
        assert!(
            server.game.player.stats.health > 0.0 && server.game.others[0].stats.health > 0.0,
            "Bob's edit was refused"
        );
        // Both clients see the same blocks; only the op hears the output.
        run_until(&mut server, &mut clients, |_, c| {
            c.iter().all(|(_, g)| {
                block_at(g, base).is_some_and(|b| b.is(blocks::REPEATING_COMMAND_BLOCK))
                    && block_at(g, base + IVec3::X)
                        .is_some_and(|b| b.is(blocks::CHAIN_COMMAND_BLOCK))
            })
        });
        assert!(
            heard(
                &clients[0].0,
                "§7§o[@: Added 1 to [Count] for #cmd (now 1)]"
            ) || clients[0]
                .0
                .chat
                .iter()
                .any(|(l, _)| l.starts_with("§7§o[@: Added 1 to"))
        );
        assert!(
            !clients[1]
                .0
                .chat
                .iter()
                .any(|(l, _)| l.starts_with("§7§o[@:")),
            "Bob isn't an operator"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// How many of `name` player `inventory` holds (hotbar and main).
    fn held(game: &Game, name: &str) -> u32 {
        let id = crate::inventory::items::by_name(name).unwrap();
        (0..crate::inventory::player::SLOTS)
            .filter_map(|i| game.player.inventory.slot(i))
            .filter(|stack| stack.id == id)
            .map(|stack| u32::from(stack.count))
            .sum()
    }

    #[test]
    fn a_mods_command_event_and_block_play_out_the_same_for_every_client() {
        let (mut server, mut clients) = world_with(&["Alice", "Bob"]);
        server.game.cheats = true;
        run_until(&mut server, &mut clients, |s, c| {
            s.player_count() == 2
                && c.iter().all(|(client, game)| {
                    client.welcomed && block_at(game, IVec3::new(3, 63, 3)).is_some()
                })
        });
        let ruby = crate::world::block::BlockState::of(blocks::by_name("ruby:ruby_block").unwrap());
        let at = IVec3::new(3, 64, 3);
        server.game.set_blocks(&[(at, ruby)]);
        let is_ruby = move |game: &Game| block_at(game, at) == Some(ruby);
        run_until(&mut server, &mut clients, |_, c| {
            c.iter().all(|(_, game)| is_ruby(game))
        });
        // /rubies is the Ruby mod's command, run on the server.
        clients[0].0.send(ClientMsg::Chat("/rubies 3".to_owned()));
        run_until(&mut server, &mut clients, |s, c| {
            held(&s.game, "ruby:ruby") == 3 && held(&c[0].1, "ruby:ruby") == 3
        });
        assert_eq!(held(&clients[1].1, "ruby:ruby"), 0, "only Alice got them");
        // A bad count comes back as the command's error.
        clients[0].0.send(ClientMsg::Chat("/rubies 99".to_owned()));
        run_until(&mut server, &mut clients, |_, c| {
            heard(&c[0].0, "§cThe count must be from 1 to 64")
        });
        // Bob has no permission for the command without cheats.
        clients[1].0.send(ClientMsg::Chat("/rubies".to_owned()));
        run_until(&mut server, &mut clients, |_, c| {
            heard(&c[1].0, "§cYou do not have permission to use this command")
        });
        // Alice breaks the block of ruby: the mod's block_break handler
        // writes to her chat and hands her gold, and both clients see the
        // block gone.
        server
            .game
            .with_player(0, |game| game.fly_to(DVec3::new(3.5, 66.0, 3.5)));
        clients[0].1.player.camera.pitch = -std::f32::consts::FRAC_PI_2;
        clients[0].0.send(ClientMsg::Action {
            action: Action::Attack,
            pressed: true,
            repeat: false,
            grabbed: true,
        });
        run_until(&mut server, &mut clients, |s, c| {
            block_at(&s.game, at).is_some_and(|b| b.is_air())
                && c.iter()
                    .all(|(_, game)| block_at(game, at).is_some_and(|b| b.is_air()))
                && held(&s.game, "gold_nugget") == 3
                && held(&c[0].1, "gold_nugget") == 3
                && heard(&c[0].0, "A block of ruby was broken at 3, 64, 3")
        });
        assert!(
            !clients[1]
                .0
                .chat
                .iter()
                .any(|(l, _)| l.contains("block of ruby")),
            "the message was for Alice only"
        );
    }

    #[test]
    fn a_guest_with_other_mods_is_refused_and_told_which_differ() {
        let (mut server, _clients) = world_with(&[]);
        let try_login = |server: &mut Server, mods: Vec<(String, String)>| {
            let (server_end, mut client_end) = local_pair();
            server.connect(Box::new(server_end));
            client_end.send(ClientMsg::Login {
                version: PROTOCOL_VERSION,
                name: "Carol".to_owned(),
                mods,
            });
            server.advance(0.0);
            client_end.try_recv()
        };
        // None at all: the host's required mods are missing.
        let Some(ServerMsg::Refused(why)) = try_login(&mut server, Vec::new()) else {
            panic!("a guest with no mods should be refused");
        };
        assert!(why.contains("missing: ruby 1.0.0"), "{why}");
        assert!(
            !why.contains("fullbright"),
            "client-only mods needn't match: {why}"
        );
        // The wrong version of the right mod.
        let Some(ServerMsg::Refused(why)) =
            try_login(&mut server, vec![("ruby".to_owned(), "0.9".to_owned())])
        else {
            panic!("the wrong version should be refused");
        };
        assert!(why.contains("ruby: host has 1.0.0, you have 0.9"), "{why}");
        // The same mods get in.
        let required = crate::mods::active().required();
        assert!(matches!(
            try_login(&mut server, required),
            Some(ServerMsg::Welcome { .. })
        ));
    }

    #[test]
    fn crops_and_calves_grow_on_the_server_and_both_clients_see_the_same() {
        let (mut server, mut clients) = world_with(&["Alice", "Bob"]);
        server.game.cheats = true;
        run_until(&mut server, &mut clients, |s, c| {
            s.player_count() == 2
                && c.iter().all(|(client, game)| {
                    client.welcomed && block_at(game, IVec3::new(3, 63, 3)).is_some()
                })
                && s.game.chunks.light_at(3, 64, 3).is_some()
        });
        // Noon, a wet farm with wheat, and two cows in love.
        let _ = server.game.run_command(0, "time set 6000", true, true);
        let farm = crate::world::block::BlockState::of(blocks::FARMLAND)
            .with_number(crate::world::props::Prop::Moisture, 7);
        let wheat = crate::world::block::BlockState::of(blocks::WHEAT);
        let spot = IVec3::new(3, 64, 3);
        server
            .game
            .set_blocks(&[(IVec3::new(3, 63, 3), farm), (spot, wheat)]);
        server.game.rules.random_tick_speed = 4096;
        for x in [8.5, 9.5] {
            server
                .game
                .summon("cow", DVec3::new(x, 64.0, 3.5), 0.0)
                .unwrap()
                .data
                .love = 600;
        }
        let calves = |game: &Game| game.mobs.iter().filter(|m| m.data.age < 0).count();
        let ripe = |game: &Game| {
            block_at(game, spot).is_some_and(|b| {
                b.is(blocks::WHEAT) && b.number(crate::world::props::Prop::Age7) == 7
            })
        };
        run_until(&mut server, &mut clients, |s, c| {
            ripe(&s.game)
                && c.iter().all(|(_, game)| ripe(game))
                && calves(&s.game) == 1
                && c.iter().all(|(_, game)| calves(game) == 1)
        });
        assert_eq!(
            calves(&server.game),
            1,
            "one calf, and the parents are done"
        );
    }

    #[test]
    fn two_lan_players_fish_side_by_side_and_both_see_both_bobbers() {
        let (mut server, mut clients) = world_with(&["Alice", "Bob"]);
        run_until(&mut server, &mut clients, |s, c| {
            s.player_count() == 2
                && c.iter().all(|(client, game)| {
                    client.welcomed && block_at(game, IVec3::new(3, 63, 3)).is_some()
                })
        });
        // A pond in front of both, and a Lure III rod each.
        let mut water = Vec::new();
        for x in -2..=8 {
            for z in 4..=12 {
                water.push((
                    IVec3::new(x, 63, z),
                    crate::world::block::BlockState::of(blocks::WATER),
                ));
            }
        }
        server.game.set_blocks(&water);
        for (i, x) in [1.5, 3.5].into_iter().enumerate() {
            server.game.with_player(i, |game| {
                game.fly_to(DVec3::new(x, 64.0, 1.5));
                let mut rod = ItemStack::new(items::FISHING_ROD, 1);
                rod.enchant(crate::inventory::enchant::Enchantment::Lure, 3);
                game.player
                    .inventory
                    .set(game.player.inventory.selected, Some(rod));
            });
            clients[i].1.player.camera.yaw = std::f32::consts::PI;
            clients[i].1.player.camera.pitch = -0.25;
        }
        let tap = |client: &mut Client| {
            for pressed in [true, false] {
                client.send(ClientMsg::Action {
                    action: Action::Use,
                    pressed,
                    repeat: false,
                    grabbed: true,
                });
            }
        };
        tap(&mut clients[0].0);
        tap(&mut clients[1].0);
        // Both bobbers float, and each client sees both.
        run_until(&mut server, &mut clients, |s, c| {
            s.game.bobbers.len() == 2
                && s.game.bobbers.iter().all(|b| b.in_water)
                && c.iter().all(|(_, game)| game.bobbers.len() == 2)
        });
        assert_ne!(server.game.bobbers[0].owner, server.game.bobbers[1].owner);
        // Each waits for its own bite; reel in both when they bite.
        run_until(&mut server, &mut clients, |s, _| {
            s.game.bobbers.iter().all(|b| b.nibble > 0)
        });
        tap(&mut clients[0].0);
        tap(&mut clients[1].0);
        let caught = |game: &Game| {
            (0..crate::inventory::player::SLOTS)
                .filter_map(|i| game.player.inventory.slot(i))
                .any(|s| s.id != items::FISHING_ROD)
        };
        run_until(&mut server, &mut clients, |s, c| {
            s.game.bobbers.is_empty()
                && c.iter().all(|(_, game)| game.bobbers.is_empty())
                && caught(&c[0].1)
                && caught(&c[1].1)
        });
    }

    /// Flies Bob (a spectator, with the wheel at `scale`) east through
    /// water, mobs and terrain for `ticks` ticks, holding `sprint`: the
    /// client's and server's speeds in blocks a second each tick (from how
    /// far each moved), and how often the server corrected the client.
    fn fly_spectator(scale: f64, sprint: bool, ticks: usize) -> (Vec<f64>, Vec<f64>, u32) {
        let (mut server, mut clients) = world_with(&["Alice", "Bob"]);
        server.game.cheats = true;
        run_until(&mut server, &mut clients, |s, c| {
            s.player_count() == 2 && c.iter().all(|(client, _)| client.welcomed)
        });
        let say = |clients: &mut [(Client, Game)], line: &str| {
            clients[0].0.send(ClientMsg::Chat(line.to_owned()));
        };
        say(&mut clients, "/gamemode spectator Bob");
        say(&mut clients, "/fill 8 64 -3 24 70 3 water");
        say(&mut clients, "/fill 30 64 -3 34 66 3 stone");
        say(&mut clients, "/setblock 40 65 0 cobweb");
        say(&mut clients, "/setblock 44 65 0 soul_sand");
        for x in [12, 20, 36] {
            say(&mut clients, &format!("/summon pig {x} 64 0"));
        }
        run_until(&mut server, &mut clients, |s, c| {
            s.game.others[0].mode == GameMode::Spectator
                && c[1].1.player.mode == GameMode::Spectator
        });
        // Bob's start: above the flat world, facing east.
        let start = DVec3::new(0.5, 66.0, 0.5);
        for player in [&mut server.game.others[0], &mut clients[1].1.player] {
            let mut body = player.body_state().clone();
            body.set_position(start);
            body.velocity = DVec3::ZERO;
            player.set_body(body, start);
        }
        let (client, game) = &mut clients[1];
        let mut body = game.player.body_state().clone();
        body.fly_scale = scale;
        game.player.set_body(body, start);
        game.player.camera.yaw = std::f32::consts::FRAC_PI_2;
        game.player.camera.pitch = 0.0;
        game.player.keys.set_key(Action::Forward, true);
        game.player.keys.set_key(Action::Sprint, sprint);
        let before = client.corrections;
        let (mut on_client, mut on_server) = (Vec::new(), Vec::new());
        let (mut last_client, mut last_server) = (start, start);
        for _ in 0..ticks {
            for (client, game) in clients.iter_mut() {
                client.advance(game, 0.05);
            }
            server.advance(0.05);
            for (client, game) in clients.iter_mut() {
                client.receive(game);
            }
            let here = clients[1].1.player.body_state().position();
            on_client.push((here - last_client).length() * 20.0);
            last_client = here;
            let there = server.game.others[0].body_state().position();
            on_server.push((there - last_server).length() * 20.0);
            last_server = there;
        }
        (on_client, on_server, clients[1].0.corrections - before)
    }

    /// (min, max, average) once past the ramp-up (the first five seconds).
    fn summary(speeds: &[f64]) -> (f64, f64, f64) {
        let steady = &speeds[100..];
        let min = steady.iter().copied().fold(f64::MAX, f64::min);
        let max = steady.iter().copied().fold(0.0, f64::max);
        (min, max, steady.iter().sum::<f64>() / steady.len() as f64)
    }

    #[test]
    fn a_spectator_flies_steadily_through_water_terrain_and_mobs_without_corrections() {
        // 30 s at the wheel's 2×, then 30 s of sprinting at 1×.
        let (client, server, corrections) = fly_spectator(2.0, false, 600);
        let (min, max, avg) = summary(&client);
        eprintln!(
            "M46 wheel 2x: min {min:.3} max {max:.3} avg {avg:.3} b/s, {corrections} corrections"
        );
        assert_eq!(summary(&server), (min, max, avg), "the server agrees");
        assert_eq!(corrections, 0, "the server never corrected a spectator");
        assert!(
            max - min < 0.01 && (avg - 2.0 * 10.89).abs() < 0.05,
            "steady at twice flight speed: {min}..{max}"
        );
        let (client, _, corrections) = fly_spectator(1.0, true, 600);
        let (min, max, avg) = summary(&client);
        eprintln!(
            "M46 sprint: min {min:.3} max {max:.3} avg {avg:.3} b/s, {corrections} corrections"
        );
        assert_eq!(corrections, 0);
        assert!(
            max - min < 0.01 && (avg - 2.0 * 10.89).abs() < 0.05,
            "{min}..{max}"
        );
    }

    #[test]
    fn both_clients_hear_a_jukebox_start_and_stop_but_only_those_in_range() {
        let (mut server, mut clients) = world_with(&["Alice", "Bob"]);
        run_until(&mut server, &mut clients, |s, c| {
            s.player_count() == 2 && c.iter().all(|(client, _)| client.welcomed)
        });
        let at = IVec3::new(2, 64, 0);
        run_until(&mut server, &mut clients, |s, c| {
            block_at(&s.game, at).is_some()
                && c.iter().all(|(_, game)| block_at(game, at).is_some())
        });
        server
            .game
            .set_blocks(&[(at, crate::world::block::BlockState::of(blocks::JUKEBOX))]);
        let cat = ItemStack::new(items::by_name("music_disc_cat").unwrap(), 1);
        assert!(server.game.insert_disc(at, cat.clone()).is_none());
        run_until(&mut server, &mut clients, |_, c| {
            c.iter().all(|(client, _)| client.records.len() == 1)
        });
        for (client, game) in &clients {
            assert_eq!(client.records, vec![(at, Some("cat".to_owned()))]);
            // "Now Playing" is on both screens, and a note rises.
            assert_eq!(game.player.title.actionbar, "Now Playing: C418 - cat");
        }
        // Stopping reaches both.
        server.game.take_out_disc(at, true);
        run_until(&mut server, &mut clients, |_, c| {
            c.iter().all(|(client, _)| client.records.len() == 2)
        });
        for (client, _) in &clients {
            assert_eq!(client.records[1], (at, None));
        }
        // Bob walks off beyond earshot: a song starting now is Alice's alone.
        let mut body = server.game.others[0].body_state().clone();
        body.set_position(DVec3::new(300.5, 64.0, 0.5));
        server.game.others[0].set_body(body, DVec3::new(300.5, 64.0, 0.5));
        assert!(server.game.insert_disc(at, cat).is_none());
        run_until(&mut server, &mut clients, |_, c| c[0].0.records.len() == 3);
        for _ in 0..40 {
            for (client, game) in clients.iter_mut() {
                client.advance(game, 0.05);
            }
            server.advance(0.05);
            for (client, game) in clients.iter_mut() {
                client.receive(game);
            }
        }
        assert_eq!(clients[0].0.records[2], (at, Some("cat".to_owned())));
        assert_eq!(clients[1].0.records.len(), 2, "Bob heard nothing");
    }
}
