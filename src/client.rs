//! A client's link to the server: it moves the player ahead of the server
//! (prediction), sends what the player does, and takes in what the server
//! says into the client's copy of the world (`Game::new_client`).

use std::collections::VecDeque;

use log::{info, warn};

use crate::{
    audio,
    game::Game,
    net::{
        protocol::{ClientMsg, Input, PROTOCOL_VERSION, PlayerSettings, ServerMsg},
        transport::Connection,
    },
    player::physics::Body,
};

/// How far the server's body may be from the predicted one before the
/// client takes the server's (in blocks, and blocks a tick).
const PREDICTION_SLACK: f64 = 1e-4;
/// Predicted ticks kept for checking against the server's.
const MAX_HISTORY: usize = 64;

pub type ClientConnection = Box<dyn Connection<ClientMsg, ServerMsg>>;

pub struct Client {
    conn: ClientConnection,
    /// The last input sent, and the inputs since the server's last word
    /// with the body each one predicted.
    seq: u32,
    history: VecDeque<(Input, Body)>,
    /// The world's seed, once welcomed.
    pub seed: Option<u64>,
    pub welcomed: bool,
    /// Why the server refused or sent the player away, once it has.
    pub gone: Option<String>,
}

impl Client {
    /// Logs in as `name` with `settings`.
    pub fn join(mut conn: ClientConnection, name: &str, settings: PlayerSettings) -> Self {
        conn.send(ClientMsg::Login {
            version: PROTOCOL_VERSION,
            name: name.to_owned(),
        });
        conn.send(ClientMsg::Settings(settings));
        Self {
            conn,
            seq: 0,
            history: VecDeque::new(),
            seed: None,
            welcomed: false,
            gone: None,
        }
    }

    pub fn send(&mut self, msg: ClientMsg) {
        if !self.conn.send(msg) && self.gone.is_none() {
            self.gone = Some("Lost connection".to_owned());
        }
    }

    /// Moves the player for the ticks due, sending each tick's input.
    pub fn advance(&mut self, game: &mut Game, dt: f32) {
        #[cfg(debug_assertions)]
        if game.player.is_flying() {
            let feet = game.fly(dt);
            self.send(ClientMsg::Fly { feet });
        }
        for _ in 0..game.clock(dt) {
            self.seq += 1;
            let input = Input {
                seq: self.seq,
                keys: game.player.keys,
                yaw: game.player.camera.yaw,
                pitch: game.player.camera.pitch,
            };
            game.predict(&input);
            self.history
                .push_back((input, game.player.body_state().clone()));
            while self.history.len() > MAX_HISTORY {
                self.history.pop_front();
            }
            self.send(ClientMsg::Input(input));
        }
    }

    /// Takes in everything the server sent, then puts the camera at the
    /// player's eyes.
    pub fn receive(&mut self, game: &mut Game) {
        while let Some(msg) = self.conn.try_recv() {
            match msg {
                ServerMsg::Welcome { seed } => {
                    self.seed = seed;
                    self.welcomed = true;
                }
                ServerMsg::Refused(reason) | ServerMsg::Disconnect(reason) => {
                    warn!("server: {reason}");
                    self.gone = Some(reason);
                }
                ServerMsg::Chunk(chunk) => game.chunks.receive(chunk),
                ServerMsg::Blocks { edits, urgent } => {
                    if urgent {
                        game.chunks.set_blocks(&edits);
                    } else {
                        game.chunks.set_blocks_later(&edits);
                    }
                }
                ServerMsg::Unload(pos) => game.chunks.forget(pos),
                ServerMsg::Dimension(dimension) => {
                    info!("in {dimension:?}");
                    game.enter_dimension(dimension);
                }
                ServerMsg::Snapshot(snapshot) => {
                    let ack = snapshot.ack;
                    let (server, prev) = game.apply_snapshot(*snapshot);
                    self.reconcile(game, ack, server, prev);
                }
                ServerMsg::Sounds(cues) => cues.into_iter().for_each(audio::requeue),
            }
        }
        if !self.conn.is_open() && self.gone.is_none() {
            self.gone = Some("Lost connection".to_owned());
        }
        game.place_camera();
    }

    /// Checks the server's body after input `ack` against the one predicted
    /// then: if they differ (or the client doesn't predict now), the
    /// server's is taken and the inputs since are played again.
    fn reconcile(&mut self, game: &mut Game, ack: u32, server: Body, prev: glam::DVec3) {
        let predicted = self
            .history
            .iter()
            .find(|(input, _)| input.seq == ack)
            .map(|(_, body)| body);
        let agrees = predicted.is_some_and(|body| same_body(body, &server));
        self.history.retain(|(input, _)| input.seq > ack);
        if agrees && game.predicts() {
            return;
        }
        game.player.set_body(server, prev);
        if game.predicts() {
            let keep = (
                game.player.keys,
                game.player.camera.yaw,
                game.player.camera.pitch,
            );
            for (input, body) in &mut self.history {
                game.predict(input);
                *body = game.player.body_state().clone();
            }
            game.player.keys = keep.0;
            game.player.camera.yaw = keep.1;
            game.player.camera.pitch = keep.2;
        }
    }
}

/// Whether two bodies are where the same moves put them.
fn same_body(a: &Body, b: &Body) -> bool {
    a.position().distance(b.position()) < PREDICTION_SLACK
        && a.velocity.distance(b.velocity) < PREDICTION_SLACK
        && a.on_ground == b.on_ground
}
