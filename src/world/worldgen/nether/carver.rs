//! The Nether's cave carver (modern `nether_cave`): winding tall tunnels
//! and a few rooms cut through the rock after the terrain is shaped.
//!
//! Probability 0.2 per chunk, y 0..126, room `yScale` 0.5, `floor_level`
//! -0.7 (nothing is cut below 0.7 of an ellipsoid's radius under its
//! middle, so tunnels have flat floors). A source chunk's tunnels reach up
//! to 112 blocks, so a chunk is carved by the sources within 8 chunks.
//! Sources are worked out once as lists of ellipsoids and the cut is a
//! bit mask per column, so every chunk agrees on the same caves.
//!
//! Only the rock is ever cut (netherrack, soul sand and soul soil,
//! basalt, blackstone, nylium, wart blocks, magma, gravel: everything the
//! terrain has before features), never the bedrock. Cut space is air, as in
//! modern, except below `LAVA_BELOW` (the carver's `lava_level`), where it
//! is lava; so caves under the lava sea stay dry pockets above y 10.

use std::f64::consts::PI;

use super::super::rng::Rng;
use crate::config::CHUNK_SIZE;

/// Chunks around a chunk whose caves can reach it.
pub const RANGE: i32 = 8;
const CHANCE: u32 = 5;
/// Tunnel length: `(range * 2 - 1)` chunks.
const LENGTH: i32 = 7 * 16;
const CAVE_BOUND: u32 = 10;
const FLOOR: f64 = -0.7;
const ROOM_Y_SCALE: f64 = 0.5;
/// Tunnels are five times as tall as they are wide.
const TUNNEL_Y_SCALE: f64 = 5.0;
/// Carved space under this y fills with lava.
pub const LAVA_BELOW: i32 = 10;
const MIN_Y: i32 = 1;
const MAX_Y: i32 = 126;

/// One cut ellipsoid.
#[derive(Debug, Clone, Copy)]
pub struct Ellipsoid {
    x: f64,
    y: f64,
    z: f64,
    horizontal: f64,
    vertical: f64,
}

/// A source chunk's ellipsoids, shared.
pub type Source = std::sync::Arc<Vec<Ellipsoid>>;

/// The ellipsoids of the caves that start in chunk (cx, cz).
pub fn source(seed: u64, cx: i32, cz: i32) -> Vec<Ellipsoid> {
    let mut rng = Rng::for_chunk(seed, cx, cz, 0xCA7E);
    let mut out = Vec::new();
    if rng.below(CHANCE) != 0 {
        return out;
    }
    let float = |rng: &mut Rng| rng.next_f64();
    let n = {
        let a = rng.below(CAVE_BOUND) + 1;
        let b = rng.below(a) + 1;
        rng.below(b)
    };
    for _ in 0..n {
        let x = (cx * 16 + rng.below(16) as i32) as f64;
        let y = rng.below(MAX_Y as u32 + 1) as f64;
        let z = (cz * 16 + rng.below(16) as i32) as f64;
        let mut tunnels = 1;
        if rng.below(4) == 0 {
            let radius = 1.0 + float(&mut rng) * 6.0;
            let wide = 1.5 + radius;
            out.push(Ellipsoid {
                x: x + 1.0,
                y,
                z,
                horizontal: wide,
                vertical: wide * ROOM_Y_SCALE,
            });
            tunnels += rng.below(4);
        }
        for _ in 0..tunnels {
            let yaw = float(&mut rng) * 2.0 * PI;
            let pitch = (float(&mut rng) - 0.5) / 4.0;
            let thickness = (float(&mut rng) * 2.0 + float(&mut rng)) * 2.0;
            let length = LENGTH - rng.below(LENGTH as u32 / 4) as i32;
            tunnel(
                &mut out,
                rng.next_u64(),
                (x, y, z),
                thickness,
                (yaw, pitch),
                (0, length),
            );
        }
    }
    out
}

/// Vanilla `createTunnel`: a wandering path of ellipsoids that may split
/// in two once.
fn tunnel(
    out: &mut Vec<Ellipsoid>,
    seed: u64,
    (mut x, mut y, mut z): (f64, f64, f64),
    thickness: f64,
    (mut yaw, mut pitch): (f64, f64),
    (first, count): (i32, i32),
) {
    let mut rng = Rng::new(seed);
    let split = rng.below((count / 2).max(1) as u32) as i32 + count / 4;
    let steady = rng.below(6) == 0;
    let (mut yaw_drift, mut pitch_drift) = (0.0, 0.0);
    for j in first..count {
        let radius = 1.5 + (PI * j as f64 / count as f64).sin() * thickness;
        let flat = pitch.cos();
        x += yaw.cos() * flat;
        y += pitch.sin();
        z += yaw.sin() * flat;
        pitch *= if steady { 0.92 } else { 0.7 };
        pitch += pitch_drift * 0.1;
        yaw += yaw_drift * 0.1;
        pitch_drift *= 0.9;
        yaw_drift *= 0.75;
        let f = |rng: &mut Rng| rng.next_f64();
        pitch_drift += (f(&mut rng) - f(&mut rng)) * f(&mut rng) * 2.0;
        yaw_drift += (f(&mut rng) - f(&mut rng)) * f(&mut rng) * 4.0;
        if j == split && thickness > 1.0 {
            for side in [-1.0, 1.0] {
                let branch = rng.next_u64();
                let thin = f(&mut rng) * 0.5 + 0.5;
                tunnel(
                    out,
                    branch,
                    (x, y, z),
                    thin,
                    (yaw + side * PI / 2.0, pitch / 3.0),
                    (j, count),
                );
            }
            return;
        }
        if rng.below(4) != 0 {
            out.push(Ellipsoid {
                x,
                y,
                z,
                horizontal: radius,
                vertical: radius * TUNNEL_Y_SCALE,
            });
        }
    }
}

/// What the caves cut from a chunk: bit y of a column's word is set where
/// the block is carved away.
pub struct Mask {
    columns: [u128; 256],
}

impl Mask {
    pub fn column(&self, lx: usize, lz: usize) -> u128 {
        self.columns[lz * CHUNK_SIZE + lx]
    }

    /// Cuts `ellipsoid` from the chunk whose minimum corner is (x0, z0).
    fn carve(&mut self, e: &Ellipsoid, x0: i32, z0: i32) {
        let size = CHUNK_SIZE as i32;
        let reach = e.horizontal;
        let lo_x = ((e.x - reach).floor() as i32 - x0).max(0);
        let hi_x = ((e.x + reach).floor() as i32 - x0).min(size - 1);
        let lo_z = ((e.z - reach).floor() as i32 - z0).max(0);
        let hi_z = ((e.z + reach).floor() as i32 - z0).min(size - 1);
        for lz in lo_z..=hi_z {
            let dz = ((z0 + lz) as f64 + 0.5 - e.z) / e.horizontal;
            for lx in lo_x..=hi_x {
                let dx = ((x0 + lx) as f64 + 0.5 - e.x) / e.horizontal;
                let rest = 1.0 - dx * dx - dz * dz;
                if rest <= 0.0 {
                    continue;
                }
                // Block centers with -0.7 < dy and dx² + dy² + dz² < 1.
                let half = rest.sqrt();
                let lo = (e.y + e.vertical * FLOOR.max(-half) - 0.5).floor() as i32 + 1;
                let hi = (e.y + e.vertical * half - 0.5).ceil() as i32 - 1;
                let (lo, hi) = (lo.max(MIN_Y), hi.min(MAX_Y));
                if lo <= hi {
                    let bits = (!0u128 >> (127 - (hi - lo))) << lo;
                    self.columns[(lz * size + lx) as usize] |= bits;
                }
            }
        }
    }
}

/// The cut for the chunk at (cx, cz), from the sources around it (given
/// by `source_of`).
pub fn mask(cx: i32, cz: i32, mut source_of: impl FnMut(i32, i32) -> Source) -> Mask {
    let mut mask = Mask { columns: [0; 256] };
    let (x0, z0) = (cx * 16, cz * 16);
    for dz in -RANGE..=RANGE {
        for dx in -RANGE..=RANGE {
            for e in source_of(cx + dx, cz + dz).iter() {
                mask.carve(e, x0, z0);
            }
        }
    }
    mask
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn about_a_fifth_of_chunks_have_caves() {
        let with = (0..400)
            .filter(|&i| !source(7, i % 20, i / 20).is_empty())
            .count();
        // 0.2 of them roll caves, a few of those roll none.
        assert!((20..110).contains(&with), "{with}");
    }

    #[test]
    fn caves_cut_tall_tunnels_with_flat_floors() {
        let mut cut = 0;
        for i in 0..100 {
            let m = mask(i, 0, |x, z| Arc::new(source(3, x, z)));
            for lz in 0..16 {
                for lx in 0..16 {
                    let col = m.column(lx, lz);
                    cut += col.count_ones();
                    assert_eq!(col & 1, 0, "bedrock floor");
                    assert_eq!(col >> 127, 0, "bedrock roof");
                }
            }
        }
        assert!(cut > 1000, "{cut}");
    }
}
