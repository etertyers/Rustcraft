//! Village pieces in the five styles: a meeting point (well or bell),
//! streets, houses, farms, a pen and the job-site buildings, laid out like
//! vanilla's and dressed in each biome's materials.
//!
//! Local coordinates: x east, z south, y up. Every building's door is in
//! its north wall, where its connector joins the street.

use super::{b, stairs};
use crate::world::{
    Face,
    block::BlockState,
    structure::template::{Sketch, Template},
};

#[derive(Clone, Copy)]
struct Style {
    name: &'static str,
    floor: BlockState,
    foundation: BlockState,
    wall: BlockState,
    pillar: BlockState,
    /// The roof's stairs block, and the block along its ridge (or the
    /// flat roof's top).
    roof: &'static str,
    roof_top: BlockState,
    door: &'static str,
    window: BlockState,
    fence: BlockState,
    path: BlockState,
    plaza: BlockState,
    light: BlockState,
    farm_edge: BlockState,
    pen_floor: BlockState,
    pen_animals: [&'static str; 2],
    flat: bool,
}

const AIR: BlockState = BlockState::AIR;

fn styles() -> [Style; 5] {
    [
        Style {
            name: "plains",
            floor: b("oak_planks"),
            foundation: b("cobblestone"),
            wall: b("oak_planks"),
            pillar: b("oak_log"),
            roof: "oak_stairs",
            roof_top: b("oak_planks"),
            door: "oak_door",
            window: b("glass_pane"),
            fence: b("oak_fence"),
            path: b("dirt_path"),
            plaza: b("cobblestone"),
            light: b("torch"),
            farm_edge: b("oak_log[axis=x]"),
            pen_floor: b("grass_block"),
            pen_animals: ["cow", "sheep"],
            flat: false,
        },
        Style {
            name: "desert",
            floor: b("smooth_sandstone"),
            foundation: b("sandstone"),
            wall: b("cut_sandstone"),
            pillar: b("smooth_sandstone"),
            roof: "sandstone_stairs",
            roof_top: b("smooth_sandstone"),
            door: "jungle_door",
            window: AIR,
            fence: b("oak_fence"),
            path: b("smooth_sandstone"),
            plaza: b("cut_sandstone"),
            light: b("torch"),
            farm_edge: b("cut_sandstone"),
            pen_floor: b("sand"),
            pen_animals: ["rabbit", "rabbit"],
            flat: true,
        },
        Style {
            name: "savanna",
            floor: b("acacia_planks"),
            foundation: b("orange_terracotta"),
            wall: b("acacia_planks"),
            pillar: b("acacia_log"),
            roof: "acacia_stairs",
            roof_top: b("acacia_planks"),
            door: "acacia_door",
            window: b("glass_pane"),
            fence: b("acacia_fence"),
            path: b("dirt_path"),
            plaza: b("orange_terracotta"),
            light: b("torch"),
            farm_edge: b("acacia_log[axis=x]"),
            pen_floor: b("grass_block"),
            pen_animals: ["sheep", "horse"],
            flat: false,
        },
        Style {
            name: "taiga",
            floor: b("spruce_planks"),
            foundation: b("mossy_cobblestone"),
            wall: b("spruce_planks"),
            pillar: b("spruce_log"),
            roof: "spruce_stairs",
            roof_top: b("spruce_planks"),
            door: "spruce_door",
            window: b("glass_pane"),
            fence: b("spruce_fence"),
            path: b("dirt_path"),
            plaza: b("cobblestone"),
            light: b("lantern"),
            farm_edge: b("spruce_log[axis=x]"),
            pen_floor: b("grass_block"),
            pen_animals: ["sheep", "pig"],
            flat: false,
        },
        Style {
            name: "snowy",
            floor: b("spruce_planks"),
            foundation: b("stone_bricks"),
            wall: b("snow_block"),
            pillar: b("stripped_spruce_log"),
            roof: "spruce_stairs",
            roof_top: b("packed_ice"),
            door: "spruce_door",
            window: b("glass_pane"),
            fence: b("spruce_fence"),
            path: b("dirt_path"),
            plaza: b("packed_ice"),
            light: b("lantern"),
            farm_edge: b("stripped_spruce_log[axis=x]"),
            pen_floor: b("snow_block"),
            pen_animals: ["rabbit", "pig"],
            flat: false,
        },
    ]
}

fn pool(s: &Style, kind: &str) -> String {
    format!("village/{}/{kind}", s.name)
}

/// The rows a gable roof over a `w`-wide building takes.
fn roof_height(s: &Style, w: i32) -> i32 {
    if s.flat { 2 } else { (w + 1) / 2 }
}

/// A building `w` wide, `d` deep and `h` high inside: floor, walls with
/// posts at the corners, a roof, a door in the north wall with its
/// connector, windows in the sides and a torch.
fn building(s: &Style, w: i32, d: i32, h: i32) -> Sketch {
    let mut k = Sketch::new([w, h + 1 + roof_height(s, w), d]);
    k.foundation = Some(s.foundation);
    k.fill([0, 0, 0], [w - 1, 0, d - 1], s.floor);
    for y in 1..=h {
        for z in 0..d {
            for x in 0..w {
                let (edge_x, edge_z) = (x == 0 || x == w - 1, z == 0 || z == d - 1);
                let block = match (edge_x, edge_z) {
                    (true, true) => s.pillar,
                    (true, false) | (false, true) => s.wall,
                    _ => AIR,
                };
                k.set(x, y, z, block);
            }
        }
    }
    let top = h + 1;
    if s.flat {
        k.fill([0, top, 0], [w - 1, top, d - 1], s.roof_top);
        for z in 0..d {
            for x in 0..w {
                let edge = x == 0 || x == w - 1 || z == 0 || z == d - 1;
                if edge && (x + z) % 2 == 0 {
                    k.set(x, top + 1, z, s.wall);
                }
            }
        }
    } else {
        // A gable along z: stairs rising from both sides, the ends walled.
        for i in 0.. {
            let (y, l, r) = (top + i, i, w - 1 - i);
            if l > r {
                break;
            }
            if l == r {
                k.fill([l, y, 0], [l, y, d - 1], s.roof_top);
                break;
            }
            for z in 0..d {
                k.set(l, y, z, stairs(s.roof, Face::East, false));
                k.set(r, y, z, stairs(s.roof, Face::West, false));
                for x in l + 1..r {
                    let end = z == 0 || z == d - 1;
                    k.set(x, y, z, if end { s.wall } else { AIR });
                }
            }
        }
    }
    let door = w / 2;
    k.door(door, 1, 0, s.door, Face::North);
    k.jigsaw(door, 0, 0, Face::North, &pool(s, "streets"));
    if d >= 5 {
        k.set(0, 2, d / 2, s.window);
        k.set(w - 1, 2, d / 2, s.window);
    }
    if w >= 5 {
        k.set(w / 2, 2, d - 1, s.window);
    }
    k.wall_torch(1, 2, d - 2, Face::East);
    k
}

fn villager(k: &mut Sketch, x: f64, z: f64, job: Option<&str>) {
    k.mob(x, 1.0, z, "villager", job);
}

fn small_house(s: &Style) -> Template {
    let mut k = building(s, 5, 5, 3);
    k.bed(1, 1, 2, Face::South);
    k.set(3, 1, 3, b("crafting_table"));
    k.chest(3, 1, 1, b("chest[facing=west]"), "village_house");
    villager(&mut k, 2.5, 2.5, None);
    k.weight = 4;
    k.template()
}

fn medium_house(s: &Style) -> Template {
    let mut k = building(s, 7, 6, 3);
    k.bed(1, 1, 3, Face::South);
    k.bed(5, 1, 3, Face::South);
    k.set(3, 1, 4, b("crafting_table"));
    k.chest(1, 1, 1, b("chest[facing=east]"), "village_house");
    villager(&mut k, 2.5, 3.5, None);
    villager(&mut k, 4.5, 3.5, None);
    k.weight = 3;
    k.template()
}

fn big_house(s: &Style) -> Template {
    let mut k = building(s, 9, 7, 4);
    k.bed(1, 1, 4, Face::South);
    k.bed(7, 1, 4, Face::South);
    k.bed(3, 1, 4, Face::South);
    k.set(5, 1, 5, b("furnace[facing=north]"));
    k.set(6, 1, 5, b("crafting_table"));
    k.chest(7, 1, 1, b("chest[facing=west]"), "village_house");
    for x in [2.5, 4.5, 6.5] {
        villager(&mut k, x, 2.5, None);
    }
    k.weight = 2;
    k.template()
}

/// A house where a villager works at `site` (and keeps `table`'s chest).
fn workshop(s: &Style, job: &str, site: BlockState, table: Option<&str>) -> Template {
    let mut k = building(s, 7, 7, 3);
    k.bed(1, 1, 4, Face::South);
    k.set(5, 1, 5, site);
    if let Some(table) = table {
        k.chest(5, 1, 1, b("chest[facing=west]"), table);
    }
    villager(&mut k, 3.5, 3.5, Some(job));
    k.weight = 1;
    k.template()
}

fn library(s: &Style) -> Template {
    let mut k = building(s, 7, 9, 4);
    for z in 2..=6 {
        for y in 1..=3 {
            k.set(1, y, z, b("bookshelf"));
            k.set(5, y, z, b("bookshelf"));
        }
    }
    k.set(3, 1, 6, b("lectern[facing=north]"));
    k.bed(2, 1, 7, Face::East);
    villager(&mut k, 3.5, 4.5, Some("librarian"));
    k.weight = 1;
    k.template()
}

fn temple(s: &Style) -> Template {
    let mut k = building(s, 5, 7, 6);
    k.set(2, 1, 5, b("brewing_stand"));
    k.set(1, 1, 5, b("cauldron"));
    k.chest(3, 1, 5, b("chest[facing=north]"), "village_cleric");
    k.set(2, 4, 6, s.window);
    k.bed(1, 1, 2, Face::South);
    villager(&mut k, 2.5, 3.5, Some("cleric"));
    k.weight = 1;
    k.template()
}

/// A wheat field `w` wide and `d` deep, edged, with water down every
/// `channel`th column and a composter by the path.
fn farm(s: &Style, w: i32, d: i32, weight: u32) -> Template {
    let mut k = Sketch::new([w, 2, d]);
    k.foundation = Some(s.foundation);
    let water_columns: Vec<i32> = if w > 7 {
        vec![w / 3, w - 1 - w / 3]
    } else {
        vec![w / 2]
    };
    for z in 0..d {
        for x in 0..w {
            let edge = x == 0 || x == w - 1 || z == 0 || z == d - 1;
            if edge {
                k.set(x, 0, z, s.farm_edge);
                k.set(x, 1, z, AIR);
            } else if water_columns.contains(&x) {
                k.set(x, 0, z, b("water"));
                k.set(x, 1, z, AIR);
            } else {
                k.set(x, 0, z, b("farmland[moisture=7]"));
                let age = 3 + ((x * 7 + z * 3) % 5);
                k.set(x, 1, z, b(&format!("wheat[age={age}]")));
            }
        }
    }
    k.set(1, 1, 0, b("composter"));
    k.jigsaw(w / 2, 0, 0, Face::North, &pool(s, "streets"));
    villager(&mut k, 1.5, 1.5, Some("farmer"));
    k.weight = weight;
    k.template()
}

fn animal_pen(s: &Style) -> Template {
    let mut k = Sketch::new([7, 2, 7]);
    k.fill([0, 0, 0], [6, 0, 6], s.pen_floor);
    for z in 0..7 {
        for x in 0..7 {
            let edge = x == 0 || x == 6 || z == 0 || z == 6;
            k.set(x, 1, z, if edge { s.fence } else { AIR });
        }
    }
    let gate = s.fence.id().name().replace("_fence", "_fence_gate");
    k.set(3, 1, 0, b(&format!("{gate}[facing=north]")));
    k.set(5, 1, 5, b("hay_block[axis=y]"));
    k.jigsaw(3, 0, 0, Face::North, &pool(s, "streets"));
    for (i, animal) in s.pen_animals.iter().enumerate() {
        k.mob(2.5 + i as f64 * 2.0, 1.0, 3.5, animal, None);
    }
    k.weight = 2;
    k.template()
}

fn lamp(s: &Style) -> Template {
    let mut k = Sketch::new([1, 4, 1]);
    k.set(0, 0, 0, s.plaza);
    k.set(0, 1, 0, s.fence);
    k.set(0, 2, 0, s.fence);
    k.set(0, 3, 0, s.light);
    k.jigsaw(0, 0, 0, Face::North, &pool(s, "streets"));
    k.weight = 3;
    k.template()
}

/// The meeting point: a plaza with a well, or with the village's bell,
/// streets leaving from all four sides, an iron golem and two villagers.
fn center(s: &Style, bell: bool) -> Template {
    let mut k = Sketch::new([9, 6, 9]);
    k.foundation = Some(s.foundation);
    k.fill([0, 0, 0], [8, 0, 8], s.plaza);
    k.fill([0, 1, 0], [8, 5, 8], AIR);
    if bell {
        k.set(4, 0, 4, s.foundation);
        k.set(4, 1, 4, s.foundation);
        k.set(4, 2, 4, b("bell[facing=north]"));
        for (x, z) in [(1, 1), (7, 1), (1, 7), (7, 7)] {
            k.set(x, 1, z, s.fence);
            k.set(x, 2, z, s.light);
        }
    } else {
        for x in 3..=5 {
            for z in 3..=5 {
                let middle = x == 4 && z == 4;
                k.set(x, 1, z, if middle { b("water") } else { s.foundation });
                if middle {
                    k.set(x, 0, z, b("water"));
                }
            }
        }
        for (x, z) in [(3, 3), (5, 3), (3, 5), (5, 5)] {
            k.set(x, 2, z, s.fence);
            k.set(x, 3, z, s.fence);
        }
        k.fill([3, 4, 3], [5, 4, 5], s.roof_top);
    }
    for (x, z, face) in [
        (4, 0, Face::North),
        (4, 8, Face::South),
        (0, 4, Face::West),
        (8, 4, Face::East),
    ] {
        k.jigsaw(x, 0, z, face, &pool(s, "streets"));
    }
    k.mob(1.5, 1.0, 6.5, "iron_golem", None);
    k.mob(6.5, 1.0, 1.5, "villager", None);
    k.mob(1.5, 1.0, 1.5, "villager", None);
    k.template()
}

/// A street piece laid into the ground: path wherever `path(x, z)`,
/// with its connectors.
fn street(
    s: &Style,
    size: (i32, i32),
    path: impl Fn(i32, i32) -> bool,
    joins: &[(i32, i32, Face, &str)],
    weight: u32,
) -> Template {
    let mut k = Sketch::new([size.0, 1, size.1]);
    k.on_surface = true;
    for z in 0..size.1 {
        for x in 0..size.0 {
            if path(x, z) {
                k.set(x, 0, z, s.path);
            }
        }
    }
    for &(x, z, face, kind) in joins {
        k.jigsaw(x, 0, z, face, &pool(s, kind));
    }
    k.weight = weight;
    k.template()
}

fn streets(s: &Style) -> Vec<(&'static str, Template)> {
    use Face::*;
    vec![
        (
            "straight",
            street(
                s,
                (3, 7),
                |_, _| true,
                &[
                    (1, 0, North, "streets"),
                    (1, 6, South, "streets"),
                    (0, 3, West, "houses"),
                    (2, 3, East, "houses"),
                ],
                4,
            ),
        ),
        (
            "long",
            street(
                s,
                (3, 13),
                |_, _| true,
                &[
                    (1, 0, North, "streets"),
                    (1, 12, South, "streets"),
                    (0, 3, West, "houses"),
                    (2, 3, East, "houses"),
                    (0, 9, West, "houses"),
                    (2, 9, East, "houses"),
                ],
                3,
            ),
        ),
        (
            "crossing",
            street(
                s,
                (7, 7),
                |x, z| (2..=4).contains(&x) || (2..=4).contains(&z),
                &[
                    (3, 0, North, "streets"),
                    (3, 6, South, "streets"),
                    (0, 3, West, "streets"),
                    (6, 3, East, "streets"),
                ],
                2,
            ),
        ),
        (
            "turn",
            street(
                s,
                (5, 5),
                |x, z| (z <= 3 && (1..=3).contains(&x)) || (x <= 3 && (1..=3).contains(&z)),
                &[
                    (2, 0, North, "streets"),
                    (0, 2, West, "streets"),
                    (4, 2, East, "houses"),
                    (2, 4, South, "houses"),
                ],
                2,
            ),
        ),
        (
            "end",
            street(
                s,
                (3, 3),
                |_, _| true,
                &[
                    (1, 0, North, "streets"),
                    (0, 1, West, "houses"),
                    (2, 1, East, "houses"),
                    (1, 2, South, "houses"),
                ],
                1,
            ),
        ),
    ]
}

fn houses(s: &Style) -> Vec<(&'static str, Template)> {
    vec![
        ("small_house", small_house(s)),
        ("medium_house", medium_house(s)),
        ("big_house", big_house(s)),
        ("small_farm", farm(s, 7, 9, 3)),
        ("large_farm", farm(s, 11, 13, 2)),
        ("animal_pen", animal_pen(s)),
        ("lamp", lamp(s)),
        ("library", library(s)),
        ("temple", temple(s)),
        (
            "armorer",
            workshop(
                s,
                "armorer",
                b("blast_furnace[facing=north]"),
                Some("village_armorer"),
            ),
        ),
        (
            "toolsmith",
            workshop(
                s,
                "toolsmith",
                b("smithing_table"),
                Some("village_toolsmith"),
            ),
        ),
        (
            "weaponsmith",
            workshop(
                s,
                "weaponsmith",
                b("grindstone[facing=north]"),
                Some("village_weaponsmith"),
            ),
        ),
        (
            "butcher",
            workshop(
                s,
                "butcher",
                b("smoker[facing=north]"),
                Some("village_butcher"),
            ),
        ),
    ]
}

pub fn all() -> Vec<(String, Template)> {
    let mut all = Vec::new();
    for s in styles() {
        all.push((pool(&s, "centers/well"), center(&s, false)));
        all.push((pool(&s, "centers/bell"), center(&s, true)));
        for (name, t) in streets(&s) {
            all.push((pool(&s, &format!("streets/{name}")), t));
        }
        for (name, t) in houses(&s) {
            all.push((pool(&s, &format!("houses/{name}")), t));
        }
    }
    all
}
