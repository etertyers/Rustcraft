//! How messages travel between a server and a client: a connection sends
//! one kind of message and receives the other. Single player uses
//! in-process channels (no copying or encoding); LAN adds TCP in M37.

use crossbeam_channel::{Receiver, Sender, TryRecvError};

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
}
