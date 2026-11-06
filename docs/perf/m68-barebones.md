# m68-barebones performance

3 flights of 60 s (seed 20111118, render distance 9), median and spread (max − min as % of the median). Measured on Macmini9,1 (Apple M1), commit fb92c992b9c76caa74d6a7a868b62eb95ff6a302, row `barebones`.

| Measure | Median | Spread |
|---|---|---|
| average_fps | 128.48 | 1.6% |
| chunk_generation_ms | 7.92 | 3.1% |
| chunks_generated | 1344.00 | 0.0% |
| chunks_meshed | 1688.00 | 3.8% |
| cpu_acquire_ms | 0.01 | 4.4% |
| cpu_chunks_ms | 0.07 | 10.1% |
| cpu_draw_ms | 6.92 | 1.3% |
| cpu_egui_ms | 0.07 | 1.9% |
| cpu_encode_ms | 0.28 | 4.1% |
| cpu_game_ms | 0.76 | 3.5% |
| cpu_present_ms | 4.94 | 0.6% |
| cpu_submit_ms | 0.93 | 5.7% |
| frames | 7709.00 | 1.6% |
| frames_over_33ms | 0.00 | 0.0% |
| frames_over_50ms | 0.00 | 0.0% |
| gpu_hand_ms | 1.42 | 1.1% |
| gpu_total_ms | 3.90 | 0.5% |
| gpu_ui_ms | 1.36 | 1.2% |
| gpu_world_ms | 3.84 | 0.5% |
| light_mesh_ms | 8.92 | 2.1% |
| memory_mib | 704.98 | 11.1% |
| missed_frame_pct | 0.03 | 99.2% |
| one_percent_low_fps | 65.67 | 1.2% |
| refresh_hz | 60.00 | 0.0% |
| worst_frame_ms | 32.17 | 38.8% |
