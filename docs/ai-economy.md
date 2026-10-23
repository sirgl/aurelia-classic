# The AI's happiness-and-production budget (21 October 2026)

**One lever of the plan moved the economy: the AI never built a work boat, so about half of the luxuries it could have improved (pearls, whales, crab) stood unconnected for the whole game. Since 21 October its cities build work boats for the sea deposits in the borders (`ai/sea_work.rs`): science +6.7 % at rounds 200 and 300, +1.4 technologies at round 500, score +4.8 %, unhappy rounds -3.6 points. The other levers of the plan (a payback gate for buildings, more workers, workers to luxuries first, a settler charged at the cost of the happiness it needs) moved nothing and are removed.** Branch `ai/economy-budget` (on top of `ai/open-horizon`). Method and tools: [ai-bench.md](../../platform/docs/ai-bench.md), [ai-science-diagnosis.md](ai-science-diagnosis.md); the numbers are in the last section.

## What was asked and what was found

The diagnosis (science is about 1.5 beakers a citizen; an unhappy empire grows 93 % slower; the AI is unhappy in about a quarter of the rounds 30-250; 1.7 luxury tiles stand unimproved at round 100) asked for a happiness-and-production budget in the allocator. Four levers were built behind one switch and measured one by one (the dev suite: 20 games, 4 seed blocks, 160 seats), then dropped for lack of effect:

| Lever | What | Result on science at round 200 |
|---|---|---|
| payback gate | a building may take a third of the rounds left to build instead of 40 turns | +0.34 (CI -0.18..+0.73) |
| more workers | the jobs of a worker count the luxury tiles at the price of their happiness; a worker serves 2 jobs, not 4 | workers a city +0.27 at round 200, luxury kinds +0.01, science +0.54 (-1.87..+4.47) |
| luxury first | the workers take the luxury tile first | same run |
| settler net | a new city's unhappiness is charged at the cheapest way the rules offer to make the happiness | cities at 100 and 300 unchanged (the small maps run out of sites at 4-5 cities) |

What explained the idle luxuries was a new measurement, not a lever: the bench now counts, per seat, the luxury kinds it could improve now and has no copy of (`luxury_lacking`) and how many of them lie only on water. At round 100 1.1 kinds are idle, 0.55 of them only on water; at round 200 1.2 and 0.84. The AI never ordered a work boat: `unit_of_role(Worker)` is land-only, and the planner's `buildable` refused the water improvements (the ruleset marks them unbuildable for workers: work time 0, the boat is used up by the job).

## The change (`ai/sea_work.rs`, generic, no cheat)

* `is_sea_builder`: a unit that is a builder and sails. Found by data; no name.
* `sea_values`: for every water deposit inside the borders (within the cities' work radius) that some improvement of its resource can improve now: the yields the improvement adds at the prices of the turn, plus the happiness of a luxury kind the empire has no copy of (once per kind) at the price of a point of happiness. Sorted, best first.
* The allocator offers a work boat in a coastal city at the value of the first deposit no boat (alive or queued) is meant for, delayed by the build time and `SEA_JOB_ROUNDS`; it competes with every other order by value per turn, so an empire with something better to do builds none.
* The workers: a builder of the sea takes only water tiles, a land worker only land; boats leave the empire's worker count (the land workers' target is unchanged).
* No price, cap or multiplier of the game is changed. `Variant::Landwork` (`landwork`) is the control: the AI without boats. `Standard`, `Planner` and `Closed` have them.

## Measured (Civ V rules, small maps, 6 nations, 500 rounds, the five map scripts of `pace`)

`pace --solo`: every seat plays the variant, 20 seeds x 5 maps = 100 games, 600 seats, against the same seats without boats (`standard` before the change); seats are paired by (map, seed, nation), the unit of resampling is the seed block (20), 95 % bootstrap CI. Machine under load average 50-70; round times are not comparable between runs.

| | without boats | with boats | difference (95 % CI) |
|---|---:|---:|---|
| citizens at round 200 | 28.95 | 31.09 | +2.14 (+1.66..+2.75) |
| science a round at 100 / 200 / 300 | 21.89 / 53.36 / 104.44 | 22.30 / 56.95 / 111.39 | +0.41 / +3.59 / +6.95 (all CI above 0) |
| technologies at 200 / 300 / 500 | 28.23 / 36.74 / 55.19 | 28.66 / 37.53 / 56.59 | +0.43 / +0.79 / +1.40 |
| luxury kinds a seat has at 200 | 3.09 | 3.62 | +0.54 (+0.45..+0.62) |
| luxury kinds it could improve and has no copy of, at 200 (only on water) | 1.23 (0.84) | 0.74 (0.34) | -0.48 (-0.57..-0.41) |
| unhappy rounds 30-250, % | 26.79 | 23.24 | -3.55 (-4.50..-2.59) |
| happiness excess at 100 / 200 / 300 | 1.69 / 3.65 / 5.73 | 2.57 / 4.38 / 7.06 | +0.88 / +0.73 / +1.33 |
| hammers a round at 300 | 92.10 | 98.69 | +6.59 (+4.47..+8.72) |
| final score | 1073.79 | 1125.41 | +51.63 (+37.53..+67.76) |
| cities at 100 / 300 | 4.35 / 5.71 | 4.36 / 5.76 | 0.00 / +0.05 (not separable) |
| implementation lag of unlocked buildings, median, rounds (science / growth / production / gold / happiness) | 58 / 70 / 73 / 48 / 52 | 58 / 71 / 72 / 49 / 54 | within the CI (the lag is not what the boats change) |

* Games: result hash of the dev suite `91f55d434df5e160` (boats) against `8cabb1c14cd8df5a` (without); the 4-block dev suite gave the same direction (science at 200 +4.66, CI +3.49..+5.78).
* The science race (time victory off, to round 650, seeds 1-20 x 5 maps): science victories 66 -> 71 of 100 (+5 pp, CI -2..+12; not separable), median round of a victory 588 -> 581, technologies at 500 58.9 -> 60.8. The boats lift the economy, the race is decided by the open horizon ([ai-investment.md](ai-investment.md)).
* Strength (`pace`, head to head against `allocator`, seed offset 0: 20 seeds x 5 maps x both sides = 200 games, paired by game): points share 67.8 % (CI 63.0-72.5) without boats, 73.0 % (68.5-77.5) with them, paired difference +5.2 pp (CI +2.0..+8.5); 177 of 200 games end the same way. Result hash `887e4686c0455bb6` without, `376270ca90133fab` with.
* Round time (the same two runs plus the pre-open-horizon run that shared the machine with the boats' run, load average 50-70): p50 / p95 of a round 119.9 / 648.6 ms (without, run alongside) against 113.0 / 618.4 ms (with boats); the AI's own time per turn p50 / p95 9.66 / 68.2 against 9.37 / 71.7 ms. An earlier run on a quieter machine gave 99.4 / 342.3: round times are only comparable between runs that shared the load. No cost of the boats is visible.

## Not done, not measured

* Seeds 21-40 (the second offset of the pace suite): the runs were cancelled at the deadline (the machine ran at load average 50-70), so every table above is 20 seeds x 5 maps, not 40.
* The first-level policy branch (Tradition, Liberty, ...) is not valued here: it depends on position, neighbours and nation and needs its own payback model.
* The luxuries still idle at round 200 (0.74 kinds, 0.34 only on water): boats wait for Sailing, for a coastal city with a free queue, and the best use of a coastal city's hammers is sometimes another order.
* Luxury trade with other nations: the AI already swaps spare copies when it prices happiness (`diplomacy::luxury_swaps`); no change was measured or made.
* Nothing in `lands/crates/lal-rules` is touched; the L&L module has no work boats and is unaffected (`--rules lal` smoke: see the report).
