# m58-barebones performance

3 flights of 60 s (seed 20111118, render distance 9), median and spread (max − min as % of the median). Measured on Macmini9,1 (Apple M1), commit 12eeca09487cac0e23bbd5a0ea3378b233a07e9d-dirty, row `barebones`.

| Measure | Median | Spread |
|---|---|---|
| average_fps | 123.69 | 1.3% |
| chunk_generation_ms | 7.74 | 3.9% |
| chunks_generated | 1365.00 | 0.0% |
| chunks_meshed | 1706.00 | 5.0% |
| frames | 7422.00 | 1.3% |
| frames_over_33ms | 1.00 | 200.0% |
| frames_over_50ms | 0.00 | 0.0% |
| gpu_hand_ms | 1.42 | 1.7% |
| gpu_total_ms | 3.93 | 1.0% |
| gpu_ui_ms | 1.37 | 1.7% |
| gpu_world_ms | 3.87 | 0.8% |
| light_mesh_ms | 8.67 | 1.7% |
| memory_mib | 693.12 | 9.8% |
| one_percent_low_fps | 57.26 | 1.8% |
| worst_frame_ms | 37.41 | 30.8% |
