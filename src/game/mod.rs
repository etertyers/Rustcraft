//! The world being played: chunks, the player, their camera, inventory and
//! input, and dropped items, advanced on the 20 TPS fixed timestep. `App`
//! owns at most one.

pub mod advance;
mod animals;
mod armor;
mod benches;
mod boats;
mod capture;
mod combat;
mod containers;
mod dimensions;
mod dragon;
mod elytra;
mod end_portal;
#[cfg(test)]
mod end_tests;
pub mod environment;
mod experience;
mod explosion;
mod fluids;
mod gateways;
mod gear;
mod hands;
mod logistics;
mod minecarts;
mod mob_acts;
#[cfg(test)]
mod mob_tests;
mod mobs;
#[cfg(test)]
mod moves;
mod nether_mobs;
#[cfg(test)]
mod offhand;
mod portals;
mod potions;
#[cfg(test)]
mod progression;
mod redstone;
mod shulkers;
mod sleep;
mod sounds;
pub mod spawn_test;
mod spawners;
pub mod sync;
mod updates;
pub mod view;
mod villagers;

use std::{path::Path, sync::Arc};

pub use containers::{Bench, BenchKind, Container};

use anyhow::Result;
use glam::DVec3;
use log::{error, info};
use winit::event::MouseScrollDelta;

use crate::{
    bindings::Action,
    config,
    entity::{arrow::Arrow, item::ItemEntity, mob::Mob, mobs::MobRegistry, particle::Particle},
    inventory::{
        enchant::Enchantment,
        player::{HOTBAR_SLOTS, Inventory},
        stack::{ItemStack, Items},
    },
    level::{Level, PlayerData, PlayerFile, WorldDir},
    options::Options,
    player::{
        collision::{Collider, WorldCollider},
        input::MoveKeys,
        mode::GameMode,
        physics::{Body, MoveInput},
        raycast::{self, Hit},
        stats::{DamageSource, Stats, Surroundings},
    },
    render::camera::Camera,
    world::{
        block::Fluid,
        chunk::ChunkPos,
        fast_hash::{FastMap, FastSet},
        fire::Fires,
        fluid::{self, Fluids},
        manager::ChunkManager,
        mesh::BlockModels,
        redstone::Redstone,
        worldgen::{rng::Rng, terrain::Terrain},
    },
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

/// Falling below this hurts 4 a tick, as 1.0.0's void does.
const VOID_Y: f64 = -64.0;
const VOID_DAMAGE: f32 = 4.0;

/// Modern Java Edition's exhaustion for moving: per block sprinted or
/// swum (walking and sneaking are free), and per jump.
const SPRINT_EXHAUSTION: f32 = 0.1;
const SWIM_EXHAUSTION: f32 = 0.01;
const JUMP_EXHAUSTION: f32 = 0.05;
const SPRINT_JUMP_EXHAUSTION: f32 = 0.2;

/// An inventory screen for a `--shot` scene to show, and the slot the
/// pointer rests on (so the highlight can be checked against the slot).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShotUi {
    pub screen: ShotScreen,
    pub hover: Option<crate::inventory::slots::SlotRef>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShotScreen {
    /// The survival inventory.
    Inventory,
    /// The creative inventory on the table's `n`th tab.
    Creative(usize),
    /// Options → Controls → Key Binds over the paused game.
    KeyBinds,
}

/// Slots in an ender chest (a single chest's).
pub const ENDER_CHEST_SLOTS: usize = 27;

/// Everything about one player: where they are and how they move, what
/// they hold and carry, their health and screens, and their settings.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct PlayerState {
    /// The name other players see (Options → Player Name).
    pub name: String,
    /// The ender chest's 27 slots, the same in every ender chest and
    /// different for each player.
    pub ender_chest: Vec<Option<ItemStack>>,
    /// How many times the world has turned the player to face a new way
    /// (teleports), so a client takes the new look instead of its own.
    turned: u32,
    /// Look direction comes from the mouse; position follows `body` each frame.
    pub camera: Camera,
    /// How the player looks to the camera: swinging, bobbing, F5.
    view: view::ViewState,
    body: Body,
    /// What the player sounded like last tick, for the sounds that changed.
    sounds: sounds::Memory,
    /// Feet position at the previous tick, for interpolating the camera.
    prev_feet: DVec3,
    /// How far the camera still trails below the feet after a step up (≤ 0).
    step_lag: f64,
    pub(crate) keys: MoveKeys,
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
    pub inventory: Inventory,
    pub stats: Stats,
    /// The stack held on the mouse in an inventory screen.
    pub cursor: Option<ItemStack>,
    /// The crafting grid, chest or furnace open, if any.
    pub container: Option<Container>,
    /// Lying in a bed.
    sleep: Option<sleep::Sleep>,
    /// The head of the bed the player last slept in, where they respawn.
    bed: Option<glam::IVec3>,
    /// A notice on screen and the ticks it has left (1.0.0's chat lines).
    pub message: Option<(String, u32)>,
    /// The capture tool's two corners (creative).
    capture: [Option<glam::IVec3>; 2],
    /// Ticks the eyes have been under water, for how far the fog reaches
    /// (`environment::water_vision`).
    water_vision: u32,
    /// Ticks since the last swing or item change, which the next swing's
    /// charge builds from.
    attack_ticker: u32,
    last_main_item: Option<crate::inventory::item::ItemId>,
    /// Ticks until a disabled shield can be raised again.
    shield_cooldown: u32,
    /// The hand a right-click is using the item of this tick (the main hand
    /// unless it had nothing to do), and the hand a bow is being drawn in.
    use_hand: crate::inventory::player::Hand,
    drawing_hand: crate::inventory::player::Hand,
    /// The hand holding a raised shield and for how many ticks.
    shield_hand: Option<crate::inventory::player::Hand>,
    shield_ticks: u32,
    /// Ticks the bow has been drawn.
    drawing: Option<u32>,
    mounted: Option<u64>,
    /// The hotbar slot being eaten from and for how many ticks.
    eating: Option<(usize, u32)>,
    /// The block being mined and how far along it is (0–1).
    mining: Option<(glam::IVec3, f32)>,
    boat: Option<u64>,
    pub riding: Option<usize>,
    /// Trackpad scroll not yet turned into slot changes.
    scroll_pixels: f64,
    /// Mouse look speed, 1 for the default.
    sensitivity: f32,
    /// The Controls options: Invert Mouse, Auto-Jump, and Sneak and Sprint
    /// toggled instead of held.
    invert_y: bool,
    auto_jump: bool,
    /// The Particles option.
    particle_level: crate::options::Particles,
    /// FOV Effects and Damage Tilt, 0–1.
    pub(crate) fov_effects: f32,
    pub(crate) damage_tilt: f32,
    /// Skin Customization: the outer layers worn, and Main Hand: Left.
    skin_layers: crate::render::player::OuterLayers,
    pub(crate) left_handed: bool,
    toggle_sneak: bool,
    toggle_sprint: bool,
    /// Free flight, a dev tool toggled with F4 in debug builds.
    #[cfg(debug_assertions)]
    flying: bool,
    pub mode: GameMode,
    /// Ticks spent waiting for the chunks around a player who just went
    /// to another dimension.
    arriving: Option<u32>,
    /// Standing in a portal.
    portal: portals::PortalState,
    /// Ticks the poem and credits have rolled, while they do.
    credits: Option<u32>,
    /// Gliding on elytra: ticks so far, ticks of a rocket's push left,
    /// and whether jump was held last tick.
    glide_ticks: u32,
    rocket: u32,
    jump_was: bool,
    /// Ticks before an End gateway works for the player again.
    gateway_cooldown: u32,
    /// What the world has done, the toasts on screen, and where the
    /// player was when Levitation began.
    progress: crate::advancements::Progress,
    /// The recipe book's open state and filter, kept with the world.
    pub recipe_book: crate::inventory::book::Settings,
    toasts: Vec<advance::Toast>,
    levitation_from: Option<f64>,
}

impl PlayerState {
    /// A player standing at `feet`, looking along `yaw` and `pitch`, with
    /// nothing in their inventory.
    pub fn new(feet: DVec3, yaw: f32, pitch: f32) -> Self {
        let body = Body::new(feet);
        let eye = feet + DVec3::Y * body.eye_height();
        Self {
            name: crate::options::DEFAULT_NAME.to_owned(),
            turned: 0,
            ender_chest: vec![None; ENDER_CHEST_SLOTS],
            camera: Camera::new(eye.as_vec3(), yaw, pitch),
            view: view::ViewState::new(yaw, pitch, None),
            body,
            sounds: Default::default(),
            prev_feet: feet,
            step_lag: 0.0,
            keys: MoveKeys::default(),
            target: None,
            breaking: false,
            placing: false,
            break_clicked: false,
            place_clicked: false,
            break_cooldown: 0,
            place_cooldown: 0,
            inventory: Inventory::default(),
            stats: Stats::default(),
            cursor: None,
            container: None,
            sleep: None,
            bed: None,
            message: None,
            capture: [None, None],
            water_vision: 0,
            attack_ticker: 100,
            last_main_item: None,
            shield_cooldown: 0,
            use_hand: Default::default(),
            drawing_hand: Default::default(),
            shield_hand: None,
            shield_ticks: 0,
            drawing: None,
            mounted: None,
            mining: None,
            eating: None,
            boat: None,
            riding: None,
            scroll_pixels: 0.0,
            sensitivity: 1.0,
            invert_y: false,
            auto_jump: false,
            particle_level: crate::options::Particles::All,
            fov_effects: 1.0,
            damage_tilt: 1.0,
            skin_layers: crate::render::player::ALL_LAYERS,
            left_handed: false,
            toggle_sneak: false,
            toggle_sprint: false,
            #[cfg(debug_assertions)]
            flying: false,
            mode: GameMode::default(),
            arriving: None,
            portal: Default::default(),
            credits: None,
            glide_ticks: 0,
            rocket: 0,
            jump_was: false,
            gateway_cooldown: 0,
            progress: Default::default(),
            recipe_book: Default::default(),
            toasts: Vec::new(),
            levitation_from: None,
        }
    }
}

pub struct Game {
    pub chunks: ChunkManager,
    items: Items,
    /// The player whose turn it is: the only one in single player, and
    /// the host's except while `for_each_player` runs.
    pub player: PlayerState,
    /// Everyone else in the world (LAN players).
    pub others: Vec<PlayerState>,
    /// The world itself, or a client's copy of it.
    role: sync::Role,
    spawn: DVec3,
    /// The world's game rules.
    pub rules: crate::level::GameRules,
    pub difficulty: crate::level::Difficulty,
    /// Dropped items lying around.
    pub entities: Vec<ItemEntity>,
    /// Experience orbs lying around.
    pub orbs: Vec<crate::entity::orb::XpOrb>,
    /// Bits of broken and mined blocks.
    pub particles: Vec<Particle>,
    /// Stars of critical hits and arcs of sweeps.
    pub effects: Vec<crate::entity::effects::Effect>,
    pub arrows: Vec<Arrow>,
    /// Ghasts' fireballs in flight.
    pub fireballs: Vec<crate::entity::fireball::Fireball>,
    /// Shulkers' bullets in flight (M28).
    pub bullets: Vec<crate::entity::bullet::ShulkerBullet>,
    /// Splash potions in flight.
    pub thrown: Vec<crate::entity::thrown::ThrownPotion>,
    /// Sand and gravel falling.
    pub falling: Vec<crate::entity::falling::FallingBlock>,
    /// Blocks due to start falling, by tick.
    falls_due: Vec<(u64, glam::IVec3)>,
    /// Loaded chunks with upgraded v1.0 blocks to join to their neighbors.
    post_processing: Vec<ChunkPos>,
    mob_defs: Arc<MobRegistry>,
    pub mobs: Vec<Mob>,
    /// The id the next mob gets, the mob that last hurt the player (tame
    /// wolves attack it) and for how many more ticks, and the mob the
    /// player sits on.
    next_mob_id: u64,
    threat_id: u64,
    threat_ticks: u32,
    /// Animals in unloaded chunks, waiting for their chunk to come back.
    stashed_mobs: FastMap<ChunkPos, Vec<Mob>>,
    /// Chunks that have had their animals placed (once per chunk).
    populated: FastSet<ChunkPos>,
    /// Scheduled water and lava updates.
    fluids: Fluids,
    fires: Fires,
    redstone: Redstone,
    /// Minecarts, and which one the player sits in.
    pub carts: Vec<crate::entity::minecart::Minecart>,
    /// Boats in every dimension, the one the player sits in, and the last
    /// id given out.
    pub boats: Vec<crate::entity::boat::Boat>,
    next_boat_id: u64,
    /// TNT with its fuse lit.
    pub primed: Vec<crate::entity::tnt::PrimedTnt>,
    /// The last notes note blocks played: (block, instrument, note).
    pub played_notes: Vec<(glam::IVec3, u8, u8)>,
    /// For drops and throws.
    rng: Rng,
    /// Game time not yet simulated, in seconds (less than one tick after a frame).
    tick_time: f32,
    /// Ticks since the world was created.
    time: u64,
    /// Where the world is saved; `None` for the test worlds.
    saved: Option<SavedWorld>,
    ticks_since_save: u32,
    /// The dimension the player is in, and the others as they were left.
    dimension: crate::world::dimension::Dimension,
    away: FastMap<crate::world::dimension::Dimension, dimensions::Away>,
    /// Set on going to another dimension, until the app has seen it.
    dimension_changed: bool,
    /// Every Nether portal lit or built, by dimension: a block of its
    /// bottom row (where linking looks first).
    portals: Vec<(crate::world::dimension::Dimension, [i32; 3])>,
    /// Eyes of ender in flight.
    pub eyes: Vec<crate::entity::eye::EyeOfEnder>,
    /// How the End stands (the dragon fight, the credits seen).
    end: crate::level::EndData,
    /// The ender dragon, while there is one, and what it left: crystals,
    /// its fireballs and clouds of its breath.
    pub dragon: Option<crate::entity::dragon::EnderDragon>,
    pub crystals: Vec<crate::entity::crystal::EndCrystal>,
    pub dragon_fireballs: Vec<crate::entity::breath::DragonFireball>,
    pub breath: Vec<crate::entity::breath::BreathCloud>,
    shot_tab: Option<usize>,
    /// An inventory screen a `--shot` scene wants open.
    shot_ui: Option<ShotUi>,
}

/// A world's folder and its level data, kept for the next save.
struct SavedWorld {
    dir: WorldDir,
    level: Level,
}

impl Game {
    /// Starts with the player's feet at `feet`, looking along `yaw` and `pitch`.
    pub fn new(
        chunks: ChunkManager,
        items: Items,
        feet: DVec3,
        spawn: DVec3,
        yaw: f32,
        pitch: f32,
    ) -> Self {
        Self {
            chunks,
            items,
            player: PlayerState::new(feet, yaw, pitch),
            others: Vec::new(),
            role: sync::Role::Server,
            spawn,
            rules: crate::level::GameRules::default(),
            difficulty: crate::level::Difficulty::default(),
            entities: Vec::new(),
            orbs: Vec::new(),
            particles: Vec::new(),
            effects: Vec::new(),
            arrows: Vec::new(),
            fireballs: Vec::new(),
            thrown: Vec::new(),
            bullets: Vec::new(),
            falling: Vec::new(),
            falls_due: Vec::new(),
            post_processing: Vec::new(),
            mob_defs: Arc::new(MobRegistry::empty()),
            mobs: Vec::new(),
            next_mob_id: 0,
            threat_id: 0,
            threat_ticks: 0,
            stashed_mobs: FastMap::default(),
            populated: FastSet::default(),
            fluids: Fluids::default(),
            fires: Fires::default(),
            redstone: Redstone::default(),
            primed: Vec::new(),
            carts: Vec::new(),
            boats: Vec::new(),
            next_boat_id: 0,
            played_notes: Vec::new(),
            rng: Rng::new(level_random_seed()),
            tick_time: 0.0,
            time: 0,
            saved: None,
            ticks_since_save: 0,
            dimension: Default::default(),
            away: FastMap::default(),
            dimension_changed: false,
            portals: Vec::new(),
            eyes: Vec::new(),
            end: Default::default(),
            dragon: None,
            crystals: Vec::new(),
            dragon_fireballs: Vec::new(),
            breath: Vec::new(),
            shot_tab: None,
            shot_ui: None,
        }
    }

    /// Opens the saved world in `dir` where the player left it.
    pub fn open(dir: WorldDir, items: Items, models: Arc<BlockModels>) -> Result<Self> {
        let mut level = dir.load_level()?;
        // A v1.0 world is upgraded the first time it's opened.
        dir.migrate_v1(&mut level)?;
        info!(
            "opening {:?} (seed {}, {})",
            level.name,
            level.seed,
            level.game_mode.name()
        );
        let source = dimensions::source_for(level.dimension, level.seed);
        let chunks = ChunkManager::with_store(
            source,
            dir.regions(level.dimension),
            models,
            config::RENDER_DISTANCE,
            chunk_workers(),
        );
        let player = &level.player;
        let mut game = Self::new(
            chunks,
            items,
            DVec3::from_array(player.position),
            DVec3::from_array(level.spawn),
            player.yaw,
            player.pitch,
        );
        game.player.mode = level.game_mode;
        game.time = level.time;
        // The host's state as single player kept it, until the host's own
        // file is read (`restore_player`).
        game.take_player_file(&PlayerFile {
            player: player.clone(),
            ender_chest: Vec::new(),
            advancements: level.advancements.clone(),
            recipe_book: level.recipe_book.clone(),
        });
        game.rules = level.rules.clone();
        game.difficulty = level.difficulty;
        game.load_boats(&level.boats);
        game.populated = level
            .populated
            .iter()
            .map(|&(x, z)| ChunkPos::new(x, z))
            .collect();
        game.dimension = level.dimension;
        game.fluids.warm = level.dimension == crate::world::dimension::Dimension::Nether;
        game.portals = level.portals.clone();
        game.end = level.end.clone();
        game.restore_fight(level.end.dragon);
        game.saved = Some(SavedWorld { dir, level });
        Ok(game)
    }

    /// `self.player` as their file keeps them. A stack held in an open
    /// inventory screen, and the crafting grid, go back in the inventory
    /// rather than being lost.
    pub fn player_file(&self) -> PlayerFile {
        let p = &self.player;
        let mut inventory = p.inventory.clone();
        let grid = p
            .container
            .as_ref()
            .and_then(Container::grid)
            .map(<[_]>::to_vec)
            .unwrap_or_default();
        for held in p
            .cursor
            .clone()
            .into_iter()
            .chain(grid.into_iter().flatten())
        {
            inventory.add(held, &self.items);
        }
        PlayerFile {
            player: PlayerData {
                position: self.standing_feet().to_array(),
                yaw: p.camera.yaw,
                pitch: p.camera.pitch,
                inventory,
                flying: p.body.flying,
                stats: p.stats.clone(),
                bed: p.bed.map(|bed| bed.to_array()),
            },
            ender_chest: p.ender_chest.clone(),
            advancements: p.progress.clone(),
            recipe_book: p.recipe_book.clone(),
        }
    }

    /// Puts `self.player` where their file says, carrying what it says.
    pub fn take_player_file(&mut self, file: &PlayerFile) {
        let data = &file.player;
        let feet = DVec3::from_array(data.position);
        let p = &mut self.player;
        p.body = Body::new(feet);
        p.prev_feet = feet;
        p.camera.yaw = data.yaw;
        p.camera.pitch = data.pitch;
        p.turned = p.turned.wrapping_add(1);
        p.inventory = data.inventory.clone();
        p.inventory.repair(&self.items);
        p.stats = data.stats.clone();
        p.bed = data.bed.map(glam::IVec3::from_array);
        p.body.may_fly = p.mode.can_fly();
        p.body.flying = data.flying && p.body.may_fly;
        p.ender_chest = file.ender_chest.clone();
        p.ender_chest.resize(ENDER_CHEST_SLOTS, None);
        p.progress = file.advancements.clone();
        p.recipe_book = file.recipe_book.clone();
    }

    /// Reads `self.player`'s own file by their name, if they've played in
    /// this world before. Returns whether there was one.
    pub fn restore_player(&mut self) -> bool {
        let Some(saved) = &self.saved else {
            return false;
        };
        match saved.dir.load_player(&self.player.name) {
            Ok(Some(file)) => {
                self.take_player_file(&file);
                true
            }
            Ok(None) => false,
            Err(err) => {
                error!("{err:#}");
                false
            }
        }
    }

    /// Writes `self.player`'s own file (a player leaving).
    pub fn save_player(&self) -> Result<()> {
        if let Some(saved) = &self.saved {
            saved
                .dir
                .save_player(&self.player.name, &self.player_file())?;
        }
        Ok(())
    }

    /// Gives the game its mob kinds, and brings back saved animals.
    pub fn with_mobs(mut self, mobs: Arc<MobRegistry>) -> Self {
        self.mob_defs = mobs;
        if let Some(saved) = &self.saved {
            let (animals, populated) =
                (saved.level.mobs.clone(), saved.level.away_populated.clone());
            let here: Vec<_> = animals
                .iter()
                .filter(|mob| mob.dimension == self.dimension)
                .cloned()
                .collect();
            self.restore_mobs(&here);
            self.restore_away(&animals, &populated);
        }
        self
    }

    /// Creates a world in a new folder under `saves` and opens it.
    pub fn create(
        saves: &Path,
        name: &str,
        seed: u64,
        mode: GameMode,
        items: Items,
        models: Arc<BlockModels>,
    ) -> Result<Self> {
        let (x, y, z) = Terrain::new(seed).spawn_point();
        let spawn = [x as f64 + 0.5, y as f64, z as f64 + 0.5];
        let dir = WorldDir::create(saves, &Level::new(name, seed, mode, spawn))?;
        Self::open(dir, items, models)
    }

    /// Writes the changed chunks and the level (player, time). Test worlds
    /// aren't saved.
    pub fn save(&mut self) -> Result<()> {
        self.ticks_since_save = 0;
        let mobs = self.saved_mobs();
        let populated = self.populated.iter().map(|pos| (pos.x, pos.z)).collect();
        let (away_mobs, away_populated) = self.away_for_saving();
        let crystals = self.fight_for_saving();
        let dragon = self.dragon.as_ref().map(|d| d.save());
        if self.saved.is_none() {
            return Ok(());
        }
        let chunks = self.chunks.save()?;
        // Every player's own file, then the host's state in the level too,
        // for older builds.
        let mut files = Vec::new();
        self.for_each_player(|game| files.push((game.player.name.clone(), game.player_file())));
        let host = files[0].1.clone();
        let boats = self.saved_boats();
        let Some(saved) = self.saved.as_mut() else {
            return Ok(());
        };
        for (name, file) in &files {
            saved.dir.save_player(name, file)?;
        }
        let progress = host.advancements;
        let recipe_book = host.recipe_book;
        saved.level.player = host.player;
        saved.level.time = self.time;
        saved.level.mobs = mobs;
        saved.level.mobs.extend(away_mobs);
        saved.level.populated = populated;
        saved.level.away_populated = away_populated;
        saved.level.dimension = self.dimension;
        saved.level.portals = self.portals.clone();
        saved.level.end = self.end.clone();
        saved.level.advancements = progress;
        saved.level.recipe_book = recipe_book;
        saved.level.rules = self.rules.clone();
        saved.level.difficulty = self.difficulty;
        saved.level.boats = boats;
        saved.level.end.crystals = crystals;
        saved.level.end.dragon = dragon;
        saved.dir.save_level(&saved.level)?;
        info!("saved {:?} ({chunks} changed chunks)", saved.level.name);
        Ok(())
    }

    pub fn items(&self) -> &Items {
        &self.items
    }

    /// Puts a new mob of the kind called `name` at `feet`, for scripted
    /// scenes; returns it to dress up.
    pub fn summon(&mut self, name: &str, feet: DVec3, yaw: f32) -> Option<&mut Mob> {
        let kind = self.mob_defs.find(name)?;
        let mob = self.new_mob(kind, feet, yaw);
        self.add_mob(mob);
        self.mobs.last_mut()
    }

    pub fn mob_defs(&self) -> &MobRegistry {
        &self.mob_defs
    }

    /// Game time in ticks since the world was created.
    pub fn time(&self) -> u64 {
        self.time
    }

    /// How far the frame is between the last two ticks, 0–1.
    pub fn tick_alpha(&self) -> f64 {
        (self.tick_time / TICK) as f64
    }

    /// A bound key or button: the hotbar slots, throwing, swapping hands,
    /// the F5 view, the capture tool, the debug keys and, while the cursor
    /// is grabbed, movement, attacking, using and picking blocks. A held
    /// attack or use acts on the next tick, then every few ticks.
    pub fn on_action(&mut self, action: Action, pressed: bool, repeat: bool, grabbed: bool) {
        let press = pressed && !repeat;
        match action {
            #[cfg(debug_assertions)]
            Action::DevKit if press => self.give_dev_kit(),
            // A dev tool for checking the day cycle: skip an hour ahead.
            #[cfg(debug_assertions)]
            Action::SkipHour if pressed => {
                self.time += 1000;
                info!(
                    "time skipped to {}",
                    crate::world::daytime::clock(self.time)
                );
            }
            #[cfg(debug_assertions)]
            Action::DevFlight if press => {
                self.player.flying = !self.player.flying;
                self.player.body.velocity = DVec3::ZERO;
                info!("flying {}", if self.player.flying { "on" } else { "off" });
            }
            Action::Perspective if press => self.cycle_view(),
            Action::CaptureCorner1 if press => self.mark_corner(0),
            Action::CaptureCorner2 if press => self.mark_corner(1),
            Action::CaptureSave if press => self.save_capture(),
            _ if pressed && action.hotbar_slot().is_some() => {
                self.player.inventory.selected = action.hotbar_slot().unwrap_or(0);
            }
            Action::Drop if pressed && grabbed => self.throw_selected(),
            Action::SwapHands if press && grabbed => self.swap_hands(),
            Action::Attack if grabbed || !pressed => {
                self.player.breaking = pressed;
                self.player.break_clicked |= pressed;
                self.player.break_cooldown = 0;
            }
            Action::Use if grabbed || !pressed => {
                self.player.placing = pressed;
                self.player.place_clicked |= pressed;
                self.player.place_cooldown = 0;
            }
            Action::PickBlock if press && grabbed => self.pick_block(),
            Action::Sneak if grabbed && self.player.toggle_sneak => {
                if press {
                    self.player.keys.sneak = !self.player.keys.sneak;
                }
            }
            Action::Sprint if grabbed && self.player.toggle_sprint => {
                if press {
                    self.player.keys.sprint = !self.player.keys.sprint;
                }
            }
            _ if grabbed => {
                self.player.keys.set_key(action, pressed);
            }
            _ => {}
        }
    }

    /// The skin's outer layers the player wears.
    pub fn skin_layers(&self) -> crate::render::player::OuterLayers {
        self.player.skin_layers
    }

    /// Main Hand: Left: the main hand is drawn on the left.
    pub fn left_handed(&self) -> bool {
        self.player.left_handed
    }

    /// Forgets held keys and buttons, e.g. when the cursor is released.
    pub fn release_input(&mut self) {
        self.player.keys.clear();
        self.player.breaking = false;
        self.player.placing = false;
        self.player.break_clicked = false;
        self.player.place_clicked = false;
    }

    /// Applies the player's settings: render distance, biome blend, field
    /// of view and mouse sensitivity.
    pub fn apply_options(&mut self, options: &Options) {
        self.chunks.set_radius(options.render_distance);
        self.chunks.set_blend(options.biome_blend);
        self.chunks.set_look(
            options.smooth_lighting,
            options.graphics == crate::options::Graphics::Fast,
        );
        self.player.camera.fov = options.fov;
        self.player.view.bobbing = options.view_bobbing;
        self.player.sensitivity = options.sensitivity;
        self.player.invert_y = options.invert_y;
        self.player.particle_level = options.particles;
        self.player.fov_effects = options.fov_effects;
        self.player.damage_tilt = options.damage_tilt;
        let skin = options.skin;
        self.player.skin_layers = [
            skin.hat,
            skin.jacket,
            skin.right_sleeve,
            skin.left_sleeve,
            skin.right_pants,
            skin.left_pants,
        ];
        self.player.left_handed = options.main_hand == crate::options::MainHand::Left;
        self.player.auto_jump = options.auto_jump;
        self.player.toggle_sneak = options.toggle_sneak;
        self.player.toggle_sprint = options.toggle_sprint;
    }

    pub fn look(&mut self, dx: f64, dy: f64) {
        let s = config::MOUSE_SENSITIVITY * self.player.sensitivity;
        let dy = if self.player.invert_y { -dy } else { dy };
        self.player.camera.rotate(dx as f32 * s, -dy as f32 * s);
    }

    /// Scrolling down moves right along the hotbar, like Minecraft.
    pub fn on_scroll(&mut self, delta: MouseScrollDelta) {
        let steps = match delta {
            MouseScrollDelta::LineDelta(_, y) => -y.signum() as i32,
            MouseScrollDelta::PixelDelta(pos) => {
                self.player.scroll_pixels -= pos.y;
                let steps = (self.player.scroll_pixels / SCROLL_PIXELS_PER_SLOT).trunc();
                self.player.scroll_pixels -= steps * SCROLL_PIXELS_PER_SLOT;
                steps as i32
            }
        };
        let slot = self.player.inventory.selected as i32 + steps;
        self.player.inventory.selected = slot.rem_euclid(HOTBAR_SLOTS as i32) as usize;
    }

    /// Puts the player's feet at `feet`, looking along `yaw` and `pitch`
    /// (for `--shot` scenes).
    pub fn teleport(&mut self, feet: DVec3, yaw: f32, pitch: f32) {
        self.player.body = Body::new(feet);
        self.player.body.flying = self.player.mode.can_fly();
        self.player.body.may_fly = self.player.mode.can_fly();
        self.player.prev_feet = feet;
        self.player.camera.yaw = yaw;
        self.player.camera.pitch = pitch;
        self.player.turned = self.player.turned.wrapping_add(1);
        self.advance(0.0);
    }

    /// Sets the time of day and game mode (for `--shot` scenes).
    pub fn set_time_and_mode(&mut self, time: u64, mode: GameMode) {
        self.time = time;
        self.player.mode = mode;
    }

    /// Changes blocks the way the player's edits do, waking fluids, fire
    /// and falling blocks around them (for `--shot` scenes).
    pub fn set_blocks(&mut self, edits: &[(glam::IVec3, crate::world::block::BlockState)]) {
        self.edit(edits);
    }

    /// Runs `ticks` game ticks at once (for `--shot` scenes).
    pub fn run_ticks(&mut self, ticks: u32) {
        for _ in 0..ticks {
            self.tick();
        }
        self.advance(0.0);
    }

    /// Flies the player `distance` blocks north at feet height `height`, in
    /// creative, looking a little down (`--bench`'s route).
    pub fn glide(&mut self, distance: f64, height: f64) {
        self.player.mode = GameMode::Creative;
        self.player.body.flying = true;
        let mut feet = self.player.body.position();
        feet.y = height;
        feet.z -= distance;
        self.player.body.set_position(feet);
        self.player.prev_feet = feet;
        self.player.camera.yaw = 0.0;
        self.player.camera.pitch = -0.35;
    }

    /// Advances the game by `dt` seconds of real time: runs any ticks due,
    /// then places the camera between the last two ticks.
    pub fn advance(&mut self, dt: f32) {
        #[cfg(debug_assertions)]
        if self.player.flying {
            self.fly(dt);
        }
        for _ in 0..self.clock(dt) {
            self.tick();
        }
        self.place_camera();
    }

    /// Adds `dt` seconds to the game's clock and returns how many ticks are
    /// due (a few at most, so a stall doesn't run the world ahead).
    pub fn clock(&mut self, dt: f32) -> u32 {
        self.tick_time = (self.tick_time + dt).min(MAX_CATCH_UP);
        let mut due = 0;
        while self.tick_time >= TICK {
            self.tick_time -= TICK;
            due += 1;
        }
        due
    }

    /// Puts the camera at the player's eyes between the last two ticks and
    /// finds what they aim at.
    pub fn place_camera(&mut self) {
        let alpha = (self.tick_time / TICK) as f64;
        let feet = self
            .player
            .prev_feet
            .lerp(self.player.body.position(), alpha);
        let eye = self
            .lying_eye()
            .unwrap_or(feet + DVec3::Y * self.player.body.eye_height());
        // The camera eases between eye heights; what you aim and reach with
        // is the eye itself.
        let seen = self
            .lying_eye()
            .unwrap_or(feet + DVec3::Y * self.player.view.eye_height(alpha as f32) as f64);
        // A step up eases the camera instead of snapping it.
        self.player.camera.position = (seen + DVec3::Y * self.player.step_lag).as_vec3();
        self.player.target = self.find_target(eye);
    }

    /// One game tick: each player's turn, then the world, then what each
    /// player made of it. The world waits while every player is arriving
    /// in a dimension or watching the credits.
    fn tick(&mut self) {
        let mut waiting = true;
        self.for_each_player(|game| waiting &= !game.tick_player());
        if waiting {
            return;
        }
        self.tick_world();
        self.for_each_player(Game::tick_player_after);
        self.time += 1;
        self.ticks_since_save += 1;
        if self.ticks_since_save >= config::AUTOSAVE_TICKS
            && let Err(err) = self.save()
        {
            error!("autosave failed: {err:#}");
        }
    }

    /// Runs `turn` for each player in turn, the host first, with that
    /// player in `self.player`.
    pub(crate) fn for_each_player(&mut self, mut turn: impl FnMut(&mut Game)) {
        turn(self);
        for i in 0..self.others.len() {
            std::mem::swap(&mut self.player, &mut self.others[i]);
            turn(self);
            std::mem::swap(&mut self.player, &mut self.others[i]);
        }
    }

    /// A player's part of the tick: moving, riding or sleeping, health,
    /// using their hands, portals, the view and the attack recharge.
    /// Returns false while they wait for chunks or the credits roll.
    fn tick_player(&mut self) -> bool {
        self.player.prev_feet = self.player.body.position();
        // What the player aims at, from where they stand this tick (a
        // client's camera may be anywhere between ticks).
        let eye = self
            .lying_eye()
            .unwrap_or(self.player.body.position() + DVec3::Y * self.player.body.eye_height());
        self.player.target = self.find_target(eye);
        // Nothing happens until the chunks around an arriving player load.
        self.tick_arrival();
        if self.is_arriving() {
            return false;
        }
        // The poem and credits stop the world while they roll.
        if self.player.credits.is_some() {
            self.tick_credits();
            return false;
        }
        if !self.player.stats.is_dead() {
            if self.is_sleeping() {
                self.tick_sleep();
            } else if self.player.boat.is_some() {
                self.ride_boat();
            } else if self.player.mounted.is_some() {
                self.ride_mob();
            } else if self.is_riding() {
                self.ride_cart();
            } else {
                self.tick_elytra();
                self.move_player();
                self.glide_impact();
            }
            self.tick_armor();
            let health = self.player.stats.health;
            self.tick_stats();
            // Getting hurt wakes a sleeping player (1.0.0).
            if self.player.stats.health < health {
                self.wake();
            }
            if !self.is_sleeping() {
                self.use_hands();
            }
            self.tick_portal();
            self.tick_end_portal();
            self.tick_gateway();
        }
        self.tick_view();
        if self.eye_in() == Some(crate::world::block::Fluid::Water) {
            self.player.water_vision =
                (self.player.water_vision + 1).min(environment::WATER_VISION_TICKS);
        } else {
            self.player.water_vision = self
                .player
                .water_vision
                .saturating_sub(environment::WATER_VISION_FADE);
        }
        if !self.is_sleeping() {
            self.player.stats.awake = self.player.stats.awake.saturating_add(1);
        }
        if let Some((_, ticks)) = &mut self.player.message {
            *ticks = ticks.saturating_sub(1);
            if *ticks == 0 {
                self.player.message = None;
            }
        }
        self.tick_combat();
        true
    }

    /// The world's part of the tick: things in flight, mobs, blocks and
    /// machines.
    fn tick_world(&mut self) {
        self.effects.iter_mut().for_each(|effect| effect.tick());
        self.effects.retain(|effect| !effect.expired());
        self.tick_items();
        self.tick_particles();
        self.tick_arrows();
        self.tick_fireballs();
        self.tick_thrown();
        self.tick_bullets();
        self.tick_eyes();
        self.tick_end();
        self.swirl_effects();
        self.tick_mobs();
        self.tick_spawners();
        self.tick_fluids();
        self.tick_fires();
        self.tick_redstone();
        self.tick_tnt();
        self.tick_carts();
        self.tick_boats();
        self.tick_hoppers();
        self.tick_falling();
        self.post_process();
        self.tick_furnaces();
        self.tick_brewing();
    }

    /// After the world's tick: advancements, the player's sounds and what
    /// dying drops.
    fn tick_player_after(&mut self) {
        self.tick_advancements();
        self.tick_sounds();
        self.on_death();
    }

    fn move_player(&mut self) {
        #[cfg(debug_assertions)]
        if self.player.flying {
            return;
        }
        let collider = WorldCollider {
            world: self.chunks.world(),
            blocks: self.items.blocks,
        };
        self.player.body.may_fly = self.player.mode.can_fly();
        // Wait for the chunks around the player to load before moving them.
        if !collider.is_loaded(&self.player.body.aabb()) {
            return;
        }
        let input = MoveInput {
            look_y: self.player.camera.pitch.sin() as f64,
            using_item: self.using_item(),
            auto_jump: self.player.auto_jump,
            ..self.player.keys.move_input()
        };
        effects_move(&mut self.player.body, &self.player.stats.effects);
        self.player
            .body
            .tick(&input, self.player.camera.yaw, &collider);
        self.player.step_lag = self.player.step_lag * 0.5 - self.player.body.step_up();
        if self.player.body.position().y < VOID_Y {
            if self.player.mode.is_vulnerable() {
                self.player.stats.damage(VOID_DAMAGE, DamageSource::Fall);
            } else {
                // Creative players can't die, so the void sends them home:
                // to the world's spawn, or in the End to the island's middle.
                let home = if self.dimension == crate::world::dimension::Dimension::End {
                    let podium = crate::world::worldgen::end::PODIUM;
                    DVec3::new(
                        f64::from(podium.x) + 5.5,
                        f64::from(podium.y) + 1.0,
                        f64::from(podium.z) + 0.5,
                    )
                } else {
                    self.spawn
                };
                self.player.body = Body::new(home);
                self.player.prev_feet = home;
            }
        }
    }

    /// The liquid at `point`, if it's below the surface (1.0.0's
    /// `isInsideOfMaterial`, which counts a source's whole cell and the
    /// lower part of a shallower flow's).
    fn fluid_at(&self, point: DVec3) -> Option<Fluid> {
        let p = point.floor().as_ivec3();
        let block = self.chunks.world().block(p.x, p.y, p.z)?;
        let fluid = self.items.blocks.get(block.id())?.fluid?;
        let surface =
            (p.y + 1) as f64 - (fluid::height_percent(fluid::level(block)) - 1.0 / 9.0) as f64;
        (point.y < surface).then_some(fluid)
    }

    /// Exhaustion from moving, then a tick of health, hunger, air and fire
    /// (`Player.checkMovementStatistics` and 1.0.0's `onEntityUpdate`).
    fn tick_stats(&mut self) {
        let vulnerable = self.player.mode.is_vulnerable();
        let moved = self.player.body.position() - self.player.prev_feet;
        let horizontal = moved.x.hypot(moved.z) as f32;
        let eye = self.player.body.position() + DVec3::Y * self.player.body.eye_height();
        let eyes_in_water = self.fluid_at(eye) == Some(Fluid::Water);
        let liquid = self.player.body.liquid;
        if vulnerable {
            if self.player.body.jumped {
                let cost = if self.player.body.sprinting {
                    SPRINT_JUMP_EXHAUSTION
                } else {
                    JUMP_EXHAUSTION
                };
                self.player.stats.add_exhaustion(cost);
            }
            if eyes_in_water {
                self.player
                    .stats
                    .add_exhaustion(SWIM_EXHAUSTION * moved.length() as f32);
            } else if liquid == Some(Fluid::Water) {
                self.player
                    .stats
                    .add_exhaustion(SWIM_EXHAUSTION * horizontal);
            } else if self.player.body.on_ground && self.player.body.sprinting {
                self.player
                    .stats
                    .add_exhaustion(SPRINT_EXHAUSTION * horizontal);
            }
        }
        let around = Surroundings {
            on_ground: self.player.body.on_ground,
            fell: -moved.y as f32,
            in_water: liquid == Some(Fluid::Water),
            in_lava: liquid == Some(Fluid::Lava),
            in_fire: WorldCollider {
                world: self.chunks.world(),
                blocks: self.items.blocks,
            }
            .in_fire(&self.player.body.aabb()),
            eyes_in_water,
            flying: self.player.body.flying,
            no_fall: self.player.body.on_climbable || self.player.body.bounced,
            on_magma: self.player.body.on_ground
                && !self.player.body.sneaking
                && self.standing_on(crate::world::blocks::MAGMA_BLOCK),
        };
        self.player.stats.difficulty = self.difficulty;
        self.player.stats.tick(&around, vulnerable);
        if vulnerable {
            self.player.stats.tick_food();
            if self.difficulty == crate::level::Difficulty::Peaceful {
                self.player.stats.tick_peaceful(self.time);
            }
        }
        self.player.body.can_sprint = !vulnerable || self.player.stats.can_sprint();
    }

    /// A dead player's items and experience fall where they died, however
    /// they died (a hit, an arrow, a blast or the fall), unless the world
    /// keeps them. Run every tick while dead; once they're dropped there's
    /// nothing left to drop.
    fn on_death(&mut self) {
        if !self.player.stats.is_dead() || self.rules.keep_inventory {
            return;
        }
        self.drop_experience();
        self.drop_everything();
    }

    /// Scatters every stack in the inventory (and the mouse) around the player.
    fn drop_everything(&mut self) {
        let eye = self.player.body.position() + DVec3::Y * self.player.body.eye_height();
        let mut stacks: Vec<ItemStack> = self.player.cursor.take().into_iter().collect();
        match self.player.container.take() {
            Some(Container::Crafting { grid, .. }) => stacks.extend(grid.into_iter().flatten()),
            Some(Container::Bench(bench)) => stacks.extend(bench.slots.into_iter().flatten()),
            _ => {}
        }
        for slot in 0..crate::inventory::player::SLOTS {
            if let Some(stack) = self.player.inventory.slot(slot).cloned() {
                self.player.inventory.set(slot, None);
                stacks.push(stack);
            }
        }
        // Items with the Curse of Vanishing are gone with the player.
        stacks.retain(|stack| stack.level_of(Enchantment::VanishingCurse) == 0);
        let rng = &mut self.rng;
        let mut random = || rng.next_f64() as f32;
        for stack in stacks {
            self.entities
                .push(ItemEntity::scattered(stack, eye, &mut random));
        }
    }

    /// Whether the block under the player's feet is `id`.
    fn standing_on(&self, id: crate::world::block::BlockId) -> bool {
        let below = (self.player.body.position() - DVec3::Y * 0.01)
            .floor()
            .as_ivec3();
        self.chunks
            .world()
            .block(below.x, below.y, below.z)
            .is_some_and(|b| b.is(id))
    }

    /// Back beside the bed last slept in, or the world spawn, with full
    /// health and food (1.0.0's respawn).
    pub fn respawn(&mut self) {
        // Beds and the world spawn are in the Overworld.
        let away = self.dimension != crate::world::dimension::Dimension::Overworld;
        if away {
            let spawn = self.spawn;
            self.change_dimension(crate::world::dimension::Dimension::Overworld, spawn);
        }
        let feet = self.respawn_point();
        self.player.body = Body::new(feet);
        self.player.prev_feet = feet;
        self.player.stats = Stats::default();
        self.player.mining = None;
        self.release_input();
        if away && let Err(err) = self.save() {
            error!("saving after respawning: {err}");
        }
    }

    /// The block the crosshair points at from `eye`, within reach: the
    /// first block outline the view passes through.
    fn find_target(&self, eye: DVec3) -> Option<Hit> {
        let world = self.chunks.world();
        let dir = self.player.camera.forward().as_dvec3();
        let shapes = crate::world::shape::shapes();
        raycast::raycast_boxes(eye, dir, config::REACH, |cell| {
            world
                .block(cell.x, cell.y, cell.z)
                .map_or(&[], |block| shapes.outline(block))
        })
    }
}

/// What status effects do to how `body` moves this tick.
pub(super) fn effects_move(body: &mut Body, effects: &crate::player::effects::Effects) {
    use crate::player::effects::Effect;
    body.speed_factor = effects.speed_factor();
    body.jump_bonus = effects.jump_boost().0;
    body.slow_falling = effects.has(Effect::SlowFalling);
    body.levitation = effects
        .get(Effect::Levitation)
        .map(|active| active.amplifier);
}

/// A seed for drop and throw randomness, from the clock.
/// The game's random numbers start from the clock, except in tests,
/// which start from the same seed every run so they pass or fail the same
/// way each time.
fn level_random_seed() -> u64 {
    if cfg!(test) {
        0x5eed_2026
    } else {
        crate::level::random_seed()
    }
}

/// One chunk worker per core, leaving one core for the main thread.
pub fn chunk_workers() -> usize {
    let workers = std::thread::available_parallelism()
        .map_or(2, |n| n.get().saturating_sub(1))
        .max(1);
    info!("{workers} chunk worker threads");
    workers
}

#[cfg(test)]
pub(crate) mod tests {
    use std::time::{Duration, Instant};

    use glam::IVec3;

    use super::*;
    use crate::{
        entity::mobs::MobRegistry,
        inventory::{crafting::Recipes, items, stack::ItemStack},
        player::stats::MAX_HEALTH,
        world::{
            Face,
            biome::BiomeColors,
            block::BlockState,
            blocks,
            chunk::ChunkPos,
            props::{Prop, bed_part, door_half},
            save::TempDir,
        },
    };

    pub(crate) fn registries() -> (Items, Arc<BlockModels>) {
        let assets = crate::assets::dir();
        let models = BlockModels::new(
            crate::world::block::registry(),
            |_| Some([0.0; 4]),
            [0.0; 4],
            BiomeColors::uniform(),
        );
        let recipes = Recipes::load(&assets.join("data/recipes.ron")).unwrap();
        (
            Items::new().with_recipes(Arc::new(recipes)),
            Arc::new(models),
        )
    }

    /// Runs chunk loading until the player's chunk is in memory.
    pub(crate) fn load_player_chunk(game: &mut Game) {
        let feet = game.player.body.position();
        let center = ChunkPos::of_block(feet.x.floor() as i32, feet.z.floor() as i32);
        let deadline = Instant::now() + Duration::from_secs(20);
        while game.chunks.world().get(center).is_none() {
            assert!(Instant::now() < deadline, "the player's chunk never loaded");
            game.chunks.update(center);
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    #[test]
    fn each_player_is_saved_in_their_own_file_and_put_back_as_they_left() {
        use crate::{
            inventory::{items, stack::ItemStack},
            world::save::TempDir,
        };
        let saves = TempDir::new("playerdata");
        let (items, models) = registries();
        Game::create(
            saves.path(),
            "Two",
            3,
            GameMode::Survival,
            items.clone(),
            models.clone(),
        )
        .unwrap();
        let dir = || WorldDir::new(saves.path().join("Two"));
        // A world from before M36 has only the level's player: the host
        // takes it, and the first save writes their file.
        let mut game = Game::open(dir(), items.clone(), models.clone()).unwrap();
        game.player.name = "Kenzo".to_owned();
        assert!(!game.restore_player(), "no file yet");
        game.player
            .inventory
            .set(0, Some(ItemStack::new(items::DIAMOND, 2)));
        let mut guest = PlayerState::new(DVec3::new(10.5, 70.0, -3.5), 1.0, 0.2);
        guest.name = "Guest".to_owned();
        guest
            .inventory
            .set(4, Some(ItemStack::new(items::APPLE, 7)));
        guest.ender_chest[3] = Some(ItemStack::new(items::EMERALD, 1));
        game.others.push(guest);
        game.save().unwrap();
        assert!(dir().player_path("Kenzo").is_file());
        assert!(dir().player_path("Guest").is_file());
        drop(game);

        let mut game = Game::open(dir(), items.clone(), models.clone()).unwrap();
        game.others.push(PlayerState::new(DVec3::ZERO, 0.0, 0.0));
        game.others[0].name = "Guest".to_owned();
        assert!(game.with_player(1, Game::restore_player));
        let guest = &game.others[0];
        assert_eq!(
            guest.inventory.slot(4),
            Some(&ItemStack::new(items::APPLE, 7))
        );
        assert_eq!(
            guest.ender_chest[3],
            Some(ItemStack::new(items::EMERALD, 1))
        );
        assert_eq!(guest.body.position(), DVec3::new(10.5, 70.0, -3.5));
        assert_eq!((guest.camera.yaw, guest.camera.pitch), (1.0, 0.2));
        game.player.name = "Kenzo".to_owned();
        assert!(game.restore_player());
        assert_eq!(
            game.player.inventory.slot(0),
            Some(&ItemStack::new(items::DIAMOND, 2))
        );
    }

    #[test]
    fn guests_pick_things_up_and_get_hurt_by_blasts_and_arrows_like_the_host() {
        use crate::{
            entity::arrow::Arrow,
            inventory::{items, stack::ItemStack},
        };
        let mut game = flat_game(GameMode::Survival);
        let mut guest = PlayerState::new(DVec3::new(10.5, 64.0, 0.5), 0.0, 0.0);
        guest.name = "Guest".to_owned();
        guest.mode = GameMode::Survival;
        game.others.push(guest);
        game.run_ticks(2);
        // An apple at the guest's feet is theirs.
        game.entities.push(crate::entity::item::ItemEntity::new(
            ItemStack::new(items::APPLE, 1),
            DVec3::new(10.5, 64.2, 0.5),
            DVec3::ZERO,
            0.0,
        ));
        game.run_ticks(15);
        assert!(game.entities.is_empty());
        assert!(game.others[0].inventory.find(items::APPLE).is_some());
        assert!(game.player.inventory.find(items::APPLE).is_none());
        // The host's arrow hits the guest, never the host.
        let mut random = || 0.5;
        let mut arrow = Arrow::shoot(
            DVec3::new(6.5, 65.5, 0.5),
            DVec3::X,
            1.0,
            DVec3::ZERO,
            false,
            &mut random,
        );
        arrow.shooter = Some(crate::options::DEFAULT_NAME.to_owned());
        game.arrows.push(arrow);
        game.run_ticks(10);
        let after_arrow = game.others[0].stats.health;
        assert!(after_arrow < 20.0, "the arrow hit the guest");
        assert_eq!(game.player.stats.health, 20.0);
        // A blast by the guest hurts them too.
        game.run_ticks(30);
        game.explode(DVec3::new(11.5, 64.5, 2.5), 2.0);
        assert!(
            game.others[0].stats.health < after_arrow,
            "the blast hurt the guest"
        );
    }

    #[test]
    fn a_player_can_hit_another_unless_they_are_in_creative() {
        let mut game = flat_game(GameMode::Survival);
        game.player.camera.pitch = 0.0;
        game.player.camera.yaw = 0.0;
        let mut guest = PlayerState::new(DVec3::new(0.5, 64.0, -1.5), 0.0, 0.0);
        guest.mode = GameMode::Survival;
        game.others.push(guest);
        game.run_ticks(20);
        game.on_action(Action::Attack, true, false, true);
        game.run_ticks(2);
        game.on_action(Action::Attack, false, false, true);
        let health = game.others[0].stats.health;
        assert!(health < 20.0, "hit: {health}");
        assert_eq!(game.others[0].stats.last_source, Some(DamageSource::Player));
        // A creative player can't be hurt.
        game.others[0].mode = GameMode::Creative;
        game.run_ticks(20);
        game.on_action(Action::Attack, true, false, true);
        game.run_ticks(2);
        assert_eq!(game.others[0].stats.health, health);
    }

    #[test]
    fn every_player_gets_a_turn_each_tick_and_keeps_their_own_state() {
        let mut game = flat_game(GameMode::Survival);
        let mut guest = PlayerState::new(DVec3::new(4.5, 70.0, 4.5), 0.0, 0.0);
        guest.name = "Guest".to_owned();
        game.others.push(guest);
        game.run_ticks(40);
        let guest = &game.others[0];
        assert_eq!(guest.name, "Guest");
        assert_eq!(
            guest.body.position().y,
            64.0,
            "the guest fell to the ground"
        );
        assert!(guest.body.position().x > 4.0, "and stayed where they were");
        assert_eq!(
            game.player.name,
            crate::options::DEFAULT_NAME,
            "the host is back"
        );
        assert_eq!(game.player.body.position().x, 0.5);
    }

    #[test]
    fn the_particles_option_shows_all_two_in_three_or_none() {
        let mut game = flat_game(GameMode::Survival);
        let shown = |game: &mut Game, level| {
            game.player.particle_level = level;
            (0..300).filter(|_| game.shows_particle()).count()
        };
        use crate::options::Particles;
        assert_eq!(shown(&mut game, Particles::All), 300);
        assert_eq!(shown(&mut game, Particles::Minimal), 0);
        let some = shown(&mut game, Particles::Decreased);
        assert!((170..230).contains(&some), "{some}");
    }

    #[test]
    fn controls_options_invert_the_mouse_and_toggle_sneak_and_sprint() {
        let mut game = flat_game(GameMode::Survival);
        game.player.camera.pitch = 0.0;
        let pitch = game.player.camera.pitch;
        game.look(0.0, 50.0);
        let down = game.player.camera.pitch - pitch;
        game.apply_options(&Options {
            invert_y: true,
            toggle_sneak: true,
            toggle_sprint: true,
            ..Options::default()
        });
        game.player.camera.pitch = 0.0;
        game.look(0.0, 50.0);
        assert!(
            down != 0.0 && (game.player.camera.pitch + down).abs() < 1e-6,
            "inverted"
        );
        game.on_action(Action::Sneak, true, false, true);
        game.on_action(Action::Sneak, false, false, true);
        assert!(game.player.keys.sneak, "still sneaking after letting go");
        game.on_action(Action::Sneak, true, false, true);
        assert!(!game.player.keys.sneak, "a second press stops");
        game.on_action(Action::Sprint, true, false, true);
        game.on_action(Action::Sprint, true, true, true);
        assert!(
            game.player.keys.sprint,
            "a held key's repeats don't toggle it back"
        );
    }

    #[test]
    fn a_creative_world_reloads_as_creative_with_its_edits_and_player() {
        let saves = TempDir::new("game");
        let (items, models) = registries();
        let mut game = Game::create(
            saves.path(),
            "Creative Test",
            42,
            GameMode::Creative,
            items.clone(),
            models.clone(),
        )
        .unwrap();
        load_player_chunk(&mut game);
        let feet = game.player.body.position() + DVec3::new(0.25, 0.0, -0.5);
        let at = IVec3::new(
            feet.x.floor() as i32,
            feet.y as i32 + 3,
            feet.z.floor() as i32,
        );
        let bricks = BlockState::of(blocks::BRICKS);
        game.chunks.set_blocks(&[(at, bricks)]);
        game.player.body = Body::new(feet);
        game.player.camera.yaw = 1.25;
        game.player.camera.pitch = -0.5;
        game.time = 500;
        game.player.inventory.set(
            3,
            Some(ItemStack::with_damage(items::WOODEN_PICKAXE, 1, 12)),
        );
        game.player
            .inventory
            .set(20, Some(ItemStack::new(items::RED_WOOL, 40)));
        game.player.inventory.selected = 3;
        game.player.cursor = Some(ItemStack::new(items::TORCH, 7));
        game.player.body.flying = true;
        game.player.bed = Some(IVec3::new(3, 70, -9));
        game.save().unwrap();
        drop(game);

        // As after quitting and relaunching.
        let dir = WorldDir::new(saves.path().join("Creative Test"));
        let mut game = Game::open(dir, items, models).unwrap();
        assert_eq!(game.player.mode, GameMode::Creative);
        assert_eq!(game.time, 500);
        assert_eq!(game.player.body.position(), feet);
        assert_eq!(
            (game.player.camera.yaw, game.player.camera.pitch),
            (1.25, -0.5)
        );
        assert_eq!(
            game.player.inventory.slot(3),
            Some(&ItemStack::with_damage(items::WOODEN_PICKAXE, 1, 12))
        );
        assert_eq!(
            game.player.inventory.slot(20),
            Some(&ItemStack::new(items::RED_WOOL, 40))
        );
        assert_eq!(game.player.inventory.selected, 3);
        // The held torches went back into the first free slot.
        assert_eq!(
            game.player.inventory.slot(0),
            Some(&ItemStack::new(items::TORCH, 7))
        );
        assert!(game.player.body.flying);
        assert_eq!(game.player.bed, Some(IVec3::new(3, 70, -9)));
        load_player_chunk(&mut game);
        assert_eq!(game.chunks.world().block(at.x, at.y, at.z), Some(bricks));
    }

    /// `RUSTCRAFT_V1_WORLD=saves/New\ World cargo test --release
    /// opens_a_real_v1_world -- --ignored`: opens a copy of a world made by
    /// v1.0 (never the original), plays it a second and saves it again.
    #[test]
    #[ignore]
    fn opens_a_real_v1_world() {
        let Some(from) = std::env::var_os("RUSTCRAFT_V1_WORLD") else {
            return;
        };
        let saves = TempDir::new("v1");
        let name = "Old World";
        let copy = saves.path().join(name);
        let status = std::process::Command::new("cp")
            .arg("-R")
            .arg(&from)
            .arg(&copy)
            .status()
            .unwrap();
        assert!(status.success());
        let (items, models) = registries();
        let dir = WorldDir::new(copy);
        let mut game = Game::open(dir, items, models)
            .unwrap()
            .with_mobs(crate::game::tests::mob_registry());
        load_player_chunk(&mut game);
        game.run_ticks(40);
        assert!(!game.chunks.world().is_empty());
        game.save().unwrap();
        // The upgrade kept a backup of the old files beside the new ones.
        assert!(saves.path().join(name).read_dir().unwrap().count() > 2);
        println!(
            "opened at {:?}, {} chunks, seed {:?}",
            game.player.body.position(),
            game.chunks.world().len(),
            game.chunks.seed()
        );
    }

    /// A flat test world with the player standing at the origin, looking
    /// straight down, with every chunk around them loaded.
    pub(crate) fn flat_game(mode: GameMode) -> Game {
        let (items, models) = registries();
        let chunks = ChunkManager::new(crate::world::manager::ChunkSource::Flat, models, 1, 2);
        let feet = DVec3::new(0.5, 64.0, 0.5);
        let mut game = Game::new(chunks, items, feet, feet, 0.0, -std::f32::consts::FRAC_PI_2);
        game.player.mode = mode;
        let deadline = Instant::now() + Duration::from_secs(20);
        while game.chunks.world().len() < 9 {
            assert!(Instant::now() < deadline, "chunks never loaded");
            game.chunks.update(ChunkPos::new(0, 0));
            std::thread::sleep(Duration::from_millis(1));
        }
        // One tick to settle onto the ground.
        game.tick();
        game.advance(0.0);
        game
    }

    #[test]
    fn survival_mining_takes_1_0_0_time_and_the_drop_is_picked_up() {
        let mut game = flat_game(GameMode::Survival);
        let grass = |game: &Game| game.chunks.world().block(0, 63, 0);
        game.player.breaking = true;
        // Grass by hand: 1 / 0.6 / 30 per tick, so 18 ticks.
        for _ in 0..17 {
            game.tick();
        }
        assert_eq!(
            grass(&game),
            Some(BlockState::of(blocks::GRASS_BLOCK)),
            "still mining"
        );
        game.tick();
        assert_eq!(grass(&game), Some(BlockState::AIR));
        assert_eq!(game.entities.len(), 1);
        assert_eq!(
            game.entities[0].stack,
            ItemStack::new(items::DIRT, 1),
            "grass drops dirt"
        );
        game.player.breaking = false;
        for _ in 0..40 {
            game.tick();
        }
        assert!(game.entities.is_empty(), "picked up");
        assert_eq!(
            game.player.inventory.slot(0),
            Some(&ItemStack::new(items::DIRT, 1))
        );

        // Placing it on the grass two blocks north uses it up.
        game.player.camera.pitch = -0.3;
        game.advance(0.0);
        game.player.placing = true;
        game.tick();
        assert_eq!(
            game.chunks.world().block(0, 64, -2),
            Some(BlockState::of(blocks::DIRT))
        );
        assert_eq!(game.player.inventory.slot(0), None);
    }

    #[test]
    fn a_bed_goes_down_as_two_halves_and_breaks_into_one_bed() {
        let mut game = flat_game(GameMode::Survival);
        game.player
            .inventory
            .set(0, Some(ItemStack::new(items::RED_BED, 1)));
        // Looking north at the grass two blocks ahead.
        game.player.camera.pitch = -0.7;
        game.advance(0.0);
        game.player.place_clicked = true;
        game.tick();
        let block = |game: &Game, z| game.chunks.world().block(0, 64, z);
        let foot = BlockState::of(blocks::RED_BED).with_face(Prop::Facing, Face::North);
        assert_eq!(block(&game, -2), Some(foot), "the foot");
        assert_eq!(
            block(&game, -3),
            Some(foot.with(Prop::BedPart, bed_part::HEAD)),
            "the head"
        );
        assert_eq!(game.player.inventory.slot(0), None);
        game.break_block(IVec3::new(0, 64, -3));
        assert_eq!(block(&game, -2), Some(BlockState::AIR));
        let drops: Vec<ItemStack> = game
            .entities
            .iter()
            .map(|item| item.stack.clone())
            .collect();
        assert_eq!(drops, [ItemStack::new(items::RED_BED, 1)]);
    }

    #[test]
    fn a_closed_door_blocks_the_way_until_right_clicked_open() {
        let mut game = flat_game(GameMode::Survival);
        game.player
            .inventory
            .set(0, Some(ItemStack::new(items::OAK_DOOR, 1)));
        game.player.camera.pitch = -0.7;
        game.advance(0.0);
        game.player.place_clicked = true;
        game.tick();
        let door = |game: &Game, y| game.chunks.world().block(0, y, -2);
        let lower = BlockState::of(blocks::OAK_DOOR).with_face(Prop::Facing, Face::North);
        let upper = lower.with(Prop::DoubleBlockHalf, door_half::UPPER);
        assert_eq!(door(&game, 64), Some(lower));
        assert_eq!(door(&game, 65), Some(upper));
        // Walking north stops against the door's slab on the cell's south side.
        game.player.camera.pitch = 0.0;
        game.player.keys.set_key(Action::Forward, true);
        for _ in 0..40 {
            game.tick();
        }
        let front = game.player.body.position().z;
        assert!((front - (-1.0 + 0.3)).abs() < 0.01, "stopped at {front}");
        // Right-clicking the upper half opens both.
        game.player.keys.set_key(Action::Forward, false);
        game.advance(0.0);
        game.player.place_clicked = true;
        game.tick();
        assert_eq!(door(&game, 64), Some(lower.with_bool(Prop::Open, true)));
        assert_eq!(door(&game, 65), Some(upper.with_bool(Prop::Open, true)));
        game.player.keys.set_key(Action::Forward, true);
        for _ in 0..40 {
            game.tick();
        }
        assert!(game.player.body.position().z < -3.0, "walked through");
    }

    #[test]
    fn slabs_are_half_a_block_to_walk_up_onto() {
        let mut game = flat_game(GameMode::Survival);
        let slabs: Vec<_> = (-6..=-2)
            .map(|z| (IVec3::new(0, 64, z), BlockState::of(blocks::OAK_SLAB)))
            .collect();
        game.chunks.set_blocks(&slabs);
        game.player.camera.pitch = 0.0;
        game.player.keys.set_key(Action::Forward, true);
        for _ in 0..30 {
            game.tick();
        }
        let feet = game.player.body.position();
        assert!(feet.z < -2.0, "stepped up without jumping: {feet}");
        assert_eq!(feet.y, 64.5);
    }

    #[test]
    fn walking_from_a_path_or_farmland_onto_a_full_block_needs_no_jump() {
        for low in [blocks::DIRT_PATH, blocks::FARMLAND] {
            let mut game = flat_game(GameMode::Survival);
            // The path or farmland lies at the player's feet; past z -3 the floor is
            // a full block, 1/16 higher.
            let edits: Vec<_> = (-8..=3)
                .map(|z| {
                    let block = if z < -3 { blocks::DIRT } else { low };
                    (IVec3::new(0, 63, z), BlockState::of(block))
                })
                .collect();
            game.chunks.set_blocks(&edits);
            game.player.body =
                crate::player::physics::Body::new(glam::DVec3::new(0.5, 63.9375, 2.5));
            game.player.body.on_ground = true;
            game.player.camera.pitch = 0.0;
            game.player.keys.set_key(Action::Forward, true);
            let mut jumped = false;
            for _ in 0..60 {
                game.tick();
                jumped |= game.player.body.velocity.y > 0.1;
            }
            let feet = game.player.body.position();
            assert!(feet.z < -5.0, "walked on: {feet}");
            assert!(!jumped, "jumped");
            assert_eq!(feet.y, 64.0);
        }
    }

    /// A game looking north with `held` in the first slot, hunger at `food`.
    fn eater(held: crate::inventory::item::ItemId, food: i32) -> Game {
        let mut game = flat_game(GameMode::Survival);
        game.player.inventory.set(0, Some(ItemStack::new(held, 4)));
        game.player.stats.food = food;
        game.player.camera.pitch = 0.0;
        game.advance(0.0);
        game
    }

    fn right_click(game: &mut Game) {
        game.advance(0.0);
        game.player.place_clicked = true;
        game.tick();
    }

    #[test]
    fn food_in_hand_still_opens_doors_and_crafting_tables() {
        let mut game = eater(items::BREAD, 10);
        game.chunks.set_blocks(&[(
            IVec3::new(0, 65, -2),
            BlockState::of(blocks::CRAFTING_TABLE),
        )]);
        right_click(&mut game);
        assert!(game.player.container.is_some(), "the crafting table opened");
        assert!(game.player.eating.is_none(), "and nothing was eaten");
        assert_eq!(game.player.inventory.slot(0).map(|s| s.count), Some(4));

        let mut game = eater(items::BREAD, 10);
        let door = BlockState::of(blocks::OAK_DOOR).with_face(Prop::Facing, Face::North);
        game.chunks.set_blocks(&[
            (IVec3::new(0, 65, -2), door),
            (
                IVec3::new(0, 66, -2),
                door.with(Prop::DoubleBlockHalf, door_half::UPPER),
            ),
        ]);
        right_click(&mut game);
        let opened = game.chunks.world().block(0, 65, -2);
        assert_eq!(opened, Some(door.with_bool(Prop::Open, true)));
        assert!(game.player.eating.is_none());
    }

    #[test]
    fn bread_is_eaten_only_when_nothing_is_there_to_use_and_hunger_is_below_20() {
        let mut game = eater(items::BREAD, 10);
        game.player.keys.set_key(Action::Forward, false);
        game.player.place_clicked = true;
        game.tick();
        assert!(
            game.player.eating.is_some(),
            "looking at air with an empty belly eats"
        );
        let mut game = eater(items::BREAD, 20);
        game.player.place_clicked = true;
        game.tick();
        assert!(game.player.eating.is_none(), "a full belly doesn't");
    }

    #[test]
    fn wheat_goes_to_the_cow_not_the_mouth() {
        let mut game = eater(items::WHEAT, 10).with_mobs(mob_registry());
        game.player.camera.pitch = -0.2;
        let kind = game.mob_defs.find("cow").expect("cows exist");
        let cow = game.new_mob(kind, glam::DVec3::new(0.5, 64.0, -1.5), 0.0);
        game.add_mob(cow);
        right_click(&mut game);
        let cow = game.mobs.iter().find(|m| m.kind == kind).expect("cow");
        assert!(cow.data.love > 0, "the cow is in love");
        assert!(game.player.eating.is_none());
        assert_eq!(game.player.inventory.slot(0).map(|s| s.count), Some(3));
    }

    #[test]
    fn the_player_walks_through_torches_and_flowers_but_not_over_fences() {
        let mut game = flat_game(GameMode::Survival);
        // A torch and a flower on the ground, and at head height a torch
        // hanging off a stone just east of the way.
        let wall_torch = BlockState::of(blocks::WALL_TORCH).with_face(Prop::Facing, Face::West);
        game.chunks.set_blocks(&[
            (IVec3::new(0, 64, -2), BlockState::of(blocks::TORCH)),
            (IVec3::new(0, 64, -4), BlockState::of(blocks::DANDELION)),
            (IVec3::new(1, 65, -6), BlockState::of(blocks::STONE)),
            (IVec3::new(0, 65, -6), wall_torch),
        ]);
        game.player.camera.pitch = 0.0;
        game.player.keys.set_key(Action::Forward, true);
        for _ in 0..40 {
            game.tick();
        }
        let feet = game.player.body.position();
        assert!(feet.z < -7.0, "walked through them all: {feet}");
        assert_eq!(feet.y, 64.0);

        // A fence's collision is 1.5 high: jumping doesn't clear it.
        let mut game = flat_game(GameMode::Survival);
        let fences: Vec<_> = (-3..=3)
            .map(|x| (IVec3::new(x, 64, -2), BlockState::of(blocks::OAK_FENCE)))
            .collect();
        game.chunks.set_blocks(&fences);
        game.player.camera.pitch = 0.0;
        game.player.keys.set_key(Action::Forward, true);
        game.player.keys.set_key(Action::Jump, true);
        for _ in 0..60 {
            game.tick();
        }
        let feet = game.player.body.position();
        assert!(feet.z > -1.5, "stopped by the fence: {feet}");
    }

    #[test]
    fn holding_right_click_on_a_door_toggles_it_once() {
        let mut game = flat_game(GameMode::Survival);
        let lower = BlockState::of(blocks::OAK_DOOR).with_face(Prop::Facing, Face::North);
        game.chunks.set_blocks(&[
            (IVec3::new(0, 64, -2), lower),
            (
                IVec3::new(0, 65, -2),
                lower.with(Prop::DoubleBlockHalf, door_half::UPPER),
            ),
        ]);
        game.player.camera.pitch = 0.0;
        game.advance(0.0);
        // One press, held for a second and a half.
        game.on_action(Action::Use, true, false, true);
        for _ in 0..30 {
            game.tick();
        }
        game.on_action(Action::Use, false, false, true);
        let door = game.chunks.world().block(0, 64, -2);
        assert_eq!(door, Some(lower.with_bool(Prop::Open, true)), "opened once");
        // Open, it lies along the west side of its cell: looking straight
        // on passes beside it, so aim at it.
        game.player.camera.yaw = (-0.4f32).atan2(2.0);
        game.player.camera.pitch = (-0.12f32 / 2.04).atan();
        game.advance(0.0);
        assert_eq!(
            game.player.target.map(|hit| hit.block),
            Some(IVec3::new(0, 65, -2))
        );
        game.on_action(Action::Use, true, false, true);
        game.tick();
        game.on_action(Action::Use, false, false, true);
        assert_eq!(game.chunks.world().block(0, 64, -2), Some(lower));
    }

    #[test]
    fn creative_breaks_instantly_without_drops_and_keeps_placed_items() {
        let mut game = flat_game(GameMode::Creative);
        game.player
            .inventory
            .set(0, Some(ItemStack::new(items::STONE, 1)));
        game.player.break_clicked = true;
        game.tick();
        assert_eq!(game.chunks.world().block(0, 63, 0), Some(BlockState::AIR));
        assert!(game.entities.is_empty());
        // Drop into the hole, then place on the grass two blocks north.
        for _ in 0..20 {
            game.tick();
        }
        game.player.camera.pitch = -0.3;
        game.advance(0.0);
        game.player.place_clicked = true;
        game.tick();
        assert_eq!(
            game.chunks.world().block(0, 64, -2),
            Some(BlockState::of(blocks::STONE))
        );
        assert_eq!(
            game.player.inventory.slot(0),
            Some(&ItemStack::new(items::STONE, 1))
        );
    }

    #[test]
    fn stone_mined_with_a_pickaxe_gives_cobblestone_and_wears_it() {
        let mut game = flat_game(GameMode::Survival);
        let stone = BlockState::of(blocks::STONE);
        game.chunks.set_blocks(&[(IVec3::new(0, 63, 0), stone)]);
        game.player
            .inventory
            .set(0, Some(ItemStack::new(items::WOODEN_PICKAXE, 1)));
        game.player.breaking = true;
        // Wooden pickaxe on stone: 2 / 1.5 / 30 per tick, so 23 ticks.
        for _ in 0..22 {
            game.tick();
        }
        assert_eq!(game.chunks.world().block(0, 63, 0), Some(stone));
        game.tick();
        assert_eq!(game.chunks.world().block(0, 63, 0), Some(BlockState::AIR));
        assert_eq!(
            game.entities[0].stack,
            ItemStack::new(items::COBBLESTONE, 1),
            "cobblestone"
        );
        assert_eq!(
            game.player.inventory.slot(0).unwrap().damage,
            1,
            "one use worn"
        );
    }

    #[test]
    fn mining_chips_and_breaking_bursts_into_particles_that_fade() {
        let mut game = flat_game(GameMode::Survival);
        game.player.breaking = true;
        game.tick();
        assert_eq!(game.particles.len(), 1, "a chip from the face being mined");
        for _ in 0..17 {
            game.tick();
        }
        assert_eq!(game.chunks.world().block(0, 63, 0), Some(BlockState::AIR));
        assert!(game.particles.len() >= 64, "{}", game.particles.len());
        game.player.breaking = false;
        for _ in 0..100 {
            game.tick();
        }
        assert!(game.particles.is_empty());
    }

    #[test]
    fn stone_mined_by_hand_drops_nothing() {
        let mut game = flat_game(GameMode::Survival);
        game.chunks
            .set_blocks(&[(IVec3::new(0, 63, 0), BlockState::of(blocks::STONE))]);
        game.player.breaking = true;
        // 1 / 1.5 / 100 per tick, summed in f32 as 1.0.0 does: 151 ticks.
        for _ in 0..151 {
            game.tick();
        }
        assert_eq!(game.chunks.world().block(0, 63, 0), Some(BlockState::AIR));
        assert!(game.entities.is_empty());
    }

    #[test]
    fn a_ten_block_fall_hurts_7_in_survival_and_nothing_in_creative() {
        for (mode, hurt) in [(GameMode::Survival, 7), (GameMode::Creative, 0)] {
            let mut game = flat_game(mode);
            game.player.body = Body::new(DVec3::new(0.5, 74.0, 0.5));
            // No saturation, so healing doesn't start within the 3 seconds.
            game.player.stats.saturation = 0.0;
            for _ in 0..60 {
                game.tick();
            }
            assert_eq!(game.player.body.position().y, 64.0);
            assert_eq!(
                MAX_HEALTH - game.player.stats.health,
                hurt as f32,
                "{mode:?}"
            );
        }
    }

    #[test]
    fn dying_scatters_the_inventory_and_respawning_starts_fresh() {
        let mut game = flat_game(GameMode::Survival);
        game.player
            .inventory
            .set(0, Some(ItemStack::new(items::COBBLESTONE, 30)));
        game.player
            .inventory
            .set(20, Some(ItemStack::new(items::WOODEN_PICKAXE, 1)));
        game.player.body = Body::new(DVec3::new(0.5, 100.0, 0.5));
        for _ in 0..100 {
            game.tick();
        }
        assert!(game.player.stats.is_dead());
        assert_eq!(game.player.stats.last_source, Some(DamageSource::Fall));
        assert_eq!(game.player.inventory.slot(0), None);
        assert_eq!(game.entities.len(), 2, "both stacks dropped");
        game.respawn();
        assert_eq!(game.player.stats, Stats::default());
        assert_eq!(game.player.body.position(), game.spawn);
    }

    #[test]
    fn holding_right_click_for_32_ticks_eats_a_porkchop() {
        let mut game = flat_game(GameMode::Survival);
        game.player.stats.food = 10;
        game.player.stats.saturation = 0.0;
        game.player
            .inventory
            .set(0, Some(ItemStack::new(items::COOKED_PORKCHOP, 2)));
        game.player.camera.pitch = 1.5;
        game.advance(0.0);
        game.player.placing = true;
        for _ in 0..31 {
            game.tick();
        }
        assert_eq!(game.player.stats.food, 10, "still chewing");
        game.tick();
        assert_eq!(game.player.stats.food, 18);
        assert_eq!(
            game.player.inventory.slot(0),
            Some(&ItemStack::new(items::COOKED_PORKCHOP, 1))
        );
        // Full: no more eating.
        game.player.stats.food = 20;
        for _ in 0..40 {
            game.tick();
        }
        assert_eq!(
            game.player.inventory.slot(0),
            Some(&ItemStack::new(items::COOKED_PORKCHOP, 1))
        );
    }

    #[test]
    fn stew_leaves_its_bowl_and_creative_doesnt_eat() {
        let mut game = flat_game(GameMode::Survival);
        game.player.stats.food = 5;
        game.player
            .inventory
            .set(0, Some(ItemStack::new(items::MUSHROOM_STEW, 1)));
        game.player.camera.pitch = 1.5;
        game.player.placing = true;
        for _ in 0..32 {
            game.tick();
        }
        assert_eq!(
            game.player.inventory.slot(0),
            Some(&ItemStack::new(items::BOWL, 1))
        );

        let mut creative = flat_game(GameMode::Creative);
        creative.player.stats.food = 5;
        creative
            .player
            .inventory
            .set(0, Some(ItemStack::new(items::COOKED_PORKCHOP, 1)));
        creative.player.camera.pitch = 1.5;
        creative.player.placing = true;
        for _ in 0..40 {
            creative.tick();
        }
        assert_eq!(
            creative.player.inventory.slot(0),
            Some(&ItemStack::new(items::COOKED_PORKCHOP, 1))
        );
    }

    pub(crate) fn mob_registry() -> Arc<MobRegistry> {
        Arc::new(MobRegistry::load(&crate::assets::dir().join("data/mobs.ron")).unwrap())
    }

    /// A flat world of 17 × 17 chunks (a full spawn area for one player) with
    /// light computed.
    pub(crate) fn full_flat_game(mode: GameMode) -> Game {
        let (items, models) = registries();
        let chunks = ChunkManager::new(crate::world::manager::ChunkSource::Flat, models, 8, 4);
        let feet = DVec3::new(0.5, 64.0, 0.5);
        let mut game = Game::new(chunks, items, feet, feet, 0.0, 0.0).with_mobs(mob_registry());
        game.player.mode = mode;
        let deadline = Instant::now() + Duration::from_secs(60);
        while game.chunks.world().len() < 289
            || game.chunks.light_at(-127, 64, -127).is_none()
            || game.chunks.light_at(127, 64, 127).is_none()
        {
            assert!(Instant::now() < deadline, "chunks never loaded");
            game.chunks.update(ChunkPos::new(0, 0));
            std::thread::sleep(Duration::from_millis(1));
        }
        game
    }

    /// A flat world 5×5 chunks around the origin with light computed.
    /// Waits for the chunk holding block column (x, z) to load, for tests
    /// that build outside the nearest chunks.
    pub(crate) fn wait_for_chunk(game: &mut Game, x: i32, z: i32) {
        let pos = ChunkPos::of_block(x, z);
        let deadline = Instant::now() + Duration::from_secs(20);
        while game.chunks.world().get(pos).is_none() {
            assert!(Instant::now() < deadline, "chunk {pos:?} never loaded");
            game.chunks.update(ChunkPos::new(0, 0));
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    pub(crate) fn lit_flat_game(mode: GameMode) -> Game {
        let (items, models) = registries();
        let chunks = ChunkManager::new(crate::world::manager::ChunkSource::Flat, models, 2, 4);
        let feet = DVec3::new(0.5, 64.0, 0.5);
        let mut game = Game::new(chunks, items, feet, feet, 0.0, 0.0).with_mobs(mob_registry());
        game.player.mode = mode;
        let deadline = Instant::now() + Duration::from_secs(30);
        while game.chunks.light_at(-31, 64, -31).is_none()
            || game.chunks.light_at(47, 64, 47).is_none()
        {
            assert!(Instant::now() < deadline, "light never computed");
            game.chunks.update(ChunkPos::new(0, 0));
            std::thread::sleep(Duration::from_millis(1));
        }
        game
    }

    #[test]
    fn dying_any_way_drops_everything_unless_the_world_keeps_it() {
        use crate::inventory::{
            enchant::Enchantment,
            items,
            player::{ARMOR_HEAD, OFFHAND},
        };
        let stuff = |game: &mut Game| {
            game.player
                .inventory
                .set(0, Some(ItemStack::new(items::DIAMOND, 5)));
            game.player
                .inventory
                .set(20, Some(ItemStack::new(items::OAK_LOG, 30)));
            game.player
                .inventory
                .set(ARMOR_HEAD, Some(ItemStack::new(items::IRON_HELMET, 1)));
            game.player
                .inventory
                .set(OFFHAND, Some(ItemStack::new(items::SHIELD, 1)));
            let mut cursed = ItemStack::new(items::GOLDEN_SWORD, 1);
            cursed.enchant(Enchantment::VanishingCurse, 1);
            game.player.inventory.set(1, Some(cursed));
            game.player.stats.experience.level = 20;
        };
        let mut game = flat_game(GameMode::Survival);
        stuff(&mut game);
        // Killed outside the stats tick (a mob's hit lands after it).
        game.player.stats.health = 0.0;
        game.tick();
        game.tick();
        let dropped: Vec<_> = game.entities.iter().map(|e| e.stack.id).collect();
        assert_eq!(dropped.len(), 4, "{dropped:?}");
        assert!(!dropped.contains(&items::GOLDEN_SWORD), "vanishing curse");
        assert!(
            (0..crate::inventory::player::SLOTS).all(|s| game.player.inventory.slot(s).is_none())
        );
        assert_eq!(game.player.stats.experience.level, 0);
        let orbs: u32 = game.orbs.iter().map(|o| o.value).sum();
        assert_eq!(orbs, 100, "7 a level, at most 100");

        let mut kept = flat_game(GameMode::Survival);
        kept.rules.keep_inventory = true;
        stuff(&mut kept);
        kept.player.stats.health = 0.0;
        kept.tick();
        assert!(kept.entities.is_empty());
        assert!(kept.player.inventory.slot(ARMOR_HEAD).is_some());
        assert_eq!(kept.player.stats.experience.level, 20);
    }

    #[test]
    fn zombies_spawn_at_night_far_from_the_player_and_never_by_day() {
        let mut game = lit_flat_game(GameMode::Survival);
        game.time = 6000;
        for _ in 0..200 {
            game.spawn_monsters();
        }
        assert!(game.mobs.is_empty(), "nothing spawns on lit grass at noon");
        game.time = 18000;
        // The cap allows only a few at once, so clear them and spawn again
        // until a zombie has been among them.
        let mut kinds = std::collections::BTreeSet::new();
        for _ in 0..40 {
            for _ in 0..200 {
                game.spawn_monsters();
            }
            assert!(!game.mobs.is_empty(), "monsters at midnight");
            for monster in &game.mobs {
                assert!(game.mob_defs.get(monster.kind).ai.is_monster());
                assert_eq!(monster.feet().y, 64.0, "on the ground");
                assert!(monster.feet().distance(game.player.body.position()) >= 24.0);
            }
            kinds.extend(
                game.mobs
                    .iter()
                    .map(|m| game.mob_defs.get(m.kind).name.clone()),
            );
            if kinds.contains("zombie") {
                break;
            }
            game.mobs.clear();
        }
        assert!(kinds.contains("zombie"), "zombies among them: {kinds:?}");
    }

    #[test]
    fn pigs_appear_on_grass_in_new_chunks_once_and_survive_saving() {
        let mut game = lit_flat_game(GameMode::Survival);
        let chunks: Vec<ChunkPos> = (-2..=2)
            .flat_map(|z| (-2..=2).map(move |x| ChunkPos::new(x, z)))
            .collect();
        for _ in 0..20 {
            game.populated.clear();
            game.on_chunks_loaded(&chunks);
        }
        assert!(!game.mobs.is_empty(), "some animals");
        assert!(game.mobs.iter().all(|animal| animal.feet().y == 64.0));
        let names: std::collections::BTreeSet<_> = game
            .mobs
            .iter()
            .map(|mob| game.mob_defs.get(mob.kind).name.clone())
            .collect();
        assert!(
            names.len() >= 3,
            "a mix of animals on the plains: {names:?}"
        );
        for name in ["pig", "cow", "sheep", "chicken"] {
            assert!(names.contains(name), "{name} on the plains");
        }
        let animals = game.mobs.len();
        game.on_chunks_loaded(&chunks);
        assert_eq!(game.mobs.len(), animals, "a chunk gets its animals once");
        let saved = game.saved_mobs();
        assert_eq!(saved.len(), animals);
        assert!(saved.iter().all(|mob| {
            let def = game.mob_defs.get(game.mob_defs.find(&mob.name).unwrap());
            mob.health == def.health as f32
        }));
    }

    #[test]
    fn two_diamond_sword_hits_kill_a_zombie_which_drops_its_loot() {
        let mut game = flat_game(GameMode::Survival);
        game.mob_defs = mob_registry();
        let kind = game.mob_defs.find("zombie").unwrap();
        let zombie = Mob::new(
            kind,
            game.mob_defs.get(kind),
            DVec3::new(0.5, 64.0, -1.5),
            0.0,
        );
        game.mobs.push(zombie);
        game.player
            .inventory
            .set(0, Some(ItemStack::new(items::DIAMOND_SWORD, 1)));
        game.player.camera.pitch = 0.0;
        game.time = 18000;
        game.player.break_clicked = true;
        game.tick();
        // A diamond sword swings for 7 at full charge.
        assert_eq!(game.mobs[0].stats.health, 13.0, "diamond sword: 7");
        assert!(game.mobs[0].body.velocity.length() > 0.0, "knocked back");
        assert_eq!(
            game.player.inventory.slot(0).unwrap().damage,
            1,
            "a sword wears 1 per hit"
        );
        // The hurt cooldown absorbs a second hit straight away.
        game.player.break_clicked = true;
        game.tick();
        assert_eq!(game.mobs[0].stats.health, 13.0);
        // The sword recharges in 12.5 ticks; wait for a full swing.
        let front = |game: &mut Game| {
            let feet = game.player.body.position() + DVec3::new(0.0, 0.0, -1.5);
            game.mobs[0].body = crate::player::physics::Body::sized(feet, 0.6, 1.8, 1.53);
        };
        for _ in 0..14 {
            game.tick();
        }
        // Knocked back out of reach? Bring it back in front and hit again.
        front(&mut game);
        game.player.break_clicked = true;
        game.tick();
        assert_eq!(game.mobs[0].stats.health, 6.0);
        for _ in 0..14 {
            game.tick();
        }
        front(&mut game);
        game.player.break_clicked = true;
        game.tick();
        assert!(game.mobs[0].is_dead());
        for _ in 0..25 {
            game.tick();
        }
        assert!(game.mobs.is_empty(), "removed after lying dead");
    }

    #[test]
    fn a_fully_drawn_bow_shoots_an_arrow_into_a_zombie() {
        let mut game = flat_game(GameMode::Survival);
        game.mob_defs = mob_registry();
        let kind = game.mob_defs.find("zombie").unwrap();
        let zombie = Mob::new(
            kind,
            game.mob_defs.get(kind),
            DVec3::new(0.5, 64.0, -4.5),
            0.0,
        );
        game.mobs.push(zombie);
        game.player
            .inventory
            .set(0, Some(ItemStack::new(items::BOW, 1)));
        game.player
            .inventory
            .set(5, Some(ItemStack::new(items::ARROW, 5)));
        game.player.camera.pitch = 0.0;
        game.time = 18000;
        game.player.placing = true;
        for _ in 0..20 {
            game.tick();
        }
        assert!(game.arrows.is_empty(), "drawing, not shot yet");
        game.player.placing = false;
        game.tick();
        assert_eq!(
            game.player.inventory.slot(5).unwrap().count,
            4,
            "one arrow used"
        );
        assert_eq!(
            game.player.inventory.slot(0).unwrap().damage,
            1,
            "the bow wears"
        );
        for _ in 0..5 {
            game.tick();
        }
        assert!(
            game.mobs[0].stats.health <= 20.0 - 6.0,
            "hit: {}",
            game.mobs[0].stats.health
        );

        // Creative shoots without arrows and its arrows can't be picked up.
        let mut creative = flat_game(GameMode::Creative);
        creative
            .player
            .inventory
            .set(0, Some(ItemStack::new(items::BOW, 1)));
        creative.player.camera.pitch = 0.0;
        creative.player.placing = true;
        for _ in 0..20 {
            creative.tick();
        }
        creative.player.placing = false;
        creative.tick();
        assert_eq!(creative.arrows.len(), 1);
        assert!(!creative.arrows[0].pickup);
    }

    #[test]
    fn a_zombie_at_night_hurts_a_survival_player_but_ignores_a_creative_one() {
        for (mode, hurt) in [(GameMode::Survival, true), (GameMode::Creative, false)] {
            let mut game = flat_game(mode);
            game.mob_defs = mob_registry();
            game.time = 18000;
            let kind = game.mob_defs.find("zombie").unwrap();
            let zombie = Mob::new(
                kind,
                game.mob_defs.get(kind),
                DVec3::new(0.5, 64.0, -2.5),
                0.0,
            );
            game.mobs.push(zombie);
            for _ in 0..100 {
                game.tick();
            }
            assert_eq!(
                game.player.stats.health < MAX_HEALTH,
                hurt,
                "{mode:?}: {}",
                game.player.stats.health
            );
        }
    }
}
