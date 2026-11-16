//! Which folders hold `vanilla/` and `downloads/`, and whether a copy is
//! complete. The asset root keeps its order (`--assets`,
//! `$RUSTCRAFT_ASSETS`, the app's Resources, the source tree). A clone of
//! the repository (root = the source tree, or a folder given by flag or
//! environment) counts only its own `assets/vanilla` and `assets/downloads`
//! and fills them (M74, per clone). The .app's Resources are read-only, so
//! only a bundle root falls back to the user layer
//! (`~/Library/Application Support/Rustcraft/assets`) and downloads into it.

use super::manifest::{self, Extra};
use std::path::{Path, PathBuf};
use std::sync::RwLock;

/// Written beside `vanilla/` and `downloads/` in the user layer.
pub const STAMP_FILE: &str = "installed.ron";

/// `~/Library/Application Support/Rustcraft/assets`.
pub fn user_assets() -> Option<PathBuf> {
    crate::assets::support_dir().map(|dir| dir.join("assets"))
}

/// Written in `downloads/` when the player chose "Only Minecraft's files":
/// this clone doesn't ask for the packs again. Git-ignored with the folder.
pub const SKIP_PACKS_FILE: &str = ".skip-packs";

/// Where the last run from a clone recorded its assets folder, in the user
/// layer's parent (`…/Rustcraft/last-assets.txt`).
pub const LAST_ASSETS_FILE: &str = "last-assets.txt";

/// Whether this root reads and fills the shared user layer: only the app
/// bundle, whose Resources can't be written.
pub fn shares_user_layer(source: crate::assets::RootSource) -> bool {
    source == crate::assets::RootSource::Bundle
}

/// The folder a download or copy fills for `root`: the root itself for a
/// clone (its own `vanilla/` and `downloads/`), the user layer for the
/// bundle. `None`: a bundle with no home folder.
pub fn target_for(root: &Path, source: crate::assets::RootSource) -> Option<PathBuf> {
    if shares_user_layer(source) {
        user_assets()
    } else {
        Some(root.to_owned())
    }
}

/// [`target_for`] the game's own root.
pub fn target() -> Option<PathBuf> {
    target_for(&crate::assets::dir(), crate::assets::source())
}

/// Whether the player chose to go without the packs, under `target`.
pub fn skip_packs(target: &Path) -> bool {
    target.join("downloads").join(SKIP_PACKS_FILE).exists()
}

pub fn set_skip_packs(target: &Path, skip: bool) {
    let marker = target.join("downloads").join(SKIP_PACKS_FILE);
    if skip {
        let _ = std::fs::create_dir_all(target.join("downloads"));
        let _ = std::fs::write(
            &marker,
            "Only Minecraft's files; remove this file to be asked again.\n",
        );
    } else {
        let _ = std::fs::remove_file(&marker);
    }
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
    resolve_with(root, user, true)
}

/// The choice for a root of this kind: a clone counts only its own folders.
pub fn resolve_for(root: &Path, user: Option<&Path>, source: crate::assets::RootSource) -> Layers {
    resolve_with(root, user, shares_user_layer(source))
}

fn resolve_with(root: &Path, user: Option<&Path>, shared: bool) -> Layers {
    let root_vanilla = root.join("vanilla");
    let root_downloads = root.join("downloads");
    let (mut vanilla, mut in_user_vanilla) = (root_vanilla.clone(), false);
    let (mut downloads, mut in_user_downloads) = (root_downloads.clone(), false);
    if let Some(user) = user.filter(|u| shared && *u != root) {
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
    let layers = resolve_for(root, user_assets().as_deref(), crate::assets::source());
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

// --- A complete copy elsewhere on this Mac ----------------------------------

/// A folder (an `assets/` folder) that has what this clone is missing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CopySource {
    pub assets: PathBuf,
    /// Copies Minecraft's files.
    pub vanilla: bool,
    /// The packs and jars it copies.
    pub extras: Vec<manifest::Extra>,
    pub bytes: u64,
}

/// Remembers `root` (a clone with complete files) in the user layer's
/// parent, so a fresh clone can offer to copy from it.
pub fn remember_clone(root: &Path, source: crate::assets::RootSource) {
    if shares_user_layer(source) || !vanilla_complete(&root.join("vanilla")) {
        return;
    }
    let Some(dir) = crate::assets::support_dir() else {
        return;
    };
    let text = root.display().to_string();
    if std::fs::read_to_string(dir.join(LAST_ASSETS_FILE)).is_ok_and(|t| t.trim() == text) {
        return;
    }
    let _ = std::fs::create_dir_all(&dir);
    let _ = std::fs::write(dir.join(LAST_ASSETS_FILE), text);
}

/// The first of the user layer and the last clone that has everything the
/// clone at `root` is missing: Minecraft's files when `need_vanilla`, and
/// each of `need_extras` (only counts a copy that has them all). `None`
/// when no such folder exists.
pub fn find_copy_source(
    root: &Path,
    need_vanilla: bool,
    need_extras: &[manifest::Extra],
    candidates: &[PathBuf],
) -> Option<CopySource> {
    candidates.iter().find_map(|assets| {
        if assets == root || !assets.is_dir() {
            return None;
        }
        if need_vanilla && !vanilla_complete(&assets.join("vanilla")) {
            return None;
        }
        let downloads = assets.join("downloads");
        if need_extras
            .iter()
            .any(|e| presence(&downloads, e) != Presence::Present)
        {
            return None;
        }
        if !need_vanilla && need_extras.is_empty() {
            return None;
        }
        let bytes = if need_vanilla {
            super::download::dir_size(&assets.join("vanilla/assets"))
        } else {
            0
        } + need_extras.iter().map(|e| e.size).sum::<u64>();
        Some(CopySource {
            assets: assets.clone(),
            vanilla: need_vanilla,
            extras: need_extras.to_vec(),
            bytes,
        })
    })
}

/// The folders worth looking in, best first: the user layer, then the clone
/// the last run used.
pub fn copy_candidates() -> Vec<PathBuf> {
    let mut list = Vec::new();
    list.extend(user_assets());
    if let Some(dir) = crate::assets::support_dir()
        && let Ok(text) = std::fs::read_to_string(dir.join(LAST_ASSETS_FILE))
    {
        let path = PathBuf::from(text.trim());
        if !path.as_os_str().is_empty() && !list.contains(&path) {
            list.push(path);
        }
    }
    list
}

/// What this start is missing, for the first-launch screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Needs {
    /// Minecraft's files are missing or incomplete.
    pub vanilla: bool,
    /// The pinned packs and jars not in `downloads/` (none when the player
    /// chose "Only Minecraft's files" for this clone).
    pub extras: Vec<manifest::Extra>,
    /// The folder a download or copy fills (see [`target_for`]).
    pub target: PathBuf,
}

/// What `root` (of this `source`) lacks, counting only its own folders when
/// it is a clone. `None`: nothing, so no screen.
pub fn needs(root: &Path, source: crate::assets::RootSource, user: Option<&Path>) -> Option<Needs> {
    let target = target_for(root, source)?;
    let layers = resolve_for(root, user, source);
    let vanilla = !vanilla_complete(&layers.vanilla);
    let extras: Vec<manifest::Extra> = if skip_packs(&target) {
        Vec::new()
    } else {
        manifest::extras()
            .iter()
            .filter(|e| presence(&layers.downloads, e) != Presence::Present)
            .cloned()
            .collect()
    };
    (vanilla || !extras.is_empty()).then_some(Needs {
        vanilla,
        extras,
        target,
    })
}
