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

/// The treasury carries a gold balance of `gold_income` a turn for
/// `GOLD_FOCUS_TURNS`: the cities would not turn to gold for it
/// (`draining`). The rules punish only a treasury below zero.
pub(crate) fn carries(gold_income: i32, treasury: u32) -> bool {
    !draining(gold_income, treasury)
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
    focus_commands_priced(obs, rules, None, GrowthPolicy::STANDARD)
}

/// `focus_commands`, by `priced_focus` when the AI decides by `prices`, its
/// cities growing by `policy`.
pub fn focus_commands_priced(obs: &AIObservation<'_>, rules: &CompiledRuleset, prices: Option<&Prices>, policy: GrowthPolicy) -> Vec<Command> {
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
            let avoid = policy.avoid(obs.economy.turn_happiness, city.citizens.avoid_growth);
            if avoid != city.citizens.avoid_growth {
                out.push(Command::SetAvoidGrowth { player: obs.player, city: city.handle, avoid });
            }
        }
    }
    out
}

/// Percent a filled specialist slot is worth beyond the tile it replaces
/// before it is given up: a city does not move a citizen between a slot and
/// a tile each turn as the prices move by a little.
pub const KEEP_SPECIALIST_PERCENT: i64 = 10;

/// The specialists of an AI that prices its slots (`ScienceLevers::
/// specialists`): in every city with specialist slots, as many of each
/// kind as `specialist_plan` wants, set by hand (`SetSpecialists`; the
/// rules then assign the other citizens by the focus).
pub fn specialist_commands(obs: &AIObservation<'_>, rules: &CompiledRuleset, prices: &Prices) -> Vec<Command> {
    use crate::ai::science_levers as levers;
    let points = levers::point_prices(obs, rules, prices);
    let techs = crate::derived::tile_yields::TechSet::of(&obs.economy.techs);
    let mut out = Vec::new();
    for city in &obs.own_cities {
        let slots = levers::slots(rules, city);
        if slots.iter().all(|n| *n == 0) && city.citizens.specialists.is_empty() {
            continue;
        }
        let wanted = specialist_plan(obs, rules, prices, &points, &techs, city, &slots);
        for (kind, free) in slots.iter().enumerate() {
            let specialist = crate::rules::content::SpecialistId(kind as u16);
            let count = wanted[kind].min(*free).min(u16::from(crate::world::citizens::MAX_FORCED_SPECIALISTS)) as u8;
            if city.citizens.forced(specialist) != count {
                out.push(Command::SetSpecialists { player: obs.player, city: city.handle, specialist, count });
            }
        }
    }
    out
}

/// `science-flow`: the science specialists of every city at `net2`'s level (`Net2Plan::science`, `weights::SCIENTISTS`):
/// the share `level` (thousandths) of the city's slots of the specialists that yield science (read from the rules'
/// specialists, none named), set by hand (`SetSpecialists`; the rules assign the other citizens by the focus). The
/// executor only reads the level; it keeps the city's food: a scientist is added only while the food a turn, less the
/// food of the worked tile the rules give up first (the lowest by the focus' weights), stays at `food_floor` or more,
/// and one is taken back a turn while the city makes less than the floor.
pub fn science_specialist_commands(obs: &AIObservation<'_>, rules: &CompiledRuleset, level: crate::ai::strategy_net::Milli, food_floor: i32) -> Vec<Command> {
    use crate::ai::science_levers as levers;
    let default = rules.roles.default_specialist;
    // The kinds that yield science, the most science first (then the lower id).
    let mut kinds: Vec<usize> = rules.specialists.iter().enumerate().filter(|(i, d)| d.yields.science > 0 && Some(crate::rules::content::SpecialistId(*i as u16)) != default).map(|(i, _)| i).collect();
    kinds.sort_by_key(|i| (std::cmp::Reverse(rules.specialists[*i].yields.science), *i));
    if kinds.is_empty() {
        return Vec::new();
    }
    let techs = crate::derived::tile_yields::TechSet::of(&obs.economy.techs);
    let mut out = Vec::new();
    for city in &obs.own_cities {
        let slots = levers::slots(rules, city);
        let free: u32 = kinds.iter().map(|k| u32::from(slots[*k])).sum();
        let now: u32 = kinds.iter().map(|k| u32::from(city.citizens.forced(crate::rules::content::SpecialistId(*k as u16)))).sum();
        if free == 0 && now == 0 {
            continue;
        }
        let target = ((i64::from(level.clamp(0, 1000)) * i64::from(free) + 500) / 1000) as u32;
        let count = if city.income.food_surplus < food_floor {
            now.saturating_sub(1).min(target)
        } else if target <= now {
            target
        } else {
            // The worked tiles the rules give up first, lowest score by the focus' weights.
            let w = kernel::derived::citizens::weights(city.citizens.focus);
            let mut worked: Vec<(i32, i32)> = city
                .worked
                .iter()
                .filter(|t| **t != city.tile)
                .map(|t| {
                    let y = crate::derived::tile_yields::plot_yields(&obs.view, rules, *t, &techs).at_least(0);
                    (y.food * w[0] + y.production * w[1] + y.gold * w[2] + y.science * w[3] + y.culture * w[4] + y.module * w[5], y.food)
                })
                .collect();
            worked.sort_unstable();
            let (mut food, mut count) = (city.income.food_surplus, now);
            for (_, tile_food) in worked {
                if count >= target || food - tile_food < food_floor {
                    break;
                }
                food -= tile_food;
                count += 1;
            }
            count
        };
        // Filled the most science first, each kind up to its slots.
        let mut left = count;
        for k in &kinds {
            let specialist = crate::rules::content::SpecialistId(*k as u16);
            let n = left.min(u32::from(slots[*k])).min(u32::from(crate::world::citizens::MAX_FORCED_SPECIALISTS));
            left -= n;
            if city.citizens.forced(specialist) != n as u8 {
                out.push(Command::SetSpecialists { player: obs.player, city: city.handle, specialist, count: n as u8 });
            }
        }
    }
    out
}

/// How many specialists of each kind `city` should have (index =
/// `SpecialistId`): its citizens on the best of its tiles and its slots,
/// each at the prices of the turn (a tile its yields, a specialist its
/// yields and the great person points it makes, `science_levers::
/// specialist_worth`; a filled slot keeps `KEEP_SPECIALIST_PERCENT` more),
/// so a specialist takes a citizen off a tile only when it pays more; while
/// the city would eat more food than it makes, the specialist worth least
/// gives its citizen back to the tile with the most food.
pub fn specialist_plan(
    obs: &AIObservation<'_>,
    rules: &CompiledRuleset,
    prices: &Prices,
    points: &[crate::ai::value::Centi; crate::classic::content::GREAT_PERSON_KINDS],
    techs: &crate::derived::tile_yields::TechSet,
    city: &OwnCity,
    slots: &[u16; crate::world::city::MAX_SPECIALIST_KINDS],
) -> [u16; crate::world::city::MAX_SPECIALIST_KINDS] {
    use crate::ai::science_levers as levers;
    use crate::ai::value::Centi;
    /// One place a citizen may take: a tile (its food) or a slot.
    #[derive(Clone, Copy)]
    struct Place {
        value: Centi,
        food: i32,
        slot: Option<usize>,
    }
    let mut places: Vec<Place> = Vec::new();
    let mut worked_food = 0;
    for tile in crate::ai::territory::territory(&obs.view, city.tile, crate::world::city::CITY_WORK_RADIUS, obs.player) {
        let elsewhere = obs.own_cities.iter().any(|c| c.handle != city.handle && c.worked.contains(&tile));
        if tile == city.tile || elsewhere || crate::world::facts::TileFacts::owner(&obs.view, tile) != Some(obs.player) {
            continue;
        }
        let yields = crate::derived::tile_yields::plot_yields(&obs.view, rules, tile, techs).at_least(0);
        if city.worked.contains(&tile) {
            worked_food += yields.food;
        }
        places.push(Place { value: levers::yields_worth(prices, &yields), food: yields.food, slot: None });
    }
    let default = rules.roles.default_specialist;
    for (kind, free) in slots.iter().enumerate() {
        let specialist = crate::rules::content::SpecialistId(kind as u16);
        if *free == 0 || Some(specialist) == default {
            continue;
        }
        let worth = levers::specialist_worth(obs, rules, prices, points, city, specialist);
        let kept = u16::from(city.citizens.forced(specialist));
        for n in 0..*free {
            let value = if n < kept { worth * (100 + KEEP_SPECIALIST_PERCENT) / 100 } else { worth };
            places.push(Place { value, food: 0, slot: Some(kind) });
        }
    }
    // Best first; a tile before a slot of the same worth.
    places.sort_by_key(|p| (std::cmp::Reverse(p.value), p.slot.is_some()));
    let citizens = (city.population as usize).min(places.len());
    let (mut taken, mut left) = (places[..citizens].to_vec(), places[citizens..].to_vec());
    let food = |taken: &[Place]| city.income.food_surplus + taken.iter().map(|p| p.food).sum::<i32>() - worked_food;
    while food(&taken) < 0 {
        let Some(at) = taken.iter().rposition(|p| p.slot.is_some()) else { break };
        let Some(tile) = left.iter().enumerate().filter(|(_, p)| p.slot.is_none()).max_by_key(|(i, p)| (p.food, p.value, std::cmp::Reverse(*i))).map(|(i, _)| i) else { break };
        taken[at] = left.remove(tile);
    }
    let mut wanted = [0u16; crate::world::city::MAX_SPECIALIST_KINDS];
    for slot in taken.iter().filter_map(|p| p.slot) {
        wanted[slot] += 1;
    }
    wanted
}

/// Whether a city of the game's AI should avoid growth at the empire's
/// excess happiness `excess` (`GrowthPolicy::STANDARD`).
pub fn avoid_growth(excess: i32, avoiding: bool) -> bool {
    GrowthPolicy::STANDARD.avoid(excess, avoiding)
}

/// Excess happiness at which the cities stop growing.
pub const AVOID_GROWTH_AT: i32 = -7;
/// Excess happiness at which they grow again.
pub const RESUME_GROWTH_AT: i32 = -5;

/// Where a variant's cities stop growing and grow again (excess happiness;
/// `Variant::growth_policy`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GrowthPolicy {
    pub avoid_at: i32,
    pub resume_at: i32,
}

impl GrowthPolicy {
    /// The game's AI: the cities grow while the empire is unhappy (the
    /// rules slow the growth by 75 % from −1 and stop it below −10, where
    /// production falls by half) and stop only near that line, at −7; they
    /// grow again at −5. A citizen is a beaker and more: measured against
    /// `CAUTIOUS`, 60 % of the points (classic/docs/ai-growth.md).
    pub const STANDARD: GrowthPolicy = GrowthPolicy { avoid_at: AVOID_GROWTH_AT, resume_at: RESUME_GROWTH_AT };
    /// The old line: stop at −2, grow again at +2, so from round ~100 every
    /// city of an expanding empire stood at 2–3 citizens
    /// (`Variant::CautiousGrowth`, the control).
    pub const CAUTIOUS: GrowthPolicy = GrowthPolicy { avoid_at: -2, resume_at: 2 };

    /// Whether a city should avoid growth at `excess`, kept until the excess
    /// climbs back to `resume_at` so a city does not flip every turn.
    pub fn avoid(self, excess: i32, avoiding: bool) -> bool {
        if avoiding {
            excess < self.resume_at
        } else {
            excess <= self.avoid_at
        }
    }
}

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
