//! The memory trainers (work/nets/army-net/M-HANDOFF.md, proof (c)): the right act depends on something the
//! army saw earlier and does not see now. These families play in the fog: each side sees what its units and
//! cities see (the rules' sight: units 2, cities 3, hills, mountains and forests shut the line), and the army
//! net's tokens hold only the foes our side sees (`army_net::Orders::fog`).
//!
//! WIN RULES (fixed before the first run; "lost" = our scene units dead or captured; version 2, after the first
//! measurement: a net with no learned memory trained on the same data won both, because the army could still
//! choose while the foe was in sight and its position then carried the choice; now the army comes down a
//! one-tile corridor and leaves it only after the foe is out of sight):
//! - ambush-fog: two routes round a mountain block lead from our corridor to our refuge city. At the start a
//!   strong stack of the enemy (3 of the era's strongest melee and mounted) stands at the mouth of one of
//!   them, in sight of our outpost city at the fork; from its second turn it walks into the forest at the
//!   far end of that route, out of sight, and waits there: it strikes any of ours that comes within three
//!   tiles. Our army starts 12-15 tiles down the corridor. WIN: within 18 rounds every unit of ours alive stands within 2 tiles of the refuge at a round's
//!   end with at most 1 of ours lost. Which route is safe is seen only at the start.
//! - hunt-fog: an enemy worker stands next to our outpost city at the start; from its second turn it walks to
//!   a forest 6 tiles north or south (seen for its first step, then out of sight) and stays there. Our three
//!   melee start 13-15 tiles west in a corridor that ends 3 tiles before the city. WIN: the worker captured or
//!   killed within 12 rounds. Where it went is seen
//!   only in its first steps; the two forests are 12 tiles apart, so a wrong guess costs the time.
//!
//! THE REFERENCES: `ref-recall` keeps the foes it saw (`Track::seen`: the last tile and round of each) and
//! chooses by them (the route away from the stack; the forest on the side the worker was last seen);
//! `ref-forget` is the same script without that memory (it takes the north route, searches the north forest
//! first): the control that shows the memory is what wins.

use classic_rules::ids::TileId;
use classic_rules::{Command, CompiledRuleset, GameState};
use kernel::world::unit::UnitCore;

use super::act::{self, Aim, Nerve};
use super::families::{blank, Fam, Painter};
use super::lab::{dist, Arm, Lab};
use super::{Piece, Scene};

pub fn is_memory(f: Fam) -> bool {
    matches!(f, Fam::AmbushFog | Fam::HuntFog)
}

pub fn rounds(f: Fam) -> u32 {
    match f {
        Fam::AmbushFog => 18,
        _ => 12,
    }
}

pub fn solvers(_f: Fam) -> &'static [&'static str] {
    &["ref-recall"]
}

pub fn task(f: Fam) -> &'static str {
    match f {
        Fam::AmbushFog => "take the route the ambush did not walk into (seen at the start only)",
        _ => "catch a worker that walked into the fog (its way seen for one step)",
    }
}

/// The foes `side` sees now (the rules' sight).
pub fn seen_foes(rules: &CompiledRuleset, game: &GameState, side: classic_rules::ids::PlayerId, foe: classic_rules::ids::PlayerId) -> Vec<UnitCore> {
    let auth = game.view().auth;
    let view = game.player_view(rules, side);
    auth.indexes().units_of(foe).iter().filter_map(|h| auth.units().get(*h)).map(|u| *u.core).filter(|c| view.sees_unit(c)).collect()
}

fn rel_xy(game: &GameState, centre: TileId, t: TileId) -> (i64, i64) {
    let w = i64::from(game.view().auth.map().dimensions.width);
    (i64::from(t.0) % w - i64::from(centre.0) % w, i64::from(t.0) / w - i64::from(centre.0) / w)
}

/// The tiles are axial (q, r) offsets from the arena's centre. The south side is drawn; the north is its mirror
/// across the row r = 0, (q, r) -> (q + r, -r), the reflection that keeps the hex grid (and every distance).
pub fn mirror(dq: i64, dr: i64, side: i64) -> (i64, i64) {
    if side > 0 {
        (dq, dr)
    } else {
        (dq + dr, -dr)
    }
}

/// Hex distance of two axial offsets.
fn hd(a: (i64, i64), b: (i64, i64)) -> i64 {
    let (dq, dr) = (a.0 - b.0, a.1 - b.1);
    (dq.abs() + dr.abs() + (dq + dr).abs()) / 2
}

/// ambush-fog: the block is every tile within 1 of the segment (-1, 0)..(2, 0); the routes are the tiles 2 and 3
/// from it, the outer wall the tiles 4 from it (west of x 3.5, off the row r = 0); the corridor's walls the rows
/// r = 1 (q -10..-4) and their mirror.
const SEGMENT: [(i64, i64); 4] = [(-1, 0), (0, 0), (1, 0), (2, 0)];
fn from_segment(t: (i64, i64)) -> i64 {
    SEGMENT.iter().map(|x| hd(*x, t)).min().unwrap_or(99)
}
/// The waypoints of the south route (mirrored for the north) and the stack's hiding place (south).
pub const ROUTE: [(i64, i64); 2] = [(-2, 2), (4, 1)];
const HIDE_AMBUSH: (i64, i64) = (2, 2);
/// hunt-fog: the worker's start, its first step (seen), its forest (south; mirrored for the north).
const PREY_START: (i64, i64) = (3, 0);
const PREY_STEP: (i64, i64) = (1, 2);
const HIDE_PREY: (i64, i64) = (0, 6);

pub(super) fn build(p: &mut Painter, fam: Fam, lab: &Lab, instance: usize, attempt: u32) -> Result<Scene, String> {
    let rules = p.rules;
    let mut s = blank(fam, lab, instance, attempt);
    let (me, foe) = (lab.learner, lab.enemy);
    let at = |p: &Painter, d: (i64, i64)| p.at(d.0, d.1).ok_or_else(|| "off the map".to_owned());
    let side: i64 = if p.d.chance(50) { 1 } else { -1 };
    s.info.insert("side".into(), if side > 0 { "south" } else { "north" }.to_owned());
    let m = |d: (i64, i64)| mirror(d.0, d.1, side);
    match fam {
        Fam::AmbushFog => {
            let mountain = lab.grounds.mountain.ok_or("no impassable terrain")?;
            let mut wall = Vec::new();
            for q in -8..=8i64 {
                for r in -6..=6i64 {
                    let ds = from_segment((q, r));
                    let x2 = 2 * q + r;
                    if ds <= 1 || (ds == 4 && r != 0 && x2 <= 7) {
                        wall.push(at(p, (q, r))?);
                    }
                }
            }
            for q in -17..=-4 {
                wall.push(at(p, (q, 1))?);
                wall.push(at(p, mirror(q, 1, -1))?);
            }
            p.paint(&wall, mountain);
            // The far end of both routes in forest (where the stack hides; the same ground on both sides).
            let mut forest = Vec::new();
            for sd in [1, -1] {
                let h = mirror(HIDE_AMBUSH.0, HIDE_AMBUSH.1, sd);
                for q in -1..=1 {
                    for r in -1..=1 {
                        let t = (h.0 + q, h.1 + r);
                        if hd(t, h) <= 1 && (2..=3).contains(&from_segment(t)) {
                            forest.push(at(p, t)?);
                        }
                    }
                }
            }
            p.cover(&forest);
            let outpost = at(p, (-3, 0))?;
            p.city(me, outpost, 1, false);
            let refuge = at(p, (6, 0))?;
            p.city(me, refuge, 3, false);
            s.refuge = Some(refuge);
            let arms = [Arm::Melee, Arm::Ranged, Arm::Mounted, Arm::Melee];
            for (i, q) in [-12, -13, -14, -15].into_iter().enumerate() {
                let k = p.kind(arms[i]).ok_or("no kind")?;
                let role = if act::range_of(rules, k).is_some() { "ranged" } else { "army" };
                let piece = p.put(me, at(p, (q, 0))?, k, role);
                s.mine.push(piece);
            }
            let strong = lab.kinds.by_arm.get(&Arm::Melee).and_then(|v| v.iter().copied().max_by_key(|k| (rules.units[k.0 as usize].strength, k.0))).ok_or("no melee")?;
            let horse = lab.kinds.by_arm.get(&Arm::Mounted).and_then(|v| v.iter().copied().max_by_key(|k| (rules.units[k.0 as usize].strength, k.0))).unwrap_or(strong);
            // At the mouth of its route, in the outpost's sight (the south's three tiles, mirrored).
            for (i, d) in [(-3, 2), (-3, 3), (-4, 3)].into_iter().enumerate() {
                let k = if i == 2 { horse } else { strong };
                let piece = p.put(foe, at(p, m(d))?, k, "ambush");
                s.theirs.push(piece);
            }
            let seen = seen_foes(rules, &p.game, me, foe).len();
            if seen < 2 {
                return Err(format!("the outpost sees {seen} of the stack"));
            }
            s.info.insert("hide".into(), at(p, m(HIDE_AMBUSH))?.0.to_string());
        }
        Fam::HuntFog => {
            let outpost = at(p, (0, 0))?;
            p.city(me, outpost, 1, false);
            // The hunters come down a corridor (rows r = 1 and its mirror walled, q -17..-3): until they leave it
            // where they stand tells nothing of the worker's way.
            let mountain = lab.grounds.mountain.ok_or("no impassable terrain")?;
            let mut wall = Vec::new();
            for q in -17..=-3 {
                wall.push(at(p, (q, 1))?);
                wall.push(at(p, mirror(q, 1, -1))?);
            }
            p.paint(&wall, mountain);
            let mut forest = Vec::new();
            for sd in [1, -1] {
                let h = mirror(HIDE_PREY.0, HIDE_PREY.1, sd);
                for q in -1..=1 {
                    for r in -1..=1 {
                        if hd((h.0 + q, h.1 + r), h) <= 1 {
                            forest.push(at(p, (h.0 + q, h.1 + r))?);
                        }
                    }
                }
            }
            p.cover(&forest);
            let civ = super::families::civil_kind(rules).ok_or("no worker")?;
            let prey = p.put(foe, at(p, PREY_START)?, civ, "civilian");
            s.theirs.push(prey);
            for t in [at(p, (-13, 0))?, at(p, (-14, 0))?, at(p, (-15, 0))?] {
                let k = p.kind(Arm::Melee).ok_or("no melee")?;
                let piece = p.put(me, t, k, "hunter");
                s.mine.push(piece);
            }
            if seen_foes(rules, &p.game, me, foe).is_empty() {
                return Err("the outpost does not see the worker".into());
            }
            s.info.insert("step".into(), at(p, m(PREY_STEP))?.0.to_string());
            s.info.insert("hide".into(), at(p, m(HIDE_PREY))?.0.to_string());
        }
        _ => return Err(format!("{} is not a memory family", fam.name())),
    }
    Ok(s)
}

/// The enemy's turn: the stack (or the worker) stands on its first turn, then walks to its hiding place and
/// waits there; the stack strikes what comes near.
pub fn enemy_turn(rules: &CompiledRuleset, game: &mut GameState, sc: &mut Scene) {
    let (me, foe) = (sc.enemy, sc.learner);
    let rel = game.view().auth.turn().round.saturating_sub(sc.start_round);
    // The hiding place is the scene's secret (not its objective: the army net's tokens read the objective).
    let hide = sc.info.get("hide").and_then(|t| t.parse().ok()).map_or(sc.centre, TileId);
    let ours = |g: &GameState| -> Vec<TileId> { sc.mine.iter().filter_map(|p| act::alive(g, p, foe)).map(|c| c.tile).collect() };
    let friends: Vec<classic_rules::ids::UnitHandle> = sc.theirs.iter().map(|p| p.h).collect();
    for p in sc.theirs.clone() {
        let Some(c) = act::alive(game, &p, me) else { continue };
        if rel >= 1 {
            // On the way it only walks; at its place it strikes what comes within three tiles on its route (what
            // reaches the refuge is out of its play).
            let key = format!("placed {}", p.id.0);
            if dist(game, c.tile, hide) <= 1 {
                sc.info.insert(key.clone(), rel.to_string());
            }
            let placed = sc.info.contains_key(&key);
            let side: i64 = if sc.info.get("side").is_some_and(|s| s == "south") { 1 } else { -1 };
            let near = ours(game).iter().any(|t| {
                let (q, r) = rel_xy(game, sc.centre, *t);
                dist(game, *t, c.tile) <= 3 && 2 * q + r <= 8 && r * side >= 1
            });
            // The worker's first step is its way (seen from the outpost), then the forest.
            let step = sc.info.get("step").and_then(|t| t.parse().ok()).map(TileId);
            let goal = match step {
                Some(st) if rel == 1 => st,
                _ => hide,
            };
            if !placed {
                act::move_by(game, rules, me, p.h, |g, t, _| (dist(g, t, goal), t.0));
            } else if p.role == "ambush" && near {
                act::unit_turn(game, rules, me, foe, p.h, &Aim::Close, Nerve::Even, &ours, &friends);
            } else if p.role == "ambush" {
                let _ = act::fortify(game, rules, me, p.h);
            }
        }
        act::stand(game, rules, p.h);
    }
}

/// The references: `ref-recall` remembers the foes it saw, `ref-forget` does not.
pub fn reference_turn(rules: &CompiledRuleset, game: &mut GameState, sc: &mut Scene, name: &str) {
    let (me, foe) = (sc.learner, sc.enemy);
    let remember = name == "ref-recall";
    let round = game.view().auth.turn().round;
    let now = seen_foes(rules, game, me, foe);
    if remember {
        for u in &now {
            sc.track.seen.insert(u.id, (u.tile, round));
        }
    }
    // The side the foes went (by the last sightings), else north.
    let ys: Vec<i64> = sc.track.seen.values().map(|(t, _)| rel_xy(game, sc.centre, *t).1).collect();
    let foe_side: i64 = if ys.is_empty() { 0 } else { ys.iter().sum::<i64>().signum() };
    let visible = now.iter().map(|u| u.tile).collect::<Vec<_>>();
    let refuge = sc.refuge.unwrap_or(sc.centre);
    let mut order: Vec<Piece> = sc.mine.clone();
    order.sort_by_key(|p| p.id.0);
    for p in order {
        let Some(c) = act::alive(game, &p, me) else { continue };
        let (x, y) = rel_xy(game, sc.centre, c.tile);
        let goal = match sc.fam {
            Fam::AmbushFog => {
                // The route away from the stack (north when nothing is known); the waypoints keep the straight walk
                // off the block.
                let route = if foe_side == 0 { -1 } else { -foe_side };
                let (w1, w2) = (mirror(ROUTE[0].0, ROUTE[0].1, route), mirror(ROUTE[1].0, ROUTE[1].1, route));
                // By the unit's x (2q + r, the mirror keeps it).
                let x2 = 2 * x + y;
                if x2 < 2 * w1.0 + w1.1 {
                    p_at(game, sc.centre, w1.0, w1.1)
                } else if x2 < 2 * w2.0 + w2.1 {
                    p_at(game, sc.centre, w2.0, w2.1)
                } else {
                    Some(refuge)
                }
            }
            _ => {
                if let Some(t) = visible.first() {
                    Some(*t)
                } else {
                    // The forest on the side the worker was last seen (north when it was not).
                    let s = if foe_side == 0 { -1 } else { foe_side };
                    let h = mirror(HIDE_PREY.0, HIDE_PREY.1, s);
                    p_at(game, sc.centre, h.0, h.1)
                }
            }
        };
        if sc.fam == Fam::HuntFog {
            if let Some(t) = visible.first() {
                let prey = sc.theirs.iter().find(|q| act::alive(game, q, foe).is_some_and(|u| u.tile == *t)).map(|q| q.h);
                if let Some(h) = prey {
                    if act::try_cmd(game, rules, me, Command::AttackUnit { attacker: p.h, defender: h }) {
                        act::stand(game, rules, p.h);
                        continue;
                    }
                    let near: Vec<TileId> = act::reachable(game, rules, me, p.h).into_iter().filter(|(x, left)| *left > 0 && dist(game, *x, *t) == 1 && act::free_for(game, rules, p.h, *x)).map(|(x, _)| x).collect();
                    if let Some(&x) = near.iter().min_by_key(|x| x.0) {
                        if act::try_cmd(game, rules, me, Command::MoveUnit { unit: p.h, destination: x }) && act::try_cmd(game, rules, me, Command::AttackUnit { attacker: p.h, defender: h }) {
                            act::stand(game, rules, p.h);
                            continue;
                        }
                    }
                }
            }
        }
        let goal = goal.unwrap_or(refuge);
        let gy = rel_xy(game, sc.centre, goal).1;
        act::move_by(game, rules, me, p.h, |g, t, _| (dist(g, t, goal), (rel_xy(g, sc.centre, t).1 - gy).abs(), t.0));
        act::stand(game, rules, p.h);
    }
}

fn p_at(game: &GameState, centre: TileId, dx: i64, dy: i64) -> Option<TileId> {
    let dims = game.view().auth.map().dimensions;
    let (w, h) = (i64::from(dims.width), i64::from(dims.height));
    let (x, y) = (i64::from(centre.0) % w + dx, i64::from(centre.0) / w + dy);
    (x >= 0 && y >= 0 && x < w && y < h).then(|| TileId((y * w + x) as u32))
}

/// The verdict at a round's end.
pub fn judge(rules: &CompiledRuleset, game: &GameState, sc: &Scene, rel: u32) -> Option<bool> {
    let _ = rules;
    let limit = rel >= sc.rounds;
    let mine = sc.mine_alive(game);
    let lost = sc.mine.len() - mine.len();
    match sc.fam {
        Fam::AmbushFog => {
            let refuge = sc.refuge.unwrap_or(sc.centre);
            if lost >= 2 {
                Some(false)
            } else if !mine.is_empty() && mine.iter().all(|(_, c)| dist(game, c.tile, refuge) <= 2) {
                Some(true)
            } else if limit {
                Some(false)
            } else {
                None
            }
        }
        _ => {
            let caught = sc.theirs.iter().filter(|q| q.role == "civilian").any(|q| act::alive(game, q, sc.enemy).is_none());
            if caught {
                Some(true)
            } else if limit || mine.is_empty() {
                Some(false)
            } else {
                None
            }
        }
    }
}
