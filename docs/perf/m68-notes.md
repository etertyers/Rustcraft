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
