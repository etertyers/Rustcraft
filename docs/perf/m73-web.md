# m73-web performance

3 flights of 60 s (seed 20111118, render distance 9) in Chrome 157.0.8103.50 on the Mac mini/MacBook the sessions run on (web (Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/157.0.0.0 Safari/537.); median and spread (max − min as % of the median). Row `web-vanilla`, style `vanilla`, shaders `off`.

| Measure | Median | Spread |
|---|---|---|
| average_fps | 59.96 | 0.0% |
| chunk_generation_ms | 0.00 | 0.0% |
| chunks_generated | 0.00 | 0.0% |
| chunks_meshed | 1519.00 | 14.8% |
| cpu_acquire_ms | 0.02 | 11.3% |
| cpu_chunks_ms | 1.94 | 20.0% |
| cpu_draw_ms | 0.86 | 9.4% |
| cpu_egui_ms | 0.09 | 10.1% |
| cpu_encode_ms | 0.40 | 7.5% |
| cpu_game_ms | 1.84 | 3.8% |
| cpu_present_ms | 0.01 | 9.2% |
| cpu_submit_ms | 0.04 | 5.1% |
| frames | 3599.00 | 0.0% |
| frames_over_33ms | 1.00 | 100.0% |
| frames_over_50ms | 1.00 | 0.0% |
| gpu_hand_ms | 1.61 | 0.4% |
| gpu_total_ms | 3.86 | 1.4% |
| gpu_ui_ms | 1.56 | 0.3% |
| gpu_world_ms | 3.79 | 1.3% |
| light_mesh_ms | 5.36 | 2.2% |
| memory_mib | 804.56 | 0.0% |
| missed_frame_pct | 0.06 | 50.0% |
| one_percent_low_fps | 52.91 | 0.5% |
| refresh_hz | 60.00 | 0.0% |
| worst_frame_ms | 63.80 | 25.7% |
| worst frame over all runs (ms) | 76.40 | |

## Gate (Kenzo's: §7 rule 12 applied to the browser) — measured, partly failing

Chrome 157 (headed, real GPU, WebGPU) on the Mac the sessions run on, 1920×1080, render distance 9, **Vanilla style, Shaders off, Render Scale Auto, Distant Terrain off**, audio on, `?perf-test` flown by `web/test/perf.mjs` (three fresh page loads, 60 s after the 5 s warm-up each). The display ran at 60 Hz and the browser presents with Fifo (vsync), so no frame rate above 60 is possible here.

| Gate clause | Result | Runs |
|---|---|---|
| average 60+ FPS | **59.96** (at the 60 Hz cap) | 60.0 / 60.0 / 59.9 |
| 1% low 40+ FPS | **52.9, pass** | 52.9 / 53.2 / 52.9 |
| no frame over 50 ms after 5 s | **FAIL: exactly one per run** | worst 60.0 / 63.8 / 76.4 ms |

Per frame the game costs about 4.6 ms of CPU (game 1.84, chunks 1.94, draw 0.86 ms) and 3.9 ms of GPU, so throughput has headroom; vsync, not work, sets the 60. The one slow frame per run is the failing clause.

### What the slow frame is (diagnosed, not fixed)
- It happens once per run, at about the same point of the flight (roughly 8 s after warm-up), 55–95 ms long. The first frame of a run (338 ms) is start-up and is inside the warm-up.
- The wasm heap doesn't grow in it (804 MiB before and after): not `memory.grow`.
- Muting audio (`?mute`) lowers it (73 → 58 ms in one trial) but doesn't remove it: sound decoding is part of it, not all.
- A Chrome trace (`web/test/trace.mjs`) shows the frame's `requestAnimationFrame` callback running 65–72 ms with the main thread *idle* in the CPU profile (365 of 420 samples), while the game's own phases for that frame add up to about 17 ms. So the page is waiting inside the frame (on the browser's GPU process or the compositor), not computing. The V8 tier-up compile tasks (`wasm.TopTierCompilation`, hundreds of ms on background threads) are not on the main thread.
- Tried: `wasm-opt -O3` (already on), `target-feature=+simd128` (no measurable change: 59.9 FPS, same hitch). Not tried: mesher workers and an upload time budget (M72's unbuilt items), which would lower the main-thread work around chunk bursts; a GPU-buffer pool for chunk uploads (the likely wait).

## Rows reported without a gate (one flight each)

| Row | average FPS | 1% low | worst frame | over 33 / 50 ms | gpu_total ms | cpu game+chunks+draw ms |
|---|---|---|---|---|---|---|
| Vanilla, Shaders off (the gate row, median of 3) | 59.96 | 52.9 | 63.8 | 1 / 1 | 3.86 | 4.64 |
| Barebones, Shaders Low | 59.93 | 53.2 | 91.0 | 2 / 1 | 6.24 | 5.08 |
| Barebones, Shaders Medium | 59.93 | 53.2 | 70.6 | 3 / 1 | 8.36 | 4.67 |

Barebones used Kenzo's packs from `assets/downloads/resourcepacks` loaded into a throwaway test profile (never committed, never in `target/web/`). All three sit on the 60 Hz vsync cap; GPU time per frame (3.9 / 6.2 / 8.4 ms) is the better headroom number, and Medium is right at a 120 Hz frame budget.

**Distant Terrain at 32 and 128: not measurable.** Its tile makers need worker threads and the browser build only starts workers for chunk generation, so it makes no tiles there yet; it stays Off (`docs/DECISIONS.md`).

Not gated and not measured: `chunk_generation_ms` is 0 here because generation runs in the generator workers, outside the manager's own timer (`chunks_generated` is 0 for the same reason); `light_mesh_ms` (5.3 ms per chunk on the main thread, about 1.8× native's 3 ms) is the heaviest main-thread cost.
