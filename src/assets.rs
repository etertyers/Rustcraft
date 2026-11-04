//! Locating the `assets/` folder, and where the game keeps its files.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// Where the read-only asset root was found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RootSource {
    /// `--assets <dir>`.
    Flag,
    /// `$RUSTCRAFT_ASSETS`.
    Env,
    /// The app's `Contents/Resources/assets`, found from the executable.
    Bundle,
    /// The `assets/` folder of the source tree (for `cargo run`).
    Repo,
}

impl RootSource {
    pub fn describe(self) -> &'static str {
        match self {
            Self::Flag => "--assets",
            Self::Env => "$RUSTCRAFT_ASSETS",
            Self::Bundle => "the app bundle's Contents/Resources",
            Self::Repo => "the source tree",
        }
    }
}

/// The read-only asset root, in this order: `--assets <dir>`, then
/// `$RUSTCRAFT_ASSETS` (the same override, for scripts), then the app
/// bundle's `Contents/Resources/assets` (from `exe`'s location), then the
/// source tree's `assets/` under `manifest`. The working folder is never
/// looked at, so the game finds the same assets whether it is opened from
/// the Finder, a terminal or `cargo run`.
pub fn resolve(
    flag: Option<PathBuf>,
    env: Option<PathBuf>,
    exe: &Path,
    manifest: &Path,
) -> (PathBuf, RootSource) {
    if let Some(dir) = flag {
        return (dir, RootSource::Flag);
    }
    if let Some(dir) = env {
        return (dir, RootSource::Env);
    }
    if let Some(resources) = resources_of(exe) {
        return (resources.join("assets"), RootSource::Bundle);
    }
    (manifest.join("assets"), RootSource::Repo)
}

/// The value after `--assets` in `args`, if given.
fn assets_flag(args: &[String]) -> Option<PathBuf> {
    let at = args.iter().position(|arg| arg == "--assets")?;
    args.get(at + 1).map(PathBuf::from)
}

static ROOT: OnceLock<(PathBuf, RootSource)> = OnceLock::new();

fn root() -> &'static (PathBuf, RootSource) {
    ROOT.get_or_init(|| {
        let args: Vec<String> = std::env::args().collect();
        let exe = std::env::current_exe().unwrap_or_default();
        let found = resolve(
            assets_flag(&args),
            std::env::var_os("RUSTCRAFT_ASSETS").map(PathBuf::from),
            &exe,
            Path::new(env!("CARGO_MANIFEST_DIR")),
        );
        log::info!(
            "assets: {} (from {}); {}",
            found.0.display(),
            found.1.describe(),
            layers(&found.0)
        );
        found
    })
}

/// The assets folder, resolved once.
pub fn dir() -> PathBuf {
    root().0.clone()
}

/// Where [`dir`] came from.
pub fn source() -> RootSource {
    root().1
}

/// The entries of `dir` with the given extension (`""` for any), or 0.
fn count_in(dir: &Path, extension: &str) -> usize {
    std::fs::read_dir(dir).map_or(0, |entries| {
        entries
            .filter_map(Result::ok)
            .filter(|e| {
                extension.is_empty()
                    || e.path()
                        .extension()
                        .is_some_and(|x| x.eq_ignore_ascii_case(extension))
            })
            .count()
    })
}

/// One line saying which asset layers `assets` has.
pub fn layers(assets: &Path) -> String {
    let yes = |present: bool| if present { "found" } else { "missing" };
    format!(
        "layers: Rustcraft textures {}, vanilla {}, downloads: {} packs, {} mod jars",
        yes(assets.join("textures").is_dir()),
        yes(assets.join("vanilla/assets/minecraft").is_dir()),
        count_in(&assets.join("downloads/resourcepacks"), "")
            .saturating_sub(count_hidden(&assets.join("downloads/resourcepacks"))),
        count_in(&assets.join("downloads/mods"), "jar")
    )
}

fn count_hidden(dir: &Path) -> usize {
    std::fs::read_dir(dir).map_or(0, |entries| {
        entries
            .filter_map(Result::ok)
            .filter(|e| e.file_name().to_string_lossy().starts_with('.'))
            .count()
    })
}

/// The `Contents/Resources` folder of the macOS app bundle the game runs
/// from, if it runs from one.
fn bundle_resources() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    resources_of(&exe)
}

/// `Rustcraft.app/Contents/Resources` for an executable in
/// `Rustcraft.app/Contents/MacOS`.
fn resources_of(exe: &Path) -> Option<PathBuf> {
    let macos = exe.parent()?;
    let contents = macos.parent()?;
    let is_bundle = macos.file_name()? == "MacOS"
        && contents.file_name()? == "Contents"
        && contents.parent()?.extension()? == "app";
    is_bundle.then(|| contents.join("Resources"))
}

/// Whether the game runs from an app bundle (opened from the Finder),
/// where the working folder is `/` and can't hold saves.
pub fn in_app_bundle() -> bool {
    bundle_resources().is_some()
}

/// `~/Library/Application Support/Rustcraft`, for saves and options when
/// running from the app bundle.
pub fn support_dir() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(Path::new(&home).join("Library/Application Support/Rustcraft"))
}

/// What `--asset-report` found.
pub struct AssetReport {
    /// The report, one fact per line.
    pub text: String,
    /// Things that make a build wrong: no vanilla layer, a texture no
    /// layer has, a silent sound event, no font.
    pub problems: Vec<String>,
}

/// Where each file of vanilla's textures is, by file name, for telling a
/// lookup bug (the texture is under another path) from a Rustcraft-only
/// name (vanilla has no such file).
fn vanilla_texture_index(assets: &Path) -> std::collections::HashMap<String, Vec<String>> {
    fn walk(dir: &Path, root: &Path, index: &mut std::collections::HashMap<String, Vec<String>>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for path in entries.filter_map(|e| e.ok().map(|e| e.path())) {
            if path.is_dir() {
                walk(&path, root, index);
            } else if path.extension().is_some_and(|e| e == "png")
                && let (Some(stem), Ok(rel)) = (path.file_stem(), path.strip_prefix(root))
            {
                index
                    .entry(stem.to_string_lossy().into_owned())
                    .or_default()
                    .push(rel.with_extension("").to_string_lossy().into_owned());
            }
        }
    }
    let root = assets.join("vanilla/assets/minecraft/textures");
    let mut index = std::collections::HashMap::new();
    walk(&root, &root, &mut index);
    index
}

/// What the asset layers cover, in lines for `--asset-report`: the root,
/// the layers, textures requested and where each came from, the missing
/// ones (a lookup bug, or Rustcraft-only), sounds and the font.
pub fn report(assets: &Path) -> anyhow::Result<AssetReport> {
    use crate::{
        entity::mobs::MobRegistry, inventory::item, look::mob_texture, pack::Resources,
        world::block,
    };
    use std::{cell::RefCell, collections::HashMap};
    let res = Resources::load(assets, Vec::new())?;
    let mut out = String::new();
    let mut problems = Vec::new();
    out += &format!(
        "asset root: {} (from {})\n{}\n",
        assets.display(),
        source().describe(),
        layers(assets)
    );
    if !res.has_vanilla() {
        problems.push("the vanilla layer is missing".to_owned());
    }
    out += &format!(
        "vanilla layer: {}\n",
        if res.has_vanilla() {
            "present"
        } else {
            "missing"
        }
    );
    let blocks = block::registry();
    let items = item::registry();
    let placeholders = blocks.iter().filter(|d| d.placeholder).count();
    let placeholder_items = items.iter().filter(|d| d.placeholder).count();
    out += &format!(
        "blocks: {} ({placeholders} placeholders), items: {} ({placeholder_items} placeholders, incl. block items)\n",
        blocks.iter().count(),
        items.len()
    );
    // Every requested texture once, and the layer that gave it.
    let seen: RefCell<HashMap<String, Option<(usize, bool)>>> = RefCell::new(HashMap::new());
    let has = |name: &str| {
        seen.borrow_mut()
            .entry(name.to_owned())
            .or_insert_with(|| res.texture(name).map(|t| (t.layer, t.vanilla)))
            .is_some()
    };
    let missing_blocks = blocks.missing_textures_in(has, true);
    let missing_items = items.missing_textures_in(has, true);
    let mobs = MobRegistry::load(&assets.join("data/mobs.ron"))?;
    let mut missing_mobs = Vec::new();
    let mut wrong_size = Vec::new();
    for (name, size) in mobs.textures_sized() {
        if mob_texture(&res, name, Some(size)).is_some() {
            continue;
        }
        if res.texture(name).is_some() {
            wrong_size.push(name);
        } else {
            missing_mobs.push(name);
        }
    }
    let (mut base, mut packs, mut vanilla) = (0, 0, 0);
    for found in seen.borrow().values().flatten() {
        match *found {
            (_, true) => vanilla += 1,
            (0, false) => base += 1,
            _ => packs += 1,
        }
    }
    let requested = seen.borrow().len();
    let missing_names: Vec<String> = missing_blocks
        .iter()
        .chain(&missing_items)
        .cloned()
        .chain(missing_mobs.iter().map(|n| n.to_string()))
        .collect();
    out += &format!(
        "textures requested (blocks and items): {requested}; found in Rustcraft's own: {base}, in packs: {packs}, in vanilla: {vanilla}; missing (incl. mobs): {}\n",
        missing_names.len()
    );
    if !missing_names.is_empty() {
        let index = vanilla_texture_index(assets);
        for name in &missing_names {
            let stem = name.rsplit('/').next().unwrap_or(name);
            match index.get(stem) {
                Some(paths) => {
                    out +=
                        &format!("  missing {name}: vanilla has it as {paths:?} (a lookup bug)\n")
                }
                None => out += &format!("  missing {name}: not in vanilla (Rustcraft-only)\n"),
            }
        }
        problems.push(format!("{} textures no layer has", missing_names.len()));
    }
    out += &format!(
        "mob textures: {} missing, {} with a file of the wrong size for the model {:?}\n",
        missing_mobs.len(),
        wrong_size.len(),
        wrong_size
    );
    let sounds = crate::audio::report(&res);
    out += &format!(
        "sound events: {} in the table, {} with no file at all {:?}, {} files named but absent\n",
        sounds.events,
        sounds.silent.len(),
        sounds.silent,
        sounds.missing_files.len()
    );
    if sounds.events == 0 || !sounds.silent.is_empty() {
        problems.push(format!(
            "{} of {} sound events are silent",
            sounds.silent.len(),
            sounds.events
        ));
    }
    let providers = crate::ui::font::providers(&res);
    let from_json = res.bytes("font/default.json").is_some();
    let pages = providers
        .iter()
        .filter(|p| res.image(&p.file).is_some())
        .count();
    out += &format!(
        "font: default.json {}, {} providers, {pages} glyph pages found, {} glyphs\n",
        if from_json { "found" } else { "missing" },
        providers.len(),
        crate::ui::font::load(&res).len()
    );
    if !from_json || pages == 0 {
        problems.push("the Minecraft font is missing (egui's own font will be used)".to_owned());
    }
    Ok(AssetReport {
        text: out,
        problems,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_resources_of_an_app_bundle() {
        assert_eq!(
            resources_of(Path::new(
                "/Applications/Rustcraft.app/Contents/MacOS/rustcraft"
            )),
            Some(PathBuf::from(
                "/Applications/Rustcraft.app/Contents/Resources"
            ))
        );
        assert_eq!(
            resources_of(Path::new("/code/rustcraft/target/release/rustcraft")),
            None
        );
    }

    #[test]
    fn the_root_is_the_bundles_whatever_the_current_folder_is() {
        let tmp = crate::world::save::TempDir::new("app-root");
        let exe = tmp.path().join("Rustcraft.app/Contents/MacOS/rustcraft");
        // A folder named `assets` in the current folder must not matter:
        // resolve() never looks at it.
        std::fs::create_dir_all(tmp.path().join("assets")).unwrap();
        let (root, source) = resolve(None, None, &exe, Path::new("/src/rustcraft"));
        assert_eq!(
            root,
            tmp.path().join("Rustcraft.app/Contents/Resources/assets")
        );
        assert_eq!(source, RootSource::Bundle);
    }

    #[test]
    fn the_flag_wins_then_the_bundle_then_the_source_tree() {
        let exe = Path::new("/Applications/Rustcraft.app/Contents/MacOS/rustcraft");
        let dev = Path::new("/code/rustcraft/target/release/rustcraft");
        let manifest = Path::new("/code/rustcraft");
        let flag = Some(PathBuf::from("/elsewhere"));
        assert_eq!(
            resolve(flag.clone(), None, exe, manifest),
            (PathBuf::from("/elsewhere"), RootSource::Flag)
        );
        assert_eq!(
            resolve(None, flag, dev, manifest),
            (PathBuf::from("/elsewhere"), RootSource::Env)
        );
        assert_eq!(resolve(None, None, dev, manifest).1, RootSource::Repo);
        assert_eq!(
            resolve(None, None, dev, manifest).0,
            PathBuf::from("/code/rustcraft/assets")
        );
        assert_eq!(resolve(None, None, exe, manifest).1, RootSource::Bundle);
    }

    #[test]
    fn reads_the_assets_flag() {
        let args = |words: &[&str]| words.iter().map(|w| w.to_string()).collect::<Vec<_>>();
        assert_eq!(
            assets_flag(&args(&["rustcraft", "--assets", "/a", "--exit"])),
            Some(PathBuf::from("/a"))
        );
        assert_eq!(assets_flag(&args(&["rustcraft", "--exit"])), None);
    }

    /// With the vanilla layer present, nothing is silent and no texture is
    /// missing (the report would list Rustcraft-only names as problems).
    #[test]
    fn the_report_finds_every_sound_texture_and_the_font() {
        let assets = dir();
        if !assets.join("vanilla/assets/minecraft").is_dir() {
            eprintln!("skipping: no assets/vanilla (run scripts/fetch-vanilla-assets.sh)");
            return;
        }
        let report = report(&assets).unwrap();
        assert!(report.problems.is_empty(), "{}", report.text);
        assert!(report.text.contains("0 with no file at all"));
    }
}
