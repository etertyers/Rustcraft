# m56-trailer-medium performance

3 flights of 60 s (seed 20111118, render distance 9), median and spread (max − min as % of the median). Measured on Macmini9,1 (Apple M1), commit 53ca9990039d91eed67843cb75cd89bd0122bcb1-dirty, row `trailer-medium`.

| Measure | Median | Spread |
|---|---|---|
| average_fps | 98.91 | 0.5% |
| chunk_generation_ms | 9.00 | 4.5% |
| chunks_generated | 1365.00 | 0.0% |
| chunks_meshed | 1657.00 | 9.8% |
| frames | 5935.00 | 0.5% |
| frames_over_33ms | 0.00 | 0.0% |
| frames_over_50ms | 0.00 | 0.0% |
| gpu_hand_ms | 4.11 | 0.4% |
| gpu_post_ms | 4.18 | 0.4% |
| gpu_shadow_ms | 0.26 | 1.1% |
| gpu_total_ms | 7.23 | 0.2% |
| gpu_ui_ms | 4.18 | 0.4% |
| gpu_water_ms | 4.91 | 0.3% |
| gpu_world_ms | 3.47 | 0.5% |
| light_mesh_ms | 9.92 | 3.2% |
| memory_mib | 588.20 | 5.3% |
| one_percent_low_fps | 68.49 | 1.2% |
| worst_frame_ms | 17.76 | 49.2% |
