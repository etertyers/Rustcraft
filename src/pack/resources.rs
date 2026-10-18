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

pub struct Resources {
    /// Bottom (the base) first.
    layers: Vec<Pack>,
    /// Older names to try for a texture after its modern one.
    aliases: HashMap<String, Vec<String>>,
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
        Ok(Self::new(Pack::base(assets), packs, aliases))
    }

    pub fn new(base: Pack, packs: Vec<Pack>, aliases: HashMap<String, Vec<String>>) -> Self {
        let mut layers = vec![base];
        layers.extend(packs.into_iter().rev());
        Self { layers, aliases }
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
        None
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
            match serde_json::from_slice(&bytes) {
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
    use crate::{pack::tests::*, world::save::TempDir};

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
        let packs = ["Ashen_1.8.3.zip", "Prime's HD Textures (FREE)"]
            .map(|name| Pack::open(&assets.join(name)).unwrap());
        let res = Resources::load(&assets, packs.into()).unwrap();
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
