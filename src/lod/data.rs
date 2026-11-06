//! LOD data: samples of a column's vertical slices, grouped in tiles.

use super::{MAX_LEVEL, MAX_SLICES, TILE_SAMPLES};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Material {
    /// Solid ground (and ice).
    Ground = 0,
    Water = 1,
    /// Tree canopy.
    Leaves = 2,
    /// Solid rock hanging above the ground, with air between.
    Overhang = 3,
}

impl Material {
    fn from_u8(v: u8) -> Option<Self> {
        Some(match v {
            0 => Self::Ground,
            1 => Self::Water,
            2 => Self::Leaves,
            3 => Self::Overhang,
            _ => return None,
        })
    }
}

/// One vertical run of a single material: blocks `bottom..=top`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Slice {
    pub top: i16,
    pub bottom: i16,
    pub color: [u8; 3],
    pub material: Material,
    /// Sky light at the top, 0–15.
    pub light: u8,
}

impl Slice {
    pub const EMPTY: Slice = Slice {
        top: 0,
        bottom: 0,
        color: [0; 3],
        material: Material::Ground,
        light: 15,
    };
}

/// A column sampled at some level: slices from the bottom up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sample {
    pub count: u8,
    /// Built from a real chunk rather than the fast path.
    pub real: bool,
    pub slices: [Slice; MAX_SLICES],
}

impl Sample {
    pub const EMPTY: Sample = Sample {
        count: 0,
        real: false,
        slices: [Slice::EMPTY; MAX_SLICES],
    };

    pub fn slices(&self) -> &[Slice] {
        &self.slices[..self.count as usize]
    }

    pub fn push(&mut self, slice: Slice) {
        if (self.count as usize) < MAX_SLICES {
            self.slices[self.count as usize] = slice;
            self.count += 1;
        }
    }

    /// The top of the ground slice (the first one), or `None` if empty.
    pub fn ground_top(&self) -> Option<i32> {
        self.slices().first().map(|s| i32::from(s.top))
    }

    /// The highest point of any slice.
    pub fn highest(&self) -> Option<i32> {
        self.slices().iter().map(|s| i32::from(s.top)).max()
    }
}

/// A tile's place: level `level` (1 << level blocks per sample), and
/// tile coordinates in units of `TILE_SAMPLES << level` blocks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TileKey {
    pub level: u8,
    pub tx: i32,
    pub tz: i32,
}

impl TileKey {
    pub fn new(level: u8, tx: i32, tz: i32) -> Self {
        Self {
            level: level.min(MAX_LEVEL),
            tx,
            tz,
        }
    }

    /// Blocks per sample.
    pub fn step(self) -> i32 {
        1 << self.level
    }

    /// Blocks along a side of the tile.
    pub fn span(self) -> i32 {
        (TILE_SAMPLES as i32) << self.level
    }

    /// The block coordinates of the tile's minimum corner.
    pub fn origin(self) -> (i32, i32) {
        (self.tx * self.span(), self.tz * self.span())
    }

    /// The tile at `level` holding block (x, z).
    pub fn containing(level: u8, x: i32, z: i32) -> Self {
        let span = (TILE_SAMPLES as i32) << level.min(MAX_LEVEL);
        Self::new(level, x.div_euclid(span), z.div_euclid(span))
    }

    /// Block coordinates sampled by sample (i, j): the cell's middle, so
    /// level 0 is the column itself.
    pub fn sample_block(self, i: usize, j: usize) -> (i32, i32) {
        let (ox, oz) = self.origin();
        let half = self.step() / 2;
        (
            ox + i as i32 * self.step() + half,
            oz + j as i32 * self.step() + half,
        )
    }

    /// The tile's center, in blocks.
    pub fn center(self) -> (f32, f32) {
        let (ox, oz) = self.origin();
        let half = self.span() as f32 / 2.0;
        (ox as f32 + half, oz as f32 + half)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Tile {
    pub key: TileKey,
    /// `TILE_SAMPLES²` samples, row by row over x then z (`j * N + i`).
    pub samples: Vec<Sample>,
}

impl Tile {
    pub fn empty(key: TileKey) -> Self {
        Self {
            key,
            samples: vec![Sample::EMPTY; TILE_SAMPLES * TILE_SAMPLES],
        }
    }

    pub fn get(&self, i: usize, j: usize) -> &Sample {
        &self.samples[j * TILE_SAMPLES + i]
    }

    pub fn set(&mut self, i: usize, j: usize, sample: Sample) {
        self.samples[j * TILE_SAMPLES + i] = sample;
    }

    /// Bytes of memory the samples take.
    pub fn memory(&self) -> usize {
        self.samples.len() * std::mem::size_of::<Sample>() + std::mem::size_of::<Self>()
    }

    /// Whether any sample came from a real chunk.
    pub fn any_real(&self) -> bool {
        self.samples.iter().any(|s| s.real)
    }

    /// The tile as compressed bytes.
    pub fn encode(&self) -> Vec<u8> {
        let mut raw = Vec::with_capacity(self.samples.len() * 12);
        raw.push(self.key.level);
        raw.extend(self.key.tx.to_le_bytes());
        raw.extend(self.key.tz.to_le_bytes());
        for s in &self.samples {
            raw.push(s.count | if s.real { 0x80 } else { 0 });
            for sl in s.slices() {
                raw.extend(sl.top.to_le_bytes());
                raw.extend(sl.bottom.to_le_bytes());
                raw.extend(sl.color);
                raw.push((sl.material as u8) | (sl.light << 4));
            }
        }
        lz4_flex::compress_prepend_size(&raw)
    }

    /// The tile in `bytes`, or `None` if they are damaged.
    pub fn decode(bytes: &[u8]) -> Option<Self> {
        let raw = lz4_flex::decompress_size_prepended(bytes).ok()?;
        let mut at = 0usize;
        let mut take = |n: usize| -> Option<&[u8]> {
            let part = raw.get(at..at + n)?;
            at += n;
            Some(part)
        };
        let level = take(1)?[0];
        if level > MAX_LEVEL {
            return None;
        }
        let tx = i32::from_le_bytes(take(4)?.try_into().ok()?);
        let tz = i32::from_le_bytes(take(4)?.try_into().ok()?);
        let mut tile = Tile::empty(TileKey::new(level, tx, tz));
        for s in tile.samples.iter_mut() {
            let head = take(1)?[0];
            let count = head & 0x7F;
            if count as usize > MAX_SLICES {
                return None;
            }
            s.real = head & 0x80 != 0;
            for _ in 0..count {
                let b = take(8)?;
                s.push(Slice {
                    top: i16::from_le_bytes([b[0], b[1]]),
                    bottom: i16::from_le_bytes([b[2], b[3]]),
                    color: [b[4], b[5], b[6]],
                    material: Material::from_u8(b[7] & 0x0F)?,
                    light: b[7] >> 4,
                });
            }
        }
        Some(tile)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub fn sample_tile() -> Tile {
        let mut tile = Tile::empty(TileKey::new(2, -3, 5));
        for j in 0..TILE_SAMPLES {
            for i in 0..TILE_SAMPLES {
                let mut s = Sample::EMPTY;
                s.real = (i + j) % 3 == 0;
                s.push(Slice {
                    top: (60 + i + j) as i16,
                    bottom: -64,
                    color: [i as u8, j as u8, 7],
                    material: Material::Ground,
                    light: 15,
                });
                if i % 2 == 0 {
                    s.push(Slice {
                        top: 63,
                        bottom: ((61 + i) % 63) as i16,
                        color: [10, 20, 200],
                        material: Material::Water,
                        light: 9,
                    });
                }
                tile.set(i, j, s);
            }
        }
        tile
    }

    #[test]
    fn tile_bytes_round_trip() {
        let tile = sample_tile();
        let bytes = tile.encode();
        assert_eq!(Tile::decode(&bytes).as_ref(), Some(&tile));
    }

    #[test]
    fn damaged_bytes_decode_to_none() {
        let bytes = sample_tile().encode();
        assert!(Tile::decode(&bytes[..bytes.len() / 2]).is_none());
        assert!(Tile::decode(&[1, 2, 3]).is_none());
    }

    #[test]
    fn keys_find_their_tiles() {
        let key = TileKey::containing(1, -1, 64);
        assert_eq!((key.tx, key.tz), (-1, 1));
        let (ox, oz) = key.origin();
        assert!(ox <= -1 && -1 < ox + key.span() && oz <= 64 && 64 < oz + key.span());
        // Level 0 samples are the columns themselves.
        let k0 = TileKey::new(0, 0, 0);
        assert_eq!(k0.sample_block(3, 4), (3, 4));
    }
}
