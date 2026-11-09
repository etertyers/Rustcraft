# l-fixes-barebones-medium performance

3 flights of 60 s (seed 20111118, render distance 9), median and spread (max − min as % of the median). Measured on Macmini9,1 (Apple M1), commit 67a2ec5e51e0bf968721c74ea480172e0a62ecfa, row `barebones-medium`.

| Measure | Median | Spread |
|---|---|---|
| average_fps | 76.64 | 2.5% |
| chunk_generation_ms | 7.87 | 6.6% |
| chunks_generated | 1365.00 | 0.6% |
| chunks_meshed | 1621.00 | 20.0% |
| frames | 4599.00 | 2.5% |
| frames_over_33ms | 1.00 | 2000.0% |
| frames_over_50ms | 0.00 | 0.0% |
| gpu_bloom_ms | 22.09 | 2.2% |
| gpu_fxaa_ms | 5.10 | 1.6% |
| gpu_hand_ms | 4.60 | 1.5% |
| gpu_post_ms | 4.93 | 1.5% |
| gpu_shadow_ms | 0.31 | 0.7% |
| gpu_shafts_ms | 9.47 | 1.5% |
| gpu_total_ms | 8.19 | 1.7% |
| gpu_ui_ms | 5.09 | 1.5% |
| gpu_water_ms | 5.34 | 1.4% |
| gpu_world_ms | 3.39 | 1.0% |
| light_mesh_ms | 8.03 | 5.7% |
| memory_mib | 731.56 | 0.4% |
| one_percent_low_fps | 44.64 | 14.4% |
| worst_frame_ms | 36.23 | 50.2% |
