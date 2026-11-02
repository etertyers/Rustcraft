# M55 performance gate

Macmini9,1 (M1), 1920x1080, render distance 9, 3 flights per look. GPU frame ms is the span from the first pass's start to the last pass's end (passes overlap on this GPU, so per-pass times add up to more).

| Look | avg FPS | 1% low | GPU frame ms | shadow | world | extra over Vanilla |
|---|---|---|---|---|---|---|
| Vanilla | 130 | 71 | 4.10 | - | 4.04 | - |
| Trailer Low | 104 | 54 | 5.38 | 0.11 | 5.09 | 1.23 |
| Trailer Medium | 95 | 50 | 6.28 | 0.35 | 5.69 | 2.13 |
| Trailer High | 78 | 41 | 8.01 | 0.65 | 7.32 | 3.86 |

```

### docs/perf/m55.json

| Measure | Baseline | Now | Change | Limit | Result |
|---|---|---|---|---|---|
| average_fps | 132.11 | 130.28 | -1.4% | >= 79% | pass |
| one_percent_low_fps | 59.15 | 70.75 | +19.6% | >= 46% | pass |
| chunk_generation_ms | 8.25 | 9.40 | +13.9% | <= 105% | FAIL |
| light_mesh_ms | 9.61 | 9.86 | +2.7% | <= 105% | pass |
| memory_mib | 665.30 | 634.20 | -4.7% | <= 110% | pass |
| bench: 100 ticks of redstone lines | 2.86 | 2.75 | -3.7% | <= 105% | pass |
| bench: 100 ticks of spreading water | 3.52 | 3.24 | -7.8% | <= 105% | pass |
| bench: generate a chunk | 2.76 | 2.75 | -0.3% | <= 105% | pass |
| bench: generate a nether chunk | 1.39 | 1.38 | -0.5% | <= 105% | pass |
| bench: generate an end chunk | 0.04 | 0.04 | -0.1% | <= 105% | pass |
| bench: generate an end city chunk | 0.72 | 0.72 | +0.1% | <= 105% | pass |
| bench: light a chunk | 1.17 | 1.17 | -0.2% | <= 105% | pass |
| bench: mesh a chunk | 2.13 | 2.18 | +1.9% | <= 105% | pass |
| frames_over_50ms | 0 | 0.00 |  | <= 0 | pass |
| budget: one_percent_low_fps |  | 70.75 |  | min 40 | pass |

FAIL: chunk_generation_ms

### docs/perf/m55-trailer-low.json

| Measure | Baseline | Now | Change | Limit | Result |
|---|---|---|---|---|---|
| budget: average_fps |  | 104.39 |  | min 100 | pass |
| budget: frames_over_50ms |  | 0.00 |  | max 0 | pass |
| budget: gpu_extra_ms |  | 1.23 |  | max 2.0 | pass |
| budget: one_percent_low_fps |  | 54.12 |  | min 50 | pass |

PASS

### docs/perf/m55-trailer-medium.json

| Measure | Baseline | Now | Change | Limit | Result |
|---|---|---|---|---|---|
| budget: average_fps |  | 95.30 |  | min 75 | pass |
| budget: frames_over_50ms |  | 0.00 |  | max 0 | pass |
| budget: gpu_extra_ms |  | 2.13 |  | max 4.5 | pass |
| budget: one_percent_low_fps |  | 50.32 |  | min 45 | pass |

PASS

### docs/perf/m55-trailer-high.json

| Measure | Baseline | Now | Change | Limit | Result |
|---|---|---|---|---|---|
| budget: average_fps |  | 77.63 |  | min 60 | pass |
| budget: frames_over_50ms |  | 0.00 |  | max 0 | pass |
| budget: gpu_extra_ms |  | 3.86 |  | max 7.5 | pass |
| budget: one_percent_low_fps |  | 41.40 |  | min 40 | pass |

PASS

FAIL (see above)
```

## Notes

- `chunk_generation_ms` fails on the Vanilla row: 9.40 vs the baseline's 8.25. M53's untouched code measured 9.05 in M54's Step 0, and `generate a chunk` benches unchanged (2.77 vs 2.76 ms), so this is machine drift, not code.
- `mesh a chunk` benched against M53's code on this machine in one session: 2.133 vs 2.177 ms (+2.1%). M54's face attribute cost 4.3% before this milestone made it per quad; M54's own table had stale bench numbers.
- The Medium shadow pass is 0.35 ms on the flight; the world pass is 1.65 ms over Vanilla's (the lit shading and lookups; its share was 0.6): the total stays inside 4.5 ms.
