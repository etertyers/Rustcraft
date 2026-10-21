//! The basalt deltas' and soul sand valleys' features, after modern
//! `delta`, `small_basalt_columns`, `large_basalt_columns`, `basalt_blobs`,
//! `blackstone_blobs` and `basalt_pillar`. Every count lives in `TABLE`.
//!
//! Like all the Nether's features they work from the bare terrain, so each
//! chunk replays the same placements for its neighbors and keeps what lands
//! in it. Each placement draws everything from a seed of its own, so a
//! chunk that skips a far one stays in step with the rest.

use super::*;

/// A cluster of basalt columns on every floor layer.
pub struct Columns {
    pub per_layer: u32,
    pub height: (i32, i32),
    pub reach: (i32, i32),
}

/// A ball of one block replacing netherrack, per chunk.
pub struct Blobs {
    pub count: u32,
    pub radius: (i32, i32),
}

pub struct Table {
    /// Lava pools on every floor layer: tries, size, magma rim width.
    pub delta_per_layer: u32,
    pub delta_size: (i32, i32),
    pub delta_rim: (i32, i32),
    pub small_columns: Columns,
    pub large_columns: Columns,
    pub basalt_blobs: Blobs,
    pub blackstone_blobs: Blobs,
    /// Pillar tries per chunk, at any y.
    pub pillar_tries: u32,
}

pub const TABLE: Table = Table {
    delta_per_layer: 40,
    delta_size: (3, 7),
    delta_rim: (0, 2),
    small_columns: Columns {
        per_layer: 4,
        height: (1, 4),
        reach: (1, 1),
    },
    large_columns: Columns {
        per_layer: 2,
        height: (5, 10),
        reach: (2, 3),
    },
    basalt_blobs: Blobs {
        count: 75,
        radius: (3, 7),
    },
    blackstone_blobs: Blobs {
        count: 25,
        radius: (3, 7),
    },
    pillar_tries: 10,
};

/// Floor layers to try at most (the Nether has a few).
const MAX_LAYERS: u32 = 12;

fn range(rng: &mut Rng, (lo, hi): (i32, i32)) -> i32 {
    between(rng, lo, hi)
}

/// Runs `place` for `count` tries on each floor layer, topmost first, until
/// a layer turns up nowhere (modern `count_on_every_layer`). Only tries in
/// the deltas count, which saves working out the terrain elsewhere. `place`
/// gets the floor block's position and a seed for its own draws.
fn on_every_layer(
    land: &mut Land,
    origin: ChunkPos,
    rng: &mut Rng,
    count: u32,
    mut place: impl FnMut(&mut Land, u64, (i32, i32, i32)),
) {
    for layer in 0..MAX_LAYERS {
        let mut found = false;
        for _ in 0..count {
            let (x, z) = spot(rng, origin);
            let seed = rng.next_u64();
            if land.biome(x, z) != Biome::BasaltDeltas {
                continue;
            }
            if let Some(y) = land.layer(x, z, layer) {
                found = true;
                place(land, seed, (x, y, z));
            }
        }
        if !found {
            break;
        }
    }
}

/// Air or the lava sea at (x, y, z).
fn empty(land: &mut Land, x: i32, y: i32, z: i32) -> bool {
    y < HEIGHT - BEDROCK_LAYERS && !land.solid(x, y, z)
}

/// A floor block with air above and no air on its other five sides: where
/// a lava pool may be (modern `DeltaFeature.isClear`).
fn clear(land: &mut Land, x: i32, y: i32, z: i32) -> bool {
    if !(BEDROCK_LAYERS..HEIGHT - BEDROCK_LAYERS).contains(&y) || !land.solid(x, y, z) {
        return false;
    }
    if !land.open(x, y + 1, z) {
        return false;
    }
    [(1, 0, 0), (-1, 0, 0), (0, 0, 1), (0, 0, -1), (0, -1, 0)]
        .iter()
        .all(|(dx, dy, dz)| !land.open(x + dx, y + dy, z + dz))
}

/// Lava pools rimmed with magma on the deltas' floors.
pub fn deltas(land: &mut Land, out: &mut Out, origin: ChunkPos, rng: &mut Rng) {
    on_every_layer(
        land,
        origin,
        rng,
        TABLE.delta_per_layer,
        |land, seed, (x, y, z)| {
            if land.biome(x, z) != Biome::BasaltDeltas {
                return;
            }
            let mut rng = Rng::new(seed);
            let rimmed = rng.next_f64() < 0.9;
            let (shift_x, shift_z) = if rimmed {
                (
                    range(&mut rng, TABLE.delta_rim),
                    range(&mut rng, TABLE.delta_rim),
                )
            } else {
                (0, 0)
            };
            let rim = rimmed && shift_x != 0 && shift_z != 0;
            let (k, l) = (
                range(&mut rng, TABLE.delta_size),
                range(&mut rng, TABLE.delta_size),
            );
            let farthest = k.max(l);
            if !out.near(x, z, farthest + shift_x.max(shift_z)) {
                return;
            }
            for dz in -l..=l {
                for dx in -k..=k {
                    if dx.abs() + dz.abs() > farthest {
                        continue;
                    }
                    let (bx, bz) = (x + dx, z + dz);
                    if !clear(land, bx, y, bz) {
                        continue;
                    }
                    if rim {
                        out.set(bx, y, bz, MAGMA);
                    }
                    let (lx, lz) = (bx + shift_x, bz + shift_z);
                    if clear(land, lx, y, lz) {
                        out.set(lx, y, lz, LAVA);
                    }
                }
            }
        },
    );
}

/// What a basalt column won't stand on.
fn bad_ground(block: BlockState) -> bool {
    matches!(
        block.id(),
        blocks::LAVA
            | blocks::BEDROCK
            | blocks::MAGMA_BLOCK
            | blocks::SOUL_SAND
            | blocks::NETHER_BRICKS
            | blocks::NETHER_BRICK_FENCE
            | blocks::NETHER_BRICK_STAIRS
            | blocks::NETHER_WART
            | blocks::CHEST
    )
}

/// The cell a column at (x, z) starts in, found from `y` down through air
/// or the lava sea to the first one with ground under it, in `reach` steps
/// (modern `findSurface`); or up through rock to the first air.
fn base_of(land: &mut Land, x: i32, mut y: i32, z: i32, mut reach: i32) -> Option<i32> {
    if empty(land, x, y, z) {
        while y > BEDROCK_LAYERS + 1 && reach > 0 {
            reach -= 1;
            if land.solid(x, y - 1, z) && !bad_ground(land.base(x, y - 1, z)) {
                return Some(y);
            }
            y -= 1;
        }
    } else {
        while y < HEIGHT - BEDROCK_LAYERS && reach > 0 {
            reach -= 1;
            if bad_ground(land.base(x, y, z)) {
                return None;
            }
            if empty(land, x, y, z) {
                return Some(y);
            }
            y += 1;
        }
    }
    None
}

/// One column's cells around (cx, y, cz): `height` blocks tall at the
/// middle, shorter toward the edge of `reach`.
fn column(land: &mut Land, out: &mut Out, (cx, y, cz): (i32, i32, i32), height: i32, reach: i32) {
    for dz in -reach..=reach {
        for dx in -reach..=reach {
            let distance = dx.abs() + dz.abs();
            let (x, z) = (cx + dx, cz + dz);
            if !out.near(x, z, 0) {
                continue;
            }
            let Some(base) = base_of(land, x, y, z, distance.max(1)) else {
                continue;
            };
            // No stacking on what an earlier feature made of the floor.
            if let Some(ground) = out.get(x, base - 1, z)
                && bad_ground(ground)
            {
                continue;
            }
            for i in 0..height - distance / 2 {
                let cell = base + i;
                if cell > ROOF_ROCK {
                    break;
                }
                match out.get(x, cell, z) {
                    Some(block) if block == AIR || (block == LAVA && cell <= LAVA_LEVEL) => {
                        out.set(x, cell, z, upright(blocks::BASALT));
                    }
                    Some(block) if block.is(blocks::BASALT) => {}
                    _ => break,
                }
            }
        }
    }
}

/// Clusters of basalt columns rising from the deltas' floors and lava.
fn clusters(land: &mut Land, out: &mut Out, origin: ChunkPos, rng: &mut Rng, spec: &Columns) {
    on_every_layer(
        land,
        origin,
        rng,
        spec.per_layer,
        |land, seed, (x, floor, z)| {
            let mut rng = Rng::new(seed);
            let height = range(&mut rng, spec.height);
            let tight = rng.next_f64() < 0.9;
            let spread = height.min(if tight { 5 } else { 10 });
            let tries = if tight { 50 } else { 15 };
            let mut cells = Vec::with_capacity(tries);
            for _ in 0..tries {
                let (dx, dz) = (
                    between(&mut rng, -spread, spread),
                    between(&mut rng, -spread, spread),
                );
                let left = height - (dx.abs() + dz.abs());
                if left >= 0 {
                    cells.push((dx, dz, left, range(&mut rng, spec.reach)));
                }
            }
            let y = floor + 1;
            if land.biome(x, z) != Biome::BasaltDeltas
                || !empty(land, x, y, z)
                || !land.solid(x, floor, z)
                || bad_ground(land.base(x, floor, z))
                || !out.near(x, z, spread + spec.reach.1)
            {
                return;
            }
            for (dx, dz, left, reach) in cells {
                column(land, out, (x + dx, y, z + dz), left, reach);
            }
        },
    );
}

pub fn small_columns(land: &mut Land, out: &mut Out, origin: ChunkPos, rng: &mut Rng) {
    clusters(land, out, origin, rng, &TABLE.small_columns);
}

pub fn large_columns(land: &mut Land, out: &mut Out, origin: ChunkPos, rng: &mut Rng) {
    clusters(land, out, origin, rng, &TABLE.large_columns);
}

/// Balls of `block` through the deltas' netherrack (modern
/// `ReplaceBlobsFeature`): found by going down from a random block.
fn blobs(
    land: &mut Land,
    out: &mut Out,
    origin: ChunkPos,
    rng: &mut Rng,
    spec: &Blobs,
    block: BlockState,
) {
    for _ in 0..spec.count {
        let (x, z) = spot(rng, origin);
        let y = between(rng, 1, HEIGHT - 2);
        let seed = rng.next_u64();
        let reach = spec.radius.1;
        if !out.near(x, z, reach) || land.biome(x, z) != Biome::BasaltDeltas {
            continue;
        }
        let Some(y) = (1..=y)
            .rev()
            .find(|&y| land.solid(x, y, z) && land.base(x, y, z) == NETHERRACK)
        else {
            continue;
        };
        let mut rng = Rng::new(seed);
        let (rx, ry, rz) = (
            range(&mut rng, spec.radius),
            range(&mut rng, spec.radius),
            range(&mut rng, spec.radius),
        );
        let farthest = rx.max(ry).max(rz);
        let (x0, z0) = out.chunk.pos.min_block();
        let size = CHUNK_SIZE as i32;
        for bz in (z - rz).max(z0)..=(z + rz).min(z0 + size - 1) {
            for bx in (x - rx).max(x0)..=(x + rx).min(x0 + size - 1) {
                for by in (y - ry).max(1)..=(y + ry).min(HEIGHT - 2) {
                    if (bx - x).abs() + (by - y).abs() + (bz - z).abs() <= farthest {
                        out.replace(bx, by, bz, is_netherrack, block);
                    }
                }
            }
        }
    }
}

pub fn basalt_blobs(land: &mut Land, out: &mut Out, origin: ChunkPos, rng: &mut Rng) {
    blobs(
        land,
        out,
        origin,
        rng,
        &TABLE.basalt_blobs,
        upright(blocks::BASALT),
    );
}

pub fn blackstone_blobs(land: &mut Land, out: &mut Out, origin: ChunkPos, rng: &mut Rng) {
    blobs(land, out, origin, rng, &TABLE.blackstone_blobs, BLACKSTONE);
}

/// A few blocks beside a pillar's end: from (x, y) going `step` along it,
/// each side keeps going while a roll of 10 isn't a miss.
fn skirt(land: &mut Land, out: &mut Out, rng: &mut Rng, (x, y, z): (i32, i32, i32), step: i32) {
    for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
        let (mut cy, mut grows) = (y, true);
        while grows && land.air(x + dx, cy, z + dz) {
            out.set(x + dx, cy, z + dz, upright(blocks::BASALT));
            grows = rng.below(10) < 4;
            cy += step;
        }
    }
}

/// Basalt pillars hanging from the soul sand valleys' ceilings down to the
/// first thing in the way (modern `basalt_pillar`).
pub fn pillars(land: &mut Land, out: &mut Out, origin: ChunkPos, rng: &mut Rng) {
    for _ in 0..TABLE.pillar_tries {
        let (x, z) = spot(rng, origin);
        let y = between(rng, 0, HEIGHT - 1);
        let seed = rng.next_u64();
        if !out.near(x, z, 1) || land.biome(x, z) != Biome::SoulSandValley {
            continue;
        }
        if y > ROOF_ROCK || !land.air(x, y, z) || !land.solid(x, y + 1, z) {
            continue;
        }
        let mut rng = Rng::new(seed);
        let mut bottom = y;
        while land.air(x, bottom - 1, z) {
            bottom -= 1;
        }
        for cy in bottom..=y {
            out.set(x, cy, z, upright(blocks::BASALT));
        }
        skirt(land, out, &mut rng, (x, y, z), -1);
        skirt(land, out, &mut rng, (x, bottom, z), 1);
    }
}
