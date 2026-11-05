# m65-vanilla-medium performance

3 flights of 60 s (seed 20111118, render distance 9), median and spread (max − min as % of the median). Measured on Macmini9,1 (Apple M1), commit de2b3f6d6b3674e68839cf58ac1d6060a52fb910-dirty, row `vanilla-medium`.

| Measure | Median | Spread |
|---|---|---|
| average_fps | 77.50 | 0.9% |
| chunk_generation_ms | 7.97 | 1.8% |
| chunks_generated | 1344.00 | 0.0% |
| chunks_meshed | 1605.00 | 2.9% |
| frames | 4651.00 | 0.9% |
| frames_over_33ms | 0.00 | 0.0% |
| frames_over_50ms | 0.00 | 0.0% |
| gpu_bloom_ms | 22.27 | 1.1% |
| gpu_fxaa_ms | 5.08 | 1.0% |
| gpu_hand_ms | 4.57 | 1.1% |
| gpu_post_ms | 4.91 | 1.0% |
| gpu_shadow_ms | 0.30 | 1.4% |
| gpu_shafts_ms | 9.42 | 1.1% |
| gpu_total_ms | 8.11 | 1.0% |
| gpu_ui_ms | 5.07 | 1.0% |
| gpu_water_ms | 5.32 | 0.9% |
| gpu_world_ms | 3.37 | 0.6% |
| light_mesh_ms | 7.75 | 2.7% |
| memory_mib | 705.86 | 2.0% |
| one_percent_low_fps | 46.59 | 5.0% |
| worst_frame_ms | 29.95 | 14.8% |
