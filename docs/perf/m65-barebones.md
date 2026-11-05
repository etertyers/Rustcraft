# m65-barebones performance

3 flights of 60 s (seed 20111118, render distance 9), median and spread (max − min as % of the median). Measured on Macmini9,1 (Apple M1), commit de2b3f6d6b3674e68839cf58ac1d6060a52fb910-dirty, row `barebones`.

| Measure | Median | Spread |
|---|---|---|
| average_fps | 122.41 | 0.4% |
| chunk_generation_ms | 7.86 | 7.0% |
| chunks_generated | 1344.00 | 0.0% |
| chunks_meshed | 1629.00 | 5.0% |
| frames | 7345.00 | 0.4% |
| frames_over_33ms | 0.00 | 0.0% |
| frames_over_50ms | 0.00 | 0.0% |
| gpu_hand_ms | 1.42 | 0.8% |
| gpu_total_ms | 3.93 | 0.6% |
| gpu_ui_ms | 1.37 | 0.8% |
| gpu_world_ms | 3.87 | 0.7% |
| light_mesh_ms | 8.28 | 3.4% |
| memory_mib | 697.53 | 11.0% |
| one_percent_low_fps | 62.48 | 2.0% |
| worst_frame_ms | 23.90 | 9.4% |
