//! Texture atlas: every block and item texture packed into one grid of
//! square tiles, sized to the pack's resolution (16 for Kenzo's textures,
//! 32 for a 32× pack). Animated textures show their first frame
//! in the image and change tiles as they play (`Atlas::animate`).

use std::collections::{BTreeSet, HashMap};

use image::{RgbaImage, imageops};
use log::{info, warn};

use crate::pack::{
    Resources, Texture,
    anim::{Player, Shown},
};

/// Smallest tile edge in pixels (Kenzo's textures are 16×16).
pub const MIN_TILE_SIZE: u32 = 16;
/// Largest tile edge; bigger packs are scaled down.
pub const MAX_TILE_SIZE: u32 = 128;
/// Widest and tallest the atlas may be (wgpu's default texture limit).
pub const MAX_ATLAS_SIZE: u32 = 8192;

/// Tile 0 is the magenta/black checkerboard used for missing textures.
pub const MISSING_TILE: u32 = 0;

/// Shrinks UV rects by this fraction of a texel so edges never sample a neighbor tile.
const UV_INSET_TEXELS: f32 = 0.01;

/// An animated texture's tile and what it shows now.
struct AnimatedTile {
    tile: u32,
    player: Player,
    shown: Shown,
}

pub struct Atlas {
    image: RgbaImage,
    tiles: HashMap<String, u32>,
    columns: u32,
    tile_size: u32,
    animations: Vec<AnimatedTile>,
}

/// A tile's frames: one for a still texture, all of them for an animation.
struct Frames {
    frames: Vec<RgbaImage>,
    /// (frame, ticks) in play order; empty for a still texture.
    schedule: Vec<(u32, u32)>,
    interpolate: bool,
}

impl Frames {
    fn of(texture: Texture) -> Self {
        if let Some(anim) = &texture.animation {
            let frames = anim.split(&texture.image);
            if !frames.is_empty() {
                let schedule = anim.schedule(frames.len() as u32);
                return Self {
                    frames,
                    schedule,
                    interpolate: anim.interpolate,
                };
            }
        }
        // A tall strip without a .mcmeta keeps its top square, as before.
        let (width, height) = texture.image.dimensions();
        let side = width.min(height);
        Self::still(imageops::crop_imm(&texture.image, 0, 0, side, side).to_image())
    }

    fn still(image: RgbaImage) -> Self {
        Self {
            frames: vec![image],
            schedule: Vec::new(),
            interpolate: false,
        }
    }

    fn width(&self) -> u32 {
        self.frames[0].width()
    }
}

impl Atlas {
    /// Packs the textures named (`block/stone`, `item/stick`), each from the
    /// highest layer of `res` that has it. Names no layer has are left
    /// out, so they draw as the missing texture.
    pub fn build(res: &Resources, names: impl IntoIterator<Item = String>) -> Self {
        let names: BTreeSet<String> = names.into_iter().collect();
        let mut textures = Vec::with_capacity(names.len());
        let mut missing = Vec::new();
        let mut from_packs = 0;
        for name in names {
            match res.texture(&name) {
                Some(texture) => {
                    from_packs += usize::from(texture.layer > 0);
                    textures.push((name, Frames::of(texture)));
                }
                None => missing.push(name),
            }
        }
        if !missing.is_empty() {
            warn!("no layer has these textures: {missing:?}");
        }
        let atlas = Self::pack_frames(textures);
        info!(
            "atlas: {} textures ({from_packs} from packs, {} animated), {}px tiles, {}×{}",
            atlas.tiles.len(),
            atlas.animations.len(),
            atlas.tile_size,
            atlas.image.width(),
            atlas.image.height()
        );
        atlas
    }

    /// Packs still textures (for tests and tools).
    pub fn pack(textures: Vec<(String, RgbaImage)>) -> Self {
        Self::pack_frames(
            textures
                .into_iter()
                .map(|(name, image)| (name, Frames::still(image)))
                .collect(),
        )
    }

    fn pack_frames(mut textures: Vec<(String, Frames)>) -> Self {
        // Sorted so tile indices don't depend on lookup order.
        textures.sort_by(|a, b| a.0.cmp(&b.0));
        let count = textures.len() as u32 + 1;
        let columns = ((count as f32).sqrt().ceil().max(1.0) as u32).next_power_of_two();
        let rows = count.div_ceil(columns);
        let mut tile_size = common_width(&textures)
            .next_power_of_two()
            .clamp(MIN_TILE_SIZE, MAX_TILE_SIZE);
        while tile_size > MIN_TILE_SIZE && columns.max(rows) * tile_size > MAX_ATLAS_SIZE {
            tile_size /= 2;
        }
        let mut image = RgbaImage::new(columns * tile_size, rows * tile_size);
        let mut tiles = HashMap::with_capacity(textures.len());
        let mut animations = Vec::new();

        let place = |image: &mut RgbaImage, index: u32, tile: &RgbaImage| {
            let x = (index % columns) * tile_size;
            let y = (index / columns) * tile_size;
            imageops::replace(image, tile, x.into(), y.into());
        };
        place(&mut image, MISSING_TILE, &missing_texture(tile_size));
        for (i, (name, frames)) in textures.into_iter().enumerate() {
            let index = i as u32 + 1;
            let Frames {
                frames,
                schedule,
                interpolate,
            } = frames;
            let frames: Vec<RgbaImage> = frames.iter().map(|f| to_tile(f, tile_size)).collect();
            let first = schedule.first().map_or(0, |&(frame, _)| frame as usize);
            place(&mut image, index, &frames[first]);
            if let Some(player) = Player::new(frames, schedule, interpolate) {
                animations.push(AnimatedTile {
                    tile: index,
                    shown: player.shown(0),
                    player,
                });
            }
            tiles.insert(name, index);
        }
        Self {
            image,
            tiles,
            columns,
            tile_size,
            animations,
        }
    }

    pub fn image(&self) -> &RgbaImage {
        &self.image
    }

    /// Edge of one tile in pixels.
    pub fn tile_size(&self) -> u32 {
        self.tile_size
    }

    /// Tile index for a texture name, if it exists.
    pub fn get(&self, name: &str) -> Option<u32> {
        self.tiles.get(name).copied()
    }

    /// Whether the atlas holds a texture called `name`.
    pub fn has(&self, name: &str) -> bool {
        self.tiles.contains_key(name)
    }

    /// UV rect `[u0, v0, u1, v1]` of a tile, with v0 at the top.
    pub fn uv_rect(&self, tile: u32) -> [f32; 4] {
        let (width, height) = self.image.dimensions();
        let inset_u = UV_INSET_TEXELS / width as f32;
        let inset_v = UV_INSET_TEXELS / height as f32;
        let (x, y) = self.tile_origin(tile);
        let size = self.tile_size as f32;
        [
            x as f32 / width as f32 + inset_u,
            y as f32 / height as f32 + inset_v,
            (x as f32 + size) / width as f32 - inset_u,
            (y as f32 + size) / height as f32 - inset_v,
        ]
    }

    /// The UV rect of a texture, or of the missing texture.
    pub fn uv_or_missing(&self, name: &str) -> [f32; 4] {
        self.uv_rect(self.get(name).unwrap_or(MISSING_TILE))
    }

    /// Pixel position of a tile's top-left corner.
    fn tile_origin(&self, tile: u32) -> (u32, u32) {
        (
            (tile % self.columns) * self.tile_size,
            (tile / self.columns) * self.tile_size,
        )
    }

    /// How many textures animate.
    pub fn animated(&self) -> usize {
        self.animations.len()
    }

    /// Moves animations to game tick `tick`; returns the tiles whose picture
    /// changed as (x, y, picture), to copy into the GPU texture.
    pub fn animate(&mut self, tick: u64) -> Vec<(u32, u32, RgbaImage)> {
        let mut changed = Vec::new();
        for i in 0..self.animations.len() {
            let anim = &mut self.animations[i];
            let shown = anim.player.shown(tick);
            if shown == anim.shown {
                continue;
            }
            anim.shown = shown;
            let picture = anim.player.image(shown);
            let (x, y) = self.tile_origin(self.animations[i].tile);
            changed.push((x, y, picture));
        }
        changed
    }
}

/// The pack's resolution: the most common texture width (the widest of
/// equally common ones). Odd ones out, like vanilla's 32-wide flowing
/// water in a 16× pack, are scaled to fit.
fn common_width(textures: &[(String, Frames)]) -> u32 {
    let mut counts: HashMap<u32, usize> = HashMap::new();
    for (_, frames) in textures {
        *counts.entry(frames.width()).or_default() += 1;
    }
    counts
        .into_iter()
        .max_by_key(|&(width, count)| (count, width))
        .map_or(0, |(width, _)| width)
}

/// Scales a frame to a `size`-pixel tile with nearest-neighbor filtering.
fn to_tile(frame: &RgbaImage, size: u32) -> RgbaImage {
    if frame.dimensions() == (size, size) {
        frame.clone()
    } else {
        imageops::resize(frame, size, size, imageops::FilterType::Nearest)
    }
}

/// Magenta and black 2×2 checkerboard, like Minecraft's missing texture.
fn missing_texture(size: u32) -> RgbaImage {
    let half = size / 2;
    RgbaImage::from_fn(size, size, |x, y| {
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
    use crate::pack::anim::Animation;

    fn solid(width: u32, height: u32, rgba: [u8; 4]) -> RgbaImage {
        RgbaImage::from_pixel(width, height, image::Rgba(rgba))
    }

    fn pixel_at(atlas: &Atlas, name: &str) -> [u8; 4] {
        let [u0, v0, ..] = atlas.uv_rect(atlas.get(name).unwrap());
        let (w, h) = atlas.image().dimensions();
        atlas
            .image()
            .get_pixel((u0 * w as f32) as u32, (v0 * h as f32) as u32)
            .0
    }

    #[test]
    fn missing_tile_is_a_checkerboard() {
        let atlas = Atlas::pack(vec![]);
        let image = atlas.image();
        assert_eq!(image.get_pixel(0, 0).0, [248, 0, 248, 255]);
        assert_eq!(image.get_pixel(8, 0).0, [0, 0, 0, 255]);
        assert_eq!(image.get_pixel(8, 8).0, [248, 0, 248, 255]);
        assert_eq!(atlas.get("nope"), None);
        assert_eq!(atlas.uv_or_missing("nope"), atlas.uv_rect(MISSING_TILE));
    }

    #[test]
    fn textures_land_in_their_tiles() {
        let red = [255, 0, 0, 255];
        let blue = [0, 0, 255, 255];
        let atlas = Atlas::pack(vec![
            ("red".into(), solid(16, 16, red)),
            ("blue".into(), solid(16, 16, blue)),
        ]);
        assert_eq!(pixel_at(&atlas, "red"), red);
        assert_eq!(pixel_at(&atlas, "blue"), blue);
    }

    #[test]
    fn tiles_match_the_common_size_and_odd_ones_scale() {
        let atlas = Atlas::pack(vec![
            ("hd".into(), solid(32, 32, [1; 4])),
            ("sd".into(), solid(16, 16, [2; 4])),
        ]);
        assert_eq!(atlas.tile_size(), 32);
        let (x, y) = atlas.tile_origin(atlas.get("sd").unwrap());
        assert_eq!(atlas.image().get_pixel(x + 31, y + 31).0, [2; 4]);
        // Checkerboard squares stay a quarter of the tile.
        assert_eq!(atlas.image().get_pixel(15, 0).0, [248, 0, 248, 255]);
        assert_eq!(atlas.image().get_pixel(16, 0).0, [0, 0, 0, 255]);
        // Huge textures shrink to the largest tile.
        let big = Atlas::pack(vec![("big".into(), solid(512, 512, [3; 4]))]);
        assert_eq!(big.tile_size(), MAX_TILE_SIZE);
        // One wide texture among many small ones doesn't set the size.
        let mostly_small = Atlas::pack(vec![
            ("flow".into(), solid(32, 32, [1; 4])),
            ("a".into(), solid(16, 16, [2; 4])),
            ("b".into(), solid(16, 16, [3; 4])),
        ]);
        assert_eq!(mostly_small.tile_size(), 16);
    }

    #[test]
    fn strips_without_mcmeta_keep_their_first_frame() {
        let mut strip = solid(16, 64, [0, 255, 0, 255]);
        strip.put_pixel(0, 16, image::Rgba([1, 2, 3, 255]));
        let texture = Texture {
            image: strip,
            animation: None,
            layer: 0,
            vanilla: false,
        };
        let frames = Frames::of(texture);
        assert_eq!(frames.frames, [solid(16, 16, [0, 255, 0, 255])]);
        assert!(frames.schedule.is_empty());
    }

    #[test]
    fn animations_change_their_tile_as_ticks_pass() {
        let mut strip = solid(16, 32, [10, 0, 0, 255]);
        for y in 16..32 {
            for x in 0..16 {
                strip.put_pixel(x, y, image::Rgba([20, 0, 0, 255]));
            }
        }
        let texture = Texture {
            image: strip,
            animation: Animation::parse(r#"{"animation": {"frametime": 3}}"#).unwrap(),
            layer: 1,
            vanilla: false,
        };
        let mut atlas = Atlas::pack_frames(vec![
            ("water".into(), Frames::of(texture)),
            ("stone".into(), Frames::still(solid(16, 16, [9; 4]))),
        ]);
        assert_eq!(atlas.animated(), 1);
        assert_eq!(pixel_at(&atlas, "water"), [10, 0, 0, 255]);
        assert!(atlas.animate(2).is_empty());
        let changed = atlas.animate(3);
        assert_eq!(changed.len(), 1);
        let (x, y, picture) = &changed[0];
        assert_eq!((*x, *y), atlas.tile_origin(atlas.get("water").unwrap()));
        assert_eq!(picture.get_pixel(0, 0).0, [20, 0, 0, 255]);
        assert!(atlas.animate(4).is_empty());
        assert_eq!(atlas.animate(6).len(), 1);
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
            let (x, y) = atlas.tile_origin(tile);
            let (x, y) = (x as f32, y as f32);
            assert!(u0 * w as f32 > x && u1 * (w as f32) < x + 16.0);
            assert!(v0 * h as f32 > y && v1 * (h as f32) < y + 16.0);
        }
    }
}
