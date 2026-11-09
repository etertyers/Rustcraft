# m65-barebones-medium performance

3 flights of 60 s (seed 20111118, render distance 9), median and spread (max − min as % of the median). Measured on Macmini9,1 (Apple M1), commit de2b3f6d6b3674e68839cf58ac1d6060a52fb910-dirty, row `barebones-medium`.

| Measure | Median | Spread |
|---|---|---|
| average_fps | 77.27 | 1.3% |
| chunk_generation_ms | 8.09 | 3.1% |
| chunks_generated | 1344.00 | 0.0% |
| chunks_meshed | 1686.00 | 7.1% |
| frames | 4638.00 | 1.3% |
| frames_over_33ms | 0.00 | 0.0% |
| frames_over_50ms | 0.00 | 0.0% |
| gpu_bloom_ms | 22.03 | 0.4% |
| gpu_fxaa_ms | 5.06 | 0.9% |
| gpu_hand_ms | 4.55 | 0.9% |
| gpu_post_ms | 4.89 | 0.9% |
| gpu_shadow_ms | 0.30 | 1.6% |
| gpu_shafts_ms | 9.37 | 0.9% |
| gpu_total_ms | 8.11 | 0.7% |
| gpu_ui_ms | 5.05 | 0.9% |
| gpu_water_ms | 5.29 | 0.8% |
| gpu_world_ms | 3.37 | 0.5% |
| light_mesh_ms | 8.03 | 4.5% |
| memory_mib | 708.33 | 1.4% |
| one_percent_low_fps | 46.34 | 1.0% |
| worst_frame_ms | 33.31 | 26.5% |
