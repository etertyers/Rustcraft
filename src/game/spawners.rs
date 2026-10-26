//! Monster spawners (modern `BaseSpawner`): with the player within 16
//! blocks, every 10 to 40 seconds one puts up to four of its mob within 4
//! blocks, where there's room and (for monsters) the block light is 11 or
//! less, unless six are already about.

use glam::{DVec3, IVec3};

use super::Game;
use crate::world::{block_entity::BlockEntity, chunk::ChunkPos, worldgen::rng::Rng};

const ACTIVE_RANGE: f64 = 16.0;
const SPAWN_COUNT: u32 = 4;
const SPAWN_RANGE: i32 = 4;
const MAX_NEARBY: usize = 6;
const MIN_DELAY: u32 = 200;
const MAX_DELAY: u32 = 800;
/// Monsters from a spawner need block light this low.
const MAX_BLOCK_LIGHT: u8 = 11;

impl Game {
    pub(super) fn tick_spawners(&mut self) {
        let player = self.player.body.position();
        let center = ChunkPos::of_block(player.x.floor() as i32, player.z.floor() as i32);
        let mut near = Vec::new();
        for dz in -2..=2 {
            for dx in -2..=2 {
                let pos = center.offset(dx, dz);
                let Some(chunk) = self.chunks.world().get(pos) else {
                    continue;
                };
                let (x0, z0) = pos.min_block();
                for ([x, y, z], entity) in chunk.block_entities() {
                    if matches!(entity, BlockEntity::Spawner(_)) {
                        let at = IVec3::new(x0 + x as i32, y as i32, z0 + z as i32);
                        if (at.as_dvec3() + DVec3::splat(0.5)).distance(player) <= ACTIVE_RANGE {
                            near.push(at);
                        }
                    }
                }
            }
        }
        for at in near {
            let Some(BlockEntity::Spawner(spawner)) = self.chunks.block_entity_mut(at) else {
                continue;
            };
            if spawner.delay > 0 {
                spawner.delay -= 1;
                continue;
            }
            let mob = spawner.mob.clone();
            spawner.delay = MIN_DELAY + self.rng.below(MAX_DELAY - MIN_DELAY);
            self.spawn_from(at, &mob);
        }
    }

    /// Up to four of `mob` round the spawner at `at`.
    fn spawn_from(&mut self, at: IVec3, mob: &str) {
        let Some(kind) = self.mob_defs.find(mob) else {
            return;
        };
        let monster = self.mob_defs.get(kind).ai.is_monster();
        let mut rng = Rng::new(self.rng.next_u64());
        for _ in 0..SPAWN_COUNT {
            let nearby = self
                .mobs
                .iter()
                .filter(|m| m.kind == kind && !m.is_dead())
                .filter(|m| m.feet().distance(at.as_dvec3()) < 9.0)
                .count();
            if nearby >= MAX_NEARBY {
                return;
            }
            let cell = at
                + IVec3::new(
                    rng.below(2 * SPAWN_RANGE as u32 + 1) as i32 - SPAWN_RANGE,
                    rng.below(3) as i32 - 1,
                    rng.below(2 * SPAWN_RANGE as u32 + 1) as i32 - SPAWN_RANGE,
                );
            if !self.room_for_spawn(cell) {
                continue;
            }
            if monster
                && self
                    .chunks
                    .light_at(cell.x, cell.y, cell.z)
                    .is_none_or(|(_, block)| block > MAX_BLOCK_LIGHT)
            {
                continue;
            }
            let feet = cell.as_dvec3() + DVec3::new(0.5, 0.0, 0.5);
            let yaw = rng.next_f64() as f32 * std::f32::consts::TAU;
            let new = self.new_mob(kind, feet, yaw);
            self.add_mob(new);
        }
    }

    /// Solid ground under `cell` and room for a mob in it and above.
    fn room_for_spawn(&self, cell: IVec3) -> bool {
        let world = self.chunks.world();
        let blocks = self.items.blocks;
        let solid = |p: IVec3| {
            world
                .block(p.x, p.y, p.z)
                .is_some_and(|b| blocks.get(b.id()).is_some_and(|d| d.solid))
        };
        solid(cell - IVec3::Y) && !solid(cell) && !solid(cell + IVec3::Y)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        game::tests::{flat_game, mob_registry},
        player::mode::GameMode,
        world::{block::BlockState, block_entity::Spawner, blocks},
    };

    #[test]
    fn a_spawner_near_the_player_makes_its_mob_in_the_dark_up_to_six() {
        let mut game = flat_game(GameMode::Survival);
        game.mob_defs = mob_registry();
        let at = IVec3::new(5, 64, 5);
        game.chunks
            .set_blocks(&[(at, BlockState::of(blocks::SPAWNER))]);
        game.chunks
            .set_block_entity(at, Some(BlockEntity::Spawner(Spawner::new("pig"))));
        let pig = game.mob_defs.find("pig").unwrap();
        for _ in 0..2000 {
            game.tick_spawners();
        }
        let pigs = game.mobs.iter().filter(|m| m.kind == pig).count();
        assert!(
            (1..=MAX_NEARBY + SPAWN_COUNT as usize).contains(&pigs),
            "{pigs}"
        );
        // Far from the player it rests.
        game.mobs.clear();
        game.player
            .body
            .set_position(DVec3::new(200.5, 64.0, 200.5));
        for _ in 0..2000 {
            game.tick_spawners();
        }
        assert!(game.mobs.is_empty());
    }
}
