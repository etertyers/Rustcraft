# l-fixes-vanilla-medium performance

3 flights of 60 s (seed 20111118, render distance 9), median and spread (max − min as % of the median). Measured on Macmini9,1 (Apple M1), commit 67a2ec5e51e0bf968721c74ea480172e0a62ecfa, row `vanilla-medium`.

| Measure | Median | Spread |
|---|---|---|
| average_fps | 77.91 | 0.9% |
| chunk_generation_ms | 8.08 | 3.2% |
| chunks_generated | 1365.00 | 0.0% |
| chunks_meshed | 1625.00 | 7.8% |
| frames | 4676.00 | 0.9% |
| frames_over_33ms | 1.00 | 0.0% |
| frames_over_50ms | 0.00 | 0.0% |
| gpu_bloom_ms | 21.98 | 0.6% |
| gpu_fxaa_ms | 5.04 | 0.1% |
| gpu_hand_ms | 4.53 | 0.2% |
| gpu_post_ms | 4.87 | 0.2% |
| gpu_shadow_ms | 0.30 | 1.8% |
| gpu_shafts_ms | 9.34 | 0.2% |
| gpu_total_ms | 8.07 | 0.3% |
| gpu_ui_ms | 5.03 | 0.1% |
| gpu_water_ms | 5.27 | 0.1% |
| gpu_world_ms | 3.36 | 0.1% |
| light_mesh_ms | 8.04 | 6.1% |
| memory_mib | 695.08 | 0.4% |
| one_percent_low_fps | 45.09 | 3.0% |
| worst_frame_ms | 37.75 | 33.8% |
