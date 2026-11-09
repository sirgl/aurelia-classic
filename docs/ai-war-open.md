# War open: one brain for the strategist and the war machine

**`war-open` is `standard` whose strategy net sees the war machine (the best known target's learned capture chance × its worth, an operation forming, its army against the neighbour's), whose strike window opens in any era on an opportunity, whose posture permits and prices the offensive operation, and whose war case prices a capture as the AI really keeps it. On the acceptance seeds (offsets 220–224, run once after the code was frozen) it passes the three gates: 50.5 % of the points against `standard` on the standard suite (95 % CI 48.5–52.5 %, 400 games, no difference), 3.68 wars declared, 1.00 city captured and 4 capitals lost in 100 pace games where `standard` declares 0.24 wars and captures 0.04 cities, and the prepare or war posture in 16.5 % of the reviews (`standard`: 1.6 %). The diagnosis behind it: the war case was negative because of two defects (a city the AI puppets priced as annexed, a mood step charged to the end of the game), an undervalued capture (half a hammer and half a beaker a citizen) and an expected loss that is real; on the tuning seeds (offsets 200–201, 160 games) every setting of the wars measured 46–48 % — more wars did not buy strength, and the acceptance's 50.5 % is within the same noise. `standard` is byte-identical.** State of 9 November 2026, `classic` branch `ai/war-open` on `main` `186bb9c`. Labels: [measured] in this work, [read] in the code.

Code: the variant `Variant::WarOpen` (`war-open`, `TurnBrief::war_open`); the strategy net's features `fe::OPPORTUNITY`, `OP_FORMING`, `STRENGTH` (`strategy_net::features::open` ← `operations::outlook`, `known_power`, `era_strength`), the window of any era and the intent's terms (`weights::open`, `concepts_of`, `intent_of`); the posture's gate (`operations::posture_permits`) and the net's army orders (`heads::military_value`); the war case (`SiegeFixes::OPEN`: `puppet`, `recovery` → `Prices::kept_city_unrest_recovering`, `yields` → `city_worth_yielding` / `Prices::citizen_yield`, `loss` → `OPEN_LOSS_PERCENT`); the capture chance's three new features (`CHANCE_FEATURES`: `strength`, `era_edge`, `ready`) and its refit for `war-open` (`OPEN_CHANCE_*`, `open_chance`). The probe: `operations::CaseParts`, `war_probe::Estimate` (the case's terms), `war_probe::WarProbe::{members, alive}` (what an operation's army loses); the tool `ai-war-ops` (`--speed`, `--estimates`, the losses per operation). Tests: `ai::variant::tests::war_open_is_standard_but_for_its_wars`, scenarios 3b and 3c of `tests/ai_strategy_net.rs`. `standard` is byte-identical (result hashes below).

## 1. Diagnosis: why 99 % of the war cases are negative [measured]

`ai-war-ops --variant standard --suite pace --speed quick --seeds 200001..200021 --estimates …` (100 games, every seat `standard`): 653 599 war cases priced, **100.0 % negative** (30 positive), 13 operations started. The case is `appetite × chance × capture − (built + lost + weapons)`; medians in points:

| term | p10 | median | p90 |
|---|---:|---:|---:|
| chance (learned) | 10 % | 21 % | 35 % |
| capture's worth before the chance | −817 | 19.6 | 55.4 |
| units built (upkeep; hammers with `asset` off) | 0 | 7.0 | 32.4 |
| units expected lost (`LOSS_PERCENT` 50 % of the need) | 41.2 | 102.5 | 187.7 |
| value | −282 | −128 | −60 |

| share of the cases negative if … | |
|---|---:|
| as priced | 100.0 % |
| the chance were 100 % | 93.4 % |
| no expected loss | 68.4 % |
| no cost at all | 43.2 % |

Two terms make them negative, in this order:

1. **The expected loss of the army**: half the planned army (9 units at the median) at the hammer's price, 102 points against a capture worth 20; the break-even chance is 287 % at the median. Measured on 1 399 operations of `war-fix-model` (pace, Quick, offset 200), the members lose **25 % of the need** from start to end (62 % of those that march, 8 % of those recalled before).
2. **The capture priced below nothing in 44 % of the cases**: a target of 6+ citizens is priced as annexed (`captured_parts` before `puppet`), though the game's AI puppets every city since the eighth promotion, and a mood line the city's unrest crosses is charged to the end of the game, though the empire climbs back within ~80 Quick rounds (classic/docs/ai-conquest-payoff.md). Median capture by size: 1–2 citizens +31.6, 3–5 +39.7, 6–8 −11.0, 9–12 −107.9, 13+ −330.9 points.
3. And a third, found by the realised yields: `city_worth` counts half a hammer and half a beaker a citizen, where a captured city held to the end makes 3.1 yields a citizen (5.0 hammers, 5.8 beakers, 2.9 gold, 2.1 culture at 5.1 citizens).

**Calibration of the chance.** On the 1 399 exploration operations the compiled learned chance predicts 11.9 % and 8.8 % take a city of their enemy (0–10 %: 5.7 → 4.9 %; 10–20 %: 14.3 → 10.9 %; 20–30 %: 22.6 → 13.8 %), log loss 0.295 against 0.298 for a constant: calibrated at the low end, overconfident above 20 %, almost without skill. 66 % of the operations end recalled by the defence. On `standard`'s own 13 operations: admitted at 17.6 %, 30.8 % took a city (too few to say more).

**What the diagnosis says.** Conquest is priced out first by two defects of the case (the annex price of a city the AI puppets, the mood step charged to the end of the game) and an undervalued capture (half a hammer and half a beaker a citizen), then by the army's loss, which is real: `war-open`'s own operations lose 66 % of their planned army (marched ones 124 %: every unit on an assault of the target joins them and the war is fought by both sides), take a city of their enemy in 14.7 % of the cases (marched: 23 %), and 37 % of them are recalled by the defence. With every term honest (section 2) with the capture's three corrections and the first fit 86 % of the cases are still negative (break-even chance median 43 %; `war-open`'s first build at offset 201), and the wars that pass cost the bench about 2.5 points of share (section 3): **under these rules and with this war machine, conquest does not pay its army in a game of 4–6 nations** — the rival's loss is shared with the bystanders, and the realised capture rate stays at 12–15 % whatever the features (the refitted model barely beats a constant: test log loss 0.314 against 0.316). The rule that makes it so is the price of war against the price of a city: a city is worth about 10 points plus a few citizens of score and its yields, an operation loses 6–13 units of 50–120 hammers each, and the city heals 20 HP a round (`CITY_HEAL_PER_TURN`) behind walls, so most sieges end by peace or recall before the city falls.

## 2. What `war-open` changes

| Part | `standard` | `war-open` | Where |
|---|---|---|---|
| What the net sees of the war machine | nothing | `opportunity` = the best target's learned chance × its capture's worth over an own city's (50 % at an own city's worth = 1); `op_forming` = an operation forms or marches, or its chest is saved; `strength` = own military power over own plus the target owner's known power (seen units, remembered threat, or `ARMY_PER_CITY_TENTHS` units a known city of its era's best melee unit; 0.5 level); `pays` = the war machine admits a target now (`candidate` with the leader's stakes, before the route) | `features::open`, `operations::outlook`, `known_power`, `era_strength` |
| Strike window | AND(warlike, rival near, early): shut after the first eras | the larger of that and AND(pays, opportunity, strength) with the leader's appetite (`warlike`), an era ahead, danger and a crisis as terms; any era | `weights::open::WINDOW`, `concepts_of` |
| Posture | peace / defence / prepare / war from danger, the early window, at war | + `op_forming` → prepare (+2000), peace (−1500) | `weights::open::POSTURE` |
| Style, aim | conquest and domination from the early window | + the opportunity (+1000 conquest, +600 domination); a locked style reads the opportunity and the events, not the window (a transient window flipped the style for 35 % of the reviews and cost the economy) | `weights::open::STYLE`, `AIM`, `STYLE_EVENTS` |
| Who starts an operation | the war machine whenever its case pays (any posture but the old `Defend` strategy) | only in the posture prepare or war: the net opens the window, the war machine picks the target in it | `operations::posture_permits` |
| The net's own army orders | in prepare and war | in war, and in prepare only while an operation forms (the army is the operation's need, `operations::demand`) | `heads::military_value` |
| The capture's price | annexed at 4+ citizens; mood steps to the end of the game; ½ hammer + ½ beaker a citizen; half the city's points as the rival's loss | a puppet (`puppet`); a mood step for `UNREST_RECOVERY_ROUNDS` = 120 Standard rounds (`recovery`); the empire's own yields a citizen (`yields`, `Prices::citizen_yield`); the rival's points and yields by its standing: all when it leads the others, half when it is ahead of us, nothing when behind (`rank`, `rival_percent`) | `SiegeFixes::OPEN`, `captured_parts_by`, `city_worth_ranked` |
| The army's loss | `LOSS_PERCENT` 50 % of the need | `OPEN_LOSS_PERCENT` 66 %: measured on 638 operations of `war-open` (offset 201) | `SiegeFixes::loss` |
| The capture chance | `CHANCE_WEIGHTS` (12 features; the 3 new ones weighed 0) | `OPEN_CHANCE_WEIGHTS`, the same logistic model with `strength`, `era_edge`, `ready`, refitted on 2 055 operations (two turns of the loop) | `open_chance`, `chance_features` |

Every hand weight carries its reason in `weights::open`; the fitted constants carry their data and fit. Integer arithmetic throughout; nothing reads hidden information (`met_eras`, the victory screen's scores and the units in sight are public).

The loop: the first fit (1 417 operations: 1 399 of `war-fix-model`, whose siege-model chance starts many, and 18 of the first `war-open`) admitted `war-open`'s operations at 27.7 % where 14.7 % took a city; the second fit adds those 638 operations and is calibrated on the held-out seeds (9 % → 9 %, 27 % → 13 %, 44 % → 22 %), and the loss was refitted from 25 % (`war-fix-model`'s operations, mostly recalled before they march) to `war-open`'s own 66 %.

## 3. Measurements

Tuning (offsets 200–201 only; `ai-tournament --candidate war-open --suite standard --speed quick --repeat 2 --seed-offset 200`, 160 games, and `--solo war-open --suite pace --speed quick --seed-offset 200`, 100 games). The builds in order, each with the earlier ones' changes:

| build | change | standard suite share (95 % CI) | pace: wars / captured / capitals lost | war length, mean |
|---|---|---|---|---:|
| `standard` | — | 50 % | 24 / 4 / 0 (offset 0) | 26.5 |
| E1 | posture gate, window of any era, `puppet` + `recovery`, loss 25 %, first fit | 48.1 % (45.0–50.6) | 226 / 36 / 0 | 17.7 |
| E2 | + `yields` | 46.9 % (42.5–50.6) | 500 / 134 / 4 | 21.6 |
| E3 | + a locked style ignores the window | 48.1 % (45.0–51.2) | 572 / 161 / 4 | 23.3 |
| E4 | + `rank` (the rival's loss by its standing) | 45.9 % (41.9–50.0) | 609 / 230 / 4 | 26.3 |
| E5 | + second fit, loss 66 % (`war-open`'s own operations) | 47.5 % (43.8–51.2) | 275 / 110 / 3 | 20.2 |
| E6 = frozen | + `pays` in the window | 47.5 % (44.4–50.6) | 307 / 71 / 2 | 18.2 |

Strategy traces of the tuning seeds (two games each, all seats `war-open`): prepare + war 12.2 % (E1), 38.7 % (E2, the conquest style 35 % of the reviews), 18.6 % (E5, but an operation formed in only 5.6 % of them: windows without a war that pays — the reason for `pays`).

**Acceptance** (offsets 220–224, frozen at `53f1704`, run once):

| gate | command | result | |
|---|---|---|---|
| points share ≥ 50 %, CI low ≥ 46 % | `ai-tournament --candidate war-open --suite standard --speed quick --repeat 5 --seed-offset 220 --jobs 5` (400 games) | **50.5 % (48.5–52.5)**; runs 51.2 / 52.5 / 50.0 / 51.2 / 47.5 %; score difference −4.5 / −3.0 / +1.9 / +9.9 / −12.6 a seed block | passed |
| wars ≥ 1 a game, captures ≥ 0.5 a game, capitals lost > 0 | `ai-tournament --solo war-open --suite pace --speed quick --seed-offset 220 --jobs 5` (100 games) | **368 wars (3.68), 100 captured (1.00), 4 capitals lost**; 3 nations eliminated; 94 games by time | passed |
| prepare or war ≥ 5 % of the reviews | `ai-strategy-trace --suite pace --map pangaea|continents --seed 220001…220006 --candidate war-open --baseline war-open --only review` (12 games) | **16.5 %** (prepare 8.7 %, war 7.9 %; peace 67.7 %, defence 15.7 %; 8 612 reviews) | passed |

Head to head (400 games): `war-open` captured 37 cities and lost 22, `standard` captured 9 and lost 22; wars declared 93 against 49.

**Peace.** The wars `war-open` starts end by a peace deal (`ai::peace`): pace 361 of 368 by deal, 4 running at the end, 3 by elimination, mean 19.5 rounds, median 12–13 (`standard`: 22 of 24 by deal, mean 26.5); head to head 89 of 93 by deal, mean 13.8 rounds. A war whose operation was recalled or ended has no attack case left (`running_case`), so the next deal ends it — the short median is that, not a refusal to make peace.

**`standard` unchanged** (the frozen binary): `ai-tournament --solo standard --suite standard --speed quick --jobs 5` → result hash `22e61a7bed77b7d5` (40 games), `--suite pace` → `e09b7312a032550d` (100 games), both as on `main`.

## 4. Not done

- The tuning seeds put every build at 46–48 % and the acceptance seeds at 50.5 %: the difference is the noise of 160 against 400 games; the effect of the wars on strength is not shown to be positive, only not shown to be negative.
- What costs the tuning share when the wars are few (E5, E6 at 47.5 % with 16–37 wars) is not isolated: the posture's price tilts while an operation forms, the gate on the net's army orders, or noise.
- The capture chance has almost no skill on `war-open`'s own operations (log loss 0.383 against 0.372 for a constant on 638 of them); the pooled fit is calibrated but the opportunity it feeds is close to a constant 12–15 %.
- The recall still ends 37 % of `war-open`'s operations; the loss of 66 % counts every unit that joins a marching operation, which is the war's cost, not the planned army's.
- `rank` reads the victory screen's scores each turn (`TurnBrief::scores`, `war-open` only); `known_power` guesses an army out of sight from `ARMY_PER_CITY_TENTHS`.
- Not measured: duels, Standard speed, the land duels against the books.

## Manual check

```sh
cd classic
CARGO_TARGET_DIR=target cargo +1.98.1 build --release -p aurelia-qa --bin ai-tournament --bin ai-strategy-trace --bin ai-war-ops
target/release/ai-strategy-trace --suite pace --map pangaea --seed 220001 --candidate war-open --baseline war-open --only review | grep "posture.*→ prepare" | head
target/release/ai-tournament --solo war-open --suite pace --speed quick --seed-offset 220 --jobs 5   # the war tables
target/release/ai-war-ops --variant war-open --suite pace --speed quick --seeds 201001..201021 --out ops.csv --estimates est.csv
```

In a game: start a Classic game at Quick speed with `AURELIA_AI_VARIANT=war-open` on a small pangaea map with five AI nations, play to round ~100–150 and watch the nearest AI: its review lines (strategy trace) show `pays=1` and `window` above 0.5, the posture turns to prepare, its cities build the operation's units, and a few rounds later it declares war on the neighbour whose city its war case priced; the war ends by a peace deal 12–20 rounds later, often with a city taken.
