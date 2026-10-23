# Fitness of the AI: a whole game as a metric

**One command, `ai-fitness`, plays a seed set of whole games and answers with one number per candidate (a variant, or a variant tuned by a genome): the mean z-score of its seats against the reference AI, with a bootstrap interval over seed blocks and the paired difference to a baseline.** What is measured and how it is weighed is data (`integration/benches/ai/fitness.json`), the tunable numbers are a documented genome of 30 knobs, and the cost, the noise and the value of a short horizon are measured below. State of 20 October 2026, branch `ai/fitness`. Built on the AI bench ([ai-bench.md](../../platform/docs/ai-bench.md)) and the development metrics and the unlock-to-built lag of [ai-science-diagnosis.md](ai-science-diagnosis.md); nothing there is duplicated.

Contents: [Use](#use) · [Stable interface](#stable-interface-for-other-agents-and-scripts) · [Definition](#definition) · [Why these components](#why-these-components) · [Gaming it](#ways-to-game-it-and-the-guards) · [Genome](#genome) · [Cost, noise and the horizon](#cost-noise-and-the-horizon) · [Mac](#mac-and-external-optimisers) · [Not verified](#what-is-not-verified)

## Use

```sh
cargo +1.98.1 build --release -p aurelia-qa --bin ai-fitness
# a variant against the standard AI, 20 seeds, 150 rounds
target/release/ai-fitness --variant allocator --paired-with standard --suite fit --seeds 20 --horizon 150 --jobs 3 --out r.json
# a genome (a JSON object of knob: number) tuning the standard AI
target/release/ai-fitness --genome g.json --suite fit --seeds 20 --horizon 150 --jobs 3 --out r.json
target/release/ai-fitness --list-knobs      # names, defaults, ranges (JSON)
```

| option | meaning |
|---|---|
| `--variant NAME` / `--genome FILE` | the candidate: any variant of `ai-tournament --list` (a new variant of another agent works without a change), alone or tuned by a genome (`--variant` is then the base, default `standard`) |
| `--paired-with NAME` | the baseline (default `standard`): the opponent on the other seats in versus mode, the arm played alone on the same seeds in solo mode |
| `--mode versus\|solo` | *versus* (default): on every map the candidate takes the even seats among the civilizations, then, in a second game of the same seed, the odd ones; the baseline plays the rest. Both arms play the very same worlds, every start and every place in the turn order is shared equally (the rotation of [ai-bench.md](../../platform/docs/ai-bench.md)). *solo*: every seat plays the candidate |
| `--suite fit` `--size S` `--seeds N` `--seed-offset K` | the seed set: seed `i` (1-based, `+1000·K`) plays the script `continents, pangaea, fractal, archipelago, small-continents` cyclically; size `small` (6 civilizations) by default; a set of N seeds is N seed blocks |
| `--horizon R` | the round the games are stopped after (default 150). The AI still plans for a game of `--game-rounds` (500, the time victory), so a stopped game is the first part of the whole one, move for move |
| `--also-horizons 150,300` | shorter horizons read off the same games (everything but the lag, which is only finished at the end) |
| `--preset science\|balanced\|win` | which weights give the headline scalar (all three are in the result) |
| `--config FILE` | another fitness definition (default: the shipped `integration/benches/ai/fitness.json`) |
| `--reference FILE` | the reference distributions; absent: cached in `--cache-dir` (default `.ai-fitness-cache`) and made on the first call by playing `--reference-variant` (default `standard`) alone on `--reference-seeds` (40) seeds at `--reference-offset` (900) |
| `--jobs N` | games in parallel (default 3: the machine is shared). Results do not depend on it |
| `--detail` | every seat's components in the result |
| `--out FILE` | the result as JSON (default stdout) |

Games are deterministic: the same options give the same `hash` and the same numbers on any machine and with any `--jobs`; only `wall_seconds` and `load_average` differ.

**Result** (`--out`, JSON; the keys an optimiser reads are the first two):

- `fitness`: the candidate arm's mean fitness in z units under `preset`, at `suite.horizon`; `ci`: its 95 % bootstrap interval over seed blocks (10 000 resamples of whole seeds, the generator of `bench::stats`);
- `paired`: `diff` = candidate arm minus baseline arm, taken inside each seed block, with its interval `ci` and the number of blocks (null when no baseline arm exists);
- `horizons.<h>`: for each horizon, `presets.<name>` (fitness, ci, baseline_fitness, diff, diff_ci), `components[]` (name, weight, raw mean, z mean with interval, the reference mean and sd, the baseline arm's z and the paired difference with interval, the share of seats that have the value), `eliminated`, `penalty_mean`, and `blocks[]`, the per-seed means of both arms (the data of the noise study);
- `candidate`/`baseline` (variant, the genome's changes from the defaults), `suite` (mode, size, seeds, maps, horizon, checkpoints), `reference` (file, variant, seeds), `games`, `seats`, `hash`, `wall_seconds`, `reference_seconds`, `games_per_hour`, `jobs`, `load_average` (Linux), `knobs` (the genome's table), `seat_rows` (with `--detail`).

## Stable interface (for other agents and scripts)

What a caller may rely on; a change to any item below is a breaking change and is announced in this file.

- **Command**: `ai-fitness [--variant NAME] [--genome FILE] [--paired-with NAME] [--mode versus|solo] [--suite fit] [--size small|standard] [--seeds N] [--seed-offset K] [--horizon R] [--also-horizons A,B] [--preset science|balanced|win] [--jobs N] [--cache-dir DIR] [--out FILE]`, plus `--list-knobs`. Exit 0 and the result file, or exit 1 and a message on stderr.
- **Genome file**: one JSON object `{"knob_name": number, ...}`; absent knobs keep their defaults, values are rounded and clamped to the range, an unknown key is an error, keys starting with `_` are comments. Knob names are never renamed or reused with another meaning; a knob is added by one line in `SPECS` (`classic/crates/classic-rules/src/ai/genome.rs`) and shows up in `--list-knobs` (`name`, `default`, `min`, `max`, `group`, `doc`). A knob that the code stops reading stays accepted and is ignored.
- **Result file keys**: `fitness`, `ci`, `paired.{diff,ci}`, `horizons.<h>.presets.<preset>.{fitness,ci,diff,diff_ci}`, `horizons.<h>.components[]`, `horizons.<h>.blocks[]` (`seed`, `candidate`, `baseline`), `candidate`, `baseline`, `suite`, `games`, `hash`, `wall_seconds`. Keys are only added, never removed.
- **In the games**: a candidate with a genome is the bench mind `variant@handle`; a scenario or a test that wants a tuned AI in its own games builds the mind with `ClassicMind::with_genome(variant, Genome::from_pairs(&pairs)?)` (the genome is the active one only while that mind plays, so other seats are untouched); with no genome every knob is its default constant and the game is move for move the game of the code without the mechanism.
- **Determinism**: the same options and genome give the same numbers and the same `hash` on any machine and with any `--jobs`. `--seed-offset` is the only knob that changes the maps; the shipped definition `integration/benches/ai/fitness.json` is data (`--config` replaces it).

## Definition

The definition is `integration/benches/ai/fitness.json` (embedded in the binary; `--config` replaces it). Code knows no weight.

1. **Components.** A *metric* read at some *rounds* is a component, named `metric@round` (`techs@100`; `@end` is the horizon). Checkpoint metrics: `cities`, `citizens_per_city`, `techs`, `science` (beakers a round), `science_buildings` (libraries plus universities), `explored` (percent of the map seen) at rounds 50/100/150/200/300 (buildings from 100, exploration to 150); `score` at 100, 200 and the end. Outcome metrics at the horizon: `lag` (the median rounds from an unlocked science, growth or production building or improvement to the first one built, [ai-science-diagnosis.md](ai-science-diagnosis.md); unbuilt ones count as their wait, so it is a lower bound), `score_share` (score over the mean of the game's civilizations), `placement` (0 last … 1 first: winner, then the living by score, then the dead the later out the higher), `military` (the AI's own strength-times-health measure), `survival` (alive), `victory` (kind value, 0.3 for the time victory and 1 for the others, times `1 + 0.5·(1 − round/500)` so an earlier one is worth more; 0 without one). Components after the horizon are not read: a 150-round fitness has no `@200`.
2. **Normalisation.** Each component is turned into a z-score against the reference AI: `z = (value − mean)/max(sd, min_sd)` where mean and sd are those of the reference AI's seats (every seat of 40 games of `standard` playing alone, on seeds that no evaluation uses unless asked), and `min_sd` (per metric, in the file) keeps a nearly constant metric (survival) from blowing up. Lower-is-better metrics (`lag`) change sign. Units therefore do not matter, and 0 is "a seat of the reference AI". The reference is per map size, game length and horizon, and cached.
3. **Missing values.** A dead civilization has 0 cities, techs, science and score from its death on. A game that ended before a checkpoint keeps the last sample. A seat with no value for a component (no unlock for `lag`) leaves it out; its weights are renormalised.
4. **Weights.** A preset gives a weight per *metric*; the metric's weight is split over its rounds by the round weights (the earliest checkpoints count half), so a short horizon and a long one weigh the same things in the same proportion. A seat's fitness is the weighted mean of its z-scores, minus the guards' penalties, in z units: +0.1 is a tenth of a reference seat's standard deviation. A fitness of the *candidate* is the mean over its seats; seeds are the blocks of the bootstrap.
5. **Presets** (weights per metric):

   | metric | `science` | `balanced` | `win` |
   |---|---:|---:|---:|
   | techs | 3 | 1.5 | 1 |
   | science | 3 | 1.5 | 1 |
   | science_buildings | 1.5 | 1 | – |
   | lag | 1.5 | 1 | – |
   | cities | 1 | 1.5 | 1 |
   | citizens_per_city | 1 | 1 | 0.5 |
   | explored | 0.5 | 0.5 | – |
   | score | 1 | 2 | 3 |
   | score_share | – | 0.5 | 1 |
   | placement | 0.5 | 1.5 | 3 |
   | military | 0.25 | 1 | 1.5 |
   | survival | 1 | 1 | 1.5 |
   | victory | 0.5 | 1 | 2.5 |

   `science` is what the project owner asked the AI to aim at by default (science and the economy under it, with the outcome kept as a floor); `balanced` weighs development and outcome about evenly; `win` is the outcome (score, place, victory, survival, army) with the economy as a means.
6. **Solo mode.** When every seat plays the candidate, `score_share`, `placement` and `victory` compare a seat with copies of itself: their mean is fixed, so they carry no information and their weights are 0 (the `win` preset is then mostly score and army). Use versus mode for them.
7. **Guards** (`guards` in the file; subtracted from a seat's fitness, in z): 1.0 for an eliminated civilization; 2.0 per unit its score share is below 0.5; 1.0 per unit its military share is below 0.2 ([below](#ways-to-game-it-and-the-guards)).

## Why these components

The ai-science-diagnosis found that in this ruleset science is almost a head count (about 1.5 beakers a citizen), that the AI's head count is held down by the hammers it spends elsewhere, by a happiness it does not build and by slow growth, and that a built building can wait 43–71 rounds after it was unlocked ([ai-science-diagnosis.md](ai-science-diagnosis.md)). The components follow the chain.

| component | why |
|---|---|
| `cities`, `citizens_per_city` | the head count is the root of science and of the score; both, so neither "many tiny cities" nor "one huge city" wins; the allocator's value of a new city is the biggest brake found |
| `techs`, `science` | the output of the economy and its stock; the science victory needs 75 of 81 technologies, and the AI reaches 53 by round 500 |
| `science_buildings` | the conversion of an unlocked technology into beakers (libraries and universities, 1.3 and 0.0 at round 100 in the diagnosis) |
| `lag` | how fast what is unlocked is used: the diagnosis' second finding. It also punishes hoarding unlocks |
| `explored` | contact, trade partners, site knowledge: 10 % of the map at round 100 is poor, and it is cheap to measure |
| `score`, `score_share`, `placement`, `victory` | the game's own verdict. In real games the score and the time victory decide almost every game (science, culture and diplomacy are practically out of reach, [ai.md](ai.md)); a science/domination victory is worth more because it is rarer and wins for good |
| `military`, `survival` | the cost side: a development that cannot be defended is not worth having |

Several checkpoints per metric reward the trajectory, not one lucky round, and make the short horizon a prefix of the long one.

## Ways to game it, and the guards

| way | why it would pay | guard |
|---|---|---|
| **An AI that never fights** (peaceful turtle) | development metrics reward growth and science; no army costs nothing in them | in versus mode the baseline attacks, so a defenceless candidate loses cities and score; `survival` and the elimination penalty; `military` as a component and the military-share floor (below 20 % of the game's mean, −1 z per unit); `placement` and `score_share` with the floor at 0.5. In solo mode (no attacker of another kind) the military floor is the only direct guard: **do not tune the `win` preset in solo mode** |
| **Hoarding** (gold, production, unbuilt unlocks, a huge standing army that is never used) | cheap to look good at a checkpoint | gold and hammers are not components (hoarding them earns nothing); `lag` counts unbuilt unlocks as their wait; cities, citizens, techs and science are read at several rounds, so a late spend does not hide an early stall; a standing army is capped by the score and the guards' share (military is z-scored, no unbounded reward) |
| **Score farming** (inflating the game's score at the expense of strength) | score is in three components | the score is 13 % of the weight of `balanced` (2 of 15.5) and 7 % of `science` (1 of 14.75), 3 of 16 in `win` where it is the aim; a seat whose score share is below 0.5 pays; the score is the game's own, so its parts (cities, citizens, land, wonders, techs) are the parts of a good empire |
| **Optimising the start, collapsing later** | the horizon is 150 | checkpoints up to 300 exist for horizon 300/500; the correlation study below says how well 150 predicts 500; validate every tuned genome at 500 before trusting it |
| **Overfitting the seed set** | an optimiser sees the same 20 maps every time | `--seed-offset` makes an independent set; the reference is on seeds no evaluation uses; evaluate winners on fresh offsets |
| **Metric collusion in solo** | all seats play the same AI | the relative metrics are off in solo; versus is the default |
| **Making others worse** (a candidate that cripples the baseline looks better in versus) | placement and share are relative | development components are absolute z-scores of the candidate's own seats against a fixed reference; the baseline arm's fitness is reported too (`paired.baseline_fitness`), a rise of the candidate with a drop of the baseline shows in the pair |

None of these is proved closed: the guards raise the price of each, and a trained controller will find the next one. The honest check is to look at the components of a winner (`components[]`, `--detail`), not at the scalar.

## Genome

`classic_rules::ai::genome` (dev-only, off by default). 30 numeric knobs of the heuristic AI read through `genome::get(Knob)`: with no genome the value is the constant the knob is named after (the default *is* the constant, no second copy), so a game without a genome is move for move the game of before. A mind that carries a genome (`ClassicMind::with_genome`) makes it the active one on its own thread for the span of each of its calls and puts the previous one back, so a game can seat different genomes on different seats and minds of other games are untouched. The bench names such a mind `variant@handle` (`ai-fitness --genome g.json` registers the file under a handle and plays `variant@candidate`).

Rules for a knob: a generic number the heuristic already has (a threshold, a patience, a weight, a valuation); no name of a technology, building, belief or civilization; **no price, cap or multiplier of the rules for the AI** (the rules every player plays under are untouched; `tile_price` is the AI's *estimate* of what a tile costs, not the price); a range inside which the AI still plays; an unknown key is an error, a value outside the range is clamped and an integer is rounded. A genome file is a JSON object of `knob: number`; keys starting with `_` are comments; absent knobs keep their defaults.

| knob | group | default | range | meaning |
|---|---|---:|---|---|
| `band_percent` | leaders | 10 | 0–30 | options within this percent of the best value are tied; a leader takes the one it loves |
| `switch_percent` | leaders | 130 | 100–220 | another victory path replaces the current one only when worth this percent of it |
| `review_rounds` | leaders | 10 | 4–30 | rounds between two reviews of the victory path |
| `path_yield_percent` | leaders | 150 | 100–300 | price of the chosen path's yield, percent |
| `score_power` | leaders | 4 | 1–8 | power of the scores in the chance of the time victory (a lead counts more than its share) |
| `plan_limit` | war | 40 | 10–90 | rounds a war plan waits for its treasury chest |
| `plan_keep_percent` | war | 70 | 30–100 | a war plan is dropped when its target is worth less than this percent of its value at the choice |
| `plan_switch_percent` | war | 130 | 100–250 | another war target replaces the plan only when worth this percent of it |
| `success_percent` | war | 35 | 10–80 | expected chance an operation takes its city, percent (war appetite; also in the danger estimate of peace) |
| `loss_percent` | war | 50 | 20–100 | share of the army an operation expects to lose, percent |
| `rival_percent` | war | 50 | 0–100 | share of a captured city's points that counts against the best rival, percent |
| `stranger_percent` | war | 50 | 0–100 | how hostile an untrusted stranger's army is taken to be in a danger estimate, percent |
| `max_settlers` | economy | 3 | 1–6 | most settlers the allocator keeps wanted at once |
| `jobs_per_worker` | economy | 4 | 2–8 | tile jobs one worker is counted to serve |
| `improvement_yields` | economy | 150 | 50–400 | yield a finished tile job adds, as percent of the price of a food or hammer |
| `wonder_min_chance` | economy | 30 | 0–100 | lowest chance, percent, a wonder is still counted ours however long it takes |
| `first_worker_factor` | economy | 3 | 1–6 | how much the first worker counts once a settler is on the way |
| `tile_price` | prices | 75 | 30–200 | gold a bought tile is taken to cost on average |
| `citizen_rounds` | prices | 25 | 10–60 | rounds a new city is taken to need for one more citizen |
| `opening_cities` | prices | 3 | 1–6 | cities of the opening: their citizens are valued as the empire's growth |
| `happiness_comfort` | prices | 6 | 2–12 | excess happiness at which one more point stops being worth a citizen |
| `tech_rounds` | prices | 9 | 4–20 | rounds one technology is taken to take, when valuing what research buys |
| `settle_travel_cost` | settling | 30 | 10–80 | points a settler's turn of walking costs a candidate site |
| `settle_distance_penalty` | settling | 40 | 10–80 | points per tile a site costs beyond the free distance from the empire's cities |
| `settle_free_distance` | settling | 4 | 2–7 | tiles from the nearest own city a site is free of the distance penalty |
| `branch_expansion_percent` | policies | 400 | 100–800 | the wish for expansion when free sites are in reach, percent (weighs a policy branch) |
| `branch_military_at_war` | policies | 450 | 0–900 | percent the wish for an army adds while at war (weighs a policy branch) |
| `branch_happiness_per_city` | policies | 25 | 0–100 | percent the wish for happiness grows with each city beyond the free ones |
| `branch_settler_points` | policies | 60 | 20–150 | points a free settler is worth in a policy branch |
| `branch_technology_points` | policies | 60 | 20–150 | points a free technology is worth in a policy branch |

The knobs are in `classic/crates/classic-rules/src/ai/genome.rs` (`SPECS`); `ai-fitness --list-knobs` prints the same table as JSON, so an optimiser reads the ranges from the binary. A knob only acts in the variants that use the code it feeds (the war knobs in the variants with operations, the price knobs in the variants with an allocator, the branch knobs everywhere a branch is chosen); on `standard` all 30 are live. A variant of another agent that changes the code around a knob keeps the mechanism: a new knob is one line in `SPECS` and one `genome::get` at the use.

**Proof that nothing changes without a genome.** (1) The 20 games of the `dev` suite (`ai-tournament --suite dev --solo standard`, 500 rounds, Standard-size maps) give the result hash `8cabb1c14cd8df5a` on `origin/main`'s AI before the change and after it [measured by me, 20 October]. (2) Test `tests/ai_fitness.rs`: a genome of the defaults plays the same game as no genome (equal world hash on the smoke map), a genome of three tuned knobs plays another, and after it the plain mind plays the original again. (3) The pinned hashes of `tests/ai_bench.rs` and the AI acceptance tests pass unchanged. (4) The lookup is a thread-local read; 15 interleaved smoke runs show no difference in user time (median 0.45 s before, 0.41 s after, noise ±0.1 s on a loaded machine) — a precise per-call cost was not measured.

## Cost, noise and the horizon

All numbers are from `integration/tools/ai-fitness/study.sh` and `analyze.py` (the raw results are not in the repository; the runs are cheap to repeat). Map size `small` (6 civilizations), balanced preset, the machine of 20 cores shared with about eight other agents (load average 40-80 the whole time, `--jobs 3`), 20 October 2026. **Sizes of the sets are small and said at each table.**

### Cost of one game

CPU seconds of a whole `ai-fitness` process with `--jobs 1`, 3 solo games each (6 AIs in a game), reference cached; measured with `/usr/bin/time`:

| horizon | CPU seconds a game | games an hour on one core | relative |
|---:|---:|---:|---:|
| 150 | 4.6 | 780 | 1 |
| 300 | 12.0 | 300 | 2.6 |
| 500 | 31.4 | 115 | 6.8 |
| 150, Standard-size map (8 civilizations) | 6.4 | 560 | 1.4 |

Games an hour is therefore (cores x the figure) on a quiet machine: with `--jobs 12` about 9000 / 3600 / 1400. On the shared box with `--jobs 3` the wall clock gave 713 / 286 / 97 games an hour for 12 games each (load 70-76) and up to 1060 an hour at 150 for a longer run (160 games) — the cores were not ours. A reference (40 games alone) costs one batch of games at its horizon: 90 s, 423 s and 769 s of wall clock at 150, 300 and 500 under load, and is cached. One evaluation of a candidate against a baseline at horizon 150 with 20 seeds is 40 games, about 3 CPU minutes.

### Noise of the scalar

Standard AI alone against its reference, horizon 150, 160 seeds (each seed one game; 160 games): the sd of one seed's fitness is **0.317 z**. Disjoint random sets of seeds from these 160 give:

| seeds in the set | sd of the set's mean | sd/sqrt(n) | 95 % half-width |
|---:|---:|---:|---:|
| 10 | 0.098 | 0.100 | 0.19 |
| 20 | 0.067 | 0.071 | 0.13 |
| 40 | 0.044 | 0.050 | 0.09 |
| 80 | 0.025 | 0.035 | 0.05 |

The seeds behave as independent draws, so the absolute fitness of one genome is known to +-0.13 z at 20 seeds. That is too coarse for comparisons, hence the **pair**: the candidate and the baseline play the same worlds, and what is compared is the difference inside each seed. Paired noise (sd of one seed's difference): a twin AI (`nudged`: the standard one weighing 40 candidates a decision instead of 48, the games diverge, the strength is the same), 160 seeds (320 games): **0.090 z** (mean difference -0.000, as it must be); the real genomes of the sensitivity study (40 seeds each, 12 genomes): from 0.003 (a war knob that changes nothing before round 150) to 0.208, **0.123 on average over the 8 that change the games**. Seeds to see a true effect with 95 % confidence at 80 % power (sd 0.123):

| effect to detect | seeds | games (two per seed in versus mode) | CPU time at horizon 150 |
|---:|---:|---:|---:|
| +0.05 z | 48 | 96 | 7 min |
| **+0.10 z** | **12** | **24** | **2 min** |
| +0.20 z | 3 | 6 | 30 s |

(The twin's sd, 0.090, would give 7 seeds for +0.1 z: optimistic.) Seeds, not games, are the unit; the base rate is one seed = one game in solo, two in versus.

A correction of the paired number for choosing among many genomes (an optimiser that tries hundreds takes the lucky one) is not applied: use fresh `--seed-offset`s for the winners.

### Sensitivity: one knob, +-30 %

Versus the standard AI, 40 seeds, 80 games each, horizon 150, balanced preset (95 % bootstrap intervals over seeds):

| genome | knob (default → value) | balanced diff (95 % CI) | science diff (95 % CI) | win diff (95 % CI) | diff / half-width | paired sd of a seed |
|---|---|---:|---:|---:|---:|---:|
| branch_expansion_percent-minus | branch_expansion_percent 400 → 280 | -0.018 (-0.063..+0.026) | -0.041 (-0.086..+0.004) | +0.011 (-0.036..+0.061) | -0.4 | 0.146 |
| branch_expansion_percent-plus | branch_expansion_percent 400 → 520 | +0.003 (-0.008..+0.014) | +0.003 (-0.009..+0.016) | +0.001 (-0.009..+0.010) | +0.2 | 0.035 |
| max_settlers-minus | max_settlers 3 → 2 | -0.001 (-0.024..+0.024) | +0.023 (+0.004..+0.043) | -0.006 (-0.036..+0.026) | -0.0 | 0.079 |
| max_settlers-plus | max_settlers 3 → 4 | +0.005 (-0.003..+0.013) | +0.000 (-0.007..+0.008) | +0.008 (-0.001..+0.019) | +0.6 | 0.027 |
| settle_distance_penalty-minus | settle_distance_penalty 40 → 28 | -0.022 (-0.055..+0.008) | -0.018 (-0.051..+0.010) | -0.021 (-0.056..+0.011) | -0.7 | 0.102 |
| settle_distance_penalty-plus | settle_distance_penalty 40 → 52 | +0.012 (-0.015..+0.042) | +0.013 (-0.013..+0.041) | +0.012 (-0.020..+0.048) | +0.4 | 0.093 |
| settle_travel_cost-minus | settle_travel_cost 30 → 21 | +0.002 (-0.032..+0.033) | +0.005 (-0.026..+0.034) | -0.001 (-0.039..+0.034) | +0.1 | 0.107 |
| settle_travel_cost-plus | settle_travel_cost 30 → 39 | +0.015 (-0.023..+0.053) | +0.014 (-0.022..+0.051) | +0.011 (-0.029..+0.051) | +0.4 | 0.125 |
| success_percent-minus | success_percent 35 → 24 | -0.000 (-0.002..+0.000) | +0.002 (+0.000..+0.004) | -0.002 (-0.004..-0.001) | -0.5 | 0.003 |
| success_percent-plus | success_percent 35 → 46 | -0.001 (-0.005..+0.002) | +0.000 (-0.003..+0.004) | -0.003 (-0.008..+0.000) | -0.4 | 0.012 |
| tile_price-minus | tile_price 75 → 52 | +0.011 (-0.053..+0.073) | +0.038 (-0.022..+0.096) | -0.015 (-0.086..+0.055) | +0.2 | 0.208 |
| tile_price-plus | tile_price 75 → 98 | +0.002 (-0.036..+0.041) | -0.001 (-0.039..+0.037) | -0.001 (-0.047..+0.044) | +0.1 | 0.127 |

**No single move of 30 % is visible.** All differences are within -0.022..+0.015 z and every interval contains 0 (in the `science` preset one of 36 intervals excludes 0, `max_settlers` 3 -> 2: +0.023 (+0.004..+0.043), what chance gives). Three things follow: an optimiser cannot read a gradient of one of these knobs at 20-40 seeds; the war knobs (`success_percent`) change nothing by round 150 (paired sd 0.003-0.012: nobody has attacked yet) and need a long horizon or a war-heavy suite; and the knobs do act together: all 30 knobs moved up to 30 % at random make a visible difference: 12 genomes of this kind (every knob moved by up to 30 %) differ at horizon 150 by up to 0.26 z between the best and the worst (see below), and one of them against the standard AI shows -0.128 (-0.198..-0.035) on only 3 seeds.

### Short horizon against the whole game

Played alone (solo) to round 500, fitness read off the same games at 150, 300 and 500. **13 candidates, 20 seeds each** (the standard AI with 12 genomes in which every knob is moved by a random factor in 0.7..1.3, and the `nudged` twin; 20 seeds = 20 games of 6 AIs each at 500 rounds, 260 games in all), plus the standard AI alone on **40 seeds** (the first 20 are the common ones). Raw balanced-preset fitness against the reference of the same horizon:

| candidate | fitness@150 | fitness@300 | fitness@500 |
|---|---:|---:|---:|
| all-00 | -0.003 | -0.002 | +0.005 |
| all-01 | +0.055 | +0.039 | +0.035 |
| all-02 | +0.038 | +0.035 | +0.036 |
| all-03 | +0.002 | -0.001 | +0.027 |
| all-04 | -0.063 | -0.038 | -0.034 |
| all-05 | -0.141 | -0.023 | -0.007 |
| all-06 | -0.151 | +0.035 | +0.015 |
| all-07 | -0.150 | -0.098 | -0.069 |
| all-08 | -0.127 | -0.121 | -0.085 |
| all-09 | -0.208 | -0.066 | -0.044 |
| all-10 | +0.016 | +0.026 | +0.003 |
| all-11 | -0.046 | -0.030 | -0.004 |
| nudged | -0.028 | -0.006 | -0.012 |

Rank correlation of the 13 candidates (balanced unless named; "same games" shares the noise, because round 150 is the beginning of the same game that goes on to 500):

| comparison | Spearman | Pearson |
|---|---:|---:|
| balanced: 150 vs 500, same games | +0.69 | +0.69 |
| balanced: 300 vs 500, same games | +0.95 | +0.95 |
| balanced: 150 vs 300, same games | +0.68 | +0.66 |
| science: 150 vs 500, same games | +0.63 | +0.71 |
| science: 300 vs 500, same games | +0.93 | +0.96 |
| science: 150 vs 300, same games | +0.68 | +0.73 |
| win: 150 vs 500, same games | +0.49 | +0.53 |
| win: 300 vs 500, same games | +0.86 | +0.90 |
| win: 150 vs 300, same games | +0.42 | +0.45 |
| balanced: 150 on one half of the seeds vs 500 on the other half (mean of both ways) | +0.42 | |
| balanced: 500 vs 500, halves (the ceiling: noise alone) | +0.36 | |
| balanced: 150 vs 150, halves | +0.89 | |

Per seed block, standard AI, 40 seeds: Spearman +0.93, Pearson +0.95 (fitness@150 against fitness@500 of the same games).

Pooled over all 14 candidates, candidate means removed (280 games): Spearman +0.91, Pearson +0.95.

Pooled, seed means and candidate means both removed (280 games): Spearman +0.78, Pearson +0.79 (what a candidate does differently on the same map at round 150 against round 500).

How to read it, honestly:

- **The candidates differ by little.** At 150 the spread of the means is -0.21..+0.06 z, at 500 -0.09..+0.04, against the noise of a 20-seed mean of 0.067: the genomes are a weak test of rank correlation. The 500-round ranking is hardly reproducible by itself (500 against 500 on disjoint halves of the seeds: Spearman +0.36), so **+0.4..+0.7 is about the best a 150-round fitness can show**: 150 on one half of the seeds against 500 on the other gives +0.42, the 500-against-500 ceiling is +0.36.
- **The game-level signal does carry over.** Within a map (seed and candidate means removed, 280 games) what a seat does differently at 150 and at 500 correlates +0.78 (Spearman); per seed for the standard AI (40 games) +0.93. Maps decide most of a game's fitness, so the second number is mostly the map.
- **Horizon 300 predicts 500 very well (+0.95 on the same games) at 2.6 times the cost of 150 and 0.38 of the cost of 500;** the `win` preset needs it most (150 against 500: +0.49, 300 against 500: +0.86, because victories and placement are late).
- **A short horizon prefers fast starters.** Five genomes (`all-05`, `-06`, `-07`, `-09`, `-08`) are at -0.13..-0.21 at 150 and -0.01..-0.09 at 500: they start slower and recover part or all of it (`all-06`: -0.151, +0.035 at 300, +0.015 at 500; `all-08` stays low). A tuning at 150 undervalues such a genome. Practical use: tune at 150, **confirm every finalist at 300 or 500** (the "optimising the start" row of the table of ways to game the metric).
- The `science` preset is the best predicted at 300 (+0.93) and fair at 150 (+0.63): development metrics are early; outcome metrics (`win`) are late.

## Mac and external optimisers

The same code runs on macOS (Apple Silicon): no Linux-only code (the load average is read from `/proc/loadavg` when the file exists and is left out otherwise), no GPU, no window, no assets beyond what the crate already compiles. Commands (the machine has 14–16 cores; keep a couple free, `--jobs 12` or so):

```sh
git fetch origin ai/fitness && git checkout ai/fitness
rustup toolchain install 1.98.1
cargo +1.98.1 build --release -p aurelia-qa --bin ai-fitness      # assets/visual is needed by the crate; see the asset rules
target/release/ai-fitness --list-knobs > knobs.json
# once: the reference (about 40 games of the standard AI); later calls reuse the cache
target/release/ai-fitness --genome g0.json --seeds 1 --horizon 150 --jobs 12 --out /dev/null
# every evaluation
target/release/ai-fitness --genome g.json --suite fit --size small --seeds 20 --horizon 150 --jobs 12 --out r.json
```

**Black-box contract.** Input: a JSON object `{knob: number}` (missing knobs keep their defaults, values are rounded and clamped, unknown knobs are an error). Output: `r.json`, whose `fitness` (float, higher is better, in z units) is the objective and whose `ci` gives its noise; `paired.diff` and `paired.ci` compare with the baseline on the same games. Exit code 0 on success, 1 with a message on stderr otherwise. The call is deterministic: the same genome, options and `--seed-offset` give the same number, so an optimiser that wants fresh data per generation changes `--seed-offset`, one that wants common random numbers keeps it. `integration/tools/ai-fitness/example.py` (Python standard library only) and `example.sh` do exactly this call; there is no optimiser and no training in the repository. For parallel evaluations give each process its own `--jobs` and the same `--cache-dir` after the reference exists (the first call writes it; do not start many first calls at once).

## What is not verified

- **Never run on macOS / Apple Silicon**; only Linux x86-64 (i9-10900K class) was used. The commands there are the same by construction (no Linux-only code except the optional load average), not tested.
- **Samples are small**: the rank correlations of the horizon study rest on 13 candidates of 20 seeds (and 40 for the standard AI), the noise numbers on 160 seeds of one AI and a twin, the sensitivity on 12 genomes of 40 seeds, the throughput on 3-12 games per horizon. Spearman values have wide intervals (not computed) and the 500-round ranking is itself noisy (+0.36 against itself). Only map size `small` was studied, except one Standard-size throughput run.
- **Nothing was optimised**: whether a tuned genome really plays better (at 500 rounds, on fresh seeds, in the real game against humans) is unknown; no learning prototype exists. The gaming guards are reasoned, not tested against an adversarial controller.
- **Weights and presets are my choice**, not validated by the project owner or by play; `science` is not shown to be what he wants to maximise. The `lag` component is a lower bound and exists only at the end of a game; the reference (40 seeds, 6 seats each) has its own sampling error (the standard AI scores -0.07 .. -0.12 against it on other seeds), so only numbers made with one reference are comparable.
- **Per-call cost of the genome lookup** in the AI was not measured precisely (15 interleaved smoke runs show no difference above ±0.1 s); the "nothing changes without a genome" proof is the equal result hash of the 20-game `dev` suite before and after the mechanism (measured before the rename; after the rename and the merge of `main` the pinned hashes of `aurelia-qa` and `classic-rules` tests pass and `ai-fitness` gives the same result hash as before the rename, but the 20-game suite was not repeated).
- **Wall-clock rates** are those of a machine shared with other agents (load 40-80 on 20 cores); only the CPU seconds per game are comparable between machines, and they come from 3 games per horizon.
- `cargo fmt --check` is red on the whole tree (it was before this branch; the base is not rustfmt-formatted), so formatting of the new files was not enforced.
- The war, operation and peace knobs show no effect at horizon 150; whether they matter, and whether the war-heavy part of the metric (`military`, `survival`, `placement`) sees them, was not studied.
