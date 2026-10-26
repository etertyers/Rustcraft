//! Open to LAN: the host listens for players on a TCP port and announces
//! the world on the local network every 1.5 s, as vanilla does (UDP
//! multicast to 224.0.2.60:4445, `[MOTD]<player> - <world>[/MOTD][AD]<port>[/AD]`);
//! the Multiplayer screen listens for those announcements.

use std::{
    io,
    net::{Ipv4Addr, SocketAddr, SocketAddrV4, TcpListener, TcpStream, UdpSocket},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use crossbeam_channel::{Receiver, Sender};
use log::{info, warn};

/// Vanilla's LAN announcement group and port.
pub const GROUP: Ipv4Addr = Ipv4Addr::new(224, 0, 2, 60);
pub const ANNOUNCE_PORT: u16 = 4445;
/// How often the host announces itself.
const ANNOUNCE_EVERY: Duration = Duration::from_millis(1500);
/// A world not heard from for this long leaves the list.
const FORGET_AFTER: Duration = Duration::from_secs(5);
/// How often the listening threads check whether to stop.
const POLL: Duration = Duration::from_millis(50);

/// A world open to LAN: the players arriving on its port, and the threads
/// listening and announcing, which stop when this is dropped.
pub struct Listener {
    pub port: u16,
    incoming: Receiver<TcpStream>,
    stop: Arc<AtomicBool>,
}

impl Listener {
    /// Listens on `port` (0 picks a free one) and announces `motd`.
    pub fn open(port: u16, motd: String) -> io::Result<Self> {
        let listener = TcpListener::bind(SocketAddr::from((Ipv4Addr::UNSPECIFIED, port)))?;
        listener.set_nonblocking(true)?;
        let port = listener.local_addr()?.port();
        let stop = Arc::new(AtomicBool::new(false));
        let (tx, incoming) = crossbeam_channel::unbounded();
        let accept_stop = stop.clone();
        std::thread::Builder::new()
            .name("lan-accept".to_owned())
            .spawn(move || accept(listener, tx, accept_stop))?;
        let announce_stop = stop.clone();
        let message = announcement(&motd, port);
        std::thread::Builder::new()
            .name("lan-announce".to_owned())
            .spawn(move || announce(message, announce_stop))?;
        info!("open to LAN on port {port}");
        Ok(Self {
            port,
            incoming,
            stop,
        })
    }

    /// The players who connected since the last call.
    pub fn arrivals(&self) -> Vec<TcpStream> {
        self.incoming.try_iter().collect()
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

fn accept(listener: TcpListener, tx: Sender<TcpStream>, stop: Arc<AtomicBool>) {
    while !stop.load(Ordering::Relaxed) {
        match listener.accept() {
            Ok((stream, from)) => {
                info!("connection from {from}");
                if stream.set_nonblocking(false).is_err() || tx.send(stream).is_err() {
                    break;
                }
            }
            Err(err) if err.kind() == io::ErrorKind::WouldBlock => std::thread::sleep(POLL),
            Err(err) => {
                warn!("LAN listener: {err}");
                std::thread::sleep(POLL);
            }
        }
    }
}

fn announce(message: String, stop: Arc<AtomicBool>) {
    let socket = match UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)) {
        Ok(socket) => socket,
        Err(err) => return warn!("can't announce the world: {err}"),
    };
    let _ = socket.set_multicast_ttl_v4(1);
    let target = SocketAddrV4::new(GROUP, ANNOUNCE_PORT);
    let mut next = Instant::now();
    while !stop.load(Ordering::Relaxed) {
        if Instant::now() >= next {
            if let Err(err) = socket.send_to(message.as_bytes(), target) {
                warn!("announcing the world: {err}");
            }
            next += ANNOUNCE_EVERY;
        }
        std::thread::sleep(POLL);
    }
}

/// Vanilla's announcement for a world.
pub fn announcement(motd: &str, port: u16) -> String {
    format!("[MOTD]{motd}[/MOTD][AD]{port}[/AD]")
}

/// Reads an announcement: the world's description and its port.
pub fn read_announcement(text: &str) -> Option<(String, u16)> {
    let between = |open: &str, close: &str| {
        let start = text.find(open)? + open.len();
        let end = start + text[start..].find(close)?;
        Some(&text[start..end])
    };
    let motd = between("[MOTD]", "[/MOTD]")?.to_owned();
    let port = between("[AD]", "[/AD]")?.trim().parse().ok()?;
    Some((motd, port))
}

/// A world found on the network.
#[derive(Debug, Clone, PartialEq)]
pub struct Found {
    pub motd: String,
    pub address: SocketAddr,
    seen: Instant,
}

/// Listens for worlds announced on the local network (the Multiplayer
/// screen), until dropped.
pub struct Finder {
    found: Receiver<(String, SocketAddr)>,
    stop: Arc<AtomicBool>,
    worlds: Vec<Found>,
}

impl Finder {
    pub fn start() -> io::Result<Self> {
        let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, ANNOUNCE_PORT))?;
        socket.join_multicast_v4(&GROUP, &Ipv4Addr::UNSPECIFIED)?;
        socket.set_read_timeout(Some(POLL))?;
        let stop = Arc::new(AtomicBool::new(false));
        let (tx, found) = crossbeam_channel::unbounded();
        let listen_stop = stop.clone();
        std::thread::Builder::new()
            .name("lan-finder".to_owned())
            .spawn(move || {
                let mut buf = [0u8; 1024];
                while !listen_stop.load(Ordering::Relaxed) {
                    let Ok((len, from)) = socket.recv_from(&mut buf) else {
                        continue;
                    };
                    let text = String::from_utf8_lossy(&buf[..len]);
                    if let Some((motd, port)) = read_announcement(&text)
                        && tx.send((motd, SocketAddr::new(from.ip(), port))).is_err()
                    {
                        break;
                    }
                }
            })?;
        Ok(Self {
            found,
            stop,
            worlds: Vec::new(),
        })
    }

    /// The worlds heard from lately, in the order first heard.
    pub fn worlds(&mut self) -> &[Found] {
        let now = Instant::now();
        for (motd, address) in self.found.try_iter() {
            match self.worlds.iter_mut().find(|w| w.address == address) {
                Some(world) => {
                    world.motd = motd;
                    world.seen = now;
                }
                None => self.worlds.push(Found {
                    motd,
                    address,
                    seen: now,
                }),
            }
        }
        self.worlds
            .retain(|world| now.duration_since(world.seen) < FORGET_AFTER);
        &self.worlds
    }
}

impl Drop for Finder {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn announcements_read_back_as_vanilla_writes_them() {
        let text = announcement("Kenzo - New World", 51234);
        assert_eq!(text, "[MOTD]Kenzo - New World[/MOTD][AD]51234[/AD]");
        assert_eq!(
            read_announcement(&text),
            Some(("Kenzo - New World".to_owned(), 51234))
        );
        assert_eq!(read_announcement("[MOTD]x[/MOTD]"), None);
        assert_eq!(read_announcement("[MOTD]x[/MOTD][AD]port[/AD]"), None);
    }

    #[test]
    fn a_world_open_to_lan_takes_connections() {
        let listener = Listener::open(0, "Test - World".to_owned()).unwrap();
        assert_ne!(listener.port, 0);
        let _client = TcpStream::connect(("127.0.0.1", listener.port)).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut arrived = Vec::new();
        while arrived.is_empty() {
            assert!(Instant::now() < deadline, "nobody arrived");
            arrived = listener.arrivals();
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}
