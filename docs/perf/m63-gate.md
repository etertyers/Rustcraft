
### docs/perf/m63-barebones-high.json

| Measure | Baseline | Now | Change | Limit | Result |
|---|---|---|---|---|---|
| average_fps | 59.84 | 59.68 | -0.3% | >= 95% | pass |
| one_percent_low_fps | 39.27 | 39.60 | +0.8% | >= 95% | pass |
| chunk_generation_ms | 8.20 | 7.85 | -4.2% | <= 105% | pass |
| light_mesh_ms | 8.28 | 8.12 | -1.9% | <= 105% | pass |
| memory_mib | 680.02 | 714.95 | +5.1% | <= 110% | pass |
| bench: 100 ticks of redstone lines | 2.80 | 2.77 | -0.8% | <= 105% | pass |
| bench: 100 ticks of spreading water | 3.18 | 3.27 | +2.8% | <= 105% | pass |
| bench: generate a chunk | 2.73 | 2.77 | +1.6% | <= 105% | pass |
| bench: generate a nether chunk | 1.39 | 1.38 | -0.6% | <= 105% | pass |
| bench: generate an end chunk | 0.04 | 0.04 | +0.1% | <= 105% | pass |
| bench: generate an end city chunk | 0.72 | 0.72 | -0.0% | <= 105% | pass |
| bench: light a chunk | 1.17 | 1.23 | +5.7% | <= 105% | FAIL |
| bench: mesh a chunk | 2.14 | 1.88 | -12.4% | <= 105% | pass |
| frames_over_50ms | 0 | 0.00 |  | <= 0 | pass |
| budget: average_fps |  | 59.68 |  | min 60 | FAIL |
| budget: frames_over_50ms |  | 0.00 |  | max 0 | pass |
| budget: gpu_extra_ms |  | 7.10 |  | max 7.5 | pass |
| budget: one_percent_low_fps |  | 39.60 |  | min 40 | FAIL |

FAIL: bench: light a chunk, budget: average_fps, budget: one_percent_low_fps

### docs/perf/m63-barebones-low.json

| Measure | Baseline | Now | Change | Limit | Result |
|---|---|---|---|---|---|
| average_fps | 93.36 | 91.85 | -1.6% | >= 95% | pass |
| one_percent_low_fps | 57.36 | 54.35 | -5.2% | >= 95% | FAIL |
| chunk_generation_ms | 7.90 | 8.02 | +1.5% | <= 105% | pass |
| light_mesh_ms | 9.02 | 8.51 | -5.7% | <= 105% | pass |
| memory_mib | 605.03 | 710.25 | +17.4% | <= 110% | FAIL |
| bench: 100 ticks of redstone lines | 2.80 | 2.77 | -0.8% | <= 105% | pass |
| bench: 100 ticks of spreading water | 3.18 | 3.27 | +2.8% | <= 105% | pass |
| bench: generate a chunk | 2.73 | 2.77 | +1.6% | <= 105% | pass |
| bench: generate a nether chunk | 1.39 | 1.38 | -0.6% | <= 105% | pass |
| bench: generate an end chunk | 0.04 | 0.04 | +0.1% | <= 105% | pass |
| bench: generate an end city chunk | 0.72 | 0.72 | -0.0% | <= 105% | pass |
| bench: light a chunk | 1.17 | 1.23 | +5.7% | <= 105% | FAIL |
| bench: mesh a chunk | 2.14 | 1.88 | -12.4% | <= 105% | pass |
| frames_over_50ms | 0 | 0.00 |  | <= 0 | pass |
| budget: average_fps |  | 91.85 |  | min 100 | FAIL |
| budget: frames_over_50ms |  | 0.00 |  | max 0 | pass |
| budget: gpu_extra_ms |  | 2.27 |  | max 2.0 | FAIL |
| budget: one_percent_low_fps |  | 54.35 |  | min 50 | pass |

FAIL: one_percent_low_fps, memory_mib, bench: light a chunk, budget: average_fps, budget: gpu_extra_ms

### docs/perf/m63-barebones-medium.json

| Measure | Baseline | Now | Change | Limit | Result |
|---|---|---|---|---|---|
| average_fps | 77.64 | 77.66 | +0.0% | >= 95% | pass |
| one_percent_low_fps | 47.67 | 47.42 | -0.5% | >= 95% | pass |
| chunk_generation_ms | 7.91 | 7.97 | +0.8% | <= 105% | pass |
| light_mesh_ms | 8.65 | 7.89 | -8.9% | <= 105% | pass |
| memory_mib | 579.50 | 715.25 | +23.4% | <= 110% | FAIL |
| bench: 100 ticks of redstone lines | 2.80 | 2.77 | -0.8% | <= 105% | pass |
| bench: 100 ticks of spreading water | 3.18 | 3.27 | +2.8% | <= 105% | pass |
| bench: generate a chunk | 2.73 | 2.77 | +1.6% | <= 105% | pass |
| bench: generate a nether chunk | 1.39 | 1.38 | -0.6% | <= 105% | pass |
| bench: generate an end chunk | 0.04 | 0.04 | +0.1% | <= 105% | pass |
| bench: generate an end city chunk | 0.72 | 0.72 | -0.0% | <= 105% | pass |
| bench: light a chunk | 1.17 | 1.23 | +5.7% | <= 105% | FAIL |
| bench: mesh a chunk | 2.14 | 1.88 | -12.4% | <= 105% | pass |
| frames_over_50ms | 0 | 0.00 |  | <= 0 | pass |
| budget: average_fps |  | 77.66 |  | min 75 | pass |
| budget: frames_over_50ms |  | 0.00 |  | max 0 | pass |
| budget: gpu_extra_ms |  | 4.18 |  | max 4.5 | pass |
| budget: one_percent_low_fps |  | 47.42 |  | min 45 | pass |

FAIL: memory_mib, bench: light a chunk

### docs/perf/m63-barebones.json

| Measure | Baseline | Now | Change | Limit | Result |
|---|---|---|---|---|---|
| budget: average_fps |  | 123.69 |  | min 111.4 | pass |
| budget: frames_over_50ms |  | 0.00 |  | max 0 | pass |

PASS

### docs/perf/m63-realistic-medium.json

| Measure | Baseline | Now | Change | Limit | Result |
|---|---|---|---|---|---|
| budget: average_fps |  | 77.80 |  | min 75 | pass |
| budget: frames_over_50ms |  | 0.00 |  | max 0 | pass |
| budget: gpu_extra_ms |  | 4.20 |  | max 4.5 | pass |
| budget: one_percent_low_fps |  | 47.13 |  | min 45 | pass |

PASS

### docs/perf/m63-realistic.json

| Measure | Baseline | Now | Change | Limit | Result |
|---|---|---|---|---|---|
| budget: average_fps |  | 123.31 |  | min 111.4 | pass |
| budget: frames_over_50ms |  | 0.00 |  | max 0 | pass |

PASS

### docs/perf/m63-vanilla-medium.json

| Measure | Baseline | Now | Change | Limit | Result |
|---|---|---|---|---|---|
| budget: average_fps |  | 77.99 |  | min 75 | pass |
| budget: frames_over_50ms |  | 0.00 |  | max 0 | pass |
| budget: gpu_extra_ms |  | 4.18 |  | max 4.5 | pass |
| budget: one_percent_low_fps |  | 45.67 |  | min 45 | pass |

PASS

### docs/perf/m63.json

| Measure | Baseline | Now | Change | Limit | Result |
|---|---|---|---|---|---|
| average_fps | 123.80 | 123.76 | -0.0% | >= 79% | pass |
| one_percent_low_fps | 61.77 | 57.79 | -6.4% | >= 46% | pass |
| chunk_generation_ms | 8.07 | 7.92 | -2.0% | <= 105% | pass |
| light_mesh_ms | 9.08 | 8.67 | -4.4% | <= 105% | pass |
| memory_mib | 678.34 | 626.89 | -7.6% | <= 110% | pass |
| bench: 100 ticks of redstone lines | 2.80 | 2.77 | -0.8% | <= 105% | pass |
| bench: 100 ticks of spreading water | 3.18 | 3.27 | +2.8% | <= 105% | pass |
| bench: generate a chunk | 2.73 | 2.77 | +1.6% | <= 105% | pass |
| bench: generate a nether chunk | 1.39 | 1.38 | -0.6% | <= 105% | pass |
| bench: generate an end chunk | 0.04 | 0.04 | +0.1% | <= 105% | pass |
| bench: generate an end city chunk | 0.72 | 0.72 | -0.0% | <= 105% | pass |
| bench: light a chunk | 1.17 | 1.23 | +5.7% | <= 105% | FAIL |
| bench: mesh a chunk | 2.14 | 1.88 | -12.4% | <= 105% | pass |
| frames_over_50ms | 0 | 0.00 |  | <= 0 | pass |
| budget: one_percent_low_fps |  | 57.79 |  | min 40 | pass |

FAIL: bench: light a chunk

FAIL (see above)
