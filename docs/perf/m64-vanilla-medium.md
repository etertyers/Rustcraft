# m64-vanilla-medium performance

3 flights of 60 s (seed 20111118, render distance 9), median and spread (max − min as % of the median). Measured on Macmini9,1 (Apple M1), commit 7219090ac27587bdfc05f557ab617b795189d7cd, row `vanilla-medium`.

| Measure | Median | Spread |
|---|---|---|
| average_fps | 77.46 | 0.7% |
| chunk_generation_ms | 7.89 | 5.6% |
| chunks_generated | 1365.00 | 0.0% |
| chunks_meshed | 1618.00 | 3.8% |
| frames | 4649.00 | 0.7% |
| frames_over_33ms | 0.00 | 0.0% |
| frames_over_50ms | 0.00 | 0.0% |
| gpu_bloom_ms | 22.04 | 0.4% |
| gpu_fxaa_ms | 5.05 | 0.3% |
| gpu_hand_ms | 4.54 | 0.3% |
| gpu_post_ms | 4.88 | 0.3% |
| gpu_shadow_ms | 0.30 | 1.0% |
| gpu_shafts_ms | 9.37 | 0.2% |
| gpu_total_ms | 8.10 | 0.3% |
| gpu_ui_ms | 5.04 | 0.3% |
| gpu_water_ms | 5.29 | 0.2% |
| gpu_world_ms | 3.37 | 0.4% |
| light_mesh_ms | 8.06 | 4.3% |
| memory_mib | 711.03 | 1.2% |
| one_percent_low_fps | 45.30 | 1.6% |
| worst_frame_ms | 30.23 | 17.3% |
