//! winit event loop: owns the window, renderer and input state.

use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use anyhow::Result;
use glam::DVec3;
use log::{error, info, warn};
use winit::{
    application::ApplicationHandler,
    dpi::{LogicalSize, PhysicalPosition},
    error::ExternalError,
    event::{
        DeviceEvent, DeviceId, ElementState, KeyEvent, MouseButton, MouseScrollDelta, StartCause,
        WindowEvent,
    },
    event_loop::{ActiveEventLoop, ControlFlow},
    keyboard::{KeyCode, PhysicalKey},
    window::{CursorGrabMode, Window, WindowId},
};

use crate::{
    assets, config,
    inventory::item::ItemRegistry,
    player::{
        collision::WorldCollider,
        input::MoveKeys,
        interaction::{self, BREAK_REPEAT_TICKS, HOTBAR, PLACE_REPEAT_TICKS},
        physics::Body,
        raycast::{self, Hit},
    },
    render::{
        Renderer,
        atlas::{Atlas, MISSING_TILE},
        camera::Camera,
    },
    ui::{DebugStats, DebugUi, Hud},
    world::{
        biome::BiomeColors,
        block::BlockRegistry,
        chunk::ChunkPos,
        manager::{ChunkManager, ChunkSource},
        mesh::BlockModels,
        worldgen::{gallery, terrain::Terrain},
    },
};

/// Which test world to build.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorldKind {
    /// Generated terrain from a seed (the default).
    Generated { seed: u64 },
    /// Endless flat grass plain (`--flat`).
    Flat,
    /// Every block and variant on display (`--gallery`).
    Gallery,
}

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

/// Wait before retrying after the surface skipped a frame, instead of spinning.
const SKIPPED_FRAME_RETRY: Duration = Duration::from_millis(50);

pub struct App {
    window: Option<Arc<Window>>,
    renderer: Option<Renderer>,
    debug_ui: Option<DebugUi>,
    world_kind: WorldKind,
    chunks: Option<ChunkManager>,
    blocks: Option<Arc<BlockRegistry>>,
    /// Look direction comes from the mouse; position follows `body` each frame.
    camera: Camera,
    body: Body,
    /// Feet position at the previous tick, for interpolating the camera.
    prev_feet: DVec3,
    spawn: DVec3,
    keys: MoveKeys,
    /// The block under the crosshair this frame.
    target: Option<Hit>,
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
    slot: usize,
    /// Trackpad scroll not yet turned into slot changes.
    scroll_pixels: f64,
    /// Game time not yet simulated, in seconds (less than one tick after a frame).
    tick_time: f32,
    /// Free flight, a dev tool toggled with F4 in debug builds.
    #[cfg(debug_assertions)]
    flying: bool,
    cursor_grabbed: bool,
    last_frame: Instant,
    /// The window is fully hidden; don't draw until it's visible again.
    occluded: bool,
    /// The last frame was skipped by the surface; retry after a short wait.
    frame_skipped: bool,
    /// First fatal error; the event loop exits and `main` returns it.
    error: Option<anyhow::Error>,
}

impl App {
    pub fn new(world_kind: WorldKind) -> Self {
        let ground = config::SEA_LEVEL as f64 + 1.0;
        let (spawn, yaw, pitch) = match world_kind {
            WorldKind::Generated { seed } => {
                let (x, y, z) = Terrain::new(seed).spawn_point();
                (
                    DVec3::new(x as f64 + 0.5, y as f64, z as f64 + 0.5),
                    0.0,
                    0.0,
                )
            }
            // On the grass at the origin.
            WorldKind::Flat => (DVec3::new(0.5, ground, 0.5), 0.0, 0.0),
            // In front of the first gallery row, looking south along it.
            WorldKind::Gallery => (DVec3::new(22.5, ground, -4.0), std::f32::consts::PI, -0.3),
        };
        Self {
            window: None,
            renderer: None,
            debug_ui: None,
            world_kind,
            chunks: None,
            blocks: None,
            camera: Camera::new(spawn.as_vec3(), yaw, pitch),
            body: Body::new(spawn),
            prev_feet: spawn,
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
            cursor_grabbed: false,
            last_frame: Instant::now(),
            occluded: false,
            frame_skipped: false,
            error: None,
        }
    }

    pub fn into_result(self) -> Result<()> {
        self.error.map_or(Ok(()), Err)
    }

    fn fail(&mut self, event_loop: &ActiveEventLoop, err: anyhow::Error) {
        error!("{err:#}");
        self.error.get_or_insert(err);
        event_loop.exit();
    }

    fn init(&mut self, event_loop: &ActiveEventLoop) -> Result<()> {
        let attrs = Window::default_attributes()
            .with_title(config::WINDOW_TITLE)
            .with_inner_size(LogicalSize::new(
                config::WINDOW_WIDTH,
                config::WINDOW_HEIGHT,
            ));
        let window = Arc::new(event_loop.create_window(attrs)?);
        let assets = assets::dir();
        let blocks = BlockRegistry::load(&assets.join("data/blocks.ron"))?;
        let items = ItemRegistry::load(&assets.join("data/items.ron"))?;
        let atlas = Atlas::load(&assets.join("textures/blocks"))?;
        let item_dir = assets.join("textures/items");
        let missing_blocks = blocks.missing_textures(|name| atlas.get(name).is_some());
        let missing_items =
            items.missing_textures(|name| item_dir.join(format!("{name}.png")).is_file());
        info!(
            "loaded {} blocks and {} items",
            blocks.iter().count(),
            items.len()
        );
        if !missing_blocks.is_empty() || !missing_items.is_empty() {
            warn!("missing block textures: {missing_blocks:?}, item textures: {missing_items:?}");
        }

        let biome_colors =
            BiomeColors::load(&assets.join("textures/colormap")).unwrap_or_else(|err| {
                warn!("{err:#}; using default biome colors");
                BiomeColors::uniform()
            });
        let renderer = Renderer::new(window.clone(), &atlas)?;
        let models = BlockModels::new(
            &blocks,
            |name| atlas.get(name).map(|tile| atlas.uv_rect(tile)),
            atlas.uv_rect(MISSING_TILE),
            biome_colors,
        );

        let source = match self.world_kind {
            WorldKind::Generated { seed } => {
                info!("world seed {seed}");
                ChunkSource::Terrain(Box::new(Terrain::new(seed)))
            }
            WorldKind::Flat => ChunkSource::Flat,
            WorldKind::Gallery => ChunkSource::Fixed(
                gallery::gallery_chunks(&blocks)
                    .into_iter()
                    .map(|chunk| (chunk.pos, chunk))
                    .collect(),
            ),
        };
        // Leave one core for the main thread.
        let workers = std::thread::available_parallelism()
            .map_or(2, |n| n.get().saturating_sub(1))
            .max(1);
        info!("{workers} chunk worker threads");
        self.chunks = Some(ChunkManager::new(
            source,
            Arc::new(models),
            config::RENDER_DISTANCE,
            workers,
        ));
        self.blocks = Some(Arc::new(blocks));
        self.renderer = Some(renderer);
        self.debug_ui = Some(DebugUi::new(&window));
        window.request_redraw();
        self.window = Some(window);
        Ok(())
    }

    /// macOS sometimes stops pinning a locked cursor, and the hidden cursor
    /// then wanders out of the window. If it drifts away from the middle,
    /// put it back.
    fn recenter_cursor(&self, position: PhysicalPosition<f64>) {
        let Some(window) = &self.window else {
            return;
        };
        let size = window.inner_size();
        let center = PhysicalPosition::new(size.width as f64 / 2.0, size.height as f64 / 2.0);
        if (position.x - center.x).abs() < center.x / 2.0
            && (position.y - center.y).abs() < center.y / 2.0
        {
            return;
        }
        // Moving the cursor unlocks it on macOS, so lock it again.
        if let Err(err) = window
            .set_cursor_position(center)
            .and_then(|()| lock_cursor(window))
        {
            warn!("recentering the cursor failed: {err}");
        }
    }

    /// Locks and hides the cursor for mouse look, or releases it.
    fn set_cursor_grab(&mut self, grab: bool) {
        let Some(window) = &self.window else {
            return;
        };
        let result = if grab {
            lock_cursor(window)
        } else {
            window.set_cursor_grab(CursorGrabMode::None)
        };
        if let Err(err) = result {
            warn!("cursor grab failed: {err}");
            return;
        }
        window.set_cursor_visible(!grab);
        self.cursor_grabbed = grab;
        if !grab {
            self.keys.clear();
            self.breaking = false;
            self.placing = false;
            self.break_clicked = false;
            self.place_clicked = false;
        }
    }

    fn on_key(&mut self, event: &KeyEvent) {
        let PhysicalKey::Code(key) = event.physical_key else {
            return;
        };
        let pressed = event.state == ElementState::Pressed;
        if key == KeyCode::Escape && pressed {
            self.set_cursor_grab(false);
        } else if key == KeyCode::F3 && pressed && !event.repeat {
            if let Some(debug_ui) = self.debug_ui.as_mut() {
                debug_ui.toggle();
            }
        } else if key == KeyCode::F4 && pressed && !event.repeat {
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
        } else if self.cursor_grabbed {
            self.keys.set_key(key, pressed);
        }
    }

    /// The block the crosshair points at from `eye`, within reach.
    fn find_target(&self, eye: DVec3) -> Option<Hit> {
        let (chunks, blocks) = (self.chunks.as_ref()?, self.blocks.as_ref()?);
        let world = chunks.world();
        let dir = self.camera.forward().as_dvec3();
        raycast::raycast(eye, dir, config::REACH, |cell| {
            world
                .block(cell.x, cell.y, cell.z)
                .and_then(|block| blocks.get(block.id()))
                .is_some_and(|def| def.is_targetable())
        })
    }

    /// A held button acts on the next tick, then every few ticks.
    fn on_mouse_button(&mut self, button: MouseButton, pressed: bool) {
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
    fn on_scroll(&mut self, delta: MouseScrollDelta) {
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
        let (Some(chunks), Some(blocks)) = (&self.chunks, &self.blocks) else {
            return;
        };
        let collider = WorldCollider {
            world: chunks.world(),
            blocks,
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
        let (Some(chunks), Some(blocks)) = (self.chunks.as_mut(), &self.blocks) else {
            return;
        };
        if wants_break {
            let edits = interaction::break_block(chunks.world(), blocks, hit.block);
            if !edits.is_empty() {
                chunks.set_blocks(&edits);
                self.break_cooldown = BREAK_REPEAT_TICKS;
            }
        } else if let Some(edit) = interaction::place_block(
            chunks.world(),
            blocks,
            &hit,
            HOTBAR[self.slot],
            &self.body.aabb(),
        ) {
            chunks.set_blocks(&[edit]);
            self.place_cooldown = PLACE_REPEAT_TICKS;
        }
    }

    fn redraw(&mut self, event_loop: &ActiveEventLoop) {
        let now = Instant::now();
        let dt = (now - self.last_frame).as_secs_f32();
        self.last_frame = now;
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

        let (Some(window), Some(renderer), Some(debug_ui), Some(chunks)) = (
            &self.window,
            self.renderer.as_mut(),
            self.debug_ui.as_mut(),
            self.chunks.as_mut(),
        ) else {
            return;
        };
        let p = self.camera.position;
        let (x, z) = (p.x.floor() as i32, p.z.floor() as i32);
        let changes = chunks.update(ChunkPos::of_block(x, z));
        for (pos, mesh) in &changes.meshes {
            renderer.upload_chunk(*pos, mesh);
        }
        for pos in changes.unloaded {
            renderer.remove_chunk(pos);
        }
        renderer.set_outline(self.target.map(|hit| hit.block));

        let stats = DebugStats {
            chunks_drawn: renderer.chunks_drawn(),
            chunks_loaded: chunks.world().len(),
            biome: chunks.world().biome_at(x, z),
            seed: chunks.seed(),
            light: chunks.light_at(x, p.y.floor() as i32, z),
        };
        let selected = HOTBAR[self.slot];
        let hud = Hud {
            crosshair: self.cursor_grabbed,
            selected: self.blocks.as_ref().map_or_else(String::new, |blocks| {
                format!("{}: {}", self.slot + 1, blocks.display_name(selected))
            }),
        };
        let ui = debug_ui.frame(window, &self.camera, &stats, &hud);
        match renderer.render(&self.camera, ui) {
            Ok(presented) => {
                self.frame_skipped = !presented;
                if presented {
                    debug_ui.frame_presented(now);
                }
            }
            Err(err) => self.fail(event_loop, err),
        }
    }
}

impl ApplicationHandler for App {
    fn new_events(&mut self, _event_loop: &ActiveEventLoop, cause: StartCause) {
        if let StartCause::ResumeTimeReached { .. } = cause
            && let Some(window) = &self.window
        {
            window.request_redraw();
        }
    }
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        if let Err(err) = self.init(event_loop) {
            self.fail(event_loop, err);
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        if let (Some(window), Some(debug_ui)) = (&self.window, self.debug_ui.as_mut()) {
            debug_ui.on_window_event(window, &event);
        }
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if let Some(renderer) = self.renderer.as_mut() {
                    renderer.resize(size);
                }
            }
            WindowEvent::Focused(false) => self.set_cursor_grab(false),
            WindowEvent::Occluded(occluded) => {
                self.occluded = occluded;
                if !occluded && let Some(window) = &self.window {
                    window.request_redraw();
                }
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } if !self.cursor_grabbed => self.set_cursor_grab(true),
            WindowEvent::MouseInput { state, button, .. } if self.cursor_grabbed => {
                self.on_mouse_button(button, state == ElementState::Pressed);
            }
            WindowEvent::MouseWheel { delta, .. } if self.cursor_grabbed => self.on_scroll(delta),
            WindowEvent::CursorMoved { position, .. } if self.cursor_grabbed => {
                self.recenter_cursor(position);
            }
            WindowEvent::KeyboardInput { event, .. } => self.on_key(&event),
            WindowEvent::RedrawRequested => self.redraw(event_loop),
            _ => {}
        }
    }

    fn device_event(&mut self, _event_loop: &ActiveEventLoop, _id: DeviceId, event: DeviceEvent) {
        if let DeviceEvent::MouseMotion { delta: (dx, dy) } = event
            && self.cursor_grabbed
        {
            let s = config::MOUSE_SENSITIVITY;
            self.camera.rotate(dx as f32 * s, -dy as f32 * s);
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if self.occluded {
            event_loop.set_control_flow(ControlFlow::Wait);
        } else if self.frame_skipped {
            // new_events requests the redraw when the wait ends.
            event_loop
                .set_control_flow(ControlFlow::WaitUntil(Instant::now() + SKIPPED_FRAME_RETRY));
        } else if let Some(window) = &self.window {
            event_loop.set_control_flow(ControlFlow::Wait);
            window.request_redraw();
        }
    }

    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        // Drop the GPU surface before the window it points to.
        self.debug_ui = None;
        self.renderer = None;
        // Joins the chunk workers.
        self.chunks = None;
        self.window = None;
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

/// Pins the cursor in place. macOS supports only Locked; other platforms may
/// only support Confined.
fn lock_cursor(window: &Window) -> Result<(), ExternalError> {
    window
        .set_cursor_grab(CursorGrabMode::Locked)
        .or_else(|_| window.set_cursor_grab(CursorGrabMode::Confined))
}
