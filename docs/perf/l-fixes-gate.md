# Checkpoint L fixes: perf gate

Mac mini M1, `--perf-test --runs 3 --all-looks --label l-fixes`, then `cargo bench` added to each file.


### docs/perf/l-fixes.json

| Measure | Baseline | Now | Change | Limit | Result |
|---|---|---|---|---|---|
| average_fps | 123.80 | 122.65 | -0.9% | >= 79% | pass |
| one_percent_low_fps | 61.77 | 57.61 | -6.7% | >= 46% | pass |
| chunk_generation_ms | 8.07 | 7.72 | -4.3% | <= 105% | pass |
| light_mesh_ms | 9.08 | 8.57 | -5.6% | <= 105% | pass |
| memory_mib | 678.34 | 680.61 | +0.3% | <= 110% | pass |
| bench: 100 ticks of redstone lines | 2.80 | 2.79 | -0.3% | <= 105% | pass |
| bench: 100 ticks of spreading water | 3.18 | 3.19 | +0.3% | <= 105% | pass |
| bench: generate a chunk | 2.73 | 2.76 | +1.3% | <= 105% | pass |
| bench: generate a nether chunk | 1.39 | 1.40 | +0.8% | <= 105% | pass |
| bench: generate an end chunk | 0.04 | 0.03 | -1.0% | <= 105% | pass |
| bench: generate an end city chunk | 0.72 | 0.72 | +0.0% | <= 105% | pass |
| bench: light a chunk | 1.17 | 1.23 | +5.7% | <= 105% | FAIL |
| bench: mesh a chunk | 2.14 | 1.96 | -8.3% | <= 105% | pass |
| frames_over_50ms | 0 | 0.00 |  | <= 0 | pass |
| budget: one_percent_low_fps |  | 57.61 |  | min 40 | pass |

FAIL: bench: light a chunk

### docs/perf/l-fixes-barebones-high.json

| Measure | Baseline | Now | Change | Limit | Result |
|---|---|---|---|---|---|
| average_fps | 59.84 | 59.10 | -1.2% | >= 95% | pass |
| one_percent_low_fps | 39.27 | 39.33 | +0.1% | >= 95% | pass |
| chunk_generation_ms | 8.20 | 7.96 | -2.9% | <= 105% | pass |
| light_mesh_ms | 8.28 | 8.14 | -1.7% | <= 105% | pass |
| memory_mib | 680.02 | 729.19 | +7.2% | <= 110% | pass |
| bench: 100 ticks of redstone lines | 2.80 | 2.79 | -0.3% | <= 105% | pass |
| bench: 100 ticks of spreading water | 3.18 | 3.19 | +0.3% | <= 105% | pass |
| bench: generate a chunk | 2.73 | 2.76 | +1.3% | <= 105% | pass |
| bench: generate a nether chunk | 1.39 | 1.40 | +0.8% | <= 105% | pass |
| bench: generate an end chunk | 0.04 | 0.03 | -1.0% | <= 105% | pass |
| bench: generate an end city chunk | 0.72 | 0.72 | +0.0% | <= 105% | pass |
| bench: light a chunk | 1.17 | 1.23 | +5.7% | <= 105% | FAIL |
| bench: mesh a chunk | 2.14 | 1.96 | -8.3% | <= 105% | pass |
| frames_over_50ms | 0 | 0.00 |  | <= 0 | pass |
| budget: average_fps |  | 59.10 |  | min 60 | FAIL |
| budget: frames_over_50ms |  | 0.00 |  | max 0 | pass |
| budget: gpu_extra_ms |  | 7.14 |  | max 7.5 | pass |
| budget: one_percent_low_fps |  | 39.33 |  | min 40 | FAIL |

FAIL: bench: light a chunk, budget: average_fps, budget: one_percent_low_fps

### docs/perf/l-fixes-barebones-low.json

| Measure | Baseline | Now | Change | Limit | Result |
|---|---|---|---|---|---|
| average_fps | 93.36 | 91.04 | -2.5% | >= 95% | pass |
| one_percent_low_fps | 57.36 | 53.70 | -6.4% | >= 95% | FAIL |
| chunk_generation_ms | 7.90 | 7.89 | -0.2% | <= 105% | pass |
| light_mesh_ms | 9.02 | 8.53 | -5.5% | <= 105% | pass |
| memory_mib | 605.03 | 724.31 | +19.7% | <= 110% | FAIL |
| bench: 100 ticks of redstone lines | 2.80 | 2.79 | -0.3% | <= 105% | pass |
| bench: 100 ticks of spreading water | 3.18 | 3.19 | +0.3% | <= 105% | pass |
| bench: generate a chunk | 2.73 | 2.76 | +1.3% | <= 105% | pass |
| bench: generate a nether chunk | 1.39 | 1.40 | +0.8% | <= 105% | pass |
| bench: generate an end chunk | 0.04 | 0.03 | -1.0% | <= 105% | pass |
| bench: generate an end city chunk | 0.72 | 0.72 | +0.0% | <= 105% | pass |
| bench: light a chunk | 1.17 | 1.23 | +5.7% | <= 105% | FAIL |
| bench: mesh a chunk | 2.14 | 1.96 | -8.3% | <= 105% | pass |
| frames_over_50ms | 0 | 0.00 |  | <= 0 | pass |
| budget: average_fps |  | 91.04 |  | min 100 | FAIL |
| budget: frames_over_50ms |  | 0.00 |  | max 0 | pass |
| budget: gpu_extra_ms |  | 2.27 |  | max 2.0 | FAIL |
| budget: one_percent_low_fps |  | 53.70 |  | min 50 | pass |

FAIL: one_percent_low_fps, memory_mib, bench: light a chunk, budget: average_fps, budget: gpu_extra_ms

### docs/perf/l-fixes-barebones-medium.json

| Measure | Baseline | Now | Change | Limit | Result |
|---|---|---|---|---|---|
| average_fps | 77.64 | 76.64 | -1.3% | >= 95% | pass |
| one_percent_low_fps | 47.67 | 44.64 | -6.4% | >= 95% | FAIL |
| chunk_generation_ms | 7.91 | 7.87 | -0.5% | <= 105% | pass |
| light_mesh_ms | 8.65 | 8.03 | -7.1% | <= 105% | pass |
| memory_mib | 579.50 | 731.56 | +26.2% | <= 110% | FAIL |
| bench: 100 ticks of redstone lines | 2.80 | 2.79 | -0.3% | <= 105% | pass |
| bench: 100 ticks of spreading water | 3.18 | 3.19 | +0.3% | <= 105% | pass |
| bench: generate a chunk | 2.73 | 2.76 | +1.3% | <= 105% | pass |
| bench: generate a nether chunk | 1.39 | 1.40 | +0.8% | <= 105% | pass |
| bench: generate an end chunk | 0.04 | 0.03 | -1.0% | <= 105% | pass |
| bench: generate an end city chunk | 0.72 | 0.72 | +0.0% | <= 105% | pass |
| bench: light a chunk | 1.17 | 1.23 | +5.7% | <= 105% | FAIL |
| bench: mesh a chunk | 2.14 | 1.96 | -8.3% | <= 105% | pass |
| frames_over_50ms | 0 | 0.00 |  | <= 0 | pass |
| budget: average_fps |  | 76.64 |  | min 75 | pass |
| budget: frames_over_50ms |  | 0.00 |  | max 0 | pass |
| budget: gpu_extra_ms |  | 4.25 |  | max 4.5 | pass |
| budget: one_percent_low_fps |  | 44.64 |  | min 45 | FAIL |

FAIL: one_percent_low_fps, memory_mib, bench: light a chunk, budget: one_percent_low_fps

### docs/perf/l-fixes-barebones.json

| Measure | Baseline | Now | Change | Limit | Result |
|---|---|---|---|---|---|
| budget: average_fps |  | 121.82 |  | min 111.4 | pass |
| budget: frames_over_50ms |  | 0.00 |  | max 0 | pass |

PASS

### docs/perf/l-fixes-realistic-medium.json

| Measure | Baseline | Now | Change | Limit | Result |
|---|---|---|---|---|---|
| budget: average_fps |  | 77.10 |  | min 75 | pass |
| budget: frames_over_50ms |  | 0.00 |  | max 0 | pass |
| budget: gpu_extra_ms |  | 4.22 |  | max 4.5 | pass |
| budget: one_percent_low_fps |  | 46.12 |  | min 45 | pass |

PASS

### docs/perf/l-fixes-realistic.json

| Measure | Baseline | Now | Change | Limit | Result |
|---|---|---|---|---|---|
| budget: average_fps |  | 122.59 |  | min 111.4 | pass |
| budget: frames_over_50ms |  | 0.00 |  | max 0 | pass |

PASS

### docs/perf/l-fixes-vanilla-medium.json

| Measure | Baseline | Now | Change | Limit | Result |
|---|---|---|---|---|---|
| budget: average_fps |  | 77.91 |  | min 75 | pass |
| budget: frames_over_50ms |  | 0.00 |  | max 0 | pass |
| budget: gpu_extra_ms |  | 4.14 |  | max 4.5 | pass |
| budget: one_percent_low_fps |  | 45.09 |  | min 45 | pass |

PASS

FAIL (see above)
