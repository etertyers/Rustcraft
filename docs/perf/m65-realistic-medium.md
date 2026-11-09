# m65-realistic-medium performance

3 flights of 60 s (seed 20111118, render distance 9), median and spread (max − min as % of the median). Measured on Macmini9,1 (Apple M1), commit de2b3f6d6b3674e68839cf58ac1d6060a52fb910-dirty, row `realistic-medium`.

| Measure | Median | Spread |
|---|---|---|
| average_fps | 77.23 | 2.2% |
| chunk_generation_ms | 7.95 | 2.7% |
| chunks_generated | 1344.00 | 0.0% |
| chunks_meshed | 1600.00 | 1.0% |
| frames | 4635.00 | 2.2% |
| frames_over_33ms | 0.00 | 0.0% |
| frames_over_50ms | 0.00 | 0.0% |
| gpu_bloom_ms | 22.16 | 0.4% |
| gpu_fxaa_ms | 5.05 | 1.1% |
| gpu_hand_ms | 4.55 | 1.1% |
| gpu_post_ms | 4.88 | 1.2% |
| gpu_shadow_ms | 0.30 | 1.0% |
| gpu_shafts_ms | 9.37 | 1.1% |
| gpu_total_ms | 8.08 | 0.9% |
| gpu_ui_ms | 5.04 | 1.1% |
| gpu_water_ms | 5.29 | 1.1% |
| gpu_world_ms | 3.36 | 1.0% |
| light_mesh_ms | 7.79 | 0.9% |
| memory_mib | 744.83 | 0.4% |
| one_percent_low_fps | 45.49 | 3.5% |
| worst_frame_ms | 29.50 | 1.6% |
