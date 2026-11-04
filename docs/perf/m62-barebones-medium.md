# m62-barebones-medium performance

3 flights of 60 s (seed 20111118, render distance 9), median and spread (max − min as % of the median). Measured on Macmini9,1 (Apple M1), commit 95e8c1357dd30b821783ab0ebcf2e914a120ce6b, row `barebones-medium`.

| Measure | Median | Spread |
|---|---|---|
| average_fps | 76.90 | 0.4% |
| chunk_generation_ms | 7.93 | 2.3% |
| chunks_generated | 1365.00 | 0.0% |
| chunks_meshed | 1632.00 | 2.3% |
| frames | 4615.00 | 0.4% |
| frames_over_33ms | 0.00 | 0.0% |
| frames_over_50ms | 0.00 | 0.0% |
| gpu_bloom_ms | 22.12 | 0.5% |
| gpu_fxaa_ms | 5.06 | 1.3% |
| gpu_hand_ms | 4.56 | 1.3% |
| gpu_post_ms | 4.90 | 1.3% |
| gpu_shadow_ms | 0.30 | 1.2% |
| gpu_shafts_ms | 9.40 | 1.3% |
| gpu_total_ms | 8.11 | 0.7% |
| gpu_ui_ms | 5.06 | 1.3% |
| gpu_water_ms | 5.30 | 1.1% |
| gpu_world_ms | 3.37 | 0.7% |
| light_mesh_ms | 8.10 | 1.3% |
| memory_mib | 697.41 | 0.4% |
| one_percent_low_fps | 44.95 | 2.8% |
| worst_frame_ms | 30.36 | 13.6% |
