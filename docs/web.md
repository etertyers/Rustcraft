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

`assets.zip` holds the repo's own assets only (`assets/` without `assets/vanilla/` and `assets/downloads/`).

**Default build (M75).** `scripts/build-web.sh` also puts this clone's own Minecraft files and packs in `target/web/builtin/`, so the page opens straight to the title screen with the Minecraft font, the menu music and Barebones as the default style (Shaders stay off, the web gate's configuration). It takes them from `assets/vanilla` (the game's files only: never `data/` or `server/`) and `assets/downloads`; if the clone's files are incomplete it first fills them with the native downloader and no window (`cargo run --release -- --download-only`), so a fresh clone needs only `scripts/build-web.sh`, and it stops with the downloader's one-line reason if that fails. `target/` is git-ignored: those files are never committed. **Don't host this build publicly**; it holds Mojang's files and the packs.

**`--lite`.** `scripts/build-web.sh --lite` builds without them; on first start a setup screen asks for Minecraft's files (the `assets/vanilla` folder, or a `.zip` or client `.jar`), checks them like `scripts/check-vanilla.sh` does, and keeps them in the browser's private storage (OPFS). Nothing is uploaded anywhere. Packs can be added there too. This is the build to host: `scripts/check-web-output.sh --lite` verifies it holds nothing of Mojang's. The setup screen is also the fallback when a default build's built-in files fail to load (it says why in one line).

### How the built-in files load (measured)

Built-in layers (`scripts/web_builtin.py`; `builtin/manifest.json` lists them with sizes and a stamp of the content): **564 MB in 23 files**, of which the browser needs **35 MB before the title screen**:

| Layer | Size | Files | When |
|---|---|---|---|
| Minecraft's core files + menu music (`vanilla-core.zip`, deflated) | 18.7 MB | 11,958 (12.6 MB raw without sounds) | before the title screen |
| The 4 Barebones packs | 7.0 MB | 4 | before the title screen |
| The 8 small mod jars (the extras read particle sprites at atlas build, footstep sounds when played) | 9.2 MB | 8 | before the title screen |
| Gameplay sounds, records, music (`vanilla-sounds-N.zip`, stored) | 377 MB | 5,330 | in the background, most needed first |
| Patrix 32x (Realistic) | 76.0 MB | 1 | in the background |
| AmbientSounds jar | 84.8 MB | 1 | in the background |

Not built in: `assets/vanilla/data` (world-gen JSON, read by a test only) and `server/`. Mod jars are included because the extras do read them on the web (`extras::jar`: particle sprites when the atlas is built, footsteps and ambient sounds when played).

Measured in Chrome on the M1 Mac mini over localhost (`web/test/load-times.mjs`):

| Approach | Title screen, first load | Title screen, reload |
|---|---|---|
| **Boot layers only, the rest in the background, archives cached in OPFS (chosen)** | **1.3 s** (35 MB fetched; the rest done after 2.9 s) | **1.1 s** (0 fetched; the rest done after 1.9 s) |
| Everything before the title screen (564 MB) | 3.2 s | 2.0 s |
| Old: Minecraft's files stored in OPFS from the folder picker, unzipped at every start | 9.4 s once to pick and store | 1.9 s |
| One request per file, the 11,531 core files (12.6 MB), localhost only | 3.9 s at 6 at a time, 2.5 s at 24 | (HTTP cache) |

Why this one: boot-only loading is the fastest to the title screen on both loads and keeps working while the other 529 MB arrives; archives beat one request per file because the files are small and many (11,531 requests, each paying a round trip: free on localhost, not over Wi-Fi) and the game reads files synchronously from memory, so on-demand fetching of individual files would need every reader made asynchronous. OPFS (not the HTTP cache) holds the archives because it is keyed by the build's stamp, so a rebuild with different files (a new stamp in the manifest) fetches again and deletes the old cache, while a rebuild of the same files keeps it; the builds are deterministic.

**Estimate over Wi-Fi to the M4 (not measured):** at an assumed 25 MB/s (about 200 Mbit/s) the 35 MB boot fetch is about 1.4 s on top of about 1 s of unpacking (the M4 is no slower than the M1): **roughly 2.5 s to the title screen on the first load, about 1.1 s on reloads**; the remaining 529 MB would take about 20 s in the background on the first visit. At 6 MB/s it would be about 7 s and 90 s. Per-file fetching would add about 11,531 ÷ 6 round trips (about 6 s at 3 ms each) to that.

Sounds arrive in the background, so a sound used before its layer is in plays silently once (and a music track waits 30 s before the next try); the menu music and the UI clicks are in the boot layer. Unpacking runs a few milliseconds at a time so no frame stalls. `?eager` loads everything first (only for measuring), `?nobackground` skips the background layers (the perf tests use it).

## What is left out, and why

| Left out | Why |
|---|---|
| Open to LAN, Multiplayer | A page can't open sockets. |
| Lua mods, Fullbright | `mlua` needs a C toolchain for wasm; the Mods screen says "Lua mods aren't available in the browser version" and still routes resource packs and data packs. |
| `--perf-test --native`, `--texture-audit`, `--lan-test`, `--shot` | Native dev tools. The Texture Viewer stays. |
| Quit Game | A page has no program to quit. |

## Window size and mouse look

- **The page sizes the canvas, the game follows.** The canvas is CSS `100vw × 100vh` (no scrollbars); the game never asks winit for a window size on the web (that writes a fixed inline style that overrides the CSS). winit's `ResizeObserver` reports the canvas in device pixels (CSS size × `devicePixelRatio`), the renderer reconfigures the surface and every render target on each `Resized`, and Render Scale Auto works on that buffer as on the Mac (the UI stays at full size). Browser zoom, window resizing and fullscreen are all the same event.
- **Mouse look** reads raw pointer-lock movement (`requestPointerLock({unadjustedMovement: true})`, with a plain lock as the fallback); every motion event turns the camera at once by `movement × 0.0025` rad, the same formula as the Mac app. Clicking the canvas locks the pointer, Esc releases it and opens the pause menu. Audio starts at "Click to start", not on the first click in the world: creating the browser's `AudioContext` blocks the page for 200 to 600 ms on an M1, which used to freeze the camera at the first click.
- `?mute` keeps the audio engine off (for measuring).

## Keys the browser keeps

The browser keeps Cmd+W / Ctrl+W (close tab), Cmd+Q, Ctrl+T, Cmd+R / Ctrl+R and the like; Rustcraft doesn't fight them. Losing the pointer lock (Esc, switching tab) opens the pause menu, like Minecraft does when the window loses focus. In fullscreen on Chromium the Keyboard Lock API lets Esc and the F-keys through. F11 is the browser's fullscreen.

## Reaching it from another computer (the Windows PC on the same Wi-Fi)

Browsers give a page the storage (OPFS) and the GPU (WebGPU) the game needs only on `https://` or `http://localhost`. So `http://<the Mac's address>:8080/` does **not** work from another computer. Use `scripts/serve-web.sh --https`: it makes a self-signed certificate in `target/web-cert/` and prints `https://<the Mac's address>:8080/`; open that on the PC, click through the one certificate warning ("Advanced", "Proceed"), and pick Minecraft's files at the setup screen. The loader says so if the page isn't on a secure address.

## The server in its own worker (M76, opt-in)

`?server-worker` (with `?creative`, `?world=Name&seed=N`, or Create/Open in the menu) starts the world's server in a Web Worker with its own wasm instance: `web/worker.js` `server` kind, `src/web/server_worker.rs`. The page is only the client (the same protocol as a LAN guest, over a `MessagePort`). The worker needs just `assets.zip` (no Minecraft files or packs): block models with placeholder texture rects are enough for lighting and shapes. It owns the generator workers, and is the only writer of `saves/` in OPFS (synchronous access handles). Tests: `web/test/server-worker*.mjs`. Not the default yet (see `docs/milestones/v1.6.md`, M76).

## What runs where today

- The page is the client: winit on a canvas, wgpu on WebGPU (WebGL2 is requested as the fallback, but untested), egui, kira audio, input.
- The server (`server.rs`, the game tick) still runs **in-process on the page's main thread** ("single thread mode"). Chunk generation goes to generator Web Workers (`min(3, hardwareConcurrency − 2)`, at least 1); lighting and meshing run on the main thread inside a 4 ms budget per update. The in-process path is also what `?single-thread` forces (it turns the generator workers off).
- Worlds save to the browser's private storage (OPFS) and come back after a reload; closing or hiding the tab asks the game to save. Worlds export and import as `.zip` from Select World, with the same folder layout as the Mac app's `saves/`.
- Not built yet (v1.6 known gaps): the server in its own Web Worker, mesher workers, the mesh-upload time budget, frustum-first chunk requests, the render distance ramp, OPFS sync access handles, Distant Terrain in the browser.

## Speed (Chrome on the M4's Mac, Vanilla, Shaders off, render distance 9, 1080p)

`web/test/perf.mjs` flies the same scripted path as the native `--perf-test`: average 60.0 FPS (the 60 Hz display's cap), 1% low 52.9, about 4.6 ms of CPU and 3.9 ms of GPU per frame. One frame per run is 60–76 ms (the page waits inside the frame), which misses the "no frame over 50 ms" rule. Details, the Barebones rows and the diagnosis: `docs/perf/m73-web.md`.

## Testing

`web/test/` has Playwright scripts (`npm install` there once; they drive your installed Chrome, headed): `smoke.mjs`, `play.mjs`, `persist.mjs` (save, reload, reopen), `exchange.mjs` (world zips between browser and Mac app), `perf.mjs` (the gate), `trace.mjs` (long tasks and a CPU profile of a flight), `check-output.mjs` (the build refuses Mojang's files), `look-test.mjs` (the first click causes no long task; mouse moves turn the camera by exactly `movement × 0.0025` rad within two frames, with none dropped), `resize.mjs` and `resize-real.mjs` (the canvas, its drawing buffer and the wgpu surface follow viewport and window resizes), `look.mjs` and `profile-click.mjs` (the probes that found the click stall). Chrome under automation can't be granted a real pointer lock or enter fullscreen, so those two are manual checks.

## Hosting later

Any static host works: nothing needs special headers (no SharedArrayBuffer, no COOP/COEP). It must serve `https`, `.wasm` as `application/wasm`, and only what `scripts/build-web.sh --lite` puts in `target/web/` (the repo's own assets, never Mojang's files, the packs or the mod jars; `scripts/check-web-output.sh --lite` checks). Each visitor supplies their own Minecraft files at the setup screen, which stay in their browser. A default build holds the files the clone has: for your own machine and network only. Nothing here has been deployed anywhere.
