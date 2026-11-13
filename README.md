# Rustcraft

A Minecraft Java Edition (1.21.x) clone in Rust and wgpu, for macOS on Apple silicon. See `CLAUDE.md` for the rules, `docs/milestones/` for what was built, `docs/architecture.md` for the file map.

- **Mac app:** `cargo run --release`, or `scripts/bundle-macos.sh` for `Rustcraft.app`.
- **web:** the same game in a browser (Chrome, Edge, Safari, Firefox; Mac and Windows). `scripts/build-web.sh`, then `scripts/serve-web.sh`; on first start it asks for Minecraft's own files and keeps them in the browser. How to build, serve and play, what is left out, the keys browsers keep, and hosting: `docs/web.md`.
