# m68-barebones-medium performance

3 flights of 60 s (seed 20111118, render distance 9), median and spread (max − min as % of the median). Measured on Macmini9,1 (Apple M1), commit fb92c992b9c76caa74d6a7a868b62eb95ff6a302, row `barebones-medium`.

| Measure | Median | Spread |
|---|---|---|
| average_fps | 79.97 | 2.4% |
| chunk_generation_ms | 8.09 | 2.1% |
| chunks_generated | 1344.00 | 0.0% |
| chunks_meshed | 1616.00 | 9.2% |
| cpu_acquire_ms | 0.01 | 0.5% |
| cpu_chunks_ms | 0.10 | 11.1% |
| cpu_draw_ms | 11.50 | 2.6% |
| cpu_egui_ms | 0.08 | 4.4% |
| cpu_encode_ms | 0.39 | 5.9% |
| cpu_game_ms | 0.87 | 1.3% |
| cpu_present_ms | 8.82 | 0.8% |
| cpu_submit_ms | 1.63 | 4.8% |
| frames | 4799.00 | 2.4% |
| frames_over_33ms | 0.00 | 0.0% |
| frames_over_50ms | 0.00 | 0.0% |
| gpu_bloom_ms | 21.88 | 0.5% |
| gpu_fxaa_ms | 4.99 | 0.7% |
| gpu_hand_ms | 4.49 | 0.7% |
| gpu_post_ms | 4.82 | 0.7% |
| gpu_shadow_ms | 0.29 | 2.3% |
| gpu_shafts_ms | 9.25 | 0.7% |
| gpu_total_ms | 7.99 | 0.8% |
| gpu_ui_ms | 4.98 | 0.7% |
| gpu_water_ms | 5.23 | 0.7% |
| gpu_world_ms | 3.34 | 0.6% |
| light_mesh_ms | 8.31 | 1.9% |
| memory_mib | 712.05 | 0.8% |
| missed_frame_pct | 0.15 | 152.8% |
| one_percent_low_fps | 47.42 | 0.6% |
| refresh_hz | 60.00 | 0.0% |
| worst_frame_ms | 31.57 | 12.7% |
