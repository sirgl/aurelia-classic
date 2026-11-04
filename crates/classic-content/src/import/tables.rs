//! Catalog records → RON records, one function per table. Field names are
//! the RON schema of `crate::ron`; values come from the catalogs only
//! (generator approximations are named in `GENERATOR_NOTES`).

use serde_json::Value;

use crate::import::catalog::{
    flag, id, int, items, key_counts, opt, slug, strings, text, yields, Catalogs,
};
use crate::import::effects::{collect_refs, inert, map_effect, Mapped, Owner};
use crate::import::ron_out::{quote, Record};

/// Typed effects and inert effects of a record.
#[derive(Default)]
pub struct Effects {
    pub typed: Vec<String>,
    pub inert: Vec<String>,
}

impl Effects {
    pub fn of(list: &Value, owner: Owner) -> Self {
        let mut out = Effects::default();
        for e in list.as_array().map(|a| a.as_slice()).unwrap_or(&[]) {
            out.push(e, owner);
        }
        out
    }

    pub fn push(&mut self, e: &Value, owner: Owner) {
        match map_effect(e, owner) {
            Mapped::Typed(t) => self.typed.push(t),
            Mapped::Inert(r) => self.inert.push(r.inline()),
            Mapped::Covered => {}
        }
    }

    pub fn inert(&mut self, record: Record) {
        self.inert.push(record.inline());
    }

    fn write(self, record: Record, typed_allowed: bool) -> Record {
        assert!(
            typed_allowed || self.typed.is_empty(),
            "typed effects on an inert-only table"
        );
        record
            .lines("effects", self.typed)
            .lines("inert", self.inert)
    }
}

/// An inert entry for a data field the engine has no mechanic for.
fn field_inert(mechanic: &str, modifier: &str, scope: &str, value: &Value) -> Record {
    let mut refs = Vec::new();
    collect_refs(value, &mut refs);
    refs.sort();
    refs.dedup();
    let amount = match value {
        Value::Number(_) | Value::Bool(_) => value.clone(),
        _ => Value::Null,
    };
    inert(mechanic, modifier, scope, "this", "passive", &amount, &refs)
}

fn opt_key(record: Record, name: &'static str, v: &Value, field: &str) -> Record {
    record.opt_str(name, opt(v, field))
}

/// Era display name for the tech tree (`core:era/ancient` → "Ancient").
fn era_label(cat: &Catalogs, era: &str) -> String {
    for e in items(&cat.eras, "items") {
        if id(e) == era {
            return text(e, "name").trim_end_matches(" Era").to_string();
        }
    }
    panic!("unknown era {era}")
}

// ---- eras, terrain, features, natural wonders ----

pub fn eras(cat: &Catalogs) -> Vec<Record> {
    items(&cat.eras, "items")
        .into_iter()
        .map(|e| {
            Record::new()
                .str("id", id(e))
                .str("name", text(e, "name"))
                .raw("index", int(e, "index").to_string())
                .raw(
                    "starting_defense_units",
                    int(e, "starting_defense_units").to_string(),
                )
                .raw(
                    "starting_worker_units",
                    int(e, "starting_worker_units").to_string(),
                )
                .raw(
                    "starting_explore_units",
                    int(e, "starting_explore_units").to_string(),
                )
                .raw("starting_gold", int(e, "starting_gold").to_string())
                .raw(
                    "research_agreement_cost",
                    int(e, "research_agreement_cost").to_string(),
                )
                .raw(
                    "embarked_unit_defense",
                    int(e, "embarked_unit_defense").to_string(),
                )
                .raw(
                    "faith_cost_multiplier",
                    int(e, "faith_cost_multiplier").to_string(),
                )
        })
        .collect()
}

/// Map codes of the terrains (`rules::roles::TerrainPalette`; the grounds
/// they play are in `roles.ron`): 0 plains, 1 grassland, 2 hills, 3 desert,
/// 4 water, 5 snow, 6 mountain, 7 tundra. Ocean has no code: deep water is the water away from land
/// (coast is the water next to land), drawn by the renderer.
pub fn map_code(terrain: &str) -> Option<u16> {
    Some(match terrain {
        "core:terrain/plains" => 0,
        "core:terrain/grassland" => 1,
        "core:terrain/hill" => 2,
        "core:terrain/desert" => 3,
        "core:terrain/coast" => 4,
        "core:terrain/snow" => 5,
        "core:terrain/mountain" => 6,
        "core:terrain/tundra" => 7,
        _ => return None,
    })
}

/// The map has one hills code without a base terrain under it: its yields
/// are those of hills over plains (base terrain + hills change; the catalog
/// gives 0 food, 2 production on hills over every base except snow).
fn hills_over_plains(cat: &Catalogs) -> Value {
    let plains = items(&cat.terrain, "items")
        .into_iter()
        .find(|t| id(t) == "core:terrain/plains")
        .expect("plains in terrain.json");
    let mut out = serde_json::Map::new();
    for y in ["food", "production", "gold", "science", "culture", "faith"] {
        let total = int(&plains["yields"], y) + int(&plains["hills_yield_change"], y);
        if total != 0 {
            out.insert(y.to_string(), Value::from(total));
        }
    }
    Value::Object(out)
}

/// Hills and mountains are plot types here. Their rows in the catalog
/// are graphics-only art entries that carry the water flag and no
/// impassable flag; the plot rules are what the game applies: both are land, a
/// mountain is impassable (Unciv, Gods & Kings/Terrains.json:
/// Mountain `impassable`, Hill movement cost 2 and defense bonus 0.25;
/// checked 2026-10-20). Hills' +1 movement and +25 % defense are plot rules in code
/// (`navigation::cost::HILLS_EXTRA_MOVEMENT`, terrain.json `rivers`).
fn plot_flags(terrain: &str, water: bool, impassable: bool) -> (bool, bool) {
    match terrain {
        "core:terrain/hill" => (false, false),
        "core:terrain/mountain" => (false, true),
        _ => (water, impassable),
    }
}

pub fn terrains(cat: &Catalogs) -> Vec<Record> {
    items(&cat.terrain, "items")
        .into_iter()
        .map(|t| {
            let mut r = Record::new().str("id", id(t)).str("name", text(t, "name"));
            let own = if id(t) == "core:terrain/hill" {
                hills_over_plains(cat)
            } else {
                t["yields"].clone()
            };
            if let Some(y) = yields(&own) {
                r = r.raw("yields", y);
            }
            if let Some(y) = yields(&t["hills_yield_change"]) {
                r = r.raw("hills_yields", y);
            }
            let (water, impassable) = plot_flags(id(t), flag(t, "water"), flag(t, "impassable"));
            r = r
                .raw("movement_cost", int(t, "movement_cost").to_string())
                .flag("impassable", impassable)
                .num("defense_pct", int(t, "defense_pct"))
                .flag("water", water)
                .flag("can_found_city", flag(t, "can_found_city"));
            match map_code(id(t)) {
                Some(code) => r.raw("map_code", format!("Some({code})")),
                None => r,
            }
        })
        .collect()
}

/// Fields of features and natural wonders without a mechanic yet.
const FEATURE_INERT_FIELDS: [&str; 3] = [
    "happiness_in_borders",
    "first_finder_gold",
    "adjacent_unit_free_promotion",
];

pub fn features(cat: &Catalogs) -> Vec<Record> {
    items(&cat.features, "items")
        .into_iter()
        .map(|f| {
            let mut fx = Effects::default();
            for field in FEATURE_INERT_FIELDS {
                if !f[field].is_null() && f[field] != Value::Bool(false) {
                    fx.inert(field_inert(
                        &format!("feature.{field}"),
                        &format!("feature.{field}"),
                        "tile",
                        &f[field],
                    ));
                }
            }
            let mut r = Record::new().str("id", id(f)).str("name", text(f, "name"));
            if let Some(y) = yields(&f["yields"]) {
                r = r.raw("yields", y);
            }
            let r = r
                .flag("yield_not_additive", flag(f, "yield_not_additive"))
                .num("movement_cost", int(f, "movement_cost"))
                .num("defense_pct", int(f, "defense_pct"))
                .flag("impassable", flag(f, "impassable"))
                .flag("rough", flag(f, "rough"))
                .flag("fresh_water", flag(f, "adds_fresh_water"))
                .flag("no_improvement", flag(f, "no_improvement"))
                .flag("requires_river", flag(f, "requires_river"))
                .flag("requires_flatlands", flag(f, "requires_flatlands"))
                .num("see_through", int(f, "see_through"))
                .strs("valid_terrains", &strings(f, "valid_terrains"));
            let r = match feature_removal(f) {
                Some(removal) => r.raw("removal", removal),
                None => r.flag("unremovable", true),
            };
            fx.write(r, false)
        })
        .collect()
}

/// `Some((tech: ..., work_time: N, production: P))` of a removable feature.
fn feature_removal(f: &Value) -> Option<String> {
    let removal = f["removal"].as_object()?;
    let removal = Value::Object(removal.clone());
    let tech = match opt(&removal, "tech") {
        Some(t) => format!("Some({})", quote(t)),
        None => "None".into(),
    };
    Some(format!(
        "Some((tech: {tech}, work_time: {}, production: {}))",
        int(&removal, "work_time"),
        int(&removal, "production")
    ))
}

pub fn natural_wonders(cat: &Catalogs) -> Vec<Record> {
    items(&cat.natural_wonders, "items")
        .into_iter()
        .map(|w| {
            let fx = Effects::default();
            for field in ["requires_river", "requires_flatlands"] {
                assert!(!flag(w, field), "{}: natural wonder {field} has no rule", id(w));
            }
            let mut r = Record::new().str("id", id(w)).str("name", text(w, "name"));
            if let Some(y) = yields(&w["yields"]) {
                r = r.raw("yields", y);
            }
            let r = r
                .flag("impassable", flag(w, "impassable"))
                .num("movement_cost", int(w, "movement_cost"))
                .num("defense_pct", int(w, "defense_pct"))
                .num("happiness", int(w, "happiness_in_borders"))
                .num("first_finder_gold", int(w, "first_finder_gold"))
                .flag("fresh_water", flag(w, "adds_fresh_water"))
                .strs("valid_terrains", &strings(w, "valid_terrains"));
            let r = opt_key(
                r,
                "adjacent_unit_free_promotion",
                w,
                "adjacent_unit_free_promotion",
            )
            .num("occurrence_frequency", int(w, "occurrence_frequency"))
            .flag("rough", flag(w, "rough"))
            .flag("no_improvement", flag(w, "no_improvement"));
            let r = match w["placement"].as_object() {
                Some(placement) => r.raw("placement", natural_wonder_placement(w, placement)),
                None => r,
            };
            fx.write(r, false)
        })
        .collect()
}

/// The `placement` record of a natural wonder as RON
/// (`classic_rules::rules::natural_wonders::Placement`): the catalog's
/// placement columns folded into typed rules. The
/// `adjacent_tiles_care_about_*` switches only enable the require/avoid
/// rules of their group; an unknown column stops the import.
fn natural_wonder_placement(w: &Value, p: &serde_json::Map<String, Value>) -> String {
    let wonder = id(w);
    let on = |k: &str| p.get(k).and_then(Value::as_bool).unwrap_or(false);
    let number = |k: &str, default: i64| p.get(k).and_then(Value::as_i64).unwrap_or(default);
    let mut parts = Vec::new();
    match (number("eligibility_method_number", 0), p.get("land_based").and_then(Value::as_bool)) {
        (1, _) => parts.push("method: Reef".to_string()),
        (2, _) => parts.push("method: PeninsulaTip".to_string()),
        (0, Some(false)) => parts.push("method: Water".to_string()),
        (0, _) => {}
        (other, _) => panic!("{wonder}: eligibility method {other}"),
    }
    assert_eq!(
        number("tile_changes_method_number", 0),
        number("eligibility_method_number", 0),
        "{wonder}: custom tile changes follow the custom eligibility"
    );
    if let Some(freq) = p.get("occurrence_frequency") {
        assert_eq!(freq.as_i64(), w["occurrence_frequency"].as_i64(), "{wonder}: two frequencies");
    }
    if on("avoid_land_adjacent_to_ocean") {
        parts.push("avoid_ocean_coast: true".into());
    }
    if on("avoid_biggest_landmass") {
        parts.push("avoid_biggest_landmass: true".into());
    }
    if p.get("core_tile_can_be_any_terrain_type").and_then(Value::as_bool) == Some(false) {
        let allowed: Vec<String> = ["desert", "grass", "plains", "tundra"]
            .iter()
            .filter(|t| on(&format!("core_tile_can_be_{t}")))
            .map(|t| quote(&placement_terrain(t)))
            .collect();
        parts.push(format!("core_terrains: [{}]", allowed.join(", ")));
    }
    let adjacent = adjacent_rules(wonder, p);
    if !adjacent.is_empty() {
        parts.push(format!("adjacent: [{}]", adjacent.join(", ")));
    }
    if on("change_core_tile_to_mountain") {
        parts.push("core_shape: Mountain".into());
    } else if on("change_core_tile_to_flatland") {
        parts.push("core_shape: Flat".into());
    }
    for t in ["grass", "plains"] {
        if on(&format!("change_core_tile_terrain_to_{t}")) {
            parts.push(format!("core_terrain: Some({})", quote(&placement_terrain(t))));
        }
    }
    if on("set_adjacent_tiles_to_shallow_water") {
        parts.push("surround_with_water: true".into());
    }
    check_placement_columns(wonder, p);
    format!("({})", parts.join(", "))
}

/// `adjacent_tiles_require_<kind>` (at least `required_number_of_adjacent_<kind>`,
/// default 1) and `adjacent_tiles_avoid_<kind>` (at most
/// `maximum_allowed_adjacent_<kind>`, default 0).
fn adjacent_rules(wonder: &str, p: &serde_json::Map<String, Value>) -> Vec<String> {
    let mut out = Vec::new();
    for (column, value) in p {
        if value.as_bool() != Some(true) {
            continue;
        }
        let (kind, bound) = if let Some(kind) = column.strip_prefix("adjacent_tiles_require_") {
            let n = p.get(&format!("required_number_of_adjacent_{kind}")).and_then(Value::as_i64);
            (kind, ("AtLeast", n.unwrap_or(1)))
        } else if let Some(kind) = column.strip_prefix("adjacent_tiles_avoid_") {
            let n = p.get(&format!("maximum_allowed_adjacent_{kind}")).and_then(Value::as_i64);
            (kind, ("AtMost", n.unwrap_or(0)))
        } else {
            continue;
        };
        out.push(format!("{}({}, {})", bound.0, adjacent_kind(wonder, kind), bound.1));
    }
    out
}

fn adjacent_kind(wonder: &str, kind: &str) -> String {
    match kind {
        "hills" => "Hills".into(),
        "mountain" => "Mountain".into(),
        "hills_plus_mountains" => "HillsOrMountain".into(),
        "anyland" => "Land".into(),
        "shallow_water" => "Water".into(),
        "desert" | "grass" | "plains" | "tundra" => format!("Terrain({})", quote(&placement_terrain(kind))),
        "marsh" | "jungle" | "ice" => format!("Feature({})", quote(&format!("core:feature/{kind}"))),
        other => panic!("{wonder}: adjacent kind {other}"),
    }
}

fn placement_terrain(name: &str) -> String {
    match name {
        "grass" => "core:terrain/grassland".into(),
        other => format!("core:terrain/{other}"),
    }
}

/// Every placement column is one the typed record carries.
fn check_placement_columns(wonder: &str, p: &serde_json::Map<String, Value>) {
    const SCALAR: [&str; 16] = [
        "eligibility_method_number",
        "tile_changes_method_number",
        "occurrence_frequency",
        "land_based",
        "avoid_land_adjacent_to_ocean",
        "avoid_biggest_landmass",
        "core_tile_can_be_any_terrain_type",
        "change_core_tile_to_mountain",
        "change_core_tile_to_flatland",
        "change_core_tile_terrain_to_grass",
        "change_core_tile_terrain_to_plains",
        "set_adjacent_tiles_to_shallow_water",
        "adjacent_tiles_care_about_plot_types",
        "adjacent_tiles_care_about_terrain_types",
        "adjacent_tiles_care_about_feature_types",
        "core_tile_can_be_desert",
    ];
    const PREFIXES: [&str; 5] = [
        "core_tile_can_be_",
        "adjacent_tiles_require_",
        "adjacent_tiles_avoid_",
        "required_number_of_adjacent_",
        "maximum_allowed_adjacent_",
    ];
    for column in p.keys() {
        let known = SCALAR.contains(&column.as_str()) || PREFIXES.iter().any(|x| column.starts_with(x));
        assert!(known, "{wonder}: placement column {column} has no rule");
    }
}

// ---- technologies ----

/// Unconditional improvement yield changes of a technology become Tile effects of the
/// technology (`OwnerCityTiles` + `TileImprovement`); the fresh water ones
/// stay on the improvement.
fn improvement_tech_effects(cat: &Catalogs, tech: &str) -> Vec<String> {
    let mut out = Vec::new();
    for imp in items(&cat.improvements, "items") {
        for change in imp["tech_yield_changes"]
            .as_array()
            .map(|a| a.as_slice())
            .unwrap_or(&[])
        {
            if change["tech"].as_str() != Some(tech) || !change["condition"].is_null() {
                continue;
            }
            let y = yield_name(text(change, "yield"));
            let amount = int(change, "amount");
            out.push(format!(
                "(scope: Tile, selector: OwnerCityTiles, condition: TileImprovement({}), modifier: AddYield(yield_kind: {y}, amount: {amount}))",
                quote(id(imp))
            ));
        }
    }
    out
}

fn yield_name(y: &str) -> &'static str {
    match y {
        "food" => "Food",
        "production" => "Production",
        "gold" => "Gold",
        "science" => "Science",
        "culture" => "Culture",
        "faith" => "Faith",
        other => panic!("unknown yield {other}"),
    }
}

/// Movement flags of a technology (`tech.allows_embarking`,
/// `tech.embarked_all_water_passage`) read by navigation, and the other effects.
fn tech_movement_flags(effects: &Value) -> (bool, bool, Value) {
    let mut embark = false;
    let mut all_water = false;
    let mut rest = Vec::new();
    for e in effects.as_array().into_iter().flatten() {
        match e["modifier"].as_str() {
            Some("tech.allows_embarking") => embark = true,
            Some("tech.embarked_all_water_passage") => all_water = true,
            _ => rest.push(e.clone()),
        }
    }
    (embark, all_water, Value::Array(rest))
}

/// Flags of a technology its systems read as fields: archaeology
/// (`tech.triggers_archaeological_sites`), knowledge (`tech.map_visible`), movement
/// (`tech.bridge_building`, the road's movement cost change).
struct TechFields {
    triggers_archaeological_sites: bool,
    map_visible: bool,
    bridge_building: bool,
    road_move_change: i64,
}

/// The technology's fields and its other effects.
fn tech_fields(effects: &Value) -> (TechFields, Value) {
    let mut fields = TechFields {
        triggers_archaeological_sites: false,
        map_visible: false,
        bridge_building: false,
        road_move_change: 0,
    };
    let mut rest = Vec::new();
    for e in effects.as_array().into_iter().flatten() {
        let road = e["selector"]["filter"]["route"].as_str() == Some("core:route/road");
        match e["modifier"].as_str() {
            Some("tech.triggers_archaeological_sites") => fields.triggers_archaeological_sites = true,
            Some("tech.map_visible") => fields.map_visible = true,
            Some("tech.bridge_building") => fields.bridge_building = true,
            Some("route.movement_cost.flat") if road => {
                fields.road_move_change = e["amount"].as_i64().expect("road movement change")
            }
            _ => rest.push(e.clone()),
        }
    }
    (fields, Value::Array(rest))
}

/// Diplomacy flags of a technology (embassy, open borders, defensive pact,
/// research agreement, World Congress) and the other effects.
fn tech_diplomacy_flags(effects: &Value) -> ([bool; 5], Value) {
    const NAMES: [&str; 5] = ["tech.allow_embassy_trading_allowed", "tech.open_borders_trading_allowed", "tech.defensive_pact_trading_allowed", "tech.research_agreement_trading_allowed", "tech.allows_world_congress"];
    let mut flags = [false; 5];
    let mut rest = Vec::new();
    for e in effects.as_array().into_iter().flatten() {
        let name = e["modifier"].as_str().unwrap_or("");
        match NAMES.iter().position(|n| *n == name) {
            Some(i) => flags[i] = true,
            None => rest.push(e.clone()),
        }
    }
    (flags, Value::Array(rest))
}

pub fn technologies(cat: &Catalogs) -> Vec<Record> {
    items(&cat.techs, "items")
        .into_iter()
        .map(|t| {
            let unlocks = &t["unlocks"];
            let mut actions = strings(unlocks, "routes");
            actions.extend(strings(unlocks, "worker_actions"));
            actions.sort();
            let (embark, all_water, rest) = tech_movement_flags(&t["effects"]);
            let (diplomacy, rest) = tech_diplomacy_flags(&rest);
            let (fields, rest) = tech_fields(&rest);
            let mut fx = Effects::of(&rest, Owner::Technology);
            for process in strings(unlocks, "processes") {
                match process.as_str() {
                    "wealth" => actions.push("core:process/wealth".to_string()),
                    "research" => actions.push("core:process/research".to_string()),
                    _ => {
                        let name = format!("tech.unlocks_process={process}");
                        fx.inert(field_inert("production_processes", &name, "player", &Value::Bool(true)));
                    }
                }
            }
            actions.sort();
            fx.typed.extend(improvement_tech_effects(cat, id(t)));
            let grid = &t["grid"];
            let era = text(t, "era");
            let r = Record::new()
                .str("id", id(t))
                .str("name", text(t, "name"))
                .raw("era_name", format!("Some({})", quote(&era_label(cat, era))))
                .opt_str("era", Some(era))
                .raw("cost", int(t, "cost").to_string())
                .raw(
                    "grid",
                    format!(
                        "({}, {})",
                        grid[0].as_i64().unwrap_or(0),
                        grid[1].as_i64().unwrap_or(0)
                    ),
                )
                .strs("requires", &strings(t, "prerequisites"))
                .strs("unlocks_buildings", &strings(unlocks, "buildings"))
                .strs("unlocks_units", &strings(unlocks, "units"))
                .strs("unlocks_improvements", &strings(unlocks, "improvements"))
                .strs("unlocks_projects", &strings(unlocks, "projects"))
                .strs("unlocks_actions", &actions)
                .flag("repeatable", flag(t, "repeatable"))
                .flag("allows_embarking", embark)
                .flag("embarked_all_water", all_water)
                .flag("allows_embassy", diplomacy[0])
                .flag("allows_open_borders", diplomacy[1])
                .flag("allows_defensive_pact", diplomacy[2])
                .flag("allows_research_agreement", diplomacy[3])
                .flag("allows_world_congress", diplomacy[4])
                .flag("triggers_archaeological_sites", fields.triggers_archaeological_sites)
                .flag("map_visible", fields.map_visible)
                .flag("bridge_building", fields.bridge_building)
                .num("road_move_change", fields.road_move_change);
            fx.write(r, true)
        })
        .collect()
}

// ---- units ----

fn unit_role(role: &str) -> &'static str {
    match role {
        "civilian" => "Civilian",
        "great-person" => "GreatPerson",
        "religious" => "Religious",
        "spaceship-part" => "SpaceshipPart",
        "recon" => "Recon",
        "melee" => "Melee",
        "archer" => "Archer",
        "mounted" => "Mounted",
        "siege" => "Siege",
        "gun" => "Gun",
        "armor" => "Armor",
        "navalmelee" => "NavalMelee",
        "navalranged" => "NavalRanged",
        "submarine" => "Submarine",
        "carrier" => "Carrier",
        "fighter" => "Fighter",
        "bomber" => "Bomber",
        "helicopter" => "Helicopter",
        "missile" => "Missile",
        other => panic!("unknown unit role {other}"),
    }
}

fn domain(key: &str) -> &'static str {
    match key {
        "core:domain/land" => "Land",
        "core:domain/sea" => "Sea",
        "core:domain/air" => "Air",
        other => panic!("unknown domain {other}"),
    }
}

/// Workers: civilians with a full work rate (worker, work boat,
/// archaeologist). Great people (rate 1) and military units that build
/// (legion, samurai) keep `work_rate` as a typed ability; the military
/// ones also list what they build (`military_builds`).
fn is_builder(u: &Value) -> bool {
    text(u, "role") == "civilian" && int(&u["abilities"], "work_rate") >= 100 && !is_archaeologist(u)
}

/// Military units that work like a worker for some builds (Legion: roads,
/// forts; Samurai: fishing boats): a full work rate without the civilian
/// role.
fn is_military_builder(u: &Value) -> bool {
    text(u, "role") != "civilian" && int(&u["abilities"], "work_rate") >= 100
}

/// The improvement each build action makes (`improvements.json` `build.action`).
fn build_actions(cat: &Catalogs) -> Vec<(String, String)> {
    items(&cat.improvements, "items")
        .into_iter()
        .filter_map(|i| Some((i["build"]["action"].as_str()?.to_string(), id(i).to_string())))
        .collect()
}

/// What a military builder builds: the improvements of its worker actions
/// and whether it makes roads. Other actions (remove a feature, repair) are
/// not listed on such units in the catalog.
fn military_builds(u: &Value, actions: &[(String, String)]) -> (Vec<String>, bool) {
    let mut improvements = Vec::new();
    let mut roads = false;
    for action in strings(u, "worker_actions") {
        if action == "core:build/road" {
            roads = true;
        } else if let Some((_, improvement)) = actions.iter().find(|(a, _)| *a == action) {
            improvements.push(improvement.clone());
        }
    }
    (improvements, roads)
}

/// Abilities the combat, air and nuclear systems read: typed in the
/// unit's `combat` record, not inert.
const COMBAT_ABILITIES: [&str; 10] = [
    "capture",
    "ignore_building_defense",
    "immobile",
    "air_intercept_range",
    "suicide",
    "nuke_damage_level",
    "range_attack_only_in_domain",
    "special_cargo",
    "domain_cargo",
    "pillage",
];

/// Cargo kind of a catalog special-unit class name (fighter, missile).
fn cargo_kind(name: &str) -> Option<&'static str> {
    match name {
        "fighter" => Some("Aircraft"),
        "missile" => Some("Missile"),
        _ => None,
    }
}

/// The unit's `combat` record (None when it has no combat ability).
fn unit_combat(u: &Value) -> Option<String> {
    let a = &u["abilities"];
    let mut r = Record::new();
    if let Some(capture) = a["capture"].as_str() {
        r = r.raw("capture", format!("Some({})", crate::import::ron_out::quote(capture)));
    }
    r = r
        .flag("ignore_building_defense", flag(a, "ignore_building_defense"))
        .flag("immobile", flag(a, "immobile"))
        .num("air_intercept_range", int(a, "air_intercept_range"))
        .flag("suicide", flag(a, "suicide"))
        .num("nuke_level", int(a, "nuke_damage_level"))
        .flag("ranged_only_in_domain", flag(a, "range_attack_only_in_domain"));
    if let Some(kind) = a["special_cargo"].as_str().and_then(cargo_kind) {
        r = r.raw("carries", format!("Some({kind})"));
    }
    let air = text(u, "domain") == "core:domain/air";
    if let Some(kind) = a["special"].as_str().and_then(cargo_kind).filter(|_| air) {
        r = r.raw("carried_as", format!("Some({kind})"));
    }
    r = r.flag("pillage", flag(a, "pillage"));
    let text = r.inline();
    if text == "()" {
        return None;
    }
    Some(text)
}

/// Digs antiquity sites (and builds only those): not a worker.
fn is_archaeologist(u: &Value) -> bool {
    u["worker_actions"]
        .as_array()
        .is_some_and(|a| a.iter().any(|x| x.as_str() == Some("core:build/archaeology-dig")))
}

/// A catalog unit ability the engine implements, as a typed RON ability.
fn typed_unit_ability(name: &str, value: &Value) -> Option<String> {
    let n = value.as_i64().unwrap_or(0);
    let flag = value.as_bool().unwrap_or(false);
    Some(match name {
        "found_religion" if flag => "FoundReligion".to_string(),
        "spread_religion" if flag => "SpreadReligion".to_string(),
        "religion_spreads" if n > 0 => format!("ReligionSpreads({n})"),
        "religious_strength" if n > 0 => format!("ReligiousStrength({n})"),
        "remove_heresy" if flag => "RemoveHeresy".to_string(),
        "prohibits_spread" if flag => "ProhibitsSpread".to_string(),
        "requires_enhanced_religion" if flag => "RequiresEnhancedReligion".to_string(),
        "one_shot_tourism" if n > 0 => format!("OneShotTourism({n})"),
        "one_shot_tourism_percent_others" if n > 0 => format!("OneShotTourismPercentOthers({n})"),
        "base_culture_turns_to_count" if n > 0 => format!("CultureTurnsToCount({n})"),
        // The economy and society abilities (track M4-A).
        "food" if flag => "Food".to_string(),
        "golden_age_turns" if n > 0 => format!("GoldenAgeTurns({n})"),
        "base_beakers_turns_to_count" if n > 0 => format!("BeakersTurns({n})"),
        "base_hurry" if n > 0 => format!("BaseHurry({n})"),
        "hurry_multiplier" if n > 0 => format!("HurryMultiplier({n})"),
        "base_gold" if n > 0 => format!("BaseGold({n})"),
        "num_gold_per_era" if n > 0 => format!("GoldPerEra({n})"),
        "no_maintenance" if flag => "NoMaintenance".to_string(),
        "trade" if flag => "Trade".to_string(),
        // Purchase, founding, air and target rules of units.
        "requires_faith_purchase_enabled" if flag => "FaithPurchaseNeedsBelief".to_string(),
        "mechanized" if flag => "Mechanized".to_string(),
        "civilian_attack_priority" => {
            let priority = match value.as_str()? {
                "high" => "High",
                "high_early_game_only" => "HighEarlyGameOnly",
                "low" => "Low",
                _ => return None,
            };
            format!("CivilianAttackPriority({priority})")
        }
        "conscription" if n > 0 => format!("Conscription({n})"),
        "special" => match value.as_str()? {
            "people" => "Special(People)".to_string(),
            "stealth" => "Special(Stealth)".to_string(),
            _ => return None,
        },
        "air_unit_cap" if n > 0 => format!("AirSlots({n})"),
        // Great people (1: their improvement is instant) and the military
        // builders (100): data next to the builds the unit lists.
        "work_rate" if n > 0 => format!("WorkRate({n})"),
        "move_after_purchase" if flag => "MoveAfterPurchase".to_string(),
        "no_bad_goodies" if flag => "NoBadGoodies".to_string(),
        "purchase_only" if flag => "PurchaseOnly".to_string(),
        "found_abroad" if flag => "FoundAbroad".to_string(),
        "can_buy_city_state" if flag => "BuyCityState".to_string(),
        "num_exotic_goods" if n > 0 => format!("ExoticGoods({n})"),
        _ => return None,
    })
}

/// Unit abilities other than founding and the worker rate: typed abilities
/// the engine implements, inert entries for the rest.
fn unit_abilities(u: &Value, fx: &mut Effects, typed: &mut Vec<String>) {
    let builder = is_builder(u) || is_archaeologist(u);
    if is_archaeologist(u) {
        typed.push("Excavates".to_string());
    }
    let Some(abilities) = u["abilities"].as_object() else {
        return;
    };
    let air = text(u, "domain") == "core:domain/air";
    let mut names: Vec<&String> = abilities.keys().collect();
    names.sort();
    for name in names {
        let value = &abilities[name.as_str()];
        if name == "found" || (name == "work_rate" && builder) {
            continue;
        }
        if name == "project_prereq" || name == "spaceship_project" || name == "policy_type" {
            continue;
        }
        if COMBAT_ABILITIES.contains(&name.as_str()) {
            continue;
        }
        let typed_special = value.as_str().and_then(cargo_kind).is_some() && air;
        if name == "special" && typed_special {
            continue;
        }
        if let Some(ability) = typed_unit_ability(name, value) {
            typed.push(ability);
            continue;
        }
        let mut modifier = format!("unit.ability.{name}");
        if let Some(s) = value.as_str() {
            if !s.starts_with("core:") {
                modifier = format!("{modifier}={s}");
            }
        }
        let mechanic = format!("unit_ability.{name}");
        fx.inert(field_inert(&mechanic, &modifier, "unit", value));
    }
}

fn unit(u: &Value, actions: &[(String, String)]) -> Record {
    let mut fx = Effects::default();
    let mut abilities = Vec::new();
    unit_abilities(u, &mut fx, &mut abilities);
    let cost = int(u, "cost");
    let unbuildable = cost <= 0;
    let mut r = Record::new()
        .str("id", id(u))
        .str("name", text(u, "name"))
        .opt_str("class", opt(u, "class"))
        .raw("role", format!("Some({})", unit_role(text(u, "role"))))
        .raw("domain", domain(text(u, "domain")))
        .raw("movement", int(u, "moves").to_string())
        .raw("strength", int(u, "combat").to_string())
        .num("ranged_strength", int(u, "ranged_combat"))
        .num("range", int(u, "range"))
        .num("sight", int(u, "sight"))
        .raw("cost", cost.max(0).to_string())
        .flag("unbuildable", unbuildable);
    if let Some(faith) = u["faith_cost"].as_i64() {
        r = r.raw("faith_cost", format!("Some({faith})"));
    }
    let r = r
        .flag("founder", flag(&u["abilities"], "found"))
        .flag("builder", is_builder(u));
    let r = opt_key(r, "required_tech", u, "required_tech");
    let r = opt_key(r, "obsolete_tech", u, "obsolete_tech")
        .lines(
            "required_resources",
            key_counts(&u["required_resources"], "resource", "amount"),
        )
        .strs("upgrades_to", &strings(u, "upgrades_to"))
        .strs("free_promotions", &strings(u, "free_promotions"))
        .opt_str("era", opt(u, "era"));
    let r = opt_key(r, "nation", u, "nation");
    let mut r = opt_key(r, "replaces", u, "replaces")
        .strs(
            "requires_buildings",
            &strings(u, "requires_buildings_in_city"),
        )
        .lines("abilities", abilities);
    if let Some(combat) = unit_combat(u) {
        r = r.raw("combat", combat);
    }
    let r = opt_key(r, "required_project", &u["abilities"], "project_prereq");
    let r = opt_key(r, "spaceship_project", &u["abilities"], "spaceship_project");
    let r = opt_key(r, "required_policy", &u["abilities"], "policy_type");
    let r = if is_military_builder(u) {
        let (builds, roads) = military_builds(u, actions);
        r.strs("builds", &builds).flag("builds_roads", roads)
    } else {
        r
    };
    fx.write(r, true)
}

pub fn units(cat: &Catalogs) -> Vec<Record> {
    let actions = build_actions(cat);
    items(&cat.units, "items").into_iter().map(|u| unit(u, &actions)).collect()
}

pub fn unique_units(cat: &Catalogs) -> Vec<Record> {
    let actions = build_actions(cat);
    items(&cat.unique_units, "items")
        .into_iter()
        .map(|u| unit(u, &actions))
        .collect()
}

// ---- buildings and wonders ----

fn building_kind(kind: &str) -> &'static str {
    match kind {
        "regular" => "Regular",
        "religious" => "Religious",
        "guild" => "Guild",
        "palace" => "Palace",
        "unique" => "Unique",
        "world_wonder" => "WorldWonder",
        "national_wonder" => "NationalWonder",
        other => panic!("unknown building kind {other}"),
    }
}

fn great_person_kind(specialist: &str) -> &'static str {
    match slug(specialist) {
        "writer" => "Writer",
        "artist" => "Artist",
        "musician" => "Musician",
        "scientist" => "Scientist",
        "merchant" => "Merchant",
        "engineer" => "Engineer",
        other => panic!("specialist {other} has no great person"),
    }
}

/// Placement requirements the engine checks (`placement`), as RON.
const BUILDING_PLACEMENT_FLAGS: [&str; 7] = [
    "water",
    "river",
    "mountain",
    "fresh_water",
    "flat",
    "capital",
    "nearby_mountain_required",
];

/// `(water: true, ...)` of a building's typed placement; None when empty.
fn building_placement(b: &Value) -> Option<String> {
    let reqs = b["requirements"].as_object();
    let mut set = Vec::new();
    for (name, value) in reqs.into_iter().flatten() {
        let field = if name == "nearby_mountain_required" { "nearby_mountain" } else { name.as_str() };
        if BUILDING_PLACEMENT_FLAGS.contains(&name.as_str()) && value.as_bool() == Some(true) {
            set.push(format!("{field}: true"));
        }
        match name.as_str() {
            "nearby_terrain" | "prohibited_city_terrain" => {
                if let Some(k) = value.as_str() {
                    set.push(format!("{name}: Some({})", quote(k)));
                }
            }
            "local_resources_any" | "local_resources_all" => {
                let keys: Vec<String> = strings(&b["requirements"], name).iter().map(|k| quote(k)).collect();
                set.push(format!("{name}: [{}]", keys.join(", ")));
            }
            "mutually_exclusive_group" => {
                if let Some(g) = value.as_i64() {
                    set.push(format!("exclusive_group: Some({g})"));
                }
            }
            _ => {}
        }
    }
    let effects = b["effects"].as_array().map(|a| a.as_slice()).unwrap_or(&[]);
    if effects.iter().any(|e| e["modifier"].as_str() == Some("building.allows_water_routes")) {
        set.push("allows_water_routes: true".to_string());
    }
    if set.is_empty() {
        return None;
    }
    set.sort();
    Some(format!("({})", set.join(", ")))
}

/// Requirements the engine has no rule for (another track's): inert.
const TYPED_BUILDING_REQUIREMENTS: [&str; 14] = [
    "buildings_in_city",
    "buildings_in_empire",
    "note_count",
    "water",
    "river",
    "mountain",
    "fresh_water",
    "flat",
    "capital",
    "nearby_mountain_required",
    "nearby_terrain",
    "prohibited_city_terrain",
    "local_resources_any",
    "local_resources_all",
];

/// Requirements other than buildings and placement: inert rules.
fn building_requirements(b: &Value, fx: &mut Effects) {
    let Some(reqs) = b["requirements"].as_object() else {
        return;
    };
    let mut names: Vec<&String> = reqs.keys().collect();
    names.sort();
    for name in names {
        // `unlocked_by_league_project`: the building is unbuildable and a
        // World Congress project rewards it (the compiler checks both).
        let typed = [
            "policy_branch",
            "holy_city",
            "unlocked_by_league_project",
            "faith_purchase_unlocked_by_belief",
        ];
        if TYPED_BUILDING_REQUIREMENTS.contains(&name.as_str())
            || name == "mutually_exclusive_group"
            || typed.contains(&name.as_str())
        {
            continue;
        }
        let modifier = format!("building.requirement.{name}");
        let mechanic = format!("building_requirement.{name}");
        fx.inert(field_inert(
            &mechanic,
            &modifier,
            "city",
            &reqs[name.as_str()],
        ));
    }
}

/// A catalog building flag the engine implements, as a typed RON ability.
fn typed_building_ability(e: &Value) -> Option<String> {
    let n = e["amount"].as_i64().unwrap_or(0);
    match e["modifier"].as_str() {
        // Parthenon: the frieze, a great work of art by the owner.
        Some("grant.great_work") => return Some("FreeGreatWork".into()),
        Some("missionary.spreads.flat") if n > 0 => return Some(format!("MissionaryExtraSpreads({n})")),
        Some("religion.pressure_out.pct") if e["scope"].as_str() == Some("city") && n > 0 => {
            return Some(format!("ReligiousPressure({n})"));
        }
        _ => {}
    }
    match e["needs_mechanic"].as_str()? {
        "x_built_triggers_ideology_choice" if n > 0 => Some(format!("TriggersIdeologyChoice({n})")),
        "nullify_tourism_tech_bonus" => Some("NullifyInfluence".into()),
        "internal_food_trade_routes" => Some("FoodTradeRoutes".into()),
        "internal_production_trade_routes" => Some("ProductionTradeRoutes".into()),
        "espionage_building" => Some("CounterEspionage".into()),
        "espionage_rank_now" => Some("AffectSpiesNow".into()),
        "border_obstacle" => Some("BorderObstacle".into()),
        "airlift" => Some("Airlift".into()),
        // Walls, Castle, Arsenal, Military Base and their replacements.
        "city_ranged_strike" => Some("AllowsRangeStrike".into()),
        _ => None,
    }
}

/// One catalog theming bonus as a RON `ThemingBonus` (the description is
/// UI text and stays in the catalog).
fn theming_bonus(theme: &Value) -> String {
    let mut parts = vec![format!("bonus: {}", theme["bonus"].as_i64().unwrap_or(0))];
    for flag in [
        "same_era",
        "unique_eras",
        "must_be_art",
        "must_be_artifact",
        "must_be_equal_art_artifact",
        "requires_owner",
        "requires_any_but_owner",
        "requires_same_player",
        "requires_unique_players",
    ] {
        if theme[flag].as_bool() == Some(true) {
            parts.push(format!("{flag}: true"));
        }
    }
    format!("({})", parts.join(", "))
}

fn building(b: &Value) -> Record {
    // `building.allows_water_routes` is typed placement (`building_placement`).
    let mut fx = Effects::default();
    let mut abilities = Vec::new();
    for e in b["effects"].as_array().map(|a| a.as_slice()).unwrap_or(&[]) {
        if e["modifier"].as_str() == Some("building.allows_water_routes") {
            continue;
        }
        match typed_building_ability(e) {
            Some(ability) => abilities.push(ability),
            None => fx.push(e, Owner::Building),
        }
    }
    // The National Intelligence Agency's spy is given at the old
    // starting rank and then every spy is raised one rank (rules.md): the
    // new spy ends at the new starting rank. The starting rank here is a
    // passive stat, already raised when OnBuilt fires, so the rank-up runs
    // before the spy is given (stable: the rest keeps catalog order).
    fx.typed.sort_by_key(|t| t.contains("GrantSpies("));
    if let Some(gpp) = b["great_person_points"].as_object() {
        let kind = great_person_kind(gpp["specialist"].as_str().unwrap_or(""));
        let amount = gpp["per_turn"].as_i64().unwrap_or(0);
        fx.typed.push(format!(
            "(scope: City, selector: SourceCity, modifier: AddGreatPersonPoints(kind: {kind}, amount: {amount}))"
        ));
    }
    let theming: Vec<String> = b["theming_bonuses"]
        .as_array()
        .map(|themes| themes.iter().map(theming_bonus).collect())
        .unwrap_or_default();
    building_requirements(b, &mut fx);
    let cost = int(b, "cost");
    let kind = building_kind(text(b, "kind"));
    let mut r = Record::new()
        .str("id", id(b))
        .str("name", text(b, "name"))
        .raw("kind", kind)
        .opt_str("class", opt(b, "class"))
        .raw("cost", cost.max(0).to_string())
        .flag("unbuildable", cost <= 0);
    if let Some(faith) = b["faith_cost"].as_i64() {
        r = r.raw("faith_cost", format!("Some({faith})"));
    }
    let r = opt_key(r, "required_tech", b, "required_tech");
    let mut r = opt_key(r, "obsolete_tech", b, "obsolete_tech")
        .num("maintenance", int(b, "maintenance"))
        .opt_str("era", opt(b, "era"))
        .num("hurry_cost_modifier", int(b, "hurry_cost_modifier"))
        .num("conquest_survival_pct", int(b, "conquest_survival_pct"))
        .flag("never_capture", flag(b, "never_capture"));
    if let Some(s) = b["specialists"].as_object() {
        let key = s["type"].as_str().unwrap_or("");
        r = r.raw(
            "specialist_slots",
            format!(
                "Some(({}, {}))",
                quote(key),
                s["slots"].as_i64().unwrap_or(0)
            ),
        );
    }
    if let Some(s) = b["great_work_slots"].as_object() {
        let key = s["type"].as_str().unwrap_or("");
        r = r.raw(
            "great_work_slots",
            format!(
                "Some(({}, {}))",
                quote(key),
                s["count"].as_i64().unwrap_or(0)
            ),
        );
    }
    let reqs = &b["requirements"];
    let r = r
        .lines(
            "consumes_resources",
            key_counts(&b["consumes_resources"], "resource", "amount"),
        )
        .strs("requires_buildings", &strings(reqs, "buildings_in_city"))
        .strs("requires_buildings_in_empire", &in_all_cities(reqs))
        .num("cost_per_city_pct", int(b, "cost_per_city_pct"));
    let r = match building_placement(b) {
        Some(p) => r.raw("placement", p),
        None => r,
    };
    let r = opt_key(r, "max_start_era", b, "max_start_era");
    let r = opt_key(r, "free_start_era", b, "free_start_era");
    let r = opt_key(r, "nation", b, "nation");
    let r = opt_key(r, "replaces", b, "replaces")
        .lines("abilities", abilities)
        .lines("theming", theming);
    let r = opt_key(r, "requires_policy_branch", reqs, "policy_branch")
        .flag("requires_holy_city", flag(reqs, "holy_city"))
        .flag(
            "faith_purchase_needs_belief",
            flag(reqs, "faith_purchase_unlocked_by_belief"),
        );
    fx.write(r, true)
}

/// Buildings every city of the empire needs (`buildings_in_empire` with
/// `count_in_all_cities: -1`, national wonders).
fn in_all_cities(reqs: &Value) -> Vec<String> {
    let items = reqs["buildings_in_empire"].as_array().map(|a| a.as_slice()).unwrap_or(&[]);
    items
        .iter()
        .filter(|i| i["count_in_all_cities"].as_i64() == Some(-1))
        .filter_map(|i| i["building"].as_str().map(String::from))
        .collect()
}

pub fn buildings(cat: &Catalogs) -> Vec<Record> {
    items(&cat.buildings, "items")
        .into_iter()
        .map(building)
        .collect()
}

pub fn unique_buildings(cat: &Catalogs) -> Vec<Record> {
    items(&cat.unique_buildings, "items")
        .into_iter()
        .map(building)
        .collect()
}

pub fn wonders(cat: &Catalogs) -> Vec<Record> {
    items(&cat.wonders, "items")
        .into_iter()
        .filter(|w| matches!(text(w, "kind"), "world_wonder" | "national_wonder"))
        .map(building)
        .collect()
}

/// The technology prerequisite of the resolutions that start the
/// World Congress projects (classic/docs/classic/rules.md; the catalog's project
/// records do not carry it): International Games need Radio, the
/// International Space Station Satellites, the World's Fair nothing.
const LEAGUE_PROJECT_TECHS: [(&str, &str); 2] = [
    ("core:project/international_games", "core:tech/radio"),
    ("core:project/international_space_station", "core:tech/satellites"),
];

/// One catalog reward effect as a RON `LeagueReward`; None: no typed
/// reward (stays inert).
fn league_reward(e: &Value) -> Option<String> {
    let n = e["amount"].as_i64()?;
    let target = || e["target"].as_str().map(quote);
    Some(match e["modifier"].as_str()? {
        "league_reward.happiness" => format!("Happiness({n})"),
        "league_reward.free_social_policies" => format!("FreePolicies({n})"),
        "league_reward.golden_age_points" => format!("GoldenAgePoints({n})"),
        "league_reward.culture_bonus_turns" => format!("CultureBoostTurns({n})"),
        "league_reward.tourism_bonus_turns" => format!("TourismBoostTurns({n})"),
        "league_reward.city_state_influence_boost" => format!("CityStateInfluence({n})"),
        "league_reward.base_beakers_turns_to_count" => format!("ResearchTurns({n})"),
        "league_reward.free_unit_class" if n == 1 => format!("FreeUnit({})", target()?),
        "league_reward.building" if n == 1 => format!("Building({})", target()?),
        _ => return None,
    })
}

/// The `league: Some(..)` record of a World Congress project: its cost per
/// nation, the technology a member must know, the rewards of its
/// three tiers (untyped rewards go to `fx` as inert).
fn league_project(p: &Value, fx: &mut Effects) -> String {
    let mut tiers = Vec::new();
    for tier in p["reward_tiers"].as_array().map(|a| a.as_slice()).unwrap_or(&[]) {
        let mut rewards = Vec::new();
        for e in tier["effects"].as_array().map(|a| a.as_slice()).unwrap_or(&[]) {
            match league_reward(e) {
                Some(reward) => rewards.push(reward),
                None => fx.push(e, Owner::Other),
            }
        }
        tiers.push(format!("[{}]", rewards.join(", ")));
    }
    let tech = LEAGUE_PROJECT_TECHS
        .iter()
        .find(|(project, _)| *project == id(p))
        .map(|(_, tech)| format!(", tech_any_member: Some({})", quote(tech)))
        .unwrap_or_default();
    format!(
        "Some((cost_per_player: {}{tech}, tiers: [{}]))",
        int(p, "cost_per_player"),
        tiers.join(", ")
    )
}

pub fn projects(cat: &Catalogs) -> Vec<Record> {
    items(&cat.wonders, "items")
        .into_iter()
        .filter(|w| matches!(text(w, "kind"), "project" | "league_project"))
        .map(|p| {
            let league = text(p, "kind") == "league_project";
            let mut fx = Effects::default();
            let mut allows_nukes = false;
            for e in p["effects"].as_array().map(|a| a.as_slice()).unwrap_or(&[]) {
                if e["modifier"].as_str() == Some("project.allows_nukes") && e["amount"].as_bool() == Some(true) {
                    allows_nukes = true;
                    continue;
                }
                fx.push(e, Owner::Other);
            }
            let league_record = league.then(|| league_project(p, &mut fx));
            let r = Record::new()
                .str("id", id(p))
                .str("name", text(p, "name"))
                .raw("kind", if league { "LeagueProject" } else { "Project" })
                .raw(
                    "cost",
                    if league { 0 } else { int(p, "cost").max(0) }.to_string(),
                )
                .flag("unbuildable", !league && int(p, "cost") <= 0);
            let r = opt_key(r, "required_tech", p, "required_tech")
                .lines(
                    "prerequisite_projects",
                    key_counts(&p["prerequisite_projects"], "project", "amount"),
                )
                .num(
                    "max_global_instances",
                    int(p, "max_global_instances").max(0),
                )
                .num("max_team_instances", int(p, "max_team_instances").max(0));
            let mut r = opt_key(r, "victory", p, "victory").flag("allows_nukes", allows_nukes);
            if let Some(text) = league_record {
                r = r.raw("league", text);
            }
            fx.write(r, false)
        })
        .collect()
}

// ---- improvements and resources ----

fn improvement_kind(kind: &str) -> &'static str {
    match kind {
        "regular" => "Regular",
        "great_person" => "GreatPerson",
        "unique" => "Unique",
        "barbarian_camp" => "BarbarianCamp",
        "ancient_ruins" => "AncientRuins",
        other => panic!("unknown improvement kind {other}"),
    }
}

fn fresh_water_yields(i: &Value) -> Vec<String> {
    let mut out = Vec::new();
    for change in i["tech_yield_changes"]
        .as_array()
        .map(|a| a.as_slice())
        .unwrap_or(&[])
    {
        let fresh = match change["condition"].as_str() {
            None => continue,
            Some("fresh_water") => "Required",
            Some("no_fresh_water") => "Absent",
            Some(other) => panic!("unknown improvement yield condition {other}"),
        };
        out.push(format!(
            "(tech: {}, yield_kind: {}, amount: {}, fresh_water: {fresh})",
            quote(text(change, "tech")),
            yield_name(text(change, "yield")),
            int(change, "amount")
        ));
    }
    out
}

/// Placement flags the engine reads (`ImprovementPlacement`); the others
/// stay inert.
const PLACEMENT_FLAGS: [&str; 16] = [
    "ignore_ownership",
    "only_city_state_territory",
    "requires_improvement",
    "prompt_when_complete",
    "destroyed_when_pillaged",
    "hills_makes_valid",
    "fresh_water_makes_valid",
    "requires_flatlands_or_fresh_water",
    "requires_feature",
    "buildable_on_resources",
    "water",
    "coastal",
    "no_two_adjacent",
    "adjacent_luxury",
    "in_adjacent_friendly",
    "outside_borders",
];

/// `(flag: true, ...)` of the typed placement flags, None when none is set.
fn placement(i: &Value, fx: &mut Effects) -> Option<String> {
    let map = i["placement"].as_object()?;
    let mut set = Vec::new();
    for (k, v) in map {
        if PLACEMENT_FLAGS.contains(&k.as_str()) {
            if v.as_bool() == Some(true) {
                set.push(format!("{k}: true"));
            }
            continue;
        }
        let name = format!("improvement.placement.{k}");
        fx.inert(field_inert("improvement_placement", &name, "tile", v));
    }
    if set.is_empty() {
        return None;
    }
    set.sort();
    Some(format!("({})", set.join(", ")))
}

pub fn improvements(cat: &Catalogs) -> Vec<Record> {
    items(&cat.improvements, "items")
        .into_iter()
        .map(|i| {
            let (fields, other_effects) = improvement_fields(&i["effects"]);
            let mut fx = Effects::of(&other_effects, Owner::Other);
            let placement = placement(i, &mut fx);
            for field in ["yield_per_era"] {
                if let Some(map) = i[field].as_object() {
                    for (y, amount) in map {
                        if field == "yield_per_era" && y == "culture" {
                            continue;
                        }
                        let name = format!("improvement.{field}.{y}");
                        fx.inert(field_inert(
                            &format!("improvement.{field}"),
                            &name,
                            "tile",
                            amount,
                        ));
                    }
                }
            }
            let build = &i["build"];
            let work_time = int(build, "work_time");
            let mut r = Record::new()
                .str("id", id(i))
                .str("name", text(i, "name"))
                .raw("kind", improvement_kind(text(i, "kind")))
                .flag("unbuildable", work_time <= 0);
            if let Some(y) = yields(&i["yields"]) {
                r = r.raw("yields", y);
            }
            let r = r.num("work_time", work_time);
            let r = opt_key(r, "build_action", build, "action");
            let r = opt_key(r, "required_tech", build, "required_tech")
                .strs("valid_terrains", &strings(i, "valid_terrains"))
                .strs("valid_features", &strings(i, "valid_features"))
                .strs("resources", &strings(i, "resources"))
                .lines("fresh_water_yields", fresh_water_yields(i))
                .num("defense_pct", int(i, "defense_pct"))
                .num("pillage_gold", int(i, "pillage_gold"))
                .num("culture_bomb_radius", fields.culture_bomb_radius)
                .num("nearby_enemy_damage", fields.nearby_enemy_damage)
                .flag("removes_resource", fields.removes_resource)
                .num("adjacent_same_culture", fields.adjacent_same_culture)
                .num("city_state_luxury_copies", fields.city_state_luxury_copies)
                .num("culture_per_era", int(&i["yield_per_era"], "culture"));
            let mut r = opt_key(r, "nation", i, "nation_unique").flag(
                "created_by_great_person",
                flag(i, "created_by_great_person"),
            );
            if let Some(p) = placement {
                r = r.raw("placement", p);
            }
            if let Some(y) = yields(&i["adjacent_mountain_yield"]) {
                r = r.raw("adjacent_mountain_yields", y);
            }
            fx.write(r, false)
        })
        .collect()
}

/// The improvement effects its systems read as fields: combat (citadel:
/// culture bomb radius, damage to adjacent enemies), archaeology (the dig
/// removes the site), tile yields (Moai culture per adjacent Moai),
/// resources (Feitoria luxury copies from the city-state).
#[derive(Default)]
struct ImprovementFields {
    culture_bomb_radius: i64,
    nearby_enemy_damage: i64,
    removes_resource: bool,
    adjacent_same_culture: i64,
    city_state_luxury_copies: i64,
}

/// The improvement's fields and its other effects.
fn improvement_fields(effects: &Value) -> (ImprovementFields, Value) {
    let mut fields = ImprovementFields::default();
    let mut rest = Vec::new();
    for e in effects.as_array().into_iter().flatten() {
        let amount = e["amount"].as_f64().unwrap_or(0.0) as i64;
        match e["modifier"].as_str() {
            Some("improvement.culture_bomb_radius") => fields.culture_bomb_radius = amount,
            Some("improvement.nearby_enemy_damage") => fields.nearby_enemy_damage = amount,
            Some("improvement.removes_resource") => fields.removes_resource = e["amount"] == Value::Bool(true),
            Some("improvement.culture_adjacent_same_type") => fields.adjacent_same_culture = amount,
            Some("improvement.luxury_copies_siphoned_from_minor") => fields.city_state_luxury_copies = amount,
            _ => rest.push(e.clone()),
        }
    }
    (fields, Value::Array(rest))
}

fn resource_class(class: &str) -> &'static str {
    match class {
        "bonus" => "Bonus",
        "strategic" => "Strategic",
        "luxury" => "Luxury",
        "archaeology" => "Artifact",
        other => panic!("unknown resource class {other}"),
    }
}

/// Placement density of the generator (percent of eligible tiles). The
/// catalogs do not describe how resources are placed on the map: this is a
/// generator approximation by class, 0 for resources the standard map never
/// places.
fn density(r: &Value, terrains: &[String]) -> i64 {
    let on_map = terrains
        .iter()
        .any(|t| map_code(t).filter(|c| *c != 4).is_some());
    if !on_map || !placed_on_map(r) {
        return 0;
    }
    match text(r, "class") {
        "bonus" => 3,
        "strategic" => strategic_land_density(id(r)),
        "luxury" => 1,
        _ => 0,
    }
}

/// Does the standard map place the resource at all (not a scenario-only,
/// city-state or nation resource)?
fn placed_on_map(r: &Value) -> bool {
    let placed = flag(r, "in_standard_game") || r["in_standard_game"].is_null();
    placed && !flag(r, "only_city_states") && r["nation_unique"].is_null()
}

/// Percent of the eligible land tiles of a strategic resource. The generator
/// places by terrain only, so a flat 2 % gave the modern-era trio very
/// different totals (a standard continents map: uranium 30, aluminium 16,
/// oil 6 deposits, oil having only desert, tundra and snow to lie on).
/// The modern strategics are balanced to one another by the sum of land
/// and sea (`sea_density`); `lands/own-game-docs/DECISIONS.md`, "Awaiting numbers".
fn strategic_land_density(id: &str) -> i64 {
    match id {
        "core:resource/oil" => OIL_LAND_DENSITY,
        "core:resource/uranium" => URANIUM_LAND_DENSITY,
        _ => 2,
    }
}

const OIL_LAND_DENSITY: i64 = 4;
const URANIUM_LAND_DENSITY: i64 = 1;

/// Percent of the shallow sea (water within two tiles of land) a resource is
/// placed on, 0 for one that does not grow at sea: the resources whose
/// terrains include the coast. Fish, whales, pearls, crab and oil sit on
/// coast tiles; the catalogs do not give counts, so by class: a bonus
/// (fish) 4 %, a luxury (whales, pearls, crab) 2 %, a strategic resource
/// (oil) 1 %. A tile of the first ring takes the whole percent, the second
/// ring half (`kernel::scenario::sea_resources`).
fn sea_density(r: &Value, terrains: &[String]) -> i64 {
    let at_sea = terrains.iter().any(|t| map_code(t) == Some(4));
    if !at_sea || !placed_on_map(r) {
        return 0;
    }
    match text(r, "class") {
        "bonus" => 4,
        "luxury" => 2,
        "strategic" => 1,
        _ => 0,
    }
}

/// Marble: `Some((percent: 15, before_era: Some("core:era/medieval")))`
/// (the catalog's wonder production percent and its obsolete era).
fn wonder_production(res: &Value) -> Option<String> {
    let percent = res["wonder_production_pct"].as_i64()?;
    let before_era = match res["wonder_production_obsolete_era"].as_str() {
        Some(era) => format!("Some({})", quote(era)),
        None => "None".to_string(),
    };
    Some(format!("Some((percent: {percent}, before_era: {before_era}))"))
}

pub fn resources(cat: &Catalogs) -> Vec<Record> {
    items(&cat.resources, "items")
        .into_iter()
        .map(|res| {
            let fx = Effects::default();
            let mut improved = Vec::new();
            if let Some(map) = res["improved_yields"].as_object() {
                let mut keys: Vec<&String> = map.keys().collect();
                keys.sort();
                for improvement in keys {
                    if let Some(y) = yields(&map[improvement.as_str()]) {
                        improved.push(format!("({}, {y})", quote(improvement)));
                    }
                }
            }
            let mut terrains = strings(res, "terrains");
            if flag(res, "hills") {
                terrains.push("core:terrain/hill".into());
            }
            terrains.sort();
            terrains.dedup();
            let mut r = Record::new()
                .str("id", id(res))
                .str("name", text(res, "name"))
                .raw(
                    "class",
                    format!("Some({})", resource_class(text(res, "class"))),
                );
            if let Some(y) = yields(&res["yields"]) {
                r = r.raw("yields", y);
            }
            let r = r.num("happiness", int(res, "happiness"));
            let r = opt_key(r, "required_tech", res, "revealed_by_tech")
                .strs("improvements", &strings(res, "improvement"));
            let r = opt_key(r, "tradeable_with_tech", res, "tradeable_with_tech")
                .raw("density", density(res, &terrains).to_string())
                .num("sea_density", sea_density(res, &terrains))
                .strs("valid_terrains", &terrains)
                .strs("valid_features", &strings(res, "features"))
                .flag("hills", flag(res, "hills"))
                .flag("flatlands", flag(res, "flatlands"));
            let in_game = res["in_standard_game"].as_bool().unwrap_or(true);
            let r = if in_game {
                r
            } else {
                r.raw("in_standard_game", "false")
            };
            let r = r.flag("only_city_states", flag(res, "only_city_states"));
            let r = opt_key(r, "nation", res, "nation_unique");
            let r = opt_key(r, "revealed_by_policy", res, "revealed_by_policy");
            let quantities: Vec<String> = res["quantities"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|q| q.as_i64())
                        .map(|q| q.to_string())
                        .collect()
                })
                .unwrap_or_default();
            let r = if quantities.is_empty() {
                r
            } else {
                r.raw("quantities", format!("[{}]", quantities.join(", ")))
            };
            let r = if improved.is_empty() {
                r
            } else {
                r.raw("improved_yields", format!("[{}]", improved.join(", ")))
            };
            let r = match wonder_production(res) {
                Some(w) => r.raw("wonder_production", w),
                None => r,
            };
            fx.write(r, false)
        })
        .collect()
}

// ---- policies, beliefs, promotions ----

pub fn policy_branches(cat: &Catalogs) -> Vec<Record> {
    items(&cat.policies, "branches")
        .into_iter()
        .map(|b| {
            let kind = match text(b, "kind") {
                "branch" => "Normal",
                "ideology" => "Ideology",
                other => panic!("unknown policy branch kind {other}"),
            };
            let r = Record::new()
                .str("id", id(b))
                .str("name", text(b, "name"))
                .raw("kind", kind);
            let r = opt_key(r, "era_prerequisite", b, "era_prerequisite");
            let r = opt_key(r, "opener", b, "opener");
            opt_key(r, "finisher", b, "finisher")
                .strs(
                    "mutually_exclusive_with",
                    &strings(b, "mutually_exclusive_with"),
                )
                .flag(
                    "locked_without_religion",
                    flag(b, "locked_without_religion"),
                )
                .num(
                    "first_adopter_free_policies",
                    int(b, "first_adopter_free_policies"),
                )
                .num(
                    "second_adopter_free_policies",
                    int(b, "second_adopter_free_policies"),
                )
        })
        .collect()
}

/// Catalog policy effects that other typed data already carries. The
/// Exploration finisher's text (`hardcoded_policy_exploration_finisher`):
/// hidden antiquity sites are seen through the site's `revealed_by_policy`
/// (resources), Great Admirals are bought with faith from the Industrial
/// era once the branch is complete (`derived::religion`).
fn without_covered_policy_effects(effects: &Value) -> Value {
    let kept: Vec<Value> = effects
        .as_array()
        .into_iter()
        .flatten()
        .filter(|e| e["needs_mechanic"].as_str() != Some("hardcoded_policy_exploration_finisher"))
        .cloned()
        .collect();
    Value::Array(kept)
}

pub fn policies(cat: &Catalogs) -> Vec<Record> {
    items(&cat.policies, "items")
        .into_iter()
        .map(|p| {
            let role = match text(p, "role") {
                "policy" => "Policy".to_string(),
                "tenet" => format!("Tenet({})", int(p, "tier")),
                "opener" => "Opener".to_string(),
                "finisher" => "Finisher".to_string(),
                other => panic!("unknown policy role {other}"),
            };
            let fx = Effects::of(&without_covered_policy_effects(&p["effects"]), Owner::Policy);
            let grid = &p["grid"];
            let mut r = Record::new()
                .str("id", id(p))
                .str("name", text(p, "name"))
                .opt_str("branch", opt(p, "branch"))
                .raw("role", role)
                .strs("prerequisites", &strings(p, "prerequisites"));
            if grid.is_array() {
                r = r.raw(
                    "grid",
                    format!(
                        "({}, {})",
                        grid[0].as_i64().unwrap_or(0),
                        grid[1].as_i64().unwrap_or(0)
                    ),
                );
            }
            fx.write(r.text("text", text(p, "text")), true)
        })
        .collect()
}

pub fn religions(cat: &Catalogs) -> Vec<Record> {
    items(&cat.beliefs, "religions")
        .into_iter()
        .map(|r| Record::new().str("id", id(r)).str("name", text(r, "name")))
        .collect()
}

pub fn beliefs(cat: &Catalogs) -> Vec<Record> {
    items(&cat.beliefs, "items")
        .into_iter()
        .map(|b| {
            let category = match text(b, "category") {
                "pantheon" => "Pantheon",
                "founder" => "Founder",
                "follower" => "Follower",
                "enhancer" => "Enhancer",
                "reformation" => "Reformation",
                other => panic!("unknown belief category {other}"),
            };
            let fx = Effects::of(&b["effects"], Owner::Belief);
            let r = Record::new()
                .str("id", id(b))
                .str("name", text(b, "name"))
                .raw("category", format!("Some({category})"))
                .text("text", text(b, "text"));
            fx.write(r, true)
        })
        .collect()
}

pub fn promotions(cat: &Catalogs) -> Vec<Record> {
    items(&cat.promotions, "items")
        .into_iter()
        .map(|p| {
            let fx = Effects::of(&p["effects"], Owner::Promotion);
            let roles: Vec<String> = strings(p, "unit_combats")
                .iter()
                .map(|k| unit_role(slug(k)).to_string())
                .collect();
            let r = Record::new()
                .str("id", id(p))
                .str("name", text(p, "name"))
                .strs("requires", &strings(p, "requires"))
                .strs("requires_any_of", &strings(p, "requires_any_of"))
                .flag("choosable", flag(p, "choosable"))
                .flag("lost_with_upgrade", flag(p, "lost_with_upgrade"));
            let r = opt_key(r, "required_tech", p, "required_tech");
            let r = if roles.is_empty() {
                r
            } else {
                r.raw("unit_roles", format!("[{}]", roles.join(", ")))
            };
            let r = r.strs("civilian_units", &strings(p, "civilian_units"));
            fx.write(r, true)
        })
        .collect()
}

// ---- nations, city-states, great people ----

/// Team colors: the ten nations of the first content keep theirs;
/// the catalog has no colors (player colors are not exported), so the
/// others get a deterministic hue from their key — a display choice, not a
/// rule.
fn nation_color(key: &str, legacy: &[(String, (u8, u8, u8))]) -> (u8, u8, u8) {
    if let Some((_, c)) = legacy.iter().find(|(k, _)| k == key) {
        return *c;
    }
    let mut h: u32 = 0x811c_9dc5;
    for b in key.bytes() {
        h ^= u32::from(b);
        h = h.wrapping_mul(0x0100_0193);
    }
    let hue = (h % 360) as f32;
    hsv(hue, 0.55, 0.78)
}

fn hsv(hue: f32, s: f32, v: f32) -> (u8, u8, u8) {
    let c = v * s;
    let x = c * (1.0 - ((hue / 60.0) % 2.0 - 1.0).abs());
    let m = v - c;
    let (r, g, b) = match (hue / 60.0) as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let byte = |f: f32| ((f + m) * 255.0).round() as u8;
    (byte(r), byte(g), byte(b))
}

fn start_region(name: &str) -> &'static str {
    match name {
        "grass" => "Grass",
        "plains" => "Plains",
        "desert" => "Desert",
        "tundra" => "Tundra",
        "hills" => "Hills",
        "forest" => "Forest",
        "jungle" => "Jungle",
        other => panic!("unknown start region {other}"),
    }
}

fn start_regions(bias: &Value) -> Option<String> {
    let prefer: Vec<&str> = strings(bias, "region_priority")
        .iter()
        .map(|r| start_region(r))
        .collect();
    let avoid: Vec<&str> = strings(bias, "region_avoid")
        .iter()
        .map(|r| start_region(r))
        .collect();
    let along_ocean = flag(bias, "placed_first_along_ocean");
    if prefer.is_empty() && avoid.is_empty() && !along_ocean {
        return None;
    }
    let mut parts = Vec::new();
    if !prefer.is_empty() {
        parts.push(format!("prefer: [{}]", prefer.join(", ")));
    }
    if !avoid.is_empty() {
        parts.push(format!("avoid: [{}]", avoid.join(", ")));
    }
    if along_ocean {
        parts.push("along_ocean: true".into());
    }
    Some(format!("({})", parts.join(", ")))
}

/// `[(Defense, "core:unit/pathfinder")]` from `{"defense": "core:unit/pathfinder"}`: the nation's own
/// unit for a starting slot. The slots come out in the order of `StartingUnit`.
fn start_units(v: &Value) -> Vec<String> {
    let mut out = Vec::new();
    let Some(map) = v.as_object() else {
        return out;
    };
    for (slot, role) in [("defense", "Defense"), ("worker", "Worker"), ("explore", "Explore")] {
        if let Some(unit) = map.get(slot) {
            let unit = unit.as_str().unwrap_or_else(|| panic!("start unit {slot}: {unit} is not a key"));
            out.push(format!("({role}, {})", quote(unit)));
        }
    }
    if let Some(unknown) = map.keys().find(|k| !["defense", "worker", "explore"].contains(&k.as_str())) {
        panic!("unknown start unit slot {unknown:?} (expected defense|worker|explore)");
    }
    out
}

pub fn nations(cat: &Catalogs, legacy: &[(String, (u8, u8, u8))], curated: &CuratedNames) -> Vec<Record> {
    items(&cat.nations, "items")
        .into_iter()
        .map(|c| {
            let mut fx = Effects::default();
            let mut ability_names = Vec::new();
            for trait_ in c["ability"].as_array().map(|a| a.as_slice()).unwrap_or(&[]) {
                ability_names.push(text(trait_, "name").to_string());
                let effects = trait_["effects"].as_array().map(|a| a.as_slice()).unwrap_or(&[]);
                for e in &crate::import::society::merge_trait_effects(effects) {
                    fx.push(e, Owner::Nation);
                }
            }
            let (r_, g, b) = nation_color(id(c), legacy);
            let bias = &c["start_bias"];
            let coastal = flag(bias, "coastal");
            let city_names = curated.cities.get(id(c)).cloned().unwrap_or_default();
            let spy_names = curated.spies.get(id(c)).cloned().unwrap_or_default();
            let mut r = Record::new()
                .str("id", id(c))
                .lines("city_names", city_names)
                .lines("spy_names", spy_names)
                .str("name", text(c, "name"))
                .str("adjective", text(c, "adjective"))
                .str("leader", text(&c["leader"], "name"))
                .raw("color", format!("(r: {r_}, g: {g}, b: {b})"))
                .str("start_bias", if coastal { "coastal" } else { "none" });
            if let Some(regions) = start_regions(bias) {
                r = r.raw("start_regions", regions);
            }
            let r = r
                .text("ability_name", &ability_names.join(" / "))
                .strs("unique_units", &strings(c, "unique_units"))
                .strs("unique_buildings", &strings(c, "unique_buildings"))
                .strs("unique_improvements", &strings(c, "unique_improvements"))
                .strs("free_techs", &strings(c, "free_techs"))
                .lines("free_units", key_counts(&c["free_units"], "unit", "count"))
                .lines("start_units", start_units(&c["start_units"]))
                .strs("preferred_religions", &strings(c, "preferred_religion"));
            fx.write(r, true)
        })
        .collect()
}

pub fn city_state_types(cat: &Catalogs) -> Vec<Record> {
    items(&cat.city_states, "types")
        .into_iter()
        .map(|t| {
            let mut fx = Effects::default();
            let mut bonuses = Vec::new();
            for level in ["friends", "allies"] {
                city_state_bonuses(&t[level], level, "", &mut bonuses, &mut fx);
            }
            let thresholds = &t["thresholds"];
            let mut r = Record::new()
                .str("id", id(t))
                .str("name", text(t, "name"))
                .raw("friends_threshold", int(thresholds, "friends").to_string())
                .raw("allies_threshold", int(thresholds, "allies").to_string());
            if let Some(name) = minor_trait(id(t)) {
                r = r.raw("minor_trait", format!("Some({name})"));
            }
            fx.write(r.lines("bonuses", bonuses), false)
        })
        .collect()
}

/// The city-state type id → the RON trait variant.
fn minor_trait(type_id: &str) -> Option<&'static str> {
    match type_id {
        "core:city-state-type/cultured" => Some("Cultured"),
        "core:city-state-type/militaristic" => Some("Militaristic"),
        "core:city-state-type/maritime" => Some("Maritime"),
        "core:city-state-type/mercantile" => Some("Mercantile"),
        "core:city-state-type/religious" => Some("Religious"),
        _ => None,
    }
}

/// Friend/ally bonus tables: typed bonuses where the mechanic knows the
/// number, one inert entry per other number.
fn city_state_bonuses(v: &Value, level: &str, prefix: &str, out: &mut Vec<String>, fx: &mut Effects) {
    let Some(map) = v.as_object() else {
        return;
    };
    let mut names: Vec<&String> = map.keys().collect();
    names.sort();
    for name in names {
        let value = &map[name.as_str()];
        let path = if prefix.is_empty() { name.clone() } else { format!("{prefix}.{name}") };
        match value {
            Value::Object(_) => city_state_bonuses(value, level, &path, out, fx),
            Value::Number(n) => match typed_city_state_bonus(level, &path, n.as_i64().unwrap_or(0)) {
                Some(line) => out.push(line),
                None => {
                    let full = format!("city_state.{level}.{path}");
                    fx.inert(field_inert("city_state_bonus", &full, "player", value));
                }
            },
            Value::Bool(_) => {
                let full = format!("city_state.{level}.{path}");
                fx.inert(field_inert("city_state_bonus", &full, "player", value));
            }
            _ => {}
        }
    }
}

/// One catalog bonus number as a typed RON bonus (None: not modelled).
fn typed_city_state_bonus(level: &str, path: &str, amount: i64) -> Option<String> {
    let (name, era) = match path.split_once('.') {
        Some((name, era)) => (name, Some(era)),
        None => (path, None),
    };
    let (kind, era) = match (name, era) {
        ("capital_food_times100_pre_renaissance", None) => ("CapitalFood", None),
        ("capital_food_times100_post_renaissance", None) => ("CapitalFood", Some("renaissance")),
        ("capital_food_times100_additional", None) => ("CapitalFood", None),
        ("other_cities_food_times100", None) => ("AllCitiesFood", None),
        ("culture_per_turn_by_era" | "culture_per_turn_by_era_additional", Some(e)) => ("Culture", Some(e)),
        ("faith_per_turn_by_era" | "faith_per_turn_by_era_additional", Some(e)) => ("Faith", Some(e)),
        ("happiness_by_era" | "happiness_by_era_additional", Some(e)) => ("Happiness", Some(e)),
        ("unit_gift_every_turns_base" | "unit_gift_turns_change", None) => ("UnitGiftTurns", None),
        ("unit_gift_every_turns_random", None) => ("UnitGiftRandom", None),
        _ => return None,
    };
    let level = if level == "allies" { "Allies" } else { "Friends" };
    let era = match era {
        Some(e) => format!("Some(\"core:era/{e}\")"),
        None => "None".to_string(),
    };
    Some(format!("(level: {level}, kind: {kind}, era: {era}, amount: {amount})"))
}

pub fn city_states(cat: &Catalogs) -> Vec<Record> {
    items(&cat.city_states, "items")
        .into_iter()
        .map(|c| {
            Record::new()
                .str("id", id(c))
                .str("name", text(c, "name"))
                .str("city_state_type", text(c, "type"))
        })
        .collect()
}

pub fn specialists(cat: &Catalogs) -> Vec<Record> {
    items(&cat.great_people, "specialists")
        .into_iter()
        .map(|s| {
            let mut ys = s["yields"].clone();
            if let Some(culture) = s["culture_per_turn"].as_i64() {
                ys["culture"] = Value::from(culture);
            }
            let name = text(s, "name").split('|').next().unwrap_or("").to_string();
            let mut r = Record::new().str("id", id(s)).str("name", &name);
            if let Some(y) = yields(&ys) {
                r = r.raw("yields", y);
            }
            let r = r.num(
                "great_person_points",
                int(s, "great_person_points_per_turn"),
            );
            opt_key(r, "great_person", s, "great_person")
        })
        .collect()
}

pub fn great_people(cat: &Catalogs) -> Vec<Record> {
    items(&cat.great_people, "items")
        .into_iter()
        .map(|g| {
            let unit = id(g);
            let kind = match slug(unit) {
                "great-writer" => "Writer",
                "great-artist" => "Artist",
                "great-musician" => "Musician",
                "great-scientist" => "Scientist",
                "great-engineer" => "Engineer",
                "great-merchant" => "Merchant",
                "great-general" => "General",
                "great-admiral" => "Admiral",
                "great-prophet" => "Prophet",
                other => panic!("unknown great person {other}"),
            };
            let key = format!(
                "core:great-person/{}",
                slug(unit).trim_start_matches("great-")
            );
            let fx = Effects::default();
            let source = match g["points_source"].as_str() {
                None | Some("specialists") => None,
                Some("combat_experience") => Some("CombatExperience"),
                Some("faith") => Some("Faith"),
                Some(other) => panic!("unknown great person points source {other}"),
            };
            let mut r = Record::new()
                .str("id", &key)
                .raw("kind", kind)
                .str("unit", unit);
            if let Some(source) = source {
                r = r.raw("points_source", source);
            }
            let r = opt_key(r, "specialist", g, "specialist")
                .strs("improvements", &strings(g, "tile_improvement"));
            fx.write(r, false)
        })
        .collect()
}

/// The name lists of each nation the catalogs do not have: this project's own, kept
/// through every regeneration (`curated_names`).
#[derive(Debug, Default)]
pub struct CuratedNames {
    pub cities: std::collections::BTreeMap<String, Vec<String>>,
    pub spies: std::collections::BTreeMap<String, Vec<String>>,
}

/// The city and spy names of each nation in a written `nations.ron`.
pub fn curated_names(ron: &str) -> CuratedNames {
    CuratedNames { cities: name_lists(ron, "city_names"), spies: name_lists(ron, "spy_names") }
}

/// The lines of the `field` list of each nation in a written `nations.ron`
/// (without the trailing comma).
fn name_lists(ron: &str, field: &str) -> std::collections::BTreeMap<String, Vec<String>> {
    let opening = format!("{field}: [");
    let mut names = std::collections::BTreeMap::new();
    let mut nation: Option<String> = None;
    let mut list: Option<Vec<String>> = None;
    for line in ron.lines() {
        let line = line.trim();
        if let Some(items) = list.as_mut() {
            if line == "]," {
                if let (Some(nation), Some(items)) = (nation.clone(), list.take()) {
                    names.insert(nation, items);
                }
            } else {
                items.push(line.trim_end_matches(',').to_string());
            }
            continue;
        }
        if let Some(id) = line.strip_prefix("id: \"").and_then(|l| l.strip_suffix("\",")) {
            nation = Some(id.to_string());
        } else if line == opening {
            list = Some(Vec::new());
        }
    }
    names
}

#[cfg(test)]
mod curated_tests {
    use super::curated_names;

    #[test]
    fn city_and_spy_names_are_read_back_per_nation() {
        let written = r#"(
    nations: [
        (
            id: "core:nation/rome",
            city_names: [
                "Rome", "Antium",
                "Cumae",
            ],
            spy_names: [
                "Livia", "Sejanus",
            ],
            name: "Rome",
        ),
        (
            id: "core:nation/zulu",
            city_names: [
                "Ulundi",
            ],
            name: "Zulu",
        ),
    ],
)"#;
        let curated = curated_names(written);
        assert_eq!(curated.cities["core:nation/rome"], [r#""Rome", "Antium""#, r#""Cumae""#]);
        assert_eq!(curated.spies["core:nation/rome"], [r#""Livia", "Sejanus""#]);
        assert_eq!(curated.cities["core:nation/zulu"], [r#""Ulundi""#]);
        assert!(!curated.spies.contains_key("core:nation/zulu"));
    }
}
