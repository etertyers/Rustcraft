//! Worlds as .zip files (M72): export and import with the same folder layout
//! as native saves (`<world>/level.ron`, `<world>/region/...`,
//! `<world>/playerdata/...`), so a world made in the browser opens in the Mac
//! app (unzip into `saves/`) and the other way round. Works on `platform::fs`,
//! so it runs the same natively (tests) and in the browser.

use std::{
    io::{Cursor, Read, Write},
    path::{Component, Path, PathBuf},
};

use anyhow::{Context, Result, bail};

use super::fs;

/// A world's folder (`saves/<name>`) as a zip whose entries start `<name>/`.
pub fn export(saves: &Path, name: &str) -> Result<Vec<u8>> {
    let root = saves.join(name);
    if !fs::is_file(root.join("level.ron")) {
        bail!("{name} is not a world (no level.ron)");
    }
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    let mut stack = vec![(root.clone(), PathBuf::from(name))];
    while let Some((dir, rel)) = stack.pop() {
        let mut entries: Vec<_> = fs::read_dir(&dir)?.flatten().collect();
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            let (path, rel) = (entry.path(), rel.join(entry.file_name()));
            if entry.file_type()?.is_dir() {
                stack.push((path, rel));
            } else {
                zip.start_file(rel.to_string_lossy().replace('\\', "/"), options)?;
                zip.write_all(&fs::read(&path)?)?;
            }
        }
    }
    Ok(zip.finish()?.into_inner())
}

/// Whether a zip entry path stays inside the folder it is unpacked into.
fn safe(path: &str) -> bool {
    !path.is_empty()
        && Path::new(path)
            .components()
            .all(|c| matches!(c, Component::Normal(_)))
}

/// Unpacks a world zip into `saves/` and returns the folder name used: the
/// zip's own top folder, or "Imported World" for a zip with `level.ron` at
/// the top; a name already taken gets " (2)", " (3)", ...
pub fn import(saves: &Path, bytes: &[u8]) -> Result<String> {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).context("not a zip file")?;
    let names: Vec<String> = (0..zip.len())
        .map(|i| {
            zip.by_index(i)
                .map(|e| e.name().map(|n| n.into_owned()).unwrap_or_default())
                .unwrap_or_default()
        })
        .collect();
    let level = names
        .iter()
        .filter(|n| n.ends_with("level.ron") && safe(n))
        .min_by_key(|n| n.matches('/').count())
        .context("no level.ron in the zip: it is not a Rustcraft world")?;
    let prefix = level.trim_end_matches("level.ron"); // "" or "World/" or "a/World/"
    let base = prefix
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or("Imported World")
        .to_owned();
    let mut name = base.clone();
    let mut n = 2;
    while fs::exists(saves.join(&name)) {
        name = format!("{base} ({n})");
        n += 1;
    }
    let dest = saves.join(&name);
    let mut buf = Vec::new();
    for (i, entry_name) in names.iter().enumerate() {
        let Some(rel) = entry_name.strip_prefix(prefix) else {
            continue;
        };
        if rel.is_empty() || rel.ends_with('/') || !safe(rel) {
            continue;
        }
        let mut entry = zip.by_index(i)?;
        buf.clear();
        entry.read_to_end(&mut buf)?;
        let out = dest.join(rel);
        if let Some(parent) = out.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&out, &buf)?;
    }
    Ok(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::save::TempDir;

    fn tree(root: &Path) -> Vec<(String, Vec<u8>)> {
        let mut out = Vec::new();
        let mut stack = vec![root.to_path_buf()];
        while let Some(dir) = stack.pop() {
            for e in fs::read_dir(&dir).unwrap().flatten() {
                if e.file_type().unwrap().is_dir() {
                    stack.push(e.path());
                } else {
                    let rel = e
                        .path()
                        .strip_prefix(root)
                        .unwrap()
                        .to_string_lossy()
                        .into_owned();
                    out.push((rel, fs::read(e.path()).unwrap()));
                }
            }
        }
        out.sort();
        out
    }

    fn make_world(saves: &Path, name: &str) {
        let w = saves.join(name);
        fs::create_dir_all(w.join("region")).unwrap();
        fs::create_dir_all(w.join("playerdata")).unwrap();
        fs::write(w.join("level.ron"), "(name: \"x\")").unwrap();
        fs::write(w.join("region/r.0.0.rcr"), [1u8, 2, 3, 4, 5]).unwrap();
        fs::write(w.join("playerdata/Kenzo.ron"), "p").unwrap();
    }

    #[test]
    fn a_world_survives_export_and_import_with_the_same_layout() {
        let a = TempDir::new("worldzip-a");
        let b = TempDir::new("worldzip-b");
        make_world(a.path(), "My World");
        let bytes = export(a.path(), "My World").unwrap();
        let name = import(b.path(), &bytes).unwrap();
        assert_eq!(name, "My World");
        assert_eq!(
            tree(&a.path().join("My World")),
            tree(&b.path().join("My World"))
        );
        // Importing again doesn't overwrite.
        assert_eq!(import(b.path(), &bytes).unwrap(), "My World (2)");
        assert_eq!(import(b.path(), &bytes).unwrap(), "My World (3)");
    }

    #[test]
    fn export_of_a_missing_world_and_import_of_junk_fail_cleanly() {
        let a = TempDir::new("worldzip-c");
        assert!(export(a.path(), "Nope").is_err());
        assert!(import(a.path(), b"not a zip").is_err());
    }

    #[test]
    fn entries_cannot_escape_the_saves_folder() {
        let a = TempDir::new("worldzip-d");
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let o = zip::write::SimpleFileOptions::default();
        zip.start_file("W/level.ron", o).unwrap();
        zip.write_all(b"x").unwrap();
        zip.start_file("W/../../evil.txt", o).unwrap();
        zip.write_all(b"x").unwrap();
        let bytes = zip.finish().unwrap().into_inner();
        assert_eq!(import(a.path(), &bytes).unwrap(), "W");
        assert!(!fs::exists(a.path().join("../evil.txt")));
        assert_eq!(tree(&a.path().join("W")).len(), 1);
    }
}

#[cfg(test)]
mod browser_world {
    use super::*;
    use crate::world::save::TempDir;

    /// `RUSTCRAFT_BROWSER_WORLD_ZIP=<file>` is a world exported from the
    /// browser (`web/test/exchange.mjs` makes one): it unpacks into a saves
    /// folder and opens with the code the game's Select World uses.
    #[test]
    #[ignore = "needs a world zip exported from the browser"]
    fn a_world_exported_from_the_browser_opens_natively() {
        let Some(path) = crate::platform::env::var_os("RUSTCRAFT_BROWSER_WORLD_ZIP") else {
            panic!("set RUSTCRAFT_BROWSER_WORLD_ZIP to a world zip");
        };
        let saves = TempDir::new("browser-world");
        let name = import(saves.path(), &std::fs::read(path).unwrap()).unwrap();
        let dir = crate::level::WorldDir::new(saves.path().join(&name));
        assert!(dir.exists(), "{name} has a level.ron");
        let level = dir.load_level().expect("the level loads");
        let (items, models) = crate::game::tests::registries();
        crate::game::Game::open(dir, items, models).expect("the world opens");
        eprintln!("opened {name} ({}): ok", level.name);
    }

    /// Makes `RUSTCRAFT_NATIVE_WORLD_ZIP=<file>`: a world created and saved
    /// by the native code, as a zip for the browser to import
    /// (`web/test/exchange.mjs native`).
    #[test]
    #[ignore = "writes a world zip for the browser tests"]
    fn make_a_native_world_zip_for_the_browser() {
        let Some(path) = crate::platform::env::var_os("RUSTCRAFT_NATIVE_WORLD_ZIP") else {
            panic!("set RUSTCRAFT_NATIVE_WORLD_ZIP to the file to write");
        };
        let saves = TempDir::new("native-world");
        let (items, models) = crate::game::tests::registries();
        let mut game = crate::game::Game::create(
            saves.path(),
            "Native1",
            7,
            crate::player::mode::GameMode::Survival,
            items,
            models,
        )
        .expect("a world");
        for _ in 0..40 {
            game.chunks.update(crate::world::chunk::ChunkPos::new(0, 0));
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        game.save().expect("saved");
        let zip = export(saves.path(), "Native1").expect("exported");
        std::fs::write(path, zip).unwrap();
    }
}
