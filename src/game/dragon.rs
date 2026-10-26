//! The dragon fight (M27): what the ender dragon does to the world and the
//! player, the crystals that heal it, its fireballs and breath, and the
//! end of it all (modern `EnderDragon`, `EndDragonFight`, `EndCrystal`).
//!
//! The first time the player enters the End, ten crystals appear on their
//! pillars and the dragon starts circling. Only the dimension the player
//! is in is simulated, so it waits where it was while they're away.

use glam::{DVec3, IVec3};

use super::{
    Game,
    combat::{Strike, resolve_with},
};
use crate::{
    entity::{
        breath::{self, BreathCloud, DragonFireball},
        crystal::{self, EndCrystal},
        dragon::{self, Act, EnderDragon, Part, Phase, Senses},
        particle::Particle,
    },
    inventory::{item::ToolKind, items},
    player::{collision::Aabb, raycast::Hit, stats::DamageSource},
    world::{block::BlockState, blocks, chunk::ChunkPos, dimension::Dimension, worldgen::end},
};

/// Most blocks the dragon breaks in a tick, so one pass can't stall a frame.
const MAX_BROKEN: usize = 6000;
/// What its head and neck do to what they hit, and how hard its wings
/// throw things.
const RAM_DAMAGE: f32 = 5.0;
const BITE_DAMAGE: f32 = 10.0;
const WING_PUSH: f64 = 1.3;
/// The dragon dies this far above the top block of the exit's column.
const EXIT_HOVER: f64 = 2.0;
/// How far wings reach beyond their boxes.
const WING_GROW: DVec3 = DVec3::new(3.0, 1.0, 3.0);
/// What a crystal's blast does to the dragon, and how far it reaches.
const BLAST_DAMAGE: f32 = 10.0;
const BLAST_REACH: f64 = 12.0;
/// Colors of the breath and of the light a dying dragon throws off.
const BREATH_COLOR: u32 = 0xB32FD6;
const DEATH_COLOR: u32 = 0xE8D8FF;
/// How the dragon takes arrows: they have no more weight against it.
const SPAWN_AT: DVec3 = DVec3::new(0.0, 128.0, 0.0);

/// What the crosshair is on in the End.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EndTarget {
    Crystal(usize),
    Dragon(Part),
}

/// Blocks the dragon flies through without breaking (modern
/// `#dragon_immune`).
fn dragon_immune(state: BlockState) -> bool {
    [
        blocks::BEDROCK,
        blocks::OBSIDIAN,
        blocks::CRYING_OBSIDIAN,
        blocks::END_STONE,
        blocks::END_PORTAL,
        blocks::END_PORTAL_FRAME,
        blocks::END_GATEWAY,
        blocks::IRON_BARS,
    ]
    .iter()
    .any(|&id| state.is(id))
}

impl Game {
    /// On arriving in the End: the first time, the crystals and the dragon
    /// appear. After the dragon has died there is nothing to start.
    pub(super) fn start_fight(&mut self) {
        if self.end.fight_started {
            return;
        }
        self.end.fight_started = true;
        let seed = self.chunks.seed().unwrap_or(0);
        self.crystals = end::spikes(seed)
            .iter()
            .map(|spike| EndCrystal::new(spike.crystal(), false))
            .collect();
        let yaw = self.rng.next_f64() as f32 * std::f32::consts::TAU;
        self.dragon = Some(EnderDragon::new(SPAWN_AT, yaw, self.rng.next_u64()));
    }

    /// The dragon and crystals saved with the world, put back.
    pub(super) fn restore_fight(&mut self, dragon: Option<dragon::SavedDragon>) {
        self.crystals = self
            .end
            .crystals
            .iter()
            .map(|&(at, base)| EndCrystal::new(DVec3::from_array(at), base))
            .collect();
        let seed = self.rng.next_u64();
        self.dragon = dragon.map(|saved| EnderDragon::restore(saved, seed));
    }

    /// The fight as the level file keeps it.
    pub(super) fn fight_for_saving(&self) -> Vec<([f64; 3], bool)> {
        self.crystals
            .iter()
            .map(|c| (c.position.to_array(), c.show_base))
            .collect()
    }

    /// What the dragon senses: the player, the crystals, the fountain.
    pub(super) fn dragon_senses(&self) -> Senses {
        let podium = end::PODIUM.as_dvec3() + DVec3::new(0.5, 1.0, 0.5);
        let at = self
            .dragon
            .as_ref()
            .map_or(podium, |dragon| dragon.position);
        Senses {
            // The nearest player it may hurt (creative players are never
            // its target).
            player: self
                .players_here()
                .into_iter()
                .map(|i| self.player_at(i))
                .filter(|p| !p.stats.is_dead() && p.mode.is_vulnerable())
                .map(|p| p.body.position())
                .min_by(|a, b| a.distance(at).total_cmp(&b.distance(at))),
            crystals: self.crystals.len(),
            podium,
            exit: self.exit_point(),
        }
    }

    /// The point above the highest block of the exit portal's column, where
    /// the dragon dies (the top of the fountain, or its torches, at 0, 0).
    fn exit_point(&self) -> DVec3 {
        let (x, z) = (end::PODIUM.x, end::PODIUM.z);
        let world = self.chunks.world();
        let top = (0..crate::config::MAX_Y)
            .rev()
            .find(|&y| world.block(x, y, z).is_some_and(|b| !b.is_air()))
            .unwrap_or(end::PODIUM.y);
        DVec3::new(
            f64::from(x) + 0.5,
            f64::from(top) + EXIT_HOVER,
            f64::from(z) + 0.5,
        )
    }

    /// One tick of everything in the End's fight; nothing happens while
    /// the player is elsewhere.
    pub(super) fn tick_end(&mut self) {
        if self.dimension != Dimension::End {
            return;
        }
        for crystal in &mut self.crystals {
            crystal.age += 1;
        }
        self.tick_dragon_fireballs();
        self.tick_breath();
        let Some(mut dragon) = self.dragon.take() else {
            return;
        };
        let senses = self.dragon_senses();
        let acts = dragon.tick(&senses);
        self.dragon_sounds(&dragon);
        self.heal_from_crystals(&mut dragon);
        self.dragon_breaks_blocks(&dragon);
        // Dying, it still throws players on the way to the portal, but
        // not once the death sequence starts.
        if !dragon.in_death_sequence() {
            self.dragon_hurts_things(&dragon);
        }
        let mut finished = false;
        for act in acts {
            match act {
                Act::Fireball { from, dir } => {
                    crate::audio::play_at("entity.ender_dragon.shoot", from, 5.0, 1.0);
                    self.dragon_fireballs
                        .push(DragonFireball::new(from, dir * dragon::FIREBALL_SPEED));
                }
                Act::Breath { at } => self.breath.push(BreathCloud::new(at, breath::BREATH_CLOUD)),
                Act::Bite { at } => self.dragon_bites(at),
                Act::Burst { at } => {
                    let rng = &mut self.rng;
                    let mut random = || rng.next_f64() as f32;
                    let sparks: Vec<_> = (0..6)
                        .map(|_| Particle::colored(at, DEATH_COLOR, &mut random))
                        .collect();
                    self.add_particles(sparks);
                }
                Act::Died => finished = true,
            }
        }
        // Breathing leaves a haze of specks round its mouth.
        if dragon.phase == Phase::SittingFlaming {
            let at = dragon.head();
            let rng = &mut self.rng;
            let mut random = || rng.next_f64() as f32;
            let puffs: Vec<_> = (0..3)
                .map(|_| Particle::colored(at, BREATH_COLOR, &mut random))
                .collect();
            self.add_particles(puffs);
        }
        // Experience rains out of it over its last fifty ticks (modern).
        if dragon.is_dying() {
            let ticks = dragon.death_ticks;
            let xp = EnderDragon::kill_xp(!self.end.dragon_killed);
            if ticks >= dragon::DEATH_XP_FROM && ticks % 5 == 4 {
                self.spawn_xp((xp as f32 * 0.08) as u32, dragon.position);
            }
            if finished {
                self.spawn_xp((xp as f32 * 0.2) as u32, dragon.position);
            }
        }
        if finished {
            self.dragon_is_dead();
        } else {
            self.dragon = Some(dragon);
        }
    }

    /// Crystals within 32 blocks heal it a point every ten ticks, and the
    /// nearest shows its beam.
    fn heal_from_crystals(&mut self, dragon: &mut EnderDragon) {
        let nearest = self
            .crystals
            .iter()
            .enumerate()
            .map(|(i, c)| (i, c.center().distance(dragon.position)))
            .filter(|&(_, d)| d <= dragon::HEAL_REACH)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(i, _)| i)
            .filter(|_| !dragon.is_dying());
        for (i, crystal) in self.crystals.iter_mut().enumerate() {
            crystal.beam_to = (Some(i) == nearest).then_some(dragon.position);
        }
        if nearest.is_some() && dragon.age.is_multiple_of(dragon::HEAL_EVERY) {
            dragon.heal(1.0);
        }
    }

    /// It flies through anything not immune, breaking it.
    pub(super) fn dragon_breaks_blocks(&mut self, dragon: &EnderDragon) {
        let mut cells = Vec::new();
        'parts: for (_, bb) in dragon.parts() {
            let min = bb.min.floor().as_ivec3();
            let max = bb.max.floor().as_ivec3();
            for y in min.y..=max.y {
                for z in min.z..=max.z {
                    for x in min.x..=max.x {
                        let Some(state) = self.chunks.world().block(x, y, z) else {
                            continue;
                        };
                        if state.is_air() || dragon_immune(state) {
                            continue;
                        }
                        cells.push((IVec3::new(x, y, z), BlockState::AIR));
                        if cells.len() >= MAX_BROKEN {
                            break 'parts;
                        }
                    }
                }
            }
        }
        if !cells.is_empty() {
            cells.sort_by_key(|(at, _)| (at.x, at.y, at.z));
            cells.dedup_by_key(|(at, _)| *at);
            self.edit_later(&cells);
        }
    }

    /// Its head and neck hurt what they touch; its wings throw it.
    pub(super) fn dragon_hurts_things(&mut self, dragon: &EnderDragon) {
        // Not for ten ticks after it was hurt (modern: `hurtTime == 0`).
        if dragon.hurt_ticks > 0 {
            return;
        }
        let from = dragon.position;
        let strikers: Vec<Aabb> = dragon
            .parts()
            .into_iter()
            .filter(|(part, _)| matches!(part, Part::Head | Part::Neck))
            .map(|(_, bb)| bb)
            .collect();
        let wings: Vec<Aabb> = dragon
            .parts()
            .into_iter()
            .filter(|(part, _)| *part == Part::Wing)
            .map(|(_, bb)| Aabb::new(bb.min - WING_GROW, bb.max + WING_GROW))
            .collect();
        let sitting = dragon.phase.is_sitting();
        self.for_each_player(|game| {
            if game.player.stats.is_dead() {
                return;
            }
            let body = game.player.body.aabb();
            if strikers.iter().any(|bb| bb.intersects(&body)) && game.player.mode.is_vulnerable() {
                game.hurt_player(Strike {
                    amount: RAM_DAMAGE,
                    source: DamageSource::Mob,
                    from: Some(from),
                    ranged: false,
                    axe: false,
                });
            }
            // Creative players are neither pushed nor hurt.
            if !sitting
                && game.player.mode.is_vulnerable()
                && wings.iter().any(|bb| bb.intersects(&body))
            {
                let away = game.player.body.position() - from;
                let flat = DVec3::new(away.x, 0.0, away.z).normalize_or_zero();
                game.player.body.velocity += flat * WING_PUSH + DVec3::Y * 0.3;
            }
        });
        for mob in &mut self.mobs {
            if mob.is_dead() {
                continue;
            }
            let body = mob.body.aabb();
            if strikers.iter().any(|bb| bb.intersects(&body)) {
                mob.hurt(RAM_DAMAGE, Some(from), DamageSource::Mob);
            }
            if !sitting && wings.iter().any(|bb| bb.intersects(&body)) {
                let away = mob.feet() - from;
                let flat = DVec3::new(away.x, 0.0, away.z).normalize_or_zero();
                mob.body.velocity += flat * WING_PUSH + DVec3::Y * 0.3;
            }
        }
    }

    /// The perched dragon bites at `at`.
    fn dragon_bites(&mut self, at: DVec3) {
        self.for_each_player(|game| {
            if game.player.stats.is_dead() || !game.player.mode.is_vulnerable() {
                return;
            }
            if game.player.body.position().distance(at) <= dragon::BITE_REACH {
                game.hurt_player(Strike {
                    amount: BITE_DAMAGE,
                    source: DamageSource::Mob,
                    from: Some(at),
                    ranged: false,
                    axe: false,
                });
            }
        });
    }

    /// One tick of the dragon's fireballs: those that hit make a cloud.
    pub(super) fn tick_dragon_fireballs(&mut self) {
        let mut balls = std::mem::take(&mut self.dragon_fireballs);
        // The nearest living player to each ball can be hit by it.
        let players: Vec<Aabb> = self
            .players_here()
            .into_iter()
            .map(|i| self.player_at(i))
            .filter(|p| !p.stats.is_dead())
            .map(|p| p.body.aabb())
            .collect();
        let mut bursts = Vec::new();
        balls.retain_mut(|ball| {
            let world = self.chunks.world();
            let blocks = self.items.blocks;
            let solid = |cell: IVec3| {
                world
                    .block(cell.x, cell.y, cell.z)
                    .and_then(|block| blocks.get(block.id()))
                    .is_none_or(|def| def.solid)
            };
            let block_hit = |from: DVec3, dir: DVec3, reach: f64| {
                let hit = crate::player::raycast::raycast(from, dir, reach, solid)?;
                Aabb::block(hit.block.x, hit.block.y, hit.block.z).ray_hit(from, dir, reach)
            };
            let at = ball.position;
            let player = players.iter().min_by(|a, b| {
                let (da, db) = (
                    (a.min + a.max).distance(at * 2.0),
                    (b.min + b.max).distance(at * 2.0),
                );
                da.total_cmp(&db)
            });
            match ball.tick(block_hit, &self.mobs, player) {
                Some(at) => {
                    bursts.push(at);
                    false
                }
                None => true,
            }
        });
        self.dragon_fireballs = balls;
        for at in bursts {
            crate::audio::play_at("entity.dragon_fireball.explode", at, 4.0, 1.0);
            // The cloud lies on the ground under where it burst.
            let ground = self.ground_under(at);
            self.breath
                .push(BreathCloud::new(ground, breath::FIREBALL_CLOUD));
        }
    }

    /// The top of the first solid block at or below `at` (the spot itself
    /// if there's none within sixty blocks).
    fn ground_under(&self, at: DVec3) -> DVec3 {
        let world = self.chunks.world();
        let cell = at.floor().as_ivec3();
        for y in (cell.y - 60..=cell.y).rev() {
            if world
                .block(cell.x, y, cell.z)
                .is_some_and(|b| !b.is_air() && b.def().solid)
            {
                return DVec3::new(at.x, f64::from(y) + 1.0, at.z);
            }
        }
        at
    }

    /// One tick of the clouds of breath: they grow, and every second harm
    /// what's in them.
    pub(super) fn tick_breath(&mut self) {
        let mut clouds = std::mem::take(&mut self.breath);
        for cloud in &mut clouds {
            cloud.tick();
            if cloud.age % 4 == 0 {
                let r = f64::from(cloud.radius());
                let rng = &mut self.rng;
                let mut random = || rng.next_f64() as f32;
                let angle = f64::from(random()) * std::f64::consts::TAU;
                let spot = f64::from(random()).sqrt() * r;
                let at = cloud.center + DVec3::new(angle.cos() * spot, 0.2, angle.sin() * spot);
                let speck = Particle::colored(at, BREATH_COLOR, &mut random);
                self.add_particles([speck]);
            }
            if !cloud.pulses() {
                continue;
            }
            let held = |game: &Game| {
                !game.player.stats.is_dead()
                    && game.player.mode.is_vulnerable()
                    && cloud.holds(&game.player.body.aabb())
            };
            self.for_each_player(|game| {
                if held(game) {
                    game.player.stats.damage(breath::HARM, DamageSource::Magic);
                }
            });
            for mob in &mut self.mobs {
                if !mob.is_dead() && cloud.holds(&mob.body.aabb()) {
                    mob.hurt(breath::HARM, None, DamageSource::Magic);
                }
            }
        }
        clouds.retain(|cloud| !cloud.expired());
        self.breath = clouds;
    }

    /// The dragon is gone: the exit portal and fountain light, the egg
    /// appears for the first kill, and a gateway opens.
    fn dragon_is_dead(&mut self) {
        let first = !self.end.dragon_killed;
        self.end.dragon_killed = true;
        let seed = self.chunks.seed().unwrap_or(0);
        let origin = end::PODIUM;
        for dz in -1..=1 {
            for dx in -1..=1 {
                self.chunks
                    .load_now(ChunkPos::of_block(origin.x + dx * 16, origin.z + dz * 16));
            }
        }
        crate::audio::play_here_with("block.end_portal.spawn", 1.0, 1.0);
        let mut edits = end::podium(origin, true);
        if first {
            edits.push((origin + IVec3::Y * 4, BlockState::of(blocks::DRAGON_EGG)));
        }
        self.edit(&edits);
        let gateway = end::gateway_position(seed, self.end.gateways as usize);
        self.end.gateways += 1;
        self.chunks
            .load_now(ChunkPos::of_block(gateway.x, gateway.z));
        crate::audio::play_here_with("block.end_gateway.spawn", 1.0, 1.0);
        self.edit(&end::gateway(gateway));
        self.say("The Ender Dragon has been defeated");
        self.note(crate::advancements::Event::Used("kill_dragon".to_owned()));
        if let Err(err) = self.save() {
            log::error!("saving after the dragon's death: {err}");
        }
    }

    /// Where the End gateways that have opened stand, while the player is
    /// in the End.
    pub fn gateways(&self) -> Vec<IVec3> {
        if self.dimension != Dimension::End {
            return Vec::new();
        }
        let seed = self.chunks.seed().unwrap_or(0);
        (0..self.end.gateways as usize)
            .map(|i| end::gateway_position(seed, i))
            .collect()
    }

    /// The dragon's health as the boss bar shows it: a fraction, while one
    /// fights the player.
    pub fn dragon_bar(&self) -> Option<f32> {
        let dragon = self.dragon.as_ref()?;
        // Shown while the dragon is within a few hundred blocks, as modern's
        // boss bars are for those near the fight.
        let near = dragon.position.distance(self.player.body.position()) < 300.0;
        (self.dimension == Dimension::End && near)
            .then(|| (dragon.health / dragon::MAX_HEALTH).clamp(0.0, 1.0))
    }

    // ---- hitting things ----

    /// What the crosshair is on in the End, if nearer than any block:
    /// crystals and the dragon's parts, within the player's reach.
    pub(super) fn target_end(&self, eye: DVec3, block: Option<&Hit>) -> Option<EndTarget> {
        if self.dimension != Dimension::End {
            return None;
        }
        let dir = self.player.camera.forward().as_dvec3();
        let reach = crate::config::combat::REACH;
        let block_distance = block.map_or(f64::MAX, |hit| {
            (hit.block.as_dvec3() + DVec3::splat(0.5)).distance(eye) - 0.5
        });
        let crystal =
            self.crystals.iter().enumerate().filter_map(|(i, c)| {
                Some((EndTarget::Crystal(i), c.aabb().ray_hit(eye, dir, reach)?))
            });
        let dragon = self
            .dragon
            .as_ref()
            .filter(|d| !d.is_dying())
            .and_then(|d| {
                let (part, distance) = d.ray_hit(eye, dir, reach)?;
                Some((EndTarget::Dragon(part), distance))
            });
        crystal
            .chain(dragon)
            .filter(|&(_, distance)| distance < block_distance)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(target, _)| target)
    }

    /// A swing at a crystal or the dragon.
    pub(super) fn attack_end(&mut self, target: EndTarget) {
        let tool = self.selected_tool().cloned();
        let sword = tool.as_ref().is_some_and(|t| t.kind == ToolKind::Sword);
        let charge = self.attack_charge(0.5);
        let blow = resolve_with(self.attack_damage(), 0.0, &self.stance(charge), sword);
        self.player.attack_ticker = 0;
        match target {
            EndTarget::Crystal(index) => self.pop_crystal(index),
            EndTarget::Dragon(part) => {
                if let Some(dragon) = self.dragon.as_mut() {
                    dragon.hurt(part, blow.damage);
                }
            }
        }
        if self.player.mode.is_vulnerable() {
            self.player
                .stats
                .add_exhaustion(crate::config::combat::EXHAUSTION);
        }
        if self.player.mode.wears_tools()
            && let Some(tool) = tool
        {
            self.wear_selected(tool.hit_wear);
        }
    }

    /// An arrow in flight that would hit a crystal or the dragon before
    /// anything solid: what it hits.
    pub(super) fn arrow_hits_end(
        &self,
        from: DVec3,
        velocity: DVec3,
        wall: Option<f64>,
    ) -> Option<EndTarget> {
        if self.dimension != Dimension::End {
            return None;
        }
        let speed = velocity.length();
        if speed < 1e-9 {
            return None;
        }
        let dir = velocity / speed;
        let reach = wall.unwrap_or(speed).min(speed);
        let crystal = self.crystals.iter().enumerate().filter_map(|(i, c)| {
            Some((
                EndTarget::Crystal(i),
                c.aabb().grow(0.3).ray_hit(from, dir, reach)?,
            ))
        });
        let dragon = self
            .dragon
            .as_ref()
            .filter(|d| !d.is_dying())
            .into_iter()
            .flat_map(|d| d.parts())
            .filter_map(|(part, bb)| {
                Some((
                    EndTarget::Dragon(part),
                    bb.grow(0.3).ray_hit(from, dir, reach)?,
                ))
            });
        crystal
            .chain(dragon)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(target, _)| target)
    }

    /// An arrow lands on a crystal or the dragon for `damage`.
    pub(super) fn arrow_strikes_end(&mut self, target: EndTarget, damage: f32) {
        match target {
            EndTarget::Crystal(index) => self.pop_crystal(index),
            EndTarget::Dragon(part) => {
                if let Some(dragon) = self.dragon.as_mut() {
                    dragon.hurt(part, damage);
                }
            }
        }
    }

    /// A crystal that was hit goes off: a power-6 blast that breaks
    /// blocks, hurts what's near and hurts the dragon.
    fn pop_crystal(&mut self, index: usize) {
        if index >= self.crystals.len() {
            return;
        }
        let crystal = self.crystals.remove(index);
        self.explode(crystal.center(), crystal::BLAST_POWER);
    }

    /// What a blast at `center` of `power` does to the End's crystals
    /// (they go off quietly) and the dragon.
    pub(super) fn blast_end(&mut self, center: DVec3, power: f32) {
        let reach = f64::from(power) * 2.0;
        self.crystals
            .retain(|crystal| crystal.center().distance(center) > reach);
        if let Some(dragon) = self.dragon.as_mut()
            && dragon
                .parts()
                .iter()
                .any(|(_, bb)| ((bb.min + bb.max) / 2.0).distance(center) < BLAST_REACH)
        {
            dragon.hurt(Part::Head, BLAST_DAMAGE);
        }
    }

    /// Right-click with an end crystal on obsidian or bedrock with room
    /// above: a crystal goes there. They only work in the End.
    pub(super) fn place_crystal(&mut self, hit: &Hit) -> bool {
        if self.dimension != Dimension::End {
            self.say("End crystals only work in the End");
            return false;
        }
        let world = self.chunks.world();
        let Some(base) = world.block(hit.block.x, hit.block.y, hit.block.z) else {
            return false;
        };
        if !(base.is(blocks::OBSIDIAN) || base.is(blocks::BEDROCK)) {
            return false;
        }
        let above = hit.block + IVec3::Y;
        let free = |at: IVec3| {
            world
                .block(at.x, at.y, at.z)
                .is_some_and(|b| b.is_air() || b.is(blocks::FIRE))
        };
        if !free(above) || !free(above + IVec3::Y) {
            return false;
        }
        let position = DVec3::new(
            f64::from(above.x) + 0.5,
            f64::from(above.y),
            f64::from(above.z) + 0.5,
        );
        let probe = EndCrystal::new(position, true);
        if self
            .crystals
            .iter()
            .any(|c| c.aabb().intersects(&probe.aabb()))
        {
            return false;
        }
        if world
            .block(above.x, above.y, above.z)
            .is_some_and(|b| b.is(blocks::FIRE))
        {
            self.edit(&[(above, BlockState::AIR)]);
        }
        self.crystals.push(probe);
        if self.player.mode.uses_up_items() {
            self.player.inventory.take(self.held_slot(), 1);
        }
        self.player.view.swing();
        true
    }

    /// Whether the selected item is an end crystal.
    pub(super) fn holding_crystal(&self) -> bool {
        self.held().is_some_and(|s| s.id == items::END_CRYSTAL)
    }
}
