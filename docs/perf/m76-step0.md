# m76-step0 performance

3 flights of 60 s (seed 20111118, render distance 9) in Chrome 157.0.8103.79 on the Mac mini/MacBook the sessions run on (web (Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/157.0.0.0 Safari/537.); median and spread (max − min as % of the median). Row `web-vanilla`, style `vanilla`, shaders `off`, extra `&eager`.

| Measure | Median | Spread |
|---|---|---|
| average_fps | 59.45 | 0.1% |
| chunk_generation_ms | 0.00 | 0.0% |
| chunks_generated | 0.00 | 0.0% |
| chunks_meshed | 1511.00 | 7.5% |
| cpu_acquire_ms | 0.02 | 5.8% |
| cpu_chunks_ms | 2.15 | 5.9% |
| cpu_draw_ms | 0.84 | 3.6% |
| cpu_egui_ms | 0.10 | 6.2% |
| cpu_encode_ms | 0.38 | 3.5% |
| cpu_game_ms | 1.99 | 2.4% |
| cpu_present_ms | 0.02 | 13.1% |
| cpu_submit_ms | 0.04 | 1.9% |
| frames | 3568.00 | 0.1% |
| frames_over_33ms | 2.00 | 0.0% |
| frames_over_50ms | 2.00 | 0.0% |
| gpu_hand_ms | 1.71 | 1.5% |
| gpu_total_ms | 4.19 | 0.4% |
| gpu_ui_ms | 1.68 | 1.5% |
| gpu_world_ms | 4.10 | 0.4% |
| light_mesh_ms | 5.71 | 4.4% |
| memory_mib | 1107.31 | 1.0% |
| missed_frame_pct | 0.22 | 37.6% |
| one_percent_low_fps | 52.91 | 2.6% |
| refresh_hz | 60.00 | 0.0% |
| worst_frame_ms | 530.70 | 18.7% |
| worst frame over all runs (ms) | 537.80 | |
