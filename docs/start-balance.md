# Balanced starts

A new game used to put each nation on the best land the greedy pick found for it, one nation after the other. The result
was uneven: of 8,000 starts measured over 1,600 maps, 19 % had too little food, too little production or too little land, 7.7 % had no
tile with production within two tiles, 61 % of the games had at least one such start, and only 9.5 % of the starts
had both horses and iron within three tiles. The code is `platform/crates/kernel/src/scenario/start_balance.rs`; the setup of a
Classic game calls it from `crates/classic-rules/src/scenario/new_game.rs`.

## What a game does now

| Step | What |
|---|---|
| value | The value of a start tile is the sum, over the tile and the three rings round it, of the real tile yields (terrain, feature, the bonus resources known from turn 0) by a weight per yield channel and per ring (`StartPolicy`). Production weighs more than food, because ocean and coast feed but do not build. |
| choose | A candidate that reaches the minimum food and production within two tiles and a floor value beats any other. Among the rest, the nearer the value is to the target (the 75th percentile of all candidates), the better, from below and from above; nearness to the sea and the nation's wishes count as before. So no nation takes the best land and the starts of a game come out alike. |
| strategic | Every start gets every early strategic resource (reveal technology in an era up to `strategic_up_to_era`: horses and iron in Classic) within three tiles, not under the start tile, in the smallest quantity the rules give the resource. The tile is a free one of the right terrain, nearest the preferred distance first; if none is free, a plain tile is turned to a terrain the resource lives on. |
| top up | A start under the minimum food or production, or under `even_percent` of the game's median value, gets at most `edit_budget` edits: a bonus deposit on a free tile, or a poor flat tile turned to hills or grass. In the 1,600 measured maps no start needed one; the safety net is there for cramped maps and is tested with a raised minimum. |

The balance lives in the Classic game path (`generate_new_game_on`). The kernel's own fixture generators (`generate_player_game`,
`pick_starts`) keep `StartBalance::OFF`, so the platform's pinned fixtures do not move.

`StartBalance` is the switch: `ON` (default), `OFF` (the generator as it was, bit for bit), and `strategic` for the setup screen's
"Strategic balance" (`NewGameOptions::start_balance`, `GameOptions::start_balance`; the setup screen has no control for it yet).

## Measuring

`cargo +1.98.1 run --profile fasttest -p classic-rules --example start_census -- duel,tiny,small,standard continents,fractal 200 > rows.csv`
prints one row per start; `tools/start-census-report.py report rows.csv --thresholds t.json` turns the rows into the distribution
tables (`--write-thresholds` fixes the "bad start" thresholds from the medians of a run, the same file judges the next one);
`compare before.json after.json` prints the before and after table. Add `1 off` after the seed count to run the generator without the balance.
`32x24:4` as a size is the world of the offscreen screenshot (`AURELIA_GAME_SEED=<seed> AURELIA_GAME_START_BALANCE=off aurelia-classic --screenshot out.png`).

The tests are `crates/classic-rules/tests/start_balance.rs` and the unit tests of `start_balance.rs`.
