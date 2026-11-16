//! Files (M70). Game code calls `platform::fs::*` where it used to call
//! `std::fs::*`. Native: the same functions, re-exported. Browser: the same
//! signatures over the installed [`super::storage::Storage`].
//!
//! `exists`, `is_dir` and `is_file` replace `Path::exists` and friends,
//! which ask the operating system and are always false in the browser.

#[cfg(not(target_arch = "wasm32"))]
pub use std::fs::*;

use std::path::Path;

/// Marks a file as used just now (its modification time). The browser's
/// storage keeps none, so there it does nothing.
#[cfg(not(target_arch = "wasm32"))]
pub fn touch(path: impl AsRef<Path>) -> std::io::Result<()> {
    File::options()
        .write(true)
        .open(path)?
        .set_modified(std::time::SystemTime::now())
}

#[cfg(target_arch = "wasm32")]
pub fn touch(_path: impl AsRef<Path>) -> std::io::Result<()> {
    Ok(())
}

/// Whether something is at `path`.
pub fn exists(path: impl AsRef<Path>) -> bool {
    metadata(path).is_ok()
}

/// Whether `path` is a folder.
pub fn is_dir(path: impl AsRef<Path>) -> bool {
    metadata(path).is_ok_and(|m| m.is_dir())
}

/// Whether `path` is a file.
pub fn is_file(path: impl AsRef<Path>) -> bool {
    metadata(path).is_ok_and(|m| m.is_file())
}

#[cfg(target_arch = "wasm32")]
pub use web::*;

#[cfg(target_arch = "wasm32")]
mod web {
    use std::{
        ffi::OsString,
        io::{self, Cursor, Read, Seek, SeekFrom, Write},
        path::{Path, PathBuf},
    };

    use crate::platform::storage::{Entry, storage};

    pub fn read(path: impl AsRef<Path>) -> io::Result<Vec<u8>> {
        storage().read(path.as_ref())
    }

    pub fn read_to_string(path: impl AsRef<Path>) -> io::Result<String> {
        String::from_utf8(read(path)?)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))
    }

    pub fn write(path: impl AsRef<Path>, data: impl AsRef<[u8]>) -> io::Result<()> {
        storage().write(path.as_ref(), data.as_ref())
    }

    pub fn create_dir_all(path: impl AsRef<Path>) -> io::Result<()> {
        storage().create_dir_all(path.as_ref())
    }

    pub fn create_dir(path: impl AsRef<Path>) -> io::Result<()> {
        storage().create_dir_all(path.as_ref())
    }

    pub fn remove_file(path: impl AsRef<Path>) -> io::Result<()> {
        storage().delete(path.as_ref())
    }

    pub fn remove_dir_all(path: impl AsRef<Path>) -> io::Result<()> {
        storage().delete(path.as_ref())
    }

    pub fn rename(from: impl AsRef<Path>, to: impl AsRef<Path>) -> io::Result<()> {
        storage().rename(from.as_ref(), to.as_ref())
    }

    pub fn copy(from: impl AsRef<Path>, to: impl AsRef<Path>) -> io::Result<u64> {
        let data = read(from)?;
        write(to, &data)?;
        Ok(data.len() as u64)
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct FileType(bool);

    impl FileType {
        pub fn is_dir(&self) -> bool {
            self.0
        }
        pub fn is_file(&self) -> bool {
            !self.0
        }
    }

    #[derive(Debug, Clone)]
    pub struct Metadata {
        is_dir: bool,
        len: u64,
    }

    impl Metadata {
        pub fn is_dir(&self) -> bool {
            self.is_dir
        }
        pub fn is_file(&self) -> bool {
            !self.is_dir
        }
        pub fn len(&self) -> u64 {
            self.len
        }
        pub fn is_empty(&self) -> bool {
            self.len == 0
        }
        pub fn file_type(&self) -> FileType {
            FileType(self.is_dir)
        }
        /// The browser's storage keeps no modification times.
        pub fn modified(&self) -> io::Result<crate::platform::time::SystemTime> {
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "no modification times",
            ))
        }
    }

    pub fn metadata(path: impl AsRef<Path>) -> io::Result<Metadata> {
        let s = storage().stat(path.as_ref())?;
        Ok(Metadata {
            is_dir: s.is_dir,
            len: s.len,
        })
    }

    pub struct DirEntry {
        dir: PathBuf,
        entry: Entry,
    }

    impl DirEntry {
        pub fn path(&self) -> PathBuf {
            self.dir.join(&self.entry.name)
        }
        pub fn file_name(&self) -> OsString {
            OsString::from(&self.entry.name)
        }
        pub fn metadata(&self) -> io::Result<Metadata> {
            Ok(Metadata {
                is_dir: self.entry.is_dir,
                len: self.entry.len,
            })
        }
        pub fn file_type(&self) -> io::Result<FileType> {
            Ok(FileType(self.entry.is_dir))
        }
    }

    pub struct ReadDir {
        dir: PathBuf,
        entries: std::vec::IntoIter<Entry>,
    }

    impl Iterator for ReadDir {
        type Item = io::Result<DirEntry>;
        fn next(&mut self) -> Option<Self::Item> {
            self.entries.next().map(|entry| {
                Ok(DirEntry {
                    dir: self.dir.clone(),
                    entry,
                })
            })
        }
    }

    pub fn read_dir(path: impl AsRef<Path>) -> io::Result<ReadDir> {
        let dir = path.as_ref().to_owned();
        let entries = storage().list(&dir)?.into_iter();
        Ok(ReadDir { dir, entries })
    }

    /// A file held in memory: read whole on `open`, written whole when
    /// dropped (or flushed) if it was made with `create`.
    pub struct File {
        path: PathBuf,
        data: Cursor<Vec<u8>>,
        writable: bool,
        dirty: bool,
    }

    impl File {
        pub fn open(path: impl AsRef<Path>) -> io::Result<File> {
            Ok(File {
                path: path.as_ref().to_owned(),
                data: Cursor::new(read(path)?),
                writable: false,
                dirty: false,
            })
        }

        pub fn create(path: impl AsRef<Path>) -> io::Result<File> {
            write(&path, [])?;
            Ok(File {
                path: path.as_ref().to_owned(),
                data: Cursor::new(Vec::new()),
                writable: true,
                dirty: false,
            })
        }

        pub fn metadata(&self) -> io::Result<Metadata> {
            Ok(Metadata {
                is_dir: false,
                len: self.data.get_ref().len() as u64,
            })
        }
    }

    impl Read for File {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            self.data.read(buf)
        }
    }

    impl Seek for File {
        fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
            self.data.seek(pos)
        }
    }

    impl Write for File {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            if !self.writable {
                return Err(io::Error::new(io::ErrorKind::PermissionDenied, "read-only"));
            }
            self.dirty = true;
            self.data.write(buf)
        }

        fn flush(&mut self) -> io::Result<()> {
            if self.dirty {
                storage().write(&self.path, self.data.get_ref())?;
                self.dirty = false;
            }
            Ok(())
        }
    }

    impl Drop for File {
        fn drop(&mut self) {
            let _ = self.flush();
        }
    }
}
