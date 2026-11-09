# m66-realistic performance

3 flights of 60 s (seed 20111118, render distance 9), median and spread (max − min as % of the median). Measured on Macmini9,1 (Apple M1), commit cefb167027aea1cf9c63d84d8ee6148d8c635284, row `realistic`.

| Measure | Median | Spread |
|---|---|---|
| average_fps | 124.42 | 1.9% |
| chunk_generation_ms | 8.03 | 3.6% |
| chunks_generated | 1344.00 | 0.0% |
| chunks_meshed | 1605.00 | 12.6% |
| frames | 7467.00 | 1.9% |
| frames_over_33ms | 0.00 | 0.0% |
| frames_over_50ms | 0.00 | 0.0% |
| gpu_hand_ms | 1.42 | 0.9% |
| gpu_total_ms | 3.94 | 0.7% |
| gpu_ui_ms | 1.37 | 1.0% |
| gpu_world_ms | 3.87 | 0.7% |
| light_mesh_ms | 8.59 | 5.1% |
| memory_mib | 735.64 | 1.3% |
| one_percent_low_fps | 64.54 | 3.4% |
| worst_frame_ms | 28.29 | 38.0% |
