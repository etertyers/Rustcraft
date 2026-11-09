# m67-barebones-medium performance

3 flights of 60 s (seed 20111118, render distance 9), median and spread (max − min as % of the median). Measured on Macmini9,1 (Apple M1), commit df8de7d39cb22178d85b8ded9c09f7cdd96e1997, row `barebones-medium`.

| Measure | Median | Spread |
|---|---|---|
| average_fps | 78.80 | 2.1% |
| chunk_generation_ms | 8.03 | 0.8% |
| chunks_generated | 1344.00 | 0.0% |
| chunks_meshed | 1613.00 | 6.6% |
| frames | 4728.00 | 2.1% |
| frames_over_33ms | 0.00 | 0.0% |
| frames_over_50ms | 0.00 | 0.0% |
| gpu_bloom_ms | 21.93 | 1.5% |
| gpu_fxaa_ms | 5.04 | 1.2% |
| gpu_hand_ms | 4.54 | 1.2% |
| gpu_post_ms | 4.88 | 1.1% |
| gpu_shadow_ms | 0.29 | 1.2% |
| gpu_shafts_ms | 9.36 | 1.2% |
| gpu_total_ms | 8.06 | 0.8% |
| gpu_ui_ms | 5.04 | 1.2% |
| gpu_water_ms | 5.28 | 1.1% |
| gpu_world_ms | 3.36 | 0.9% |
| light_mesh_ms | 8.29 | 4.3% |
| memory_mib | 709.06 | 1.2% |
| one_percent_low_fps | 46.76 | 4.9% |
| worst_frame_ms | 29.73 | 11.0% |
