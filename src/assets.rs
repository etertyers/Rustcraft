//! Locating the `assets/` folder, and where the game keeps its files.

use std::path::{Path, PathBuf};

/// The assets folder: `$RUSTCRAFT_ASSETS` if set, else `./assets` if it
/// exists, else the app bundle's `Contents/Resources/assets`, else the one
/// in the source tree (for `cargo run` from another folder).
pub fn dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("RUSTCRAFT_ASSETS") {
        return dir.into();
    }
    let local = PathBuf::from("assets");
    if local.is_dir() {
        return local;
    }
    if let Some(resources) = bundle_resources() {
        return resources.join("assets");
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets")
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

/// What the asset layers cover, in lines for `--asset-report`: textures
/// and sounds still missing with Kenzo's assets, the packs' none, and
/// vanilla under them.
pub fn report(assets: &Path) -> anyhow::Result<String> {
    use crate::{
        entity::mobs::MobRegistry, inventory::item, look::mob_texture, pack::Resources,
        world::block,
    };
    let res = Resources::load(assets, Vec::new())?;
    let mut out = String::new();
    let has = |name: &str| res.texture(name).is_some();
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
    let missing_blocks = blocks.missing_textures_in(has, true);
    let missing_items = items.missing_textures_in(has, true);
    let mobs = MobRegistry::load(&assets.join("data/mobs.ron"))?;
    let missing_mobs: Vec<_> = mobs
        .textures_sized()
        .into_iter()
        .filter(|&(name, size)| mob_texture(&res, name, Some(size)).is_none())
        .map(|(name, _)| name)
        .collect();
    out += &format!(
        "missing textures: {} block, {} item, {} mob {:?}{:?}{:?}\n",
        missing_blocks.len(),
        missing_items.len(),
        missing_mobs.len(),
        missing_blocks,
        missing_items,
        missing_mobs
    );
    let sounds = crate::audio::report(&res);
    out += &format!(
        "sound events: {} in the table, {} with no file at all {:?}, {} files named but absent\n",
        sounds.events,
        sounds.silent.len(),
        sounds.silent,
        sounds.missing_files.len()
    );
    Ok(out)
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
}
