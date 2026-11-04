# m59-trailer-medium performance

3 flights of 60 s (seed 20111118, render distance 9), median and spread (max − min as % of the median). Measured on Macmini9,1 (Apple M1), commit d0c6c5bc3d85c0e42c8808a510908ca1bd3812ef-dirty, row `trailer-medium`.

| Measure | Median | Spread |
|---|---|---|
| average_fps | 78.68 | 1.3% |
| chunk_generation_ms | 7.98 | 1.7% |
| chunks_generated | 1365.00 | 0.0% |
| chunks_meshed | 1709.00 | 9.7% |
| frames | 4722.00 | 1.3% |
| frames_over_33ms | 0.00 | 0.0% |
| frames_over_50ms | 0.00 | 0.0% |
| gpu_bloom_ms | 21.89 | 1.0% |
| gpu_fxaa_ms | 5.01 | 0.7% |
| gpu_hand_ms | 4.51 | 0.7% |
| gpu_post_ms | 4.84 | 0.7% |
| gpu_shadow_ms | 0.30 | 0.7% |
| gpu_shafts_ms | 9.30 | 0.7% |
| gpu_total_ms | 8.03 | 0.7% |
| gpu_ui_ms | 5.00 | 0.7% |
| gpu_water_ms | 5.25 | 0.7% |
| gpu_world_ms | 3.35 | 0.4% |
| light_mesh_ms | 8.78 | 4.4% |
| memory_mib | 633.50 | 17.5% |
| one_percent_low_fps | 44.80 | 1.3% |
| worst_frame_ms | 28.37 | 59.0% |
