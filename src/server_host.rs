//! Where the world's server runs (M76), as the window sees it. `App` asks
//! this for the few things it needs (tick, LAN, the cheats flag, `/stop`,
//! save, shut down) and never reaches into the `Server` for gameplay, so the
//! server can move into a Web Worker on the web without `App` changing again.
//! Today the one host is [`ServerHost::new`]: the server in this process,
//! on this thread. The test flights (`--bench`, `--shot`) still need the
//! world itself, which only a local host has ([`ServerHost::local_mut`]).

use crate::{player::mode::GameMode, server::Server};

/// A running server and what `App` can do with it.
pub struct ServerHost {
    server: Server,
}

impl ServerHost {
    /// A server running in this process.
    pub fn new(server: Server) -> Self {
        Self { server }
    }

    /// Runs the server for `dt` seconds (whole ticks at 20 per second).
    pub fn advance(&mut self, dt: f32) {
        self.server.advance(dt);
    }

    /// Joins a client: the server's end of a connection.
    pub fn connect(&mut self, conn: crate::server::ServerConnection) {
        self.server.connect(conn);
    }

    /// Opens the world to LAN; returns the port.
    pub fn open_to_lan(
        &mut self,
        port: u16,
        guest_mode: GameMode,
        cheats: bool,
    ) -> std::io::Result<u16> {
        self.server.open_to_lan(port, guest_mode, cheats)
    }

    /// The port the world is open to LAN on, if it is.
    pub fn lan_port(&self) -> Option<u16> {
        self.server.lan_port()
    }

    /// Whether the world was made with Allow Cheats.
    pub fn cheats(&self) -> bool {
        self.server.game.cheats
    }

    /// The host player's game mode.
    pub fn mode(&self) -> GameMode {
        self.server.game.player.mode
    }

    /// `/stop` was run: the window should save and go back to the title.
    pub fn stopping(&self) -> bool {
        self.server.stopping
    }

    /// Shows the host player a line of text.
    pub fn tell_host(&mut self, text: String) {
        self.server.game.player.show(text);
    }

    /// Saves the world, everyone in it included.
    pub fn save(&mut self) -> anyhow::Result<()> {
        self.server.game.save()
    }

    /// Tells guests the world is closing and saves it.
    pub fn close(&mut self) -> anyhow::Result<()> {
        self.server.shut_down();
        self.save()
    }

    /// The server itself, for the test flights and scenes (`--bench`,
    /// `--shot`), which drive the world directly. A remote host has none.
    pub fn local(&self) -> Option<&Server> {
        Some(&self.server)
    }

    pub fn local_mut(&mut self) -> Option<&mut Server> {
        Some(&mut self.server)
    }
}
