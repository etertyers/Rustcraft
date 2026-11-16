# Rustcraft

A Minecraft Java Edition (1.21.x) clone in Rust and wgpu, for macOS on Apple silicon. See `CLAUDE.md` for the rules, `docs/milestones/` for what was built, `docs/architecture.md` for the file map.

- **Mac app:** `cargo run --release`, or `scripts/bundle-macos.sh` for `Rustcraft.app`.
- **web:** the same game in a browser (Chrome, Edge, Safari, Firefox; Mac and Windows). `scripts/build-web.sh`, then `scripts/serve-web.sh`; on first start it asks for Minecraft's own files and keeps them in the browser. How to build, serve and play, what is left out, the keys browsers keep, and hosting: `docs/web.md`.

## Setup

```bash
git clone https://github.com/etertyers/Rustcraft.git
cd Rustcraft
cargo run --release
```

Press the big **Download game files** button when the game opens its first screen. Minecraft's textures, sounds and font (from Mojang's servers) and the style packs and mod jars (from Modrinth) go into this clone's own `assets/vanilla` and `assets/downloads`, about 600 MB. It can be cancelled and resumed, and every file is checksummed. The game starts by itself when it is done, with no restart.

- Every clone has its own copy, so a new clone shows the screen again. If another copy already exists on this Mac, the main button reads **Copy from this Mac** instead: that is instant and works offline. Download is then the second choice.
- **Only Minecraft's files** (the small link) skips the packs for this clone, so it doesn't ask again. **Options → Game Files** can still get them.
- The screen also comes back whenever a file is missing from this clone, before the title screen.
- Nothing is committed to this repository or hosted anywhere else. **Options → Game Files** shows what is installed, verifies it and downloads it again.

The scripts are for CI and developers only; the screen does the same job:

```bash
scripts/fetch-vanilla-assets.sh      # Mojang's files into assets/vanilla (also the world-gen data the tests read)
scripts/check-vanilla.sh             # is assets/vanilla complete?
scripts/check-downloads.sh           # are the 5 packs and 9 mod jars (assets/data/downloads.ron) in assets/downloads?
cargo run --release -- --no-download   # never show the screen (CI, tools)
cargo run --release -- --download-only # no window: fill this clone's assets/vanilla and assets/downloads (Copy from this Mac when possible, else download), print progress, exit 0 when complete, non-zero with the reason if not
```

## Building the app

```bash
scripts/bundle-macos.sh          # Rustcraft.app with everything inside (needs assets/vanilla and assets/downloads)
scripts/bundle-macos.sh --lite   # Rustcraft.app without Mojang's files, packs or jars; downloads them on first launch into `~/Library/Application Support/Rustcraft/assets` (the app's Resources are read-only)
```
