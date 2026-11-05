# m64-realistic performance

3 flights of 60 s (seed 20111118, render distance 9), median and spread (max − min as % of the median). Measured on Macmini9,1 (Apple M1), commit 7219090ac27587bdfc05f557ab617b795189d7cd, row `realistic`.

| Measure | Median | Spread |
|---|---|---|
| average_fps | 121.86 | 1.3% |
| chunk_generation_ms | 7.93 | 6.3% |
| chunks_generated | 1365.00 | 0.4% |
| chunks_meshed | 1694.00 | 7.1% |
| frames | 7312.00 | 1.3% |
| frames_over_33ms | 0.00 | 0.0% |
| frames_over_50ms | 0.00 | 0.0% |
| gpu_hand_ms | 1.43 | 1.0% |
| gpu_total_ms | 3.95 | 1.5% |
| gpu_ui_ms | 1.37 | 1.0% |
| gpu_world_ms | 3.89 | 1.4% |
| light_mesh_ms | 8.64 | 3.2% |
| memory_mib | 737.30 | 0.7% |
| one_percent_low_fps | 61.08 | 4.0% |
| worst_frame_ms | 24.70 | 24.1% |
