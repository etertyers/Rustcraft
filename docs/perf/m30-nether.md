# M30 performance

`cargo run --release -- --perf-test --nether`: seed 20111118, a 60 s flight north at 10.9 blocks/s and feet height 72, render distance 9, 1920×1080, vsync off, after 5 s of warm-up.

Measured on Macmini9,1 (Apple M1); ended at [0, 74, -708], 462 chunks loaded, 190 drawn.

| Measure | Value |
|---|---|
| Average FPS | 179 |
| 1% low FPS | 87 |
| Worst frame | 17.4 ms |
| Frames over 33 ms / 50 ms | 0 / 0 (of 10736) |
| Chunks generated or loaded | 1365 (avg 2.24 ms each, on worker threads) |
| Chunks lit and meshed | 31387 (avg 6.65 ms each, on worker threads) |
| Memory (resident) | 258 MiB |

Gate (average ≥ 60, 1% low ≥ 40): **pass**
