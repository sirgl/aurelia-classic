//! The science levers of the rules priced by what they do for the player
//! (classic/docs/ai-science-levers.md): a policy by all its effects, a
//! specialist slot by its yields and the great person its points bring, a
//! research agreement by the beakers the rules pay for it. One measure, the
//! allocator's (`ai::value::Prices`: hundredths of a score point); no
//! policy, specialist or great person is named, everything is read from the
//! rules' data.
//!
//! Assumptions the prices rely on (each one a plan can re-check):
//! - a great person point a turn kept to the last round brings
//!   `rounds_left / threshold` great people at the threshold the player
//!   needs now, born evenly over the rounds left, so each is worth half of
//!   what it brings now (`Prices::later` averaged over the horizon);
//! - a partner's science is ours times its public score over ours (the
//!   victory screen, `PlayerQueries::race`): the only public measure of it;
//! - the specialists a city will have are its slots when the player fills
//!   the slots that pay (`ScienceLevers::specialists`), else the citizens
//!   that work no tile now.

use crate::ai::value::{Centi, Prices};
use crate::classic::content::{GreatPersonKind, GREAT_PERSON_KINDS};
use crate::classic::effects::ClassicTrigger;
use crate::ids::{PlayerId, UnitKindId};
use crate::knowledge::observation::{AIObservation, OwnCity};
use crate::knowledge::public::PlayerQueries;
use crate::rules::content::{PointsSource, SpecialistId, Yields};
use crate::rules::effects::{CompiledEffect, Condition, Modifier, Trigger};
use crate::rules::stats::{ClassicStat, Stat};
use crate::rules::CompiledRuleset;
use crate::world::city::MAX_SPECIALIST_KINDS;
use crate::world::player::PlayerKind;

/// Which science levers a variant prices by what they do
/// (`Variant::science_levers`, `TurnBrief::levers`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ScienceLevers {
    /// Policies, branch openers and tenets by all their effects, a new
    /// branch and a policy of a started one on one scale (`ai::policies`).
    pub policies: bool,
    /// Specialist slots filled while a specialist pays more than the tile
    /// it takes a citizen from (`ai::citizens::specialist_commands`).
    pub specialists: bool,
    /// Research agreements by the beakers the rules pay against their gold
    /// (`ai::diplomacy`).
    pub agreements: bool,
}

impl ScienceLevers {
    pub const NONE: ScienceLevers = ScienceLevers { policies: false, specialists: false, agreements: false };
    pub const ALL: ScienceLevers = ScienceLevers { policies: true, specialists: true, agreements: true };
}

/// The great person kinds in index order (`GreatPersonKind::index`).
const KINDS: [GreatPersonKind; GREAT_PERSON_KINDS] = [
    GreatPersonKind::Artist,
    GreatPersonKind::Engineer,
    GreatPersonKind::Merchant,
    GreatPersonKind::Scientist,
    GreatPersonKind::Writer,
    GreatPersonKind::Musician,
    GreatPersonKind::General,
    GreatPersonKind::Admiral,
    GreatPersonKind::Prophet,
];

/// A grant of the rules priced once as the allocator prices a building's
/// grant on completion (`Prices::effect_in`): the same effect fired on the
/// city `at`.
pub fn grant_once(obs: &AIObservation<'_>, rules: &CompiledRuleset, prices: &Prices, at: &OwnCity, effect: &CompiledEffect) -> Centi {
    let fired = CompiledEffect { trigger: Trigger::OnBuilt, ..*effect };
    prices.effect_in(obs, rules, at, &fired, obs.own_cities.len() as i32)
}

/// What one great person of the unit `kind` brings once by its action
/// (`UnitAbilities`), as the allocator prices a free great person
/// (`Prices::great_person`, here for a great person born in the capital):
/// a scientist the empire's science of its turns, an engineer its hurry, a
/// merchant its trade mission's gold in this era, an artist its golden
/// age; the best of them. None: no such action (its points are then worth
/// nothing here).
pub fn great_person_worth(obs: &AIObservation<'_>, rules: &CompiledRuleset, prices: &Prices, kind: UnitKindId) -> Option<Centi> {
    use crate::classic::yields::GOLDEN_AGE_PRODUCTION_PERCENT;
    let a = &rules.units.get(kind.0 as usize)?.module.economy;
    let city = capital(obs)?;
    let t = i64::from(prices.rounds_left.max(1));
    let science = i64::from(obs.economy.income.science.max(0)) * i64::from(a.beakers_turns) * prices.science / t;
    let hurry = (i64::from(a.base_hurry) + i64::from(a.hurry_multiplier) * i64::from(city.population)) * prices.production / t;
    let gold = if a.base_gold > 0 { (i64::from(a.base_gold) + i64::from(a.gold_per_era) * i64::from(obs.view.own_era())) * prices.gold / t } else { 0 };
    let golden = if a.golden_age_turns > 0 {
        let production: i64 = obs.own_cities.iter().map(|c| i64::from(c.income.production.max(0))).sum();
        production * i64::from(GOLDEN_AGE_PRODUCTION_PERCENT) / 100 * i64::from(a.golden_age_turns) * prices.production / t
    } else {
        0
    };
    let best = science.max(hurry).max(gold).max(golden);
    (best > 0).then_some(best)
}

/// The price of one great person point of `kind` a turn kept to the last
/// round: the great people `rounds_left` points bring at the threshold the
/// player needs now, each worth half of its worth now (born evenly over the
/// rounds left).
pub fn point_price(obs: &AIObservation<'_>, rules: &CompiledRuleset, prices: &Prices, kind: GreatPersonKind) -> Centi {
    let Some(def) = rules.module.great_people.iter().find(|g| g.kind == kind) else { return 0 };
    let Some(worth) = great_person_worth(obs, rules, prices, def.unit) else { return 0 };
    let threshold = i64::from(obs.economy.great_person_thresholds[kind.index()].max(1));
    worth * i64::from(prices.rounds_left) / (2 * threshold)
}

/// The capital, else the first city.
pub fn capital<'o>(obs: &'o AIObservation<'_>) -> Option<&'o OwnCity> {
    let handle = obs.view.own_capital();
    obs.own_cities.iter().find(|c| Some(c.handle) == handle).or_else(|| obs.own_cities.first())
}

/// Yields at the turn's prices, a turn kept to the last round.
pub fn yields_worth(prices: &Prices, y: &Yields) -> Centi {
    i64::from(y.food) * prices.food
        + i64::from(y.production) * prices.production
        + i64::from(y.gold) * prices.gold
        + i64::from(y.science) * prices.science
        + i64::from(y.culture) * prices.culture
        + i64::from(y.module) * prices.faith
}

/// The passive effects that reach `city` from the player's adopted
/// policies and the city's own buildings, whose condition always holds.
fn effects_on<'r>(obs: &'r AIObservation<'_>, rules: &'r CompiledRuleset, city: &'r OwnCity) -> impl Iterator<Item = &'r CompiledEffect> + 'r {
    let policies = obs.culture.policies.iter().filter_map(|p| rules.module.policies.get(p.0 as usize)).map(|d| &d.effects);
    let buildings = city.buildings.iter().filter_map(|b| rules.buildings.get(b.0 as usize)).map(|d| &d.effects);
    policies
        .chain(buildings)
        .flat_map(|set| set.passive_player.iter().chain(&set.passive_city))
        .filter(|e| matches!(e.condition, Condition::Always) && matches!(e.second_condition, Condition::Always))
}

/// The great person rate percent `city` has for `kind` (its policies'
/// and buildings' `GreatPeopleRate` and `GreatPersonRate`).
pub fn rate_percent(obs: &AIObservation<'_>, rules: &CompiledRuleset, city: &OwnCity, kind: GreatPersonKind) -> i32 {
    effects_on(obs, rules, city)
        .map(|e| match e.modifier {
            Modifier::AddStat { stat: Stat::Module(ClassicStat::GreatPeopleRate), amount } => amount,
            Modifier::AddStat { stat: Stat::Module(ClassicStat::GreatPersonRate(k)), amount } if k == kind => amount,
            _ => 0,
        })
        .sum()
}

/// What one specialist of `kind` yields in `city`: its own yields and the
/// `SpecialistYield` stats of the city's policies and buildings.
pub fn specialist_yields(obs: &AIObservation<'_>, rules: &CompiledRuleset, city: &OwnCity, kind: SpecialistId) -> Yields {
    let Some(def) = rules.specialists.get(kind.0 as usize) else { return Yields::default() };
    let mut yields = def.yields;
    let default = rules.roles.default_specialist;
    for e in effects_on(obs, rules, city) {
        if let Modifier::AddStat { stat: Stat::SpecialistYield { specialist, yield_kind }, amount } = e.modifier {
            let applies = match specialist {
                Some(s) => s == kind,
                None => Some(kind) != default,
            };
            if applies {
                yields.add(yield_kind, amount);
            }
        }
    }
    yields
}

/// The great person a specialist of `kind` makes points toward, and its
/// points a turn.
pub fn specialist_points(rules: &CompiledRuleset, kind: SpecialistId) -> Option<(GreatPersonKind, i64)> {
    let def = rules.specialists.get(kind.0 as usize)?;
    let points = i64::from(def.module.great_person_points);
    let person = rules.module.great_people.iter().find(|g| g.points_source == PointsSource::Specialists && g.specialist == Some(kind))?;
    (points > 0).then_some((person.kind, points))
}

/// Point prices of every great person kind this turn (index =
/// `GreatPersonKind::index`).
pub fn point_prices(obs: &AIObservation<'_>, rules: &CompiledRuleset, prices: &Prices) -> [Centi; GREAT_PERSON_KINDS] {
    KINDS.map(|k| point_price(obs, rules, prices, k))
}

/// What one specialist of `kind` in `city` is worth a turn kept to the
/// last round: its yields at the prices, its points at the point price of
/// its great person with the city's rate.
pub fn specialist_worth(obs: &AIObservation<'_>, rules: &CompiledRuleset, prices: &Prices, points: &[Centi; GREAT_PERSON_KINDS], city: &OwnCity, kind: SpecialistId) -> Centi {
    let yields = yields_worth(prices, &specialist_yields(obs, rules, city, kind));
    let great = specialist_points(rules, kind).map_or(0, |(person, p)| {
        let rate = i64::from(100 + rate_percent(obs, rules, city, person)).max(0);
        p * rate / 100 * points[person.index()]
    });
    yields + great
}

/// Free specialist slots of `city` per kind (index = `SpecialistId`), from
/// its buildings (`derived::citizens::specialist_slots`).
pub fn slots(rules: &CompiledRuleset, city: &OwnCity) -> [u16; MAX_SPECIALIST_KINDS] {
    let mut slots = [0u16; MAX_SPECIALIST_KINDS];
    for def in city.buildings.iter().filter_map(|b| rules.buildings.get(b.0 as usize)) {
        if let Some((kind, count)) = def.specialist_slots {
            if let Some(slot) = slots.get_mut(kind.0 as usize) {
                *slot = slot.saturating_add(count);
            }
        }
    }
    slots
}

/// Citizens of `city` that work no tile now (the specialists, the
/// unemployed included).
pub fn idle_citizens(city: &OwnCity) -> u32 {
    city.population.saturating_sub(city.worked.len().saturating_sub(1) as u32)
}

/// The research agreement percent the player has (`ResearchAgreementPercent`
/// of its policies and buildings, over the rules' base).
pub fn agreement_percent(obs: &AIObservation<'_>, rules: &CompiledRuleset) -> i64 {
    use crate::systems::diplomacy::MEDIAN_TECH_PERCENT;
    let policies = obs.culture.policies.iter().filter_map(|p| rules.module.policies.get(p.0 as usize)).map(|d| &d.effects);
    let buildings = obs.own_cities.iter().flat_map(|c| &c.buildings).filter_map(|b| rules.buildings.get(b.0 as usize)).map(|d| &d.effects);
    let extra: i32 = policies
        .chain(buildings)
        .flat_map(|s| &s.passive_player)
        .map(|e| match e.modifier {
            Modifier::AddStat { stat: Stat::Module(ClassicStat::ResearchAgreementPercent), amount } => amount,
            _ => 0,
        })
        .sum();
    (i64::from(MEDIAN_TECH_PERCENT) + i64::from(extra)).max(0)
}

/// A partner's science a turn as the player can tell it: ours times its
/// public score over ours.
pub fn partner_science(obs: &AIObservation<'_>, scores: &[(PlayerId, i64)], other: PlayerId) -> i64 {
    let ours = i64::from(obs.economy.income.science.max(0));
    let own = scores.first().map_or(0, |(_, s)| *s).max(1);
    let theirs = scores.iter().find(|(p, _)| *p == other).map_or(own, |(_, s)| *s).max(0);
    ours * theirs / own
}

/// Beakers a turn the pool of an agreement with `other` adds to the player
/// at `percent` (the rules: the smaller of the two sciences, over the pool
/// divisor, at the percent).
pub fn agreement_beakers_per_turn(obs: &AIObservation<'_>, scores: &[(PlayerId, i64)], other: PlayerId, percent: i64) -> i64 {
    use crate::systems::diplomacy::RESEARCH_AGREEMENT_POOL_DIVISOR;
    let ours = i64::from(obs.economy.income.science.max(0));
    ours.min(partner_science(obs, scores, other)) * percent / (100 * i64::from(RESEARCH_AGREEMENT_POOL_DIVISOR))
}

/// What an agreement with `other` signed now is worth in gold: the
/// beakers the rules pay at its end (`DEAL_DURATION` rounds of the pool;
/// nothing when the game ends first), at the beaker's price from then on
/// (`Prices::later`) over the gold's, less the gold the rules take.
pub fn agreement_gold(obs: &AIObservation<'_>, rules: &CompiledRuleset, prices: &Prices, other: PlayerId, cost: u32) -> i64 {
    // The game speed's deal (`DEAL_DURATION` at Standard).
    let rounds = obs.speed.deal_rounds;
    if prices.rounds_left <= rounds {
        return -i64::from(cost);
    }
    let scores = obs.view.race().scores;
    let beakers = agreement_beakers_per_turn(obs, &scores, other, agreement_percent(obs, rules)) * i64::from(rounds);
    prices.later(beakers * prices.science, rounds) / prices.gold.max(1) - i64::from(cost)
}

/// The nations the player has or may soon have an agreement with: its
/// friends and its partners in an agreement.
pub fn agreement_partners<'o>(obs: &'o AIObservation<'_>) -> impl Iterator<Item = PlayerId> + 'o {
    obs.relations.iter().filter(|r| r.kind == PlayerKind::Major && !r.at_war && (r.friends || r.research_agreement)).map(|r| r.other)
}

/// Whether an effect fires when the player adopts its source.
pub fn on_adopt(effect: &CompiledEffect) -> bool {
    matches!(effect.trigger, Trigger::Module(ClassicTrigger::OnAdopt))
}
