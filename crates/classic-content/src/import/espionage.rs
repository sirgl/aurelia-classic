//! Catalog effects of espionage and the World Congress delegates →
//! typed effects (classic/docs/classic/rules.md):
//! `AddStat(Espionage(..))`, `AddStat(League(..))` and the instant
//! `GrantSpies` / `PromoteSpies` of the National Intelligence Agency.
//!
//! Returns None when the effect is not one of them. The catalog marks
//! Globalization's delegates per diplomat as `on_research`; they count
//! while the technology is known, so the effect is passive.

use serde_json::Value;

use crate::import::effects::Owner;

/// The catalog modifiers this mapper types.
const MODIFIERS: [&str; 10] = [
    "espionage.enemy_steal_rate.pct",
    "espionage.steal_rate.pct",
    "espionage.catch_chance.pct",
    "espionage.rig_election.pct",
    "espionage.spies.flat",
    "espionage.new_spy_rank.flat",
    "espionage.spy_rank.flat",
    "belief.spy_pressure",
    "league.delegates.flat",
    "league.delegates_per_diplomat.flat",
];

/// The typed RON effect of an espionage or delegate modifier.
pub fn espionage_effect(e: &Value, owner: Owner) -> Option<Result<String, String>> {
    let modifier = e["modifier"].as_str().unwrap_or("");
    if !MODIFIERS.contains(&modifier) {
        return None;
    }
    let trigger = e["trigger"].as_str().unwrap_or("");
    let amount = match e["amount"].as_i64() {
        Some(n) => n,
        None => return Some(Err(format!("amount:{modifier}"))),
    };
    let player = |stat: &str| {
        Ok(format!("(scope: Player, selector: Owner, modifier: AddStat(stat: {stat}, amount: {amount}))"))
    };
    let text = match (modifier, trigger) {
        ("espionage.enemy_steal_rate.pct", "passive") if owner == Owner::Building => {
            let selector = match e["selector"]["kind"].as_str() {
                Some("this_city") => "SourceCity",
                Some("all_cities") => "OwnerCities",
                other => return Some(Err(format!("selector:{}", other.unwrap_or("")))),
            };
            Ok(format!(
                "(scope: City, selector: {selector}, modifier: AddStat(stat: Espionage(EnemyStealPercent), amount: {amount}))"
            ))
        }
        ("espionage.steal_rate.pct", "passive") => player("Espionage(StealPercent)"),
        ("espionage.catch_chance.pct", "passive") => player("Espionage(CatchPercent)"),
        ("espionage.rig_election.pct", "passive") => player("Espionage(RigElectionPercent)"),
        ("espionage.spies.flat", "passive") => player("Espionage(ExtraSpies)"),
        ("espionage.new_spy_rank.flat", "passive") => player("Espionage(StartingRank)"),
        ("espionage.spies.flat", "on_complete") if owner == Owner::Building => Ok(format!(
            "(scope: Player, selector: Owner, trigger: OnBuilt, modifier: GrantSpies(count: {amount}), duration: Once)"
        )),
        ("espionage.spy_rank.flat", "on_complete") if owner == Owner::Building => Ok(format!(
            "(scope: Player, selector: Owner, trigger: OnBuilt, modifier: PromoteSpies(ranks: {amount}), duration: Once)"
        )),
        ("belief.spy_pressure", "passive") if owner == Owner::Belief => Ok(format!(
            "(scope: Player, selector: Founder, modifier: AddStat(stat: Espionage(SpyPressure), amount: {amount}))"
        )),
        ("league.delegates.flat", "passive") => player("League(Delegates)"),
        ("league.delegates_per_diplomat.flat", "passive" | "on_research") => player("League(DelegatesPerDiplomat)"),
        _ => return None,
    };
    Some(text)
}
