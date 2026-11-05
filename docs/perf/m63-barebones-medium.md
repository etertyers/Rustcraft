# m58-barebones-medium performance

3 flights of 60 s (seed 20111118, render distance 9), median and spread (max − min as % of the median). Measured on Macmini9,1 (Apple M1), commit 12eeca09487cac0e23bbd5a0ea3378b233a07e9d-dirty, row `barebones-medium`.

| Measure | Median | Spread |
|---|---|---|
| average_fps | 77.66 | 1.4% |
| chunk_generation_ms | 7.97 | 4.1% |
| chunks_generated | 1365.00 | 0.3% |
| chunks_meshed | 1614.00 | 2.2% |
| frames | 4661.00 | 1.4% |
| frames_over_33ms | 1.00 | 200.0% |
| frames_over_50ms | 0.00 | 0.0% |
| gpu_bloom_ms | 22.10 | 1.0% |
| gpu_fxaa_ms | 5.05 | 0.6% |
| gpu_hand_ms | 4.55 | 0.6% |
| gpu_post_ms | 4.88 | 0.6% |
| gpu_shadow_ms | 0.30 | 2.8% |
| gpu_shafts_ms | 9.37 | 0.6% |
| gpu_total_ms | 8.10 | 0.5% |
| gpu_ui_ms | 5.04 | 0.6% |
| gpu_water_ms | 5.29 | 0.6% |
| gpu_world_ms | 3.36 | 0.7% |
| light_mesh_ms | 7.89 | 2.0% |
| memory_mib | 715.25 | 1.4% |
| one_percent_low_fps | 47.42 | 7.9% |
| worst_frame_ms | 34.67 | 17.3% |
