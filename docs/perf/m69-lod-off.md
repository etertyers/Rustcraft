# M58 performance

`cargo run --release -- --perf-test`: seed 20111118, a 60 s flight north at 10.9 blocks/s and feet height 100, render distance 9, 1920×1080, vsync off, after 5 s of warm-up.

Measured on Macmini9,1 (Apple M1); ended at [64, 102, -628], 441 chunks loaded, 212 drawn.

| Measure | Value |
|---|---|
| Average FPS | 85 |
| 1% low FPS | 48 |
| Worst frame | 31.3 ms |
| Frames over 33 ms / 50 ms | 0 / 0 (of 5131) |
| Chunks generated or loaded | 1344 (avg 7.87 ms each, on worker threads) |
| Chunks lit and meshed | 1635 (avg 7.89 ms each, on worker threads) |
| Memory (resident) | 714 MiB |

Gate (average ≥ 60, 1% low ≥ 40): **pass**
