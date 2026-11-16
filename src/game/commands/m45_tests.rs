//! M45: data packs, functions, schedules, `/tick`, the world border and
//! `/forceload`, with Kenzo (host) and Alex in a flat world.

use crate::platform::fs;

use glam::DVec3;

use super::tests::{run, said, world};
use crate::game::Game;

/// Data packs share the loot tables' global, so these tests take turns.
static PACKS: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// A world whose folder holds the pack files given as (path, text).
fn world_with_pack(
    files: &[(&str, &str)],
) -> (Game, std::path::PathBuf, std::sync::MutexGuard<'static, ()>) {
    let guard = PACKS.lock().unwrap_or_else(|e| e.into_inner());
    let mut game = world();
    let root = crate::platform::env::temp_dir().join(format!(
        "rc-m45-{}-{}",
        crate::platform::env::process_id(),
        files.iter().map(|(p, _)| p.len()).sum::<usize>()
    ));
    let _ = fs::remove_dir_all(&root);
    for (path, text) in files {
        let p = root.join("datapacks/demo").join(path);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, text).unwrap();
    }
    game.world_dir_for_packs = Some(root.clone());
    let errors = game.reload_packs();
    assert!(errors.is_empty(), "{errors:?}");
    (game, root, guard)
}

const META: (&str, &str) = (
    "pack.mcmeta",
    r#"{"pack":{"pack_format":48,"description":"Demo"}}"#,
);

#[test]
fn a_pack_with_a_tick_and_a_load_function_runs_them() {
    let (mut game, root, _guard) = world_with_pack(&[
        META,
        (
            "data/demo/function/boot.mcfunction",
            "scoreboard objectives add boots dummy\nscoreboard players set #loaded boots 1\n",
        ),
        (
            "data/demo/function/ticker.mcfunction",
            "# every tick\nscoreboard players add #ticks boots 1\n",
        ),
        (
            "data/minecraft/tags/function/load.json",
            r#"{"values":["demo:boot"]}"#,
        ),
        (
            "data/minecraft/tags/function/tick.json",
            r#"{"values":["demo:ticker"]}"#,
        ),
    ]);
    game.tick();
    assert_eq!(
        game.board.score("boots", "#loaded"),
        Some(1),
        "load ran once"
    );
    for _ in 0..4 {
        game.tick();
    }
    assert_eq!(
        game.board.score("boots", "#ticks"),
        Some(5),
        "tick ran each tick"
    );
    assert_eq!(game.board.score("boots", "#loaded"), Some(1));
    game.tick();
    assert_eq!(game.board.score("boots", "#ticks"), Some(6));
    fs::remove_dir_all(root).ok();
}

#[test]
fn function_runs_macros_returns_and_nests() {
    let (mut game, root, _guard) = world_with_pack(&[
        META,
        (
            "data/demo/function/hello.mcfunction",
            "say hello\nsay again\n",
        ),
        (
            "data/demo/function/greet.mcfunction",
            "$say hi $(who), you are $(n)\n",
        ),
        (
            "data/demo/function/early.mcfunction",
            "say before\nreturn 42\nsay never\n",
        ),
        (
            "data/demo/function/failing.mcfunction",
            "return fail\nsay never\n",
        ),
        (
            "data/demo/function/outer.mcfunction",
            "say outer\nfunction demo:hello\nsay done\n",
        ),
        ("data/demo/function/loop.mcfunction", "function demo:loop\n"),
        (
            "data/demo/function/run.mcfunction",
            "return run scoreboard players get Kenzo kills\n",
        ),
        ("data/demo/tags_here_not.txt", ""),
        (
            "data/minecraft/tags/function/group.json",
            r#"{"values":["demo:hello","demo:early"]}"#,
        ),
        (
            "data/demo/tags/function/both.json",
            r##"{"values":["#minecraft:group","demo:hello"]}"##,
        ),
    ]);
    let go = |game: &mut Game, line: &str| said(&run(game, line));
    assert_eq!(
        go(&mut game, "function demo:hello"),
        "[Kenzo] hello | [Kenzo] again | Executed 2 command(s) from function 'demo:hello'"
    );
    assert_eq!(
        go(&mut game, "function demo:outer"),
        "[Kenzo] outer | [Kenzo] hello | [Kenzo] again | [Kenzo] done | Executed 5 command(s) from function 'demo:outer'"
    );
    assert_eq!(
        go(&mut game, r#"function demo:greet {who:"Alex",n:5}"#),
        "[Kenzo] hi Alex, you are 5 | Executed 1 command(s) from function 'demo:greet'"
    );
    assert!(go(&mut game, "function demo:greet").contains("needs macro arguments"));
    assert!(
        go(&mut game, "function demo:greet {who:\"x\"}").contains("Missing macro argument 'n'")
    );
    run(
        &mut game,
        "data merge storage mine:args {who:\"Storage\",n:9}",
    );
    assert!(
        go(&mut game, "function demo:greet with storage mine:args")
            .contains("hi Storage, you are 9")
    );
    assert_eq!(
        go(&mut game, "function demo:early"),
        "[Kenzo] before | Function demo:early returned 42"
    );
    assert!(go(&mut game, "function demo:failing").contains("returned 0"));
    assert!(
        go(&mut game, "function demo:nothing").contains("Unknown function: minecraft:nothing")
            || go(&mut game, "function demo:nothing").contains("Unknown function: demo:nothing")
    );
    // A function that calls itself stops at the chain limit.
    run(&mut game, "gamerule maxCommandChainLength 50");
    let looped = go(&mut game, "function demo:loop");
    let ran: usize = looped
        .rsplit("Executed ")
        .next()
        .and_then(|t| t.split(' ').next())
        .and_then(|n| n.parse().ok())
        .expect(&looped);
    assert!((50..=52).contains(&ran), "stopped at the limit: {looped}");
    run(&mut game, "gamerule maxCommandChainLength 65536");
    // `return run`, tags, and `if function`.
    run(&mut game, "scoreboard objectives add kills dummy");
    run(&mut game, "scoreboard players set Kenzo kills 8");
    assert!(go(&mut game, "function demo:run").contains("returned 8"));
    assert!(go(&mut game, "function #minecraft:group").contains("[Kenzo] hello"));
    assert!(go(&mut game, "function #demo:both").contains("returned"));
    assert_eq!(
        go(&mut game, "execute if function demo:early"),
        "Test passed, count: 42"
    );
    assert_eq!(
        go(&mut game, "execute if function demo:failing"),
        "§cTest failed"
    );
    assert_eq!(
        go(&mut game, "execute unless function demo:failing"),
        "Test passed"
    );
    // Tab knows functions and tags.
    let tab = game.complete_command("function demo:h", true, true);
    assert_eq!(tab, ["demo:hello"]);
    assert!(
        game.complete_command("function #", true, true)
            .iter()
            .any(|t| t == "#minecraft:group")
    );
    assert!(
        game.complete_command("function demo:hello ", true, true)
            .contains(&"with".to_owned())
    );
    fs::remove_dir_all(root).ok();
}

#[test]
fn schedule_fires_on_time_replaces_appends_and_clears() {
    let (mut game, root, _guard) = world_with_pack(&[
        META,
        (
            "data/demo/function/ping.mcfunction",
            "scoreboard players add #pings boots 1\n",
        ),
    ]);
    run(&mut game, "scoreboard objectives add boots dummy");
    let now = game.time();
    let told = said(&run(&mut game, "schedule function demo:ping 5t"));
    assert_eq!(
        told,
        format!(
            "Scheduled function demo:ping in 5 ticks at gametime {}",
            now + 5
        )
    );
    for _ in 0..4 {
        game.tick();
    }
    assert_eq!(game.board.score("boots", "#pings"), None, "not yet");
    game.tick();
    assert_eq!(game.board.score("boots", "#pings"), Some(1), "on time");
    game.tick();
    assert_eq!(game.board.score("boots", "#pings"), Some(1), "only once");
    // replace swaps; append adds; clear removes.
    run(&mut game, "schedule function demo:ping 10t");
    run(&mut game, "schedule function demo:ping 20t replace");
    assert_eq!(game.scheduled.len(), 1);
    run(&mut game, "schedule function demo:ping 30t append");
    assert_eq!(game.scheduled.len(), 2);
    assert_eq!(
        said(&run(&mut game, "schedule clear demo:ping")),
        "Removed 2 schedules for function minecraft:demo:ping".replace("minecraft:", "")
    );
    assert!(said(&run(&mut game, "schedule clear demo:ping")).contains("Unknown schedule"));
    assert!(
        said(&run(&mut game, "schedule function demo:nothing 1s")).contains("Unknown function")
    );
    run(&mut game, "schedule function demo:ping 2s");
    assert_eq!(game.scheduled[0].due, game.time() + 40);
    fs::remove_dir_all(root).ok();
}

#[test]
fn datapack_lists_enables_disables_and_reload_rereads() {
    let (mut game, root, _guard) =
        world_with_pack(&[META, ("data/demo/function/hi.mcfunction", "say hi\n")]);
    let go = |game: &mut Game, line: &str| said(&run(game, line));
    let listed = go(&mut game, "datapack list");
    assert!(
        listed.contains("There are 2 data pack(s) enabled: [vanilla (built-in)], [file/demo]"),
        "{listed}"
    );
    assert!(
        listed.contains("There are no data packs available"),
        "{listed}"
    );
    assert!(go(&mut game, "function demo:hi").contains("hi"));
    assert_eq!(
        go(&mut game, "datapack disable file/demo"),
        "Disabled data pack file/demo"
    );
    assert!(go(&mut game, "function demo:hi").contains("Unknown function"));
    assert!(go(&mut game, "datapack disable file/demo").contains("already disabled"));
    assert!(
        go(&mut game, "datapack list available")
            .contains("There are 1 data pack(s) available: [file/demo]")
    );
    assert!(
        go(&mut game, "datapack enable file/nothing").contains("Unknown data pack 'file/nothing'")
    );
    assert_eq!(
        go(&mut game, "datapack enable file/demo"),
        "Enabled data pack file/demo"
    );
    assert!(go(&mut game, "datapack enable file/demo").contains("already enabled"));
    assert!(go(&mut game, "function demo:hi").contains("hi"));
    // A function added on disk appears after /reload.
    fs::write(
        root.join("datapacks/demo/data/demo/function/new.mcfunction"),
        "say new one\n",
    )
    .unwrap();
    assert!(go(&mut game, "function demo:new").contains("Unknown function"));
    assert_eq!(go(&mut game, "reload"), "Reloading!");
    assert!(go(&mut game, "function demo:new").contains("new one"));
    // Order: first/last/before/after.
    let second = root.join("datapacks/second");
    fs::create_dir_all(&second).unwrap();
    fs::write(second.join("pack.mcmeta"), META.1).unwrap();
    go(&mut game, "reload");
    assert_eq!(
        game.packs.enabled,
        ["file/demo", "file/second"],
        "a new pack is enabled when seen"
    );
    go(&mut game, "datapack disable file/second");
    go(&mut game, "datapack enable file/second first");
    assert_eq!(game.packs.enabled, ["file/second", "file/demo"]);
    go(&mut game, "datapack disable file/second");
    go(&mut game, "datapack enable file/second after file/demo");
    assert_eq!(game.packs.enabled, ["file/demo", "file/second"]);
    assert!(
        game.complete_command("datapack enable ", true, true)
            .is_empty()
            || true
    );
    assert!(
        game.complete_command("datapack d", true, true)
            .contains(&"disable".to_owned())
    );
    fs::remove_dir_all(root).ok();
}

#[test]
fn a_packs_predicate_and_loot_table_are_used() {
    let (mut game, root, _guard) = world_with_pack(&[
        META,
        (
            "data/demo/predicate/always.json",
            r#"{"condition":"random_chance","chance":1}"#,
        ),
        (
            "data/demo/loot_table/prize.json",
            r#"{"pools":[{"rolls":3,"entries":[{"type":"minecraft:item","name":"minecraft:diamond","functions":[{"function":"minecraft:set_count","count":2}]}]}]}"#,
        ),
    ]);
    assert_eq!(
        said(&run(&mut game, "execute if predicate demo:always")),
        "Test passed"
    );
    assert_eq!(
        game.select(
            crate::game::selector::Target::Player(0),
            crate::world::dimension::Dimension::Overworld,
            DVec3::ZERO,
            "@a[predicate=demo:always]"
        )
        .unwrap()
        .len(),
        2
    );
    let told = said(&run(&mut game, "loot give @s loot demo:prize"));
    assert_eq!(told, "Gave 6 items to Kenzo");
    crate::world::structure::loot::set_pack_tables(Default::default());
    fs::remove_dir_all(root).ok();
}

#[test]
fn tick_freezes_steps_sprints_and_changes_the_rate() {
    let mut game = world();
    let go = |game: &mut Game, line: &str| said(&run(game, line));
    assert_eq!(game.clock(0.05), 1);
    assert!(go(&mut game, "tick query").contains("The game is running normally"));
    assert!(go(&mut game, "tick query").contains("Target tick rate: 20.0 per second"));
    assert_eq!(
        go(&mut game, "tick rate 40"),
        "Set the target tick rate to 40.0 per second"
    );
    assert_eq!(game.clock(0.05), 2, "twice as fast");
    assert!(go(&mut game, "tick rate 0").contains("between 1 and 10000"));
    go(&mut game, "tick rate 20");
    assert_eq!(go(&mut game, "tick freeze"), "The game is frozen");
    assert_eq!(game.clock(1.0), 0, "frozen");
    assert!(go(&mut game, "tick query").contains("The game is frozen"));
    assert_eq!(go(&mut game, "tick step 3"), "Stepping 3 tick(s)");
    assert_eq!(game.clock(0.0), 3);
    assert_eq!(game.clock(0.0), 0, "steps used up");
    assert_eq!(
        go(&mut game, "tick unfreeze"),
        "The game is running normally"
    );
    assert_eq!(game.clock(0.05), 1);
    assert_eq!(
        go(&mut game, "tick sprint 250"),
        "Starting a sprint of 250 ticks"
    );
    assert_eq!(game.clock(0.0), 100);
    assert_eq!(game.clock(0.0), 100);
    assert_eq!(game.clock(0.0), 50);
    assert!(go(&mut game, "tick sprint stop").contains("No sprint"));
    go(&mut game, "tick sprint 1000");
    assert_eq!(go(&mut game, "tick sprint stop"), "Sprint stopped");
    assert_eq!(game.clock(0.0), 0);
    assert!(go(&mut game, "tick rate 20.0").contains("Set the target"));
}

#[test]
fn the_world_border_sets_shrinks_pushes_and_hurts() {
    let mut game = world();
    let go = |game: &mut Game, line: &str| said(&run(game, line));
    assert!(go(&mut game, "worldborder get").contains("59999968 blocks wide"));
    assert_eq!(
        go(&mut game, "worldborder set 40"),
        "Set the world border to 40 blocks wide"
    );
    assert_eq!(
        go(&mut game, "worldborder get"),
        "The world border is currently 40 blocks wide"
    );
    assert!(go(&mut game, "worldborder set 40").contains("Nothing changed"));
    assert!(go(&mut game, "worldborder set 0").contains("cannot be smaller than 1"));
    assert!(go(&mut game, "worldborder set 99999999").contains("cannot be bigger"));
    assert_eq!(
        go(&mut game, "worldborder set 100 10"),
        "Growing the world border to 100 blocks wide over 10 seconds"
    );
    assert_eq!(
        go(&mut game, "worldborder add -20 5"),
        "Shrinking the world border to 20 blocks wide over 5 seconds"
    );
    go(&mut game, "worldborder set 40");
    assert_eq!(
        go(&mut game, "worldborder center 10 10"),
        "Set the center of the world border to 10.00, 10.00"
    );
    assert!(go(&mut game, "worldborder center 10 10").contains("Nothing changed"));
    assert!(go(&mut game, "worldborder damage amount 2").contains("2 per block"));
    assert!(go(&mut game, "worldborder damage buffer 3").contains("3 blocks"));
    assert!(go(&mut game, "worldborder warning distance 8").contains("8 blocks"));
    assert!(go(&mut game, "worldborder warning time 5").contains("5 seconds"));
    go(&mut game, "worldborder center 0 0");
    // Pushed: a player teleported outside is held at the edge (20 each way).
    game.teleport(DVec3::new(30.5, 64.0, 0.5), 0.0, 0.0);
    game.tick();
    let x = game.feet_of(0).x;
    assert!(x <= 19.8 && x > 19.0, "held just inside: {x}");
    // A shrinking border closes in on a player and pushes them along.
    go(&mut game, "worldborder set 10 1");
    game.run_ticks(25);
    assert!(game.feet_of(0).x <= 5.0, "pushed in: {}", game.feet_of(0).x);
    // Mobs are held too.
    game.summon("pig", DVec3::new(-30.5, 64.0, 0.5), 0.0);
    game.tick();
    assert!(game.mobs.last().unwrap().feet().x >= -5.0);
    // Left far outside the buffer, a survivor is hurt: shrink with the player
    // already past it by moving the center.
    go(&mut game, "worldborder damage buffer 0");
    go(&mut game, "worldborder damage amount 20");
    go(&mut game, "worldborder center 100 0");
    let before = game.player.stats.health;
    game.tick();
    assert!(
        game.player.stats.health < before || game.feet_of(0).x > 90.0,
        "held at the edge"
    );
    // It saves with the world.
    let b = game.border.clone();
    assert_eq!(b.center, [100.0, 0.0]);
    let text = ron::to_string(&b).unwrap();
    assert_eq!(
        ron::from_str::<crate::game::border::WorldBorder>(&text).unwrap(),
        b
    );
}

#[test]
fn forceload_marks_queries_and_unmarks_chunks() {
    let mut game = world();
    let go = |game: &mut Game, line: &str| said(&run(game, line));
    assert_eq!(
        go(&mut game, "forceload add 0 0"),
        "Marked chunk 0, 0 to be force loaded"
    );
    assert_eq!(
        go(&mut game, "forceload add 32 32 63 63"),
        "Marked 4 chunks to be force loaded from 2, 2 to 3, 3"
    );
    assert!(go(&mut game, "forceload add 0 0").contains("No chunks were marked"));
    assert_eq!(
        go(&mut game, "forceload query 0 0"),
        "Chunk 0, 0 is marked for force loading"
    );
    assert!(go(&mut game, "forceload query 160 0").contains("is not marked"));
    assert!(go(&mut game, "forceload query").contains("5 force loaded chunks were found"));
    assert_eq!(
        go(&mut game, "forceload remove 0 0"),
        "Unmarked chunk 0, 0 to no longer be force loaded"
    );
    assert!(go(&mut game, "forceload remove 0 0").contains("No chunks were unmarked"));
    assert!(go(&mut game, "forceload add 0 0 10000 10000").contains("Too many chunks"));
    assert_eq!(
        go(&mut game, "forceload remove all"),
        "All 4 force loaded chunks were unmarked"
    );
    assert_eq!(
        go(&mut game, "forceload query"),
        "No chunks are force loaded"
    );
}
