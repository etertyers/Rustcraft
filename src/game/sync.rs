//! The game for and as a client. A server's game builds a snapshot of the
//! world for each player after every tick; a client's game is a copy of
//! the world made from those snapshots, which only moves its own player,
//! ahead of the server (prediction), until the server's word arrives.

use std::sync::Arc;

use glam::DVec3;

use super::Game;
use crate::{
    entity::mobs::MobRegistry,
    inventory::stack::Items,
    net::protocol::{Entities, Input, Snapshot},
    player::physics::Body,
    world::{
        manager::{ChunkManager, ChunkSource},
        mesh::BlockModels,
    },
};

/// What a game is for.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Role {
    /// The world itself, ticking every player and everything in it.
    #[default]
    Server,
    /// A client's copy of the world, shown on screen.
    Client,
}

impl Game {
    /// A client's empty copy of a world, filled in by the server's chunks
    /// and snapshots. `radius` is the render distance.
    pub fn new_client(
        items: Items,
        models: Arc<BlockModels>,
        mobs: Arc<MobRegistry>,
        radius: i32,
        workers: usize,
    ) -> Game {
        let chunks = ChunkManager::new(ChunkSource::Remote, models, radius, workers);
        let mut game = Game::new(chunks, items, DVec3::ZERO, DVec3::ZERO, 0.0, 0.0).with_mobs(mobs);
        game.role = Role::Client;
        game
    }

    pub fn is_client(&self) -> bool {
        self.role == Role::Client
    }

    /// One server tick (a server drives its own clock).
    pub fn step(&mut self) {
        self.tick();
    }

    /// Runs `f` with player `index` in `self.player` (0 is the host, then
    /// `others` in order).
    pub fn with_player<T>(&mut self, index: usize, f: impl FnOnce(&mut Game) -> T) -> T {
        if index == 0 {
            return f(self);
        }
        std::mem::swap(&mut self.player, &mut self.others[index - 1]);
        let result = f(self);
        std::mem::swap(&mut self.player, &mut self.others[index - 1]);
        result
    }

    /// The world after this tick as `self.player` sees it.
    pub fn snapshot(&self, ack: u32) -> Snapshot {
        Snapshot {
            ack,
            time: self.time,
            difficulty: self.difficulty,
            rules: self.rules.clone(),
            player: self.player.clone(),
            entities: Entities {
                mobs: self.mobs.clone(),
                items: self.entities.clone(),
                orbs: self.orbs.clone(),
                particles: self.particles.clone(),
                effects: self.effects.clone(),
                arrows: self.arrows.clone(),
                fireballs: self.fireballs.clone(),
                bullets: self.bullets.clone(),
                thrown: self.thrown.clone(),
                falling: self.falling.clone(),
                carts: self.carts.clone(),
                boats: self.boats.clone(),
                primed: self.primed.clone(),
                eyes: self.eyes.clone(),
                dragon: self.dragon.clone(),
                crystals: self.crystals.clone(),
                dragon_fireballs: self.dragon_fireballs.clone(),
                breath: self.breath.clone(),
            },
        }
    }

    /// Takes in the server's snapshot: the world and the player's state,
    /// except what the client owns (where it looks and aims, the keys held,
    /// its view and settings) and the predicted body, which stays for the
    /// caller to check against the server's, returned.
    pub fn apply_snapshot(&mut self, snapshot: Snapshot) -> Body {
        let mut player = snapshot.player;
        let local = &self.player;
        let server_body = std::mem::replace(&mut player.body, local.body.clone());
        player.prev_feet = local.prev_feet;
        player.step_lag = local.step_lag;
        player.camera = local.camera;
        player.keys = local.keys;
        player.target = local.target;
        player.view.mode = local.view.mode;
        player.view.bobbing = local.view.bobbing;
        player.sensitivity = local.sensitivity;
        player.invert_y = local.invert_y;
        player.fov_effects = local.fov_effects;
        player.damage_tilt = local.damage_tilt;
        player.scroll_pixels = local.scroll_pixels;
        self.player = player;
        self.time = snapshot.time;
        self.difficulty = snapshot.difficulty;
        self.rules = snapshot.rules;
        let e = snapshot.entities;
        self.mobs = e.mobs;
        self.entities = e.items;
        self.orbs = e.orbs;
        self.particles = e.particles;
        self.effects = e.effects;
        self.arrows = e.arrows;
        self.fireballs = e.fireballs;
        self.bullets = e.bullets;
        self.thrown = e.thrown;
        self.falling = e.falling;
        self.carts = e.carts;
        self.boats = e.boats;
        self.primed = e.primed;
        self.eyes = e.eyes;
        self.dragon = e.dragon;
        self.crystals = e.crystals;
        self.dragon_fireballs = e.dragon_fireballs;
        self.breath = e.breath;
        server_body
    }

    /// Whether the client moves its player itself: walking, swimming,
    /// flying or gliding, but not riding, sleeping, dead or waiting.
    pub fn predicts(&self) -> bool {
        let p = &self.player;
        !p.stats.is_dead()
            && p.sleep.is_none()
            && p.boat.is_none()
            && p.mounted.is_none()
            && p.riding.is_none()
            && p.arriving.is_none()
            && p.credits.is_none()
    }

    /// A client's tick of its own player's movement with `input`.
    pub fn predict(&mut self, input: &Input) {
        let p = &mut self.player;
        p.prev_feet = p.body.position();
        p.keys = input.keys;
        p.camera.yaw = input.yaw;
        p.camera.pitch = input.pitch;
        if self.predicts() {
            self.move_player();
        }
    }

    /// The server's side of `input`: the keys and look the player's turn
    /// plays with.
    pub fn take_input(&mut self, input: &Input) {
        let p = &mut self.player;
        p.keys = input.keys;
        p.camera.yaw = input.yaw;
        p.camera.pitch = input.pitch;
    }
}
