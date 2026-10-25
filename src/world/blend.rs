//! Biome blend (modern's "Biome Blend" video setting): grass and foliage
//! colors averaged over the columns around each block, so colors fade
//! across a biome border instead of changing in one step. Each mesh job
//! works the averages out once per column; vertices then blend between
//! the columns around them.

use crate::{
    config::CHUNK_SIZE,
    world::{
        biome::{Biome, BiomeColors},
        block::FaceTint,
    },
};

/// Columns -1..=16 of a chunk: one past each side, for the vertices on
/// its edges.
const SIDE: i32 = CHUNK_SIZE as i32 + 2;

/// A chunk's blended grass, foliage and water colors per column.
pub struct Tints {
    radius: i32,
    grass: Vec<[f32; 3]>,
    foliage: Vec<[f32; 3]>,
    water: Vec<[f32; 3]>,
}

impl Tints {
    /// Averages each column's biome colors over the (2 × `radius` + 1)²
    /// columns around it. `biome(x, z)` takes chunk-local columns, up to
    /// `radius + 1` outside the chunk.
    pub fn new(colors: &BiomeColors, radius: i32, biome: impl Fn(i32, i32) -> Biome) -> Self {
        let r = radius.max(0);
        // Running sums of [grass, foliage] over the columns the averages
        // reach, which start at -1 - r; f64 so a 1024-column sum stays
        // exact enough.
        let wide = SIDE + 2 * r;
        let low = -1 - r;
        let row = (wide + 1) as usize;
        let mut sums = vec![[[0.0f64; 3]; 3]; row * row];
        for j in 0..wide as usize {
            for i in 0..wide as usize {
                let b = biome(low + i as i32, low + j as i32);
                let here = [colors.grass(b), colors.foliage(b), colors.water(b)];
                for k in 0..3 {
                    for c in 0..3 {
                        sums[(j + 1) * row + i + 1][k][c] = here[k][c] as f64
                            + sums[(j + 1) * row + i][k][c]
                            + sums[j * row + i + 1][k][c]
                            - sums[j * row + i][k][c];
                    }
                }
            }
        }
        let count = ((2 * r + 1) * (2 * r + 1)) as f64;
        let span = (2 * r + 1) as usize;
        let mut grass = Vec::with_capacity((SIDE * SIDE) as usize);
        let mut foliage = Vec::with_capacity((SIDE * SIDE) as usize);
        let mut water = Vec::with_capacity((SIDE * SIDE) as usize);
        for z in 0..SIDE as usize {
            for x in 0..SIDE as usize {
                // Column (x - 1, z - 1)'s window starts at sum index (x, z).
                let average = |k: usize| {
                    [0, 1, 2].map(|c| {
                        let s = |i: usize, j: usize| sums[j * row + i][k][c];
                        ((s(x + span, z + span) - s(x, z + span) - s(x + span, z) + s(x, z))
                            / count) as f32
                    })
                };
                grass.push(average(0));
                foliage.push(average(1));
                water.push(average(2));
            }
        }
        Self {
            radius: r,
            grass,
            foliage,
            water,
        }
    }

    /// The blended color of chunk-local column (x, z), -1..=16.
    fn column(&self, tint: FaceTint, x: i32, z: i32) -> [f32; 3] {
        let i = ((z + 1) * SIDE + x + 1) as usize;
        match tint {
            FaceTint::None => [1.0; 3],
            FaceTint::Grass => self.grass[i],
            FaceTint::Foliage => self.foliage[i],
            FaceTint::Water => self.water[i],
            FaceTint::Rgb(rgb) => rgb,
        }
    }

    /// One color for a whole block in column (x, z).
    pub fn flat(&self, tint: FaceTint, x: i32, z: i32) -> [f32; 3] {
        self.column(tint, x, z)
    }

    /// The color at chunk-local point (x, z): bilinear between the four
    /// nearest column centers.
    pub fn at(&self, tint: FaceTint, x: f32, z: f32) -> [f32; 3] {
        let last = CHUNK_SIZE as i32 - 1;
        let (u, v) = (x - 0.5, z - 0.5);
        let (i, j) = (
            (u.floor() as i32).clamp(-1, last),
            (v.floor() as i32).clamp(-1, last),
        );
        let (tu, tv) = (
            (u - i as f32).clamp(0.0, 1.0),
            (v - j as f32).clamp(0.0, 1.0),
        );
        let lerp = |a: [f32; 3], b: [f32; 3], t: f32| [0, 1, 2].map(|c| a[c] + (b[c] - a[c]) * t);
        let c = |a: i32, b: i32| self.column(tint, a, b);
        lerp(
            lerp(c(i, j), c(i + 1, j), tu),
            lerp(c(i, j + 1), c(i + 1, j + 1), tu),
            tv,
        )
    }

    /// The color at each corner of a quad on the block in column (x, z),
    /// corners given relative to the block. With blending off, the
    /// block's own column color, as before biome blend.
    pub fn corners(&self, tint: FaceTint, x: i32, z: i32, corners: [[f32; 3]; 4]) -> [[f32; 3]; 4] {
        if self.radius == 0
            || !matches!(tint, FaceTint::Grass | FaceTint::Foliage | FaceTint::Water)
        {
            return [self.column(tint, x, z); 4];
        }
        corners.map(|[cx, _, cz]| self.at(tint, x as f32 + cx, z as f32 + cz))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Colors where every biome differs: the colormap pixel's own
    /// coordinates.
    fn colors() -> BiomeColors {
        let map =
            image::RgbaImage::from_fn(256, 256, |x, y| image::Rgba([x as u8, y as u8, 90, 255]));
        BiomeColors::from_colormaps(&map, &map)
    }

    /// Forest west of x = 8, plains from there east.
    fn border(x: i32, _z: i32) -> Biome {
        if x < 8 { Biome::Forest } else { Biome::Plains }
    }

    #[test]
    fn colors_step_by_at_most_a_window_share_across_a_border() {
        let colors = colors();
        for tint in [FaceTint::Grass, FaceTint::Foliage] {
            let pure = |b: Biome| match tint {
                FaceTint::Grass => colors.grass(b),
                _ => colors.foliage(b),
            };
            let (forest, plains) = (pure(Biome::Forest), pure(Biome::Plains));
            assert_ne!(forest, plains, "the test needs two colors");
            for radius in 0..=7 {
                let tints = Tints::new(&colors, radius, border);
                let most =
                    |c: usize| (forest[c] - plains[c]).abs() / (2 * radius + 1) as f32 + 1e-5;
                // Neighboring blocks, and points a quarter block apart.
                for x in -1..CHUNK_SIZE as i32 {
                    let (a, b) = (tints.flat(tint, x, 3), tints.flat(tint, x + 1, 3));
                    for c in 0..3 {
                        assert!(
                            (a[c] - b[c]).abs() <= most(c),
                            "r {radius}, x {x}: {a:?} then {b:?}"
                        );
                    }
                }
                for q in 0..64 {
                    let (x0, x1) = (q as f32 / 4.0, (q + 1) as f32 / 4.0);
                    let (a, b) = (tints.at(tint, x0, 3.5), tints.at(tint, x1, 3.5));
                    for c in 0..3 {
                        assert!((a[c] - b[c]).abs() <= most(c) / 4.0 + 1e-5);
                    }
                }
                // Columns 0 and 15 are 8 from the border, beyond any
                // window, so they keep their own biome's color.
                let (west, east) = (tints.flat(tint, 0, 3), tints.flat(tint, 15, 3));
                for c in 0..3 {
                    assert!((west[c] - forest[c]).abs() < 1e-5, "r {radius}");
                    assert!((east[c] - plains[c]).abs() < 1e-5, "r {radius}");
                }
            }
        }
    }

    #[test]
    fn radius_zero_keeps_each_blocks_own_color() {
        let colors = colors();
        let tints = Tints::new(&colors, 0, border);
        let corners = [
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [1.0, 0.0, 1.0],
            [0.0, 0.0, 1.0],
        ];
        assert_eq!(
            tints.corners(FaceTint::Grass, 7, 0, corners),
            [colors.grass(Biome::Forest); 4]
        );
        assert_eq!(
            tints.corners(FaceTint::Grass, 8, 0, corners),
            [colors.grass(Biome::Plains); 4]
        );
    }

    #[test]
    fn fixed_tints_never_blend() {
        let tints = Tints::new(&colors(), 2, border);
        let spruce = FaceTint::Rgb([0.38, 0.6, 0.38]);
        let corners = [[0.0; 3], [1.0, 0.0, 0.0], [1.0, 0.0, 1.0], [0.0, 0.0, 1.0]];
        assert_eq!(tints.corners(spruce, 7, 0, corners), [[0.38, 0.6, 0.38]; 4]);
        assert_eq!(tints.corners(FaceTint::None, 7, 0, corners), [[1.0; 3]; 4]);
    }
}
