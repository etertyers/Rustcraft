//! Loading read-only assets (M70). The game reads textures, sounds, data
//! files and packs through [`AssetSource`]. Native reads them from the asset
//! root synchronously. In the browser (M71) a source fetches them over HTTP
//! or from OPFS, so loading is asynchronous: the app shows a loading screen
//! until [`AssetSource::ready`] says the files it asked for are in.

use std::{io, path::Path};

pub trait AssetSource: Send + Sync {
    /// The bytes of the file at `path` under the asset root.
    fn read(&self, path: &Path) -> io::Result<Vec<u8>>;
    /// Whether the file exists.
    fn exists(&self, path: &Path) -> bool;
    /// The names in a folder.
    fn list(&self, path: &Path) -> io::Result<Vec<String>>;
    /// Whether everything asked for with `prefetch` has arrived. Native is
    /// always ready.
    fn ready(&self) -> bool {
        true
    }
    /// Asks for files to be loaded in the background (a no-op on native).
    fn prefetch(&self, _paths: &[&Path]) {}
}

/// Assets in a folder, read synchronously.
pub struct FolderAssets {
    root: std::path::PathBuf,
}

impl FolderAssets {
    pub fn new(root: impl Into<std::path::PathBuf>) -> Self {
        Self { root: root.into() }
    }
}

impl AssetSource for FolderAssets {
    fn read(&self, path: &Path) -> io::Result<Vec<u8>> {
        super::fs::read(self.root.join(path))
    }

    fn exists(&self, path: &Path) -> bool {
        super::fs::exists(self.root.join(path))
    }

    fn list(&self, path: &Path) -> io::Result<Vec<String>> {
        let mut names: Vec<String> = super::fs::read_dir(self.root.join(path))?
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        Ok(names)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folder_assets_read_list_and_exist() {
        let tmp = std::env::temp_dir().join(format!("rc-assets-{}", std::process::id()));
        std::fs::create_dir_all(tmp.join("data")).unwrap();
        std::fs::write(tmp.join("data/blocks.ron"), "x").unwrap();
        let a = FolderAssets::new(&tmp);
        assert!(a.ready());
        assert!(a.exists(Path::new("data/blocks.ron")));
        assert!(!a.exists(Path::new("data/none.ron")));
        assert_eq!(a.read(Path::new("data/blocks.ron")).unwrap(), b"x");
        assert_eq!(a.list(Path::new("data")).unwrap(), vec!["blocks.ron"]);
        std::fs::remove_dir_all(&tmp).unwrap();
    }
}
