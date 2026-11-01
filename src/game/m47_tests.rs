//! M47: jukeboxes and discs.

use glam::{DVec3, IVec3};

use super::{jukebox::RecordEvent, tests::flat_game};
use crate::{
    inventory::{discs, items, stack::ItemStack},
    player::mode::GameMode,
    world::{block::BlockState, block_entity::BlockEntity, blocks, props::Prop},
};

const AT: IVec3 = IVec3::new(2, 64, 0);

fn disc(name: &str) -> ItemStack {
    ItemStack::new(items::by_name(&format!("music_disc_{name}")).unwrap(), 1)
}

fn game_with_jukebox() -> super::Game {
    let mut game = flat_game(GameMode::Creative);
    game.set_blocks(&[(AT, BlockState::of(blocks::JUKEBOX))]);
    game.run_ticks(1);
    game.record_events.clear();
    game
}

fn jukebox(game: &super::Game) -> BlockState {
    game.chunks.world().block(AT.x, AT.y, AT.z).unwrap()
}

fn hold(game: &mut super::Game, stack: ItemStack) {
    let slot = game.player.inventory.selected;
    game.player.inventory.set(slot, Some(stack));
}

fn started(game: &super::Game, name: &str) -> bool {
    game.record_events.iter().any(|e| {
        *e == RecordEvent {
            dimension: game.dimension,
            at: AT,
            song: Some(name.to_owned()),
        }
    })
}

fn stopped(game: &super::Game) -> bool {
    game.record_events
        .iter()
        .any(|e| e.at == AT && e.song.is_none())
}

#[test]
fn a_disc_goes_in_plays_its_song_and_comes_out_on_the_next_right_click() {
    let mut game = game_with_jukebox();
    hold(&mut game, disc("cat"));
    assert!(game.use_jukebox(AT));
    assert!(started(&game, "cat"), "music_disc.cat starts");
    assert!(jukebox(&game).bool(Prop::HasRecord) && jukebox(&game).bool(Prop::Powered));
    assert_eq!(
        game.player.inventory.selected_stack().map(|s| s.id),
        Some(items::by_name("music_disc_cat").unwrap()),
        "a creative player keeps the disc"
    );
    assert_eq!(
        game.player.title.actionbar, "Now Playing: C418 - cat",
        "shown above the hotbar"
    );
    assert!(game.player.title.actionbar_rainbow);
    // Right-click again: the disc pops out above the box and the song stops.
    game.record_events.clear();
    game.entities.clear();
    assert!(game.use_jukebox(AT));
    assert!(stopped(&game));
    assert!(!jukebox(&game).bool(Prop::HasRecord) && !jukebox(&game).bool(Prop::Powered));
    assert_eq!(game.entities.len(), 1);
    assert_eq!(
        game.entities[0].stack.id,
        items::by_name("music_disc_cat").unwrap()
    );
}

#[test]
fn a_survival_player_hands_the_disc_over() {
    let mut game = game_with_jukebox();
    game.set_game_mode(GameMode::Survival);
    hold(&mut game, disc("13"));
    assert!(game.use_jukebox(AT));
    assert!(game.player.inventory.selected_stack().is_none());
    assert!(started(&game, "13"));
}

#[test]
fn the_redstone_is_15_while_the_song_plays_and_0_after_it_ends() {
    let mut game = game_with_jukebox();
    // A lamp beside it, lit by the jukebox's signal through dust.
    hold(&mut game, disc("11"));
    assert!(game.use_jukebox(AT));
    let power = |game: &super::Game| jukebox(game).bool(Prop::Powered);
    assert!(power(&game), "15 while it plays");
    // Disc 11 is 71 s: one tick short of it, still playing; at it, over.
    let ticks = discs::of_item("music_disc_11").unwrap().ticks();
    game.record_events.clear();
    game.run_ticks(ticks as u32 - 2);
    assert!(power(&game) && game.record_events.is_empty());
    game.run_ticks(3);
    assert!(!power(&game), "0 after the song ends");
    assert!(stopped(&game));
    // The disc stays in, and nothing plays.
    assert!(jukebox(&game).bool(Prop::HasRecord));
    match game.chunks.block_entity(AT) {
        Some(BlockEntity::Jukebox(j)) => assert!(j.slots[0].is_some()),
        other => panic!("{other:?}"),
    }
}

#[test]
fn breaking_the_jukebox_drops_the_box_and_the_disc_and_stops_the_song() {
    let mut game = game_with_jukebox();
    hold(&mut game, disc("blocks"));
    game.use_jukebox(AT);
    game.record_events.clear();
    game.entities.clear();
    game.break_block(AT);
    game.run_ticks(2);
    assert!(
        game.entities
            .iter()
            .any(|e| e.stack.id == items::by_name("music_disc_blocks").unwrap()),
        "the disc drops"
    );
    assert!(stopped(&game), "and the music stops");
}

#[test]
fn a_hopper_takes_the_disc_and_a_dropper_style_insert_starts_it() {
    let mut game = game_with_jukebox();
    assert!(
        game.insert_into(AT, crate::world::Face::Top, disc("far"))
            .is_none()
    );
    assert!(started(&game, "far"));
    // Not a second one while it holds a disc, and not something that isn't one.
    assert!(
        game.insert_into(AT, crate::world::Face::Top, disc("far"))
            .is_some()
    );
    game.record_events.clear();
    let (slot, _) = game.take_one_from(AT).expect("the disc");
    game.remove_one(AT, slot);
    assert!(stopped(&game));
    assert!(!jukebox(&game).bool(Prop::HasRecord));
    let stick = ItemStack::new(items::STICK, 1);
    assert!(
        game.insert_into(AT, crate::world::Face::Top, stick)
            .is_some()
    );
}

#[test]
fn the_disc_survives_a_save_and_the_song_does_not() {
    let mut game = game_with_jukebox();
    hold(&mut game, disc("mall"));
    game.use_jukebox(AT);
    let entity = game.chunks.block_entity(AT).cloned().unwrap();
    let bytes = bincode::serde::encode_to_vec(&entity, bincode::config::standard()).unwrap();
    let (back, _): (BlockEntity, usize) =
        bincode::serde::decode_from_slice(&bytes, bincode::config::standard()).unwrap();
    assert_eq!(back, entity);
    // Loaded again, the chunk lets go of the redstone but keeps the disc.
    game.jukeboxes.clear();
    game.settle_jukeboxes(&[crate::world::chunk::ChunkPos::of_block(AT.x, AT.z)]);
    assert!(!jukebox(&game).bool(Prop::Powered));
    assert!(jukebox(&game).bool(Prop::HasRecord));
}

#[test]
fn a_creeper_killed_by_a_skeleton_arrow_drops_one_of_the_twelve_discs() {
    let mut game = flat_game(GameMode::Creative).with_mobs(super::tests::mob_registry());
    let kind = game.mob_defs.find("creeper").expect("creepers exist");
    let mut creeper = crate::entity::mob::Mob::new(
        kind,
        game.mob_defs.get(kind),
        DVec3::new(4.5, 64.0, 4.5),
        0.0,
    );
    creeper.shot_by_skeleton = true;
    game.entities.clear();
    game.on_mob_death(&creeper);
    let dropped: Vec<_> = game
        .entities
        .iter()
        .filter_map(|e| game.items.item(e.stack.id))
        .filter_map(|item| discs::of_item(&item.name))
        .collect();
    assert_eq!(dropped.len(), 1);
    assert!(discs::CREEPER_DROPS.contains(&dropped[0].name));
    // Killed any other way, no disc.
    creeper.shot_by_skeleton = false;
    game.entities.clear();
    game.on_mob_death(&creeper);
    assert!(
        game.entities
            .iter()
            .filter_map(|e| game.items.item(e.stack.id))
            .all(|item| discs::of_item(&item.name).is_none())
    );
}
