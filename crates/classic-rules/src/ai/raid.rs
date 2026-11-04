//! Raids (classic/docs/ai-raids.md): a small fast group of army units that pillages the
//! improvements and roads of an enemy's land, plunders trade routes, kills workers,
//! settlers and lone units, avoids battle with stronger forces and goes home when the enemy
//! responds. It is chosen where a conquest does not pay (the case of nearly every target:
//! classic/docs/ai-war-audit-current.md) but the damage does: the enemy's yields lost over
//! the rounds the damage lasts and the gold the pillage brings, less the group's risk and,
//! at peace, the declaration of war, in the allocator's currency (hundredths of score
//! points, `ai::value`).
//!
//! The raid has no record of its own: it is the units whose task is `UnitTask::Raid { target }`
//! (target: the enemy city whose land is raided). Its age is the age of its tasks, its enemy
//! the owner of the target city, its budget the size of the group at its formation
//! (`RAID_SIZE` units), its exit the conditions of `verdict`. Everything is derived on each
//! turn from the memory the operations already keep (the unit tasks and the seen cities), so
//! the only thing a save holds of a raid is the task code (`persistence::ai`, version 5).
//!
//! A war is declared for a raid only against a nation that plays itself no more than
//! the AI does: never on a player the host named a human (`TurnBrief::humans`), and only on
//! one that is reachable, no stronger by score, seen no stronger on the spot, and
//! allied with at most `MAX_PACTS` nations we know of. A raid in a war that already
//! runs needs no declaration (it never targets a human either: a new aim against a human is not
//! the AI's to choose).
//!
//! Everything reads the brief, the memory, the player's view of the map and the rules' own
//! commands (`Command::Pillage`, `Command::PlunderTradeRoute`); nothing hidden.

use crate::ai::operations::{army_role, strike_units, Routes, WarGate};
use crate::ai::planner::{role_of, unit_power, AiRole, Decision, PlanTools, TurnBrief, UnitBrief, UnitView};
use crate::ai::planner_strategy::{rounds_since, EARLIEST_WAR_ROUND, OBJECTIVE_RANGE, PEACE_COOLDOWN_ROUNDS};
use crate::ai::planner_units::{hold, move_along, nearest_safe_tile};
use crate::ai::state::{AiState, ArmyRole, UnitTask};
use crate::ai::trace::{Actor, Reason};
use crate::ai::value::{Centi, Prices};
use crate::classic::content::BARBARIAN_CAMP;
use crate::command::Command;
use crate::ids::{PlayerId, TileId};
use crate::knowledge::public::PlayerQueries;
use crate::rules::content::ImprovementKind;
use crate::rules::effects::YieldKind;
use crate::rules::CompiledRuleset;

/// Units a raid takes at most, and the fewest it forms with. Own design: a group small enough
/// to be drawn from the units free in peace and to move as one.
pub const RAID_SIZE: usize = 3;
pub const RAID_MIN: usize = 2;
/// A raid ends this many rounds after it formed (the time a pillage trip takes and the
/// enemy needs to answer). Own design.
pub const RAID_ROUNDS: u32 = 30;
/// A raid that has not set out (the war not declared) after this many rounds ends.
pub const FORM_ROUNDS: u32 = 10;
/// Raiders within this distance of the muster city are gathered.
pub const GATHER_RADIUS: i32 = 3;
/// Tiles around the target city a raid pillages in (the city's own work radius is 3; the
/// ring beyond holds the roads and the farms of the next city).
pub const REGION_RADIUS: i32 = 4;
/// The nearest ring of tiles around a city a raid pillages in (the first is the garrison's).
pub const MIN_PREY_RING: i32 = 2;
/// The ring around the target city the raid walks to when the land beyond is unexplored.
pub const STAGING_RING: i32 = 3;
/// A raider at least this healthy pillages a tile the city can strike (ring `MIN_PREY_RING` up
/// to `CITY_ATTACK_RANGE`; pillaging heals it).
pub const BOLD_HP: u16 = 80;
/// A declaration of war for a raid needs a gain this many times its risk and the price of the
/// declaration: the war costs more than its group (the answer of the enemy, the trade and the
/// friendships it ends). Own design.
pub const DECLARE_FACTOR: i64 = 2;
/// Rounds a raid may take to arrive (the gathering, the walk, the last stretch) and routes checked
/// for the best cases of a turn. Own design: wars between the AIs last a few rounds at the median
/// (classic/docs/ai-war-audit-current.md).
pub const MAX_ARRIVAL_ROUNDS: u32 = 10;
/// The same for a raid that must first declare its war: the enemy may buy peace from
/// `diplomacy::ACCEPT_PEACE_AFTER_ROUNDS` on, so the raid must be there by then.
pub const MAX_DECLARED_ARRIVAL_ROUNDS: u32 = 5;
pub const MAX_WAY_CHECKS: usize = 3;
/// Improvements one raider pillages in a raid (the rest of its time is the way and the
/// retreat). Own design.
pub const PILLAGES_PER_RAIDER: usize = 4;
/// Rounds an enemy worker takes to bring a pillaged improvement back, beyond its building
/// time (the walk and the wait). Own design.
pub const REPAIR_ROUNDS: u32 = 6;
/// Enemy power within `DANGER_RADIUS` of a raider above this share of the group's power
/// sends it home.
pub const RETREAT_PERCENT: u32 = 120;
pub const DANGER_RADIUS: i32 = 3;
/// A raider below this health goes home.
pub const RETREAT_HP: u16 = 55;
/// The fewest nations a raid's enemy may be allied with (pacts we know of).
pub const MAX_PACTS: u8 = 1;
/// A raider within this distance of the prey it can kill (a civilian) goes for it.
pub const PREY_RADIUS: i32 = 4;
/// Share of the group's price the raid expects to lose at the least, and the share added when
/// the enemy's power near the target equals the group's: the risk of the raid. Own design.
pub const LOSS_MIN_PERCENT: i64 = 10;
pub const LOSS_RISK_PERCENT: i64 = 50;
/// Mid-game percent of the warmonger weight of a declaration of war (the rules scale it by the
/// era, `systems::diplomacy`), for the price of a declaration at peace.
pub const WARMONGER_ERA_PERCENT: i64 = 60;

/// The members of the raid on `target`, with the round the oldest task was given.
fn members<'b>(brief: &'b TurnBrief, state: &AiState, target: TileId) -> (Vec<&'b UnitBrief>, u32) {
    let task = Some(UnitTask::Raid { target });
    let mut since = u32::MAX;
    let list = brief
        .units
        .iter()
        .filter(|u| state.task_of(u.id).map(|t| t.task) == task)
        .inspect(|u| since = since.min(state.task_of(u.id).map_or(u32::MAX, |t| t.since_round)))
        .collect();
    (list, since)
}

/// The targets of the raids that run (the tasks' targets, sorted, once each).
fn targets(state: &AiState) -> Vec<TileId> {
    let mut out: Vec<TileId> = state
        .tasks()
        .iter()
        .filter_map(|t| match t.task {
            UnitTask::Raid { target } => Some(target),
            _ => None,
        })
        .collect();
    out.sort();
    out.dedup();
    out
}

/// The nation whose land the raid on `target` raids (the owner of the city we last saw
/// there).
fn enemy_of(state: &AiState, target: TileId) -> Option<PlayerId> {
    state.seen_cities().iter().find(|c| c.tile == target).map(|c| c.owner)
}

/// The enemies of the raids that run: a war with one of them is one a raid fights
/// (`operations::idle_wars` keeps it).
pub fn enemies(state: &AiState) -> Vec<PlayerId> {
    targets(state).into_iter().filter_map(|t| enemy_of(state, t)).collect()
}

/// The raid on `target` is still one: its city is known and an enemy's.
pub fn valid(view: &UnitView<'_, '_>, state: &AiState, target: TileId) -> bool {
    view.brief.war.raids && enemy_of(state, target).is_some_and(|e| e != view.brief.player)
}

// ---- The case of a raid ----

/// What a raid is expected to do, in hundredths of score points.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RaidCase {
    /// Improvements it expects to pillage.
    pub tiles: usize,
    /// The enemy's yields lost, as far as they count for us (`RIVAL_PERCENT`).
    pub damage: Centi,
    /// The pillage gold.
    pub gold: Centi,
    /// The group's expected loss.
    pub risk: Centi,
    /// The warmonger memory a declaration of war at peace costs.
    pub declaration: Centi,
}

impl RaidCase {
    pub fn value(&self) -> Centi {
        self.damage + self.gold - self.risk - self.declaration
    }
}

/// The improvements of `enemy` near its city on `city`, as far as the player has seen them,
/// not pillaged: (tile, distance from the city, improvement).
fn prey(view: &crate::knowledge::view::PlayerView<'_>, rules: &CompiledRuleset, enemy: PlayerId, city: TileId) -> Vec<(TileId, i32, crate::ids::ImprovementId)> {
    let dims = view.dimensions();
    let mut out = Vec::new();
    for tile in crate::math::tiles_in_hex_radius(&dims, city, REGION_RADIUS) {
        if tile == city {
            continue;
        }
        let Some(snapshot) = view.tile(tile).snapshot().copied() else { continue };
        if snapshot.owner != Some(enemy) || snapshot.city || view.known_pillaged(tile) {
            continue;
        }
        // The ring next to the city is its garrison's: no prey.
        if dims.distance(tile, city).is_none_or(|d| d < MIN_PREY_RING) {
            continue;
        }
        let Some(improvement) = snapshot.improvement else { continue };
        let Some(def) = rules.improvements.get(improvement.0 as usize) else { continue };
        if matches!(def.kind, ImprovementKind::AncientRuins) || def.kind == BARBARIAN_CAMP {
            continue;
        }
        out.push((tile, dims.distance(tile, city).unwrap_or(i32::MAX), improvement));
    }
    out.sort_by_key(|(tile, distance, _)| (*distance, *tile));
    out
}

/// What an improvement is worth to the raid that pillages it: the enemy's yields lost until a
/// worker brings it back, and the gold the pillage brings (hundredths of points).
fn improvement_value(prices: &Prices, rules: &CompiledRuleset, improvement: crate::ids::ImprovementId) -> (Centi, Centi) {
    let Some(def) = rules.improvements.get(improvement.0 as usize) else { return (0, 0) };
    let yields = [YieldKind::Food, YieldKind::Production, YieldKind::Gold, YieldKind::Science, YieldKind::Culture, YieldKind::Module(crate::classic::effects::ClassicYield::Faith)];
    let per_round: Centi = yields.iter().map(|k| prices.of_yield(*k) * i64::from(def.yields.of(*k))).sum();
    let lasts = i64::from((u32::from(def.work_turns) + REPAIR_ROUNDS).min(prices.rounds_left));
    let damage = per_round * lasts / i64::from(prices.rounds_left.max(1)) * crate::ai::genome::get(crate::ai::genome::Knob::RivalPercent) / 100;
    let gold = prices.gold * i64::from(def.pillage_gold) / i64::from(prices.rounds_left.max(1));
    (damage, gold)
}

/// The case of a raid of `group` on the land around `city` of `enemy`.
#[allow(clippy::too_many_arguments)]
fn case(
    brief: &TurnBrief,
    rules: &CompiledRuleset,
    prices: &Prices,
    view: &crate::knowledge::view::PlayerView<'_>,
    enemy: PlayerId,
    city: TileId,
    group: &[&UnitBrief],
    declares: bool,
) -> RaidCase {
    let mut values: Vec<(Centi, Centi)> = prey(view, rules, enemy, city)
        .into_iter()
        // The city strikes what stands within its reach: that ring is worth half.
        .map(|(_, distance, improvement)| {
            let (damage, gold) = improvement_value(prices, rules, improvement);
            if distance <= crate::combat::city::CITY_ATTACK_RANGE { (damage / 2, gold / 2) } else { (damage, gold) }
        })
        .collect();
    values.sort_by_key(|(damage, gold)| std::cmp::Reverse(damage + gold));
    let take = values.len().min(group.len() * PILLAGES_PER_RAIDER);
    let (damage, gold) = values.iter().take(take).fold((0, 0), |(d, g), (vd, vg)| (d + vd, g + vg));
    let hammer = prices.production / i64::from(prices.rounds_left.max(1));
    let price: i64 = group.iter().map(|u| i64::from(rules.units[u.kind.0 as usize].cost)).sum::<i64>() * hammer;
    let ours: u32 = group.iter().map(|u| unit_power(rules, u.kind, u.hp)).sum::<u32>().max(1);
    let theirs = enemy_power_near(brief, enemy, city, view.dimensions());
    let loss = LOSS_MIN_PERCENT + (LOSS_RISK_PERCENT * i64::from(theirs) / i64::from(ours)).min(100 - LOSS_MIN_PERCENT);
    let risk = price * loss / 100;
    let declaration = if declares {
        let others = crate::ai::infamy::witnesses(&brief.relations).saturating_sub(1) as i64;
        others * i64::from(crate::systems::diplomacy::WARMONGER_ATTACKED_WEIGHT) * WARMONGER_ERA_PERCENT / 100 * crate::ai::infamy::CENTI_PER_POINT
    } else {
        0
    };
    RaidCase { tiles: take, damage, gold, risk, declaration }
}

/// Power of `enemy`'s military units the player saw near `city` this turn.
fn enemy_power_near(brief: &TurnBrief, enemy: PlayerId, city: TileId, dims: crate::math::MapDimensions) -> u32 {
    brief
        .sightings
        .iter()
        .filter(|s| s.owner == enemy && s.military && dims.distance(s.tile, city).is_some_and(|d| d <= REGION_RADIUS))
        .map(|s| s.power)
        .sum()
}

/// The case of going on with the raid on `enemy`'s land, if one runs: the damage and the gold
/// still to be had there against the group's risk (`ai::peace`: a war that a raid fights is
/// worth going on with while it pays).
pub fn running_case(brief: &TurnBrief, state: &AiState, rules: &CompiledRuleset, prices: &Prices, view: &crate::knowledge::view::PlayerView<'_>, enemy: PlayerId) -> Option<crate::ai::operations::WarCase> {
    let target = targets(state).into_iter().find(|t| enemy_of(state, *t) == Some(enemy))?;
    let (group, _) = members(brief, state, target);
    if group.is_empty() {
        return None;
    }
    let left = case(brief, rules, prices, view, enemy, target, &group, false);
    Some(crate::ai::operations::WarCase { gain: left.damage + left.gold, cost: left.risk })
}

// ---- The turn's plan: advance, end, form ----

/// The raids of one turn (at its start, after the operations): end the ones that are over,
/// declare the war a gathered one waits for, form a new one when none runs and one pays.
pub fn plan(brief: &TurnBrief, state: &mut AiState, rules: &CompiledRuleset, routes: &mut Routes<'_>, gate: WarGate<'_>) -> Vec<Decision> {
    let mut out = Vec::new();
    if !brief.war.raids {
        return out;
    }
    for target in targets(state) {
        out.extend(advance(brief, state, rules, routes, gate, target));
    }
    if targets(state).is_empty() {
        out.extend(form(brief, state, rules, routes, gate));
    }
    out
}

/// Why a raid is over, if it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Over {
    /// Nobody holds the task any more.
    GroupGone,
    /// The time is up, or the war was never declared.
    Limit,
    /// The war ended while the raid ran.
    Peace,
    /// The target is not an enemy city any more, or the damage stops paying.
    Stops,
}

fn advance(brief: &TurnBrief, state: &mut AiState, rules: &CompiledRuleset, routes: &mut Routes<'_>, gate: WarGate<'_>, target: TileId) -> Vec<Decision> {
    let (group, since) = members(brief, state, target);
    let age = brief.round.saturating_sub(since);
    let enemy = enemy_of(state, target);
    let at_war = enemy.is_some_and(|e| brief.at_war(e));
    // A raid formed at peace waits for its war until it declares; the war that began (or ended)
    // after the raid formed moved the relation's last change past the raid's first round.
    let changed = enemy.and_then(|e| state.relation(e)).and_then(|m| m.last_change_round).is_some_and(|r| r >= since);
    let over = if group.is_empty() {
        Some(Over::GroupGone)
    } else if enemy.is_none_or(|e| e == brief.player || brief.humans.contains(&e)) {
        Some(Over::Stops)
    } else if age > RAID_ROUNDS {
        Some(Over::Limit)
    } else if !at_war && changed {
        Some(Over::Peace)
    } else if !at_war && age > FORM_ROUNDS {
        Some(Over::Limit)
    } else if at_war && still_pays(brief, state, rules, routes, gate, target, enemy.unwrap_or(brief.player), &group).is_some_and(|pays| !pays) {
        Some(Over::Stops)
    } else {
        None
    };
    if let Some(cause) = over {
        crate::ai::war_probe::note(|p| {
            p.raids.ended += 1;
            match cause {
                Over::GroupGone => p.raids.ended_group_gone += 1,
                Over::Limit => p.raids.ended_limit += 1,
                Over::Peace => p.raids.ended_peace += 1,
                Over::Stops => {}
            }
        });
        for unit in &group {
            state.clear_task(unit.id);
        }
        // Tasks of units that are gone go with them.
        return vec![Decision::refuse(Actor::Player, Reason::EndRaid)];
    }
    if at_war {
        return Vec::new();
    }
    // Waiting for the war: the group declares it once it has gathered.
    let Some(enemy) = enemy else { return Vec::new() };
    let muster = brief.nearest_city(brief.dims, target).map(|c| c.tile);
    let gathered = muster.is_some_and(|m| group.iter().filter(|u| brief.dims.distance(u.tile, m).is_some_and(|d| d <= GATHER_RADIUS)).count() >= RAID_MIN.min(group.len()));
    if !gathered || !may_declare(brief, state, routes, enemy) {
        return Vec::new();
    }
    crate::ai::war_probe::note(|p| {
        p.raids.declared += 1;
        p.declarations.push((brief.round, brief.player, enemy, true));
    });
    let command = Command::DeclareWar { player: brief.player, enemy };
    vec![Decision::act(Actor::Player, Reason::DeclareWar, command)]
}

/// Does the raid still pay (the damage left over the region, less the group's risk)? None: not
/// priced here (a gate without prices).
#[allow(clippy::too_many_arguments)]
fn still_pays(brief: &TurnBrief, _state: &AiState, rules: &CompiledRuleset, routes: &Routes<'_>, gate: WarGate<'_>, target: TileId, enemy: PlayerId, group: &[&UnitBrief]) -> Option<bool> {
    let WarGate::Value(prices, _) = gate else { return None };
    let left = case(brief, rules, prices, routes.view(), enemy, target, group, false);
    // Prey in sight or known keeps the raid going; a region with nothing left to pillage ends it
    // (the civilians and the routes it meets on the way are not counted: they are a bonus).
    Some(left.tiles > 0)
}

/// A war on `enemy` may be declared now: the season, no other war, the cooldown after the last
/// peace, and the rules accept the declaration.
fn may_declare(brief: &TurnBrief, state: &AiState, routes: &Routes<'_>, enemy: PlayerId) -> bool {
    let last = state.relation(enemy).and_then(|m| m.last_change_round);
    brief.round >= brief.speed.game_round(EARLIEST_WAR_ROUND)
        && !brief.relations.iter().any(|r| r.at_war)
        && rounds_since(brief, last) >= brief.speed.pace_rounds(PEACE_COOLDOWN_ROUNDS)
        && routes.view().accepts(&Command::DeclareWar { player: brief.player, enemy })
}

/// The army units that make a raid: melee units that can pillage, the fastest first, then the
/// strongest, lower id on a tie.
fn raiders<'b>(brief: &'b TurnBrief, state: &'b AiState, rules: &'b CompiledRuleset) -> Vec<&'b UnitBrief> {
    let mut list: Vec<&UnitBrief> = strike_units(brief, state, rules)
        .filter(|u| army_role(rules, u.kind) == Some(ArmyRole::Melee) && rules.units[u.kind.0 as usize].combat.pillage)
        .filter(|u| u.hp >= RETREAT_HP + 20)
        .collect();
    list.sort_by_key(|u| (std::cmp::Reverse(rules.units[u.kind.0 as usize].movement), std::cmp::Reverse(unit_power(rules, u.kind, u.hp)), u.id));
    list.truncate(RAID_SIZE);
    list
}

fn form(brief: &TurnBrief, state: &mut AiState, rules: &CompiledRuleset, routes: &mut Routes<'_>, gate: WarGate<'_>) -> Vec<Decision> {
    let WarGate::Value(prices, _) = gate else { return Vec::new() };
    let ready = crate::ai::operations::in_season(brief)
        && brief.cities.len() >= 2
        && crate::ai::operations::strategy_wages_war(brief, state.strategy.current);
    // An operation that runs or forms keeps the units it took; a raid does not compete with it.
    if !ready || !state.operations().is_empty() {
        return Vec::new();
    }
    let group: Vec<UnitBrief> = raiders(brief, state, rules).into_iter().copied().collect();
    if group.len() < RAID_MIN {
        return Vec::new();
    }
    let group: Vec<&UnitBrief> = group.iter().collect();
    let walker = group[0].kind;
    let race = routes.view().race();
    let own_score = race.own_score();
    let mut found_cases: Vec<(Centi, TileId, PlayerId, bool)> = Vec::new();
    for relation in brief.relations.iter().filter(|r| r.kind == crate::world::player::PlayerKind::Major) {
        let enemy = relation.other;
        // A new aim never targets a player that plays itself.
        if brief.humans.contains(&enemy) {
            continue;
        }
        let declares = !relation.at_war;
        if declares {
            let score = race.scores.iter().find(|(p, _)| *p == enemy).map_or(i64::MAX, |(_, s)| *s);
            if score > own_score || relation.pacts_known > MAX_PACTS || !may_declare(brief, state, routes, enemy) {
                continue;
            }
        } else if brief.relations.iter().filter(|r| r.at_war).count() > 1 {
            // Several wars: the army is the operations' and the defence's.
            continue;
        }
        for city in state.seen_cities().iter().filter(|c| c.owner == enemy) {
            let Some(home) = brief.nearest_city(brief.dims, city.tile) else { continue };
            if brief.dims.distance(home.tile, city.tile).is_none_or(|d| d > OBJECTIVE_RANGE) {
                continue;
            }
            let found = case(brief, rules, prices, routes.view(), enemy, city.tile, &group, declares);
            let value = found.value();
            if found.tiles == 0 || value <= 0 {
                continue;
            }
            // A war declared for a raid must pay several times its group's risk.
            if declares && (found.damage + found.gold) < DECLARE_FACTOR * (found.risk + found.declaration) {
                continue;
            }
            crate::ai::war_probe::note(|p| p.raids.evaluated += 1);
            found_cases.push((value, city.tile, enemy, declares));
        }
    }
    // The best cases first (lower tile on a tie); the first with a known way that the group walks
    // in time wins: a war lasts a few rounds, a raid that arrives after it pillages nothing. The
    // staging tile lies in the region the raid pillages (three tiles from the target), so the
    // walk to it is the arrival.
    found_cases.sort_by_key(|(value, tile, _, _)| (std::cmp::Reverse(*value), *tile));
    let mut chosen = None;
    for (_, target, _, declares) in found_cases.into_iter().take(MAX_WAY_CHECKS) {
        let Some(home) = brief.nearest_city(brief.dims, target).map(|c| c.tile) else { continue };
        let Some(stage) = routes.stage(walker, home, target) else { continue };
        let gather = group
            .iter()
            .map(|u| brief.dims.distance(u.tile, home).unwrap_or(0).max(0) as u32 / u32::from(rules.units[u.kind.0 as usize].movement.max(1)))
            .max()
            .unwrap_or(0);
        if gather + stage.walk <= if declares { MAX_DECLARED_ARRIVAL_ROUNDS } else { MAX_ARRIVAL_ROUNDS } {
            chosen = Some(target);
            break;
        }
    }
    let Some(target) = chosen else { return Vec::new() };
    for unit in &group {
        state.set_task(unit.id, UnitTask::Raid { target }, brief.round);
    }
    crate::ai::war_probe::note(|p| {
        p.raids.formed += 1;
        p.raids.chosen_negative_conquest += 1;
    });
    
    vec![Decision::refuse(Actor::Player, Reason::FormRaid)]
}

// ---- Raiders ----

/// One step of a raider: gather and wait for the war; go home when the enemy answers or it is
/// hurt; plunder, pillage or kill what it stands on or next to; else walk to the prey.
pub fn member_step(view: &UnitView<'_, '_>, state: &mut AiState, tools: &mut PlanTools<'_>, target: TileId) -> Decision {
    let unit = view.local.unit;
    let actor = Actor::Unit(unit.id);
    let Some(enemy) = enemy_of(state, target) else {
        return Decision::refuse(actor, Reason::TargetGone);
    };
    let dims = view.local.view.dimensions();
    if !view.brief.at_war(enemy) {
        // Gather at the own city nearest to the target and wait for the war.
        let Some(muster) = view.brief.nearest_city(dims, target) else { return hold(view, Reason::Rally) };
        if dims.distance(unit.tile, muster.tile).is_some_and(|d| d <= 1) {
            return hold(view, Reason::Rally);
        }
        return move_along(view, tools, muster.tile, Reason::Rally);
    }
    if unit.moves_left == 0 {
        return Decision::refuse(actor, Reason::Waiting);
    }
    // The enemy answers: stronger forces near, or this unit hurt.
    let ours: u32 = group_power(view, state, target);
    let theirs = foes_power_near(view, unit.tile, DANGER_RADIUS);
    if unit.hp < RETREAT_HP || theirs * 100 > ours.max(1) * RETREAT_PERCENT {
        crate::ai::war_probe::note(|p| p.raids.retreats += 1);
        if let Some(safe) = nearest_safe_tile(view, tools) {
            if safe != unit.tile {
                // The raider leaves the raid: it heals where it is safe, then serves as any unit
                // (no dithering at the edge of the enemy's sight).
                state.set_task(unit.id, UnitTask::Heal { at: safe }, view.brief.round);
                return move_along(view, tools, safe, Reason::Retreat);
            }
        }
        return hold(view, Reason::Retreat);
    }
    // Plunder a route whose trade unit stands here, pillage what is here.
    if view.local.foreign_routes.iter().any(|r| r.tile == unit.tile && !r.sea) {
        let command = Command::PlunderTradeRoute { unit: unit.handle };
        if view.local.view.accepts(&command) {
            crate::ai::war_probe::note(|p| p.raids.plunders += 1);
            return Decision::act(actor, Reason::Plunder, command);
        }
    }
    let command = Command::Pillage { unit: unit.handle };
    if unit.can_attack && pillage_here(view, enemy) && view.local.view.accepts(&command) {
        crate::ai::war_probe::note(|p| p.raids.pillages += 1);
        return Decision::act(actor, Reason::Pillage, command);
    }
    // Prey: a civilian within reach first (the front takes what is adjacent), a caravan next,
    // then the nearest improvement not under the city's guns.
    let bold = unit.hp >= BOLD_HP;
    let safe_from_city = |tile: TileId| {
        view.local.foreign_cities.iter().all(|c| dims.distance(c.tile, tile).is_none_or(|d| d > crate::combat::city::CITY_ATTACK_RANGE || (bold && d >= MIN_PREY_RING)))
    };
    let civilian = view
        .local
        .foreign_units
        .iter()
        .filter(|f| view.local.relation(f.owner).is_some_and(|r| r.at_war) && role_of(tools.rules, f.kind) != AiRole::Military)
        .filter(|f| dims.distance(unit.tile, f.tile).is_some_and(|d| d <= PREY_RADIUS) && safe_from_city(f.tile))
        .min_by_key(|f| (dims.distance(unit.tile, f.tile).unwrap_or(i32::MAX), f.tile));
    if let Some(prey) = civilian {
        return crate::ai::planner_units::approach(view, tools, prey.tile, Reason::Raid);
    }
    let caravan = view
        .local
        .foreign_routes
        .iter()
        .filter(|r| !r.sea && view.local.relation(r.owner).is_some_and(|rel| rel.at_war) && safe_from_city(r.tile))
        .filter(|r| dims.distance(unit.tile, r.tile).is_some_and(|d| d <= PREY_RADIUS))
        .min_by_key(|r| (dims.distance(unit.tile, r.tile).unwrap_or(i32::MAX), r.tile));
    if let Some(route) = caravan {
        return walk_to(view, tools, route.tile, target);
    }
    let goal = prey(&view.local.view, tools.rules, enemy, target)
        .into_iter()
        .filter(|(tile, _, _)| safe_from_city(*tile) && !crowded(view, *tile))
        .min_by_key(|(tile, _, _)| (dims.distance(unit.tile, *tile).unwrap_or(i32::MAX), *tile));
    match goal {
        Some((tile, _, _)) if tile != unit.tile => walk_to(view, tools, tile, target),
        Some(_) => hold(view, Reason::Raid),
        None => {
            // Nothing left to pillage in the region: the plan ends the raid; the unit waits.
            Decision::refuse(actor, Reason::Waiting)
        }
    }
}

/// A step toward `tile`; when the known map has no way all the way (the land beyond the border is
/// unexplored), toward the ring `STAGING_RING` tiles from the target city, whose way the raid's
/// formation found.
fn walk_to(view: &UnitView<'_, '_>, tools: &mut PlanTools<'_>, tile: TileId, city: TileId) -> Decision {
    let step = move_along(view, tools, tile, Reason::Raid);
    if step.command.is_some() || !matches!(step.reason, Reason::NoPath | Reason::PathOverBudget) {
        return step;
    }
    crate::ai::planner_units::approach_at(view, tools, city, STAGING_RING, false, Reason::Raid)
}

/// The unit stands on a tile it may pillage: the enemy's (the rules decide the rest).
fn pillage_here(view: &UnitView<'_, '_>, enemy: PlayerId) -> bool {
    let tile = view.local.unit.tile;
    view.local.view.tile(tile).snapshot().is_some_and(|s| s.owner == Some(enemy) && !s.city && (s.improvement.is_some() || s.route != crate::world::map::Route::None))
}

/// Power of the raid's members within `DANGER_RADIUS` of the asked unit, itself included.
fn group_power(view: &UnitView<'_, '_>, state: &AiState, target: TileId) -> u32 {
    let unit = view.local.unit;
    let dims = view.local.view.dimensions();
    let (list, _) = members(view.brief, state, target);
    list.iter()
        .filter(|u| u.id == unit.id || dims.distance(u.tile, unit.tile).is_some_and(|d| d <= DANGER_RADIUS))
        .map(|u| unit_power(view.local.view.rules(), u.kind, u.hp))
        .sum()
}

/// Power of the military units at war with us the unit sees within `radius` of `tile`.
fn foes_power_near(view: &UnitView<'_, '_>, tile: TileId, radius: i32) -> u32 {
    let dims = view.local.view.dimensions();
    view.local
        .foreign_units
        .iter()
        .filter(|f| view.local.relation(f.owner).is_some_and(|r| r.at_war) && dims.distance(f.tile, tile).is_some_and(|d| d <= radius))
        .filter(|f| role_of(view.local.view.rules(), f.kind) == AiRole::Military)
        // A garrison stays in its city: the city's own strike is `safe_from_city`.
        .filter(|f| !view.local.foreign_cities.iter().any(|c| c.tile == f.tile))
        .map(|f| unit_power(view.local.view.rules(), f.kind, f.hp))
        .sum()
}

/// A tile where an enemy military unit stands next to or on: no prey for a raider.
fn crowded(view: &UnitView<'_, '_>, tile: TileId) -> bool {
    let dims = view.local.view.dimensions();
    view.local.foreign_units.iter().any(|f| {
        view.local.relation(f.owner).is_some_and(|r| r.at_war) && role_of(view.local.view.rules(), f.kind) == AiRole::Military && dims.distance(f.tile, tile).is_some_and(|d| d <= 1)
    })
}

