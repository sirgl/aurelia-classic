//! Part A: the per-rival inputs of `net2` (design §2.3, R1–R24, and the facts of the war order). Every
//! met major rival (at most `MAX_RIVALS`, by id) has a row of 0…1000 values read from the seat's
//! observation, the turn's brief and the inputs' memory; nothing reads the world.
//!
//! The public war list (design §9, open point 1): R22 `ally_hit` stays 0. The rules announce a war
//! only to the nation it is declared on (`classic-ui` `game/news.rs`: `enemy == me`), the observation
//! carries only the seat's own relations (`Relation`), and no screen lists the wars of two other
//! nations; a list added to the observation would show the AI what a human cannot see.

use crate::ai::net2::inputs::{InputMemory, SoldierRead, Survey, GRAB_ROUNDS, GRAB_TILES, SIGHT_ROUNDS, SURVEY_ROUNDS, WAR_RAMP_ROUNDS};
use crate::ai::operations::{Arsenal, Routes, BORDER_REACH, DECISIVE_RANGE, STAGING_DISTANCE};
use crate::ai::planner::{unit_power, AiRole, TurnBrief};
use crate::ai::planner_strategy::THREAT_RADIUS;
use crate::ai::rival_model::{Rival, Rivals, SoldiersEvidence};
use crate::ai::state::AiState;
use crate::ai::strategy_net::layers::ramp;
use crate::ai::strategy_net::weights::threat as threat_w;
use crate::ai::strategy_net::Milli;
use crate::derived::standing::Measure;
use crate::ids::{PlayerId, TileId};
use crate::knowledge::observation::AIObservation;
use crate::rules::CompiledRuleset;
use crate::world::player::PlayerKind;

use super::plan::{RivalInputs, MAX_RIVALS};

/// Every city of another nation the seat knows (in sight or remembered): tile, owner, in sight now
/// (the memory's `foreign`, written by `InputMemory::observe` from the sight and the state's
/// `seen_cities`).
pub fn known_cities(obs: &AIObservation<'_>, _state: &AiState, memory: &InputMemory) -> Vec<(TileId, PlayerId, bool)> {
    memory.foreign.iter().map(|c| (c.tile, c.owner, obs.visible_foreign_cities.iter().any(|v| v.tile == c.tile))).collect()
}

/// `theirs` over `theirs + ours`: 500 level, 0 nothing of theirs, 1000 nothing of ours; `none` when both
/// are 0.
fn share(theirs: i64, ours: i64, none: Milli) -> Milli {
    let (theirs, ours) = (theirs.max(0), ours.max(0));
    if theirs + ours == 0 {
        none
    } else {
        theirs * 1000 / (theirs + ours)
    }
}

/// Every met major rival's inputs, by id, at most `MAX_RIVALS`.
pub fn read(obs: &AIObservation<'_>, brief: &TurnBrief, state: &AiState, rules: &CompiledRuleset, model: &Rivals, memory: &mut InputMemory) -> Vec<RivalInputs> {
    let now = obs.round;
    let me = obs.player;
    let met: Vec<&crate::knowledge::observation::Relation> = obs.relations.iter().filter(|r| r.kind == PlayerKind::Major).take(MAX_RIVALS).collect();
    let soldiers = soldier_reads(obs, model, &met, memory);
    memory.soldiers = soldiers.clone();
    let known = known_cities(obs, state, memory);
    let own_tiles: Vec<TileId> = obs.own_cities.iter().map(|c| c.tile).collect();
    let dist = |a: TileId, b: TileId| brief.dims.distance(a, b).unwrap_or(i32::MAX);
    let own_power = i64::from(brief.military_power);
    let sight = obs.speed.pace_rounds(SIGHT_ROUNDS);
    let war_ramp = obs.speed.pace_rounds(WAR_RAMP_ROUNDS);
    let grab_window = obs.speed.pace_rounds(GRAB_ROUNDS);
    let citadel = crate::systems::combat::actions::citadel_of(rules);
    let capitals = &obs.public.capitals;
    let mut out = Vec::with_capacity(met.len());
    for relation in met {
        let r = relation.other;
        let mut row = RivalInputs::stranger(r);
        let theirs: Vec<(TileId, bool)> = known.iter().filter(|(_, o, _)| *o == r).map(|(t, _, v)| (*t, *v)).collect();
        // R1: the pair of our nearest city and its nearest seen city.
        let nearest = own_tiles
            .iter()
            .flat_map(|a| theirs.iter().map(move |(b, _)| (*a, *b)))
            .min_by_key(|(a, b)| (dist(*a, *b), *b, *a));
        if let Some((from, target)) = nearest {
            let d = dist(from, target);
            row.near = ramp(15 - i64::from(d), 0, 10);
            let (land, sea) = survey(obs, brief, rules, memory, r, from, target, now);
            row.land = if land { 1000 } else { 0 };
            row.sea = if sea { 1000 } else { 0 };
            // R23: our free power within 6 tiles of its nearest seen city over that + its sighted power there.
            let ours: i64 = brief
                .units
                .iter()
                .filter(|u| u.role == AiRole::Military && crate::ai::net2::inputs::is_free(state, u.id) && dist(u.tile, target) <= 6)
                .map(|u| i64::from(unit_power(rules, u.kind, u.hp)))
                .sum();
            let theirs_there: i64 = brief.sightings.iter().filter(|s| s.owner == r && s.military && dist(s.tile, target) <= 6).map(|s| i64::from(s.power)).sum();
            row.local = if ours == 0 { 0 } else { share(ours, theirs_there, 1000) };
        }
        // R4: its soldiers by the fixed estimator over its + ours.
        if let Some(read) = soldiers.iter().find(|s| s.player == r) {
            row.soldiers = share(read.value, model.own[soldiers_row()], 500);
        }
        // R5: the army we sighted in the window, decayed, over itself + ours.
        row.seen_army = share(memory.seen_power(r, now, sight), own_power, 0);
        // R6: the share of its seen cities last seen with no military unit on them or next to them.
        let seen_open: Vec<bool> = memory.foreign.iter().filter(|c| c.owner == r).filter_map(|c| c.open).collect();
        if !seen_open.is_empty() {
            row.open_cities = (seen_open.iter().filter(|o| **o).count() * 1000 / seen_open.len()) as Milli;
        }
        // R7–R9: the runaway's three inputs, as `threat::of` reads them.
        if let Some(rival) = model.get(r) {
            let (tree, lead, pace) = race(model, rival);
            row.tree = tree;
            row.lead = lead;
            row.pace = pace;
        }
        // R10: its culture's level on us over both levels.
        if let Some(v) = obs.tourism.influence.iter().find(|v| v.other == r) {
            row.culture = share(i64::from(v.their_level.index()), i64::from(v.level.index()), 0);
        }
        // R11: at peace, its military power inside our borders or near our cities, over ours there.
        row.stack = if relation.at_war { 0 } else { stack(obs, brief, rules, &known, r) };
        // R12–R17.
        row.warmonger = i64::from(relation.warmonger).saturating_mul(1000).div_euclid(i64::from(crate::ai::diplomacy::DENOUNCE_WARMONGER)).clamp(0, 1000);
        row.betrayed = ramp(i64::from(memory.ties_of(r).map_or(0, |t| t.betrayals)), 0, 2);
        row.denounced_us = if relation.they_denounced { 1000 } else { 0 };
        row.friend = if relation.friends { 1000 } else { 0 };
        row.pact = if relation.defensive_pact { 1000 } else { 0 };
        row.at_war = if relation.at_war { 1000 } else { 0 };
        // R18: paced rounds of the war with it.
        row.war_rounds = if relation.at_war { ramp(i64::from(now.saturating_sub(relation.war_since)), 0, i64::from(war_ramp)) } else { 0 };
        // R19, R20: our capital and our lost cities in its hands.
        row.holds_capital = if capitals.iter().any(|c| c.founder == me && c.holder == r) { 1000 } else { 0 };
        row.holds_cities = ramp(memory.lost.iter().filter(|l| l.captor == Some(r)).count() as i64, 0, 2);
        // R21: its cities founded near ours within the window, a citadel of it by our land.
        let forward = memory
            .foreign
            .iter()
            .filter(|c| c.owner == r && c.first_seen > 0 && now.saturating_sub(c.first_seen) <= grab_window)
            .filter(|c| own_tiles.iter().any(|t| dist(*t, c.tile) <= GRAB_TILES && memory.own_since(*t).is_some_and(|own| own <= c.first_seen)))
            .count() as i64;
        let citadels = citadels_by_our_land(obs, brief, &own_tiles, citadel, r);
        row.grab = (forward * 1000 + citadels * 500).clamp(0, 1000);
        // R22: 0, the rules announce no war between others (module doc).
        row.ally_hit = 0;
        // R24: its score over its + ours.
        if let (Some(theirs), Some(ours)) = (obs.public.score_of(r), obs.public.score_of(me)) {
            row.score = share(theirs, ours, 500);
        }
        row.retake_case = if brief.retaking_from(r) { 1000 } else { 0 };
        row.captured_recent = if memory.captured.iter().any(|c| c.from == r && now.saturating_sub(c.round) <= sight) { 1000 } else { 0 };
        row.operating = if state.operations().iter().any(|o| o.enemy == r) { 1000 } else { 0 };
        row.alone = alone(obs, r);
        // R31: the city-states allied to it, which join a war declared on it (`minor_allies_join_war`).
        row.cs_held = ramp(obs.city_states.iter().filter(|c| c.ally == Some(r)).count() as i64, 0, 3);
        out.push(row);
    }
    out
}

/// R32: the league elects a World Leader and `rival`'s delegates on the Congress screen reach the votes needed.
fn alone(obs: &AIObservation<'_>, rival: PlayerId) -> Milli {
    let Some(info) = obs.congress.as_ref().filter(|c| c.united_nations && c.needed > 0) else { return 0 };
    let delegates = info.delegates.iter().find(|(p, _)| *p == rival).map_or(0, |(_, n)| *n);
    if delegates >= info.needed {
        1000
    } else {
        0
    }
}

/// R7, R8, R9 of one rival (`strategy_net::threat::of` reads the same three).
fn race(model: &Rivals, rival: &Rival) -> (Milli, Milli, Milli) {
    let total = model.techs_total.max(1);
    let tree = (rival.techs.value * 1000 / total).clamp(0, 1000);
    let lead = ((rival.techs.value - model.own_techs) * 1000 * threat_w::LEAD_SCALE / total).clamp(0, 1000);
    let pace = match rival.science {
        Some(theirs) if theirs + model.own_science > 0 => theirs.max(0) * 1000 / (theirs.max(0) + model.own_science.max(0)).max(1),
        _ => 500,
    };
    (tree, lead, pace)
}

fn soldiers_row() -> usize {
    Measure::ALL.iter().position(|m| *m == Measure::Soldiers).unwrap_or(0)
}

/// The fixed estimate of every met rival's soldiers (`rival_model::soldiers`) from the demographics row,
/// the units sighted in the window and the cities seen.
fn soldier_reads(obs: &AIObservation<'_>, model: &Rivals, met: &[&crate::knowledge::observation::Relation], memory: &InputMemory) -> Vec<SoldierRead> {
    let Some(row) = obs.public.row(Measure::Soldiers) else { return Vec::new() };
    let window = obs.speed.pace_rounds(SIGHT_ROUNDS);
    let evidence: Vec<SoldiersEvidence> = met
        .iter()
        .map(|r| SoldiersEvidence {
            player: r.other,
            lower: memory.seen_units(r.other, obs.round, window),
            score: obs.public.score_of(r.other).unwrap_or(0),
        })
        .collect();
    let guesses = crate::ai::rival_model::soldiers(row, model.living, &evidence);
    evidence.iter().zip(guesses).map(|(e, g)| SoldierRead { player: e.player, value: g.value, lo: g.lo, hi: g.hi }).collect()
}

/// The route survey for R2 and R3, kept per rival for `SURVEY_ROUNDS` and while our cities and its
/// nearest city stay the same (`Routes::stage` is the costly search).
fn survey(obs: &AIObservation<'_>, brief: &TurnBrief, rules: &CompiledRuleset, memory: &mut InputMemory, r: PlayerId, from: TileId, target: TileId, now: u32) -> (bool, bool) {
    let cities = obs.own_cities.len().min(usize::from(u16::MAX)) as u16;
    let due = obs.speed.pace_rounds(SURVEY_ROUNDS);
    if let Some(s) = memory.surveys.iter().find(|s| s.player == r) {
        if s.our_cities == cities && s.target == target && now.saturating_sub(s.round) < due {
            return (s.land, s.sea);
        }
    }
    // R2: the war machine's own test, from the nearest of our cities to its nearest seen city.
    let land = Arsenal::of(brief, rules).melee.is_some_and(|walker| Routes::new(obs.view).stage(walker, from, target, STAGING_DISTANCE, BORDER_REACH).is_some());
    // R3: not by land, its city on the coast, one of our coastal cities within range, and embarkation known.
    let embark = obs.economy.techs.iter().any(|t| rules.technologies.get(t.0 as usize).is_some_and(|d| d.allows_embarking));
    let sea = !land
        && embark
        && crate::ai::fleet::coastal(&obs.view, target)
        && obs.own_cities.iter().any(|c| crate::ai::fleet::coastal(&obs.view, c.tile) && brief.dims.distance(c.tile, target).is_some_and(|d| d <= DECISIVE_RANGE));
    let fresh = Survey { player: r, round: now, our_cities: cities, target, land, sea };
    match memory.surveys.iter_mut().find(|s| s.player == r) {
        Some(s) => *s = fresh,
        None => memory.surveys.push(fresh),
    }
    (land, sea)
}

/// R11: the power of `r`'s military units in the field inside our borders or within `THREAT_RADIUS` of
/// one of our cities, over that and the power of ours near the cities it stands by.
fn stack(obs: &AIObservation<'_>, brief: &TurnBrief, rules: &CompiledRuleset, known: &[(TileId, PlayerId, bool)], r: PlayerId) -> Milli {
    let me = obs.player;
    let dist = |a: TileId, b: TileId| brief.dims.distance(a, b).unwrap_or(i32::MAX);
    let mut theirs = 0i64;
    let mut by: Vec<TileId> = Vec::new();
    for s in brief.sightings.iter().filter(|s| s.owner == r && s.military) {
        // A garrison stands in its own city.
        if known.iter().any(|(t, o, _)| *t == s.tile && *o == r) {
            continue;
        }
        let nearest = obs.own_cities.iter().min_by_key(|c| (dist(c.tile, s.tile), c.tile));
        let close: Vec<TileId> = obs.own_cities.iter().filter(|c| dist(c.tile, s.tile) <= THREAT_RADIUS).map(|c| c.tile).collect();
        let ours_land = obs.view.tile(s.tile).snapshot().is_some_and(|t| t.owner == Some(me));
        if close.is_empty() && !ours_land {
            continue;
        }
        theirs += i64::from(s.power);
        if close.is_empty() {
            by.extend(nearest.map(|c| c.tile));
        } else {
            by.extend(close);
        }
    }
    if theirs == 0 {
        return 0;
    }
    by.sort();
    by.dedup();
    let ours: i64 = brief
        .units
        .iter()
        .filter(|u| u.role == AiRole::Military && by.iter().any(|c| dist(u.tile, *c) <= THREAT_RADIUS))
        .map(|u| i64::from(unit_power(rules, u.kind, u.hp)))
        .sum();
    share(theirs, ours, 0)
}

/// Citadels of `r` standing within two tiles of land that is ours (R21: 500 each; a citadel's culture
/// bomb takes the ring around it, so our land begins at the second tile), from the tiles around our
/// cities that we see or remember.
fn citadels_by_our_land(obs: &AIObservation<'_>, brief: &TurnBrief, own_tiles: &[TileId], citadel: Option<crate::ids::ImprovementId>, r: PlayerId) -> i64 {
    let Some(citadel) = citadel else { return 0 };
    let me = obs.player;
    let mut seen: Vec<TileId> = Vec::new();
    for c in own_tiles {
        for t in crate::math::hex_disc(brief.dims, *c, crate::world::city::CITY_WORK_RADIUS + 2) {
            if seen.contains(&t) {
                continue;
            }
            let Some(snap) = obs.view.tile(t).snapshot().copied() else { continue };
            if snap.improvement != Some(citadel) || snap.owner != Some(r) {
                continue;
            }
            let by_ours = crate::math::hex_disc(brief.dims, t, 2).any(|n| obs.view.tile(n).snapshot().is_some_and(|s| s.owner == Some(me)));
            if by_ours {
                seen.push(t);
            }
        }
    }
    seen.len() as i64
}
