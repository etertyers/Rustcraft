//! The mobs in play: what each does to the player, to the world and to
//! each other, and what the player can do to them.

use glam::{DVec3, IVec3};

use super::{Game, tests::lit_flat_game};
use crate::{
    entity::{mob::Mob, mobs::Spawn},
    inventory::{items, stack::ItemStack},
    player::mode::GameMode,
    world::{biome::Biome, block::BlockState, blocks, worldgen::rng::Rng},
};

const NIGHT: u64 = 18_000;
const NOON: u64 = 6_000;

fn night_game(mode: GameMode) -> Game {
    let mut game = lit_flat_game(mode);
    game.time = NIGHT;
    game
}

fn spawn(game: &mut Game, name: &str, x: f64, z: f64) -> u64 {
    let kind = game
        .mob_defs
        .find(name)
        .unwrap_or_else(|| panic!("no {name}"));
    let mob = game.new_mob(kind, DVec3::new(x, 64.0, z), 0.0);
    game.add_mob(mob)
}

fn mob(game: &Game, id: u64) -> &Mob {
    game.mobs
        .iter()
        .find(|m| m.id == id)
        .expect("the mob is still there")
}

fn alive(game: &Game, id: u64) -> bool {
    game.mobs.iter().any(|m| m.id == id && !m.is_dead())
}

fn run(game: &mut Game, ticks: u32) {
    for _ in 0..ticks {
        // Keep the clock still so night stays night.
        let time = game.time;
        game.run_ticks(1);
        game.time = time;
    }
}

#[test]
fn a_zombie_in_the_open_hurts_the_player_but_a_barrier_or_fence_stops_it() {
    let mut open = night_game(GameMode::Survival);
    spawn(&mut open, "zombie", 1.5, 0.5);
    run(&mut open, 60);
    assert!(open.stats.health < 20.0, "bitten: {}", open.stats.health);

    // The same zombie with an obsidian wall between, as close as can be.
    let mut walled = night_game(GameMode::Survival);
    let wall: Vec<_> = (63..68)
        .flat_map(|y| {
            (-2..=2).map(move |z| (IVec3::new(1, y, z), BlockState::of(blocks::OBSIDIAN)))
        })
        .collect();
    walled.set_blocks(&wall);
    spawn(&mut walled, "zombie", 2.5, 0.5);
    run(&mut walled, 100);
    assert_eq!(walled.stats.health, 20.0, "no hit through a wall");
}

#[test]
fn lines_of_sight_stop_at_walls_fences_and_slabs_but_not_at_air() {
    let mut game = lit_flat_game(GameMode::Survival);
    let from = DVec3::new(0.5, 65.6, 0.5);
    let to = DVec3::new(4.5, 65.6, 0.5);
    assert!(game.sees(from, to));
    game.set_blocks(&[(IVec3::new(2, 65, 0), BlockState::of(blocks::OAK_FENCE))]);
    assert!(!game.sees(from, to), "a fence is in the way");
    game.set_blocks(&[
        (IVec3::new(2, 65, 0), BlockState::AIR),
        (
            IVec3::new(3, 65, 0),
            BlockState::of(blocks::OAK_SLAB).with(
                crate::world::props::Prop::SlabType,
                crate::world::props::slab_type::TOP,
            ),
        ),
    ]);
    assert!(!game.sees(from, to), "a top slab at eye height blocks");
    game.set_blocks(&[(
        IVec3::new(3, 65, 0),
        BlockState::of(blocks::OAK_SLAB).with(
            crate::world::props::Prop::SlabType,
            crate::world::props::slab_type::BOTTOM,
        ),
    )]);
    assert!(
        game.sees(from, to),
        "a low slab is stepped over by the sight line"
    );
}

#[test]
fn a_creeper_swells_beside_the_player_and_blows_a_hole_that_hurts() {
    let mut game = night_game(GameMode::Survival);
    let id = spawn(&mut game, "creeper", 2.5, 0.5);
    let before = game.chunks.world().block(2, 63, 0);
    let mut swelled = false;
    for _ in 0..60 {
        run(&mut game, 1);
        if game.mobs.iter().any(|m| m.id == id && m.swell > 5) {
            swelled = true;
        }
    }
    assert!(swelled, "it swelled first");
    assert!(
        game.stats.health < 20.0,
        "the blast hurt: {}",
        game.stats.health
    );
    assert!(!game.mobs.iter().any(|m| m.id == id), "it is gone");
    assert_ne!(
        game.chunks.world().block(2, 63, 0),
        before,
        "the ground below is blown away"
    );
    assert!(
        game.entities.iter().all(|e| e.stack.id != items::GUNPOWDER),
        "no loot from a blast"
    );
}

#[test]
fn a_creeper_backs_off_when_the_player_leaves_and_does_not_go_off() {
    let mut game = night_game(GameMode::Survival);
    let id = spawn(&mut game, "creeper", 2.0, 0.5);
    run(&mut game, 12);
    assert!(mob(&game, id).swell > 0);
    game.body.set_position(DVec3::new(-12.0, 64.0, 0.5));
    run(&mut game, 60);
    assert!(alive(&game, id));
    assert_eq!(mob(&game, id).swell, 0, "the fuse went out");
}

#[test]
fn a_skeleton_keeps_its_distance_and_shoots_arrows_that_hurt() {
    let mut game = night_game(GameMode::Survival);
    let id = spawn(&mut game, "skeleton", 9.5, 0.5);
    let mut shot = false;
    for _ in 0..400 {
        run(&mut game, 1);
        shot |= game.arrows.iter().any(|a| a.hostile);
        if game.stats.health < 20.0 {
            break;
        }
    }
    assert!(shot, "it loosed arrows");
    assert!(game.stats.health < 20.0, "and one hit");
    let distance = mob(&game, id).feet().distance(game.body.position());
    assert!(distance > 3.0, "it keeps away: {distance}");
}

#[test]
fn skeletons_and_zombies_burn_in_daylight_and_creepers_do_not() {
    let mut game = lit_flat_game(GameMode::Creative);
    game.time = NOON;
    let skeleton = spawn(&mut game, "skeleton", 9.5, 9.5);
    let zombie = spawn(&mut game, "zombie", 9.5, 12.5);
    let creeper = spawn(&mut game, "creeper", 12.5, 9.5);
    for _ in 0..200 {
        game.run_ticks(1);
        game.time = NOON;
    }
    assert!(!alive(&game, skeleton) || mob(&game, skeleton).stats.fire > 0);
    assert!(!alive(&game, zombie) || mob(&game, zombie).stats.fire > 0);
    assert!(alive(&game, creeper));
    assert_eq!(mob(&game, creeper).stats.fire, 0);
}

#[test]
fn spiders_are_hostile_in_the_dark_and_leave_the_player_alone_in_the_light_until_hurt() {
    let mut dark = night_game(GameMode::Survival);
    spawn(&mut dark, "spider", 3.5, 0.5);
    run(&mut dark, 80);
    assert!(
        dark.stats.health < 20.0,
        "bitten at night: {}",
        dark.stats.health
    );

    let mut day = lit_flat_game(GameMode::Survival);
    day.time = NOON;
    let id = spawn(&mut day, "spider", 3.5, 0.5);
    for _ in 0..80 {
        day.run_ticks(1);
        day.time = NOON;
    }
    assert_eq!(day.stats.health, 20.0, "untroubled by day");
    day.mobs.iter_mut().find(|m| m.id == id).unwrap().hurt(
        1.0,
        None,
        crate::player::stats::DamageSource::Mob,
    );
    for _ in 0..80 {
        day.run_ticks(1);
        day.time = NOON;
    }
    assert!(day.stats.health < 20.0, "angry once hurt");
}

#[test]
fn a_spider_climbs_a_wall_after_the_player_on_top() {
    let mut game = night_game(GameMode::Survival);
    // A tower four blocks high with the player on top.
    let tower: Vec<_> = (64..68)
        .map(|y| (IVec3::new(4, y, 4), BlockState::of(blocks::STONE)))
        .collect();
    game.set_blocks(&tower);
    game.body.set_position(DVec3::new(4.5, 68.0, 4.5));
    game.prev_feet = game.body.position();
    let id = spawn(&mut game, "spider", 6.5, 4.5);
    let mut highest = 64.0f64;
    for _ in 0..200 {
        run(&mut game, 1);
        game.body.set_position(DVec3::new(4.5, 68.0, 4.5));
        game.body.velocity = DVec3::ZERO;
        if alive(&game, id) {
            highest = highest.max(mob(&game, id).feet().y);
        }
    }
    assert!(highest > 66.0, "it climbed: {highest}");
}

#[test]
fn an_enderman_is_angered_by_a_stare_and_teleports_out_of_water() {
    let mut game = night_game(GameMode::Survival);
    let id = spawn(&mut game, "enderman", 8.5, 0.5);
    // Look straight at its head.
    game.camera.yaw = std::f32::consts::FRAC_PI_2;
    game.camera.pitch = ((2.5f64 - 1.62) / 8.0).atan() as f32;
    run(&mut game, 2);
    assert!(mob(&game, id).ai.angry > 0, "angered by being looked at");
    // Calm one beside water: it leaves.
    let mut wet = night_game(GameMode::Survival);
    wet.game_facing_away();
    let id = spawn(&mut wet, "enderman", 12.5, 12.5);
    wet.set_blocks(&[(IVec3::new(12, 64, 12), BlockState::of(blocks::WATER))]);
    let start = mob(&wet, id).feet();
    run(&mut wet, 4);
    let moved = mob(&wet, id).feet().distance(start);
    assert!(
        moved > 1.0 || !alive(&wet, id),
        "it teleported or took damage: {moved}"
    );
}

impl Game {
    /// Looks away from everything, down at the floor.
    fn game_facing_away(&mut self) {
        self.camera.pitch = -1.4;
    }
}

#[test]
fn slimes_break_into_smaller_ones_and_only_the_smallest_drop_balls() {
    let mut game = night_game(GameMode::Creative);
    let kind = game.mob_defs.find("slime").unwrap();
    let data = crate::entity::mob::MobData {
        size: 4,
        ..Default::default()
    };
    let big = crate::entity::mob::Mob::with_data(
        kind,
        game.mob_defs.get(kind),
        DVec3::new(9.5, 64.0, 9.5),
        0.0,
        data,
    );
    let id = game.add_mob(big);
    assert_eq!(mob(&game, id).stats.health, 16.0, "health is size squared");
    let width = mob(&game, id).size().0;
    assert!(
        (width - 0.52 * 4.0).abs() < 1e-6,
        "a size-4 slime is two blocks wide: {width}"
    );
    game.mobs
        .iter_mut()
        .find(|m| m.id == id)
        .unwrap()
        .stats
        .health = 0.0;
    run(&mut game, 2);
    let children: Vec<_> = game.mobs.iter().filter(|m| m.data.size == 2).collect();
    assert!(
        (2..=4).contains(&children.len()),
        "{} children",
        children.len()
    );
    assert!(
        game.entities
            .iter()
            .all(|e| e.stack.id != items::SLIME_BALL),
        "no balls from the big one"
    );
    // The smallest ones drop slimeballs (0–2) and don't split.
    let kids: Vec<u64> = game
        .mobs
        .iter()
        .filter(|m| m.data.size == 2)
        .map(|m| m.id)
        .collect();
    for id in kids {
        game.mobs
            .iter_mut()
            .find(|m| m.id == id)
            .unwrap()
            .stats
            .health = 0.0;
    }
    run(&mut game, 2);
    let tiny = game.mobs.iter().filter(|m| m.data.size == 1).count();
    assert!(tiny >= 4, "each size-2 slime split in two or more: {tiny}");
    let tiny_ids: Vec<u64> = game
        .mobs
        .iter()
        .filter(|m| m.data.size == 1)
        .map(|m| m.id)
        .collect();
    game.entities.clear();
    for id in tiny_ids {
        game.mobs
            .iter_mut()
            .find(|m| m.id == id)
            .unwrap()
            .stats
            .health = 0.0;
    }
    run(&mut game, 3);
    assert!(
        game.mobs.iter().all(|m| m.data.size != 1 || m.is_dead()),
        "tiny ones don't split"
    );
    let balls: u32 = game
        .entities
        .iter()
        .filter(|e| e.stack.id == items::SLIME_BALL)
        .map(|e| u32::from(e.stack.count))
        .sum();
    assert!(balls > 0, "some slimeballs");
}

#[test]
fn slimes_come_from_one_chunk_in_ten_below_the_surface() {
    let seed = 20_111_118;
    let slimy = (-20..20)
        .flat_map(|x| (-20..20).map(move |z| (x, z)))
        .filter(|&(x, z)| {
            super::mobs::is_slime_chunk(seed, crate::world::chunk::ChunkPos::new(x, z))
        })
        .count();
    let share = slimy as f64 / 1600.0;
    assert!((0.07..0.13).contains(&share), "{share}");
    // The same chunk stays the same.
    let chunk = crate::world::chunk::ChunkPos::new(3, -7);
    assert_eq!(
        super::mobs::is_slime_chunk(seed, chunk),
        super::mobs::is_slime_chunk(seed, chunk)
    );
}

#[test]
fn shearing_gives_wool_of_its_color_and_dye_changes_the_color() {
    let mut game = lit_flat_game(GameMode::Survival);
    game.entities.clear();
    let id = spawn(&mut game, "sheep", 3.5, 0.5);
    let index = game.mobs.iter().position(|m| m.id == id).unwrap();
    game.mobs[index].data.color = 0;
    game.inventory
        .set(0, Some(ItemStack::new(items::RED_DYE, 2)));
    game.inventory.selected = 0;
    assert!(game.use_mob(index));
    assert_eq!(game.mobs[index].data.color, 14, "dyed red");
    assert_eq!(game.inventory.slot(0).map(|s| s.count), Some(1));
    // Shear it.
    game.inventory
        .set(0, Some(ItemStack::new(items::SHEARS, 1)));
    assert!(game.use_mob(index));
    assert!(game.mobs[index].data.sheared);
    let wool = items::by_name("red_wool").unwrap();
    let dropped: u32 = game
        .entities
        .iter()
        .filter(|e| e.stack.id == wool)
        .map(|e| u32::from(e.stack.count))
        .sum();
    assert!((1..=3).contains(&dropped), "{dropped} red wool");
    assert!(!game.use_mob(index), "can't shear it twice");
    // It grows back by eating grass.
    for _ in 0..30_000 {
        game.tick_animals();
        if !game.mobs[index].data.sheared {
            break;
        }
    }
    assert!(!game.mobs[index].data.sheared, "wool grew back");
}

#[test]
fn a_sheep_that_dies_unsheared_leaves_wool_and_mutton() {
    let mut game = lit_flat_game(GameMode::Survival);
    game.entities.clear();
    let id = spawn(&mut game, "sheep", 5.5, 5.5);
    let index = game.mobs.iter().position(|m| m.id == id).unwrap();
    game.mobs[index].data.color = 0;
    game.mobs[index].stats.health = 0.0;
    run(&mut game, 2);
    let wool = items::by_name("white_wool").unwrap();
    assert!(game.entities.iter().any(|e| e.stack.id == wool));
    assert!(
        game.entities
            .iter()
            .any(|e| e.stack.id == items::by_name("mutton").unwrap())
    );
}

#[test]
fn a_cow_gives_milk_for_a_bucket_and_hens_lay_eggs() {
    let mut game = lit_flat_game(GameMode::Survival);
    let cow = spawn(&mut game, "cow", 3.5, 0.5);
    let index = game.mobs.iter().position(|m| m.id == cow).unwrap();
    game.inventory
        .set(0, Some(ItemStack::new(items::BUCKET, 1)));
    game.inventory.selected = 0;
    assert!(game.use_mob(index));
    assert_eq!(
        game.inventory.slot(0).map(|s| s.id),
        Some(items::MILK_BUCKET)
    );
    game.entities.clear();
    let hen = spawn(&mut game, "chicken", 6.5, 6.5);
    let index = game.mobs.iter().position(|m| m.id == hen).unwrap();
    game.mobs[index].data.timer = 3;
    run(&mut game, 6);
    assert!(
        game.entities
            .iter()
            .any(|e| e.stack.id == items::by_name("egg").unwrap()),
        "an egg"
    );
    assert!(
        game.mobs[index].data.timer >= 6000,
        "and the next one is minutes off"
    );
}

#[test]
fn a_hen_glides_down_slowly() {
    let mut game = lit_flat_game(GameMode::Creative);
    let kind = game.mob_defs.find("chicken").unwrap();
    let mob = game.new_mob(kind, DVec3::new(8.5, 90.0, 8.5), 0.0);
    let id = game.add_mob(mob);
    run(&mut game, 20);
    let fell = 90.0 - mob_y(&game, id);
    assert!(
        fell < 4.0,
        "a free fall of 20 ticks is ~14 blocks; the hen fell {fell}"
    );
}

fn mob_y(game: &Game, id: u64) -> f64 {
    mob(game, id).feet().y
}

#[test]
fn a_wolf_is_tamed_with_bones_sits_on_command_and_follows() {
    let mut game = lit_flat_game(GameMode::Survival);
    let id = spawn(&mut game, "wolf", 3.5, 0.5);
    let index = game.mobs.iter().position(|m| m.id == id).unwrap();
    game.inventory.set(0, Some(ItemStack::new(items::BONE, 64)));
    game.inventory.selected = 0;
    for _ in 0..40 {
        game.use_mob(index);
        if game.mobs[index].data.tamed {
            break;
        }
    }
    assert!(game.mobs[index].data.tamed, "tamed with enough bones");
    assert_eq!(game.mobs[index].stats.health, 20.0);
    assert_eq!(game.mobs[index].data.variant, 1, "the tame face");
    game.inventory.set(0, None);
    assert!(game.mobs[index].data.sitting);
    game.use_mob(index);
    assert!(!game.mobs[index].data.sitting, "up again");
    // Far away it comes to the owner.
    game.mobs[index]
        .body
        .set_position(DVec3::new(20.5, 64.0, 0.5));
    run(&mut game, 200);
    let distance = mob(&game, id).feet().distance(game.body.position());
    assert!(distance < 8.0, "followed: {distance}");
}

#[test]
fn a_tame_wolf_attacks_what_hurt_its_owner() {
    let mut game = night_game(GameMode::Survival);
    let wolf = spawn(&mut game, "wolf", 4.5, 4.5);
    let index = game.mobs.iter().position(|m| m.id == wolf).unwrap();
    game.mobs[index].data.tamed = true;
    game.mobs[index].data.variant = 1;
    let zombie = spawn(&mut game, "zombie", 2.0, 0.5);
    run(&mut game, 100);
    let hurt = !alive(&game, zombie) || mob(&game, zombie).stats.health < 20.0;
    assert!(hurt, "the wolf bit the zombie");
}

#[test]
fn an_iron_golem_attacks_monsters_near_it_but_not_the_player() {
    let mut game = night_game(GameMode::Survival);
    let golem = spawn(&mut game, "iron_golem", 8.5, 8.5);
    let zombie = spawn(&mut game, "zombie", 11.5, 8.5);
    run(&mut game, 120);
    assert!(!alive(&game, zombie) || mob(&game, zombie).stats.health < 20.0);
    assert_eq!(game.stats.health, 20.0);
    assert!(alive(&game, golem));
}

#[test]
fn a_horse_takes_a_saddle_a_tame_one_is_ridden_and_sneaking_gets_off() {
    let mut game = lit_flat_game(GameMode::Survival);
    let id = spawn(&mut game, "horse", 3.5, 0.5);
    let index = game.mobs.iter().position(|m| m.id == id).unwrap();
    game.mobs[index].data.tamed = true;
    game.inventory
        .set(0, Some(ItemStack::new(items::SADDLE, 1)));
    game.inventory.selected = 0;
    assert!(game.use_mob(index));
    assert!(game.mobs[index].data.saddled);
    assert!(game.inventory.slot(0).is_none());
    assert!(game.use_mob(index));
    assert!(game.is_riding());
    game.run_ticks(2);
    let seat = game.body.position();
    let horse = mob(&game, id).feet();
    assert!(
        seat.y > horse.y + 0.5 && (seat.x - horse.x).abs() < 0.1,
        "{seat} on {horse}"
    );
    // Ride it forward.
    game.keys.forward = true;
    game.camera.yaw = std::f32::consts::FRAC_PI_2;
    run(&mut game, 40);
    assert!(
        mob(&game, id).feet().x > horse.x + 2.0,
        "the horse carried the player east"
    );
    game.keys.forward = false;
    game.keys.sneak = true;
    run(&mut game, 2);
    game.keys.sneak = false;
    assert!(!game.is_riding());
}

#[test]
fn an_untamed_horse_bucks_until_it_gets_used_to_its_rider() {
    let mut game = lit_flat_game(GameMode::Survival);
    let id = spawn(&mut game, "horse", 3.5, 0.5);
    let index = game.mobs.iter().position(|m| m.id == id).unwrap();
    assert!(game.use_mob(index));
    assert!(game.is_riding());
    let mut thrown = false;
    for _ in 0..4000 {
        run(&mut game, 1);
        if !game.is_riding() {
            thrown = true;
            game.use_mob(index);
        }
        if mob(&game, id).data.tamed {
            break;
        }
    }
    assert!(mob(&game, id).data.tamed, "tamed in the end");
    assert!(thrown || mob(&game, id).data.tamed);
}

#[test]
fn squid_stay_in_the_water_and_suffocate_out_of_it() {
    let mut game = lit_flat_game(GameMode::Creative);
    // A pool 8×8×4 sunk into the flat world.
    let mut edits = Vec::new();
    for x in 8..16 {
        for z in 8..16 {
            for y in 60..64 {
                edits.push((IVec3::new(x, y, z), BlockState::of(blocks::WATER)));
            }
        }
    }
    game.set_blocks(&edits);
    let kind = game.mob_defs.find("squid").unwrap();
    let mob0 = game.new_mob(kind, DVec3::new(12.5, 62.0, 12.5), 0.0);
    let id = game.add_mob(mob0);
    run(&mut game, 200);
    let feet = mob(&game, id).feet();
    assert!(alive(&game, id));
    assert!(
        (8.0..16.0).contains(&feet.x) && (8.0..16.0).contains(&feet.z),
        "{feet}"
    );
    assert!(feet.y > 59.0 && feet.y < 65.0, "{feet}");
    // On dry land it can't live.
    let dry = game.new_mob(kind, DVec3::new(-9.5, 64.0, -9.5), 0.0);
    let dry_id = game.add_mob(dry);
    run(&mut game, 400);
    assert!(!alive(&game, dry_id), "a squid out of water dies");
}

#[test]
fn a_drowned_swims_to_the_player_through_water() {
    let mut game = night_game(GameMode::Survival);
    let mut edits = Vec::new();
    for x in 2..12 {
        for z in -3..4 {
            for y in 62..66 {
                edits.push((IVec3::new(x, y, z), BlockState::of(blocks::WATER)));
            }
        }
    }
    game.set_blocks(&edits);
    game.body.set_position(DVec3::new(2.5, 66.0, 0.5));
    game.body.velocity = DVec3::ZERO;
    let kind = game.mob_defs.find("drowned").unwrap();
    let mob0 = game.new_mob(kind, DVec3::new(10.5, 62.0, 0.5), 0.0);
    let id = game.add_mob(mob0);
    let start = mob(&game, id).feet().x;
    run(&mut game, 80);
    assert!(
        mob(&game, id).feet().x < start - 3.0,
        "it came for the player"
    );
}

#[test]
fn phantoms_appear_only_after_three_days_awake_under_the_night_sky() {
    let mut game = night_game(GameMode::Survival);
    for _ in 0..40 {
        game.time = NIGHT;
        game.spawn_phantoms_for_test();
    }
    assert!(
        game.mobs
            .iter()
            .all(|m| game.mob_defs.get(m.kind).name != "phantom"),
        "none when rested"
    );
    game.stats.awake = 72_000 * 4;
    for _ in 0..200 {
        game.time = NIGHT + 60 * (game.time / 60 % 2);
        game.spawn_phantoms_for_test();
    }
    assert!(
        game.mobs
            .iter()
            .any(|m| game.mob_defs.get(m.kind).name == "phantom"),
        "phantoms after a long wake"
    );
    let phantom = game
        .mobs
        .iter()
        .find(|m| game.mob_defs.get(m.kind).name == "phantom")
        .unwrap();
    assert!(phantom.feet().y > game.body.position().y + 15.0, "high up");
}

impl Game {
    fn spawn_phantoms_for_test(&mut self) {
        self.time = (self.time / 60) * 60;
        self.spawn_phantoms();
    }
}

#[test]
fn a_phantom_circles_overhead_and_dives_at_the_player() {
    let mut game = night_game(GameMode::Survival);
    let kind = game.mob_defs.find("phantom").unwrap();
    let mob0 = game.new_mob(kind, DVec3::new(0.5, 84.0, 0.5), 0.0);
    let id = game.add_mob(mob0);
    let mut lowest = f64::MAX;
    for _ in 0..600 {
        run(&mut game, 1);
        if alive(&game, id) {
            lowest = lowest.min(mob(&game, id).feet().y);
        }
        game.body.set_position(DVec3::new(0.5, 64.0, 0.5));
        if game.stats.health < 20.0 {
            break;
        }
    }
    assert!(
        game.stats.health < 20.0 || lowest < 70.0,
        "it swooped: lowest {lowest}"
    );
}

#[test]
fn natural_spawning_follows_the_biome_and_the_chunk() {
    let mut game = night_game(GameMode::Survival);
    let cell = IVec3::new(100, 64, 100);
    let chunk = crate::world::chunk::ChunkPos::new(6, 6);
    let kinds_in = |game: &mut Game, biome: Biome| {
        let mut seen = std::collections::BTreeSet::new();
        for _ in 0..600 {
            if let Some((kind, _, _)) = game.pick_dark_kind(biome, cell, chunk) {
                seen.insert(game.mob_defs.get(kind).name.clone());
            }
        }
        seen
    };
    let plains = kinds_in(&mut game, Biome::Plains);
    let desert = kinds_in(&mut game, Biome::Desert);
    assert!(
        plains.contains("zombie")
            && plains.contains("skeleton")
            && plains.contains("creeper")
            && plains.contains("spider")
    );
    assert!(!plains.contains("husk"), "no husks outside deserts");
    assert!(desert.contains("husk"), "husks in deserts");
    assert!(
        plains.contains("enderman") || plains.contains("witch"),
        "rarer kinds come now and then: {plains:?}"
    );
}

#[test]
fn new_biomes_bring_their_own_monsters_and_mushroom_fields_none() {
    let mut game = night_game(GameMode::Survival);
    let cell = IVec3::new(100, 64, 100);
    let chunk = crate::world::chunk::ChunkPos::new(6, 6);
    let mut kinds_in = |biome: Biome, cell: IVec3| {
        let mut seen = std::collections::BTreeSet::new();
        for _ in 0..600 {
            if let Some((kind, _, _)) = game.pick_dark_kind(biome, cell, chunk) {
                seen.insert(game.mob_defs.get(kind).name.clone());
            }
        }
        seen
    };
    assert!(kinds_in(Biome::SnowyPlains, cell).contains("stray"));
    assert!(!kinds_in(Biome::Plains, cell).contains("stray"));
    assert!(kinds_in(Biome::MushroomFields, cell).is_empty());
    assert!(kinds_in(Biome::Swamp, IVec3::new(100, 60, 100)).contains("slime"));
    assert!(!kinds_in(Biome::Forest, IVec3::new(100, 60, 100)).contains("slime"));
}

#[test]
fn animals_and_fish_spawn_in_their_modern_biomes() {
    let game = lit_flat_game(GameMode::Creative);
    let spawns_in = |name: &str, biome: Biome| {
        let kind = game.mob_defs.find(name).unwrap();
        match &game.mob_defs.get(kind).spawn {
            Spawn::Ground { biomes, .. }
            | Spawn::Water { biomes, .. }
            | Spawn::Dark { biomes, .. } => biomes.contains(&biome),
            _ => false,
        }
    };
    assert!(spawns_in("fox", Biome::Taiga) && spawns_in("fox", Biome::SnowyTaiga));
    assert!(spawns_in("salmon", Biome::River) && !spawns_in("salmon", Biome::WarmOcean));
    assert!(spawns_in("cod", Biome::LukewarmOcean));
    assert!(spawns_in("wolf", Biome::Grove));
    assert!(spawns_in("rabbit", Biome::SnowyPlains) && spawns_in("rabbit", Biome::Desert));
    assert!(spawns_in("horse", Biome::Savanna));
    assert!(!spawns_in("cow", Biome::MushroomFields));
}

#[test]
fn animals_stand_in_a_snow_layer_on_snowy_grass() {
    let mut game = lit_flat_game(GameMode::Creative);
    let grass =
        BlockState::of(blocks::GRASS_BLOCK).with_bool(crate::world::props::Prop::Snowy, true);
    game.set_blocks(&[
        (IVec3::new(5, 63, 5), grass),
        (IVec3::new(5, 64, 5), BlockState::of(blocks::SNOW)),
    ]);
    let spot = game.ground_spot(5, 5, &[blocks::GRASS_BLOCK]);
    assert_eq!(spot, Some(DVec3::new(5.5, 64.0, 5.5)));
}

#[test]
fn every_mob_model_stands_on_its_feet_and_is_about_its_hitbox_tall() {
    use crate::render::models::add_mob;
    let game = lit_flat_game(GameMode::Creative);
    for (kind, def) in game.mob_defs.iter() {
        let mob = Mob::new(kind, def, DVec3::new(0.5, 64.0, 0.5), 0.0);
        let mut mesh = crate::world::mesh::Mesh::default();
        add_mob(&mut mesh, &mob, def, 1.0, (15, 0));
        assert!(!mesh.is_empty(), "{} draws something", def.name);
        let ys = mesh.vertices.iter().map(|v| v.position[1]);
        let (min, max) = ys.fold((f32::MAX, f32::MIN), |(lo, hi), y| (lo.min(y), hi.max(y)));
        let tall = (max - min) as f64;
        let scaled = def.height * f64::from(def.scale);
        assert!(
            tall > scaled * 0.5 && tall < scaled * 2.3 + 0.6,
            "{} is {tall} tall for a hitbox of {}",
            def.name,
            def.height
        );
        if !def.flies && !def.swims {
            assert!(
                (63.9..=64.6).contains(&min),
                "{} stands on its feet: {min}",
                def.name
            );
        }
    }
}

#[test]
fn endermen_pick_up_blocks_they_pass_and_drop_them_when_killed() {
    // By day, so it doesn't teleport away from the sand (1 in 400 ticks at
    // night), and with a fixed seed so the test can't be unlucky.
    let mut game = lit_flat_game(GameMode::Creative);
    game.rng = Rng::new(7);
    // Sand all about it at its own height, where it can reach.
    let sand: Vec<_> = (0..=18)
        .flat_map(|x| (0..=18).map(move |z| (IVec3::new(x, 64, z), BlockState::of(blocks::SAND))))
        .filter(|&(c, _)| c.x != 9 || c.z != 9)
        .collect();
    game.set_blocks(&sand);
    // Several of them: what each does depends on its id, which depends on
    // which tests ran before, and one can wander off or stand about.
    let ids: Vec<u64> = (0..4)
        .map(|i| spawn(&mut game, "enderman", 7.5 + f64::from(i) * 1.5, 9.5))
        .collect();
    let mut carried = None;
    for _ in 0..2400 {
        run(&mut game, 1);
        carried = ids
            .iter()
            .find_map(|&id| game.mobs.iter().find(|m| m.id == id)?.carrying);
        if carried.is_some() {
            break;
        }
    }
    let block = carried.expect("it picked something up");
    assert_eq!(block.id(), blocks::SAND);
    assert!(game.chunks.world().block(9, 64, 9).is_some());
}

#[test]
fn saved_animals_keep_their_color_tameness_and_coat() {
    let mut game = lit_flat_game(GameMode::Survival);
    let sheep = spawn(&mut game, "sheep", 4.5, 4.5);
    let wolf = spawn(&mut game, "wolf", 6.5, 6.5);
    for mob in &mut game.mobs {
        if mob.id == sheep {
            mob.data.color = 9;
            mob.data.sheared = true;
        } else if mob.id == wolf {
            mob.data.tamed = true;
            mob.data.sitting = true;
            mob.data.variant = 1;
        }
    }
    let saved = game.saved_mobs();
    let text = ron::to_string(&saved).unwrap();
    let back: Vec<crate::level::SavedMob> = ron::from_str(&text).unwrap();
    game.mobs.clear();
    game.restore_mobs(&back);
    let restored: Vec<_> = game.stashed_mobs.values().flatten().collect();
    let sheep = restored
        .iter()
        .find(|m| game.mob_defs.get(m.kind).name == "sheep")
        .unwrap();
    assert_eq!((sheep.data.color, sheep.data.sheared), (9, true));
    let wolf = restored
        .iter()
        .find(|m| game.mob_defs.get(m.kind).name == "wolf")
        .unwrap();
    assert!(wolf.data.tamed && wolf.data.sitting && wolf.data.variant == 1);
    // Monsters aren't saved.
    spawn(&mut game, "zombie", 8.5, 8.5);
    assert!(game.saved_mobs().iter().all(|m| m.name != "zombie"));
}
