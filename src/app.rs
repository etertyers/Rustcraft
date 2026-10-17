//! winit event loop: owns the window, renderer and UI, and the game being played.

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
    event::{DeviceEvent, DeviceId, ElementState, KeyEvent, MouseButton, StartCause, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow},
    keyboard::{KeyCode, PhysicalKey},
    window::{CursorGrabMode, Window, WindowId},
};

use crate::{
    assets, config,
    game::Game,
    inventory::item::ItemRegistry,
    player::interaction::HOTBAR,
    render::{
        Renderer,
        atlas::{Atlas, MISSING_TILE},
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

/// Wait before retrying after the surface skipped a frame, instead of spinning.
const SKIPPED_FRAME_RETRY: Duration = Duration::from_millis(50);

pub struct App {
    window: Option<Arc<Window>>,
    renderer: Option<Renderer>,
    debug_ui: Option<DebugUi>,
    world_kind: WorldKind,
    blocks: Option<Arc<BlockRegistry>>,
    models: Option<Arc<BlockModels>>,
    game: Option<Game>,
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
        Self {
            window: None,
            renderer: None,
            debug_ui: None,
            world_kind,
            blocks: None,
            models: None,
            game: None,
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
        self.blocks = Some(Arc::new(blocks));
        self.models = Some(Arc::new(models));
        self.game = Some(self.start_game(self.world_kind));
        self.renderer = Some(renderer);
        self.debug_ui = Some(DebugUi::new(&window));
        window.request_redraw();
        self.window = Some(window);
        Ok(())
    }

    /// A new game in a test world (not saved).
    fn start_game(&self, kind: WorldKind) -> Game {
        let (blocks, models) = (
            self.blocks.clone().expect("loaded in init"),
            self.models.clone().expect("loaded in init"),
        );
        let ground = config::SEA_LEVEL as f64 + 1.0;
        let (source, spawn, yaw, pitch) = match kind {
            WorldKind::Generated { seed } => {
                info!("world seed {seed}");
                let terrain = Terrain::new(seed);
                let (x, y, z) = terrain.spawn_point();
                let spawn = DVec3::new(x as f64 + 0.5, y as f64, z as f64 + 0.5);
                (ChunkSource::Terrain(Box::new(terrain)), spawn, 0.0, 0.0)
            }
            // On the grass at the origin.
            WorldKind::Flat => (ChunkSource::Flat, DVec3::new(0.5, ground, 0.5), 0.0, 0.0),
            // In front of the first gallery row, looking south along it.
            WorldKind::Gallery => (
                ChunkSource::Fixed(
                    gallery::gallery_chunks(&blocks)
                        .into_iter()
                        .map(|chunk| (chunk.pos, chunk))
                        .collect(),
                ),
                DVec3::new(22.5, ground, -4.0),
                std::f32::consts::PI,
                -0.3,
            ),
        };
        let chunks = ChunkManager::new(source, models, config::RENDER_DISTANCE, chunk_workers());
        Game::new(chunks, blocks, spawn, spawn, yaw, pitch)
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
        if !grab && let Some(game) = self.game.as_mut() {
            game.release_input();
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
        } else if let Some(game) = self.game.as_mut() {
            game.on_key(key, pressed, event.repeat, self.cursor_grabbed);
        }
    }

    fn redraw(&mut self, event_loop: &ActiveEventLoop) {
        let now = Instant::now();
        let dt = (now - self.last_frame).as_secs_f32();
        self.last_frame = now;
        let (Some(window), Some(renderer), Some(debug_ui), Some(game)) = (
            &self.window,
            self.renderer.as_mut(),
            self.debug_ui.as_mut(),
            self.game.as_mut(),
        ) else {
            return;
        };
        game.advance(dt);

        let p = game.camera.position;
        let (x, z) = (p.x.floor() as i32, p.z.floor() as i32);
        let changes = game.chunks.update(ChunkPos::of_block(x, z));
        for (pos, mesh) in &changes.meshes {
            renderer.upload_chunk(*pos, mesh);
        }
        for pos in changes.unloaded {
            renderer.remove_chunk(pos);
        }
        renderer.set_outline(game.target.map(|hit| hit.block));

        let chunks = &game.chunks;
        let stats = DebugStats {
            chunks_drawn: renderer.chunks_drawn(),
            chunks_loaded: chunks.world().len(),
            biome: chunks.world().biome_at(x, z),
            seed: chunks.seed(),
            light: chunks.light_at(x, p.y.floor() as i32, z),
        };
        let selected = HOTBAR[game.slot];
        let hud = Hud {
            crosshair: self.cursor_grabbed,
            selected: format!(
                "{}: {}",
                game.slot + 1,
                game.blocks().display_name(selected)
            ),
        };
        let ui = debug_ui.frame(window, &game.camera, &stats, &hud);
        match renderer.render(&game.camera, ui) {
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
                if let Some(game) = self.game.as_mut() {
                    game.on_mouse_button(button, state == ElementState::Pressed);
                }
            }
            WindowEvent::MouseWheel { delta, .. } if self.cursor_grabbed => {
                if let Some(game) = self.game.as_mut() {
                    game.on_scroll(delta);
                }
            }
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
            && let Some(game) = self.game.as_mut()
        {
            game.look(dx, dy);
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
        self.game = None;
        self.window = None;
    }
}

/// One worker per core, leaving one core for the main thread.
fn chunk_workers() -> usize {
    let workers = std::thread::available_parallelism()
        .map_or(2, |n| n.get().saturating_sub(1))
        .max(1);
    info!("{workers} chunk worker threads");
    workers
}

/// Pins the cursor in place. macOS supports only Locked; other platforms may
/// only support Confined.
fn lock_cursor(window: &Window) -> Result<(), ExternalError> {
    window
        .set_cursor_grab(CursorGrabMode::Locked)
        .or_else(|_| window.set_cursor_grab(CursorGrabMode::Confined))
}
