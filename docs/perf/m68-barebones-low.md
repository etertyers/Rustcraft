# m68-barebones-low performance

3 flights of 60 s (seed 20111118, render distance 9), median and spread (max − min as % of the median). Measured on Macmini9,1 (Apple M1), commit fb92c992b9c76caa74d6a7a868b62eb95ff6a302, row `barebones-low`.

| Measure | Median | Spread |
|---|---|---|
| average_fps | 95.57 | 1.9% |
| chunk_generation_ms | 8.09 | 3.0% |
| chunks_generated | 1344.00 | 0.0% |
| chunks_meshed | 1632.00 | 7.2% |
| cpu_acquire_ms | 0.01 | 7.3% |
| cpu_chunks_ms | 0.09 | 7.7% |
| cpu_draw_ms | 9.45 | 1.9% |
| cpu_egui_ms | 0.08 | 3.7% |
| cpu_encode_ms | 0.35 | 3.2% |
| cpu_game_ms | 0.94 | 7.8% |
| cpu_present_ms | 7.15 | 1.1% |
| cpu_submit_ms | 1.21 | 2.0% |
| frames | 5735.00 | 1.9% |
| frames_over_33ms | 0.00 | 0.0% |
| frames_over_50ms | 0.00 | 0.0% |
| gpu_bloom_ms | 11.00 | 1.1% |
| gpu_hand_ms | 2.79 | 1.4% |
| gpu_post_ms | 2.95 | 1.3% |
| gpu_shadow_ms | 0.11 | 2.5% |
| gpu_total_ms | 6.12 | 1.3% |
| gpu_ui_ms | 2.95 | 1.3% |
| gpu_world_ms | 5.69 | 1.1% |
| light_mesh_ms | 8.69 | 6.8% |
| memory_mib | 712.22 | 21.9% |
| missed_frame_pct | 0.03 | 509.5% |
| one_percent_low_fps | 54.79 | 5.4% |
| refresh_hz | 60.00 | 0.0% |
| worst_frame_ms | 27.40 | 69.8% |
