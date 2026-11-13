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

## Window size and mouse look

- **The page sizes the canvas, the game follows.** The canvas is CSS `100vw × 100vh` (no scrollbars); the game never asks winit for a window size on the web (that writes a fixed inline style that overrides the CSS). winit's `ResizeObserver` reports the canvas in device pixels (CSS size × `devicePixelRatio`), the renderer reconfigures the surface and every render target on each `Resized`, and Render Scale Auto works on that buffer as on the Mac (the UI stays at full size). Browser zoom, window resizing and fullscreen are all the same event.
- **Mouse look** reads raw pointer-lock movement (`requestPointerLock({unadjustedMovement: true})`, with a plain lock as the fallback); every motion event turns the camera at once by `movement × 0.0025` rad, the same formula as the Mac app. Clicking the canvas locks the pointer, Esc releases it and opens the pause menu. Audio starts at "Click to start", not on the first click in the world: creating the browser's `AudioContext` blocks the page for 200 to 600 ms on an M1, which used to freeze the camera at the first click.
- `?mute` keeps the audio engine off (for measuring).

## Keys the browser keeps

The browser keeps Cmd+W / Ctrl+W (close tab), Cmd+Q, Ctrl+T, Cmd+R / Ctrl+R and the like; Rustcraft doesn't fight them. Losing the pointer lock (Esc, switching tab) opens the pause menu, like Minecraft does when the window loses focus. In fullscreen on Chromium the Keyboard Lock API lets Esc and the F-keys through. F11 is the browser's fullscreen.

## Reaching it from another computer (the Windows PC on the same Wi-Fi)

Browsers give a page the storage (OPFS) and the GPU (WebGPU) the game needs only on `https://` or `http://localhost`. So `http://<the Mac's address>:8080/` does **not** work from another computer. Use `scripts/serve-web.sh --https`: it makes a self-signed certificate in `target/web-cert/` and prints `https://<the Mac's address>:8080/`; open that on the PC, click through the one certificate warning ("Advanced", "Proceed"), and pick Minecraft's files at the setup screen. The loader says so if the page isn't on a secure address.

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

Any static host works: nothing needs special headers (no SharedArrayBuffer, no COOP/COEP). It must serve `https`, `.wasm` as `application/wasm`, and only what `scripts/build-web.sh` puts in `target/web/` (the repo's own assets, never Mojang's files, the packs or the mod jars; `scripts/check-web-output.sh` checks). Each visitor supplies their own Minecraft files at the setup screen, which stay in their browser. Nothing here has been deployed anywhere.
