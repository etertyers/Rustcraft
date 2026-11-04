# m62-vanilla-medium performance

3 flights of 60 s (seed 20111118, render distance 9), median and spread (max − min as % of the median). Measured on Macmini9,1 (Apple M1), commit 95e8c1357dd30b821783ab0ebcf2e914a120ce6b, row `vanilla-medium`.

| Measure | Median | Spread |
|---|---|---|
| average_fps | 77.24 | 0.6% |
| chunk_generation_ms | 7.90 | 2.7% |
| chunks_generated | 1365.00 | 0.0% |
| chunks_meshed | 1658.00 | 3.6% |
| frames | 4635.00 | 0.6% |
| frames_over_33ms | 0.00 | 0.0% |
| frames_over_50ms | 0.00 | 0.0% |
| gpu_bloom_ms | 22.07 | 1.2% |
| gpu_fxaa_ms | 5.07 | 0.5% |
| gpu_hand_ms | 4.56 | 0.5% |
| gpu_post_ms | 4.90 | 0.4% |
| gpu_shadow_ms | 0.30 | 0.6% |
| gpu_shafts_ms | 9.40 | 0.5% |
| gpu_total_ms | 8.11 | 0.5% |
| gpu_ui_ms | 5.06 | 0.5% |
| gpu_water_ms | 5.30 | 0.4% |
| gpu_world_ms | 3.38 | 0.4% |
| light_mesh_ms | 8.09 | 3.4% |
| memory_mib | 695.02 | 0.8% |
| one_percent_low_fps | 44.81 | 0.6% |
| worst_frame_ms | 30.98 | 61.7% |
