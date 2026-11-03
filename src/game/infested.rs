//! Infested blocks and the silverfish in them (M52): breaking one without
//! Silk Touch lets a silverfish out, and a silverfish that is hurt wakes
//! the ones hiding in infested blocks about it (10 blocks each way along
//! x and z, 5 along y).

use glam::{DVec3, IVec3};

use super::Game;
use crate::{entity::mob::HURT_FLASH, world::block::BlockState};

/// How far a hurt silverfish calls to the ones in the walls.
const CALL: IVec3 = IVec3::new(10, 5, 10);
/// The most it wakes at once, so a wall doesn't empty all in one tick.
const WAKES: usize = 4;

/// Whether `block` is one of the infested blocks.
pub fn is_infested(block: BlockState) -> bool {
    block.def().name.starts_with("infested_")
}

impl Game {
    /// A silverfish comes out of the block at `cell`.
    pub(super) fn release_silverfish(&mut self, cell: IVec3) {
        let at = cell.as_dvec3() + DVec3::new(0.5, 0.0, 0.5);
        self.summon("silverfish", at, 0.0);
    }

    /// Silverfish hurt this tick wake the infested blocks near them.
    pub(super) fn tick_silverfish_calls(&mut self) {
        let hurt: Vec<IVec3> = self
            .mobs
            .iter()
            .filter(|mob| {
                !mob.is_dead()
                    && mob.hurt_time + 1 == HURT_FLASH
                    && self.mob_defs.get(mob.kind).name == "silverfish"
            })
            .map(|mob| mob.body.position().floor().as_ivec3())
            .collect();
        for at in hurt {
            let mut woken = Vec::new();
            'search: for dy in -CALL.y..=CALL.y {
                for dz in -CALL.z..=CALL.z {
                    for dx in -CALL.x..=CALL.x {
                        let cell = at + IVec3::new(dx, dy, dz);
                        let Some(block) = self.chunks.world().block(cell.x, cell.y, cell.z) else {
                            continue;
                        };
                        if is_infested(block) {
                            woken.push(cell);
                            if woken.len() >= WAKES {
                                break 'search;
                            }
                        }
                    }
                }
            }
            let edits: Vec<_> = woken.iter().map(|&c| (c, BlockState::AIR)).collect();
            self.edit(&edits);
            for cell in woken {
                self.release_silverfish(cell);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        game::tests::flat_game,
        inventory::{enchant::Enchantment, items, stack::ItemStack},
        player::{mode::GameMode, stats::DamageSource},
    };

    fn silverfish(game: &Game) -> usize {
        game.mobs
            .iter()
            .filter(|m| !m.is_dead() && game.mob_defs.get(m.kind).name == "silverfish")
            .count()
    }

    fn infested_in(game: &mut Game, at: &[IVec3]) {
        let block = BlockState::parse("infested_stone").unwrap();
        let edits: Vec<_> = at.iter().map(|&c| (c, block)).collect();
        game.chunks.set_blocks(&edits);
    }

    fn game() -> Game {
        let mut game = flat_game(GameMode::Survival);
        game.mob_defs = crate::game::tests::mob_registry();
        game
    }

    #[test]
    fn breaking_an_infested_block_lets_a_silverfish_out_unless_it_is_silk_touched() {
        let mut game = game();
        let (a, b) = (IVec3::new(3, 64, 3), IVec3::new(5, 64, 3));
        infested_in(&mut game, &[a, b]);
        assert!(is_infested(
            game.chunks.world().block(a.x, a.y, a.z).unwrap()
        ));
        // Infested blocks break at once and drop nothing.
        let def = game
            .chunks
            .world()
            .block(a.x, a.y, a.z)
            .unwrap()
            .def()
            .clone();
        assert_eq!(def.hardness, 0.0);
        game.break_block(a);
        assert_eq!(silverfish(&game), 1);
        assert!(game.entities.is_empty(), "no drops");
        // With Silk Touch, none comes out.
        let mut pick = ItemStack::new(items::by_name("iron_pickaxe").unwrap(), 1);
        pick.enchant(Enchantment::SilkTouch, 1);
        game.player.inventory.set(0, Some(pick));
        game.player.inventory.selected = 0;
        game.break_block(b);
        assert_eq!(silverfish(&game), 1);
    }

    #[test]
    fn a_hurt_silverfish_wakes_the_infested_blocks_around_it_but_not_far_ones() {
        let mut game = game();
        let near = [
            IVec3::new(6, 64, 4),
            IVec3::new(2, 65, 5),
            IVec3::new(8, 66, 0),
        ];
        let far = IVec3::new(30, 64, 4);
        infested_in(&mut game, &[near[0], near[1], near[2], far]);
        game.release_silverfish(IVec3::new(4, 64, 4));
        assert_eq!(silverfish(&game), 1);
        game.run_ticks(2);
        game.mobs[0].hurt(1.0, None, DamageSource::Mob);
        game.run_ticks(2);
        assert_eq!(silverfish(&game), 4, "three woke");
        for cell in near {
            assert!(
                game.chunks
                    .world()
                    .block(cell.x, cell.y, cell.z)
                    .unwrap()
                    .is_air()
            );
        }
        assert!(is_infested(
            game.chunks.world().block(far.x, far.y, far.z).unwrap()
        ));
    }
}
