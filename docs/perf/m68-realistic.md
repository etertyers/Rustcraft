# m68-realistic performance

3 flights of 60 s (seed 20111118, render distance 9), median and spread (max − min as % of the median). Measured on Macmini9,1 (Apple M1), commit fb92c992b9c76caa74d6a7a868b62eb95ff6a302, row `realistic`.

| Measure | Median | Spread |
|---|---|---|
| average_fps | 128.65 | 1.7% |
| chunk_generation_ms | 8.10 | 5.6% |
| chunks_generated | 1344.00 | 0.0% |
| chunks_meshed | 1629.00 | 3.1% |
| cpu_acquire_ms | 0.01 | 2.6% |
| cpu_chunks_ms | 0.07 | 8.2% |
| cpu_draw_ms | 6.96 | 1.7% |
| cpu_egui_ms | 0.07 | 5.5% |
| cpu_encode_ms | 0.28 | 5.4% |
| cpu_game_ms | 0.74 | 8.3% |
| cpu_present_ms | 4.93 | 0.4% |
| cpu_submit_ms | 0.93 | 3.8% |
| frames | 7720.00 | 1.7% |
| frames_over_33ms | 0.00 | 0.0% |
| frames_over_50ms | 0.00 | 0.0% |
| gpu_hand_ms | 1.42 | 0.1% |
| gpu_total_ms | 3.89 | 0.4% |
| gpu_ui_ms | 1.37 | 0.3% |
| gpu_world_ms | 3.83 | 0.4% |
| light_mesh_ms | 8.79 | 4.1% |
| memory_mib | 738.17 | 3.5% |
| missed_frame_pct | 0.01 | 101.7% |
| one_percent_low_fps | 62.06 | 5.3% |
| refresh_hz | 60.00 | 0.0% |
| worst_frame_ms | 26.35 | 65.6% |
