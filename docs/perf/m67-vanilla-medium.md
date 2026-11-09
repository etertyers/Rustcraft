# m67-vanilla-medium performance

3 flights of 60 s (seed 20111118, render distance 9), median and spread (max − min as % of the median). Measured on Macmini9,1 (Apple M1), commit df8de7d39cb22178d85b8ded9c09f7cdd96e1997, row `vanilla-medium`.

| Measure | Median | Spread |
|---|---|---|
| average_fps | 80.03 | 1.5% |
| chunk_generation_ms | 8.04 | 0.1% |
| chunks_generated | 1344.00 | 0.0% |
| chunks_meshed | 1601.00 | 5.4% |
| frames | 4803.00 | 1.5% |
| frames_over_33ms | 0.00 | 0.0% |
| frames_over_50ms | 0.00 | 0.0% |
| gpu_bloom_ms | 22.01 | 0.1% |
| gpu_fxaa_ms | 5.01 | 0.5% |
| gpu_hand_ms | 4.51 | 0.5% |
| gpu_post_ms | 4.84 | 0.5% |
| gpu_shadow_ms | 0.29 | 1.1% |
| gpu_shafts_ms | 9.30 | 0.5% |
| gpu_total_ms | 8.02 | 0.6% |
| gpu_ui_ms | 5.00 | 0.5% |
| gpu_water_ms | 5.25 | 0.4% |
| gpu_world_ms | 3.35 | 0.7% |
| light_mesh_ms | 8.29 | 1.6% |
| memory_mib | 708.16 | 0.6% |
| one_percent_low_fps | 47.37 | 1.2% |
| worst_frame_ms | 30.74 | 17.6% |
