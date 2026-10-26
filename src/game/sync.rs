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
        // So the first snapshot's look is taken.
        game.player.turned = u32::MAX;
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
            return self.hearing(f);
        }
        std::mem::swap(&mut self.player, &mut self.others[index - 1]);
        let result = self.hearing(f);
        std::mem::swap(&mut self.player, &mut self.others[index - 1]);
        result
    }

    /// Keeps each player's own sounds apart for them (a server's game).
    pub fn keep_sounds(&mut self) {
        self.keeps_sounds = true;
    }

    /// The world after this tick as `self.player` sees it: mobs, items and
    /// orbs within `range` blocks (across), and everything else.
    pub fn snapshot(
        &self,
        ack: u32,
        range: f64,
        players: Vec<crate::net::protocol::PlayerView>,
    ) -> Snapshot {
        let feet = self.player.body.position();
        let near = |at: DVec3| (at.x - feet.x).abs().max((at.z - feet.z).abs()) <= range;
        Snapshot {
            ack,
            time: self.time,
            difficulty: self.difficulty,
            rules: self.rules.clone(),
            player: self.player.clone(),
            players: players
                .into_iter()
                .filter(|view| view.name != self.player.name && near(view.feet))
                .collect(),
            entities: Entities {
                mobs: self
                    .mobs
                    .iter()
                    .filter(|m| near(m.feet()))
                    .cloned()
                    .collect(),
                items: self
                    .entities
                    .iter()
                    .filter(|i| near(i.center()))
                    .cloned()
                    .collect(),
                orbs: self
                    .orbs
                    .iter()
                    .filter(|o| near(o.center()))
                    .cloned()
                    .collect(),
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
    /// caller to check against the server's, returned with where the
    /// server had the player a tick before.
    pub fn apply_snapshot(&mut self, snapshot: Snapshot) -> (Body, DVec3) {
        let mut player = snapshot.player;
        let local = &self.player;
        let server_body = std::mem::replace(&mut player.body, local.body.clone());
        let server_prev = std::mem::replace(&mut player.prev_feet, local.prev_feet);
        player.step_lag = local.step_lag;
        // The client's look, unless the world turned the player.
        let (yaw, pitch) = (player.camera.yaw, player.camera.pitch);
        player.camera = local.camera;
        if player.turned != local.turned {
            (player.camera.yaw, player.camera.pitch) = (yaw, pitch);
        }
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
        self.remote_players = snapshot.players;
        (server_body, server_prev)
    }

    /// A client's player went to `dimension`: its chunks go, and the
    /// renderer is told to drop what it drew.
    pub fn enter_dimension(&mut self, dimension: crate::world::dimension::Dimension) {
        if dimension != self.dimension {
            self.dimension_changed = true;
        }
        self.dimension = dimension;
        self.chunks.unload_all();
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
            && !p.is_flying()
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

    /// How many players there are, the host included.
    pub fn player_count(&self) -> usize {
        1 + self.others.len()
    }

    /// Player `index` (0 is the host, then `others`).
    pub fn player_at(&self, index: usize) -> &super::PlayerState {
        match index {
            0 => &self.player,
            i => &self.others[i - 1],
        }
    }

    /// `self.player` as the other players see them.
    pub fn player_view(&self) -> crate::net::protocol::PlayerView {
        let p = &self.player;
        crate::net::protocol::PlayerView {
            name: p.name.clone(),
            prev_feet: p.prev_feet,
            feet: p.body.position(),
            yaw: p.view.body_yaw(1.0),
            pose: self.player_pose(),
            wearer: self.wearer(),
            invisible: p
                .stats
                .effects
                .has(crate::player::effects::Effect::Invisibility),
        }
    }

    /// Where player `index` stands (0 is the host, then `others`).
    pub fn feet_of(&self, index: usize) -> DVec3 {
        match index {
            0 => self.player.body.position(),
            i => self.others[i - 1].body.position(),
        }
    }

    /// The saved world's name, if it's saved.
    pub fn world_name(&self) -> Option<String> {
        self.saved.as_ref().map(|saved| saved.level.name.clone())
    }

    /// Where new players start.
    pub fn spawn_point(&self) -> DVec3 {
        self.spawn
    }

    /// Puts the player at `feet` (debug flight, which a client moves
    /// between ticks).
    pub fn fly_to(&mut self, feet: DVec3) {
        self.player.body.set_position(feet);
        self.player.prev_feet = feet;
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

impl Game {
    /// Does what `self.player` did on a screen: the slot clicks, drags and
    /// throws of every inventory screen, creative picks, the recipe book,
    /// enchanting, naming, trading and cutting, and the death and bed
    /// screens' buttons. `book` is every recipe in the book's order.
    pub fn apply_screen_action(
        &mut self,
        action: crate::ui::screens::ScreenAction,
        book: &[crate::inventory::book::BookRecipe],
    ) {
        use crate::{
            inventory::{creative, stack::ItemStack},
            ui::{recipe_book::BookAction, screens::ScreenAction},
        };
        let items = self.items().clone();
        let p = &mut self.player;
        match action {
            ScreenAction::Click(slot, click) => self.click_slot(slot, click),
            ScreenAction::Drag { slots, click } => self.drag_across(&slots, click),
            ScreenAction::Collect(_) => self.collect_onto_cursor(),
            ScreenAction::Throw { all } => {
                let Some(held) = p.cursor.as_mut() else {
                    return;
                };
                let count = if all { held.count } else { 1 };
                held.count -= count;
                let thrown = ItemStack {
                    count,
                    ..held.clone()
                };
                if held.count == 0 {
                    p.cursor = None;
                }
                self.throw(thrown);
            }
            ScreenAction::CreativePick { stack, pick } => {
                creative::pick(&mut p.cursor, &mut p.inventory, &stack, pick, &items);
            }
            ScreenAction::CreativeDestroy { shift } => {
                creative::destroy(&mut p.cursor, &mut p.inventory, shift);
            }
            ScreenAction::Book(BookAction::Open(open)) => p.recipe_book.open = open,
            ScreenAction::Book(BookAction::Filter(only)) => p.recipe_book.craftable_only = only,
            ScreenAction::Book(BookAction::Category(category)) => {
                p.recipe_book.category = category;
            }
            ScreenAction::Book(BookAction::Place { index, max }) => {
                if let Some(recipe) = book.get(index) {
                    self.place_recipe(recipe, max);
                }
            }
            ScreenAction::Enchant(button) => {
                self.enchant_item(button);
            }
            ScreenAction::AnvilName(name) => self.set_anvil_name(name),
            ScreenAction::Trade(index) => {
                self.trade(index);
            }
            ScreenAction::Choose(index) => self.choose_cut(index),
            ScreenAction::Respawn => self.respawn(),
            ScreenAction::LeaveBed => self.wake(),
            // The client's own: the book's ghost recipe, the advancements
            // tab, the pause menu.
            ScreenAction::Book(BookAction::Ghost(_))
            | ScreenAction::AdvTab(_)
            | ScreenAction::Pause(_) => {}
        }
    }
}

impl Game {
    /// The settings a client plays its player by.
    pub fn take_settings(&mut self, settings: &crate::net::protocol::PlayerSettings) {
        let p = &mut self.player;
        p.auto_jump = settings.auto_jump;
        p.toggle_sneak = settings.toggle_sneak;
        p.toggle_sprint = settings.toggle_sprint;
        p.left_handed = settings.left_handed;
        p.skin_layers = settings.skin_layers;
        p.particle_level = settings.particles;
    }
}

impl super::PlayerState {
    /// The player's body as physics moves it.
    pub fn body_state(&self) -> &Body {
        &self.body
    }

    /// Takes the server's body for the player's, and where it was the
    /// tick before (for drawing between the two).
    pub fn set_body(&mut self, body: Body, prev_feet: DVec3) {
        self.body = body;
        self.prev_feet = prev_feet;
    }

    /// Flying with the debug flight key (F4), which moves the player
    /// between ticks.
    pub fn is_flying(&self) -> bool {
        #[cfg(debug_assertions)]
        {
            self.flying
        }
        #[cfg(not(debug_assertions))]
        {
            false
        }
    }
}

impl Game {
    /// Debug flight's move for `dt` seconds of the held keys; returns
    /// where the player is now.
    #[cfg(debug_assertions)]
    pub fn fly(&mut self, dt: f32) -> DVec3 {
        let mut feet = self.player.body.position();
        crate::player::fly::fly(
            &mut feet,
            self.player.camera.yaw,
            &self.player.keys,
            dt.min(super::MAX_FRAME_DT),
        );
        self.fly_to(feet);
        feet
    }
}
