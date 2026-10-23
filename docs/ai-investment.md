# AI investment planner (`Variant::Planner`)

**Science and the economy under it are every leader's foundation, the spaceship is every leader's aim, and a leaning only decides what the lead is used for; most of what this buys in a game without a time victory comes from one fix: the AI no longer plans for a game that ends at round 500 when nothing ends it there.** Code: `ai/grand_strategy.rs`, `ai/value.rs`, `ai/variant.rs`. Rules and the allocator: [ai-allocator.md](ai-allocator.md), [ai-leaders.md](ai-leaders.md). Numbers: the last section.

## Why

Before this work (59-60 pace games, no science victory; AIs know about 49 of the 75 technologies the ship needs at round 500; about two libraries per AI [earlier agents' notes, not rechecked]) the code had three defects:

1. A neutral leader weighed the science path at `(leaning - 5) * 200 / 5 = 0`; only the score path counted. With the time victory off every path value was 0: no aim at all.
2. `science_chance` divided the beakers still needed by the science *now*: an empire with a tenth of the science it needs was never "on track" and never invested (chicken and egg).
3. Every price counted `max_rounds - round` rounds left: **1 from round 500 on, whether or not a time victory exists.** With it off, the AI near and past the nominal end values a citizen, a library or a technology for one more round, and grabs cities instead (13 cities against 7 at round 500).

## Design: work backward from the aim, price the chain

The planner changes the *prices* the allocator already values every building, technology and order by; it is not a second planner beside it.

| Piece | What | Where |
|---|---|---|
| Aim | The spaceship is every leader's path at the score's weight (`FOUNDATION_PERCENT`); a leaning adds to it. | `Outlook::value` |
| Chance | Beakers by the time the ship must start, with science growing as it has grown (in proportion to the rounds played), never divided by today's science. | `grown_science_chance`, `projected_chance` |
| Horizon | Without a time victory the rounds left are as many again as were played, never fewer than to the nominal end. Growth, culture, research and every price read it. | `value::horizon` |
| Price of the aim | On the spaceship path a beaker a round, kept to the end, is worth at least the chance of the prize shared by the beakers still needed; production reaches it through the research process. | `grand_strategy::aim_prices` |

Technologies, policies, buildings and orders are valued by their declared effects at these prices, as before. No name of a technology, building or leader appears; the aim is the victory the rules declare. No price, cap or multiplier of the game is changed for the AI. The open horizon is now the game's AI (`Standard`, 20 October): `Variant::Closed` is `Standard` with the nominal horizon (the AI of the game before), the control of this change; with the time victory on (the default) the two play identical games.

## Measured (Civ V rules, small maps, 6 civilizations, 500-round games)

Fresh seeds 11-50 x 5 map scripts = 200 games per cell, time victory off, the game played on to round 650 or a victory; blocks = seeds (40), bootstrap 95 % CI.

| | Standard | Horizon only | Planner |
|---|---:|---:|---:|
| science victories by round 500 / 550 / 600 / 650 (of 200) | 0 / 3 / 26 / 63 | 0 / 25 / 88 / 139 | 1 / 25 / 86 / 134 |
| median round of a science victory | 605 | 591 | 583 |
| technologies / beakers a round at round 500 | 55.0 / 324 | 59.6 / 417 | 61.0 / 429 |
| cities / citizens at round 500 | 12.4 / 98 | 7.0 / 88 | 7.1 / 89 |
| seats with all 75 technologies by round 500 (of 1200) | 0 | 4 | 5 |

Planner minus Standard: victories by 650 +35 points (CI +29..+42); planner minus Horizon: -3 points (CI -7..+3), +1.5 technologies (+1.1..+1.9), +37 score (+27..+48). So the open horizon makes the difference, the aim adds a little economy and no victories. With the time victory on (200 games, 500 rounds) planner and standard are the same within noise (score -5 [-12..+0.3], no victory of either kind); head to head on the pace suite (200 games) the planner scores 50.0 % (48.0-52.0) against `standard`, and 77.2 % (73.2-81.5) against `allocator` where `standard` scores 77.2 % (73.8-80.5); per-turn AI time p95 13.1 against 13.2 ms in the same games. The ship is late rather than out of reach: the median seat that completes the 75 technologies does so in round ~581.

Implementation lag (the rounds from the unlock of a building or improvement with an effect on science, growth, production, gold or happiness to the first one built; `lag_probe`, 100 games each, time off, 300 rounds), Standard -> Planner: science median 49 -> 41 rounds (CI of the difference -10..-5), gold 86 -> 80, production 133 -> 134, growth 144 -> 154, happiness 59 -> 66; unbuilt at round 200: science 34 -> 30 %, growth 60 -> 63 %, production 69 -> 72 %, gold 67 -> 66 %, happiness 34 -> 38 %. A University waits second or third in the city's queue behind a Courthouse, Colosseum or Caravansary (700-3000 centi a turn; a point of happiness costs 2200-4700) and 25-35 % of the choices at round 200 are "nothing worth building": the lag is production and happiness prices, and a payback ordering is already what the allocator does (value per turn).

Tried and dropped (seeds 1-8, 40 games each): an aim price from the slope of the chance with a citizen worth the yields it works (score -32 +-27 with the time victory on, no gain in victories), research along the way to the best goal and a science want in the policy branches (58.3 against 59.7 technologies; the same finding as the reference orders of classic/docs/ai-science-diagnosis.md), the same with culture and gold wanted too (no better). The policy branch is not fixed by the planner: it stays the allocator's choice from the situation (room, rivals, civilization, resources); forcing Tradition in the diagnosis was an experiment about the price of a branch, not a rule.

## Not done

Wars are not priced against the aim. A payback model of policy branches and a model of the happiness budget (science is a population statistic, about 1.5 beakers a citizen) are the levers left; the price of a beaker is not. With the time victory on, nobody wins by science in 500 rounds with either AI.
