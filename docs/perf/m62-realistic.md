# m62-realistic performance

3 flights of 60 s (seed 20111118, render distance 9), median and spread (max − min as % of the median). Measured on Macmini9,1 (Apple M1), commit 95e8c1357dd30b821783ab0ebcf2e914a120ce6b, row `realistic`.

| Measure | Median | Spread |
|---|---|---|
| average_fps | 122.42 | 1.3% |
| chunk_generation_ms | 7.73 | 2.9% |
| chunks_generated | 1365.00 | 0.0% |
| chunks_meshed | 1694.00 | 8.7% |
| frames | 7346.00 | 1.3% |
| frames_over_33ms | 0.00 | 0.0% |
| frames_over_50ms | 0.00 | 0.0% |
| gpu_hand_ms | 1.42 | 0.9% |
| gpu_total_ms | 3.94 | 0.3% |
| gpu_ui_ms | 1.37 | 1.0% |
| gpu_world_ms | 3.88 | 0.5% |
| light_mesh_ms | 8.90 | 6.4% |
| memory_mib | 632.06 | 1.5% |
| one_percent_low_fps | 61.82 | 7.1% |
| worst_frame_ms | 32.29 | 30.2% |
