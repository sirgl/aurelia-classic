# Campaign status: Apple M5 Max (updated 2026-11-23 23:38 CET)

Machine: Apple M5 Max, 6P+12E cores, 128 GB RAM, on charger. Plain fp32 (`MLX_ENABLE_TF32=0`) for a like-for-like
comparison with the M4. Code of the game, trainer and net is unchanged.

State: running, 10 of 27 jobs done, now `train:d96-l3`, estimate about 3.1 h left (calibration says 3.4 h, +-30 %).
No failed jobs.

| Net | hold acc (unit+action) | train decisions/s | quick eval: families >= 90 % | ms per AI turn |
|---|---:|---:|---:|---:|
| d48-l3 | 0.680 | 5097 | 5 of 13 | 2.5 |
| d64-l3 | 0.686 | 4991 | 2 of 13 | 3.0 |

Speed against M4 (benchmark, TF32 default, Spotlight was indexing, so a lower bound): MB training 5744 decisions/s
(M4 2700-3000), trainer 133 plays/s on 1 thread (M4 about 95), net 2.55 ms per turn (M4 about 15).

Not committed: local `tools/m5/env.sh` (`MLX_ENABLE_TF32=0`) and the auto-tune result in `campaign.toml`.
The final report and `m5-results.tgz` come when the campaign finishes.
