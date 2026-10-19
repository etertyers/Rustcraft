//! What grows underground: lush caves (moss and glow berries), dripstone
//! caves (dripstone and its spikes) and amethyst geodes.

use super::{
    ores,
    rng::Rng,
    terrain::{Area, Terrain},
};
use crate::{
    config::{CHUNK_SIZE, WORLD_HEIGHT},
    world::{
        biome::Biome,
        block::BlockState,
        blocks,
        chunk::{Chunk, ChunkPos},
        props::{Prop, thickness, vertical_direction},
    },
};

const MOSS: BlockState = BlockState::of(blocks::MOSS_BLOCK);
const MOSS_CARPET: BlockState = BlockState::of(blocks::MOSS_CARPET);
const DRIPSTONE: BlockState = BlockState::of(blocks::DRIPSTONE_BLOCK);
const POINTED: BlockState = BlockState::of(blocks::POINTED_DRIPSTONE);
const VINES: BlockState = BlockState::of(blocks::CAVE_VINES);
const VINES_PLANT: BlockState = BlockState::of(blocks::CAVE_VINES_PLANT);
const CLAY: BlockState = BlockState::of(blocks::CLAY);
const SHORT_GRASS: BlockState = BlockState::of(blocks::SHORT_GRASS);
const AMETHYST: BlockState = BlockState::of(blocks::AMETHYST_BLOCK);
const BUDDING: BlockState = BlockState::of(blocks::BUDDING_AMETHYST);
const CALCITE: BlockState = BlockState::of(blocks::CALCITE);
const BASALT: BlockState = BlockState::of(blocks::SMOOTH_BASALT);

const CAVE_SALT: u64 = 0xCA7E;
const GEODE_SALT: u64 = 0x6E0D;

/// One chunk in this many has a geode (modern `rarity_filter` 24).
const GEODE_RARITY: u32 = 24;
/// Geode centers' heights: modern y -58..30, squeezed.
const GEODE_LOW: i32 = ores::ours(-58);
const GEODE_HIGH: i32 = ores::ours(30);

pub fn populate(terrain: &Terrain, area: &Area, chunk: &mut Chunk) {
    caves(terrain, area, chunk);
    let center = chunk.pos;
    for dz in -1..=1 {
        for dx in -1..=1 {
            geode(terrain, center.offset(dx, dz), chunk);
        }
    }
}

/// Whether a block is the kind of rock cave biomes dress.
fn rock(block: BlockState) -> bool {
    matches!(
        block.id(),
        blocks::STONE
            | blocks::DEEPSLATE
            | blocks::GRANITE
            | blocks::DIORITE
            | blocks::ANDESITE
            | blocks::TUFF
            | blocks::DIRT
            | blocks::GRAVEL
    )
}

/// Dresses the floors and ceilings of caves inside cave biomes.
fn caves(terrain: &Terrain, area: &Area, chunk: &mut Chunk) {
    let pos = chunk.pos;
    let mut rng = Rng::for_chunk(terrain.seed(), pos.x, pos.z, CAVE_SALT);
    let (min_x, min_z) = pos.min_block();
    for z in 0..CHUNK_SIZE {
        for x in 0..CHUNK_SIZE {
            let column = area.get(min_x + x as i32, min_z + z as i32);
            let Some(cave) = column.cave else {
                continue;
            };
            let top = (column.height - 12).min(WORLD_HEIGHT as i32 - 2) as usize;
            for y in 2..top {
                if chunk.biome_at(x, y, z) != cave || !chunk.get(x, y, z).is_air() {
                    continue;
                }
                let below = chunk.get(x, y - 1, z);
                let above = chunk.get(x, y + 1, z);
                if rock(below) {
                    floor(cave, chunk, (x, y, z), &mut rng);
                }
                if rock(above) && chunk.get(x, y, z).is_air() {
                    ceiling(cave, chunk, (x, y, z), &mut rng);
                }
            }
        }
    }
}

/// The floor under the air at `at`.
fn floor(cave: Biome, chunk: &mut Chunk, (x, y, z): (usize, usize, usize), rng: &mut Rng) {
    let roll = rng.next_f64();
    match cave {
        Biome::LushCaves => {
            let ground = if roll < 0.08 { CLAY } else { MOSS };
            if roll < 0.85 {
                chunk.set(x, y - 1, z, ground);
                let cover = rng.next_f64();
                if ground == MOSS && cover < 0.25 {
                    chunk.set(x, y, z, MOSS_CARPET);
                } else if ground == MOSS && cover < 0.4 {
                    chunk.set(x, y, z, SHORT_GRASS);
                }
            }
        }
        _ => {
            if roll < 0.6 {
                chunk.set(x, y - 1, z, DRIPSTONE);
            }
            if roll < 0.1 {
                let length = 1 + rng.below(4) as usize;
                spike(chunk, (x, y, z), length, false);
            }
        }
    }
}

/// The ceiling over the air at `at`.
fn ceiling(cave: Biome, chunk: &mut Chunk, (x, y, z): (usize, usize, usize), rng: &mut Rng) {
    let roll = rng.next_f64();
    match cave {
        Biome::LushCaves => {
            if roll < 0.5 {
                chunk.set(x, y + 1, z, MOSS);
            }
            if roll < 0.12 {
                // Glow berry vines, lit in places.
                let length = 1 + rng.below(6) as usize;
                for k in 0..length {
                    let cy = y - k;
                    if cy < 2 || !chunk.get(x, cy, z).is_air() || !chunk.get(x, cy - 1, z).is_air()
                    {
                        break;
                    }
                    let tip = k + 1 == length;
                    let berries = rng.next_f64() < 0.3;
                    let vine = if tip { VINES } else { VINES_PLANT };
                    chunk.set(x, cy, z, vine.with_bool(Prop::Berries, berries));
                }
            }
        }
        _ => {
            if roll < 0.6 {
                chunk.set(x, y + 1, z, DRIPSTONE);
            }
            if roll < 0.12 {
                let length = 1 + rng.below(5) as usize;
                spike(chunk, (x, y, z), length, true);
            }
        }
    }
}

/// Pointed dripstone `length` long from `at`, hanging down from the block
/// above (`down`) or standing up from the block below.
fn spike(chunk: &mut Chunk, (x, y, z): (usize, usize, usize), length: usize, down: bool) {
    // Shorten it to the open air it has, leaving a block of air at its tip.
    let mut room = 0;
    while room <= length {
        let cy = if down {
            y.checked_sub(room)
        } else {
            Some(y + room)
        };
        match cy {
            Some(cy) if (1..WORLD_HEIGHT - 1).contains(&cy) && chunk.get(x, cy, z).is_air() => {
                room += 1
            }
            _ => break,
        }
    }
    let length = length.min(room.saturating_sub(1));
    let direction = if down {
        vertical_direction::DOWN
    } else {
        vertical_direction::UP
    };
    for k in 0..length {
        // k counts from the base; the tip is the last.
        let from_tip = length - 1 - k;
        let shape = match from_tip {
            0 => thickness::TIP,
            1 => thickness::FRUSTUM,
            _ if k == 0 => thickness::BASE,
            _ => thickness::MIDDLE,
        };
        let cy = if down { y - k } else { y + k };
        chunk.set(
            x,
            cy,
            z,
            POINTED
                .with(Prop::VerticalDirection, direction)
                .with(Prop::Thickness, shape),
        );
    }
}

/// Replays chunk `source`'s geode, if it has one, writing the part inside
/// `chunk` (modern `GeodeFeature`, simplified: a ball of smooth basalt,
/// calcite and amethyst around a hollow, buds and clusters on the floor).
fn geode(terrain: &Terrain, source: ChunkPos, chunk: &mut Chunk) {
    let mut rng = Rng::for_chunk(terrain.seed(), source.x, source.z, GEODE_SALT);
    if rng.below(GEODE_RARITY) != 0 {
        return;
    }
    let (sx, sz) = source.min_block();
    let cx = sx + rng.below(16) as i32;
    let cz = sz + rng.below(16) as i32;
    let cy = GEODE_LOW + rng.below((GEODE_HIGH - GEODE_LOW) as u32) as i32;
    let radius = 4.0 + rng.next_f64() * 1.5;
    let (min_x, min_z) = chunk.pos.min_block();
    let outer = (radius + 3.0).ceil() as i32;
    let seed = rng.next_u64();
    for dz in -outer..=outer {
        for dx in -outer..=outer {
            let (x, z) = (cx + dx, cz + dz);
            let (lx, lz) = (x - min_x, z - min_z);
            if !(0..16).contains(&lx) || !(0..16).contains(&lz) {
                continue;
            }
            for dy in -outer..=outer {
                let y = cy + dy;
                if !(1..WORLD_HEIGHT as i32 - 1).contains(&y) {
                    continue;
                }
                let d = ((dx * dx + dy * dy + dz * dz) as f64).sqrt();
                // A per-block roll, the same whichever chunk asks.
                let mut cell = Rng::for_chunk(seed, x, z, y as u64);
                let wobble = cell.next_f64() * 0.6;
                let d = d - wobble;
                let (lx, ly, lz) = (lx as usize, y as usize, lz as usize);
                let block = if d < radius - 1.0 {
                    BlockState::AIR
                } else if d < radius {
                    if cell.next_f64() < 0.083 {
                        BUDDING
                    } else {
                        AMETHYST
                    }
                } else if d < radius + 1.0 {
                    CALCITE
                } else if d < radius + 2.0 {
                    BASALT
                } else {
                    continue;
                };
                let here = chunk.get(lx, ly, lz);
                if here.id() == blocks::BEDROCK {
                    continue;
                }
                chunk.set(lx, ly, lz, block);
            }
        }
    }
    // Buds and clusters on budding amethyst in the floor of the hollow.
    let buds = [
        blocks::SMALL_AMETHYST_BUD,
        blocks::MEDIUM_AMETHYST_BUD,
        blocks::LARGE_AMETHYST_BUD,
        blocks::AMETHYST_CLUSTER,
    ];
    for dz in -outer..=outer {
        for dx in -outer..=outer {
            let (lx, lz) = (cx + dx - min_x, cz + dz - min_z);
            if !(0..16).contains(&lx) || !(0..16).contains(&lz) {
                continue;
            }
            for dy in -outer..0 {
                let y = cy + dy;
                if y < 1 || y + 1 >= WORLD_HEIGHT as i32 {
                    continue;
                }
                let y = y as usize;
                let (lx, lz) = (lx as usize, lz as usize);
                if chunk.get(lx, y, lz) == BUDDING && chunk.get(lx, y + 1, lz).is_air() {
                    let mut cell = Rng::for_chunk(seed ^ 0xB0D, cx + dx, cz + dz, y as u64);
                    if cell.below(3) != 0 {
                        let bud = buds[cell.below(4) as usize];
                        chunk.set(lx, y + 1, lz, BlockState::of(bud));
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn census(terrain: &Terrain, chunks: impl Iterator<Item = ChunkPos>) -> HashMap<u16, usize> {
        let mut counts = HashMap::new();
        for pos in chunks {
            let chunk = terrain.generate(pos);
            for y in 0..WORLD_HEIGHT {
                for i in 0..256 {
                    *counts
                        .entry(chunk.get(i % 16, y, i / 16).id().0)
                        .or_insert(0) += 1;
                }
            }
        }
        counts
    }

    fn find_cave(terrain: &Terrain, biome: Biome) -> ChunkPos {
        (0..2000)
            .map(|i| ChunkPos::new((i % 45) * 5 - 110, (i / 45) * 5 - 110))
            .find(|pos| {
                let (x, z) = pos.min_block();
                terrain.column(x + 8, z + 8).cave == Some(biome)
                    && terrain.column(x + 8, z + 8).height > 70
            })
            .unwrap_or_else(|| panic!("no {biome:?}"))
    }

    #[test]
    fn lush_caves_grow_moss_and_glow_berries() {
        let terrain = Terrain::new(31);
        let center = find_cave(&terrain, Biome::LushCaves);
        let around = (-1..=1).flat_map(|dz| (-1..=1).map(move |dx| center.offset(dx, dz)));
        let counts = census(&terrain, around);
        let n = |id: crate::world::block::BlockId| counts.get(&id.0).copied().unwrap_or(0);
        assert!(n(blocks::MOSS_BLOCK) > 50, "{} moss", n(blocks::MOSS_BLOCK));
        assert!(n(blocks::CAVE_VINES) + n(blocks::CAVE_VINES_PLANT) > 0);
        // Dripstone only from any dripstone caves next door.
        assert!(n(blocks::DRIPSTONE_BLOCK) < n(blocks::MOSS_BLOCK));
    }

    #[test]
    fn dripstone_caves_grow_dripstone() {
        let terrain = Terrain::new(31);
        let center = find_cave(&terrain, Biome::DripstoneCaves);
        let around = (-1..=1).flat_map(|dz| (-1..=1).map(move |dx| center.offset(dx, dz)));
        let counts = census(&terrain, around);
        let n = |id: crate::world::block::BlockId| counts.get(&id.0).copied().unwrap_or(0);
        assert!(n(blocks::DRIPSTONE_BLOCK) > 50);
        assert!(n(blocks::POINTED_DRIPSTONE) > 5);
        assert!(n(blocks::MOSS_BLOCK) < n(blocks::DRIPSTONE_BLOCK));
    }

    #[test]
    fn spikes_run_from_base_to_tip() {
        let mut chunk = Chunk::new(ChunkPos::new(0, 0));
        chunk.set(3, 40, 3, DRIPSTONE);
        spike(&mut chunk, (3, 39, 3), 4, true);
        let at = |y| chunk.get(3, y, 3);
        assert_eq!(at(39).get(Prop::Thickness), thickness::BASE);
        assert_eq!(at(38).get(Prop::Thickness), thickness::MIDDLE);
        assert_eq!(at(37).get(Prop::Thickness), thickness::FRUSTUM);
        assert_eq!(at(36).get(Prop::Thickness), thickness::TIP);
        assert_eq!(
            at(36).get(Prop::VerticalDirection),
            vertical_direction::DOWN
        );
        assert!(at(35).is_air());
    }

    #[test]
    fn geodes_are_hollow_balls_of_amethyst_in_calcite_and_basalt() {
        let terrain = Terrain::new(2);
        let counts = census(&terrain, (0..64).map(|i| ChunkPos::new(i % 8, i / 8)));
        let n = |id: crate::world::block::BlockId| counts.get(&id.0).copied().unwrap_or(0);
        assert!(n(blocks::AMETHYST_BLOCK) > 0, "no geode in 64 chunks");
        assert!(n(blocks::CALCITE) > n(blocks::AMETHYST_BLOCK) / 2);
        assert!(n(blocks::SMOOTH_BASALT) > 0);
        assert!(n(blocks::BUDDING_AMETHYST) > 0);
    }
}
