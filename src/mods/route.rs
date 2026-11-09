//! Putting files where they belong (M66): what the Mods screen says about a
//! file that isn't a Lua mod, and the moves that fix it. The same routing
//! serves the Mods screen's list, Add Files and files dropped on the window.

use std::{
    fs::File,
    io::BufReader,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};

use super::kind::{FileKind, detect};

/// The mods of lowresbones' video that Rustcraft has built in, as the words
/// a download's file name has (lower-case letters only), and where each is
/// found in the game.
const BUILT_IN: &[(&str, &str)] = &[
    ("notenoughanimations", "Not Enough Animations"),
    ("visuality", "Visuality"),
    ("cavedust", "Cave Dust"),
    ("particlerain", "Particle Rain"),
    ("makebubblespop", "Make Bubbles Pop"),
    ("wakes", "Wakes"),
    ("presencefootsteps", "Presence Footsteps"),
    ("soundphysics", "Sound Physics Remastered"),
    ("ambientsounds", "Ambient Sounds"),
];

/// Letters only, lower case: `Cave_Dust-3.1.0.jar` → `cavedustjar`.
fn words(name: &str) -> String {
    name.to_lowercase()
        .chars()
        .filter(char::is_ascii_lowercase)
        .collect()
}

/// The video mod a Java mod's file name is, if it is one.
pub fn built_in_name(file_name: &str) -> Option<&'static str> {
    let n = words(file_name);
    BUILT_IN
        .iter()
        .find(|(key, _)| n.contains(key))
        .map(|(_, title)| *title)
}

/// What can be done about a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fix {
    /// It is a Lua mod: it belongs in `mods/`.
    Install,
    /// Move it to `resourcepacks/` and turn it on.
    MoveToResourcePacks,
    /// Put it in a world's `datapacks/`.
    AddToWorld,
    /// Nothing can be done; the Finder can show it.
    ShowInFinder,
}

/// The words for a file and what the screen offers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Advice {
    pub kind: FileKind,
    /// The one line under the name: "… is a resource pack, not a mod".
    pub message: String,
    pub fix: Fix,
}

/// A file's display name: its stem, or its folder name.
pub fn display_name(path: &Path) -> String {
    path.file_stem()
        .or_else(|| path.file_name())
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// A download's file name as a title: `FreshAnimations_v1.11.2_26.1+` →
/// `Fresh Animations`. The version tail (a `_`, `-` or space followed by a
/// digit or `v` and a digit) is dropped, underscores become spaces and
/// CamelCase words are split.
pub fn pretty_name(stem: &str) -> String {
    let chars: Vec<char> = stem.chars().collect();
    let cut = (1..chars.len())
        .find(|&i| {
            matches!(chars[i - 1], '_' | '-' | ' ' | '+')
                && (chars[i].is_ascii_digit()
                    || (chars[i] == 'v' && chars.get(i + 1).is_some_and(char::is_ascii_digit)))
        })
        .unwrap_or(chars.len());
    let head: String = chars[..cut].iter().collect();
    let mut out = String::new();
    let mut last: Option<char> = None;
    for c in head
        .trim_matches(|c| matches!(c, '_' | '-' | ' ' | '+'))
        .chars()
    {
        let c = if c == '_' { ' ' } else { c };
        if c.is_uppercase() && last.is_some_and(char::is_lowercase) {
            out.push(' ');
        }
        out.push(c);
        last = Some(c);
    }
    if out.is_empty() { stem.to_owned() } else { out }
}

/// What to say about the file or folder at `path`.
pub fn advise(path: &Path) -> Advice {
    advise_as(path, detect(path))
}

pub fn advise_as(path: &Path, kind: FileKind) -> Advice {
    let name = pretty_name(&display_name(path));
    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let (message, fix) = match kind {
        FileKind::LuaMod => (format!("{name} is a Rustcraft mod"), Fix::Install),
        FileKind::ResourcePack => (
            format!("{name} is a resource pack, not a mod"),
            Fix::MoveToResourcePacks,
        ),
        FileKind::ResourceAndDataPack => (
            format!("{name} is a resource pack (and a data pack), not a mod"),
            Fix::MoveToResourcePacks,
        ),
        FileKind::DataPack => (
            format!("{name} is a data pack. Data packs go in a world."),
            Fix::AddToWorld,
        ),
        FileKind::JavaMod => match built_in_name(&file_name) {
            Some(title) => (
                format!(
                    "{title} is built in: see Barebones Extras (Options, Video Settings, Extras...)"
                ),
                Fix::ShowInFinder,
            ),
            None => (
                format!("{name} is a Java mod. Rustcraft can't run Java mods."),
                Fix::ShowInFinder,
            ),
        },
        FileKind::ShaderPack => (
            format!("{name} is a shader pack. Shader packs are not supported."),
            Fix::ShowInFinder,
        ),
        FileKind::Unknown => (
            format!("{name} isn't a mod, a resource pack or a data pack Rustcraft recognizes."),
            Fix::ShowInFinder,
        ),
    };
    Advice { kind, message, fix }
}

/// Starts the game again a moment after this one has quit (Restart now):
/// the same program with the same arguments.
pub fn relaunch() {
    let Ok(exe) = std::env::current_exe() else {
        return log::warn!("can't restart: the game's own path is unknown");
    };
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = std::process::Command::new("sh")
        .arg("-c")
        .arg("sleep 1; exec \"$0\" \"$@\"")
        .arg(exe)
        .args(args)
        .spawn();
    if let Err(err) = result {
        log::warn!("can't restart: {err}");
    }
}

/// Moves `src` to `dest`, falling back to copy and delete across disks.
fn move_path(src: &Path, dest: &Path) -> Result<()> {
    if dest.exists() {
        if dest.is_dir() {
            std::fs::remove_dir_all(dest)
        } else {
            std::fs::remove_file(dest)
        }
        .with_context(|| format!("replacing {}", dest.display()))?;
    }
    if std::fs::rename(src, dest).is_ok() {
        return Ok(());
    }
    copy_path(src, dest)?;
    if src.is_dir() {
        std::fs::remove_dir_all(src)
    } else {
        std::fs::remove_file(src)
    }
    .with_context(|| format!("removing {}", src.display()))
}

fn copy_path(src: &Path, dest: &Path) -> Result<()> {
    if src.is_dir() {
        std::fs::create_dir_all(dest)?;
        for entry in std::fs::read_dir(src)? {
            let entry = entry?;
            copy_path(&entry.path(), &dest.join(entry.file_name()))?;
        }
    } else {
        std::fs::copy(src, dest)
            .with_context(|| format!("copying {} to {}", src.display(), dest.display()))?;
    }
    Ok(())
}

/// The file name to use for `src` inside `dir`.
fn target(src: &Path, dir: &Path) -> Result<PathBuf> {
    let name = src
        .file_name()
        .with_context(|| format!("{} has no name", src.display()))?;
    std::fs::create_dir_all(dir).with_context(|| format!("making {}", dir.display()))?;
    Ok(dir.join(name))
}

/// Puts the resource pack `src` in `packs_dir`: moved when it is already in
/// `moving_from` (the Mods folder), copied otherwise (a download).
/// Returns the pack's id: its file name in the folder.
pub fn install_resource_pack(src: &Path, packs_dir: &Path, move_it: bool) -> Result<String> {
    let dest = target(src, packs_dir)?;
    if src.canonicalize().ok() != dest.canonicalize().ok() {
        if move_it {
            move_path(src, &dest)?;
        } else {
            if dest.is_dir() {
                std::fs::remove_dir_all(&dest)?;
            }
            copy_path(src, &dest)?;
        }
    }
    Ok(dest
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default())
}

/// Puts the Lua mod `src` in `mods_dir`.
pub fn install_lua_mod(src: &Path, mods_dir: &Path, move_it: bool) -> Result<()> {
    let dest = target(src, mods_dir)?;
    if src.canonicalize().ok() == dest.canonicalize().ok() {
        return Ok(());
    }
    if move_it {
        move_path(src, &dest)
    } else {
        copy_path(src, &dest)
    }
}

/// Puts the data pack `src` in `world/datapacks/<name>/`: a folder is
/// copied, a `.zip` is unpacked (the game reads data packs as folders),
/// without the folder that wraps it. Returns the pack's folder name.
pub fn install_data_pack(src: &Path, world: &Path) -> Result<String> {
    let folder = display_name(src);
    let dest = world.join("datapacks").join(&folder);
    if dest.exists() {
        std::fs::remove_dir_all(&dest).with_context(|| format!("replacing {folder}"))?;
    }
    if src.is_dir() {
        // A wrapped folder: the one that holds pack.mcmeta.
        let root = if src.join("pack.mcmeta").is_file() {
            src.to_owned()
        } else {
            std::fs::read_dir(src)?
                .flatten()
                .map(|e| e.path())
                .find(|p| p.join("pack.mcmeta").is_file())
                .context("no pack.mcmeta in it")?
        };
        copy_path(&root, &dest)?;
        return Ok(folder);
    }
    let file = File::open(src).with_context(|| format!("opening {}", src.display()))?;
    let mut archive = zip::ZipArchive::new(BufReader::new(file))
        .with_context(|| format!("{} isn't a readable .zip", src.display()))?;
    let prefix = {
        let names: Vec<String> = archive
            .file_names()
            .filter_map(|n| n.ok().map(|n| n.into_owned()))
            .collect();
        if names.iter().any(|n| n == "pack.mcmeta") {
            String::new()
        } else {
            names
                .iter()
                .filter_map(|n| n.strip_suffix("pack.mcmeta"))
                .filter(|p| p.ends_with('/') && !p[..p.len() - 1].contains('/'))
                .min_by_key(|p| p.len())
                .map(str::to_owned)
                .context("no pack.mcmeta in it")?
        }
    };
    std::fs::create_dir_all(&dest)?;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        let Some(path) = entry.enclosed_name() else {
            bail!("the .zip has an unsafe path in it");
        };
        let Ok(rel) = path.strip_prefix(prefix.trim_end_matches('/')) else {
            continue;
        };
        if rel.as_os_str().is_empty() {
            continue;
        }
        let out = dest.join(rel);
        if entry.is_dir() {
            std::fs::create_dir_all(&out)?;
        } else {
            if let Some(parent) = out.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::io::copy(&mut entry, &mut File::create(&out)?)?;
        }
    }
    Ok(folder)
}

/// The saved worlds (folders with a `level.ron`) in `saves`, by name.
pub fn worlds_in(saves: &Path) -> Vec<(String, PathBuf)> {
    let mut out: Vec<(String, PathBuf)> = std::fs::read_dir(saves)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.join("level.ron").is_file())
        .filter_map(|p| Some((p.file_name()?.to_string_lossy().into_owned(), p)))
        .collect();
    out.sort_by_key(|(name, _)| name.to_lowercase());
    out
}

/// Files in `dir` (the resource packs folder) that aren't resource packs,
/// with what each is, for the Resource Packs screen to say so.
pub fn misplaced_in(dir: &Path) -> Vec<(String, Advice)> {
    super::kind::children(dir)
        .into_iter()
        .filter(|p| {
            p.is_dir()
                || p.extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("zip") || e.eq_ignore_ascii_case("jar"))
        })
        .filter_map(|p| {
            let advice = advise(&p);
            (!advice.kind.is_resource_pack()).then(|| {
                let name = p
                    .file_name()
                    .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
                (name, advice)
            })
        })
        .collect()
}

/// A mod's text for a failure, in plain words with what to do about it
/// (the raw `ron` and loader messages are for the log).
pub fn plain_problem(raw: &str) -> String {
    let raw = raw.trim();
    let lower = raw.to_lowercase();
    if let Some(field) = missing_field(raw) {
        return format!("mod.ron is missing a {field} line. Add it, then restart.");
    }
    if lower.contains("no mod.ron") || lower.contains("cannot read mod.ron") {
        return "There is no mod.ron in it. Every mod needs one at its top.".to_owned();
    }
    if lower.contains("unknown field") {
        let field = raw
            .split('`')
            .nth(1)
            .map_or_else(String::new, |f| format!(" {f}"));
        return format!(
            "mod.ron has a line Rustcraft doesn't know{field}. Remove it, then restart."
        );
    }
    if lower.starts_with("mod.ron") || lower.contains("expected") {
        return "mod.ron can't be read: check its brackets, commas and quotes, then restart."
            .to_owned();
    }
    raw.to_owned()
}

/// "missing field `version`" → `version`.
fn missing_field(raw: &str) -> Option<&str> {
    let at = raw.find("missing field")?;
    raw[at..].split('`').nth(1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{pack::tests::write_zip, world::save::TempDir};

    fn zip_at(dir: &Path, name: &str, files: &[&str]) -> PathBuf {
        let path = dir.join(name);
        let files: Vec<(&str, &[u8])> = files
            .iter()
            .map(|f| (*f, crate::pack::tests::MCMETA))
            .collect();
        write_zip(&path, &files);
        path
    }

    #[test]
    fn video_mods_are_named_by_their_files() {
        assert_eq!(built_in_name("cave_dust-3.1.0.jar"), Some("Cave Dust"));
        assert_eq!(
            built_in_name("sound-physics-remastered-fabric-1.5.2+26.3.jar"),
            Some("Sound Physics Remastered")
        );
        assert_eq!(built_in_name("sodium-fabric-0.6.jar"), None);
    }

    #[test]
    fn download_names_become_titles() {
        assert_eq!(
            pretty_name("FreshAnimations_v1.11.2_26.1+"),
            "Fresh Animations"
        );
        assert_eq!(pretty_name("Bare Bones 1.21.11"), "Bare Bones");
        assert_eq!(pretty_name("wakes-0.7.3+26.3"), "wakes");
        assert_eq!(pretty_name("Patrix_26.3_32x_basic"), "Patrix");
        assert_eq!(pretty_name("ruby"), "ruby");
    }

    #[test]
    fn advice_for_each_kind() {
        let dir = TempDir::new("advice");
        let fa = zip_at(
            dir.path(),
            "Fresh Animations.zip",
            &["pack.mcmeta", "assets/minecraft/a"],
        );
        let a = advise(&fa);
        assert_eq!(a.fix, Fix::MoveToResourcePacks);
        assert_eq!(a.message, "Fresh Animations is a resource pack, not a mod");
        let data = zip_at(dir.path(), "tweaks.zip", &["pack.mcmeta", "data/a/b.json"]);
        assert_eq!(advise(&data).fix, Fix::AddToWorld);
        let java = zip_at(dir.path(), "wakes-0.7.3.jar", &["fabric.mod.json"]);
        let a = advise(&java);
        assert!(
            a.message.contains("built in: see Barebones Extras"),
            "{}",
            a.message
        );
        let other = zip_at(dir.path(), "sodium.jar", &["fabric.mod.json"]);
        assert_eq!(
            advise(&other).message,
            "sodium is a Java mod. Rustcraft can't run Java mods."
        );
        let shader = zip_at(dir.path(), "bsl.zip", &["shaders/final.fsh"]);
        assert_eq!(advise(&shader).fix, Fix::ShowInFinder);
        assert!(advise(&shader).message.contains("not supported"));
    }

    #[test]
    fn a_resource_pack_moves_and_a_download_is_copied() {
        let dir = TempDir::new("move-pack");
        let mods = dir.path().join("mods");
        let packs = dir.path().join("resourcepacks");
        std::fs::create_dir_all(&mods).unwrap();
        let in_mods = zip_at(&mods, "FA.zip", &["pack.mcmeta", "assets/minecraft/a"]);
        let id = install_resource_pack(&in_mods, &packs, true).unwrap();
        assert_eq!(id, "FA.zip");
        assert!(!in_mods.exists() && packs.join("FA.zip").is_file());
        let download = zip_at(
            dir.path(),
            "Other.zip",
            &["pack.mcmeta", "assets/minecraft/a"],
        );
        install_resource_pack(&download, &packs, false).unwrap();
        assert!(download.exists() && packs.join("Other.zip").is_file());
    }

    #[test]
    fn a_data_pack_zip_is_unpacked_into_the_world_without_its_wrapper() {
        let dir = TempDir::new("data-pack");
        let world = dir.path().join("saves/w");
        std::fs::create_dir_all(&world).unwrap();
        let src = zip_at(
            dir.path(),
            "tweaks.zip",
            &["Tweaks/pack.mcmeta", "Tweaks/data/x/y.json"],
        );
        let name = install_data_pack(&src, &world).unwrap();
        assert_eq!(name, "tweaks");
        assert!(world.join("datapacks/tweaks/pack.mcmeta").is_file());
        assert!(world.join("datapacks/tweaks/data/x/y.json").is_file());
        // The game's own reader finds it.
        assert_eq!(crate::game::datapack::discover(&world).len(), 1);
    }

    #[test]
    fn plain_words_for_a_broken_mod_ron() {
        let err = crate::mods::parse_manifest(r#"(id: "x", name: "X")"#).unwrap_err();
        let plain = plain_problem(&format!("{err:#}"));
        assert!(
            plain.contains("mod.ron is missing a version line"),
            "{plain}"
        );
        assert_eq!(
            plain_problem("no mod.ron"),
            "There is no mod.ron in it. Every mod needs one at its top."
        );
        let junk = crate::mods::parse_manifest("(((").unwrap_err();
        assert!(plain_problem(&format!("{junk:#}")).starts_with("mod.ron can't be read"));
    }

    #[test]
    fn the_packs_folder_lists_what_isnt_a_pack() {
        let dir = TempDir::new("misplaced");
        zip_at(
            dir.path(),
            "good.zip",
            &["pack.mcmeta", "assets/minecraft/a"],
        );
        zip_at(dir.path(), "visuality-0.7.jar", &["fabric.mod.json"]);
        let list = misplaced_in(dir.path());
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].0, "visuality-0.7.jar");
    }
}
