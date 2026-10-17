//! winit event loop: owns the window, renderer and input state.

use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use anyhow::Result;
use glam::Vec3;
use log::{error, info, warn};
use winit::{
    application::ApplicationHandler,
    dpi::LogicalSize,
    event::{DeviceEvent, DeviceId, ElementState, KeyEvent, MouseButton, StartCause, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow},
    keyboard::{KeyCode, PhysicalKey},
    window::{CursorGrabMode, Window, WindowId},
};

use crate::{
    assets, config,
    inventory::item::ItemRegistry,
    player::fly::{self, FlyInput},
    render::{
        Renderer,
        atlas::{Atlas, MISSING_TILE},
        camera::Camera,
    },
    ui::{DebugStats, DebugUi},
    world::{
        World,
        biome::BiomeColors,
        block::BlockRegistry,
        chunk::ChunkPos,
        mesh::{self, BlockModels},
        worldgen::{flat, gallery, terrain::Terrain},
    },
};

/// Which test world to build.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorldKind {
    /// Generated terrain from a seed (the default).
    Generated { seed: u64 },
    /// Flat grass plain, `FLAT_RADIUS` chunks in each direction.
    Flat,
    /// Every block and variant on display (`--gallery`).
    Gallery,
}

/// Chunks generated in each direction around the origin for the flat world,
/// and around spawn for generated terrain.
const FLAT_RADIUS: i32 = 4;

/// Longest frame step applied to movement, so a stall doesn't teleport the camera.
const MAX_FRAME_DT: f32 = 0.1;

/// Wait before retrying after the surface skipped a frame, instead of spinning.
const SKIPPED_FRAME_RETRY: Duration = Duration::from_millis(50);

pub struct App {
    window: Option<Arc<Window>>,
    renderer: Option<Renderer>,
    debug_ui: Option<DebugUi>,
    world_kind: WorldKind,
    world: World,
    terrain: Option<Terrain>,
    camera: Camera,
    fly_input: FlyInput,
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
        let mut terrain = None;
        let camera = match world_kind {
            WorldKind::Generated { seed } => {
                let generator = Terrain::new(seed);
                let (x, y, z) = generator.spawn_point();
                terrain = Some(generator);
                Camera::new(
                    Vec3::new(x as f32 + 0.5, y as f32 + 1.62, z as f32 + 0.5),
                    0.0,
                    0.0,
                )
            }
            // Standing height above the grass at the origin.
            WorldKind::Flat => Camera::new(
                Vec3::new(0.5, config::SEA_LEVEL as f32 + 2.62, 0.5),
                0.0,
                0.0,
            ),
            // In front of the first gallery row, looking south over it and down.
            WorldKind::Gallery => Camera::new(
                Vec3::new(22.5, config::SEA_LEVEL as f32 + 12.0, -10.0),
                std::f32::consts::PI,
                -0.6,
            ),
        };
        Self {
            window: None,
            renderer: None,
            debug_ui: None,
            world_kind,
            world: World::default(),
            terrain,
            camera,
            fly_input: FlyInput::default(),
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
        let mut renderer = Renderer::new(window.clone(), &atlas)?;
        let models = BlockModels::new(
            &blocks,
            |name| atlas.get(name).map(|tile| atlas.uv_rect(tile)),
            atlas.uv_rect(MISSING_TILE),
            biome_colors,
        );

        let started = Instant::now();
        match self.world_kind {
            WorldKind::Generated { seed } => {
                info!("world seed {seed}");
                let terrain = self
                    .terrain
                    .as_ref()
                    .expect("terrain is set for generated worlds");
                let center = ChunkPos::of_block(
                    self.camera.position.x as i32,
                    self.camera.position.z as i32,
                );
                for z in -FLAT_RADIUS..=FLAT_RADIUS {
                    for x in -FLAT_RADIUS..=FLAT_RADIUS {
                        self.world.insert(terrain.generate(center.offset(x, z)));
                    }
                }
            }
            WorldKind::Flat => {
                for z in -FLAT_RADIUS..=FLAT_RADIUS {
                    for x in -FLAT_RADIUS..=FLAT_RADIUS {
                        self.world.insert(flat::flat_chunk(ChunkPos::new(x, z)));
                    }
                }
            }
            WorldKind::Gallery => {
                for chunk in gallery::gallery_chunks(&blocks) {
                    self.world.insert(chunk);
                }
            }
        }
        let positions: Vec<ChunkPos> = self.world.positions().collect();
        for pos in positions {
            if let Some(area) = self.world.area(pos) {
                renderer.upload_chunk(pos, &mesh::mesh_chunk(&models, &area));
            }
        }
        info!(
            "built {} chunks of the {:?} world in {:.0?}",
            self.world.len(),
            self.world_kind,
            started.elapsed()
        );
        self.renderer = Some(renderer);
        self.debug_ui = Some(DebugUi::new(&window));
        window.request_redraw();
        self.window = Some(window);
        Ok(())
    }

    /// Locks and hides the cursor for mouse look, or releases it.
    fn set_cursor_grab(&mut self, grab: bool) {
        let Some(window) = &self.window else {
            return;
        };
        let result = if grab {
            // macOS supports only Locked; other platforms may only support Confined.
            window
                .set_cursor_grab(CursorGrabMode::Locked)
                .or_else(|_| window.set_cursor_grab(CursorGrabMode::Confined))
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
            self.fly_input.clear();
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
        } else if self.cursor_grabbed {
            self.fly_input.set_key(key, pressed);
        }
    }

    fn redraw(&mut self, event_loop: &ActiveEventLoop) {
        let now = Instant::now();
        let dt = (now - self.last_frame).as_secs_f32().min(MAX_FRAME_DT);
        self.last_frame = now;
        fly::fly(&mut self.camera, &self.fly_input, dt);

        let (Some(window), Some(renderer), Some(debug_ui)) =
            (&self.window, self.renderer.as_mut(), self.debug_ui.as_mut())
        else {
            return;
        };
        let p = self.camera.position;
        let column = (p.x.floor() as i32, p.z.floor() as i32);
        let biome = self
            .world
            .get(ChunkPos::of_block(column.0, column.1))
            .map(|chunk| {
                chunk.biome(
                    column.0.rem_euclid(16) as usize,
                    column.1.rem_euclid(16) as usize,
                )
            });
        let stats = DebugStats {
            chunks_drawn: renderer.chunks_drawn(),
            chunks_loaded: renderer.chunks_loaded(),
            biome,
            seed: self.terrain.as_ref().map(Terrain::seed),
        };
        let ui = debug_ui.frame(window, &self.camera, &stats);
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
        self.window = None;
    }
}
