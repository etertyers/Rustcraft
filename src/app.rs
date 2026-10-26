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
    event::{
        DeviceEvent, DeviceId, ElementState, KeyEvent, MouseButton, MouseScrollDelta, StartCause,
        WindowEvent,
    },
    event_loop::{ActiveEventLoop, ControlFlow},
    keyboard::{KeyCode, PhysicalKey},
    window::{CursorGrabMode, Window, WindowId},
};

use crate::{
    assets,
    audio::{Audio, Listener},
    bench::{self, Bench, FrameTimes},
    bindings::{Action, Input},
    client::Client,
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
    pack::{Resources, model::ItemModels},
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
            EffectUvs, Shadow, add_cart_blocks, add_crystal_beams, add_dragon_fireballs,
            add_effects, add_eyes, add_falling_blocks, add_fireballs, add_gateway_beams, add_orbs,
            add_particles, add_primed_tnt, add_shadows, item_mesh,
        },
        held::HeldCache,
        icons::ItemIcons,
        models::{DYE_COLORS, add_arrow, add_minecart, add_mob, add_mob_model},
    },
    server::Server,
    shots,
    ui::{
        DebugStats, DebugUi, Hud,
        creative::CreativeTab,
        gui::{SlotView, SurvivalHud},
        menu::{Menu, MenuAction},
        pause::{PauseAction, PauseMenu},
        screens::{BenchView, Screen, ScreenAction, ScreenView},
        stack::{Layer, ScreenStack},
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
    /// A new world in a folder named after `name` (with `-` added if that
    /// is taken), with a random seed (`--creative` alone).
    New { name: String, mode: GameMode },
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
    /// The advancements screen (L), with the tab it shows.
    advancements: Option<usize>,
    /// The player's settings, from `options.ron`.
    options: Options,
    /// The sound engine, and the resource layers its sounds come from.
    audio: Option<Audio>,
    res: Option<Resources>,
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
    /// The creative inventory's tab buttons.
    tab_views: Vec<crate::ui::creative::TabView>,
    /// Every crafting recipe, for the recipe book.
    book_recipes: Arc<Vec<crate::inventory::book::BookRecipe>>,
    /// The icons on the recipe book's category tabs.
    book_tab_icons: Vec<SlotView>,
    /// The recipe the book shows the missing ingredients of in the grid.
    book_ghost: Option<usize>,
    /// The tab the creative inventory opens on: the last one used.
    creative_tab: CreativeTab,
    /// The screens open over the game; the one place that decides whether
    /// the camera has the mouse.
    stack: ScreenStack,
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
    /// Hide GUI (F1) is on.
    hide_gui: bool,
    /// The world being played, served in this process, and this client's
    /// link to it; `game` is the client's copy of the world.
    server: Option<Server>,
    link: Option<Client>,
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
            advancements: None,
            options: Options::load(&Options::path()),
            audio: None,
            res: None,
            pause: None,
            hide_gui: false,
            server: None,
            link: None,
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
            tab_views: Vec::new(),
            book_recipes: Arc::new(Vec::new()),
            book_tab_icons: Vec::new(),
            book_ghost: None,
            creative_tab: CreativeTab::Items(0),
            stack: ScreenStack::default(),
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
        renderer.set_clouds(self.options.clouds != crate::options::Clouds::Off);
        crate::ui::gui::set_gui_scale(self.options.gui_scale);
        crate::ui::glint::set_glint(self.options.glint_speed, self.options.glint_strength);
        self.debug_ui = Some(DebugUi::new(
            &window,
            look.atlas.image(),
            look.figure(),
            &look.res,
        )?);
        let recipes = Recipes::load(&assets.join("data/recipes.ron"))?;
        let items = Items::new().with_recipes(Arc::new(recipes));
        self.book_recipes = Arc::new(crate::inventory::book::all(&items.recipes, &items));
        self.items = Some(items);
        self.mob_defs = Some(mob_defs);
        self.install_look(look);
        match self.world_kind.clone() {
            Some(kind) => {
                let world = self.start_game(kind)?;
                self.play(world);
                // A click puts the mouse in the window.
                self.stack.set_in_world(true, false);
            }
            None => self.menu = Some(Menu::new(level::saves_dir())),
        }
        self.renderer = Some(renderer);
        window.request_redraw();
        self.window = Some(window);
        self.apply_fullscreen();
        Ok(())
    }

    fn start_game(&self, kind: WorldKind) -> Result<Game> {
        let mobs = self.mob_defs.clone().expect("loaded in init");
        let mut game = self.start_world(kind)?.with_mobs(mobs);
        if let Some(run) = &self.shot {
            let shot = run.shot;
            game.set_time_and_mode(
                shot.time.unwrap_or(game.time()),
                shot.mode.unwrap_or(game.player.mode),
            );
            if let Some((feet, yaw, pitch)) = shot.view {
                game.teleport(DVec3::from_array(feet), yaw, pitch);
            }
        }
        Ok(game)
    }

    /// Plays `world`: it's served in this process, with this window as its
    /// one client, which shows its own copy of the world.
    fn play(&mut self, world: Game) {
        let (items, models, mobs) = (
            self.items.clone().expect("loaded in init"),
            self.models.clone().expect("loaded in init"),
            self.mob_defs.clone().expect("loaded in init"),
        );
        let mut server = Server::new(world);
        let (server_end, client_end) = crate::net::transport::local_pair();
        server.connect(Box::new(server_end));
        let name = self.options.player_name.clone();
        let mut link = Client::join(Box::new(client_end), &name, self.player_settings());
        let mut game = Game::new_client(
            items,
            models,
            mobs,
            self.options.render_distance,
            chunk_workers(),
        );
        game.apply_options(&self.options);
        game.set_view_mode(self.start_view);
        // The welcome, the dimension and the player arrive before the first
        // frame.
        server.advance(0.0);
        link.receive(&mut game);
        self.server = Some(server);
        self.link = Some(link);
        self.game = Some(game);
    }

    /// The options the server plays this player by.
    fn player_settings(&self) -> crate::net::protocol::PlayerSettings {
        let o = &self.options;
        crate::net::protocol::PlayerSettings {
            view_distance: o.render_distance,
            auto_jump: o.auto_jump,
            toggle_sneak: o.toggle_sneak,
            toggle_sprint: o.toggle_sprint,
            left_handed: o.main_hand == crate::options::MainHand::Left,
            skin_layers: [
                o.skin.hat,
                o.skin.jacket,
                o.skin.right_sleeve,
                o.skin.left_sleeve,
                o.skin.right_pants,
                o.skin.left_pants,
            ],
            particles: o.particles,
        }
    }

    /// Sends the server `msg` (when playing through one).
    fn tell_server(&mut self, msg: crate::net::protocol::ClientMsg) {
        if let Some(link) = self.link.as_mut() {
            link.send(msg);
        }
    }

    /// A bound key's action: the view, the hotbar and movement happen here
    /// at once (the server hears of them too); everything else is the
    /// server's to do. A world played without a server (tests) does it all
    /// itself.
    fn act(&mut self, action: Action, pressed: bool, repeat: bool, grabbed: bool) {
        let Some(game) = self.game.as_mut() else {
            return;
        };
        if self.link.is_none() {
            return game.on_action(action, pressed, repeat, grabbed);
        }
        let local = matches!(
            action,
            Action::Forward
                | Action::Back
                | Action::Left
                | Action::Right
                | Action::Jump
                | Action::Sneak
                | Action::Sprint
        ) || action.hotbar_slot().is_some();
        if action == Action::Perspective {
            return game.on_action(action, pressed, repeat, grabbed);
        }
        if local {
            game.on_action(action, pressed, repeat, grabbed);
        }
        self.tell_server(crate::net::protocol::ClientMsg::Action {
            action,
            pressed,
            repeat,
            grabbed,
        });
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
            WorldKind::New { name, mode } => {
                let seed = level::random_seed();
                return Game::create(&level::saves_dir(), &name, seed, mode, items, models);
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
                game.player.camera.yaw = -std::f32::consts::FRAC_PI_2;
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

    /// Makes the window match the screen stack: the cursor is locked and
    /// hidden for mouse look when no screen is open, and free otherwise
    /// (centered, like Minecraft does when a screen opens).
    fn apply_grab(&mut self) {
        let Some(grab) = self.stack.pending() else {
            return;
        };
        if let Some(window) = &self.window {
            let result = if grab {
                lock_cursor(window)
            } else {
                window.set_cursor_grab(CursorGrabMode::None)
            };
            if let Err(err) = result {
                warn!("cursor grab failed: {err}");
                // Wait for the next click rather than failing every frame.
                self.stack.set_engaged(false);
                return;
            }
            window.set_cursor_visible(!grab);
            if !grab {
                Self::center_cursor(window, self.debug_ui.as_mut());
            }
        }
        self.stack.confirm(grab);
        if !grab && let Some(game) = self.game.as_mut() {
            game.release_input();
            self.tell_server(crate::net::protocol::ClientMsg::ReleaseInput);
        }
    }

    /// Puts the free cursor in the middle of the window, where egui's idea
    /// of the pointer is moved too (a locked cursor leaves it stale).
    fn center_cursor(window: &Window, debug_ui: Option<&mut DebugUi>) {
        let size = window.inner_size();
        let center = PhysicalPosition::new(size.width as f64 / 2.0, size.height as f64 / 2.0);
        if let Err(err) = window.set_cursor_position(center) {
            log::debug!("centering the cursor failed: {err}");
        }
        if let Some(debug_ui) = debug_ui {
            let scale = window.scale_factor() as f32;
            debug_ui.point_at(egui::pos2(center.x as f32 / scale, center.y as f32 / scale));
        }
    }

    /// A mouse button press or release in the window. A click with no
    /// screen open takes the mouse back after a pause or leaving the
    /// window; otherwise it reaches the player only while the camera has
    /// the mouse.
    fn on_mouse_button(&mut self, button: MouseButton, pressed: bool) {
        if let Some(screen) = options_screen(&mut self.pause, &mut self.menu) {
            if pressed && screen.waiting().is_some() {
                screen.bind(&mut self.options.keys, Some(Input::Mouse(button)));
                return self.options_changed();
            }
            if !pressed {
                screen.mouse_released();
            }
        }
        if pressed && button == MouseButton::Left && self.playing() && self.stack.click_in() {
            return self.apply_grab();
        }
        if self.stack.reaches_game() {
            self.on_input(Input::Mouse(button), pressed, false);
        }
    }

    fn on_scroll(&mut self, delta: MouseScrollDelta) {
        if self.stack.reaches_game()
            && let Some(game) = self.game.as_mut()
        {
            game.on_scroll(delta);
            let slot = game.player.inventory.selected;
            self.tell_server(crate::net::protocol::ClientMsg::SelectSlot(slot));
        }
    }

    /// Raw mouse motion: it turns the camera only while the camera has the
    /// mouse.
    fn on_mouse_motion(&mut self, dx: f64, dy: f64) {
        if self.stack.reaches_game()
            && let Some(game) = self.game.as_mut()
        {
            game.look(dx, dy);
        }
    }

    /// A `--shot` scene's inventory screen, opened once and kept with the
    /// pointer on the slot the scene names.
    fn show_staged_screen(&mut self) {
        use crate::game::ShotScreen;
        let world = self.server.as_ref().map(|s| &s.game).or(self.game.as_ref());
        let Some(staged) = world.and_then(Game::shot_ui) else {
            return;
        };
        if staged.screen == ShotScreen::KeyBinds {
            // A conflict (Sneak on W, in red) and Jump waiting for a key.
            if self.pause.is_none() {
                use crate::ui::options::{OptionsScreen, Page};
                let mut screen = OptionsScreen::new(Page::KeyBinds);
                screen.wait_for(Action::Jump);
                self.options
                    .keys
                    .set(Action::Sneak, Some(Input::Key(KeyCode::KeyW)));
                self.pause = Some(PauseMenu {
                    options: Some(screen),
                    packs: None,
                });
                self.stack.push(Layer::Pause);
                self.apply_grab();
            }
            return;
        }
        if self.screen.is_none() {
            if let ShotScreen::Creative(tab) = staged.screen {
                self.creative_tab = CreativeTab::Items(tab);
            }
            self.open_screen();
        }
        let (Some(window), Some(screen), Some(hover)) =
            (self.window.clone(), self.screen, staged.hover)
        else {
            return;
        };
        let ppp = window.scale_factor() as f32;
        let size = window.inner_size();
        let full = egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(size.width as f32 / ppp, size.height as f32 / ppp),
        );
        let open = self
            .game
            .as_ref()
            .is_some_and(|g| g.player.recipe_book.open);
        let shift = match screen {
            Screen::Inventory | Screen::Crafting => crate::ui::recipe_book::shift(open, full),
            _ => 0.0,
        };
        if let Some(at) = crate::ui::screens::slot_point(&screen, full, ppp, shift, hover)
            && let Some(debug_ui) = self.debug_ui.as_mut()
        {
            debug_ui.point_at(at);
        }
    }

    /// A text box has the keyboard: the anvil's name box, the creative
    /// inventory's search box on its Search tab, or a menu's text box.
    fn types_into_a_box(&self) -> bool {
        if self
            .debug_ui
            .as_ref()
            .is_some_and(|ui| ui.context().text_edit_focused())
        {
            return true;
        }
        let book_search = matches!(self.screen, Some(Screen::Inventory | Screen::Crafting))
            && self
                .game
                .as_ref()
                .is_some_and(|g| g.player.recipe_book.open)
            && self
                .debug_ui
                .as_ref()
                .is_some_and(|ui| crate::ui::recipe_book::search_focused(ui.context()));
        book_search
            || matches!(
                self.screen,
                Some(Screen::Anvil)
                    | Some(Screen::Creative {
                        tab: CreativeTab::Search,
                        ..
                    })
            )
    }

    fn open_advancements(&mut self) {
        self.advancements = Some(0);
        self.stack.push(Layer::Advancements);
        self.apply_grab();
    }

    fn close_advancements(&mut self) {
        self.advancements = None;
        self.stack.remove(Layer::Advancements);
        self.apply_grab();
    }

    /// Shows an inventory or container screen.
    fn show_container(&mut self, screen: Screen) {
        if matches!(screen, Screen::Inventory | Screen::Crafting)
            && let Some(debug_ui) = self.debug_ui.as_ref()
        {
            crate::ui::recipe_book::reset(debug_ui.context());
        }
        self.screen = Some(screen);
        self.stack.push(Layer::Container);
        self.apply_grab();
    }

    fn hide_container(&mut self) {
        self.book_ghost = None;
        // The creative inventory opens on the tab it was left on.
        if let Some(Screen::Creative { tab, .. }) = self.screen {
            self.creative_tab = tab;
        }
        self.screen = None;
        self.stack.remove(Layer::Container);
        self.apply_grab();
    }

    fn on_key(&mut self, event: &KeyEvent) {
        let PhysicalKey::Code(key) = event.physical_key else {
            return;
        };
        let pressed = event.state == ElementState::Pressed;
        self.on_input(Input::Key(key), pressed, event.repeat);
    }

    /// A key press or release, or a mouse button's while the camera has
    /// the mouse. Escape is always the menu key, as in vanilla; everything
    /// else goes through the key bindings.
    fn on_input(&mut self, input: Input, pressed: bool, repeat: bool) {
        let actions: Vec<Action> = self.options.keys.actions(input).collect();
        let is = |action| actions.contains(&action);
        let escape = input == Input::Key(KeyCode::Escape);
        let press = pressed && !repeat;
        // The Key Binds screen is waiting for a key: this is it, or Escape
        // to unbind.
        if press
            && let Some(screen) = options_screen(&mut self.pause, &mut self.menu)
            && screen.waiting().is_some()
        {
            screen.bind(&mut self.options.keys, (!escape).then_some(input));
            return self.options_changed();
        }
        let sleeping = self.game.as_ref().is_some_and(Game::is_sleeping);
        let rolling = self
            .game
            .as_ref()
            .is_some_and(|g| g.credits_ticks().is_some());
        if escape
            && press
            && let Some(menu) = self.menu.as_mut()
        {
            menu.back();
        } else if self.advancements.is_some() && press && (escape || is(Action::Advancements)) {
            self.close_advancements();
        } else if self.advancements.is_some() {
            // Nothing else works while it is open.
        } else if is(Action::Advancements)
            && press
            && self.playing()
            && self.stack.is_empty()
            && !rolling
        {
            self.open_advancements();
        } else if escape && press && rolling {
            // Skipping the poem and credits goes home.
            if self.link.is_some() {
                self.tell_server(crate::net::protocol::ClientMsg::FinishCredits);
            } else if let Some(game) = self.game.as_mut() {
                game.finish_credits();
            }
        } else if rolling {
            // Nothing else works while they roll.
        } else if escape && pressed && sleeping {
            self.on_screen_action(ScreenAction::LeaveBed);
        } else if escape && pressed && self.screen.is_some() {
            self.close_screen();
        } else if escape && press && self.pause.is_some() {
            if self.pause.as_mut().and_then(PauseMenu::back) == Some(PauseAction::Resume) {
                self.resume();
            }
        } else if escape && press && self.game.is_some() {
            self.pause_game();
        } else if self.types_into_a_box() && !is(Action::DebugOverlay) {
            // The anvil's name box or the creative search box has the
            // keyboard (E and the digits are letters).
        } else if is(Action::Screenshot) && press {
            self.take_screenshot();
        } else if is(Action::Fullscreen) && press {
            self.options.fullscreen = !self.options.fullscreen;
            self.options_changed();
        } else if is(Action::HideGui) && press && self.game.is_some() {
            self.hide_gui = !self.hide_gui;
        } else if is(Action::Inventory)
            && press
            && self.game.is_some()
            && self.pause.is_none()
            && !sleeping
        {
            if self.screen.is_some() {
                self.close_screen();
            } else {
                self.open_screen();
            }
        } else if is(Action::DebugOverlay) && press {
            if let Some(debug_ui) = self.debug_ui.as_mut() {
                debug_ui.toggle();
            }
        } else if self.pause.is_none()
            && let Some(game) = self.game.as_mut()
        {
            let _ = game;
            let grabbed = self.stack.reaches_game();
            for action in actions {
                self.act(action, pressed, repeat, grabbed);
            }
        }
    }

    /// Opens the pause menu over a world being played (not over a screen,
    /// the death screen or a bed), freeing the mouse. The game stops.
    fn pause_game(&mut self) {
        // The bench keeps flying when the window loses focus, and a scene
        // being captured never pauses.
        if self.bench.is_none() && self.shot.is_none() && self.playing() && self.stack.is_empty() {
            self.pause = Some(PauseMenu::default());
            self.stack.push(Layer::Pause);
            self.apply_grab();
        }
    }

    fn resume(&mut self) {
        self.pause = None;
        self.stack.remove(Layer::Pause);
        self.apply_grab();
    }

    /// Saves the world and goes back to the title screen.
    fn quit_to_title(&mut self) {
        self.tell_server(crate::net::protocol::ClientMsg::Disconnect);
        if let Some(mut server) = self.server.take()
            && let Err(err) = server.game.save()
        {
            error!("saving on quitting to the title failed: {err:#}");
        }
        self.link = None;
        self.game = None;
        if let Some(renderer) = self.renderer.as_mut() {
            renderer.clear_world();
        }
        self.pause = None;
        self.screen = None;
        self.advancements = None;
        self.menu = Some(Menu::new(level::saves_dir()));
        self.stack.set_in_world(false, false);
        self.apply_grab();
    }

    /// Keeps a newly built look's models, icons and atlas.
    fn install_look(&mut self, look: Look) {
        if let Some(items) = &self.items {
            self.catalog = crate::ui::creative::catalog(items, &look.icons);
            self.tab_views = crate::ui::creative::tab_views(items, &look.icons, &self.catalog);
            use crate::inventory::items as ids;
            self.book_tab_icons = [
                ids::COMPASS,
                ids::IRON_SWORD,
                ids::BRICK,
                ids::LAVA_BUCKET,
                ids::REDSTONE,
            ]
            .map(|id| SlotView::of(&ItemStack::new(id, 1), &look.icons, items))
            .to_vec();
        }
        self.icons = Some(Arc::new(look.icons));
        self.models = Some(look.models);
        self.crack_uvs = look.crack_uvs;
        self.effect_uvs = look.effect_uvs;
        self.atlas = Some(look.atlas);
        self.item_models = Some(look.item_models);
        self.skin_slim = look.skin_slim;
        self.held.clear();
        match self.audio.as_mut() {
            Some(audio) => audio.reload(&look.res),
            None => self.audio = Some(Audio::new(&look.res)),
        }
        self.res = Some(look.res);
    }

    /// Plays the sounds the game and menus queued, and keeps the music going.
    fn update_audio(&mut self, dt: f32) {
        let (Some(audio), Some(res)) = (self.audio.as_mut(), self.res.as_ref()) else {
            return;
        };
        let (listener, music) = match &self.game {
            Some(game) => {
                let forward = game.player.camera.forward();
                let right = glam::DVec3::new(-f64::from(forward.z), 0.0, f64::from(forward.x))
                    .normalize_or_zero();
                (
                    Listener {
                        position: game.player.camera.position.as_dvec3(),
                        right,
                    },
                    game.music_event(),
                )
            }
            None => (
                Listener {
                    position: glam::DVec3::ZERO,
                    right: glam::DVec3::X,
                },
                Some("music.menu"),
            ),
        };
        audio.update(res, &listener, self.options.volume, &self.options.volumes);
        audio.update_music(res, music, dt, self.options.volumes.music);
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
            && let Err(err) = debug_ui.set_textures(look.atlas.image(), look.figure(), &look.res)
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

    /// F2: saves the next frame in `screenshots/` beside the saves, named
    /// for when it was taken like vanilla's (in UTC).
    fn take_screenshot(&mut self) {
        let Some(renderer) = self.renderer.as_mut() else {
            return;
        };
        let name = format!("{}.png", screenshot_stamp(std::time::SystemTime::now()));
        let dir = level::saves_dir().with_file_name("screenshots");
        renderer.capture_next_frame(dir.join(&name));
        info!("saving screenshot {name}");
        if let Some(game) = self.game.as_mut() {
            game.player.message = Some((
                format!("Saved screenshot as {name}"),
                SCREENSHOT_MESSAGE_TICKS,
            ));
        }
    }

    /// Fullscreen (F11 or Video Settings), borderless on the window's
    /// screen. `--bench` and `--shot` keep their window.
    fn apply_fullscreen(&self) {
        let Some(window) = &self.window else {
            return;
        };
        let wanted = self.options.fullscreen && self.bench.is_none() && self.shot.is_none();
        if wanted != window.fullscreen().is_some() {
            window.set_fullscreen(wanted.then_some(winit::window::Fullscreen::Borderless(None)));
        }
    }

    /// When Max Framerate lets the next frame start, if it holds frames
    /// back (never for `--bench` and `--shot`).
    fn next_frame_at(&self) -> Option<Instant> {
        let limit = self.options.max_framerate;
        if limit >= crate::options::UNLIMITED_FRAMERATE
            || self.bench.is_some()
            || self.shot.is_some()
        {
            return None;
        }
        Some(self.last_frame + Duration::from_secs_f64(1.0 / f64::from(limit)))
    }

    /// Applies settings changed on the Options screen and saves them.
    fn options_changed(&mut self) {
        if let Some(renderer) = self.renderer.as_mut() {
            renderer.set_vsync(self.options.vsync);
            renderer.set_clouds(self.options.clouds != crate::options::Clouds::Off);
        }
        crate::ui::gui::set_gui_scale(self.options.gui_scale);
        crate::ui::glint::set_glint(self.options.glint_speed, self.options.glint_strength);
        self.apply_fullscreen();
        if self.options.resource_packs != self.applied_packs {
            self.apply_resource_packs();
        }
        if let Some(game) = self.game.as_mut() {
            game.apply_options(&self.options);
        }
        let settings = self.player_settings();
        self.tell_server(crate::net::protocol::ClientMsg::Settings(settings));
        // `--bench` and `--shot` runs leave the player's settings alone.
        if self.bench.is_some() || self.shot.is_some() {
            return;
        }
        let path = Options::path();
        if let Err(err) = self.options.save(&path) {
            error!("{err:#}");
        }
    }

    /// In a world and alive.
    fn playing(&self) -> bool {
        self.game
            .as_ref()
            .is_some_and(|game| !game.player.stats.is_dead())
    }

    /// Opens the inventory (or creative item list) and frees the mouse.
    fn open_screen(&mut self) {
        let Some(game) = self
            .game
            .as_ref()
            .filter(|game| !game.player.stats.is_dead())
        else {
            return;
        };
        let screen = if game.player.mode.has_every_item() {
            Screen::Creative {
                tab: self.creative_tab,
                scroll: 0.0,
            }
        } else {
            Screen::Inventory
        };
        if self.link.is_some() {
            self.tell_server(crate::net::protocol::ClientMsg::OpenInventory);
        } else if let Some(game) = self.game.as_mut() {
            game.open_inventory();
        }
        if matches!(screen, Screen::Creative { .. })
            && let Some(debug_ui) = self.debug_ui.as_ref()
        {
            crate::ui::creative::reset_search(debug_ui.context());
        }
        self.show_container(screen);
    }

    /// Closes the screen, throwing out whatever was held on the mouse or
    /// left in a crafting grid (1.0.0).
    fn close_screen(&mut self) {
        if self.link.is_some() {
            self.tell_server(crate::net::protocol::ClientMsg::CloseScreen);
            // Closed here at once, so the screen doesn't come back before
            // the server's next word.
            if let Some(game) = self.game.as_mut() {
                game.player.container = None;
            }
        } else if let Some(game) = self.game.as_mut() {
            game.close_container();
        }
        self.hide_container();
    }

    /// Shows the screen for a container the game opened (a right-clicked
    /// crafting table, chest or furnace).
    fn show_opened_container(&mut self) {
        if self.screen.is_some() {
            return;
        }
        let Some(container) = self
            .game
            .as_ref()
            .and_then(|game| game.player.container.as_ref())
        else {
            return;
        };
        let screen = match container {
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
                BenchKind::Stonecutter => Screen::Stonecutter,
            },
            Container::Boat(_) => Screen::Chest,
            Container::Cart(index) => match self.game.as_ref().and_then(|g| g.carts.get(*index)) {
                Some(cart) if cart.kind == crate::entity::minecart::CartKind::Hopper => {
                    Screen::Hopper
                }
                _ => Screen::Chest,
            },
        };
        self.show_container(screen);
    }

    fn on_screen_action(&mut self, action: ScreenAction) {
        use crate::ui::recipe_book::BookAction;
        match &action {
            ScreenAction::AdvTab(tab) => return self.advancements = Some(*tab),
            ScreenAction::Pause(PauseAction::Resume) => return self.resume(),
            ScreenAction::Pause(PauseAction::SaveAndQuit) => return self.quit_to_title(),
            ScreenAction::Book(BookAction::Ghost(index)) => return self.book_ghost = Some(*index),
            ScreenAction::Book(BookAction::Open(_) | BookAction::Place { .. }) => {
                self.book_ghost = None;
            }
            _ => {}
        }
        if self.link.is_some() {
            self.tell_server(crate::net::protocol::ClientMsg::Screen(action));
        } else if let Some(game) = self.game.as_mut() {
            game.apply_screen_action(action, &self.book_recipes.clone());
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
            Ok(world) => {
                self.play(world);
                self.menu = None;
                self.stack.set_in_world(true, true);
                self.apply_grab();
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
        self.update_audio(dt);
        if self.game.is_none() {
            return self.redraw_menu(event_loop, now);
        }
        self.show_opened_container();
        self.show_staged_screen();
        // Lying in bed frees the mouse for the Leave Bed button; dying
        // closes any screen and frees it for the Respawn button.
        let sleeping = self.game.as_ref().is_some_and(Game::is_sleeping);
        self.stack.set(Layer::Bed, sleeping);
        if !self.playing() {
            self.screen = None;
            self.stack.remove(Layer::Container);
        }
        self.stack.set(Layer::Death, !self.playing());
        self.apply_grab();
        let (Some(window), Some(renderer), Some(debug_ui), Some(game)) = (
            &self.window,
            self.renderer.as_mut(),
            self.debug_ui.as_mut(),
            self.game.as_mut(),
        ) else {
            return;
        };
        // The bench flies the world's player; this client follows.
        if let (Some(bench), Some(server)) = (&self.bench, self.server.as_mut()) {
            let world = &mut server.game;
            if let Some(start) = bench.start().filter(|_| world.time() == 0) {
                world.teleport(glam::DVec3::from_array(start), 0.0, -0.35);
            }
            world.glide(bench::SPEED * dt as f64, bench.height());
            // This client looks where the flight does.
            game.player.camera.yaw = world.player.camera.yaw;
            game.player.camera.pitch = world.player.camera.pitch;
        }
        // The pause menu stops the game; the camera still follows it. A
        // scene being captured stands still.
        let frozen = self.pause.is_some() || self.shot.is_some();
        let step = if frozen { 0.0 } else { dt };
        match (self.link.as_mut(), self.server.as_mut()) {
            (Some(link), Some(server)) => {
                link.advance(game, step);
                server.advance(step);
                link.receive(game);
            }
            _ => game.advance(step),
        }
        // Another dimension's chunks replace this one's.
        if game.take_dimension_change() {
            renderer.clear_world();
        }
        let ticked = Instant::now();

        let p = game.player.camera.position;
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
                    match self.server.as_mut() {
                        Some(server) => (run.shot.stage)(&mut server.game),
                        None => (run.shot.stage)(game),
                    }
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
        renderer.set_outline(game.player.target.and_then(|hit| {
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
                add_eyes(
                    &mut mesh,
                    &game.eyes,
                    icons,
                    &view_camera,
                    game.tick_alpha(),
                );
                add_dragon_fireballs(
                    &mut mesh,
                    &game.dragon_fireballs,
                    &self.effect_uvs,
                    &view_camera,
                    game.tick_alpha(),
                );
                crate::render::entities::add_shulker_bullets(
                    &mut mesh,
                    &game.bullets,
                    &self.effect_uvs,
                    &view_camera,
                    game.tick_alpha(),
                );
                add_crystal_beams(
                    &mut mesh,
                    &game.crystals,
                    &self.effect_uvs,
                    &view_camera,
                    game.tick_alpha(),
                );
                add_gateway_beams(&mut mesh, &game.gateways(), &self.effect_uvs, &view_camera);
                if let Some(dragon) = &game.dragon {
                    crate::render::entities::add_death_beams(
                        &mut mesh,
                        dragon,
                        &self.effect_uvs,
                        &view_camera,
                        game.tick_alpha(),
                    );
                }
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
                    let hand_light = light_at(game.player.camera.position.as_dvec3());
                    figure::build(game, &mut self.held, &fig, body_light, hand_light)
                }
                _ => PlayerMeshes::default(),
            };
            mesh.append(&player.items);
            if let (Some(atlas), Some(item_models)) = (&self.atlas, &self.item_models) {
                let fig = Figure {
                    icons,
                    item_models,
                    atlas: atlas.image(),
                    slim: self.skin_slim,
                };
                let eye = game.player.camera.position.as_dvec3();
                figure::add_mob_items(
                    &mut mesh,
                    game,
                    &mut self.held,
                    &fig,
                    game.tick_alpha() as f32,
                    |mob| mob.eye().distance_squared(eye) < MOB_DRAW_DISTANCE * MOB_DRAW_DISTANCE,
                    light_at,
                );
            }
            if self.options.entity_shadows
                && let Some(models) = &self.models
            {
                let alpha = game.tick_alpha();
                let eye = game.player.camera.position.as_dvec3();
                let mut shadows: Vec<Shadow> = game
                    .mobs
                    .iter()
                    .filter(|mob| mob.eye().distance_squared(eye) < 256.0)
                    .map(|mob| Shadow {
                        feet: mob.pose(alpha as f32).0,
                        radius: 0.05 + 0.75 * mob.size().0,
                        strength: 1.0,
                    })
                    .chain(game.entities.iter().map(|item| Shadow {
                        feet: item.center_between_ticks(alpha) - DVec3::Y * 0.125,
                        radius: 0.15,
                        strength: 0.75,
                    }))
                    .collect();
                if game.view_mode() != crate::game::view::ViewMode::First {
                    shadows.push(Shadow {
                        feet: game.player_body().0.as_dvec3(),
                        radius: 0.5,
                        strength: 1.0,
                    });
                }
                let world = chunks.world();
                add_shadows(
                    &mut mesh,
                    &shadows,
                    self.effect_uvs.shadow,
                    eye,
                    |cell| {
                        world
                            .block(cell.x, cell.y, cell.z)
                            .is_some_and(|b| !b.is_air() && models.get(b).opaque_cube)
                    },
                    |cell| chunks.light_at(cell.x, cell.y, cell.z).unwrap_or((15, 0)),
                );
            }
            renderer.set_entities(&mesh);
            if self.hide_gui {
                renderer.set_hand(None, None);
            } else {
                renderer.set_hand(Some(&player.hand_arm), Some(&player.hand_item));
            }
            let alpha = game.tick_alpha() as f32;
            let mut meshes: Vec<(usize, Mesh)> = Vec::new();
            let eye = game.player.camera.position.as_dvec3();
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
                        || layer.only_marking.is_some_and(|m| m != mob.data.markings)
                        || layer.only_armor.is_some_and(|a| a != mob.data.armor)
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
            // The minecart texture follows the arrow's, then the dragon's and
            // the crystal's, and the skin them.
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
            // Boats, a mesh for each wood's boat and chest boat.
            let mut boats: Vec<Mesh> = (0..crate::look::BOAT_TEXTURES)
                .map(|_| Mesh::default())
                .collect();
            for boat in game.boats_here() {
                crate::render::boat::add_boat(
                    &mut boats[crate::look::boat_texture(boat.kind)],
                    boat,
                    alpha,
                    light_at(boat.position + DVec3::Y * 0.5),
                );
            }
            for (i, mesh) in boats.into_iter().enumerate() {
                meshes.push((mob_kinds + crate::look::EXTRA_BOATS + i, mesh));
            }
            // The dragon and the crystals, in the End.
            let mut dragon = Mesh::default();
            let mut crystals = Mesh::default();
            if game.dimension() == crate::world::dimension::Dimension::End {
                if let Some(d) = &game.dragon {
                    crate::render::dragon::add_dragon(&mut dragon, d, alpha, light_at(d.position));
                }
                for crystal in &game.crystals {
                    let light = light_at(crystal.center());
                    crate::render::dragon::add_crystal(&mut crystals, crystal, alpha, light);
                }
            }
            meshes.push((mob_kinds + 2, dragon));
            meshes.push((mob_kinds + 3, crystals));
            meshes.push((mob_kinds + 4, player.elytra.clone()));
            meshes.push((mob_kinds + crate::look::EXTRA_ARMOR, player.armor.clone()));
            // Invisible players aren't drawn, but their armor is (modern).
            let invisible = game
                .player
                .stats
                .effects
                .has(crate::player::effects::Effect::Invisibility);
            meshes.push((
                mob_kinds + crate::look::EXTRA_SKIN,
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
            mode: game.player.mode,
            time: game.time(),
            dimension: game.dimension(),
        };
        let hud = Hud {
            hidden: self.hide_gui,
            crosshair: self.stack.grabbed(),
            attack: self.stack.grabbed().then(|| game.attack_indicator()),
            hotbar: (0..HOTBAR_SLOTS)
                .map(|i| {
                    let stack = game.player.inventory.slot(i)?;
                    let icons = self.icons.as_ref()?;
                    let drawn = (i == game.player.inventory.selected)
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
                .player
                .inventory
                .slot(crate::inventory::player::OFFHAND)
                .and_then(|stack| Some(SlotView::of(stack, self.icons.as_ref()?, game.items()))),
            selected: game.player.inventory.selected,
            survival: (game.player.mode.is_vulnerable() && !game.player.stats.is_dead()).then(
                || SurvivalHud {
                    health: game.player.stats.health.ceil() as i32,
                    food: game.player.stats.food,
                    air: (game.player.stats.air < MAX_AIR).then_some(game.player.stats.air),
                    level: game.player.stats.experience.level,
                    progress: game.player.stats.experience.progress,
                },
            ),
            death: game.player.stats.is_dead().then(|| {
                let source = game.player.stats.last_source.unwrap_or(DamageSource::Mob);
                format!("Player {}", source.death_message())
            }),
            sleep: game.sleep_fade(),
            message: game.player.message.as_ref().map(|(text, _)| text.clone()),
            portal: game.portal_swirl(),
            loading: game.is_arriving(),
            boss: game.dragon_bar(),
            credits: game.credits_ticks(),
            toasts: toast_views(game, self.icons.as_deref()),
            advancements: self
                .advancements
                .or(self
                    .server
                    .as_ref()
                    .map_or(game.shot_advancements(), |s| s.game.shot_advancements()))
                .and_then(|tab| advancements_view(game, self.icons.as_deref(), tab)),
            effects: game
                .player
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
        let held = std::cell::RefCell::new(&mut self.held);
        let doll = |angles| match (&self.atlas, &self.item_models, self.icons.as_deref()) {
            (Some(atlas), Some(item_models), Some(icons)) => {
                let fig = Figure {
                    icons,
                    item_models,
                    atlas: atlas.image(),
                    slim: self.skin_slim,
                };
                crate::ui::doll::build(game, &mut held.borrow_mut(), &fig, angles)
            }
            _ => crate::ui::doll::Doll::default(),
        };
        // The recipe book's entries, built only while it is open.
        let (entries, ghost) = match (&self.screen, self.icons.as_deref()) {
            (Some(Screen::Inventory | Screen::Crafting), Some(icons))
                if game.player.recipe_book.open =>
            {
                let grid = game.crafting_grid().unwrap_or(&[]);
                let items = game.items();
                let entries: Vec<_> = self
                    .book_recipes
                    .iter()
                    .enumerate()
                    .map(|(index, recipe)| {
                        let stack = ItemStack::new(recipe.result.item, 1);
                        crate::ui::recipe_book::RecipeEntry {
                            index,
                            icon: SlotView::of(&stack, icons, items),
                            name: items.name(&stack),
                            craftable: recipe.craftable(&game.player.inventory, grid),
                            category: recipe.category,
                            fits_small: recipe.fits(2),
                        }
                    })
                    .collect();
                let ghost = self
                    .book_ghost
                    .and_then(|index| self.book_recipes.get(index))
                    .map(|recipe| crate::ui::recipe_book::Ghost {
                        cells: recipe
                            .placements(game.crafting_size().unwrap_or(2))
                            .into_iter()
                            .map(|(cell, ingredient)| {
                                let choices = ingredient
                                    .items()
                                    .iter()
                                    .map(|&id| SlotView::of(&ItemStack::new(id, 1), icons, items))
                                    .collect();
                                (cell, choices)
                            })
                            .collect(),
                    });
                (entries, ghost)
            }
            _ => (Vec::new(), None),
        };
        let book = matches!(self.screen, Some(Screen::Inventory | Screen::Crafting)).then(|| {
            crate::ui::recipe_book::BookView {
                settings: &game.player.recipe_book,
                entries: &entries,
                ghost: ghost.as_ref(),
                tab_icons: &self.book_tab_icons,
            }
        });
        let view = ScreenView {
            slot: &slot,
            furnace: game.furnace_progress(),
            brewing: game.brewing_progress(),
            cursor: game.player.cursor.as_ref().and_then(view_of),
            catalog: &self.catalog,
            tabs: &self.tab_views,
            bench: bench_view(game, self.icons.as_deref()),
            merchant: merchant_view(game, self.icons.as_deref()),
            doll: Some(&doll),
            book,
        };
        let screen = self.screen.as_mut().map(|screen| (screen, &view));
        let options_before = self.options.clone();
        let mut difficulty = game.difficulty;
        let pause = self
            .pause
            .as_mut()
            .map(|menu| (menu, &mut self.options, &mut difficulty));
        let (ui, actions) =
            debug_ui.frame(window, &game.player.camera, &stats, &hud, screen, pause);
        game.difficulty = difficulty;
        let mut environment = game.environment();
        environment.brightness = self.options.brightness;
        environment.glint = crate::ui::glint::glint_scale().into();
        let result = renderer.render(&game.view_camera(), &environment, ui);
        if capturing && let Some(run) = self.shot.as_mut() {
            run.captured = true;
        }
        let render_distance = game.chunks.radius();
        let place = format!(
            "ended at {:.0}, {} chunks loaded, {} drawn",
            game.player.camera.position,
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
                        // Generation is the server's work, meshing this
                        // client's.
                        let generated = self
                            .server
                            .as_ref()
                            .map_or(game.chunks.work_stats(), |s| s.game.chunks.work_stats());
                        let stats = game.chunks.work_stats();
                        (generated.generated.summary(), stats.meshed.summary())
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
                self.stack.set_engaged(false);
                self.pause_game();
                self.apply_grab();
            }
            WindowEvent::Occluded(occluded) => {
                // The bench draws off screen when hidden.
                self.occluded = occluded && self.bench.is_none();
                if !occluded && let Some(window) = &self.window {
                    window.request_redraw();
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                self.on_mouse_button(button, state == ElementState::Pressed);
            }
            WindowEvent::MouseWheel { delta, .. } => self.on_scroll(delta),
            WindowEvent::CursorMoved { position, .. } if self.stack.reaches_game() => {
                self.recenter_cursor(position);
            }
            WindowEvent::KeyboardInput { event, .. } => self.on_key(&event),
            WindowEvent::RedrawRequested => self.redraw(event_loop),
            _ => {}
        }
    }

    fn device_event(&mut self, _event_loop: &ActiveEventLoop, _id: DeviceId, event: DeviceEvent) {
        if let DeviceEvent::MouseMotion { delta: (dx, dy) } = event {
            self.on_mouse_motion(dx, dy);
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if self.occluded {
            event_loop.set_control_flow(ControlFlow::Wait);
        } else if self.frame_skipped {
            // new_events requests the redraw when the wait ends.
            event_loop
                .set_control_flow(ControlFlow::WaitUntil(Instant::now() + SKIPPED_FRAME_RETRY));
        } else if let Some(at) = self.next_frame_at().filter(|&at| Instant::now() < at) {
            // new_events requests the redraw when the wait ends.
            event_loop.set_control_flow(ControlFlow::WaitUntil(at));
        } else if let Some(window) = &self.window {
            event_loop.set_control_flow(ControlFlow::Wait);
            window.request_redraw();
        }
    }

    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(server) = self.server.as_mut()
            && let Err(err) = server.game.save()
        {
            error!("saving on quit failed: {err:#}");
        }
        // Drop the GPU surface before the window it points to.
        self.debug_ui = None;
        self.renderer = None;
        // Joins the chunk workers.
        self.game = None;
        self.link = None;
        self.server = None;
        self.window = None;
    }
}

/// How long "Saved screenshot as ..." shows.
const SCREENSHOT_MESSAGE_TICKS: u32 = 60;

/// Vanilla's screenshot name for a moment, `2026-10-25_16.25.42`.
fn screenshot_stamp(at: std::time::SystemTime) -> String {
    let secs = at
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let (days, rest) = ((secs / 86_400) as i64, secs % 86_400);
    // Howard Hinnant's civil_from_days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}_{:02}.{:02}.{:02}",
        rest / 3600,
        rest / 60 % 60,
        rest % 60
    )
}

/// The Options screen open in the pause menu or on the title screen.
fn options_screen<'a>(
    pause: &'a mut Option<PauseMenu>,
    menu: &'a mut Option<Menu>,
) -> Option<&'a mut crate::ui::options::OptionsScreen> {
    match pause {
        Some(pause) => pause.options.as_mut(),
        None => menu.as_mut()?.options_screen(),
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
fn bench_view(game: &Game, icons: Option<&ItemIcons>) -> Option<BenchView> {
    use crate::{
        inventory::{anvil::Refusal, enchant::roman},
        ui::screens::{AnvilStatus, OfferView},
    };
    let bench = game.bench()?;
    let free = game.player.mode.has_every_item();
    let level = game.player.stats.experience.level;
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
        BenchKind::Stonecutter => {
            if let Some(icons) = icons {
                let items = game.items();
                view.choice = bench.choice;
                view.cuts = game
                    .stonecutter_cuts()
                    .iter()
                    .map(|cut| {
                        let stack = cut.stack();
                        (SlotView::of(&stack, icons, items), items.tooltip(&stack))
                    })
                    .collect();
            }
        }
        BenchKind::Grindstone | BenchKind::Smithing => {}
    }
    Some(view)
}

/// Pins the cursor in place. macOS supports only Locked; other platforms may
/// only support Confined.
fn lock_cursor(window: &Window) -> Result<(), ExternalError> {
    window
        .set_cursor_grab(CursorGrabMode::Locked)
        .or_else(|_| window.set_cursor_grab(CursorGrabMode::Confined))
}

/// The icon of an item or block called `name`.
fn named_icon(name: &str, game: &Game, icons: &ItemIcons) -> Option<SlotView> {
    let id = crate::inventory::items::by_name(name)?;
    Some(SlotView::of(&ItemStack::new(id, 1), icons, game.items()))
}

/// The toasts on screen, sliding in and out at the edges.
fn toast_views(game: &Game, icons: Option<&ItemIcons>) -> Vec<crate::ui::ToastView> {
    let Some(icons) = icons else {
        return Vec::new();
    };
    let registry = crate::advancements::registry();
    let total = crate::game::advance::TOAST_TICKS as f32;
    game.toasts()
        .iter()
        .map(|toast| {
            let def = &registry.defs[toast.index];
            let age = total - toast.ticks as f32;
            let slide = (age / 8.0).min(toast.ticks as f32 / 8.0).clamp(0.0, 1.0);
            crate::ui::ToastView {
                header: def.frame.toast(),
                title: def.title.clone(),
                icon: named_icon(&def.icon, game, icons),
                frame: def.frame,
                slide,
            }
        })
        .collect()
}

/// The advancements screen's tabs and the tree of `tab`.
fn advancements_view(
    game: &Game,
    icons: Option<&ItemIcons>,
    tab: usize,
) -> Option<crate::ui::AdvView> {
    use crate::{
        advancements::Tab,
        ui::{AdvView, NodeView, TabView},
    };
    let icons = icons?;
    let registry = crate::advancements::registry();
    let progress = game.progress();
    let tabs = Tab::ALL
        .iter()
        .map(|&t| {
            let (done, total) = progress.tally(registry, t);
            TabView {
                title: t.title(),
                icon: named_icon(t.icon(), game, icons)
                    .or_else(|| named_icon("compass", game, icons)),
                done,
                total,
            }
        })
        .collect();
    let current = tab.min(Tab::ALL.len() - 1);
    let members: Vec<usize> = registry.in_tab(Tab::ALL[current]).collect();
    let nodes = members
        .iter()
        .map(|&i| {
            let def = &registry.defs[i];
            let place = registry.place(i);
            let parent = def
                .parent
                .as_ref()
                .and_then(|p| registry.index(p))
                .and_then(|p| members.iter().position(|&m| m == p));
            let locked = def.parent.as_ref().is_some_and(|p| !progress.is_done(p));
            NodeView {
                column: place.column,
                row: place.row,
                icon: named_icon(&def.icon, game, icons),
                frame: def.frame,
                title: def.title.clone(),
                description: def.description.clone(),
                done: progress.is_done(&def.id),
                locked,
                met: (progress.met(def), def.criteria.len()),
                parent,
            }
        })
        .collect();
    Some(AdvView {
        tabs,
        current,
        nodes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{advancements::Event, game::tests::flat_game, look::Look, player::mode::GameMode};

    fn icons() -> crate::render::icons::ItemIcons {
        let mobs =
            crate::entity::mobs::MobRegistry::load(&crate::assets::dir().join("data/mobs.ron"))
                .unwrap();
        Look::build(&crate::assets::dir(), Vec::new(), &mobs)
            .unwrap()
            .icons
    }

    #[test]
    fn the_advancements_screen_shows_every_tab_with_its_tree_and_what_is_done() {
        let icons = icons();
        let mut game = flat_game(GameMode::Survival);
        game.run_ticks(2);
        game.note(Event::Used("trade".into()));
        let story = advancements_view(&game, Some(&icons), 0).unwrap();
        assert_eq!(story.tabs.len(), 5);
        assert!(story.tabs.iter().all(|t| t.icon.is_some() && t.total >= 6));
        assert_eq!(story.tabs[0].done, 1, "the root");
        assert_eq!(story.tabs[3].done, 1, "trading, but not the root yet");
        assert!(story.nodes.iter().all(|n| n.icon.is_some()));
        // One root without a parent; every parent index points into the tab.
        assert_eq!(story.nodes.iter().filter(|n| n.parent.is_none()).count(), 1);
        assert!(
            story
                .nodes
                .iter()
                .filter_map(|n| n.parent)
                .all(|p| p < story.nodes.len())
        );
        // The root is done, what hangs off it open, what's under that locked.
        let by_title = |t: &str| story.nodes.iter().find(|n| n.title == t).unwrap();
        assert!(by_title("Minecraft").done);
        assert!(!by_title("Stone Age").done && !by_title("Stone Age").locked);
        assert!(by_title("Getting an Upgrade").locked);
        let adventure = advancements_view(&game, Some(&icons), 3).unwrap();
        let deal = adventure
            .nodes
            .iter()
            .find(|n| n.title == "What a Deal!")
            .unwrap();
        assert!(deal.done, "made even though its root hasn't been");
        // A tab out of range shows the last one.
        assert_eq!(
            advancements_view(&game, Some(&icons), 99).unwrap().current,
            4
        );
    }

    /// A game with the player in a world, the camera holding the mouse.
    fn app_in_a_world() -> App {
        let mut app = App::new(None);
        app.game = Some(flat_game(GameMode::Survival));
        app.stack.set_in_world(true, true);
        app.apply_grab();
        app
    }

    fn view_of(app: &App) -> (f32, f32, usize) {
        let game = app.game.as_ref().unwrap();
        (
            game.player.camera.yaw,
            game.player.camera.pitch,
            game.player.inventory.selected,
        )
    }

    /// Everything the mouse can do in a window: a click, a drag with the
    /// button held, wheel notches and trackpad scrolling.
    fn use_the_mouse(app: &mut App) {
        for button in [MouseButton::Left, MouseButton::Right, MouseButton::Middle] {
            app.on_mouse_button(button, true);
            for step in 0..5 {
                app.on_mouse_motion(40.0 + step as f64, -25.0);
            }
            app.on_mouse_button(button, false);
        }
        app.on_scroll(MouseScrollDelta::LineDelta(0.0, -3.0));
        app.on_scroll(MouseScrollDelta::PixelDelta(PhysicalPosition::new(
            0.0, -90.0,
        )));
    }

    #[test]
    fn screenshots_are_named_for_when_they_were_taken() {
        let at = std::time::UNIX_EPOCH + Duration::from_secs(1_793_028_342);
        assert_eq!(screenshot_stamp(at), "2026-10-26_15.25.42");
        assert_eq!(
            screenshot_stamp(std::time::UNIX_EPOCH),
            "1970-01-01_00.00.00"
        );
    }

    #[test]
    fn keys_do_what_they_are_bound_to() {
        use winit::keyboard::KeyCode;
        let mut app = app_in_a_world();
        app.options
            .keys
            .set(Action::Forward, Some(Input::Key(KeyCode::KeyI)));
        app.options
            .keys
            .set(Action::Hotbar5, Some(Input::Mouse(MouseButton::Back)));
        app.on_input(Input::Key(KeyCode::KeyW), true, false);
        assert!(
            !app.game.as_ref().unwrap().player.keys.forward,
            "W does nothing now"
        );
        app.on_input(Input::Key(KeyCode::KeyI), true, false);
        assert!(
            app.game.as_ref().unwrap().player.keys.forward,
            "I walks forward"
        );
        app.on_mouse_button(MouseButton::Back, true);
        assert_eq!(app.game.as_ref().unwrap().player.inventory.selected, 4);
        // An unbound action does nothing.
        app.options.keys.set(Action::Inventory, None);
        app.on_input(Input::Key(KeyCode::KeyE), true, false);
        assert!(app.screen.is_none());
        app.options.keys.reset_all();
        app.on_input(Input::Key(KeyCode::KeyE), true, false);
        assert!(app.screen.is_some(), "E opens the inventory again");
    }

    #[test]
    fn the_key_binds_screen_takes_the_next_key_or_button_and_escape_unbinds() {
        use crate::ui::options::{OptionsScreen, Page};
        use winit::keyboard::KeyCode;
        let mut app = app_in_a_world();
        app.pause_game();
        let pause = app.pause.as_mut().expect("paused");
        pause.options = Some(OptionsScreen::new(Page::KeyBinds));
        let screen = || OptionsScreen::new(Page::KeyBinds);
        let waiting = |app: &mut App, action| {
            app.pause.as_mut().unwrap().options = Some({
                let mut s = screen();
                s.wait_for(action);
                s
            });
        };
        waiting(&mut app, Action::Jump);
        app.on_input(Input::Key(KeyCode::KeyG), true, false);
        assert_eq!(
            app.options.keys.get(Action::Jump),
            Some(Input::Key(KeyCode::KeyG))
        );
        assert!(
            app.pause.is_some(),
            "the key didn't reach the game or menus"
        );
        waiting(&mut app, Action::Attack);
        app.on_mouse_button(MouseButton::Forward, true);
        assert_eq!(
            app.options.keys.get(Action::Attack),
            Some(Input::Mouse(MouseButton::Forward))
        );
        waiting(&mut app, Action::Drop);
        app.on_input(Input::Key(KeyCode::Escape), true, false);
        assert_eq!(app.options.keys.get(Action::Drop), None);
        assert!(app.pause.is_some(), "Escape only unbound the key");
    }

    #[test]
    fn the_mouse_turns_the_camera_when_no_screen_is_open() {
        let mut app = app_in_a_world();
        let before = view_of(&app);
        use_the_mouse(&mut app);
        let after = view_of(&app);
        assert_ne!(
            (before.0, before.1),
            (after.0, after.1),
            "the drag turned the view"
        );
        assert_ne!(before.2, after.2, "scrolling changed the hotbar slot");
    }

    #[test]
    fn opening_the_advancements_screen_and_clicking_dragging_and_scrolling_leaves_yaw_and_pitch_alone()
     {
        let mut app = app_in_a_world();
        app.open_advancements();
        assert!(!app.stack.grabbed(), "the cursor is released");
        let before = view_of(&app);
        use_the_mouse(&mut app);
        assert_eq!(view_of(&app), before);
        assert!(
            !app.stack.grabbed(),
            "a click in the screen didn't grab it again"
        );
        // Closing the last screen gives the mouse back to the camera.
        app.close_advancements();
        assert!(app.stack.grabbed());
        use_the_mouse(&mut app);
        assert_ne!(view_of(&app), before);
    }

    #[test]
    fn no_screen_lets_the_mouse_reach_the_player() {
        for layer in [
            Layer::Container,
            Layer::Advancements,
            Layer::Pause,
            Layer::Bed,
            Layer::Death,
        ] {
            let mut app = app_in_a_world();
            app.stack.push(layer);
            app.apply_grab();
            let before = view_of(&app);
            use_the_mouse(&mut app);
            assert_eq!(view_of(&app), before, "{layer:?}");
            assert!(!app.stack.grabbed(), "{layer:?}");
        }
    }

    #[test]
    fn a_container_opened_over_another_screen_keeps_the_mouse_free_until_both_close() {
        let mut app = app_in_a_world();
        app.show_container(Screen::Chest);
        app.stack.push(Layer::Pause);
        app.apply_grab();
        app.hide_container();
        assert!(!app.stack.grabbed(), "the pause menu is still open");
        app.resume();
        assert!(app.stack.grabbed());
    }

    #[test]
    fn toasts_slide_in_stay_and_slide_out() {
        let icons = icons();
        let mut game = flat_game(GameMode::Survival);
        game.run_ticks(2);
        game.note(Event::Used("trade".into()));
        let first = toast_views(&game, Some(&icons));
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].title, "What a Deal!");
        assert_eq!(first[0].header, "Advancement Made!");
        assert!(first[0].slide < 0.2, "just arriving");
        game.run_ticks(30);
        assert_eq!(toast_views(&game, Some(&icons))[0].slide, 1.0);
        game.run_ticks(65);
        let leaving = toast_views(&game, Some(&icons));
        assert!(leaving.is_empty() || leaving[0].slide < 1.0);
    }
}
