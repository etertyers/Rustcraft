# M56 performance gate

Macmini9,1 (M1), 1920x1080, render distance 9, 3 flights per look. GPU frame ms is first pass start to last pass end.

| Look | avg FPS | 1% low | GPU frame ms | shadow | world | water | extra over Vanilla |
|---|---|---|---|---|---|---|---|
| Vanilla | 154 | 90 | 4.05 | - | 4.00 | - | - |
| Trailer Low | 117 | 76 | 5.76 | 0.10 | 5.52 | - | 1.70 |
| Trailer Medium | 99 | 68 | 7.23 | 0.26 | 3.47 | 4.91 | 3.18 |
| Trailer High | 77 | 54 | 9.64 | 0.58 | 4.54 | 7.25 | 5.59 |

```

### docs/perf/m56.json

| Measure | Baseline | Now | Change | Limit | Result |
|---|---|---|---|---|---|
| average_fps | 132.11 | 154.42 | +16.9% | >= 79% | pass |
| one_percent_low_fps | 59.15 | 89.85 | +51.9% | >= 46% | pass |
| chunk_generation_ms | 8.25 | 10.27 | +24.4% | <= 105% | FAIL |
| light_mesh_ms | 9.61 | 9.81 | +2.2% | <= 105% | pass |
| memory_mib | 665.30 | 664.77 | -0.1% | <= 110% | pass |
| bench: 100 ticks of redstone lines | 2.86 | 2.85 | -0.3% | <= 105% | pass |
| bench: 100 ticks of spreading water | 3.52 | 3.52 | -0.0% | <= 105% | pass |
| bench: generate a chunk | 2.76 | 2.75 | -0.3% | <= 105% | pass |
| bench: generate a nether chunk | 1.39 | 1.37 | -1.5% | <= 105% | pass |
| bench: generate an end chunk | 0.04 | 0.03 | -0.5% | <= 105% | pass |
| bench: generate an end city chunk | 0.72 | 0.72 | -0.3% | <= 105% | pass |
| bench: light a chunk | 1.17 | 1.17 | -0.4% | <= 105% | pass |
| bench: mesh a chunk | 2.13 | 2.22 | +3.8% | <= 105% | pass |
| frames_over_50ms | 0 | 0.00 |  | <= 0 | pass |
| budget: one_percent_low_fps |  | 89.85 |  | min 40 | pass |

FAIL: chunk_generation_ms

### docs/perf/m56-trailer-low.json

| Measure | Baseline | Now | Change | Limit | Result |
|---|---|---|---|---|---|
| budget: average_fps |  | 117.33 |  | min 100 | pass |
| budget: frames_over_50ms |  | 0.00 |  | max 0 | pass |
| budget: gpu_extra_ms |  | 1.70 |  | max 2.0 | pass |
| budget: one_percent_low_fps |  | 75.92 |  | min 50 | pass |

PASS

### docs/perf/m56-trailer-medium.json

| Measure | Baseline | Now | Change | Limit | Result |
|---|---|---|---|---|---|
| budget: average_fps |  | 98.91 |  | min 75 | pass |
| budget: frames_over_50ms |  | 0.00 |  | max 0 | pass |
| budget: gpu_extra_ms |  | 3.18 |  | max 4.5 | pass |
| budget: one_percent_low_fps |  | 68.49 |  | min 45 | pass |

PASS

### docs/perf/m56-trailer-high.json

| Measure | Baseline | Now | Change | Limit | Result |
|---|---|---|---|---|---|
| budget: average_fps |  | 76.62 |  | min 60 | pass |
| budget: frames_over_50ms |  | 0.00 |  | max 0 | pass |
| budget: gpu_extra_ms |  | 5.59 |  | max 7.5 | pass |
| budget: one_percent_low_fps |  | 54.09 |  | min 40 | pass |

PASS

FAIL (see above)
```

## Notes

- `chunk_generation_ms` fails the Vanilla row (10.27 vs 8.25): the same machine drift as M54 and M55 (untouched M53 code measured 9.05 before any change; generation isn't touched here).
- This milestone moved the shadow index buffer (and the mesh's y range) from the main thread's upload to the mesh worker thread. That alone took Low from 97 to 117+ fps and removed the 1% low and slow-frame misses; Vanilla's uploads got cheaper too.
- Tile-based GPUs pay for texture usage flags: `COPY_SRC` on the depth and HDR textures cost fps at Low, so it is only set while a frame copies the scene (Medium and High with Reflections or Refraction on).
- Each Trailer pipeline is drawn once with a zero-area triangle on the first Trailer frame, so none compiles mid-play (the first water in view cost a 50 ms frame on High, and 1 s once on Medium).
