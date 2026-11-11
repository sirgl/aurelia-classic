# AI: the retake of a lost city (`retake2`, bench-only)

The question: does the defending AI understand whether it can retake a city it has just lost? Right after a
capture the city has no defences of its own yet and stands at the health the siege left it.

## What the base game does (measured, `war-pays`)

After a loss the strategy is `Recover` (`planner_strategy::desired_strategy`). Then

- `planner_units::assault_objective` gives an `Assault` task only under `Strategy::Attack`, so every melee unit near
  the city keeps its `Defend { city }` task and holds; only ranged units shoot (`tactics::immediate`), and a melee
  unit strikes a city only when it already stands next to it (`tactics::adjacent_enemy_city`);
- `operations::odds_are_good` admits an operation of `war-pays` against a city of a war already running in no case,
  and the lost city is estimated like any city (200 hit points, a garrison like our best melee unit);
- the holder's peace offer is accepted five rounds into the war (`diplomacy::peace_value`).

Base rate on 490 captures of a major's city in the solo pace games of offsets 200-209 (position = the start of the
former owner's next turn, 8 rounds replayed with every seat `war-pays`, two dice per position): the former owner
holds the city again within 2 / 5 / 8 rounds in 8.3 / 9.2 / 9.5 % of the plays, and makes peace without it in 36 %.

## What `retake2` changes

See "The second iteration" below for the mechanism: a case is a city the seat held at its last turn and lost, found by
the seat's own memory; the units on the spot assault it at once, and no peace is made with the holder while the case
stands. The strategy itself is not changed: the case runs inside `Recover`.

## Not done

Far units (more than two turns away) and the operations layer are not given the lost city (a retake by a mustered army);
a lost city out of sight is not retaken.

## History: the first iteration (`retake`, removed from the tree)

`retake` read the founder and the round of acquisition of the city from the kernel
(`ObservedCity::original_owner`, `acquired_round`, platform `1e64d0e`). The human client shows neither, so the review
(11 November) required the memory instead. `Variant::Retake` and the platform commit are removed (the platform revert is
`a4b129a`); its numbers stand as history and reproduce from the frozen binaries `runs/bin-final/` of the lane: held-out
offsets 207-209 retaken within 8 rounds 58.1 % against 8.9 % for `war-pays`, own units lost +1.30; whole games 51.4 %
(offset 410), 51.8 % (411). What it did:


`ai::retake`: a **case** is a city the player founded, held by an enemy at war, in sight, taken within the march
limit. Its real health (the banner) and the strongest unit of the holder standing in it enter the estimate; the
player's units within two turns of it are the group; the case stands when the group is not outmatched by the holder's
units in the open within two tiles and the siege model (`front::siege_rounds_within`) brings the city to capture
within two rounds. Then

1. the units of the group take the `Assault` task at once and do not wait for a bigger group (`planner_units::ensure_task`,
   `tactics::siege_step`); garrisons of cities in danger, the last guard of a city and operation members stay;
2. no peace is made, offered or accepted with the holder while the case stands (`planner_strategy::consider_peace`,
   `operations::idle_wars`, the diplomacy advisor).

The case reads only the city banner (`ObservedCity::original_owner`, `acquired_round`) and the units in sight; it keeps
no memory, so nothing is added to the saved AI memory. The strategy itself is not changed: the case runs inside `Recover`.

### Not done (first iteration)

Far units (more than two turns away) and the operations layer are not given the lost city (a retake by a mustered army);
a lost city out of sight is not retaken.


## The second iteration (review of 11 November)

The review of `retake` found three things; `retake2` (`Variant::Retake2`, bench-only, `retake` itself unchanged) answers them.

**The seat finds the lost city by its own memory.** The first iteration read the founder and the round of acquisition of
the city from the kernel (`ObservedCity::original_owner`, `acquired_round`) and called them public. The human client shows
neither, and the round told the AI that a city fell even where it did not see it. `retake2` reads neither. The seat keeps
`ai::retake::Memory` in the strategy memory (`AiState::mind_memory`, `strategy_net::Memory::retake`): its cities at its last
turn, and the ones that were gone at its next turn ("my city at tile T was taken in round R by player P"; the player is
read from the city when the seat sees it, as a human does). An entry goes when the seat holds the city again, when a player
other than the first taker holds it (a city that changed hands again is not this enemy's), and after the window
(`operations::march_limit`). The health of the city and the garrison are read while the city is in sight, as before.

Versioning: the memory is a section after the strategy memory (marker `0x5E`, a length, a body whose first byte is its own
version; `0x5D` on the retake line, renumbered at the merge onto the thirteenth promotion, whose `next` section took `0x5D`). A variant that does not play `retake2` never writes it, so its memory keeps its bytes; a memory without the
section (every older save) loads as no memory (a first turn finds nothing lost); a section of another version or length
loads as no memory and the rest is read as before. The platform needs no change.

**The guard test** (`retake::cases_remembered`) compares what the group can do against what stands there: the group's
power after the strikes the city makes while the assault lasts (one strike a round, `city_strike_damage` at the group's
mean strength, over the rounds the siege model needs) against the power of the captor's units in sight within four tiles
of the city, times a margin (`GUARD_MARGIN_PERCENT`). Walls seen are in the health bar (above a city's base) and so in the
siege model. The first iteration counted the captor's units within two tiles only.

**Tests**: `tests/ai_retake.rs` (a case opens with the city's real health; not for a city out of sight, lost long ago
(through `cases_remembered` and in the memory) or taken by a third player; the holder's peace offer is refused by the
diplomacy advisor while a case stands and accepted when it is gone; a unit does not rally for a city being retaken; the
guard test refuses a heavily defended city and does not count captor units out of the seat's sight; the memory's versions).

## Fit

The margin is the one constant fitted: the tuning positions (offsets 200-206, 355 positions, two dice, 8 rounds) replayed
with a margin knob (removed at the freeze), margins 100, 150, 175, 200, 250, 300, 400, 800; the rule, set before the runs: the
smallest margin with own units lost at most 1.0 above `war-pays` and failures that lose more than 5 units at most 2 points above
it. 175 is the first to pass (+0.98 units, +2.0 points; 150: +1.20). Table (retaken within 8 rounds / own units lost / fail and >5 lost):

| layer | all | captor units within 4 tiles >= 14 | < 14 |
|---|---|---|---|
| `war-pays` | 9.7 % / 2.53 / 8.0 % | 11.0 % / 3.08 / 13.0 % | 9.4 % / 2.39 / 6.7 % |
| `retake` | 54.1 % / 3.95 / 10.4 % | 37.0 % / 5.39 / 17.8 % | 58.5 % / 3.57 / 8.5 % |
| `retake2` | 40.3 % / 3.51 / 10.0 % | 22.6 % / 4.36 / 17.8 % | 44.9 % / 3.30 / 8.0 % |

## Cost, replication and limits (review of 11 November, second round)

**Cost in the heavy-guard class.** The margin does not solve the case of a heavily guarded city. With 14 or more captor
units within 4 tiles of the city (70 of 236 replication plays) `retake2` retakes 15.7 % against 2.9 % for `war-pays`, loses
4.99 own units against 4.04, and 31.4 % of its attempts fail with more than 5 units lost against 30.0 % for the base: about
the same failure rate, with a retake costing **5-7 own units per extra city retaken** (5.1 on the lane's acceptance data,
7.3 on the replication positions). Under 14 units (166 plays) it retakes 54.8 % against 12.7 %, loses 3.48 against 2.46
units and fails heavily in 7.2 % against 3.0 %: about 2.4 own units per extra city retaken. The lane is a gain in the
second class and a cost, not a saving, in the first.

**Replication** (offsets 440-441, 413; the lane's own runs are offsets 430-431, 412). Gates against `war-pays`:

| Gate | Lane | Replication |
|---|---|---|
| Retaken within 8 rounds (gate: at least 2x) | 49.2 % vs 9.7 % | 43.2 % vs 9.7 % (102 vs 23 of 236 plays; 4.4x, bootstrap 2.9-8.4x) |
| Own units lost per position (gate: at most +1.5) | +1.03 | +0.99 (3.92 vs 2.93) |
| Fail and lose more than 5 units (gate: at most +3 points) | +0.4 | +3.4 points (14.4 vs 11.0 %), interval -2.5...+8.9: misses the gate by 0.4 and is not distinguishable from zero |
| Whole games, weighted share (gate: at least 48 %) | 51.8 % | 48.9 % (plain 50.0 %, 47.0-53.0) |

"Heavy failures unchanged" is not claimed without that interval. The hashes of `standard` and `war-pays` are unchanged.

**The fit of the margin reproduces only from `runs/bin-tune`**, the binary with the margin knob (`RETAKE_MARGIN`), over offsets
200-206 (`runs/cap/tuning-table.txt`). The frozen binary `runs/bin-r2-final/` has the margin compiled in (175) and no knob.

**Known limits.**

- A seat that loads another variant's mid-game save starts with an empty memory (that variant never wrote the section): a
  city lost on the turn of loading is missed. The bench plays whole games, so it does not show this.
- A captor first seen after the loss is taken as the holder at that moment (`Memory::observe`): a city that changed hands
  between the loss and the first sight is retaken from the wrong enemy, or not at all.
- The case is opened only for a city in sight; far units and the operations layer are not given the city.
