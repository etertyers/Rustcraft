//! Small deterministic RNG (SplitMix64) for world generation.
//!
//! World generation must give the same result for the same seed on every run
//! and every machine, so it uses this instead of an OS-seeded generator.

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed)
    }

    /// An RNG for one chunk and purpose (`salt`), independent of every other
    /// chunk's and of the order chunks are generated in.
    pub fn for_chunk(world_seed: u64, x: i32, z: i32, salt: u64) -> Self {
        let mut h = world_seed ^ salt.wrapping_mul(0x9E37_79B9_7F4A_7C15);
        h = mix(h ^ (x as u32 as u64));
        h = mix(h ^ ((z as u32 as u64) << 32));
        Self(h)
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        mix(self.0)
    }

    /// Uniform in 0..n (n > 0).
    pub fn below(&mut self, n: u32) -> u32 {
        (((self.next_u64() >> 32) * n as u64) >> 32) as u32
    }

    /// Uniform in [0, 1).
    pub fn next_f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }
}

/// SplitMix64's output mix; also a good 64-bit hash.
pub fn mix(mut z: u64) -> u64 {
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_seed_same_sequence() {
        let mut a = Rng::new(42);
        let mut b = Rng::new(42);
        for _ in 0..100 {
            assert_eq!(a.next_u64(), b.next_u64());
        }
    }

    #[test]
    fn chunk_rngs_differ_by_position_seed_and_salt() {
        let first = |seed, x, z, salt| Rng::for_chunk(seed, x, z, salt).next_u64();
        let base = first(1, 0, 0, 0);
        assert_ne!(base, first(2, 0, 0, 0));
        assert_ne!(base, first(1, 1, 0, 0));
        assert_ne!(base, first(1, 0, 1, 0));
        assert_ne!(base, first(1, 0, 0, 1));
        assert_ne!(first(1, 1, 2, 0), first(1, 2, 1, 0));
        assert_ne!(first(1, -1, 0, 0), first(1, 1, 0, 0));
    }

    #[test]
    fn below_stays_in_range_and_covers_it() {
        let mut rng = Rng::new(7);
        let mut seen = [false; 5];
        for _ in 0..1000 {
            let n = rng.below(5);
            assert!(n < 5);
            seen[n as usize] = true;
        }
        assert!(seen.iter().all(|&s| s));
    }

    #[test]
    fn floats_are_in_unit_range() {
        let mut rng = Rng::new(9);
        assert!(
            (0..1000)
                .map(|_| rng.next_f64())
                .all(|f| (0.0..1.0).contains(&f))
        );
    }
}
