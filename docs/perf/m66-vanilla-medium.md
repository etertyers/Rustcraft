# m66-vanilla-medium performance

3 flights of 60 s (seed 20111118, render distance 9), median and spread (max − min as % of the median). Measured on Macmini9,1 (Apple M1), commit cefb167027aea1cf9c63d84d8ee6148d8c635284, row `vanilla-medium`.

| Measure | Median | Spread |
|---|---|---|
| average_fps | 78.91 | 1.6% |
| chunk_generation_ms | 7.95 | 1.4% |
| chunks_generated | 1344.00 | 0.0% |
| chunks_meshed | 1607.00 | 1.3% |
| frames | 4736.00 | 1.6% |
| frames_over_33ms | 1.00 | 100.0% |
| frames_over_50ms | 0.00 | 0.0% |
| gpu_bloom_ms | 22.02 | 1.3% |
| gpu_fxaa_ms | 5.04 | 0.6% |
| gpu_hand_ms | 4.54 | 0.6% |
| gpu_post_ms | 4.87 | 0.6% |
| gpu_shadow_ms | 0.30 | 2.0% |
| gpu_shafts_ms | 9.35 | 0.6% |
| gpu_total_ms | 8.06 | 0.4% |
| gpu_ui_ms | 5.03 | 0.6% |
| gpu_water_ms | 5.27 | 0.5% |
| gpu_world_ms | 3.36 | 0.6% |
| light_mesh_ms | 7.97 | 0.9% |
| memory_mib | 707.31 | 0.7% |
| one_percent_low_fps | 48.76 | 2.3% |
| worst_frame_ms | 33.90 | 34.7% |
