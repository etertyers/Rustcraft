# m68-barebones-medium performance

3 flights of 60 s (seed 20111118, render distance 9), median and spread (max − min as % of the median). Measured on Macmini9,1 (Apple M1), commit 3826713a3915b64a137cd1d20f67b57bb2e86759-dirty, row `barebones-medium`.

| Measure | Median | Spread |
|---|---|---|
| average_fps | 82.33 | 2.4% |
| chunk_generation_ms | 8.01 | 0.9% |
| chunks_generated | 1344.00 | 0.0% |
| chunks_meshed | 1667.00 | 9.0% |
| cpu_acquire_ms | 0.01 | 6.4% |
| cpu_chunks_ms | 0.09 | 6.5% |
| cpu_draw_ms | 11.10 | 1.8% |
| cpu_egui_ms | 0.07 | 3.6% |
| cpu_encode_ms | 0.38 | 3.8% |
| cpu_game_ms | 0.92 | 9.9% |
| cpu_present_ms | 8.41 | 1.2% |
| cpu_submit_ms | 1.58 | 3.8% |
| frames | 4940.00 | 2.4% |
| frames_over_33ms | 1.00 | 300.0% |
| frames_over_50ms | 0.00 | 0.0% |
| gpu_bloom_ms | 18.53 | 3.0% |
| gpu_fxaa_ms | 4.37 | 1.6% |
| gpu_hand_ms | 3.80 | 1.7% |
| gpu_post_ms | 4.19 | 1.7% |
| gpu_shadow_ms | 0.31 | 1.8% |
| gpu_shafts_ms | 7.93 | 1.7% |
| gpu_total_ms | 7.56 | 1.3% |
| gpu_ui_ms | 4.37 | 1.6% |
| gpu_water_ms | 4.59 | 1.5% |
| gpu_world_ms | 3.61 | 0.6% |
| light_mesh_ms | 8.33 | 1.4% |
| memory_mib | 709.73 | 0.4% |
| missed_frame_pct | 0.16 | 14.7% |
| one_percent_low_fps | 46.83 | 2.3% |
| refresh_hz | 60.00 | 0.0% |
| worst_frame_ms | 37.77 | 29.2% |
