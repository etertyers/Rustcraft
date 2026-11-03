//! Sky light and block light, computed by BFS flood fill.
//!
//! Light reaches at most 14 blocks from its source, less than a chunk's
//! width, so a chunk's light depends only on its 3×3 neighborhood. Each mesh
//! job recomputes the center chunk's light from scratch over that area, which
//! makes removal (breaking a torch, opening a roof) and chunk borders correct
//! by construction: there's no stored light to undo or to stitch together.

use std::collections::VecDeque;

use super::{block::BlockState, mesh::BlockModels, mesh::ChunkArea};
use crate::config::{CHUNK_SIZE, MIN_Y, WORLD_HEIGHT};

pub const MAX_LIGHT: u8 = 15;

/// The area spans the center chunk and one chunk on each side.
const AREA: usize = CHUNK_SIZE * 3;
const AREA_OFFSET: i32 = CHUNK_SIZE as i32;
/// The result keeps a one-block border around the chunk for face lighting.
const BORDER: usize = CHUNK_SIZE + 2;

/// Light for one chunk plus a one-block border, as `sky << 4 | block`,
/// up to `height`: above that (a section clear of every block in the 3×3
/// area) there's only full sky light.
#[derive(Debug, Clone, PartialEq)]
pub struct ChunkLight {
    cells: Vec<u8>,
    height: usize,
}

impl ChunkLight {
    /// (sky, block) light at chunk-local x and z in -1..=16 and world `y`.
    /// Above the blocks is full sky light; below the world it's dark.
    pub fn get(&self, x: i32, y: i32, z: i32) -> (u8, u8) {
        let y = y - MIN_Y;
        if y >= self.height as i32 {
            return (MAX_LIGHT, 0);
        }
        if y < 0 {
            return (0, 0);
        }
        debug_assert!(
            (-1..=CHUNK_SIZE as i32).contains(&x) && (-1..=CHUNK_SIZE as i32).contains(&z)
        );
        let i = (y as usize * BORDER + (z + 1) as usize) * BORDER + (x + 1) as usize;
        let cell = self.cells[i];
        (cell >> 4, cell & 0xF)
    }
}

fn area_index(x: usize, y: usize, z: usize) -> usize {
    (y * AREA + z) * AREA + x
}

/// Computes light for the center chunk of `area`. Missing neighbor chunks
/// count as empty. Heights here count from `MIN_Y`.
pub fn light_chunk(models: &BlockModels, area: &ChunkArea) -> ChunkLight {
    // Up to a section past the highest blocks, which block light can't
    // cross; everything above is open sky.
    let height = ((area.top_section() + 1) * CHUNK_SIZE).min(WORLD_HEIGHT);
    // The sections under the lowest block are air: the Nether and End sit
    // well above the bottom of the 384-tall world and needn't be walked.
    let bottom = (area.bottom_section() * CHUNK_SIZE).min(height);
    let volume = AREA * AREA * height;
    let mut opacity = vec![0u8; volume];
    let mut sky = vec![0u8; volume];
    let mut block = vec![0u8; volume];
    // Lowest Y of each column's open sky (above the topmost block that dims light).
    let mut open_from = vec![0usize; AREA * AREA];
    let mut sky_queue = VecDeque::new();
    let mut block_queue = VecDeque::new();

    // Each column's light so far, going down, and whether it is still open.
    let mut column_level = vec![MAX_LIGHT; AREA * AREA];
    let mut column_open = vec![true; AREA * AREA];
    let first_band = bottom / CHUNK_SIZE;
    let end_band = height / CHUNK_SIZE;
    for band in (first_band..end_band).rev() {
        let (y_low, y_high) = (band * CHUNK_SIZE, (band + 1) * CHUNK_SIZE);
        for cz in 0..3 {
            for cx in 0..3 {
                let air = area.section_is_air(cx, cz, band);
                for lz in 0..CHUNK_SIZE {
                    for lx in 0..CHUNK_SIZE {
                        let (x, z) = (cx * CHUNK_SIZE + lx, cz * CHUNK_SIZE + lz);
                        let col = z * AREA + x;
                        let mut level = column_level[col];
                        if air {
                            // Open air: the light just goes down (and none
                            // at all needs writing).
                            if level > 0 {
                                for y in y_low..y_high {
                                    sky[area_index(x, y, z)] = level;
                                }
                            }
                            continue;
                        }
                        let (ax, az) = (x as i32 - AREA_OFFSET, z as i32 - AREA_OFFSET);
                        for y in (y_low..y_high).rev() {
                            let state = area
                                .get(ax, y as i32 + MIN_Y, az)
                                .unwrap_or(BlockState::AIR);
                            if state.is_air() {
                                sky[area_index(x, y, z)] = level;
                                continue;
                            }
                            let model = models.get(state);
                            let (block_opacity, emission) = (model.opacity, model.emission);
                            let i = area_index(x, y, z);
                            opacity[i] = block_opacity;
                            level = level.saturating_sub(block_opacity);
                            sky[i] = level;
                            if column_open[col] && block_opacity > 0 {
                                column_open[col] = false;
                                open_from[col] = y + 1;
                            }
                            if emission > 0 {
                                block[i] = emission;
                                block_queue.push_back(i as u32);
                            }
                        }
                        column_level[col] = level;
                    }
                }
            }
        }
    }
    // Whatever light is left goes straight down through the air under
    // the lowest block.
    for (col, &level) in column_level.iter().enumerate() {
        if level > 0 {
            let (x, z) = (col % AREA, col / AREA);
            for y in 0..bottom {
                sky[area_index(x, y, z)] = level;
            }
        }
    }

    // Sunlit cells next to shade (a neighbor column's surface is higher, or
    // this column is dimmed below) start the sideways spread.
    for z in 0..AREA {
        for x in 0..AREA {
            let mut top = open_from[z * AREA + x];
            for (dx, dz) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
                let (nx, nz) = (x as i32 + dx, z as i32 + dz);
                if (0..AREA as i32).contains(&nx) && (0..AREA as i32).contains(&nz) {
                    top = top.max(open_from[nz as usize * AREA + nx as usize]);
                }
            }
            for y in 0..top.min(height) {
                let i = area_index(x, y, z);
                if sky[i] > 1 {
                    sky_queue.push_back(i as u32);
                }
            }
        }
    }

    spread(&mut sky, &opacity, sky_queue, height);
    // An emitter boxed in by rock and emitters as bright (deep lava) can't
    // light anything its neighbors don't already: leave it out.
    let block_queue: VecDeque<u32> = block_queue
        .into_iter()
        .filter(|&i| lights_a_neighbor(i as usize, &block, &opacity, height))
        .collect();
    spread(&mut block, &opacity, block_queue, height);

    let mut cells = vec![0u8; BORDER * BORDER * height];
    for y in 0..height {
        for z in 0..BORDER {
            for x in 0..BORDER {
                // Border cell 0 is area cell CHUNK_SIZE - 1.
                let i = area_index(x + CHUNK_SIZE - 1, y, z + CHUNK_SIZE - 1);
                cells[(y * BORDER + z) * BORDER + x] = sky[i] << 4 | block[i];
            }
        }
    }
    ChunkLight { cells, height }
}

/// Whether the emitter at `i` has a neighbor it could light: one that
/// isn't opaque and isn't itself an emitter at least as bright.
fn lights_a_neighbor(i: usize, levels: &[u8], opacity: &[u8], height: usize) -> bool {
    let (x, z, y) = (i % AREA, (i / AREA) % AREA, i / (AREA * AREA));
    let own = levels[i];
    let mut neighbors = [None; 6];
    if x > 0 {
        neighbors[0] = Some(i - 1);
    }
    if x + 1 < AREA {
        neighbors[1] = Some(i + 1);
    }
    if z > 0 {
        neighbors[2] = Some(i - AREA);
    }
    if z + 1 < AREA {
        neighbors[3] = Some(i + AREA);
    }
    if y > 0 {
        neighbors[4] = Some(i - AREA * AREA);
    }
    if y + 1 < height {
        neighbors[5] = Some(i + AREA * AREA);
    }
    neighbors
        .into_iter()
        .flatten()
        .any(|n| opacity[n] < 15 && levels[n] < own)
}

/// Floods light outward from the queued cells: each step loses 1, or the
/// entered block's opacity if that's more.
fn spread(levels: &mut [u8], opacity: &[u8], mut queue: VecDeque<u32>, height: usize) {
    const STEPS: [(i32, i32, i32); 6] = [
        (1, 0, 0),
        (-1, 0, 0),
        (0, 1, 0),
        (0, -1, 0),
        (0, 0, 1),
        (0, 0, -1),
    ];
    let (w, h) = (AREA as i32, height as i32);
    while let Some(i) = queue.pop_front() {
        let i = i as usize;
        let level = levels[i];
        if level <= 1 {
            continue;
        }
        let x = (i % AREA) as i32;
        let z = ((i / AREA) % AREA) as i32;
        let y = (i / (AREA * AREA)) as i32;
        for (dx, dy, dz) in STEPS {
            let (nx, ny, nz) = (x + dx, y + dy, z + dz);
            if nx < 0 || nx >= w || nz < 0 || nz >= w || ny < 0 || ny >= h {
                continue;
            }
            let n = area_index(nx as usize, ny as usize, nz as usize);
            let next = level.saturating_sub(opacity[n].max(1));
            if next > levels[n] {
                levels[n] = next;
                queue.push_back(n as u32);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::blocks;
    use crate::world::{
        biome::BiomeColors,
        chunk::{Chunk, ChunkPos},
    };
    use std::{collections::HashMap, sync::Arc};

    const STONE: BlockState = BlockState::of(blocks::STONE);
    const GLOWSTONE: BlockState = BlockState::of(blocks::GLOWSTONE);
    const TORCH: BlockState = BlockState::of(blocks::TORCH);
    const LEAVES: BlockState = BlockState::of(blocks::OAK_LEAVES);
    const WATER: BlockState = BlockState::of(blocks::WATER);
    const GLASS: BlockState = BlockState::of(blocks::GLASS);

    fn models() -> BlockModels {
        let registry = crate::world::block::registry();
        BlockModels::new(registry, |_| None, [0.0; 4], BiomeColors::uniform())
    }

    /// A 3×3 chunk area centered on chunk (0, 0), with `fill` choosing each
    /// block by world coordinates.
    fn area(fill: impl Fn(i32, i32, i32) -> BlockState) -> ChunkArea {
        let mut chunks = HashMap::new();
        for cz in -1..=1 {
            for cx in -1..=1 {
                let pos = ChunkPos::new(cx, cz);
                let mut chunk = Chunk::new(pos);
                let (min_x, min_z) = pos.min_block();
                for y in crate::config::MIN_Y..crate::config::MAX_Y {
                    for z in 0..CHUNK_SIZE {
                        for x in 0..CHUNK_SIZE {
                            let block = fill(min_x + x as i32, y, min_z + z as i32);
                            chunk.set(x, y, z, block);
                        }
                    }
                }
                chunks.insert(pos, Arc::new(chunk));
            }
        }
        let center = chunks.remove(&ChunkPos::new(0, 0)).unwrap();
        ChunkArea::new(center, |pos| chunks.get(&pos).cloned())
    }

    /// Stone up to y 63, with `extra` overriding single blocks.
    fn ground(extra: &[(i32, i32, i32, BlockState)]) -> ChunkArea {
        let extra = extra.to_vec();
        area(move |x, y, z| {
            if let Some(&(.., b)) = extra
                .iter()
                .find(|&&(ex, ey, ez, _)| (ex, ey, ez) == (x, y, z))
            {
                b
            } else if y <= 63 {
                STONE
            } else {
                BlockState::AIR
            }
        })
    }

    #[test]
    fn open_sky_is_full_and_solid_ground_is_dark() {
        let light = light_chunk(&models(), &ground(&[]));
        assert_eq!(light.get(5, 64, 5), (15, 0));
        assert_eq!(light.get(5, 127, 5), (15, 0));
        assert_eq!(light.get(5, 63, 5), (0, 0));
        assert_eq!(light.get(5, 255, 5), (15, 0));
        assert_eq!(light.get(5, crate::config::MAX_Y, 5), (15, 0));
    }

    #[test]
    fn blocks_at_the_top_of_the_world_are_lit_like_any_other() {
        let top = crate::config::MAX_Y - 1;
        let tower: Vec<_> = (64..=top).map(|y| (5, y, 5, STONE)).collect();
        let mut with_torch = tower.clone();
        with_torch.push((6, top, 5, TORCH));
        let light = light_chunk(&models(), &ground(&with_torch));
        assert_eq!(light.get(5, 200, 6), (15, 0), "beside the tower, open sky");
        assert_eq!(light.get(7, top, 5).1, 13, "a torch at the top spreads");
        assert_eq!(light.get(5, top + 1, 5), (15, 0));
    }

    #[test]
    fn a_sealed_cave_is_dark() {
        // A pocket of air deep underground.
        let cave: Vec<_> = (3..8).map(|x| (x, 30, 5, BlockState::AIR)).collect();
        let light = light_chunk(&models(), &ground(&cave));
        for x in 3..8 {
            assert_eq!(light.get(x, 30, 5), (0, 0));
        }
    }

    #[test]
    fn sky_light_spreads_sideways_under_an_overhang_losing_one_per_block() {
        // A shaft down from the surface at x = 0 into a roofed tunnel at y 60.
        let mut edits: Vec<_> = (60..=63).map(|y| (0, y, 5, BlockState::AIR)).collect();
        edits.extend((1..10).map(|x| (x, 60, 5, BlockState::AIR)));
        let light = light_chunk(&models(), &ground(&edits));
        // The shaft is open to the sky; each block into the tunnel loses one.
        assert_eq!(light.get(0, 60, 5).0, 15);
        for x in 1..10 {
            assert_eq!(light.get(x, 60, 5).0, 15 - x as u8, "x = {x}");
        }
    }

    #[test]
    fn a_torch_lights_a_cave_and_fades_with_distance() {
        let mut cave: Vec<_> = (0..16).map(|x| (x, 30, 5, BlockState::AIR)).collect();
        cave[2] = (2, 30, 5, TORCH);
        let light = light_chunk(&models(), &ground(&cave));
        assert_eq!(light.get(2, 30, 5), (0, 14));
        assert_eq!(light.get(3, 30, 5), (0, 13));
        assert_eq!(light.get(15, 30, 5), (0, 1));
        // The stone around the tunnel is dark.
        assert_eq!(light.get(3, 31, 5), (0, 0));
    }

    #[test]
    fn opaque_emitters_light_their_neighbors() {
        let light = light_chunk(&models(), &ground(&[(4, 64, 4, GLOWSTONE)]));
        assert_eq!(light.get(4, 64, 4).1, 15);
        assert_eq!(light.get(5, 64, 4).1, 14);
        assert_eq!(light.get(4, 65, 4).1, 14);
    }

    #[test]
    fn light_crosses_chunk_borders() {
        // A torch in the west neighbor, 3 blocks from the border.
        let mut cave: Vec<_> = (-8..8).map(|x| (x, 30, 5, BlockState::AIR)).collect();
        cave[5] = (-3, 30, 5, TORCH);
        let light = light_chunk(&models(), &ground(&cave));
        assert_eq!(light.get(-1, 30, 5).1, 12);
        assert_eq!(light.get(0, 30, 5).1, 11);
        assert_eq!(light.get(7, 30, 5).1, 4);
    }

    #[test]
    fn leaves_and_water_dim_sky_light() {
        let light = light_chunk(
            &models(),
            &ground(&[
                (3, 66, 3, LEAVES),
                (3, 65, 3, LEAVES),
                (8, 63, 8, WATER),
                (8, 62, 8, WATER),
                (12, 64, 12, GLASS),
            ]),
        );
        // Two leaves dim the column to 13, but open air beside it gives 14.
        assert_eq!(light.get(3, 64, 3).0, 14);
        assert_eq!(light.get(3, 65, 3).0, 14);
        // Water absorbs 1 per block (modern; 1.0.0's was 3).
        assert_eq!(light.get(8, 63, 8).0, 14);
        assert_eq!(light.get(8, 62, 8).0, 13);
        // Glass is clear.
        assert_eq!(light.get(12, 64, 12).0, 15);
    }

    #[test]
    fn removing_a_torch_leaves_no_light_behind() {
        let mut cave: Vec<_> = (0..8).map(|x| (x, 30, 5, BlockState::AIR)).collect();
        let models = models();
        cave[3] = (3, 30, 5, TORCH);
        assert!(light_chunk(&models, &ground(&cave)).get(4, 30, 5).1 > 0);
        cave[3] = (3, 30, 5, BlockState::AIR);
        let light = light_chunk(&models, &ground(&cave));
        assert!((0..8).all(|x| light.get(x, 30, 5) == (0, 0)));
    }
}
