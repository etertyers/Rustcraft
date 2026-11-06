# M68: shader speed (measured on a Mac mini M1, 1080p at 60 Hz, not the M4)

This session ran on the M1 (Macmini9,1), as Kenzo chose when asked. The M4 targets (native resolution, 120 Hz ProMotion, "20% less GPU than M67") could not be measured here, so M68 is **not marked complete**.

## Step 0
- Surface: Fifo with vsync on, Immediate off (the surface offers `[Fifo, Immediate]`), frame latency 2. Drawable 1920×1080 = window 1920×1080 at scale factor 1; winit reports 60.0 Hz.
- Barebones + Shaders Medium, 1080p flight: 79–80 FPS. CPU per frame: game 0.80, chunks 0.14, egui 0.08, acquire 0.01, encode 0.38, submit 1.64, **GPU wait 9.0 ms**. The frame is GPU-bound; the CPU work is about 3 ms.
- The harness draws off screen when the screen is locked (all frames "off screen" here), so it waits for the GPU each frame; a shown window overlaps CPU and GPU.
- The profiler's per-pass times overlap on this tile GPU (their sum is several times the frame), so cost was measured by turning each effect off (`RUSTCRAFT_EFFECTS_OFF`), frame wait in ms (all on 8.9):

| Off | Wait | Saved |
|---|---|---|
| Reflections (screen-space march, 16 steps) | 7.81 | 1.1 |
| Reflections + refraction | 8.06 | 0.85 |
| Shadows | 8.64 | 0.35 |
| Bloom, FXAA, light shafts (together) | 8.74 | 0.17 |
| Shaded clouds, cloud shadows, mob shadows, fog, sway, wetness, leaf light | each | under 0.1 |
| Every effect except shadows | 7.64 | 1.3 |
| Every effect and shadows (HDR pipeline alone) | 6.95 | |
| Shaders off (Barebones) | 5.13 | |
| Vanilla | 4.98 | |

The hand pass costs 0.1 ms. At render distance 1 the frame is 4.6 ms with shaders (2.5 vanilla): a fixed floor from the post chain. Render Scale 80/60/50% gives 88/90/93 FPS (8.9 → 8.3/7.8/7.5 ms): only about 1.5 ms is per pixel; the rest is geometry and fixed cost.

## What M68 added
Render Scale (Auto, 50–100%, Shaders group; Auto is 100% at 1080p, 75% at 3024×1964, 50% at 4K), drawn small and upscaled with a sharp filter before the UI (a 60% capture differs from 100% by 1.5/255 on average); explicit present modes; `--perf-test --native` and `--vsync`; the missed-frame share; the CPU split in every report. Env aids: `RUSTCRAFT_RENDER_SCALE`, `RUSTCRAFT_EFFECTS_OFF`.

## What is left (not done)
- GPU time at Medium 1080p is unchanged from M67 (7.99 vs 8.06 ms span; 80 vs 78.8 FPS). The 20% target needs ~1.8 ms; the biggest single item is the reflections march (1.1 ms). Cheaper marching (view-space depth compare instead of world-from-depth per step) changes where reflections hit, so it was not done without Kenzo's say.
- Native resolution and 120 Hz: here the display is 1080p/60, and a `--native` flight (fullscreen, 76 FPS, all frames off screen) says nothing about the M4. No native gate row was added; it needs the M4.
- Mesh uploads already have a per-frame count budget (`MAX_RESULTS_PER_FRAME`). No blocking readbacks exist (a test checks the profiler polls only and that the renderer waits only for captures and off-screen frames).

# Second session: the M4's files, the bundle bug, Medium reflections

## The M4 (docs/perf/m4-info.txt, m4-native-vsync-on.txt, m4-native-vsync-off.txt, m4-1080p-all.txt)
MacBook Air, Apple M4 (Mac16,12), 2560×1664 Retina. **winit reports 60.0 Hz: this is not a ProMotion screen** (the Air has none), so the milestone's "120 FPS" target does not apply to this Mac; its refresh rate is 60.

| Native, full screen, Barebones + Shaders Medium | vsync on (Fifo) | vsync off (Immediate) |
|---|---|---|
| Drawable | 2940×1846 (window = drawable, scale factor 2) | same |
| Render Scale Auto picked | 75% (2940×1846 is 5.4 M pixels; 75% is 3.1 M) | same |
| Average FPS | 60 (3594 frames in 60 s) | 143 (8598 frames) |
| 1% low | 51 | 60 |
| Frames over 33 ms / worst | 3 / 49.7 ms | 2 / 91.6 ms |
| CPU per frame | game 0.77, chunks 0.11, encode 0.46, submit 1.80; waiting for the drawable 12.96 | game 0.16, chunks 0.03, encode 0.15, submit 0.70; waiting 5.70 |
| Missed-frame share | not in the file (the run printed the console summary only); at most 0.08% were over 33 ms | n/a |

- **Does it reach its refresh rate with vsync on?** Yes: 60 of 60 Hz. It is not GPU-bound (vsync off gives 143 FPS, 7.0 ms per frame, so vsync-on frames use about 40% of the 16.7 ms), not CPU-bound (about 3 ms of CPU work; the rest of the frame is the wait for the next drawable) and the present mode is correct (Fifo). **It is capped by the display.** Kenzo's "50–60 FPS" is the 60 Hz screen plus a 1% low of 51 (a few late frames; the worst frame spends 3.6 ms in draw and 3.0 in game, so the late frames are waiting on presentation, not working).
- The vsync-off native run shows the headroom: 143 FPS average at 75% scale.
- **m4-1080p-all.txt is not a 1080p measurement.** Every row, Vanilla with shaders off included, reads exactly 60.0 FPS, and the drawable is 2940×1678–1682, not 1920×1080. The bench asked winit for a *logical* 1920×1080 window; on a scale-2 display that is 3840×2160 pixels, which macOS clamps to the screen. So these rows ran at about 5 M pixels (Auto picked 75% for the shaders-on rows, 100% for off), in a window the compositor holds to the refresh rate even in Immediate mode, so the FPS says nothing about GPU speed. The profiler's GPU times in that file include the wait for the drawable (Vanilla "gpu_total" 10 ms against 3.9 on the mini) and can't be compared with the mini's.

## What changed
- `fix(m68)`: assets only from `--assets`, `$RUSTCRAFT_ASSETS`, the bundle, or an `assets/` above the executable; no compile-time path (the cause of the M4 "reading /Users/kenzo/emdash/...mobs.ron" error). `scripts/check-bundle.sh` checks the binary for the repo path and runs a copy of the app with the repo's `assets/` hidden.
- `perf(m68)`: Medium's screen-space reflections: 12 steps (was 16), the ray stepped in clip space, a view-depth hit test, 3 refine steps, no march where Fresnel is under 3.5%. High is unchanged. It is not a half-resolution pass: the reflection is computed in the water's fragment shader, and a real half-resolution pass would need a separate buffer of water positions and normals.
- The bench window is 1920×1080 **pixels** (was logical), so "1080p" is 1080p on a scale-2 Mac. This only changes `--bench` and `--perf-test`; the game window is unchanged. Nothing else limits the M4: it already runs at the display's refresh rate.
- `perf_gate.py` accepts a per-row `machine`; new row `barebones-native` (below).

## GPU time at Medium 1080p (Barebones + Shaders Medium, Macmini9,1)
| | Before (fb92c99) | After (3826713) |
|---|---|---|
| gpu_total_ms (profiler, flight median) | 7.99 | 7.56 (−5.4%) |
| gpu_extra_ms over Vanilla | 4.11 | 3.68 |
| Frame wait in `--bench 20` | 8.82 ms | 8.40 ms |
| Average FPS / 1% low | 80.0 / 47.4 | 82.3 / 46.8 |

The reflections cost 1.1 ms; about 0.4 ms of that is gone. M68's 20% target (about 1.8 ms) is **not met**: what is left is 0.7 ms of reflections and the HDR post chain's fixed 1.8 ms (see Step 0).

## The native gate row
`docs/perf/baseline.json` row `barebones-native`, machine Mac16,12 (the M4). The Mac mini can't measure it, and `perf_gate.py` refuses to compare it with the mini's results (exit 2), so it is checked on the M4 only. Reference: m4-native-vsync-on.txt, 60 FPS average, 1% low 51. Budgets: average ≥ 57 (95% of the 60 Hz refresh), 1% low ≥ 48 (95% of 51), missed frames ≤ 1% (M68's own target; the first M4 run with this build is the first time it is measured).

## The table (1080p on the mini, 60 s flights, 3 runs; average FPS / 1% low)
| Style | Shaders off | Shaders on (Medium) |
|---|---|---|
| Vanilla | 130.0 / 63.5 | 82.0 / 48.5 |
| Barebones | 128.5 / 65.7 | 82.3 / 46.8 |
| Realistic | 128.7 / 62.1 | 81.9 / 47.9 |

Barebones + Shaders Medium on the M4 at native resolution (Auto, 75%), from Kenzo's files: vsync on 60 / 51 (the display's refresh), vsync off 143 / 60.

Gate: Vanilla-medium and Realistic-medium pass; Barebones-medium passes every row but `memory_mib` (+22.5%), the known baseline-draw issue in docs/perf/memory-note.md (the earlier commit read 712 MiB too).

## Known issue: a flaky test
`server::tests::two_clients_agree_on_a_villagers_and_a_zombies_animation_variables` fails with "only 0 ticks could be compared" (src/server.rs). In this session it failed in 7 of 10 full `cargo test --lib` runs (including once on the branch with my changes stashed) and passed in all 4 runs of that test alone. I read the failing test's name in three of the seven; the others only showed one failure. It depends on the other tests running; not investigated.
