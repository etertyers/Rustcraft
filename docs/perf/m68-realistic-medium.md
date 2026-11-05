# m68-realistic-medium performance

3 flights of 60 s (seed 20111118, render distance 9), median and spread (max − min as % of the median). Measured on Macmini9,1 (Apple M1), commit fb92c992b9c76caa74d6a7a868b62eb95ff6a302, row `realistic-medium`.

| Measure | Median | Spread |
|---|---|---|
| average_fps | 79.44 | 0.7% |
| chunk_generation_ms | 7.98 | 0.9% |
| chunks_generated | 1344.00 | 0.0% |
| chunks_meshed | 1633.00 | 2.3% |
| cpu_acquire_ms | 0.01 | 3.2% |
| cpu_chunks_ms | 0.10 | 4.7% |
| cpu_draw_ms | 11.60 | 0.5% |
| cpu_egui_ms | 0.08 | 3.4% |
| cpu_encode_ms | 0.39 | 3.4% |
| cpu_game_ms | 0.85 | 3.6% |
| cpu_present_ms | 8.88 | 0.4% |
| cpu_submit_ms | 1.63 | 3.0% |
| frames | 4767.00 | 0.7% |
| frames_over_33ms | 0.00 | 0.0% |
| frames_over_50ms | 0.00 | 0.0% |
| gpu_bloom_ms | 21.93 | 0.6% |
| gpu_fxaa_ms | 5.03 | 0.2% |
| gpu_hand_ms | 4.53 | 0.1% |
| gpu_post_ms | 4.87 | 0.1% |
| gpu_shadow_ms | 0.30 | 1.2% |
| gpu_shafts_ms | 9.34 | 0.1% |
| gpu_total_ms | 8.06 | 0.2% |
| gpu_ui_ms | 5.03 | 0.2% |
| gpu_water_ms | 5.28 | 0.1% |
| gpu_world_ms | 3.36 | 0.3% |
| light_mesh_ms | 8.20 | 3.2% |
| memory_mib | 751.53 | 1.1% |
| missed_frame_pct | 0.17 | 25.5% |
| one_percent_low_fps | 47.92 | 3.8% |
| refresh_hz | 60.00 | 0.0% |
| worst_frame_ms | 30.95 | 9.1% |
