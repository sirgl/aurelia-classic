# The AI and the truth of the world: every read of the authoritative world in AI code

**The Civ V AI sees the world only through its observation, and the types now hold that: `AiMind` no longer receives the `GameState`, the planners get an `AIObservation` or a `LocalObservation`, and the one door from a `PlayerView` to the authoritative world (`PlayerView::world`) needs a key that only the module's sensor code can name. Two real leaks were found in the Civ V reads and closed (antiquity sites on unexplored hexes, the victory-screen capitals); the AI plays the same games after the fix (20 games x up to 500 rounds, result hash identical). Lands & Laws and the sandbox are not converted (decision of the project owner, 2026-10-21: no L&L work in this window); their reads are counted and frozen by a ratchet test.** State of 2026-10-21, branch `ai/no-truth-reads`. Marks: [measured] = measured by me in this session, [doc] = taken from a document, not rechecked.

Contract (`classic/docs/ai.md`, `platform/docs/kernel-audit.md`): an AI player sees the world only through its observation (own units, cities and economy, what is visible now, the map as known through `PlayerView`, relations, offers addressed to it, culture, faith, tourism, publicly announced facts) and acts only through commands.

## 1. The boundary: what the types hold and what a test holds

| What | Held by |
|---|---|
| A planner of any module gets no game: `AiMind::start_turn`, `decide_unit`, `plan_end_of_turn` take the `Sight` / `Near` that only `sense` and `sense_unit` build (`kernel/src/ai/mind.rs`) | types, all modules |
| Civ V: `Sight = AIObservation`, `Near = LocalObservation`; `ClassicMind` has no path to `GameState` outside `sense*` (`classic-rules/src/ai/driver.rs`) | types |
| The way round the types, `PlayerView::world(key)`: the key type `Sensor` is `pub(in crate::knowledge)` in `classic-rules`, so code in `classic-rules/src/ai` cannot name it | types; a `compile_fail` doctest (`knowledge/mod.rs`) shows it |
| The remaining textual patterns (`.auth`, `.indexes()`, `.derived()`, `GameState` ...) in `classic-rules/src/ai` | guard test `classic-rules/tests/ai_truth_guard.rs`: zero reads, the allow-list holds only the sensors |
| L&L (`lal-rules/src/ai`) and the sandbox (`sandbox-rules/src/ai.rs`): `Sight = &Game`, the types hold nothing | ratchet in the same test: 235 reading lines in 25 files [measured], per-file counts can only go down; two keyed `world` reads in `lal-rules/src/ai/sites.rs` |
| The observation does not lie about hidden things | twin games: `aurelia-qa/tests/non_interference.rs`, `classic-rules/tests/knowledge.rs` |

The driver (the sensor) may read the truth: it builds the observation and executes commands. Sensors: `ClassicMind::sense/sense_unit`, `AIObservation::for_player`, `LocalObservation::for_unit`, `PlayerView` and `PlayerQueries` (`knowledge/public.rs`).

## 2. Classification of the Civ V reads

Verdicts: OWN, the player's own state; PUBLIC, a fact every player may know (with the reason: a screen of the interface); KNOWN, derivable from the player's knowledge; LEAK, hidden information; ORACLE, a question to the truth whose answer reveals something hidden.

### 2.1. Direct reads of the truth in `classic-rules/src/ai` (before the change)

| file:line (before) | Read | Verdict | Reason | Now |
|---|---|---|---|---|
| ai/driver.rs:83,116,136,162,214,242 | `game: &GameState` in six methods of `ClassicMind` | interface | a planner had the whole game | gone: `sense`/`sense_unit` build the observation, the rest takes it |
| ai/driver.rs:89 | `PlayerView::new(game.view().auth, ...)` for the routes of operations | KNOWN | the same `PlayerView` the observation holds | `operations::Routes::new(sight.view)` |
| ai/driver.rs:183 | `game.view().auth.players()`: kind of the own player | OWN | | `sight.view.own_kind()` |
| ai/great_people.rs:80 | `world().auth.cities().get(city)` and `engineer_may_hurry` | OWN | the player's city (owner checked) | `PlayerQueries::engineer_may_hurry(city)` |
| ai/greedy.rs, ai/value.rs | `game.view().auth` | INFRA | `mod tests` only | not counted |

9 direct reads outside tests (OWN 2, KNOWN 1, interface 6), no LEAK among them [measured by grep, before the change].

### 2.2. Reads through `PlayerView` / `PlayerQueries` (what the AI calls)

| Method (calls in the AI) | Reads | Verdict | Reason |
|---|---|---|---|
| `tile`, `is_known`, `is_visible`, `remembered_cities`, `known_pillaged`, `terrain_kinds`, `dimensions` | a snapshot of a tile: unknown gives nothing, remembered gives what was last seen | KNOWN | the one filter between the truth and the AI; the map size is PUBLIC (new-game screen, minimap) |
| `accepts` / `refusal` (9 sites: planner_cities, great_people x2, espionage, front x2, victory x3, trade; since 23 October also `ai/legal.rs` for every order, `strikes`, `carrier`, `operations::muster_jump`, `air`; and `ClassicGame::accepts_order`, the driver's last look before an order is sent) | `validate` of a command against the truth | ORACLE by construction, but the interface asks the same question (greyed buttons); commands name own units and cities, targets on visible tiles | residual risk: a move into a tile held by an unseen unit is accepted or not by the truth. The heavy twin (below) found no divergence in 2,003 turns [measured] |
| `attack_preview*`, `city_strike_*` | combat estimate | KNOWN | needs an own unit and a visible target (`is_visible(target)` in the body) |
| `race`, `original_capitals` | scores of met civilizations; the original capitals | scores PUBLIC (`classic-ui/src/game/victory.rs`: the player and every met civilization); capitals were a LEAK, see 3.2 | |
| `own_*` (14 calls) | own cities and units | OWN | owner checked in the body |
| `offer_problem`, `deal_item_problem`, `tradeable_luxuries`, `city_state_terms`, `trade_prospects`, `upgrade_offer`, `promotions_available`, `must_set_up_first` | trade screen, city-states, own units | OWN / PUBLIC | `classic-ui/src/ui/screen_diplomacy.rs`, `screen_city_states.rs`; meeting is checked in the body |

### 2.3. The observation builders (sensor, `classic-rules/src/knowledge/observation.rs`)

| Field | Reads | Verdict | Reason / example |
|---|---|---|---|
| `own_units`, `own_cities`, `economy`, `culture`, `spies`, `offers_to_us`, `faith.units` | own | OWN | |
| `visible_foreign_units`, `visible_foreign_cities`, `faith.cities` | foreign on visible tiles | KNOWN | filter `sees_unit` / `is_visible` |
| `ObservedCity.buildings` (count of buildings of a foreign city) | truth of a visible city | LEAK, small, removed | a human sees population and walls, not the number of buildings; no planner used it |
| `relations` | alliances, embassies, treaties with met civilizations | PUBLIC | `screen_diplomacy.rs`; filter "met or at war" |
| `built_wonders` | wonders in all cities | PUBLIC | every finished wonder is announced to all |
| `city_states` (`capital`) | capital of a met city-state, not filtered by exploration | KNOWN in use | only used to find a visible city (`espionage.rs`: `visible_foreign_cities.find(tile == capital)`) |
| `faith.overview.religions`, `tourism.influence` | founders and followers; culture of met civilizations | PUBLIC | `screen_religion.rs`; `classic-ui/src/game/culture.rs` |
| `tourism.sites` | antiquity sites of the whole map | LEAK, fixed (3.1) | |
| `congress`, `steal`, `victories`, map size | Congress screen, theft rights, match settings | PUBLIC / OWN | |

## 3. The leaks found and fixed (Civ V)

1. **Antiquity sites on unexplored hexes** (`derived::archaeology::open_sites` into `AIObservation.tourism.sites`). Hidden fact: which unexplored hexes carry antiquities. The AI archaeologist (`ai/tourism.rs`) walked to the nearest site of the whole list, across unknown ground. A human sees sites only on explored hexes. Fix: `AIObservation::for_player` keeps only the sites on hexes the player knows. Test: `the_observed_antiquity_sites_are_explored_hexes` (observation and the march of a digger over 8 seeds).
2. **The victory-screen capitals** (`PlayerQueries::original_capitals`, used by `ai/capital_search.rs` and `ai/grand_strategy.rs`). It listed a civilization's capital only once it was founded, gave the true holder of any capital and the tile of any explored hex. Hidden facts: when an unmet civilization founds its capital, who took it behind the fog, a capital founded after the hex was explored. Evidence: a twin game in which two other civilizations found a capital on hexes never explored changed the standings (`[]` against two standings) before the fix, not after (`the_capital_standings_do_not_show_the_unseen`) [measured]. Fix: one standing per living major civilization (a constant of the match), the founder as holder and no tile until the capital is in sight or was sighted (then the true or the sighted holder).
3. **Number of buildings of a visible foreign city** (`ObservedCity.buildings`): field removed.
4. Residual, not fixed: `accepts` (2.2). Not a measured leak; the twin games found no divergence.

## 4. Measurements [measured]

- Hidden-change twins: the same game twice, the turn of an AI player played on both, the worlds differ in places the player never looked (deposit of every hidden hex in two assignments, terrain of the hidden hexes rotated, four cities of not-yet-met civilizations and four units of any other civilization on different hidden hexes); observations, commands, memory and what each own unit senses around itself must be equal. 24 seeds x 120 rounds, every 4th round: 2,003 turns, 0 diverged. (Cities of civilizations the player has met are left out on purpose: a new luxury of a met civilization shows on the trade screen. Hidden hexes keep 5 hexes of distance from the explored area: the border of a new city is revealed legitimately when a unit steps next to it.) The first version of this twin (margin 3, cities of met civilizations) diverged in 17 of 2,825 turns; the ones I traced were test artefacts (trade items of met civilizations; a border revealed next to a hidden city, seed 1616 round 24: two scouts targets differed until the margin was raised). The "memory changed" ones were not traced one by one; they vanished with the same two changes.
- Same games before and after: `ai-tournament --suite dev --solo standard` (20 games, up to 500 rounds, 160 seats): result hash `91f55d434df5e160` before and after, the compared values equal. The two fixes change no game of this set.
- `ai-fitness --mode versus --seeds 8 --horizon 300` (16 games), origin/main binary against this branch with the same cached reference: balanced fitness -0.0274366 both (difference 4e-17), result hash `bb8d8e0deeb1e2cf` both.

## 5. Not done

- L&L and the sandbox: not on the contract (decision of the project owner). Counted and frozen: 235 reading lines.
- `accepts` oracle: documented, not removed.
- The guard test is a textual scan; it cannot see a read that goes through a new helper in another module of `classic-rules` that is itself reachable from `ai`. The key type closes the `PlayerView` door; a new public function in `knowledge` that returns truth would be a new door (review it).
