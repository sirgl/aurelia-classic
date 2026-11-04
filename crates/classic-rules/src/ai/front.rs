//! The front of the AI since the `tactics` variant (classic/docs/ai-tactics.md): the
//! military decisions of the units fighting around one place, planned
//! together.
//!
//! Every decision of a front member rebuilds the front from a fresh local
//! observation: the enemies in sight and the members (units on an
//! assault). It returns the first step of the best plan — a strike that
//! kills or takes a target (ranged first, melee last), else a favourable
//! strike, else the asked unit's place (heal at home when wounded, gather
//! out of the city's strike, close in when the siege is ready). A strike
//! may belong to another member: the driver runs it for that unit and asks
//! again.
//! Combat is judged by the rules' public previews (`attack_preview_from`),
//! moves by the player's move preview (`own_reachable`); nothing hidden is
//! read. The tiles a unit strikes from and gathers on are ranked by their
//! danger: the count of the enemies that strike there, or with the danger
//! net (`TurnBrief::danger_net`) the expected loss the net gives for the
//! unit ending there (`danger::Threats`, built once per decision).

use std::cell::OnceCell;

use crate::ai::battlefield::{worth, KindStats};
use crate::ai::combat::{ranged_range, strike_value, Exposure, FightFix};
use crate::ai::danger::{self, Place, Threats};
use crate::ai::planner::{kind_strength, role_of, AiRole, Decision, PlanTools, UnitView};
use crate::ai::planner_units::{ensure_task, follow_task, hold, move_along};
use crate::ai::siege_tactics::{self, range_of};
use crate::ai::state::{AiState, OperationPhase, UnitTask};
use crate::ai::trace::{Actor, Reason};
use crate::combat::city::CITY_ATTACK_RANGE;
use crate::command::Command;
use crate::ids::TileId;
use crate::knowledge::observation::{ObservedCity, ObservedUnit, OwnUnit};
use crate::knowledge::public::PlayerQueries;
use crate::world::stacking::stack_class;

/// Members a front plans for, the nearest to the asked unit first.
pub const MAX_MEMBERS: usize = 12;
/// Combat previews one decision may compute.
pub const MAX_PREVIEWS: u32 = 120;
/// A member below this health leaves the front to heal: the one health of
/// the wounded (`planner_units::HEAL_BELOW_HP`).
pub const HEAL_BELOW: u16 = crate::ai::planner_units::HEAL_BELOW_HP;
/// A melee strike on a city that is not taken leaves the attacker this much.
pub const STRIKE_KEEPS_HP: u16 = 25;
/// The siege starts when the group would take the city within this many
/// rounds of strikes (`siege_rounds`).
pub const SIEGE_ROUNDS: u32 = 4;
/// Members this close to a city make its siege group.
pub const GROUP_RADIUS: i32 = 4;
/// The gathering ring around an enemy city: just out of its strike.
pub const STAGING_DISTANCE: i32 = CITY_ATTACK_RANGE + 1;
/// Health a city regains per round (`combat::city`).
const CITY_HEAL: u32 = crate::combat::city::CITY_HEAL_PER_TURN as u32;
/// Value of taking a city, in our hit points.
const CITY_VALUE: i32 = 1000;
/// Cost of a strike that may cost the attacker its life.
const LOSS_RISK_COST: i32 = 100;
/// A step the rules refuse is dropped and the next best tried this often.
const MAX_TRIES: usize = 4;

/// An enemy in sight: a unit or a city of a player at war with us.
#[derive(Debug, Clone, Copy)]
enum Foe {
    Unit(ObservedUnit),
    City(ObservedCity),
}

impl Foe {
    fn tile(&self) -> TileId {
        match self {
            Foe::Unit(u) => u.tile,
            Foe::City(c) => c.tile,
        }
    }

    fn hp(&self) -> u16 {
        match self {
            Foe::Unit(u) => u.hp,
            Foe::City(c) => c.hp,
        }
    }

    fn is_city(&self) -> bool {
        matches!(self, Foe::City(_))
    }
}

/// One possible strike this turn: a member, the tile it strikes from (its
/// own or one it reaches with moves left) and the expected exchange.
#[derive(Debug, Clone, Copy)]
struct Strike {
    unit: OwnUnit,
    from: TileId,
    foe: Foe,
    ranged: bool,
    dealt: u16,
    taken: u16,
    /// The worst roll could kill the attacker.
    risky: bool,
    /// The attacker's extra loss at `from` after the strike, beyond the
    /// safest tile it reaches (`combat::Exposure::strike_cost`); 0 without
    /// `FightFix::reply`.
    cost: i32,
}

/// What the asked unit sees of its front.
struct Front<'v, 'o, 'a> {
    view: &'v UnitView<'o, 'a>,
    /// The asked unit first (when it is a member), then by distance and id.
    members: Vec<OwnUnit>,
    /// By distance to the asked unit, then by tile.
    foes: Vec<Foe>,
    /// The danger net's threats of the decision, built at the first tile
    /// it ranks (None inside: the variant does not read the net).
    threats: OnceCell<Option<Threats<'o, 'a>>>,
}

/// The front's decision for the asked unit; None when it is not a front
/// member or no enemy is near (the planners decide then).
pub fn decide(view: &UnitView<'_, '_>, state: &mut AiState, tools: &mut PlanTools<'_>) -> Option<Decision> {
    let unit = view.local.unit;
    if role_of(tools.rules, unit.kind) != AiRole::Military {
        return None;
    }
    let foes = foes(view);
    if foes.is_empty() {
        return wait_out_of_sight(view, state, tools);
    }
    let task = ensure_task(view, state, tools);
    if !is_member(state, task) {
        return None;
    }
    let front = Front { view, members: members(view, state), foes, threats: OnceCell::new() };
    let mut strikes = front.strikes(tools);
    tools.counters.candidates += strikes.len() as u64;
    let ready = front.ready_sieges(&strikes, tools);
    for _ in 0..MAX_TRIES {
        let Some(strike) = front.next_strike(&strikes, &ready, tools) else { break };
        let decision = front.step(&strike);
        if decision.command.as_ref().is_some_and(|c| view.local.view.accepts(c)) {
            return Some(decision);
        }
        strikes.retain(|s| (s.unit.id, s.foe.tile()) != (strike.unit.id, strike.foe.tile()));
    }
    let place = front.place(state, tools, &ready);
    match &place.command {
        Some(command) if !view.local.view.accepts(command) => Some(hold(view, Reason::Blocked)),
        _ => Some(place),
    }
}

/// A member on the gathering ring of its target, which it does not see
/// (the ring lies out of the city's strike and may lie beyond the unit's
/// sight), holds there while its siege group would not take the city as the
/// player estimates it from its last sighting (`operations::estimate_target`,
/// the members' health): it heals instead of stepping into sight only to be
/// sent back by `gather` in the same turn. None: the planners decide.
fn wait_out_of_sight(view: &UnitView<'_, '_>, state: &AiState, tools: &PlanTools<'_>) -> Option<Decision> {
    let unit = view.local.unit;
    let task = state.task_of(unit.id).map(|t| t.task);
    let target = match task {
        Some(UnitTask::Assault { target }) | Some(UnitTask::Operation { target }) if is_member(state, task) => target,
        _ => return None,
    };
    if distance(view, unit.tile, target) > STAGING_DISTANCE || view.local.view.tile(target).is_visible() {
        return None;
    }
    let seen = state.seen_cities().iter().find(|c| c.tile == target && at_war(view, c.owner))?;
    let city = crate::ai::operations::estimate_target(view.brief, tools.rules, seen);
    let dims = view.local.view.dimensions();
    let mut group: Vec<Besieger> = view
        .brief
        .units
        .iter()
        .filter(|u| {
            let task = state.task_of(u.id).map(|t| t.task);
            is_member(state, task) && matches!(task, Some(UnitTask::Assault { target: t }) | Some(UnitTask::Operation { target: t }) if t == target)
        })
        .filter(|u| dims.distance(u.tile, target).is_some_and(|d| d <= GROUP_RADIUS))
        .filter_map(|u| crate::ai::operations::estimated_strike(tools.rules, u.kind, u.hp, &city))
        .filter(|b| b.ranged || b.hp >= HEAL_BELOW)
        .collect();
    if view.brief.war.siege.close && wears_down(&group, city.heal) {
        return None;
    }
    if siege_rounds(&mut group, city.hp, city.heal).is_some() {
        return None;
    }
    Some(hold(view, Reason::Rally))
}

/// Front members: units on an assault, and members of a marching
/// operation (`ai::operations`; a forming one gathers away from the front).
pub(crate) fn is_member(state: &AiState, task: Option<UnitTask>) -> bool {
    match task {
        Some(UnitTask::Assault { .. }) => true,
        Some(UnitTask::Operation { target }) => state.operation(target).is_some_and(|o| o.phase == OperationPhase::Marching),
        _ => false,
    }
}

fn at_war(view: &UnitView<'_, '_>, owner: crate::ids::PlayerId) -> bool {
    view.local.relation(owner).is_some_and(|r| r.at_war)
}

/// Enemy units and cities in sight, nearest first.
fn foes(view: &UnitView<'_, '_>) -> Vec<Foe> {
    let local = view.local;
    let in_city = |t: TileId| local.foreign_cities.iter().any(|c| c.tile == t);
    let units = local
        .foreign_units
        .iter()
        .filter(|u| at_war(view, u.owner) && !in_city(u.tile))
        .map(|u| Foe::Unit(*u));
    let cities = local.foreign_cities.iter().filter(|c| at_war(view, c.owner)).map(|c| Foe::City(*c));
    let mut foes: Vec<Foe> = units.chain(cities).collect();
    // One foe per tile: the rules pick the tile's defender.
    foes.sort_by_key(|f| (distance(view, local.unit.tile, f.tile()), f.tile(), !f.is_city()));
    foes.dedup_by_key(|f| f.tile());
    foes
}

/// The asked unit and the other units on an assault, nearest first.
fn members(view: &UnitView<'_, '_>, state: &AiState) -> Vec<OwnUnit> {
    let rules = view.local.view.rules();
    let unit = view.local.unit;
    let mut others: Vec<OwnUnit> = view
        .local
        .own_units
        .iter()
        .filter(|u| role_of(rules, u.kind) == AiRole::Military)
        .filter(|u| is_member(state, state.task_of(u.id).map(|t| t.task)))
        // The tactical layer's members take their orders from its plan.
        .filter(|u| !view.brief.tactics.as_ref().is_some_and(|t| t.commands(u.handle)))
        .copied()
        .collect();
    others.sort_by_key(|u| (distance(view, unit.tile, u.tile), u.id));
    others.truncate(MAX_MEMBERS - 1);
    let mut members = vec![unit];
    members.extend(others);
    members
}

fn distance(view: &UnitView<'_, '_>, a: TileId, b: TileId) -> i32 {
    view.local.view.dimensions().distance(a, b).unwrap_or(i32::MAX)
}

impl<'o, 'a> Front<'_, 'o, 'a> {
    fn distance(&self, a: TileId, b: TileId) -> i32 {
        distance(self.view, a, b)
    }

    /// How bad `tile` is for `unit` to end its move on, the lower the
    /// better (ignoring the foe on `except`, the one it strikes): the
    /// danger net's expected loss there with `TurnBrief::danger_net`, else
    /// the count of threats (`danger`); with `SiegeTactics::ground` the
    /// battlefield's danger there (the enemy's blows on the ground's defence).
    fn risk(&self, unit: &OwnUnit, tile: TileId, except: Option<TileId>) -> u32 {
        let brief = self.view.brief;
        let read = brief.danger_net || brief.fight.reply || brief.siege.ground;
        match self.threats().filter(|_| read) {
            Some(threats) => self.expected_loss(threats, unit, tile, except),
            None => self.danger(tile, except),
        }
    }

    /// The danger net's threats of the decision: the members may move.
    fn threats(&self) -> Option<&Threats<'o, 'a>> {
        self.threats
            .get_or_init(|| {
                let brief = self.view.brief;
                let sight = brief.danger.as_ref().filter(|_| brief.danger_net || brief.fight.reply || brief.siege.ground)?;
                let started = prof::now_ns();
                let movers: Vec<(TileId, i32)> = self.members.iter().map(|u| (u.tile, self.steps_left(u))).collect();
                let threats = Threats::around(self.view.local, self.view.local.view.rules(), brief, sight, &movers);
                danger::note_build(prof::now_ns().saturating_sub(started));
                Some(threats)
            })
            .as_ref()
    }

    /// The net's expected loss of `unit` ending its move on `tile`, in hit
    /// points (0 where no enemy strikes it); the foe on `except` counted gone.
    fn expected_loss(&self, threats: &Threats<'_, '_>, unit: &OwnUnit, tile: TileId, except: Option<TileId>) -> u32 {
        let started = prof::now_ns();
        let rules = self.view.local.view.rules();
        let posture = threats.posture_with(unit, tile);
        let gone: Vec<(TileId, u16)> = except.map(|t| (t, 0)).into_iter().collect();
        let place = Place { tile, posture: &posture, hurt: &gone };
        let worth = worth(&KindStats::of(rules, unit.kind), unit.level);
        let fight = self.view.brief.fight;
        let loss = if fight.reply {
            danger::expected_loss(threats.danger_by(fight.source, rules, &danger::moved(unit, tile), &place), worth)
        } else if self.view.brief.danger_net {
            threats.net_danger(rules, &danger::moved(unit, tile), &place).map_or(0, |d| danger::expected_loss(d, worth))
        } else {
            danger::expected_loss(threats.search_danger(rules, &danger::moved(unit, tile), &place), worth)
        };
        danger::note_row(prof::now_ns().saturating_sub(started));
        loss.max(0) as u32
    }

    /// Threats on `tile` for `unit`: enemy cities that strike it there and
    /// enemy military units next to it (ignoring `except`).
    fn danger(&self, tile: TileId, except: Option<TileId>) -> u32 {
        let rules = self.view.local.view.rules();
        let mut threats = 0;
        for foe in self.foes.iter().filter(|f| Some(f.tile()) != except) {
            let d = self.distance(tile, foe.tile());
            threats += u32::from(match foe {
                Foe::City(_) => d <= CITY_ATTACK_RANGE,
                Foe::Unit(u) => d <= 1 && role_of(rules, u.kind) == AiRole::Military,
            });
        }
        threats
    }

    /// May `unit` end its move on `tile` as far as the player sees (one
    /// unit of a stack class, no foreign unit)?
    fn free_for(&self, unit: &OwnUnit, tile: TileId) -> bool {
        if tile == unit.tile {
            return true;
        }
        let rules = self.view.local.view.rules();
        let local = self.view.local;
        let class = stack_class(rules, unit.kind);
        let kinds = local.view.terrain_kinds();
        let water = local.view.tile(tile).snapshot().is_none_or(|s| kinds.is_water(s.terrain.0));
        let mine = std::iter::once(&local.unit).chain(&local.own_units);
        let own = mine.filter(|u| u.id != unit.id).any(|u| u.tile == tile && stack_class(rules, u.kind) == class);
        !water && !own && !local.foreign_units.iter().any(|u| u.tile == tile)
    }

    /// Most tiles `unit` may still walk this turn (every step on a road).
    fn steps_left(&self, unit: &OwnUnit) -> i32 {
        let profile = self.view.local.view.own_unit_profile(unit.handle);
        let step = profile.map_or(1, |p| p.routes.road_points.max(1));
        (u32::from(unit.moves_left) / step) as i32
    }

    /// Tiles `unit` may stop on this turn with moves left, its own first.
    fn stops(&self, unit: &OwnUnit) -> Vec<(TileId, u16)> {
        let mut stops: Vec<(TileId, u16)> = self
            .view
            .local
            .view
            .own_reachable(unit.handle)
            .into_iter()
            .filter(|(t, _)| self.free_for(unit, *t))
            .collect();
        stops.sort_by_key(|(t, _)| (*t != unit.tile, *t));
        stops
    }
}

impl Front<'_, '_, '_> {
    /// Every member's best strike on every foe it can reach this turn,
    /// within the preview budget.
    fn strikes(&self, tools: &PlanTools<'_>) -> Vec<Strike> {
        let mut out = Vec::new();
        let mut previews = 0u32;
        for unit in &self.members {
            let range = range_of(tools.rules, unit, self.view.brief.siege);
            // With the reply valued the strike's cost judges a wounded attacker.
            let fit = range.is_some() || unit.hp >= HEAL_BELOW || self.view.brief.fight.reply;
            if !unit.can_attack || unit.moves_left == 0 || !fit {
                continue;
            }
            // The move preview only when a foe is out of reach from here.
            let mut stops = None;
            let mut safe = None;
            let reach = range.unwrap_or(1) + self.steps_left(unit);
            for foe in self.foes.iter().filter(|f| self.distance(unit.tile, f.tile()) <= reach) {
                if previews >= MAX_PREVIEWS {
                    return out;
                }
                let Some(from) = self.strike_tile(unit, range, foe, &mut stops) else { continue };
                previews += 1;
                if self.view.brief.fight.reply && safe.is_none() {
                    safe = Some(self.safe_loss(unit, &mut stops));
                }
                if let Some(strike) = self.preview(unit, from, foe, range.is_some(), safe.flatten()) {
                    out.push(strike);
                }
            }
        }
        out
    }

    /// Where `unit` strikes `foe` from this turn: its own tile when in
    /// reach, else the safest reachable tile with moves left (a ranged
    /// unit keeps its distance).
    fn strike_tile(&self, unit: &OwnUnit, range: Option<i32>, foe: &Foe, stops: &mut Option<Vec<(TileId, u16)>>) -> Option<TileId> {
        let siege = self.view.brief.siege;
        if let (Some(range), true) = (range, siege.firing || siege.outrange) {
            // The firing tile (`siege_tactics::choose_firing_tile`).
            let stops = stops.get_or_insert_with(|| self.stops(unit));
            let rules = self.view.local.view.rules();
            let cities = siege_tactics::enemy_cities(self.view);
            let melee = siege_tactics::enemy_melee(self.view);
            return siege_tactics::choose_firing_tile(&self.view.local.view, rules, siege, unit, range, foe.tile(), foe.hp(), stops, &cities, &melee);
        }
        let reach = range.unwrap_or(1);
        let fits = |t: TileId| (1..=reach).contains(&self.distance(t, foe.tile()));
        if fits(unit.tile) {
            return Some(unit.tile);
        }
        stops
            .get_or_insert_with(|| self.stops(unit))
            .iter()
            .filter(|(t, left)| *left > 0 && fits(*t))
            .min_by_key(|(t, left)| (self.risk(unit, *t, Some(foe.tile())), -self.ground(*t), -self.distance(*t, foe.tile()), std::cmp::Reverse(*left), *t))
            .map(|(t, _)| *t)
    }

    /// The defence of `tile`'s ground for our unit with
    /// `SiegeTactics::ground` (`siege_tactics::ground_percent`), 0 without it:
    /// the tie-break of the tiles a member ends on.
    fn ground(&self, tile: TileId) -> i64 {
        if !self.view.brief.siege.ground {
            return 0;
        }
        siege_tactics::ground_percent(&self.view.local.view, self.view.local.view.rules(), tile, self.view.local.player)
    }

    /// The loss of `unit` at the safest tile it reaches (`Exposure::safest`);
    /// None without the threats.
    fn safe_loss(&self, unit: &OwnUnit, stops: &mut Option<Vec<(TileId, u16)>>) -> Option<i32> {
        let threats = self.threats()?;
        let exposure = Exposure::of(threats, self.view.local.view.rules(), self.view.brief.fight.source, unit);
        let tiles: Vec<TileId> = stops.get_or_insert_with(|| self.stops(unit)).iter().map(|(t, _)| *t).chain(std::iter::once(unit.tile)).collect();
        exposure.safest(&tiles, |t| i32::from(t != unit.tile)).map(|(_, loss)| loss)
    }

    fn preview(&self, unit: &OwnUnit, from: TileId, foe: &Foe, ranged: bool, safe: Option<i32>) -> Option<Strike> {
        let preview = self.view.local.view.attack_preview_from(unit.handle, from, foe.tile())?;
        let dealt = preview.to_defender.1;
        let taken = preview.to_attacker.1;
        let cost = match (self.threats(), safe) {
            (Some(threats), Some(safe)) if taken < unit.hp => {
                let exposure = Exposure::of(threats, self.view.local.view.rules(), self.view.brief.fight.source, unit);
                exposure.strike_cost(from, unit.hp - taken, foe.tile(), foe.hp().saturating_sub(dealt), safe)
            }
            _ => 0,
        };
        (dealt > 0 && taken < unit.hp).then_some(Strike {
            unit: *unit,
            from,
            foe: *foe,
            ranged,
            dealt,
            taken,
            risky: preview.to_attacker.2 >= unit.hp,
            cost,
        })
    }
}

impl Front<'_, '_, '_> {
    /// The strike to make now: the first of the best plan that kills or
    /// takes a target, else the best favourable strike.
    fn next_strike(&self, strikes: &[Strike], ready: &[TileId], tools: &PlanTools<'_>) -> Option<Strike> {
        let mut best: Option<(i32, TileId, Strike)> = None;
        for foe in &self.foes {
            let Some((cost, first)) = kill_plan(foe, strikes) else { continue };
            let score = value(foe, tools) - cost;
            if score > 0 && best.is_none_or(|(s, t, _)| score > s || (score == s && foe.tile() < t)) {
                best = Some((score, foe.tile(), first));
            }
        }
        if let Some((_, _, first)) = best {
            return Some(first);
        }
        strikes
            .iter()
            .filter(|s| favourable(s, ready, self.view.brief.fight, self.view.brief.war.siege.worth, tools) || self.bombards(s))
            .max_by_key(|s| (s.ranged, i32::from(s.dealt) - i32::from(s.taken), std::cmp::Reverse((s.unit.id, s.foe.tile()))))
            .copied()
    }

    /// A ranged strike on a city from a tile no enemy city strikes
    /// (`SiegeTactics::outrange`): the city cannot answer it, so it is made
    /// whenever it hurts, the siege ready or not.
    fn bombards(&self, strike: &Strike) -> bool {
        let cities = || siege_tactics::enemy_cities(self.view);
        self.view.brief.siege.outrange && strike.ranged && strike.foe.is_city() && strike.dealt > 0 && siege_tactics::city_strikes(&self.view.local.view, &cities(), strike.from) == 0
    }

    /// The command of a strike's first step: move to its tile, set up, or
    /// strike.
    fn step(&self, strike: &Strike) -> Decision {
        let unit = strike.unit;
        let actor = Actor::Unit(unit.id);
        let reason = if strike.foe.is_city() { Reason::AttackCity } else { Reason::AttackUnit };
        if strike.from != unit.tile {
            return Decision::act(actor, Reason::Advance, Command::MoveUnit { unit: unit.handle, destination: strike.from });
        }
        if self.view.local.view.must_set_up_first(unit.handle) {
            return Decision::act(actor, reason, Command::SetUpRangedAttack { unit: unit.handle });
        }
        let command = match (strike.foe, strike.ranged) {
            (_, true) => Command::RangedAttack { unit: unit.handle, target: strike.foe.tile() },
            (Foe::City(city), false) => Command::AttackCity { attacker: unit.handle, city: city.handle },
            (Foe::Unit(foe), false) => Command::AttackUnit { attacker: unit.handle, defender: foe.handle },
        };
        Decision::act(actor, reason, command)
    }
}

/// A strike worth making without a kill: a ranged shot from where the unit
/// stands, a melee exchange clearly in our favour, a strike on a city
/// whose siege is `ready`.
fn favourable(strike: &Strike, ready: &[TileId], fight: FightFix, worth: bool, tools: &PlanTools<'_>) -> bool {
    let stays = strike.unit.tile == strike.from;
    let left = strike.unit.hp.saturating_sub(strike.taken);
    if fight.reply || fight.odds {
        return favourable_fixed(strike, ready, fight, worth, tools);
    }
    match (strike.foe, strike.ranged) {
        (Foe::Unit(_), true) => true,
        (Foe::City(city), true) => stays || ready.contains(&city.tile),
        (Foe::Unit(_), false) => 2 * strike.dealt >= 3 * strike.taken && left >= HEAL_BELOW && !strike.risky,
        (Foe::City(city), false) => left >= STRIKE_KEEPS_HP && !strike.risky && ready.contains(&city.tile),
    }
}

/// `favourable` with the fight fix: a strike on a unit when its exchange's
/// value (`combat::strike_value`) less its cost is positive (a charge also
/// when the worst roll spares the attacker, `FightFix::odds`; without it the
/// melee rule stays the old one); a ranged strike on a city when its siege is
/// ready, or from where the unit stands while its damage beats its cost; a
/// charge on a city as before. With `worth` (`SiegeFixes::worth`) a ranged strike on a city
/// whose siege is ready is made when the damage at the city's worth (`city_damage_worth`)
/// beats its cost, not whatever it costs: the striker's risk against what the siege gains.
fn favourable_fixed(strike: &Strike, ready: &[TileId], fight: FightFix, siege_worth: bool, tools: &PlanTools<'_>) -> bool {
    let stays = strike.unit.tile == strike.from;
    let left = strike.unit.hp.saturating_sub(strike.taken);
    let foe_worth = |u: &ObservedUnit| worth(&KindStats::of(tools.rules, u.kind), 0);
    match (strike.foe, strike.ranged) {
        (Foe::City(city), true) if siege_worth => {
            let cost = strike.cost;
            ready.contains(&city.tile) && city_damage_worth(strike.dealt, city.population) > cost || stays && i32::from(strike.dealt) > cost
        }
        (Foe::Unit(u), true) => strike_value(strike.dealt, 0, u.hp, Some(foe_worth(&u))) > strike.cost,
        (Foe::City(city), true) => ready.contains(&city.tile) || stays && i32::from(strike.dealt) > strike.cost,
        (Foe::Unit(u), false) if fight.odds => !strike.risky && strike_value(strike.dealt, strike.taken, u.hp, Some(foe_worth(&u))) > strike.cost,
        (Foe::Unit(u), false) => 2 * strike.dealt >= 3 * strike.taken && left >= HEAL_BELOW && !strike.risky && strike_value(strike.dealt, strike.taken, u.hp, Some(foe_worth(&u))) > strike.cost,
        (Foe::City(city), false) => left >= STRIKE_KEEPS_HP && !strike.risky && ready.contains(&city.tile),
    }
}

/// Damage `dealt` to a city of `population` whose siege takes it, in our hit points
/// (`SiegeFixes::worth`): its share of the city's value (`value`), the city's base hit points
/// being the whole of it (walls are not seen).
pub(crate) fn city_damage_worth(dealt: u16, population: u32) -> i32 {
    let whole = CITY_VALUE + 20 * population as i32;
    i32::from(dealt) * whole / i32::from(crate::world::city::CITY_BASE_HIT_POINTS)
}

/// Value of destroying `foe`, in our hit points: a city is worth taking at
/// a high price, a unit by its strength, a civilian little.
fn value(foe: &Foe, tools: &PlanTools<'_>) -> i32 {
    match foe {
        Foe::City(city) => CITY_VALUE + 20 * city.population as i32,
        Foe::Unit(unit) if role_of(tools.rules, unit.kind) == AiRole::Military => 60 + 4 * kind_strength(tools.rules, unit.kind) as i32,
        Foe::Unit(_) => 40,
    }
}

/// The strikes that destroy `foe` this turn by the expected damage — ranged
/// first (a city keeps 1 HP against them), then melee by the best exchange —
/// with their cost (damage taken, a price for each strike that may lose the
/// attacker) and the first strike. None when they do not suffice.
fn kill_plan(foe: &Foe, strikes: &[Strike]) -> Option<(i32, Strike)> {
    let mut own: Vec<&Strike> = strikes.iter().filter(|s| s.foe.tile() == foe.tile()).collect();
    own.sort_by_key(|s| (!s.ranged, -i32::from(s.dealt) + i32::from(s.taken) * i32::from(!s.ranged), s.unit.id));
    let mut left = foe.hp();
    let (mut cost, mut first) = (0, None);
    for strike in own {
        let floor = u16::from(foe.is_city() && strike.ranged);
        let dealt = strike.dealt.min(left.saturating_sub(floor));
        if left == 0 || dealt == 0 {
            continue;
        }
        left -= dealt;
        cost += i32::from(strike.taken) + if strike.risky { LOSS_RISK_COST } else { 0 } + strike.cost;
        first.get_or_insert(*strike);
    }
    (left == 0).then_some((cost, first?))
}

impl Front<'_, '_, '_> {
    /// The cities in the front whose siege group would take them.
    fn ready_sieges(&self, strikes: &[Strike], tools: &PlanTools<'_>) -> Vec<TileId> {
        let cities = self.foes.iter().filter_map(|f| match f {
            Foe::City(city) => Some(city),
            Foe::Unit(_) => None,
        });
        cities.filter(|c| self.siege_takes(c, strikes, tools)).map(|c| c.tile).collect()
    }

    /// The siege group — the healthy members within `GROUP_RADIUS` of
    /// `city`, each with its strike this turn or a preview from next to the
    /// city — would take it within `SIEGE_ROUNDS` (`siege_rounds`). One
    /// test for closing in and for striking: members in reach strike while
    /// the others come.
    fn siege_takes(&self, city: &ObservedCity, strikes: &[Strike], tools: &PlanTools<'_>) -> bool {
        let mut besiegers = Vec::new();
        for unit in &self.members {
            let ranged = ranged_range(tools.rules, unit.kind).is_some();
            if self.distance(unit.tile, city.tile) > GROUP_RADIUS || (!ranged && unit.hp < HEAL_BELOW) {
                continue;
            }
            let this_turn = strikes.iter().find(|s| s.unit.id == unit.id && s.foe.tile() == city.tile);
            let exchange = this_turn.map(|s| (s.dealt, s.taken)).or_else(|| self.siege_preview(unit, city, ranged));
            if let Some((dealt, taken)) = exchange {
                besiegers.push(Besieger { ranged, dealt, taken, hp: unit.hp });
            }
        }
        if self.view.brief.war.siege.close && wears_down(&besiegers, CITY_HEAL as u16) {
            return true;
        }
        siege_rounds(&mut besiegers, city.hp, CITY_HEAL as u16).is_some()
    }

    /// Expected damage dealt and taken by `unit` striking `city` from a
    /// land tile next to it (or at its range): the planning preview of a
    /// strike in a later round.
    fn siege_preview(&self, unit: &OwnUnit, city: &ObservedCity, ranged: bool) -> Option<(u16, u16)> {
        let range = if ranged { range_of(self.view.local.view.rules(), unit, self.view.brief.siege)? } else { 1 };
        let dims = self.view.local.view.dimensions();
        crate::math::hex_disc(dims, city.tile, range)
            .into_iter()
            .filter(|t| self.distance(*t, city.tile) == range && self.free_for(unit, *t))
            .take(3)
            .find_map(|t| self.view.local.view.attack_preview_from(unit.handle, t, city.tile))
            .map(|p| (p.to_defender.1, p.to_attacker.1))
    }
}

/// A member of a siege group as the siege model sees it.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Besieger {
    pub ranged: bool,
    pub dealt: u16,
    pub taken: u16,
    pub hp: u16,
}

/// The group wears the city down (`SiegeFixes::close`): the expected damage
/// of a round of its strikes — every ranged unit, every melee unit that keeps
/// `STRIKE_KEEPS_HP` after the city's answer — beats what the city heals a
/// round. The rules let such a group take the city however many rounds it
/// takes; the members' own safety stays the front's (`favourable`,
/// `HEAL_BELOW`).
pub(crate) fn wears_down(group: &[Besieger], heal: u16) -> bool {
    let strikes = group.iter().filter(|b| b.ranged || b.hp.saturating_sub(b.taken) >= STRIKE_KEEPS_HP);
    strikes.map(|b| u32::from(b.dealt)).sum::<u32>() > u32::from(heal)
}

/// Rounds the group needs to take a city of `hp` that heals `heal` a
/// round, by the expected exchanges: every round the ranged units shoot
/// (a city keeps 1 HP against them), then every melee unit strikes while
/// it keeps `STRIKE_KEEPS_HP`; the city falls to a melee strike. None when
/// it does not fall within `SIEGE_ROUNDS`.
pub(crate) fn siege_rounds(group: &mut [Besieger], hp: u16, heal: u16) -> Option<u32> {
    siege_rounds_within(group, hp, heal, SIEGE_ROUNDS)
}

/// `siege_rounds` with the number of rounds the group may take (the tactical
/// layer allows more to a group that is at the walls already).
pub(crate) fn siege_rounds_within(group: &mut [Besieger], hp: u16, heal: u16, limit: u32) -> Option<u32> {
    group.sort_by_key(|b| !b.ranged);
    let mut city = hp;
    for round in 1..=limit {
        for b in group.iter_mut() {
            if b.ranged {
                city -= b.dealt.min(city.saturating_sub(1));
                continue;
            }
            if b.hp.saturating_sub(b.taken) < STRIKE_KEEPS_HP {
                continue;
            }
            b.hp -= b.taken;
            if b.dealt >= city {
                return Some(round);
            }
            city -= b.dealt;
        }
        city = city.saturating_add(heal).min(hp.max(city));
    }
    None
}

impl Front<'_, '_, '_> {
    /// The asked unit's place when it strikes nothing: heal at home when
    /// wounded, gather out of the city's strike or close in, else its task.
    fn place(&self, state: &mut AiState, tools: &mut PlanTools<'_>, ready: &[TileId]) -> Decision {
        let unit = self.view.local.unit;
        if unit.hp < HEAL_BELOW {
            // An operation's member heals with its army out of the city's
            // strike (it stays in the operation); others go home.
            let member = matches!(state.task_of(unit.id).map(|t| t.task), Some(UnitTask::Operation { .. }));
            return match self.siege_city(state) {
                Some(city) if member => self.gather(&city, tools),
                _ => self.rest(state, tools),
            };
        }
        let Some(city) = self.siege_city(state) else {
            return follow_task(self.view, state, tools);
        };
        let ranged = range_of(tools.rules, &unit, self.view.brief.siege);
        if ready.contains(&city.tile) {
            return self.close_in(&city, ranged.unwrap_or(1));
        }
        match ranged {
            Some(range) if self.view.brief.siege.outrange && siege_tactics::outranges(ranged) => self.post(&city, range, tools),
            _ => self.gather(&city, tools),
        }
    }

    /// A unit that outranges the city (`SiegeTactics::outrange`) waits for
    /// the siege where it strikes the city and no enemy city strikes it: a
    /// tile beyond the city's strike and within its range, the least risky,
    /// then the strongest ground, staying on ties; with no such tile in reach
    /// it gathers as the others do.
    fn post(&self, city: &ObservedCity, range: i32, tools: &mut PlanTools<'_>) -> Decision {
        let unit = self.view.local.unit;
        let cities = siege_tactics::enemy_cities(self.view);
        let posted = |t: TileId| {
            let d = self.distance(t, city.tile);
            d > CITY_ATTACK_RANGE && d <= range && siege_tactics::city_strikes(&self.view.local.view, &cities, t) == 0
        };
        let stops = self.stops(&unit);
        let best = stops
            .iter()
            .filter(|(t, _)| posted(*t))
            .min_by_key(|(t, left)| (self.risk(&unit, *t, None), -self.ground(*t), *t != unit.tile, std::cmp::Reverse(*left), *t))
            .map(|(t, _)| *t);
        match best {
            Some(tile) if tile != unit.tile => self.move_to(tile, Reason::Rally),
            Some(_) => hold(self.view, Reason::Rally),
            None => self.gather(city, tools),
        }
    }

    /// The war objective when it is in sight, else the nearest enemy city.
    fn siege_city(&self, state: &AiState) -> Option<ObservedCity> {
        let objective = state.task_of(self.view.local.unit.id).and_then(|t| match t.task {
            UnitTask::Assault { target } | UnitTask::Operation { target } => Some(target),
            _ => None,
        });
        let cities = self.foes.iter().filter_map(|f| match f {
            Foe::City(c) => Some(*c),
            Foe::Unit(_) => None,
        });
        let mut cities: Vec<ObservedCity> = cities.collect();
        cities.sort_by_key(|c| (Some(c.tile) != objective, self.distance(c.tile, self.view.local.unit.tile), c.tile));
        cities.first().copied()
    }

    /// Wounded: leave the front to heal in the nearest own city (the
    /// planners' heal task; the unit takes a new task once healed).
    fn rest(&self, state: &mut AiState, tools: &mut PlanTools<'_>) -> Decision {
        let unit = self.view.local.unit;
        let dims = self.view.local.view.dimensions();
        let city = match self.view.brief.fight.retreat {
            true => crate::ai::planner_units::heal_city(self.view),
            false => self.view.brief.nearest_city(dims, unit.tile).map(|c| c.tile),
        };
        if let Some(city) = city {
            state.set_task(unit.id, UnitTask::Heal { at: city }, self.view.brief.round);
            tools.counters.tasks_assigned += 1;
        }
        follow_task(self.view, state, tools)
    }

    /// The siege is not ready: wait on the ring just out of the city's
    /// strike, away from enemy units.
    fn gather(&self, city: &ObservedCity, tools: &mut PlanTools<'_>) -> Decision {
        let unit = self.view.local.unit;
        let off_ring = |t: TileId| (self.distance(t, city.tile) - STAGING_DISTANCE).abs();
        let stops = self.stops(&unit);
        let best = stops
            .iter()
            .filter(|(t, _)| self.distance(*t, city.tile) >= STAGING_DISTANCE)
            .min_by_key(|(t, left)| (self.risk(&unit, *t, None), off_ring(*t) > i32::from(self.view.brief.siege.ground), -self.ground(*t), off_ring(*t), std::cmp::Reverse(*left), *t))
            .map(|(t, _)| *t);
        match best {
            Some(tile) if tile != unit.tile => self.move_to(tile, Reason::Rally),
            Some(_) => hold(self.view, Reason::Rally),
            None if self.distance(unit.tile, city.tile) > STAGING_DISTANCE => move_along(self.view, tools, city.tile, Reason::Advance),
            None => hold(self.view, Reason::Rally),
        }
    }

    /// The siege is ready but the unit cannot strike this turn: come as
    /// close as its range (a melee unit next to the city).
    fn close_in(&self, city: &ObservedCity, range: i32) -> Decision {
        let unit = self.view.local.unit;
        let off = |t: TileId| (self.distance(t, city.tile) - range).abs();
        let stops = self.stops(&unit);
        let best = stops.iter().min_by_key(|(t, left)| (off(*t), -self.ground(*t), std::cmp::Reverse(*left), *t)).map(|(t, _)| *t);
        match best {
            Some(tile) if off(tile) < off(unit.tile) => self.move_to(tile, Reason::Advance),
            _ => hold(self.view, Reason::Rally),
        }
    }

    fn move_to(&self, tile: TileId, reason: Reason) -> Decision {
        let unit = self.view.local.unit;
        Decision::act(Actor::Unit(unit.id), reason, Command::MoveUnit { unit: unit.handle, destination: tile })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two archers and a swordsman against a full city: their round of strikes deals more than
    /// the city heals, but the swordsman is worn out before the city falls within
    /// `SIEGE_ROUNDS`. The game's AI waits on the ring for ever; with `SiegeFixes::close` the
    /// group closes in and wears the city down. A group that deals less than the city heals waits.
    #[test]
    fn the_siege_closes_in_when_its_damage_beats_the_healing() {
        let archer = Besieger { ranged: true, dealt: 12, taken: 0, hp: 100 };
        let swordsman = Besieger { ranged: false, dealt: 20, taken: 30, hp: 100 };
        let mut group = vec![archer, archer, swordsman];
        let heal = CITY_HEAL as u16;
        assert!(wears_down(&group, heal), "36 a round against {heal}");
        assert_eq!(siege_rounds(&mut group, 200, heal), None, "no fall within {SIEGE_ROUNDS} rounds");
        let weak = [Besieger { dealt: 9, ..archer }, Besieger { dealt: 9, ..archer }];
        assert!(!wears_down(&weak, heal), "18 a round heals back");
        let spent = [archer, Besieger { hp: 40, ..swordsman }];
        assert!(!wears_down(&spent, heal), "a melee unit that would fall below the strike's floor does not count");
    }

    /// A siege's damage is worth the city's share of its value (`SiegeFixes::worth`): the whole
    /// of the city's base hit points is the whole of `value`, so 20 damage to a city of four is
    /// worth 108 of our hit points, more than a shot that costs its shooter 50 — and 0 is worth 0.
    #[test]
    fn a_siege_shot_is_worth_its_share_of_the_city() {
        let whole = CITY_VALUE + 80;
        assert_eq!(city_damage_worth(crate::world::city::CITY_BASE_HIT_POINTS, 4), whole);
        assert_eq!(city_damage_worth(20, 4), 20 * whole / i32::from(crate::world::city::CITY_BASE_HIT_POINTS));
        assert!(city_damage_worth(20, 4) > 50);
        assert_eq!(city_damage_worth(0, 4), 0);
    }
}
