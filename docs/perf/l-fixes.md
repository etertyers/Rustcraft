# l-fixes performance, every look

## l-fixes performance

3 flights of 60 s (seed 20111118, render distance 9), median and spread (max − min as % of the median). Measured on Macmini9,1 (Apple M1), commit 67a2ec5e51e0bf968721c74ea480172e0a62ecfa, row `vanilla`.

| Measure | Median | Spread |
|---|---|---|
| average_fps | 122.65 | 1.6% |
| chunk_generation_ms | 7.72 | 6.0% |
| chunks_generated | 1365.00 | 0.0% |
| chunks_meshed | 1645.00 | 4.2% |
| frames | 7360.00 | 1.6% |
| frames_over_33ms | 0.00 | 0.0% |
| frames_over_50ms | 0.00 | 0.0% |
| gpu_hand_ms | 1.41 | 1.5% |
| gpu_total_ms | 3.94 | 1.0% |
| gpu_ui_ms | 1.35 | 1.5% |
| gpu_world_ms | 3.87 | 1.0% |
| light_mesh_ms | 8.57 | 4.6% |
| memory_mib | 680.61 | 9.7% |
| one_percent_low_fps | 57.61 | 1.6% |
| worst_frame_ms | 27.24 | 40.9% |

## l-fixes-barebones performance

3 flights of 60 s (seed 20111118, render distance 9), median and spread (max − min as % of the median). Measured on Macmini9,1 (Apple M1), commit 67a2ec5e51e0bf968721c74ea480172e0a62ecfa, row `barebones`.

| Measure | Median | Spread |
|---|---|---|
| average_fps | 121.82 | 0.7% |
| chunk_generation_ms | 7.97 | 5.8% |
| chunks_generated | 1365.00 | 0.0% |
| chunks_meshed | 1689.00 | 11.2% |
| frames | 7310.00 | 0.7% |
| frames_over_33ms | 0.00 | 0.0% |
| frames_over_50ms | 0.00 | 0.0% |
| gpu_extra_ms | 0.00 | 0.0% |
| gpu_hand_ms | 1.42 | 0.8% |
| gpu_total_ms | 3.93 | 0.5% |
| gpu_ui_ms | 1.37 | 0.8% |
| gpu_world_extra_ms | 0.00 | 0.0% |
| gpu_world_ms | 3.87 | 0.5% |
| light_mesh_ms | 8.90 | 7.5% |
| memory_mib | 713.17 | 15.6% |
| one_percent_low_fps | 62.67 | 1.9% |
| worst_frame_ms | 27.39 | 6.7% |

## l-fixes-realistic performance

3 flights of 60 s (seed 20111118, render distance 9), median and spread (max − min as % of the median). Measured on Macmini9,1 (Apple M1), commit 67a2ec5e51e0bf968721c74ea480172e0a62ecfa, row `realistic`.

| Measure | Median | Spread |
|---|---|---|
| average_fps | 122.59 | 1.2% |
| chunk_generation_ms | 8.00 | 2.6% |
| chunks_generated | 1365.00 | 0.0% |
| chunks_meshed | 1732.00 | 10.4% |
| frames | 7357.00 | 1.2% |
| frames_over_33ms | 0.00 | 0.0% |
| frames_over_50ms | 0.00 | 0.0% |
| gpu_extra_ms | 0.01 | 0.0% |
| gpu_hand_ms | 1.43 | 1.6% |
| gpu_total_ms | 3.95 | 0.8% |
| gpu_ui_ms | 1.37 | 1.7% |
| gpu_world_extra_ms | 0.01 | 0.0% |
| gpu_world_ms | 3.88 | 0.6% |
| light_mesh_ms | 9.06 | 8.2% |
| memory_mib | 613.20 | 17.9% |
| one_percent_low_fps | 58.22 | 2.0% |
| worst_frame_ms | 30.32 | 17.4% |

## l-fixes-barebones-low performance

3 flights of 60 s (seed 20111118, render distance 9), median and spread (max − min as % of the median). Measured on Macmini9,1 (Apple M1), commit 67a2ec5e51e0bf968721c74ea480172e0a62ecfa, row `barebones-low`.

| Measure | Median | Spread |
|---|---|---|
| average_fps | 91.04 | 0.8% |
| chunk_generation_ms | 7.89 | 0.8% |
| chunks_generated | 1365.00 | 0.0% |
| chunks_meshed | 1659.00 | 8.4% |
| frames | 5463.00 | 0.8% |
| frames_over_33ms | 0.00 | 0.0% |
| frames_over_50ms | 0.00 | 0.0% |
| gpu_bloom_ms | 11.13 | 1.7% |
| gpu_extra_ms | 2.27 | 0.0% |
| gpu_hand_ms | 2.84 | 1.1% |
| gpu_post_ms | 3.00 | 1.0% |
| gpu_shadow_ms | 0.12 | 3.0% |
| gpu_total_ms | 6.21 | 0.6% |
| gpu_ui_ms | 2.99 | 1.0% |
| gpu_world_extra_ms | 1.89 | 0.0% |
| gpu_world_ms | 5.76 | 0.8% |
| light_mesh_ms | 8.53 | 1.8% |
| memory_mib | 724.31 | 13.9% |
| one_percent_low_fps | 53.70 | 0.4% |
| worst_frame_ms | 30.32 | 44.7% |

## l-fixes-barebones-medium performance

3 flights of 60 s (seed 20111118, render distance 9), median and spread (max − min as % of the median). Measured on Macmini9,1 (Apple M1), commit 67a2ec5e51e0bf968721c74ea480172e0a62ecfa, row `barebones-medium`.

| Measure | Median | Spread |
|---|---|---|
| average_fps | 76.64 | 2.5% |
| chunk_generation_ms | 7.87 | 6.6% |
| chunks_generated | 1365.00 | 0.6% |
| chunks_meshed | 1621.00 | 20.0% |
| frames | 4599.00 | 2.5% |
| frames_over_33ms | 1.00 | 2000.0% |
| frames_over_50ms | 0.00 | 0.0% |
| gpu_bloom_ms | 22.09 | 2.2% |
| gpu_extra_ms | 4.25 | 0.0% |
| gpu_fxaa_ms | 5.10 | 1.6% |
| gpu_hand_ms | 4.60 | 1.5% |
| gpu_post_ms | 4.93 | 1.5% |
| gpu_shadow_ms | 0.31 | 0.7% |
| gpu_shafts_ms | 9.47 | 1.5% |
| gpu_total_ms | 8.19 | 1.7% |
| gpu_ui_ms | 5.09 | 1.5% |
| gpu_water_ms | 5.34 | 1.4% |
| gpu_world_extra_ms | 0.00 | 0.0% |
| gpu_world_ms | 3.39 | 1.0% |
| light_mesh_ms | 8.03 | 5.7% |
| memory_mib | 731.56 | 0.4% |
| one_percent_low_fps | 44.64 | 14.4% |
| worst_frame_ms | 36.23 | 50.2% |

## l-fixes-barebones-high performance

3 flights of 60 s (seed 20111118, render distance 9), median and spread (max − min as % of the median). Measured on Macmini9,1 (Apple M1), commit 67a2ec5e51e0bf968721c74ea480172e0a62ecfa, row `barebones-high`.

| Measure | Median | Spread |
|---|---|---|
| average_fps | 59.10 | 0.3% |
| chunk_generation_ms | 7.96 | 1.4% |
| chunks_generated | 1357.00 | 0.1% |
| chunks_meshed | 1648.00 | 7.6% |
| frames | 3547.00 | 0.3% |
| frames_over_33ms | 2.00 | 0.0% |
| frames_over_50ms | 0.00 | 0.0% |
| gpu_bloom_ms | 33.68 | 0.0% |
| gpu_extra_ms | 7.14 | 0.0% |
| gpu_fxaa_ms | 7.84 | 0.0% |
| gpu_hand_ms | 6.70 | 0.1% |
| gpu_post_ms | 7.69 | 0.0% |
| gpu_shadow_ms | 0.72 | 0.8% |
| gpu_shafts_ms | 15.03 | 0.1% |
| gpu_total_ms | 11.07 | 0.3% |
| gpu_ui_ms | 7.83 | 0.0% |
| gpu_water_ms | 7.51 | 0.0% |
| gpu_world_extra_ms | 0.50 | 0.0% |
| gpu_world_ms | 4.37 | 0.0% |
| light_mesh_ms | 8.14 | 4.7% |
| memory_mib | 729.19 | 13.9% |
| one_percent_low_fps | 39.33 | 0.4% |
| worst_frame_ms | 34.15 | 3.4% |

## l-fixes-vanilla-medium performance

3 flights of 60 s (seed 20111118, render distance 9), median and spread (max − min as % of the median). Measured on Macmini9,1 (Apple M1), commit 67a2ec5e51e0bf968721c74ea480172e0a62ecfa, row `vanilla-medium`.

| Measure | Median | Spread |
|---|---|---|
| average_fps | 77.91 | 0.9% |
| chunk_generation_ms | 8.08 | 3.2% |
| chunks_generated | 1365.00 | 0.0% |
| chunks_meshed | 1625.00 | 7.8% |
| frames | 4676.00 | 0.9% |
| frames_over_33ms | 1.00 | 0.0% |
| frames_over_50ms | 0.00 | 0.0% |
| gpu_bloom_ms | 21.98 | 0.6% |
| gpu_extra_ms | 4.14 | 0.0% |
| gpu_fxaa_ms | 5.04 | 0.1% |
| gpu_hand_ms | 4.53 | 0.2% |
| gpu_post_ms | 4.87 | 0.2% |
| gpu_shadow_ms | 0.30 | 1.8% |
| gpu_shafts_ms | 9.34 | 0.2% |
| gpu_total_ms | 8.07 | 0.3% |
| gpu_ui_ms | 5.03 | 0.1% |
| gpu_water_ms | 5.27 | 0.1% |
| gpu_world_extra_ms | 0.00 | 0.0% |
| gpu_world_ms | 3.36 | 0.1% |
| light_mesh_ms | 8.04 | 6.1% |
| memory_mib | 695.08 | 0.4% |
| one_percent_low_fps | 45.09 | 3.0% |
| worst_frame_ms | 37.75 | 33.8% |

## l-fixes-realistic-medium performance

3 flights of 60 s (seed 20111118, render distance 9), median and spread (max − min as % of the median). Measured on Macmini9,1 (Apple M1), commit 67a2ec5e51e0bf968721c74ea480172e0a62ecfa, row `realistic-medium`.

| Measure | Median | Spread |
|---|---|---|
| average_fps | 77.10 | 0.6% |
| chunk_generation_ms | 7.94 | 1.8% |
| chunks_generated | 1365.00 | 0.0% |
| chunks_meshed | 1706.00 | 5.4% |
| frames | 4627.00 | 0.6% |
| frames_over_33ms | 1.00 | 0.0% |
| frames_over_50ms | 0.00 | 0.0% |
| gpu_bloom_ms | 22.20 | 1.2% |
| gpu_extra_ms | 4.22 | 0.0% |
| gpu_fxaa_ms | 5.10 | 0.3% |
| gpu_hand_ms | 4.59 | 0.4% |
| gpu_post_ms | 4.93 | 0.4% |
| gpu_shadow_ms | 0.31 | 1.1% |
| gpu_shafts_ms | 9.47 | 0.4% |
| gpu_total_ms | 8.15 | 0.4% |
| gpu_ui_ms | 5.09 | 0.3% |
| gpu_water_ms | 5.33 | 0.4% |
| gpu_world_extra_ms | 0.00 | 0.0% |
| gpu_world_ms | 3.38 | 0.5% |
| light_mesh_ms | 8.29 | 3.7% |
| memory_mib | 729.64 | 0.9% |
| one_percent_low_fps | 46.12 | 2.8% |
| worst_frame_ms | 36.96 | 37.3% |

