//! The browser's storage (M71): memory, with the files worth keeping
//! mirrored to OPFS in the background. M72 replaces it with OPFS sync access
//! handles in the server worker.

use std::{io, path::Path};

use crate::platform::storage::{Entry, MemoryStorage, Stat, Storage, set_storage, storage};

/// Files that survive a reload: settings, packs, marks and the worlds.
fn persisted(path: &str) -> bool {
    path == "options.ron"
        || path == "texture-marks.ron"
        || path.starts_with("resourcepacks/")
        || path.starts_with("assets/downloads/resourcepacks/")
        || path.starts_with("saves/")
}

pub struct WebStorage {
    memory: MemoryStorage,
}

fn key(path: &Path) -> String {
    path.components()
        .filter(|c| {
            !matches!(
                c,
                std::path::Component::CurDir | std::path::Component::RootDir
            )
        })
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

impl WebStorage {
    /// Sends a file, or everything in a folder, to OPFS.
    fn persist(&self, path: &Path) {
        let key = key(path);
        if !persisted(&key) {
            return;
        }
        let Ok(data) = self.memory.read(path) else {
            if let Ok(entries) = self.memory.list(path) {
                for e in entries {
                    self.persist(&path.join(e.name));
                }
            }
            return;
        };
        wasm_bindgen_futures::spawn_local(async move {
            if let Err(err) = super::opfs_write(&key, &data).await {
                log::warn!("couldn't keep {key} in the browser's storage: {err:?}");
            }
        });
    }

    fn forget(&self, path: &Path) {
        let key = key(path);
        if persisted(&key) {
            wasm_bindgen_futures::spawn_local(async move {
                let _ = super::opfs_delete(&key).await;
            });
        }
    }
}

impl Storage for WebStorage {
    fn read(&self, path: &Path) -> io::Result<Vec<u8>> {
        self.memory.read(path)
    }
    fn write(&self, path: &Path, data: &[u8]) -> io::Result<()> {
        self.memory.write(path, data)?;
        self.persist(path);
        Ok(())
    }
    fn write_atomic(&self, path: &Path, data: &[u8]) -> io::Result<()> {
        self.write(path, data)
    }
    fn list(&self, path: &Path) -> io::Result<Vec<Entry>> {
        self.memory.list(path)
    }
    fn delete(&self, path: &Path) -> io::Result<()> {
        self.memory.delete(path)?;
        self.forget(path);
        Ok(())
    }
    fn stat(&self, path: &Path) -> io::Result<Stat> {
        self.memory.stat(path)
    }
    fn create_dir_all(&self, path: &Path) -> io::Result<()> {
        self.memory.create_dir_all(path)
    }
    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        self.memory.rename(from, to)?;
        self.forget(from);
        self.persist(to);
        Ok(())
    }
}

pub fn install() {
    let _ = set_storage(Box::new(WebStorage {
        memory: MemoryStorage::new(),
    }));
}

/// Every file path under `prefix`, relative to it.
pub fn paths_under(prefix: &str) -> std::collections::HashSet<String> {
    fn walk(dir: &str, prefix: &str, out: &mut std::collections::HashSet<String>) {
        let Ok(entries) = storage().list(Path::new(dir)) else {
            return;
        };
        for e in entries {
            let full = format!("{dir}/{}", e.name);
            if e.is_dir {
                walk(&full, prefix, out);
            } else if let Some(rel) = full.strip_prefix(prefix) {
                out.insert(rel.to_owned());
            }
        }
    }
    let mut out = std::collections::HashSet::new();
    walk(prefix.trim_end_matches('/'), prefix, &mut out);
    out
}

/// The files under `from` as a zip (no compression: they are mostly Ogg and
/// PNG already), with each path starting `to` instead of `from`.
pub fn pack_zip(from: &str, to: &str) -> Result<Vec<u8>, String> {
    use std::io::Write;
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    for rel in paths_under(from) {
        let data = storage()
            .read(Path::new(&format!("{from}{rel}")))
            .map_err(|e| e.to_string())?;
        zip.start_file(format!("{to}{rel}"), options)
            .map_err(|e| e.to_string())?;
        zip.write_all(&data).map_err(|e| e.to_string())?;
    }
    zip.finish()
        .map(|c| c.into_inner())
        .map_err(|e| e.to_string())
}
