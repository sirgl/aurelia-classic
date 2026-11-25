# net2: one net for the strategist's turn (the sixteenth promotion)

`net2` (`ai::net2`, design `work/new-net-design-2026-11-22.md`) is one integer net that reads the seat and every rival
and takes the strategist's turn: the style, the aim and the posture (`strategy_net::intent_step` with `net2`'s tilt),
the war orders and their aims, the deals, the peace answer, the city-state share, the congress ballot, the guard
level of every city, the fleet, the city cap (`CITY_CAP`), the copies of a strategic resource held for the spaceship
(`PART_HOLD`), the research on the spaceship's way (`SPACE_RESEARCH`) and the capital kept free for the spaceship's
parts (`CAPITAL_GATE`: no own civilian parks in it or at its gate while a part is outside). Its site head values the
settlers' sites.
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

Gates of round 5 that failed as written (the owner decides on accepting them): the exam lost A5 mild, A6 medium, D3
strong and HMO5 mild; A5 `nobody_attacked` 0.863 against a gate of 0.861 (a diagnostic, not a goal); HD8
`script_white_peace` 0.70 against a gate of 0.5.

### The promotion's gates (written 23 November before any run of the promoted build; not changed after)

Old build: the merge of `ai/net2` into main (`ai/promote-net2` bfe8966) before the promotion commit. New build: the
promotion commit. Whole games: `ai-tournament --candidate standard --baseline pre-net2 --suite pace` on the new build,
fresh offsets (not in the STATE lane's notes.md` nor in any gates file); shares pooled with the tools lane's notes.
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
| P7 | the bench quick: `bench.py run --clone <clone> --candidate standard --baseline pre-net2 --level quick --parallel 2` | reported |
| P8 | a save made by today's main build loads into the promoted build and plays on | the game goes on, no panic |

The results are in the promote-net2 lane's notes.

## The stack that declares war stays a menace (lane `capitals`, 23 November)

The promotion's P2 failed by capitals lost at Standard speed (8 against 5, 1.6×). Six capitals lost by `standard` were
traced (the capitals lane's notes, `NET2_TRACE` and a scratch trace of `defence::watch`, not kept). In each the
rival's army stood at our cities at peace, `menace` read it (R11), and on the turn the rival declared war R11 was set to
0: it was read at peace only. With it the military price's tilt (800 · max menace), the guard (1200 · the stack's
menace) and the defence posture's term (1500 · max menace) fell to 0, and war weariness pushed the tilt below zero.
Case: offset 3340, seed 3340001 (even), seat 4: r172 menace 930, tilt +744, guard 916; r173 (war) menace 0, tilt −94,
guard 0; the capital fell at r183. The fix reads R11 at war as well (`rival_inputs::read`); no weight changed. Guards:
`ai_net2_inputs::r11_a_stack_at_the_border` (the stack stays above 500 at war) and
`ai_net2_inputs::the_stack_that_declares_war_stays_a_menace` (read from the game, the menace, the max menace and the
defence term hold across the declaration; it fails with R11 read at peace only).

Whole games, `standard` against `pre-net2`, Standard speed (gates in the capitals lane's notes, written before the
acceptance run):

| run | games | capitals lost | victory-weighted share |
|---|---:|---|---:|
| main, tuning 3340–3345 | 1200 | 22 / 26 | 68.7 % |
| fix, tuning 3340–3345 | 1200 | 21 / 27 | 70.4 % |
| main, acceptance 3360–3361 (for information, after the gates) | 400 | 3 / 7 | 70.0 % |
| fix, acceptance 3360–3361 | 400 | 7 / 9 | 72.7 % |

The gates pass (7 / 9 = 0.78×, 72.7 %), and they pass on main too: the fix moves no capital count beyond the noise
(1600 games: main 25 / 33, fix 28 / 36) and the share by +1.7 and +2.7 points (within the intervals). The 1.6× of P2
does not reproduce on 1200 fresh games of main (0.85×); over every Standard-speed run of main so far
(P2, 1852–1853, 3340–3345) it is 37 / 34 = 1.09×. In the traced case the fixed outputs did not change the play at the
capital: the defence did not buy or build more (gold 171–240 unspent, buildings in the queue under a threat 2–3 × the
guard); that is the shared defence executor's, not `net2`'s.

## Candidate: science-flow (bench only, `experimental-ai`)

`science-flow` is `standard` with two more neurons of `net2` (`net2/science.rs`, `weights::SCIENTISTS`, `AGREEMENT`;
`Net2Plan::science`) and one more rival input (`r::CHEST_WAIT`, kept in `plan::ScienceMemory`). Every other variant keeps
them off (`TurnBrief::science_flow`; the input is 0 and the memory writes no bytes), so the hashes of `standard` and
`pre-net2` do not move.

| neuron | output | executor that reads it |
|---|---|---|
| `SCIENTISTS` | the share of the science specialists' slots filled | `citizens::science_specialist_commands`: forced specialists, the city keeps `gate::SCIENTIST_FOOD` food a turn |
| `AGREEMENT` (per rival) | the ties and the cadence of each (`science::cadence`: every round at 1000, `gate::LADDER_ROUNDS` at the gate) | `Net2Plan::denies_item` lifts the runaway's denial for the friendship and the agreement; `diplomacy::ladder_offers` offers the ladder at the tie's cadence; the allocator keeps the agreements' gold (`Science::chest`, the rules' cost: `deals::agreement_cost_of`) |

The chest is released by the net: `r::CHEST_WAIT` (the rounds the chest held a partner's gold unsigned, over
`gate::CHEST_WAIT_ROUNDS`) lowers `AGREEMENT` until the rival is no tie. The look, the measurements and the gates are in
the science-flow lane's notes.
