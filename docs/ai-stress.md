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

See the end of this file (filled after the run).
