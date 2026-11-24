//! The history and multi-front families (work/nets/TRAINERS-PLAN.md): the right act depends on what the army saw in the
//! rounds before (feint, fog-sniper, city-reinforced, scout-report) or on how it divides itself between two goals
//! (two-fronts, second-city). The win rules are fixed in work/nets/trainers-hist/GATES.md before any run.
//!
//! Every march scene is built so that WAITING one round for more evidence is too late: the enemy at its decision
//! round stands 4 tiles from each city it may attack (it strikes in the second round after), the army 4 tiles from
//! each city (two rounds of foot movement), so only a unit that leaves at the decision round arrives in time.
//! All units of the march scenes are foot units of movement 2 (a scene with other speeds is not built).
//!
//! - feint (no fog): two walled cities of ours (A, B), our army 4 tiles from both; an enemy force of four marches
//!   three rounds along a path whose last step points at A or at B (half the instances: the first legs pointed at the
//!   other city, a feint) and stands, at the decision round (round 3), 4 tiles from both. It then attacks the
//!   city its last step pointed at. WIN: no city of ours lost when the enemy is dead or 9 rounds end.
//! - scout-report (fog): three cities of ours 4 tiles from the army (mutually 4 apart); the enemy force marches
//!   along the ray to one of them; a small outpost of ours beside each of the three possible roads sees it in rounds
//!   0-2 only; at round 3 it is in the fog, 4 tiles from its target. WIN: as feint.
//! - fog-sniper (fog): an enemy bowman (the weakest ranged kind of range 2) on a hill, hidden by a ridge, shoots the
//!   weakest of our three units 2 tiles behind the ridge every round and is never seen from where our units stand.
//!   WIN: the sniper killed within 3 rounds. The strike log (`src`: the tile the last strike came from) is what the
//!   informed teacher reads; the blind one walks away from the group.
//! - city-reinforced (fog): our army marches on a worn-down city (55 % left, one garrison); a scout on a hill sees it
//!   for three rounds. Half the instances: in the enemy's turn of round 0 three strong melee enter the city (WIN: the
//!   city taken within 24 rounds with at most 2 of the army lost, the scout apart). Half: nothing enters but a wave
//!   of 2-3 strong melee lands at round N (5-7) (WIN: the city taken before N). The round limit is 24 in both.
//! - two-fronts (no fog): eight of ours, an enemy city worn down to 20 % to take (5 tiles east) and our capital (3 %
//!   left, 4 west) to hold against a force of three strong melee that appears 7 tiles west. WIN: our city kept, the
//!   enemy city taken and the force dead, within 7 rounds.
//! - second-city (no fog): the siege of the same city by seven; at round K (3-5) a force of four strong melee appears
//!   7 tiles west of our second city (a town). WIN: as two-fronts, within 16 rounds.
//! - twin-siege (no fog): two enemy cities 10 tiles apart, a strong one (walls, worn down to 40 %) and a weak one (5 %
//!   left), either west; our eleven stand between; a relief force of two strong melee marches to the city where more
//!   of ours stand. WIN: both cities taken within 12 rounds with at most 3 of ours lost.
//!
//! THE REFERENCES. Right teacher first (`Fam::solvers`), the wrong ones are played with `--players`:
//! - feint: ref-trace (last step of the leader), ref-split, ref-guess (nearest city: a tie goes to the lower tile),
//!   ref-first (the first legs), ref-wait (ref-trace one round late);
//! - scout-report: ref-extend (the road through the sightings), ref-spread, ref-stay (reacts when the enemy shows);
//! - fog-sniper: ref-source, ref-search;
//! - city-reinforced: ref-plan (the garrison the scout saw decides: careful `ref-planned` or `ref-burst`), ref-burst,
//!   ref-planned (the assault planners with the scout set apart);
//! - two-fronts: ref-share (the holders are as many as the enemy force), ref-pile (all on the siege), ref-even (half
//!   and half), ref-hold-first (all on the defence first);
//! - second-city: ref-detach (all go home while the force lives, then back to the siege), ref-ignore, ref-abandon (home for good);
//! - twin-siege: ref-twin (two armies split 6/8 to the strong city, a capturer at each), ref-serial (all on one city, then
//!   the other), ref-equal (alternate units).

use std::collections::BTreeMap;

use classic_rules::ids::TileId;
use classic_rules::{CompiledRuleset, GameState};
use kernel::world::unit::UnitCore;

use super::act::{self, Aim, Nerve};
use super::assault;
use super::families::{blank, defences, Fam, Painter};
use super::lab::{dist, tiles_within, Arm, Lab};
use super::memory;
use super::{Piece, Scene};

pub fn is_history(f: Fam) -> bool {
    matches!(f, Fam::Feint | Fam::FogSniper | Fam::CityReinforced | Fam::ScoutReport | Fam::TwoFronts | Fam::SecondCity | Fam::TwinSiege)
}

/// The families whose tokens hold only the foes our side sees (`army_net::Orders::fog`).
pub fn is_fog(f: Fam) -> bool {
    matches!(f, Fam::FogSniper | Fam::CityReinforced | Fam::ScoutReport)
}

pub fn rounds(f: Fam) -> u32 {
    match f {
        Fam::Feint | Fam::ScoutReport => 9,
        Fam::FogSniper => 3,
        Fam::CityReinforced => 24,
        Fam::TwoFronts => 7,
        Fam::TwinSiege => 12,
        _ => 16,
    }
}

pub fn solvers(f: Fam) -> &'static [&'static str] {
    match f {
        Fam::Feint => &["ref-trace"],
        Fam::ScoutReport => &["ref-extend"],
        Fam::FogSniper => &["ref-source"],
        Fam::CityReinforced => &["ref-plan"],
        Fam::TwoFronts => &["ref-share"],
        Fam::TwinSiege => &["ref-twin"],
        _ => &["ref-detach"],
    }
}

/// Every reference name each family plays (the right teacher first).
pub fn teachers(f: Fam) -> &'static [&'static str] {
    match f {
        Fam::Feint => &["ref-trace", "ref-split", "ref-guess", "ref-first", "ref-wait"],
        Fam::ScoutReport => &["ref-extend", "ref-spread", "ref-stay"],
        Fam::FogSniper => &["ref-source", "ref-search"],
        Fam::CityReinforced => &["ref-plan", "ref-burst", "ref-planned"],
        Fam::TwoFronts => &["ref-share", "ref-pile", "ref-even", "ref-hold-first"],
        Fam::TwinSiege => &["ref-twin", "ref-serial", "ref-equal"],
        _ => &["ref-detach", "ref-ignore", "ref-abandon"],
    }
}

pub fn task(f: Fam) -> &'static str {
    match f {
        Fam::Feint => "an enemy force marched three rounds, now equidistant from our two cities: hold the one its last step points at",
        Fam::FogSniper => "an enemy bowman in the fog shoots our units: find him from where the strikes came and kill him",
        Fam::CityReinforced => "take a walled city; defenders may have entered it, seen once and no longer in sight: pick the siege or the assault",
        Fam::ScoutReport => "an enemy force was seen marching toward one of our three cities, then lost in the fog: reinforce that one",
        Fam::TwoFronts => "take an enemy city and hold our own against a marching force at once: divide the army by need",
        Fam::TwinSiege => "two enemy cities to take at once, a relief force between them: divide the army by the cities' strength and keep a capturer at each",
        _ => "besieging a city, an enemy force marches on our second city: detach the right part and go on",
    }
}

pub fn claims(f: Fam, k: &super::lab::EraKinds) -> bool {
    let land = k.has(Arm::Melee) && k.has(Arm::Ranged);
    match f {
        Fam::CityReinforced | Fam::TwoFronts | Fam::SecondCity | Fam::TwinSiege => land && k.has(Arm::Siege),
        _ => land,
    }
}

// ---------------------------------------------------------------------------
// Geometry: axial offsets (q, r) from the arena's centre.

const DIRS: [(i64, i64); 6] = [(1, 0), (1, -1), (0, -1), (-1, 0), (-1, 1), (0, 1)];

fn rel_xy(game: &GameState, centre: TileId, t: TileId) -> (i64, i64) {
    let w = i64::from(game.view().auth.map().dimensions.width);
    (i64::from(t.0) % w - i64::from(centre.0) % w, i64::from(t.0) / w - i64::from(centre.0) / w)
}

fn tile_rel(game: &GameState, centre: TileId, d: (i64, i64)) -> Option<TileId> {
    let dims = game.view().auth.map().dimensions;
    let (w, h) = (i64::from(dims.width), i64::from(dims.height));
    let (x, y) = (i64::from(centre.0) % w + d.0, i64::from(centre.0) / w + d.1);
    (x >= 0 && y >= 0 && x < w && y < h).then(|| TileId((y * w + x) as u32))
}

fn hdf(dq: f64, dr: f64) -> f64 {
    (dq.abs() + dr.abs() + (dq + dr).abs()) / 2.0
}

fn put_tiles(info: &mut BTreeMap<String, String>, key: &str, v: &[TileId]) {
    info.insert(key.into(), v.iter().map(|t| t.0.to_string()).collect::<Vec<_>>().join(","));
}

fn get_tiles(info: &BTreeMap<String, String>, key: &str) -> Vec<TileId> {
    info.get(key).map(|s| s.split(',').filter_map(|x| x.parse().ok()).map(TileId).collect()).unwrap_or_default()
}

fn put_pairs(info: &mut BTreeMap<String, String>, key: &str, v: &[(i64, i64)]) {
    info.insert(key.into(), v.iter().map(|(a, b)| format!("{a}:{b}")).collect::<Vec<_>>().join(";"));
}

fn get_pairs(info: &BTreeMap<String, String>, key: &str) -> Vec<(i64, i64)> {
    info.get(key)
        .map(|s| s.split(';').filter_map(|x| x.split_once(':')).filter_map(|(a, b)| Some((a.parse().ok()?, b.parse().ok()?))).collect())
        .unwrap_or_default()
}

fn rel_round(game: &GameState, sc: &Scene) -> u32 {
    game.view().auth.turn().round.saturating_sub(sc.start_round)
}

fn owner_of_city(game: &GameState, t: TileId) -> Option<classic_rules::ids::PlayerId> {
    let auth = game.view().auth;
    auth.indexes().city_on_tile(t).and_then(|c| auth.cities().get(c)).map(|c| c.owner)
}

/// A kind of `arm` of movement 2 (the march scenes are timed for it).
fn pace2(p: &mut Painter, arm: Arm) -> Result<classic_rules::ids::UnitKindId, String> {
    let k = p.kind(arm).ok_or("no kind")?;
    if act::moves_of(p.rules, k) != 2 {
        return Err(format!("{} has movement {}", p.rules.units[k.0 as usize].name, act::moves_of(p.rules, k)));
    }
    Ok(k)
}

/// The era's strongest melee kind of the common kinds.
fn strongest_melee(p: &Painter) -> Result<classic_rules::ids::UnitKindId, String> {
    p.lab.kinds.by_arm.get(&Arm::Melee).and_then(|v| v.iter().copied().max_by_key(|k| (p.rules.units[k.0 as usize].strength, k.0))).ok_or_else(|| "no melee".to_owned())
}

fn put_group(p: &mut Painter, side: classic_rules::ids::PlayerId, anchor: TileId, r: i32, arms: &[(Arm, &'static str)], flat: bool) -> Result<Vec<Piece>, String> {
    put_group_by(p, side, anchor, r, arms, flat, false)
}

/// The era's strongest kind of `arm` (the common kinds; the ranged by ranged strength).
fn strongest(p: &Painter, arm: Arm) -> Result<classic_rules::ids::UnitKindId, String> {
    let rules = p.rules;
    let power = |k: classic_rules::ids::UnitKindId| {
        let d = &rules.units[k.0 as usize];
        if arm == Arm::Ranged { d.ranged_strength } else { d.strength }
    };
    p.lab.kinds.by_arm.get(&arm).and_then(|v| v.iter().copied().max_by_key(|k| (power(*k), k.0))).ok_or_else(|| format!("no {}", arm.name()))
}

/// A group on the free tiles nearest `anchor` (within `r`); `flat`: foot units of movement 2 only; `strong`: the era's strongest kinds.
fn put_group_by(p: &mut Painter, side: classic_rules::ids::PlayerId, anchor: TileId, r: i32, arms: &[(Arm, &'static str)], flat: bool, strong: bool) -> Result<Vec<Piece>, String> {
    let spots = p.spots(anchor, r, arms.len())?;
    let mut out = Vec::new();
    for ((arm, role), t) in arms.iter().zip(spots) {
        let k = if strong {
            let k = strongest(p, *arm)?;
            if flat && act::moves_of(p.rules, k) != 2 {
                return Err(format!("{} has movement {}", p.rules.units[k.0 as usize].name, act::moves_of(p.rules, k)));
            }
            k
        } else if flat {
            pace2(p, *arm)?
        } else {
            p.kind(*arm).ok_or("no kind")?
        };
        out.push(p.put(side, t, k, role));
    }
    Ok(out)
}

pub(super) fn build(p: &mut Painter, fam: Fam, lab: &Lab, instance: usize, attempt: u32) -> Result<Scene, String> {
    let mut s = match fam {
        Fam::Feint => build_feint(p, lab, instance, attempt)?,
        Fam::ScoutReport => build_scout(p, lab, instance, attempt)?,
        Fam::FogSniper => build_sniper(p, lab, instance, attempt)?,
        Fam::CityReinforced => build_reinforced(p, lab, instance, attempt)?,
        Fam::TwoFronts | Fam::SecondCity => build_fronts(p, fam, lab, instance, attempt)?,
        Fam::TwinSiege => build_twin(p, lab, instance, attempt)?,
        _ => return Err(format!("{} is not a history family", fam.name())),
    };
    s.info.insert("mine".into(), super::families::kinds_label(p.rules, &s.mine));
    s.info.insert("theirs".into(), super::families::kinds_label(p.rules, &s.theirs));
    s.rounds = rounds(fam);
    Ok(s)
}

// ---------------------------------------------------------------------------
// feint and scout-report: the marching force


/// A city of ours worn down to `left` percent of its hit points (the march scenes: an empty city falls to the first blows).
fn weaken(p: &mut Painter, city: classic_rules::ids::CityHandle, left: i64) {
    let max = {
        let auth = p.game.view().auth;
        auth.cities().get(city).map(|c| i64::from(classic_rules::combat::city::max_hit_points(auth, p.rules, c)))
    };
    if let Some(max) = max {
        p.game.debug_set_city_damage(city, (max * (100 - left) / 100) as u16);
    }
}

fn foe_arms(m: usize, r: usize) -> Vec<(Arm, &'static str)> {
    let mut v = vec![(Arm::Melee, "raider"); m];
    v.extend(vec![(Arm::Ranged, "raider"); r]);
    v
}

fn march_offsets(p: &Painter, start: TileId, ps: &[Piece]) -> Vec<(i64, i64)> {
    let o = p.xy(start);
    ps.iter().map(|q| {
        let c = act::core_of(&p.game, q.h).map_or(start, |c| c.tile);
        let (x, y) = p.xy(c);
        (x - o.0, y - o.1)
    }).collect()
}

fn build_feint(p: &mut Painter, lab: &Lab, instance: usize, attempt: u32) -> Result<Scene, String> {
    let mut s = blank(Fam::Feint, lab, instance, attempt);
    let (me, foe) = (lab.learner, lab.enemy);
    let at = |p: &Painter, d: (i64, i64)| p.at(d.0, d.1).ok_or_else(|| "off the map".to_owned());
    // City 0 = A (west), city 1 = B (east); the cities 4 from the decision point E and from the army's start.
    let cities = [(-4, 2), (2, 2)];
    let real = usize::from(p.d.chance(50));
    let turned = p.d.chance(50);
    let e = (0, 0);
    let heads = [(-2, 1), (1, 1)];
    let hr = heads[real];
    let h1 = if turned { heads[1 - real] } else { hr };
    let p2 = (e.0 - hr.0, e.1 - hr.1);
    let p1 = (p2.0 - h1.0, p2.1 - h1.1);
    let p0 = (p1.0 - h1.0, p1.1 - h1.1);
    let pop = p.d.range(1, 1) as u32;
    let mut tiles = Vec::new();
    for c in cities {
        let t = at(p, c)?;
        let h = p.city(me, t, pop, false);
        weaken(p, h, 10);
        tiles.push(t);
    }
    let mut arms_me = vec![(Arm::Melee, "army"); 2];
    arms_me.extend(vec![(Arm::Ranged, "ranged"); 2]);
    let start = at(p, (-2, 4))?;
    s.mine = put_group_by(p, me, start, 2, &arms_me, true, true)?;
    let arms_foe = foe_arms(5, 2);
    let t0 = at(p, p0)?;
    s.theirs = put_group_by(p, foe, t0, 2, &arms_foe, true, true)?;
    let offs = march_offsets(p, t0, &s.theirs);
    put_tiles(&mut s.info, "cities", &tiles);
    put_pairs(&mut s.info, "wp", &[p1, p2, e]);
    put_pairs(&mut s.info, "off", &offs);
    s.info.insert("real".into(), real.to_string());
    s.info.insert("turned".into(), u8::from(turned).to_string());
    s.hold = Some(at(p, (-1, 2))?);
    Ok(s)
}

/// The three cities of the scout scene stand at the corners (4, -4), (0, -4), (-4, 0) of the ring of radius 4 round the army;
/// the enemy walks the ray to one of them from 3.5 times as far.
const SCOUT_CITIES: [(i64, i64); 3] = [(4, -4), (0, -4), (-4, 0)];

fn build_scout(p: &mut Painter, lab: &Lab, instance: usize, attempt: u32) -> Result<Scene, String> {
    let mut s = blank(Fam::ScoutReport, lab, instance, attempt);
    let (me, foe) = (lab.learner, lab.enemy);
    let rules = p.rules;
    let at = |p: &Painter, d: (i64, i64)| p.at(d.0, d.1).ok_or_else(|| "off the map".to_owned());
    let real = p.d.range(0, 2) as usize;
    let x = SCOUT_CITIES[real];
    let waypoints = |k2: i64| (x.0 * k2 / 2, x.1 * k2 / 2);
    // E = 2X (4 beyond the city), P2 = 2.5X, P1 = 3X, P0 = 3.5X.
    let (e, p2, p1, p0) = (waypoints(4), waypoints(5), waypoints(6), waypoints(7));
    let pop = p.d.range(2, 3) as u32;
    let mut tiles = Vec::new();
    for c in SCOUT_CITIES {
        let t = at(p, c)?;
        let h = p.city(me, t, pop, false);
        weaken(p, h, 30);
        tiles.push(t);
    }
    // One outpost beside each possible road, where the road's first three stops are in its sight and the stop at the decision round is not.
    for c in SCOUT_CITIES {
        let w = |k2: i64| (c.0 * k2 / 2, c.1 * k2 / 2);
        let (pe, q2, q1, q0) = (w(4), w(5), w(6), w(7));
        let tq = |d: (i64, i64)| at(p, d);
        let (t0, t1, t2, te) = (tq(q0)?, tq(q1)?, tq(q2)?, tq(pe)?);
        let cand: Vec<TileId> = tiles_within(&p.game, t1, 3)
            .into_iter()
            .filter(|t| p.free(*t) && dist(&p.game, *t, t0) <= 3 && dist(&p.game, *t, t1) <= 3 && dist(&p.game, *t, t2) <= 3 && dist(&p.game, *t, te) >= 4)
            .filter(|t| [t0, t1, t2].iter().all(|q| dist(&p.game, *t, *q) >= 2))
            .collect();
        let y = cand.into_iter().min_by_key(|t| (dist(&p.game, *t, t1), t.0)).ok_or("no outpost site")?;
        p.city(me, y, 1, false);
    }
    let start = at(p, (0, 0))?;
    let arms_me = [(Arm::Melee, "army"), (Arm::Melee, "army"), (Arm::Ranged, "ranged"), (Arm::Ranged, "ranged")];
    s.mine = put_group(p, me, start, 1, &arms_me, true)?;
    let arms_foe = foe_arms(5, 2);
    let t0 = at(p, p0)?;
    s.theirs = put_group_by(p, foe, t0, 2, &arms_foe, true, true)?;
    // The checks the scene stands on: the road's first three stops are seen by the outposts, the fourth is not, and nobody else sees it.
    let view = p.game.player_view(rules, me);
    for (i, d) in [p0, p1, p2, e].into_iter().enumerate() {
        let seen = view.is_visible(at(p, d)?);
        if seen != (i < 3) {
            return Err(format!("stop {i} of the road: visible {seen}"));
        }
    }
    let offs = march_offsets(p, t0, &s.theirs);
    put_tiles(&mut s.info, "cities", &tiles);
    put_pairs(&mut s.info, "wp", &[p1, p2, e]);
    put_pairs(&mut s.info, "off", &offs);
    s.info.insert("real".into(), real.to_string());
    s.hold = Some(start);
    Ok(s)
}

/// The enemy's turn of a march scene: the waypoints of the first three rounds, then the strike on the real city.
fn march_enemy(rules: &CompiledRuleset, game: &mut GameState, sc: &mut Scene) {
    let (me, foe) = (sc.enemy, sc.learner);
    let rel = rel_round(game, sc) as usize;
    let wps = get_pairs(&sc.info, "wp");
    let offs = get_pairs(&sc.info, "off");
    let cities = get_tiles(&sc.info, "cities");
    let real: usize = sc.info.get("real").and_then(|v| v.parse().ok()).unwrap_or(0);
    let theirs = sc.theirs.clone();
    if rel < wps.len() {
        for (i, p) in theirs.iter().enumerate() {
            if act::alive(game, p, me).is_none() {
                continue;
            }
            let o = offs.get(i).copied().unwrap_or((0, 0));
            if let Some(g) = tile_rel(game, sc.centre, (wps[rel].0 + o.0, wps[rel].1 + o.1)) {
                act::move_by(game, rules, me, p.h, |gm, t, _| (dist(gm, t, g), t.0));
            }
            act::stand(game, rules, p.h);
        }
        return;
    }
    let goal = cities[real.min(cities.len() - 1)];
    let sc2 = sc.clone();
    let friends: Vec<classic_rules::ids::UnitHandle> = theirs.iter().map(|p| p.h).collect();
    let ours = move |g: &GameState| -> Vec<TileId> {
        let mut v: Vec<TileId> = sc2.mine_alive(g).iter().map(|(_, c)| c.tile).collect();
        if owner_of_city(g, goal) == Some(sc2.learner) {
            v.push(goal);
        }
        v.sort_by_key(|t| t.0);
        v.dedup();
        v
    };
    for p in &theirs {
        if act::alive(game, p, me).is_some() {
            act::unit_turn(game, rules, me, foe, p.h, &Aim::To(goal), Nerve::Reckless, &ours, &friends);
        }
    }
}

/// The foes our side may use: the seen ones in a fog family, else all.
fn foes_seen(rules: &CompiledRuleset, game: &GameState, sc: &Scene) -> Vec<UnitCore> {
    if is_fog(sc.fam) {
        memory::seen_foes(rules, game, sc.learner, sc.enemy).into_iter().filter(|c| !act::is_civilian(rules, c.kind)).collect()
    } else {
        sc.theirs_alive(game).into_iter().map(|(_, c)| c).collect()
    }
}

/// Where a marching foe is going: of `cities`, the one the points `b + k * (b - a) / n` (k = 1..8) come nearest to.
fn project(game: &GameState, centre: TileId, a: TileId, b: TileId, n: i64, cities: &[TileId]) -> usize {
    let (aq, ar) = rel_xy(game, centre, a);
    let (bq, br) = rel_xy(game, centre, b);
    let (dq, dr) = ((bq - aq) as f64 / n.max(1) as f64, (br - ar) as f64 / n.max(1) as f64);
    let mut best = (f64::MAX, 0usize);
    for (i, c) in cities.iter().enumerate() {
        let (cq, cr) = rel_xy(game, centre, *c);
        let d = (1..=8).map(|k| hdf(bq as f64 + dq * k as f64 - cq as f64, br as f64 + dr * k as f64 - cr as f64)).fold(f64::MAX, f64::min);
        if d < best.0 - 1e-9 {
            best = (d, i);
        }
    }
    best.1
}

/// The defence of one city: the units go to it (the melee first), strike at even odds and fortify.
fn defend(rules: &CompiledRuleset, game: &mut GameState, sc: &Scene, units: &[Piece], city: TileId) {
    let (me, foe) = (sc.learner, sc.enemy);
    let friends: Vec<classic_rules::ids::UnitHandle> = units.iter().map(|p| p.h).collect();
    let sc2 = sc.clone();
    let foes = move |g: &GameState| -> Vec<TileId> { act::foe_tiles(g, sc2.enemy, Some(&sc2.theirs), false) };
    let mut order: Vec<Piece> = units.to_vec();
    order.sort_by_key(|p| (u8::from(act::range_of(rules, p.kind).is_some()), p.id.0));
    for p in order {
        if act::alive(game, &p, me).is_some() {
            act::unit_turn(game, rules, me, foe, p.h, &Aim::Hold(city), Nerve::Even, &foes, &friends);
        }
    }
}

fn hold_all(rules: &CompiledRuleset, game: &mut GameState, sc: &Scene) {
    for p in &sc.mine {
        if act::alive(game, p, sc.learner).is_some() {
            let _ = act::fortify(game, rules, sc.learner, p.h);
            act::stand(game, rules, p.h);
        }
    }
}

fn march_teacher(rules: &CompiledRuleset, game: &mut GameState, sc: &mut Scene, name: &str) {
    let rel = rel_round(game, sc);
    let cities = get_tiles(&sc.info, "cities");
    let seen = foes_seen(rules, game, sc);
    if let Some(l) = seen.iter().min_by_key(|c| c.id.0) {
        sc.info.insert(format!("p{rel}"), l.tile.0.to_string());
    }
    if matches!(name, "ref-split" | "ref-spread") {
        let n = cities.len();
        for (i, c) in cities.iter().enumerate() {
            let group: Vec<Piece> = sc.mine.iter().enumerate().filter(|(j, _)| j % n == i).map(|(_, p)| p.clone()).collect();
            defend(rules, game, sc, &group, *c);
        }
        return;
    }
    let sighted = |sc: &Scene, r: u32| -> Option<TileId> { sc.info.get(&format!("p{r}")).and_then(|v| v.parse().ok()).map(TileId) };
    if !sc.info.contains_key("decided") {
        let decide_at = if name == "ref-wait" { 4 } else { 3 };
        let pick: Option<usize> = match name {
            "ref-trace" | "ref-wait" if rel >= decide_at => match (sighted(sc, rel - 1), sighted(sc, rel)) {
                (Some(a), Some(b)) => Some(project(game, sc.centre, a, b, 1, &cities)),
                _ => None,
            },
            "ref-first" if rel >= decide_at => match (sighted(sc, 0), sighted(sc, 1)) {
                (Some(a), Some(b)) => Some(project(game, sc.centre, a, b, 1, &cities)),
                _ => None,
            },
            "ref-extend" if rel >= decide_at => {
                let pts: Vec<(u32, TileId)> = (0..rel).filter_map(|r| sighted(sc, r).map(|t| (r, t))).collect();
                match (pts.first(), pts.last()) {
                    (Some(a), Some(b)) if b.0 > a.0 => Some(project(game, sc.centre, a.1, b.1, i64::from(b.0 - a.0), &cities)),
                    _ => None,
                }
            }
            "ref-guess" if rel >= decide_at => seen.iter().min_by_key(|c| c.id.0).map(|l| cities.iter().enumerate().min_by_key(|(_, c)| (dist(game, **c, l.tile), c.0)).map_or(0, |(i, _)| i)),
            "ref-stay" => {
                // Reacts to what shows: a foe within 3 of one of the cities.
                seen.iter().find_map(|f| cities.iter().position(|c| dist(game, *c, f.tile) <= 3))
            }
            _ => None,
        };
        if let Some(i) = pick {
            sc.info.insert("decided".into(), i.to_string());
            sc.info.insert("decided_rel".into(), rel.to_string());
        } else if rel >= 5 && name != "ref-stay" {
            // The evidence never came (an outpost lost): the first city.
            sc.info.insert("decided".into(), "0".into());
        }
    }
    match sc.info.get("decided").and_then(|v| v.parse::<usize>().ok()) {
        Some(i) => {
            let all = sc.mine.clone();
            defend(rules, game, sc, &all, cities[i.min(cities.len() - 1)]);
        }
        None => hold_all(rules, game, sc),
    }
}

fn march_judge(game: &GameState, sc: &Scene, rel: u32) -> Option<bool> {
    let cities = get_tiles(&sc.info, "cities");
    if cities.iter().any(|c| owner_of_city(game, *c) != Some(sc.learner)) {
        return Some(false);
    }
    if sc.theirs_alive(game).is_empty() || rel >= sc.rounds {
        return Some(true);
    }
    None
}

// ---------------------------------------------------------------------------
// fog-sniper

fn build_sniper(p: &mut Painter, lab: &Lab, instance: usize, attempt: u32) -> Result<Scene, String> {
    let mut s = blank(Fam::FogSniper, lab, instance, attempt);
    let (me, foe) = (lab.learner, lab.enemy);
    let rules = p.rules;
    let at = |p: &Painter, d: (i64, i64)| p.at(d.0, d.1).ok_or_else(|| "off the map".to_owned());
    let k = p.d.range(0, 5) as usize;
    let dirs = [DIRS[k], DIRS[(k + 1) % 6], DIRS[(k + 2) % 6]];
    let z = at(p, (0, 0))?;
    let mut ridge = vec![z];
    for d in dirs {
        ridge.push(at(p, d)?);
    }
    let hills = lab.grounds.hills;
    p.paint(&ridge, hills);
    let sniper_kind = lab.kinds.by_arm.get(&Arm::Ranged).and_then(|v| v.iter().copied().filter(|k| act::range_of(rules, *k).is_some_and(|r| r >= 2)).min_by_key(|k| (rules.units[k.0 as usize].ranged_strength, k.0))).ok_or("no ranged kind of range 2")?;
    let sniper = p.put(foe, z, sniper_kind, "sniper");
    s.theirs.push(sniper);
    let arms = [(Arm::Melee, "army"), (Arm::Ranged, "ranged"), (Arm::Melee, "army")];
    for (d, (arm, role)) in dirs.iter().zip(arms) {
        let kind = p.kind(arm).ok_or("no kind")?;
        let piece = p.put(me, at(p, (2 * d.0, 2 * d.1))?, kind, role);
        s.mine.push(piece);
    }
    // The ridge hides the sniper: none of ours sees his tile.
    let view = p.game.player_view(rules, me);
    if view.is_visible(z) {
        return Err("the sniper is in sight".into());
    }
    s.info.insert("sniper".into(), z.0.to_string());
    Ok(s)
}

fn sniper_enemy(rules: &CompiledRuleset, game: &mut GameState, sc: &mut Scene) {
    let (me, foe) = (sc.enemy, sc.learner);
    let rel = rel_round(game, sc);
    let Some(p) = sc.theirs.first().cloned() else { return };
    let Some(c) = act::alive(game, &p, me) else { return };
    let ours: Vec<TileId> = sc.mine_alive(game).iter().map(|(_, u)| u.tile).collect();
    let _ = foe;
    // The weakest unit in reach: a kill first, then the fewest hit points left.
    if let Some((_, t)) = act::best_blow(game, rules, me, p.h, &ours, Nerve::Reckless) {
        if act::strike(game, rules, me, p.h, t) {
            sc.info.insert("src".into(), c.tile.0.to_string());
            sc.info.insert("src_round".into(), rel.to_string());
            let n: u32 = sc.info.get("hits").and_then(|v| v.parse().ok()).unwrap_or(0);
            sc.info.insert("hits".into(), (n + 1).to_string());
        }
    }
    act::stand(game, rules, p.h);
}

fn can_hit(game: &GameState, range: i32, from: TileId, to: TileId) -> bool {
    dist(game, from, to) <= range && kernel::derived::visibility::los_clear(game.view().auth.map(), from, to)
}

fn sniper_teacher(rules: &CompiledRuleset, game: &mut GameState, sc: &mut Scene, name: &str) {
    let me = sc.learner;
    let rel = rel_round(game, sc);
    let src: Option<TileId> = sc.info.get("src").and_then(|v| v.parse().ok()).map(TileId);
    let friends: Vec<classic_rules::ids::UnitHandle> = sc.mine.iter().map(|p| p.h).collect();
    let mut order: Vec<Piece> = sc.mine.clone();
    order.sort_by_key(|p| (u8::from(act::range_of(rules, p.kind).is_some()), p.id.0));
    if name == "ref-source" {
        let Some(z) = src else {
            hold_all(rules, game, sc);
            return;
        };
        let target = [z];
        for p in order {
            let Some(c) = act::alive(game, &p, me) else { continue };
            if let Some(range) = act::range_of(rules, c.kind) {
                // A shooter goes to a tile with a clear line to the tile the strikes came from, in range, else as near as it can.
                if !can_hit(game, range, c.tile, z) {
                    act::move_by(game, rules, me, p.h, |g, x, _| (!can_hit(g, range, x, z), dist(g, x, z), x.0));
                }
                if act::strike(game, rules, me, p.h, z) {
                    act::stand(game, rules, p.h);
                    continue;
                }
                act::stand(game, rules, p.h);
            } else {
                act::unit_turn(game, rules, me, sc.enemy, p.h, &Aim::Close, Nerve::Reckless, &|_g: &GameState| target.to_vec(), &friends);
            }
        }
        return;
    }
    // ref-search: blind. Each unit walks its own bearing outward from the start, a quarter turn every two rounds; whatever it sees it attacks.
    let seen: Vec<TileId> = foes_seen(rules, game, sc).iter().map(|c| c.tile).collect();
    for (i, p) in order.iter().enumerate() {
        let Some(c) = act::alive(game, p, me) else { continue };
        if !seen.is_empty() {
            act::unit_turn(game, rules, me, sc.enemy, p.h, &Aim::Close, Nerve::Reckless, &|_g: &GameState| seen.clone(), &friends);
            continue;
        }
        // Blind: away from the group's middle (every direction is as good as any other), a sixth of a turn each round.
        let mine_now: Vec<(i64, i64)> = sc.mine_alive(game).iter().map(|(_, u)| rel_xy(game, sc.centre, u.tile)).collect();
        let n = mine_now.len().max(1) as f64;
        let (mq, mr) = (mine_now.iter().map(|t| t.0 as f64).sum::<f64>() / n, mine_now.iter().map(|t| t.1 as f64).sum::<f64>() / n);
        let (uq, ur) = rel_xy(game, sc.centre, c.tile);
        let away = (uq as f64 - mq, ur as f64 - mr);
        let bearing = if away.0.abs() + away.1.abs() < 0.3 {
            DIRS[(i + rel as usize) % 6]
        } else {
            *DIRS.iter().min_by(|a, b| hdf(a.0 as f64 - away.0 / 2.0, a.1 as f64 - away.1 / 2.0).total_cmp(&hdf(b.0 as f64 - away.0 / 2.0, b.1 as f64 - away.1 / 2.0))).expect("dirs")
        };
        let goal = tile_rel(game, c.tile, (bearing.0 * 4, bearing.1 * 4)).unwrap_or(c.tile);
        act::move_by(game, rules, me, p.h, |g, x, _| (dist(g, x, goal), x.0));
        act::stand(game, rules, p.h);
    }
}

fn sniper_judge(game: &GameState, sc: &Scene, rel: u32) -> Option<bool> {
    let dead = sc.theirs.iter().all(|p| act::alive(game, p, sc.enemy).is_none());
    if dead {
        Some(sc.mine_alive(game).len() >= 1)
    } else if rel >= sc.rounds || sc.mine_alive(game).is_empty() {
        Some(false)
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// city-reinforced

const CITY_AT: (i64, i64) = (4, 0);
const SCOUT_AT: (i64, i64) = (1, 0);

fn build_reinforced(p: &mut Painter, lab: &Lab, instance: usize, attempt: u32) -> Result<Scene, String> {
    let mut s = blank(Fam::CityReinforced, lab, instance, attempt);
    let (me, foe) = (lab.learner, lab.enemy);
    let rules = p.rules;
    let at = |p: &Painter, d: (i64, i64)| p.at(d.0, d.1).ok_or_else(|| "off the map".to_owned());
    let city_t = at(p, CITY_AT)?;
    let pop = p.d.range(3, 4) as u32;
    let city = p.city(foe, city_t, pop, false);
    // Worn down (55 % left), no walls: the two modes differ by the defenders, not by the city.
    weaken(p, city, 55);
    let scout_t = at(p, SCOUT_AT)?;
    let hills = lab.grounds.hills;
    p.paint(&[scout_t], hills);
    p.clutter(p.d.range(1, 3) as usize, &[city_t, scout_t]);
    let gk = p.kind(if p.d.chance(50) { Arm::Ranged } else { Arm::Melee }).ok_or("no garrison")?;
    s.theirs.push(p.put(foe, city_t, gk, "garrison"));
    let reinforced = p.d.chance(50);
    s.info.insert("reinforced".into(), u8::from(reinforced).to_string());
    // The scout: the army's first unit in the list, a melee unit on the hill 3 tiles from the city (outside the city's strike, its sight 3).
    let sk = p.kind(Arm::Melee).ok_or("no melee")?;
    s.mine.push(p.put(me, scout_t, sk, "scout"));
    let mut plan: Vec<(Arm, &'static str)> = vec![(Arm::Melee, "capturer"), (Arm::Melee, "capturer")];
    for _ in 0..p.d.range(3, 4) {
        plan.push((Arm::Ranged, "ranged"));
    }
    for _ in 0..p.d.range(1, 2) {
        plan.push((Arm::Siege, "siege"));
    }
    let anchor = at(p, (-2, 0))?;
    let spots = p.spots(anchor, 2, plan.len())?;
    for ((arm, role), t) in plan.iter().zip(spots) {
        let k = p.kind(*arm).ok_or("no kind")?;
        s.mine.push(p.put(me, t, k, role));
    }
    let strong = strongest_melee(p)?;
    let n_in = p.d.range(2, 3) as usize;
    if reinforced {
        s.info.insert("enter".into(), format!("{} x2 at round 0", rules.units[strong.0 as usize].name));
    } else {
        let n = 5 + (super::lab::mix(0x0c17_e1f0 ^ instance as u64) % 3) as u32;
        s.reinforce = vec![strong; n_in];
        s.info.insert("wave".into(), n.to_string());
        s.info.insert("wave_kind".into(), format!("{} x{n_in}", rules.units[strong.0 as usize].name));
    }
    s.target = Some(city_t);
    s.aim_learner = Some(city_t);
    s.centre = city_t;
    Ok(s)
}

/// Our army without the scout (the assault planners know the roles capturer, ranged and siege).
fn army_only(sc: &Scene) -> Scene {
    let mut s = sc.clone();
    s.mine.retain(|p| p.role != "scout");
    s
}

fn land_wave(rules: &CompiledRuleset, game: &mut GameState, sc: &mut Scene, kinds: Vec<classic_rules::ids::UnitKindId>, role: &'static str, near: Option<TileId>, adjacent_to: TileId) {
    let ours: Vec<TileId> = sc.mine_alive(game).iter().map(|(_, c)| c.tile).collect();
    let auth = game.view().auth;
    let empty = |g: &GameState, t: TileId| {
        let a = g.view().auth;
        crate::battle::standable(rules, g, t) && a.indexes().units_on(t).is_empty() && a.indexes().city_on_tile(t).is_none()
    };
    let _ = auth;
    let mut tiles: Vec<TileId> = tiles_within(game, adjacent_to, 1).into_iter().filter(|t| empty(game, *t)).collect();
    match near {
        Some(n) => tiles.sort_by_key(|t| (dist(game, *t, n), t.0)),
        None => tiles.sort_by_key(|t| (-act::nearest(game, *t, &ours), t.0)),
    }
    for (k, t) in kinds.into_iter().zip(tiles) {
        let h = game.debug_spawn_unit(rules, sc.enemy, t, k);
        let id = game.view().auth.units().get(h).expect("a spawned unit").core.id;
        let piece = Piece { h, id, kind: k, role };
        let _ = act::fortify(game, rules, sc.enemy, h);
        sc.theirs.push(piece);
    }
}

fn reinforced_enemy(rules: &CompiledRuleset, game: &mut GameState, sc: &mut Scene) {
    let rel = rel_round(game, sc);
    let city = sc.target.unwrap_or(sc.centre);
    let reinforced = sc.info.get("reinforced").is_some_and(|v| v == "1");
    if reinforced && rel == 0 && !sc.info.contains_key("entered") {
        sc.info.insert("entered".into(), "1".into());
        let strong = strong_kind(rules, sc);
        let near = tile_rel(game, sc.centre, (SCOUT_AT.0 - CITY_AT.0, SCOUT_AT.1 - CITY_AT.1));
        land_wave(rules, game, sc, vec![strong; 3], "defender", near, city);
    }
    if !reinforced {
        let n: u32 = sc.info.get("wave").and_then(|v| v.parse().ok()).unwrap_or(99);
        if rel + 1 >= n && !sc.reinforce.is_empty() && owner_of_city(game, city) == Some(sc.enemy) {
            let kinds = std::mem::take(&mut sc.reinforce);
            land_wave(rules, game, sc, kinds, "reinforce", None, city);
        }
    }
    // The enemy's field units keep to the city (no unit of the garrison leaves): they stand.
    for p in sc.theirs.clone() {
        if act::alive(game, &p, sc.enemy).is_some() {
            act::stand(game, rules, p.h);
        }
    }
}

fn strong_kind(rules: &CompiledRuleset, sc: &Scene) -> classic_rules::ids::UnitKindId {
    // The strongest melee kind of the era the scene's own kinds come from: the enemy's garrison kind's era.
    let era = sc.era;
    let k = super::lab::EraKinds::of(rules, era);
    k.by_arm.get(&Arm::Melee).and_then(|v| v.iter().copied().max_by_key(|x| (rules.units[x.0 as usize].strength, x.0))).unwrap_or(sc.theirs[0].kind)
}

fn scout_turn(rules: &CompiledRuleset, game: &mut GameState, sc: &mut Scene) {
    let me = sc.learner;
    let rel = rel_round(game, sc);
    let Some(scout) = sc.mine.iter().find(|p| p.role == "scout").cloned() else { return };
    let Some(c) = act::alive(game, &scout, me) else { return };
    if rel < 3 {
        let _ = act::fortify(game, rules, me, scout.h);
        act::stand(game, rules, scout.h);
    } else {
        // Back to the army, out of the city's sight.
        let home = tile_rel(game, sc.centre, (-6, 0)).unwrap_or(c.tile);
        act::move_by(game, rules, me, scout.h, |g, x, _| (dist(g, x, home), x.0));
        act::stand(game, rules, scout.h);
    }
}

/// The garrison the scout saw: the most enemy military units seen within 1 of the city in any round so far.
fn note_garrison(rules: &CompiledRuleset, game: &GameState, sc: &mut Scene) {
    let city = sc.target.unwrap_or(sc.centre);
    let n = foes_seen(rules, game, sc).iter().filter(|c| dist(game, c.tile, city) <= 1).count();
    let best: usize = sc.info.get("garrison_seen").and_then(|v| v.parse().ok()).unwrap_or(0);
    if n > best {
        sc.info.insert("garrison_seen".into(), n.to_string());
    }
}

fn reinforced_teacher(rules: &CompiledRuleset, game: &mut GameState, sc: &mut Scene, name: &str) {
    scout_turn(rules, game, sc);
    note_garrison(rules, game, sc);
    let rel = rel_round(game, sc);
    let careful = match name {
        "ref-burst" => false,
        "ref-planned" => true,
        _ => {
            // The informed plan: decided at round 2 from the garrison the scout saw (3 or more: the city was reinforced).
            if !sc.info.contains_key("plan") && rel >= 2 {
                let seen: usize = sc.info.get("garrison_seen").and_then(|v| v.parse().ok()).unwrap_or(0);
                sc.info.insert("plan".into(), if seen >= 3 { "careful" } else { "burst" }.into());
            }
            sc.info.get("plan").is_some_and(|p| p == "careful")
        }
    };
    if name == "ref-plan" && rel < 2 {
        // Until it has seen, the army marches up as one (the planner's staging).
    }
    let f = assault::flags(if careful { "ref-planned" } else { "ref-burst" }).expect("a flag set");
    let mut sub = army_only(sc);
    assault::turn(rules, game, &mut sub, f);
    sc.track = sub.track;
}

fn reinforced_judge(game: &GameState, sc: &Scene, rel: u32) -> Option<bool> {
    let city = sc.target.unwrap_or(sc.centre);
    let reinforced = sc.info.get("reinforced").is_some_and(|v| v == "1");
    let army: Vec<&Piece> = sc.mine.iter().filter(|p| p.role != "scout").collect();
    let lost = army.iter().filter(|p| act::alive(game, p, sc.learner).is_none()).count();
    let taken = owner_of_city(game, city) == Some(sc.learner);
    if reinforced {
        if lost > 2 {
            Some(false)
        } else if taken {
            Some(true)
        } else if rel >= sc.rounds {
            Some(false)
        } else {
            None
        }
    } else {
        let n: u32 = sc.info.get("wave").and_then(|v| v.parse().ok()).unwrap_or(99);
        if taken && rel < n {
            Some(true)
        } else if rel >= n || army.iter().all(|p| act::alive(game, p, sc.learner).is_none()) {
            Some(false)
        } else {
            None
        }
    }
}

// ---------------------------------------------------------------------------
// two-fronts and second-city

const FRONT_CITY: (i64, i64) = (5, 0);
const HOME_CITY: (i64, i64) = (-4, 0);
/// Where the force of the two-fronts scenes stands (and appears, in second-city): 7 tiles west of the centre.
fn force_at(_fam: Fam) -> (i64, i64) {
    (-7, 0)
}

fn build_fronts(p: &mut Painter, fam: Fam, lab: &Lab, instance: usize, attempt: u32) -> Result<Scene, String> {
    let mut s = blank(fam, lab, instance, attempt);
    let (me, foe) = (lab.learner, lab.enemy);
    let rules = p.rules;
    let at = |p: &Painter, d: (i64, i64)| p.at(d.0, d.1).ok_or_else(|| "off the map".to_owned());
    let city_t = at(p, FRONT_CITY)?;
    let pop = p.d.range(5, 8) as u32;
    let city = p.city(foe, city_t, pop, false);
    // The city to take is worn down to 20 % and has no walls: the deadline, not the walls, sets the pace.
    weaken(p, city, 20);
    let home_t = at(p, HOME_CITY)?;
    // Two-fronts: a capital worn down to 3 %; second-city: a small town at full strength.
    let two = fam == Fam::TwoFronts;
    let hc = p.city(me, home_t, if two { 4 } else { 2 }, two);
    weaken(p, hc, if two { 3 } else { 100 });
    p.clutter(p.d.range(1, 3) as usize, &[city_t, home_t]);
    let gk = p.kind(if p.d.chance(50) { Arm::Ranged } else { Arm::Melee }).ok_or("no garrison")?;
    s.theirs.push(p.put(foe, city_t, gk, "garrison"));
    let mut plan: Vec<(Arm, &'static str)> = Vec::new();
    let (n_cap, n_rng, n_sg) = if fam == Fam::TwoFronts { (4, 2, 2) } else { (2, 3, 2) };
    for _ in 0..n_cap {
        plan.push((Arm::Melee, "capturer"));
    }
    for _ in 0..n_rng {
        plan.push((Arm::Ranged, "ranged"));
    }
    for _ in 0..n_sg {
        plan.push((Arm::Siege, "siege"));
    }
    let anchor = at(p, (0, 0))?;
    let spots = p.spots(anchor, 2, plan.len())?;
    for ((arm, role), t) in plan.iter().zip(spots) {
        let k = p.kind(*arm).ok_or("no kind")?;
        s.mine.push(p.put(me, t, k, role));
    }
    let strong = strongest_melee(p)?;
    let n_force = if fam == Fam::TwoFronts { p.d.range(3, 3) as usize } else { 4 };
    s.info.insert("force".into(), n_force.to_string());
    s.info.insert("force_kind".into(), rules.units[strong.0 as usize].name.clone());
    s.target = Some(city_t);
    s.refuge = Some(home_t);
    s.aim_learner = Some(city_t);
    s.centre = at(p, (0, 0))?;
    if fam == Fam::TwoFronts {
        let spawn = at(p, force_at(fam))?;
        for t in p.spots(spawn, 2, n_force)? {
            s.theirs.push(p.put(foe, t, strong, "force"));
        }
        s.info.insert("k".into(), "0".into());
    } else {
        let k = p.d.range(3, 5);
        s.info.insert("k".into(), k.to_string());
        s.info.insert("strong".into(), strong.0.to_string());
    }
    Ok(s)
}

fn fronts_enemy(rules: &CompiledRuleset, game: &mut GameState, sc: &mut Scene) {
    let (me, foe) = (sc.enemy, sc.learner);
    let rel = rel_round(game, sc);
    let home = sc.refuge.unwrap_or(sc.centre);
    // Second-city: the force appears in the enemy's turn of round K - 1, 9 tiles west of our city.
    if sc.fam == Fam::SecondCity && !sc.info.contains_key("spawned") {
        let k: u32 = sc.info.get("k").and_then(|v| v.parse().ok()).unwrap_or(3);
        if rel + 1 >= k {
            sc.info.insert("spawned".into(), rel.to_string());
            let strong = classic_rules::ids::UnitKindId(sc.info.get("strong").and_then(|v| v.parse().ok()).unwrap_or(0));
            let n: usize = sc.info.get("force").and_then(|v| v.parse().ok()).unwrap_or(3);
            if let Some(spawn) = tile_rel(game, sc.centre, force_at(sc.fam)) {
                let mut tiles: Vec<TileId> = tiles_within(game, spawn, 2).into_iter().filter(|t| crate::battle::standable(rules, game, *t) && game.view().auth.indexes().units_on(*t).is_empty() && game.view().auth.indexes().city_on_tile(*t).is_none()).collect();
                tiles.sort_by_key(|t| (dist(game, *t, spawn), t.0));
                for t in tiles.into_iter().take(n) {
                    let h = game.debug_spawn_unit(rules, me, t, strong);
                    let id = game.view().auth.units().get(h).expect("a spawned unit").core.id;
                    sc.theirs.push(Piece { h, id, kind: strong, role: "force" });
                }
            }
            return;
        }
    }
    let sc2 = sc.clone();
    let friends: Vec<classic_rules::ids::UnitHandle> = sc.theirs.iter().map(|p| p.h).collect();
    let ours = move |g: &GameState| -> Vec<TileId> {
        let mut v: Vec<TileId> = sc2.mine_alive(g).iter().map(|(_, c)| c.tile).filter(|t| dist(g, *t, home) <= 4).collect();
        if owner_of_city(g, home) == Some(sc2.learner) {
            v.push(home);
        }
        v
    };
    for p in sc.theirs.clone() {
        if p.role == "force" && act::alive(game, &p, me).is_some() {
            act::unit_turn(game, rules, me, foe, p.h, &Aim::To(home), Nerve::Reckless, &ours, &friends);
        } else if act::alive(game, &p, me).is_some() {
            act::stand(game, rules, p.h);
        }
    }
}

/// The units that hold the home city against a force of `n`: the capturers beyond the first, then the ranged beyond the
/// first two, then the rest (never the siege); the siege and the others go on.
fn pick_holders(sc: &Scene, n: usize) -> Vec<Piece> {
    let mut out: Vec<Piece> = Vec::new();
    let by = |role: &str| -> Vec<Piece> {
        let mut v: Vec<Piece> = sc.mine.iter().filter(|p| p.role == role).cloned().collect();
        v.sort_by_key(|p| p.id.0);
        v
    };
    let caps = by("capturer");
    let rng = by("ranged");
    out.extend(rng.iter().cloned());
    out.extend(caps.iter().skip(2).cloned());
    out.extend(caps.iter().take(2).cloned());
    out.truncate(n);
    out
}

fn fronts_teacher(rules: &CompiledRuleset, game: &mut GameState, sc: &mut Scene, name: &str) {
    let home = sc.refuge.unwrap_or(sc.centre);
    let force_alive = sc.theirs_alive(game).iter().filter(|(p, _)| p.role == "force").count();
    let spawned = sc.fam == Fam::TwoFronts || sc.info.contains_key("spawned");
    let all = sc.mine.clone();
    let n_force: usize = sc.info.get("force").and_then(|v| v.parse().ok()).unwrap_or(3);
    // The holders (None: no holders yet).
    let holders: Vec<Piece> = if !spawned {
        Vec::new()
    } else {
        match name {
            "ref-share" | "ref-detach" => pick_holders(sc, n_force),
            "ref-pile" | "ref-ignore" => Vec::new(),
            "ref-even" => all.iter().enumerate().filter(|(i, _)| i % 2 == 1).map(|(_, p)| p.clone()).collect(),
            // All on the defence first, and back to the siege when the force is dead.
            _ => {
                if force_alive > 0 || !sc.info.contains_key("spawned") && sc.fam == Fam::SecondCity {
                    all.clone()
                } else {
                    Vec::new()
                }
            }
        }
    };
    let holders = if name == "ref-abandon" {
        // Gives the siege up for good: every unit goes home when the force shows.
        if spawned {
            all.clone()
        } else {
            Vec::new()
        }
    } else if name == "ref-hold-first" || (name == "ref-detach" && sc.fam == Fam::SecondCity) {
        if force_alive > 0 && spawned {
            all.clone()
        } else {
            Vec::new()
        }
    } else {
        holders
    };
    let hold_ids: std::collections::BTreeSet<_> = holders.iter().map(|p| p.id).collect();
    // The holders defend the home city.
    let hold_group: Vec<Piece> = sc.mine.iter().filter(|p| hold_ids.contains(&p.id)).cloned().collect();
    if !hold_group.is_empty() {
        defend(rules, game, sc, &hold_group, home);
    }
    // The rest take the city.
    let mut sub = sc.clone();
    sub.mine.retain(|p| !hold_ids.contains(&p.id));
    if !sub.mine.is_empty() {
        let f = assault::flags("ref-planned").expect("a flag set");
        assault::turn(rules, game, &mut sub, f);
        sc.track = sub.track;
    }
}

fn fronts_judge(game: &GameState, sc: &Scene, rel: u32) -> Option<bool> {
    let city = sc.target.unwrap_or(sc.centre);
    let home = sc.refuge.unwrap_or(sc.centre);
    if owner_of_city(game, home) != Some(sc.learner) {
        return Some(false);
    }
    let force_dead = sc.theirs_alive(game).iter().all(|(p, _)| p.role != "force");
    let spawned = sc.fam == Fam::TwoFronts || sc.info.contains_key("spawned");
    if owner_of_city(game, city) == Some(sc.learner) && spawned && force_dead {
        return Some(true);
    }
    if rel >= sc.rounds || sc.mine_alive(game).is_empty() {
        return Some(false);
    }
    None
}

// ---------------------------------------------------------------------------
// twin-siege

/// Our army (3 capturers, 3 ranged, 2 siege) stands between two enemy cities 8 tiles apart: the strong one (walled,
/// two garrison units) and the weak one (worn down, one garrison unit); which of the two is west is drawn. A relief
/// force of two strong melee, 3 tiles off the middle, marches to the city where more of ours stand within 3.
/// WIN: both cities ours within the round limit with at most `TWK` of ours lost.
fn twin_at() -> [(i64, i64); 2] {
    let x = 5;
    [(-x, 0), (x, 0)]
}

fn build_twin(p: &mut Painter, lab: &Lab, instance: usize, attempt: u32) -> Result<Scene, String> {
    let mut s = blank(Fam::TwinSiege, lab, instance, attempt);
    let (me, foe) = (lab.learner, lab.enemy);
    let rules = p.rules;
    let at = |p: &Painter, d: (i64, i64)| p.at(d.0, d.1).ok_or_else(|| "off the map".to_owned());
    let strong_side = usize::from(p.d.chance(50));
    let mut tiles = Vec::new();
    for (i, d) in twin_at().iter().enumerate() {
        let t = at(p, *d)?;
        let strong = i == strong_side;
        let city = p.city(foe, t, if strong { 7 } else { 4 }, false);
        if strong {
            for b in defences(rules, lab.era) {
                p.game.debug_add_building(rules, city, b);
            }
            weaken(p, city, 40);
        } else {
            weaken(p, city, 5);
        }
        tiles.push((t, strong));
    }
    p.clutter(p.d.range(1, 3) as usize, &[tiles[0].0, tiles[1].0]);
    for (t, strong) in &tiles {
        let n = if *strong { 1 } else { 1 };
        for _ in 0..n {
            let gk = p.kind(if p.d.chance(50) { Arm::Ranged } else { Arm::Melee }).ok_or("no garrison")?;
            let spot = p.spots(*t, 1, 1).ok().and_then(|v| v.first().copied()).unwrap_or(*t);
            let at_t = if s.theirs.iter().any(|q| act::core_of(&p.game, q.h).is_some_and(|c| c.tile == *t)) { spot } else { *t };
            s.theirs.push(p.put(foe, at_t, gk, "garrison"));
        }
    }
    let mut plan: Vec<(Arm, &'static str)> = Vec::new();
    for _ in 0..4 {
        plan.push((Arm::Melee, "capturer"));
    }
    for _ in 0..4 {
        plan.push((Arm::Ranged, "ranged"));
    }
    for _ in 0..3 {
        plan.push((Arm::Siege, "siege"));
    }
    let anchor = at(p, (0, 0))?;
    let spots = p.spots(anchor, 2, plan.len())?;
    for ((arm, role), t) in plan.iter().zip(spots) {
        let k = p.kind(*arm).ok_or("no kind")?;
        s.mine.push(p.put(me, t, k, role));
    }
    let strong_kind = strongest_melee(p)?;
    let relief_at = at(p, (0, if p.d.chance(50) { -3 } else { 3 }))?;
    for t in p.spots(relief_at, 1, 2)? {
        s.theirs.push(p.put(foe, t, strong_kind, "relief"));
    }
    put_tiles(&mut s.info, "cities", &[tiles[0].0, tiles[1].0]);
    s.info.insert("strong".into(), strong_side.to_string());
    s.info.insert("a".into(), strong_side.to_string());
    s.target = Some(tiles[strong_side].0);
    s.aim_learner = Some(tiles[strong_side].0);
    s.centre = anchor;
    Ok(s)
}

/// Where the relief force goes: the city with more of ours within 3 (a tie: the weak one).
fn twin_enemy(rules: &CompiledRuleset, game: &mut GameState, sc: &mut Scene) {
    let (me, foe) = (sc.enemy, sc.learner);
    let cities = get_tiles(&sc.info, "cities");
    let strong: usize = sc.info.get("strong").and_then(|v| v.parse().ok()).unwrap_or(0);
    let ours: Vec<TileId> = sc.mine_alive(game).iter().map(|(_, c)| c.tile).collect();
    let near = |c: TileId| ours.iter().filter(|t| dist(game, **t, c) <= 3).count();
    let goal = if near(cities[strong]) > near(cities[1 - strong]) { cities[strong] } else { cities[1 - strong] };
    let sc2 = sc.clone();
    let friends: Vec<classic_rules::ids::UnitHandle> = sc.theirs.iter().map(|p| p.h).collect();
    let foes = move |g: &GameState| -> Vec<TileId> { sc2.mine_alive(g).iter().map(|(_, c)| c.tile).collect() };
    for p in sc.theirs.clone() {
        if act::alive(game, &p, me).is_none() {
            continue;
        }
        if p.role == "relief" {
            act::unit_turn(game, rules, me, foe, p.h, &Aim::To(goal), Nerve::Reckless, &foes, &friends);
        } else {
            act::stand(game, rules, p.h);
        }
    }
}

/// The army split in two for the cities (strong, weak): by role, the strong city's share of each role is 5/8 (at least one of each of the first two).
fn twin_groups(sc: &Scene, name: &str) -> (Vec<Piece>, Vec<Piece>) {
    let by = |role: &str| -> Vec<Piece> {
        let mut v: Vec<Piece> = sc.mine.iter().filter(|p| p.role == role).cloned().collect();
        v.sort_by_key(|p| p.id.0);
        v
    };
    let (mut a, mut b) = (Vec::new(), Vec::new());
    if name == "ref-equal" {
        for (i, p) in sc.mine.iter().enumerate() {
            if i % 2 == 0 { a.push(p.clone()) } else { b.push(p.clone()) }
        }
        return (a, b);
    }
    for role in ["capturer", "ranged", "siege"] {
        let v = by(role);
        let k = ((v.len() * 6 + 4) / 8).clamp(1, v.len().saturating_sub(1).max(1));
        a.extend(v.iter().take(k).cloned());
        b.extend(v.iter().skip(k).cloned());
    }
    (a, b)
}

fn twin_teacher(rules: &CompiledRuleset, game: &mut GameState, sc: &mut Scene, name: &str) {
    let cities = get_tiles(&sc.info, "cities");
    let strong: usize = sc.info.get("strong").and_then(|v| v.parse().ok()).unwrap_or(0);
    let (c_strong, c_weak) = (cities[strong], cities[1 - strong]);
    let flags = assault::flags("ref-planned").expect("a flag set");
    if name == "ref-serial" {
        // All on the strong city, then all on the weak one.
        let target = if owner_of_city(game, c_strong) == Some(sc.learner) { c_weak } else { c_strong };
        let mut sub = sc.clone();
        sub.target = Some(target);
        assault::turn(rules, game, &mut sub, flags);
        sc.track = sub.track;
        return;
    }
    let (ga, gb) = twin_groups(sc, name);
    let mut sub = sc.clone();
    sub.mine = ga;
    sub.target = Some(c_strong);
    sub.track = std::mem::take(&mut sc.track);
    if owner_of_city(game, c_strong) != Some(sc.learner) {
        assault::turn(rules, game, &mut sub, flags);
    }
    sc.track = sub.track;
    let mut sub = sc.clone();
    sub.mine = gb;
    sub.target = Some(c_weak);
    sub.track = std::mem::take(&mut sc.track2);
    if owner_of_city(game, c_weak) != Some(sc.learner) {
        assault::turn(rules, game, &mut sub, flags);
    }
    sc.track2 = sub.track;
}

fn twin_judge(game: &GameState, sc: &Scene, rel: u32) -> Option<bool> {
    let cities = get_tiles(&sc.info, "cities");
    let both = cities.iter().all(|c| owner_of_city(game, *c) == Some(sc.learner));
    let lost = sc.mine.iter().filter(|p| act::alive(game, p, sc.learner).is_none()).count();
    if lost > 3 {
        return Some(false);
    }
    if both {
        return Some(true);
    }
    if rel >= sc.rounds || sc.mine_alive(game).is_empty() {
        return Some(false);
    }
    None
}

// ---------------------------------------------------------------------------
// The dispatch (`families.rs`)

/// The enemy's turn of a history family.
pub fn enemy_turn(rules: &CompiledRuleset, game: &mut GameState, sc: &mut Scene) {
    match sc.fam {
        Fam::Feint | Fam::ScoutReport => march_enemy(rules, game, sc),
        Fam::FogSniper => sniper_enemy(rules, game, sc),
        Fam::CityReinforced => reinforced_enemy(rules, game, sc),
        Fam::TwinSiege => twin_enemy(rules, game, sc),
        _ => fronts_enemy(rules, game, sc),
    }
}

/// The learner's reference `name` for a history family. False: the name is none of this family's.
pub fn reference_turn(rules: &CompiledRuleset, game: &mut GameState, sc: &mut Scene, name: &str) -> bool {
    if !teachers(sc.fam).contains(&name) {
        return false;
    }
    match sc.fam {
        Fam::Feint | Fam::ScoutReport => march_teacher(rules, game, sc, name),
        Fam::FogSniper => sniper_teacher(rules, game, sc, name),
        Fam::CityReinforced => reinforced_teacher(rules, game, sc, name),
        Fam::TwinSiege => twin_teacher(rules, game, sc, name),
        _ => fronts_teacher(rules, game, sc, name),
    }
    true
}

/// The verdict at a round's end.
pub fn judge(_rules: &CompiledRuleset, game: &GameState, sc: &Scene, rel: u32) -> Option<bool> {
    match sc.fam {
        Fam::Feint | Fam::ScoutReport => march_judge(game, sc, rel),
        Fam::FogSniper => sniper_judge(game, sc, rel),
        Fam::CityReinforced => reinforced_judge(game, sc, rel),
        Fam::TwinSiege => twin_judge(game, sc, rel),
        _ => fronts_judge(game, sc, rel),
    }
}
