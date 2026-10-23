//! classic/docs/classic/implemented.md: per catalog category — entities in the
//! catalog, compiled, typed (active) effects, inert effects by
//! needs_mechanic. Generated from the compiled ruleset, so it is what the
//! game loads, not what the importer meant to write.

use std::collections::BTreeMap;

use classic_rules::rules::compiled::CompiledRuleset;
use classic_rules::rules::content::{BuildingKind, InertEffect};
use classic_rules::rules::effects::EffectSet;

use crate::import::catalog::{id, items, Catalogs};

/// One catalog category: catalog ids and the compiled rows it maps to.
struct Category<'a> {
    name: &'static str,
    catalog: &'static str,
    ids: Vec<String>,
    rows: Vec<Row<'a>>,
}

struct Row<'a> {
    key: &'a str,
    typed: usize,
    inert: &'a [InertEffect],
}

fn typed_count(set: &EffectSet) -> usize {
    set.passive_player.len()
        + set.passive_city.len()
        + set.passive_unit.len()
        + set.passive_tile.len()
        + set.triggered.iter().map(|t| t.len()).sum::<usize>()
}

fn ids_of(
    catalog: &serde_json::Value,
    list: &str,
    keep: impl Fn(&serde_json::Value) -> bool,
) -> Vec<String> {
    items(catalog, list)
        .into_iter()
        .filter(|x| keep(x))
        .map(|x| id(x).to_string())
        .collect()
}

fn all(_: &serde_json::Value) -> bool {
    true
}

fn kind_is(kinds: &'static [&'static str]) -> impl Fn(&serde_json::Value) -> bool {
    move |x| kinds.contains(&x["kind"].as_str().unwrap_or(""))
}

fn categories<'a>(cat: &Catalogs, r: &'a CompiledRuleset) -> Vec<Category<'a>> {
    let row = |key: &'a str, typed: usize, inert: &'a [InertEffect]| Row { key, typed, inert };
    let units = |unique: bool| -> Vec<Row<'a>> {
        r.units
            .iter()
            .filter(|u| u.civilization.is_some() == unique)
            .map(|u| row(&u.key, typed_count(&u.effects), &u.inert))
            .collect()
    };
    let buildings = |kinds: &[BuildingKind]| -> Vec<Row<'a>> {
        r.buildings
            .iter()
            .filter(|b| kinds.contains(&b.kind))
            .map(|b| row(&b.key, typed_count(&b.effects), &b.inert))
            .collect()
    };
    use BuildingKind as K;
    vec![
        Category {
            name: "Technologies",
            catalog: "techs.json",
            ids: ids_of(&cat.techs, "items", all),
            rows: r
                .technologies
                .iter()
                .map(|t| row(&t.key, typed_count(&t.effects), &t.inert))
                .collect(),
        },
        Category {
            name: "Units (regular, great people, spaceship parts)",
            catalog: "units.json",
            ids: ids_of(&cat.units, "items", all),
            rows: units(false),
        },
        Category {
            name: "Unique units",
            catalog: "unique_units.json",
            ids: ids_of(&cat.unique_units, "items", all),
            rows: units(true),
        },
        Category {
            name: "Buildings (regular, religious, guilds, palace)",
            catalog: "buildings.json",
            ids: ids_of(&cat.buildings, "items", all),
            rows: buildings(&[K::Regular, classic_rules::rules::content::RELIGIOUS_BUILDING, classic_rules::rules::content::GUILD, K::Palace]),
        },
        Category {
            name: "Unique buildings",
            catalog: "unique_buildings.json",
            ids: ids_of(&cat.unique_buildings, "items", all),
            rows: buildings(&[K::Unique]),
        },
        Category {
            name: "World wonders",
            catalog: "wonders.json",
            ids: ids_of(&cat.wonders, "items", kind_is(&["world_wonder"])),
            rows: buildings(&[K::WorldWonder]),
        },
        Category {
            name: "National wonders",
            catalog: "wonders.json",
            ids: ids_of(&cat.wonders, "items", kind_is(&["national_wonder"])),
            rows: buildings(&[K::NationalWonder]),
        },
        Category {
            name: "Projects (incl. World Congress)",
            catalog: "wonders.json",
            ids: ids_of(
                &cat.wonders,
                "items",
                kind_is(&["project", "league_project"]),
            ),
            rows: r
                .module.projects
                .iter()
                .map(|p| row(&p.key, 0, &p.inert))
                .collect(),
        },
        Category {
            name: "Policy branches and ideologies",
            catalog: "policies.json",
            ids: ids_of(&cat.policies, "branches", all),
            rows: r
                .module.policy_branches
                .iter()
                .map(|b| row(&b.key, 0, &[]))
                .collect(),
        },
        Category {
            name: "Policies, tenets, openers, finishers",
            catalog: "policies.json",
            ids: ids_of(&cat.policies, "items", all),
            rows: r
                .module.policies
                .iter()
                .map(|p| row(&p.key, typed_count(&p.effects), &p.inert))
                .collect(),
        },
        Category {
            name: "Beliefs",
            catalog: "beliefs.json",
            ids: ids_of(&cat.beliefs, "items", all),
            rows: r
                .module.beliefs
                .iter()
                .map(|b| row(&b.key, typed_count(&b.effects), &b.inert))
                .collect(),
        },
        Category {
            name: "Religions",
            catalog: "beliefs.json",
            ids: ids_of(&cat.beliefs, "religions", all),
            rows: r.module.religions.iter().map(|x| row(&x.key, 0, &[])).collect(),
        },
        Category {
            name: "Resources",
            catalog: "resources.json",
            ids: ids_of(&cat.resources, "items", all),
            rows: r
                .resources
                .iter()
                .map(|x| row(&x.key, 0, &x.inert))
                .collect(),
        },
        Category {
            name: "Improvements",
            catalog: "improvements.json",
            ids: ids_of(&cat.improvements, "items", all),
            rows: r
                .improvements
                .iter()
                .map(|x| row(&x.key, 0, &x.inert))
                .collect(),
        },
        Category {
            name: "Terrain types",
            catalog: "terrain.json",
            ids: ids_of(&cat.terrain, "items", all),
            rows: r.terrains.iter().map(|x| row(&x.key, 0, &[])).collect(),
        },
        Category {
            name: "Features",
            catalog: "features.json",
            ids: ids_of(&cat.features, "items", all),
            rows: r
                .features
                .iter()
                .map(|x| row(&x.key, 0, &x.inert))
                .collect(),
        },
        Category {
            name: "Natural wonders",
            catalog: "natural_wonders.json",
            ids: ids_of(&cat.natural_wonders, "items", all),
            rows: r
                .natural_wonders
                .iter()
                .map(|x| row(&x.key, 0, &x.inert))
                .collect(),
        },
        Category {
            name: "Civilizations",
            catalog: "civilizations.json",
            ids: ids_of(&cat.civilizations, "items", all),
            rows: r
                .civs
                .iter()
                .map(|c| row(&c.key, typed_count(&c.effects), &c.inert))
                .collect(),
        },
        Category {
            name: "City-state types",
            catalog: "city_states.json",
            ids: ids_of(&cat.city_states, "types", all),
            rows: r
                .module.city_state_types
                .iter()
                .map(|x| row(&x.key, 0, &x.inert))
                .collect(),
        },
        Category {
            name: "City-states",
            catalog: "city_states.json",
            ids: ids_of(&cat.city_states, "items", all),
            rows: r.module.city_states.iter().map(|x| row(&x.key, 0, &[])).collect(),
        },
        Category {
            name: "Promotions",
            catalog: "promotions.json",
            ids: ids_of(&cat.promotions, "items", all),
            rows: r
                .promotions
                .iter()
                .map(|p| row(&p.key, typed_count(&p.effects), &p.inert))
                .collect(),
        },
        Category {
            name: "Specialists",
            catalog: "great_people.json",
            ids: ids_of(&cat.great_people, "specialists", all),
            rows: r.specialists.iter().map(|x| row(&x.key, 0, &[])).collect(),
        },
        Category {
            name: "Great people",
            catalog: "great_people.json",
            ids: items(&cat.great_people, "items")
                .into_iter()
                .map(|g| {
                    format!(
                        "core:great-person/{}",
                        id(g)
                            .rsplit('/')
                            .next()
                            .unwrap_or("")
                            .trim_start_matches("great-")
                    )
                })
                .collect(),
            rows: r
                .module.great_people
                .iter()
                .map(|x| row(&x.key, 0, &x.inert))
                .collect(),
        },
        Category {
            name: "Eras",
            catalog: "eras.json",
            ids: ids_of(&cat.eras, "items", all),
            rows: r.eras.iter().map(|x| row(&x.key, 0, &[])).collect(),
        },
    ]
}

/// Mechanic → number of inert effects (sorted by name).
fn by_mechanic<'a>(rows: impl Iterator<Item = &'a Row<'a>>) -> BTreeMap<&'a str, usize> {
    let mut out = BTreeMap::new();
    for row in rows {
        for e in row.inert {
            *out.entry(e.needs_mechanic.as_str()).or_insert(0) += 1;
        }
    }
    out
}

/// Family of a mechanic name for the summary (`modifier:combat.flank_bonus.pct`
/// → `modifier:combat`, `unit_ability.pillage` → `unit_ability`).
fn family(mechanic: &str) -> String {
    if let Some((kind, rest)) = mechanic.split_once(':') {
        let head = rest.split('.').next().unwrap_or(rest);
        return format!("{kind}:{head}");
    }
    match mechanic.split_once('.') {
        Some((head, _)) => head.to_string(),
        None => mechanic.to_string(),
    }
}

pub fn render(cat: &Catalogs, rules: &CompiledRuleset) -> String {
    let cats = categories(cat, rules);
    let mut out = String::new();
    out.push_str(INTRO);
    summary_table(&mut out, &cats);
    families_table(&mut out, &cats);
    out.push_str(NOTES);
    per_category(&mut out, &cats);
    out
}

fn summary_table(out: &mut String, cats: &[Category]) {
    out.push_str("## Summary\n\n");
    out.push_str("| Category | Catalog | In catalog | Compiled | Typed effects | Inert effects | Entities with inert effects |\n");
    out.push_str("|---|---|---:|---:|---:|---:|---:|\n");
    let mut totals = [0usize; 5];
    for c in cats {
        let compiled = c
            .ids
            .iter()
            .filter(|i| c.rows.iter().any(|r| r.key == i.as_str()))
            .count();
        let typed: usize = c.rows.iter().map(|r| r.typed).sum();
        let inert: usize = c.rows.iter().map(|r| r.inert.len()).sum();
        let with_inert = c.rows.iter().filter(|r| !r.inert.is_empty()).count();
        out.push_str(&format!(
            "| {} | `{}` | {} | {} | {} | {} | {} |\n",
            c.name,
            c.catalog,
            c.ids.len(),
            compiled,
            typed,
            inert,
            with_inert
        ));
        for (t, v) in totals
            .iter_mut()
            .zip([c.ids.len(), compiled, typed, inert, with_inert])
        {
            *t += v;
        }
    }
    out.push_str(&format!(
        "| **Total** | | **{}** | **{}** | **{}** | **{}** | **{}** |\n\n",
        totals[0], totals[1], totals[2], totals[3], totals[4]
    ));
}

fn families_table(out: &mut String, cats: &[Category]) {
    let mut families: BTreeMap<String, (usize, usize, Vec<&str>)> = BTreeMap::new();
    for c in cats {
        for (mechanic, count) in by_mechanic(c.rows.iter()) {
            let entry = families
                .entry(family(mechanic))
                .or_insert((0, 0, Vec::new()));
            entry.0 += count;
            entry.1 += 1;
            if !entry.2.contains(&c.name) {
                entry.2.push(c.name);
            }
        }
    }
    let mut sorted: Vec<_> = families.into_iter().collect();
    sorted.sort_by(|a, b| b.1 .0.cmp(&a.1 .0).then(a.0.cmp(&b.0)));
    // No inert effect left: no section.
    if sorted.is_empty() {
        return;
    }
    out.push_str("## Inert effects by mechanic family (M4 work list)\n\n");
    out.push_str("| Mechanic family | Inert effects | Distinct mechanics | Categories |\n|---|---:|---:|---|\n");
    for (name, (count, distinct, categories)) in sorted {
        out.push_str(&format!(
            "| `{name}` | {count} | {distinct} | {} |\n",
            categories.join(", ")
        ));
    }
    out.push('\n');
}

fn per_category(out: &mut String, cats: &[Category]) {
    if cats.iter().all(|c| by_mechanic(c.rows.iter()).is_empty()) {
        return;
    }
    out.push_str("## Inert effects by needs_mechanic, per category\n\n");
    for c in cats {
        let mechanics = by_mechanic(c.rows.iter());
        if mechanics.is_empty() {
            continue;
        }
        out.push_str(&format!(
            "### {}\n\n| needs_mechanic | Inert effects |\n|---|---:|\n",
            c.name
        ));
        let mut sorted: Vec<_> = mechanics.into_iter().collect();
        sorted.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
        for (mechanic, count) in sorted {
            out.push_str(&format!("| `{mechanic}` | {count} |\n"));
        }
        out.push('\n');
    }
}

const INTRO: &str = "# What of the classic rule set is implemented

Generated by the importer (`cargo run --release --offline -p classic-content --features importer --bin classic-import`) from the compiled classic ruleset (`assets/rules/classic/*.ron`). Do not edit by hand.

- **Compiled**: catalog ids present in the compiled ruleset (every reference validated).
- **Typed effects**: catalog effects mapped onto the M3.0 effect system (`rules::effects`), evaluated by the engine.
- **Inert effects**: catalog effects, unit abilities and placement rules that need a mechanic the engine does not have. They are compiled and their references validated, but nothing evaluates them; `needs_mechanic` names the missing piece. Names with a prefix come from the importer: `modifier:<name>` (no typed modifier), `trigger:<name>`, `passive_grant:<modifier>` (a grant while the source exists), `triggered_modifier:<modifier>` (a continuous modifier started by an event), `selector:<kind>`, `condition:<key>`, `scope:<modifier>@<scope>`, `amount:<name>`, `source_effects:<name>` (the owner table has no typed effects); names without a prefix are the catalog's own `needs_mechanic` or a data field (`unit_ability.*`, `building_requirement.*`, `improvement_placement`, ...).

";

const NOTES: &str = "## Typed, but not applied yet

- Policies: adopted policies are owner sources (their passive effects reach the player's cities, `OnAdopt` grants fire on adoption). Typed unit and building abilities (`UnitDefinition::abilities`, `BuildingDefinition::abilities`) are read by their mechanics and are not counted as inert.
- Beliefs: the beliefs of a city's majority (its religion, or its owner's pantheon) reach the city (`FollowingCities`, `FollowingCityTiles`), founder beliefs reach the religion's founder (`Founder`) and its holy city (`HolyCity`), religion-wide numbers (`ModifyReligion`) change pressure, spread distance, missionary and prophet strength and costs.
- Tourism (`ModifyTourism`, `TourismToward`): great work slots and theming bonuses (`BuildingDefinition::great_work_slots`, `theming`) are read by `derived::tourism`; the trade route percent (`TradeRoute`) holds while a route of `world::trade` runs between the two players, the happiness percent (`LessHappy`) holds while the sender's excess happiness is above the target's, and the diplomat's +25 % toward a rival with another ideology (`DIPLOMAT_TOURISM_PERCENT`) holds while a diplomat of the sender is at work in the rival's capital (`systems::congress::diplomat_visiting`).

## Applied with a known gap

- Bonus against helicopters: the catalog gives no number, +100 % is a chosen value.
- Inca (hills for one point): the hills step also ignores the river penalty of that step; this is kept.

## Compiled data without a consumer yet

- Buildings: the start eras (`max_start_era`, `free_start_era`: they act only in a game that starts after the first era, which the setup does not offer; about 1 % of Civ V's setup options, no effect on a game from the first era) and the `AllowsRangeStrike` ability (Walls, Castle, Arsenal, Military Base and their replacements: every city can strike in the classic rules, docs/classic/rules.md; the flag is kept as data).
- Eras: the fields of a game that starts in a later era (`start_percent` and the other start percents, `starting_gold`, `starting_culture`, `free_population`, `starting_unit_multiplier`, `later_era_building_construct_mod`, `no_goodies`, `no_barb_units`, `no_religion` and the era's technology list); a new game takes the first era's starting units only. The numbers an era sets while it is played are read as constants of the rules that equal the catalog (warmonger percent, spies of the Renaissance and Industrial eras, the trade route era bonus, league percent) or from the catalog (`embarked_unit_defense`, `research_agreement_cost`, `faith_cost_multiplier`).
- Terrain: the hills' extra defense and movement are constants of the rules (+25 %, +1 point) that equal the catalog header of `terrain.json`.

Read by their mechanics, so not in this list: a unit's `sight` and the buildings it needs in its city (`derived::visibility`, `classic::systems::unit_problem`; a replaced building is met by the unique building of the civilization), the national wonder cost per city (`CityModifiers::production_cost`), the improvements' `defense_pct` (`combat::terrain`), `pillage_gold` (the pillage command) and `yield_per_era` (landmark culture), the movement and defense of terrains and features (`navigation::cost`, `combat::terrain`), Ancient Ruins (the generator lays about one per 80 tiles of enterable land, `scenario::ruins`; a human player's unit standing on them claims one of three rewards the place offers on a card, `Command::ClaimRuins`; an AI unit is paid when it enters, and a unit that chooses, Native Tongue, takes the best), the Iroquois' Great Warpath (forests and jungles of their own land are roads for walking, `navigation::cost`, and join cities in a city connection, `derived::connections`; a trade route's path enters any forest or jungle at a road's price, `derived::trade_path`, docs/classic/rules.md; the city connection half is the trait's public description, the docs give no formula for it) and the player's era (the era of its latest technology; entering a new one fires `OnEraEntered`, shows a notice and the top bar).
Natural wonders are placed by the generator on their typed placement rules (`rules::natural_wonders`, `scenario/landscape.rs`); their tile yields, happiness, rough ground, no-improvement rule, first-finder gold, find triggers and adjacent promotions are applied (`systems::natural_wonders`). The production rules (`world::building_rules`) enforce the unique-to-civilization, unbuildable, one-national-wonder, required-building and placement rules; maintenance, specialist and great work slots, consumed resources and the hurry modifier are read by their mechanics.

## Generator approximations (own values, not from Unciv)

- Resource placement density by class (bonus 3 %, strategic 2 %, luxury 1 % of the eligible land tiles, 0 for resources outside the standard game, city-state only or civilization unique): the original game places resources with a script of its own, which the catalogs do not describe. The modern-era strategics are balanced to one another instead (oil 4 % of desert, tundra and snow, uranium 1 %; `strategic_land_density`).
- Resources at sea (`sea_density`, percent of the shallow sea, water within two tiles of land; the first ring of the shore takes the whole percent, the second half): a bonus resource 4 % (fish), a luxury 2 % (whales, pearls, crab), a strategic one 1 % (oil), for the resources whose terrains include the coast. A coastal start without a deposit at sea in its work radius gets one fish (`kernel::scenario::sea_resources`).
- Map codes of terrains (`map_code`): plains 0, grassland 1, hill 2, desert 3, coast 4, snow 5, mountain 6, tundra 7. Ocean has no code: the map has one water code, deep water is the water away from land and is drawn by the renderer; resources found only on ocean are not placed.
- Hills and mountains are plot types here, not terrains of the catalog: the ruleset gives them the plot rules instead (land; a mountain is impassable).
- Civilization colors: the ten civilizations of the first content keep theirs; the others get a deterministic hue from their key (the catalogs carry no colours).
- Feature removal (forest, jungle, marsh): the times and technologies in features.json were added by hand and agree with Unciv (Civ V - Gods & Kings/TileImprovements.json, checked 2026-10-20); the chop production is not in Unciv's data and carries `verify`.
- Hills: the map has one hills code without a base terrain; it yields as hills over plains (0 food, 2 production, as every base terrain but snow).
- Natural wonders: the Great Barrier Reef takes one tile (Civ V two) and the Rock of Gibraltar keeps its neighbours (Civ V's custom tile changes are not in the catalogs); shallow water next to a wonder is any water (one water code); a natural wonder find is settled at the end of the finder's turn (Civ V: when the tile is revealed).

";
