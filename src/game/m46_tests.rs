//! M46: spectators are pushed by nothing; flying sprint never costs food.

use glam::{DVec3, IVec3};

use super::tests::flat_game;
use crate::{
    player::{input::MoveKeys, mode::GameMode},
    world::{block::BlockState, blocks, props::Prop},
};

/// A spectator standing (hovering) at the origin of the flat world.
fn spectator() -> super::Game {
    let mut game = flat_game(GameMode::Creative);
    game.set_game_mode(GameMode::Spectator);
    game.player.body.set_position(DVec3::new(0.5, 64.0, 0.5));
    game.player.body.velocity = DVec3::ZERO;
    game
}

#[test]
fn a_piston_a_blast_and_a_mob_leave_a_spectator_where_they_are() {
    let mut game = spectator();
    let start = game.player.body.position();
    // A piston whose head would reach the spectator's cell.
    let piston =
        BlockState::of(blocks::PISTON).with_face(Prop::FacingAll, crate::world::Face::West);
    game.set_blocks(&[
        (IVec3::new(1, 64, 0), piston),
        (IVec3::new(1, 65, 0), BlockState::of(blocks::REDSTONE_BLOCK)),
    ]);
    game.run_ticks(3);
    assert_eq!(game.player.body.position(), start, "a piston");
    game.explode(DVec3::new(0.5, 64.5, 2.5), 4.0);
    assert_eq!(game.player.body.velocity, DVec3::ZERO, "a blast");
    crate::entity::mob::knock_back(&mut game.player.body, DVec3::new(0.5, 64.0, 2.5));
    assert_eq!(game.player.body.velocity, DVec3::ZERO, "a blow");
    game.run_ticks(20);
    assert_eq!(game.player.body.position(), start, "nothing moved them");
}

#[test]
fn flying_sprint_costs_no_food_in_creative_and_spectator() {
    for mode in [GameMode::Creative, GameMode::Spectator] {
        let mut game = flat_game(GameMode::Creative);
        game.set_game_mode(mode);
        game.player.body.set_position(DVec3::new(0.5, 70.0, 0.5));
        game.player.body.flying = true;
        game.player.stats.food = 0;
        game.player.keys = {
            let mut keys = MoveKeys::default();
            keys.set_key(crate::bindings::Action::Forward, true);
            keys.set_key(crate::bindings::Action::Sprint, true);
            keys
        };
        let exhaustion = game.player.stats.exhaustion;
        game.run_ticks(40);
        assert!(game.player.body.sprinting, "{mode:?} sprints at 0 food");
        assert_eq!(game.player.stats.food, 0);
        assert_eq!(game.player.stats.exhaustion, exhaustion, "{mode:?}");
    }
}
