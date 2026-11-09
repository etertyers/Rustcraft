# m58-vanilla-medium performance

3 flights of 60 s (seed 20111118, render distance 9), median and spread (max − min as % of the median). Measured on Macmini9,1 (Apple M1), commit 12eeca09487cac0e23bbd5a0ea3378b233a07e9d-dirty, row `vanilla-medium`.

| Measure | Median | Spread |
|---|---|---|
| average_fps | 77.99 | 1.6% |
| chunk_generation_ms | 7.85 | 2.5% |
| chunks_generated | 1357.00 | 0.6% |
| chunks_meshed | 1706.00 | 10.9% |
| frames | 4681.00 | 1.6% |
| frames_over_33ms | 0.00 | 0.0% |
| frames_over_50ms | 0.00 | 0.0% |
| gpu_bloom_ms | 22.00 | 1.0% |
| gpu_fxaa_ms | 5.05 | 1.0% |
| gpu_hand_ms | 4.54 | 1.0% |
| gpu_post_ms | 4.88 | 1.0% |
| gpu_shadow_ms | 0.30 | 1.7% |
| gpu_shafts_ms | 9.37 | 1.0% |
| gpu_total_ms | 8.10 | 0.9% |
| gpu_ui_ms | 5.04 | 1.0% |
| gpu_water_ms | 5.28 | 0.9% |
| gpu_world_ms | 3.36 | 0.5% |
| light_mesh_ms | 8.09 | 6.1% |
| memory_mib | 702.94 | 0.7% |
| one_percent_low_fps | 45.67 | 3.5% |
| worst_frame_ms | 31.12 | 6.7% |
