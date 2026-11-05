# l-fixes-realistic performance

3 flights of 60 s (seed 20111118, render distance 9), median and spread (max − min as % of the median). Measured on Macmini9,1 (Apple M1), commit 67a2ec5e51e0bf968721c74ea480172e0a62ecfa, row `realistic`.

| Measure | Median | Spread |
|---|---|---|
| average_fps | 122.59 | 1.2% |
| chunk_generation_ms | 8.00 | 2.6% |
| chunks_generated | 1365.00 | 0.0% |
| chunks_meshed | 1732.00 | 10.4% |
| frames | 7357.00 | 1.2% |
| frames_over_33ms | 0.00 | 0.0% |
| frames_over_50ms | 0.00 | 0.0% |
| gpu_hand_ms | 1.43 | 1.6% |
| gpu_total_ms | 3.95 | 0.8% |
| gpu_ui_ms | 1.37 | 1.7% |
| gpu_world_ms | 3.88 | 0.6% |
| light_mesh_ms | 9.06 | 8.2% |
| memory_mib | 613.20 | 17.9% |
| one_percent_low_fps | 58.22 | 2.0% |
| worst_frame_ms | 30.32 | 17.4% |
