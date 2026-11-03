# M51 performance

`cargo run --release -- --perf-test --underwater`: seed 20111118, a 60 s flight north at 10.9 blocks/s and feet height 56.4, render distance 9, 1920×1080, vsync off, after 5 s of warm-up.

Measured on Macmini9,1 (Apple M1); ended at [1248, 58, 540], 441 chunks loaded, 212 drawn.

| Measure | Value |
|---|---|
| Average FPS | 122 |
| 1% low FPS | 43 |
| Worst frame | 64.9 ms |
| Frames over 33 ms / 50 ms | 28 / 3 (of 7314) |
| Chunks generated or loaded | 1379 (avg 9.88 ms each, on worker threads) |
| Chunks lit and meshed | 1986 (avg 9.13 ms each, on worker threads) |
| Memory (resident) | 397 MiB |

Gate (average ≥ 60, 1% low ≥ 40): **pass**
