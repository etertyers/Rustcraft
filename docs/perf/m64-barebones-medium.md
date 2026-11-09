# m64-barebones-medium performance

3 flights of 60 s (seed 20111118, render distance 9), median and spread (max − min as % of the median). Measured on Macmini9,1 (Apple M1), commit 7219090ac27587bdfc05f557ab617b795189d7cd, row `barebones-medium`.

| Measure | Median | Spread |
|---|---|---|
| average_fps | 77.36 | 0.7% |
| chunk_generation_ms | 7.92 | 4.3% |
| chunks_generated | 1361.00 | 0.3% |
| chunks_meshed | 1642.00 | 2.7% |
| frames | 4642.00 | 0.7% |
| frames_over_33ms | 0.00 | 0.0% |
| frames_over_50ms | 0.00 | 0.0% |
| gpu_bloom_ms | 22.11 | 0.7% |
| gpu_fxaa_ms | 5.05 | 0.8% |
| gpu_hand_ms | 4.55 | 0.8% |
| gpu_post_ms | 4.89 | 0.9% |
| gpu_shadow_ms | 0.30 | 2.2% |
| gpu_shafts_ms | 9.38 | 0.9% |
| gpu_total_ms | 8.10 | 0.7% |
| gpu_ui_ms | 5.05 | 0.8% |
| gpu_water_ms | 5.29 | 0.8% |
| gpu_world_ms | 3.37 | 0.6% |
| light_mesh_ms | 7.87 | 1.8% |
| memory_mib | 715.00 | 3.4% |
| one_percent_low_fps | 45.91 | 2.6% |
| worst_frame_ms | 28.14 | 27.7% |
