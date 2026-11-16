//! Rustcraft's native entry: the command line (`native.rs`). The browser
//! build has no `main`; its entry is `rustcraft::web` (M71).

#[cfg(not(target_arch = "wasm32"))]
mod native;

#[cfg(not(target_arch = "wasm32"))]
fn main() -> anyhow::Result<()> {
    native::main()
}

#[cfg(target_arch = "wasm32")]
fn main() {}
