> Obsolete: replaced by [classic/docs/classic/implemented.md](../classic/implemented.md) (what is compiled and applied) and [platform/docs/ui-render.md](../../../platform/docs/ui-render.md) (art coverage); `core.ron` no longer exists.

# Civ V: Brave New World content coverage

What of BNW is already in `classic/assets/rules/core.ron` and `civs.ron`, what has 2D art in `classic/assets/visual/generated/`, and what does not. Generated from the catalogs of this folder; "art present" means the entity id was found in the art catalog (`buildings/index.tsv`, `units/inventory-bnw.json`, `research-units/atlas-index.tsv`, `policies/catalog.tsv`, `religion/inventory.tsv`, `civilizations/completion-catalog.tsv`, `coverage.json`). The presence of art does not mean it is connected to the game.

The data in `core.ron`, `civs.ron` and the art catalogs was made earlier and is not a source of Civ V values: below they are compared with BNW, and where they differ the BNW value is the correct one.

## Summary by category

| Category | Total in BNW | Already in core.ron / civs.ron | Art present | No art | Catalog | Note |
|---|---:|---:|---:|---:|---|---|
| Nations | 43 | 10 | 43 | 0 | `civilizations.json` | art: portrait + emblem + background by slug |
| Technologies | 81 | 81 | 81 | 0 | `techs.json` |  |
| Regular units (by class) | 82 | 6 | 78 | 4 | `units.json` | including great people, 4 spaceship parts, religious and civilian units, and the Foreign Legion |
| Unique units | 59 | 0 | 2 | 57 | `unique_units.json` | art exists only for the Khan and the Venetian Merchant |
| Buildings (regular, religious, guilds, palace) | 62 | 12 | 62 | 0 | `buildings.json` |  |
| Unique buildings | 20 | 0 | 20 | 0 | `unique_buildings.json` |  |
| World wonders | 47 | 3 | 47 | 0 | `wonders.json` |  |
| National wonders | 11 | 0 | 11 | 0 | `wonders.json` |  |
| Projects | 6 | 0 | 2 | 4 | `wonders.json` | the 4 SS_* "projects" are internal victory thresholds; the art `bnw:project/spaceship` covers the whole spaceship |
| World Congress projects | 3 | 0 | 3 | 0 | `wonders.json` |  |
| Policy branches and ideologies | 12 | 0 | 12 | 0 | `policies.json` |  |
| Policies | 45 | 0 | 45 | 0 | `policies.json` |  |
| Ideology tenets | 48 | 0 | 48 | 0 | `policies.json` |  |
| Branch opener and finisher bonuses | 18 | 0 | 0 | 18 | `policies.json` | no separate art, the branch art is used |
| Beliefs | 69 | 0 | 69 | 0 | `beliefs.json` |  |
| Religions | 13 | 0 | 13 | 0 | `beliefs.json` |  |
| Resources | 42 | 2 | 0 | 42 | `resources.json` | Bison and cocoa are in the XML, but not in the game's final DB (scenario-only) |
| Tile improvements | 29 | 5 | 0 | 29 | `improvements.json` | no 2D art catalog |
| Terrain types | 9 | 0 | 0 | 9 | `terrain.json` | core.ron specifies terrain by the numbers 0..3, not by id |
| Terrain features | 8 | 1 | 0 | 8 | `features.json` |  |
| Natural wonders | 17 | 0 | 0 | 17 | `natural_wonders.json` |  |
| Promotions | 214 | 0 | 0 | 214 | `promotions.json` |  |
| Great people | 9 | 0 | 9 | 0 | `great_people.json` |  |
| City-states | 58 | 0 | 0 | 58 | `city_states.json` |  |
| Eras | 8 | 0 | 0 | 8 | `eras.json` | core.ron stores the era as the string era_name on technologies |
| **Total** | **1013** | **120** | **545** | **468** | | |

"Already in core.ron" counts an id match. A matching id does not mean matching values: see the next section.

## Discrepancies between core.ron / civs.ron and BNW

Rule: the BNW value from the catalog goes into the implementation. `core.ron` must be brought in line with the catalog when switching to data (M3.0/M5).

| Section | id | State | Discrepancy |
|---|---|---|---|
| units | `core:unit/archer` | differs | cost: core.ron 35 vs BNW 40; strength: core.ron 8 vs BNW 5; movement: core.ron 3 vs BNW 2 |
| units | `core:unit/horseman` | differs | cost: core.ron 50 vs BNW 75 |
| units | `core:unit/settler` | differs | cost: core.ron 40 vs BNW 106; strength: core.ron 1 vs BNW 0 |
| units | `core:unit/swordsman` | differs | cost: core.ron 50 vs BNW 75; strength: core.ron 15 vs BNW 14 |
| units | `core:unit/warrior` | differs | cost: core.ron 30 vs BNW 40; strength: core.ron 10 vs BNW 8 |
| units | `core:unit/worker` | differs | cost: core.ron 30 vs BNW 70; strength: core.ron 1 vs BNW 0 |
| buildings | `core:building/bank` | differs | cost: core.ron 340 vs BNW 200; effects in core.ron are placeholders (add_yield/multiply_yield) — replace with BNW effects |
| buildings | `core:building/barracks` | differs | cost: core.ron 80 vs BNW 75; effects in core.ron are placeholders (add_yield/multiply_yield) — replace with BNW effects |
| buildings | `core:building/factory` | differs | cost: core.ron 450 vs BNW 360; effects in core.ron are placeholders (add_yield/multiply_yield) — replace with BNW effects |
| buildings | `core:building/granary` | differs | effects in core.ron are placeholders (add_yield/multiply_yield) — replace with BNW effects |
| buildings | `core:building/great_library` | differs | cost: core.ron 350 vs BNW 185; effects in core.ron are placeholders (add_yield/multiply_yield) — replace with BNW effects |
| buildings | `core:building/library` | differs | cost: core.ron 110 vs BNW 75; effects in core.ron are placeholders (add_yield/multiply_yield) — replace with BNW effects |
| buildings | `core:building/lighthouse` | differs | cost: core.ron 130 vs BNW 75; effects in core.ron are placeholders (add_yield/multiply_yield) — replace with BNW effects |
| buildings | `core:building/market` | differs | cost: core.ron 150 vs BNW 100; effects in core.ron are placeholders (add_yield/multiply_yield) — replace with BNW effects |
| buildings | `core:building/monument` | differs | cost: core.ron 50 vs BNW 40; effects in core.ron are placeholders (add_yield/multiply_yield) — replace with BNW effects |
| buildings | `core:building/oracle` | differs | cost: core.ron 300 vs BNW 250; effects in core.ron are placeholders (add_yield/multiply_yield) — replace with BNW effects |
| buildings | `core:building/pyramids` | differs | cost: core.ron 250 vs BNW 185; effects in core.ron are placeholders (add_yield/multiply_yield) — replace with BNW effects |
| buildings | `core:building/shrine` | differs | cost: core.ron 70 vs BNW 40; required_tech: core.ron core:tech/calendar vs BNW core:tech/pottery; effects in core.ron are placeholders (add_yield/multiply_yield) — replace with BNW effects |
| buildings | `core:building/temple` | differs | cost: core.ron 180 vs BNW 100; required_tech: core.ron core:tech/theology vs BNW core:tech/philosophy; effects in core.ron are placeholders (add_yield/multiply_yield) — replace with BNW effects |
| buildings | `core:building/university` | differs | cost: core.ron 280 vs BNW 160; effects in core.ron are placeholders (add_yield/multiply_yield) — replace with BNW effects |
| buildings | `core:building/workshop` | differs | cost: core.ron 200 vs BNW 120; required_tech: core.ron core:tech/machinery vs BNW core:tech/metal-casting; effects in core.ron are placeholders (add_yield/multiply_yield) — replace with BNW effects |
| technologies | `core:tech/iron-working` | differs | cost: core.ron 195 vs BNW 175 |
| technologies | `core:tech/metallurgy` | differs | requires: core.ron ['core:tech/gunpowder'] vs BNW ['core:tech/gunpowder', 'core:tech/printing-press'] |
| technologies | `core:tech/dynamite` | differs | cost: core.ron 2000 vs BNW 2350 |
| technologies | `core:tech/fertilizer` | differs | cost: core.ron 1300 vs BNW 1600 |
| technologies | `core:tech/rifling` | differs | cost: core.ron 1425 vs BNW 1600; requires: core.ron ['core:tech/metallurgy'] vs BNW ['core:tech/economics', 'core:tech/metallurgy'] |
| technologies | `core:tech/ballistics` | differs | requires: core.ron ['core:tech/railroad'] vs BNW ['core:tech/flight', 'core:tech/railroad'] |
| improvements | `core:improvement/farm` | differs | required_tech: core.ron None vs BNW core:tech/agriculture; food_bonus: core.ron 2 vs BNW 1; work_turns core.ron 3 vs BNW Builds.Time 700 (hundredths of worker-turns, rules.md) |
| improvements | `core:improvement/lumber-mill` | differs | food_bonus: core.ron 1 vs BNW 0; prod_bonus: core.ron 2 vs BNW 1; work_turns core.ron 4 vs BNW Builds.Time 700 (hundredths of worker-turns, rules.md) |
| improvements | `core:improvement/mine` | differs | required_tech: core.ron None vs BNW core:tech/mining; prod_bonus: core.ron 2 vs BNW 1; work_turns core.ron 3 vs BNW Builds.Time 700 (hundredths of worker-turns, rules.md) |
| improvements | `core:improvement/pasture` | differs | food_bonus: core.ron 1 vs BNW 0; prod_bonus: core.ron 1 vs BNW 0; work_turns core.ron 3 vs BNW Builds.Time 800 (hundredths of worker-turns, rules.md) |
| improvements | `core:improvement/quarry` | differs | prod_bonus: core.ron 3 vs BNW 0; work_turns core.ron 4 vs BNW Builds.Time 800 (hundredths of worker-turns, rules.md) |
| resources | `core:resource/wheat` | differs | food_bonus: core.ron 2 vs BNW 1 |
| resources | `core:resource/iron` | differs | prod_bonus: core.ron 2 vs BNW 1 |
| resources | `core:resource/gold_ore` | not in BNW catalog under this id |  |
| civs | `core:nation/rome` | differs | leader: civs.ron Trajan vs BNW Augustus Caesar |

Fully matching: 85 entries out of 121 (mostly technologies: cost and prerequisites match BNW, except for those listed).

Additionally:

- `core:resource/gold_ore` — in BNW the resource is called Gold (`core:resource/gold`, luxury, +2 gold, mine); rename it.
- `core.ron`: for improvements `work_turns` is in turns, while the ruleset keeps work time in hundredths of worker turns (700 = 7 turns at worker speed 100); the bonuses of the farm/mine/pasture/quarry in BNW are different (the pasture and the quarry give a yield only on a resource).
- `core.ron`: building effects (`add_yield`, `multiply_yield`) are placeholders; the correct effects are in `buildings.json` / `wonders.json`.
- `civs.ron`: start preferences are simplified to `coastal`/`none`; BNW has region priorities and prohibitions (`start_bias` in `civilizations.json`). The leader of Rome in BNW is Augustus Caesar, not Trajan (Trajan is a leader from Civ VI); the art catalog already uses Augustus.
- `core.ron` contains 3 wonders as buildings with `wonder: true`; BNW has 47 world and 11 national wonders, and the national ones have separate rules (a requirement for a building in all cities, a cost increase depending on the number of cities).

## Art without an entity and entities without art

- Art (buildings/wonders/projects) without an entity in the catalogs: `bnw:project/spaceship`.
- Art (units) without an entity in the catalogs: none.
- Art (technologies) without an entity in the catalogs: none.
- Art (policies) without an entity in the catalogs: none.
- Art (religion) without an entity in the catalogs: `ui/pantheon`, `unit/great-prophet`, `unit/inquisitor`, `unit/missionary`.
- Art (nations) without an entity in the catalogs: none.
  - `religion/unit/*` — duplicates of units from `units/inventory-bnw.json`; `religion/ui/pantheon` — an interface icon; `bnw:project/spaceship` — shared spaceship art (in BNW these are 4 types of part-units and 4 internal projects).
- Without art: 57 unique units, 4 spaceship parts, all resources, improvements, terrain, features, natural wonders, promotions, city-states. Separate generation packages are needed.

## What is not yet described by data

Entities whose in-game behavior is hardcoded in the code rather than in tables are marked in the catalogs with the effect `"modifier": "text_only"` with `needs_mechanic: "hardcoded_<type>"` and `verify: true`. Mechanic flags (`needs_mechanic` on boolean and special columns) are listed in `effect_vocabulary.json`.
