# Game rules of the classic set

> **Generated file, do not edit.** Made by `classic/tools/gen-rules-doc.py` from the project's own data: the rule constants of the Rust source (`classic/crates/classic-rules/src`, `platform/crates/kernel/src`) with their doc comments, and the JSON catalogs in `classic/docs/classic/`. Regenerate with `python3 -I classic/tools/gen-rules-doc.py`; `--check` fails when this file is stale (`tools/check-all.sh` runs it).
>
> **Sources.** The classic set is a Civ V-style rule set. Its rules and numbers may come only from (1) Unciv (open source, MPL-2.0; cited as `Unciv: <file>`) or (2) our own design (`own value`; `own design` for the systems Unciv lacks (BNW-DESIGN.md); `own value (unverified)` where it was not compared with a public source). See [SOURCES.md](SOURCES.md) and [UNCIV-ATTRIBUTION.md](UNCIV-ATTRIBUTION.md). The source column of every constant below is read from the constant's own doc comment; a comment without a recognised source clause is listed as `own value (source not recorded)`.
>
> **Formulas** are described in short sentences in our own words, with the numbers taken from the constants; the code is the authority. This file holds no pseudo-code or text of the original game.

## Summary

Rule constants: 479 (AI constants: 228; 12 constants of engine plumbing in `platform/crates/kernel/src/persistence/`, `platform/crates/kernel/src/test_module.rs`, `platform/crates/kernel/src/math.rs`, `platform/crates/kernel/src/module.rs`, `classic/crates/classic-rules/src/save_codec.rs`, `classic/crates/classic-rules/src/fixtures.rs` are not listed).

| Source | Rule constants | AI constants |
|---|---|---|
| Unciv | 246 | 0 |
| own design | 71 | 0 |
| own formula (expansion-only system; Unciv has no equivalent) | 0 | 0 |
| own value | 14 | 0 |
| own value (unverified) | 17 | 4 |
| own value (source not recorded) | 131 | 224 |
| total | 479 | 228 |

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
- [AI constants](#ai-constants)
- [Data catalogs](#data-catalogs)

## Formulas

Short descriptions in our own words of the main formulas; the code is the authority (the function named in each item). Every number below is the value of the named constant.

- **Growth** (`systems::growth::growth_threshold`). A city of size n needs 15 (`FOOD_TO_NEXT_POPULATION_BASE`) food plus 8 (`FOOD_TO_NEXT_POPULATION_PER_POP`) for every citizen beyond the first, plus the integer square root of the cube of (n - 1). An AI player's threshold is then scaled by its difficulty level's growth percent.
- **Technology cost** (`derived::research::tech_cost`). The technology's catalog cost is scaled by the difficulty level's research percent and the map size's research percent, multiplied by (1 + the map size's per-city percent times (cities - 1)), counting the most cities the player ever had, and divided by (1 + 30 (`KNOWN_DISCOUNT_PERCENT`) percent times the share of living civilizations that the player has met and that know the technology); the result is truncated.
- **Policy cost** (`derived::culture::policy_cost`). 25 (`POLICY_CULTURE_COST_BASE`) culture plus (3 (`POLICY_CULTURE_COST_PER_POLICY`) times the policies already paid) raised to the power 2.01 (`POLICY_CULTURE_COST_EXPONENT`); the cost modifiers (never more than 75 (`POLICY_COST_MAX_DISCOUNT_PERCENT`) percent off) and the difficulty percent apply, the result is multiplied by (1 + the map size's per-city percent times (cities - 1)), rounded to the nearest whole and then down to a multiple of 5 (`POLICY_CULTURE_COST_ROUND_TO`).
- **Border growth by culture** (`derived::culture::border_cost`, table `kernel::world::city::CULTURE_COSTS`). A city that has claimed t tiles beyond its first ring needs 6.0 (`CULTURE_TO_NEXT_TILE_FACTOR`) times (t + 1.4813 (`CULTURE_TO_NEXT_TILE_OFFSET`)) raised to the power 1.3 (`CULTURE_TO_NEXT_TILE_EXPONENT`) culture for its next tile, rounded to the nearest whole. Modifiers can lower the cost by at most 85 (`CULTURE_TO_NEXT_TILE_MAX_DISCOUNT_PERCENT`) percent.
- **Tile purchase** (`kernel::world::territory::tile_purchase_offer`). A tile at distance d (at most 3 (`PURCHASE_RADIUS_MAX`)) from the city costs 50 (`PLOT_BASE_COST`) gold times (d - 1) plus 5 (`PLOT_COST_PER_PLOT_BOUGHT`) gold for every tile the city has claimed beyond its first ring (by culture or purchase); the city's tile purchase modifiers then apply, and a tile is never free.
- **Item gold price** (`kernel::world::city`). The gold price of a unit or building grows with (30 (`GOLD_PER_PRODUCTION`) times its production cost) raised to the power 0.75, rounded down to a multiple of 10 (`PURCHASE_DIVISOR`); a partly built item costs less.
- **Combat damage** (`combat::damage`). The stronger side's strength divided by the weaker's gives a ratio r; the damage factor is the mean of ((r + 3) / 4) to the fourth power and 1 (its inverse for the weaker side). A unit takes that factor times a base of 2400 (`MELEE_EQUAL_STRENGTH_BASE_DAMAGE`) hundredths of a hit point plus a random part up to 1200 (`MELEE_EQUAL_STRENGTH_RANDOM_DAMAGE`), out of 100 (`UNIT_MAX_HP`) hit points, rounded to the nearest hit point with no minimum. A wounded unit's damage drops by a percentage equal to 33 (`WOUNDED_DAMAGE_PERCENT`) percent of its missing hit points (cities never weaken). A civilian that is attacked loses 40 (`DAMAGE_TO_CIVILIAN_UNIT`) hit points.
- **Healing** (`systems::combat::heal`). A unit that rests heals 25 (`HEALING_IN_CITY_CENTER`) hit points in a city, 20 (`HEALING_FRIENDLY_TERRITORY`) on its own or a friendly player's land, 10 (`HEALING_NEUTRAL_TERRITORY`) on neutral land and 10 (`HEALING_ENEMY_TERRITORY`) on enemy land, plus the bonuses of its promotions and of nearby units.
- **Unit upkeep** (`derived::maintenance::unit_maintenance`). Every player keeps 3 (`FREE_UNITS`) units free, plus the difficulty's, the policies' and the garrison free units. With n paid units and p the turn over 500 (`ESTIMATED_END_TURN`) (at most 1), the gold per turn is (50 (`GOLD_PER_UNIT_TIMES100`) hundredths times n times (1 + 1 (`UNIT_COST_GROWTH_FACTOR`) p)) raised to the power (1 + p / 3 (`UNIT_COST_EXPONENT_DIVISOR`)), then the difficulty's unit cost percent applies.
- **Golden age** (`systems::golden_age::golden_age_threshold`). The first golden age needs 500 (`GOLDEN_AGE_BASE_THRESHOLD`) happiness, every later one 250 (`GOLDEN_AGE_THRESHOLD_INCREASE`) more; the threshold grows by 1 (`GOLDEN_AGE_THRESHOLD_CITY_PERCENT`) percent per city, truncated. A golden age lasts 10 (`UNMODIFIED_GOLDEN_AGE_TURNS`) turns before modifiers.

## Rule constants by system

Every `pub const` of the rule code, by system. Value as written in the code; meaning is the first sentence of the doc comment; the source is the doc comment's first source clause.

### Turn order and game length

7 constants (Unciv 1, own value (unverified) 1, own value (source not recorded) 5). Files: `classic/crates/classic-rules/src/classic/mod.rs`, `classic/crates/classic-rules/src/classic/systems.rs`, `classic/crates/classic-rules/src/classic/turn.rs`, `platform/crates/kernel/src/turn/calendar.rs`, `platform/crates/kernel/src/turn/resolution.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `CLASSIC_FINGERPRINT` | `14820141147641077048` | The fingerprint of the current content (`CompiledRuleset::hash`). | own value (source not recorded) | `mod.rs` |
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
| `MAX_QUEUE` | `16` | Order queue limit (spam protection; Civ has none; we will raise it when the UI shows the need). | own value (source not recorded) | `city.rs` |
| `CULTURE_RADIUS_MAX` | `5` | Maximum culture growth radius (Civ V: 5 rings; rings 4-5 are a buffer territory the city does not work, as in Civ V). | Unciv: models/ModConstants.kt (`cityExpandRange` 5), equal | `city.rs` |
| `PURCHASE_RADIUS_MAX` | `3` | Radius for buying tiles with gold (Classic: only within 3 of the centre). | own value (source not recorded) | `city.rs` |
| `CITY_DEFENSE_BASE` | `5` | The AI strategy's rough estimate of a city's defense (base + citizens); the combat value is `combat::city::city_strength`. | own value | `city.rs` |
| `GOLD_PER_PRODUCTION` | `30` | Factor on the production cost inside the item price formula `(30 P)^0.75` (the form is also Unciv's gold cost of an item; see classic/docs/classic/SOURCES.md). | Unciv: models/ruleset/IConstruction.kt (`(30 cost)^0.75`), equal | `city.rs` |
| `PURCHASE_DIVISOR` | `10` | Item prices are rounded down to a multiple of this. | Unciv: models/ruleset/IConstruction.kt (rounded down to 10), equal | `city.rs` |
| `CITY_BASE_HIT_POINTS` | `200` | Hit points of a city without buildings (walls, castles and the rest add theirs, `crate::combat::city`). | own value (unverified) | `city.rs` |
| `HURRY_RATE` | `2` | Urgency surcharge (Civ): buying out costs x2 of the missing points. | own value | `city.rs` |
| `CULTURE_COST_LEN` | `128` | Cost of the city's next territory tile (border growth): `round(6 (t + 1.4813)^1.3)`, t is how many tiles the city has already claimed beyond its first ring (by culture and purchase together). | Unciv: CityExpansionManager.getCultureToNextTile, equal (Standard speed, no modifiers) | `city.rs` |
| `GAME_MAX_TURNS` | `500` | Game length for GameProgress (Standard speed, 500 turns; Unciv `Speeds.json` has the same scale of speeds). | Unciv: Speeds.json (Standard, 500 turns), equal | `city.rs` |
| `UNLOCKED` | `[Process::Wealth, Process::Research]` | The processes a technology unlocks. | own value (source not recorded) | `city_economy.rs` |

### Gold and maintenance

11 constants (Unciv 10, own value (unverified) 1). Files: `classic/crates/classic-rules/src/derived/income.rs`, `classic/crates/classic-rules/src/derived/maintenance.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `ALLY_SCIENCE_PERCENT` | `25` | Percent of the science of a major's allied city-states that the major gets. | own value (unverified) | `income.rs` |
| `GOLD_PER_UNIT_TIMES100` | `50` | Gold per paid unit in hundredths. | Unciv: CivInfoStatsForNextTurn.getUnitMaintenance, equal (0.5) | `maintenance.rs` |
| `UNIT_COST_GROWTH_FACTOR` | `1` | How fast the unit cost grows with the turn: the base is multiplied by `1 + MULTIPLIER × p` and raised to `1 + p / EXPONENT_DIVISOR`, where p is the turn over the end turn of the Standard game (at most 1). | Unciv: CivInfoStatsForNextTurn.getUnitMaintenance, equal (1 and 3) | `maintenance.rs` |
| `UNIT_COST_EXPONENT_DIVISOR` | `3` | Unciv: CivInfoStatsForNextTurn.getUnitMaintenance, equal. | Unciv: CivInfoStatsForNextTurn.getUnitMaintenance, equal | `maintenance.rs` |
| `ESTIMATED_END_TURN` | `500` | The end turn of the Standard game. | Unciv: Speeds.json (Standard, 500 turns), equal | `maintenance.rs` |
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
| `REBELLION_THRESHOLD` | `-21` | Excess happiness at or below which rebels may rise (below -20). | Unciv: GlobalUniques.json (`Rebel units may spawn <when below [-20] [Happiness]>`), equal | `happiness.rs` |
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
| `KNOWN_DISCOUNT_PERCENT` | `30` | The discount in percent when every living civilization knows the technology. | Unciv: TechManager.getScienceModifier, equal (0.3) | `research.rs` |
| `MEDIAN_TECH_PERCENT` | `50` | Percent of the median research cost that a science boost gives, for a player without modifiers. | own value (unverified) | `research.rs` |

### Golden ages

4 constants (Unciv 4). Files: `classic/crates/classic-rules/src/systems/golden_age.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `UNMODIFIED_GOLDEN_AGE_TURNS` | `10` | Rounds of a golden age at Standard speed, before modifiers. | Unciv: GoldenAgeManager.enterGoldenAge, 10; equal | `golden_age.rs` |
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

72 constants (Unciv 59, own value (unverified) 4, own value (source not recorded) 9). Files: `classic/crates/classic-rules/src/combat/damage.rs`, `classic/crates/classic-rules/src/combat/experience.rs`, `classic/crates/classic-rules/src/combat/strength.rs`, `classic/crates/classic-rules/src/combat/terrain.rs`, `classic/crates/classic-rules/src/systems/airlift.rs`, `classic/crates/classic-rules/src/systems/combat/actions.rs`, `classic/crates/classic-rules/src/systems/combat/air.rs`, `classic/crates/classic-rules/src/systems/combat/heal.rs`, `classic/crates/classic-rules/src/systems/combat/upgrade.rs`, `classic/crates/classic-rules/src/systems/combat/validate.rs`, `classic/crates/classic-rules/src/systems/paradrop.rs`, `classic/crates/classic-rules/src/systems/plunder.rs`, `platform/crates/kernel/src/effects/unit.rs`.

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
| `SIGHT_RADIUS` | `2` | Sight of a unit kind without a sight value (Civ V: every unit sees 2). | Unciv: MapUnit.getVisibilityRange (2), equal | `visibility.rs` |
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
| `ALL` | `[RuinsBlock::UnknownUnit, RuinsBlock::NotACivilization, RuinsBlock::NoRuinsHere, RuinsB...` |  | own value (source not recorded) | `ruins.rs` |
| `LAND_TILES_PER_RUINS` | `80` | One ruin for this many tiles of land a unit can enter. | own value (source not recorded) | `ruins.rs` |
| `MIN_DISTANCE_FROM_START` | `4` | No ruin closer than this to a start, a city or a city-state's warrior. | own value (source not recorded) | `ruins.rs` |
| `MIN_DISTANCE_BETWEEN` | `4` | No ruin closer than this to another one. | own value (source not recorded) | `ruins.rs` |
| `FIRST_WAVE_DIVISOR` | `3` | The first barbarian turn places the target camp count divided by this (at least one). | Unciv: BarbarianManager.placeBarbarianEncampment, a third on the first turn; equal | `barbarians.rs` |
| `NEW_CAMP_ODDS` | `2` | A new camp is placed on one turn in this many (one in two). | Unciv: BarbarianManager.placeBarbarianEncampment, a coin flip per turn; equal | `barbarians.rs` |
| `MIN_DISTANCE_CITY` | `4` | A camp is never within this many tiles of a major civilization's capital (other cities do not count). | Unciv: BarbarianManager.placeBarbarianEncampment, 4 tiles from major capitals; equal | `barbarians.rs` |
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
| `FOUNDING_CITY_PRESSURE` | `5000` | Pressure a new religion gets in its founding city (× population). | Unciv: logic/civilization/managers/ReligionManager.kt (500 per citizen); equal at the 10 times larger sc... | `religion.rs` |
| `HOLY_CITY_PRESSURE` | `5` | Pressure a holy city adds to its own religion each turn, as a multiple of `ADJACENT_CITY_PRESSURE`. | Unciv: CityReligionManager.getAffectedBySurroundingCities, equal (5) | `religion.rs` |
| `ADJACENT_CITY_PRESSURE` | `60` | Pressure a city spreads to a city in range each turn, at Standard speed. | Unciv: Speeds.json (Standard, religiousPressureAdjacentCity 6), equal at the 10 times larger scale used... | `religion.rs` |
| `ADJACENT_CITY_DISTANCE` | `10` | Range in tiles within which a city spreads its religion. | Unciv: CityReligionManager.getSpreadRange, equal (10) | `religion.rs` |
| `MISSIONARY_PRESSURE_MULTIPLIER` | `10` | Pressure of a missionary action is its religious strength times this: the scale of this module. | Unciv: logic/map/mapunit/actions/ UnitActionsReligion.kt (pressure = religious strength); equal | `religion.rs` |
| `FIRST_PANTHEON_FAITH` | `10` | Faith for the first pantheon. | Unciv: ModConstants.pantheonBase, equal (10) | `religion.rs` |
| `NEXT_PANTHEON_FAITH` | `5` | Faith added to the pantheon price for each pantheon founded. | Unciv: ModConstants.pantheonGrowth, equal (5) | `religion.rs` |
| `FIRST_PROPHET_FAITH` | `200` | Faith for the first Great Prophet. | Unciv: ReligionManager.faithForNextGreatProphet, equal (200) | `religion.rs` |
| `NEXT_PROPHET_FAITH` | `100` | The step between two Great Prophet prices grows by this much each time. | Unciv: ReligionManager.faithForNextGreatProphet, equal (100) | `religion.rs` |
| `PROPHET_BASE_CHANCE` | `5` | Base chance (percent) of a Great Prophet appearing from faith each turn, plus one per faith above the price. | Unciv: logic/civilization/managers/ ReligionManager.kt (`5 + stored faith - prophet cost`); equal | `religion.rs` |
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
| `ALL` | `[ CityStateBlock::NotACivilization, CityStateBlock::NotACityState, CityStateBlock::NotM...` |  | own value (source not recorded) | `block.rs` |
| `BUYOUT_COST` | `500` | Gold base price of buying out a city-state at Standard speed. | Unciv: logic/civilization/diplomacy/CityStateFunctions.kt (`getDiplomaticMarriageCost`, 500); equal | `buyout.rs` |
| `BUYOUT_ROUNDS` | `5` | Rounds of alliance before the buyout is possible. | Unciv: Nations.json (Gods & Kings, Austria: allied for 5 turns); equal | `buyout.rs` |
| `SCRAP_GOLD_DIVISOR` | `20` | Scrap gold of a unit: its gold price `(30 × cost)^0.75` divided by this. | Unciv: models/ruleset/unit/BaseUnit.kt (`getDisbandGold`, / 20) and IConstruction.kt (`getBaseGoldCost`)... | `buyout.rs` |
| `GOLD_GIFT_EXPONENT` | `1.01` | Exponent and divisor of the influence a gold gift gives: `gold^1.01 / 9.8`. | Unciv: logic/civilization/diplomacy/ CityStateFunctions.kt (`influenceGainedByGift`); equal | `gifts.rs` |
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
| `CAN_PLEDGE_THRESHOLD` | `0` | Influence needed to pledge to protect; rounds after a withdrawn or broken pledge before a new one, rounds before a pledge may be withdrawn. | Unciv: logic/civilization/diplomacy/CityStateFunctions.kt (influence at least 0, RecentlyWithdrewProtect... | `gifts.rs` |
| `PLEDGE_AFTER_BROKEN` | `20` | Unciv: CityStateFunctions.kt, equal. | Unciv: CityStateFunctions.kt, equal | `gifts.rs` |
| `PLEDGE_MIN_ROUNDS` | `10` | Unciv: CityStateFunctions.kt, equal. | Unciv: CityStateFunctions.kt, equal | `gifts.rs` |
| `PLEDGE_WITHDRAW_DROP` | `-2000` | Influence (×100) a major loses when it withdraws its pledge. | Unciv: CityStateFunctions.kt (`removeProtectorCiv`, -20); equal | `gifts.rs` |
| `TRIBUTE_GLOBAL_MODIFIER` | `100` | Highest global and local parts of the bully score. | Unciv: models/ModConstants.kt (`tributeGlobalModifier`, `tributeLocalModifier`, both 100); equal | `gifts.rs` |
| `TRIBUTE_LOCAL_MODIFIER` | `100` | Unciv: models/ModConstants.kt (`tributeLocalModifier` 100), equal. | Unciv: models/ModConstants.kt (`tributeLocalModifier` 100), equal | `gifts.rs` |
| `FRIENDS_THRESHOLD` | `30` | Influence at which a city-state counts a major as a friend and as an ally (the types carry the same). | Unciv: DiplomacyManager.relationshipLevel, 30 and 60; equal | `mod.rs` |
| `ALLIES_THRESHOLD` | `60` | Unciv: DiplomacyManager.relationshipLevel (60), equal. | Unciv: DiplomacyManager.relationshipLevel (60), equal | `mod.rs` |
| `AT_WAR_INFLUENCE` | `-60` | Effective influence at war, the floor. | Unciv: DiplomacyManager, MINIMUM_INFLUENCE, -60; equal | `mod.rs` |
| `ANCHOR_PROTECTED` | `10` | Shift of the resting point for a major the city-state is protected by, and for one it is wary of. | Unciv: logic/civilization/diplomacy/ DiplomacyManager.kt (`getCityStateInfluenceRestingPoint`: protector... | `mod.rs` |
| `ANCHOR_WARY` | `-20` | Unciv: DiplomacyManager.kt (wary -20), equal. | Unciv: DiplomacyManager.kt (wary -20), equal | `mod.rs` |
| `AGGRESSOR_THRESHOLD` | `2` | Attacks on city-states after which a major counts as an aggressor, and as a warmonger. | Unciv: logic/civilization/Civilization.kt (`isMinorCivAggressor` 2, `isMinorCivWarmonger` 4); equal | `mod.rs` |
| `WARMONGER_THRESHOLD` | `4` | Unciv: Civilization.kt (`isMinorCivWarmonger` 4), equal. | Unciv: Civilization.kt (`isMinorCivWarmonger` 4), equal | `mod.rs` |
| `WARY_AGGRESSOR_CHANCE` | `50` | Chance (percent) that an aggressor makes the city-state wary of it. | Unciv: CityStateFunctions.kt (`cityStateAttacked`, 50 %); equal | `mod.rs` |
| `FIRST_QUEST_ROUND` | `30` | First round a quest may start, and the random extra rounds added to it. | Unciv: logic/civilization/managers/quests/QuestManager.kt (30, 20); equal | `quests.rs` |
| `FIRST_QUEST_RAND` | `20` | Unciv: QuestManager.kt, equal. | Unciv: QuestManager.kt, equal | `quests.rs` |
| `PERSONAL_MIN_BETWEEN` | `20` | Least rounds between two personal quests, between two global quests, and the random extra rounds. | Unciv: QuestManager.kt (20, 40, 25); equal | `quests.rs` |
| `GLOBAL_MIN_BETWEEN` | `40` | Unciv: QuestManager.kt, equal. | Unciv: QuestManager.kt, equal | `quests.rs` |
| `RAND_BETWEEN` | `25` | Unciv: QuestManager.kt, equal. | Unciv: QuestManager.kt, equal | `quests.rs` |
| `MAX_PERSONAL` | `2` | Active personal quests per civilization and city-state. | Unciv: QuestManager.kt (2); equal | `quests.rs` |
| `CONTEST_LENGTH` | `30` | Rounds a contest quest lasts at Standard speed. | Unciv: Quests.json (Gods & Kings, duration 30); equal | `quests.rs` |
| `KILL_CAMP_RANGE` | `8` | Distance (tiles) from the city-state's capital within which a barbarian camp may be given as a quest. | Unciv: logic/civilization/managers/quests/ QuestTargetHelpers.kt (8); equal | `quests.rs` |
| `RECENT_BULLY_ROUNDS` | `20` | Bullying this recent revokes personal quests (rounds). | own value (source not recorded) | `quests.rs` |
| `DROP_PER_TURN` | `-100` | Influence change per round, ×100: the decay above the resting point, its hostile and aggressor variants (hostile wins), and the recovery below it. | Unciv: logic/civilization/diplomacy/DiplomacyManager.kt (`getCityStateInfluenceDegrade`: 1, 1.5, 2) and... | `turn.rs` |
| `DROP_PER_TURN_HOSTILE` | `-150` | Unciv: DiplomacyManager.kt and DiplomacyTurnManager.kt, equal. | Unciv: DiplomacyManager.kt and DiplomacyTurnManager.kt, equal | `turn.rs` |
| `DROP_PER_TURN_AGGRESSOR` | `-200` | Unciv: DiplomacyManager.kt and DiplomacyTurnManager.kt, equal. | Unciv: DiplomacyManager.kt and DiplomacyTurnManager.kt, equal | `turn.rs` |
| `RECOVERY_PER_TURN` | `100` | Unciv: DiplomacyManager.kt and DiplomacyTurnManager.kt, equal. | Unciv: DiplomacyManager.kt and DiplomacyTurnManager.kt, equal | `turn.rs` |
| `CONTACT_GOLD_FIRST` | `30` | Gold for the first major to meet a city-state, and for each later one; faith from a city-state that gives faith as a bonus (Religious). | Unciv: logic/civilization/diplomacy/DiplomacyFunctions.kt (`makeCivilizationsMeet`: 15 gold, doubled for... | `turn.rs` |
| `CONTACT_GOLD_OTHER` | `15` | Unciv: DiplomacyFunctions.kt (`makeCivilizationsMeet`), equal. | Unciv: DiplomacyFunctions.kt (`makeCivilizationsMeet`), equal | `turn.rs` |
| `CONTACT_FAITH` | `4` | Unciv: DiplomacyFunctions.kt (`makeCivilizationsMeet`), equal. | Unciv: DiplomacyFunctions.kt (`makeCivilizationsMeet`), equal | `turn.rs` |
| `INTRUSION_PER_UNIT` | `-1000` | Influence lost (×100 per round) while any military unit of a non-friend stands on the city-state's land, whatever their number. | Unciv: logic/civilization/diplomacy/CityStateFunctions.kt (`updateDiplomaticRelationshipForCityState`, -... | `turn.rs` |
| `UNIT_SPAWN_BIAS` | `100` | Percent of the full interval the first gifted unit waits. | Unciv: logic/civilization/diplomacy/DiplomacyTurnManager.kt (the first countdown is the full `N + -1..1`... | `turn.rs` |
| `SAME_RELIGION_DECAY_PERCENT` | `-25` | Percent change of the decay, and of the recovery, while the city-state's capital follows the major's religion as majority. | Unciv: logic/civilization/diplomacy/DiplomacyManager.kt and DiplomacyTurnManager.kt (-25 % decay, +50 %... | `turn.rs` |
| `SAME_RELIGION_RECOVERY_PERCENT` | `50` | Unciv: DiplomacyTurnManager.kt (+50 % recovery), equal. | Unciv: DiplomacyTurnManager.kt (+50 % recovery), equal | `turn.rs` |
| `ALLY_GREAT_PERSON_BASE` | `37` | Great person gifts from allied city-states: base rounds, random extra rounds (0..6), the percent for the first gift, and the cap on the rounds each ally takes off (the gift comes once the count is below `min(allies, 10)`; the count runs only while there is an ally). | Unciv: logic/civilization/diplomacy/CityStateFunctions.kt (`turnsForGreatPersonFromCityState`), managers... | `turn.rs` |
| `ALLY_GREAT_PERSON_RANDOM` | `7` | Unciv: CityStateFunctions.kt, equal. | Unciv: CityStateFunctions.kt, equal | `turn.rs` |
| `ALLY_GREAT_PERSON_FIRST_PERCENT` | `50` | Unciv: CityStateFunctions.kt, equal. | Unciv: CityStateFunctions.kt, equal | `turn.rs` |
| `ALLY_GREAT_PERSON_MAX_ALLIES_CHANGE` | `-10` | Unciv: CityStateFunctions.kt, equal. | Unciv: CityStateFunctions.kt, equal | `turn.rs` |
| `ALL` | `[ Personality::Friendly, Personality::Neutral, Personality::Hostile, Personality::Irrat...` |  | own value (source not recorded) | `city_states.rs` |

### Diplomacy

14 constants (Unciv 5, own value 6, own value (source not recorded) 3). Files: `classic/crates/classic-rules/src/systems/diplomacy/deals.rs`, `classic/crates/classic-rules/src/systems/diplomacy/mod.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `MAX_DEAL_ITEMS` | `12` | At most this many items in one deal. | own value | `deals.rs` |
| `OFFER_ROUNDS` | `1` | An offer waits this many rounds for its answer. | own value | `deals.rs` |
| `EMBASSY_REVEAL_RADIUS` | `2` | An embassy reveals the tiles this close to the host's capital (our rule: public descriptions name the capital and the tiles around it, no number). | own value (source not recorded) | `deals.rs` |
| `ALL` | `[ DealBlock::NotOurDeal, DealBlock::GoldNeedsFriendship, DealBlock::NotEnoughGold, Deal...` |  | own value (source not recorded) | `deals.rs` |
| `DEAL_DURATION` | `30` | Rounds a timed deal lasts at Standard speed: gold per turn, resources, open borders, defensive pacts, research agreements. | Unciv: Speeds.json (Gods & Kings ruleset), Standard dealDuration; equal | `mod.rs` |
| `PEACE_DURATION` | `10` | Rounds after a peace treaty during which no new war may start, at Standard speed. | Unciv: Speeds.json, Standard peaceDealDuration; equal | `mod.rs` |
| `RELATIONSHIP_DURATION` | `30` | Rounds a declaration of friendship or a denunciation lasts. | Unciv: logic/civilization/diplomacy/DiplomacyManager.kt (both flags set to 30 turns, not scaled by speed... | `mod.rs` |
| `WARMONGER_ATTACKED_WEIGHT` | `250` | Warmonger weight of an attack on a major or a city-state (the same for both). | own value | `mod.rs` |
| `WARMONGER_DECAY` | `-5` | Change of the warmonger memory per round (negative: it fades). | own value | `mod.rs` |
| `WARMONGER_NUKE_WEIGHT` | `400` | Warmonger weight of a nuclear weapon of level 1; level 2 weighs twice. | own value (source not recorded) | `mod.rs` |
| `EXPECTED_CITY_COUNT` | `52` | Cities expected on a standard-size map; it scales the warmonger weight of a captured city. | own value | `mod.rs` |
| `WARMONGER_CITY_STATE_PERCENT` | `50` | Percent of the warmonger weight kept when the victim is a city-state. | own value | `mod.rs` |
| `RESEARCH_AGREEMENT_POOL_DIVISOR` | `3` | Divisor of the boost a research agreement gives (the smaller of the two partners' science during the agreement, / 3), and the base percent of it paid (50, plus the player's research-agreement percent). | Unciv: logic/civilization/diplomacy/DiplomacyTurnManager.kt and managers/TechManager.kt (`scienceFromRes... | `mod.rs` |
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
| `ELECTION_GAIN` | `20` | Influence gained by the winner of a rigged election and lost by every other major that knows the city-state (by every rigging major when the neutral party wins); the votes of the neutral party. | Unciv: logic/civilization/diplomacy/CityStateFunctions.kt (`holdElections`); equal | `espionage.rs` |
| `ELECTION_LOSS` | `5` | Unciv: CityStateFunctions.kt (`holdElections`), equal. | Unciv: CityStateFunctions.kt (`holdElections`), equal | `espionage.rs` |
| `ELECTION_NEUTRAL_VOTES` | `20` | Unciv: CityStateFunctions.kt (`holdElections`), equal. | Unciv: CityStateFunctions.kt (`holdElections`), equal | `espionage.rs` |
| `COUP_BASE_CHANCE` | `50` | Coup: base chance (percent), minus half the influence gap to the ally, plus half the skill gap (skill `RANK_SKILL_PERCENT` × (rank + 1) of the spy against the ally's spy in the city), capped at 85; on success the spy's owner takes the ally's influence, the ally loses 20 and every other major 10; on failure the spy dies and its owner loses 20. | Unciv: models/Spy.kt (`getCoupChanceOfSuccess`, `initiateCoup`); equal | `espionage.rs` |
| `COUP_MAX_CHANCE` | `85` | Unciv: models/Spy.kt (coup: cap 85, ally -20, others -10, failure -20), equal. | Unciv: models/Spy.kt (coup: cap 85, ally -20, others -10, failure -20), equal | `espionage.rs` |
| `COUP_ALLY_DROP` | `20` | Unciv: models/Spy.kt (coup: cap 85, ally -20, others -10, failure -20), equal. | Unciv: models/Spy.kt (coup: cap 85, ally -20, others -10, failure -20), equal | `espionage.rs` |
| `COUP_OTHERS_DROP` | `10` | Unciv: models/Spy.kt (coup: cap 85, ally -20, others -10, failure -20), equal. | Unciv: models/Spy.kt (coup: cap 85, ally -20, others -10, failure -20), equal | `espionage.rs` |
| `COUP_FAIL_DROP` | `20` | Unciv: models/Spy.kt (coup: cap 85, ally -20, others -10, failure -20), equal. | Unciv: models/Spy.kt (coup: cap 85, ally -20, others -10, failure -20), equal | `espionage.rs` |
| `RANK_SKILL_PERCENT` | `30` | Skill percent per spy rank (rank 0 here is Unciv's rank 1). | Unciv: models/ModConstants.kt (`spyRankSkillPercentBonus` 30); equal | `espionage.rs` |
| `RENAISSANCE` | `3` | Era index of the Renaissance: every civilization gets a spy once a civilization enters it. | Unciv: Eras.json (Renaissance era, "Every major Civilization gains a spy once a civilization enters this... | `espionage.rs` |
| `INDUSTRIAL` | `4` | Era index of the Industrial era: from it on a civilization gets one more spy as it enters each era. | own design | `espionage.rs` |
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
| `SITES_PER_CIV` | `(4, 6)` | Dig sites per major nation: (fewest, most). | own design | `archaeology.rs` |
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
| `SESSIONS` | `[ SpecialSession { era: 3, turns_between: 28, civ_delegates: 2, host_delegates: 1, city...` | The three special sessions: the Assembly (Renaissance), the Council (Industrial era, the city-states get a seat) and the United Nations (Atomic era, the World Leader vote). | own design | `congress.rs` |
| `LEADERS_BONUS_ON_FAIL` | `1` | Extra delegates each of the two leading candidates keeps for good when the World Leader vote fails. | own design | `congress.rs` |
| `FOUNDING_MET_PERCENT` | `50` | Percent a nation must have met of the other civilizations (rounded up, at least one) to found the World Congress with the technology that allows it. | own design | `congress.rs` |
| `ARMY_TAX_PERCENT` | `20` | Percent added to unit maintenance while the Standing Army Tax is in force. | own design | `congress.rs` |
| `SCHOLARS_PERCENT` | `12` | Percent added to the science of a nation researching a technology another civilization knows while Scholars in Residence is in force. | own design | `congress.rs` |
| `HERITAGE_CULTURE_PER_WONDER` | `2` | Culture per turn each world wonder gives while the Cultural Heritage Sites resolution is in force. | own design | `congress.rs` |
| `FUNDING_PERCENT` | `25` | Percent the arts (writers, artists, musicians) gain and the industries (scientists, engineers, merchants) lose in great person points under Arts Funding, and the reverse under Sciences Funding. | own design | `congress.rs` |
| `VICTORY_SHARE_PERCENT` | `67` | Share of the available votes the World Leader needs: with `n` votes, `n × (67 - floor(1.1 n)) / 100 + 1` up to 28 votes, `n × 35 / 100` above. | Unciv: logic/civilization/managers/VictoryManager.kt (`votesNeededForDiplomaticVictory`, applied, as the... | `congress.rs` |
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
| `SCORE_CITY` | `10` | Score points per city, citizen, land tile, wonder and technology. | Unciv: logic/civilization/Civilization.kt (`calculateScoreBreakdown`: city 10, land tile 1, technology 4... | `victory.rs` |
| `SCORE_POPULATION` | `3` | Unciv: Civilization.kt and ModConstants.kt (score), equal. | Unciv: Civilization.kt and ModConstants.kt (score), equal | `victory.rs` |
| `SCORE_LAND` | `1` | Unciv: Civilization.kt and ModConstants.kt (score), equal. | Unciv: Civilization.kt and ModConstants.kt (score), equal | `victory.rs` |
| `SCORE_WONDER` | `40` | Unciv: Civilization.kt and ModConstants.kt (score), equal. | Unciv: Civilization.kt and ModConstants.kt (score), equal | `victory.rs` |
| `SCORE_TECH` | `4` | Unciv: Civilization.kt and ModConstants.kt (score), equal. | Unciv: Civilization.kt and ModConstants.kt (score), equal | `victory.rs` |
| `STANDARD_MAP_PLOTS` | `4160` | Plots of a Standard map (the reference size of the map score modifier: 80 × 52). | Unciv: logic/civilization/Civilization.kt (reference / tiles, above 1 damped to (m - 1) / 3 + 1), whose... | `victory.rs` |
| `ALL` | `[ VictoryKind::Time, VictoryKind::Science, VictoryKind::Domination, VictoryKind::Cultur...` |  | own value (source not recorded) | `victory.rs` |
| `STANDARD_MAX_ROUNDS` | `500` | The last round of a game at Standard speed. | Unciv: Speeds.json (Standard, 500 turns), equal | `victory.rs` |

### Map, terrain and tile yields

26 constants (Unciv 5, own value (source not recorded) 21). Files: `classic/crates/classic-rules/src/classic/yields.rs`, `platform/crates/kernel/src/derived/tile_yields.rs`, `platform/crates/kernel/src/derived/yields.rs`, `platform/crates/kernel/src/rules/natural_wonders.rs`, `platform/crates/kernel/src/scenario/generator.rs`, `platform/crates/kernel/src/scenario/landscape.rs`, `platform/crates/kernel/src/scenario/map_script.rs`, `platform/crates/kernel/src/scenario/sea_resources.rs`, `platform/crates/kernel/src/world/map.rs`, `platform/crates/kernel/src/world/size.rs`.

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
| `ALL` | `[ MapScript::Continents, MapScript::Pangaea, MapScript::Archipelago, MapScript::Fractal...` |  | own value (source not recorded) | `map_script.rs` |
| `SHALLOW_REACH` | `2` | Hex distance from land within which water is shallow: the coast and the ring behind it. | own value (source not recorded) | `sea_resources.rs` |
| `START_REACH` | `3` | Tiles around a start that its first city works: the sea within this reach is the start's own. | own value (source not recorded) | `sea_resources.rs` |
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

34 constants (Unciv 2, own design 1, own value (source not recorded) 31). Files: `classic/crates/classic-rules/src/classic/content.rs`, `classic/crates/classic-rules/src/classic/effects.rs`, `classic/crates/classic-rules/src/classic/tables.rs`, `platform/crates/kernel/src/rules/compiled.rs`, `platform/crates/kernel/src/rules/content.rs`, `platform/crates/kernel/src/rules/effects.rs`, `platform/crates/kernel/src/rules/modifiers.rs`, `platform/crates/kernel/src/rules/roles.rs`.

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
| `IMPROVE_ROUNDS` | `8` | Rounds a worker takes for one improvement, the walk included. | own value (source not recorded) | `allocator.rs` |
| `FIRST_WORKER_FACTOR` | `3` | The empire's first worker is worth this many times the yields of the jobs it takes, once a settler is on the way (or the second city stands): its first improvements, a luxury among them, come a settler's cycle sooner (600 seats at round 60: excess happiness +7.0 instead of +3.9; bench `pace`: cities at round 100 3.6 instead of 3.1; platform/docs/ai-bench.md, "Happiness and the first worker"). | own value (source not recorded) | `allocator.rs` |
| `BUILDING_TURNS` | `40` | A building taking longer than this is not ordered. | own value (source not recorded) | `allocator.rs` |
| `WONDER_TURNS` | `60` | A world wonder taking longer than this is not ordered. | own value (source not recorded) | `allocator.rs` |
| `WONDER_RISK_PER_TURN` | `2` | A wonder's chance to be ours falls by this many percent per turn it takes (another civilization may finish it first). | own value (source not recorded) | `allocator.rs` |
| `WONDER_MIN_CHANCE` | `30` | The least chance a long wonder keeps, percent. | own value (source not recorded) | `allocator.rs` |
| `TILES_PER_TURN` | `2` | Tiles bought in one turn. | own value (source not recorded) | `allocator.rs` |

### `barbarians.rs`

1 constants (own value (source not recorded) 1). Files: `classic/crates/classic-rules/src/ai/barbarians.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `HUNT_RANGE` | `8` | Barbarians go after targets no farther than this. | own value (source not recorded) | `barbarians.rs` |

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

### `citizens.rs`

5 constants (own value (source not recorded) 5). Files: `classic/crates/classic-rules/src/ai/citizens.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `SMALL_CITY` | `4` | Population below which a city favours food. | own value (source not recorded) | `citizens.rs` |
| `GOLD_COMFORT` | `3` | Gold a turn a gold-focused empire earns before its cities go back to their usual focus. | own value (source not recorded) | `citizens.rs` |
| `GOLD_FOCUS_TURNS` | `20` | Turns of deficit the treasury must cover before the cities work gold. | own value (source not recorded) | `citizens.rs` |
| `AVOID_GROWTH_AT` | `-2` | Excess happiness at which the cities stop growing. | own value (source not recorded) | `citizens.rs` |
| `RESUME_GROWTH_AT` | `2` | Excess happiness at which they grow again. | own value (source not recorded) | `citizens.rs` |

### `city_states.rs`

3 constants (own value (unverified) 2, own value (source not recorded) 1). Files: `classic/crates/classic-rules/src/ai/city_states.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `GIFT_RESERVE` | `300` | Gold the AI keeps before it gives any away. | own value (source not recorded) | `city_states.rs` |
| `SMALL_GIFT` | `250` | The gift sizes (own values, unverified). | own value (unverified) | `city_states.rs` |
| `MEDIUM_GIFT` | `500` | (with `SMALL_GIFT`) The gift sizes (own values, unverified). | own value (unverified) | `city_states.rs` |

### `combat.rs`

2 constants (own value (source not recorded) 2). Files: `classic/crates/classic-rules/src/ai/combat.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `ANNEX_POPULATION` | `4` | Citizens of a captured city the AI annexes; a smaller one is a puppet. | own value (source not recorded) | `combat.rs` |
| `UPGRADE_RESERVE` | `150` | Gold the military advisor keeps when it upgrades units. | own value (source not recorded) | `combat.rs` |

### `defence.rs`

5 constants (own value (source not recorded) 5). Files: `classic/crates/classic-rules/src/ai/defence.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `THREAT_RADIUS` | `crate::ai::planner_strategy::THREAT_RADIUS` | Hostile units this close to an own city threaten it. | own value (source not recorded) | `defence.rs` |
| `GUARD_RADIUS` | `2` | Own units this close to a city guard it. | own value (source not recorded) | `defence.rs` |
| `RESPONSE_ROUNDS` | `crate::ai::front::SIEGE_ROUNDS` | A unit joins a guard it reaches within this many rounds: a ready siege takes a city within the front's `SIEGE_ROUNDS`. | own value (source not recorded) | `defence.rs` |
| `STRANGER_PERCENT` | `50` | Weight of the field units of a civilization at peace that has done nothing to lose our trust, percent of an enemy's: an army gathering at our border for many rounds before its war is the warning (bench probe: 5–20 rounds), a few passing units count for little (`chance`). | own value (source not recorded) | `defence.rs` |
| `CHANCE_POWER` | `4` | The power of the ratio of threat and guard in `chance`. | own value (source not recorded) | `defence.rs` |

### `diplomacy.rs`

6 constants (own value (unverified) 1, own value (source not recorded) 5). Files: `classic/crates/classic-rules/src/ai/diplomacy.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `FRIENDSHIP_MIN_CONTACT_ROUNDS` | `20` | No friendship before this many rounds of contact; own value, unverified. | own value (unverified) | `diplomacy.rs` |
| `ACCEPT_PEACE_AFTER_ROUNDS` | `5` | The AI accepts a peace (and proposes one to a rival in the diplomacy advisor) only after this many rounds of war. | own value (source not recorded) | `diplomacy.rs` |
| `DISTRUST_WARMONGER` | `100` | Warmonger memory above which the AI distrusts (no friendship, pacts). | own value (source not recorded) | `diplomacy.rs` |
| `DENOUNCE_WARMONGER` | `250` | Warmonger memory above which the AI denounces. | own value (source not recorded) | `diplomacy.rs` |
| `RECENT_WAR_ROUNDS` | `30` | No friendly deal with a former enemy for this many rounds after the peace treaty ends. | own value (source not recorded) | `diplomacy.rs` |
| `PROPOSAL_INTERVAL` | `5` | Rounds between two rounds of proposals of one player. | own value (source not recorded) | `diplomacy.rs` |

### `economy.rs`

4 constants (own value (source not recorded) 4). Files: `classic/crates/classic-rules/src/ai/economy.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `GREAT_PERSON_PRIORITY` | `20` | Priority of a great person action (before ordinary economy orders). | own value (source not recorded) | `economy.rs` |
| `HAPPINESS_PRIORITY` | `10` | Priority of a happiness building order. | own value (source not recorded) | `economy.rs` |
| `TRADE_PRIORITY` | `8` | Priority of a trade route and of a trade unit order. | own value (source not recorded) | `economy.rs` |
| `FOCUS_PRIORITY` | `5` | Priority of a city focus change. | own value (source not recorded) | `economy.rs` |

### `espionage.rs`

2 constants (own value (source not recorded) 2). Files: `classic/crates/classic-rules/src/ai/espionage.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `SLOW_THEFT_TURNS` | `50` | A theft slower than this is not worth the spy: it looks for another city. | own value (source not recorded) | `espionage.rs` |
| `COUP_MIN_CHANCE` | `60` | Coups the AI tries (percent chance of success). | own value (source not recorded) | `espionage.rs` |

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
| `HEAL_BELOW` | `50` | A member below this health leaves the front to heal. | own value (source not recorded) | `front.rs` |
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
| `ROUNDS_PER_CAPITAL` | `30` | Rounds one war operation takes: forming, marching, the siege. | own value (source not recorded) | `grand_strategy.rs` |
| `NEAR_CAPITAL_CHANCE` | `350` | Chance one war operation takes a rival's original capital known within its reach (`OBJECTIVE_RANGE` of an own city), and farther or not yet found, permille (step 3: of the operations that marched with their army, about a third took their city). | own value (source not recorded) | `grand_strategy.rs` |
| `FAR_CAPITAL_CHANCE` | `150` |  | own value (source not recorded) | `grand_strategy.rs` |
| `REACHABLE_CHANCE` | `500` | Chance of a science, culture or diplomatic victory in reach with no time to spare, permille (rivals race too); more with time to spare. | own value (source not recorded) | `grand_strategy.rs` |
| `SPACESHIP_ROUNDS` | `25` | Rounds the spaceship takes once its technologies are known. | own value (source not recorded) | `grand_strategy.rs` |
| `WAR_STEP` | `20` | Percent of a war's weight one point of aggression or war tolerance adds. | own value (source not recorded) | `grand_strategy.rs` |

### `greedy.rs`

1 constants (own value (source not recorded) 1). Files: `classic/crates/classic-rules/src/ai/greedy.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `PEACE_AFTER_UNSEEN_ROUNDS` | `5` | Rounds without sight of an enemy after which the AI makes peace. | own value (source not recorded) | `greedy.rs` |

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
| `CENTI_PER_POINT` | `6` | Hundredths of a score point one point of warmonger memory is worth: a civilization that remembers 400 points (one strike) distrusts, denounces and will not make peace but for 24 points of compensation (own design, not tuned: a city of six citizens is about 100 points). | own value (source not recorded) | `infamy.rs` |
| `WAR_BONUS_PERCENT` | `50` | The part of the price of peace that is a bonus for war, percent: the world forgives a war on an aggressor, it does not pay for it. | own value (source not recorded) | `infamy.rs` |

### `legal.rs`

1 constants (own value (source not recorded) 1). Files: `classic/crates/classic-rules/src/ai/legal.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `MAX_REPLANS` | `4` | Times a city plans again after its choice was refused. | own value (source not recorded) | `legal.rs` |

### `operations.rs`

31 constants (own value (source not recorded) 31). Files: `classic/crates/classic-rules/src/ai/operations.rs`.

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
| `DECISIVE_FORM_LIMIT` | `2 * FORM_LIMIT` | Rounds the army of a capture that wins the game may take to form. | own value (source not recorded) | `operations.rs` |
| `DECISIVE_RANGE` | `32` | The farthest a capture that wins the game may lie from our nearest city: rival capitals lie mostly 16–30 tiles from it on the duel suite, where `OBJECTIVE_RANGE` left three quarters of the known ones out of reach (classic/docs/ai-leaders.md "Capital search"); the route to the staging tile and the horizon still decide. | own value (source not recorded) | `operations.rs` |
| `PLAN_LIMIT` | `40` | A war plan waits at most this many rounds for its chest. | own value (source not recorded) | `operations.rs` |
| `PLAN_KEEP_PERCENT` | `70` | A plan is dropped when its target is worth less than this share of its value at the choice, percent. | own value (source not recorded) | `operations.rs` |
| `PLAN_SWITCH_PERCENT` | `130` | Another target replaces a plan only when worth this share of it. | own value (source not recorded) | `operations.rs` |
| `SUCCESS_PERCENT` | `35` | Chance an operation takes its city, percent (step 3: of the operations that marched with their army, about a third took the city or forced it). | own value (source not recorded) | `operations.rs` |
| `LOSS_PERCENT` | `50` | Share of the army an operation expects to lose, percent. | own value (source not recorded) | `operations.rs` |
| `RIVAL_PERCENT` | `50` | Share of the city's points taken from the rival that counts for us, percent: the time victory compares our score with the best other one. | own value (source not recorded) | `operations.rs` |
| `JUMP_MIN_DISTANCE` | `3` | A paratrooper jumps when the way to the muster point is at least this long (a shorter one is walked in a round or two). | own value (source not recorded) | `operations.rs` |
| `JUMP_CANDIDATES` | `7` | Tiles near the muster point one jump tries. | own value (source not recorded) | `operations.rs` |

### `pantheon.rs`

1 constants (own value (source not recorded) 1). Files: `classic/crates/classic-rules/src/ai/pantheon.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `LAND_RADIUS` | `3` | Hex radius around a city whose tiles it can work. | own value (source not recorded) | `pantheon.rs` |

### `plan.rs`

1 constants (own value (unverified) 1). Files: `platform/crates/kernel/src/ai/plan.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `WITHOUT_HANDICAPS` | `UnitSupply { free_units: 7, per_city: 3, population_percent: 50, ai_percent: 20, }` | Content without a difficulty table (old test fixtures): the AI row's supply at the default level. | own value (unverified) | `plan.rs` |

### `planner_cities.rs`

7 constants (own value (source not recorded) 7). Files: `classic/crates/classic-rules/src/ai/planner_cities.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `SLOW_ORDER_TURNS` | `40` | An order taking longer than this many turns is not chosen while something faster is wanted. | own value (source not recorded) | `planner_cities.rs` |
| `SETTLER_MIN_POPULATION` | `2` | Settlers come from cities of at least this population. | own value (source not recorded) | `planner_cities.rs` |
| `HURRY_RESERVE` | `30` | Gold kept in the treasury before hurrying. | own value (source not recorded) | `planner_cities.rs` |
| `DEFICIT_RESERVE_TURNS` | `30` | Turns of gold deficit the treasury keeps in reserve before buying. | own value (source not recorded) | `planner_cities.rs` |
| `QUEUE_TARGET` | `1` | Orders a city keeps queued. | own value (source not recorded) | `planner_cities.rs` |
| `SETTLER_AHEAD_CITIES` | `2` | While the empire has fewer cities than this (only the second city), a city that can train a settler puts it ahead of a building in its queue: the stock the building gathered (a Granary of the size-1 capital) goes into the settler. | own value (source not recorded) | `planner_cities.rs` |
| `SETTLER_AHEAD_TURNS` | `20` | A settler taking longer than this is not put ahead. | own value (source not recorded) | `planner_cities.rs` |

### `planner_research.rs`

1 constants (own value (source not recorded) 1). Files: `classic/crates/classic-rules/src/ai/planner_research.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `SEAFARING_KNOWN_PERCENT` | `50` | The map beyond the sea is mostly unknown below this share: the technologies that let units embark and cross the ocean are worth more (the other civilizations, city-states and land to settle are there). | own value (source not recorded) | `planner_research.rs` |

### `planner_settle.rs`

7 constants (own value (source not recorded) 7). Files: `classic/crates/classic-rules/src/ai/planner_settle.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `SITE_MAX_TURNS` | `12` | Sites farther than this many turns from the settler are not considered. | own value (source not recorded) | `planner_settle.rs` |
| `TRAVEL_COST` | `30` | Score points one turn of the settler's walk costs. | own value (source not recorded) | `planner_settle.rs` |
| `FREE_DISTANCE` | `4` | Tiles from the nearest own city a site is free of the distance penalty. | own value (source not recorded) | `planner_settle.rs` |
| `DISTANCE_PENALTY` | `40` | Score points a tile beyond `FREE_DISTANCE` costs. | own value (source not recorded) | `planner_settle.rs` |
| `SITE_MIN_SCORE` | `20` | A site below this score is not worth a city. | own value (source not recorded) | `planner_settle.rs` |
| `ESCORT_WAIT_ROUNDS` | `10` | Rounds a settler that met danger waits for an escort before it gives its site up. | own value (source not recorded) | `planner_settle.rs` |
| `DANGER_RANGE` | `3` | No site this close to a hostile military unit in sight. | own value (source not recorded) | `planner_settle.rs` |

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
| `SCOUTING_ENDS_ROUND` | `150` | After this round scouting stops (the known world is what matters). | own value (source not recorded) | `planner_units.rs` |
| `GARRISON_FROM_ROUND` | `10` | From this round every city wants a garrison even in peace. | own value (source not recorded) | `planner_units.rs` |
| `GARRISON_ALONE_ROUND` | `40` | From this round every city wants a garrison, even with nobody met. | own value (source not recorded) | `planner_units.rs` |
| `HEAL_BELOW_HP` | `50` | Health below which a unit goes to heal. | own value (source not recorded) | `planner_units.rs` |

### `planner_value.rs`

3 constants (own value (source not recorded) 3). Files: `classic/crates/classic-rules/src/ai/planner_value.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `DEFICIT_GOLD_WEIGHT` | `4` | Added to the gold weight at a deficit. | own value (source not recorded) | `planner_value.rs` |
| `UNHAPPY_HAPPINESS_WEIGHT` | `6` | Added to the happiness weight while the empire is unhappy. | own value (source not recorded) | `planner_value.rs` |
| `STRATEGIC_RESOURCE_WEIGHT` | `3` | Yield-equivalents of connecting a strategic resource, per military weight. | own value (source not recorded) | `planner_value.rs` |

### `planner_workers.rs`

4 constants (own value (source not recorded) 4). Files: `classic/crates/classic-rules/src/ai/planner_workers.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `REPLACE_GAIN` | `20` | A new improvement replaces an existing one only for this much more. | own value (source not recorded) | `planner_workers.rs` |
| `ROADS_FROM_ROUND` | `40` | Roads are built from this round on, with this much gold income. | own value (source not recorded) | `planner_workers.rs` |
| `ROADS_MIN_INCOME` | `3` |  | own value (source not recorded) | `planner_workers.rs` |
| `ROAD_CITY_RANGE` | `7` | Cities this close are joined by roads. | own value (source not recorded) | `planner_workers.rs` |

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

### `religion.rs`

1 constants (own value (source not recorded) 1). Files: `classic/crates/classic-rules/src/ai/religion.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `MISSIONARIES_WANTED` | `2` | Missionaries an AI keeps out at most. | own value (source not recorded) | `religion.rs` |

### `scouting.rs`

17 constants (own value (source not recorded) 17). Files: `classic/crates/classic-rules/src/ai/scouting.rs`.

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
| `SECOND_SCOUT_ROUND` | `12` | The second scout is wanted from this round while less is explored. | own value (source not recorded) | `scouting.rs` |
| `SECOND_SCOUT_UNTIL_ROUND` | `100` |  | own value (source not recorded) | `scouting.rs` |
| `SECOND_SCOUT_BELOW_PERCENT` | `40` |  | own value (source not recorded) | `scouting.rs` |
| `RECON_FROM_ROUND` | `30` | The first recon unit is built from this round (before it the starting warrior explores and the capital builds workers and settlers: a scout at round 12 cost the empires a tenth of their first workers and a third of their luxury kinds at round 60, `aurelia-qa` `happiness_pace`). | own value (source not recorded) | `scouting.rs` |
| `RECON_UNITS` | `1` | Recon units the empire builds for scouting. | own value (source not recorded) | `scouting.rs` |

### `sea_work.rs`

1 constants (own value (source not recorded) 1). Files: `classic/crates/classic-rules/src/ai/sea_work.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `SEA_JOB_ROUNDS` | `4` | Rounds a work boat takes to reach its deposit and finish the job. | own value (source not recorded) | `sea_work.rs` |

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

### `tactics.rs`

5 constants (own value (source not recorded) 5). Files: `classic/crates/classic-rules/src/ai/tactics.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `ATTACK_MIN_HP` | `40` | A unit attacks only with at least this much health. | own value (source not recorded) | `tactics.rs` |
| `RETREAT_BELOW_HP` | `35` | A unit this hurt next to an enemy falls back. | own value (source not recorded) | `tactics.rs` |
| `WEAK_CITY_HP` | `50` | A city at or below this health is attacked without waiting for a group. | own value (source not recorded) | `tactics.rs` |
| `SIEGE_GROUP` | `3` | Units of the assault group that gather before the siege starts. | own value (source not recorded) | `tactics.rs` |
| `RALLY_ROUNDS` | `8` | Rounds an assault unit waits for its group before it attacks anyway. | own value (source not recorded) | `tactics.rs` |

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

### `trade.rs`

1 constants (own value (source not recorded) 1). Files: `classic/crates/classic-rules/src/ai/trade.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `TRADE_FROM_POPULATION` | `8` | Population of the largest city before the first trade unit. | own value (source not recorded) | `trade.rs` |

### `value.rs`

15 constants (own value (source not recorded) 15). Files: `classic/crates/classic-rules/src/ai/value.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `BASE` | `Unhappiness { city: crate::derived::happiness::UNHAPPINESS_FROM_CITY_TIMES100, citizen:...` | The rules' base rates (`derived::happiness`: unhappiness per city and per citizen), before the empire's modifiers. | own value (source not recorded) | `value.rs` |
| `HAPPINESS_COMFORT` | `6` | Excess happiness at which one more point stops being worth a citizen: below it growth stalls soon (−75 % food while unhappy). | own value (source not recorded) | `value.rs` |
| `TILE_PRICE` | `75` | Gold a bought tile costs on average: an own estimate (Unciv's tile price, `CityExpansionManager.getGoldCostOfTile`, is 50 per ring beyond the first plus 5 per tile the city already claimed, scaled by game speed; ours is a flat mean). | own value (source not recorded) | `value.rs` |
| `PROCESS_SHARE` | `25` | Share of production the Wealth and Research processes convert, percent. | own value (source not recorded) | `value.rs` |
| `CRISIS_CITIZENS` | `6` | Citizens a point of happiness is worth at least at the very-unhappy line (`Outlook::crisis`): enough that a happiness building or a luxury beats sitting at the line unless the other choice gains a lot of score (bench, offsets 20–21: 4 left 32 % of the seats at or below the line at the end, 6 with `Prices::local_cap` 26 %, 8 28 %, 20 no fewer and less strength; classic/docs/ai-allocator.md). | own value (source not recorded) | `value.rs` |
| `OPENING_CITIES` | `3` | The empire's first cities found the opening: their citizens count as the empire's growth (the food price), not against its happiness: a player founds them for their land. | own value (source not recorded) | `value.rs` |
| `FIRST_RING` | `7` | Land tiles of a new city: its first ring. | own value (source not recorded) | `value.rs` |
| `TILE_ROUNDS` | `25` | Rounds a city's borders take for one more tile (bench: ~18 tiles per city after 250 rounds). | own value (source not recorded) | `value.rs` |
| `CITIZEN_ROUNDS` | `25` | Rounds a new city takes for one more citizen (bench: ~10 citizens per city after 250 rounds). | own value (source not recorded) | `value.rs` |
| `NEW_CITY_PRODUCTION` | `2` | Yields of a young city a turn. | own value (source not recorded) | `value.rs` |
| `NEW_CITY_SCIENCE` | `2` |  | own value (source not recorded) | `value.rs` |
| `NEW_CITY_UNHAPPINESS` | `4` | A city and its first citizen. | own value (source not recorded) | `value.rs` |
| `MAX_CITIZENS_PER_HAPPINESS` | `10` | Most citizens one point of happiness is worth. | own value (source not recorded) | `value.rs` |
| `TECH_ROUNDS` | `9` | Rounds an empire takes for a technology (bench: 34 in 300 rounds). | own value (source not recorded) | `value.rs` |
| `UNLOCK_WONDER_CHANCE` | `50` | Percent of a wonder's surplus that counts as ours when the technology that opens it is learned: the others build wonders too. | own value (source not recorded) | `value.rs` |

### `variant.rs`

1 constants (own value (source not recorded) 1). Files: `classic/crates/classic-rules/src/ai/variant.rs`.

| Name | Value | Meaning | Source | File |
|---|---|---|---|---|
| `ALL` | `[ Variant::Standard, Variant::Previous, Variant::Strikes, Variant::Landings, Variant::U...` | The game's AI first. | own value (source not recorded) | `variant.rs` |

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
| Civilizations | `civilizations.json` | 43 | 35 | 8 | 0 | 0 |
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

### Civilizations

| Civilization | Leader | Ability | Provenance |
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
