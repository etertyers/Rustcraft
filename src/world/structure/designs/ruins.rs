//! The single-piece structures: the desert temple (a pyramid over a
//! trapped treasure room), the jungle temple, shipwrecks and ruined
//! portals.

use super::{b, stairs};
use crate::world::{
    Face,
    block::BlockState,
    structure::template::{Sketch, Template},
};

const AIR: BlockState = BlockState::AIR;

/// Fixed scatter, so the same design always comes out the same.
fn scatter(x: i32, y: i32, z: i32) -> u32 {
    let h = (x as u32).wrapping_mul(73_856_093)
        ^ (y as u32).wrapping_mul(19_349_663)
        ^ (z as u32).wrapping_mul(83_492_791);
    h.wrapping_mul(2_654_435_761) >> 16
}

/// 21 × 21, sunk 14 blocks: a treasure room with four chests and nine
/// TNT under a pressure plate, a shaft up to the floor of a sandstone
/// pyramid with two towers and a terracotta star round the hole.
fn desert_temple() -> Template {
    const G: i32 = 14; // ground level in the template
    let mut k = Sketch::new([21, G + 13, 21]);
    let sandstone = b("sandstone");
    let cut = b("cut_sandstone");
    let chiseled = b("chiseled_sandstone");
    // The treasure room.
    k.hollow([5, 2, 5], [15, 8, 15], cut);
    for (x, z, f) in [
        (10, 5, Face::South),
        (10, 15, Face::North),
        (5, 10, Face::East),
        (15, 10, Face::West),
    ] {
        k.chest(
            x,
            3,
            z,
            b(&format!("chest[facing={}]", super::facing_name(f))),
            "desert_pyramid",
        );
        k.set(x, 4, z, chiseled);
    }
    k.fill([9, 1, 9], [11, 1, 11], b("tnt"));
    k.set(10, 2, 10, sandstone);
    k.set(10, 3, 10, b("stone_pressure_plate"));
    // The shaft.
    for y in 8..=G {
        k.set(10, y, 10, AIR);
    }
    // The pyramid: a step in each layer, a hall inside, a tunnel in.
    for y in G..G + 12 {
        let inset = (y - G - 1).max(0);
        for z in 0..21 {
            for x in 0..21 {
                let inside = (inset..=20 - inset).contains(&x) && (inset..=20 - inset).contains(&z);
                if !inside {
                    continue;
                }
                let hall =
                    (4..=16).contains(&x) && (4..=16).contains(&z) && (G + 1..=G + 5).contains(&y);
                let tunnel = (9..=11).contains(&x) && z < 6 && (G + 1..=G + 3).contains(&y);
                let block = if y == G {
                    sandstone
                } else if hall || tunnel {
                    AIR
                } else if (y - G) % 4 == 0 {
                    cut
                } else {
                    sandstone
                };
                k.set(x, y, z, block);
            }
        }
    }
    // The star on the hall floor round the hole.
    for (dx, dz) in [
        (0, 2),
        (0, -2),
        (2, 0),
        (-2, 0),
        (1, 1),
        (1, -1),
        (-1, 1),
        (-1, -1),
    ] {
        k.set(10 + dx, G, 10 + dz, b("orange_terracotta"));
    }
    for (dx, dz) in [(0, 1), (0, -1), (1, 0), (-1, 0)] {
        k.set(10 + dx, G, 10 + dz, b("blue_terracotta"));
    }
    k.set(10, G, 10, AIR);
    // Towers at the front corners.
    for x0 in [0, 16] {
        for y in G + 1..G + 13 {
            for z in 0..5 {
                for x in x0..x0 + 5 {
                    let shell = x == x0 || x == x0 + 4 || z == 0 || z == 4;
                    let stripe = (y - G) % 3 == 0;
                    let block = match (shell, stripe) {
                        (true, true) => b("orange_terracotta"),
                        (true, false) => cut,
                        _ => AIR,
                    };
                    k.set(x, y, z, block);
                }
            }
        }
        k.fill([x0, G + 12, 0], [x0 + 4, G + 12, 4], chiseled);
    }
    // The way in.
    for y in G + 1..=G + 3 {
        for x in 9..=11 {
            k.set(x, y, 0, AIR);
        }
    }
    k.set(9, G + 4, 0, chiseled);
    k.set(11, G + 4, 0, chiseled);
    k.template()
}

/// 12 × 15: a mossy cobblestone temple in two storeys, two chests, and
/// dispensers of arrows that fire at whoever steps on the plate beside
/// them.
fn jungle_temple() -> Template {
    let mut k = Sketch::new([12, 12, 15]);
    let stone = |x: i32, y: i32, z: i32| {
        if scatter(x, y, z).is_multiple_of(3) {
            b("cobblestone")
        } else {
            b("mossy_cobblestone")
        }
    };
    for y in 0..=9 {
        let inset = if y >= 6 { 1 } else { 0 } + if y >= 9 { 1 } else { 0 };
        for z in inset..15 - inset {
            for x in inset..12 - inset {
                let edge = x == inset || x == 11 - inset || z == inset || z == 14 - inset;
                let floor = y == 0 || y == 5 || y == 9;
                let block = if edge || floor { stone(x, y, z) } else { AIR };
                k.set(x, y, z, block);
            }
        }
    }
    // Doorway, windows and the stairs up.
    for y in 1..=3 {
        for x in 5..=6 {
            k.set(x, y, 0, AIR);
        }
    }
    for z in [4, 10] {
        k.set(0, 3, z, AIR);
        k.set(11, 3, z, AIR);
    }
    for (i, z) in (8..=11).enumerate() {
        k.set(
            9,
            1 + i as i32,
            z,
            stairs("cobblestone_stairs", Face::South, false),
        );
        k.set(9, 5, z, AIR);
    }
    k.set(9, 5, 12, AIR);
    // Chiseled stone bricks over the door.
    for x in 4..=7 {
        k.set(x, 4, 0, b("chiseled_stone_bricks"));
    }
    // Chests downstairs at the back and upstairs.
    k.chest(2, 1, 13, b("chest[facing=north]"), "jungle_temple");
    k.chest(5, 6, 12, b("chest[facing=north]"), "jungle_temple");
    // Arrow traps along the hall.
    for z in [5, 9] {
        k.chest(
            1,
            1,
            z,
            b("dispenser[facing=east]"),
            "jungle_temple_dispenser",
        );
        k.set(2, 1, z, b("stone_pressure_plate"));
    }
    // Vines down the front.
    for x in [1, 3, 8, 10] {
        for y in 4..=8 {
            if scatter(x, y, 0).is_multiple_of(2) {
                k.set(x, y, 0, stone(x, y, 0));
            }
        }
    }
    k.template()
}

/// A sailing ship lying on the sea floor, 7 wide and 21 long: hold,
/// deck, mast and a cabin at the stern, with supply, map and treasure
/// chests. Inside is left as the world has it, water or sand.
fn shipwreck(broken: bool) -> Template {
    let mut k = Sketch::new([7, 13, 21]);
    let planks = b("spruce_planks");
    let side = b("oak_planks");
    let rail = b("spruce_fence");
    for z in 0..21 {
        let half = match z {
            0 => 0,
            1 | 2 => 1,
            3 | 4 => 2,
            18..=20 => 2,
            _ => 3,
        };
        // A broken ship has lost its bow.
        if broken && z < 5 {
            continue;
        }
        for x in 3 - half..=3 + half {
            k.set(x, 0, z, planks);
            for y in 1..=3 {
                if x == 3 - half || x == 3 + half || z == 0 || z == 20 {
                    k.set(x, y, z, side);
                }
            }
            let hatch = x == 3 && (7..=8).contains(&z);
            if !hatch {
                k.set(x, 4, z, planks);
            }
            if x == 3 - half || x == 3 + half {
                k.set(x, 5, z, rail);
            }
        }
    }
    // The mast.
    for y in 5..=12 {
        k.set(3, y, 10, b("spruce_log[axis=y]"));
    }
    for x in 1..=5 {
        k.set(x, 10, 10, b("spruce_log[axis=x]"));
    }
    // The cabin at the stern.
    for y in 5..=7 {
        for z in 15..=19 {
            for x in 1..=5 {
                let wall = x == 1 || x == 5 || z == 15 || z == 19;
                k.set(x, y, z, if wall { side } else { AIR });
            }
        }
    }
    k.fill([1, 8, 15], [5, 8, 19], planks);
    k.door(3, 5, 15, "spruce_door", Face::North);
    k.chest(2, 5, 18, b("chest[facing=east]"), "shipwreck_treasure");
    k.chest(4, 5, 18, b("chest[facing=west]"), "shipwreck_map");
    let supply_z = if broken { 6 } else { 4 };
    k.chest(3, 1, supply_z, b("chest[facing=south]"), "shipwreck_supply");
    k.template()
}

/// A ruined nether portal: an obsidian frame (some of it crying, some
/// gone) standing or fallen, netherrack and gold about, and a chest.
fn ruined_portal(variant: u32) -> Template {
    let mut k = Sketch::new([8, 7, 7]);
    for z in 0..7 {
        for x in 0..8 {
            let roll = scatter(x, variant as i32, z) % 10;
            let block = match roll {
                0..=3 => Some(b("netherrack")),
                4 => Some(b("stone_bricks")),
                5 => Some(b("cracked_stone_bricks")),
                _ => None,
            };
            if let Some(block) = block {
                k.set(x, 0, z, block);
            }
        }
    }
    let frame: Vec<(i32, i32)> = (0..4)
        .flat_map(|x| [(x, 0), (x, 4)])
        .chain((1..4).flat_map(|y| [(0, y), (3, y)]))
        .collect();
    for (i, &(fx, fy)) in frame.iter().enumerate() {
        let roll = scatter(fx, fy, variant as i32 + i as i32) % 10;
        if roll < 2 {
            continue;
        }
        let block = if roll < 4 {
            b("crying_obsidian")
        } else {
            b("obsidian")
        };
        if variant == 1 {
            // Fallen over, lying on the ground.
            k.set(2 + fx, 1, 1 + fy, block);
        } else {
            k.set(2 + fx, 1 + fy, 3, block);
        }
    }
    k.set(1, 1, 1, b("gold_block"));
    k.chest(6, 1, 5, b("chest[facing=west]"), "ruined_portal");
    k.template()
}

pub fn all() -> Vec<(String, Template)> {
    let mut all = vec![
        ("desert_temple/desert_temple".to_owned(), desert_temple()),
        ("jungle_temple/jungle_temple".to_owned(), jungle_temple()),
        ("shipwreck/upright".to_owned(), shipwreck(false)),
        ("shipwreck/broken".to_owned(), shipwreck(true)),
        ("shipwreck_beached/upright".to_owned(), shipwreck(false)),
    ];
    for variant in 0..3 {
        all.push((
            format!("ruined_portal/portal_{variant}"),
            ruined_portal(variant),
        ));
    }
    all
}
