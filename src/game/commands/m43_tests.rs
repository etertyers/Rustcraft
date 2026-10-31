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

fn block_name(game: &crate::game::Game, x: i32, y: i32, z: i32) -> String {
    game.chunks
        .world()
        .block(x, y, z)
        .unwrap()
        .def()
        .name
        .clone()
}

#[test]
fn give_takes_item_components_and_clear_counts() {
    let mut game = world();
    let told = said(&run(
        &mut game,
        r#"give @s diamond_sword[damage=5,enchantments={levels:{"minecraft:sharpness":3}},can_break=["stone"],custom_name='"Excalibur"'] 2"#,
    ));
    assert!(told.starts_with("Gave 2 ["), "{told}");
    let sword = (0..crate::inventory::player::SLOTS)
        .filter_map(|i| game.player.inventory.slot(i))
        .find(|s| s.id == items::DIAMOND_SWORD)
        .unwrap()
        .clone();
    assert_eq!(sword.damage, 5);
    assert_eq!(
        sword.level_of(crate::inventory::enchant::Enchantment::Sharpness),
        3
    );
    assert_eq!(sword.can_break(), ["stone"]);
    assert_eq!(sword.custom_name(), Some("Excalibur"));
    assert!(
        said(&run(&mut game, "give @s diamond_sword[bogus=1]")).contains("Unknown item component")
    );
    assert!(
        said(&run(&mut game, "give @s dirt[can_place_on=[\"nothing\"]]"))
            .contains("Unknown block type")
    );
    // /clear with a count of 0 only counts.
    assert_eq!(
        said(&run(&mut game, "clear @s diamond_sword 0")),
        "Found 2 matching item(s) on player Kenzo"
    );
    assert!(!said(&run(&mut game, "clear @s diamond_sword 1")).is_empty());
    assert_eq!(
        said(&run(&mut game, "clear @s diamond_sword 0")),
        "Found 1 matching item(s) on player Kenzo"
    );
}

#[test]
fn setblock_and_fill_take_block_states_filters_and_destroy() {
    let mut game = world();
    run(
        &mut game,
        "setblock 0 70 0 minecraft:oak_stairs[facing=east,half=top]",
    );
    let stairs = game.chunks.world().block(0, 70, 0).unwrap();
    let facing = crate::world::props::Prop::Facing;
    assert_eq!(stairs.def().name, "oak_stairs");
    assert_eq!(stairs.face(facing), crate::world::Face::East);
    assert!(
        said(&run(&mut game, "setblock 0 70 1 oak_stairs[nope=1]"))
            .contains("does not have property")
    );
    assert!(
        said(&run(&mut game, "setblock 0 70 1 oak_stairs[facing=up]")).contains("does not accept")
    );
    // Filtered replace only touches the filter.
    run(&mut game, "fill 0 70 2 2 70 2 stone");
    run(&mut game, "setblock 1 70 2 dirt");
    let told = said(&run(&mut game, "fill 0 70 2 2 70 2 glass replace stone"));
    assert_eq!(told, "Successfully filled 2 block(s)");
    assert_eq!(block_name(&game, 1, 70, 2), "dirt");
    assert_eq!(block_name(&game, 0, 70, 2), "glass");
    // Destroy drops items; replace doesn't.
    let before = game.entities.len();
    run(&mut game, "setblock 1 70 2 air destroy");
    assert!(game.entities.len() > before, "dirt dropped");
    // Hollow and outline keep their shapes.
    run(&mut game, "fill 10 70 10 12 72 12 stone hollow");
    assert_eq!(block_name(&game, 11, 71, 11), "air");
    assert_eq!(block_name(&game, 10, 71, 11), "stone");
    run(&mut game, "fill 20 70 10 22 72 12 stone outline");
    assert_eq!(block_name(&game, 21, 71, 11), "air");
    run(&mut game, "fill 20 70 10 22 72 12 dirt keep");
    assert_eq!(block_name(&game, 21, 71, 11), "dirt");
    assert!(said(&run(&mut game, "fill 0 0 0 100 100 100 stone")).contains("Too many blocks"));
}

#[test]
fn clone_copies_masked_filtered_and_moves_with_block_entities() {
    let mut game = world();
    run(&mut game, "fill 0 70 0 2 70 0 stone");
    run(&mut game, "setblock 1 70 0 dirt");
    run(&mut game, "setblock 0 70 1 chest");
    game.chunks.set_block_entity(
        IVec3::new(0, 70, 1),
        Some(crate::world::block_entity::BlockEntity::Chest({
            let mut slots = vec![None; crate::world::block_entity::CHEST_SLOTS];
            slots[0] = Some(ItemStack::new(items::DIAMOND, 3));
            slots
        })),
    );
    let told = said(&run(&mut game, "clone 0 70 0 2 70 0 0 72 0"));
    assert_eq!(told, "Successfully cloned 3 block(s)");
    assert_eq!(block_name(&game, 1, 72, 0), "dirt");
    // Overlap needs force or move.
    assert!(said(&run(&mut game, "clone 0 70 0 2 70 0 1 70 0")).contains("cannot overlap"));
    // Masked skips air; filtered takes only the filter.
    run(&mut game, "clone 0 70 0 2 71 0 10 70 0 masked");
    assert_eq!(block_name(&game, 10, 71, 0), "air");
    run(&mut game, "clone 0 70 0 2 70 0 20 70 0 filtered dirt");
    assert_eq!(block_name(&game, 21, 70, 0), "dirt");
    assert_eq!(
        block_name(&game, 20, 70, 0),
        "air",
        "only the dirt was cloned"
    );
    // A chest's contents travel with it, and move leaves air behind.
    let told = said(&run(&mut game, "clone 0 70 1 0 70 1 5 70 1 replace move"));
    assert_eq!(told, "Successfully cloned 1 block(s)");
    assert_eq!(block_name(&game, 0, 70, 1), "air");
    match game.chunks.block_entity(IVec3::new(5, 70, 1)) {
        Some(crate::world::block_entity::BlockEntity::Chest(slots)) => {
            assert_eq!(slots[0], Some(ItemStack::new(items::DIAMOND, 3)));
        }
        other => panic!("the chest's contents should move: {other:?}"),
    }
}

#[test]
fn fillbiome_sets_columns_and_place_puts_down_a_template() {
    let mut game = world();
    let told = said(&run(&mut game, "fillbiome 0 64 0 3 64 3 minecraft:desert"));
    assert!(told.starts_with("16 biome entries set"), "{told}");
    assert_eq!(
        game.chunks.world().biome_at(2, 2),
        Some(crate::world::biome::Biome::Desert)
    );
    assert!(
        said(&run(&mut game, "fillbiome 0 64 0 3 64 3 minecraft:desert"))
            .contains("No biomes were changed")
    );
    let told = said(&run(
        &mut game,
        "fillbiome 0 64 0 3 64 3 plains replace desert",
    ));
    assert!(told.starts_with("16 biome entries set"));
    assert!(said(&run(&mut game, "fillbiome 0 64 0 1 64 1 nowhere")).contains("Unknown biome"));
    let registry = crate::world::structure::template::registry();
    if let Some(name) = registry.templates.keys().next().cloned() {
        let told = said(&run(&mut game, &format!("place template {name} 0 80 0")));
        assert!(told.starts_with("Placed "), "{told}");
    }
    assert!(said(&run(&mut game, "place template nothing")).contains("Unknown template"));
}

#[test]
fn teleport_takes_rotation_facing_and_other_entities() {
    let mut game = world();
    run(&mut game, "tp @s 10 70 10 90 0");
    assert_eq!(game.feet_of(0), DVec3::new(10.0, 70.0, 10.0));
    assert!(
        (game.player.camera.yaw + std::f32::consts::FRAC_PI_2).abs() < 0.01,
        "faces west"
    );
    run(&mut game, "tp @s 0 70 0 facing 0 70 -5");
    assert!(game.player.camera.yaw.abs() < 0.2);
    game.summon("pig", DVec3::new(0.5, 70.0, -8.5), 0.0);
    run(&mut game, "tp @s 0.5 70 0.5 facing entity @e[type=pig]");
    assert!(
        game.player.camera.yaw.abs() < 0.2,
        "faces the pig, to the north"
    );
    // Mobs and other players too.
    run(&mut game, "tp @e[type=pig] Alex");
    let pig = game
        .mobs
        .iter()
        .find(|m| game.mob_defs.get(m.kind).name == "pig")
        .unwrap();
    assert_eq!(pig.feet(), game.feet_of(1));
    run(&mut game, "tp Alex 3 70 3");
    assert_eq!(game.feet_of(1), DVec3::new(3.0, 70.0, 3.0));
    run(&mut game, "tp 7 70 7");
    assert_eq!(game.feet_of(0), DVec3::new(7.0, 70.0, 7.0));
    run(&mut game, "tp Alex Kenzo");
    assert_eq!(game.feet_of(1), game.feet_of(0));
}

#[test]
fn summon_takes_entity_data() {
    let mut game = world();
    let told = said(&run(
        &mut game,
        r#"summon pig 4 70 4 {CustomName:'"Bacon"',Tags:["farm","tasty"],Health:5f}"#,
    ));
    assert_eq!(told, "Summoned new Pig");
    let pig = game.mobs.last().unwrap();
    assert_eq!(pig.data.custom_name.as_deref(), Some("Bacon"));
    assert_eq!(pig.data.tags, ["farm", "tasty"]);
    assert_eq!(pig.stats.health, 5.0);
    let at = DVec3::ZERO;
    assert_eq!(game.select(0, at, "@e[tag=farm]").unwrap().len(), 1);
    assert_eq!(game.select(0, at, "@e[name=Bacon]").unwrap().len(), 1);
    assert_eq!(game.select(0, at, "@e[nbt={Health:5f}]").unwrap().len(), 1);
    let count = game.mobs.len();
    assert!(said(&run(&mut game, "summon pig {Bogus:1}")).contains("Unknown entity data"));
    assert_eq!(game.mobs.len(), count, "a bad summon leaves nothing");
    assert!(said(&run(&mut game, "summon dragonfruit")).contains("Unknown entity type"));
}

#[test]
fn locate_finds_points_of_interest_and_tab_knows_block_states() {
    let mut game = world();
    run(&mut game, "setblock 20 64 0 composter");
    let told = said(&run(&mut game, "locate poi farmer"));
    assert!(told.contains("[20, ~, 0]"), "{told}");
    assert!(said(&run(&mut game, "locate poi cleric")).contains("Could not find"));
    assert!(said(&run(&mut game, "locate poi nonsense")).contains("Unknown point of interest"));
    let options = game.complete_command("locate poi f", true, true);
    assert_eq!(options, ["farmer"]);
    let options = game.complete_command("setblock ~ ~ ~ oak_stairs[fac", true, true);
    assert_eq!(options, ["oak_stairs[facing="]);
    let options = game.complete_command("setblock ~ ~ ~ oak_stairs[facing=n", true, true);
    assert_eq!(options, ["oak_stairs[facing=north"]);
    let options = game.complete_command("setblock ~ ~ ~ oak_stairs[facing=north,ha", true, true);
    assert_eq!(options, ["oak_stairs[facing=north,half="]);
    let options = game.complete_command("gamemode sp", true, true);
    assert_eq!(options, ["spectator"]);
    let options = game.complete_command("tp @", true, true);
    assert!(options.contains(&"@e".to_owned()) && options.contains(&"@s".to_owned()));
    let options = game.complete_command("setblock ~ ~ ", true, true);
    assert_eq!(options, ["^", "~"]);
}
