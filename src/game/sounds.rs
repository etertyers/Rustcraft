//! The sounds the player makes by living: hurt and death, footsteps, landing
//! and splashing, level-ups, bubbles. Everything is queued as a named event
//! (`crate::audio`); nothing here depends on the sound engine.

use glam::{DVec3, IVec3};

use super::Game;
use crate::{
    audio::{self, events},
    player::stats::DamageSource,
    world::block::Fluid,
};

/// Blocks walked between footsteps.
const STEP_DISTANCE: f64 = 1.6;
/// A fall of at least this many blocks lands with a thud.
const THUD_FALL: f32 = 1.5;

/// What the player was like last tick, to tell what happened since.
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct Memory {
    health: f32,
    dead: bool,
    level: u32,
    air: i32,
    fire: i32,
    on_ground: bool,
    liquid: Option<Fluid>,
    /// Blocks walked since the last footstep.
    walked: f64,
    /// The longest fall so far, kept to sound the landing.
    fall: f32,
    started: bool,
    /// The dragon's health and whether it was dying, last tick.
    dragon_health: f32,
    dragon_dying: bool,
}

/// The event for the player being hurt by `source` (`damage` points).
pub fn hurt_event(source: DamageSource, damage: f32) -> &'static str {
    match source {
        DamageSource::Fall if damage > 4.0 => "entity.player.big_fall",
        DamageSource::Fall => "entity.player.small_fall",
        DamageSource::Drowning => "entity.player.hurt_drown",
        DamageSource::Lava | DamageSource::InFire | DamageSource::Fire | DamageSource::HotFloor => {
            "entity.player.hurt_on_fire"
        }
        _ => "entity.player.hurt",
    }
}

/// Block cell and name of what the player walks on or through: the block
/// at the feet if it has a sound of its own (snow layers, carpets, plants),
/// else the one below.
fn ground_name(game: &Game) -> Option<String> {
    let feet = game.player.body.position();
    let world = game.chunks.world();
    let cell = feet.floor().as_ivec3();
    let solid = |at: IVec3| {
        world
            .block(at.x, at.y, at.z)
            .map(|b| b.def())
            .filter(|def| def.fluid.is_none() && def.shape != crate::world::block::Shape::None)
    };
    let here = solid(cell).filter(|def| def.name.contains("snow") || def.name.contains("carpet"));
    here.or_else(|| solid((feet - DVec3::Y * 0.2).floor().as_ivec3()))
        .map(|def| def.name.clone())
}

impl Game {
    /// Queues the player's sounds for what changed this tick.
    pub(super) fn tick_sounds(&mut self) {
        let at = self.player.body.position();
        let mut mem = std::mem::take(&mut self.player.sounds);
        if !mem.started {
            mem = Memory {
                health: self.player.stats.health,
                level: self.player.stats.experience.level,
                air: self.player.stats.air,
                fire: self.player.stats.fire,
                on_ground: self.player.body.on_ground,
                liquid: self.player.body.liquid,
                started: true,
                ..Memory::default()
            };
        }
        // Hurt, and dying.
        let dead = self.player.stats.is_dead();
        if dead && !mem.dead {
            audio::play("entity.player.death", at);
        } else if !dead && self.player.stats.health < mem.health {
            let source = self.player.stats.last_source.unwrap_or(DamageSource::Mob);
            audio::play(
                hurt_event(source, mem.health - self.player.stats.health),
                at,
            );
        }
        mem.dead = dead;
        mem.health = self.player.stats.health;
        if self.player.stats.experience.level > mem.level {
            audio::play_here_with("entity.player.levelup", 0.75, 1.0);
        }
        mem.level = self.player.stats.experience.level;
        // Fire going out.
        if mem.fire > 0 && self.player.stats.fire == 0 && !dead {
            audio::play_at("entity.generic.extinguish_fire", at, 0.7, 1.6);
        }
        mem.fire = self.player.stats.fire;
        // Bubbles popping as the air runs low under water.
        if self.player.stats.air < mem.air
            && self.player.stats.air < 300
            && self.player.stats.air % 30 == 0
        {
            audio::play_here("ui.hud.bubble_pop");
        }
        mem.air = self.player.stats.air;
        // Into the water and out, and a splash.
        let liquid = self.player.body.liquid;
        if liquid == Some(Fluid::Water) && mem.liquid.is_none() {
            let fast = mem.fall > 3.0;
            audio::play_at(
                if fast {
                    "entity.player.splash.high_speed"
                } else {
                    "entity.player.splash"
                },
                at,
                0.4 + (mem.fall * 0.1).min(0.6),
                1.0,
            );
        }
        // Landing: a thud from the block, once a fall was long enough.
        if self.player.body.on_ground
            && !mem.on_ground
            && mem.fall >= THUD_FALL
            && liquid.is_none()
            && let Some(name) = ground_name(self)
        {
            audio::play_at(events::block_event(&name, "fall"), at, 0.5, 0.75);
        }
        if self.player.body.on_ground || liquid.is_some() {
            mem.fall = 0.0;
        } else {
            mem.fall = mem.fall.max(self.player.stats.fall_distance);
        }
        // Footsteps: one every so far walked on the ground, swimming strokes
        // in water; flying and gliding are silent.
        let moved = at - self.player.prev_feet;
        let flat = moved.x.hypot(moved.z);
        let quiet =
            self.player.body.flying || self.player.body.fall_flying || dead || self.is_sleeping();
        if !quiet && flat > 0.0 {
            mem.walked += flat;
            if mem.walked >= STEP_DISTANCE {
                mem.walked -= STEP_DISTANCE;
                if liquid == Some(Fluid::Water) {
                    audio::play_at("entity.player.swim", at, 0.3, 1.0);
                } else if self.player.body.on_ground
                    && let Some(name) = ground_name(self)
                {
                    let quiet = if self.player.body.sneaking { 0.5 } else { 1.0 };
                    audio::play_at(events::block_event(&name, "step"), at, 0.15 * quiet, 1.0);
                }
            }
        }
        mem.on_ground = self.player.body.on_ground;
        mem.liquid = liquid;
        self.player.sounds = mem;
        if !dead {
            self.tick_ambient();
        }
    }

    /// Noises of the surroundings: fire crackling, lava popping and
    /// bubbling, furnaces burning, and the dark caves and the Nether
    /// whispering. A few random spots near the player are tried each tick.
    fn tick_ambient(&mut self) {
        use crate::world::{blocks, dimension::Dimension, props::Prop};
        let feet = self.player.body.position();
        for _ in 0..12 {
            let offset = IVec3::new(
                self.rng.below(21) as i32 - 10,
                self.rng.below(11) as i32 - 5,
                self.rng.below(21) as i32 - 10,
            );
            let cell = feet.floor().as_ivec3() + offset;
            let Some(block) = self.chunks.world().block(cell.x, cell.y, cell.z) else {
                continue;
            };
            if block.is_air() {
                continue;
            }
            let at = cell.as_dvec3() + DVec3::splat(0.5);
            let roll = self.rng.next_f64();
            let def = block.def();
            if block.is(blocks::FIRE) || block.is(blocks::SOUL_FIRE) {
                if roll < 1.0 / 24.0 {
                    let pitch = 0.3 + self.rng.next_f64() as f32 * 0.7;
                    audio::play_at(
                        "block.fire.ambient",
                        at,
                        1.0 + self.rng.next_f64() as f32,
                        pitch,
                    );
                }
            } else if def.fluid == Some(Fluid::Lava) && crate::world::fluid::level(block) == 0 {
                if roll < 1.0 / 100.0 {
                    audio::play_at(
                        "block.lava.pop",
                        at,
                        0.2 + self.rng.next_f64() as f32 * 0.2,
                        0.6 + self.rng.next_f64() as f32 * 0.7,
                    );
                } else if roll < 1.0 / 100.0 + 1.0 / 200.0 {
                    audio::play_at("block.lava.ambient", at, 0.2, 1.0);
                }
            } else if block.is(blocks::NETHER_PORTAL) {
                if roll < 1.0 / 100.0 {
                    audio::play_at(
                        "block.portal.ambient",
                        at,
                        0.5,
                        self.rng.next_f64() as f32 * 0.4 + 0.8,
                    );
                }
            } else if block.has(Prop::Lit)
                && block.bool(Prop::Lit)
                && def.name.ends_with("furnace")
                && roll < 1.0 / 40.0
            {
                audio::play_at("block.furnace.fire_crackle", at, 0.6, 1.0);
            }
        }
        // Dark places creak: about every half minute underground.
        let eye = feet.floor().as_ivec3() + IVec3::Y;
        match self.dimension {
            Dimension::Overworld => {
                if feet.y < 56.0
                    && self.light_level(eye).is_some_and(|light| light <= 4)
                    && self.rng.next_f64() < 1.0 / 600.0
                {
                    audio::play_here_with(
                        "ambient.cave",
                        0.7,
                        0.9 + self.rng.next_f64() as f32 * 0.2,
                    );
                }
            }
            Dimension::Nether => {
                use crate::world::biome::Biome;
                let biome = self
                    .chunks
                    .world()
                    .biome_at(eye.x, eye.z)
                    .unwrap_or_default();
                let (mood, additions) = match biome {
                    Biome::CrimsonForest => (
                        "ambient.crimson_forest.mood",
                        "ambient.crimson_forest.additions",
                    ),
                    Biome::WarpedForest => (
                        "ambient.warped_forest.mood",
                        "ambient.warped_forest.additions",
                    ),
                    Biome::SoulSandValley => (
                        "ambient.soul_sand_valley.mood",
                        "ambient.soul_sand_valley.additions",
                    ),
                    Biome::BasaltDeltas => (
                        "ambient.basalt_deltas.mood",
                        "ambient.basalt_deltas.additions",
                    ),
                    _ => (
                        "ambient.nether_wastes.mood",
                        "ambient.nether_wastes.additions",
                    ),
                };
                let roll = self.rng.next_f64();
                if roll < 1.0 / 900.0 {
                    audio::play_here_with(mood, 1.0, 1.0);
                } else if roll < 1.0 / 900.0 + 1.0 / 400.0 {
                    audio::play_here_with(additions, 0.5, 1.0);
                }
            }
            Dimension::End => {}
        }
    }

    /// The ender dragon's wingbeats, growls, hurt and death.
    pub(super) fn dragon_sounds(&mut self, dragon: &crate::entity::dragon::EnderDragon) {
        use crate::entity::dragon::Phase;
        let at = dragon.position;
        let dying = dragon.in_death_sequence();
        if dying && !self.player.sounds.dragon_dying {
            audio::play_at("entity.ender_dragon.death", at, 5.0, 1.0);
        } else if !dying {
            if dragon.health < self.player.sounds.dragon_health {
                audio::play_at("entity.ender_dragon.hurt", at, 5.0, 1.0);
            }
            if dragon.age.is_multiple_of(12) && dragon.phase != Phase::SittingScanning {
                let pitch = 0.8 + self.rng.next_f64() as f32 * 0.3;
                audio::play_at("entity.ender_dragon.flap", at, 5.0, pitch);
            }
            let roll = self.rng.next_f64();
            if roll < 1.0 / 120.0 {
                audio::play_at("entity.ender_dragon.growl", at, 5.0, 1.0);
            } else if roll < 1.0 / 120.0 + 1.0 / 200.0 {
                audio::play_at("entity.ender_dragon.ambient", at, 5.0, 1.0);
            }
        }
        self.player.sounds.dragon_dying = dying;
        self.player.sounds.dragon_health = dragon.health;
    }

    /// The sounds of a block going away or arriving by the player's hand.
    pub(super) fn block_sound(&self, name: &str, verb: &str, cell: IVec3) {
        let at = cell.as_dvec3() + DVec3::splat(0.5);
        let (volume, pitch) = match verb {
            "hit" => (0.25, 0.5),
            "step" => (0.15, 1.0),
            _ => (1.0, 0.8),
        };
        audio::play_at(events::block_event(name, verb), at, volume, pitch);
    }
}

/// Mobs that have a footstep sound of their own in vanilla; the rest are
/// silent walkers.
fn has_step(name: &str) -> bool {
    matches!(
        name,
        "pig"
            | "cow"
            | "sheep"
            | "chicken"
            | "horse"
            | "wolf"
            | "iron_golem"
            | "zombie"
            | "husk"
            | "drowned"
            | "skeleton"
            | "stray"
            | "spider"
            | "cave_spider"
            | "silverfish"
            | "piglin"
            | "hoglin"
            | "strider"
            | "wither_skeleton"
            | "piglin_brute"
    )
}

/// Sounds for a mob this tick: a grunt when hurt, a last one on dying, now
/// and then a noise of its own, and footsteps as it walks. `near` is
/// whether the player is close enough for the idle noises to matter.
pub fn mob_sounds(
    mob: &mut crate::entity::mob::Mob,
    name: &str,
    near: bool,
    random: &mut impl FnMut() -> f32,
) {
    let at = mob.feet();
    let small = mob.data.size > 0 && mob.data.size <= 1;
    let voice = |verb: &str| {
        // Small slimes and magma cubes have smaller voices.
        if small && matches!(name, "slime" | "magma_cube") {
            events::mob_event(name, &format!("{verb}_small"))
        } else {
            events::mob_event(name, verb)
        }
    };
    let dead = mob.is_dead();
    if dead && !mob.heard.dead {
        audio::play_at(voice("death"), at, 1.0, 0.9 + random() * 0.2);
    } else if !dead && mob.stats.health < mob.heard.health {
        audio::play_at(voice("hurt"), at, 1.0, 0.9 + random() * 0.2);
    }
    mob.heard.dead = dead;
    mob.heard.health = mob.stats.health;
    if dead {
        return;
    }
    // A creeper lighting its fuse, an enderman noticing it is stared at.
    if name == "creeper" && mob.swell > 0 && mob.heard.swell == 0 {
        audio::play_at("entity.creeper.primed", at, 1.0, 0.5);
    }
    mob.heard.swell = mob.swell;
    if name == "enderman" && mob.aggressive && !mob.heard.aggressive {
        audio::play_at("entity.enderman.stare", at, 2.5, 1.0);
    }
    mob.heard.aggressive = mob.aggressive;
    // Idle noises: about once every eight seconds; an angry enderman screams.
    if near && random() < 1.0 / 160.0 {
        let verb = if name == "enderman" && mob.aggressive {
            "scream"
        } else {
            "ambient"
        };
        audio::play_at(voice(verb), at, 1.0, 0.9 + random() * 0.2);
    }
    if has_step(name) && mob.body.on_ground && mob.body.liquid.is_none() {
        let stride = mob.limb_swing * 0.5;
        if stride.floor() > mob.heard.stride.floor() && mob.limb_amount > 0.1 {
            audio::play_at(voice("step"), at, 0.15, 1.0);
        }
        mob.heard.stride = stride;
    }
}

/// Which wood (or metal) a door, trapdoor, gate, button or plate is made of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Material {
    Wood,
    Bamboo,
    Cherry,
    Nether,
    Iron,
    Copper,
    Stone,
}

fn material(name: &str) -> Material {
    if name.starts_with("iron") || name.contains("weighted") {
        Material::Iron
    } else if name.contains("copper") {
        Material::Copper
    } else if name.starts_with("bamboo") {
        Material::Bamboo
    } else if name.starts_with("cherry") {
        Material::Cherry
    } else if name.starts_with("crimson") || name.starts_with("warped") {
        Material::Nether
    } else if name.contains("stone") || name.contains("blackstone") {
        Material::Stone
    } else {
        Material::Wood
    }
}

/// The sound of a door, trapdoor or fence gate named `name` opening or closing.
pub fn open_event(name: &str, open: bool) -> Option<&'static str> {
    let kind = if name.ends_with("trapdoor") {
        1
    } else if name.ends_with("door") {
        0
    } else if name.ends_with("fence_gate") {
        2
    } else {
        return None;
    };
    Some(match (kind, material(name), open) {
        (0, Material::Iron, true) => "block.iron_door.open",
        (0, Material::Iron, false) => "block.iron_door.close",
        (0, Material::Copper, true) => "block.copper_door.open",
        (0, Material::Copper, false) => "block.copper_door.close",
        (0, Material::Bamboo, true) => "block.bamboo_wood_door.open",
        (0, Material::Bamboo, false) => "block.bamboo_wood_door.close",
        (0, Material::Cherry, true) => "block.cherry_wood_door.open",
        (0, Material::Cherry, false) => "block.cherry_wood_door.close",
        (0, Material::Nether, true) => "block.nether_wood_door.open",
        (0, Material::Nether, false) => "block.nether_wood_door.close",
        (0, _, true) => "block.wooden_door.open",
        (0, _, false) => "block.wooden_door.close",
        (1, Material::Iron, true) => "block.iron_trapdoor.open",
        (1, Material::Iron, false) => "block.iron_trapdoor.close",
        (1, Material::Copper, true) => "block.copper_trapdoor.open",
        (1, Material::Copper, false) => "block.copper_trapdoor.close",
        (1, Material::Bamboo, true) => "block.bamboo_wood_trapdoor.open",
        (1, Material::Bamboo, false) => "block.bamboo_wood_trapdoor.close",
        (1, Material::Cherry, true) => "block.cherry_wood_trapdoor.open",
        (1, Material::Cherry, false) => "block.cherry_wood_trapdoor.close",
        (1, Material::Nether, true) => "block.nether_wood_trapdoor.open",
        (1, Material::Nether, false) => "block.nether_wood_trapdoor.close",
        (1, _, true) => "block.wooden_trapdoor.open",
        (1, _, false) => "block.wooden_trapdoor.close",
        (_, Material::Bamboo, true) => "block.bamboo_wood_fence_gate.open",
        (_, Material::Bamboo, false) => "block.bamboo_wood_fence_gate.close",
        (_, Material::Cherry, true) => "block.cherry_wood_fence_gate.open",
        (_, Material::Cherry, false) => "block.cherry_wood_fence_gate.close",
        (_, Material::Nether, true) => "block.nether_wood_fence_gate.open",
        (_, Material::Nether, false) => "block.nether_wood_fence_gate.close",
        (_, _, true) => "block.fence_gate.open",
        (_, _, false) => "block.fence_gate.close",
    })
}

/// The click of a lever, button or pressure plate named `name` switching
/// on or off, and its pitch.
pub fn click_event(name: &str, on: bool) -> Option<(&'static str, f32)> {
    if name == "lever" {
        return Some(("block.lever.click", if on { 0.6 } else { 0.5 }));
    }
    let button = name.ends_with("_button");
    let plate = name.ends_with("pressure_plate");
    if !button && !plate {
        return None;
    }
    let pitch = if on { 0.6 } else { 0.5 };
    Some((
        match (button, material(name), on) {
            (true, Material::Stone, true) => "block.stone_button.click_on",
            (true, Material::Stone, false) => "block.stone_button.click_off",
            (true, Material::Bamboo, true) => "block.bamboo_wood_button.click_on",
            (true, Material::Bamboo, false) => "block.bamboo_wood_button.click_off",
            (true, Material::Cherry, true) => "block.cherry_wood_button.click_on",
            (true, Material::Cherry, false) => "block.cherry_wood_button.click_off",
            (true, Material::Nether, true) => "block.nether_wood_button.click_on",
            (true, Material::Nether, false) => "block.nether_wood_button.click_off",
            (true, _, true) => "block.wooden_button.click_on",
            (true, _, false) => "block.wooden_button.click_off",
            (false, Material::Stone, true) => "block.stone_pressure_plate.click_on",
            (false, Material::Stone, false) => "block.stone_pressure_plate.click_off",
            (false, Material::Iron | Material::Copper, true) => {
                "block.metal_pressure_plate.click_on"
            }
            (false, Material::Iron | Material::Copper, false) => {
                "block.metal_pressure_plate.click_off"
            }
            (false, Material::Bamboo, true) => "block.bamboo_wood_pressure_plate.click_on",
            (false, Material::Bamboo, false) => "block.bamboo_wood_pressure_plate.click_off",
            (false, Material::Cherry, true) => "block.cherry_wood_pressure_plate.click_on",
            (false, Material::Cherry, false) => "block.cherry_wood_pressure_plate.click_off",
            (false, Material::Nether, true) => "block.nether_wood_pressure_plate.click_on",
            (false, Material::Nether, false) => "block.nether_wood_pressure_plate.click_off",
            (false, _, true) => "block.wooden_pressure_plate.click_on",
            (false, _, false) => "block.wooden_pressure_plate.click_off",
        },
        pitch,
    ))
}

impl Game {
    /// Sounds the changes `edits` are about to make to blocks that stay the
    /// same block: doors, trapdoors and gates opening and closing, levers,
    /// buttons and plates switching. Call before applying them.
    pub(super) fn edit_sounds(&self, edits: &[(IVec3, crate::world::block::BlockState)]) {
        use crate::world::props::{Prop, door_half};
        let world = self.chunks.world();
        for &(at, new) in edits {
            let Some(old) = world.block(at.x, at.y, at.z) else {
                continue;
            };
            if old.id() != new.id() {
                continue;
            }
            let name = &new.def().name;
            let center = at.as_dvec3() + DVec3::splat(0.5);
            if new.has(Prop::Open) && old.bool(Prop::Open) != new.bool(Prop::Open) {
                // A door's two halves change together: sound the lower one.
                let upper = new.has(Prop::DoubleBlockHalf)
                    && new.get(Prop::DoubleBlockHalf) != door_half::LOWER;
                if let Some(event) = open_event(name, new.bool(Prop::Open))
                    && !upper
                {
                    audio::play(event, center);
                }
            }
            if new.has(Prop::Powered)
                && old.bool(Prop::Powered) != new.bool(Prop::Powered)
                && let Some((event, pitch)) = click_event(name, new.bool(Prop::Powered))
            {
                audio::play_at(event, center, 0.3, pitch);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn doors_gates_buttons_and_plates_click_in_their_material() {
        assert_eq!(open_event("oak_door", true), Some("block.wooden_door.open"));
        assert_eq!(
            open_event("iron_door", false),
            Some("block.iron_door.close")
        );
        assert_eq!(
            open_event("crimson_trapdoor", true),
            Some("block.nether_wood_trapdoor.open")
        );
        assert_eq!(
            open_event("spruce_fence_gate", false),
            Some("block.fence_gate.close")
        );
        assert_eq!(open_event("stone", true), None);
        assert_eq!(click_event("lever", true), Some(("block.lever.click", 0.6)));
        assert_eq!(
            click_event("stone_button", true).unwrap().0,
            "block.stone_button.click_on"
        );
        assert_eq!(
            click_event("heavy_weighted_pressure_plate", false)
                .unwrap()
                .0,
            "block.metal_pressure_plate.click_off"
        );
        assert_eq!(
            click_event("oak_pressure_plate", true).unwrap().0,
            "block.wooden_pressure_plate.click_on"
        );
        assert_eq!(
            click_event("polished_blackstone_button", true).unwrap().0,
            "block.stone_button.click_on"
        );
    }

    #[test]
    fn hurt_sounds_follow_what_hurt() {
        assert_eq!(
            hurt_event(DamageSource::Fall, 2.0),
            "entity.player.small_fall"
        );
        assert_eq!(
            hurt_event(DamageSource::Fall, 9.0),
            "entity.player.big_fall"
        );
        assert_eq!(
            hurt_event(DamageSource::Drowning, 2.0),
            "entity.player.hurt_drown"
        );
        assert_eq!(
            hurt_event(DamageSource::Lava, 4.0),
            "entity.player.hurt_on_fire"
        );
        assert_eq!(hurt_event(DamageSource::Mob, 3.0), "entity.player.hurt");
    }
}
