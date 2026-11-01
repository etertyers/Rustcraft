//! M49's `/locate`: structures in every dimension, found from the
//! generators' own placement rule.

use glam::DVec3;

use super::locate::{kinds_for, nearest, structure_names};
use crate::world::{
    chunk::ChunkPos,
    dimension::Dimension,
    structure::{self, Kind},
    worldgen::{end::End, nether::Nether, terrain::Terrain},
};

const SEED: u64 = 12345;

/// Three spread-out spots to search from.
const FROM: [DVec3; 3] = [
    DVec3::new(0.0, 64.0, 0.0),
    DVec3::new(-1500.0, 64.0, 900.0),
    DVec3::new(2200.0, 64.0, -1700.0),
];

#[test]
fn every_structure_id_and_tag_is_offered_and_means_something() {
    let names = structure_names();
    for id in [
        "village",
        "mineshaft",
        "fortress",
        "bastion_remnant",
        "end_city",
        "stronghold",
    ] {
        assert!(names.contains(&id.to_owned()), "{id}");
    }
    for tag in [
        "#village",
        "#mineshaft",
        "#shipwreck",
        "#ocean_ruin",
        "#ruined_portal",
        "#on_treasure_maps",
    ] {
        assert!(names.contains(&tag.to_owned()), "{tag}");
    }
    assert!(
        kinds_for("#village", Dimension::Overworld)
            .unwrap()
            .contains(&Kind::Village)
    );
    assert!(
        kinds_for("minecraft:fortress", Dimension::Nether)
            .unwrap()
            .contains(&Kind::Fortress)
    );
    // An id the other dimension doesn't have: nothing to find, not an error.
    assert_eq!(kinds_for("village", Dimension::Nether).unwrap(), vec![]);
    assert_eq!(kinds_for("fortress", Dimension::Overworld).unwrap(), vec![]);
    assert_eq!(kinds_for("end_city", Dimension::Overworld).unwrap(), vec![]);
    // Vanilla's, but not Rustcraft's.
    assert_eq!(kinds_for("igloo", Dimension::Overworld).unwrap(), vec![]);
    assert_eq!(
        kinds_for("#ocean_ruin", Dimension::Overworld).unwrap(),
        vec![]
    );
    // Neither's.
    assert!(kinds_for("not_a_structure", Dimension::Overworld).is_err());
    assert!(kinds_for("#nothing", Dimension::Overworld).is_err());
}

/// Whether the generator, asked about the chunk where `/locate` says a
/// structure starts, starts one there.
fn generator_starts_it(dim: Dimension, kind: Kind, start: ChunkPos) -> bool {
    match dim {
        Dimension::Overworld => {
            kind.starts_near(SEED, start).contains(&start)
                && structure::structure_at(&Terrain::new(SEED), kind, start).is_some()
        }
        Dimension::Nether => {
            let nether = Nether::new(SEED);
            structure::nether::starts_within(&nether, kind, start, 1).contains(&start)
                && structure::nether::structure_at(&nether, kind, start).is_some()
        }
        Dimension::End => {
            let end = End::new(SEED);
            structure::end::starts_within(&end, start, 1).contains(&start)
                && structure::end::structure_at(&end, start).is_some()
        }
    }
}

#[test]
fn locate_finds_a_start_the_generator_makes_for_each_structure_in_each_dimension() {
    // Deserts are rare: the temples are searched for from where there are some.
    let desert = [
        DVec3::new(-3000.0, 64.0, 3000.0),
        DVec3::new(-6000.0, 64.0, 3000.0),
        DVec3::new(-3400.0, 64.0, 2800.0),
    ];
    let cases: &[(Dimension, &str)] = &[
        (Dimension::Overworld, "village"),
        (Dimension::Overworld, "desert_pyramid"),
        (Dimension::Overworld, "shipwreck"),
        (Dimension::Overworld, "ruined_portal"),
        (Dimension::Overworld, "mineshaft"),
        (Dimension::Overworld, "stronghold"),
        (Dimension::Nether, "fortress"),
        (Dimension::Nether, "bastion_remnant"),
        (Dimension::Nether, "ruined_portal"),
        (Dimension::End, "end_city"),
    ];
    for &(dim, id) in cases {
        let kinds = kinds_for(id, dim).unwrap();
        let mut found = 0;
        let places = if id == "desert_pyramid" { desert } else { FROM };
        for from in places {
            if let Some((kind, start)) = nearest(SEED, dim, &kinds, from) {
                found += 1;
                assert!(
                    generator_starts_it(dim, kind, start),
                    "{dim:?} {id} at {start:?}"
                );
                let (x, z) = start.min_block();
                let away = (f64::from(x) - from.x).hypot(f64::from(z) - from.z);
                assert!(away <= 100.0 * 16.0 * 1.5 + 16.0, "{id}: {away} blocks");
            }
        }
        assert!(found >= 2, "{dim:?} {id} found from {found} of 3 places");
    }
}

#[test]
fn the_nearest_is_the_nearest() {
    let kinds = kinds_for("village", Dimension::Overworld).unwrap();
    let from = FROM[1];
    let (_, start) = nearest(SEED, Dimension::Overworld, &kinds, from).unwrap();
    let chunk = ChunkPos::of_block(from.x as i32, from.z as i32);
    let d = |p: ChunkPos| {
        let (x, z) = p.min_block();
        (f64::from(x) + 8.0 - from.x).hypot(f64::from(z) + 8.0 - from.z)
    };
    // No other made village's start is nearer.
    let terrain = Terrain::new(SEED);
    for dz in -40..=40 {
        for dx in -40..=40 {
            for other in Kind::Village.starts_near(SEED, chunk.offset(dx * 15, dz * 15)) {
                if d(other) + 1.0 < d(start) {
                    assert!(
                        structure::structure_at(&terrain, Kind::Village, other).is_none(),
                        "{other:?} is nearer than {start:?}"
                    );
                }
            }
        }
    }
}

fn terrain_game() -> crate::game::Game {
    use std::time::{Duration, Instant};

    use crate::world::manager::{ChunkManager, ChunkSource};
    let (items, models) = crate::game::tests::registries();
    let chunks = ChunkManager::new(
        ChunkSource::Terrain(Box::new(Terrain::new(SEED))),
        models,
        1,
        2,
    );
    let feet = DVec3::new(0.5, 90.0, 0.5);
    let mut game = crate::game::Game::new(chunks, items, feet, feet, 0.0, 0.0)
        .with_mobs(crate::game::tests::mob_registry());
    let deadline = Instant::now() + Duration::from_secs(60);
    while game.chunks.world().len() < 9 {
        assert!(Instant::now() < deadline, "chunks never loaded");
        game.chunks.update(ChunkPos::new(0, 0));
        std::thread::sleep(Duration::from_millis(1));
    }
    game
}

#[test]
fn the_command_answers_with_the_vanilla_wording_clickable_and_fails_for_the_wrong_dimension() {
    use super::tests::run;
    use crate::game::commands::Reply;
    let mut game = terrain_game();
    let told = run(&mut game, "locate structure village");
    match &told[..] {
        [
            Reply::Rich {
                to: 0, text, click, ..
            },
        ] => {
            assert!(text.starts_with("The nearest village is at §a["), "{text}");
            assert!(
                text.contains(", ~, ") && text.ends_with(" blocks away)"),
                "{text}"
            );
            let (action, value) = click.as_ref().expect("clickable with cheats on");
            assert_eq!(action, "suggest_command");
            assert!(value.starts_with("/tp @s "), "{value}");
        }
        other => panic!("{other:?}"),
    }
    // A tag works the same.
    let told = run(&mut game, "locate structure #village");
    assert!(
        matches!(&told[..], [Reply::Rich { text, .. }] if text.starts_with("The nearest #village is at"))
    );
    // The Overworld's id in the Nether, and the Nether's in the Overworld:
    // vanilla's failure.
    for line in [
        "execute in minecraft:the_nether run locate structure village",
        "locate structure fortress",
        "locate structure end_city",
        "locate structure igloo",
    ] {
        let told = run(&mut game, line);
        let said = super::tests::said(&told);
        assert!(
            said.contains("Could not find a structure of type \"") && said.contains("\" nearby"),
            "{line}: {said}"
        );
    }
    let said = super::tests::said(&run(&mut game, "locate structure nonsense"));
    assert!(said.contains("Unknown structure"), "{said}");
}

/// Prints the table the PR shows (`cargo test … locate_table -- --ignored --nocapture`).
#[test]
#[ignore]
fn locate_table() {
    let rows: &[(Dimension, &str)] = &[
        (Dimension::Overworld, "village"),
        (Dimension::Overworld, "desert_pyramid"),
        (Dimension::Overworld, "jungle_pyramid"),
        (Dimension::Overworld, "shipwreck"),
        (Dimension::Overworld, "ruined_portal"),
        (Dimension::Overworld, "mineshaft"),
        (Dimension::Overworld, "stronghold"),
        (Dimension::Nether, "fortress"),
        (Dimension::Nether, "bastion_remnant"),
        (Dimension::Nether, "ruined_portal"),
        (Dimension::End, "end_city"),
    ];
    for &(dim, id) in rows {
        let kinds = kinds_for(id, dim).unwrap();
        let from = if id.ends_with("pyramid") {
            DVec3::new(-3000.0, 64.0, 3000.0)
        } else {
            FROM[0]
        };
        match nearest(SEED, dim, &kinds, from) {
            Some((kind, start)) => {
                let (x, z) = start.min_block();
                let away = (f64::from(x + 8) - from.x).hypot(f64::from(z + 8) - from.z);
                println!(
                    "TABLE | {id} | {dim:?} | {} | {} |",
                    away.round() as i64,
                    if generator_starts_it(dim, kind, start) {
                        "yes"
                    } else {
                        "NO"
                    }
                );
            }
            None => println!("TABLE | {id} | {dim:?} | none within 100 chunks | n/a |"),
        }
    }
}
