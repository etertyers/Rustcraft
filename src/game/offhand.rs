//! Tests of the two hands: the off-hand slot and the F swap, using items
//! from the off-hand, shields, and flint and steel.

use glam::{DVec3, IVec3};
use winit::{event::MouseButton, keyboard::KeyCode};

use super::{
    Game,
    combat::{Outcome, Strike},
    tests::{flat_game, mob_registry},
};
use crate::{
    entity::mob::Mob,
    inventory::{
        items,
        player::{Hand, OFFHAND},
        stack::ItemStack,
    },
    player::{mode::GameMode, stats::DamageSource},
    world::{block::BlockState, blocks},
};

fn hold(game: &mut Game, main: Option<ItemStack>, off: Option<ItemStack>) {
    game.inventory.set(0, main);
    game.inventory.set(OFFHAND, off);
    game.inventory.selected = 0;
}

fn right_click(game: &mut Game, held: bool) {
    game.on_mouse_button(MouseButton::Right, held);
}

fn zombie_at(game: &mut Game, feet: DVec3) {
    game.mob_defs = mob_registry();
    let kind = game.mob_defs.find("zombie").unwrap();
    let zombie = Mob::new(kind, game.mob_defs.get(kind), feet, 0.0);
    game.mobs.push(zombie);
}

#[test]
fn f_swaps_the_hands_and_ends_whatever_was_in_use() {
    let mut game = flat_game(GameMode::Survival);
    hold(
        &mut game,
        Some(ItemStack::new(items::IRON_SWORD, 1)),
        Some(ItemStack::new(items::SHIELD, 1)),
    );
    game.on_key(KeyCode::KeyF, true, false, true);
    assert_eq!(
        game.inventory.hand_stack(Hand::Main).unwrap().id,
        items::SHIELD
    );
    assert_eq!(
        game.inventory.hand_stack(Hand::Off).unwrap().id,
        items::IRON_SWORD
    );
    // Swapping drops the shield you were holding up.
    game.camera.pitch = 0.0;
    right_click(&mut game, true);
    game.run_ticks(8);
    assert!(game.is_blocking());
    game.on_key(KeyCode::KeyF, true, false, true);
    assert!(!game.is_blocking() && game.shield_hand().is_none());
    // Held down, the key swaps once.
    let before = game.inventory.hand_stack(Hand::Main).copied();
    game.on_key(KeyCode::KeyF, true, true, true);
    assert_eq!(game.inventory.hand_stack(Hand::Main).copied(), before);
}

#[test]
fn right_click_places_from_the_off_hand_when_the_main_hand_has_nothing_to_do() {
    let mut game = flat_game(GameMode::Survival);
    hold(
        &mut game,
        Some(ItemStack::new(items::IRON_SWORD, 1)),
        Some(ItemStack::new(items::STONE, 3)),
    );
    game.camera.pitch = -0.6;
    game.advance(0.0);
    right_click(&mut game, true);
    game.run_ticks(2);
    right_click(&mut game, false);
    assert_eq!(game.inventory.slot(OFFHAND).unwrap().count, 2);
    assert_eq!(game.inventory.slot(0).unwrap().id, items::IRON_SWORD);
    // A main hand that can place goes first.
    hold(
        &mut game,
        Some(ItemStack::new(items::DIRT, 3)),
        Some(ItemStack::new(items::STONE, 3)),
    );
    game.run_ticks(10);
    right_click(&mut game, true);
    game.run_ticks(2);
    right_click(&mut game, false);
    assert_eq!(game.inventory.slot(0).unwrap().count, 2);
    assert_eq!(game.inventory.slot(OFFHAND).unwrap().count, 3);
}

#[test]
fn food_in_the_off_hand_is_eaten_when_the_main_hand_cant() {
    let mut game = flat_game(GameMode::Survival);
    game.stats.food = 10;
    hold(
        &mut game,
        Some(ItemStack::new(items::IRON_SWORD, 1)),
        Some(ItemStack::new(items::COOKED_PORKCHOP, 2)),
    );
    right_click(&mut game, true);
    game.run_ticks(40);
    assert_eq!(game.inventory.slot(OFFHAND).unwrap().count, 1);
    assert!(game.stats.food > 10);
}

fn blocking_game() -> Game {
    let mut game = flat_game(GameMode::Survival);
    hold(
        &mut game,
        Some(ItemStack::new(items::IRON_SWORD, 1)),
        Some(ItemStack::new(items::SHIELD, 1)),
    );
    game.camera.pitch = 0.0;
    right_click(&mut game, true);
    game.run_ticks(2);
    game
}

fn strike(from: DVec3, amount: f32) -> Strike {
    Strike {
        amount,
        source: DamageSource::Mob,
        from: Some(from),
        ranged: false,
        axe: false,
    }
}

#[test]
fn a_shield_takes_five_ticks_to_raise() {
    let mut game = blocking_game();
    assert!(!game.is_blocking(), "still going up");
    // Facing north, a blow from the north (in front) hurts while it's rising.
    let front = DVec3::new(0.5, 64.0, -2.5);
    assert_eq!(
        game.hurt_player(strike(front, 3.0)),
        Outcome::Hurt { fresh: true }
    );
    game.run_ticks(25);
    assert!(game.is_blocking());
    assert_eq!(game.shield_hand(), Some(Hand::Off));
    // And it goes down when right-click is let go.
    right_click(&mut game, false);
    game.run_ticks(1);
    assert!(!game.is_blocking());
}

#[test]
fn a_raised_shield_blocks_blows_from_the_front_but_not_the_back() {
    let mut game = blocking_game();
    game.run_ticks(6);
    assert!(game.is_blocking());
    let health = game.stats.health;
    let front = DVec3::new(0.5, 64.0, -2.5);
    let back = DVec3::new(0.5, 64.0, 3.5);
    assert_eq!(game.hurt_player(strike(front, 3.0)), Outcome::Blocked);
    assert_eq!(game.stats.health, health);
    // A blow of 3 wears the shield by 1 + 3.
    assert_eq!(game.inventory.slot(OFFHAND).unwrap().damage, 4);
    // A small blow doesn't wear it.
    game.hurt_player(strike(front, 2.0));
    assert_eq!(game.inventory.slot(OFFHAND).unwrap().damage, 4);
    // From behind it hurts.
    assert_eq!(
        game.hurt_player(strike(back, 3.0)),
        Outcome::Hurt { fresh: true }
    );
    assert_eq!(game.stats.health, health - 3.0);
    // Fire and falls go through a shield.
    game.stats.health = 20.0;
    let burn = Strike {
        source: DamageSource::Fire,
        ..strike(front, 1.0)
    };
    game.run_ticks(25);
    assert!(matches!(game.hurt_player(burn), Outcome::Hurt { .. }));
}

#[test]
fn an_axe_blow_puts_the_shield_out_of_use_for_five_seconds() {
    let mut game = blocking_game();
    game.run_ticks(6);
    let axe = Strike {
        axe: true,
        ..strike(DVec3::new(0.5, 64.0, -2.5), 7.0)
    };
    assert_eq!(game.hurt_player(axe), Outcome::Blocked);
    assert!(!game.is_blocking());
    assert_eq!(game.shield_cooldown(), 100);
    // Holding right-click doesn't raise it again until the time is up.
    game.run_ticks(60);
    assert!(!game.is_blocking());
    game.run_ticks(45);
    assert!(game.is_blocking());
}

#[test]
fn you_can_attack_with_the_sword_while_the_shield_stays_up() {
    let mut game = blocking_game();
    zombie_at(&mut game, DVec3::new(0.5, 64.0, -1.5));
    game.run_ticks(8);
    assert!(game.is_blocking());
    game.attack_ticker = 100;
    game.mobs[0].body.set_position(DVec3::new(0.5, 64.0, -1.5));
    game.on_mouse_button(MouseButton::Left, true);
    game.run_ticks(1);
    game.on_mouse_button(MouseButton::Left, false);
    assert!(game.mobs[0].stats.health < 20.0, "the sword hit");
    assert!(game.is_blocking(), "and the shield is still up");
}

#[test]
fn you_cant_mine_with_the_shield_up() {
    let mut game = blocking_game();
    game.camera.pitch = -0.8;
    game.advance(0.0);
    let target = game.target.unwrap().block;
    game.run_ticks(8);
    game.on_mouse_button(MouseButton::Left, true);
    game.run_ticks(40);
    assert!(game.mining_progress().is_none());
    assert!(
        !game
            .chunks
            .world()
            .block(target.x, target.y, target.z)
            .unwrap()
            .is_air(),
        "the block is still there"
    );
}

#[test]
fn a_zombie_swinging_at_a_shield_is_pushed_back_and_does_no_harm() {
    let mut game = blocking_game();
    game.run_ticks(6);
    zombie_at(&mut game, DVec3::new(0.5, 64.0, -1.0));
    game.time = 18000;
    let health = game.stats.health;
    let start = game.mobs[0].feet();
    for _ in 0..40 {
        game.tick();
    }
    assert_eq!(game.stats.health, health, "the shield took every hit");
    assert!(game.inventory.slot(OFFHAND).unwrap().damage >= 4);
    assert!(game.mobs[0].feet().z != start.z);
}

#[test]
fn a_broken_shield_is_gone() {
    let mut game = blocking_game();
    game.run_ticks(6);
    game.inventory.set(
        OFFHAND,
        Some(ItemStack {
            damage: 335,
            ..ItemStack::new(items::SHIELD, 1)
        }),
    );
    game.hurt_player(strike(DVec3::new(0.5, 64.0, -2.5), 3.0));
    assert!(game.inventory.slot(OFFHAND).is_none());
    assert!(!game.is_blocking());
}

#[test]
fn raising_a_shield_slows_you_to_a_fifth() {
    let mut game = blocking_game();
    game.run_ticks(6);
    game.on_key(KeyCode::KeyW, true, false, true);
    game.run_ticks(40);
    let blocking = game.body.velocity.x.hypot(game.body.velocity.z);
    // Walking settles at 0.118 a tick after friction; a fifth of the push is 0.024.
    assert!((blocking - 0.0236).abs() < 0.004, "{blocking}");
}

#[test]
fn flint_and_steel_lights_a_fire_beside_the_block_and_wears_out() {
    let mut game = flat_game(GameMode::Survival);
    hold(
        &mut game,
        Some(ItemStack::new(items::FLINT_AND_STEEL, 1)),
        None,
    );
    game.camera.pitch = -0.6;
    game.advance(0.0);
    let target = game.target.unwrap();
    right_click(&mut game, true);
    game.run_ticks(1);
    right_click(&mut game, false);
    let at = target.adjacent().unwrap();
    assert_eq!(
        game.chunks.world().block(at.x, at.y, at.z).unwrap().id(),
        blocks::FIRE
    );
    assert_eq!(game.inventory.slot(0).unwrap().damage, 1);

    // Not into a cell that's already taken, and not in mid-air.
    game.run_ticks(10);
    right_click(&mut game, true);
    game.run_ticks(1);
    right_click(&mut game, false);
    assert_eq!(game.inventory.slot(0).unwrap().damage, 1);
    game.edit(&[
        (IVec3::new(5, 71, 5), BlockState::of(blocks::STONE)),
        (IVec3::new(5, 69, 7), BlockState::of(blocks::STONE)),
    ]);
    game.body.set_position(DVec3::new(5.5, 70.0, 7.5));
    game.prev_feet = game.body.position();
    game.camera.pitch = 0.0;
    game.camera.yaw = 0.0;
    game.advance(0.0);
    let wall = game.target.expect("looking at the stone's south face");
    assert_eq!(wall.block, IVec3::new(5, 71, 5));
    game.run_ticks(10);
    right_click(&mut game, true);
    game.run_ticks(1);
    right_click(&mut game, false);
    let beside = wall.adjacent().unwrap();
    assert!(
        game.chunks
            .world()
            .block(beside.x, beside.y, beside.z)
            .unwrap()
            .is_air(),
        "fire needs something under it or beside it that burns"
    );
}

#[test]
fn fire_spreads_to_wood_beside_it() {
    let mut game = flat_game(GameMode::Survival);
    game.edit(&[(IVec3::new(2, 64, 0), BlockState::of(blocks::OAK_PLANKS))]);
    hold(
        &mut game,
        Some(ItemStack::new(items::FLINT_AND_STEEL, 1)),
        None,
    );
    // Light the grass beside the planks.
    game.camera.pitch = -0.5;
    game.camera.yaw = std::f32::consts::FRAC_PI_2;
    game.advance(0.0);
    right_click(&mut game, true);
    game.run_ticks(1);
    right_click(&mut game, false);
    let lit = game.chunks.world().block(1, 64, 0).unwrap();
    assert_eq!(lit.id(), blocks::FIRE, "lit beside the planks");
    game.run_ticks(2400);
    let burnt = game
        .chunks
        .world()
        .block(2, 64, 0)
        .map(|b| b.id() != blocks::OAK_PLANKS)
        .unwrap_or(false);
    assert!(burnt, "the planks burned away");
}

#[test]
fn the_survival_screen_can_put_items_in_the_off_hand_slot() {
    use crate::inventory::slots::{Click, SlotRef};
    let mut game = flat_game(GameMode::Survival);
    game.cursor = Some(ItemStack::new(items::SHIELD, 1));
    game.click_slot(SlotRef::Inventory(OFFHAND), Click::Left);
    assert!(game.cursor.is_none());
    assert_eq!(
        game.inventory.hand_stack(Hand::Off).unwrap().id,
        items::SHIELD
    );
    // Picking it back up empties the slot.
    game.click_slot(SlotRef::Inventory(OFFHAND), Click::Left);
    assert!(game.inventory.hand_stack(Hand::Off).is_none());
    assert_eq!(game.cursor.unwrap().id, items::SHIELD);
}
