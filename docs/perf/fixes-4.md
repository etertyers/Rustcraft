# Pre-release fixes 4/4 performance

`cargo run --release -- --perf-test`: seed 20111118, a 60 s flight north at 10.9 blocks/s and feet height 100, render distance 9, 1920×1080, vsync off, after 5 s of warm-up.

Measured on Macmini9,1 (Apple M1); ended at [40, 102, -636], 462 chunks loaded, 194 drawn.

| Measure | Value |
|---|---|
| Average FPS | 148 |
| 1% low FPS | 83 |
| Worst frame | 23.9 ms |
| Frames over 33 ms / 50 ms | 0 / 0 (of 8851) |
| Chunks generated or loaded | 1344 (avg 3.54 ms each, on worker threads) |
| Chunks lit and meshed | 1449 (avg 4.49 ms each, on worker threads) |
| Memory (resident) | 215 MiB |

Gate (average ≥ 60, 1% low ≥ 40): **pass**

The flight has no mob crowd; the 50-mob AI cost is covered by `entity::ai::tests::fifty_pathing_mobs_cost_under_2_ms_a_tick` (about 0.2 ms a tick of thinking in a debug build).
