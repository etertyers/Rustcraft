# Rustcraft

A Minecraft Java Edition clone for macOS (Apple silicon), written in Rust with wgpu. See `CLAUDE.md` for what it is, its rules and its milestones.

## Setup

```bash
cargo run --release
```

That is all. The first time the game starts without Minecraft's textures, sounds and font, it opens a small download screen and fetches them from Mojang's servers (and, if you leave the box ticked, the Barebones and Realistic style packs and their mods from Modrinth) into `~/Library/Application Support/Rustcraft/assets`. About 600 MB; it can be cancelled and resumed, and every file is checksummed. Nothing is committed to this repository or hosted anywhere else. **Options → Game Files** shows what is installed, verifies it and downloads it again.

The scripts still work for developers and CI, and a complete `assets/vanilla` and `assets/downloads` in the repo (or the app's Resources) win over the downloaded copy:

```bash
scripts/fetch-vanilla-assets.sh      # Mojang's files into assets/vanilla (also the world-gen data the tests read)
scripts/check-vanilla.sh             # is assets/vanilla complete?
scripts/check-downloads.sh           # are the 5 packs and 9 mod jars (assets/data/downloads.ron) in assets/downloads?
cargo run --release -- --no-download   # never show the download screen (CI, tools)
```

## Building the app

```bash
scripts/bundle-macos.sh          # Rustcraft.app with everything inside (needs assets/vanilla and assets/downloads)
scripts/bundle-macos.sh --lite   # Rustcraft.app without Mojang's files, packs or jars; downloads them on first launch
```
