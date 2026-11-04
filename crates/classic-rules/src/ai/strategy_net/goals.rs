//! The goal pointer: 3–5 goals chosen one at a time (autoregressively) from
//! the world wonders nobody built, the key buildings our cities lack, the
//! policy branches and the technologies that open a project. A candidate's
//! rate is its marginal value — the economy cell's run of the plan with it
//! (from the round it is expected) less the run without it, plus what its
//! other effects are worth (`effects`) — times its chance (the wonder race),
//! over the rounds to get it (its missing technologies at our beakers, then
//! its hammers or the culture of its policies) plus two. The best is taken,
//! added to the plan (its technologies count as planned for the rest: a
//! shared prerequisite shortens the next goal's path), and the rest are
//! rated again against the new plan, so two goals that feed each other —
//! growth, then more tiles of the kind another goal improves — are found
//! together. The review also sets the cell's food price and growth factors.

use super::cell::{food_price, run, CellPrices, Place, Plan as CellPlan};
use super::effects::Valuer;
use super::layers::ONE;
use super::model::Model;
use super::weights::{goal as w, head, kc, pd, FOOD_MAX, FOOD_MIN};
use super::{Base, Goal, GoalKind, Intent, Milli, Review};
use crate::ids::{BuildingKindId, PolicyId, TechId};
use crate::rules::content::{BuildingKind, PolicyBranchId, PolicyRole};
use crate::rules::CompiledRuleset;

/// The technologies missing on the way to `target` (its prerequisites
/// first, the target last), beside those known or planned.
pub fn path(rules: &CompiledRuleset, known: &[TechId], planned: &[TechId], target: TechId) -> Vec<TechId> {
    fn walk(rules: &CompiledRuleset, known: &[TechId], planned: &[TechId], t: TechId, out: &mut Vec<TechId>, depth: u32) {
        if depth > 64 || known.contains(&t) || planned.contains(&t) || out.contains(&t) {
            return;
        }
        if let Some(def) = rules.technologies.get(usize::from(t.0)) {
            for r in &def.requires {
                walk(rules, known, planned, *r, out, depth + 1);
            }
        }
        out.push(t);
    }
    let mut out = Vec::new();
    walk(rules, known, planned, target, &mut out, 0);
    out
}

/// Beakers of a set of technologies at the game's speed.
pub fn beakers(rules: &CompiledRuleset, techs: &[TechId], science_percent: i64) -> i64 {
    techs.iter().filter_map(|t| rules.technologies.get(usize::from(t.0))).map(|d| i64::from(d.cost)).sum::<i64>() * science_percent / 100
}

/// What a candidate adds to the plan.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Apply {
    Building { city: usize, building: BuildingKindId },
    Everywhere { building: BuildingKindId, cities: Vec<usize> },
    Policies(Vec<PolicyId>),
    Tech,
}

#[derive(Debug, Clone)]
struct Candidate {
    kind: GoalKind,
    apply: Apply,
    /// The technology it needs (None: none).
    tech: Option<TechId>,
    /// Rounds beyond its technologies (hammers, culture).
    build: i64,
    /// What the classifier values it at within H.
    extra: i64,
    /// Chance to be ours (thousandths).
    chance: i64,
    /// Content era (wonders: the race).
    era: usize,
}

/// The era of a building (its own, else its technology's).
fn building_era(rules: &CompiledRuleset, b: BuildingKindId) -> usize {
    let Some(def) = rules.buildings.get(usize::from(b.0)) else { return 0 };
    def.era
        .or_else(|| def.required_tech.and_then(|t| rules.technologies.get(usize::from(t.0))).and_then(|t| t.content_era))
        .map(|e| super::features::era_index(rules, e))
        .unwrap_or(0)
}

/// A building the model may plan: buildable by this nation (its unique
/// replacement instead of the generic one), not obsolete.
fn buildable(model: &Model, rules: &CompiledRuleset, b: BuildingKindId) -> bool {
    let Some(def) = rules.buildings.get(usize::from(b.0)) else { return false };
    if def.unbuildable || def.nation.is_some_and(|n| n != model.nation) || def.obsolete_tech.is_some_and(|t| model.techs.contains(&t)) {
        return false;
    }
    let replaced = rules.buildings.iter().any(|o| o.replaces == Some(b) && o.nation == Some(model.nation));
    if replaced {
        return false;
    }
    if let Some(branch) = def.module.requires_policy_branch {
        if !model.branches.contains(&branch) {
            return false;
        }
    }
    true
}

/// A building with an effect the cell models on a city's growth or tiles
/// (a percent, a yield per citizen, a tile yield, growth): a key building.
fn key_building(rules: &CompiledRuleset, b: BuildingKindId) -> bool {
    use crate::rules::effects::Modifier;
    let Some(def) = rules.buildings.get(usize::from(b.0)) else { return false };
    super::cell::effects_of(&def.effects).any(|e| {
        matches!(e.modifier, Modifier::AddYieldPercent { .. } | Modifier::AddYieldPerPopulation { .. } | Modifier::ModifyGrowth { .. })
            || matches!((e.scope, e.modifier), (crate::rules::effects::Scope::Tile, Modifier::AddYield { .. }))
    })
}

/// The policies of a branch not adopted (the opener while the branch is
/// closed).
fn branch_rest(model: &Model, rules: &CompiledRuleset, branch: PolicyBranchId) -> Vec<PolicyId> {
    let open = model.branches.contains(&branch);
    rules
        .module
        .policies
        .iter()
        .enumerate()
        .filter(|(i, p)| p.branch == Some(branch) && !model.policies.contains(&PolicyId(*i as u16)) && (!open || p.role != PolicyRole::Opener))
        .map(|(i, _)| PolicyId(i as u16))
        .collect()
}

/// Rounds for a technology path and the beakers of the empire.
fn tech_turns(model: &Model, rules: &CompiledRuleset, techs: &[TechId]) -> i64 {
    beakers(rules, techs, model.science_percent) / model.science.max(1)
}

/// Rounds a city builds `b` in.
fn build_turns(model: &Model, rules: &CompiledRuleset, city: usize, b: BuildingKindId) -> i64 {
    let cost = rules.buildings.get(usize::from(b.0)).map_or(0, |d| i64::from(d.cost));
    cost * model.pace / 100 / model.cities[city].production.max(1)
}

/// Every candidate goal of the empire.
fn candidates(model: &Model, rules: &CompiledRuleset, intent: &Intent, valuer: &Valuer<'_>) -> Vec<Candidate> {
    let mut out = Vec::new();
    let capital = model.capital();
    // The cities a wonder may stand in: the capital and the two most
    // productive others.
    let mut builders: Vec<usize> = (0..model.cities.len()).filter(|i| *i != capital).collect();
    builders.sort_by_key(|i| (std::cmp::Reverse(model.cities[*i].production), *i));
    builders.truncate(2);
    builders.insert(0, capital);
    let owned = |b: BuildingKindId| model.cities.iter().any(|c| c.buildings.contains(&b));
    for (index, def) in rules.buildings.iter().enumerate() {
        let b = BuildingKindId(index as u16);
        if !buildable(model, rules, b) {
            continue;
        }
        // Reach is the horizon's: a goal whose technologies and hammers take
        // longer than H is worth nothing in the cell (`rate`).
        let era = building_era(rules, b);
        if def.wonder && def.kind == BuildingKind::WorldWonder {
            if model.built_wonders.contains(&b) || owned(b) {
                continue;
            }
            for &city in &builders {
                if !model.placement_ok(rules, city, b) {
                    continue;
                }
                let rivals = model.rival_eras.iter().filter(|e| **e >= era).count() as i64;
                let concept = intent.concepts[kc::WONDER_CHANCE];
                let build = build_turns(model, rules, city, b);
                let chance_base = w::CHANCE_BASE + w::CHANCE_PER_RIVAL * rivals + w::CHANCE_CONCEPT * concept / ONE;
                out.push(Candidate {
                    kind: GoalKind::Wonder { building: b, city: model.cities[city].id },
                    apply: Apply::Building { city, building: b },
                    tech: def.required_tech,
                    build,
                    extra: valuer.building(b, Place::City(city)),
                    chance: chance_base,
                    era,
                });
            }
        } else if matches!(def.kind, BuildingKind::Regular | BuildingKind::Unique) && key_building(rules, b) {
            let cities: Vec<usize> = (0..model.cities.len())
                .filter(|i| {
                    let c = &model.cities[*i];
                    !c.buildings.contains(&b) && def.requires_buildings.iter().all(|r| c.buildings.contains(r)) && model.placement_ok(rules, *i, b)
                })
                .collect();
            if cities.is_empty() {
                continue;
            }
            let build = cities.iter().map(|i| build_turns(model, rules, *i, b)).sum::<i64>() / cities.len() as i64;
            let extra = cities.iter().map(|i| valuer.building(b, Place::City(*i))).sum();
            out.push(Candidate { kind: GoalKind::Building(b), apply: Apply::Everywhere { building: b, cities }, tech: def.required_tech, build, extra, chance: ONE, era });
        }
    }
    for (index, def) in rules.module.policy_branches.iter().enumerate() {
        let branch = PolicyBranchId(index as u16);
        if def.kind == crate::classic::content::PolicyBranchKind::Ideology {
            continue;
        }
        if def.locked_without_religion && !model.religion {
            continue;
        }
        if !model.branches.contains(&branch) && def.mutually_exclusive_with.iter().any(|x| model.branches.contains(x)) {
            continue;
        }
        // The branch's fit to the intent multiplies its worth (`branches`);
        // a shut branch (the war branch outside war) is no goal.
        let Some(fit) = super::branches::branch_fit(rules, intent, branch) else { continue };
        let era = def.era_prerequisite.map_or(0, |e| super::features::era_index(rules, e));
        if era > model.era + 1 {
            continue;
        }
        let rest = branch_rest(model, rules, branch);
        if rest.is_empty() {
            continue;
        }
        let steps = rest.iter().filter(|p| rules.module.policies[usize::from(p.0)].role != PolicyRole::Finisher).count() as i64;
        // Each adoption raises the next policy's cost: about 10 % a step.
        let culture = model.policy_cost * steps * (100 + 10 * steps) / 100;
        let mut build = culture / model.culture.max(1);
        if era > model.era {
            // The era's cheapest technology first.
            let cheapest = rules
                .technologies
                .iter()
                .enumerate()
                .filter(|(_, t)| t.content_era.is_some_and(|e| super::features::era_index(rules, e) >= era))
                .map(|(i, _)| TechId(i as u16))
                .min_by_key(|t| (beakers(rules, &path(rules, &model.techs, &[], *t), model.science_percent), t.0));
            match cheapest {
                Some(t) => build += tech_turns(model, rules, &path(rules, &model.techs, &[], t)),
                None => continue,
            }
        }
        let extra = rest.iter().map(|p| valuer.set(&rules.module.policies[usize::from(p.0)].effects, Place::Empire)).sum();
        out.push(Candidate { kind: GoalKind::Branch(branch), apply: Apply::Policies(rest), tech: None, build, extra, chance: fit, era });
    }
    // Technologies that open a project (the spaceship's), for the science aim.
    let science = intent.aim[0];
    if science >= 300 {
        for (index, def) in rules.technologies.iter().enumerate() {
            let t = TechId(index as u16);
            if def.unlocks_projects.is_empty() || model.techs.contains(&t) {
                continue;
            }
            let extra = w::PROJECT_TECH_WONDERS * valuer.base.wonder * science / ONE;
            out.push(Candidate { kind: GoalKind::Tech(t), apply: Apply::Tech, tech: Some(t), build: 0, extra, chance: ONE, era: model.era });
        }
    }
    out
}

/// When a candidate is done against the plan's queues: (its round, the
/// round its technologies are known). Its technologies are researched after
/// the plan's (`research_free`); its hammers start when its city is free and
/// its technology known; its policies after the plan's (`culture_free`).
fn timing(model: &Model, rules: &CompiledRuleset, plan: &CellPlan, c: &Candidate, way: &[TechId]) -> (i64, i64) {
    let known = if way.is_empty() { 0 } else { plan.research_free + tech_turns(model, rules, way) };
    let done = match &c.apply {
        Apply::Building { city, .. } => known.max(plan.city_free[*city]) + c.build,
        Apply::Everywhere { cities, .. } => {
            let free = cities.iter().map(|i| plan.city_free[*i]).sum::<i64>() / cities.len().max(1) as i64;
            known.max(free) + c.build
        }
        Apply::Policies(_) => known.max(plan.culture_free) + c.build,
        Apply::Tech => known,
    };
    (done, known)
}

/// The plan's queues after a candidate done at `done` (its technologies
/// known at `known`).
fn occupy(plan: &mut CellPlan, c: &Candidate, done: i64, known: i64) {
    plan.research_free = plan.research_free.max(known);
    match &c.apply {
        Apply::Building { city, .. } => plan.city_free[*city] = done,
        Apply::Everywhere { cities, building: _ } => {
            for i in cities {
                plan.city_free[*i] = plan.city_free[*i].max(done);
            }
        }
        Apply::Policies(_) => plan.culture_free = done,
        Apply::Tech => {}
    }
}

/// The plan with a candidate in it from round `from`.
fn applied(model: &Model, rules: &CompiledRuleset, plan: &CellPlan, c: &Candidate, from: i64, techs: &[TechId]) -> CellPlan {
    let mut next = plan.clone();
    match &c.apply {
        Apply::Building { city, building } => next.add_building(model, rules, *city, *building, from),
        Apply::Everywhere { building, cities } => {
            for i in cities {
                next.add_building(model, rules, *i, *building, from);
            }
        }
        Apply::Policies(policies) => {
            // The policies come one by one over the rounds to get them all:
            // on average at half of them.
            for p in policies {
                next.add(model, rules, &rules.module.policies[usize::from(p.0)].effects, Place::Empire, from / 2);
            }
        }
        Apply::Tech => {}
    }
    next.techs.extend_from_slice(techs);
    next
}

/// A candidate's rating against a plan: (worth, rate, rounds, path).
type Rating = (i64, i64, i64, Vec<TechId>);

/// A candidate rated against `plan`.
#[allow(clippy::too_many_arguments)]
fn rate(model: &Model, rules: &CompiledRuleset, prices: &CellPrices, plan: &CellPlan, base: i64, c: &Candidate, horizon: i64, rounds_left: i64) -> Rating {
    let way = c.tech.map_or_else(Vec::new, |t| path(rules, &model.techs, &plan.techs, t));
    let turns = timing(model, rules, plan, c, &way).0;
    let gained = if turns < horizon { run(model, &applied(model, rules, plan, c, turns, &way), prices, horizon).value - base } else { 0 };
    let chance = match c.kind {
        GoalKind::Wonder { .. } => {
            let standard = turns * 100 / model.pace.max(1);
            (c.chance + w::CHANCE_PER_ROUND * standard).clamp(w::CHANCE_MIN, w::CHANCE_MAX)
        }
        _ => c.chance,
    };
    // Both to the end of the game (the allocator's measure): the cell's gain
    // within H extrapolated, the other effects from the round the goal comes.
    let extra = c.extra * (rounds_left - turns).max(0) / rounds_left.max(1);
    let worth = (gained + extra) * chance / ONE;
    let rate = worth * ONE / (turns + head::TURNS_PAD);
    (worth, rate, turns, way)
}

/// The review: the cell's food price (into `mult`), the growth factors,
/// and the goals.
pub fn review(model: &Model, rules: &CompiledRuleset, intent: &Intent, mult: &mut [Milli; pd::N], base: &Base, round: u32, at_war: bool) -> Review {
    let horizon = i64::from(intent.horizon.max(1));
    let prices = CellPrices::of(base, mult);
    let empty = CellPlan::new(model);
    let food = food_price(model, &empty, &prices, horizon);
    mult[pd::FOOD] = if base.food > 0 { (food * ONE / base.food).clamp(FOOD_MIN, FOOD_MAX) } else { ONE };
    let outcome = run(model, &empty, &prices, horizon);
    let growth = model
        .cities
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let mut g = [ONE; 6];
            for (s, v) in g.iter_mut().enumerate() {
                if outcome.first[i][s] > 0 {
                    *v = (outcome.mean[i][s] * ONE / outcome.first[i][s]).clamp(ONE, 4 * ONE);
                }
            }
            (c.id, g)
        })
        .collect();
    let goals = choose(model, rules, intent, mult, base);
    Review { round, cities: model.cities.len() as u32, at_war, food: mult[pd::FOOD], goals, growth }
}

/// Every candidate against the empire as it is: (goal, worth, rate,
/// rounds), best rate first (a diagnostic of the pointer's screening).
pub fn ranked(model: &Model, rules: &CompiledRuleset, intent: &Intent, mult: &[Milli; pd::N], base: &Base) -> Vec<(GoalKind, i64, i64, i64)> {
    let horizon = i64::from(intent.horizon.max(1));
    let prices = CellPrices::of(base, mult);
    let rounds_left = base.rounds_left.max(1);
    let valuer = Valuer { model, rules, base, mult, horizon: rounds_left, room: i64::from(intent.more_cities), fight: super::heads::fight(intent) };
    let plan = CellPlan::new(model);
    let value = run(model, &plan, &prices, horizon).value;
    let mut out: Vec<(GoalKind, i64, i64, i64)> = candidates(model, rules, intent, &valuer)
        .iter()
        .map(|c| {
            let (worth, r, turns, _) = rate(model, rules, &prices, &plan, value, c, horizon, rounds_left);
            (c.kind, worth, r, turns)
        })
        .collect();
    out.sort_by_key(|x| std::cmp::Reverse(x.2));
    out
}

/// The pointer: the goals, best first.
pub fn choose(model: &Model, rules: &CompiledRuleset, intent: &Intent, mult: &[Milli; pd::N], base: &Base) -> Vec<Goal> {
    if model.cities.is_empty() {
        return Vec::new();
    }
    let horizon = i64::from(intent.horizon.max(1));
    let prices = CellPrices::of(base, mult);
    let rounds_left = base.rounds_left.max(1);
    let valuer = Valuer { model, rules, base, mult, horizon: rounds_left, room: i64::from(intent.more_cities), fight: super::heads::fight(intent) };
    let mut plan = CellPlan::new(model);
    let mut value = run(model, &plan, &prices, horizon).value;
    let all = candidates(model, rules, intent, &valuer);
    // Screening: the best by their rate against the empire as it is. Their
    // ratings serve the first choice, which rates them against the same plan.
    let mut rated: Vec<(Rating, usize)> = all.iter().enumerate().map(|(i, c)| (rate(model, rules, &prices, &plan, value, c, horizon, rounds_left), i)).collect();
    rated.sort_by_key(|(r, i)| (std::cmp::Reverse(r.1), *i));
    rated.truncate(w::CANDIDATES);
    let mut pool: Vec<(Candidate, Option<Rating>)> = rated.into_iter().filter(|(r, _)| r.1 > 0).map(|(r, i)| (all[i].clone(), Some(r))).collect();
    let mut goals: Vec<Goal> = Vec::new();
    let mut first = 0;
    // Key buildings ride along: they take at most `MAX_BUILDINGS` places of
    // their own beside the `MAX` goals (wonders, branches, technologies).
    let is_building = |k: GoalKind| matches!(k, GoalKind::Building(_));
    let big = |goals: &[Goal]| goals.iter().filter(|g| !is_building(g.kind)).count();
    let small = |goals: &[Goal]| goals.iter().filter(|g| is_building(g.kind)).count();
    while big(&goals) < w::MAX && !pool.is_empty() {
        if small(&goals) >= w::MAX_BUILDINGS {
            pool.retain(|(c, _)| !is_building(c.kind));
        }
        let mut best: Option<(i64, i64, i64, Vec<TechId>, usize)> = None;
        for (i, (c, screened)) in pool.iter_mut().enumerate() {
            let (worth, r, turns, way) = screened.take().unwrap_or_else(|| rate(model, rules, &prices, &plan, value, c, horizon, rounds_left));
            if best.as_ref().is_none_or(|b| r > b.1) {
                best = Some((worth, r, turns, way, i));
            }
        }
        let Some((worth, r, turns, way, at)) = best else { break };
        if r <= 0 || big(&goals) >= w::MIN && r * ONE < first * w::STOP_SHARE {
            break;
        }
        if goals.is_empty() {
            first = r;
        }
        let (c, _) = pool.remove(at);
        let (_, known) = timing(model, rules, &plan, &c, &way);
        plan = applied(model, rules, &plan, &c, turns, &way);
        occupy(&mut plan, &c, turns, known);
        value = run(model, &plan, &prices, horizon).value;
        // A wonder is one goal whatever its city; a branch excludes its rivals.
        pool.retain(|(o, _)| match (o.kind, c.kind) {
            (GoalKind::Wonder { building: a, .. }, GoalKind::Wonder { building: b, .. }) => a != b,
            (GoalKind::Branch(a), GoalKind::Branch(b)) => rules.module.policy_branches.get(usize::from(a.0)).is_none_or(|d| !d.mutually_exclusive_with.contains(&b)),
            _ => true,
        });
        let _ = c.era;
        goals.push(Goal { kind: c.kind, worth, rate: r, turns, path: way });
    }
    goals
}
