//! winit event loop: owns the window, renderer and input state.

use std::{sync::Arc, time::Instant};

use anyhow::{Context, Result};
use glam::Vec3;
use log::{error, info, warn};
use winit::{
    application::ApplicationHandler,
    dpi::LogicalSize,
    event::{DeviceEvent, DeviceId, ElementState, KeyEvent, MouseButton, WindowEvent},
    event_loop::ActiveEventLoop,
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
        cube::{self, FaceTexture},
    },
    world::block::{BlockId, BlockRegistry},
};

/// The block shown as M1's test cube.
const GRASS: BlockId = 2;

/// Longest frame step applied to movement, so a stall doesn't teleport the camera.
const MAX_FRAME_DT: f32 = 0.1;

pub struct App {
    window: Option<Arc<Window>>,
    renderer: Option<Renderer>,
    camera: Camera,
    fly_input: FlyInput,
    cursor_grabbed: bool,
    last_frame: Instant,
    /// First fatal error; the event loop exits and `main` returns it.
    error: Option<anyhow::Error>,
}

impl Default for App {
    fn default() -> Self {
        Self {
            window: None,
            renderer: None,
            // A few blocks south of the test cube, looking north and slightly down at it.
            camera: Camera::new(Vec3::new(0.5, 1.8, 3.5), 0.0, -0.2),
            fly_input: FlyInput::default(),
            cursor_grabbed: false,
            last_frame: Instant::now(),
            error: None,
        }
    }
}

impl App {
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

        let grass = blocks.get(GRASS).context("grass block not defined")?;
        let mesh = cube::cube_mesh([0.0; 3], |face| FaceTexture {
            uv: atlas.uv_rect(
                grass
                    .textures
                    .as_ref()
                    .and_then(|t| t.get(face))
                    .map_or(MISSING_TILE, |name| atlas.tile_or_missing(name)),
            ),
            tint: grass.tint.color(face),
        });
        self.renderer = Some(Renderer::new(window.clone(), &atlas, &mesh)?);
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
        } else if self.cursor_grabbed {
            self.fly_input.set_key(key, pressed);
        }
    }

    fn redraw(&mut self, event_loop: &ActiveEventLoop) {
        let now = Instant::now();
        let dt = (now - self.last_frame).as_secs_f32().min(MAX_FRAME_DT);
        self.last_frame = now;
        fly::fly(&mut self.camera, &self.fly_input, dt);

        if let Some(renderer) = self.renderer.as_mut()
            && let Err(err) = renderer.render(&self.camera)
        {
            self.fail(event_loop, err);
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        if let Err(err) = self.init(event_loop) {
            self.fail(event_loop, err);
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if let Some(renderer) = self.renderer.as_mut() {
                    renderer.resize(size);
                }
            }
            WindowEvent::Focused(false) => self.set_cursor_grab(false),
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

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }

    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        // Drop the GPU surface before the window it points to.
        self.renderer = None;
        self.window = None;
    }
}
