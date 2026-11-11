//! The storage trait (M70): read, write, list, delete and atomic replace.
//! Paths are the same relative paths native uses (`saves/<world>/level.ron`,
//! `options.ron`, `resourcepacks/...`). Backends:
//!
//! - `NativeStorage`: `std::fs`.
//! - `MemoryStorage`: in memory. The browser build starts with it until
//!   M72 installs the OPFS backend with `set_storage`; tests use it too.

use std::{
    collections::{BTreeMap, BTreeSet},
    io,
    path::{Component, Path, PathBuf},
    sync::Mutex,
};

/// One name in a folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub is_dir: bool,
    pub len: u64,
}

/// What `stat` says about a path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stat {
    pub is_dir: bool,
    pub len: u64,
}

pub trait Storage: Send + Sync {
    fn read(&self, path: &Path) -> io::Result<Vec<u8>>;
    /// Writes the file, creating or replacing it (the folder must exist).
    fn write(&self, path: &Path, data: &[u8]) -> io::Result<()>;
    /// Replaces the file so a reader sees the old or the new content, never
    /// half of either (native: write a temp file, then rename).
    fn write_atomic(&self, path: &Path, data: &[u8]) -> io::Result<()>;
    /// The names in a folder, sorted.
    fn list(&self, path: &Path) -> io::Result<Vec<Entry>>;
    /// Deletes a file, or a folder with everything in it.
    fn delete(&self, path: &Path) -> io::Result<()>;
    fn stat(&self, path: &Path) -> io::Result<Stat>;
    fn create_dir_all(&self, path: &Path) -> io::Result<()>;
    fn rename(&self, from: &Path, to: &Path) -> io::Result<()>;
}

fn not_found(path: &Path) -> io::Error {
    io::Error::new(
        io::ErrorKind::NotFound,
        format!("{}: not found", path.display()),
    )
}

/// `std::fs`.
pub struct NativeStorage;

impl Storage for NativeStorage {
    fn read(&self, path: &Path) -> io::Result<Vec<u8>> {
        std::fs::read(path)
    }

    fn write(&self, path: &Path, data: &[u8]) -> io::Result<()> {
        std::fs::write(path, data)
    }

    fn write_atomic(&self, path: &Path, data: &[u8]) -> io::Result<()> {
        let mut tmp = path.as_os_str().to_owned();
        tmp.push(".tmp");
        let tmp = PathBuf::from(tmp);
        std::fs::write(&tmp, data)?;
        std::fs::rename(&tmp, path)
    }

    fn list(&self, path: &Path) -> io::Result<Vec<Entry>> {
        let mut out = Vec::new();
        for e in std::fs::read_dir(path)? {
            let e = e?;
            let meta = e.metadata()?;
            out.push(Entry {
                name: e.file_name().to_string_lossy().into_owned(),
                is_dir: meta.is_dir(),
                len: if meta.is_dir() { 0 } else { meta.len() },
            });
        }
        out.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(out)
    }

    fn delete(&self, path: &Path) -> io::Result<()> {
        if std::fs::metadata(path)?.is_dir() {
            std::fs::remove_dir_all(path)
        } else {
            std::fs::remove_file(path)
        }
    }

    fn stat(&self, path: &Path) -> io::Result<Stat> {
        let m = std::fs::metadata(path)?;
        Ok(Stat {
            is_dir: m.is_dir(),
            len: if m.is_dir() { 0 } else { m.len() },
        })
    }

    fn create_dir_all(&self, path: &Path) -> io::Result<()> {
        std::fs::create_dir_all(path)
    }

    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        std::fs::rename(from, to)
    }
}

#[derive(Default)]
struct Tree {
    files: BTreeMap<PathBuf, Vec<u8>>,
    dirs: BTreeSet<PathBuf>,
}

/// Files in memory, lost when the page closes.
#[derive(Default)]
pub struct MemoryStorage {
    tree: Mutex<Tree>,
}

/// `a/./b/` and `a/b` are the same; the root is the empty path.
fn norm(path: &Path) -> PathBuf {
    path.components()
        .filter(|c| !matches!(c, Component::CurDir | Component::RootDir))
        .collect()
}

impl MemoryStorage {
    pub fn new() -> Self {
        Self::default()
    }

    fn tree(&self) -> std::sync::MutexGuard<'_, Tree> {
        self.tree.lock().unwrap_or_else(|e| e.into_inner())
    }
}

impl Tree {
    fn is_dir(&self, p: &Path) -> bool {
        p.as_os_str().is_empty() || self.dirs.contains(p)
    }

    fn make_dirs(&mut self, p: &Path) {
        let mut cur = PathBuf::new();
        for c in p.components() {
            cur.push(c);
            self.dirs.insert(cur.clone());
        }
    }
}

impl Storage for MemoryStorage {
    fn read(&self, path: &Path) -> io::Result<Vec<u8>> {
        let p = norm(path);
        self.tree()
            .files
            .get(&p)
            .cloned()
            .ok_or_else(|| not_found(path))
    }

    fn write(&self, path: &Path, data: &[u8]) -> io::Result<()> {
        let p = norm(path);
        let mut t = self.tree();
        if let Some(parent) = p.parent()
            && !t.is_dir(parent)
        {
            return Err(not_found(parent));
        }
        if t.is_dir(&p) {
            return Err(io::Error::other("is a directory"));
        }
        t.files.insert(p, data.to_vec());
        Ok(())
    }

    fn write_atomic(&self, path: &Path, data: &[u8]) -> io::Result<()> {
        // One lock, one insert: already all or nothing.
        self.write(path, data)
    }

    fn list(&self, path: &Path) -> io::Result<Vec<Entry>> {
        let p = norm(path);
        let t = self.tree();
        if !t.is_dir(&p) {
            return Err(not_found(path));
        }
        let mut out = BTreeMap::new();
        for (f, data) in &t.files {
            if f.parent() == Some(p.as_path())
                && let Some(name) = f.file_name()
            {
                out.insert(
                    name.to_string_lossy().into_owned(),
                    (false, data.len() as u64),
                );
            }
        }
        for d in &t.dirs {
            if d.parent() == Some(p.as_path())
                && let Some(name) = d.file_name()
            {
                out.insert(name.to_string_lossy().into_owned(), (true, 0));
            }
        }
        Ok(out
            .into_iter()
            .map(|(name, (is_dir, len))| Entry { name, is_dir, len })
            .collect())
    }

    fn delete(&self, path: &Path) -> io::Result<()> {
        let p = norm(path);
        let mut t = self.tree();
        if t.files.remove(&p).is_some() {
            return Ok(());
        }
        if !t.dirs.contains(&p) {
            return Err(not_found(path));
        }
        t.files.retain(|f, _| !f.starts_with(&p));
        t.dirs.retain(|d| !d.starts_with(&p));
        Ok(())
    }

    fn stat(&self, path: &Path) -> io::Result<Stat> {
        let p = norm(path);
        let t = self.tree();
        if let Some(f) = t.files.get(&p) {
            return Ok(Stat {
                is_dir: false,
                len: f.len() as u64,
            });
        }
        if t.is_dir(&p) {
            return Ok(Stat {
                is_dir: true,
                len: 0,
            });
        }
        Err(not_found(path))
    }

    fn create_dir_all(&self, path: &Path) -> io::Result<()> {
        let p = norm(path);
        let mut t = self.tree();
        if t.files.contains_key(&p) {
            return Err(io::Error::new(io::ErrorKind::AlreadyExists, "is a file"));
        }
        t.make_dirs(&p);
        Ok(())
    }

    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        let (f, t_) = (norm(from), norm(to));
        let mut t = self.tree();
        if let Some(data) = t.files.remove(&f) {
            t.files.insert(t_, data);
            return Ok(());
        }
        if !t.dirs.contains(&f) {
            return Err(not_found(from));
        }
        let moved: Vec<_> = t
            .files
            .keys()
            .filter(|k| k.starts_with(&f))
            .cloned()
            .collect();
        for k in moved {
            let data = t.files.remove(&k).unwrap_or_default();
            t.files
                .insert(t_.join(k.strip_prefix(&f).unwrap_or(&k)), data);
        }
        let dirs: Vec<_> = t
            .dirs
            .iter()
            .filter(|d| d.starts_with(&f))
            .cloned()
            .collect();
        for d in dirs {
            t.dirs.remove(&d);
            t.dirs.insert(t_.join(d.strip_prefix(&f).unwrap_or(&d)));
        }
        t.make_dirs(&t_);
        Ok(())
    }
}

#[cfg(target_arch = "wasm32")]
static CURRENT: std::sync::OnceLock<Box<dyn Storage>> = std::sync::OnceLock::new();

/// The backend the browser build's `platform::fs` uses (M72 installs OPFS
/// before the game starts; until then it is [`MemoryStorage`]).
#[cfg(target_arch = "wasm32")]
pub fn set_storage(storage: Box<dyn Storage>) -> bool {
    CURRENT.set(storage).is_ok()
}

#[cfg(target_arch = "wasm32")]
pub fn storage() -> &'static dyn Storage {
    CURRENT
        .get_or_init(|| Box::new(MemoryStorage::new()))
        .as_ref()
}

/// The backend `platform::fs` uses.
#[cfg(not(target_arch = "wasm32"))]
pub fn storage() -> &'static dyn Storage {
    &NativeStorage
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exercise(s: &dyn Storage, root: &Path) {
        let dir = root.join("saves/w");
        s.create_dir_all(&dir).unwrap();
        s.write(&dir.join("level.ron"), b"one").unwrap();
        s.write_atomic(&dir.join("level.ron"), b"two").unwrap();
        assert_eq!(s.read(&dir.join("level.ron")).unwrap(), b"two");
        assert!(!s.stat(&dir.join("level.ron")).unwrap().is_dir);
        assert_eq!(s.stat(&dir.join("level.ron")).unwrap().len, 3);
        s.create_dir_all(&dir.join("region")).unwrap();
        s.write(&dir.join("region/r.0.0.rcr"), b"abcd").unwrap();
        let list = s.list(&dir).unwrap();
        assert_eq!(
            list,
            vec![
                Entry {
                    name: "level.ron".into(),
                    is_dir: false,
                    len: 3
                },
                Entry {
                    name: "region".into(),
                    is_dir: true,
                    len: 0
                },
            ]
        );
        s.rename(&dir, &root.join("saves/x")).unwrap();
        assert!(s.stat(&dir).is_err());
        assert_eq!(
            s.read(&root.join("saves/x/region/r.0.0.rcr")).unwrap(),
            b"abcd"
        );
        assert_eq!(
            s.read(&root.join("nothing")).unwrap_err().kind(),
            io::ErrorKind::NotFound
        );
        s.delete(&root.join("saves/x")).unwrap();
        assert!(s.list(&root.join("saves")).unwrap().is_empty());
        assert!(s.delete(&root.join("saves/x")).is_err());
    }

    #[test]
    fn memory_storage_behaves_like_files() {
        exercise(&MemoryStorage::new(), Path::new(""));
    }

    #[test]
    fn native_storage_behaves_like_files() {
        let tmp = std::env::temp_dir().join(format!("rc-storage-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        exercise(&NativeStorage, &tmp);
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn memory_write_needs_the_folder() {
        let s = MemoryStorage::new();
        assert!(s.write(Path::new("a/b.txt"), b"x").is_err());
    }
}
