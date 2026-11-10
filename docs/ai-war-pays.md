# War pays: the second iteration of the war lane

**`war-pays` is `war-open` with the first iteration's review resolved. No effect on the points share is shown: every build of the lane, including this one, sits between 48 % and 50 % of the points against `standard` on the tuning seeds, and the trend of `war-open` was slightly negative (48.5 %, 47.0 % in the head-to-head pace games, 48.2 % on the reviewer's replication). What the work found is where the points went: not in the wars, but in the strategy net's price tilt while an operation forms. `war-pays` removes that tilt, makes the window a war that pays a condition, and starts an operation only where the measured odds are good (a first war against a city of at most 8 citizens: 29 % of those operations take a city, 6–12 % of the others). On the tuning seeds it declared 96 wars in 800 standard-suite games against `standard`'s 42 (`war-open`: 170 against 78) and 1.65 wars and 0.54 captured cities per solo pace game; it did not make the AI stronger.** State of 10 November 2026, `classic` branch `ai/war-pays`, on `ai/sharp` (`153808a`) on `main` `186bb9c`. `standard`, `war-open` and `sharp` are byte-identical (result hashes below). Labels: [measured] in this work, [read] in the code.

Code: `Variant::WarPays` (`war-pays`; `ClassicMind::new` plays it as `WarOpen` with the flag `TurnBrief::war_pays`, as `sharp` does for `Standard`); `strategy_net::Net::pays` (the window needs `fe::PAYS`; no posture terms; no opportunity in the style and the aim: `Net::pays_no_style`); `operations::odds_are_good`, `MAX_TARGET_POPULATION`, `SiegeFixes::PAYS`, `PAYS_LOSS_PERCENT`, `counted_free`; `war_probe::muted`. Tests: `war_pays_needs_a_war_that_pays`, `war_pays_does_not_tilt_the_posture_for_a_forming_operation` (`tests/ai_strategy_net.rs`), `war_pays_plays_war_open_with_its_flag` (`ai::driver`).

## 1. Where the points went: ablations of `war-open` [measured]

Each row is `war-open` with one part removed, against `standard`, Quick, standard suite, 10 seed offsets 200–209 (800 games, tuning seeds). Share: mean of the ten offsets, 95 % CI over them. Paired: the difference to the reference on the same ten offsets (the games are the same worlds), 95 % CI. Wars and captures are the candidate's, summed over the 800 games.

| removed | share (95 % CI) | wars / cities captured | paired to the reference |
|---|---:|---:|---:|
| nothing (`war-open`) | 48.5 % (46.7–50.3) | 170 / 74 | — |
| (a) everything but the net: no gate, `standard`'s war case, the compiled chance (the posture, window, style and aim stay) | 49.6 % (48.9–50.3) | 34 / 8 | +1.14 (−0.57…+2.85) |
| (b) the gate "an operation only in prepare or war" | 48.1 % (46.2–50.1) | 322 / 124 | −0.38 (−1.72…+0.96) |
| (c) the re-priced war case (`standard`'s) | 49.5 % (48.4–50.6) | 16 / 2 | +1.01 (−0.92…+2.94) |
| (d) the refit chance model (the compiled one) | 49.2 % (46.5–51.9) | 286 / 101 | +0.74 (−0.78…+2.26) |
| (e) the opportunity in the style and the aim | 49.3 % (48.2–50.3) | 171 / 80 | +0.76 (−0.66…+2.18) |
| the posture terms of a forming operation (prepare +2000, peace −1500) | 49.5 % (47.5–51.5) | 171 / 76 | **+0.99 (+0.30…+1.68)** |
| the gate on the net's army orders | 48.5 % (46.7–50.3) | 170 / 70 | +0.00 (−0.73…+0.73) |
| the window of any era | 48.9 % (47.2–50.6) | 18 / 2 | +0.39 (−0.47…+1.25) |

Reading: at 800 games nothing but one row separates from noise. **The posture terms of a forming operation cost about a point** with the same number of wars: while an operation forms the net holds the posture at prepare and prices the army up (for 20–40 rounds), on top of the units the operation itself builds (`operations::demand`). The wars alone did not cost it: the same 170 wars without the tilt score 49.5 %. The first iteration's open question "the price tilts, the gate on army orders, or noise" is answered: the price tilt; the gate on army orders is neutral. Parts (a)–(e) each move the share by +0.7…+1.1 with a CI that includes zero: more wars lower the share a little (322 wars 48.1 %, 16–34 wars 49.5–49.6 %), the fitted chance and the opportunity in the style are not what costs.

## 2. What separates the operations that take a city [measured]

`ai-war-ops --variant war-open --suite pace --speed quick --seeds K001..K021` (100 games each), K = 200, 201 (fit) and 202 (held out): 334, 336 and 294 operations, 13.8 %, 17.9 % and 18.7 % of them took a city of their enemy (in the war from the start of the operation to 20 rounds after its end).

| observable at the start | operations | took a city |
|---|---:|---:|
| a war with the target's owner already running | 218 of 670 (offsets 200–201) | 6.4 % (2.9 % at 200 alone) |
| no war running | 452 | 20.4 % |
| no war running, target of 9+ citizens | 224 | 11.6 % |
| no war running, target of at most 8 citizens | 228 | **28.9 %** |
| the same on the held-out offset 202 | 96 of 294 | **29.2 %** (the rest 13.6 %) |

Nothing else separates in a way that survives the held-out seeds: the distance, the strength share, the guard, the enemy's cities, the era edge and the readiness show 8–25 % bins with 30–180 operations each. A logistic fit on at_war, population, guard, enemy cities and readiness (train 200, test 201) is no better than a constant at the test rate (log loss 0.483 against 0.469): the rule on two observables is used instead of a model. An operation started in a war that is running took a city in 3–11 % of the cases and lost 2.3 units on average (it rarely marches: 16 %); one started at peace against a small city took one in 29 % and lost 6.2.

`war-pays` ops (offsets 203–204, 292 operations): 24.0 % took a city, 67.3 % of the planned army lost (101 % of those that marched). On the frozen build (offset 205, 141 operations): 31.2 % took a city, 51.8 % of the army lost.

## 3. What `war-pays` changes

| Part | `war-open` | `war-pays` | Where |
|---|---|---|---|
| `PAYS` in the window | a term (+1000); the strict test was loosened to 0.4 | a condition: without a war that pays the window is the early one's; the strict test is back (`< 0.1`) | `concepts_net`, `war_pays_needs_a_war_that_pays` |
| Posture | + `op_forming` → prepare (+2000), peace (−1500) | `standard`'s | `intent_net` (`Net::pays`) |
| Style, aim | + the opportunity | `standard`'s (no measured effect: p2 49.9 %, p3 49.9 %, paired −0.01 (−1.17…+1.15); kept out for the smaller coupling) | `Net::pays_no_style` |
| Which operation starts | the war case alone, in the posture prepare or war | + `odds_are_good`: a war not yet running, a target of at most `MAX_TARGET_POPULATION` = 8 citizens; the outlook's `pays` and `start` use the same filter | `candidate` |
| Units counted free for `pays` | the strike units | the strike units and those on an assault, as `start` counts them | `counted_free` |
| The rival's loss | by standing (`rank`), (points + yields) × 2 when it leads | a flat `RIVAL_PERCENT` = 50 % of points and yields | `rival_percent` |
| The army's loss | `OPEN_LOSS_PERCENT` = 66 % (E2) | `PAYS_LOSS_PERCENT` = 67 %, measured on its own operations | `SiegeFixes::PAYS` |
| The probe | `outlook` and `start` both record | `outlook` is muted | `war_probe::muted` |

The window, the stakes, the capture's yields, the puppet price and the unrest recovery are `war-open`'s, unchanged. Ended wars: a war with no operation has no attack case (`peace::WarProspect`), so the next deal ends it; the odds rule starts no second operation in a running war, so a war that stopped paying is ended by the existing peace by value (mean 20.3 rounds, 161 of 165 wars by deal in solo pace). Army for a war that is not coming: the window needs a war that pays and the posture is no longer held at prepare while an operation forms.

## 4. Resolution of the reviewer's findings

1. `PAYS` is a condition of the window in `war-pays` (`Net::pays`); `war-open` keeps the term and its loosened test, whose comment now says 0.4. The strict assertion is restored for `war-pays`.
2. `rival_percent`: measured, ranking the rival (all when it leads, half ahead, nothing behind) against a flat half of the points and yields: 49.1 % and 49.1 %, paired +0.00 (−1.35…+1.35), 138 against 102 wars, 54 against 28 captured cities in the head to head (offsets 200–209, build p1). No share is bought, so `war-pays` returns to the flat `RIVAL_PERCENT`; the formula is points + yields at 150 %, not the old half of the points alone.
3. Constants stated as tuned: `OPEN_LOSS_PERCENT` (measured on build E2, bypasses the genome's `LossPercent`), `UNREST_RECOVERY_ROUNDS` (one case measured 80, 120 is a margin), `ARMY_PER_CITY_TENTHS` (the AI's own ratio applied to rivals), the window's weights (hand-set on offsets 200–201) are marked TUNED in the code. `war-pays` re-measures the loss on its own operations: 67 % at p1 (292 operations), 52 % on the frozen build (141); the constant is 67 % and not lowered, because a lower loss admits more operations that were not tuned; it is a tuned constant. Its other new numbers (`MAX_TARGET_POPULATION` = 8, the exclusion of running wars) are tuned on 670 operations and checked on 294 held out.
4. The probe's double count is fixed (`war_probe::muted` around `outlook`'s look).
5. `pays` and `start` priced with the same prices: the `Prices` handed to the net and to the war machine are the same (only `city_cap`, which the war case never reads, is set after the net), and the plan's military multiplier is read by the net's army orders, not by the war machine. They differed in the free units (`start` counts units on an assault) and in the route, the season and the chest; the free units are now the same (`counted_free`).
6. This document says "no effect shown", above.

## 5. Tuning [measured]

Tuning seeds 200–209 only, 800 games each (`war-pays` against `standard`, Quick). p1 = the odds rule and the `PAYS` condition on `war-open`; p2 = p1 without the posture terms; p3 = p2 without the opportunity in the style and the aim; p4 = `war-open`'s war machine with the net removed altogether (`standard`'s net, no gate); f1 = p3 with the flat rival (the frozen build).

| build | share (95 % CI) | wars / captured | paired |
|---|---:|---:|---|
| `war-open` | 48.5 % (46.7–50.3) | 170 / 74 | |
| p1 | 49.1 % (47.2–51.0) | 138 / 54 | vs `war-open` +0.62 (+0.15…+1.09) |
| p1 with the flat rival | 49.1 % (47.4–50.8) | 102 / 28 | vs p1 +0.00 (−1.35…+1.35) |
| p1 with targets of at most 6 citizens | 48.9 % (46.9–50.8) | 107 / 40 | vs p1 −0.25 (−0.80…+0.30) |
| p2 | 49.9 % (48.1–51.7) | 140 / 55 | vs p1 +0.76 (+0.13…+1.39) |
| p3 | 49.9 % (48.7–51.0) | 121 / 56 | vs p2 −0.01 (−1.17…+1.15) |
| p4 | 49.8 % (47.9–51.6) | 251 / 82 | vs p3 −0.12 (−2.43…+2.19) |
| **f1 (frozen)** | **49.6 % (48.9–50.4)** | **96 / 31** | vs p3 −0.24 (−1.32…+0.84) |

Pace on offset 200 (the frozen build): head to head 200 games 51.0 % (48.5–53.5), science per turn at round 264 335.3 against 339.4 (−1.2 %); solo 100 games 1.65 wars, 0.54 captured cities and 4 capitals lost per game / in total, 161 of 165 wars by peace deal, 3 science victories by round 330 (`standard`: 2 in 100). A better share than the 49.6 % cannot be told from noise: the standard error of a ten-offset mean is about 0.8 points.

## 6. Not done

- The gates ask 50 % with a lower bound of 47 % at 800 games; every build of the lane lies at 49–50 %, so the point estimate decides about half the time.
- The loss constant was not iterated: lowering it to the 52 % measured on the frozen build admits more operations and changes the build that was tuned.
- Operations that take a city 29 % of the time still lose 6–7 units each; the first iteration measured the expected loss of an operation at about 100 points against a capture worth 20–60, and a capture rate of 29 % does not close that gap. Wars cannot pay until the operations take cities more often, or lose fewer units: tactics, which this lane does not touch.
- Not measured: duels, Standard speed, a human opponent.
