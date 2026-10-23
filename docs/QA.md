# QA: a full AI-versus-AI match

**A full BNW match from world creation to victory passes on five map scripts with invariant checks every round; the open defects are at the end, some of them are out of date.** State as of 19 October 2026 (the sections "Results" and "Fixed defects" are as of 17-18 October).

The goal is to prove that the game can be played through from world creation to victory, and to find the defects that prevent it. The tool is the crate `integration/crates/aurelia-qa`: a BNW match where all players are AI, configured like the "New game" screen and played to victory or to the last round, with checks every round.

## How to run

All commands are from the repository root, `CARGO_TARGET_DIR` anything, `--offline`.

| What | Command |
|---|---|
| CI tests (a duel on all 5 map scripts with checks every round, a Tiny replay of 150 rounds, saving and continuing Tiny) | `cargo test --release --offline -p aurelia-qa` (≈ 15 s) |
| Long tests (all sizes Tiny…Huge × all scripts; Standard: a replay of 300 rounds, a save at round 250 and 50 rounds of continuation) | `cargo test --release --offline -p aurelia-qa -- --ignored` |
| Match matrix | `cargo run --release --offline -p aurelia-qa --example full_game -- --size duel,tiny,small --script all --difficulty settler,prince,deity --civs --stalls` |
| One victory only (the others are off, time remains) | `… full_game -- --only science --rounds 1000` (`science`, `diplomatic`, `culture`, `domination`, `time`) |
| Checks | `--checks full` (every round, twins every 25), `light` (default: every 10, twins every 50), `none`; `--replay`, `--continuation R` |
| AI version tournament: win share with an interval, score, refusals, turn time ([ai-bench.md](../../platform/docs/ai-bench.md)) | `cargo run --release --offline -p aurelia-qa --bin ai-tournament -- --suite standard --candidate nudged` |
| A long match: memory, round time, saving | `cargo run --release --offline -p aurelia-qa --example soak -- standard continents 500 1` |
| `desktop` without a window, 300 turns with presentation, frames at turns 100/200/300 | `AURELIA_GAME_AUTOPLAY=1 AURELIA_GAME_TURNS=300 AURELIA_GAME_TURN_FRAMES=4 AURELIA_GAME_CAPTURE_EVERY=100 desktop --screenshot out/qa.png` |

What is checked in every match (`integration/crates/aurelia-qa/src/runner.rs`, `invariants.rs`, `checks.rs`):

- `GameState::validate`: the world validator and a comparison of the derived state with a rebuild (visibility, incomes, etc.);
- knowledge rules: a visible tile is explored and remembered as it is now, the memory round is not from the future; contacts are not from the future; the AI observation shows foreign units and cities only on visible tiles;
- non-interference: the turn of the active player played twice from the saved AI memory - on the world and on a copy with a different resource where the player has not been - gives the same observation, commands and memory;
- replay: the same match twice gives the same hash after every round;
- saving to bytes (the `desktop` codec) in the middle of a match and loading: both matches play on with the same hash every round and are saved to the same bytes;
- the report: the winner and kind of victory, rounds; per nation - cities (and the peak), population, technologies, policies, ideology, religion, wonders, allied city-states, spaceship parts, tourism influence, score, army, nations met, treasury and income, improved tiles, workers, happiness; wars (start, end), congress sessions, the UN; round time p50/p95/max; AI "stalls" (a city with no queue and no process for ≥ 3 rounds, a settler alive for ≥ 30 rounds, no research for ≥ 3 rounds, a nation without a second city by round 100).

## Results

All matches are BNW, AI against AI, city-states and barbarians at the size default, 500 rounds (a win on time) unless stated otherwise. Machine: Apple M4 Max; other agents were working on it during the measurements (load average 75-95), so the time comes with the caveat below.

### Matrix: size × script × difficulty (45 matches, `light` checks, not a single invariant violation)

Victories other than time: `—` - the match ended with a win on time at round 500.

| Size / script | Settler | Prince | Deity |
|---|---|---|---|
| Duel continents | — | — | culture, r. 359 |
| Duel pangaea | — | — | diplomacy, r. 477 |
| Duel archipelago | — | — | — |
| Duel fractal | — | — | — |
| Duel small-continents | — | — | culture, r. 346 |
| Tiny (all 5 scripts) | — | — | — |
| Small (all 5 scripts) | — | — | — |

Wars: 0-4 in duels, 2-19 on Tiny, 12-42 on Small; the congress is convened in all Small/Tiny matches on Deity and in most on Prince.

### Each victory separately (`--only`, Prince)

| Victory | 500 rounds: Duel/Tiny/Small × 5 scripts (15 matches) | 1000 rounds: Duel × continents/pangaea/fractal |
|---|---|---|
| Science | 0 | continents, r. 855 |
| Diplomatic | 0 | pangaea, r. 694 |
| Culture | 0 | 0 |
| Domination | 0 | 0 |
| Time | all | — |

Before the M4-A economy merge (faster-growing empires), after the QA fixes a science victory was reached in 7 of 10 Duel/Tiny matches in 331-496 rounds, there were domination wins (pangaea/duel, r. 230), culture wins (fractal/duel, r. 319, 262), diplomacy wins (fractal/small, r. 473; pangaea/duel, r. 370). With the BNW economy the AI empires grow about half as fast (see "Defects", item 1), and in 500 rounds only time comes in; all kinds of victory except domination and culture on Prince are reachable in a longer match or on Deity.

### Standard, Large, Huge (Prince, seed 1, 500 rounds, no checks)

`full_game --size standard,large,huge --script continents,pangaea --checks none --civs --stalls`. The correctness of these sizes is checked by the long test `every_size_and_script_ends_with_valid_worlds`, and by the test on Standard: replay, saving and continuation.

| Game | Victory | Wars | Congress sessions | Cities (all) | Technologies of the leader | p50 / p95 ms | Stalls |
|---|---|---:|---:|---:|---:|---|---:|
| continents/standard | time, P1 | 41 | 7 | 51 | 47 | 16 / 111 | 3 |
| pangaea/standard | time, P5 | 34 | 2 | 62 | 44 | 16 / 231 | 8 |
| continents/large | time, P7 | 49 | 0 | 72 | 48 | 51 / 294 | 10 |
| pangaea/large | time, P3 | 28 | 0 | 62 | 51 | 24 / 230 | 16 |
| continents/huge | time, P11 | 61 | 0 | 100 | 52 | 30 / 51 | 10 |
| pangaea/huge | time, P5 | 27 | 0 | 71 | 54 | 27 / 51 | 17 |

On Large and Huge the congress is not convened: only a nation with printing that has met all the others can found a congress, and by round 500 nobody had met everyone (the best: 8 of 9 neighbours on Large, 9 of 11 on Huge). The causes are the slow pace (item 1 of the defects) and weak scouting over the sea.

## Measurements

Method: release, a separate `CARGO_TARGET_DIR`, the binary copied before the run; round time is all AI turns of the round together with the kernel's turn processing (`AiDriver::run` up to `RoundDone`); the checks run after the measurement and are not part of it. Memory is the process RSS (`ps -o rss`). The machine's load average during the measurements was 75-95 (parallel builds of other agents): medians are stable, p95 and the maximum are inflated by thread preemption; a repeat of the same seed is given for comparison.

| Measurement | Result |
|---|---|
| Soak: Standard, continents, seed 1, 500 rounds, time only | RSS 17 MB (r. 25-100) → 25 MB (r. 300) → 23 MB (r. 500): no growth. Units 210-250, cities 67 |
| Round time, soak | p50 21.8 ms, p95 116.7 ms, max 248.5 ms (under load; 100-round windows: p50 7 → 16 → 21 → 36 → 38 ms). A repeat of the same seed without checks at a lower load: p50 13.3, p95 21.6, max 24.5 ms, the final hash is the same (checks do not change the match) |
| Saving at round 500 | 485 KiB, write 1.6 ms, read and decode 2.6 ms (r. 100: 346 KiB, 1.5 / 3.0 ms) |
| AI round contract (`classic-rules` example `ai_rounds 5 8 2`) | representative 200×125: median 9.1 ms, p95 86.6 ms (≤ 250 ms); stress 224×224: median 238 ms, p95 614 ms (≤ 1 s). The longest driver step in a separate per-phase measurement: 1.6 ms on stress (PlanNext), 1.0 ms Observe; in the run under load - 190 ms (preemption) |
| `desktop` without a window, a passive human, 300 turns, 4 frames per turn | no panics, 7.9 s, max RSS 170 MB; the human was destroyed on turn 190, afterwards the defeat screen |
| `desktop`, `AURELIA_GAME_AUTOPLAY`, 300 iterations, 4 frames per iteration | no panics, the match reached turn 500 (end on time), 32 s; RSS 237 MB (turn 281) → 251 MB (399) → 260 MB (499), max 280 MB; an iteration 86-355 ms. The frames t100/t200/t300 were reviewed: relief, cities with banners, units, fog, the top panel and notifications are in place |
| AI economy, 12 matches Small/Standard × pangaea/continents/fractal × 2 seeds, round 250 | bankrupts (0 gold and a deficit) 38 % → 3 %; average income −3.3 → +5.5 per turn; military units 2.9 → 6.1; cities 3.8 → 4.2 (before and after the AI economy fixes) |

## Fixed defects

Each one is a separate commit with a regression test.

| Defect (where found) | Fix | Test |
|---|---|---|
| A missionary (RivalTerritory) walked through a closed border, the command was rejected after the move, visibility and knowledge diverged (fractal/small, r. 193) | one border rule `unit_may_enter`; a rejected command panics in debug, in release it still syncs the derived state | `full_game_regressions::a_missionary_marches_through_closed_borders` |
| A city-state without cities lived on units; the death of the units did not recompute the friends' bonuses (continents/small, r. 197) | a player without a city (after the first) drops out together with its units, the effects of all are recomputed | `…::a_city_state_without_cities_is_eliminated_with_its_units` |
| A city-state joining an ally's war left the enemy the friendship bonus in incomes (fractal/small/2, r. 79) | `set_war_state` journals the recomputation | `…::a_city_state_joining_its_allys_war_ends_the_enemys_friendship_bonus` |
| A change of the religion majority without a change in the number of followers did not recompute incomes (pangaea/duel, r. 432) | compare both the followers and the majority | `aurelia-qa full_game::duel_games_end_on_every_map_script` |
| An empty queue built warriors for free, the AI finished the queue off with workers: 60 units in the capital | an empty queue builds nothing; later the AI gets a Wealth/Research process | `production::an_empty_queue_builds_nothing_and_keeps_the_production`, `ai_treasury::a_city_with_nothing_to_build_works_a_process` |
| The AI never had a science victory | Apollo, exactly the missing parts, clearing the capital | `ai_victories::*` |
| Great scientists, engineers, merchants stood in the capital | the Unit_Builds catalog; the AI leads them to the place of improvement | `full_game_regressions::great_people_build_…`, `…::the_ai_builds_a_customs_house_with_its_great_merchant` |
| The AI never landed on water: on archipelago nobody met anybody | AI paths by the unit's profile, scouting over the sea, Optics/Astronomy | `ai_seafaring::*` |
| The AI went bankrupt by round ~200 | roads to the capital, no extra workers, gold more important in a deficit, a defender in every city, happiness more important under unhappiness | `ai_treasury::*`, `planner_value::tests::weights_follow_the_treasury_and_the_mood` |
| Settlers walked back and forth from a barbarian for hundreds of rounds | places away from threats, waiting for an escort, the garrison escorts | `ai_treasury::a_settler_threatened_by_a_barbarian_still_founds_its_city` |
| Happiness fell from +11 to +4 by round 60: the luxury within the borders is not connected, the first worker at round 50-60 | the empire's first worker is three times as expensive as soon as a settler is on the way (`allocator::FIRST_WORKER_FACTOR`) | `happiness_pace::the_first_workers_connect_luxuries_by_round_60` |

## Known defects and gaps

The check of 19 October ([report: classic-parity-2026-10-19]; not rechecked in code, except item 8): item 1 - the pace was shifted by the BNW formulas and the pace AI, the remainder (4+ cities by round 100) is in [ru/PLAN.en.md](../../ru/PLAN.en.md); item 4 - the AI fleet and landing are merged (`ai/fleet.rs`), landings are rare ([ai.md](ai.md), contract 6); item 8 - the missionary limit is now `MISSIONARIES_WANTED = 2` (`ai/religion.rs:28`); items 3, 5, 6, 7, 9-11 were not rechecked, the spec-vs-code audit checks them.

1. **The pace of the match is about half as fast as in Civ V.** By round 300 the AI has 30-38 technologies, a population of 30-80 in 4-8 cities; by 500 - 40-55 technologies of 81. The cause is growth: the AI is unhappy in ~30 % of turns (growth −75 %), few improved tiles in some empires, small cities on bad sites. The consequence: in 500 rounds on Prince only time wins; a science victory comes at about round 855. AI work on happiness (luxury, buildings, policies) is needed - the next step. 18 October (platform/docs/ai-bench.md, "Match pace"): the BNW cost formulas and the pace AI shifted the eras by 3-13 rounds and brought the second city twice as close (the first in a match 42 → 26), but by round 100 a site still has 2.6 cities and 16 technologies: after the third city a settler is negative in happiness price. Further (the same place, "Continuation"): Avoid Growth for the player and the AI, libraries in the allocator's price; sites at −10 and below at the end - twice as rare, Industrial/Modern 12-21 rounds earlier, but the second city of an average site at round 43, cities at round 100 3.1 and 100 % of matches ending on time: what remains is happiness on the fourth city (platform/docs/ai-bench.md, "Settlers"). The branch `civ5/happiness` (the same place, "Happiness and the first worker"): the first worker earlier - happiness at round 60 +7.0 instead of +3.9, luxuries 0.71 kinds instead of 0.28, cities at round 100 3.6 instead of 3.1, score +1…5 %; `OPENING_CITIES` 4 gives 4.2 cities, but costs strength (57.7 % against `allocator`).
2. ~~Technology cost not per section 10.2 of `rules.md`~~ - closed on 18 October: `derived::research` computes a technology price with the difficulty percentage (AI 85 %), the map percentage, +5 % per city (by the largest number of cities during the game) and a discount for knowing nations; the same for AI policies (75 %), buildings and national wonders (+30 hammers per city) and unhappiness from cities on Large and Huge. The measurement is platform/docs/ai-bench.md, "Match pace".
3. **Coast and ocean without gold**: in the catalog `terrain.json` Coast/Ocean give only 1 food; in Civ V (Terrain_Yields) there are 1 food and 1 gold - check the source.
4. **AI domination is almost unreachable**: the AI does not take a city with one warrior using five swordsmen and four archers if the defenders do not come out of the city (the conquest test relies on the rival training warriors); naval landings are rare (the fleet and landing are merged, see the check above).
5. **A culture victory** only on Deity in duels: the AI's tourism is weak.
6. **Settlers live longer than 30 rounds** (on Large/Huge - for most nations). The causes from the trace analysis (`continents/standard` P6, `continents/large` P3):
   - the best sites lie behind a barbarian camp that the AI does not destroy: its units drive the settler away again and again;
   - during a war with a city-state its army stands at the cities, and the AI keeps building settlers in the Expand strategy;
   - settlers hinder each other in narrow places (`Blocked`).

   Three fixes were tried: a settler waits for an escort in the city, the city trains an escort, sites near a known camp are excluded. On 4 Standard/Large games neither the number of stalls nor the number of cities improved, so the fixes were reverted. Two things are needed: the AI must destroy barbarian camps near its borders, and the strategy during a war near the cities must not build settlers. Separately: there is no "disband unit" command in the kernel.
7. **Cities without a queue and without a process**: Wealth and Research are unlocked by medieval technologies. Before that, an empire in a deficit that has no available buildings without upkeep, and whose army and workers are at the limit, builds nothing. Production accumulates in the stock (`an_empty_queue_builds_nothing_and_keeps_the_production`) but is not spent. It occurs in 1-2 of 10 nations on Large.
8. The AI sometimes has more missionaries than `MISSIONARIES_WANTED` (5 at a limit of 2) - not investigated.
9. A trade route is searched by Dijkstra, in Civ V by A* (low priority, a message from the economy engineer).
10. In autoplay mode the button at the bottom shows "Game over", and the turn counter grows by more than one per iteration (several AI rounds per iteration) - a cosmetic issue of the mode, not of the game.
11. The time measurements were made under load (load average 75-105); the perf contract needs a repeat on an idle machine.
