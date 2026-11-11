# M74 performance notes (Mac mini M1)

M74 changes no rendering, world generation, lighting, redstone or fluids; the golden hashes are unchanged and `tests/vanilla_reference.rs` passes. The perf gate (`scripts/perf_gate.py docs/perf/m74.json`, which holds the last of three attempts) does **not** pass on the `cargo bench` rows today, and the average FPS row passes or fails with what else the mini is doing. The mini was shared with another session's builds and test runs (load average 10 to 70 at times), so this is a machine question for Kenzo, not something to fix in M74's code. Nothing was lowered or re-baselined.

## What the three attempts showed

First attempt (load average 27 to 38 while the benches ran): the three FPS rows passed with room (average 130.8 against 123.8, 1% low 78.4 against 61.8, chunk generation and light+mesh faster than the baseline), but the bench rows were up to +193% (a stray copy of the app from my own lite-bundle check was spinning one core at 99% for ten minutes; fixed in `check-bundle.sh --lite`, and the idle setup window no longer spins).

```
| Measure | Baseline | Now | Change | Limit | Result |
|---|---|---|---|---|---|
| average_fps | 123.80 | 130.83 | +5.7% | >= 79% | pass |
| one_percent_low_fps | 61.77 | 78.43 | +27.0% | >= 46% | pass |
| chunk_generation_ms | 8.07 | 5.67 | -29.8% | <= 105% | pass |
| light_mesh_ms | 9.08 | 6.33 | -30.2% | <= 105% | pass |
| memory_mib | 678.34 | 714.38 | +5.3% | <= 110% | pass |
| bench: 100 ticks of redstone lines | 2.80 | 3.00 | +7.3% | <= 105% | FAIL |
| bench: 100 ticks of spreading water | 3.18 | 3.72 | +16.9% | <= 105% | FAIL |
| bench: generate a chunk | 2.73 | 7.99 | +193.1% | <= 105% | FAIL |
| bench: generate a nether chunk | 1.39 | 4.04 | +190.3% | <= 105% | FAIL |
| bench: generate an end chunk | 0.04 | 0.10 | +195.4% | <= 105% | FAIL |
| bench: generate an end city chunk | 0.72 | 1.39 | +92.6% | <= 105% | FAIL |
| bench: light a chunk | 1.17 | 1.25 | +7.2% | <= 105% | FAIL |
| bench: mesh a chunk | 2.14 | 2.22 | +3.4% | <= 105% | pass |
| frames_over_50ms | 0 | 0.00 |  | <= 0 | pass |
| budget: one_percent_low_fps |  | 78.43 |  | min 40 | pass |
```

Last attempt (quiet for the start, the other session's tests running during the second flight; this is `docs/perf/m74.md`):

```
| Measure | Baseline | Now | Change | Limit | Result |
|---|---|---|---|---|---|
| average_fps | 123.80 | 96.62 | -22.0% | >= 79% | FAIL |
| one_percent_low_fps | 61.77 | 47.69 | -22.8% | >= 46% | pass |
| chunk_generation_ms | 8.07 | 6.93 | -14.2% | <= 105% | pass |
| light_mesh_ms | 9.08 | 7.27 | -19.9% | <= 105% | pass |
| memory_mib | 678.34 | 404.70 | -40.3% | <= 110% | pass |
| bench: 100 ticks of redstone lines | 2.80 | 2.96 | +5.8% | <= 105% | FAIL |
| bench: 100 ticks of spreading water | 3.18 | 3.51 | +10.1% | <= 105% | FAIL |
| bench: generate a chunk | 2.73 | 3.00 | +10.0% | <= 105% | FAIL |
| bench: generate a nether chunk | 1.39 | 1.45 | +4.5% | <= 105% | pass |
| bench: generate an end chunk | 0.04 | 0.04 | +6.2% | <= 105% | FAIL |
| bench: generate an end city chunk | 0.72 | 0.73 | +1.3% | <= 105% | pass |
| bench: light a chunk | 1.17 | 1.31 | +12.0% | <= 105% | FAIL |
| bench: mesh a chunk | 2.14 | 2.21 | +3.2% | <= 105% | pass |
| frames_over_50ms | 0 | 0.00 |  | <= 0 | pass |
| budget: one_percent_low_fps |  | 47.69 |  | min 40 | pass |
```

## Same machine, same hour: main against this branch

Because the bench rows moved on code M74 doesn't touch, `cargo bench` was run on `origin/main` (v1.5.0) and on this branch in the order mine, main, main, mine, mine, main (the criterion median of each run, milliseconds; each side's slowest runs are the ones that overlapped the other session's load):

| bench | main (median of 3) | this branch (median of 3) | ratio |
|---|---|---|---|
| 100 ticks of redstone lines | 3.248 | 3.439 | 1.059 |
| 100 ticks of spreading water | 3.608 | 3.606 | 0.999 |
| generate a chunk | 3.018 | 3.014 | 0.998 |
| generate an end chunk | 0.036 | 0.036 | 1.010 |
| light a chunk | 1.350 | 1.238 | 0.917 |
| mesh a chunk | 2.179 | 2.165 | 0.993 |

`main` itself measures about 10% over `docs/perf/baseline.json` today (generate a chunk 3.02 ms against 2.73, light a chunk 1.35 against 1.17), so the bench rows of the gate would fail on v1.5.0 as well. Decision for Kenzo: re-run the gate on a quiet mini, or accept a new baseline (`--update-baseline --accept`) if the mini simply got slower.
