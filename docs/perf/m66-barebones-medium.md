# m66-barebones-medium performance

3 flights of 60 s (seed 20111118, render distance 9), median and spread (max − min as % of the median). Measured on Macmini9,1 (Apple M1), commit cefb167027aea1cf9c63d84d8ee6148d8c635284, row `barebones-medium`.

| Measure | Median | Spread |
|---|---|---|
| average_fps | 78.31 | 1.4% |
| chunk_generation_ms | 7.96 | 6.0% |
| chunks_generated | 1344.00 | 0.0% |
| chunks_meshed | 1636.00 | 13.2% |
| frames | 4700.00 | 1.4% |
| frames_over_33ms | 0.00 | 0.0% |
| frames_over_50ms | 0.00 | 0.0% |
| gpu_bloom_ms | 21.97 | 0.3% |
| gpu_fxaa_ms | 5.04 | 0.8% |
| gpu_hand_ms | 4.54 | 0.8% |
| gpu_post_ms | 4.87 | 0.7% |
| gpu_shadow_ms | 0.30 | 1.3% |
| gpu_shafts_ms | 9.35 | 0.7% |
| gpu_total_ms | 8.06 | 0.6% |
| gpu_ui_ms | 5.03 | 0.7% |
| gpu_water_ms | 5.28 | 0.7% |
| gpu_world_ms | 3.36 | 0.5% |
| light_mesh_ms | 8.07 | 4.1% |
| memory_mib | 711.14 | 0.8% |
| one_percent_low_fps | 47.75 | 2.2% |
| worst_frame_ms | 33.12 | 6.4% |
