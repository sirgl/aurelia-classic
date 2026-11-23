//! The retake of a lost city (`Variant::Retake2`, `TurnBrief::retake`; the `retake` lane, classic/docs/ai-retake.md).
//!
//! Right after a capture a city is at its weakest: its health is what the siege left, its walls and
//! garrison are not yet the new owner's, and the units that took it stand around it hurt. The former owner
//! sees all of this when its units are near (the city's health bar, the units in sight). Without this
//! module the lost city is an ordinary target estimated at full health and handled by the operations (a war
//! already running starts none: `operations::odds_are_good`), the units next to it keep their garrison
//! tasks, and the war ends by the usual peace.
//!
//! A **case** is a city the seat held at its last turn (its own memory, `Memory`), that the player it
//! lost it to holds at war, in sight, within the window, which the seat's units within two turns of it
//! bring to capture within `RETAKE_ROUNDS` rounds of strikes by the siege model
//! (`front::siege_rounds_within`) with the city's real health, against what the captor has in sight
//! around it (`cases_remembered`). The units of the case take part in the assault at once
//! (`planner_units::ensure_task`, no gathering: `tactics::siege_step`), and while a case runs the
//! seat's strategy makes no peace with its holder (`planner_strategy::consider_peace`,
//! `operations::idle_wars`, the diplomacy advisor).
//!
//! The memory is in the strategy memory of `AiState::mind_memory` (a versioned section, see `Memory`).

use crate::ai::front::{siege_rounds_within, Besieger};
use crate::combat::damage::{city_strike_damage, Roll};
use crate::ai::strategy_net;
use crate::ai::grand_strategy::Stakes;
use crate::ai::operations::{self, army_role, estimate_city, estimated_strike};
use crate::ai::planner::{unit_power, TurnBrief};
use crate::ai::planner_units::HEAL_BELOW_HP;
use crate::ai::state::{AiState, SeenCity, UnitTask};
use crate::ids::{EntityId, PlayerId, TileId};
use crate::knowledge::observation::AIObservation;
use crate::knowledge::public::PlayerQueries;
use crate::rules::CompiledRuleset;

/// Rounds of strikes the units on the spot may take to bring the city to capture: this turn's and the next.
/// A longer siege is the operations' (the city heals back, the captor's reinforcements arrive: the
/// operations of a war already running took a city in 3 % of the cases, `operations::odds_are_good`).
pub const RETAKE_ROUNDS: u32 = 2;

/// A unit takes part when it strikes within this many of its own turns of walking (its movement a turn,
/// then its range): the model counts `RETAKE_ROUNDS` rounds of strikes, and a unit that needs more turns
/// to arrive strikes after the city healed or was reinforced.
const TURNS: i32 = 2;

/// A lost city the units on the spot can retake.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Case {
    pub tile: TileId,
    pub enemy: PlayerId,
    /// The city's health as seen.
    pub hp: u16,
    /// Rounds of strikes the siege model needs.
    pub rounds: u32,
    /// The units that strike.
    pub group: Vec<EntityId>,
}

/// The unit may leave its task for the retake. Free: a unit with no task; a unit that rested (it is fit by
/// the caller's hit point test); a guard of a city that is no longer ours (the lost city's own garrison); a
/// guard of a city that has more guards than it wants and that stays safe without it (`kept_guards`: the
/// defence has not made its dangers yet when the retake looks); a unit on the assault of a lost
/// city (the retake's own task of the turn before). Not free: a member of an operation or a raid, a unit on
/// the assault of a city that is not a lost one, an escort of a civilian unit (its ward would walk alone),
/// and a unit whose task is no war task (scout, settle, improve).
fn free_for_retake(brief: &TurnBrief, state: &AiState, unit: EntityId, lost: &[TileId], kept: &[EntityId]) -> bool {
    match state.task_of(unit).map(|t| t.task) {
        // A unit that rests is fit once it stands above `HEAL_BELOW_HP` (the caller tests it): its task is done.
        None | Some(UnitTask::Heal { .. }) => true,
        // The assault of a lost city is the retake's own task of the turn before (`planner_units::ensure_task`).
        Some(UnitTask::Assault { target }) => lost.contains(&target),
        Some(UnitTask::Defend { city }) => {
            !brief.cities.iter().any(|c| c.tile == city)
                || (!kept.contains(&unit) && surplus_guard(brief, state, city))
        }
        Some(
            UnitTask::Operation { .. }
            | UnitTask::Raid { .. }
            | UnitTask::Escort { .. }
            | UnitTask::Scout { .. }
            | UnitTask::Settle { .. }
            | UnitTask::Improve { .. },
        ) => false,
    }
}

/// The city has more guards on its defence than it wants.
fn surplus_guard(brief: &TurnBrief, state: &AiState, city: TileId) -> bool {
    state.tasks().iter().filter(|t| t.task == UnitTask::Defend { city }).count() > crate::ai::planner_units::garrison_wanted_at(brief, state.strategy.current, city).max(1)
}

/// The chance of the threat to take the city (`defence::chance`, per mille) from which a surplus guard stays on it:
/// one half. `chance` weighs the fourth powers of threat and guard, so it is exactly one half where the guard left
/// equals the threat: a guard leaves for the retake only while the guard it leaves behind still outweighs the hostile
/// power near the city. Not tuned: the point where the defence's own model calls the city as likely lost as held.
const GUARD_KEPT_CHANCE: crate::ai::grand_strategy::Permille = 500;

/// The surplus guards that stay on their city this turn: those of a city with hostile power near it (the
/// defence's own reading, `defence::threats`) whose guard without them would not outweigh the threat. A
/// surplus guard goes only while the guard left is above the threat (`defence::chance` under one half);
/// the units are taken in the order of the brief, each release lowers the guard. The second list is every
/// surplus guard of a threatened city, kept or not (the count of the defect: what 5c15543 took).
fn kept_guards(observation: &AIObservation<'_>, brief: &TurnBrief, state: &AiState, rules: &CompiledRuleset) -> (Vec<EntityId>, usize) {
    let mut threats = crate::ai::defence::threats(observation, brief, rules);
    let (mut kept, mut threatened) = (Vec::new(), 0);
    if threats.is_empty() {
        return (kept, threatened);
    }
    for unit in brief.units.iter().filter(|u| army_role(rules, u.kind).is_some() && u.hp >= HEAL_BELOW_HP) {
        let Some(UnitTask::Defend { city }) = state.task_of(unit.id).map(|t| t.task) else { continue };
        let Some(t) = threats.iter_mut().find(|t| t.city == city) else { continue };
        if !surplus_guard(brief, state, city) {
            continue;
        }
        threatened += 1;
        let left = t.guard.saturating_sub(crate::ai::defence::guard_power(rules, unit));
        if crate::ai::defence::chance(t.threat, left) >= GUARD_KEPT_CHANCE {
            kept.push(unit.id);
        } else {
            t.guard = left;
        }
    }
    (kept, threatened)
}

impl TurnBrief {
    /// The city the unit retakes this turn.
    pub fn retake_target_of(&self, unit: EntityId) -> Option<TileId> {
        self.retaking.iter().find(|c| c.group.contains(&unit)).map(|c| c.tile)
    }

    /// `tile` is a city being retaken this turn.
    pub fn is_retaken(&self, tile: TileId) -> bool {
        self.retaking.iter().any(|c| c.tile == tile)
    }

    /// A retake against `enemy` runs: no peace with it.
    pub fn retaking_from(&self, enemy: PlayerId) -> bool {
        self.retaking.iter().any(|c| c.enemy == enemy)
    }
}


// ---- retake2: the seat finds the lost city by its own memory, and weighs the guard by what it can see ----

/// The captor's units this close to the city, in sight, answer the assault (a unit four tiles away is
/// two turns from the walls for most of the kinds, and the assault lasts `RETAKE_ROUNDS` rounds).
/// The review of the first iteration: with 14 or more captor units within four tiles a retake cost
/// 7 units per extra city retaken, while the test counted the ones within two tiles only.
const GUARD_RADIUS_NEAR: i32 = 4;

/// The group is not outmatched when its power (after the city's strikes while the assault lasts) is at
/// least this percent of the power of the captor's units in sight within `GUARD_RADIUS_NEAR`. Fitted on
/// the tuning positions (offsets 200-206, 355 positions, 710 plays; classic/docs/ai-retake.md "Fit"):
/// the smallest margin (the most retakes) with own units lost at most 1.0 above `war-pays` per position
/// and failures that lose more than 5 units at most 2 points above it, a third under the gates of the
/// lane (1.5 and 3 points). Margins 100 / 150 / 175 / 200 gave +1.48 / +1.20 / +0.98 / +0.86 units.
const GUARD_MARGIN_PERCENT: u64 = 175;

/// The margin of a retake against `captor` under `net2`'s war order (design §4.4): with a `Retake` order on
/// it the margin falls from `GUARD_MARGIN_PERCENT` to `RETAKE_ORDER_MARGIN_PERCENT` as the order's value
/// rises from the value that starts an order (550) to 1000, a capital in the captor's hands (retake 1200 · R19
/// in `retake_r`). Without such an order the margin is the fitted one.
fn margin_percent(brief: &TurnBrief, captor: PlayerId) -> u64 {
    crate::ai::operations::with_net2(brief, crate::ai::net2::plan::Part::War, |plan| plan.war_on(captor).filter(|o| o.kind == crate::ai::net2::plan::WarKind::Retake).map(|o| o.value))
        .flatten()
        .map_or(GUARD_MARGIN_PERCENT, order_margin)
}

/// The value at which a war order starts (design §4.4).
const ORDER_START_VALUE: i64 = 550;
/// The margin at an order value of 1000: the fitted margins 100 / 150 / 175 cost +1.48 / +1.20 / +0.98 units
/// a retake (above); 125 is the cost of about +1.35, accepted for a capital, not for a city (the order's value
/// reaches 1000 only with R19, or with R20 and a local force).
const RETAKE_ORDER_MARGIN_PERCENT: u64 = 125;

/// The margin for an order of `value` (thousandths): linear between the two margins.
pub fn order_margin(value: i64) -> u64 {
    let t = (value - ORDER_START_VALUE).clamp(0, 1000 - ORDER_START_VALUE) as u64;
    let span = 1000 - ORDER_START_VALUE as u64;
    GUARD_MARGIN_PERCENT - (GUARD_MARGIN_PERCENT - RETAKE_ORDER_MARGIN_PERCENT) * t / span
}

/// The strategy memory of `state` as the retake reads it: its part of `AiState::mind_memory`. The bytes may
/// hold the commander's memory beside it (`unit_net::memory`, an envelope of the two); the retake decoded the
/// whole bytes, took the envelope for a spoiled memory and wiped the strategy net's and the commander's
/// memory every turn (tactical design §6.2, lane B's `Memory::of`; this is its local stand-in until part B
/// lands `strategy_net::Memory::of` and `store`, then the two bodies are one line each).
fn strategy_memory(state: &AiState) -> strategy_net::Memory {
    #[cfg(feature = "experimental-ai")]
    let (strategy, _) = crate::ai::unit_net::memory::split(&state.mind_memory);
    #[cfg(not(feature = "experimental-ai"))]
    let strategy = state.mind_memory.clone();
    strategy_net::Memory::decode(&strategy)
}

/// Writes `memory` back, the commander's part kept.
fn store_strategy_memory(memory: &strategy_net::Memory, state: &mut AiState) {
    #[cfg(feature = "experimental-ai")]
    {
        let (_, unit) = crate::ai::unit_net::memory::split(&state.mind_memory);
        state.mind_memory = crate::ai::unit_net::memory::join(&memory.encode(), &unit);
    }
    #[cfg(not(feature = "experimental-ai"))]
    {
        state.mind_memory = memory.encode();
    }
}

/// The cities the seat lost in the window and still counts as lost (its own memory), for the war order of a
/// retake (`operations::targets_for`).
pub fn lost_tiles(state: &AiState) -> Vec<TileId> {
    strategy_memory(state).retake.map(|m| m.lost.iter().map(|l| l.tile).collect()).unwrap_or_default()
}

/// Most cities the memory keeps as lost (the oldest go first): a seat that loses more than this in a
/// window has no group left to retake any of them.
const LOST_KEPT: usize = 8;

/// A city of the seat's that another player took.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Lost {
    pub tile: TileId,
    /// The round the seat found it gone (its next turn after the capture).
    pub round: u32,
    /// The player that holds it: read from the city when the seat sees it; None until then.
    pub captor: Option<PlayerId>,
}

/// What the seat remembers for the retake (`retake2`): its cities at its last turn, and the ones it lost
/// since. It always knows when a city of its own is gone; who took it, and the city's health and
/// garrison, it reads while the city is in sight, as a human player does.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Memory {
    pub held: Vec<TileId>,
    pub lost: Vec<Lost>,
}

/// The version of the section's body (its first byte).
const MEMORY_VERSION: u8 = 1;

impl Memory {
    /// Bytes: version, lost count (1), each lost city (tile 4, round 4, captor 1: 255 none), held count (2), tiles (4 each).
    pub fn encode(&self) -> Vec<u8> {
        let mut out = vec![MEMORY_VERSION, self.lost.len().min(LOST_KEPT) as u8];
        for l in self.lost.iter().take(LOST_KEPT) {
            out.extend_from_slice(&l.tile.0.to_le_bytes());
            out.extend_from_slice(&l.round.to_le_bytes());
            out.push(l.captor.map_or(u8::MAX, |p| p.0));
        }
        out.extend_from_slice(&(self.held.len().min(usize::from(u16::MAX)) as u16).to_le_bytes());
        for t in self.held.iter().take(usize::from(u16::MAX)) {
            out.extend_from_slice(&t.0.to_le_bytes());
        }
        out
    }

    /// The memory of a section's body; None for a version or a length it does not know (the seat then starts afresh).
    pub fn decode(bytes: &[u8]) -> Option<Memory> {
        let (&version, mut rest) = bytes.split_first()?;
        if version != MEMORY_VERSION {
            return None;
        }
        let mut take = |n: usize| -> Option<&[u8]> {
            let (head, tail) = rest.split_at_checked(n)?;
            rest = tail;
            Some(head)
        };
        let n = usize::from(take(1)?[0]);
        let mut lost = Vec::with_capacity(n.min(LOST_KEPT));
        for _ in 0..n {
            let b = take(9)?;
            let captor = (b[8] != u8::MAX).then_some(PlayerId(b[8]));
            lost.push(Lost { tile: TileId(u32::from_le_bytes(b[0..4].try_into().ok()?)), round: u32::from_le_bytes(b[4..8].try_into().ok()?), captor });
        }
        let h = usize::from(u16::from_le_bytes(take(2)?.try_into().ok()?));
        let mut held = Vec::with_capacity(h.min(256));
        for _ in 0..h {
            held.push(TileId(u32::from_le_bytes(take(4)?.try_into().ok()?)));
        }
        Some(Memory { held, lost })
    }

    /// The seat's turn: `own` its cities now, `seen` the cities of others in sight (tile, holder), `window` the
    /// rounds a lost city stays a case. A city held at the last turn and not now was taken; it stays while
    /// the window lasts, the seat does not hold it again, and no third player has taken it from its captor
    /// (a city that changed hands again is not the one the seat lost to this enemy, and is no case).
    pub fn observe(&mut self, now: u32, window: u32, own: &[TileId], seen: &[(TileId, PlayerId)]) {
        let holder = |tile: TileId| seen.iter().find(|(t, _)| *t == tile).map(|(_, p)| *p);
        for tile in &self.held {
            if !own.contains(tile) && !self.lost.iter().any(|l| l.tile == *tile) {
                self.lost.push(Lost { tile: *tile, round: now, captor: holder(*tile) });
            }
        }
        self.lost.retain_mut(|l| {
            if own.contains(&l.tile) || now.saturating_sub(l.round) > window {
                return false;
            }
            match (holder(l.tile), l.captor) {
                (Some(now_holder), None) => {
                    l.captor = Some(now_holder);
                    true
                }
                (Some(now_holder), Some(captor)) => now_holder == captor,
                (None, _) => true,
            }
        });
        if self.lost.len() > LOST_KEPT {
            self.lost.drain(..self.lost.len() - LOST_KEPT);
        }
        self.held = own.to_vec();
        self.held.sort();
    }
}

/// The group is outmatched: its power after the strikes the city makes while the assault lasts
/// (`rounds` strikes of `city_hit` hit points among the group's `group_hp`) is less than `margin`
/// percent of the power of the units that stand there.
pub fn outmatched(own_power: u32, group_hp: u32, city_hit: u32, rounds: u32, guard_power: u32, margin: u64) -> bool {
    let group_hp = u64::from(group_hp.max(1));
    let left = group_hp.saturating_sub(u64::from(city_hit) * u64::from(rounds));
    let after = u64::from(own_power) * left / group_hp;
    after * 100 < u64::from(guard_power) * margin
}

/// A lost city in sight, with what every case needs of it: the estimate of its defence, the power of the
/// captor's units around it and the units that could reach it.
struct Site {
    seen: SeenCity,
    captor: PlayerId,
    hp: u16,
    estimate: crate::ai::operations::CityEstimate,
    guard_power: u32,
    /// The free army units within reach, with their strike against this city.
    reach: Vec<Reach>,
}

/// A free unit within reach of a lost city.
#[derive(Clone, Copy)]
struct Reach {
    /// The index in `TurnBrief::units`.
    unit: usize,
    distance: i32,
    strike: Besieger,
}

/// The expected value of retaking `site` with `members`, in the strategist's measure (`operations::war_case`): what
/// the capture brings (`operations::capture_gain`: the city's worth by the rules, its yields from the turn the city
/// is ours again, the capital's share) times the chance the group takes it, less the units it is expected to
/// lose (`operations::loss_percent` of their hammers at the production price). A seat without prices has no
/// measure and retakes as before.
fn pays(brief: &TurnBrief, rules: &CompiledRuleset, stakes: &Stakes, site: &Site, members: &[&Reach], rounds: u32, power_after: u32) -> bool {
    let Some(prices) = brief.prices.as_ref() else { return true };
    let fixes = brief.war.siege;
    // The city yields from the turn it is retaken: the units walk up to `TURNS` turns, then strike `rounds` rounds.
    let held_from = TURNS as u32 + rounds;
    let (gain, _decisive) = operations::capture_gain(prices, stakes, &site.seen, held_from, fixes, operations::rival_percent(brief, site.captor));
    // The chance of the group against the city's own power and the units that stand there (`defence::chance`, the
    // chance the strategist gives a siege, `operations::siege_chance`).
    let city_power = (site.estimate.strength * i64::from(site.estimate.hp) / 10_000).max(0) as u32;
    let success = i64::from(crate::ai::defence::chance(power_after, city_power.saturating_add(site.guard_power)) / 10);
    // The hammers' price as the strategist has it (`prices.production` over the rounds left), the division last: a
    // price per hammer of under one centi-point (a long game ahead) is not a free unit.
    let hammers: i64 = members.iter().map(|m| i64::from(rules.units[brief.units[m.unit].kind.0 as usize].cost)).sum();
    let cost = hammers * operations::loss_percent(fixes) * prices.production / (100 * i64::from(prices.rounds_left.max(1)));
    operations::WarCase { gain: success * gain / 100, cost }.value(stakes) > 0
}

/// The case `site` makes with `members` (units that no other case has), if the group takes the city in time, is
/// not outmatched, and the city is worth the units it costs; the second value is true when only its worth fails.
fn assess(brief: &TurnBrief, rules: &CompiledRuleset, stakes: &Stakes, site: &Site, members: &[&Reach]) -> (Option<Case>, bool) {
    if members.is_empty() {
        return (None, false);
    }
    let (mut own_power, mut group_hp, mut strength_sum) = (0u32, 0u32, 0i64);
    for m in members {
        let unit = &brief.units[m.unit];
        own_power += unit_power(rules, unit.kind, unit.hp);
        group_hp += u32::from(unit.hp);
        strength_sum += i64::from(rules.units[unit.kind.0 as usize].strength.max(1)) * 100;
    }
    let mut besiegers: Vec<Besieger> = members.iter().map(|m| m.strike).collect();
    let Some(rounds) = siege_rounds_within(&mut besiegers, site.estimate.hp, site.estimate.heal, RETAKE_ROUNDS) else {
        return (None, false);
    };
    // The city strikes once a round at the group (a unit of the group's mean strength).
    let mean = strength_sum / members.len() as i64;
    let city_hit = u32::from(city_strike_damage(site.estimate.strength, mean, Roll::Expected));
    if outmatched(own_power, group_hp, city_hit, rounds, site.guard_power, margin_percent(brief, site.captor)) {
        return (None, false);
    }
    let left = u64::from(group_hp.max(1)).saturating_sub(u64::from(city_hit) * u64::from(rounds));
    let power_after = (u64::from(own_power) * left / u64::from(group_hp.max(1))) as u32;
    if !pays(brief, rules, stakes, site, members, rounds, power_after) {
        return (None, true);
    }
    let group = members.iter().map(|m| brief.units[m.unit].id).collect();
    (Some(Case { tile: site.seen.tile, enemy: site.captor, hp: site.hp, rounds, group }), false)
}

/// `cases` for `retake2`: the lost cities come from the seat's own memory (`Memory`, kept in the
/// strategy memory of `AiState::mind_memory`), not from the banner; a major only.
///
/// A unit belongs to the group of one case. Each free unit within reach goes to the nearest of the lost cities
/// (the earlier lost on a tie); a case is judged with the units it gets, and a case that fails (the group takes
/// the city too late, is outmatched, or the city is not worth the units) gives its units back to the others,
/// until every case left holds. `stakes` are the strategist's of the turn (`ai::grand_strategy`).
pub fn cases_remembered(observation: &AIObservation<'_>, brief: &TurnBrief, state: &mut AiState, rules: &CompiledRuleset, stakes: &Stakes) -> Vec<Case> {
    if !brief.retake || observation.view.own_kind() != crate::world::player::PlayerKind::Major {
        return Vec::new();
    }
    let dims = brief.dims;
    let window = crate::ai::operations::march_limit(brief);
    let own: Vec<TileId> = brief.cities.iter().map(|c| c.tile).collect();
    let seen: Vec<(TileId, PlayerId)> = observation.visible_foreign_cities.iter().map(|c| (c.tile, c.owner)).collect();
    let mut kept = strategy_memory(state);
    let mut memory = kept.retake.take().unwrap_or_default();
    memory.observe(brief.round, window, &own, &seen);
    kept.retake = Some(memory.clone());
    store_strategy_memory(&kept, state);
    let lost_tiles: Vec<TileId> = memory.lost.iter().map(|l| l.tile).collect();
    let (kept, threatened) = kept_guards(observation, brief, state, rules);
    let free: Vec<usize> = (0..brief.units.len())
        .filter(|&i| {
            let unit = &brief.units[i];
            army_role(rules, unit.kind).is_some() && unit.hp >= HEAL_BELOW_HP && free_for_retake(brief, state, unit.id, &lost_tiles, &kept)
        })
        .collect();
    // Write-only: surplus guards of a city in danger (what 5c15543 took, its check of the danger being dead), and the ones kept.
    if threatened > 0 && !lost_tiles.is_empty() {
        crate::ai::war_probe::note(|p| p.held_guards.push((brief.round, brief.player, threatened as u32, kept.len() as u32)));
    }
    let mut sites: Vec<Site> = Vec::new();
    for lost in &memory.lost {
        let Some(captor) = lost.captor else { continue };
        let Some(city) = observation.visible_foreign_cities.iter().find(|c| c.tile == lost.tile && c.owner == captor) else { continue };
        if !brief.at_war(captor) {
            continue;
        }
        let seen_city = SeenCity { tile: city.tile, owner: city.owner, population: city.population, last_seen_round: brief.round };
        let garrison = observation
            .visible_foreign_units
            .iter()
            .filter(|u| u.owner == captor && u.tile == city.tile && army_role(rules, u.kind).is_some())
            .max_by_key(|u| (unit_power(rules, u.kind, u.hp), u.handle.index))
            .map(|u| u.kind);
        let mut estimate = estimate_city(brief, rules, &seen_city, garrison);
        // The health bar shows what the siege left; a bar above a city's base shows walls.
        estimate.hp = city.hp;
        let mut reach = Vec::new();
        for &i in &free {
            let unit = &brief.units[i];
            let Some(distance) = dims.distance(unit.tile, city.tile) else { continue };
            let def = &rules.units[unit.kind.0 as usize];
            let steps = i32::from(def.movement.max(1));
            let range = crate::ai::combat::ranged_range(rules, unit.kind).unwrap_or(1);
            if distance > TURNS * steps + range {
                continue;
            }
            let Some(strike) = estimated_strike(rules, unit.kind, unit.hp, &estimate) else { continue };
            reach.push(Reach { unit: i, distance, strike });
        }
        if reach.is_empty() {
            continue;
        }
        // What stands there: the captor's units in sight within `GUARD_RADIUS_NEAR` of the city, outside it (the one
        // in it is the garrison of the estimate).
        let guard_power: u32 = observation
            .visible_foreign_units
            .iter()
            .filter(|u| u.owner == captor && u.tile != city.tile && army_role(rules, u.kind).is_some())
            .filter(|u| dims.distance(u.tile, city.tile).is_some_and(|d| d <= GUARD_RADIUS_NEAR))
            .map(|u| unit_power(rules, u.kind, u.hp))
            .sum();
        sites.push(Site { seen: seen_city, captor, hp: city.hp, estimate, guard_power, reach });
    }
    let mut open: Vec<usize> = (0..sites.len()).collect();
    loop {
        // Every free unit goes to the nearest case still open.
        let mut members: Vec<Vec<&Reach>> = vec![Vec::new(); sites.len()];
        for &i in &free {
            let nearest = open.iter().filter_map(|&s| sites[s].reach.iter().find(|r| r.unit == i).map(|r| (r.distance, s))).min();
            if let Some((_, s)) = nearest {
                let r = sites[s].reach.iter().find(|r| r.unit == i).expect("the unit is within reach of its site");
                members[s].push(r);
            }
        }
        let mut cases = Vec::new();
        // The cases that fail, with whether only the city's worth failed them.
        let mut failed: Vec<(usize, bool)> = Vec::new();
        for &s in &open {
            match assess(brief, rules, stakes, &sites[s], &members[s]) {
                (Some(case), _) => cases.push((s, case)),
                (None, declined) => failed.push((s, declined)),
            }
        }
        if failed.is_empty() {
            return cases
                .into_iter()
                .map(|(s, case)| {
                    crate::ai::war_probe::note(|p| p.retakes.push((brief.round, brief.player, sites[s].seen.tile.0, sites[s].captor, case.group.len() as u32)));
                    case
                })
                .collect();
        }
        // A case that has no unit goes at once; of the others the weakest goes (the fewest units, the later lost on a
        // tie), and its units go to the cases left: two cases that split the units may both fail where one holds.
        let weakest = failed.iter().filter(|(s, _)| !members[*s].is_empty()).min_by_key(|(s, _)| (members[*s].len(), std::cmp::Reverse(*s))).copied();
        for &(s, declined) in &failed {
            if members[s].is_empty() || Some((s, declined)) == weakest {
                if declined {
                    crate::ai::war_probe::note(|p| p.declined.push((brief.round, brief.player, sites[s].seen.tile.0, sites[s].captor, members[s].len() as u32)));
                }
                open.retain(|o| *o != s);
            }
        }
    }
}
