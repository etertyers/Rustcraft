# m67-barebones performance

3 flights of 60 s (seed 20111118, render distance 9), median and spread (max − min as % of the median). Measured on Macmini9,1 (Apple M1), commit df8de7d39cb22178d85b8ded9c09f7cdd96e1997, row `barebones`.

| Measure | Median | Spread |
|---|---|---|
| average_fps | 128.63 | 3.8% |
| chunk_generation_ms | 7.86 | 4.9% |
| chunks_generated | 1344.00 | 0.0% |
| chunks_meshed | 1623.00 | 1.9% |
| frames | 7719.00 | 3.8% |
| frames_over_33ms | 0.00 | 0.0% |
| frames_over_50ms | 0.00 | 0.0% |
| gpu_hand_ms | 1.41 | 1.1% |
| gpu_total_ms | 3.89 | 1.0% |
| gpu_ui_ms | 1.36 | 1.1% |
| gpu_world_ms | 3.83 | 0.9% |
| light_mesh_ms | 8.74 | 2.3% |
| memory_mib | 695.98 | 9.2% |
| one_percent_low_fps | 64.93 | 3.3% |
| worst_frame_ms | 21.01 | 8.3% |
