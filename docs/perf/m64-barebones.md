# m64-barebones performance

3 flights of 60 s (seed 20111118, render distance 9), median and spread (max − min as % of the median). Measured on Macmini9,1 (Apple M1), commit 7219090ac27587bdfc05f557ab617b795189d7cd, row `barebones`.

| Measure | Median | Spread |
|---|---|---|
| average_fps | 124.08 | 0.4% |
| chunk_generation_ms | 7.72 | 1.2% |
| chunks_generated | 1365.00 | 0.0% |
| chunks_meshed | 1773.00 | 13.9% |
| frames | 7445.00 | 0.4% |
| frames_over_33ms | 0.00 | 0.0% |
| frames_over_50ms | 0.00 | 0.0% |
| gpu_hand_ms | 1.41 | 0.2% |
| gpu_total_ms | 3.91 | 0.5% |
| gpu_ui_ms | 1.36 | 0.2% |
| gpu_world_ms | 3.85 | 0.4% |
| light_mesh_ms | 9.05 | 7.9% |
| memory_mib | 692.56 | 2.3% |
| one_percent_low_fps | 58.63 | 9.0% |
| worst_frame_ms | 25.09 | 48.2% |
