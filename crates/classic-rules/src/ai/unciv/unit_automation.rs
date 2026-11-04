// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
//
// Derived from Unciv (https://github.com/yairm210/Unciv), tag 4.22.10, MPL-2.0:
// core/src/com/unciv/logic/automation/unit/UnitAutomation.kt (`automateUnitMoves` for military
// units and the functions it calls), Automation.afraidOfBarbarians.

//! Unciv's military unit automation (`UnitAutomation.automateUnitMoves`), one
//! order a decision: the driver asks the unit again after each order, and the
//! chain runs again from the top on the new observation, as Unciv's chain goes
//! on after an action that leaves the unit moves. An order that would repeat
//! one already sent from the same tile with the same moves (it changed nothing)
//! is skipped (`Memo::tried`), so the chain goes on to its next step.

use crate::ai::planner::{Decision, PlanTools};
use crate::ai::state::AiState;
use crate::ai::trace::Reason;
use crate::command::Command;
use crate::ids::TileId;
use crate::knowledge::public::PlayerQueries;
use crate::rules::effects::UnitAbility;

use super::battle_helper::{attack_command, attackable_enemies, try_attacking, try_disembark_unit_to_attack_position};
use super::head_towards::{head_towards, head_towards_enemy_city, move_to_tile, try_head_towards_enemy_city};
use super::next_turn;
use super::sight::{actor, Kind, Unit, FULL_HP};
use super::{Memo, Step};

const CLOSE_ENEMY_TILES_AWAY_LIMIT: i32 = 5;

/// `automateUnitMoves` for a military unit (land or sea; the air arm and
/// the nuclear weapons are the caller's).
pub fn automate_military(u: &Unit<'_, '_>, state: &AiState, memo: &mut Memo, tools: &mut PlanTools<'_>) -> Decision {
    macro_rules! step {
        ($e:expr) => {
            let step = $e;
            match memo.guard(u, step) {
                Step::Next => {}
                Step::Act(d) => return d,
                Step::Done(reason) => return done(u, reason),
            }
        };
    }
    if !u.has_movement() {
        return Decision::refuse(actor(&u.me), Reason::Waiting);
    }
    // The second half of `Battle.moveAndAttack`: the blow after the walk.
    if let Some((from, target)) = memo.pending.take() {
        if from == u.tile() {
            if let Some(decision) = attack_command(u, target) {
                step!(Step::Act(decision));
            }
        }
    }
    if let Some((tile, _)) = memo.pillage.take() {
        if tile == u.tile() {
            step!(pillage_here(u));
        }
    }
    // `NextTurnAutomation.automateCityConquer` and `automateSettlerEscorting` run
    // before the units' own moves, once a turn.
    if !memo.conquer_done {
        memo.conquer_done = true;
        step!(next_turn::city_conquer(u, memo));
    }
    if !memo.escort_done {
        memo.escort_done = true;
        step!(next_turn::settler_escort(u, tools));
    }
    step!(try_accompany_settler_or_great_person(u, tools));
    step!(try_go_to_ruin(u, tools));
    if u.health() < 50 {
        step!(try_retreat(u));
        step!(try_heal_unit(u, memo, tools));
    }
    // No enemies near and the unit heals here: wait until it is at full health.
    if u.health() < FULL_HP && can_unit_heal_in_turns_on_current_tile(u, 2, 3) {
        return Decision::refuse(actor(&u.me), Reason::Heal);
    }
    step!(try_head_towards_our_sieged_city(u, tools));
    // An embarked melee unit that can land and attack next turn does not attack from the water.
    if let Some(decision) = try_disembark_unit_to_attack_position(u) {
        step!(Step::Act(decision));
    }
    if let Some(decision) = try_attacking(u, memo, tools) {
        step!(Step::Act(decision));
    }
    step!(try_take_back_captured_city(u, state, tools));
    // Every unit without a specific target heads for the enemy city closest to one of ours.
    step!(try_head_towards_enemy_city(u, state, tools));
    step!(try_head_towards_encampment(u, tools));
    step!(try_garrisoning_land_unit(u, tools));
    if u.health() < 80 {
        step!(try_heal_unit(u, memo, tools));
    }
    step!(try_advance_towards_close_enemy(u, tools));
    if u.health() < FULL_HP {
        step!(try_heal_unit(u, memo, tools));
    }
    step!(try_prepare(u, tools));
    step!(try_explore(u, tools));
    step!(try_fog_bust(u, tools));
    if u.local().view.own_kind() != crate::world::player::PlayerKind::Major {
        // Idle city-state units wander so they do not obstruct the players.
        step!(wander(u, true));
    }
    Decision::refuse(actor(&u.me), Reason::Idle)
}

/// The end of a unit's turn: nothing more this turn.
fn done(u: &Unit<'_, '_>, reason: Reason) -> Decision {
    Decision::refuse(actor(&u.me), reason)
}

/// The unit has the ability from its kind or its promotions (the rules' data).
pub fn has_ability(u: &Unit<'_, '_>, ability: UnitAbility) -> bool {
    use crate::rules::effects::Modifier;
    let rules = u.rules();
    let grant = Modifier::GrantAbility(ability);
    let Some(def) = rules.units.get(u.me.kind.0 as usize) else {
        return false;
    };
    let own = def.effects.passive_unit.iter().any(|e| e.modifier == grant);
    own || u.me.promotions.iter().filter_map(|p| rules.promotions.get(p.0 as usize)).any(|p| p.effects.passive_unit.iter().any(|e| e.modifier == grant))
}

/// `isGoodTileToExplore`.
fn is_good_tile_to_explore(u: &Unit<'_, '_>, tools: &mut PlanTools<'_>, tile: TileId, sight: i32) -> bool {
    let city_state_land = u.owner_of(tile).and_then(|o| u.local().relation(o)).is_some_and(|r| matches!(r.kind, crate::world::player::PlayerKind::Module(crate::world::player::ClassicKind::CityState(_))));
    if city_state_land {
        return false;
    }
    let dims = u.dims();
    let view = &u.local().view;
    if !crate::math::hex_disc(dims, tile, sight).any(|t| !view.is_known(t)) {
        return false;
    }
    let own_kind = view.own_kind();
    if own_kind != crate::world::player::PlayerKind::Major && !dims.neighbors(tile).into_iter().any(|n| u.owner_of(n) == Some(u.local().player)) {
        return false;
    }
    u.damage_from_terrain(tile) <= 0 && u.distance_to_closest_enemy(tile, 3, true) > 3 && u.can_move_to(tile) && u.can_reach(tools, tile)
}

/// `tryExplore`.
pub fn try_explore(u: &Unit<'_, '_>, tools: &mut PlanTools<'_>) -> Step {
    let sight = u.kind.sight();
    let reached: Vec<TileId> = u.reachable().iter().map(|(t, _)| *t).collect();
    let view = &u.local().view;
    let dims = u.dims();
    let mut best: Option<(i32, TileId)> = None;
    for tile in reached.into_iter().take(tools.settings.budget.candidates as usize * 2) {
        if !is_good_tile_to_explore(u, tools, tile, sight) {
            continue;
        }
        let unexplored = crate::math::hex_disc(dims, tile, sight).filter(|t| dims.distance(*t, tile) == Some(sight) && !view.is_known(*t)).count() as i32;
        let score = u.tile_height(tile) + unexplored;
        if best.is_none_or(|(s, t)| score > s || (score == s && tile < t)) {
            best = Some((score, tile));
        }
    }
    if let Some((_, tile)) = best {
        return head_towards(u, tools, tile, Reason::Explore);
    }
    // Nothing immediate: look further.
    let mut near: Vec<TileId> = crate::math::hex_disc(dims, u.tile(), 5).collect();
    near.sort_by_key(|t| (u.distance(*t, u.tile()), *t));
    for tile in near.into_iter().take(tools.settings.budget.candidates as usize) {
        if is_good_tile_to_explore(u, tools, tile, sight) {
            return head_towards(u, tools, tile, Reason::Explore);
        }
    }
    Step::Next
}

/// `tryGoToRuin`: the ruins the unit sees, when it may walk there.
fn try_go_to_ruin(u: &Unit<'_, '_>, tools: &mut PlanTools<'_>) -> Step {
    let view = &u.local().view;
    if view.own_kind() != crate::world::player::PlayerKind::Major {
        return Step::Next;
    }
    let Some(ruins) = crate::ai::scouting::ruins_improvement(u.rules()) else {
        return Step::Next;
    };
    let dims = u.dims();
    let mut seen: Vec<TileId> = crate::math::hex_disc(dims, u.tile(), u.kind.sight())
        .filter(|t| view.is_visible(*t) && view.tile(*t).snapshot().is_some_and(|s| s.improvement == Some(ruins)))
        .collect();
    seen.sort_by_key(|t| (u.distance(*t, u.tile()), *t));
    match seen.into_iter().find(|t| u.can_move_to(*t) && u.can_reach(tools, *t)) {
        Some(tile) => head_towards(u, tools, tile, Reason::Explore),
        None => Step::Next,
    }
}

/// `Automation.afraidOfBarbarians` from what the player knows: the continent's
/// size read as the land it has explored.
fn afraid_of_barbarians(u: &Unit<'_, '_>) -> bool {
    let view = &u.local().view;
    if view.own_kind() != crate::world::player::PlayerKind::Major {
        return false;
    }
    let barbarians = u.local().relations.iter().any(|r| r.kind == crate::world::player::PlayerKind::Module(crate::world::player::ClassicKind::Barbarian));
    let speed = u.brief().speed;
    if u.sight.round > speed.game_round(200) {
        return false;
    }
    let mut multiplier = 1.0f32;
    if u.sight.round > speed.game_round(120) {
        multiplier /= 2.0;
    }
    if u.sight.own_cities.is_empty() || u.sight.own_units.len() as f32 >= 4.0 * multiplier {
        return false;
    }
    let _ = barbarians;
    let tiles = view.tile_count() as u32;
    let kinds = view.terrain_kinds();
    let (mut land, mut visible) = (0u32, 0u32);
    for t in 0..tiles {
        let tile = TileId(t);
        if view.is_visible(tile) {
            visible += 1;
        }
        if view.tile(tile).snapshot().is_some_and(|s| !kinds.is_water(s.terrain.0)) {
            land += 1;
        }
    }
    (land as f32) >= visible as f32 * multiplier
}

/// `tryFogBust`: stand just outside the borders where barbarians would spawn.
fn try_fog_bust(u: &Unit<'_, '_>, tools: &mut PlanTools<'_>) -> Step {
    let view = &u.local().view;
    let dims = u.dims();
    if crate::math::hex_disc(dims, u.tile(), 5).all(|t| view.is_visible(t)) {
        return Step::Next;
    }
    if !afraid_of_barbarians(u) {
        return Step::Next;
    }
    let reached: Vec<TileId> = u.reachable().iter().map(|(t, _)| *t).collect();
    let good: Vec<TileId> = reached.into_iter().filter(|t| is_good_tile_for_fog_busting(u, tools, *t)).collect();
    if !good.is_empty() {
        let pick = good[next_turn::draw(u.sight.round, u.me.id.0, 0x6f67, good.len())];
        return head_towards(u, tools, pick, Reason::Explore);
    }
    let mut near: Vec<TileId> = crate::math::hex_disc(dims, u.tile(), 5).collect();
    near.sort_by_key(|t| (u.distance(*t, u.tile()), *t));
    for tile in near.into_iter().take(tools.settings.budget.candidates as usize) {
        if is_good_tile_for_fog_busting(u, tools, tile) {
            return head_towards(u, tools, tile, Reason::Explore);
        }
    }
    Step::Next
}

fn is_good_tile_for_fog_busting(u: &Unit<'_, '_>, tools: &mut PlanTools<'_>, tile: TileId) -> bool {
    let dims = u.dims();
    let me = u.local().player;
    u.can_move_to(tile)
        && u.owner_of(tile).is_none()
        && dims.neighbors(tile).into_iter().all(|n| u.owner_of(n).is_none())
        && u.local().view.is_known(tile)
        && crate::math::hex_disc(dims, tile, 2).any(|t| u.owner_of(t) == Some(me))
        && u.damage_from_terrain(tile) <= 0
        && u.can_reach(tools, tile)
}

/// `wander`: one step to a random neighbour (in its territory when asked).
pub fn wander(u: &Unit<'_, '_>, stay_in_territory: bool) -> Step {
    if !u.has_movement() {
        return Step::Next;
    }
    let me = u.local().player;
    let here_own = u.owner_of(u.tile()) == Some(me);
    let tiles: Vec<TileId> = u
        .dims()
        .neighbors(u.tile())
        .into_iter()
        .filter(|t| u.can_move_to(*t) && u.reaches_now(*t) && u.damage_from_terrain(*t) <= 0)
        .filter(|t| !stay_in_territory || u.owner_of(*t) == Some(me) || !here_own)
        .collect();
    if tiles.is_empty() {
        return Step::Next;
    }
    let pick = tiles[next_turn::draw(u.sight.round, u.me.id.0, 0x7761, tiles.len())];
    move_to_tile(u, pick, Reason::Idle)
}

/// `tryHeadTowardsEncampment`: the known barbarian camps within 6 tiles of our cities.
fn try_head_towards_encampment(u: &Unit<'_, '_>, tools: &mut PlanTools<'_>) -> Step {
    if u.kind.self_destructs() {
        return Step::Next;
    }
    let Some(camp) = crate::systems::barbarians::camp_improvement(u.rules()) else {
        return Step::Next;
    };
    let view = &u.local().view;
    let dims = u.dims();
    let mut camps: Vec<TileId> = Vec::new();
    for city in &u.sight.own_cities {
        for t in crate::math::hex_disc(dims, city.tile, 6) {
            if view.is_known(t) && view.tile(t).snapshot().is_some_and(|s| s.improvement == Some(camp)) {
                camps.push(t);
            }
        }
    }
    camps.sort_by_key(|t| (u.distance(*t, u.tile()), *t));
    camps.dedup();
    match camps.into_iter().take(tools.settings.budget.candidates as usize).find(|t| u.can_reach(tools, *t)) {
        Some(tile) => head_towards(u, tools, tile, Reason::AttackUnit),
        None => Step::Next,
    }
}

/// `tryRetreat`: a wounded unit near the enemy steps away from it, toward its
/// nearest city. (Unciv also swaps with a healthy unit of ours; the rules have
/// no swap, so a tile held by our unit is skipped.)
pub fn try_retreat(u: &Unit<'_, '_>) -> Step {
    if u.kind.is_civilian() || u.kind.is_air() {
        return Step::Next;
    }
    if u.distance_to_closest_enemy(u.tile(), 4, true) > 3 {
        return Step::Next;
    }
    let closest_city = u.sight.own_cities.iter().map(|c| c.tile).min_by_key(|t| (u.distance(*t, u.tile()), *t)).filter(|t| u.distance(*t, u.tile()) < 20);
    let mut tiles: Vec<TileId> = u.reachable().iter().map(|(t, _)| *t).collect();
    match closest_city {
        Some(city) => tiles.sort_by_key(|t| (u.distance(*t, city), *t)),
        None => tiles.sort_by_key(|t| (std::cmp::Reverse(u.distance_to_closest_enemy(*t, 3, false)), *t)),
    }
    let ours = u.distance_to_closest_enemy(u.tile(), 4, false);
    for tile in tiles {
        if ours >= u.distance_to_closest_enemy(tile, 4, false) {
            continue;
        }
        if u.own_military_on(tile).is_none() && u.can_move_to(tile) {
            return move_to_tile(u, tile, Reason::Retreat);
        }
    }
    Step::Next
}

/// `tryHealUnit`.
pub fn try_heal_unit(u: &Unit<'_, '_>, memo: &mut Memo, tools: &mut PlanTools<'_>) -> Step {
    if u.kind.is_ranged() && has_ability(u, UnitAbility::AlwaysHeal) {
        return Step::Next; // It heals anyway, and its attacks do not hurt.
    }
    // Pillage improvements while healing.
    if let Some(step) = try_pillage_improvement(u, memo, true) {
        return step;
    }
    if u.reachable().is_empty() {
        return Step::Done(Reason::Heal);
    }
    let dangerous = u.dangerous_tiles(3);
    let is_dangerous = |t: &TileId| dangerous.binary_search(t).is_ok();
    if !is_dangerous(&u.tile()) && can_unit_heal_in_turns_on_current_tile(u, 3, 3) {
        return Step::Done(Reason::Heal);
    }
    let viable: Vec<TileId> = u.reachable().iter().map(|(t, _)| *t).filter(|t| !is_dangerous(t) && (*t == u.tile() || u.can_move_to(*t))).collect();
    let ranked: Vec<(i32, TileId)> = viable.iter().map(|t| (u.rank_tile_for_healing(*t), *t)).collect();
    if ranked.iter().all(|(rank, _)| *rank == 0) {
        // We cannot heal here at all: probably embarked.
        if !u.kind.is_air() {
            let mut cities: Vec<TileId> = u.sight.own_cities.iter().map(|c| c.tile).collect();
            cities.sort_by_key(|t| (u.distance(*t, u.tile()), *t));
            return match cities.into_iter().find(|t| u.can_reach(tools, *t)) {
                Some(city) => head_towards(u, tools, city, Reason::Heal),
                None => match wander(u, false) {
                    Step::Next => Step::Done(Reason::Heal),
                    other => other,
                },
            };
        }
        return Step::Next;
    }
    let best_rank = ranked.iter().map(|(r, _)| *r).max().unwrap_or(0);
    let best = ranked.iter().filter(|(r, _)| *r == best_rank).map(|(_, t)| *t).max_by_key(|t| (u.defensive_bonus(*t), std::cmp::Reverse(*t)));
    let Some(best) = best else {
        return Step::Next;
    };
    if best != u.tile() && best_rank > u.rank_tile_for_healing(u.tile()) - u.damage_from_terrain(u.tile()) {
        return move_to_tile(u, best, Reason::Heal);
    }
    fortify_if_can(u)
}

/// `fortifyIfCan`: fortify, or end the turn.
fn fortify_if_can(u: &Unit<'_, '_>) -> Step {
    if u.me.can_fortify && !u.me.fortified {
        return Step::Act(Decision::act(actor(&u.me), Reason::Heal, Command::Fortify { unit: u.me.handle }));
    }
    Step::Done(Reason::Heal)
}

/// `canUnitHealInTurnsOnCurrentTile`: safe here and healed to full within `turns`.
fn can_unit_heal_in_turns_on_current_tile(u: &Unit<'_, '_>, turns: i32, no_enemy_distance: i32) -> bool {
    if has_ability(u, UnitAbility::AlwaysHeal) {
        return false;
    }
    let in_safe_city = u.sight.own_cities.iter().any(|c| c.tile == u.tile() && c.hp > 50);
    if !in_safe_city && u.distance_to_closest_enemy(u.tile(), no_enemy_distance, true) <= no_enemy_distance {
        return false;
    }
    let required = (i32::from(FULL_HP) - i32::from(u.health()) + turns - 1) / turns;
    required <= u.rank_tile_for_healing(u.tile())
}

/// `tryPillageImprovement`: an improvement of an enemy at war the unit can walk
/// to and pillage this turn (the most defensible one).
fn try_pillage_improvement(u: &Unit<'_, '_>, memo: &mut Memo, only_to_heal: bool) -> Option<Step> {
    if u.kind.is_civilian() {
        return None;
    }
    let _ = only_to_heal;
    let here = pillage_here(u);
    if matches!(here, Step::Act(_)) {
        return Some(here);
    }
    let view = &u.local().view;
    let camp = crate::systems::barbarians::camp_improvement(u.rules());
    let ruins = crate::ai::scouting::ruins_improvement(u.rules());
    let tiles: Vec<TileId> = u
        .reachable()
        .iter()
        .filter(|(t, left)| *left > 0 && *t != u.tile() && u.can_move_to(*t))
        .map(|(t, _)| *t)
        .filter(|t| {
            let known = view.tile(*t);
            let Some(s) = known.snapshot() else {
                return false;
            };
            let enemy_land = s.owner.is_some_and(|o| u.at_war(o));
            enemy_land && !s.city && s.improvement.is_some() && s.improvement != camp && s.improvement != ruins && !view.known_pillaged(*t)
        })
        .collect();
    let best = tiles.into_iter().max_by_key(|t| (u.defensive_bonus(*t), std::cmp::Reverse(*t)))?;
    memo.pillage = Some((best, u.sight.round));
    Some(move_to_tile(u, best, Reason::Heal))
}

/// Pillage the tile the unit stands on, when the rules accept it.
fn pillage_here(u: &Unit<'_, '_>) -> Step {
    let pillage = Command::Pillage { unit: u.me.handle };
    let enemy_land = u.owner_of(u.tile()).is_some_and(|o| u.at_war(o));
    if enemy_land && u.local().view.accepts(&pillage) {
        return Step::Act(Decision::act(actor(&u.me), Reason::Heal, pillage));
    }
    Step::Next
}

/// `tryAdvanceTowardsCloseEnemy`: toward the nearest enemy the unit could
/// strike (one that would not kill it), within 5 tiles.
fn try_advance_towards_close_enemy(u: &Unit<'_, '_>, tools: &mut PlanTools<'_>) -> Step {
    let near: Vec<TileId> = crate::math::hex_disc(u.dims(), u.tile(), CLOSE_ENEMY_TILES_AWAY_LIMIT).collect();
    let close = attackable_enemies(u, Some(&near), false)
        .into_iter()
        .filter(|a| !(u.kind.is_ranged() && u.foreign_city_at(a.target).is_some_and(|c| c.hp == Some(1))))
        .filter(|a| u.damage_from_terrain(a.from) <= 0)
        .filter(|a| a.preview.to_attacker.1 < u.health())
        .min_by_key(|a| (u.distance(a.target, u.tile()), a.target, a.from));
    match close {
        Some(a) => head_towards(u, tools, a.from, Reason::Advance),
        None => Step::Next,
    }
}

/// `tryPrepare`: to the own city nearest a hostile civilization's (at war, or
/// denounced either way).
fn try_prepare(u: &Unit<'_, '_>, tools: &mut PlanTools<'_>) -> Step {
    let hostile = |owner: crate::ids::PlayerId| u.local().relation(owner).is_some_and(|r| r.at_war || r.we_denounced || r.they_denounced);
    let pairs: Vec<(TileId, TileId)> = u
        .sight
        .own_cities
        .iter()
        .flat_map(|ours| u.sight.foreign_cities.iter().filter(move |theirs| u.distance(ours.tile, theirs.tile) <= 12).map(move |theirs| (ours.tile, theirs.tile, theirs.owner)))
        .filter(|(_, _, owner)| hostile(*owner))
        .map(|(a, b, _)| (a, b))
        .collect();
    let Some(closest) = pairs.iter().map(|(a, b)| u.distance(*a, *b)).min() else {
        return Step::Next;
    };
    let mut to_defend: Vec<TileId> = pairs.iter().filter(|(a, b)| u.distance(*a, *b) <= closest + 2).map(|(a, _)| *a).collect();
    to_defend.sort_by_key(|t| (u.distance(u.tile(), *t), *t));
    to_defend.dedup();
    for city in to_defend {
        if u.distance(u.tile(), city) <= 2 {
            return Step::Done(Reason::Rally);
        }
        let mut near: Vec<TileId> = crate::math::hex_disc(u.dims(), city, 2).collect();
        near.sort_by_key(|t| (u.distance(*t, city), *t));
        if let Some(tile) = near.into_iter().find(|t| u.can_move_to(*t) && u.can_reach(tools, *t)) {
            return head_towards(u, tools, tile, Reason::Rally);
        }
    }
    Step::Next
}

/// `tryAccompanySettlerOrGreatPerson`.
fn try_accompany_settler_or_great_person(u: &Unit<'_, '_>, tools: &mut PlanTools<'_>) -> Step {
    let rules = u.rules();
    let found = u.own_units().iter().find(|o| {
        let kind = Kind { rules, kind: o.kind };
        kind.is_civilian()
            && (kind.founds_cities() || (kind.great_person() && !kind.war_great_person()))
            && (o.tile == u.tile() || (u.own_military_on(o.tile).is_none() && u.can_move_to(o.tile)))
            && (o.tile == u.tile() || u.reaches_now(o.tile))
    });
    match found {
        Some(o) => head_towards(u, tools, o.tile, Reason::Escort),
        None => Step::Next,
    }
}

/// `tryHeadTowardsOurSiegedCity`.
fn try_head_towards_our_sieged_city(u: &Unit<'_, '_>, tools: &mut PlanTools<'_>) -> Step {
    let sieged: Vec<TileId> = u.sight.own_cities.iter().filter(|c| c.damaged).map(|c| c.tile).collect();
    if sieged.iter().any(|c| u.distance(*c, u.tile()) <= 2) {
        return Step::Next;
    }
    let dims = u.dims();
    let mut ring: Vec<TileId> = sieged.iter().flat_map(|c| crate::math::hex_disc(dims, *c, 2).filter(move |t| dims.distance(*t, *c) == Some(2))).collect();
    ring.sort_by_key(|t| (u.distance(*t, u.tile()), *t));
    ring.dedup();
    let goal = ring.into_iter().take(tools.settings.budget.candidates as usize).find(|t| u.can_move_to(*t) && u.damage_from_terrain(*t) <= 0 && u.can_reach(tools, *t));
    match goal {
        Some(tile) => head_towards(u, tools, tile, Reason::Garrison),
        None => Step::Next,
    }
}

/// `tryTakeBackCapturedCity`: a city we founded, taken by an enemy and still
/// damaged (Unciv: in resistance), the nearest reachable.
fn try_take_back_captured_city(u: &Unit<'_, '_>, state: &AiState, tools: &mut PlanTools<'_>) -> Step {
    let founded = super::founded_tiles(state);
    let mut cities: Vec<super::sight::KnownCity> = u
        .sight
        .foreign_cities
        .iter()
        .copied()
        .filter(|c| u.at_war(c.owner) && founded.contains(&c.tile))
        .filter(|c| c.hp.is_some_and(|hp| hp < crate::world::city::CITY_BASE_HIT_POINTS))
        .collect();
    if u.kind.is_ranged() {
        cities.retain(|c| c.hp != Some(1));
    }
    cities.sort_by_key(|c| (u.distance(c.tile, u.tile()), c.tile));
    match cities.into_iter().find(|c| u.approach(tools, c.tile).is_some()) {
        Some(city) => head_towards_enemy_city(u, state, tools, city),
        None => Step::Next,
    }
}

/// `tryGarrisoningLandUnit`.
fn try_garrisoning_land_unit(u: &Unit<'_, '_>, tools: &mut PlanTools<'_>) -> Step {
    if u.kind.is_water() {
        return Step::Next;
    }
    let without: Vec<(TileId, bool)> = u
        .sight
        .own_cities
        .iter()
        .filter(|c| u.own_military_on(c.tile).is_none() || c.tile == u.tile())
        .filter(|c| c.tile == u.tile() || u.can_move_to(c.tile))
        .map(|c| (c.tile, c.damaged))
        .collect();
    let on_city = u.sight.own_cities.iter().find(|c| c.tile == u.tile()).copied();
    let at_war = u.sight.is_at_war(&u.local().relations);
    let mut to_try: Vec<TileId> = if !at_war {
        if on_city.is_some() {
            return Step::Done(Reason::Garrison);
        }
        without.iter().map(|(t, _)| *t).collect()
    } else {
        if let Some(city) = on_city {
            if needs_defending_in_wartime(u, city.tile, city.damaged) {
                return Step::Done(Reason::Garrison);
            }
        }
        let needing: Vec<TileId> = without.iter().filter(|(t, d)| *t != u.tile() && needs_defending_in_wartime(u, *t, *d)).map(|(t, _)| *t).collect();
        if needing.is_empty() {
            without.iter().map(|(t, _)| *t).filter(|t| *t != u.tile()).collect()
        } else {
            needing
        }
    };
    to_try.retain(|t| *t != u.tile());
    to_try.sort_by_key(|t| (u.distance(*t, u.tile()), *t));
    match to_try.into_iter().find(|t| u.can_reach(tools, *t)) {
        Some(city) => head_towards(u, tools, city, Reason::Garrison),
        None => Step::Next,
    }
}

/// `isCityThatNeedsDefendingInWartime`: under attack, or within 5 tiles of an
/// enemy city at war.
fn needs_defending_in_wartime(u: &Unit<'_, '_>, city: TileId, damaged: bool) -> bool {
    damaged || u.sight.foreign_cities.iter().any(|c| u.at_war(c.owner) && u.local().relation(c.owner).is_some_and(|r| r.kind != crate::world::player::PlayerKind::Module(crate::world::player::ClassicKind::Barbarian)) && u.distance(c.tile, city) <= 5)
}
