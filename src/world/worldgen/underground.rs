//! What grows underground: lush caves (moss and glow berries), dripstone
//! caves (dripstone and its spikes) and amethyst geodes.

use super::{
    ores,
    rng::Rng,
    terrain::{Area, Terrain},
};
use crate::{
    config::{CHUNK_SIZE, MAX_Y, MIN_Y},
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
    lava_pools(terrain, area, chunk);
    glow_lichen(terrain, area, chunk);
    let center = chunk.pos;
    for dz in -1..=1 {
        for dx in -1..=1 {
            geode(terrain, center.offset(dx, dz), chunk);
        }
    }
}

const LICHEN_SALT: u64 = 0x11C7;
const POOL_SALT: u64 = 0x9001;
/// Glow lichen patches tried per chunk (modern `glow_lichen`: 104 tries
/// a chunk below the surface), and blocks each spreads to at most.
const LICHEN_TRIES: u32 = 104;
const LICHEN_SPREAD: u32 = 6;
/// One chunk in this many has an underground lava pool (modern
/// `lake_lava_underground`, rarity 9).
const POOL_RARITY: u32 = 9;

/// Glow lichen on the walls and ceilings of caves: each try picks an air
/// block in the rock and covers the solid sides next to it, then a few
/// blocks around.
fn glow_lichen(terrain: &Terrain, area: &Area, chunk: &mut Chunk) {
    use crate::world::{Face, props::Prop};
    let pos = chunk.pos;
    let mut rng = Rng::for_chunk(terrain.seed(), pos.x, pos.z, LICHEN_SALT);
    let (min_x, min_z) = pos.min_block();
    let lichen = BlockState::of(blocks::GLOW_LICHEN);
    for _ in 0..LICHEN_TRIES {
        let (x, z) = (rng.below(16) as i32, rng.below(16) as i32);
        let column = area.get(min_x + x, min_z + z);
        let top = column.height - 10;
        if top <= MIN_Y + 6 {
            continue;
        }
        let y = MIN_Y + 6 + rng.below((top - MIN_Y - 6) as u32) as i32;
        let mut at = (x, y, z);
        for _ in 0..1 + rng.below(LICHEN_SPREAD) {
            let (x, y, z) = at;
            if !(0..16).contains(&x) || !(0..16).contains(&z) {
                break;
            }
            let (lx, lz) = (x as usize, z as usize);
            if !chunk.get(lx, y, lz).is_air() {
                break;
            }
            let mut placed = lichen;
            let mut any = false;
            for face in [Face::North, Face::South, Face::East, Face::West, Face::Top] {
                let [dx, dy, dz] = face.normal();
                let (nx, ny, nz) = (x + dx, y + dy, z + dz);
                let solid = (0..16).contains(&nx)
                    && (0..16).contains(&nz)
                    && rock(chunk.get(nx as usize, ny, nz as usize));
                if solid && rng.below(3) != 0 {
                    placed = placed.with_bool(Prop::side(face), true);
                    any = true;
                }
            }
            if any {
                chunk.set(lx, y, lz, placed);
            }
            let step = [
                (1, 0, 0),
                (-1, 0, 0),
                (0, 0, 1),
                (0, 0, -1),
                (0, 1, 0),
                (0, -1, 0),
            ][rng.below(6) as usize];
            at = (x + step.0, y + step.1, z + step.2);
        }
    }
}

/// Now and then a small pool of lava sunk into the rock underground, with
/// air over it (modern `lake_lava_underground`, smaller). It stays inside
/// the chunk.
fn lava_pools(terrain: &Terrain, area: &Area, chunk: &mut Chunk) {
    let pos = chunk.pos;
    let mut rng = Rng::for_chunk(terrain.seed(), pos.x, pos.z, POOL_SALT);
    if rng.below(POOL_RARITY) != 0 {
        return;
    }
    let (min_x, min_z) = pos.min_block();
    let (cx, cz) = (4 + rng.below(8) as i32, 4 + rng.below(8) as i32);
    let column = area.get(min_x + cx, min_z + cz);
    let top = column.height - 12;
    if top <= MIN_Y + 10 {
        return;
    }
    let cy = MIN_Y + 10 + rng.below((top - MIN_Y - 10) as u32) as i32;
    let (rx, ry, rz) = (2.0 + rng.next_f64() * 2.0, 2.0, 2.0 + rng.next_f64() * 2.0);
    let lava = BlockState::of(blocks::LAVA);
    let inside = |dx: i32, dy: i32, dz: i32| {
        (dx as f64 / rx).powi(2) + (dy as f64 / ry).powi(2) + (dz as f64 / rz).powi(2) < 1.0
    };
    // The pool's lower half is lava where every side and the floor are
    // lava or rock (so nothing flows when the chunk loads); the rest is
    // hollowed out.
    let wet = |dx: i32, dy: i32, dz: i32| inside(dx, dy, dz) && dy <= 0;
    let mut edits = Vec::new();
    for dx in -4i32..=4 {
        for dz in -4i32..=4 {
            for dy in -2i32..=2 {
                if !inside(dx, dy, dz) {
                    continue;
                }
                let (lx, y, lz) = ((cx + dx) as usize, cy + dy, (cz + dz) as usize);
                if chunk.get(lx, y, lz) == BlockState::of(blocks::BEDROCK) {
                    continue;
                }
                let held = |ox: i32, oy: i32, oz: i32| {
                    wet(dx + ox, dy + oy, dz + oz)
                        || (!inside(dx + ox, dy + oy, dz + oz)
                            && rock(chunk.get(
                                (cx + dx + ox) as usize,
                                cy + dy + oy,
                                (cz + dz + oz) as usize,
                            )))
                };
                let block = if wet(dx, dy, dz)
                    && [(1, 0, 0), (-1, 0, 0), (0, 0, 1), (0, 0, -1), (0, -1, 0)]
                        .into_iter()
                        .all(|(ox, oy, oz)| held(ox, oy, oz))
                {
                    lava
                } else if wet(dx, dy, dz) {
                    // Rock where lava would spill.
                    continue;
                } else {
                    BlockState::AIR
                };
                edits.push((lx, y, lz, block));
            }
        }
    }
    for (lx, y, lz, block) in edits {
        chunk.set(lx, y, lz, block);
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
            let top = (column.height - 12).min(MAX_Y - 2);
            for y in MIN_Y + 2..top {
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
fn floor(cave: Biome, chunk: &mut Chunk, (x, y, z): (usize, i32, usize), rng: &mut Rng) {
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
                let length = 1 + rng.below(4) as i32;
                spike(chunk, (x, y, z), length, false);
            }
        }
    }
}

/// The ceiling over the air at `at`.
fn ceiling(cave: Biome, chunk: &mut Chunk, (x, y, z): (usize, i32, usize), rng: &mut Rng) {
    let roll = rng.next_f64();
    match cave {
        Biome::LushCaves => {
            if roll < 0.5 {
                chunk.set(x, y + 1, z, MOSS);
            }
            if roll < 0.12 {
                // Glow berry vines, lit in places.
                let length = 1 + rng.below(6) as i32;
                for k in 0..length {
                    let cy = y - k;
                    if cy < MIN_Y + 2
                        || !chunk.get(x, cy, z).is_air()
                        || !chunk.get(x, cy - 1, z).is_air()
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
                let length = 1 + rng.below(5) as i32;
                spike(chunk, (x, y, z), length, true);
            }
        }
    }
}

/// Pointed dripstone `length` long from `at`, hanging down from the block
/// above (`down`) or standing up from the block below.
fn spike(chunk: &mut Chunk, (x, y, z): (usize, i32, usize), length: i32, down: bool) {
    // Shorten it to the open air it has, leaving a block of air at its tip.
    let mut room = 0;
    while room <= length {
        let cy = if down { y - room } else { y + room };
        if (MIN_Y + 1..MAX_Y - 1).contains(&cy) && chunk.get(x, cy, z).is_air() {
            room += 1;
        } else {
            break;
        }
    }
    let length = length.min((room - 1).max(0));
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

/// A geode's place and size, and the seed of its per-block rolls.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Geode {
    pub center: glam::IVec3,
    pub radius: f64,
    seed: u64,
}

/// Points tested on a geode's outer shell, and how many of them may be
/// open (air, water or lava) before the geode is cancelled: modern
/// `GeodeFeature`'s `invalid_blocks_threshold` idea, sampled more finely.
pub const GEODE_SHELL_POINTS: usize = 64;
pub const GEODE_INVALID_THRESHOLD: usize = 3;

impl Geode {
    /// Points spread evenly over the smooth basalt shell's middle.
    pub fn shell(&self) -> impl Iterator<Item = glam::IVec3> + '_ {
        let n = GEODE_SHELL_POINTS as f64;
        let golden = std::f64::consts::PI * (3.0 - 5.0f64.sqrt());
        (0..GEODE_SHELL_POINTS).map(move |i| {
            let y = 1.0 - 2.0 * (i as f64 + 0.5) / n;
            let r = (1.0 - y * y).sqrt();
            let a = golden * i as f64;
            let d = glam::DVec3::new(a.cos() * r, y, a.sin() * r) * (self.radius + 1.5);
            self.center + d.round().as_ivec3()
        })
    }
}

/// Whether worldgen leaves (x, y, z) open: above the land, or in a cave.
pub fn open_at(terrain: &Terrain, x: i32, y: i32, z: i32) -> bool {
    y > terrain.height(x, z) || terrain.is_cave(x, y, z)
}

/// Whether chunk `pos` rolls a geode at all (one in 24), before the check
/// that cancels exposed ones.
pub fn rolls_geode(terrain: &Terrain, pos: ChunkPos) -> bool {
    let mut rng = Rng::for_chunk(terrain.seed(), pos.x, pos.z, GEODE_SALT);
    rng.below(GEODE_RARITY) == 0
}

/// Chunk `source`'s geode, if it has one and its shell is set in solid
/// rock: a geode with more than `GEODE_INVALID_THRESHOLD` shell points
/// open would hang in a cave, so it isn't made.
pub fn geode_at(terrain: &Terrain, source: ChunkPos) -> Option<Geode> {
    let mut rng = Rng::for_chunk(terrain.seed(), source.x, source.z, GEODE_SALT);
    if rng.below(GEODE_RARITY) != 0 {
        return None;
    }
    let (sx, sz) = source.min_block();
    let cx = sx + rng.below(16) as i32;
    let cz = sz + rng.below(16) as i32;
    let cy = GEODE_LOW + rng.below((GEODE_HIGH - GEODE_LOW) as u32) as i32;
    let radius = 4.0 + rng.next_f64() * 1.5;
    let geode = Geode {
        center: glam::IVec3::new(cx, cy, cz),
        radius,
        seed: rng.next_u64(),
    };
    let shell: Vec<glam::IVec3> = geode.shell().collect();
    let worms = terrain.in_worms(&shell);
    let open = shell
        .iter()
        .zip(worms)
        .filter(|&(p, worm)| worm || open_at(terrain, p.x, p.y, p.z))
        .count();
    (open <= GEODE_INVALID_THRESHOLD).then_some(geode)
}

/// Replays chunk `source`'s geode, if it has one, writing the part inside
/// `chunk` (modern `GeodeFeature`, simplified: a ball of smooth basalt,
/// calcite and amethyst around a hollow, buds and clusters on the floor).
/// Caves cut into it: nothing is written into a carved cell.
fn geode(terrain: &Terrain, source: ChunkPos, chunk: &mut Chunk) {
    let Some(Geode {
        center,
        radius,
        seed,
    }) = geode_at(terrain, source)
    else {
        return;
    };
    let (cx, cy, cz) = (center.x, center.y, center.z);
    let (min_x, min_z) = chunk.pos.min_block();
    let outer = (radius + 3.0).ceil() as i32;
    for dz in -outer..=outer {
        for dx in -outer..=outer {
            let (x, z) = (cx + dx, cz + dz);
            let (lx, lz) = (x - min_x, z - min_z);
            if !(0..16).contains(&lx) || !(0..16).contains(&lz) {
                continue;
            }
            for dy in -outer..=outer {
                let y = cy + dy;
                if !(MIN_Y + 1..MAX_Y - 1).contains(&y) {
                    continue;
                }
                let d = ((dx * dx + dy * dy + dz * dz) as f64).sqrt();
                // A per-block roll, the same whichever chunk asks.
                let mut cell = Rng::for_chunk(seed, x, z, y as u64);
                let wobble = cell.next_f64() * 0.6;
                let d = d - wobble;
                let (lx, ly, lz) = (lx as usize, y, lz as usize);
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
                let carved = here.is_air() || here.def().fluid.is_some();
                if here.id() == blocks::BEDROCK || (carved && !block.is_air()) {
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
                if y < MIN_Y + 1 || y + 1 >= MAX_Y {
                    continue;
                }
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
            for y in crate::config::MIN_Y..crate::config::MAX_Y {
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

    #[test]
    fn caves_get_glow_lichen_and_some_chunks_lava_pools() {
        let terrain = Terrain::new(20111118);
        let counts = census(&terrain, (0..36).map(|i| ChunkPos::new(i % 6, i / 6)));
        let n = |id: crate::world::block::BlockId| counts.get(&id.0).copied().unwrap_or(0);
        assert!(
            n(blocks::GLOW_LICHEN) > 50,
            "{} glow lichen",
            n(blocks::GLOW_LICHEN)
        );
        assert!(n(blocks::LAVA) > 0);
    }
}
