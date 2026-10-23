//! The AI's one-shot weapons (classic/docs/ai-strikes.md): missiles and
//! nuclear weapons. A weapon is an air unit that is used up by its strike
//! (`suicide`); it waits in a city (or on a carrier) and strikes a target
//! within its range. Nothing is named: a *missile* is a one-way air unit
//! with a ranged strength, a *nuclear weapon* one with a `nuke_level`.
//!
//! The value model is the allocator's (`ai::value`, hundredths of a score
//! point). A strike is worth what it takes from the rival and what it
//! gives our armies, a weapon costs the hammers it was built from at the
//! empire's production price:
//!
//! - a **missile** on an enemy unit takes the share of the unit's cost its
//!   damage is, half of it counting for us (`RIVAL_PERCENT`, the war
//!   model's own rival share), twice that when the unit stands near our
//!   cities or an operation; on an enemy city it shortens the siege of an
//!   operation whose army stands near: the damage share of the city's
//!   hit points times the capture's expected gain (`SUCCESS_PERCENT` of
//!   `city_gain`); a city nobody besieges heals the damage again, so it
//!   counts a quarter;
//! - a **nuclear weapon** is judged on its whole blast: the citizens it
//!   kills in every enemy city, half the points they were, the units it
//!   kills or hurts, and for a city an army besieges the better chance
//!   to take it (`NUKE_SUCCESS_PERCENT` against `SUCCESS_PERCENT`), less
//!   the cost of our own units in the blast (their hammers times the
//!   damage). A blast that reaches one of our cities is never fired; one
//!   that reaches a player we are not at war with is refused by the rules
//!   and so is never ordered (`ai::legal`).
//!
//! A weapon already built is fired when its best strike pays
//! `FIRE_PERCENT` of its replacement cost (the cost is spent); one is
//! built when the best prospect in reach pays its whole cost, the project
//! it needs included (`build_gain`). Prospects are the cities our war
//! operations march on, and the enemy units near our cities.

use crate::ai::operations::{city_worth, estimate_city, estimate_target, RIVAL_PERCENT, SUCCESS_PERCENT};
use crate::ai::planner::{role_of, AiRole, Decision, PlanTools, TurnBrief, UnitView};
use crate::ai::state::AiState;
use crate::ai::trace::{Actor, Reason};
use crate::ai::value::{Centi, Prices};
use crate::command::Command;
use crate::ids::{TileId, UnitKindId};
use crate::rules::content::ProjectId;
use crate::knowledge::observation::{AIObservation, OwnUnit};
use crate::knowledge::public::PlayerQueries;
use crate::math::MapDimensions;
use crate::rules::content::Domain;
use crate::rules::CompiledRuleset;
use crate::systems::combat::actions::{
    NUKE_BLAST_RADIUS, NUKE_LEVEL1_POPULATION_DEATH_BASE, NUKE_LEVEL1_POPULATION_DEATH_RAND, NUKE_LEVEL2_POPULATION_DEATH_BASE,
    NUKE_LEVEL2_POPULATION_DEATH_RAND, NUKE_UNIT_DAMAGE_BASE, NUKE_UNIT_DAMAGE_RAND,
};
use crate::world::unit::UNIT_MAX_HP;

/// A weapon is fired when its strike pays this share of its replacement
/// cost, percent (the cost is spent; waiting has a price too: the target
/// moves, the war ends).
pub const FIRE_PERCENT: i64 = 30;
/// Chance an operation takes a city its nuclear weapon has struck, percent
/// (the garrison is dead and the city lost half its health; against
/// `SUCCESS_PERCENT` without).
pub const NUKE_SUCCESS_PERCENT: i64 = 70;
/// Weapons of the player alive or queued at most (they are expensive and
/// the war may end).
pub const MAX_WEAPONS: usize = 2;
/// A city nobody besieges heals the damage of a strike: it counts this
/// share, percent (the air arm's `LONELY_CITY_PERCENT`).
pub const LONELY_PERCENT: i64 = 25;
/// Own land units this close to the target are the army that follows a
/// strike (the air arm's `ARMY_RADIUS`).
pub const ARMY_RADIUS: i32 = 6;
/// Enemy units this close to one of our cities, or to a running
/// operation's target, are engaged with us: killing them also saves what
/// they would take.
pub const ENGAGED_RADIUS: i32 = 4;
/// Enemy units a missile's build counts when they stand next to our cities.
pub const DEFENCE_TARGETS: usize = 3;
/// Candidate blast centres one decision tries.
pub const MAX_CENTRES: usize = 16;

/// What a one-shot weapon is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Weapon {
    /// A one-way air unit with a ranged strength: damages its target.
    Missile,
    /// A nuclear weapon of this level.
    Nuke(u8),
}

/// The weapon a unit kind is (None: not a one-shot weapon this module
/// commands). From data only.
pub fn weapon(rules: &CompiledRuleset, kind: UnitKindId) -> Option<Weapon> {
    let def = rules.units.get(kind.0 as usize)?;
    if def.domain != Domain::Air || !def.combat.suicide || def.range == 0 {
        return None;
    }
    if def.combat.nuke_level > 0 {
        return Some(Weapon::Nuke(def.combat.nuke_level));
    }
    (def.ranged_strength > 0).then_some(Weapon::Missile)
}

/// Centi-points of one hammer of production now (the allocator's rate).
fn hammer(prices: &Prices) -> Centi {
    prices.production / i64::from(prices.rounds_left.max(1))
}

/// What a new weapon of `kind` costs: its hammers at the production price.
pub fn cost(prices: &Prices, rules: &CompiledRuleset, kind: UnitKindId) -> Centi {
    rules.units.get(kind.0 as usize).map_or(0, |d| i64::from(d.cost)) * hammer(prices)
}

/// A place and a thing the enemy has there.
#[derive(Debug, Clone, Copy)]
struct Foe {
    tile: TileId,
    kind: Option<UnitKindId>,
    population: u32,
    city: bool,
}

fn distance(dims: MapDimensions, a: TileId, b: TileId) -> i32 {
    dims.distance(a, b).unwrap_or(i32::MAX)
}

/// Enemy units and cities (of players at war with us) the unit sees.
fn foes(view: &UnitView<'_, '_>, state: &AiState) -> Vec<Foe> {
    let at_war = |owner| view.local.relation(owner).is_some_and(|r| r.at_war);
    let mut out: Vec<Foe> = view
        .local
        .foreign_units
        .iter()
        .filter(|u| at_war(u.owner))
        .map(|u| Foe { tile: u.tile, kind: Some(u.kind), population: 0, city: false })
        .collect();
    out.extend(
        view.local
            .foreign_cities
            .iter()
            .filter(|c| at_war(c.owner))
            .map(|c| Foe { tile: c.tile, kind: None, population: c.population, city: true }),
    );
    // The cities remembered (a nuclear blast needs only a known tile).
    let at_war_with = |owner| view.brief.at_war(owner);
    for city in state.seen_cities().iter().filter(|c| at_war_with(c.owner)) {
        if !out.iter().any(|f| f.tile == city.tile) {
            out.push(Foe { tile: city.tile, kind: None, population: city.population, city: true });
        }
    }
    out.sort_by_key(|f| (f.tile.0, f.city));
    out
}

/// A land army of ours stands near the city (a strike is followed up) or an
/// operation marches on it.
fn besieged(view: &UnitView<'_, '_>, state: &AiState, rules: &CompiledRuleset, city: TileId) -> bool {
    let dims = view.brief.dims;
    let target = state.operations().iter().any(|o| o.target == city);
    let army = view.brief.units.iter().any(|u| {
        u.role == AiRole::Military
            && rules.units.get(u.kind.0 as usize).is_some_and(|d| d.domain == Domain::Land)
            && distance(dims, u.tile, city) <= ARMY_RADIUS
    });
    target && army
}

/// The enemy unit near one of our cities or an operation's target.
fn engaged(brief: &TurnBrief, state: &AiState, tile: TileId) -> bool {
    let near = |other: TileId| distance(brief.dims, tile, other) <= ENGAGED_RADIUS;
    brief.cities.iter().any(|c| near(c.tile)) || state.operations().iter().any(|o| near(o.target))
}

/// What the rival's loss of `share` percent of a unit of `kind` is worth
/// to us.
fn unit_loss(prices: &Prices, rules: &CompiledRuleset, kind: UnitKindId, share: i64) -> Centi {
    let cost = rules.units.get(kind.0 as usize).map_or(0, |d| i64::from(d.cost));
    cost * hammer(prices) * share / 100 * RIVAL_PERCENT / 100
}

/// What a captured city of `population` brings us (`operations::city_worth`
/// over the rounds left, the unrest not counted: the strike's share is small).
fn capture_gain(prices: &Prices, population: u32) -> Centi {
    city_worth(prices, population, prices.rounds_left)
}

/// Points of a city of `population` the rival loses when it is destroyed
/// or shrinks (the points of `city_worth`, half of them counting for us).
fn city_points(prices: &Prices, population: u32) -> Centi {
    let score = prices.score;
    let population = i64::from(population.max(1));
    score.city + score.land * (crate::ai::value::FIRST_RING + population) + score.citizen * population
}

/// The value of a missile strike on `foe` for the damage the rules'
/// preview promises.
fn missile_gain(view: &UnitView<'_, '_>, state: &AiState, rules: &CompiledRuleset, prices: &Prices, unit: &OwnUnit, foe: &Foe) -> Centi {
    let Some(preview) = view.local.view.attack_preview(unit.handle, foe.tile) else {
        return 0;
    };
    let hp = i64::from(preview.defender_hp.max(1));
    let dealt = i64::from(preview.to_defender.1).min(hp);
    if foe.city {
        let city = capture_gain(prices, foe.population);
        let mut gain = city * SUCCESS_PERCENT / 100 * dealt / hp;
        if !besieged(view, state, rules, foe.tile) {
            gain = gain * LONELY_PERCENT / 100;
        }
        return gain;
    }
    let Some(kind) = foe.kind else { return 0 };
    let worth = unit_loss(prices, rules, kind, dealt * 100 / hp);
    if engaged(view.brief, state, foe.tile) {
        2 * worth
    } else {
        worth
    }
}

/// Hit points a nuclear blast of `level` takes from a unit one tile off
/// the centre, on average.
fn blast_damage(level: u8) -> i64 {
    if level == 1 {
        (NUKE_UNIT_DAMAGE_BASE + 2 * (NUKE_UNIT_DAMAGE_RAND - 1) / 2) as i64
    } else {
        i64::from(UNIT_MAX_HP)
    }
}

/// Citizens a nuclear blast of `level` kills in a city, percent, on average.
fn population_loss(level: u8) -> i64 {
    if level >= 2 {
        (NUKE_LEVEL2_POPULATION_DEATH_BASE + NUKE_LEVEL2_POPULATION_DEATH_RAND - 1) as i64
    } else {
        (NUKE_LEVEL1_POPULATION_DEATH_BASE + NUKE_LEVEL1_POPULATION_DEATH_RAND - 1) as i64
    }
}

/// The evaluation of a blast: what it gains and costs, None when it would
/// reach one of our cities.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Blast {
    pub centre: TileId,
    pub gain: Centi,
    pub loss: Centi,
}

impl Blast {
    pub fn net(&self) -> Centi {
        self.gain - self.loss
    }
}

/// The blast of a nuclear weapon of `level` on `centre`, from what the
/// player knows: the enemy's cities and units in sight, its own units and
/// cities. None: it reaches a city of ours.
#[allow(clippy::too_many_arguments)]
fn blast_at(
    brief: &TurnBrief,
    state: &AiState,
    rules: &CompiledRuleset,
    prices: &Prices,
    level: u8,
    centre: TileId,
    enemies: &[Foe],
    army_near: &dyn Fn(TileId) -> bool,
) -> Option<Blast> {
    let dims = brief.dims;
    let radius = NUKE_BLAST_RADIUS;
    let inside = |tile: TileId| distance(dims, centre, tile) <= radius;
    if brief.cities.iter().any(|c| inside(c.tile)) {
        return None;
    }
    let (mut gain, mut loss) = (0, 0);
    for foe in enemies.iter().filter(|f| inside(f.tile)) {
        if foe.city {
            let points = city_points(prices, foe.population);
            gain += points * population_loss(level) / 100 * RIVAL_PERCENT / 100;
            let target = state.operations().iter().any(|o| o.target == foe.tile);
            if target && army_near(foe.tile) {
                gain += capture_gain(prices, foe.population) * (NUKE_SUCCESS_PERCENT - SUCCESS_PERCENT) / 100;
            }
        } else if let Some(kind) = foe.kind {
            let share = if foe.tile == centre { 100 } else { blast_damage(level) * 100 / i64::from(UNIT_MAX_HP) };
            let worth = unit_loss(prices, rules, kind, share);
            gain += if engaged(brief, state, foe.tile) { 2 * worth } else { worth };
        }
    }
    for unit in brief.units.iter().filter(|u| inside(u.tile)) {
        let share = if unit.tile == centre { 100 } else { blast_damage(level) * 100 / i64::from(UNIT_MAX_HP) };
        let cost = rules.units.get(unit.kind.0 as usize).map_or(0, |d| i64::from(d.cost));
        loss += cost * hammer(prices) * share / 100;
    }
    // A nuclear strike is a public act: the civilizations it has met and is not at war with
    // remember it (`ai::infamy`, the rules' warmonger memory).
    loss += crate::ai::infamy::strike_cost(crate::ai::infamy::witnesses(&brief.relations), level);
    Some(Blast { centre, gain, loss })
}

/// The best blast of a nuclear weapon at `from` among the enemy places in
/// its range, the cities first (at most `MAX_CENTRES` tried).
fn best_blast(view: &UnitView<'_, '_>, state: &AiState, rules: &CompiledRuleset, prices: &Prices, unit: &OwnUnit, level: u8, enemies: &[Foe]) -> Option<Blast> {
    let range = rules.units.get(unit.kind.0 as usize).map_or(0, |d| i32::from(d.range));
    let dims = view.brief.dims;
    let army_near = |city: TileId| besieged(view, state, rules, city);
    let mut centres: Vec<&Foe> = enemies.iter().filter(|f| distance(dims, unit.tile, f.tile) <= range).collect();
    centres.sort_by_key(|f| (!f.city, f.tile.0));
    centres.dedup_by_key(|f| f.tile);
    let mut best: Option<Blast> = None;
    for foe in centres.into_iter().take(MAX_CENTRES) {
        let Some(blast) = blast_at(view.brief, state, rules, prices, level, foe.tile, enemies, &army_near) else {
            continue;
        };
        let command = Command::Nuke { unit: unit.handle, target: blast.centre };
        if !view.local.view.accepts(&command) {
            continue;
        }
        if best.is_none_or(|b| (blast.net(), std::cmp::Reverse(blast.centre.0)) > (b.net(), std::cmp::Reverse(b.centre.0))) {
            best = Some(blast);
        }
    }
    best
}

/// The decision of one weapon: fire when the best strike pays, else go
/// where the enemy is in reach (the air arm's rebase), else wait.
pub fn decide_weapon(view: &UnitView<'_, '_>, state: &AiState, tools: &mut PlanTools<'_>) -> Decision {
    let unit = view.local.unit;
    let actor = Actor::Unit(unit.id);
    let rules = tools.rules;
    let Some(kind) = weapon(rules, unit.kind) else {
        return Decision::refuse(actor, Reason::Idle);
    };
    let Some(prices) = view.brief.prices.as_ref() else {
        return Decision::refuse(actor, Reason::Idle);
    };
    if unit.moves_left == 0 {
        return Decision::refuse(actor, Reason::Idle);
    }
    let replacement = cost(prices, rules, unit.kind);
    let enemies = foes(view, state);
    let range = rules.units.get(unit.kind.0 as usize).map_or(0, |d| i32::from(d.range));
    let dims = view.brief.dims;
    let pays = |gain: Centi| gain > 0 && gain * 100 >= replacement * FIRE_PERCENT;
    match kind {
        Weapon::Missile => {
            let mut best: Option<(Centi, &Foe)> = None;
            let mut targets: Vec<&Foe> = enemies.iter().filter(|f| distance(dims, unit.tile, f.tile) <= range).collect();
            targets.sort_by_key(|f| (distance(dims, unit.tile, f.tile), f.tile.0));
            targets.dedup_by_key(|f| f.tile);
            for foe in targets.into_iter().take(MAX_CENTRES) {
                let gain = missile_gain(view, state, rules, prices, &unit, foe);
                if best.is_none_or(|(g, f)| gain > g || (gain == g && foe.tile.0 < f.tile.0)) {
                    best = Some((gain, foe));
                }
            }
            if let Some((_, foe)) = best.filter(|(g, _)| pays(*g)) {
                let command = Command::RangedAttack { unit: unit.handle, target: foe.tile };
                if view.local.view.accepts(&command) {
                    return Decision::act(actor, Reason::FireMissile, command);
                }
            }
        }
        Weapon::Nuke(level) => {
            if let Some(blast) = best_blast(view, state, rules, prices, &unit, level, &enemies).filter(|b| pays(b.net())) {
                return Decision::act(actor, Reason::LaunchNuke, Command::Nuke { unit: unit.handle, target: blast.centre });
            }
        }
    }
    if let Some(decision) = crate::ai::air::rebase_target(view, state, rules, false) {
        return decision;
    }
    if view.brief.carriers {
        if let Some(decision) = crate::ai::carrier::board(view, state, rules) {
            return decision;
        }
    }
    Decision::refuse(actor, Reason::Idle)
}

// ---- what to build ----

/// A prospect the war has for a weapon: an enemy city one of our
/// operations marches on, with an army near or forming.
fn prospects(brief: &TurnBrief, state: &AiState) -> Vec<crate::ai::state::SeenCity> {
    let mut out: Vec<crate::ai::state::SeenCity> = state
        .operations()
        .iter()
        .filter_map(|o| state.seen_cities().iter().find(|c| c.tile == o.target && c.owner == o.enemy))
        .filter(|c| brief.at_war(c.owner) || state.operations().iter().any(|o| o.target == c.tile))
        .copied()
        .collect();
    out.sort_by_key(|c| c.tile.0);
    out.dedup_by_key(|c| c.tile);
    out
}

/// The own cities from which a weapon of `range` can be based on the
/// city (the weapon is built there or rebased one hop).
fn in_reach(brief: &TurnBrief, range: i32, target: TileId) -> bool {
    brief.cities.iter().any(|c| distance(brief.dims, c.tile, target) <= 2 * range)
}

/// What the best prospect is worth to a new weapon of `kind` (0: none in
/// reach), from the war's operations and the estimates of their
/// targets.
pub fn build_gain(obs: &AIObservation<'_>, brief: &TurnBrief, state: &AiState, rules: &CompiledRuleset, prices: &Prices, kind: UnitKindId) -> (Centi, u32) {
    let Some(weapon) = weapon(rules, kind) else { return (0, 0) };
    let def = &rules.units[kind.0 as usize];
    let range = i32::from(def.range);
    let (mut best, mut life) = (0, 0);
    for city in prospects(brief, state) {
        if !in_reach(brief, range, city.tile) {
            continue;
        }
        let gain = match weapon {
            Weapon::Missile => {
                let estimate = estimate_target(brief, rules, &city);
                let strength = i64::from(def.ranged_strength) * 100;
                let dealt = i64::from(crate::combat::damage::ranged_damage(strength, estimate.strength, 0, 0, crate::combat::damage::Roll::Expected));
                capture_gain(prices, city.population) * SUCCESS_PERCENT / 100 * dealt.min(i64::from(estimate.hp)) / i64::from(estimate.hp.max(1))
            }
            Weapon::Nuke(level) => {
                let points = city_points(prices, city.population);
                points * population_loss(level) / 100 * RIVAL_PERCENT / 100 + capture_gain(prices, city.population) * (NUKE_SUCCESS_PERCENT - SUCCESS_PERCENT) / 100
                    - crate::ai::infamy::strike_cost(crate::ai::infamy::witnesses(&brief.relations), level)
            }
        };
        // An operation planned around the weapon (its army is smaller than the army alone
        // would need, `operations::candidate`) saves those units' hammers by it.
        let gain = gain + state.operations().iter().find(|o| o.target == city.tile).map_or(0, |op| army_saved(brief, rules, prices, &city, op));
        if gain > best {
            best = gain;
            life = state.operations().iter().find(|o| o.target == city.tile).map_or(0, |o| operation_life(brief, o));
        }
    }
    // The enemy units next to our cities (a missile kills the besiegers; one
    // salvo of the weapons we may have `MISSILE_PACKAGE` strong): the
    // units' hammers at the production price, half counting for us, twice
    // for being engaged, the best `DEFENCE_TARGETS` of them.
    if matches!(weapon, Weapon::Missile) {
        let mut values: Vec<Centi> = obs
            .visible_foreign_units
            .iter()
            .filter(|u| brief.at_war(u.owner) && matches!(role_of(rules, u.kind), AiRole::Military | AiRole::Naval))
            .filter(|u| brief.cities.iter().any(|c| distance(brief.dims, c.tile, u.tile) <= ENGAGED_RADIUS))
            .filter(|u| in_reach(brief, range, u.tile))
            .map(|u| 2 * unit_loss(prices, rules, u.kind, 100))
            .collect();
        values.sort_by_key(|v| std::cmp::Reverse(*v));
        let defence: Centi = values.iter().take(DEFENCE_TARGETS).sum();
        if defence > best {
            best = defence;
            life = DEFENCE_LIFE;
        }
    }
    (best, life)
}

/// The hammers an operation's weapon saves: the units the army alone would
/// need beyond the operation's own army (`MAX_ARMY` when the city is out of the
/// army's reach), at the price of a melee unit.
fn army_saved(brief: &TurnBrief, rules: &CompiledRuleset, prices: &Prices, city: &crate::ai::state::SeenCity, op: &crate::ai::state::Operation) -> Centi {
    let arsenal = crate::ai::operations::Arsenal::of(brief, rules);
    let Some(melee) = arsenal.melee else { return 0 };
    let threat = 0;
    let alone = crate::ai::operations::composition(rules, &arsenal, &estimate_target(brief, rules, city), threat).map_or(crate::ai::operations::MAX_ARMY, |c| c.total());
    let saved = alone.saturating_sub(op.need.total());
    Centi::from(saved) * i64::from(rules.units[melee.0 as usize].cost) * hammer(prices)
}

/// Rounds a stack at our gates is expected to stay.
pub const DEFENCE_LIFE: u32 = 10;

/// Rounds an operation has left to use a weapon: a forming one forms for
/// `FORM_LIMIT` and marches for `MARCH_LIMIT`, a marching one for
/// `MARCH_LIMIT`, from its phase's round. A weapon that is built later
/// than that arrives after the war and is hammers lost.
pub fn operation_life(brief: &TurnBrief, operation: &crate::ai::state::Operation) -> u32 {
    use crate::ai::operations::{FORM_LIMIT, MARCH_LIMIT};
    let age = brief.round.saturating_sub(operation.phase_round);
    match operation.phase {
        crate::ai::state::OperationPhase::Forming => (FORM_LIMIT + MARCH_LIMIT).saturating_sub(age),
        crate::ai::state::OperationPhase::Marching => MARCH_LIMIT.saturating_sub(age),
    }
}

/// Weapons alive or queued.
pub fn on_hand(obs: &AIObservation<'_>, rules: &CompiledRuleset) -> usize {
    let alive = obs.own_units.iter().filter(|u| weapon(rules, u.kind).is_some()).count();
    let queued = obs
        .own_cities
        .iter()
        .flat_map(|c| &c.queue)
        .filter(|item| matches!(item, crate::ids::ProductionItem::Unit(k) if weapon(rules, *k).is_some()))
        .count();
    alive + queued
}

/// The project a unit kind needs and the player lacks (a weapon that
/// needs a project built first).
pub fn missing_project(obs: &AIObservation<'_>, rules: &CompiledRuleset, kind: UnitKindId) -> Option<ProjectId> {
    let project = rules.units.get(kind.0 as usize)?.required_project?;
    let done = obs.economy.projects.get(project.0 as usize).copied().unwrap_or(0);
    (done == 0).then_some(project)
}

/// A player has a reason to want weapons: at war with a civilization.
pub fn wants_weapons(brief: &TurnBrief) -> bool {
    brief.relations.iter().any(|r| r.at_war && r.kind == crate::world::player::PlayerKind::Major)
}

/// Is the unit kind a unit this module builds for an operation's army?
pub fn is_weapon_kind(rules: &CompiledRuleset, kind: UnitKindId) -> bool {
    weapon(rules, kind).is_some() && role_of(rules, kind) == AiRole::Naval
}


/// What the war calls for: a weapon kind, and the project that must be
/// built first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuildPlan {
    pub kind: UnitKindId,
    pub project: Option<ProjectId>,
    /// Rounds the prospect it is built for lasts: the weapon (its project
    /// included) must be ready within them.
    pub life: u32,
}

impl BuildPlan {
    /// The production order now: the project while it is missing, then
    /// the weapon.
    pub fn item(&self) -> crate::ids::ProductionItem {
        match self.project {
            Some(project) => crate::ids::ProductionItem::Project(project),
            None => crate::ids::ProductionItem::Unit(self.kind),
        }
    }
}

/// The weapon worth building now, if any: the best prospect in reach pays
/// the weapon and its project, fewer than `MAX_WEAPONS` are on hand, the
/// player is at war and `available` (the planner's own gate: technology,
/// resources) lets it train the kind.
pub fn build_plan(obs: &AIObservation<'_>, brief: &TurnBrief, state: &AiState, rules: &CompiledRuleset, available: impl Fn(UnitKindId) -> bool) -> Option<BuildPlan> {
    let prices = brief.prices.as_ref()?;
    if !wants_weapons(brief) || on_hand(obs, rules) >= MAX_WEAPONS {
        return None;
    }
    let mut best: Option<(Centi, UnitKindId, Option<ProjectId>, u32)> = None;
    for index in 0..rules.units.len() {
        let kind = UnitKindId(index as u16);
        if weapon(rules, kind).is_none() || !available(kind) {
            continue;
        }
        let project = missing_project(obs, rules, kind);
        let project_cost = project.map_or(0, |p| i64::from(rules.module.projects[p.0 as usize].cost) * hammer(prices));
        let (gain, life) = build_gain(obs, brief, state, rules, prices, kind);
        let profit = gain - cost(prices, rules, kind) - project_cost;
        if profit > 0 && best.is_none_or(|(p, k, _, _)| profit > p || (profit == p && kind.0 < k.0)) {
            best = Some((profit, kind, project, life));
        }
    }
    best.map(|(_, kind, project, life)| BuildPlan { kind, project, life })
}

/// A weapon an operation may count on: what it does to the case (`struck`,
/// the chance with it), what it still costs and when it is ready.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Armed {
    pub kind: UnitKindId,
    pub weapon: Weapon,
    /// The chance an operation takes its city with it, percent.
    pub success: i64,
    /// Hammers still to build for it, its project included (0: on hand).
    pub extra_hammers: u32,
    /// Rounds until it is ready, at half the empire's production.
    pub ready: u32,
    /// What its use costs beyond hammers: the world's memory of a nuclear
    /// strike (`ai::infamy`).
    pub public_cost: Centi,
}

/// The weapons the player has or could build now (technology, resources, the
/// project they need counted in the hammers), for the war case
/// (`operations::candidate`): a nuclear weapon makes the capture
/// `NUKE_SUCCESS_PERCENT` likely, a package of `MISSILE_PACKAGE` missiles adds
/// `MISSILE_STEP` points each to the chance without them (`base`).
pub fn armory(obs: &AIObservation<'_>, brief: &TurnBrief, rules: &CompiledRuleset, base: i64) -> Vec<Armed> {
    let have = |kind: UnitKindId| brief.units.iter().filter(|u| u.kind == kind).count() as u32;
    let production: u32 = brief.cities.iter().map(|c| c.production).sum();
    let witnesses = crate::ai::infamy::witnesses(&brief.relations);
    let mut out = Vec::new();
    for index in 0..rules.units.len() {
        let kind = UnitKindId(index as u16);
        let Some(weapon) = weapon(rules, kind) else { continue };
        let def = &rules.units[index];
        let known = def.required_tech.is_none_or(|t| brief.knows(t)) && !def.obsolete_tech.is_some_and(|t| brief.knows(t));
        let resources = def.required_resources.iter().all(|(r, n)| obs.economy.resources.available(*r) >= i32::from(*n));
        let allowed = rules.production_gate(crate::ids::ProductionItem::Unit(kind), brief.civ).is_none() && !def.unbuildable && def.cost > 0;
        if !(known && resources && allowed) {
            continue;
        }
        let project = missing_project(obs, rules, kind).map_or(0, |p| rules.module.projects[p.0 as usize].cost);
        let (count, success, public_cost) = match weapon {
            Weapon::Nuke(level) => (1, base.max(NUKE_SUCCESS_PERCENT), crate::ai::infamy::strike_cost(witnesses, level)),
            Weapon::Missile => (MISSILE_PACKAGE, (base + MISSILE_STEP * i64::from(MISSILE_PACKAGE)).min(100), 0),
        };
        let to_build = count.saturating_sub(have(kind));
        let extra_hammers = to_build * def.cost + if to_build > 0 { project } else { 0 };
        let ready = extra_hammers.div_ceil((production / 2).max(1));
        out.push(Armed { kind, weapon, success, extra_hammers, ready, public_cost });
    }
    out
}

/// The weapon on hand that gives a running operation the best chance.
pub fn on_hand_best(armory: &[Armed]) -> Option<&Armed> {
    armory.iter().filter(|a| a.extra_hammers == 0).max_by_key(|a| (a.success, std::cmp::Reverse(a.kind.0)))
}

/// The estimate of a city after the weapon has struck it, before the army
/// comes: a nuclear weapon kills the garrison (the estimate without its
/// garrison term) and takes half the city's hit points
/// (`NUKE_CITY_HIT_POINT_DAMAGE`); a package of missiles takes the damage its
/// strength does, at most `MISSILE_HP_CAP` percent of the hit points.
pub fn struck(brief: &TurnBrief, rules: &CompiledRuleset, city: &crate::ai::state::SeenCity, estimate: &crate::ai::operations::CityEstimate, armed: &Armed) -> crate::ai::operations::CityEstimate {
    match armed.weapon {
        Weapon::Nuke(_) => {
            let mut bare = estimate_city(brief, rules, city, None);
            bare.hp = bare.hp.saturating_sub(crate::systems::combat::actions::NUKE_CITY_HIT_POINT_DAMAGE);
            bare
        }
        Weapon::Missile => {
            let strength = i64::from(rules.units[armed.kind.0 as usize].ranged_strength) * 100;
            let each = i64::from(crate::combat::damage::ranged_damage(strength, estimate.strength, 0, 0, crate::combat::damage::Roll::Expected));
            let total = (each * i64::from(MISSILE_PACKAGE)).min(i64::from(estimate.hp) * MISSILE_HP_CAP / 100);
            crate::ai::operations::CityEstimate { hp: estimate.hp.saturating_sub(total as u16), ..*estimate }
        }
    }
}

/// Missiles that count toward an operation, and what each adds to its chance
/// without them, percentage points; the share of a city's hit points they may
/// take at most (the rest of the siege is the army's).
pub const MISSILE_PACKAGE: u32 = 2;
pub const MISSILE_STEP: i64 = 5;
pub const MISSILE_HP_CAP: i64 = 60;
