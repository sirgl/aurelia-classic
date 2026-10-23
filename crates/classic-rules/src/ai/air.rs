//! The AI's air arm (classic/docs/ai-air.md): aircraft are based in cities and
//! strike from their base within their range, so the whole job is choosing
//! the base, the target, and the posture.
//!
//! Everything is read from the ruleset's data and from the player's
//! observation, no unit is named:
//! - a *striker* is an aircraft with a ranged strength and a range that is
//!   neither a one-way missile (`suicide`) nor a nuclear weapon
//!   (`nuke_level`, which has no ranged strength anyway);
//! - a *fighter* is an aircraft with an interception range
//!   (`air_intercept_range`); it is also a (weaker) striker by its data.
//!
//! The decision of one aircraft, in this order (the first that applies):
//! 1. a base city in danger is left for the nearest safe city in reach;
//! 2. strike: the best target in range by the rules' public preview, less
//!    the answer of the target and the interception the visible
//!    interceptors would make; fighters first sweep the interceptors that
//!    would hit a profitable strike (by the order of one of them);
//! 3. a fighter sweeps a visible enemy interceptor in range when its odds
//!    are good, strikes only to kill, else
//! 4. rebases to the own city from which the most enemy value is in range
//!    (the war operations' targets count), and
//! 5. a fighter that stays takes the interception posture.
//!
//! Aircraft are built for the war operations (`wanted_strikers`) and for the
//! enemy's air force seen (`wanted_fighters`), outside the army cap like
//! the fleet. Nuclear weapons are never ordered: whether to use one is a
//! decision about the whole war (the blast reaches cities of third
//! players, and the rules refuse a blast that would), and no generic rule
//! is known that is not a gamble; the rules module offers them to a human.
//! One-way missiles are not built (nothing returns them to a base).

use crate::ai::planner::{
    kind_strength, role_of, AiRole, Decision, PlanTools, TurnBrief, UnitView,
};
use crate::ai::state::AiState;
use crate::ai::trace::{Actor, Reason};
use crate::command::Command;
use crate::ids::{TileId, UnitHandle, UnitKindId};
use crate::knowledge::observation::{AIObservation, OwnUnit};
use crate::knowledge::public::PlayerQueries;
use crate::rules::content::Domain;
use crate::rules::CompiledRuleset;
use crate::world::unit::{Stance, UNIT_MAX_HP};

/// Strikers a running war operation asks for.
pub const STRIKERS_PER_OPERATION: usize = 2;
/// Strikers wanted at most (the aircraft are expensive).
pub const MAX_STRIKERS: usize = 4;
/// Fighters wanted at most.
pub const MAX_FIGHTERS: usize = 2;
/// An aircraft below this health stays in its base to mend (aircraft heal
/// in a city, about a quarter of their health a turn): a surprise
/// interception, which the forecast cannot see, then finds it whole.
pub const MEND_BELOW_HP: u16 = 70;
/// A strike must leave the aircraft at least this many hit points.
pub const MIN_HP_AFTER: u16 = 20;
/// A fighter sweeps only from this health.
pub const SWEEP_MIN_HP: u16 = 70;
/// A fighter sweeps an interceptor when its strength is at least this
/// percent of the interceptor's.
pub const SWEEP_ODDS_PERCENT: i64 = 90;
/// Previews one decision may compute.
pub const MAX_PREVIEWS: usize = 24;
/// A rebase must raise the enemy value in reach by this percent.
pub const REBASE_GAIN_PERCENT: i64 = 50;
/// Value of a city as a target, in hit points of the front's scale
/// (`front`): a city is worth taking at a high price.
const CITY_VALUE: i64 = 1000;
/// A city struck from the air without an army near it heals the damage
/// again: its value counts this many percent.
const LONELY_CITY_PERCENT: i64 = 25;
/// Own land units this close to an enemy city are the army that follows an
/// air strike.
const ARMY_RADIUS: i32 = 6;
/// An aircraft mends its wounds in its base: the damage it takes counts
/// this many percent of its value (only its loss counts in full).
const DAMAGE_RECOVERS_PERCENT: i64 = 35;
/// Cost of a strike that may lose the aircraft.
const LOSS_RISK_COST: i64 = 100;

/// What an aircraft is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wing {
    /// Strikes ground and sea targets.
    Striker,
    /// Intercepts, sweeps, strikes only to kill.
    Fighter,
}

/// The wing of a unit kind (None: not an aircraft this module commands).
pub fn wing(rules: &CompiledRuleset, kind: UnitKindId) -> Option<Wing> {
    let def = rules.units.get(kind.0 as usize)?;
    if def.domain != Domain::Air || def.combat.suicide || def.combat.nuke_level > 0 {
        return None;
    }
    if def.combat.air_intercept_range > 0 {
        return Some(Wing::Fighter);
    }
    (def.ranged_strength > 0 && def.range > 0).then_some(Wing::Striker)
}

/// The aircraft of a wing the player may build, the strongest (the cheaper
/// on a tie).
pub fn best_of_wing(
    rules: &CompiledRuleset,
    wing_wanted: Wing,
    available: impl Fn(UnitKindId) -> bool,
) -> Option<UnitKindId> {
    let mut best: Option<(u16, u32, UnitKindId)> = None;
    for index in 0..rules.units.len() {
        let kind = UnitKindId(index as u16);
        if wing(rules, kind) != Some(wing_wanted) || !available(kind) {
            continue;
        }
        let def = &rules.units[index];
        if best.is_none_or(|(s, c, _)| {
            def.ranged_strength > s || (def.ranged_strength == s && def.cost < c)
        }) {
            best = Some((def.ranged_strength, def.cost, kind));
        }
    }
    best.map(|(_, _, kind)| kind)
}

/// Aircraft of the wing alive or queued.
pub fn on_hand(obs: &AIObservation<'_>, rules: &CompiledRuleset, wing_counted: Wing) -> usize {
    let alive = obs
        .own_units
        .iter()
        .filter(|u| wing(rules, u.kind) == Some(wing_counted))
        .count();
    let queued = obs
        .own_cities
        .iter()
        .flat_map(|c| &c.queue)
        .filter(|item| matches!(item, crate::ids::ProductionItem::Unit(k) if wing(rules, *k) == Some(wing_counted)))
        .count();
    alive + queued
}

/// Enemy aircraft the player sees now (units of players at war with it).
fn enemy_air_seen(obs: &AIObservation<'_>, rules: &CompiledRuleset) -> usize {
    obs.visible_foreign_units
        .iter()
        .filter(|u| obs.relation(u.owner).is_some_and(|r| r.at_war))
        .filter(|u| {
            rules
                .units
                .get(u.kind.0 as usize)
                .is_some_and(|d| d.domain == Domain::Air)
        })
        .count()
}

/// The player is at war with a civilization whose city it has seen: a war
/// worth an air wing even before an operation forms (wars are also fought
/// by the units of the front, `ai::front`).
fn war_footing(obs: &AIObservation<'_>, state: &AiState) -> bool {
    let major_at_war = |owner| {
        obs.relation(owner)
            .is_some_and(|r| r.at_war && r.kind == crate::world::player::PlayerKind::Major)
    };
    state.seen_cities().iter().any(|c| major_at_war(c.owner))
}

/// Strikers wanted: two for each running war operation, two for a war
/// without one, never more than a city in two can base.
pub fn wanted_strikers(obs: &AIObservation<'_>, state: &AiState) -> usize {
    let asked =
        (state.operations().len() * STRIKERS_PER_OPERATION).max(if war_footing(obs, state) {
            STRIKERS_PER_OPERATION
        } else {
            0
        });
    asked
        .min(MAX_STRIKERS)
        .min(obs.own_cities.len().div_ceil(2))
}

/// Fighters wanted: one to cover the strikers, one more against an enemy
/// air force in sight.
pub fn wanted_fighters(obs: &AIObservation<'_>, state: &AiState, rules: &CompiledRuleset) -> usize {
    let cover = usize::from(wanted_strikers(obs, state) > 0);
    let defence = usize::from(enemy_air_seen(obs, rules) > 0);
    (cover + defence).min(MAX_FIGHTERS)
}

/// Aircraft of a wing still to build.
pub fn shortfall(
    obs: &AIObservation<'_>,
    state: &AiState,
    rules: &CompiledRuleset,
    wing_counted: Wing,
) -> usize {
    let wanted = match wing_counted {
        Wing::Striker => wanted_strikers(obs, state),
        Wing::Fighter => wanted_fighters(obs, state, rules),
    };
    wanted.saturating_sub(on_hand(obs, rules, wing_counted))
}

/// How far around an aircraft the local observation looks: its range (what
/// it can strike from its base; the places a rebase is judged by come from
/// the brief and the memory).
pub fn radius(rules: &CompiledRuleset, kind: UnitKindId, default: i32) -> i32 {
    if wing(rules, kind).is_none() && crate::ai::strikes::weapon(rules, kind).is_none() {
        return default;
    }
    let range = rules.units.get(kind.0 as usize).map_or(0, |d| i32::from(d.range));
    range.max(default)
}

/// An enemy in sight: a unit or a city of a player at war with us, or the
/// target of one of our operations.
#[derive(Debug, Clone, Copy)]
struct Foe {
    tile: TileId,
    kind: Option<UnitKindId>,
    population: u32,
    city: bool,
}

fn foes(view: &UnitView<'_, '_>) -> Vec<Foe> {
    let at_war = |owner| view.local.relation(owner).is_some_and(|r| r.at_war);
    let mut out: Vec<Foe> = view
        .local
        .foreign_units
        .iter()
        .filter(|u| at_war(u.owner))
        .map(|u| Foe {
            tile: u.tile,
            kind: Some(u.kind),
            population: 0,
            city: false,
        })
        .collect();
    out.extend(
        view.local
            .foreign_cities
            .iter()
            .filter(|c| at_war(c.owner))
            .map(|c| Foe {
                tile: c.tile,
                kind: None,
                population: c.population,
                city: true,
            }),
    );
    out.sort_by_key(|f| (f.tile.0, f.city));
    out
}

/// Value of destroying a foe, the scale of the front's (`ai::front`).
fn value(rules: &CompiledRuleset, foe: &Foe) -> i64 {
    match foe.kind {
        None => CITY_VALUE + 20 * i64::from(foe.population),
        Some(kind) if role_of(rules, kind) == AiRole::Military || is_air(rules, kind) => {
            60 + 4 * i64::from(kind_strength_any(rules, kind))
        }
        Some(_) => 40,
    }
}

fn is_air(rules: &CompiledRuleset, kind: UnitKindId) -> bool {
    rules
        .units
        .get(kind.0 as usize)
        .is_some_and(|d| d.domain == Domain::Air)
}

/// The strength a unit kind fights with in the air or on land.
fn kind_strength_any(rules: &CompiledRuleset, kind: UnitKindId) -> u32 {
    match rules.units.get(kind.0 as usize) {
        Some(def) if def.strength == 0 => u32::from(def.ranged_strength),
        _ => kind_strength(rules, kind),
    }
}

fn distance(view: &UnitView<'_, '_>, a: TileId, b: TileId) -> i32 {
    view.local
        .view
        .dimensions()
        .distance(a, b)
        .unwrap_or(i32::MAX)
}

/// An interceptor kind: air or ground units that hit air strikes.
fn intercept_range(rules: &CompiledRuleset, kind: UnitKindId) -> i32 {
    rules
        .units
        .get(kind.0 as usize)
        .map_or(0, |d| i32::from(d.combat.air_intercept_range))
}

/// Interceptors of the enemy whose reach covers `tile`.
fn interceptors_over(
    view: &UnitView<'_, '_>,
    rules: &CompiledRuleset,
    foes: &[Foe],
    tile: TileId,
) -> Vec<Foe> {
    foes.iter()
        .filter(|f| {
            f.kind.is_some_and(|k| {
                intercept_range(rules, k) > 0
                    && distance(view, f.tile, tile) <= intercept_range(rules, k)
            })
        })
        .copied()
        .collect()
}

/// One strike considered: the target tile and what it is expected to give.
#[derive(Debug, Clone, Copy)]
struct Strike {
    target: TileId,
    /// Value gained less the value risked.
    net: i64,
    /// The strike pays with the interception expected.
    pays: bool,
    /// Without any interception the strike would pay.
    pays_unintercepted: bool,
    /// An interceptor in sight would try to hit it.
    intercepted: bool,
}

/// Is a land army of ours near the city, so that the damage is followed up?
fn army_near(view: &UnitView<'_, '_>, rules: &CompiledRuleset, city: TileId) -> bool {
    view.brief.units.iter().any(|u| {
        u.role == AiRole::Military
            && rules
                .units
                .get(u.kind.0 as usize)
                .is_some_and(|d| d.domain == Domain::Land)
            && distance(view, u.tile, city) <= ARMY_RADIUS
    })
}

fn is_target_of_operation(state: &AiState, tile: TileId) -> bool {
    state.operations().iter().any(|o| o.target == tile)
}

/// The strikes the aircraft could make from its base now, best first.
fn strikes(
    view: &UnitView<'_, '_>,
    state: &AiState,
    rules: &CompiledRuleset,
    foes: &[Foe],
    wing_of: Wing,
) -> Vec<Strike> {
    let unit = view.local.unit;
    let Some(def) = rules.units.get(unit.kind.0 as usize) else {
        return Vec::new();
    };
    let range = i32::from(def.range);
    let own_value = 60 + 4 * i64::from(def.ranged_strength);
    let mut out = Vec::new();
    // Aircraft are no defenders of their tile: only cities and ground or sea
    // units are struck.
    let mut targets: Vec<&Foe> = foes
        .iter()
        .filter(|f| {
            distance(view, unit.tile, f.tile) <= range && !f.kind.is_some_and(|k| is_air(rules, k))
        })
        .collect();
    targets.sort_by_key(|f| (distance(view, unit.tile, f.tile), f.tile.0));
    targets.dedup_by_key(|f| f.tile);
    for foe in targets.into_iter().take(MAX_PREVIEWS) {
        let Some(preview) = view.local.view.attack_preview(unit.handle, foe.tile) else {
            continue;
        };
        let hp = i64::from(preview.defender_hp.max(1));
        let dealt = i64::from(preview.to_defender.1).min(hp);
        let mut gain = value(rules, foe) * dealt / hp;
        if foe.city && !is_target_of_operation(state, foe.tile) && !army_near(view, rules, foe.tile)
        {
            gain = gain * LONELY_CITY_PERCENT / 100;
        }
        // The forecast of the rules (`air_strike_risk`): the answer of the
        // target and the interception by the best interceptor in sight; the
        // preview of a ranged attack shows no damage to the attacker.
        let risk = view
            .local
            .view
            .air_strike_risk(unit.handle, foe.tile)
            .unwrap_or_default();
        let (answer, chance) = (i64::from(risk.answer), i64::from(risk.chance));
        let interception = i64::from(risk.interception);
        let hp_now = i64::from(unit.hp);
        let loss = |answer: i64, interception: i64, chance: i64| {
            let damage = answer + interception * chance / 100;
            let mut loss =
                own_value * damage * DAMAGE_RECOVERS_PERCENT / (100 * i64::from(UNIT_MAX_HP));
            if answer >= hp_now {
                loss += own_value + LOSS_RISK_COST;
            } else if answer + interception >= hp_now {
                loss += (own_value + LOSS_RISK_COST) * chance / 100;
            }
            loss
        };
        let survives = |answer: i64, interception: i64, chance: i64| {
            answer + if chance > 0 { interception } else { 0 } < hp_now
                && hp_now - answer - interception * chance / 100 >= i64::from(MIN_HP_AFTER)
        };
        let pays_unintercepted = survives(answer, 0, 0) && gain > loss(answer, 0, 0);
        let net = gain - loss(answer, interception, chance);
        let decisive = preview.verdict == crate::combat::exchange::Verdict::DecisiveVictory;
        let allowed = match wing_of {
            Wing::Striker => true,
            Wing::Fighter => decisive,
        };
        let pays = survives(answer, interception, chance) && net > 0;
        if allowed && (pays || pays_unintercepted) {
            out.push(Strike {
                target: foe.tile,
                net,
                pays,
                pays_unintercepted,
                intercepted: chance > 0,
            });
        }
    }
    out.sort_by_key(|s| (std::cmp::Reverse(s.net), s.target.0));
    out
}

/// The fighters of the player (not `except`) in the brief.
fn own_fighters<'v>(
    view: &'v UnitView<'_, '_>,
    rules: &'v CompiledRuleset,
    except: UnitHandle,
) -> impl Iterator<Item = (UnitHandle, crate::ids::EntityId, u16)> + 'v {
    view.brief
        .units
        .iter()
        .filter(move |u| u.handle != except && wing(rules, u.kind) == Some(Wing::Fighter))
        .map(|u| (u.handle, u.id, u.hp))
}

/// A sweep by one of our fighters of an interceptor over the strike's
/// target, before the striker goes in. None: no fighter can.
fn sweep_for(view: &UnitView<'_, '_>, rules: &CompiledRuleset, guards: &[Foe]) -> Option<Decision> {
    // Air interceptors first (a dogfight), then the ground ones (the sweep
    // spends their interception).
    let mut order: Vec<&Foe> = guards.iter().collect();
    order.sort_by_key(|g| (!g.kind.is_some_and(|k| is_air(rules, k)), g.tile.0));
    for (fighter, id, hp) in own_fighters(view, rules, view.local.unit.handle) {
        if hp < SWEEP_MIN_HP {
            continue;
        }
        for guard in &order {
            let command = Command::AirSweep {
                unit: fighter,
                target: guard.tile,
            };
            if view.local.view.accepts(&command) {
                return Some(Decision::act(Actor::Unit(id), Reason::AirSweep, command));
            }
        }
    }
    None
}

/// The strongest enemy air interceptor a fighter may sweep with good odds
/// from its base.
fn sweep_target(view: &UnitView<'_, '_>, rules: &CompiledRuleset, foes: &[Foe]) -> Option<Foe> {
    let unit = view.local.unit;
    let def = rules.units.get(unit.kind.0 as usize)?;
    if unit.hp < SWEEP_MIN_HP {
        return None;
    }
    let mine = i64::from(def.ranged_strength);
    foes.iter()
        .filter(|f| !f.city)
        .filter(|f| {
            f.kind
                .is_some_and(|k| is_air(rules, k) && intercept_range(rules, k) > 0)
        })
        .filter(|f| distance(view, unit.tile, f.tile) <= i32::from(def.range))
        .filter(|f| {
            let theirs = f.kind.map_or(0, |k| i64::from(kind_strength_any(rules, k)));
            mine * 100 >= theirs * SWEEP_ODDS_PERCENT
        })
        .min_by_key(|f| (distance(view, unit.tile, f.tile), f.tile.0))
        .copied()
}

/// A place the enemy is known to be at, with what destroying it is worth:
/// the units seen at the start of the turn, the cities remembered
/// (`AiState::seen_cities`) and the targets of the war operations, of the
/// players at war with us. Wider than the local observation, which sees the
/// range of the aircraft only; this is what a rebase is judged by.
fn places(view: &UnitView<'_, '_>, state: &AiState) -> Vec<(TileId, i64)> {
    let brief = view.brief;
    let mut out: Vec<(TileId, i64)> = brief
        .sightings
        .iter()
        .filter(|s| brief.at_war(s.owner))
        .map(|s| {
            (
                s.tile,
                if s.military {
                    60 + 4 * i64::from(s.power)
                } else {
                    40
                },
            )
        })
        .collect();
    out.extend(
        state
            .seen_cities()
            .iter()
            .filter(|c| brief.at_war(c.owner))
            .map(|c| (c.tile, CITY_VALUE + 20 * i64::from(c.population))),
    );
    out.extend(state.operations().iter().map(|o| (o.target, CITY_VALUE)));
    out.sort_by_key(|p| p.0 .0);
    // One value per tile (a city and the units in it): the city's.
    out.dedup_by(|later, kept| {
        let same = later.0 == kept.0;
        if same {
            kept.1 = kept.1.max(later.1);
        }
        same
    });
    out
}

/// Enemy value in reach of `base` for an aircraft of `range`.
fn reach_value(view: &UnitView<'_, '_>, places: &[(TileId, i64)], base: TileId, range: i32) -> i64 {
    places
        .iter()
        .filter(|(tile, _)| distance(view, base, *tile) <= range)
        .map(|(_, v)| v)
        .sum()
}

/// The own city to rebase to: the most enemy value in reach, a safe city
/// within the rebase range, accepted by the rules (room, range), or, with
/// `retreat`, the nearest safe city.
pub(crate) fn rebase_target(
    view: &UnitView<'_, '_>,
    state: &AiState,
    rules: &CompiledRuleset,
    retreat: bool,
) -> Option<Decision> {
    let unit = view.local.unit;
    let def = rules.units.get(unit.kind.0 as usize)?;
    let range = i32::from(def.range);
    let places = places(view, state);
    let here = reach_value(view, &places, unit.tile, range);
    let mut best: Option<(i64, i32, &crate::ai::planner::CityBrief)> = None;
    for city in &view.brief.cities {
        if city.tile == unit.tile
            || !sheltering(view.brief, city.tile)
            || distance(view, unit.tile, city.tile) > range * 2
        {
            continue;
        }
        let score = if retreat {
            0
        } else {
            reach_value(view, &places, city.tile, range)
        };
        let better = best.is_none_or(|(s, d, c)| {
            (
                score,
                std::cmp::Reverse(distance(view, unit.tile, city.tile)),
                std::cmp::Reverse(city.id),
            ) > (s, std::cmp::Reverse(d), std::cmp::Reverse(c.id))
        });
        if better {
            best = Some((score, distance(view, unit.tile, city.tile), city));
        }
    }
    let (score, _, city) = best?;
    if !retreat && (score == 0 || score * 100 < here * (100 + REBASE_GAIN_PERCENT)) {
        return None;
    }
    let command = Command::Rebase {
        unit: unit.handle,
        city: city.handle,
    };
    if !view.local.view.accepts(&command) {
        return None;
    }
    let reason = if retreat {
        Reason::Retreat
    } else {
        Reason::Rebase
    };
    Some(Decision::act(Actor::Unit(unit.id), reason, command))
}

/// The own city on `tile` is threatened by more than twice what its guard,
/// planned guards included, can hold (`ai::defence`): its aircraft would be
/// lost with it. Twice, and `sheltering` below at most half, so that an
/// aircraft does not flee a city and return to it turn by turn (the
/// measured ping-pong of a fighter between a front city and a rear one).
fn falling(brief: &TurnBrief, tile: TileId) -> bool {
    brief
        .defence
        .as_ref()
        .and_then(|watch| watch.danger(tile))
        .is_some_and(|d| d.threat > 2 * d.planned)
}

/// The city may take an aircraft in: it is in no danger, or its guard is
/// planned to be twice the threat.
fn sheltering(brief: &TurnBrief, tile: TileId) -> bool {
    brief
        .defence
        .as_ref()
        .and_then(|watch| watch.danger(tile))
        .is_none_or(|d| 2 * d.threat <= d.planned)
}

/// The decision of one aircraft (module doc).
pub fn decide_air(view: &UnitView<'_, '_>, state: &AiState, tools: &mut PlanTools<'_>) -> Decision {
    let unit: OwnUnit = view.local.unit;
    let actor = Actor::Unit(unit.id);
    let rules = tools.rules;
    let Some(wing_of) = wing(rules, unit.kind) else {
        return Decision::refuse(actor, Reason::Idle);
    };
    let foes = foes(view);
    let based = view.brief.cities.iter().any(|c| c.tile == unit.tile);
    let can_act = unit.moves_left > 0;

    // 1. A base city about to fall takes its aircraft with it.
    if based && can_act && falling(view.brief, unit.tile) {
        if let Some(decision) = rebase_target(view, state, rules, true) {
            return decision;
        }
    }
    // A wounded aircraft mends in its base.
    if unit.hp < MEND_BELOW_HP {
        return Decision::refuse(actor, Reason::Heal);
    }
    if !can_act {
        return post(view, wing_of, actor);
    }
    // 2. Strike, after the sweeps that clear the way.
    if unit.can_attack {
        for best in strikes(view, state, rules, &foes, wing_of) {
            if best.intercepted && best.pays_unintercepted {
                let guards = interceptors_over(view, rules, &foes, best.target);
                if let Some(sweep) = sweep_for(view, rules, &guards) {
                    return sweep;
                }
            }
            if !best.pays {
                continue;
            }
            let reason = if foes.iter().any(|f| f.tile == best.target && f.city) {
                Reason::AttackCity
            } else {
                Reason::AttackUnit
            };
            return Decision::act(
                actor,
                reason,
                Command::RangedAttack {
                    unit: unit.handle,
                    target: best.target,
                },
            );
        }
        // 3. A fighter clears the sky of an interceptor it outfights.
        if wing_of == Wing::Fighter {
            if let Some(guard) = sweep_target(view, rules, &foes) {
                let command = Command::AirSweep {
                    unit: unit.handle,
                    target: guard.tile,
                };
                if view.local.view.accepts(&command) {
                    return Decision::act(actor, Reason::AirSweep, command);
                }
            }
        }
    }
    // 4. To the base from which the enemy is in reach.
    if let Some(decision) = rebase_target(view, state, rules, false) {
        return decision;
    }
    if view.brief.carriers && can_act {
        if let Some(decision) = crate::ai::carrier::board(view, state, rules) {
            return decision;
        }
    }
    post(view, wing_of, actor)
}

/// The posture of an aircraft with nothing to do: a fighter waits to
/// intercept (once; the stance stays until it moves or strikes).
fn post(view: &UnitView<'_, '_>, wing_of: Wing, actor: Actor) -> Decision {
    let unit = view.local.unit;
    if wing_of == Wing::Fighter && unit.stance != Stance::Intercept {
        let command = Command::Intercept { unit: unit.handle };
        if view.local.view.accepts(&command) {
            return Decision::act(actor, Reason::Intercept, command);
        }
    }
    Decision::refuse(actor, Reason::Idle)
}
