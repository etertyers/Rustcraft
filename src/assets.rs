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
