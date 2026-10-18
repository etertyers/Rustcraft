//! winit event loop: owns the window, renderer and UI, and the game being played.

use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
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
    assets,
    bench::{self, Bench, FrameTimes},
    config,
    entity::mobs::MobRegistry,
    game::{Container, Game, chunk_workers, environment::environment},
    inventory::item::ItemRegistry,
    inventory::{
        crafting::Recipes,
        player::HOTBAR_SLOTS,
        slots::SlotRef,
        stack::{ItemStack, Items},
    },
    level::{self, WorldDir},
    options::Options,
    player::{
        mode::GameMode,
        stats::{DamageSource, MAX_AIR},
    },
    render::{
        Renderer, SkyImages,
        atlas::{Atlas, ITEM_PREFIX, MISSING_TILE},
        camera::Camera,
        crack,
        entities::{add_particles, item_mesh},
        icons::ItemIcons,
        models::{add_arrow, add_mob},
    },
    ui::{
        DebugStats, DebugUi, Hud,
        gui::{SlotView, SurvivalHud},
        menu::{Menu, MenuAction},
        pause::{PauseAction, PauseMenu},
        screens::{Screen, ScreenAction, ScreenView},
    },
    world::{
        biome::{Biome, BiomeColors},
        block::BlockRegistry,
        chunk::ChunkPos,
        manager::{ChunkManager, ChunkSource},
        mesh::{BlockModels, Mesh},
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

/// Mobs farther than this from the camera aren't drawn (1.0.0's
/// `isInRangeToRenderDist` for a mob about a block across).
const MOB_DRAW_DISTANCE: f64 = 64.0;
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
    mob_defs: Option<Arc<MobRegistry>>,
    icons: Option<Arc<ItemIcons>>,
    /// Exactly one of `menu` and `game` is set once the window is up.
    menu: Option<Menu>,
    game: Option<Game>,
    /// An inventory screen open over the game.
    screen: Option<Screen>,
    /// Whether the player was in bed last frame.
    sleeping: bool,
    /// The player's settings, from `options.ron`.
    options: Options,
    /// The pause menu, open over a stopped game.
    pause: Option<PauseMenu>,
    /// Atlas rects of the mining crack's stages.
    crack_uvs: Vec<[f32; 4]>,
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
    /// `--bench`: the flight being timed.
    bench: Option<Bench>,
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
            mob_defs: None,
            icons: None,
            menu: None,
            game: None,
            screen: None,
            sleeping: false,
            options: Options::load(&Options::path()),
            pause: None,
            crack_uvs: Vec::new(),
            catalog: Vec::new(),
            cursor_grabbed: false,
            last_frame: Instant::now(),
            occluded: false,
            frame_skipped: false,
            error: None,
            bench: None,
        }
    }

    pub fn with_bench(mut self, bench: Option<Bench>) -> Self {
        if let Some(distance) = bench.as_ref().and_then(|bench| bench.distance) {
            self.options.render_distance = distance;
        }
        self.bench = bench;
        self
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

        // The mining crack's stages; a missing one shows the checkerboard.
        self.crack_uvs = (0..crack::STAGES)
            .map(|i| {
                let tile = atlas
                    .get(&format!("destroy_stage_{i}"))
                    .unwrap_or(MISSING_TILE);
                atlas.uv_rect(tile)
            })
            .collect();

        let biome_colors =
            BiomeColors::load(&assets.join("textures/colormap")).unwrap_or_else(|err| {
                warn!("{err:#}; using default biome colors");
                BiomeColors::uniform()
            });
        let sky = SkyImages::load(&textures.join("environment"))?;
        let mob_defs = Arc::new(MobRegistry::load(&assets.join("data/mobs.ron"))?);
        // Textures for entity models: each mob kind, then the arrow.
        let mob_textures = mob_defs
            .iter()
            .map(|(_, def)| def.texture.as_str())
            .chain(["arrow.png"])
            .map(|name| {
                let path = textures.join("entity").join(name);
                Ok(image::open(&path)
                    .with_context(|| format!("loading {}", path.display()))?
                    .into_rgba8())
            })
            .collect::<Result<Vec<_>>>()?;
        let mut renderer = Renderer::new(window.clone(), &atlas, &sky, &mob_textures)?;
        // The bench measures how fast frames can go.
        renderer.set_vsync(self.options.vsync && self.bench.is_none());
        renderer.set_offscreen_fallback(self.bench.is_some());
        let models = BlockModels::new(
            &blocks,
            |name| atlas.get(name).map(|tile| atlas.uv_rect(tile)),
            atlas.uv_rect(MISSING_TILE),
            biome_colors,
        );
        let models = Arc::new(models);
        let icons = ItemIcons::new(&atlas, models.clone(), &items);
        let recipes = Recipes::load(&assets.join("data/recipes.ron"))?;
        let items = Items::new(Arc::new(blocks), Arc::new(items)).with_recipes(Arc::new(recipes));
        self.catalog = catalog(&items, &icons);
        self.icons = Some(Arc::new(icons));
        self.items = Some(items);
        self.models = Some(models);
        self.mob_defs = Some(mob_defs);
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
        let mobs = self.mob_defs.clone().expect("loaded in init");
        let mut game = self.start_world(kind)?.with_mobs(mobs);
        game.apply_options(&self.options);
        Ok(game)
    }

    fn start_world(&self, kind: WorldKind) -> Result<Game> {
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
        let sleeping = self.game.as_ref().is_some_and(Game::is_sleeping);
        if key == KeyCode::Escape && pressed && sleeping {
            if let Some(game) = self.game.as_mut() {
                game.wake();
            }
        } else if key == KeyCode::Escape && pressed && self.screen.is_some() {
            self.close_screen();
        } else if key == KeyCode::Escape && pressed && !event.repeat && self.pause.is_some() {
            if self.pause.as_mut().and_then(PauseMenu::back) == Some(PauseAction::Resume) {
                self.resume();
            }
        } else if key == KeyCode::Escape && pressed && !event.repeat && self.game.is_some() {
            self.pause_game();
        } else if key == KeyCode::Escape && pressed {
            self.set_cursor_grab(false);
        } else if key == KeyCode::KeyE
            && pressed
            && !event.repeat
            && self.game.is_some()
            && self.pause.is_none()
            && !sleeping
        {
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

    /// Opens the pause menu over a world being played (not over a screen,
    /// the death screen or a bed), freeing the mouse. The game stops.
    fn pause_game(&mut self) {
        if self.playing() && self.screen.is_none() && !self.sleeping && self.pause.is_none() {
            self.pause = Some(PauseMenu::default());
            self.set_cursor_grab(false);
        }
    }

    fn resume(&mut self) {
        self.pause = None;
        self.set_cursor_grab(true);
    }

    /// Saves the world and goes back to the title screen.
    fn quit_to_title(&mut self) {
        if let Some(mut game) = self.game.take()
            && let Err(err) = game.save()
        {
            error!("saving on quitting to the title failed: {err:#}");
        }
        if let Some(renderer) = self.renderer.as_mut() {
            renderer.clear_world();
        }
        self.pause = None;
        self.screen = None;
        self.sleeping = false;
        self.menu = Some(Menu::new(level::saves_dir()));
        self.set_cursor_grab(false);
    }

    /// Applies settings changed on the Options screen and saves them.
    fn options_changed(&mut self) {
        if let Some(renderer) = self.renderer.as_mut() {
            renderer.set_vsync(self.options.vsync);
        }
        if let Some(game) = self.game.as_mut() {
            game.apply_options(&self.options);
        }
        let path = Options::path();
        if let Err(err) = self.options.save(&path) {
            error!("{err:#}");
        }
    }

    /// In a world and alive.
    fn playing(&self) -> bool {
        self.game.as_ref().is_some_and(|game| !game.stats.is_dead())
    }

    /// Opens the inventory (or creative item list) and frees the mouse.
    fn open_screen(&mut self) {
        let Some(game) = self.game.as_ref().filter(|game| !game.stats.is_dead()) else {
            return;
        };
        self.screen = Some(if game.mode.has_every_item() {
            Screen::Creative { scroll: 0.0 }
        } else {
            Screen::Inventory
        });
        if let Some(game) = self.game.as_mut() {
            game.open_inventory();
        }
        self.set_cursor_grab(false);
    }

    /// Closes the screen, throwing out whatever was held on the mouse or
    /// left in a crafting grid (1.0.0).
    fn close_screen(&mut self) {
        self.screen = None;
        if let Some(game) = self.game.as_mut() {
            game.close_container();
        }
        self.set_cursor_grab(true);
    }

    /// Shows the screen for a container the game opened (a right-clicked
    /// crafting table, chest or furnace).
    fn show_opened_container(&mut self) {
        if self.screen.is_some() {
            return;
        }
        let Some(container) = self.game.as_ref().and_then(|game| game.container.as_ref()) else {
            return;
        };
        self.screen = Some(match container {
            Container::Crafting { size: 2, .. } => Screen::Inventory,
            Container::Crafting { .. } => Screen::Crafting,
            Container::Chest(_) => Screen::Chest,
            Container::Furnace(_) => Screen::Furnace,
        });
        self.set_cursor_grab(false);
    }

    fn on_screen_action(&mut self, action: ScreenAction) {
        let Some(game) = self.game.as_mut() else {
            return;
        };
        let items = game.items().clone();
        match action {
            ScreenAction::Click(slot, click) => game.click_slot(slot, click),
            ScreenAction::Drag { slots, click } => game.drag_across(&slots, click),
            ScreenAction::Collect(_) => game.collect_onto_cursor(),
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
            ScreenAction::Respawn => {
                game.respawn();
                self.set_cursor_grab(true);
            }
            ScreenAction::LeaveBed => game.wake(),
            ScreenAction::Pause(PauseAction::Resume) => self.resume(),
            ScreenAction::Pause(PauseAction::SaveAndQuit) => self.quit_to_title(),
        }
    }

    /// Opens or creates the world chosen in the menu, or quits.
    fn on_menu_action(&mut self, event_loop: &ActiveEventLoop, action: MenuAction) {
        let (items, models) = (
            self.items.clone().expect("loaded in init"),
            self.models.clone().expect("loaded in init"),
        );
        let mobs = self.mob_defs.clone().expect("loaded in init");
        let game = match action {
            MenuAction::Quit => return event_loop.exit(),
            MenuAction::Open(dir) => Game::open(dir, items, models),
            MenuAction::Create { name, seed, mode } => {
                Game::create(&level::saves_dir(), &name, seed, mode, items, models)
            }
        }
        .map(|game| game.with_mobs(mobs));
        match game {
            Ok(mut game) => {
                game.apply_options(&self.options);
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
        self.show_opened_container();
        // Lying in bed frees the mouse for the Leave Bed button; getting up
        // takes it back.
        let sleeping = self.game.as_ref().is_some_and(Game::is_sleeping);
        if sleeping != self.sleeping {
            self.sleeping = sleeping;
            self.set_cursor_grab(!sleeping);
        }
        // Dying closes any screen and frees the mouse for the Respawn button.
        if !self.playing() {
            self.screen = None;
            if self.cursor_grabbed {
                self.set_cursor_grab(false);
            }
        }
        let (Some(window), Some(renderer), Some(debug_ui), Some(game)) = (
            &self.window,
            self.renderer.as_mut(),
            self.debug_ui.as_mut(),
            self.game.as_mut(),
        ) else {
            return;
        };
        if self.bench.is_some() {
            game.glide(bench::SPEED * dt as f64, bench::HEIGHT);
        }
        // The pause menu stops the game; the camera still follows it.
        game.advance(if self.pause.is_some() { 0.0 } else { dt });
        let ticked = Instant::now();

        let p = game.camera.position;
        let (x, z) = (p.x.floor() as i32, p.z.floor() as i32);
        let changes = game.chunks.update(ChunkPos::of_block(x, z));
        game.on_chunks_loaded(&changes.loaded);
        for (pos, mesh) in &changes.meshes {
            renderer.upload_chunk(*pos, mesh);
        }
        for pos in changes.unloaded {
            renderer.remove_chunk(pos);
        }
        let uploaded = Instant::now();
        renderer.set_outline(game.target.map(|hit| hit.block));
        let crack = game.mining_progress().and_then(|(cell, progress)| {
            let uv = *self.crack_uvs.get(crack::stage(progress))?;
            let block = game.chunks.world().block(cell.x, cell.y, cell.z)?;
            // Over the block's own box (a slab's half, a door's slab), or
            // the whole cell for blocks without one.
            let (min, max) = game
                .items()
                .blocks
                .get(block.id())
                .and_then(|def| def.collision_box(block.data()))
                .unwrap_or(([0.0; 3], [1.0; 3]));
            let cell = cell.as_vec3();
            Some(crack::crack_mesh(
                cell + Vec3::from(min),
                cell + Vec3::from(max),
                uv,
            ))
        });
        renderer.set_crack(crack.as_ref());
        if let Some(icons) = &self.icons {
            let chunks = &game.chunks;
            let light_at = |p: DVec3| {
                let p = p.floor().as_ivec3();
                chunks.light_at(p.x, p.y, p.z).unwrap_or((15, 0))
            };
            let mut mesh = item_mesh(
                &game.entities,
                icons,
                &game.camera,
                game.tick_alpha(),
                light_at,
            );
            if let Some(models) = &self.models {
                add_particles(
                    &mut mesh,
                    &game.particles,
                    models,
                    &game.camera,
                    game.tick_alpha(),
                    light_at,
                );
            }
            renderer.set_entities(&mesh);
            let alpha = game.tick_alpha() as f32;
            let mut meshes: Vec<(usize, Mesh)> = Vec::new();
            let eye = game.camera.position.as_dvec3();
            let near = |mob: &&crate::entity::mob::Mob| {
                mob.eye().distance_squared(eye) < MOB_DRAW_DISTANCE * MOB_DRAW_DISTANCE
            };
            for mob in game.mobs.iter().filter(near) {
                let index = match meshes.iter().position(|(kind, _)| *kind == mob.kind) {
                    Some(index) => index,
                    None => {
                        meshes.push((mob.kind, Mesh::default()));
                        meshes.len() - 1
                    }
                };
                let def = game.mob_defs().get(mob.kind);
                add_mob(&mut meshes[index].1, mob, def, alpha, light_at(mob.eye()));
            }
            // The arrow texture follows the mob textures.
            let mut arrows = Mesh::default();
            for arrow in &game.arrows {
                add_arrow(&mut arrows, arrow, alpha, light_at(arrow.position));
            }
            meshes.push((game.mob_defs().iter().count(), arrows));
            renderer.set_mobs(&meshes);
        }

        let chunks = &game.chunks;
        let stats = DebugStats {
            chunks_drawn: renderer.chunks_drawn(),
            chunks_loaded: chunks.world().len(),
            biome: chunks.world().biome_at(x, z),
            seed: chunks.seed(),
            light: chunks.light_at(x, p.y.floor() as i32, z),
            mode: game.mode,
            time: game.time(),
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
            survival: (game.mode.is_vulnerable() && !game.stats.is_dead()).then(|| SurvivalHud {
                health: game.stats.health,
                food: game.stats.food,
                air: (game.stats.air < MAX_AIR).then_some(game.stats.air),
            }),
            death: game.stats.is_dead().then(|| {
                let source = game.stats.last_source.unwrap_or(DamageSource::Mob);
                format!("Player {}", source.death_message())
            }),
            sleep: game.sleep_fade(),
            message: game.message.as_ref().map(|(text, _)| text.clone()),
        };
        let view_of = |stack: &ItemStack| {
            let icons = self.icons.as_ref()?;
            Some(SlotView::of(stack, icons, game.items()))
        };
        let slot = |slot: SlotRef| {
            let stack = game.slot_stack(slot)?;
            Some((view_of(&stack)?, game.items().name(&stack)))
        };
        let view = ScreenView {
            slot: &slot,
            furnace: game.furnace_progress(),
            cursor: game.cursor.as_ref().and_then(view_of),
            catalog: &self.catalog,
        };
        let screen = self.screen.as_mut().map(|screen| (screen, &view));
        let options_before = self.options.clone();
        let pause = self.pause.as_mut().map(|menu| (menu, &mut self.options));
        let (ui, actions) = debug_ui.frame(window, &game.camera, &stats, &hud, screen, pause);
        let result = renderer.render(&game.camera, &game.environment(), ui);
        let render_distance = game.chunks.radius();
        let place = format!(
            "ended at {:.0}, {} chunks loaded, {} drawn",
            game.camera.position,
            game.chunks.world().len(),
            renderer.chunks_drawn()
        );
        let offscreen = renderer.drew_offscreen();
        self.finish_frame(event_loop, now, result);
        if let Some(bench) = self.bench.as_mut() {
            bench.record(FrameTimes {
                frame: dt,
                game: ticked - now,
                chunks: uploaded - ticked,
                draw: uploaded.elapsed(),
                offscreen,
            });
            if bench.done() {
                let report = format!("{}\n  {place}", bench.report(render_distance));
                info!("{report}");
                println!("{report}");
                event_loop.exit();
            }
        }
        if self.options != options_before {
            self.options_changed();
        }
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
        let options_before = self.options.clone();
        let (ui, action) = debug_ui.menu_frame(window, menu, &mut self.options);
        // A noon sky behind the menu.
        let noon = environment(6000, 0.0, Biome::Plains, Vec3::NEG_Z, None);
        let result = renderer.render(&Camera::new(Vec3::ZERO, 0.0, 0.0), &noon, ui);
        self.finish_frame(event_loop, now, result);
        if self.options != options_before {
            self.options_changed();
        }
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
            // Like 1.0.0, leaving the window pauses the game.
            WindowEvent::Focused(false) => {
                self.set_cursor_grab(false);
                self.pause_game();
            }
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
            } if !self.cursor_grabbed
                && self.screen.is_none()
                && self.pause.is_none()
                && self.playing()
                && !self.sleeping =>
            {
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
