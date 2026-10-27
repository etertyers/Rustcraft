//! How messages travel between a server and a client: a connection sends
//! one kind of message and receives the other. Single player uses
//! in-process channels (no copying or encoding); LAN uses TCP, each
//! message a length-prefixed frame of lz4-compressed bincode, read and
//! written on threads of their own so the game never waits on the network.

use std::{
    io::{BufReader, BufWriter, Read, Write},
    net::TcpStream,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};

use crossbeam_channel::{Receiver, Sender, TryRecvError};
use log::debug;
use serde::{Serialize, de::DeserializeOwned};

/// One end of a connection: sends `S`, receives `R`.
pub trait Connection<S, R>: Send {
    /// Queues `msg` for the other end. Returns false once the connection
    /// is closed (the message is dropped).
    fn send(&mut self, msg: S) -> bool;
    /// The next message that has arrived, if any.
    fn try_recv(&mut self) -> Option<R>;
    /// Whether the other end is still there.
    fn is_open(&self) -> bool;
}

/// An in-process connection end over channels.
pub struct Local<S, R> {
    tx: Sender<S>,
    rx: Receiver<R>,
    open: bool,
}

/// Two connected ends: what one sends, the other receives.
pub fn local_pair<A, B>() -> (Local<A, B>, Local<B, A>) {
    let (a_tx, a_rx) = crossbeam_channel::unbounded();
    let (b_tx, b_rx) = crossbeam_channel::unbounded();
    (
        Local {
            tx: a_tx,
            rx: b_rx,
            open: true,
        },
        Local {
            tx: b_tx,
            rx: a_rx,
            open: true,
        },
    )
}

impl<S: Send, R: Send> Connection<S, R> for Local<S, R> {
    fn send(&mut self, msg: S) -> bool {
        self.open &= self.tx.send(msg).is_ok();
        self.open
    }

    fn try_recv(&mut self) -> Option<R> {
        match self.rx.try_recv() {
            Ok(msg) => Some(msg),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                self.open = false;
                None
            }
        }
    }

    fn is_open(&self) -> bool {
        self.open
    }
}

/// The largest frame accepted (a frame claiming more ends the connection).
const MAX_FRAME: usize = 64 << 20;

/// Bytes a TCP connection has sent and received, for `--lan-test`.
#[derive(Debug, Default)]
pub struct Traffic {
    pub sent: AtomicU64,
    pub received: AtomicU64,
}

/// A TCP connection end.
pub struct Tcp<S, R> {
    tx: Sender<S>,
    rx: Receiver<R>,
    open: Arc<AtomicBool>,
    pub traffic: Arc<Traffic>,
    stream: TcpStream,
}

impl<S, R> Tcp<S, R>
where
    S: Serialize + Send + 'static,
    R: DeserializeOwned + Send + 'static,
{
    /// Starts the reading and writing threads on `stream`.
    pub fn new(stream: TcpStream) -> std::io::Result<Self> {
        stream.set_nodelay(true)?;
        let open = Arc::new(AtomicBool::new(true));
        let traffic = Arc::new(Traffic::default());
        let (out_tx, out_rx) = crossbeam_channel::unbounded::<S>();
        let (in_tx, in_rx) = crossbeam_channel::unbounded::<R>();
        let writer = stream.try_clone()?;
        let (open_w, traffic_w) = (open.clone(), traffic.clone());
        std::thread::Builder::new()
            .name("net-writer".to_owned())
            .spawn(move || {
                let mut writer = BufWriter::new(writer);
                while let Ok(msg) = out_rx.recv() {
                    let frame = encode(&msg);
                    let ok = writer
                        .write_all(&(frame.len() as u32).to_le_bytes())
                        .is_ok()
                        && writer.write_all(&frame).is_ok();
                    // What's queued goes out together, flushed once it's all
                    // written.
                    if !ok || (out_rx.is_empty() && writer.flush().is_err()) {
                        break;
                    }
                    traffic_w
                        .sent
                        .fetch_add(frame.len() as u64 + 4, Ordering::Relaxed);
                }
                let _ = writer.flush();
                open_w.store(false, Ordering::Relaxed);
            })?;
        let reader = stream.try_clone()?;
        let (open_r, traffic_r) = (open.clone(), traffic.clone());
        std::thread::Builder::new()
            .name("net-reader".to_owned())
            .spawn(move || {
                let mut reader = BufReader::new(reader);
                loop {
                    let mut len = [0; 4];
                    if reader.read_exact(&mut len).is_err() {
                        break;
                    }
                    let len = u32::from_le_bytes(len) as usize;
                    if len > MAX_FRAME {
                        debug!("frame of {len} bytes refused");
                        break;
                    }
                    let mut frame = vec![0; len];
                    if reader.read_exact(&mut frame).is_err() {
                        break;
                    }
                    traffic_r
                        .received
                        .fetch_add(len as u64 + 4, Ordering::Relaxed);
                    match decode(&frame) {
                        Some(msg) => {
                            if in_tx.send(msg).is_err() {
                                break;
                            }
                        }
                        None => {
                            debug!("unreadable frame of {len} bytes");
                            break;
                        }
                    }
                }
                open_r.store(false, Ordering::Relaxed);
            })?;
        Ok(Self {
            tx: out_tx,
            rx: in_rx,
            open,
            traffic,
            stream,
        })
    }
}

fn encode<T: Serialize>(msg: &T) -> Vec<u8> {
    let bytes = bincode::serde::encode_to_vec(msg, bincode::config::standard())
        .expect("messages always encode");
    lz4_flex::compress_prepend_size(&bytes)
}

fn decode<T: DeserializeOwned>(frame: &[u8]) -> Option<T> {
    let bytes = lz4_flex::decompress_size_prepended(frame).ok()?;
    let (msg, _) = bincode::serde::decode_from_slice(&bytes, bincode::config::standard()).ok()?;
    Some(msg)
}

impl<S: Send, R: Send> Connection<S, R> for Tcp<S, R> {
    fn send(&mut self, msg: S) -> bool {
        self.is_open() && self.tx.send(msg).is_ok()
    }

    fn try_recv(&mut self) -> Option<R> {
        self.rx.try_recv().ok()
    }

    fn is_open(&self) -> bool {
        // Open until the threads stop and everything read has been taken.
        self.open.load(Ordering::Relaxed) || !self.rx.is_empty()
    }
}

impl<S, R> Drop for Tcp<S, R> {
    fn drop(&mut self) {
        let _ = self.stream.shutdown(std::net::Shutdown::Both);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_local_pair_carries_messages_both_ways_in_order_and_notices_a_hang_up() {
        let (mut server, mut client) = local_pair::<String, u32>();
        assert!(server.send("hello".to_owned()));
        assert!(server.send("again".to_owned()));
        assert!(client.send(7));
        assert_eq!(client.try_recv().as_deref(), Some("hello"));
        assert_eq!(client.try_recv().as_deref(), Some("again"));
        assert_eq!(client.try_recv(), None);
        assert_eq!(server.try_recv(), Some(7));
        drop(client);
        assert_eq!(server.try_recv(), None);
        assert!(!server.is_open());
        assert!(!server.send("anyone?".to_owned()));
    }

    #[test]
    fn tcp_carries_messages_and_chunks_both_ways() {
        use crate::net::protocol::{ClientMsg, PROTOCOL_VERSION, ServerMsg};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let mut client: Tcp<ClientMsg, ServerMsg> =
            Tcp::new(TcpStream::connect(addr).unwrap()).unwrap();
        let mut server: Tcp<ServerMsg, ClientMsg> = Tcp::new(listener.accept().unwrap().0).unwrap();
        client.send(ClientMsg::Login {
            version: PROTOCOL_VERSION,
            name: "Alice".to_owned(),
            mods: Vec::new(),
        });
        let mut chunk = crate::world::chunk::Chunk::new(crate::world::chunk::ChunkPos::new(3, -2));
        chunk.set(
            1,
            64,
            2,
            crate::world::block::BlockState::of(crate::world::blocks::STONE),
        );
        server.send(ServerMsg::Chunk(Arc::new(chunk)));
        let wait = |f: &mut dyn FnMut() -> bool| {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            while !f() {
                assert!(std::time::Instant::now() < deadline, "nothing arrived");
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
        };
        let mut login = None;
        wait(&mut || {
            login = server.try_recv();
            login.is_some()
        });
        assert!(matches!(login, Some(ClientMsg::Login { name, .. }) if name == "Alice"));
        let mut got = None;
        wait(&mut || {
            got = client.try_recv();
            got.is_some()
        });
        let Some(ServerMsg::Chunk(chunk)) = got else {
            panic!("not a chunk");
        };
        assert_eq!(chunk.pos, crate::world::chunk::ChunkPos::new(3, -2));
        assert!(chunk.get(1, 64, 2).is(crate::world::blocks::STONE));
        assert!(client.traffic.received.load(Ordering::Relaxed) > 0);
        drop(server);
        wait(&mut || !client.is_open());
    }
}
