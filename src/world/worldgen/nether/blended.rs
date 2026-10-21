//! Classic (1.17) blended terrain noise, which modern Minecraft still uses
//! for the Nether's shape: two stacks of 3D Perlin octaves ("lower" and
//! "upper") that a third, smoother stack ("main") chooses between.
//!
//! Octave `k` runs at `2^-k` the frequency and `2^k` the weight of the
//! first, so the big octaves (wavelengths of 100+ blocks) carry nearly all
//! of the weight. Octaves under `LIMIT_FIRST` weigh 0.1% of the total
//! between them, so the limit stacks skip them.

use super::super::rng::Rng;

/// Vanilla's `xz_scale` 0.25 and `y_scale` 0.375 (times its 684.412), and
/// the `xz_factor` 80, `y_factor` 60 and `smear_scale_multiplier` 8 of the
/// main noise.
const XZ_MUL: f64 = 684.412 * 0.25;
const Y_MUL: f64 = 684.412 * 0.375;
const XZ_FACTOR: f64 = 80.0;
const Y_FACTOR: f64 = 60.0;
const SMEAR: f64 = 8.0;

const MAIN_OCTAVES: usize = 8;
const LIMIT_OCTAVES: usize = 16;
/// The finest limit octave still sampled.
const LIMIT_FIRST: usize = 6;

/// The 16 gradients of improved Perlin noise.
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

/// Vanilla `ImprovedNoise`, with the y "smear" the blended noise needs.
struct Improved {
    perm: [u8; 256],
    offset: [f64; 3],
}

impl Improved {
    fn new(rng: &mut Rng) -> Self {
        let offset = [0, 1, 2].map(|_| rng.next_f64() * 256.0);
        let mut perm: [u8; 256] = std::array::from_fn(|i| i as u8);
        for i in (1..256).rev() {
            perm.swap(i, rng.below(i as u32 + 1) as usize);
        }
        Self { perm, offset }
    }

    fn p(&self, i: i32) -> usize {
        self.perm[(i & 255) as usize] as usize
    }

    fn grad(&self, hash: usize, x: f64, y: f64, z: f64) -> f64 {
        let g = GRADIENTS[hash & 15];
        g[0] * x + g[1] * y + g[2] * z
    }

    /// Noise at (x, y, z); a nonzero `y_scale` snaps the y gradient lookup
    /// to multiples of it (up to `y_max`), which smears features upward.
    fn noise(&self, x: f64, y: f64, z: f64, y_scale: f64, y_max: f64) -> f64 {
        let (x, y, z) = (x + self.offset[0], y + self.offset[1], z + self.offset[2]);
        let (xf, yf, zf) = (x.floor(), y.floor(), z.floor());
        let (gx, gy, gz) = (xf as i32, yf as i32, zf as i32);
        let (dx, fy, dz) = (x - xf, y - yf, z - zf);
        let snap = if y_scale != 0.0 {
            let top = if y_max >= 0.0 && y_max < fy {
                y_max
            } else {
                fy
            };
            (top / y_scale + 1.0e-7).floor() * y_scale
        } else {
            0.0
        };
        let dy = fy - snap;
        let (a, b) = (self.p(gx), self.p(gx + 1));
        let (aa, ab) = (self.p(a as i32 + gy), self.p(a as i32 + gy + 1));
        let (ba, bb) = (self.p(b as i32 + gy), self.p(b as i32 + gy + 1));
        let v = |h: usize, x, y, z| self.grad(self.p(h as i32 + gz), x, y, z);
        let w = |h: usize, x, y, z| self.grad(self.p(h as i32 + gz + 1), x, y, z);
        let c000 = v(aa, dx, dy, dz);
        let c100 = v(ba, dx - 1.0, dy, dz);
        let c010 = v(ab, dx, dy - 1.0, dz);
        let c110 = v(bb, dx - 1.0, dy - 1.0, dz);
        let c001 = w(aa, dx, dy, dz - 1.0);
        let c101 = w(ba, dx - 1.0, dy, dz - 1.0);
        let c011 = w(ab, dx, dy - 1.0, dz - 1.0);
        let c111 = w(bb, dx - 1.0, dy - 1.0, dz - 1.0);
        let fade = |t: f64| t * t * t * (t * (t * 6.0 - 15.0) + 10.0);
        let (u, vv, ww) = (fade(dx), fade(fy), fade(dz));
        let lerp = |t: f64, a: f64, b: f64| a + t * (b - a);
        let x00 = lerp(u, c000, c100);
        let x10 = lerp(u, c010, c110);
        let x01 = lerp(u, c001, c101);
        let x11 = lerp(u, c011, c111);
        lerp(ww, lerp(vv, x00, x10), lerp(vv, x01, x11))
    }
}

/// Vanilla `PerlinNoise.wrap`: keeps far coordinates precise.
fn wrap(value: f64) -> f64 {
    const RANGE: f64 = 3.355_443_2e7;
    value - (value / RANGE + 0.5).floor() * RANGE
}

pub struct BlendedNoise {
    main: Vec<Improved>,
    lower: Vec<Improved>,
    upper: Vec<Improved>,
}

impl BlendedNoise {
    pub fn new(seed: u64) -> Self {
        let mut rng = Rng::new(seed);
        let mut stack = |n: usize| (0..n).map(|_| Improved::new(&mut rng)).collect::<Vec<_>>();
        Self {
            main: stack(MAIN_OCTAVES),
            lower: stack(LIMIT_OCTAVES - LIMIT_FIRST),
            upper: stack(LIMIT_OCTAVES - LIMIT_FIRST),
        }
    }

    /// The noise at a block position, roughly -0.3..0.3.
    pub fn get(&self, x: f64, y: f64, z: f64) -> f64 {
        let (d0, d1, d2) = (x * XZ_MUL, y * Y_MUL, z * XZ_MUL);
        let (d3, d4, d5) = (d0 / XZ_FACTOR, d1 / Y_FACTOR, d2 / XZ_FACTOR);
        let smear = Y_MUL * SMEAR;
        let main_smear = smear / Y_FACTOR;
        let mut main = 0.0;
        let mut f = 1.0;
        for octave in &self.main {
            main += octave.noise(
                wrap(d3 * f),
                wrap(d4 * f),
                wrap(d5 * f),
                main_smear * f,
                d4 * f,
            ) / f;
            f /= 2.0;
        }
        let blend = (main / 10.0 + 1.0) / 2.0;
        let limit = |stack: &[Improved]| {
            let mut sum = 0.0;
            let mut f = 2f64.powi(-(LIMIT_FIRST as i32));
            for octave in stack {
                sum +=
                    octave.noise(wrap(d0 * f), wrap(d1 * f), wrap(d2 * f), smear * f, d1 * f) / f;
                f /= 2.0;
            }
            sum / 512.0
        };
        let lower = if blend < 1.0 { limit(&self.lower) } else { 0.0 };
        let upper = if blend > 0.0 { limit(&self.upper) } else { 0.0 };
        let t = blend.clamp(0.0, 1.0);
        (lower + (upper - lower) * t) / 128.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_is_smooth_and_centered_on_zero() {
        let noise = BlendedNoise::new(5);
        let mut values = Vec::new();
        for i in 0..40 {
            for j in 0..10 {
                values.push(noise.get(i as f64 * 37.0, j as f64 * 12.0, i as f64 * 11.0));
            }
        }
        let mean = values.iter().sum::<f64>() / values.len() as f64;
        let spread =
            (values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / values.len() as f64).sqrt();
        eprintln!("blended noise: mean {mean:.3}, deviation {spread:.3}");
        assert!(mean.abs() < 0.3 && spread > 0.02 && spread < 1.0);
        // Close blocks are close in value.
        let a = noise.get(10.0, 60.0, 10.0);
        let b = noise.get(11.0, 60.0, 10.0);
        assert!((a - b).abs() < 0.05, "{a} {b}");
    }
}
