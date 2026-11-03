# m54-step0 performance

3 flights of 60 s (seed 20111118, render distance 9), median and spread (max − min as % of the median). Measured on Macmini9,1 (Apple M1), commit 0b0099bbc5eebd5a7f7a11c952198af8285e7b15-dirty, row `vanilla`.

| Measure | Median | Spread |
|---|---|---|
| average_fps | 134.52 | 14.0% |
| chunk_generation_ms | 9.05 | 5.6% |
| chunks_generated | 1365.00 | 0.0% |
| chunks_meshed | 1616.00 | 9.0% |
| frames | 8072.00 | 14.0% |
| frames_over_33ms | 0.00 | 0.0% |
| frames_over_50ms | 0.00 | 0.0% |
| light_mesh_ms | 9.66 | 6.6% |
| memory_mib | 607.56 | 18.9% |
| one_percent_low_fps | 59.87 | 16.9% |
| worst_frame_ms | 31.71 | 7901.1% |
