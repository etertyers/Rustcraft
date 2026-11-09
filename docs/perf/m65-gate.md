
### docs/perf/m65.json

| Measure | Baseline | Now | Change | Limit | Result |
|---|---|---|---|---|---|
| average_fps | 123.80 | 125.02 | +1.0% | >= 79% | pass |
| one_percent_low_fps | 61.77 | 61.99 | +0.4% | >= 46% | pass |
| chunk_generation_ms | 8.07 | 7.79 | -3.5% | <= 105% | pass |
| light_mesh_ms | 9.08 | 8.51 | -6.2% | <= 105% | pass |
| memory_mib | 678.34 | 695.25 | +2.5% | <= 110% | pass |
| bench: 100 ticks of redstone lines | 2.80 | 2.86 | +2.2% | <= 105% | pass |
| bench: 100 ticks of spreading water | 3.18 | 3.49 | +9.7% | <= 105% | FAIL |
| bench: generate a chunk | 2.73 | 2.74 | +0.4% | <= 105% | pass |
| bench: generate a nether chunk | 1.39 | 1.38 | -0.7% | <= 105% | pass |
| bench: generate an end chunk | 0.04 | 0.04 | +0.1% | <= 105% | pass |
| bench: generate an end city chunk | 0.72 | 0.72 | +0.2% | <= 105% | pass |
| bench: light a chunk | 1.17 | 1.23 | +5.6% | <= 105% | FAIL |
| bench: mesh a chunk | 2.14 | 1.87 | -13.0% | <= 105% | pass |
| frames_over_50ms | 0 | 0.00 |  | <= 0 | pass |
| budget: one_percent_low_fps |  | 61.99 |  | min 40 | pass |

FAIL: bench: 100 ticks of spreading water, bench: light a chunk

### docs/perf/m65-barebones-high.json

| Measure | Baseline | Now | Change | Limit | Result |
|---|---|---|---|---|---|
| average_fps | 59.84 | 59.19 | -1.1% | >= 95% | pass |
| one_percent_low_fps | 39.27 | 39.17 | -0.3% | >= 95% | pass |
| chunk_generation_ms | 8.20 | 8.09 | -1.4% | <= 105% | pass |
| light_mesh_ms | 8.28 | 7.84 | -5.3% | <= 105% | pass |
| memory_mib | 680.02 | 712.31 | +4.7% | <= 110% | pass |
| bench: 100 ticks of redstone lines | 2.80 | 2.86 | +2.2% | <= 105% | pass |
| bench: 100 ticks of spreading water | 3.18 | 3.49 | +9.7% | <= 105% | FAIL |
| bench: generate a chunk | 2.73 | 2.74 | +0.4% | <= 105% | pass |
| bench: generate a nether chunk | 1.39 | 1.38 | -0.7% | <= 105% | pass |
| bench: generate an end chunk | 0.04 | 0.04 | +0.1% | <= 105% | pass |
| bench: generate an end city chunk | 0.72 | 0.72 | +0.2% | <= 105% | pass |
| bench: light a chunk | 1.17 | 1.23 | +5.6% | <= 105% | FAIL |
| bench: mesh a chunk | 2.14 | 1.87 | -13.0% | <= 105% | pass |
| frames_over_50ms | 0 | 0.00 |  | <= 0 | pass |
| budget: average_fps |  | 59.19 |  | min 60 | FAIL |
| budget: frames_over_50ms |  | 0.00 |  | max 0 | pass |
| budget: gpu_extra_ms |  | 7.12 |  | max 7.5 | pass |
| budget: one_percent_low_fps |  | 39.17 |  | min 40 | FAIL |

FAIL: bench: 100 ticks of spreading water, bench: light a chunk, budget: average_fps, budget: one_percent_low_fps

### docs/perf/m65-barebones-low.json

| Measure | Baseline | Now | Change | Limit | Result |
|---|---|---|---|---|---|
| average_fps | 93.36 | 92.67 | -0.7% | >= 95% | pass |
| one_percent_low_fps | 57.36 | 53.98 | -5.9% | >= 95% | FAIL |
| chunk_generation_ms | 7.90 | 7.93 | +0.4% | <= 105% | pass |
| light_mesh_ms | 9.02 | 8.05 | -10.8% | <= 105% | pass |
| memory_mib | 605.03 | 701.03 | +15.9% | <= 110% | FAIL |
| bench: 100 ticks of redstone lines | 2.80 | 2.86 | +2.2% | <= 105% | pass |
| bench: 100 ticks of spreading water | 3.18 | 3.49 | +9.7% | <= 105% | FAIL |
| bench: generate a chunk | 2.73 | 2.74 | +0.4% | <= 105% | pass |
| bench: generate a nether chunk | 1.39 | 1.38 | -0.7% | <= 105% | pass |
| bench: generate an end chunk | 0.04 | 0.04 | +0.1% | <= 105% | pass |
| bench: generate an end city chunk | 0.72 | 0.72 | +0.2% | <= 105% | pass |
| bench: light a chunk | 1.17 | 1.23 | +5.6% | <= 105% | FAIL |
| bench: mesh a chunk | 2.14 | 1.87 | -13.0% | <= 105% | pass |
| frames_over_50ms | 0 | 0.00 |  | <= 0 | pass |
| budget: average_fps |  | 92.67 |  | min 100 | FAIL |
| budget: frames_over_50ms |  | 0.00 |  | max 0 | pass |
| budget: gpu_extra_ms |  | 2.24 |  | max 2.0 | FAIL |
| budget: one_percent_low_fps |  | 53.98 |  | min 50 | pass |

FAIL: one_percent_low_fps, memory_mib, bench: 100 ticks of spreading water, bench: light a chunk, budget: average_fps, budget: gpu_extra_ms

### docs/perf/m65-barebones-medium.json

| Measure | Baseline | Now | Change | Limit | Result |
|---|---|---|---|---|---|
| average_fps | 77.64 | 77.27 | -0.5% | >= 95% | pass |
| one_percent_low_fps | 47.67 | 46.34 | -2.8% | >= 95% | pass |
| chunk_generation_ms | 7.91 | 8.09 | +2.3% | <= 105% | pass |
| light_mesh_ms | 8.65 | 8.03 | -7.2% | <= 105% | pass |
| memory_mib | 579.50 | 708.33 | +22.2% | <= 110% | FAIL |
| bench: 100 ticks of redstone lines | 2.80 | 2.86 | +2.2% | <= 105% | pass |
| bench: 100 ticks of spreading water | 3.18 | 3.49 | +9.7% | <= 105% | FAIL |
| bench: generate a chunk | 2.73 | 2.74 | +0.4% | <= 105% | pass |
| bench: generate a nether chunk | 1.39 | 1.38 | -0.7% | <= 105% | pass |
| bench: generate an end chunk | 0.04 | 0.04 | +0.1% | <= 105% | pass |
| bench: generate an end city chunk | 0.72 | 0.72 | +0.2% | <= 105% | pass |
| bench: light a chunk | 1.17 | 1.23 | +5.6% | <= 105% | FAIL |
| bench: mesh a chunk | 2.14 | 1.87 | -13.0% | <= 105% | pass |
| frames_over_50ms | 0 | 0.00 |  | <= 0 | pass |
| budget: average_fps |  | 77.27 |  | min 75 | pass |
| budget: frames_over_50ms |  | 0.00 |  | max 0 | pass |
| budget: gpu_extra_ms |  | 4.20 |  | max 4.5 | pass |
| budget: one_percent_low_fps |  | 46.34 |  | min 45 | pass |

FAIL: memory_mib, bench: 100 ticks of spreading water, bench: light a chunk

### docs/perf/m65-barebones.json

| Measure | Baseline | Now | Change | Limit | Result |
|---|---|---|---|---|---|
| budget: average_fps |  | 122.41 |  | min 111.4 | pass |
| budget: frames_over_50ms |  | 0.00 |  | max 0 | pass |

PASS

### docs/perf/m65-realistic-medium.json

| Measure | Baseline | Now | Change | Limit | Result |
|---|---|---|---|---|---|
| budget: average_fps |  | 77.23 |  | min 75 | pass |
| budget: frames_over_50ms |  | 0.00 |  | max 0 | pass |
| budget: gpu_extra_ms |  | 4.18 |  | max 4.5 | pass |
| budget: one_percent_low_fps |  | 45.49 |  | min 45 | pass |

PASS

### docs/perf/m65-realistic.json

| Measure | Baseline | Now | Change | Limit | Result |
|---|---|---|---|---|---|
| budget: average_fps |  | 122.74 |  | min 111.4 | pass |
| budget: frames_over_50ms |  | 0.00 |  | max 0 | pass |

PASS

### docs/perf/m65-vanilla-medium.json

| Measure | Baseline | Now | Change | Limit | Result |
|---|---|---|---|---|---|
| budget: average_fps |  | 77.50 |  | min 75 | pass |
| budget: frames_over_50ms |  | 0.00 |  | max 0 | pass |
| budget: gpu_extra_ms |  | 4.20 |  | max 4.5 | pass |
| budget: one_percent_low_fps |  | 46.59 |  | min 45 | pass |

PASS

FAIL (see above)
