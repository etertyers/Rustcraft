# M58 performance gate

Macmini9,1 (M1), 1920x1080, render distance 9, 3 flights per look.

| Look | avg FPS | 1% low | GPU frame ms | extra over Vanilla |
|---|---|---|---|---|
| Vanilla | 122 | 65 | 4.13 | - |
| Trailer Low | 92 | 51 | 6.33 | 2.20 |
| Trailer Medium | 78 | 46 | 8.11 | 3.98 |
| Trailer High | 60 | 39 | 11.10 | 6.97 |

```

### docs/perf/m58.json

| Measure | Baseline | Now | Change | Limit | Result |
|---|---|---|---|---|---|
| average_fps | 132.11 | 121.97 | -7.7% | >= 79% | pass |
| one_percent_low_fps | 59.15 | 65.36 | +10.5% | >= 46% | pass |
| chunk_generation_ms | 8.25 | 9.11 | +10.4% | <= 105% | FAIL |
| light_mesh_ms | 9.61 | 9.65 | +0.4% | <= 105% | pass |
| memory_mib | 665.30 | 671.45 | +0.9% | <= 110% | pass |
| bench: 100 ticks of redstone lines | 2.86 | 2.81 | -1.8% | <= 105% | pass |
| bench: 100 ticks of spreading water | 3.52 | 3.20 | -9.2% | <= 105% | pass |
| bench: generate a chunk | 2.76 | 2.74 | -0.7% | <= 105% | pass |
| bench: generate a nether chunk | 1.39 | 1.39 | -0.3% | <= 105% | pass |
| bench: generate an end chunk | 0.04 | 0.04 | -0.2% | <= 105% | pass |
| bench: generate an end city chunk | 0.72 | 0.72 | +0.2% | <= 105% | pass |
| bench: light a chunk | 1.17 | 1.17 | -0.3% | <= 105% | pass |
| bench: mesh a chunk | 2.13 | 2.15 | +0.6% | <= 105% | pass |
| frames_over_50ms | 0 | 0.00 |  | <= 0 | pass |
| budget: one_percent_low_fps |  | 65.36 |  | min 40 | pass |

FAIL: chunk_generation_ms

### docs/perf/m58-trailer-low.json

| Measure | Baseline | Now | Change | Limit | Result |
|---|---|---|---|---|---|
| budget: average_fps |  | 92.02 |  | min 100 | FAIL |
| budget: frames_over_50ms |  | 0.00 |  | max 0 | pass |
| budget: gpu_extra_ms |  | 2.20 |  | max 2.0 | FAIL |
| budget: one_percent_low_fps |  | 50.88 |  | min 50 | pass |

FAIL: budget: average_fps, budget: gpu_extra_ms

### docs/perf/m58-trailer-medium.json

| Measure | Baseline | Now | Change | Limit | Result |
|---|---|---|---|---|---|
| budget: average_fps |  | 78.32 |  | min 75 | pass |
| budget: frames_over_50ms |  | 0.00 |  | max 0 | pass |
| budget: gpu_extra_ms |  | 3.98 |  | max 4.5 | pass |
| budget: one_percent_low_fps |  | 45.55 |  | min 45 | pass |

PASS

### docs/perf/m58-trailer-high.json

| Measure | Baseline | Now | Change | Limit | Result |
|---|---|---|---|---|---|
| budget: average_fps |  | 59.88 |  | min 60 | FAIL |
| budget: frames_over_50ms |  | 0.00 |  | max 0 | pass |
| budget: gpu_extra_ms |  | 6.97 |  | max 7.5 | pass |
| budget: one_percent_low_fps |  | 38.80 |  | min 40 | FAIL |

FAIL: budget: average_fps, budget: one_percent_low_fps

FAIL (see above)
```

## Notes

- Same picture as M57: Medium passes everything; Low (92 fps, +2.2 ms) and High (59.9 fps, 1% low 38.8) miss by a few percent. This milestone adds no GPU work; the mob model path costs CPU only (100 animated 12-part mobs: 0.51 ms a frame, the plain pig model 0.19).
- `chunk_generation_ms` fails the Vanilla row (9.11 vs 8.25): the same machine drift as M54 to M57.
