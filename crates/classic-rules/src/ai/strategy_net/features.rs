//! Layer 0, the situation: about fifty numbers 0…1 (thousandths) from the
//! player's observation and the rules (`weights::FEATURES` gives each one's
//! scale). The land survey (`Survey`: the land around the capital, the room
//! to expand, the rivals and city-states, the ruins and the caravans, the
//! gaps to the rivals) changes slowly and is taken every
//! `weights::goal::SURVEY_ROUNDS`; the rest every turn.

use super::layers::{ramp, unit, ONE};
use super::weights::fe::*;
use super::Milli;
use crate::ai::planner::{role_of, AiRole, TurnBrief};
use crate::ai::planner_settle::{SiteContext, SITE_MIN_SCORE};
use crate::ai::state::AiState;
use crate::derived::tile_yields::{plot_yields, TechSet};
use crate::ids::{PlayerId, TechId, TileId};
use crate::knowledge::observation::AIObservation;
use crate::knowledge::public::PlayerQueries;
use crate::math::hex_disc;
use crate::rules::CompiledRuleset;
use crate::world::facts::TileFacts;
use crate::world::player::PlayerKind;

/// The features the survey keeps, in this order.
pub const SURVEY_FEATURES: [usize; SURVEYED] =
    [CAP_FOOD, CAP_PROD, CAP_LUX, ROOM, SITES_FOOD, RIVAL_NEAR, RIVAL_KNOWN, RUINS, CARAVAN_FOOD, CITY_STATES, CS_NEAR, COASTAL, TECH_GAP, TECH_LEAD, SCORE_GAP, UNIQUE_MILITARY];
pub const SURVEYED: usize = 16;

/// Tiles of the capital's best tiles the land is judged by (a city of
/// about this size).
pub const BEST_TILES: usize = 12;
/// The radius in which free sites are counted.
pub const ROOM_RADIUS: i32 = 10;
/// Tiles between two sites.
pub const SITE_SPACING: i32 = 4;
/// Sites counted at most.
pub const ROOM_SITES: usize = 6;
/// The radius in which ruins near home are counted.
pub const RUINS_RADIUS: i32 = 12;
/// The radius in which a city-state is near.
pub const CITY_STATE_NEAR: i32 = 12;
/// Barbarian units this near a city press it.
pub const BARBARIAN_NEAR: i32 = 6;
/// Hostile units this near a city count against our force.
pub const HOSTILE_NEAR: i32 = 8;

/// The slow part of the situation and when it was taken.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Survey {
    pub round: u32,
    pub cities: u32,
    pub values: [Milli; SURVEYED],
}

/// A situation of neutral values with `pairs` set (scenario tests): the
/// traits at 5 of 10, a content empire, nothing hostile in sight.
pub fn neutral(pairs: &[(usize, Milli)]) -> [Milli; N] {
    let mut out = [0; N];
    for t in [T_EXPANSION, T_WONDERS, T_SCIENCE, T_CULTURE, T_GOLD, T_RELIGION, T_AGGRESSION, T_WAR_TOLERANCE, T_DIPLOMACY] {
        out[t] = 500;
    }
    out[FORCE] = ONE;
    out[HAPPY] = 700;
    out[GOLD] = 300;
    out[CITIES] = 125;
    out[CITIZENS] = 300;
    out[SCIENCE] = 500;
    out[PRODUCTION] = 300;
    for (i, v) in pairs {
        out[*i] = *v;
    }
    out
}

/// The capital's tile (else the first city's).
pub fn capital_tile(obs: &AIObservation<'_>) -> Option<TileId> {
    let capital = obs.view.own_capital();
    obs.own_cities.iter().find(|c| Some(c.handle) == capital).or(obs.own_cities.first()).map(|c| c.tile)
}

/// The yields of a tile as a slot array (food, production, gold, science,
/// culture, faith).
pub fn slots(y: crate::rules::content::Yields) -> [i64; 6] {
    [i64::from(y.food), i64::from(y.production), i64::from(y.gold), i64::from(y.science), i64::from(y.culture), i64::from(y.module)]
}

/// Mean food and production (hundredths) of the best `BEST_TILES` tiles a
/// city at `centre` could work (known, ours or nobody's), by 3 food + 2
/// production + 1 gold.
pub fn best_tiles(facts: &impl TileFacts, rules: &CompiledRuleset, techs: &TechSet, centre: TileId, player: PlayerId) -> (i64, i64) {
    let mut tiles: Vec<(i64, i64, i64)> = hex_disc(facts.dimensions(), centre, crate::world::city::CITY_WORK_RADIUS)
        .filter(|t| *t != centre && facts.terrain(*t).is_some() && facts.owner(*t).is_none_or(|o| o == player))
        .map(|t| {
            let y = slots(plot_yields(facts, rules, t, techs));
            (y[0] * 3 + y[1] * 2 + y[2], y[0], y[1])
        })
        .collect();
    tiles.sort_by_key(|(w, _, _)| std::cmp::Reverse(*w));
    tiles.truncate(BEST_TILES);
    let n = tiles.len().max(1) as i64;
    (tiles.iter().map(|t| t.1).sum::<i64>() * 100 / n, tiles.iter().map(|t| t.2).sum::<i64>() * 100 / n)
}

/// Food (hundredths a tile) → the land scale (1.2 → 0, 3.0 → 1).
pub fn food_scale(food: i64) -> Milli {
    ramp(food, 120, 300)
}

/// Production (hundredths a tile) → 0.5 → 0, 2.0 → 1.
pub fn production_scale(production: i64) -> Milli {
    ramp(production, 50, 200)
}

/// Luxury resources within the work radius of `centre`.
pub fn luxuries(facts: &impl TileFacts, rules: &CompiledRuleset, centre: TileId) -> i64 {
    hex_disc(facts.dimensions(), centre, crate::world::city::CITY_WORK_RADIUS)
        .filter(|t| {
            facts
                .resource(*t)
                .and_then(|r| rules.resources.get(r.0 as usize))
                .is_some_and(|d| d.class == Some(crate::rules::content::ResourceClass::Luxury))
        })
        .count() as i64
}

/// The order of an era in the game (the rules' `index`; an era's id is its
/// row, which is not the game's order).
pub fn era_index(rules: &CompiledRuleset, era: crate::rules::content::EraId) -> usize {
    usize::from(crate::derived::culture::era_index(rules, era))
}

/// The era of a set of technologies: the latest content era among them
/// (the era's order, as `met_eras` gives the rivals').
pub fn era_of(rules: &CompiledRuleset, techs: &[TechId]) -> usize {
    techs
        .iter()
        .filter_map(|t| rules.technologies.get(usize::from(t.0)))
        .filter_map(|d| d.content_era)
        .map(|e| era_index(rules, e))
        .max()
        .unwrap_or(0)
}

/// The land survey of a nation.
pub fn survey(obs: &AIObservation<'_>, rules: &CompiledRuleset, state: &AiState) -> Survey {
    let mut values = [0; SURVEYED];
    let mut set = |feature: usize, v: Milli| {
        if let Some(at) = SURVEY_FEATURES.iter().position(|f| *f == feature) {
            values[at] = v;
        }
    };
    let techs = TechSet::of(&obs.economy.techs);
    let player = obs.player;
    let dims = obs.view.dimensions();
    let is_major = |p: PlayerId| obs.relations.iter().any(|r| r.other == p && r.kind == PlayerKind::Major);
    let is_city_state = |p: PlayerId| obs.city_states.iter().any(|c| c.player == p);
    if let Some(capital) = capital_tile(obs) {
        let (food, production) = best_tiles(&obs.view, rules, &techs, capital, player);
        set(CAP_FOOD, food_scale(food));
        set(CAP_PROD, production_scale(production));
        set(CAP_LUX, unit(luxuries(&obs.view, rules, capital) * ONE / 3));
        set(COASTAL, if obs.view.coastal_land(capital) { ONE } else { 0 });
        // Room: free sites around the capital, the best first, apart.
        let own: Vec<TileId> = obs.own_cities.iter().map(|c| c.tile).collect();
        let foreign: Vec<TileId> = state.seen_cities().iter().map(|c| c.tile).collect();
        let ctx = SiteContext {
            view: &obs.view,
            profile: Default::default(),
            rules,
            player,
            own_cities: &own,
            foreign_cities: &foreign,
            reserved: &[],
            danger: &[],
            tile_memo: Default::default(),
        };
        let mut sites: Vec<(i32, TileId)> = hex_disc(dims, capital, ROOM_RADIUS).filter_map(|t| ctx.score(t).filter(|s| *s >= SITE_MIN_SCORE).map(|s| (s, t))).collect();
        sites.sort_by_key(|(s, t)| (std::cmp::Reverse(*s), *t));
        let mut taken = own.clone();
        let mut best: Vec<TileId> = Vec::new();
        for (_, t) in sites {
            if best.len() >= ROOM_SITES {
                break;
            }
            let apart = taken.iter().chain(&foreign).all(|o| dims.distance(*o, t).is_some_and(|d| d >= SITE_SPACING));
            if apart {
                taken.push(t);
                best.push(t);
            }
        }
        set(ROOM, best.len() as i64 * ONE / ROOM_SITES as i64);
        let fed: Vec<i64> = best.iter().take(4).map(|t| best_tiles(&obs.view, rules, &techs, *t, player).0).collect();
        if !fed.is_empty() {
            set(SITES_FOOD, food_scale(fed.iter().sum::<i64>() / fed.len() as i64));
        }
        // The nearest known city of a rival nation.
        let rival = state.seen_cities().iter().filter(|c| is_major(c.owner)).filter_map(|c| dims.distance(capital, c.tile)).min();
        set(RIVAL_KNOWN, if rival.is_some() { ONE } else { 0 });
        set(RIVAL_NEAR, rival.map_or(0, |d| ramp(18 - i64::from(d), 0, 11)));
        // City-states with a city near the capital.
        let mut near: Vec<PlayerId> = state
            .seen_cities()
            .iter()
            .filter(|c| is_city_state(c.owner) && dims.distance(capital, c.tile).is_some_and(|d| d <= CITY_STATE_NEAR))
            .map(|c| c.owner)
            .collect();
        near.sort_by_key(|p| p.0);
        near.dedup();
        set(CS_NEAR, unit(near.len() as i64 * ONE / 2));
        // Ruins near home.
        if let Some(ruins) = crate::ai::scouting::ruins_improvement(rules) {
            let n = hex_disc(dims, capital, RUINS_RADIUS).filter(|t| obs.view.improvement(*t) == Some(ruins)).count() as i64;
            set(RUINS, unit(n * 250));
        }
        // Internal food routes: pairs of cities (and planned sites with the
        // capital) within the land range, × the food a route carries.
        let mut pairs = 0i64;
        for (i, a) in own.iter().enumerate() {
            for b in &own[i + 1..] {
                if dims.distance(*a, *b).is_some_and(|d| d <= crate::derived::trade::LAND_RANGE) {
                    pairs += 1;
                }
            }
        }
        pairs += best.iter().take(3).filter(|t| dims.distance(capital, **t).is_some_and(|d| d <= crate::derived::trade::LAND_RANGE)).count() as i64;
        let era = era_of(rules, &obs.economy.techs) as u32;
        let citizens = (obs.own_cities.iter().map(|c| c.population).sum::<u32>() / obs.own_cities.len().max(1) as u32).max(3);
        let food = pairs.min(4) * i64::from(crate::derived::trade::internal_amount(era, citizens));
        set(CARAVAN_FOOD, unit(food * ONE / 800));
    }
    set(CITY_STATES, unit(obs.city_states.len() as i64 * ONE / 4));
    // The gaps to the rivals: eras and score.
    let ours = era_of(rules, &obs.economy.techs) as i64;
    let theirs = obs.view.met_eras().iter().map(|(_, e)| i64::from(*e)).max();
    if let Some(theirs) = theirs {
        set(TECH_GAP, unit((theirs - ours).max(0) * ONE / 2));
        set(TECH_LEAD, unit((ours - theirs).max(0) * ONE / 2));
    }
    let race = obs.view.race();
    let mine = race.scores.iter().find(|(p, _)| *p == player).map_or(0, |(_, s)| *s);
    if let Some(best) = race.scores.iter().filter(|(p, _)| *p != player).map(|(_, s)| *s).max() {
        set(SCORE_GAP, unit((best - mine).max(0) * ONE / mine.max(1)));
    }
    // Unique military units of the first two eras.
    let nation = obs.economy.nation;
    let uniques = rules
        .units
        .iter()
        .enumerate()
        .filter(|(i, d)| {
            d.nation == Some(nation)
                && role_of(rules, crate::ids::UnitKindId(*i as u16)) == AiRole::Military
                && d.required_tech.and_then(|t| rules.technologies.get(usize::from(t.0))).and_then(|t| t.content_era).is_none_or(|e| era_index(rules, e) <= 1)
        })
        .count() as i64;
    set(UNIQUE_MILITARY, unit(uniques * ONE));
    Survey { round: obs.round, cities: obs.own_cities.len() as u32, values }
}

/// The situation of a turn: the survey and the turn's own features.
pub fn situation(obs: &AIObservation<'_>, brief: &TurnBrief, rules: &CompiledRuleset, survey: &Survey) -> [Milli; N] {
    let mut out = [0; N];
    for (at, feature) in SURVEY_FEATURES.iter().enumerate() {
        out[*feature] = survey.values[at];
    }
    let cities = obs.own_cities.len() as i64;
    let citizens: i64 = obs.own_cities.iter().map(|c| i64::from(c.population)).sum();
    let eras = rules.eras.len().max(2) as i64 - 1;
    out[ERA] = unit(era_of(rules, &obs.economy.techs) as i64 * ONE / eras);
    let last = i64::from(obs.victories.max_rounds.max(1));
    out[PROGRESS] = unit(i64::from(obs.round) * ONE / last);
    out[CITIES] = unit(cities * ONE / 8);
    out[CITIZENS] = unit(citizens * ONE / (cities.max(1) * 10));
    out[SCIENCE] = unit(i64::from(brief.science.max(0)) * ONE * 2 / (citizens.max(1) * 3));
    let production: i64 = obs.own_cities.iter().map(|c| i64::from(c.income.production.max(0))).sum();
    out[PRODUCTION] = unit(production * ONE / (cities.max(1) * 15));
    out[GOLD] = ramp(i64::from(brief.gold_income), -5, 30);
    let at_war = brief.relations.iter().any(|r| r.at_war);
    out[AT_WAR] = if at_war { ONE } else { 0 };
    // Force: ours against the hostile power in sight near our cities.
    let near = |tile: TileId, reach: i32| obs.own_cities.iter().any(|c| obs.view.dimensions().distance(c.tile, tile).is_some_and(|d| d <= reach));
    let is_major = |p: PlayerId| brief.relations.iter().any(|r| r.other == p);
    let is_city_state = |p: PlayerId| obs.city_states.iter().any(|c| c.player == p);
    let hostile: i64 = brief
        .sightings
        .iter()
        .filter(|s| s.military && near(s.tile, HOSTILE_NEAR) && (brief.at_war(s.owner) || !is_major(s.owner) && !is_city_state(s.owner)))
        .map(|s| i64::from(s.power))
        .sum();
    let ours = i64::from(brief.military_power);
    out[FORCE] = if hostile == 0 { ONE } else { ours * ONE / (ours + hostile) };
    let endangered = brief.defence.as_ref().map_or(0, |w| obs.own_cities.iter().filter(|c| w.danger(c.tile).is_some()).count() as i64);
    out[THREAT] = unit(endangered * ONE / cities.max(1));
    out[HAPPY] = ramp(i64::from(brief.happiness), -10, 10);
    out[UNHAPPY] = ramp(-i64::from(brief.happiness), 0, 10);
    out[FAITH] = unit(i64::from(obs.economy.income.faith) * ONE / 8);
    out[RELIGION] = if obs.faith.overview.religion.is_some() { ONE } else { 0 };
    out[TOURISM] = match obs.tourism.opinion {
        crate::world::tourism::PublicOpinion::Content => 0,
        crate::world::tourism::PublicOpinion::Murmurs => 400,
        crate::world::tourism::PublicOpinion::Unrest => 700,
        crate::world::tourism::PublicOpinion::Upheaval => ONE,
    };
    let wonders = rules.buildings.iter().filter(|b| b.wonder && b.kind == crate::rules::content::BuildingKind::WorldWonder && !b.unbuildable).count() as i64;
    out[WONDERS_TAKEN] = unit(obs.built_wonders.len() as i64 * ONE / wonders.max(1));
    let ours_built = obs.own_cities.iter().flat_map(|c| c.buildings.iter()).filter(|b| rules.buildings.get(usize::from(b.0)).is_some_and(|d| d.wonder)).count() as i64;
    out[OUR_WONDERS] = unit(ours_built * ONE / 4);
    out[RIVALS] = unit(brief.relations.len() as i64 * ONE / 4);
    let p = rules.module.personality(obs.economy.nation);
    for (feature, value) in [
        (T_EXPANSION, p.expansion),
        (T_WONDERS, p.wonders),
        (T_SCIENCE, p.science),
        (T_CULTURE, p.culture),
        (T_GOLD, p.gold),
        (T_RELIGION, p.religion),
        (T_AGGRESSION, p.aggression),
        (T_WAR_TOLERANCE, p.war_tolerance),
        (T_DIPLOMACY, p.diplomacy),
    ] {
        out[feature] = unit(i64::from(value) * 100);
    }
    out[IDEOLOGY] = if obs.culture.ideology.is_some() { ONE } else { 0 };
    out[ROUTES_LEFT] = unit(i64::from(obs.economy.trade_routes_left) * ONE / 3);
    let camps = crate::ai::camps::known_camps(obs).len() as i64;
    let raiders = brief.sightings.iter().filter(|s| s.military && !is_major(s.owner) && !is_city_state(s.owner) && near(s.tile, BARBARIAN_NEAR)).count() as i64;
    out[BARBS] = unit(camps * 350 + raiders * 200);
    out
}
