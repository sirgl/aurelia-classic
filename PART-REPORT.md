# Part E report (net2 wiring, sites, tests)

Commit c27e659 on `work/net2-e` (on B's c3abf05, fast-forwarded).

Built
- variant.rs: `Net2`, `Net2Defence`, `Tac2`, `Net2Tac` (names net2, net2-defence, tac2, net2-tac; experimental-ai only); `Variant::net2()`, `net2_offence()` (false for net2-defence, inert until the offence exists), `unit_net()` true for the tactical two.
- driver.rs `ClassicMind::new`: all four play as `nets-x-site` under their own name; `net2` / `net2_offence` / `tactical_nets` fields; `net2::think` replaces `strategy_net::think` for net2 seats and sets `brief.net2`; `economy_nets` and `variant()` know them; U-9 sensor (wide only with a hostile in the unit's own radius) for the tactical two.
- planner.rs: `TurnBrief::net2: Option<Net2Plan>` (None for every other variant).
- planner_settle.rs: `SiteQuality::Head` + `HeadRead`, `head_inputs` (S1-S9; S10 `claim` = 0); used only when `brief.net2` is Some and the site net is on; `standard` keeps `net_score`.
- tests/ai_net2.rs, ai_experimental_off.rs (new names have no name in the game build).
- tools/net2-gates.sh: build, g0, g1-run/check, g2-run/check (G2 and G3). Not run.

Tests (fasttest)
- ai_net2: 5 passed (plan on every strategic turn and deterministic; net2-defence == net2; standard and pre-nets-x 120-round hashes pinned and no plan; only net2 variants think with net2; head with a silent plan == guarded net_score on every candidate, tall adds only +25, menace never raises). lib variant/driver/net2/planner_settle: 58 passed. ai_experimental_off 4 passed, site_search 1 passed.
- Hashes (ai-tournament --solo ... --suite standard --speed quick): standard `8460fdf4e1b4bad1`, pre-nets-x `d2d736c1273a82bb`.

Coordinator's checks
- Float parity on a real trace: NET2_TRACE, 30 rounds of net2, 116 turns: the exact port differs by 0 everywhere, the float twin by 0 on all neurons except y 2.27 and cs_gold_share 0.9 (limit 2 / 3 for y), 0 decisions differ.
- net2 vs nets-x-site, 4 seeds, 100 rounds (ignored test `look_net2_against_nets_x_site`): plans 396/396 turns (nets-x-site 0); same city totals (11/10/11/9). War order, peace wish, guard and denied help are all 0 in these games, because part A's inputs and C/D readers are not merged: with stubs the levers sit at neutral. The executors that exist in my files (settlers) read the plan; C/D readers are not in this branch, so "executors see it" is checked only for the site head.

Left / not done
- Tactical hand-over: `unit_net::begin(..., scope)` and `LocalObservation::narrowed` for the decider (U-9 second half) wait for T0/T1 (`Scope`, `sees_hostile`) and part A. Tac2/Net2Tac currently play the old unit nets with the U-9 sensor; a placeholder.
- G4-G6 are not scripted. The wave-3 situations are not in this ai-stress.
- No NEEDS-FROM-B.md: nothing missing.
