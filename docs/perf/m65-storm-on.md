# m65-storm-on performance

3 flights of 60 s (seed 20111118, render distance 9), median and spread (max − min as % of the median). Measured on Macmini9,1 (Apple M1), commit de2b3f6d6b3674e68839cf58ac1d6060a52fb910-dirty, row `barebones-medium`.

| Measure | Median | Spread |
|---|---|---|
| average_fps | 68.89 | 1.8% |
| chunk_generation_ms | 8.13 | 4.3% |
| chunks_generated | 1358.00 | 0.7% |
| chunks_meshed | 1544.00 | 3.6% |
| frames | 4134.00 | 1.8% |
| frames_over_33ms | 18.00 | 50.0% |
| frames_over_50ms | 1.00 | 400.0% |
| gpu_bloom_ms | 16.89 | 2.0% |
| gpu_fxaa_ms | 4.01 | 1.9% |
| gpu_hand_ms | 3.37 | 2.0% |
| gpu_post_ms | 3.80 | 1.9% |
| gpu_shadow_ms | 3.21 | 0.8% |
| gpu_shafts_ms | 7.10 | 2.0% |
| gpu_total_ms | 8.67 | 1.2% |
| gpu_ui_ms | 4.01 | 1.9% |
| gpu_water_ms | 3.59 | 1.8% |
| gpu_world_ms | 4.97 | 1.1% |
| light_mesh_ms | 7.72 | 2.5% |
| memory_mib | 734.30 | 1.7% |
| one_percent_low_fps | 37.96 | 16.2% |
| worst_frame_ms | 57.50 | 16.0% |
