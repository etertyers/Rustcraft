
### docs/perf/m64-barebones-high.json

| Measure | Baseline | Now | Change | Limit | Result |
|---|---|---|---|---|---|
| average_fps | 59.84 | 59.33 | -0.9% | >= 95% | pass |
| one_percent_low_fps | 39.27 | 37.77 | -3.8% | >= 95% | pass |
| chunk_generation_ms | 8.20 | 7.88 | -3.9% | <= 105% | pass |
| light_mesh_ms | 8.28 | 8.09 | -2.3% | <= 105% | pass |
| memory_mib | 680.02 | 696.45 | +2.4% | <= 110% | pass |
| bench: 100 ticks of redstone lines | 2.80 | 2.77 | -0.8% | <= 105% | pass |
| bench: 100 ticks of spreading water | 3.18 | 3.27 | +2.8% | <= 105% | pass |
| bench: generate a chunk | 2.73 | 2.77 | +1.6% | <= 105% | pass |
| bench: generate a nether chunk | 1.39 | 1.38 | -0.6% | <= 105% | pass |
| bench: generate an end chunk | 0.04 | 0.04 | +0.1% | <= 105% | pass |
| bench: generate an end city chunk | 0.72 | 0.72 | -0.0% | <= 105% | pass |
| bench: light a chunk | 1.17 | 1.23 | +5.7% | <= 105% | FAIL |
| bench: mesh a chunk | 2.14 | 1.88 | -12.4% | <= 105% | pass |
| frames_over_50ms | 0 | 0.00 |  | <= 0 | pass |
| budget: average_fps |  | 59.33 |  | min 60 | FAIL |
| budget: frames_over_50ms |  | 0.00 |  | max 0 | pass |
| budget: gpu_extra_ms |  | 7.13 |  | max 7.5 | pass |
| budget: one_percent_low_fps |  | 37.77 |  | min 40 | FAIL |

FAIL: bench: light a chunk, budget: average_fps, budget: one_percent_low_fps

### docs/perf/m64-barebones-low.json

| Measure | Baseline | Now | Change | Limit | Result |
|---|---|---|---|---|---|
| average_fps | 93.36 | 91.42 | -2.1% | >= 95% | pass |
| one_percent_low_fps | 57.36 | 54.13 | -5.6% | >= 95% | FAIL |
| chunk_generation_ms | 7.90 | 8.06 | +2.0% | <= 105% | pass |
| light_mesh_ms | 9.02 | 8.27 | -8.3% | <= 105% | pass |
| memory_mib | 605.03 | 708.78 | +17.1% | <= 110% | FAIL |
| bench: 100 ticks of redstone lines | 2.80 | 2.77 | -0.8% | <= 105% | pass |
| bench: 100 ticks of spreading water | 3.18 | 3.27 | +2.8% | <= 105% | pass |
| bench: generate a chunk | 2.73 | 2.77 | +1.6% | <= 105% | pass |
| bench: generate a nether chunk | 1.39 | 1.38 | -0.6% | <= 105% | pass |
| bench: generate an end chunk | 0.04 | 0.04 | +0.1% | <= 105% | pass |
| bench: generate an end city chunk | 0.72 | 0.72 | -0.0% | <= 105% | pass |
| bench: light a chunk | 1.17 | 1.23 | +5.7% | <= 105% | FAIL |
| bench: mesh a chunk | 2.14 | 1.88 | -12.4% | <= 105% | pass |
| frames_over_50ms | 0 | 0.00 |  | <= 0 | pass |
| budget: average_fps |  | 91.42 |  | min 100 | FAIL |
| budget: frames_over_50ms |  | 0.00 |  | max 0 | pass |
| budget: gpu_extra_ms |  | 2.30 |  | max 2.0 | FAIL |
| budget: one_percent_low_fps |  | 54.13 |  | min 50 | pass |

FAIL: one_percent_low_fps, memory_mib, bench: light a chunk, budget: average_fps, budget: gpu_extra_ms

### docs/perf/m64-barebones-medium.json

| Measure | Baseline | Now | Change | Limit | Result |
|---|---|---|---|---|---|
| average_fps | 77.64 | 77.36 | -0.4% | >= 95% | pass |
| one_percent_low_fps | 47.67 | 45.91 | -3.7% | >= 95% | pass |
| chunk_generation_ms | 7.91 | 7.92 | +0.2% | <= 105% | pass |
| light_mesh_ms | 8.65 | 7.87 | -9.0% | <= 105% | pass |
| memory_mib | 579.50 | 715.00 | +23.4% | <= 110% | FAIL |
| bench: 100 ticks of redstone lines | 2.80 | 2.77 | -0.8% | <= 105% | pass |
| bench: 100 ticks of spreading water | 3.18 | 3.27 | +2.8% | <= 105% | pass |
| bench: generate a chunk | 2.73 | 2.77 | +1.6% | <= 105% | pass |
| bench: generate a nether chunk | 1.39 | 1.38 | -0.6% | <= 105% | pass |
| bench: generate an end chunk | 0.04 | 0.04 | +0.1% | <= 105% | pass |
| bench: generate an end city chunk | 0.72 | 0.72 | -0.0% | <= 105% | pass |
| bench: light a chunk | 1.17 | 1.23 | +5.7% | <= 105% | FAIL |
| bench: mesh a chunk | 2.14 | 1.88 | -12.4% | <= 105% | pass |
| frames_over_50ms | 0 | 0.00 |  | <= 0 | pass |
| budget: average_fps |  | 77.36 |  | min 75 | pass |
| budget: frames_over_50ms |  | 0.00 |  | max 0 | pass |
| budget: gpu_extra_ms |  | 4.20 |  | max 4.5 | pass |
| budget: one_percent_low_fps |  | 45.91 |  | min 45 | pass |

FAIL: memory_mib, bench: light a chunk

### docs/perf/m64-barebones.json

| Measure | Baseline | Now | Change | Limit | Result |
|---|---|---|---|---|---|
| budget: average_fps |  | 124.08 |  | min 111.4 | pass |
| budget: frames_over_50ms |  | 0.00 |  | max 0 | pass |

PASS

### docs/perf/m64-realistic-medium.json

| Measure | Baseline | Now | Change | Limit | Result |
|---|---|---|---|---|---|
| budget: average_fps |  | 77.25 |  | min 75 | pass |
| budget: frames_over_50ms |  | 0.00 |  | max 0 | pass |
| budget: gpu_extra_ms |  | 4.24 |  | max 4.5 | pass |
| budget: one_percent_low_fps |  | 45.71 |  | min 45 | pass |

PASS

### docs/perf/m64-realistic.json

| Measure | Baseline | Now | Change | Limit | Result |
|---|---|---|---|---|---|
| budget: average_fps |  | 121.86 |  | min 111.4 | pass |
| budget: frames_over_50ms |  | 0.00 |  | max 0 | pass |

PASS

### docs/perf/m64-vanilla-medium.json

| Measure | Baseline | Now | Change | Limit | Result |
|---|---|---|---|---|---|
| budget: average_fps |  | 77.46 |  | min 75 | pass |
| budget: frames_over_50ms |  | 0.00 |  | max 0 | pass |
| budget: gpu_extra_ms |  | 4.20 |  | max 4.5 | pass |
| budget: one_percent_low_fps |  | 45.30 |  | min 45 | pass |

PASS

### docs/perf/m64.json

| Measure | Baseline | Now | Change | Limit | Result |
|---|---|---|---|---|---|
| average_fps | 123.80 | 124.38 | +0.5% | >= 79% | pass |
| one_percent_low_fps | 61.77 | 58.31 | -5.6% | >= 46% | pass |
| chunk_generation_ms | 8.07 | 7.77 | -3.8% | <= 105% | pass |
| light_mesh_ms | 9.08 | 8.57 | -5.6% | <= 105% | pass |
| memory_mib | 678.34 | 638.22 | -5.9% | <= 110% | pass |
| bench: 100 ticks of redstone lines | 2.80 | 2.77 | -0.8% | <= 105% | pass |
| bench: 100 ticks of spreading water | 3.18 | 3.27 | +2.8% | <= 105% | pass |
| bench: generate a chunk | 2.73 | 2.77 | +1.6% | <= 105% | pass |
| bench: generate a nether chunk | 1.39 | 1.38 | -0.6% | <= 105% | pass |
| bench: generate an end chunk | 0.04 | 0.04 | +0.1% | <= 105% | pass |
| bench: generate an end city chunk | 0.72 | 0.72 | -0.0% | <= 105% | pass |
| bench: light a chunk | 1.17 | 1.23 | +5.7% | <= 105% | FAIL |
| bench: mesh a chunk | 2.14 | 1.88 | -12.4% | <= 105% | pass |
| frames_over_50ms | 0 | 0.00 |  | <= 0 | pass |
| budget: one_percent_low_fps |  | 58.31 |  | min 40 | pass |

FAIL: bench: light a chunk

FAIL (see above)
