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

## The stack that declares war stays a menace (lane `capitals`, 23 November)

The promotion's P2 failed by capitals lost at Standard speed (8 against 5, 1.6×). Six capitals lost by `standard` were
traced (`work/nets/capitals`, `NET2_TRACE` and a scratch trace of `defence::watch`, not kept). In each the
rival's army stood at our cities at peace, `menace` read it (R11), and on the turn the rival declared war R11 was set to
0: it was read at peace only. With it the military price's tilt (800 · max menace), the guard (1200 · the stack's
menace) and the defence posture's term (1500 · max menace) fell to 0, and war weariness pushed the tilt below zero.
Case: offset 3340, seed 3340001 (even), seat 4: r172 menace 930, tilt +744, guard 916; r173 (war) menace 0, tilt −94,
guard 0; the capital fell at r183. The fix reads R11 at war as well (`rival_inputs::read`); no weight changed. Guards:
`ai_net2_inputs::r11_a_stack_at_the_border` (the stack stays above 500 at war) and
`ai_net2_inputs::the_stack_that_declares_war_stays_a_menace` (read from the game, the menace, the max menace and the
defence term hold across the declaration; it fails with R11 read at peace only).

Whole games, `standard` against `pre-net2`, Standard speed (gates in `work/nets/capitals/GATES.md`, written before the
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
`work/nets/science-flow/SCIENCE-FLOW.md`.

## Candidate: early-growth (bench only, `experimental-ai`; not measured against its gates)

`early-growth` is `science-flow` with three more inputs of `net2` (`weights::g::OPENING`, `OPENING_GAP`, `LUX_OPEN`;
`net2::opening`, `opening_gap`, `lux_open`), one more term of `CITY_CAP` and two price tilts (`weights::EXPANSION_TILT`,
`WORKER_TILT`, outside `neurons()` like the science-flow neurons). Every other variant keeps the inputs at 0
(`TurnBrief::early_growth`), so the hashes of `standard`, `pre-net2` and `science-flow` do not move.

| piece | what | why (look, 20 self-play games, seeds 3610–3614, Quick) |
|---|---|---|
| `OPENING` → `CITY_CAP` | the opening's cities (below four, on free sites, each keeping excess happiness ≥ 0) beyond the size model's | the model keeps 6 points after a city (`tall_real::CONTENT_FLOOR`): 2 cities in 4909 of 6650 turns of r1–100 |
| `OPENING_GAP` → `EXPANSION_TILT` | the expansion price +1.5 per opening city | the head replaced the allocator's settler by a granary or a library; a city the cap allowed waited 32 rounds for its settler |
| `LUX_OPEN` → `WORKER_TILT` | the worker price +2 per luxury kind waiting beyond the workers | first worker r46 with 1.9 luxury kinds unconnected at r40 |

Measured on the tuning set only: the 3rd city r80 → r34, the 4th r108 → r93, cities at r80 2.57 → 3.31 (science
winners); science at r132 +13 %, technologies +1.8 %; 52.7 % against `science-flow` on offset 3650 (200 games, median
science-win round 314 against 309). The research did not move: each city adds 5 % to every technology's cost on the
pace maps, the new cities have no library (lag 34 rounds), and the first worker stays at r45. The look and the tables are
in `work/nets/early-growth`; the trace is `ai-science-pace --early FILE`.

Round 2 (`net2::growth`, `plan::Growth`; off for every other variant). Every number that decides is a weight with its
why in `weights.rs` (the data: `work/nets/early-growth/runs/derive-e1-sf.md`, science-flow's self-play 3700–3716,
`tools/derive.py`); the executors only read the plan.

| piece | what the executor reads | why |
|---|---|---|
| `OPENING_ROOM` (a `Join::Min`), `OPENING` | `CITY_CAP`'s `g::OPENING`, `EXPANSION_TILT`'s `g::OPENING_GAP`: the first of four cities, the free sites, the cities the happiness pays for | seats at 3–4 cities at r80 made 82–83 beakers at r132 against 72 at 2 cities |
| `GARRISON` (gate 500, per city; the capital keeps it) | `Growth::garrison_waits`: `planner_cities::garrison_order` orders no flat garrison there | young cities below the mean production had a military head in 49.6 % of their rounds, 70 % of it the flat garrison; capitals lost 4 against 3 in E3 with the wait on every city |
| `CHEST_ITEM`, `BUY_CHEST` | `Growth::chest_heads`, `Growth::chest`: in `allocator::spend_gold` only the listed heads spend the chest | a library's offer had the better ratio (1853 against 1195) but its price was above the spendable gold in 3706 of 4591 offers |
| `WORKER_SHORT`, `JOBS_CAP` | `Growth::jobs_ahead`: the allocator's worker counts at least the open tiles up to the citizens + 2 while a city lacks its worker | a city grows 2 citizens in 12 rounds, a first worker's second improvement takes 14 |
| site head S11·S12 (`site::SHARED` = 750) | three quarters of the land a site shares with an own city is subtracted | the share of citizens lost on the share of tiles shared: 0.76 (95 % CI 0.44–0.99) |

Found and not changed: the building price does not count the buildings a building opens (`building_here`'s depth is
the free buildings'); the worker's chop does not count the hammers the chop gives; a worker's job weight is linear in its
gain. A higher worker gain for a lacking luxury (tried) left the improvable lacking luxuries as open as before (1.4 kinds
at r50): their cause is not the job's value.

## The beliefs by their effects (part `beliefs`, 23 November)

Before, the religion's founder, follower and enhancer beliefs came from three fixed lists of content keys
(`ai::belief_lists`, now frozen for the seats without the part) and the reformation belief was the first free one of
the table. The lists' first follower beliefs only let the cities buy a building with faith, which no executor does (the
faith buys missionaries and inquisitors only): in 12 traced games every religion took one of them.

The belief head (`strategy_net::beliefs`) values every free belief by its declared effects at the strategy plan's
prices, as the policy head values a policy: the effects on following cities as the cell's run and the classifier's
value in our cities (our following cities are our cities, the holy city the capital, a gate on followers a gate on
citizens), times the share of our cities that follow; the founder's per following city and per follower by the
religion's reach (ours now, abroad as the rivals' religions reach, at least the priors); a number of the religion by
the foreign cities it adds at what one pays us; items for faith by what the executors buy. The weights and their
reasons are `strategy_net::weights::belief` (measured on 40 standard-suite games of the base: 83 % of a founder's own
cities follow, ~27 foreign followers in ~4.5 foreign cities at the end). `net2-no-beliefs` plays the lists.

Measured (`work/nets/beliefs/GATES.md`): 400 Quick pace games, offsets 3910-3911, `standard` against
`net2-no-beliefs` (hash = the base `standard`'s ded99cb94d6e55fc): victory-weighted share 48.8 % (52.0 / 45.7), real
victories 129 / 138; religions founded 61 % / 60 % of the seats, faith a turn at the end 18.3 / 16.1, followers of a
religion 147 / 160; AI ms p50 1.00×. Hashes then: `standard` d81486aa82df06e7, `pre-net2` 380c45a20e751d31.

A guard (`tests/ai_content_names.rs`) fails when a content key or a content's display name is written in the AI's
code outside the scripted probes and the frozen lists.

## The ideology by its tenets (part `ideology`, 23 November)

The policy head took the ideology the rules' roles name for the plan (`roles.ai_ideologies`: Order wide, Autocracy at
war, Freedom at peace) and valued the tenets only when that one was not offered. With the part `ideology` it takes the
ideology whose tenets are worth most at the plan's prices (`Culture::by_tenets`); `net2-no-ideology` plays the named
one. In 12 traced Quick pace games the two disagree in 48 of 72 choices (the roles' Freedom against the tenets' Order
25 times, Autocracy 18). 400 Quick pace games, offsets 3920-3921, `standard` against `net2-no-ideology` (= the
beliefs build's `standard`): victory-weighted share 54.7 % (55.3 / 54.1), real victories 154 / 123. Hash `standard`
cf535f70eb6b37bf; `net2-no-ideology` d81486aa82df06e7.
