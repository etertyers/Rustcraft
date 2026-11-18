//! The server in a Web Worker (M76), as the window sees it. The browser build
//! can run a world's server in its own worker, away from the page's thread;
//! native never does (the server runs in-process). `Remote` is the page's
//! handle on that worker.

use serde::{Deserialize, Serialize};

use crate::player::mode::GameMode;

/// What a server worker plays: sent when it starts.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorldSpec {
    pub name: String,
    pub seed: Option<u64>,
    pub mode: GameMode,
    pub cheats: bool,
}

impl WorldSpec {
    pub fn to_bytes(&self) -> Vec<u8> {
        bincode::serde::encode_to_vec(self, bincode::config::standard()).unwrap_or_default()
    }

    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        bincode::serde::decode_from_slice(bytes, bincode::config::standard())
            .ok()
            .map(|(spec, _)| spec)
    }
}

#[cfg(target_arch = "wasm32")]
pub use crate::web::server_worker::{Remote, requested, spawn};

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use super::WorldSpec;
    use crate::client::ClientConnection;

    /// There is no server worker natively.
    pub struct Remote;

    impl Remote {
        pub fn cheats(&self) -> bool {
            false
        }
        pub fn stopping(&self) -> bool {
            false
        }
        pub fn save(&self) {}
        pub fn close(&mut self) {}
    }

    pub fn requested() -> bool {
        false
    }

    pub fn spawn(_spec: &WorldSpec) -> anyhow::Result<(ClientConnection, Remote)> {
        anyhow::bail!("the server worker only exists in the browser")
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub use native::{Remote, requested, spawn};
