//! The server: the world and every player in it, ticking 20 times a second
//! and telling each connected client what they see. Single player runs one
//! in the same process with a single client, so there's one way to play.

use std::{collections::VecDeque, sync::Arc};

use glam::DVec3;
use log::{info, warn};

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
        let heard = audio::drain();
        for cue in earlier {
            audio::requeue(cue);
        }
        self.send_chunks();
        self.publish(heard);
    }

    /// Sends each player their snapshot and the sounds they hear.
    fn publish(&mut self, heard: Vec<Cue>) {
        let views: Vec<_> = (0..self.remotes.len())
            .map(|index| {
                let view = self.game.with_player(index, |game| game.player_view());
                (self.game.player_at(index).dimension, view)
            })
            .collect();
        for index in 0..self.remotes.len() {
            let ack = self.remotes[index].ack;
            // What's in the chunks the client has, in their dimension.
            let dimension = self.game.player_at(index).dimension;
            self.game.enter_realm(dimension);
            let range = f64::from((self.remotes[index].view_distance + 1) * 16);
            let here: Vec<_> = views
                .iter()
                .filter(|(d, _)| *d == dimension)
                .map(|(_, view)| view.clone())
                .collect();
            let snapshot = self
                .game
                .with_player(index, |game| game.snapshot(ack, range, here));
            let feet = self.feet(index);
            self.remotes[index]
                .conn
                .send(ServerMsg::Snapshot(Box::new(snapshot)));
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
            let ClientMsg::Login { version, name } = msg else {
                continue;
            };
            let refusal = if version != PROTOCOL_VERSION {
                Some(format!(
                    "Incompatible version: the server runs protocol {PROTOCOL_VERSION}, you {version}"
                ))
            } else if !crate::options::valid_name(&name) {
                Some("Names are 3 to 16 letters, digits or underscores".to_owned())
            } else if self.remotes.len() >= MAX_PLAYERS {
                Some("The world is full".to_owned())
            } else if self.names().any(|taken| taken == name) {
                Some(format!("{name} is already playing"))
            } else {
                None
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
            player.mode = self
                .lan
                .as_ref()
                .map_or(self.game.player.mode, |lan| lan.guest_mode);
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
        let player = self.game.others.remove(index - 1);
        info!("{} left", player.name);
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
        // Leaving takes Bob out of the world.
        clients.pop();
        run_until(&mut server, &mut clients, |server, _| {
            server.player_count() == 1
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
        });
        server.advance(0.0);
        assert!(matches!(client_end.try_recv(), Some(ServerMsg::Refused(_))));
        assert_eq!(server.player_count(), 1);
    }
}
