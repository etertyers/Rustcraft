//! `--spawn-test --seed <n>`: one in-game night in the plains with no
//! rendering, printing the monsters and animals loaded within 128 blocks of
//! the player every 1,000 ticks (and how long the ticks took).

use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use anyhow::{Result, bail};
use glam::DVec3;

use super::Game;
use crate::{
    entity::mobs::{MobRegistry, Spawn},
    inventory::{crafting::Recipes, stack::Items},
    player::mode::GameMode,
    world::{
        biome::{Biome, BiomeColors},
        chunk::ChunkPos,
        manager::{ChunkManager, ChunkSource},
        mesh::BlockModels,
        worldgen::terrain::Terrain,
    },
};

/// Dusk: the night is the 12,000 ticks from here.
const START_TIME: u64 = 12_000;
const NIGHT_TICKS: u32 = 12_000;
const REPORT_EVERY: u32 = 1_000;
/// The distance mobs are counted within.
const COUNT_RADIUS: f64 = 128.0;

/// Finds a column in a stretch of mostly plains (at least 80% of a 7×7
/// sample 24 blocks apart), searching outward from the origin on a grid.
fn plains_spot(terrain: &Terrain) -> Option<(i32, i32)> {
    let share = |x: i32, z: i32| {
        let plains = (-3..=3)
            .flat_map(|dz| (-3..=3).map(move |dx| (dx, dz)))
            .filter(|&(dx, dz)| terrain.biome_at(x + dx * 24, z + dz * 24) == Biome::Plains)
            .count();
        plains as f64 / 49.0
    };
    (0..100i32).find_map(|ring| {
        (-ring..=ring)
            .flat_map(|i| [(i, -ring), (i, ring), (-ring, i), (ring, i)])
            .map(|(i, j)| (i * 160, j * 160))
            .find(|&(x, z)| share(x, z) >= 0.8)
    })
}

/// Runs the night and prints the counts.
pub fn run(seed: u64) -> Result<()> {
    let terrain = Terrain::new(seed);
    let Some((x, z)) = plains_spot(&terrain) else {
        bail!("no wide plains near the origin with seed {seed}");
    };
    let feet = DVec3::new(
        x as f64 + 0.5,
        terrain.height(x, z) as f64 + 1.0,
        z as f64 + 0.5,
    );
    let assets = crate::assets::dir();
    let models = BlockModels::new(
        crate::world::block::registry(),
        |_| Some([0.0; 4]),
        [0.0; 4],
        BiomeColors::uniform(),
    );
    let recipes = Recipes::load(&assets.join("data/recipes.ron"))?;
    let items = Items::new().with_recipes(Arc::new(recipes));
    let mobs = Arc::new(MobRegistry::load(&assets.join("data/mobs.ron"))?);
    let chunks = ChunkManager::new(
        ChunkSource::Terrain(Box::new(terrain)),
        Arc::new(models),
        9,
        4,
    );
    let mut game = Game::new(chunks, items, feet, feet, 0.0, 0.0).with_mobs(mobs);
    game.set_time_and_mode(START_TIME, GameMode::Creative);
    let center = ChunkPos::of_block(x, z);

    println!("seed {seed}, plains at ({x}, {z}), one night from tick {START_TIME}");
    println!("tick  monsters  animals  chunks  ms/tick");
    let deadline = Instant::now() + Duration::from_secs(120);
    while game.chunks.world().len() < 100 {
        if Instant::now() > deadline {
            bail!("chunks never loaded");
        }
        let changes = game.chunks.update(center);
        game.on_chunks_loaded(&changes.loaded);
        std::thread::sleep(Duration::from_millis(5));
    }
    let mut spent = Duration::ZERO;
    for tick in 1..=NIGHT_TICKS {
        let started = Instant::now();
        let changes = game.chunks.update(center);
        game.on_chunks_loaded(&changes.loaded);
        game.advance(0.05);
        spent += started.elapsed();
        if tick % REPORT_EVERY == 0 {
            let (monsters, animals) = game.mob_counts(COUNT_RADIUS);
            println!(
                "{tick:>5}  {monsters:>8}  {animals:>7}  {:>6}  {:>7.2}",
                game.chunks.world().len(),
                spent.as_secs_f64() * 1000.0 / f64::from(REPORT_EVERY),
            );
            spent = Duration::ZERO;
        }
    }
    Ok(())
}

impl Game {
    /// Monsters and animals loaded within `radius` blocks of the player.
    pub fn mob_counts(&self, radius: f64) -> (usize, usize) {
        let player = self.body.position();
        let (mut monsters, mut animals) = (0, 0);
        for mob in self.mobs.iter().filter(|m| !m.is_dead()) {
            if mob.feet().distance(player) > radius {
                continue;
            }
            let def = self.mob_defs.get(mob.kind);
            if def.ai.is_monster() {
                monsters += 1;
            } else if matches!(def.spawn, Spawn::Ground { .. }) {
                animals += 1;
            }
        }
        (monsters, animals)
    }
}
