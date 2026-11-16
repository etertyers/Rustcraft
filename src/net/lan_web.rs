//! Open to LAN in the browser build (M70): not available. Same names as
//! `lan.rs` so the server and the Multiplayer screen compile unchanged; the
//! buttons that reach them are hidden (`platform::LAN`) and every call
//! fails with a clear message.

use std::{
    io,
    net::{SocketAddr, TcpStream},
};

fn unavailable<T>() -> io::Result<T> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "LAN play isn't available in the browser version",
    ))
}

pub struct Listener {
    pub port: u16,
}

impl Listener {
    pub fn open(_port: u16, _motd: String) -> io::Result<Self> {
        unavailable()
    }

    pub fn arrivals(&self) -> Vec<TcpStream> {
        Vec::new()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Found {
    pub motd: String,
    pub address: SocketAddr,
}

pub struct Finder;

impl Finder {
    pub fn start() -> io::Result<Self> {
        unavailable()
    }

    pub fn worlds(&mut self) -> &[Found] {
        &[]
    }
}
