# Speed benchmark: Apple M5 Max vs M4 Max (2026-11-23)

Machine: Apple M5 Max, 6 performance + 12 efficiency cores, 128 GB RAM, on charger. Run with `tools/m5/bench.sh`
through the broker, on the `exp/m5-kit` branches (`aurelia-classic` 9de99a9).

| Measure | M5 Max | M4 Max (reference) |
|---|---:|---:|
| MB training (d 64, 3 layers, plays 128), decisions/s | 5744 | 2700-3000 |
| Rust trainer plays/s, 1 thread (12 quick families, solvers) | 133 | about 95 |
| Rust trainer plays/s, 6 threads | 732 | n/a |
| Net ms per AI turn, 1 thread (MB) | 2.55 | about 15 |

Caveats: the machine was still indexing by Spotlight and had a load average near 20 during the run, so the numbers
are likely a lower bound. The M4 trainer figure was measured under load 15.

Raw output: `lab/runs/bench/results.json` (git-ignored), copied below.

```json
{
 "chip": "Apple M5 Max",
 "cores": "18",
 "ram_gb": 128,
 "train_decisions_per_s": 5744,
 "trainer_plays_per_s_1_threads": 133.09,
 "trainer_plays_per_s_6_threads": 732.0,
 "tournament_smoke_s": 2,
 "net_ms_per_turn_1_thread": 2.55
}```
