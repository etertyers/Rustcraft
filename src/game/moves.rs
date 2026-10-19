//! Tests of the modern movement rules through the whole game: hunger
//! and sprinting, swimming, crawling, and landing on ladders.

use glam::{DVec3, IVec3};
use winit::keyboard::KeyCode;

use super::tests::flat_game;
use crate::{
    player::{mode::GameMode, physics::Pose},
    world::{block::BlockState, blocks},
};

fn press(game: &mut super::Game, key: KeyCode) {
    game.on_key(key, true, false, true);
}

#[test]
fn hunger_of_six_or_less_cant_sprint() {
    for (food, sprints) in [(6, false), (7, true)] {
        let mut game = flat_game(GameMode::Survival);
        game.stats.food = food;
        game.camera.pitch = 0.0;
        press(&mut game, KeyCode::KeyW);
        press(&mut game, KeyCode::ControlLeft);
        game.run_ticks(10);
        assert_eq!(game.body.sprinting, sprints, "food {food}");
    }
    // Creative players eat nothing and sprint regardless.
    let mut game = flat_game(GameMode::Creative);
    game.stats.food = 0;
    press(&mut game, KeyCode::KeyW);
    press(&mut game, KeyCode::ControlLeft);
    game.run_ticks(10);
    assert!(game.body.sprinting);
}

#[test]
fn running_out_of_hunger_mid_sprint_stops_it() {
    let mut game = flat_game(GameMode::Survival);
    game.camera.pitch = 0.0;
    press(&mut game, KeyCode::KeyW);
    press(&mut game, KeyCode::ControlLeft);
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
    press(&mut game, KeyCode::KeyW);
    press(&mut game, KeyCode::ControlLeft);
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
    assert!(fall(false) < 20, "an unbroken 26-block fall hurts");
    assert_eq!(fall(true), 20, "vines catch the fall");
}
