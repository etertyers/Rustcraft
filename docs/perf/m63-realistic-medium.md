# m58-realistic-medium performance

3 flights of 60 s (seed 20111118, render distance 9), median and spread (max − min as % of the median). Measured on Macmini9,1 (Apple M1), commit 12eeca09487cac0e23bbd5a0ea3378b233a07e9d-dirty, row `realistic-medium`.

| Measure | Median | Spread |
|---|---|---|
| average_fps | 77.80 | 0.9% |
| chunk_generation_ms | 7.98 | 3.6% |
| chunks_generated | 1353.00 | 0.3% |
| chunks_meshed | 1777.00 | 11.8% |
| frames | 4668.00 | 0.9% |
| frames_over_33ms | 0.00 | 0.0% |
| frames_over_50ms | 0.00 | 0.0% |
| gpu_bloom_ms | 22.12 | 0.8% |
| gpu_fxaa_ms | 5.06 | 0.5% |
| gpu_hand_ms | 4.56 | 0.5% |
| gpu_post_ms | 4.89 | 0.5% |
| gpu_shadow_ms | 0.30 | 2.2% |
| gpu_shafts_ms | 9.40 | 0.5% |
| gpu_total_ms | 8.11 | 0.5% |
| gpu_ui_ms | 5.05 | 0.5% |
| gpu_water_ms | 5.30 | 0.5% |
| gpu_world_ms | 3.37 | 0.3% |
| light_mesh_ms | 8.34 | 6.9% |
| memory_mib | 743.95 | 0.8% |
| one_percent_low_fps | 47.13 | 1.3% |
| worst_frame_ms | 29.49 | 10.9% |
