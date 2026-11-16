//! First-launch downloads (M74): Mojang's vanilla files, and the style
//! packs and mod jars from Modrinth, fetched onto the player's own Mac
//! into a user asset layer (`~/Library/Application Support/Rustcraft/assets`).
//! Nothing here is committed, hosted or bundled.
//!
//! - [`manifest`]: what to fetch and from where (`assets/data/downloads.ron`).
//! - [`layers`]: which folder holds `vanilla/` and `downloads/`, and whether
//!   a copy is complete.
//! - [`vanilla`], [`modrinth`]: the two servers' formats.
//! - [`download`]: the worker (threads, progress, resume, checksums).
//! - [`screen`], [`window`]: the first-launch screen and its window.

pub mod download;
pub mod game_files;
pub mod layers;
pub mod manifest;
pub mod modrinth;
pub mod screen;
pub mod vanilla;
pub mod window;

/// Why a download did not finish. `Display` is the one-line reason the
/// screen shows.
#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum SetupError {
    #[error("Can't reach {host}. Is this Mac online? ({detail})")]
    Offline { host: String, detail: String },
    #[error("{url} answered with status {status}")]
    Status { url: String, status: u16 },
    #[error("{file} arrived damaged (expected {expected}, got {got}) after {attempts} tries")]
    Checksum {
        file: String,
        expected: String,
        got: String,
        attempts: u32,
    },
    #[error("{0}")]
    Io(String),
    #[error("Mojang's launcher files have no version {0}")]
    NoVersion(String),
    #[error("Modrinth no longer lists {0} (the pinned file is gone)")]
    ExtraGone(String),
    #[error("{0}")]
    Format(String),
    #[error("cancelled")]
    Cancelled,
}

impl SetupError {
    /// Whether trying again can help.
    pub fn retryable(&self) -> bool {
        match self {
            Self::Offline { .. } | Self::Checksum { .. } | Self::Io(_) => true,
            Self::Status { status, .. } => *status >= 500 || *status == 429,
            _ => false,
        }
    }
}

impl From<std::io::Error> for SetupError {
    fn from(err: std::io::Error) -> Self {
        Self::Io(err.to_string())
    }
}

pub type Result<T, E = SetupError> = std::result::Result<T, E>;

/// The first-launch screen, if this start needs it: this clone is missing
/// Minecraft's files or one of the pinned packs and jars (unless the player
/// chose "Only Minecraft's files"), and this isn't a tool or test run
/// (`--no-download`, `$RUSTCRAFT_NO_DOWNLOAD`, a bench or a shot). A clone
/// counts only its own `assets/vanilla` and `assets/downloads`; only the
/// .app falls back to the user layer. `$RUSTCRAFT_SETUP_BASE` points the
/// download at a local server (tests).
pub fn first_launch_screen(args: &[String], tool_run: bool) -> Option<screen::SetupScreen> {
    if tool_run
        || args.iter().any(|a| a == "--no-download")
        || std::env::var_os("RUSTCRAFT_NO_DOWNLOAD").is_some()
    {
        return None;
    }
    let assets = crate::assets::dir();
    let source = crate::assets::source();
    let user = layers::user_assets();
    let Some(needs) = layers::needs(&assets, source, user.as_deref()) else {
        // Complete: remember this clone for the next fresh one's "Copy".
        layers::remember_clone(&assets, source);
        return None;
    };
    log::info!(
        "{}: Minecraft's files {}, {} packs and mods missing; showing the setup screen",
        assets.display(),
        if needs.vanilla { "missing" } else { "present" },
        needs.extras.len()
    );
    let copy = layers::find_copy_source(
        &assets,
        needs.vanilla,
        &needs.extras,
        &layers::copy_candidates(),
    );
    let endpoints = match std::env::var("RUSTCRAFT_SETUP_BASE") {
        Ok(base) => manifest::Endpoints::local(&base),
        Err(_) => manifest::Endpoints::official(),
    };
    Some(screen::SetupScreen::new(
        endpoints,
        download::Policy::default(),
        needs,
        copy,
    ))
}

/// Called after the screen finished: records this clone as the last one
/// with complete files.
pub fn note_this_clone() {
    layers::remember_clone(&crate::assets::dir(), crate::assets::source());
}
