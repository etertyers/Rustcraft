//! Where the world's server runs (M76), as the window sees it. `App` asks
//! this for the few things it needs (tick, LAN, the cheats flag, `/stop`,
//! save, shut down) and never reaches into the `Server` for gameplay, so the
//! server can move into a Web Worker on the web without `App` changing again.
//! Today the one host is [`ServerHost::new`]: the server in this process,
//! on this thread. The test flights (`--bench`, `--shot`) still need the
//! world itself, which only a local host has ([`ServerHost::local_mut`]).

use crate::{platform::server_worker::Remote, player::mode::GameMode, server::Server};

/// A running server and what `App` can do with it.
pub struct ServerHost {
    host: Host,
}

enum Host {
    /// The server in this process.
    Local(Box<Server>),
    /// The server in a Web Worker (the browser build, `?server-worker`): it
    /// ticks itself; the window only reads what it reports.
    Remote(Remote),
}

impl ServerHost {
    /// A server running in this process.
    pub fn new(server: Server) -> Self {
        Self {
            host: Host::Local(Box::new(server)),
        }
    }

    /// A server running in a Web Worker.
    pub fn remote(remote: Remote) -> Self {
        Self {
            host: Host::Remote(remote),
        }
    }

    /// Runs the server for `dt` seconds (whole ticks at 20 per second). A
    /// worker's server runs itself.
    pub fn advance(&mut self, dt: f32) {
        if let Host::Local(server) = &mut self.host {
            server.advance(dt);
        }
    }

    /// Opens the world to LAN; returns the port.
    pub fn open_to_lan(
        &mut self,
        port: u16,
        guest_mode: GameMode,
        cheats: bool,
    ) -> std::io::Result<u16> {
        match &mut self.host {
            Host::Local(server) => server.open_to_lan(port, guest_mode, cheats),
            Host::Remote(_) => Err(std::io::Error::other("a worker's world can't open to LAN")),
        }
    }

    /// The port the world is open to LAN on, if it is.
    pub fn lan_port(&self) -> Option<u16> {
        match &self.host {
            Host::Local(server) => server.lan_port(),
            Host::Remote(_) => None,
        }
    }

    /// Whether the world was made with Allow Cheats.
    pub fn cheats(&self) -> bool {
        match &self.host {
            Host::Local(server) => server.game.cheats,
            Host::Remote(remote) => remote.cheats(),
        }
    }

    /// The host player's game mode.
    pub fn mode(&self) -> GameMode {
        match &self.host {
            Host::Local(server) => server.game.player.mode,
            Host::Remote(_) => GameMode::default(),
        }
    }

    /// `/stop` was run: the window should save and go back to the title.
    pub fn stopping(&self) -> bool {
        match &self.host {
            Host::Local(server) => server.stopping,
            Host::Remote(remote) => remote.stopping(),
        }
    }

    /// Shows the host player a line of text.
    pub fn tell_host(&mut self, text: String) {
        if let Host::Local(server) = &mut self.host {
            server.game.player.show(text);
        }
    }

    /// Saves the world, everyone in it included.
    pub fn save(&mut self) -> anyhow::Result<()> {
        match &mut self.host {
            Host::Local(server) => server.game.save(),
            Host::Remote(remote) => {
                remote.save();
                Ok(())
            }
        }
    }

    /// Tells guests the world is closing and saves it.
    pub fn close(&mut self) -> anyhow::Result<()> {
        match &mut self.host {
            Host::Local(server) => {
                server.shut_down();
                server.game.save()
            }
            Host::Remote(remote) => {
                remote.close();
                Ok(())
            }
        }
    }

    /// The server itself, for the test flights and scenes (`--bench`,
    /// `--shot`), which drive the world directly. A worker's server has none.
    pub fn local(&self) -> Option<&Server> {
        match &self.host {
            Host::Local(server) => Some(server),
            Host::Remote(_) => None,
        }
    }

    pub fn local_mut(&mut self) -> Option<&mut Server> {
        match &mut self.host {
            Host::Local(server) => Some(server),
            Host::Remote(_) => None,
        }
    }
}
