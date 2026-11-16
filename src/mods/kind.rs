//! What a downloaded file is (M66). Many downloads are not Lua mods:
//! Fresh Animations is a resource pack, Visuality a Java mod. One function
//! looks at a folder or `.zip` (and one wrapping folder level, since many
//! downloads wrap their contents) and says which kind it is, so the Mods
//! screen, the Resource Packs screen and drag-and-drop route it the same.

use crate::platform::fs::File;
use std::{
    io::BufReader,
    path::{Path, PathBuf},
};

/// What a folder or archive holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    /// A Rustcraft Lua mod (`mod.ron`).
    LuaMod,
    /// `pack.mcmeta` with `assets/`.
    ResourcePack,
    /// `pack.mcmeta` with `data/`.
    DataPack,
    /// `pack.mcmeta` with both `assets/` and `data/`.
    ResourceAndDataPack,
    /// A `.jar`, `fabric.mod.json`, `mods.toml` or `META-INF`.
    JavaMod,
    /// A `shaders/` folder (OptiFine or Iris).
    ShaderPack,
    Unknown,
}

impl FileKind {
    /// Whether it can be used as a resource pack.
    pub fn is_resource_pack(self) -> bool {
        matches!(self, Self::ResourcePack | Self::ResourceAndDataPack)
    }

    /// Whether it can be used as a data pack.
    pub fn is_data_pack(self) -> bool {
        matches!(self, Self::DataPack | Self::ResourceAndDataPack)
    }

    /// The words the screens use ("a resource pack").
    pub fn noun(self) -> &'static str {
        match self {
            Self::LuaMod => "a Rustcraft mod",
            Self::ResourcePack => "a resource pack",
            Self::DataPack => "a data pack",
            Self::ResourceAndDataPack => "a resource pack and a data pack",
            Self::JavaMod => "a Java mod",
            Self::ShaderPack => "a shader pack",
            Self::Unknown => "something Rustcraft doesn't recognize",
        }
    }
}

/// Every file name inside `path`, with `/` separators: a `.zip`'s entries,
/// or a folder's files three levels down at most.
fn names_in(path: &Path) -> Vec<String> {
    if crate::platform::fs::is_dir(path) {
        let mut out = Vec::new();
        walk(path, "", 0, &mut out);
        return out;
    }
    let Ok(file) = File::open(path) else {
        return Vec::new();
    };
    let Ok(archive) = zip::ZipArchive::new(BufReader::new(file)) else {
        return Vec::new();
    };
    archive
        .file_names()
        .filter_map(|n| n.ok().map(|n| n.into_owned()))
        .collect()
}

fn walk(dir: &Path, prefix: &str, depth: usize, out: &mut Vec<String>) {
    let Ok(entries) = crate::platform::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = format!("{prefix}{}", entry.file_name().to_string_lossy());
        if crate::platform::fs::is_dir(entry.path()) {
            out.push(format!("{name}/"));
            if depth < 3 {
                walk(&entry.path(), &format!("{name}/"), depth + 1, out);
            }
        } else {
            out.push(name);
        }
    }
}

/// What the entries under one root say.
fn kind_of(names: &[String], root: &str) -> FileKind {
    let has = |rel: &str| {
        let wanted = format!("{root}{rel}");
        names.iter().any(|n| n == &wanted)
    };
    let has_dir = |rel: &str| {
        let wanted = format!("{root}{rel}/");
        names.iter().any(|n| n.starts_with(&wanted))
    };
    if has("fabric.mod.json")
        || has("quilt.mod.json")
        || has("META-INF/mods.toml")
        || has("META-INF/neoforge.mods.toml")
        || has("mcmod.info")
    {
        return FileKind::JavaMod;
    }
    if has("mod.ron") {
        return FileKind::LuaMod;
    }
    if has("pack.mcmeta") {
        return match (has_dir("assets"), has_dir("data")) {
            (true, true) => FileKind::ResourceAndDataPack,
            (true, false) => FileKind::ResourcePack,
            (false, true) => FileKind::DataPack,
            (false, false) => FileKind::Unknown,
        };
    }
    if has_dir("shaders") {
        return FileKind::ShaderPack;
    }
    FileKind::Unknown
}

/// Which kind of thing the folder or `.zip` at `path` is.
pub fn detect(path: &Path) -> FileKind {
    let is_jar = path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("jar"));
    let names = names_in(path);
    if names.is_empty() {
        return if is_jar {
            FileKind::JavaMod
        } else {
            FileKind::Unknown
        };
    }
    // The top level first, then each single wrapping folder.
    let mut roots = vec![String::new()];
    for name in &names {
        if let Some((top, _)) = name.split_once('/') {
            let root = format!("{top}/");
            if !roots.contains(&root) {
                roots.push(root);
            }
        }
    }
    let found = roots
        .iter()
        .map(|root| kind_of(&names, root))
        .find(|kind| *kind != FileKind::Unknown);
    match found {
        // A jar with a plain META-INF folder and no recognized files is
        // still a Java mod.
        None if is_jar || names.iter().any(|n| n.starts_with("META-INF/")) => FileKind::JavaMod,
        None => FileKind::Unknown,
        Some(kind) => kind,
    }
}

/// The path of each `.zip`, `.jar` or folder directly inside `dir`,
/// sorted by name.
pub fn children(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = crate::platform::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut paths: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
    paths.sort();
    paths
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{pack::tests::write_zip, world::save::TempDir};

    fn zip_of(dir: &TempDir, name: &str, files: &[&str]) -> PathBuf {
        let path = dir.path().join(name);
        let files: Vec<(&str, &[u8])> = files.iter().map(|f| (*f, b"x".as_slice())).collect();
        write_zip(&path, &files);
        path
    }

    #[test]
    fn each_kind_is_recognized_in_a_zip() {
        let dir = TempDir::new("kinds");
        let kind = |name: &str, files: &[&str]| detect(&zip_of(&dir, name, files));
        assert_eq!(kind("lua.zip", &["mod.ron", "main.lua"]), FileKind::LuaMod);
        assert_eq!(
            kind("res.zip", &["pack.mcmeta", "assets/minecraft/a.png"]),
            FileKind::ResourcePack
        );
        assert_eq!(
            kind("data.zip", &["pack.mcmeta", "data/x/y.json"]),
            FileKind::DataPack
        );
        assert_eq!(
            kind(
                "both.zip",
                &["pack.mcmeta", "assets/minecraft/a.png", "data/x/y.json"]
            ),
            FileKind::ResourceAndDataPack
        );
        assert_eq!(
            kind("java.jar", &["fabric.mod.json", "a/B.class"]),
            FileKind::JavaMod
        );
        assert_eq!(
            kind("forge.zip", &["META-INF/mods.toml", "a/B.class"]),
            FileKind::JavaMod
        );
        assert_eq!(
            kind("shader.zip", &["shaders/final.fsh", "shaders/final.vsh"]),
            FileKind::ShaderPack
        );
        assert_eq!(kind("what.zip", &["readme.txt"]), FileKind::Unknown);
    }

    #[test]
    fn a_zip_with_one_wrapping_folder_is_looked_into() {
        let dir = TempDir::new("kinds-nested");
        let res = zip_of(
            &dir,
            "wrapped.zip",
            &["My Pack/pack.mcmeta", "My Pack/assets/minecraft/a.png"],
        );
        assert_eq!(detect(&res), FileKind::ResourcePack);
        let lua = zip_of(&dir, "lua.zip", &["ruby/mod.ron", "ruby/main.lua"]);
        assert_eq!(detect(&lua), FileKind::LuaMod);
    }

    #[test]
    fn a_jar_is_a_java_mod_even_without_a_manifest() {
        let dir = TempDir::new("kinds-jar");
        let jar = zip_of(&dir, "odd.jar", &["a/B.class"]);
        assert_eq!(detect(&jar), FileKind::JavaMod);
    }

    #[test]
    fn folders_are_recognized_too() {
        let dir = TempDir::new("kinds-folder");
        let pack = dir.path().join("pack");
        crate::platform::fs::create_dir_all(pack.join("assets/minecraft")).unwrap();
        crate::platform::fs::write(pack.join("pack.mcmeta"), "{}").unwrap();
        assert_eq!(detect(&pack), FileKind::ResourcePack);
        let wrapped = dir.path().join("outer");
        crate::platform::fs::create_dir_all(wrapped.join("inner/data")).unwrap();
        crate::platform::fs::write(wrapped.join("inner/pack.mcmeta"), "{}").unwrap();
        assert_eq!(detect(&wrapped), FileKind::DataPack);
        let empty = dir.path().join("empty");
        crate::platform::fs::create_dir_all(&empty).unwrap();
        assert_eq!(detect(&empty), FileKind::Unknown);
    }

    /// The real download: Fresh Animations is a resource pack, not a mod.
    #[test]
    fn the_real_fresh_animations_file_is_a_resource_pack() {
        let dir = crate::pack::builtin_dir();
        let Some(name) = crate::pack::find_by_words(&dir, &["freshanimations"], &["barebones"])
        else {
            eprintln!("Fresh Animations isn't in assets/downloads; skipped");
            return;
        };
        assert_eq!(detect(&dir.join(name)), FileKind::ResourcePack);
    }
}
