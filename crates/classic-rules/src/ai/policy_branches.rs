//! Which policy branch the AI unlocks next (classic/docs/ai-policies.md).
//!
//! No branch or policy is named here. Every branch is valued by what its
//! policies declare (`PolicyDefinition::effects`): each effect is sorted by
//! its kind into an `Aim` (growth, expansion, happiness, a yield, war, ...),
//! sized by its amount, its reach (the capital or the player once, every
//! city once per city) and its tempo (a policy that needs others adopted
//! first, or a building not yet built, pays later), and weighed by what the
//! player's situation wants of that aim (`Situation::wants`): room for
//! cities, food of the land, the nearest known rival, war, happiness, the
//! leader's personality (`Personality`: the table of leaders is the one
//! place that binds a nation to a faith or a war branch). The best
//! branch wins; the rules' order (`rules::roles`) breaks ties.
//!
//! Assumptions the weights rely on (a plan that guards them can re-plan
//! when one fails):
//! - the nearest known rival stays at the distance seen, and no rival takes
//!   the free sites before `EXPECTED_CITIES_HORIZON` more cities are founded;
//! - the empire holds `Situation::expected_cities` cities by round 100 (the
//!   cities it has plus the free sites around the capital, at most
//!   `EXPECTED_CITIES_HORIZON` more), so a per-city effect is worth that
//!   many times its amount;
//! - the room around the capital (`ROOM_RADIUS`) stays free of war, and the
//!   food of the land around the sites stays what the known map shows;
//! - the branch is adopted in order, so a policy needing `d` others counts
//!   `1 / (1 + d)`, and a building that needs a technology stands late;
//! - the first branch is chosen around round 16 with no religion, no war
//!   and no building beyond those anyone can build from the start (the
//!   measurement of classic/docs/ai-policies.md was made there).
//!
//! The function is pure and deterministic: no hash order, no random, no
//! state. One decision is a bounded walk (`ROOM_RADIUS` around the capital)
//! and a pass over the policies of the candidate branches; it runs only
//! when the culture pool allows a branch, so at most once per round.

use crate::ai::planner_settle::{SiteContext, SITE_MIN_SCORE};
use crate::ai::state::AiState;
use crate::classic::effects::{ClassicCost, ClassicGrant, ClassicModifier, ClassicYield};
use crate::classic::stats::ClassicStat;
use crate::derived::tile_yields::nature_yields;
use crate::ids::{PlayerId, TileId};
use crate::knowledge::observation::AIObservation;
use crate::math::hex_disc;
use crate::rules::content::{Personality, PolicyBranchId, PolicyRole};
use crate::rules::effects::{
    CompiledEffect, Condition, CostTarget, Count, EffectSet, Grant, Modifier, PerCount, ProductionFilter, Selector, YieldKind,
};
use crate::rules::stats::Stat;
use crate::rules::CompiledRuleset;
use crate::world::city::CITY_WORK_RADIUS;
use crate::world::player::PlayerKind;

/// Tiles around the capital in which room for cities is counted.
pub const ROOM_RADIUS: i32 = 7;
/// Tiles between two cities that count as two sites.
pub const SITE_SPACING: i32 = 4;
/// Sites whose food counts as "the land" (the best ones).
pub const FEEDING_SITES: usize = 4;
/// Cities beyond those it has the empire is expected to found at most.
pub const EXPECTED_CITIES_HORIZON: usize = 3;
/// Distance to a known rival (tiles) from which it takes no room away.
pub const RIVAL_FAR: i32 = 16;
/// Distance to a known rival (tiles) at which it is a threat.
pub const RIVAL_NEAR: i32 = 10;

/// Points of one flat unit of a yield (+1 food, +1 culture...).
const FLAT: i32 = 10;
/// Points of one happiness.
const HAPPY: i32 = 20;
/// Points of a settler, of a great person, of a free technology.
pub(crate) const SETTLER: i32 = 60;
const GREAT_PERSON: i32 = 30;
pub(crate) const TECHNOLOGY: i32 = 60;
/// What a "per N of something" effect counts early, when the thing is not
/// known (policies, followers, trade partners).
const TYPICAL_COUNT: i32 = 2;
/// Citizens of a young city.
const CITIZENS: i32 = 5;
/// Longest chain of prerequisites counted between policies of a branch.
const MAX_DEPTH: i32 = 8;
/// Percent of an effect that holds only in some situation (a building, a
/// garrison, a kind of tile).
const CONDITIONAL_PERCENT: i32 = 50;
/// Percent of an effect that needs a building (of a technology) or an
/// improvement the empire does not have yet: it pays late, while the first
/// policies are chosen.
const LATER_PERCENT: i32 = 15;
/// Percent of a production bonus that holds for one kind of item only.
const NARROW_PERCENT: i32 = 15;
/// Average food of the best sites, as 100 % of the wish for growth.
const FOOD_REFERENCE: i32 = 40;
/// Sites that make 100 % of the wish for expansion.
const ROOM_REFERENCE: i32 = 3;
/// Bounds of the wish for growth (percent) given by the food of the land.
const FED_MIN: i32 = 70;
const FED_MAX: i32 = 150;
/// The wish for faith (percent): base, added once a religion is founded,
/// and per point of the leader's religion above the middle.
const FAITH_BASE: i32 = 100;
const FAITH_FOUNDED: i32 = 600;
const FAITH_PER_LEAN: i32 = 10;
/// The wish for gold, science and culture (percent) per point of the
/// leader's matching trait above the middle.
const ECONOMY_PER_LEAN: i32 = 10;
/// The wish for expansion (percent) when `ROOM_REFERENCE` sites are free.
pub(crate) const EXPANSION_PERCENT: i32 = 400;
/// The wish for the army (percent): base, per point of the leader's
/// aggression above the middle, at war, and with a rival city near.
const MILITARY_BASE: i32 = 0;
const MILITARY_PER_LEAN: i32 = 10;
pub(crate) const MILITARY_AT_WAR: i32 = 450;
const MILITARY_THREAT: i32 = 60;

/// The wish for happiness (percent): per city beyond `HAPPINESS_FREE_CITIES`,
/// and when the empire is unhappy now.
const HAPPINESS_FREE_CITIES: i32 = 3;
pub(crate) const HAPPINESS_PER_CITY: i32 = 25;
const HAPPINESS_UNHAPPY: i32 = 100;
/// The wish for diplomacy (percent): base, per point of the leader's
/// diplomacy above the middle, per known city-state (up to
/// `CITY_STATES_COUNTED`).
const DIPLOMACY_BASE: i32 = 50;
const DIPLOMACY_PER_LEAN: i32 = 20;
const DIPLOMACY_PER_CITY_STATE: i32 = 20;
const CITY_STATES_COUNTED: usize = 5;
/// The wish for trade (percent) while a trade route can be started.
const TRADE_WITH_ROUTES: i32 = 100;

/// What an effect adds to: the things a player wants of a branch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Aim {
    Growth,
    Expansion,
    Happiness,
    Production,
    Gold,
    Science,
    Culture,
    Faith,
    Military,
    Diplomacy,
    Trade,
}

/// Number of aims.
pub const AIMS: usize = 11;

/// What the player's situation is, in the terms the weights read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Situation {
    pub cities: usize,
    /// Cities that still fit around the capital, `SITE_SPACING` apart.
    pub room: usize,
    /// Mean food of the tiles a city on each of the best sites would work.
    pub food: i32,
    /// Tiles from the capital to the nearest known rival nation's
    /// city (None: none known).
    pub rival: Option<i32>,
    /// Percent of the cities on a coast.
    pub coastal: i32,
    pub at_war: bool,
    /// The empire's excess happiness is below zero.
    pub unhappy: bool,
    pub city_states: usize,
    pub trade_routes: bool,
    pub religion: bool,
    pub personality: Personality,
}

impl Situation {
    pub fn observe(obs: &AIObservation<'_>, rules: &CompiledRuleset, memory: &AiState) -> Situation {
        let land = Land::of(obs, rules, memory);
        Situation {
            cities: obs.own_cities.len(),
            room: land.room,
            food: land.food,
            rival: land.rival,
            coastal: coastal_percent(obs),
            at_war: obs.relations.iter().any(|r| r.at_war && r.kind == PlayerKind::Major),
            unhappy: obs.economy.turn_happiness < 0,
            city_states: obs.city_states.len(),
            trade_routes: obs.economy.trade_routes_left > 0,
            religion: obs.faith.overview.religion.is_some(),
            personality: rules.module.personality(obs.economy.nation),
        }
    }

    /// Cities the empire is expected to have: the room, discounted by a
    /// rival near the sites, over the cities it has.
    pub fn expected_cities(&self) -> i32 {
        self.cities as i32 + self.free_room().min(EXPECTED_CITIES_HORIZON as i32)
    }

    fn free_room(&self) -> i32 {
        let far = self.rival.map_or(RIVAL_FAR, |d| d.min(RIVAL_FAR));
        self.room as i32 * far / RIVAL_FAR
    }

    /// How much the situation wants each aim, in percent of an ordinary want.
    pub fn wants(&self) -> [i32; AIMS] {
        let p = &self.personality;
        let lean = |value: u8| i32::from(value) - i32::from(Personality::MIDDLE);
        let threat = self.rival.is_some_and(|d| d <= RIVAL_NEAR);
        let mut wants = [100; AIMS];
        let fed = (100 * self.food / FOOD_REFERENCE).clamp(FED_MIN, FED_MAX);
        wants[Aim::Growth as usize] = fed;
        wants[Aim::Expansion as usize] = crate::ai::genome::get_i32(crate::ai::genome::Knob::BranchExpansionPercent) * self.free_room().min(ROOM_REFERENCE) / ROOM_REFERENCE;
        wants[Aim::Happiness as usize] = 100
            + crate::ai::genome::get_i32(crate::ai::genome::Knob::BranchHappinessPerCity) * (self.cities as i32 - HAPPINESS_FREE_CITIES).max(0)
            + if self.unhappy { HAPPINESS_UNHAPPY } else { 0 };
        wants[Aim::Gold as usize] = 100 + ECONOMY_PER_LEAN * lean(p.gold);
        wants[Aim::Science as usize] = 100 + ECONOMY_PER_LEAN * lean(p.science);
        wants[Aim::Culture as usize] = 100 + ECONOMY_PER_LEAN * lean(p.culture);
        wants[Aim::Faith as usize] = (FAITH_BASE + if self.religion { FAITH_FOUNDED } else { 0 } + FAITH_PER_LEAN * lean(p.religion)).max(0);
        wants[Aim::Military as usize] =
            (MILITARY_BASE
                + MILITARY_PER_LEAN * lean(p.aggression)
                + if self.at_war { crate::ai::genome::get_i32(crate::ai::genome::Knob::BranchMilitaryAtWar) } else { 0 }
                + if threat { MILITARY_THREAT } else { 0 })
            .max(0);
        wants[Aim::Diplomacy as usize] =
            (DIPLOMACY_BASE + DIPLOMACY_PER_LEAN * lean(p.diplomacy) + DIPLOMACY_PER_CITY_STATE * self.city_states.min(CITY_STATES_COUNTED) as i32).max(0);
        wants[Aim::Trade as usize] = if self.trade_routes { TRADE_WITH_ROUTES } else { 0 };
        wants
    }
}

/// The land around the capital as the player knows it.
struct Land {
    room: usize,
    food: i32,
    rival: Option<i32>,
}

impl Land {
    fn of(obs: &AIObservation<'_>, rules: &CompiledRuleset, memory: &AiState) -> Land {
        let Some(capital) = capital_tile(obs) else {
            return Land { room: 0, food: 0, rival: None };
        };
        let dims = obs.view.dimensions();
        let own: Vec<TileId> = obs.own_cities.iter().map(|c| c.tile).collect();
        let foreign: Vec<TileId> = memory.seen_cities().iter().map(|c| c.tile).collect();
        let ctx = SiteContext {
            view: &obs.view,
            profile: Default::default(),
            rules,
            player: obs.player,
            own_cities: &own,
            foreign_cities: &foreign,
            reserved: &[],
            danger: &[],
            tile_memo: Default::default(),
        };
        let mut sites: Vec<(i32, TileId)> = hex_disc(dims, capital, ROOM_RADIUS)
            .filter_map(|tile| ctx.score(tile).filter(|s| *s >= SITE_MIN_SCORE).map(|s| (s, tile)))
            .collect();
        sites.sort_by_key(|(score, tile)| (std::cmp::Reverse(*score), *tile));
        let mut taken = own.clone();
        let mut best: Vec<TileId> = Vec::new();
        for (_, tile) in sites {
            if taken.iter().all(|t| dims.distance(*t, tile).is_some_and(|d| d >= SITE_SPACING)) {
                taken.push(tile);
                best.push(tile);
            }
        }
        let feeding = &best[..best.len().min(FEEDING_SITES)];
        let food = if feeding.is_empty() {
            0
        } else {
            feeding.iter().map(|t| site_food(&ctx, rules, *t)).sum::<i32>() / feeding.len() as i32
        };
        let rival = memory
            .seen_cities()
            .iter()
            .filter(|c| is_nation(obs, c.owner))
            .filter_map(|c| dims.distance(capital, c.tile))
            .min();
        Land { room: best.len(), food, rival }
    }
}

fn capital_tile(obs: &AIObservation<'_>) -> Option<TileId> {
    let capital = obs.view.own_capital()?;
    obs.own_cities.iter().find(|c| c.handle == capital).map(|c| c.tile)
}

fn is_nation(obs: &AIObservation<'_>, player: PlayerId) -> bool {
    obs.relations.iter().any(|r| r.other == player && r.kind == PlayerKind::Major)
}

/// Food of the known tiles a city on `site` works (by the core's tables).
fn site_food(ctx: &SiteContext<'_, '_>, rules: &CompiledRuleset, site: TileId) -> i32 {
    hex_disc(ctx.view.dimensions(), site, CITY_WORK_RADIUS)
        .filter(|t| ctx.view.tile(*t).snapshot().is_some())
        .map(|t| nature_yields(ctx.view, rules, t).food)
        .sum()
}

fn coastal_percent(obs: &AIObservation<'_>) -> i32 {
    if obs.own_cities.is_empty() {
        return 0;
    }
    let dims = obs.view.dimensions();
    let coastal = |tile: TileId| {
        hex_disc(dims, tile, 1)
            .any(|t| obs.view.tile(t).snapshot().is_some_and(|s| obs.view.terrain_kinds().is_water(s.terrain.0)))
    };
    let count = obs.own_cities.iter().filter(|c| coastal(c.tile)).count();
    (100 * count / obs.own_cities.len()) as i32
}

/// The next branch to unlock, if the culture pool allows one.
pub fn next_branch(obs: &AIObservation<'_>, rules: &CompiledRuleset, memory: &AiState) -> Option<PolicyBranchId> {
    if obs.culture.unlockable.is_empty() {
        return None;
    }
    best_branch(rules, &Situation::observe(obs, rules, memory), &obs.culture.unlockable)
}

/// The best of `candidates` for `situation`: highest value, then the rules'
/// order, then the lowest id.
pub fn best_branch(rules: &CompiledRuleset, situation: &Situation, candidates: &[PolicyBranchId]) -> Option<PolicyBranchId> {
    let wants = situation.wants();
    let order = rules.roles.module.ai_branches();
    let rank = |branch: &PolicyBranchId| order.iter().position(|b| b == branch).unwrap_or(order.len());
    candidates
        .iter()
        .max_by_key(|b| (branch_value(rules, situation, &wants, **b), std::cmp::Reverse(rank(b)), std::cmp::Reverse(b.0)))
        .copied()
}

/// What the policies of `branch` are worth to a player that wants `wants`.
pub fn branch_value(rules: &CompiledRuleset, situation: &Situation, wants: &[i32; AIMS], branch: PolicyBranchId) -> i64 {
    let worth = branch_worth(rules, situation, branch);
    (0..AIMS).map(|aim| i64::from(worth[aim]) * i64::from(wants[aim]) / 100).sum()
}

/// What the policies of `branch` declare, in points per aim for the whole
/// empire (before the situation weighs the aims). A policy that needs
/// `depth` others adopted first counts `1 / (1 + depth)` of its worth: the
/// branch is adopted in order, and the culture to reach the deep ones
/// comes late.
pub fn branch_worth(rules: &CompiledRuleset, situation: &Situation, branch: PolicyBranchId) -> [i32; AIMS] {
    let mut worth = [0; AIMS];
    let own = || rules.module.policies.iter().enumerate().filter(|(_, p)| p.branch == Some(branch));
    let ordinary = own().filter(|(_, p)| p.role == PolicyRole::Policy).count() as i32;
    for (index, policy) in own() {
        // The finisher waits for every ordinary policy of the branch.
        let before = if policy.role == PolicyRole::Finisher { ordinary } else { depth(rules, index) };
        let share = 100 / (1 + before);
        for (aim, points) in effects_of(&policy.effects).filter_map(|effect| worth_of(rules, situation, effect)) {
            worth[aim as usize] += points * share / 100;
        }
    }
    worth
}

/// How many policies one must adopt before `policy` (the longest chain of
/// prerequisites; the chain is bounded, a cycle counts as `MAX_DEPTH`).
fn depth(rules: &CompiledRuleset, policy: usize) -> i32 {
    fn walk(rules: &CompiledRuleset, policy: usize, left: i32) -> i32 {
        if left == 0 {
            return 0;
        }
        let Some(definition) = rules.module.policies.get(policy) else { return 0 };
        definition.prerequisites.iter().map(|p| 1 + walk(rules, p.0 as usize, left - 1)).max().unwrap_or(0)
    }
    walk(rules, policy, MAX_DEPTH)
}

fn effects_of(set: &EffectSet) -> impl Iterator<Item = &CompiledEffect> {
    set.passive_player
        .iter()
        .chain(&set.passive_module)
        .chain(&set.passive_city)
        .chain(&set.passive_unit)
        .chain(&set.passive_tile)
        .chain(set.triggered.iter().flatten())
}

/// The aim an effect adds to and its points for the whole empire.
fn worth_of(rules: &CompiledRuleset, situation: &Situation, effect: &CompiledEffect) -> Option<(Aim, i32)> {
    let (aim, amount) = classify(rules, situation, effect)?;
    let reach = match effect.selector {
        Selector::OwnerCities | Selector::OwnerCityTiles => situation.expected_cities(),
        _ => 1,
    };
    Some((aim, amount * reach * condition_percent(rules, situation, effect.condition) / 100))
}

fn condition_percent(rules: &CompiledRuleset, situation: &Situation, condition: Condition) -> i32 {
    match condition {
        Condition::Always
        | Condition::HasTechnology(_)
        | Condition::LacksTechnology(_)
        | Condition::MinPopulation(_)
        | Condition::ConnectedToCapital
        | Condition::Producing(ProductionFilter::Units | ProductionFilter::Buildings) => 100,
        Condition::Producing(_) => NARROW_PERCENT,
        // A building anyone can build from the start stands in every city by
        // the first policies; one that needs a technology comes later.
        Condition::HasBuilding(building) if rules.buildings.get(building.0 as usize).is_some_and(|b| b.required_tech.is_none()) => 100,
        Condition::HasBuilding(_) | Condition::TileImprovement(_) => LATER_PERCENT,
        Condition::CityCoastal => situation.coastal,
        Condition::AtPeace if situation.at_war => 0,
        Condition::AtPeace => 100,
        _ => CONDITIONAL_PERCENT,
    }
}

fn classify(rules: &CompiledRuleset, situation: &Situation, effect: &CompiledEffect) -> Option<(Aim, i32)> {
    match effect.modifier {
        Modifier::AddYield { yield_kind, amount } => Some((yield_aim(yield_kind)?, amount * FLAT)),
        Modifier::AddYieldPercent { yield_kind, percent } => percent_worth(percent_aim(rules, yield_kind, effect.condition)?, percent),
        Modifier::AddYieldPerPopulation { yield_kind, hundredths } => Some((yield_aim(yield_kind)?, hundredths * FLAT / 20)),
        Modifier::AddYieldPerCount { yield_kind, amount, per } => Some((yield_aim(yield_kind)?, counted(situation, amount * FLAT, per))),
        Modifier::AddYieldPercentPerCount { yield_kind, percent, per } => percent_worth(yield_aim(yield_kind)?, counted(situation, percent, per)),
        Modifier::ModifyGrowth { percent } => percent_worth(Aim::Growth, percent),
        Modifier::ModifyCost { target, percent } => cost_worth(target, -percent),
        Modifier::ModifyCombat { percent, .. } => percent_worth(Aim::Military, percent),
        Modifier::ModifyUnitStat { .. }
        | Modifier::ModifyMovement { .. }
        | Modifier::PassivePromotion(_)
        | Modifier::PostCombatPromotion(_)
        | Modifier::AddPromotion(_)
        | Modifier::GrantExperience(_)
        | Modifier::ModifyCityCombat { .. }
        | Modifier::YieldFromKill { .. } => Some((Aim::Military, FLAT)),
        Modifier::FreeUnit { unit, count } => free_unit_worth(rules, unit, count),
        Modifier::AddStat { stat, amount } => stat_worth(stat, amount),
        Modifier::Grant(Grant::FreeTechnologies(count)) => Some((Aim::Science, i32::from(count) * crate::ai::genome::get_i32(crate::ai::genome::Knob::BranchTechnologyPoints))),
        Modifier::Grant(Grant::Module(ClassicGrant::FreeGreatPeople(count))) => Some((Aim::Culture, i32::from(count) * GREAT_PERSON)),
        Modifier::Grant(Grant::Module(ClassicGrant::GoldenAge { .. })) => Some((Aim::Happiness, 2 * HAPPY)),
        Modifier::Module(module) => module_worth(situation, module),
        _ => None,
    }
}

/// An amount paid per `every` of something the empire counts, with what it
/// counts early on: the cities besides the capital it will have, the
/// citizens of a young city, one wonder, otherwise `TYPICAL_COUNT`
/// (fractions are kept: the count grows).
fn counted(situation: &Situation, amount: i32, per: PerCount) -> i32 {
    let count = match per.count {
        Count::ConnectedCities => (situation.expected_cities() - 1).max(0),
        Count::Population => CITIZENS,
        Count::WorldWonders => 1,
        Count::Module(_) => TYPICAL_COUNT,
    };
    amount * count / i32::from(per.every.max(1))
}

fn yield_aim(kind: YieldKind) -> Option<Aim> {
    match kind {
        YieldKind::Food => Some(Aim::Growth),
        YieldKind::Production => Some(Aim::Production),
        YieldKind::Gold => Some(Aim::Gold),
        YieldKind::Science => Some(Aim::Science),
        YieldKind::Culture => Some(Aim::Culture),
        YieldKind::Module(ClassicYield::Faith) => Some(Aim::Faith),
        YieldKind::Output => None,
    }
}

/// A percent of a yield, while the city builds something: a settler or a
/// military unit is expansion or war, not production.
fn percent_aim(rules: &CompiledRuleset, kind: YieldKind, condition: Condition) -> Option<Aim> {
    match condition {
        Condition::Producing(ProductionFilter::UnitClassOf(unit)) if rules.units.get(unit.0 as usize).is_some_and(|u| u.founder) => {
            Some(Aim::Expansion)
        }
        Condition::Producing(ProductionFilter::MilitaryUnits | ProductionFilter::UnitRole(_)) => Some(Aim::Military),
        _ => yield_aim(kind),
    }
}

/// A cost cut (`cut` percent) as the aim it serves.
fn cost_worth(target: CostTarget, cut: i32) -> Option<(Aim, i32)> {
    match target {
        CostTarget::Units | CostTarget::Buildings => percent_worth(Aim::Production, cut),
        CostTarget::Technologies => percent_worth(Aim::Science, cut),
        CostTarget::TilePurchase | CostTarget::BorderGrowth | CostTarget::BorderGrowthExponent => percent_worth(Aim::Expansion, cut / 2),
        CostTarget::Hurry | CostTarget::UnitPurchase | CostTarget::BuildingPurchase => percent_worth(Aim::Gold, cut / 2),
        CostTarget::UnitUpgrade => percent_worth(Aim::Military, cut / 2),
        CostTarget::Module(ClassicCost::Policies) => percent_worth(Aim::Culture, cut / 2),
        CostTarget::Module(ClassicCost::PolicyCityPenalty) => percent_worth(Aim::Expansion, cut),
        CostTarget::Module(ClassicCost::FaithPurchase) => percent_worth(Aim::Faith, cut / 2),
    }
}

/// A percent of what the aim yields in an ordinary city, in points.
fn percent_worth(aim: Aim, percent: i32) -> Option<(Aim, i32)> {
    let base = match aim {
        Aim::Growth | Aim::Production => 4 * FLAT,
        Aim::Gold | Aim::Science | Aim::Diplomacy | Aim::Trade | Aim::Military => 3 * FLAT,
        Aim::Culture => 2 * FLAT,
        Aim::Faith => FLAT,
        Aim::Expansion => 6 * FLAT,
        Aim::Happiness => 4 * FLAT,
    };
    Some((aim, percent * base / 100))
}

fn free_unit_worth(rules: &CompiledRuleset, unit: crate::ids::UnitKindId, count: u8) -> Option<(Aim, i32)> {
    let def = rules.units.get(unit.0 as usize)?;
    let count = i32::from(count);
    Some(if def.founder {
        (Aim::Expansion, count * crate::ai::genome::get_i32(crate::ai::genome::Knob::BranchSettlerPoints))
    } else if def.builder {
        (Aim::Growth, count * FLAT)
    } else if def.strength > 0 {
        (Aim::Military, count * 2 * FLAT)
    } else {
        (Aim::Culture, count * GREAT_PERSON)
    })
}

fn stat_worth(stat: Stat, amount: i32) -> Option<(Aim, i32)> {
    match stat {
        Stat::FoodKept => percent_worth(Aim::Growth, amount),
        Stat::WorkerSpeedPercent => percent_worth(Aim::Growth, amount / 2),
        Stat::Module(ClassicStat::Trade(_)) => Some((Aim::Trade, amount.abs() * FLAT)),
        Stat::Module(ClassicStat::HappinessPerCity | ClassicStat::HappinessPerLuxury) => Some((Aim::Happiness, amount * HAPPY)),
        Stat::Module(ClassicStat::PopulationUnhappinessPercent | ClassicStat::CityCountUnhappinessPercent) => {
            Some((Aim::Happiness, -amount / 5 * HAPPY / FLAT))
        }
        Stat::Module(ClassicStat::GreatPeopleRate | ClassicStat::GreatPersonRate(_)) => percent_worth(Aim::Culture, amount / 2),
        _ => None,
    }
}

fn module_worth(situation: &Situation, modifier: ClassicModifier) -> Option<(Aim, i32)> {
    match modifier {
        ClassicModifier::AddHappiness { amount } => Some((Aim::Happiness, amount * HAPPY)),
        ClassicModifier::AddHappinessPerCount { amount, per } => Some((Aim::Happiness, counted(situation, amount * HAPPY, per))),
        ClassicModifier::AddHappinessPerFollowingCity { .. } => Some((Aim::Happiness, HAPPY)),
        ClassicModifier::ModifyCityState { amount, .. } => Some((Aim::Diplomacy, amount.abs().max(FLAT))),
        ClassicModifier::FreePolicies { count } => Some((Aim::Culture, i32::from(count) * 4 * GREAT_PERSON / 3)),
        ClassicModifier::AddYieldPerFollowingCity { yield_kind, amount, .. } => Some((yield_aim(yield_kind)?, amount * FLAT)),
        ClassicModifier::AddYieldPerFollowers { yield_kind, .. } => Some((yield_aim(yield_kind)?, FLAT)),
        ClassicModifier::ModifyReligion { percent, .. } => percent_worth(Aim::Faith, percent),
        ClassicModifier::UnlockFaithBuilding(_) | ClassicModifier::UnlockFaithUnits(_) => Some((Aim::Faith, 2 * FLAT)),
        ClassicModifier::ModifyTourism { .. } | ClassicModifier::TourismToward { .. } => Some((Aim::Culture, FLAT)),
        _ => None,
    }
}

/// The policies, branch openers and tenets priced by what they do for the
/// player (`ScienceLevers::policies`, classic/docs/ai-science-levers.md),
/// each effect in the allocator's measure (`ai::value::Prices`, hundredths
/// of a score point):
/// - a yield, a yield percent, a yield per citizen and happiness as the
///   allocator prices a building's (`Prices::effect_in`: what they change
///   in the cities and tiles they reach, while their conditions hold);
/// - a grant on adoption once, as a building's grant on completion; a free
///   settler as the city it founds (`Prices::new_city`);
/// - the science levers the allocator has no model of, by what the rules
///   make of them for this player: a science percent while the empire is
///   content on the empire's beakers, a yield per specialist on the
///   specialists it has or will have, a great person rate on the points its
///   specialists make at the point price of their great person, a research
///   agreement percent on the agreements it may sign;
/// - anything else by the branch weights (`worth_of`: its aim, amount,
///   reach and condition, weighed by the situation's wants) at the price of
///   its aim: a flat point (`FLAT`) is one of the aim's yield a turn, a
///   happiness `HAPPY` points, a settler `SETTLER` points of expansion.
pub mod priced {
    use crate::ai::science_levers::{self as levers, ScienceLevers};
    use crate::ai::state::AiState;
    use crate::classic::effects::ClassicModifier;
    use crate::ai::value::{Centi, Prices};
    use crate::classic::content::GREAT_PERSON_KINDS;
    use crate::ids::PolicyId;
    use crate::rules::content::SpecialistId;
    use crate::knowledge::observation::{AIObservation, OwnCity};
    use crate::knowledge::public::PlayerQueries;
    use crate::rules::content::{PolicyBranchId, PolicyRole};
    use crate::rules::effects::{CompiledEffect, Modifier};
    use crate::rules::stats::{ClassicStat, SocietyStat, Stat};
    use crate::rules::CompiledRuleset;

    /// The turn's facts the prices of the effects read, gathered once.
    pub struct PolicyMarket<'o, 'a> {
        obs: &'o AIObservation<'a>,
        rules: &'o CompiledRuleset,
        prices: &'o Prices,
        levers: ScienceLevers,
        capital: Option<&'o OwnCity>,
        points: [Centi; GREAT_PERSON_KINDS],
        situation: super::Situation,
        wants: [i32; super::AIMS],
        /// The price of a flat point of each aim (`aim_price`).
        aims: [Centi; super::AIMS],
        /// The public scores of the met nations (`PlayerQueries::race`),
        /// read when an effect needs a partner's science.
        scores: std::cell::OnceCell<Vec<(crate::ids::PlayerId, i64)>>,
    }

    /// The price of a flat point (`FLAT`) of each aim: one of its yield a
    /// turn; happiness `HAPPY` points a point of happiness; expansion
    /// `SETTLER` points a new city; the army, diplomacy and trade gold.
    fn aim_prices(prices: &Prices) -> [Centi; super::AIMS] {
        use super::Aim;
        let flat = i64::from(super::FLAT);
        let mut out = [0; super::AIMS];
        for (aim, price) in [
            (Aim::Growth, prices.food),
            (Aim::Production, prices.production),
            (Aim::Gold, prices.gold),
            (Aim::Science, prices.science),
            (Aim::Culture, prices.culture),
            (Aim::Faith, prices.faith),
            (Aim::Happiness, prices.happiness * flat / i64::from(super::HAPPY)),
            (Aim::Expansion, prices.new_city(0) * flat / i64::from(super::SETTLER)),
            (Aim::Military, prices.gold),
            (Aim::Diplomacy, prices.gold),
            (Aim::Trade, prices.gold),
        ] {
            out[aim as usize] = price;
        }
        out
    }

    impl<'o, 'a> PolicyMarket<'o, 'a> {
        pub fn new(obs: &'o AIObservation<'a>, rules: &'o CompiledRuleset, memory: &AiState, prices: &'o Prices, levers: ScienceLevers) -> Self {
            let points = levers::point_prices(obs, rules, prices);
            let situation = super::Situation::observe(obs, rules, memory);
            let wants = situation.wants();
            let aims = aim_prices(prices);
            Self { obs, rules, prices, levers, capital: levers::capital(obs), points, situation, wants, aims, scores: std::cell::OnceCell::new() }
        }

        /// What adopting `policy` brings: the sum of its effects.
        pub fn policy(&self, policy: PolicyId) -> Centi {
            let Some(def) = self.rules.module.policies.get(policy.0 as usize) else { return 0 };
            super::effects_of(&def.effects).map(|e| self.effect(e)).sum()
        }

        /// A started branch on the scale of a step of culture: what is left
        /// of it (its policies not adopted and its finisher) over the
        /// policies left; a tenet of an ideology alone (`adoption`'s value:
        /// an ideology has no finisher and its tenets come tier by tier).
        pub fn rest_step(&self, branch: PolicyBranchId) -> Centi {
            let adopted = |p: PolicyId| self.obs.culture.policies.contains(&p);
            let mut total = 0;
            let mut left = 0i64;
            for (i, def) in self.rules.module.policies.iter().enumerate() {
                let id = PolicyId(i as u16);
                if def.branch != Some(branch) || adopted(id) {
                    continue;
                }
                match def.role {
                    PolicyRole::Policy => {
                        total += self.policy(id);
                        left += 1;
                    }
                    PolicyRole::Finisher => total += self.policy(id),
                    _ => {}
                }
            }
            total / left.max(1)
        }

        /// One step of a branch not started, on the scale of an adoption:
        /// everything the branch brings (its opener, its policies, its
        /// finisher) over the steps of culture it costs (the opener and
        /// each policy).
        pub fn branch_step(&self, branch: PolicyBranchId) -> Centi {
            let mut total = 0;
            let mut steps = 0i64;
            for (i, def) in self.rules.module.policies.iter().enumerate() {
                if def.branch != Some(branch) {
                    continue;
                }
                total += self.policy(PolicyId(i as u16));
                if def.role != PolicyRole::Finisher {
                    steps += 1;
                }
            }
            total / steps.max(1)
        }

        /// An ideology chosen now: its tenets' mean worth for each tenet it
        /// gives at once (one bought, `free` given) — the tenets the choice
        /// opens, on the scale of an adoption.
        pub fn ideology(&self, branch: PolicyBranchId, free: u8) -> Centi {
            let mut total = 0;
            let mut tenets = 0i64;
            for (i, def) in self.rules.module.policies.iter().enumerate() {
                if def.branch == Some(branch) && matches!(def.role, PolicyRole::Tenet(_)) {
                    total += self.policy(PolicyId(i as u16));
                    tenets += 1;
                }
            }
            total / tenets.max(1) * (1 + i64::from(free))
        }

        /// One effect for the rest of the game.
        pub fn effect(&self, effect: &CompiledEffect) -> Centi {
            let Some(at) = self.capital else { return 0 };
            let (obs, rules, prices) = (self.obs, self.rules, self.prices);
            let cities = obs.own_cities.len() as i32;
            if levers::on_adopt(effect) {
                return match effect.modifier {
                    Modifier::FreeUnit { unit, count } if rules.units.get(unit.0 as usize).is_some_and(|u| u.founder) => i64::from(count) * prices.new_city(0),
                    _ => levers::grant_once(obs, rules, prices, at, effect),
                };
            }
            match effect.modifier {
                Modifier::AddYield { .. }
                | Modifier::AddYieldPercent { .. }
                | Modifier::AddYieldPerPopulation { .. }
                | Modifier::Module(ClassicModifier::AddHappiness { .. })
                | Modifier::AddStat { stat: Stat::Module(ClassicStat::HappinessPerCity), .. } => prices.effect_in(obs, rules, at, effect, cities),
                Modifier::AddStat { stat: Stat::Module(ClassicStat::Society(SocietyStat::ScienceWhileHappyPercent)), amount } => {
                    if obs.economy.turn_happiness < 0 {
                        return 0;
                    }
                    let beakers: i64 = obs.own_cities.iter().map(|c| i64::from(c.income.science.max(0))).sum();
                    beakers * i64::from(amount) / 100 * prices.science
                }
                Modifier::AddStat { stat: Stat::SpecialistYield { specialist, yield_kind }, amount } => {
                    let default = rules.roles.default_specialist;
                    let fits = |kind: SpecialistId| match specialist {
                        Some(s) => s == kind,
                        None => Some(kind) != default,
                    };
                    let count: i64 = obs.own_cities.iter().map(|c| self.specialists(c, &fits)).sum();
                    count * i64::from(amount) * prices.of_yield(yield_kind)
                }
                Modifier::AddStat { stat: Stat::AnySpecialistYield(yield_kind), amount } => {
                    let default = rules.roles.default_specialist;
                    let cities = obs.own_cities.iter().filter(|c| self.specialists(c, &|k| Some(k) != default) > 0).count() as i64;
                    cities * i64::from(amount) * prices.of_yield(yield_kind)
                }
                Modifier::AddStat { stat: Stat::Module(ClassicStat::ResearchAgreementPercent), amount } => {
                    let scores = self.scores.get_or_init(|| obs.view.race().scores);
                    let beakers: i64 = levers::agreement_partners(obs).map(|p| levers::agreement_beakers_per_turn(obs, scores, p, i64::from(amount))).sum();
                    beakers * prices.science
                }
                Modifier::AddStat { stat: Stat::Module(ClassicStat::GreatPeopleRate), amount } => self.rate(amount, None),
                Modifier::AddStat { stat: Stat::Module(ClassicStat::GreatPersonRate(kind)), amount } => self.rate(amount, Some(kind)),
                _ => match super::worth_of(rules, &self.situation, effect) {
                    Some((aim, points)) => {
                        let aim = aim as usize;
                        i64::from(points) * self.aims[aim] / i64::from(super::FLAT) * i64::from(self.wants[aim]) / 100
                    }
                    None => 0,
                },
            }
        }

        /// Specialists of the kinds `fits` takes that `city` has or will
        /// have: its slots of those kinds when the player fills the slots
        /// that pay, else as many as its citizens that work no tile.
        fn specialists(&self, city: &OwnCity, fits: &dyn Fn(SpecialistId) -> bool) -> i64 {
            let slots = levers::slots(self.rules, city);
            let open: i64 = slots.iter().enumerate().filter(|(k, _)| fits(SpecialistId(*k as u16))).map(|(_, n)| i64::from(*n)).sum();
            if self.levers.specialists {
                open
            } else {
                open.min(i64::from(levers::idle_citizens(city)))
            }
        }

        /// A great person rate of `percent` (for `kind` only, or for every
        /// kind): the points the specialists make a turn times the percent,
        /// at the point price of their great person.
        fn rate(&self, percent: i32, kind: Option<crate::classic::content::GreatPersonKind>) -> Centi {
            let mut total = 0;
            for city in &self.obs.own_cities {
                for k in 0..self.rules.specialists.len().min(crate::world::city::MAX_SPECIALIST_KINDS) {
                    let specialist = SpecialistId(k as u16);
                    let Some((person, points)) = levers::specialist_points(self.rules, specialist) else { continue };
                    if kind.is_some_and(|wanted| wanted != person) {
                        continue;
                    }
                    let count = self.specialists(city, &|s| s == specialist);
                    total += count * points * i64::from(percent) / 100 * self.points[person.index()];
                }
            }
            total
        }
    }
}
