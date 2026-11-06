//! Filling LOD tiles: from real chunks (exact at level 0) and from the
//! fast path for land nobody has loaded yet.
//!
//! The fast path asks `Terrain::lod_surface` — the same noise router,
//! climate and surface rules as real generation — at the sample's column.
//! It leaves out carvers and structures, and draws trees only as colored
//! canopy shapes.

use super::colors::{Class, ColorTable};
use super::data::{Material, Sample, Slice, Tile, TileKey};
use super::{Quality, TILE_SAMPLES};
use crate::config::{CHUNK_SIZE, MAX_Y, MIN_Y, SEA_LEVEL};
use crate::world::biome::Biome;
use crate::world::block::BlockState;
use crate::world::blocks;
use crate::world::chunk::{Chunk, ChunkPos};
use crate::world::worldgen::terrain::Terrain;

/// Overhangs are thin runs of rock, at least this much air above the
/// ground under them.
const OVERHANG_GAP: i32 = 6;
const OVERHANG_MAX_THICKNESS: i32 = 10;

/// How much of a biome's ground its tree canopy covers (0–1) and how tall
/// the trees stand.
fn canopy(biome: Biome) -> (f32, i32) {
    match biome {
        Biome::Forest | Biome::BirchForest => (0.6, 6),
        Biome::DarkForest => (0.9, 8),
        Biome::Taiga | Biome::SnowyTaiga => (0.45, 9),
        Biome::Jungle => (0.8, 14),
        Biome::BambooJungle => (0.3, 10),
        Biome::MangroveSwamp => (0.5, 6),
        Biome::Swamp => (0.15, 6),
        Biome::CherryGrove => (0.2, 6),
        Biome::Grove => (0.25, 8),
        Biome::Savanna => (0.04, 6),
        Biome::WindsweptHills => (0.05, 7),
        Biome::Plains | Biome::Meadow => (0.015, 6),
        _ => (0.0, 0),
    }
}

fn hash(x: i32, z: i32, salt: u64) -> f32 {
    let mut h = (x as u64) << 32 ^ (z as u32 as u64) ^ salt.wrapping_mul(0x9E3779B97F4A7C15);
    h ^= h >> 33;
    h = h.wrapping_mul(0xff51afd7ed558ccd);
    h ^= h >> 33;
    h = h.wrapping_mul(0xc4ceb9fe1a85ec53);
    h ^= h >> 33;
    (h >> 40) as f32 / (1u64 << 24) as f32
}

/// One column from the terrain alone.
pub fn fast_sample(terrain: &Terrain, x: i32, z: i32, step: i32, quality: Quality) -> Sample {
    let (height, biome, top) = terrain.lod_surface(x, z);
    let mut sample = Sample::EMPTY;
    let pool = top.id() == blocks::WATER;
    let ground = if pool {
        BlockState::of(blocks::MUD)
    } else {
        top
    };
    let (cover, tree_height) = canopy(biome);
    let grassy = matches!(top.id(), blocks::GRASS_BLOCK | blocks::PODZOL);
    let treed = grassy && height >= SEA_LEVEL && cover > 0.0;
    // Coarse samples are too big for single trees: tint the ground with
    // leaf green instead.
    let blend = if treed && step >= 8 {
        (cover * 0.7 * 255.0) as u8
    } else {
        0
    };
    let bi = biome.index() as u8;
    sample.push(Slice {
        top: height as i16,
        bottom: MIN_Y as i16,
        block: ground.id().index() as u16,
        biome: bi,
        blend,
        material: Material::Ground,
        light: 15,
    });
    if height < SEA_LEVEL || pool {
        // Frozen oceans are ice at the surface, as real chunks are.
        let surface = if matches!(biome, Biome::FrozenOcean | Biome::DeepFrozenOcean) {
            blocks::ICE
        } else {
            blocks::WATER
        };
        sample.push(Slice {
            top: SEA_LEVEL as i16,
            bottom: (height + 1).min(SEA_LEVEL) as i16,
            block: surface.index() as u16,
            biome: bi,
            blend: 0,
            material: Material::Water,
            light: 15,
        });
    } else if treed && step < 8 && hash(x, z, 77) < cover {
        let jitter = (hash(x, z, 5) * 3.0) as i32 - 1;
        sample.push(Slice {
            top: (height + tree_height + jitter) as i16,
            bottom: (height + 3) as i16,
            block: blocks::OAK_LEAVES.index() as u16,
            biome: bi,
            blend: 0,
            material: Material::Leaves,
            light: 15,
        });
    }
    let keep = quality.slices();
    sample.count = sample.count.min(keep as u8);
    sample
}

/// One column from a column accessor `get(y)` (a real chunk's), scanning
/// down from `top`. Exact: `ground` is the topmost solid block.
pub fn scan_column(
    get: impl Fn(i32) -> BlockState,
    top: i32,
    biome: Biome,
    colors: &ColorTable,
    quality: Quality,
) -> Sample {
    let mut sample = Sample::EMPTY;
    sample.real = true;
    let class = |y: i32| colors.class(get(y));
    let mut y = top.min(MAX_Y - 1);
    while y >= MIN_Y && class(y) == Class::Skip {
        y -= 1;
    }
    if y < MIN_Y {
        return sample;
    }
    // What lies over the ground: water, or a tree's canopy.
    let mut cover: Option<Slice> = None;
    match class(y) {
        Class::Water => {
            let block = get(y).id().index() as u16;
            let water_top = y;
            while y >= MIN_Y && matches!(class(y), Class::Water | Class::Skip) {
                y -= 1;
            }
            cover = Some(Slice {
                top: water_top as i16,
                bottom: (y + 1) as i16,
                block,
                biome: biome.index() as u8,
                blend: 0,
                material: Material::Water,
                light: 15,
            });
        }
        Class::Leaves => {
            let canopy_top = y;
            let mut lowest = y;
            let block = get(y).id().index() as u16;
            while y >= MIN_Y && matches!(class(y), Class::Leaves | Class::Log | Class::Skip) {
                if class(y) == Class::Leaves {
                    lowest = y;
                }
                y -= 1;
            }
            cover = Some(Slice {
                top: canopy_top as i16,
                bottom: lowest as i16,
                block,
                biome: biome.index() as u8,
                blend: 0,
                material: Material::Leaves,
                light: 15,
            });
        }
        _ => {}
    }
    if y < MIN_Y {
        return sample;
    }
    let ground_y = y;
    // Thin rock over a gap over the real ground: an overhang (High only).
    // It sits above the cover, so look for it before the ground is final.
    sample.push(Slice {
        top: ground_y as i16,
        bottom: MIN_Y as i16,
        block: get(ground_y).id().index() as u16,
        biome: biome.index() as u8,
        blend: 0,
        material: Material::Ground,
        light: 15,
    });
    if let Some(cover) = cover {
        sample.push(cover);
    }
    let mut sample = if quality == Quality::High && cover.is_none() {
        add_overhang(sample, &get, ground_y, biome, colors)
    } else {
        sample
    };
    sample.count = sample.count.min(quality.slices() as u8);
    sample
}

/// If the topmost solid run is thin and floats over a gap, it is an
/// overhang above the ground below; the ground becomes the lower slice.
fn add_overhang(
    mut sample: Sample,
    get: &impl Fn(i32) -> BlockState,
    top: i32,
    biome: Biome,
    colors: &ColorTable,
) -> Sample {
    let solid = |y: i32| colors.class(get(y)) == Class::Ground;
    let mut y = top;
    while y >= MIN_Y && solid(y) {
        y -= 1;
    }
    let run_bottom = y + 1;
    if top - run_bottom + 1 > OVERHANG_MAX_THICKNESS {
        return sample;
    }
    let gap_top = y;
    while y >= MIN_Y && !solid(y) {
        y -= 1;
    }
    if y < MIN_Y || gap_top - y < OVERHANG_GAP {
        return sample;
    }
    // The ground is the run found below the gap; the thin run is on top.
    let overhang = Slice {
        top: top as i16,
        bottom: run_bottom as i16,
        block: get(top).id().index() as u16,
        biome: biome.index() as u8,
        blend: 0,
        material: Material::Overhang,
        light: 15,
    };
    sample.slices[0].top = y as i16;
    sample.slices[0].block = get(y).id().index() as u16;
    sample.push(overhang);
    sample
}

/// A chunk's column sample, local x and z in 0..16.
pub fn chunk_sample(
    chunk: &Chunk,
    colors: &ColorTable,
    lx: usize,
    lz: usize,
    quality: Quality,
) -> Sample {
    let top = chunk.height_bound() - 1;
    scan_column(
        |y| chunk.get(lx, y, lz),
        top,
        chunk.biome(lx, lz),
        colors,
        quality,
    )
}

/// A whole tile from the terrain's fast path.
pub fn fast_tile(terrain: &Terrain, key: TileKey, quality: Quality) -> Tile {
    let mut tile = Tile::empty(key);
    for j in 0..TILE_SAMPLES {
        for i in 0..TILE_SAMPLES {
            let (x, z) = key.sample_block(i, j);
            tile.set(i, j, fast_sample(terrain, x, z, key.step(), quality));
        }
    }
    tile
}

/// A chunk's 256 column samples, `z * 16 + x`.
pub fn chunk_columns(chunk: &Chunk, colors: &ColorTable, quality: Quality) -> Vec<Sample> {
    (0..CHUNK_SIZE * CHUNK_SIZE)
        .map(|n| chunk_sample(chunk, colors, n % CHUNK_SIZE, n / CHUNK_SIZE, quality))
        .collect()
}

/// Writes a real chunk's columns (from `chunk_columns`) into `tile`
/// wherever a sample's cell middle lies in the chunk. Returns how many
/// samples it replaced.
pub fn patch_from_columns(tile: &mut Tile, pos: ChunkPos, columns: &[Sample]) -> usize {
    let size = CHUNK_SIZE as i32;
    let (cx, cz) = pos.min_block();
    let mut replaced = 0;
    for j in 0..TILE_SAMPLES {
        for i in 0..TILE_SAMPLES {
            let (x, z) = tile.key.sample_block(i, j);
            if (cx..cx + size).contains(&x) && (cz..cz + size).contains(&z) {
                let at = (z - cz) as usize * CHUNK_SIZE + (x - cx) as usize;
                tile.set(i, j, columns[at]);
                replaced += 1;
            }
        }
    }
    replaced
}

/// `patch_from_columns` straight from a chunk.
pub fn patch_from_chunk(
    tile: &mut Tile,
    chunk: &Chunk,
    colors: &ColorTable,
    quality: Quality,
) -> usize {
    patch_from_columns(tile, chunk.pos, &chunk_columns(chunk, colors, quality))
}

/// The tiles, one per level, that a chunk's columns land in (a chunk is
/// 16 blocks and tiles are at least 32, so it lies inside one).
pub fn tiles_of_chunk(pos: ChunkPos, finest: u8, coarsest: u8) -> Vec<TileKey> {
    let (x, z) = pos.min_block();
    (finest..=coarsest)
        .map(|level| TileKey::containing(level, x, z))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::biome::BiomeColors;

    fn table() -> ColorTable {
        ColorTable::new(None, BiomeColors::uniform())
    }

    /// The first solid block from the top that is neither plant, water,
    /// leaves nor wood, found the plain way.
    fn plain_ground(chunk: &Chunk, colors: &ColorTable, lx: usize, lz: usize) -> Option<i32> {
        (MIN_Y..MAX_Y)
            .rev()
            .find(|&y| colors.class(chunk.get(lx, y, lz)) == Class::Ground)
    }

    fn chunks() -> (Terrain, Vec<Chunk>) {
        let terrain = Terrain::new(12345);
        let chunks = [(0, 0), (3, -2), (-6, 5), (10, 10), (-9, -9), (20, 4)]
            .iter()
            .map(|&(x, z)| terrain.generate(ChunkPos::new(x, z)))
            .collect();
        (terrain, chunks)
    }

    #[test]
    fn finest_level_heights_match_real_chunks_over_1000_columns() {
        let colors = table();
        let (_, chunks) = chunks();
        let mut checked = 0;
        for chunk in &chunks {
            for lz in 0..CHUNK_SIZE {
                for lx in 0..CHUNK_SIZE {
                    let s = chunk_sample(chunk, &colors, lx, lz, Quality::High);
                    let expected = plain_ground(chunk, &colors, lx, lz);
                    // An overhang lifts the ground slice's top to the run
                    // below the gap, so compare with the highest slice.
                    let got = s
                        .slices()
                        .iter()
                        .find(|sl| {
                            sl.material != Material::Water && sl.material != Material::Leaves
                        })
                        .map(|sl| i32::from(sl.top));
                    let top = s.highest().filter(|_| {
                        s.slices()
                            .iter()
                            .any(|sl| sl.material == Material::Overhang)
                    });
                    match (expected, top.or(got)) {
                        (Some(e), Some(g)) => assert_eq!(e, g, "column {lx},{lz}"),
                        (None, None) => {}
                        other => panic!("column {lx},{lz}: {other:?}"),
                    }
                    assert!(s.real);
                    checked += 1;
                }
            }
        }
        assert!(checked >= 1000);
    }

    /// The top of the surface a player would stand on or swim under: the
    /// last slice that isn't a canopy.
    fn surface_top(s: &Sample) -> Option<i32> {
        s.slices()
            .iter()
            .rfind(|sl| sl.material != Material::Leaves && sl.material != Material::Overhang)
            .map(|sl| i32::from(sl.top))
    }

    #[test]
    fn fast_path_heights_match_real_ones_except_where_caves_open_the_surface() {
        let colors = table();
        let (terrain, chunks) = chunks();
        let (mut exact, mut n) = (0, 0);
        for chunk in &chunks {
            let (cx, cz) = chunk.pos.min_block();
            for lz in 0..CHUNK_SIZE {
                for lx in 0..CHUNK_SIZE {
                    let (x, z) = (cx + lx as i32, cz + lz as i32);
                    let real = chunk_sample(chunk, &colors, lx, lz, Quality::Medium);
                    let fast = fast_sample(&terrain, x, z, 1, Quality::Medium);
                    let (r, f) = (surface_top(&real).unwrap(), surface_top(&fast).unwrap());
                    n += 1;
                    if r == f {
                        exact += 1;
                    } else {
                        assert!(
                            terrain.column(x, z).entrance,
                            "{x},{z}: real {r}, fast {f}, and no cave opens it"
                        );
                    }
                }
            }
        }
        assert!(exact * 100 >= n * 75, "{exact}/{n} exact");
    }

    #[test]
    fn coarse_fast_samples_are_within_their_size_of_real_columns() {
        let colors = table();
        let (terrain, chunks) = chunks();
        // One sample of 16 blocks covers a chunk: its middle column is
        // within 16 blocks of most columns' heights.
        let (mut close, mut n) = (0, 0);
        for chunk in &chunks {
            let (cx, cz) = chunk.pos.min_block();
            let mid = fast_sample(&terrain, cx + 8, cz + 8, 16, Quality::Medium);
            let m = surface_top(&mid).unwrap();
            for lz in 0..CHUNK_SIZE {
                for lx in 0..CHUNK_SIZE {
                    let real = chunk_sample(chunk, &colors, lx, lz, Quality::Medium);
                    n += 1;
                    close += i32::from((surface_top(&real).unwrap() - m).abs() <= 16);
                }
            }
        }
        assert!(close * 100 >= n * 95, "{close}/{n} within 16 blocks");
    }

    #[test]
    fn water_fills_to_sea_level_and_canopy_stands_above_the_ground() {
        let (terrain, _) = chunks();
        let mut water = 0;
        let mut canopy = 0;
        for x in (-600..600).step_by(7) {
            for z in (-600..600).step_by(7) {
                let s = fast_sample(&terrain, x, z, 1, Quality::High);
                let ground = s.slices()[0];
                for sl in &s.slices()[1..] {
                    match sl.material {
                        Material::Water => {
                            water += 1;
                            assert_eq!(i32::from(sl.top), SEA_LEVEL);
                            assert!(
                                sl.bottom > ground.top || i32::from(ground.top) >= SEA_LEVEL - 1
                            );
                        }
                        Material::Leaves => {
                            canopy += 1;
                            assert!(sl.bottom > ground.top && sl.top >= sl.bottom);
                        }
                        _ => {}
                    }
                }
            }
        }
        assert!(water > 0 && canopy > 0, "water {water}, canopy {canopy}");
    }

    #[test]
    fn a_real_chunk_replaces_fast_samples_only_inside_itself() {
        let colors = table();
        let (terrain, chunks) = chunks();
        let chunk = &chunks[0];
        for level in 0..=3u8 {
            let (cx, cz) = chunk.pos.min_block();
            let key = TileKey::containing(level, cx, cz);
            let mut tile = fast_tile(&terrain, key, Quality::Medium);
            let before = tile.clone();
            let n = patch_from_chunk(&mut tile, chunk, &colors, Quality::Medium);
            let per_side = (CHUNK_SIZE as i32 / key.step()).max(1) as usize;
            let expected = if key.step() <= 16 {
                per_side * per_side
            } else {
                1
            };
            assert!(n <= expected, "level {level}: replaced {n}");
            for (i, (a, b)) in tile.samples.iter().zip(&before.samples).enumerate() {
                if a != b {
                    assert!(a.real);
                    let (x, z) = key.sample_block(i % TILE_SAMPLES, i / TILE_SAMPLES);
                    assert!((cx..cx + 16).contains(&x) && (cz..cz + 16).contains(&z));
                }
            }
        }
    }
}
