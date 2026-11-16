//! The browser's connection (M72): the same messages as TCP, each one
//! bincode + lz4 in a byte frame, but handed over as `postMessage` payloads
//! between the page and the server's Web Worker, so nothing on the main
//! thread ever waits. A [`Port`] is the thing that moves frames:
//!
//! - [`CopyPort`]: channels that copy each frame (what `postMessage` does with
//!   a transferred `ArrayBuffer`); native and the tests use it, and so does
//!   the protocol test with a server and two clients.
//! - `web::MessagePortPort` (wasm only): a `MessagePort` or a `Worker`.

use std::marker::PhantomData;

use crossbeam_channel::{Receiver, Sender, TryRecvError};
use serde::{Serialize, de::DeserializeOwned};

use super::transport::{Connection, decode, encode};

/// Moves byte frames to the other end.
pub trait Port: Send {
    /// Hands a frame over. False once the other end is gone.
    fn post(&mut self, frame: Vec<u8>) -> bool;
    /// The next frame that has arrived, if any (never waits).
    fn poll(&mut self) -> Option<Vec<u8>>;
    fn is_open(&self) -> bool;
}

/// One end of a connection over a [`Port`]: sends `S`, receives `R`.
pub struct WorkerTransport<S, R, P: Port> {
    port: P,
    open: bool,
    _messages: PhantomData<fn(S) -> R>,
}

impl<S, R, P: Port> WorkerTransport<S, R, P> {
    pub fn new(port: P) -> Self {
        Self {
            port,
            open: true,
            _messages: PhantomData,
        }
    }
}

impl<S, R, P> Connection<S, R> for WorkerTransport<S, R, P>
where
    S: Serialize + Send,
    R: DeserializeOwned + Send,
    P: Port,
{
    fn send(&mut self, msg: S) -> bool {
        self.open &= self.port.post(encode(&msg));
        self.open
    }

    fn try_recv(&mut self) -> Option<R> {
        let frame = self.port.poll()?;
        match decode(&frame) {
            Some(msg) => Some(msg),
            None => {
                log::debug!("unreadable frame of {} bytes", frame.len());
                self.open = false;
                None
            }
        }
    }

    fn is_open(&self) -> bool {
        self.open && self.port.is_open()
    }
}

/// Frames copied through channels.
pub struct CopyPort {
    tx: Sender<Vec<u8>>,
    rx: Receiver<Vec<u8>>,
    open: bool,
}

/// Two connected ports.
pub fn copy_pair() -> (CopyPort, CopyPort) {
    let (a_tx, a_rx) = crossbeam_channel::unbounded();
    let (b_tx, b_rx) = crossbeam_channel::unbounded();
    (
        CopyPort {
            tx: a_tx,
            rx: b_rx,
            open: true,
        },
        CopyPort {
            tx: b_tx,
            rx: a_rx,
            open: true,
        },
    )
}

impl Port for CopyPort {
    fn post(&mut self, frame: Vec<u8>) -> bool {
        self.open &= self.tx.send(frame).is_ok();
        self.open
    }

    fn poll(&mut self) -> Option<Vec<u8>> {
        match self.rx.try_recv() {
            Ok(frame) => Some(frame),
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

/// Two connected ends of a worker-shaped connection: what one sends, the
/// other receives, each message serialized, copied and deserialized.
pub fn worker_pair<A, B>() -> (
    WorkerTransport<A, B, CopyPort>,
    WorkerTransport<B, A, CopyPort>,
) {
    let (a, b) = copy_pair();
    (WorkerTransport::new(a), WorkerTransport::new(b))
}
