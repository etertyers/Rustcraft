//! M52's `/locate`: the Nether's bastions by either name, and the
//! strongholds' placement.

use glam::DVec3;

use super::tests::{run, said};
use crate::{
    game::Game,
    world::{
        chunk::ChunkPos,
        dimension::Dimension,
        manager::{ChunkManager, ChunkSource},
        structure::{self, Kind},
        worldgen::nether::Nether,
    },
};

const SEED: u64 = 12345;

/// A game in the Nether on seed 12345 (no chunks needed to locate).
pub(super) fn nether_game() -> Game {
    let (items, models) = crate::game::tests::registries();
    let chunks = ChunkManager::new(
        ChunkSource::Nether(Box::new(Nether::new(SEED))),
        models,
        1,
        1,
    );
    let feet = DVec3::new(0.5, 64.0, 0.5);
    let mut game = Game::new(chunks, items, feet, feet, 0.0, 0.0).starting_in(Dimension::Nether);
    game.mob_defs = crate::game::tests::mob_registry();
    game.player.name = "Kenzo".to_owned();
    game
}

/// The chunk `/locate` named in `reply` ("... is at [x, ~, z] ...").
fn located(reply: &str) -> Option<ChunkPos> {
    let open = reply.find("§a[")? + 4;
    let text = &reply[open..reply[open..].find(']')? + open];
    let mut parts = text.split(", ~, ");
    let x: i32 = parts.next()?.parse().ok()?;
    let z: i32 = parts.next()?.parse().ok()?;
    Some(ChunkPos::of_block(x - 8, z - 8))
}

#[test]
fn bastion_is_an_alias_of_bastion_remnant_and_always_points_at_a_real_bastion() {
    let mut game = nether_game();
    let nether = Nether::new(SEED);
    let mut state = 0x2545_F491_4F6C_DD1Du64;
    let mut next = |range: i32| {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1);
        ((state >> 33) as i32).rem_euclid(range) - range / 2
    };
    for _ in 0..20 {
        let (x, z) = (f64::from(next(8000)) + 0.5, f64::from(next(8000)) + 0.5);
        game.fly_to(DVec3::new(x, 64.0, z));
        let mut found = Vec::new();
        for id in ["bastion_remnant", "bastion"] {
            let told = said(&run(&mut game, &format!("locate structure {id}")));
            assert!(
                told.starts_with("The nearest bastion_remnant is at"),
                "{id} names the vanilla id: {told}"
            );
            let start = located(&told).unwrap_or_else(|| panic!("{told}"));
            assert!(
                structure::nether::starts_within(&nether, Kind::Bastion, start, 1).contains(&start)
                    && structure::nether::structure_at(&nether, Kind::Bastion, start).is_some(),
                "{id} from {x} {z}: no bastion starts at {start:?}"
            );
            found.push(told);
        }
        assert_eq!(found[0], found[1]);
    }
    // Tab completion offers the vanilla ids only.
    let names = super::locate::structure_names();
    assert!(names.contains(&"bastion_remnant".to_owned()));
    assert!(!names.contains(&"bastion".to_owned()));
    assert_eq!(
        super::locate::resolve("minecraft:bastion"),
        "bastion_remnant"
    );
    assert_eq!(super::locate::resolve("fortress"), "fortress");
}

/// Prints Step 0's /locate output
/// (`cargo test m52_step0 -- --ignored --nocapture`).
#[test]
#[ignore]
fn m52_step0() {
    let mut game = nether_game();
    let nether = Nether::new(SEED);
    for (x, z) in [
        (0.5, 0.5),
        (700.5, -300.5),
        (-1200.5, 900.5),
        (2500.5, 2500.5),
        (-3000.5, -2000.5),
    ] {
        game.fly_to(DVec3::new(x, 64.0, z));
        for id in ["bastion_remnant", "bastion"] {
            let told = said(&run(&mut game, &format!("locate structure {id}")));
            let real = located(&told).is_some_and(|start| {
                structure::nether::starts_within(&nether, Kind::Bastion, start, 1).contains(&start)
            });
            println!(
                "STEP0 | from {x:.0} {z:.0} | /locate structure {id} | {told} | bastion start there: {real}"
            );
        }
    }
    let positions = structure::stronghold::positions(SEED);
    println!("STEP0 | strongholds in the world: {}", positions.len());
    let terrain = crate::world::worldgen::terrain::Terrain::new(SEED);
    for (i, pos) in positions.iter().take(9).enumerate() {
        let (x, z) = pos.min_block();
        let (x, z) = (x + 8, z + 8);
        println!(
            "STEP0 | stronghold {} | {x} {z} | {:.0} blocks from 0,0 | {:?}",
            i + 1,
            f64::from(x).hypot(f64::from(z)),
            terrain.biome_at(x, z)
        );
    }
    let in_ocean = positions
        .iter()
        .filter(|p| {
            terrain
                .biome_at(p.min_block().0 + 8, p.min_block().1 + 8)
                .is_ocean()
        })
        .count();
    println!(
        "STEP0 | strongholds in an ocean: {in_ocean} of {}",
        positions.len()
    );
    let mut over = super::m49_tests::terrain_game();
    let told = said(&run(&mut over, "locate structure stronghold"));
    println!("STEP0 | from 0 0 | /locate structure stronghold | {told}");
}
