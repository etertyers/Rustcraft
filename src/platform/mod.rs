//! The platform layer (M70). Everything that differs between the native
//! game and the browser build lives behind the small modules here, so game
//! code has no `cfg(target_arch = "wasm32")`:
//!
//! - [`storage`] and [`fs`]: files (native: `std::fs`; browser: a
//!   [`storage::Storage`] backend, OPFS from M72),
//! - [`task`]: background work (native: threads; browser: workers, M72),
//! - [`time`]: `Instant` and friends (browser: `web-time`),
//! - [`logging`]: `log` output (native: env_logger; browser: the console),
//! - [`env`]: arguments, variables and exiting,
//! - [`window`]: the window (a canvas in the browser) and the async GPU request,
//! - [`assets`]: loading read-only assets (native: synchronous; browser:
//!   asynchronous behind a loading screen, M71).

pub mod assets;
pub mod env;
pub mod fs;
pub mod layers;
pub mod logging;
pub mod storage;
pub mod task;
pub mod time;
pub mod window;

/// Whether this is the browser build.
pub const WEB: bool = cfg!(target_arch = "wasm32");

/// Whether Open to LAN and Multiplayer exist (they need sockets).
pub const LAN: bool = !WEB;
