//! Citizen decisions from the observation: the focus of each own city.
//!
//! ```text
//! starving (food surplus < 0)        Food
//! building a settler (food unit)     Production: the surplus already turns
//!                                    into production, and only its first
//!                                    2 points count in full
//! deficit the treasury cannot carry   Gold (kept until the empire earns
//!   for GOLD_FOCUS_TURNS             GOLD_COMFORT a turn): a treasury in
//!                                    deficit disbands units; a small
//!                                    deficit on a full treasury is paid
//!                                    from it (gold tiles starved the
//!                                    early capitals: size 2 at round 60)
//! small (population < 4)             Food
//! otherwise                          Default (balanced weights)
//! ```
//!
//! The inputs change slowly (population, the head of the queue, the gold
//! balance with its margin), so the focus does not flip back and forth.
//!
//! An AI that prices its economy (`PriceModel::Uses`, `priced_focus`) keeps
//! the first three rows and otherwise takes the focus whose assignment
//! weights are the most alike its prices: food while growth pays, production
//! or gold while unhappiness makes growth cheap.

use crate::ai::value::Prices;
use crate::command::Command;
use crate::ids::ProductionItem;
use crate::knowledge::observation::{AIObservation, OwnCity};
use crate::rules::CompiledRuleset;
use crate::world::citizens::CityFocus;

/// Population below which a city favours food.
pub const SMALL_CITY: u32 = 4;
/// Gold a turn a gold-focused empire earns before its cities go back to
/// their usual focus.
pub const GOLD_COMFORT: i32 = 3;
/// Turns of deficit the treasury must cover before the cities work gold.
pub const GOLD_FOCUS_TURNS: u32 = 20;

/// The empire loses more gold than its treasury carries for
/// `GOLD_FOCUS_TURNS`.
fn draining(gold_income: i32, treasury: u32) -> bool {
    let deficit = (-gold_income).max(0) as u32;
    deficit > 0 && treasury < deficit * GOLD_FOCUS_TURNS
}

/// The head of the queue is a food unit (settler).
fn building_food_unit(rules: &CompiledRuleset, city: &OwnCity) -> bool {
    match city.queue.first() {
        Some(ProductionItem::Unit(kind)) => rules
            .units
            .get(kind.0 as usize)
            .is_some_and(|u| u.module.economy.food),
        _ => false,
    }
}

/// The focus the city should have now (`gold_income`: the empire's gold
/// balance a turn, `treasury`: its gold).
pub fn city_focus(rules: &CompiledRuleset, city: &OwnCity, gold_income: i32, treasury: u32) -> CityFocus {
    if city.income.food_surplus < 0 {
        return CityFocus::Food;
    }
    if building_food_unit(rules, city) {
        return CityFocus::Production;
    }
    let keep_gold = city.citizens.focus == CityFocus::Gold && gold_income < GOLD_COMFORT;
    if draining(gold_income, treasury) || keep_gold {
        return CityFocus::Gold;
    }
    if city.population < SMALL_CITY {
        return CityFocus::Food;
    }
    CityFocus::Default
}

/// The focus of a city when the AI decides by its prices
/// (`PriceModel::Uses`): food while starving, production while a settler
/// takes the surplus, gold while the treasury loses gold (a deficit
/// disbands units); otherwise the focus whose assignment weights are the
/// most alike the prices of the yields (the largest cosine), so a city
/// works gold tiles when gold pays more than the default weights say.
pub fn priced_focus(rules: &CompiledRuleset, city: &OwnCity, gold_income: i32, treasury: u32, prices: &Prices) -> CityFocus {
    if city.income.food_surplus < 0 {
        return CityFocus::Food;
    }
    if building_food_unit(rules, city) {
        return CityFocus::Production;
    }
    if draining(gold_income, treasury) {
        return CityFocus::Gold;
    }
    alike_focus([prices.food, prices.production, prices.gold, prices.science, prices.culture, prices.faith, 0])
}

/// The focus whose assignment weights are the most alike `prices` (food,
/// production, gold, science, culture, faith, specialist points): the
/// largest cosine; the default on a tie.
pub fn alike_focus(prices: [i64; 7]) -> CityFocus {
    let alike = |focus: CityFocus| {
        let w = kernel::derived::citizens::weights(focus);
        let dot: i128 = w.iter().zip(prices).map(|(w, p)| i128::from(*w) * i128::from(p.max(0))).sum();
        let norm: i128 = w.iter().map(|w| i128::from(*w) * i128::from(*w)).sum();
        (dot, norm)
    };
    // dot² / |w|² compared without division.
    let better = |a: (i128, i128), b: (i128, i128)| a.0 * a.0 * b.1 > b.0 * b.0 * a.1;
    let mut best = (CityFocus::Default, alike(CityFocus::Default));
    for focus in PRICED_FOCUSES {
        let candidate = alike(focus);
        if better(candidate, best.1) {
            best = (focus, candidate);
        }
    }
    best.0
}

/// The focuses `priced_focus` weighs against the default (great people
/// points have no price).
const PRICED_FOCUSES: [CityFocus; 6] =
    [CityFocus::Food, CityFocus::Production, CityFocus::Gold, CityFocus::Science, CityFocus::Culture, CityFocus::ModuleYield];

/// SetCityFocus for every own city whose focus differs from `city_focus`.
pub fn focus_commands(obs: &AIObservation<'_>, rules: &CompiledRuleset) -> Vec<Command> {
    focus_commands_priced(obs, rules, None)
}

/// `focus_commands`, by `priced_focus` when the AI decides by `prices`.
pub fn focus_commands_priced(obs: &AIObservation<'_>, rules: &CompiledRuleset, prices: Option<&Prices>) -> Vec<Command> {
    let mut out = Vec::new();
    for city in &obs.own_cities {
        let (gold, treasury) = (obs.economy.income.gold, obs.economy.treasury);
        let focus = match prices {
            Some(prices) => priced_focus(rules, city, gold, treasury, prices),
            None => city_focus(rules, city, gold, treasury),
        };
        if city.citizens.focus != focus {
            out.push(Command::SetCityFocus {
                player: obs.player,
                city: city.handle,
                focus,
            });
        }
        if prices.is_some() {
            let avoid = avoid_growth(obs.economy.turn_happiness, city.citizens.avoid_growth);
            if avoid != city.citizens.avoid_growth {
                out.push(Command::SetAvoidGrowth { player: obs.player, city: city.handle, avoid });
            }
        }
    }
    out
}

/// Whether a city should avoid growth: the empire's excess happiness
/// (`excess`) is at or below `AVOID_GROWTH_AT` (kept until it climbs back
/// to `RESUME_GROWTH_AT`, so a city does not flip every turn). A citizen
/// that grows past the line costs more happiness than it brings; the
/// rules' own stop at the very-unhappy line (growth −75 %, no new city)
/// comes later and takes the whole empire's growth with it.
pub fn avoid_growth(excess: i32, avoiding: bool) -> bool {
    if avoiding {
        excess < RESUME_GROWTH_AT
    } else {
        excess <= AVOID_GROWTH_AT
    }
}

/// Excess happiness at which the cities stop growing.
pub const AVOID_GROWTH_AT: i32 = -2;
/// Excess happiness at which they grow again.
pub const RESUME_GROWTH_AT: i32 = 2;

#[cfg(test)]
mod tests {
    use super::*;

    /// Prices alike the default weights keep the default; a yield priced
    /// far above them takes its focus.
    #[test]
    fn the_focus_follows_the_prices() {
        assert_eq!(alike_focus([12, 8, 5, 6, 6, 5, 4]), CityFocus::Default);
        assert_eq!(alike_focus([4_000, 900, 300, 150, 250, 150, 0]), CityFocus::Food);
        assert_eq!(alike_focus([0, 900, 300, 150, 250, 150, 0]), CityFocus::Production);
        assert_eq!(alike_focus([100, 300, 1_500, 100, 100, 100, 0]), CityFocus::Gold);
    }
}
