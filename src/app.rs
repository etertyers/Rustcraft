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
    assets,
    bench::{self, Bench, FrameTimes},
    config,
    entity::mobs::MobRegistry,
    figure::{self, Figure, PlayerMeshes},
    game::{BenchKind, Container, Game, chunk_workers, environment::environment},
    inventory::item,
    inventory::{
        crafting::Recipes,
        player::HOTBAR_SLOTS,
        slots::SlotRef,
        stack::{ItemStack, Items},
    },
    level::{self, WorldDir},
    look::Look,
    options::Options,
    pack::model::ItemModels,
    player::{
        mode::GameMode,
        stats::{DamageSource, MAX_AIR},
    },
    render::{
        Renderer,
        atlas::Atlas,
        camera::Camera,
        crack,
        entities::{
            EffectUvs, add_cart_blocks, add_effects, add_falling_blocks, add_fireballs, add_orbs,
            add_particles, add_primed_tnt, item_mesh,
        },
        held::HeldCache,
        icons::ItemIcons,
        models::{DYE_COLORS, add_arrow, add_minecart, add_mob, add_mob_model},
    },
    shots,
    ui::{
        DebugStats, DebugUi, Hud,
        gui::{SlotView, SurvivalHud},
        menu::{Menu, MenuAction},
        pause::{PauseAction, PauseMenu},
        screens::{BenchView, Screen, ScreenAction, ScreenView},
    },
    world::{
        biome::Biome,
        block,
        chunk::ChunkPos,
        manager::{ChunkManager, ChunkSource},
        mesh::{BlockModels, Mesh},
        shape::Cuboid,
        worldgen::{end::End, gallery, nether::Nether, terrain::Terrain},
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
    /// The Nether of a seed, starting there (`--nether`).
    Nether { seed: u64 },
    /// The End of a seed, starting there with the dragon circling
    /// (`--end`).
    End { seed: u64 },
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
    effect_uvs: EffectUvs,
    /// The block and item atlas, for playing its animations.
    atlas: Option<Atlas>,
    /// Display transforms and sprites of the item models.
    item_models: Option<Arc<ItemModels>>,
    /// Geometry of items in hand, built from the atlas.
    held: HeldCache,
    /// Whether the skin has slim arms.
    skin_slim: bool,
    /// The view worlds start in.
    start_view: crate::game::view::ViewMode,
    /// The resource packs the look was built with, highest first.
    applied_packs: Vec<String>,
    /// When texture animations started.
    animation_start: Instant,
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
    /// `--shot`: the scene being captured.
    shot: Option<ShotRun>,
}

/// How far `--shot` has got with its scene.
pub struct ShotRun {
    shot: &'static shots::Shot,
    out: std::path::PathBuf,
    /// Staged, and frames drawn since.
    staged: Option<u32>,
    captured: bool,
}

/// Frames drawn after staging a scene before capturing it, so remeshed
/// chunks and the UI catch up.
const SHOT_SETTLE_FRAMES: u32 = 10;

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
            effect_uvs: EffectUvs::default(),
            atlas: None,
            item_models: None,
            held: HeldCache::default(),
            skin_slim: false,
            start_view: Default::default(),
            applied_packs: Vec::new(),
            animation_start: Instant::now(),
            catalog: Vec::new(),
            cursor_grabbed: false,
            last_frame: Instant::now(),
            occluded: false,
            frame_skipped: false,
            error: None,
            bench: None,
            shot: None,
        }
    }

    /// Captures `shot` to `out` instead of playing.
    pub fn with_shot(mut self, shot: &'static shots::Shot, out: std::path::PathBuf) -> Self {
        // Every scene is seen at the standard render distance.
        self.options.render_distance = config::RENDER_DISTANCE;
        if self.world_kind.is_none() || shot.world.is_some() {
            self.world_kind = Some(shot.world.clone().unwrap_or(WorldKind::Flat));
        }
        self.shot = Some(ShotRun {
            shot,
            out,
            staged: None,
            captured: false,
        });
        self
    }

    /// Uses these resource packs this time, without saving them as chosen.
    pub fn with_packs(mut self, packs: Vec<String>) -> Self {
        self.options.resource_packs = packs;
        self
    }

    pub fn with_bench(mut self, bench: Option<Bench>) -> Self {
        if let Some(distance) = bench.as_ref().and_then(|bench| bench.distance) {
            self.options.render_distance = distance;
        }
        self.bench = bench;
        self
    }

    /// Starts worlds in the given view (`--view`).
    pub fn with_view(mut self, view: crate::game::view::ViewMode) -> Self {
        self.start_view = view;
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
        // The bench draws a full 1080p screen's worth.
        let (width, height) = match self.bench {
            Some(_) => bench::WINDOW_SIZE,
            None => (config::WINDOW_WIDTH, config::WINDOW_HEIGHT),
        };
        let attrs = Window::default_attributes()
            .with_title(config::WINDOW_TITLE)
            .with_inner_size(LogicalSize::new(width, height));
        let window = Arc::new(event_loop.create_window(attrs)?);
        let assets = assets::dir();
        let blocks = block::registry();
        info!(
            "loaded {} blocks and {} items",
            blocks.iter().count(),
            item::registry().len()
        );
        let mob_defs = Arc::new(MobRegistry::load(&assets.join("data/mobs.ron"))?);
        self.applied_packs = self.options.resource_packs.clone();
        let look = Look::with_packs(&assets, &self.applied_packs, &mob_defs)?;
        let mut renderer =
            Renderer::new(window.clone(), &look.atlas, &look.sky, &look.mob_textures)?;
        // The bench measures how fast frames can go.
        renderer.set_vsync(self.options.vsync && self.bench.is_none() && self.shot.is_none());
        renderer.set_offscreen_fallback(self.bench.is_some() || self.shot.is_some());
        self.debug_ui = Some(DebugUi::new(&window, look.atlas.image(), &look.res)?);
        let recipes = Recipes::load(&assets.join("data/recipes.ron"))?;
        self.items = Some(Items::new().with_recipes(Arc::new(recipes)));
        self.mob_defs = Some(mob_defs);
        self.install_look(look);
        match self.world_kind.clone() {
            Some(kind) => self.game = Some(self.start_game(kind)?),
            None => self.menu = Some(Menu::new(level::saves_dir())),
        }
        self.renderer = Some(renderer);
        window.request_redraw();
        self.window = Some(window);
        Ok(())
    }

    fn start_game(&self, kind: WorldKind) -> Result<Game> {
        let mobs = self.mob_defs.clone().expect("loaded in init");
        let mut game = self.start_world(kind)?.with_mobs(mobs);
        game.apply_options(&self.options);
        game.set_view_mode(self.start_view);
        if let Some(run) = &self.shot {
            let shot = run.shot;
            game.set_time_and_mode(
                shot.time.unwrap_or(game.time()),
                shot.mode.unwrap_or(game.mode),
            );
            if let Some((feet, yaw, pitch)) = shot.view {
                game.teleport(DVec3::from_array(feet), yaw, pitch);
            }
        }
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
            // On the floor nearest the origin.
            WorldKind::Nether { seed } => {
                let nether = Nether::new(seed);
                let spawn = (0..64)
                    .flat_map(|r| [(r, 0), (0, r), (-r, 0), (0, -r)])
                    .find_map(|(x, z)| {
                        let y = nether.standing_height(x, z)?;
                        Some(DVec3::new(x as f64 + 0.5, y as f64, z as f64 + 0.5))
                    })
                    .unwrap_or(DVec3::new(0.5, 64.0, 0.5));
                let chunks = ChunkManager::new(
                    ChunkSource::Nether(Box::new(nether)),
                    models,
                    config::RENDER_DISTANCE,
                    chunk_workers(),
                );
                let game = Game::new(chunks, items, spawn, spawn, 0.0, 0.0);
                return Ok(game.starting_in(crate::world::dimension::Dimension::Nether));
            }
            // On the island, a little way from its middle.
            WorldKind::End { seed } => {
                let end = End::new(seed);
                let top = end
                    .top(24, 6)
                    .unwrap_or(crate::world::worldgen::end::SURFACE_Y);
                let spawn = DVec3::new(24.5, top as f64 + 1.0, 6.5);
                let chunks = ChunkManager::new(
                    ChunkSource::End(Box::new(end)),
                    models,
                    config::RENDER_DISTANCE,
                    chunk_workers(),
                );
                let mut game = Game::new(chunks, items, spawn, spawn, 0.0, 0.0)
                    .starting_in(crate::world::dimension::Dimension::End);
                game.camera.yaw = -std::f32::consts::FRAC_PI_2;
                return Ok(game);
            }
            // On the grass at the origin.
            WorldKind::Flat => (ChunkSource::Flat, DVec3::new(0.5, ground, 0.5), 0.0, 0.0),
            // In front of the first gallery row, looking south along it.
            WorldKind::Gallery => (
                ChunkSource::Fixed(
                    gallery::gallery_chunks(items.blocks)
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
        } else if self.screen == Some(Screen::Anvil) && key != KeyCode::F3 {
            // The name box has the keyboard (E and the digits are letters).
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
        // The bench keeps flying when the window loses focus, and a scene
        // being captured never pauses.
        if self.bench.is_none()
            && self.shot.is_none()
            && self.playing()
            && self.screen.is_none()
            && !self.sleeping
            && self.pause.is_none()
        {
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

    /// Keeps a newly built look's models, icons and atlas.
    fn install_look(&mut self, look: Look) {
        if let Some(items) = &self.items {
            self.catalog = catalog(items, &look.icons);
        }
        self.icons = Some(Arc::new(look.icons));
        self.models = Some(look.models);
        self.crack_uvs = look.crack_uvs;
        self.effect_uvs = look.effect_uvs;
        self.atlas = Some(look.atlas);
        self.item_models = Some(look.item_models);
        self.skin_slim = look.skin_slim;
        self.held.clear();
    }

    /// Rebuilds the look with the selected resource packs and swaps it in
    /// everywhere, without restarting. On failure the old look stays.
    fn apply_resource_packs(&mut self) {
        let Some(mobs) = self.mob_defs.clone() else {
            return;
        };
        let packs = self.options.resource_packs.clone();
        info!("applying resource packs {packs:?}");
        let look = match Look::with_packs(&assets::dir(), &packs, &mobs) {
            Ok(look) => look,
            Err(err) => return error!("can't apply resource packs: {err:#}"),
        };
        if let Some(renderer) = self.renderer.as_mut() {
            renderer.set_textures(&look.atlas, &look.sky, &look.mob_textures);
        }
        if let Some(debug_ui) = self.debug_ui.as_mut()
            && let Err(err) = debug_ui.set_textures(look.atlas.image(), &look.res)
        {
            error!("can't load the GUI images: {err:#}");
        }
        if let Some(game) = self.game.as_mut() {
            game.chunks.set_models(look.models.clone());
        }
        self.applied_packs = packs;
        self.animation_start = Instant::now();
        self.install_look(look);
    }

    /// Plays texture animations up to now.
    fn animate_textures(&mut self) {
        let (Some(atlas), Some(renderer)) = (self.atlas.as_mut(), self.renderer.as_ref()) else {
            return;
        };
        let ticks = self.animation_start.elapsed().as_secs_f64() * config::TICKS_PER_SECOND as f64;
        let changed = atlas.animate(ticks as u64);
        if !changed.is_empty() {
            renderer.update_atlas(&changed);
        }
    }

    /// Applies settings changed on the Options screen and saves them.
    fn options_changed(&mut self) {
        if let Some(renderer) = self.renderer.as_mut() {
            renderer.set_vsync(self.options.vsync);
        }
        if self.options.resource_packs != self.applied_packs {
            self.apply_resource_packs();
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
            Container::Brewing(_) => Screen::Brewing,
            Container::Dispenser(_, dropper) => Screen::Dispenser { dropper: *dropper },
            Container::Hopper(_) => Screen::Hopper,
            Container::Merchant(_) => Screen::Merchant,
            Container::Bench(bench) => match bench.kind {
                BenchKind::Enchanting => Screen::Enchanting,
                BenchKind::Anvil => Screen::Anvil,
                BenchKind::Grindstone => Screen::Grindstone,
                BenchKind::Smithing => Screen::Smithing,
            },
            Container::Cart(index) => match self.game.as_ref().and_then(|g| g.carts.get(*index)) {
                Some(cart) if cart.kind == crate::entity::minecart::CartKind::Hopper => {
                    Screen::Hopper
                }
                _ => Screen::Chest,
            },
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
                let thrown = ItemStack {
                    count,
                    ..held.clone()
                };
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
            ScreenAction::Enchant(button) => {
                game.enchant_item(button);
            }
            ScreenAction::AnvilName(name) => game.set_anvil_name(name),
            ScreenAction::Trade(index) => {
                game.trade(index);
            }
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
                game.set_view_mode(self.start_view);
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
        self.animate_textures();
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
        // The pause menu stops the game; the camera still follows it. A
        // scene being captured stands still.
        let frozen = self.pause.is_some() || self.shot.is_some();
        game.advance(if frozen { 0.0 } else { dt });
        // Another dimension's chunks replace this one's.
        if game.take_dimension_change() {
            renderer.clear_world();
        }
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
        let center = ChunkPos::of_block(x, z);
        let mut capturing = false;
        if let Some(run) = self.shot.as_mut() {
            match run.staged {
                None if game.chunks.settled(center) => {
                    (run.shot.stage)(game);
                    run.staged = Some(0);
                }
                Some(frames) if frames >= SHOT_SETTLE_FRAMES && game.chunks.settled(center) => {
                    renderer.capture_next_frame(run.out.clone());
                    capturing = true;
                }
                Some(frames) => run.staged = Some(frames + 1),
                None => {}
            }
            if run.shot.overlay && !debug_ui.is_visible() {
                debug_ui.toggle();
            }
        }
        let shapes = crate::world::shape::shapes();
        renderer.set_outline(game.target.and_then(|hit| {
            let block = game
                .chunks
                .world()
                .block(hit.block.x, hit.block.y, hit.block.z)?;
            Some((hit.block, shapes.outline(block).to_vec()))
        }));
        let crack = game.mining_progress().and_then(|(cell, progress)| {
            let uv = *self.crack_uvs.get(crack::stage(progress))?;
            let block = game.chunks.world().block(cell.x, cell.y, cell.z)?;
            // Over the block's outline (a slab's half, a door's slab).
            let bounds = Cuboid::bounds(shapes.outline(block)).unwrap_or(Cuboid::FULL);
            let cell = cell.as_vec3();
            Some(crack::crack_mesh(
                cell + Vec3::from(bounds.min),
                cell + Vec3::from(bounds.max),
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
            let view_camera = game.view_camera();
            let mut mesh = item_mesh(
                &game.entities,
                icons,
                &view_camera,
                game.tick_alpha(),
                light_at,
            );
            if let Some(models) = &self.models {
                add_falling_blocks(
                    &mut mesh,
                    &game.falling,
                    models,
                    game.tick_alpha(),
                    light_at,
                );
                add_cart_blocks(&mut mesh, &game.carts, models, game.tick_alpha(), light_at);
                add_primed_tnt(&mut mesh, &game.primed, models, game.tick_alpha(), light_at);
                add_fireballs(
                    &mut mesh,
                    &game.fireballs,
                    icons,
                    &view_camera,
                    game.tick_alpha(),
                );
                add_particles(
                    &mut mesh,
                    &game.particles,
                    models,
                    &view_camera,
                    game.tick_alpha(),
                    light_at,
                );
                add_effects(
                    &mut mesh,
                    &game.effects,
                    &self.effect_uvs,
                    &view_camera,
                    game.tick_alpha(),
                    light_at,
                );
                add_orbs(
                    &mut mesh,
                    &game.orbs,
                    &self.effect_uvs,
                    &view_camera,
                    game.tick_alpha(),
                    light_at,
                );
            }
            // The player: the hand in first person, the model in third.
            let player = match (&self.atlas, &self.item_models) {
                (Some(atlas), Some(item_models)) => {
                    let fig = Figure {
                        icons,
                        item_models,
                        atlas: atlas.image(),
                        slim: self.skin_slim,
                    };
                    let (feet, _) = game.player_body();
                    let body_light = light_at(feet.as_dvec3() + DVec3::Y);
                    let hand_light = light_at(game.camera.position.as_dvec3());
                    figure::build(game, &mut self.held, &fig, body_light, hand_light)
                }
                _ => PlayerMeshes::default(),
            };
            mesh.append(&player.items);
            renderer.set_entities(&mesh);
            renderer.set_hand(Some(&player.hand_arm), Some(&player.hand_item));
            let alpha = game.tick_alpha() as f32;
            let mut meshes: Vec<(usize, Mesh)> = Vec::new();
            let eye = game.camera.position.as_dvec3();
            let near = |mob: &&crate::entity::mob::Mob| {
                mob.eye().distance_squared(eye) < MOB_DRAW_DISTANCE * MOB_DRAW_DISTANCE
            };
            let defs = game.mob_defs();
            let mesh_for = |meshes: &mut Vec<(usize, Mesh)>, slot: usize| -> usize {
                match meshes.iter().position(|(s, _)| *s == slot) {
                    Some(index) => index,
                    None => {
                        meshes.push((slot, Mesh::default()));
                        meshes.len() - 1
                    }
                }
            };
            for mob in game.mobs.iter().filter(near) {
                let def = defs.get(mob.kind);
                let light = light_at(mob.eye());
                let slot = defs.texture_slot(mob.kind, mob.data.variant);
                let index = mesh_for(&mut meshes, slot);
                add_mob(&mut meshes[index].1, mob, def, alpha, light);
                for (i, layer) in def.layers.iter().enumerate() {
                    if (layer.wool && mob.data.sheared)
                        || (layer.saddle && !mob.data.saddled)
                        || layer.only_variant.is_some_and(|v| v != mob.data.variant)
                    {
                        continue;
                    }
                    let tint = if layer.dyed {
                        DYE_COLORS[usize::from(mob.data.color) % 16]
                    } else {
                        [1.0; 3]
                    };
                    let light = if layer.glow { (15, 15) } else { light };
                    let index = mesh_for(&mut meshes, defs.layer_slot(mob.kind, i));
                    add_mob_model(
                        &mut meshes[index].1,
                        mob,
                        def,
                        &layer.model,
                        alpha,
                        light,
                        tint,
                    );
                }
            }
            // The arrow texture follows the mob textures.
            let mut arrows = Mesh::default();
            for arrow in &game.arrows {
                add_arrow(&mut arrows, arrow, alpha, light_at(arrow.position));
            }
            let mob_kinds = game.mob_defs().texture_count();
            meshes.push((mob_kinds, arrows));
            // The minecart texture follows the arrow's, and the skin it.
            let mut carts = Mesh::default();
            for cart in &game.carts {
                add_minecart(
                    &mut carts,
                    cart,
                    alpha,
                    light_at(cart.position + DVec3::Y * 0.5),
                );
            }
            meshes.push((mob_kinds + 1, carts));
            // Invisible players aren't drawn (modern draws their armor).
            let invisible = game
                .stats
                .effects
                .has(crate::player::effects::Effect::Invisibility);
            meshes.push((
                mob_kinds + 2,
                if invisible {
                    Default::default()
                } else {
                    player.body
                },
            ));
            renderer.set_mobs(&meshes);
        }

        let chunks = &game.chunks;
        let stats = DebugStats {
            chunks_drawn: renderer.chunks_drawn(),
            chunks_loaded: chunks.world().len(),
            biome: chunks.world().biome_at_block(x, p.y.floor() as i32, z),
            seed: chunks.seed(),
            climate: chunks.terrain().map(|t| t.climate(x, z)),
            light: chunks.light_at(x, p.y.floor() as i32, z),
            mode: game.mode,
            time: game.time(),
            dimension: game.dimension(),
        };
        let hud = Hud {
            crosshair: self.cursor_grabbed,
            attack: self.cursor_grabbed.then(|| game.attack_indicator()),
            hotbar: (0..HOTBAR_SLOTS)
                .map(|i| {
                    let stack = game.inventory.slot(i)?;
                    let icons = self.icons.as_ref()?;
                    let drawn = (i == game.inventory.selected)
                        .then(|| game.bow_ticks())
                        .flatten();
                    Some(match drawn {
                        Some(ticks) => {
                            SlotView::with_icon(stack, icons.icon_drawn(stack, ticks), game.items())
                        }
                        None => SlotView::of(stack, icons, game.items()),
                    })
                })
                .collect(),
            offhand: game
                .inventory
                .slot(crate::inventory::player::OFFHAND)
                .and_then(|stack| Some(SlotView::of(stack, self.icons.as_ref()?, game.items()))),
            selected: game.inventory.selected,
            survival: (game.mode.is_vulnerable() && !game.stats.is_dead()).then(|| SurvivalHud {
                health: game.stats.health.ceil() as i32,
                food: game.stats.food,
                air: (game.stats.air < MAX_AIR).then_some(game.stats.air),
                level: game.stats.experience.level,
                progress: game.stats.experience.progress,
            }),
            death: game.stats.is_dead().then(|| {
                let source = game.stats.last_source.unwrap_or(DamageSource::Mob);
                format!("Player {}", source.death_message())
            }),
            sleep: game.sleep_fade(),
            message: game.message.as_ref().map(|(text, _)| text.clone()),
            portal: game.portal_swirl(),
            loading: game.is_arriving(),
            effects: game
                .stats
                .effects
                .0
                .iter()
                .map(|active| crate::ui::EffectView {
                    index: crate::player::effects::Effect::ALL
                        .iter()
                        .position(|&e| e == active.effect)
                        .unwrap_or(0),
                    color: active.effect.color(),
                    timer: active.timer(),
                    blinked: active.duration < 200 && (active.duration / 5) % 2 == 1,
                })
                .collect(),
        };
        let view_of = |stack: &ItemStack| {
            let icons = self.icons.as_ref()?;
            Some(SlotView::of(stack, icons, game.items()))
        };
        let slot = |slot: SlotRef| {
            let stack = game.slot_stack(slot)?;
            Some((view_of(&stack)?, game.items().tooltip(&stack)))
        };
        let view = ScreenView {
            slot: &slot,
            furnace: game.furnace_progress(),
            brewing: game.brewing_progress(),
            cursor: game.cursor.as_ref().and_then(view_of),
            catalog: &self.catalog,
            bench: bench_view(game),
            merchant: merchant_view(game, self.icons.as_deref()),
        };
        let screen = self.screen.as_mut().map(|screen| (screen, &view));
        let options_before = self.options.clone();
        let pause = self.pause.as_mut().map(|menu| (menu, &mut self.options));
        let (ui, actions) = debug_ui.frame(window, &game.camera, &stats, &hud, screen, pause);
        let result = renderer.render(&game.view_camera(), &game.environment(), ui);
        if capturing && let Some(run) = self.shot.as_mut() {
            run.captured = true;
        }
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
                if let Some(path) = bench.perf_report.clone() {
                    let work = self.game.as_ref().map(|game| {
                        let stats = game.chunks.work_stats();
                        (stats.generated.summary(), stats.meshed.summary())
                    });
                    let (generated, meshed) = work.unwrap_or(((0, 0.0), (0, 0.0)));
                    let run = bench::RunStats {
                        generated,
                        meshed,
                        memory_mib: bench::resident_memory_mib(),
                    };
                    let about = format!("Measured on {}; {place}.", bench::machine());
                    let text = bench.perf_markdown(render_distance, run, &about);
                    if let Err(err) = std::fs::create_dir_all(path.parent().unwrap_or(&path))
                        .and_then(|_| std::fs::write(&path, &text))
                    {
                        error!("writing {}: {err}", path.display());
                    } else {
                        println!("wrote {}", path.display());
                    }
                }
                event_loop.exit();
            }
        }
        if self.shot.as_ref().is_some_and(|run| run.captured) {
            println!(
                "{}",
                self.shot
                    .as_ref()
                    .map_or(String::new(), |run| run.out.display().to_string())
            );
            event_loop.exit();
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
                // The bench draws off screen when hidden.
                self.occluded = occluded && self.bench.is_none();
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

/// Every item, blocks first, for the creative inventory, with how it looks
/// and its name.
/// The trading screen's trades, if one is open.
fn merchant_view(
    game: &Game,
    icons: Option<&ItemIcons>,
) -> Option<crate::ui::screens::MerchantView> {
    use crate::{
        entity::villager::LEVEL_XP,
        ui::screens::{MerchantView, TradeView},
    };
    let villager = game.merchant()?;
    let icons = icons?;
    let items = game.items();
    let title = format!(
        "{} - {}",
        villager.profession.map_or("Villager", |p| p.name()),
        villager.level_name()
    );
    let level = usize::from(villager.level.clamp(1, 5));
    let progress = (level < 5).then(|| {
        let (from, to) = (LEVEL_XP[level - 1], LEVEL_XP[level]);
        (villager.xp.saturating_sub(from)) as f32 / (to - from) as f32
    });
    let trades = villager
        .trades
        .iter()
        .enumerate()
        .map(|(i, trade)| TradeView {
            buy: SlotView::of(&trade.buy, icons, items),
            buy_b: trade.buy_b.as_ref().map(|b| SlotView::of(b, icons, items)),
            sell: SlotView::of(&trade.sell, icons, items),
            name: items.tooltip(&trade.sell),
            uses_left: trade.max_uses.saturating_sub(trade.uses),
            affordable: game.can_trade(i),
        })
        .collect();
    Some(MerchantView {
        title,
        progress,
        trades,
    })
}

/// What the enchanting table, anvil or grindstone screen shows besides
/// its slots.
fn bench_view(game: &Game) -> Option<BenchView> {
    use crate::{
        inventory::{anvil::Refusal, enchant::roman},
        ui::screens::{AnvilStatus, OfferView},
    };
    let bench = game.bench()?;
    let free = game.mode.has_every_item();
    let level = game.stats.experience.level;
    let filled = bench.slots.iter().any(Option::is_some);
    let has_result = game.slot_stack(SlotRef::Result).is_some();
    let mut view = BenchView {
        lapis: game.lapis_count(),
        level,
        free,
        name: bench.name.clone(),
        has_item: bench.slots.first().is_some_and(Option::is_some),
        blocked: filled && !has_result,
        ..Default::default()
    };
    match bench.kind {
        BenchKind::Enchanting => {
            let offers = game.enchant_offers();
            for (shown, offer) in view.offers.iter_mut().zip(offers) {
                *shown = offer.map(|offer| OfferView {
                    requires: offer.requires,
                    clue: offer
                        .clue
                        .map_or_else(String::new, |(e, l)| format!("{} {}", e.name(), roman(l))),
                });
            }
        }
        BenchKind::Anvil => {
            view.anvil = match game.anvil_outcome() {
                Some(Ok(made)) => AnvilStatus::Cost {
                    levels: made.cost,
                    affordable: free || level >= made.cost,
                },
                Some(Err(Refusal::TooExpensive)) => AnvilStatus::TooExpensive,
                _ => AnvilStatus::Nothing,
            };
        }
        BenchKind::Grindstone | BenchKind::Smithing => {}
    }
    Some(view)
}

fn catalog(items: &Items, icons: &ItemIcons) -> Vec<(ItemStack, SlotView, String)> {
    // Every item, then an enchanted book for each enchantment at each of
    // its levels (the plain one would be useless), then the potions.
    let books = crate::inventory::enchant::ALL
        .iter()
        .flat_map(|&enchantment| {
            (1..=enchantment.max_level()).map(move |level| {
                let mut book = ItemStack::new(crate::inventory::items::ENCHANTED_BOOK, 1);
                book.enchant(enchantment, level);
                book
            })
        });
    // Every potion, drunk and thrown, in place of the plain potion items.
    use crate::inventory::{items as ids, potion};
    let potions = [ids::POTION, ids::SPLASH_POTION]
        .into_iter()
        .flat_map(|item| {
            (0..potion::POTIONS.len() as u16).map(move |n| ItemStack::with_damage(item, 1, n))
        });
    items
        .items
        .iter()
        .filter(|item| {
            !matches!(
                item.id,
                ids::ENCHANTED_BOOK | ids::POTION | ids::SPLASH_POTION
            )
        })
        .map(|item| ItemStack::with_damage(item.id, 1, 0))
        .chain(books)
        .chain(potions)
        .map(|stack| {
            let (view, name) = (SlotView::of(&stack, icons, items), items.tooltip(&stack));
            (stack, view, name)
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
