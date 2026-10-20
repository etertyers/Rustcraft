//! What the Nether's mobs do in the world (M24): ghast fireballs flying
//! and bursting, and sent back by a punch; piglins taking gold and
//! bartering; zombified piglins and piglins rallying round one that's
//! hurt; striders shivering off the lava, saddled, ridden and steered with
//! a warped fungus on a stick.

use glam::DVec3;

use super::Game;
use crate::{
    entity::{
        fireball::{self, Fireball, FireballHit},
        item::ItemEntity,
        mobs::Ai,
    },
    inventory::{items, stack::ItemStack},
    player::{
        collision::{Aabb, Collider, WorldCollider},
        stats::DamageSource,
    },
    world::block::Fluid,
};

/// Ticks a piglin admires gold before paying for it (modern 119).
const ADMIRE_TICKS: u32 = 120;
/// A piglin picks up gold lying this close.
const PICKUP_REACH: f64 = 1.0;
/// Kin this close sideways and up or down join in when one is hurt
/// (modern zombified piglins' 35-block follow range, 10 up and down).
const RALLY_REACH: f64 = 35.0;
const RALLY_HEIGHT: f64 = 10.0;
/// A punch reaches a fireball this far.
const DEFLECT_REACH: f64 = 4.0;
/// A returned fireball kills a ghast outright (modern `Ghast.hurt`).
const RETURN_TO_SENDER: f32 = 1000.0;

/// Modern `piglin_bartering` loot, as far as its items exist here: (item,
/// least, most, weight). Left out: potions of fire resistance (M26),
/// spectral arrows, water bottles and Soul Speed books; the iron boots
/// come without Soul Speed.
const BARTERING: [(&str, u8, u8, u32); 13] = [
    ("iron_boots", 1, 1, 8),
    ("iron_nugget", 9, 36, 10),
    ("ender_pearl", 2, 4, 10),
    ("string", 3, 9, 20),
    ("quartz", 5, 12, 20),
    ("obsidian", 1, 1, 40),
    ("crying_obsidian", 1, 3, 40),
    ("fire_charge", 1, 1, 40),
    ("leather", 2, 4, 40),
    ("soul_sand", 2, 8, 40),
    ("nether_brick", 2, 8, 40),
    ("gravel", 8, 16, 40),
    ("blackstone", 8, 16, 40),
];

impl Game {
    /// One tick of the fireballs in flight.
    pub(super) fn tick_fireballs(&mut self) {
        let mut balls = std::mem::take(&mut self.fireballs);
        let player = (!self.stats.is_dead()).then(|| self.body.aabb());
        let mut bursts = Vec::new();
        balls.retain_mut(|ball| {
            let world = self.chunks.world();
            let blocks = self.items.blocks;
            let solid = |cell: glam::IVec3| {
                world
                    .block(cell.x, cell.y, cell.z)
                    .and_then(|block| blocks.get(block.id()))
                    .is_none_or(|def| def.solid)
            };
            let block_hit = |from: DVec3, dir: DVec3, reach: f64| {
                let hit = crate::player::raycast::raycast(from, dir, reach, solid)?;
                Aabb::block(hit.block.x, hit.block.y, hit.block.z).ray_hit(from, dir, reach)
            };
            match ball.tick(block_hit, &self.mobs, player.as_ref()) {
                None => true,
                Some(hit) => {
                    bursts.push((hit, ball.returned, ball.small, ball.heading));
                    false
                }
            }
        });
        self.fireballs = balls;
        for (hit, returned, small, heading) in bursts {
            if small {
                self.small_fireball_hits(hit, heading);
                continue;
            }
            let at = match hit {
                FireballHit::Spent => continue,
                FireballHit::Block(at) => at,
                FireballHit::Player(at) => {
                    if self.mode.is_vulnerable() {
                        self.hurt_player(super::combat::Strike {
                            amount: fireball::HIT_DAMAGE,
                            source: DamageSource::Fireball,
                            from: Some(at),
                            ranged: true,
                            axe: false,
                        });
                    }
                    at
                }
                FireballHit::Mob { index, at } => {
                    let ghast = matches!(self.mob_defs.get(self.mobs[index].kind).ai, Ai::Ghast);
                    let damage = if ghast {
                        RETURN_TO_SENDER
                    } else {
                        fireball::HIT_DAMAGE
                    };
                    self.mobs[index].hurt(damage, Some(at), DamageSource::Fireball);
                    if returned {
                        self.mobs[index].hit_by_player();
                    }
                    at
                }
            };
            self.explode_with_fire(at, fireball::BURST_POWER);
        }
    }

    /// A small fireball sets the player or mob it hits alight, or lights
    /// a fire on the face of the block it hits.
    fn small_fireball_hits(&mut self, hit: FireballHit, heading: DVec3) {
        match hit {
            FireballHit::Player(at) => {
                if self.mode.is_vulnerable() {
                    self.hurt_player(super::combat::Strike {
                        amount: fireball::SMALL_HIT_DAMAGE,
                        source: DamageSource::Fireball,
                        from: Some(at),
                        ranged: true,
                        axe: false,
                    });
                    self.stats.fire = self.stats.fire.max(fireball::SMALL_FIRE_TICKS);
                }
            }
            FireballHit::Mob { index, at } => {
                let mob = &mut self.mobs[index];
                if !mob.fire_immune {
                    mob.hurt(fireball::SMALL_HIT_DAMAGE, Some(at), DamageSource::Fireball);
                    mob.stats.fire = mob.stats.fire.max(fireball::SMALL_FIRE_TICKS);
                }
            }
            FireballHit::Block(at) => {
                let cell = (at - heading * 0.05).floor().as_ivec3();
                let free = self
                    .chunks
                    .world()
                    .block(cell.x, cell.y, cell.z)
                    .is_some_and(|b| b.is_air());
                if free {
                    let fire = crate::world::block::BlockState::of(crate::world::blocks::FIRE);
                    self.edit(&[(cell, fire)]);
                }
            }
            FireballHit::Spent => {}
        }
    }

    /// A ghast's fireball loosed from `from` at `at`, starting a little
    /// ahead of it (modern: four blocks along its look).
    pub(super) fn loose_fireball(&mut self, from: DVec3, at: DVec3) {
        let ahead = from + (at - from).normalize_or_zero() * 2.5;
        self.fireballs.push(Fireball::toward(ahead, at));
    }

    /// A blaze's small fireball from `from` at `at`.
    pub(super) fn loose_small_fireball(&mut self, from: DVec3, at: DVec3) {
        let ahead = from + (at - from).normalize_or_zero() * 0.6;
        self.fireballs.push(Fireball::small(ahead, at));
    }

    /// A left click at a fireball in reach sends it back where the player
    /// looks. Returns whether one was there.
    pub(super) fn deflect_fireball(&mut self, eye: DVec3) -> bool {
        let dir = self.camera.forward().as_dvec3();
        let nearest = self
            .fireballs
            .iter()
            .enumerate()
            .filter(|(_, ball)| !ball.small)
            .filter_map(|(i, ball)| Some((i, ball.bounds().ray_hit(eye, dir, DEFLECT_REACH)?)))
            .min_by(|a, b| a.1.total_cmp(&b.1));
        let Some((index, _)) = nearest else {
            return false;
        };
        self.fireballs[index].send_back(dir);
        self.attack_ticker = 0;
        true
    }

    /// Kin of mob `index` near it get angry with the player too: every
    /// zombified piglin round a hurt one, every piglin round a piglin.
    pub(super) fn rally(&mut self, index: usize) {
        let Some(hurt) = self.mobs.get(index) else {
            return;
        };
        let kind = hurt.kind;
        if !matches!(
            self.mob_defs.get(kind).ai,
            Ai::ZombifiedPiglin { .. } | Ai::Piglin { .. }
        ) {
            return;
        }
        let at = hurt.feet();
        for mob in &mut self.mobs {
            let away = mob.feet() - at;
            if mob.kind == kind
                && !mob.is_dead()
                && away.x.hypot(away.z) < RALLY_REACH
                && away.y.abs() < RALLY_HEIGHT
            {
                mob.ai.enrage();
            }
        }
    }

    /// Piglins pick up gold ingots lying near them and admire them for six
    /// seconds, then throw out what they barter for; striders shiver off
    /// the lava.
    pub(super) fn tick_nether_mobs(&mut self) {
        let defs = self.mob_defs.clone();
        let mut paid = Vec::new();
        for index in 0..self.mobs.len() {
            if self.mobs[index].is_dead() {
                continue;
            }
            match defs.get(self.mobs[index].kind).ai {
                Ai::Piglin { .. } => {
                    let mob = &mut self.mobs[index];
                    if mob.data.timer > 0 {
                        mob.data.timer -= 1;
                        if mob.data.timer == 0 {
                            paid.push(mob.feet() + DVec3::Y * 1.2);
                        }
                        continue;
                    }
                    let reach = mob.body.aabb().grow(PICKUP_REACH);
                    if let Some(gold) = self.entities.iter_mut().find(|item| {
                        item.stack.id == items::GOLD_INGOT
                            && item.stack.count > 0
                            && reach.contains(item.center())
                    }) {
                        gold.stack.count -= 1;
                        self.mobs[index].data.timer = ADMIRE_TICKS;
                    }
                }
                Ai::Strider => {
                    let mob = &self.mobs[index];
                    let collider = WorldCollider {
                        world: self.chunks.world(),
                        blocks: self.items.blocks,
                    };
                    let feet = mob.feet();
                    let warm = collider.liquid(&mob.body.aabb()) == Some(Fluid::Lava)
                        || collider.fluid_at(feet - DVec3::Y * 0.5) == Some(Fluid::Lava);
                    self.mobs[index].data.variant = u8::from(!warm);
                }
                _ => {}
            }
        }
        self.entities.retain(|item| item.stack.count > 0);
        for at in paid {
            self.barter(at);
        }
    }

    /// Throws out one roll of the bartering loot from `at`, toward the
    /// player.
    fn barter(&mut self, at: DVec3) {
        let total: u32 = BARTERING.iter().map(|b| b.3).sum();
        let mut roll = self.rng.below(total);
        let Some(&(name, least, most, _)) = BARTERING.iter().find(|b| {
            let hit = roll < b.3;
            roll = roll.saturating_sub(b.3);
            hit
        }) else {
            return;
        };
        let Some(item) = items::by_name(name) else {
            return;
        };
        let count = least + self.rng.below(u32::from(most - least) + 1) as u8;
        let toward = (self.body.position() - at).with_y(0.0).normalize_or_zero();
        let velocity = toward * 0.3 + DVec3::Y * 0.3;
        let hover = self.rng.next_f64() as f32 * std::f32::consts::TAU;
        self.entities.push(ItemEntity::new(
            ItemStack::new(item, count),
            at,
            velocity,
            hover,
        ));
    }

    /// Right-click on a strider: a saddle saddles it, and a saddled one is
    /// sat on.
    pub(super) fn use_strider(&mut self, index: usize, held: Option<ItemStack>) -> bool {
        let mob = &mut self.mobs[index];
        if let Some(stack) = held
            && stack.id == items::SADDLE
            && !mob.data.saddled
        {
            mob.data.saddled = true;
            if self.mode.uses_up_items() {
                self.inventory.take(self.held_slot(), 1);
            }
            return true;
        }
        if mob.data.saddled && self.mounted.is_none() && self.riding.is_none() {
            self.mounted = Some(mob.id);
            mob.ridden = true;
            return true;
        }
        false
    }

    /// Whether the player steers the mob they ride: a horse always (a wild
    /// one only bucks), a strider while they hold a warped fungus on a
    /// stick; otherwise it goes its own way.
    pub(super) fn steers_mount(&self, ai: Ai) -> bool {
        match ai {
            Ai::Strider => self
                .held()
                .is_some_and(|stack| stack.id == items::WARPED_FUNGUS_ON_A_STICK),
            _ => true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::tests::{flat_game, mob_registry};
    use crate::player::mode::GameMode;

    #[test]
    fn every_bartered_item_exists() {
        for (name, least, most, weight) in BARTERING {
            assert!(items::by_name(name).is_some(), "{name}");
            assert!(least <= most && weight > 0);
        }
    }

    #[test]
    fn a_piglin_takes_thrown_gold_admires_it_and_pays() {
        let mut game = flat_game(GameMode::Creative);
        game.mob_defs = mob_registry();
        let kind = game.mob_defs.find("piglin").unwrap();
        let feet = DVec3::new(4.5, 64.0, 0.5);
        let piglin = game.new_mob(kind, feet, 0.0);
        game.add_mob(piglin);
        game.entities.push(ItemEntity::new(
            ItemStack::new(items::GOLD_INGOT, 1),
            feet + DVec3::new(0.3, 0.2, 0.0),
            DVec3::ZERO,
            0.0,
        ));
        game.tick_nether_mobs();
        assert!(game.entities.is_empty(), "picked up");
        assert_eq!(game.mobs[0].data.timer, ADMIRE_TICKS);
        for _ in 0..ADMIRE_TICKS {
            game.tick_nether_mobs();
        }
        assert_eq!(game.mobs[0].data.timer, 0);
        assert_eq!(game.entities.len(), 1, "something bartered");
        let paid = game.entities[0].stack.id.name();
        assert!(BARTERING.iter().any(|b| b.0 == paid), "{paid}");
    }

    #[test]
    fn hurting_a_zombified_piglin_angers_the_pack_but_not_others() {
        let mut game = flat_game(GameMode::Survival);
        game.mob_defs = mob_registry();
        let zp = game.mob_defs.find("zombified_piglin").unwrap();
        let pig = game.mob_defs.find("pig").unwrap();
        for (kind, x) in [(zp, 3.0), (zp, 20.0), (zp, 60.0), (pig, 5.0)] {
            let mob = game.new_mob(kind, DVec3::new(x, 64.0, 0.5), 0.0);
            game.add_mob(mob);
        }
        game.rally(0);
        let angry: Vec<bool> = game.mobs.iter().map(|m| m.ai.angry > 0).collect();
        assert_eq!(angry, [true, true, false, false]);
    }

    #[test]
    fn a_ghast_fireballs_a_player_in_sight() {
        let mut game = flat_game(GameMode::Survival);
        game.mob_defs = mob_registry();
        let kind = game.mob_defs.find("ghast").unwrap();
        let ghast = game.new_mob(kind, DVec3::new(0.5, 66.0, -14.5), 0.0);
        game.add_mob(ghast);
        let mut loosed = false;
        for _ in 0..200 {
            game.tick();
            loosed |= !game.fireballs.is_empty();
            if game.stats.health < 20.0 {
                break;
            }
        }
        assert!(loosed, "a fireball flew");
        assert!(game.stats.health < 20.0, "and hit");
        assert!(matches!(
            game.stats.last_source,
            Some(DamageSource::Fireball | DamageSource::Explosion)
        ));
    }

    #[test]
    fn a_blaze_sets_a_player_in_sight_alight_with_small_fireballs() {
        let mut game = flat_game(GameMode::Survival);
        game.mob_defs = mob_registry();
        game.summon("blaze", DVec3::new(0.5, 66.0, -10.5), 0.0);
        let mut burnt = false;
        for _ in 0..200 {
            game.tick();
            burnt |= game.stats.fire > 0;
            if burnt {
                break;
            }
        }
        assert!(burnt, "set alight");
        assert!(game.stats.health < 20.0);
        // Blazes' fireballs can't be punched back.
        game.loose_small_fireball(DVec3::new(0.5, 65.6, -3.0), DVec3::new(0.5, 65.6, 0.5));
        game.camera.pitch = 0.0;
        game.camera.yaw = 0.0;
        let eye = game.body.position() + DVec3::Y * game.body.eye_height();
        assert!(!game.deflect_fireball(eye));
    }

    #[test]
    fn a_strider_walks_on_lava_unhurt_and_goes_where_its_rider_looks() {
        let mut game = flat_game(GameMode::Creative);
        game.mob_defs = mob_registry();
        // A lava pool 9 wide, level with the ground.
        let lava = crate::world::block::BlockState::of(crate::world::blocks::LAVA);
        let mut pool = Vec::new();
        for z in -4..=4 {
            for x in 4..=20 {
                pool.push((glam::IVec3::new(x, 63, z), lava));
            }
        }
        game.chunks.set_blocks(&pool);
        let kind = game.mob_defs.find("strider").unwrap();
        let strider = game.new_mob(kind, DVec3::new(6.5, 64.0, 0.5), 0.0);
        game.add_mob(strider);
        for _ in 0..40 {
            game.tick();
        }
        let mob = &game.mobs[0];
        assert!(
            (mob.feet().y - 64.0).abs() < 0.05,
            "on the lava at {}",
            mob.feet().y
        );
        assert_eq!(mob.stats.health, 20.0);
        assert_eq!(mob.stats.fire, 0);
        assert_eq!(mob.data.variant, 0, "warm");

        // Saddled and ridden with a warped fungus on a stick, looking east.
        game.inventory
            .set(0, Some(ItemStack::new(items::SADDLE, 1)));
        assert!(game.use_strider(0, Some(ItemStack::new(items::SADDLE, 1))));
        assert!(game.use_strider(0, None));
        game.inventory
            .set(0, Some(ItemStack::new(items::WARPED_FUNGUS_ON_A_STICK, 1)));
        game.camera.yaw = std::f32::consts::FRAC_PI_2;
        let start = game.mobs[0].feet();
        for _ in 0..40 {
            game.tick();
        }
        let moved = game.mobs[0].feet() - start;
        assert!(moved.x > 2.0 && moved.z.abs() < 1.0, "went east: {moved}");
    }

    #[test]
    fn a_strider_shivers_out_of_the_lava() {
        let mut game = flat_game(GameMode::Creative);
        game.mob_defs = mob_registry();
        let kind = game.mob_defs.find("strider").unwrap();
        let mob = game.new_mob(kind, DVec3::new(2.5, 64.0, 2.5), 0.0);
        game.add_mob(mob);
        game.tick_nether_mobs();
        assert_eq!(game.mobs[0].data.variant, 1, "cold on grass");
        let lava = crate::world::block::BlockState::of(crate::world::blocks::LAVA);
        game.chunks
            .set_blocks(&[(glam::IVec3::new(2, 63, 2), lava)]);
        game.tick_nether_mobs();
        assert_eq!(game.mobs[0].data.variant, 0, "warm over lava");
    }
}
