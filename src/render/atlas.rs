//! Texture atlas: every block texture packed into one grid of 16×16 tiles.

use std::{collections::HashMap, path::Path};

use anyhow::{Context, Result};
use image::{RgbaImage, imageops};

/// Edge length of one tile in pixels.
pub const TILE_SIZE: u32 = 16;

/// Tile 0 is the magenta/black checkerboard used for missing textures.
pub const MISSING_TILE: u32 = 0;

/// Shrinks UV rects by this fraction of a texel so edges never sample a neighbor tile.
const UV_INSET_TEXELS: f32 = 0.01;

pub struct Atlas {
    image: RgbaImage,
    tiles: HashMap<String, u32>,
    columns: u32,
}

impl Atlas {
    /// Packs every `.png` directly in `dir`, keyed by file stem (`grass_top.png` → `grass_top`).
    pub fn load(dir: &Path) -> Result<Self> {
        let mut textures = Vec::new();
        for entry in std::fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))? {
            let path = entry?.path();
            if path.extension().is_none_or(|ext| ext != "png") {
                continue;
            }
            let name = path
                .file_stem()
                .and_then(|s| s.to_str())
                .with_context(|| format!("bad texture file name {}", path.display()))?
                .to_owned();
            let image = image::open(&path)
                .with_context(|| format!("loading {}", path.display()))?
                .into_rgba8();
            textures.push((name, image));
        }
        // Sort so tile indices don't depend on directory order.
        textures.sort_by(|a, b| a.0.cmp(&b.0));
        log::info!("packed {} textures from {}", textures.len(), dir.display());
        Ok(Self::pack(textures))
    }

    pub fn pack(textures: Vec<(String, RgbaImage)>) -> Self {
        let count = textures.len() as u32 + 1;
        let columns = (count as f32).sqrt().ceil().max(1.0) as u32;
        let columns = columns.next_power_of_two();
        let rows = count.div_ceil(columns);
        let mut image = RgbaImage::new(columns * TILE_SIZE, rows * TILE_SIZE);
        let mut tiles = HashMap::with_capacity(textures.len());

        let mut place = |index: u32, tile: &RgbaImage| {
            let x = (index % columns) * TILE_SIZE;
            let y = (index / columns) * TILE_SIZE;
            imageops::replace(&mut image, tile, x.into(), y.into());
        };
        place(MISSING_TILE, &missing_texture());
        for (i, (name, texture)) in textures.into_iter().enumerate() {
            let index = i as u32 + 1;
            place(index, &to_tile(&texture));
            tiles.insert(name, index);
        }
        Self {
            image,
            tiles,
            columns,
        }
    }

    pub fn image(&self) -> &RgbaImage {
        &self.image
    }

    /// Tile index for a texture name, if it exists.
    pub fn get(&self, name: &str) -> Option<u32> {
        self.tiles.get(name).copied()
    }

    /// UV rect `[u0, v0, u1, v1]` of a tile, with v0 at the top.
    pub fn uv_rect(&self, tile: u32) -> [f32; 4] {
        let (width, height) = self.image.dimensions();
        let inset_u = UV_INSET_TEXELS / width as f32;
        let inset_v = UV_INSET_TEXELS / height as f32;
        let x = ((tile % self.columns) * TILE_SIZE) as f32;
        let y = ((tile / self.columns) * TILE_SIZE) as f32;
        let size = TILE_SIZE as f32;
        [
            x / width as f32 + inset_u,
            y / height as f32 + inset_v,
            (x + size) / width as f32 - inset_u,
            (y + size) / height as f32 - inset_v,
        ]
    }
}

/// Fits a texture to one tile: animated vertical strips keep their first
/// frame, and other sizes are scaled with nearest-neighbor filtering.
fn to_tile(texture: &RgbaImage) -> RgbaImage {
    let (width, height) = texture.dimensions();
    let frame = imageops::crop_imm(texture, 0, 0, width, width.min(height)).to_image();
    if frame.dimensions() == (TILE_SIZE, TILE_SIZE) {
        frame
    } else {
        imageops::resize(&frame, TILE_SIZE, TILE_SIZE, imageops::FilterType::Nearest)
    }
}

/// Magenta and black 2×2 checkerboard, like Minecraft's missing texture.
fn missing_texture() -> RgbaImage {
    let half = TILE_SIZE / 2;
    RgbaImage::from_fn(TILE_SIZE, TILE_SIZE, |x, y| {
        if (x / half + y / half).is_multiple_of(2) {
            image::Rgba([248, 0, 248, 255])
        } else {
            image::Rgba([0, 0, 0, 255])
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(width: u32, height: u32, rgba: [u8; 4]) -> RgbaImage {
        RgbaImage::from_pixel(width, height, image::Rgba(rgba))
    }

    #[test]
    fn missing_tile_is_a_checkerboard() {
        let atlas = Atlas::pack(vec![]);
        let image = atlas.image();
        assert_eq!(image.get_pixel(0, 0).0, [248, 0, 248, 255]);
        assert_eq!(image.get_pixel(8, 0).0, [0, 0, 0, 255]);
        assert_eq!(image.get_pixel(8, 8).0, [248, 0, 248, 255]);
        assert_eq!(atlas.get("nope"), None);
    }

    #[test]
    fn textures_land_in_their_tiles() {
        let red = [255, 0, 0, 255];
        let blue = [0, 0, 255, 255];
        let atlas = Atlas::pack(vec![
            ("red".into(), solid(16, 16, red)),
            ("blue".into(), solid(16, 16, blue)),
        ]);
        for (name, color) in [("red", red), ("blue", blue)] {
            let [u0, v0, ..] = atlas.uv_rect(atlas.get(name).unwrap());
            let (w, h) = atlas.image().dimensions();
            let px = atlas
                .image()
                .get_pixel((u0 * w as f32) as u32, (v0 * h as f32) as u32);
            assert_eq!(px.0, color, "{name}");
        }
    }

    #[test]
    fn animated_strip_keeps_first_frame_and_large_textures_shrink() {
        let mut strip = solid(16, 64, [0, 255, 0, 255]);
        strip.put_pixel(0, 16, image::Rgba([1, 2, 3, 255]));
        assert_eq!(to_tile(&strip), solid(16, 16, [0, 255, 0, 255]));
        assert_eq!(to_tile(&solid(32, 32, [9; 4])), solid(16, 16, [9; 4]));
    }

    #[test]
    fn uv_rects_stay_inside_their_tile() {
        let atlas = Atlas::pack(
            (0..20)
                .map(|i| (i.to_string(), solid(16, 16, [0; 4])))
                .collect(),
        );
        let (w, h) = atlas.image().dimensions();
        for tile in 0..21 {
            let [u0, v0, u1, v1] = atlas.uv_rect(tile);
            let x = (tile % atlas.columns * TILE_SIZE) as f32;
            let y = (tile / atlas.columns * TILE_SIZE) as f32;
            assert!(u0 * w as f32 > x && u1 * (w as f32) < x + 16.0);
            assert!(v0 * h as f32 > y && v1 * (h as f32) < y + 16.0);
        }
    }

    #[test]
    fn real_block_textures_load() {
        let atlas = Atlas::load(&crate::assets::dir().join("textures/blocks")).unwrap();
        assert!(atlas.get("stone").is_some());
        assert!(atlas.get("grass_top").is_some());
    }
}
