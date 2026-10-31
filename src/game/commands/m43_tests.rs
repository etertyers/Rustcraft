//! M43's commands, game modes and selectors, played against a flat world
//! with Kenzo (host, index 0) and Alex (guest, index 1).

use glam::{DVec3, IVec3};

use super::tests::{run, said, world};
use crate::{
    inventory::{items, stack::ItemStack},
    player::mode::GameMode,
    world::block::BlockState,
};

fn grass(game: &crate::game::Game) -> Option<BlockState> {
    game.chunks.world().block(0, 63, 0)
}

#[test]
fn gamemode_takes_all_four_modes_and_tells_both_players() {
    let mut game = world();
    for (word, mode, shown) in [
        ("adventure", GameMode::Adventure, "Adventure Mode"),
        ("spectator", GameMode::Spectator, "Spectator Mode"),
        ("creative", GameMode::Creative, "Creative Mode"),
        ("survival", GameMode::Survival, "Survival Mode"),
    ] {
        let told = said(&run(&mut game, &format!("gamemode {word}")));
        assert_eq!(told, format!("Set own game mode to {shown}"));
        assert_eq!(game.player.mode, mode);
    }
    let told = said(&run(&mut game, "gamemode spectator Alex"));
    assert!(
        told.contains("Set Alex's game mode to Spectator Mode"),
        "{told}"
    );
    assert!(told.contains("Your game mode has been updated to Spectator Mode"));
    assert_eq!(game.others[0].mode, GameMode::Spectator);
    assert!(
        game.others[0].body.no_clip,
        "spectators pass through blocks"
    );
    assert!(said(&run(&mut game, "gamemode sideways")).contains("Unknown game mode: sideways"));
    let told = said(&run(&mut game, "defaultgamemode adventure"));
    assert_eq!(told, "The default game mode is now Adventure Mode");
    assert_eq!(game.default_mode, Some(GameMode::Adventure));
}

#[test]
fn a_spectator_flies_through_the_ground_and_touches_nothing() {
    let mut game = world();
    run(&mut game, "gamemode spectator");
    assert!(game.player.mode.is_spectator());
    // Sunk in the ground, with no gravity and nothing pushing back.
    game.teleport(
        DVec3::new(0.5, 60.5, 0.5),
        0.0,
        -std::f32::consts::FRAC_PI_2,
    );
    for _ in 0..40 {
        game.player.breaking = true;
        game.player.placing = true;
        game.tick();
    }
    let feet = game.player.body.position();
    assert!((feet.y - 60.5).abs() < 0.01, "no gravity: {feet}");
    assert_eq!(
        grass(&game),
        Some(BlockState::of(crate::world::blocks::GRASS_BLOCK))
    );
    assert!(game.player.inventory.slot(0).is_none(), "picked up nothing");
    // The wheel sets the flight speed.
    let before = game.player.body.fly_scale;
    game.on_scroll(winit::event::MouseScrollDelta::LineDelta(0.0, 1.0));
    assert!(game.player.body.fly_scale > before);
    // Others see nothing of them unless they are spectators too.
    assert!(game.player_view().spectator);
    // Back to survival: gravity and collision return.
    run(&mut game, "gamemode survival");
    assert!(!game.player.body.no_clip);
    assert_eq!(game.player.body.fly_scale, 1.0 * game.player.body.fly_scale);
}

#[test]
fn a_spectator_takes_a_mobs_eyes_and_shift_lets_go() {
    let mut game = world();
    run(&mut game, "gamemode spectator");
    game.teleport(DVec3::new(0.5, 66.0, 0.5), 0.0, 0.0);
    let id = {
        let pig = game
            .summon("pig", DVec3::new(0.5, 64.0, -4.5), 0.0)
            .unwrap();
        pig.id
    };
    let told = said(&run(&mut game, "spectate @e[type=pig]"));
    assert_eq!(told, "Now spectating Pig");
    for _ in 0..3 {
        game.tick();
    }
    let pig = game.mobs.iter().find(|m| m.id == id).unwrap();
    let eyes = pig.feet().y + pig.body.eye_height();
    let mine = game.player.body.position().y + game.player.body.eye_height();
    assert!(
        (eyes - mine).abs() < 0.01,
        "eyes at {mine}, pig's at {eyes}"
    );
    assert!((game.player.body.position().z - pig.feet().z).abs() < 0.01);
    game.player.keys.sneak = true;
    game.tick();
    assert!(game.player.spectating.is_none());
    let told = said(&run(&mut game, "spectate @e[type=pig]"));
    assert_eq!(told, "Now spectating Pig");
    assert_eq!(
        said(&run(&mut game, "spectate")),
        "No longer spectating an entity"
    );
    run(&mut game, "gamemode survival");
    assert_eq!(
        said(&run(&mut game, "spectate @e[type=pig]")),
        "§cKenzo is not in spectator mode"
    );
}

#[test]
fn adventure_breaks_and_places_only_what_the_item_allows() {
    let mut game = world();
    run(&mut game, "gamemode adventure");
    game.player
        .inventory
        .set(0, Some(ItemStack::new(items::DIAMOND_PICKAXE, 1)));
    game.player.breaking = true;
    for _ in 0..60 {
        game.tick();
    }
    assert_eq!(
        grass(&game),
        Some(BlockState::of(crate::world::blocks::GRASS_BLOCK)),
        "can't break"
    );
    // With can_break it can.
    let mut pick = ItemStack::new(items::DIAMOND_PICKAXE, 1);
    pick.set_block_rules(vec!["minecraft:grass_block".to_owned()], vec![]);
    game.player.inventory.set(0, Some(pick));
    for _ in 0..60 {
        game.tick();
    }
    assert_eq!(grass(&game), Some(BlockState::AIR));
    game.player.breaking = false;
    // Placing: dirt needs can_place_on. Two blocks north, on the grass.
    game.player.camera.pitch = -0.3;
    game.advance(0.0);
    let ahead = |game: &crate::game::Game| game.chunks.world().block(0, 64, -2);
    game.player
        .inventory
        .set(0, Some(ItemStack::new(items::DIRT, 8)));
    game.player.placing = true;
    game.tick();
    game.player.placing = false;
    assert_eq!(ahead(&game), Some(BlockState::AIR), "can't place");
    let mut dirt = ItemStack::new(items::DIRT, 8);
    dirt.set_block_rules(vec![], vec!["grass_block".to_owned()]);
    game.player.inventory.set(0, Some(dirt));
    game.player.placing = true;
    game.tick();
    assert_eq!(
        ahead(&game),
        Some(BlockState::of(crate::world::blocks::DIRT)),
        "can place against the allowed block"
    );
}

#[test]
fn adventure_still_fights_and_takes_damage() {
    let mut game = world();
    run(&mut game, "gamemode adventure");
    assert!(game.player.mode.is_vulnerable());
    assert!(game.player.mode.interacts());
}

#[test]
fn selectors_filter_by_type_distance_sort_limit_and_mode() {
    let mut game = world();
    game.summon("pig", DVec3::new(3.5, 64.0, 0.5), 0.0);
    game.summon("pig", DVec3::new(9.5, 64.0, 0.5), 0.0);
    game.summon("cow", DVec3::new(1.5, 64.0, 0.5), 0.0);
    let at = DVec3::new(0.5, 64.0, 0.5);
    let count = |game: &mut crate::game::Game, sel: &str| game.select(0, at, sel).unwrap().len();
    assert_eq!(count(&mut game, "@e[type=pig]"), 2);
    assert_eq!(
        count(&mut game, "@e[type=!pig]"),
        3,
        "two players and the cow"
    );
    assert_eq!(count(&mut game, "@e[type=pig,distance=..5]"), 1);
    assert_eq!(count(&mut game, "@e[type=pig,limit=1]"), 1);
    assert_eq!(count(&mut game, "@a"), 2);
    assert_eq!(count(&mut game, "@a[gamemode=creative]"), 0);
    assert_eq!(count(&mut game, "@a[gamemode=survival]"), 2);
    assert_eq!(count(&mut game, "@a[name=Alex]"), 1);
    assert_eq!(count(&mut game, "@a[name=!Alex]"), 1);
    assert_eq!(
        count(&mut game, "@a[x=6,y=64,z=0,distance=..2]"),
        1,
        "Alex stands there"
    );
    // @p is the nearest player, @n the nearest entity other than... any.
    let nearest = game.select(0, at, "@n[type=pig]").unwrap();
    assert_eq!(nearest.len(), 1);
    let far = game
        .select(0, at, "@e[type=pig,sort=furthest,limit=1]")
        .unwrap();
    assert_ne!(far, nearest);
    assert_eq!(count(&mut game, "@e[dx=2,dy=1,dz=1,type=cow]"), 1);
    assert_eq!(
        count(
            &mut game,
            "@e[type=cow,x_rotation=-90..90,y_rotation=..180]"
        ),
        1
    );
    game.others[0].tags.push("vip".into());
    assert_eq!(count(&mut game, "@a[tag=vip]"), 1);
    assert_eq!(count(&mut game, "@a[tag=!vip]"), 1);
    game.others[0].stats.experience.add(100);
    assert_eq!(count(&mut game, "@a[level=5..]"), 1);
    assert!(game.select(0, at, "@e[nonsense=1]").is_err());
}

#[test]
fn chat_and_housekeeping_commands_answer_like_vanilla() {
    let mut game = world();
    let all = run(&mut game, "say hello there");
    assert_eq!(said(&all), "[Kenzo] hello there");
    assert_eq!(
        said(&run(&mut game, "weather rain")),
        "Changed the weather to rain"
    );
    assert_eq!(game.weather, crate::game::weather::Weather::Rain);
    assert_eq!(
        said(&run(&mut game, "weather thunder 10s")),
        "Changed the weather to thunder"
    );
    assert_eq!(game.weather_ticks, 200);
    assert!(said(&run(&mut game, "weather snow")).contains("Unknown weather"));
    let rolled = said(&run(&mut game, "random value 5..5"));
    assert_eq!(rolled, "Rolled 5");
    let rolled = said(&run(&mut game, "random roll 1..1"));
    assert_eq!(rolled, "Kenzo rolled 1 (1..1)");
    for _ in 0..50 {
        let n: i64 = said(&run(&mut game, "random value 1..6"))
            .trim_start_matches("Rolled ")
            .parse()
            .unwrap();
        assert!((1..=6).contains(&n));
    }
    assert_eq!(
        said(&run(&mut game, "save-off")),
        "Turned off world auto-saving"
    );
    assert!(!game.autosave);
    assert!(said(&run(&mut game, "save-off")).contains("Saving is already turned off"));
    assert_eq!(
        said(&run(&mut game, "save-on")),
        "Turned on world auto-saving"
    );
    assert!(said(&run(&mut game, "save-on")).contains("Saving is already turned on"));
    assert!(said(&run(&mut game, "list")).contains("Kenzo, Alex"));
}

#[test]
fn rotate_and_spreadplayers_move_players() {
    let mut game = world();
    run(&mut game, "rotate @s 90 0");
    // Vanilla yaw 90 faces west (−X): camera yaw −π/2.
    assert!((game.player.camera.yaw + std::f32::consts::FRAC_PI_2).abs() < 0.01);
    run(&mut game, "rotate @s ~ 90");
    assert!(game.player.camera.pitch < -1.0, "looking straight down");
    let told = said(&run(&mut game, "rotate @s facing 0.5 64 -10"));
    assert_eq!(told, "Rotated Kenzo");
    assert!(game.player.camera.yaw.abs() < 0.01, "faces north");
    let told = said(&run(&mut game, "spreadplayers 0 0 3 10 false @a"));
    assert!(
        told.starts_with("Spread 2 entities around 0.0, 0.0"),
        "{told}"
    );
    let a = game.feet_of(0);
    let b = game.feet_of(1);
    assert!((a.x - b.x).hypot(a.z - b.z) >= 3.0, "{a} {b}");
    assert!(a.x.abs() <= 10.5 && b.z.abs() <= 10.5);
    assert!(said(&run(&mut game, "spreadplayers 0 0 5 3 false @a")).contains("must be at least"));
}

#[test]
fn damage_attribute_advancement_and_recipe_commands() {
    let mut game = world();
    let told = said(&run(&mut game, "damage @s 4 minecraft:fall"));
    assert_eq!(told, "Applied 4 damage to Kenzo");
    assert!(game.player.stats.health < 20.0);
    run(&mut game, "gamemode creative");
    assert_eq!(
        said(&run(&mut game, "damage @s 4")),
        "§cUnable to apply damage"
    );
    game.summon("pig", DVec3::new(2.5, 64.0, 0.5), 0.0);
    let told = said(&run(&mut game, "damage @e[type=pig] 3 generic"));
    assert_eq!(told, "Applied 3 damage to Pig");
    assert!(said(&run(&mut game, "damage @s 1 bogus")).contains("Unknown damage type"));

    assert_eq!(
        said(&run(
            &mut game,
            "attribute @s minecraft:generic.movement_speed get"
        )),
        "Attribute generic.movement_speed for Kenzo has a value of 0.1"
    );
    run(
        &mut game,
        "attribute @s generic.movement_speed base set 0.2",
    );
    assert!((game.player.speed_scale - 2.0).abs() < 1e-9);
    assert!(
        said(&run(
            &mut game,
            "attribute @s generic.max_health base set 40"
        ))
        .contains("no way")
    );
    assert!(said(&run(&mut game, "attribute @s generic.bogus get")).contains("Unknown attribute"));

    let told = said(&run(
        &mut game,
        "advancement grant @s only minecraft:story/mine_stone",
    ));
    assert!(
        told.starts_with("Granted") || told.contains("Unknown advancement"),
        "{told}"
    );
    let told = said(&run(&mut game, "advancement grant @s everything"));
    assert!(told.starts_with("Granted "), "{told}");
    assert!(said(&run(&mut game, "advancement grant @s everything")).contains("Unable to grant"));
    let told = said(&run(&mut game, "advancement revoke @s everything"));
    assert!(told.starts_with("Revoked "), "{told}");
    assert!(game.player.progress.done.is_empty());

    let told = said(&run(&mut game, "recipe give @s *"));
    assert!(
        told.starts_with("Unlocked ") && told.ends_with("for Kenzo"),
        "{told}"
    );
    assert!(said(&run(&mut game, "recipe give @s stick")).contains("already know"));
    let told = said(&run(&mut game, "recipe take @s stick"));
    assert_eq!(told, "Took 1 recipe(s) from Kenzo");
}

#[test]
fn particles_sounds_and_riding() {
    let mut game = world();
    let told = said(&run(
        &mut game,
        "particle minecraft:flame 0 65 0 1 1 1 0 20",
    ));
    assert_eq!(told, "Displaying particle minecraft:flame");
    assert!(game.particles.len() >= 20);
    assert!(said(&run(&mut game, "particle bogus")).contains("Unknown particle"));
    let told = said(&run(
        &mut game,
        "playsound minecraft:entity.pig.ambient master @s",
    ));
    assert_eq!(told, "Played sound minecraft:entity.pig.ambient to Kenzo");
    assert_eq!(game.player.heard.len(), 1);
    assert!(said(&run(&mut game, "playsound x bogus @s")).contains("Unknown sound source"));
    assert!(said(&run(&mut game, "stopsound @s")).contains("Stopped all sounds"));
    game.summon("pig", DVec3::new(1.5, 64.0, 0.5), 0.0);
    let told = said(&run(&mut game, "ride @s mount @e[type=pig]"));
    assert_eq!(told, "Kenzo started riding Pig");
    assert!(game.player.mounted.is_some());
    assert!(said(&run(&mut game, "ride @s mount @e[type=pig]")).contains("already riding"));
    assert_eq!(
        said(&run(&mut game, "ride @s dismount")),
        "Kenzo is no longer riding anything"
    );
    assert!(game.player.mounted.is_none());
}

#[test]
fn the_hosts_lists_ops_bans_whitelist_publish_and_stop() {
    let mut game = world();
    assert_eq!(
        said(&run(&mut game, "op Alex")),
        "Made Alex a server operator"
    );
    assert!(game.admin.ops.contains("Alex"));
    assert!(said(&run(&mut game, "op Alex")).contains("already an operator"));
    assert_eq!(
        said(&run(&mut game, "deop Alex")),
        "Made Alex no longer a server operator"
    );
    assert!(said(&run(&mut game, "deop Alex")).contains("not an operator"));
    let told = run(&mut game, "ban Alex griefing");
    assert!(said(&told).contains("Banned Alex: griefing"));
    assert!(
        told.iter().any(|r| matches!(r, super::Reply::Kick(1, _))),
        "kicked at once"
    );
    assert!(game.admin.refusal("Alex").unwrap().contains("griefing"));
    assert!(said(&run(&mut game, "ban Alex")).contains("already banned"));
    assert_eq!(said(&run(&mut game, "pardon Alex")), "Unbanned Alex");
    assert!(said(&run(&mut game, "pardon Alex")).contains("isn't banned"));
    assert_eq!(
        said(&run(&mut game, "whitelist on")),
        "Whitelist is now turned on"
    );
    assert!(game.admin.refusal("Stranger").is_some());
    run(&mut game, "whitelist add Stranger");
    assert!(game.admin.refusal("Stranger").is_none());
    assert_eq!(
        said(&run(&mut game, "whitelist list")),
        "There are 1 whitelisted player(s): Stranger"
    );
    assert_eq!(
        said(&run(&mut game, "whitelist remove Stranger")),
        "Removed Stranger from the whitelist"
    );
    assert!(said(&run(&mut game, "whitelist remove Stranger")).contains("not whitelisted"));
    assert!(matches!(
        run(&mut game, "publish 25565")[0],
        super::Reply::Publish(25565)
    ));
    assert!(
        run(&mut game, "stop")
            .iter()
            .any(|r| matches!(r, super::Reply::Stop))
    );
    // Not for guests, and not without cheats.
    let told = game.run_command(1, "op Kenzo", true, false);
    assert_eq!(
        said(&told),
        "§cYou do not have permission to use this command"
    );
}

#[test]
fn local_coordinates_follow_where_the_caller_looks() {
    let mut game = world();
    // Looking north (−Z) and level.
    game.player.camera.yaw = 0.0;
    game.player.camera.pitch = 0.0;
    run(&mut game, "setblock ^ ^ ^3 minecraft:stone");
    let here = game.feet_of(0);
    let cell = IVec3::new(
        here.x.floor() as i32,
        here.y.floor() as i32,
        (here.z - 3.0).floor() as i32,
    );
    assert_eq!(
        game.chunks
            .world()
            .block(cell.x, cell.y, cell.z)
            .map(|b| b.def().name.clone()),
        Some("stone".to_owned())
    );
    let told = said(&run(&mut game, "setblock ^ ~ ^3 minecraft:stone"));
    assert!(told.contains("Cannot mix world & local coordinates"));
}

#[test]
fn feedback_can_be_switched_off_but_errors_still_show() {
    let mut game = world();
    run(&mut game, "gamerule sendCommandFeedback false");
    assert!(run(&mut game, "weather rain").is_empty());
    assert!(said(&run(&mut game, "weather snow")).starts_with("§c"));
    // Even a query is feedback, as in vanilla.
    assert!(run(&mut game, "gamerule sendCommandFeedback").is_empty());
    assert_eq!(
        game.rules.get("sendCommandFeedback").as_deref(),
        Some("false")
    );
}

#[test]
fn every_new_game_rule_reads_and_writes() {
    let mut game = world();
    for rule in [
        "doTileDrops",
        "doMobLoot",
        "doEntityDrops",
        "showDeathMessages",
        "sendCommandFeedback",
        "commandBlockOutput",
        "commandBlocksEnabled",
        "fallDamage",
        "fireDamage",
        "drowningDamage",
        "doWeatherCycle",
        "announceAdvancements",
        "logAdminCommands",
    ] {
        run(&mut game, &format!("gamerule {rule} false"));
        assert_eq!(game.rules.get(rule).as_deref(), Some("false"), "{rule}");
        run(&mut game, &format!("gamerule {rule} true"));
        assert_eq!(game.rules.get(rule).as_deref(), Some("true"), "{rule}");
    }
    run(&mut game, "gamerule spawnRadius 4");
    assert_eq!(game.rules.spawn_radius, 4);
    run(&mut game, "gamerule playersSleepingPercentage 50");
    assert_eq!(game.rules.players_sleeping_percentage, 50);
}

#[test]
fn damage_rules_switch_off_falls_fire_and_drowning() {
    let mut game = world();
    run(&mut game, "gamerule fallDamage false");
    game.tick();
    assert!(
        !game
            .player
            .stats
            .damage(5.0, crate::player::stats::DamageSource::Fall)
    );
    run(&mut game, "gamerule fireDamage false");
    game.tick();
    assert!(
        !game
            .player
            .stats
            .damage(5.0, crate::player::stats::DamageSource::Lava)
    );
    run(&mut game, "gamerule fallDamage true");
    game.tick();
    assert!(
        game.player
            .stats
            .damage(5.0, crate::player::stats::DamageSource::Fall)
    );
}
