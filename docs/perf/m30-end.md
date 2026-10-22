# M30 performance

`cargo run --release -- --perf-test --end`: seed 20111118, a 60 s flight north at 10.9 blocks/s and feet height 90, render distance 9, 1920×1080, vsync off, after 5 s of warm-up.

Measured on Macmini9,1 (Apple M1); ended at [28, 92, -1259], 462 chunks loaded, 73 drawn.

| Measure | Value |
|---|---|
| Average FPS | 329 |
| 1% low FPS | 155 |
| Worst frame | 20.3 ms |
| Frames over 33 ms / 50 ms | 0 / 0 (of 19757) |
| Chunks generated or loaded | 1365 (avg 0.67 ms each, on worker threads) |
| Chunks lit and meshed | 1197 (avg 1.41 ms each, on worker threads) |
| Memory (resident) | 165 MiB |

Gate (average ≥ 60, 1% low ≥ 40): **pass**
