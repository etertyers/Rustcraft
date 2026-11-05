# m67-realistic performance

3 flights of 60 s (seed 20111118, render distance 9), median and spread (max − min as % of the median). Measured on Macmini9,1 (Apple M1), commit df8de7d39cb22178d85b8ded9c09f7cdd96e1997, row `realistic`.

| Measure | Median | Spread |
|---|---|---|
| average_fps | 128.73 | 0.3% |
| chunk_generation_ms | 7.82 | 5.2% |
| chunks_generated | 1344.00 | 0.0% |
| chunks_meshed | 1660.00 | 3.2% |
| frames | 7725.00 | 0.3% |
| frames_over_33ms | 0.00 | 0.0% |
| frames_over_50ms | 0.00 | 0.0% |
| gpu_hand_ms | 1.42 | 0.4% |
| gpu_total_ms | 3.90 | 0.3% |
| gpu_ui_ms | 1.36 | 0.5% |
| gpu_world_ms | 3.84 | 0.3% |
| light_mesh_ms | 9.00 | 2.9% |
| memory_mib | 733.78 | 9.5% |
| one_percent_low_fps | 64.97 | 8.0% |
| worst_frame_ms | 22.57 | 60.3% |
