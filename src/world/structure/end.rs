//! End cities and their ships (M28), on the outer islands: one in each
//! 320-block region with an island of at least 26 blocks' radius, standing on
//! that island's middle (modern places them on the outer highlands by
//! `RandomSpread` 20/11 and builds them from jigsaw templates; this lays
//! them out in code after them).
//!
//! A city is a tall tower of three floors with a ladder up, windows, doors
//! and rooms with `end_city_treasure` chests and shulkers, two shorter
//! towers joined to it by bridges, and, in half of them, a ship tied on by a
//! bridge, with a chest holding elytra.

use std::{cell::RefCell, collections::HashMap, sync::Arc};

use glam::{DVec3, IVec3};

use super::{Kind, MobSpawn, Structure, build::Build, write};
use crate::world::{
    Face,
    block::BlockState,
    blocks,
    chunk::{Chunk, ChunkPos},
    props::{Prop, axis},
    worldgen::{end::End, rng::Rng},
};

/// A city needs an island at least this wide, in a region this big.
const MIN_RADIUS: i32 = 26;
const REGION: i32 = 320;
/// Height of a floor.
const FLOOR: i32 = 6;

fn of(id: crate::world::block::BlockId) -> BlockState {
    BlockState::of(id)
}

fn wall_block(at: IVec3) -> BlockState {
    let h = (at.x as u32).wrapping_mul(73_856_093)
        ^ (at.y as u32).wrapping_mul(19_349_663)
        ^ (at.z as u32).wrapping_mul(83_492_791);
    if h.wrapping_mul(2_654_435_761) >> 29 < 2 {
        of(blocks::PURPUR_BLOCK)
    } else {
        of(blocks::END_STONE_BRICKS)
    }
}

fn pillar(at: IVec3) -> BlockState {
    let _ = at;
    of(blocks::PURPUR_PILLAR).with(Prop::Axis, axis::Y)
}

/// The cities that start in the regions round chunk `pos`: where, as the
/// chunk their island's middle is in.
fn starts_near(end: &End, pos: ChunkPos) -> Vec<ChunkPos> {
    let reach = Kind::EndCity.reach();
    let (x, z) = pos.min_block();
    let mut found = Vec::new();
    let region = |c: i32| (c * 16).div_euclid(REGION);
    for rz in region(pos.z - reach)..=region(pos.z + reach) {
        for rx in region(pos.x - reach)..=region(pos.x + reach) {
            let mut best: Option<(IVec3, i32)> = None;
            for (at, radius) in end.big_islands_in(rx * REGION, rz * REGION, REGION, MIN_RADIUS) {
                if best.is_none_or(|(_, r)| radius > r) {
                    best = Some((at, radius));
                }
            }
            if let Some((at, _)) = best {
                let start = ChunkPos::of_block(at.x, at.z);
                let _ = (x, z);
                if (start.x - pos.x).abs() <= reach && (start.z - pos.z).abs() <= reach {
                    found.push(start);
                }
            }
        }
    }
    found
}

/// One storey-and-a-half of walls: a hollow box of `half` blocks each way
/// of `center` from `base` up `floors` floors, with a floor and ceiling at
/// each level, pillars in the corners, windows and doors.
struct Tower {
    center: IVec3,
    half: i32,
    floors: i32,
}

impl Tower {
    fn corner(&self, dx: i32, dz: i32, y: i32) -> IVec3 {
        IVec3::new(
            self.center.x + dx * self.half,
            y,
            self.center.z + dz * self.half,
        )
    }

    fn build(&self, b: &mut Build, rng: &mut Rng, treasure: bool) {
        let (c, h) = (self.center, self.half);
        let top = c.y + self.floors * FLOOR;
        let lo = IVec3::new(c.x - h, c.y, c.z - h);
        let hi = IVec3::new(c.x + h, top, c.z + h);
        // Walls and floors, hollowed inside.
        b.hollow(lo, hi, wall_block);
        for f in 1..self.floors {
            let y = c.y + f * FLOOR;
            b.fill(
                IVec3::new(c.x - h + 1, y, c.z - h + 1),
                IVec3::new(c.x + h - 1, y, c.z + h - 1),
                of(blocks::PURPUR_BLOCK),
            );
        }
        // Corner pillars, and the foundations under the whole footprint.
        for (dx, dz) in [(-1, -1), (1, -1), (-1, 1), (1, 1)] {
            b.fill(
                self.corner(dx, dz, c.y),
                self.corner(dx, dz, top),
                pillar(c),
            );
        }
        for z in c.z - h..=c.z + h {
            for x in c.x - h..=c.x + h {
                b.foundation(IVec3::new(x, c.y, z), of(blocks::END_STONE_BRICKS));
            }
        }
        for f in 0..self.floors {
            let y = c.y + f * FLOOR;
            // Windows in the middle of each wall.
            for off in [-1, 0, 1] {
                for dy in 2..=3 {
                    for (wx, wz) in [(c.x + off, c.z - h), (c.x + off, c.z + h)] {
                        b.set(IVec3::new(wx, y + dy, wz), BlockState::AIR);
                    }
                    for (wx, wz) in [(c.x - h, c.z + off), (c.x + h, c.z + off)] {
                        b.set(IVec3::new(wx, y + dy, wz), BlockState::AIR);
                    }
                }
            }
            // Lights in the corners of the ceiling.
            for (dx, dz) in [(-1, -1), (1, -1), (-1, 1), (1, 1)] {
                let at = IVec3::new(c.x + dx * (h - 1), y + FLOOR - 1, c.z + dz * (h - 1));
                b.set(at, of(blocks::END_ROD));
            }
        }
        // Doors in the south and north walls of the ground floor.
        for off in [-1, 0, 1] {
            for dy in 1..=3 {
                b.set(IVec3::new(c.x + off, c.y + dy, c.z + h), BlockState::AIR);
                b.set(IVec3::new(c.x + off, c.y + dy, c.z - h), BlockState::AIR);
            }
        }
        // A ladder up the inside of the north wall, through the floors.
        let ladder = of(blocks::LADDER).with_face(Prop::Facing, Face::South);
        for y in c.y + 1..top {
            b.set(IVec3::new(c.x, y, c.z - h + 1), ladder);
        }
        for f in 1..self.floors {
            b.set(IVec3::new(c.x, c.y + f * FLOOR, c.z - h + 1), ladder);
        }
        // The roof, with a hole for the ladder and a rim of rods.
        b.fill(
            IVec3::new(c.x - h, top, c.z - h),
            IVec3::new(c.x + h, top, c.z + h),
            of(blocks::PURPUR_BLOCK),
        );
        b.set(IVec3::new(c.x, top, c.z - h + 1), BlockState::AIR);
        for (dx, dz) in [(-1, -1), (1, -1), (-1, 1), (1, 1)] {
            b.set(self.corner(dx, dz, top + 1), of(blocks::END_ROD));
        }
        // Chests and shulkers on the upper floors (and a chest below).
        for f in 0..self.floors {
            let y = c.y + f * FLOOR;
            if f == 0 && !treasure {
                continue;
            }
            let at = IVec3::new(c.x + h - 2, y + 1, c.z + h - 2);
            let facing = of(blocks::CHEST).with_face(Prop::Facing, Face::North);
            b.chest(at, facing, "end_city_treasure");
            if f >= 1 {
                for _ in 0..1 + rng.below(2) {
                    let spot = DVec3::new(
                        f64::from(c.x - 1 + rng.below(3) as i32) + 0.5,
                        f64::from(y + 1),
                        f64::from(c.z + 1 + rng.below(2) as i32) + 0.5,
                    );
                    b.mob(spot, "shulker", None);
                }
            }
        }
    }
}

/// A floating ship with a cabin and a chest of elytra, its deck at `deck`
/// (the middle of its stern) pointing south.
fn ship(b: &mut Build, deck: IVec3, rng: &mut Rng) {
    let length = 14;
    // The hull, narrowing toward the keel.
    for k in 0..5 {
        let half = 3 - k / 2;
        for z in 0..length {
            // The bow narrows too.
            let bow = (z - (length - 4)).max(0);
            let half = (half - bow / 2).max(0);
            for x in -half..=half {
                let at = deck + IVec3::new(x, -k, z);
                b.set(at, wall_block(at));
            }
        }
    }
    // The deck's rail and masts.
    for z in (0..length - 2).step_by(3) {
        for side in [-3, 3] {
            b.set(deck + IVec3::new(side, 1, z), pillar(deck));
        }
    }
    for rise in 1..=7 {
        b.set(deck + IVec3::new(0, rise, 8), pillar(deck));
    }
    b.set(deck + IVec3::new(0, 8, 8), of(blocks::END_ROD));
    // The cabin at the stern: 5 wide, 4 long, 3 high, open at the front.
    let lo = deck + IVec3::new(-2, 1, 0);
    let hi = deck + IVec3::new(2, 3, 4);
    b.hollow(lo, hi, wall_block);
    for dy in 1..=2 {
        b.set(deck + IVec3::new(0, dy, 4), BlockState::AIR);
    }
    b.set(deck + IVec3::new(-2, 2, 2), of(blocks::END_ROD));
    let facing = of(blocks::CHEST).with_face(Prop::Facing, Face::South);
    b.chest(deck + IVec3::new(0, 1, 1), facing, "end_ship");
    b.chest(deck + IVec3::new(1, 1, 1), facing, "end_city_treasure");
    let _ = rng;
    b.mob(
        DVec3::new(
            f64::from(deck.x) + 0.5,
            f64::from(deck.y) + 1.0,
            f64::from(deck.z) + 3.5,
        ),
        "shulker",
        None,
    );
}

/// A 3-wide walkway of purpur from `from` to `to` along x or z.
fn bridge(b: &mut Build, from: IVec3, to: IVec3) {
    let along_x = (to.x - from.x).abs() > (to.z - from.z).abs();
    let (lo, hi) = (from.min(to), from.max(to));
    for z in lo.z..=hi.z {
        for x in lo.x..=hi.x {
            let spread = if along_x {
                [(0, -1), (0, 0), (0, 1)]
            } else {
                [(-1, 0), (0, 0), (1, 0)]
            };
            for (dx, dz) in spread {
                let at = IVec3::new(x + dx, from.y, z + dz);
                b.set(at, of(blocks::PURPUR_BLOCK));
                // Clear the way above.
                for dy in 1..=3 {
                    b.set(at + IVec3::Y * dy, BlockState::AIR);
                }
            }
        }
    }
    // Rails of slabs along both edges.
    let rail = of(blocks::PURPUR_SLAB);
    for z in lo.z..=hi.z {
        for x in lo.x..=hi.x {
            let edges: [(i32, i32); 2] = if along_x {
                [(0, -2), (0, 2)]
            } else {
                [(-2, 0), (2, 0)]
            };
            for (dx, dz) in edges {
                b.set(IVec3::new(x + dx, from.y + 1, z + dz), rail);
            }
        }
    }
}

fn generate(end: &End, start: ChunkPos) -> Option<Structure> {
    let (x, z) = start.min_block();
    let (island, _) = end.island_near(x + 8, z + 8)?;
    let mut rng = Rng::for_chunk(
        end.world_seed(),
        start.x,
        start.z,
        Kind::EndCity.salt() ^ 0x5EED,
    );
    let ground = island;
    let mut b = Build::new();
    let main = Tower {
        center: ground,
        half: 5,
        floors: 3,
    };
    main.build(&mut b, &mut rng, false);
    // Two shorter towers, east and west, each with a bridge from the second
    // floor of the main one.
    let level = ground.y + FLOOR;
    for side in [1, -1] {
        let center = IVec3::new(ground.x + side * 17, ground.y, ground.z);
        let tower = Tower {
            center,
            half: 3,
            floors: 2,
        };
        tower.build(&mut b, &mut rng, true);
        bridge(
            &mut b,
            IVec3::new(ground.x + side * 5, level, ground.z),
            IVec3::new(center.x - side * 3, level, ground.z),
        );
        // The wall doorways the bridge goes through.
        for dz in -1..=1 {
            for dy in 1..=3 {
                b.set(
                    IVec3::new(ground.x + side * 5, level + dy, ground.z + dz),
                    BlockState::AIR,
                );
                b.set(
                    IVec3::new(center.x - side * 3, level + dy, ground.z + dz),
                    BlockState::AIR,
                );
            }
        }
    }
    // Half of them have a ship tied to the south, off the second floor.
    if rng.below(2) == 0 {
        let deck_y = ground.y + 2 * FLOOR;
        bridge(
            &mut b,
            IVec3::new(ground.x, deck_y, ground.z + 6),
            IVec3::new(ground.x, deck_y, ground.z + 14),
        );
        for dx in -1..=1 {
            for dy in 1..=3 {
                b.set(
                    IVec3::new(ground.x + dx, deck_y + dy, ground.z + 5),
                    BlockState::AIR,
                );
            }
        }
        ship(
            &mut b,
            IVec3::new(ground.x, deck_y, ground.z + 15),
            &mut rng,
        );
    }
    Structure::new(Kind::EndCity, start, vec![b.finish()])
}

type CacheKey = (u64, ChunkPos);

thread_local! {
    static CACHE: RefCell<HashMap<CacheKey, Option<Arc<Structure>>>> = RefCell::new(HashMap::new());
}

const CACHE_SIZE: usize = 64;

/// The city starting in `start`, laid out once per thread.
pub fn structure_at(end: &End, start: ChunkPos) -> Option<Arc<Structure>> {
    let key = (end.world_seed(), start);
    if let Some(found) = CACHE.with(|cache| cache.borrow().get(&key).cloned()) {
        return found;
    }
    let made = generate(end, start).map(Arc::new);
    CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        if cache.len() >= CACHE_SIZE {
            cache.clear();
        }
        cache.insert(key, made.clone());
    });
    made
}

/// Every city with a piece reaching into chunk `pos`.
pub fn near(end: &End, pos: ChunkPos) -> Vec<Arc<Structure>> {
    starts_near(end, pos)
        .into_iter()
        .filter_map(|start| structure_at(end, start))
        .filter(|s| s.bounds.touches_chunk(pos))
        .collect()
}

/// Writes the parts of cities inside `chunk`.
pub fn place(end: &End, chunk: &mut Chunk) {
    let structures = near(end, chunk.pos);
    if !structures.is_empty() {
        write(&structures, end.world_seed(), chunk);
    }
}

/// Mobs the cities put in chunk `pos` when it first loads: their shulkers.
pub fn spawns_in(end: &End, pos: ChunkPos) -> Vec<MobSpawn> {
    let mut spawns = Vec::new();
    for structure in near(end, pos) {
        for mob in structure.mobs() {
            let at = ChunkPos::of_block(mob.at.x.floor() as i32, mob.at.z.floor() as i32);
            if at == pos {
                spawns.push(mob.clone());
            }
        }
    }
    spawns
}

/// The city, if any, whose start is the nearest within `radius` chunks of
/// `pos` (for tests and screenshots).
pub fn find(end: &End, pos: ChunkPos, radius: i32) -> Option<Arc<Structure>> {
    let mut best: Option<(i32, Arc<Structure>)> = None;
    for dz in -radius / 5..=radius / 5 {
        for dx in -radius / 5..=radius / 5 {
            let at = pos.offset(dx * 5, dz * 5);
            for start in starts_near(end, at) {
                let d = (start.x - pos.x).abs().max((start.z - pos.z).abs());
                if d <= radius
                    && best.as_ref().is_none_or(|(b, _)| d < *b)
                    && let Some(found) = structure_at(end, start)
                {
                    best = Some((d, found));
                }
            }
        }
    }
    best.map(|(_, s)| s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::structure::loot;

    const SEED: u64 = 20111118;

    #[test]
    fn a_city_stands_in_the_middle_of_a_big_outer_island_and_not_near_the_main_one() {
        let end = End::new(SEED);
        let city = find(&end, ChunkPos::new(60, 0), 120).expect("a city within reach");
        let center = city.bounds.center();
        assert!(f64::from(center.x).hypot(f64::from(center.z)) > 600.0);
        let (island, radius) = end.island_near(center.x, center.z).unwrap();
        assert!(radius >= MIN_RADIUS);
        assert!((island.x - center.x).abs() < 40 && (island.z - center.z).abs() < 40);
        // None of them anywhere near the main island.
        for dz in -20..=20 {
            for dx in -20..=20 {
                assert!(near(&end, ChunkPos::new(dx, dz)).is_empty());
            }
        }
    }

    #[test]
    fn cities_have_towers_chests_shulkers_and_often_a_ship_with_elytra() {
        let end = End::new(SEED);
        let mut ships = 0;
        let mut cities = 0;
        let mut seen = std::collections::HashSet::new();
        for z in (-120..=120).step_by(20) {
            for x in (-120..=120).step_by(20) {
                let Some(city) = find(&end, ChunkPos::new(x, z), 20) else {
                    continue;
                };
                if !seen.insert(city.start) {
                    continue;
                }
                cities += 1;
                let piece = &city.pieces[0];
                assert!(piece.blocks.len() > 2500, "{}", piece.blocks.len());
                assert!(
                    piece
                        .blocks
                        .iter()
                        .any(|(_, b)| b.is(blocks::PURPUR_PILLAR))
                );
                assert!(
                    piece
                        .loot
                        .iter()
                        .filter(|(_, t)| t == "end_city_treasure")
                        .count()
                        >= 4
                );
                assert!(city.mobs().filter(|m| m.mob == "shulker").count() >= 4);
                if piece.loot.iter().any(|(_, t)| t == "end_ship") {
                    ships += 1;
                }
            }
        }
        assert!(cities >= 6, "{cities}");
        assert!(ships >= 1 && ships < cities, "{ships} of {cities}");
    }

    #[test]
    #[ignore]
    fn where_are_the_cities() {
        let end = End::new(SEED);
        let mut seen = std::collections::HashSet::new();
        for z in (-60..=60).step_by(10) {
            for x in (-60..=60).step_by(10) {
                if let Some(city) = find(&end, ChunkPos::new(x, z), 20)
                    && seen.insert(city.start)
                {
                    let ship = city.pieces[0].loot.iter().any(|(_, t)| t == "end_ship");
                    println!(
                        "city {:?} center {} ship {ship}",
                        city.start,
                        city.bounds.center()
                    );
                }
            }
        }
    }

    #[test]
    fn the_ship_chest_always_holds_elytra() {
        let mut rng = Rng::new(4);
        for _ in 0..40 {
            let slots = loot::fill("end_ship", &mut rng);
            assert!(
                slots
                    .iter()
                    .flatten()
                    .any(|s| s.id == crate::inventory::items::ELYTRA)
            );
        }
    }

    #[test]
    fn chunks_over_a_city_hold_its_blocks_and_chests_the_same_whichever_draws_them() {
        let end = End::new(SEED);
        let city = find(&end, ChunkPos::new(60, 0), 120).unwrap();
        let chest = city.pieces[0].loot[0].0;
        let pos = ChunkPos::of_block(chest.x, chest.z);
        let chunk = end.generate(pos);
        let (lx, lz) = (
            chest.x.rem_euclid(16) as usize,
            chest.z.rem_euclid(16) as usize,
        );
        let local = [lx as i16, chest.y as i16, lz as i16];
        assert!(chunk.get(lx, chest.y, lz).is(blocks::CHEST));
        assert!(
            chunk
                .block_entity(local)
                .is_some_and(|e| e.slots().iter().any(Option::is_some))
        );
        let again = end.generate(pos);
        for y in crate::config::MIN_Y..crate::config::MAX_Y {
            assert_eq!(chunk.get(3, y, 5), again.get(3, y, 5));
        }
        // A tower wall stands on end stone, not in the air.
        let wall = city.pieces[0]
            .blocks
            .iter()
            .find(|(_, b)| b.is(blocks::END_STONE_BRICKS))
            .unwrap()
            .0;
        assert!(
            spawns_in(&end, ChunkPos::of_block(wall.x, wall.z))
                .iter()
                .all(|m| m.mob == "shulker")
        );
    }
}
