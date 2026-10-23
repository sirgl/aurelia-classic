//! Catalog effects of the society systems (classic/docs/classic/rules.md):
//! instant grants of wonders, policies and traits (`Grant`), society stats
//! (`AddStat(stat: Society(..))`), the Venetian bans (`Forbid`) and the
//! civilization traits with their own trigger (Assyria, Poland, the Maya,
//! Babylon and Venice, Carthage). None: not one of these effects.
//!
//! ```text
//! grant.tech / grant.great_person_choice  Grant(FreeTechnologies / FreeGreatPeople)
//! grant.unit_per_military_unit            Grant(MilitaryUnitCopies) in the wonder's city
//! golden_age.start / .start_turns         Grant(GoldenAge(turns)) (start: UNMODIFIED_GOLDEN_AGE_TURNS)
//! city.population.flat                    Grant(Population) in every city (on complete)
//!                                         or the founded city (on founding)
//! policy.free_building_on_conquest        FreeBuilding in the conquered city
//! happiness.per_city_connection           AddHappinessPerCount(ConnectedCities)
//! tourism.pct + golden_age                ModifyTourism(Percent) of every city
//! trait.free_unit (+ free_unit_prereq_tech, merged by `merge_trait_effects`)
//!                                         FreeUnit in the capital when the tech is learned
//! trait.free_building                     FreeBuilding in founded coastal cities
//! forbid.train_unit_class                 Forbid(UnitClassOf)
//! catalog mechanics                       see `mechanic`
//! ```

use serde_json::Value;

use crate::import::effects::Owner;
use crate::import::ron_out::quote;

/// Length of the golden age granted by a building (Taj Mahal), in turns at
/// the standard speed. Unciv: GoldenAgeManager.enterGoldenAge default of 10 turns at
/// speed modifier 1.0 (Speeds.json); equal.
const UNMODIFIED_GOLDEN_AGE_TURNS: i64 = 10;

/// Map a society effect (see the module docs).
pub fn society_effect(e: &Value, owner: Owner) -> Option<Result<String, String>> {
    if owner == Owner::Other {
        return None;
    }
    if let Some(m) = e["needs_mechanic"].as_str() {
        return mechanic(e, m);
    }
    let name = e["modifier"].as_str().unwrap_or("");
    let trigger = e["trigger"].as_str().unwrap_or("");
    match name {
        "grant.tech" => Some(grant(trigger, owner, int(e).map(|n| format!("FreeTechnologies({n})")))),
        "grant.great_person_choice" => {
            Some(grant(trigger, owner, int(e).map(|n| format!("FreeGreatPeople({n})"))))
        }
        "golden_age.start" => Some(grant(trigger, owner, Ok(format!("GoldenAge(turns: {UNMODIFIED_GOLDEN_AGE_TURNS})")))),
        "golden_age.start_turns" => Some(grant(trigger, owner, int(e).map(|n| format!("GoldenAge(turns: {n})")))),
        "grant.unit_per_military_unit" if owner == Owner::Building => Some(Ok(
            "(scope: City, selector: SourceCity, trigger: OnBuilt, modifier: Grant(MilitaryUnitCopies), duration: Once)"
                .into(),
        )),
        "city.population.flat" => Some(population(e, trigger)),
        "policy.free_building_on_conquest" => Some(target(e).map(|b| {
            format!(
                "(scope: City, selector: EventCity, trigger: OnCityConquered, modifier: FreeBuilding({}), duration: Once)",
                quote(&b)
            )
        })),
        "happiness.per_city_connection" => Some(Ok(format!(
            "(scope: Player, selector: Owner, modifier: AddHappinessPerCount(amount: {}, per: (count: ConnectedCities)))",
            e["amount"].as_f64().unwrap_or(1.0).round() as i64
        ))),
        "wonder.culture.pct" => Some(int(e).map(|n| city_stat("WonderCulturePercent", n))),
        "tourism.pct" if e["condition"]["golden_age"] == Value::Bool(true) => Some(int(e).map(|n| {
            format!("(scope: City, selector: OwnerCities, condition: GoldenAge, modifier: ModifyTourism(stat: Percent, amount: {n}))")
        })),
        "trait.free_unit" => free_unit(e),
        "trait.free_building" => Some(target(e).map(|b| {
            format!(
                "(scope: City, selector: EventCity, condition: CityCoastal, trigger: OnCityFounded, modifier: FreeBuilding({}), duration: Once)",
                quote(&b)
            )
        })),
        "forbid.train_unit_class" => Some(match e["selector"]["filter"]["unit"].as_str() {
            Some(unit) => Ok(format!("(scope: Player, selector: Owner, modifier: Forbid(UnitClassOf({})))", quote(unit))),
            None => Err("modifier:forbid.train_unit_class".into()),
        }),
        _ => None,
    }
}

/// Catalog `needs_mechanic` entries of the society systems.
fn mechanic(e: &Value, mechanic: &str) -> Option<Result<String, String>> {
    let amount = || match &e["amount"] {
        Value::Bool(true) => Ok(1),
        _ => int(e),
    };
    let player = |stat: &str| Some(amount().map(|n| player_stat(stat, n)));
    match mechanic {
        "happiness_to_culture" => player("HappinessToCulturePercent"),
        "happiness_to_science" => player("ScienceWhileHappyPercent"),
        "luxury_happiness_retention" => player("LuxuryRetentionPercent"),
        "raze_speed_modifier" => player("RazeSpeedPercent"),
        "extra_founded_city_territory_claim_range" => player("FoundingTerritoryTiles"),
        "tech_boost_from_capital_science_buildings" => player("CapitalScienceBuildingBoost"),
        "unique_luxury_cities" => player("UniqueLuxuryCities"),
        "unique_luxury_quantity" => player("UniqueLuxuryQuantity"),
        "unique_luxury_requires_new_area" => player("UniqueLuxuryNewLandmass"),
        // Halves: a specialist eats and grieves 50 % less.
        "half_specialist_food" => Some(Ok(city_stat("SpecialistFoodPercent", -50))),
        "half_specialist_unhappiness" => Some(Ok(city_stat("SpecialistUnhappinessPercent", -50))),
        "no_occupied_unhappiness" => Some(Ok(
            "(scope: City, selector: SourceCity, modifier: AddStat(stat: Society(NoOccupiedUnhappiness), amount: 1))".into(),
        )),
        "no_annexing" => Some(Ok("(scope: Player, selector: Owner, modifier: Forbid(Annexing))".into())),
        "tech_from_city_conquer" => Some(Ok(
            "(scope: Player, selector: Owner, trigger: OnCityConquered, modifier: Grant(ConquestTechnology), duration: Once)"
                .into(),
        )),
        "free_social_policies_per_era" => Some(amount().map(|n| {
            format!("(scope: Player, selector: Owner, trigger: OnEraEntered, modifier: FreePolicies(count: {n}), duration: Once)")
        })),
        // The Long Count gives its great people once Theology is known.
        "maya_calendar_bonuses" => Some(Ok(
            "(scope: Player, selector: Owner, condition: HasTechnology(\"core:tech/theology\"), trigger: OnLongCountCycle, modifier: Grant(LongCountGreatPerson), duration: Once)"
                .into(),
        )),
        _ => None,
    }
}

/// A Player-scope instant grant on the catalog trigger (completion or
/// adoption).
fn grant(trigger: &str, owner: Owner, grant: Result<String, String>) -> Result<String, String> {
    let grant = grant?;
    let trigger = match (trigger, owner) {
        ("on_complete", Owner::Building) => "OnBuilt",
        ("on_adopt", Owner::Policy) => "OnAdopt",
        (other, _) => return Err(format!("trigger:{other}")),
    };
    Ok(format!(
        "(scope: Player, selector: Owner, trigger: {trigger}, modifier: Grant({grant}), duration: Once)"
    ))
}

/// Citizens in every city when the wonder is built, or in the founded city.
fn population(e: &Value, trigger: &str) -> Result<String, String> {
    let n = int(e)?;
    let (selector, trigger) = match trigger {
        "on_complete" => ("OwnerCities", "OnBuilt"),
        "on_city_founded" => ("EventCity", "OnCityFounded"),
        other => return Err(format!("trigger:{other}")),
    };
    Ok(format!(
        "(scope: City, selector: {selector}, trigger: {trigger}, modifier: Grant(Population({n})), duration: Once)"
    ))
}

/// A free unit in the capital when the trait's technology is learned
/// (Babylon, Venice); None without the merged technology.
fn free_unit(e: &Value) -> Option<Result<String, String>> {
    let tech = e["prereq_tech"].as_str()?;
    Some(target(e).and_then(|unit| {
        let count = int(e)?;
        Ok(format!(
            "(scope: City, selector: Capital, condition: LearnedTechnology({}), trigger: OnTechnologyUnlocked, modifier: FreeUnit(unit: {}, count: {count}), duration: Once)",
            quote(tech),
            quote(&unit)
        ))
    }))
}

/// A trait's `free_unit` and `free_unit_prereq_tech` records name one
/// mechanic (a trait's free unit and the technology it needs): the first
/// gets the technology (`prereq_tech`), the second is dropped. Other
/// records pass unchanged.
pub fn merge_trait_effects(effects: &[Value]) -> Vec<Value> {
    let prereq = effects
        .iter()
        .find(|e| e["modifier"].as_str() == Some("trait.free_unit_prereq_tech"))
        .and_then(|e| e["target"].as_str());
    let mut out = Vec::with_capacity(effects.len());
    for e in effects {
        match (e["modifier"].as_str(), prereq) {
            (Some("trait.free_unit_prereq_tech"), Some(_)) => {}
            (Some("trait.free_unit"), Some(tech)) => {
                let mut merged = e.clone();
                merged["prereq_tech"] = Value::String(tech.to_string());
                out.push(merged);
            }
            _ => out.push(e.clone()),
        }
    }
    out
}

fn player_stat(stat: &str, amount: i64) -> String {
    format!("(scope: Player, selector: Owner, modifier: AddStat(stat: Society({stat}), amount: {amount}))")
}

fn city_stat(stat: &str, amount: i64) -> String {
    format!("(scope: City, selector: OwnerCities, modifier: AddStat(stat: Society({stat}), amount: {amount}))")
}

fn int(e: &Value) -> Result<i64, String> {
    match e["amount"].as_f64() {
        Some(f) if f.fract() == 0.0 => Ok(f as i64),
        _ => Err(format!("amount:{}", e["modifier"].as_str().unwrap_or(""))),
    }
}

fn target(e: &Value) -> Result<String, String> {
    match e["target"].as_str() {
        Some(t) => Ok(t.to_string()),
        None => Err(format!("modifier:{}", e["modifier"].as_str().unwrap_or(""))),
    }
}
