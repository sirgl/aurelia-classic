# Why the AI gains science so slowly (diagnosis, 20 October 2026)

**In this ruleset science is almost a head count (about 1.5 beakers a citizen); the AI's head count is held down by the hammers it spends elsewhere, by a happiness supply it does not build, and by slow growth, and from round ~75 its allocator values a new city below zero. The research order, the sites of the cities and the exploration are not what limits it.** Diagnosis only: nothing in the AI was changed; every number below is measured by the probe described in «Method» unless it is marked otherwise. Branch `ai/science-diagnosis`.

Contents: [Answer](#answer) · [Method](#method) · [Where the beakers come from](#where-the-beakers-come-from) · [Findings, ranked](#findings-ranked) · [1. Research order](#1-research-order) · [2. Cities and growth](#2-city-placement-and-growth) · [3. Tradition](#3-the-policy-opening-tradition) · [4. Beakers and the binding constraints](#4-the-binding-constraints) · [5. Exploration](#5-exploration) · [6. Permanent metrics](#6-permanent-metrics) · [Not measured](#what-is-not-measured) · [Reproduce](#reproduce)

## Answer

| The project owner's question | Answer | Evidence |
|---|---|---|
| Why is science slow? | Beakers = citizens × 1 plus small building terms: of the 22 beakers at round 100, 66 % are citizens, 14 % the palace, 17 % libraries; trade, policies, great people, specialists and tiles are about zero. The AI has 14.6 citizens at round 100 and 29 at round 200 (the 90th-percentile seat: 20 and 45). None of the 20 baseline games and none of 150 Prince games with an intervention ended by science; 1 of 10 at Emperor did (round 456). | [Where the beakers come from](#where-the-beakers-come-from) |
| What are the constraints, by what they cost? | Measured by changing one cost in the rules for the AI and replaying the same 20 (10) seeds: production 20-25 % cheaper gives +25 % science at round 200 and +39 % at 300; happiness +8 gives +23 % and +24 %; growth 20 % cheaper +11 % and +19 %. The AI's own choices that sit behind them: hammers go to wonders (26 % of the queue heads at round 100), soldiers and walls while the first library comes 42 rounds after Writing; 1.7 buildable luxury tiles stand unimproved per seat at round 100 (4 happiness each); from round 75 a settler is worth less than nothing to the allocator. | [Constraint tests](#constraint-tests), [4](#4-the-binding-constraints) |
| Are the researched techs optimal? | Not demonstrably worse than anything derivable from the effects: three reference orders (greedy by beaker yield per path cost, economy first, economy plus luxuries) are worse than or equal to the AI's own: -40 %, -8 % (not significant) and ±0 % of the science at round 200, -34 %, -12 % and -8 % at round 300. | [1](#1-research-order) |
| Are cities placed correctly? | Mostly yes: 92 % of the chosen sites are in the top 40 by the true score, the median chosen site is rank 4 and 48 points (14 % of the mean chosen score of 561) below the best, 79 on average; map knowledge hid a better site in 43 % of the foundings (35 points, 6 %). The brake on the number of cities is not the site: it is the allocator's value of a settler (picked in 81 % of the decisions at rounds 0-24, 35 % at 50-74, 5 % at 75-99, 0 % from 125). | [2](#2-city-placement-and-growth) |
| Do cities grow fast? | No: capital size 3.6 at round 50 and 6.1 at 100; other cities 3.4 at age 50-74; food surplus +2.9 / +3.5 in the capital (a constant +4 would reach size 8 in 68 rounds, +2 in 136). At round 50, 58 % of the cities are food-bound; at 100, 44 % sit in an unhappy empire (growth -93 %). | [2](#2-city-placement-and-growth) |
| Tradition opening | The AI takes Liberty first in 156 of 160 seats (round 16). This is an experiment on the price of the branch, not advice to always take Tradition: the right branch depends on the place, the neighbours and the civilization (the project owner). Forced Tradition first on every seat (same maps, same seats) costs early (second city 4 rounds later, 0.7 cities fewer and science -2 at round 100) and pays later: 7.0 policies by round 100 against 4.7, unhappy rounds -10 points, science +4.4 at round 200 and +20 (+19 %) at 300, +2.4 techs at 500. | [3](#3-the-policy-opening-tradition) |
| Exploration | Slow and poor: no scout unit before round 47 (median), 4 % of the map known at round 25, 10 % at 100, 13 % at 150, the own continent 65 % at round 100; the first civilization is met at round 43 (median). The simple rule "walk to the nearest unknown tile" knows 13 % more of the map by round 150 than the allocation of `ai::scouting` (13.6 against 12.0 %). It costs site choice little (35 points) and a trading partner for luxuries a lot more. | [5](#5-exploration) |
| Permanent metrics | Done: the `dev` suite and the development table in the bench; one command, 20 games, 3-4 minutes on a quiet machine (9 minutes at load 40); new in the second round: the implementation lag of unlocked buildings and improvements. | [6](#6-permanent-metrics), `platform/docs/ai-bench.md` |

## Method

**Probe.** `integration/crates/aurelia-qa/examples/ai_diag.rs` plays one AI-only game (the game's own AI, `Variant::default()`, on every seat) and prints one JSON object: per round and per major player the series (techs, science, cities, citizens, libraries, happiness excess, strategy, settlers alive and queued, sites on offer as the player knows the map and as the map is, explored share of the map and of the land, scouts, workers, units, gold), the events (cities with their site, settlers with the rounds they appear and found, techs and buildings and policies with rounds, contacts, ruins, lost scouts), and per-city snapshots at rounds 25, 50, 75, 100, 125, 150, 200, 250, 300 with food, hammers, queue, buildings, the science broken down by source, the happiness report and the luxuries. `ai_diag_batch.sh` runs a seed set in parallel, `ai_diag_analyze.py` makes the tables of this document, `ai_diag_compare.py` the paired comparisons. It reads the world after each round and never writes to it.

**Interventions.** A wrapper mind (`OrderedMind`) around `ClassicMind` delegates everything; it replaces exactly one decision, and only when an option asks for it: `--order greedy|econ|happy` the technology of a `ChooseTechnology` (the AI still decides when to research), `--first-branch NAME` the first `UnlockPolicyBranch`, `--scouts greedy` the decision of every recon unit. Three more options change the rules the AI plays under, to test a constraint: `--extra-happiness N` raises the AI base happiness, `--patch growth,production,unhappiness` gives the game level the Emperor numbers of these groups (growth cost 80 % and -3 % per era; training 80 %, construction 75 %, creation 80 %, building cost 80 %, unit cost 75 %; AI unhappiness 90 %; the three together reproduce the Emperor game exactly), and `--alloc 1` records the allocator's candidate lists (`ai::allocator::probe`, which never feeds a decision). No file of `classic/crates/classic-rules` or `platform/crates/kernel` was touched: the probe, the wrapper and the scripts live under `integration/crates/aurelia-qa`.

**Seeds and maps.** Seeds 1-20, standard-size maps (80 × 52 = 4160 tiles, 8 civilizations, 16 city-states), the map script cycles with the seed: pangaea 1, 6, 11, 16 · fractal 2, 7, 12, 17 · archipelago 3, 8, 13, 18 · small-continents 4, 9, 14, 19 · continents 5, 10, 15, 20. Prince, 500 rounds (the time victory), every victory on; all 20 baseline games ended by the time victory (no science launch, no other victory; the same for 150 Prince games with an intervention; the one exception is an Emperor game). Paired runs share seed, script and seat (civilization): a seat is compared with itself. The unit of resampling is the seed block (the 8 seats of one game, 20 blocks); intervals are 95 % percentile bootstrap, 10 000 resamples, fixed generator, as in the bench.

**Machine.** The runs shared 20 cores with about eight other agents (load average 38-52). Game results do not depend on it (the games are deterministic); the wall times in this document do.

**Reference orders (derivation).** For a technology *t* the value a round of what it opens is `V(t)` = the sum over what *t* itself and the buildings and improvements it unlocks add, evaluated from the declared effects of the ruleset (`AddYield`, `AddYieldPercent` on the empire's base per city, `AddYieldPerPopulation`, `AddHappiness`; a conditional effect counts half; a city effect counts in every city, a wonder at half), minus a building's upkeep. A target technology is scored by the value of its whole unresearched path over the path's cost, `sum V / sum cost`; the player researches the cheapest researchable technology on the path of the best target (no target of value: the cheapest researchable). The empire is the player's real one at each choice (cities, mean citizens, beakers, gold and hammers a city). *Greedy by beaker yield* counts science only. *Economy first* counts food, hammers, gold and beakers at 1 each, culture 0.5, happiness 1.5. *Economy + luxuries* (`happy`) adds, for an improvement that connects a luxury resource, half of that luxury's happiness. The first technologies each order picks for a fixed empire (4 cities of 4 citizens, 22 beakers) are printed by `AI_DIAG_ORDERS=1 ai_diag`.

**Definitions.** *Unhappy* = the empire's excess happiness at the start of the player's turn is below 0 (growth -75 %, production and gold -2 % a point); *strategy* = the label of `ai::state::Strategy` (Expand, Stabilize, ...), which the allocator variant does not use to order settlers (section 2); *Library/University* include the civilizations' own replacements. Technologies count the starting one.

## Where the beakers come from

Mean per living seat, beakers a round, 160 seats (20 games); the breakdown is computed from the declared effects of the buildings in each city and the city's yield stages, the rest is the remainder.

| round | total | citizens (1 each) | palace | wonders | libraries etc. | percent stage | tiles + specialists | rest (trade, policies, religion) | per citizen | cities with a Library % | beakers per city |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 50 | 9.7 | 6.3 | 3.0 | 0.0 | 0.3 | 0.0 | 0.0 | 0.0 | 1.53 | 6 | 4.4 |
| 100 | 22.0 | 14.6 | 3.0 | 0.4 | 3.8 | 0.1 | 0.0 | 0.1 | 1.51 | 35 | 5.3 |
| 150 | 37.9 | 22.8 | 3.0 | 0.4 | 9.8 | 2.0 | 0.0 | 0.0 | 1.67 | 64 | 8.6 |
| 200 | 56.1 | 29.3 | 3.0 | 0.4 | 15.1 | 8.0 | 0.6 | -0.3 | 1.91 | 78 | 12.6 |

Trade routes, policies, great people and worked tiles contribute almost nothing: the AI's science is its citizens, the palace and (from round 120) libraries. The spaceship chain needs 75 of the 81 technologies, 168 370 base beakers [measured: the sum of the content costs of the closure of the seven spaceship technologies]; the AI knows 53.1 technologies on average at round 500 (median 55, best seat 71).

**Seat averages (160 seats)**

| | round 50 | 100 | 150 | 200 | 300 | 500 |
|---|---:|---:|---:|---:|---:|---:|
| cities | 2.2 | 4.1 | 4.4 | 4.4 | 5.7 | 12.1 |
| citizens | 6.3 | 14.6 | 22.8 | 29.3 | 45.0 | |
| citizens per city | 3.0 | 3.8 | 5.4 | | | |
| technologies | | 16.1 | | 27.7 | 36.0 | 53.1 |
| science a round | 9.7 | 22.0 | 37.9 | 56.1 | 107.4 | |
| libraries | | 1.3 | | 3.4 | | |
| universities | | 0.0 | | 0.3 | | |

By map script the picture is the same (science at round 200: pangaea 60, fractal 53, archipelago 38, small continents 63, continents 66; archipelago is worst, 4.9 % of the map known at round 100).

## Constraint tests

One cost changed at a time in the rules the AI plays under (the AI itself unchanged), the same seeds and seats, mean difference to the AI as it is, in beakers a round or in the unit of the column; `*` = the 95 % interval over seed blocks excludes 0, % = of the AI's own mean.

| change (paired with the AI as it is) | seeds | science at 100 | science at 200 | science at 300 | citizens at 200 | techs at 500 | cities at 200 | unhappy rounds 51-150, points |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| happiness +4 | 20 | +1.7* (+8 %) | +6.8* (+12 %) | +17.3* (+16 %) | +3.7* (+13 %) | +2.0* (+4 %) | +0.2* (+4 %) | -12.3* |
| happiness +8 | 20 | +2.9* (+13 %) | +12.7* (+23 %) | +26.1* (+24 %) | +6.6* (+22 %) | +3.6* (+7 %) | +0.2* (+5 %) | -17.9* |
| growth cost -20 % | 10 | +0.9* (+4 %) | +5.7* (+11 %) | +19.3* (+19 %) | +1.5 (+5 %) | +1.5 (+3 %) | -0.0 (-0 %) | +5.3* |
| production cost -20..25 % | 10 | +0.9 (+4 %) | +13.2* (+25 %) | +40.3* (+39 %) | +3.2* (+11 %) | +5.8* (+11 %) | +0.0 (+0 %) | -1.3 |
| AI unhappiness x0.9 | 10 | +1.2* (+5 %) | +8.6* (+16 %) | +18.2* (+18 %) | +4.0* (+14 %) | +2.5* (+5 %) | +0.2* (+4 %) | -7.4* |
| Emperor level (all three) | 10 | +5.5* (+25 %) | +35.6* (+66 %) | +104.7* (+101 %) | +11.7* (+41 %) | +11.7* (+22 %) | +0.2* (+6 %) | -5.3 |
| Tradition first | 20 | -1.9* (-9 %) | +4.4* (+8 %) | +20.1* (+19 %) | +3.2* (+11 %) | +2.4* (+4 %) | +0.1 (+3 %) | -10.0* |

* = the 95 % interval over seed blocks excludes 0

The Emperor level is the bundle of the three cost groups; its gain (+101 % at round 300) is larger than the sum of the single groups (they compound: more hammers build the libraries and the settlers that more food and happiness then fill). The order of the single levers by science at round 300: production cost +39 %, happiness +8 +24 %, growth cost +19 %, the Tradition opening +19 %, AI unhappiness ×0.9 +18 %, happiness +4 +16 %; at round 200: production +25 %, happiness +8 +23 %, unhappiness ×0.9 +16 %, happiness +4 +12 %, growth +11 %, Tradition +8 %.

## Findings, ranked

Ranked by the science they hold back on the mean seat, using the constraint tests as the yardstick; the numbers are not additive.

| # | Finding | Evidence | Generic fix (not implemented) |
|---|---|---|---|
| 1 | **The hammers go to things that pay late for the beaker engine.** Production 20-25 % cheaper = +25 % science at 200, +39 % at 300, +5.8 techs at 500. Heads of the city queues: wonders 10 % at round 50, 26 % at 100, 31 % at 150; soldiers 26 % → 14 % → 3 %; walls 10 % at 100; library 18 % → 11 % → 7 %. The first library comes 42 rounds after Writing (median), the second 91; 68 % of the cities have none at round 100; the University (Education) at round 165 and 0.3 per seat at 200. A settler is 106 hammers at 5-8 hammers a round. Implementation lag over the dev suite: a building or improvement that adds production waits a median of 71 rounds after its unlock (p90 414), growth 66 (420), science 49 (232), happiness 50 (362), gold 43 (341); at round 200 the cities that have what was open to them: science 49 %, happiness 41 %, production 36 %, gold 31 %, growth 22 %. | [4](#4-the-binding-constraints) | Price a hammer by what the thing built returns in the yield the aim needs (beakers/citizens), not in score points: workers, libraries, happiness sources and settlers before wonders and walls unless a threat is on the map; the wonders the AI takes first are the ones its cities can finish soonest. |
| 2 | **The happiness supply is not built.** An unhappy empire grows 93 % slower (0.0031 against 0.0467 citizens per city per round); the AI is unhappy in 26 % of the rounds 30-250; the most unhappy quartile makes 32 beakers at round 200 against 68 in the least unhappy (-52 %). Happiness +4 = +12 %/+16 % science (round 200/300), +8 = +23 %/+24 %. Content empires have 3.0 luxury kinds counted at round 100 against 1.4 for unhappy ones; both own about as many in their borders (3.8 and 2.8); per seat 1.7 buildable luxury tiles stand unimproved (4 happiness each = 6.6 points, the size of the +4..+8 tests). | [4](#4-the-binding-constraints) | Plan happiness as the budget the rules compute (3 per city, 1 per citizen against base 15 + 4 per luxury kind + buildings); put the worker on the luxury tile and the trade of a luxury copy ahead of the next building when the excess is below the margin; price a point at the growth it releases. |
| 3 | **Expansion stops at round ~75 because a settler is worth less than nothing to the allocator.** `Prices::new_city` charges the unhappiness of the city (3 + the citizens it will have) at the empire's price of happiness; the settler's value goes from +14 000 centi-points at rounds 0-24 to -17 000 at 75-99 and -22 000 at 150-174 (+16 000...+20 000 without the price), it would win 75-80 % of the decisions at 3-5 times the best alternative a turn if the price were 0, and is picked in 81 / 32 / 35 / 5 / 2 / 0 % of the decisions in the windows 0-24 / 25-49 / 50-74 / 75-99 / 100-124 / 125-149. Cities: 4.1 at round 100, 4.4 at 200, 12 at 500. | [2](#2-city-placement-and-growth) | Not a lowering of the price: wide play was measured worse under today's happiness (earlier wide variants: -38 % [from a doc, not rechecked]); couple it to finding 2: a new city is cheap exactly when a luxury or a happiness building is on the way, so price the city against the supply the empire is building, not against the supply it has. |
| 4 | **Growth is slow.** Growth cost -20 % = +11 % / +19 % science. Capital surplus +2.9 at round 50, +3.5 at 100, +4.8 at 150 (a constant +4 reaches size 8 in 68 rounds; the AI capital is size 6.1 at 100, 8.2 at 150); other cities +2; 58 % of the cities food-bound at round 50; 0.5 workers a city at round 100, 46 % of the worked tiles improved. | [2](#2-city-placement-and-growth) | One worker a city while improvable tiles are worked; food and luxury tiles first; do not run a settler out of a capital that is not yet size 4. |
| 5 | **The first policy branch is priced by a static sum.** Liberty at round 16 in 98 % of the seats. Forcing Tradition on every seat (an experiment on what the branch costs and returns, not a recommendation: the best branch depends on the place, the neighbours and the civilization) is better in this sample from round ~180. +8 % / +19 % science (200 / 300), +2.4 techs at 500, at the price of -9 % science at round 100. | [3](#3-the-policy-opening-tradition) | Value culture by the policies it buys and the capital's growth and happiness by what the budget lacks, per seat (room for cities, rivals near, the land's food, the leader), so that Tradition wins where it pays and Liberty where it does; today a static sum with expansion at 400 %. |
| 6 | **Exploration is late and the target rule is not better than "nearest unknown".** First scout unit at round 47 (median); 13 % of the map at round 150; the nearest-unknown reference knows 13 % more (13.6 %). Effect on science not measured; on site choice 6 %. | [5](#5-exploration) | A scout from round ~10 (it is the contact that brings luxury trades and city-states); a plain nearest-unknown rule as the base of the allocation. |
| 7 | **Research order.** Not a cost: the three reference orders are worse or equal. | [1](#1-research-order) | Keep. |

## 1. Research order

**Question:** are the technologies the AI researches really optimal?

**Audit of the AI's own order** (the 160 seats). The AI researches in a nearly fixed sequence (median rounds: Agriculture 1, Pottery 9, Writing 19, Mining 25, Bronze Working 35, Calendar 43, Masonry 47, Sailing 55, Optics 63, Animal Husbandry 66, Archery 70, The Wheel 73, Trapping 79, Mathematics 90, Construction 97, Philosophy 100, Currency 110, Drama and Poetry 117, Theology 128, Civil Service 142, Education 165, Astronomy 346). Against a straight line to each science-building technology with the beakers the seat really made:

| tech (unlocks) | learned, median round | straight line, median round | delay | beakers on other techs before it | share of the beakers spent so far |
|---|---:|---:|---:|---:|---:|
| Writing (Library) | 19 | 24 | -4 | 0 | 0 % |
| Philosophy (National College) | 100 | 54 | 48 | 780 | 70 % |
| Education (University) | 165 | 140 | 33 | 1290 | 38 % |
| Astronomy (Observatory) | 346 | 171 | 167 | 11260 | 77 % |
| Scientific Theory (Public School) | 342 | 282 | 57 | 5310 | 33 % |

A "delay" is an upper bound of what a straight line could win, not a gain: the techs skipped on the way are what the AI's economy stands on (next paragraph).

**Paired experiment** (same maps, same seats, 20 seeds × 8 seats, 500 rounds; mean difference to the AI's own order, 95 % CI over seed blocks):

| metric | AI mean | greedy (diff, CI) | econ (diff, CI) | happy (diff, CI) |
|---|---:|---:|---:|---:|
| techs@100 | 16.1 | -3.9* (-4.1..-3.7) | -2.3* (-2.5..-2.1) | -1.3* (-1.4..-1.1) |
| techs@200 | 27.7 | -6.2* (-6.8..-5.6) | -3.7* (-4.3..-3.1) | -2.5* (-2.9..-2.0) |
| techs@300 | 36.0 | -4.4* (-5.3..-3.6) | -1.6* (-2.5..-0.8) | -0.9* (-1.5..-0.3) |
| techs@500 | 53.1 | -6.1* (-7.5..-4.7) | -2.4* (-3.7..-1.1) | -1.8* (-3.0..-0.5) |
| science@100 | 22.0 | -3.1* (-3.8..-2.3) | -1.7* (-2.6..-0.8) | +0.3 (-0.4..+1.0) |
| science@200 | 56.1 | -22.2* (-26.7..-17.2) | -4.2 (-8.9..+0.5) | +0.3 (-3.0..+3.7) |
| science@300 | 107.4 | -36.9* (-45.4..-27.8) | -12.9* (-21.4..-4.6) | -8.7* (-14.9..-2.0) |
| citizens@100 | 14.6 | -2.4* (-2.8..-1.9) | -1.3* (-1.7..-0.9) | +0.3 (-0.1..+0.8) |
| citizens@200 | 29.3 | -11.3* (-13.0..-9.6) | -2.9* (-4.5..-1.1) | -1.0 (-2.3..+0.5) |
| cities@100 | 4.1 | -0.2 (-0.4..+0.1) | -0.2 (-0.4..+0.1) | -0.1 (-0.4..+0.2) |
| unhappy share 51-150 % | 26.1 | +22.8* (+16.5..+28.5) | +5.9* (+1.4..+10.6) | +4.2* (+0.7..+7.8) |
| Philosophy round | 99.2 | -46.8* (-49.9..-43.6) | -30.2* (-33.2..-27.3) | -29.9* (-33.1..-26.8) |
| Education round | 179.5 | -16.1* (-22.7..-9.3) | +12.2* (+4.8..+19.3) | +8.2* (+2.7..+13.6) |
| first Library round | 80.4 | +8.1* (+0.7..+15.2) | +2.6 (-3.4..+9.0) | +13.6* (+9.4..+18.0) |
| libraries@200 | 3.4 | -0.8* (-1.1..-0.6) | -0.2 (-0.4..+0.1) | -0.0 (-0.3..+0.3) |

* = the 95 % interval over seed blocks excludes 0

What the references did to the order (median round of the technology, AI → reference): *greedy* Philosophy 100 → 52, Currency 110 → 95, Calendar 43 → 29 and Mining 25 → 209, Bronze Working 34 → 211, Masonry 47 → 213, Sailing 55 → 159, Construction 97 → 218; *economy first* Animal Husbandry 66 → 26, Archery 70 → 32, The Wheel 73 → 40 and Mining 25 → 77, Bronze Working 34 → 223, Sailing 55 → 222, Optics 63 → 226. Both references value what a technology *unlocks* by the yields they can see in the effects and miss what it *reveals and connects*: mines on hills (hammers), luxury and strategic tiles, harbours; the loss shows in citizens (-39 % at round 200 for greedy) and in the unhappy rounds (+23 points).

**Answer.** Not materially suboptimal against the three orders derived from the effects; the cost of a bad order is high (-40 % science at round 200), the gain over the present one is not demonstrated: the third reference, which also values the improvements that connect luxuries, only equals the AI's science at rounds 100 and 200 and is behind at 300. This agrees with the earlier roadmap experiment (-0.22 techs): the order moves the economy through the techs that unlock tile improvements and happiness, not through the science buildings. The choices that cost most in the references are the late Mining, Bronze Working, Masonry and Sailing.

## 2. City placement and growth

**Sites.** Cities founded per seat: 2nd city median round 30 (mean 33.5), 3rd 61, 4th 77. The capital is the start position (coastal in 76 %). Site quality at founding (nature yields, unimproved, radius 3):

| city | n | ring 1 food | ring 1 hammers | ring 2 food | ring 3 food | coastal % | river % | hills in r3 | luxuries in r3 | strategics in r3 |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| capital | 160 | 8.3 | 2.3 | 14.4 | 20.1 | 76 | 4 | 2.3 | 1.97 | 1.39 |
| 2nd | 159 | 8.3 | 2.9 | 15.6 | 22.2 | 28 | 6 | 3.4 | 2.43 | 1.90 |
| 3rd | 155 | 7.7 | 3.4 | 15.2 | 21.8 | 28 | 6 | 3.2 | 2.47 | 2.02 |
| 4th and later | 1454 | 6.9 | 3.1 | 13.6 | 20.0 | 31 | 6 | 3.9 | 2.28 | 2.06 |

For 1768 non-capital foundings the probe recorded the offer before the round (all legal sites within 9 tiles of an own city, scored by the AI's own site function, `planner_settle::SiteContext::score`, once as the player knows the map and once as the map is): the chosen tile is in the top 40 of the true ranking in 92 %, is the best in 23 %, rank 4 (median); the best true site beats the chosen one by 79 points on average (median 48; the chosen sites score 561 on average, p10 274, p90 857), the best *known* site beats it by 50. A better site existed beyond the explored map in 43 % of the foundings (by more than 20 points), by 35 points on average: **map knowledge limits site choice by about 6 % of the score, the AI's own preferences (distance to its cities, travel time) by about 8 % more.** (The yardstick is the AI's own score: a site function that valued coast, rivers and resources differently could rank them differently; this was not tested.)

**Delay to settle and settlers.** A settler lives 3 rounds from appearing to founding (median; 12 at p90). The first settler appears at round 26 (median), 2 rounds before the second city. Settler cost is 106 hammers, 5-8 hammers a round in the capital: the production and the allocator's value of the settler decide, not the walk. Per seat 11.85 settlers were built over 500 rounds and 11.05 cities founded.

**Why the number of cities stops.** In the game's AI (`standard`, the allocator) the strategy label does not order settlers: settlers alive or queued in 21 % of the Expand rounds and 17 % of the Stabilize rounds (rounds 30-250, two or more cities); with the empire's excess ≥ 4 the share is 24 %, between 0 and 4 18 %, unhappy 13 %. What orders them is the allocator's value of a settler against the other orders of the city (`allocator::candidates`, `Prices::new_city`). The probe recorded every city decision in which a settler was among the candidates (20 seeds, rounds 0-250, 500-round horizon):

| rounds | city decisions | settler on offer | settler picked | would win with the price 0 | settler value / best other, a turn | the same, price 0 | settler value, centi-points (price 0) |
|---|---:|---:|---:|---:|---:|---:|---:|
| 0-24 | 232 | 180 (78 %) | 146 (81 %) | 145 (81 %) | 2.36 | 2.49 | 14920 (15559) |
| 25-49 | 290 | 217 (75 %) | 69 (32 %) | 81 (37 %) | 0.98 | 1.55 | 13779 (20446) |
| 50-74 | 585 | 403 (69 %) | 142 (35 %) | 304 (75 %) | 0.15 | 4.05 | 2083 (20547) |
| 75-99 | 750 | 567 (76 %) | 29 (5 %) | 437 (77 %) | -3.69 | 4.49 | -17275 (20161) |
| 100-124 | 1328 | 945 (71 %) | 17 (2 %) | 752 (80 %) | -4.05 | 4.60 | -16275 (18093) |
| 125-149 | 1340 | 1032 (77 %) | 4 (0 %) | 828 (80 %) | -4.89 | 4.40 | -18984 (16464) |
| 150-174 | 1420 | 1110 (78 %) | 1 (0 %) | 875 (79 %) | -6.30 | 4.52 | -22598 (15415) |
| 175-199 | 1929 | 1559 (81 %) | 4 (0 %) | 1096 (70 %) | -8.07 | 4.85 | -22725 (13771) |
| 200-224 | 2538 | 2037 (80 %) | 29 (1 %) | 1351 (66 %) | -10.42 | 6.39 | -22031 (12441) |
| 225-249 | 2925 | 2285 (78 %) | 40 (2 %) | 1342 (59 %) | -12.29 | 8.01 | -21095 (11469) |
| 250-274 | 130 | 101 (78 %) | 0 (0 %) | 68 (67 %) | -12.76 | 7.67 | -18661 (10820) |

The value of a new city is its city points, land, citizens and young yields minus the unhappiness of the city (3) and of the citizens it will have, at the empire's price of happiness (`Prices::happiness`: the outlook's growth held by a point, up to 10 citizens a point near the very-unhappy line, `CRISIS_CITIZENS`). From round ~75 that price makes it negative: the settler would win three quarters of the decisions at 4-5 times the best alternative a turn if the price were 0, and is picked in about 2 % (5 % at 75-99, 0 % from 125). When it loses it loses to buildings other than the library (64-93 % of the lost decisions from round 100), wonders (48 % at rounds 50-74, 3-20 % from round 100), and a few units. The empire's excess is +7.2 at round 50, +2.1 at 100, +3.2 at 150: at the margin the price is high. Founding resumes only after round 300 (cities 5.7 at 300, 12.1 at 500), when buildings and policies have raised the budget. This is the consistent consequence of pricing happiness in score points; it is not a defect of the model by itself (earlier wide variants lost science [from a doc, not rechecked]), but it means that the number of cities is a function of the happiness supply (finding 2), not of sites or of a gate. The old gate (`planner_strategy::happiness_allows_city`, excess ≥ 4 for a third city) and the label "Stabilize" (49 % of the rounds 30-250, 92 % of them with excess below 4) shape the strategy label and the planner variants, not the order of settlers in `standard`.

**Population curves against a reference.** Citizens by the age of the city (capital / other cities; mean and p90):

| age (rounds) | capital: n | capital mean | capital p90 | other cities: n | mean | p90 |
|---|---:|---:|---:|---:|---:|---:|
| 0-24 | 160 | 2.2 | 3 | 673 | 1.6 | 3 |
| 25-49 | 160 | 3.6 | 6 | 612 | 2.6 | 4 |
| 50-74 | 160 | 5.0 | 7 | 557 | 3.4 | 6 |
| 75-99 | 160 | 6.1 | 8 | 412 | 4.2 | 7 |
| 100-124 | 160 | 7.1 | 10 | 354 | 5.2 | 9 |
| 125-149 | 160 | 8.2 | 12 | 210 | 5.6 | 10 |
| 150-174 | 0 | – | – | 337 | 6.5 | 11 |
| 175-199 | 160 | 9.8 | 14 | 209 | 7.0 | 13 |
| 200+ | 320 | 12.2 | 19 | 698 | 8.8 | 15 |

and the reference derived from the rules (growth thresholds 15, 22, 29, 38, 47, 56, 65, 75 food for the sizes 1 to 2, 2 to 3, ...; the rounds a city needs for a size at a constant food surplus after eating): +2: size 5 in 52, size 8 in 136; +3: 35, 91; +4: 26, 68; +6: 17, 45; +8: 13, 34. The AI capital's surplus is +2.9 at round 50 and +4.8 at round 150, the other cities' about +2, the same lines. A human yardstick for comparison is unverified: BNW players run +4 to +8 in the capital early [from memory, not verified].

**The binding constraint per city.** Every city at a checkpoint, first matching rule: a Settler at the head of the queue (the food goes to hammers); the empire unhappy; food surplus ≤ 0; surplus 1-2; otherwise growing:

| round | cities | settler % | happiness % | starving % | low food (+1..+2) % | growing % | mean pop | mean surplus | mean turns to grow | mean prod | cities w/o Library % |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 50 | 355 | 14 | 7 | 11 | 33 | 35 | 2.9 | 2.2 | 8 | 5.7 | 95 |
| 100 | 663 | 2 | 44 | 10 | 22 | 21 | 3.5 | 1.7 | 16 | 5.5 | 68 |
| 150 | 706 | 0 | 34 | 11 | 17 | 37 | 5.2 | 2.8 | 16 | 8.2 | 38 |
| 200 | 710 | 0 | 31 | 10 | 16 | 44 | 6.6 | 3.3 | 15 | 10.8 | 23 |

## 3. The policy opening (Tradition)

**What the AI does.** The first branch is chosen at round 16 (the first culture): Liberty in 156 of 160 seats (4 Tradition: Polynesia, China, Brazil, France), 154 of 160 in the bench suite. Adoption rounds (median): 1st policy 16, 2nd 29, 3rd 45, 4th 72, 5th 102, 6th 139; 3.0 policies by round 50, 4.7 by 100; the second branch is unlocked at round 178 (median). Culture is 3.7 a round at 50, 6.8 at 100, 12.6 at 200. The chooser's view before the first branch (round 15, 160 seats; value in points; the want is the situation's weight of an aim in percent): Liberty 238, Tradition 214, Honor 75, Piety 60 (the four unlockable branches; run of 20 rounds, whose first choice agrees with the 500-round game in 157 of 160 seats); wants Expansion 271 %, Gold 107, Science 103, Culture 103, Growth 88. Liberty's worth is mostly Expansion (41) and Happiness (43) and Production (36); Tradition's is Culture (91) and Happiness (36).

**What each opening gives in this ruleset.** Liberty: culture +1 per city, worker speed +25 %, +1 hammer a city and +5 % building production, +50 % settler production in the capital, +1 happiness per connected city and -5 % citizen unhappiness (Meritocracy), policy cost -33 % per city (Representation), a free great person at the finisher. Tradition: +3 culture in the capital and cheaper borders (opener), the Oligarchy garrison effects, Landed Elite (capital +2 food, +10 % growth), Monarchy (capital citizen unhappiness -50 %, +0.5 gold a citizen), Aristocracy (+15 % wonder hammers, +1 happiness per 10 citizens), the finisher +15 % growth in every city. Neither gives a beaker directly.

**Paired experiment: Tradition forced as the first branch** (same seeds, seats, 160 seats):

| | AI (Liberty first) | Tradition first | difference (CI) |
|---|---:|---:|---|
| second city, round | 33.5 | 37.4 | +3.9 (0.5..7.8) |
| cities at 100 | 4.14 | 3.46 | -0.69 (-0.83..-0.54) |
| share of seats with ≥ 4 cities at 100 / 150 / 200 / 300 | 79 / 86 / 86 / 88 % | 47 / 74 / 79 / 92 % | |
| policies adopted by round 100 | 4.7 | 7.0 | |
| culture a round at 50 / 100 / 150 / 200 | 3.7 / 6.8 / 9.0 / 12.6 | 5.4 / 12.6 / 17.8 / 19.8 | |
| capital citizens at 100 / 150 | 6.1 / 8.2 | 6.5 / 10.3 | |
| capital food surplus at 100 / 150 | +3.5 / +4.8 | +5.2 / +6.9 | |
| unhappy rounds 51-150 | 26 % | 16 % | -10.0 points (-13.6..-6.2) |
| mean excess happiness at 100 | 2.1 | 5.2 | |
| citizens at 100 / 200 | 14.6 / 29.3 | 13.5 / 32.5 | -1.1 / +3.2 (1.8..4.5) |
| science at 100 / 150 / 200 / 300 | 22.0 / 37.9 / 56.1 / 107.4 | 20.0 / 36.4 / 60.5 / 127.4 | -2.0 / -1.6 / +4.4 / +20.1 |
| techs at 300 / 500 | 36.0 / 53.1 | 37.1 / 55.5 | +1.1 / +2.4 (1.1..3.7) |

Tradition costs early expansion (the settler production of Liberty's Collective Rule and the +1 hammer are missing) and returns more culture (more policies sooner: 7.0 against 4.7 by round 100), a faster capital (+2 food Landed Elite, +10 %, finisher +15 %) and, above all, more happiness (Monarchy, Aristocracy, fewer cities at the same time): the gain starts about round 180 and grows. Four natural Tradition seats are too few to compare. **How to read this.** The experiment forces one branch on all 160 seats, whatever the map, the neighbours and the civilization; it measures what the branch costs and returns on average, not which branch to take. A seat with room for ten cities and no neighbour, or a leader of war, may do better with another opening; the gain here is a mean over seats, and by seat, science at round 300 was higher under Tradition in 104 of 160 seats (65 %) and lower in 56 (median difference +11.5, p10 -39, p90 +90; by map script 59-72 % of the seats gained); per civilization the groups (1-4 seats) are too small to say. The conclusion for the AI is about its pricing (it ignores that culture buys policies and what the capital's growth and happiness are worth when the budget binds), not a fixed order.

**Does it reach 4+ cities?** Yes, later (table). **What it gave in food/culture/science:** food surplus of the capital +1.7 at round 100, culture +85 % at round 100, science -9 % at round 100 and +19 % at round 300.

## 4. The binding constraints

### Where the beakers come from, and what is lost

The table at the top of this document is the first answer: at round 100, 66 % of the 22 beakers are citizens, 14 % the palace, 17 % libraries; at round 150 citizens are 60 %, libraries 26 %; the percent stage (University, Observatory, National College) is 5 % at 150 and 14 % at 200. Trade routes, policies, great people, specialists and tile science are about zero at every checkpoint (specialists per city 0.00 at 50, 0.02 at 100, 0.10 at 150, 0.29 at 200).

What is lost, in beakers a round on the mean seat:

| loss | at round 100 | at round 150 | at round 200 | how measured |
|---|---:|---:|---:|---|
| **Unhappiness: citizens not grown** (growth -93 % in the unhappy rounds) | 3.5 (2.3 citizens, 16 %) | 8.4 (5.0, 22 %) | 16.1 (8.4, 29 %) | cities × the growth gap in the unhappy rounds 51..R, times the beakers a citizen makes at R; growth only, no compounding |
| **Unhappiness, by seat**: most unhappy quartile against the least | | -18 | -36 | 25.6 against 44.0 and 32.4 against 68.1 beakers |
| **Happiness not supplied**: +4 and +8 base happiness | +1.7 / +2.9 | +4.0 / +7.1 | +6.8 / +12.7 | constraint tests (20 seeds) |
| **No Library** (0.5 a citizen in cities without one) | 3.7 (17 %) | 2.7 (7 %) | 1.9 (3 %) | cities without a Library 68 % / 38 % / 23 % |
| **Hammers**: production 20-25 % cheaper | +0.9 | +5.2 | +13.2 | constraint tests (10 seeds) |
| **Growth**: food to grow 20 % cheaper | +0.9 | +2.5 | +5.7 | constraint tests (10 seeds) |
| **Unhappiness** of the AI ×0.9 | +1.2 | +4.4 | +8.6 | constraint tests (10 seeds) |

### Implementation lag: what an unlock waits for

New permanent metric (bench `dev` suite, 160 seats, 20 games; observer `bench/metrics_lag.rs`, definitions in `platform/docs/ai-bench.md`): for every building or improvement that a technology or a policy branch unlocks and that adds science, food, production, gold or happiness, the rounds from the unlock to the first one the power built. Items pooled over the seats; an item unbuilt at the end counts at the rounds it has waited (so median and p90 are lower bounds); items unlocked after round 300 are left out.

| category | items | lag, median (95 % CI) | p90 | unbuilt at round 100, of the unlocked | unbuilt at 200 | cities with it at 100 | at 200 |
|---|---:|---:|---:|---:|---:|---:|---:|
| science (Library, University, Observatory, Public School, Research Lab) | 365 | 49 (45-54) | 232 | 22 % | 33 % | 37 % | 49 % |
| growth (Granary, Aqueduct, Hospital, Water Mill, Lighthouse, Farm, Pasture, Plantation, Mine ...) | 1236 | 66 (55-79) | 420 | 50 % | 40 % | 10 % | 22 % |
| production (Forge, Workshop, Factory, Windmill, Stable, Stone Works, Lumber Mill, Camp, Quarry ...) | 1780 | 71 (64-76) | 414 | 67 % | 48 % | 9 % | 36 % |
| gold (Market, Bank, Mint, Stock Exchange, Seaport, Trading Post ...) | 815 | 43 (42-44) | 341 | 42 % | 44 % | 0 % | 31 % |
| happiness (Circus, Colosseum, Stadium, Zoo, Stone Works, luxury camps, plantations, quarries) | 942 | 50 (48-52) | 362 | 65 % | 33 % | 8 % | 41 % |

(Columns 5-6 are means over seats of the share of the items unlocked by round 100 and 200 that were not built then; the last two are shares of (city, building) pairs where the building was unlocked and allowed.) The slowest tenth of all items waits 230-420 rounds or more (the unbuilt ones count at the rounds they have waited). The cases measured earlier by name agree: the first Library comes 42 rounds after Writing (median), the second after 91. A Library is the best case of its category: half of the science items are built within 49 rounds of the unlock, but at round 200 only 49 % of the cities have the science building that was open to them; for the happiness category the figure is 41 %, though every unbuilt one is worth a luxury's worth of growth.

**The binding constraints**, in the order of what they cost:

1. **Hammers.** 20-25 % cheaper production is worth more than any other single change: +25 % science at round 200, +39 % at 300, +5.8 techs at 500, 0.5 more libraries and 0.5 more universities at 200. Hammers are 5.5 a city at round 100, 8.2 at 150, 10.8 at 200. What they build: at round 100 the heads of the queues are wonders (26 %), soldiers (14 %), walls (10 %), libraries (11 %), settlers 2 %; at 150 wonders 31 %, libraries 7 %, soldiers 3 %. Nothing in the data says these are wasted for the time-victory score the AI maximises (a wonder is score and culture); they are late for the beaker engine.
2. **The happiness supply.** The empire's happiness (AI base 15, luxuries 4 each, a few buildings) has to cover 3 per city and 1 per citizen (× 0.9 for an AI). Content empires carry 23-38 points of unhappiness between rounds 100 and 200 and cover it with 9 more points of luxury happiness than unhappy ones, which carry 24-31 points (luxuries 11.4 against 5.1 at round 100, 14.9 against 7.5 at 150, 15.0 against 7.9 at 200). The luxuries they own in their borders at round 100: 3.8 kinds (content) against 2.8 (unhappy); the ones they count: 3.0 against 1.4. Per seat 1.7 luxury tiles at round 100 and 1.5 at 150 stand unimproved though the player knows how to improve them (1.2 at round 50, plus 0.44 not yet buildable). Every kind is +4 happiness = 4.4 citizens = about 7-8 beakers a round at round 150 (a fifth of the empire's science). Content empires have 0.67 workers a city at round 100 against 0.44 for unhappy ones, and twice the libraries (0.43 against 0.22 a city). The same budget stops the cities (section 2).
3. **Growth.** Capital surplus +2.9 (round 50), +3.5 (100), +4.8 (150); other cities about +2; 58 % of the cities food-bound at round 50 (settler 14 %, starving 11 %, +1..+2 33 %), 22 % low food at round 100 next to 44 % in an unhappy empire. 46 % of the worked tiles improved at round 100, 0.5 workers a city.

Not binding: the research order (section 1), the sites of the cities (section 2), the exploration as such (section 5; it works through contact: the first civilization is met at round 43, and a luxury trade needs one).

**A seat that has all three.** The Emperor game that ended by science (seed 10, round 456, Korea): 1 worker a city and a library in all four cities at round 100, 24 points of luxury happiness at round 150 (6 kinds) against 15 for content Prince seats, excess +4..+12 throughout, 26 citizens at round 100, 61 at 200, 106 at 300 in 4 cities, science 40 / 122 / 194 / 527 / 914 at rounds 100 / 150 / 200 / 300 / 400, 12 cities from round 400.

## 5. Exploration

**What the AI knows of the map** (160 seats, 500-round horizon; the map has 4160 tiles):

| round | map known % | land known % | own continent known % |
|---|---:|---:|---:|
| 25 | 4.1 | 6.7 | 49.1 |
| 50 | 6.1 | 9.9 | 56.5 |
| 100 | 9.8 | 15.2 | 64.7 |
| 150 | 12.5 | 18.9 | 69.5 |
| 200 | 14.1 | 21.1 | 72.4 |

Own continent known: 51.4 % at round 30, 58.7 % at round 60. By map script the map known at round 100 is 10.5 % (pangaea), 13.3 % (fractal), 4.9 % (archipelago), 8.1 % (small continents), 12.3 % (continents).

**Contacts.** The first scout unit appears at round 47 (median; p10-p90 37-58) and in none of the 160 seats by round 30: a unit is ordered from round 30 (`scouting::wants_recon`) and the city builds it behind other orders. Scout units alive: 0.66 per seat at round 50, 0.78 at 100, 0.88 at 150. The first civilization is met at round 43 (median), by 50 / 100 / 150 / 300 in 51 / 66 / 73 / 82 % of the seats (18 % never in 300 rounds). City-states: the first at round 7.5 (median, barbarians not counted), 2.6 met by round 50, 3.7 by 100, 4.1 by 150, 4.8 by 300 of 16; civilizations 1.0 by 50, 1.5 by 100, 1.8 by 150, 2.3 by 300 of 7. Ruins entered per seat: 1.21 by round 50, 1.81 by 100, 2.12 by 200. Scouts lost per seat in 150 rounds: 0.9.

**Against the simple reference** (every recon unit goes to the nearest known tile that has an unknown neighbour; both runs on a 150-round horizon, because the AI's prices depend on the horizon, 20 seeds × 8 seats, paired):

| | AI scouting | nearest unknown tile |
|---|---:|---:|
| map known at 50 / 100 / 150, % | 6.1 / 9.6 / 12.0 | 6.1 / 10.2 / 13.6 |
| difference at 100 / 150 | | +0.54 (0.12..0.94) / +1.62 (1.01..2.22) |
| new tiles per scout-round | 2.79 | 3.43 |
| scout-rounds standing still (of rounds with a scout alive, rounds 1-150) | 40 % | 54 % |
| scouts lost per seat in 150 rounds | 0.90 | 1.12 |
| civilizations / city-states met by 100 | 1.40 / 3.61 | 1.41 / 3.68 |
| own continent known at 60 | 58.6 % | 59.2 % |

The allocation of `ai::scouting` knows 12 % less of the map than the nearest-unknown rule by round 150 (CI above, 20 seeds) and loses fewer scouts. Both are far below what a scout can do: a land scout with 2 moves and sight 2 reveals up to 5 new tiles per step in open terrain, about 10 a round; 2.8-3.4 is a third of it [bound derived by me, not measured]. The larger loss is not in the choice of the target but in the timing: nothing explores until round ~47.

**How much did the map knowledge limit city-site choice?** Little (section 2): a better site beyond the explored map in 43 % of the foundings, 35 points of the mean score 561 (6 %) on average. The cost of not knowing the others is elsewhere: the luxuries a second civilization could trade (the AI owns 3.35 luxury kinds at round 100 but counts 2.38, section 4) and the city-states met (2.6 by round 50 of 16).

## 6. Permanent metrics

The observations of this diagnosis are now columns of the bench, filled by watching the games the bench plays anyway (`integration/crates/aurelia-qa/src/bench/metrics_dev.rs`, `metrics_classic.rs`; `platform/docs/ai-bench.md`, section «Development metrics»). No threshold, no change of `integration/benches/ai/baseline.json`.

```sh
cargo run --release --offline -p aurelia-qa --bin ai-tournament -- --suite dev --solo standard --jobs 3 [--save dev.json]
```

The `dev` suite: 5 scripts × standard size × seeds 1-4 = 20 games of 500 rounds, 160 seats; **2 min 53 s - 4 min 17 s** with `--jobs 3` at load average 15-23 (with the lag observer), 9 min 0 s at load ~40 (before it) [measured]. Columns (mean over seats with a value, share of seats that have it, seed-block bootstrap interval): second-city round; the implementation lag of unlocked buildings and improvements by category (science, growth, production, gold, happiness: median and p90 of the lag, share unbuilt at 100 / 200, share of cities with the building at 100 / 200); cities at 50 / 100 / 200; citizens per city at 50 / 100; libraries and universities at 100 / 200; technologies at 100 / 200 / 300 / 500; science a round at 50 / 100 / 150; map explored at 25 / 50 / 100 / 150; first contact and first civilization met; policies at 100; Tradition first (share of seats and round: 6 of 160 seats, all at round 16) and Tradition unlocked in any order (99 % of the seats, round 178), and the first branch by seats. The bench run reproduces the probe's numbers on other maps (cities at 100 4.12 against 4.14, technologies at 500 51.4 against 53.1, science at 100 20.9 against 22.0, second city 32.6 against 33.5).

## What is not measured

* **A human yardstick.** BNW science victories on Standard speed at turns 164 (Settler) to 199 (Deity) and "what a good player's capital grows by" are from public records and memory, not verified here; the only reference used for growth is the rules' own threshold arithmetic.
* **Optimal play.** The three reference orders are derivations from the effects, not an optimum; "not demonstrably worse" is the strongest statement the data allows. No search for a best order (simulation, learning) was run; no reference placement oracle exists (the sites are judged by the AI's own score, section 2).
* **What the constraint tests mean.** They change a number in the rules the AI plays under (a cost, a happiness offset) and say how steeply the economy answers; they do not say that the AI can win that efficiency back by playing better, nor which part of it. The Emperor bundle is bigger than the sum of its three parts.
* **No counterfactual of the allocator.** What the AI would do with a settler valued differently (the price of the city's unhappiness lowered, or counted against the supply being built) was not run: it needs a change of the AI. The only happiness interventions are base-happiness offsets in the rules.
* **Difficulty and size.** Prince on standard-size maps for everything but the Emperor comparison (10 seeds). Not run: Deity, small/tiny/large maps (the bench suites use tiny and small), other speeds; the three cost groups only on seeds 1-10.
* **Late game.** The analysis is detailed to round 300 (cities 12 at 500, techs 53); the science victory path itself (Apollo, parts) was not examined here. One Emperor game won by science (round 456); no other.
* **Sources not decomposed.** Trade routes, religion beliefs, city-state alliances, great people and golden ages sit in the "rest" bucket, which is near zero at every checkpoint; nothing in the data shows a hidden source, but they were not separated.
* **Horizon.** The AI's prices depend on the length of the game (`Prices::rounds_left`): runs of 150 or 200 rounds play differently from round 1 than runs of 500 (123 of 160 seats differ at round 50). All comparisons are inside one horizon; the 500-round baseline is the reference, the 150-round pair is used only for scouting, the 20-round run only for the chooser's view of the first branch (its choice agrees with the 500-round game in 157 of 160 seats).
* **Scouts "standing still"** means the unit's tile did not change since the last round; a unit that waits in a city, fortified or sleeping, counts.
* **"Starving"** (surplus ≤ 0) in the city table can also be a city that has run out of tiles it may use; the probe did not tell them apart.
* **Seeds per script.** Four games per map script: script-level differences are descriptive.
* **Effect of the exploration on science** was not measured (the greedy-scout pair ran to round 150 on the 150-round horizon).
* **Wall times** of the batches and of the bench run are measured on a machine shared with other agents (load average 28-52).

## Reproduce

```sh
export PATH=$HOME/.cargo/bin:$PATH CARGO_TARGET_DIR=<cache>
cargo +1.98.1 build --release -p aurelia-qa --example ai_diag --bin ai-tournament --offline
BIN=$CARGO_TARGET_DIR/release/examples/ai_diag
AI_DIAG_DUMP=1 $BIN > out/rules.json            # the ruleset tables the analysis reads
AI_DIAG_ORDERS=1 $BIN                           # the first 30 techs of each reference order
S="1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20"
sh integration/crates/aurelia-qa/examples/ai_diag_batch.sh out/prince-ai ai prince 500 "$S" 3                 # the AI as it is
sh integration/crates/aurelia-qa/examples/ai_diag_batch.sh out/prince-greedy greedy prince 500 "$S" 3         # --order greedy|econ|happy
EXTRA="--first-branch Tradition" sh integration/crates/aurelia-qa/examples/ai_diag_batch.sh out/prince-tradition ai prince 500 "$S" 3
EXTRA="--extra-happiness 4" sh integration/crates/aurelia-qa/examples/ai_diag_batch.sh out/extra-happy-4 ai prince 500 "$S" 3
EXTRA="--scouts greedy" sh integration/crates/aurelia-qa/examples/ai_diag_batch.sh out/scout-greedy ai prince 150 "$S" 3
sh integration/crates/aurelia-qa/examples/ai_diag_batch.sh out/patch-production ai prince 500 "1 2 3 4 5 6 7 8 9 10" 3   # EXTRA="--patch growth|production|unhappiness"
EXTRA="--alloc 1 --stop 250 --detail 0" sh integration/crates/aurelia-qa/examples/ai_diag_batch.sh out/alloc ai prince 500 "$S" 3   # the settler in the allocator
python3 integration/crates/aurelia-qa/examples/ai_diag_analyze.py out/prince-ai                              # the tables of sections 2-5 (rules.json next to the run directory)
python3 integration/crates/aurelia-qa/examples/ai_diag_compare.py out/prince-ai out/prince-tradition         # paired differences of every metric
python3 integration/crates/aurelia-qa/examples/ai_diag_compare.py --tests out/prince-ai "happiness +4=out/extra-happy-4" "Tradition first=out/prince-tradition"
python3 integration/crates/aurelia-qa/examples/ai_diag_compare.py --orders out/prince-ai greedy=out/prince-greedy econ=out/prince-econ happy=out/prince-happy
```

The 20 games of one set took 9-12 minutes on 3-4 threads on the shared machine [measured]. The runs of this document are in `~/Projects/civ/work2/diag-out` (not in git).
