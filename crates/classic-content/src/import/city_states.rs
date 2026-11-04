//! Catalog effects on city-state dealings (policies, beliefs, traits) →
//! typed `ModifyCityState` effects (classic/docs/classic/rules.md).
//!
//! Returns None when the effect is not one of them. Influence per turn is
//! two stats by its catalog condition: `afraid` (Gunboat Diplomacy) and
//! `protected` (Treaty Organization: the per-turn influence holds with every
//! city-state a trade route connects — the catalog's name
//! follows its source column, not the rule). The unit gift rate
//! carries its common-enemy condition in its consumer. The resting point of
//! a belief (Papal Primacy) is its own stat, applied only with the
//! city-states following the religion (`RestingPointSameReligion`).

use serde_json::Value;

use crate::import::effects::Owner;

fn stat_of(modifier: &str, owner: Owner) -> Option<&'static str> {
    Some(match modifier {
        "city_state.influence_resting_point.flat" if owner != Owner::Belief => "RestingPoint",
        // Papal Primacy: only with city-states following the religion.
        "city_state.influence_resting_point.flat" => "RestingPointSameReligion",
        "city_state.influence_decay.pct" => "DecayPercent",
        "city_state.gold_gift_influence.pct" => "GoldGiftPercent",
        "city_state.bully_score.pct" => "BullyScorePercent",
        "city_state.unit_gift_influence.flat" => "UnitGiftInfluence",
        "city_state.friend_bonus.pct" => "FriendBonusPercent",
        "trait.city_state_friendship_modifier" => "FriendshipRate",
        "trait.able_to_annex_city_states" => "AnnexCityStates",
        "city_state.unit_gift_rate.pct" => "UnitGiftRatePercent",
        "policy.minor_resource_bonus" => "AllyResourceBonus",
        "policy.minor_great_people_allies" => "AllyGreatPeople",
        "policy.minor_science_allies" => "AllyScience",
        "trait.great_person_gift_influence" => "GreatPersonGiftInfluence",
        "trade_mission.gold.pct" => "TradeMissionGoldPercent",
        _ => return None,
    })
}

/// Influence per turn by the city-states it holds with.
fn influence_stat(e: &Value) -> Option<&'static str> {
    let condition = e["condition"].as_object()?;
    if condition.len() != 1 {
        return None;
    }
    if condition.get("afraid") == Some(&Value::Bool(true)) {
        return Some("AfraidInfluence");
    }
    if condition.get("protected") == Some(&Value::Bool(true)) {
        return Some("TradeRouteInfluence");
    }
    None
}

/// The typed RON effect of a city-state modifier of a policy, belief or
/// nation.
pub fn city_state_effect(e: &Value, owner: Owner) -> Option<Result<String, String>> {
    let modifier = e["modifier"].as_str().unwrap_or("");
    let stat = if modifier == "city_state.influence_per_turn" {
        match influence_stat(e) {
            Some(stat) => stat,
            None => return Some(Err(format!("condition:{modifier}"))),
        }
    } else {
        stat_of(modifier, owner)?
    };
    let selector = match owner {
        Owner::Belief => "Founder",
        Owner::Policy | Owner::Nation | Owner::Building => "Owner",
        _ => return None,
    };
    let amount = match &e["amount"] {
        Value::Number(n) => n.as_f64().unwrap_or(0.0).round() as i64,
        Value::Bool(true) => 1,
        _ => return Some(Err(format!("amount:{modifier}"))),
    };
    Some(Ok(format!(
        "(scope: Player, selector: {selector}, modifier: ModifyCityState(stat: {stat}, amount: {amount}))"
    )))
}
