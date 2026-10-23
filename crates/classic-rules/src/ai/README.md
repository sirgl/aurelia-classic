# AI decision functions of the M4 mechanics

> Advisors of the M4 mechanics. The AI as a whole (chain, contract, variants, goal status): [classic/docs/ai.md](../../../../docs/ai.md).

Each mechanic brings a stateless advisor that the AI driver asks through the
plug-in registry (`ai::system::AdvisorRegistry`, at the end of an AI turn;
the classic module registers them in `classic::advisors`): `ai::policies::PolicyAdvisor` (`ClassicSystem::Policies`),
`ai::religion::ReligionAdvisor` (`ClassicSystem::Religion`) and
`ai::tourism::TourismAdvisor` (`ClassicSystem::Culture`). An advisor reads
only `AIObservation` (platform/structure.md §22: nothing hidden), returns ordinary
`Command`s as prioritized proposals (`Proposal { command, priority }`, not goals with
values: the allocator's goals and values are a separate layer, `classic/docs/ai-allocator.md`)
and keeps no state: the driver observes
again after executing, traces each proposal (`Reason::SystemAdvisor`) and
counts the core's refusals. Ties are broken by table order, so the same
observation gives the same commands. `tests/ai_track_b.rs` plays a 4-civ
game with the driver and checks that the AI adopts policies, founds
pantheons and a religion, and that the core refuses few proposals. That the
advisors do not see what the player has not seen is checked by
`integration/crates/aurelia-qa/tests/non_interference.rs` (policies, pantheons, a Great
Writer, an excavation: the same turn played in two worlds that differ only
in a deposit, a city and a unit where the player never looked) and, for the
whole driver, by `tests/knowledge.rs` and the twin turns of the aurelia-qa runner
(`invariants::check_non_interference`).

## Culture and policies — `ai::policies`

| Function | Returns | When to call |
|---|---|---|
| `policy_command(&obs, &rules, &AiState) -> Option<Command>` | `ChooseIdeology`, `AdoptPolicy` or `UnlockPolicyBranch` | once per turn of an AI player, before `EndTurn`; call again after executing to spend several free policies |

Reads `obs.culture` (`derived::culture::CultureOverview`: pool, next cost,
free policies and tenets, what may be unlocked, adopted or chosen now),
`obs.own_cities.len()` and `obs.relations` (war), the memory of foreign
cities (`AiState::seen_cities`) and the leader's personality. Strategy: an ideology as
soon as one is available (Order with 4+ cities, Autocracy at war, else
Freedom); otherwise complete the branch with the most adopted policies;
otherwise unlock the branch `ai::policy_branches` values most: every branch
is valued by the effects its policies declare against the situation (room for
cities, food of the land, nearest known rival, war, faith, the leader's
personality); no branch is named. See `classic/docs/ai-policies.md`.

## Faith and religion — `ai::religion`

| Function | Returns | When to call |
|---|---|---|
| `religion_commands(&obs, &rules) -> Vec<Command>` | `FoundPantheon`, `AddReformationBelief`, per Great Prophet `FoundReligion` / `EnhanceReligion` / `SpreadReligion` / `MarchTo` / `CreateGreatImprovement` (it first steps out of a city), per missionary `SpreadReligion` / `MarchTo` (only cities of its own land or of players granting entry), one `PurchaseWithFaith` (missionary, where `FaithOverview::missionary_purchase` says it is affordable) | once per turn of an AI player; the commands are independent, a refused one changes nothing |
| `religious_units(&obs, &rules) -> Vec<UnitHandle>` | the units `religion_commands` gives orders to | so that the unit tasking of the framework leaves them alone |

Reads `obs.faith` (`FaithObservation`: `derived::religion::FaithOverview` —
pool, prices, free beliefs and religions, public facts of every religion —
the religion and spreads of its own religious units, and the majority of the
cities it owns or sees now). The pantheon is the free belief of the highest
score on the empire's land and situation (`ai::pantheon`: one pass over the
tiles around the own cities once per game, effects of the compiled belief
valued by the planners' weights, shifted by the leader's personality and by
war, barbarians and a close neighbour); the other beliefs are chosen from short
preference lists, then the first free one.

## Great works, concerts, ideology — `ai::tourism`

| Function | Returns | When to call |
|---|---|---|
| `tourism_commands(&obs, &rules) -> Vec<Command>` | per Great Writer / Artist / Musician `CreateGreatWork` (next to an own city while a slot of its kind is free) or `MarchTo` the nearest own city; without a free slot `WriteTreatise` (writer, when the player makes culture) or `PerformConcert` / `MarchTo` next to the nearest known city of a civilization granting entry (musician); `SwitchIdeology` on a upheaval | once per turn of an AI player |

Reads `obs.tourism` (`derived::tourism::TourismObservation`: slots of each
type, tourism, influence on every met civilization, public opinion),
`obs.own_units`, `obs.view` (tile owners) and `obs.relations`. Artists without
a slot wait: their golden age belongs to track A. `tests/ai_track_b.rs`
(`ai_puts_a_great_writer_into_a_free_slot`) checks the slot choice in the
driver.

## Economy and society (M4, track A)

Pure functions over `AIObservation` (platform/structure.md §22–23): they read the
player's own state and its knowledge of the map (`PlayerView`), never the
truth, and return ordinary `Command`s. The tile and site rules they use are
the core's own (`world::builds::site`, `derived::tile_yields`) evaluated
over the knowledge (`world::facts::TileFacts`), so an order the AI picks is
an order the core accepts unless the world changed since the observation.
They keep no state.

The driver reaches them through the advisor registry (`ai::system`):
`ai::economy::EconomyAdvisor` is registered for `ClassicSystem::Economy` in
`classic::advisors`. The planner's workers (`planner_workers`)
check builds with the same core site rule.

| Module | Function | Returns | Call when |
|---|---|---|---|
| `ai::economy` | `EconomyAdvisor` (start of turn) | great person actions (priority 20), a happiness building (priority 10), trade routes and trade units (8); with priced happiness and trade (`PriceModel::Uses`, `allocator-econ2`) the allocator buys the buildings and the trade units | registry, before unit moves and production |
| `ai::economy` | `EconomyAdvisor` (end of turn) | city focus changes (priority 5) | registry, after production |
| `ai::happiness` | `happiness_commands(obs, rules)` | `EnqueueProduction` of the cheapest happiness building in the largest city while excess happiness < `NEW_CITY_UNHAPPINESS` | each AI turn (advisor) |
| `planner_strategy` | `happiness_allows_city(brief)` | expansion while one more city keeps the empire happy (replaces the 12-city cap) | strategy review |
| `ai::improvements` | `worker_commands(obs, rules)` | build or march order for every idle land worker | tools and tests; the driver's workers are the planner's |
| `ai::improvements` | `best_worker_job(obs, rules, worker)` | `WorkerJob { tile, improvement, score }` or None | to rank one worker's options |
| `ai::improvements` | `worker_command(worker, job)` | `BuildImprovement` on the spot, else `MarchTo` | to turn a chosen job into an order |
| `ai::citizens` | `focus_commands(obs, rules)`, `focus_commands_priced(obs, rules, prices)` | `SetCityFocus` where the city's focus should change (by the prices: `priced_focus`) | each AI turn (advisor) |
| `ai::citizens` | `city_focus(rules, city, gold_income)` | Food when starving, Production while a settler is built, Gold while the empire loses gold (until it earns `GOLD_COMFORT`), Food when small, else Default | to inspect or override |
| `planner_cities` | building choice | no new building maintenance at a deficit (unless it earns gold); the core's building rules (`PlayerView::own_building_site`) | each city order |
| `planner_cities` | gold reserve | purchases keep `DEFICIT_RESERVE_TURNS` of the deficit in the treasury (a treasury below zero disbands units) | each purchase |
| `ai::great_people` | `great_person_commands(obs, rules)` | `GreatPersonAction` for every own great person with a use now | each AI turn (advisor) |
| `ai::great_people` | `great_person_action(obs, rules, unit)` | golden age, discovery, hurry or its improvement on the spot, or None | to inspect or to move the unit to a better tile first |
| `ai::improvements` | `best_worker_job` (resources) | a known strategic resource, or a luxury the empire lacks, adds `STRATEGIC_CONNECTION` / `LUXURY_CONNECTION` to the improvement that connects it | worker jobs |
| `planner_cities` | unit choice | units only while `obs.economy.resources` (the core's ledger) has their copies available | each city order |
| `planner_value` | `resource_value` | strategic resources worth `STRATEGIC_RESOURCE_WEIGHT` × military weight | planner workers |
| `ai::trade` | `route_commands(obs)` | `EstablishTradeRoute` for every own trade unit with an offer (`OwnEconomy::trade_offers`): the best origin gold + 2 × science, or the food / production of an internal route | economy advisor, start of turn (priority 8) |
| `ai::trade` | `production_commands(obs, rules)` | a trade unit (caravan first) in the most productive city while routes are left, none idle or queued, and the empire has two cities, one of `TRADE_FROM_POPULATION` (8) citizens | economy advisor, start of turn (priority 8) |

Scores are deterministic integers (ties to the lower tile or id), so a
planner may sort them stably.
## Track C: combat, diplomacy, barbarians, city-states, victory

Every function reads an `AIObservation` (and the compiled rules) and
returns ordinary `Command`s; none reads the authoritative world except one
check in `ai::great_people` (`building_in_city`: it asks the core's `engineer_may_hurry` about an own city through `PlayerView::world()`, after checking that the city is the player's own). Numbers
come from the public queries of `PlayerView` (`knowledge/public.rs`): the
same combat previews, promotion lists, upgrade prices and deal checks the
interface shows a human player (`attack_preview`, `city_strike_damage`,
`promotions_available`, `upgrade_offer`, `must_set_up_first`,
`deal_item_problem`, `own_kind`). A rejected command is harmless
(validation refuses it).

## How the M6 driver calls them

| where | what |
|---|---|
| `classic::advisors` (`classic/advisors.rs`) | `combat::MilitaryAdvisor` (Military), `diplomacy::DiplomacyAdvisor` (Diplomacy) and `city_states::CityStatesAdvisor` (CityStates) are registered with the module's other advisors |
| `CityStatesAdvisor` | start of turn: `pledge_orders` (15), `tribute_orders` (5); end of turn: `gift_orders` (3), except under `PriceModel::Uses`, whose allocator buys `priced_gifts` |
| `congress::CongressAdvisor` (system Congress) | start of turn: `vote_orders` (8), `proposal_orders` (6) |
| `espionage::EspionageAdvisor` (system Espionage) | start of turn: `steal_orders` (9), `coup_orders` (7), `placement_orders` (5) |
| `victory::VictoryAdvisor` (system Victory) | start of turn: `spaceship_orders` (12); end of turn: `space_production` (4) |
| `AiDriver` stops | when `GameState::is_over()` (a victory nobody continued after) |
| `MilitaryAdvisor::start_of_turn` | `city_strikes` (priority 30), `stance_orders` (25), `ranged_orders` (20) — before the units move |
| `MilitaryAdvisor::end_of_turn` | `conquest_decisions` (30), `promotion_choices` (10), `upgrade_orders(obs, UPGRADE_RESERVE)` (5) |
| `DiplomacyAdvisor::start_of_turn` | civilizations only: `answer_offers` (40); every `PROPOSAL_INTERVAL` rounds (staggered by player) `proposals` without peace items, under `PriceModel::Uses` `luxury_swaps`, and `denunciations` (5); war and peace stay with `planner_strategy` |
| `ai/tactics.rs` (`immediate`, `siege_step`) | ranged units never charge: they set up (`must_set_up_first`) and shoot the target `combat::best_shot` rates best within `combat::ranged_range`; in a siege they stop at their range. Melee units attack only targets the rules allow (`attack_preview`) and while `OwnUnit::can_attack` |
| `AiDriver::observe` (`ai/driver.rs`) | the barbarian player plays `barbarians::barbarian_orders` (no strategy, no cities, no per-unit planning); a city-state skips the strategy (no war or peace of its own) |
| `TurnBrief::relations` (`ai/planner.rs`) | civilizations only: barbarians (always at war) and city-states do not enter war and peace planning |

## Combat — `ai/combat.rs`

| function | returns | when to call |
|---|---|---|
| `ranged_orders(obs, rules)` | `RangedAttack` for every own ranged unit (archers, siege, ships, aircraft) with a worthwhile shot, best score first | before moving units, before `melee_orders` |
| `melee_orders(obs, rules)` | `AttackUnit` / `AttackCity` for melee units whose best attack is not a likely defeat | after `ranged_orders` |
| `best_attack(obs, rules, unit)` | the best `AttackChoice` of one unit (target tile, score, preview) | tactical planners |
| `attack_score(preview)` | % of the defender's HP taken − 0.6 × % of own HP lost, +100 for a sure kill | ranking |
| `city_strikes(obs)` | one `CityStrike` per own city on the visible enemy it hurts most | every turn |
| `promotion_choices(obs, rules)` | `Promote` for every unit with enough experience (heal when below 50 HP, else the first available) | every turn |
| `upgrade_orders(obs, reserve)` | `UpgradeUnit` while the treasury stays above `reserve` | every few turns |
| `conquest_decisions(obs)` | `DecideCity`: annex cities of 4+ citizens, puppet the rest, never raze | right after a capture |
| `stance_orders(obs, rules)` | `SetUpRangedAttack` for siege units with a target in range | before `ranged_orders` |

| `ranged_range(rules, kind)` | range of a ranged unit kind (None: melee) | tactics |
| `best_shot(view, unit, targets)` | the best-rated shot of one ranged unit at the given tiles | tactics |

## Diplomacy — `ai/diplomacy.rs`

| function | returns | when to call |
|---|---|---|
| `accepts(obs, rules, offer)` | whether the sum of item values for us is not negative (gold and luxuries at face value, peace only after `MIN_WAR_ROUNDS` of war, wars against third parties never) | answering offers |
| `answer_offers(obs, rules)` | `RespondDeal` for every offer addressed to us | at the start of the turn |
| `proposals(obs, rules)` | one `ProposeDeal` per trusted civilization, in this order: embassy exchange, friendship after `FRIENDSHIP_MIN_CONTACT_ROUNDS` rounds of contact, a research agreement when affordable, mutual open borders, a defensive pact; peace after two `MIN_WAR_ROUNDS` of war | every few turns |
| `denunciations(obs)` | `Denounce` for civilizations whose warmonger memory passed `DENOUNCE_WARMONGER` | every few turns |

## Barbarians — `ai/barbarians.rs`

| function | returns | when to call |
|---|---|---|
| `barbarian_orders(obs, rules)` | the barbarian player's turn: `ranged_orders`, `melee_orders` (one attack per target), then for every other unit `Pillage` on a foreign improvement or `MarchTo` next to the nearest visible city or unit within `HUNT_RANGE` (8) reachable over known land; nothing in sight: the unit stays at its camp | every turn of the barbarian player (the driver does it) |
| `pillage_order(obs, rules, unit)` / `march_order(obs, rules, unit, scratch)` | one unit's pillage or march | building blocks |

## City-states — `ai/city_states.rs`

Reads `AIObservation::city_states` (the met city-states as their screen
shows them: influence, resting point, friend, ally, pledge, whether a
tribute would be paid, our quests).

| function | returns | when to call |
|---|---|---|
| `gift_orders(obs)` | one `GiftGold` (250, or 500 when rich) to the city-state worth courting most — a gold quest first, then the most influence below the alliance — while the treasury stays above `GIFT_RESERVE` | end of turn |
| `pledge_orders(obs)` | `PledgeProtection` for every friend not yet protected | start of turn |
| `buyout_orders(obs)` | at most one `BuyoutCityState` of a city-state we may buy out (Austria, allied five rounds, gold enough) | start of turn |
| `tribute_orders(obs)` | at most one `BullyGold` from a city-state that would pay and that we do not court | start of turn |

## Science victory — `ai/victory.rs`

| function | returns | when to call |
|---|---|---|
| `space_production(obs, rules)` | `EnqueueProduction` of the Apollo Program in the capital, then one spaceship part per city with an empty queue — only orders `PlayerView::accepts` | end of turn |
| `spaceship_orders(obs, rules)` | `BuildSpaceshipPart` for parts in the capital, `MarchTo` the capital for the others | start of turn |

## World Congress — `ai/congress.rs`

Reads `AIObservation::congress` (the Congress screen: host, session,
proposals, resolutions in force, our delegates left, the delegates the
World Leader needs).

| function | returns | when to call |
|---|---|---|
| `proposal_orders(obs)` | one `ProposeResolution` while we may propose: Scholars in Residence, else Cultural Heritage Sites, else Standing Army Tax (the first neither proposed nor in force) | start of turn |
| `vote_orders(obs)` | `CastVotes` for every proposal of the open session: elections for ourselves, yes to our own proposals and to research/culture/non-proliferation (and to an army tax while our army is small), no to the rest; repeals the reverse | start of turn |

## Espionage — `ai/espionage.rs`

Reads `AIObservation::spies` (our spies: rank, state, city, coup chance)
and `AIObservation::steal` (victims we hold steal rights on and what may
be stolen).

| function | returns | when to call |
|---|---|---|
| `steal_orders(obs)` | one `StealTechnology` per victim: the first technology of the list (the lowest id, as the AI here takes it) | start of turn |
| `coup_orders(obs)` | `StageCoup` where the chance is at least `COUP_MIN_CHANCE` (60 %) | start of turn |
| `placement_orders(obs)` | `MoveSpy` for idle spies: the biggest visible city of a civilization, else a city-state allied to someone else, else our capital (counter-intelligence) — only where `PlayerView::accepts` | start of turn |
