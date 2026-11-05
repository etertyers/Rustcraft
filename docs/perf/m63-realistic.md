# m58-realistic performance

3 flights of 60 s (seed 20111118, render distance 9), median and spread (max − min as % of the median). Measured on Macmini9,1 (Apple M1), commit 12eeca09487cac0e23bbd5a0ea3378b233a07e9d-dirty, row `realistic`.

| Measure | Median | Spread |
|---|---|---|
| average_fps | 123.31 | 2.0% |
| chunk_generation_ms | 7.89 | 2.8% |
| chunks_generated | 1365.00 | 0.0% |
| chunks_meshed | 1707.00 | 5.0% |
| frames | 7400.00 | 2.0% |
| frames_over_33ms | 1.00 | 100.0% |
| frames_over_50ms | 0.00 | 0.0% |
| gpu_hand_ms | 1.42 | 1.1% |
| gpu_total_ms | 3.94 | 1.1% |
| gpu_ui_ms | 1.37 | 1.1% |
| gpu_world_ms | 3.87 | 1.1% |
| light_mesh_ms | 8.78 | 3.7% |
| memory_mib | 721.70 | 9.1% |
| one_percent_low_fps | 61.73 | 1.9% |
| worst_frame_ms | 33.89 | 57.7% |
