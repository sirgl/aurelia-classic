# AI: the retake of a lost city (`retake`, bench-only)

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

## What `retake` changes

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

## Not done

Far units (more than two turns away) and the operations layer are not given the lost city (a retake by a mustered army);
a lost city out of sight is not retaken.

## `retake2`: the second iteration (review of 11 November)

The review of `retake` found three things; `retake2` (`Variant::Retake2`, bench-only, `retake` itself unchanged) answers them.

**The seat finds the lost city by its own memory.** The first iteration read the founder and the round of acquisition of
the city from the kernel (`ObservedCity::original_owner`, `acquired_round`) and called them public. The human client shows
neither, and the round told the AI that a city fell even where it did not see it. `retake2` reads neither. The seat keeps
`ai::retake::Memory` in the strategy memory (`AiState::mind_memory`, `strategy_net::Memory::retake`): its cities at its last
turn, and the ones that were gone at its next turn ("my city at tile T was taken in round R by player P"; the player is
read from the city when the seat sees it, as a human does). An entry goes when the seat holds the city again, when a player
other than the first taker holds it (a city that changed hands again is not this enemy's), and after the window
(`operations::march_limit`). The health of the city and the garrison are read while the city is in sight, as before.

Versioning: the memory is a section after the strategy memory (marker `0x5D`, a length, a body whose first byte is its own
version). A variant that does not play `retake2` never writes it, so its memory keeps its bytes; a memory without the
section (every older save) loads as no memory (a first turn finds nothing lost); a section of another version or length
loads as no memory and the rest is read as before. The platform needs no change.

**The guard test** (`retake::cases_remembered`) compares what the group can do against what stands there: the group's
power after the strikes the city makes while the assault lasts (one strike a round, `city_strike_damage` at the group's
mean strength, over the rounds the siege model needs) against the power of the captor's units in sight within four tiles
of the city, times a margin (`GUARD_MARGIN_PERCENT`). Walls seen are in the health bar (above a city's base) and so in the
siege model. The first iteration counted the captor's units within two tiles only.

**Tests**: `tests/ai_retake.rs` (a case opens with the city's real health; not for a city out of sight, lost long ago or
taken by a third player; peace refusal only while a case stands; the guard test refuses a heavily defended city; the
memory's versions).

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
