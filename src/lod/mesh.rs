//! Turning a tile into triangles: a top quad for every slice of every
//! sample, and side walls wherever a slice isn't covered by its neighbor.
//! Pure CPU code, run on worker threads.

use std::sync::Arc;

use super::data::{Material, Sample, Tile, TileKey};
use super::{Quality, TILE_SAMPLES};

/// 16 bytes: position, color, and in `alpha` the face (high nibble) and
/// sky light (low nibble).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct LodVertex {
    pub pos: [f32; 3],
    pub rgba: [u8; 4],
}

impl LodVertex {
    pub fn face(&self) -> u8 {
        self.rgba[3] >> 4
    }
}

/// Face numbers in the vertex's alpha.
pub const FACE_TOP: u8 = 0;
pub const FACE_NORTH: u8 = 1; // -z
pub const FACE_SOUTH: u8 = 2; // +z
pub const FACE_WEST: u8 = 3; // -x
pub const FACE_EAST: u8 = 4; // +x

#[derive(Debug, Default, Clone)]
pub struct LodMesh {
    pub vertices: Vec<LodVertex>,
    pub indices: Vec<u32>,
    /// Top quads made, for the tests and the stats.
    pub tops: usize,
}

impl LodMesh {
    pub fn triangles(&self) -> usize {
        self.indices.len() / 3
    }

    pub fn bytes(&self) -> usize {
        self.vertices.len() * std::mem::size_of::<LodVertex>() + self.indices.len() * 4
    }

    fn quad(&mut self, corners: [[f32; 3]; 4], color: [u8; 3], face: u8, light: u8) {
        let base = self.vertices.len() as u32;
        let rgba = [color[0], color[1], color[2], (face << 4) | light.min(15)];
        for pos in corners {
            self.vertices.push(LodVertex { pos, rgba });
        }
        self.indices
            .extend([base, base + 1, base + 2, base, base + 2, base + 3]);
    }
}

/// The blocks the loaded world covers: samples whose cell lies wholly
/// inside draw nothing. `min` is inclusive, `max` exclusive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hole {
    pub min: (i32, i32),
    pub max: (i32, i32),
}

impl Hole {
    pub fn contains_cell(&self, x: i32, z: i32, step: i32) -> bool {
        x >= self.min.0 && z >= self.min.1 && x + step <= self.max.0 && z + step <= self.max.1
    }

    pub fn touches(&self, key: TileKey) -> bool {
        let (ox, oz) = key.origin();
        let span = key.span();
        ox < self.max.0 && ox + span > self.min.0 && oz < self.max.1 && oz + span > self.min.1
    }
}

/// A tile and the four tiles of the same level around it (any may be
/// missing).
pub struct Neighborhood {
    pub tile: Arc<Tile>,
    /// -x, +x, -z, +z.
    pub around: [Option<Arc<Tile>>; 4],
}

impl Neighborhood {
    /// The sample next to (i, j) in direction `dir` (0 -x, 1 +x, 2 -z,
    /// 3 +z).
    fn next(&self, i: usize, j: usize, dir: usize) -> Option<&Sample> {
        let n = TILE_SAMPLES;
        let (di, dj): (isize, isize) = match dir {
            0 => (-1, 0),
            1 => (1, 0),
            2 => (0, -1),
            _ => (0, 1),
        };
        let (ni, nj) = (i as isize + di, j as isize + dj);
        if (0..n as isize).contains(&ni) && (0..n as isize).contains(&nj) {
            return Some(self.tile.get(ni as usize, nj as usize));
        }
        let other = self.around[dir].as_ref()?;
        let (wi, wj) = (ni.rem_euclid(n as isize), nj.rem_euclid(n as isize));
        Some(other.get(wi as usize, wj as usize))
    }
}

/// The part of `[bottom, top]` (block rows) that no slice of `other`
/// covers, as a list of intervals.
fn uncovered(bottom: i32, top: i32, other: &Sample) -> Vec<(i32, i32)> {
    let mut parts = vec![(bottom, top)];
    for s in other.slices() {
        let (b, t) = (i32::from(s.bottom), i32::from(s.top));
        let mut next = Vec::with_capacity(parts.len() + 1);
        for (lo, hi) in parts {
            if t < lo || b > hi {
                next.push((lo, hi));
                continue;
            }
            if b > lo {
                next.push((lo, b - 1));
            }
            if t < hi {
                next.push((t + 1, hi));
            }
        }
        parts = next;
    }
    parts
}

/// Meshes the tile. Slices past `quality`'s count are ignored.
pub fn build(near: &Neighborhood, hole: Option<Hole>, quality: Quality) -> LodMesh {
    let key = near.tile.key;
    let step = key.step();
    let s = step as f32;
    let mut mesh = LodMesh::default();
    for j in 0..TILE_SAMPLES {
        for i in 0..TILE_SAMPLES {
            let sample = near.tile.get(i, j);
            if sample.count == 0 {
                continue;
            }
            let (ox, oz) = key.origin();
            let (x0, z0) = (ox + i as i32 * step, oz + j as i32 * step);
            if hole.is_some_and(|h| h.contains_cell(x0, z0, step)) {
                continue;
            }
            let (fx, fz) = (x0 as f32, z0 as f32);
            let kept = &sample.slices()[..sample.slices().len().min(quality.slices())];
            for slice in kept {
                let top = f32::from(slice.top) + 1.0;
                let light = slice.light;
                mesh.quad(
                    [
                        [fx, top, fz],
                        [fx, top, fz + s],
                        [fx + s, top, fz + s],
                        [fx + s, top, fz],
                    ],
                    slice.color,
                    FACE_TOP,
                    light,
                );
                mesh.tops += 1;
                // Walls, per direction.
                for dir in 0..4 {
                    let Some(next) = near.next(i, j, dir) else {
                        continue;
                    };
                    if next.count == 0 {
                        // Nothing there (no data yet): no wall.
                        continue;
                    }
                    let exposed = uncovered(i32::from(slice.bottom), i32::from(slice.top), next);
                    for (lo, hi) in exposed {
                        // Wall rows below the sample's own bottom for the
                        // ground slice are unbounded: cap at a few cells.
                        let lo = lo.max(hi - 255);
                        wall(&mut mesh, dir, (fx, fz), s, lo, hi, slice.color, light);
                    }
                }
            }
        }
    }
    mesh
}

#[allow(clippy::too_many_arguments)]
fn wall(
    mesh: &mut LodMesh,
    dir: usize,
    (fx, fz): (f32, f32),
    s: f32,
    lo: i32,
    hi: i32,
    color: [u8; 3],
    light: u8,
) {
    let (y0, y1) = (lo as f32, hi as f32 + 1.0);
    let (corners, face) = match dir {
        0 => (
            [
                [fx, y1, fz + s],
                [fx, y1, fz],
                [fx, y0, fz],
                [fx, y0, fz + s],
            ],
            FACE_WEST,
        ),
        1 => (
            [
                [fx + s, y1, fz],
                [fx + s, y1, fz + s],
                [fx + s, y0, fz + s],
                [fx + s, y0, fz],
            ],
            FACE_EAST,
        ),
        2 => (
            [
                [fx, y1, fz],
                [fx + s, y1, fz],
                [fx + s, y0, fz],
                [fx, y0, fz],
            ],
            FACE_NORTH,
        ),
        _ => (
            [
                [fx + s, y1, fz + s],
                [fx, y1, fz + s],
                [fx, y0, fz + s],
                [fx + s, y0, fz + s],
            ],
            FACE_SOUTH,
        ),
    };
    mesh.quad(corners, color, face, light);
}

/// Whether a material is drawn as water (for the shader's tint).
pub fn is_water(material: Material) -> bool {
    material == Material::Water
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lod::data::Slice;

    fn flat(key: TileKey, height: i16) -> Tile {
        let mut tile = Tile::empty(key);
        for j in 0..TILE_SAMPLES {
            for i in 0..TILE_SAMPLES {
                let mut s = Sample::EMPTY;
                s.push(Slice {
                    top: height,
                    bottom: -64,
                    color: [10, 200, 30],
                    material: Material::Ground,
                    light: 15,
                });
                tile.set(i, j, s);
            }
        }
        tile
    }

    fn alone(tile: Tile) -> Neighborhood {
        Neighborhood {
            tile: Arc::new(tile),
            around: [None, None, None, None],
        }
    }

    #[test]
    fn a_flat_tile_is_one_top_quad_per_sample_and_no_walls() {
        let mesh = build(&alone(flat(TileKey::new(1, 0, 0), 70)), None, Quality::High);
        assert_eq!(mesh.tops, TILE_SAMPLES * TILE_SAMPLES);
        assert_eq!(mesh.triangles(), 2 * TILE_SAMPLES * TILE_SAMPLES);
    }

    #[test]
    fn a_step_in_height_gets_a_wall_on_the_higher_side_only() {
        let mut tile = flat(TileKey::new(0, 0, 0), 70);
        let mut low = *tile.get(5, 5);
        low.slices[0].top = 66;
        tile.set(5, 5, low);
        let mesh = build(&alone(tile), None, Quality::High);
        // Four higher neighbors each show 4 blocks of wall (67..=70).
        let walls = mesh
            .vertices
            .iter()
            .filter(|v| v.face() != FACE_TOP)
            .count()
            / 4;
        assert_eq!(walls, 4);
    }

    #[test]
    fn the_loaded_area_is_never_covered_and_the_boundary_has_no_gaps() {
        let key = TileKey::new(1, 0, 0); // 64 blocks, 2 per sample
        let hole = Hole {
            min: (10, 10),
            max: (40, 40),
        };
        let mesh = build(&alone(flat(key, 70)), Some(hole), Quality::High);
        let mut covered = 0;
        for j in 0..TILE_SAMPLES {
            for i in 0..TILE_SAMPLES {
                let (x, z) = (i as i32 * 2, j as i32 * 2);
                let inside = hole.contains_cell(x, z, 2);
                let drawn = mesh.vertices.chunks(4).any(|q| {
                    let cx = q.iter().map(|v| v.pos[0]).sum::<f32>() / 4.0;
                    let cz = q.iter().map(|v| v.pos[2]).sum::<f32>() / 4.0;
                    q[0].face() == FACE_TOP
                        && cx > x as f32
                        && cx < (x + 2) as f32
                        && cz > z as f32
                        && cz < (z + 2) as f32
                });
                assert_eq!(drawn, !inside, "cell {x},{z}");
                covered += i32::from(drawn);
            }
        }
        // Every cell not wholly inside the hole is drawn: no gaps.
        assert_eq!(mesh.tops as i32, covered);
        assert!(covered > 0 && (covered as usize) < TILE_SAMPLES * TILE_SAMPLES);
    }

    #[test]
    fn quality_caps_the_slices_drawn() {
        let mut tile = flat(TileKey::new(0, 0, 0), 70);
        for j in 0..TILE_SAMPLES {
            for i in 0..TILE_SAMPLES {
                let mut s = *tile.get(i, j);
                for k in 0..2 {
                    s.push(Slice {
                        top: 80 + k,
                        bottom: 75,
                        color: [1, 2, 3],
                        material: Material::Leaves,
                        light: 15,
                    });
                }
                tile.set(i, j, s);
            }
        }
        let n = alone(tile);
        assert_eq!(build(&n, None, Quality::Low).tops, 2 * 1024);
        assert_eq!(build(&n, None, Quality::High).tops, 3 * 1024);
    }

    #[test]
    fn walls_across_a_tile_border_use_the_neighbor_tile() {
        let key = TileKey::new(0, 0, 0);
        let east = flat(TileKey::new(0, 1, 0), 60);
        let hood = Neighborhood {
            tile: Arc::new(flat(key, 70)),
            around: [None, Some(Arc::new(east)), None, None],
        };
        let mesh = build(&hood, None, Quality::High);
        let east_walls = mesh
            .vertices
            .iter()
            .filter(|v| v.face() == FACE_EAST)
            .count()
            / 4;
        assert_eq!(east_walls, TILE_SAMPLES);
    }
}
