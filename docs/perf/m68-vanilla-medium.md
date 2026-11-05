# m68-vanilla-medium performance

3 flights of 60 s (seed 20111118, render distance 9), median and spread (max − min as % of the median). Measured on Macmini9,1 (Apple M1), commit fb92c992b9c76caa74d6a7a868b62eb95ff6a302, row `vanilla-medium`.

| Measure | Median | Spread |
|---|---|---|
| average_fps | 81.40 | 2.7% |
| chunk_generation_ms | 7.88 | 5.5% |
| chunks_generated | 1344.00 | 0.0% |
| chunks_meshed | 1637.00 | 9.5% |
| cpu_acquire_ms | 0.01 | 2.4% |
| cpu_chunks_ms | 0.09 | 5.9% |
| cpu_draw_ms | 11.32 | 2.5% |
| cpu_egui_ms | 0.07 | 2.8% |
| cpu_encode_ms | 0.39 | 5.1% |
| cpu_game_ms | 0.89 | 11.9% |
| cpu_present_ms | 8.74 | 1.6% |
| cpu_submit_ms | 1.60 | 7.7% |
| frames | 4885.00 | 2.7% |
| frames_over_33ms | 0.00 | 0.0% |
| frames_over_50ms | 0.00 | 0.0% |
| gpu_bloom_ms | 21.82 | 1.3% |
| gpu_fxaa_ms | 4.94 | 1.7% |
| gpu_hand_ms | 4.45 | 1.7% |
| gpu_post_ms | 4.77 | 1.7% |
| gpu_shadow_ms | 0.29 | 3.1% |
| gpu_shafts_ms | 9.16 | 1.7% |
| gpu_total_ms | 7.94 | 1.6% |
| gpu_ui_ms | 4.93 | 1.7% |
| gpu_water_ms | 5.18 | 1.5% |
| gpu_world_ms | 3.33 | 1.2% |
| light_mesh_ms | 8.33 | 3.3% |
| memory_mib | 711.55 | 0.4% |
| missed_frame_pct | 0.14 | 74.1% |
| one_percent_low_fps | 46.84 | 6.5% |
| refresh_hz | 60.00 | 0.0% |
| worst_frame_ms | 28.94 | 6.2% |
