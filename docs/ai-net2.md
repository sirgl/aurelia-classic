# net2: one net for the strategist's turn (the sixteenth promotion)

`net2` (`ai::net2`, design `work/new-net-design-2026-11-22.md`) is one integer net that reads the seat and every rival
and takes the strategist's turn: the style, the aim and the posture (`strategy_net::intent_step` with `net2`'s tilt),
the war orders and their aims, the deals, the peace answer, the city-state share, the congress ballot, the guard
level of every city, the fleet, the city cap (`CITY_CAP`), the copies of a strategic resource held for the spaceship
(`PART_HOLD`) and the research on the spaceship's way (`SPACE_RESEARCH`). Its site head values the settlers' sites.
Every neuron is a hand-written weight with a one-line reason (`net2/weights.rs`). Where `net2` has no say, the seat
plays the mind of `nets-x-site` (the standard of the fifteenth promotion).

## Promoted: net2 (the sixteenth promotion, 23 November)

`standard` plays as the bench's `net2`. The promotion is a choice of the mind (`ClassicMind::new`): `Variant::Standard`
builds the mind of `nets-x-site` with `net2` on (`Variant::net2_parts`, `net2_offence` and `values_war_at_peace` answer
for `Standard` as for `Net2`), under the name `standard`. `ai::net2` is compiled in every build, so the game built
without `experimental-ai` plays it (`ai_experimental_off`).

`pre-net2` is the standard before it: the mind of `nets-x-site`, bench only (`experimental-ai`, last in `Variant::ALL`).
Every other variant plays as it was measured: `nets-x-site` (now the play of `pre-net2`), `net2-defence`, `net2-tac`,
the ablation's `net2-no-<part>` and `net2-off`, `tac2`, `peace-fix`, and the older lines; `net2` stays as an alias that
plays as `standard`.

No save section is added: `net2`'s memory is a section of the strategist's (`strategy_net::Memory::net2`); a save of the
old `standard` has no such section and loads as `net2` with an empty memory.

### Measured before the promotion (branch `ai/net2`, against `nets-x-site`, the standard then)

| run | games | victory-weighted share | real victories | capitals lost |
|---|---:|---:|---|---|
| round 5 acceptance, Quick, offsets 1844–1847 | 800 | 67.9 % | 339 / 133 | — |
| replication, Quick, offsets 1848–1851 | 800 | 66.2 % | 332 / 147 | — |
| Standard speed, offsets 1852–1853 | 400 | 68.6 % | 202 / 82 | 8 / 3 (offset 1853: 6 / 0) |
| duel | — | 48.8 % (parity) | — | — |

Gates of round 5 that failed as written (Roman decides on accepting them): the exam lost A5 mild, A6 medium, D3
strong and HMO5 mild; A5 `nobody_attacked` 0.863 against a gate of 0.861 (a diagnostic, not a goal); HD8
`script_white_peace` 0.70 against a gate of 0.5.

### The promotion's gates (written 23 November before any run of the promoted build; not changed after)

Old build: the merge of `ai/net2` into main (`ai/promote-net2` bfe8966) before the promotion commit. New build: the
promotion commit. Whole games: `ai-tournament --candidate standard --baseline pre-net2 --suite pace` on the new build,
fresh offsets (not in `work/nets/STATE.md` nor in any gates file); shares pooled with `work/nets/tools/victory-share.py`.
Every run goes through `work/res/res`.

| gate | what | pass |
|---|---|---|
| P0 | hashes, `--solo <v> --suite standard --speed quick`: new `standard` = old `net2`; new `pre-net2` = old `standard` = main's `380c45a20e751d31`; new `pre-nets-x` = `3743c16568ce4435`; `nets-x-site`, `net2-defence`, `net2-off`, `tac2`, `peace-fix`, `next-x` equal before and after | every hash as stated |
| P1 | 800 games, Quick, offsets 2200–2203 | victory-weighted share ≥ 55 % pooled; real victories ≥ 1.5 × `pre-net2`'s |
| P2 | 400 games, Standard speed, offsets 2210–2211 | victory-weighted share ≥ 55 % pooled; capitals lost ≤ 1.5 × `pre-net2`'s |
| P3 | the exam (`ai-stress`, every cell of the round-5 suite) and the human tricks (h*, o*), `standard` and `pre-net2` on the same positions and instances | reported cell by cell against `pre-net2`; no pass rule |
| P4 | AI ms p50 a turn, from P1's games | ≤ 1.15 × `pre-net2`'s; p95 reported |
| P5 | `cargo nextest run --profile full` with `experimental-ai` and without it, the doc tests, platform client + kernel + render; the fast tier's budget (`tools/test-budget.py`) | all green; every re-pin named with its reason |
| P6 | the game built without `experimental-ai` plays `net2`: `ai_experimental_off` pinned on the feature-on game of `net2` | green |
| P7 | the bench quick: `bench.py run --clone work/nets/promote-net2 --candidate standard --baseline pre-net2 --level quick --parallel 2` | reported |
| P8 | a save made by today's main build loads into the promoted build and plays on | the game goes on, no panic |

The results are in `work/nets/promote-net2/PROMOTE.md`.
