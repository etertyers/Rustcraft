# m58-trailer-medium performance

3 flights of 60 s (seed 20111118, render distance 9), median and spread (max − min as % of the median). Measured on Macmini9,1 (Apple M1), commit afafa15eed41d39df48cbfba3d4222ed89eb0b81-dirty, row `trailer-medium`.

| Measure | Median | Spread |
|---|---|---|
| average_fps | 78.32 | 1.1% |
| chunk_generation_ms | 8.86 | 1.5% |
| chunks_generated | 1365.00 | 0.0% |
| chunks_meshed | 1661.00 | 14.0% |
| frames | 4701.00 | 1.1% |
| frames_over_33ms | 0.00 | 0.0% |
| frames_over_50ms | 0.00 | 0.0% |
| gpu_bloom_ms | 20.97 | 1.8% |
| gpu_fxaa_ms | 4.83 | 0.7% |
| gpu_hand_ms | 4.33 | 0.7% |
| gpu_post_ms | 4.67 | 0.7% |
| gpu_shadow_ms | 0.30 | 0.7% |
| gpu_shafts_ms | 8.93 | 0.7% |
| gpu_total_ms | 8.11 | 0.7% |
| gpu_ui_ms | 4.83 | 0.7% |
| gpu_water_ms | 5.14 | 0.6% |
| gpu_world_ms | 3.64 | 0.6% |
| light_mesh_ms | 9.89 | 6.2% |
| memory_mib | 697.69 | 15.5% |
| one_percent_low_fps | 45.55 | 1.0% |
| worst_frame_ms | 25.45 | 3.5% |
