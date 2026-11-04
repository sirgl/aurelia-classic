//! Catalog effect (classic/docs/classic effect record) → typed RON effect or inert
//! effect. Typed only when the whole record fits the M3.0 effect system
//! (platform/structure.md §5.1, the vocabulary-to-schema table): known modifier family,
//! selector, trigger and at most one condition the compiler accepts for
//! this source kind. Everything else becomes an inert effect whose
//! `needs_mechanic` names the first missing piece, in this order: the
//! catalog's own `needs_mechanic`, the modifier, the trigger, the selector,
//! the condition.

use serde_json::Value;

use crate::import::ron_out::{quote, Record};

/// Kind of content that owns the effect (what the compiler allows differs).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Owner {
    Building,
    Technology,
    Nation,
    Promotion,
    Policy,
    Belief,
    /// Improvements, resources, projects, natural wonders, ...: no typed
    /// effects in the compiled ruleset.
    Other,
}

pub enum Mapped {
    Typed(String),
    Inert(Record),
    /// Read from another field of the entity: neither typed nor inert.
    Covered,
}

/// Map one catalog effect.
pub fn map_effect(e: &Value, owner: Owner) -> Mapped {
    if covered_by_unit_row(e) {
        return Mapped::Covered;
    }
    match typed(e, owner) {
        Ok(text) => Mapped::Typed(text),
        Err(missing) => Mapped::Inert(inert_record(e, &missing)),
    }
}

/// A policy's unlock of a unit that the unit's own row already gates
/// (the unit's policy-requirement column: the Landsknecht needs Mercenary Army, read from the
/// unit's `required_policy`): not an effect of the policy. Also the two
/// parameter rows of Polynesia's Moai bonus, which the row with the percent
/// reads (`military::MOAI_BONUS_PARAMETERS`).
fn covered_by_unit_row(e: &Value) -> bool {
    (e["modifier"].as_str() == Some("unlock.unit") && e["tag"].as_str() == Some("unit_policy_gate"))
        || e["tag"].as_str().is_some_and(|s| crate::import::military::MOAI_BONUS_PARAMETERS.contains(&s))
}

/// The inert record of a catalog effect with the given missing mechanic.
pub fn inert_record(e: &Value, missing: &str) -> Record {
    let catalog = e["needs_mechanic"].as_str().unwrap_or("");
    let mechanic = if catalog.is_empty() { missing } else { catalog };
    let mut refs = Vec::new();
    collect_refs(&e["target"], &mut refs);
    collect_refs(&e["selector"]["filter"], &mut refs);
    collect_refs(&e["condition"], &mut refs);
    refs.sort();
    refs.dedup();
    let selector = e["selector"]["kind"].as_str().unwrap_or("");
    inert(
        mechanic,
        e["modifier"].as_str().unwrap_or(""),
        e["scope"].as_str().unwrap_or(""),
        selector,
        e["trigger"].as_str().unwrap_or(""),
        &e["amount"],
        &refs,
    )
}

/// An inert effect record.
pub fn inert(
    mechanic: &str,
    modifier: &str,
    scope: &str,
    selector: &str,
    trigger: &str,
    amount: &Value,
    refs: &[String],
) -> Record {
    Record::new()
        .str("needs_mechanic", mechanic)
        .str("modifier", modifier)
        .str("scope", scope)
        .str("selector", selector)
        .str("trigger", trigger)
        .raw("amount", inert_amount(amount))
        .strs("refs", refs)
}

/// `None`, `Flag`, `Int(n)` or `Hundredths(n)`.
pub fn inert_amount(amount: &Value) -> String {
    match amount {
        Value::Bool(true) => "Flag".into(),
        Value::Number(n) => {
            let f = n.as_f64().unwrap_or(0.0);
            if f.fract() == 0.0 {
                format!("Int({})", f as i64)
            } else {
                format!("Hundredths({})", (f * 100.0).round() as i64)
            }
        }
        _ => "None".into(),
    }
}

/// Every `core:` key inside a JSON value (strings, lists, maps).
pub fn collect_refs(v: &Value, out: &mut Vec<String>) {
    match v {
        Value::String(s) if s.starts_with("core:") => out.push(s.clone()),
        Value::Array(items) => {
            for item in items {
                collect_refs(item, out);
            }
        }
        Value::Object(map) => {
            for item in map.values() {
                collect_refs(item, out);
            }
        }
        _ => {}
    }
}

// ---- typed mapping ----

/// What a selector maps to: scope, selector and the condition it implies.
struct Target {
    scope: &'static str,
    selector: &'static str,
    condition: Option<String>,
}

fn typed(e: &Value, owner: Owner) -> Result<String, String> {
    if let Some(military) = crate::import::military::military_effect(e, owner) {
        return military;
    }
    if let Some(minor) = crate::import::city_states::city_state_effect(e, owner) {
        return minor;
    }
    if let Some(production) = crate::import::production::production_effect(e, owner) {
        return production;
    }
    if let Some(society) = crate::import::society::society_effect(e, owner) {
        return society;
    }
    if let Some(espionage) = crate::import::espionage::espionage_effect(e, owner) {
        return espionage;
    }
    let modifier_name = e["modifier"].as_str().unwrap_or("");
    if let Some(m) = e["needs_mechanic"].as_str() {
        if let Some(typed) = mechanic_effect(e, owner, m) {
            return Ok(typed);
        }
        return Err(m.to_string());
    }
    if owner == Owner::Other {
        return Err(format!("source_effects:{modifier_name}"));
    }
    if let Some(typed) = tourism_effect(e, owner, modifier_name) {
        return typed;
    }
    if let Some(typed) = new_cities_grant(e, owner) {
        return Ok(typed);
    }
    if let Some(typed) = natural_wonder_find(e) {
        return typed;
    }
    if let Some(typed) = great_person_tourism(e, owner) {
        return typed;
    }
    if let Some(typed) = trade_effect(e, owner, modifier_name) {
        return typed;
    }
    let trigger = trigger(e["trigger"].as_str().unwrap_or(""))?;
    if modifier_name == "tile.yield.any.pct" {
        return every_yield_percent(e, owner, trigger);
    }
    let (modifier, instant) = modifier(e, modifier_name)?;
    let instant = instant || one_shot_grant(trigger, &modifier);
    if let Some(tech) = e["condition"]["doubles_with_tech"].as_str() {
        // Religious Texts: the percent once more after the technology
        // (two effects; the second gated by the founder knowing it).
        let target = target(e, owner, trigger)?;
        let base = format!("(scope: {}, selector: {}, modifier: {modifier})", target.scope, target.selector);
        let doubled = format!(
            "(scope: {}, selector: {}, condition: HasTechnology({}), modifier: {modifier})",
            target.scope,
            target.selector,
            quote(tech)
        );
        return Ok(format!("{base}, {doubled}"));
    }
    check_trigger(owner, trigger, instant, modifier_name)?;
    let e = &without_parameter_filters(e, modifier_name);
    let target = target(e, owner, trigger)?;
    let implied = [target.condition, modifier_condition(modifier_name)];
    let conditions = conditions(&e["condition"], &implied, target.scope)?;
    // Golden-age unit traits (Persia) belong to the unit-trait work: kept inert.
    if target.scope == "Unit" && conditions.first().map(String::as_str) == Some("GoldenAge") {
        return Err("condition:golden_age".into());
    }
    check_scope(modifier_name, target.scope, &modifier)?;
    let mut parts = vec![
        format!("scope: {}", target.scope),
        format!("selector: {}", target.selector),
    ];
    if let Some(c) = conditions.first() {
        parts.push(format!("condition: {c}"));
    }
    if let Some(c) = conditions.get(1) {
        parts.push(format!("second_condition: {c}"));
    }
    if trigger != "Passive" {
        parts.push(format!("trigger: {trigger}"));
    }
    parts.push(format!("modifier: {modifier}"));
    if instant {
        parts.push("duration: Once".into());
    }
    Ok(format!("({})", parts.join(", ")))
}

/// A percent of every tile yield (Spain's natural wonder yield bonus):
/// one Tile-scope percent per yield, under the record's tile condition.
fn every_yield_percent(e: &Value, owner: Owner, trigger: &str) -> Result<String, String> {
    let name = "tile.yield.any.pct";
    let percent = int_amount(e, name)?;
    let target = target(e, owner, trigger)?;
    let conditions = conditions(&e["condition"], &[target.condition], target.scope)?;
    let condition = match conditions.first() {
        Some(c) => format!("condition: {c}, "),
        None => String::new(),
    };
    let mut effects = Vec::new();
    for y in ["Food", "Production", "Gold", "Science", "Culture", "Faith"] {
        let modifier = format!("AddYieldPercent(yield_kind: {y}, percent: {percent})");
        check_scope(name, target.scope, &modifier)?;
        effects.push(format!(
            "(scope: {}, selector: {}, {condition}modifier: {modifier})",
            target.scope, target.selector
        ));
    }
    Ok(effects.join(", "))
}

/// Catalog `needs_mechanic` entries whose mechanic the engine has, as
/// typed effects. Legalism and the Tradition finisher: the culture (food)
/// building in the first cities, given once on adoption; the catalog names
/// no building; the Monument (Aqueduct) is chosen here, unverified.
fn mechanic_effect(e: &Value, owner: Owner, mechanic: &str) -> Option<String> {
    if owner == Owner::Belief {
        return religion_mechanic(e, mechanic);
    }
    if owner == Owner::Nation && mechanic == "bonus_religious_belief" {
        return Some("(scope: Player, selector: Owner, modifier: ExtraFoundingBelief)".into());
    }
    // Sweden: great people per declaration of friendship.
    if owner == Owner::Nation && mechanic == "dof_great_person_modifier" {
        let percent = e["amount"].as_i64()?;
        return Some(format!(
            "(scope: Player, selector: Owner, modifier: AddStat(stat: GreatPeopleRatePerFriend, amount: {percent}))"
        ));
    }
    if owner == Owner::Nation && mechanic == "faith_from_unimproved_forest" {
        return Some(unimproved_forest_faith());
    }
    if owner != Owner::Policy {
        return None;
    }
    if mechanic == "add_reformation_belief" {
        return Some("(scope: Player, selector: Owner, modifier: AllowReformationBelief)".into());
    }
    if mechanic == "second_religion_pantheon" {
        return Some("(scope: Player, selector: Owner, modifier: SecondReligionPantheon)".into());
    }
    if mechanic == "always_see_barb_camps" {
        return Some("(scope: Player, selector: Owner, modifier: AddStat(stat: Rule(SeeBarbarianCamps), amount: 1))".into());
    }
    let building = match mechanic {
        "num_cities_free_culture_building" => "core:building/monument",
        "num_cities_free_food_building" => "core:building/aqueduct",
        _ => return None,
    };
    let count = e["amount"].as_i64()?;
    Some(format!(
        "(scope: Player, selector: Owner, trigger: OnAdopt, modifier: FreeBuildingInCities(building: {}, count: {count}), duration: Once)",
        quote(building)
    ))
}

/// Reformation beliefs the catalog marks `needs_mechanic`: numbers and
/// rules of the religion itself (rules.md). `spy_pressure`
/// (Underground Sect) is espionage's and stays inert.
fn religion_mechanic(e: &Value, mechanic: &str) -> Option<String> {
    let at = "scope: Religion, selector: OwnReligion";
    let percent = || e["amount"].as_i64();
    Some(match mechanic {
        "other_religion_pressure_erosion" => {
            format!("({at}, modifier: {})", religion("OtherReligionErosion", percent()?))
        }
        "inquisitor_pressure_retention" => {
            format!("({at}, modifier: {})", religion("PressureRetention", percent()?))
        }
        "converts_barbarians" => format!("({at}, modifier: GrantReligionAbility(ConvertsBarbarians))"),
        "faith_purchase_all_great_people" => {
            format!("({at}, modifier: GrantReligionAbility(FaithPurchaseAllGreatPeople))")
        }
        _ => return None,
    })
}

/// Celts (faith from unimproved forest, catalog text): +1 faith in a city
/// with an unimproved forest next to its centre, +2 with three or more.
/// The tiles adjacent to the city centre count whatever their owner
/// (unverified).
fn unimproved_forest_faith() -> String {
    let effect = |at_least: u8| {
        format!(
            "(scope: City, selector: OwnerCities, condition: CityAdjacentUnimprovedFeature(feature: {}, at_least: {at_least}), modifier: AddYield(yield_kind: Faith, amount: 1))",
            quote("core:feature/forest")
        )
    };
    format!("{}, {}", effect(1), effect(3))
}

/// Tourism effects (classic/docs/classic/rules.md):
/// the catalog puts building-local tourism numbers (Hotel, Airport,
/// National Visitor Center) on the player and the Internet's spread bonus on
/// research; here they stay on the city and while the technology is known.
/// None: not a tourism effect.
fn tourism_effect(e: &Value, owner: Owner, name: &str) -> Option<Result<String, String>> {
    let amount = |stat: &str| int_amount(e, name).map(|n| format!("ModifyTourism(stat: {stat}, amount: {n})"));
    let city_stat = match name {
        "tourism.great_works.pct" => Some("GreatWorksPercent"),
        "tourism.landmarks.pct_of_culture" => Some("LandmarksPercent"),
        _ => None,
    };
    if let (Some(stat), Owner::Building) = (city_stat, owner) {
        return Some(amount(stat).map(|m| format!("(scope: City, selector: SourceCity, modifier: {m})")));
    }
    let condition = e["condition"]["target"].as_str();
    match (name, condition) {
        ("tourism.pct", None) if owner == Owner::Technology => Some(
            amount("Spread").map(|m| format!("(scope: Player, selector: Owner, modifier: {m})")),
        ),
        ("tourism.pct", Some(target)) => {
            let when = match target {
                "shares_majority_religion" => "SharedReligion",
                "has_trade_route" => "TradeRoute",
                "open_borders" => "OpenBorders",
                "same_ideology" => "SameIdeology",
                "less_happy" => "LessHappy",
                "common_enemy" => "CommonEnemy",
                _ => return Some(Err(format!("condition:{target}"))),
            };
            Some(int_amount(e, name).map(|n| {
                format!("(scope: Player, selector: Owner, modifier: TourismToward(when: {when}, percent: {n}))")
            }))
        }
        _ => None,
    }
}

/// Trade route numbers (classic/docs/classic/rules.md) as
/// `AddStat(stat: Trade(..))`. The catalog marks a technology's route as
/// `on_research`; it counts while the technology is known, so it is
/// passive here. Economic Union's route gold holds only toward a
/// nation of the same ideology: the stat itself says so (the
/// condition is about the route's partner, not the owner). None: not a
/// typed trade effect.
fn trade_effect(e: &Value, owner: Owner, name: &str) -> Option<Result<String, String>> {
    let domain = || match e["selector"]["filter"]["domain"].as_str() {
        Some("core:domain/sea") => "Sea",
        _ => "Land",
    };
    if name == "trade.route.gold.flat" {
        return Some(shared_ideology_gold(e));
    }
    let (stat, player) = match name {
        "trade.routes.flat" => ("Routes".to_string(), true),
        "trade.range.pct" => (format!("RangeTiles({})", domain()), true),
        "trade.land_range.pct" => ("RangePercent(Land)".to_string(), false),
        "trade.sea_range.pct" => ("RangePercent(Sea)".to_string(), false),
        "trade.land_route.gold.flat" => ("RouteGold(Land)".to_string(), owner == Owner::Policy),
        "trade.sea_route.gold.flat" => ("RouteGold(Sea)".to_string(), owner == Owner::Policy),
        "trade.route_target.gold.flat" => ("TargetGold".to_string(), false),
        "trade.route_recipient.gold.flat" => ("RecipientGold".to_string(), false),
        "trade.internal_route.yield.pct" => ("InternalPercent".to_string(), true),
        // Arabia: tiles before the land range percent (rules 3.4).
        "trade.land_range.flat_tiles" => ("RangeTiles(Land)".to_string(), true),
        "trade.religious_pressure.pct" => ("ReligiousPressurePercent".to_string(), true),
        "trade.resource_diversity_gold.pct" => ("ResourceGoldPercent".to_string(), true),
        "trade.routes.pct" => ("RoutesPercent".to_string(), true),
        "trade.incoming_route.yield.gold.flat" => ("IncomingRouteGold".to_string(), true),
        "trade.city_state_route.gold.flat" => ("CityStateRouteGold".to_string(), true),
        _ => return None,
    };
    let amount = match e["amount"].as_f64() {
        Some(a) if a.fract() == 0.0 => a as i64,
        _ => return Some(Err(format!("amount:{name}"))),
    };
    let target = if player {
        "scope: Player, selector: Owner"
    } else {
        "scope: City, selector: SourceCity"
    };
    Some(Ok(format!("({target}, modifier: AddStat(stat: Trade({stat}), amount: {amount}))")))
}

/// Spain: gold when a natural wonder is found, more for the first finder
/// (the first-finder and subsequent-finder gold amounts of the Spanish trait).
/// None: not a find effect.
fn natural_wonder_find(e: &Value) -> Option<Result<String, String>> {
    if e["trigger"].as_str() != Some("on_discover_natural_wonder") {
        return None;
    }
    let name = e["modifier"].as_str().unwrap_or("");
    let trigger = match name {
        "gold.on_discover_natural_wonder_first" => "OnNaturalWonderFoundFirst",
        "gold.on_discover_natural_wonder" => "OnNaturalWonderFound",
        other => return Some(Err(format!("modifier:{other}"))),
    };
    Some(int_amount(e, name).map(|amount| {
        format!(
            "(scope: Player, selector: Owner, trigger: {trigger}, modifier: AddYield(yield_kind: Gold, amount: {amount}), duration: Once)"
        )
    }))
}

/// Economic Union (shared-ideology trade gold): +3 gold per route to a
/// nation of the same ideology. The catalog's 0.03 is the value
/// scaled down twice (3 gold is `3 × 100` hundredths, rules 3.6
/// "Policies"); the importer reads 3 gold.
fn shared_ideology_gold(e: &Value) -> Result<String, String> {
    if e["condition"]["target"].as_str() != Some("same_ideology") {
        return Err("condition:trade.route.gold.flat".into());
    }
    let amount = e["amount"].as_f64().unwrap_or(0.0);
    let gold = if amount > 0.0 && amount < 1.0 { (amount * 100.0).round() as i64 } else { amount as i64 };
    Ok(format!(
        "(scope: Player, selector: Owner, modifier: AddStat(stat: Trade(SharedIdeologyRouteGold), amount: {gold}))"
    ))
}

/// Futurism (tourism on unit creation): when a great person of the
/// filtered class appears, influence on every known nation. The
/// catalog names it a percent of recent tourism; the effect
/// `AddTourismAllKnownNations(250)` adds the column itself, unscaled (rules
/// "Culture" §1.5). None: not this effect.
fn great_person_tourism(e: &Value, owner: Owner) -> Option<Result<String, String>> {
    let fits = owner == Owner::Policy
        && e["trigger"].as_str() == Some("on_unit_created")
        && e["modifier"].as_str() == Some("tourism.on_unit_created.pct_of_recent");
    if !fits {
        return None;
    }
    let unit = e["selector"]["filter"]["unit"].as_str().unwrap_or("");
    let Some(kind) = great_person_kind(unit) else {
        return Some(Err(format!("selector:unit:{unit}")));
    };
    let amount = match int_amount(e, "tourism.on_unit_created.pct_of_recent") {
        Ok(a) => a,
        Err(missing) => return Some(Err(missing)),
    };
    Some(Ok(format!(
        "(scope: Player, selector: Owner, condition: GreatPersonIs({kind}), trigger: OnGreatPersonBorn, modifier: InstantTourism(amount: {amount}), duration: Once)"
    )))
}

/// CN Tower: the building in every city now and in every city founded later
/// (`trigger: on_complete_and_new_cities`): free buildings owed to all cities.
fn new_cities_grant(e: &Value, owner: Owner) -> Option<String> {
    let fits = owner == Owner::Building
        && e["trigger"].as_str() == Some("on_complete_and_new_cities")
        && e["modifier"].as_str() == Some("grant.building");
    if !fits {
        return None;
    }
    let building = e["target"].as_str()?;
    Some(format!(
        "(scope: Player, selector: Owner, trigger: OnBuilt, modifier: FreeBuildingInCities(building: {}, count: 255), duration: Once)",
        quote(building)
    ))
}

fn trigger(name: &str) -> Result<&'static str, String> {
    Ok(match name {
        "passive" => "Passive",
        "on_complete" => "OnBuilt",
        "on_research" => "OnTechnologyUnlocked",
        "on_kill" => "OnUnitKilled",
        "on_city_founded" => "OnCityFounded",
        "on_unit_trained" => "OnUnitTrained",
        "on_adopt" => "OnAdopt",
        "on_great_person_expended" => "OnGreatPersonExpended",
        "on_city_converted" => "OnCityConverted",
        "on_religion_spread" => "OnReligionSpread",
        other => return Err(format!("trigger:{other}")),
    })
}

/// Triggers whose yield effects are one-shot grants into a stock (paid
/// once per event): Reliquary, Mausoleum, Initiation Rites,
/// Interfaith Dialogue.
const GRANT_TRIGGERS: [&str; 3] = ["OnGreatPersonExpended", "OnCityConverted", "OnReligionSpread"];

fn one_shot_grant(trigger: &str, modifier: &str) -> bool {
    let yield_grant = modifier.starts_with("AddYield(") || modifier.starts_with("AddYieldPerCount(");
    GRANT_TRIGGERS.contains(&trigger) && yield_grant
}

/// Instant modifiers need a trigger; continuous ones must be passive (a
/// timed or one-shot yield grant has no catalog use yet).
fn check_trigger(owner: Owner, trigger: &str, instant: bool, modifier: &str) -> Result<(), String> {
    if instant && trigger == "Passive" {
        return Err(format!("passive_grant:{modifier}"));
    }
    if !instant && trigger != "Passive" {
        return Err(format!("triggered_modifier:{modifier}"));
    }
    let fits = match trigger {
        "OnBuilt" => owner == Owner::Building,
        "OnUnitTrained" => owner != Owner::Promotion,
        "OnAdopt" => owner == Owner::Policy,
        "OnCityConverted" | "OnReligionSpread" => owner == Owner::Belief,
        "OnGreatPersonExpended" => owner != Owner::Promotion,
        _ => true,
    };
    if fits {
        return Ok(());
    }
    Err(format!("trigger:{trigger}"))
}

fn yield_kind(name: &str) -> Option<&'static str> {
    Some(match name {
        "food" => "Food",
        "production" => "Production",
        "gold" => "Gold",
        "science" => "Science",
        "culture" => "Culture",
        "faith" => "Faith",
        _ => return None,
    })
}

fn int_amount(e: &Value, modifier: &str) -> Result<i64, String> {
    match &e["amount"] {
        Value::Number(n) if n.as_f64().map(|f| f.fract() == 0.0).unwrap_or(false) => {
            Ok(n.as_f64().unwrap_or(0.0) as i64)
        }
        _ => Err(format!("amount:{modifier}")),
    }
}

/// The key a grant names (target, else the filter of its kind).
fn granted(e: &Value, filter_key: &str, modifier: &str) -> Result<String, String> {
    if let Some(t) = e["target"].as_str() {
        return Ok(t.to_string());
    }
    match e["selector"]["filter"][filter_key].as_str() {
        Some(k) => Ok(k.to_string()),
        None => Err(format!("modifier:{modifier}")),
    }
}

/// RON modifier and whether it is instant.
fn modifier(e: &Value, name: &str) -> Result<(String, bool), String> {
    let unknown = || format!("modifier:{name}");
    let parts: Vec<&str> = name.split('.').collect();
    let text = match parts.as_slice() {
        ["yield", "tourism", "flat"] | ["building", "yield", "tourism", "flat"]
            if e["selector"]["filter"]["building"].as_str() == Some("purchased_with_faith") =>
        {
            format!("ModifyTourism(stat: PerFaithBuilding, amount: {})", int_amount(e, name)?)
        }
        ["yield", "tourism", "flat"] | ["building", "yield", "tourism", "flat"] => {
            format!("ModifyTourism(stat: Flat, amount: {})", int_amount(e, name)?)
        }
        ["building", "tourism", "pct"] => {
            format!("ModifyTourism(stat: Percent, amount: {})", int_amount(e, name)?)
        }
        ["theming", "bonus", "pct"] => {
            format!("ModifyTourism(stat: ThemingPercent, amount: {})", int_amount(e, name)?)
        }
        ["great_work", "yield", "culture", "flat"] => {
            format!("ModifyTourism(stat: GreatWorkCulture, amount: {})", int_amount(e, name)?)
        }
        // One-shot grants of the religion and great person triggers.
        ["faith", "on_great_person_expended"] => {
            format!("AddYield(yield_kind: Faith, amount: {})", int_amount(e, name)?)
        }
        ["great_person_expended", "gold", "flat"] | ["gold", "on_city_first_converted"] => {
            format!("AddYield(yield_kind: Gold, amount: {})", int_amount(e, name)?)
        }
        ["science", "on_spread_per_other_religion_follower"] => format!(
            "AddYieldPerCount(yield_kind: Science, amount: {}, per: (count: OtherReligionFollowers))",
            int_amount(e, name)?
        ),
        // A belief's trade route yield: the city's, when it is connected
        // to the capital (`modifier_condition`).
        ["trade", "route", "yield", y, "flat"] => {
            let y = yield_kind(y).ok_or_else(unknown)?;
            format!("AddYield(yield_kind: {y}, amount: {})", int_amount(e, name)?)
        }
        ["religion", "pressure_to_friendly_city_states", "pct"] => {
            religion("FriendlyCityStatePressure", int_amount(e, name)?)
        }
        ["happiness", "per_x_peaceful_foreign_followers"] => {
            per_count_happiness("PeacefulForeignFollowers", int_amount(e, name)?)
        }
        // Religious Community: +1 % per follower, the amount is the cap.
        ["yield", y, "pct_per_follower_max"] => {
            let y = yield_kind(y).ok_or_else(unknown)?;
            format!(
                "AddYieldPercentPerCount(yield_kind: {y}, percent: 1, per: (count: MajorityFollowers, max: Some({})))",
                int_amount(e, name)?
            )
        }
        ["yield", y, "per_religion_present"] => {
            let y = yield_kind(y).ok_or_else(unknown)?;
            format!(
                "AddYieldPerCount(yield_kind: {y}, amount: {}, per: (count: ReligionsPresent))",
                int_amount(e, name)?
            )
        }
        // Guruship: once per city with a specialist, not per specialist.
        ["specialist", "yield", y, "flat"]
            if e["selector"]["kind"].as_str() == Some("specialists_of_following_cities") =>
        {
            let y = yield_kind(y).ok_or_else(unknown)?;
            format!("AddStat(stat: AnySpecialistYield({y}), amount: {})", int_amount(e, name)?)
        }
        ["yield", y, "flat"]
        | ["tile", "yield", y, "flat" | "flat_if_unimproved"]
        | ["building", "yield", y, "flat"] => {
            let y = yield_kind(y).ok_or_else(unknown)?;
            format!(
                "AddYield(yield_kind: {y}, amount: {})",
                int_amount(e, name)?
            )
        }
        ["yield", y, "pct"] => {
            let y = yield_kind(y).ok_or_else(unknown)?;
            format!(
                "AddYieldPercent(yield_kind: {y}, percent: {})",
                int_amount(e, name)?
            )
        }
        ["yield", y, "per_pop"] => {
            let y = yield_kind(y).ok_or_else(unknown)?;
            let f = e["amount"].as_f64().ok_or_else(unknown)?;
            let hundredths = (f * 100.0).round() as i64;
            format!("AddYieldPerPopulation(yield_kind: {y}, hundredths: {hundredths})")
        }
        ["growth", "rate", "pct"] => format!("ModifyGrowth(percent: {})", int_amount(e, name)?),
        ["purchase", "cost", "pct"] => cost("Hurry", int_amount(e, name)?),
        ["purchase", "units", "cost", "pct"] => cost("UnitPurchase", int_amount(e, name)?),
        ["purchase", "buildings", "cost", "pct"] => cost("BuildingPurchase", int_amount(e, name)?),
        ["tile_purchase", "cost", "pct"] => cost("TilePurchase", int_amount(e, name)?),
        ["border", "culture_cost", "pct"] => cost("BorderGrowth", int_amount(e, name)?),
        ["border", "culture_cost_exponent", "pct"] => {
            cost("BorderGrowthExponent", int_amount(e, name)?)
        }
        ["policy", "cost", "pct"] => cost("Policies", int_amount(e, name)?),
        ["faith_purchase", "cost", "pct"] => cost("FaithPurchase", int_amount(e, name)?),
        ["unlock", "faith_purchase_building"] => {
            let building = granted(e, "building", name)?;
            format!("UnlockFaithBuilding({})", quote(&building))
        }
        ["unlock", "faith_purchase_units_from_era"] => {
            let era = e["selector"]["filter"]["era"].as_str().ok_or_else(unknown)?;
            format!("UnlockFaithUnits({})", quote(era))
        }
        ["yield", y, "per_following_city"] => {
            let y = yield_kind(y).ok_or_else(unknown)?;
            format!(
                "AddYieldPerFollowingCity(yield_kind: {y}, amount: {}, foreign_only: false)",
                int_amount(e, name)?
            )
        }
        ["yield", y, "per_foreign_following_city"] => {
            let y = yield_kind(y).ok_or_else(unknown)?;
            format!(
                "AddYieldPerFollowingCity(yield_kind: {y}, amount: {}, foreign_only: true)",
                int_amount(e, name)?
            )
        }
        ["yield", y, "per_x_followers"] => {
            let y = yield_kind(y).ok_or_else(unknown)?;
            format!(
                "AddYieldPerFollowers(yield_kind: {y}, per: {}, foreign_only: false)",
                int_amount(e, name)?
            )
        }
        ["yield", y, "per_x_foreign_followers"] => {
            let y = yield_kind(y).ok_or_else(unknown)?;
            format!(
                "AddYieldPerFollowers(yield_kind: {y}, per: {}, foreign_only: true)",
                int_amount(e, name)?
            )
        }
        // Morocco: per nation or city-state trading with the player.
        ["yield", y, "per_trade_partner"] => {
            let y = yield_kind(y).ok_or_else(unknown)?;
            format!(
                "AddYieldPerCount(yield_kind: {y}, amount: {}, per: (count: TradePartners))",
                int_amount(e, name)?
            )
        }
        // Hanse: per trade route of the nation to a city-state.
        ["yield", y, "pct_per_city_state_route"] => {
            let y = yield_kind(y).ok_or_else(unknown)?;
            format!(
                "AddYieldPercentPerCount(yield_kind: {y}, percent: {}, per: (count: CityStateRoutes))",
                int_amount(e, name)?
            )
        }
        ["city_connection", "gold", "pct"] => stat("ConnectionGoldPercent", int_amount(e, name)?),
        ["research_agreement", "science", "pct"] => {
            stat("ResearchAgreementPercent", int_amount(e, name)?)
        }
        ["great_scientist", "science", "pct"] => {
            stat("GreatScientistSciencePercent", int_amount(e, name)?)
        }
        ["happiness", "per_x_pop_in_city"] => per_count_happiness("Population", int_amount(e, name)?),
        ["happiness", "per_x_policies"] => per_count_happiness("Policies", int_amount(e, name)?),
        ["world_wonder", "yield", y, "flat"] => {
            let y = yield_kind(y).ok_or_else(unknown)?;
            format!(
                "AddYieldPerCount(yield_kind: {y}, amount: {}, per: (count: WorldWonders))",
                int_amount(e, name)?
            )
        }
        ["happiness", "per_following_city"] => {
            let f = e["amount"].as_f64().ok_or_else(unknown)?;
            format!("AddHappinessPerFollowingCity(hundredths: {})", (f * 100.0).round() as i64)
        }
        ["religion", "pressure_out", "pct"] => religion("PressureOut", int_amount(e, name)?),
        ["religion", "spread_distance", "pct"] => religion("SpreadDistance", int_amount(e, name)?),
        ["missionary", "strength", "pct"] => religion("MissionaryStrength", int_amount(e, name)?),
        ["prophet", "strength", "pct"] => religion("ProphetStrength", int_amount(e, name)?),
        ["prophet", "faith_threshold", "pct"] => religion("ProphetCost", int_amount(e, name)?),
        ["faith_purchase", "missionary_inquisitor", "cost", "pct"] => {
            religion("MissionaryCost", int_amount(e, name)?)
        }
        ["policy", "cost_per_city", "pct"] => cost("PolicyCityPenalty", int_amount(e, name)?),
        ["grant", "policy"] => {
            let count = int_amount(e, name)?;
            return Ok((format!("FreePolicies(count: {count})"), true));
        }
        ["happiness", "local" | "global"] | ["building", "happiness", "flat"] => {
            format!("AddHappiness(amount: {})", int_amount(e, name)?)
        }
        ["combat", context, "pct"] => {
            let context = match *context {
                "strength" => "Any",
                "attack" => "Attack",
                "defense" => "Defense",
                "attack_vs_city" => "VsCity",
                _ => return Err(unknown()),
            };
            format!(
                "ModifyCombat(context: {context}, percent: {})",
                int_amount(e, name)?
            )
        }
        ["unit", "moves", "flat"] => format!("ModifyMovement(amount: {})", int_amount(e, name)?),
        ["great_people", "rate", "pct"] => {
            let stat = match e["selector"]["filter"]["great_person"].as_str() {
                Some(unit) => format!("GreatPersonRate({})", great_person_kind(unit).ok_or_else(unknown)?),
                None => "GreatPeopleRate".to_string(),
            };
            format!("AddStat(stat: {stat}, amount: {})", int_amount(e, name)?)
        }
        ["growth", "food_kept", "pct"] => {
            format!("AddStat(stat: FoodKept, amount: {})", int_amount(e, name)?)
        }
        ["unhappiness", "pct"] | ["unhappiness", "from_population", "pct"] => {
            stat("PopulationUnhappinessPercent", int_amount(e, name)?)
        }
        ["unhappiness", "from_cities", "pct"] => stat("CityCountUnhappinessPercent", int_amount(e, name)?),
        ["happiness", "per_city"] => stat("HappinessPerCity", int_amount(e, name)?),
        ["happiness", "per_luxury", "flat"] => stat("HappinessPerLuxury", int_amount(e, name)?),
        ["happiness", "natural_wonders", "pct"] => {
            stat("NaturalWonderHappinessPercent", int_amount(e, name)?)
        }
        ["golden_age", "duration", "pct"] => stat("GoldenAgeLengthPercent", int_amount(e, name)?),
        // Songhai: camps and captured cities; Burial Tomb: its captor.
        ["gold", "plunder", "pct"] => stat("PlunderGoldPercent", int_amount(e, name)?),
        ["capture", "plunder_gold", "pct"] => stat("CapturedGoldPercent", int_amount(e, name)?),
        ["resource", "provide"] => {
            let resource = granted(e, "resource", name)?;
            stat(&format!("ResourceCopies({})", quote(&resource)), int_amount(e, name)?)
        }
        ["resource", "quantity", "pct"] => {
            let resource = granted(e, "resource", name)?;
            stat(&format!("ResourceQuantityPercent({})", quote(&resource)), int_amount(e, name)?)
        }
        // The catalog value 200 is the quantity in percent: +100 %
        // (policy text: "increased by 100%").
        ["resource", "strategic_quantity", "pct"] => {
            stat("StrategicQuantityPercent", int_amount(e, name)? - 100)
        }
        ["resource", "extra_luxury_copy"] if e["amount"] == Value::Bool(true) => {
            stat("ExtraLuxuryCopies", 1)
        }
        ["specialist", "yield", y, "flat"] => {
            let y = yield_kind(y).ok_or_else(unknown)?;
            let specialist = match e["selector"]["filter"]["specialist"].as_str() {
                Some(s) => format!("Some({})", quote(s)),
                None => "None".to_string(),
            };
            format!(
                "AddStat(stat: SpecialistYield(specialist: {specialist}, yield_kind: {y}), amount: {})",
                int_amount(e, name)?
            )
        }
        ["unlock", "unit"] => format!("UnlockUnit({})", quote(&granted(e, "unit", name)?)),
        ["grant", "unit"] => {
            let unit = granted(e, "unit", name)?;
            let count = int_amount(e, name)?;
            return Ok((
                format!("FreeUnit(unit: {}, count: {count})", quote(&unit)),
                true,
            ));
        }
        ["grant", "building"] => {
            let building = granted(e, "building", name)?;
            return Ok((format!("FreeBuilding({})", quote(&building)), true));
        }
        ["grant", "promotion"] => {
            let promotion = granted(e, "promotion", name)?;
            return Ok((format!("AddPromotion({})", quote(&promotion)), true));
        }
        // Honor, the Aztecs, God of War: a share of the victim's strength.
        [y, "on_kill", "pct_of_strength"] if matches!(*y, "gold" | "science" | "culture" | "faith") => {
            let y = yield_kind(y).ok_or_else(unknown)?;
            let percent = int_amount(e, name)?;
            return Ok((format!("YieldFromKill(yield_kind: {y}, percent: {percent})"), true));
        }
        _ => return Err(unknown()),
    };
    Ok((text, false))
}

/// Great person kind of a great person unit key.
fn great_person_kind(unit: &str) -> Option<&'static str> {
    Some(match unit.rsplit('/').next()? {
        "great-artist" => "Artist",
        "great-engineer" => "Engineer",
        "great-merchant" => "Merchant",
        "great-scientist" => "Scientist",
        "great-writer" => "Writer",
        "great-musician" => "Musician",
        "great-general" => "General",
        "great-admiral" => "Admiral",
        "great-prophet" => "Prophet",
        _ => return None,
    })
}

/// Filters that parameterise a stat (the great person of a rate, the
/// specialist of a yield) are not a selection: drop them before the
/// selector is mapped.
fn without_parameter_filters(e: &Value, modifier: &str) -> Value {
    let key = match modifier {
        "great_people.rate.pct" => "great_person",
        "resource.provide" | "resource.quantity.pct" => "resource",
        m if m.starts_with("specialist.yield.") => "specialist",
        _ => return e.clone(),
    };
    let mut out = e.clone();
    if let Some(filter) = out["selector"]["filter"].as_object_mut() {
        filter.remove(key);
    }
    out
}

/// +1 happiness per `every` counted things.
fn per_count_happiness(count: &str, every: i64) -> String {
    format!("AddHappinessPerCount(amount: 1, per: (count: {count}, every: {every}))")
}

fn stat(stat: &str, amount: i64) -> String {
    format!("AddStat(stat: {stat}, amount: {amount})")
}

/// Stats a player effect may add to (the rest are city stats).
const PLAYER_STATS: &[&str] = &[
    "PlunderGoldPercent",
    "ResourceCopies",
    "ResourceQuantityPercent",
    "StrategicQuantityPercent",
    "GreatPe",
    "ConnectionGoldPercent",
    "ResearchAgreementPercent",
    "GreatScientistSciencePercent",
    "HappinessPerCity",
    "HappinessPerLuxury",
    "NaturalWonderHappinessPercent",
    "CityCountUnhappinessPercent",
    "PopulationUnhappinessPercent",
    "GoldenAgeLengthPercent",
    "GreatPersonRate",
];

/// Stats a city effect may not add to.
const PLAYER_ONLY_STATS: &[&str] = &[
    "ConnectionGoldPercent",
    "ResearchAgreementPercent",
    "GreatScientistSciencePercent",
    "PlunderGoldPercent",
    "ResourceCopies",
    "ResourceQuantityPercent",
    "StrategicQuantityPercent",
    "HappinessPerCity",
    "HappinessPerLuxury",
    "NaturalWonderHappinessPercent",
    "CityCountUnhappinessPercent",
    "GoldenAgeLengthPercent",
];

fn stat_fits(modifier: &str, scope: &str) -> bool {
    let named = |list: &[&str]| list.iter().any(|s| modifier.starts_with(&format!("AddStat(stat: {s}")));
    match scope {
        "Player" => named(&PLAYER_STATS),
        "City" => !named(&PLAYER_ONLY_STATS),
        _ => false,
    }
}

fn cost(target: &str, percent: i64) -> String {
    format!("ModifyCost(target: {target}, percent: {percent})")
}

fn religion(stat: &str, percent: i64) -> String {
    format!("ModifyReligion(stat: {stat}, percent: {percent})")
}

/// Selector → scope/selector (+ implied condition) the compiler accepts
/// for this owner and trigger.
fn target(e: &Value, owner: Owner, trigger: &str) -> Result<Target, String> {
    let kind = e["selector"]["kind"].as_str().unwrap_or("");
    let filter = &e["selector"]["filter"];
    let missing = || format!("selector:{kind}");
    let only = |key: &str| filter_only(filter, key);
    let t = |scope, selector, condition| Target {
        scope,
        selector,
        condition,
    };
    let modifier = e["modifier"].as_str().unwrap_or("");
    let grant = modifier.starts_with("grant.");
    let target = match kind {
        "player" if no_filter(filter) => t("Player", "Owner", None),
        "specialists_of_city" if owner == Owner::Building && no_filter(filter) => {
            t("City", "SourceCity", None)
        }
        "specialists_of_player" if no_filter(filter) => t("City", "OwnerCities", None),
        "great_works_of_player" if no_filter(filter) => t("Player", "Owner", None),
        "capital" if no_filter(filter) || (grant && grant_filter(filter)) => {
            t("City", "Capital", None)
        }
        "this_city"
            if owner == Owner::Building
                && (no_filter(filter) || (grant && grant_filter(filter))) =>
        {
            if trigger == "OnUnitTrained" {
                return Err(missing());
            }
            t("City", "SourceCity", None)
        }
        "all_cities" if no_filter(filter) && trigger == "OnCityFounded" => t("City", "EventCity", None),
        "all_cities" if no_filter(filter) => t("City", "OwnerCities", None),
        // The catalog names the gold purchase (Mercantilism, Big Ben): the
        // only hurry of the ruleset. A building's hurry percent goes to its
        // owner (rules.md, building purchase modifiers): Big Ben
        // makes every city's purchases cheaper (unverified).
        "all_cities" | "this_city" if only("hurry") && filter["hurry"].as_str() == Some("gold") => {
            t("City", "OwnerCities", None)
        }
        "coastal_cities" if no_filter(filter) => t("City", "OwnerCities", Some("CityCoastal".into())),
        "all_cities" | "buildings_of_player" | "cities_with_building" if only("building") => {
            let b = filter["building"].as_str().unwrap_or("");
            t(
                "City",
                "OwnerCities",
                Some(format!("HasBuilding({})", quote(b))),
            )
        }
        "river_tiles_of_city" | "water_tiles_of_city" | "water_resource_tiles_of_city"
        | "lake_tiles_of_city"
            if owner == Owner::Building && no_filter(filter) =>
        {
            let plot = plot_of_selector(kind).ok_or_else(missing)?;
            t("Tile", "SourceCityTiles", Some(format!("TilePlot({plot})")))
        }
        "natural_wonder_tiles_of_following_cities" if owner == Owner::Belief && no_filter(filter) => {
            t("Tile", "FollowingCityTiles", Some("TilePlot(NaturalWonder)".into()))
        }
        "tiles_of_city" | "tiles_of_player" if no_filter(filter) => {
            let source = e["tag"].as_str().unwrap_or("");
            match lost_tile_filter(source) {
                Some(condition) => t("Tile", "OwnerCityTiles", Some(condition.into())),
                // A tile condition of the catalog (Spain: `tile: natural_wonder`).
                None if kind == "tiles_of_player" && !e["condition"].is_null() => {
                    t("Tile", "OwnerCityTiles", None)
                }
                None => return Err(format!("tile_filter:{source}")),
            }
        }
        "tiles_of_player" if modifier.ends_with(".flat_if_unimproved") && only("feature") => {
            let feature = filter["feature"].as_str().unwrap_or("");
            t(
                "Tile",
                "OwnerCityTiles",
                Some(format!("TileFeatureUnimproved({})", quote(feature))),
            )
        }
        "tiles_of_city" if owner == Owner::Building => {
            let condition = tile_condition(filter).ok_or_else(missing)?;
            t("Tile", "SourceCityTiles", Some(condition))
        }
        "tiles_of_player" => {
            let condition = tile_condition(filter).ok_or_else(missing)?;
            t("Tile", "OwnerCityTiles", Some(condition))
        }
        "following_cities"
            if owner == Owner::Belief
                && (no_filter(filter) || only("building") || only("era"))
                && modifier.starts_with("unlock.") =>
        {
            t("City", "FollowingCities", None)
        }
        "following_cities" if owner == Owner::Belief && no_filter(filter) => {
            t("City", "FollowingCities", None)
        }
        "buildings_in_following_cities" if owner == Owner::Belief && only("building") => {
            let b = filter["building"].as_str().unwrap_or("");
            t(
                "City",
                "FollowingCities",
                Some(format!("HasBuilding({})", quote(b))),
            )
        }
        "tiles_of_following_cities" if owner == Owner::Belief => {
            let condition = tile_condition(filter).ok_or_else(missing)?;
            t("Tile", "FollowingCityTiles", Some(condition))
        }
        // A triggered belief effect reaches the player of its event.
        "religion_founder" if owner == Owner::Belief && no_filter(filter) && trigger != "Passive" => {
            t("Player", "Owner", None)
        }
        "religion_founder" if owner == Owner::Belief && no_filter(filter) => {
            t("Player", "Founder", None)
        }
        "specialists_of_following_cities" if owner == Owner::Belief && no_filter(filter) => {
            t("City", "FollowingCities", None)
        }
        // Sacred Sites: tourism of the religion's cities per faith building.
        "religion_founder"
            if owner == Owner::Belief
                && filter["building"].as_str() == Some("purchased_with_faith") =>
        {
            t("City", "FollowingCities", None)
        }
        "holy_city" if owner == Owner::Belief && no_filter(filter) => t("City", "HolyCity", None),
        "religion" if owner == Owner::Belief && no_filter(filter) => {
            t("Religion", "OwnReligion", None)
        }
        "this_unit" if owner == Owner::Promotion && no_filter(filter) => {
            t("Unit", "SourceUnit", None)
        }
        "all_units" if no_filter(filter) && owner != Owner::Promotion => {
            t("Unit", "OwnerUnits", None)
        }
        "units_trained_here" if trigger == "OnUnitTrained" && no_filter(filter) => {
            t("Unit", "EventUnit", None)
        }
        _ => return Err(missing()),
    };
    Ok(target)
}

/// The plot kind of a catalog plot-yield selector (river, water, lake tiles of a city).
fn plot_of_selector(kind: &str) -> Option<&'static str> {
    Some(match kind {
        "river_tiles_of_city" => "River",
        "water_tiles_of_city" => "Sea",
        "water_resource_tiles_of_city" => "SeaResource",
        "lake_tiles_of_city" => "Lake",
        _ => return None,
    })
}

/// The tile condition a catalog record lost with its filter, restored from
/// the catalog column it was read from: the strategic-resource yield change
/// of a trait (Russia) yields on strategic resource tiles.
fn lost_tile_filter(source: &str) -> Option<&'static str> {
    match source {
        "strategic_resource_tiles" => Some("TileResourceClass(Strategic)"),
        _ => None,
    }
}

/// Filters a grant names its target with (not a selection).
fn grant_filter(filter: &Value) -> bool {
    match filter.as_object() {
        Some(map) => map
            .keys()
            .all(|k| matches!(k.as_str(), "unit" | "building" | "promotion")),
        None => true,
    }
}

fn no_filter(filter: &Value) -> bool {
    match filter.as_object() {
        Some(map) => map.is_empty(),
        None => true,
    }
}

fn filter_only(filter: &Value, key: &str) -> bool {
    match filter.as_object() {
        Some(map) => map.len() == 1 && map.contains_key(key),
        None => false,
    }
}

/// Tile filter → Tile condition; None when the filter has no Tile
/// condition (terrain, several keys, ...) or is missing: every tile
/// effect of the ruleset is conditional, an unfiltered one lost its condition in
/// the catalog (Russia: the strategic-resource yield change has no filter).
fn tile_condition(filter: &Value) -> Option<String> {
    let map = filter.as_object()?;
    if map.len() != 1 {
        return None;
    }
    let (key, value) = map.iter().next()?;
    let value = value.as_str()?;
    let condition = match key.as_str() {
        "resource" => "TileResource",
        "improvement" => "TileImprovement",
        "feature" => "TileFeature",
        "terrain" => "TileTerrain",
        _ => return None,
    };
    Some(format!("{condition}({})", quote(value)))
}

/// The condition a modifier name implies: the trade route yield of a
/// belief is the yield of a city connected to the capital
/// (Messenger of the Gods).
fn modifier_condition(name: &str) -> Option<String> {
    if name.starts_with("trade.route.yield.") {
        return Some("ConnectedToCapital".into());
    }
    None
}

/// Catalog condition plus the implied ones (selector, modifier) → at most
/// two RON conditions (`condition`, `second_condition`). Two conditions
/// stay off Unit scope (the compiler keeps the second owner-level).
fn conditions(c: &Value, implied: &[Option<String>], scope: &str) -> Result<Vec<String>, String> {
    let mut out: Vec<String> = implied.iter().flatten().cloned().collect();
    if let Some(map) = c.as_object() {
        for (key, value) in map {
            if let Some(text) = catalog_condition(key, value)? {
                out.push(text);
            }
        }
    }
    let too_many = out.len() > 2 || (out.len() == 2 && scope == "Unit");
    if too_many {
        let keys: Vec<&str> = match c.as_object() {
            Some(map) => map.keys().map(|k| k.as_str()).collect(),
            None => Vec::new(),
        };
        return Err(format!("condition:{}", keys.join("+")));
    }
    Ok(out)
}

/// One catalog condition key → RON condition (None: no gate of its own).
fn catalog_condition(key: &str, value: &Value) -> Result<Option<String>, String> {
    let text = match (key, value) {
        ("tech_known", Value::String(t)) => format!("HasTechnology({})", quote(t)),
        ("until_tech", Value::String(t)) => format!("LacksTechnology({})", quote(t)),
        ("min_population", Value::Number(n)) => format!("MinPopulation({})", n),
        ("min_followers", Value::Number(n)) => format!("MinFollowers({})", n),
        // On a kill: the victim, and the distance to the source's city.
        ("victim", Value::String(v)) if v == "barbarian" => "OpponentIsBarbarian".to_string(),
        ("max_distance", Value::Number(n)) => format!("EventNearSourceCity({n})"),
        ("tile", Value::String(t)) if t == "natural_wonder" => "TilePlot(NaturalWonder)".to_string(),
        ("golden_age", Value::Bool(true)) => "GoldenAge".to_string(),
        ("city_garrisoned", Value::Bool(true)) => "CityGarrisoned".to_string(),
        ("city_on_river", Value::Bool(true)) => "CityOnRiver".to_string(),
        ("requires_peace", Value::Bool(true)) => "AtPeace".to_string(),
        ("doubles_with_tech", Value::String(_)) => return Ok(None),
        _ => return Err(format!("condition:{key}")),
    };
    Ok(Some(text))
}

/// The scope rules of the compiler (`modifier_fits_scope`), so that a
/// typed effect never fails to compile.
fn check_scope(name: &str, scope: &str, modifier: &str) -> Result<(), String> {
    let player_counts = ["Policies", "ConnectedCities", "PeacefulForeignFollowers", "OtherReligionFollowers"];
    let city_counts = ["Population", "WorldWonders", "MajorityFollowers", "ReligionsPresent"];
    let counts = |list: &[&str]| list.iter().any(|c| modifier.contains(&format!("count: {c}")));
    let count_fits = if counts(&player_counts) {
        scope == "Player"
    } else if counts(&city_counts) {
        scope == "City"
    } else {
        true
    };
    if !count_fits {
        return Err(format!("scope:{name}@{}", scope.to_lowercase()));
    }
    let player_yield = ["Gold", "Science", "Faith"]
        .iter()
        .any(|y| modifier.contains(&format!("yield_kind: {y}")));
    let ok =
        if modifier.starts_with("AddYieldPerPopulation") || modifier.starts_with("ModifyGrowth") {
            scope == "City"
        } else if modifier.starts_with("AddYieldPercent") {
            matches!(scope, "City" | "Tile") || (scope == "Player" && player_yield)
        } else if modifier.starts_with("AddYield") {
            matches!(scope, "City" | "Tile") || (scope == "Player" && player_yield)
        } else if modifier.starts_with("ModifyCost") {
            let player_cost = ["Technologies", "Policies", "PolicyCityPenalty", "FaithPurchase"]
                .iter()
                .any(|t| modifier.contains(&format!("target: {t},")));
            if player_cost {
                scope == "Player"
            } else {
                scope == "City"
            }
        } else if modifier.starts_with("FreeBuildingInCities")
            || modifier.starts_with("FreePolicies")
            || modifier.starts_with("AddHappinessPerFollowingCity")
            || modifier.starts_with("AllowReformationBelief")
        {
            scope == "Player"
        } else if modifier.starts_with("AddYieldPerFollowingCity")
            || modifier.starts_with("AddYieldPerFollowers")
        {
            scope == "City" || (scope == "Player" && player_yield)
        } else if modifier.starts_with("ModifyTourism") {
            let player = ["Spread", "GreatWorkCulture"]
                .iter()
                .any(|s| modifier.contains(&format!("stat: {s},")));
            if modifier.contains("stat: ThemingPercent,") {
                matches!(scope, "Player" | "City")
            } else {
                scope == if player { "Player" } else { "City" }
            }
        } else if modifier.starts_with("ModifyReligion") {
            scope == "Religion"
        } else if modifier.starts_with("UnlockFaithBuilding") || modifier.starts_with("UnlockFaithUnits") {
            scope == "City"
        } else if modifier.starts_with("FreeUnit") || modifier.starts_with("FreeBuilding") {
            scope == "City"
        } else if modifier.starts_with("AddHappiness") {
            matches!(scope, "City" | "Player")
        } else if modifier.starts_with("ModifyCombat")
            || modifier.starts_with("ModifyMovement")
            || modifier.starts_with("AddPromotion")
        {
            scope == "Unit"
        } else if modifier.starts_with("UnlockUnit") {
            scope == "Player"
        } else if modifier.starts_with("AddStat") {
            stat_fits(modifier, scope)
        } else if modifier.starts_with("YieldFromKill") {
            scope == "Player"
        } else {
            false
        };
    if ok {
        return Ok(());
    }
    Err(format!("scope:{name}@{}", scope.to_lowercase()))
}
