//! The stack of resource layers: Kenzo's textures at the bottom, then the
//! selected packs. Every lookup takes the highest layer that has the file,
//! so anything a pack leaves out falls back to the layer below.

use std::{collections::HashMap, path::Path};

use anyhow::{Context, Result};
use image::RgbaImage;
use log::warn;

use super::{Pack, anim::Animation};

/// A texture and how it animates.
#[derive(Debug, Clone)]
pub struct Texture {
    pub image: RgbaImage,
    pub animation: Option<Animation>,
    /// The layer it came from (0 is the base).
    pub layer: usize,
}

impl Texture {
    /// The texture, or its first frame if it's animated.
    pub fn first_frame(self) -> RgbaImage {
        match &self.animation {
            Some(anim) => anim
                .split(&self.image)
                .into_iter()
                .next()
                .unwrap_or(self.image),
            None => self.image,
        }
    }
}

/// Where on a sheet a texture is: the sheet, and x, y, width and height
/// in 128-pixel sheet units.
type Slice = (String, (u32, u32, u32, u32));

pub struct Resources {
    /// Bottom (the base) first.
    layers: Vec<Pack>,
    /// Older names to try for a texture after its modern one.
    aliases: HashMap<String, Vec<String>>,
    /// Textures that older layouts keep on a shared sheet.
    slices: HashMap<String, Slice>,
}

/// A shield's picture for the inventory: the plate on the front of its
/// 64×64 entity texture (12×22 pixels from 1,1), fitted into a square tile.
fn shield_icon(sheet: &RgbaImage) -> RgbaImage {
    let scale = (sheet.width() / 64).max(1);
    let plate = image::imageops::crop_imm(sheet, scale, scale, 12 * scale, 22 * scale).to_image();
    let tile = 16 * scale;
    let width = (tile * 12).div_ceil(22);
    let plate = image::imageops::resize(&plate, width, tile, image::imageops::FilterType::Nearest);
    let mut icon = RgbaImage::new(tile, tile);
    image::imageops::replace(&mut icon, &plate, ((tile - width) / 2).into(), 0);
    icon
}

/// `minecraft:block/stone` → `block/stone`.
pub fn location(name: &str) -> &str {
    name.strip_prefix("minecraft:").unwrap_or(name)
}

impl Resources {
    /// The base layer from `assets`, with `packs` (highest priority first)
    /// over it, and the texture names in `assets/data/texture_names.ron`.
    pub fn load(assets: &Path, packs: Vec<Pack>) -> Result<Self> {
        let path = assets.join("data/texture_names.ron");
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        let aliases =
            ron::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
        let path = assets.join("data/texture_slices.ron");
        let slices = match std::fs::read_to_string(&path) {
            Ok(text) => {
                ron::from_str(&text).with_context(|| format!("parsing {}", path.display()))?
            }
            Err(_) => HashMap::new(),
        };
        Ok(Self::new(Pack::base(assets), packs, aliases).with_slices(slices))
    }

    /// Adds the sheets textures can be cut from.
    pub fn with_slices(mut self, slices: HashMap<String, Slice>) -> Self {
        self.slices = slices;
        self
    }

    pub fn new(base: Pack, packs: Vec<Pack>, aliases: HashMap<String, Vec<String>>) -> Self {
        let mut layers = vec![base];
        layers.extend(packs.into_iter().rev());
        Self {
            layers,
            aliases,
            slices: HashMap::new(),
        }
    }

    /// Bottom (the base) first.
    pub fn layers(&self) -> &[Pack] {
        &self.layers
    }

    /// Paths a texture may have in a pack, best first: its modern
    /// location, its older names, and the same in the pre-1.13 folders.
    fn candidates(&self, name: &str) -> Vec<String> {
        let name = location(name);
        let mut names = vec![name.to_owned()];
        names.extend(self.aliases.get(name).into_iter().flatten().cloned());
        let old_folders: Vec<String> = names
            .iter()
            .filter_map(|n| {
                let (folder, rest) = n.split_once('/')?;
                let old = match folder {
                    "block" => "blocks",
                    "item" => "items",
                    _ => return None,
                };
                Some(format!("{old}/{rest}"))
            })
            .collect();
        for old in old_folders {
            if !names.contains(&old) {
                names.push(old);
            }
        }
        names
            .into_iter()
            .map(|n| format!("textures/{n}.png"))
            .collect()
    }

    /// A texture by its modern location (`block/stone`), from the highest
    /// layer that has it.
    pub fn texture(&self, name: &str) -> Option<Texture> {
        (0..self.layers.len())
            .rev()
            .find_map(|layer| self.texture_in(layer, name))
    }

    /// A texture from one layer only.
    pub fn texture_in(&self, layer: usize, name: &str) -> Option<Texture> {
        let pack = &self.layers[layer];
        for path in self.candidates(name) {
            let Some(bytes) = pack.read(&path) else {
                continue;
            };
            let image = match image::load_from_memory(&bytes) {
                Ok(image) => image.into_rgba8(),
                Err(err) => {
                    warn!("can't decode {path} in {}: {err}", pack.name());
                    continue;
                }
            };
            let animation = pack.read(&format!("{path}.mcmeta")).and_then(|meta| {
                Animation::parse(&String::from_utf8_lossy(&meta)).unwrap_or_else(|err| {
                    warn!("{path}.mcmeta in {}: {err:#}", pack.name());
                    None
                })
            });
            return Some(Texture {
                image,
                animation,
                layer,
            });
        }
        self.slice_in(layer, name)
            .or_else(|| self.derived_in(layer, name))
    }

    /// Textures made from others because no layout has them as files: the
    /// shield's inventory picture is the front of its entity texture.
    fn derived_in(&self, layer: usize, name: &str) -> Option<Texture> {
        if location(name) != "item/shield" {
            return None;
        }
        let sheet = self.texture_in(layer, "entity/shield_base_nopattern")?;
        Some(Texture {
            image: shield_icon(&sheet.image),
            animation: None,
            layer,
        })
    }

    /// A texture cut from a sheet in one layer, if the name is on a sheet.
    fn slice_in(&self, layer: usize, name: &str) -> Option<Texture> {
        let (sheet, (x, y, w, h)) = self.slices.get(location(name))?;
        let sheet = self.texture_in(layer, sheet)?;
        let scale = (sheet.image.width() / 128).max(1);
        let (x, y, w, h) = (x * scale, y * scale, w * scale, h * scale);
        let (iw, ih) = sheet.image.dimensions();
        if x + w > iw || y + h > ih {
            return None;
        }
        Some(Texture {
            image: image::imageops::crop_imm(&sheet.image, x, y, w, h).to_image(),
            animation: None,
            layer,
        })
    }

    /// A texture's first frame, from the highest layer that has it.
    pub fn image(&self, name: &str) -> Option<RgbaImage> {
        self.texture(name).map(Texture::first_frame)
    }

    /// A JSON file (`models/block/stone.json`) from the highest layer that
    /// has a readable one.
    pub fn json(&self, path: &str) -> Option<serde_json::Value> {
        self.layers.iter().rev().find_map(|pack| {
            let bytes = pack.read(path)?;
            match super::parse_json(&String::from_utf8_lossy(&bytes)) {
                Ok(json) => Some(json),
                Err(err) => {
                    warn!("can't read {path} in {}: {err}", pack.name());
                    None
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;
    use crate::{
        pack::tests::{ASHEN, MCMETA, PRIME, kenzos_pack, write_zip},
        world::save::TempDir,
    };

    fn png(rgba: [u8; 4], width: u32, height: u32) -> Vec<u8> {
        let mut bytes = Vec::new();
        RgbaImage::from_pixel(width, height, image::Rgba(rgba))
            .write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Png)
            .unwrap();
        bytes
    }

    /// A base layer in 1.12 layout and two packs over it.
    fn stack(dir: &Path) -> Resources {
        let base = dir.join("base");
        std::fs::create_dir_all(base.join("textures/blocks")).unwrap();
        std::fs::create_dir_all(base.join("textures/items")).unwrap();
        let write = |path: &str, bytes: &[u8]| std::fs::write(base.join(path), bytes).unwrap();
        write(
            "textures/blocks/planks_oak.png",
            &png([1, 0, 0, 255], 16, 16),
        );
        write("textures/blocks/stone.png", &png([2, 0, 0, 255], 16, 16));
        write("textures/blocks/dirt.png", &png([3, 0, 0, 255], 16, 16));
        write("textures/items/stick.png", &png([4, 0, 0, 255], 16, 16));
        write(
            "textures/blocks/water_still.png",
            &png([5, 0, 0, 255], 16, 64),
        );
        write(
            "textures/blocks/water_still.png.mcmeta",
            br#"{"animation": {"frametime": 2}}"#,
        );
        let low = dir.join("low.zip");
        write_zip(
            &low,
            &[
                ("pack.mcmeta", MCMETA),
                (
                    "assets/minecraft/textures/block/stone.png",
                    &png([20, 0, 0, 255], 32, 32),
                ),
                (
                    "assets/minecraft/textures/block/dirt.png",
                    &png([30, 0, 0, 255], 32, 32),
                ),
            ],
        );
        let high = dir.join("high.zip");
        write_zip(
            &high,
            &[
                ("pack.mcmeta", MCMETA),
                (
                    "assets/minecraft/textures/block/stone.png",
                    &png([200, 0, 0, 255], 16, 16),
                ),
                ("assets/minecraft/textures/block/broken.png", b"not a png"),
                (
                    "assets/minecraft/models/block/stone.json",
                    br#"{"parent": "block/cube_all"}"#,
                ),
            ],
        );
        let aliases = HashMap::from([(
            "block/oak_planks".to_owned(),
            vec!["blocks/planks_oak".to_owned()],
        )]);
        Resources::new(
            Pack::base(&base),
            vec![Pack::open(&high).unwrap(), Pack::open(&low).unwrap()],
            aliases,
        )
    }

    fn red(texture: Option<Texture>) -> Option<(u8, usize)> {
        texture.map(|t| (t.image.get_pixel(0, 0).0[0], t.layer))
    }

    #[test]
    fn higher_packs_win_and_gaps_fall_through() {
        let dir = TempDir::new("stack");
        let res = stack(dir.path());
        assert_eq!(res.layers().len(), 3);
        assert_eq!(res.layers()[2].name(), "high");
        assert_eq!(red(res.texture("block/stone")), Some((200, 2)));
        assert_eq!(red(res.texture("minecraft:block/dirt")), Some((30, 1)));
        // Only the base has it, under its 1.12 name and folder.
        assert_eq!(red(res.texture("block/oak_planks")), Some((1, 0)));
        assert_eq!(red(res.texture("item/stick")), Some((4, 0)));
        assert!(res.texture("block/nothing").is_none());
        assert_eq!(red(res.texture_in(1, "block/stone")), Some((20, 1)));
        assert!(res.texture_in(1, "block/oak_planks").is_none());
    }

    #[test]
    fn textures_on_sheets_are_cut_out_unless_a_layer_has_them_whole() {
        let dir = TempDir::new("slices");
        let base = dir.path().join("base");
        std::fs::create_dir_all(base.join("textures/particle")).unwrap();
        // A 128-pixel sheet that is red at (8, 32) and blue elsewhere.
        let mut sheet = RgbaImage::from_pixel(128, 128, image::Rgba([0, 0, 255, 255]));
        for y in 32..40 {
            for x in 8..16 {
                sheet.put_pixel(x, y, image::Rgba([255, 0, 0, 255]));
            }
        }
        let mut bytes = Vec::new();
        sheet
            .write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Png)
            .unwrap();
        std::fs::write(base.join("textures/particle/particles.png"), &bytes).unwrap();
        let slices = HashMap::from([(
            "particle/critical_hit".to_owned(),
            ("particle/particles".to_owned(), (8, 32, 8, 8)),
        )]);
        let pack = dir.path().join("whole.zip");
        write_zip(
            &pack,
            &[
                ("pack.mcmeta", MCMETA),
                (
                    "assets/minecraft/textures/particle/critical_hit.png",
                    &png([0, 255, 0, 255], 8, 8),
                ),
            ],
        );
        let bare = Resources::new(Pack::base(&base), Vec::new(), HashMap::new())
            .with_slices(slices.clone());
        let cut = bare.texture("minecraft:particle/critical_hit").unwrap();
        assert_eq!(cut.image.dimensions(), (8, 8));
        assert_eq!(cut.image.get_pixel(3, 3).0, [255, 0, 0, 255]);
        // A pack with its own file wins; one with a bigger sheet scales the cut.
        let over = Resources::new(
            Pack::base(&base),
            vec![Pack::open(&pack).unwrap()],
            HashMap::new(),
        )
        .with_slices(slices);
        let whole = over.texture("particle/critical_hit").unwrap();
        assert_eq!(
            (whole.layer, whole.image.get_pixel(0, 0).0),
            (1, [0, 255, 0, 255])
        );
        assert!(bare.texture("particle/nothing").is_none());
    }

    #[test]
    fn the_shield_icon_is_cut_from_its_entity_texture() {
        let dir = TempDir::new("shield");
        let base = dir.path().join("base");
        std::fs::create_dir_all(base.join("textures/entity")).unwrap();
        // A sheet that is red inside the 12×22 plate at (1,1) and blue outside.
        let mut sheet = RgbaImage::from_pixel(64, 64, image::Rgba([0, 0, 255, 255]));
        for y in 1..23 {
            for x in 1..13 {
                sheet.put_pixel(x, y, image::Rgba([255, 0, 0, 255]));
            }
        }
        let mut bytes = Vec::new();
        sheet
            .write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Png)
            .unwrap();
        std::fs::write(
            base.join("textures/entity/shield_base_nopattern.png"),
            &bytes,
        )
        .unwrap();
        let res = Resources::new(Pack::base(&base), Vec::new(), HashMap::new());
        let icon = res.texture("item/shield").unwrap().image;
        assert_eq!(icon.dimensions(), (16, 16));
        // The plate fills the height and leaves clear margins at the sides.
        assert_eq!(icon.get_pixel(8, 0).0, [255, 0, 0, 255]);
        assert_eq!(icon.get_pixel(8, 15).0, [255, 0, 0, 255]);
        assert_eq!(icon.get_pixel(0, 8).0[3], 0);
        assert_eq!(icon.get_pixel(15, 8).0[3], 0);
        assert!(res.texture("item/bucket").is_none());
    }

    #[test]
    fn broken_files_fall_back_with_a_warning() {
        let dir = TempDir::new("broken");
        let res = stack(dir.path());
        assert!(res.texture("block/broken").is_none());
        assert!(res.json("models/block/stone.json").is_some());
        assert!(res.json("models/block/dirt.json").is_none());
    }

    #[test]
    fn animated_textures_bring_their_mcmeta() {
        let dir = TempDir::new("anim");
        let res = stack(dir.path());
        let water = res.texture("block/water_still").unwrap();
        assert_eq!(water.animation.as_ref().unwrap().frametime, 2);
        assert_eq!(
            res.image("block/water_still").unwrap().dimensions(),
            (16, 16)
        );
    }

    #[test]
    fn every_texture_name_in_the_data_files_resolves_in_the_base_or_a_pack() {
        let assets = crate::assets::dir();
        let (Some(ashen), Some(prime)) = (kenzos_pack(ASHEN), kenzos_pack(PRIME)) else {
            return;
        };
        let res = Resources::load(&assets, vec![ashen, prime]).unwrap();
        let blocks = crate::world::block::registry();
        let missing = blocks.missing_textures(|name| res.texture(name).is_some());
        // Drawn as entities in modern Minecraft; v1.0 never had them either.
        let expected = ["block/end_portal", "block/sign"].map(str::to_owned);
        assert_eq!(missing, expected.into());
        let items = crate::inventory::item::registry();
        let missing = items.missing_textures(|name| res.texture(name).is_some());
        assert!(missing.is_empty(), "{missing:?}");
    }
}
