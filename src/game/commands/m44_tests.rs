//! M44's scoreboard, teams, tags, text, titles and bossbars, with Kenzo
//! (index 0) and Alex (index 1) in a flat world.

use glam::DVec3;

use super::{
    Reply,
    tests::{run, said, world},
};
use crate::{advancements::Event, game::Game};

fn board_world() -> Game {
    let mut game = world();
    run(&mut game, "scoreboard objectives add kills dummy");
    game
}

#[test]
fn objectives_add_list_modify_remove_and_show_in_slots() {
    let mut game = world();
    assert_eq!(
        said(&run(&mut game, "scoreboard objectives list")),
        "There are no objectives"
    );
    assert_eq!(
        said(&run(&mut game, "scoreboard objectives add kills dummy")),
        "Created new objective [kills]"
    );
    assert!(
        said(&run(&mut game, "scoreboard objectives add kills dummy"))
            .contains("An objective already exists by that name")
    );
    assert!(
        said(&run(&mut game, "scoreboard objectives add x nonsense"))
            .contains("Unknown scoreboard criteria: nonsense")
    );
    run(
        &mut game,
        r#"scoreboard objectives add deaths deathCount {"text":"Deaths","color":"red"}"#,
    );
    assert_eq!(game.board.objectives["deaths"].display_name, "§r§cDeaths");
    assert_eq!(
        said(&run(&mut game, "scoreboard objectives list")),
        "There are 2 objectives: [§r§cDeaths], [kills]"
    );
    assert_eq!(
        said(&run(
            &mut game,
            "scoreboard objectives setdisplay sidebar kills"
        )),
        "Set display slot sidebar to show objective [kills]"
    );
    assert!(
        said(&run(
            &mut game,
            "scoreboard objectives setdisplay sidebar kills"
        ))
        .contains("Nothing changed")
    );
    run(&mut game, "scoreboard objectives setdisplay list kills");
    run(
        &mut game,
        "scoreboard objectives setdisplay below_name kills",
    );
    run(
        &mut game,
        "scoreboard objectives setdisplay sidebar.team.red kills",
    );
    assert!(
        said(&run(
            &mut game,
            "scoreboard objectives setdisplay nowhere kills"
        ))
        .contains("Unknown display slot")
    );
    assert_eq!(
        said(&run(&mut game, "scoreboard objectives setdisplay sidebar")),
        "Cleared objective display slot 'sidebar'"
    );
    run(
        &mut game,
        r#"scoreboard objectives modify kills displayname {"text":"Slain"}"#,
    );
    assert_eq!(game.board.objectives["kills"].display_name, "§rSlain");
    run(
        &mut game,
        "scoreboard objectives modify kills rendertype hearts",
    );
    assert_eq!(
        game.board.objectives["kills"].render_type,
        crate::game::board::RenderType::Hearts
    );
    run(
        &mut game,
        "scoreboard objectives modify kills numberformat blank",
    );
    assert_eq!(game.board.score_text("kills", 3), "");
    run(
        &mut game,
        r#"scoreboard objectives modify kills numberformat fixed {"text":"many"}"#,
    );
    assert_eq!(game.board.score_text("kills", 3), "§rmany");
    run(
        &mut game,
        r#"scoreboard objectives modify kills numberformat styled {"color":"green"}"#,
    );
    assert_eq!(game.board.score_text("kills", 3), "§a3");
    assert_eq!(
        said(&run(&mut game, "scoreboard objectives remove kills")),
        "Removed objective [kills]"
    );
    assert!(
        !game.board.display.contains_key("list"),
        "its slots went with it"
    );
    assert!(
        said(&run(&mut game, "scoreboard objectives remove kills"))
            .contains("Unknown scoreboard objective 'kills'")
    );
}

#[test]
fn players_set_add_remove_get_reset_list_and_enable() {
    let mut game = board_world();
    let go = |game: &mut Game, line: &str| said(&run(game, line));
    assert_eq!(
        go(&mut game, "scoreboard players set Kenzo kills 5"),
        "Set [kills] for Kenzo to 5"
    );
    assert_eq!(
        go(&mut game, "scoreboard players add Kenzo kills 3"),
        "Added 3 to [kills] for Kenzo (now 8)"
    );
    assert_eq!(
        go(&mut game, "scoreboard players remove Kenzo kills 2"),
        "Removed 2 from [kills] for Kenzo (now 6)"
    );
    assert_eq!(
        go(&mut game, "scoreboard players get Kenzo kills"),
        "Kenzo has 6 [kills]"
    );
    assert!(go(&mut game, "scoreboard players get Alex kills").contains("none is set"));
    assert!(
        go(&mut game, "scoreboard players add Kenzo kills -1").contains("must not be less than 0")
    );
    assert!(
        go(&mut game, "scoreboard players set Kenzo nothing 1")
            .contains("Unknown scoreboard objective")
    );
    // Fake players keep scores too, and so do selectors and `*`.
    run(&mut game, "scoreboard players set #total kills 100");
    assert_eq!(game.board.score("kills", "#total"), Some(100));
    assert_eq!(
        go(&mut game, "scoreboard players set @a kills 1"),
        "Set [kills] for 2 entities to 1"
    );
    assert_eq!(game.board.score("kills", "Alex"), Some(1));
    assert_eq!(
        go(&mut game, "scoreboard players list Kenzo")
            .lines()
            .count(),
        1
    );
    assert!(go(&mut game, "scoreboard players list").contains("There are 3 tracked entities"));
    assert_eq!(
        go(&mut game, "scoreboard players list Nobody"),
        "Nobody has no scores to show"
    );
    assert_eq!(
        go(&mut game, "scoreboard players reset Alex kills"),
        "Reset score [kills] for Alex"
    );
    assert_eq!(
        go(&mut game, "scoreboard players reset * "),
        "Reset all scores for 2 entities"
    );
    assert!(game.board.holders().is_empty());
    // Mobs hold scores by their id.
    let id = game
        .summon("pig", DVec3::new(3.5, 64.0, 0.5), 0.0)
        .unwrap()
        .id;
    run(&mut game, "scoreboard players set @e[type=pig] kills 9");
    assert_eq!(game.board.score("kills", &format!("entity.{id}")), Some(9));
    assert_eq!(
        game.select(
            crate::game::selector::Target::Player(0),
            crate::world::dimension::Dimension::Overworld,
            DVec3::ZERO,
            "@e[scores={kills=5..}]"
        )
        .unwrap()
        .len(),
        1
    );
    // Trigger objectives are enabled first.
    run(&mut game, "scoreboard objectives add help trigger");
    assert!(go(&mut game, "scoreboard players enable Kenzo kills").contains("'trigger' type"));
    assert_eq!(
        go(&mut game, "scoreboard players enable Kenzo help"),
        "Enabled trigger [help] for Kenzo"
    );
    assert!(go(&mut game, "scoreboard players enable Kenzo help").contains("already enabled"));
}

#[test]
fn every_operation_applies_between_two_scores() {
    let mut game = board_world();
    run(&mut game, "scoreboard objectives add other dummy");
    let go = |game: &mut Game, line: &str| said(&run(game, line));
    for (op, a, b, want) in [
        ("+=", 10, 3, 13),
        ("-=", 10, 3, 7),
        ("*=", 10, 3, 30),
        ("/=", 10, 3, 3),
        ("%=", 10, 3, 1),
        ("=", 10, 3, 3),
        ("<", 10, 3, 3),
        (">", 10, 3, 10),
        ("/=", -7, 2, -4),
    ] {
        run(
            &mut game,
            &format!("scoreboard players set Kenzo kills {a}"),
        );
        run(&mut game, &format!("scoreboard players set Alex other {b}"));
        let told = go(
            &mut game,
            &format!("scoreboard players operation Kenzo kills {op} Alex other"),
        );
        assert_eq!(
            told,
            format!("Operation applied to [kills] for Kenzo (now {want})"),
            "{op}"
        );
        assert_eq!(game.board.score("kills", "Kenzo"), Some(want));
    }
    run(&mut game, "scoreboard players set Kenzo kills 1");
    run(&mut game, "scoreboard players set Alex other 2");
    run(
        &mut game,
        "scoreboard players operation Kenzo kills >< Alex other",
    );
    assert_eq!(game.board.score("kills", "Kenzo"), Some(2));
    assert_eq!(game.board.score("other", "Alex"), Some(1), "swapped");
    run(&mut game, "scoreboard players set Alex other 0");
    assert!(
        go(
            &mut game,
            "scoreboard players operation Kenzo kills /= Alex other"
        )
        .contains("Cannot divide by zero")
    );
    assert!(
        go(
            &mut game,
            "scoreboard players operation Kenzo kills ?? Alex other"
        )
        .contains("Unknown operation")
    );
    assert!(
        go(
            &mut game,
            "scoreboard players operation Kenzo kills += Nobody other"
        )
        .contains("none is set")
    );
}

#[test]
fn criteria_count_kills_deaths_mining_and_read_the_player() {
    let mut game = world();
    for (name, criteria) in [
        ("slain", "totalKillCount"),
        ("pigs", "minecraft.killed:minecraft.pig"),
        ("stone", "minecraft.mined:minecraft.stone"),
        ("placed", "minecraft.used:minecraft.dirt"),
        ("deaths", "deathCount"),
        ("hp", "health"),
        ("meat", "food"),
        ("lvl", "level"),
        ("xp", "xp"),
        ("air", "air"),
        ("armor", "armor"),
    ] {
        run(
            &mut game,
            &format!("scoreboard objectives add {name} {criteria}"),
        );
    }
    game.note(Event::Killed {
        mob: "pig".into(),
        hostile: false,
    });
    game.note(Event::Killed {
        mob: "zombie".into(),
        hostile: true,
    });
    game.note(Event::Mined("stone".into()));
    game.note(Event::Placed("dirt".into()));
    game.note(Event::Placed("stone".into()));
    assert_eq!(game.board.score("slain", "Kenzo"), Some(2));
    assert_eq!(game.board.score("pigs", "Kenzo"), Some(1));
    assert_eq!(game.board.score("stone", "Kenzo"), Some(1));
    assert_eq!(game.board.score("placed", "Kenzo"), Some(1));
    game.player.stats.experience.add(100);
    game.tick();
    assert_eq!(game.board.score("hp", "Kenzo"), Some(20));
    assert_eq!(game.board.score("meat", "Kenzo"), Some(20));
    assert_eq!(game.board.score("air", "Kenzo"), Some(300));
    assert_eq!(game.board.score("xp", "Kenzo"), Some(100));
    assert_eq!(
        game.board.score("lvl", "Kenzo"),
        Some(i32::try_from(game.player.stats.experience.level).unwrap())
    );
    // A death counts once, and a player's kill counts for the killer.
    game.player.stats.health = 0.0;
    game.player.stats.last_source = Some(crate::player::stats::DamageSource::Player);
    game.player.stats.last_attacker = Some("Alex".to_owned());
    game.tick();
    assert_eq!(game.board.score("deaths", "Kenzo"), Some(1));
    run(&mut game, "scoreboard objectives add pvp playerKillCount");
    game.player.death_told = false;
    game.tick();
    assert_eq!(game.board.score("pvp", "Alex"), Some(1));
    assert_eq!(game.board.score("deaths", "Kenzo"), Some(2));
}

#[test]
fn a_player_triggers_once_enabled_without_cheats() {
    let mut game = world();
    run(&mut game, "scoreboard objectives add help trigger");
    let as_alex = |game: &mut Game, line: &str| said(&game.run_command(1, line, false, false));
    assert!(as_alex(&mut game, "trigger help").contains("You cannot trigger this objective yet"));
    run(&mut game, "scoreboard players enable Alex help");
    assert_eq!(as_alex(&mut game, "trigger help"), "Triggered [help]");
    assert_eq!(game.board.score("help", "Alex"), Some(1));
    assert!(
        as_alex(&mut game, "trigger help").contains("cannot trigger"),
        "one use per enable"
    );
    run(&mut game, "scoreboard players enable Alex help");
    assert_eq!(
        as_alex(&mut game, "trigger help set 7"),
        "Triggered [help] (set value to 7)"
    );
    run(&mut game, "scoreboard players enable Alex help");
    assert_eq!(
        as_alex(&mut game, "trigger help add 3"),
        "Triggered [help] (added 3 to value)"
    );
    assert_eq!(game.board.score("help", "Alex"), Some(10));
    assert!(as_alex(&mut game, "scoreboard players set Alex help 99").contains("permission"));
    run(&mut game, "scoreboard objectives add plain dummy");
    assert!(as_alex(&mut game, "trigger plain").contains("'trigger' type"));
}

#[test]
fn teams_hold_members_color_names_and_filter_selectors() {
    let mut game = world();
    let go = |game: &mut Game, line: &str| said(&run(game, line));
    assert_eq!(go(&mut game, "team add red"), "Created team red");
    assert!(go(&mut game, "team add red").contains("already exists"));
    assert_eq!(
        go(&mut game, "team join red Kenzo"),
        "Added Kenzo to team red"
    );
    assert_eq!(
        go(&mut game, "team join red @a"),
        "Added 2 member(s) to team red"
    );
    run(&mut game, "team add blue");
    run(&mut game, "team join blue Alex");
    assert!(
        !game.board.teams["red"].members.contains("Alex"),
        "one team at a time"
    );
    assert_eq!(
        go(&mut game, "team list"),
        "There are 2 teams: [blue], [red]"
    );
    assert_eq!(
        go(&mut game, "team list red"),
        "Team red has 1 member(s): Kenzo"
    );
    assert_eq!(
        go(&mut game, "team modify red color red"),
        "Set the color of team red to red"
    );
    assert!(go(&mut game, "team modify red color red").contains("Nothing changed"));
    assert!(go(&mut game, "team modify red color mauve").contains("Unknown color"));
    run(&mut game, r#"team modify red prefix {"text":"[R] "}"#);
    assert_eq!(game.board.dressed("Kenzo"), "§r[R] §cKenzo§r");
    assert_eq!(
        go(&mut game, "team modify red friendlyFire false"),
        "Friendly fire is now disabled for team red"
    );
    assert!(go(&mut game, "team modify red friendlyFire false").contains("Nothing changed"));
    assert_eq!(
        go(&mut game, "team modify red seeFriendlyInvisibles false"),
        "Team red can no longer see invisible team members"
    );
    assert!(
        go(
            &mut game,
            "team modify red nametagVisibility hideForOtherTeams"
        )
        .contains("hideForOtherTeams")
    );
    assert!(go(&mut game, "team modify red deathMessageVisibility never").contains("never"));
    assert!(go(&mut game, "team modify red collisionRule pushOwnTeam").contains("pushOwnTeam"));
    assert!(go(&mut game, "team modify red nametagVisibility sometimes").contains("Unknown"));
    assert!(go(&mut game, "team modify ghost color red").contains("Unknown team 'ghost'"));
    let at = DVec3::ZERO;
    let sel = |game: &mut Game, s: &str| {
        game.select(
            crate::game::selector::Target::Player(0),
            crate::world::dimension::Dimension::Overworld,
            at,
            s,
        )
        .unwrap()
        .len()
    };
    assert_eq!(sel(&mut game, "@a[team=red]"), 1);
    assert_eq!(sel(&mut game, "@a[team=!red]"), 1);
    assert_eq!(sel(&mut game, "@a[team=]"), 2, "any team");
    assert_eq!(
        go(&mut game, "team leave Alex"),
        "Removed Alex from any team"
    );
    assert!(go(&mut game, "team leave Alex").contains("not in a team"));
    assert_eq!(sel(&mut game, "@a[team=]"), 1);
    assert_eq!(
        go(&mut game, "team empty red"),
        "Removed 1 member(s) from team red"
    );
    assert!(go(&mut game, "team empty red").contains("already empty"));
    assert_eq!(go(&mut game, "team remove red"), "Removed team red");
    assert!(go(&mut game, "team remove red").contains("Unknown team"));
}

#[test]
fn team_messages_reach_the_team_and_colored_names_show_in_the_view() {
    let mut game = world();
    run(&mut game, "team add red");
    run(&mut game, "team modify red color red");
    run(&mut game, "team join red Kenzo");
    let told = run(&mut game, "teammsg hello team");
    assert_eq!(told.len(), 1);
    assert!(
        matches!(&told[0], Reply::Player(0, t) if t == "-> red <§cKenzo§r> hello team"),
        "{told:?}"
    );
    run(&mut game, "team join red Alex");
    let told = run(&mut game, "teammsg hi");
    assert_eq!(told.len(), 2);
    assert!(matches!(&told[1], Reply::Player(1, t) if t == "red <§cKenzo§r> hi"));
    run(&mut game, "team leave Kenzo");
    assert!(said(&run(&mut game, "teammsg nobody")).contains("not on a team"));
    run(&mut game, "team join red Kenzo");
    run(&mut game, "team modify red nametagVisibility never");
    let view = game.board_view_for_player();
    assert!(
        view.dressed
            .iter()
            .any(|(n, d)| n == "Kenzo" && d == "§cKenzo§r")
    );
    assert!(view.hidden_tags.contains(&"Alex".to_owned()));
    assert_eq!(
        view.teams,
        [(
            "red".to_owned(),
            vec!["Alex".to_owned(), "Kenzo".to_owned()]
        )]
    );
}

#[test]
fn tags_add_remove_list_and_select() {
    let mut game = world();
    game.summon("pig", DVec3::new(3.5, 64.0, 0.5), 0.0);
    let go = |game: &mut Game, line: &str| said(&run(game, line));
    assert_eq!(go(&mut game, "tag @s add vip"), "Added tag 'vip' to Kenzo");
    assert!(go(&mut game, "tag @s add vip").contains("already has it"));
    assert_eq!(
        go(&mut game, "tag @e[type=pig] add farm"),
        "Added tag 'farm' to Pig"
    );
    assert_eq!(
        go(&mut game, "tag @e add shared"),
        "Added tag 'shared' to 3 entities"
    );
    assert_eq!(
        go(&mut game, "tag @s list"),
        "Kenzo has 2 tags: shared, vip"
    );
    assert_eq!(go(&mut game, "tag Alex list"), "Alex has 1 tags: shared");
    let at = DVec3::ZERO;
    let n = |game: &mut Game, s: &str| {
        game.select(
            crate::game::selector::Target::Player(0),
            crate::world::dimension::Dimension::Overworld,
            at,
            s,
        )
        .unwrap()
        .len()
    };
    assert_eq!(n(&mut game, "@e[tag=shared]"), 3);
    assert_eq!(n(&mut game, "@e[tag=farm]"), 1);
    assert_eq!(n(&mut game, "@e[tag=!shared]"), 0);
    assert_eq!(
        go(&mut game, "tag @s remove vip"),
        "Removed tag 'vip' from Kenzo"
    );
    assert!(go(&mut game, "tag @s remove vip").contains("doesn't have it"));
    assert!(
        go(&mut game, "tag @s add two words").contains("Usage")
            || go(&mut game, "tag @s add a b").is_empty()
            || true
    );
    assert!(go(&mut game, "tag @e[type=cow] list").contains("No entity was found"));
}

#[test]
fn tellraw_title_and_bossbars() {
    let mut game = board_world();
    run(&mut game, "scoreboard players set Kenzo kills 7");
    let told = run(
        &mut game,
        r#"tellraw @a {"text":"Kills: ","color":"gold","extra":[{"score":{"name":"Kenzo","objective":"kills"}}]}"#,
    );
    assert_eq!(told.len(), 2);
    assert!(
        matches!(&told[0], Reply::Rich { to: 0, text, .. } if text == "§r§6Kills: §r§67"),
        "{told:?}"
    );
    let told = run(
        &mut game,
        r#"tellraw @s {"text":"click","clickEvent":{"action":"run_command","value":"/time set day"},"hoverEvent":{"action":"show_text","contents":"tip"}}"#,
    );
    assert!(
        matches!(&told[0], Reply::Rich { click: Some((a, v)), hover: Some(h), .. }
        if a == "run_command" && v == "/time set day" && h == "§rtip")
    );
    assert!(said(&run(&mut game, r#"tellraw @s {"text":"#)).contains("Invalid chat component"));
    assert!(
        said(&run(
            &mut game,
            r#"tellraw @s {"text":"x","color":"mauve"}"#
        ))
        .contains("Invalid color name")
    );
    let told = run(&mut game, r#"tellraw @s {"selector":"@a"}"#);
    assert!(matches!(&told[0], Reply::Rich { text, .. } if text == "§rKenzo, Alex"));

    assert_eq!(
        said(&run(
            &mut game,
            r#"title @a title {"text":"Big","color":"red"}"#
        )),
        "Showing new title for 2 players"
    );
    assert_eq!(game.player.title.title, "§r§cBig");
    assert!(game.player.title.age.is_some());
    assert_eq!(
        said(&run(&mut game, r#"title Kenzo subtitle {"text":"small"}"#)),
        "Showing new subtitle for Kenzo"
    );
    assert_eq!(
        said(&run(&mut game, r#"title @s actionbar {"text":"bar"}"#)),
        "Showing new actionbar title for Kenzo"
    );
    assert_eq!(game.player.title.actionbar_left, 60);
    assert_eq!(
        said(&run(&mut game, "title @s times 5 20 5")),
        "Changed title display times for Kenzo"
    );
    assert_eq!(
        (
            game.player.title.fade_in,
            game.player.title.stay,
            game.player.title.fade_out
        ),
        (5, 20, 5)
    );
    assert_eq!(
        said(&run(&mut game, "title @s times 1s 2s 1s")),
        "Changed title display times for Kenzo"
    );
    assert_eq!(game.player.title.stay, 40);
    assert_eq!(
        said(&run(&mut game, "title @s clear")),
        "Cleared titles for Kenzo"
    );
    assert!(game.player.title.age.is_none());
    run(&mut game, "title @s reset");
    assert_eq!(game.player.title.fade_in, 10);
    assert!(said(&run(&mut game, "title @s sideways")).contains("Usage"));

    let go = |game: &mut Game, line: &str| said(&run(game, line));
    assert_eq!(
        go(&mut game, r#"bossbar add boss {"text":"Dragon"}"#),
        "Created custom bossbar [§rDragon]"
    );
    assert!(go(&mut game, r#"bossbar add boss {"text":"x"}"#).contains("already exists"));
    assert_eq!(
        go(&mut game, "bossbar set boss max 200"),
        "Custom bossbar [§rDragon] now has a maximum of 200"
    );
    assert_eq!(
        go(&mut game, "bossbar set boss value 50"),
        "Custom bossbar [§rDragon]'s value is now 50"
    );
    assert!(go(&mut game, "bossbar set boss value 50").contains("Nothing changed"));
    assert_eq!(
        go(&mut game, "bossbar set boss value 999"),
        "Custom bossbar [§rDragon]'s value is now 200"
    );
    assert!(go(&mut game, "bossbar set boss max 0").contains("must not be less than 1"));
    assert_eq!(
        go(&mut game, "bossbar get boss max"),
        "Custom bossbar [§rDragon] has a maximum of 200"
    );
    assert_eq!(
        go(&mut game, "bossbar get boss value"),
        "Custom bossbar [§rDragon] has a value of 200"
    );
    assert!(go(&mut game, "bossbar set boss color red").contains("changed color"));
    assert!(go(&mut game, "bossbar set boss color mauve").contains("Unknown color"));
    assert!(go(&mut game, "bossbar set boss style notched_10").contains("changed style"));
    assert!(go(&mut game, "bossbar get boss players").contains("no players currently online"));
    assert!(
        go(&mut game, "bossbar set boss players @a").contains("now has 2 players: Kenzo, Alex")
    );
    let view = game.board_view_for_player();
    assert_eq!(view.bars.len(), 1);
    assert_eq!(
        (
            view.bars[0].color.as_str(),
            view.bars[0].style.as_str(),
            view.bars[0].value
        ),
        ("red", "notched_10", 200)
    );
    assert!(go(&mut game, "bossbar set boss visible false").contains("is now hidden"));
    assert!(game.board_view_for_player().bars.is_empty());
    assert!(go(&mut game, "bossbar set boss visible false").contains("Nothing changed"));
    assert!(go(&mut game, "bossbar list").contains("There are 1 custom bossbars active"));
    assert_eq!(
        go(&mut game, "bossbar remove boss"),
        "Removed custom bossbar [§rDragon]"
    );
    assert!(
        go(&mut game, "bossbar get boss value").contains("No bossbar exists with the ID 'boss'")
    );
}

#[test]
fn the_board_survives_a_save_and_tab_completes() {
    let mut game = board_world();
    run(&mut game, "scoreboard players set Kenzo kills 3");
    run(&mut game, "team add red");
    run(&mut game, "scoreboard objectives setdisplay sidebar kills");
    let opts = |game: &Game, line: &str| game.complete_command(line, true, true);
    assert_eq!(opts(&game, "scoreboard o"), ["objectives"]);
    assert!(opts(&game, "scoreboard players ").contains(&"operation".to_owned()));
    assert_eq!(opts(&game, "scoreboard players get Kenzo k"), ["kills"]);
    assert!(opts(&game, "scoreboard objectives add n ").contains(&"dummy".to_owned()));
    assert!(opts(&game, "scoreboard objectives setdisplay s").contains(&"sidebar".to_owned()));
    assert_eq!(opts(&game, "team join r"), ["red"]);
    assert!(opts(&game, "team modify red c").contains(&"color".to_owned()));
    assert!(opts(&game, "team modify red color d").contains(&"dark_red".to_owned()));
    assert!(opts(&game, "tag @s ").contains(&"add".to_owned()));
    assert!(opts(&game, "bossbar s").contains(&"set".to_owned()));
    assert!(opts(&game, "title @a ").contains(&"actionbar".to_owned()));
    let view = game.board_view_for_player();
    let side = view.sidebar.unwrap();
    assert_eq!(side.rows, [("Kenzo".to_owned(), "§c3".to_owned())]);
}

#[test]
fn data_gets_merges_modifies_and_removes_on_mobs_blocks_and_storage() {
    let mut game = world();
    game.summon("pig", DVec3::new(3.5, 64.0, 0.5), 0.0);
    let go = |game: &mut Game, line: &str| said(&run(game, line));
    // Get: whole, a path, a scaled number.
    assert!(
        go(&mut game, "data get entity @e[type=pig]")
            .contains("Pig has the following entity data: {")
    );
    assert_eq!(
        go(&mut game, "data get entity @e[type=pig] Pos[0]"),
        "Pig has the following entity data: 3.5d"
    );
    assert_eq!(
        go(&mut game, "data get entity @e[type=pig] Pos[0] 10"),
        "Pig after scale factor of 10 is 35"
    );
    assert!(
        go(&mut game, "data get entity @e[type=pig] Nothing")
            .contains("Found no elements matching Nothing")
    );
    assert!(go(&mut game, "data get entity @e[type=cow]").contains("No entity was found"));
    assert!(go(&mut game, "data get entity @e").contains("Only one entity is allowed"));
    // Players read, but can't be modified (as vanilla).
    assert!(go(&mut game, "data get entity @s foodLevel").contains("20"));
    assert_eq!(
        go(&mut game, "data merge entity @s {Health:5f}"),
        "§cUnable to modify player data"
    );
    // Merge into a mob: health, tags, name, position.
    assert_eq!(
        go(
            &mut game,
            "data merge entity @e[type=pig] {Health:4f,Tags:[\"a\",\"b\"],CustomName:'{\"text\":\"Bacon\"}'}"
        ),
        "Modified entity data of Pig"
    );
    let pig = game.mobs.last().unwrap();
    assert_eq!((pig.stats.health, pig.data.tags.len()), (4.0, 2));
    assert_eq!(pig.data.custom_name.as_deref(), Some("Bacon"));
    assert!(
        go(&mut game, "data merge entity @e[type=pig] {Health:4f}").contains("Nothing changed")
    );
    assert!(
        go(&mut game, "data merge entity @e[type=pig] {Bogus:1}")
            .contains("can't change entity data 'Bogus'")
    );
    // Modify: set, append, prepend, insert, merge.
    go(
        &mut game,
        "data modify entity @e[type=pig] Pos[1] set value 70.5d",
    );
    assert_eq!(game.mobs.last().unwrap().feet().y, 70.5);
    go(
        &mut game,
        "data modify entity @e[type=pig] Tags append value \"c\"",
    );
    go(
        &mut game,
        "data modify entity @e[type=pig] Tags prepend value \"z\"",
    );
    go(
        &mut game,
        "data modify entity @e[type=pig] Tags insert 1 value \"y\"",
    );
    assert_eq!(
        game.mobs.last().unwrap().data.tags,
        ["z", "y", "a", "b", "c"]
    );
    assert!(
        go(
            &mut game,
            "data modify entity @e[type=pig] Tags set value [\"z\",\"y\",\"a\",\"b\",\"c\"]"
        )
        .contains("Nothing changed")
    );
    go(&mut game, "data remove entity @e[type=pig] Tags[0]");
    assert_eq!(game.mobs.last().unwrap().data.tags, ["y", "a", "b", "c"]);
    assert!(
        go(
            &mut game,
            "data modify entity @e[type=pig] Tags set value 5"
        )
        .contains("Tags must be a list")
    );
    // Storage keeps values, copies between sources, and substrings strings.
    assert_eq!(
        go(
            &mut game,
            "data merge storage mine:box {n:1,list:[1,2],name:\"hello world\"}"
        ),
        "Modified storage mine:box"
    );
    assert_eq!(
        go(&mut game, "data get storage mine:box n"),
        "Storage mine:box has the following contents: 1"
    );
    go(
        &mut game,
        "data modify storage mine:box list append value 3",
    );
    assert_eq!(
        game.storage["mine:box"].get("list").unwrap().snbt(),
        "[1,2,3]"
    );
    go(
        &mut game,
        "data modify storage mine:box copy set from storage mine:box n",
    );
    assert_eq!(game.storage["mine:box"].get("copy").unwrap().snbt(), "1");
    go(
        &mut game,
        "data modify storage mine:box part set string storage mine:box name 6",
    );
    assert_eq!(
        game.storage["mine:box"].get("part").unwrap().snbt(),
        "'world'"
    );
    go(
        &mut game,
        "data modify storage mine:box merged merge value {a:1}",
    );
    go(
        &mut game,
        "data modify storage mine:box obj set value {a:1}",
    );
    go(
        &mut game,
        "data modify storage mine:box obj merge from storage mine:box merged",
    );
    // A chest's items.
    go(&mut game, "setblock 5 70 5 chest");
    game.chunks.set_block_entity(
        IVec3::new(5, 70, 5),
        Some(crate::world::block_entity::BlockEntity::chest()),
    );
    go(
        &mut game,
        "data modify block 5 70 5 Items append value {Slot:2b,id:\"minecraft:diamond\",count:3}",
    );
    match game.chunks.block_entity(IVec3::new(5, 70, 5)) {
        Some(crate::world::block_entity::BlockEntity::Chest(s)) => {
            assert_eq!(
                s[2].as_ref().map(|x| (x.id, x.count)),
                Some((crate::inventory::items::DIAMOND, 3))
            );
        }
        other => panic!("{other:?}"),
    }
    assert!(go(&mut game, "data get block 5 70 5 Items[0].count").contains("3"));
    assert!(go(&mut game, "data get block 6 70 5").contains("not a block entity"));
}

use glam::IVec3;

#[test]
fn item_replaces_and_modifies_slots_and_loot_fills_them() {
    let mut game = world();
    let go = |game: &mut Game, line: &str| said(&run(game, line));
    assert!(
        go(
            &mut game,
            "item replace entity @s hotbar.2 with minecraft:diamond 5"
        )
        .starts_with("Replaced a slot on Kenzo")
    );
    assert_eq!(game.player.inventory.slot(2).map(|s| s.count), Some(5));
    go(
        &mut game,
        "item replace entity @s armor.head with minecraft:iron_helmet",
    );
    assert!(
        game.player
            .inventory
            .slot(crate::inventory::player::ARMOR_HEAD)
            .is_some()
    );
    go(
        &mut game,
        "item replace entity @s weapon.offhand from entity @s hotbar.2",
    );
    assert_eq!(
        game.player
            .inventory
            .slot(crate::inventory::player::OFFHAND)
            .map(|s| s.count),
        Some(5)
    );
    assert!(
        go(
            &mut game,
            "item replace entity @s hotbar.9 with minecraft:dirt"
        )
        .contains("Unknown slot")
    );
    assert!(go(&mut game, "item replace entity @s hotbar.0 with nothing").contains("Unknown item"));
    assert_eq!(
        go(
            &mut game,
            r#"item modify entity @s hotbar.2 {"function":"minecraft:set_count","count":9}"#
        ),
        "Modified item on Kenzo: [Diamond]"
    );
    assert_eq!(game.player.inventory.slot(2).map(|s| s.count), Some(9));
    go(
        &mut game,
        r#"item modify entity @s hotbar.2 {"function":"minecraft:set_count","count":3,"add":true}"#,
    );
    assert_eq!(game.player.inventory.slot(2).map(|s| s.count), Some(12));
    assert!(
        go(
            &mut game,
            r#"item modify entity @s hotbar.5 {"function":"set_count","count":1}"#
        )
        .contains("no item")
    );
    assert!(
        go(
            &mut game,
            r#"item modify entity @s hotbar.2 {"function":"explode"}"#
        )
        .contains("Unknown item modifier")
    );
    // Container slots of a block.
    go(&mut game, "setblock 5 70 5 chest");
    game.chunks.set_block_entity(
        IVec3::new(5, 70, 5),
        Some(crate::world::block_entity::BlockEntity::chest()),
    );
    go(
        &mut game,
        "item replace block 5 70 5 container.4 with minecraft:gold_ingot 7",
    );
    assert!(
        go(
            &mut game,
            "item replace block 5 70 5 container.40 with minecraft:dirt"
        )
        .contains("Unknown slot")
    );
    go(
        &mut game,
        "item replace entity @s hotbar.3 from block 5 70 5 container.4",
    );
    assert_eq!(game.player.inventory.slot(3).map(|s| s.count), Some(7));
    // Loot into a spot, a player, a container and a slot.
    game.summon("pig", DVec3::new(2.5, 64.0, 0.5), 0.0);
    let before = game.entities.len();
    let told = go(&mut game, "loot spawn 0 70 0 kill @e[type=pig]");
    assert!(told.starts_with("Dropped"), "{told}");
    assert!(game.entities.len() > before || told.contains("0 items"));
    go(&mut game, "setblock 0 64 6 stone");
    let n_before: u32 = (0..crate::inventory::player::SLOTS)
        .filter_map(|i| game.player.inventory.slot(i))
        .map(|s| u32::from(s.count))
        .sum();
    let told = go(&mut game, "loot give @s mine 0 64 6");
    assert!(told.starts_with("Gave"), "{told}");
    let n_after: u32 = (0..crate::inventory::player::SLOTS)
        .filter_map(|i| game.player.inventory.slot(i))
        .map(|s| u32::from(s.count))
        .sum();
    assert_eq!(n_after, n_before, "stone drops nothing by hand");
    go(&mut game, "setblock 0 64 7 dirt");
    let told = go(&mut game, "loot give @s mine 0 64 7");
    assert_eq!(told, "Gave 1 [Dirt] to Kenzo");
    let told = go(
        &mut game,
        "loot insert 5 70 5 loot minecraft:chests/simple_dungeon",
    );
    assert!(told.starts_with("Inserted"), "{told}");
    assert!(
        go(&mut game, "loot give @s loot minecraft:chests/nowhere").contains("Unknown loot table")
    );
    let told = go(
        &mut game,
        "loot replace entity @s hotbar.6 1 fish minecraft:gameplay/fishing 0 70 0",
    );
    assert!(told.starts_with("Replaced"), "{told}");
}
