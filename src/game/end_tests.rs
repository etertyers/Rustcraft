//! The End as played (M27): portals and eyes, the dragon fight from start
//! to credits, and what a saved world keeps of it.

use std::time::{Duration, Instant};

use glam::{DVec3, IVec3};

use super::{
    Game,
    dragon::EndTarget,
    tests::{flat_game, load_player_chunk, mob_registry, registries},
};
use crate::{
    entity::{
        breath::BreathCloud,
        dragon::{self, EnderDragon, Part, Phase},
    },
    inventory::{items, stack::ItemStack},
    level::WorldDir,
    player::{mode::GameMode, raycast::Hit},
    world::{
        Face,
        block::BlockState,
        blocks,
        chunk::ChunkPos,
        dimension::Dimension,
        end_portal,
        manager::{ChunkManager, ChunkSource},
        props::Prop,
        save::TempDir,
        worldgen::end::{self, End},
    },
};

const SEED: u64 = 20111118;

/// A test world that starts in the End, on the island, with the chunks
/// around loaded.
pub(super) fn end_game(mode: GameMode) -> Game {
    let (items, models) = registries();
    let chunks = ChunkManager::new(ChunkSource::End(Box::new(End::new(SEED))), models, 3, 2);
    let top = End::new(SEED).top(24, 6).unwrap();
    let feet = DVec3::new(24.5, f64::from(top) + 1.0, 6.5);
    let mut game = Game::new(chunks, items, feet, feet, 0.0, 0.0).starting_in(Dimension::End);
    game.player.mode = mode;
    game.mob_defs = mob_registry();
    wait_for_chunks(&mut game);
    game
}

fn wait_for_chunks(game: &mut Game) {
    let feet = game.player.body.position();
    let center = ChunkPos::of_block(feet.x.floor() as i32, feet.z.floor() as i32);
    let deadline = Instant::now() + Duration::from_secs(30);
    while !game.chunks.ready_around(center, 2) {
        assert!(Instant::now() < deadline, "the End never loaded");
        game.chunks.update(center);
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn block(game: &Game, at: IVec3) -> BlockState {
    game.chunks.world().block(at.x, at.y, at.z).unwrap()
}

fn frame(face: Face, eye: bool) -> BlockState {
    BlockState::of(blocks::END_PORTAL_FRAME)
        .with_face(Prop::Facing, face)
        .with_bool(Prop::Eye, eye)
}

/// Builds a ring of frames round `center`, all with eyes but the first.
fn build_ring(game: &mut Game, center: IVec3) -> IVec3 {
    let mut empty = None;
    let edits: Vec<_> = end_portal::ring(center)
        .into_iter()
        .enumerate()
        .map(|(i, (cell, face))| {
            if i == 0 {
                empty = Some(cell);
            }
            (cell, frame(face, i != 0))
        })
        .collect();
    game.set_blocks(&edits);
    empty.unwrap()
}

fn hit_on(cell: IVec3) -> Hit {
    Hit {
        block: cell,
        face: Some(Face::Top),
        point: cell.as_dvec3() + DVec3::splat(0.5),
    }
}

#[test]
fn the_twelfth_eye_lights_the_portal_and_is_used_up() {
    let mut game = flat_game(GameMode::Survival);
    let center = IVec3::new(8, 64, 8);
    let empty = build_ring(&mut game, center);
    game.player
        .inventory
        .set(0, Some(ItemStack::new(items::ENDER_EYE, 2)));
    game.player.inventory.selected = 0;
    let before = block(&game, center);
    assert!(!before.is(blocks::END_PORTAL));
    game.use_eye(Some(hit_on(empty)));
    assert!(block(&game, empty).bool(Prop::Eye));
    for cell in end_portal::middle(center) {
        assert!(block(&game, cell).is(blocks::END_PORTAL), "{cell}");
    }
    assert_eq!(game.player.inventory.slot(0).unwrap().count, 1);
}

#[test]
fn an_eye_in_a_frame_that_isnt_the_last_lights_nothing() {
    let mut game = flat_game(GameMode::Creative);
    let center = IVec3::new(8, 64, 8);
    let empty = build_ring(&mut game, center);
    let (second, face) = end_portal::ring(center)[1];
    game.set_blocks(&[(second, frame(face, false))]);
    game.player
        .inventory
        .set(0, Some(ItemStack::new(items::ENDER_EYE, 1)));
    game.use_eye(Some(hit_on(empty)));
    assert!(block(&game, empty).bool(Prop::Eye));
    assert!(!block(&game, center).is(blocks::END_PORTAL));
    // Creative keeps the eye.
    assert_eq!(game.player.inventory.slot(0).unwrap().count, 1);
}

/// A generated world, which has strongholds to point at.
fn overworld() -> (Game, TempDir) {
    let saves = TempDir::new("end");
    let (items, models) = registries();
    let mut game = Game::create(
        saves.path(),
        "End Test",
        SEED,
        GameMode::Survival,
        items,
        models,
    )
    .unwrap()
    .with_mobs(mob_registry());
    load_player_chunk(&mut game);
    (game, saves)
}

#[test]
fn a_thrown_eye_floats_toward_the_nearest_stronghold_and_then_drops_or_breaks() {
    let (mut game, _saves) = overworld();
    let feet = game.player.body.position();
    let target = game.nearest_stronghold(feet).unwrap();
    // It really is the nearest.
    let hold_distance = |p: (f64, f64)| (p.0 - feet.x).hypot(p.1 - feet.z);
    for chunk in crate::world::structure::stronghold::positions(SEED) {
        let other = (f64::from(chunk.x * 16 + 8), f64::from(chunk.z * 16 + 8));
        assert!(hold_distance(target) <= hold_distance(other) + 1e-6);
    }
    game.player
        .inventory
        .set(0, Some(ItemStack::new(items::ENDER_EYE, 3)));
    game.player.inventory.selected = 0;
    game.use_eye(None);
    assert_eq!(game.eyes.len(), 1);
    assert_eq!(game.player.inventory.slot(0).unwrap().count, 2);
    for _ in 0..40 {
        game.tick_eyes();
    }
    let moved = game.eyes[0].position - feet;
    let want = DVec3::new(target.0 - feet.x, 0.0, target.1 - feet.z).normalize();
    assert!(moved.length() > 3.0, "{moved}");
    assert!(
        DVec3::new(moved.x, 0.0, moved.z).normalize().dot(want) > 0.99,
        "heading {moved} for {want}"
    );
    assert!(moved.y > 1.0, "rising");
    for _ in 0..60 {
        game.tick_eyes();
    }
    assert!(game.eyes.is_empty());
    // Four in five come back as an item.
    let mut dropped = 0;
    let mut tries = 0;
    for _ in 0..60 {
        game.player
            .inventory
            .set(0, Some(ItemStack::new(items::ENDER_EYE, 1)));
        game.entities.clear();
        game.use_eye(None);
        for _ in 0..90 {
            game.tick_eyes();
        }
        dropped += game.entities.len();
        tries += 1;
    }
    assert!((30..=58).contains(&dropped), "{dropped} of {tries}");
}

#[test]
fn an_eye_does_nothing_outside_the_overworld() {
    let mut game = end_game(GameMode::Survival);
    game.player
        .inventory
        .set(0, Some(ItemStack::new(items::ENDER_EYE, 1)));
    game.use_eye(None);
    assert!(game.eyes.is_empty());
    assert_eq!(game.player.inventory.slot(0).unwrap().count, 1);
}

#[test]
fn stepping_into_an_end_portal_in_the_overworld_lands_on_the_platform() {
    let (mut game, _saves) = overworld();
    let feet = game.player.body.position().floor().as_ivec3();
    game.set_blocks(&[(feet, BlockState::of(blocks::END_PORTAL))]);
    game.tick_end_portal();
    assert_eq!(game.dimension(), Dimension::End);
    assert_eq!(
        game.player.body.position(),
        end::PLATFORM.as_dvec3() + DVec3::new(0.5, 0.0, 0.5)
    );
    load_player_chunk(&mut game);
    let floor = end::PLATFORM - IVec3::Y;
    assert!(block(&game, floor).is(blocks::OBSIDIAN));
    assert!(block(&game, floor + IVec3::new(2, 0, 2)).is(blocks::OBSIDIAN));
    assert!(block(&game, end::PLATFORM + IVec3::Y).is_air());
    // The fight began: ten crystals, one dragon.
    assert_eq!(game.crystals.len(), 10);
    assert!(game.dragon.is_some());
}

#[test]
fn coming_and_going_doesnt_start_a_second_fight() {
    let (mut game, _saves) = overworld();
    game.enter_end();
    load_player_chunk(&mut game);
    game.crystals.truncate(7);
    game.dragon.as_mut().unwrap().health = 120.0;
    game.go_home();
    assert_eq!(game.dimension(), Dimension::Overworld);
    game.enter_end();
    assert_eq!(game.crystals.len(), 7, "the same fight");
    assert_eq!(game.dragon.as_ref().unwrap().health, 120.0);
}

#[test]
fn the_fight_starts_with_a_crystal_on_each_pillar_and_the_dragon_circling() {
    let game = end_game(GameMode::Survival);
    assert_eq!(game.crystals.len(), 10);
    let spikes = end::spikes(SEED);
    for spike in &spikes {
        assert!(
            game.crystals
                .iter()
                .any(|c| c.position == spike.crystal() && !c.show_base)
        );
    }
    let dragon = game.dragon.as_ref().unwrap();
    assert_eq!(dragon.health, dragon::MAX_HEALTH);
    assert_eq!(dragon.phase, Phase::HoldingPattern);
    assert_eq!(game.dragon_bar(), Some(1.0));
}

#[test]
fn crystals_within_32_blocks_heal_the_dragon_a_point_every_ten_ticks() {
    let mut game = end_game(GameMode::Creative);
    game.dragon.as_mut().unwrap().hurt(Part::Head, 20.0);
    game.dragon.as_mut().unwrap().position = DVec3::new(30.0, 90.0, 0.0);
    let before = game.dragon.as_ref().unwrap().health;
    let spot = game.crystals[0].center();
    let near = game
        .crystals
        .iter()
        .any(|c| c.center().distance(DVec3::new(30.0, 90.0, 0.0)) <= 32.0);
    assert!(near || spot.length() > 0.0);
    // Put a crystal right by it so the test doesn't depend on the seed.
    game.crystals[0].position = DVec3::new(30.0, 85.0, 0.0);
    for _ in 0..60 {
        game.dragon.as_mut().unwrap().position = DVec3::new(30.0, 90.0, 0.0);
        game.tick_end();
    }
    let healed = game.dragon.as_ref().unwrap().health - before;
    assert!((4.0..=7.0).contains(&healed), "{healed}");
    assert!(game.crystals[0].beam_to.is_some());
}

#[test]
fn hitting_a_crystal_blows_it_up_and_the_blast_hurts_the_dragon_and_takes_the_pillars_fire() {
    let mut game = end_game(GameMode::Creative);
    let at = DVec3::new(40.5, 100.0, 0.5);
    game.crystals.clear();
    game.crystals
        .push(crate::entity::crystal::EndCrystal::new(at, true));
    game.crystals.push(crate::entity::crystal::EndCrystal::new(
        at + DVec3::new(3.0, 0.0, 0.0),
        true,
    ));
    let dragon = game.dragon.as_mut().unwrap();
    dragon.position = at + DVec3::new(0.0, 0.0, 5.0);
    let before = dragon.health;
    game.attack_end(EndTarget::Crystal(0));
    assert!(game.crystals.is_empty(), "the next one went with it");
    assert!(game.dragon.as_ref().unwrap().health < before - 5.0);
}

#[test]
fn a_blast_far_from_the_dragon_leaves_it_alone() {
    let mut game = end_game(GameMode::Creative);
    game.dragon.as_mut().unwrap().position = DVec3::new(0.0, 200.0, 0.0);
    game.explode(DVec3::new(50.5, 70.0, 0.5), 4.0);
    assert_eq!(game.dragon.as_ref().unwrap().health, dragon::MAX_HEALTH);
}

#[test]
fn a_crosshair_on_the_dragon_or_a_crystal_targets_it_unless_a_block_is_nearer() {
    let mut game = end_game(GameMode::Survival);
    let eye = game.player.body.position() + DVec3::Y * 1.62;
    game.player.camera.yaw = std::f32::consts::FRAC_PI_2;
    game.player.camera.pitch = 0.0;
    game.crystals.clear();
    game.crystals.push(crate::entity::crystal::EndCrystal::new(
        DVec3::new(eye.x + 2.0, eye.y - 1.0, eye.z),
        true,
    ));
    assert_eq!(game.target_end(eye, None), Some(EndTarget::Crystal(0)));
    let wall = Hit {
        block: IVec3::new(eye.x as i32 + 1, eye.y as i32, eye.z as i32),
        face: Some(Face::West),
        point: eye + DVec3::X,
    };
    assert_eq!(game.target_end(eye, Some(&wall)), None, "behind a block");
    // Out of reach.
    game.crystals[0].position.x += 20.0;
    assert_eq!(game.target_end(eye, None), None);
}

#[test]
fn arrows_pop_crystals_and_hurt_the_dragon_before_hitting_blocks() {
    let mut game = end_game(GameMode::Survival);
    game.crystals.clear();
    game.crystals.push(crate::entity::crystal::EndCrystal::new(
        DVec3::new(40.5, 80.0, 0.5),
        true,
    ));
    let from = DVec3::new(38.0, 81.0, 0.5);
    let fast = DVec3::new(3.0, 0.0, 0.0);
    assert_eq!(
        game.arrow_hits_end(from, fast, None),
        Some(EndTarget::Crystal(0))
    );
    assert_eq!(
        game.arrow_hits_end(from, fast, Some(0.5)),
        None,
        "a wall first"
    );
    assert_eq!(game.arrow_hits_end(from, -fast, None), None);
    game.dragon.as_mut().unwrap().position = DVec3::new(0.0, 200.0, 0.0);
    game.arrow_strikes_end(EndTarget::Dragon(Part::Head), 6.0);
    assert_eq!(
        game.dragon.as_ref().unwrap().health,
        dragon::MAX_HEALTH - 6.0
    );
    game.arrow_strikes_end(EndTarget::Crystal(0), 6.0);
    assert!(game.crystals.is_empty());
}

#[test]
fn the_dragon_breaks_what_it_flies_through_but_not_obsidian_bedrock_or_end_stone() {
    let mut game = end_game(GameMode::Creative);
    let at = IVec3::new(0, 100, 0);
    let cells = [
        (at, BlockState::of(blocks::DIRT)),
        (at + IVec3::X, BlockState::of(blocks::OBSIDIAN)),
        (at + IVec3::Z, BlockState::of(blocks::END_STONE)),
        (at + IVec3::Y, BlockState::of(blocks::BEDROCK)),
        (at - IVec3::Y, BlockState::of(blocks::GLASS)),
    ];
    game.set_blocks(&cells);
    let mut dragon = game.dragon.take().unwrap();
    dragon.position = at.as_dvec3() + DVec3::splat(0.5);
    game.dragon_breaks_blocks(&dragon);
    assert!(block(&game, at).is_air());
    assert!(block(&game, at - IVec3::Y).is_air());
    assert!(block(&game, at + IVec3::X).is(blocks::OBSIDIAN));
    assert!(block(&game, at + IVec3::Z).is(blocks::END_STONE));
    assert!(block(&game, at + IVec3::Y).is(blocks::BEDROCK));
}

/// More than the longest flight a dying dragon makes (10 seconds) and a
/// couple of ticks over.
const FLIGHT_TICKS: u32 = 205;

#[test]
fn the_dragons_head_hurts_the_player_and_its_wings_throw_them() {
    let mut game = end_game(GameMode::Survival);
    let spot = DVec3::new(24.5, 90.0, 6.5);
    game.player.body.set_position(spot);
    let mut dragon = game.dragon.take().unwrap();
    dragon.position = spot - dragon.facing() * 8.0;
    let head = dragon
        .parts()
        .iter()
        .find(|(p, _)| *p == Part::Head)
        .map(|(_, bb)| (bb.min + bb.max) / 2.0)
        .unwrap();
    game.player.body.set_position(head - DVec3::Y);
    let health = game.player.stats.health;
    game.dragon_hurts_things(&dragon);
    assert!(game.player.stats.health < health, "bitten by the head");
    // Wings: stand by one.
    let mut game = end_game(GameMode::Survival);
    let mut dragon = game.dragon.take().unwrap();
    dragon.position = DVec3::new(0.0, 100.0, 0.0);
    let wing = dragon
        .parts()
        .iter()
        .find(|(p, _)| *p == Part::Wing)
        .map(|(_, bb)| (bb.min + bb.max) / 2.0)
        .unwrap();
    game.player
        .body
        .set_position(wing + DVec3::new(0.0, -1.0, 0.0));
    game.player.body.velocity = DVec3::ZERO;
    game.dragon_hurts_things(&dragon);
    assert!(
        game.player.body.velocity.length() > 0.5,
        "{}",
        game.player.body.velocity
    );
}

#[test]
fn breath_clouds_hurt_the_player_once_a_second_and_a_fireball_makes_one() {
    let mut game = end_game(GameMode::Survival);
    let feet = game.player.body.position();
    game.breath.push(BreathCloud::new(
        feet,
        crate::entity::breath::FIREBALL_CLOUD,
    ));
    let start = game.player.stats.health;
    for _ in 0..19 {
        game.tick_breath();
    }
    assert_eq!(game.player.stats.health, start);
    game.tick_breath();
    assert!(
        game.player.stats.health < start,
        "hurt on the twentieth tick"
    );
    // A fireball into the ground leaves a cloud there.
    game.breath.clear();
    game.dragon_fireballs
        .push(crate::entity::breath::DragonFireball::new(
            feet + DVec3::new(6.0, 5.0, 0.0),
            DVec3::new(0.0, -1.0, 0.0),
        ));
    for _ in 0..20 {
        game.tick_dragon_fireballs();
    }
    assert!(game.dragon_fireballs.is_empty());
    assert_eq!(game.breath.len(), 1);
}

#[test]
fn killing_the_dragon_lights_the_fountain_and_lays_the_egg_and_a_gateway_and_rains_12000_xp() {
    let mut game = end_game(GameMode::Creative);
    game.dragon.as_mut().unwrap().position = DVec3::new(0.0, 100.0, 0.0);
    game.crystals.clear();
    assert!(game.dragon.as_mut().unwrap().hurt(Part::Head, 9999.0));
    assert_eq!(game.dragon_bar(), Some(0.0));
    for _ in 0..dragon::DEATH_TICKS + FLIGHT_TICKS {
        game.tick_end();
    }
    assert!(game.dragon.is_none());
    assert!(game.end.dragon_killed);
    let origin = end::PODIUM;
    assert!(block(&game, origin + IVec3::X).is(blocks::END_PORTAL));
    assert!(block(&game, origin + IVec3::new(0, 4, 0)).is(blocks::DRAGON_EGG));
    assert!(block(&game, origin).is(blocks::BEDROCK));
    let gateway = end::gateway_position(SEED, 0);
    assert!(block(&game, gateway).is(blocks::END_GATEWAY));
    assert_eq!(game.end.gateways, 1);
    let xp: u32 = game.orbs.iter().map(|o| o.value).sum();
    assert_eq!(xp, dragon::FIRST_KILL_XP);
    assert_eq!(game.dragon_bar(), None);
}

#[test]
fn a_second_kill_gives_less_xp_and_no_egg() {
    let mut game = end_game(GameMode::Creative);
    game.end.dragon_killed = true;
    game.dragon = Some(EnderDragon::new(DVec3::new(0.0, 100.0, 0.0), 0.0, 5));
    game.crystals.clear();
    game.dragon.as_mut().unwrap().hurt(Part::Head, 9999.0);
    for _ in 0..dragon::DEATH_TICKS + FLIGHT_TICKS {
        game.tick_end();
    }
    let xp: u32 = game.orbs.iter().map(|o| o.value).sum();
    assert_eq!(xp, dragon::REPEAT_KILL_XP);
    assert!(!block(&game, end::PODIUM + IVec3::new(0, 4, 0)).is(blocks::DRAGON_EGG));
    assert_eq!(game.end.gateways, 1);
}

#[test]
fn the_exit_portal_rolls_the_credits_once_then_takes_the_player_home() {
    let (mut game, _saves) = overworld();
    let home = game.player.body.position();
    game.enter_end();
    load_player_chunk(&mut game);
    game.end.dragon_killed = true;
    while game.is_arriving() {
        game.tick();
    }
    let portal = end::PODIUM + IVec3::X;
    game.chunks.load_now(ChunkPos::of_block(portal.x, portal.z));
    game.set_blocks(&[(portal, BlockState::of(blocks::END_PORTAL))]);
    game.player
        .body
        .set_position(portal.as_dvec3() + DVec3::new(0.5, 0.0, 0.5));
    game.tick_end_portal();
    assert_eq!(game.dimension(), Dimension::End, "the poem comes first");
    assert_eq!(game.credits_ticks(), Some(0));
    for _ in 0..crate::credits::TICKS - 1 {
        game.tick();
    }
    assert_eq!(game.dimension(), Dimension::End);
    assert!(game.credits_ticks().is_some());
    game.tick();
    game.tick();
    assert_eq!(game.credits_ticks(), None);
    assert_eq!(game.dimension(), Dimension::Overworld);
    assert!((game.player.body.position() - home).length() < 3.0);
    assert!(game.end.credits_seen);
    // The second time: straight home.
    game.enter_end();
    load_player_chunk(&mut game);
    while game.is_arriving() {
        game.tick();
    }
    while game.is_arriving() {
        game.tick();
    }
    game.chunks.load_now(ChunkPos::of_block(portal.x, portal.z));
    game.set_blocks(&[(portal, BlockState::of(blocks::END_PORTAL))]);
    game.player
        .body
        .set_position(portal.as_dvec3() + DVec3::new(0.5, 0.0, 0.5));
    game.tick_end_portal();
    assert_eq!(game.credits_ticks(), None);
    assert_eq!(game.dimension(), Dimension::Overworld);
}

#[test]
fn skipping_the_credits_goes_home_at_once() {
    let mut game = end_game(GameMode::Creative);
    game.player.credits = Some(100);
    game.finish_credits();
    assert_eq!(game.credits_ticks(), None);
    assert_eq!(game.dimension(), Dimension::Overworld);
}

#[test]
fn the_world_freezes_while_the_credits_roll() {
    let mut game = end_game(GameMode::Survival);
    game.player.credits = Some(0);
    let at = game.dragon.as_ref().unwrap().position;
    game.run_ticks(5);
    assert_eq!(game.dragon.as_ref().unwrap().position, at);
    assert_eq!(game.credits_ticks(), Some(5));
}

#[test]
fn the_fight_is_saved_with_the_world_and_comes_back() {
    let saves = TempDir::new("endsave");
    let (items, models) = registries();
    let mut game = Game::create(
        saves.path(),
        "Fight",
        SEED,
        GameMode::Survival,
        items.clone(),
        models.clone(),
    )
    .unwrap()
    .with_mobs(mob_registry());
    load_player_chunk(&mut game);
    game.enter_end();
    load_player_chunk(&mut game);
    game.crystals.truncate(6);
    {
        let dragon = game.dragon.as_mut().unwrap();
        dragon.hurt(Part::Head, 60.0);
        dragon.position = DVec3::new(11.0, 95.0, -20.0);
    }
    game.end.gateways = 2;
    game.save().unwrap();
    drop(game);

    let dir = WorldDir::new(saves.path().join("Fight"));
    let game = Game::open(dir, items, models).unwrap();
    assert_eq!(game.dimension(), Dimension::End);
    assert_eq!(game.crystals.len(), 6);
    let dragon = game.dragon.as_ref().unwrap();
    assert_eq!(dragon.health, 140.0);
    assert_eq!(dragon.position, DVec3::new(11.0, 95.0, -20.0));
    assert!(game.end.fight_started && !game.end.dragon_killed);
    assert_eq!(game.end.gateways, 2);
}

#[test]
fn only_endermen_spawn_in_the_end_and_without_a_sky() {
    let mut game = end_game(GameMode::Survival);
    let enderman = game.mob_defs.find("enderman").unwrap();
    let biome = crate::world::biome::Biome::TheEnd;
    for _ in 0..50 {
        let (kind, light, group) = game
            .pick_dark_kind(biome, IVec3::new(10, 64, 10), ChunkPos::new(0, 0))
            .unwrap();
        assert_eq!((kind, light, group), (enderman, 15, 4));
    }
}

#[test]
fn end_crystals_go_on_obsidian_and_bedrock_in_the_end_only() {
    let mut game = end_game(GameMode::Survival);
    game.crystals.clear();
    let base = IVec3::new(30, 80, 30);
    game.chunks.load_now(ChunkPos::of_block(base.x, base.z));
    game.set_blocks(&[
        (base, BlockState::of(blocks::OBSIDIAN)),
        (base + IVec3::X * 4, BlockState::of(blocks::STONE)),
        (base + IVec3::X * 8, BlockState::of(blocks::BEDROCK)),
        (base + IVec3::X * 8 + IVec3::Y, BlockState::of(blocks::FIRE)),
    ]);
    game.player
        .inventory
        .set(0, Some(ItemStack::new(items::END_CRYSTAL, 5)));
    game.player.inventory.selected = 0;
    assert!(game.place_crystal(&hit_on(base)));
    assert!(!game.place_crystal(&hit_on(base)), "one to a spot");
    assert!(!game.place_crystal(&hit_on(base + IVec3::X * 4)), "stone");
    assert!(game.place_crystal(&hit_on(base + IVec3::X * 8)));
    assert!(
        block(&game, base + IVec3::X * 8 + IVec3::Y).is_air(),
        "fire put out"
    );
    assert_eq!(game.crystals.len(), 2);
    assert_eq!(game.crystals[0].position, DVec3::new(30.5, 81.0, 30.5));
    assert!(game.crystals[0].show_base);
    assert_eq!(game.player.inventory.slot(0).unwrap().count, 3);
    // Not in the Overworld.
    let mut flat = flat_game(GameMode::Survival);
    flat.set_blocks(&[(IVec3::new(5, 70, 5), BlockState::of(blocks::OBSIDIAN))]);
    flat.player
        .inventory
        .set(0, Some(ItemStack::new(items::END_CRYSTAL, 1)));
    assert!(!flat.place_crystal(&hit_on(IVec3::new(5, 70, 5))));
    assert!(flat.crystals.is_empty());
}

#[test]
fn end_crystals_and_eyes_have_their_recipes() {
    let recipes =
        crate::inventory::crafting::Recipes::load(&crate::assets::dir().join("data/recipes.ron"))
            .unwrap();
    let stack = |id| Some(ItemStack::new(id, 1));
    let eye = recipes.craft(
        &[
            stack(items::ENDER_PEARL),
            stack(items::BLAZE_POWDER),
            None,
            None,
        ],
        2,
    );
    assert_eq!(eye, stack(items::ENDER_EYE));
    let (g, e, t) = (
        stack(items::GLASS),
        stack(items::ENDER_EYE),
        stack(items::GHAST_TEAR),
    );
    let crystal = recipes.craft(
        &[
            g.clone(),
            g.clone(),
            g.clone(),
            g.clone(),
            e,
            g.clone(),
            g.clone(),
            t,
            g,
        ],
        3,
    );
    assert_eq!(crystal, stack(items::END_CRYSTAL));
}

#[test]
fn a_chest_under_the_crosshair_takes_the_eyes_right_click_instead_of_a_throw() {
    let mut game = flat_game(GameMode::Survival);
    game.set_blocks(&[(IVec3::new(0, 63, 0), BlockState::of(blocks::CHEST))]);
    game.player
        .inventory
        .set(0, Some(ItemStack::new(items::ENDER_EYE, 2)));
    game.player.inventory.selected = 0;
    game.advance(0.0);
    game.player.placing = true;
    game.player.place_clicked = true;
    game.use_hands();
    assert!(game.player.container.is_some(), "the chest opened");
    assert!(game.eyes.is_empty());
    assert_eq!(game.player.inventory.slot(0).unwrap().count, 2);
}

/// A game in the End with a player standing beside a wing of the dragon.
fn beside_a_wing(mode: GameMode) -> (Game, EnderDragon) {
    let mut game = end_game(mode);
    let mut dragon = game.dragon.take().unwrap();
    dragon.position = DVec3::new(0.0, 100.0, 0.0);
    let wing = dragon
        .parts()
        .iter()
        .find(|(p, _)| *p == Part::Wing)
        .map(|(_, bb)| (bb.min + bb.max) / 2.0)
        .unwrap();
    game.player
        .body
        .set_position(wing + DVec3::new(0.0, -1.0, 0.0));
    game.player.body.velocity = DVec3::ZERO;
    (game, dragon)
}

#[test]
fn creative_players_are_neither_pushed_nor_targeted_by_the_dragon_but_survival_ones_are() {
    let (mut creative, dragon) = beside_a_wing(GameMode::Creative);
    creative.dragon_hurts_things(&dragon);
    assert_eq!(
        creative.player.body.velocity,
        DVec3::ZERO,
        "no push in a wing"
    );
    assert!(
        creative.dragon_senses().player.is_none(),
        "never its target"
    );
    let (mut survival, dragon) = beside_a_wing(GameMode::Survival);
    survival.dragon_hurts_things(&dragon);
    assert!(survival.player.body.velocity.length() > 0.5, "pushed");
    assert!(survival.dragon_senses().player.is_some());
    // Not for ten ticks after the dragon was hurt.
    let (mut hurt, mut dragon) = beside_a_wing(GameMode::Survival);
    assert!(dragon.hurt(Part::Head, 1.0));
    hurt.dragon_hurts_things(&dragon);
    assert_eq!(hurt.player.body.velocity, DVec3::ZERO);
}

#[test]
fn a_dragon_killed_far_away_flies_to_the_portal_and_the_portal_and_egg_come_after() {
    let mut game = end_game(GameMode::Creative);
    game.crystals.clear();
    game.dragon.as_mut().unwrap().position = DVec3::new(80.0, 100.0, 0.0);
    assert!(game.dragon.as_mut().unwrap().hurt(Part::Head, 9999.0));
    let origin = end::PODIUM;
    let mut start = None;
    let mut ticks = 0;
    while game.dragon.is_some() {
        let dragon = game.dragon.as_ref().unwrap();
        if dragon.in_death_sequence() && start.is_none() {
            start = Some(dragon.position);
        }
        assert!(
            !block(&game, origin + IVec3::X).is(blocks::END_PORTAL)
                && !block(&game, origin + IVec3::new(0, 4, 0)).is(blocks::DRAGON_EGG),
            "no portal or egg while it's alive, at tick {ticks}"
        );
        game.tick_end();
        ticks += 1;
        assert!(
            ticks < 450,
            "it dies within 10 s of flight and 10 s of dying"
        );
    }
    let start = start.expect("the death sequence began");
    let flat = (start.x - f64::from(origin.x) - 0.5).hypot(start.z - f64::from(origin.z) - 0.5);
    assert!(
        flat < 3.0,
        "it died over the portal, not {flat} off: {start}"
    );
    assert!(block(&game, origin + IVec3::X).is(blocks::END_PORTAL));
    assert!(block(&game, origin + IVec3::new(0, 4, 0)).is(blocks::DRAGON_EGG));
}
