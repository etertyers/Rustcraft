
### docs/perf/m69.json

| Measure | Baseline | Now | Change | Limit | Result |
|---|---|---|---|---|---|
| average_fps | 123.80 | 127.27 | +2.8% | >= 79% | pass |
| one_percent_low_fps | 61.77 | 65.54 | +6.1% | >= 46% | pass |
| chunk_generation_ms | 8.07 | 8.11 | +0.4% | <= 105% | pass |
| light_mesh_ms | 9.08 | 8.68 | -4.4% | <= 105% | pass |
| memory_mib | 678.34 | 698.80 | +3.0% | <= 110% | pass |
| bench: 100 ticks of redstone lines | 2.80 | 2.92 | +4.2% | <= 105% | pass |
| bench: 100 ticks of spreading water | 3.18 | 3.48 | +9.2% | <= 105% | FAIL |
| bench: generate a chunk | 2.73 | 2.70 | -1.1% | <= 105% | pass |
| bench: generate a nether chunk | 1.39 | 1.37 | -1.2% | <= 105% | pass |
| bench: generate an end chunk | 0.04 | 0.03 | -7.7% | <= 105% | pass |
| bench: generate an end city chunk | 0.72 | 0.72 | -0.1% | <= 105% | pass |
| bench: light a chunk | 1.17 | 1.25 | +6.7% | <= 105% | FAIL |
| bench: mesh a chunk | 2.14 | 1.88 | -12.0% | <= 105% | pass |
| frames_over_50ms | 0 | 0.00 |  | <= 0 | pass |
| budget: one_percent_low_fps |  | 65.54 |  | min 40 | pass |

FAIL: bench: 100 ticks of spreading water, bench: light a chunk

### docs/perf/m69-barebones.json

| Measure | Baseline | Now | Change | Limit | Result |
|---|---|---|---|---|---|
| budget: average_fps |  | 126.81 |  | min 111.4 | pass |
| budget: frames_over_50ms |  | 0.00 |  | max 0 | pass |

PASS

### docs/perf/m69-barebones-low.json

| Measure | Baseline | Now | Change | Limit | Result |
|---|---|---|---|---|---|
| average_fps | 93.36 | 95.38 | +2.2% | >= 95% | pass |
| one_percent_low_fps | 57.36 | 56.58 | -1.4% | >= 95% | pass |
| chunk_generation_ms | 7.90 | 8.06 | +2.0% | <= 105% | pass |
| light_mesh_ms | 9.02 | 8.31 | -7.9% | <= 105% | pass |
| memory_mib | 605.03 | 690.80 | +14.2% | <= 110% | FAIL |
| bench: 100 ticks of redstone lines | 2.80 | 2.92 | +4.2% | <= 105% | pass |
| bench: 100 ticks of spreading water | 3.18 | 3.48 | +9.2% | <= 105% | FAIL |
| bench: generate a chunk | 2.73 | 2.70 | -1.1% | <= 105% | pass |
| bench: generate a nether chunk | 1.39 | 1.37 | -1.2% | <= 105% | pass |
| bench: generate an end chunk | 0.04 | 0.03 | -7.7% | <= 105% | pass |
| bench: generate an end city chunk | 0.72 | 0.72 | -0.1% | <= 105% | pass |
| bench: light a chunk | 1.17 | 1.25 | +6.7% | <= 105% | FAIL |
| bench: mesh a chunk | 2.14 | 1.88 | -12.0% | <= 105% | pass |
| frames_over_50ms | 0 | 0.00 |  | <= 0 | pass |
| budget: average_fps |  | 95.38 |  | min 100 | FAIL |
| budget: frames_over_50ms |  | 0.00 |  | max 0 | pass |
| budget: gpu_extra_ms |  | 2.14 |  | max 2.0 | FAIL |
| budget: one_percent_low_fps |  | 56.58 |  | min 50 | pass |

FAIL: memory_mib, bench: 100 ticks of spreading water, bench: light a chunk, budget: average_fps, budget: gpu_extra_ms

### docs/perf/m69-barebones-medium.json

| Measure | Baseline | Now | Change | Limit | Result |
|---|---|---|---|---|---|
| average_fps | 77.64 | 83.50 | +7.5% | >= 95% | pass |
| one_percent_low_fps | 47.67 | 49.20 | +3.2% | >= 95% | pass |
| chunk_generation_ms | 7.91 | 8.01 | +1.4% | <= 105% | pass |
| light_mesh_ms | 8.65 | 8.32 | -3.9% | <= 105% | pass |
| memory_mib | 579.50 | 712.56 | +23.0% | <= 110% | FAIL |
| bench: 100 ticks of redstone lines | 2.80 | 2.92 | +4.2% | <= 105% | pass |
| bench: 100 ticks of spreading water | 3.18 | 3.48 | +9.2% | <= 105% | FAIL |
| bench: generate a chunk | 2.73 | 2.70 | -1.1% | <= 105% | pass |
| bench: generate a nether chunk | 1.39 | 1.37 | -1.2% | <= 105% | pass |
| bench: generate an end chunk | 0.04 | 0.03 | -7.7% | <= 105% | pass |
| bench: generate an end city chunk | 0.72 | 0.72 | -0.1% | <= 105% | pass |
| bench: light a chunk | 1.17 | 1.25 | +6.7% | <= 105% | FAIL |
| bench: mesh a chunk | 2.14 | 1.88 | -12.0% | <= 105% | pass |
| frames_over_50ms | 0 | 0.00 |  | <= 0 | pass |
| budget: average_fps |  | 83.50 |  | min 75 | pass |
| budget: frames_over_50ms |  | 0.00 |  | max 0 | pass |
| budget: gpu_extra_ms |  | 3.65 |  | max 4.5 | pass |
| budget: one_percent_low_fps |  | 49.20 |  | min 45 | pass |

FAIL: memory_mib, bench: 100 ticks of spreading water, bench: light a chunk

### docs/perf/m69-barebones-high.json

| Measure | Baseline | Now | Change | Limit | Result |
|---|---|---|---|---|---|
| average_fps | 59.84 | 59.76 | -0.1% | >= 95% | pass |
| one_percent_low_fps | 39.27 | 39.36 | +0.2% | >= 95% | pass |
| chunk_generation_ms | 8.20 | 8.02 | -2.3% | <= 105% | pass |
| light_mesh_ms | 8.28 | 7.86 | -5.1% | <= 105% | pass |
| memory_mib | 680.02 | 717.17 | +5.5% | <= 110% | pass |
| bench: 100 ticks of redstone lines | 2.80 | 2.92 | +4.2% | <= 105% | pass |
| bench: 100 ticks of spreading water | 3.18 | 3.48 | +9.2% | <= 105% | FAIL |
| bench: generate a chunk | 2.73 | 2.70 | -1.1% | <= 105% | pass |
| bench: generate a nether chunk | 1.39 | 1.37 | -1.2% | <= 105% | pass |
| bench: generate an end chunk | 0.04 | 0.03 | -7.7% | <= 105% | pass |
| bench: generate an end city chunk | 0.72 | 0.72 | -0.1% | <= 105% | pass |
| bench: light a chunk | 1.17 | 1.25 | +6.7% | <= 105% | FAIL |
| bench: mesh a chunk | 2.14 | 1.88 | -12.0% | <= 105% | pass |
| frames_over_50ms | 0 | 0.00 |  | <= 0 | pass |
| budget: average_fps |  | 59.76 |  | min 60 | FAIL |
| budget: frames_over_50ms |  | 0.00 |  | max 0 | pass |
| budget: gpu_extra_ms |  | 7.17 |  | max 7.5 | pass |
| budget: one_percent_low_fps |  | 39.36 |  | min 40 | FAIL |

FAIL: bench: 100 ticks of spreading water, bench: light a chunk, budget: average_fps, budget: one_percent_low_fps

### docs/perf/m69-realistic.json

| Measure | Baseline | Now | Change | Limit | Result |
|---|---|---|---|---|---|
| budget: average_fps |  | 128.00 |  | min 111.4 | pass |
| budget: frames_over_50ms |  | 0.00 |  | max 0 | pass |

PASS

### docs/perf/m69-realistic-medium.json

| Measure | Baseline | Now | Change | Limit | Result |
|---|---|---|---|---|---|
| budget: average_fps |  | 82.14 |  | min 75 | pass |
| budget: frames_over_50ms |  | 0.00 |  | max 0 | pass |
| budget: gpu_extra_ms |  | 3.71 |  | max 4.5 | pass |
| budget: one_percent_low_fps |  | 48.35 |  | min 45 | pass |

PASS

### docs/perf/m69-vanilla-medium.json

| Measure | Baseline | Now | Change | Limit | Result |
|---|---|---|---|---|---|
| budget: average_fps |  | 84.17 |  | min 75 | pass |
| budget: frames_over_50ms |  | 0.00 |  | max 0 | pass |
| budget: gpu_extra_ms |  | 3.59 |  | max 4.5 | pass |
| budget: one_percent_low_fps |  | 50.85 |  | min 45 | pass |

PASS

### docs/perf/m69-lod-128.json

| Measure | Baseline | Now | Change | Limit | Result |
|---|---|---|---|---|---|
| average_fps | 79.20 | 79.20 | +0.0% | >= 95% | pass |
| one_percent_low_fps | 47.94 | 47.94 | +0.0% | >= 95% | pass |
| chunk_generation_ms | 7.86 | 7.86 | +0.0% | <= 105% | pass |
| memory_mib | 936.69 | 936.69 | +0.0% | <= 110% | pass |
| frames_over_50ms | 0 | 0.00 |  | <= 0 | pass |
| budget: average_fps |  | 79.20 |  | min 75.8 | pass |
| budget: chunk_generation_ms |  | 7.86 |  | max 9.05 | pass |
| budget: memory_mib |  | 936.69 |  | max 1738.390625 | pass |
| budget: one_percent_low_fps |  | 47.94 |  | min 45.8 | pass |

PASS

### docs/perf/m69-lod-256.json

| Measure | Baseline | Now | Change | Limit | Result |
|---|---|---|---|---|---|
| average_fps | 79.41 | 79.41 | +0.0% | >= 95% | pass |
| one_percent_low_fps | 46.79 | 46.79 | +0.0% | >= 95% | pass |
| chunk_generation_ms | 7.90 | 7.90 | +0.0% | <= 105% | pass |
| memory_mib | 931.22 | 931.22 | +0.0% | <= 110% | pass |
| frames_over_50ms | 0 | 0.00 |  | <= 0 | pass |
| budget: average_fps |  | 79.41 |  | min 75.8 | pass |
| budget: chunk_generation_ms |  | 7.90 |  | max 9.05 | pass |
| budget: memory_mib |  | 931.22 |  | max 1738.390625 | pass |
| budget: one_percent_low_fps |  | 46.79 |  | min 45.8 | pass |

PASS

### docs/perf/m69-lod-512.json

| Measure | Baseline | Now | Change | Limit | Result |
|---|---|---|---|---|---|
| average_fps | 79.82 | 79.82 | +0.0% | >= 95% | pass |
| one_percent_low_fps | 47.04 | 47.04 | +0.0% | >= 95% | pass |
| chunk_generation_ms | 7.84 | 7.84 | +0.0% | <= 105% | pass |
| memory_mib | 768.72 | 768.72 | +0.0% | <= 110% | pass |
| frames_over_50ms | 0 | 0.00 |  | <= 0 | pass |
| budget: average_fps |  | 79.82 |  | min 75.8 | pass |
| budget: chunk_generation_ms |  | 7.84 |  | max 9.05 | pass |
| budget: memory_mib |  | 768.72 |  | max 1738.390625 | pass |
| budget: one_percent_low_fps |  | 47.04 |  | min 45.8 | pass |

PASS

FAIL (see above)

## Reading the gate
Failures are the ones M65 and M66 already carried: the "spreading water" and "light a chunk" benchmarks (+9% and +6%, benchmarks that never touch LOD), Barebones Low and Medium memory, and Low and High's budgets by a few percent. Chunk generation and meshing are unchanged (-0.3% and -12%), and Distant Terrain is off in every one of these rows. The three new rows (`barebones-medium-distant128/256/512`) pass; their baselines are their own first measurement, so their budgets (docs/perf/m69-notes.md) are the real check. Rows ran once (`--runs 1`) except the Vanilla row (3 runs).
