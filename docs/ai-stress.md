# ai-stress: the situation exam of the AI

A measurement, not a change: it alters no rule and no AI. It takes a real mid-game position, changes ONE
thing, plays on with the game's own AI on every seat, and compares the result with the same position
played on without the change (the control). The table "edited minus control" is judged against gates that
were written down before the first run and are not changed after seeing a result.

Code: `crates/qa/src/stress/` and `crates/qa/src/bin/ai-stress.rs`. Catalogue of the situations, with the
expectation and the owning small net of each: `work/ai-scenarios.html`.

## How to run

Every cargo build and every run goes through the resource broker (`work/res/res run --cpu 2 --name stress -- ...`).

```
# 1. Positions: logs of the first rounds of whole games (the AI plays by the game's own length).
ai-play --suite pace --speed quick --variant standard --games 100 --stop-round 112 --jobs 2 --out POSITIONS

# 2. The exam: every (situation, instance) at three strengths, the control once.
ai-stress run --positions POSITIONS --out OUT --situations a5,b1,a1,b4,b9,d1,c5 --instances 100 --jobs 2
#   --round 100 --jitter 10   an instance starts at round 100 + a draw of 0..=10 from its index
#   --no-end                  play the window only (default: each arm is played out to the end of the game)

# 3. One instance with a line a round on stderr (STRESS_TRACE), and the row.
ai-stress one --positions POSITIONS --situation a5 --instance 3 --level 2

# 4. The result table against the gates; a saved edited start built again.
ai-stress report --out OUT
ai-stress rebuild --scene OUT/scenes/a5-003-L2.json
```

`OUT/rows.jsonl` (one row per situation, instance and level; a rerun skips the rows it has),
`OUT/unbuilt.jsonl` (a situation that cannot be built on a position, with the reason), `OUT/scenes/`.

## What a row holds

For one (situation, instance, level): `edited` and `control`, each a map metric -> number, and `info`.
`seat_*` are the OUTCOME of the nations the situation judges after the window: `seat_cities`, `seat_techs`,
`seat_score_share` (their share of the score of all nations), `seat_won` (a real victory of one of them).
`end_seat_*` are the same at the end of the game, with `end_seat_win_weight`: a real victory of one of them
counts 1, a time victory a quarter, else 0. The other metrics are BEHAVIOUR diagnostics named for the
expectation (`weak_captured`, `counter_by_3`, `wars_on_leader`, ...); a share is 0 or 1 a position and the
report averages it.

## Strengths

Every situation with a strength is run at three levels, and a failure at the mild level is the more serious
finding (a human is much stronger than our AI):

| situation | mild | medium | strong |
|---|---|---|---|
| A5 weak neighbour | 50 % of its military units removed | 75 % | 100 % |
| B1 invasion | 4 enemy units | 7 | 10 |
| A1 runaway science | science x1.25 | x1.5 | x2 |
| B4 capital lost | the capital | + the nearest city | + the two nearest |
| B9 near victory | 3 parts missing | 2 | 1 |
| D1, C5 | one level | | |

## The situations built

- A5: the two nations whose cities are nearest on one landmass; the weak one (alternating by instance) loses
  its military units (any domain, city defenders included); the neighbour is judged. Window 40 rounds.
- B1: a war between two nations at war now, else the nearest pair put at war (`info.war`, in the base: both arms
  have it); units of the enemy's own best kinds (melee, ranged, mounted by the enemy's technologies and nation)
  placed on free tiles of the seat's land 1-3 tiles from its cities. Window 15.
- A1: the nation with the most science; every round the edited arm adds a share of the beakers it earned to its
  pool (`Gift`: the rules have no per-seat science setting; the gift lives only in the exam's play loop). The
  others are judged. Window 60.
- B4: the capital of one of the nearest pair goes to the other by a real capture (`debug_capture_city`); the base
  of both arms is the war between them. Window 30.
- B9: the rival with the most science gets the space technologies (base) and Apollo and the spaceship parts but
  some (edit). Others judged; `rival_wins_science` is the real science victory. Window 40.
- D1: the nation with the most cities, and the two nations nearest to it, put at war with it at once (the
  edit); the two are judged.
- C5: the congress host is taken out of the game (its cities to the nearest nation, its units removed); the
  metric reads whether the congress keeps the dead nation as host (the known defect, congress.rs :477 and :570;
  measured here, not fixed).

## Gates (written before the first run)

An outcome gate reads what happened to the nations in focus; a behaviour gate is a diagnostic. A situation
passes when its outcome gates pass at every level. `ai-stress gates` prints this table; the test
`gates_match_the_document` keeps it equal to the code.

| situation | kind | metric | test | mild | medium | strong | owning small net |
|---|---|---|---|---|---|---|---|
| A5 | outcome | `seat_cities` | delta >= | 0.2 | 0.35 | 0.5 | strategist (posture), commander |
| A5 | behaviour | `weak_captured` | delta >= | 0.15 | 0.25 | 0.3 | commander |
| A5 | behaviour | `nobody_attacked` | edited <= | 0.6 | 0.5 | 0.4 | strategist (posture) |
| B1 | outcome | `seat_cities` | delta >= | -0.3 | -0.5 | -0.8 | tactician, commander, danger net |
| B1 | outcome | `seat_score_share` | delta >= | -0.01 | -0.02 | -0.03 | tactician, commander, danger net |
| B1 | behaviour | `counter_by_3` | edited >= | 0.8 | 0.8 | 0.8 | commander, tactician |
| B1 | behaviour | `purchases` | delta >= | 0.5 | 1 | 1 | tactician, danger net |
| A1 | outcome | `seat_score_share` | delta >= | -0.01 | -0.02 | -0.04 | strategist (no diplomacy outputs) |
| A1 | outcome | `leader_won` | edited <= | 0.05 | 0.1 | 0.2 | strategist |
| A1 | behaviour | `wars_on_leader` | delta >= | 0.25 | 0.5 | 0.5 | strategist |
| A1 | behaviour | `denouncements_of_leader` | delta >= | 0.5 | 1 | 1 | strategist |
| A1 | behaviour | `research_pacts_others` | delta >= | 0.25 | 0.5 | 0.5 | strategist |
| A1 | behaviour | `tech_gap` | delta <= | 2 | 4 | 8 | strategist |
| B4 | outcome | `seat_cities` | delta >= | -0.7 | -1.4 | -2.1 | commander, scripted retake |
| B4 | outcome | `seat_score_share` | delta >= | -0.04 | -0.07 | -0.1 | commander, scripted retake |
| B4 | behaviour | `capital_retaken` | edited >= | 0.3 | 0.3 | 0.3 | commander, scripted retake |
| B4 | behaviour | `further_losses` | delta <= | 0.5 | 0.8 | 1 | commander |
| B9 | outcome | `rival_wins_science` | edited <= | 0.5 | 0.4 | 0.3 | strategist |
| B9 | behaviour | `rival_cities_lost` | delta >= | 0.5 | 0.5 | 0.5 | strategist, commander |
| D1 | outcome | `seat_cities` | delta >= | 0.3 | 0.3 | 0.3 | none (no net for allied action) |
| D1 | behaviour | `both_struck` | edited >= | 0.5 | 0.5 | 0.5 | none |
| D1 | behaviour | `separate_peace` | edited <= | 0.3 | 0.3 | 0.3 | none |
| C5 | behaviour | `host_dead_at_end` | edited <= | 0 | 0 | 0 | none (the rules' table: congress.rs) |
| A5 | outcome | `end_seat_win_weight` | delta >= | 0 | 0 | 0 | strategist (posture), commander |
| A5 | outcome | `end_seat_score_share` | delta >= | 0 | 0.005 | 0.01 | strategist (posture), commander |
| B1 | outcome | `end_seat_win_weight` | delta >= | -0.03 | -0.05 | -0.08 | tactician, commander, danger net |
| B1 | outcome | `end_seat_score_share` | delta >= | -0.01 | -0.02 | -0.03 | tactician, commander, danger net |
| A1 | outcome | `end_seat_win_weight` | delta >= | -0.05 | -0.1 | -0.15 | strategist |
| A1 | outcome | `end_seat_score_share` | delta >= | -0.02 | -0.04 | -0.06 | strategist |
| B4 | outcome | `end_seat_win_weight` | delta >= | -0.03 | -0.05 | -0.08 | commander, scripted retake |
| B4 | outcome | `end_seat_score_share` | delta >= | -0.03 | -0.05 | -0.07 | commander, scripted retake |
| B9 | outcome | `end_seat_win_weight` | delta >= | -0.05 | -0.1 | -0.15 | strategist |
| B9 | outcome | `end_seat_score_share` | delta >= | -0.02 | -0.04 | -0.06 | strategist |
| D1 | outcome | `end_seat_win_weight` | delta >= | 0 | 0 | 0 | none |
| D1 | outcome | `end_seat_score_share` | delta >= | 0 | 0 | 0 | none |

## Saved scenes (for forks)

Every edited start is saved as `OUT/scenes/<situation>-<instance>-L<level>.json`:

```
{ "log": "/abs/path/N-pace-script-small.aurlog",   the recorded game
  "round": 104,                                    replay it to the start of this round
  "situation": "a5", "level": 2, "instance": 3,
  "roles": {...},                                  who is who (scene::Roles)
  "base":  [ {"change":"war","a":1,"b":4}, ... ],  made in both arms (edit::Change, in order)
  "edit":  [ {"change":"remove_units","ids":[...]}, ... ],   made in the edited arm only
  "gift":  {"player":2,"percent":100} | null,      a per-round science gift (edit::Gift), made by the play loop
  "world": "9f3c..." }                             the world hash after replay, base and edit
```

`stress::rebuild(rules, &saved)` replays the log, makes the base and the edit and checks the hash: the edited
position is rebuilt exactly. A fork plays on from it with any mind or any forced decision.

## Edits are data (the grammar)

An edit is a list of `edit::Change` values (`meet`, `war`, `remove_units`, `spawn`, `capture_city`,
`discover`, `project`) plus an optional per-round `Gift`. The situations only choose the changes; the saved
scene, a random sampler and the scripted adversaries of wave 2 all use the same primitives.

## Wave 2: how a scripted adversary plugs in

A scripted adversary is a seat whose turn is played by a script instead of the AI: a `Seat` hook in
`play::play_arm` called at the start of the seat's turn with the world, that returns `Command`s (forward-settle,
plant citadels, bait unit, pillage). The harness already has the other parts: the position and edit
(`Scene`), the control without the adversary, the watch, the saved scene. Not built in wave 1.

## Results


Run on 22 November 2026: 100 positions of the pace suite (Small, six nations, Quick, standard AI, rounds 100-110;
C5 at rounds 150-160 from a second set), every arm played out to the end of the game (an arm costs about 5 CPU
seconds, so all 100 instances carry the end metrics). Gates unchanged. Raw rows: `runs/wave1`, `runs/b1b`, `runs/d1b`,
`runs/c5`, `runs/random` (under `work/nets/stress/`). A situation is built on 81 positions (A5, B1, B4: the others have no
two nations on one landmass), 57 (D1), 100 (A1, B9), 61 (C5: the congress is founded on 61).

## Measurement fixes after the first run (gates not changed)

- D1 `both_struck` counted melee attacks only: ranged attacks were missing, so it read 0.000. Fixed; rerun in `runs/d1b`.
- B1 `counter_by_3` counted an invader hurt by anyone, a city's strike included: it read 1.000 at every level. It now
  counts only strikes of the seat's own units (melee, ranged) on the enemy within three rounds; the old value is kept as
  `invader_hurt_by_3`. Rerun in `runs/b1b`.

Before and after:

| situation | level | kind | metric | n | edited | control | delta (se) | gate | result |
|---|---|---|---|---|---|---|---|---|---|
| D1 | 0 | behaviour | `both_struck` | 57 | 0.000 | 0.000 | +0.000 (0.000) | edited >= 0.5 | FAIL |
| B1 | 0 | behaviour | `counter_by_3` | 81 | 1.000 | 0.000 | +1.000 (0.000) | edited >= 0.8 | pass |
| B1 | 1 | behaviour | `counter_by_3` | 81 | 1.000 | 0.000 | +1.000 (0.000) | edited >= 0.8 | pass |
| B1 | 2 | behaviour | `counter_by_3` | 81 | 1.000 | 0.000 | +1.000 (0.000) | edited >= 0.8 | pass |

After the fix:

| situation | level | kind | metric | n | edited | control | delta (se) | gate | result |
|---|---|---|---|---|---|---|---|---|---|
| D1 | 0 | outcome | `seat_cities` | 57 | 3.789 | 4.053 | -0.263 (0.069) | delta >= 0.3 | FAIL |
| D1 | 0 | behaviour | `both_struck` | 57 | 0.088 | 0.000 | +0.088 (0.038) | edited >= 0.5 | FAIL |
| D1 | 0 | behaviour | `separate_peace` | 57 | 0.000 | 0.000 | +0.000 (0.000) | edited <= 0.3 | pass |
| D1 | 0 | outcome | `end_seat_win_weight` | 57 | 0.083 | 0.123 | -0.039 (0.028) | delta >= 0 | FAIL |
| D1 | 0 | outcome | `end_seat_score_share` | 57 | 0.330 | 0.337 | -0.007 (0.005) | delta >= 0 | FAIL |
| B1 | 0 | behaviour | `counter_by_3` | 81 | 0.716 | 0.198 | +0.519 (0.061) | edited >= 0.8 | FAIL |
| B1 | 1 | behaviour | `counter_by_3` | 81 | 0.790 | 0.198 | +0.593 (0.058) | edited >= 0.8 | FAIL |
| B1 | 2 | behaviour | `counter_by_3` | 81 | 0.914 | 0.198 | +0.716 (0.050) | edited >= 0.8 | pass |

## Result table (A5, A1, B4, B9 from `runs/wave1`; B1, D1 reruns; C5)

| situation | level | kind | metric | n | edited | control | delta (se) | gate | result |
|---|---|---|---|---|---|---|---|---|---|
| A5 | 0 | outcome | `seat_cities` | 81 | 4.407 | 4.309 | +0.099 (0.076) | delta >= 0.2 | FAIL |
| A5 | 0 | behaviour | `weak_captured` | 81 | 0.148 | 0.099 | +0.049 (0.030) | delta >= 0.15 | FAIL |
| A5 | 0 | behaviour | `nobody_attacked` | 81 | 0.852 | 0.889 | -0.037 (0.033) | edited <= 0.6 | FAIL |
| A5 | 0 | outcome | `end_seat_win_weight` | 81 | 0.068 | 0.040 | +0.028 (0.018) | delta >= 0 | pass |
| A5 | 0 | outcome | `end_seat_score_share` | 81 | 0.164 | 0.155 | +0.009 (0.004) | delta >= 0 | pass |
| A5 | 1 | outcome | `seat_cities` | 81 | 4.358 | 4.309 | +0.049 (0.066) | delta >= 0.35 | FAIL |
| A5 | 1 | behaviour | `weak_captured` | 81 | 0.062 | 0.099 | -0.037 (0.027) | delta >= 0.25 | FAIL |
| A5 | 1 | behaviour | `nobody_attacked` | 81 | 0.914 | 0.889 | +0.025 (0.030) | edited <= 0.5 | FAIL |
| A5 | 1 | outcome | `end_seat_win_weight` | 81 | 0.071 | 0.040 | +0.031 (0.017) | delta >= 0 | pass |
| A5 | 1 | outcome | `end_seat_score_share` | 81 | 0.167 | 0.155 | +0.012 (0.004) | delta >= 0.005 | pass |
| A5 | 2 | outcome | `seat_cities` | 81 | 4.370 | 4.309 | +0.062 (0.075) | delta >= 0.5 | FAIL |
| A5 | 2 | behaviour | `weak_captured` | 81 | 0.086 | 0.099 | -0.012 (0.028) | delta >= 0.3 | FAIL |
| A5 | 2 | behaviour | `nobody_attacked` | 81 | 0.877 | 0.889 | -0.012 (0.033) | edited <= 0.4 | FAIL |
| A5 | 2 | outcome | `end_seat_win_weight` | 81 | 0.065 | 0.040 | +0.025 (0.018) | delta >= 0 | pass |
| A5 | 2 | outcome | `end_seat_score_share` | 81 | 0.167 | 0.155 | +0.012 (0.003) | delta >= 0.01 | pass |
| A1 | 0 | outcome | `seat_score_share` | 100 | 0.762 | 0.773 | -0.011 (0.001) | delta >= -0.01 | FAIL |
| A1 | 0 | outcome | `leader_won` | 100 | 0.000 | 0.000 | +0.000 (0.000) | edited <= 0.05 | pass |
| A1 | 0 | behaviour | `wars_on_leader` | 100 | 0.030 | 0.030 | +0.000 (0.000) | delta >= 0.25 | FAIL |
| A1 | 0 | behaviour | `denouncements_of_leader` | 100 | 0.170 | 0.180 | -0.010 (0.039) | delta >= 0.5 | FAIL |
| A1 | 0 | behaviour | `research_pacts_others` | 100 | 1.120 | 1.100 | +0.020 (0.072) | delta >= 0.25 | FAIL |
| A1 | 0 | behaviour | `tech_gap` | 100 | 5.178 | 3.708 | +1.470 (0.125) | delta <= 2 | pass |
| A1 | 0 | outcome | `end_seat_win_weight` | 100 | 0.180 | 0.107 | +0.072 (0.028) | delta >= -0.05 | pass |
| A1 | 0 | outcome | `end_seat_score_share` | 100 | 0.759 | 0.767 | -0.009 (0.003) | delta >= -0.02 | pass |
| A1 | 1 | outcome | `seat_score_share` | 100 | 0.751 | 0.773 | -0.022 (0.002) | delta >= -0.02 | FAIL |
| A1 | 1 | outcome | `leader_won` | 100 | 0.000 | 0.000 | +0.000 (0.000) | edited <= 0.1 | pass |
| A1 | 1 | behaviour | `wars_on_leader` | 100 | 0.030 | 0.030 | +0.000 (0.000) | delta >= 0.5 | FAIL |
| A1 | 1 | behaviour | `denouncements_of_leader` | 100 | 0.140 | 0.180 | -0.040 (0.055) | delta >= 1 | FAIL |
| A1 | 1 | behaviour | `research_pacts_others` | 100 | 1.110 | 1.100 | +0.010 (0.059) | delta >= 0.5 | FAIL |
| A1 | 1 | behaviour | `tech_gap` | 100 | 6.786 | 3.708 | +3.078 (0.138) | delta <= 4 | pass |
| A1 | 1 | outcome | `end_seat_win_weight` | 100 | 0.175 | 0.107 | +0.068 (0.030) | delta >= -0.1 | pass |
| A1 | 1 | outcome | `end_seat_score_share` | 100 | 0.751 | 0.767 | -0.016 (0.003) | delta >= -0.04 | pass |
| A1 | 2 | outcome | `seat_score_share` | 100 | 0.740 | 0.773 | -0.033 (0.002) | delta >= -0.04 | pass |
| A1 | 2 | outcome | `leader_won` | 100 | 0.000 | 0.000 | +0.000 (0.000) | edited <= 0.2 | pass |
| A1 | 2 | behaviour | `wars_on_leader` | 100 | 0.010 | 0.030 | -0.020 (0.014) | delta >= 0.5 | FAIL |
| A1 | 2 | behaviour | `denouncements_of_leader` | 100 | 0.120 | 0.180 | -0.060 (0.045) | delta >= 1 | FAIL |
| A1 | 2 | behaviour | `research_pacts_others` | 100 | 1.090 | 1.100 | -0.010 (0.070) | delta >= 0.5 | FAIL |
| A1 | 2 | behaviour | `tech_gap` | 100 | 8.760 | 3.708 | +5.052 (0.145) | delta <= 8 | pass |
| A1 | 2 | outcome | `end_seat_win_weight` | 100 | 0.125 | 0.107 | +0.018 (0.025) | delta >= -0.15 | pass |
| A1 | 2 | outcome | `end_seat_score_share` | 100 | 0.741 | 0.767 | -0.026 (0.003) | delta >= -0.06 | pass |
| B4 | 0 | outcome | `seat_cities` | 81 | 3.210 | 4.012 | -0.802 (0.097) | delta >= -0.7 | FAIL |
| B4 | 0 | outcome | `seat_score_share` | 81 | 0.112 | 0.160 | -0.048 (0.004) | delta >= -0.04 | FAIL |
| B4 | 0 | behaviour | `capital_retaken` | 81 | 0.210 | 0.000 | +0.210 (0.046) | edited >= 0.3 | FAIL |
| B4 | 0 | behaviour | `further_losses` | 81 | 0.099 | 0.000 | +0.099 (0.045) | delta <= 0.5 | pass |
| B4 | 0 | outcome | `end_seat_win_weight` | 81 | 0.003 | 0.062 | -0.059 (0.022) | delta >= -0.03 | FAIL |
| B4 | 0 | outcome | `end_seat_score_share` | 81 | 0.099 | 0.152 | -0.053 (0.005) | delta >= -0.03 | FAIL |
| B4 | 1 | outcome | `seat_cities` | 81 | 2.469 | 4.012 | -1.543 (0.129) | delta >= -1.4 | FAIL |
| B4 | 1 | outcome | `seat_score_share` | 81 | 0.085 | 0.160 | -0.074 (0.004) | delta >= -0.07 | FAIL |
| B4 | 1 | behaviour | `capital_retaken` | 81 | 0.148 | 0.000 | +0.148 (0.040) | edited >= 0.3 | FAIL |
| B4 | 1 | behaviour | `further_losses` | 81 | 0.086 | 0.000 | +0.086 (0.036) | delta <= 0.8 | pass |
| B4 | 1 | outcome | `end_seat_win_weight` | 81 | 0.000 | 0.062 | -0.062 (0.022) | delta >= -0.05 | FAIL |
| B4 | 1 | outcome | `end_seat_score_share` | 81 | 0.073 | 0.152 | -0.079 (0.005) | delta >= -0.05 | FAIL |
| B4 | 2 | outcome | `seat_cities` | 81 | 1.272 | 4.012 | -2.741 (0.135) | delta >= -2.1 | FAIL |
| B4 | 2 | outcome | `seat_score_share` | 81 | 0.056 | 0.160 | -0.104 (0.004) | delta >= -0.1 | FAIL |
| B4 | 2 | behaviour | `capital_retaken` | 81 | 0.049 | 0.000 | +0.049 (0.024) | edited >= 0.3 | FAIL |
| B4 | 2 | behaviour | `further_losses` | 81 | 0.049 | 0.000 | +0.049 (0.030) | delta <= 1 | pass |
| B4 | 2 | outcome | `end_seat_win_weight` | 81 | 0.000 | 0.062 | -0.062 (0.022) | delta >= -0.08 | pass |
| B4 | 2 | outcome | `end_seat_score_share` | 81 | 0.039 | 0.152 | -0.113 (0.005) | delta >= -0.07 | FAIL |
| B9 | 0 | outcome | `rival_wins_science` | 100 | 0.330 | 0.060 | +0.270 (0.045) | edited <= 0.5 | pass |
| B9 | 0 | behaviour | `rival_cities_lost` | 100 | 0.000 | 0.000 | +0.000 (0.000) | delta >= 0.5 | FAIL |
| B9 | 0 | outcome | `end_seat_win_weight` | 100 | 0.033 | 0.062 | -0.030 (0.022) | delta >= -0.05 | pass |
| B9 | 0 | outcome | `end_seat_score_share` | 100 | 0.738 | 0.753 | -0.015 (0.002) | delta >= -0.02 | pass |
| B9 | 1 | outcome | `rival_wins_science` | 100 | 0.360 | 0.060 | +0.300 (0.046) | edited <= 0.4 | pass |
| B9 | 1 | behaviour | `rival_cities_lost` | 100 | 0.000 | 0.000 | +0.000 (0.000) | delta >= 0.5 | FAIL |
| B9 | 1 | outcome | `end_seat_win_weight` | 100 | 0.033 | 0.062 | -0.030 (0.017) | delta >= -0.1 | pass |
| B9 | 1 | outcome | `end_seat_score_share` | 100 | 0.736 | 0.753 | -0.017 (0.002) | delta >= -0.04 | pass |
| B9 | 2 | outcome | `rival_wins_science` | 100 | 0.390 | 0.060 | +0.330 (0.047) | edited <= 0.3 | FAIL |
| B9 | 2 | behaviour | `rival_cities_lost` | 100 | 0.000 | 0.000 | +0.000 (0.000) | delta >= 0.5 | FAIL |
| B9 | 2 | outcome | `end_seat_win_weight` | 100 | 0.043 | 0.062 | -0.020 (0.020) | delta >= -0.15 | pass |
| B9 | 2 | outcome | `end_seat_score_share` | 100 | 0.733 | 0.753 | -0.020 (0.002) | delta >= -0.06 | pass |
| B1 | 0 | outcome | `seat_cities` | 81 | 3.704 | 3.753 | -0.049 (0.039) | delta >= -0.3 | pass |
| B1 | 0 | outcome | `seat_score_share` | 81 | 0.158 | 0.162 | -0.004 (0.001) | delta >= -0.01 | pass |
| B1 | 0 | behaviour | `counter_by_3` | 81 | 0.716 | 0.198 | +0.519 (0.061) | edited >= 0.8 | FAIL |
| B1 | 0 | behaviour | `purchases` | 81 | 1.901 | 2.123 | -0.222 (0.197) | delta >= 0.5 | FAIL |
| B1 | 0 | outcome | `end_seat_win_weight` | 81 | 0.037 | 0.056 | -0.019 (0.014) | delta >= -0.03 | pass |
| B1 | 0 | outcome | `end_seat_score_share` | 81 | 0.150 | 0.150 | +0.000 (0.003) | delta >= -0.01 | pass |
| B1 | 1 | outcome | `seat_cities` | 81 | 3.691 | 3.753 | -0.062 (0.041) | delta >= -0.5 | pass |
| B1 | 1 | outcome | `seat_score_share` | 81 | 0.156 | 0.162 | -0.007 (0.001) | delta >= -0.02 | pass |
| B1 | 1 | behaviour | `counter_by_3` | 81 | 0.790 | 0.198 | +0.593 (0.058) | edited >= 0.8 | FAIL |
| B1 | 1 | behaviour | `purchases` | 81 | 2.148 | 2.123 | +0.025 (0.213) | delta >= 1 | FAIL |
| B1 | 1 | outcome | `end_seat_win_weight` | 81 | 0.052 | 0.056 | -0.003 (0.018) | delta >= -0.05 | pass |
| B1 | 1 | outcome | `end_seat_score_share` | 81 | 0.144 | 0.150 | -0.006 (0.003) | delta >= -0.02 | pass |
| B1 | 2 | outcome | `seat_cities` | 81 | 3.704 | 3.753 | -0.049 (0.039) | delta >= -0.8 | pass |
| B1 | 2 | outcome | `seat_score_share` | 81 | 0.156 | 0.162 | -0.007 (0.001) | delta >= -0.03 | pass |
| B1 | 2 | behaviour | `counter_by_3` | 81 | 0.914 | 0.198 | +0.716 (0.050) | edited >= 0.8 | pass |
| B1 | 2 | behaviour | `purchases` | 81 | 1.951 | 2.123 | -0.173 (0.207) | delta >= 1 | FAIL |
| B1 | 2 | outcome | `end_seat_win_weight` | 81 | 0.034 | 0.056 | -0.022 (0.019) | delta >= -0.08 | pass |
| B1 | 2 | outcome | `end_seat_score_share` | 81 | 0.150 | 0.150 | -0.000 (0.003) | delta >= -0.03 | pass |
| D1 | 0 | outcome | `seat_cities` | 57 | 3.789 | 4.053 | -0.263 (0.069) | delta >= 0.3 | FAIL |
| D1 | 0 | behaviour | `both_struck` | 57 | 0.088 | 0.000 | +0.088 (0.038) | edited >= 0.5 | FAIL |
| D1 | 0 | behaviour | `separate_peace` | 57 | 0.000 | 0.000 | +0.000 (0.000) | edited <= 0.3 | pass |
| D1 | 0 | outcome | `end_seat_win_weight` | 57 | 0.083 | 0.123 | -0.039 (0.028) | delta >= 0 | FAIL |
| D1 | 0 | outcome | `end_seat_score_share` | 57 | 0.330 | 0.337 | -0.007 (0.005) | delta >= 0 | FAIL |
| C5 | 0 | behaviour | `host_dead_at_end` | 61 | 0.164 | 0.000 | +0.164 (0.048) | edited <= 0 | FAIL |

## Random mode (200 samples, seed 1, `ai-stress random`), the 10 worst by damage to the seat

Damage = control minus edited of the seat's end score share plus a quarter of the victory weight.

| rank | damage | sample | position | recipe |
|---|---|---|---|---|
| 1 | +0.520 | 180 | 18-pace-small-continents-small r109 | seat 0 / Seize { taker: PlayerId(3), cities: 2 } + Invade { enemy: PlayerId(3), units: 11 } |
| 2 | +0.426 | 192 | 2-pace-fractal-small r104 | seat 3 / Seize { taker: PlayerId(2), cities: 1 } + Seize { taker: PlayerId(4), cities: 1 } |
| 3 | +0.357 | 113 | 20-pace-small-continents-small r106 | seat 3 / Seize { taker: PlayerId(0), cities: 1 } + Seize { taker: PlayerId(1), cities: 2 } |
| 4 | +0.310 | 70 | 6-pace-continents-small r109 | seat 5 / Seize { taker: PlayerId(0), cities: 3 } + Disarm { nation: PlayerId(3), percent: 59 } |
| 5 | +0.263 | 91 | 20-pace-fractal-small r104 | seat 4 / Seize { taker: PlayerId(5), cities: 2 } + Boost { nation: PlayerId(3), percent: 66 } |
| 6 | +0.226 | 188 | 1-pace-continents-small r103 | seat 5 / Seize { taker: PlayerId(1), cities: 1 } |
| 7 | +0.222 | 115 | 10-pace-archipelago-small r102 | seat 2 / Seize { taker: PlayerId(0), cities: 1 } |
| 8 | +0.214 | 14 | 13-pace-continents-small r107 | seat 1 / War { enemy: PlayerId(0) } + Seize { taker: PlayerId(4), cities: 2 } |
| 9 | +0.196 | 176 | 6-pace-fractal-small r101 | seat 1 / Seize { taker: PlayerId(4), cities: 1 } |
| 10 | +0.192 | 78 | 12-pace-continents-small r100 | seat 5 / Seize { taker: PlayerId(2), cities: 1 } |
