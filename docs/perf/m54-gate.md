# M54 performance gate

Measured on Macmini9,1 (Apple M1), 1920x1080, render distance 9, 3 flights per look, GPU times from timestamp queries (Metal supports them at pass boundaries only). Trailer's HDR target is Rg11b10Ufloat (4 bytes a pixel); Rgba16Float cost 2.5 ms extra on Low, over its 2.0 ms budget.

| Look | avg FPS | 1% low | GPU total ms | world | hand | post | ui | extra over Vanilla |
|---|---|---|---|---|---|---|---|---|
| Vanilla | 153 | 90 | 6.84 | 4.01 | 1.44 | - | 1.39 | - |
| Trailer Low | 150 | 93 | 8.39 | 4.02 | 1.44 | 1.47 | 1.46 | 1.55 |
| Trailer Medium | 150 | 93 | 8.40 | 4.03 | 1.44 | 1.47 | 1.46 | 1.56 |
| Trailer High | 150 | 91 | 8.41 | 4.03 | 1.45 | 1.47 | 1.46 | 1.57 |

## perf_gate.py


### docs/perf/m54.json

| Measure | Baseline | Now | Change | Limit | Result |
|---|---|---|---|---|---|
| average_fps | 132.11 | 152.86 | +15.7% | >= 79% | pass |
| one_percent_low_fps | 59.15 | 90.26 | +52.6% | >= 46% | pass |
| chunk_generation_ms | 8.25 | 9.92 | +20.2% | <= 105% | FAIL |
| light_mesh_ms | 9.61 | 9.69 | +0.9% | <= 105% | pass |
| memory_mib | 665.30 | 569.22 | -14.4% | <= 110% | pass |
| bench: 100 ticks of redstone lines | 2.86 | 2.86 | +0.0% | <= 105% | pass |
| bench: 100 ticks of spreading water | 3.52 | 3.52 | +0.0% | <= 105% | pass |
| bench: generate a chunk | 2.76 | 2.76 | +0.0% | <= 105% | pass |
| bench: generate a nether chunk | 1.39 | 1.39 | +0.0% | <= 105% | pass |
| bench: generate an end chunk | 0.04 | 0.04 | +0.0% | <= 105% | pass |
| bench: generate an end city chunk | 0.72 | 0.72 | +0.0% | <= 105% | pass |
| bench: light a chunk | 1.17 | 1.17 | +0.0% | <= 105% | pass |
| bench: mesh a chunk | 2.13 | 2.13 | +0.0% | <= 105% | pass |
| frames_over_50ms | 0 | 0.00 |  | <= 0 | pass |
| budget: one_percent_low_fps |  | 90.26 |  | min 40 | pass |

FAIL: chunk_generation_ms

### docs/perf/m54-trailer-low.json

| Measure | Baseline | Now | Change | Limit | Result |
|---|---|---|---|---|---|
| budget: average_fps |  | 149.55 |  | min 100 | pass |
| budget: frames_over_50ms |  | 0.00 |  | max 0 | pass |
| budget: gpu_extra_ms |  | 1.55 |  | max 2.0 | pass |
| budget: one_percent_low_fps |  | 92.93 |  | min 50 | pass |

PASS

### docs/perf/m54-trailer-medium.json

| Measure | Baseline | Now | Change | Limit | Result |
|---|---|---|---|---|---|
| budget: average_fps |  | 150.08 |  | min 75 | pass |
| budget: frames_over_50ms |  | 0.00 |  | max 0 | pass |
| budget: gpu_extra_ms |  | 1.56 |  | max 4.5 | pass |
| budget: one_percent_low_fps |  | 92.51 |  | min 45 | pass |

PASS

### docs/perf/m54-trailer-high.json

| Measure | Baseline | Now | Change | Limit | Result |
|---|---|---|---|---|---|
| budget: average_fps |  | 149.64 |  | min 60 | pass |
| budget: frames_over_50ms |  | 0.00 |  | max 0 | pass |
| budget: gpu_extra_ms |  | 1.57 |  | max 7.5 | pass |
| budget: one_percent_low_fps |  | 91.09 |  | min 40 | pass |

PASS

FAIL (see above)

## Known issue

`chunk_generation_ms` fails the Vanilla row (9.92 vs 8.25 ms). The unchanged M53 code measured 9.05 ms in Step 0 (docs/perf/m54-step0.md), before any M54 change, and nothing in worldgen changed (golden hashes pass unchanged), so the baseline looks recorded on a cooler or quieter machine. Kenzo: re-record the baseline or tell me to chase it.

Full per-look tables: docs/perf/m54-*.md
