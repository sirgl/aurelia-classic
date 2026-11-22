# Needs from B / E / A / T-lanes (part C)

Seam: every reader fetches the plan in ONE place, `operations::with_net2(brief, |plan| ..)` (also `Net2War::of`).
Its body is a test override only; E replaces it with `brief.net2.as_ref().map(read)` (override kept first).

- E (`TurnBrief::net2`): the body of `operations::with_net2`.
- E (`driver.rs`): after `net2::think`, call `brief.defence.as_mut().map(|w| w.apply_net2(&brief, &obs.own_cities))`
  (the watch is made BEFORE `think`, so its guard levels are set after).
- E (`planner_cities.rs:595`): `fleet::shortfall` -> `fleet::shortfall_for(obs, &brief, state, rules)`.
- E (`economy.rs`, after `great_person_commands`): add `great_people::general_commands(obs, rules, great_people::generals_of(&brief))`.
- B (`strategy_net::Memory::of/store`): `retake.rs` has local stand-ins (`strategy_memory`, `store_strategy_memory`); swap when B lands them.
- A: `soldiers_r` estimate for `known_power` (design 2.4 item 4): no API in the contract; `known_power` unchanged.
- Tactical T2/T4: `zone_damage::at` (danger.rs has `zone_damage_at`, same rule, citadel only), `unit_net::calls`/`city_ease` (`operations::TacticalHints`, test override only).
