# m62-barebones performance

3 flights of 60 s (seed 20111118, render distance 9), median and spread (max − min as % of the median). Measured on Macmini9,1 (Apple M1), commit 95e8c1357dd30b821783ab0ebcf2e914a120ce6b, row `barebones`.

| Measure | Median | Spread |
|---|---|---|
| average_fps | 122.90 | 0.9% |
| chunk_generation_ms | 7.72 | 0.3% |
| chunks_generated | 1365.00 | 0.0% |
| chunks_meshed | 1623.00 | 2.0% |
| frames | 7375.00 | 0.9% |
| frames_over_33ms | 0.00 | 0.0% |
| frames_over_50ms | 0.00 | 0.0% |
| gpu_hand_ms | 1.42 | 0.2% |
| gpu_total_ms | 3.94 | 1.1% |
| gpu_ui_ms | 1.36 | 0.2% |
| gpu_world_ms | 3.87 | 1.0% |
| light_mesh_ms | 8.62 | 1.8% |
| memory_mib | 691.08 | 9.6% |
| one_percent_low_fps | 57.19 | 0.5% |
| worst_frame_ms | 24.28 | 82.6% |
