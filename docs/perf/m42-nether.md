# M42 performance

`cargo run --release -- --perf-test --nether`: seed 20111118, a 60 s flight north at 10.9 blocks/s and feet height 72, render distance 9, 1920×1080, vsync off, after 5 s of warm-up.

Measured on Macmini9,1 (Apple M1); ended at [0, 74, -708], 441 chunks loaded, 218 drawn.

| Measure | Value |
|---|---|
| Average FPS | 126 |
| 1% low FPS | 52 |
| Worst frame | 28.7 ms |
| Frames over 33 ms / 50 ms | 0 / 0 (of 7584) |
| Chunks generated or loaded | 1365 (avg 2.78 ms each, on worker threads) |
| Chunks lit and meshed | 30234 (avg 9.18 ms each, on worker threads) |
| Memory (resident) | 377 MiB |

Gate (average ≥ 60, 1% low ≥ 40): **pass**
