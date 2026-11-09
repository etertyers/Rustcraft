# m61-trailer-medium performance

3 flights of 60 s (seed 20111118, render distance 9), median and spread (max − min as % of the median). Measured on Macmini9,1 (Apple M1), commit cd8b86d0d50421800e82f9e1f8e17796f4a15d49-dirty, row `trailer-medium`.

| Measure | Median | Spread |
|---|---|---|
| average_fps | 78.27 | 1.9% |
| chunk_generation_ms | 7.91 | 0.3% |
| chunks_generated | 1365.00 | 0.0% |
| chunks_meshed | 1615.00 | 5.7% |
| frames | 4697.00 | 1.9% |
| frames_over_33ms | 1.00 | 100.0% |
| frames_over_50ms | 0.00 | 0.0% |
| gpu_bloom_ms | 22.08 | 1.1% |
| gpu_fxaa_ms | 5.03 | 2.0% |
| gpu_hand_ms | 4.53 | 1.9% |
| gpu_post_ms | 4.87 | 2.0% |
| gpu_shadow_ms | 0.30 | 1.3% |
| gpu_shafts_ms | 9.34 | 1.9% |
| gpu_total_ms | 8.06 | 1.7% |
| gpu_ui_ms | 5.02 | 1.9% |
| gpu_water_ms | 5.27 | 1.6% |
| gpu_world_ms | 3.36 | 0.8% |
| light_mesh_ms | 7.95 | 3.2% |
| memory_mib | 689.03 | 9.7% |
| one_percent_low_fps | 45.11 | 3.6% |
| worst_frame_ms | 40.69 | 28.1% |
