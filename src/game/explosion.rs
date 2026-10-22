//! Explosions and primed TNT, following modern Minecraft's `Explosion`:
//! the blocks a blast destroys come from `world::explosion`; things within
//! twice its power are hurt and thrown by how close and how exposed they
//! are; TNT blocks caught in it are lit with a short fuse.

use glam::{DVec3, IVec3};

use super::Game;
use crate::{
    entity::{item::ItemEntity, tnt::PrimedTnt},
    player::{
        collision::{Aabb, WorldCollider},
        raycast,
        stats::DamageSource,
    },
    world::{blocks, explosion, harvest},
};

/// The blast of TNT (modern `PrimedTnt.explode`).
pub const TNT_POWER: f32 = 4.0;
/// Damage on normal difficulty is this times the usual (modern: ×1 on
/// normal), here the factor of the formula's 7.
const DAMAGE_FACTOR: f32 = 7.0;

impl Game {
    /// Lights the TNT at `at` (already gone from the world).
    pub(super) fn prime_tnt(&mut self, at: IVec3) {
        self.prime_at(at, crate::entity::tnt::FUSE);
    }

    fn prime_at(&mut self, at: IVec3, fuse: i32) {
        let angle = self.rng.next_f64() * std::f64::consts::TAU;
        let position = at.as_dvec3() + DVec3::new(0.5, 0.0, 0.5);
        self.primed.push(PrimedTnt::new(position, angle, fuse));
    }

    /// One tick of lit TNT.
    pub(super) fn tick_tnt(&mut self) {
        if self.primed.is_empty() {
            return;
        }
        let mut primed = std::mem::take(&mut self.primed);
        let mut blasts = Vec::new();
        primed.retain_mut(|tnt| {
            let collider = WorldCollider {
                world: self.chunks.world(),
                blocks: self.items.blocks,
            };
            let blew = tnt.tick(&collider);
            if blew {
                blasts.push(tnt.center());
            }
            !blew
        });
        self.primed.extend(primed);
        for center in blasts {
            self.explode(center, TNT_POWER);
        }
    }

    /// Whether the cell is solid enough to hide behind.
    fn hides(&self, cell: IVec3) -> bool {
        self.chunks
            .world()
            .block(cell.x, cell.y, cell.z)
            .is_some_and(|state| state.def().solid)
    }

    /// The share of `bounds` that `center` can see (modern
    /// `Explosion.getSeenPercent`): points across the box, each with a
    /// clear line to the blast.
    fn exposure(&self, center: DVec3, bounds: &Aabb) -> f32 {
        let size = bounds.max - bounds.min;
        let step = DVec3::ONE / (size * 2.0 + DVec3::ONE);
        let margin = (DVec3::ONE - (DVec3::ONE / step).floor() * step) / 2.0;
        let (mut seen, mut total) = (0, 0);
        let mut x = 0.0;
        while x <= 1.0 {
            let mut y = 0.0;
            while y <= 1.0 {
                let mut z = 0.0;
                while z <= 1.0 {
                    let point = DVec3::new(
                        bounds.min.x + size.x * x + margin.x,
                        bounds.min.y + size.y * y,
                        bounds.min.z + size.z * z + margin.z,
                    );
                    let to = center - point;
                    let blocked = raycast::raycast(point, to, to.length(), |cell| self.hides(cell));
                    if blocked.is_none() {
                        seen += 1;
                    }
                    total += 1;
                    z += step.z;
                }
                y += step.y;
            }
            x += step.x;
        }
        seen as f32 / total as f32
    }

    /// The hurt and throw a blast of `power` at `center` gives something
    /// with `bounds` and its eyes at `eye`: (damage, push).
    fn blast_on(
        &self,
        center: DVec3,
        power: f32,
        feet: DVec3,
        eye: DVec3,
        bounds: &Aabb,
    ) -> Option<(f32, DVec3)> {
        let reach = f64::from(power) * 2.0;
        let distance = feet.distance(center) / reach;
        if distance > 1.0 {
            return None;
        }
        let away = eye - center;
        let length = away.length();
        if length == 0.0 {
            return None;
        }
        let impact = ((1.0 - distance) as f32) * self.exposure(center, bounds);
        let damage =
            ((impact * impact + impact) / 2.0 * DAMAGE_FACTOR * reach as f32 + 1.0).floor();
        Some((damage, away / length * f64::from(impact)))
    }

    /// Hurts and throws what is near a blast.
    fn blast_entities(&mut self, center: DVec3, power: f32) {
        // The player.
        if !self.stats.is_dead() {
            let bounds = self.body.aabb();
            let eye = self.body.position() + DVec3::Y * self.body.eye_height();
            if let Some((damage, push)) =
                self.blast_on(center, power, self.body.position(), eye, &bounds)
                && self.mode.is_vulnerable()
            {
                self.stats.damage(damage, DamageSource::Explosion);
                self.body.velocity += push;
            }
        }
        // Mobs.
        for index in 0..self.mobs.len() {
            let mob = &self.mobs[index];
            if mob.is_dead() {
                continue;
            }
            let hit = self.blast_on(center, power, mob.feet(), mob.eye(), &mob.body.aabb());
            if let Some((damage, push)) = hit {
                let mob = &mut self.mobs[index];
                mob.hurt(damage, None, DamageSource::Explosion);
                mob.body.velocity += push;
            }
        }
        // The End's crystals and dragon.
        self.blast_end(center, power);
        // Lit TNT is thrown; items take damage and may burn up.
        for index in 0..self.primed.len() {
            let tnt = &self.primed[index];
            let eye = tnt.position + DVec3::Y * 0.49;
            if let Some((_, push)) = self.blast_on(center, power, tnt.position, eye, &tnt.aabb()) {
                self.primed[index].velocity += push;
            }
        }
        for index in 0..self.entities.len() {
            let item = &self.entities[index];
            let hit = self.blast_on(center, power, item.center(), item.center(), &item.bounds());
            if let Some((damage, _)) = hit {
                self.entities[index].hurt(damage as i32);
            }
        }
    }

    /// Blows up at `center`: hurts what is near, destroys blocks (each
    /// dropping with a chance of one in `power`) and lights TNT in the way.
    pub fn explode(&mut self, center: DVec3, power: f32) {
        self.blow_up(center, power);
    }

    /// Blows up at `center` like `explode`, then sets fire to a third of
    /// the cleared cells with ground under them (modern explosions that
    /// cause fire: beds in the Nether, ghast fireballs).
    pub fn explode_with_fire(&mut self, center: DVec3, power: f32) {
        let cleared = self.blow_up(center, power);
        let mut fires = Vec::new();
        for cell in cleared {
            let world = self.chunks.world();
            let free = world
                .block(cell.x, cell.y, cell.z)
                .is_some_and(|b| b.is_air());
            let ground = world
                .block(cell.x, cell.y - 1, cell.z)
                .is_some_and(|b| b.def().is_opaque_cube());
            if free && ground && self.rng.below(3) == 0 {
                fires.push((cell, crate::world::block::BlockState::of(blocks::FIRE)));
            }
        }
        self.edit(&fires);
    }

    /// The explosion itself; returns the cells it cleared.
    fn blow_up(&mut self, center: DVec3, power: f32) -> Vec<glam::IVec3> {
        let rng = &mut self.rng;
        let mut random = || rng.next_f64() as f32;
        let cells = explosion::destroyed(self.chunks.world(), center, power, &mut random);
        self.blast_entities(center, power);
        let mut edits = Vec::with_capacity(cells.len());
        for cell in cells {
            let Some(state) = self.chunks.world().block(cell.x, cell.y, cell.z) else {
                continue;
            };
            if state.def().hardness < 0.0 {
                continue;
            }
            if state.is(blocks::TNT) {
                // 80 ticks quartered, plus an eighth (modern: 10–29).
                let fuse = (self.rng.next_f64() * 20.0) as i32 + 10;
                self.prime_at(cell, fuse);
            }
            let rng = &mut self.rng;
            let mut random = || rng.next_f64() as f32;
            if !state.is(blocks::TNT) && random() < 1.0 / power {
                for stack in harvest::drops(state, None, &mut random) {
                    self.entities
                        .push(ItemEntity::from_block(stack, cell, &mut random));
                }
            }
            if let Some(held) = self.chunks.set_block_entity(cell, None) {
                for stack in held.contents() {
                    self.entities
                        .push(ItemEntity::from_block(stack, cell, &mut random));
                }
            }
            edits.push((cell, crate::world::block::BlockState::AIR));
        }
        self.edit(&edits);
        edits.into_iter().map(|(cell, _)| cell).collect()
    }
}

#[cfg(test)]
mod tests {
    use glam::{DVec3, IVec3};

    use crate::{
        game::tests::flat_game,
        player::mode::GameMode,
        world::{block::BlockState, blocks},
    };

    fn block(game: &crate::game::Game, at: IVec3) -> BlockState {
        game.chunks.world().block(at.x, at.y, at.z).unwrap()
    }

    #[test]
    fn powered_tnt_is_lit_pops_up_and_blows_a_crater_after_four_seconds() {
        let mut game = flat_game(GameMode::Creative);
        let at = IVec3::new(8, 64, -8);
        game.set_blocks(&[(at, BlockState::of(blocks::TNT))]);
        game.run_ticks(1);
        game.set_blocks(&[(
            IVec3::new(8, 64, -7),
            BlockState::of(blocks::REDSTONE_BLOCK),
        )]);
        game.run_ticks(2);
        assert_eq!(block(&game, at), BlockState::AIR, "the block is gone");
        assert_eq!(game.primed.len(), 1);
        assert!(game.primed[0].fuse < 80);
        game.run_ticks(80);
        assert!(game.primed.is_empty());
        // The grass and dirt below are blown away: a crater.
        assert_eq!(block(&game, IVec3::new(8, 63, -8)), BlockState::AIR);
        assert_eq!(block(&game, IVec3::new(8, 62, -8)), BlockState::AIR);
        // And bedrock holds.
        assert_eq!(
            block(&game, IVec3::new(8, 0, -8)),
            BlockState::of(blocks::BEDROCK)
        );
    }

    #[test]
    fn tnt_caught_in_a_blast_goes_off_in_a_chain() {
        let mut game = flat_game(GameMode::Creative);
        let tnt = BlockState::of(blocks::TNT);
        game.set_blocks(&[
            (IVec3::new(8, 64, -10), tnt),
            (IVec3::new(10, 64, -10), tnt),
            (IVec3::new(8, 64, -12), tnt),
        ]);
        game.run_ticks(1);
        game.set_blocks(&[(
            IVec3::new(8, 64, -9),
            BlockState::of(blocks::REDSTONE_BLOCK),
        )]);
        game.run_ticks(200);
        assert!(game.primed.is_empty());
        for at in [IVec3::new(10, 64, -10), IVec3::new(8, 64, -12)] {
            assert_eq!(
                block(&game, at),
                BlockState::AIR,
                "{at} was lit by the first blast"
            );
        }
    }

    #[test]
    fn a_blast_hurts_and_throws_a_survival_player_by_how_close_they_are() {
        let mut game = flat_game(GameMode::Survival);
        game.body.set_position(DVec3::new(4.5, 64.0, 4.5));
        let before = game.stats.health;
        game.explode(DVec3::new(4.5, 64.5, 6.5), 4.0);
        let near = before - game.stats.health;
        assert!(near > 8.0, "two blocks from a TNT blast is deadly: {near}");
        assert!(game.body.velocity.z < -0.3, "thrown away from it");
        let mut far = flat_game(GameMode::Survival);
        far.body.set_position(DVec3::new(4.5, 64.0, 4.5));
        far.explode(DVec3::new(4.5, 64.5, 11.5), 4.0);
        let lost = 20.0 - far.stats.health;
        assert!(lost > 0.0 && lost < near, "{lost} vs {near}");
        let mut safe = flat_game(GameMode::Survival);
        safe.body.set_position(DVec3::new(4.5, 64.0, 4.5));
        safe.explode(DVec3::new(4.5, 64.5, 14.0), 4.0);
        assert_eq!(safe.stats.health, 20.0, "more than twice the power away");
    }

    #[test]
    fn creative_players_are_unhurt_and_walls_shield_from_the_blast() {
        let mut game = flat_game(GameMode::Creative);
        game.body.set_position(DVec3::new(4.5, 64.0, 4.5));
        game.explode(DVec3::new(4.5, 64.5, 6.5), 4.0);
        assert_eq!(game.stats.health, 20.0);
        let mut shielded = flat_game(GameMode::Survival);
        shielded.body.set_position(DVec3::new(4.5, 64.0, 4.5));
        let wall: Vec<_> = (63..70)
            .flat_map(|y| {
                (2..8).map(move |x| (IVec3::new(x, y, 5), BlockState::of(blocks::OBSIDIAN)))
            })
            .collect();
        shielded.set_blocks(&wall);
        shielded.explode(DVec3::new(4.5, 64.5, 6.5), 4.0);
        // Unseen, only the formula's flat point of damage is left.
        assert!(shielded.stats.health >= 19.0, "an obsidian wall shields");
        assert!(shielded.body.velocity.z.abs() < 0.01);
    }
}
