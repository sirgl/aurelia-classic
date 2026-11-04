//! Catalog effects of production, purchase and maintenance → typed RON
//! effects (classic/docs/classic/rules.md):
//!
//! ```text
//! production.<items>.pct     AddYieldPercent(Production) gated by
//!                            Producing(filter): the selector's filter names
//!                            the items, not the cities
//! trait.capital_building_modifier   Producing(BuiltInCapital) (Rome)
//! worker.speed.pct, route.maintenance.pct   player stats
//! policy.* / trait.* rule flags     AddStat(Rule(..)) (Oligarchy, Inca,
//!                                   Space Procurements, Spaceflight Pioneers)
//! ```
//!
//! Returns None when the effect is not one of them, Some(Err(mechanic))
//! when it is but does not fit (it stays inert).

use serde_json::Value;

use crate::import::effects::Owner;
use crate::import::military::{domain_variant, role_variant};
use crate::import::ron_out::quote;

/// The typed RON effect of a production, worker, route or rule effect.
pub fn production_effect(e: &Value, owner: Owner) -> Option<Result<String, String>> {
    if owner == Owner::Other {
        return None;
    }
    let modifier = e["modifier"].as_str().unwrap_or("");
    let items = modifier.strip_prefix("production.").and_then(|m| m.strip_suffix(".pct"));
    let ours = items.is_some()
        || player_rule(modifier).is_some()
        || matches!(
            modifier,
            "worker.speed.pct" | "route.maintenance.pct" | "trait.capital_building_modifier"
        );
    if !ours {
        return None;
    }
    let trigger = e["trigger"].as_str().unwrap_or("");
    if trigger != "passive" {
        return Some(Err(format!("trigger:{trigger}")));
    }
    if let Some(items) = items {
        return Some(item_percent(e, owner, items));
    }
    Some(player_effect(e, modifier))
}

/// Player stats and rules, and Rome's percent toward buildings of the capital.
fn player_effect(e: &Value, modifier: &str) -> Result<String, String> {
    if let Some(rule) = player_rule(modifier) {
        return Ok(player_stat(&format!("Rule({rule})"), 1));
    }
    match modifier {
        "worker.speed.pct" => percent(e).map(|n| player_stat("WorkerSpeedPercent", n)),
        "route.maintenance.pct" => percent(e).map(|n| player_stat("RouteMaintenancePercent", n)),
        _ => {
            let filter = "BuiltInCapital".to_string();
            percent(e).map(|n| production_percent("OwnerCities", filter, n))
        }
    }
}

/// Catalog flags that are exceptions to a rule (`PlayerRule`).
fn player_rule(modifier: &str) -> Option<&'static str> {
    Some(match modifier {
        "policy.garrison_free_maintenance" => "GarrisonFreeMaintenance",
        "trait.no_hills_improvement_maintenance" => "FreeHillsRouteMaintenance",
        "policy.enables_ss_part_purchase" => "SpaceshipPartPurchase",
        "policy.enables_ss_part_hurry" => "SpaceshipPartHurry",
        _ => return None,
    })
}

fn player_stat(stat: &str, amount: i64) -> String {
    format!("(scope: Player, selector: Owner, modifier: AddStat(stat: {stat}, amount: {amount}))")
}

fn percent(e: &Value) -> Result<i64, String> {
    let modifier = e["modifier"].as_str().unwrap_or("");
    match e["amount"].as_f64() {
        Some(a) if a.fract() == 0.0 => Ok(a as i64),
        _ => Err(format!("amount:{modifier}")),
    }
}

fn production_percent(selector: &str, filter: String, percent: i64) -> String {
    format!(
        "(scope: City, selector: {selector}, condition: Producing({filter}), modifier: AddYieldPercent(yield_kind: Production, percent: {percent}))"
    )
}

/// `production.<items>.pct`: the cities the selector names, the items its
/// filter (and, for wonders, the `before_era` condition) names.
fn item_percent(e: &Value, owner: Owner, items: &str) -> Result<String, String> {
    let modifier = e["modifier"].as_str().unwrap_or("");
    let kind = e["selector"]["kind"].as_str().unwrap_or("");
    let selector = match (kind, owner) {
        ("this_city", Owner::Building) => "SourceCity",
        ("all_cities", _) => "OwnerCities",
        ("capital", _) => "Capital",
        ("following_cities", Owner::Belief) => "FollowingCities",
        _ => return Err(format!("selector:{kind}")),
    };
    let filter = &e["selector"]["filter"];
    let filter = match items {
        "units" => unit_filter(filter)?,
        "military_units" if no_filter(filter) => "MilitaryUnits".to_string(),
        "spaceship" if no_filter(filter) => "SpaceshipParts".to_string(),
        "buildings" if no_filter(filter) => "Buildings".to_string(),
        "wonders" if no_filter(filter) => wonder_filter(&e["condition"])?,
        "building_class" => match filter["building"].as_str() {
            Some(building) => format!("BuildingClassOf({})", quote(building)),
            None => return Err(format!("modifier:{modifier}")),
        },
        _ => return Err(format!("modifier:{modifier}")),
    };
    if items != "wonders" && !e["condition"].is_null() {
        return Err("condition:production".to_string());
    }
    Ok(production_percent(selector, filter, percent(e)?))
}

/// The unit filter of `production.units.pct` (one key).
fn unit_filter(filter: &Value) -> Result<String, String> {
    let Some(map) = filter.as_object() else {
        return Ok("Units".to_string());
    };
    if map.is_empty() {
        return Ok("Units".to_string());
    }
    if map.len() != 1 {
        return Err("selector:production_filter".to_string());
    }
    if let Some(domain) = map.get("domain").and_then(|d| d.as_str()) {
        return Ok(format!("UnitDomain({})", domain_variant(domain)?));
    }
    if let Some(role) = map.get("unit_combat").and_then(|r| r.as_str()) {
        return Ok(format!("UnitRole({})", role_variant(role)?));
    }
    if let Some(unit) = map.get("unit").and_then(|u| u.as_str()) {
        return Ok(format!("UnitClassOf({})", quote(unit)));
    }
    Err("selector:production_filter".to_string())
}

/// World wonders, of an era before the condition's `before_era` when set
/// (Monument to the Gods).
fn wonder_filter(condition: &Value) -> Result<String, String> {
    let Some(map) = condition.as_object() else {
        return Ok("WorldWonders(before_era: None)".to_string());
    };
    match (map.len(), map.get("before_era").and_then(|e| e.as_str())) {
        (1, Some(era)) => Ok(format!("WorldWonders(before_era: Some({}))", quote(era))),
        _ => Err("condition:production".to_string()),
    }
}

fn no_filter(filter: &Value) -> bool {
    match filter.as_object() {
        Some(map) => map.is_empty(),
        None => true,
    }
}
