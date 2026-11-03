//! Command blocks: modes, chains, conditions, power, the editor's messages
//! and who may use them.

use glam::IVec3;

use super::tests::{run, said, world};
use crate::{
    game::{
        Game,
        command_blocks::{Edit, Mode},
    },
    player::mode::GameMode,
    world::{Face, block::BlockState, blocks, props::Prop},
};

/// Kenzo, an operator in creative, with a flat world around him.
fn operator() -> Game {
    let mut game = world();
    game.player.mode = GameMode::Creative;
    game.cheats = true;
    game
}

fn put(
    game: &mut Game,
    at: IVec3,
    mode: Mode,
    facing: Face,
    command: &str,
    auto: bool,
    conditional: bool,
) {
    let state = BlockState::of(mode.block())
        .with_face(Prop::FacingAll, facing)
        .with_bool(Prop::Conditional, conditional);
    game.set_blocks(&[(at, state)]);
    game.player.command_block_edit = Some(Edit {
        at: at.to_array(),
        command: command.to_owned(),
        mode,
        conditional,
        auto,
        track_output: true,
        last_output: String::new(),
    });
    let edit = game.player.command_block_edit.take();
    game.finish_command_block_edit(edit);
}

fn score(game: &Game, holder: &str) -> Option<i32> {
    game.board.score("n", holder)
}

fn setup(game: &mut Game) {
    run(game, "scoreboard objectives add n dummy");
}

#[test]
fn a_repeating_block_runs_every_tick_while_powered_and_a_chain_follows_in_order() {
    let mut game = operator();
    setup(&mut game);
    let (a, b, c) = (
        IVec3::new(5, 70, 5),
        IVec3::new(6, 70, 5),
        IVec3::new(7, 70, 5),
    );
    put(
        &mut game,
        a,
        Mode::Repeat,
        Face::East,
        "scoreboard players add #a n 1",
        false,
        false,
    );
    put(
        &mut game,
        b,
        Mode::Chain,
        Face::East,
        "scoreboard players operation #b n = #a n",
        true,
        false,
    );
    put(
        &mut game,
        c,
        Mode::Chain,
        Face::East,
        "scoreboard players add #c n 10",
        true,
        false,
    );
    game.run_ticks(5);
    assert_eq!(score(&game, "#a"), None, "unpowered: nothing runs");
    // Power the repeater from above.
    game.set_blocks(&[(IVec3::new(5, 71, 5), BlockState::of(blocks::REDSTONE_BLOCK))]);
    game.run_ticks(10);
    let ran = score(&game, "#a").unwrap();
    assert!((8..=10).contains(&ran), "about once a tick: {ran}");
    // The chain ran in the same tick, in order: b copied a, c ran as often.
    assert_eq!(
        score(&game, "#b"),
        Some(ran),
        "b saw a's value of the same tick"
    );
    assert_eq!(score(&game, "#c"), Some(ran * 10));
    // Cutting the power stops it.
    game.set_blocks(&[(IVec3::new(5, 71, 5), BlockState::AIR)]);
    game.run_ticks(3);
    let stopped = score(&game, "#a").unwrap();
    game.run_ticks(5);
    assert_eq!(score(&game, "#a"), Some(stopped));
}

#[test]
fn an_impulse_block_runs_once_per_power_up_and_always_active_runs_once() {
    let mut game = operator();
    setup(&mut game);
    let at = IVec3::new(5, 70, 8);
    put(
        &mut game,
        at,
        Mode::Impulse,
        Face::East,
        "scoreboard players add #i n 1",
        false,
        false,
    );
    game.run_ticks(3);
    assert_eq!(score(&game, "#i"), None);
    let power = IVec3::new(5, 71, 8);
    game.set_blocks(&[(power, BlockState::of(blocks::REDSTONE_BLOCK))]);
    game.run_ticks(10);
    assert_eq!(score(&game, "#i"), Some(1), "once while it stays powered");
    game.set_blocks(&[(power, BlockState::AIR)]);
    game.run_ticks(3);
    game.set_blocks(&[(power, BlockState::of(blocks::REDSTONE_BLOCK))]);
    game.run_ticks(5);
    assert_eq!(score(&game, "#i"), Some(2), "again on the next pulse");
    // Always Active: once, with no redstone.
    let auto = IVec3::new(9, 70, 8);
    put(
        &mut game,
        auto,
        Mode::Impulse,
        Face::East,
        "scoreboard players add #auto n 1",
        true,
        false,
    );
    game.run_ticks(10);
    assert_eq!(score(&game, "#auto"), Some(1));
    // A repeating block always active keeps going.
    let rep = IVec3::new(11, 70, 8);
    put(
        &mut game,
        rep,
        Mode::Repeat,
        Face::East,
        "scoreboard players add #rep n 1",
        true,
        false,
    );
    game.run_ticks(10);
    assert!(score(&game, "#rep").unwrap() >= 9);
}

#[test]
fn conditional_blocks_run_only_if_the_one_behind_succeeded() {
    let mut game = operator();
    setup(&mut game);
    run(&mut game, "scoreboard players set #ok n 1");
    // good → conditional (runs) ... bad → conditional (does not).
    let (a, b) = (IVec3::new(5, 70, 12), IVec3::new(6, 70, 12));
    put(
        &mut game,
        a,
        Mode::Impulse,
        Face::East,
        "scoreboard players get #ok n",
        true,
        false,
    );
    put(
        &mut game,
        b,
        Mode::Chain,
        Face::East,
        "scoreboard players add #cond n 1",
        true,
        true,
    );
    game.run_ticks(3);
    assert_eq!(score(&game, "#cond"), Some(1), "the block behind succeeded");
    let (c, d) = (IVec3::new(5, 70, 15), IVec3::new(6, 70, 15));
    put(
        &mut game,
        c,
        Mode::Impulse,
        Face::East,
        "scoreboard players get #nobody n",
        true,
        false,
    );
    put(
        &mut game,
        d,
        Mode::Chain,
        Face::East,
        "scoreboard players add #skipped n 1",
        true,
        true,
    );
    game.run_ticks(3);
    assert_eq!(score(&game, "#skipped"), None, "the block behind failed");
    // An unconditional chain block after it runs regardless.
    put(
        &mut game,
        IVec3::new(7, 70, 15),
        Mode::Chain,
        Face::East,
        "scoreboard players add #after n 1",
        true,
        false,
    );
    game.set_blocks(&[(
        c,
        BlockState::of(Mode::Impulse.block()).with_face(Prop::FacingAll, Face::East),
    )]);
    // Re-arm the impulse with auto again.
    let edit = Edit {
        at: c.to_array(),
        command: "scoreboard players get #nobody n".into(),
        mode: Mode::Impulse,
        conditional: false,
        auto: false,
        track_output: true,
        last_output: String::new(),
    };
    game.finish_command_block_edit(Some(edit));
    let edit = Edit {
        at: c.to_array(),
        command: "scoreboard players get #nobody n".into(),
        mode: Mode::Impulse,
        conditional: false,
        auto: true,
        track_output: true,
        last_output: String::new(),
    };
    game.finish_command_block_edit(Some(edit));
    game.run_ticks(3);
    assert_eq!(score(&game, "#after"), Some(1));
    assert_eq!(score(&game, "#skipped"), None);
}

#[test]
fn a_chain_block_that_needs_redstone_and_has_none_ends_the_chain() {
    let mut game = operator();
    setup(&mut game);
    let a = IVec3::new(5, 70, 20);
    put(
        &mut game,
        a,
        Mode::Impulse,
        Face::East,
        "scoreboard players add #1 n 1",
        true,
        false,
    );
    put(
        &mut game,
        IVec3::new(6, 70, 20),
        Mode::Chain,
        Face::East,
        "scoreboard players add #2 n 1",
        false,
        false,
    );
    put(
        &mut game,
        IVec3::new(7, 70, 20),
        Mode::Chain,
        Face::East,
        "scoreboard players add #3 n 1",
        true,
        false,
    );
    game.run_ticks(3);
    assert_eq!(score(&game, "#1"), Some(1));
    assert_eq!(score(&game, "#2"), None);
    assert_eq!(
        score(&game, "#3"),
        None,
        "the chain stopped at the unpowered block"
    );
}

#[test]
fn output_goes_to_ops_the_editor_remembers_it_and_the_rules_switch_things_off() {
    let mut game = operator();
    setup(&mut game);
    let at = IVec3::new(5, 70, 24);
    put(
        &mut game,
        at,
        Mode::Repeat,
        Face::East,
        "say from the block",
        true,
        false,
    );
    game.run_ticks(2);
    assert!(game.replies_out.iter().any(
        |r| matches!(r, crate::game::commands::Reply::Everyone(t) if t == "[@] from the block")
    ));
    game.replies_out.clear();
    // A command with feedback: the op hears it, in gray, and the editor keeps it.
    put(
        &mut game,
        at,
        Mode::Repeat,
        Face::East,
        "scoreboard players set #x n 4",
        true,
        false,
    );
    game.chat_to.clear();
    game.run_ticks(1);
    assert!(
        game.chat_to
            .iter()
            .any(|(_, t)| t == "§7§o[@: Set [n] for #x to 4]"),
        "{:?}",
        game.chat_to
    );
    game.open_command_block_for_test(at);
    let edit = game.player.command_block_edit.clone().unwrap();
    assert_eq!(edit.last_output, "Set [n] for #x to 4");
    assert_eq!(edit.command, "scoreboard players set #x n 4");
    game.player.command_block_edit = None;
    // commandBlockOutput off, track output off: quiet.
    run(&mut game, "gamerule commandBlockOutput false");
    game.chat_to.clear();
    game.run_ticks(2);
    assert!(game.chat_to.is_empty());
    // commandBlocksEnabled off: nothing runs.
    run(&mut game, "scoreboard players set #count n 0");
    put(
        &mut game,
        at,
        Mode::Repeat,
        Face::East,
        "scoreboard players add #count n 1",
        true,
        false,
    );
    run(&mut game, "gamerule commandBlocksEnabled false");
    game.run_ticks(5);
    assert_eq!(score(&game, "#count"), Some(0));
    run(&mut game, "gamerule commandBlocksEnabled true");
    game.run_ticks(5);
    assert!(score(&game, "#count").unwrap() >= 4);
}

#[test]
fn only_operators_in_creative_place_break_and_edit_them_and_the_editor_changes_the_block() {
    let mut game = world();
    // Survival, no cheats: nothing.
    assert!(!game.may_edit_command_blocks());
    game.player.mode = GameMode::Creative;
    assert!(
        !game.may_edit_command_blocks(),
        "creative alone isn't enough"
    );
    game.cheats = true;
    assert!(game.may_edit_command_blocks());
    game.player.mode = GameMode::Survival;
    assert!(!game.may_edit_command_blocks(), "an op in survival neither");
    game.player.mode = GameMode::Creative;
    // Editing changes the block's kind, keeps its facing, sets conditional.
    let at = IVec3::new(5, 70, 28);
    put(
        &mut game,
        at,
        Mode::Impulse,
        Face::North,
        "say x",
        false,
        false,
    );
    let edit = Edit {
        at: at.to_array(),
        command: "say y".into(),
        mode: Mode::Repeat,
        conditional: true,
        auto: true,
        track_output: false,
        last_output: String::new(),
    };
    game.finish_command_block_edit(Some(edit));
    let state = game.chunks.world().block(at.x, at.y, at.z).unwrap();
    assert_eq!(state.kind(), blocks::REPEATING_COMMAND_BLOCK);
    assert_eq!(state.face(Prop::FacingAll), Face::North);
    assert!(state.bool(Prop::Conditional));
    // Not allowed any more: the edit is ignored.
    game.player.mode = GameMode::Survival;
    let edit = Edit {
        at: at.to_array(),
        command: "say z".into(),
        mode: Mode::Chain,
        conditional: false,
        auto: false,
        track_output: true,
        last_output: String::new(),
    };
    game.finish_command_block_edit(Some(edit));
    let state = game.chunks.world().block(at.x, at.y, at.z).unwrap();
    assert_eq!(state.kind(), blocks::REPEATING_COMMAND_BLOCK);
    // /give hands them out, and /setblock makes one with its data readable.
    game.player.mode = GameMode::Creative;
    assert!(said(&run(&mut game, "give @s command_block")).starts_with("Gave 1"));
    assert!(said(&run(&mut game, "give @s chain_command_block 2")).starts_with("Gave 2"));
    assert!(said(&run(&mut game, "give @s repeating_command_block")).starts_with("Gave 1"));
    assert!(said(&run(&mut game, "data get block 5 70 28 Command")).contains("say y"));
    assert!(
        said(&run(
            &mut game,
            "data merge block 5 70 28 {Command:\"say new\",auto:0b}"
        ))
        .starts_with("Modified")
    );
    assert!(said(&run(&mut game, "data get block 5 70 28 Command")).contains("say new"));
    // Editing opens only for those allowed.
    game.player.mode = GameMode::Survival;
    game.open_command_block_for_test(at);
    assert!(game.player.command_block_edit.is_none());
    assert!(
        game.player
            .message
            .as_ref()
            .is_some_and(|(m, _)| m.contains("Only operators in creative"))
    );
}

#[test]
fn a_command_block_minecart_runs_on_a_powered_activator_rail() {
    use crate::entity::minecart::CartKind;
    let mut game = operator();
    setup(&mut game);
    let rail = IVec3::new(8, 64, 8);
    // A redstone block under the rail powers it.
    game.set_blocks(&[
        (rail - IVec3::Y, BlockState::of(blocks::REDSTONE_BLOCK)),
        (rail, BlockState::of(blocks::ACTIVATOR_RAIL)),
    ]);
    game.run_ticks(3);
    assert!(game.place_cart(rail, CartKind::CommandBlock));
    // The editor edits it by index; the command lands on the cart.
    game.open_command_block_for_test(rail); // not a block: nothing opens
    assert!(game.player.command_block_edit.is_none());
    let edit = Edit {
        at: [0, crate::game::CART_EDIT, 0],
        command: "scoreboard players add #cart n 1".to_owned(),
        mode: Mode::Impulse,
        conditional: false,
        auto: false,
        track_output: true,
        last_output: String::new(),
    };
    game.finish_command_block_edit(Some(edit));
    assert_eq!(game.carts[0].command, "scoreboard players add #cart n 1");
    game.run_ticks(20);
    let n = score(&game, "#cart").unwrap();
    assert!(
        (4..=6).contains(&n),
        "every four ticks on a powered rail: {n}"
    );
    // Off the powered rail it stops.
    game.set_blocks(&[(rail - IVec3::Y, BlockState::of(blocks::DIRT))]);
    game.run_ticks(5);
    let stopped = score(&game, "#cart").unwrap();
    game.run_ticks(10);
    assert_eq!(score(&game, "#cart"), Some(stopped));
    assert_eq!(
        crate::game::minecarts_item_of(CartKind::CommandBlock),
        crate::inventory::items::COMMAND_BLOCK_MINECART
    );
}
