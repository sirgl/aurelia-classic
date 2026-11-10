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
use crate::ai::front::{siege_rounds, wears_down, Besieger};
use crate::ai::planner::{role_of, unit_power, AiRole, Decision, DecisionBudget, PlanTools, TurnBrief, UnitBrief, UnitView};
use crate::ai::planner_map::{explore, turns_for, Blockers, KnownMap, MapScratch, SearchStats, Visit};
use crate::ai::planner_strategy::{
    rounds_since, EARLIEST_WAR_ROUND, MIN_WAR_ROUNDS, OBJECTIVE_RANGE, PEACE_COOLDOWN_ROUNDS, PREPARE_ROUNDS,
    WAR_WEARINESS_ROUNDS,
};
use crate::ai::planner_units::{approach, garrison_wanted_by, hold, move_along, HEAL_BELOW_HP};
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
use crate::derived::income::unit_upkeep_at;
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

/// Rounds a marching operation may last. The check that admits an operation (`Candidate::fits`)
/// lets the march and the siege take `WAR_WEARINESS_ROUNDS / 2`; `MARCH_LIMIT` ended
/// the operations that check had admitted (22 % of the operations that set out ended by it,
/// classic/docs/ai-war-audit-current.md), so the war-fix variants end an operation at the
/// time the check allows.
pub fn march_limit(brief: &TurnBrief) -> u32 {
    if brief.war.march { WAR_WEARINESS_ROUNDS / 2 } else { MARCH_LIMIT }
}
/// The age at which a marching operation ends: `march_limit`; for `war-muster`
/// (`TurnBrief::war_muster`) no later than the time its plan gave the march and the siege (the walk
/// of `running_case`) with the siege estimate once and a half. The army is composed to take the city
/// in `PLAN_SIEGE_ROUNDS` rounds of strikes; a siege that outlasts its estimate by half is not the
/// one the army was planned for, and the operations that ran to `march_limit` lost 25 units each
/// (median) for the 8 of 17 heavy losers of offset 200 that ended by it, while every operation that
/// ended by taking its city had taken at most its planned time (`war-muster` before this rule,
/// runs/ops/wm2-200.csv, wm3-200.csv).
fn march_end(brief: &TurnBrief, operation: &Operation) -> u32 {
    if !brief.war_muster {
        return march_limit(brief);
    }
    let distance = brief.dims.distance(operation.muster, operation.target).unwrap_or(0).max(0) as u32;
    let planned = (distance * 2).div_ceil(3) + 1 + SIEGE_ROUNDS_ESTIMATE + SIEGE_ROUNDS_ESTIMATE / 2;
    planned.min(march_limit(brief))
}
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
// FORM_PATIENCE, FORM_LIMIT and WAR_BUILD_TURNS are Standard rounds: forming is
// production-bound, so another speed reads them × its pace (`SpeedValues::pace_rounds`).
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

/// The changes of the war logic a variant carries (`Variant::war_tuning`); all off is the game's AI
/// (classic/docs/ai-war-audit-current.md, classic/docs/ai-raids.md).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct WarTuning {
    /// A march may last as long as the check that admits an operation lets it
    /// (`march_limit`), not the shorter `MARCH_LIMIT`.
    pub march: bool,
    /// The unrest of a captured city is priced by the city's size and the empire's
    /// happiness curve (`Prices::captured_city_unrest_curve`).
    pub unrest: bool,
    /// Garrisons beyond what a city needs in peace count as free units (`free_units`).
    pub group: bool,
    /// Raids (`ai::raid`).
    pub raids: bool,
    /// The defence recalls an operation only for a saving the army can make and that beats
    /// the operation's worth by a margin (`defence::recall`).
    pub recall: bool,
    /// The fixes of the defects that kept the AI from ever taking a city (`SiegeFixes`).
    pub siege: SiegeFixes,
}

impl WarTuning {
    pub const NONE: WarTuning = WarTuning { march: false, unrest: false, group: false, raids: false, recall: false, siege: SiegeFixes::NONE };
}

/// The fixes of the class of defects that kept the AI from ever taking a city (the
/// `siege` variants): an assault counted as free, garrisons wanted whatever the danger,
/// a siege that waited for a fall within a constant of rounds, a constant chance of
/// success and a recovery that never ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SiegeFixes {
    /// A unit on an assault or an operation is worth its share of what its target is worth
    /// (`task_shares`): the defence or a new operation takes it only when it saves or brings
    /// more than that.
    pub assault: bool,
    /// Each city wants its guard by its own danger: the Defend strategy no longer doubles
    /// every garrison (`planner_units::garrison_wanted_by`), and it has a hysteresis
    /// (`planner_strategy::threatened`).
    pub garrison: bool,
    /// A siege group closes in when its expected damage a round beats the city's healing
    /// (`front::wears_down`), not only when the city falls within `front::SIEGE_ROUNDS`.
    pub close: bool,
    /// The chance an operation takes its city comes from the siege model and the army's
    /// power over the city's (`siege_chance`), not `SUCCESS_PERCENT`.
    pub chance: bool,
    /// A city lost long enough ago no longer keeps the strategy in Recover, and Recover
    /// starts operations and raids (to take it back among others).
    pub recover: bool,
    /// The chance an operation takes its city is the one learned from the outcomes of
    /// operations in bench games (`learned_chance`, classic/docs/ai-war-fix.md), not the
    /// constant nor the siege model.
    pub learned: bool,
    /// A captured city is priced by what it changes for the conqueror under the rules
    /// (`captured_gain`): the population the capture leaves, the resistance before it yields,
    /// the puppet or the annexed city the AI makes of it, its unrest by the moods it crosses.
    pub priced: bool,
    /// A shot at a city whose siege group wears it down is worth the city's share of
    /// what taking it is worth (`front::city_damage_worth`), not its hit points alone: the
    /// wounded and exposed weigh their risk against the siege, not against the damage.
    pub worth: bool,
    /// The units an operation has built stay the player's when they live: the war costs the
    /// units it expects to lose and the upkeep of the units it builds (`war_case`), not the
    /// hammers of every unit built besides (the army "spent whole", 2026-10-31 defect list).
    pub asset: bool,
    /// With `priced`: the captured city is priced as the puppet the AI makes of it since the
    /// eighth promotion (`captured_parts`), not annexed at `combat::ANNEX_POPULATION` citizens
    /// (`Variant::PuppetWar`, classic/docs/ai-conquest-payoff.md "Promoted").
    pub puppet: bool,
    /// With `priced`: a mood line the captured city's unrest pushes the empire below costs its
    /// rate for the rounds the empire takes to climb back (`value::UNREST_RECOVERY_ROUNDS`), not
    /// to the end of the game (`Prices::kept_city_unrest_recovering`; `war-open`).
    pub recovery: bool,
    /// The army an operation expects to lose is the share its members lost in the operations
    /// measured (`OPEN_LOSS_PERCENT`), not the constant `LOSS_PERCENT` (`war-open`).
    pub loss: bool,
    /// With `priced`: a captured city's citizens yield what the empire's own do (`Prices::
    /// citizen_yield`: production, science, gold, culture, faith), not half a hammer and half a
    /// beaker each (`city_worth`; `war-open`).
    pub yields: bool,
    /// With `yields`: the rival's loss counts by where it stands (`rival_percent`): all of the
    /// city's points and yields when it leads the others, half when it is ahead of us, nothing
    /// when it is behind (the time victory compares our score with the best other one; a war on a
    /// laggard feeds the bystanders; `war-open`).
    pub rank: bool,
    /// With `loss`: the loss is the one measured on `war-pays`'s own operations
    /// (`PAYS_LOSS_PERCENT`), not `war-open`'s (`war-pays`).
    pub pays: bool,
}

impl SiegeFixes {
    pub const NONE: SiegeFixes = SiegeFixes { assault: false, garrison: false, close: false, chance: false, recover: false, learned: false, priced: false, worth: false, asset: false, puppet: false, recovery: false, loss: false, yields: false, rank: false, pays: false };
    /// The five fixes of the `siege` variant.
    pub const ALL: SiegeFixes = SiegeFixes { assault: true, garrison: true, close: true, chance: true, recover: true, learned: false, priced: false, worth: false, asset: false, puppet: false, recovery: false, loss: false, yields: false, rank: false, pays: false };
    /// The fixes of `war-fix` (classic/docs/ai-war-fix.md): the `siege` five with the learned
    /// chance in place of the siege model's, the capture priced by the rules, and a siege worth
    /// its city.
    pub const WAR_FIX: SiegeFixes = SiegeFixes { assault: true, garrison: true, close: true, chance: false, recover: true, learned: true, priced: true, worth: true, asset: true, puppet: false, recovery: false, loss: false, yields: false, rank: false, pays: false };
    /// `war-open`'s (classic/docs/ai-war-open.md): `WAR_FIX` with the capture priced as the puppet
    /// the AI makes of it, its unrest for the rounds the empire takes to recover, its citizens
    /// yielding what the empire's do, and the army's loss as measured.
    pub const OPEN: SiegeFixes = SiegeFixes { puppet: true, recovery: true, loss: true, yields: true, rank: true, ..SiegeFixes::WAR_FIX };
    /// `war-pays`' (classic/docs/ai-war-pays.md): `OPEN` with the loss measured on its own operations;
    /// `rival_percent` gives it a flat `RIVAL_PERCENT` (`rank` bought twice the captures in the head to head and no
    /// share: tuning offsets 200–209, paired +0.00, CI −1.35…+1.35).
    pub const PAYS: SiegeFixes = SiegeFixes { pays: true, ..SiegeFixes::OPEN };
}

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

/// Known technology, not obsolete, the nation's own version, no
/// required building.
fn trainable(brief: &TurnBrief, rules: &CompiledRuleset, kind: UnitKindId) -> bool {
    let def = &rules.units[kind.0 as usize];
    let gate = rules.production_gate(ProductionItem::Unit(kind), brief.nation).is_none();
    let replaced = rules.units.iter().any(|u| u.nation == Some(brief.nation) && u.replaces == Some(kind));
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
    composition_by(rules, arsenal, city, threat, false)
}

/// `composition` whose escorts are ranged units when `ranged_escorts` and the arsenal has
/// a ranged kind (`TurnBrief::army_fit`: the field fights of the training grounds are won
/// by armies about half ranged, classic/docs/ai-army.md).
pub fn composition_by(rules: &CompiledRuleset, arsenal: &Arsenal, city: &CityEstimate, threat: u32, ranged_escorts: bool) -> Option<Composition> {
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
    if ranged_escorts && arsenal.ranged.is_some() {
        army.ranged += escorts;
    } else {
        army.melee += escorts;
    }
    (army.total() <= MAX_ARMY).then_some(army)
}

/// One more unit for `army` as `composition` adds them: a distance unit while they are fewer
/// than the melee units, else a melee unit.
fn grow(army: &mut Composition, arsenal: &Arsenal) {
    let distance_role = if arsenal.siege.is_some() { ArmyRole::Siege } else { ArmyRole::Ranged };
    if arsenal.kind(distance_role).is_some() && army.ranged + army.siege < army.melee {
        army.add(distance_role);
    } else {
        army.add(ArmyRole::Melee);
    }
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
    crate::ai::war_probe::note(|p| {
        for operation in state.operations().iter().filter(|o| o.phase == OperationPhase::Marching) {
            let members = members(brief, state, operation.target);
            let nearest = members.iter().filter_map(|u| brief.dims.distance(u.tile, operation.target)).min().unwrap_or(-1);
            let sighted = routes.view().is_visible(operation.target);
            p.marches.push(crate::ai::war_probe::MarchSample { round: brief.round, player: brief.player, enemy: operation.enemy, target: operation.target.0, launched: operation.phase_round, members: members.len() as u32, nearest, sighted });
        }
        // What the operations' armies lose: their members, and the player's army a turn later.
        for operation in state.operations() {
            let members = members(brief, state, operation.target).iter().map(|u| (u.id.0, rules.units[u.kind.0 as usize].cost)).collect();
            p.members.push((brief.round, brief.player, operation.target.0, operation.phase == OperationPhase::Marching, members));
        }
        let army = brief.units.iter().filter(|u| army_role(rules, u.kind).is_some()).map(|u| u.id.0).collect();
        p.alive.push((brief.round, brief.player, army));
    });
    for operation in state.operations().to_vec() {
        out.extend(advance(brief, state, rules, &operation, gate.decisive(operation.target)));
    }
    if state.operations().is_empty() {
        out.extend(start(brief, state, rules, routes, gate));
    }
    for operation in state.operations().to_vec() {
        recruit(brief, state, rules, &operation, gate);
    }
    out.extend(crate::ai::raid::plan(brief, state, rules, routes, gate));
    out
}

/// Peace in every war old enough that no operation fights (the war was for
/// an operation that ended, or the enemy's cities are out of reach) and
/// that `decided` does not end already.
pub fn idle_wars(brief: &TurnBrief, state: &AiState, decided: &[Decision]) -> Vec<Decision> {
    let raided = crate::ai::raid::enemies(state);
    let fought = |enemy: PlayerId| state.operations().iter().any(|o| o.enemy == enemy) || raided.contains(&enemy);
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

    /// The player's view of the map the routes are searched over.
    pub fn view(&self) -> &PlayerView<'a> {
        &self.view
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
    let form_limit = brief.speed.pace_rounds(if decisive { DECISIVE_FORM_LIMIT } else { FORM_LIMIT });
    let over = match operation.phase {
        _ if !target_held(state, operation) => true,
        OperationPhase::Forming => age > form_limit,
        OperationPhase::Marching => !at_war || age > march_end(brief, operation) || outmatched(brief, rules, operation, &members),
    };
    if over {
        crate::ai::war_probe::note(|p| {
            use crate::ai::war_probe::EndCause;
            let marching = operation.phase == OperationPhase::Marching;
            let cause = if !target_held(state, operation) {
                if brief.cities.iter().any(|c| c.tile == operation.target) { EndCause::Captured } else { EndCause::TargetGone }
            } else if !marching {
                EndCause::FormTimeout
            } else if !at_war {
                EndCause::Peace
            } else if age > march_end(brief, operation) {
                EndCause::MarchTimeout
            } else {
                EndCause::Outmatched
            };
            p.ends.push(crate::ai::war_probe::OperationEnd { round: brief.round, player: brief.player, enemy: operation.enemy, cause, marching, age, target: operation.target.0 });
        });
        state.end_operation(operation.target);
        return vec![Decision::refuse(Actor::Player, Reason::EndOperation)];
    }
    let ready = assembled(brief, rules, operation, &members, age) && !outmatched(brief, rules, operation, &members);
    if operation.phase == OperationPhase::Forming {
        crate::ai::war_probe::note(|p| {
            p.forming.push(crate::ai::war_probe::FormingSample {
                round: brief.round,
                player: brief.player,
                target: operation.target.0,
                assembled: assembled(brief, rules, operation, &members, age),
                outmatched: outmatched(brief, rules, operation, &members),
                may_declare: at_war || may_declare(brief, state, operation),
                members: members.len() as u32,
                near: members.iter().filter(|u| brief.dims.distance(u.tile, operation.muster).is_some_and(|d| d <= MUSTER_RADIUS + 1)).count() as u32,
                near_healthy: members.iter().filter(|u| u.hp >= HEAL_BELOW_HP && brief.dims.distance(u.tile, operation.muster).is_some_and(|d| d <= MUSTER_RADIUS + 1)).count() as u32,
                wounded: members.iter().filter(|u| u.hp < HEAL_BELOW_HP).count() as u32,
                distance_sum: members.iter().filter_map(|u| brief.dims.distance(u.tile, operation.muster)).sum::<i32>().max(0) as u32,
                need: operation.need.total(),
                muster_to_target: brief.dims.distance(operation.muster, operation.target).unwrap_or(-1),
            })
        });
    }
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
        .filter(|u| u.hp >= HEAL_BELOW_HP && dims.distance(u.tile, operation.muster).is_some_and(|d| d <= muster_reach(brief, rules, u.kind)));
    let present = composition_of(rules, present.map(|u| u.kind));
    if shortfall(operation.need, present).total() == 0 {
        return true;
    }
    age >= brief.speed.pace_rounds(FORM_PATIENCE) && present.melee > 0 && present.total() * 4 >= operation.need.total() * 3
}

/// How far from the muster point a member stands with the army: `MUSTER_RADIUS + 1`; for
/// `war-muster` and `tactician-fit` (`TurnBrief::muster_fix`) one turn's march more, the unit's movement in tiles.
/// The muster point lies on the staging ring at the enemy's border: at peace the tiles within
/// `MUSTER_RADIUS` of it that are neither the enemy's land nor in the city's strike hold four or
/// five units (one military unit a tile), so a twelve-unit army stood on average 4.8 tiles from it
/// and four of its members within reach: 27 of 114 operations of `war-pays` (tuning offset 200)
/// formed for their whole limit with 12.9 members against a need of 11.9 and never marched
/// (classic/docs/ai-tactician-fit.md).
fn muster_reach(brief: &TurnBrief, rules: &CompiledRuleset, kind: UnitKindId) -> i32 {
    let base = MUSTER_RADIUS + 1;
    if !brief.muster_fix {
        return base;
    }
    base + i32::from(rules.units.get(kind.0 as usize).map_or(1, |d| d.movement.max(1)))
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
        crate::ai::war_probe::note(|p| p.declarations.push((brief.round, brief.player, operation.enemy, false)));
    }
    crate::ai::war_probe::note(|p| p.launched.push((brief.round, brief.player, operation.enemy)));
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
    brief.round >= brief.speed.game_round(EARLIEST_WAR_ROUND)
        && !brief.relations.iter().any(|r| r.at_war)
        && rounds_since(brief, last) >= brief.speed.pace_rounds(PEACE_COOLDOWN_ROUNDS)
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
    /// The game speed's pace percent: the forming limits are Standard rounds.
    pace: u32,
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
        let limit = if self.decisive { DECISIVE_FORM_LIMIT } else { FORM_LIMIT };
        if self.pace == 100 {
            return limit;
        }
        kernel::rules::speeds::SpeedValues::scale(limit, self.pace).max(1)
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
        && brief.round + brief.speed.pace_rounds(PREPARE_ROUNDS) >= brief.speed.game_round(EARLIEST_WAR_ROUND)
        && strategy_wages_war(brief, state.strategy.current)
        && posture_permits(brief);
    if !ready {
        return None;
    }
    let arsenal = Arsenal::of(brief, rules);
    let walker = arsenal.melee?;
    let free = counted_free(brief, state, rules);
    let mut candidates: Vec<Candidate> = targets(brief, state)
        .into_iter()
        .filter_map(|(city, horizon)| candidate(brief, state, rules, &arsenal, free, &city, horizon, gate))
        .filter(Candidate::fits)
        .collect();
    candidates.sort_by_key(Candidate::key);
    let considered = candidates.len();
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
    crate::ai::war_probe::note(|p| {
        let classes = held_classes(brief, state, rules);
        p.checks.push(crate::ai::war_probe::StartCheck {
            round: brief.round,
            player: brief.player,
            free: free.total().min(255) as u8,
            army: classes.0.min(255) as u8,
            in_operation: classes.1.min(255) as u8,
            garrison: classes.2.min(255) as u8,
            guard: classes.3.min(255) as u8,
            hurt: classes.4.min(255) as u8,
            other: classes.5.min(255) as u8,
            cities: brief.cities.len().min(255) as u8,
            candidates: considered.min(255) as u8,
            started: chosen.is_some(),
        })
    });
    let chosen = chosen?;
    crate::ai::war_probe::note(|p| {
        let city = state.seen_cities().iter().find(|c| c.tile == chosen.operation.target).copied();
        let (features, chance) = city.map_or(([0; CHANCE_N], None), |city| {
            let estimate = estimate_city(brief, rules, &city, arsenal.melee);
            let features = chance_features(brief, state, rules, &arsenal, &city, &estimate, chosen.operation.need, None);
            (features, model_chance(brief, state, rules, &arsenal, &city, &estimate, chosen.operation.need, None))
        });
        p.starts.push(crate::ai::war_probe::StartNote { round: brief.round, player: brief.player, enemy: chosen.operation.enemy, target: chosen.operation.target.0, value: chosen.value, rounds: chosen.assemble + chosen.march + SIEGE_ROUNDS_ESTIMATE, missing: shortfall(chosen.operation.need, free).total(), need: chosen.operation.need.total(), free: free.total(), features, chance })
    });
    state.start_operation(chosen.operation);
    Some(Decision::refuse(Actor::Player, Reason::FormOperation))
}

/// The army a new operation counts on: the units free for a strike. With `SiegeFixes::assault`
/// the units on an assault are not free, but an operation may still take them
/// (`assault_recruits`): its army counts them as the game's AI does. `start` and the strategist's
/// look at the war machine (`outlook`) count the same units, so their verdicts agree.
fn counted_free(brief: &TurnBrief, state: &AiState, rules: &CompiledRuleset) -> Composition {
    let on_assault = |u: &&UnitBrief| brief.war.siege.assault && u.hp >= HEAL_BELOW_HP && army_role(rules, u.kind).is_some() && matches!(state.task_of(u.id).map(|t| t.task), Some(UnitTask::Assault { .. }));
    let arsenal = Arsenal::of(brief, rules);
    composition_of(rules, strike_units(brief, state, rules).chain(brief.units.iter().filter(on_assault)).filter(|u| current(brief, rules, &arsenal, u.kind)).map(|u| u.kind))
}

/// The share of the best trainable kind's strength a unit needs to fill a place of the army, percent:
/// two thirds. The rules' generations of one role step by a third to a half of the strength (melee:
/// 8, 11, 14, 16, 21), so a unit under two thirds of the best kind is a generation or more behind and
/// deals the siege model a fraction of the damage the army was planned with (`composition` plans
/// with the best kind). On offset 200 the operations that started with most places filled by
/// existing units took a city in 15–30 % of the cases, those that built their army in 39–43 %
/// (`war-pays`, classic/docs/ai-war-muster.md). Only `war-muster` (`TurnBrief::war_muster`).
pub const CURRENT_PERCENT: i64 = 67;

/// A unit of `kind` fills a place of an army: the army counts every unit with the strength the plan
/// was made with, not any unit of the role (`CURRENT_PERCENT`). Without `TurnBrief::war_muster`
/// every unit does.
fn current(brief: &TurnBrief, rules: &CompiledRuleset, arsenal: &Arsenal, kind: UnitKindId) -> bool {
    if !brief.war_muster {
        return true;
    }
    let Some(role) = army_role(rules, kind) else { return true };
    let Some(best) = arsenal.kind(role) else { return true };
    i64::from(role_strength(rules, kind, role)) * 100 >= i64::from(role_strength(rules, best, role)) * CURRENT_PERCENT
}

/// A new operation or raid may start under `strategy`: not while defending; not while
/// recovering, but with `SiegeFixes::recover` (the war case decides then, and a lost city is
/// one of the targets).
pub(crate) fn strategy_wages_war(brief: &TurnBrief, strategy: Strategy) -> bool {
    match strategy {
        Strategy::Expand | Strategy::Stabilize | Strategy::Attack => true,
        Strategy::Recover => brief.war.siege.recover,
        Strategy::Defend => false,
    }
}

/// `war-open` (`TurnBrief::war_open`): a new offensive operation starts only when the strategy
/// net's posture is to prepare a war or to wage one; at peace and in defence the war machine
/// starts none (one brain: the net opens the window, the operation picks the target in it).
/// Every other variant: always.
pub(crate) fn posture_permits(brief: &TurnBrief) -> bool {
    !brief.war_open || brief.strategy.as_ref().is_some_and(|plan| plan.intent.posture_pick >= 2)
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
/// The same, measured on `war-open`'s own operations (`SiegeFixes::loss`): the units its members
/// lost from the operation's start to its end over the units it planned — 66 % in 638 operations
/// of `war-open` (pace, Quick, seed offset 201; 124 % of those that marched, whose every unit on
/// an assault of the target joins them, 15 % of those recalled or timed out before; `ai-war-ops`,
/// classic/docs/ai-war-open.md). The operations of `war-fix-model`, mostly recalled before they
/// march, lost 25 %; `LOSS_PERCENT` guessed half. TUNED: measured on build E2 of the lane, not on the frozen
/// one, and it bypasses the genome's `LossPercent` knob.
pub const OPEN_LOSS_PERCENT: i64 = 66;
/// The same measured on `war-pays`'s own operations: 67.3 % of the need in 292 operations (pace, Quick,
/// seeds 203001–204021 with `ai-war-ops`; 101 % of those that marched). A measured constant, not
/// derived; re-measure it when the operations change.
pub const PAYS_LOSS_PERCENT: i64 = 67;
/// Share of the city's points taken from the rival that counts for us,
/// percent: the time victory compares our score with the best other one.
pub const RIVAL_PERCENT: i64 = 50;

/// A war on an aggressor the world remembers is worth more (`ai::infamy`).
pub(crate) fn infamy_bonus(brief: &TurnBrief, owner: PlayerId) -> Centi {
    brief.relation(owner).map_or(0, |r| crate::ai::infamy::war_bonus(r.warmonger))
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
/// With `chance` (`SiegeFixes::chance`, `siege_chance`) the chance is the model's, not the
/// constant's or the weapon's.
#[allow(clippy::too_many_arguments)]
fn war_case(prices: &Prices, stakes: &Stakes, rules: &CompiledRuleset, arsenal: &Arsenal, city: &SeenCity, need: Composition, missing: Composition, rounds: u32, armed: Option<&crate::ai::strikes::Armed>, chance: Option<i64>, fixes: SiegeFixes, rival: i64) -> WarCase {
    war_case_parts(prices, stakes, rules, arsenal, city, need, missing, rounds, armed, chance, fixes, rival).0
}

/// The terms of a war's case (the war probe's diagnosis, `war_probe::Estimate`): the chance it
/// counted (percent), the capture's worth before the chance, the hammers and upkeep of the units
/// it builds, the units it expects to lose and the weapons it plans.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CaseParts {
    pub success: i64,
    pub capture: Centi,
    pub built: Centi,
    pub lost: Centi,
    pub weapons: Centi,
}

/// `war_case` and its terms.
#[allow(clippy::too_many_arguments)]
/// `rival`: the share of the rival's loss that counts, percent (`rival_percent`).
fn war_case_parts(prices: &Prices, stakes: &Stakes, rules: &CompiledRuleset, arsenal: &Arsenal, city: &SeenCity, need: Composition, missing: Composition, rounds: u32, armed: Option<&crate::ai::strikes::Armed>, chance: Option<i64>, fixes: SiegeFixes, rival: i64) -> (WarCase, CaseParts) {
    let priced = fixes.priced;
    let stake = stakes.capital(city.tile);
    let decisive = stake.is_some_and(|s| s.decisive);
    let gain = match stake {
        Some(stake) if decisive => stake.value,
        _ if priced => captured_gain_by(prices, stakes, city.population, rounds, fixes, rival) + stake.map_or(0, |s| s.value),
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
    let built = if fixes.asset { 0 } else { army(missing) * hammer } + i64::from(missing.total()) * upkeep;
    let loss = if fixes.loss { if fixes.pays { PAYS_LOSS_PERCENT } else { OPEN_LOSS_PERCENT } } else { crate::ai::genome::get(crate::ai::genome::Knob::LossPercent) };
    let lost = army(need) * loss / 100 * hammer;
    let base = crate::ai::genome::get(crate::ai::genome::Knob::SuccessPercent);
    // The weapon the plan counts on (`strikes::armory`) gives the chance it gives and costs its
    // hammers and, for a nuclear one, the world's memory of it.
    let (success, weapons) = armed.map_or((base, 0), |a| (a.success, i64::from(a.extra_hammers) * hammer + a.public_cost));
    let success = chance.unwrap_or(success);
    (WarCase { gain: success * gain / 100, cost: built + lost + weapons }, CaseParts { success, capture: gain, built, lost, weapons })
}

/// The chance, percent, that `army` takes `city` (`SiegeFixes::chance`): none when the
/// siege model says its damage a round does not beat the city's healing or it has no melee
/// unit to take the city (`front::wears_down`); else the defence's own chance of a city's
/// fall (`defence::chance`) for the army's power against the city's own (strength ×
/// health) and `guard`, the power of the enemy units seen defending it.
pub(crate) fn siege_chance(rules: &CompiledRuleset, arsenal: &Arsenal, city: &CityEstimate, army: Composition, guard: u32) -> i64 {
    let Some(melee) = arsenal.melee else { return 0 };
    let melee_strike = strike(rules, melee, ArmyRole::Melee, city);
    let distance_role = if arsenal.siege.is_some() { ArmyRole::Siege } else { ArmyRole::Ranged };
    let distance_strike = arsenal.kind(distance_role).map(|k| strike(rules, k, distance_role, city));
    if army.melee == 0 || !wears_down(&besiegers(army, melee_strike, distance_strike), city.heal) {
        return 0;
    }
    let full = crate::world::unit::UNIT_MAX_HP;
    let power = |role: ArmyRole| {
        let kind = arsenal.kind(role).or(arsenal.ranged).or(arsenal.melee);
        kind.map_or(0, |k| unit_power(rules, k, full)) * u32::from(army.of(role))
    };
    let ours = power(ArmyRole::Melee) + power(ArmyRole::Ranged) + power(ArmyRole::Siege);
    let theirs = (city.strength * i64::from(city.hp) / 10_000).max(0) as u32 + guard;
    crate::ai::defence::chance(ours, theirs) / 10
}

/// The chance of an operation of `army` against the remembered `city`: learned from outcomes
/// with `SiegeFixes::learned` (`learned_chance`), the siege model's with `SiegeFixes::chance`
/// (None without either: the constant's).
/// `since`: the round a running operation started (its features are the ones it started with:
/// a war it declared itself is not a war it started in).
#[allow(clippy::too_many_arguments)]
fn model_chance(brief: &TurnBrief, state: &AiState, rules: &CompiledRuleset, arsenal: &Arsenal, city: &SeenCity, estimate: &CityEstimate, army: Composition, since: Option<u32>) -> Option<i64> {
    if brief.war.siege.learned {
        let features = chance_features(brief, state, rules, arsenal, city, estimate, army, since);
        return Some(if brief.war_open { open_chance(&features) } else { learned_chance(&features) });
    }
    brief.war.siege.chance.then(|| siege_chance(rules, arsenal, estimate, army, defence(brief, city.owner, city.tile)))
}

/// The features of the learned capture chance (`CHANCE_FEATURES`).
pub const CHANCE_N: usize = 15;

/// What an operation's outcome is learned from, all known to the player when it plans
/// (classic/docs/ai-war-fix.md "The capture chance", classic/docs/ai-war-open.md): integers, in
/// this order; the last three are `war-open`'s (the game's AI weighs them 0).
pub const CHANCE_FEATURES: [&str; CHANCE_N] = [
    "power_share",
    "siege_model",
    "distance",
    "population",
    "guard_share",
    "at_war",
    "round_tens",
    "enemy_cities",
    "pacts",
    "enemy_threat",
    "army",
    "own_cities",
    "strength",
    "era_edge",
    "ready",
];

/// The features of an operation of `army` against `city` (`CHANCE_FEATURES`): the army's
/// share of the power at the target (its units at full health against the city's strength ×
/// health and the defenders seen within `DEFENCE_RADIUS`), the siege model's chance
/// (`siege_chance`), the distance from our nearest city, the city's population, the
/// defenders' power over the army's (percent, at most 400), at war already, the round in
/// tens, the enemy's cities we know, its defensive pacts we know, its threat near our cities
/// over our whole army (percent, at most 400), the army's units and our cities; our military
/// power's share of ours and the enemy's known power (`known_power`, percent), our era less the
/// enemy's (−3…3), and the share of the army that stands ready (its members and the free units
/// that fill its roles, percent).
#[allow(clippy::too_many_arguments)]
pub fn chance_features(brief: &TurnBrief, state: &AiState, rules: &CompiledRuleset, arsenal: &Arsenal, city: &SeenCity, estimate: &CityEstimate, army: Composition, since: Option<u32>) -> [i64; CHANCE_N] {
    let guard = defence(brief, city.owner, city.tile);
    let full = crate::world::unit::UNIT_MAX_HP;
    let power = |role: ArmyRole| {
        let kind = arsenal.kind(role).or(arsenal.ranged).or(arsenal.melee);
        i64::from(kind.map_or(0, |k| unit_power(rules, k, full))) * i64::from(army.of(role))
    };
    let ours = power(ArmyRole::Melee) + power(ArmyRole::Ranged) + power(ArmyRole::Siege);
    let theirs = (estimate.strength * i64::from(estimate.hp) / 10_000).max(0) + i64::from(guard);
    let distance = brief.nearest_city(brief.dims, city.tile).and_then(|c| brief.dims.distance(c.tile, city.tile)).unwrap_or(0).max(0);
    let relation = brief.relation(city.owner);
    let threat = state.relation(city.owner).map_or(0, |m| m.threat);
    let enemy_cities = state.seen_cities().iter().filter(|c| c.owner == city.owner).count() as i64;
    let mine = i64::from(brief.military_power);
    let strength = mine * 100 / (mine + i64::from(known_power(brief, state, rules, city.owner, &brief.met_eras))).max(1);
    let own_era = crate::ai::strategy_net::features::era_of(rules, &brief.techs) as i64;
    let their_era = brief.met_eras.iter().find(|(p, _)| *p == city.owner).map_or(own_era, |(_, e)| i64::from(*e));
    let mut have = composition_of(rules, members(brief, state, city.tile).iter().map(|u| u.kind));
    let free = composition_of(rules, free_units(brief, state, rules).map(|u| u.kind));
    (have.melee, have.ranged, have.siege) = (have.melee.saturating_add(free.melee), have.ranged.saturating_add(free.ranged), have.siege.saturating_add(free.siege));
    let ready = i64::from(army.total() - shortfall(army, have).total()) * 100 / i64::from(army.total().max(1));
    [
        ours * 100 / (ours + theirs).max(1),
        siege_chance(rules, arsenal, estimate, army, guard),
        i64::from(distance),
        i64::from(city.population),
        (i64::from(guard) * 100 / ours.max(1)).min(400),
        i64::from(relation.is_some_and(|r| r.at_war && since.is_none_or(|s| r.war_since < s))),
        i64::from(brief.round / 10),
        enemy_cities,
        relation.map_or(0, |r| i64::from(r.pacts_known)),
        (i64::from(threat) * 100 / i64::from(brief.military_power.max(1))).min(400),
        i64::from(army.total()),
        brief.cities.len() as i64,
        strength,
        (own_era - their_era).clamp(-3, 3),
        ready,
    ]
}

/// The logit of the learned capture chance, thousandths: the intercept, then a weight per
/// feature of `CHANCE_FEATURES` (`CHANCE_WEIGHTS`).
fn chance_logit(features: &[i64; CHANCE_N]) -> i64 {
    CHANCE_INTERCEPT + features.iter().zip(CHANCE_WEIGHTS).map(|(x, w)| x * w).sum::<i64>() / 1000
}

/// The learned capture chance's intercept and weights (logit in thousandths; a weight in
/// millionths of the logit per unit of its feature), fitted by a logistic regression on the
/// operations of bench games whose seeds the reported benches do not use
/// (`ai-war-ops`, `tools/war_chance_fit.py`, classic/docs/ai-war-fix.md).
/// The fit of 1 November, the second turn of the loop (`ai-war-ops --fit ops-model-2.csv
/// ops-warfix7.csv --use power_share,guard_share,at_war,enemy_threat,distance,population,army,
/// own_cities --ridge 5`): 400 games of `war-fix-model` (seeds 100001–100040) and 600 of
/// `war-fix` with the first fit (seeds 100041–100100), the standard suite's maps; 2 448
/// operations with an outcome, of which 11.7 % (train) and 13.2 % (test) saw their enemy lose a
/// city to the player in their war; test log loss 0.377 against 0.391 for a constant and 2.08
/// for the siege model. The first fit, on `war-fix-model` alone, gave the large armies `war-fix`
/// then planned 53 % where they took 37 %; the second tempers the army's weight. The siege
/// model's chance, the round, the enemy's cities and pacts add nothing on the held-out seeds
/// (classic/docs/ai-war-fix.md).
pub const CHANCE_INTERCEPT: i64 = -1657;
pub const CHANCE_WEIGHTS: [i64; CHANCE_N] = [-6986, 0, -61418, -53107, 1909, -990031, 0, 0, 0, -36668, 204117, 18862, 0, 0, 0];

/// The learned chance, percent, that an operation with `features` takes its city
/// (`SiegeFixes::learned`): the logistic of `chance_logit`, integer only.
pub fn learned_chance(features: &[i64; CHANCE_N]) -> i64 {
    logistic_percent(chance_logit(features))
}

/// The same model refitted on the operations of `war-open` (`TurnBrief::war_open`): its army is
/// prepared by the posture and its wars open on an opportunity, so its operations are not the
/// ones `CHANCE_WEIGHTS` were fitted on; the fit may use the three features the game's AI weighs 0.
/// Fitted (`ai-war-ops --fit ops-wfm-200.csv ops-v1-200.csv ops-e2-201.csv --use power_share,
/// guard_share,at_war,enemy_threat,distance,population,army,own_cities,strength,era_edge,ready
/// --ridge 5`) on 2 055 operations with an outcome of the pace suite at Quick (100 games of
/// `war-fix-model` and 100 of the first `war-open` at seed offset 200, 100 of `war-open` with the
/// first fit at offset 201): test log loss 0.314 against 0.316 for a constant and 0.331 for
/// `CHANCE_WEIGHTS`, calibrated (predicted 9 % → 9 % taken, 27 % → 13 %, 44 % → 22 %). The
/// second turn of the loop: the first fit (1 417 operations) admitted `war-open`'s operations at
/// 27.7 % and 14.7 % took a city. Per unit, logit: an era ahead +0.28, the army +0.24 a unit,
/// distance −0.067 a tile, at war already −0.69; the strength and the ready army add nothing
/// (classic/docs/ai-war-open.md).
pub const OPEN_CHANCE_INTERCEPT: i64 = -1193;
pub const OPEN_CHANCE_WEIGHTS: [i64; CHANCE_N] = [-9830, 0, -66899, -65621, -8240, -689142, 0, 0, 0, -19137, 235035, -35079, -2122, 278011, -4646];

/// `learned_chance` by `war-open`'s fit (`OPEN_CHANCE_WEIGHTS`).
pub fn open_chance(features: &[i64; CHANCE_N]) -> i64 {
    logistic_percent(OPEN_CHANCE_INTERCEPT + features.iter().zip(OPEN_CHANCE_WEIGHTS).map(|(x, w)| x * w).sum::<i64>() / 1000)
}

/// The logistic of a logit in thousandths, percent: a table at quarters, linearly between.
pub fn logistic_percent(logit: i64) -> i64 {
    // 1000 / (1 + e^(-z)) for z = -8, -7.75, …, 8.
    const TABLE: [i64; 65] = [
        0, 0, 1, 1, 1, 1, 2, 2, 2, 3, 4, 5, 7, 9, 11, 14, 18, 23, 29, 37, 47, 60, 76, 95, 119, 148, 182, 223, 269, 321, 378, 438, 500, 562, 622, 679, 731,
        777, 818, 852, 881, 905, 924, 940, 953, 963, 971, 977, 982, 986, 989, 991, 993, 995, 996, 997, 998, 998, 998, 999, 999, 999, 999, 1000, 1000,
    ];
    let at = (logit + 8000).clamp(0, 16_000);
    let (index, rest) = ((at / 250) as usize, at % 250);
    let low = TABLE[index];
    let high = TABLE[(index + 1).min(64)];
    (low + (high - low) * rest / 250) / 10
}

/// What a running operation is still worth (`war_value`): its target with
/// the army it has — only the units it lacks are still to be built — over
/// the rounds its phase may still take and the siege. None: its target is
/// no longer known.
pub(crate) fn running_value(brief: &TurnBrief, state: &AiState, rules: &CompiledRuleset, prices: &Prices, stakes: &Stakes, operation: &Operation) -> Option<i64> {
    running_detail(brief, state, rules, prices, stakes, operation, brief.war.recall).map(|(case, _, _)| case.value(stakes) + infamy_bonus(brief, operation.enemy))
}

/// `running_value` in its parts: the case, and the units still missing and needed.
pub(crate) fn running_detail(brief: &TurnBrief, state: &AiState, rules: &CompiledRuleset, prices: &Prices, stakes: &Stakes, operation: &Operation, expected: bool) -> Option<(WarCase, u32, u32)> {
    let city = state.seen_cities().iter().find(|c| c.tile == operation.target)?;
    let arsenal = Arsenal::of(brief, rules);
    let have = composition_of(rules, members(brief, state, operation.target).iter().map(|u| u.kind));
    let missing = shortfall(operation.need, have);
    let limit = match operation.phase {
        OperationPhase::Forming => brief.speed.pace_rounds(FORM_LIMIT),
        OperationPhase::Marching => march_limit(brief),
    };
    let age = brief.round.saturating_sub(operation.phase_round);
    let mut rounds = limit.saturating_sub(age) + SIEGE_ROUNDS_ESTIMATE;
    if expected {
        // The rounds the operation is still to take, as the start counts them (`candidate`): the
        // units still to build and gather, the walk to the target and the siege, not the whole
        // time its phase may last.
        let dims = brief.dims;
        let nearest = members(brief, state, operation.target).iter().filter_map(|u| dims.distance(u.tile, operation.target)).min();
        let strike = nearest.unwrap_or_else(|| dims.distance(operation.muster, operation.target).unwrap_or(0)).max(0) as u32;
        let march = (strike * 2).div_ceil(3) + 1;
        let assemble = match operation.phase {
            OperationPhase::Forming => assemble_rounds(brief, rules, &arsenal, missing).saturating_sub(age),
            OperationPhase::Marching => 0,
        };
        rounds = rounds.min(assemble + march + SIEGE_ROUNDS_ESTIMATE);
    }
    let chance = model_chance(brief, state, rules, &arsenal, city, &estimate_target(brief, rules, city), operation.need, Some(operation.since_round));
    let case = war_case(prices, stakes, rules, &arsenal, city, operation.need, missing, rounds, crate::ai::strikes::on_hand_best(&brief.armory), chance, brief.war.siege, rival_percent(brief, city.owner));
    Some((case, missing.total(), operation.need.total()))
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
    let arsenal = Arsenal::of(brief, rules);
    let chance = model_chance(brief, state, rules, &arsenal, city, &estimate_target(brief, rules, city), operation.need, Some(operation.since_round));
    let mut case = war_case(prices, stakes, rules, &arsenal, city, operation.need, missing, rounds, crate::ai::strikes::on_hand_best(&brief.armory), chance, brief.war.siege, rival_percent(brief, city.owner));
    case.gain += infamy_bonus(brief, enemy);
    Some(case)
}

/// The score a captured city of `population` brings by the rules (`SiegeFixes::priced`):
/// the rules' capture leaves it fewer citizens (`captured_population`); the AI keeps a city of
/// `ANNEX_POPULATION` and more annexed, the rest as puppets (`combat::conquest_decisions`); an
/// annexed city yields nothing while its resistance lasts (as many rounds as its citizens), a
/// puppet has none; its unrest is the rules' for a puppet or an annexed city
/// (`Prices::captured_city_unrest_hundredths`), priced by what it changes for the empire: the
/// drift of every point and each mood the empire crosses (`Prices::captured_city_unrest_steps`),
/// not every point at the price of a point in the deficit the empire lives in. With `puppet`
/// (`SiegeFixes::puppet`) every captured city is priced as a puppet, as the game's AI keeps it
/// since the eighth promotion (`Conquest::PuppetAll`).
pub fn captured_gain(prices: &Prices, stakes: &Stakes, population: u32, rounds: u32, puppet: bool) -> Centi {
    captured_gain_by(prices, stakes, population, rounds, SiegeFixes { puppet, ..SiegeFixes::NONE }, RIVAL_PERCENT)
}

/// `captured_gain` with the fixes' pricing of the capture (`SiegeFixes::puppet`, `recovery`).
pub fn captured_gain_by(prices: &Prices, stakes: &Stakes, population: u32, rounds: u32, fixes: SiegeFixes, rival: i64) -> Centi {
    let (worth, unrest) = captured_parts_by(prices, population, rounds, fixes, rival);
    worth - unrest * stakes.unrest / 100
}

/// `captured_gain` in its parts: the worth from the rounds the city yields on, and the unrest.
pub fn captured_parts(prices: &Prices, population: u32, rounds: u32, puppet: bool) -> (Centi, Centi) {
    captured_parts_by(prices, population, rounds, SiegeFixes { puppet, ..SiegeFixes::NONE }, RIVAL_PERCENT)
}

/// `captured_parts` with the fixes' pricing (`SiegeFixes::puppet`, `recovery`, `yields`, `rank`
/// with `rival`, the share of the rival's loss that counts).
pub fn captured_parts_by(prices: &Prices, population: u32, rounds: u32, fixes: SiegeFixes, rival: i64) -> (Centi, Centi) {
    let kept = crate::systems::combat::city::captured_population(population.max(1), 0);
    let annexed = !fixes.puppet && kept >= crate::ai::combat::ANNEX_POPULATION;
    let resistance = if annexed { kept } else { 0 };
    let unrest = if fixes.recovery { prices.kept_city_unrest_recovering(kept, annexed) } else { prices.kept_city_unrest_steps(kept, annexed) };
    let worth = match (fixes.yields, fixes.rank) {
        (true, true) => city_worth_ranked(prices, kept, rounds + resistance, rival),
        (true, false) => city_worth_yielding(prices, kept, rounds + resistance),
        _ => city_worth(prices, kept, rounds + resistance),
    };
    (worth, unrest)
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

// ---- What the strategist reads of the war machine (`war-open`) ----

/// The war machine as the strategy net of `war-open` reads it (`strategy_net::features::open`):
/// the best known target and the strength of its owner's army against ours.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Outlook {
    /// The best known target (by chance × relative worth): its learned capture chance (percent),
    /// the capture's worth over an own city's of the mean size (thousandths), its tile and owner.
    pub chance: i64,
    pub relative: i64,
    pub target: Option<TileId>,
    pub enemy: Option<PlayerId>,
    /// Our military power and the known power of the target's owner (else the nearest rival's).
    pub ours: u32,
    pub theirs: u32,
    /// An offensive operation forms or marches, or a war plan saves its chest.
    pub forming: bool,
    /// A target whose war case pays now: `candidate` admits it (value ≥ 0, in time) with the
    /// leader's `stakes` and the units free for a march, before the route is checked.
    pub pays: bool,
}

/// The war machine's outlook of a turn (`Outlook`): every known city of a nation within
/// `OBJECTIVE_RANGE` of our nearest city, valued as `candidate` values it before the route is
/// known — the army that takes it by the siege model (`composition_by`), its chance by the model
/// learned from outcomes (`learned_chance`), the capture priced by the rules as the puppet the
/// AI makes of it (`captured_gain`) over the worth of an own city of the empire's mean size.
/// `met_eras`: the era of every nation met (`PlayerQueries::met_eras`); `stakes`: the leader's.
pub fn outlook(brief: &TurnBrief, state: &AiState, rules: &CompiledRuleset, prices: &Prices, stakes: &Stakes, met_eras: &[(PlayerId, u8)]) -> Outlook {
    let mut out = Outlook { ours: brief.military_power, forming: !state.operations().is_empty() || state.war_plan.is_some(), ..Outlook::default() };
    let arsenal = Arsenal::of(brief, rules);
    let major = |p: PlayerId| brief.relation(p).is_some_and(|r| r.kind == crate::world::player::PlayerKind::Major);
    let citizens: u32 = brief.cities.iter().map(|c| c.population).sum();
    let mean = citizens / (brief.cities.len() as u32).max(1);
    let own = if brief.war.siege.yields { city_worth_yielding(prices, mean.max(1), 0) } else { city_worth(prices, mean.max(1), 0) }.max(1);
    let neutral = Stakes::neutral();
    let mut best: Option<(i64, i64, i64, SeenCity)> = None;
    let mut nearest: Option<(i32, PlayerId)> = None;
    for city in state.seen_cities().iter().filter(|c| major(c.owner)) {
        let Some(distance) = brief.nearest_city(brief.dims, city.tile).and_then(|c| brief.dims.distance(c.tile, city.tile)) else { continue };
        if nearest.is_none_or(|(d, p)| (distance, city.owner.0) < (d, p.0)) {
            nearest = Some((distance, city.owner));
        }
        if distance > OBJECTIVE_RANGE || arsenal.melee.is_none() {
            continue;
        }
        let estimate = estimate_city(brief, rules, city, arsenal.melee);
        let threat = state.relation(city.owner).map_or(0, |m| m.threat).max(defence(brief, city.owner, city.tile));
        let Some(need) = composition_by(rules, &arsenal, &estimate, threat, brief.army_fit) else { continue };
        let chance = open_chance(&chance_features(brief, state, rules, &arsenal, city, &estimate, need, None));
        let rounds = (distance as u32 * 2).div_ceil(3) + 1 + SIEGE_ROUNDS_ESTIMATE;
        let relative = (captured_gain_by(prices, &neutral, city.population, rounds, brief.war.siege, rival_percent(brief, city.owner)) * 1000 / own).clamp(0, 2000);
        let key = chance * relative;
        if best.is_none_or(|(k, _, _, c)| (key, std::cmp::Reverse(city.tile)) > (k, std::cmp::Reverse(c.tile))) {
            best = Some((key, chance, relative, *city));
        }
    }
    if let Some((_, chance, relative, city)) = best {
        (out.chance, out.relative, out.target, out.enemy) = (chance, relative, Some(city.tile), Some(city.owner));
    }
    if arsenal.melee.is_some() && brief.cities.len() >= 2 {
        // `war-pays`: the units `start` counts (`free_units`), so that the two verdicts agree.
        let free = if brief.war_pays { counted_free(brief, state, rules) } else { composition_of(rules, strike_units(brief, state, rules).map(|u| u.kind)) };
        out.pays = crate::ai::war_probe::muted(|| targets(brief, state).into_iter().any(|(city, horizon)| candidate(brief, state, rules, &arsenal, free, &city, horizon, WarGate::Value(prices, stakes)).is_some_and(|c| c.fits())));
    }
    let rival = out.enemy.or(nearest.map(|(_, p)| p));
    if let Some(rival) = rival {
        out.theirs = known_power(brief, state, rules, rival, met_eras);
    }
    out
}

/// Army units a nation of these rules keeps a city, tenths: the AI's own army at a look for an
/// operation is 14.35 units for 5.5 cities (classic/docs/ai-war-audit-current.md, `pace`).
/// TUNED, not derived: the AI's own ratio applied to rivals, whose armies nothing tells.
pub const ARMY_PER_CITY_TENTHS: u64 = 26;

/// The military power of `rival` as the player can know it (`unit_power`): the largest of its
/// units in sight, its army remembered near our cities (`RelationMemory::threat`) and the army a
/// nation of its known cities keeps (`ARMY_PER_CITY_TENTHS`) of its era's best melee unit
/// (`era_strength`), since nothing tells the size of an army out of sight. `met_eras`: the era of
/// every nation met.
pub fn known_power(brief: &TurnBrief, state: &AiState, rules: &CompiledRuleset, rival: PlayerId, met_eras: &[(PlayerId, u8)]) -> u32 {
    let seen: u32 = brief.sightings.iter().filter(|s| s.owner == rival && s.military).map(|s| s.power).sum();
    let threat = state.relation(rival).map_or(0, |m| m.threat);
    let cities = state.seen_cities().iter().filter(|c| c.owner == rival).count() as u64;
    let own_era = crate::ai::strategy_net::features::era_of(rules, &brief.techs);
    let their_era = met_eras.iter().find(|(p, _)| *p == rival).map_or(own_era, |(_, e)| usize::from(*e));
    let kept = cities * ARMY_PER_CITY_TENTHS * era_strength(rules, their_era) / 10;
    seen.max(threat).max(kept.min(u64::from(u32::MAX)) as u32)
}

/// The strength of the best melee unit the rules give by an era (its index): what an army of that
/// era fights with.
pub fn era_strength(rules: &CompiledRuleset, era: usize) -> u64 {
    let era_of = |t: Option<crate::ids::TechId>| t.and_then(|t| rules.technologies.get(usize::from(t.0))).and_then(|d| d.content_era).map_or(0, |e| crate::ai::strategy_net::features::era_index(rules, e));
    (0..rules.units.len())
        .filter(|i| army_role(rules, UnitKindId(*i as u16)) == Some(ArmyRole::Melee))
        .filter(|i| rules.units[*i].nation.is_none() && !rules.units[*i].unbuildable && era_of(rules.units[*i].required_tech) <= era)
        .map(|i| u64::from(rules.units[i].strength))
        .max()
        .unwrap_or(1)
        .max(1)
}

/// `city_worth` whose citizens yield what the empire's own do (`SiegeFixes::yields`,
/// `Prices::citizen_yield`): a captured city held to the end made 5.0 hammers, 5.8 beakers, 2.9
/// gold and 2.1 culture a round at 5.1 citizens (classic/docs/ai-conquest-payoff.md), where
/// `city_worth` counts half a hammer and half a beaker a citizen.
pub fn city_worth_yielding(prices: &Prices, population: u32, rounds: u32) -> Centi {
    let score = prices.score;
    let population = i64::from(population.max(1));
    let points = score.city + score.land * (crate::ai::value::FIRST_RING + population) + score.citizen * population;
    let yields = prices.later(population * prices.citizen_yield, rounds);
    points + yields + points * crate::ai::genome::get(crate::ai::genome::Knob::RivalPercent) / 100
}

/// `city_worth_yielding` whose rival's loss counts `rival` percent of the city's points and yields
/// (`SiegeFixes::rank`, `rival_percent`): what the rival loses is the city's score and the yields
/// it no longer makes.
pub fn city_worth_ranked(prices: &Prices, population: u32, rounds: u32, rival: i64) -> Centi {
    let score = prices.score;
    let population = i64::from(population.max(1));
    let points = score.city + score.land * (crate::ai::value::FIRST_RING + population) + score.citizen * population;
    let yields = prices.later(population * prices.citizen_yield, rounds);
    (points + yields) * (100 + rival) / 100
}

/// The share of a rival's loss that counts for us, percent: with `SiegeFixes::rank` by where the
/// rival stands on the victory screen (`TurnBrief::scores`) — 100 when it leads the others (the
/// one the time victory compares us with), half when it is only ahead of us, nothing when it is
/// behind us; `RIVAL_PERCENT` without it or with no scores known.
pub fn rival_percent(brief: &TurnBrief, rival: PlayerId) -> i64 {
    let score = |p: PlayerId| brief.scores.iter().find(|(q, _)| *q == p).map(|(_, s)| *s);
    let (Some(ours), Some(theirs)) = (score(brief.player), score(rival)) else { return RIVAL_PERCENT };
    if !brief.war.siege.rank || brief.war_pays {
        return RIVAL_PERCENT;
    }
    let best = brief.scores.iter().filter(|(p, _)| *p != brief.player).map(|(_, s)| *s).max().unwrap_or(0);
    if theirs >= best {
        100
    } else if theirs > ours {
        RIVAL_PERCENT
    } else {
        0
    }
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
            (false, true) if rounds_since(brief, last) >= brief.speed.pace_rounds(PEACE_COOLDOWN_ROUNDS) => Horizon::Peace,
            _ => continue,
        };
        out.extend(state.seen_cities().iter().filter(|c| c.owner == relation.other).map(|c| (*c, horizon)));
    }
    out
}

/// `war-pays`: the odds an operation takes a city, read from the operations of `war-open` (pace,
/// Quick, 300 games of the tuning seeds 200–202, classic/docs/ai-war-pays.md): those started in a war
/// that was already running took a city of their enemy in 3 % of the cases (the first operation of the war
/// ended recalled, outmatched or by peace, and the enemy stands armed), those started at peace against
/// a city of at most `MAX_TARGET_POPULATION` citizens in about 29 %, against a larger one 9 %. Tuned
/// constants, not derived ones.
fn odds_are_good(city: &SeenCity, horizon: Horizon) -> bool {
    matches!(horizon, Horizon::Peace) && i64::from(city.population) <= MAX_TARGET_POPULATION
}

/// The largest target `war-pays` starts an operation against, in citizens: a city of 9 or more
/// held 8 % of the cases against 22–29 % for the smaller ones (it heals behind more walls and
/// keeps a larger garrison). Tuned on the war-open operations of offsets 200–201.
pub const MAX_TARGET_POPULATION: i64 = 8;

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
    if brief.war_pays && !odds_are_good(city, horizon) {
        return None;
    }
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
    let plan = |need: Composition, armed: Option<&crate::ai::strikes::Armed>, target: &CityEstimate| -> Option<Candidate> {
        let missing = shortfall(need, free);
        let assemble = assemble_rounds(brief, rules, arsenal, missing).max(armed.map_or(0, |a| a.ready));
        let value = match gate {
            WarGate::Orders if armed.is_some() || missing.total() > MAX_WAR_ORDERS || !affordable(brief, missing) => return None,
            WarGate::Orders => 0,
            WarGate::Value(prices, stakes) => {
                let rounds = assemble + march + SIEGE_ROUNDS_ESTIMATE;
                let chance = model_chance(brief, state, rules, arsenal, city, target, need, None);
                let (case, parts) = war_case_parts(prices, stakes, rules, arsenal, city, need, missing, rounds, armed, chance, brief.war.siege, rival_percent(brief, city.owner));
                let value = case.value(stakes) + infamy_bonus(brief, city.owner);
                crate::ai::war_probe::note(|p| {
                    p.estimates.push(crate::ai::war_probe::Estimate {
                        target: city.tile.0,
                        chance: parts.success,
                        capture: parts.capture,
                        built: parts.built,
                        lost: parts.lost,
                        weapons: parts.weapons,
                        infamy: infamy_bonus(brief, city.owner),
                        appetite: stakes.appetite,
                        stake: stakes.capital(city.tile).map_or(0, |s| s.value),
                        rounds,
                        distance,
                        at_war: brief.at_war(city.owner),
                        armed: armed.is_some(),
                        features: chance_features(brief, state, rules, arsenal, city, target, need, None),
                        round: brief.round,
                        player: brief.player,
                        enemy: city.owner,
                        population: city.population,
                        value,
                        gain: case.gain,
                        cost: case.cost,
                        unrest: prices.captured_city_unrest(city.population) * stakes.unrest / 100,
                        worth: city_worth(prices, city.population, rounds),
                        missing: missing.total().min(255) as u8,
                        need: need.total().min(255) as u8,
                        free: free.total().min(255) as u8,
                        excess: brief.happiness,
                        happiness_price: prices.happiness,
                        citizen: prices.score.citizen,
                    })
                });
                value
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
        (value >= 0).then_some(Candidate { operation, horizon, assemble, march, distance, value, chest, decisive, pace: brief.speed.pace_percent })
    };
    // The army alone, and with each weapon the player has or could build: the
    // weapon strikes the city before the army comes (`strikes::struck`), the army it
    // needs is the army for what is left, and its price and project are in the
    // case. The best plan that fits in time counts; a city the army alone cannot
    // take (more than `MAX_ARMY` units) can be taken with the weapon.
    let base = composition_by(rules, arsenal, &estimate, threat, brief.army_fit);
    let mut best: Option<Candidate> = base.and_then(|need| plan(need, None, &estimate)).filter(Candidate::fits);
    // With the learned chance a larger army takes a city more often (`learned_chance`): the
    // army that is worth most up to `MAX_ARMY`, each unit added as `composition` adds them.
    if let Some(mut need) = base.filter(|_| brief.war.siege.learned) {
        while need.total() < MAX_ARMY {
            grow(&mut need, arsenal);
            let Some(larger) = plan(need, None, &estimate).filter(Candidate::fits) else { continue };
            if best.as_ref().is_none_or(|b| larger.value > b.value) {
                best = Some(larger);
            }
        }
    }
    for armed in &brief.armory {
        let hit = crate::ai::strikes::struck(brief, rules, city, &estimate, armed);
        let Some(need) = composition_by(rules, arsenal, &hit, threat, brief.army_fit) else { continue };
        if let Some(armed_plan) = plan(need, Some(armed), &hit).filter(Candidate::fits) {
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
    let extra = unit_upkeep_at(units + missing.total() as usize, brief.round, brief.speed.max_rounds()) - unit_upkeep_at(units, brief.round, brief.speed.max_rounds());
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
    // The content costs at the game speed (the rules' production cost).
    let total = kernel::rules::speeds::SpeedValues::scale(total, brief.speed.production_percent);
    let production: u32 = brief.cities.iter().map(|c| c.production).sum();
    total.div_ceil((production / 2).max(1)) + GATHER_ROUNDS
}

/// The player's healthy army units no task holds: none, on an assault (but
/// with `SiegeFixes::assault`, which values it by `task_shares`), or a
/// garrison beyond what its city wants (but never a guard of a city in
/// danger, `ai::defence`).
pub(crate) fn free_units<'b>(brief: &'b TurnBrief, state: &'b AiState, rules: &'b CompiledRuleset) -> impl Iterator<Item = &'b UnitBrief> {
    let wanted = garrison_wanted_by(brief, state.strategy.current).max(1);
    brief.units.iter().filter(move |u| {
        let free = match state.task_of(u.id).map(|t| t.task) {
            None => true,
            Some(UnitTask::Assault { .. }) => !brief.war.siege.assault,
            Some(UnitTask::Defend { city }) if brief.endangered(city) => false,
            Some(UnitTask::Defend { city }) => senior_defenders(state, city, u.id) >= wanted,
            Some(_) => false,
        };
        free && u.hp >= HEAL_BELOW_HP && army_role(rules, u.kind).is_some()
    })
}

/// How the player's army units are held (the war probe): all, in an operation, as the garrison
/// a city wants, as a guard of a city in danger, hurt, with another task.
fn held_classes(brief: &TurnBrief, state: &AiState, rules: &CompiledRuleset) -> (usize, usize, usize, usize, usize, usize) {
    let wanted = garrison_wanted_by(brief, state.strategy.current).max(1);
    let mut out = (0, 0, 0, 0, 0, 0);
    for u in brief.units.iter().filter(|u| army_role(rules, u.kind).is_some()) {
        out.0 += 1;
        match state.task_of(u.id).map(|t| t.task) {
            Some(UnitTask::Operation { .. }) => out.1 += 1,
            _ if u.hp < HEAL_BELOW_HP => out.4 += 1,
            Some(UnitTask::Defend { city }) if brief.endangered(city) => out.3 += 1,
            Some(UnitTask::Defend { city }) if senior_defenders(state, city, u.id) < wanted => out.2 += 1,
            Some(UnitTask::Defend { .. }) | Some(UnitTask::Assault { .. }) | None => {}
            Some(_) => out.5 += 1,
        }
    }
    out
}

/// Foreign military units this close to a city make its garrison needed.
pub const STRIKE_SAFE_RADIUS: i32 = 6;
/// The share of the cities that lend their garrison to a march: one city in this many.
pub const STRIKE_LEND_ONE_IN: usize = 3;

/// The standing strike group (`WarTuning::group`, classic/docs/ai-war-audit-current.md): a
/// garrison whose city is out of danger and has no foreign military unit in sight within
/// `STRIKE_SAFE_RADIUS` may march (the city orders a new defender at once, `ungarrisoned_city`),
/// for one city in `STRIKE_LEND_ONE_IN` (the lowest ids first): the army that
/// stands in peace is the garrison, so without it every march started from nothing (median free
/// troops 0 to 1).
fn lent_garrisons(brief: &TurnBrief, state: &AiState, rules: &CompiledRuleset) -> Vec<EntityId> {
    let dims = brief.dims;
    let mut out = Vec::new();
    let allowance = brief.cities.len() / STRIKE_LEND_ONE_IN;
    for city in brief.cities.iter().filter(|c| !brief.endangered(c.tile)) {
        if out.len() >= allowance {
            break;
        }
        let quiet = !brief.sightings.iter().any(|s| s.military && dims.distance(s.tile, city.tile).is_some_and(|d| d <= STRIKE_SAFE_RADIUS));
        if !quiet {
            continue;
        }
        let holder = brief
            .units
            .iter()
            .filter(|u| u.hp >= HEAL_BELOW_HP && army_role(rules, u.kind).is_some())
            .filter(|u| state.task_of(u.id).map(|t| t.task) == Some(UnitTask::Defend { city: city.tile }))
            .map(|u| u.id)
            .min();
        out.extend(holder);
    }
    out
}

/// The army units an operation or a raid may take: `free_units`, and with `WarTuning::group` the
/// garrisons `lent_garrisons` names.
pub(crate) fn strike_units<'b>(brief: &'b TurnBrief, state: &'b AiState, rules: &'b CompiledRuleset) -> impl Iterator<Item = &'b UnitBrief> {
    let lent = if brief.war.group { lent_garrisons(brief, state, rules) } else { Vec::new() };
    brief.units.iter().filter(move |u| lent.contains(&u.id)).chain(free_units(brief, state, rules)).collect::<Vec<_>>().into_iter()
}

/// Garrison holders of `city` with a lower id than `unit`.
fn senior_defenders(state: &AiState, city: TileId, unit: EntityId) -> usize {
    state.tasks().iter().filter(|t| t.task == UnitTask::Defend { city } && t.unit < unit).count()
}

/// Give free units to the operation: while it forms, the nearest to the
/// muster point while a role is missing, then up to `EXTRA_MEMBERS` more —
/// within `RECRUIT_RANGE`, or `DECISIVE_RANGE` for a `decisive` one (its
/// muster point may lie farther from every city); while it marches, every
/// unit on an assault of its target. With `SiegeFixes::assault` the units on
/// an assault are not free: it takes those of its own target, and while it
/// forms those of another whose share of their target (`task_shares`) is
/// below what one member of this operation is worth (`assault_recruits`).
fn recruit(brief: &TurnBrief, state: &mut AiState, rules: &CompiledRuleset, operation: &Operation, gate: WarGate<'_>) {
    let range = if gate.decisive(operation.target) { DECISIVE_RANGE } else { RECRUIT_RANGE };
    let dims = brief.dims;
    let target = operation.target;
    let mut have = composition_of(rules, members(brief, state, target).iter().map(|u| u.kind));
    let switching = if brief.war.siege.assault { assault_recruits(brief, state, rules, operation, gate) } else { Vec::new() };
    let arsenal = Arsenal::of(brief, rules);
    let mut candidates: Vec<(i32, EntityId, UnitKindId)> = strike_units(brief, state, rules)
        .filter(|u| match operation.phase {
            OperationPhase::Forming => true,
            OperationPhase::Marching => state.task_of(u.id).map(|t| t.task) == Some(UnitTask::Assault { target }),
        })
        .chain(switching)
        .filter(|u| current(brief, rules, &arsenal, u.kind))
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

/// The healthy army units on an assault that `operation` takes with
/// `SiegeFixes::assault`: those on an assault of its own target (no task
/// switch: they join the army that goes there); while it forms, under a
/// valued gate, also those on an assault of another target whose share of it
/// (`task_shares`) is below their share of `operation` (`unit_share`).
fn assault_recruits<'b>(brief: &'b TurnBrief, state: &AiState, rules: &CompiledRuleset, operation: &Operation, gate: WarGate<'_>) -> Vec<&'b UnitBrief> {
    let assault = |u: &UnitBrief| match state.task_of(u.id).map(|t| t.task) {
        Some(UnitTask::Assault { target }) if u.hp >= HEAL_BELOW_HP && army_role(rules, u.kind).is_some() => Some(target),
        _ => None,
    };
    let mut out: Vec<&UnitBrief> = brief.units.iter().filter(|u| assault(u) == Some(operation.target)).collect();
    let WarGate::Value(prices, stakes) = gate else { return out };
    if operation.phase != OperationPhase::Forming {
        return out;
    }
    let others: Vec<&UnitBrief> = brief.units.iter().filter(|u| assault(u).is_some_and(|t| t != operation.target)).collect();
    if others.is_empty() {
        return out;
    }
    let shares = task_shares(brief, state, rules, prices, stakes);
    let share = |u: &UnitBrief| shares.iter().find(|(id, _)| *id == u.id).map_or(0, |(_, s)| *s);
    let worth = target_worth(brief, state, rules, prices, stakes, operation.target);
    out.extend(others.into_iter().filter(|u| share(u) < unit_share(rules, worth, u)));
    out
}

/// What each unit on an assault or an operation is worth to its task
/// (`SiegeFixes::assault`): its share of what its target is worth
/// (`unit_share`). A unit leaves its task for another only when the other
/// saves or brings more than this — one rule for every task switch, as
/// `defence::recall` weighs an operation against its saving.
pub(crate) fn task_shares(brief: &TurnBrief, state: &AiState, rules: &CompiledRuleset, prices: &Prices, stakes: &Stakes) -> Vec<(EntityId, Centi)> {
    let target_of = |u: &UnitBrief| match state.task_of(u.id).map(|t| t.task) {
        Some(UnitTask::Assault { target }) | Some(UnitTask::Operation { target }) => Some(target),
        _ => None,
    };
    let mut targets: Vec<TileId> = brief.units.iter().filter_map(target_of).collect();
    targets.sort();
    targets.dedup();
    let mut out = Vec::new();
    for target in targets {
        let worth = target_worth(brief, state, rules, prices, stakes, target);
        out.extend(brief.units.iter().filter(|u| target_of(u) == Some(target)).map(|u| (u.id, unit_share(rules, worth, u))));
    }
    out
}

/// The city a unit sent to the war assaults: the objective of a war (as
/// `planner_units` gives an assault), the first by the relations' order.
pub(crate) fn assault_target(state: &AiState) -> Option<TileId> {
    let objective = |m: &crate::ai::state::RelationMemory| m.objective.filter(|o| m.at_war && o.kind == ObjectiveKind::City);
    state.relations().iter().find_map(objective).map(|o| o.tile)
}

/// What `unit` would be worth on an assault of `target` (`SiegeFixes::assault`,
/// `unit_share`).
pub(crate) fn assault_share(brief: &TurnBrief, state: &AiState, rules: &CompiledRuleset, prices: &Prices, stakes: &Stakes, target: TileId, unit: &UnitBrief) -> Centi {
    unit_share(rules, target_worth(brief, state, rules, prices, stakes, target), unit)
}

/// A unit's share of `worth` (`target_worth`: the target's value and the
/// army it takes): the target's value, never below nothing, by the unit's
/// power over the army's at full health.
fn unit_share(rules: &CompiledRuleset, worth: Option<(Centi, Composition, Arsenal)>, unit: &UnitBrief) -> Centi {
    let Some((value, army, arsenal)) = worth else { return 0 };
    let full = crate::world::unit::UNIT_MAX_HP;
    let power = |role: ArmyRole| {
        let kind = arsenal.kind(role).or(arsenal.ranged).or(arsenal.melee);
        i64::from(kind.map_or(0, |k| unit_power(rules, k, full))) * i64::from(army.of(role))
    };
    let total = (power(ArmyRole::Melee) + power(ArmyRole::Ranged) + power(ArmyRole::Siege)).max(1);
    value.max(0) * i64::from(unit_power(rules, unit.kind, unit.hp)) / total
}

/// What taking `target` is worth and the army it takes: a running
/// operation's value (`running_value`) and its need; for an assault, the case
/// of the army that takes the city (`composition`, as an operation would
/// plan it) with its units at hand — the walk from our nearest city and the
/// siege. None for a target no longer known or that no army takes.
fn target_worth(brief: &TurnBrief, state: &AiState, rules: &CompiledRuleset, prices: &Prices, stakes: &Stakes, target: TileId) -> Option<(Centi, Composition, Arsenal)> {
    let arsenal = Arsenal::of(brief, rules);
    if let Some(operation) = state.operation(target) {
        let value = running_value(brief, state, rules, prices, stakes, operation)?;
        return Some((value, operation.need, arsenal));
    }
    let city = state.seen_cities().iter().find(|c| c.tile == target)?;
    let estimate = estimate_target(brief, rules, city);
    let threat = state.relation(city.owner).map_or(0, |m| m.threat).max(defence(brief, city.owner, city.tile));
    let army = composition_by(rules, &arsenal, &estimate, threat, brief.army_fit)?;
    let distance = brief.nearest_city(brief.dims, target).and_then(|c| brief.dims.distance(c.tile, target)).unwrap_or(0).max(0) as u32;
    let rounds = (distance * 2).div_ceil(3) + 1 + SIEGE_ROUNDS_ESTIMATE;
    let chance = model_chance(brief, state, rules, &arsenal, city, &estimate, army, None);
    let case = war_case(prices, stakes, rules, &arsenal, city, army, Composition::default(), rounds, crate::ai::strikes::on_hand_best(&brief.armory), chance, brief.war.siege, rival_percent(brief, city.owner));
    Some((case.value(stakes) + infamy_bonus(brief, city.owner), army, arsenal))
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
    let step = approach(view, tools, operation.target, Reason::Advance);
    if view.brief.fog_march && matches!(step.reason, Reason::NoPath | Reason::PathOverBudget) {
        // No known way to the target: on through the fog.
        let fog = crate::ai::planner_units::fog_approach(view, tools, operation.target, 1, Reason::Advance);
        if fog.command.is_some() {
            return fog;
        }
    }
    step
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
