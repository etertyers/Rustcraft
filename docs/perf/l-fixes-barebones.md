# l-fixes-barebones performance

3 flights of 60 s (seed 20111118, render distance 9), median and spread (max − min as % of the median). Measured on Macmini9,1 (Apple M1), commit 67a2ec5e51e0bf968721c74ea480172e0a62ecfa, row `barebones`.

| Measure | Median | Spread |
|---|---|---|
| average_fps | 121.82 | 0.7% |
| chunk_generation_ms | 7.97 | 5.8% |
| chunks_generated | 1365.00 | 0.0% |
| chunks_meshed | 1689.00 | 11.2% |
| frames | 7310.00 | 0.7% |
| frames_over_33ms | 0.00 | 0.0% |
| frames_over_50ms | 0.00 | 0.0% |
| gpu_hand_ms | 1.42 | 0.8% |
| gpu_total_ms | 3.93 | 0.5% |
| gpu_ui_ms | 1.37 | 0.8% |
| gpu_world_ms | 3.87 | 0.5% |
| light_mesh_ms | 8.90 | 7.5% |
| memory_mib | 713.17 | 15.6% |
| one_percent_low_fps | 62.67 | 1.9% |
| worst_frame_ms | 27.39 | 6.7% |
