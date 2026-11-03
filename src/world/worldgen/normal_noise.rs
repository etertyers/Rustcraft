//! Vanilla's `NormalNoise` (1.18+): two octave stacks of Perlin noise, the
//! second sampled at a slightly different scale, summed and scaled so every
//! noise has about the same spread. The caves and aquifers use it with
//! vanilla's own first octaves and amplitudes (`NoiseData`).

use super::{climate::noise_seed, rng::Rng};

/// Octaves with a smaller weight than this are left out.
const MIN_WEIGHT: f64 = 0.01;
/// The second stack's coordinates are scaled by this.
const INPUT_FACTOR: f64 = 1.018_126_888_217_522_7;
/// Vanilla's `ImprovedNoise`: Perlin's improved gradient noise over a
/// shuffled permutation table, offset by a random amount so the lattice
/// zero isn't at the origin.
struct Improved {
    perm: [u8; 256],
    offset: [f64; 3],
}

/// The sixteen gradients (twelve edges of a cube, four repeated).
const GRADIENTS: [[f64; 3]; 16] = [
    [1.0, 1.0, 0.0],
    [-1.0, 1.0, 0.0],
    [1.0, -1.0, 0.0],
    [-1.0, -1.0, 0.0],
    [1.0, 0.0, 1.0],
    [-1.0, 0.0, 1.0],
    [1.0, 0.0, -1.0],
    [-1.0, 0.0, -1.0],
    [0.0, 1.0, 1.0],
    [0.0, -1.0, 1.0],
    [0.0, 1.0, -1.0],
    [0.0, -1.0, -1.0],
    [1.0, 1.0, 0.0],
    [0.0, -1.0, 1.0],
    [-1.0, 1.0, 0.0],
    [0.0, -1.0, -1.0],
];

fn fade(t: f64) -> f64 {
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

fn lerp(t: f64, a: f64, b: f64) -> f64 {
    a + t * (b - a)
}

impl Improved {
    fn new(seed: u32) -> Self {
        let mut rng = Rng::new(u64::from(seed) ^ 0x1357_9BDF_2468_ACE0);
        let offset = [0; 3].map(|_| rng.next_f64() * 256.0);
        let mut perm: [u8; 256] = std::array::from_fn(|i| i as u8);
        for i in 0..256 {
            let j = i + rng.below(256 - i as u32) as usize;
            perm.swap(i, j);
        }
        Self { perm, offset }
    }

    fn get(&self, x: f64, y: f64, z: f64) -> f64 {
        let (x, y, z) = (x + self.offset[0], y + self.offset[1], z + self.offset[2]);
        let (fx, fy, fz) = (x.floor(), y.floor(), z.floor());
        let (xi, yi, zi) = (fx as i32, fy as i32, fz as i32);
        let (xr, yr, zr) = (x - fx, y - fy, z - fz);
        let p = |i: i32| i32::from(self.perm[(i & 255) as usize]);
        let (a, b) = (p(xi), p(xi + 1));
        let (aa, ab) = (p(a + yi), p(a + yi + 1));
        let (ba, bb) = (p(b + yi), p(b + yi + 1));
        let dot = |h: i32, dx: f64, dy: f64, dz: f64| {
            let g = &GRADIENTS[(h & 15) as usize];
            g[0] * dx + g[1] * dy + g[2] * dz
        };
        let d1 = dot(p(aa + zi), xr, yr, zr);
        let d2 = dot(p(ba + zi), xr - 1.0, yr, zr);
        let d3 = dot(p(ab + zi), xr, yr - 1.0, zr);
        let d4 = dot(p(bb + zi), xr - 1.0, yr - 1.0, zr);
        let d5 = dot(p(aa + zi + 1), xr, yr, zr - 1.0);
        let d6 = dot(p(ba + zi + 1), xr - 1.0, yr, zr - 1.0);
        let d7 = dot(p(ab + zi + 1), xr, yr - 1.0, zr - 1.0);
        let d8 = dot(p(bb + zi + 1), xr - 1.0, yr - 1.0, zr - 1.0);
        let (u, v, w) = (fade(xr), fade(yr), fade(zr));
        lerp(
            w,
            lerp(v, lerp(u, d1, d2), lerp(u, d3, d4)),
            lerp(v, lerp(u, d5, d6), lerp(u, d7, d8)),
        )
    }
}

/// One stack of octaves.
struct Octaves {
    /// (noise, frequency, weight) for each octave with a nonzero amplitude.
    layers: Vec<(Improved, f64, f64)>,
}

impl Octaves {
    fn new(seed: u64, stream: u64, first_octave: i32, amplitudes: &[f64]) -> Self {
        let count = amplitudes.len();
        let lowest_value = (1u64 << (count - 1)) as f64 / ((1u64 << count) - 1) as f64;
        let lowest_input = 2f64.powi(first_octave);
        let mut layers = Vec::new();
        for (i, &amplitude) in amplitudes.iter().enumerate() {
            if amplitude == 0.0 {
                continue;
            }
            let weight = amplitude * lowest_value / 2f64.powi(i as i32);
            // An octave this faint (under 1% of a unit) is finer than the
            // 4-block grid the density is sampled on: it only aliases.
            if weight < MIN_WEIGHT {
                continue;
            }
            layers.push((
                Improved::new(noise_seed(seed, stream * 64 + i as u64)),
                lowest_input * 2f64.powi(i as i32),
                weight,
            ));
        }
        Self { layers }
    }

    fn get(&self, x: f64, y: f64, z: f64) -> f64 {
        self.layers
            .iter()
            .map(|(noise, frequency, weight)| {
                weight * noise.get(x * frequency, y * frequency, z * frequency)
            })
            .sum()
    }
}

pub struct NormalNoise {
    first: Octaves,
    second: Octaves,
    factor: f64,
}

impl NormalNoise {
    /// Noise number `index` of the world `seed`, with vanilla's first
    /// octave (`-7` is wavelengths from 128 blocks) and amplitudes.
    pub fn new(seed: u64, index: u64, first_octave: i32, amplitudes: &[f64]) -> Self {
        let first = Octaves::new(seed, index * 2, first_octave, amplitudes);
        let second = Octaves::new(seed, index * 2 + 1, first_octave, amplitudes);
        // `NormalNoise.valueFactor`: 1/6 over the expected deviation of the
        // octaves' span.
        let used: Vec<usize> = amplitudes
            .iter()
            .enumerate()
            .filter(|(_, a)| **a != 0.0)
            .map(|(i, _)| i)
            .collect();
        let span = used.last().zip(used.first()).map_or(0, |(hi, lo)| hi - lo);
        let expected = 0.1 * (1.0 + 1.0 / (span as f64 + 1.0));
        Self {
            first,
            second,
            factor: (1.0 / 6.0) / expected,
        }
    }

    pub fn get(&self, x: f64, y: f64, z: f64) -> f64 {
        (self.first.get(x, y, z)
            + self
                .second
                .get(x * INPUT_FACTOR, y * INPUT_FACTOR, z * INPUT_FACTOR))
            * self.factor
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn deviation(noise: &NormalNoise, scale: f64) -> f64 {
        let n = 20_000;
        let mut sum = 0.0;
        let mut sum_sq = 0.0;
        let mut rng = super::super::rng::Rng::new(7);
        for _ in 0..n {
            let [x, y, z] = [0; 3].map(|_| (rng.next_f64() - 0.5) * 20_000.0);
            let v = noise.get(x * scale, y * scale, z * scale);
            sum += v;
            sum_sq += v * v;
        }
        let mean = sum / n as f64;
        (sum_sq / n as f64 - mean * mean).sqrt()
    }

    #[test]
    fn a_single_octave_spreads_about_0_3_and_more_octaves_less() {
        let single = NormalNoise::new(99, 5, -7, &[1.0]);
        let d = deviation(&single, 1.0);
        assert!((d - 0.3).abs() < 0.06, "single {d}");
        let cheese = NormalNoise::new(99, 6, -8, &[0.5, 1.0, 2.0, 1.0, 2.0, 1.0, 0.0, 2.0, 0.0]);
        let entrance = NormalNoise::new(99, 7, -7, &[0.4, 0.5, 1.0]);
        let (dc, de) = (deviation(&cheese, 1.0), deviation(&entrance, 1.0));
        assert!(dc < d && de < d, "{dc} {de} against {d}");
        assert!(de > 0.08, "{de}");
        let top = (0..2000)
            .map(|i| single.get(i as f64 * 37.1, 11.0, i as f64 * 5.3).abs())
            .fold(0.0, f64::max);
        assert!(top < 1.5, "{top}");
    }

    #[test]
    fn it_is_deterministic_and_smooth() {
        let a = NormalNoise::new(1, 2, -8, &[1.0]);
        let b = NormalNoise::new(1, 2, -8, &[1.0]);
        assert_eq!(a.get(10.5, -20.25, 3.0), b.get(10.5, -20.25, 3.0));
        assert!((a.get(10.0, 0.0, 0.0) - a.get(10.5, 0.0, 0.0)).abs() < 0.05);
    }
}
