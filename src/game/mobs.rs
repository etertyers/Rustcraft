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
    config::spawning::{self, Category},
    entity::{
        ai::{self, Env, Other, Target, View},
        mob::{Mob, MobData},
        mobs::{Ai, MobKind, Spawn},
        path::Terrain,
    },
    level::SavedMob,
    player::{
        collision::{Collider, LavaFloor, WorldCollider},
        raycast,
        stats::DamageSource,
    },
    world::{
        World,
        biome::Biome,
        block::{BlockId, BlockRegistry, Fluid},
        blocks,
        chunk::ChunkPos,
        daytime, shape,
        worldgen::rng::Rng,
    },
};

/// Monsters don't spawn closer than this to the player (1.0.0).
const MIN_SPAWN_DISTANCE: f64 = spawning::MIN_DISTANCE;
/// Chunks this far from the player (Chebyshev) take part in spawning.
const SPAWN_CHUNK_RADIUS: i32 = spawning::CHUNK_RADIUS;
/// Heights slimes spawn at in swamps (modern y 51..70, above sea level,
/// so the same here).
const SWAMP_SLIMES: std::ops::Range<i32> = 51..70;
/// The cap on sea creatures (modern: 5 squid, 20 fish; one cap here).
const WATER_MOBS_PER_256_CHUNKS: usize = 20;
/// Besides blazes and wither skeletons, what spawns in fortresses: name,
/// weight and group (modern: zombified piglins 5, skeletons 2, magma
/// cubes 3), at block light 11 or less.
const FORTRESS_EXTRAS: [(&str, u32, u8); 3] = [
    ("zombified_piglin", 5, 4),
    ("skeleton", 2, 5),
    ("magma_cube", 3, 4),
];
const FORTRESS_LIGHT: u8 = 11;
/// Striders near the player at most.
const STRIDER_CAP: usize = 8;
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
/// An invisible player is seen only this close (modern scales follow
/// range by 0.07).
const INVISIBLE_SIGHT: f64 = 2.8;
/// A mob looks for the player only this close.
const SIGHT_RANGE: f64 = 40.0;
/// An enderman is angered by a stare from this far (modern 64).
const ENDERMAN_SIGHT: f64 = 64.0;
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
    fn hazard(&self, cell: IVec3) -> bool {
        self.world.block(cell.x, cell.y, cell.z).is_some_and(|s| {
            [
                blocks::FIRE,
                blocks::SOUL_FIRE,
                blocks::CACTUS,
                blocks::SWEET_BERRY_BUSH,
                blocks::POWDER_SNOW,
                blocks::CAMPFIRE,
                blocks::SOUL_CAMPFIRE,
            ]
            .contains(&s.kind())
        })
    }
    fn hot_floor(&self, cell: IVec3) -> bool {
        self.world
            .block(cell.x, cell.y, cell.z)
            .is_some_and(|s| s.kind() == blocks::MAGMA_BLOCK)
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
        daytime::celestial_angle(self.day_time(), 0.0)
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

    /// A mob with a hand and nothing in it picks up a sword or axe lying
    /// within a block of it and holds that from then on (modern mobs that
    /// `canPickUpLoot`), so a skeleton that has lost its bow wields what it
    /// finds.
    fn mobs_pick_up_weapons(&mut self, mobs: &mut [Mob]) {
        if self.entities.is_empty() {
            return;
        }
        let defs = self.mob_defs.clone();
        for mob in mobs.iter_mut() {
            if mob.is_dead()
                || mob.data.held.is_some()
                || !defs.get(mob.kind).model.parts.iter().any(|part| part.hand)
            {
                continue;
            }
            let feet = mob.feet();
            let found = self.entities.iter().position(|item| {
                let name = item.stack.id.name();
                (name.ends_with("_sword") || name.ends_with("_axe"))
                    && !item.pickup_delayed()
                    && (item.center() - feet).with_y(0.0).length() < 1.0
                    && (item.center().y - feet.y).abs() < 1.5
            });
            if let Some(index) = found {
                let item = self.entities.remove(index);
                mob.data.held = Some(item.stack.id);
            }
        }
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
            "villager" => data.villager = Self::new_villager_data(),
            _ => {}
        }
        if !matches!(def.name.as_str(), "wolf") && !def.variants.is_empty() {
            data.variant = self.rng.below(def.variants.len() as u32 + 1) as u8;
        }
        if def.name == "horse" {
            // Half have markings, of the four kinds.
            if self.rng.below(2) == 0 {
                data.markings = 1 + self.rng.below(4) as u8;
            }
        }
        Mob::with_data(kind, def, feet, yaw, data)
    }

    /// One tick of every mob: AI, movement, daylight, hits and shots, drops,
    /// despawning and stashing mobs whose chunk unloaded.
    pub(super) fn tick_mobs(&mut self) {
        // Every player as the mobs see them; each mob goes after the
        // nearest it may attack (vanilla), and listens for the nearest.
        let seen: Vec<Seen> = self
            .players_here()
            .into_iter()
            .map(|index| {
                let p = self.player_at(index);
                let feet = p.body.position();
                Seen {
                    index,
                    feet,
                    eye: feet + DVec3::Y * p.body.eye_height(),
                    aabb: p.body.aabb(),
                    attackable: p.mode.is_vulnerable() && !p.stats.is_dead(),
                    invisible: p
                        .stats
                        .effects
                        .has(crate::player::effects::Effect::Invisibility),
                    pumpkin: p
                        .inventory
                        .slot(crate::inventory::player::ARMOR_HEAD)
                        .is_some_and(|s| s.id == crate::inventory::items::CARVED_PUMPKIN),
                    look: p.camera.forward().as_dvec3(),
                }
            })
            .collect();
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
        let threat = (self.threat_ticks > 0)
            .then(|| mobs.iter().position(|m| m.id == self.threat_id))
            .flatten();
        let mut pending = Vec::new();
        for (index, mob) in mobs.iter_mut().enumerate() {
            let def = defs.get(mob.kind);
            // A ridden horse is steered by the player; a strider only with
            // a warped fungus on a stick.
            let steered = mob.ridden && self.steers_mount(def.ai);
            if steered {
                self.steer_ridden(mob);
            }
            let eye = mob.eye();
            let nearest = seen
                .iter()
                .min_by(|a, b| a.eye.distance(eye).total_cmp(&b.eye.distance(eye)))
                .expect("there's always a player");
            let prey = seen
                .iter()
                .filter(|p| p.attackable)
                .min_by(|a, b| a.eye.distance(eye).total_cmp(&b.eye.distance(eye)))
                .unwrap_or(nearest);
            let (player_feet, player_eye, attackable) = (prey.feet, prey.eye, prey.attackable);
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
                    | Ai::Strider
                    | Ai::Fox { .. }
            );
            // An invisible player is noticed only up close.
            let sight = if prey.invisible {
                INVISIBLE_SIGHT
            } else if matches!(def.ai, Ai::Enderman { .. }) {
                ENDERMAN_SIGHT
            } else if def.ai.has_follow_range() {
                def.follow_range
            } else {
                SIGHT_RANGE
            };
            let near_player = eye.distance(player_eye) < sight;
            let visible = attackable && hunts && near_player && self.sees(eye, player_eye);
            // A carved pumpkin on the head hides the face from endermen.
            let stares = matches!(def.ai, Ai::Enderman { .. }) && {
                let to = eye - player_eye;
                let d = to.length();
                d < 64.0
                    && d > 0.0
                    && prey.look.dot(to / d) > 1.0 - 0.025 / d
                    && visible
                    && !prey.pumpkin
            };
            let target = (attackable && hunts && near_player).then_some(Target {
                feet: player_feet,
                eye: player_eye,
                aabb: prey.aabb,
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
                sky: self
                    .chunks
                    .light_at(mob.cell().x, mob.cell().y, mob.cell().z)
                    .map_or(0, |(sky, _)| sky),
            };
            let view = View {
                terrain: &terrain,
                player: target,
                weight: &weight,
                env,
                others: &others,
                owner: Some(nearest.feet),
                threat,
                errand: mob.data.villager.as_ref().and_then(|v| v.errand),
            };
            let mut random = || thinking.next_f64() as f32;
            let mut intent = if steered {
                Default::default()
            } else {
                ai::think(mob, def, &view, &mut random)
            };
            if steered {
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
            super::effects_move(&mut mob.body, &mob.stats.effects);
            if def.walks_on_lava {
                mob.tick(&intent.input, &LavaFloor(&collider), eyes);
            } else {
                mob.tick(&intent.input, &collider, eyes);
            }
            let near_ears = mob.feet().distance(nearest.feet) < 32.0;
            let mut chance = || thinking.next_f64() as f32;
            super::sounds::mob_sounds(mob, &def.name, near_ears, &mut chance);
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
            pending.push((index, intent, prey.index));
            if mob.is_dead() && !mob.looted {
                mob.looted = true;
                self.on_mob_death(mob);
            }
        }
        self.apply_intents(&mut mobs, pending);
        self.mobs_pick_up_weapons(&mut mobs);
        self.push_apart(&mut mobs);
        // Despawn far monsters; stash animals whose chunk unloaded.
        mobs.retain_mut(|mob| {
            let def = defs.get(mob.kind);
            let distance = seen
                .iter()
                .map(|p| mob.feet().distance(p.feet))
                .fold(f64::INFINITY, f64::min);
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
            // Peaceful sends every monster away at once.
            if def.ai.is_monster() && self.difficulty == crate::level::Difficulty::Peaceful {
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
        self.tick_nether_mobs();
        self.tick_villagers();
        if self.rules.do_mob_spawning {
            self.spawn_cycle();
            self.spawn_water_mobs();
            self.spawn_striders();
            self.spawn_phantoms();
        }
    }

    /// Phantoms come out of the night sky above a player who hasn't slept
    /// in three days (modern `PhantomSpawner`): every three seconds, with
    /// a chance that grows with the time awake, one to three of them 20
    /// to 34 blocks up.
    pub(super) fn spawn_phantoms(&mut self) {
        if !self.time.is_multiple_of(60)
            || self.dimension() != crate::world::dimension::Dimension::Overworld
            || self.player.stats.is_dead()
            || !self.player.mode.is_vulnerable()
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
        let feet = self.player.body.position();
        let head = (feet + DVec3::Y * 1.0).floor().as_ivec3();
        let open_sky = self
            .chunks
            .light_at(head.x, head.y, head.z)
            .is_some_and(|(sky, _)| sky == 15);
        if !open_sky || self.rng.below(self.player.stats.awake.max(1)) < PHANTOM_AFTER {
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
            if !mobs[i].ridden {
                let (bb, feet) = (near(mobs[i].body.aabb()), mobs[i].feet());
                let mut pushed = DVec3::ZERO;
                self.for_each_player(|game| {
                    let body = &mut game.player.body;
                    if !body.flying && bb.intersects(&body.aabb()) {
                        let push = push_between(feet, body.position());
                        pushed += push;
                        body.velocity -= push;
                    }
                });
                mobs[i].body.velocity += pushed;
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
            Spawn::Lava { .. } => below.is_some_and(|b| b.is(crate::world::blocks::LAVA)),
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
        // A client's chunks come with their fluids and mobs from the server.
        if self.is_client() {
            return;
        }
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
                self.spawn_structure_mobs(pos);
            }
        }
    }

    /// The biome at a block column.
    fn biome_at(&self, x: i32, z: i32) -> Biome {
        self.chunks.world().biome_at(x, z).unwrap_or_default()
    }

    /// `NaturalSpawner.spawnMobsForChunkGeneration`: a new chunk has a chance
    /// (its biome's creature spawn probability) of one pack of animals, of a
    /// kind picked by the biome's weights.
    fn populate(&mut self, pos: ChunkPos) {
        let (min_x, min_z) = pos.min_block();
        let mut x = min_x + (self.rng.next_f64() * 16.0) as i32;
        let mut z = min_z + (self.rng.next_f64() * 16.0) as i32;
        let biome = self.biome_at(x, z);
        let chance = match biome {
            Biome::SnowyPlains => spawning::SNOWY_PLAINS_PACK_CHANCE,
            Biome::Badlands => spawning::BADLANDS_PACK_CHANCE,
            _ => spawning::CHUNK_PACK_CHANCE,
        };
        if self.rng.next_f64() as f32 >= chance {
            return;
        }
        let Some((kind, group)) = self.pick_creature_kind(biome) else {
            return;
        };
        let Spawn::Ground { ref blocks, .. } = self.mob_defs.get(kind).spawn else {
            return;
        };
        let blocks = blocks.clone();
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

    /// An animal kind for `biome` by weight, and its pack size.
    fn pick_creature_kind(&mut self, biome: Biome) -> Option<(MobKind, u8)> {
        let options: Vec<(MobKind, u8, u32)> = self
            .mob_defs
            .iter()
            .filter_map(|(kind, def)| match &def.spawn {
                Spawn::Ground {
                    group,
                    weight,
                    biomes,
                    ..
                } if biomes.contains(&biome) => Some((kind, *group, *weight)),
                _ => None,
            })
            .collect();
        let total: u32 = options.iter().map(|o| o.2).sum();
        if total == 0 {
            return None;
        }
        let mut roll = self.rng.below(total);
        options.into_iter().find_map(|(kind, group, weight)| {
            if roll < weight {
                return Some((kind, group));
            }
            roll -= weight;
            None
        })
    }

    /// Feet position on top of column (x, z) if its surface is one of
    /// `ground` with two free blocks above.
    pub(super) fn ground_spot(&self, x: i32, z: i32, ground: &[BlockId]) -> Option<DVec3> {
        let world = self.chunks.world();
        let top = (0..crate::config::MAX_Y)
            .rev()
            .find(|&y| world.block(x, y, z).is_some_and(|b| !b.is_air()))?;
        let mut surface = world.block(x, top, z)?;
        let free = |y| {
            world
                .block(x, y, z)
                .is_some_and(|b: crate::world::block::BlockState| b.is_air())
        };
        // Standing in a snow layer on the ground below it.
        let mut feet = top + 1;
        if surface.id() == crate::world::blocks::SNOW {
            surface = world.block(x, top - 1, z)?;
            feet = top;
        }
        (ground.contains(&surface.id()) && free(top + 1) && free(feet + 1))
            .then(|| DVec3::new(x as f64 + 0.5, feet as f64, z as f64 + 0.5))
    }

    /// The category a mob counts toward, if natural spawning tracks it.
    fn category_of(&self, mob: &Mob) -> Option<&'static Category> {
        match self.mob_defs.get(mob.kind).spawn {
            Spawn::Dark { .. } | Spawn::SlimeChunk { .. } | Spawn::Fortress { .. } => {
                Some(&spawning::MONSTER)
            }
            Spawn::Nether { .. } => Some(&spawning::MONSTER),
            Spawn::Ground { .. } => Some(&spawning::CREATURE),
            _ => None,
        }
    }

    /// Mobs that count toward a cap: not tamed or ridden (persistent), and
    /// in a chunk near the player.
    fn counted_in(&self, category: &Category, centers: &[ChunkPos]) -> usize {
        self.mobs
            .iter()
            .filter(|mob| !mob.is_dead() && !mob.data.tamed && !mob.ridden)
            .filter(|mob| {
                self.category_of(mob)
                    .is_some_and(|c| c.name == category.name)
            })
            .filter(|mob| {
                let at =
                    ChunkPos::of_block(mob.feet().x.floor() as i32, mob.feet().z.floor() as i32);
                centers.iter().any(|center| {
                    (at.x - center.x).abs().max((at.z - center.z).abs()) <= SPAWN_CHUNK_RADIUS
                })
            })
            .count()
    }

    /// The chunk each player stands in.
    pub fn player_chunks(&self) -> Vec<ChunkPos> {
        let mut chunks: Vec<ChunkPos> = self
            .players_here()
            .into_iter()
            .map(|i| {
                let feet = self.feet_of(i);
                ChunkPos::of_block(feet.x.floor() as i32, feet.z.floor() as i32)
            })
            .collect();
        chunks.sort_by_key(|pos| (pos.x, pos.z));
        chunks.dedup();
        chunks
    }

    /// How far the nearest player is from `at`.
    pub(super) fn nearest_player_distance(&self, at: DVec3) -> f64 {
        self.players_here()
            .into_iter()
            .map(|i| self.feet_of(i).distance(at))
            .fold(f64::INFINITY, f64::min)
    }

    /// The chunks that take part in spawning: loaded, within the radius of
    /// any player, each once, in random order.
    fn spawnable_chunks(&mut self, centers: &[ChunkPos]) -> Vec<ChunkPos> {
        let mut seen = crate::world::fast_hash::FastSet::default();
        let mut chunks: Vec<ChunkPos> = centers
            .iter()
            .flat_map(|&center| {
                (-SPAWN_CHUNK_RADIUS..=SPAWN_CHUNK_RADIUS).flat_map(move |dz| {
                    (-SPAWN_CHUNK_RADIUS..=SPAWN_CHUNK_RADIUS).map(move |dx| center.offset(dx, dz))
                })
            })
            .filter(|&pos| seen.insert(pos) && self.chunks.world().get(pos).is_some())
            .collect();
        for i in (1..chunks.len()).rev() {
            let j = self.rng.below(i as u32 + 1) as usize;
            chunks.swap(i, j);
        }
        chunks
    }

    /// One tick of natural spawning: each category whose turn it is.
    pub(super) fn spawn_cycle(&mut self) {
        if self.difficulty != crate::level::Difficulty::Peaceful {
            self.spawn_category(&spawning::MONSTER);
        }
        self.spawn_category(&spawning::CREATURE);
    }

    /// `NaturalSpawner.spawnForChunk` for every spawnable chunk and each
    /// category under its cap.
    pub(super) fn spawn_category(&mut self, category: &Category) {
        if !self.time.is_multiple_of(category.every) {
            return;
        }
        let centers = self.player_chunks();
        let chunks = self.spawnable_chunks(&centers);
        // The cap grows with the chunks in range: 70 for a full 17 × 17
        // around each player.
        let cap = category.cap * chunks.len() / spawning::CAP_CHUNKS;
        let mut count = self.counted_in(category, &centers);
        for pos in chunks {
            if count >= cap {
                return;
            }
            count += self.spawn_in_chunk(category, pos, cap - count);
        }
    }

    /// A random spot in the chunk, then up to three packs from it, no more
    /// than `room` mobs in all. Returns how many arrived.
    fn spawn_in_chunk(&mut self, category: &Category, pos: ChunkPos, room: usize) -> usize {
        // Up to the top of the chunk's highest blocks (modern picks a height
        // up to the surface), so mountains get monsters too.
        let Some(top) = self.chunks.world().get(pos).map(|c| c.height_bound()) else {
            return 0;
        };
        let (min_x, min_z) = pos.min_block();
        // From the bottom of the world (Y −64 in the Overworld since M34).
        let floor = self.dimension.min_y();
        let start = IVec3::new(
            min_x + self.rng.below(16) as i32,
            floor + self.rng.below((top - floor).max(0) as u32 + 2) as i32,
            min_z + self.rng.below(16) as i32,
        );
        // A solid block there leaves no room for anything.
        if self.solid_at(start) {
            return 0;
        }
        let creature = category.name == spawning::CREATURE.name;
        let mut arrived = 0;
        for _ in 0..spawning::PACKS {
            let mut cell = start;
            let mut picked: Option<(MobKind, u8, u8)> = None;
            let mut in_pack = 0;
            for _ in 0..spawning::PACK_STEPS {
                cell.x += self.rng.below(spawning::PACK_STEP) as i32
                    - self.rng.below(spawning::PACK_STEP) as i32;
                cell.z += self.rng.below(spawning::PACK_STEP) as i32
                    - self.rng.below(spawning::PACK_STEP) as i32;
                let here = ChunkPos::of_block(cell.x, cell.z);
                if self.chunks.world().get(here).is_none() {
                    continue;
                }
                let (kind, max_light, group) = match picked {
                    Some(found) => found,
                    None => {
                        let biome = self.biome_at(cell.x, cell.z);
                        let found = if creature {
                            self.pick_creature_kind(biome)
                                .map(|(kind, group)| (kind, 15, group))
                        } else {
                            self.pick_dark_kind(biome, cell, here)
                        };
                        let Some(found) = found else {
                            break;
                        };
                        picked = Some(found);
                        found
                    }
                };
                let fits = if creature {
                    self.can_animal_spawn(kind, cell)
                } else {
                    self.can_monster_spawn(cell, max_light)
                };
                if fits && self.has_room(kind, cell) {
                    let feet = cell.as_dvec3() + DVec3::new(0.5, 0.0, 0.5);
                    let yaw = self.rng.next_f64() as f32 * std::f32::consts::TAU;
                    let mob = self.new_mob(kind, feet, yaw);
                    self.add_mob(mob);
                    arrived += 1;
                    in_pack += 1;
                    if arrived >= room {
                        return arrived;
                    }
                    if in_pack >= group {
                        break;
                    }
                }
            }
        }
        arrived
    }

    /// Whether the block at `cell` is solid.
    fn solid_at(&self, cell: IVec3) -> bool {
        let terrain = WorldTerrain {
            world: self.chunks.world(),
            blocks: self.items.blocks,
        };
        terrain.def(cell).is_none_or(|def| def.solid)
    }

    /// Far enough from the player and the world spawn, and not out of range.
    fn right_distance(&self, feet: DVec3) -> bool {
        let player = self.nearest_player_distance(feet);
        (spawning::MIN_DISTANCE..=spawning::MAX_DISTANCE).contains(&player)
            && feet.distance(self.spawn) >= spawning::MIN_DISTANCE
    }

    /// Animals: on their kind's ground (grass), two free cells above, and
    /// brighter than 8 (block or sky light, not dimmed by the time of day).
    fn can_animal_spawn(&self, kind: MobKind, cell: IVec3) -> bool {
        let Spawn::Ground { ref blocks, .. } = self.mob_defs.get(kind).spawn else {
            return false;
        };
        let terrain = WorldTerrain {
            world: self.chunks.world(),
            blocks: self.items.blocks,
        };
        let free = |c: IVec3| terrain.passable(c) && !terrain.water(c) && !terrain.lava(c);
        let ground = self
            .chunks
            .world()
            .block(cell.x, cell.y - 1, cell.z)
            .is_some_and(|b| blocks.contains(&b.id()));
        if !ground || !free(cell) || !free(cell + IVec3::Y) {
            return false;
        }
        if !self.right_distance(cell.as_dvec3() + DVec3::new(0.5, 0.0, 0.5)) {
            return false;
        }
        self.chunks
            .light_at(cell.x, cell.y, cell.z)
            .is_some_and(|(sky, block)| sky.max(block) > spawning::ANIMAL_LIGHT_ABOVE)
    }

    /// The monsters near the player (for the tests and the cap).
    #[cfg(test)]
    pub(super) fn monster_count(&self) -> usize {
        let player = self.player.body.position();
        let center = ChunkPos::of_block(player.x.floor() as i32, player.z.floor() as i32);
        self.counted_in(&spawning::MONSTER, &[center])
    }

    /// Only the monster category's cycle (for the tests).
    #[cfg(test)]
    pub(super) fn spawn_monsters(&mut self) {
        self.spawn_category(&spawning::MONSTER);
    }

    /// A kind that can spawn here by weight: (kind, max light, group size).
    pub(super) fn pick_dark_kind(
        &mut self,
        biome: Biome,
        cell: IVec3,
        chunk: ChunkPos,
    ) -> Option<(MobKind, u8, u8)> {
        // Mushroom fields never have monsters (modern `MobSpawnSettings`).
        if biome == Biome::MushroomFields {
            return None;
        }
        // The End's only monsters are endermen, in groups of four.
        if biome.is_end() {
            return self.mob_defs.find("enderman").map(|kind| (kind, 15, 4));
        }
        let seed = self.chunks.seed().unwrap_or(0);
        let mut options: Vec<(MobKind, u8, u8, u32)> = Vec::new();
        let nether = biome.is_nether();
        // A fortress has its own list (modern `NetherFortressStructure`).
        let fortress = nether
            && self.chunks.nether().is_some_and(|n| {
                crate::world::structure::nether::kind_at(n, cell)
                    == Some(crate::world::structure::Kind::Fortress)
            });
        if fortress {
            for (kind, def) in self.mob_defs.iter() {
                let entry = match (&def.spawn, def.name.as_str()) {
                    (Spawn::Fortress { weight, group }, _) => Some((*weight, *group)),
                    (_, name) => FORTRESS_EXTRAS
                        .iter()
                        .find(|(n, _, _)| *n == name)
                        .map(|&(_, weight, group)| (weight, group)),
                };
                if let Some((weight, group)) = entry {
                    options.push((kind, FORTRESS_LIGHT, group, weight));
                }
            }
        }
        for (kind, def) in self.mob_defs.iter() {
            match def.spawn {
                // The Nether's biomes have their own lists.
                Spawn::Dark {
                    max_light,
                    nether: ref spawns,
                    ..
                }
                | Spawn::Nether {
                    max_light,
                    biomes: ref spawns,
                } if nether && !fortress => {
                    if let Some(&(_, weight, group)) = spawns.iter().find(|s| s.0 == biome) {
                        options.push((kind, max_light, group, weight));
                    }
                }
                _ if nether || fortress => {}
                Spawn::Dark {
                    max_light,
                    group,
                    ref biomes,
                    weight,
                    ..
                } if biomes.is_empty() || biomes.contains(&biome) => {
                    options.push((kind, max_light, group, weight));
                }
                // Slimes come out of the ground in slime chunks, deep down.
                Spawn::SlimeChunk { below } if cell.y < below && is_slime_chunk(seed, chunk) => {
                    options.push((kind, 15, 1, 100));
                }
                // And out of swamps at night, from y 51 to 69 (modern).
                Spawn::SlimeChunk { .. }
                    if biome == Biome::Swamp && (SWAMP_SLIMES).contains(&cell.y) =>
                {
                    options.push((kind, 7, 1, 100));
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
    pub(super) fn can_monster_spawn(&mut self, cell: IVec3, max_light: u8) -> bool {
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
        if !self.right_distance(feet) {
            return false;
        }
        let Some((sky, block)) = self.chunks.light_at(cell.x, cell.y, cell.z) else {
            return false;
        };
        // The End has no sky light (modern `hasSkyLight` is false there).
        let sky = if self.dimension == crate::world::dimension::Dimension::End {
            0
        } else {
            sky
        };
        // Monsters of the Overworld need no block light at all.
        if self.dimension == crate::world::dimension::Dimension::Overworld
            && block > spawning::MONSTER_BLOCK_LIGHT
        {
            return false;
        }
        let subtracted = daytime::sky_light_subtracted(self.celestial_angle());
        let sky = sky.saturating_sub(subtracted);
        if sky as f64 > self.rng.next_f64() * 32.0 {
            return false;
        }
        let level = sky.max(block);
        level <= max_light && u32::from(level) <= self.rng.below(spawning::MONSTER_LIGHT_BELOW)
    }

    /// Whether a mob of `kind` standing in `cell` touches nothing solid
    /// (a ghast needs a 4-block cube of air).
    fn has_room(&self, kind: MobKind, cell: IVec3) -> bool {
        let def = self.mob_defs.get(kind);
        let feet = cell.as_dvec3() + DVec3::new(0.5, 0.0, 0.5);
        let half = def.width / 2.0;
        let region = crate::player::collision::Aabb::new(
            feet - DVec3::new(half, 0.0, half),
            feet + DVec3::new(half, def.height, half),
        );
        let collider = WorldCollider {
            world: self.chunks.world(),
            blocks: self.items.blocks,
        };
        let mut boxes = Vec::new();
        crate::player::collision::Collider::boxes(&collider, &region, &mut boxes);
        !boxes.iter().any(|b| b.intersects(&region)) && collider.liquid(&region).is_none()
    }

    /// Striders: now and then, in each chunk near the player, a spot on
    /// the lava sea with air above (modern spawns them as creatures on
    /// lava), up to a few at a time.
    fn spawn_striders(&mut self) {
        if !self.time.is_multiple_of(20)
            || self.dimension() != crate::world::dimension::Dimension::Nether
        {
            return;
        }
        let Some((kind, group, biomes)) =
            self.mob_defs
                .iter()
                .find_map(|(kind, def)| match &def.spawn {
                    Spawn::Lava { group, biomes } => Some((kind, *group, biomes.clone())),
                    _ => None,
                })
        else {
            return;
        };
        let count = self
            .mobs
            .iter()
            .filter(|m| m.kind == kind && !m.is_dead())
            .count();
        if count >= STRIDER_CAP {
            return;
        }
        let centers = self.player_chunks();
        let center = centers[self.rng.below(centers.len() as u32) as usize];
        let reach = SPAWN_CHUNK_RADIUS as u32 * 2 + 1;
        let pos = center.offset(
            self.rng.below(reach) as i32 - SPAWN_CHUNK_RADIUS,
            self.rng.below(reach) as i32 - SPAWN_CHUNK_RADIUS,
        );
        if self.chunks.world().get(pos).is_none() {
            return;
        }
        let (min_x, min_z) = pos.min_block();
        let x = min_x + self.rng.below(16) as i32;
        let z = min_z + self.rng.below(16) as i32;
        if !biomes.contains(&self.biome_at(x, z)) {
            return;
        }
        let world = self.chunks.world();
        let is_lava = |y: i32| {
            world
                .block(x, y, z)
                .is_some_and(|b| b.is(crate::world::blocks::LAVA))
        };
        let Some(surface) = (1..crate::world::worldgen::nether::HEIGHT)
            .find(|&y| is_lava(y) && world.block(x, y + 1, z).is_some_and(|b| b.is_air()))
        else {
            return;
        };
        let feet = DVec3::new(x as f64 + 0.5, (surface + 1) as f64, z as f64 + 0.5);
        if self.nearest_player_distance(feet) < MIN_SPAWN_DISTANCE {
            return;
        }
        for i in 0..1 + self.rng.below(u32::from(group)) {
            let at = feet + DVec3::new(f64::from(i), 0.0, 0.0);
            let yaw = self.rng.next_f64() as f32 * std::f32::consts::TAU;
            let mob = self.new_mob(kind, at, yaw);
            self.add_mob(mob);
        }
    }

    /// Squid and fish: in each chunk near the player, a random spot in deep
    /// water, now and then, up to a cap.
    fn spawn_water_mobs(&mut self) {
        if !self.time.is_multiple_of(20) {
            return;
        }
        let centers = self.player_chunks();
        let center = centers[self.rng.below(centers.len() as u32) as usize];
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
                if !deep || self.nearest_player_distance(cell.as_dvec3()) < 16.0 {
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
                dimension: self.dimension,
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

/// A player as the mobs see them this tick.
struct Seen {
    /// Which player (0 the host, then `others`).
    index: usize,
    feet: DVec3,
    eye: DVec3,
    aabb: crate::player::collision::Aabb,
    attackable: bool,
    invisible: bool,
    /// A carved pumpkin on their head.
    pumpkin: bool,
    look: DVec3,
}
