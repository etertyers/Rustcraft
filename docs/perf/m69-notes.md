# M69: Distant Terrain, measured on the Mac mini M1 (not the M4)

Barebones + Shaders Medium, 1080p window (the bench window is 1920×1080 pixels; the drawable here is 1920×939), render distance 9, a 60 s flight on seed 20111118, one run per row, `RUSTCRAFT_DISTANT=<chunks>`. "Off" is the same flight with Distant Terrain off. The profiler's pass times overlap on this tile GPU, so the frame time (1000 / FPS) is the honest cost; the profiler's `world` pass is shown beside it.

| | Off | 128 chunks | 256 chunks | 512 chunks |
|---|---|---|---|---|
| Average FPS (1% low) | 85.5 (48) | 79.2 (48) | 79.4 (47) | 79.8 (47) |
| Frame time | 11.70 ms | 12.63 ms (+0.93) | 12.59 ms (+0.89) | 12.53 ms (+0.83) |
| GPU `world` pass (profiler) | 3.58 ms | 4.09 (+0.51) | 4.12 (+0.54) | 4.12 (+0.54) |
| Main-thread CPU: the "chunks" slot, which now holds the LOD update | 0.09 ms | 0.15 ms (+0.06) | 0.14 ms (+0.05) | 0.15 ms (+0.06) |
| Process memory | 714 MiB | 937 MiB (+223) | 931 MiB (+217) | 769 MiB (+55; run-to-run noise, not a saving) |
| Nearby chunk generation | 7.87 ms | 7.86 ms (-0.1%) | 7.90 ms (+0.4%) | 7.84 ms (-0.4%) |
| Disk written by the server's tile store in the flight | 0 | 1.7 MB | 2.3 MB | 2.9 MB |

- Budgets (M69 entry; for the M4 at native resolution): at most 1.5 ms GPU: **met here** (+0.5 ms in the world pass, +0.9 ms in frame time). At most 0.3 ms main-thread CPU: **met** (+0.06 ms; before the idle-frame fix it was +0.8 to +1.4 ms, because the per-tile pass ran every frame). Chunk loading no more than 15% slower: **met** (within ±0.4%). Memory at most 1 GB: **met** (the largest LOD share seen is about 220 MiB; the flight only covers a strip of the land, so a long session that explores more keeps more tiles in the store but not in memory: only selected tiles are held).
- The cost barely depends on the distance because the quadtree keeps about 360 tiles at 128 chunks whatever the reach (each level doubles the distance), and far tiles are coarse. A flight of 60 s also cannot show the cost of making far tiles for unexplored land: the server's workers do that at low priority (6 threads here), 1,024 samples a tile.
- **The existing Barebones gate rows fail with Distant Terrain on by default** (85.5 to 79.2 FPS is -7.4% against a 95% floor), so `lod::ON_BY_DEFAULT` is false. Turning it on for Barebones and Realistic, as the entry says, needs Kenzo to accept that or the baseline row to move. The new rows `barebones-medium-distant128/256/512` carry the budgets above (FPS at least 1000 / (off frame time + 1.5 ms) = 75.8; 1% low at least 95% of off; memory at most 1 GB over off; chunk generation at most 115% of off) and pass.
- Not measured: the M4 at native resolution; the opaque chunk pass's GPU time at render distance 12; memory per loaded chunk (Step 0 asked for both). Chunk generation was 7.9 ms a chunk (`chunk_generation_ms`), render distance runs 2 to 16 in the options.
- Step 0's Modrinth page and docs were not fetched: the defaults are the ones in the session's brief (128 chunks, loaded and unexplored land, Medium, Auto threads).

## What was built
- `lod/data.rs`: tiles of 32×32 samples at levels 0–6 (1 to 64 blocks a sample); each sample up to four slices (ground, water, canopy, overhang) as block id + biome + foliage blend, so a client colors them from its own packs.
- `lod/sample.rs`: real chunks give exact finest-level heights (tests over 1,536 columns); unexplored land uses `Terrain::lod_surface` (the same noise, climate and surface rules, no carvers or structures; trees as canopy slices at 1–4 block samples, and a leaf-green tint at coarser levels). Heights match real ones wherever no cave opens the surface (every other column is checked to be a cave opening).
- `lod/store.rs`: region files in `saves/<world>/lod/`, oldest-used removed over a 1 GB cap.
- `lod/maker.rs` + `server.rs`: the server makes tiles and sends them (`LodRequest` / `LodTile`, protocol 6) at 256 KB/s a client; `lod/manager.rs`: a quadtree picks tiles, workers mesh them; `render/lod.rs` + `lod.wgsl`: one buffer pair a tile, drawn before the loaded chunks.
- Settings: Video Settings → Distant Terrain (Off, 32–512 chunks), Distant Quality, Distant Threads.

## Known gaps
- No dithered cross-fade: real chunks draw over the LOD (the hole is two chunks inside the loaded square and the LOD sits 1.5 blocks lower near it), so there are no holes or flicker, but the seam is a plain edge. A dither would need `shader.wgsl` changed.
- LOD shows terrain as it was generated: a block a player breaks or builds beyond the loaded area isn't reflected until its chunk is observed again, and sky light in slices is always 15.
- LOD doesn't cast shadows (the entry allows it only at High), and water reflections don't show it.
- Overhang slices (High only) use a thickness and gap rule that can also pick up thin cave roofs.
- A world's details don't show the LOD disk size yet.
