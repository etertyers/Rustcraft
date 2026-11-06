# m68-realistic-medium performance

3 flights of 60 s (seed 20111118, render distance 9), median and spread (max − min as % of the median). Measured on Macmini9,1 (Apple M1), commit 3826713a3915b64a137cd1d20f67b57bb2e86759-dirty, row `realistic-medium`.

| Measure | Median | Spread |
|---|---|---|
| average_fps | 81.87 | 1.7% |
| chunk_generation_ms | 8.07 | 1.0% |
| chunks_generated | 1344.00 | 0.0% |
| chunks_meshed | 1695.00 | 5.4% |
| cpu_acquire_ms | 0.01 | 13.6% |
| cpu_chunks_ms | 0.10 | 7.4% |
| cpu_draw_ms | 11.21 | 1.6% |
| cpu_egui_ms | 0.07 | 0.6% |
| cpu_encode_ms | 0.40 | 1.3% |
| cpu_game_ms | 0.87 | 3.5% |
| cpu_present_ms | 8.46 | 1.0% |
| cpu_submit_ms | 1.63 | 0.4% |
| frames | 4913.00 | 1.7% |
| frames_over_33ms | 0.00 | 0.0% |
| frames_over_50ms | 0.00 | 0.0% |
| gpu_bloom_ms | 18.58 | 1.8% |
| gpu_fxaa_ms | 4.42 | 1.3% |
| gpu_hand_ms | 3.85 | 1.2% |
| gpu_post_ms | 4.23 | 1.3% |
| gpu_shadow_ms | 0.32 | 2.6% |
| gpu_shafts_ms | 8.02 | 1.3% |
| gpu_total_ms | 7.63 | 0.9% |
| gpu_ui_ms | 4.41 | 1.3% |
| gpu_water_ms | 4.63 | 1.1% |
| gpu_world_ms | 3.63 | 0.9% |
| light_mesh_ms | 8.53 | 3.4% |
| memory_mib | 744.33 | 0.7% |
| missed_frame_pct | 0.17 | 61.2% |
| one_percent_low_fps | 47.94 | 1.3% |
| refresh_hz | 60.00 | 0.0% |
| worst_frame_ms | 29.26 | 6.0% |
