//! Mobs in the game: running their AI and movement, spawning them (animals
//! when a chunk is first generated, monsters in the dark near the player
//! and sea creatures in the water, after 1.0.0's `SpawnerAnimals` with
//! modern kinds and biomes), burning undead in daylight, despawning, and
//! keeping animals while their chunk is unloaded. What mobs do beyond
//! walking (shooting, exploding, teleporting) is in `mob_acts.rs`, and what
//! the player can do to animals in `animals.rs`.

use glam::{DVec3, IVec3};

use super::Game;
use crate::{
    entity::{
        ai::{self, Env, Other, Target, View},
        mob::{Mob, MobData},
        mobs::{Ai, MobKind, Spawn},
        path::Terrain,
    },
    level::SavedMob,
    player::{collision::WorldCollider, raycast, stats::DamageSource},
    world::{
        World,
        biome::Biome,
        block::{BlockId, BlockRegistry, Fluid},
        chunk::ChunkPos,
        daytime, shape,
        worldgen::rng::Rng,
    },
};

/// Monsters don't spawn closer than this to the player (1.0.0).
const MIN_SPAWN_DISTANCE: f64 = 24.0;
/// Chunks this far from the player (Chebyshev) take part in spawning.
const SPAWN_CHUNK_RADIUS: i32 = 8;
/// 1.0.0's monster cap: this many per 256 eligible chunks.
const MONSTERS_PER_256_CHUNKS: usize = 70;
/// The cap on sea creatures (modern: 5 squid, 20 fish; one cap here).
const WATER_MOBS_PER_256_CHUNKS: usize = 20;
/// Despawning (1.0.0's `despawnEntity`): instantly this far away, at
/// random after this many ticks when farther than `NEAR`.
const DESPAWN_FAR: f64 = 128.0;
const DESPAWN_NEAR: f64 = 32.0;
const DESPAWN_AGE: u32 = 600;
const DESPAWN_CHANCE: f32 = 1.0 / 800.0;
/// Bodies this close sideways push each other (1.0.0's `expand(0.2, 0, 0.2)`).
const PUSH_REACH: f64 = 0.2;
/// Strength of that push.
const PUSH: f64 = 0.05;
/// A mob looks for the player only this close.
const SIGHT_RANGE: f64 = 40.0;
/// Mobs farther than this from the player aren't drawn or thought about
/// for others' sake (the list of nearby mobs each one considers).
const NEIGHBOR_RANGE: f64 = 20.0;
/// A player must have gone this long (three days) without sleeping for
/// phantoms to come.
const PHANTOM_AFTER: u32 = 72_000;
/// Ticks of fire daylight gives undead (1.0.0's `setFire(8)`).
const DAYLIGHT_FIRE: i32 = 160;
/// Sheep colors by chance, as in vanilla (wool color numbers).
const SHEEP_GRAY: u8 = 7;
const SHEEP_LIGHT_GRAY: u8 = 8;
const SHEEP_BLACK: u8 = 15;
const SHEEP_BROWN: u8 = 12;
const SHEEP_PINK: u8 = 6;

/// The push `a` gets away from `b` (and `b` gets the opposite), after
/// 1.0.0's `applyEntityCollision`.
fn push_between(a: DVec3, b: DVec3) -> DVec3 {
    let (mut dx, mut dz) = (a.x - b.x, a.z - b.z);
    let mut d = dx.abs().max(dz.abs());
    if d < 0.01 {
        return DVec3::ZERO;
    }
    d = d.sqrt();
    dx /= d;
    dz /= d;
    let f = (1.0 / d).min(1.0);
    DVec3::new(dx * f * PUSH, 0.0, dz * f * PUSH)
}

/// The loaded world as mobs' path finding sees it.
pub(super) struct WorldTerrain<'a> {
    pub world: &'a World,
    pub blocks: &'a BlockRegistry,
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

/// What the other mobs need to know about one.
struct Snap {
    feet: DVec3,
    eye: DVec3,
    width: f64,
    monster: bool,
    creeper: bool,
    prey: bool,
    cat: bool,
    dead: bool,
}

impl Game {
    /// Light at `cell` as mobs feel it: the brighter of block light and
    /// sky light dimmed by the time of day. `None` if unknown.
    pub(super) fn light_level(&self, cell: IVec3) -> Option<u8> {
        let (sky, block) = self.chunks.light_at(cell.x, cell.y, cell.z)?;
        let subtracted = daytime::sky_light_subtracted(self.celestial_angle());
        Some(sky.saturating_sub(subtracted).max(block))
    }

    pub(super) fn celestial_angle(&self) -> f32 {
        daytime::celestial_angle(self.time, 0.0)
    }

    /// Whether nothing solid lies between `from` and `to`: slabs, stairs
    /// and fences block a line of sight as they block movement (modern
    /// `hasLineOfSight`, which clips against collision shapes).
    pub(super) fn sees(&self, from: DVec3, to: DVec3) -> bool {
        let world = self.chunks.world();
        let distance = from.distance(to);
        let hit = raycast::raycast_boxes(from, to - from, distance, |cell| {
            match world.block(cell.x, cell.y, cell.z) {
                Some(state) => shape::shapes().collision(state),
                None => &[crate::world::shape::Cuboid::FULL],
            }
        });
        hit.is_none()
    }

    /// Gives a new mob its id and adds it to the world.
    pub(super) fn add_mob(&mut self, mut mob: Mob) -> u64 {
        self.next_mob_id += 1;
        mob.id = self.next_mob_id;
        self.mobs.push(mob);
        self.next_mob_id
    }

    /// A mob of `kind` with the random traits its kind has: a sheep's
    /// color, a cat's or horse's coat, a slime's size, the time to the
    /// next egg.
    pub(super) fn new_mob(&mut self, kind: MobKind, feet: DVec3, yaw: f32) -> Mob {
        let def = self.mob_defs.get(kind);
        let mut data = MobData::default();
        let roll = self.rng.next_f64();
        match def.name.as_str() {
            "sheep" => {
                data.color = match roll {
                    r if r < 0.81836 => 0,
                    r if r < 0.86836 => SHEEP_BLACK,
                    r if r < 0.91836 => SHEEP_GRAY,
                    r if r < 0.96836 => SHEEP_LIGHT_GRAY,
                    r if r < 0.99836 => SHEEP_BROWN,
                    _ => SHEEP_PINK,
                };
            }
            "slime" | "magma_cube" => data.size = 1 << self.rng.below(3),
            "chicken" => data.timer = 6000 + self.rng.below(6000),
            _ => {}
        }
        if !matches!(def.name.as_str(), "wolf") && !def.variants.is_empty() {
            data.variant = self.rng.below(def.variants.len() as u32 + 1) as u8;
        }
        Mob::with_data(kind, def, feet, yaw, data)
    }

    /// One tick of every mob: AI, movement, daylight, hits and shots, drops,
    /// despawning and stashing mobs whose chunk unloaded.
    pub(super) fn tick_mobs(&mut self) {
        let player_feet = self.body.position();
        let player_eye = player_feet + DVec3::Y * self.body.eye_height();
        let attackable = self.mode.is_vulnerable() && !self.stats.is_dead();
        let daytime = daytime::is_daytime(self.celestial_angle());
        let ridden_input = self.ridden_input();
        let mut mobs = std::mem::take(&mut self.mobs);
        let defs = self.mob_defs.clone();
        let mut thinking = Rng::new(self.rng.next_u64());
        self.threat_ticks = self.threat_ticks.saturating_sub(1);
        let snaps: Vec<Snap> = mobs
            .iter()
            .map(|mob| {
                let def = defs.get(mob.kind);
                Snap {
                    feet: mob.feet(),
                    eye: mob.eye(),
                    width: mob.size().0,
                    monster: def.ai.is_monster(),
                    creeper: matches!(def.ai, Ai::Creeper { .. }),
                    prey: matches!(def.name.as_str(), "chicken" | "rabbit"),
                    cat: matches!(def.ai, Ai::Cat),
                    dead: mob.is_dead(),
                }
            })
            .collect();
        let look = self.camera.forward().as_dvec3();
        let threat = (self.threat_ticks > 0)
            .then(|| mobs.iter().position(|m| m.id == self.threat_id))
            .flatten();
        let mut pending = Vec::new();
        for (index, mob) in mobs.iter_mut().enumerate() {
            let def = defs.get(mob.kind);
            if mob.ridden {
                // A ridden horse is steered by the player.
                self.steer_ridden(mob);
            }
            let eye = mob.eye();
            // Only mobs that go after the player look for them, and only
            // when they're near enough to notice.
            let hunts = !matches!(
                def.ai,
                Ai::Passive
                    | Ai::Villager
                    | Ai::Horse
                    | Ai::Rabbit
                    | Ai::Swimmer
                    | Ai::Cat
                    | Ai::Fox { .. }
            );
            let near_player = eye.distance(player_eye) < SIGHT_RANGE;
            let visible = attackable && hunts && near_player && self.sees(eye, player_eye);
            let stares = matches!(def.ai, Ai::Enderman { .. }) && {
                let to = eye - player_eye;
                let d = to.length();
                d < 64.0 && d > 0.0 && look.dot(to / d) > 1.0 - 0.025 / d && visible
            };
            let target = (attackable && hunts && near_player).then(|| Target {
                feet: player_feet,
                eye: player_eye,
                aabb: self.body.aabb(),
                visible,
                stares,
            });
            // Only these look at their neighbors.
            let social = matches!(
                def.ai,
                Ai::Neutral { .. } | Ai::Wolf { .. } | Ai::Fox { .. } | Ai::Creeper { .. }
            );
            let others: Vec<Other> = if social {
                snaps
                    .iter()
                    .enumerate()
                    .filter(|&(i, s)| {
                        i != index && !s.dead && s.feet.distance(mob.feet()) < NEIGHBOR_RANGE
                    })
                    .map(|(i, s)| Other {
                        index: i,
                        feet: s.feet,
                        eye: s.eye,
                        monster: s.monster,
                        creeper: s.creeper,
                        prey: s.prey,
                        cat: s.cat,
                        visible: self.sees(eye, s.eye),
                        width: s.width,
                    })
                    .collect()
            } else {
                Vec::new()
            };
            let terrain = WorldTerrain {
                world: self.chunks.world(),
                blocks: self.items.blocks,
            };
            let weight = |cell: IVec3| self.spot_weight(def, cell);
            let in_water = self.fluid_at(mob.feet() + DVec3::Y * 0.2) == Some(Fluid::Water);
            let env = Env {
                brightness: self
                    .light_level(mob.cell())
                    .map_or(0.0, daytime::brightness),
                day: daytime,
                in_water,
            };
            let view = View {
                terrain: &terrain,
                player: target,
                weight: &weight,
                env,
                others: &others,
                owner: Some(player_feet),
                threat,
            };
            let mut random = || thinking.next_f64() as f32;
            let mut intent = if mob.ridden {
                Default::default()
            } else {
                ai::think(mob, def, &view, &mut random)
            };
            if mob.ridden {
                intent.input = ridden_input;
            }
            let collider = WorldCollider {
                world: self.chunks.world(),
                blocks: self.items.blocks,
            };
            let eyes = self.fluid_at(mob.eye()) == Some(Fluid::Water);
            mob.body.free_flight = def.flies || (def.swims && in_water);
            if !mob.body.free_flight {
                mob.body.may_fly = false;
            }
            mob.tick(&intent.input, &collider, eyes);
            if def.climbs && mob.body.collided_horizontally() && intent.input.forward > 0.0 {
                mob.body.velocity.y = 0.2;
                mob.stats.fall_distance = 0.0;
            }
            if def.no_fall {
                mob.stats.fall_distance = 0.0;
            }
            if def.burns_in_daylight && daytime && mob.body.liquid.is_none() {
                self.maybe_ignite(mob);
            }
            // Water hurts endermen; sea creatures out of it suffocate.
            if (def.hurt_by_water && in_water)
                || (def.swims && !in_water && mob.age.is_multiple_of(20))
            {
                mob.hurt_by_world(1.0, DamageSource::Drowning);
            }
            pending.push((index, intent));
            if mob.is_dead() && !mob.looted {
                mob.looted = true;
                self.on_mob_death(mob);
            }
        }
        self.apply_intents(&mut mobs, pending, player_feet);
        self.push_apart(&mut mobs);
        // Despawn far monsters; stash animals whose chunk unloaded.
        mobs.retain_mut(|mob| {
            let def = defs.get(mob.kind);
            let distance = mob.feet().distance(player_feet);
            if mob.is_gone() {
                return false;
            }
            let pos = ChunkPos::of_block(mob.feet().x.floor() as i32, mob.feet().z.floor() as i32);
            if self.chunks.world().get(pos).is_none() {
                if def.ai.is_creature() {
                    self.stashed_mobs.entry(pos).or_default().push(mob.clone());
                }
                return false;
            }
            if def.ai.is_monster() && !mob.ridden {
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
        // Mobs spawned while thinking (split slimes, chain reactions).
        mobs.append(&mut self.mobs);
        self.mobs = mobs;
        self.tick_animals();
        self.spawn_monsters();
        self.spawn_water_mobs();
        self.spawn_phantoms();
    }

    /// Phantoms come out of the night sky above a player who hasn't slept
    /// in three days (modern `PhantomSpawner`): every three seconds, with
    /// a chance that grows with the time awake, one to three of them 20
    /// to 34 blocks up.
    pub(super) fn spawn_phantoms(&mut self) {
        if !self.time.is_multiple_of(60)
            || self.stats.is_dead()
            || !self.mode.is_vulnerable()
            || daytime::is_daytime(self.celestial_angle())
        {
            return;
        }
        let Some(kind) = self
            .mob_defs
            .iter()
            .find_map(|(kind, def)| matches!(def.spawn, Spawn::Phantom).then_some(kind))
        else {
            return;
        };
        let feet = self.body.position();
        let head = (feet + DVec3::Y * 1.0).floor().as_ivec3();
        let open_sky = self
            .chunks
            .light_at(head.x, head.y, head.z)
            .is_some_and(|(sky, _)| sky == 15);
        if !open_sky || self.rng.below(self.stats.awake.max(1)) < PHANTOM_AFTER {
            return;
        }
        let count = 1 + self.rng.below(3);
        for _ in 0..count {
            let spot = feet
                + DVec3::new(
                    f64::from(self.rng.below(21)) - 10.0,
                    20.0 + f64::from(self.rng.below(15)),
                    f64::from(self.rng.below(21)) - 10.0,
                );
            let cell = spot.floor().as_ivec3();
            let free = self
                .chunks
                .world()
                .block(cell.x, cell.y, cell.z)
                .is_some_and(|b| b.is_air());
            if free {
                let yaw = self.rng.next_f64() as f32 * std::f32::consts::TAU;
                let mob = self.new_mob(kind, spot, yaw);
                self.add_mob(mob);
            }
        }
    }

    /// 1.0.0's `applyEntityCollision`: overlapping living bodies (mobs and
    /// the player) nudge each other apart.
    fn push_apart(&mut self, mobs: &mut [Mob]) {
        let reach = DVec3::new(PUSH_REACH, 0.0, PUSH_REACH);
        for i in 0..mobs.len() {
            if mobs[i].is_dead() {
                continue;
            }
            let near = |bb: crate::player::collision::Aabb| {
                crate::player::collision::Aabb::new(bb.min - reach, bb.max + reach)
            };
            if !self.body.flying
                && !mobs[i].ridden
                && near(mobs[i].body.aabb()).intersects(&self.body.aabb())
            {
                let push = push_between(mobs[i].feet(), self.body.position());
                mobs[i].body.velocity += push;
                self.body.velocity -= push;
            }
            for j in i + 1..mobs.len() {
                if !mobs[j].is_dead() && near(mobs[i].body.aabb()).intersects(&mobs[j].body.aabb())
                {
                    let push = push_between(mobs[i].feet(), mobs[j].feet());
                    mobs[i].body.velocity += push;
                    mobs[j].body.velocity -= push;
                }
            }
        }
    }

    /// How much a mob likes `cell` to wander to: animals like the ground
    /// they spawn on (grass) and light, monsters like the dark (1.0.0's
    /// `getBlockPathWeight`).
    fn spot_weight(&self, def: &crate::entity::mobs::MobDef, cell: IVec3) -> f32 {
        let brightness = self.light_level(cell).map_or(0.0, daytime::brightness);
        if def.ai.is_monster() {
            return 0.5 - brightness;
        }
        let below = self.chunks.world().block(cell.x, cell.y - 1, cell.z);
        let liked = match &def.spawn {
            Spawn::Ground { blocks, .. } => below.is_some_and(|b| blocks.contains(&b.id())),
            _ => false,
        };
        if liked { 10.0 } else { brightness - 0.5 }
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

    /// Chunks entering the world: schedule their flowing fluids, bring
    /// back their stashed animals, and place animals in chunks seen for
    /// the first time.
    pub fn on_chunks_loaded(&mut self, chunks: &[ChunkPos]) {
        self.schedule_fluids(chunks);
        self.queue_post_process(chunks);
        for &pos in chunks {
            if let Some(mobs) = self.stashed_mobs.remove(&pos) {
                for mob in mobs {
                    self.add_mob(mob);
                }
            }
            if self.populated.insert(pos) {
                self.populate(pos);
            }
        }
    }

    /// The biome at a block column.
    fn biome_at(&self, x: i32, z: i32) -> Biome {
        self.chunks.world().biome_at(x, z).unwrap_or_default()
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
                ref biomes,
                ..
            } = self.mob_defs.get(kind).spawn
            else {
                continue;
            };
            let (blocks, biomes) = (blocks.clone(), biomes.clone());
            if self.rng.next_f64() as f32 >= chance {
                continue;
            }
            let (min_x, min_z) = pos.min_block();
            let mut x = min_x + (self.rng.next_f64() * 16.0) as i32;
            let mut z = min_z + (self.rng.next_f64() * 16.0) as i32;
            if !biomes.is_empty() && !biomes.contains(&self.biome_at(x, z)) {
                continue;
            }
            for _ in 0..group {
                if let Some(feet) = self.ground_spot(x, z, &blocks) {
                    let yaw = (self.rng.next_f64() as f32) * std::f32::consts::TAU;
                    let mob = self.new_mob(kind, feet, yaw);
                    self.add_mob(mob);
                }
                x += (self.rng.next_f64() * 5.0) as i32 - (self.rng.next_f64() * 5.0) as i32;
                z += (self.rng.next_f64() * 5.0) as i32 - (self.rng.next_f64() * 5.0) as i32;
            }
        }
    }

    /// Feet position on top of column (x, z) if its surface is one of
    /// `ground` with two free blocks above.
    fn ground_spot(&self, x: i32, z: i32, ground: &[BlockId]) -> Option<DVec3> {
        let world = self.chunks.world();
        let top = (0..crate::config::WORLD_HEIGHT as i32)
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

    /// The monsters near the player, and how many may be (the cap grows with
    /// the chunks in range).
    fn monster_count(&self) -> usize {
        self.mobs
            .iter()
            .filter(|mob| self.mob_defs.get(mob.kind).ai.is_monster() && !mob.is_dead())
            .count()
    }

    /// 1.0.0's `findChunksForSpawning` for monsters: in each chunk near the
    /// player, a random spot that's dark, free and on solid ground, at
    /// least 24 blocks away, up to the cap. The kind is picked among those
    /// that spawn in the spot's biome by weight, and arrives with a pack.
    pub(super) fn spawn_monsters(&mut self) {
        let player = self.body.position();
        let center = ChunkPos::of_block(player.x.floor() as i32, player.z.floor() as i32);
        let side = SPAWN_CHUNK_RADIUS * 2 + 1;
        let cap = MONSTERS_PER_256_CHUNKS * (side * side) as usize / 256;
        for dz in -SPAWN_CHUNK_RADIUS..=SPAWN_CHUNK_RADIUS {
            for dx in -SPAWN_CHUNK_RADIUS..=SPAWN_CHUNK_RADIUS {
                if self.monster_count() >= cap {
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
                let biome = self.biome_at(cell.x, cell.z);
                let Some((kind, max_light, group)) = self.pick_dark_kind(biome, cell, pos) else {
                    continue;
                };
                for _ in 0..group {
                    cell.x +=
                        (self.rng.next_f64() * 6.0) as i32 - (self.rng.next_f64() * 6.0) as i32;
                    cell.z +=
                        (self.rng.next_f64() * 6.0) as i32 - (self.rng.next_f64() * 6.0) as i32;
                    if self.can_monster_spawn(cell, max_light) {
                        let feet = cell.as_dvec3() + DVec3::new(0.5, 0.0, 0.5);
                        let yaw = self.rng.next_f64() as f32 * std::f32::consts::TAU;
                        let mob = self.new_mob(kind, feet, yaw);
                        self.add_mob(mob);
                    }
                }
            }
        }
    }

    /// A kind that can spawn here by weight: (kind, max light, group size).
    pub(super) fn pick_dark_kind(
        &mut self,
        biome: Biome,
        cell: IVec3,
        chunk: ChunkPos,
    ) -> Option<(MobKind, u8, u8)> {
        let seed = self.chunks.seed().unwrap_or(0);
        let mut options: Vec<(MobKind, u8, u8, u32)> = Vec::new();
        for (kind, def) in self.mob_defs.iter() {
            match def.spawn {
                Spawn::Dark {
                    max_light,
                    group,
                    ref biomes,
                    weight,
                } if biomes.is_empty() || biomes.contains(&biome) => {
                    options.push((kind, max_light, group, weight));
                }
                // Slimes come out of the ground in slime chunks, deep down.
                Spawn::SlimeChunk { below } if cell.y < below && is_slime_chunk(seed, chunk) => {
                    options.push((kind, 15, 1, 100));
                }
                _ => {}
            }
        }
        let total: u32 = options.iter().map(|o| o.3).sum();
        if total == 0 {
            return None;
        }
        let mut roll = self.rng.below(total);
        for (kind, max_light, group, weight) in options {
            if roll < weight {
                return Some((kind, max_light, group));
            }
            roll -= weight;
        }
        None
    }

    /// Solid ground below, two free cells (no liquid), far enough from the
    /// player and dark enough (1.0.0's `EntityMob.getCanSpawnHere`).
    fn can_monster_spawn(&mut self, cell: IVec3, max_light: u8) -> bool {
        let terrain = WorldTerrain {
            world: self.chunks.world(),
            blocks: self.items.blocks,
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

    /// Squid and fish: in each chunk near the player, a random spot in deep
    /// water, now and then, up to a cap.
    fn spawn_water_mobs(&mut self) {
        if !self.time.is_multiple_of(20) {
            return;
        }
        let player = self.body.position();
        let center = ChunkPos::of_block(player.x.floor() as i32, player.z.floor() as i32);
        let side = SPAWN_CHUNK_RADIUS * 2 + 1;
        let cap = WATER_MOBS_PER_256_CHUNKS * (side * side) as usize / 256;
        let count = |game: &Game| {
            game.mobs
                .iter()
                .filter(|m| game.mob_defs.get(m.kind).swims && !m.is_dead())
                .count()
        };
        for dz in -SPAWN_CHUNK_RADIUS..=SPAWN_CHUNK_RADIUS {
            for dx in -SPAWN_CHUNK_RADIUS..=SPAWN_CHUNK_RADIUS {
                if count(self) >= cap {
                    return;
                }
                let pos = center.offset(dx, dz);
                if self.chunks.world().get(pos).is_none() {
                    continue;
                }
                let (min_x, min_z) = pos.min_block();
                let cell = IVec3::new(
                    min_x + (self.rng.next_f64() * 16.0) as i32,
                    45 + (self.rng.next_f64() * 18.0) as i32,
                    min_z + (self.rng.next_f64() * 16.0) as i32,
                );
                let biome = self.biome_at(cell.x, cell.z);
                let kinds: Vec<(MobKind, f32, u8, u8)> = self
                    .mob_defs
                    .iter()
                    .filter_map(|(kind, def)| match def.spawn {
                        Spawn::Water {
                            chance,
                            group,
                            ref biomes,
                            min_depth,
                        } if biomes.is_empty() || biomes.contains(&biome) => {
                            Some((kind, chance, group, min_depth))
                        }
                        _ => None,
                    })
                    .collect();
                if kinds.is_empty() {
                    continue;
                }
                let (kind, chance, group, min_depth) =
                    kinds[self.rng.below(kinds.len() as u32) as usize];
                if (self.rng.next_f64() as f32) >= chance {
                    continue;
                }
                let terrain = WorldTerrain {
                    world: self.chunks.world(),
                    blocks: self.items.blocks,
                };
                let deep = (0..=min_depth as i32).all(|dy| terrain.water(cell + IVec3::Y * dy));
                if !deep || cell.as_dvec3().distance(player) < 16.0 {
                    continue;
                }
                for i in 0..group {
                    let at = cell + IVec3::new(i as i32 % 2, 0, i as i32 / 2);
                    let terrain = WorldTerrain {
                        world: self.chunks.world(),
                        blocks: self.items.blocks,
                    };
                    if terrain.water(at) && terrain.water(at + IVec3::Y) {
                        let feet = at.as_dvec3() + DVec3::new(0.5, 0.0, 0.5);
                        let yaw = self.rng.next_f64() as f32 * std::f32::consts::TAU;
                        let mob = self.new_mob(kind, feet, yaw);
                        self.add_mob(mob);
                    }
                }
            }
        }
    }

    /// Every mob worth keeping (animals, loaded or stashed), for saving.
    pub(super) fn saved_mobs(&self) -> Vec<SavedMob> {
        let stashed = self.stashed_mobs.values().flatten();
        self.mobs
            .iter()
            .chain(stashed)
            .filter(|mob| !mob.is_dead() && self.mob_defs.get(mob.kind).ai.is_creature())
            .map(|mob| SavedMob {
                name: self.mob_defs.get(mob.kind).name.clone(),
                position: mob.feet().to_array(),
                yaw: mob.yaw,
                health: mob.stats.health,
                data: mob.data.clone(),
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
            let mut mob = Mob::with_data(
                kind,
                self.mob_defs.get(kind),
                feet,
                entry.yaw,
                entry.data.clone(),
            );
            mob.stats.health = entry.health;
            let pos = ChunkPos::of_block(feet.x.floor() as i32, feet.z.floor() as i32);
            self.stashed_mobs.entry(pos).or_default().push(mob);
        }
    }
}

/// Whether `chunk` is one of the tenth of chunks where slimes spawn
/// underground, picked by the seed as 1.0.0's `isSlimeChunk` does.
pub fn is_slime_chunk(seed: u64, chunk: ChunkPos) -> bool {
    let (x, z) = (i64::from(chunk.x), i64::from(chunk.z));
    let mixed =
        seed as i64 + (x * x * 4_987_142) + (x * 5_947_611) + (z * z) * 4_392_871 + (z * 389_711);
    let mut rng = Rng::new((mixed ^ 987_234_911) as u64);
    rng.below(10) == 0
}
