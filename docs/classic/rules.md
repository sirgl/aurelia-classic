# Game rules of the classic set

> **Generated file, do not edit.** Made by `classic/tools/gen-rules-doc.py` from the project's own data: the rule constants of the Rust source (`classic/crates/classic-rules/src`, `platform/crates/kernel/src`) with their doc comments, and the JSON catalogs in `classic/docs/classic/`. Regenerate with `python3 -I classic/tools/gen-rules-doc.py`; `--check` fails when this file is stale (`tools/check-all.sh` runs it).
>
> **Sources.** The classic set is a rule set in the style of Unciv. Its rules and numbers may come only from (1) Unciv (open source, MPL-2.0; cited as `Unciv: <file>`) or (2) our own design (`own value`; `own design` for the systems Unciv lacks (BNW-DESIGN.md); `own value (unverified)` where it was not compared with a public source). See [SOURCING-POLICY.md](SOURCING-POLICY.md) and [UNCIV-ATTRIBUTION.md](UNCIV-ATTRIBUTION.md). The source column of every constant below is read from the constant's own doc comment; a comment without a recognised source clause is listed as `own value (source not recorded)`.
>
> **Formulas** are described in short sentences in our own words, with the numbers taken from the constants; the code is the authority. This file holds no pseudo-code or text of the original game.

## Summary

Rule constants: 506 (AI constants: 966; 14 constants of engine plumbing in `platform/crates/kernel/src/persistence/`, `platform/crates/kernel/src/test_module.rs`, `platform/crates/kernel/src/math.rs`, `platform/crates/kernel/src/module.rs`, `classic/crates/classic-rules/src/save_codec.rs`, `classic/crates/classic-rules/src/fixtures.rs` are not listed).

| Source | Rule constants | AI constants |
|---|---|---|
| Unciv | 260 | 0 |
| own design | 72 | 11 |
| own formula (expansion-only system; Unciv has no equivalent) | 0 | 0 |
| own value | 14 | 4 |
| own value (unverified) | 17 | 4 |
| own value (source not recorded) | 143 | 947 |
| total | 506 | 966 |

Sections:

- [Formulas](#formulas)
- [Rule constants by system](#rule-constants-by-system)
  - [Turn order and game length](#turn-order-and-game-length)
  - [Growth and citizens](#growth-and-citizens)
  - [Production, purchase and processes](#production-purchase-and-processes)
  - [Gold and maintenance](#gold-and-maintenance)
  - [Happiness and resources](#happiness-and-resources)
  - [Culture, policies and borders](#culture-policies-and-borders)
  - [Science](#science)
  - [Golden ages](#golden-ages)
  - [Great people](#great-people)
  - [City combat strength](#city-combat-strength)
  - [Combat, experience and healing](#combat-experience-and-healing)
  - [Movement and vision](#movement-and-vision)
  - [Barbarians and ancient ruins](#barbarians-and-ancient-ruins)
  - [Religion](#religion)
  - [City-states](#city-states)
  - [Diplomacy](#diplomacy)
  - [Espionage](#espionage)
  - [Trade routes](#trade-routes)
  - [Tourism and archaeology](#tourism-and-archaeology)
  - [World Congress](#world-congress)
  - [Victory and score](#victory-and-score)
  - [Map, terrain and tile yields](#map-terrain-and-tile-yields)
  - [Players](#players)
  - [Content and effect model](#content-and-effect-model)
  - [Other rule constants](#other-rule-constants)
- [AI constants](#ai-constants)
- [Data catalogs](#data-catalogs)

## Formulas

Short descriptions in our own words of the main formulas; the code is the authority (the function named in each item). Every number below is the value of the named constant.

- **Growth** (`systems::growth::growth_threshold`). A city of size n needs 15 (`FOOD_TO_NEXT_POPULATION_BASE`) food plus 8 (`FOOD_TO_NEXT_POPULATION_PER_POP`) for every citizen beyond the first, plus the integer square root of the cube of (n - 1). An AI player's threshold is then scaled by its difficulty level's growth percent.
- **Technology cost** (`derived::research::tech_cost`). The technology's catalog cost is scaled by the difficulty level's research percent and the map size's research percent, multiplied by (1 + the map size's per-city percent times (cities - 1)), counting the most cities the player ever had, and divided by (1 + 30 (`KNOWN_DISCOUNT_PERCENT`) percent times the share of living nations that the player has met and that know the technology); the result is truncated.
- **Policy cost** (`derived::culture::policy_cost`). 25 (`POLICY_CULTURE_COST_BASE`) culture plus (3 (`POLICY_CULTURE_COST_PER_POLICY`) times the policies already paid) raised to the power 2.01 (`POLICY_CULTURE_COST_EXPONENT`); the cost modifiers (never more than 75 (`POLICY_COST_MAX_DISCOUNT_PERCENT`) percent off) and the difficulty percent apply, the result is multiplied by (1 + the map size's per-city percent times (cities - 1)), rounded to the nearest whole and then down to a multiple of 5 (`POLICY_CULTURE_COST_ROUND_TO`).
- **Border growth by culture** (`derived::culture::border_cost`, table `kernel::world::city::CULTURE_COSTS`). A city that has claimed t tiles beyond its first ring needs 6.0 (`CULTURE_TO_NEXT_TILE_FACTOR`) times (t + 1.4813 (`CULTURE_TO_NEXT_TILE_OFFSET`)) raised to the power 1.3 (`CULTURE_TO_NEXT_TILE_EXPONENT`) culture for its next tile, rounded to the nearest whole. Modifiers can lower the cost by at most 85 (`CULTURE_TO_NEXT_TILE_MAX_DISCOUNT_PERCENT`) percent.
- **Tile purchase** (`kernel::world::territory::tile_purchase_offer`). A tile at distance d (at most 3 (`PURCHASE_RADIUS_MAX`)) from the city costs 50 (`PLOT_BASE_COST`) gold times (d - 1) plus 5 (`PLOT_COST_PER_PLOT_BOUGHT`) gold for every tile the city has claimed beyond its first ring (by culture or purchase); the city's tile purchase percents then apply, multiplied one after the other (America -50 %, Angkor Wat -25 %), and a tile is never free.
- **Item gold price** (`kernel::world::city`). The gold price of a unit or building is (30 (`GOLD_PER_PRODUCTION`) times its production cost) raised to the power 0.75, times the item's own percent (its `hurry_cost_modifier`: a settler, a horseman or a catapult +20, a caravel +30, a monument +40; a wonder is not for sale) and the purchase percents of the city, each source one factor (multiplied one after the other, as Unciv does: -25 % and -15 % leave 63.75 %, not 60 %), rounded down to a multiple of 10 (`PURCHASE_DIVISOR`) once, at the end; the production already invested does not lower it.
- **Combat damage** (`combat::damage`). The stronger side's strength divided by the weaker's gives a ratio r; the damage factor is the mean of ((r + 3) / 4) to the fourth power and 1 (its inverse for the weaker side). A unit takes that factor times a base of 2400 (`MELEE_EQUAL_STRENGTH_BASE_DAMAGE`) hundredths of a hit point plus a random part up to 1200 (`MELEE_EQUAL_STRENGTH_RANDOM_DAMAGE`), out of 100 (`UNIT_MAX_HP`) hit points, rounded to the nearest hit point with no minimum. A wounded unit's damage drops by a percentage equal to 33 (`WOUNDED_DAMAGE_PERCENT`) percent of its missing hit points (cities never weaken). A civilian that is attacked loses 40 (`DAMAGE_TO_CIVILIAN_UNIT`) hit points.
- **Healing** (`systems::combat::heal`). A unit that rests heals 25 (`HEALING_IN_CITY_CENTER`) hit points in a city, 20 (`HEALING_FRIENDLY_TERRITORY`) on its own or a friendly player's land, 10 (`HEALING_NEUTRAL_TERRITORY`) on neutral land and 10 (`HEALING_ENEMY_TERRITORY`) on enemy land, plus the bonuses of its promotions and of nearby units.
- **Unit upkeep** (`derived::maintenance::unit_maintenance`). Every player keeps 3 (`FREE_UNITS`) units free, plus the difficulty's, the policies' and the garrison free units. With n paid units and p the turn over 500 (`ESTIMATED_END_TURN`) (at most 1), the gold per turn is (50 (`GOLD_PER_UNIT_TIMES100`) hundredths times n times (1 + 1 (`UNIT_COST_GROWTH_FACTOR`) p)) raised to the power (1 + p / 3 (`UNIT_COST_EXPONENT_DIVISOR`)), then the difficulty's unit cost percent applies.
- **Game speed** (`kernel::rules::speeds`, table below). Every rule constant is written at Standard speed; a game of another speed multiplies each cost, length and interval Unciv scales by the speed's percent of the matching kind (production, gold, science, culture, faith, build time, pace, golden age, gold gift), takes its own peace, deal and pressure numbers, and plays to the last round of its calendar (Standard 500 (`STANDARD_MAX_ROUNDS`)).
- **Golden age** (`systems::golden_age::golden_age_threshold`). The first golden age needs 500 (`GOLDEN_AGE_BASE_THRESHOLD`) happiness, every later one 250 (`GOLDEN_AGE_THRESHOLD_INCREASE`) more; the threshold grows by 1 (`GOLDEN_AGE_THRESHOLD_CITY_PERCENT`) percent per city, truncated. A golden age lasts 10 (`UNMODIFIED_GOLDEN_AGE_TURNS`) turns before modifiers.
- **Elimination** (`systems::elimination::forget_eliminated`). A nation or city-state that loses its last city is eliminated, and every tie the rules keep with it ends at once, both ways: wars, peace treaties, deals and gold per turn, offers, embassies, open borders, pacts, friendships, research agreements, the memory of it (warmonger, denunciation), its standing with the city-states and the quests of it or about it, its roles in the World Congress (its proposals and votes, an embargo on it; a dead host passes the league to the living nation with the most delegates), its spies and the World Leader title. A city-state whose ally it was takes its next ally at once. What it did stays as history (great works, archaeology, a religion it founded).

## Rule constants by system

Every `pub const` of the rule code, by system. Value as written in the code; meaning is the first sentence of the doc comment; the source is the doc comment's first source clause.

### Turn order and game length

7 constants (Unciv 1, own value (unverified) 1, own value (source not recorded) 5). Files: `classic/crates/classic-rules/src/classic/mod.rs`, `classic/crates/classic-rules/src/classic/systems.rs`, `classic/crates/classic-rules/src/classic/turn.rs`, `platform/crates/kernel/src/turn/calendar.rs`, `platform/crates/kernel/src/turn/resolution.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `CLASSIC_FINGERPRINT` | `9970789956216259444` | The fingerprint of the current content (`CompiledRuleset::hash`). | own value (source not recorded) | `mod.rs` |
| `DEFICIT_DISBAND_THRESHOLD` | `-5` | Treasury balance at or below which a deficit may disband a unit (own value, not compared with a public source (unverified)). | own value (unverified) | `systems.rs` |
| `SCHEDULE` | `TurnSchedule { end_turn: &[ (Start, Step::Kernel(K::HealUnits)), (Start, Step::Module(M...` | The order of a classic turn (the tables in the module doc). | own value (source not recorded) | `turn.rs` |
| `START_YEAR` | `-4000` | The year of round 0 (4000 BC; negative years are BC). | Unciv: Speeds.json (`startYear` -4000), equal | `calendar.rs` |
| `ALL` | `[ KernelStep::HealUnits, KernelStep::RestoreMoves, KernelStep::CapitalConnections, Kern...` |  | own value (source not recorded) | `resolution.rs` |
| `ALL` | `[ KernelCityStep::Growth, KernelCityStep::Production, KernelCityStep::Borders]` |  | own value (source not recorded) | `resolution.rs` |
| `TURN_START_SEQUENCE` | `[StartStep::Fortification, StartStep::MoveOrders]` | What runs when a player's turn begins (after the previous player ended its turn): units with a multi-turn order move at turn start. | own value (source not recorded) | `resolution.rs` |

### Growth and citizens

7 constants (Unciv 4, own value (source not recorded) 3). Files: `classic/crates/classic-rules/src/systems/growth.rs`, `platform/crates/kernel/src/derived/citizens.rs`, `platform/crates/kernel/src/world/citizens.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `FOOD_TO_NEXT_POPULATION_BASE` | `15` | Food a city of size 1 needs to grow (the constant term of `growth_threshold`). | Unciv: CityPopulationManager.getFoodToNextPopulation; equal | `growth.rs` |
| `FOOD_TO_NEXT_POPULATION_PER_POP` | `8` | Food added to the threshold per citizen beyond the first (the linear term of `growth_threshold`). | Unciv: same function; equal | `growth.rs` |
| `SETTLER_MIN_CITY_SIZE` | `2` | Least city size at which a settler can be trained. | Unciv: Units.json (Settler, `Requires at least [2] population`), equal | `growth.rs` |
| `FOOD_PER_CITIZEN` | `2` | Food each citizen eats. | Unciv: logic/city/CityStats.kt (2 food per citizen), equal | `citizens.rs` |
| `ALL` | `[ CityFocus::Default, CityFocus::Food, CityFocus::Production, CityFocus::Gold, CityFocu...` |  | own value (source not recorded) | `citizens.rs` |
| `AVOID_GROWTH_BIT` | `0x80` | The bit of the saved focus byte that holds `CitizenPlan::avoid_growth`. | own value (source not recorded) | `citizens.rs` |
| `MAX_FORCED_SPECIALISTS` | `32` | Upper bound of hand-assigned specialists of one kind (Classic slots per building are 1–3; a city has a few buildings per kind). | own value (source not recorded) | `citizens.rs` |

### Production, purchase and processes

22 constants (Unciv 7, own value 2, own value (unverified) 2, own value (source not recorded) 11). Files: `classic/crates/classic-rules/src/command/validate.rs`, `classic/crates/classic-rules/src/derived/grants.rs`, `classic/crates/classic-rules/src/systems/processes.rs`, `platform/crates/kernel/src/world/builds.rs`, `platform/crates/kernel/src/world/city.rs`, `platform/crates/kernel/src/world/city_economy.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `NOT_YOUR_TURN` | `"not your turn"` | The rejection of a command of a player who is not on turn. | own value (source not recorded) | `validate.rs` |
| `ALL` | `[ ChoiceBlock::NothingOwed, ChoiceBlock::NotAnOption, ChoiceBlock::NoCapital, ChoiceBlo...` |  | own value (source not recorded) | `grants.rs` |
| `PROCESS_PERCENT` | `25` | Share (percent) of a city's production that a process turns into gold or science. | own value (unverified) | `processes.rs` |
| `REPAIR_WORK_TIME` | `300` | Work time of repairing (hundredths of worker-turns): building a pillaged improvement again repairs it. | own value (source not recorded) | `builds.rs` |
| `CHOP_FALLOFF_PER_TILE` | `5` | Classic chop production fall-off: −5 per tile beyond 2 from the city. | own value (source not recorded) | `builds.rs` |
| `CHOP_FULL_DISTANCE` | `2` |  | own value (source not recorded) | `builds.rs` |
| `BASE_WORK_RATE` | `100` | Work rate of a worker: hundredths of worker-turns a turn. | own value (source not recorded) | `builds.rs` |
| `CITY_WORK_RADIUS` | `3` | A city works tiles up to 3 rings away. | Unciv: models/ModConstants.kt (`cityWorkRange` 3), equal | `city.rs` |
| `MAX_WORKED` | `37` | Tiles in 3 rings, centre included. | own value (source not recorded) | `city.rs` |
| `MAX_SPECIALIST_KINDS` | `8` | Specialist kinds a city tracks (Classic has 7; the compiler bounds it). | own value (source not recorded) | `city.rs` |
| `MAX_TERRITORY_RADIUS` | `5` | Farthest tile a city's territory reaches. | Unciv: models/ModConstants.kt (`cityExpandRange` 5), equal | `city.rs` |
| `MAX_QUEUE` | `16` | Order queue limit (spam protection; Nation has none; we will raise it when the UI shows the need). | own value (source not recorded) | `city.rs` |
| `CULTURE_RADIUS_MAX` | `5` | Maximum culture growth radius (5 rings; rings 4-5 are a buffer territory the city does not work, as in the rules). | Unciv: models/ModConstants.kt (`cityExpandRange` 5), equal | `city.rs` |
| `PURCHASE_RADIUS_MAX` | `3` | Radius for buying tiles with gold (Classic: only within 3 of the centre). | own value (source not recorded) | `city.rs` |
| `CITY_DEFENSE_BASE` | `5` | The AI strategy's rough estimate of a city's defense (base + citizens); the combat value is `combat::city::city_strength`. | own value | `city.rs` |
| `GOLD_PER_PRODUCTION` | `30` | Factor on the production cost inside the item price formula `(30 P)^0.75` (the form is also Unciv's gold cost of an item; see classic/docs/classic/SOURCES.md). | Unciv: models/ruleset/IConstruction.kt (`(30 cost)^0.75`), equal | `city.rs` |
| `PURCHASE_DIVISOR` | `10` | Item prices are rounded down to a multiple of this. | Unciv: models/ruleset/IConstruction.kt (rounded down to 10), equal | `city.rs` |
| `CITY_BASE_HIT_POINTS` | `200` | Hit points of a city without buildings (walls, castles and the rest add theirs, `crate::combat::city`). | own value (unverified) | `city.rs` |
| `HURRY_RATE` | `2` | Urgency surcharge (Nation): buying out costs x2 of the missing points. | own value | `city.rs` |
| `CULTURE_COST_LEN` | `128` | Cost of the city's next territory tile (border growth): `round(6 (t + 1.4813)^1.3)`, t is how many tiles the city has already claimed beyond its first ring (by culture and purchase together). | Unciv: CityExpansionManager.getCultureToNextTile, equal (Standard speed, no modifiers) | `city.rs` |
| `GAME_MAX_TURNS` | `500` | Game length for GameProgress of the legacy tile price `purchase_price` (Standard speed, 500 turns; only tests read that price: the game's tile price is `territory::tile_purchase_offer`, its length `SpeedValues::max_rounds`). | Unciv: Speeds.json (Standard, 500 turns), equal | `city.rs` |
| `UNLOCKED` | `[Process::Wealth, Process::Research]` | The processes a technology unlocks. | own value (source not recorded) | `city_economy.rs` |

### Gold and maintenance

11 constants (Unciv 10, own value (unverified) 1). Files: `classic/crates/classic-rules/src/derived/income.rs`, `classic/crates/classic-rules/src/derived/maintenance.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `ALLY_SCIENCE_PERCENT` | `25` | Percent of the science of a major's allied city-states that the major gets. | own value (unverified) | `income.rs` |
| `GOLD_PER_UNIT_TIMES100` | `50` | Gold per paid unit in hundredths. | Unciv: CivInfoStatsForNextTurn.getUnitMaintenance, equal (0.5) | `maintenance.rs` |
| `UNIT_COST_GROWTH_FACTOR` | `1` | How fast the unit cost grows with the turn: the base is multiplied by `1 + MULTIPLIER × p` and raised to `1 + p / EXPONENT_DIVISOR`, where p is the turn over the game's last round (the speed's; at most 1). | Unciv: CivInfoStatsForNextTurn.getUnitMaintenance, equal (1 and 3) | `maintenance.rs` |
| `UNIT_COST_EXPONENT_DIVISOR` | `3` | Unciv: CivInfoStatsForNextTurn.getUnitMaintenance, equal. | Unciv: CivInfoStatsForNextTurn.getUnitMaintenance, equal | `maintenance.rs` |
| `ESTIMATED_END_TURN` | `500` | The end turn of the Standard game (another speed: its `SpeedValues::max_rounds`). | Unciv: Speeds.json (Standard, 500 turns), equal | `maintenance.rs` |
| `FREE_UNITS` | `3` | Units every player keeps without gold upkeep, before the difficulty's and the policies' free units. | Unciv: CivInfoStatsForNextTurn.getUnitMaintenance, equal (3) | `maintenance.rs` |
| `ROAD_MAINTENANCE` | `1` | Gold per turn of a road tile and of a railroad tile. | Unciv: TileImprovements.json (Road, Railroad), equal | `maintenance.rs` |
| `RAILROAD_MAINTENANCE` | `2` | Unciv: TileImprovements.json (Railroad), equal. | Unciv: TileImprovements.json (Railroad), equal | `maintenance.rs` |
| `CONNECTION_BASE_GOLD_TIMES100` | `-100` | Gold of a city connected to the capital: a base, plus a share per citizen of the capital and per citizen of the connected city (hundredths). | Unciv: CityStats.kt (trade route gold `0.15 capital + 1.1 size - 1`), equal | `maintenance.rs` |
| `CONNECTION_CAPITAL_POP_TIMES100` | `15` | Unciv: CityStats.kt (connection gold), equal. | Unciv: CityStats.kt (connection gold), equal | `maintenance.rs` |
| `CONNECTION_CITY_POP_TIMES100` | `110` | Unciv: CityStats.kt (connection gold), equal. | Unciv: CityStats.kt (connection gold), equal | `maintenance.rs` |

### Happiness and resources

15 constants (Unciv 12, own value (unverified) 3). Files: `classic/crates/classic-rules/src/derived/happiness.rs`, `classic/crates/classic-rules/src/derived/resources.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `UNHAPPINESS_FROM_CITY_TIMES100` | `300` | Unhappiness of each normal city (hundredths). | Unciv: CityStats.kt, equal (3) | `happiness.rs` |
| `UNHAPPINESS_FROM_ANNEXED_CITY_TIMES100` | `500` | Unhappiness of each occupied city before the flat occupation term (hundredths): 3 for a city and 2 more for an annexed one, scaled like a city's. | Unciv: CityStats.kt, equal (5) | `happiness.rs` |
| `OCCUPIED_CITY_UNHAPPINESS_TIMES100` | `200` | Flat unhappiness of each occupied city, not scaled by any percent (hundredths). | Unciv: CityStats.kt ("Occupied City" -2), equal | `happiness.rs` |
| `UNHAPPINESS_FROM_CITIZENS_TIMES100` | `100` | Unhappiness of each citizen of a normal city (hundredths). | Unciv: CityStats.kt, equal (1) | `happiness.rs` |
| `UNHAPPINESS_FROM_ANNEXED_CITIZENS_TIMES100` | `200` | Unhappiness of each citizen of an occupied city (hundredths): twice a normal citizen's. | Unciv: CityStats.kt (`unhappinessFromCitizens *= 2` for an annexed city), equal | `happiness.rs` |
| `NATURAL_WONDER_HAPPINESS` | `1` | Happiness of each natural wonder the player found. | own value (unverified) | `happiness.rs` |
| `VERY_UNHAPPY_THRESHOLD` | `-11` | Excess happiness at or below which the empire is very unhappy (below -10). | Unciv: GlobalUniques.json (`when below [-10] [Happiness]`), equal | `happiness.rs` |
| `VERY_UNHAPPY_PRODUCTION_PERCENT` | `-50` | Production of the cities of a very unhappy empire (percent); no gold penalty. | Unciv: GlobalUniques.json, equal (-50) | `happiness.rs` |
| `VERY_UNHAPPY_COMBAT_PERCENT` | `-33` | Combat strength of the units of a very unhappy empire (percent, flat). | Unciv: GlobalUniques.json, equal (-33) | `happiness.rs` |
| `REBELLION_THRESHOLD` | `-21` | Excess happiness at or below which rebels rise (below -20; `systems::revolts`). | Unciv: GlobalUniques.json (`Rebel units may spawn <when below [-20] [Happiness]>`), equal | `happiness.rs` |
| `UNHAPPY_GROWTH_PERCENT` | `-75` | Growth modifier (percent) of an unhappy (-1 to -10) and of a very unhappy empire (growth nullified). | Unciv: GlobalUniques.json, equal | `happiness.rs` |
| `VERY_UNHAPPY_GROWTH_PERCENT` | `-100` | Unciv: GlobalUniques.json, equal. | Unciv: GlobalUniques.json, equal | `happiness.rs` |
| `ALLY_LUXURY_HAPPINESS_PERCENT` | `150` | Percent of a luxury's happiness when allied city-states give it and the player has Cultural Diplomacy. | own value (unverified) | `happiness.rs` |
| `MISSING_RESOURCES_MALUS` | `-25` | Combat penalty (percent) of a unit whose owner is short of a strategic resource the unit requires (flat, whatever the shortfall). | Unciv: `battle/BattleConstants.kt` (`MISSING_RESOURCES_MALUS`, -25), applied in `BattleDamage.kt`; equal | `resources.rs` |
| `ALLY_RESOURCE_BONUS_PERCENT` | `200` | Percent of a city-state's resources that an ally with Cultural Diplomacy receives. | own value (unverified) | `resources.rs` |

### Culture, policies and borders

17 constants (Unciv 10, own design 1, own value 3, own value (source not recorded) 3). Files: `classic/crates/classic-rules/src/derived/culture.rs`, `classic/crates/classic-rules/src/world/culture.rs`, `platform/crates/kernel/src/world/territory.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `POLICY_CULTURE_COST_BASE` | `25` | Culture cost of the first policy. | Unciv: PolicyManager.getPolicyCultureCost, equal | `culture.rs` |
| `POLICY_CULTURE_COST_PER_POLICY` | `3` | Policies paid so far are multiplied by this before the exponent. | Unciv: PolicyManager.getPolicyCultureCost, equal | `culture.rs` |
| `POLICY_CULTURE_COST_EXPONENT` | `2.01` | Exponent of the policy cost curve (double precision, the power not truncated). | Unciv: PolicyManager.getPolicyCultureCost, equal | `culture.rs` |
| `POLICY_COST_MAX_DISCOUNT_PERCENT` | `-75` | Largest total discount of the policy cost (percent, negative). | own value | `culture.rs` |
| `POLICY_CULTURE_COST_ROUND_TO` | `5` | The policy cost is rounded to the nearest whole culture and then down to a multiple of this. | Unciv: PolicyManager.getPolicyCultureCost, equal | `culture.rs` |
| `CULTURE_TO_NEXT_TILE_FACTOR` | `6.0` | Culture cost of a city's next border tile, the factor of `factor × (claimed + offset)^exponent`. | Unciv: CityExpansionManager.getCultureToNextTile, equal (6) | `culture.rs` |
| `CULTURE_TO_NEXT_TILE_OFFSET` | `1.4813` | Added to the tiles claimed beyond the first ring before the exponent. | Unciv: CityExpansionManager.getCultureToNextTile, equal (1.4813) | `culture.rs` |
| `CULTURE_TO_NEXT_TILE_EXPONENT` | `1.3` | Exponent of the border tile cost curve. | Unciv: CityExpansionManager.getCultureToNextTile, equal (1.3) | `culture.rs` |
| `CULTURE_TO_NEXT_TILE_MAX_DISCOUNT_PERCENT` | `-85` | Largest total discount of the border tile cost (percent, negative). | own value | `culture.rs` |
| `ALL` | `[ PolicyBlock::UnknownPlayer, PolicyBlock::UnknownPolicy, PolicyBlock::NotChoosable, Po...` |  | own value (source not recorded) | `culture.rs` |
| `TENETS_TO_OPEN_NEXT_TIER` | `3` | Tenets of the tier below a tier needs before its first tenet may be taken: three. | own design | `culture.rs` |
| `ALL` | `[ BranchBlock::UnknownPlayer, BranchBlock::UnknownBranch, BranchBlock::Ideology, Branch...` |  | own value (source not recorded) | `culture.rs` |
| `ALL` | `[ IdeologyBlock::UnknownPlayer, IdeologyBlock::NotAnIdeology, IdeologyBlock::AlreadyCho...` |  | own value (source not recorded) | `culture.rs` |
| `RECENT_TURNS` | `8` | Turns of culture history kept per player (Great Writer, archaeology culture boost). | Unciv: `PolicyManager.kt` (`cultureOfLast8Turns`, 8); equal | `culture.rs` |
| `MIN_CITY_RANGE` | `2` | Minimal city range: no city within this many tiles of another city (one tile between two cities). | own value | `territory.rs` |
| `PLOT_BASE_COST` | `50` | Gold per ring beyond the first of a bought tile. | Unciv: CityExpansionManager.getGoldCostOfTile, equal (50) | `territory.rs` |
| `PLOT_COST_PER_PLOT_BOUGHT` | `5` | Gold added per tile the city has already claimed beyond its first ring (by culture and purchase). | Unciv: CityExpansionManager.getGoldCostOfTile, equal (5) | `territory.rs` |

### Science

2 constants (Unciv 1, own value (unverified) 1). Files: `classic/crates/classic-rules/src/derived/research.rs`, `classic/crates/classic-rules/src/systems/research.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `KNOWN_DISCOUNT_PERCENT` | `30` | The discount in percent when every living nation knows the technology. | Unciv: TechManager.getScienceModifier, equal (0.3) | `research.rs` |
| `MEDIAN_TECH_PERCENT` | `50` | Percent of the median research cost that a science boost gives, for a player without modifiers. | own value (unverified) | `research.rs` |

### Golden ages

4 constants (Unciv 4). Files: `classic/crates/classic-rules/src/systems/golden_age.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `UNMODIFIED_GOLDEN_AGE_TURNS` | `10` | Rounds of a golden age at Standard speed, before modifiers (the game speed's `golden_age_percent` scales it in `golden_age_length`). | Unciv: GoldenAgeManager.enterGoldenAge, 10; equal | `golden_age.rs` |
| `GOLDEN_AGE_BASE_THRESHOLD` | `500` | Happiness the first golden age needs, and the increase per golden age already had. | Unciv: GoldenAgeManager.happinessRequiredForNextGoldenAge, 500 and 250; equal | `golden_age.rs` |
| `GOLDEN_AGE_THRESHOLD_INCREASE` | `250` | Unciv: GoldenAgeManager.happinessRequiredForNextGoldenAge (250), equal. | Unciv: GoldenAgeManager.happinessRequiredForNextGoldenAge (250), equal | `golden_age.rs` |
| `GOLDEN_AGE_THRESHOLD_CITY_PERCENT` | `1` | Percent added to the threshold per city, the first included; the result is truncated, not rounded to 5. | Unciv: GoldenAgeManager.happinessRequiredForNextGoldenAge, equal | `golden_age.rs` |

### Great people

5 constants (Unciv 3, own design 1, own value (source not recorded) 1). Files: `classic/crates/classic-rules/src/derived/great_people.rs`, `classic/crates/classic-rules/src/systems/great_people.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `POINTS_FOR_NEXT_GREAT_PERSON` | `100` | Points of the first great person of a kind (Standard speed). | Unciv: GreatPersonManager, equal (100) | `great_people.rs` |
| `POINTS_FOR_NEXT_GREAT_PERSON_FACTOR` | `2` | The threshold doubles for each great person of the kind born (100, 200, 400, ...). | Unciv: GreatPersonManager (`pointsForNextGreatPersonCounter *= 2`), equal | `great_people.rs` |
| `MIN_GREAT_PERSON_GOLDEN_AGE_TURNS` | `3` | Shortest golden age a great artist can start, in turns: a guard for a rules file that gives the unit no length. | own design | `great_people.rs` |
| `ALL` | `[ GreatPersonBlock::NotAGreatPerson, GreatPersonBlock::WrongImprovement, GreatPersonBlo...` |  | own value (source not recorded) | `great_people.rs` |
| `TRADE_MISSION_INFLUENCE` | `30` | Influence a trade mission gives with a city-state. | Unciv: Units.json (Gods & Kings, Great Merchant, 30 Influence); equal | `great_people.rs` |

### City combat strength

10 constants (Unciv 10). Files: `classic/crates/classic-rules/src/combat/city.rs`, `classic/crates/classic-rules/src/systems/combat/city.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `CITY_STRENGTH_BASE` | `800` | Base strength of a city, ×100. | Unciv: `ModConstants.kt` (`cityStrengthBase` 8.0); equal | `city.rs` |
| `CITY_STRENGTH_PER_POP` | `40` | Strength per citizen, ×100. | Unciv: `ModConstants.kt` (`cityStrengthPerPop` 0.4); equal | `city.rs` |
| `CITY_STRENGTH_FROM_GARRISON_DIVISOR` | `500` | A garrison adds its strength × its health / this (a fifth). | Unciv: `ModConstants.kt` (`cityStrengthFromGarrison` 0.2); equal | `city.rs` |
| `CITY_HILL_STRENGTH` | `500` | Strength of a city on hills, ×100. | Unciv: `Terrains.json` (Hill, +5 Strength for cities built on it); equal | `city.rs` |
| `CITY_HEAL_PER_TURN` | `20` | Hit points a city heals per turn. | Unciv: `CityTurnManager.kt` (+20 at the end of the turn, nothing else); equal | `city.rs` |
| `CITY_RANGED_STRENGTH_PERCENT` | `75` | A city's ranged strike is this percent of its strength. | Unciv: `CityCombatant.kt` (`getAttackingStrength` × 0.75); equal | `city.rs` |
| `CITY_ATTACK_RANGE` | `2` | Range of a city's strike. | Unciv: `ModConstants.kt` (`baseCityBombardRange` 2); equal | `city.rs` |
| `CITY_CAPTURE_DAMAGE_PERCENT` | `50` | A captured city keeps at most this percent of its hit points. | Unciv: `CityConquestFunctions.kt` (health = max / 2); equal | `city.rs` |
| `CAPTURED_CITY_POPULATION_LOSS_DIVISOR` | `4` | Citizens a captured city loses: one plus one per this many citizens (a city of 1 loses none). | Unciv: `city/managers/CityConquestFunctions.kt` (`conquerCity`: `-1 - population / 4` when the populatio... | `city.rs` |
| `BARBARIAN_CAPTURE_RANSOM` | `200` | Gold barbarians take from the treasury of a city's owner when they capture it (at most the treasury). | Unciv: Battle.kt, 200; equal | `city.rs` |

### Combat, experience and healing

74 constants (Unciv 61, own value (unverified) 4, own value (source not recorded) 9). Files: `classic/crates/classic-rules/src/combat/damage.rs`, `classic/crates/classic-rules/src/combat/experience.rs`, `classic/crates/classic-rules/src/combat/strength.rs`, `classic/crates/classic-rules/src/combat/terrain.rs`, `classic/crates/classic-rules/src/systems/airlift.rs`, `classic/crates/classic-rules/src/systems/combat/actions.rs`, `classic/crates/classic-rules/src/systems/combat/air.rs`, `classic/crates/classic-rules/src/systems/combat/heal.rs`, `classic/crates/classic-rules/src/systems/combat/upgrade.rs`, `classic/crates/classic-rules/src/systems/combat/validate.rs`, `classic/crates/classic-rules/src/systems/paradrop.rs`, `classic/crates/classic-rules/src/systems/plunder.rs`, `platform/crates/kernel/src/effects/unit.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `MAX_HIT_POINTS` | `UNIT_MAX_HP as i64` | Unciv: logic/map/mapunit/MapUnit.kt (health 100), equal. | Unciv: logic/map/mapunit/MapUnit.kt (health 100), equal | `damage.rs` |
| `MELEE_EQUAL_STRENGTH_BASE_DAMAGE` | `2400` | Damage at equal strength, `24 + 12 × random`, in hundredths (melee and ranged alike). | Unciv: `BattleDamage.kt` (`damageModifier`); equal | `damage.rs` |
| `MELEE_EQUAL_STRENGTH_RANDOM_DAMAGE` | `1200` | Unciv: battle/BattleDamage.kt, equal. | Unciv: battle/BattleDamage.kt, equal | `damage.rs` |
| `RANGED_EQUAL_STRENGTH_BASE_DAMAGE` | `2400` | Unciv: battle/BattleDamage.kt, equal. | Unciv: battle/BattleDamage.kt, equal | `damage.rs` |
| `RANGED_EQUAL_STRENGTH_RANDOM_DAMAGE` | `1200` | Unciv: battle/BattleDamage.kt, equal. | Unciv: battle/BattleDamage.kt, equal | `damage.rs` |
| `WOUNDED_DAMAGE_PERCENT` | `33` | A wounded striker deals this percent less per missing hit point (1 % per 3 HP). | Unciv: `BattleConstants.kt` (`DAMAGE_REDUCTION_WOUNDED_UNIT_RATIO_PERCENTAGE` 300, i.e | `damage.rs` |
| `CITY_ATTACK_DAMAGE_PERCENT` | `100` | Factor (percent) of a city's damage to a unit and of melee damage to a city. | Unciv: `BattleDamage.kt` (no such factor, i.e | `damage.rs` |
| `MELEE_VS_CITY_DAMAGE_PERCENT` | `100` | Unciv: battle/BattleDamage.kt, equal. | Unciv: battle/BattleDamage.kt, equal | `damage.rs` |
| `DAMAGE_TO_CIVILIAN_UNIT` | `40` | Damage to a civilian that is attacked. | Unciv: `BattleConstants.kt` (`DAMAGE_TO_CIVILIAN_UNIT` 40); equal | `damage.rs` |
| `EXPERIENCE_PER_LEVEL` | `10` | Experience step of the promotion ladder (10, 30, 60, 100, ...). | Unciv: `map/mapunit/UnitPromotions.kt` (`xpForNextPromotion`); equal | `experience.rs` |
| `EXPERIENCE_ATTACKING_UNIT_MELEE` | `5` | Experience per combat: melee 5 to the attacker and 4 to the defender (attacking a city too), ranged 2 and 2 (3 against a city), air strikes 4 and 2. | Unciv: `battle/Battle.kt` (`addXp` after an attack); equal | `experience.rs` |
| `EXPERIENCE_DEFENDING_UNIT_MELEE` | `4` | Unciv: battle/Battle.kt (`addXp`), equal. | Unciv: battle/Battle.kt (`addXp`), equal | `experience.rs` |
| `EXPERIENCE_ATTACKING_CITY_MELEE` | `5` | Unciv: battle/Battle.kt (`addXp`), equal. | Unciv: battle/Battle.kt (`addXp`), equal | `experience.rs` |
| `EXPERIENCE_ATTACKING_UNIT_RANGED` | `2` | Unciv: battle/Battle.kt (`addXp`), equal. | Unciv: battle/Battle.kt (`addXp`), equal | `experience.rs` |
| `EXPERIENCE_DEFENDING_UNIT_RANGED` | `2` | Unciv: battle/Battle.kt (`addXp`), equal. | Unciv: battle/Battle.kt (`addXp`), equal | `experience.rs` |
| `EXPERIENCE_ATTACKING_CITY_RANGED` | `3` | Unciv: battle/Battle.kt (`addXp`), equal. | Unciv: battle/Battle.kt (`addXp`), equal | `experience.rs` |
| `EXPERIENCE_ATTACKING_UNIT_AIR` | `4` | Unciv: battle/Battle.kt (`addXp`), equal. | Unciv: battle/Battle.kt (`addXp`), equal | `experience.rs` |
| `EXPERIENCE_DEFENDING_UNIT_AIR` | `2` | Unciv: battle/Battle.kt (`addXp`), equal. | Unciv: battle/Battle.kt (`addXp`), equal | `experience.rs` |
| `EXPERIENCE_ATTACKING_CITY_AIR` | `4` | Unciv: battle/Battle.kt (`addXp`), equal. | Unciv: battle/Battle.kt (`addXp`), equal | `experience.rs` |
| `EXPERIENCE_DEFENDING_AIR_SWEEP_GROUND` | `2` | Experience of an interceptor that hits an air strike. | Unciv: `battle/AirInterception.kt` (2 when it deals damage); equal | `experience.rs` |
| `EXPERIENCE_ATTACKING_AIR_SWEEP` | `5` | Experience of both fighters of an air sweep dogfight. | Unciv: `battle/AirInterception.kt` (5 to both); equal | `experience.rs` |
| `MAX_XP_FROM_BARBARIANS` | `30` | Experience from fighting barbarians stops here. | Unciv: `ModConstants.kt` (`maxXPfromBarbarians`, 30); equal | `experience.rs` |
| `INSTANT_HEAL_HIT_POINTS` | `50` | Hit points restored by the instant-heal promotion. | Unciv: `UnitPromotions.json` (Heal Instantly, 50 HP); equal | `experience.rs` |
| `POINTS_FOR_NEXT_GREAT_GENERAL` | `200` | Experience points for the first great general and the rise of that threshold after each one (great admirals use the same values). | Unciv: `GreatPersonManager.kt` (`pointsForNextGreatGeneral` 200, +50); equal | `experience.rs` |
| `POINTS_FOR_NEXT_GREAT_GENERAL_INCREASE` | `50` | Unciv: GreatPersonManager.kt (+50), equal. | Unciv: GreatPersonManager.kt (+50), equal | `experience.rs` |
| `GREAT_GENERAL_BONUS_PERCENT` | `15` | Strength bonus (percent) of a unit near a great general or admiral (the largest bonus counts, they do not add). | Unciv: `Units.json` (Great General, "+15 % Strength bonus for Military Land units within 2 tiles"; Great... | `strength.rs` |
| `GREAT_GENERAL_BONUS_RANGE` | `2` | Range in tiles of a great general's bonus. | Unciv: `Units.json` (within 2 tiles); equal | `strength.rs` |
| `BASE_FLANKING_BONUS` | `10` | Flanking: percent per adjacent friendly unit. | Unciv: `BattleConstants.kt` (`BASE_FLANKING_BONUS`, 10); equal | `strength.rs` |
| `ATTACKING_ACROSS_RIVER_MALUS` | `-20` | Attack across a river, percent. | Unciv: `BattleConstants.kt` (`ATTACKING_ACROSS_RIVER_MALUS`, -20); equal | `strength.rs` |
| `LANDING_MALUS` | `-50` | Amphibious landing attack, percent. | Unciv: `BattleConstants.kt` (`LANDING_MALUS`, -50); equal | `strength.rs` |
| `FORTIFICATION_BONUS` | `20` | Fortification, percent per fortified turn. | Unciv: `BattleConstants.kt` (`FORTIFICATION_BONUS`, 20, times the fortified turns); equal | `strength.rs` |
| `SAPPED_WALLS_ATTACK_PERCENT` | `50` | Attack bonus (percent) against a city whose walls were sapped, and the range of a sapper's effect in tiles. | own value (unverified) | `strength.rs` |
| `SAPPER_EFFECT_RANGE` | `2` | (with `SAPPED_WALLS_ATTACK_PERCENT`) Attack bonus (percent) against a city whose walls were sapped, and the range of a sapper's effect in tiles. | own value (unverified) | `strength.rs` |
| `BARBARIAN_COMBAT_BONUS_BY_LEVEL` | `[75, 50, 40, 33, 25, 20, 10, 0]` | Bonus (percent) of every non-barbarian unit fighting barbarians, by the game's difficulty level (Settler .. | Unciv: `Difficulties.json` (`barbarianBonus` 0.75 / 0.5 / 0.4 / 0.33 / 0.25 / 0.2 / 0.1 / 0), read in `b... | `strength.rs` |
| `BARBARIAN_COMBAT_BONUS` | `33` | The bonus without a difficulty table (legacy fixtures): Prince's. | Unciv: `Difficulties.json`, Prince 0.33; equal | `strength.rs` |
| `MIN_STRENGTH` | `100` | Least strength after the modifiers, in hundredths (1.0): the modifier sum has no floor. | Unciv: `battle/BattleDamage.kt` (`max(1f, strength x modifier)`); equal | `strength.rs` |
| `HILLS_DEFENCE_BONUS` | `25` | Defense percent of hills and mountains (+25 %). | Unciv: `Terrains.json` (Hill, `defenceBonus` 0.25); equal for hills | `terrain.rs` |
| `ALL` | `[ AirliftBlock::UnknownUnit, AirliftBlock::UnknownCity, AirliftBlock::NotLandUnit, Airl...` |  | own value (source not recorded) | `airlift.rs` |
| `PILLAGE_HEAL_HP` | `25` | Hit points a unit heals by pillaging an improvement. | Unciv: `map/mapunit/actions/UnitActionsPillage.kt` (25, improvements only); equal | `actions.rs` |
| `MOUNTAIN_TURN_DAMAGE` | `50` | Hit points a unit that crosses mountains loses for ending its turn on one (the Phoenician Heritage text); it never dies of it. | Unciv: Nations.json (the mountain-crossing unique, 50 damage), equal | `actions.rs` |
| `NUKE_BLAST_RADIUS` | `2` | Unciv: battle/Nuke.kt (default blast radius 2), equal. | Unciv: battle/Nuke.kt (default blast radius 2), equal | `actions.rs` |
| `NUKE_UNIT_DAMAGE_BASE` | `30` | Unciv: battle/Nuke.kt (30 plus two rolls below 40), equal. | Unciv: battle/Nuke.kt (30 plus two rolls below 40), equal | `actions.rs` |
| `NUKE_UNIT_DAMAGE_RAND` | `40` | Unciv: battle/Nuke.kt (30 plus two rolls below 40), equal. | Unciv: battle/Nuke.kt (30 plus two rolls below 40), equal | `actions.rs` |
| `NUKE_NON_COMBAT_DEATH_THRESHOLD` | `6` |  | own value (source not recorded) | `actions.rs` |
| `NUKE_CITY_HIT_POINT_DAMAGE` | `50` | Unciv: battle/Nuke.kt (half the city's health), equal. | Unciv: battle/Nuke.kt (half the city's health), equal | `actions.rs` |
| `NUKE_LEVEL1_POPULATION_DEATH_BASE` | `30` | Unciv: battle/Nuke.kt (30 plus two rolls below 20 percent), equal. | Unciv: battle/Nuke.kt (30 plus two rolls below 20 percent), equal | `actions.rs` |
| `NUKE_LEVEL1_POPULATION_DEATH_RAND` | `20` | Unciv: battle/Nuke.kt (30 plus two rolls below 20 percent), equal. | Unciv: battle/Nuke.kt (30 plus two rolls below 20 percent), equal | `actions.rs` |
| `NUKE_LEVEL2_POPULATION_DEATH_BASE` | `60` | Unciv: battle/Nuke.kt (60 plus two rolls below 10 percent), equal. | Unciv: battle/Nuke.kt (60 plus two rolls below 10 percent), equal | `actions.rs` |
| `NUKE_LEVEL2_POPULATION_DEATH_RAND` | `10` | Unciv: battle/Nuke.kt (60 plus two rolls below 10 percent), equal. | Unciv: battle/Nuke.kt (60 plus two rolls below 10 percent), equal | `actions.rs` |
| `NUKE_LEVEL2_ELIM_POPULATION_THRESHOLD` | `5` | Unciv: battle/Nuke.kt (a strength-2 nuke destroys a city under 5 citizens), equal. | Unciv: battle/Nuke.kt (a strength-2 nuke destroys a city under 5 citizens), equal | `actions.rs` |
| `NUKE_FALLOUT_PROB` | `50` | Unciv: battle/Nuke.kt (fallout chance 0.5), equal. | Unciv: battle/Nuke.kt (fallout chance 0.5), equal | `actions.rs` |
| `CITADEL_CITY_STATE_INFLUENCE` | `15` | Influence a city-state loses with the player whose culture bomb takes one of its tiles. | Unciv: UniqueTriggerActivation.kt (`OneTimeTakeOverTilesInRadius`, `addInfluence(-15f)`); equal | `actions.rs` |
| `RAZING_CITY_EXTRA_DISTANCE` | `5` | Extra distance of a razing city when the culture bomb picks the city that gets the tiles. | Unciv: UniqueTriggerActivation.kt (`cityPriority`); equal | `actions.rs` |
| `REBASE_RANGE_PERCENT` | `200` | Rebase range of an air unit, in percent of its range. | Unciv: MapUnit.getAirUnitMovementRange, twice the range; equal | `air.rs` |
| `CITY_AIR_UNIT_CAPACITY` | `6` | Air slots of a city before buildings add more. | Unciv: ModConstants, cityAirUnitCapacity, 6; equal | `air.rs` |
| `ALL` | `[ AirOrderBlock::CannotSweep, AirOrderBlock::NoAttacksLeft, AirOrderBlock::OutOfRange,...` |  | own value (source not recorded) | `air.rs` |
| `HEALING_IN_CITY_CENTER` | `25` | Hit points a resting unit heals per turn in a city. | Unciv: MapUnit.rankTileForHealing, 25; equal | `heal.rs` |
| `HEALING_FRIENDLY_TERRITORY` | `20` | Heal per turn on own land, or land of a player granting open borders. | Unciv: MapUnit.rankTileForHealing, 20; equal | `heal.rs` |
| `HEALING_NEUTRAL_TERRITORY` | `10` | Heal per turn on unowned land or land of a player not at war. | Unciv: MapUnit.rankTileForHealing, 10; equal | `heal.rs` |
| `HEALING_ENEMY_TERRITORY` | `10` | Heal per turn on land of a player at war. | Unciv: MapUnit.rankTileForHealing, 10; equal | `heal.rs` |
| `BASE_UNIT_UPGRADE_COST` | `10` | Gold price of an upgrade: this base plus `UNIT_UPGRADE_COST_PER_PRODUCTION` per point of production cost the new unit has over the old, times the owner's upgrade cost modifiers, rounded down to 5 (no era term, exponent 1, standard speed). | Unciv: `models/ModConstants.kt` (`unitUpgradeCost`: base 10, perProduction 2, eraMultiplier 0, exponent... | `upgrade.rs` |
| `UNIT_UPGRADE_COST_PER_PRODUCTION` | `2` | Unciv: models/ModConstants.kt (`unitUpgradeCost`), equal. | Unciv: models/ModConstants.kt (`unitUpgradeCost`), equal | `upgrade.rs` |
| `EXOTIC_GOODS_GOLD_MIN` | `100` | Gold of a cargo sold, from 100 next to the capital to 400 at the far end of the map. | own value (unverified) | `validate.rs` |
| `EXOTIC_GOODS_GOLD_MAX` | `400` | (with `EXOTIC_GOODS_GOLD_MIN`) Gold of a cargo sold, from 100 next to the capital to 400 at the far end of the map. | own value (unverified) | `validate.rs` |
| `EXOTIC_GOODS_XP_MIN` | `5` | Experience of the sale on the same scale (unverified). | own value (source not recorded) | `validate.rs` |
| `EXOTIC_GOODS_XP_MAX` | `20` |  | own value (source not recorded) | `validate.rs` |
| `ALL` | `[ ParadropBlock::UnknownUnit, ParadropBlock::CannotParadrop, ParadropBlock::AlreadyMove...` |  | own value (source not recorded) | `paradrop.rs` |
| `PLUNDER_BASE_GOLD` | `20` | Flat part of the gold taken from a captured city. | Unciv: CityConquestFunctions.getGoldForCapturingCity, 20; equal | `plunder.rs` |
| `PLUNDER_GOLD_PER_POPULATION` | `10` | Gold taken per citizen of the captured city. | Unciv: CityConquestFunctions.getGoldForCapturingCity, 10; equal | `plunder.rs` |
| `CITY_PLUNDER_ROLL` | `40` | Bound of the one random roll added to the gold taken (the roll is below it). | Unciv: CityConquestFunctions.getGoldForCapturingCity, one roll below 40; equal | `plunder.rs` |
| `CITY_PLUNDER_FULL_AFTER_ROUNDS` | `50` | Rounds of holding after which a city gives full gold (less before). | Unciv: CityConquestFunctions.getGoldForCapturingCity, 50; equal | `plunder.rs` |
| `COMBAT_CONTEXTS` | `23` | Number of `CombatContext` variants. | own value (source not recorded) | `unit.rs` |
| `MAX_UNIT_STATS` | `40` | Capacity of per-stat arrays: the kernel's 25 unit stats and room for the module's (`Vocabulary::UNIT_STATS`). | own value (source not recorded) | `unit.rs` |
| `EMBARKED_UNIT_MOVEMENT` | `2` | Whole moves of an embarked unit before its extra embarked moves (Classic Classic: 2). | own value (source not recorded) | `unit.rs` |

### Movement and vision

19 constants (Unciv 5, own value 1, own value (unverified) 2, own value (source not recorded) 11). Files: `classic/crates/classic-rules/src/classic/unit.rs`, `platform/crates/kernel/src/derived/visibility.rs`, `platform/crates/kernel/src/navigation/cost.rs`, `platform/crates/kernel/src/navigation/profile.rs`, `platform/crates/kernel/src/world/unit.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `SAVED_SINCE` | `20` | The first codec version whose unit records carry the carrier. | own value (source not recorded) | `unit.rs` |
| `SIGHT_RADIUS` | `2` | Sight of a unit kind without a sight value (every unit sees 2). | Unciv: MapUnit.getVisibilityRange (2), equal | `visibility.rs` |
| `CITY_SIGHT_RADIUS` | `3` | Sight of a city (one ring further than a unit). | own value (source not recorded) | `visibility.rs` |
| `PLOT_SIGHT_RADIUS` | `1` | Every owned tile shows its neighbours to the owner. | own value (unverified) | `visibility.rs` |
| `HILLS_SIGHT_BONUS` | `1` | A unit on hills sees one ring further. | own value (source not recorded) | `visibility.rs` |
| `EMBARKED_SIGHT` | `1` | Sight of an embarked unit before its extra embarked sight. | own value (unverified) | `visibility.rs` |
| `MAX_SIGHT` | `7` | The farthest a unit sees, hills included (bounds the work per observer and the knowledge sync). | own value (source not recorded) | `visibility.rs` |
| `SIGHT_AREA` | `(1 + 3 * SIGHT_RADIUS * (SIGHT_RADIUS + 1)) as u64` | Tiles in a sight disc: 1 + 3R(R+1) = 19 at R=2. | own value (source not recorded) | `visibility.rs` |
| `LEGACY` | `{ let p = TerrainPalette::LEGACY; let mountains = 1 << p.code(Ground::Mountains); Self...` |  | own value (source not recorded) | `cost.rs` |
| `HILLS_EXTRA_MOVEMENT` | `1` | Extra movement points to enter hills (not a hill city): hills cost 2. | Unciv: `Terrains.json` (Hill `movementCost` 2), `MovementCost.kt` (a city centre costs 1); equal | `cost.rs` |
| `MOUNTAIN_MOVEMENT` | `3` | Whole movement points to enter a mountain for a unit that crosses them (the terrain row of the catalog holds the plot's graphical 1; the plot itself is the slowest ground). | own value (source not recorded) | `cost.rs` |
| `ROAD_MOVE_POINTS` | `30` | Road step in move points (30/60 = half a point); the owner's technologies change it (`RouteRules`: a third after the road-speed technology, 20/60). | Unciv: `logic/map/tile/RoadStatus.kt` (Road movement 0.5, improved 1/3), `MovementCost.kt` (a river cros... | `cost.rs` |
| `BASE` | `RouteRules { bridges: false, road_points: ROAD_MOVE_POINTS, }` | Before any technology. | own value (source not recorded) | `cost.rs` |
| `MAX_TERRAIN_RULES` | `6` | At most this many terrain rules per unit (double move, impassable). | own value (source not recorded) | `profile.rs` |
| `LAND` | `MoveProfile { domain: Domain::Land, embark: Embark::No, immobile: false, ignore_zone_of...` | A plain land unit without abilities. | own value (source not recorded) | `profile.rs` |
| `ROAD_WORK_TURNS` | `2` | Turns to build a road (B3): a mechanic parameter, not content (a road has no bonuses/prices, only passability; a content table is redundant). | own value (source not recorded) | `unit.rs` |
| `UNIT_MAX_HP` | `100` | Full health of a unit (own scale, 100). | Unciv: logic/map/mapunit/MapUnit.kt (health 100), equal | `unit.rs` |
| `MOVE_DENOMINATOR` | `60` | Move points per whole movement point (an own scale): a road step costs 30, a flat step 60. | own value | `unit.rs` |
| `MAX_FORTIFY_TURNS` | `2` | A fortified unit gets +20 % per turn, up to 2 turns. | Unciv: equal (`BattleConstants` fortification bonus 20, `UnitTurnManager` stops at 2) | `unit.rs` |

### Barbarians and ancient ruins

24 constants (Unciv 15, own value (unverified) 2, own value (source not recorded) 7). Files: `classic/crates/classic-rules/src/derived/ruins.rs`, `classic/crates/classic-rules/src/scenario/ruins.rs`, `classic/crates/classic-rules/src/systems/barbarians.rs`, `classic/crates/classic-rules/src/systems/ruins.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `ALL` | `[RuinsBlock::UnknownUnit, RuinsBlock::NotANation, RuinsBlock::NoRuinsHere, RuinsBlock::...` |  | own value (source not recorded) | `ruins.rs` |
| `LAND_TILES_PER_RUINS` | `80` | One ruin for this many tiles of land a unit can enter. | own value (source not recorded) | `ruins.rs` |
| `MIN_DISTANCE_FROM_START` | `4` | No ruin closer than this to a start, a city or a city-state's warrior. | own value (source not recorded) | `ruins.rs` |
| `MIN_DISTANCE_BETWEEN` | `4` | No ruin closer than this to another one. | own value (source not recorded) | `ruins.rs` |
| `FIRST_WAVE_DIVISOR` | `3` | The first barbarian turn places the target camp count divided by this (at least one). | Unciv: BarbarianManager.placeBarbarianEncampment, a third on the first turn; equal | `barbarians.rs` |
| `NEW_CAMP_ODDS` | `2` | A new camp is placed on one turn in this many (one in two). | Unciv: BarbarianManager.placeBarbarianEncampment, a coin flip per turn; equal | `barbarians.rs` |
| `MIN_DISTANCE_CITY` | `4` | A camp is never within this many tiles of a major nation's capital (other cities do not count). | Unciv: BarbarianManager.placeBarbarianEncampment, 4 tiles from major capitals; equal | `barbarians.rs` |
| `MIN_DISTANCE_CAMP` | `7` | Least distance (tiles) between two camps. | Unciv: BarbarianManager, 7 tiles from a live camp; equal | `barbarians.rs` |
| `MAX_NEARBY` | `2` | A camp spawns a wandering unit only while at most this many barbarian units stand within `NEARBY_RANGE` tiles (a third may come). | Unciv: BarbarianManager.spawnBarbarian (no spawn when more than 2 within 4 tiles); equal | `barbarians.rs` |
| `NEARBY_RANGE` | `4` | Unciv: BarbarianManager.spawnBarbarian (4 tiles), equal. | Unciv: BarbarianManager.spawnBarbarian (4 tiles), equal | `barbarians.rs` |
| `CAMP_GOLD` | `25` | Gold for clearing a camp at the default difficulty level. | Unciv: Difficulties.json (Gods & Kings ruleset), clearBarbarianCampReward of Prince, 25; equal | `barbarians.rs` |
| `EARLIEST_RELEASE_ROUND` | `10` | First round in which camps spawn wandering units; camps and their defenders appear from the first barbarian turn. | Unciv: BarbarianManager.spawnBarbarian (no wandering units before turn 10, a defender for an empty camp... | `barbarians.rs` |
| `SPAWN_DELAY_BASE` | `8` | Turns between two spawns of a camp: `SPAWN_DELAY_BASE + rand(SPAWN_DELAY_RANDOM)`, halved with raging barbarians, plus the difficulty's delay. | Unciv: `BarbarianEncampment.resetCountdown` (8 + 0..4, raging / 2, `Difficulties.json` `barbarianSpawnDe... | `barbarians.rs` |
| `SPAWN_DELAY_RANDOM` | `5` | Unciv: BarbarianEncampment.resetCountdown (0..4), equal. | Unciv: BarbarianEncampment.resetCountdown (0..4), equal | `barbarians.rs` |
| `SPAWN_DELAY_BY_LEVEL` | `[8, 5, 3, 0, 0, 0, 0, 0]` | Extra spawn delay by the game's difficulty level (Settler .. | Unciv: `Difficulties.json` (`barbarianSpawnDelay` 8 / 5 / 3 / 0 ...); equal | `barbarians.rs` |
| `CONVERSION_GOLD` | `25` | Gold of a converted camp unit (the victor's barbarian-conversion stat, the trait text: 25 gold). | Unciv: Nations.json (barbarian conversion, 25 gold), equal | `barbarians.rs` |
| `CLEARED_MEMORY_ROUNDS` | `30` | Cleared camps are remembered this long (city-state quests). | own value (source not recorded) | `barbarians.rs` |
| `GOLD` | `60` | Gold of the gold reward (and of a citizen without a city). | own value (unverified) | `ruins.rs` |
| `CULTURE` | `20` | Culture of the culture reward. | Unciv: `Ruins.json` ("discover cultural artifacts", 20 at standard speed); equal | `ruins.rs` |
| `FAITH` | `30` | Faith of the faith reward. | own value (unverified) | `ruins.rs` |
| `EXPERIENCE` | `10` | Experience of the unit. | Unciv: `Ruins.json` ("your exploring unit receives training", 10 XP); equal (Unciv offers it only from t... | `ruins.rs` |
| `MAP_RADIUS` | `4` | Tiles around the ruins the map reward shows. | Unciv: `Ruins.json` ("find a crudely-drawn map", radius 4); equal in radius (Unciv centres the disc on a... | `ruins.rs` |
| `REWARDS` | `[Reward::Citizen, Reward::Culture, Reward::Faith, Reward::Gold, Reward::Experience, Rew...` | Every reward, best first (the order the chooser prefers). | own value (source not recorded) | `ruins.rs` |
| `OFFERS` | `3` | How many rewards the ruins offer. | own value (source not recorded) | `ruins.rs` |

### Religion

17 constants (Unciv 13, own value (source not recorded) 4). Files: `classic/crates/classic-rules/src/derived/religion.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `PRESSURE_PER_POP` | `1000` | Pressure of the "no religion" creed per citizen of a city (a pantheon gets twice this per citizen when founded). | Unciv: logic/city/managers/CityReligionManager.kt (100 per citizen) and ReligionManager.kt (pantheon 200... | `religion.rs` |
| `FOUNDING_CITY_PRESSURE` | `5000` | Pressure a new religion gets in its founding city (× population). | Unciv: logic/nation/managers/ReligionManager.kt (500 per citizen); equal at the 10 times larger scale us... | `religion.rs` |
| `HOLY_CITY_PRESSURE` | `5` | Pressure a holy city adds to its own religion each turn, as a multiple of `ADJACENT_CITY_PRESSURE`. | Unciv: CityReligionManager.getAffectedBySurroundingCities, equal (5) | `religion.rs` |
| `ADJACENT_CITY_PRESSURE` | `60` | Pressure a city spreads to a city in range each turn, at Standard speed. | Unciv: Speeds.json (Standard, religiousPressureAdjacentCity 6), equal at the 10 times larger scale used... | `religion.rs` |
| `ADJACENT_CITY_DISTANCE` | `10` | Range in tiles within which a city spreads its religion. | Unciv: CityReligionManager.getSpreadRange, equal (10) | `religion.rs` |
| `MISSIONARY_PRESSURE_MULTIPLIER` | `10` | Pressure of a missionary action is its religious strength times this: the scale of this module. | Unciv: logic/map/mapunit/actions/ UnitActionsReligion.kt (pressure = religious strength); equal | `religion.rs` |
| `FIRST_PANTHEON_FAITH` | `10` | Faith for the first pantheon. | Unciv: ModConstants.pantheonBase, equal (10) | `religion.rs` |
| `NEXT_PANTHEON_FAITH` | `5` | Faith added to the pantheon price for each pantheon founded. | Unciv: ModConstants.pantheonGrowth, equal (5) | `religion.rs` |
| `FIRST_PROPHET_FAITH` | `200` | Faith for the first Great Prophet. | Unciv: ReligionManager.faithForNextGreatProphet, equal (200) | `religion.rs` |
| `NEXT_PROPHET_FAITH` | `100` | The step between two Great Prophet prices grows by this much each time. | Unciv: ReligionManager.faithForNextGreatProphet, equal (100) | `religion.rs` |
| `PROPHET_BASE_CHANCE` | `5` | Base chance (percent) of a Great Prophet appearing from faith each turn, plus one per faith above the price. | Unciv: logic/nation/managers/ ReligionManager.kt (`5 + stored faith - prophet cost`); equal | `religion.rs` |
| `FIRST_GREAT_PERSON_FAITH` | `1000` | Faith for the first great person bought with faith. | Unciv: Policies.json (Gods & Kings finishers: 1000, increasing by 500, from the Industrial era); equal | `religion.rs` |
| `NEXT_GREAT_PERSON_FAITH` | `500` | The step between two such prices grows by this much each time. | Unciv: Policies.json (500); equal | `religion.rs` |
| `INDUSTRIAL_ERA` | `4` | Order of the Industrial era: prophets stop spawning from faith, great people may be bought with faith. | own value (source not recorded) | `religion.rs` |
| `ALL` | `[ FaithBlock::UnknownPlayer, FaithBlock::UnknownUnit, FaithBlock::NotYourUnit, FaithBlo...` |  | own value (source not recorded) | `religion.rs` |
| `BONUS_BELIEF_CATEGORIES` | `[ BeliefCategory::Pantheon, BeliefCategory::Founder, BeliefCategory::Follower, BeliefCa...` | Categories a founding bonus belief may come from. | own value (source not recorded) | `religion.rs` |
| `ALL` | `[ FaithPurchaseBlock::UnknownCity, FaithPurchaseBlock::NotYourCity, FaithPurchaseBlock:...` |  | own value (source not recorded) | `religion.rs` |

### City-states

57 constants (Unciv 52, own value 1, own value (source not recorded) 4). Files: `classic/crates/classic-rules/src/scenario/city_states.rs`, `classic/crates/classic-rules/src/systems/city_states/block.rs`, `classic/crates/classic-rules/src/systems/city_states/buyout.rs`, `classic/crates/classic-rules/src/systems/city_states/gifts.rs`, `classic/crates/classic-rules/src/systems/city_states/mod.rs`, `classic/crates/classic-rules/src/systems/city_states/quests.rs`, `classic/crates/classic-rules/src/systems/city_states/turn.rs`, `classic/crates/classic-rules/src/world/city_states.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `CITY_STATE_MIN_DISTANCE` | `6` | No city-state closer than this to another city (start or city-state). | own value (source not recorded) | `city_states.rs` |
| `ALL` | `[ CityStateBlock::NotANation, CityStateBlock::NotACityState, CityStateBlock::NotMet, Ci...` |  | own value (source not recorded) | `block.rs` |
| `BUYOUT_COST` | `500` | Gold base price of buying out a city-state at Standard speed. | Unciv: logic/nation/diplomacy/CityStateFunctions.kt (`getDiplomaticMarriageCost`, 500); equal | `buyout.rs` |
| `BUYOUT_ROUNDS` | `5` | Rounds of alliance before the buyout is possible. | Unciv: Nations.json (Gods & Kings, Austria: allied for 5 turns); equal | `buyout.rs` |
| `SCRAP_GOLD_DIVISOR` | `20` | Scrap gold of a unit: its gold price `(30 × cost)^0.75` divided by this. | Unciv: models/ruleset/unit/BaseUnit.kt (`getDisbandGold`, / 20) and IConstruction.kt (`getBaseGoldCost`)... | `buyout.rs` |
| `GOLD_GIFT_EXPONENT` | `1.01` | Exponent and divisor of the influence a gold gift gives: `gold^1.01 / 9.8`. | Unciv: logic/nation/diplomacy/ CityStateFunctions.kt (`influenceGainedByGift`); equal | `gifts.rs` |
| `GOLD_GIFT_DIVISOR` | `9.8` | Unciv: CityStateFunctions.kt, equal. | Unciv: CityStateFunctions.kt, equal | `gifts.rs` |
| `ESTIMATED_END_ROUND` | `400` | Rounds of the approximate game progress that shrinks a gold gift by up to two thirds (Standard speed). | Unciv: CityStateFunctions.kt (`turns / (400 × speed)`); equal | `gifts.rs` |
| `GOLD_GIFT_MINIMUM` | `5` | Least influence a gold gift gives, and the step the gift is rounded down to. | Unciv: CityStateFunctions.kt (rounded down to 5, at least 5); equal | `gifts.rs` |
| `VISIBLE_DIVISOR` | `5` | Unciv: CityStateFunctions.kt, equal. | Unciv: CityStateFunctions.kt, equal | `gifts.rs` |
| `UNIT_GIFT_INFLUENCE` | `5` | Influence given by a gifted unit. | Unciv: logic/map/mapunit/actions/ UnitActions.kt (`getGiftActions`, +5); equal | `gifts.rs` |
| `IMPROVEMENT_GIFT_COST` | `200` | Gold cost of gifting a tile improvement. | own value | `gifts.rs` |
| `BULLY_GOLD_BASE` | `50` | Gold a bully takes: the base, plus `BULLY_GOLD_GROWTH` for every `BULLY_GOLD_INTERVAL_TIMES10` / 10 rounds (Standard speed). | Unciv: CityStateFunctions.kt (`goldGainedByTribute`: 50 + 5 per 6.5 turns, Speeds.json `cityStateTribute... | `gifts.rs` |
| `BULLY_GOLD_GROWTH` | `5` | Unciv: CityStateFunctions.kt, equal. | Unciv: CityStateFunctions.kt, equal | `gifts.rs` |
| `BULLY_GOLD_INTERVAL_TIMES10` | `65` | Unciv: CityStateFunctions.kt, equal. | Unciv: CityStateFunctions.kt, equal | `gifts.rs` |
| `CAN_BULLY_THRESHOLD` | `-30` | Influence below which a city-state cannot be bullied (-300 to the score). | Unciv: CityStateFunctions.kt (`getTributeModifiers`, -30); equal | `gifts.rs` |
| `BULLY_GOLD_DROP` | `-1500` | Influence lost by a successful bullying for gold, and for a worker (×100). | Unciv: CityStateFunctions.kt (`tributeGold` -15, `tributeWorker` -50); equal | `gifts.rs` |
| `BULLY_WORKER_DROP` | `-5000` | Unciv: CityStateFunctions.kt, equal. | Unciv: CityStateFunctions.kt, equal | `gifts.rs` |
| `CAN_PLEDGE_THRESHOLD` | `0` | Influence needed to pledge to protect; rounds after a withdrawn or broken pledge before a new one, rounds before a pledge may be withdrawn. | Unciv: logic/nation/diplomacy/CityStateFunctions.kt (influence at least 0, RecentlyWithdrewProtection 20... | `gifts.rs` |
| `PLEDGE_AFTER_BROKEN` | `20` | Unciv: CityStateFunctions.kt, equal. | Unciv: CityStateFunctions.kt, equal | `gifts.rs` |
| `PLEDGE_MIN_ROUNDS` | `10` | Unciv: CityStateFunctions.kt, equal. | Unciv: CityStateFunctions.kt, equal | `gifts.rs` |
| `PLEDGE_WITHDRAW_DROP` | `-2000` | Influence (×100) a major loses when it withdraws its pledge. | Unciv: CityStateFunctions.kt (`removeProtectorCiv`, -20); equal | `gifts.rs` |
| `TRIBUTE_GLOBAL_MODIFIER` | `100` | Highest global and local parts of the bully score. | Unciv: models/ModConstants.kt (`tributeGlobalModifier`, `tributeLocalModifier`, both 100); equal | `gifts.rs` |
| `TRIBUTE_LOCAL_MODIFIER` | `100` | Unciv: models/ModConstants.kt (`tributeLocalModifier` 100), equal. | Unciv: models/ModConstants.kt (`tributeLocalModifier` 100), equal | `gifts.rs` |
| `FRIENDS_THRESHOLD` | `30` | Influence at which a city-state counts a major as a friend and as an ally (the types carry the same). | Unciv: DiplomacyManager.relationshipLevel, 30 and 60; equal | `mod.rs` |
| `ALLIES_THRESHOLD` | `60` | Unciv: DiplomacyManager.relationshipLevel (60), equal. | Unciv: DiplomacyManager.relationshipLevel (60), equal | `mod.rs` |
| `AT_WAR_INFLUENCE` | `-60` | Effective influence at war, the floor. | Unciv: DiplomacyManager, MINIMUM_INFLUENCE, -60; equal | `mod.rs` |
| `ANCHOR_PROTECTED` | `10` | Shift of the resting point for a major the city-state is protected by, and for one it is wary of. | Unciv: logic/nation/diplomacy/ DiplomacyManager.kt (`getCityStateInfluenceRestingPoint`: protector +10,... | `mod.rs` |
| `ANCHOR_WARY` | `-20` | Unciv: DiplomacyManager.kt (wary -20), equal. | Unciv: DiplomacyManager.kt (wary -20), equal | `mod.rs` |
| `AGGRESSOR_THRESHOLD` | `2` | Attacks on city-states after which a major counts as an aggressor, and as a warmonger. | Unciv: logic/nation/Nation.kt (`isMinorCivAggressor` 2, `isMinorCivWarmonger` 4); equal | `mod.rs` |
| `WARMONGER_THRESHOLD` | `4` | Unciv: Nation.kt (`isMinorCivWarmonger` 4), equal. | Unciv: Nation.kt (`isMinorCivWarmonger` 4), equal | `mod.rs` |
| `WARY_AGGRESSOR_CHANCE` | `50` | Chance (percent) that an aggressor makes the city-state wary of it. | Unciv: CityStateFunctions.kt (`cityStateAttacked`, 50 %); equal | `mod.rs` |
| `FIRST_QUEST_ROUND` | `30` | First round a quest may start, and the random extra rounds added to it. | Unciv: logic/nation/managers/quests/QuestManager.kt (30, 20); equal | `quests.rs` |
| `FIRST_QUEST_RAND` | `20` | Unciv: QuestManager.kt, equal. | Unciv: QuestManager.kt, equal | `quests.rs` |
| `PERSONAL_MIN_BETWEEN` | `20` | Least rounds between two personal quests, between two global quests, and the random extra rounds. | Unciv: QuestManager.kt (20, 40, 25); equal | `quests.rs` |
| `GLOBAL_MIN_BETWEEN` | `40` | Unciv: QuestManager.kt, equal. | Unciv: QuestManager.kt, equal | `quests.rs` |
| `RAND_BETWEEN` | `25` | Unciv: QuestManager.kt, equal. | Unciv: QuestManager.kt, equal | `quests.rs` |
| `MAX_PERSONAL` | `2` | Active personal quests per nation and city-state. | Unciv: QuestManager.kt (2); equal | `quests.rs` |
| `CONTEST_LENGTH` | `30` | Rounds a contest quest lasts at Standard speed. | Unciv: Quests.json (Gods & Kings, duration 30); equal | `quests.rs` |
| `KILL_CAMP_RANGE` | `8` | Distance (tiles) from the city-state's capital within which a barbarian camp may be given as a quest. | Unciv: logic/nation/managers/quests/ QuestTargetHelpers.kt (8); equal | `quests.rs` |
| `RECENT_BULLY_ROUNDS` | `20` | Bullying this recent revokes personal quests (rounds). | own value (source not recorded) | `quests.rs` |
| `DROP_PER_TURN` | `-100` | Influence change per round, ×100: the decay above the resting point, its hostile and aggressor variants (hostile wins), and the recovery below it. | Unciv: logic/nation/diplomacy/DiplomacyManager.kt (`getCityStateInfluenceDegrade`: 1, 1.5, 2) and Diplom... | `turn.rs` |
| `DROP_PER_TURN_HOSTILE` | `-150` | Unciv: DiplomacyManager.kt and DiplomacyTurnManager.kt, equal. | Unciv: DiplomacyManager.kt and DiplomacyTurnManager.kt, equal | `turn.rs` |
| `DROP_PER_TURN_AGGRESSOR` | `-200` | Unciv: DiplomacyManager.kt and DiplomacyTurnManager.kt, equal. | Unciv: DiplomacyManager.kt and DiplomacyTurnManager.kt, equal | `turn.rs` |
| `RECOVERY_PER_TURN` | `100` | Unciv: DiplomacyManager.kt and DiplomacyTurnManager.kt, equal. | Unciv: DiplomacyManager.kt and DiplomacyTurnManager.kt, equal | `turn.rs` |
| `CONTACT_GOLD_FIRST` | `30` | Gold for the first major to meet a city-state, and for each later one; faith from a city-state that gives faith as a bonus (Religious). | Unciv: logic/nation/diplomacy/DiplomacyFunctions.kt (`makeNationsMeet`: 15 gold, doubled for the first m... | `turn.rs` |
| `CONTACT_GOLD_OTHER` | `15` | Unciv: DiplomacyFunctions.kt (`makeNationsMeet`), equal. | Unciv: DiplomacyFunctions.kt (`makeNationsMeet`), equal | `turn.rs` |
| `CONTACT_FAITH` | `4` | Unciv: DiplomacyFunctions.kt (`makeNationsMeet`), equal. | Unciv: DiplomacyFunctions.kt (`makeNationsMeet`), equal | `turn.rs` |
| `INTRUSION_PER_UNIT` | `-1000` | Influence lost (×100 per round) while any military unit of a non-friend stands on the city-state's land, whatever their number. | Unciv: logic/nation/diplomacy/CityStateFunctions.kt (`updateDiplomaticRelationshipForCityState`, -10); e... | `turn.rs` |
| `UNIT_SPAWN_BIAS` | `100` | Percent of the full interval the first gifted unit waits. | Unciv: logic/nation/diplomacy/DiplomacyTurnManager.kt (the first countdown is the full `N + -1..1` turns... | `turn.rs` |
| `SAME_RELIGION_DECAY_PERCENT` | `-25` | Percent change of the decay, and of the recovery, while the city-state's capital follows the major's religion as majority. | Unciv: logic/nation/diplomacy/DiplomacyManager.kt and DiplomacyTurnManager.kt (-25 % decay, +50 % recove... | `turn.rs` |
| `SAME_RELIGION_RECOVERY_PERCENT` | `50` | Unciv: DiplomacyTurnManager.kt (+50 % recovery), equal. | Unciv: DiplomacyTurnManager.kt (+50 % recovery), equal | `turn.rs` |
| `ALLY_GREAT_PERSON_BASE` | `37` | Great person gifts from allied city-states: base rounds, random extra rounds (0..6), the percent for the first gift, and the cap on the rounds each ally takes off (the gift comes once the count is below `min(allies, 10)`; the count runs only while there is an ally). | Unciv: logic/nation/diplomacy/CityStateFunctions.kt (`turnsForGreatPersonFromCityState`), managers/TurnM... | `turn.rs` |
| `ALLY_GREAT_PERSON_RANDOM` | `7` | Unciv: CityStateFunctions.kt, equal. | Unciv: CityStateFunctions.kt, equal | `turn.rs` |
| `ALLY_GREAT_PERSON_FIRST_PERCENT` | `50` | Unciv: CityStateFunctions.kt, equal. | Unciv: CityStateFunctions.kt, equal | `turn.rs` |
| `ALLY_GREAT_PERSON_MAX_ALLIES_CHANGE` | `-10` | Unciv: CityStateFunctions.kt, equal. | Unciv: CityStateFunctions.kt, equal | `turn.rs` |
| `ALL` | `[ Personality::Friendly, Personality::Neutral, Personality::Hostile, Personality::Irrat...` |  | own value (source not recorded) | `city_states.rs` |

### Diplomacy

15 constants (Unciv 5, own value 6, own value (source not recorded) 4). Files: `classic/crates/classic-rules/src/systems/diplomacy/deals.rs`, `classic/crates/classic-rules/src/systems/diplomacy/mod.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `MAX_DEAL_ITEMS` | `12` | At most this many items in one deal. | own value | `deals.rs` |
| `OFFER_ROUNDS` | `1` | An offer waits this many rounds for its answer. | own value | `deals.rs` |
| `EMBASSY_REVEAL_RADIUS` | `2` | An embassy reveals the tiles this close to the host's capital (our rule: public descriptions name the capital and the tiles around it, no number). | own value (source not recorded) | `deals.rs` |
| `ALL` | `[ DealBlock::NotOurDeal, DealBlock::GoldNeedsFriendship, DealBlock::NotEnoughGold, Deal...` |  | own value (source not recorded) | `deals.rs` |
| `MIN_WAR_ROUNDS_BEFORE_PEACE` | `10` | Rounds a war between two nations lasts before either may end it: no peace deal until then (the original game's ten turns). | own value (source not recorded) | `deals.rs` |
| `DEAL_DURATION` | `30` | Rounds a timed deal lasts at Standard speed: gold per turn, resources, open borders, defensive pacts, research agreements (another speed: `deal_duration`). | Unciv: Speeds.json (Gods & Kings ruleset), Standard dealDuration; equal | `mod.rs` |
| `PEACE_DURATION` | `10` | Rounds after a peace treaty during which no new war may start, at Standard speed (another speed: `peace_duration`). | Unciv: Speeds.json, Standard peaceDealDuration; equal | `mod.rs` |
| `RELATIONSHIP_DURATION` | `30` | Rounds a declaration of friendship or a denunciation lasts. | Unciv: logic/nation/diplomacy/DiplomacyManager.kt (both flags set to 30 turns, not scaled by speed); equal | `mod.rs` |
| `WARMONGER_ATTACKED_WEIGHT` | `250` | Warmonger weight of an attack on a major or a city-state (the same for both). | own value | `mod.rs` |
| `WARMONGER_DECAY` | `-5` | Change of the warmonger memory per round (negative: it fades). | own value | `mod.rs` |
| `WARMONGER_NUKE_WEIGHT` | `400` | Warmonger weight of a nuclear weapon of level 1; level 2 weighs twice. | own value (source not recorded) | `mod.rs` |
| `EXPECTED_CITY_COUNT` | `52` | Cities expected on a standard-size map; it scales the warmonger weight of a captured city. | own value | `mod.rs` |
| `WARMONGER_CITY_STATE_PERCENT` | `50` | Percent of the warmonger weight kept when the victim is a city-state. | own value | `mod.rs` |
| `RESEARCH_AGREEMENT_POOL_DIVISOR` | `3` | Divisor of the boost a research agreement gives (the smaller of the two partners' science during the agreement, / 3), and the base percent of it paid (50, plus the player's research-agreement percent). | Unciv: logic/nation/diplomacy/DiplomacyTurnManager.kt and managers/TechManager.kt (`scienceFromResearchA... | `mod.rs` |
| `MEDIAN_TECH_PERCENT` | `50` | Unciv: DiplomacyTurnManager.kt and TechManager.kt, equal. | Unciv: DiplomacyTurnManager.kt and TechManager.kt, equal | `mod.rs` |

### Espionage

21 constants (Unciv 17, own design 2, own value (source not recorded) 2). Files: `classic/crates/classic-rules/src/systems/espionage.rs`, `classic/crates/classic-rules/src/world/espionage.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `TRAVEL_TURNS` | `1` | Rounds a spy travels to a new city. | Unciv: models/Spy.kt (`moveTo`), equal (1) | `espionage.rs` |
| `INTRODUCTION_TURNS` | `4` | Rounds a diplomat spends making introductions in a foreign capital before it works. | own design | `espionage.rs` |
| `REVIVE_TURNS` | `5` | Unciv: models/Spy.kt (`killSpy`, 5 turns), equal. | Unciv: models/Spy.kt (`killSpy`, 5 turns), equal | `espionage.rs` |
| `SURVEILLANCE_TURNS` | `3` | Turns a spy needs to set up its network in a foreign city. | Unciv: models/Spy.kt (EstablishNetwork, 3 turns); equal | `espionage.rs` |
| `INTEL_COST_PERCENT` | `125` | Percent of the dearest stealable technology that gathering intel must reach, and the percent a spy's rate grows per rank. | Unciv: models/ModConstants.kt (`spyTechStealCostModifier` 1.25, `spyRankStealPercentBonus` 25 with rank... | `espionage.rs` |
| `RANK_RATE_PERCENT` | `25` | Unciv: models/ModConstants.kt (`spyRankStealPercentBonus` 25), equal. | Unciv: models/ModConstants.kt (`spyRankStealPercentBonus` 25), equal | `espionage.rs` |
| `ELECTION_ROUNDS` | `15` | Rounds between city-state elections at Standard speed. | Unciv: models/ModConstants.kt (`cityStateElectionTurns` 15); equal | `espionage.rs` |
| `ELECTION_GAIN` | `20` | Influence gained by the winner of a rigged election and lost by every other major that knows the city-state (by every rigging major when the neutral party wins); the votes of the neutral party. | Unciv: logic/nation/diplomacy/CityStateFunctions.kt (`holdElections`); equal | `espionage.rs` |
| `ELECTION_LOSS` | `5` | Unciv: CityStateFunctions.kt (`holdElections`), equal. | Unciv: CityStateFunctions.kt (`holdElections`), equal | `espionage.rs` |
| `ELECTION_NEUTRAL_VOTES` | `20` | Unciv: CityStateFunctions.kt (`holdElections`), equal. | Unciv: CityStateFunctions.kt (`holdElections`), equal | `espionage.rs` |
| `COUP_BASE_CHANCE` | `50` | Coup: base chance (percent), minus half the influence gap to the ally, plus half the skill gap (skill `RANK_SKILL_PERCENT` × (rank + 1) of the spy against the ally's spy in the city), capped at 85; on success the spy's owner takes the ally's influence, the ally loses 20 and every other major 10; on failure the spy dies and its owner loses 20. | Unciv: models/Spy.kt (`getCoupChanceOfSuccess`, `initiateCoup`); equal | `espionage.rs` |
| `COUP_MAX_CHANCE` | `85` | Unciv: models/Spy.kt (coup: cap 85, ally -20, others -10, failure -20), equal. | Unciv: models/Spy.kt (coup: cap 85, ally -20, others -10, failure -20), equal | `espionage.rs` |
| `COUP_ALLY_DROP` | `20` | Unciv: models/Spy.kt (coup: cap 85, ally -20, others -10, failure -20), equal. | Unciv: models/Spy.kt (coup: cap 85, ally -20, others -10, failure -20), equal | `espionage.rs` |
| `COUP_OTHERS_DROP` | `10` | Unciv: models/Spy.kt (coup: cap 85, ally -20, others -10, failure -20), equal. | Unciv: models/Spy.kt (coup: cap 85, ally -20, others -10, failure -20), equal | `espionage.rs` |
| `COUP_FAIL_DROP` | `20` | Unciv: models/Spy.kt (coup: cap 85, ally -20, others -10, failure -20), equal. | Unciv: models/Spy.kt (coup: cap 85, ally -20, others -10, failure -20), equal | `espionage.rs` |
| `RANK_SKILL_PERCENT` | `30` | Skill percent per spy rank (rank 0 here is Unciv's rank 1). | Unciv: models/ModConstants.kt (`spyRankSkillPercentBonus` 30); equal | `espionage.rs` |
| `RENAISSANCE` | `3` | Era index of the Renaissance: every nation gets a spy once a nation enters it. | Unciv: Eras.json (Renaissance era, "Every major Nation gains a spy once a nation enters this era"), equal | `espionage.rs` |
| `INDUSTRIAL` | `4` | Era index of the Industrial era: from it on a nation gets one more spy as it enters each era. | own design | `espionage.rs` |
| `ALL` | `[ SpyBlock::Dead, SpyBlock::NoSuchCity, SpyBlock::NotMet, SpyBlock::Occupied, SpyBlock:...` |  | own value (source not recorded) | `espionage.rs` |
| `ALL` | `[ SpyState::Unassigned, SpyState::Travelling, SpyState::Surveillance, SpyState::Gatheri...` |  | own value (source not recorded) | `espionage.rs` |
| `MAX_RANK` | `2` | Highest spy rank (0 = recruit .. | Unciv: models/ModConstants.kt (`maxSpyRank` 3: three ranks), equal | `espionage.rs` |

### Trade routes

34 constants (own design 32, own value (source not recorded) 2). Files: `classic/crates/classic-rules/src/derived/trade.rs`, `classic/crates/classic-rules/src/world/trade.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `LAND_RANGE` | `12` | Range of a land route in tiles before modifiers. | own design | `trade.rs` |
| `SEA_RANGE` | `18` | Range of a sea route in tiles before modifiers. | own design | `trade.rs` |
| `ERAS_PER_RANGE_TILE` | `2` | Eras per extra tile of range: traders reach further as roads and charts improve (+1 every 2 eras). | own design | `trade.rs` |
| `CITIES_PER_ROUTE_SLOT` | `4` | Cities per extra route slot on top of the technology slots: a larger empire fields more traders. | own design | `trade.rs` |
| `LAND_SPEED` | `3` | Tiles a caravan moves along its path in a turn. | own design | `trade.rs` |
| `SEA_SPEED` | `5` | Tiles a cargo ship moves along its path in a turn. | own design | `trade.rs` |
| `TARGET_TURNS` | `24` | Target length of a route in turns; the circuits are the whole number that comes closest. | own design | `trade.rs` |
| `MIN_CIRCUITS` | `2` | Fewest circuits of one route (a very short route would turn the unit over every few turns, a very long one would tie a slot up for good, hence also a most). | own design | `trade.rs` |
| `MAX_CIRCUITS` | `8` | Most circuits of one route. | own design | `trade.rs` |
| `STEP_COST` | `100` | Path cost of an ordinary land step (also the scale of the range budget). | own design | `trade.rs` |
| `ROAD_STEP_COST` | `60` | Path cost of a step between two tiles with a road or railroad. | own design | `trade.rs` |
| `ROUGH_STEP_COST` | `115` | Path cost of a step into rough ground (hills, forest, jungle, ice). | own design | `trade.rs` |
| `OPEN_SEA_STEP_COST` | `120` | Path cost of a sea step away from the coast. | own design | `trade.rs` |
| `ENEMY_WATER_COST` | `500` | Extra path cost of water owned by an enemy (the path stays valid but long). | own design | `trade.rs` |
| `INTERNATIONAL_BASE` | `80` | Base gold of an international route in the first era, hundredths. | own design | `trade.rs` |
| `INTERNATIONAL_PER_ERA` | `25` | Gold added to the base of an international route per era of its sender, hundredths. | own design | `trade.rs` |
| `RECIPIENT_BASE` | `25` | Base gold of the recipient end in the first era, hundredths. | own design | `trade.rs` |
| `RECIPIENT_PER_ERA` | `10` | Gold added to the recipient base per era of the recipient, hundredths. | own design | `trade.rs` |
| `CITY_GOLD_PERCENT` | `18` | Percent of the geometric mean of the two cities' gold income (hundredths of gold per turn) a route adds. | own design | `trade.rs` |
| `RESOURCE_GOLD` | `25` | Gold per luxury or strategic resource only one of the cities has, hundredths. | own design | `trade.rs` |
| `RESOURCE_GOLD_MAX` | `5` | Most differing resources that pay. | own design | `trade.rs` |
| `DISTANCE_PERCENT_PER_TILE` | `3` | Percent added to the gold of a route per tile of its path: a far partner pays more, up to +45 %. | own design | `trade.rs` |
| `DISTANCE_PERCENT_MAX_TILES` | `15` | Path length after which the distance bonus stops growing. | own design | `trade.rs` |
| `SEA_GOLD_PERCENT` | `150` | Percent of the gold a sea route yields (cargo ships cost more and need two coasts). | own design | `trade.rs` |
| `SCIENCE_PER_TECH` | `30` | Science per technology the partner knows and the receiver lacks, hundredths. | own design | `trade.rs` |
| `SCIENCE_MAX` | `450` | Most science one end of a route gets, hundredths. | own design | `trade.rs` |
| `INTERNAL_BASE` | `200` | Food or production of an internal route at the start, hundredths. | own design | `trade.rs` |
| `INTERNAL_PER_ERA` | `45` | Food or production added to an internal route per era of the destination's owner, hundredths. | own design | `trade.rs` |
| `INTERNAL_PER_CITIZEN` | `15` | Food or production added to an internal route per citizen of the destination, hundredths. | own design | `trade.rs` |
| `INTERNAL_SEA_PERCENT` | `60` | Percent a sea route adds to the food or production of an internal route. | own design | `trade.rs` |
| `PLUNDER_BASE` | `60` | Whole gold a plunderer takes from a route in the first era (×150 % at sea). | own design | `trade.rs` |
| `PLUNDER_PER_ERA` | `10` | Whole gold added to the plunder per era of the route's owner. | own design | `trade.rs` |
| `ALL` | `[ TradeBlock::NotATradeUnit, TradeBlock::NotInOwnCity, TradeBlock::UnknownCity, TradeBl...` |  | own value (source not recorded) | `trade.rs` |
| `ALL` | `[RouteKind::International, RouteKind::Food, RouteKind::Production]` |  | own value (source not recorded) | `trade.rs` |

### Tourism and archaeology

33 constants (own design 25, own value (source not recorded) 8). Files: `classic/crates/classic-rules/src/derived/archaeology.rs`, `classic/crates/classic-rules/src/derived/tourism.rs`, `classic/crates/classic-rules/src/world/tourism.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `SITES_PER_NATION` | `(4, 6)` | Dig sites per major nation: (fewest, most). | own design | `archaeology.rs` |
| `PERCENT_HIDDEN` | `25` | Percent of the sites that start hidden until a policy reveals them. | own design | `archaeology.rs` |
| `PERCENT_HIDDEN_WRITING` | `40` | Percent of the hidden sites that hold a writing. | own design | `archaeology.rs` |
| `ARCHAEOLOGY_WRITINGS` | `10` | Most sites of the game that hold a writing, whatever the number of nations. | own design | `archaeology.rs` |
| `BOOST_TURNS` | `crate::world::culture::RECENT_TURNS` | Turns of culture a writing site's culture boost gives: the whole recent-culture window ([`crate::world::culture::RECENT_TURNS`], Unciv's 8 turns). | own value (source not recorded) | `archaeology.rs` |
| `SITE_BASE_WEIGHT` | `6` | Weight of a tile without any history in the choice of the sites. | own design | `archaeology.rs` |
| `SITE_WEIGHT_PER_RANK` | `5` | Weight a history record adds per rank of its kind (battle < camp < ruin < razed city < writing). | own design | `archaeology.rs` |
| `SITE_WEIGHT_PER_OLD_ERA` | `2` | Weight a history record adds per era it lies back from the seventh: old history draws sites. | own design | `archaeology.rs` |
| `SITE_TERRAIN_PERCENT` | `150` | Percent of the weight of the terrains where ruins survive best (desert and jungle), 150 %. | own design | `archaeology.rs` |
| `SITE_OWNED_PERCENT` | `85` | Percent of the weight of land inside someone's borders: sites lie a little more often in the wild. | own design | `archaeology.rs` |
| `SITE_IMPROVED_PERCENT` | `60` | Percent of the weight of an improved or road tile (building would have found the ruin). | own design | `archaeology.rs` |
| `SITE_CITY_PERCENT` | `10` | Percent of the weight of a city tile. | own design | `archaeology.rs` |
| `SITE_CROWD_RADIUS` | `4` | Radius around a chosen site where other sites are made less likely; the penalty is `radius + 1 - distance` (4 next to the site, 1 at the edge). | own design | `archaeology.rs` |
| `ALL` | `[ ArchaeologyChoice::Landmark, ArchaeologyChoice::ArtifactFirst, ArchaeologyChoice::Art...` |  | own value (source not recorded) | `archaeology.rs` |
| `ALL` | `[ DigBlock::UnknownUnit, DigBlock::NotAnArchaeologist, DigBlock::NoSite, DigBlock::Hidd...` |  | own value (source not recorded) | `archaeology.rs` |
| `INVENTED_KINDS` | `[ ArtifactKind::AncientRuin, ArtifactKind::AncientRuin, ArtifactKind::RazedCity, Artifa...` | A history record invented for a site without one: era by weight (`highest - i`, the oldest eras most likely), kind drawn from this table (ruins and barbarian camps twice in seven, the others once). | own design | `archaeology.rs` |
| `WORK_CULTURE_BASE` | `1` | Culture of a great work per turn in its first era. | own design | `tourism.rs` |
| `WORK_CULTURE_ERAS_PER_STEP` | `3` | Eras of the work per extra culture a turn: a later work is worth a little more. | own design | `tourism.rs` |
| `WORK_TOURISM_BASE` | `2` | Tourism of a great work per turn in its first era. | own design | `tourism.rs` |
| `WORK_TOURISM_ERAS_PER_STEP` | `2` | Eras of the work per extra tourism a turn. | own design | `tourism.rs` |
| `TIE_TOURISM_PERCENT` | `[0, 12, 21, 27]` | Tourism percent when 0, 1, 2 or 3 of the ties (shared religion, open borders, a trade route) join the two nations. | own design | `tourism.rs` |
| `DIFFERENT_IDEOLOGY_TOURISM_PERCENT` | `-25` | Tourism percent for different ideologies: a quarter lost, so rivals of another creed are the hard targets. | own design | `tourism.rs` |
| `DIPLOMAT_TOURISM_PERCENT` | `15` | Tourism percent bonus for a diplomat in the target's capital, only when the ideologies differ: it undoes more than half of the ideology penalty. | own design | `tourism.rs` |
| `INFLUENCE_DECAY_PERMILLE` | `20` | Per mille of the influence stock that fades each turn: what a nation stops spreading is forgotten with a half-life of about 34 turns. | own design | `tourism.rs` |
| `CULTURE_WINDOW_FLOOR` | `40` | Floor of the target's recent culture (the last 8 turns) the influence is measured against, so a nation that makes almost no culture is not "dominated" by a trickle of tourism. | own design | `tourism.rs` |
| `LEVEL_THRESHOLDS` | `[ level_threshold(1), level_threshold(2), level_threshold(3), level_threshold(4), level...` | Thresholds of the five levels, per mille of the target's recent culture ([`level_threshold`]). | own value (source not recorded) | `tourism.rs` |
| `OPINION_STEPS` | `[1, 3, 6]` | Dissatisfaction (points) at which the people start to murmur, then unrest, then upheaval. | own design | `tourism.rs` |
| `OPINION_UNHAPPY_BASE_PERCENT` | `3` | Unhappiness of the people with their ideology: percent of the population at one point of dissatisfaction (`3 + 3 d`), and the most. | own design | `tourism.rs` |
| `OPINION_UNHAPPY_PER_POINT_PERCENT` | `3` | See [`OPINION_UNHAPPY_BASE_PERCENT`]. | own value (source not recorded) | `tourism.rs` |
| `OPINION_UNHAPPY_MAX_PERCENT` | `30` | See [`OPINION_UNHAPPY_BASE_PERCENT`]. | own value (source not recorded) | `tourism.rs` |
| `OPINION_PRESSURE_CAP` | `4` | Cap of the pressure one nation puts on another's opinion, in points (a nation cannot be talked out of its creed by one rival alone). | own design | `tourism.rs` |
| `ALL` | `[ TourismBlock::UnknownUnit, TourismBlock::WrongUnit, TourismBlock::NotNearOwnCity, Tou...` |  | own value (source not recorded) | `tourism.rs` |
| `ALL` | `[ ArtifactKind::BattleRanged, ArtifactKind::BattleMelee, ArtifactKind::BarbarianCamp, A...` |  | own value (source not recorded) | `tourism.rs` |

### World Congress

16 constants (Unciv 4, own design 9, own value (source not recorded) 3). Files: `classic/crates/classic-rules/src/systems/congress.rs`, `classic/crates/classic-rules/src/systems/league_projects.rs`, `classic/crates/classic-rules/src/world/congress.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `SESSIONS` | `[ SpecialSession { era: 3, turns_between: 28, nation_delegates: 2, host_delegates: 1, c...` | The three special sessions: the Assembly (Renaissance), the Council (Industrial era, the city-states get a seat) and the United Nations (Atomic era, the World Leader vote). | own design | `congress.rs` |
| `LEADERS_BONUS_ON_FAIL` | `1` | Extra delegates each of the two leading candidates keeps for good when the World Leader vote fails. | own design | `congress.rs` |
| `FOUNDING_MET_PERCENT` | `50` | Percent a nation must have met of the other nations (rounded up, at least one) to found the World Congress with the technology that allows it. | own design | `congress.rs` |
| `ARMY_TAX_PERCENT` | `20` | Percent added to unit maintenance while the Standing Army Tax is in force. | own design | `congress.rs` |
| `SCHOLARS_PERCENT` | `12` | Percent added to the science of a nation researching a technology another nation knows while Scholars in Residence is in force. | own design | `congress.rs` |
| `HERITAGE_CULTURE_PER_WONDER` | `2` | Culture per turn each world wonder gives while the Cultural Heritage Sites resolution is in force. | own design | `congress.rs` |
| `FUNDING_PERCENT` | `25` | Percent the arts (writers, artists, musicians) gain and the industries (scientists, engineers, merchants) lose in great person points under Arts Funding, and the reverse under Sciences Funding. | own design | `congress.rs` |
| `VICTORY_SHARE_PERCENT` | `67` | Share of the available votes the World Leader needs: with `n` votes, `n × (67 - floor(1.1 n)) / 100 + 1` up to 28 votes, `n × 35 / 100` above. | Unciv: logic/nation/managers/VictoryManager.kt (`votesNeededForDiplomaticVictory`, applied, as there, to... | `congress.rs` |
| `VICTORY_SHARE_DROP_TIMES10` | `11` | Unciv: VictoryManager.kt (`votesNeededForDiplomaticVictory`), equal. | Unciv: VictoryManager.kt (`votesNeededForDiplomaticVictory`), equal | `congress.rs` |
| `VICTORY_LARGE_VOTES` | `28` | Unciv: VictoryManager.kt (`votesNeededForDiplomaticVictory`), equal. | Unciv: VictoryManager.kt (`votesNeededForDiplomaticVictory`), equal | `congress.rs` |
| `VICTORY_LARGE_PERCENT` | `35` | Unciv: VictoryManager.kt (`votesNeededForDiplomaticVictory`), equal. | Unciv: VictoryManager.kt (`votesNeededForDiplomaticVictory`), equal | `congress.rs` |
| `PROPOSERS` | `3` | Proposals per session: the host and the two other members with the most delegates. | own design | `congress.rs` |
| `YES` | `1` | Yes and no of a yes/no proposal. | own value (source not recorded) | `congress.rs` |
| `NO` | `0` |  | own value (source not recorded) | `congress.rs` |
| `BOOST_PERCENT` | `75` | Percent boost to culture and to tourism while a league project reward runs. | own design | `league_projects.rs` |
| `ALL` | `[ ResolutionKind::DiplomaticVictory, ResolutionKind::ChangeHost, ResolutionKind::Standi...` |  | own value (source not recorded) | `congress.rs` |

### Victory and score

9 constants (Unciv 7, own value 1, own value (source not recorded) 1). Files: `classic/crates/classic-rules/src/systems/victory.rs`, `classic/crates/classic-rules/src/world/victory.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `MIN_ROUNDS_TO_TEST` | `10` | Rounds that must pass before victory conditions are tested. | own value | `victory.rs` |
| `SCORE_CITY` | `10` | Score points per city, citizen, land tile, wonder and technology. | Unciv: logic/nation/Nation.kt (`calculateScoreBreakdown`: city 10, land tile 1, technology 4) and models... | `victory.rs` |
| `SCORE_POPULATION` | `3` | Unciv: Nation.kt and ModConstants.kt (score), equal. | Unciv: Nation.kt and ModConstants.kt (score), equal | `victory.rs` |
| `SCORE_LAND` | `1` | Unciv: Nation.kt and ModConstants.kt (score), equal. | Unciv: Nation.kt and ModConstants.kt (score), equal | `victory.rs` |
| `SCORE_WONDER` | `40` | Unciv: Nation.kt and ModConstants.kt (score), equal. | Unciv: Nation.kt and ModConstants.kt (score), equal | `victory.rs` |
| `SCORE_TECH` | `4` | Unciv: Nation.kt and ModConstants.kt (score), equal. | Unciv: Nation.kt and ModConstants.kt (score), equal | `victory.rs` |
| `STANDARD_MAP_PLOTS` | `4160` | Plots of a Standard map (the reference size of the map score modifier: 80 × 52). | Unciv: logic/nation/Nation.kt (reference / tiles, above 1 damped to (m - 1) / 3 + 1), whose reference 12... | `victory.rs` |
| `ALL` | `[ VictoryKind::Time, VictoryKind::Science, VictoryKind::Domination, VictoryKind::Cultur...` |  | own value (source not recorded) | `victory.rs` |
| `STANDARD_MAX_ROUNDS` | `500` | The last round of a game at Standard speed. | Unciv: Speeds.json (Standard, 500 turns), equal | `victory.rs` |

### Map, terrain and tile yields

31 constants (Unciv 5, own value (source not recorded) 26). Files: `classic/crates/classic-rules/src/classic/yields.rs`, `platform/crates/kernel/src/derived/tile_yields.rs`, `platform/crates/kernel/src/derived/yields.rs`, `platform/crates/kernel/src/rules/natural_wonders.rs`, `platform/crates/kernel/src/scenario/generator.rs`, `platform/crates/kernel/src/scenario/landscape.rs`, `platform/crates/kernel/src/scenario/map_script.rs`, `platform/crates/kernel/src/scenario/sea_resources.rs`, `platform/crates/kernel/src/scenario/start_balance.rs`, `platform/crates/kernel/src/world/map.rs`, `platform/crates/kernel/src/world/size.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `SCIENCE_PER_CITIZEN` | `1` | Science of every citizen. | Unciv: CityStats.kt (population stats), equal | `yields.rs` |
| `GOLDEN_AGE_PRODUCTION_PERCENT` | `20` | Percent production bonus of a city during a golden age (classic/docs/classic/terrain.json `golden_age_city_pct`). | Unciv: CityStats.kt (golden age percent bonuses), equal | `yields.rs` |
| `GOLDEN_AGE_TILE_GOLD_THRESHOLD` | `1` | Golden age: a tile with at least this much gold gets `GOLDEN_AGE_TILE_GOLD` more. | Unciv: TileStatFunctions.kt (a tile with gold gets +1), equal | `yields.rs` |
| `GOLDEN_AGE_TILE_GOLD` | `1` | Unciv: TileStatFunctions.kt, equal. | Unciv: TileStatFunctions.kt, equal | `yields.rs` |
| `LAKE_FOOD` | `1` | Extra food of a lake tile (classic/docs/classic/terrain.json). | own value (source not recorded) | `tile_yields.rs` |
| `CITY_MIN_FOOD` | `2` | the city centre yields at least this much food and production (classic/docs/classic/terrain.json `city_tile_minimum`), the default of `YieldTerms::CITY_MINIMUM`. | own value (source not recorded) | `tile_yields.rs` |
| `CITY_MIN_PRODUCTION` | `1` |  | own value (source not recorded) | `tile_yields.rs` |
| `CITY_TERRITORY_RADIUS` | `1` | Territory radius at founding: ring 1 (Classic). | own value (source not recorded) | `yields.rs` |
| `NEIGHBOURS` | `6` | Most neighbours a hex tile has. | own value (source not recorded) | `natural_wonders.rs` |
| `MIN_START_DIST` | `12` | Minimum spread of starts (hexes). | own value (source not recorded) | `generator.rs` |
| `START_MARGIN` | `3` | Margin of starts from the map edge (rows/columns). | own value (source not recorded) | `generator.rs` |
| `WATER` | `Ground::Water` |  | own value (source not recorded) | `landscape.rs` |
| `HILLS` | `Ground::Hills` |  | own value (source not recorded) | `landscape.rs` |
| `MOUNTAIN` | `Ground::Mountains` |  | own value (source not recorded) | `landscape.rs` |
| `PLAINS` | `Ground::Flat(Climate::Temperate)` |  | own value (source not recorded) | `landscape.rs` |
| `GRASS` | `Ground::Flat(Climate::Humid)` |  | own value (source not recorded) | `landscape.rs` |
| `DESERT` | `Ground::Flat(Climate::Arid)` |  | own value (source not recorded) | `landscape.rs` |
| `TUNDRA` | `Ground::Flat(Climate::Cold)` |  | own value (source not recorded) | `landscape.rs` |
| `SNOW` | `Ground::Flat(Climate::Frozen)` |  | own value (source not recorded) | `landscape.rs` |
| `CLASSIC` | `LandOptions { mountain_percent: MOUNTAIN_SHARE as u8, hill_percent: HILL_SHARE as u8, f...` | The classic map: every share as it always was. | own value (source not recorded) | `landscape.rs` |
| `RICH` | `LandOptions { mountain_percent: 12, hill_percent: 22, forest_percent: 150, river_percen...` | A land made for a tall science empire (the science audit's ideal case, classic/docs/ai-science-audit.md): twice the mountains, more hills, half again the forest, two and a half times the rivers, twice the land resources. | own value (source not recorded) | `landscape.rs` |
| `ALL` | `[ MapScript::Continents, MapScript::Pangaea, MapScript::Archipelago, MapScript::Fractal...` |  | own value (source not recorded) | `map_script.rs` |
| `SHALLOW_REACH` | `2` | Hex distance from land within which water is shallow: the coast and the ring behind it. | own value (source not recorded) | `sea_resources.rs` |
| `START_REACH` | `3` | Tiles around a start that its first city works: the sea within this reach is the start's own. | own value (source not recorded) | `sea_resources.rs` |
| `CLASSIC` | `StartPolicy { weights: YieldWeights { food: 2, production: 3, gold: 1, science: 1, cult...` |  | own value (source not recorded) | `start_balance.rs` |
| `OFF` | `StartBalance { fair: false, strategic: false, policy: StartPolicy::CLASSIC }` | The starts as they were before this module (tests that pin an old map). | own value (source not recorded) | `start_balance.rs` |
| `ON` | `StartBalance { fair: true, strategic: true, policy: StartPolicy::CLASSIC }` |  | own value (source not recorded) | `start_balance.rs` |
| `LAKE_MAX_AREA_SIZE` | `10` | A water body of at most this many tiles is a lake (fresh water, +1 food). | Unciv: ModConstants.kt (`maxLakeSize` 10, `MapGenerator.kt` compares with `<=`), equal | `map.rs` |
| `MIN_WATER_SIZE_FOR_OCEAN` | `10` | A coastal city touches a water body of at least this many tiles (harbors, coastal buildings, sea trade). | own value (source not recorded) | `map.rs` |
| `NO_AREA` | `u32::MAX` | Water area of a land tile. | own value (source not recorded) | `map.rs` |
| `WORLD_SIZES` | `[ WorldSize::Duel, WorldSize::Tiny, WorldSize::Small, WorldSize::Standard, WorldSize::L...` |  | own value (source not recorded) | `size.rs` |

### Players

3 constants (own value (unverified) 1, own value (source not recorded) 2). Files: `classic/crates/classic-rules/src/classic/player.rs`, `platform/crates/kernel/src/world/player.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `SCIENCE_HISTORY_TURNS` | `8` | Turns of science a player remembers; a great scientist grants the sum (own value, unverified). | own value (unverified) | `player.rs` |
| `FREE_TECH_ANY` | `u16::MAX` | `SavedPlayerEconomy::free_technologies` code of `FreeTechnology::Any`. | own value (source not recorded) | `player.rs` |
| `RESEARCH_QUEUE_MAX` | `u16::MAX as usize + 1` | Technologies a research queue holds at most: a full dependency plan may contain every representable technology once. | own value (source not recorded) | `player.rs` |

### Content and effect model

36 constants (Unciv 3, own design 2, own value (source not recorded) 31). Files: `classic/crates/classic-rules/src/classic/content.rs`, `classic/crates/classic-rules/src/classic/effects.rs`, `classic/crates/classic-rules/src/classic/tables.rs`, `platform/crates/kernel/src/rules/compiled.rs`, `platform/crates/kernel/src/rules/content.rs`, `platform/crates/kernel/src/rules/effects.rs`, `platform/crates/kernel/src/rules/modifiers.rs`, `platform/crates/kernel/src/rules/roles.rs`, `platform/crates/kernel/src/rules/speeds.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `MIDDLE` | `5` | The middle of the scale: no leaning. | Unciv: models/ruleset/nation/Personality.kt (scale 0 to 10, default 5), equal | `content.rs` |
| `MAX` | `10` | Unciv: models/ruleset/nation/Personality.kt (scale 0 to 10, default 5), equal. | Unciv: models/ruleset/nation/Personality.kt (scale 0 to 10, default 5), equal | `content.rs` |
| `NEUTRAL` | `Personality { expansion: Self::MIDDLE, wonders: Self::MIDDLE, science: Self::MIDDLE, cu...` |  | own value (source not recorded) | `content.rs` |
| `GREAT_PERSON_KINDS` | `9` |  | own value (source not recorded) | `content.rs` |
| `GREAT_PERSON_ROLE` | `UnitRole::Module(RoleTag::new(1))` | The classic words in the kernel's content enums, by the tags they had there before the split (`kernel::rules::content::module_tag!`). | own value (source not recorded) | `content.rs` |
| `RELIGIOUS_ROLE` | `UnitRole::Module(RoleTag::new(2))` |  | own value (source not recorded) | `content.rs` |
| `RELIGIOUS_BUILDING` | `BuildingKind::Module(BuildingKindTag::new(1))` | Bought with faith only (Monastery, Cathedral, Mosque, Pagoda). | own value (source not recorded) | `content.rs` |
| `GUILD` | `BuildingKind::Module(BuildingKindTag::new(2))` |  | own value (source not recorded) | `content.rs` |
| `GREAT_PERSON_IMPROVEMENT` | `ImprovementKind::Module(ImprovementKindTag::new(1))` | Created by a great person (academy, citadel, ...). | own value (source not recorded) | `content.rs` |
| `BARBARIAN_CAMP` | `ImprovementKind::Module(ImprovementKindTag::new(3))` |  | own value (source not recorded) | `content.rs` |
| `CITY_STATE_STATS` | `17` |  | own value (source not recorded) | `effects.rs` |
| `RELIGION_STATS` | `9` |  | own value (source not recorded) | `effects.rs` |
| `RELIGION_ABILITIES` | `2` |  | own value (source not recorded) | `effects.rs` |
| `TOURISM_CONDITIONS` | `6` |  | own value (source not recorded) | `effects.rs` |
| `LEAGUE_TIER_PERCENTS` | `[40, 90]` | Share of the per-player cost a contribution needs for tiers 1 and 2: 40 % and 90 %. | own design | `tables.rs` |
| `POLICIES` | `ContentTable::Module(TableTag::new(8))` | The classic tables a reference may point into, by the tags they had among the kernel's tables before the split (`ContentTable::tag`). | own value (source not recorded) | `tables.rs` |
| `BELIEFS` | `ContentTable::Module(TableTag::new(9))` |  | own value (source not recorded) | `tables.rs` |
| `POLICY_BRANCHES` | `ContentTable::Module(TableTag::new(14))` |  | own value (source not recorded) | `tables.rs` |
| `RELIGIONS` | `ContentTable::Module(TableTag::new(15))` |  | own value (source not recorded) | `tables.rs` |
| `CITY_STATE_TYPES` | `ContentTable::Module(TableTag::new(16))` |  | own value (source not recorded) | `tables.rs` |
| `CITY_STATES` | `ContentTable::Module(TableTag::new(17))` |  | own value (source not recorded) | `tables.rs` |
| `GREAT_PEOPLE` | `ContentTable::Module(TableTag::new(19))` |  | own value (source not recorded) | `tables.rs` |
| `DEFAULT_STRENGTH` | `10` | Strength of debug/default units not described by content (UnitKindId(0) with an empty SourceRules). | own value (source not recorded) | `compiled.rs` |
| `MOVEMENT_DEFAULT` | `2` | Movement points of default units (content sets them via movement). | own value (source not recorded) | `compiled.rs` |
| `MAX_PROMOTIONS` | `256` | Upper bound of the promotion table (units keep promotions in a bitset). | own value (source not recorded) | `compiled.rs` |
| `ZERO` | `Yields { food: 0, production: 0, gold: 0, science: 0, culture: 0, module: 0 }` | Nothing in every channel. | own value (source not recorded) | `content.rs` |
| `MAX_YIELD_KINDS` | `8` | Capacity of per-yield arrays: the kernel's 6 channels and room for the module's (`Vocabulary::YIELDS`, at most one). | own value (source not recorded) | `effects.rs` |
| `MAX_COST_TARGETS` | `16` | Capacity of per-cost arrays: the kernel's 10 costs and room for the module's (`Vocabulary::COSTS`). | own value (source not recorded) | `effects.rs` |
| `MAX_MULTIPLIERS` | `8` | Distinct multiplicative modifiers on one value. | own value (source not recorded) | `modifiers.rs` |
| `NONE` | `Caps { min: None, max: None, }` |  | own value (source not recorded) | `modifiers.rs` |
| `ALL` | `[ Climate::Humid, Climate::Temperate, Climate::Arid, Climate::Cold, Climate::Frozen]` |  | own value (source not recorded) | `roles.rs` |
| `ALL` | `[ Ground::Water, Ground::Mountains, Ground::Hills, Ground::Flat(Climate::Humid), Ground...` |  | own value (source not recorded) | `roles.rs` |
| `ALL` | `[ FeatureRole::Forest, FeatureRole::Jungle, FeatureRole::Marsh, FeatureRole::Oasis, Fea...` |  | own value (source not recorded) | `roles.rs` |
| `LEGACY` | `TerrainPalette { codes: [4, 6, 2, 1, 0, 3, 7, 5] }` | The codes of rules without a terrain table (test fixtures paint them): plains 0, grassland 1, hills 2, desert 3, water 4, snow 5, mountains 6, tundra 7. | own value (source not recorded) | `roles.rs` |
| `SPEED_STRETCHES` | `8` | The most stretches a speed's calendar has. | own design | `speeds.rs` |
| `STANDARD` | `SpeedValues { pace_percent: 100, production_percent: 100, gold_percent: 100, science_pe...` | Standard speed: every percent 100, the numbers every rule constant is written at. | Unciv: Speeds.json (Standard), equal; the classic content's `core:speed/standard` row holds the same num... | `speeds.rs` |

### Other rule constants

17 constants (Unciv 11, own value (source not recorded) 6). Files: `classic/crates/classic-rules/src/derived/building_sale.rs`, `classic/crates/classic-rules/src/derived/disband.rs`, `classic/crates/classic-rules/src/derived/standing.rs`, `classic/crates/classic-rules/src/knowledge/actions.rs`, `classic/crates/classic-rules/src/systems/revolts.rs`, `classic/crates/classic-rules/src/world/mod.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `BUILDING_SALE_DIVISOR` | `10` | Unciv: City.getGoldForSellingBuilding, `cost / 10` (Civ V: BUILDING_SALE_DIVISOR). | Unciv: City.getGoldForSellingBuilding, `cost / 10` (Civ V: BUILDING_SALE_DIVISOR) | `building_sale.rs` |
| `ALL` | `[ SaleBlock::UnknownCity, SaleBlock::NotYourCity, SaleBlock::CityTakesNoOrders, SaleBlo...` |  | own value (source not recorded) | `building_sale.rs` |
| `DISBAND_GOLD_DIVISOR` | `20` | Unciv: BaseUnit.getDisbandGold divides the base gold cost by 20. | Unciv: BaseUnit.getDisbandGold divides the base gold cost by 20 | `disband.rs` |
| `ALL` | `[DisbandBlock::UnknownUnit, DisbandBlock::NoMovesLeft]` |  | own value (source not recorded) | `disband.rs` |
| `ALL` | `[Measure::Population, Measure::Crop, Measure::Goods, Measure::Gnp, Measure::Literacy, M...` |  | own value (source not recorded) | `standing.rs` |
| `LEADER_VOTE_WARNING_TURNS` | `3` | Turns before the Congress session that elects the World Leader from which the player is told that the vote is near. | own value (source not recorded) | `standing.rs` |
| `MARCH_TAG` | `10` | The codec tag of `MarchTo` (`command/codec.rs`), not enumerated. | own value (source not recorded) | `actions.rs` |
| `REVOLT_DELAY_BASE` | `4` | Turns from the first turn below the threshold to the rising, before the random part and the game speed. | Unciv: `ModConstants.baseTurnsUntilRevolt` (4); equal | `revolts.rs` |
| `REVOLT_DELAY_RANDOM` | `3` | The random part of the delay: `rand(3)` more turns. | Unciv: TurnManager.getTurnsBeforeRevolt (`nextInt(3)`); equal | `revolts.rs` |
| `REBEL_PERCENT_PER_CITY` | `20` | Percent of the spawn roll per city beyond the first: one rebel, and one more with the chance `20 × (cities - 1)` percent (a third from 11 cities). | Unciv: TurnManager.doRevoltSpawn (`1 + nextInt(100 + 20 * (cities - 1)) / 100`); equal | `revolts.rs` |
| `REBEL_CITY_WEIGHT` | `10` | Weight of a city in the draw of the rising city on top of its population. | Unciv: TurnManager.doRevoltSpawn (`nextInt(population + 10)`); equal | `revolts.rs` |
| `REBEL_TILE_BASE` | `10` | Rate of a free land tile for the rising, and its additions: no improvement, a resource on an unimproved tile, a defence bonus. | Unciv: TurnManager.rateTileForRevoltSpawn (10, +4, +3, +4); equal | `revolts.rs` |
| `REBEL_TILE_UNIMPROVED` | `4` | Unciv: TurnManager.rateTileForRevoltSpawn (+4 unimproved); equal. | Unciv: TurnManager.rateTileForRevoltSpawn (+4 unimproved); equal | `revolts.rs` |
| `REBEL_TILE_RESOURCE` | `3` | Unciv: TurnManager.rateTileForRevoltSpawn (+3 resource on an unimproved tile); equal. | Unciv: TurnManager.rateTileForRevoltSpawn (+3 resource on an unimproved tile); equal | `revolts.rs` |
| `REBEL_TILE_DEFENCE` | `4` | Unciv: TurnManager.rateTileForRevoltSpawn (+4 defence bonus); equal. | Unciv: TurnManager.rateTileForRevoltSpawn (+4 defence bonus); equal | `revolts.rs` |
| `REBEL_PLACE_RINGS` | `10` | Rings searched around the rising tile for a free tile per rebel. | Unciv: TileMap.placeUnitNearTile (10 tries over the neighbours of the tiles tried); equal | `revolts.rs` |
| `PUPPET_RESISTS` | `true` | Whether a puppet city is in resistance after its capture as an annexed one is (the owner's decision on the rule; Civ V: a captured city resists whether puppeted or annexed). | own value (source not recorded) | `mod.rs` |

## AI constants

Tuning numbers of the computer players (`classic/crates/classic-rules/src/ai`). They are heuristics of our own: generic, derived from rule data, no cheats.

### `air.rs`

9 constants (own value (source not recorded) 9). Files: `classic/crates/classic-rules/src/ai/air.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `STRIKERS_PER_OPERATION` | `2` | Strikers a running war operation asks for. | own value (source not recorded) | `air.rs` |
| `MAX_STRIKERS` | `4` | Strikers wanted at most (the aircraft are expensive). | own value (source not recorded) | `air.rs` |
| `MAX_FIGHTERS` | `2` | Fighters wanted at most. | own value (source not recorded) | `air.rs` |
| `MEND_BELOW_HP` | `70` | An aircraft below this health stays in its base to mend (aircraft heal in a city, about a quarter of their health a turn): a surprise interception, which the forecast cannot see, then finds it whole. | own value (source not recorded) | `air.rs` |
| `MIN_HP_AFTER` | `20` | A strike must leave the aircraft at least this many hit points. | own value (source not recorded) | `air.rs` |
| `SWEEP_MIN_HP` | `70` | A fighter sweeps only from this health. | own value (source not recorded) | `air.rs` |
| `SWEEP_ODDS_PERCENT` | `90` | A fighter sweeps an interceptor when its strength is at least this percent of the interceptor's. | own value (source not recorded) | `air.rs` |
| `MAX_PREVIEWS` | `24` | Previews one decision may compute. | own value (source not recorded) | `air.rs` |
| `REBASE_GAIN_PERCENT` | `50` | A rebase must raise the enemy value in reach by this percent. | own value (source not recorded) | `air.rs` |

### `allocator.rs`

11 constants (own value (source not recorded) 11). Files: `classic/crates/classic-rules/src/ai/allocator.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `MAX_SETTLERS` | `3` | Settlers on the way or queued at once (2 gave 0.4 fewer cities at round 100 and the same win share). | own value (source not recorded) | `allocator.rs` |
| `SETTLE_WALK_ROUNDS` | `6` | Rounds a settler walks to its site. | own value (source not recorded) | `allocator.rs` |
| `JOBS_PER_WORKER` | `4` | Unimproved worked tiles one worker serves. | own value (source not recorded) | `allocator.rs` |
| `IMPROVEMENT_YIELDS` | `150` | Yields an improvement adds, percent (a farm or a mine one, a resource's improvement two or three). | own value (source not recorded) | `allocator.rs` |
| `IMPROVE_ROUNDS` | `8` | Rounds a worker takes for one improvement, the walk included (Standard speed; × the speed's pace: `SpeedValues::pace_rounds`). | own value (source not recorded) | `allocator.rs` |
| `FIRST_WORKER_FACTOR` | `3` | The empire's first worker is worth this many times the yields of the jobs it takes, once a settler is on the way (or the second city stands): its first improvements, a luxury among them, come a settler's cycle sooner (600 seats at round 60: excess happiness +7.0 instead of +3.9; bench `pace`: cities at round 100 3.6 instead of 3.1; platform/docs/ai-bench.md, "Happiness and the first worker"). | own value (source not recorded) | `allocator.rs` |
| `BUILDING_TURNS` | `40` | A building taking longer than this is not ordered (production turns at Standard speed; × the speed's pace). | own value (source not recorded) | `allocator.rs` |
| `WONDER_TURNS` | `60` | A world wonder taking longer than this is not ordered (production turns at Standard speed; × the speed's pace). | own value (source not recorded) | `allocator.rs` |
| `WONDER_RISK_PER_TURN` | `2` | A wonder's chance to be ours falls by this many percent per turn it takes (another nation may finish it first); a turn at Standard speed (a turn of another speed counts 100 / its pace). | own value (source not recorded) | `allocator.rs` |
| `WONDER_MIN_CHANCE` | `30` | The least chance a long wonder keeps, percent. | own value (source not recorded) | `allocator.rs` |
| `TILES_PER_TURN` | `2` | Tiles bought in one turn. | own value (source not recorded) | `allocator.rs` |

### `barbarians.rs`

1 constants (own value (source not recorded) 1). Files: `classic/crates/classic-rules/src/ai/barbarians.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `HUNT_RANGE` | `8` | Barbarians go after targets no farther than this. | own value (source not recorded) | `barbarians.rs` |

### `battlefield.rs`

6 constants (own value (source not recorded) 6). Files: `classic/crates/classic-rules/src/ai/battlefield.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `MAX_TARGETS` | `12` | Enemy units a theatre models (the nearest to the members first). | own value (source not recorded) | `battlefield.rs` |
| `REACH_NODES` | `80` | Nodes of one enemy unit's reach search. | own value (source not recorded) | `battlefield.rs` |
| `FORTIFIED_PERCENT` | `20` | Fortification a unit that held its place last turn has (percent; `combat::strength::FORTIFICATION_BONUS` for one turn). | own value (source not recorded) | `battlefield.rs` |
| `CITY_TILE_PERCENT` | `40` | What a unit in an own city is taken to have against an attack, percent: the planning estimate (the city fights with its own strength). | own value (source not recorded) | `battlefield.rs` |
| `FOG_PRIOR_PERCENT` | `12` | The prior of a fogged tile next to the unit: the chance (percent) that an enemy of the typical kind stands on it. | own value (source not recorded) | `battlefield.rs` |
| `PRIOR_ROUNDS` | `20` | Rounds after which a remembered threat is no prior any more. | own value (source not recorded) | `battlefield.rs` |

### `camps.rs`

6 constants (own value (source not recorded) 6). Files: `classic/crates/classic-rules/src/ai/camps.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `CAMP_REACH` | `6` | Camps within this many tiles of an own city are the empire's to clear (Unciv's `tryHeadTowardsEncampment`: 6 tiles). | own value (source not recorded) | `camps.rs` |
| `GROUP_MOST` | `3` | Units a clearing group takes at most (a raid's, `raid::RAID_SIZE`). | own value (source not recorded) | `camps.rs` |
| `WALK_MOST` | `CAMP_REACH + 4` | Free units this far from a camp, beyond its reach of the cities, may go. | own value (source not recorded) | `camps.rs` |
| `LOSS_PER_CAMP_ROUND` | `32` | Losses to barbarians (civilians captured, military units killed) per round of a camp within `CAMP_REACH` of a city, per mille: `standard` on the standard and duel suites at Quick, 80 games, 14.9 losses a seat for 462 such camp-rounds (classic/docs/ai-army.md). | own value (source not recorded) | `camps.rs` |
| `CAMP_LIFE` | `95` | The rounds a camp near the cities still lives when the player leaves it, in Standard rounds: the measured median life of a camp, 63 rounds at Quick's pace (2/3). | own value (source not recorded) | `camps.rs` |
| `READY_HP` | `80` | A member goes on only this healthy (a full unit strikes the defender twice). | own value (source not recorded) | `camps.rs` |

### `capital_search.rs`

4 constants (own value (source not recorded) 4). Files: `classic/crates/classic-rules/src/ai/capital_search.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `OUTWARD_PER_TILE` | `10` | Score of a frontier tile per tile it lies farther from our capital (a revealed tile scores 10): the scouts head away from home. | own value (source not recorded) | `capital_search.rs` |
| `LEAD_BONUS` | `150` | Score of a frontier tile this close to a known tile of a holder of a capital we look for: the scouts walk its borders. | own value (source not recorded) | `capital_search.rs` |
| `LEAD_RADIUS` | `3` |  | own value (source not recorded) | `capital_search.rs` |
| `CAMP_RADIUS` | `2` | A frontier tile this close to a known barbarian camp is no target: the scouts that chased one died there or walked home to heal again and again. | own value (source not recorded) | `capital_search.rs` |

### `carrier.rs`

1 constants (own value (source not recorded) 1). Files: `classic/crates/classic-rules/src/ai/carrier.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `STANDOFF` | `crate::combat::city::CITY_ATTACK_RANGE + 1` | The carrier stays this far from its target: the city's strike (`operations::STAGING_DISTANCE` is the army's). | own value (source not recorded) | `carrier.rs` |

### `choice.rs`

13 constants (own value (source not recorded) 13). Files: `classic/crates/classic-rules/src/ai/choice.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `TOP` | `4` | Candidates of a choice set: the allocator's best this many. | own value (source not recorded) | `choice.rs` |
| `NET` | `ChoiceMode::Net { production: true, research: true }` | The net for both choices. | own value (source not recorded) | `choice.rs` |
| `RESEARCH_KEY` | `u64::MAX` | The `key` of a research choice for `Explore::pick`. | own value (source not recorded) | `choice.rs` |
| `EMPIRE_FEATURES` | `20` |  | own value (source not recorded) | `choice.rs` |
| `CITY_FEATURES` | `16` |  | own value (source not recorded) | `choice.rs` |
| `PRODUCTION_FEATURES` | `43` |  | own value (source not recorded) | `choice.rs` |
| `RESEARCH_FEATURES` | `42` |  | own value (source not recorded) | `choice.rs` |
| `PRODUCTION_INPUTS` | `EMPIRE_FEATURES + CITY_FEATURES + PRODUCTION_FEATURES` | A production row: the empire, the city, the candidate. | own value (source not recorded) | `choice.rs` |
| `RESEARCH_INPUTS` | `EMPIRE_FEATURES + RESEARCH_FEATURES` | A research row: the empire, the candidate. | own value (source not recorded) | `choice.rs` |
| `EMPIRE_NAMES` | `[ "round", "cities", "citizens", "beakers", "gold_income", "treasury", "happiness", "te...` |  | own value (source not recorded) | `choice.rs` |
| `CITY_NAMES` | `[ "city_population", "city_food", "city_food_surplus", "city_production", "city_science...` |  | own value (source not recorded) | `choice.rs` |
| `PRODUCTION_NAMES` | `[ "is_settler", "is_worker", "is_sea_worker", "is_trade_unit", "is_unit", "is_building"...` |  | own value (source not recorded) | `choice.rs` |
| `RESEARCH_NAMES` | `[ "tech_cost", "tech_turns", "value_per_beaker", "permille_of_best", "rank", "own_value...` |  | own value (source not recorded) | `choice.rs` |

### `choice_weights.rs`

2 constants (own value (source not recorded) 2). Files: `classic/crates/classic-rules/src/ai/choice_weights.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `PRODUCTION_MARGIN` | `32768` |  | own value (source not recorded) | `choice_weights.rs` |
| `RESEARCH_MARGIN` | `32768` |  | own value (source not recorded) | `choice_weights.rs` |

### `citizens.rs`

8 constants (own value (source not recorded) 8). Files: `classic/crates/classic-rules/src/ai/citizens.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `SMALL_CITY` | `4` | Population below which a city favours food. | own value (source not recorded) | `citizens.rs` |
| `GOLD_COMFORT` | `3` | Gold a turn a gold-focused empire earns before its cities go back to their usual focus. | own value (source not recorded) | `citizens.rs` |
| `GOLD_FOCUS_TURNS` | `20` | Turns of deficit the treasury must cover before the cities work gold. | own value (source not recorded) | `citizens.rs` |
| `KEEP_SPECIALIST_PERCENT` | `10` | Percent a filled specialist slot is worth beyond the tile it replaces before it is given up: a city does not move a citizen between a slot and a tile each turn as the prices move by a little. | own value (source not recorded) | `citizens.rs` |
| `AVOID_GROWTH_AT` | `-7` | Excess happiness at which the cities stop growing. | own value (source not recorded) | `citizens.rs` |
| `RESUME_GROWTH_AT` | `-5` | Excess happiness at which they grow again. | own value (source not recorded) | `citizens.rs` |
| `STANDARD` | `GrowthPolicy { avoid_at: AVOID_GROWTH_AT, resume_at: RESUME_GROWTH_AT }` | The game's AI: the cities grow while the empire is unhappy (the rules slow the growth by 75 % from −1 and stop it below −10, where production falls by half) and stop only near that line, at −7; they grow again at −5. | own value (source not recorded) | `citizens.rs` |
| `CAUTIOUS` | `GrowthPolicy { avoid_at: -2, resume_at: 2 }` | The old line: stop at −2, grow again at +2, so from round ~100 every city of an expanding empire stood at 2–3 citizens (`Variant::CautiousGrowth`, the control). | own value (source not recorded) | `citizens.rs` |

### `city_states.rs`

4 constants (own value (unverified) 2, own value (source not recorded) 2). Files: `classic/crates/classic-rules/src/ai/city_states.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `GIFT_RESERVE` | `300` | Gold the AI keeps before it gives any away. | own value (source not recorded) | `city_states.rs` |
| `SMALL_GIFT` | `250` | The gift sizes (own values, unverified). | own value (unverified) | `city_states.rs` |
| `MEDIUM_GIFT` | `500` | (with `SMALL_GIFT`) The gift sizes (own values, unverified). | own value (unverified) | `city_states.rs` |
| `LUMP_MARGIN` | `10` | Points of influence a seat aims above its estimate of a rival's influence in its ally when it takes the alliance in one lump (the winners' alliances hold 73 points at the median and 94 at the third quartile at the last round of the bench, against 80 for the seat's own: `runs/m1`). | own value (source not recorded) | `city_states.rs` |

### `combat.rs`

3 constants (own value (source not recorded) 3). Files: `classic/crates/classic-rules/src/ai/combat.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `ANNEX_POPULATION` | `4` | Citizens of a captured city the AI annexes; a smaller one is a puppet. | own value (source not recorded) | `combat.rs` |
| `NONE` | `FightFix { source: Source::Search, retreat: false, reply: false, focus: false, odds: fa...` |  | own value (source not recorded) | `combat.rs` |
| `UPGRADE_RESERVE` | `150` | Gold the military advisor keeps when it upgrades units. | own value (source not recorded) | `combat.rs` |

### `commander.rs`

1 constants (own value (source not recorded) 1). Files: `classic/crates/classic-rules/src/ai/unit_net/commander.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `OPTIONS` | `["attack", "wait", "hold", "retreat"]` | The operation head's options (design section 6). | own value (source not recorded) | `commander.rs` |

### `danger.rs`

4 constants (own value (source not recorded) 4). Files: `classic/crates/classic-rules/src/ai/danger.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `FEATURES` | `37` | Values in a row of `unit_features`. | own value (source not recorded) | `danger.rs` |
| `NAMES` | `[ "hp", "melee", "ranged", "range", "moves", "level", "promotions", "fortified", "forti...` | The names of the values, in the row's order (the module's table). | own value (source not recorded) | `danger.rs` |
| `THREATS` | `19` | The column of `threats`: the net reads only rows where it is above 0. | own value (source not recorded) | `danger.rs` |
| `FAR` | `20` | A distance this large or larger, or none, is written as this. | own value (source not recorded) | `danger.rs` |

### `decide.rs`

4 constants (own value (source not recorded) 4). Files: `classic/crates/classic-rules/src/ai/net_e2e/v2/decide.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `HEAD_UNIT` | `0` |  | own value (source not recorded) | `decide.rs` |
| `HEAD_PRODUCTION` | `2` |  | own value (source not recorded) | `decide.rs` |
| `WAIT_TAG` | `250` |  | own value (source not recorded) | `decide.rs` |
| `DEFER_TAG` | `251` |  | own value (source not recorded) | `decide.rs` |

### `defence.rs`

5 constants (own value (source not recorded) 5). Files: `classic/crates/classic-rules/src/ai/defence.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `THREAT_RADIUS` | `crate::ai::planner_strategy::THREAT_RADIUS` | Hostile units this close to an own city threaten it. | own value (source not recorded) | `defence.rs` |
| `GUARD_RADIUS` | `2` | Own units this close to a city guard it. | own value (source not recorded) | `defence.rs` |
| `RESPONSE_ROUNDS` | `crate::ai::front::SIEGE_ROUNDS` | A unit joins a guard it reaches within this many rounds: a ready siege takes a city within the front's `SIEGE_ROUNDS`. | own value (source not recorded) | `defence.rs` |
| `STRANGER_PERCENT` | `50` | Weight of the field units of a nation at peace that has done nothing to lose our trust, percent of an enemy's: an army gathering at our border for many rounds before its war is the warning (bench probe: 5–20 rounds), a few passing units count for little (`chance`). | own value (source not recorded) | `defence.rs` |
| `CHANCE_POWER` | `4` | The power of the ratio of threat and guard in `chance`. | own value (source not recorded) | `defence.rs` |

### `diplomacy.rs`

7 constants (own value (unverified) 1, own value (source not recorded) 6). Files: `classic/crates/classic-rules/src/ai/diplomacy.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `FRIENDSHIP_MIN_CONTACT_ROUNDS` | `20` | No friendship before this many rounds of contact (Standard speed; × the speed's pace); own value, unverified. | own value (unverified) | `diplomacy.rs` |
| `ACCEPT_PEACE_AFTER_ROUNDS` | `crate::systems::diplomacy::deals::MIN_WAR_ROUNDS_BEFORE_PEACE` | The plain proposals (`proposals`) offer peace to a rival only after twice this many rounds of war. | own value (source not recorded) | `diplomacy.rs` |
| `DISTRUST_WARMONGER` | `100` | Warmonger memory above which the AI distrusts (no friendship, pacts). | own value (source not recorded) | `diplomacy.rs` |
| `DENOUNCE_WARMONGER` | `250` | Warmonger memory above which the AI denounces. | own value (source not recorded) | `diplomacy.rs` |
| `RECENT_WAR_ROUNDS` | `30` | No friendly deal with a former enemy for this many rounds after the peace treaty ends. | own value (source not recorded) | `diplomacy.rs` |
| `REFUSED` | `-1000` | The gold-scale value of an item the AI refuses outright (`item_value_priced`'s untrusted). | own value (source not recorded) | `diplomacy.rs` |
| `PROPOSAL_INTERVAL` | `5` | Rounds between two rounds of proposals of one player. | own value (source not recorded) | `diplomacy.rs` |

### `driver.rs`

3 constants (own value (source not recorded) 3). Files: `classic/crates/classic-rules/src/ai/driver.rs`, `platform/crates/kernel/src/ai/driver.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `VARIANT_ENV` | `"AURELIA_AI_VARIANT"` | The environment variable that makes the game's AI another variant (dev-only: `AURELIA_AI_VARIANT=net` plays the net in the game window and the observer; an unknown name is an error). | own value (source not recorded) | `driver.rs` |
| `DEFERS_PER_UNIT` | `1` | How many times a turn a unit's decision may be `Reason::Deferred` and the unit moved to the end of the turn's queue: the mind asks (no command, reason `Deferred`), the driver grants it while the unit has deferrals left; a request beyond them ends the unit's turn as no command does. | own value (source not recorded) | `driver.rs` |
| `REORDERS_PER_UNIT` | `3` | Deferrals a unit has a turn when its mind picks the next unit itself (`AiMind::picks_next`, "who is next"): the mind may hold a unit back while others act and call it again later, so it needs more than one; a bound keeps every turn finite (the queue grows by at most this many times its length). | own value (source not recorded) | `driver.rs` |

### `economy.rs`

4 constants (own value (source not recorded) 4). Files: `classic/crates/classic-rules/src/ai/economy.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `GREAT_PERSON_PRIORITY` | `20` | Priority of a great person action (before ordinary economy orders). | own value (source not recorded) | `economy.rs` |
| `HAPPINESS_PRIORITY` | `10` | Priority of a happiness building order. | own value (source not recorded) | `economy.rs` |
| `TRADE_PRIORITY` | `8` | Priority of a trade route and of a trade unit order. | own value (source not recorded) | `economy.rs` |
| `FOCUS_PRIORITY` | `5` | Priority of a city focus change. | own value (source not recorded) | `economy.rs` |

### `effects.rs`

2 constants (own value (source not recorded) 2). Files: `classic/crates/classic-rules/src/ai/effects.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `NONE` | `EffectChecks { moves: false, demand: false, jobs: false }` |  | own value (source not recorded) | `effects.rs` |
| `ALL` | `EffectChecks { moves: true, demand: true, jobs: true }` |  | own value (source not recorded) | `effects.rs` |

### `encode.rs`

55 constants (own value (source not recorded) 55). Files: `classic/crates/classic-rules/src/ai/net_e2e/encode.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `FORMAT` | `10` | The format number of the contract (`meta.json`). | own value (source not recorded) | `encode.rs` |
| `JOBS_PER_WORKER` | `4` | A worker serves this many jobs (the scripted mind's `genome::Knob::JobsPerWorker`): `free_worker_jobs` = jobs − 4 × workers alive or queued. | own value (source not recorded) | `encode.rs` |
| `CLIP` | `32` | Distances are clipped here; "none" (no capital, no city) is `CLIP`. | own value (source not recorded) | `encode.rs` |
| `REGION` | `4` | Tiles of a region's side (blocks of `REGION` × `REGION` tiles). | own value (source not recorded) | `encode.rs` |
| `MARCH_NEAR` | `2` | A march matches a point of interest at most this far from its destination (the nearest one; exact first). | own value (source not recorded) | `encode.rs` |
| `SITE_RADIUS` | `10` | City sites are offered to a founder within this many tiles. | own value (source not recorded) | `encode.rs` |
| `MARCH` | `crate::knowledge::actions::MARCH_TAG as i32` | A long march (`Command::MarchTo`) to a point of interest. | own value (source not recorded) | `encode.rs` |
| `WAIT` | `250` | The unit does nothing more this turn (no command). | own value (source not recorded) | `encode.rs` |
| `DEFER` | `251` | Format 9: the unit decides again after the rest of the turn's queue (no command; `defer_candidate`). | own value (source not recorded) | `encode.rs` |
| `RESEARCH` | `300` |  | own value (source not recorded) | `encode.rs` |
| `PRODUCE_UNIT` | `400` |  | own value (source not recorded) | `encode.rs` |
| `PRODUCE_BUILDING` | `401` |  | own value (source not recorded) | `encode.rs` |
| `PRODUCE_PROJECT` | `402` |  | own value (source not recorded) | `encode.rs` |
| `ADOPT_POLICY` | `500` |  | own value (source not recorded) | `encode.rs` |
| `UNLOCK_BRANCH` | `501` |  | own value (source not recorded) | `encode.rs` |
| `CHOOSE_IDEOLOGY` | `502` |  | own value (source not recorded) | `encode.rs` |
| `PANTHEON` | `600` | `FoundPantheon`: the pantheon belief. | own value (source not recorded) | `encode.rs` |
| `FOUND_FOUNDER` | `601` | `FoundReligion`, one decision per belief slot in this order: founder, follower, the pantheon belief (only without a pantheon), the bonus belief (only with an extra founding belief). | own value (source not recorded) | `encode.rs` |
| `FOUND_FOLLOWER` | `602` |  | own value (source not recorded) | `encode.rs` |
| `FOUND_PANTHEON` | `603` |  | own value (source not recorded) | `encode.rs` |
| `FOUND_BONUS` | `604` |  | own value (source not recorded) | `encode.rs` |
| `ENHANCE_FOLLOWER` | `605` | `EnhanceReligion`: follower, then enhancer. | own value (source not recorded) | `encode.rs` |
| `ENHANCE_ENHANCER` | `606` |  | own value (source not recorded) | `encode.rs` |
| `REFORMATION` | `607` | `AddReformationBelief`. | own value (source not recorded) | `encode.rs` |
| `FAITH_UNIT` | `610` | `PurchaseWithFaith` of a unit or a building in one own city. | own value (source not recorded) | `encode.rs` |
| `FAITH_BUILDING` | `611` |  | own value (source not recorded) | `encode.rs` |
| `FAITH_SAVE` | `612` | Keep the faith (no purchase). | own value (source not recorded) | `encode.rs` |
| `FOCUS` | `700` | A city's focus (`SetCityFocus`); parameter the focus code (`CityFocus::code`). | own value (source not recorded) | `encode.rs` |
| `AVOID_GROWTH` | `701` | A city's avoid growth (`SetAvoidGrowth`); parameter 0 grow, 1 avoid. | own value (source not recorded) | `encode.rs` |
| `SPECIALISTS` | `702` | A city's hand-assigned specialists of one kind (`SetSpecialists`); parameter `kind * 100 + count` (0: the automatic assignment). | own value (source not recorded) | `encode.rs` |
| `UNIT` | `0` |  | own value (source not recorded) | `encode.rs` |
| `RESEARCH` | `1` |  | own value (source not recorded) | `encode.rs` |
| `PRODUCTION` | `2` |  | own value (source not recorded) | `encode.rs` |
| `POLICY` | `3` |  | own value (source not recorded) | `encode.rs` |
| `RELIGION` | `4` |  | own value (source not recorded) | `encode.rs` |
| `CITIZENS` | `5` |  | own value (source not recorded) | `encode.rs` |
| `NAMES` | `["unit", "research", "production", "policy", "religion", "citizens"]` |  | own value (source not recorded) | `encode.rs` |
| `OWN_UNIT` | `0` |  | own value (source not recorded) | `encode.rs` |
| `OWN_CITY` | `1` |  | own value (source not recorded) | `encode.rs` |
| `FOREIGN_UNIT` | `2` |  | own value (source not recorded) | `encode.rs` |
| `FOREIGN_CITY` | `3` |  | own value (source not recorded) | `encode.rs` |
| `CITY_STATE` | `4` |  | own value (source not recorded) | `encode.rs` |
| `PLAYER` | `5` |  | own value (source not recorded) | `encode.rs` |
| `OWN` | `0` |  | own value (source not recorded) | `encode.rs` |
| `FRIEND` | `1` | A nation we declared friendship or a defensive pact with. | own value (source not recorded) | `encode.rs` |
| `NEUTRAL` | `2` | A met nation at peace. | own value (source not recorded) | `encode.rs` |
| `WAR` | `3` | At war (a city-state at war included; barbarians are `BARBARIAN`). | own value (source not recorded) | `encode.rs` |
| `CITY_STATE` | `4` |  | own value (source not recorded) | `encode.rs` |
| `BARBARIAN` | `5` |  | own value (source not recorded) | `encode.rs` |
| `UNMET` | `6` | A player we have not met. | own value (source not recorded) | `encode.rs` |
| `COUNT` | `7` |  | own value (source not recorded) | `encode.rs` |
| `FIELDS` | `&[$(($label, Scale { num: $num, den: $den })),*]` |  | own value (source not recorded) | `encode.rs` |
| `COUNT` | `Self::FIELDS.len()` |  | own value (source not recorded) | `encode.rs` |
| `FORMAT_9_CANDIDATE_COLUMNS` | `C::MoveBlowCaptures as usize` | The candidate columns of a format-9 file: the format-10 columns (`C::MoveBlowCaptures` on) are appended after them, so a format-9 net reads the first `FORMAT_9_CANDIDATE_COLUMNS` of every row (`mind::layout_text_of`). | own value (source not recorded) | `encode.rs` |
| `STANDARD` | `SpeedFacts { production_percent: 100, science_percent: 100, culture_percent: 100, faith...` |  | own value (source not recorded) | `encode.rs` |

### `espionage.rs`

4 constants (own value (source not recorded) 4). Files: `classic/crates/classic-rules/src/ai/espionage.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `SLOW_THEFT_TURNS` | `50` | A theft slower than this is not worth the spy: it looks for another city. | own value (source not recorded) | `espionage.rs` |
| `COUP_MIN_CHANCE` | `60` | Coups the AI tries (percent chance of success). | own value (source not recorded) | `espionage.rs` |
| `GUARD_COUP_MIN_CHANCE` | `30` | Coups the vote guard tries on a rival's alliance (percent chance of success): a failure costs the spy (revived after `REVIVE_TURNS`) and 20 points of influence, a success takes the alliance whole (the spy's owner takes the ally's influence, the ally loses 20). | own value (source not recorded) | `espionage.rs` |
| `GUARD_COUP_WINDOW` | `4` | Our turns (at Standard speed) before the session that holds the vote opens within which the guard stages its coups: late, so that the rival cannot buy the alliance back; the delegates are fixed when the session opens. | own value (source not recorded) | `espionage.rs` |

### `events.rs`

24 constants (own value (source not recorded) 24). Files: `classic/crates/classic-rules/src/ai/net_e2e/events.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `KEEP_ROUNDS` | `10` | Events older than this many rounds are forgotten (age 0 … 9). | own value (source not recorded) | `events.rs` |
| `MAX_EVENTS` | `64` | At most this many events in a log (the newest). | own value (source not recorded) | `events.rs` |
| `MAX_PER_KIND` | `8` | At most this many events of one kind in a log (the newest). | own value (source not recorded) | `events.rs` |
| `WAR_BEGAN` | `0` | We are at war with a nation or city-state we were at peace with. | own value (source not recorded) | `events.rs` |
| `PEACE_MADE` | `1` | A war of ours ended. | own value (source not recorded) | `events.rs` |
| `MET` | `2` | We met a nation or city-state. | own value (source not recorded) | `events.rs` |
| `ELIMINATED` | `3` | A nation or city-state we knew is gone. | own value (source not recorded) | `events.rs` |
| `FRIENDSHIP` | `4` | A friendship began (value 1) or ended (−1). | own value (source not recorded) | `events.rs` |
| `DEFENSIVE_PACT` | `5` | A defensive pact began (1) or ended (−1). | own value (source not recorded) | `events.rs` |
| `DENOUNCED_US` | `6` | A nation denounced us. | own value (source not recorded) | `events.rs` |
| `CITY_LOST` | `7` | A city of ours is no longer ours (taken, or razed). | own value (source not recorded) | `events.rs` |
| `CITY_TAKEN` | `8` | We took a city (it was another player's). | own value (source not recorded) | `events.rs` |
| `CITY_ATTACKED` | `9` | A city of ours was damaged. | own value (source not recorded) | `events.rs` |
| `FOREIGN_CITY` | `10` | A foreign city we did not know: founded, or first seen. | own value (source not recorded) | `events.rs` |
| `CITY_CHANGED_HANDS` | `11` | A foreign city we know changed hands between other players. | own value (source not recorded) | `events.rs` |
| `UNIT_LOST` | `12` | A unit of ours is gone (not by founding a city or by an upgrade). | own value (source not recorded) | `events.rs` |
| `UNITS_DAMAGED` | `13` | Units of ours were damaged (one event per turn). | own value (source not recorded) | `events.rs` |
| `WONDER_ELSEWHERE` | `14` | A world wonder was completed by another player. | own value (source not recorded) | `events.rs` |
| `RELIGION_FOUNDED` | `15` | Another player founded a religion. | own value (source not recorded) | `events.rs` |
| `QUEST_GIVEN` | `16` | A city-state gave us a quest. | own value (source not recorded) | `events.rs` |
| `QUEST_ENDED` | `17` | A quest of a city-state to us ended (done, or expired). | own value (source not recorded) | `events.rs` |
| `ALLY_CHANGED` | `18` | A city-state's ally changed. | own value (source not recorded) | `events.rs` |
| `ERA_REACHED` | `19` | A nation we know entered a later era. | own value (source not recorded) | `events.rs` |
| `NAMES` | `[ "war_began", "peace_made", "met", "eliminated", "friendship", "defensive_pact", "deno...` |  | own value (source not recorded) | `events.rs` |

### `features.rs`

12 constants (own value (source not recorded) 12). Files: `classic/crates/classic-rules/src/ai/strategy_net/features.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `SURVEY_FEATURES` | `[CAP_FOOD, CAP_PROD, CAP_LUX, ROOM, SITES_FOOD, RIVAL_NEAR, RIVAL_KNOWN, RUINS, CARAVAN...` | The features the survey keeps, in this order. | own value (source not recorded) | `features.rs` |
| `SURVEYED` | `16` |  | own value (source not recorded) | `features.rs` |
| `BEST_TILES` | `12` | Tiles of the capital's best tiles the land is judged by (a city of about this size). | own value (source not recorded) | `features.rs` |
| `ROOM_RADIUS` | `10` | The radius in which free sites are counted. | own value (source not recorded) | `features.rs` |
| `SITE_SPACING` | `4` | Tiles between two sites. | own value (source not recorded) | `features.rs` |
| `ROOM_SITES` | `6` | Sites counted at most. | own value (source not recorded) | `features.rs` |
| `RIVAL_NEAR_FULL` | `7` | The nearest rival city's distance as the `RIVAL_NEAR` feature: 1 at `RIVAL_NEAR_FULL` tiles or closer (its cities' rings and ours meet on the sites between, three tiles each way and one between), 0 from `RIVAL_NEAR_NONE` (farther than an operation's reach, `planner_strategy::OBJECTIVE_RANGE`, by a city's ring) on, linear between. | own value (source not recorded) | `features.rs` |
| `RIVAL_NEAR_NONE` | `18` |  | own value (source not recorded) | `features.rs` |
| `RUINS_RADIUS` | `12` | The radius in which ruins near home are counted. | own value (source not recorded) | `features.rs` |
| `CITY_STATE_NEAR` | `12` | The radius in which a city-state is near. | own value (source not recorded) | `features.rs` |
| `BARBARIAN_NEAR` | `6` | Barbarian units this near a city press it. | own value (source not recorded) | `features.rs` |
| `HOSTILE_NEAR` | `8` | Hostile units this near a city count against our force. | own value (source not recorded) | `features.rs` |

### `field_craft.rs`

3 constants (own value (source not recorded) 3). Files: `classic/crates/classic-rules/src/ai/field_craft.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `SORTIE_ROUNDS` | `3` | Rounds of walking a sortie looks ahead for prey: a trip that takes longer is the raid's (`raid::MAX_ARRIVAL_ROUNDS` is the raid's own limit, a war lasts a few rounds). | own value (source not recorded) | `field_craft.rs` |
| `SORTIE_TRIES` | `8` | Prey tried a turn, nearest first: the way to one may not exist (the sea, a border) and the nearest few may all lie across it. | own value (source not recorded) | `field_craft.rs` |
| `GUARD_MIN_HP` | `crate::ai::planner_units::HEAL_BELOW_HP` | Hit points a unit needs to count as a guard of a tile: below the health of the wounded (`planner_units::HEAL_BELOW_HP`) it is one more thing to capture. | own value (source not recorded) | `field_craft.rs` |

### `fields.rs`

40 constants (own value (source not recorded) 40). Files: `classic/crates/classic-rules/src/ai/unit_net/fields.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `PASSABLE` | `0` |  | own value (source not recorded) | `fields.rs` |
| `MOVE_COST` | `1` |  | own value (source not recorded) | `fields.rs` |
| `DEFENCE` | `2` |  | own value (source not recorded) | `fields.rs` |
| `BLOCKS_SIGHT` | `3` |  | own value (source not recorded) | `fields.rs` |
| `RIVER` | `4` |  | own value (source not recorded) | `fields.rs` |
| `TERRITORY` | `5` |  | own value (source not recorded) | `fields.rs` |
| `OURS` | `6` |  | own value (source not recorded) | `fields.rs` |
| `OURS_RANGED` | `7` |  | own value (source not recorded) | `fields.rs` |
| `THEIRS` | `8` |  | own value (source not recorded) | `fields.rs` |
| `THEIRS_RANGED` | `9` |  | own value (source not recorded) | `fields.rs` |
| `HIDDEN` | `10` |  | own value (source not recorded) | `fields.rs` |
| `CITY` | `11` |  | own value (source not recorded) | `fields.rs` |
| `OUR_REACH1` | `12` |  | own value (source not recorded) | `fields.rs` |
| `OUR_REACH2` | `13` |  | own value (source not recorded) | `fields.rs` |
| `THEIR_REACH1` | `14` |  | own value (source not recorded) | `fields.rs` |
| `THEIR_REACH2` | `15` |  | own value (source not recorded) | `fields.rs` |
| `ZOC` | `16` |  | own value (source not recorded) | `fields.rs` |
| `DANGER` | `17` |  | own value (source not recorded) | `fields.rs` |
| `SUPPORT` | `18` |  | own value (source not recorded) | `fields.rs` |
| `FOCUS` | `19` |  | own value (source not recorded) | `fields.rs` |
| `OP_TARGET` | `20` |  | own value (source not recorded) | `fields.rs` |
| `TARGET_TURNS` | `21` |  | own value (source not recorded) | `fields.rs` |
| `HEAL` | `22` |  | own value (source not recorded) | `fields.rs` |
| `PILLAGE` | `23` |  | own value (source not recorded) | `fields.rs` |
| `N` | `24` |  | own value (source not recorded) | `fields.rs` |
| `OURS2` | `0` |  | own value (source not recorded) | `fields.rs` |
| `THEIRS2` | `1` |  | own value (source not recorded) | `fields.rs` |
| `BALANCE` | `2` |  | own value (source not recorded) | `fields.rs` |
| `FRONT` | `3` |  | own value (source not recorded) | `fields.rs` |
| `FRONT_ENEMY` | `4` |  | own value (source not recorded) | `fields.rs` |
| `CHOKE` | `5` |  | own value (source not recorded) | `fields.rs` |
| `GAP` | `6` |  | own value (source not recorded) | `fields.rs` |
| `SIEGE` | `7` |  | own value (source not recorded) | `fields.rs` |
| `CUT_OFF` | `8` |  | own value (source not recorded) | `fields.rs` |
| `OUR_CUT` | `9` |  | own value (source not recorded) | `fields.rs` |
| `SAFE2` | `10` |  | own value (source not recorded) | `fields.rs` |
| `COHESION` | `11` |  | own value (source not recorded) | `fields.rs` |
| `LONELY` | `12` |  | own value (source not recorded) | `fields.rs` |
| `FLOW` | `13` |  | own value (source not recorded) | `fields.rs` |
| `N` | `14` |  | own value (source not recorded) | `fields.rs` |

### `fit.rs`

2 constants (own value (source not recorded) 2). Files: `classic/crates/classic-rules/src/ai/unit_net/fit.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `HAND` | `Numbers { table: rows(), war: [weights::prices::POSTURES[3].kill, weights::prices::POST...` | The hand weights (`weights`): what `unit-net` and `hybrid` play. | own value (source not recorded) | `fit.rs` |
| `FITTED` | `HAND` | The fitted numbers (`tactician-fit`); see the doc for the bank, the method and what moved. | own value (source not recorded) | `fit.rs` |

### `fleet.rs`

1 constants (own value (source not recorded) 1). Files: `classic/crates/classic-rules/src/ai/fleet.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `FLEET_SIZE` | `2` | Warships an operation against a coastal city wants. | own value (source not recorded) | `fleet.rs` |

### `front.rs`

7 constants (own value (source not recorded) 7). Files: `classic/crates/classic-rules/src/ai/front.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `MAX_MEMBERS` | `12` | Members a front plans for, the nearest to the asked unit first. | own value (source not recorded) | `front.rs` |
| `MAX_PREVIEWS` | `120` | Combat previews one decision may compute. | own value (source not recorded) | `front.rs` |
| `HEAL_BELOW` | `crate::ai::planner_units::HEAL_BELOW_HP` | A member below this health leaves the front to heal: the one health of the wounded (`planner_units::HEAL_BELOW_HP`). | own value (source not recorded) | `front.rs` |
| `STRIKE_KEEPS_HP` | `25` | A melee strike on a city that is not taken leaves the attacker this much. | own value (source not recorded) | `front.rs` |
| `SIEGE_ROUNDS` | `4` | The siege starts when the group would take the city within this many rounds of strikes (`siege_rounds`). | own value (source not recorded) | `front.rs` |
| `GROUP_RADIUS` | `4` | Members this close to a city make its siege group. | own value (source not recorded) | `front.rs` |
| `STAGING_DISTANCE` | `CITY_ATTACK_RANGE + 1` | The gathering ring around an enemy city: just out of its strike. | own value (source not recorded) | `front.rs` |

### `genome.rs`

2 constants (own value (source not recorded) 2). Files: `classic/crates/classic-rules/src/ai/genome.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `SPECS` | `&[ $( Spec { knob: Knob::$variant, name: $name, default: $default as i64, min: $min, ma...` | Every knob with its default, range and meaning. | own value (source not recorded) | `genome.rs` |
| `COUNT` | `SPECS.len()` | Number of knobs. | own value (source not recorded) | `genome.rs` |

### `grand_strategy.rs`

13 constants (own value (source not recorded) 13). Files: `classic/crates/classic-rules/src/ai/grand_strategy.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `ALL` | `[VictoryPath::Score, VictoryPath::Domination, VictoryPath::Science, VictoryPath::Cultur...` |  | own value (source not recorded) | `grand_strategy.rs` |
| `REVIEW_ROUNDS` | `10` | Rounds between two reviews of the path (staggered by player). | own value (source not recorded) | `grand_strategy.rs` |
| `SWITCH_PERCENT` | `130` | Another path replaces the current one only when its value is this share of the current one's, percent. | own value (source not recorded) | `grand_strategy.rs` |
| `FULL_LEANING_PERCENT` | `200` | The weight of a path at the top of its leaning (10), percent of the score path's; 0 at neutral (5) and below. | own value (source not recorded) | `grand_strategy.rs` |
| `FOUNDATION_PERCENT` | `100` | The weight of the spaceship path for every leader of a planner (`Variant::Planner`), percent of the score path's. | own value (source not recorded) | `grand_strategy.rs` |
| `PATH_YIELD_PERCENT` | `150` | The price of the chosen path's yield, percent. | own value (source not recorded) | `grand_strategy.rs` |
| `SCORE_POWER` | `4` | The power of the scores in the chance of the time victory: the best score wins, a lead counts more than its share. | own value (source not recorded) | `grand_strategy.rs` |
| `ROUNDS_PER_CAPITAL` | `30` | Rounds one war operation takes: forming, marching, the siege (Standard speed; × the speed's pace: its forming is production-bound). | own value (source not recorded) | `grand_strategy.rs` |
| `NEAR_CAPITAL_CHANCE` | `350` | Chance one war operation takes a rival's original capital known within its reach (`OBJECTIVE_RANGE` of an own city), and farther or not yet found, permille (step 3: of the operations that marched with their army, about a third took their city). | own value (source not recorded) | `grand_strategy.rs` |
| `FAR_CAPITAL_CHANCE` | `150` |  | own value (source not recorded) | `grand_strategy.rs` |
| `REACHABLE_CHANCE` | `500` | Chance of a science, culture or diplomatic victory in reach with no time to spare, permille (rivals race too); more with time to spare. | own value (source not recorded) | `grand_strategy.rs` |
| `SPACESHIP_ROUNDS` | `25` | Rounds the spaceship takes once its technologies are known (Standard speed; × the speed's pace). | own value (source not recorded) | `grand_strategy.rs` |
| `WAR_STEP` | `20` | Percent of a war's weight one point of aggression or war tolerance adds. | own value (source not recorded) | `grand_strategy.rs` |

### `greedy.rs`

1 constants (own value (source not recorded) 1). Files: `classic/crates/classic-rules/src/ai/greedy.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `PEACE_AFTER_UNSEEN_ROUNDS` | `5` | Rounds without sight of an enemy after which the AI ends the war: it offers peace to a nation and makes it with a city-state. | own value (source not recorded) | `greedy.rs` |

### `improvements.rs`

4 constants (own value (source not recorded) 4). Files: `classic/crates/classic-rules/src/ai/improvements.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `WORKER_SEARCH_RADIUS` | `6` | How far a worker looks for work. | own value (source not recorded) | `improvements.rs` |
| `DISTANCE_PENALTY` | `10` | Value lost per hex the worker must walk. | own value (source not recorded) | `improvements.rs` |
| `STRATEGIC_CONNECTION` | `60` | Yield value of connecting a strategic resource (its units). | own value (source not recorded) | `improvements.rs` |
| `LUXURY_CONNECTION` | `80` | Yield value of connecting a luxury the empire does not have (happiness). | own value (source not recorded) | `improvements.rs` |

### `infamy.rs`

2 constants (own value (source not recorded) 2). Files: `classic/crates/classic-rules/src/ai/infamy.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `CENTI_PER_POINT` | `6` | Hundredths of a score point one point of warmonger memory is worth: a nation that remembers 400 points (one strike) distrusts, denounces and will not make peace but for 24 points of compensation (own design, not tuned: a city of six citizens is about 100 points). | own value (source not recorded) | `infamy.rs` |
| `WAR_BONUS_PERCENT` | `50` | The part of the price of peace that is a bonus for war, percent: the world forgives a war on an aggressor, it does not pay for it. | own value (source not recorded) | `infamy.rs` |

### `landless.rs`

5 constants (own design 1, own value (source not recorded) 4). Files: `classic/crates/classic-rules/src/ai/landless.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `TAKE_ROUNDS` | `5` | Rounds of siege within which a group takes a city to be a target. | own design | `landless.rs` |
| `STRENGTH_MARGIN_PERCENT` | `125` | The margin on the city's strength when the war is not on yet, percent: walls and the buildings are not seen, so the estimate is raised before the group commits to a war it cannot call off. | own value (source not recorded) | `landless.rs` |
| `LAST_UNIT_PRICE` | `24` | The price of the player's last unit in units of its own price: its death is elimination. | own value (source not recorded) | `landless.rs` |
| `MAJOR_WAR_PRICE` | `1` | A war on a nation, in units of a unit's price: the nation's army hunts the player's units. | own value (source not recorded) | `landless.rs` |
| `ASSUMED_POPULATION` | `2` |  | own value (source not recorded) | `landless.rs` |

### `layers.rs`

1 constants (own value (source not recorded) 1). Files: `classic/crates/classic-rules/src/ai/strategy_net/layers.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `ONE` | `1000` | 1.0 in thousandths. | own value (source not recorded) | `layers.rs` |

### `legal.rs`

1 constants (own value (source not recorded) 1). Files: `classic/crates/classic-rules/src/ai/legal.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `MAX_REPLANS` | `4` | Times a city plans again after its choice was refused. | own value (source not recorded) | `legal.rs` |

### `mind.rs`

10 constants (own value (source not recorded) 10). Files: `classic/crates/classic-rules/src/ai/net_e2e/mind.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `WEIGHTS_ENV` | `"AURELIA_NET_WEIGHTS"` | The environment variable naming the weights file of `Variant::Net` (`dummy`: the [`LinearScorer`]). | own value (source not recorded) | `mind.rs` |
| `DUMMY` | `"dummy"` | The value of [`WEIGHTS_ENV`] that plays the [`LinearScorer`]. | own value (source not recorded) | `mind.rs` |
| `SIMD_ENV` | `"AURELIA_NET_SIMD"` | The environment variables of the forward pass's speed ([`Speed`]; none changes a value): `AURELIA_NET_SIMD=0` the portable loops instead of the aarch64 kernels, `AURELIA_NET_THREADS=N` threads for a turn's encoding (1), `AURELIA_NET_INCREMENTAL=0` every tile state recomputed every turn. | own value (source not recorded) | `mind.rs` |
| `THREADS_ENV` | `"AURELIA_NET_THREADS"` |  | own value (source not recorded) | `mind.rs` |
| `INCREMENTAL_ENV` | `"AURELIA_NET_INCREMENTAL"` |  | own value (source not recorded) | `mind.rs` |
| `DEFER_ENV` | `"AURELIA_NET_DEFER"` | `AURELIA_NET_DEFER`: `off` the defer choice off whatever the weights file says (the A/B of a net with and without it); a number `B` (logits, may be negative or 0) the choice on with the bias `round(B × 256)` in place of the file's, also for a file without defer (whose tag row of the defer bucket is what its training left there). | own value (source not recorded) | `mind.rs` |
| `DAGGER_ENV` | `"AURELIA_NET_DAGGER"` | The environment variable that records the net's games for DAgger: a directory; the bench writes a shard per game there (`aurelia-qa`), its labels the scripted mind's choices at the states the net visited. | own value (source not recorded) | `mind.rs` |
| `HEADS_ENV` | `"AURELIA_NET_HEADS"` | The environment variable naming the decision heads the net plays (`AURELIA_NET_HEADS=unit,research`; the names of `encode::head::NAMES`; unset: every head the weights file has). | own value (source not recorded) | `mind.rs` |
| `SCRIPTED_ENV` | `"AURELIA_NET_SCRIPTED"` | The environment variable naming the unit roles the scripted mind keeps (`AURELIA_NET_SCRIPTED=founder,builder`; [`ScriptedRoles::parse`]). | own value (source not recorded) | `mind.rs` |
| `NAMES` | `["founder", "builder", "civilian", "naval", "military"]` |  | own value (source not recorded) | `mind.rs` |

### `mod.rs`

17 constants (own value (source not recorded) 17). Files: `classic/crates/classic-rules/src/ai/books/mod.rs`, `classic/crates/classic-rules/src/ai/net_e2e/v2/mod.rs`, `classic/crates/classic-rules/src/ai/unciv/mod.rs`, `classic/crates/classic-rules/src/ai/unit_net/mod.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `MUSTER_PATIENCE` | `8` | Rounds (Standard speed) a muster waits for its stragglers before the army marches with half of it. | own value (source not recorded) | `mod.rs` |
| `MUSTER_LIMIT` | `24` | Rounds (Standard speed) after which the army marches from wherever it stands: a muster point the walk at peace no longer reaches (borders grow) is reached at war. | own value (source not recorded) | `mod.rs` |
| `ATTACK_ROUNDS` | `75` | Rounds (Standard speed) the attack lasts from its launch before the book hands the war to `standard`. | own value (source not recorded) | `mod.rs` |
| `UNREACHABLE_TRIES` | `6` | Failed tries to find a land way to the target before the book gives the attack up (another landmass). | own value (source not recorded) | `mod.rs` |
| `ASSAULT_GROUP` | `3` | The book presses the siege once this many members stand within `ASSAULT_RADIUS` of the target. | own value (source not recorded) | `mod.rs` |
| `ASSAULT_RADIUS` | `6` |  | own value (source not recorded) | `mod.rs` |
| `FOE_RADIUS` | `1` | An enemy unit this close to a marching member hands it to the front. | own value (source not recorded) | `mod.rs` |
| `FIELD_HEAL_RADIUS` | `8` | A wounded member this close to the target heals there. | own value (source not recorded) | `mod.rs` |
| `MELEE_KEEPS_HP` | `35` | A melee member strikes a worn city only when it keeps this much health. | own value (source not recorded) | `mod.rs` |
| `MIN_ARMY` | `2` | The fewest units that form or re-form the attack's operation. | own value (source not recorded) | `mod.rs` |
| `MUSTER_REACH` | `14` | The farthest a muster point may lie from its target. | own value (source not recorded) | `mod.rs` |
| `WAYPOINT_RADIUS` | `8` | A long march's leg reaches this far; this many leg ends are tried. | own value (source not recorded) | `mod.rs` |
| `WAYPOINT_TRIES` | `4` |  | own value (source not recorded) | `mod.rs` |
| `PREFIX` | `"v2:"` | The prefix of a v2 weights spec (`v2:W.v2`). | own value (source not recorded) | `mod.rs` |
| `UNCIV_RADIUS` | `9` | The local observation's radius of an Unciv decision: Unciv's threat queries look 4-6 tiles around the tiles a unit reaches. | own value (source not recorded) | `mod.rs` |
| `CANDIDATE_LIMIT` | `24` | Candidate tiles a search tries before it gives up (each may cost a path search; Unciv walks lazy sequences with the same effect). | own value (source not recorded) | `mod.rs` |
| `UNIT` | `ONE` | Kept for `ONE`'s import in the docs. | own value (source not recorded) | `mod.rs` |

### `model.rs`

16 constants (own value (source not recorded) 16). Files: `classic/crates/classic-rules/src/ai/net_e2e/model.rs`, `classic/crates/classic-rules/src/ai/strategy_net/model.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `FORMAT_VERSION` | `5` | The newest weights file version this reader knows: 5 carries the defer choice (format 9: `Config::defer` and its logit bias); 4 carries the event table (format 7) and, events on, the event cross-attention; 3 carries the turn trace; a version-2, 3 or 4 file reads as before (no defer). | own value (source not recorded) | `model.rs` |
| `MIN_FORMAT_VERSION` | `2` | The oldest version it reads. | own value (source not recorded) | `model.rs` |
| `ACT_BITS` | `8` | Fraction bits of the activations (Q8). | own value (source not recorded) | `model.rs` |
| `INPUT_SHIFT` | `16` | Input normalisation: `x_q = round_shift(stored × mult, INPUT_SHIFT)`. | own value (source not recorded) | `model.rs` |
| `MAX_OUT` | `1024` | The widest dense layer output (stack buffers). | own value (source not recorded) | `model.rs` |
| `MAX_ROW` | `1024` | The longest input row of a table (stack buffers). | own value (source not recorded) | `model.rs` |
| `GATE_BITS` | `15` | GRU gate tables: values at 2^GATE_BITS over Q8 pre-activations. | own value (source not recorded) | `model.rs` |
| `UNIT_SLOT_KINDS` | `[0]` | Entity kinds with a unit slot (an own unit) and with a nation slot (a city-state, another nation). | own value (source not recorded) | `model.rs` |
| `NATION_SLOT_KINDS` | `[4, 5]` |  | own value (source not recorded) | `model.rs` |
| `HEAD_UNIT` | `0` | Decision heads, in the order of the data's head column (the file's config says how many; these are the ones with an actor's context). | own value (source not recorded) | `model.rs` |
| `HEAD_RESEARCH` | `1` |  | own value (source not recorded) | `model.rs` |
| `HEAD_PRODUCTION` | `2` |  | own value (source not recorded) | `model.rs` |
| `HEAD_POLICY` | `3` |  | own value (source not recorded) | `model.rs` |
| `MAX_DEFER_BIAS` | `1 << 24` | The largest \|defer bias\| a weights file may carry (Q8): the biased logit stays far inside i32. | own value (source not recorded) | `model.rs` |
| `PLAIN` | `Speed { simd: false, threads: 1, incremental: false }` | The portable loops on one thread, every tile recomputed: the reference the others are tested against. | own value (source not recorded) | `model.rs` |
| `MAX_CONDITIONS` | `256` | Static conditions the model keeps at most (bits of a tile). | own value (source not recorded) | `model.rs` |

### `net.rs`

2 constants (own value (source not recorded) 2). Files: `classic/crates/classic-rules/src/ai/net.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `ACT` | `16` | The fraction bits of the activations between the layers (Q16). | own value (source not recorded) | `net.rs` |
| `MAX_WIDTH` | `256` | The widest layer, inputs included, the inference takes (stack buffers). | own value (source not recorded) | `net.rs` |

### `operations.rs`

51 constants (own value (source not recorded) 51). Files: `classic/crates/classic-rules/src/ai/operations.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `MAX_ARMY` | `12` | Most units an operation plans for: a bigger need means the target is too strong. | own value (source not recorded) | `operations.rs` |
| `PLAN_SIEGE_ROUNDS` | `3` | Rounds of strikes in which the planned army takes the city — one less than the front's siege test (`front::SIEGE_ROUNDS`): room for the city's strikes and bad rolls. | own value (source not recorded) | `operations.rs` |
| `HEAVY_STRIKE_HP` | `34` | A melee unit that loses this much per strike on the city survives at most two: the army takes ranged units first. | own value (source not recorded) | `operations.rs` |
| `MAX_ESCORTS` | `3` | Most escorts added against the enemy's field army. | own value (source not recorded) | `operations.rs` |
| `MUSTER_RADIUS` | `2` | Members gather within this distance of the muster point. | own value (source not recorded) | `operations.rs` |
| `FORM_PATIENCE` | `20` | After this many rounds of forming, three quarters of the army marches. | own value (source not recorded) | `operations.rs` |
| `FORM_LIMIT` | `30` | A forming operation that has not marched after this many rounds ends. | own value (source not recorded) | `operations.rs` |
| `MARCH_LIMIT` | `20` | A marching operation that has not taken its target after this many rounds ends. | own value (source not recorded) | `operations.rs` |
| `EXTRA_MEMBERS` | `2` | Members beyond the need an operation takes while it forms. | own value (source not recorded) | `operations.rs` |
| `RECRUIT_RANGE` | `15` | Free units farther than this from the muster point are not recruited. | own value (source not recorded) | `operations.rs` |
| `COHESION` | `2` | A member this much closer to the target than the members' median waits. | own value (source not recorded) | `operations.rs` |
| `GATHER_ROUNDS` | `5` | Rounds to bring the recruited units to the muster point. | own value (source not recorded) | `operations.rs` |
| `SIEGE_ROUNDS_ESTIMATE` | `8` | Rounds of the siege itself, from the arrival. | own value (source not recorded) | `operations.rs` |
| `MAX_WAR_ORDERS` | `2` | Most units the cities build for one operation: the rest of its army must exist. | own value (source not recorded) | `operations.rs` |
| `WAR_BUILD_TURNS` | `10` | A unit an operation orders is built in at most this many turns. | own value (source not recorded) | `operations.rs` |
| `START_INTERVAL` | `5` | At peace, a player looks for a new operation every this many rounds. | own value (source not recorded) | `operations.rs` |
| `MAX_ROUTE_CHECKS` | `3` | Routes checked for new operations in one turn (best estimates first). | own value (source not recorded) | `operations.rs` |
| `WAR_CHEST_ROUNDS` | `40` | Rounds of the deficit of an operation's army the treasury must pay. | own value (source not recorded) | `operations.rs` |
| `DEFENCE_RADIUS` | `3` | Enemy units this close to the target defend it. | own value (source not recorded) | `operations.rs` |
| `BORDER_REACH` | `5` | A route reaching this close to the target over open land is a way to it (the rest is the enemy's land, entered at war). | own value (source not recorded) | `operations.rs` |
| `STAGING_DISTANCE` | `crate::combat::city::CITY_ATTACK_RANGE + 1` | The army stages this far from its target: out of the city's strike. | own value (source not recorded) | `operations.rs` |
| `NONE` | `WarTuning { march: false, unrest: false, group: false, raids: false, recall: false, sie...` |  | own value (source not recorded) | `operations.rs` |
| `NONE` | `SiegeFixes { assault: false, garrison: false, close: false, chance: false, recover: fal...` |  | own value (source not recorded) | `operations.rs` |
| `ALL` | `SiegeFixes { assault: true, garrison: true, close: true, chance: true, recover: true, l...` | The five fixes of the `siege` variant. | own value (source not recorded) | `operations.rs` |
| `WAR_FIX` | `SiegeFixes { assault: true, garrison: true, close: true, chance: false, recover: true,...` | The fixes of `war-fix` (classic/docs/ai-war-fix.md): the `siege` five with the learned chance in place of the siege model's, the capture priced by the rules, and a siege worth its city. | own value (source not recorded) | `operations.rs` |
| `OPEN` | `SiegeFixes { puppet: true, recovery: true, loss: true, yields: true, rank: true, ..Sieg...` | `war-open`'s (classic/docs/ai-war-open.md): `WAR_FIX` with the capture priced as the puppet the AI makes of it, its unrest for the rounds the empire takes to recover, its citizens yielding what the empire's do, and the army's loss as measured. | own value (source not recorded) | `operations.rs` |
| `PAYS` | `SiegeFixes { pays: true, ..SiegeFixes::OPEN }` | `war-pays`' (classic/docs/ai-war-pays.md): `OPEN` with the loss measured on its own operations; `rival_percent` gives it a flat `RIVAL_PERCENT` (`rank` bought twice the captures in the head to head and no share: tuning offsets 200–209, paired +0.00, CI −1.35…+1.35). | own value (source not recorded) | `operations.rs` |
| `READY_TURNS` | `2` | Turns the army may need to reach the ring after the declaration (`strike_ready`). | own value (source not recorded) | `operations.rs` |
| `DECISIVE_FORM_LIMIT` | `2 * FORM_LIMIT` | Rounds the army of a capture that wins the game may take to form. | own value (source not recorded) | `operations.rs` |
| `DECISIVE_RANGE` | `32` | The farthest a capture that wins the game may lie from our nearest city: rival capitals lie mostly 16–30 tiles from it on the duel suite, where `OBJECTIVE_RANGE` left three quarters of the known ones out of reach (classic/docs/ai-leaders.md "Capital search"); the route to the staging tile and the horizon still decide. | own value (source not recorded) | `operations.rs` |
| `CURRENT_PERCENT` | `67` | The share of the best trainable kind's strength a unit needs to fill a place of the army, percent: two thirds. | own value (source not recorded) | `operations.rs` |
| `PLAN_LIMIT` | `40` | A war plan waits at most this many rounds for its chest. | own value (source not recorded) | `operations.rs` |
| `PLAN_KEEP_PERCENT` | `70` | A plan is dropped when its target is worth less than this share of its value at the choice, percent. | own value (source not recorded) | `operations.rs` |
| `PLAN_SWITCH_PERCENT` | `130` | Another target replaces a plan only when worth this share of it. | own value (source not recorded) | `operations.rs` |
| `SUCCESS_PERCENT` | `35` | Chance an operation takes its city, percent (step 3: of the operations that marched with their army, about a third took the city or forced it). | own value (source not recorded) | `operations.rs` |
| `LOSS_PERCENT` | `50` | Share of the army an operation expects to lose, percent. | own value (source not recorded) | `operations.rs` |
| `OPEN_LOSS_PERCENT` | `66` | The same, measured on `war-open`'s own operations (`SiegeFixes::loss`): the units its members lost from the operation's start to its end over the units it planned — 66 % in 638 operations of `war-open` (pace, Quick, seed offset 201; 124 % of those that marched, whose every unit on an assault of the target joins them, 15 % of those recalled or timed out before; `ai-war-ops`, classic/docs/ai-war-open.md). | own value (source not recorded) | `operations.rs` |
| `PAYS_LOSS_PERCENT` | `67` | The same measured on `war-pays`'s own operations: 67.3 % of the need in 292 operations (pace, Quick, seeds 203001–204021 with `ai-war-ops`; 101 % of those that marched). | own value (source not recorded) | `operations.rs` |
| `RIVAL_PERCENT` | `50` | Share of the city's points taken from the rival that counts for us, percent: the time victory compares our score with the best other one. | own value (source not recorded) | `operations.rs` |
| `CHANCE_N` | `15` | The features of the learned capture chance (`CHANCE_FEATURES`). | own value (source not recorded) | `operations.rs` |
| `CHANCE_FEATURES` | `[ "power_share", "siege_model", "distance", "population", "guard_share", "at_war", "rou...` | What an operation's outcome is learned from, all known to the player when it plans (classic/docs/ai-war-fix.md "The capture chance", classic/docs/ai-war-open.md): integers, in this order; the last three are `war-open`'s (the game's AI weighs them 0). | own value (source not recorded) | `operations.rs` |
| `CHANCE_INTERCEPT` | `-1657` | The learned capture chance's intercept and weights (logit in thousandths; a weight in millionths of the logit per unit of its feature), fitted by a logistic regression on the operations of bench games whose seeds the reported benches do not use (`ai-war-ops`, `tools/war_chance_fit.py`, classic/docs/ai-war-fix.md). | own value (source not recorded) | `operations.rs` |
| `CHANCE_WEIGHTS` | `[-6986, 0, -61418, -53107, 1909, -990031, 0, 0, 0, -36668, 204117, 18862, 0, 0, 0]` |  | own value (source not recorded) | `operations.rs` |
| `OPEN_CHANCE_INTERCEPT` | `-1193` | The same model refitted on the operations of `war-open` (`TurnBrief::war_open`): its army is prepared by the posture and its wars open on an opportunity, so its operations are not the ones `CHANCE_WEIGHTS` were fitted on; the fit may use the three features the game's AI weighs 0. | own value (source not recorded) | `operations.rs` |
| `OPEN_CHANCE_WEIGHTS` | `[-9830, 0, -66899, -65621, -8240, -689142, 0, 0, 0, -19137, 235035, -35079, -2122, 2780...` |  | own value (source not recorded) | `operations.rs` |
| `ARMY_PER_CITY_TENTHS` | `26` | Army units a nation of these rules keeps a city, tenths: the AI's own army at a look for an operation is 14.35 units for 5.5 cities (classic/docs/ai-war-audit-current.md, `pace`). | own value (source not recorded) | `operations.rs` |
| `MAX_TARGET_POPULATION` | `8` | The largest target `war-pays` starts an operation against, in citizens: a city of 9 or more held 8 % of the cases against 22–29 % for the smaller ones (it heals behind more walls and keeps a larger garrison). | own value (source not recorded) | `operations.rs` |
| `STRIKE_SAFE_RADIUS` | `6` | Foreign military units this close to a city make its garrison needed. | own value (source not recorded) | `operations.rs` |
| `STRIKE_LEND_ONE_IN` | `3` | The share of the cities that lend their garrison to a march: one city in this many. | own value (source not recorded) | `operations.rs` |
| `JUMP_MIN_DISTANCE` | `3` | A paratrooper jumps when the way to the muster point is at least this long (a shorter one is walked in a round or two). | own value (source not recorded) | `operations.rs` |
| `JUMP_CANDIDATES` | `7` | Tiles near the muster point one jump tries. | own value (source not recorded) | `operations.rs` |

### `pantheon.rs`

1 constants (own value (source not recorded) 1). Files: `classic/crates/classic-rules/src/ai/pantheon.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `LAND_RADIUS` | `3` | Hex radius around a city whose tiles it can work. | own value (source not recorded) | `pantheon.rs` |

### `parts.rs`

2 constants (own value (source not recorded) 2). Files: `classic/crates/classic-rules/src/ai/unciv/parts.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `DRY_ENV` | `"AURELIA_UNCIV_DRY"` | Dev-only: a hybrid whose families decide but whose decisions are dropped (`standard` plays everything): the proof of the plumbing by result hash. | own value (source not recorded) | `parts.rs` |
| `NONE` | `UncivParts { research: false, policies: false, settle: false, construction: false, work...` |  | own value (source not recorded) | `parts.rs` |

### `personality.rs`

3 constants (own value (source not recorded) 3). Files: `classic/crates/classic-rules/src/ai/unciv/personality.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `BRANCHES` | `["Tradition", "Liberty", "Honor", "Commerce", "Patronage", "Piety", "Rationalism", "Fre...` | The policy branches Unciv's priorities name, in the order of `UncivPersonality::priorities`. | own value (source not recorded) | `personality.rs` |
| `TEMPLATE` | `UncivPersonality { leader: "Template", victory: Victory::Neutral, values: [5; 15], prio...` | Unciv's template: every value neutral. | own value (source not recorded) | `personality.rs` |
| `LEADERS` | `&[ UncivPersonality { leader: "Ahmad al-Mansur", victory: Victory::Neutral, values: [6,...` | Unciv's leaders (Personalities.json), generated. | own value (source not recorded) | `personality.rs` |

### `plan.rs`

1 constants (own value (unverified) 1). Files: `platform/crates/kernel/src/ai/plan.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `WITHOUT_HANDICAPS` | `UnitSupply { free_units: 7, per_city: 3, population_percent: 50, ai_percent: 20, }` | Content without a difficulty table (old test fixtures): the AI row's supply at the default level. | own value (unverified) | `plan.rs` |

### `plan_search.rs`

6 constants (own value (source not recorded) 6). Files: `classic/crates/classic-rules/src/ai/plan_search.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `BEAM_WIDTH` | `8` | States the beam keeps after each member. | own value (source not recorded) | `plan_search.rs` |
| `STRIKE_TILES` | `2` | Strike options a member keeps per target. | own value (source not recorded) | `plan_search.rs` |
| `MAX_STRIKES` | `5` | Strike options a member keeps in all. | own value (source not recorded) | `plan_search.rs` |
| `MAX_GOES` | `4` | Positions (go or withdraw) a member keeps. | own value (source not recorded) | `plan_search.rs` |
| `SWEEPS` | `2` | Sweeps of one-member improvements after the beam. | own value (source not recorded) | `plan_search.rs` |
| `LETHAL_PERMILLE` | `360` | With the danger net, a member whose chance of death at its final tile is this or more (permille) falls under the lethal rule: the threshold of the net's best F1 on the validation rows (`ai-train-danger`'s report, 0.36). | own value (source not recorded) | `plan_search.rs` |

### `planner.rs`

3 constants (own value (source not recorded) 3). Files: `classic/crates/classic-rules/src/ai/planner.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `NONE` | `CallFixes { ready: false, ranged: false, melee: false, call: false, walls: false }` |  | own value (source not recorded) | `planner.rs` |
| `ALL` | `CallFixes { ready: true, ranged: true, melee: true, call: true, walls: true }` |  | own value (source not recorded) | `planner.rs` |
| `DEFAULT` | `CallFixes { ready: false, ranged: true, melee: false, call: true, walls: false }` | What `siege-call` plays: the fixes that measured better on the tuning offsets 200-202 (300 solo pace games, `runs/ops`): ranged units first and the press call. | own value (source not recorded) | `planner.rs` |

### `planner_cities.rs`

9 constants (own value (source not recorded) 9). Files: `classic/crates/classic-rules/src/ai/planner_cities.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `SLOW_ORDER_TURNS` | `40` | An order taking longer than this many turns is not chosen while something faster is wanted (production turns at Standard speed; × the speed's pace, as its costs). | own value (source not recorded) | `planner_cities.rs` |
| `SETTLER_MIN_POPULATION` | `2` | Settlers come from cities of at least this population. | own value (source not recorded) | `planner_cities.rs` |
| `HURRY_RESERVE` | `30` | Gold kept in the treasury before hurrying. | own value (source not recorded) | `planner_cities.rs` |
| `DEFICIT_RESERVE_TURNS` | `30` | Turns of gold deficit the treasury keeps in reserve before buying. | own value (source not recorded) | `planner_cities.rs` |
| `NONE` | `PricedStops { upkeep: false, reserve: false, missionaries: false }` | The stops of the game's AI. | own value (source not recorded) | `planner_cities.rs` |
| `ALL` | `PricedStops { upkeep: true, reserve: true, missionaries: true }` | Every stop priced. | own value (source not recorded) | `planner_cities.rs` |
| `QUEUE_TARGET` | `1` | Orders a city keeps queued. | own value (source not recorded) | `planner_cities.rs` |
| `SETTLER_AHEAD_CITIES` | `2` | While the empire has fewer cities than this (only the second city), a city that can train a settler puts it ahead of a building in its queue: the stock the building gathered (a Granary of the size-1 capital) goes into the settler. | own value (source not recorded) | `planner_cities.rs` |
| `SETTLER_AHEAD_TURNS` | `20` | A settler taking longer than this is not put ahead (Standard speed; × the speed's pace). | own value (source not recorded) | `planner_cities.rs` |

### `planner_research.rs`

1 constants (own value (source not recorded) 1). Files: `classic/crates/classic-rules/src/ai/planner_research.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `SEAFARING_KNOWN_PERCENT` | `50` | The map beyond the sea is mostly unknown below this share: the technologies that let units embark and cross the ocean are worth more (the other nations, city-states and land to settle are there). | own value (source not recorded) | `planner_research.rs` |

### `planner_settle.rs`

12 constants (own value (source not recorded) 12). Files: `classic/crates/classic-rules/src/ai/planner_settle.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `SITE_MAX_TURNS` | `12` | Sites farther than this many turns from the settler are not considered. | own value (source not recorded) | `planner_settle.rs` |
| `TRAVEL_COST` | `30` | Score points one turn of the settler's walk costs. | own value (source not recorded) | `planner_settle.rs` |
| `FREE_DISTANCE` | `4` | Tiles from the nearest own city a site is free of the distance penalty. | own value (source not recorded) | `planner_settle.rs` |
| `DISTANCE_PENALTY` | `40` | Score points a tile beyond `FREE_DISTANCE` costs. | own value (source not recorded) | `planner_settle.rs` |
| `SITE_MIN_SCORE` | `20` | A site below this score is not worth a city. | own value (source not recorded) | `planner_settle.rs` |
| `ESCORT_WAIT_ROUNDS` | `10` | Rounds a settler that met danger waits for an escort before it gives its site up. | own value (source not recorded) | `planner_settle.rs` |
| `DANGER_RANGE` | `3` | No site this close to a hostile military unit in sight. | own value (source not recorded) | `planner_settle.rs` |
| `RIVAL_SAFE_DISTANCE` | `7` | Tiles from the nearest rival city a site on the net's path is free of `rival_risk`. | own value (source not recorded) | `planner_settle.rs` |
| `RIVAL_PENALTY` | `30` | Score points a tile closer than `RIVAL_SAFE_DISTANCE` costs: at 4 tiles 3 × 30 = 90, three turns of travel, about what the land score takes for 15 rival tiles in the radius. | own value (source not recorded) | `planner_settle.rs` |
| `ARMY_RANGE` | `6` | Hostile military units in sight within this many tiles of a site weigh on it. | own value (source not recorded) | `planner_settle.rs` |
| `ARMY_PENALTY` | `60` | Score points each such unit costs: two turns of travel. | own value (source not recorded) | `planner_settle.rs` |
| `NET_TRAVEL_RISK` | `10` | Score points each turn of a settler's walk costs on the site net's path on top of `TRAVEL_COST`: the net's sites lie farther out and 11.5 % of its settlers never founded a city (5.1 % on the land score); a turn on the road is a turn an unescorted settler can be caught. | own value (source not recorded) | `planner_settle.rs` |

### `planner_strategy.rs`

13 constants (own value (source not recorded) 13). Files: `classic/crates/classic-rules/src/ai/planner_strategy.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `MIN_WAR_ROUNDS` | `15` | A war lasts at least this many rounds. | own value (source not recorded) | `planner_strategy.rs` |
| `PEACE_COOLDOWN_ROUNDS` | `30` | After a peace, no new war with the same player for this many rounds. | own value (source not recorded) | `planner_strategy.rs` |
| `WAR_WEARINESS_ROUNDS` | `60` | A war without success ends after this many rounds. | own value (source not recorded) | `planner_strategy.rs` |
| `EARLIEST_WAR_ROUND` | `40` | No war before this round (the empire first grows). | own value (source not recorded) | `planner_strategy.rs` |
| `THREAT_RADIUS` | `5` | Foreign military power within this distance of our cities is a threat. | own value (source not recorded) | `planner_strategy.rs` |
| `THREAT_MEMORY_ROUNDS` | `10` | A threat not seen again for this many rounds is forgotten. | own value (source not recorded) | `planner_strategy.rs` |
| `OBJECTIVE_RANGE` | `14` | A war objective is a known city at most this far from our cities. | own value (source not recorded) | `planner_strategy.rs` |
| `BORDERS_AFTER_PEACE` | `20` | Rounds of peace before the AI opens its borders unasked. | own value (source not recorded) | `planner_strategy.rs` |
| `NEW_CITY_UNHAPPINESS` | `4` | Unhappiness a new city brings (3 for the city, 1 for its citizen): the empire expands while it stays happy after one more city (expansion follows happiness, instead of a fixed number of cities). | own value (source not recorded) | `planner_strategy.rs` |
| `FIRST_CITIES` | `2` | Cities an empire founds whatever its happiness. | own value (source not recorded) | `planner_strategy.rs` |
| `NO_SITE_PAUSE_ROUNDS` | `20` | Rounds without expansion after a settler found no site. | own value (source not recorded) | `planner_strategy.rs` |
| `UNSEEN_CITY_POPULATION` | `4` | Population assumed for an enemy city not seen yet. | own value (source not recorded) | `planner_strategy.rs` |
| `PREPARE_ROUNDS` | `10` | Rounds before `EARLIEST_WAR_ROUND` an AI starts to arm for a war. | own value (source not recorded) | `planner_strategy.rs` |

### `planner_units.rs`

4 constants (own value (source not recorded) 4). Files: `classic/crates/classic-rules/src/ai/planner_units.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `SCOUTING_ENDS_ROUND` | `150` | After this round scouting stops (the known world is what matters) (a round of a Standard game: another speed reads its round as the same share of its game, `SpeedValues::standard_round`). | own value (source not recorded) | `planner_units.rs` |
| `GARRISON_FROM_ROUND` | `10` | From this round every city wants a garrison even in peace (a round of a Standard game: another speed reads its round as the same share of its game, `SpeedValues::standard_round`). | own value (source not recorded) | `planner_units.rs` |
| `GARRISON_ALONE_ROUND` | `40` | From this round every city wants a garrison, even with nobody met (a round of a Standard game: another speed reads its round as the same share of its game, `SpeedValues::standard_round`). | own value (source not recorded) | `planner_units.rs` |
| `HEAL_BELOW_HP` | `50` | Health below which a unit goes to heal. | own value (source not recorded) | `planner_units.rs` |

### `planner_value.rs`

3 constants (own value (source not recorded) 3). Files: `classic/crates/classic-rules/src/ai/planner_value.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `DEFICIT_GOLD_WEIGHT` | `4` | Added to the gold weight at a deficit. | own value (source not recorded) | `planner_value.rs` |
| `UNHAPPY_HAPPINESS_WEIGHT` | `6` | Added to the happiness weight while the empire is unhappy. | own value (source not recorded) | `planner_value.rs` |
| `STRATEGIC_RESOURCE_WEIGHT` | `3` | Yield-equivalents of connecting a strategic resource, per military weight. | own value (source not recorded) | `planner_value.rs` |

### `planner_workers.rs`

6 constants (own value (source not recorded) 6). Files: `classic/crates/classic-rules/src/ai/planner_workers.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `REPLACE_GAIN` | `20` | A new improvement replaces an existing one only for this much more. | own value (source not recorded) | `planner_workers.rs` |
| `ROADS_FROM_ROUND` | `40` | Roads are built from this round on, with this much gold income (a round of a Standard game: another speed reads its round as the same share of its game, `SpeedValues::standard_round`). | own value (source not recorded) | `planner_workers.rs` |
| `ROADS_MIN_INCOME` | `3` |  | own value (source not recorded) | `planner_workers.rs` |
| `ROAD_CITY_RANGE` | `7` | Cities this close are joined by roads. | own value (source not recorded) | `planner_workers.rs` |
| `NONE` | `WorkersFit { demand: false, park: false, flee: false, repair: false }` |  | own value (source not recorded) | `planner_workers.rs` |
| `ALL` | `WorkersFit { demand: true, park: true, flee: true, repair: true }` |  | own value (source not recorded) | `planner_workers.rs` |

### `plans.rs`

9 constants (own value (source not recorded) 9). Files: `classic/crates/classic-rules/src/ai/books/plans.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `SIEGE_FORCE` | `55` |  | own value (source not recorded) | `plans.rs` |
| `SIEGE_LAST` | `95` |  | own value (source not recorded) | `plans.rs` |
| `ARCHER_FORCE` | `55` |  | own value (source not recorded) | `plans.rs` |
| `ARCHER_LAST` | `95` |  | own value (source not recorded) | `plans.rs` |
| `SWORD_FORCE` | `60` |  | own value (source not recorded) | `plans.rs` |
| `SWORD_LAST` | `100` |  | own value (source not recorded) | `plans.rs` |
| `UU_FORCE` | `130` |  | own value (source not recorded) | `plans.rs` |
| `UU_LAST` | `175` |  | own value (source not recorded) | `plans.rs` |
| `REX_LAST` | `130` |  | own value (source not recorded) | `plans.rs` |

### `policies.rs`

1 constants (own value (source not recorded) 1). Files: `classic/crates/classic-rules/src/ai/policies.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `WIDE_EMPIRE_CITIES` | `4` | Cities from which the AI plays a wide empire (the ideology it picks). | own value (source not recorded) | `policies.rs` |

### `policy_branches.rs`

7 constants (own value (source not recorded) 7). Files: `classic/crates/classic-rules/src/ai/policy_branches.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `ROOM_RADIUS` | `7` | Tiles around the capital in which room for cities is counted. | own value (source not recorded) | `policy_branches.rs` |
| `SITE_SPACING` | `4` | Tiles between two cities that count as two sites. | own value (source not recorded) | `policy_branches.rs` |
| `FEEDING_SITES` | `4` | Sites whose food counts as "the land" (the best ones). | own value (source not recorded) | `policy_branches.rs` |
| `EXPECTED_CITIES_HORIZON` | `3` | Cities beyond those it has the empire is expected to found at most. | own value (source not recorded) | `policy_branches.rs` |
| `RIVAL_FAR` | `16` | Distance to a known rival (tiles) from which it takes no room away. | own value (source not recorded) | `policy_branches.rs` |
| `RIVAL_NEAR` | `10` | Distance to a known rival (tiles) at which it is a threat. | own value (source not recorded) | `policy_branches.rs` |
| `AIMS` | `11` | Number of aims. | own value (source not recorded) | `policy_branches.rs` |

### `raid.rs`

23 constants (own design 7, own value (source not recorded) 16). Files: `classic/crates/classic-rules/src/ai/raid.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `RAID_SIZE` | `3` | Units a raid takes at most, and the fewest it forms with. | own design | `raid.rs` |
| `RAID_MIN` | `2` |  | own value (source not recorded) | `raid.rs` |
| `RAID_ROUNDS` | `30` | A raid ends this many rounds after it formed (the time a pillage trip takes and the enemy needs to answer). | own design | `raid.rs` |
| `FORM_ROUNDS` | `10` | A raid that has not set out (the war not declared) after this many rounds ends. | own value (source not recorded) | `raid.rs` |
| `GATHER_RADIUS` | `3` | Raiders within this distance of the muster city are gathered. | own value (source not recorded) | `raid.rs` |
| `REGION_RADIUS` | `4` | Tiles around the target city a raid pillages in (the city's own work radius is 3; the ring beyond holds the roads and the farms of the next city). | own value (source not recorded) | `raid.rs` |
| `MIN_PREY_RING` | `2` | The nearest ring of tiles around a city a raid pillages in (the first is the garrison's). | own value (source not recorded) | `raid.rs` |
| `STAGING_RING` | `3` | The ring around the target city the raid walks to when the land beyond is unexplored. | own value (source not recorded) | `raid.rs` |
| `BOLD_HP` | `80` | A raider at least this healthy pillages a tile the city can strike (ring `MIN_PREY_RING` up to `CITY_ATTACK_RANGE`; pillaging heals it). | own value (source not recorded) | `raid.rs` |
| `DECLARE_FACTOR` | `2` | A declaration of war for a raid needs a gain this many times its risk and the price of the declaration: the war costs more than its group (the answer of the enemy, the trade and the friendships it ends). | own design | `raid.rs` |
| `MAX_ARRIVAL_ROUNDS` | `10` | Rounds a raid may take to arrive (the gathering, the walk, the last stretch) and routes checked for the best cases of a turn. | own design | `raid.rs` |
| `MAX_DECLARED_ARRIVAL_ROUNDS` | `5` | The same for a raid that must first declare its war: the enemy may buy peace from `diplomacy::ACCEPT_PEACE_AFTER_ROUNDS` on, so the raid must be there by then. | own value (source not recorded) | `raid.rs` |
| `MAX_WAY_CHECKS` | `3` |  | own value (source not recorded) | `raid.rs` |
| `PILLAGES_PER_RAIDER` | `4` | Improvements one raider pillages in a raid (the rest of its time is the way and the retreat). | own design | `raid.rs` |
| `REPAIR_ROUNDS` | `6` | Rounds an enemy worker takes to bring a pillaged improvement back, beyond its building time (the walk and the wait). | own design | `raid.rs` |
| `RETREAT_PERCENT` | `120` | Enemy power within `DANGER_RADIUS` of a raider above this share of the group's power sends it home. | own value (source not recorded) | `raid.rs` |
| `DANGER_RADIUS` | `3` |  | own value (source not recorded) | `raid.rs` |
| `RETREAT_HP` | `55` | A raider below this health goes home. | own value (source not recorded) | `raid.rs` |
| `MAX_PACTS` | `1` | The fewest nations a raid's enemy may be allied with (pacts we know of). | own value (source not recorded) | `raid.rs` |
| `PREY_RADIUS` | `4` | A raider within this distance of the prey it can kill (a civilian) goes for it. | own value (source not recorded) | `raid.rs` |
| `LOSS_MIN_PERCENT` | `10` | Share of the group's price the raid expects to lose at the least, and the share added when the enemy's power near the target equals the group's: the risk of the raid. | own design | `raid.rs` |
| `LOSS_RISK_PERCENT` | `50` |  | own value (source not recorded) | `raid.rs` |
| `WARMONGER_ERA_PERCENT` | `60` | Mid-game percent of the warmonger weight of a declaration of war (the rules scale it by the era, `systems::diplomacy`), for the price of a declaration at peace. | own value (source not recorded) | `raid.rs` |

### `raid_war.rs`

4 constants (own design 1, own value (source not recorded) 3). Files: `classic/crates/classic-rules/src/ai/raid_war.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `NEAR_TURNS` | `4` | A hostile fighter within this many of its turns of walking and striking is "near" a unit holding ground. | own value (source not recorded) | `raid_war.rs` |
| `SORTIE_ROUNDS` | `5` | Rounds of its walking a free unit looks ahead for prey. | own design | `raid_war.rs` |
| `WARD_DANGER_RADIUS` | `3` | A ward (a civilian or a ranged unit of ours beside the unit) is left exposed when a hostile fighter is this close to it: the distance at which the empire already sends an escort with a settler (`planner_units`). | own value (source not recorded) | `raid_war.rs` |
| `CITY_RING` | `3` | Tiles around a city that are its own ground (the city's work radius): a unit farther from the city it is to garrison is not guarding it now. | own value (source not recorded) | `raid_war.rs` |

### `religion.rs`

1 constants (own value (source not recorded) 1). Files: `classic/crates/classic-rules/src/ai/religion.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `MISSIONARIES_WANTED` | `2` | Missionaries an AI keeps out at most. | own value (source not recorded) | `religion.rs` |

### `remote.rs`

1 constants (own value (source not recorded) 1). Files: `classic/crates/classic-rules/src/ai/net_e2e/remote.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `PREFIX` | `"remote:"` | The prefix of a remote weights spec (`remote:SOCKET`). | own value (source not recorded) | `remote.rs` |

### `retake.rs`

1 constants (own value (source not recorded) 1). Files: `classic/crates/classic-rules/src/ai/retake.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `RETAKE_ROUNDS` | `2` | Rounds of strikes the units on the spot may take to bring the city to capture: this turn's and the next. | own value (source not recorded) | `retake.rs` |

### `rival_model.rs`

4 constants (own value (source not recorded) 4). Files: `classic/crates/classic-rules/src/ai/rival_model.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `MEASURES` | `Measure::ALL.len()` | The demographics rows, in `Measure::ALL`'s order. | own value (source not recorded) | `rival_model.rs` |
| `SAMPLE_ROUNDS` | `5` | A sample of the book is taken every this many Standard rounds (× the speed's pace). | own value (source not recorded) | `rival_model.rs` |
| `TREND_ROUNDS` | `30` | The trends (science, score) are read over this many Standard rounds (× the pace): six technologies of the early game, two or three of the late. | own value (source not recorded) | `rival_model.rs` |
| `TREND_MIN_ROUNDS` | `15` | A trend needs a sample at least this many Standard rounds old (× the pace). | own value (source not recorded) | `rival_model.rs` |

### `roles.rs`

2 constants (own design 1, own value (source not recorded) 1). Files: `classic/crates/classic-rules/src/ai/roles.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `SCREEN_BELOW_PERCENT` | `75` | A melee unit with less than this share (percent) of the best melee strength of the group is a screen. | own value (source not recorded) | `roles.rs` |
| `WITHDRAW_BELOW` | `35` | Health below which a melee unit leaves the line (35), and 5 more for each level it has gained (up to 4), so that veterans are preserved. | own design | `roles.rs` |

### `sample.rs`

1 constants (own value (source not recorded) 1). Files: `classic/crates/classic-rules/src/ai/net_e2e/sample.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `UNSTAMPED` | `u32::MAX` | Not stamped yet ([`Trace::stamp`]). | own value (source not recorded) | `sample.rs` |

### `science_levers.rs`

2 constants (own value (source not recorded) 2). Files: `classic/crates/classic-rules/src/ai/science_levers.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `NONE` | `ScienceLevers { policies: false, specialists: false, agreements: false }` |  | own value (source not recorded) | `science_levers.rs` |
| `ALL` | `ScienceLevers { policies: true, specialists: true, agreements: true }` |  | own value (source not recorded) | `science_levers.rs` |

### `scouting.rs`

19 constants (own value (source not recorded) 19). Files: `classic/crates/classic-rules/src/ai/scouting.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `REVEAL_SCORE` | `10` | Score of an unknown tile a frontier tile would reveal. | own value (source not recorded) | `scouting.rs` |
| `TURN_COST` | `4` | Score lost per turn on the way. | own value (source not recorded) | `scouting.rs` |
| `RUIN_SCORE` | `120` | What known ruins are worth, in the score of a frontier tile: about twelve unknown tiles (the reward is a citizen, culture, faith, gold or a map). | own value (source not recorded) | `scouting.rs` |
| `TARGET_ZONE` | `8` | A candidate this close to the target of another scout loses `TARGET_CROWDING` per tile of the missing distance to `TARGET_ZONE`; the same for the tile where another scout stands. | own value (source not recorded) | `scouting.rs` |
| `TARGET_CROWDING` | `12` |  | own value (source not recorded) | `scouting.rs` |
| `SCOUT_ZONE` | `5` |  | own value (source not recorded) | `scouting.rs` |
| `SCOUT_CROWDING` | `10` |  | own value (source not recorded) | `scouting.rs` |
| `DANGER_RADIUS` | `2` | A candidate this close to a hostile unit or a barbarian camp is no target. | own value (source not recorded) | `scouting.rs` |
| `ALARM_RADIUS` | `3` | A scout that sees a hostile military unit this close aims only at tiles farther from it than it stands. | own value (source not recorded) | `scouting.rs` |
| `MAX_CANDIDATES` | `64` | Candidates one decision looks at (the decision budget caps it lower). | own value (source not recorded) | `scouting.rs` |
| `MAX_RIVALS` | `6` | Other scouts one decision takes into account. | own value (source not recorded) | `scouting.rs` |
| `EXPLORED_ENOUGH_PERCENT` | `90` | The share of the map explored (percent) from which the empire wants no more scouts: what is left lies over the sea, or in the fog of others. | own value (source not recorded) | `scouting.rs` |
| `SECOND_SCOUT_ROUND` | `12` | The second scout is wanted from this round while less is explored (a round of a Standard game: another speed reads its round as the same share of its game, `SpeedValues::standard_round`). | own value (source not recorded) | `scouting.rs` |
| `SECOND_SCOUT_UNTIL_ROUND` | `100` |  | own value (source not recorded) | `scouting.rs` |
| `SECOND_SCOUT_BELOW_PERCENT` | `40` |  | own value (source not recorded) | `scouting.rs` |
| `RECON_FROM_ROUND` | `30` | The first recon unit is built from this round (before it the starting warrior explores and the capital builds workers and settlers: a scout at round 12 cost the empires a tenth of their first workers and a third of their luxury kinds at round 60, `aurelia-qa` `happiness_pace`). | own value (source not recorded) | `scouting.rs` |
| `RECON_UNITS` | `1` | Recon units the empire builds for scouting. | own value (source not recorded) | `scouting.rs` |
| `SCOUT_SHARE_PERCENT` | `33` | `scout-early` (`Variant::scout_early`): the share of the ruins near home a recon unit claims that the starting warrior would not (measured, classic/docs/ai-scouting.md «The opening»: the books, whose scout comes at round 3, claim 1.11 ruins by round 30 of a Quick game where `standard`'s warrior alone claims 0.76, about a third more). | own value (source not recorded) | `scouting.rs` |
| `RUINS_RACE_ROUNDS` | `10` | `scout-early`: the Standard rounds the ruins near home stay unclaimed (the race with the neighbours' units); the scout's reach is its moves over them. | own value (source not recorded) | `scouting.rs` |

### `sea_work.rs`

1 constants (own value (source not recorded) 1). Files: `classic/crates/classic-rules/src/ai/sea_work.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `SEA_JOB_ROUNDS` | `4` | Rounds a work boat takes to reach its deposit and finish the job. | own value (source not recorded) | `sea_work.rs` |

### `sharp.rs`

16 constants (own value (source not recorded) 16). Files: `classic/crates/classic-rules/src/ai/strategy_net/sharp.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `STYLE_TEMPERATURE` | `600` | The temperatures of the intent's softmaxes: the logits are divided by them (1000 is `standard`'s softmax). | own value (source not recorded) | `sharp.rs` |
| `AIM_TEMPERATURE` | `600` |  | own value (source not recorded) | `sharp.rs` |
| `POSTURE_TEMPERATURE` | `1000` | The posture's logits are already apart by thousands (a war is +3000): its own scale. | own value (source not recorded) | `sharp.rs` |
| `PRICE_PICK` | `500` | The share of each table of the price layer's rows the held choice takes (its one-hot), the rest the shares: at `standard`'s blend a science aim at 52 % priced science as a half-hearted one (1.15–1.34 for every seat). | own value (source not recorded) | `sharp.rs` |
| `SCIENCE_ROUNDS` | `2` | A technology of our era in this many rounds is the science feature's 1 (the era's mean cost at the game's speed over our beakers): it moves with the era instead of saturating at 1.5 beakers a citizen by round 66. | own value (source not recorded) | `sharp.rs` |
| `PRODUCTION_ROUNDS` | `4` | A regular building of our era in this many rounds a city is the production feature's 1. | own value (source not recorded) | `sharp.rs` |
| `CITIES_RATIO` | `4000` | The cities feature: ours over the rivals' known cities a nation, this ratio → 1 (level 0.25): the rivals' cities are known through the fog only, so ours were twice theirs in 44 % of the reviews at a ratio of 2 (tuning traces, step 4). | own value (source not recorded) | `sharp.rs` |
| `PARTS_SHARE` | `200` | The share of the parts in the way to the spaceship (the technologies take the rest). | own value (source not recorded) | `sharp.rs` |
| `TREND_SCALE` | `5000` | The score's trend: a change of our score over the best rival's by 1 (thousandths) moves the feature this much: 0.1 of the ratio is the half range. | own value (source not recorded) | `sharp.rs` |
| `RUNAWAY_SCALE` | `2000` | A rival running away: its lead over the next best nation × this (50 % → 1). | own value (source not recorded) | `sharp.rs` |
| `CONCEPTS` | `&[ (SCIENCE_LEAD, f(SCIENCE, 300, "the era's technologies at our beakers (the era-relat...` | Terms added to the concepts before their activation: (concept, term). | own value (source not recorded) | `sharp.rs` |
| `STYLE` | `&[ (0, f(T_EXPANSION, -1500, "a leader who likes new cities does not stay small")), (0,...` | Terms added to the style logits: (style, term). | own value (source not recorded) | `sharp.rs` |
| `TALL_BIAS` | `400` | The bias added to tall: the terms above sum to about −50 for a neutral leader on middling land; with this bias a third of the traced seats play tall (the replica on the step-4 traces: korea, india, germany, venice, the netherlands, sweden most), the expanders wide. | own value (source not recorded) | `sharp.rs` |
| `AIM` | `&[ (0, f(SPACE_OWN, 600, "the spaceship within reach: the science victory is ours to fi...` | Terms added to the aim logits: (aim, term). | own value (source not recorded) | `sharp.rs` |
| `HORIZON` | `&[(SPACE_RIVAL, -30, "a rival near the spaceship may end the game: plans pay sooner")]` | Terms of the horizon (Standard rounds per unit of the feature). | own value (source not recorded) | `sharp.rs` |
| `STYLE_EVENTS` | `[SPACE_OWN, SPACE_RIVAL, SCORE_TREND, RUNAWAY]` | The features a locked style takes from the turn besides `weights::STYLE_EVENTS`: the memory keeps the lock's first `fe::BASE_N` features only, and these change with the race, not with the empire's growth. | own value (source not recorded) | `sharp.rs` |

### `siege_tactics.rs`

2 constants (own value (source not recorded) 2). Files: `classic/crates/classic-rules/src/ai/siege_tactics.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `NONE` | `SiegeTactics { ground: false, firing: false, outrange: false, fortify: false }` |  | own value (source not recorded) | `siege_tactics.rs` |
| `ALL` | `SiegeTactics { ground: true, firing: true, outrange: true, fortify: true }` |  | own value (source not recorded) | `siege_tactics.rs` |

### `siege_walls.rs`

15 constants (own value (source not recorded) 15). Files: `classic/crates/classic-rules/src/ai/siege_walls.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `NONE` | `SiegeJoin { join: false, close: false, sight: false }` |  | own value (source not recorded) | `siege_walls.rs` |
| `ALL` | `SiegeJoin { join: true, close: true, sight: true }` |  | own value (source not recorded) | `siege_walls.rs` |
| `GROUP_MARGIN` | `3` | Members this many tiles beyond the ring make the city's siege group; the others are still on their way (they keep marching, `books::press`). | own value (source not recorded) | `siege_walls.rs` |
| `STRIKE_WINDOW` | `8` | Rounds a strike is valued over: a siege unit that sets up fires in the second round, a city that survives a volley is struck again, a group whose volleys beat the healing wears the city down over several rounds (its losses counted every round). | own value (source not recorded) | `siege_walls.rs` |
| `MAX_WAIT` | `8` | The longest wait on the ring that is weighed against striking now. | own value (source not recorded) | `siege_walls.rs` |
| `TIME_ROUNDS` | `30` | The capture loses its whole value over this many Standard rounds of delay (the speed scales it): the time a capture saves. | own value (source not recorded) | `siege_walls.rs` |
| `CLEAR_MARGIN` | `1.5` | A fortified city (its hit points above a city's base: walls) is besieged by an army without a shooter that outranges its strike only when its value a round beats the best other city's this many times: such a city is taken by units that bombard it without being hit, or not at all. | own value (source not recorded) | `siege_walls.rs` |
| `SURE_CAPTURE` | `0.9` | A strike whose capture chance within `SOON_ROUNDS` is at least this is made now, whatever a wait promises. | own value (source not recorded) | `siege_walls.rs` |
| `SOON_ROUNDS` | `3` | The rounds of a strike's start that count as "now": the step in, the volley, the blow. | own value (source not recorded) | `siege_walls.rs` |
| `MIN_CAPTURE` | `0.25` | The least chance of a capture within the window that makes the group muster for it (below it the group fights as the front does). | own value (source not recorded) | `siege_walls.rs` |
| `HEADER` | `"round,target,seen,city_hp,cap,pop,group,melee,ranged,siege,wounded,group_hp,awaited,aw...` |  | own value (source not recorded) | `siege_walls.rs` |
| `NEVER` | `9999` | More rounds than any wait or siege lasts, in tenths. | own value (source not recorded) | `siege_walls.rs` |
| `CALL` | `CallFit { press: NEVER - 1, withdraw: NEVER - 1 }` | Fitted on the bank (`runs/tactic-call-fit.py`, offsets 200-206 of `runs/bank`, 460 arrival positions, two reseeds each; held out 207-209, 196 positions). | own value (source not recorded) | `siege_walls.rs` |
| `CALL_REINFORCE_ROUNDS` | `4` | Rounds a member on its way is waited for in the call's count of the group's lasting. | own value (source not recorded) | `siege_walls.rs` |
| `ARRIVAL` | `ArrivalWeights { base: 0.095, early: 0.223, late: 11.268, turns: -0.504, engaged: -0.90...` | Fitted on bench games (classic/docs/ai-siege-walls.md "Arrival"): 36 504 predictions for members not in place in 400 Quick land duels of the four rushes and `siege-walls` against `standard` (seed offset 0), each against the first later round the member stood in place (it strikes that round or the next), if it did; checked on 36 715 of another 400 (offset 40): mean predicted 0.221, observed 0.227, Brier 0.128 against 0.175 for the base rate. | own value (source not recorded) | `siege_walls.rs` |

### `sight.rs`

2 constants (own value (source not recorded) 2). Files: `classic/crates/classic-rules/src/ai/unciv/sight.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `FULL_HP` | `100` | The full health of a unit (Unciv's 100). | own value (source not recorded) | `sight.rs` |
| `CITY_BOMBARD_RANGE` | `2` | A city's bombard range (`City.getBombardRange`: 2 in the classic rules). | own value (source not recorded) | `sight.rs` |

### `site.rs`

8 constants (own value (source not recorded) 8). Files: `classic/crates/classic-rules/src/ai/site.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `RINGS` | `CITY_WORK_RADIUS as usize + 1` | Rings of the site: the centre and every distance up to the work radius. | own value (source not recorded) | `site.rs` |
| `CHANNELS` | `6` | Yield channels of a feature group (`Yields`' six). | own value (source not recorded) | `site.rs` |
| `TERRAIN_SLOTS` | `12` | Slots of the terrain and feature counts (the tables' indexes; a later index counts in the last slot). | own value (source not recorded) | `site.rs` |
| `FEATURE_SLOTS` | `12` |  | own value (source not recorded) | `site.rs` |
| `FAR` | `99` | The distance feature when no such city is known. | own value (source not recorded) | `site.rs` |
| `FOREIGN_TILES` | `TILES + 2` | The index of `foreign_tiles` (a hand edit of the site net reads it as 0: `site_net::SiteNetPlay::OwnLand`). | own value (source not recorded) | `site.rs` |
| `SITE_FEATURES` | `HEURISTIC + 2` | Number of site features. | own value (source not recorded) | `site.rs` |
| `SITE_NAMES` | `[ "nature_r0_food", "nature_r0_production", "nature_r0_gold", "nature_r0_science", "nat...` | The features' names, in order (the module docs say what each is). | own value (source not recorded) | `site.rs` |

### `site_weights.rs`

2 constants (own value (source not recorded) 2). Files: `classic/crates/classic-rules/src/ai/site_weights.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `SCORE_AT_ZERO` | `113` | The settling planner's score points of an answer of 0 citizens and of each citizen more (`site_net::planner_score`; fitted by `ai-train-site`). | own value (source not recorded) | `site_weights.rs` |
| `SCORE_PER_CITIZEN` | `169` |  | own value (source not recorded) | `site_weights.rs` |

### `state.rs`

3 constants (own value (source not recorded) 3). Files: `platform/crates/kernel/src/ai/state.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `ALL` | `[ Strategy::Stabilize, Strategy::Expand, Strategy::Defend, Strategy::Attack, Strategy::...` |  | own value (source not recorded) | `state.rs` |
| `STRATEGY_REVIEW_ROUNDS` | `10` | Rounds between two periodic strategy reviews. | own value (source not recorded) | `state.rs` |
| `TASK_FAILURE_LIMIT` | `3` | Rejected commands after which a task is given up. | own value (source not recorded) | `state.rs` |

### `strikes.rs`

12 constants (own value (source not recorded) 12). Files: `classic/crates/classic-rules/src/ai/strikes.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `FIRE_PERCENT` | `30` | A weapon is fired when its strike pays this share of its replacement cost, percent (the cost is spent; waiting has a price too: the target moves, the war ends). | own value (source not recorded) | `strikes.rs` |
| `NUKE_SUCCESS_PERCENT` | `70` | Chance an operation takes a city its nuclear weapon has struck, percent (the garrison is dead and the city lost half its health; against `SUCCESS_PERCENT` without). | own value (source not recorded) | `strikes.rs` |
| `MAX_WEAPONS` | `2` | Weapons of the player alive or queued at most (they are expensive and the war may end). | own value (source not recorded) | `strikes.rs` |
| `LONELY_PERCENT` | `25` | A city nobody besieges heals the damage of a strike: it counts this share, percent (the air arm's `LONELY_CITY_PERCENT`). | own value (source not recorded) | `strikes.rs` |
| `ARMY_RADIUS` | `6` | Own land units this close to the target are the army that follows a strike (the air arm's `ARMY_RADIUS`). | own value (source not recorded) | `strikes.rs` |
| `ENGAGED_RADIUS` | `4` | Enemy units this close to one of our cities, or to a running operation's target, are engaged with us: killing them also saves what they would take. | own value (source not recorded) | `strikes.rs` |
| `DEFENCE_TARGETS` | `3` | Enemy units a missile's build counts when they stand next to our cities. | own value (source not recorded) | `strikes.rs` |
| `MAX_CENTRES` | `16` | Candidate blast centres one decision tries. | own value (source not recorded) | `strikes.rs` |
| `DEFENCE_LIFE` | `10` | Rounds a stack at our gates is expected to stay. | own value (source not recorded) | `strikes.rs` |
| `MISSILE_PACKAGE` | `2` | Missiles that count toward an operation, and what each adds to its chance without them, percentage points; the share of a city's hit points they may take at most (the rest of the siege is the army's). | own value (source not recorded) | `strikes.rs` |
| `MISSILE_STEP` | `5` |  | own value (source not recorded) | `strikes.rs` |
| `MISSILE_HP_CAP` | `60` |  | own value (source not recorded) | `strikes.rs` |

### `tactical.rs`

12 constants (own design 1, own value (source not recorded) 11). Files: `classic/crates/classic-rules/src/ai/tactical.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `ENGAGE_RADIUS` | `7` | Members within this many tiles of an enemy join the plan. | own value (source not recorded) | `tactical.rs` |
| `READY_RADIUS` | `6` | Members within this many tiles of the city count for the siege group: the ones that can join it in a round or two (the front counts those within `GROUP_RADIUS`). | own value (source not recorded) | `tactical.rs` |
| `COMMITTED_DISTANCE` | `2` | A member this close to the city (hit points aside) commits the group; the siege test then allows this many rounds. | own value (source not recorded) | `tactical.rs` |
| `COMMITTED_ROUNDS` | `8` |  | own value (source not recorded) | `tactical.rs` |
| `MAX_MEMBERS` | `16` | Members a plan holds. | own value (source not recorded) | `tactical.rs` |
| `RANGED_WITHDRAW_BELOW` | `30` | A ranged unit leaves the line only below this health. | own value (source not recorded) | `tactical.rs` |
| `REST_BAND` | `20` | A melee unit this many hit points above its threshold, out of the enemy's reach, keeps resting. | own value (source not recorded) | `tactical.rs` |
| `REST_HEAL_WEIGHT` | `2` | Points of a withdrawal tile's rating per hit point a turn of rest heals more there than on land nobody owns (own land and cities heal faster). | own design | `tactical.rs` |
| `THEATRE_RADIUS` | `5` | Tiles around members and enemies the model covers. | own value (source not recorded) | `tactical.rs` |
| `EVALUATIONS_PER_CANDIDATE` | `24` | Evaluations of the search per candidate of the decision budget. | own value (source not recorded) | `tactical.rs` |
| `PREVIEWS_PER_CANDIDATE` | `5` | Previews of the options per candidate of the decision budget. | own value (source not recorded) | `tactical.rs` |
| `SIEGE_PREVIEWS` | `48` | Previews the siege test may use besides. | own value (source not recorded) | `tactical.rs` |

### `tactics.rs`

5 constants (own value (source not recorded) 5). Files: `classic/crates/classic-rules/src/ai/tactics.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `ATTACK_MIN_HP` | `40` | A unit attacks only with at least this much health. | own value (source not recorded) | `tactics.rs` |
| `RETREAT_BELOW_HP` | `35` | A unit this hurt next to an enemy falls back. | own value (source not recorded) | `tactics.rs` |
| `WEAK_CITY_HP` | `50` | A city at or below this health is attacked without waiting for a group. | own value (source not recorded) | `tactics.rs` |
| `SIEGE_GROUP` | `3` | Units of the assault group that gather before the siege starts. | own value (source not recorded) | `tactics.rs` |
| `RALLY_ROUNDS` | `8` | Rounds an assault unit waits for its group before it attacks anyway. | own value (source not recorded) | `tactics.rs` |

### `tall.rs`

11 constants (own value (source not recorded) 11). Files: `classic/crates/classic-rules/src/ai/tall.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `FIRST` | `BookPlan { scout_first: false, tenets_first: false, filler: false }` | The first edition (`tall-book`): no scout, the culture waits, the science lens after the science branch. | own value (source not recorded) | `tall.rs` |
| `SECOND` | `BookPlan { scout_first: true, tenets_first: true, filler: false }` |  | own value (source not recorded) | `tall.rs` |
| `FILLER` | `BookPlan { scout_first: true, tenets_first: true, filler: true }` |  | own value (source not recorded) | `tall.rs` |
| `MAX_CITIES` | `40` | Most cities the model looks at. | own value (source not recorded) | `tall.rs` |
| `SITE_POINTS_PER_FOOD` | `3 * 3` | Site points of a food a turn on a worked tile (`planner_settle`'s land tile score: 3 a food; a tile of the worked rings counts 3 times). | own value (source not recorded) | `tall.rs` |
| `BOOK_CITIES` | `4` | The book's cities: exactly four. | own value (source not recorded) | `tall.rs` |
| `BOOK_GROWTH_BRANCH` | `"core:policy/tradition"` | The book's growth branch, completed first. | own value (source not recorded) | `tall.rs` |
| `BOOK_SCIENCE_BRANCH` | `"core:policy/rationalism"` | The book's science branch, completed second. | own value (source not recorded) | `tall.rs` |
| `BOOK_OBSERVATORY` | `"core:building/observatory"` | The building whose site the book's cities look for (a mountain next to the city). | own value (source not recorded) | `tall.rs` |
| `BOOK_BUILDINGS` | `["core:building/library", "core:building/university", "core:building/observatory", "cor...` | The book's science buildings, in the order each city builds them. | own value (source not recorded) | `tall.rs` |
| `BOOK_MOUNTAIN_POINTS` | `300` | Site points of a site next to a mountain for the book: about ten turns of a settler's walk (`planner_settle::TRAVEL_COST`). | own value (source not recorded) | `tall.rs` |

### `tall_real.rs`

4 constants (own value (source not recorded) 4). Files: `classic/crates/classic-rules/src/ai/tall_real.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `EARLIEST_VICTORY_PERCENT` | `75` | No economy has won by science before this share of the speed's game (percent): the earliest victory of `tall-book-2`, the book that wins by science, came at round 249 of 330 at Quick and 405 of 500 at Standard (classic/docs/ai-science-audit.md; `victory-price` uses the same bound). | own value (source not recorded) | `tall_real.rs` |
| `SCIENCE_GROWTH_POWER` | `2` | The science a turn grows as the round to this power: the seat-mean science of `standard` on the pace bench grew ×29 over ×5 rounds, a power of 2.1 (`victory-price`'s measure, reach/LOG.md). | own value (source not recorded) | `tall_real.rs` |
| `CONTENT_FLOOR` | `crate::ai::value::HAPPINESS_COMFORT as i64` | Excess happiness the empire keeps after a new city and its first citizen: the allocator's comfort (`value::HAPPINESS_COMFORT`, below which growth stalls soon), so that the cities it has keep the headroom to grow; the science branch's opener adds its percent only while the empire is not unhappy (`SocietyStat::ScienceWhileHappyPercent`). | own value (source not recorded) | `tall_real.rs` |
| `MAX_MORE` | `12` | At most this many new cities are looked at in a turn (a bound of the loop: the model's share test stops every seat of the tuning traces well before it). | own value (source not recorded) | `tall_real.rs` |

### `taste.rs`

8 constants (own value (source not recorded) 8). Files: `classic/crates/classic-rules/src/ai/taste.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `BAND_PERCENT` | `10` | Width of the band of near-equal options, percent of the best value. | own value (source not recorded) | `taste.rs` |
| `NONE` | `Sides(0)` |  | own value (source not recorded) | `taste.rs` |
| `EXPANSION` | `Sides(1)` |  | own value (source not recorded) | `taste.rs` |
| `WONDERS` | `Sides(1 << 1)` |  | own value (source not recorded) | `taste.rs` |
| `SCIENCE` | `Sides(1 << 2)` |  | own value (source not recorded) | `taste.rs` |
| `CULTURE` | `Sides(1 << 3)` |  | own value (source not recorded) | `taste.rs` |
| `GOLD` | `Sides(1 << 4)` |  | own value (source not recorded) | `taste.rs` |
| `FAITH` | `Sides(1 << 5)` |  | own value (source not recorded) | `taste.rs` |

### `trace.rs`

1 constants (own value (source not recorded) 1). Files: `classic/crates/classic-rules/src/ai/unit_net/trace.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `ENV` | `"AURELIA_UNIT_NET_TRACE"` | The environment variable that turns the trace on. | own value (source not recorded) | `trace.rs` |

### `trade.rs`

5 constants (own value (source not recorded) 5). Files: `classic/crates/classic-rules/src/ai/trade.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `TRADE_FROM_POPULATION` | `8` | Population of the largest city before the first trade unit. | own value (source not recorded) | `trade.rs` |
| `HOSTILE_REACH` | `3` | Tiles from the straight way between a route's two cities within which a hostile military unit or a barbarian camp makes the route a risk (`HAZARD_LOSS_PERCENT`): the rules plunder a route whose unit meets an enemy combat unit on its path (`derived::trade::enemy_on`), and a unit of two moves a turn reaches the way within the route's first turns. | own value (source not recorded) | `trade.rs` |
| `FOOD_PRICE_MIN_PERCENT` | `50` | Bounds of a destination's food price, percent of the empire's average one (`food_price_in`). | own value (source not recorded) | `trade.rs` |
| `FOOD_PRICE_MAX_PERCENT` | `200` |  | own value (source not recorded) | `trade.rs` |
| `HAZARD_LOSS_PERCENT` | `18` | Chance (percent) that a route with a hazard near its way is lost before its end, measured on `standard`'s routes (which ignore hazards; Quick, solo, 80 games, `ai-trade`): 17.5 % of the routes with a hazard at their start were lost on the standard suite, 18 % on the duel suite, against 1.5 % and 4 % without one (classic/docs/ai-trade.md). | own value (source not recorded) | `trade.rs` |

### `value.rs`

17 constants (own value (source not recorded) 17). Files: `classic/crates/classic-rules/src/ai/value.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `BASE` | `Unhappiness { city: crate::derived::happiness::UNHAPPINESS_FROM_CITY_TIMES100, citizen:...` | The rules' base rates (`derived::happiness`: unhappiness per city and per citizen), before the empire's modifiers. | own value (source not recorded) | `value.rs` |
| `HAPPINESS_COMFORT` | `6` | Excess happiness at which one more point stops being worth a citizen: below it growth stalls soon (−75 % food while unhappy). | own value (source not recorded) | `value.rs` |
| `TILE_PRICE` | `75` | Gold a bought tile costs on average: an own estimate (Unciv's tile price, `CityExpansionManager.getGoldCostOfTile`, is 50 per ring beyond the first plus 5 per tile the city already claimed, scaled by game speed; ours is a flat mean). | own value (source not recorded) | `value.rs` |
| `PROCESS_SHARE` | `25` | Share of production the Wealth and Research processes convert, percent. | own value (source not recorded) | `value.rs` |
| `UNREST_RECOVERY_ROUNDS` | `120` | Standard rounds an empire takes to climb back over a mood line a captured city pushed it below (`Prices::kept_city_unrest_recovering`): after a puppet given at round 66 (Quick) the taker's excess happiness was −4.3 ten rounds later and −1.3 sixty rounds later (classic/docs/ai-conquest-payoff.md, the gift twins), about 80 Quick rounds to the line. | own value (source not recorded) | `value.rs` |
| `FOUNDING_MARGIN` | `2` | Points of excess happiness a new city must leave over (`Prices::affords_city`). | own value (source not recorded) | `value.rs` |
| `CRISIS_CITIZENS` | `6` | Citizens a point of happiness is worth at least at the very-unhappy line (`Outlook::crisis`): enough that a happiness building or a luxury beats sitting at the line unless the other choice gains a lot of score (bench, offsets 20–21: 4 left 32 % of the seats at or below the line at the end, 6 with `Prices::local_cap` 26 %, 8 28 %, 20 no fewer and less strength; classic/docs/ai-allocator.md). | own value (source not recorded) | `value.rs` |
| `OPENING_CITIES` | `3` | The empire's first cities found the opening: their citizens count as the empire's growth (the food price), not against its happiness: a player founds them for their land. | own value (source not recorded) | `value.rs` |
| `FIRST_RING` | `7` | Land tiles of a new city: its first ring. | own value (source not recorded) | `value.rs` |
| `TILE_ROUNDS` | `25` | Rounds a city's borders take for one more tile (bench: ~18 tiles per city after 250 rounds; Standard speed, × the speed's pace). | own value (source not recorded) | `value.rs` |
| `CITIZEN_ROUNDS` | `25` | Rounds a new city takes for one more citizen (bench: ~10 citizens per city after 250 rounds; Standard speed, × the speed's pace). | own value (source not recorded) | `value.rs` |
| `NEW_CITY_PRODUCTION` | `2` | Yields of a young city a turn. | own value (source not recorded) | `value.rs` |
| `NEW_CITY_SCIENCE` | `2` |  | own value (source not recorded) | `value.rs` |
| `NEW_CITY_UNHAPPINESS` | `4` | A city and its first citizen. | own value (source not recorded) | `value.rs` |
| `MAX_CITIZENS_PER_HAPPINESS` | `10` | Most citizens one point of happiness is worth. | own value (source not recorded) | `value.rs` |
| `TECH_ROUNDS` | `9` | Rounds an empire takes for a technology (bench: 34 in 300 rounds; Standard speed, × the speed's pace). | own value (source not recorded) | `value.rs` |
| `UNLOCK_WONDER_CHANCE` | `50` | Percent of a wonder's surplus that counts as ours when the technology that opens it is learned: the others build wonders too. | own value (source not recorded) | `value.rs` |

### `variant.rs`

2 constants (own value (source not recorded) 2). Files: `classic/crates/classic-rules/src/ai/variant.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `NET_BASE` | `Variant::PrePuppet` | The net's base: the expert the end-to-end net learns from and the mind it plays on top of. | own value (source not recorded) | `variant.rs` |
| `ALL` | `[ Variant::Standard, Variant::PreCamps, Variant::PreWorkers, Variant::PreJoin, Variant:...` | The game's AI first. | own value (source not recorded) | `variant.rs` |

### `victory.rs`

2 constants (own value 2). Files: `classic/crates/classic-rules/src/ai/victory.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `STUCK_FACTOR` | `2` | A part waiting in a city is moved when another city finishes it in at most `1 / STUCK_FACTOR` of the turns it still needs where it is, and at least `STUCK_MIN_TURNS` sooner (own values: a move must be worth more than the noise of the estimate, and must not swing back the next turn; `STUCK_MIN_TURNS` is production turns at Standard speed, × the speed's pace). | own value | `victory.rs` |
| `STUCK_MIN_TURNS` | `5` | (with `STUCK_FACTOR`) A part waiting in a city is moved when another city finishes it in at most `1 / STUCK_FACTOR` of the turns it still needs where it is, and at least `STUCK_MIN_TURNS` sooner (own values: a move must be worth more than the noise of the estimate, and must not swing back the next turn; `STUCK_MIN_TURNS` is production turns at Standard speed, × the speed's pace). | own value | `victory.rs` |

### `weights.rs`

322 constants (own value 2, own value (source not recorded) 320). Files: `classic/crates/classic-rules/src/ai/net_e2e/v2/weights.rs`, `classic/crates/classic-rules/src/ai/strategy_net/weights.rs`, `classic/crates/classic-rules/src/ai/unit_net/weights.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `CLIP` | `128.0` |  | own value (source not recorded) | `weights.rs` |
| `N_BUCKETS` | `14` | Hex distance buckets (`model_v2._BUCKET_OF`, `N_BUCKETS`, `NO_POSITION`). | own value (source not recorded) | `weights.rs` |
| `N_BUCKETS_PAD` | `16` | [`N_BUCKETS`] rounded up to whole vector blocks. | own value (source not recorded) | `weights.rs` |
| `NO_POSITION` | `13` |  | own value (source not recorded) | `weights.rs` |
| `CAP_FOOD` | `0` |  | own value (source not recorded) | `weights.rs` |
| `CAP_PROD` | `1` |  | own value (source not recorded) | `weights.rs` |
| `CAP_LUX` | `2` |  | own value (source not recorded) | `weights.rs` |
| `ROOM` | `3` |  | own value (source not recorded) | `weights.rs` |
| `SITES_FOOD` | `4` |  | own value (source not recorded) | `weights.rs` |
| `RIVAL_NEAR` | `5` |  | own value (source not recorded) | `weights.rs` |
| `RIVAL_KNOWN` | `6` |  | own value (source not recorded) | `weights.rs` |
| `BARBS` | `7` |  | own value (source not recorded) | `weights.rs` |
| `RUINS` | `8` |  | own value (source not recorded) | `weights.rs` |
| `CARAVAN_FOOD` | `9` |  | own value (source not recorded) | `weights.rs` |
| `CITY_STATES` | `10` |  | own value (source not recorded) | `weights.rs` |
| `CS_NEAR` | `11` |  | own value (source not recorded) | `weights.rs` |
| `COASTAL` | `12` |  | own value (source not recorded) | `weights.rs` |
| `ERA` | `13` |  | own value (source not recorded) | `weights.rs` |
| `PROGRESS` | `14` |  | own value (source not recorded) | `weights.rs` |
| `CITIES` | `15` |  | own value (source not recorded) | `weights.rs` |
| `CITIZENS` | `16` |  | own value (source not recorded) | `weights.rs` |
| `SCIENCE` | `17` |  | own value (source not recorded) | `weights.rs` |
| `PRODUCTION` | `18` |  | own value (source not recorded) | `weights.rs` |
| `GOLD` | `19` |  | own value (source not recorded) | `weights.rs` |
| `AT_WAR` | `20` |  | own value (source not recorded) | `weights.rs` |
| `FORCE` | `21` |  | own value (source not recorded) | `weights.rs` |
| `THREAT` | `22` |  | own value (source not recorded) | `weights.rs` |
| `HAPPY` | `23` |  | own value (source not recorded) | `weights.rs` |
| `UNHAPPY` | `24` |  | own value (source not recorded) | `weights.rs` |
| `FAITH` | `25` |  | own value (source not recorded) | `weights.rs` |
| `RELIGION` | `26` |  | own value (source not recorded) | `weights.rs` |
| `TOURISM` | `27` |  | own value (source not recorded) | `weights.rs` |
| `TECH_GAP` | `28` |  | own value (source not recorded) | `weights.rs` |
| `TECH_LEAD` | `29` |  | own value (source not recorded) | `weights.rs` |
| `SCORE_GAP` | `30` |  | own value (source not recorded) | `weights.rs` |
| `WONDERS_TAKEN` | `31` |  | own value (source not recorded) | `weights.rs` |
| `OUR_WONDERS` | `32` |  | own value (source not recorded) | `weights.rs` |
| `RIVALS` | `33` |  | own value (source not recorded) | `weights.rs` |
| `T_EXPANSION` | `34` |  | own value (source not recorded) | `weights.rs` |
| `T_WONDERS` | `35` |  | own value (source not recorded) | `weights.rs` |
| `T_SCIENCE` | `36` |  | own value (source not recorded) | `weights.rs` |
| `T_CULTURE` | `37` |  | own value (source not recorded) | `weights.rs` |
| `T_GOLD` | `38` |  | own value (source not recorded) | `weights.rs` |
| `T_RELIGION` | `39` |  | own value (source not recorded) | `weights.rs` |
| `T_AGGRESSION` | `40` |  | own value (source not recorded) | `weights.rs` |
| `T_WAR_TOLERANCE` | `41` |  | own value (source not recorded) | `weights.rs` |
| `T_DIPLOMACY` | `42` |  | own value (source not recorded) | `weights.rs` |
| `UNIQUE_MILITARY` | `43` |  | own value (source not recorded) | `weights.rs` |
| `IDEOLOGY` | `44` |  | own value (source not recorded) | `weights.rs` |
| `ROUTES_LEFT` | `45` |  | own value (source not recorded) | `weights.rs` |
| `OPPORTUNITY` | `46` | `war-open`'s features (`features::open`); 0 for every other variant. | own value (source not recorded) | `weights.rs` |
| `OP_FORMING` | `47` |  | own value (source not recorded) | `weights.rs` |
| `STRENGTH` | `48` |  | own value (source not recorded) | `weights.rs` |
| `PAYS` | `49` |  | own value (source not recorded) | `weights.rs` |
| `SPACE_OWN` | `50` | `sharp`'s features (`sharp::features`); 0 for every other variant. | own value (source not recorded) | `weights.rs` |
| `SPACE_RIVAL` | `51` |  | own value (source not recorded) | `weights.rs` |
| `SCORE_TREND` | `52` |  | own value (source not recorded) | `weights.rs` |
| `RUNAWAY` | `53` |  | own value (source not recorded) | `weights.rs` |
| `RIVAL_POPULATION` | `54` | The model of the rivals (`ai::rival_model`, `features::rivals`), every variant's: read by no neuron yet (`rival`: every weight 0), the inputs of the next step's training. | own value (source not recorded) | `weights.rs` |
| `RIVAL_CROP` | `55` |  | own value (source not recorded) | `weights.rs` |
| `RIVAL_GOODS` | `56` |  | own value (source not recorded) | `weights.rs` |
| `RIVAL_GNP` | `57` |  | own value (source not recorded) | `weights.rs` |
| `RIVAL_LITERACY` | `58` |  | own value (source not recorded) | `weights.rs` |
| `RIVAL_LAND` | `59` |  | own value (source not recorded) | `weights.rs` |
| `RIVAL_SOLDIERS` | `60` |  | own value (source not recorded) | `weights.rs` |
| `RIVAL_SCIENCE` | `61` |  | own value (source not recorded) | `weights.rs` |
| `RIVAL_SCORE_TREND` | `62` |  | own value (source not recorded) | `weights.rs` |
| `RIVAL_DOUBT` | `63` |  | own value (source not recorded) | `weights.rs` |
| `RIVAL_DOMINATION` | `64` |  | own value (source not recorded) | `weights.rs` |
| `RIVAL_TREE` | `65` |  | own value (source not recorded) | `weights.rs` |
| `RIVAL_CULTURE` | `66` |  | own value (source not recorded) | `weights.rs` |
| `RIVAL_DIPLOMACY` | `67` |  | own value (source not recorded) | `weights.rs` |
| `RIVAL_FIRST` | `RIVAL_POPULATION` | The first of them, in `Measure::ALL`'s order for the seven rows. | own value (source not recorded) | `weights.rs` |
| `N` | `68` |  | own value (source not recorded) | `weights.rs` |
| `BASE_N` | `46` | The features every variant's net reads: the memory keeps these of a style's lock (`Memory::encode`), `war-open`'s are events of the turn (`open::STYLE_EVENTS`). | own value (source not recorded) | `weights.rs` |
| `FEATURES` | `[ ("cap_food", "mean food of the capital's 12 best tiles: 1.2 → 0, 3.0 → 1"), ("cap_pro...` | Each feature's name and scale (0 … 1). | own value (source not recorded) | `weights.rs` |
| `GOOD_LAND` | `0` |  | own value (source not recorded) | `weights.rs` |
| `ROOM_TO_EXPAND` | `1` |  | own value (source not recorded) | `weights.rs` |
| `TALL_GROWTH` | `2` |  | own value (source not recorded) | `weights.rs` |
| `DANGER` | `3` |  | own value (source not recorded) | `weights.rs` |
| `WARLIKE` | `4` |  | own value (source not recorded) | `weights.rs` |
| `EARLY` | `5` |  | own value (source not recorded) | `weights.rs` |
| `STRIKE_WINDOW` | `6` |  | own value (source not recorded) | `weights.rs` |
| `WONDER_CHANCE` | `7` |  | own value (source not recorded) | `weights.rs` |
| `SCIENCE_LEAD` | `8` |  | own value (source not recorded) | `weights.rs` |
| `CULTURE_PRESSURE` | `9` |  | own value (source not recorded) | `weights.rs` |
| `HAPPINESS_CRISIS` | `10` |  | own value (source not recorded) | `weights.rs` |
| `CITY_STATE_PLAY` | `11` |  | own value (source not recorded) | `weights.rs` |
| `RELIGIOUS_PLAY` | `12` |  | own value (source not recorded) | `weights.rs` |
| `N` | `13` |  | own value (source not recorded) | `weights.rs` |
| `CONCEPTS` | `[ Neuron { name: "good_land", bias: -100, why: "a middling capital is not good land", a...` | The concept neurons: AND(a, b) is relu(a + b − 1), NOT a is a negative weight, a threshold is the bias; every concept is clamped to 0…1. | own value (source not recorded) | `weights.rs` |
| `STYLES` | `["tall", "wide", "conquest"]` |  | own value (source not recorded) | `weights.rs` |
| `AIMS` | `["science", "culture", "domination", "diplomacy"]` |  | own value (source not recorded) | `weights.rs` |
| `POSTURES` | `["peace", "defence", "prepare", "war"]` |  | own value (source not recorded) | `weights.rs` |
| `STYLE_TALL` | `0` | The choices the code names, by their index in `STYLES` and `AIMS` (the test `the_named_choices_are_their_names` reads them back). | own value (source not recorded) | `weights.rs` |
| `AIM_SCIENCE` | `0` |  | own value (source not recorded) | `weights.rs` |
| `AIM_DIPLOMACY` | `3` |  | own value (source not recorded) | `weights.rs` |
| `STYLE` | `[ Neuron { name: "style_tall", bias: -200, why: "no tilt without a reason", act: Act::L...` |  | own value (source not recorded) | `weights.rs` |
| `AIM` | `[ Neuron { name: "aim_science", bias: 300, why: "the science victory is the project's g...` |  | own value (source not recorded) | `weights.rs` |
| `POSTURE` | `[ Neuron { name: "posture_peace", bias: 1000, why: "peace is the default", act: Act::Li...` |  | own value (source not recorded) | `weights.rs` |
| `WINDOW` | `Neuron { name: "opportunity_window", bias: -2100, why: "AND(a war that pays, opportunit...` | TUNED, not derived: the bias and the term weights below were set by hand on the tuning seeds (offsets 200–201, classic/docs/ai-war-open.md); `war-pays` keeps them and makes `PAYS` a condition (`Net::pays`). | own value (source not recorded) | `weights.rs` |
| `STYLE` | `&[ (2, f(OPPORTUNITY, 1000, "conquest: a target worth taking, in any era (the window ad...` | Terms added to the logits of the styles, the aims and the postures: (index, term). | own value (source not recorded) | `weights.rs` |
| `AIM` | `&[(2, f(OPPORTUNITY, 600, "domination: the capitals come into reach when cities fall"))]` |  | own value (source not recorded) | `weights.rs` |
| `POSTURE` | `&[ (2, f(OP_FORMING, 2000, "the war machine forms an operation: the posture prepares it...` |  | own value (source not recorded) | `weights.rs` |
| `STYLE_EVENTS` | `[OPPORTUNITY, OP_FORMING, STRENGTH, PAYS]` | The features a locked style takes from the turn besides `STYLE_EVENTS`: an opportunity and the army's strength change with the neighbours, not with the empire's own growth. | own value (source not recorded) | `weights.rs` |
| `AIM` | `&[ (0, f(RIVAL_TREE, 0, "science: a rival nearer the end of the tree (race it or turn t...` | Terms added to the aims' logits (science, culture, domination, diplomacy). | own value (source not recorded) | `weights.rs` |
| `POSTURE` | `&[ (1, f(RIVAL_SOLDIERS, 0, "defence: a rival's army larger than ours")), (2, f(RIVAL_G...` | Terms added to the postures' logits (peace, defence, prepare, war). | own value (source not recorded) | `weights.rs` |
| `WIDE` | `(-1200, "wide needs a strong reason, not room alone: a seat held to tall, science and p...` | Added to the wide style's logit. | own value (source not recorded) | `weights.rs` |
| `BIAS` | `(-1000, "a rival level with us is no threat: the race is open")` |  | own value (source not recorded) | `weights.rs` |
| `TREE` | `(1200, "its share of the tree is its road to the spaceship: a rival with the space tech...` |  | own value (source not recorded) | `weights.rs` |
| `LEAD` | `(1000, "a tenth of the tree ahead of us: it reaches the spaceship first")` |  | own value (source not recorded) | `weights.rs` |
| `PACE` | `(600, "it researches faster than we do: the lead grows (twice our science gives +200)")` |  | own value (source not recorded) | `weights.rs` |
| `LEAD_SCALE` | `10` | The tenth of the tree a full lead stands for (per mille of the tree, × 10). | own value (source not recorded) | `weights.rs` |
| `MIN` | `(300, "below it the rival is one of the race")` | The least threat the plan names: below it the rival is in the race, not running away. | own value (source not recorded) | `weights.rs` |
| `DENOUNCE` | `(500, "a rival more likely than not to run away: no help to it, and the world told")` | The threat from which the rival is denounced and offered no friendship, research or open borders. | own value (source not recorded) | `weights.rs` |
| `POSTURE` | `&[ (2, 2000, "prepare: the army that stops the leader before it wins (a threat of 0.6 o...` | Terms of the threat in the posture logits (peace, defence, prepare, war): (posture, weight, why). | own value (source not recorded) | `weights.rs` |
| `HYSTERESIS` | `1150` | A choice of the intent changes only when the new one's share leads the held one's by more than this (thousandths: 15 %). | own value (source not recorded) | `weights.rs` |
| `LOCK_MIN_ROUNDS` | `20` | The style is chosen once (classic/docs/ai-strategy-net.md, "The style lock"): it is locked when the first branch may be opened after the first survey of the surroundings (`LOCK_MIN_ROUNDS` Standard rounds), or at `LOCK_MAX_ROUNDS` at the latest. | own value (source not recorded) | `weights.rs` |
| `LOCK_MAX_ROUNDS` | `40` |  | own value (source not recorded) | `weights.rs` |
| `STYLE_LOCK` | `1500` |  | own value (source not recorded) | `weights.rs` |
| `STYLE_EVENTS` | `[AT_WAR, THREAT, FORCE, UNHAPPY]` |  | own value (source not recorded) | `weights.rs` |
| `SURVIVAL_THREAT` | `300` |  | own value (source not recorded) | `weights.rs` |
| `SURVIVAL_DANGER` | `600` |  | own value (source not recorded) | `weights.rs` |
| `CITY_ROWS` | `[ (500, 0, "tall takes the best few sites: half of the free ones (the book keeps four c...` | More cities by style: (per free site, base, why). | own value (source not recorded) | `weights.rs` |
| `CITY_CRISIS` | `-6000` | Fewer new cities per unit of the happiness crisis. | own value (source not recorded) | `weights.rs` |
| `CITY_DANGER` | `-1000` | Fewer new cities per unit of danger. | own value (source not recorded) | `weights.rs` |
| `CITY_FLOOR` | `4` | The plan keeps at least this many cities while sites are free. | own value (source not recorded) | `weights.rs` |
| `CITY_CEILING_WIDE` | `1000` | The ceiling of the city count: the rules' city model (`tall::city_target`: the count with the most science per research cost) plus this many per unit of the wide and the conquest shares (thousandths of a city). | own value (source not recorded) | `weights.rs` |
| `CITY_CEILING_CONQUEST` | `500` |  | own value (source not recorded) | `weights.rs` |
| `HORIZON_BASE` | `60` | The horizon H in Standard rounds: base and each term's weight (rounds per unit of a share or a concept). | own value (source not recorded) | `weights.rs` |
| `HORIZON_TERMS` | `[ ("style_tall", 50, "tall play invests in growth that pays late"), ("aim_science", 30,...` |  | own value (source not recorded) | `weights.rs` |
| `HORIZON_MIN` | `20` |  | own value (source not recorded) | `weights.rs` |
| `HORIZON_MAX` | `150` |  | own value (source not recorded) | `weights.rs` |
| `SCIENCE` | `0` |  | own value (source not recorded) | `weights.rs` |
| `PRODUCTION` | `1` |  | own value (source not recorded) | `weights.rs` |
| `GOLD` | `2` |  | own value (source not recorded) | `weights.rs` |
| `CULTURE` | `3` |  | own value (source not recorded) | `weights.rs` |
| `FAITH` | `4` |  | own value (source not recorded) | `weights.rs` |
| `HAPPINESS` | `5` |  | own value (source not recorded) | `weights.rs` |
| `MILITARY` | `6` |  | own value (source not recorded) | `weights.rs` |
| `WONDER` | `7` |  | own value (source not recorded) | `weights.rs` |
| `EXPANSION` | `8` |  | own value (source not recorded) | `weights.rs` |
| `TRADE` | `9` |  | own value (source not recorded) | `weights.rs` |
| `GREAT_PEOPLE` | `10` |  | own value (source not recorded) | `weights.rs` |
| `TOURISM` | `11` |  | own value (source not recorded) | `weights.rs` |
| `WORKER` | `12` |  | own value (source not recorded) | `weights.rs` |
| `CITIZEN` | `13` |  | own value (source not recorded) | `weights.rs` |
| `FOOD` | `14` |  | own value (source not recorded) | `weights.rs` |
| `N` | `15` |  | own value (source not recorded) | `weights.rs` |
| `PRICES` | `[ "science", "production", "gold", "culture", "faith", "happiness", "military", "wonder...` |  | own value (source not recorded) | `weights.rs` |
| `ROWS` | `[ "style_tall", "style_wide", "style_conquest", "aim_science", "aim_culture", "aim_domi...` | The rows of the price layer: the intent's shares (3 styles, 4 aims, 4 postures) and the two gates (danger, happiness crisis). | own value (source not recorded) | `weights.rs` |
| `PRICE_ROWS` | `{ use pd::*; &[ (0, SCIENCE, 500, "a tall empire turns its citizens into beakers"), (0,...` | (row, price, deviation from the allocator's price per unit of the row, why). | own value (source not recorded) | `weights.rs` |
| `PRICE_GAIN` | `500` | The share of the written deviations a price takes (thousandths): the allocator's prices are already the score's, the intent tilts them (at the full deviations the net bought science buildings over food and gold and grew 21 % fewer citizens: dev bench, offsets 120–125). | own value (source not recorded) | `weights.rs` |
| `PRICE_MIN` | `100` | The least and the most a multiplier may be (thousandths). | own value (source not recorded) | `weights.rs` |
| `PRICE_MAX` | `6000` |  | own value (source not recorded) | `weights.rs` |
| `FOOD_MIN` | `700` | The food price from the cell, as a multiplier of the allocator's, is kept within these bounds (thousandths). | own value (source not recorded) | `weights.rs` |
| `FOOD_MAX` | `4000` |  | own value (source not recorded) | `weights.rs` |
| `STEPS` | `10` | Coarse steps of the cell over the horizon H. | own value (source not recorded) | `weights.rs` |
| `TILE_WEIGHTS` | `[3, 2, 1, 2, 1, 1]` | The weights a citizen's tile is chosen by (per unit of a yield): food, production, gold, science, culture, faith. | own value (source not recorded) | `weights.rs` |
| `UNHAPPY_GROWTH` | `25` | The growth an unhappy empire keeps (percent): unhappy −75 %, very unhappy (−10) none (the rules' moods). | own value (source not recorded) | `weights.rs` |
| `FED_SURPLUS` | `2` | Food a turn a city keeps above what it eats before its citizens prefer hammers and beakers to food. | own value (source not recorded) | `weights.rs` |
| `VERY_UNHAPPY` | `-10` |  | own value (source not recorded) | `weights.rs` |
| `PRODUCING_SHARE` | `30` | A production percent toward some items counts this share of the city's production when the planner has no share of its own. | own value (source not recorded) | `weights.rs` |
| `GARRISON_SHARE` | `50` | A garrison is assumed in this share of the cities (a garrison condition counts this much). | own value (source not recorded) | `weights.rs` |
| `CENTRE_FOOD` | `2` | The yields of a city centre are at least these (food, production). | own value (source not recorded) | `weights.rs` |
| `CENTRE_PRODUCTION` | `1` |  | own value (source not recorded) | `weights.rs` |
| `MAX` | `5` | Goals chosen at most, and at least while any is worth something. | own value (source not recorded) | `weights.rs` |
| `MIN` | `3` |  | own value (source not recorded) | `weights.rs` |
| `MAX_BUILDINGS` | `2` | Key buildings the plan holds beside those goals (they are cheap commitments the allocator makes anyway: they must not crowd out the wonders, branches and technologies). | own value (source not recorded) | `weights.rs` |
| `STOP_SHARE` | `200` | A goal beyond the minimum stops the pointer when its rate is below this share of the first goal's rate (thousandths). | own value (source not recorded) | `weights.rs` |
| `CANDIDATES` | `24` | Candidates the pointer keeps after a first screening by their value alone. | own value (source not recorded) | `weights.rs` |
| `CHANCE_BASE` | `850` | A world wonder's chance to be ours (thousandths): base, per rival met in or past its era, per Standard round to get it, per unit of the wonder-chance concept, bounds. | own value (source not recorded) | `weights.rs` |
| `CHANCE_PER_RIVAL` | `-120` |  | own value (source not recorded) | `weights.rs` |
| `CHANCE_PER_ROUND` | `-8` |  | own value (source not recorded) | `weights.rs` |
| `CHANCE_CONCEPT` | `200` |  | own value (source not recorded) | `weights.rs` |
| `CHANCE_MIN` | `100` |  | own value (source not recorded) | `weights.rs` |
| `CHANCE_MAX` | `950` |  | own value (source not recorded) | `weights.rs` |
| `PROJECT_TECH_WONDERS` | `3` | A technology that opens a project (the spaceship's) is worth this many world wonders' points times the science aim's share. | own value (source not recorded) | `weights.rs` |
| `REVIEW_ROUNDS` | `5` | The goals are reviewed every this many Standard rounds (and when a goal is lost, the cities change or a war starts or ends). | own value (source not recorded) | `weights.rs` |
| `SURVEY_ROUNDS` | `10` | The land is surveyed again every this many Standard rounds. | own value (source not recorded) | `weights.rs` |
| `TALL` | `0` | Few strong cities: growth, the capital, free buildings in the first cities, border growth. | own value (source not recorded) | `weights.rs` |
| `WIDE` | `1` | Many cities: settlers, workers, per-city yields and happiness, the policy cost of a city. | own value (source not recorded) | `weights.rs` |
| `WAR` | `2` | Fighting units: combat, promotions, experience, kills, generals. | own value (source not recorded) | `weights.rs` |
| `DEFENCE` | `3` | The cities' defence: garrisons, city combat. | own value (source not recorded) | `weights.rs` |
| `SCIENCE` | `4` |  | own value (source not recorded) | `weights.rs` |
| `GOLD` | `5` |  | own value (source not recorded) | `weights.rs` |
| `FAITH` | `6` |  | own value (source not recorded) | `weights.rs` |
| `CULTURE` | `7` |  | own value (source not recorded) | `weights.rs` |
| `DIPLOMACY` | `8` |  | own value (source not recorded) | `weights.rs` |
| `N` | `9` |  | own value (source not recorded) | `weights.rs` |
| `CHARACTERS` | `["tall", "wide", "war", "defence", "science", "gold", "faith", "culture", "diplomacy"]` |  | own value (source not recorded) | `weights.rs` |
| `FIT` | `&[ (Style(0), TALL, 1500, "tall: the growth branch is the play of few strong cities"),...` | (input, character, weight per unit of the input and of the character's share, why). | own value (source not recorded) | `weights.rs` |
| `FIT_MIN` | `100` | The bounds of the fit (thousandths). | own value (source not recorded) | `weights.rs` |
| `FIT_MAX` | `4000` |  | own value (source not recorded) | `weights.rs` |
| `WAR_GATE` | `true` | A branch whose largest character is war is opened and continued only in the war posture (the owner's play: "the war branch only at war"). | own value (source not recorded) | `weights.rs` |
| `IDEOLOGY_FIT` | `2000` | The fit of the chosen ideology's tenets (it was chosen for the plan): on par with the style's branch, below the science branch. | own value (source not recorded) | `weights.rs` |
| `TURNS_PAD` | `2` | Every head divides its score by (turns + TURNS_PAD). | own value (source not recorded) | `weights.rs` |
| `BEELINE` | `5000` | Technologies: the share of a goal's worth carried back to each technology of its missing prerequisites (by the share of the path's cost the technology is), thousandths. | own value | `weights.rs` |
| `SHARED_BONUS` | `300` | A technology on the paths of two or more goals: its goal value × (1 + this). | own value (source not recorded) | `weights.rs` |
| `KEEP_MARGIN` | `1150` | The target technology is kept unless another leads its path by this much (thousandths: 15 %). | own value (source not recorded) | `weights.rs` |
| `OPEN_MARGIN` | `1500` | Policies: a new branch is opened only when its step leads the best started branch's by this much (thousandths: 50 %), so a branch is finished first (its finisher, and the policy cost that grows with every adoption whatever the branch; traced games opened three branches at 10 %). | own value (source not recorded) | `weights.rs` |
| `GOAL_BRANCH` | `500` | A goal branch's steps are worth this much more (thousandths). | own value (source not recorded) | `weights.rs` |
| `VALUE_WEIGHT` | `500` | Policies: a branch is scored by its fit (`branch::FIT`) × (1 + this × its step's value in the cell over the best candidate's): the owner's play is the key, the cell breaks the near ties (the cell sees little of the growth and science branches: border growth, free buildings, a science percent while content, free technologies). | own value (source not recorded) | `weights.rs` |
| `SWITCH_PRESSURE` | `600` | The ideology is switched at unrest (the rules force it at upheaval) when the culture-pressure concept is above this. | own value (source not recorded) | `weights.rs` |
| `GOAL_ITEM` | `500` | Production: a goal's item takes this share of the goal's worth beside its own value (thousandths). | own value | `weights.rs` |
| `MILITARY_RETURN` | `1000` | Production: a military unit returns this share of its hammers' worth at the military price's excess over the allocator's, while the army is short of what it sees (thousandths). | own value (source not recorded) | `weights.rs` |
| `KIND_ROWS` | `&[ (0, 6, 150, "a tall capital races for wonders"), (0, 3, 150, "tall play feeds its ci...` | The own correction of the production head by kind of order (thousandths of the order's value), by style: (style, kind, value, why). | own value (source not recorded) | `weights.rs` |
| `POINTS_PER_PERSON` | `100` | A great person point is worth a technology's score points over this many points (a great person is about a technology). | own value (source not recorded) | `weights.rs` |
| `GOLDEN_AGE_PERCENT` | `20` | A golden age adds this percent of the empire's production and gold. | own value (source not recorded) | `weights.rs` |
| `COMBAT_HAMMERS` | `20` | A combat effect (a promotion, a combat or unit stat) is worth this many hammers per city at the production price, scaled by the military price. | own value (source not recorded) | `weights.rs` |
| `CITY_STATE_GOLD` | `1` | A city-state effect is worth this much gold a turn per city-state met, at the gold price. | own value (source not recorded) | `weights.rs` |
| `TILE_GOLD_SHARE` | `30` | A cost cut of `p` % is worth p % of the yield it saves: border growth (culture), tile purchase (gold, this share of the income), the policy penalty of a city (culture × cities over this). | own value (source not recorded) | `weights.rs` |
| `POLICY_PENALTY_CITIES` | `10` |  | own value (source not recorded) | `weights.rs` |
| `TOURISM_ON_CULTURE` | `100` | A tourism percent counts on the empire's culture a turn. | own value (source not recorded) | `weights.rs` |
| `UNKNOWN` | `0` | An effect the classifier has no model of is worth nothing. | own value (source not recorded) | `weights.rs` |
| `RADIUS` | `5` | Tiles around every unit of a theatre the window covers: a unit's strike this turn and the next (2 rounds of a 2-move unit) and the convolutions' radius 2 beyond. | own value (source not recorded) | `weights.rs` |
| `ENGAGE` | `9` | An own military unit within this many tiles of a hostile unit or city (or of its operation's target) is in a theatre. | own value (source not recorded) | `weights.rs` |
| `CITY_REACH` | `4` | A hostile city this close is contact for any unit (its fire, an assault next turn); farther, only for an operation's member. | own value (source not recorded) | `weights.rs` |
| `SIGHT` | `8` | The tiles the mind's sensor sees around a deciding unit of a theatre (`ClassicMind::sense_unit`): the whole theatre, so a fresh observation shows every member and every foe of it. | own value (source not recorded) | `weights.rs` |
| `FORCE_DIV` | `100` | Strength × health is divided by this for the force channels: a unit of strength 20 at full health is 0.2 (the design's «/100»). | own value (source not recorded) | `weights.rs` |
| `FOG_SHARE` | `120` | A tile in the fog near a hostile city or a foe seen holds this share of a typical enemy (`battlefield::FOG_PRIOR_PERCENT`, 12 %). | own value (source not recorded) | `weights.rs` |
| `FOG_REACH` | `3` | The fog prior reaches this many tiles from a hostile city or foe. | own value (source not recorded) | `weights.rs` |
| `HP_DIV` | `100` | Damage in hit points is divided by this for the danger, support and focus channels (the design's «/100»: a unit's whole health is 1.0). | own value (source not recorded) | `weights.rs` |
| `HEAL_DIV` | `20` | Healing per turn is divided by this (the design's «/20 hp»). | own value (source not recorded) | `weights.rs` |
| `PILLAGE` | `1000` | The value of a pillage on a hostile improvement (the design's «/10»): 1.0, the improvement's cost to its owner and our unit's heal of 25. | own value (source not recorded) | `weights.rs` |
| `SECOND_TURN_SLACK` | `1` | The 2-turn reach is the 1-turn reach widened by the unit's moves less one tile (a road or rough ground costs some of the second turn). | own value (source not recorded) | `weights.rs` |
| `CITY_FULL_HP` | `200` | A city's full health when the player does not know it (the rules' base, `CITY_BASE_HIT_POINTS`); ch11 is health / this. | own value (source not recorded) | `weights.rs` |
| `COST_DIV` | `3` | The movement cost channel: a tile costing 3 moves is 1.0. | own value (source not recorded) | `weights.rs` |
| `DEFENCE_DIV` | `50` | The defence channel: a bonus of 50 % is 1.0. | own value (source not recorded) | `weights.rs` |
| `CHANNELS` | `24` | Unused here; the count of the channels. | own value (source not recorded) | `weights.rs` |
| `MAPS` | `14` |  | own value (source not recorded) | `weights.rs` |
| `CHANNEL_NAMES` | `[ "passable", "move_cost", "defence", "blocks_sight", "river", "territory", "ours", "ou...` | Kept for the page: the design's 24 channels by name. | own value (source not recorded) | `weights.rs` |
| `MAP_NAMES` | `["ours2", "theirs2", "balance", "front", "front_enemy", "choke", "gap", "siege", "cut_o...` |  | own value (source not recorded) | `weights.rs` |
| `DENSITY_CENTRE` | `1000` | V1: the centre of the density kernel: the force on the tile itself. | own value (source not recorded) | `weights.rs` |
| `DENSITY_NEIGHBOUR` | `500` | V1: a neighbour's force counts half: it can come, but not at once. | own value (source not recorded) | `weights.rs` |
| `DENSITY_DIV` | `4` | V1: two passes (radius 2 ≈ a turn's strike) and divided by 4, so a lone unit's own tile reads about its own force. | own value (source not recorded) | `weights.rs` |
| `BALANCE_GAIN` | `2000` | V1: balance = tanh(2 · (ours2 − theirs2)): a lead of 0.5 (a full strength-25 unit more nearby) is 0.76, almost sure. | own value (source not recorded) | `weights.rs` |
| `FRONT_GAIN` | `4000` | V2: the front = σ(4 · min(our force near, their reach and zone of control near) − 0.5): both sides touch the tile. | own value (source not recorded) | `weights.rs` |
| `FRONT_BIAS` | `500` |  | own value (source not recorded) | `weights.rs` |
| `CHOKE_OPEN` | `3500` | V3: a choke has at most 3 open neighbours: σ((3.5 − open) · 2). | own value (source not recorded) | `weights.rs` |
| `CHOKE_OPEN_GAIN` | `2000` |  | own value (source not recorded) | `weights.rs` |
| `CHOKE_FLOW` | `200` | V3: and the enemy's 2-turn reach flows through it: σ((flow − 0.2) · 8) (flow = the share of its neighbours they reach). | own value (source not recorded) | `weights.rs` |
| `CHOKE_FLOW_GAIN` | `8000` |  | own value (source not recorded) | `weights.rs` |
| `SIEGE_RING` | `2000` | V4: a siege forms with 2 or more of ours at the walls: σ(ring − 2). | own value (source not recorded) | `weights.rs` |
| `SIEGE_FIRE_BIAS` | `200` | V4: and the fire nearly enough: σ((focus + 0.2) · 5), focus = (our summed blows − its health) / 100. | own value (source not recorded) | `weights.rs` |
| `SIEGE_FIRE_GAIN` | `5000` |  | own value (source not recorded) | `weights.rs` |
| `CUT_ALLIES` | `1000` | V5: a foe is cut off with no ally next to it: σ((1 − allies) · 3) … | own value (source not recorded) | `weights.rs` |
| `CUT_GAIN` | `3000` |  | own value (source not recorded) | `weights.rs` |
| `CUT_OURS` | `1500` | V5: … and 2 or more of ours next to it: σ((ours − 1.5) · 3). | own value (source not recorded) | `weights.rs` |
| `SAFE_DANGER` | `100` | V6: safe where the danger is under 0.1 of a unit's health: 1 − σ((danger − 0.1) · 10). | own value (source not recorded) | `weights.rs` |
| `SAFE_GAIN` | `10000` |  | own value (source not recorded) | `weights.rs` |
| `SAFE_HEAL_BASE` | `500` | V6: (0.5 + heal): a safe tile is worth half without healing. | own value (source not recorded) | `weights.rs` |
| `SAFE_SPREAD` | `800` | V6: the rear spreads one step losing 20 %. | own value (source not recorded) | `weights.rs` |
| `COHESION_NEIGHBOUR` | `600` | V7: a group member's neighbours count 0.6 (two passes, radius 2) … | own value (source not recorded) | `weights.rs` |
| `COHESION_DIV` | `5` | V7: … divided by 5: a tile with four members around reads about 1. | own value (source not recorded) | `weights.rs` |
| `LONELY_AT` | `500` | V7: lonely = σ((0.5 − cohesion) · 4). | own value (source not recorded) | `weights.rs` |
| `LONELY_GAIN` | `4000` |  | own value (source not recorded) | `weights.rs` |
| `WOUNDED_AT` | `500` | Wounded: σ((0.5 − hp) / 0.1): below half health a unit's behaviour changes. | own value (source not recorded) | `weights.rs` |
| `WOUNDED_WIDTH` | `100` |  | own value (source not recorded) | `weights.rs` |
| `FIRE_AT` | `800` | Under fire: σ((D_u / hp − 0.8) / 0.1): the enemy may take 80 % of what is left. | own value (source not recorded) | `weights.rs` |
| `FIRE_WIDTH` | `100` |  | own value (source not recorded) | `weights.rs` |
| `CAPTURER_AT` | `-100` | Capturer: a melee unit at a city that is about to fall: σ((focus(g) + 0.1) / 0.05). | own value (source not recorded) | `weights.rs` |
| `CAPTURER_WIDTH` | `50` |  | own value (source not recorded) | `weights.rs` |
| `KILL_CUT` | `500` | m2: a kill of a cut-off foe is worth half more (V5). | own value (source not recorded) | `weights.rs` |
| `SURVIVE_AT` | `500` | m4: danger above half the unit's health starts to cost. | own value (source not recorded) | `weights.rs` |
| `DOOM` | `10_000` | m4: ending the turn where the danger reaches the unit's health costs 10, unless the move takes a city or kills (the rule of the design). | own value (source not recorded) | `weights.rs` |
| `TOWARD` | `1000` | m5: a turn closer to the target is worth 1.0. | own value (source not recorded) | `weights.rs` |
| `LONELY` | `500` | m6: cohesion − 0.5 · lonely. | own value (source not recorded) | `weights.rs` |
| `FIRE_SAFE_AT` | `500` | m8: a firing position loses its worth as the danger there passes half the unit's health: 1 − σ((D_u − 0.5·hp) / 10 hp). | own value (source not recorded) | `weights.rs` |
| `FIRE_SAFE_WIDTH_HP` | `10` |  | own value (source not recorded) | `weights.rs` |
| `DEFER` | `2000` | m11: deferring a capturer while others still shoot is worth 2. | own value (source not recorded) | `weights.rs` |
| `PILLAGE_BASE` | `500` | m12: a pillage is worth (0.5 + wounded) of its value: a wounded unit heals by it. | own value (source not recorded) | `weights.rs` |
| `COVER` | `1000` | m13: a melee unit covering a weak ally: the blows it keeps off, in the ally's health, times the unit price. | own value (source not recorded) | `weights.rs` |
| `ASSIGNED` | `2500` | m14 (TurnPlan): a candidate that executes the commander's assignment for the unit is worth 2.5 — more than a kill elsewhere (m2 in war: 1.5 · 1.5), less than the doom rule (10): the plan leads, safety overrides. | own value (source not recorded) | `weights.rs` |
| `ASSIGNED_STEP` | `800` | A move toward the assigned strike's tile that keeps the strike for this turn is worth this share of the assignment. | own value (source not recorded) | `weights.rs` |
| `MOTIVES` | `["take", "kill", "damage", "survive", "toward", "formation", "wall", "fire", "choke", "...` | The motives' names in order (m1…m13). | own value (source not recorded) | `weights.rs` |
| `MELEE_GATHER` | `0` |  | own value (source not recorded) | `weights.rs` |
| `MELEE_APPROACH` | `1` |  | own value (source not recorded) | `weights.rs` |
| `MELEE_ASSAULT` | `2` |  | own value (source not recorded) | `weights.rs` |
| `RANGED_ASSAULT` | `3` |  | own value (source not recorded) | `weights.rs` |
| `MOUNTED_ASSAULT` | `4` |  | own value (source not recorded) | `weights.rs` |
| `WOUNDED` | `5` |  | own value (source not recorded) | `weights.rs` |
| `DEFENCE` | `6` |  | own value (source not recorded) | `weights.rs` |
| `RETREAT` | `7` |  | own value (source not recorded) | `weights.rs` |
| `CIVILIAN` | `8` |  | own value (source not recorded) | `weights.rs` |
| `RANGED_GATHER` | `9` |  | own value (source not recorded) | `weights.rs` |
| `RANGED_APPROACH` | `10` |  | own value (source not recorded) | `weights.rs` |
| `MOUNTED_GATHER` | `11` |  | own value (source not recorded) | `weights.rs` |
| `MOUNTED_APPROACH` | `12` |  | own value (source not recorded) | `weights.rs` |
| `TABLE` | `[ Row { name: "melee · gather", w: [0, 500, 200, 2000, 500, 1500, 0, 0, 500, 500, 0, 0,...` | The output layer (design section 5): the motives' weights by role and phase. | own value (source not recorded) | `weights.rs` |
| `POSTURES` | `[ PostureRow { name: "peace", kill: 1000, city: 3000, damage: 100, unit: 2000, risk: 15...` | By posture: peace, defence, prepare, war (`strategy_net::weights::POSTURES`). | own value (source not recorded) | `weights.rs` |
| `CITY_BASE_HAMMERS` | `150` | A city's worth in hammers for the values (V): its buildings and land (base) and each citizen. | own value (source not recorded) | `weights.rs` |
| `CITY_HAMMERS_PER_POP` | `50` |  | own value (source not recorded) | `weights.rs` |
| `HAMMERS_PER_POINT` | `100` | Hammers per value point: V is in hundreds of hammers (a 100-hammer unit is 1.0). | own value (source not recorded) | `weights.rs` |
| `WOUND_SHARE` | `500` | A wound removes half the threat in proportion: f = P·V + (1 − P) · 0.5 · V · d / hp. | own value (source not recorded) | `weights.rs` |
| `NEAR_CERTAIN` | `900` | Fire stops being added to a target once its kill chance reaches this: the next shooter is kept in reserve (for the fast update after a miss, or for another target). | own value (source not recorded) | `weights.rs` |
| `REPLY_ROUNDS` | `2` | The reply check runs twice, its changes damped 0.5 then 0.25. | own value (source not recorded) | `weights.rs` |
| `REPLY_DAMPING` | `[500, 250]` |  | own value (source not recorded) | `weights.rs` |
| `DEATH_PENALTY` | `1000` | A unit whose gain is less than its price times its chance to die gets this penalty share of its value on that variant. | own value (source not recorded) | `weights.rs` |
| `WAR_WEIGHT` | `1000` | Players' danger weights by relation: at war 1, others 0 (peace keeps their units off ours), a war on a third × 0.5. | own value (source not recorded) | `weights.rs` |
| `BUSY_WEIGHT` | `500` |  | own value (source not recorded) | `weights.rs` |
| `CIVILIAN_DANGER` | `3000` | A civilian's danger counts three times (design 5d): it cannot fight back. | own value (source not recorded) | `weights.rs` |
| `STRIKE_TILES` | `4` | Most strike candidates the allocation keeps per unit (its best tiles). | own value (source not recorded) | `weights.rs` |
| `SPREAD_PERCENT` | `33` | The spread of a blow around its expected damage when the preview gives none (a shot of 30 lands 20–40). | own value (source not recorded) | `weights.rs` |
| `MAX_OPS` | `8` | Most operations at once. | own value (source not recorded) | `weights.rs` |
| `READY_TURNS` | `2` | Members within this many turns of the target count for readiness. | own value (source not recorded) | `weights.rs` |
| `READY` | `1000` | Readiness r ≥ 1 moves the group from gathering to the approach. | own value (source not recorded) | `weights.rs` |
| `GATHER_DEADLINE` | `10` | After this many rounds of gathering the group sets out anyway. | own value (source not recorded) | `weights.rs` |
| `ASSAULT_TURNS` | `2` | Within this many turns of the target the approach is an assault. | own value (source not recorded) | `weights.rs` |
| `TAKE_AT` | `1200` | P(take) = σ((ρ − 1.2) / 0.2): an edge of 1.2 in the Lanchester ratio is even. | own value (source not recorded) | `weights.rs` |
| `TAKE_WIDTH` | `200` |  | own value (source not recorded) | `weights.rs` |
| `WAIT_COST` | `300` | Waiting a turn costs 0.3 of a city's price. | own value (source not recorded) | `weights.rs` |
| `RETREAT_RATIO` | `1500` | The assault turns to retreat when the losses pass 1.5 × the gain. | own value (source not recorded) | `weights.rs` |
| `HYSTERESIS` | `1150` | A change of option needs a lead of 15 %. | own value (source not recorded) | `weights.rs` |
| `LEAD` | `2` | Approaching out of contact, a member more than this many tiles ahead of the group's median waits (no trickle into the enemy's reach). | own value (source not recorded) | `weights.rs` |
| `RETREAT_ROUNDS` | `4` | Most rounds a retreat lasts before the operation ends. | own value (source not recorded) | `weights.rs` |
| `DEFEND_AT` | `300` | A defence operation for a city whose danger is at least this share of its defenders' health. | own value (source not recorded) | `weights.rs` |
| `DEFEND_RADIUS` | `6` | Members of a defence operation come from this many tiles. | own value (source not recorded) | `weights.rs` |
| `CITY_DEFENCE_PERCENT` | `25` | The city's defence bonus in N, percent. | own value (source not recorded) | `weights.rs` |
| `MAX_SIEGE_TURNS` | `8` | The most turns to take a city the losses are counted over. | own value (source not recorded) | `weights.rs` |
| `ORDER_SHORT` | `800` | Orders: a defence operation short of this share of the threat's force orders a defender. | own value (source not recorded) | `weights.rs` |
| `ORDER_VALUE` | `600` | An order's value per hammer of the threatened city's worth at risk. | own value (source not recorded) | `weights.rs` |

### `wishes.rs`

1 constants (own value (source not recorded) 1). Files: `classic/crates/classic-rules/src/ai/strategy_net/wishes.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `NONE` | `Parts { threat: false, tall_first: false }` | Every variant but `strategist-x` and `strategist-x-threat`. | own value (source not recorded) | `wishes.rs` |

### `wonder_chance.rs`

4 constants (own value (source not recorded) 4). Files: `classic/crates/classic-rules/src/ai/wonder_chance.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `TURNS` | `[6, 10, 14, 19, 25, 32, 42, 1000000]` | Upper bound (inclusive) of each row, in turns at Standard speed. | own value (source not recorded) | `wonder_chance.rs` |
| `POPULATION` | `[0, 3, 7, 1000000]` | Upper bound (inclusive) of each column, in citizens of the largest sighted rival city (0: none sighted). | own value (source not recorded) | `wonder_chance.rs` |
| `FITTED_AGE` | `15` | Age widening, rounds per citizen. | own value (source not recorded) | `wonder_chance.rs` |
| `CHANCE` | `[ [[80, 80, 80, 80], [80, 76, 68, 68], [80, 76, 68, 68]], [[66, 66, 66, 66], [66, 66, 5...` | By turns row, then the seat's place (1, 2, 3 or worse), then the rival's size column. | own value (source not recorded) | `wonder_chance.rs` |

### `wonder_race.rs`

1 constants (own value (source not recorded) 1). Files: `classic/crates/classic-rules/src/ai/wonder_race.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `AGE_ROUNDS_PER_CITIZEN` | `crate::ai::wonder_chance::FITTED_AGE` | A sighted city is taken to have grown by a citizen in this many rounds since it was seen (the sighting's age widens the estimate of the rival's production; fitted with the table: the step where the won rate of the orders by the sighted population is the same at every age). | own value (source not recorded) | `wonder_race.rs` |

## Data catalogs

Generated from the JSON catalogs in `classic/docs/classic/` (the input of the content importer) and, for difficulty levels and projects, from the compiled files in `classic/assets/rules/classic/`. The provenance column counts what each entry's `provenance` field says.

### Overview

| Catalog | File | Entries | Unciv | own | own, unverified | not recorded |
|---|---|---|---|---|---|---|
| Units | `units.json` | 82 | 74 | 8 | 0 | 0 |
| Unique units | `unique_units.json` | 59 | 47 | 12 | 0 | 0 |
| Buildings | `buildings.json` | 62 | 55 | 7 | 0 | 0 |
| Unique buildings | `unique_buildings.json` | 20 | 15 | 5 | 0 | 0 |
| Wonders | `wonders.json` | 67 | 50 | 17 | 0 | 0 |
| Technologies | `techs.json` | 81 | 80 | 1 | 0 | 0 |
| Policies | `policies.json` | 111 | 51 | 60 | 0 | 0 |
| Beliefs | `beliefs.json` | 69 | 55 | 14 | 0 | 0 |
| Promotions | `promotions.json` | 214 | 140 | 44 | 30 | 0 |
| Resources | `resources.json` | 42 | 35 | 7 | 0 | 0 |
| Improvements | `improvements.json` | 29 | 23 | 6 | 0 | 0 |
| Terrain | `terrain.json` | 9 | 9 | 0 | 0 | 0 |
| Terrain features | `features.json` | 8 | 8 | 0 | 0 | 0 |
| Natural wonders | `natural_wonders.json` | 17 | 11 | 6 | 0 | 0 |
| Eras | `eras.json` | 8 | 8 | 0 | 0 | 0 |
| Great people | `great_people.json` | 9 | 7 | 2 | 0 | 0 |
| Nations | `nations.json` | 43 | 35 | 8 | 0 | 0 |
| City-states | `city_states.json` | 58 | 42 | 16 | 0 | 0 |
| Policy branches | `policies.json` (branches) | 12 | 11 | 1 | 0 | 0 |
| Religions | `beliefs.json` (religions) | 13 | 10 | 3 | 0 | 0 |
| Specialists | `great_people.json` (specialists) | 7 | 4 | 0 | 3 | 0 |
| City-state types | `city_states.json` (types) | 5 | 5 | 0 | 0 | 0 |

### Units

| Unit | Role | Era | Strength | Ranged | Range | Moves | Cost | Technology | Provenance |
|---|---|---|---|---|---|---|---|---|---|
| Settler | civilian | ancient |  |  |  | 2 | 106 |  | Unciv |
| Worker | civilian | ancient |  |  |  | 2 | 70 |  | Unciv |
| Work Boat | civilian | ancient |  |  |  | 4 | 30 | Sailing | own |
| Great Artist | great-person | ancient |  |  |  | 2 | -1 |  | Unciv |
| Great Scientist | great-person | ancient |  |  |  | 2 | -1 |  | Unciv |
| Great Merchant | great-person | ancient |  |  |  | 2 | -1 |  | Unciv |
| Great Engineer | great-person | ancient |  |  |  | 2 | -1 |  | Unciv |
| Great General | great-person | ancient |  |  |  | 2 | -1 |  | Unciv |
| SS Stasis Chamber | spaceship-part | information |  |  |  | 2 | 750 | Nanotechnology | Unciv |
| SS Engine | spaceship-part | information |  |  |  | 2 | 750 | Particle Physics | Unciv |
| SS Cockpit | spaceship-part | information |  |  |  | 2 | 750 | Satellites | Unciv |
| SS Booster | spaceship-part | information |  |  |  | 2 | 750 | Advanced Ballistics | Unciv |
| Missile Cruiser | navalranged | information | 80 | 100 | 3 | 7 | 425 | Robotics | Unciv |
| Nuclear Submarine | submarine | information | 50 | 85 | 3 | 6 | 425 | Telecommunications | Unciv |
| Carrier | carrier | modern | 40 |  |  | 5 | 375 | Electronics | Unciv |
| Battleship | navalranged | modern | 55 | 55 | 3 | 5 | 375 | Electronics | Unciv |
| Submarine | submarine | modern | 35 | 60 | 2 | 5 | 325 | Refrigeration | Unciv |
| Destroyer | navalmelee | modern | 55 |  |  | 6 | 375 | Combustion | Unciv |
| Ironclad | navalmelee | industrial | 45 |  |  | 3 | 250 | Steam Power | Unciv |
| Frigate | navalranged | renaissance | 25 | 28 | 2 | 5 | 185 | Navigation | Unciv |
| Caravel | navalmelee | renaissance | 20 |  |  | 4 | 120 | Astronomy | Unciv |
| Trireme | navalmelee | ancient | 10 |  |  | 4 | 45 | Sailing | Unciv |
| Giant Death Robot | armor | information | 150 |  |  | 5 | 425 | Nuclear Fusion | Unciv |
| Nuclear Missile | missile | information |  |  | 12 | 2 | 1000 | Advanced Ballistics | Unciv |
| Stealth Bomber | bomber | information |  | 85 | 20 | 2 | 425 | Stealth | Unciv |
| Jet Fighter | fighter | information |  | 75 | 10 | 2 | 425 | Lasers | Unciv |
| Guided Missile | missile | information |  | 60 | 8 | 2 | 150 | Advanced Ballistics | Unciv |
| Modern Armor | armor | information | 100 |  |  | 5 | 425 | Lasers | Unciv |
| Helicopter Gunship | helicopter | atomic | 60 |  |  | 6 | 425 | Computers | Unciv |
| Mobile SAM | gun | atomic | 65 |  | 2 | 3 | 425 | Rocketry | Unciv |
| Rocket Artillery | siege | atomic | 45 | 60 | 3 | 2 | 425 | Rocketry | Unciv |
| Mechanized Infantry | gun | information | 90 |  |  | 3 | 375 | Mobile Tactics | Unciv |
| Atomic Bomb | missile | atomic |  |  | 10 | 2 | 600 | Nuclear Fission | Unciv |
| Bomber | bomber | atomic |  | 65 | 10 | 2 | 375 | Radar | Unciv |
| Fighter | fighter | atomic |  | 45 | 8 | 2 | 375 | Radar | Unciv |
| Paratrooper | gun | atomic | 65 |  |  | 2 | 375 | Radar | Unciv |
| Tank | armor | atomic | 70 |  |  | 5 | 375 | Combined Arms | Unciv |
| Artillery | siege | industrial | 21 | 28 | 3 | 2 | 320 | Dynamite | Unciv |
| Anti-Aircraft Gun | gun | modern | 50 |  | 2 | 2 | 375 | Ballistics | Unciv |
| Anti-Tank Gun | gun | atomic | 50 |  |  | 2 | 300 | Combined Arms | Unciv |
| Infantry | gun | modern | 70 |  |  | 2 | 375 | Plastics | Unciv |
| Foreign Legion | gun | modern | 50 |  |  | 2 | 320 | Replaceable Parts | Unciv |
| Cavalry | mounted | industrial | 34 |  |  | 4 | 225 | Military Science | Unciv |
| Rifleman | gun | industrial | 34 |  |  | 2 | 225 | Rifling | Unciv |
| Lancer | mounted | renaissance | 25 |  |  | 4 | 185 | Metallurgy | Unciv |
| Cannon | siege | renaissance | 14 | 20 | 2 | 2 | 185 | Chemistry | Unciv |
| Musketman | gun | renaissance | 24 |  |  | 2 | 150 | Gunpowder | Unciv |
| Longswordsman | melee | medieval | 21 |  |  | 2 | 120 | Steel | Unciv |
| Trebuchet | siege | medieval | 12 | 14 | 2 | 2 | 120 | Physics | Unciv |
| Knight | mounted | medieval | 20 |  |  | 4 | 120 | Chivalry | Unciv |
| Crossbowman | archer | medieval | 13 | 18 | 2 | 2 | 120 | Machinery | Unciv |
| Pikeman | melee | medieval | 16 |  |  | 2 | 90 | Civil Service | Unciv |
| Landsknecht | melee | medieval | 16 |  |  | 2 | 45 | Civil Service | Unciv |
| Catapult | siege | classical | 7 | 8 | 2 | 2 | 75 | Mathematics | Unciv |
| Horseman | mounted | classical | 12 |  |  | 4 | 75 | Horseback Riding | Unciv |
| Swordsman | melee | classical | 14 |  |  | 2 | 75 | Iron Working | Unciv |
| Chariot Archer | archer | ancient | 6 | 10 | 2 | 4 | 56 | The Wheel | Unciv |
| Spearman | melee | ancient | 11 |  |  | 2 | 56 | Bronze Working | Unciv |
| Archer | archer | ancient | 5 | 7 | 2 | 2 | 40 | Archery | Unciv |
| Scout | recon | ancient | 5 |  |  | 2 | 25 |  | Unciv |
| Warrior | melee | ancient | 8 |  |  | 2 | 40 |  | Unciv |
| Great Prophet | great-person | ancient |  |  |  | 2 | -1 |  | Unciv |
| Missionary | religious | ancient |  |  |  | 4 | -1 |  | Unciv |
| Composite Bowman | archer | classical | 7 | 11 | 2 | 2 | 75 | Construction | Unciv |
| Galleass | navalranged | medieval | 16 | 17 | 2 | 3 | 100 | Compass | Unciv |
| Great War Infantry | gun | modern | 50 |  |  | 2 | 320 | Replaceable Parts | Unciv |
| Marine | gun | atomic | 65 |  |  | 2 | 400 | Penicillin | Unciv |
| Triplane | fighter | modern |  | 35 | 5 | 2 | 325 | Flight | Unciv |
| Great War Bomber | bomber | modern |  | 50 | 6 | 2 | 325 | Flight | Unciv |
| Landship | armor | modern | 60 |  |  | 4 | 350 | Combustion | Unciv |
| Machine Gun | archer | modern | 60 | 60 | 1 | 2 | 350 | Ballistics | Unciv |
| Privateer | navalmelee | renaissance | 25 |  |  | 5 | 150 | Navigation | Unciv |
| Inquisitor | religious | ancient |  |  |  | 3 | -1 |  | Unciv |
| Great Admiral | great-person | ancient |  |  |  | 4 | -1 |  | Unciv |
| Gatling Gun | archer | industrial | 30 | 30 | 1 | 2 | 225 | Industrialization | Unciv |
| Cargo Ship | civilian | ancient |  |  |  | 4 | 90 | Sailing | own |
| Caravan | civilian | ancient |  |  |  | 2 | 70 | Animal Husbandry | own |
| Great Writer | great-person | ancient |  |  |  | 2 | -1 |  | own |
| Great Musician | great-person | ancient |  |  |  | 2 | -1 |  | own |
| Archaeologist | civilian | industrial |  |  |  | 4 | 175 | Archaeology | own |
| Bazooka | archer | atomic | 85 | 85 | 1 | 2 | 375 | Nuclear Fission | own |
| XCOM Squad | gun | information | 100 |  |  | 2 | 400 | Nanotechnology | own |

### Unique units

| Unit | Role | Era | Strength | Ranged | Range | Moves | Cost | Technology | Provenance |
|---|---|---|---|---|---|---|---|---|---|
| Ship of the Line | navalranged | renaissance | 30 | 35 | 2 | 5 | 185 | Navigation | Unciv |
| B17 | bomber | atomic |  | 70 | 10 | 2 | 375 | Radar | Unciv |
| Zero | fighter | atomic |  | 45 | 8 | 2 | 375 | Radar | Unciv |
| Panzer | armor | atomic | 80 |  |  | 6 | 375 | Combined Arms | Unciv |
| Cossack | mounted | industrial | 34 |  |  | 4 | 225 | Military Science | Unciv |
| Sipahi | mounted | renaissance | 25 |  |  | 5 | 185 | Metallurgy | Unciv |
| Minuteman | gun | renaissance | 24 |  |  | 2 | 150 | Gunpowder | Unciv |
| Musketeer | gun | renaissance | 28 |  |  | 2 | 150 | Gunpowder | Unciv |
| Janissary | gun | renaissance | 24 |  |  | 2 | 150 | Gunpowder | Unciv |
| Samurai | melee | medieval | 21 |  |  | 2 | 120 | Steel | Unciv |
| Camel Archer | archer | medieval | 17 | 21 | 2 | 4 | 120 | Chivalry | Unciv |
| Naresuan's Elephant | mounted | medieval | 25 |  |  | 3 | 120 | Chivalry | Unciv |
| Mandekalu Cavalry | mounted | medieval | 20 |  |  | 4 | 110 | Chivalry | Unciv |
| Chu-Ko-Nu | archer | medieval | 13 | 14 | 2 | 2 | 120 | Machinery | Unciv |
| Longbowman | archer | medieval | 13 | 18 | 2 | 2 | 120 | Machinery | Unciv |
| Ballista | siege | classical | 8 | 10 | 2 | 2 | 75 | Mathematics | Unciv |
| Companion Cavalry | mounted | classical | 14 |  |  | 5 | 75 | Horseback Riding | Unciv |
| Mohawk Warrior | melee | classical | 14 |  |  | 2 | 75 | Iron Working | Unciv |
| Legion | melee | classical | 17 |  |  | 2 | 75 | Iron Working | Unciv |
| War Chariot | archer | ancient | 6 | 10 | 2 | 5 | 56 | The Wheel | Unciv |
| War Elephant | archer | ancient | 9 | 11 | 2 | 3 | 70 | The Wheel | Unciv |
| Hoplite | melee | ancient | 13 |  |  | 2 | 56 | Bronze Working | Unciv |
| Immortal | melee | ancient | 12 |  |  | 2 | 56 | Bronze Working | own |
| Jaguar | melee | ancient | 8 |  |  | 2 | 40 |  | Unciv |
| Mehal Sefari | gun | industrial | 34 |  |  | 2 | 200 | Rifling | Unciv |
| Impi | melee | medieval | 16 |  |  | 2 | 90 | Civil Service | own |
| Merchant of Venice | great-person | ancient |  |  |  | 2 | -1 |  | own |
| Winged Hussar | mounted | renaissance | 28 |  |  | 5 | 185 | Metallurgy | own |
| Siege Tower | melee | classical | 12 |  |  | 2 | 75 | Mathematics | own |
| Pracinha | gun | modern | 70 |  |  | 2 | 375 | Plastics | own |
| Nau | navalmelee | renaissance | 20 |  |  | 5 | 120 | Astronomy | own |
| Berber Cavalry | mounted | industrial | 34 |  |  | 4 | 225 | Military Science | own |
| Great Galleass | navalranged | medieval | 18 | 20 | 2 | 3 | 110 | Compass | own |
| Pathfinder | recon | ancient | 8 |  |  | 2 | 45 |  | own |
| Comanche Riders | mounted | industrial | 34 |  |  | 5 | 200 | Military Science | own |
| Kris Swordsman | melee | classical | 14 |  |  | 2 | 75 | Iron Working | own |
| Bowman | archer | ancient | 7 | 9 | 2 | 2 | 40 | Archery | Unciv |
| Keshik | archer | medieval | 15 | 16 | 2 | 5 | 120 | Chivalry | Unciv |
| Khan | great-person | ancient |  |  |  | 5 | -1 |  | Unciv |
| Slinger | archer | ancient | 4 | 7 | 2 | 2 | 40 | Archery | Unciv |
| Tercio | melee | renaissance | 26 |  |  | 2 | 160 | Gunpowder | Unciv |
| Conquistador | mounted | medieval | 20 |  |  | 4 | 135 | Chivalry | Unciv |
| Maori Warrior | melee | ancient | 8 |  |  | 2 | 40 |  | Unciv |
| Berserker | melee | medieval | 21 |  |  | 3 | 120 | Metal Casting | Unciv |
| Norwegian Ski Infantry | gun | industrial | 34 |  |  | 2 | 225 | Rifling | Unciv |
| Turtle Ship | navalmelee | renaissance | 36 |  |  | 4 | 120 | Astronomy | Unciv |
| Hwach'a | siege | medieval | 11 | 26 | 2 | 2 | 120 | Physics | Unciv |
| Pictish Warrior | melee | ancient | 11 |  |  | 2 | 56 | Bronze Working | Unciv |
| Atlatlist | archer | ancient | 5 | 7 | 2 | 2 | 36 | Agriculture | Unciv |
| Dromon | navalranged | ancient | 8 | 10 | 2 | 4 | 56 | Sailing | Unciv |
| Cataphract | mounted | classical | 15 |  |  | 3 | 75 | Horseback Riding | Unciv |
| African Forest Elephant | mounted | classical | 14 |  |  | 3 | 100 | Horseback Riding | Unciv |
| Quinquereme | navalmelee | ancient | 13 |  |  | 4 | 45 | Sailing | Unciv |
| Horse Archer | archer | ancient | 7 | 10 | 2 | 4 | 56 | The Wheel | Unciv |
| Hussar | mounted | industrial | 34 |  |  | 5 | 225 | Military Science | Unciv |
| Carolean | gun | industrial | 34 |  |  | 2 | 225 | Rifling | Unciv |
| Hakkapeliitta | mounted | renaissance | 25 |  |  | 4 | 185 | Metallurgy | Unciv |
| Battering Ram | melee | ancient | 10 |  |  | 2 | 75 | Bronze Working | Unciv |
| Sea Beggar | navalmelee | renaissance | 25 |  |  | 5 | 150 | Navigation | Unciv |

### Buildings

| Building | Kind | Era | Cost | Upkeep | Technology | Provenance |
|---|---|---|---|---|---|---|
| Courthouse | regular | classical | 100 | 4 | Mathematics | Unciv |
| Seaport | regular | renaissance | 250 | 2 | Navigation | Unciv |
| Stable | regular | classical | 100 | 1 | Horseback Riding | Unciv |
| Water Mill | regular | ancient | 75 | 2 | The Wheel | Unciv |
| Circus | regular | ancient | 75 | 0 | Trapping | Unciv |
| Forge | regular | medieval | 120 | 1 | Metal Casting | Unciv |
| Windmill | regular | renaissance | 250 | 2 | Economics | Unciv |
| Hydro Plant | regular | industrial | 360 | 3 | Electricity | Unciv |
| Solar Plant | regular | atomic | 500 | 3 | Ecology | Unciv |
| Mint | regular | classical | 100 | 0 | Currency | Unciv |
| Observatory | regular | renaissance | 200 | 0 | Astronomy | Unciv |
| Monastery | religious | ancient | -1 | 0 |  | Unciv |
| Garden | regular | medieval | 120 | 1 | Theology | Unciv |
| Lighthouse | regular | classical | 75 | 1 | Optics | Unciv |
| Harbor | regular | medieval | 120 | 3 | Compass | Unciv |
| Colosseum | regular | classical | 100 | 1 | Construction | Unciv |
| Zoo | regular | renaissance | 200 | 2 | Printing Press | own |
| Stadium | regular | modern | 500 | 2 | Refrigeration | Unciv |
| Monument | regular | ancient | 40 | 1 |  | Unciv |
| Temple | regular | classical | 100 | 2 | Philosophy | Unciv |
| Opera House | regular | renaissance | 200 | 2 | Acoustics | Unciv |
| Museum | regular | industrial | 300 | 3 | Archaeology | Unciv |
| Broadcast Tower | regular | modern | 500 | 3 | Radio | Unciv |
| Barracks | regular | ancient | 75 | 1 | Bronze Working | Unciv |
| Armory | regular | medieval | 160 | 1 | Steel | Unciv |
| Military Academy | regular | industrial | 300 | 1 | Military Science | Unciv |
| Arsenal | regular | renaissance | 300 | 0 | Metallurgy | Unciv |
| Walls | regular | ancient | 75 | 0 | Masonry | Unciv |
| Castle | regular | medieval | 160 | 0 | Chivalry | Unciv |
| Military Base | regular | modern | 500 | 0 | Replaceable Parts | Unciv |
| Granary | regular | ancient | 60 | 1 | Pottery | Unciv |
| Hospital | regular | industrial | 360 | 2 | Biology | Unciv |
| Medical Lab | regular | atomic | 500 | 3 | Penicillin | Unciv |
| Workshop | regular | medieval | 120 | 2 | Metal Casting | Unciv |
| Factory | regular | industrial | 360 | 3 | Industrialization | Unciv |
| Nuclear Plant | regular | atomic | 500 | 3 | Nuclear Fission | Unciv |
| Spaceship Factory | regular | information | 360 | 3 | Robotics | Unciv |
| Market | regular | classical | 100 | 0 | Currency | Unciv |
| Bank | regular | renaissance | 200 | 0 | Banking | Unciv |
| Stock Exchange | regular | industrial | 360 | 0 | Electricity | Unciv |
| Library | regular | ancient | 75 | 1 | Writing | Unciv |
| University | regular | medieval | 160 | 2 | Education | Unciv |
| Public School | regular | industrial | 300 | 3 | Scientific Theory | Unciv |
| Research Lab | regular | modern | 500 | 3 | Plastics | Unciv |
| Palace | palace | ancient | 0 | 0 |  | Unciv |
| Aqueduct | regular | classical | 100 | 1 | Engineering | Unciv |
| Stone Works | regular | ancient | 75 | 1 | Calendar | Unciv |
| Amphitheater | regular | classical | 100 | 2 | Drama and Poetry | Unciv |
| Shrine | regular | ancient | 40 | 1 | Pottery | Unciv |
| Recycling Center | regular | atomic | 500 | 3 | Ecology | Unciv |
| Bomb Shelter | regular | information | 300 | 1 | Telecommunications | Unciv |
| Constabulary | regular | renaissance | 160 | 1 | Banking | Unciv |
| Police Station | regular | industrial | 300 | 1 | Electricity | Unciv |
| Cathedral | religious | ancient | -1 | 0 |  | Unciv |
| Mosque | religious | ancient | -1 | 0 |  | Unciv |
| Pagoda | religious | ancient | -1 | 0 |  | Unciv |
| Writers' Guild | guild | classical | 100 | 1 | Drama and Poetry | own |
| Artists' Guild | guild | medieval | 150 | 1 | Guilds | own |
| Musicians' Guild | guild | renaissance | 200 | 1 | Acoustics | own |
| Hotel | regular | modern | 500 | 0 | Refrigeration | own |
| Caravansary | regular | classical | 120 | 0 | Horseback Riding | own |
| Airport | regular | atomic | 400 | 5 | Radar | own |

### Unique buildings

| Building | Kind | Era | Cost | Upkeep | Technology | Provenance |
|---|---|---|---|---|---|---|
| Floating Gardens | unique | ancient | 75 | 1 | The Wheel | Unciv |
| Mughal Fort | unique | medieval | 150 | 0 | Chivalry | Unciv |
| Krepost | unique | ancient | 75 | 1 | Bronze Working | Unciv |
| Longhouse | unique | medieval | 100 | 2 | Metal Casting | Unciv |
| Bazaar | unique | classical | 100 | 0 | Currency | Unciv |
| Satrap's Court | unique | renaissance | 200 | 0 | Banking | Unciv |
| Paper Maker | unique | ancient | 75 | 0 | Writing | Unciv |
| Wat | unique | medieval | 160 | 2 | Education | Unciv |
| Mud Pyramid Mosque | unique | classical | 100 | 0 | Philosophy | Unciv |
| Burial Tomb | unique | classical | 100 | 0 | Philosophy | Unciv |
| Stele | unique | ancient | 40 | 1 |  | Unciv |
| Ikanda | unique | ancient | 75 | 1 | Bronze Working | own |
| Ducal Stable | unique | classical | 75 | 0 | Horseback Riding | own |
| Royal Library | unique | ancient | 75 | 1 | Writing | own |
| Candi | unique | medieval | 120 | 1 | Theology | own |
| Hanse | unique | renaissance | 200 | 0 | Banking | own |
| Walls of Babylon | unique | ancient | 65 | 0 | Masonry | Unciv |
| Ceilidh Hall | unique | renaissance | 200 | 2 | Acoustics | Unciv |
| Coffee House | unique | renaissance | 250 | 2 | Economics | Unciv |
| Pyramid | unique | ancient | 40 | 1 | Pottery | Unciv |

### Wonders

| Building | Kind | Era | Cost | Upkeep | Technology | Provenance |
|---|---|---|---|---|---|---|
| Heroic Epic | national_wonder | classical | 125 | 0 | Iron Working | Unciv |
| National College | national_wonder | classical | 125 | 0 | Philosophy | Unciv |
| National Epic | national_wonder | classical | 125 | 0 | Drama and Poetry | Unciv |
| Circus Maximus | national_wonder | classical | 125 | 0 | Horseback Riding | Unciv |
| East India Company | national_wonder | medieval | 125 | 0 | Guilds | own |
| Ironworks | national_wonder | medieval | 125 | 0 | Machinery | Unciv |
| Oxford University | national_wonder | medieval | 125 | 0 | Education | Unciv |
| Hermitage | national_wonder | renaissance | 125 | 0 | Architecture | Unciv |
| Great Lighthouse | world_wonder | classical | 185 | 0 | Optics | Unciv |
| Stonehenge | world_wonder | ancient | 185 | 0 | Calendar | Unciv |
| Great Library | world_wonder | ancient | 185 | 0 | Writing | Unciv |
| Pyramids | world_wonder | ancient | 185 | 0 | Masonry | Unciv |
| Colossus | world_wonder | classical | 185 | 0 | Iron Working | Unciv |
| Oracle | world_wonder | classical | 250 | 0 | Philosophy | Unciv |
| Hanging Gardens | world_wonder | classical | 250 | 0 | Mathematics | Unciv |
| Great Wall | world_wonder | classical | 250 | 0 | Engineering | Unciv |
| Angkor Wat | world_wonder | medieval | 400 | 0 | Education | Unciv |
| Hagia Sophia | world_wonder | medieval | 300 | 0 | Theology | Unciv |
| Chichen Itza | world_wonder | medieval | 300 | 0 | Civil Service | Unciv |
| Machu Picchu | world_wonder | medieval | 300 | 0 | Guilds | Unciv |
| Notre Dame | world_wonder | medieval | 400 | 0 | Physics | Unciv |
| Porcelain Tower | world_wonder | renaissance | 625 | 0 | Architecture | Unciv |
| Himeji Castle | world_wonder | renaissance | 500 | 0 | Gunpowder | Unciv |
| Sistine Chapel | world_wonder | renaissance | 500 | 0 | Acoustics | Unciv |
| Kremlin | world_wonder | modern | 625 | 0 | Metallurgy | Unciv |
| Forbidden Palace | world_wonder | renaissance | 500 | 0 | Banking | Unciv |
| Taj Mahal | world_wonder | renaissance | 625 | 0 | Architecture | Unciv |
| Big Ben | world_wonder | industrial | 750 | 0 | Industrialization | Unciv |
| Louvre | world_wonder | industrial | 750 | 0 | Archaeology | Unciv |
| Brandenburg Gate | world_wonder | industrial | 750 | 0 | Military Science | Unciv |
| Statue of Liberty | world_wonder | modern | 1060 | 0 | Replaceable Parts | Unciv |
| Cristo Redentor | world_wonder | modern | 1250 | 0 | Plastics | Unciv |
| Eiffel Tower | world_wonder | modern | 1060 | 0 | Radio | Unciv |
| Pentagon | world_wonder | atomic | 1250 | 0 | Combined Arms | Unciv |
| Sydney Opera House | world_wonder | atomic | 1250 | 0 | Ecology | Unciv |
| Statue of Zeus | world_wonder | ancient | 185 | 0 | Bronze Working | Unciv |
| National Intelligence Agency | national_wonder | modern | 120 | 0 | Radio | Unciv |
| Alhambra | world_wonder | medieval | 400 | 0 | Chivalry | Unciv |
| CN Tower | world_wonder | information | 1250 | 0 | Telecommunications | Unciv |
| Hubble Space Telescope | world_wonder | information | 1250 | 0 | Satellites | Unciv |
| Leaning Tower of Pisa | world_wonder | renaissance | 500 | 0 | Printing Press | Unciv |
| Great Mosque of Djenne | world_wonder | medieval | 300 | 0 | Theology | Unciv |
| Neuschwanstein | world_wonder | modern | 1060 | 0 | Railroad | Unciv |
| Petra | world_wonder | classical | 250 | 0 | Currency | Unciv |
| Terracotta Army | world_wonder | classical | 250 | 0 | Construction | Unciv |
| Great Firewall | world_wonder | atomic | 1250 | 0 | Computers | Unciv |
| Grand Temple | national_wonder | medieval | 125 | 0 | Theology | Unciv |
| National Visitor Center | national_wonder | information | 400 | 2 | Telecommunications | own |
| Uffizi | world_wonder | renaissance | 625 | 0 | Architecture | own |
| Globe Theatre | world_wonder | renaissance | 500 | 0 | Printing Press | own |
| Broadway | world_wonder | modern | 1060 | 0 | Radio | own |
| Red Fort | world_wonder | renaissance | 625 | 0 | Metallurgy | own |
| Prora | world_wonder | modern | 1060 | 0 | Flight | own |
| Borobudur | world_wonder | medieval | 300 | 0 | Theology | own |
| Parthenon | world_wonder | classical | 250 | 0 | Drama and Poetry | own |
| International Space Station | world_wonder | ancient | -1 | 0 |  | own |
| Temple of Artemis | world_wonder | ancient | 185 | 0 | Archery | Unciv |
| Mausoleum of Halicarnassus | world_wonder | ancient | 185 | 0 | Masonry | Unciv |
| Manhattan Project | project |  | 750 |  | Atomic Theory | Unciv |
| Apollo Program | project |  | 750 |  | Rocketry | Unciv |
| SS Cockpit | project |  | -1 |  |  | own |
| SS Stasis Chamber | project |  | -1 |  |  | own |
| SS Engine | project |  | -1 |  |  | own |
| SS Booster | project |  | -1 |  |  | own |
| International Space Station | league_project |  |  |  |  | own |
| World's Fair | league_project |  |  |  |  | own |
| International Games | league_project |  |  |  |  | own |

### Technologies

| Technology | Era | Cost | Requires | Provenance |
|---|---|---|---|---|
| Agriculture | ancient | 20 |  | Unciv |
| Pottery | ancient | 35 | Agriculture | Unciv |
| Animal Husbandry | ancient | 35 | Agriculture | Unciv |
| Archery | ancient | 35 | Agriculture | Unciv |
| Mining | ancient | 35 | Agriculture | Unciv |
| Sailing | ancient | 55 | Pottery | Unciv |
| Calendar | ancient | 55 | Pottery | Unciv |
| Writing | ancient | 55 | Pottery | Unciv |
| Trapping | ancient | 55 | Animal Husbandry | Unciv |
| The Wheel | ancient | 55 | Animal Husbandry, Archery | Unciv |
| Masonry | ancient | 55 | Mining | Unciv |
| Bronze Working | ancient | 55 | Mining | Unciv |
| Optics | classical | 85 | Sailing | Unciv |
| Horseback Riding | classical | 105 | The Wheel, Trapping | Unciv |
| Mathematics | classical | 105 | The Wheel | Unciv |
| Construction | classical | 105 | Masonry, The Wheel | Unciv |
| Philosophy | classical | 175 | Calendar, Writing | Unciv |
| Drama and Poetry | classical | 175 | Writing | Unciv |
| Currency | classical | 175 | Mathematics | Unciv |
| Engineering | classical | 175 | Mathematics, Construction | Unciv |
| Iron Working | classical | 175 | Bronze Working | Unciv |
| Theology | medieval | 275 | Philosophy, Drama and Poetry | Unciv |
| Civil Service | medieval | 275 | Drama and Poetry, Horseback Riding, Currency | Unciv |
| Guilds | medieval | 275 | Currency | Unciv |
| Metal Casting | medieval | 275 | Engineering, Iron Working | Unciv |
| Compass | medieval | 375 | Optics, Theology | Unciv |
| Education | medieval | 485 | Theology, Civil Service | Unciv |
| Chivalry | medieval | 485 | Civil Service, Guilds | Unciv |
| Machinery | medieval | 485 | Engineering, Guilds | Unciv |
| Physics | medieval | 485 | Metal Casting | Unciv |
| Steel | medieval | 485 | Metal Casting | Unciv |
| Astronomy | renaissance | 780 | Compass, Education | Unciv |
| Acoustics | renaissance | 780 | Education | Unciv |
| Banking | renaissance | 780 | Education, Chivalry | Unciv |
| Printing Press | renaissance | 780 | Machinery, Physics, Chivalry | Unciv |
| Gunpowder | renaissance | 780 | Physics, Steel | Unciv |
| Navigation | renaissance | 1150 | Astronomy | Unciv |
| Architecture | renaissance | 1150 | Acoustics, Banking | Unciv |
| Economics | renaissance | 1150 | Banking, Printing Press | Unciv |
| Metallurgy | renaissance | 1150 | Gunpowder, Printing Press | Unciv |
| Chemistry | renaissance | 1150 | Gunpowder | Unciv |
| Archaeology | industrial | 1600 | Navigation, Architecture | Unciv |
| Scientific Theory | industrial | 1600 | Economics, Architecture | Unciv |
| Industrialization | industrial | 1600 | Economics | Unciv |
| Rifling | industrial | 1600 | Economics, Metallurgy | Unciv |
| Military Science | industrial | 1600 | Chemistry, Metallurgy | Unciv |
| Fertilizer | industrial | 1600 | Chemistry | Unciv |
| Biology | industrial | 2350 | Archaeology, Scientific Theory | Unciv |
| Electricity | industrial | 2350 | Scientific Theory | Unciv |
| Steam Power | industrial | 2350 | Rifling, Industrialization, Scientific Theory | Unciv |
| Dynamite | industrial | 2350 | Fertilizer, Military Science | Unciv |
| Refrigeration | modern | 3100 | Electricity, Biology | Unciv |
| Radio | modern | 3100 | Electricity | Unciv |
| Replaceable Parts | modern | 3100 | Steam Power, Electricity | Unciv |
| Flight | modern | 3100 | Steam Power | Unciv |
| Railroad | modern | 3100 | Steam Power, Dynamite | Unciv |
| Plastics | modern | 4100 | Radio, Replaceable Parts | Unciv |
| Electronics | modern | 4100 | Replaceable Parts, Flight | Unciv |
| Ballistics | modern | 4100 | Flight | Unciv |
| Combustion | modern | 4100 | Railroad | Unciv |
| Penicillin | atomic | 5100 | Refrigeration, Plastics | Unciv |
| Atomic Theory | atomic | 5100 | Plastics, Electronics | Unciv |
| Radar | atomic | 5100 | Electronics, Ballistics | Unciv |
| Combined Arms | atomic | 5100 | Ballistics, Combustion | Unciv |
| Ecology | atomic | 6400 | Atomic Theory, Penicillin | Unciv |
| Nuclear Fission | atomic | 6400 | Atomic Theory, Radar | Unciv |
| Rocketry | atomic | 6400 | Radar | Unciv |
| Computers | atomic | 6400 | Radar, Combined Arms | Unciv |
| Telecommunications | information | 7700 | Ecology | Unciv |
| Mobile Tactics | information | 7700 | Ecology, Nuclear Fission | Unciv |
| Advanced Ballistics | information | 7700 | Nuclear Fission, Rocketry | Unciv |
| Satellites | information | 7700 | Rocketry | Unciv |
| Robotics | information | 7700 | Rocketry, Computers | Unciv |
| Lasers | information | 7700 | Computers | Unciv |
| The Internet | information | 8800 | Telecommunications | own |
| Globalization | information | 8800 | Telecommunications | Unciv |
| Particle Physics | information | 8800 | Telecommunications, Mobile Tactics, Advanced Ballistics | Unciv |
| Nuclear Fusion | information | 8800 | Advanced Ballistics, Satellites, Robotics | Unciv |
| Nanotechnology | information | 8800 | Robotics | Unciv |
| Stealth | information | 8800 | Robotics, Lasers | Unciv |
| Future Tech | information | 9500 | The Internet, Globalization, Particle Physics, Nuclear Fusion, Nanotechnology, Stealth | Unciv |

### Policy branches

| Branch | Era required | Policies (opener and finisher included) | Provenance |
|---|---|---|---|
| Tradition |  | 7 | Unciv |
| Liberty |  | 7 | Unciv |
| Honor |  | 7 | Unciv |
| Piety |  | 7 | Unciv |
| Patronage | classical | 7 | Unciv |
| Aesthetics | classical | 7 | Unciv |
| Commerce | medieval | 7 | Unciv |
| Exploration | medieval | 7 | own |
| Rationalism | renaissance | 7 | Unciv |
| Freedom | industrial | 16 | Unciv |
| Order | industrial | 16 | Unciv |
| Autocracy | industrial | 16 | Unciv |

### Beliefs

| Category | Beliefs | Unciv | own | own, unverified | not recorded |
|---|---|---|---|---|---|
| enhancer | 9 | 9 | 0 | 0 | 0 |
| follower | 16 | 15 | 1 | 0 | 0 |
| founder | 9 | 9 | 0 | 0 | 0 |
| pantheon | 26 | 22 | 4 | 0 | 0 |
| reformation | 9 | 0 | 9 | 0 | 0 |

### Resources

| Resource | Class | Yields | Happiness | Revealed by | Provenance |
|---|---|---|---|---|---|
| Iron | strategic | production 1 |  | Iron Working | Unciv |
| Horses | strategic | production 1 |  | Animal Husbandry | Unciv |
| Coal | strategic | production 1 |  | Industrialization | Unciv |
| Oil | strategic | production 1 |  | Biology | Unciv |
| Aluminum | strategic | production 1 |  | Electricity | Unciv |
| Uranium | strategic | production 1 |  | Atomic Theory | Unciv |
| Wheat | bonus | food 1 |  |  | Unciv |
| Cattle | bonus | food 1 |  |  | Unciv |
| Sheep | bonus | food 1 |  |  | Unciv |
| Deer | bonus | food 1 |  |  | Unciv |
| Bananas | bonus | food 1 |  |  | Unciv |
| Fish | bonus | food 1 |  |  | Unciv |
| Stone | bonus | production 1 |  |  | Unciv |
| Whales | luxury | food 1, gold 1 | 4 |  | Unciv |
| Pearls | luxury | gold 2 | 4 |  | Unciv |
| Gold | luxury | gold 2 | 4 |  | Unciv |
| Silver | luxury | gold 2 | 4 |  | Unciv |
| Gems | luxury | gold 3 | 4 |  | Unciv |
| Marble | luxury | gold 2 | 4 |  | Unciv |
| Ivory | luxury | gold 2 | 4 |  | Unciv |
| Furs | luxury | gold 2 | 4 |  | Unciv |
| Dyes | luxury | gold 2 | 4 |  | Unciv |
| Spices | luxury | gold 2 | 4 |  | Unciv |
| Silk | luxury | gold 2 | 4 |  | Unciv |
| Sugar | luxury | gold 2 | 4 |  | Unciv |
| Cotton | luxury | gold 2 | 4 |  | Unciv |
| Wine | luxury | gold 2 | 4 |  | Unciv |
| Incense | luxury | gold 2 | 4 |  | Unciv |
| Jewelry | luxury | gold 2 | 4 |  | Unciv |
| Porcelain | luxury | gold 2 | 4 |  | Unciv |
| Copper | luxury | gold 2 | 4 |  | Unciv |
| Salt | luxury | food 1, gold 1 | 4 |  | Unciv |
| Crab | luxury | food 1, gold 1 | 4 |  | Unciv |
| Truffles | luxury | gold 2 | 4 |  | Unciv |
| Citrus | luxury | food 1, gold 1 | 4 |  | Unciv |
| Antiquity Site | archaeology |  |  | Archaeology | own |
| Nutmeg | luxury | gold 2 | 4 |  | own |
| Cloves | luxury | gold 2 | 4 |  | own |
| Pepper | luxury | gold 2 | 4 |  | own |
| Hidden Antiquity Site | archaeology |  |  |  | own |
| Bison | bonus | food 1 |  |  | own |
| Cocoa | luxury | food 1, gold 1 | 4 |  | own |

### Improvements

| Improvement | Yields | Technology | Provenance |
|---|---|---|---|
| City Ruins |  |  | Unciv |
| Encampment |  |  | own |
| Ancient Ruins |  |  | Unciv |
| Farm | food 1 | Agriculture | Unciv |
| Mine | production 1 | Mining | Unciv |
| Quarry |  | Masonry | Unciv |
| Trading Post | gold 1 | Guilds | Unciv |
| Lumber Mill | production 1 | Construction | Unciv |
| Pasture |  | Animal Husbandry | Unciv |
| Fishing Boats |  | Sailing | Unciv |
| Plantation |  | Calendar | Unciv |
| Camp |  | Trapping | Unciv |
| Oil Well |  | Biology | Unciv |
| Offshore Platform |  | Refrigeration | Unciv |
| Fort |  | Engineering | Unciv |
| Landmark |  |  | Unciv |
| Academy | science 8 |  | Unciv |
| Customs House | gold 4 |  | Unciv |
| Manufactory | production 4 |  | Unciv |
| Citadel |  |  | Unciv |
| Holy Site | faith 6 |  | Unciv |
| Archaeological Dig |  | Archaeology | own |
| Brazilwood Camp | gold 2 | Machinery | own |
| Kasbah | food 1, gold 1, production 1 | Chivalry | own |
| Feitoria |  | Navigation | own |
| Chateau | culture 2, gold 1 | Chivalry | own |
| Terrace Farm | food 1 | Construction | Unciv |
| Moai | culture 1 | Construction | Unciv |
| Polder | food 3 | Guilds | Unciv |

### Terrain

| Name | Yields | Movement | Defence % | Impassable | Provenance |
|---|---|---|---|---|---|
| Grassland | food 2 | 1 | 0 |  | Unciv |
| Plains | food 1, production 1 | 1 | 0 |  | Unciv |
| Desert |  | 1 | 0 |  | Unciv |
| Tundra | food 1 | 1 | 0 |  | Unciv |
| Snow |  | 1 | 0 |  | Unciv |
| Coast | food 1, gold 1 | 1 | 0 |  | Unciv |
| Ocean | food 1, gold 1 | 1 | 0 |  | Unciv |
| Mountain |  | 1 | 0 |  | Unciv |
| Hill |  | 1 | 0 |  | Unciv |

### Terrain features

| Name | Yields | Movement | Defence % | Impassable | Provenance |
|---|---|---|---|---|---|
| Ice |  | 1 | 0 | yes | Unciv |
| Jungle | food 1, production -1 | 2 | 25 |  | Unciv |
| Marsh | food -1 | 3 | -15 |  | Unciv |
| Oasis | food 3, gold 1 | 1 | -10 |  | Unciv |
| Flood Plains | food 2 | 1 | -10 |  | Unciv |
| Forest | food 1, production 1 | 2 | 25 |  | Unciv |
| Fallout | food -3, gold -3, production -3 | 2 | -15 |  | Unciv |
| Atoll | food 1, production 1 | 1 | 0 |  | Unciv |

### Natural wonders

| Name | Yields | Movement | Defence % | Impassable | Provenance |
|---|---|---|---|---|---|
| The Barringer Crater | gold 2, science 3 | 1 | 0 | yes | Unciv |
| Mt. Fuji | culture 3, faith 3, gold 2 | 1 | 0 | yes | own |
| The Grand Mesa | gold 3, production 2 | 1 | 0 | yes | Unciv |
| The Great Barrier Reef | food 2, gold 1, production 1, science 2 | 1 | 0 | yes | Unciv |
| Krakatoa | science 5 | 1 | 0 | yes | Unciv |
| Rock of Gibraltar | food 2, gold 5 | 1 | 0 | yes | Unciv |
| Old Faithful | science 2 | 1 | 0 | yes | Unciv |
| Fountain of Youth |  | 1 | 0 | yes | Unciv |
| Cerro de Potosi | gold 10 | 1 | 0 | yes | Unciv |
| El Dorado | culture 5 | 1 | 0 | yes | Unciv |
| Sri Pada | faith 4, food 2 | 1 | 0 | yes | Unciv |
| Mt. Sinai | faith 8 | 1 | 0 | yes | own |
| Mt. Kailash | faith 6 | 1 | 0 | yes | own |
| Uluru | faith 6, food 2 | 1 | 0 | yes | Unciv |
| Lake Victoria | food 6 | 1 | 0 | yes | own |
| Mt. Kilimanjaro | culture 2, food 3 | 1 | 0 | yes | own |
| King Solomon's Mines | production 6 | 1 | 0 | yes | own |

### Eras

| Era | Index | Research agreement cost | Embarked defence | Starting gold | Research % | Provenance |
|---|---|---|---|---|---|---|
| Ancient Era | 0 | 150 | 3 | 0 | 100 | Unciv |
| Classical Era | 1 | 150 | 4 | 10 | 67 | Unciv |
| Medieval Era | 2 | 250 | 6 | 25 | 50 | Unciv |
| Renaissance Era | 3 | 250 | 8 | 50 | 33 | Unciv |
| Industrial Era | 4 | 300 | 10 | 100 | 20 | Unciv |
| Modern Era | 5 | 350 | 13 | 200 | 12 | Unciv |
| Atomic Era | 6 | 400 | 16 | 200 | 12 | Unciv |
| Information Era | 7 | 400 | 20 | 400 | 5 | Unciv |

### Great people

| Great person | Specialist | Points from | Provenance |
|---|---|---|---|
| Great Writer | writer | specialists | own |
| Great Artist | artist | specialists | Unciv |
| Great Musician | musician | specialists | own |
| Great Scientist | scientist | specialists | Unciv |
| Great Engineer | engineer | specialists | Unciv |
| Great Merchant | merchant | specialists | Unciv |
| Great General |  | combat_experience | Unciv |
| Great Admiral |  | combat_experience | Unciv |
| Great Prophet |  | faith | Unciv |

| Specialist | Yields | Great person points | Provenance |
|---|---|---|---|
| Unemployed Citizen | production 1 | 0 | own, unverified |
| Writer |  | 3 | own, unverified |
| Artist |  | 3 | Unciv |
| Musician |  | 3 | own, unverified |
| Scientist | science 3 | 3 | Unciv |
| Merchant | gold 3 | 3 | Unciv |
| Engineer | production 2 | 3 | Unciv |

### Nations

| Nation | Leader | Ability | Provenance |
|---|---|---|---|
| America | Washington | Manifest Destiny | Unciv |
| Arabia | Harun al-Rashid | Ships of the Desert | Unciv |
| The Aztecs | Montezuma | Sacrificial Captives | Unciv |
| China | Wu Zetian | Art of War | Unciv |
| Egypt | Ramesses II | Monument Builders | Unciv |
| England | Elizabeth | Sun Never Sets | Unciv |
| France | Napoleon | City of Light | Unciv |
| Germany | Bismarck | Furor Teutonicus | Unciv |
| Greece | Alexander | Hellenic League | Unciv |
| India | Gandhi | Population Growth | Unciv |
| The Iroquois | Hiawatha | The Great Warpath | Unciv |
| Japan | Oda Nobunaga | Bushido | Unciv |
| The Ottomans | Suleiman | Barbary Corsairs | Unciv |
| Persia | Darius I | Achaemenid Legacy | Unciv |
| Rome | Augustus Caesar | The Glory of Rome | Unciv |
| Russia | Catherine | Siberian Riches | Unciv |
| Siam | Ramkhamhaeng | Father Governs Children | Unciv |
| Songhai | Askia | River Warlord | Unciv |
| Ethiopia | Haile Selassie | Spirit of Adwa | Unciv |
| Assyria | Ashurbanipal | Treasures of Nineveh | own |
| Brazil | Pedro II | Carnival | own |
| Indonesia | Gajah Mada | Spice Islanders | own |
| Morocco | Ahmad al-Mansur | Gateway to Africa | own |
| Poland | Casimir III | Solidarity | own |
| Portugal | Maria I | Mare Clausum | own |
| The Shoshone | Pocatello | Great Expanse | own |
| Venice | Enrico Dandolo | Serenissima | Unciv |
| The Zulus | Shaka | Iklwa | own |
| Babylon | Nebuchadnezzar II | Ingenuity | Unciv |
| Mongolia | Genghis Khan | Mongol Terror | Unciv |
| The Inca | Pachacuti | Great Andean Road | Unciv |
| Spain | Isabella | Seven Cities of Gold | Unciv |
| Polynesia | Kamehameha | Wayfinding | Unciv |
| Denmark | Harald Bluetooth | Viking Fury | Unciv |
| Korea | Sejong | Scholars of the Jade Hall | Unciv |
| Austria | Maria Theresa | Diplomatic Marriage | Unciv |
| Byzantium | Theodora | Patriarchate of Constantinople | Unciv |
| Carthage | Dido | Phoenician Heritage | Unciv |
| The Celts | Boudicca | Druidic Lore | Unciv |
| The Huns | Attila | Scourge of God | Unciv |
| The Maya | Pacal | The Long Count | Unciv |
| The Netherlands | William | Dutch East India Company | Unciv |
| Sweden | Gustavus Adolphus | Nobel Prize | Unciv |

### City-states

| Type | City-states | Friends at | Allies at | Provenance |
|---|---|---|---|---|
| Cultured City-States | 14 | 30 | 60 | Unciv |
| Militaristic City-States | 8 | 30 | 60 | Unciv |
| Maritime City-States | 14 | 30 | 60 | Unciv |
| Mercantile City-States | 14 | 30 | 60 | Unciv |
| Religious City-States | 8 | 30 | 60 | Unciv |

### Promotions

214 promotions; provenance in the overview table.

### Difficulty levels

From `classic/assets/rules/classic/handicaps.ron` (fields Unciv carries come from its difficulty file; the file header names the rest).

| Level | Base happiness | City unhappiness % | Citizen unhappiness % | Unit cost % | Building cost % | Research % | Policy % | AI growth % | AI train % | AI construct % |
|---|---|---|---|---|---|---|---|---|---|---|
| Settler | 15 | 40 | 40 | 50 | 50 | 90 | 50 | 160 | 175 | 160 |
| Chieftain | 12 | 60 | 60 | 67 | 67 | 95 | 67 | 130 | 130 | 130 |
| Warlord | 12 | 75 | 75 | 85 | 85 | 100 | 85 | 110 | 110 | 110 |
| Prince | 9 | 100 | 100 | 100 | 100 | 100 | 100 | 100 | 100 | 100 |
| King | 9 | 100 | 100 | 100 | 100 | 100 | 100 | 90 | 85 | 85 |
| Emperor | 9 | 100 | 100 | 100 | 100 | 100 | 100 | 85 | 80 | 80 |
| Immortal | 9 | 100 | 100 | 100 | 100 | 100 | 100 | 75 | 65 | 65 |
| Deity | 9 | 100 | 100 | 100 | 100 | 100 | 100 | 60 | 50 | 50 |
| AI Default | 15 | 90 | 90 | 75 | 75 | 100 | 100 | 100 | 100 | 100 |

### Game speeds

From `classic/assets/rules/classic/speeds.ron` (every number from Unciv's `Speeds.json`; the file header names the commit). Factors are percents of Standard; a game keeps its speed in its save (Standard: none, as before speeds) and every rule reads it through `GameSettings::speed`.

| Speed | Pace % | Production % | Gold % | Science % | Culture % | Faith % | Build time % | Barbarian delay % | Gold gift % | Tribute step (rounds x10) | Golden age % | Pressure | Peace rounds | Deal rounds | Last round |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| Quick | 67 | 67 | 67 | 67 | 67 | 67 | 67 | 67 | 125 | 50 | 80 | 9 | 10 | 25 | 330 |
| Standard | 100 | 100 | 100 | 100 | 100 | 100 | 100 | 100 | 100 | 65 | 100 | 6 | 10 | 30 | 500 |
| Epic | 150 | 150 | 150 | 150 | 150 | 150 | 150 | 150 | 75 | 140 | 125 | 4 | 15 | 45 | 750 |
| Marathon | 300 | 300 | 300 | 300 | 300 | 300 | 300 | 400 | 67 | 320 | 200 | 2 | 30 | 90 | 1500 |

What each factor scales (Unciv: the file and function that apply the same field):

- **Pace** (`modifier`): food to grow (CityPopulationManager), great person points (GreatPersonManager), golden age happiness (GoldenAgeManager), upgrade prices (UnitUpgradeManager), quest durations and countdowns (QuestManager, AssignedQuest), the allied city-state's great person countdown and the gift progress (CityStateFunctions), feature chop production (TileImprovementFunctions); own design: the World Congress session interval (as Unciv's diplomatic votes, Civilization.getTurnsBetweenDiplomaticVotes).
- **Production** (`productionCostModifier`): units, buildings, projects and World Congress shares (Building.kt, BaseUnitCost.kt), so gold purchases too; the great engineer's hurry (UnitActionsGreatPerson); the scrap gold of a city-state buyout.
- **Gold** (`goldCostModifier`): tile purchase (CityExpansionManager), research agreements (DiplomacyFunctions), city-state buyout (CityStateFunctions), camp gold (MapUnit), the great merchant's trade mission (UnitActionsGreatPerson), ruins gold (Ruins.json).
- **Science** (`scienceCostModifier`): technology cost (TechManager.costOfTech), the great scientist's science (TechManager).
- **Culture** (`cultureCostModifier`): policy cost (PolicyManager), border culture (CityExpansionManager), the great writer's culture (PolicyManager), ruins culture (Ruins.json).
- **Faith** (`faithCostModifier`, `statCostModifiers`): pantheons and prophets (ReligionManager), faith purchases of units, buildings and great people (BaseUnitCost.kt, Building.kt), ruins faith.
- **Build time** (`improvementBuildLengthModifier`): every worker job, roads, repairs and feature removal included (TileImprovement.getTurnsToBuild), rounded to whole turns.
- **Barbarian delay** (`barbarianModifier`): rounds between a camp's spawns (BarbarianEncampment).
- **Gold gift** (`goldGiftModifier`) and **tribute step** (`cityStateTributeScalingInterval`): influence bought with gold, the tribute's base and growth (CityStateFunctions).
- **Golden age** (`goldenAgeLengthModifier`): every golden age's length (GoldenAgeManager).
- **Pressure** (`religiousPressureAdjacentCity`): a city's pressure on a city in range, at this module's 10 times larger scale (CityReligionManager).
- **Peace and deal rounds** (`peaceDealDuration`, `dealDuration`): a peace treaty, and every timed deal (TradeOffer).
- **Calendar** (`startYear`, `turns`): the year of a round (Speed.turnToYear), the Maya Long Count; the last round is the time victory's and the end of the unit upkeep curve (CivInfoStatsForNextTurn.getUnitMaintenance).

Not scaled (as Unciv): great general points, the city-state election interval, friendship and denouncement lengths, the first quest round. Not scaled yet: the one-shot yields of beliefs and wonders that Unciv marks "modified by game speed" (Initiation Rites, Reliquary, Mausoleum): our effects carry no such mark.

### Projects

From `classic/assets/rules/classic/projects.ron`.

| Project | Kind | Cost | Technology |
|---|---|---|---|
| Apollo Program | Project | 750 |  |
| International Games | LeagueProject | 0 |  |
| International Space Station | LeagueProject | 0 |  |
| Manhattan Project | Project | 750 |  |
| SS Booster | Project | 0 |  |
| SS Cockpit | Project | 0 |  |
| SS Engine | Project | 0 |  |
| SS Stasis Chamber | Project | 0 |  |
| World's Fair | LeagueProject | 0 |  |
