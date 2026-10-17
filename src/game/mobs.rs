//! Mobs in the game: running their AI and movement, spawning them (pigs
//! when a chunk is first generated, zombies in the dark near the player,
//! after 1.0.0's `SpawnerAnimals`), burning zombies in daylight,
//! despawning, and keeping pigs while their chunk is unloaded.

use glam::{DVec3, IVec3};

use super::Game;
use crate::{
    entity::{
        ai::{self, Target, View},
        item::ItemEntity,
        mob::{Mob, knock_back},
        mobs::{Ai, MobKind, Spawn},
        path::Terrain,
    },
    level::SavedMob,
    player::{collision::WorldCollider, raycast, stats::DamageSource},
    world::{
        World,
        block::{BlockRegistry, Fluid},
        chunk::ChunkPos,
        daytime,
        worldgen::rng::Rng,
    },
};

/// Monsters don't spawn closer than this to the player (1.0.0).
const MIN_SPAWN_DISTANCE: f64 = 24.0;
/// Chunks this far from the player (Chebyshev) take part in spawning.
const SPAWN_CHUNK_RADIUS: i32 = 8;
/// 1.0.0's monster cap: this many per 256 eligible chunks.
const MONSTERS_PER_256_CHUNKS: usize = 70;
/// Despawning (1.0.0's `despawnEntity`): instantly this far away, at
/// random after this many ticks when farther than `NEAR`.
const DESPAWN_FAR: f64 = 128.0;
const DESPAWN_NEAR: f64 = 32.0;
const DESPAWN_AGE: u32 = 600;
const DESPAWN_CHANCE: f32 = 1.0 / 800.0;
/// Ticks of fire daylight gives a zombie (1.0.0's `setFire(8)`).
const DAYLIGHT_FIRE: i32 = 160;

/// The loaded world as mobs' path finding sees it.
struct WorldTerrain<'a> {
    world: &'a World,
    blocks: &'a BlockRegistry,
}

impl WorldTerrain<'_> {
    fn def(&self, cell: IVec3) -> Option<&crate::world::block::BlockDef> {
        let block = self.world.block(cell.x, cell.y, cell.z)?;
        self.blocks.get(block.id())
    }
}

impl Terrain for WorldTerrain<'_> {
    fn passable(&self, cell: IVec3) -> bool {
        self.def(cell).is_some_and(|def| !def.solid)
    }
    fn water(&self, cell: IVec3) -> bool {
        self.def(cell)
            .is_some_and(|def| def.fluid == Some(Fluid::Water))
    }
    fn lava(&self, cell: IVec3) -> bool {
        self.def(cell)
            .is_some_and(|def| def.fluid == Some(Fluid::Lava))
    }
}

impl Game {
    /// Light at `cell` as mobs feel it: the brighter of block light and
    /// sky light dimmed by the time of day. `None` if unknown.
    fn light_level(&self, cell: IVec3) -> Option<u8> {
        let (sky, block) = self.chunks.light_at(cell.x, cell.y, cell.z)?;
        let subtracted = daytime::sky_light_subtracted(self.celestial_angle());
        Some(sky.saturating_sub(subtracted).max(block))
    }

    fn celestial_angle(&self) -> f32 {
        daytime::celestial_angle(self.time, 0.0)
    }

    /// Whether nothing opaque lies between `from` and `to`.
    fn clear_line(&self, from: DVec3, to: DVec3) -> bool {
        let world = self.chunks.world();
        let blocks = &self.items.blocks;
        let distance = from.distance(to);
        let hit = raycast::raycast(from, to - from, distance, |cell| {
            world
                .block(cell.x, cell.y, cell.z)
                .and_then(|block| blocks.get(block.id()))
                .is_none_or(|def| def.is_opaque_cube())
        });
        hit.is_none()
    }

    /// One tick of every mob: AI, movement, daylight, hits on the player,
    /// drops, despawning and stashing mobs whose chunk unloaded.
    pub(super) fn tick_mobs(&mut self) {
        let player_feet = self.body.position();
        let player_eye = player_feet + DVec3::Y * self.body.eye_height();
        let attackable = self.mode.is_vulnerable() && !self.stats.is_dead();
        let daytime = daytime::is_daytime(self.celestial_angle());
        let mut mobs = std::mem::take(&mut self.mobs);
        let defs = self.mob_defs.clone();
        let mut thinking = Rng::new(self.rng.next_u64());
        for mob in &mut mobs {
            let def = defs.get(mob.kind);
            let target = attackable.then(|| Target {
                feet: player_feet,
                aabb: self.body.aabb(),
                visible: self.clear_line(mob.eye(), player_eye),
            });
            let terrain = WorldTerrain {
                world: self.chunks.world(),
                blocks: &self.items.blocks,
            };
            let weight = |cell: IVec3| self.spot_weight(&def.spawn, def.ai, cell);
            let view = View {
                terrain: &terrain,
                player: target,
                weight: &weight,
            };
            let mut random = || thinking.next_f64() as f32;
            let intent = ai::think(mob, def, &view, &mut random);
            let collider = WorldCollider {
                world: self.chunks.world(),
                blocks: &self.items.blocks,
            };
            let eyes = self.fluid_at(mob.eye()) == Some(Fluid::Water);
            mob.tick(&intent.input, &collider, eyes);
            if def.burns_in_daylight && daytime && mob.body.liquid.is_none() {
                self.maybe_ignite(mob);
            }
            if let Some(damage) = intent.hit
                && self.stats.damage(damage, DamageSource::Mob)
            {
                knock_back(&mut self.body, mob.feet());
            }
            if mob.is_dead() && !mob.looted {
                mob.looted = true;
                self.drop_loot(mob);
            }
        }
        // Despawn far monsters; stash animals whose chunk unloaded.
        mobs.retain_mut(|mob| {
            let def = defs.get(mob.kind);
            let distance = mob.feet().distance(player_feet);
            if mob.is_gone() {
                return false;
            }
            let pos = ChunkPos::of_block(mob.feet().x.floor() as i32, mob.feet().z.floor() as i32);
            if self.chunks.world().get(pos).is_none() {
                if def.ai == Ai::Passive {
                    self.stashed_mobs.entry(pos).or_default().push(mob.clone());
                }
                return false;
            }
            if def.ai != Ai::Passive {
                if distance > DESPAWN_FAR {
                    return false;
                }
                if distance < DESPAWN_NEAR {
                    mob.age = 0;
                } else if mob.age > DESPAWN_AGE && (self.rng.next_f64() as f32) < DESPAWN_CHANCE {
                    return false;
                }
            }
            true
        });
        self.mobs = mobs;
        self.spawn_monsters();
    }

    /// How much a mob likes `cell` to wander to: animals like the ground
    /// they spawn on (grass) and light, monsters like the dark (1.0.0's
    /// `getBlockPathWeight`).
    fn spot_weight(&self, spawn: &Spawn, ai: Ai, cell: IVec3) -> f32 {
        let brightness = self.light_level(cell).map_or(0.0, daytime::brightness);
        match ai {
            Ai::Passive => {
                let below = self.chunks.world().block(cell.x, cell.y - 1, cell.z);
                let liked = match spawn {
                    Spawn::Ground { blocks, .. } => below.is_some_and(|b| blocks.contains(&b.id())),
                    Spawn::Dark { .. } => false,
                };
                if liked { 10.0 } else { brightness - 0.5 }
            }
            Ai::Hostile { .. } => 0.5 - brightness,
        }
    }

    /// 1.0.0's `EntityZombie`: in bright daylight under open sky, a chance
    /// each tick to catch fire.
    fn maybe_ignite(&mut self, mob: &mut Mob) {
        let head = mob.eye().floor().as_ivec3();
        let Some((sky, _)) = self.chunks.light_at(head.x, head.y, head.z) else {
            return;
        };
        let brightness = self.light_level(head).map_or(0.0, daytime::brightness);
        let roll = self.rng.next_f64() as f32 * 30.0;
        if sky == 15 && brightness > 0.5 && roll < (brightness - 0.4) * 2.0 {
            mob.stats.fire = mob.stats.fire.max(DAYLIGHT_FIRE);
        }
    }

    fn drop_loot(&mut self, mob: &Mob) {
        let def = self.mob_defs.get(mob.kind);
        let rng = &mut self.rng;
        let mut random = || rng.next_f64() as f32;
        let cell = mob.cell();
        for stack in mob.drops(def, &mut random) {
            self.entities
                .push(ItemEntity::from_block(stack, cell, &mut random));
        }
    }

    /// Chunks entering the world: bring back their stashed animals, and
    /// place animals in chunks seen for the first time.
    pub fn on_chunks_loaded(&mut self, chunks: &[ChunkPos]) {
        for &pos in chunks {
            if let Some(mobs) = self.stashed_mobs.remove(&pos) {
                self.mobs.extend(mobs);
            }
            if self.populated.insert(pos) {
                self.populate(pos);
            }
        }
    }

    /// 1.0.0's `performWorldGenSpawning`: maybe a group of animals on
    /// suitable ground in a new chunk.
    fn populate(&mut self, pos: ChunkPos) {
        let kinds: Vec<MobKind> = self.mob_defs.iter().map(|(kind, _)| kind).collect();
        for kind in kinds {
            let Spawn::Ground {
                ref blocks,
                chance,
                group,
                ..
            } = self.mob_defs.get(kind).spawn
            else {
                continue;
            };
            let blocks = blocks.clone();
            if self.rng.next_f64() as f32 >= chance {
                continue;
            }
            let (min_x, min_z) = pos.min_block();
            let mut x = min_x + (self.rng.next_f64() * 16.0) as i32;
            let mut z = min_z + (self.rng.next_f64() * 16.0) as i32;
            for _ in 0..group {
                if let Some(feet) = self.ground_spot(x, z, &blocks) {
                    let yaw = (self.rng.next_f64() as f32) * std::f32::consts::TAU;
                    let mob = Mob::new(kind, self.mob_defs.get(kind), feet, yaw);
                    self.mobs.push(mob);
                }
                x += (self.rng.next_f64() * 5.0) as i32 - (self.rng.next_f64() * 5.0) as i32;
                z += (self.rng.next_f64() * 5.0) as i32 - (self.rng.next_f64() * 5.0) as i32;
            }
        }
    }

    /// Feet position on top of column (x, z) if its surface is one of
    /// `ground` with two free blocks above.
    fn ground_spot(&self, x: i32, z: i32, ground: &[u16]) -> Option<DVec3> {
        let world = self.chunks.world();
        let top = (0..128)
            .rev()
            .find(|&y| world.block(x, y, z).is_some_and(|b| !b.is_air()))?;
        let surface = world.block(x, top, z)?;
        let free = |y| {
            world
                .block(x, y, z)
                .is_some_and(|b: crate::world::block::BlockState| b.is_air())
        };
        (ground.contains(&surface.id()) && free(top + 1) && free(top + 2))
            .then(|| DVec3::new(x as f64 + 0.5, (top + 1) as f64, z as f64 + 0.5))
    }

    /// 1.0.0's `findChunksForSpawning` for monsters: in each chunk near the
    /// player, a random spot that's dark, free and on solid ground, at
    /// least 24 blocks away, up to the cap.
    pub(super) fn spawn_monsters(&mut self) {
        let player = self.body.position();
        let center = ChunkPos::of_block(player.x.floor() as i32, player.z.floor() as i32);
        let kinds: Vec<(MobKind, u8, u8)> = self
            .mob_defs
            .iter()
            .filter_map(|(kind, def)| match def.spawn {
                Spawn::Dark { max_light, group } => Some((kind, max_light, group)),
                _ => None,
            })
            .collect();
        let side = SPAWN_CHUNK_RADIUS * 2 + 1;
        let cap = MONSTERS_PER_256_CHUNKS * (side * side) as usize / 256;
        for (kind, max_light, group) in kinds {
            for dz in -SPAWN_CHUNK_RADIUS..=SPAWN_CHUNK_RADIUS {
                for dx in -SPAWN_CHUNK_RADIUS..=SPAWN_CHUNK_RADIUS {
                    let monsters = self
                        .mobs
                        .iter()
                        .filter(|mob| self.mob_defs.get(mob.kind).ai != Ai::Passive)
                        .count();
                    if monsters >= cap {
                        return;
                    }
                    let pos = center.offset(dx, dz);
                    if self.chunks.world().get(pos).is_none() {
                        continue;
                    }
                    let (min_x, min_z) = pos.min_block();
                    let mut cell = IVec3::new(
                        min_x + (self.rng.next_f64() * 16.0) as i32,
                        (self.rng.next_f64() * 128.0) as i32,
                        min_z + (self.rng.next_f64() * 16.0) as i32,
                    );
                    for _ in 0..group {
                        cell.x +=
                            (self.rng.next_f64() * 6.0) as i32 - (self.rng.next_f64() * 6.0) as i32;
                        cell.z +=
                            (self.rng.next_f64() * 6.0) as i32 - (self.rng.next_f64() * 6.0) as i32;
                        if self.can_monster_spawn(cell, max_light) {
                            let feet = cell.as_dvec3() + DVec3::new(0.5, 0.0, 0.5);
                            let yaw = self.rng.next_f64() as f32 * std::f32::consts::TAU;
                            self.mobs
                                .push(Mob::new(kind, self.mob_defs.get(kind), feet, yaw));
                        }
                    }
                }
            }
        }
    }

    /// Solid ground below, two free cells (no liquid), far enough from the
    /// player and dark enough (1.0.0's `EntityMob.getCanSpawnHere`).
    fn can_monster_spawn(&mut self, cell: IVec3, max_light: u8) -> bool {
        let terrain = WorldTerrain {
            world: self.chunks.world(),
            blocks: &self.items.blocks,
        };
        let free = |c: IVec3| terrain.passable(c) && !terrain.water(c) && !terrain.lava(c);
        let below = cell - IVec3::Y;
        let ground = terrain
            .def(below)
            .is_some_and(|def| def.solid && def.fluid.is_none());
        if !ground || !free(cell) || !free(cell + IVec3::Y) {
            return false;
        }
        let feet = cell.as_dvec3() + DVec3::new(0.5, 0.0, 0.5);
        if feet.distance(self.body.position()) < MIN_SPAWN_DISTANCE {
            return false;
        }
        let Some((sky, block)) = self.chunks.light_at(cell.x, cell.y, cell.z) else {
            return false;
        };
        let subtracted = daytime::sky_light_subtracted(self.celestial_angle());
        let sky = sky.saturating_sub(subtracted);
        if sky as f64 > self.rng.next_f64() * 32.0 {
            return false;
        }
        let level = sky.max(block);
        level <= max_light && (level as f64) <= self.rng.next_f64() * 8.0
    }

    /// Every mob worth keeping (animals, loaded or stashed), for saving.
    pub(super) fn saved_mobs(&self) -> Vec<SavedMob> {
        let stashed = self.stashed_mobs.values().flatten();
        self.mobs
            .iter()
            .chain(stashed)
            .filter(|mob| !mob.is_dead() && self.mob_defs.get(mob.kind).ai == Ai::Passive)
            .map(|mob| SavedMob {
                name: self.mob_defs.get(mob.kind).name.clone(),
                position: mob.feet().to_array(),
                yaw: mob.yaw,
                health: mob.stats.health,
            })
            .collect()
    }

    /// Puts saved mobs in the stash; they appear as their chunks load.
    pub(super) fn restore_mobs(&mut self, saved: &[SavedMob]) {
        for entry in saved {
            let Some(kind) = self.mob_defs.find(&entry.name) else {
                continue;
            };
            let feet = DVec3::from_array(entry.position);
            let mut mob = Mob::new(kind, self.mob_defs.get(kind), feet, entry.yaw);
            mob.stats.health = entry.health;
            let pos = ChunkPos::of_block(feet.x.floor() as i32, feet.z.floor() as i32);
            self.stashed_mobs.entry(pos).or_default().push(mob);
        }
    }
}
