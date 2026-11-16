//! Arguments, environment variables, the executable and exiting. Native:
//! `std::env` and `std::process`. The browser has no command line, no
//! environment and no process: arguments come from the URL query
//! (`?creative`, `?perf-test`, ... from M71), variables are always unset,
//! and there is no executable (so the places that start the game again as a
//! child process get an error, as they would anywhere that can't).

use std::{ffi::OsString, io, path::PathBuf};

#[cfg(not(target_arch = "wasm32"))]
pub use std::env::VarError;

/// What `var` says when a variable isn't set (std's type on native).
#[cfg(target_arch = "wasm32")]
#[derive(Debug, PartialEq, Eq)]
pub enum VarError {
    NotPresent,
}

/// All the command-line arguments, program name first, like `std::env::args`.
#[cfg(not(target_arch = "wasm32"))]
pub fn args() -> std::env::Args {
    std::env::args()
}

#[cfg(target_arch = "wasm32")]
pub fn args() -> std::vec::IntoIter<String> {
    vec!["rustcraft".to_owned()].into_iter()
}

#[cfg(not(target_arch = "wasm32"))]
pub fn var(name: &str) -> Result<String, VarError> {
    std::env::var(name)
}

#[cfg(target_arch = "wasm32")]
pub fn var(_name: &str) -> Result<String, VarError> {
    Err(VarError::NotPresent)
}

#[cfg(not(target_arch = "wasm32"))]
pub fn var_os(name: &str) -> Option<OsString> {
    std::env::var_os(name)
}

#[cfg(target_arch = "wasm32")]
pub fn var_os(_name: &str) -> Option<OsString> {
    None
}

#[cfg(not(target_arch = "wasm32"))]
pub fn current_exe() -> io::Result<PathBuf> {
    std::env::current_exe()
}

#[cfg(target_arch = "wasm32")]
pub fn current_exe() -> io::Result<PathBuf> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "no executable in the browser",
    ))
}

/// A scratch folder name (`std::env::temp_dir` panics in the browser).
#[cfg(not(target_arch = "wasm32"))]
pub fn temp_dir() -> PathBuf {
    std::env::temp_dir()
}

#[cfg(target_arch = "wasm32")]
pub fn temp_dir() -> PathBuf {
    PathBuf::from("tmp")
}

/// The process id (`std::process::id` panics in the browser).
#[cfg(not(target_arch = "wasm32"))]
pub fn process_id() -> u32 {
    std::process::id()
}

#[cfg(target_arch = "wasm32")]
pub fn process_id() -> u32 {
    0
}

/// Ends the program. In the browser there is no process to end: the game
/// stops its loop instead, so this only logs and panics.
#[cfg(not(target_arch = "wasm32"))]
pub fn exit(code: i32) -> ! {
    std::process::exit(code)
}

#[cfg(target_arch = "wasm32")]
pub fn exit(code: i32) -> ! {
    log::error!("exit({code}) in the browser build");
    panic!("exit({code})");
}
