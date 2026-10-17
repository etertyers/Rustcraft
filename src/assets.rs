//! Locating the `assets/` folder.

use std::path::PathBuf;

/// The assets folder: `$RUSTCRAFT_ASSETS` if set, else `./assets` if it exists,
/// else the one in the source tree (for `cargo run` from another folder).
pub fn dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("RUSTCRAFT_ASSETS") {
        return dir.into();
    }
    let local = PathBuf::from("assets");
    if local.is_dir() {
        return local;
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets")
}
