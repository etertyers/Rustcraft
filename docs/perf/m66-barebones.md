# m66-barebones performance

3 flights of 60 s (seed 20111118, render distance 9), median and spread (max − min as % of the median). Measured on Macmini9,1 (Apple M1), commit cefb167027aea1cf9c63d84d8ee6148d8c635284, row `barebones`.

| Measure | Median | Spread |
|---|---|---|
| average_fps | 125.10 | 1.5% |
| chunk_generation_ms | 8.07 | 0.8% |
| chunks_generated | 1344.00 | 0.0% |
| chunks_meshed | 1615.00 | 2.0% |
| frames | 7507.00 | 1.4% |
| frames_over_33ms | 0.00 | 0.0% |
| frames_over_50ms | 0.00 | 0.0% |
| gpu_hand_ms | 1.42 | 0.3% |
| gpu_total_ms | 3.92 | 0.7% |
| gpu_ui_ms | 1.37 | 0.4% |
| gpu_world_ms | 3.85 | 0.7% |
| light_mesh_ms | 8.53 | 2.1% |
| memory_mib | 626.84 | 2.0% |
| one_percent_low_fps | 65.16 | 0.6% |
| worst_frame_ms | 28.07 | 43.3% |
