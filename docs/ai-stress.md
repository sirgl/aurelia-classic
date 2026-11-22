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
| P3 sneak stack | 4 units inside at peace | 7 | 10 |
| P2 early rush | 2 units by the capital | 3 | 5 |
| P7 wonder snipe | one level | | |
| P5 border grab | a forward city | + 1 citadel | + 2 citadels and war |
| P1 raid-hunt | 2 raiders | 3 | 5 |
| P4 sell then war | price 30 % of the treasury | 60 % | 90 % |
| P8 alliance sweep | +500 gold | +1000 | +2000 (and the sweep script) |
| A2 military | +50 % units | +100 % | +200 % |
| A3 culture | x1.25 | x1.5 | x2 |
| A4 gold | x1.5 | x2 | x3 |
| A6, C2 weak over the sea | 50 % of its military removed | 75 % | 100 % |
| A7 we become weak | 33 % of our military removed | 50 % | 75 % |
| B2, B5 army at the border | 4 units | 7 | 10 |
| B6 strategic resource lost | the kind held most | two kinds | all kinds |
| B7 bankrupt, unhappy | treasury 0, one luxury kind gone | two kinds | all luxuries |
| D2 | one level | | |
| D3 losing war | 1 city and 50 % of the army | 2 and 75 % | 3 and 90 % |

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

## Wave 2: the situations built

Every situation names the nations it judges (`focus`); the edit is the only difference of the arms.

- P3 sneak stack (MO3, D10): the nearest pair at peace; the seat opens its borders to the enemy (base);
  the enemy's own units stand 1-3 tiles from the seat's cities at peace; the war is a `Later` change of
  the fourth round. Window 20. `military_added_before_war` reads the seat's army at the war round.
- P2 early rush (MO1, MO8): rounds 30-50 of the same logs (`Situation::start`; any set of logs that
  reaches round 50 serves, e.g. `runs/positions-early`); the nearest pair at war (base); enemy units of
  its own best kinds 1-3 tiles from the seat's capital. Window 25.
- P7 wonder snipe (V1): the city of any nation whose head order is a world wonder nobody holds, with
  the most stock; the nation with the most score but that one completes the wonder in its capital
  (`Wonder`). Window 15. `hammers_burned`: the stock that fell when the lost wonder left the head.
- P5 border grab (MO2, MT10, M1): the nearest pair at peace; the grabber founds a city (`Found`) on free
  land 3-4 tiles from the seat's city nearest to it; one and two citadels (`Improve`, the culture bomb
  takes the tiles around) next to it; the strong level adds the grabber's war. Window 30.
- P1 raid-hunt (MO7, MO6, MT8): the nearest pair at war (base); fast units of the raider in the seat's
  countryside, played by the `Raid` script: capture civilians, pillage improved tiles, never a city or a
  combat unit. Window 20.
- P4 deal cycle (D1, D2): the nearest pair at peace and friends (base); the `SellThenWar` script offers
  its open borders for a share of the victim's treasury (the price halves each round it is
  refused) and declares war the round after the victim paid. Window 20.
- P8 alliance sweep (CS6, A4): the leader by score gets gold (`Treasury`) and the `Sweep` script gifts half
  its treasury every round to the city-state it is nearest to winning. Others judged. Window 40.
- A2 / A3 / A4: the leader by military units / culture / gold; its army copied on its land / a culture or
  gold gift (`Gift` with a yield). Others judged. Windows 40 / 60 / 40.
- A6 weak neighbour over the sea: the pair of nations nearest to each other of those that share no
  landmass; the weak one disarmed as in A5. C2 amphibious target: the same with the near nation's war on
  it in the base. Windows 50 / 40.
- A7 we become weak: the nation of the instance's draw loses a share of its army; itself judged. Window 30.
- B2 surprise war: the nearest pair at peace; the enemy's army 2-4 tiles from the seat's cities, outside
  its land, and the war at once. B5 the same between friends (friendship in the base). Window 15.
- B6 strategic resource lost: the nation with the most strategic resource tiles loses them (`Deplete`).
  B7 bankrupt and unhappy: the nation of the draw, its treasury drained (`Treasury`) and its luxuries
  gone. Window 30.
- D2 defensive pact: the nearest pair at peace and a third nation with a pact with the seat (base);
  the enemy declares war on the seat (the rules drag the partner in). Window 25.
- D3 losing war: the nearest pair at war (base); the seat loses its cities nearest to the enemy (the
  capital last) and most of its army. Window 30.

Not built in wave 2: P6 kill-zone (a scripted defender that holds hills and focus-fires needs the AI kept
off the script's units, size L), P9 sea-hunter, P10 air-nuke, P11 pests (scripted ships, aircraft,
missionaries and spies: each its own script, size M-L), C1 air battle (the air units of both sides and
their bases), B10 one vote from a diplomatic victory (the congress's world-leader vote has no harness
setter). They are the next scripts for the `Seat` hook.

## The suite (a candidate variant against `standard`)

```
ai-stress suite --variant NAME --positions POSITIONS --out OUT [--situations all] [--instances 100] [--jobs 4] [--no-end]
```

plays every situation (all by default) on the variant (every seat played on by it) into `OUT/NAME/`, and on
`standard` into `OUT/standard/` (a rerun plays only the rows that are missing, so the reference is played
once for all candidates), and writes `OUT/suite-NAME.md`: one line per situation and strength with the
verdict of its outcome gates for both, the outcome deltas and the behaviour gates passed.

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
| P3 | outcome | `seat_cities` | delta >= | -0.3 | -0.5 | -0.8 | danger net (on in standard: the fight fixes read it), tactician, commander (off); scripted defence.rs |
| P3 | outcome | `seat_score_share` | delta >= | -0.01 | -0.02 | -0.03 | danger net (on in standard: the fight fixes read it), tactician, commander (off); scripted defence.rs |
| P3 | outcome | `end_seat_win_weight` | delta >= | -0.03 | -0.05 | -0.08 | danger net (on in standard: the fight fixes read it), tactician, commander (off); scripted defence.rs |
| P3 | behaviour | `counter_by_3` | edited >= | 0.8 | 0.8 | 0.8 | danger net (on in standard: the fight fixes read it), tactician, commander (off); scripted defence.rs |
| P3 | behaviour | `military_added_before_war` | delta >= | 1 | 1 | 2 | danger net (on in standard: the fight fixes read it), tactician, commander (off); scripted defence.rs |
| P2 | outcome | `seat_alive` | delta >= | -0.02 | -0.05 | -0.1 | danger net (on in standard: the fight fixes read it), tactician, commander (off); scripted defence.rs |
| P2 | outcome | `seat_cities` | delta >= | -0.2 | -0.4 | -0.6 | danger net (on in standard: the fight fixes read it), tactician, commander (off); scripted defence.rs |
| P2 | outcome | `end_seat_win_weight` | delta >= | -0.03 | -0.05 | -0.08 | danger net (on in standard: the fight fixes read it), tactician, commander (off); scripted defence.rs |
| P2 | behaviour | `capital_lost` | edited <= | 0.1 | 0.2 | 0.3 | danger net (on in standard: the fight fixes read it), tactician, commander (off); scripted defence.rs |
| P2 | behaviour | `counter_by_3` | edited >= | 0.8 | 0.8 | 0.8 | danger net (on in standard: the fight fixes read it), tactician, commander (off); scripted defence.rs |
| P7 | outcome | `seat_score_share` | delta >= | -0.03 | -0.03 | -0.03 | none: scripted wonder_race.rs; strategist production head |
| P7 | outcome | `end_seat_win_weight` | delta >= | -0.03 | -0.03 | -0.03 | none: scripted wonder_race.rs; strategist production head |
| P7 | behaviour | `hammers_burned` | edited <= | 0 | 0 | 0 | none: scripted wonder_race.rs; strategist production head |
| P7 | behaviour | `lost_head_rounds` | edited <= | 1 | 1 | 1 | none: scripted wonder_race.rs; strategist production head |
| P5 | outcome | `seat_score_share` | delta >= | -0.01 | -0.02 | -0.03 | site net (off), danger net (on: the fight fixes); scripted defence.rs |
| P5 | outcome | `seat_cities` | delta >= | -0.1 | -0.2 | -0.4 | site net (off), danger net (on: the fight fixes); scripted defence.rs |
| P5 | outcome | `end_seat_win_weight` | delta >= | -0.02 | -0.03 | -0.05 | site net (off), danger net (on: the fight fixes); scripted defence.rs |
| P5 | behaviour | `forward_city_held` | edited <= | 0.8 | 0.7 | 0.6 | site net (off), danger net (on: the fight fixes); scripted defence.rs |
| P5 | behaviour | `war_by_seat` | edited >= | 0.1 | 0.2 | 0 | site net (off), danger net (on: the fight fixes); scripted defence.rs |
| P1 | outcome | `seat_score_share` | delta >= | -0.005 | -0.01 | -0.02 | none: scripted (worker flight, settler escort); tactician, commander (off) |
| P1 | outcome | `seat_cities` | delta >= | -0.1 | -0.2 | -0.3 | none: scripted (worker flight, settler escort); tactician, commander (off) |
| P1 | outcome | `end_seat_win_weight` | delta >= | -0.02 | -0.03 | -0.05 | none: scripted (worker flight, settler escort); tactician, commander (off) |
| P1 | behaviour | `raiders_dead` | edited >= | 0.5 | 0.5 | 0.5 | none: scripted (worker flight, settler escort); tactician, commander (off) |
| P1 | behaviour | `pillaged_end` | delta <= | 1 | 2 | 3 | none: scripted (worker flight, settler escort); tactician, commander (off) |
| P1 | behaviour | `civilians_end` | delta >= | -0.5 | -1 | -1.5 | none: scripted (worker flight, settler escort); tactician, commander (off) |
| P4 | outcome | `seat_score_share` | delta >= | -0.01 | -0.015 | -0.02 | none: scripted (peace.rs Negotiation::value, diplomacy.rs) |
| P4 | outcome | `seat_treasury` | delta >= | -50 | -100 | -150 | none: scripted (peace.rs Negotiation::value, diplomacy.rs) |
| P4 | outcome | `end_seat_win_weight` | delta >= | -0.02 | -0.03 | -0.05 | none: scripted (peace.rs Negotiation::value, diplomacy.rs) |
| P4 | behaviour | `script_sold_then_war` | edited <= | 0.3 | 0.2 | 0.1 | none: scripted (peace.rs Negotiation::value, diplomacy.rs) |
| P8 | outcome | `seat_score_share` | delta >= | -0.01 | -0.02 | -0.03 | none: scripted (city_states.rs, victory_watch.rs); strategist has no diplomacy outputs |
| P8 | outcome | `leader_won` | edited <= | 0.05 | 0.1 | 0.15 | none: scripted (city_states.rs, victory_watch.rs); strategist has no diplomacy outputs |
| P8 | outcome | `end_seat_win_weight` | delta >= | -0.05 | -0.1 | -0.15 | none: scripted (city_states.rs, victory_watch.rs); strategist has no diplomacy outputs |
| P8 | behaviour | `gifts_others` | delta >= | 1 | 2 | 3 | none: scripted (city_states.rs, victory_watch.rs); strategist has no diplomacy outputs |
| P8 | behaviour | `leader_allies` | delta <= | 2 | 3 | 4 | none: scripted (city_states.rs, victory_watch.rs); strategist has no diplomacy outputs |
| A2 | outcome | `seat_cities` | delta >= | -0.1 | -0.2 | -0.3 | strategist |
| A2 | outcome | `seat_score_share` | delta >= | -0.01 | -0.02 | -0.03 | strategist |
| A2 | outcome | `end_seat_win_weight` | delta >= | -0.05 | -0.1 | -0.15 | strategist |
| A2 | behaviour | `seat_military` | delta >= | 0.5 | 1 | 2 | strategist |
| A2 | behaviour | `wars_on_leader` | delta <= | 0 | 0 | 0 | strategist |
| A3 | outcome | `seat_score_share` | delta >= | -0.01 | -0.02 | -0.04 | strategist |
| A3 | outcome | `leader_won` | edited <= | 0.05 | 0.1 | 0.2 | strategist |
| A3 | outcome | `end_seat_win_weight` | delta >= | -0.05 | -0.1 | -0.15 | strategist |
| A3 | behaviour | `wars_on_leader` | delta >= | 0.1 | 0.2 | 0.3 | strategist |
| A4 | outcome | `seat_score_share` | delta >= | -0.01 | -0.02 | -0.04 | none: scripted (city_states.rs, victory_watch.rs); strategist has no diplomacy outputs |
| A4 | outcome | `leader_won` | edited <= | 0.05 | 0.1 | 0.2 | none: scripted (city_states.rs, victory_watch.rs); strategist has no diplomacy outputs |
| A4 | outcome | `end_seat_win_weight` | delta >= | -0.05 | -0.1 | -0.15 | none: scripted (city_states.rs, victory_watch.rs); strategist has no diplomacy outputs |
| A4 | behaviour | `gifts_others` | delta >= | 0.5 | 1 | 1 | none: scripted (city_states.rs, victory_watch.rs); strategist has no diplomacy outputs |
| A4 | behaviour | `leader_allies` | delta <= | 1 | 2 | 3 | none: scripted (city_states.rs, victory_watch.rs); strategist has no diplomacy outputs |
| A6 | outcome | `seat_cities` | delta >= | 0.1 | 0.2 | 0.3 | commander; fleet none (fleet.rs) |
| A6 | outcome | `end_seat_win_weight` | delta >= | 0 | 0 | 0 | commander; fleet none (fleet.rs) |
| A6 | behaviour | `weak_captured` | delta >= | 0.1 | 0.15 | 0.2 | commander; fleet none (fleet.rs) |
| A6 | behaviour | `nobody_attacked` | edited <= | 0.7 | 0.6 | 0.5 | commander; fleet none (fleet.rs) |
| C2 | outcome | `seat_cities` | delta >= | 0.2 | 0.3 | 0.4 | commander; fleet none (fleet.rs) |
| C2 | outcome | `end_seat_win_weight` | delta >= | 0 | 0 | 0 | commander; fleet none (fleet.rs) |
| C2 | behaviour | `weak_captured` | delta >= | 0.15 | 0.25 | 0.3 | commander; fleet none (fleet.rs) |
| A7 | outcome | `seat_cities` | delta >= | -0.3 | -0.5 | -0.8 | strategist |
| A7 | outcome | `seat_score_share` | delta >= | -0.01 | -0.02 | -0.03 | strategist |
| A7 | outcome | `end_seat_win_weight` | delta >= | -0.03 | -0.05 | -0.08 | strategist |
| A7 | behaviour | `wars_declared` | delta <= | 0 | 0 | 0 | strategist |
| A7 | behaviour | `seat_military` | delta >= | -2 | -3 | -4 | strategist |
| B2 | outcome | `seat_cities` | delta >= | -0.3 | -0.5 | -0.8 | danger net (on in standard: the fight fixes read it), tactician, commander (off); scripted defence.rs |
| B2 | outcome | `seat_score_share` | delta >= | -0.01 | -0.02 | -0.03 | danger net (on in standard: the fight fixes read it), tactician, commander (off); scripted defence.rs |
| B2 | outcome | `end_seat_win_weight` | delta >= | -0.03 | -0.05 | -0.08 | danger net (on in standard: the fight fixes read it), tactician, commander (off); scripted defence.rs |
| B2 | behaviour | `counter_by_3` | edited >= | 0.8 | 0.8 | 0.8 | danger net (on in standard: the fight fixes read it), tactician, commander (off); scripted defence.rs |
| B2 | behaviour | `purchases` | delta >= | 0.5 | 1 | 1 | danger net (on in standard: the fight fixes read it), tactician, commander (off); scripted defence.rs |
| B5 | outcome | `seat_cities` | delta >= | -0.3 | -0.5 | -0.8 | danger net (on in standard: the fight fixes read it), tactician, commander (off); scripted defence.rs |
| B5 | outcome | `seat_score_share` | delta >= | -0.01 | -0.02 | -0.03 | danger net (on in standard: the fight fixes read it), tactician, commander (off); scripted defence.rs |
| B5 | outcome | `end_seat_win_weight` | delta >= | -0.03 | -0.05 | -0.08 | danger net (on in standard: the fight fixes read it), tactician, commander (off); scripted defence.rs |
| B5 | behaviour | `counter_by_3` | edited >= | 0.8 | 0.8 | 0.8 | danger net (on in standard: the fight fixes read it), tactician, commander (off); scripted defence.rs |
| B5 | behaviour | `purchases` | delta >= | 0.5 | 1 | 1 | danger net (on in standard: the fight fixes read it), tactician, commander (off); scripted defence.rs |
| B6 | outcome | `seat_score_share` | delta >= | -0.01 | -0.015 | -0.02 | strategist; supply none |
| B6 | outcome | `seat_cities` | delta >= | -0.2 | -0.3 | -0.4 | strategist; supply none |
| B6 | outcome | `end_seat_win_weight` | delta >= | -0.02 | -0.03 | -0.05 | strategist; supply none |
| B6 | behaviour | `seat_military` | delta >= | -1 | -2 | -3 | strategist; supply none |
| B7 | outcome | `seat_score_share` | delta >= | -0.01 | -0.02 | -0.03 | strategist |
| B7 | outcome | `seat_cities` | delta >= | -0.2 | -0.3 | -0.4 | strategist |
| B7 | outcome | `end_seat_win_weight` | delta >= | -0.02 | -0.03 | -0.05 | strategist |
| B7 | behaviour | `unhappy_rounds` | delta <= | 5 | 8 | 10 | strategist |
| B7 | behaviour | `broke_rounds` | delta <= | 3 | 5 | 5 | strategist |
| D2 | outcome | `seat_cities` | delta >= | -0.3 | -0.3 | -0.3 | none (no net for allied action) |
| D2 | outcome | `end_seat_win_weight` | delta >= | -0.03 | -0.03 | -0.03 | none (no net for allied action) |
| D2 | behaviour | `ally_at_war` | edited >= | 0.9 | 0.9 | 0.9 | none (the rules' pact) |
| D2 | behaviour | `ally_struck` | edited >= | 0.5 | 0.5 | 0.5 | none (no net for allied action) |
| D2 | behaviour | `ally_left` | edited <= | 0.3 | 0.3 | 0.3 | none (no net for allied action) |
| D3 | outcome | `seat_alive` | delta >= | -0.05 | -0.1 | -0.15 | none: scripted peace.rs |
| D3 | outcome | `seat_cities` | delta >= | -1.3 | -2.5 | -3.6 | none: scripted peace.rs |
| D3 | outcome | `end_seat_win_weight` | delta >= | -0.05 | -0.08 | -0.1 | none: scripted peace.rs |
| D3 | behaviour | `peace` | edited >= | 0.4 | 0.5 | 0.6 | none: scripted peace.rs |
| D3 | behaviour | `peace_round` | edited <= | 15 | 12 | 10 | none: scripted peace.rs |

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
`discover`, `project`; wave 2: `later`, `treasury`, `improve`, `wonder`, `found`, `accord`, `deplete`) plus
an optional per-round `Gift` (science, culture or gold) and an optional `Script`. The situations only choose
the changes; the saved scene, a random sampler and the scripted adversaries all use the same primitives.

- `later {after, then}`: `then` made at the start of round `after` of the window (the play loop).
- `treasury {player, gold}`: gold in, or out down to an empty treasury (`gold` < 0).
- `improve {player, tile, improvement}`: an improvement; a citadel is raised with its culture bomb
  (`debug_raise_citadel`: what a great general's citadel does once the general is consumed).
- `wonder {player, building}`: a world wonder completed in the player's capital; everyone else's order
  of it is lost from then on.
- `found {player, tile}`, `accord {giver, receiver, accord}` (friendship, defensive pact, open borders, as an
  accepted deal makes them), `deplete {tiles}` (the resources gone).

## The scripted adversary (`Seat`)

A scripted adversary plays one seat's trick: `seat::Seat` is one trait (`player`, `play`, `counts`) with one
implementation per adversary group (`Raid` P1, `SellThenWar` P4, `Sweep` P8). `play::play_arm` plays a round
step by step and, at the start of the seat's turn (the seat active, its AI not yet started), hands the world
to the script, which plays its part through `GameState::execute`, the boundary a human's commands go through.
Then the seat's AI plays its turn as usual: the units the script spent stay spent, the rest of the seat (its
cities, its other units) is the AI's. The script is data in the saved scene (`script`); what it counted goes
to the row as `script_*`.

## Results

See the end of this file (filled after the run).

## Results of wave 2 (standard, window only)

Run of 22 Nov 2026: `ai-stress suite --variant standard --positions stress/runs/positions --situations` (the 19
of wave 2) `--instances 100 --no-end`, 42 min on 2-6 threads. 100 positions (pace, small maps, rounds
100-110; P2 rounds 30-50); a situation that cannot be built on a position is skipped with its reason
(`unbuilt.jsonl`: 19 archipelago positions have no two nations on one landmass, 31 have no pair over the
sea for A6/C2, D3 needs more cities than it takes). The end-of-game gates have no data in this run (`--no-end`).

| situation | strength | n | outcome | outcome delta | behaviour gates passed | owning net |
|---|---|---|---|---|---|---|
| P3 | mild | 78 | pass | cities -0.128; score_share -0.005 | 1/2: military_added_before_war 0.36 | danger net (on in standard: the fight fixes read it), tactician, commander (off); scripted defence.rs |
| P3 | medium | 78 | pass | cities -0.205; score_share -0.008 | 1/2: military_added_before_war 0.71 | danger net (on in standard: the fight fixes read it), tactician, commander (off); scripted defence.rs |
| P3 | strong | 78 | pass | cities -0.269; score_share -0.010 | 1/2: military_added_before_war 1.05 | danger net (on in standard: the fight fixes read it), tactician, commander (off); scripted defence.rs |
| P2 | mild | 80 | pass | alive +0.000; cities -0.013 | 2/2 | danger net (on in standard: the fight fixes read it), tactician, commander (off); scripted defence.rs |
| P2 | medium | 80 | pass | alive +0.000; cities -0.050 | 2/2 | danger net (on in standard: the fight fixes read it), tactician, commander (off); scripted defence.rs |
| P2 | strong | 80 | pass | alive +0.000; cities -0.013 | 2/2 | danger net (on in standard: the fight fixes read it), tactician, commander (off); scripted defence.rs |
| P7 | one | 92 | pass | score_share -0.012 | 2/2 | none: scripted wonder_race.rs; strategist production head |
| P5 | mild | 77 | pass | score_share -0.002; cities -0.078 | 0/2: forward_city_held 0.95, war_by_seat 0.09 | site net (off), danger net (on: the fight fixes); scripted defence.rs |
| P5 | medium | 77 | FAIL (seat_cities) | score_share -0.005; cities -0.208 | 0/2: forward_city_held 0.99, war_by_seat 0.09 | site net (off), danger net (on: the fight fixes); scripted defence.rs |
| P5 | strong | 77 | pass | score_share -0.007; cities -0.325 | 1/2: forward_city_held 1.00 | site net (off), danger net (on: the fight fixes); scripted defence.rs |
| P1 | mild | 81 | pass | score_share -0.001; cities +0.025 | 2/3: pillaged_end 1.23 | none: scripted (worker flight, settler escort); tactician, commander (off) |
| P1 | medium | 81 | pass | score_share -0.004; cities +0.111 | 3/3 | none: scripted (worker flight, settler escort); tactician, commander (off) |
| P1 | strong | 81 | pass | score_share -0.006; cities +0.049 | 3/3 | none: scripted (worker flight, settler escort); tactician, commander (off) |
| P4 | mild | 78 | pass | score_share -0.002; treasury -8.885 | 0/1: script_sold_then_war 0.40 | none: scripted (peace.rs Negotiation::value, diplomacy.rs) |
| P4 | medium | 78 | pass | score_share -0.002; treasury -12.205 | 0/1: script_sold_then_war 0.44 | none: scripted (peace.rs Negotiation::value, diplomacy.rs) |
| P4 | strong | 78 | pass | score_share -0.003; treasury -11.231 | 0/1: script_sold_then_war 0.51 | none: scripted (peace.rs Negotiation::value, diplomacy.rs) |
| P8 | mild | 100 | pass | score_share -0.000; leader_won +0.000 | 1/2: gifts_others -0.16 | none: scripted (city_states.rs, victory_watch.rs); strategist has no diplomacy outputs |
| P8 | medium | 100 | pass | score_share +0.000; leader_won +0.000 | 1/2: gifts_others -0.26 | none: scripted (city_states.rs, victory_watch.rs); strategist has no diplomacy outputs |
| P8 | strong | 100 | pass | score_share -0.001; leader_won +0.000 | 1/2: gifts_others -0.33 | none: scripted (city_states.rs, victory_watch.rs); strategist has no diplomacy outputs |
| A2 | mild | 100 | pass | cities -0.028; score_share -0.001 | 1/2: seat_military 0.28 | strategist |
| A2 | medium | 100 | pass | cities -0.010; score_share +0.000 | 1/2: seat_military 0.18 | strategist |
| A2 | strong | 100 | pass | cities -0.006; score_share +0.004 | 1/2: seat_military -0.04 | strategist |
| A3 | mild | 100 | pass | score_share -0.001; leader_won +0.000 | 0/1: wars_on_leader -0.01 | strategist |
| A3 | medium | 100 | pass | score_share -0.001; leader_won +0.000 | 0/1: wars_on_leader -0.02 | strategist |
| A3 | strong | 100 | pass | score_share -0.003; leader_won +0.000 | 0/1: wars_on_leader -0.01 | strategist |
| A4 | mild | 100 | pass | score_share -0.002; leader_won +0.000 | 1/2: gifts_others -0.21 | none: scripted (city_states.rs, victory_watch.rs); strategist has no diplomacy outputs |
| A4 | medium | 100 | pass | score_share -0.002; leader_won +0.000 | 1/2: gifts_others -0.26 | none: scripted (city_states.rs, victory_watch.rs); strategist has no diplomacy outputs |
| A4 | strong | 100 | pass | score_share -0.003; leader_won +0.000 | 1/2: gifts_others -0.23 | none: scripted (city_states.rs, victory_watch.rs); strategist has no diplomacy outputs |
| A6 | mild | 69 | FAIL (seat_cities) | cities +0.043 | 0/2: weak_captured 0.01, nobody_attacked 0.96 | commander; fleet none (fleet.rs) |
| A6 | medium | 69 | FAIL (seat_cities) | cities +0.087 | 0/2: weak_captured 0.01, nobody_attacked 0.97 | commander; fleet none (fleet.rs) |
| A6 | strong | 69 | FAIL (seat_cities) | cities +0.058 | 0/2: weak_captured 0.01, nobody_attacked 0.96 | commander; fleet none (fleet.rs) |
| A7 | mild | 100 | pass | cities -0.050; score_share +0.000 | 1/2: seat_military -2.30 | strategist |
| A7 | medium | 100 | pass | cities -0.110; score_share -0.001 | 1/2: wars_declared 0.01 | strategist |
| A7 | strong | 100 | pass | cities -0.140; score_share -0.004 | 2/2 | strategist |
| B2 | mild | 77 | pass | cities -0.169; score_share -0.005 | 1/2: purchases -2.65 | danger net (on in standard: the fight fixes read it), tactician, commander (off); scripted defence.rs |
| B2 | medium | 77 | pass | cities -0.208; score_share -0.008 | 1/2: purchases -2.66 | danger net (on in standard: the fight fixes read it), tactician, commander (off); scripted defence.rs |
| B2 | strong | 77 | pass | cities -0.195; score_share -0.009 | 1/2: purchases -2.83 | danger net (on in standard: the fight fixes read it), tactician, commander (off); scripted defence.rs |
| B5 | mild | 77 | pass | cities -0.169; score_share -0.005 | 1/2: purchases -2.65 | danger net (on in standard: the fight fixes read it), tactician, commander (off); scripted defence.rs |
| B5 | medium | 77 | pass | cities -0.208; score_share -0.008 | 1/2: purchases -2.66 | danger net (on in standard: the fight fixes read it), tactician, commander (off); scripted defence.rs |
| B5 | strong | 77 | pass | cities -0.195; score_share -0.009 | 1/2: purchases -2.83 | danger net (on in standard: the fight fixes read it), tactician, commander (off); scripted defence.rs |
| B6 | mild | 100 | pass | score_share -0.001; cities +0.020 | 1/1 | strategist; supply none |
| B6 | medium | 100 | pass | score_share -0.001; cities +0.090 | 1/1 | strategist; supply none |
| B6 | strong | 100 | pass | score_share -0.002; cities -0.020 | 1/1 | strategist; supply none |
| B7 | mild | 100 | pass | score_share -0.001; cities -0.090 | 2/2 | strategist |
| B7 | medium | 100 | pass | score_share -0.003; cities -0.060 | 2/2 | strategist |
| B7 | strong | 100 | pass | score_share -0.008; cities -0.190 | 1/2: unhappy_rounds 13.74 | strategist |
| C2 | mild | 69 | FAIL (seat_cities) | cities -0.043 | 0/1: weak_captured -0.01 | commander; fleet none (fleet.rs) |
| C2 | medium | 69 | FAIL (seat_cities) | cities +0.000 | 0/1: weak_captured 0.00 | commander; fleet none (fleet.rs) |
| C2 | strong | 69 | FAIL (seat_cities) | cities -0.014 | 0/1: weak_captured -0.01 | commander; fleet none (fleet.rs) |
| D2 | one | 78 | pass | cities -0.256 | 2/3: ally_struck 0.04 | none (no net for allied action) |
| D3 | mild | 42 | pass | alive +0.000; cities -0.595 | 2/2 | none: scripted peace.rs |
| D3 | medium | 42 | pass | alive +0.000; cities -1.190 | 1/2: peace_round 14.95 | none: scripted peace.rs |
| D3 | strong | 42 | pass | alive +0.000; cities -1.976 | 1/2: peace_round 15.38 | none: scripted peace.rs |

standard: 46 of 53 (situation, strength) pass their outcome gates.

What `standard` does wrong, in plain words (the outcome FAILs first, then the behaviour gates):

- A6, C2 (all three strengths): a neighbour over the sea with no army is never invaded (1 % of positions
  take a city, 96 % do not even declare war), even when the war is already on (C2). Owner: commander;
  the naval invasion is scripted `fleet.rs` with no net.
- P5 medium: a forward city with a citadel on the seat's border costs it 0.21 cities in 30 rounds, and the
  AI never takes the forward city back (it stands in 95-100 % of positions) and declares war on the grabber
  in 9 %. Owner: site net (off in `standard`) and danger net (on: `standard`'s fight fixes read it,
  variant.rs `FightFix::all(Source::Net)`); scripted `defence.rs`.
- P3: a foreign stack standing inside the borders at peace for three rounds is not answered: the seat adds
  0.4-1.0 military units before the war (gate 1-2). B2/B5: under a surprise war the seat buys 2.7 fewer
  items than in the control. Owner: danger net (on in `standard` through the fight fixes; only the separate
  `danger_net()` switch is off), tactician and commander (off).
- B5 equals B2 row for row, and it is not a harness fault (test
  `b5_friendship_holds_in_the_control_and_the_war_ends_it`: the friendship is in the control's world and
  hash, and the war removes it). Two facts make it so: the rules' war ends every tie and keeps no memory of
  the betrayal (`systems/diplomacy` `end_ties`), so the edited arms of B5 and B2 are the same world; and
  `standard` plays the control the same with or without the friendship for 15 rounds. Owner: none (rules:
  no opinion memory; AI: friendship read by deals only).
- P4: a friend's sale of open borders is paid for in 40-51 % of positions and the war follows. Owner:
  none, scripted `peace.rs` / `diplomacy.rs` (no counterparty risk).
- P8, A4: a rival buying the city-states with gold is not answered: the others gift less than in the
  control. A3: nobody declares war on a culture runaway. A2: the others barely arm against a doubled army.
  Owner: strategist (no diplomacy outputs), scripted `city_states.rs`.
- D2: the pact partner is dragged into the war by the rules but strikes the aggressor in 4 % of positions.
  Owner: none (no net for allied action).
- D3 medium/strong: peace comes after about 15 rounds (gates 12, 10). B7 strong: 13.7 unhappy rounds of
  30. A7 mild: the army stays 2.3 units below the control. P1 mild: 1.2 more pillaged tiles unrepaired.

### The failed situations played to the end of the game

`ai-stress suite --variant standard --situations a6,c2,p5 --instances 50` (to the end, `runs/exam-end` of the
wave-2 clone): 36 instances of A6 and C2, 38 of P5 (the rest have no pair over the sea or no free tile).

| situation | strength | `seat_cities` delta (gate) | `end_seat_win_weight` edited / control / delta (gate) | verdict |
|---|---|---|---|---|
| A6 | mild | +0.11 (>= 0.1) | 0.062 / 0.069 / -0.007 (>= 0) | FAIL (end win weight) |
| A6 | medium | +0.14 (>= 0.2) | 0.069 / 0.069 / 0.000 | FAIL (cities) |
| A6 | strong | +0.06 (>= 0.3) | 0.076 / 0.069 / +0.007 | FAIL (cities) |
| C2 | mild | -0.06 (>= 0.2) | 0.049 / 0.049 / 0.000 | FAIL (cities) |
| C2 | medium | 0.00 (>= 0.3) | 0.049 / 0.049 / 0.000 | FAIL (cities) |
| C2 | strong | -0.03 (>= 0.4) | 0.049 / 0.049 / 0.000 | FAIL (cities) |
| P5 | mild | -0.18 (>= -0.1) | 0.072 / 0.059 / +0.013 (>= -0.02) | FAIL (cities) |
| P5 | medium | -0.32 (>= -0.2) | 0.099 / 0.059 / +0.039 (>= -0.03) | FAIL (cities) |
| P5 | strong | -0.40 (>= -0.4) | 0.020 / 0.059 / -0.039 (>= -0.05) | pass |

A disarmed nation over the sea is left alone to the end of the game: the real victories do not move (C2 not
at all, A6 within one game in 36). The border grab costs cities in the window but not real victories, except
at the strong level (citadels and war): -0.04, inside its gate.
