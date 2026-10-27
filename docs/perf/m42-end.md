# M42 performance

`cargo run --release -- --perf-test --end`: seed 20111118, a 60 s flight north at 10.9 blocks/s and feet height 90, render distance 9, 1920×1080, vsync off, after 5 s of warm-up.

Measured on Macmini9,1 (Apple M1); ended at [28, 92, -1259], 441 chunks loaded, 74 drawn.

| Measure | Value |
|---|---|
| Average FPS | 319 |
| 1% low FPS | 146 |
| Worst frame | 20.5 ms |
| Frames over 33 ms / 50 ms | 0 / 0 (of 19114) |
| Chunks generated or loaded | 1379 (avg 0.70 ms each, on worker threads) |
| Chunks lit and meshed | 1197 (avg 1.55 ms each, on worker threads) |
| Memory (resident) | 208 MiB |

Gate (average ≥ 60, 1% low ≥ 40): **pass**
