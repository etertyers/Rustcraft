# m68-barebones-high performance

3 flights of 60 s (seed 20111118, render distance 9), median and spread (max − min as % of the median). Measured on Macmini9,1 (Apple M1), commit fb92c992b9c76caa74d6a7a868b62eb95ff6a302, row `barebones-high`.

| Measure | Median | Spread |
|---|---|---|
| average_fps | 60.76 | 0.9% |
| chunk_generation_ms | 8.17 | 3.4% |
| chunks_generated | 1344.00 | 0.0% |
| chunks_meshed | 1632.00 | 13.2% |
| cpu_acquire_ms | 0.01 | 3.8% |
| cpu_chunks_ms | 0.13 | 21.9% |
| cpu_draw_ms | 15.03 | 0.9% |
| cpu_egui_ms | 0.09 | 3.0% |
| cpu_encode_ms | 0.49 | 2.7% |
| cpu_game_ms | 1.22 | 8.3% |
| cpu_present_ms | 11.87 | 0.2% |
| cpu_submit_ms | 1.94 | 2.4% |
| frames | 3647.00 | 0.9% |
| frames_over_33ms | 2.00 | 150.0% |
| frames_over_50ms | 0.00 | 0.0% |
| gpu_bloom_ms | 33.51 | 0.2% |
| gpu_fxaa_ms | 7.81 | 0.0% |
| gpu_hand_ms | 6.67 | 0.1% |
| gpu_post_ms | 7.66 | 0.1% |
| gpu_shadow_ms | 0.69 | 2.0% |
| gpu_shafts_ms | 14.97 | 0.1% |
| gpu_total_ms | 11.01 | 0.2% |
| gpu_ui_ms | 7.80 | 0.0% |
| gpu_water_ms | 7.47 | 0.1% |
| gpu_world_ms | 4.34 | 0.1% |
| light_mesh_ms | 8.42 | 5.9% |
| memory_mib | 715.77 | 0.3% |
| missed_frame_pct | 0.98 | 111.2% |
| one_percent_low_fps | 40.12 | 5.4% |
| refresh_hz | 60.00 | 0.0% |
| worst_frame_ms | 39.67 | 47.5% |
