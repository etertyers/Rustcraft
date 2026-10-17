//! The world being played: chunks, the player, their camera and input,
//! advanced on the 20 TPS fixed timestep. `App` owns at most one.

use std::sync::Arc;

use glam::DVec3;
#[cfg(debug_assertions)]
use log::info;
use winit::{
    event::{MouseButton, MouseScrollDelta},
    keyboard::KeyCode,
};

use crate::{
    config,
    player::{
        collision::WorldCollider,
        input::MoveKeys,
        interaction::{self, BREAK_REPEAT_TICKS, HOTBAR, PLACE_REPEAT_TICKS},
        physics::Body,
        raycast::{self, Hit},
    },
    render::camera::Camera,
    world::{block::BlockRegistry, manager::ChunkManager},
};

/// Longest frame step applied to free flight, so a stall doesn't teleport the camera.
#[cfg(debug_assertions)]
const MAX_FRAME_DT: f32 = 0.1;

/// Seconds per game tick.
const TICK: f32 = 1.0 / config::TICKS_PER_SECOND as f32;

/// Most game time simulated in one frame; after a longer stall the game
/// slows down instead of running a burst of ticks.
const MAX_CATCH_UP: f32 = 10.0 * TICK;

/// Trackpad scrolling this many pixels moves one hotbar slot.
const SCROLL_PIXELS_PER_SLOT: f64 = 30.0;

/// Falling below this returns the player to spawn (until health exists in M9).
const VOID_Y: f64 = -64.0;

pub struct Game {
    pub chunks: ChunkManager,
    blocks: Arc<BlockRegistry>,
    /// Look direction comes from the mouse; position follows `body` each frame.
    pub camera: Camera,
    body: Body,
    /// Feet position at the previous tick, for interpolating the camera.
    prev_feet: DVec3,
    spawn: DVec3,
    keys: MoveKeys,
    /// The block under the crosshair this frame.
    pub target: Option<Hit>,
    /// Mouse buttons held for breaking and placing.
    breaking: bool,
    placing: bool,
    /// Presses not yet handled by a tick, so a click shorter than a tick still counts.
    break_clicked: bool,
    place_clicked: bool,
    /// Ticks until a held button acts again.
    break_cooldown: u8,
    place_cooldown: u8,
    /// Selected `HOTBAR` slot.
    pub slot: usize,
    /// Trackpad scroll not yet turned into slot changes.
    scroll_pixels: f64,
    /// Game time not yet simulated, in seconds (less than one tick after a frame).
    tick_time: f32,
    /// Free flight, a dev tool toggled with F4 in debug builds.
    #[cfg(debug_assertions)]
    flying: bool,
}

impl Game {
    /// Starts with the player's feet at `feet`, looking along `yaw` and `pitch`.
    pub fn new(
        chunks: ChunkManager,
        blocks: Arc<BlockRegistry>,
        feet: DVec3,
        spawn: DVec3,
        yaw: f32,
        pitch: f32,
    ) -> Self {
        let body = Body::new(feet);
        let eye = feet + DVec3::Y * body.eye_height();
        Self {
            chunks,
            blocks,
            camera: Camera::new(eye.as_vec3(), yaw, pitch),
            body,
            prev_feet: feet,
            spawn,
            keys: MoveKeys::default(),
            target: None,
            breaking: false,
            placing: false,
            break_clicked: false,
            place_clicked: false,
            break_cooldown: 0,
            place_cooldown: 0,
            slot: 0,
            scroll_pixels: 0.0,
            tick_time: 0.0,
            #[cfg(debug_assertions)]
            flying: false,
        }
    }

    pub fn blocks(&self) -> &BlockRegistry {
        &self.blocks
    }

    /// Number keys, the F4 dev flight toggle and, while the cursor is
    /// grabbed, movement keys.
    pub fn on_key(&mut self, key: KeyCode, pressed: bool, repeat: bool, grabbed: bool) {
        if key == KeyCode::F4 && pressed && !repeat {
            #[cfg(debug_assertions)]
            {
                self.flying = !self.flying;
                self.body.velocity = DVec3::ZERO;
                info!("flying {}", if self.flying { "on" } else { "off" });
            }
        } else if let Some(slot) = hotbar_slot(key)
            && pressed
        {
            self.slot = slot;
        } else if grabbed {
            self.keys.set_key(key, pressed);
        }
    }

    /// Forgets held keys and buttons, e.g. when the cursor is released.
    pub fn release_input(&mut self) {
        self.keys.clear();
        self.breaking = false;
        self.placing = false;
        self.break_clicked = false;
        self.place_clicked = false;
    }

    pub fn look(&mut self, dx: f64, dy: f64) {
        let s = config::MOUSE_SENSITIVITY;
        self.camera.rotate(dx as f32 * s, -dy as f32 * s);
    }

    /// A held button acts on the next tick, then every few ticks.
    pub fn on_mouse_button(&mut self, button: MouseButton, pressed: bool) {
        match button {
            MouseButton::Left => {
                self.breaking = pressed;
                self.break_clicked |= pressed;
                self.break_cooldown = 0;
            }
            MouseButton::Right => {
                self.placing = pressed;
                self.place_clicked |= pressed;
                self.place_cooldown = 0;
            }
            _ => {}
        }
    }

    /// Scrolling down moves right along the hotbar, like Minecraft.
    pub fn on_scroll(&mut self, delta: MouseScrollDelta) {
        let steps = match delta {
            MouseScrollDelta::LineDelta(_, y) => -y.signum() as i32,
            MouseScrollDelta::PixelDelta(pos) => {
                self.scroll_pixels -= pos.y;
                let steps = (self.scroll_pixels / SCROLL_PIXELS_PER_SLOT).trunc();
                self.scroll_pixels -= steps * SCROLL_PIXELS_PER_SLOT;
                steps as i32
            }
        };
        self.slot = (self.slot as i32 + steps).rem_euclid(HOTBAR.len() as i32) as usize;
    }

    /// Advances the game by `dt` seconds of real time: runs any ticks due,
    /// then places the camera between the last two ticks.
    pub fn advance(&mut self, dt: f32) {
        #[cfg(debug_assertions)]
        if self.flying {
            let mut feet = self.body.position();
            crate::player::fly::fly(&mut feet, self.camera.yaw, &self.keys, dt.min(MAX_FRAME_DT));
            self.body.set_position(feet);
            self.prev_feet = feet;
        }
        self.tick_time = (self.tick_time + dt).min(MAX_CATCH_UP);
        while self.tick_time >= TICK {
            self.tick();
            self.tick_time -= TICK;
        }
        let alpha = (self.tick_time / TICK) as f64;
        let feet = self.prev_feet.lerp(self.body.position(), alpha);
        let eye = feet + DVec3::Y * self.body.eye_height();
        self.camera.position = eye.as_vec3();
        self.target = self.find_target(eye);
    }

    /// One game tick: player physics, then breaking and placing.
    fn tick(&mut self) {
        self.prev_feet = self.body.position();
        self.move_player();
        self.use_hands();
    }

    fn move_player(&mut self) {
        #[cfg(debug_assertions)]
        if self.flying {
            return;
        }
        let collider = WorldCollider {
            world: self.chunks.world(),
            blocks: &self.blocks,
        };
        // Wait for the chunks around the player to load before moving them.
        if !collider.is_loaded(&self.body.aabb()) {
            return;
        }
        self.body
            .tick(&self.keys.move_input(), self.camera.yaw, &collider);
        if self.body.position().y < VOID_Y {
            self.body = Body::new(self.spawn);
            self.prev_feet = self.spawn;
        }
    }

    fn use_hands(&mut self) {
        self.break_cooldown = self.break_cooldown.saturating_sub(1);
        self.place_cooldown = self.place_cooldown.saturating_sub(1);
        let wants_break = (self.breaking || self.break_clicked) && self.break_cooldown == 0;
        let wants_place = (self.placing || self.place_clicked) && self.place_cooldown == 0;
        self.break_clicked = false;
        self.place_clicked = false;
        if !wants_break && !wants_place {
            return;
        }
        let eye = self.body.position() + DVec3::Y * self.body.eye_height();
        let Some(hit) = self.find_target(eye) else {
            return;
        };
        if wants_break {
            let edits = interaction::break_block(self.chunks.world(), &self.blocks, hit.block);
            if !edits.is_empty() {
                self.chunks.set_blocks(&edits);
                self.break_cooldown = BREAK_REPEAT_TICKS;
            }
        } else if let Some(edit) = interaction::place_block(
            self.chunks.world(),
            &self.blocks,
            &hit,
            HOTBAR[self.slot],
            &self.body.aabb(),
        ) {
            self.chunks.set_blocks(&[edit]);
            self.place_cooldown = PLACE_REPEAT_TICKS;
        }
    }

    /// The block the crosshair points at from `eye`, within reach.
    fn find_target(&self, eye: DVec3) -> Option<Hit> {
        let world = self.chunks.world();
        let dir = self.camera.forward().as_dvec3();
        raycast::raycast(eye, dir, config::REACH, |cell| {
            world
                .block(cell.x, cell.y, cell.z)
                .and_then(|block| self.blocks.get(block.id()))
                .is_some_and(|def| def.is_targetable())
        })
    }
}

/// Number keys 1–9 pick a hotbar slot.
fn hotbar_slot(key: KeyCode) -> Option<usize> {
    const KEYS: [KeyCode; 9] = [
        KeyCode::Digit1,
        KeyCode::Digit2,
        KeyCode::Digit3,
        KeyCode::Digit4,
        KeyCode::Digit5,
        KeyCode::Digit6,
        KeyCode::Digit7,
        KeyCode::Digit8,
        KeyCode::Digit9,
    ];
    KEYS.iter().position(|&k| k == key)
}
