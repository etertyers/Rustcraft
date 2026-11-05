//! Sprites read from the mod .jars in `downloads/mods/` at runtime (never
//! copied into the repo): the extras' own particle pictures, by the name
//! `fx/<mod>/<name>`. A missing jar leaves its sprites out, and the effect
//! falls back to vanilla's particles with one log line.

use std::{
    collections::HashMap,
    io::Read,
    path::{Path, PathBuf},
    sync::RwLock,
};

use image::RgbaImage;
use log::info;

/// (jar file prefix, the mod's name in `fx/<mod>/…`, namespace in the jar,
/// pictures to read, each as a name and how many numbered frames it has,
/// 0 for a plain file).
/// A jar's sprites: each a name and its numbered frame count.
type Sprites = &'static [(&'static str, u8)];

const JARS: &[(&str, &str, &str, Sprites)] = &[
    (
        "visuality-",
        "visuality",
        "visuality",
        &[
            ("sparkle", 6),
            ("water_circle", 5),
            ("feather", 0),
            ("cold_feather", 0),
            ("warm_feather", 0),
            ("bone", 0),
            ("wither_bone", 0),
            ("emerald", 0),
            ("soul", 11),
            ("small_slime_blob", 4),
            ("medium_slime_blob", 4),
            ("big_slime_blob", 4),
            ("charge", 4),
        ],
    ),
    ("wakes-", "wakes", "wakes", &[("splash_cloud", 0)]),
];

/// The names a sprite's frames have in the atlas: `base` alone, or
/// `base_0`…`base_{frames-1}`.
pub fn frame_names(base: &str, frames: u8) -> Vec<String> {
    if frames == 0 {
        vec![base.to_owned()]
    } else {
        (0..frames).map(|i| format!("{base}_{i}")).collect()
    }
}

/// Reads every sprite the jars in `dir` have, keyed `fx/<mod>/<name>`.
pub fn load(dir: &Path) -> HashMap<String, RgbaImage> {
    let mut images = HashMap::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        info!(
            "extras: no mods folder at {}; the extras use vanilla's particles",
            dir.display()
        );
        return images;
    };
    let files: Vec<_> = entries.flatten().map(|e| e.path()).collect();
    for &(prefix, name, namespace, sprites) in JARS {
        let jar = files.iter().find(|path| {
            path.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with(prefix) && n.ends_with(".jar"))
        });
        let Some(jar) = jar else {
            info!("extras: {name}'s jar is missing; it falls back to vanilla's particles");
            continue;
        };
        let Ok(file) = std::fs::File::open(jar) else {
            continue;
        };
        let Ok(mut zip) = zip::ZipArchive::new(file) else {
            continue;
        };
        for &(sprite, frames) in sprites {
            // Wakes' clouds come as four files, numbered from 1, each an
            // animation strip: the first frame is the top square.
            let numbered: Vec<String> = if name == "wakes" {
                (1..=4).map(|i| format!("{sprite}_{i}")).collect()
            } else {
                frame_names(sprite, frames)
            };
            for file_name in numbered {
                let path = format!("assets/{namespace}/textures/particle/{file_name}.png");
                let mut bytes = Vec::new();
                match zip.by_name(&path) {
                    Ok(mut entry) => {
                        if entry.read_to_end(&mut bytes).is_err() {
                            continue;
                        }
                    }
                    Err(_) => continue,
                }
                let Ok(image) = image::load_from_memory(&bytes) else {
                    continue;
                };
                let mut image = image.into_rgba8();
                if image.height() > image.width() {
                    image = image::imageops::crop_imm(&image, 0, 0, image.width(), image.width())
                        .to_image();
                }
                images.insert(format!("fx/{name}/{file_name}"), image);
            }
        }
    }
    images
}

static DIR: RwLock<Option<PathBuf>> = RwLock::new(None);

/// Where the mod jars are (`downloads/mods`), set when the resources load.
pub fn set_dir(dir: &Path) {
    if let Ok(mut d) = DIR.write() {
        *d = Some(dir.to_owned());
    }
}

/// The jar in the mods folder whose file name starts with `prefix`.
pub fn find_jar(prefix: &str) -> Option<PathBuf> {
    let dir = DIR.read().ok()?.clone()?;
    find_jar_in(&dir, prefix)
}

pub fn find_jar_in(dir: &Path, prefix: &str) -> Option<PathBuf> {
    std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .find(|path| {
            path.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with(prefix) && n.ends_with(".jar"))
        })
}

/// A file inside the jar `jar`.
pub fn read_in(jar: &Path, path: &str) -> Option<Vec<u8>> {
    let mut zip = zip::ZipArchive::new(std::fs::File::open(jar).ok()?).ok()?;
    let mut entry = zip.by_name(path).ok()?;
    let mut bytes = Vec::new();
    entry.read_to_end(&mut bytes).ok()?;
    Some(bytes)
}

/// Which jar has the sounds of a namespace.
fn sound_jar(namespace: &str) -> Option<&'static str> {
    match namespace {
        "presencefootsteps" | "presencefootstepsmono" => Some("PresenceFootsteps-"),
        "ambientsounds" => Some("AmbientSounds_"),
        _ => None,
    }
}

/// The bytes of a sound `namespace:path` from its jar (`sounds/<path>.ogg`).
pub fn sound_bytes(name: &str) -> Option<Vec<u8>> {
    let (namespace, path) = name.split_once(':')?;
    let jar = find_jar(sound_jar(namespace)?)?;
    read_in(&jar, &format!("assets/{namespace}/sounds/{path}.ogg"))
}

/// The jars' own `sounds.json` files, to merge into the sound table: their
/// events name the files above.
pub fn sounds_json_layers() -> Vec<String> {
    ["presencefootsteps", "ambientsounds"]
        .into_iter()
        .filter_map(|namespace| {
            let jar = find_jar(sound_jar(namespace)?)?;
            let bytes = read_in(&jar, &format!("assets/{namespace}/sounds.json"))?;
            String::from_utf8(bytes).ok()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_are_named_by_number_or_not_at_all() {
        assert_eq!(frame_names("bone", 0), ["bone"]);
        assert_eq!(
            frame_names("sparkle", 3),
            ["sparkle_0", "sparkle_1", "sparkle_2"]
        );
    }

    #[test]
    fn the_real_jars_give_their_sprites_when_downloaded() {
        let dir = crate::assets::dir().join("downloads/mods");
        if !dir.is_dir() {
            eprintln!("skipped: no downloads/mods");
            return;
        }
        let images = load(&dir);
        for name in [
            "fx/visuality/sparkle_0",
            "fx/visuality/water_circle_4",
            "fx/visuality/bone",
            "fx/wakes/splash_cloud_1",
        ] {
            let image = images.get(name).unwrap_or_else(|| panic!("{name} missing"));
            assert_eq!(image.width(), image.height(), "{name} is a square frame");
        }
    }

    #[test]
    fn the_sound_jars_give_their_events_and_ogg_files_when_downloaded() {
        let dir = crate::assets::dir().join("downloads/mods");
        if find_jar_in(&dir, "PresenceFootsteps-").is_none()
            || find_jar_in(&dir, "AmbientSounds_").is_none()
        {
            eprintln!("skipped: no sound jars");
            return;
        }
        // Read from `dir` directly: the global folder is set by other
        // tests running at the same time.
        let in_jar = |namespace: &str, path: &str| {
            let jar = find_jar_in(&dir, sound_jar(namespace)?)?;
            read_in(&jar, path)
        };
        let layers: Vec<(&str, String)> = ["presencefootsteps", "ambientsounds"]
            .into_iter()
            .filter_map(|namespace| {
                let bytes = in_jar(namespace, &format!("assets/{namespace}/sounds.json"))?;
                Some((namespace, String::from_utf8(bytes).ok()?))
            })
            .collect();
        assert_eq!(layers.len(), 2, "both jars have a sounds.json");
        // Every file the first events name is in a jar and is an ogg.
        let mut checked = 0;
        for (_, layer) in &layers {
            let json: serde_json::Value = serde_json::from_str(layer).unwrap();
            for (_, event) in json.as_object().unwrap().iter().take(40) {
                for sound in event["sounds"].as_array().into_iter().flatten() {
                    let name = sound.as_str().or_else(|| sound["name"].as_str()).unwrap();
                    let (namespace, path) = name.split_once(':').unwrap();
                    let bytes = in_jar(namespace, &format!("assets/{namespace}/sounds/{path}.ogg"))
                        .unwrap_or_else(|| panic!("{name} missing"));
                    assert_eq!(&bytes[..4], b"OggS", "{name}");
                    checked += 1;
                }
            }
        }
        assert!(checked > 20, "{checked}");
    }
}
