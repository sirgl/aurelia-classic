// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
//
// Derived from Unciv (https://github.com/yairm210/Unciv), tag 4.22.10, MPL-2.0:
// core/src/com/unciv/logic/automation/civilization/ReligionAutomation.kt (`spendFaithOnReligion`,
// `rateBelief`, `chooseReligiousBeliefs`) and unit/ReligiousUnitAutomation.kt
// (`automateMissionary`, `automateInquisitor`, `foundReligion`, `enhanceReligion`).

//! Unciv's religion: faith is saved for the religion, then spent on great
//! prophets until the religion is enhanced, on missionaries and inquisitors
//! while our cities lack its majority, on a guard of the holy city and four
//! missionaries; beliefs are rated by the yields they add on the tiles and in
//! the cities of the empire (Unciv's `rateBelief`, its unique types read as the
//! classic rules' compiled effects); prophets found and enhance in a city,
//! missionaries spread to our cities first, inquisitors win them back.

use crate::ai::planner::{Decision, PlanTools};
use crate::ai::trace::{Actor, Reason};
use crate::classic::effects::GreatPersonKind;
use crate::command::Command;
use crate::ids::{BeliefId, ProductionItem, TileId, UnitKindId};
use crate::knowledge::observation::AIObservation;
use crate::knowledge::public::PlayerQueries;
use crate::rules::effects::{CompiledEffect, Condition, Modifier, Scope};
use crate::rules::CompiledRuleset;
use crate::world::religion::Creed;

use super::head_towards::head_towards;
use super::next_turn::draw;
use super::sight::{actor, Unit};
use super::Step;

/// The yields of an effect's modifier, summed (Unciv's `stats.sum()`).
fn yield_sum(modifier: &Modifier) -> f32 {
    match *modifier {
        Modifier::AddYield { amount, .. } => amount as f32,
        Modifier::AddYieldPerCount { amount, .. } => amount as f32,
        _ => 0.0,
    }
}

/// Does a tile effect's condition hold on the known tile?
fn tile_matches(obs: &AIObservation<'_>, rules: &CompiledRuleset, condition: &Condition, tile: TileId) -> bool {
    let known = obs.view.tile(tile);
    let Some(s) = known.snapshot() else {
        return false;
    };
    match *condition {
        Condition::Always => true,
        Condition::TileTerrain(kind) => rules.terrains.get(kind.0 as usize).and_then(|t| t.map_code) == Some(s.terrain.0),
        Condition::TileFeature(f) => s.feature == Some(f),
        Condition::TileResource(r) => s.resource == Some(r),
        Condition::TileImprovement(i) => s.improvement == Some(i) || s.resource.is_some_and(|r| rules.improvements.get(i.0 as usize).is_some_and(|d| d.resources.contains(&r))),
        _ => false,
    }
}

/// `rateBelief`: the yields the belief adds on the tiles of our cities (a
/// worked tile 1, a tile of the city 0.7, another 0.5), in our cities, and
/// its empire-wide effects; a pantheon belief ×0.9.
pub fn rate_belief(obs: &AIObservation<'_>, rules: &CompiledRuleset, belief: BeliefId) -> f32 {
    let Some(def) = rules.module.beliefs.get(belief.0 as usize) else {
        return 0.0;
    };
    let dims = obs.view.dimensions();
    let effects: Vec<&CompiledEffect> = super::construction::all_effects(&def.effects).collect();
    let mut score = 0.0;
    for city in &obs.own_cities {
        for tile in crate::math::hex_disc(dims, city.tile, 3) {
            let weight = if city.worked.contains(&tile) {
                1.0
            } else if obs.view.tile(tile).snapshot().is_some_and(|s| s.owner == Some(obs.player)) {
                0.7
            } else {
                0.5
            };
            for e in effects.iter().filter(|e| e.scope == Scope::Tile) {
                if !tile_matches(obs, rules, &e.condition, tile) {
                    continue;
                }
                // Forest pantheons are bad: we want to remove the forests (unless a resource is there).
                if let Condition::TileFeature(f) = e.condition {
                    let chopped = rules.features.get(f.0 as usize).and_then(|d| d.removal).is_some_and(|r| r.production > 0);
                    if chopped && obs.view.tile(tile).snapshot().is_none_or(|s| s.resource.is_none()) {
                        continue;
                    }
                }
                score += yield_sum(&e.modifier) * weight;
            }
        }
        for e in effects.iter().filter(|e| e.scope == Scope::City) {
            let modifier = if e.condition == Condition::Always { 1.0 } else { 0.5 };
            score += modifier
                * match e.modifier {
                    Modifier::ModifyGrowth { percent } => percent as f32 / 3.0,
                    Modifier::AddYieldPercent { percent, .. } => percent as f32 / 3.0,
                    Modifier::ModifyCost { percent, .. } => -percent as f32 / 3.0,
                    Modifier::ModifyCityCombat { amount, .. } => amount as f32 / 10.0,
                    Modifier::AddYieldPerPopulation { hundredths, .. } => hundredths as f32 * city.population as f32 / 100.0,
                    Modifier::AddYieldPercentPerCount { percent, .. } => percent as f32 * city.population as f32,
                    ref m => yield_sum(m),
                };
        }
    }
    for e in effects.iter().filter(|e| e.scope != Scope::Tile && e.scope != Scope::City) {
        score += match e.modifier {
            Modifier::ModifyCombat { percent, .. } => percent as f32 * 3.0,
            ref m => yield_sum(m) * 2.0,
        };
    }
    if def.category == Some(crate::classic::content::BeliefCategory::Pantheon) {
        score *= 0.9;
    }
    score
}

fn best_belief(obs: &AIObservation<'_>, rules: &CompiledRuleset, pool: &[BeliefId], exclude: &[BeliefId]) -> Option<BeliefId> {
    pool.iter().copied().filter(|b| !exclude.contains(b)).max_by(|a, b| rate_belief(obs, rules, *a).total_cmp(&rate_belief(obs, rules, *b)).then(b.0.cmp(&a.0)))
}

/// The player's own founded religion.
fn our_religion<'o>(obs: &'o AIObservation<'_>) -> Option<&'o crate::derived::religion::ReligionSummary> {
    let religion = obs.faith.overview.religion?;
    obs.faith.overview.religions.iter().find(|r| r.religion == religion)
}

/// `choosePantheon`, the free beliefs (the Reformation's), and the beliefs the
/// player chooses at once.
pub fn choose_beliefs(obs: &AIObservation<'_>, rules: &CompiledRuleset) -> Vec<Decision> {
    let mut out = Vec::new();
    let overview = &obs.faith.overview;
    if overview.can_found_pantheon {
        if let Some(belief) = best_belief(obs, rules, &overview.free_pantheon, &[]) {
            let command = Command::FoundPantheon { player: obs.player, belief };
            if obs.view.accepts(&command) {
                out.push(Decision::act(Actor::Player, Reason::SystemAdvisor, command));
            }
        }
    }
    if overview.can_add_reformation {
        if let Some(belief) = best_belief(obs, rules, &overview.free_reformation, &[]) {
            let command = Command::AddReformationBelief { player: obs.player, belief };
            if obs.view.accepts(&command) {
                out.push(Decision::act(Actor::Player, Reason::SystemAdvisor, command));
            }
        }
    }
    out
}

fn prophet_kind(rules: &CompiledRuleset) -> Option<UnitKindId> {
    rules.module.great_people.iter().find(|g| g.kind == GreatPersonKind::Prophet).map(|g| g.unit)
}

/// The cheapest own city where `kind` can be bought with faith now, and its price.
fn cheapest_purchase(obs: &AIObservation<'_>, kind: UnitKindId, filter: impl Fn(&crate::knowledge::observation::OwnCity) -> bool) -> Option<(crate::ids::CityHandle, u32)> {
    obs.own_cities
        .iter()
        .filter(|c| filter(c))
        .filter_map(|c| obs.view.faith_price(c.handle, ProductionItem::Unit(kind)).map(|p| (c.handle, p)))
        .filter(|(_, p)| *p <= obs.faith.overview.faith)
        .min_by_key(|(c, p)| (*p, c.index))
}

fn purchase(obs: &AIObservation<'_>, city: crate::ids::CityHandle, kind: UnitKindId) -> Option<Decision> {
    let command = Command::PurchaseWithFaith { player: obs.player, city, item: ProductionItem::Unit(kind) };
    obs.view.accepts(&command).then(|| Decision::act(Actor::Player, Reason::SystemAdvisor, command))
}

/// `spendFaithOnReligion`.
pub fn spend_faith(obs: &AIObservation<'_>, rules: &CompiledRuleset) -> Vec<Decision> {
    let overview = &obs.faith.overview;
    if obs.own_cities.is_empty() || obs.view.own_kind() != crate::world::player::PlayerKind::Major {
        return Vec::new();
    }
    let ours = our_religion(obs);
    let enhanced = ours.is_some_and(|r| r.enhanced);
    // Save for a great prophet until the religion is enhanced.
    if !enhanced && (overview.religions_left != 0 || ours.is_some()) {
        if ours.is_none() {
            return Vec::new();
        }
        let Some(prophet) = prophet_kind(rules) else {
            return Vec::new();
        };
        return cheapest_purchase(obs, prophet, |_| true).and_then(|(c, _)| purchase(obs, c, prophet)).into_iter().collect();
    }
    let Some(ours) = ours else {
        // No religion left to found: great people with faith.
        let mut out = Vec::new();
        for gp in rules.module.great_people.iter().filter(|g| g.kind != GreatPersonKind::Prophet) {
            if let Some((c, _)) = cheapest_purchase(obs, gp.unit, |_| true) {
                out.extend(purchase(obs, c, gp.unit));
                break;
            }
        }
        return out;
    };
    let religion = ours.religion;
    let majority_ours = |handle: crate::ids::CityHandle| obs.faith.cities.iter().any(|v| v.handle == handle && v.majority == Some(Creed::Religion(religion)));
    let religious_units = obs.faith.units.len();
    let without: Vec<&crate::knowledge::observation::OwnCity> = obs.own_cities.iter().filter(|c| !majority_ours(c.handle)).collect();
    let missionary = crate::derived::religion::missionary_kind(rules);
    let inquisitor = crate::derived::religion::inquisitor_kind(rules);
    if without.len() > 4 * religious_units {
        if let Some(m) = missionary {
            if let Some((c, _)) = cheapest_purchase(obs, m, |c| majority_ours(c.handle)) {
                return purchase(obs, c, m).into_iter().collect();
            }
        }
        return Vec::new();
    }
    let holy = ours.holy_city;
    let guards = obs.own_units.iter().filter(|u| Some(u.kind) == inquisitor).count();
    if obs.own_cities.iter().any(|c| c.handle == holy) && guards == 0 {
        if let Some(i) = inquisitor {
            let holy_tile = obs.own_cities.iter().find(|c| c.handle == holy).map(|c| c.tile);
            let dims = obs.view.dimensions();
            let near = obs
                .own_cities
                .iter()
                .filter(|c| majority_ours(c.handle))
                .filter_map(|c| obs.view.faith_price(c.handle, ProductionItem::Unit(i)).filter(|p| *p <= overview.faith).map(|_| c))
                .min_by_key(|c| holy_tile.and_then(|h| dims.distance(h, c.tile)).unwrap_or(i32::MAX));
            if let Some(c) = near {
                return purchase(obs, c.handle, i).into_iter().collect();
            }
        }
        return Vec::new();
    }
    let missionaries = obs.own_units.iter().filter(|u| Some(u.kind) == missionary).count();
    if missionaries < 4 {
        if let Some(m) = missionary {
            if let Some((c, _)) = cheapest_purchase(obs, m, |c| majority_ours(c.handle)) {
                return purchase(obs, c, m).into_iter().collect();
            }
        }
    }
    Vec::new()
}

/// A unit kind `automate_religious_unit` decides: a great prophet (or its
/// replacement), a missionary, an inquisitor.
pub fn religious_kind(rules: &CompiledRuleset, kind: UnitKindId) -> bool {
    let prophet = prophet_kind(rules).is_some_and(|p| p == kind || rules.units.get(kind.0 as usize).is_some_and(|d| d.replaces == Some(p)));
    prophet || crate::derived::religion::missionary_kind(rules) == Some(kind) || crate::derived::religion::inquisitor_kind(rules) == Some(kind)
}

/// The religious units of Unciv's automation: a prophet founds or enhances,
/// then spreads; a missionary spreads; an inquisitor removes heresy or guards.
/// None: not a religious unit.
pub fn automate_religious_unit(u: &Unit<'_, '_>, tools: &mut PlanTools<'_>) -> Option<Decision> {
    let obs_faith = &u.sight.faith;
    let rules = u.rules();
    let kind = u.me.kind;
    let prophet = prophet_kind(rules).is_some_and(|p| p == kind || rules.units.get(kind.0 as usize).is_some_and(|d| d.replaces == Some(p)));
    let missionary = crate::derived::religion::missionary_kind(rules) == Some(kind);
    let inquisitor = crate::derived::religion::inquisitor_kind(rules) == Some(kind);
    if !prophet && !missionary && !inquisitor {
        return None;
    }
    let step = if prophet {
        prophet_step(u, obs_faith, tools)
    } else if missionary {
        missionary_step(u, obs_faith, tools)
    } else {
        inquisitor_step(u, obs_faith, tools)
    };
    Some(match step {
        Step::Act(d) => d,
        Step::Done(r) => Decision::refuse(actor(&u.me), r),
        Step::Next => Decision::refuse(actor(&u.me), Reason::Idle),
    })
}

fn prophet_step(u: &Unit<'_, '_>, faith: &crate::knowledge::observation::FaithObservation, tools: &mut PlanTools<'_>) -> Step {
    let overview = &faith.overview;
    let view = &u.local().view;
    let rules = u.rules();
    let ours = overview.religion.and_then(|r| overview.religions.iter().find(|s| s.religion == r));
    let holy_cities: Vec<crate::ids::CityHandle> = overview.religions.iter().map(|r| r.holy_city).collect();
    let here_city = u.sight.own_cities.iter().find(|c| c.tile == u.tile()).copied();
    if ours.is_none() && !overview.free_religions.is_empty() {
        // `foundReligion`: in a city that is no holy city.
        let city = here_city.filter(|c| !holy_cities.contains(&c.handle)).or_else(|| u.sight.own_cities.iter().copied().find(|c| !holy_cities.contains(&c.handle) && u.can_move_to(c.tile) && u.can_reach(tools, c.tile)));
        let Some(city) = city else {
            return Step::Done(Reason::Idle);
        };
        if city.tile != u.tile() {
            return head_towards(u, tools, city.tile, Reason::MoveToJob);
        }
        let religion = overview.free_religions[draw(u.sight.round, u.me.id.0, 0x7265, overview.free_religions.len())];
        let founder = rank_free(u, &overview.free_founder, &[]);
        let follower = rank_free(u, &overview.free_follower, &[]);
        let pantheon = if overview.pantheon.is_none() { rank_free(u, &overview.free_pantheon, &[]) } else { None };
        let bonus = if overview.extra_founding_beliefs > 0 { rank_free(u, &overview.free_follower, &follower.into_iter().collect::<Vec<_>>()) } else { None };
        if let (Some(founder), Some(follower)) = (founder, follower) {
            let command = Command::FoundReligion { unit: u.me.handle, religion, founder, follower, pantheon, bonus };
            if view.accepts(&command) {
                return Step::Act(Decision::act(actor(&u.me), Reason::SystemAdvisor, command));
            }
        }
        return Step::Done(Reason::Idle);
    }
    if ours.is_some_and(|r| !r.enhanced) {
        // `enhanceReligion`: in a city.
        if here_city.is_none() {
            let nearest = u.sight.own_cities.iter().map(|c| c.tile).filter(|t| u.can_reach(tools, *t)).min_by_key(|t| (u.distance(*t, u.tile()), *t));
            return match nearest {
                Some(t) => head_towards(u, tools, t, Reason::MoveToJob),
                None => Step::Done(Reason::Idle),
            };
        }
        let follower = rank_free(u, &overview.free_follower, &[]);
        let enhancer = rank_free(u, &overview.free_enhancer, &[]);
        if let (Some(follower), Some(enhancer)) = (follower, enhancer) {
            let command = Command::EnhanceReligion { unit: u.me.handle, follower, enhancer };
            if view.accepts(&command) {
                return Step::Act(Decision::act(actor(&u.me), Reason::SystemAdvisor, command));
            }
        }
        let _ = rules;
        return Step::Done(Reason::Idle);
    }
    missionary_step(u, faith, tools)
}

/// The best free belief by Unciv's rating, taken at the start of the turn.
fn rank_free(u: &Unit<'_, '_>, pool: &[BeliefId], exclude: &[BeliefId]) -> Option<BeliefId> {
    let score = |b: &BeliefId| u.sight.belief_ratings.iter().find(|(x, _)| x == b).map_or(0.0, |(_, s)| *s);
    pool.iter().copied().filter(|b| !exclude.contains(b)).max_by(|a, b| score(a).total_cmp(&score(b)).then(b.0.cmp(&a.0)))
}

fn spread_here(u: &Unit<'_, '_>) -> Option<Decision> {
    let command = Command::SpreadReligion { unit: u.me.handle };
    u.local().view.accepts(&command).then(|| Decision::act(actor(&u.me), Reason::SystemAdvisor, command))
}

/// `automateMissionary`: our cities without our religion first (nearest),
/// else the nearest known city of a nation at peace with us.
fn missionary_step(u: &Unit<'_, '_>, faith: &crate::knowledge::observation::FaithObservation, tools: &mut PlanTools<'_>) -> Step {
    let Some(religion) = faith.overview.religion else {
        return Step::Done(Reason::Idle);
    };
    let ours = |v: &&crate::knowledge::observation::CityFaithView| v.majority == Some(Creed::Religion(religion));
    let own: Vec<TileId> = faith.cities.iter().filter(|v| v.owner == u.local().player && !ours(v)).map(|v| v.tile).collect();
    let target = if !own.is_empty() {
        own.into_iter().min_by_key(|t| (u.distance(*t, u.tile()), *t))
    } else {
        faith.cities
            .iter()
            .filter(|v| v.owner != u.local().player && !ours(v))
            .filter(|v| u.local().relation(v.owner).is_some_and(|r| !r.at_war))
            .map(|v| v.tile)
            .min_by_key(|t| (u.distance(*t, u.tile()), *t))
    };
    let Some(city) = target else {
        return Step::Done(Reason::Idle);
    };
    if u.distance(u.tile(), city) <= 1 {
        if let Some(d) = spread_here(u) {
            return Step::Act(d);
        }
    }
    let mut near: Vec<TileId> = crate::math::hex_disc(u.dims(), city, 1).filter(|t| *t == u.tile() || u.can_move_to(*t)).collect();
    near.sort_by_key(|t| (u.distance(*t, u.tile()), *t));
    match near.into_iter().find(|t| *t == u.tile() || u.can_reach(tools, *t)) {
        Some(t) if t != u.tile() => head_towards(u, tools, t, Reason::MoveToJob),
        _ => Step::Done(Reason::Idle),
    }
}

/// `automateInquisitor`: an own city of another religion is won back; else
/// the holy city or our largest city of our religion is guarded.
fn inquisitor_step(u: &Unit<'_, '_>, faith: &crate::knowledge::observation::FaithObservation, tools: &mut PlanTools<'_>) -> Step {
    let Some(religion) = faith.overview.religion else {
        return Step::Done(Reason::Idle);
    };
    let me = u.local().player;
    let convert = faith.cities.iter().filter(|v| v.owner == me && v.majority.is_some_and(|m| m != Creed::Religion(religion) && m != Creed::Pantheon)).min_by_key(|v| (u.distance(v.tile, u.tile()), v.tile));
    let holy = faith.overview.religions.iter().find(|r| r.religion == religion).map(|r| r.holy_city);
    let protect = faith
        .cities
        .iter()
        .filter(|v| v.owner == me && v.majority == Some(Creed::Religion(religion)))
        .max_by_key(|v| (Some(v.handle) == holy, v.population, std::cmp::Reverse(v.tile)));
    let destination = convert.or(protect);
    let Some(city) = destination else {
        return Step::Done(Reason::Idle);
    };
    if convert.is_some_and(|c| c.tile == city.tile) && u.distance(u.tile(), city.tile) <= 1 {
        let command = Command::RemoveHeresy { unit: u.me.handle };
        if u.local().view.accepts(&command) {
            return Step::Act(Decision::act(actor(&u.me), Reason::SystemAdvisor, command));
        }
    }
    let mut near: Vec<TileId> = u.dims().neighbors(city.tile).into_iter().filter(|t| *t == u.tile() || u.can_move_to(*t)).collect();
    near.sort_by_key(|t| (u.distance(*t, u.tile()), *t));
    match near.into_iter().find(|t| *t == u.tile() || u.can_reach(tools, *t)) {
        Some(t) if t != u.tile() => head_towards(u, tools, t, Reason::Garrison),
        _ => Step::Done(Reason::Garrison),
    }
}
