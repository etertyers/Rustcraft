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
    ARGS.lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
        .into_iter()
}

#[cfg(target_arch = "wasm32")]
static ARGS: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

/// The browser's "command line", from the page's query (see `query_args`).
#[cfg(target_arch = "wasm32")]
pub fn set_args(args: Vec<String>) {
    *ARGS.lock().unwrap_or_else(|e| e.into_inner()) = args;
}

/// The page's query string as command-line arguments, program name first:
/// `?creative&world=My%20World&seed=5` is `creative --world "My World" --seed 5`
/// (`--creative --world "My World" --seed 5`). `single-thread` and other
/// page-only keys are left to the page.
pub fn query_args(query: &str) -> Vec<String> {
    fn decode(s: &str) -> String {
        let mut out = Vec::new();
        let b = s.as_bytes();
        let mut i = 0;
        while i < b.len() {
            match b[i] {
                b'+' => out.push(b' '),
                b'%' if i + 2 < b.len() => match u8::from_str_radix(&s[i + 1..i + 3], 16) {
                    Ok(v) => {
                        out.push(v);
                        i += 2;
                    }
                    Err(_) => out.push(b'%'),
                },
                c => out.push(c),
            }
            i += 1;
        }
        String::from_utf8_lossy(&out).into_owned()
    }
    let mut args = vec!["rustcraft".to_owned()];
    for part in query
        .trim_start_matches('?')
        .split('&')
        .filter(|p| !p.is_empty())
    {
        let (key, value) = part.split_once('=').unwrap_or((part, ""));
        let key = decode(key);
        if PAGE_ONLY.contains(&key.as_str()) {
            continue;
        }
        args.push(format!("--{key}"));
        if !value.is_empty() {
            args.push(decode(value));
        }
    }
    args
}

/// Query keys the page itself uses, which are not game flags.
const PAGE_ONLY: &[&str] = &["single-thread", "webgl", "debug"];

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_query_becomes_flags() {
        assert_eq!(query_args(""), ["rustcraft"]);
        assert_eq!(
            query_args("?creative&world=My%20World&seed=5&single-thread"),
            [
                "rustcraft",
                "--creative",
                "--world",
                "My World",
                "--seed",
                "5"
            ]
        );
        assert_eq!(
            query_args("?perf-test&style=vanilla+x"),
            ["rustcraft", "--perf-test", "--style", "vanilla x"]
        );
        assert_eq!(query_args("?a=100%"), ["rustcraft", "--a", "100%"]);
    }
}
