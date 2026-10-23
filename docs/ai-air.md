# The AI's air arm

> Code: `classic/crates/classic-rules/src/ai/air.rs` (decisions), `planner_cities.rs::air_orders` (production), `fleet.rs::decide_naval` (dispatch), `driver.rs` (observation radius). Tests: `classic/crates/classic-rules/tests/ai_air.rs`. Control variant: `no-air` (Standard without the air arm).

Aircraft in this ruleset never leave their base: a strike is made from the base city (or carrier) at a target within the unit's range, the strike ends the unit's turn, the target answers an air strike, and a rebase to another own city (within twice the range) is the only movement. So there is no fuel to run out of; what the AI decides is **which base, which target, which posture**.

## Classification (from data, no names)

| Wing | Rule | Used for |
|---|---|---|
| Striker | domain Air, ranged strength > 0, range > 0, not `suicide`, `nuke_level` = 0, no interception range | strikes on cities and ground/sea units |
| Fighter | domain Air, `air_intercept_range` > 0, not `suicide`, not nuclear | interception posture, sweeps, strikes only to kill |
| not commanded | one-way missiles (`suicide`), nuclear weapons (`nuke_level` > 0) | see "Not done on purpose" |

## Decision of one aircraft (first rule that applies)

1. **Leave a falling base.** If the base city is threatened by more than its (planned) guard can hold (`defence::Danger.threat > planned`), rebase to the nearest safe own city in reach.
2. **Mend.** Below 70 hit points: stay in the base (aircraft heal only in a city, about a quarter of their health a turn).
3. **Strike** the best target in range by the rules' public preview (`attack_preview`): gain = target value (city 1000 + 20 per citizen, a fighting unit 60 + 4 x strength, a civilian 40, the same scale as `front.rs`) x share of its hit points taken; a city that no own army is near and no operation targets counts 25 % (the damage heals); loss = the aircraft's value x damage taken / 100, where the damage taken is the target's answer (`air_defense_damage` from the preview's strengths, because the preview of a ranged attack shows 0 damage to the attacker) plus the damage of every visible interceptor whose reach covers the target (all assumed to intercept). A strike must leave at least 20 hit points and have positive net value. A fighter strikes only when the preview is a decisive victory.
4. **Sweep before the strike.** When the best strike pays without interception but visible interceptors cover the target, one of our fighters (hit points >= 70, order accepted by the rules) sweeps an interceptor tile first (air interceptors before ground ones, because a sweep of a ground interceptor only spends its interception); the striker is asked again. A fighter also sweeps on its own an enemy air interceptor in range when its strength is at least 90 % of the enemy's.
5. **Rebase to the front.** The own city (not falling, in rebase reach, with room: the rules' `accepts`) from which the most enemy value is in range, counting the targets of the running war operations even unseen; only when that is 50 % more than from the current base (no ping-pong).
6. **Posture.** A fighter that stays takes the interception stance (once; the stance persists until it moves or strikes).

## Production

`air_orders` follows `fleet_orders`: one city of the half asked, no military unit queued, never ahead of a settler or a wonder, bought when slower than `WAR_BUILD_TURNS` and the treasury allows, only while `war_chest_covers` pays the upkeep. Outside the army cap.

- strikers wanted = 2 per running war operation, at most 4;
- fighters wanted = 1 to cover the strikers + 1 while enemy aircraft are in sight, at most 2.

The unit built is the strongest available of the wing (technology, strategic resources, not obsolete), the cheaper on a tie.

## Not done on purpose

- **Nuclear weapons and one-way missiles**: not the air arm's (23 October 2026): `ai/strikes.rs` commands them in the variants `strikes` and `heavy`, with a value model in score points that counts every unit and city in the blast, and the rules' own check refuses a blast that would hit a player not at war ([ai-strikes.md](ai-strikes.md)). `standard` builds none.
- **Paradrop and airlift**: no AI order; a paratrooper is a land unit, it needs the army planner (not aircraft) to want it.
- **Carriers**: `standard` builds none; `ai/carrier.rs` builds them and bases aircraft on them in `heavy` ([ai-strikes.md](ai-strikes.md)).
- Interception chance is assumed 100 % in the loss estimate (the visible data does not give the enemy's promotions); a loss estimate that is too pessimistic costs strikes, not aircraft.

## Measured (21 October 2026, release, `examples/ai_air_war.rs`, paired by seed, [measured by me])

Scene: the AI (3 cities, oil, radar-era techs, 4 riflemen, 2 bombers and a fighter) at war with a passive rival (3 cities, 3 to 5 defenders each, an anti-aircraft gun, on odd seeds a fighter on duty, a field army in sight of the AI's second city). The control is `no-air` (the generic advisor shoots with the aircraft; no rebase, posture or sweep). End = round of the conquest of the rival's capital.

- 12 seeds, rounds to the end, air arm / control: mean 94.2 / 107.9. Faster in 6 seeds (for example 52/123, 14/116, 15/118, 111/123), the same in 1, slower in 3 (123/68, 123/68, 200/57). Not a clean win: the slower seeds are bombardments of a city that the ground army does not take.
- Aircraft lost in those runs: the same as in the control (0 or 2 of 3); the control's losses are the same units (a captured city), none were lost to a strike the forecast allowed in the seeds where the rival's fighter was seen. Unseen interceptors are the known loss: a bomber lost 100 hp in one strike to a fighter standing in another city (the forecast counts only interceptors in sight).
- The forecast against the dice (`tests/ai_air.rs`, 60 seeds): a city alone, forecast 19 hp, mean damage 13.9 hp; a city with a fighter on duty and a gun: forecast 167, the bomber died every time.
- No stuck turns: 0 exhausted turn budgets, at most 1 rejected command per game, in 28 games. Same seed, same line (deterministic).
- Ping-pong found and fixed: a fighter flipped between a front city and a rear one (29 rebases in a game); a dead zone between "falling" (threat > 2 x guard) and "sheltering" (threat x 2 <= guard) cut it to 5.
- Natural AI-vs-AI games (4 to 6 players, 300 rounds, radar-era techs and oil given at round 60 or 100): 0 wars in 9 games, so no aircraft were ordered or used; the air arm acts only in a war.
- Hash pins: the whole `classic-rules` suite (923 tests) is green with the air arm, no pin was changed.

Open finding (not fixed): in the scene, aircraft standing in the capital made the control's army take the rival's capital in about 120 rounds, against 14 rounds without any aircraft. Cause not proven; hypothesis: the AI's own stack check (`planner_units::can_stop`, `blockers_with_own`, `front::free_for`) counts aircraft as land military units, which the core does not limit. A fix that ignores aircraft there did not give a clear gain (8 seeds, mixed) and was reverted.
