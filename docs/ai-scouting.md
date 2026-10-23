# AI scouting: scout targets as a single assignment

Code is in `classic/crates/classic-rules/src/ai/scouting.rs`; the step toward the target is in `planner_units.rs` (`stride_along`); a scout is
ordered by `planner_cities.rs` (`recon_orders`). Measured with the probe `integration/crates/aurelia-qa/tests/ai_scouting.rs`
(`cargo test --release -p aurelia-qa --test ai_scouting -- --ignored --nocapture`, variables in the file header).

## What was wrong (commit ca330d31)

1. Each scout chose its target itself, greedily: the nearest tile on the edge of the unknown. Other scouts' targets were taken into account
   only within a radius of 3 tiles of their targets, and the positions of other scouts not at all (`planner_units.rs:601`, `taken`;
   `:540`, `reserved_tiles`). Two scouts from the same city take neighboring targets: in the probe traces the pair moves
   together on 7% of turns (13 of 178), but a pair is rare in the first place.
2. There are few scouts and they come late. The second one only while the army has ≥ 4 soldiers and before round 60
   (`planner_units.rs:511`); a scout unit is ordered by a city only when nobody is exploring and unknown tiles
   lie within 6 tiles of the city (`planner_cities.rs:75, 334, 392, 419`); a warrior counts as a scout.
3. A target occupied by a foreign unit (a barbarian camp) stayed with the scout: `Waiting | Blocked` did not clear
   the task (`planner_units.rs:617`); in a trace a scout stood like that for 75 rounds.
4. Known Ancient Ruins were not targets. Only whoever happened to pass by took them.

## How it works now

**Target** (`scouting::scout_target`). One Dijkstra over the known map (`planner_map::explore`, the same node budget
as for any search: `path_nodes` = 3000) gathers no more than `min(budget.candidates, 64)` = 48 candidates:
tiles on the edge of the unknown and known ruins. The score = 10 for each unknown tile within the sight radius
(+ the pull of the leader's capital search) − 4 per turn of path − crowding; ruins get 120. Crowding is a sum over no more than
6 other scouts: 12 for each tile on which the candidate is closer than 8 to their target, and 10 for each one on which it is
closer than 5 to their position. A tile that another scout is already heading for is not taken. The candidates lie in an array on the stack,
are sorted (the tile with the lower index on equal scores) and the first safe one wins: not closer than 2 tiles to
a known camp or a hostile soldier, and not in the direction of a hostile soldier the scout sees within 3
tiles (`ALARM_RADIUS`; the same check clears an already chosen target).

**How many scouts** (`scouting::wanted`, `wants_recon`). From round 30 one scout unit (a scout from the round-12 queue took the empires' first workers and luxury: by round 60, 0.87 workers and 0.42 luxury types per seat against 0.94 and 0.64 in the control; leaders searching for a capital build it immediately, as before); a second scout
(a soldier, not a new unit: a scout with nothing to look at would just stand in the garrison) from round 12 to 99,
while less than 40% of the map is open; they all leave when 90% is open (the edge of a small map is its first third,
rivals live in the middle). A city with an empty queue builds the needed scout immediately (`recon_orders`).

**Step.** A scout goes by a `MarchTo` order to the farthest tile of its path that its movement points allow:
one order and one local observation per turn instead of three; the core interrupts the move when an enemy comes into view. After
an order is rejected (the tile ahead turned out to be foreign closed land) it moves tile by tile (`TaskRecord::failures`).

**Cost bounds.** Per decision: one search ≤ 3000 nodes, ≤ 48 candidates, ≤ 6 rivals, fixed-size arrays;
neither the number of map tiles, nor the number of scouts, nor the number of open tiles affects it.
The exception is the brief's `known_percent` (a walk over the map once per turn, which existed before). There are no new fields in the save: the target and
the task live in the existing `UnitTask::Scout`.

## Before and after

The control is the current `origin/main` (badc0eda: policies, science and economy), without ruins on the map; both sides play
at the same time under the same load.

Probe: 40 games of the `standard` set (`AURELIA_PROBE_SEEDS=1..=4`, 10 maps), 200 seats, 100 rounds.

| | main | with scouting |
|---|---:|---:|
| map revealed at round 25 / 50 / 100, % | 6.8 / 9.8 / 15.0 | 7.0 / 10.4 / 16.7 |
| nations / city-states met by round 50 | 0.96 / 3.37 | 0.93 / 3.27 |
| ruins taken by round 50 / 100, per nation | 0 / 0 (no ruins on the map) | 1.26 / 1.77 |
| natural wonders by round 100 | 0.72 | 0.85 |
| scouts lost by round 100 | 0.54 | 0.54 |
| decisions per game / searches / nodes (100 rounds) | 13,058 / 4,719 / 205,871 | 13,757 / 5,117 / 223,348 |

Gameplay: `ai-tournament --suite standard --solo standard`, seed offsets 0…4 (200 games) and `--suite duel`, offsets 0…7
(320 games).

| | standard main → scouting | duel main → scouting |
|---|---:|---:|
| points, round 100 | 242.8 → 248.7 (+2.4%) | −0.6% (over 320 games) |
| points at the end | 676.6 → 680.7 (+0.6%) | −2.0% (−2.3%, −4.2%, −0.6% over three pairs) |
| cities, round 100 / end | 4.84 → 4.92 / 9.44 → 9.48 | −1% / −1% |
| city two, round | 32.6 → 30.8 | 35.5 → 36.0 |
| victories | all by time | all by time |
| round, ms: p50 / p95 | 7.98 → 8.22 (+3.0%) / 37.3 → 36.6 (−1.9%) | +0.0…2.2% / +0.8…7.9% |

Conclusion: the gain is small. The map is revealed 11% more (15.0 → 16.7%), ruins and wonders are taken, in standard the points
at round 100 are +2.4%. In duel the points at round 100 and at the end are no better than main's (the spread between pairs is −0.6…−4.2%,
over 320 games about −2%, roughly 1σ): there is no proven gain in play, and no proven loss either.

`ai_rounds 5 8 2` on a quiet machine (contract: 250 ms and 1 s), main → scouting: `representative` median 11.6 → 11.9 ms
(+2.6%), p95 12.3 → 12.4; `stress` median 80.5 → 80.8 ms, p95 84.1 → 86.4 (+2.7%).

The cost of a scout step by phase (CPU sampling, 30 s of games): `scout_target` ≈ 4% of turn time; within it
Dijkstra is 97%, the danger check 1%, sorting and the rest less than 1%. The step itself is one `MarchTo`, one
`find_path` and one local observation per turn. Two scout units cost +15% of AI turn time against +7% for one (each unit standing in the garrison is also a
decision per turn); that is why the second scout is a soldier.

## What remains

* Scout losses to barbarians did not fall: 0.54 per nation by round 100 for both main and scouting. Danger radii of 2–5 hardly
  change losses.
* Islands: scouting by sea begins when units can board ships (rounds 50–67), the research chain
  `planner_research.rs` is untouched — an archipelago reveals 12.2% of the map by round 100 against 9.7%.
* A lone scout follows the "farther from home" gradient (`capital_search::pull`, 10 per tile) and on equal scores takes the
  tile with the lower index, that is, it goes along the edge of the map and leaves the middle unexplored: on a 30×20 map
  a rival 8 tiles from the capital was found only by round 78. The second scout from round 12 covers this
  (tests `ai_conquest_classic`, `ai_acceptance`); a radial sweep instead of the gradient was not done.
* A scout unit costs 25 hammers at the start of the game, so it goes from round 30: from round 12 it gave, over 16
  games of `happiness_pace`, 8% fewer workers and a third fewer luxury types by round 60. What is weaker in
  the tests than it was:
  * `happiness_pace` now plays 16 games (4 seeds × 4 maps) instead of 4; the thresholds of 0.95 workers, 0.45 luxury types and
    +5.5 happiness per seat are left. Measured over 96 seats: with scouting 1.55 / 1.05 / +7.1, for main 1.45 / 1.13 / +7.9.
    The measured values are noticeably above the thresholds, so the test catches the failure of the first worker, but not a loss of 10%
    (scouting costs 7% of luxury types and 10% of happiness relative to main, and the thresholds miss it).
  * `integration/crates/aurelia-qa/tests/ai_scouting.rs`: the threshold for the share revealed on a small pangaea by round 100 is 0.24 instead of 0.30 (with a
    unit from round 12 it was 0.30, from round 30 — 0.25, greedy choice — 0.21). The test now proves only that
    scouting is not worse than greedy by three points, not that a third of the map is revealed.
  * `ai_acceptance::an_operation_survives_save_and_load`: a limit of 300 rounds instead of 250. The first operation in a game of
    four AIs is now built at round 240 instead of 126 (the war waits for a smaller neighbor and a known city); the test
    needs any marching army, not a date.
  * `ai_science::the_cities_order_only_what_the_rules_accept`: the control variant `Direct` is played on seed
    0x1_7003 instead of 0x1_7011: on the old seed it stopped running into rejections (a control without rejections proves nothing).
* `ai_production_respects_strategic_resources` passes, but is fragile: `ai/diplomacy.rs::luxury_swaps` gives away "surplus"
  luxury counting imported copies (`tradeable` includes them), while the import is shorter than its own deal; when it
  ends, the player is one copy short (this happened with citrus in rounds 140–150 at seed 0x1_7011 on an intermediate
  build). Not changed: trade is outside scouting.
* A side fix: `ai/barbarians.rs::reachable` searched for a path using the common land profile, and barbarian triremes were given
  orders to go onto land (579 `Impassable` rejections in 200 rounds in `classic_ai_minors`); now the path is searched using the unit's own
  profile. There is no separate test: `classic_ai_minors` catches it ("rejections stay rare"); building such a case around a single
  ship did not work out.
* L&L (`lal-rules/src/ai/frontier.rs`): one starting scout, the nearest border tile plus ruins; the same
  greed, but there is no second scout, so "moving together" is impossible. Not changed.
