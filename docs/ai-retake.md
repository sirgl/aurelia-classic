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
