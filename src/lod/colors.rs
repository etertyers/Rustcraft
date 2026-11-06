//! Block colors for LOD: each block's top texture averaged over the atlas,
//! tinted by the biome the way the mesher tints it.

use crate::render::atlas::Atlas;
use crate::world::Face;
use crate::world::biome::{Biome, BiomeColors};
use crate::world::block::{BlockId, BlockState, Tint, registry};
use crate::world::blocks;

/// What a block is to LOD.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Class {
    /// Air, plants, snow layers and the like: looked through.
    Skip,
    Ground,
    Water,
    Leaves,
    /// Logs and stems: part of a tree, looked through under its canopy.
    Log,
}

pub struct ColorTable {
    /// Average sRGB color of each block's top face, by block id.
    base: Vec<[u8; 3]>,
    classes: Vec<Class>,
    tints: Vec<Tint>,
    biomes: BiomeColors,
}

impl ColorTable {
    /// Builds the table from the atlas (or, with none, from a hash of each
    /// block's name, which tests use).
    pub fn new(atlas: Option<&Atlas>, biomes: BiomeColors) -> Self {
        let reg = registry();
        let count = blocks::NAMES.len();
        let mut base = Vec::with_capacity(count);
        let mut classes = Vec::with_capacity(count);
        let mut tints = Vec::with_capacity(count);
        for i in 0..count {
            let id = BlockId(i as u16);
            let def = reg.def(id);
            let class = if def.fluid.is_some() {
                Class::Water
            } else if id == blocks::OAK_LEAVES || def.acts_as == Some(blocks::OAK_LEAVES) {
                Class::Leaves
            } else if ["_log", "_wood", "_stem", "_hyphae"]
                .iter()
                .any(|end| id.name().ends_with(end))
            {
                Class::Log
            } else if def.solid
                && (def.is_opaque_cube() || id.name().ends_with("_ice") || id.name() == "ice")
            {
                Class::Ground
            } else {
                Class::Skip
            };
            let color = atlas
                .and_then(|atlas| {
                    let textures = def.textures.as_ref()?;
                    let name = textures.get(Face::Top)?;
                    let tile = atlas.get(&crate::world::block::texture_location(name))?;
                    Some(average(atlas, tile))
                })
                .unwrap_or_else(|| hashed(id.name()));
            base.push(color);
            classes.push(class);
            tints.push(def.tint);
        }
        Self {
            base,
            classes,
            tints,
            biomes,
        }
    }

    pub fn class(&self, block: BlockState) -> Class {
        self.classes
            .get(block.id().index())
            .copied()
            .unwrap_or(Class::Skip)
    }

    /// The block's color as seen from above in `biome`.
    pub fn color(&self, block: BlockState, biome: Biome) -> [u8; 3] {
        let i = block.id().index();
        let base = self.base.get(i).copied().unwrap_or([128; 3]);
        let tint = match self.tints.get(i).copied().unwrap_or(Tint::None) {
            Tint::None => return base,
            Tint::GrassTop | Tint::Grass => self.biomes.grass(biome),
            Tint::Foliage => self.biomes.foliage(biome),
            Tint::Water => self.biomes.water(biome),
            Tint::Fixed(hex) => crate::world::biome::rgb(hex),
        };
        [0, 1, 2].map(|c| (f32::from(base[c]) * tint[c]).round().clamp(0.0, 255.0) as u8)
    }

    /// A slice as drawn: its block's color in its biome, with the foliage
    /// share mixed in.
    pub fn slice_color(&self, slice: &super::data::Slice) -> [u8; 3] {
        let biome = Biome::ALL
            .get(usize::from(slice.biome))
            .copied()
            .unwrap_or_default();
        let block = BlockState::of(BlockId(slice.block));
        let base = self.color(block, biome);
        if slice.blend == 0 {
            return base;
        }
        let leaf = self.foliage(biome);
        let t = f32::from(slice.blend) / 255.0;
        [0, 1, 2].map(|i| (f32::from(base[i]) * (1.0 - t) + f32::from(leaf[i]) * t).round() as u8)
    }

    /// The water color of a biome as drawn.
    pub fn water(&self, biome: Biome) -> [u8; 3] {
        self.color(BlockState::of(blocks::WATER), biome)
    }

    /// Foliage color for canopies.
    pub fn foliage(&self, biome: Biome) -> [u8; 3] {
        self.color(BlockState::of(blocks::OAK_LEAVES), biome)
    }
}

fn average(atlas: &Atlas, tile: u32) -> [u8; 3] {
    let [u0, v0, u1, v1] = atlas.uv_rect(tile);
    let image = atlas.image();
    let (w, h) = image.dimensions();
    let (x0, x1) = ((u0 * w as f32) as u32, ((u1 * w as f32) as u32).min(w));
    let (y0, y1) = ((v0 * h as f32) as u32, ((v1 * h as f32) as u32).min(h));
    let mut sum = [0u64; 3];
    let mut n = 0u64;
    for y in y0..y1 {
        for x in x0..x1 {
            let p = image.get_pixel(x, y).0;
            if p[3] > 8 {
                for c in 0..3 {
                    sum[c] += u64::from(p[c]);
                }
                n += 1;
            }
        }
    }
    if n == 0 {
        return [128; 3];
    }
    sum.map(|s| (s / n) as u8)
}

fn hashed(name: &str) -> [u8; 3] {
    let mut h = 0xcbf29ce484222325u64;
    for b in name.bytes() {
        h = (h ^ u64::from(b)).wrapping_mul(0x100000001b3);
    }
    [
        (h >> 8) as u8 | 0x30,
        (h >> 24) as u8 | 0x30,
        (h >> 40) as u8 | 0x30,
    ]
}
