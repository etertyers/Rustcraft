//! Tests of the modern movement rules through the whole game: hunger
//! and sprinting, swimming, crawling, and landing on ladders.

use crate::bindings::Action;
use glam::{DVec3, IVec3};

use super::tests::flat_game;
use crate::{
    player::{mode::GameMode, physics::Pose},
    world::{block::BlockState, blocks},
};

fn press(game: &mut super::Game, action: Action) {
    game.on_action(action, true, false, true);
}

#[test]
fn hunger_of_six_or_less_cant_sprint() {
    for (food, sprints) in [(6, false), (7, true)] {
        let mut game = flat_game(GameMode::Survival);
        game.stats.food = food;
        game.camera.pitch = 0.0;
        press(&mut game, Action::Forward);
        press(&mut game, Action::Sprint);
        game.run_ticks(10);
        assert_eq!(game.body.sprinting, sprints, "food {food}");
    }
    // Creative players eat nothing and sprint regardless.
    let mut game = flat_game(GameMode::Creative);
    game.stats.food = 0;
    press(&mut game, Action::Forward);
    press(&mut game, Action::Sprint);
    game.run_ticks(10);
    assert!(game.body.sprinting);
}

#[test]
fn running_out_of_hunger_mid_sprint_stops_it() {
    let mut game = flat_game(GameMode::Survival);
    game.camera.pitch = 0.0;
    press(&mut game, Action::Forward);
    press(&mut game, Action::Sprint);
    game.run_ticks(10);
    assert!(game.body.sprinting);
    game.stats.food = 6;
    game.run_ticks(2);
    assert!(!game.body.sprinting);
}

#[test]
fn sprinting_with_the_head_under_water_swims() {
    let mut game = flat_game(GameMode::Survival);
    game.camera.pitch = 0.0;
    let water: Vec<_> = (-3..=3)
        .flat_map(|x| {
            (-8..=3).flat_map(move |z| {
                (64..=67).map(move |y| (IVec3::new(x, y, z), BlockState::of(blocks::WATER)))
            })
        })
        .collect();
    game.set_blocks(&water);
    press(&mut game, Action::Forward);
    press(&mut game, Action::Sprint);
    game.run_ticks(25);
    game.advance(0.0);
    assert!(game.body.swimming, "{:?}", game.body.position());
    assert_eq!(game.body.pose(), Pose::Swimming);
    assert!(game.is_swimming());
    // The model lies down (in water: following the look direction) and
    // the camera sits close to the feet.
    let pose = game.player_pose();
    assert!(pose.swim_amount > 0.9 && pose.in_water, "{pose:?}");
    let eye = game.camera.position.y - game.body.position().y as f32;
    assert!((eye - 0.4).abs() < 0.02, "{eye}");
    // Swimming is flat: a 0.6-block-tall hitbox.
    let bb = game.body.aabb();
    assert!((bb.max.y - bb.min.y - 0.6).abs() < 1e-9);
}

#[test]
fn a_block_over_your_head_makes_you_crawl_and_removing_it_stands_you_up() {
    let mut game = flat_game(GameMode::Survival);
    game.camera.pitch = 0.0;
    game.set_blocks(&[(IVec3::new(0, 65, 0), BlockState::of(blocks::STONE))]);
    game.run_ticks(2);
    assert_eq!(game.body.pose(), Pose::Swimming);
    assert!(!game.body.swimming);
    game.run_ticks(10);
    game.advance(0.0);
    let pose = game.player_pose();
    assert!(
        pose.swim_amount > 0.9 && !pose.in_water,
        "crawling flat: {pose:?}"
    );
    let eye = game.camera.position.y - game.body.position().y as f32;
    assert!((eye - 0.4).abs() < 0.02, "{eye}");

    game.set_blocks(&[(IVec3::new(0, 65, 0), BlockState::AIR)]);
    game.run_ticks(12);
    game.advance(0.0);
    assert_eq!(game.body.pose(), Pose::Standing);
    assert!(game.player_pose().swim_amount < 0.01);
}

#[test]
fn a_ladder_cancels_a_fall_that_would_hurt() {
    let fall = |ladder: bool| {
        let mut game = flat_game(GameMode::Survival);
        game.camera.pitch = 0.0;
        if ladder {
            let climbable: Vec<_> = (64..=100)
                .map(|y| (IVec3::new(0, y, 0), BlockState::of(blocks::VINE)))
                .collect();
            game.set_blocks(&climbable);
        }
        game.body.set_position(DVec3::new(0.5, 90.0, 0.5));
        game.prev_feet = game.body.position();
        game.run_ticks(60);
        game.stats.health
    };
    assert!(fall(false) < 20.0, "an unbroken 26-block fall hurts");
    assert_eq!(fall(true), 20.0, "vines catch the fall");
}

#[test]
fn a_ladder_placed_in_the_game_can_be_climbed() {
    use crate::inventory::{items, stack::ItemStack};
    let mut game = flat_game(GameMode::Survival);
    // A wall two blocks north, six high.
    let wall: Vec<_> = (64..70)
        .map(|y| (IVec3::new(0, y, -2), BlockState::of(blocks::STONE)))
        .collect();
    game.set_blocks(&wall);
    game.inventory
        .set(0, Some(ItemStack::new(items::LADDER, 8)));
    game.inventory.selected = 0;
    let place = |game: &mut super::Game, pitch: f32| {
        game.camera.pitch = pitch;
        game.advance(0.0);
        game.on_action(Action::Use, true, false, true);
        game.on_action(Action::Use, false, false, true);
        game.run_ticks(5);
    };
    // Looking at the wall at eye height, then at the floor in front of it.
    place(&mut game, -0.15);
    place(&mut game, -0.9);
    for y in [64, 65] {
        let block = game.chunks.world().block(0, y, -1).unwrap();
        assert_eq!(block.id(), blocks::LADDER, "y {y}");
    }
    // Two more above, then climb: walk into the ladder.
    game.set_blocks(&[
        (
            IVec3::new(0, 66, -1),
            game.chunks.world().block(0, 65, -1).unwrap(),
        ),
        (
            IVec3::new(0, 67, -1),
            game.chunks.world().block(0, 65, -1).unwrap(),
        ),
    ]);
    game.camera.pitch = 0.0;
    press(&mut game, Action::Forward);
    game.run_ticks(40);
    assert!(
        game.body.position().y > 66.0,
        "climbed: {:?}",
        game.body.position()
    );
}
