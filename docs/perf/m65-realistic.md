# m65-realistic performance

3 flights of 60 s (seed 20111118, render distance 9), median and spread (max − min as % of the median). Measured on Macmini9,1 (Apple M1), commit de2b3f6d6b3674e68839cf58ac1d6060a52fb910-dirty, row `realistic`.

| Measure | Median | Spread |
|---|---|---|
| average_fps | 122.74 | 0.6% |
| chunk_generation_ms | 7.57 | 2.8% |
| chunks_generated | 1344.00 | 0.0% |
| chunks_meshed | 1623.00 | 3.3% |
| frames | 7366.00 | 0.6% |
| frames_over_33ms | 0.00 | 0.0% |
| frames_over_50ms | 0.00 | 0.0% |
| gpu_hand_ms | 1.42 | 1.6% |
| gpu_total_ms | 3.93 | 0.5% |
| gpu_ui_ms | 1.36 | 1.6% |
| gpu_world_ms | 3.86 | 0.4% |
| light_mesh_ms | 8.41 | 6.9% |
| memory_mib | 734.69 | 0.5% |
| one_percent_low_fps | 61.07 | 4.9% |
| worst_frame_ms | 25.84 | 18.7% |
