//! Minecraft Java resource packs: a folder or `.zip` with a `pack.mcmeta`
//! and an `assets/minecraft/` tree, read by modern resource names
//! (`textures/block/stone.png`). Kenzo's v1.0 textures in `assets/` are the
//! base layer under every pack (see `Resources`).

pub mod anim;
pub mod model;
mod resources;

use std::{
    collections::HashSet,
    fs::File,
    io::{BufReader, Read},
    path::{Path, PathBuf},
    sync::Mutex,
};

use anyhow::{Context, Result, bail};
use log::warn;
use zip::ZipArchive;

pub use resources::{Resources, Texture};

/// Where a pack's `minecraft` namespace starts.
const NAMESPACE_ROOT: &str = "assets/minecraft/";

/// The base layer's id in the list of layers.
pub const BASE_ID: &str = "default";

/// What `pack.mcmeta` says about a pack.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PackInfo {
    /// Its description, with formatting codes removed.
    pub description: String,
    /// The pack formats it was made for, lowest and highest.
    pub formats: Option<(u32, u32)>,
}

/// Reads the first JSON value in `text`, ignoring anything after it, as
/// Minecraft's lenient reader does (some packs' files have a stray
/// closing brace at the end).
pub fn parse_json(text: &str) -> serde_json::Result<serde_json::Value> {
    let text = text.trim_start_matches('\u{feff}');
    serde_json::Deserializer::from_str(text)
        .into_iter()
        .next()
        .unwrap_or_else(|| serde_json::from_str(""))
}

/// The newest pack format the game knows (1.21.11's resource packs are 75).
pub const PACK_FORMAT: u32 = 75;

impl PackInfo {
    /// Reads a `pack.mcmeta`: `pack_format`, or modern `min_format` and
    /// `max_format` (numbers or `[major, minor]`), or `supported_formats`.
    pub fn parse(text: &str) -> Result<Self> {
        let json = parse_json(text)?;
        let pack = json.get("pack").context("pack.mcmeta has no \"pack\"")?;
        let major = |value: &serde_json::Value| {
            value
                .as_u64()
                .or_else(|| value.get(0).and_then(|v| v.as_u64()))
                .map(|v| v as u32)
        };
        let single = pack.get("pack_format").and_then(major);
        let range = match (
            pack.get("min_format").and_then(major),
            pack.get("max_format").and_then(major),
        ) {
            (Some(min), Some(max)) => Some((min, max)),
            _ => pack.get("supported_formats").and_then(|s| {
                let ends = s.as_array().map(|a| a.as_slice()).unwrap_or_default();
                match ends {
                    [min, max] => Some((major(min)?, major(max)?)),
                    _ => Some((
                        s.get("min_inclusive").and_then(major)?,
                        s.get("max_inclusive").and_then(major)?,
                    )),
                }
            }),
        };
        let formats = range.or(single.map(|f| (f, f)));
        let description = pack.get("description").map_or_else(String::new, text_of);
        Ok(Self {
            description: strip_formatting(&description),
            formats,
        })
    }

    /// Whether the pack was made for an older or newer game, as the
    /// Resource Packs screen warns ("Made for an older version").
    pub fn compatibility(&self) -> Compatibility {
        match self.formats {
            Some((_, max)) if max < PACK_FORMAT => Compatibility::Older,
            Some((min, _)) if min > PACK_FORMAT => Compatibility::Newer,
            _ => Compatibility::Ok,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Compatibility {
    Ok,
    Older,
    Newer,
}

/// The plain text of a JSON text component: a string, `{"text": ...,
/// "extra": [...]}`, or a list of components.
fn text_of(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Array(parts) => parts.iter().map(text_of).collect(),
        serde_json::Value::Object(map) => {
            let mut text = map.get("text").map_or_else(String::new, text_of);
            if let Some(extra) = map.get("extra") {
                text.push_str(&text_of(extra));
            }
            text
        }
        other => other.to_string(),
    }
}

/// Removes `§` formatting codes (`§6gold` → `gold`).
fn strip_formatting(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c == '§' {
            chars.next();
        } else {
            out.push(c);
        }
    }
    out
}

enum Files {
    Dir(PathBuf),
    Zip {
        archive: Mutex<ZipArchive<BufReader<File>>>,
        /// Every file's name, relative to the pack's root.
        names: HashSet<String>,
        /// The folder inside the zip holding `pack.mcmeta`, if not its root.
        prefix: String,
    },
}

/// One layer of resources: a resource pack, or the base textures.
pub struct Pack {
    /// The folder or zip's file name, as kept in the options.
    pub id: String,
    pub info: PackInfo,
    /// The pack's `pack.png`, if it has one.
    pub icon: Option<Vec<u8>>,
    files: Files,
    /// Where the `minecraft` namespace starts inside the pack.
    root: &'static str,
}

impl Pack {
    /// Opens a resource pack folder or `.zip`.
    pub fn open(path: &Path) -> Result<Self> {
        let id = path
            .file_name()
            .and_then(|n| n.to_str())
            .with_context(|| format!("bad pack name {}", path.display()))?
            .to_owned();
        let files = if path.is_dir() {
            Files::Dir(path.to_owned())
        } else if path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("zip"))
        {
            open_zip(path)?
        } else {
            bail!("{} is neither a folder nor a .zip", path.display());
        };
        let mut pack = Self {
            id,
            info: PackInfo::default(),
            icon: None,
            files,
            root: NAMESPACE_ROOT,
        };
        let mcmeta = pack
            .read_file("pack.mcmeta")
            .with_context(|| format!("{} has no pack.mcmeta", path.display()))?;
        pack.info = PackInfo::parse(&String::from_utf8_lossy(&mcmeta))
            .with_context(|| format!("reading {}/pack.mcmeta", path.display()))?;
        pack.icon = pack.read_file("pack.png");
        Ok(pack)
    }

    /// Kenzo's textures: `assets/` read as a namespace root, so
    /// `textures/blocks/stone.png` is `assets/textures/blocks/stone.png`.
    pub fn base(assets: &Path) -> Self {
        Self {
            id: BASE_ID.to_owned(),
            info: PackInfo {
                description: "The default look of Rustcraft".to_owned(),
                formats: None,
            },
            icon: None,
            files: Files::Dir(assets.to_owned()),
            root: "",
        }
    }

    /// The name shown for the pack: its file name without `.zip`.
    pub fn name(&self) -> &str {
        match self.id.as_str() {
            BASE_ID => "Default",
            id => id.strip_suffix(".zip").unwrap_or(id),
        }
    }

    /// A file in the `minecraft` namespace (`textures/block/stone.png`).
    pub fn read(&self, path: &str) -> Option<Vec<u8>> {
        self.read_file(&format!("{}{path}", self.root))
    }

    pub fn has(&self, path: &str) -> bool {
        let full = format!("{}{path}", self.root);
        match &self.files {
            Files::Dir(dir) => dir.join(&full).is_file(),
            Files::Zip { names, .. } => names.contains(&full),
        }
    }

    /// A file relative to the pack's root.
    fn read_file(&self, path: &str) -> Option<Vec<u8>> {
        match &self.files {
            Files::Dir(dir) => std::fs::read(dir.join(path)).ok(),
            Files::Zip {
                archive,
                names,
                prefix,
            } => {
                if !names.contains(path) {
                    return None;
                }
                let mut archive = archive.lock().unwrap_or_else(|e| e.into_inner());
                let mut entry = archive.by_name(&format!("{prefix}{path}")).ok()?;
                let mut bytes = Vec::with_capacity(entry.size() as usize);
                match entry.read_to_end(&mut bytes) {
                    Ok(_) => Some(bytes),
                    Err(err) => {
                        warn!("reading {path} from {}: {err}", self.id);
                        None
                    }
                }
            }
        }
    }
}

fn open_zip(path: &Path) -> Result<Files> {
    let file = File::open(path).with_context(|| format!("opening {}", path.display()))?;
    let archive = ZipArchive::new(BufReader::new(file))
        .with_context(|| format!("reading {}", path.display()))?;
    let all: Vec<String> = archive
        .file_names()
        .filter_map(|name| name.ok().map(|n| n.into_owned()))
        .collect();
    // Some zips wrap the pack in one folder.
    let prefix = if all.iter().any(|n| n == "pack.mcmeta") {
        String::new()
    } else {
        all.iter()
            .filter_map(|n| n.strip_suffix("pack.mcmeta"))
            .filter(|p| p.ends_with('/') && p[..p.len() - 1].find('/').is_none())
            .min_by_key(|p| p.len())
            .unwrap_or_default()
            .to_owned()
    };
    let names = all
        .iter()
        .filter_map(|n| n.strip_prefix(&prefix))
        .map(str::to_owned)
        .collect();
    Ok(Files::Zip {
        archive: Mutex::new(archive),
        names,
        prefix,
    })
}

/// The resource packs folder, beside the saves folder.
pub fn packs_dir() -> PathBuf {
    crate::level::saves_dir().with_file_name("resourcepacks")
}

/// Every pack in `dir`, sorted by name; ones that can't be read are
/// skipped with a warning.
pub fn list(dir: &Path) -> Vec<Pack> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut packs: Vec<Pack> = entries
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|path| {
            path.is_dir()
                || path
                    .extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("zip"))
        })
        .filter_map(|path| match Pack::open(&path) {
            Ok(pack) => Some(pack),
            Err(err) => {
                warn!("skipping resource pack: {err:#}");
                None
            }
        })
        .collect();
    packs.sort_by_key(|pack| pack.name().to_lowercase());
    packs
}

#[cfg(test)]
pub(crate) mod tests {
    use std::io::Write;

    use super::*;
    use crate::world::save::TempDir;

    /// Writes a zip holding `files` (path, contents).
    pub fn write_zip(path: &Path, files: &[(&str, &[u8])]) {
        let mut zip = zip::ZipWriter::new(File::create(path).unwrap());
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        for (name, bytes) in files {
            zip.start_file(*name, options).unwrap();
            zip.write_all(bytes).unwrap();
        }
        zip.finish().unwrap();
    }

    pub const MCMETA: &[u8] = br#"{"pack": {"pack_format": 34, "description": "test"}}"#;

    #[test]
    fn reads_old_and_new_pack_mcmeta() {
        let old = PackInfo::parse(MCMETA_OLD).unwrap();
        assert_eq!(old.formats, Some((9, 9)));
        assert_eq!(old.description, "A 16px pack");
        assert_eq!(old.compatibility(), Compatibility::Older);
        let new = PackInfo::parse(
            r#"{"pack": {"description": "§6Beta\n1.21", "min_format": 69, "max_format": [75, 0]}}"#,
        )
        .unwrap();
        assert_eq!(new.formats, Some((69, 75)));
        assert_eq!(new.description, "Beta\n1.21");
        assert_eq!(new.compatibility(), Compatibility::Ok);
        let ranged = PackInfo::parse(
            r#"{"pack": {"pack_format": 15, "description": [{"text": "a"}, "b"],
                "supported_formats": {"min_inclusive": 15, "max_inclusive": 99}}}"#,
        )
        .unwrap();
        assert_eq!(ranged.formats, Some((15, 99)));
        assert_eq!(ranged.description, "ab");
        assert!(PackInfo::parse("{}").is_err());
    }

    #[test]
    fn json_is_read_leniently() {
        assert_eq!(parse_json("{\"a\": 1}\n}").unwrap()["a"], 1);
        assert_eq!(parse_json("\u{feff}[2]").unwrap()[0], 2);
        assert!(parse_json("").is_err());
        assert!(parse_json("{").is_err());
    }

    const MCMETA_OLD: &str = r#"{"pack": {"pack_format": 9, "description": "A 16px pack"}}"#;

    #[test]
    fn opens_folders_and_zips_even_wrapped_in_a_folder() {
        let dir = TempDir::new("packs");
        let folder = dir.path().join("Folder Pack");
        std::fs::create_dir_all(folder.join("assets/minecraft/textures/block")).unwrap();
        std::fs::write(folder.join("pack.mcmeta"), MCMETA).unwrap();
        std::fs::write(
            folder.join("assets/minecraft/textures/block/stone.png"),
            b"png",
        )
        .unwrap();
        write_zip(
            &dir.path().join("Zipped.zip"),
            &[
                ("pack.mcmeta", MCMETA),
                ("pack.png", b"icon"),
                ("assets/minecraft/textures/item/stick.png", b"stick"),
            ],
        );
        write_zip(
            &dir.path().join("wrapped.zip"),
            &[
                ("Wrapped/pack.mcmeta", MCMETA),
                ("Wrapped/assets/minecraft/textures/item/apple.png", b"apple"),
            ],
        );
        write_zip(&dir.path().join("broken.zip"), &[("readme.txt", b"hi")]);
        std::fs::write(dir.path().join("notes.txt"), b"not a pack").unwrap();

        let packs = list(dir.path());
        let names: Vec<&str> = packs.iter().map(Pack::name).collect();
        assert_eq!(names, ["Folder Pack", "wrapped", "Zipped"]);
        assert_eq!(packs[0].read("textures/block/stone.png").unwrap(), b"png");
        assert!(packs[0].has("textures/block/stone.png"));
        assert!(!packs[0].has("textures/block/dirt.png"));
        assert_eq!(packs[1].read("textures/item/apple.png").unwrap(), b"apple");
        assert_eq!(packs[2].id, "Zipped.zip");
        assert_eq!(packs[2].read("textures/item/stick.png").unwrap(), b"stick");
        assert_eq!(packs[2].icon.as_deref(), Some(&b"icon"[..]));
        assert!(packs[2].read("textures/item/apple.png").is_none());
    }

    #[test]
    fn kenzos_packs_open() {
        let assets = crate::assets::dir();
        for name in ["Ashen_1.8.3.zip", "Prime's HD Textures (FREE)"] {
            let pack = Pack::open(&assets.join(name)).unwrap();
            assert!(pack.has("textures/block/stone.png"), "{name}");
            assert!(pack.read("textures/block/stone.png").is_some(), "{name}");
        }
        let base = Pack::base(&assets);
        assert!(base.has("textures/blocks/stone.png"));
    }
}
