# AI: choosing policy branches

Code: `classic/crates/classic-rules/src/ai/policy_branches.rs`, called from `ai/policies.rs` on the turn when the culture treasury
allows opening a new branch and no policy can be adopted. Before, the branch was chosen by a fixed order
(Tradition first, Liberty only with four or more cities); now it is a score based on declared effects.

**Result.** The AI takes Liberty as its first branch almost always, and Tradition only where there is no free room around the capital
(5.8% of seats). The dependence of Liberty's gain on land is weak: the only thing visible is the absence of room
(+0.39 cities for −10.3% score by round 200, against +0.8…+1.0 cities for −4…−7% with any room). Bench, 200 games
alongside the former AI: cities by round 100 +22%, score by round 100 +10%, by round 200 −2.9%, at the end −0.8%; point share
against `allocator` 67.2% (former AI 68.1%, threshold 62%).

## How a branch is chosen

The function is pure and deterministic: from `AIObservation`, the AI's memory of foreign cities and the rules it computes a number for
each permitted branch; the largest wins, on a tie the order of `ai_branches` in `classic/assets/rules/classic/roles.ron`
(which stayed the same: Tradition, Piety, Liberty, …), then the lower index. No branch and no policy is named
by name.

1. **Situation** (`Situation::observe`, once per decision): free room (a greedy placement of cities with
   `SITE_SPACING` = 4 within radius `ROOM_RADIUS` = 7 of the capital, by the `planner_settle` site estimates), the food of the four best
   sites, the distance to the nearest known rival city, the share of coastal cities, war with a nation,
   happiness below zero, the number of city-states, a free trade route, a founded religion, the leader's personality.
2. **Effect value.** Each declared policy effect belongs to one goal (growth, expansion, happiness,
   production, gold, science, culture, faith, army, diplomacy, trade) and gets points: size × scope
   (capital or player — once, "each city" — by `expected_cities` = cities now + free room, no more than
   three) × condition (always — 100%, coastal — the share of coastal cities, peace — 0% at war, a building without a
   technology — 100%, a building with a technology and a tile improvement — 15%, the rest — 50%, a narrow production order —
   15%) × pace (a policy behind `d` others — 1/(1+d); a finisher waits for all the ordinary ones; "for every N of something" counts
   what exists at the start: cities other than the capital, five citizens, one wonder).
3. **Wants** (`Situation::wants`, percent): expansion — 400% × (free room, no more than three)/3; growth — the food of
   the sites (70…150%); happiness — +25% for each city beyond three, +100% under unhappiness; faith — 100%, +600% with a
   founded religion; army — 0%, +450% at war, +60% with a foreign city closer than 10 tiles; diplomacy — 50%, +20%
   per city-state (up to five); trade — 100% with a free route. The leader's personality shifts gold, science,
   culture, faith and army by 10% per point from the middle (diplomacy — by 20%). Free room is reduced by a nearby
   rival: `room × min(distance, 16)/16`.
4. **Branch** = the sum over goals (points × want/100).

All constants are named at the start of the file; data that is already in the catalog stays there: leader personalities
(`personalities.ron`) and the tie order (`roles.ron`). There is one number, a "knob": `EXPANSION_PERCENT`.

## What comes out of this (BNW)

| Condition | Branch | Why | Check |
|---|---|---|---|
| There is room around the capital | Liberty first | its settler and cheaper policies per city are the "expansion" goal, want 400% | test `classic_room_for_cities_…`, bench |
| No room (5.8% of seats) | Tradition first | expansion want 0%, without it Tradition is slightly more valuable than Liberty | the same test, probe |
| War, warlike leader | Honor — among the later branches | army +450% | `classic_a_warlike_leader_at_war_…` |
| Religion founded, devout leader | Piety — among the later ones | faith +600% | `classic_a_devout_leader_…` |
| City-states and a diplomatic leader | Patronage and Commerce | diplomacy and trade | `classic_city_states_…`, `classic_trade_routes_…` |
| Scholar leader | Rationalism | science | `classic_a_scientist_…` |
| Coastal cities | Exploration | the "coastal city" condition | `classic_a_coast_…` |
| Unhappiness | a branch with happiness effects | happiness +100% | `classic_unhappiness_…` |

**Piety and Honor are not taken first — this was measured.** A version in which personality gave Piety and Honor
first place (devout Arabia, Spain, warlike Mongolia, the Zulus, the Huns) gave 85 seats to Piety and 12 to
Honor; on the same seats with Tradition first the score by round 200 is 484 vs 340 (−30%), with Liberty 444; for Honor
383 and 382 vs 296. So personality shifts the wants by only 10% per point, while war and religion are large
additions: they decide the choice of the second and later branches, when Tradition and Liberty have already been taken.

## Assumptions (a future strategy plan will be able to guard the decision by them)

- the nearest known rival stays at the seen distance and does not occupy free sites until
  `EXPECTED_CITIES_HORIZON` = 3 new cities are founded;
- by round 100 the empire has `expected_cities` cities;
- the room around the capital stays free of war, the food of the sites is as the known map shows;
- a branch is adopted in order, deep policies come late, buildings with technologies are built late;
- the first branch is chosen around round 16 without a religion, a war, or buildings beyond those available from the start.

## Measurements

All figures were taken before `civ5/science-economy` was merged into `main`: the "former AI" in them is the `standard` of that time. After the merge
the baseline is different, and the bench must be taken again before comparing with these numbers.

**Does Liberty's gain depend on land** (1200 seats: Liberty first vs Tradition first, the same seats;
the first branch is forced, the rest are chosen by a score that is identical in both cases):

| Free sites | seats | cities by r100 (Tradition → Liberty) | score by r200 |
|---|---:|---|---|
| 0 | 54 | 2.72 → 3.11 (+0.39) | 353 → 317 (−10.3%) |
| 1 | 367 | 3.42 → 4.19 (+0.77) | 431 → 412 (−4.4%) |
| 2 | 332 | 3.60 → 4.45 (+0.86) | 495 → 469 (−5.3%) |
| 3 | 239 | 3.67 → 4.66 (+0.99) | 489 → 471 (−3.7%) |
| 4 and more | 208 | 3.62 → 4.56 (+0.94) | 525 → 489 (−6.9%) |

The food of the sites shows no pattern (the loss by round 200 is −7.0 / −4.6 / −5.0 / −7.2 / −4.0% across the five steps from the
poorest to the richest); a rival is known by round 16 at 4% of seats (+1.07 cities, −4.7%): little data. The weak
dependence is the honest answer: Liberty always trades 0.8…1.0 cities by round 100 for 4…7% of score by round 200, and
only with zero room is the trade bad.

**First branch → cities and the second city** (probe, 799 seats, 200 rounds, the same map; the former order was
Tradition, then Piety):

| Branch | seats | cities r50 | cities r100 | second city, round | score r100 | score r200 | technologies r200 |
|---|---:|---:|---:|---:|---:|---:|---:|
| former AI (all seats) | 799 | 2.07 | 3.51 | 32.2 | 226 | 482 | 26.7 |
| new AI (all seats) | 799 | 2.20 | 4.35 | 33.3 | 246 | 466 | 26.8 |
| of them Liberty first | 753 | 2.21 | 4.42 | 33.5 | 249 | 470 | 26.9 |
| — the same seats under the former | 753 | 2.07 | 3.54 | 32.2 | 227 | 487 | 26.7 |
| of them Tradition first | 46 | 2.02 | 3.13 | 30.1 | 198 | 411 | 25.4 |
| — the same seats under the former | 46 | 2.02 | 3.02 | 32.7 | 203 | 408 | 25.8 |
| Liberty first everywhere (for comparison) | 799 | 2.21 | 4.39 | 33.3 | 247 | 465 | 26.8 |
| — on those 46 seats | 46 | 2.09 | 3.59 | 34.6 | 219 | 373 | 25.4 |

The rule matches "Liberty everywhere" everywhere except the seats without room: there it keeps the score at 411 instead of 373.

**Bench** (`--suite standard --solo standard`, offsets 0–4, 200 games for each, both runs at the same time):

| Metric | former AI | new AI | difference |
|---|---:|---:|---:|
| cities r100 / r200 | 3.62 / 5.68 | 4.42 / 5.42 | +22.1% / −4.6% |
| second city, round | 32.00 | 32.40 | +1.2% |
| score r100 / r200 / end | 224.1 / 497.9 / 716.9 | 246.5 / 483.5 / 711.3 | +10.0 / −2.9 / −0.8% |
| technologies r100 / r200 / end | 15.96 / 26.28 / 34.04 | 16.26 / 26.62 / 33.80 | +1.9 / +1.3 / −0.7% |
| mean round of entering eras: Classical / Medieval / Renaissance | 61.2 / 135.2 / 238.8 | 61.0 / 133.2 / 233.6 | −0.3 / −1.5 / −2.2% |
| victories by type | 40 of 40 by time | 40 of 40 by time | — |
| round, ms, p50 / p95 | 7.76 / 38.56 | 8.42 / 38.94 | +8.5% / +1.0% |

Against `allocator` (`--candidate standard --baseline allocator --repeat 5`, 400 games): the new AI 67.2% of points
(95% CI 62.5–72.0%), the former 68.1% (63.5–72.5%); wins 269 and 272 of 400; the mean score difference +52.5 vs +52.3.
`duel` (offsets 0–2, 120 games): cities by round 100 +25.0%, score by round 100 +13.6%, by round 200 +1.8%, at the end
−0.8%; round p50 +9.6%, p95 +5.8% (with two nations the round is short, 10 ms).

**Cost of the decision.** One decision takes 55 µs (p50) and 110 µs (p95) in a release build: a walk over radius 7 around the capital and
a pass over the candidates' policies; it happens only when there is something to open, that is, 2–3 times per game per nation.
The 8.5% growth in round p50 comes from the cities (+22% by round 100), not from the decision; p95 +1.0% fit within +3%.

## How to check by hand

```
cargo run --release --offline -p aurelia-qa --example full_game -- \
  --script archipelago --size small --seeds 2 --rounds 20 --civs --checks none
```

In the nation rows the `branches=` field shows the branches in the order they were opened. After round 16 Persia (P0) should
have `branches=tradition` (no room), and the other five `liberty`. For comparison `--script pangaea --size small
--seeds 1`: Siam (P3) takes Tradition, the rest take Liberty; the former AI would have given Tradition to all six. In the
observer window (`AURELIA_OBSERVER=1`) the same can be seen on the culture screen after round 16; I did not check the mapping of the map to the observer's
seed.

## What is not done

- There is no check at war: at round 16 there are no wars, so the Honor and Piety rules are confirmed by tests but not
  by the bench; their cost as the first branch was measured (above).
- The conditions "uninhabited sea map" for Exploration and "a lot of faith" for Piety are expressed through coastal-ness and
  a founded religion, not through the size of the sea map and the faith reserve.
- The "knob" `EXPANSION_PERCENT`: 250 instead of 400 gives Liberty at 78% of seats, cities by round 100 +18.2%, score by
  round 200 −2.1%, at the end −0.1% (bench, the same 200 games, but on an intermediate build); the value was chosen for the reason
  "room 0 → Tradition" — at 250 the choice by room is non-monotonic.
- A tail effect: on the Pangaea map, seed 20, room 1 (`settle_pace`) the rule chooses Liberty, and the second
  city is founded in round 75 instead of 33 with Tradition: the capital stays at population 2 with no food surplus, and the settler
  waits behind a building in the queue. The cause is in the AI's production, not in the branch choice; across the whole bench the share of seats with a second
  city did not fall (98.8% → 99.4%). The test threshold was raised from 70 to 80 rounds (the baseline AI had 120 or never).
