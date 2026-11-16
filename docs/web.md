# Rustcraft in the browser

The web build runs the same game in Chrome, Edge, Safari and Firefox on Mac and Windows (v1.6, M70–M73). It is the same crate compiled to `wasm32-unknown-unknown`; differences between a computer and a browser live in `src/platform/` and `src/web/`.

## Build, serve, play

```bash
scripts/build-web.sh      # target/web/: wasm, index.html, loader.js, assets.zip
scripts/serve-web.sh      # serves target/web/ and prints the URLs
```

Needs `wasm-bindgen-cli` 0.2.129 (`cargo install wasm-bindgen-cli --version 0.2.129 --locked`) and `wasm-opt` (`brew install binaryen`). Open `http://localhost:8080/`.

Page options (the native flags as a query): `?creative`, `?world=Name&seed=5`, `?distance=9`, `?style=vanilla`, `?shaders=off`, `?perf-test`. `?single-thread` keeps everything on the main thread (from M72).

## Assets: what is and isn't in the build

`assets.zip` holds the repo's own assets only (`assets/` without `assets/vanilla/` and `assets/downloads/`). `scripts/build-web.sh` refuses to copy Mojang's files, Kenzo's packs or the mod jars and `scripts/check-web-output.sh` checks the output for them.

On first start a setup screen asks for Minecraft's files (the `assets/vanilla` folder from `scripts/fetch-vanilla-assets.sh`, or a `.zip` or client `.jar`), checks them like `scripts/check-vanilla.sh` does, and keeps them in the browser's private storage (OPFS). Nothing is uploaded anywhere. Resource packs can be added there too (optional); they are stored as the built-in packs the Mac app has. Until Minecraft's files are loaded the game cannot start; with them, the default is the Vanilla style with Shaders off.

## What is left out, and why

| Left out | Why |
|---|---|
| Open to LAN, Multiplayer | A page can't open sockets. |
| Lua mods, Fullbright | `mlua` needs a C toolchain for wasm; the Mods screen says "Lua mods aren't available in the browser version" and still routes resource packs and data packs. |
| `--perf-test --native`, `--texture-audit`, `--lan-test`, `--shot` | Native dev tools. The Texture Viewer stays. |
| Quit Game | A page has no program to quit. |

## Keys the browser keeps

The browser keeps Cmd+W / Ctrl+W (close tab), Cmd+Q, Ctrl+T, Cmd+R / Ctrl+R and the like; Rustcraft doesn't fight them. Losing the pointer lock (Esc, switching tab) opens the pause menu, like Minecraft does when the window loses focus. In fullscreen on Chromium the Keyboard Lock API lets Esc and the F-keys through. F11 is the browser's fullscreen.

## Status

M71: the in-process server on the main thread (single thread mode). M72 moves the server, generators and meshers into Web Workers and saves worlds in OPFS. M73 measures speed.
