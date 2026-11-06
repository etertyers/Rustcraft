# m68-vanilla-medium performance

3 flights of 60 s (seed 20111118, render distance 9), median and spread (max − min as % of the median). Measured on Macmini9,1 (Apple M1), commit 3826713a3915b64a137cd1d20f67b57bb2e86759-dirty, row `vanilla-medium`.

| Measure | Median | Spread |
|---|---|---|
| average_fps | 82.03 | 0.7% |
| chunk_generation_ms | 8.03 | 4.2% |
| chunks_generated | 1344.00 | 0.0% |
| chunks_meshed | 1633.00 | 5.9% |
| cpu_acquire_ms | 0.01 | 10.7% |
| cpu_chunks_ms | 0.09 | 12.7% |
| cpu_draw_ms | 11.17 | 0.8% |
| cpu_egui_ms | 0.08 | 1.3% |
| cpu_encode_ms | 0.40 | 1.1% |
| cpu_game_ms | 0.91 | 2.8% |
| cpu_present_ms | 8.48 | 0.2% |
| cpu_submit_ms | 1.65 | 1.4% |
| frames | 4923.00 | 0.7% |
| frames_over_33ms | 0.00 | 0.0% |
| frames_over_50ms | 0.00 | 0.0% |
| gpu_bloom_ms | 18.67 | 1.0% |
| gpu_fxaa_ms | 4.41 | 0.5% |
| gpu_hand_ms | 3.84 | 0.6% |
| gpu_post_ms | 4.23 | 0.5% |
| gpu_shadow_ms | 0.32 | 1.1% |
| gpu_shafts_ms | 8.01 | 0.6% |
| gpu_total_ms | 7.63 | 0.5% |
| gpu_ui_ms | 4.41 | 0.5% |
| gpu_water_ms | 4.63 | 0.5% |
| gpu_world_ms | 3.63 | 0.4% |
| light_mesh_ms | 8.45 | 1.6% |
| memory_mib | 704.00 | 1.1% |
| missed_frame_pct | 0.12 | 32.9% |
| one_percent_low_fps | 48.50 | 2.5% |
| refresh_hz | 60.00 | 0.0% |
| worst_frame_ms | 31.70 | 7.1% |
