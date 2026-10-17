//! The world being played: chunks, the player, their camera, inventory and
//! input, and dropped items, advanced on the 20 TPS fixed timestep. `App`
//! owns at most one.

pub mod environment;
mod hands;
mod mobs;

use std::{
    collections::{HashMap, HashSet},
    path::Path,
    sync::Arc,
};

use anyhow::Result;
use glam::DVec3;
use log::{error, info};
use winit::{
    event::{MouseButton, MouseScrollDelta},
    keyboard::KeyCode,
};

use crate::{
    config,
    entity::{item::ItemEntity, mob::Mob, mobs::MobRegistry},
    inventory::{
        player::{HOTBAR_SLOTS, Inventory},
        stack::{ItemStack, Items},
    },
    level::{Level, PlayerData, WorldDir},
    player::{
        collision::WorldCollider,
        input::MoveKeys,
        mode::GameMode,
        physics::Body,
        raycast::{self, Hit},
        stats::{DamageSource, Stats, Surroundings},
    },
    render::camera::Camera,
    world::{
        block::Fluid,
        chunk::ChunkPos,
        manager::{ChunkManager, ChunkSource},
        mesh::BlockModels,
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
const VOID_DAMAGE: i32 = 4;

/// 1.0.0's exhaustion for moving: per block walked, sprinted or swum, and
/// per jump.
const WALK_EXHAUSTION: f32 = 0.01;
const SPRINT_EXHAUSTION: f32 = 0.1;
const SWIM_EXHAUSTION: f32 = 0.015;
const JUMP_EXHAUSTION: f32 = 0.2;
const SPRINT_JUMP_EXHAUSTION: f32 = 0.8;

pub struct Game {
    pub chunks: ChunkManager,
    items: Items,
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
    pub inventory: Inventory,
    pub stats: Stats,
    /// The stack held on the mouse in an inventory screen.
    pub cursor: Option<ItemStack>,
    /// Dropped items lying around.
    pub entities: Vec<ItemEntity>,
    mob_defs: Arc<MobRegistry>,
    pub mobs: Vec<Mob>,
    /// Animals in unloaded chunks, waiting for their chunk to come back.
    stashed_mobs: HashMap<ChunkPos, Vec<Mob>>,
    /// Chunks that have had their animals placed (once per chunk).
    populated: HashSet<ChunkPos>,
    /// The hotbar slot being eaten from and for how many ticks.
    eating: Option<(usize, u32)>,
    /// The block being mined and how far along it is (0–1).
    mining: Option<(glam::IVec3, f32)>,
    /// For drops and throws.
    rng: Rng,
    /// Trackpad scroll not yet turned into slot changes.
    scroll_pixels: f64,
    /// Game time not yet simulated, in seconds (less than one tick after a frame).
    tick_time: f32,
    /// Free flight, a dev tool toggled with F4 in debug builds.
    #[cfg(debug_assertions)]
    flying: bool,
    pub mode: GameMode,
    /// Ticks since the world was created.
    time: u64,
    /// Where the world is saved; `None` for the test worlds.
    saved: Option<SavedWorld>,
    ticks_since_save: u32,
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
        let body = Body::new(feet);
        let eye = feet + DVec3::Y * body.eye_height();
        Self {
            chunks,
            items,
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
            inventory: Inventory::default(),
            stats: Stats::default(),
            cursor: None,
            entities: Vec::new(),
            mob_defs: Arc::new(MobRegistry::empty()),
            mobs: Vec::new(),
            stashed_mobs: HashMap::new(),
            populated: HashSet::new(),
            mining: None,
            eating: None,
            rng: Rng::new(level_random_seed()),
            scroll_pixels: 0.0,
            tick_time: 0.0,
            #[cfg(debug_assertions)]
            flying: false,
            mode: GameMode::default(),
            time: 0,
            saved: None,
            ticks_since_save: 0,
        }
    }

    /// Opens the saved world in `dir` where the player left it.
    pub fn open(dir: WorldDir, items: Items, models: Arc<BlockModels>) -> Result<Self> {
        let level = dir.load_level()?;
        info!(
            "opening {:?} (seed {}, {})",
            level.name,
            level.seed,
            level.game_mode.name()
        );
        let source = ChunkSource::Terrain(Box::new(Terrain::new(level.seed)));
        let chunks = ChunkManager::with_store(
            source,
            dir.regions(),
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
        game.mode = level.game_mode;
        game.time = level.time;
        game.inventory = player.inventory.clone();
        game.stats = player.stats.clone();
        game.inventory.repair(&game.items);
        game.body.may_fly = game.mode.can_fly();
        game.body.flying = player.flying && game.body.may_fly;
        game.populated = level
            .populated
            .iter()
            .map(|&(x, z)| ChunkPos::new(x, z))
            .collect();
        game.saved = Some(SavedWorld { dir, level });
        Ok(game)
    }

    /// Gives the game its mob kinds, and brings back saved animals.
    pub fn with_mobs(mut self, mobs: Arc<MobRegistry>) -> Self {
        self.mob_defs = mobs;
        if let Some(saved) = &self.saved {
            let animals = saved.level.mobs.clone();
            self.restore_mobs(&animals);
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
        let Some(saved) = self.saved.as_mut() else {
            return Ok(());
        };
        let chunks = self.chunks.save()?;
        // A stack held in an open inventory screen goes back in the
        // inventory rather than being lost.
        let mut inventory = self.inventory.clone();
        if let Some(held) = self.cursor {
            inventory.add(held, &self.items);
        }
        saved.level.player = PlayerData {
            position: self.body.position().to_array(),
            yaw: self.camera.yaw,
            pitch: self.camera.pitch,
            inventory,
            flying: self.body.flying,
            stats: self.stats.clone(),
        };
        saved.level.time = self.time;
        saved.level.mobs = mobs;
        saved.level.populated = populated;
        saved.dir.save_level(&saved.level)?;
        info!("saved {:?} ({chunks} changed chunks)", saved.level.name);
        Ok(())
    }

    pub fn items(&self) -> &Items {
        &self.items
    }

    /// Game time in ticks since the world was created.
    pub fn time(&self) -> u64 {
        self.time
    }

    /// How far the frame is between the last two ticks, 0–1.
    pub fn tick_alpha(&self) -> f64 {
        (self.tick_time / TICK) as f64
    }

    /// Number keys, Q to throw, the F4 dev flight toggle and, while the
    /// cursor is grabbed, movement keys.
    pub fn on_key(&mut self, key: KeyCode, pressed: bool, repeat: bool, grabbed: bool) {
        #[cfg(debug_assertions)]
        if key == KeyCode::F6 && pressed && !repeat {
            self.give_dev_kit();
            return;
        }
        // A dev tool for checking the day cycle: skip an hour ahead.
        #[cfg(debug_assertions)]
        if key == KeyCode::F7 && pressed {
            self.time += 1000;
            info!(
                "time skipped to {}",
                crate::world::daytime::clock(self.time)
            );
            return;
        }
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
            self.inventory.selected = slot;
        } else if key == KeyCode::KeyQ && pressed && grabbed {
            self.throw_selected();
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
            MouseButton::Middle if pressed => self.pick_block(),
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
        let slot = self.inventory.selected as i32 + steps;
        self.inventory.selected = slot.rem_euclid(HOTBAR_SLOTS as i32) as usize;
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
        if !self.stats.is_dead() {
            self.move_player();
            self.tick_stats();
            self.use_hands();
        }
        self.tick_items();
        self.tick_mobs();
        self.time += 1;
        self.ticks_since_save += 1;
        if self.ticks_since_save >= config::AUTOSAVE_TICKS
            && let Err(err) = self.save()
        {
            error!("autosave failed: {err:#}");
        }
    }

    fn move_player(&mut self) {
        #[cfg(debug_assertions)]
        if self.flying {
            return;
        }
        let collider = WorldCollider {
            world: self.chunks.world(),
            blocks: &self.items.blocks,
        };
        self.body.may_fly = self.mode.can_fly();
        // Wait for the chunks around the player to load before moving them.
        if !collider.is_loaded(&self.body.aabb()) {
            return;
        }
        self.body
            .tick(&self.keys.move_input(), self.camera.yaw, &collider);
        if self.body.position().y < VOID_Y {
            if self.mode.is_vulnerable() {
                self.stats.damage(VOID_DAMAGE, DamageSource::Fall);
            } else {
                // Creative players can't die, so the void sends them home.
                self.body = Body::new(self.spawn);
                self.prev_feet = self.spawn;
            }
        }
    }

    /// The liquid at `point`, if any.
    fn fluid_at(&self, point: DVec3) -> Option<Fluid> {
        let p = point.floor().as_ivec3();
        let block = self.chunks.world().block(p.x, p.y, p.z)?;
        self.items.blocks.get(block.id())?.fluid
    }

    /// Exhaustion from moving, then a tick of health, hunger, air and fire
    /// (1.0.0's `addMovementStat` and `onEntityUpdate`). Dying drops the
    /// whole inventory.
    fn tick_stats(&mut self) {
        let vulnerable = self.mode.is_vulnerable();
        let moved = self.body.position() - self.prev_feet;
        let horizontal = moved.x.hypot(moved.z) as f32;
        let eye = self.body.position() + DVec3::Y * self.body.eye_height();
        let eyes_in_water = self.fluid_at(eye) == Some(Fluid::Water);
        let liquid = self.body.liquid;
        if vulnerable {
            if self.body.jumped {
                let cost = if self.body.sprinting {
                    SPRINT_JUMP_EXHAUSTION
                } else {
                    JUMP_EXHAUSTION
                };
                self.stats.add_exhaustion(cost);
            }
            if eyes_in_water {
                self.stats
                    .add_exhaustion(SWIM_EXHAUSTION * moved.length() as f32);
            } else if liquid == Some(Fluid::Water) {
                self.stats.add_exhaustion(SWIM_EXHAUSTION * horizontal);
            } else if self.body.on_ground {
                let per_block = if self.body.sprinting {
                    SPRINT_EXHAUSTION
                } else {
                    WALK_EXHAUSTION
                };
                self.stats.add_exhaustion(per_block * horizontal);
            }
        }
        let around = Surroundings {
            on_ground: self.body.on_ground,
            fell: -moved.y as f32,
            in_water: liquid == Some(Fluid::Water),
            in_lava: liquid == Some(Fluid::Lava),
            eyes_in_water,
            flying: self.body.flying,
        };
        self.stats.tick(&around, vulnerable);
        if vulnerable {
            self.stats.tick_food();
        }
        self.body.can_sprint = !vulnerable || self.stats.can_sprint();
        if self.stats.is_dead() {
            self.drop_everything();
        }
    }

    /// Scatters every stack in the inventory (and the mouse) around the player.
    fn drop_everything(&mut self) {
        let eye = self.body.position() + DVec3::Y * self.body.eye_height();
        let mut stacks: Vec<ItemStack> = self.cursor.take().into_iter().collect();
        for slot in 0..crate::inventory::player::SLOTS {
            if let Some(stack) = self.inventory.slot(slot).copied() {
                self.inventory.set(slot, None);
                stacks.push(stack);
            }
        }
        let rng = &mut self.rng;
        let mut random = || rng.next_f64() as f32;
        for stack in stacks {
            self.entities
                .push(ItemEntity::scattered(stack, eye, &mut random));
        }
    }

    /// Back to the world spawn with full health and food (1.0.0's respawn).
    pub fn respawn(&mut self) {
        self.body = Body::new(self.spawn);
        self.prev_feet = self.spawn;
        self.stats = Stats::default();
        self.mining = None;
        self.release_input();
    }

    /// The block the crosshair points at from `eye`, within reach.
    fn find_target(&self, eye: DVec3) -> Option<Hit> {
        let world = self.chunks.world();
        let dir = self.camera.forward().as_dvec3();
        raycast::raycast(eye, dir, config::REACH, |cell| {
            world
                .block(cell.x, cell.y, cell.z)
                .and_then(|block| self.items.blocks.get(block.id()))
                .is_some_and(|def| def.is_targetable())
        })
    }
}

/// A seed for drop and throw randomness, from the clock.
fn level_random_seed() -> u64 {
    crate::level::random_seed()
}

/// One chunk worker per core, leaving one core for the main thread.
pub fn chunk_workers() -> usize {
    let workers = std::thread::available_parallelism()
        .map_or(2, |n| n.get().saturating_sub(1))
        .max(1);
    info!("{workers} chunk worker threads");
    workers
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

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use glam::IVec3;

    use super::*;
    use crate::{
        entity::mobs::MobRegistry,
        inventory::{item::ItemRegistry, stack::ItemStack},
        player::stats::MAX_HEALTH,
        world::{
            biome::BiomeColors,
            block::{BlockRegistry, BlockState},
            chunk::ChunkPos,
            save::TempDir,
        },
    };

    fn registries() -> (Items, Arc<BlockModels>) {
        let assets = crate::assets::dir();
        let blocks = BlockRegistry::load(&assets.join("data/blocks.ron")).unwrap();
        let models = BlockModels::new(
            &blocks,
            |_| Some([0.0; 4]),
            [0.0; 4],
            BiomeColors::uniform(),
        );
        let items = ItemRegistry::load(&assets.join("data/items.ron")).unwrap();
        (
            Items::new(Arc::new(blocks), Arc::new(items)),
            Arc::new(models),
        )
    }

    /// Runs chunk loading until the player's chunk is in memory.
    fn load_player_chunk(game: &mut Game) {
        let feet = game.body.position();
        let center = ChunkPos::of_block(feet.x.floor() as i32, feet.z.floor() as i32);
        let deadline = Instant::now() + Duration::from_secs(20);
        while game.chunks.world().get(center).is_none() {
            assert!(Instant::now() < deadline, "the player's chunk never loaded");
            game.chunks.update(center);
            std::thread::sleep(Duration::from_millis(1));
        }
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
        let feet = game.body.position() + DVec3::new(0.25, 0.0, -0.5);
        let at = IVec3::new(
            feet.x.floor() as i32,
            feet.y as i32 + 3,
            feet.z.floor() as i32,
        );
        let bricks = BlockState::new(45, 0);
        game.chunks.set_blocks(&[(at, bricks)]);
        game.body = Body::new(feet);
        game.camera.yaw = 1.25;
        game.camera.pitch = -0.5;
        game.time = 500;
        game.inventory
            .set(3, Some(ItemStack::with_damage(270, 1, 12)));
        game.inventory
            .set(20, Some(ItemStack::with_damage(35, 40, 14)));
        game.inventory.selected = 3;
        game.cursor = Some(ItemStack::new(50, 7));
        game.body.flying = true;
        game.save().unwrap();
        drop(game);

        // As after quitting and relaunching.
        let dir = WorldDir::new(saves.path().join("Creative Test"));
        let mut game = Game::open(dir, items, models).unwrap();
        assert_eq!(game.mode, GameMode::Creative);
        assert_eq!(game.time, 500);
        assert_eq!(game.body.position(), feet);
        assert_eq!((game.camera.yaw, game.camera.pitch), (1.25, -0.5));
        assert_eq!(
            game.inventory.slot(3),
            Some(&ItemStack::with_damage(270, 1, 12))
        );
        assert_eq!(
            game.inventory.slot(20),
            Some(&ItemStack::with_damage(35, 40, 14))
        );
        assert_eq!(game.inventory.selected, 3);
        // The held torches went back into the first free slot.
        assert_eq!(game.inventory.slot(0), Some(&ItemStack::new(50, 7)));
        assert!(game.body.flying);
        load_player_chunk(&mut game);
        assert_eq!(game.chunks.world().block(at.x, at.y, at.z), Some(bricks));
    }

    /// A flat test world with the player standing at the origin, looking
    /// straight down, with every chunk around them loaded.
    fn flat_game(mode: GameMode) -> Game {
        let (items, models) = registries();
        let chunks = ChunkManager::new(ChunkSource::Flat, models, 1, 2);
        let feet = DVec3::new(0.5, 64.0, 0.5);
        let mut game = Game::new(chunks, items, feet, feet, 0.0, -std::f32::consts::FRAC_PI_2);
        game.mode = mode;
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
        game.breaking = true;
        // Grass by hand: 1 / 0.6 / 30 per tick, so 18 ticks.
        for _ in 0..17 {
            game.tick();
        }
        assert_eq!(grass(&game), Some(BlockState::new(2, 0)), "still mining");
        game.tick();
        assert_eq!(grass(&game), Some(BlockState::AIR));
        assert_eq!(game.entities.len(), 1);
        assert_eq!(
            game.entities[0].stack,
            ItemStack::new(3, 1),
            "grass drops dirt"
        );
        game.breaking = false;
        for _ in 0..40 {
            game.tick();
        }
        assert!(game.entities.is_empty(), "picked up");
        assert_eq!(game.inventory.slot(0), Some(&ItemStack::new(3, 1)));

        // Placing it on the grass two blocks north uses it up.
        game.camera.pitch = -0.3;
        game.advance(0.0);
        game.placing = true;
        game.tick();
        assert_eq!(
            game.chunks.world().block(0, 64, -2),
            Some(BlockState::new(3, 0))
        );
        assert_eq!(game.inventory.slot(0), None);
    }

    #[test]
    fn creative_breaks_instantly_without_drops_and_keeps_placed_items() {
        let mut game = flat_game(GameMode::Creative);
        game.inventory.set(0, Some(ItemStack::new(1, 1)));
        game.break_clicked = true;
        game.tick();
        assert_eq!(game.chunks.world().block(0, 63, 0), Some(BlockState::AIR));
        assert!(game.entities.is_empty());
        // Drop into the hole, then place on the grass two blocks north.
        for _ in 0..20 {
            game.tick();
        }
        game.camera.pitch = -0.3;
        game.advance(0.0);
        game.place_clicked = true;
        game.tick();
        assert_eq!(
            game.chunks.world().block(0, 64, -2),
            Some(BlockState::new(1, 0))
        );
        assert_eq!(game.inventory.slot(0), Some(&ItemStack::new(1, 1)));
    }

    #[test]
    fn stone_mined_with_a_pickaxe_gives_cobblestone_and_wears_it() {
        let mut game = flat_game(GameMode::Survival);
        let stone = BlockState::new(1, 0);
        game.chunks.set_blocks(&[(IVec3::new(0, 63, 0), stone)]);
        game.inventory.set(0, Some(ItemStack::new(270, 1)));
        game.breaking = true;
        // Wooden pickaxe on stone: 2 / 1.5 / 30 per tick, so 23 ticks.
        for _ in 0..22 {
            game.tick();
        }
        assert_eq!(game.chunks.world().block(0, 63, 0), Some(stone));
        game.tick();
        assert_eq!(game.chunks.world().block(0, 63, 0), Some(BlockState::AIR));
        assert_eq!(game.entities[0].stack, ItemStack::new(4, 1), "cobblestone");
        assert_eq!(game.inventory.slot(0).unwrap().damage, 1, "one use worn");
    }

    #[test]
    fn stone_mined_by_hand_drops_nothing() {
        let mut game = flat_game(GameMode::Survival);
        game.chunks
            .set_blocks(&[(IVec3::new(0, 63, 0), BlockState::new(1, 0))]);
        game.breaking = true;
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
            game.body = Body::new(DVec3::new(0.5, 74.0, 0.5));
            for _ in 0..60 {
                game.tick();
            }
            assert_eq!(game.body.position().y, 64.0);
            assert_eq!(MAX_HEALTH - game.stats.health, hurt, "{mode:?}");
        }
    }

    #[test]
    fn dying_scatters_the_inventory_and_respawning_starts_fresh() {
        let mut game = flat_game(GameMode::Survival);
        game.inventory.set(0, Some(ItemStack::new(4, 30)));
        game.inventory.set(20, Some(ItemStack::new(270, 1)));
        game.body = Body::new(DVec3::new(0.5, 100.0, 0.5));
        for _ in 0..100 {
            game.tick();
        }
        assert!(game.stats.is_dead());
        assert_eq!(game.stats.last_source, Some(DamageSource::Fall));
        assert_eq!(game.inventory.slot(0), None);
        assert_eq!(game.entities.len(), 2, "both stacks dropped");
        game.respawn();
        assert_eq!(game.stats, Stats::default());
        assert_eq!(game.body.position(), game.spawn);
    }

    #[test]
    fn holding_right_click_for_32_ticks_eats_a_porkchop() {
        let mut game = flat_game(GameMode::Survival);
        game.stats.food = 10;
        game.stats.saturation = 0.0;
        game.inventory.set(0, Some(ItemStack::new(320, 2)));
        game.camera.pitch = 1.5;
        game.advance(0.0);
        game.placing = true;
        for _ in 0..31 {
            game.tick();
        }
        assert_eq!(game.stats.food, 10, "still chewing");
        game.tick();
        assert_eq!(game.stats.food, 18);
        assert_eq!(game.inventory.slot(0), Some(&ItemStack::new(320, 1)));
        // Full: no more eating.
        game.stats.food = 20;
        for _ in 0..40 {
            game.tick();
        }
        assert_eq!(game.inventory.slot(0), Some(&ItemStack::new(320, 1)));
    }

    #[test]
    fn stew_leaves_its_bowl_and_creative_doesnt_eat() {
        let mut game = flat_game(GameMode::Survival);
        game.stats.food = 5;
        game.inventory.set(0, Some(ItemStack::new(282, 1)));
        game.camera.pitch = 1.5;
        game.placing = true;
        for _ in 0..32 {
            game.tick();
        }
        assert_eq!(game.inventory.slot(0), Some(&ItemStack::new(281, 1)));

        let mut creative = flat_game(GameMode::Creative);
        creative.stats.food = 5;
        creative.inventory.set(0, Some(ItemStack::new(320, 1)));
        creative.camera.pitch = 1.5;
        creative.placing = true;
        for _ in 0..40 {
            creative.tick();
        }
        assert_eq!(creative.inventory.slot(0), Some(&ItemStack::new(320, 1)));
    }

    fn mob_registry() -> Arc<MobRegistry> {
        Arc::new(MobRegistry::load(&crate::assets::dir().join("data/mobs.ron")).unwrap())
    }

    /// A flat world 5×5 chunks around the origin with light computed.
    fn lit_flat_game(mode: GameMode) -> Game {
        let (items, models) = registries();
        let chunks = ChunkManager::new(ChunkSource::Flat, models, 2, 4);
        let feet = DVec3::new(0.5, 64.0, 0.5);
        let mut game = Game::new(chunks, items, feet, feet, 0.0, 0.0).with_mobs(mob_registry());
        game.mode = mode;
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
    fn zombies_spawn_at_night_far_from_the_player_and_never_by_day() {
        let mut game = lit_flat_game(GameMode::Survival);
        game.time = 6000;
        for _ in 0..200 {
            game.spawn_monsters();
        }
        assert!(game.mobs.is_empty(), "nothing spawns on lit grass at noon");
        game.time = 18000;
        for _ in 0..200 {
            game.spawn_monsters();
        }
        assert!(!game.mobs.is_empty(), "zombies at midnight");
        for zombie in &game.mobs {
            assert_eq!(game.mob_defs.get(zombie.kind).name, "zombie");
            assert_eq!(zombie.feet().y, 64.0, "on the ground");
            assert!(zombie.feet().distance(game.body.position()) >= 24.0);
        }
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
        assert!(!game.mobs.is_empty(), "some pigs");
        assert!(game.mobs.iter().all(|pig| pig.feet().y == 64.0));
        let pigs = game.mobs.len();
        game.on_chunks_loaded(&chunks);
        assert_eq!(game.mobs.len(), pigs, "a chunk gets its animals once");
        let saved = game.saved_mobs();
        assert_eq!(saved.len(), pigs);
        assert!(
            saved
                .iter()
                .all(|mob| mob.name == "pig" && mob.health == 10)
        );
    }
}
