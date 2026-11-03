# M57 performance gate

Macmini9,1 (M1), 1920x1080, render distance 9, 3 flights per look.

| Look | avg FPS | 1% low | GPU frame ms | extra over Vanilla |
|---|---|---|---|---|
| Vanilla | 120 | 56 | 4.17 | - |
| Trailer Low | 93 | 51 | 6.24 | 2.07 |
| Trailer Medium | 79 | 48 | 8.15 | 3.98 |
| Trailer High | 60 | 39 | 11.11 | 6.94 |

```

### docs/perf/m57.json

| Measure | Baseline | Now | Change | Limit | Result |
|---|---|---|---|---|---|
| average_fps | 132.11 | 120.29 | -8.9% | >= 79% | pass |
| one_percent_low_fps | 59.15 | 56.17 | -5.0% | >= 46% | pass |
| chunk_generation_ms | 8.25 | 9.16 | +11.1% | <= 105% | FAIL |
| light_mesh_ms | 9.61 | 9.36 | -2.5% | <= 105% | pass |
| memory_mib | 665.30 | 613.61 | -7.8% | <= 110% | pass |
| bench: 100 ticks of redstone lines | 2.86 | 2.85 | -0.4% | <= 105% | pass |
| bench: 100 ticks of spreading water | 3.52 | 3.59 | +2.0% | <= 105% | pass |
| bench: generate a chunk | 2.76 | 2.75 | -0.1% | <= 105% | pass |
| bench: generate a nether chunk | 1.39 | 1.40 | +0.4% | <= 105% | pass |
| bench: generate an end chunk | 0.04 | 0.03 | -1.5% | <= 105% | pass |
| bench: generate an end city chunk | 0.72 | 0.72 | +0.0% | <= 105% | pass |
| bench: light a chunk | 1.17 | 1.17 | -0.3% | <= 105% | pass |
| bench: mesh a chunk | 2.13 | 2.13 | -0.1% | <= 105% | pass |
| frames_over_50ms | 0 | 0.00 |  | <= 0 | pass |
| budget: one_percent_low_fps |  | 56.17 |  | min 40 | pass |

FAIL: chunk_generation_ms

### docs/perf/m57-trailer-low.json

| Measure | Baseline | Now | Change | Limit | Result |
|---|---|---|---|---|---|
| budget: average_fps |  | 93.42 |  | min 100 | FAIL |
| budget: frames_over_50ms |  | 0.00 |  | max 0 | pass |
| budget: gpu_extra_ms |  | 2.07 |  | max 2.0 | FAIL |
| budget: one_percent_low_fps |  | 51.06 |  | min 50 | pass |

FAIL: budget: average_fps, budget: gpu_extra_ms

### docs/perf/m57-trailer-medium.json

| Measure | Baseline | Now | Change | Limit | Result |
|---|---|---|---|---|---|
| budget: average_fps |  | 79.24 |  | min 75 | pass |
| budget: frames_over_50ms |  | 0.00 |  | max 0 | pass |
| budget: gpu_extra_ms |  | 3.98 |  | max 4.5 | pass |
| budget: one_percent_low_fps |  | 48.41 |  | min 45 | pass |

PASS

### docs/perf/m57-trailer-high.json

| Measure | Baseline | Now | Change | Limit | Result |
|---|---|---|---|---|---|
| budget: average_fps |  | 59.97 |  | min 60 | FAIL |
| budget: frames_over_50ms |  | 0.00 |  | max 0 | pass |
| budget: gpu_extra_ms |  | 6.94 |  | max 7.5 | pass |
| budget: one_percent_low_fps |  | 39.19 |  | min 40 | FAIL |

FAIL: budget: average_fps, budget: one_percent_low_fps

FAIL (see above)
```

## Notes

- Medium passes everything. **Low** misses: 93.4 fps (needs 100) and +2.07 ms (needs 2.0). **High** misses by a hair: 59.97 fps (needs 60) and a 1% low of 39.2 (needs 40).
- Measured with every M57 effect on. On Low, turning all of them off gives 98.5 fps, bloom alone costs about 0.2 ms, and the rest is the Trailer frame's floor. See the PR for what was tried.
- `chunk_generation_ms` fails the Vanilla row (9.16 vs 8.25): the same machine drift as M54 to M56.
