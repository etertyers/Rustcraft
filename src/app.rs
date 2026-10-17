//! winit event loop: owns the window, renderer and UI, and the game being played.

use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use anyhow::Result;
use glam::{DVec3, Vec3};
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
    game::{
        Game, chunk_workers,
        environment::{EyeIn, environment},
    },
    inventory::item::ItemRegistry,
    inventory::{
        player::{HOTBAR_SLOTS, SLOTS},
        stack::{ItemStack, Items},
    },
    level::{self, WorldDir},
    player::mode::GameMode,
    render::{
        Renderer, SkyImages,
        atlas::{Atlas, ITEM_PREFIX, MISSING_TILE},
        camera::Camera,
        entities::item_mesh,
        icons::ItemIcons,
    },
    ui::{
        DebugStats, DebugUi, Hud,
        gui::SlotView,
        menu::{Menu, MenuAction},
        screens::{Screen, ScreenAction, ScreenView},
    },
    world::{
        biome::{Biome, BiomeColors},
        block::BlockRegistry,
        chunk::ChunkPos,
        manager::{ChunkManager, ChunkSource},
        mesh::BlockModels,
        worldgen::{gallery, terrain::Terrain},
    },
};

/// A world to start in directly, skipping the main menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorldKind {
    /// The saved world in folder `name` under the saves folder, created
    /// with `seed` (random if `None`) and `mode` if it doesn't exist yet.
    Saved {
        name: String,
        seed: Option<u64>,
        mode: GameMode,
    },
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
    /// `None` starts at the main menu.
    world_kind: Option<WorldKind>,
    items: Option<Items>,
    models: Option<Arc<BlockModels>>,
    icons: Option<Arc<ItemIcons>>,
    /// Exactly one of `menu` and `game` is set once the window is up.
    menu: Option<Menu>,
    game: Option<Game>,
    /// An inventory screen open over the game.
    screen: Option<Screen>,
    /// Every block and item, for the creative inventory.
    catalog: Vec<(ItemStack, SlotView, String)>,
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
    pub fn new(world_kind: Option<WorldKind>) -> Self {
        Self {
            window: None,
            renderer: None,
            debug_ui: None,
            world_kind,
            items: None,
            models: None,
            icons: None,
            menu: None,
            game: None,
            screen: None,
            catalog: Vec::new(),
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
        let textures = assets.join("textures");
        let atlas = Atlas::load(&[
            (&textures.join("blocks"), ""),
            (&textures.join("items"), ITEM_PREFIX),
        ])?;
        let missing_blocks = blocks.missing_textures(|name| atlas.get(name).is_some());
        let missing_items =
            items.missing_textures(|name| atlas.get(&format!("{ITEM_PREFIX}{name}")).is_some());
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
        let sky = SkyImages::load(&textures.join("environment"))?;
        let renderer = Renderer::new(window.clone(), &atlas, &sky)?;
        let models = BlockModels::new(
            &blocks,
            |name| atlas.get(name).map(|tile| atlas.uv_rect(tile)),
            atlas.uv_rect(MISSING_TILE),
            biome_colors,
        );
        let models = Arc::new(models);
        let icons = ItemIcons::new(&atlas, models.clone(), &items);
        let items = Items::new(Arc::new(blocks), Arc::new(items));
        self.catalog = catalog(&items, &icons);
        self.icons = Some(Arc::new(icons));
        self.items = Some(items);
        self.models = Some(models);
        match self.world_kind.clone() {
            Some(kind) => self.game = Some(self.start_game(kind)?),
            None => self.menu = Some(Menu::new(level::saves_dir())),
        }
        self.renderer = Some(renderer);
        self.debug_ui = Some(DebugUi::new(&window, atlas.image())?);
        window.request_redraw();
        self.window = Some(window);
        Ok(())
    }

    fn start_game(&self, kind: WorldKind) -> Result<Game> {
        let (items, models) = (
            self.items.clone().expect("loaded in init"),
            self.models.clone().expect("loaded in init"),
        );
        let ground = config::SEA_LEVEL as f64 + 1.0;
        let (source, spawn, yaw, pitch) = match kind {
            WorldKind::Saved { name, seed, mode } => {
                let saves = level::saves_dir();
                let dir = WorldDir::new(saves.join(&name));
                return if dir.exists() {
                    Game::open(dir, items, models)
                } else {
                    let seed = seed.unwrap_or_else(level::random_seed);
                    Game::create(&saves, &name, seed, mode, items, models)
                };
            }
            // The test worlds below aren't saved.
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
                    gallery::gallery_chunks(&items.blocks)
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
        Ok(Game::new(chunks, items, spawn, spawn, yaw, pitch))
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
        if key == KeyCode::Escape && pressed && self.screen.is_some() {
            self.close_screen();
        } else if key == KeyCode::Escape && pressed {
            self.set_cursor_grab(false);
        } else if key == KeyCode::KeyE && pressed && !event.repeat && self.game.is_some() {
            if self.screen.is_some() {
                self.close_screen();
            } else {
                self.open_screen();
            }
        } else if key == KeyCode::F3 && pressed && !event.repeat {
            if let Some(debug_ui) = self.debug_ui.as_mut() {
                debug_ui.toggle();
            }
        } else if let Some(game) = self.game.as_mut() {
            game.on_key(key, pressed, event.repeat, self.cursor_grabbed);
        }
    }

    /// Opens the inventory (or creative item list) and frees the mouse.
    fn open_screen(&mut self) {
        let Some(game) = &self.game else {
            return;
        };
        self.screen = Some(if game.mode.has_every_item() {
            Screen::Creative { scroll: 0.0 }
        } else {
            Screen::Inventory
        });
        self.set_cursor_grab(false);
    }

    /// Closes the screen, throwing whatever was held on the mouse (1.0.0).
    fn close_screen(&mut self) {
        self.screen = None;
        if let Some(game) = self.game.as_mut()
            && let Some(stack) = game.cursor.take()
        {
            game.throw(stack);
        }
        self.set_cursor_grab(true);
    }

    fn on_screen_action(&mut self, action: ScreenAction) {
        let Some(game) = self.game.as_mut() else {
            return;
        };
        let items = game.items().clone();
        match action {
            ScreenAction::Click(slot, click) => {
                game.inventory.click(slot, click, &mut game.cursor, &items);
            }
            ScreenAction::Throw { all } => {
                let Some(held) = game.cursor.as_mut() else {
                    return;
                };
                let count = if all { held.count } else { 1 };
                held.count -= count;
                let thrown = ItemStack { count, ..*held };
                if held.count == 0 {
                    game.cursor = None;
                }
                game.throw(thrown);
            }
            ScreenAction::Take(stack) => {
                let full = ItemStack {
                    count: items.max_stack(stack.id),
                    ..stack
                };
                game.inventory.set(game.inventory.selected, Some(full));
            }
            ScreenAction::Discard => game.cursor = None,
        }
    }

    /// Opens or creates the world chosen in the menu, or quits.
    fn on_menu_action(&mut self, event_loop: &ActiveEventLoop, action: MenuAction) {
        let (items, models) = (
            self.items.clone().expect("loaded in init"),
            self.models.clone().expect("loaded in init"),
        );
        let game = match action {
            MenuAction::Quit => return event_loop.exit(),
            MenuAction::Open(dir) => Game::open(dir, items, models),
            MenuAction::Create { name, seed, mode } => {
                Game::create(&level::saves_dir(), &name, seed, mode, items, models)
            }
        };
        match game {
            Ok(game) => {
                self.game = Some(game);
                self.menu = None;
                self.set_cursor_grab(true);
            }
            Err(err) => {
                error!("{err:#}");
                if let Some(menu) = self.menu.as_mut() {
                    menu.error = Some(format!("Couldn't open the world: {err:#}"));
                }
            }
        }
    }

    fn redraw(&mut self, event_loop: &ActiveEventLoop) {
        let now = Instant::now();
        let dt = (now - self.last_frame).as_secs_f32();
        self.last_frame = now;
        if self.game.is_none() {
            return self.redraw_menu(event_loop, now);
        }
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
        if let Some(icons) = &self.icons {
            let chunks = &game.chunks;
            let light_at = |p: DVec3| {
                let p = p.floor().as_ivec3();
                chunks.light_at(p.x, p.y, p.z).unwrap_or((15, 0))
            };
            let mesh = item_mesh(
                &game.entities,
                icons,
                &game.camera,
                game.tick_alpha(),
                light_at,
            );
            renderer.set_entities(&mesh);
        }

        let chunks = &game.chunks;
        let stats = DebugStats {
            chunks_drawn: renderer.chunks_drawn(),
            chunks_loaded: chunks.world().len(),
            biome: chunks.world().biome_at(x, z),
            seed: chunks.seed(),
            light: chunks.light_at(x, p.y.floor() as i32, z),
            mode: game.mode,
        };
        let hud = Hud {
            crosshair: self.cursor_grabbed,
            hotbar: (0..HOTBAR_SLOTS)
                .map(|i| {
                    let stack = game.inventory.slot(i)?;
                    let icons = self.icons.as_ref()?;
                    Some(SlotView::of(stack, icons, game.items()))
                })
                .collect(),
            selected: game.inventory.selected,
        };
        let view_of = |stack: &ItemStack| {
            let icons = self.icons.as_ref()?;
            Some(SlotView::of(stack, icons, game.items()))
        };
        let slots: Vec<Option<SlotView>> = (0..SLOTS)
            .map(|i| game.inventory.slot(i).and_then(view_of))
            .collect();
        let names: Vec<Option<String>> = (0..SLOTS)
            .map(|i| game.inventory.slot(i).map(|stack| game.items().name(stack)))
            .collect();
        let view = ScreenView {
            slots: &slots,
            names: &names,
            cursor: game.cursor.as_ref().and_then(view_of),
            catalog: &self.catalog,
        };
        let screen = self.screen.as_mut().map(|screen| (screen, &view));
        let (ui, actions) = debug_ui.frame(window, &game.camera, &stats, &hud, screen);
        let result = renderer.render(&game.camera, &game.environment(), ui);
        self.finish_frame(event_loop, now, result);
        for action in actions {
            self.on_screen_action(action);
        }
    }

    /// The main menu over the sky.
    fn redraw_menu(&mut self, event_loop: &ActiveEventLoop, now: Instant) {
        let (Some(window), Some(renderer), Some(debug_ui), Some(menu)) = (
            &self.window,
            self.renderer.as_mut(),
            self.debug_ui.as_mut(),
            self.menu.as_mut(),
        ) else {
            return;
        };
        let (ui, action) = debug_ui.menu_frame(window, menu);
        // A noon sky behind the menu.
        let noon = environment(6000, 0.0, Biome::Plains, Vec3::NEG_Z, EyeIn::Air);
        let result = renderer.render(&Camera::new(Vec3::ZERO, 0.0, 0.0), &noon, ui);
        self.finish_frame(event_loop, now, result);
        if let Some(action) = action {
            self.on_menu_action(event_loop, action);
        }
    }

    fn finish_frame(&mut self, event_loop: &ActiveEventLoop, now: Instant, result: Result<bool>) {
        match result {
            Ok(presented) => {
                self.frame_skipped = !presented;
                if presented && let Some(debug_ui) = self.debug_ui.as_mut() {
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
            } if !self.cursor_grabbed && self.game.is_some() && self.screen.is_none() => {
                self.set_cursor_grab(true)
            }
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
        if let Some(game) = self.game.as_mut()
            && let Err(err) = game.save()
        {
            error!("saving on quit failed: {err:#}");
        }
        // Drop the GPU surface before the window it points to.
        self.debug_ui = None;
        self.renderer = None;
        // Joins the chunk workers.
        self.game = None;
        self.window = None;
    }
}

/// Every block and item with each data-value variant, for the creative
/// inventory, with how it looks and its name.
fn catalog(items: &Items, icons: &ItemIcons) -> Vec<(ItemStack, SlotView, String)> {
    let blocks = items
        .blocks
        .iter()
        .filter(|def| def.id != 0)
        .flat_map(|def| {
            std::iter::once(0)
                .chain(def.variants.keys().copied().filter(|&data| data != 0))
                .map(|data| ItemStack::with_damage(def.id, 1, data as u16))
        });
    let others = items.items.iter().flat_map(|item| {
        std::iter::once(0)
            .chain(item.variants.keys().copied().filter(|&data| data != 0))
            .map(|data| ItemStack::with_damage(item.id, 1, data as u16))
    });
    blocks
        .chain(others)
        .map(|stack| {
            (
                stack,
                SlotView::of(&stack, icons, items),
                items.name(&stack),
            )
        })
        .collect()
}

/// Pins the cursor in place. macOS supports only Locked; other platforms may
/// only support Confined.
fn lock_cursor(window: &Window) -> Result<(), ExternalError> {
    window
        .set_cursor_grab(CursorGrabMode::Locked)
        .or_else(|_| window.set_cursor_grab(CursorGrabMode::Confined))
}
