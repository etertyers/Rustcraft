# l-fixes-barebones-low performance

3 flights of 60 s (seed 20111118, render distance 9), median and spread (max − min as % of the median). Measured on Macmini9,1 (Apple M1), commit 67a2ec5e51e0bf968721c74ea480172e0a62ecfa, row `barebones-low`.

| Measure | Median | Spread |
|---|---|---|
| average_fps | 91.04 | 0.8% |
| chunk_generation_ms | 7.89 | 0.8% |
| chunks_generated | 1365.00 | 0.0% |
| chunks_meshed | 1659.00 | 8.4% |
| frames | 5463.00 | 0.8% |
| frames_over_33ms | 0.00 | 0.0% |
| frames_over_50ms | 0.00 | 0.0% |
| gpu_bloom_ms | 11.13 | 1.7% |
| gpu_hand_ms | 2.84 | 1.1% |
| gpu_post_ms | 3.00 | 1.0% |
| gpu_shadow_ms | 0.12 | 3.0% |
| gpu_total_ms | 6.21 | 0.6% |
| gpu_ui_ms | 2.99 | 1.0% |
| gpu_world_ms | 5.76 | 0.8% |
| light_mesh_ms | 8.53 | 1.8% |
| memory_mib | 724.31 | 13.9% |
| one_percent_low_fps | 53.70 | 0.4% |
| worst_frame_ms | 30.32 | 44.7% |
