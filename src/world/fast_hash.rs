//! A fast hasher for maps keyed by block and chunk positions (rustc's
//! FxHash): every block lookup goes through the world's chunk map, and the
//! standard library's DoS-resistant SipHash made hashing the biggest cost
//! of a game tick. Nothing hashed here comes from outside the game.

use std::{
    collections::{HashMap, HashSet},
    hash::{BuildHasherDefault, Hasher},
};

pub type FastMap<K, V> = HashMap<K, V, BuildHasherDefault<FastHasher>>;
pub type FastSet<K> = HashSet<K, BuildHasherDefault<FastHasher>>;

const SEED: u64 = 0x51_7c_c1_b7_27_22_0a_95;

#[derive(Debug, Default, Clone, Copy)]
pub struct FastHasher(u64);

impl FastHasher {
    fn add(&mut self, word: u64) {
        self.0 = (self.0.rotate_left(5) ^ word).wrapping_mul(SEED);
    }
}

impl Hasher for FastHasher {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, bytes: &[u8]) {
        for chunk in bytes.chunks(8) {
            let mut word = [0; 8];
            word[..chunk.len()].copy_from_slice(chunk);
            self.add(u64::from_le_bytes(word));
        }
    }

    fn write_u8(&mut self, n: u8) {
        self.add(n as u64);
    }

    fn write_u16(&mut self, n: u16) {
        self.add(n as u64);
    }

    fn write_u32(&mut self, n: u32) {
        self.add(n as u64);
    }

    fn write_u64(&mut self, n: u64) {
        self.add(n);
    }

    fn write_usize(&mut self, n: usize) {
        self.add(n as u64);
    }

    fn write_i32(&mut self, n: i32) {
        self.add(n as u32 as u64);
    }
}

#[cfg(test)]
mod tests {
    use glam::IVec3;

    use super::*;

    #[test]
    fn neighboring_positions_hash_apart() {
        let hash = |at: IVec3| {
            let mut hasher = FastHasher::default();
            std::hash::Hash::hash(&at, &mut hasher);
            hasher.finish()
        };
        let mut seen = HashSet::new();
        for x in -8..8 {
            for y in 0..8 {
                for z in -8..8 {
                    assert!(seen.insert(hash(IVec3::new(x, y, z))));
                }
            }
        }
        let mut map: FastMap<IVec3, i32> = FastMap::default();
        map.insert(IVec3::new(1, 2, 3), 4);
        assert_eq!(map.get(&IVec3::new(1, 2, 3)), Some(&4));
    }
}
