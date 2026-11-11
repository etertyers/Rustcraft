//! Which folders hold `vanilla/` and `downloads/`, and whether a copy is
//! complete. The read-only asset root keeps its order (`--assets`,
//! `$RUSTCRAFT_ASSETS`, the app's Resources, the source tree); for these
//! two folders only, the root's copy wins when complete, otherwise the
//! user layer (`~/Library/Application Support/Rustcraft/assets`) does.

use super::manifest::{self, Extra};
use std::path::{Path, PathBuf};
use std::sync::RwLock;

/// Written beside `vanilla/` and `downloads/` in the user layer.
pub const STAMP_FILE: &str = "installed.ron";

/// `~/Library/Application Support/Rustcraft/assets`.
pub fn user_assets() -> Option<PathBuf> {
    crate::assets::support_dir().map(|dir| dir.join("assets"))
}

/// What a complete vanilla layer holds (the numbers `check-vanilla.sh` prints).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VanillaCounts {
    pub textures: usize,
    pub models: usize,
    pub blockstates: usize,
    pub items: usize,
    pub sounds: usize,
    pub font_pages: usize,
}

fn count_files(dir: &Path) -> usize {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    entries
        .filter_map(Result::ok)
        .map(|e| {
            let path = e.path();
            if path.is_dir() { count_files(&path) } else { 1 }
        })
        .sum()
}

/// The sound files `sounds.json` names, as paths under `sounds/` without
/// the extension.
pub fn sound_names(sounds_json: &[u8]) -> Vec<String> {
    let Ok(serde_json::Value::Object(events)) = serde_json::from_slice(sounds_json) else {
        return Vec::new();
    };
    let mut names = std::collections::BTreeSet::new();
    for event in events.values() {
        let Some(list) = event.get("sounds").and_then(|s| s.as_array()) else {
            continue;
        };
        for entry in list {
            let (name, kind) = match entry {
                serde_json::Value::String(s) => (s.as_str(), "sound"),
                other => (
                    other.get("name").and_then(|n| n.as_str()).unwrap_or(""),
                    other
                        .get("type")
                        .and_then(|n| n.as_str())
                        .unwrap_or("sound"),
                ),
            };
            if kind == "event" || (name.contains(':') && !name.starts_with("minecraft:")) {
                continue;
            }
            names.insert(name.trim_start_matches("minecraft:").to_owned());
        }
    }
    names.into_iter().collect()
}

fn collect_font_pages(
    mc: &Path,
    file: &str,
    depth: u32,
    pages: &mut Vec<String>,
    problems: &mut Vec<String>,
) {
    let Ok(text) = std::fs::read(mc.join(file)) else {
        problems.push(format!("missing {file}"));
        return;
    };
    let Ok(json) = serde_json::from_slice::<serde_json::Value>(&text) else {
        problems.push(format!("{file} is not JSON"));
        return;
    };
    for provider in json
        .get("providers")
        .and_then(|p| p.as_array())
        .into_iter()
        .flatten()
    {
        let id = |key: &str| {
            provider
                .get(key)
                .and_then(|v| v.as_str())
                .map(|s| s.trim_start_matches("minecraft:").to_owned())
        };
        match provider.get("type").and_then(|t| t.as_str()) {
            Some("reference") if depth < 4 => {
                if let Some(id) = id("id") {
                    collect_font_pages(mc, &format!("font/{id}.json"), depth + 1, pages, problems);
                }
            }
            Some("bitmap") => {
                if let Some(file) = id("file") {
                    pages.push(format!("textures/{file}"));
                }
            }
            _ => {}
        }
    }
}

/// `scripts/check-vanilla.sh` in Rust: whether `vanilla` (the folder that
/// holds `assets/minecraft`) has textures, models, blockstates, items,
/// `lang/en_us.json`, `sounds.json` with every sound it names, and the font
/// with every page it names. `Err` lists what is missing.
pub fn check_vanilla(vanilla: &Path) -> Result<VanillaCounts, Vec<String>> {
    let mc = vanilla.join("assets/minecraft");
    let mut problems = Vec::new();
    let mut counts = VanillaCounts {
        textures: count_files(&mc.join("textures")),
        models: count_files(&mc.join("models")),
        blockstates: count_files(&mc.join("blockstates")),
        items: count_files(&mc.join("items")),
        ..Default::default()
    };
    for (name, n) in [
        ("textures", counts.textures),
        ("models", counts.models),
        ("blockstates", counts.blockstates),
        ("items", counts.items),
    ] {
        if n == 0 {
            problems.push(format!("no {name}/ files"));
        }
    }
    if !mc.join("lang/en_us.json").is_file() {
        problems.push("missing lang/en_us.json".into());
    }
    match std::fs::read(mc.join("sounds.json")) {
        Err(_) => problems.push("missing sounds.json".into()),
        Ok(bytes) => {
            let names = sound_names(&bytes);
            let missing: Vec<&String> = names
                .iter()
                .filter(|n| !mc.join(format!("sounds/{n}.ogg")).is_file())
                .collect();
            if let Some(first) = missing.first() {
                problems.push(format!(
                    "missing {} sound files (of {} named, e.g. {first}.ogg)",
                    missing.len(),
                    names.len()
                ));
            }
            counts.sounds = names.len() - missing.len();
        }
    }
    let mut pages = Vec::new();
    collect_font_pages(&mc, "font/default.json", 0, &mut pages, &mut problems);
    let missing_pages = pages.iter().filter(|p| !mc.join(p).is_file()).count();
    if missing_pages > 0 {
        problems.push(format!("missing {missing_pages} font pages"));
    }
    counts.font_pages = pages.len() - missing_pages;
    if problems.is_empty() {
        Ok(counts)
    } else {
        Err(problems)
    }
}

/// Whether `vanilla` is a complete layer.
pub fn vanilla_complete(vanilla: &Path) -> bool {
    check_vanilla(vanilla).is_ok()
}

/// How a pinned file stands in a `downloads` folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Presence {
    /// There, with the pinned size (not hashed).
    Present,
    Missing,
    /// There, with another size.
    WrongSize,
}

pub fn presence(downloads: &Path, extra: &Extra) -> Presence {
    match std::fs::metadata(downloads.join(extra.rel_path())) {
        Ok(m) if m.is_file() && m.len() == extra.size => Presence::Present,
        Ok(m) if m.is_file() => Presence::WrongSize,
        _ => Presence::Missing,
    }
}

/// How many of the pinned files `downloads` has.
pub fn extras_present(downloads: &Path) -> usize {
    manifest::extras()
        .iter()
        .filter(|e| presence(downloads, e) == Presence::Present)
        .count()
}

/// Where each of the two folders is read from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layers {
    /// The folder holding `assets/minecraft`.
    pub vanilla: PathBuf,
    /// The folder holding `resourcepacks/` and `mods/`.
    pub downloads: PathBuf,
    pub vanilla_in_user_layer: bool,
    pub downloads_in_user_layer: bool,
}

/// The choice, from the root `assets/` folder and the user layer: the
/// root's copy if complete, else the user layer's if it is more complete,
/// else the root's (as before M74, so a missing layer is reported against
/// the root).
pub fn resolve(root: &Path, user: Option<&Path>) -> Layers {
    let root_vanilla = root.join("vanilla");
    let root_downloads = root.join("downloads");
    let (mut vanilla, mut in_user_vanilla) = (root_vanilla.clone(), false);
    let (mut downloads, mut in_user_downloads) = (root_downloads.clone(), false);
    if let Some(user) = user.filter(|u| *u != root) {
        if !vanilla_complete(&root_vanilla) && vanilla_complete(&user.join("vanilla")) {
            vanilla = user.join("vanilla");
            in_user_vanilla = true;
        }
        let total = manifest::extras().len();
        let (have_root, have_user) = (
            extras_present(&root_downloads),
            extras_present(&user.join("downloads")),
        );
        if have_root < total && have_user > have_root {
            downloads = user.join("downloads");
            in_user_downloads = true;
        }
    }
    Layers {
        vanilla,
        downloads,
        vanilla_in_user_layer: in_user_vanilla,
        downloads_in_user_layer: in_user_downloads,
    }
}

static CURRENT: RwLock<Option<(PathBuf, Layers)>> = RwLock::new(None);

/// The layers for `root`, decided once (and logged once) until
/// [`refresh`] runs after an install.
pub fn current(root: &Path) -> Layers {
    if let Ok(guard) = CURRENT.read()
        && let Some((key, layers)) = guard.as_ref()
        && key == root
    {
        return layers.clone();
    }
    let layers = resolve(root, user_assets().as_deref());
    log::info!(
        "vanilla layer: {} ({}); downloads: {} ({})",
        layers.vanilla.display(),
        if layers.vanilla_in_user_layer {
            "the user layer"
        } else {
            "the asset root"
        },
        layers.downloads.display(),
        if layers.downloads_in_user_layer {
            "the user layer"
        } else {
            "the asset root"
        },
    );
    if let Ok(mut guard) = CURRENT.write() {
        *guard = Some((root.to_owned(), layers.clone()));
    }
    layers
}

/// Forgets the decision; the next [`current`] looks again.
pub fn refresh() {
    if let Ok(mut guard) = CURRENT.write() {
        *guard = None;
    }
}

/// Whether the first-launch screen is needed: no complete vanilla layer in
/// the root or the user layer.
pub fn vanilla_missing(root: &Path) -> bool {
    !vanilla_complete(&current(root).vanilla)
}

/// [`vanilla_missing`] for explicit folders: no complete vanilla layer in
/// `root` or in `user`.
pub fn vanilla_missing_in(root: &Path, user: Option<&Path>) -> bool {
    !vanilla_complete(&resolve(root, user).vanilla)
}
