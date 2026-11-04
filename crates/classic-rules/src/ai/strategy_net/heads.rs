//! The decision heads: research, policies and production, each scoring its
//! candidates as `(prices · effects-for-us + own correction) / (turns + 2)`
//! with the plan's prices (`Plan::mult`), so one intent moves all three the
//! same way.
//!
//! - **Research**: a technology's own value at the net's prices (what it
//!   unlocks: `planner_value::tech_value`, its score points, the wonders it
//!   opens) plus the goal value carried back through the tree: each goal's
//!   worth is shared over the missing technologies of its path by their
//!   cost, so a "bridge" technology worth nothing itself carries its goal; a
//!   technology on the paths of two goals gets 30 % more. The research holds
//!   the plan's target path unless another technology leads it by 15 %.
//! - **Policies**: each policy by the cell's run with it and its other
//!   effects (`effects`); a started branch by what is left of it per policy
//!   left (its finisher in it), a new branch by all it brings per step, both
//!   times the branch's fit to the intent (`branches`: its character from
//!   the rules' data against the style, aim and posture; the war branch shut
//!   outside the war posture); a new branch only when it leads the started
//!   ones by 50 % (the tree is finished first); a goal branch +50 %; with
//!   the science aim, once a branch is open the culture waits for the
//!   science branch while its era is ahead; the first branch waits for the
//!   style's lock. The ideology the rules name for the plan (`ai_ideology`:
//!   the war posture's, the tall style's peace one, the wide or conquest
//!   style's), else by its tenets; switched at unrest when the culture
//!   pressure is high.
//! - **Production**: the allocator's best four orders (and a goal's item or
//!   a military unit when the plan wants one), the allocator's value
//!   re-weighted by the net's prices over the allocator's (by the slot of
//!   each effect; a percent by the city's growth over H), the style's
//!   correction of the kind, a goal item's share of its goal.

use super::cell::{run, slot, CellPrices, Place, Plan as CellPlan};
use super::effects::Valuer;
use super::layers::ONE;
use super::model::Model;
use super::weights::{head as w, kc, pd};
use super::{Base, Milli, Plan};
use crate::ai::choice::ProductionOption;
use crate::ai::planner::{role_of, AiRole, Decision, TurnBrief};
use crate::ai::planner_value::{tech_value, unit_strength};
use crate::ai::trace::{Actor, Reason};
use crate::ai::value::Prices;
use crate::command::Command;
use crate::ids::{PolicyId, ProductionItem, TechId};
use crate::knowledge::observation::{AIObservation, OwnCity};
use crate::rules::content::{PolicyBranchId, PolicyRole};
use crate::rules::roles::IdeologyCase;
use crate::rules::effects::{Modifier, Trigger};
use crate::rules::CompiledRuleset;

/// The allocator's prices multiplied by the net's.
pub fn net_prices(prices: &Prices, mult: &[Milli; pd::N]) -> Prices {
    let m = |v: i64, d: usize| v * mult[d] / ONE;
    let mut p = *prices;
    p.food = m(p.food, pd::FOOD);
    p.production = m(p.production, pd::PRODUCTION);
    p.gold = m(p.gold, pd::GOLD);
    p.science = m(p.science, pd::SCIENCE);
    p.culture = m(p.culture, pd::CULTURE);
    p.faith = m(p.faith, pd::FAITH);
    p.happiness = m(p.happiness, pd::HAPPINESS);
    p
}

// ---- research ----

/// One technology the research may take now, scored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TechScore {
    pub tech: TechId,
    pub own: i64,
    pub goal: i64,
    pub turns: i64,
    pub score: i64,
}

/// The research head's scores of the technologies `available` (pure: the
/// scenario tests call it). `own` gives a technology's own value.
#[allow(clippy::too_many_arguments)]
pub fn score_techs(rules: &CompiledRuleset, plan: &Plan, available: &[TechId], science: i64, science_percent: i64, own: impl Fn(TechId) -> i64) -> Vec<TechScore> {
    available
        .iter()
        .map(|&tech| {
            let cost = rules.technologies.get(usize::from(tech.0)).map_or(1, |d| i64::from(d.cost)).max(1);
            let mut goal = 0i64;
            let mut served = 0;
            for g in &plan.goals {
                if !g.path.contains(&tech) {
                    continue;
                }
                let total: i64 = g.path.iter().filter_map(|t| rules.technologies.get(usize::from(t.0))).map(|d| i64::from(d.cost)).sum::<i64>().max(1);
                goal += g.worth.max(0) * cost / total * w::BEELINE / ONE;
                served += 1;
            }
            if served >= 2 {
                goal = goal * (ONE + w::SHARED_BONUS) / ONE;
            }
            let turns = cost * science_percent / 100 / science.max(1);
            let own = own(tech);
            TechScore { tech, own, goal, turns, score: (own + goal) * ONE / (turns + w::TURNS_PAD) }
        })
        .collect()
}

/// The research choice among scored technologies: the best on the target's
/// path unless another leads it by `KEEP_MARGIN`.
pub fn pick_tech(plan: &Plan, scores: &[TechScore]) -> Option<TechId> {
    let best = scores.iter().max_by_key(|s| (s.score, std::cmp::Reverse(s.tech.0)))?;
    let on_path = plan.target.and_then(|target| {
        let path = &plan.goals.iter().find(|g| g.path.last() == Some(&target))?.path;
        scores.iter().filter(|s| path.contains(&s.tech)).max_by_key(|s| (s.score, std::cmp::Reverse(s.tech.0)))
    });
    match on_path {
        Some(p) if best.score * ONE <= p.score * w::KEEP_MARGIN => Some(p.tech),
        _ => Some(best.tech),
    }
}

/// Strongest land unit the player's technologies unlock.
fn best_known_strength(brief: &TurnBrief, rules: &CompiledRuleset) -> i32 {
    rules
        .units
        .iter()
        .enumerate()
        .filter(|(_, d)| d.required_tech.is_none_or(|t| brief.knows(t)) && !d.unbuildable)
        .map(|(i, _)| unit_strength(rules, crate::ids::UnitKindId(i as u16)))
        .max()
        .unwrap_or(0)
}

/// The research decision of a turn with an empty queue.
pub fn research(brief: &TurnBrief, rules: &CompiledRuleset, prices: &Prices, plan: &Plan) -> Option<Decision> {
    if !brief.research_queue.is_empty() {
        return None;
    }
    let available: Vec<TechId> = (0..rules.technologies.len())
        .map(|i| TechId(i as u16))
        .filter(|t| match &brief.researchable {
            Some(list) => list.binary_search_by_key(&t.0, |x| x.0).is_ok(),
            None => !brief.knows(*t) && rules.technologies[usize::from(t.0)].requires.iter().all(|r| brief.knows(*r)),
        })
        .collect();
    if available.is_empty() {
        return Some(Decision::refuse(Actor::Player, Reason::NoTechAvailable));
    }
    let net = net_prices(prices, &plan.mult);
    let weights = net.weights();
    let cities = brief.cities.len().max(1) as i32;
    let strength = best_known_strength(brief, rules);
    let own = |tech: TechId| {
        let def = &rules.technologies[usize::from(tech.0)];
        let wonders = def.unlocks_buildings.iter().filter(|b| rules.buildings[usize::from(b.0)].wonder && !brief.built_wonders.contains(b)).count() as i64;
        // The best technology it opens next counts half (the allocator's
        // research: `planner_research::best_follow_up`).
        let next = rules
            .technologies
            .iter()
            .enumerate()
            .filter(|(_, d)| d.requires.contains(&tech) && d.requires.iter().all(|r| *r == tech || brief.knows(*r)))
            .map(|(i, _)| tech_value(rules, TechId(i as u16), &weights, cities, strength))
            .max()
            .unwrap_or(0);
        // Weights count ten times their value (`planner_value`).
        (i64::from(tech_value(rules, tech, &weights, cities, strength)) + i64::from(next) / 2) * 10 + prices.score.tech + wonders * prices.score.wonder * plan.mult[pd::WONDER] / ONE / 2
    };
    let science_percent = i64::from(brief.speed.science_percent);
    let scores = score_techs(rules, plan, &available, i64::from(brief.science.max(1)), science_percent, own);
    let technology = pick_tech(plan, &scores)?;
    super::trace::research(brief.player.0, brief.round, technology, &scores, rules);
    Some(Decision::act(Actor::Player, Reason::Research, Command::ChooseTechnology { player: brief.player, technology }))
}

// ---- policies ----

/// How much a plan fights: danger, or the shares of preparing and waging a
/// war (thousandths).
pub fn fight(intent: &super::Intent) -> Milli {
    intent.concepts[kc::DANGER].max(intent.posture[2] + intent.posture[3])
}

/// What the policy head knows of the culture screen.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Culture {
    pub adoptable: Vec<PolicyId>,
    pub unlockable: Vec<PolicyBranchId>,
    pub ideologies: Vec<(PolicyBranchId, u8)>,
}

/// The policy choice (pure over the model: the scenario tests call it).
pub enum PolicyChoice {
    Adopt(PolicyId),
    Unlock(PolicyBranchId),
    Ideology(PolicyBranchId),
}

/// A policy's worth within H: the cell's run with it and its other
/// effects.
fn policy_worth(model: &Model, rules: &CompiledRuleset, prices: &CellPrices, valuer: &Valuer<'_>, base: i64, policy: PolicyId, horizon: i64) -> i64 {
    let Some(def) = rules.module.policies.get(usize::from(policy.0)) else { return 0 };
    let mut plan = CellPlan::new(model);
    plan.add(model, rules, &def.effects, Place::Empire, 0);
    run(model, &plan, prices, horizon).value - base + valuer.set(&def.effects, Place::Empire)
}

/// Every policy's worth (diagnostics and the weights page): (policy, cell
/// part, other effects).
pub fn policy_values(model: &Model, rules: &CompiledRuleset, plan: &Plan, base: &Base) -> Vec<(PolicyId, i64, i64)> {
    let horizon = i64::from(plan.intent.horizon.max(1));
    let prices = CellPrices::of(base, &plan.mult);
    let room = i64::from(plan.city_target.saturating_sub(model.cities.len() as u32));
    let valuer = Valuer { model, rules, base, mult: &plan.mult, horizon: base.rounds_left.max(1), room, fight: fight(&plan.intent) };
    let empty = CellPlan::new(model);
    let v0 = run(model, &empty, &prices, horizon).value;
    (0..rules.module.policies.len())
        .map(|i| {
            let p = PolicyId(i as u16);
            let def = &rules.module.policies[i];
            let mut plan = CellPlan::new(model);
            plan.add(model, rules, &def.effects, Place::Empire, 0);
            (p, run(model, &plan, &prices, horizon).value - v0, valuer.set(&def.effects, Place::Empire))
        })
        .collect()
}

/// The policy head over a model.
pub fn choose_policy(model: &Model, rules: &CompiledRuleset, plan: &Plan, base: &Base, culture: &Culture) -> Option<PolicyChoice> {
    let horizon = i64::from(plan.intent.horizon.max(1));
    let prices = CellPrices::of(base, &plan.mult);
    let room = i64::from(plan.city_target.saturating_sub(model.cities.len() as u32));
    let valuer = Valuer { model, rules, base, mult: &plan.mult, horizon: base.rounds_left.max(1), room, fight: fight(&plan.intent) };
    let empty = CellPlan::new(model);
    let v0 = run(model, &empty, &prices, horizon).value;
    let mut memo: Vec<(PolicyId, i64)> = Vec::new();
    let mut worth = |p: PolicyId| -> i64 {
        if let Some((_, v)) = memo.iter().find(|(q, _)| *q == p) {
            return *v;
        }
        let v = policy_worth(model, rules, &prices, &valuer, v0, p, horizon);
        memo.push((p, v));
        v
    };
    let goal = |b: PolicyBranchId, v: i64| if plan.goal_branch(b).is_some() { v * (ONE + w::GOAL_BRANCH) / ONE } else { v };
    // The fit of a branch to the intent (None: shut).
    let fit_of = |b: PolicyBranchId| super::branches::branch_fit(rules, &plan.intent, b);
    if !culture.ideologies.is_empty() {
        // The ideology the rules name for the plan: the war posture's, the
        // tall style's (peace) or the wide and conquest styles'.
        let case = if plan.intent.posture_pick >= 2 {
            IdeologyCase::AtWar
        } else if plan.intent.style_pick == 0 {
            IdeologyCase::AtPeace
        } else {
            IdeologyCase::Wide
        };
        if let Some(named) = rules.roles.module.ai_ideology(case).filter(|b| culture.ideologies.iter().any(|(x, _)| x == b)) {
            return Some(PolicyChoice::Ideology(named));
        }
        let mut best: Option<(i64, PolicyBranchId)> = None;
        for (branch, free) in &culture.ideologies {
            let tenets: Vec<PolicyId> = rules
                .module
                .policies
                .iter()
                .enumerate()
                .filter(|(_, p)| p.branch == Some(*branch) && matches!(p.role, PolicyRole::Tenet(_)))
                .map(|(i, _)| PolicyId(i as u16))
                .collect();
            let total: i64 = tenets.iter().map(|t| worth(*t)).sum();
            let v = total / tenets.len().max(1) as i64 * (1 + i64::from(*free));
            if best.is_none_or(|(b, x)| v > b || v == b && branch.0 < x.0) {
                best = Some((v, *branch));
            }
        }
        return best.map(|(_, b)| PolicyChoice::Ideology(b));
    }
    let branch_of = |p: PolicyId| rules.module.policies.get(usize::from(p.0)).and_then(|d| d.branch);
    let is_ideology = |b: PolicyBranchId| rules.module.policy_branches.get(usize::from(b.0)).is_some_and(|d| d.kind == crate::classic::content::PolicyBranchKind::Ideology);
    // Started branches: the best adoptable policy, valued by the branch's rest.
    let mut started: Vec<(Milli, i64, PolicyId)> = Vec::new();
    let mut branches: Vec<PolicyBranchId> = culture.adoptable.iter().filter_map(|p| branch_of(*p)).collect();
    branches.sort_by_key(|b| b.0);
    branches.dedup();
    for b in branches {
        let fit = if is_ideology(b) { Some(super::weights::branch::IDEOLOGY_FIT) } else { fit_of(b) };
        let Some(fit) = fit else { continue };
        let pick = culture.adoptable.iter().copied().filter(|p| branch_of(*p) == Some(b)).max_by_key(|p| (worth(*p), std::cmp::Reverse(p.0)));
        let Some(pick) = pick else { continue };
        let value = if is_ideology(b) {
            worth(pick)
        } else {
            let rest: Vec<PolicyId> = rules
                .module
                .policies
                .iter()
                .enumerate()
                .filter(|(i, p)| p.branch == Some(b) && !model.policies.contains(&PolicyId(*i as u16)) && matches!(p.role, PolicyRole::Policy | PolicyRole::Finisher))
                .map(|(i, _)| PolicyId(i as u16))
                .collect();
            let left = rest.iter().filter(|p| rules.module.policies[usize::from(p.0)].role == PolicyRole::Policy).count().max(1) as i64;
            rest.iter().map(|p| worth(*p)).sum::<i64>() / left
        };
        started.push((fit, goal(b, value), pick));
    }
    // New branches: all they bring per step. With the science aim, once a
    // branch is open, the culture waits for a science branch whose era is
    // ahead (the book's measured wait: a filler branch cost 7 % of the
    // science, classic/docs/ai-science-audit.md).
    let wait = science_ahead(model, rules, plan, culture);
    let mut fresh: Vec<(Milli, i64, PolicyBranchId)> = Vec::new();
    for b in &culture.unlockable {
        let Some(fit) = fit_of(*b) else { continue };
        if wait && super::branches::dominant(&super::branches::character(rules, *b)) != Some(super::weights::branch::ch::SCIENCE) {
            continue;
        }
        let all: Vec<PolicyId> = rules.module.policies.iter().enumerate().filter(|(_, p)| p.branch == Some(*b)).map(|(i, _)| PolicyId(i as u16)).collect();
        let steps = all.iter().filter(|p| rules.module.policies[usize::from(p.0)].role != PolicyRole::Finisher).count().max(1) as i64;
        fresh.push((fit, goal(*b, all.iter().map(|p| worth(*p)).sum::<i64>() / steps), *b));
    }
    // The fit is the key (the owner's play); the cell's value of a step, as a
    // share of the best candidate's, adds at most `VALUE_WEIGHT` of it.
    let top = started.iter().map(|c| c.1).chain(fresh.iter().map(|c| c.1)).max().unwrap_or(0).max(1);
    let score = |fit: Milli, value: i64| fit * (ONE + w::VALUE_WEIGHT * value.clamp(0, top) / top) / ONE;
    let started = started.iter().map(|(f, v, p)| (score(*f, *v), *p)).max_by_key(|(s, p)| (*s, std::cmp::Reverse(p.0)));
    let fresh = fresh.iter().map(|(f, v, b)| (score(*f, *v), *b)).max_by_key(|(s, b)| (*s, std::cmp::Reverse(b.0)));
    match (started, fresh) {
        (Some((s, p)), Some((f, b))) => {
            if f * ONE > s * w::OPEN_MARGIN {
                Some(PolicyChoice::Unlock(b))
            } else {
                Some(PolicyChoice::Adopt(p))
            }
        }
        (Some((_, p)), None) => Some(PolicyChoice::Adopt(p)),
        (None, Some((_, b))) => Some(PolicyChoice::Unlock(b)),
        (None, None) => None,
    }
}

/// With the science aim, a normal branch open and a science branch (its
/// largest character) neither open nor unlockable now (its era ahead): the
/// culture waits for it.
pub fn science_ahead(model: &Model, rules: &CompiledRuleset, plan: &Plan, culture: &Culture) -> bool {
    use super::branches::{character, dominant, is_normal};
    if plan.intent.aim_pick != 0 || !model.branches.iter().any(|b| is_normal(rules, *b)) {
        return false;
    }
    rules.module.policy_branches.iter().enumerate().any(|(i, d)| {
        let b = PolicyBranchId(i as u16);
        is_normal(rules, b)
            && !model.branches.contains(&b)
            && !culture.unlockable.contains(&b)
            && !(d.locked_without_religion && !model.religion)
            && !d.mutually_exclusive_with.iter().any(|x| model.branches.contains(x))
            && dominant(&character(rules, b)) == Some(super::weights::branch::ch::SCIENCE)
    })
}

/// The culture decision of a turn.
pub fn policy(obs: &AIObservation<'_>, rules: &CompiledRuleset, prices: &Prices, plan: &Plan) -> Option<Command> {
    let player = obs.player;
    let pressure = plan.intent.concepts[kc::CULTURE_PRESSURE];
    if obs.tourism.opinion == crate::world::tourism::PublicOpinion::Unrest && obs.tourism.preferred.is_some() && pressure >= w::SWITCH_PRESSURE {
        return Some(Command::SwitchIdeology { player });
    }
    let mut culture = Culture { adoptable: obs.culture.adoptable.clone(), unlockable: obs.culture.unlockable.clone(), ideologies: obs.culture.ideologies.clone() };
    // The first branch waits for the style's lock (it is the style's).
    if !plan.intent.locked && !obs.culture.branches.iter().any(|b| super::branches::is_normal(rules, *b)) {
        culture.unlockable.retain(|b| !super::branches::is_normal(rules, *b));
    }
    if culture.adoptable.is_empty() && culture.unlockable.is_empty() && culture.ideologies.is_empty() {
        return None;
    }
    let model = Model::of_observation(obs, rules);
    let base = Base::of(prices);
    let choice = choose_policy(&model, rules, plan, &base, &culture)?;
    let command = match choice {
        PolicyChoice::Adopt(policy) => Command::AdoptPolicy { player, policy },
        PolicyChoice::Unlock(branch) => Command::UnlockPolicyBranch { player, branch },
        PolicyChoice::Ideology(ideology) => Command::ChooseIdeology { player, ideology },
    };
    super::trace::policy(player.0, obs.round, &command, rules);
    Some(command)
}

// ---- production ----

/// The kind of an order (`weights::head::KIND_ROWS`).
pub fn kind_of(rules: &CompiledRuleset, item: ProductionItem) -> usize {
    match item {
        ProductionItem::Unit(k) => match role_of(rules, k) {
            AiRole::Settler => 0,
            AiRole::Worker if crate::ai::sea_work::is_sea_builder(rules, k) => 2,
            AiRole::Worker => 1,
            _ if crate::derived::trade::is_trade_unit(rules, k) => 3,
            AiRole::Military | AiRole::Naval => 4,
            AiRole::Civilian => 5,
        },
        ProductionItem::Building(b) => match rules.buildings.get(usize::from(b.0)) {
            Some(d) if d.wonder && d.kind == crate::rules::content::BuildingKind::WorldWonder => 6,
            Some(d) if d.kind == crate::rules::content::BuildingKind::NationalWonder => 7,
            _ => 5,
        },
        ProductionItem::Project(_) => 5,
    }
}

/// The price dimension of a building effect's value (`pd`; None: the
/// allocator's own price).
fn effect_dim(modifier: &Modifier) -> Option<(usize, bool)> {
    use crate::classic::effects::ClassicModifier;
    let yield_dim = |s: usize| [pd::FOOD, pd::PRODUCTION, pd::GOLD, pd::SCIENCE, pd::CULTURE, pd::FAITH][s];
    match modifier {
        Modifier::AddYield { yield_kind, .. } | Modifier::AddYieldPerPopulation { yield_kind, .. } => Some((yield_dim(slot(*yield_kind)), false)),
        Modifier::AddYieldPercent { yield_kind, .. } => Some((yield_dim(slot(*yield_kind)), true)),
        Modifier::ModifyGrowth { .. } => Some((pd::FOOD, false)),
        Modifier::Module(ClassicModifier::AddHappiness { .. }) => Some((pd::HAPPINESS, false)),
        Modifier::Module(ClassicModifier::AddGreatPersonPoints { .. }) => Some((pd::GREAT_PEOPLE, false)),
        Modifier::Module(ClassicModifier::ModifyTourism { .. } | ClassicModifier::TourismToward { .. }) => Some((pd::TOURISM, false)),
        Modifier::ModifyCityCombat { .. } | Modifier::PassivePromotion(_) | Modifier::ModifyCombat { .. } | Modifier::GrantExperience(_) => Some((pd::MILITARY, false)),
        _ => None,
    }
}

/// The net's value of a building in a city against the allocator's: the
/// allocator's value of each effect (`Prices::effect_in`) at the net's
/// multiplier of its dimension (a percent also by the city's growth over
/// H), its upkeep at gold's, a world wonder's points at the wonder price;
/// (net, allocator) in centi-points.
pub fn building_parts(obs: &AIObservation<'_>, rules: &CompiledRuleset, prices: &Prices, plan: &Plan, city: &OwnCity, cities: i32, b: crate::ids::BuildingKindId) -> (i64, i64) {
    let Some(def) = rules.buildings.get(usize::from(b.0)) else { return (0, 0) };
    let (mut net, mut raw) = (0i64, 0i64);
    for e in super::cell::effects_of(&def.effects) {
        if !matches!(e.trigger, Trigger::Passive | Trigger::OnBuilt) {
            continue;
        }
        let v = prices.effect_in(obs, rules, city, e, cities);
        raw += v;
        let m = match effect_dim(&e.modifier) {
            Some((dim, percent)) => {
                let grown = if percent {
                    let s = match e.modifier {
                        Modifier::AddYieldPercent { yield_kind, .. } => slot(yield_kind),
                        _ => 0,
                    };
                    plan.growth_of(city.id.0, s)
                } else {
                    ONE
                };
                plan.mult[dim] * grown / ONE
            }
            None => ONE,
        };
        net += v * m / ONE;
    }
    let upkeep = i64::from(def.maintenance) * prices.gold;
    raw -= upkeep;
    net -= upkeep * plan.mult[pd::GOLD] / ONE;
    if def.wonder {
        raw += prices.score.wonder;
        net += prices.score.wonder * plan.mult[pd::WONDER] / ONE;
    }
    (net, raw)
}

/// One production candidate's score.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ItemScore {
    pub at: usize,
    pub value: i64,
    pub score: i64,
}

/// The style's correction of a kind of order (thousandths).
pub fn kind_correction(plan: &Plan, kind: usize) -> i64 {
    w::KIND_ROWS.iter().filter(|(_, k, _, _)| *k == kind).map(|(s, _, v, _)| plan.intent.style[*s] * v / ONE).sum()
}

/// The production choice of `city` among the allocator's options (an index
/// into them); `picked` the allocator's own; `military` an extra military
/// order and its value when the plan wants one.
#[allow(clippy::too_many_arguments)]
pub fn production(obs: &AIObservation<'_>, rules: &CompiledRuleset, prices: &Prices, plan: &Plan, city: &OwnCity, cities: i32, options: &[ProductionOption], picked: Option<usize>) -> Option<usize> {
    if options.is_empty() {
        return picked;
    }
    let mut set = crate::ai::choice::top(options.iter().map(|o| o.ratio));
    for (i, o) in options.iter().enumerate() {
        let goal = matches!(o.item, ProductionItem::Building(b) if plan.goal_of_building(b).is_some());
        if (goal || kind_of(rules, o.item) == 4) && !set.contains(&i) {
            set.push(i);
        }
    }
    // A goal of this city the allocator does not offer (the rules refuse
    // it now, or it is beyond the allocator's build-time limit): traced.
    for g in &plan.goals {
        if let super::GoalKind::Wonder { building, city: at } = g.kind {
            let known = rules.buildings.get(usize::from(building.0)).and_then(|d| d.required_tech).is_none_or(|t| obs.economy.techs.contains(&t));
            if at == city.id.0 && known && !options.iter().any(|o| o.item == ProductionItem::Building(building)) {
                super::trace::missing(obs.player.0, obs.round, city.id.0, building, rules);
            }
        }
    }
    let mut scores: Vec<ItemScore> = Vec::new();
    for &i in &set {
        let o = &options[i];
        let kind = kind_of(rules, o.item);
        let value = match o.item {
            ProductionItem::Building(b) => {
                let (net, raw) = building_parts(obs, rules, prices, plan, city, cities, b);
                if raw > 0 && net > 0 {
                    o.value * net / raw
                } else if raw > 0 {
                    0
                } else {
                    o.value
                }
            }
            _ => {
                let dim = match kind {
                    0 => pd::EXPANSION,
                    1 | 2 => pd::WORKER,
                    3 => pd::TRADE,
                    4 => pd::MILITARY,
                    _ => pd::PRODUCTION,
                };
                o.value * plan.mult[dim] / ONE
            }
        };
        let value = value * (ONE + kind_correction(plan, kind)) / ONE;
        let goal = match o.item {
            ProductionItem::Building(b) => plan.goal_of_building(b).map_or(0, |g| g.worth.max(0) * w::GOAL_ITEM / ONE),
            _ => 0,
        };
        let total = value + goal;
        scores.push(ItemScore { at: i, value: total, score: total * ONE / (i64::from(o.turns) + w::TURNS_PAD) });
    }
    let best = scores.iter().filter(|s| s.value > 0).max_by_key(|s| (s.score, std::cmp::Reverse(s.at)))?;
    super::trace::production(obs.player.0, obs.round, city.id.0, options[best.at].item, picked.map(|p| options[p].item), rules);
    Some(best.at)
}

/// A military order the plan wants beside the allocator's (its value):
/// the unit's hammers at `MILITARY_RETURN` times the military price's
/// excess over the allocator's, times how short the army is (the force
/// feature, the prepare and war postures, danger).
pub fn military_value(plan: &Plan, prices: &Prices, cost: u32) -> i64 {
    let excess = plan.mult[pd::MILITARY] - ONE;
    // In peace and in defence the war logic's own needs (garrisons, the
    // defence watch's defenders, operations) build the army; the plan adds
    // units only to prepare a war or to fight one.
    if excess <= 0 || plan.intent.posture_pick < 2 {
        return 0;
    }
    let posture = if plan.intent.posture_pick >= 2 { plan.intent.posture[2] + plan.intent.posture[3] } else { 0 };
    let need = (ONE - plan.features[super::weights::fe::FORCE]).max(posture).max(plan.intent.concepts[kc::DANGER]);
    let hammers = i64::from(cost) * prices.production / i64::from(prices.rounds_left.max(1));
    hammers * w::MILITARY_RETURN / ONE * excess / ONE * need / ONE
}
