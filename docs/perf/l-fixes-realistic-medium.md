# l-fixes-realistic-medium performance

3 flights of 60 s (seed 20111118, render distance 9), median and spread (max − min as % of the median). Measured on Macmini9,1 (Apple M1), commit 67a2ec5e51e0bf968721c74ea480172e0a62ecfa, row `realistic-medium`.

| Measure | Median | Spread |
|---|---|---|
| average_fps | 77.10 | 0.6% |
| chunk_generation_ms | 7.94 | 1.8% |
| chunks_generated | 1365.00 | 0.0% |
| chunks_meshed | 1706.00 | 5.4% |
| frames | 4627.00 | 0.6% |
| frames_over_33ms | 1.00 | 0.0% |
| frames_over_50ms | 0.00 | 0.0% |
| gpu_bloom_ms | 22.20 | 1.2% |
| gpu_fxaa_ms | 5.10 | 0.3% |
| gpu_hand_ms | 4.59 | 0.4% |
| gpu_post_ms | 4.93 | 0.4% |
| gpu_shadow_ms | 0.31 | 1.1% |
| gpu_shafts_ms | 9.47 | 0.4% |
| gpu_total_ms | 8.15 | 0.4% |
| gpu_ui_ms | 5.09 | 0.3% |
| gpu_water_ms | 5.33 | 0.4% |
| gpu_world_ms | 3.38 | 0.5% |
| light_mesh_ms | 8.29 | 3.7% |
| memory_mib | 729.64 | 0.9% |
| one_percent_low_fps | 46.12 | 2.8% |
| worst_frame_ms | 36.96 | 37.3% |
