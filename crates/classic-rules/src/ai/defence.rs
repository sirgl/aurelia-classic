//! Home defence of the game's AI, `standard` (named `defence` until it
//! became the game's AI; classic/docs/ai-defence.md, M6.2 step 6).
//!
//! Why: the leaders' AI lost cities to modest attacks. Its armies were away
//! on operations or spread over the garrisons of quiet cities when a threat
//! came; production answered only an empty city; nothing said what keeping
//! a city is worth against what the army does elsewhere.
//!
//! The model: an own city with hostile units near it is a [`Danger`] — the
//! hostile power seen near it (each owner's by its hostility: war, the
//! distrust its recent behaviour earned, or an army gathering at our border
//! at peace), the power guarding it (the city itself and the own units next
//! to it) and its stake, what losing it costs in the allocator's measure:
//! its worth in score points (`operations::city_worth`, the measure of a
//! captured city) and, for the original capital, its share of the game —
//! the chance to win × the prize (`grand_strategy`) over the original
//! capitals the attacker still lacks, all of it when ours is the last one.
//! The chance to lose the city grows with the ratio of the two powers
//! (`chance`); the expected loss is chance × stake. Every answer is worth
//! the loss it saves:
//!
//! - free units join a guard as long as the loss they save exceeds their
//!   own worth (their hammers at the production price);
//! - an operation ends when its army would save more at home than the
//!   operation is still worth (`operations::running_value`);
//! - the cities build defenders — the Defend strategy, which production
//!   already reads — while new ones save more than they cost.
//!
//! Everything reads the turn's observation and the memory: units the player
//! sees, its relations, its own cities; nothing hidden.

use std::cmp::Reverse;

use crate::ai::grand_strategy::{prize, score_chance, Permille, Stakes};
use crate::ai::operations::{self, Arsenal};
use crate::ai::planner::{role_of, unit_power, AiRole, Decision, PlanTools, TurnBrief, UnitBrief, UnitView};
use crate::ai::planner_units::{can_stop, hold, move_along, HEAL_BELOW_HP};
use crate::ai::state::{AiState, SeenCity, Strategy, UnitTask};
use crate::ai::trace::{Actor, Reason};
use crate::ai::value::{Centi, Prices};
use crate::command::Command;
use crate::derived::income::unit_upkeep_at;
use crate::ids::{BuildingKindId, EntityId, PlayerId, ProductionItem, TileId, UnitKindId};
use crate::knowledge::observation::{AIObservation, OwnCity};
use crate::knowledge::public::{PlayerQueries, Race};
use crate::rules::content::BuildingKind;
use crate::rules::effects::{CityCombatStat, Modifier, Selector};
use crate::rules::CompiledRuleset;
use crate::world::player::PlayerKind;
use crate::world::victory::VictoryKind;

/// Hostile units this close to an own city threaten it.
pub const THREAT_RADIUS: i32 = crate::ai::planner_strategy::THREAT_RADIUS;
/// Own units this close to a city guard it.
pub const GUARD_RADIUS: i32 = 2;
/// A unit joins a guard it reaches within this many rounds: a ready siege
/// takes a city within the front's `SIEGE_ROUNDS`.
pub const RESPONSE_ROUNDS: u32 = crate::ai::front::SIEGE_ROUNDS;
/// Weight of the field units of a nation at peace that has done
/// nothing to lose our trust, percent of an enemy's: an army gathering at
/// our border for many rounds before its war is the warning (bench probe:
/// 5–20 rounds), a few passing units count for little (`chance`).
pub const STRANGER_PERCENT: i64 = 50;

/// One own city in danger this turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Danger {
    pub city: TileId,
    /// Hostile power near it (strength × health), each owner's by its
    /// hostility.
    pub threat: u32,
    /// The power guarding it now: the city and the own military units
    /// within `GUARD_RADIUS`.
    pub guard: u32,
    /// The power its guard is to have: the city and the units defending it
    /// (`UnitTask::Defend`), there or on the way.
    pub planned: u32,
    /// What losing it costs (hundredths of score points).
    pub stake: Centi,
    /// The owner of the most hostile power near it.
    pub attacker: Option<PlayerId>,
}

impl Danger {
    /// The expected loss with `guard` power guarding the city.
    pub fn loss(&self, guard: u32) -> Centi {
        self.stake * chance(self.threat, guard) / 1000
    }

    /// The loss `power` more on a guard of `guard` saves.
    pub fn saving(&self, guard: u32, power: u32) -> Centi {
        self.loss(guard) - self.loss(guard + power)
    }

    /// How many of `units` (power and worth of each, in the order they
    /// would come) the guard should get: the count whose saving exceeds
    /// their worth the most (0: no count is worth it). One unit saves
    /// little against a threat far above the guard, several may save the
    /// city.
    pub fn best_count(&self, guard: u32, units: &[(u32, Centi)]) -> usize {
        self.best(guard, units).0
    }

    /// The count `best_count` names and the saving beyond the worth it brings.
    pub fn best(&self, guard: u32, units: &[(u32, Centi)]) -> (usize, Centi) {
        let (mut best, mut best_net) = (0, 0);
        let (mut power, mut worth) = (0, 0);
        for (count, (p, w)) in units.iter().enumerate() {
            power += p;
            worth += w;
            let net = self.saving(guard, power) - worth;
            if net > best_net {
                (best, best_net) = (count + 1, net);
            }
        }
        (best, best_net)
    }
}

/// The chance that `threat` takes a city `guard` holds: even at equal
/// power, 94 % at twice the guard's, 6 % at half of it (the fourth power
/// of the ratio). The operations' siege model takes a city within its
/// rounds with about twice the city's power (classic/docs/ai-operations.md: a
/// city of 200 HP heals 20 a round and strikes back); a raid of a few
/// units hardly ever takes a guarded city.
pub fn chance(threat: u32, guard: u32) -> Permille {
    let (t, g) = (i64::from(threat).pow(CHANCE_POWER), i64::from(guard).pow(CHANCE_POWER));
    if t == 0 {
        return 0;
    }
    t * 1000 / (t + g)
}

/// The power of the ratio of threat and guard in `chance`.
pub const CHANCE_POWER: u32 = 4;

/// The cities in danger this turn, the greatest expected loss first.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Watch {
    dangers: Vec<Danger>,
    /// The best melee kind the player trains now (`Arsenal`): the
    /// defender new orders would bring.
    defender: Option<UnitKindId>,
}

impl Watch {
    pub fn dangers(&self) -> &[Danger] {
        &self.dangers
    }

    pub fn danger(&self, city: TileId) -> Option<&Danger> {
        self.dangers.iter().find(|d| d.city == city)
    }
}

/// The dangers of the turn: every own city with hostile power within
/// `THREAT_RADIUS`, valued by the turn's prices (none without them).
pub fn watch(obs: &AIObservation<'_>, brief: &TurnBrief, state: &AiState, rules: &CompiledRuleset) -> Watch {
    let Some(prices) = brief.prices.as_ref() else {
        return Watch::default();
    };
    let hostile = hostile_units(obs, rules);
    if hostile.is_empty() {
        return Watch::default();
    }
    let dims = brief.dims;
    let mut race = None;
    let mut dangers = Vec::new();
    for city in &obs.own_cities {
        let near = |tile: TileId, radius: i32| dims.distance(tile, city.tile).is_some_and(|d| d <= radius);
        let threats: Vec<&Hostile> = hostile.iter().filter(|h| near(h.tile, THREAT_RADIUS)).collect();
        let threat: u32 = threats.iter().map(|h| h.power).sum();
        if threat == 0 {
            continue;
        }
        let own = city_power(brief, rules, city);
        let military = brief.units.iter().filter(|u| u.role == AiRole::Military);
        let guard = own + military.filter(|u| near(u.tile, GUARD_RADIUS)).map(|u| power(rules, u)).sum::<u32>();
        let attacker = strongest(&threats);
        let capital = attacker.map_or(0, |attacker| capital_share(obs, brief, rules, city.tile, attacker, &mut race));
        let stake = operations::city_worth(prices, city.population, 0) + capital;
        dangers.push(Danger { city: city.tile, threat, guard, planned: own + defenders(brief, state, rules, city.tile), stake, attacker });
    }
    dangers.sort_by_key(|d| (Reverse(d.loss(d.guard)), d.city));
    let defender = if dangers.is_empty() { None } else { Arsenal::of(brief, rules).melee };
    Watch { dangers, defender }
}

/// A hostile military unit as the defence weighs it.
struct Hostile {
    tile: TileId,
    owner: PlayerId,
    /// Its power by its owner's hostility.
    power: u32,
}

/// The military units the player sees, by their owners' hostility; at
/// peace only those in the field (a garrison guards its own city).
fn hostile_units(obs: &AIObservation<'_>, rules: &CompiledRuleset) -> Vec<Hostile> {
    let in_city = |tile: TileId| obs.visible_foreign_cities.iter().any(|c| c.tile == tile);
    obs.visible_foreign_units
        .iter()
        .filter(|u| role_of(rules, u.kind) == AiRole::Military)
        .filter_map(|u| {
            let hostility = hostility(obs, u.owner);
            let garrison = hostility < 100 && in_city(u.tile);
            let power = i64::from(unit_power(rules, u.kind, u.hp)) * hostility / 100;
            (power > 0 && !garrison).then_some(Hostile { tile: u.tile, owner: u.owner, power: power as u32 })
        })
        .collect()
}

/// Percent of its units' power a player threatens with: all of it at war
/// and at peace once its recent behaviour lost our trust (a recent war, a
/// denunciation, warmongering: `diplomacy::trusted`); `STRANGER_PERCENT`
/// of another nation's at peace — its army gathers at our border
/// before it declares, a friend's too; none of an ally's (a defensive
/// pact) or a city-state's.
fn hostility(obs: &AIObservation<'_>, owner: PlayerId) -> i64 {
    match obs.relation(owner) {
        Some(r) if r.at_war => 100,
        Some(r) if r.kind != PlayerKind::Major || r.defensive_pact => 0,
        Some(r) if !crate::ai::diplomacy::trusted(obs.round, r) => 100,
        Some(_) => crate::ai::genome::get(crate::ai::genome::Knob::StrangerPercent),
        None => 0,
    }
}

/// The owner of the most hostile power among `threats`.
fn strongest(threats: &[&Hostile]) -> Option<PlayerId> {
    let mut by_owner: Vec<(PlayerId, u32)> = Vec::new();
    for h in threats {
        match by_owner.iter_mut().find(|(owner, _)| *owner == h.owner) {
            Some((_, power)) => *power += h.power,
            None => by_owner.push((h.owner, h.power)),
        }
    }
    by_owner.into_iter().max_by_key(|(owner, power)| (*power, Reverse(*owner))).map(|(owner, _)| owner)
}

fn power(rules: &CompiledRuleset, unit: &UnitBrief) -> u32 {
    unit_power(rules, unit.kind, unit.hp)
}

/// The city's own fighting weight: its strength times its health.
fn city_power(brief: &TurnBrief, rules: &CompiledRuleset, city: &OwnCity) -> u32 {
    (city_strength(brief, rules, city) * i64::from(city.hp) / 10_000).max(0) as u32
}

/// The city's strength in hundredths: as an attacker estimates it
/// (`operations::estimate_city`; no garrison: the guard counts its units)
/// with the defense of its own buildings, which we know.
fn city_strength(brief: &TurnBrief, rules: &CompiledRuleset, city: &OwnCity) -> i64 {
    let seen = SeenCity { tile: city.tile, owner: brief.player, population: city.population, last_seen_round: brief.round };
    let walls: i64 = city.buildings.iter().map(|b| city_defense(rules, *b).0).sum();
    operations::estimate_city(brief, rules, &seen, None).strength + walls
}

/// The strength (hundredths) and hit points a building adds to its own
/// city (`ModifyCityCombat` of the city itself: walls, castles).
pub(crate) fn city_defense(rules: &CompiledRuleset, kind: BuildingKindId) -> (i64, i64) {
    let Some(def) = rules.buildings.get(kind.0 as usize) else {
        return (0, 0);
    };
    let own = def.effects.passive_city.iter().filter(|e| e.selector == Selector::SourceCity);
    own.fold((0, 0), |(strength, hp), e| match e.modifier {
        Modifier::ModifyCityCombat { stat: CityCombatStat::Defense, amount } => (strength + i64::from(amount), hp),
        Modifier::ModifyCityCombat { stat: CityCombatStat::MaxHitPoints, amount } => (strength, hp + i64::from(amount)),
        _ => (strength, hp),
    })
}

/// Power of the units defending `city` (`UnitTask::Defend`).
fn defenders(brief: &TurnBrief, state: &AiState, rules: &CompiledRuleset, city: TileId) -> u32 {
    let task = Some(UnitTask::Defend { city });
    brief.units.iter().filter(|u| state.task_of(u.id).map(|t| t.task) == task).map(|u| power(rules, u)).sum()
}

/// What losing our original capital to `attacker` hands over beyond the
/// city: our chance of the time victory times the prize, over the original
/// capitals the attacker still lacks — all of it when ours is the last one
/// (the attacker wins by domination). Nothing for another city, an
/// attacker that cannot win by domination, or a game without it. The
/// victory screen (`race`, every nation's score) is read once a turn
/// and only for the city with our palace.
fn capital_share(obs: &AIObservation<'_>, brief: &TurnBrief, rules: &CompiledRuleset, city: TileId, attacker: PlayerId, race: &mut Option<Race>) -> Centi {
    let major = obs.relation(attacker).is_some_and(|r| r.kind == PlayerKind::Major);
    if !major || !obs.victories.enabled(VictoryKind::Domination) || !has_palace(obs, rules, city) {
        return 0;
    }
    let race = race.get_or_insert_with(|| obs.view.race());
    let ours = race.capitals.iter().any(|c| c.founder == brief.player && c.holder == brief.player && c.tile == Some(city));
    if !ours {
        return 0;
    }
    let lacking = race.capitals.iter().filter(|c| c.holder != attacker).count().max(1) as i64;
    prize(race, obs) * score_chance(race, obs.round, obs.victories.max_rounds) / 1000 / lacking
}

/// Our city on `tile` holds the palace (our original capital does while
/// we hold it).
fn has_palace(obs: &AIObservation<'_>, rules: &CompiledRuleset, tile: TileId) -> bool {
    let palace = |b: &crate::ids::BuildingKindId| rules.buildings.get(b.0 as usize).is_some_and(|d| d.kind == BuildingKind::Palace);
    obs.own_cities.iter().any(|c| c.tile == tile && c.buildings.iter().any(palace))
}

/// The turn's answers to `watch`: operations whose army saves more at home
/// than they are worth end, free units join the guards they are worth to,
/// and the cities build defenders when one is worth its cost. The planned
/// guards of `watch` count the units sent.
pub fn respond(obs: &AIObservation<'_>, brief: &TurnBrief, state: &mut AiState, rules: &CompiledRuleset, watch: &mut Watch) -> Vec<Decision> {
    let Some(prices) = brief.prices.as_ref() else {
        return Vec::new();
    };
    if watch.dangers.is_empty() {
        return Vec::new();
    }
    let mut out = recall(obs, brief, state, rules, prices, watch);
    let shares = if brief.war.siege.assault {
        let stakes = Stakes::of(obs, brief, state, &rules.module.personality(brief.nation));
        release(brief, state, rules, prices, &stakes, watch);
        operations::task_shares(brief, state, rules, prices, &stakes)
    } else {
        Vec::new()
    };
    assign(brief, state, rules, prices, watch, &shares);
    out.extend(arm(brief, state, rules, watch));
    out
}

/// End every operation whose members would save more at a city in danger
/// than the operation is still worth; its members are free again.
fn recall(obs: &AIObservation<'_>, brief: &TurnBrief, state: &mut AiState, rules: &CompiledRuleset, prices: &Prices, watch: &Watch) -> Vec<Decision> {
    let running = state.operations().to_vec();
    if running.is_empty() {
        return Vec::new();
    }
    let stakes = Stakes::of(obs, brief, state, &rules.module.personality(brief.nation));
    let mut out = Vec::new();
    for operation in running {
        let army: u32 = operations::members(brief, state, operation.target).iter().map(|u| power(rules, u)).sum();
        let value = operations::running_value(brief, state, rules, prices, &stakes, &operation).unwrap_or(0);
        let saved = if brief.war.recall {
            recall_saving(brief, state, rules, prices, watch, &operation)
        } else {
            watch.dangers.iter().map(|d| d.saving(d.planned, army)).max().unwrap_or(0)
        };
        if saved > value.max(0) {
            crate::ai::war_probe::note(|p| {
                let marching = operation.phase == crate::ai::state::OperationPhase::Marching;
                let age = brief.round.saturating_sub(operation.phase_round);
                let best = watch.dangers.iter().max_by_key(|d| d.saving(d.planned, army));
                let mine = operations::members(brief, state, operation.target);
                let nearest = best.and_then(|d| mine.iter().filter_map(|u| brief.dims.distance(u.tile, d.city)).min()).unwrap_or(-1);
                let start = p.starts.iter().rev().find(|s| s.player == brief.player && s.target == operation.target.0 && s.round <= operation.since_round);
                let case = operations::running_case(brief, state, rules, prices, &stakes, operation.enemy).map_or(0, |c| c.value(&stakes));
                let attacker = match best.and_then(|d| d.attacker) {
                    Some(a) if a == operation.enemy && brief.at_war(a) => 0,
                    Some(a) if brief.at_war(a) => 1,
                    _ => 2,
                };
                let (gain, cost, missing, need) = operations::running_detail(brief, state, rules, prices, &stakes, &operation, false).map_or((0, 0, 0, 0), |(c, m, n)| (c.gain, c.cost, m, n));
                let value_expected = operations::running_detail(brief, state, rules, prices, &stakes, &operation, true).map_or(0, |(c, _, _)| c.value(&stakes) + operations::infamy_bonus(brief, operation.enemy));
                let net_saved = recall_saving(brief, state, rules, prices, watch, &operation);
                p.recalls.push(crate::ai::war_probe::Recall {
                    value_expected,
                    net_saved,
                    gain,
                    cost,
                    missing,
                    need,
                    start_missing: start.map_or(0, |s| s.missing),
                    start_need: start.map_or(0, |s| s.need),
                    start_free: start.map_or(0, |s| s.free),
                    round: brief.round,
                    player: brief.player.0,
                    target: operation.target.0,
                    value,
                    value_case: case,
                    start_value: start.map(|s| s.value),
                    saved,
                    age,
                    life: brief.round.saturating_sub(operation.since_round),
                    marching,
                    members: mine.len() as u32,
                    army_power: army,
                    threat: best.map_or(0, |d| d.threat),
                    guard: best.map_or(0, |d| d.guard),
                    planned: best.map_or(0, |d| d.planned),
                    stake: best.map_or(0, |d| d.stake),
                    attacker,
                    at_muster: best.is_some_and(|d| d.city == operation.muster),
                    nearest_member: nearest,
                    reaches: nearest >= 0 && nearest <= RESPONSE_ROUNDS as i32 * 2,
                });
                p.ends.push(crate::ai::war_probe::OperationEnd { round: brief.round, player: brief.player, enemy: operation.enemy, cause: crate::ai::war_probe::EndCause::Recalled, marching, age, target: operation.target.0 });
            });
            state.end_operation(operation.target);
            out.push(Decision::refuse(Actor::Player, Reason::EndOperation));
        }
    }
    out
}

/// What the members of `operation` would save at the most at a city in danger, as `assign` would
/// send them: only the units that reach the city within `RESPONSE_ROUNDS`, the nearest first, as
/// many as save the most beyond their worth (`Danger::best`). A recall that saves less than that
/// frees units the guards would not take (`WarTuning::recall`).
fn recall_saving(brief: &TurnBrief, state: &AiState, rules: &CompiledRuleset, prices: &Prices, watch: &Watch, operation: &crate::ai::state::Operation) -> Centi {
    let per_hammer = hammer(prices);
    let members = operations::members(brief, state, operation.target);
    watch
        .dangers
        .iter()
        .map(|danger| {
            let mut near: Vec<(i32, &UnitBrief)> = members
                .iter()
                .filter_map(|u| brief.dims.distance(u.tile, danger.city).map(|d| (d, *u)))
                .filter(|(d, u)| *d <= reach(rules, u))
                .collect();
            near.sort_by_key(|(d, u)| (*d, u.id));
            let offer: Vec<(u32, Centi)> = near.iter().map(|(_, u)| (power(rules, u), i64::from(rules.units[u.kind.0 as usize].cost) * per_hammer)).collect();
            danger.best(danger.planned, &offer).1
        })
        .max()
        .unwrap_or(0)
}

/// The other way of the same rule (`SiegeFixes::assault`): a guard of a city
/// in danger beyond the garrison its city wants goes to the war's assault
/// when its share of the assault's target (`operations::assault_share`, as
/// one more unit on it) beats the loss it saves at the city
/// (`Danger::saving`), the youngest guards first. A guard sent by `assign`
/// came for a saving above its hammers and its share, so it does not swing
/// back and forth.
fn release(brief: &TurnBrief, state: &mut AiState, rules: &CompiledRuleset, prices: &Prices, stakes: &Stakes, watch: &mut Watch) {
    let Some(target) = operations::assault_target(state) else { return };
    let standing = crate::ai::planner_units::garrison_wanted_by(brief, state.strategy.current).max(1);
    for danger in &mut watch.dangers {
        let mut guards: Vec<&UnitBrief> = brief
            .units
            .iter()
            .filter(|u| state.task_of(u.id).map(|t| t.task) == Some(UnitTask::Defend { city: danger.city }))
            .filter(|u| u.hp >= HEAL_BELOW_HP && operations::army_role(rules, u.kind).is_some())
            .collect();
        guards.sort_by_key(|u| u.id);
        let mut leaving = Vec::new();
        for unit in guards.into_iter().skip(standing).rev() {
            let own = power(rules, unit);
            let saved = danger.saving(danger.planned.saturating_sub(own), own);
            if operations::assault_share(brief, state, rules, prices, stakes, target, unit) > saved {
                danger.planned = danger.planned.saturating_sub(own);
                leaving.push(unit.id);
            }
        }
        for unit in leaving {
            state.set_task(unit, UnitTask::Assault { target }, brief.round);
        }
    }
}

/// Units that may join a guard — the free ones (`operations::free_units`)
/// and the garrisons of cities not in danger beyond the first, healthy army
/// units that arrive within `RESPONSE_ROUNDS` — join the guards, the
/// greatest danger first, the nearest units first, as many as save the
/// most beyond their worth (`Danger::best_count`; a garrison taken is built
/// again). With `SiegeFixes::assault` a unit on an assault is worth its
/// hammers and its share of its target (`shares`, `operations::task_shares`):
/// it joins a guard only when the saving beats both.
fn assign(brief: &TurnBrief, state: &mut AiState, rules: &CompiledRuleset, prices: &Prices, watch: &mut Watch, shares: &[(EntityId, Centi)]) {
    let per_hammer = hammer(prices);
    let share = |u: &UnitBrief| shares.iter().find(|(id, _)| *id == u.id).map_or(0, |(_, s)| *s);
    let mut free = available(brief, state, rules, watch);
    for danger in &mut watch.dangers {
        let mut candidates: Vec<(i32, UnitBrief)> = free
            .iter()
            .filter_map(|u| brief.dims.distance(u.tile, danger.city).map(|d| (d, *u)))
            .filter(|(d, u)| *d <= reach(rules, u))
            .collect();
        candidates.sort_by_key(|(d, u)| (*d, u.id));
        let worth = |u: &UnitBrief| i64::from(rules.units[u.kind.0 as usize].cost) * per_hammer + share(u);
        let offer: Vec<(u32, Centi)> = candidates.iter().map(|(_, u)| (power(rules, u), worth(u))).collect();
        let count = danger.best_count(danger.planned, &offer);
        for (_, unit) in candidates.into_iter().take(count) {
            state.set_task(unit.id, UnitTask::Defend { city: danger.city }, brief.round);
            danger.planned += power(rules, &unit);
            free.retain(|u| u.id != unit.id);
        }
    }
}

/// The free units and the garrisons of the cities not in danger but the
/// first of each (an empty city invites the next raid, which no threat
/// seen now prices); with `SiegeFixes::assault` the units on an assault too,
/// which are not free (`assign` weighs their share of their target).
fn available(brief: &TurnBrief, state: &AiState, rules: &CompiledRuleset, watch: &Watch) -> Vec<UnitBrief> {
    let task = |u: &UnitBrief| state.task_of(u.id).map(|t| t.task);
    let senior = |city: TileId, unit: EntityId| !state.tasks().iter().any(|t| t.task == UnitTask::Defend { city } && t.unit < unit);
    let guard_elsewhere = |u: &UnitBrief| match task(u) {
        Some(UnitTask::Defend { city }) => watch.danger(city).is_none() && !senior(city, u.id),
        _ => false,
    };
    let fit = |u: &UnitBrief| u.hp >= HEAL_BELOW_HP && operations::army_role(rules, u.kind).is_some();
    let on_assault = |u: &UnitBrief| brief.war.siege.assault && matches!(task(u), Some(UnitTask::Assault { .. }));
    let free: Vec<EntityId> = operations::free_units(brief, state, rules).map(|u| u.id).collect();
    brief
        .units
        .iter()
        .filter(|u| fit(u) && (guard_elsewhere(u) || free.contains(&u.id) && !matches!(task(u), Some(UnitTask::Defend { .. })) || on_assault(u)))
        .copied()
        .collect()
}

/// Tiles a unit walks in `RESPONSE_ROUNDS`.
fn reach(rules: &CompiledRuleset, unit: &UnitBrief) -> i32 {
    let movement = rules.units.get(unit.kind.0 as usize).map_or(1, |d| d.movement.max(1));
    i32::from(movement) * RESPONSE_ROUNDS as i32
}

/// The Defend strategy (production builds the army before the allocator's
/// choice, `planner_cities`) while new units of the best melee kind would
/// save more at a city in danger than they cost — their hammers at the
/// production price and their upkeep to the end — beyond the guards sent
/// and the military units already queued (`Danger::best_count`, up to
/// twice the threat). Called at the start of the turn and again after the
/// strategy review, which knows nothing of it.
pub fn arm(brief: &TurnBrief, state: &mut AiState, rules: &CompiledRuleset, watch: &Watch) -> Option<Decision> {
    if state.strategy.current == Strategy::Defend {
        return None;
    }
    if !watch.dangers.iter().any(|d| defender_net(brief, rules, watch, d).is_some()) {
        return None;
    }
    state.strategy.current = Strategy::Defend;
    state.strategy.since_round = brief.round;
    Some(Decision::refuse(Actor::Player, Reason::StrategyChanged))
}

/// What one new defender of the best melee kind is worth at `d` net of its
/// cost — its hammers at the production price and its upkeep to the end —
/// beyond the guards sent and the military units already queued: the
/// count of them that saves the most beyond their cost (up to twice the
/// threat, `Danger::best_count`), per unit. None: no count pays.
pub fn defender_net(brief: &TurnBrief, rules: &CompiledRuleset, watch: &Watch, d: &Danger) -> Option<Centi> {
    let prices = brief.prices.as_ref()?;
    let kind = watch.defender?;
    let units = brief.units.len();
    let upkeep = i64::from(unit_upkeep_at(units + 1, brief.round, brief.speed.max_rounds()) - unit_upkeep_at(units, brief.round, brief.speed.max_rounds())) * prices.gold;
    let cost = i64::from(rules.units[kind.0 as usize].cost) * hammer(prices) + upkeep;
    let power = unit_power(rules, kind, crate::world::unit::UNIT_MAX_HP).max(1);
    let guard = d.planned + queued_power(brief, rules);
    let enough = (2 * d.threat).div_ceil(power).max(1) as usize;
    let count = d.best_count(guard, &vec![(power, cost); enough]);
    (count > 0).then(|| (d.saving(guard, power * count as u32) - cost * count as i64) / count as i64)
}

/// What a building is worth to `city` in danger `d` net of its cost: the
/// loss its city defense (strength and hit points, `city_defense`) saves
/// against this threat, beyond the guards sent and the military units
/// queued, less its hammers at the production price and its upkeep to the
/// end. None: it adds no defense or does not pay.
pub fn building_net(brief: &TurnBrief, rules: &CompiledRuleset, d: &Danger, city: &OwnCity, kind: BuildingKindId) -> Option<Centi> {
    let prices = brief.prices.as_ref()?;
    let (strength, hp) = city_defense(rules, kind);
    if strength <= 0 && hp <= 0 {
        return None;
    }
    let now = city_strength(brief, rules, city);
    let hp_now = i64::from(city.hp);
    let extra = ((now + strength) * (hp_now + hp) - now * hp_now) / 10_000;
    let def = &rules.buildings[kind.0 as usize];
    let cost = i64::from(def.cost) * hammer(prices) + i64::from(def.maintenance) * prices.gold;
    let net = d.saving(d.planned + queued_power(brief, rules), extra.max(0) as u32) - cost;
    (net > 0).then_some(net)
}

/// The value of a hammer: the production price over the rounds left.
fn hammer(prices: &Prices) -> Centi {
    prices.production / i64::from(prices.rounds_left.max(1))
}

/// One step of a guard of a city in danger (`UnitTask::Defend` of a city
/// in the watch): into the city while its tile is free, else onto the
/// strongest ground next to it that the unit reaches this turn (the rules'
/// defense of the tile: hills, forest), fewest enemies next to it first;
/// from farther, toward the city.
pub fn guard_step(view: &UnitView<'_, '_>, tools: &mut PlanTools<'_>, city: TileId) -> Decision {
    let unit = view.local.unit;
    let dims = view.local.view.dimensions();
    let distance = |a: TileId, b: TileId| dims.distance(a, b).unwrap_or(i32::MAX);
    if unit.tile == city {
        return hold(view, Reason::Garrison);
    }
    if can_stop(view, tools.rules, city) || distance(unit.tile, city) > reach_this_turn(tools.rules, unit.kind) {
        return move_along(view, tools, city, Reason::Garrison);
    }
    let rules = tools.rules;
    let foes_next_to = |tile: TileId| view.local.foreign_units.iter().filter(|u| hostile(view, u.owner) && dims.adjacent(u.tile, tile)).count();
    let best = view
        .local
        .view
        .own_reachable(unit.handle)
        .into_iter()
        .map(|(tile, _)| tile)
        .filter(|t| distance(*t, city) == 1 && (*t == unit.tile || can_stop(view, rules, *t)))
        .max_by_key(|t| (ground_defense(view, rules, *t), Reverse(foes_next_to(*t)), *t == unit.tile, Reverse(*t)));
    match best {
        Some(tile) if tile != unit.tile => Decision::act(Actor::Unit(unit.id), Reason::Garrison, Command::MoveUnit { unit: unit.handle, destination: tile }),
        Some(_) => hold(view, Reason::Garrison),
        None => move_along(view, tools, city, Reason::Garrison),
    }
}

/// A hurt guard of `city`, in danger, leaves its post to heal
/// (`FightFix::retreat`): when the death it risks where it stands, beyond
/// the death it risks at the safest tile it reaches (`combat::Exposure`),
/// costs more than the loss it saves at the city (`Danger::saving` of its
/// power) — its death counted at its worth as `assign` counts it, its
/// hammers at the production price.
pub fn guard_released(view: &UnitView<'_, '_>, city: TileId) -> bool {
    let unit = view.local.unit;
    if unit.hp >= crate::world::unit::UNIT_MAX_HP {
        return false;
    }
    let brief = view.brief;
    let (Some(watch), Some(prices)) = (brief.defence.as_ref(), brief.prices.as_ref()) else { return false };
    let Some(danger) = watch.danger(city) else { return false };
    let Some(threats) = crate::ai::combat::unit_threats(view) else { return false };
    let rules = view.local.view.rules();
    let exposure = crate::ai::combat::Exposure::of(&threats, rules, brief.fight.source, &unit);
    let here = exposure.danger(unit.tile, unit.hp, false, &[]).death_permille;
    let stops = crate::ai::combat::unit_stops(view);
    let Some((safe, _)) = exposure.safest(&stops, |_| 0) else { return false };
    let there = exposure.danger(safe, unit.hp, false, &[]).death_permille;
    let own = unit_power(rules, unit.kind, unit.hp);
    let saved = danger.saving(danger.planned.saturating_sub(own), own);
    let worth = i64::from(rules.units[unit.kind.0 as usize].cost) * hammer(prices);
    i64::from(here.saturating_sub(there)) * worth / 1000 > saved
}

/// Tiles a unit of `kind` walks in one round.
fn reach_this_turn(rules: &CompiledRuleset, kind: crate::ids::UnitKindId) -> i32 {
    i32::from(rules.units.get(kind.0 as usize).map_or(1, |d| d.movement.max(1)))
}

fn hostile(view: &UnitView<'_, '_>, owner: PlayerId) -> bool {
    view.local.relation(owner).is_some_and(|r| r.at_war)
}

/// The defense the ground of a known tile gives (`combat::terrain`).
fn ground_defense(view: &UnitView<'_, '_>, rules: &CompiledRuleset, tile: TileId) -> i64 {
    let Some(snapshot) = view.local.view.tile(tile).snapshot().copied() else {
        return 0;
    };
    let kinds = view.local.view.terrain_kinds();
    let high = kinds.is_hills(snapshot.terrain.0) || kinds.is_mountains(snapshot.terrain.0);
    crate::combat::terrain::ground_defense_percent(rules, high, snapshot.feature)
}

/// Power of the military units the cities' queues hold: they join the
/// guards once built.
fn queued_power(brief: &TurnBrief, rules: &CompiledRuleset) -> u32 {
    let units = brief.cities.iter().flat_map(|c| &c.queue).filter_map(|item| match item {
        ProductionItem::Unit(kind) if role_of(rules, *kind) == AiRole::Military => Some(*kind),
        _ => None,
    });
    units.map(|kind| unit_power(rules, kind, crate::world::unit::UNIT_MAX_HP)).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Even powers: an even chance; twice the guard's: 94 %; half: 6 %; no
    /// threat: none.
    #[test]
    fn the_chance_follows_the_ratio_of_the_powers() {
        assert_eq!(chance(10, 10), 500);
        assert_eq!(chance(20, 10), 941);
        assert_eq!(chance(10, 20), 58);
        assert_eq!(chance(0, 10), 0);
        assert_eq!(chance(10, 0), 1000);
    }

    /// Near an even fight one more defender saves less, the stronger the
    /// guard already is.
    #[test]
    fn a_guard_saves_less_and_less() {
        let danger = Danger { city: TileId(0), threat: 20, guard: 20, planned: 20, stake: 10_000, attacker: None };
        assert_eq!(danger.loss(20), 5_000);
        let first = danger.saving(20, 6);
        let second = danger.saving(26, 6);
        assert!(first > second && second > 0, "{first} {second}");
    }

    /// Against a threat far above the guard one unit is not worth its cost,
    /// six together are (a seventh saves less than it costs); none once the
    /// guard holds.
    #[test]
    fn several_guards_may_be_worth_what_one_is_not() {
        let danger = Danger { city: TileId(0), threat: 60, guard: 20, planned: 20, stake: 10_000, attacker: None };
        let unit = (10, 1_000);
        assert!(danger.saving(20, 10) < 1_000);
        assert_eq!(danger.best_count(20, &[unit]), 0);
        assert_eq!(danger.best_count(20, &[unit; 10]), 6);
        assert_eq!(danger.best_count(120, &[unit; 6]), 0);
    }

    /// A recall counts the saving beyond the worth of the units, as the guards that take them
    /// do: a small threat saves less than six units cost, so nothing is worth recalling for.
    #[test]
    fn the_net_saving_of_a_recall_is_what_the_guards_would_take() {
        let small = Danger { city: TileId(0), threat: 20, guard: 50, planned: 50, stake: 11_000, attacker: None };
        let army = [(10, 1_000); 5];
        assert!(small.saving(50, 50) > 0, "the gross saving is positive: {}", small.saving(50, 50));
        assert_eq!(small.best(50, &army), (0, 0), "but no count of units pays for itself");
        let big = Danger { city: TileId(0), threat: 60, guard: 20, planned: 20, stake: 10_000, attacker: None };
        let (count, net) = big.best(20, &[(10, 1_000); 10]);
        assert_eq!(count, 6);
        assert!(net > 0);
    }
}
