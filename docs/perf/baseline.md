# Performance baseline (end of M14)

Later milestones compare against these (CLAUDE.md §7.2): average FPS must
stay at or above 60, the 1% low at or above 40, and no benchmark may get
more than 15% slower than the previous milestone. Measured on the Mac mini
(Macmini9,1, Apple M1) with `cargo run --release -- --perf-test` and
`cargo bench`; the full write-up is [m14.md](m14.md).

| Measure | M14 |
|---|---|
| Average FPS (render distance 9, 1920×1080) | 178 |
| 1% low FPS | 79 |
| Worst frame | 44.7 ms |
| Chunk generation (workers, average) | 4.04 ms |
| Chunk light + mesh (workers, average) | 5.33 ms |
| Memory (resident) | 168 MiB |
| bench: generate a chunk | 1.61 ms |
| bench: light a chunk | 0.686 ms |
| bench: mesh a chunk | 0.747 ms |
| bench: 100 ticks of spreading water | 3.26 ms |
