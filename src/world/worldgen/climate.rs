//! Modern Minecraft's climate parameters (`Climate.Sampler`).
//!
//! Five 2D noise fields describe every column, as in modern Java Edition:
//! temperature, humidity (vegetation), continentalness (ocean to far
//! inland), erosion (mountainous to flat) and weirdness (which, folded,
//! gives peaks and valleys). Each is a vanilla "normal noise" (two
//! multi-octave Perlin samplers at slightly different scales, summed and
//! scaled to sit roughly in -1..1), with vanilla's octaves and amplitudes,
//! sampled at quarter-block coordinates whose x and z are first nudged by
//! a small shift noise. They are very low-frequency: a temperature band
//! spans thousands of blocks. `config::BIOME_SCALE` stretches them all.
//!
//! The terrain samples them every `GRID` blocks and blends in between
//! (modern's 4×4 noise cells); biomes take the sample at each 4×4 cell's
//! corner (a "quart" position) as it is.

use noise::{NoiseFn, Perlin};

use super::rng::mix;
use crate::config::BIOME_SCALE;

/// Blocks between climate samples (a quart).
pub const GRID: i32 = 4;

/// The five parameters at a column.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Climate {
    pub temperature: f64,
    pub humidity: f64,
    pub continent: f64,
    pub erosion: f64,
    pub weirdness: f64,
}

impl Climate {
    fn lerp(self, other: Self, t: f64) -> Self {
        let l = |a: f64, b: f64| a + (b - a) * t;
        Self {
            temperature: l(self.temperature, other.temperature),
            humidity: l(self.humidity, other.humidity),
            continent: l(self.continent, other.continent),
            erosion: l(self.erosion, other.erosion),
            weirdness: l(self.weirdness, other.weirdness),
        }
    }

    /// Modern "peaks and valleys": weirdness folded so that its middle and
    /// its ends are valleys (-1) and the bands between are peaks (1).
    pub fn peaks(&self) -> f64 {
        peaks_and_valleys(self.weirdness)
    }
}

/// PV = 1 − |3|w| − 2|.
pub fn peaks_and_valleys(weirdness: f64) -> f64 {
    1.0 - (3.0 * weirdness.abs() - 2.0).abs()
}

/// Continentalness below this is ocean, below `DEEP_OCEAN` deep ocean, and
/// below `MUSHROOM` mushroom fields; up to `COAST` is the coast (modern's
/// bands).
pub const MUSHROOM: f64 = -1.05;
pub const DEEP_OCEAN: f64 = -0.455;
pub const OCEAN: f64 = -0.19;
pub const COAST: f64 = -0.11;

/// A 32-bit noise seed derived from the world seed, different for each `index`.
pub fn noise_seed(seed: u64, index: u64) -> u32 {
    mix(seed ^ mix(index)) as u32
}

/// A shift for a noise field's coordinates, different for each `index`.
/// Perlin noise is zero on its lattice, which would otherwise pin every
/// field to the same value at the origin.
pub fn noise_offset(seed: u64, index: u64) -> [f64; 3] {
    let h = mix(seed ^ mix(index ^ 0x0FF5E7));
    [h, h >> 21, h >> 42].map(|bits| (bits & 0xFFFFF) as f64 / 64.0 + 0.37)
}

/// The noise crate's 2D Perlin spreads wider than vanilla's improved noise
/// does on a plane (standard deviation 0.43 against about 0.3); this
/// brings it back, so continentalness sinks below the mushroom fields'
/// -1.05 about as rarely as in vanilla.
const PLANE_SPREAD: f64 = 0.7;

/// Vanilla `PerlinNoise`: octaves from `2^first` up, each weighted by its
/// amplitude and by a value factor that halves per octave.
struct Octaves {
    octaves: Vec<(Perlin, f64, [f64; 2])>,
    first: i32,
    lowest_value_factor: f64,
}

impl Octaves {
    fn new(seed: u64, index: u64, first: i32, amplitudes: &[f64]) -> Self {
        let n = amplitudes.len() as i32;
        Self {
            octaves: amplitudes
                .iter()
                .enumerate()
                .map(|(i, &a)| {
                    let index = index * 64 + i as u64;
                    let [x, _, z] = noise_offset(seed, index);
                    (Perlin::new(noise_seed(seed, index)), a, [x, z])
                })
                .collect(),
            first,
            lowest_value_factor: 2f64.powi(n - 1) / (2f64.powi(n) - 1.0),
        }
    }

    fn get(&self, x: f64, z: f64) -> f64 {
        let mut input = 2f64.powi(self.first);
        let mut value = self.lowest_value_factor;
        let mut sum = 0.0;
        for (perlin, amplitude, offset) in &self.octaves {
            if *amplitude != 0.0 {
                let sample = perlin.get([x * input + offset[0], z * input + offset[1]]);
                sum += amplitude * sample * PLANE_SPREAD * value;
            }
            input *= 2.0;
            value /= 2.0;
        }
        sum
    }
}

/// Vanilla `NormalNoise`: two `Octaves` (the second at 1.0181… the scale)
/// summed and scaled by `(1/6) / expectedDeviation`.
struct NormalNoise {
    first: Octaves,
    second: Octaves,
    factor: f64,
}

/// The second sampler's input scale (vanilla's `INPUT_FACTOR`).
const INPUT_FACTOR: f64 = 1.018_126_888_217_522_7;

impl NormalNoise {
    fn new(seed: u64, index: u64, first: i32, amplitudes: &[f64]) -> Self {
        let lo = amplitudes.iter().position(|&a| a != 0.0).unwrap_or(0);
        let hi = amplitudes.iter().rposition(|&a| a != 0.0).unwrap_or(0);
        let span = (hi - lo) as f64;
        let expected_deviation = 0.1 * (1.0 + 1.0 / (span + 1.0));
        Self {
            first: Octaves::new(seed, index * 2, first, amplitudes),
            second: Octaves::new(seed, index * 2 + 1, first, amplitudes),
            factor: (1.0 / 6.0) / expected_deviation,
        }
    }

    fn get(&self, x: f64, z: f64) -> f64 {
        (self.first.get(x, z) + self.second.get(x * INPUT_FACTOR, z * INPUT_FACTOR)) * self.factor
    }
}

pub struct ClimateNoise {
    temperature: NormalNoise,
    humidity: NormalNoise,
    continent: NormalNoise,
    erosion: NormalNoise,
    weirdness: NormalNoise,
    shift_x: NormalNoise,
    shift_z: NormalNoise,
}

impl ClimateNoise {
    /// Vanilla's octaves and amplitudes (`NoiseRouterData`), each field
    /// with its own sub-seed.
    pub fn new(seed: u64) -> Self {
        Self {
            temperature: NormalNoise::new(seed, 101, -10, &[1.5, 0.0, 1.0, 0.0, 0.0, 0.0]),
            humidity: NormalNoise::new(seed, 102, -8, &[1.0, 1.0, 0.0, 0.0, 0.0, 0.0]),
            continent: NormalNoise::new(
                seed,
                103,
                -9,
                &[1.0, 1.0, 2.0, 2.0, 2.0, 1.0, 1.0, 1.0, 1.0],
            ),
            erosion: NormalNoise::new(seed, 104, -9, &[1.0, 1.0, 0.0, 1.0, 1.0]),
            weirdness: NormalNoise::new(seed, 105, -7, &[1.0, 2.0, 1.0, 0.0, 0.0, 0.0]),
            shift_x: NormalNoise::new(seed, 106, -3, &[1.0, 1.0, 1.0, 0.0]),
            shift_z: NormalNoise::new(seed, 107, -3, &[1.0, 1.0, 1.0, 0.0]),
        }
    }

    /// The climate exactly at block (x, z): quarter-block coordinates,
    /// shifted by up to about four of them (vanilla `shiftedNoise2d`).
    pub fn sample(&self, x: i32, z: i32) -> Climate {
        let (qx, qz) = (x as f64 * 0.25 / BIOME_SCALE, z as f64 * 0.25 / BIOME_SCALE);
        let (sx, sz) = (
            qx + self.shift_x.get(qx, qz) * 4.0,
            qz + self.shift_z.get(qx, qz) * 4.0,
        );
        Climate {
            temperature: self.temperature.get(sx, sz),
            humidity: self.humidity.get(sx, sz),
            continent: self.continent.get(sx, sz),
            erosion: self.erosion.get(sx, sz),
            weirdness: self.weirdness.get(sx, sz),
        }
    }

    /// The climate at the corner of the quart holding (x, z): what its
    /// biome is chosen from.
    pub fn quart(&self, x: i32, z: i32) -> Climate {
        self.sample(x.div_euclid(GRID) * GRID, z.div_euclid(GRID) * GRID)
    }

    /// The climate at (x, z), blended from the `GRID` samples around it
    /// (for the terrain's smooth shape).
    pub fn at(&self, x: i32, z: i32) -> Climate {
        let (gx, gz) = (x.div_euclid(GRID) * GRID, z.div_euclid(GRID) * GRID);
        let (tx, tz) = ((x - gx) as f64 / GRID as f64, (z - gz) as f64 / GRID as f64);
        blend(
            [
                self.sample(gx, gz),
                self.sample(gx + GRID, gz),
                self.sample(gx, gz + GRID),
                self.sample(gx + GRID, gz + GRID),
            ],
            tx,
            tz,
        )
    }
}

/// Bilinear blend of the corner climates (x0z0, x1z0, x0z1, x1z1).
pub fn blend(corners: [Climate; 4], tx: f64, tz: f64) -> Climate {
    let near = corners[0].lerp(corners[1], tx);
    let far = corners[2].lerp(corners[3], tx);
    near.lerp(far, tz)
}

/// What the terrain is like at a column, besides its climate.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Lay {
    /// Y of the topmost solid block.
    pub height: i32,
    /// The surface height the shape pulls toward (the preliminary surface).
    pub offset: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn samples(seed: u64, pick: fn(&Climate) -> f64) -> Vec<f64> {
        let noise = ClimateNoise::new(seed);
        let mut values: Vec<f64> = (0..200)
            .flat_map(|i| (0..200).map(move |j| (i * 997 - 100_000, j * 1009 - 100_000)))
            .map(|(x, z)| pick(&noise.sample(x, z)))
            .collect();
        values.sort_by(f64::total_cmp);
        values
    }

    #[test]
    fn fields_sit_roughly_in_minus_one_to_one() {
        for pick in [
            (|c: &Climate| c.temperature) as fn(&Climate) -> f64,
            |c| c.humidity,
            |c| c.continent,
            |c| c.erosion,
            |c| c.weirdness,
        ] {
            let values = samples(3, pick);
            let (lo, hi) = (values[values.len() / 100], values[values.len() * 99 / 100]);
            assert!(
                lo > -1.1 && hi < 1.1 && lo < -0.25 && hi > 0.25,
                "{lo} {hi}"
            );
        }
    }

    #[test]
    fn the_fields_change_slowly_across_the_land() {
        let noise = ClimateNoise::new(7);
        // Temperature barely moves over 64 blocks.
        let mut most = 0.0f64;
        for i in 0..200 {
            let (x, z) = (i * 512, -i * 300);
            let d = (noise.sample(x, z).temperature - noise.sample(x + 64, z).temperature).abs();
            most = most.max(d);
        }
        assert!(most < 0.15, "{most}");
    }

    #[test]
    fn blended_climate_meets_the_samples_on_the_grid() {
        let noise = ClimateNoise::new(9);
        assert_eq!(noise.at(8, -12), noise.sample(8, -12));
        assert_eq!(noise.quart(10, -11), noise.sample(8, -12));
    }

    #[test]
    fn peaks_fold_weirdness() {
        assert_eq!(peaks_and_valleys(0.0), -1.0);
        assert_eq!(peaks_and_valleys(1.0), 0.0);
        assert!((peaks_and_valleys(2.0 / 3.0) - 1.0).abs() < 1e-12);
    }
}
