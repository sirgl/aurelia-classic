//! The third-party trainers (Roman, 24 Nov: "a nation that is not our enemy stands on the battlefield: teach the
//! army to tell it from a foe, to use it, and to keep its word with it"). Seven families on a lab of three nations
//! (`lab::third_lab`: the learner, the enemy at war with it, a THIRD nation at peace with both; `Scene::neutral` is the
//! third nation, `Scene::third` its pieces, a scripted human like the camp families' neutral).
//!
//! THE RULES AS FOUND (classic-rules; Unciv 4.22.10 behaviour):
//! - at peace a strike between two nations is refused by the rules; a unit may not stop on, or pass through, a tile that
//!   a military unit of another nation stands on (`act::free_for`, `reachable`).
//! - foreign LAND is closed to a unit unless the two are at war or the host grants entry (`kernel::world::territory`:
//!   `may_enter`: at war, an open-borders grant, or a host kind that keeps its land open). A city owns the tiles round
//!   it, so a neutral city is a blob that a path must go round; a grant (a deal's open borders) opens it (harness:
//!   `debug_open_borders`).
//! - Pillage is refused on a tile whose owner is not at war with the pillager (`validate_pillage`: "not at war"); a unit
//!   may stand on an ally's land only with open borders.
//! - a city strikes one foe a turn (the scene rule): the foes of a city are the pieces of every nation at war with its
//!   owner (`scene_rule::foe_tiles` reads the diplomacy), so an ally at war with the enemy shares the city's fire.
//! - the turn order is the nations' order: learner, enemy, third. Our blows come first in a round, the third's last.
//!
//! WHAT THE NET SEES (read from `army_net/observe.rs`, not changed): its foe tokens, threat counts, strike previews and
//! the "foe" planes of the map window are built from `orders.foe` (the enemy) and nothing else; a unit or a city of the
//! third nation is no token at all, and the land it owns is no plane. So the third party is invisible to the net as a
//! unit, as a threat, as an ally, as a border: the net meets it only as a tile it may not enter.
//!
//! THE FAMILIES (the win rules, the rounds and the caps are fixed in `GATES.md` "gate G5" before the first held-out run;
//! "lost" = our scene units dead, counted when the objective is reached where it is a capture). The scene numbers
//! below were tuned on the development instances 0-39, never the win rules:
//! - neutral-path: the enemy holds a position (3 fortified units, even strikes at what comes adjacent); between us and
//!   it three units of a neutral nation walk to and fro across the field (ours: 6 units). WIN: the enemy's units all dead
//!   within 12 rounds, at most 2 of ours lost. `ref-tp-ignore` fights the enemy and treats the neutral as an obstacle;
//!   `ref-tp-wary` treats it as a foe (its units close on the nearest "foe" including the neutral and keep out of its reach).
//! - ally-siege: an ally (at war with the enemy, borders open both ways) assaults the same walled city with its own
//!   army, from nearer. WIN: OUR unit takes the city within 14 rounds with at most 2 of ours lost (when it is taken);
//!   the ally taking it is a loss (the ally: 6 units from 5 tiles, ours 6 tiles out). `ref-ally-share` holds the army out
//!   of the city's reach until the ally's units draw its fire (3 rounds at most), then runs the planned assault;
//!   `ref-burst` rushes in alone at once (the wrong one: it takes the city's fire alone and breaks the cap).
//! - defend-friend: 6 enemy units march on a city of our ally (at war with the enemy; no walls, one garrison unit) from
//!   5 tiles; our army stands off to the side with a home city of its own. WIN: the ally's city is still the ally's after
//!   10 rounds (or the attackers are dead) with at most 3 of ours lost. `ref-defend-friend` fights the attackers; `ref-hold-own` guards only our own city.
//! - closed-borders: the straight way to an enemy city crosses a neutral city's land (closed borders; in one instance
//!   in five the neutral has opened them). WIN: our unit takes the city within 16 rounds with at most 2 lost.
//!   `ref-way-planned` walks the legal way (a distance field over the tiles it may enter); `ref-way-naive` walks the
//!   straight way and stalls at the border.
//! - spare-friends: an enemy city and an ally's city side by side, an improvement ring round each (the ally's nearer to
//!   us, its borders open to us). WIN: 3 enemy improvements pillaged within 10 rounds, at most 1 of ours lost (the rules
//!   refuse the pillage of the ally's: the guard is the time lost on them). `ref-spare` goes by the owner;
//!   `ref-near` goes to the nearest improvement.
//! - neutral-joins-war: 6 of ours hold a hill against one enemy unit; a neutral nation with 2 units 2 tiles behind us
//!   declares war on us at round 3. WIN: every military unit of the enemy and of the neutral dead within 10 rounds with at
//!   most 2 of ours lost. `ref-rethink` counts the new foe (targets, strikes, shots); `ref-old-plan` keeps fighting the
//!   enemy it knew (and is hunted).
//! - capital-race: a rival army (at war with the city's owner, at peace with us) marches on the same city from a tile
//!   2 tiles away (8 units; ours 2 tiles out; the city has no defence buildings). WIN: OUR unit takes the city within 14
//!   rounds with at most 5 of ours lost (the race is worth units); LOSS if the rival takes it. `ref-burst` (every unit
//!   strikes, the capturer goes in at the first blow that takes the city); `ref-planned` (the careful plan: stage, rotate;
//!   too slow for the rival's clock).

use std::rc::Rc;

use classic_rules::combat::city::CITY_ATTACK_RANGE;
use classic_rules::ids::{PlayerId, TileId, UnitHandle};
use classic_rules::{ClassicGame, Command, CompiledRuleset, GameState};
use kernel::world::unit::UNIT_MAX_HP;

use super::act::{self, Aim, Nerve};
use super::families::{blank, city_hp, defences, farm, Fam, Painter};
use super::lab::{dist, tiles_within, Arm, Lab};
use super::{assault, Piece, Scene};

pub fn is_third(f: Fam) -> bool {
    matches!(f, Fam::NeutralPath | Fam::AllySiege | Fam::DefendFriend | Fam::ClosedBorders | Fam::SpareFriends | Fam::NeutralJoinsWar | Fam::CapitalRace)
}

pub fn rounds(f: Fam) -> u32 {
    match f {
        Fam::NeutralPath => 12,
        Fam::AllySiege | Fam::CapitalRace => 14,
        Fam::DefendFriend | Fam::SpareFriends | Fam::NeutralJoinsWar => 10,
        _ => 16,
    }
}

/// The units of ours the win rule lets us lose.
pub fn loss_cap(f: Fam) -> usize {
    let (tag, default) = match f {
        Fam::NeutralPath => ("NP", 2),
        Fam::AllySiege => ("AS", 1),
        Fam::SpareFriends => ("SF", 1),
        Fam::NeutralJoinsWar => ("NJ", 2),
        Fam::DefendFriend => ("DF", 3),
        Fam::ClosedBorders => ("CB", 2),
        _ => ("CR", 5),
    };
    knob(&format!("CAP_{tag}"), default) as usize
}

/// The right teacher first (the proof's solver order), the wrong ones after.
pub fn solvers(f: Fam) -> &'static [&'static str] {
    match f {
        Fam::NeutralPath => &["ref-tp-ignore", "ref-tp-wary"],
        Fam::AllySiege => &["ref-ally-share", "ref-burst"],
        Fam::DefendFriend => &["ref-defend-friend", "ref-hold-own"],
        Fam::ClosedBorders => &["ref-way-planned", "ref-way-naive"],
        Fam::SpareFriends => &["ref-spare", "ref-near"],
        Fam::NeutralJoinsWar => &["ref-rethink", "ref-old-plan"],
        _ => &["ref-burst", "ref-planned"],
    }
}

pub fn task(f: Fam) -> &'static str {
    match f {
        Fam::NeutralPath => "kill an enemy position while a neutral's units at peace walk across the field",
        Fam::AllySiege => "take a city before the ally that assaults it, with the ally's help and at most 2 lost",
        Fam::DefendFriend => "keep an ally's city from an enemy army for 10 rounds",
        Fam::ClosedBorders => "take an enemy city when the short way crosses a neutral's closed borders",
        Fam::SpareFriends => "pillage 3 enemy improvements with an ally's improvements beside them",
        Fam::NeutralJoinsWar => "hold a hill when a neutral behind us declares war at round 3",
        _ => "take a city before a rival army at peace with us does (at most 5 lost)",
    }
}

pub fn is_reference(name: &str) -> bool {
    matches!(name, "ref-tp-ignore" | "ref-tp-wary" | "ref-ally-share" | "ref-defend-friend" | "ref-hold-own" | "ref-way-planned" | "ref-way-naive" | "ref-spare" | "ref-near" | "ref-rethink" | "ref-old-plan")
}

/// The scene's numbers, tuned on the development instances 0-39 (never the win rules).
/// closed-borders: one instance in this many percent has the borders open.
const OPEN_PERCENT: u64 = 20;
/// capital-race / ally-siege: the army of the third party is this many tiles from the city, ours 7.
const RIVAL_FROM: i64 = 2;
const ALLY_FROM: i64 = 5;
const OURS_FROM: i64 = 2;
const OURS_FROM_ALLY: i64 = 6;
/// neutral-joins-war: the round the neutral declares war (relative to the scene's start).
const JOIN_AT: u32 = 3;
/// neutral-joins-war: the neutral's units.
const JOIN_UNITS: usize = 2;
/// closed-borders: the neutral's land is a strip of 2 x BORDER_HALF_WIDTH + 1 tiles by 2 x BORDER_HALF_HEIGHT + 1.
const BORDER_HALF_WIDTH: i64 = 1;
const BORDER_HALF_HEIGHT: i64 = 6;
/// spare-friends: the enemy improvements to pillage.
const PILLAGE_K: usize = 3;
/// ally-siege: `ref-ally-share` waits for the ally's units at the city for this many rounds at most.
const SHARE_WAIT: u32 = 3;

/// A scene number the development sweeps move (`T3_<NAME>` in the environment; the defaults are the frozen numbers).
fn knob(name: &str, default: i64) -> i64 {
    std::env::var(format!("T3_{name}")).ok().and_then(|v| v.parse().ok()).unwrap_or(default)
}

fn at_off(game: &GameState, c: TileId, dx: i64, dy: i64) -> Option<TileId> {
    let d = game.view().auth.map().dimensions;
    let (w, h) = (i64::from(d.width), i64::from(d.height));
    let (x, y) = (i64::from(c.0) % w + dx, i64::from(c.0) / w + dy);
    (x >= 0 && y >= 0 && x < w && y < h).then(|| TileId((y * w + x) as u32))
}

fn handles(ps: &[Piece]) -> Vec<UnitHandle> {
    ps.iter().map(|p| p.h).collect()
}

fn role_of(rules: &CompiledRuleset, k: classic_rules::ids::UnitKindId, arm: Arm) -> &'static str {
    if arm == Arm::Siege {
        "siege"
    } else if act::range_of(rules, k).is_some() {
        "ranged"
    } else {
        "melee"
    }
}

/// The third nation's pieces alive.
fn third_alive(game: &GameState, sc: &Scene) -> Vec<(Piece, kernel::world::unit::UnitCore)> {
    super::camp::third_alive(game, sc)
}

/// The tiles of the third nation's pieces when it is at war with `side` (what a seat's scripts and rule strike).
pub fn third_foes_of(game: &GameState, sc: &Scene, side: PlayerId) -> Vec<TileId> {
    let Some(n) = sc.neutral else { return Vec::new() };
    if side == n || !game.view().auth.diplomacy().at_war(side, n) {
        return Vec::new();
    }
    third_alive(game, sc).iter().map(|(_, c)| c.tile).collect()
}

fn owner_of_city(game: &GameState, t: TileId) -> Option<PlayerId> {
    let auth = game.view().auth;
    auth.indexes().city_on_tile(t).and_then(|c| auth.cities().get(c)).map(|c| c.owner)
}

fn lost(game: &GameState, sc: &Scene) -> usize {
    sc.mine.len() - sc.mine_alive(game).len()
}

// ---------------------------------------------------------------------------
// The world: who is at war with whom

/// Whether the third nation is at war with the enemy in this family (an ally of the learner).
pub fn ally_at_war(f: Fam) -> bool {
    matches!(f, Fam::AllySiege | Fam::DefendFriend | Fam::CapitalRace | Fam::SpareFriends)
}

/// The round (relative to the start) the neutral declares war on the learner, if the family has it.
fn joins_at(sc: &Scene) -> Option<u32> {
    (sc.fam == Fam::NeutralJoinsWar).then_some(JOIN_AT)
}

/// The relations hold for the whole play (a mind of the three nations may declare a war or sign a peace; the scene's
/// relations are its given, as the camp's peace is): the learner and the enemy at war, the third at peace with the
/// learner (until its declaration) and at war with the enemy where the family has an ally.
pub fn keep_world(rules: &CompiledRuleset, game: &mut GameState, sc: &mut Scene) {
    let Some(n) = sc.neutral else { return };
    let (me, foe) = (sc.learner, sc.enemy);
    let rel = game.view().auth.turn().round.saturating_sub(sc.start_round);
    let mut forced = 0u32;
    if !game.view().auth.diplomacy().at_war(me, foe) {
        game.debug_force_war(rules, me, foe);
        forced += 1;
    }
    let war_with_us = joins_at(sc).is_some_and(|k| rel >= k);
    let at_war_us = game.view().auth.diplomacy().at_war(me, n);
    if war_with_us && !at_war_us {
        game.debug_force_war(rules, n, me);
        sc.info.entry("declared_at".into()).or_insert_with(|| rel.to_string());
    } else if !war_with_us && at_war_us {
        game.debug_force_peace(rules, me, n);
        forced += 1;
    }
    let want_ally = ally_at_war(sc.fam);
    let at_war_foe = game.view().auth.diplomacy().at_war(n, foe);
    if want_ally && !at_war_foe {
        game.debug_force_war(rules, n, foe);
        forced += 1;
    } else if !want_ally && at_war_foe {
        game.debug_force_peace(rules, n, foe);
        forced += 1;
    }
    if forced > 0 {
        let k = sc.info.get("world_forced").and_then(|v| v.parse::<u32>().ok()).unwrap_or(0) + forced;
        sc.info.insert("world_forced".into(), k.to_string());
    }
}

// ---------------------------------------------------------------------------
// The scenes

fn kind_of(p: &mut Painter, arm: Arm) -> Result<classic_rules::ids::UnitKindId, String> {
    p.kind(arm).ok_or_else(|| format!("no {} kind", arm.name()))
}

fn put_all(p: &mut Painter, side: PlayerId, anchor: TileId, r: i32, plan: &[Arm], into: &mut Vec<Piece>) -> Result<(), String> {
    let mut ks = Vec::new();
    for a in plan {
        ks.push((*a, kind_of(p, *a)?));
    }
    for ((arm, k), t) in ks.iter().zip(p.spots(anchor, r, plan.len())?) {
        let role = role_of(p.rules, *k, *arm);
        let piece = p.put(side, t, *k, role);
        into.push(piece);
    }
    Ok(())
}

pub(super) fn build(p: &mut Painter, fam: Fam, lab: &Lab, instance: usize, attempt: u32) -> Result<Scene, String> {
    let rules = p.rules;
    if !super::scene_rule::on() {
        return Err("the third-party families need the scene rule (the default)".into());
    }
    let n = lab.neutral.ok_or("no third nation in the lab")?;
    let (me, foe) = (lab.learner, lab.enemy);
    let mut s = blank(fam, lab, instance, attempt);
    s.neutral = Some(n);
    if ally_at_war(fam) {
        p.game.debug_force_war(rules, n, foe);
        // An ally's borders are open to us and ours to it.
        p.game.debug_open_borders(rules, n, me);
        p.game.debug_open_borders(rules, me, n);
        let d = p.game.view().auth.diplomacy();
        if !d.at_war(n, foe) || d.at_war(n, me) {
            return Err("the ally is not at war with the enemy only".into());
        }
    }
    match fam {
        Fam::NeutralPath => build_path(p, &mut s, lab)?,
        Fam::ClosedBorders => build_borders(p, &mut s, lab)?,
        Fam::SpareFriends => build_spare(p, &mut s, lab)?,
        Fam::NeutralJoinsWar => build_joins(p, &mut s, lab)?,
        Fam::DefendFriend => build_defend(p, &mut s, lab)?,
        _ => build_race(p, &mut s, lab)?,
    }
    s.info.insert("mine".into(), super::families::kinds_label(rules, &s.mine));
    s.info.insert("theirs".into(), super::families::kinds_label(rules, &s.theirs));
    s.info.insert("third".into(), super::families::kinds_label(rules, &s.third));
    Ok(s)
}

/// A city of `side` with the land round it owned out to `radius` tiles (a city the harness founds owns its ring only).
fn bordered_city(p: &mut Painter, side: PlayerId, t: TileId, pop: u32, radius: i32) -> classic_rules::ids::CityHandle {
    let c = p.city(side, t, pop, false);
    let tiles = tiles_within(&p.game, t, radius);
    p.game.debug_claim_tiles(p.rules, c, &tiles);
    c
}

fn at(p: &Painter, dx: i64, dy: i64) -> Result<TileId, String> {
    p.at(dx, dy).ok_or_else(|| "off the map".to_owned())
}

fn build_path(p: &mut Painter, s: &mut Scene, lab: &Lab) -> Result<(), String> {
    let rules = p.rules;
    let (me, foe, n) = (lab.learner, lab.enemy, lab.neutral.ok_or("no third")?);
    p.clutter(p.d.range(2, 5) as usize, &[]);
    // The enemy holds three units fortified round a point east of the centre.
    let hold = at(p, 3, 0)?;
    let plan = [Arm::Melee, Arm::Ranged, Arm::Melee];
    put_all(p, foe, hold, 1, &plan, &mut s.theirs)?;
    for q in s.theirs.clone() {
        let _ = act::fortify(&mut p.game, rules, foe, q.h);
    }
    // Ours, west of the centre: two melee and two shooters (a mounted unit for one of the melee half the time).
    let arms = [Arm::Melee, Arm::Ranged, if p.d.chance(50) { Arm::Mounted } else { Arm::Melee }, Arm::Ranged, Arm::Melee, Arm::Ranged];
    put_all(p, me, at(p, -4, 0)?, 2, &arms[..knob("NP_OURS", 6) as usize], &mut s.mine)?;
    // The neutral's three walkers between: a melee, a shooter, a mounted unit, on the lanes x = -2, 0, 1.
    let arms = [Arm::Melee, Arm::Ranged, Arm::Mounted];
    let mut ks = Vec::new();
    for a in arms {
        ks.push((a, kind_of(p, a)?));
    }
    for (i, (a, k)) in ks.iter().enumerate() {
        let lane = [-2i64, 0, 1][i];
        let t = at(p, lane, p.d.range(-2, 2))?;
        let t = if p.free(t) { t } else { p.spots(t, 1, 1)?[0] };
        let role = role_of(rules, *k, *a);
        let piece = p.put(n, t, *k, role);
        s.third.push(piece);
    }
    s.hold = Some(hold);
    Ok(())
}

fn build_borders(p: &mut Painter, s: &mut Scene, lab: &Lab) -> Result<(), String> {
    let rules = p.rules;
    let (me, foe, n) = (lab.learner, lab.enemy, lab.neutral.ok_or("no third")?);
    // The enemy's city east (weak: two people, no walls, one garrison), the neutral's city on the straight way with its
    // land a wall across it: a strip 3 tiles wide and 13 tall (a city the harness founds owns its ring only).
    let city_t = at(p, 5, 0)?;
    p.city(foe, city_t, p.d.range(2, 3) as u32, false);
    let gk = kind_of(p, Arm::Melee)?;
    let g = p.put(foe, city_t, gk, "garrison");
    s.theirs.push(g);
    let neutral_t = at(p, 0, 0)?;
    let nc = p.city(n, neutral_t, p.d.range(3, 4) as u32, false);
    let strip: Vec<TileId> = (-BORDER_HALF_WIDTH..=BORDER_HALF_WIDTH).flat_map(|dx| (-BORDER_HALF_HEIGHT..=BORDER_HALF_HEIGHT).map(move |dy| (dx, dy))).filter_map(|(dx, dy)| at(p, dx, dy).ok()).collect();
    p.game.debug_claim_tiles(rules, nc, &strip);
    let nk = kind_of(p, Arm::Melee)?;
    let ng = p.put(n, neutral_t, nk, "garrison");
    s.third.push(ng);
    if p.d.chance(OPEN_PERCENT) {
        p.game.debug_open_borders(rules, n, me);
        s.info.insert("borders".into(), "open".into());
    } else {
        s.info.insert("borders".into(), "closed".into());
    }
    let plan = [Arm::Melee, Arm::Ranged, Arm::Melee, Arm::Ranged, Arm::Siege, Arm::Melee];
    put_all(p, me, at(p, -5, 0)?, 2, &plan[..knob("CB_OURS", 5) as usize], &mut s.mine)?;
    s.target = Some(city_t);
    s.aim_learner = Some(city_t);
    s.centre = city_t;
    Ok(())
}

fn ring_owned(p: &Painter, city_t: TileId, owner: PlayerId, lo: i32, hi: i32, from: TileId) -> Vec<TileId> {
    let auth = p.game.view().auth;
    let mut v: Vec<TileId> = tiles_within(&p.game, city_t, hi).into_iter().filter(|t| (lo..=hi).contains(&dist(&p.game, *t, city_t)) && p.free(*t) && auth.map().owner(*t).flatten() == Some(owner)).collect();
    v.sort_by_key(|t| (dist(&p.game, *t, from), t.0));
    v
}

fn build_spare(p: &mut Painter, s: &mut Scene, lab: &Lab) -> Result<(), String> {
    let rules = p.rules;
    let (me, foe, n) = (lab.learner, lab.enemy, lab.neutral.ok_or("no third")?);
    let enemy_t = at(p, 6, -3)?;
    let ally_t = at(p, 1, 3)?;
    bordered_city(p, foe, enemy_t, p.d.range(3, 4) as u32, 3);
    bordered_city(p, n, ally_t, p.d.range(3, 4) as u32, 3);
    let gk = kind_of(p, Arm::Melee)?;
    let g = p.put(foe, enemy_t, gk, "garrison");
    s.theirs.push(g);
    let ak = kind_of(p, Arm::Melee)?;
    let ag = p.put(n, ally_t, ak, "garrison");
    s.third.push(ag);
    let farm = farm(rules, lab.grounds.open_index).ok_or("no improvement to pillage")?;
    // Improvements: 5 on tiles the enemy owns (2-3 from its city), 5 on tiles the ally owns (the ones nearest to us first).
    let from = at(p, -4, 0)?;
    let mut e = ring_owned(p, enemy_t, foe, 2, 3, from);
    let mut a = ring_owned(p, ally_t, n, 2, 3, from);
    p.d.shuffle(&mut e);
    p.d.shuffle(&mut a);
    e.truncate(5);
    a.sort_by_key(|t| (dist(&p.game, *t, from), t.0));
    a.truncate(5);
    if e.len() < 4 || a.len() < 3 {
        return Err(format!("improvement ring: {} enemy, {} ally tiles", e.len(), a.len()));
    }
    let all: Vec<(TileId, kernel::ids::ImprovementId)> = e.iter().chain(&a).map(|t| (*t, farm)).collect();
    p.game.debug_set_improvement_batch(rules, &all);
    s.improved = e.iter().chain(&a).copied().collect();
    s.info.insert("enemy_tiles".into(), e.iter().map(|t| t.0.to_string()).collect::<Vec<_>>().join(","));
    s.info.insert("ally_tiles".into(), a.iter().map(|t| t.0.to_string()).collect::<Vec<_>>().join(","));
    let n_raid = p.d.range(2, 3) as usize;
    let plan: Vec<Arm> = vec![Arm::HitRun; n_raid];
    put_all(p, me, at(p, -4, 0)?, 1, &plan, &mut s.mine)?;
    for q in &mut s.mine {
        q.role = "raider";
    }
    s.target = Some(enemy_t);
    s.centre = at(p, 3, 0)?;
    Ok(())
}

fn build_joins(p: &mut Painter, s: &mut Scene, lab: &Lab) -> Result<(), String> {
    let (me, foe, n) = (lab.learner, lab.enemy, lab.neutral.ok_or("no third")?);
    let c = at(p, 0, 0)?;
    p.clutter(p.d.range(2, 5) as usize, &[c]);
    // Ours at the centre; the enemy's small force comes from the east; the neutral stands behind us (west), fortified.
    let plan = [Arm::Melee, Arm::Ranged, Arm::Ranged, Arm::Melee, Arm::Melee, Arm::Ranged];
    put_all(p, me, at(p, 0, 0)?, 1, &plan[..knob("NJ_OURS", 6) as usize], &mut s.mine)?;
    put_all(p, foe, at(p, knob("NJ_ENEMY_AT", 6), 0)?, 1, &[Arm::Melee, Arm::Ranged, Arm::Melee][..knob("NJ_ENEMY_N", 1) as usize], &mut s.theirs)?;
    let mut plan = vec![Arm::Melee, Arm::Ranged, Arm::Mounted, Arm::Melee];
    plan.truncate(knob("NJ_UNITS", JOIN_UNITS as i64) as usize);
    put_all(p, n, at(p, -knob("NJ_AT", 2), 1)?, 1, &plan, &mut s.third)?;
    for q in s.third.clone() {
        let _ = act::fortify(&mut p.game, p.rules, n, q.h);
    }
    s.hold = Some(c);
    s.info.insert("join_at".into(), JOIN_AT.to_string());
    Ok(())
}

fn build_defend(p: &mut Painter, s: &mut Scene, lab: &Lab) -> Result<(), String> {
    let rules = p.rules;
    let (me, foe, n) = (lab.learner, lab.enemy, lab.neutral.ok_or("no third")?);
    let city_t = at(p, 2, 0)?;
    let ally_city = p.city(n, city_t, p.d.range(3, 4) as u32, false);
    for b in defences(rules, lab.era).into_iter().take(knob("DF_WALLS", 0) as usize) {
        p.game.debug_add_building(rules, ally_city, b);
    }
    p.clutter(p.d.range(1, 4) as usize, &[city_t]);
    let gk = kind_of(p, Arm::Melee)?;
    let g = p.put(n, city_t, gk, "garrison");
    s.third.push(g.clone());
    let _ = act::fortify(&mut p.game, rules, n, g.h);
    let sk = kind_of(p, if p.d.chance(50) { Arm::Ranged } else { Arm::Melee })?;
    let t = p.spots(city_t, 1, 1)?[0];
    if knob("DF_ALLY_UNITS", 1) >= 2 {
        let g2 = p.put(n, t, sk, "garrison");
        s.third.push(g2.clone());
        let _ = act::fortify(&mut p.game, rules, n, g2.h);
    }
    // The enemy army marches from the east: two melee, two shooters, a siege unit or a mounted one.
    let last = if p.d.chance(50) { Arm::Siege } else { Arm::Mounted };
    let plan = [Arm::Melee, Arm::Ranged, Arm::Melee, Arm::Ranged, last, Arm::Siege, Arm::Melee, Arm::Ranged];
    put_all(p, foe, at(p, knob("DF_AT", 5), 0)?, 2, &plan[..knob("DF_ENEMY", 6) as usize], &mut s.theirs)?;
    // Ours stand off to the north-west, with a home city of their own at their back.
    let home_t = at(p, -5, 5)?;
    p.city(me, home_t, 3, false);
    s.refuge = Some(home_t);
    put_all(p, me, at(p, -3, 3)?, 1, &[Arm::Melee, Arm::Ranged, Arm::Melee, Arm::Ranged], &mut s.mine)?;
    s.target = Some(city_t);
    s.hold = Some(city_t);
    s.centre = city_t;
    Ok(())
}

fn build_race(p: &mut Painter, s: &mut Scene, lab: &Lab) -> Result<(), String> {
    let rules = p.rules;
    let (me, foe, n) = (lab.learner, lab.enemy, lab.neutral.ok_or("no third")?);
    let ally = s.fam == Fam::AllySiege;
    let city_t = at(p, 4, 0)?;
    let pop = p.d.range(5, 8) as u32;
    let pop = if knob("RACE_POP", 0) > 0 { knob("RACE_POP", 0) as u32 } else { pop };
    let city = p.city(foe, city_t, pop, false);
    for b in defences(rules, lab.era).into_iter().take(knob("RACE_WALLS", if ally { 99 } else { 0 }) as usize) {
        p.game.debug_add_building(rules, city, b);
    }
    p.clutter(p.d.range(1, 3) as usize, &[city_t]);
    let gk = kind_of(p, if p.d.chance(50) { Arm::Ranged } else { Arm::Melee })?;
    let g = p.put(foe, city_t, gk, "garrison");
    s.theirs.push(g);
    // Ours, 7 tiles west; the third party's army north of the city, nearer (RIVAL_FROM / ALLY_FROM tiles).
    let plan = [Arm::Melee, Arm::Melee, Arm::Ranged, Arm::Ranged, Arm::Siege, Arm::Siege];
    let theirs = [Arm::Melee, Arm::Melee, Arm::Ranged, Arm::Ranged, Arm::Ranged, Arm::Siege, Arm::Siege, Arm::Melee];
    let ours_from = knob("OURS_FROM", if ally { OURS_FROM_ALLY } else { OURS_FROM });
    put_all(p, me, at(p, 4 - ours_from, 0)?, 2, &plan, &mut s.mine)?;
    let from = if ally { knob("ALLY_FROM", ALLY_FROM) } else { knob("RIVAL_FROM", RIVAL_FROM) };
    put_all(p, n, at(p, 4, -from)?, 2, &theirs[..knob("RIVAL_N", if ally { 6 } else { 8 }) as usize], &mut s.third)?;
    s.target = Some(city_t);
    s.aim_learner = Some(city_t);
    s.centre = city_t;
    Ok(())
}

// ---------------------------------------------------------------------------
// The legal way (closed-borders)

/// The length of the shortest way to `goal` over the tiles a unit of `side` may stand on and enter (borders count).
pub fn legal_field(game: &GameState, rules: &CompiledRuleset, side: PlayerId, goal: TileId) -> Rc<Vec<i32>> {
    let auth = game.view().auth;
    let dims = auth.map().dimensions;
    let n = auth.map().tile_count();
    let mut f = vec![i32::MAX / 2; n];
    let mut queue = std::collections::VecDeque::new();
    f[goal.0 as usize] = 0;
    queue.push_back(goal);
    while let Some(t) = queue.pop_front() {
        let d = f[t.0 as usize];
        for q in dims.neighbors(t).into_iter() {
            if f[q.0 as usize] <= d + 1 || !crate::battle::standable(rules, game, q) || !kernel::world::territory::may_enter(auth, side, q) {
                continue;
            }
            f[q.0 as usize] = d + 1;
            queue.push_back(q);
        }
    }
    Rc::new(f)
}

// ---------------------------------------------------------------------------
// The scripted third party

/// The third nation's turn (a scripted human), then `third_rule`.
pub fn third_turn(rules: &CompiledRuleset, game: &mut GameState, sc: &mut Scene) {
    let Some(n) = sc.neutral else { return };
    match sc.fam {
        Fam::NeutralPath => walk_turn(rules, game, sc, n),
        Fam::AllySiege | Fam::CapitalRace => race_turn(rules, game, sc, n),
        Fam::DefendFriend => {
            // The garrison stays; the other piece strikes what comes next to it at even odds or better.
            let foe = sc.enemy;
            let friends = handles(&sc.third);
            let targets = move |g: &GameState| act::foe_tiles(g, foe, None, false);
            for p in sc.third.clone() {
                if let Some(c) = act::alive(game, &p, n) {
                    act::unit_turn(game, rules, n, foe, p.h, &Aim::Hold(c.tile), Nerve::Even, &targets, &friends);
                }
            }
        }
        Fam::NeutralJoinsWar if game.view().auth.diplomacy().at_war(n, sc.learner) => {
            // After its declaration: a hunter (strikes at even odds or better, ranged units behind a melee unit).
            let me = sc.learner;
            let friends = handles(&sc.third);
            let targets = move |g: &GameState| act::foe_tiles(g, me, None, false);
            for p in sc.third.clone() {
                let Some(c) = act::alive(game, &p, n) else { continue };
                let ranged = act::range_of(rules, c.kind).is_some();
                act::unit_turn(game, rules, n, me, p.h, if ranged { &Aim::Support } else { &Aim::Close }, Nerve::Even, &targets, &friends);
            }
        }
        _ => {}
    }
}

/// The neutral's walkers (neutral-path): each goes to and fro between two points on its lane, three tiles either side of
/// the centre, switching every second round.
fn walk_turn(rules: &CompiledRuleset, game: &mut GameState, sc: &mut Scene, n: PlayerId) {
    let rel = game.view().auth.turn().round.saturating_sub(sc.start_round);
    for (i, p) in sc.third.clone().into_iter().enumerate() {
        let Some(c) = act::alive(game, &p, n) else { continue };
        let lane = [-2i64, 0, 1][i % 3];
        let up = ((rel / 2) as usize + i) % 2 == 0;
        let Some(wp) = at_off(game, sc.centre, lane, if up { 3 } else { -3 }) else { continue };
        let _ = c;
        act::move_by(game, rules, n, p.h, |g, x, _| (dist(g, x, wp), x.0));
    }
}

/// The ally's / rival's assault on the city: the shooters fire from where they can; a melee unit strikes only a blow
/// that takes the city (or kills a unit) and else stands two tiles off.
fn race_turn(rules: &CompiledRuleset, game: &mut GameState, sc: &mut Scene, n: PlayerId) {
    let (foe, city) = (sc.enemy, sc.target.unwrap_or(sc.centre));
    let friends = handles(&sc.third);
    let targets = move |g: &GameState| {
        let mut v: Vec<TileId> = act::foe_tiles(g, foe, None, false).into_iter().filter(|t| dist(g, *t, city) <= 2).collect();
        if owner_of_city(g, city) == Some(foe) {
            v.push(city);
        }
        v.sort_by_key(|t| t.0);
        v.dedup();
        v
    };
    let mut order = sc.third.clone();
    order.sort_by_key(|p| (u8::from(!matches!(p.role, "ranged" | "siege")), p.id.0));
    for p in order {
        let Some(c) = act::alive(game, &p, n) else { continue };
        if matches!(p.role, "ranged" | "siege") {
            act::unit_turn(game, rules, n, foe, p.h, &Aim::Support, Nerve::Even, &targets, &friends);
            continue;
        }
        // The capture: a blow that takes the city.
        if owner_of_city(game, city) == Some(foe) && act::reach_blow(game, rules, n, p.h, city).is_some_and(|(d, hp)| d >= hp) {
            if let Some((_, t)) = act::best_blow(game, rules, n, p.h, &[city], Nerve::Reckless) {
                if act::strike(game, rules, n, p.h, t) {
                    act::stand(game, rules, p.h);
                    continue;
                }
            }
            if act::approach_strike(game, rules, n, foe, p.h, &[city], Nerve::Reckless) {
                act::stand(game, rules, p.h);
                continue;
            }
        }
        // A kill on a unit near the city.
        let units: Vec<TileId> = act::foe_tiles(game, foe, None, false);
        if let Some((score, t)) = act::best_blow(game, rules, n, p.h, &units, Nerve::Careful) {
            if score >= 10_000 && act::strike(game, rules, n, p.h, t) {
                act::stand(game, rules, p.h);
                continue;
            }
        }
        // Two tiles from the city (out of nobody's way but the city's strike: it takes the melee's place beside it later).
        let _ = c;
        act::move_by(game, rules, n, p.h, |g, x, _| ((dist(g, x, city) - 2).abs(), act::threat_on(g, rules, foe, x), x.0));
        act::stand(game, rules, p.h);
    }
}

/// The scene rule's part of the third nation's turn (promotions, its cities' strikes, shots, stand and fortify), as
/// `scene_rule::turn` for a seat; its foes are the pieces of the nations it is at war with.
pub fn third_rule(rules: &CompiledRuleset, game: &mut GameState, sc: &Scene) {
    let Some(n) = sc.neutral else { return };
    let foes_of = |g: &GameState| -> Vec<TileId> {
        let mut v = Vec::new();
        if g.view().auth.diplomacy().at_war(n, sc.enemy) {
            v.extend(sc.theirs_alive(g).iter().map(|(_, c)| c.tile));
        }
        if g.view().auth.diplomacy().at_war(n, sc.learner) {
            v.extend(sc.mine_alive(g).iter().map(|(_, c)| c.tile));
        }
        v.sort_by_key(|t| t.0);
        v.dedup();
        v
    };
    crate::army_net::obsdump::set_act_src("rule");
    for p in &sc.third {
        let Some(c) = act::alive(game, p, n) else { continue };
        if let Some(promotion) = super::scene_rule::promotion_pick(game, rules, &c) {
            act::try_cmd(game, rules, n, Command::Promote { unit: p.h, promotion });
        }
    }
    let mut cities: Vec<classic_rules::ids::CityHandle> = game.view().auth.indexes().cities_of(n).to_vec();
    cities.sort_by_key(|c| c.index);
    for city in cities {
        let foes = foes_of(game);
        if let Some(target) = super::scene_rule::city_target(game, rules, n, city, &foes) {
            act::try_cmd(game, rules, n, Command::CityStrike { city, target });
        }
    }
    for p in &sc.third {
        if act::alive(game, p, n).is_none() {
            continue;
        }
        let foes = foes_of(game);
        if let Some(target) = super::scene_rule::shot_target(game, rules, n, p.h, &foes) {
            act::try_cmd(game, rules, n, Command::RangedAttack { unit: p.h, target });
        }
    }
    for p in &sc.third {
        let Some(c) = act::alive(game, p, n) else { continue };
        let stood = c.moves_left > 0 && !c.has_moved();
        if c.moves_left > 0 {
            act::hold_unit(game, rules, p.h);
        }
        if stood && super::scene_rule::fortifies(game, rules, n, p.h) {
            act::try_cmd(game, rules, n, Command::Fortify { unit: p.h });
        }
    }
    crate::army_net::obsdump::set_act_src("act");
}

/// The enemy's turn (a scripted human): what it strikes is every piece of a nation it is at war with.
pub fn enemy_turn(rules: &CompiledRuleset, game: &mut GameState, sc: &mut Scene) {
    let (me, foe) = (sc.enemy, sc.learner);
    let sc2 = sc.clone();
    let friends = handles(&sc2.theirs);
    let targets = move |g: &GameState| -> Vec<TileId> {
        let mut v: Vec<TileId> = sc2.mine_alive(g).iter().map(|(_, c)| c.tile).collect();
        v.extend(third_foes_of(g, &sc2, sc2.enemy));
        v.sort_by_key(|t| t.0);
        v.dedup();
        v
    };
    let with_city = {
        let sc3 = sc.clone();
        move |g: &GameState| -> Vec<TileId> {
            let mut v = targets(g);
            // The city of the ally (defend-friend) is a target.
            if sc3.fam == Fam::DefendFriend {
                if let Some(t) = sc3.target {
                    if owner_of_city(g, t) == sc3.neutral {
                        v.push(t);
                    }
                }
            }
            v.sort_by_key(|t| t.0);
            v.dedup();
            v
        }
    };
    for p in sc.theirs.clone() {
        let Some(c) = act::alive(game, &p, me) else { continue };
        let ranged = act::range_of(rules, c.kind).is_some();
        match sc.fam {
            Fam::DefendFriend => {
                let city = sc.target.unwrap_or(sc.centre);
                let aim = if ranged { Aim::Support } else { Aim::To(city) };
                act::unit_turn(game, rules, me, foe, p.h, &aim, Nerve::Even, &with_city, &friends);
            }
            Fam::NeutralJoinsWar => {
                act::unit_turn(game, rules, me, foe, p.h, if ranged { &Aim::Support } else { &Aim::Close }, Nerve::Even, &with_city, &friends);
            }
            _ => {
                act::unit_turn(game, rules, me, foe, p.h, &Aim::Hold(c.tile), Nerve::Even, &with_city, &friends);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// The references

pub fn reference_turn(rules: &CompiledRuleset, game: &mut GameState, sc: &mut Scene, name: &str) {
    match name {
        "ref-tp-ignore" | "ref-tp-wary" => fight_turn(rules, game, sc, name == "ref-tp-wary"),
        "ref-ally-share" => ally_share_turn(rules, game, sc),
        "ref-defend-friend" => fight_turn(rules, game, sc, false),
        "ref-hold-own" => hold_own_turn(rules, game, sc),
        "ref-way-planned" | "ref-way-naive" => way_turn(rules, game, sc, name == "ref-way-planned"),
        "ref-spare" | "ref-near" => pillage_turn(rules, game, sc, name == "ref-spare"),
        "ref-rethink" | "ref-old-plan" => joins_turn(rules, game, sc, name == "ref-rethink"),
        _ => {}
    }
}

/// The competent fight against the enemy (the shooters first, behind the melee; a blow at even odds or a kill). `wary`:
/// the third nation's units count as foes: the units close on the nearest "foe" of either nation and keep out of the
/// reach of the third nation's units (the blows the rules refuse are never struck: they are only a thing to go to or away from).
fn fight_turn(rules: &CompiledRuleset, game: &mut GameState, sc: &mut Scene, wary: bool) {
    let (me, foe) = (sc.learner, sc.enemy);
    let friends = handles(&sc.mine);
    let mut order = sc.mine.clone();
    order.sort_by_key(|p| (u8::from(!matches!(p.role, "ranged" | "siege")), p.id.0));
    for p in order {
        let Some(c) = act::alive(game, &p, me) else { continue };
        let ranged = act::range_of(rules, c.kind).is_some();
        let scc = sc.clone();
        let hits = move |g: &GameState| act::foe_tiles(g, foe, Some(&scc.theirs), false);
        if !wary {
            act::unit_turn(game, rules, me, foe, p.h, if ranged { &Aim::Support } else { &Aim::Close }, Nerve::Even, &hits, &friends);
            continue;
        }
        // Wary: strike the enemy where it can, then move by the nearest foe of either nation, out of the third's reach.
        if let Some((_, t)) = act::best_blow(game, rules, me, p.h, &hits(game), Nerve::Even) {
            let _ = act::strike(game, rules, me, p.h, t);
        }
        if act::core_of(game, p.h).is_some_and(|u| u.owner == me && u.moves_left > 0) {
            let reach = act::range_of(rules, c.kind).unwrap_or(1);
            let others: Vec<TileId> = third_alive(game, sc).iter().map(|(_, u)| u.tile).collect();
            let all: Vec<TileId> = hits(game).into_iter().chain(others.iter().copied()).collect();
            let rules2 = rules;
            let others2 = third_alive(game, sc);
            act::move_by(game, rules, me, p.h, |g, x, _| {
                let danger = others2.iter().filter(|(_, u)| dist(g, u.tile, x) <= act::moves_of(rules2, u.kind) + act::range_of(rules2, u.kind).unwrap_or(1)).count();
                let d = act::nearest(g, x, &all);
                (danger, if ranged { (d - reach).abs() } else { (d - 1).max(0) }, -act::defence(g, rules2, x, me), x.0)
            });
            if let Some((_, t)) = act::best_blow(game, rules, me, p.h, &hits(game), Nerve::Even) {
                let _ = act::strike(game, rules, me, p.h, t);
            }
        }
        act::stand(game, rules, p.h);
    }
}

/// defend-friend's wrong teacher: the army guards its own city and nothing else.
fn hold_own_turn(rules: &CompiledRuleset, game: &mut GameState, sc: &mut Scene) {
    let (me, foe) = (sc.learner, sc.enemy);
    let home = sc.refuge.unwrap_or(sc.centre);
    let friends = handles(&sc.mine);
    let scc = sc.clone();
    let hits = move |g: &GameState| act::foe_tiles(g, foe, Some(&scc.theirs), false);
    for p in sc.mine.clone() {
        if act::alive(game, &p, me).is_none() {
            continue;
        }
        act::unit_turn(game, rules, me, foe, p.h, &Aim::Hold(home), Nerve::Careful, &hits, &friends);
    }
}

/// ally-siege: stay out of the city's reach until the ally's units draw its fire, then the burst with a fresh capturer.
fn ally_share_turn(rules: &CompiledRuleset, game: &mut GameState, sc: &mut Scene) {
    let (me, foe) = (sc.learner, sc.enemy);
    let city = sc.target.unwrap_or(sc.centre);
    let rel = game.view().auth.turn().round.saturating_sub(sc.start_round);
    let drawn = third_alive(game, sc).iter().any(|(_, c)| dist(game, c.tile, city) <= CITY_ATTACK_RANGE);
    if !sc.track.engaged && !drawn && rel < SHARE_WAIT {
        // Stage three tiles from the city (outside its strike), as the burst's staging does.
        for p in sc.mine.clone() {
            if act::alive(game, &p, me).is_none() {
                continue;
            }
            act::move_by(game, rules, me, p.h, |g, x, _| (dist(g, x, city) <= CITY_ATTACK_RANGE, (dist(g, x, city) - CITY_ATTACK_RANGE - 1).abs(), act::threat_on(g, rules, foe, x), x.0));
            act::stand(game, rules, p.h);
        }
        return;
    }
    if let Some(f) = assault::flags("ref-planned") {
        assault::turn(rules, game, sc, f);
    }
}

/// closed-borders: march (the legal way or the straight one), then the assault when the army is near.
fn way_turn(rules: &CompiledRuleset, game: &mut GameState, sc: &mut Scene, planned: bool) {
    let (me, foe) = (sc.learner, sc.enemy);
    let city = sc.target.unwrap_or(sc.centre);
    let alive = sc.mine_alive(game);
    if alive.iter().all(|(_, c)| dist(game, c.tile, city) <= 7) {
        if let Some(f) = assault::flags("ref-burst") {
            assault::turn(rules, game, sc, f);
        }
        return;
    }
    let field = legal_field(game, rules, me, city);
    let friends = handles(&sc.mine);
    let scc = sc.clone();
    let hits = move |g: &GameState| act::foe_tiles(g, foe, Some(&scc.theirs), false);
    for p in sc.mine.clone() {
        if act::alive(game, &p, me).is_none() {
            continue;
        }
        let aim = if planned { Aim::Field(field.clone()) } else { Aim::To(city) };
        act::unit_turn(game, rules, me, foe, p.h, &aim, Nerve::Even, &hits, &friends);
    }
}

/// spare-friends: pillage; `by_owner`: only the improvements on the enemy's land, else the nearest of all.
fn pillage_turn(rules: &CompiledRuleset, game: &mut GameState, sc: &mut Scene, by_owner: bool) {
    let (me, foe) = (sc.learner, sc.enemy);
    let strike = CITY_ATTACK_RANGE;
    let enemy_city = sc.target;
    for p in sc.mine.clone() {
        let Some(c) = act::alive(game, &p, me) else { continue };
        let auth = game.view().auth;
        let open: Vec<TileId> = sc
            .improved
            .iter()
            .copied()
            .filter(|t| !auth.map().pillaged(*t) && matches!(auth.map().improvement(*t), Some(Some(_))))
            .filter(|t| !by_owner || auth.map().owner(*t).flatten() == Some(foe))
            .collect();
        if open.contains(&c.tile) && act::try_cmd(game, rules, me, Command::Pillage { unit: p.h }) {
            act::stand(game, rules, p.h);
            continue;
        }
        let safe: Vec<TileId> = open.iter().copied().filter(|t| enemy_city.is_none_or(|ct| dist(game, *t, ct) > strike)).collect();
        let goals = if safe.is_empty() || !by_owner { open.clone() } else { safe };
        if let Some(goal) = goals.iter().copied().min_by_key(|t| (dist(game, c.tile, *t), t.0)) {
            act::move_by(game, rules, me, p.h, |g, x, _| (dist(g, x, goal), x.0));
            if act::alive(game, &p, me).is_some_and(|u| u.tile == goal) {
                let _ = act::try_cmd(game, rules, me, Command::Pillage { unit: p.h });
            }
        }
        act::stand(game, rules, p.h);
    }
}

/// neutral-joins-war: the competent fight (shooters first, behind the melee; a blow at even odds or a kill). `rethink`: the
/// neutral at war with us is a foe (targets, blows); else the old plan: the enemy it knew only.
fn joins_turn(rules: &CompiledRuleset, game: &mut GameState, sc: &mut Scene, rethink: bool) {
    let (me, foe) = (sc.learner, sc.enemy);
    let friends = handles(&sc.mine);
    let scc = sc.clone();
    let hits = move |g: &GameState| -> Vec<TileId> {
        let mut v = act::foe_tiles(g, foe, Some(&scc.theirs), false);
        if rethink {
            v.extend(third_foes_of(g, &scc, scc.learner));
        }
        v.sort_by_key(|t| t.0);
        v.dedup();
        v
    };
    let mut order = sc.mine.clone();
    order.sort_by_key(|p| (u8::from(!matches!(p.role, "ranged" | "siege")), p.id.0));
    for p in order {
        let Some(c) = act::alive(game, &p, me) else { continue };
        let ranged = act::range_of(rules, c.kind).is_some();
        let _ = ranged;
        let hill = sc.hold.unwrap_or(c.tile);
        act::unit_turn(game, rules, me, foe, p.h, &Aim::Hold(hill), Nerve::Even, &hits, &friends);
    }
}

// ---------------------------------------------------------------------------
// The judge

/// Called after every seat's script: the first time the city is seen taken, who took it and what ours had lost by then.
pub fn mark(game: &GameState, sc: &mut Scene) {
    if let Some(t) = sc.target {
        let owner = owner_of_city(game, t);
        if owner.is_some() && owner != Some(sc.enemy) && matches!(sc.fam, Fam::AllySiege | Fam::CapitalRace | Fam::ClosedBorders) && !sc.info.contains_key("taken_by") {
            sc.info.insert("taken_by".into(), if owner == Some(sc.learner) { "me" } else if owner == sc.neutral { "third" } else { "other" }.into());
            sc.info.insert("lost_at_take".into(), lost(game, sc).to_string());
        }
    }
}

fn enemy_improvements_pillaged(game: &GameState, sc: &Scene) -> usize {
    let list = sc.info.get("enemy_tiles").cloned().unwrap_or_default();
    list.split(',').filter_map(|v| v.parse::<u32>().ok()).filter(|t| game.view().auth.map().pillaged(TileId(*t))).count()
}

/// The verdict at a round's end.
pub fn judge(rules: &CompiledRuleset, game: &GameState, sc: &mut Scene, rel: u32) -> Option<bool> {
    mark(game, sc);
    let cap = loss_cap(sc.fam);
    let limit = rel >= sc.rounds;
    let gone = lost(game, sc);
    let all_dead = sc.mine_alive(game).is_empty();
    // What counts for the captures is what was lost when the city was taken.
    let counted = match (sc.info.get("taken_by"), sc.info.get("lost_at_take").and_then(|v| v.parse::<usize>().ok())) {
        (Some(_), Some(l)) => l,
        _ => gone,
    };
    if counted > cap && sc.cap_broken.is_none() {
        sc.cap_broken = Some(rel);
    }
    match sc.fam {
        Fam::NeutralPath => {
            let left = sc.theirs_alive(game).iter().filter(|(p, _)| !act::is_civilian(rules, p.kind)).count();
            if sc.cap_broken.is_some() || all_dead || (limit && left > 0) {
                Some(false)
            } else if left == 0 {
                Some(true)
            } else {
                None
            }
        }
        Fam::AllySiege | Fam::CapitalRace | Fam::ClosedBorders => match sc.info.get("taken_by").map(String::as_str) {
            Some("me") => Some(sc.cap_broken.is_none()),
            Some(_) => Some(false),
            None if sc.cap_broken.is_some() || all_dead || limit => Some(false),
            None => None,
        },
        Fam::DefendFriend => {
            let city = sc.target.unwrap_or(sc.centre);
            let held = owner_of_city(game, city) == sc.neutral;
            let attackers = sc.theirs_alive(game).iter().filter(|(p, _)| !act::is_civilian(rules, p.kind)).count();
            if !held || sc.cap_broken.is_some() || all_dead {
                Some(false)
            } else if attackers == 0 || limit {
                Some(true)
            } else {
                None
            }
        }
        Fam::SpareFriends => {
            let done = enemy_improvements_pillaged(game, sc);
            if sc.cap_broken.is_some() || all_dead {
                Some(false)
            } else if done >= PILLAGE_K {
                sc.taken = sc.taken.or(Some(rel));
                Some(true)
            } else if limit {
                Some(false)
            } else {
                None
            }
        }
        Fam::NeutralJoinsWar => {
            let left = sc.theirs_alive(game).iter().filter(|(p, _)| !act::is_civilian(rules, p.kind)).count()
                + third_alive(game, sc).iter().filter(|(p, _)| !act::is_civilian(rules, p.kind)).count();
            if sc.cap_broken.is_some() || all_dead || (limit && left > 0) {
                Some(false)
            } else if left == 0 {
                Some(true)
            } else {
                None
            }
        }
        _ => None,
    }
}

pub fn measures(rules: &CompiledRuleset, game: &GameState, sc: &Scene, m: &mut std::collections::BTreeMap<String, f64>) {
    let _ = rules;
    m.insert("third_taken".into(), f64::from(u8::from(sc.info.get("taken_by").is_some_and(|v| v == "third"))));
    m.insert("lost_at_take".into(), sc.info.get("lost_at_take").and_then(|v| v.parse().ok()).unwrap_or(-1.0));
    m.insert("cap_broken".into(), sc.cap_broken.map_or(-1.0, f64::from));
    m.insert("third_lost".into(), (sc.third.len() - third_alive(game, sc).len()) as f64);
    m.insert("world_forced".into(), sc.info.get("world_forced").and_then(|v| v.parse().ok()).unwrap_or(0.0));
    if sc.fam == Fam::SpareFriends {
        m.insert("enemy_pillaged".into(), enemy_improvements_pillaged(game, sc) as f64);
    }
    let _ = (UNIT_MAX_HP, city_hp);
}
