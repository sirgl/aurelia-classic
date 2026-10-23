//! War operations of the AI since the `operations` variant (classic/docs/ai-operations.md):
//! an army formed for one enemy city — the composition the city needs, a
//! muster point, the moment to march — owned by one function at the start
//! of the player's turn (`plan`), executed by its members' task
//! (`member_step`) and, at the target, by the front (`ai::front`). Cities
//! build what a forming operation lacks (`demand`, read by
//! `planner_cities`).
//!
//! Why: the `tactics` front wins its fights, but reinforcements reached the
//! city one by one, no army was built for a war, and wars were declared on
//! a power sum, not on whether an army could be formed and arrive before
//! war weariness (classic/docs/ai-tactics.md).
//!
//! Everything reads the brief, the memory and the rules' public combat
//! formulas; nothing hidden.

use crate::ai::combat::ranged_range;
use crate::ai::front::{siege_rounds, Besieger};
use crate::ai::planner::{role_of, unit_power, AiRole, Decision, DecisionBudget, PlanTools, TurnBrief, UnitBrief, UnitView};
use crate::ai::planner_map::{explore, turns_for, Blockers, KnownMap, MapScratch, SearchStats, Visit};
use crate::ai::planner_strategy::{
    rounds_since, EARLIEST_WAR_ROUND, MIN_WAR_ROUNDS, OBJECTIVE_RANGE, PEACE_COOLDOWN_ROUNDS, PREPARE_ROUNDS,
    WAR_WEARINESS_ROUNDS,
};
use crate::ai::planner_units::{approach, garrison_wanted, hold, move_along, HEAL_BELOW_HP};
use crate::ai::state::{
    AiState, ArmyRole, Composition, ObjectiveKind, Operation, OperationPhase, SeenCity, Strategy, UnitTask, WarObjective,
    WarPlan,
};
use crate::ai::grand_strategy::Stakes;
use crate::ai::value::{Centi, Prices};
use crate::ai::trace::{Actor, Reason};
use crate::combat::city::{
    CITY_HEAL_PER_TURN, CITY_STRENGTH_BASE, CITY_STRENGTH_PER_POP, CITY_STRENGTH_FROM_GARRISON_DIVISOR,
    CITY_TECH_STRENGTH,
};
use crate::combat::damage::{melee_damage, ranged_damage, MeleeSide, Roll};
use crate::derived::income::unit_upkeep;
use crate::command::Command;
use crate::ids::{EntityId, PlayerId, ProductionItem, TileId, UnitKindId};
use crate::knowledge::observation::AIObservation;
use crate::knowledge::public::PlayerQueries;
use crate::knowledge::view::PlayerView;
use crate::rules::content::UnitRole;
use crate::rules::effects::{CombatContext, Modifier};
use crate::rules::CompiledRuleset;
use crate::world::city::CITY_BASE_HIT_POINTS;

/// Most units an operation plans for: a bigger need means the target is
/// too strong.
pub const MAX_ARMY: u32 = 12;
/// Rounds of strikes in which the planned army takes the city — one less
/// than the front's siege test (`front::SIEGE_ROUNDS`): room for the
/// city's strikes and bad rolls.
pub const PLAN_SIEGE_ROUNDS: u32 = 3;
/// A melee unit that loses this much per strike on the city survives at
/// most two: the army takes ranged units first.
pub const HEAVY_STRIKE_HP: u16 = 34;
/// Most escorts added against the enemy's field army.
pub const MAX_ESCORTS: u8 = 3;
/// Members gather within this distance of the muster point.
pub const MUSTER_RADIUS: i32 = 2;
/// After this many rounds of forming, three quarters of the army marches.
pub const FORM_PATIENCE: u32 = 20;
/// A forming operation that has not marched after this many rounds ends.
pub const FORM_LIMIT: u32 = 30;
/// A marching operation that has not taken its target after this many
/// rounds ends.
pub const MARCH_LIMIT: u32 = 20;
/// Members beyond the need an operation takes while it forms.
pub const EXTRA_MEMBERS: u32 = 2;
/// Free units farther than this from the muster point are not recruited.
pub const RECRUIT_RANGE: i32 = 15;
/// A member this much closer to the target than the members' median waits.
pub const COHESION: i32 = 2;
/// Rounds to bring the recruited units to the muster point.
pub const GATHER_ROUNDS: u32 = 5;
/// Rounds of the siege itself, from the arrival.
pub const SIEGE_ROUNDS_ESTIMATE: u32 = 8;
/// Most units the cities build for one operation: the rest of its army
/// must exist. Armies built for a war cost more growth than their captures
/// return on the bench (classic/docs/ai-operations.md "Measurements": with no limit the
/// variant wins 50.8 %, with 2 — 57.5 %, never declaring — 56.7 %).
pub const MAX_WAR_ORDERS: u32 = 2;
/// A unit an operation orders is built in at most this many turns.
pub const WAR_BUILD_TURNS: u32 = 10;
/// At peace, a player looks for a new operation every this many rounds.
pub const START_INTERVAL: u32 = 5;
/// Routes checked for new operations in one turn (best estimates first).
pub const MAX_ROUTE_CHECKS: usize = 3;
/// Rounds of the deficit of an operation's army the treasury must pay.
pub const WAR_CHEST_ROUNDS: u32 = 40;
/// Enemy units this close to the target defend it.
pub const DEFENCE_RADIUS: i32 = 3;
/// A route reaching this close to the target over open land is a way to
/// it (the rest is the enemy's land, entered at war).
pub const BORDER_REACH: i32 = 5;
/// The army stages this far from its target: out of the city's strike.
pub const STAGING_DISTANCE: i32 = crate::combat::city::CITY_ATTACK_RANGE + 1;

/// What a unit kind is to an operation's army: None for non-combat,
/// recon, sea and air units.
pub fn army_role(rules: &CompiledRuleset, kind: UnitKindId) -> Option<ArmyRole> {
    let def = rules.units.get(kind.0 as usize)?;
    if role_of(rules, kind) != AiRole::Military || def.role == Some(UnitRole::Recon) {
        return None;
    }
    if def.role == Some(UnitRole::Siege) {
        return Some(ArmyRole::Siege);
    }
    if ranged_range(rules, kind).is_some() {
        return Some(ArmyRole::Ranged);
    }
    (def.strength > 0).then_some(ArmyRole::Melee)
}

/// The strength a role strikes with: melee strength, else ranged.
fn role_strength(rules: &CompiledRuleset, kind: UnitKindId, role: ArmyRole) -> u16 {
    let def = &rules.units[kind.0 as usize];
    match role {
        ArmyRole::Melee => def.strength,
        ArmyRole::Ranged | ArmyRole::Siege => def.ranged_strength,
    }
}

/// What `have` lacks of `need`: melee by melee; ranged and siege places
/// filled by either, a siege place first by siege.
pub fn shortfall(need: Composition, have: Composition) -> Composition {
    let distance_need = need.ranged + need.siege;
    let distance_have = have.ranged.saturating_add(have.siege);
    let distance_missing = distance_need.saturating_sub(distance_have);
    let siege = need.siege.saturating_sub(have.siege).min(distance_missing);
    Composition {
        melee: need.melee.saturating_sub(have.melee),
        ranged: distance_missing - siege,
        siege,
    }
}

/// The composition of some units.
fn composition_of(rules: &CompiledRuleset, kinds: impl Iterator<Item = UnitKindId>) -> Composition {
    let mut out = Composition::default();
    for role in kinds.filter_map(|k| army_role(rules, k)) {
        out.add(role);
    }
    out
}

// ---- The target's defence and the army it needs ----

/// The planning estimate of an enemy city from what the player knows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CityEstimate {
    /// Combat strength in hundredths.
    pub strength: i64,
    pub hp: u16,
    pub heal: u16,
}

/// The best kind of each role the player could train now by its
/// technologies (resources aside: the estimate, not the order).
#[derive(Debug, Clone, Copy, Default)]
pub struct Arsenal {
    pub melee: Option<UnitKindId>,
    pub ranged: Option<UnitKindId>,
    pub siege: Option<UnitKindId>,
}

impl Arsenal {
    pub fn of(brief: &TurnBrief, rules: &CompiledRuleset) -> Self {
        Self {
            melee: best_kind(brief, rules, ArmyRole::Melee),
            ranged: best_kind(brief, rules, ArmyRole::Ranged),
            siege: best_kind(brief, rules, ArmyRole::Siege),
        }
    }

    fn kind(&self, role: ArmyRole) -> Option<UnitKindId> {
        match role {
            ArmyRole::Melee => self.melee,
            ArmyRole::Ranged => self.ranged,
            ArmyRole::Siege => self.siege,
        }
    }
}

fn best_kind(brief: &TurnBrief, rules: &CompiledRuleset, role: ArmyRole) -> Option<UnitKindId> {
    let mut best: Option<(u16, u32, UnitKindId)> = None;
    for index in 0..rules.units.len() {
        let kind = UnitKindId(index as u16);
        if army_role(rules, kind) != Some(role) || !trainable(brief, rules, kind) {
            continue;
        }
        let strength = role_strength(rules, kind, role);
        let cost = rules.units[index].cost;
        if best.is_none_or(|(s, c, _)| strength > s || (strength == s && cost < c)) {
            best = Some((strength, cost, kind));
        }
    }
    best.map(|(_, _, kind)| kind)
}

/// Known technology, not obsolete, the civilization's own version, no
/// required building.
fn trainable(brief: &TurnBrief, rules: &CompiledRuleset, kind: UnitKindId) -> bool {
    let def = &rules.units[kind.0 as usize];
    let gate = rules.production_gate(ProductionItem::Unit(kind), brief.civ).is_none();
    let replaced = rules.units.iter().any(|u| u.civilization == Some(brief.civ) && u.replaces == Some(kind));
    gate && !def.unbuildable
        && def.cost > 0
        && def.required_tech.is_none_or(|t| brief.knows(t))
        && !def.obsolete_tech.is_some_and(|t| brief.knows(t))
        && def.requires_buildings.is_empty()
        && !replaced
}

/// A city as the player can estimate it: base strength, its population,
/// the technology part at our own share of technologies (eras are close),
/// a garrison like our best melee unit; 200 HP (walls are not seen).
pub fn estimate_city(brief: &TurnBrief, rules: &CompiledRuleset, city: &SeenCity, garrison: Option<UnitKindId>) -> CityEstimate {
    let techs = rules.technologies.len().max(1);
    let share = brief.techs.len().min(techs) * 100 / techs;
    let garrison = garrison.map_or(0, |k| i64::from(rules.units[k.0 as usize].strength) * 100 * 100 / CITY_STRENGTH_FROM_GARRISON_DIVISOR);
    CityEstimate {
        strength: CITY_STRENGTH_BASE + i64::from(city.population) * CITY_STRENGTH_PER_POP + CITY_TECH_STRENGTH[share] + garrison,
        hp: CITY_BASE_HIT_POINTS,
        heal: CITY_HEAL_PER_TURN as u16,
    }
}

/// The estimate of a remembered enemy city with a garrison like our best
/// melee unit (the planning estimate of `plan`).
pub(crate) fn estimate_target(brief: &TurnBrief, rules: &CompiledRuleset, city: &SeenCity) -> CityEstimate {
    estimate_city(brief, rules, city, best_kind(brief, rules, ArmyRole::Melee))
}

/// The expected exchange of a unit of `kind` with `hp` against an estimated
/// city; None for a unit that is not an army unit.
pub(crate) fn estimated_strike(rules: &CompiledRuleset, kind: UnitKindId, hp: u16, city: &CityEstimate) -> Option<Besieger> {
    let role = army_role(rules, kind)?;
    Some(Besieger { hp, ..strike(rules, kind, role, city) })
}

/// The bonus against cities of a kind's free promotions, percent (siege
/// units: +200 %).
fn vs_city_percent(rules: &CompiledRuleset, kind: UnitKindId) -> i64 {
    let promotions = rules.units[kind.0 as usize].free_promotions.iter().filter_map(|p| rules.promotions.get(p.0 as usize));
    promotions
        .flat_map(|p| &p.effects.passive_unit)
        .filter_map(|e| match e.modifier {
            Modifier::ModifyCombat { context: CombatContext::VsCity, percent } => Some(i64::from(percent)),
            _ => None,
        })
        .sum()
}

/// Expected damage one new unit of `kind` deals to the city and takes from
/// it: its strength with its free promotions' bonus against cities.
fn strike(rules: &CompiledRuleset, kind: UnitKindId, role: ArmyRole, city: &CityEstimate) -> Besieger {
    let strength = i64::from(role_strength(rules, kind, role)) * (100 + vs_city_percent(rules, kind));
    let (dealt, taken) = match role {
        ArmyRole::Melee => (
            melee_damage(strength, city.strength, 0, 0, Roll::Expected, MeleeSide::UnitVsCity),
            melee_damage(city.strength, strength, 0, 0, Roll::Expected, MeleeSide::CityVsUnit),
        ),
        ArmyRole::Ranged | ArmyRole::Siege => (ranged_damage(strength, city.strength, 0, 0, Roll::Expected), 0),
    };
    Besieger { ranged: role != ArmyRole::Melee, dealt, taken, hp: crate::world::unit::UNIT_MAX_HP }
}

/// The army that takes `city` within `PLAN_SIEGE_ROUNDS` by the front's
/// siege model, plus escorts against `threat` (the enemy's field power
/// seen near us). None: no melee unit to take it, or more than `MAX_ARMY`.
pub fn composition(rules: &CompiledRuleset, arsenal: &Arsenal, city: &CityEstimate, threat: u32) -> Option<Composition> {
    let melee = arsenal.melee?;
    let melee_strike = strike(rules, melee, ArmyRole::Melee, city);
    let distance_role = if arsenal.siege.is_some() { ArmyRole::Siege } else { ArmyRole::Ranged };
    let distance_strike = arsenal.kind(distance_role).map(|k| strike(rules, k, distance_role, city));
    let mut army = Composition { melee: 2, ..Composition::default() };
    if distance_strike.is_some() {
        army.add(distance_role);
    }
    loop {
        let mut group = besiegers(army, melee_strike, distance_strike);
        if siege_rounds(&mut group, city.hp, city.heal).is_some_and(|r| r <= PLAN_SIEGE_ROUNDS) {
            break;
        }
        if army.total() >= MAX_ARMY {
            return None;
        }
        let heavy = melee_strike.taken >= HEAVY_STRIKE_HP;
        let few_distance = army.ranged + army.siege < army.melee;
        match distance_strike {
            Some(_) if heavy || few_distance => army.add(distance_role),
            _ => army.add(ArmyRole::Melee),
        }
    }
    // One escort per strength of our best melee unit in the enemy's field
    // power (strength × health) seen near us.
    let strength = u32::from(rules.units[melee.0 as usize].strength).max(1);
    let escorts = threat.div_ceil(strength).min(u32::from(MAX_ESCORTS)) as u8;
    army.melee += escorts;
    (army.total() <= MAX_ARMY).then_some(army)
}

fn besiegers(army: Composition, melee: Besieger, distance: Option<Besieger>) -> Vec<Besieger> {
    let mut group = vec![melee; usize::from(army.melee)];
    if let Some(distance) = distance {
        group.extend(std::iter::repeat_n(distance, usize::from(army.ranged + army.siege)));
    }
    group
}

// ---- The turn's plan: start, advance, end, recruit ----

/// The operations of one turn (at its start; their one owner): advance or
/// end the running ones, start one when none runs and a target is in
/// reach, recruit members.
pub fn plan(brief: &TurnBrief, state: &mut AiState, rules: &CompiledRuleset, routes: &mut Routes<'_>, gate: WarGate<'_>) -> Vec<Decision> {
    let mut out = Vec::new();
    for operation in state.operations().to_vec() {
        out.extend(advance(brief, state, rules, &operation, gate.decisive(operation.target)));
    }
    if state.operations().is_empty() {
        out.extend(start(brief, state, rules, routes, gate));
    }
    for operation in state.operations().to_vec() {
        recruit(brief, state, rules, &operation, gate.decisive(operation.target));
    }
    out
}

/// Peace in every war old enough that no operation fights (the war was for
/// an operation that ended, or the enemy's cities are out of reach) and
/// that `decided` does not end already.
pub fn idle_wars(brief: &TurnBrief, state: &AiState, decided: &[Decision]) -> Vec<Decision> {
    let fought = |enemy: PlayerId| state.operations().iter().any(|o| o.enemy == enemy);
    let ended = |enemy: PlayerId| decided.iter().any(|d| matches!(d.command, Some(Command::MakePeace { enemy: e, .. }) if e == enemy));
    brief
        .relations
        .iter()
        .filter(|r| r.at_war && !fought(r.other) && !ended(r.other))
        .filter(|r| rounds_since(brief, state.relation(r.other).and_then(|m| m.last_change_round)) >= MIN_WAR_ROUNDS)
        .map(|r| {
            let command = Command::MakePeace { player: brief.player, enemy: r.other };
            Decision::act(Actor::Player, Reason::MakePeace, command)
        })
        .collect()
}

/// The army's ways over the player's known map.
pub struct Routes<'a> {
    view: PlayerView<'a>,
    scratch: MapScratch,
    stats: SearchStats,
}

impl<'a> Routes<'a> {
    pub fn new(view: PlayerView<'a>) -> Self {
        Self { view, scratch: MapScratch::default(), stats: SearchStats::default() }
    }

    /// Where an army of `kind` coming from `from` stages for `target`:
    /// the reachable tile nearest to it out of its strike
    /// (`STAGING_DISTANCE`), cheapest first — at peace the enemy's closed
    /// borders stop the walk there. None: the known way ends farther than
    /// `BORDER_REACH` (another landmass, or beyond the search budget).
    pub fn stage(&mut self, kind: UnitKindId, from: TileId, target: TileId) -> Option<Stage> {
        let map = KnownMap::of_kind(&self.view, kind);
        let dims = self.view.dimensions();
        let mut best: Option<(i32, u32, TileId)> = None;
        let mut visit = |tile: TileId, points: u32| {
            let distance = dims.distance(tile, target).unwrap_or(i32::MAX);
            if distance < STAGING_DISTANCE {
                return Visit::Continue;
            }
            if best.is_none_or(|b| (distance, points, tile) < b) {
                best = Some((distance, points, tile));
            }
            if distance == STAGING_DISTANCE { Visit::Stop } else { Visit::Continue }
        };
        let limit = DecisionBudget::default().search();
        explore(&map, from, &Blockers::new(Vec::new()), &mut self.scratch, limit, &mut self.stats, &mut visit);
        let (distance, points, tile) = best.filter(|(d, _, _)| *d <= BORDER_REACH)?;
        let moves = self.view.rules().units[kind.0 as usize].movement.max(1) * crate::world::unit::MOVE_DENOMINATOR;
        // The rest is the enemy's land, walked at war: two thirds of a tile
        // a round.
        let strike = (distance as u32 - 1).saturating_mul(2).div_ceil(3) + 1;
        Some(Stage { tile, walk: turns_for(points, moves), strike })
    }
}

/// An army's staging tile on the way to its target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stage {
    pub tile: TileId,
    /// Rounds from the muster city to the staging tile.
    pub walk: u32,
    /// Rounds from the staging tile to the target.
    pub strike: u32,
}

/// The members of the operation against `target`, by id.
pub(crate) fn members<'b>(brief: &'b TurnBrief, state: &AiState, target: TileId) -> Vec<&'b UnitBrief> {
    let task = Some(UnitTask::Operation { target });
    brief.units.iter().filter(|u| state.task_of(u.id).map(|t| t.task) == task).collect()
}

fn target_held(state: &AiState, operation: &Operation) -> bool {
    state.seen_cities().iter().any(|c| c.tile == operation.target && c.owner == operation.enemy)
}

/// End, launch or keep an operation; a `decisive` one (its capture wins the
/// game) forms for up to `DECISIVE_FORM_LIMIT` rounds.
fn advance(brief: &TurnBrief, state: &mut AiState, rules: &CompiledRuleset, operation: &Operation, decisive: bool) -> Vec<Decision> {
    let members = members(brief, state, operation.target);
    let age = brief.round.saturating_sub(operation.phase_round);
    let at_war = brief.at_war(operation.enemy);
    let form_limit = if decisive { DECISIVE_FORM_LIMIT } else { FORM_LIMIT };
    let over = match operation.phase {
        _ if !target_held(state, operation) => true,
        OperationPhase::Forming => age > form_limit,
        OperationPhase::Marching => !at_war || age > MARCH_LIMIT || outmatched(brief, rules, operation, &members),
    };
    if over {
        state.end_operation(operation.target);
        return vec![Decision::refuse(Actor::Player, Reason::EndOperation)];
    }
    let ready = assembled(brief, rules, operation, &members, age) && !outmatched(brief, rules, operation, &members);
    if operation.phase != OperationPhase::Forming || !ready {
        return Vec::new();
    }
    launch(brief, state, operation, at_war)
}

/// Enemy units seen defending the target are half as strong again as the
/// members (strength × health).
fn outmatched(brief: &TurnBrief, rules: &CompiledRuleset, operation: &Operation, members: &[&UnitBrief]) -> bool {
    let ours: u32 = members.iter().map(|u| unit_power(rules, u.kind, u.hp)).sum();
    2 * defence(brief, operation.enemy, operation.target) > 3 * ours
}

/// Power of `enemy`'s military units the player sees within
/// `DEFENCE_RADIUS` of `target`.
fn defence(brief: &TurnBrief, enemy: PlayerId, target: TileId) -> u32 {
    let near = |tile: TileId| brief.dims.distance(tile, target).is_some_and(|d| d <= DEFENCE_RADIUS);
    brief.sightings.iter().filter(|s| s.owner == enemy && s.military && near(s.tile)).map(|s| s.power).sum()
}

/// The army near the muster point covers the need, or three quarters of it
/// (a melee unit among them) after `FORM_PATIENCE`.
fn assembled(brief: &TurnBrief, rules: &CompiledRuleset, operation: &Operation, members: &[&UnitBrief], age: u32) -> bool {
    let dims = brief.dims;
    let present = members
        .iter()
        .filter(|u| u.hp >= HEAL_BELOW_HP && dims.distance(u.tile, operation.muster).is_some_and(|d| d <= MUSTER_RADIUS + 1));
    let present = composition_of(rules, present.map(|u| u.kind));
    if shortfall(operation.need, present).total() == 0 {
        return true;
    }
    age >= FORM_PATIENCE && present.melee > 0 && present.total() * 4 >= operation.need.total() * 3
}

/// March: declare the war first when at peace (when allowed now), make the
/// city the war's objective.
fn launch(brief: &TurnBrief, state: &mut AiState, operation: &Operation, at_war: bool) -> Vec<Decision> {
    let mut out = Vec::new();
    if !at_war {
        if !may_declare(brief, state, operation) {
            return out;
        }
        let command = Command::DeclareWar { player: brief.player, enemy: operation.enemy };
        out.push(Decision::act(Actor::Player, Reason::DeclareWar, command));
    }
    let objective = WarObjective { tile: operation.target, kind: ObjectiveKind::City, chosen_round: brief.round };
    state.relation_mut(operation.enemy).objective = Some(objective);
    state.set_operation_phase(operation.target, OperationPhase::Marching, brief.round);
    out.push(Decision::refuse(Actor::Player, Reason::LaunchOperation));
    out
}

/// A new war is allowed: the season, no other war, the cooldown after the
/// last peace with this player.
fn may_declare(brief: &TurnBrief, state: &AiState, operation: &Operation) -> bool {
    let last = state.relation(operation.enemy).and_then(|m| m.last_change_round);
    brief.round >= EARLIEST_WAR_ROUND
        && !brief.relations.iter().any(|r| r.at_war)
        && rounds_since(brief, last) >= PEACE_COOLDOWN_ROUNDS
}

/// A candidate operation and its time to march and to arrive.
#[derive(Debug, Clone, Copy)]
struct Candidate {
    operation: Operation,
    horizon: Horizon,
    assemble: u32,
    march: u32,
    distance: i32,
    /// Its value under `WarGate::Value` (0 otherwise).
    value: i64,
    /// The treasury its army's upkeep needs.
    chest: u32,
    /// Its capture wins the game (`Stakes`): its army may take
    /// `DECISIVE_FORM_LIMIT` rounds to form, from a city up to
    /// `DECISIVE_RANGE` away.
    decisive: bool,
}

/// Rounds the army of a capture that wins the game may take to form.
pub const DECISIVE_FORM_LIMIT: u32 = 2 * FORM_LIMIT;
/// The farthest a capture that wins the game may lie from our nearest
/// city: rival capitals lie mostly 16–30 tiles from it on the duel suite,
/// where `OBJECTIVE_RANGE` left three quarters of the known ones out of
/// reach (classic/docs/ai-leaders.md "Capital search"); the route to the staging tile
/// and the horizon still decide.
pub const DECISIVE_RANGE: i32 = 32;

impl Candidate {
    fn key(&self) -> (u32, i32, TileId) {
        (self.assemble + self.march, self.distance, self.operation.target)
    }

    /// The army arrives and takes the city before war weariness: at peace
    /// the war starts when it marches, at war the war's age counts.
    fn fits(&self) -> bool {
        let fight = self.march + SIEGE_ROUNDS_ESTIMATE;
        match self.horizon {
            Horizon::Peace => self.assemble <= self.form_limit() && fight <= WAR_WEARINESS_ROUNDS / 2,
            Horizon::War(age) => self.assemble + fight + age <= WAR_WEARINESS_ROUNDS,
        }
    }

    fn form_limit(&self) -> u32 {
        if self.decisive {
            DECISIVE_FORM_LIMIT
        } else {
            FORM_LIMIT
        }
    }
}

/// How a new operation is admitted.
#[derive(Debug, Clone, Copy)]
pub enum WarGate<'p> {
    /// Its army exists but for `MAX_WAR_ORDERS` units and the treasury pays
    /// their upkeep (the `operations` variant).
    Orders,
    /// Its expected gain in score beats its cost, and the treasury first
    /// saves its war chest (`allocator`, classic/docs/ai-allocator.md); the stakes
    /// are a leader's beyond the score (`leaders`, `standard`,
    /// classic/docs/ai-leaders.md; neutral for `allocator`).
    Value(&'p Prices, &'p Stakes),
    /// No new operation (`allocator-peace`: the control of `allocator`).
    Never,
}

impl WarGate<'_> {
    /// The capture of `target` wins the game (a leader's stakes).
    fn decisive(&self, target: TileId) -> bool {
        matches!(self, WarGate::Value(_, stakes) if stakes.capital(target).is_some_and(|s| s.decisive))
    }
}

/// Start the operation that marches soonest among the targets it can take
/// within its horizon (classic/docs/ai-operations.md "War decision"): the best
/// estimates first, each confirmed by a known way from our nearest city to
/// a staging tile near the target — the operation's muster point. Under
/// `WarGate::Value` the chosen one starts once the treasury holds its war
/// chest; until then it is the player's war plan.
fn start(brief: &TurnBrief, state: &mut AiState, rules: &CompiledRuleset, routes: &mut Routes<'_>, gate: WarGate<'_>) -> Option<Decision> {
    if matches!(gate, WarGate::Never) {
        return None;
    }
    let ready = in_season(brief)
        && brief.cities.len() >= 2
        && brief.round + PREPARE_ROUNDS >= EARLIEST_WAR_ROUND
        && matches!(state.strategy.current, Strategy::Expand | Strategy::Stabilize | Strategy::Attack);
    if !ready {
        return None;
    }
    let arsenal = Arsenal::of(brief, rules);
    let walker = arsenal.melee?;
    let free = composition_of(rules, free_units(brief, state, rules).map(|u| u.kind));
    let mut candidates: Vec<Candidate> = targets(brief, state)
        .into_iter()
        .filter_map(|(city, horizon)| candidate(brief, state, rules, &arsenal, free, &city, horizon, gate))
        .filter(Candidate::fits)
        .collect();
    candidates.sort_by_key(Candidate::key);
    let mut chosen = None;
    for mut candidate in candidates.into_iter().take(MAX_ROUTE_CHECKS) {
        let stage = routes.stage(walker, candidate.operation.muster, candidate.operation.target);
        let Some(stage) = stage else { continue };
        candidate.operation.muster = stage.tile;
        candidate.assemble += stage.walk;
        candidate.march = stage.strike;
        if candidate.fits() {
            chosen = Some(candidate);
            break;
        }
    }
    if matches!(gate, WarGate::Value(..)) {
        chosen = war_plan(brief, state, chosen);
    }
    state.start_operation(chosen?.operation);
    Some(Decision::refuse(Actor::Player, Reason::FormOperation))
}

/// The war plan's turn (classic/docs/ai-allocator.md "Military treasury"): a plan keeps
/// its target while the target is still a candidate worth at least
/// `PLAN_KEEP_PERCENT` of the value it was chosen at, for `PLAN_LIMIT`
/// rounds; a new candidate replaces it only when worth `PLAN_SWITCH_PERCENT`
/// of it. The plan's operation starts once the treasury holds its chest.
fn war_plan(brief: &TurnBrief, state: &mut AiState, chosen: Option<Candidate>) -> Option<Candidate> {
    let kept = state.war_plan.filter(|plan| {
        let fresh = brief.round.saturating_sub(plan.since_round) <= crate::ai::genome::get_u32(crate::ai::genome::Knob::PlanLimit);
        let worth = chosen.is_some_and(|c| c.operation.target != plan.target || c.value * 100 >= plan.value * crate::ai::genome::get(crate::ai::genome::Knob::PlanKeepPercent));
        fresh && worth
    });
    let candidate = chosen?;
    let replaces = kept.is_none_or(|plan| plan.target == candidate.operation.target || candidate.value * 100 >= plan.value * crate::ai::genome::get(crate::ai::genome::Knob::PlanSwitchPercent));
    if !replaces {
        state.war_plan = kept;
        return None;
    }
    if brief.treasury >= candidate.chest {
        state.war_plan = None;
        return Some(candidate);
    }
    let since_round = kept.filter(|p| p.target == candidate.operation.target).map_or(brief.round, |p| p.since_round);
    let value = kept.filter(|p| p.target == candidate.operation.target).map_or(candidate.value, |p| p.value);
    state.war_plan = Some(WarPlan { target: candidate.operation.target, enemy: candidate.operation.enemy, chest: candidate.chest, value, since_round });
    None
}

/// A war plan waits at most this many rounds for its chest.
pub const PLAN_LIMIT: u32 = 40;
/// A plan is dropped when its target is worth less than this share of its
/// value at the choice, percent.
pub const PLAN_KEEP_PERCENT: i64 = 70;
/// Another target replaces a plan only when worth this share of it.
pub const PLAN_SWITCH_PERCENT: i64 = 130;
/// Chance an operation takes its city, percent (step 3: of the operations
/// that marched with their army, about a third took the city or forced it).
pub const SUCCESS_PERCENT: i64 = 35;
/// Share of the army an operation expects to lose, percent.
pub const LOSS_PERCENT: i64 = 50;
/// Share of the city's points taken from the rival that counts for us,
/// percent: the time victory compares our score with the best other one.
pub const RIVAL_PERCENT: i64 = 50;

/// A war on an aggressor the world remembers is worth more (`ai::infamy`).
fn infamy_bonus(brief: &TurnBrief, owner: PlayerId) -> Centi {
    brief.relation(owner).map_or(0, |r| crate::ai::infamy::war_bonus(r.warmonger))
}

/// What an operation against `city` is worth (hundredths of score points):
/// its case (`war_case`) as the leader's stakes weigh its gain.
#[allow(clippy::too_many_arguments)]
fn war_value(prices: &Prices, stakes: &Stakes, rules: &CompiledRuleset, arsenal: &Arsenal, city: &SeenCity, need: Composition, missing: Composition, rounds: u32, armed: Option<&crate::ai::strikes::Armed>) -> i64 {
    war_case(prices, stakes, rules, arsenal, city, need, missing, rounds, armed).value(stakes)
}

/// An operation's prospect in hundredths of score points: its expected
/// gain — the chance it takes its city times what the capture brings,
/// before a leader's appetite weighs it — and the army it costs.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WarCase {
    pub gain: Centi,
    pub cost: Centi,
}

impl WarCase {
    /// The case as a leader weighs it.
    pub fn value(&self, stakes: &Stakes) -> Centi {
        stakes.weigh(self.gain) - self.cost
    }
}

/// The case of an operation against `city`: the chance it takes the city
/// times the city's points, its yields for the rest of the game and the
/// points the rival loses, less the unhappiness of a conquered city; and
/// the army it costs — the units to build (their hammers at the production
/// price, their upkeep to the end) and the units it expects to lose. A
/// leader's stakes scale the unrest and add the victory an original
/// capital brings closer; a capture that wins the game is worth that
/// victory alone (nothing after it counts).
#[allow(clippy::too_many_arguments)]
fn war_case(prices: &Prices, stakes: &Stakes, rules: &CompiledRuleset, arsenal: &Arsenal, city: &SeenCity, need: Composition, missing: Composition, rounds: u32, armed: Option<&crate::ai::strikes::Armed>) -> WarCase {
    let stake = stakes.capital(city.tile);
    let decisive = stake.is_some_and(|s| s.decisive);
    let gain = match stake {
        Some(stake) if decisive => stake.value,
        _ => city_gain(prices, stakes, city, rounds) + stake.map_or(0, |s| s.value),
    };
    // Upkeep to the end of the game, which a decisive capture ends.
    let upkeep = if decisive { prices.gold * i64::from(rounds) / i64::from(prices.rounds_left) } else { prices.gold };
    let hammer = prices.production / i64::from(prices.rounds_left);
    let unit_cost = |role: ArmyRole| {
        let kind = arsenal.kind(role).or(arsenal.ranged).or(arsenal.melee);
        i64::from(kind.map_or(0, |k| rules.units[k.0 as usize].cost))
    };
    let army = |c: Composition| [ArmyRole::Melee, ArmyRole::Ranged, ArmyRole::Siege].iter().map(|r| unit_cost(*r) * i64::from(c.of(*r))).sum::<i64>();
    let built = army(missing) * hammer + i64::from(missing.total()) * upkeep;
    let lost = army(need) * crate::ai::genome::get(crate::ai::genome::Knob::LossPercent) / 100 * hammer;
    let base = crate::ai::genome::get(crate::ai::genome::Knob::SuccessPercent);
    // The weapon the plan counts on (`strikes::armory`) gives the chance it gives and costs its
    // hammers and, for a nuclear one, the world's memory of it.
    let (success, weapons) = armed.map_or((base, 0), |a| (a.success, i64::from(a.extra_hammers) * hammer + a.public_cost));
    WarCase { gain: success * gain / 100, cost: built + lost + weapons }
}

/// What a running operation is still worth (`war_value`): its target with
/// the army it has — only the units it lacks are still to be built — over
/// the rounds its phase may still take and the siege. None: its target is
/// no longer known.
pub(crate) fn running_value(brief: &TurnBrief, state: &AiState, rules: &CompiledRuleset, prices: &Prices, stakes: &Stakes, operation: &Operation) -> Option<i64> {
    let city = state.seen_cities().iter().find(|c| c.tile == operation.target)?;
    let arsenal = Arsenal::of(brief, rules);
    let have = composition_of(rules, members(brief, state, operation.target).iter().map(|u| u.kind));
    let missing = shortfall(operation.need, have);
    let limit = match operation.phase {
        OperationPhase::Forming => FORM_LIMIT,
        OperationPhase::Marching => MARCH_LIMIT,
    };
    let rounds = limit.saturating_sub(brief.round.saturating_sub(operation.phase_round)) + SIEGE_ROUNDS_ESTIMATE;
    Some(war_value(prices, stakes, rules, &arsenal, city, operation.need, missing, rounds, crate::ai::strikes::on_hand_best(&brief.armory)) + infamy_bonus(brief, operation.enemy))
}

/// The case of going on with the operation against `enemy`, if one runs
/// and its target is still the enemy's: the army it still lacks and the
/// units it expects to lose against what its capture brings
/// (`ai::peace`).
pub fn running_case(brief: &TurnBrief, state: &AiState, rules: &CompiledRuleset, prices: &Prices, stakes: &Stakes, enemy: PlayerId) -> Option<WarCase> {
    let operation = state.operations().iter().find(|o| o.enemy == enemy)?;
    let city = state.seen_cities().iter().find(|c| c.tile == operation.target && c.owner == enemy)?;
    let have = composition_of(rules, members(brief, state, operation.target).iter().map(|u| u.kind));
    let missing = shortfall(operation.need, have);
    let distance = brief.dims.distance(operation.muster, operation.target).unwrap_or(0).max(0) as u32;
    let rounds = (distance * 2).div_ceil(3) + 1 + SIEGE_ROUNDS_ESTIMATE;
    let mut case = war_case(prices, stakes, rules, &Arsenal::of(brief, rules), city, operation.need, missing, rounds, crate::ai::strikes::on_hand_best(&brief.armory));
    case.gain += infamy_bonus(brief, enemy);
    Some(case)
}

/// The score a captured city brings: its worth less its unrest.
fn city_gain(prices: &Prices, stakes: &Stakes, city: &SeenCity, rounds: u32) -> i64 {
    let unhappiness = prices.captured_city_unrest(city.population) * stakes.unrest / 100;
    city_worth(prices, city.population, rounds) - unhappiness
}

/// What a city of `population` is worth to its holder from `rounds` on:
/// its points, its yields to the end and half the points its rival would
/// have with it. A captured city is worth this to the conqueror, a lost one
/// to its owner (`ai::defence`).
pub fn city_worth(prices: &Prices, population: u32, rounds: u32) -> Centi {
    let score = prices.score;
    let population = i64::from(population.max(1));
    let points = score.city + score.land * (crate::ai::value::FIRST_RING + population) + score.citizen * population;
    let yields = prices.later(population * (prices.production + prices.science) / 2, rounds);
    points + yields + points * crate::ai::genome::get(crate::ai::genome::Knob::RivalPercent) / 100
}

/// A player without an operation looks for one this turn: every turn in a
/// war, every `START_INTERVAL` rounds in peace.
pub fn in_season(brief: &TurnBrief) -> bool {
    let at_war = brief.relations.iter().any(|r| r.at_war);
    at_war || (brief.round + u32::from(brief.player.0)) % START_INTERVAL == 0
}

/// How long the war may still last when the army arrives.
#[derive(Debug, Clone, Copy)]
enum Horizon {
    /// At peace: the war starts when the army marches.
    Peace,
    /// At war for this many rounds.
    War(u32),
}

/// The enemy cities an operation may aim at: of the players at war with
/// us; with no war, of the players at peace past the cooldown.
fn targets(brief: &TurnBrief, state: &AiState) -> Vec<(SeenCity, Horizon)> {
    let at_war: Vec<_> = brief.relations.iter().filter(|r| r.at_war).collect();
    let mut out = Vec::new();
    for relation in &brief.relations {
        let last = state.relation(relation.other).and_then(|m| m.last_change_round);
        let horizon = match (relation.at_war, at_war.is_empty()) {
            (true, _) => Horizon::War(rounds_since(brief, last).min(WAR_WEARINESS_ROUNDS)),
            (false, true) if rounds_since(brief, last) >= PEACE_COOLDOWN_ROUNDS => Horizon::Peace,
            _ => continue,
        };
        out.extend(state.seen_cities().iter().filter(|c| c.owner == relation.other).map(|c| (*c, horizon)));
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn candidate(
    brief: &TurnBrief,
    state: &AiState,
    rules: &CompiledRuleset,
    arsenal: &Arsenal,
    free: Composition,
    city: &SeenCity,
    horizon: Horizon,
    gate: WarGate<'_>,
) -> Option<Candidate> {
    let muster = brief.nearest_city(brief.dims, city.tile)?;
    let distance = brief.dims.distance(muster.tile, city.tile)?;
    let decisive = gate.decisive(city.tile);
    if distance > if decisive { DECISIVE_RANGE } else { OBJECTIVE_RANGE } {
        return None;
    }
    let threat = state.relation(city.owner).map_or(0, |m| m.threat).max(defence(brief, city.owner, city.tile));
    let estimate = estimate_city(brief, rules, city, arsenal.melee);
    // Before the route is known: the whole way at two thirds of a tile a
    // round (`start` replaces it by the staging tile's walk and strike).
    let march = (distance as u32 * 2).div_ceil(3) + 1;
    let plan = |need: Composition, armed: Option<&crate::ai::strikes::Armed>| -> Option<Candidate> {
        let missing = shortfall(need, free);
        let assemble = assemble_rounds(brief, rules, arsenal, missing).max(armed.map_or(0, |a| a.ready));
        let value = match gate {
            WarGate::Orders if armed.is_some() || missing.total() > MAX_WAR_ORDERS || !affordable(brief, missing) => return None,
            WarGate::Orders => 0,
            WarGate::Value(prices, stakes) => {
                war_value(prices, stakes, rules, arsenal, city, need, missing, assemble + march + SIEGE_ROUNDS_ESTIMATE, armed) + infamy_bonus(brief, city.owner)
            }
            WarGate::Never => return None,
        };
        let operation = Operation {
            target: city.tile,
            enemy: city.owner,
            muster: muster.tile,
            need,
            phase: OperationPhase::Forming,
            since_round: brief.round,
            phase_round: brief.round,
        };
        let chest = chest(brief, missing);
        (value >= 0).then_some(Candidate { operation, horizon, assemble, march, distance, value, chest, decisive })
    };
    // The army alone, and with each weapon the player has or could build: the
    // weapon strikes the city before the army comes (`strikes::struck`), the army it
    // needs is the army for what is left, and its price and project are in the
    // case. The best plan that fits in time counts; a city the army alone cannot
    // take (more than `MAX_ARMY` units) can be taken with the weapon.
    let mut best: Option<Candidate> = composition(rules, arsenal, &estimate, threat).and_then(|need| plan(need, None)).filter(Candidate::fits);
    for armed in &brief.armory {
        let hit = crate::ai::strikes::struck(brief, rules, city, &estimate, armed);
        let Some(need) = composition(rules, arsenal, &hit, threat) else { continue };
        if let Some(armed_plan) = plan(need, Some(armed)).filter(Candidate::fits) {
            if best.as_ref().is_none_or(|b| armed_plan.value > b.value) {
                best = Some(armed_plan);
            }
        }
    }
    best
}

/// The missing units' upkeep fits the income or the war chest.
fn affordable(brief: &TurnBrief, missing: Composition) -> bool {
    brief.treasury >= chest(brief, missing)
}

/// The gold that pays the deficit the missing units' upkeep causes for
/// `WAR_CHEST_ROUNDS`.
fn chest(brief: &TurnBrief, missing: Composition) -> u32 {
    let units = brief.units.len();
    let extra = unit_upkeep(units + missing.total() as usize, brief.round) - unit_upkeep(units, brief.round);
    (extra - brief.gold_income).max(0) as u32 * WAR_CHEST_ROUNDS
}

/// The treasury pays the deficit `extra` upkeep would cause for
/// `WAR_CHEST_ROUNDS` (an operation's army may cost more than the income:
/// the war is short, a treasury below zero would disband units).
pub fn war_chest_covers(income: i32, treasury: u32, extra: i32) -> bool {
    let deficit = (extra - income).max(0) as u32;
    deficit.saturating_mul(WAR_CHEST_ROUNDS) <= treasury
}

/// Rounds to build the missing units with half of the empire's production,
/// plus the walk to the muster point.
fn assemble_rounds(brief: &TurnBrief, rules: &CompiledRuleset, arsenal: &Arsenal, missing: Composition) -> u32 {
    let cost = |role: ArmyRole| {
        let kind = arsenal.kind(role).or(arsenal.ranged).or(arsenal.melee);
        kind.map_or(0, |k| rules.units[k.0 as usize].cost) * u32::from(missing.of(role))
    };
    let total = cost(ArmyRole::Melee) + cost(ArmyRole::Ranged) + cost(ArmyRole::Siege);
    let production: u32 = brief.cities.iter().map(|c| c.production).sum();
    total.div_ceil((production / 2).max(1)) + GATHER_ROUNDS
}

/// The player's healthy army units no task holds: none, on an assault, or
/// a garrison beyond what its city wants (but never a guard of a city in
/// danger, `ai::defence`).
pub(crate) fn free_units<'b>(brief: &'b TurnBrief, state: &'b AiState, rules: &'b CompiledRuleset) -> impl Iterator<Item = &'b UnitBrief> {
    let wanted = garrison_wanted(brief.round, &brief.relations, state.strategy.current).max(1);
    brief.units.iter().filter(move |u| {
        let free = match state.task_of(u.id).map(|t| t.task) {
            None | Some(UnitTask::Assault { .. }) => true,
            Some(UnitTask::Defend { city }) if brief.endangered(city) => false,
            Some(UnitTask::Defend { city }) => senior_defenders(state, city, u.id) >= wanted,
            Some(_) => false,
        };
        free && u.hp >= HEAL_BELOW_HP && army_role(rules, u.kind).is_some()
    })
}

/// Garrison holders of `city` with a lower id than `unit`.
fn senior_defenders(state: &AiState, city: TileId, unit: EntityId) -> usize {
    state.tasks().iter().filter(|t| t.task == UnitTask::Defend { city } && t.unit < unit).count()
}

/// Give free units to the operation: while it forms, the nearest to the
/// muster point while a role is missing, then up to `EXTRA_MEMBERS` more —
/// within `RECRUIT_RANGE`, or `DECISIVE_RANGE` for a `decisive` one (its
/// muster point may lie farther from every city); while it marches, every
/// unit on an assault of its target.
fn recruit(brief: &TurnBrief, state: &mut AiState, rules: &CompiledRuleset, operation: &Operation, decisive: bool) {
    let range = if decisive { DECISIVE_RANGE } else { RECRUIT_RANGE };
    let dims = brief.dims;
    let target = operation.target;
    let mut have = composition_of(rules, members(brief, state, target).iter().map(|u| u.kind));
    let mut candidates: Vec<(i32, EntityId, UnitKindId)> = free_units(brief, state, rules)
        .filter(|u| match operation.phase {
            OperationPhase::Forming => true,
            OperationPhase::Marching => state.task_of(u.id).map(|t| t.task) == Some(UnitTask::Assault { target }),
        })
        .map(|u| (dims.distance(u.tile, operation.muster).unwrap_or(i32::MAX), u.id, u.kind))
        .filter(|(d, _, _)| operation.phase == OperationPhase::Marching || *d <= range)
        .collect();
    candidates.sort_by_key(|(d, unit, _)| (*d, *unit));
    let task = UnitTask::Operation { target };
    for (_, unit, kind) in candidates {
        let Some(role) = army_role(rules, kind) else { continue };
        if operation.phase == OperationPhase::Forming && !wanted(operation.need, have, role) {
            continue;
        }
        have.add(role);
        state.set_task(unit, task, brief.round);
    }
}

/// A forming operation takes a unit of `role`: its role is missing, or the
/// need is covered and the army has room.
fn wanted(need: Composition, have: Composition, role: ArmyRole) -> bool {
    let missing = shortfall(need, have);
    let fills = match role {
        ArmyRole::Melee => missing.melee > 0,
        ArmyRole::Ranged | ArmyRole::Siege => missing.ranged + missing.siege > 0,
    };
    fills || (missing.total() == 0 && have.total() < need.total() + EXTRA_MEMBERS)
}

// ---- Members ----

/// One step of a member: to the muster point while forming, to the target
/// in a wave while marching (the front fights once enemies are in sight).
pub fn member_step(view: &UnitView<'_, '_>, state: &mut AiState, tools: &mut PlanTools<'_>, target: TileId) -> Decision {
    let Some(operation) = state.operation(target).copied() else {
        return Decision::refuse(Actor::Unit(view.local.unit.id), Reason::TargetGone);
    };
    let decision = match operation.phase {
        OperationPhase::Forming => muster_step(view, tools, &operation),
        OperationPhase::Marching => march_step(view, state, tools, &operation),
    };
    if decision.command.is_none() && !matches!(decision.reason, Reason::Waiting | Reason::Blocked) {
        return hold(view, Reason::Rally);
    }
    decision
}

/// To the muster point; a member within `MUSTER_RADIUS` of it and out of
/// the target's strike waits there.
fn muster_step(view: &UnitView<'_, '_>, tools: &mut PlanTools<'_>, operation: &Operation) -> Decision {
    let unit = view.local.unit;
    let dims = view.local.view.dimensions();
    if view.brief.landings {
        if let Some(jump) = muster_jump(view, operation) {
            return jump;
        }
    }
    let near = dims.distance(unit.tile, operation.muster).is_some_and(|d| d <= MUSTER_RADIUS);
    let safe = dims.distance(unit.tile, operation.target).is_some_and(|d| d >= STAGING_DISTANCE);
    if near && safe {
        return hold(view, Reason::Rally);
    }
    move_along(view, tools, operation.muster, Reason::Rally)
}

/// A paratrooper that stands in an own city and can jump (the rules' own
/// range and check, `Command::Paradrop`) goes to the tile where its
/// operation gathers instead of walking or sailing there: the tile nearest
/// the muster point that is land, out of the target's strike, at least
/// `JUMP_MIN_DISTANCE` away (a shorter way is walked) and with no enemy
/// unit in sight next to it. None: it walks.
fn muster_jump(view: &UnitView<'_, '_>, operation: &Operation) -> Option<Decision> {
    let unit = view.local.unit;
    if !unit.can_attack || !view.brief.cities.iter().any(|c| c.tile == unit.tile) {
        return None;
    }
    let range = view.local.view.paradrop_range(unit.handle);
    let dims = view.local.view.dimensions();
    let distance = |a: TileId, b: TileId| dims.distance(a, b).unwrap_or(i32::MAX);
    if range == 0 || distance(unit.tile, operation.muster) < JUMP_MIN_DISTANCE {
        return None;
    }
    let mut tiles: Vec<TileId> = crate::math::tiles_in_hex_radius(&dims, operation.muster, MUSTER_RADIUS)
        .into_iter()
        .filter(|t| {
            let reach = distance(unit.tile, *t);
            reach >= JUMP_MIN_DISTANCE && reach <= range && distance(*t, operation.target) >= STAGING_DISTANCE
        })
        .filter(|t| view.local.view.is_known(*t) && !crate::ai::fleet::water(&view.local.view, *t))
        .filter(|t| {
            !view.local.foreign_units.iter().any(|f| view.local.relation(f.owner).is_some_and(|r| r.at_war) && distance(f.tile, *t) <= 2)
        })
        .collect();
    tiles.sort_by_key(|t| (distance(*t, operation.muster), t.0));
    tiles.into_iter().take(JUMP_CANDIDATES).find_map(|destination| {
        let command = Command::Paradrop { unit: unit.handle, destination };
        view.local.view.accepts(&command).then(|| Decision::act(Actor::Unit(unit.id), Reason::Paradrop, command))
    })
}

/// A paratrooper jumps when the way to the muster point is at least this
/// long (a shorter one is walked in a round or two).
pub const JUMP_MIN_DISTANCE: i32 = 3;
/// Tiles near the muster point one jump tries.
pub const JUMP_CANDIDATES: usize = 7;

/// Toward the target; a member ahead of the members' median distance by
/// more than `COHESION` waits for the others.
fn march_step(view: &UnitView<'_, '_>, state: &AiState, tools: &mut PlanTools<'_>, operation: &Operation) -> Decision {
    let unit = view.local.unit;
    let dims = view.local.view.dimensions();
    let distance = |tile: TileId| dims.distance(tile, operation.target).unwrap_or(i32::MAX);
    let mut distances: Vec<i32> = members(view.brief, state, operation.target).iter().map(|u| distance(u.tile)).collect();
    distances.sort();
    let median = distances.get(distances.len() / 2).copied().unwrap_or(0);
    if distance(unit.tile) + COHESION < median {
        return hold(view, Reason::Rally);
    }
    approach(view, tools, operation.target, Reason::Advance)
}

// ---- Production ----

/// What the forming operations still lack beyond their members and the
/// units of their roles already queued: the cities' war orders.
pub fn demand(obs: &AIObservation<'_>, state: &AiState, rules: &CompiledRuleset) -> Composition {
    let mut need = Composition::default();
    for operation in state.operations().iter().filter(|o| o.phase == OperationPhase::Forming) {
        let task = Some(UnitTask::Operation { target: operation.target });
        let members = obs.own_units.iter().filter(|u| state.task_of(u.id).map(|t| t.task) == task).map(|u| u.kind);
        let missing = shortfall(operation.need, composition_of(rules, members));
        need.melee += missing.melee;
        need.ranged += missing.ranged;
        need.siege += missing.siege;
    }
    let queued = obs.own_cities.iter().flat_map(|c| &c.queue).filter_map(|item| match item {
        ProductionItem::Unit(kind) => Some(*kind),
        ProductionItem::Building(_) | ProductionItem::Project(_) => None,
    });
    shortfall(need, composition_of(rules, queued))
}
