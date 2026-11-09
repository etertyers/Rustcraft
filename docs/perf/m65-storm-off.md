# m65-storm-off performance

3 flights of 60 s (seed 20111118, render distance 9), median and spread (max − min as % of the median). Measured on Macmini9,1 (Apple M1), commit de2b3f6d6b3674e68839cf58ac1d6060a52fb910-dirty, row `barebones-medium`.

| Measure | Median | Spread |
|---|---|---|
| average_fps | 69.84 | 0.9% |
| chunk_generation_ms | 8.14 | 3.6% |
| chunks_generated | 1358.00 | 0.0% |
| chunks_meshed | 1546.00 | 0.9% |
| frames | 4192.00 | 0.9% |
| frames_over_33ms | 12.00 | 25.0% |
| frames_over_50ms | 1.00 | 100.0% |
| gpu_bloom_ms | 16.67 | 1.5% |
| gpu_fxaa_ms | 3.97 | 1.5% |
| gpu_hand_ms | 3.33 | 1.5% |
| gpu_post_ms | 3.76 | 1.5% |
| gpu_shadow_ms | 3.20 | 0.6% |
| gpu_shafts_ms | 7.01 | 1.5% |
| gpu_total_ms | 8.61 | 0.9% |
| gpu_ui_ms | 3.96 | 1.5% |
| gpu_water_ms | 3.55 | 1.3% |
| gpu_world_ms | 4.94 | 0.9% |
| light_mesh_ms | 7.76 | 0.9% |
| memory_mib | 733.69 | 3.4% |
| one_percent_low_fps | 40.73 | 4.8% |
| worst_frame_ms | 51.58 | 13.0% |
