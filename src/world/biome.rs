//! The four 1.0.0 biomes Rustcraft generates, and their grass/foliage colors.

use std::path::Path;

use anyhow::{Context, Result};
use image::RgbaImage;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Biome {
    Ocean,
    #[default]
    Plains,
    Forest,
    Desert,
}

impl Biome {
    pub const ALL: [Biome; 4] = [Biome::Ocean, Biome::Plains, Biome::Forest, Biome::Desert];

    pub fn name(self) -> &'static str {
        match self {
            Biome::Ocean => "Ocean",
            Biome::Plains => "Plains",
            Biome::Forest => "Forest",
            Biome::Desert => "Desert",
        }
    }

    /// 1.0.0's (temperature, rainfall), which pick the biome's colors.
    pub fn climate(self) -> (f32, f32) {
        match self {
            Biome::Ocean => (0.5, 0.5),
            Biome::Plains => (0.8, 0.4),
            Biome::Forest => (0.7, 0.8),
            Biome::Desert => (2.0, 0.0),
        }
    }

    fn index(self) -> usize {
        self as usize
    }
}

/// sRGB grass and foliage colors for each biome.
#[derive(Debug, Clone, PartialEq)]
pub struct BiomeColors {
    grass: [[f32; 3]; 4],
    foliage: [[f32; 3]; 4],
}

impl BiomeColors {
    /// Samples `grass.png` and `foliage.png` from a colormap folder.
    pub fn load(dir: &Path) -> Result<Self> {
        let open = |name: &str| -> Result<RgbaImage> {
            let path = dir.join(name);
            Ok(image::open(&path)
                .with_context(|| format!("loading {}", path.display()))?
                .into_rgba8())
        };
        Ok(Self::from_colormaps(
            &open("grass.png")?,
            &open("foliage.png")?,
        ))
    }

    /// Looks each biome's climate up in the colormaps, the way 1.0.0 does.
    pub fn from_colormaps(grass: &RgbaImage, foliage: &RgbaImage) -> Self {
        let sample = |map: &RgbaImage, biome: Biome| {
            let (temperature, rainfall) = biome.climate();
            let (x, y) = colormap_coords(temperature, rainfall, map.width(), map.height());
            let [r, g, b, _] = map.get_pixel(x, y).0;
            [r, g, b].map(|c| c as f32 / 255.0)
        };
        Self {
            grass: Biome::ALL.map(|b| sample(grass, b)),
            foliage: Biome::ALL.map(|b| sample(foliage, b)),
        }
    }

    /// Every biome uses 1.0.0's vanilla plains colors (for tests, or if the
    /// colormaps are missing).
    pub fn uniform() -> Self {
        let grass = rgb(0x91BD59);
        let foliage = rgb(0x77AB2F);
        Self {
            grass: [grass; 4],
            foliage: [foliage; 4],
        }
    }

    pub fn grass(&self, biome: Biome) -> [f32; 3] {
        self.grass[biome.index()]
    }

    pub fn foliage(&self, biome: Biome) -> [f32; 3] {
        self.foliage[biome.index()]
    }
}

/// 0xRRGGBB to sRGB floats.
pub fn rgb(hex: u32) -> [f32; 3] {
    [hex >> 16, hex >> 8, hex].map(|c| (c & 0xFF) as f32 / 255.0)
}

/// Pixel in a colormap for a climate: temperature runs right to left and
/// rainfall × temperature runs bottom to top, both clamped to 0..1.
fn colormap_coords(temperature: f32, rainfall: f32, width: u32, height: u32) -> (u32, u32) {
    let t = temperature.clamp(0.0, 1.0);
    let r = rainfall.clamp(0.0, 1.0) * t;
    let x = ((1.0 - t) * (width - 1) as f32) as u32;
    let y = ((1.0 - r) * (height - 1) as f32) as u32;
    (x, y)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colormap_coords_match_1_0_0() {
        // Plains: temperature 0.8, rainfall 0.4 × 0.8. (1 - 0.8) × 255 is just
        // under 51 in floating point, and 1.0.0 truncates it to 50 as well.
        assert_eq!(colormap_coords(0.8, 0.4, 256, 256), (50, 173));
        // Desert is hotter than 1.0 and clamps to the hot, dry corner.
        assert_eq!(colormap_coords(2.0, 0.0, 256, 256), (0, 255));
    }

    #[test]
    fn each_biome_samples_its_own_pixel() {
        let map = RgbaImage::from_fn(256, 256, |x, y| image::Rgba([x as u8, y as u8, 0, 255]));
        let colors = BiomeColors::from_colormaps(&map, &map);
        let plains = colors.grass(Biome::Plains);
        assert_eq!(plains, [50.0 / 255.0, 173.0 / 255.0, 0.0]);
        assert_ne!(colors.grass(Biome::Desert), plains);
    }

    #[test]
    fn real_colormaps_load() {
        let dir = crate::assets::dir().join("textures/colormap");
        let colors = BiomeColors::load(&dir).unwrap();
        // Forest grass is a different green from desert grass.
        assert_ne!(colors.grass(Biome::Forest), colors.grass(Biome::Desert));
    }

    #[test]
    fn rgb_unpacks_hex() {
        assert_eq!(rgb(0xFF8000), [1.0, 128.0 / 255.0, 0.0]);
    }
}
