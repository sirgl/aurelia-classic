//! `tall-real` (`Variant::TallReal`, `TurnBrief::tall_real`): the tall style of `sharp` made a way
//! of playing that reaches the science victory. Nothing here runs for another variant.
//!
//! A seat plays it when its strategy net holds the tall style and the science victory fits the
//! game's length (`science_game`: no economy has won by science before three quarters of the
//! speed's game, so a 198-round bench game at Quick plays `sharp`). Then:
//!
//! - **The cities follow from the rules' data** (`SizeModel`): one more city raises the cost of
//!   every technology left (`world_size::tech_cost_per_city` a non-puppet city) and of every policy
//!   (`world_size::policy_cost_per_city`, weighed by the share of the science the science branch's
//!   percents make); it brings the beakers its citizens make until the science victory or the end
//!   (the rules' growth thresholds from size 1 at the empire's food surplus, the beakers of one
//!   citizen until it has built the science buildings the capital has, the empire's beakers a
//!   citizen after), and it must leave the empire its comfort (the science branch's opener works while
//!   happy; growth stalls while unhappy). The k-th new city is founded only when its beakers are a
//!   larger share of the empire's than the cost it adds to the research left. The target is a
//!   count of cities, not "the cities now and a few more": it binds (`adjust` refuses a settler's
//!   founding beyond it; the allocator orders no settler past it through `Prices::city_cap`).
//!   Captured cities stay puppets (the game's `Conquest::PuppetAll`): a puppet raises no cost
//!   (`derived::culture::effective_cities`), so it counts beside the target.
//! - **Its aim is the science victory** and its policies, buildings and spaceship follow it: the
//!   strategy net's science aim (`strategy_net::think`), the science branch whole before any other
//!   choice once open (`strategy_net::heads::choose_policy`), the culture waiting for it once the
//!   first branch is open (`heads::science_ahead`, the book's measured wait), the science buildings
//!   ordered first in every city by beakers per hammer (`tall::BuildingOrder::Science`), and the
//!   spaceship's items by the space plan with its parts walking around the units in the way
//!   (`victory::space_plan`, `TurnBrief::space_finish`; parts built and not installed by the end
//!   lost victories to `standard`'s single march order).
//!
//! Integer arithmetic, deterministic; only the player's own observation and the rules are read.

use crate::ai::planner::{Decision, TurnBrief};
use crate::ai::trace::Reason;
use crate::ids::TechId;
use crate::knowledge::observation::{AIObservation, OwnCity};
use crate::rules::effects::{Modifier, YieldKind};
use crate::rules::CompiledRuleset;
use crate::world::city::CityStatus;
use crate::world::victory::VictoryKind;

/// No economy has won by science before this share of the speed's game (percent): the earliest
/// victory of `tall-book-2`, the book that wins by science, came at round 249 of 330 at Quick and
/// 405 of 500 at Standard (classic/docs/ai-science-audit.md; `victory-price` uses the same bound).
pub const EARLIEST_VICTORY_PERCENT: i64 = 75;

/// The science a turn grows as the round to this power: the seat-mean science of `standard` on the
/// pace bench grew ×29 over ×5 rounds, a power of 2.1 (`victory-price`'s measure, reach/LOG.md).
pub const SCIENCE_GROWTH_POWER: u32 = 2;

/// Excess happiness the empire keeps after a new city and its first citizen: the allocator's
/// comfort (`value::HAPPINESS_COMFORT`, below which growth stalls soon), so that the cities it has
/// keep the headroom to grow; the science branch's opener adds its percent only while the empire
/// is not unhappy (`SocietyStat::ScienceWhileHappyPercent`). With a floor of 0 the tuning traces'
/// tall seats founded on every spike of the excess (r152–179 of pangaea 200007: 7 → 12 cities at
/// excess 9–11, back to 1 after each city).
pub const CONTENT_FLOOR: i64 = crate::ai::value::HAPPINESS_COMFORT as i64;

/// At most this many new cities are looked at in a turn (a bound of the loop: the model's share
/// test stops every seat of the tuning traces well before it).
pub const MAX_MORE: u32 = 12;

/// The science victory fits the game: it is on, the rules have its parts, and the game lasts at
/// least `EARLIEST_VICTORY_PERCENT` of the speed's game.
pub fn science_game(obs: &AIObservation<'_>, rules: &CompiledRuleset) -> bool {
    let speed = i64::from(obs.speed.max_rounds());
    obs.victories.enabled(VictoryKind::Science)
        && !crate::world::victory::spaceship_parts(rules).is_empty()
        && i64::from(obs.victories.max_rounds) * 100 >= speed * EARLIEST_VICTORY_PERCENT
}

/// Whether a seat of `tall-real` plays the real tall style now: the held style is tall and the
/// science victory fits the game; `next` passes `fits`, its own judgment held from the style's
/// lock (`science_fits`; None before the lock: not yet).
pub fn plays(obs: &AIObservation<'_>, rules: &CompiledRuleset, style_pick: usize, fits: Option<Option<bool>>) -> bool {
    style_pick == crate::ai::strategy_net::weights::STYLE_TALL
        && match fits {
            Some(held) => held == Some(true),
            None => science_game(obs, rules),
        }
}

// ---- `next` (`TurnBrief::next`): when the science victory fits, from the rules ----

/// Beakers of `base` (the rules' costs) at the game's costs now: the speed's and the map's
/// research percents and the per-city term of `cities` non-puppet cities.
fn at_game_cost(obs: &AIObservation<'_>, base: i64, cities: u32, research_per_city: i64) -> i64 {
    let size = crate::world::size::WorldSize::of(crate::world::facts::TileFacts::dimensions(&obs.view));
    let world = i64::from(crate::classic::world_size::research_percent(size));
    let speed = i64::from(obs.speed.science_percent);
    base * speed / 100 * world / 100 * (10_000 + research_per_city * (i64::from(cities.max(1)) - 1)) / 10_000
}

/// `next`: the beakers of the technologies the spaceship still needs — those its victory's
/// projects and its parts require, with their prerequisites (`grand_strategy::remaining_beakers`;
/// `victory_beakers` counts every project's technology) — at the game's costs now (no discount
/// for the technologies others know: the cautious bound).
fn victory_need(obs: &AIObservation<'_>, rules: &CompiledRuleset, cities: u32, research_per_city: i64) -> i64 {
    let base = i64::try_from(crate::ai::grand_strategy::remaining_beakers(obs, rules)).unwrap_or(i64::MAX / 1_000_000);
    at_game_cost(obs, base, cities, research_per_city)
}

/// `next`: the science victory fits the seat's game — it is on, the rules have its parts, and
/// the empire's science, growing as it grows on the bench (`SCIENCE_GROWTH_POWER`, `rounds_to`),
/// gathers the beakers of the spaceship's technologies (`victory_need`) with the rounds the ship
/// takes to build (`grand_strategy::SPACESHIP_ROUNDS` at the speed's pace) left before the end.
/// Read from the rules' costs and the empire's science, not from a share of the game's length.
/// It is judged once, at the style's lock (`strategy_net::think` keeps it): the power law from
/// the first rounds' science projects a victory in tens of rounds, and a mid-game projection
/// misses the late growth of the science buildings (the tuning traces' tall seat that won at
/// round 313 was projected past the end from round 60 to 230).
pub fn science_fits(obs: &AIObservation<'_>, rules: &CompiledRuleset, rounds_left: u32) -> bool {
    let ship = obs.speed.pace_rounds(crate::ai::grand_strategy::SPACESHIP_ROUNDS);
    if !obs.victories.enabled(VictoryKind::Science) || crate::world::victory::spaceship_parts(rules).is_empty() || rounds_left <= ship {
        return false;
    }
    let size = crate::world::size::WorldSize::of(crate::world::facts::TileFacts::dimensions(&obs.view));
    let need = victory_need(obs, rules, costed(obs), crate::classic::world_size::tech_cost_per_city(size));
    let rounds = rounds_to(i64::from(obs.economy.income.science), need, obs.round, rounds_left - ship);
    if crate::ai::strategy_net::trace::enabled() {
        crate::ai::strategy_net::trace::push(format!("r{} p{} FIT science {} need {} → {:?} of {} (ship {})", obs.round, obs.player.0, obs.economy.income.science, need, rounds, rounds_left - ship, ship));
    }
    rounds.is_some()
}

/// The cities that raise the research and policy costs: the non-puppet ones.
fn costed(obs: &AIObservation<'_>) -> u32 {
    obs.own_cities.iter().filter(|c| c.status != CityStatus::Puppet).count() as u32
}

/// What one more city costs and brings a tall empire, from the rules' data and the observation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SizeModel {
    /// Rounds the cities' beakers count: to the end, or to the science victory when the empire's
    /// science reaches it sooner (`horizon`).
    pub horizon: u32,
    /// Non-puppet cities now.
    pub cities: u32,
    /// Hundredths of a percent the research and the policy cost grow a non-puppet city.
    pub research_per_city: i64,
    pub policy_per_city: i64,
    /// Thousandths of the empire's science the science branch's percents make (`policy_share`).
    pub policy_share: i64,
    /// Hundredths of a beaker a citizen makes in the empire now, and in a city without science
    /// buildings (the rules' science a citizen).
    pub per_citizen: i64,
    pub bare_citizen: i64,
    /// Rounds a new city takes to build the science buildings the capital has (`lag`).
    pub lag: u32,
    /// Rounds between two new cities: a settler at the capital's production and its walk.
    pub settler_rounds: u32,
    /// Food a turn a new city grows by: the empire's mean surplus, at least 1.
    pub surplus: i64,
    /// Hundredths of beakers the cities make by the horizon (`citizen_rounds` × `per_citizen`).
    pub existing: i64,
    /// Excess happiness now and what a new city with its first citizen costs, hundredths.
    pub happiness: i64,
    pub city_unhappiness: i64,
    /// The speed's growth thresholds (`systems::growth::speed_growth_threshold`).
    pace: kernel::rules::speeds::SpeedValues,
}

/// Citizen-rounds of a city of `population` with `food` stored growing by `surplus` a turn, from
/// round `from` to round `to` (relative), at the speed's thresholds.
fn citizen_rounds(pace: &kernel::rules::speeds::SpeedValues, population: u32, food: i64, surplus: i64, from: u32, to: u32) -> i64 {
    let mut size = population.max(1);
    let mut store = food.max(0);
    let mut total = 0i64;
    for _ in from..to {
        total += i64::from(size);
        store += surplus.max(0);
        let need = i64::from(crate::systems::growth::speed_growth_threshold(pace, size));
        if store >= need {
            store -= need;
            size += 1;
        }
    }
    total
}

/// The share of the empire's science the science branch's percents make (thousandths): the
/// normal branch whose largest character is science (`strategy_net::branches`), its policies'
/// science percents (a city percent, the percent while happy) summed, over 100 and that sum.
pub fn policy_share(rules: &CompiledRuleset) -> i64 {
    use crate::ai::strategy_net::branches::{character, dominant, is_normal};
    use crate::ai::strategy_net::weights::branch::ch;
    let mut best = 0i64;
    for i in 0..rules.module.policy_branches.len() {
        let branch = crate::rules::content::PolicyBranchId(i as u16);
        if !is_normal(rules, branch) || dominant(&character(rules, branch)) != Some(ch::SCIENCE) {
            continue;
        }
        let percent: i64 = rules
            .module
            .policies
            .iter()
            .filter(|p| p.branch == Some(branch))
            .flat_map(|p| crate::ai::strategy_net::cell::effects_of(&p.effects))
            .map(|e| match e.modifier {
                Modifier::AddYieldPercent { yield_kind: YieldKind::Science, percent } => i64::from(percent.max(0)),
                Modifier::AddStat { stat: crate::rules::stats::Stat::Module(crate::rules::stats::ClassicStat::Society(crate::classic::stats::SocietyStat::ScienceWhileHappyPercent)), amount } => i64::from(amount.max(0)),
                _ => 0,
            })
            .sum();
        best = best.max(percent * 1000 / (100 + percent));
    }
    best
}

/// A regular building with a science effect (no wonder, no palace).
fn science_building(rules: &CompiledRuleset, kind: crate::ids::BuildingKindId) -> bool {
    let Some(def) = rules.buildings.get(usize::from(kind.0)) else { return false };
    !def.wonder
        && def.kind == crate::rules::content::BuildingKind::Regular
        && def.effects.passive_city.iter().any(|e| {
            matches!(
                e.modifier,
                Modifier::AddYield { yield_kind: YieldKind::Science, .. } | Modifier::AddYieldPercent { yield_kind: YieldKind::Science, .. } | Modifier::AddYieldPerPopulation { yield_kind: YieldKind::Science, .. }
            )
        })
}

/// The capital: the city with the palace, else the first.
fn capital<'o>(obs: &'o AIObservation<'_>, rules: &CompiledRuleset) -> Option<&'o OwnCity> {
    let palace = |c: &&OwnCity| c.buildings.iter().any(|b| rules.buildings.get(usize::from(b.0)).is_some_and(|d| d.kind == crate::rules::content::BuildingKind::Palace));
    obs.own_cities.iter().find(palace).or(obs.own_cities.first())
}

/// The rounds a science growing as the round to `SCIENCE_GROWTH_POWER` from `rate` at `round`
/// takes to gather `need`; None within `cap`.
fn rounds_to(rate: i64, need: i64, round: u32, cap: u32) -> Option<u32> {
    let r = i128::from(round.max(1));
    let rate = i128::from(rate.max(1));
    let mut gathered: i128 = 0;
    for d in 1..=cap {
        gathered += rate * (r + i128::from(d)).pow(SCIENCE_GROWTH_POWER) / r.pow(SCIENCE_GROWTH_POWER);
        if gathered >= i128::from(need) {
            return Some(d);
        }
    }
    None
}

/// The beakers of the science victory's technologies still unknown, at the empire's research cost
/// now (the speed's and the map's percents and the per-city term; no discount for known ones).
fn victory_beakers(obs: &AIObservation<'_>, rules: &CompiledRuleset, cities: u32, research_per_city: i64) -> i64 {
    let mut planned: Vec<TechId> = Vec::new();
    for t in crate::ai::strategy_net::sharp::space_techs(rules) {
        let way = crate::ai::strategy_net::goals::path(rules, &obs.economy.techs, &planned, t);
        planned.extend(way);
    }
    at_game_cost(obs, crate::ai::strategy_net::goals::beakers(rules, &planned, 100), cities, research_per_city)
}

impl SizeModel {
    /// The horizon: the power growth's rounds to the victory's technologies; `tall-real` floors
    /// it at `EARLIEST_VICTORY_PERCENT` of the speed's game and counts every project's
    /// technology, `next` plays only from the lock on (`science_fits`), needs no floor and counts
    /// the spaceship's own (`victory_need`).
    pub fn of(obs: &AIObservation<'_>, rules: &CompiledRuleset, rounds_left: u32, next: bool) -> Self {
        let size = crate::world::size::WorldSize::of(crate::world::facts::TileFacts::dimensions(&obs.view));
        let research_per_city = crate::classic::world_size::tech_cost_per_city(size);
        let policy_per_city = crate::classic::world_size::policy_cost_per_city(size);
        let cities = costed(obs);
        let population: i64 = obs.own_cities.iter().map(|c| i64::from(c.population)).sum();
        let bare_citizen = i64::from(crate::classic::yields::SCIENCE_PER_CITIZEN) * 100;
        let per_citizen = (i64::from(obs.economy.income.science.max(0)) * 100 / population.max(1)).max(bare_citizen);
        let pace = obs.speed;
        // The horizon: the science victory's round at the science growing as it does, if sooner.
        let horizon = if next {
            let need = victory_need(obs, rules, cities, research_per_city);
            rounds_to(i64::from(obs.economy.income.science), need, obs.round, rounds_left).unwrap_or(rounds_left)
        } else {
            let need = victory_beakers(obs, rules, cities, research_per_city);
            // Never before `EARLIEST_VICTORY_PERCENT` of the speed's game: the power growth from a
            // science of a few beakers in the first rounds projects a victory in 40 rounds.
            let earliest = (i64::from(obs.speed.max_rounds()) * EARLIEST_VICTORY_PERCENT / 100 - i64::from(obs.round)).clamp(0, i64::from(rounds_left)) as u32;
            rounds_to(i64::from(obs.economy.income.science), need, obs.round, rounds_left).unwrap_or(rounds_left).clamp(earliest, rounds_left)
        };
        let n = obs.own_cities.len().max(1) as i64;
        let surplus = (obs.own_cities.iter().map(|c| i64::from(c.income.food_surplus.max(0))).sum::<i64>() / n).max(1);
        let production = i64::from(obs.speed.production_percent.max(1));
        let capital = capital(obs, rules);
        // The science buildings a new city builds before its citizens make the empire's beakers:
        // those the capital has, at a young city's production (`value::NEW_CITY_PRODUCTION`).
        let hammers: i64 = capital
            .map(|c| c.buildings.iter().filter(|b| science_building(rules, **b)).map(|b| i64::from(rules.buildings[usize::from(b.0)].cost)).sum::<i64>())
            .unwrap_or(0)
            * production
            / 100;
        let lag = (hammers / crate::ai::value::NEW_CITY_PRODUCTION.max(1)).min(i64::from(u32::MAX)) as u32;
        // A settler at the capital's production, and its walk (`allocator::SETTLE_WALK_ROUNDS`).
        let settler = rules.units.iter().filter(|u| u.founder && u.cost > 0).map(|u| i64::from(u.cost)).min().unwrap_or(0) * production / 100;
        let at_capital = capital.map_or(1, |c| i64::from(c.income.production.max(1)));
        let settler_rounds = ((settler + at_capital - 1) / at_capital + i64::from(crate::ai::allocator::SETTLE_WALK_ROUNDS)).clamp(1, i64::from(u32::MAX)) as u32;
        let existing: i64 = obs
            .own_cities
            .iter()
            .filter(|c| c.status != CityStatus::Puppet)
            .map(|c| citizen_rounds(&pace, c.population, i64::from(c.food), i64::from(c.income.food_surplus), 0, horizon))
            .sum::<i64>()
            * per_citizen;
        let rates = crate::ai::value::Unhappiness::of(obs);
        Self {
            horizon,
            cities,
            research_per_city,
            policy_per_city,
            policy_share: policy_share(rules),
            per_citizen,
            bare_citizen,
            lag,
            settler_rounds,
            surplus,
            existing,
            happiness: i64::from(obs.economy.turn_happiness) * 100,
            city_unhappiness: rates.city + rates.citizen,
            pace,
        }
    }

    /// Hundredths of beakers the `k`-th new city (1: the next) makes by the horizon: founded
    /// `k × settler_rounds` from now at size 1, a citizen's bare beakers for `lag` rounds, the
    /// empire's after.
    pub fn new_city(&self, k: u32) -> i64 {
        let founded = k.saturating_mul(self.settler_rounds);
        if founded >= self.horizon {
            return 0;
        }
        let built = founded.saturating_add(self.lag).min(self.horizon);
        let early = citizen_rounds(&self.pace, 1, 0, self.surplus, founded, built);
        let all = citizen_rounds(&self.pace, 1, 0, self.surplus, founded, self.horizon);
        early * self.bare_citizen + (all - early) * self.per_citizen
    }

    /// Whether the `k`-th new city pays: its beakers over the empire's (with the new cities before
    /// it) at least the research cost it adds over the cost then, plus the policy cost it adds
    /// weighed by the science branch's share; and the empire stays content.
    pub fn pays(&self, k: u32, before: i64) -> bool {
        if self.happiness - i64::from(k) * self.city_unhappiness < CONTENT_FLOOR * 100 {
            return false;
        }
        let m = i64::from(self.cities) + i64::from(k) - 1;
        let research = 10_000 + self.research_per_city * (m - 1).max(0);
        let policy = 10_000 + self.policy_per_city * (m - 1).max(0);
        let gain = i128::from(self.new_city(k));
        let empire = i128::from(before.max(1));
        // gain / empire ≥ r / R + share × p / P, times R × P × 1000.
        gain * i128::from(research) * i128::from(policy) * 1000
            >= empire * (i128::from(self.research_per_city) * i128::from(policy) * 1000 + i128::from(self.policy_share) * i128::from(self.policy_per_city) * i128::from(research))
    }

    /// How many new cities pay, the next first (each over the empire with those before it).
    pub fn more(&self) -> u32 {
        let mut empire = self.existing;
        let mut more = 0;
        for k in 1..=MAX_MORE {
            if !self.pays(k, empire) {
                break;
            }
            empire += self.new_city(k);
            more = k;
        }
        more
    }
}

/// The cities a real tall seat keeps (its puppets beside them): the cities now and the new ones
/// that pay.
pub fn city_target(obs: &AIObservation<'_>, rules: &CompiledRuleset, rounds_left: u32, next: bool) -> u32 {
    let model = SizeModel::of(obs, rules, rounds_left, next);
    let more = model.more();
    if crate::ai::strategy_net::trace::enabled() {
        crate::ai::strategy_net::trace::push(format!(
            "r{} p{} TALLR cities {} (costed {}) more {} | horizon {} lag {} settler {} surplus {} happy {} unhappy/city {} | per_citizen {} existing {} next {} | costs research {} policy {} share {}",
            obs.round,
            obs.player.0,
            obs.own_cities.len(),
            model.cities,
            more,
            model.horizon,
            model.lag,
            model.settler_rounds,
            model.surplus,
            model.happiness / 100,
            model.city_unhappiness,
            model.per_citizen,
            model.existing / 100,
            model.new_city(1) / 100,
            model.research_per_city,
            model.policy_per_city,
            model.policy_share,
        ));
    }
    obs.own_cities.len() as u32 + more
}

/// A settler of a real tall seat founds no city beyond its target (`Plan::city_target`).
pub fn adjust(decision: Decision, brief: &TurnBrief) -> Decision {
    let Some(plan) = brief.strategy.as_deref().filter(|p| p.intent.tall_real) else { return decision };
    if decision.reason == Reason::FoundCity && !brief.cities.is_empty() && brief.cities.len() as u32 >= plan.city_target {
        return Decision::refuse(decision.actor, Reason::Idle);
    }
    decision
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model() -> SizeModel {
        SizeModel {
            horizon: 300,
            cities: 2,
            research_per_city: 500,
            policy_per_city: 1000,
            policy_share: 212,
            per_citizen: 200,
            bare_citizen: 100,
            lag: 0,
            settler_rounds: 15,
            surplus: 3,
            existing: 0,
            happiness: 1000,
            city_unhappiness: 400,
            pace: kernel::rules::speeds::SpeedValues::STANDARD,
        }
    }

    /// `next`'s fit at the lock (`science_fits`): the power growth from the lock's science
    /// gathers the classic spaceship's technologies (112 734 beakers at Quick on a small map, the
    /// tuning traces' need at round 15) before a 330-round game's end with the ship's rounds to
    /// spare, not before a 198-round game's; a faster start fits sooner.
    #[test]
    fn the_lock_judges_the_game_by_its_length_and_the_science() {
        let ship = 16;
        let need = 112_734;
        assert!(rounds_to(6, need, 15, 330 - 15 - ship).is_some());
        assert!(rounds_to(6, need, 15, 198 - 15 - ship).is_none());
        assert!(rounds_to(9, need, 15, 330) < rounds_to(6, need, 15, 330));
    }

    /// A young city grows by the rules' thresholds: more citizen-rounds the longer it lives and
    /// the more food it has.
    #[test]
    fn a_city_grows_by_the_thresholds() {
        let pace = kernel::rules::speeds::SpeedValues::STANDARD;
        assert_eq!(citizen_rounds(&pace, 1, 0, 0, 0, 10), 10);
        assert!(citizen_rounds(&pace, 1, 0, 3, 0, 100) > citizen_rounds(&pace, 1, 0, 2, 0, 100));
        assert!(citizen_rounds(&pace, 1, 0, 3, 0, 200) > 2 * citizen_rounds(&pace, 1, 0, 3, 0, 100));
        assert_eq!(citizen_rounds(&pace, 4, 0, 3, 50, 50), 0);
    }

    /// Early, against a small empire, a city pays; late, against a large one, it does not: the
    /// cost it adds to every technology left outweighs a city that has no time to grow.
    #[test]
    fn the_next_city_pays_early_and_not_late() {
        let early = model();
        let empire = 2 * citizen_rounds(&early.pace, 3, 0, 3, 0, 300) * early.per_citizen;
        assert!(early.pays(1, empire));
        let late = SizeModel { horizon: 120, cities: 6, lag: 60, ..model() };
        let empire = 6 * citizen_rounds(&late.pace, 12, 0, 3, 0, 120) * late.per_citizen;
        assert!(!late.pays(1, empire));
    }

    /// The science buildings a new city must first build delay its pay; happiness bounds the
    /// cities; the count is bounded.
    #[test]
    fn the_lag_and_the_mood_bound_the_cities() {
        let m = model();
        let slow = SizeModel { lag: 120, ..model() };
        assert!(slow.new_city(1) < m.new_city(1));
        let unhappy = SizeModel { happiness: 300, ..model() };
        assert!(!unhappy.pays(1, 1));
        let empire = 2 * citizen_rounds(&m.pace, 3, 0, 3, 0, 300) * m.per_citizen;
        let free = SizeModel { existing: empire, happiness: 100_000, ..model() };
        let more = free.more();
        assert!(more >= 1 && more < MAX_MORE, "{more}");
        let capped = SizeModel { happiness: 1000, ..free.clone() };
        assert!(capped.more() <= 2, "{}", capped.more());
    }
}
