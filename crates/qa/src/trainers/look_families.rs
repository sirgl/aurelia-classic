//! The "where to look" families (Roman, 24 Nov: "for aircraft you must look where they will strike. Or a sudden attack on another
//! city"): the danger arises OUTSIDE the area of the current operation and the right move needs a timely look there. The win rules,
//! the gates and the design decisions are fixed in work/nets/trainers-hist/GATES.md ("G7 (look)") before the first run.
//!
//! Every family has an OPERATION (our army besieges an enemy walled city 5 tiles east, or our ships / fighters are on a task), a
//! THREAT outside it, and two kinds of teacher:
//! - the look teacher (`ref-watch*`) acts on everything the rules' sight of our side shows: the pickets (cities of one citizen) beside
//!   the approaches, the far town's own sight, the enemy bases and their strike radius; in blind-sector it also walks a scout to the
//!   stalest approach (the one no eye of ours has seen);
//! - the tunnel teacher (`ref-tunnel*`) plays the same response on a narrower input: only the foes within `TUNNEL_N` tiles of one of
//!   its units or of the operation's target (the area of its operation).
//!
//! THE SCENES (offsets from the arena's centre; all drawn mirrored north or south; frozen on dev instances 0-39):
//! - surprise-attack: the siege of X (+5, 0) by 8 (5 capturers, 1 ranged, 2 siege); our home city H (-6, 0) at 3 % (no walls: a walled 3 % city
//!   kills the attackers with its own strike and the passive control wins); five strong melee at (-11, 0), in sight of a picket at (-9, 2)
//!   only, march on H from round 2.
//! - blind-sector: the same, three approaches to H (-3, 0) (west, north-west, south-west); pickets stand on two of the gates; the force of five
//!   masses beyond the third (west (-12, 0), diagonals (-8, +-7)). The look teacher sends a capturer to look at the approach no eye has seen.
//! - second-front: the same siege; our far town S (-6, 0) at 40 % with one garrison and a picket (-11, 2); at round K (3-5) eight strong melee
//!   appear at (-12, 0), in the picket's sight, and march on S (fewer cannot take it: its own strike kills them one by one).
//! - bait-look: the same siege; our cities A (-3, -5) and B (-4, 4) (3 %); five fortified melee at (-4, -1), 3 tiles from the army (a decoy that
//!   never advances) and five strong melee marching on B from (-14, 4), seen by a picket (-12, 5).
//! - naval-approach: a sea over columns -6..6, rows +-5; our coastal city H (7, 3) at 3 %; an islet picket (0, 1); two fortified foot units of
//!   the enemy on the north shore (0, -6) that our 3-4 ships at (0, -4) bombard (the operation); seven embarked strong units start at (-1, 3)
//!   and sail along row 3, land beside H (within 2 tiles) and strike it (fewer die to the city's own strike).
//! - air-threat (modern, atomic): our forward city D (0, 0) with 1-2 fighters, the anti-air unit and three foot units; our city C (-10, 0) with
//!   a garrison; the enemy's air base (-13, 0), 3 tiles from C, with three bombers that hold until round 4 and then strike C every round.
//!   (The scene rule leaves a piece on interception duty alone, as the shots of a fighter would kill it.)
//!
//! WIN (ground): every city to keep is still ours and at least 4 of our 8 (9) are alive when the real force is dead or the rounds end.

use std::collections::BTreeMap;

use classic_rules::ids::{PlayerId, TileId, UnitKindId};
use classic_rules::{ClassicGame, Command, CompiledRuleset, GameState};
use kernel::world::unit::UnitCore;

use super::act::{self, Aim, Nerve};
use super::assault;
use super::families::{blank, kinds_label, Fam, Painter};
use super::history_families::{self as hist, defend, get_tiles, owner_of_city, put_tiles, rel_round, strongest_melee, weaken};
use super::lab::{dist, tiles_within, Arm, EraKinds, Lab};
use super::memory;
use super::{Piece, Scene};

/// The area of an operation: foes within this many tiles of one of its units or of its target.
pub const TUNNEL_N: i32 = 4;

pub fn is_look(f: Fam) -> bool {
    matches!(f, Fam::AirThreat | Fam::SurpriseAttack | Fam::NavalApproach | Fam::BlindSector | Fam::SecondFront | Fam::BaitLook)
}

/// The families whose tokens hold only the foes our side sees.
pub fn is_fog(f: Fam) -> bool {
    matches!(f, Fam::SurpriseAttack | Fam::NavalApproach | Fam::BlindSector | Fam::SecondFront | Fam::BaitLook)
}

pub fn rounds(f: Fam) -> u32 {
    match f {
        Fam::AirThreat => 8,
        Fam::NavalApproach => 9,
        Fam::SecondFront => 12,
        _ => 10,
    }
}

pub fn solvers(f: Fam) -> &'static [&'static str] {
    teachers(f).get(..1).unwrap_or(&[])
}

pub fn teachers(f: Fam) -> &'static [&'static str] {
    match f {
        Fam::AirThreat => &["ref-watch-air", "ref-tunnel-air"],
        Fam::SurpriseAttack => &["ref-watch", "ref-tunnel"],
        Fam::NavalApproach => &["ref-watch-sea", "ref-tunnel-sea"],
        Fam::BlindSector => &["ref-watch-sector", "ref-tunnel-sector"],
        Fam::SecondFront => &["ref-watch-far", "ref-tunnel-far"],
        _ => &["ref-watch-bait", "ref-tunnel-bait"],
    }
}

pub fn task(f: Fam) -> &'static str {
    match f {
        Fam::AirThreat => "enemy bombers from a base within range of our city while our fighters cover the army: look at the base's strike radius",
        Fam::SurpriseAttack => "a siege goes on; an enemy force masses near another city of ours, seen only by a picket: look there and send the holders",
        Fam::NavalApproach => "an enemy fleet with embarked units crosses toward our coast while our ships are on a task: look at the crossing",
        Fam::BlindSector => "an approach no eye of ours has seen hides the strike: send a scout to look at the stalest one",
        Fam::SecondFront => "a far town of ours is attacked at round K while the army besieges: look at the far front in time",
        _ => "a big enemy army is shown in one place, the real strike marches elsewhere: look at what moves, not at what is big",
    }
}

fn embark_known(rules: &CompiledRuleset, era: u8) -> bool {
    rules.technologies.iter().any(|t| t.allows_embarking && t.content_era.and_then(|e| rules.eras.get(e.0 as usize)).is_some_and(|e| e.index <= era))
}

pub fn claims(f: Fam, rules: &CompiledRuleset, k: &EraKinds) -> bool {
    let land = k.has(Arm::Melee) && k.has(Arm::Ranged);
    match f {
        Fam::NavalApproach => (k.has(Arm::NavalMelee) || k.has(Arm::NavalRanged)) && embark_known(rules, k.era) && land,
        Fam::AirThreat => k.era <= 6 && k.has(Arm::Fighter) && k.has(Arm::Bomber) && k.has(Arm::Melee) && air::aa_kind(rules, k.era).is_some(),
        _ => land && k.has(Arm::Siege),
    }
}

// ---------------------------------------------------------------------------
// Building

fn flipped(sc: &Scene) -> bool {
    sc.info.get("flip").is_some_and(|v| v == "1")
}

/// The tile at offset `d` from the centre (mirrored in rows when the scene is flipped).
fn at(p: &Painter, flip: bool, d: (i64, i64)) -> Result<TileId, String> {
    p.at(d.0, if flip { -d.1 } else { d.1 }).ok_or_else(|| "off the map".to_owned())
}

fn strong_foot(p: &Painter) -> Result<UnitKindId, String> {
    let k = strongest_melee(p)?;
    if act::moves_of(p.rules, k) != 2 {
        return Err(format!("{} has movement {}", p.rules.units[k.0 as usize].name, act::moves_of(p.rules, k)));
    }
    Ok(k)
}

/// A group of `n` units of `kind` on the free tiles round `anchor`.
fn put_force(p: &mut Painter, side: PlayerId, anchor: TileId, n: usize, kind: UnitKindId, role: &'static str, into: &mut Vec<Piece>) -> Result<(), String> {
    for t in p.spots(anchor, 1, n)? {
        into.push(p.put(side, t, kind, role));
    }
    Ok(())
}

/// The operation shared by the ground families: the siege of X by 8 (5 capturers, 1 ranged, 2 siege).
fn ground_op(p: &mut Painter, s: &mut Scene, flip: bool, scout: bool) -> Result<TileId, String> {
    let (me, foe) = (s.learner, s.enemy);
    let x_t = at(p, flip, (5, 0))?;
    let pop = p.d.range(5, 8) as u32;
    let city = p.city(foe, x_t, pop, true);
    weaken(p, city, 45);
    let gk = p.kind(if p.d.chance(50) { Arm::Ranged } else { Arm::Melee }).ok_or("no garrison")?;
    s.theirs.push(p.put(foe, x_t, gk, "garrison"));
    let mut plan: Vec<(Arm, &'static str)> = vec![(Arm::Melee, "capturer"); 5];
    plan.push((Arm::Ranged, "ranged"));
    plan.extend(vec![(Arm::Siege, "siege"); 2]);
    if scout {
        plan.push((Arm::Melee, "capturer"));
    }
    let anchor = at(p, flip, (0, 0))?;
    for ((arm, role), t) in plan.iter().zip(p.spots(anchor, 2, plan.len())?) {
        let k = p.kind(*arm).ok_or("no kind")?;
        s.mine.push(p.put(me, t, k, role));
    }
    s.target = Some(x_t);
    s.aim_learner = Some(x_t);
    s.centre = anchor;
    Ok(x_t)
}

/// A walled home city of ours worn to `left` percent.
fn home_city(p: &mut Painter, flip: bool, d: (i64, i64), pop: u32, left: i64) -> Result<TileId, String> {
    let t = at(p, flip, d)?;
    let me = p.lab.learner;
    let c = p.city(me, t, pop, false);
    weaken(p, c, left);
    Ok(t)
}

/// A picket: a city of one citizen (its sight is what the look teacher uses).
fn picket(p: &mut Painter, flip: bool, d: (i64, i64)) -> Result<TileId, String> {
    let t = at(p, flip, d)?;
    let me = p.lab.learner;
    p.city(me, t, 1, false);
    Ok(t)
}

fn sees(p: &Painter, t: TileId) -> bool {
    p.game.player_view(p.rules, p.lab.learner).is_visible(t)
}

pub(super) fn build(p: &mut Painter, fam: Fam, lab: &Lab, instance: usize, attempt: u32) -> Result<Scene, String> {
    let mut s = match fam {
        Fam::AirThreat => air::build(p, lab, instance, attempt)?,
        Fam::NavalApproach => sea::build(p, lab, instance, attempt)?,
        _ => build_ground(p, fam, lab, instance, attempt)?,
    };
    s.info.insert("mine".into(), kinds_label(p.rules, &s.mine));
    s.info.insert("theirs".into(), kinds_label(p.rules, &s.theirs));
    s.rounds = rounds(fam);
    Ok(s)
}

fn build_ground(p: &mut Painter, fam: Fam, lab: &Lab, instance: usize, attempt: u32) -> Result<Scene, String> {
    let mut s = blank(fam, lab, instance, attempt);
    let foe = lab.enemy;
    let flip = p.d.chance(50);
    s.info.insert("flip".into(), u8::from(flip).to_string());
    let x_t = ground_op(p, &mut s, flip, fam == Fam::BlindSector)?;
    let strong = strong_foot(p)?;
    let n_force = 5usize;
    let army_dist = |p: &Painter, t: TileId, s: &Scene| -> i32 { s.mine.iter().filter_map(|q| act::core_of(&p.game, q.h)).map(|c| dist(&p.game, c.tile, t)).min().unwrap_or(99) };
    match fam {
        Fam::SurpriseAttack => {
            let h = home_city(p, flip, (-6, 0), 2, 3)?;
            picket(p, flip, (-9, 2))?;
            let f = at(p, flip, (-11, 0))?;
            put_force(p, foe, f, n_force, strong, "force", &mut s.theirs)?;
            put_tiles(&mut s.info, "keep", &[h]);
            s.info.insert("goal".into(), h.0.to_string());
            s.info.insert("go".into(), 2.to_string());
            s.refuge = Some(h);
        }
        Fam::BlindSector => {
            // Three approaches to H, 5 tiles out (beyond the sight of H): west, north-west, south-west; pickets two tiles inside two of the
            // gates, the force 4-5 tiles beyond the third.
            let h = home_city(p, flip, (-3, 0), 2, 3)?;
            let gates = [(-8, 0), (-6, -4), (-6, 4)];
            let insides = gates;
            let (bx, by) = (-8, 7);
            let beyond = [(-12, 0), (bx, -by), (bx, by)];
            let blind = p.d.range(0, 2) as usize;
            let mut gt = Vec::new();
            for (i, g) in gates.iter().enumerate() {
                gt.push(at(p, flip, *g)?);
                if i != blind {
                    picket(p, flip, insides[i])?;
                }
            }
            let f = at(p, flip, beyond[blind])?;
            put_force(p, foe, f, n_force, strong, "force", &mut s.theirs)?;
            for g in &gt {
                if s.theirs.iter().any(|q| act::core_of(&p.game, q.h).is_some_and(|c| dist(&p.game, c.tile, *g) < 2)) {
                    return Err("force too near a gate".into());
                }
            }
            put_tiles(&mut s.info, "keep", &[h]);
            put_tiles(&mut s.info, "gates", &gt);
            s.info.insert("blind".into(), blind.to_string());
            s.info.insert("goal".into(), h.0.to_string());
            s.info.insert("go".into(), 2.to_string());
            s.refuge = Some(h);
        }
        Fam::SecondFront => {
            let sft = home_city(p, flip, (-6, 0), 2, 40)?;
            let gk = p.kind(if p.d.chance(50) { Arm::Ranged } else { Arm::Melee }).ok_or("no garrison")?;
            let g = p.put(lab.learner, sft, gk, "garrison");
            s.mine.push(g);
            picket(p, flip, (-11, 2))?;
            let k = p.d.range(3, 5);
            s.info.insert("k".into(), k.to_string());
            s.info.insert("strong".into(), strong.0.to_string());
            s.info.insert("nforce".into(), 8.to_string());
            put_tiles(&mut s.info, "keep", &[sft]);
            s.info.insert("goal".into(), sft.0.to_string());
            s.refuge = Some(sft);
        }
        _ => {
            // bait-look
            let a = home_city(p, flip, (-3, -5), 3, 100)?;
            let b = home_city(p, flip, (-4, 4), 2, 3)?;
            picket(p, flip, (-7, -1))?;
            let decoy_at = at(p, flip, (-4, -1))?;
            let dk = strongest_melee(p)?;
            put_force(p, foe, decoy_at, 5, dk, "decoy", &mut s.theirs)?;
            for q in s.theirs.clone().iter().filter(|q| q.role == "decoy") {
                let _ = act::fortify(&mut p.game, p.rules, foe, q.h);
            }
            picket(p, flip, (-12, 5))?;
            let f = at(p, flip, (-14, 4))?;
            put_force(p, foe, f, 5 as usize, strong, "force", &mut s.theirs)?;
            put_tiles(&mut s.info, "keep", &[a, b]);
            s.info.insert("goal".into(), b.0.to_string());
            s.info.insert("go".into(), "0".into());
            s.refuge = Some(b);
        }
    }
    // The checks the scene stands on: the threat is outside the area of the operation; the eyes show it (not for second-front's later force).
    let forces: Vec<TileId> = s.theirs.iter().filter(|q| q.role == "force").filter_map(|q| act::core_of(&p.game, q.h).map(|c| c.tile)).collect();
    for t in &forces {
        if army_dist(p, *t, &s) <= TUNNEL_N || dist(&p.game, *t, x_t) <= TUNNEL_N {
            return Err("the force stands in the area of the operation".into());
        }
    }
    match fam {
        Fam::SurpriseAttack | Fam::BaitLook => {
            if !forces.iter().any(|t| sees(p, *t)) {
                return Err(format!("no eye sees the force at {:?}", forces.iter().map(|t| p.xy(*t)).collect::<Vec<_>>()));
            }
        }
        Fam::BlindSector => {
            if forces.iter().any(|t| sees(p, *t)) {
                return Err("an eye sees the blind sector's force".into());
            }
            let blind: usize = s.info.get("blind").and_then(|v| v.parse().ok()).unwrap_or(0);
            let gt = get_tiles(&s.info, "gates");
            for (i, g) in gt.iter().enumerate() {
                if sees(p, *g) == (i == blind) {
                    return Err(format!("gate {i} visibility is wrong (blind {blind}, seen {})", sees(p, *g)));
                }
            }
        }
        _ => {}
    }
    if fam == Fam::BaitLook {
        let decoys: Vec<TileId> = s.theirs.iter().filter(|q| q.role == "decoy").filter_map(|q| act::core_of(&p.game, q.h).map(|c| c.tile)).collect();
        if !decoys.iter().any(|t| army_dist(p, *t, &s) <= TUNNEL_N) || !decoys.iter().any(|t| sees(p, *t)) {
            let d: Vec<i32> = decoys.iter().map(|t| army_dist(p, *t, &s)).collect();
            return Err(format!("the decoy is not shown inside the area (army dist {d:?}, seen {})", decoys.iter().any(|t| sees(p, *t))));
        }
    }
    Ok(s)
}

// ---------------------------------------------------------------------------
// The enemy

fn force_alive(game: &GameState, sc: &Scene) -> Vec<(Piece, UnitCore)> {
    sc.theirs_alive(game).into_iter().filter(|(p, _)| p.role == "force").collect()
}

/// The signals every measure reads: the round a force unit first stood in sight of our side, and the first round one stood beside what we keep.
fn note_signals(rules: &CompiledRuleset, game: &GameState, sc: &mut Scene, rel: u32) {
    let keep = get_tiles(&sc.info, "keep");
    let live = force_alive(game, sc);
    if !sc.info.contains_key("first_seen") {
        let view = game.player_view(rules, sc.learner);
        if live.iter().any(|(_, c)| view.sees_unit(c)) {
            sc.info.insert("first_seen".into(), rel.to_string());
        }
    }
    if !sc.info.contains_key("strike") && live.iter().any(|(_, c)| keep.iter().any(|k| dist(game, *k, c.tile) <= 1)) {
        sc.info.insert("strike".into(), rel.to_string());
    }
}

pub fn enemy_turn(rules: &CompiledRuleset, game: &mut GameState, sc: &mut Scene) {
    match sc.fam {
        Fam::AirThreat => air::enemy(rules, game, sc),
        Fam::NavalApproach => sea::enemy(rules, game, sc),
        _ => ground_enemy(rules, game, sc),
    }
}

fn ground_enemy(rules: &CompiledRuleset, game: &mut GameState, sc: &mut Scene) {
    let (me, foe) = (sc.enemy, sc.learner);
    let rel = rel_round(game, sc);
    // Second-front: the force appears in the enemy's turn of round K - 1, 3 tiles beyond the far town.
    if sc.fam == Fam::SecondFront && !sc.info.contains_key("spawned") {
        let k: u32 = sc.info.get("k").and_then(|v| v.parse().ok()).unwrap_or(3);
        if rel + 1 >= k {
            sc.info.insert("spawned".into(), rel.to_string());
            let strong = UnitKindId(sc.info.get("strong").and_then(|v| v.parse().ok()).unwrap_or(0));
            let town = get_tiles(&sc.info, "keep")[0];
                        let nf: usize = sc.info.get("nforce").and_then(|v| v.parse().ok()).unwrap_or(3);
            let spawn = hist::tile_rel(game, sc.centre, (-12, 0)).unwrap_or(town);
            let mut tiles: Vec<TileId> = tiles_within(game, spawn, 2).into_iter().filter(|t| crate::battle::standable(rules, game, *t) && game.view().auth.indexes().units_on(*t).is_empty() && game.view().auth.indexes().city_on_tile(*t).is_none()).collect();
            tiles.sort_by_key(|t| (dist(game, *t, spawn), t.0));
            for t in tiles.into_iter().take(nf) {
                let h = game.debug_spawn_unit(rules, me, t, strong);
                let id = game.view().auth.units().get(h).expect("a spawned unit").core.id;
                sc.theirs.push(Piece { h, id, kind: strong, role: "force" });
            }
            note_signals(rules, game, sc, rel);
            return;
        }
    }
    note_signals(rules, game, sc, rel);
    let goal: TileId = sc.info.get("goal").and_then(|v| v.parse().ok()).map(TileId).unwrap_or(sc.centre);
    let go: u32 = sc.info.get("go").and_then(|v| v.parse().ok()).unwrap_or(0);
    let sc2 = sc.clone();
    let friends: Vec<classic_rules::ids::UnitHandle> = sc.theirs.iter().map(|p| p.h).collect();
    // The march ignores our field units: it fights only what stands at the city it is going to.
    let ours = move |g: &GameState| -> Vec<TileId> {
        let mut v: Vec<TileId> = sc2.mine_alive(g).iter().map(|(_, c)| c.tile).filter(|t| dist(g, *t, goal) <= 1).collect();
        if owner_of_city(g, goal) == Some(sc2.learner) {
            v.push(goal);
        }
        v.sort_by_key(|t| t.0);
        v.dedup();
        v
    };
    for p in sc.theirs.clone() {
        if act::alive(game, &p, me).is_none() {
            continue;
        }
        if p.role == "force" && (rel >= go || sc.fam == Fam::SecondFront) {
            act::unit_turn(game, rules, me, foe, p.h, &Aim::To(goal), Nerve::Reckless, &ours, &friends);
        } else if p.role == "force" || p.role == "decoy" {
            let _ = act::fortify(game, rules, me, p.h);
            act::stand(game, rules, p.h);
        } else {
            act::stand(game, rules, p.h);
        }
    }
}

// ---------------------------------------------------------------------------
// The teachers

/// The foes our side's eyes show (never the garrison of the operation's target).
fn seen_threats(rules: &CompiledRuleset, game: &GameState, sc: &Scene) -> Vec<UnitCore> {
    let x = sc.target;
    let stack: Vec<_> = sc.theirs.iter().filter(|p| p.role == "stack").map(|p| p.id).collect();
    memory::seen_foes(rules, game, sc.learner, sc.enemy).into_iter().filter(|c| !act::is_civilian(rules, c.kind) && Some(c.tile) != x && !stack.contains(&c.id)).collect()
}

/// The holders of a city against `n` foes: the capturers beyond the first two, then the ranged, then the two first capturers (never the siege).
fn holders_of(sc: &Scene, n: usize, skip: Option<&Piece>) -> Vec<Piece> {
    let by = |role: &str| -> Vec<Piece> {
        let mut v: Vec<Piece> = sc.mine.iter().filter(|p| p.role == role && skip.is_none_or(|s| s.id != p.id)).cloned().collect();
        v.sort_by_key(|p| p.id.0);
        v
    };
    let caps = by("capturer");
    let rng = by("ranged");
    let mut out: Vec<Piece> = caps.iter().skip(2).cloned().collect();
    out.extend(rng);
    out.extend(caps.iter().take(2).cloned());
    out.truncate(n);
    out
}

fn ground_teacher(rules: &CompiledRuleset, game: &mut GameState, sc: &mut Scene, name: &str) {
    let look = name.contains("watch");
    let rel = rel_round(game, sc);
    let me = sc.learner;
    let keep = get_tiles(&sc.info, "keep");
    let seen = seen_threats(rules, game, sc);
    let mine_tiles: Vec<TileId> = sc.mine_alive(game).iter().filter(|(p, _)| p.role != "garrison").map(|(_, c)| c.tile).collect();
    let x = sc.target;
    // The positions of the foes we saw last round (the closing test of bait-look).
    let mut closing: Vec<UnitCore> = Vec::new();
    for f in &seen {
        let key = format!("pos{}", f.id.0);
        if let Some(prev) = sc.info.get(&key).and_then(|v| v.parse::<u32>().ok()).map(TileId) {
            if keep.iter().any(|k| dist(game, *k, f.tile) < dist(game, *k, prev)) {
                closing.push(*f);
            }
        }
        sc.info.insert(key, f.tile.0.to_string());
    }
    let gaze: Vec<UnitCore> = if look {
        seen.clone()
    } else {
        seen.iter().filter(|f| mine_tiles.iter().any(|t| dist(game, *t, f.tile) <= TUNNEL_N) || x.is_some_and(|x| dist(game, x, f.tile) <= TUNNEL_N)).cloned().collect()
    };
    let threat: Vec<UnitCore> = if look && sc.fam == Fam::BaitLook { closing } else { gaze };
    // The scout of blind-sector: the capturer with the highest id; it walks to the stalest approach until a foe shows.
    let scout: Option<Piece> = (look && sc.fam == Fam::BlindSector).then(|| sc.mine.iter().filter(|p| p.role == "capturer").max_by_key(|p| p.id.0).cloned()).flatten();
    if !threat.is_empty() {
        let n = threat.len();
        let prev: usize = sc.info.get("hold_n").and_then(|v| v.parse().ok()).unwrap_or(0);
        sc.info.insert("hold_n".into(), prev.max(n).to_string());
        if !sc.info.contains_key("hold_city") {
            let cx = threat.iter().map(|c| c.tile).collect::<Vec<_>>();
            let city = keep.iter().copied().min_by_key(|k| (cx.iter().map(|t| dist(game, *k, *t)).sum::<i32>(), k.0)).unwrap_or(sc.centre);
            sc.info.insert("hold_city".into(), city.0.to_string());
            sc.info.insert("heeded".into(), rel.to_string());
        }
    }
    let live_force = sc.theirs_alive(game).iter().any(|(p, _)| p.role == "force" || p.role == "decoy");
    let hold_city: Option<TileId> = sc.info.get("hold_city").and_then(|v| v.parse().ok()).map(TileId);
    let hold_n: usize = sc.info.get("hold_n").and_then(|v| v.parse().ok()).unwrap_or(0);
    let scouting = scout.is_some() && hold_city.is_none();
    let mut holders: Vec<Piece> = Vec::new();
    if let (Some(city), true) = (hold_city, live_force) {
        holders = holders_of(sc, hold_n.clamp(2, 6), None);
        defend(rules, game, sc, &holders, city);
    }
    let mut away: Vec<Piece> = holders.clone();
    if scouting {
        let sp = scout.clone().expect("a scout");
        // The stalest approach: the gate that no eye of ours has seen the longest (a gate in sight now is fresh).
        let gates = get_tiles(&sc.info, "gates");
        let view = game.player_view(rules, me);
        for (i, g) in gates.iter().enumerate() {
            if view.is_visible(*g) {
                sc.info.insert(format!("g{i}"), rel.to_string());
            }
        }
        let last = |sc: &Scene, i: usize| -> i64 { sc.info.get(&format!("g{i}")).and_then(|v| v.parse::<i64>().ok()).unwrap_or(-1) };
        if let Some(i) = (0..gates.len()).min_by_key(|i| (last(sc, *i), *i)) {
            let goal = gates[i];
            let friends: Vec<classic_rules::ids::UnitHandle> = sc.mine.iter().map(|p| p.h).collect();
            let none = |_: &GameState| Vec::<TileId>::new();
            if act::alive(game, &sp, me).is_some() {
                act::unit_turn(game, rules, me, sc.enemy, sp.h, &Aim::To(goal), Nerve::Careful, &none, &friends);
            }
        }
        away.push(sp);
    }
    let diverted = away.len();
    let best: usize = sc.info.get("diverted").and_then(|v| v.parse().ok()).unwrap_or(0);
    sc.info.insert("diverted".into(), best.max(diverted).to_string());
    // The rest go on with the siege.
    let ids: std::collections::BTreeSet<_> = away.iter().map(|p| p.id).collect();
    let mut sub = sc.clone();
    sub.mine.retain(|p| !ids.contains(&p.id) && p.role != "garrison");
    if !sub.mine.is_empty() {
        let f = assault::flags("ref-planned").expect("a flag set");
        assault::turn(rules, game, &mut sub, f);
        sc.track = sub.track;
    }
    // The far town's garrison fortifies where it stands.
    for p in sc.mine.clone() {
        if p.role == "garrison" && act::alive(game, &p, me).is_some() {
            let _ = act::fortify(game, rules, me, p.h);
            act::stand(game, rules, p.h);
        }
    }
}

/// The learner's reference `name` of a look family. False: not this family's.
pub fn reference_turn(rules: &CompiledRuleset, game: &mut GameState, sc: &mut Scene, name: &str) -> bool {
    if !teachers(sc.fam).contains(&name) {
        return false;
    }
    match sc.fam {
        Fam::AirThreat => air::teacher(rules, game, sc, name),
        Fam::NavalApproach => sea::teacher(rules, game, sc, name),
        _ => ground_teacher(rules, game, sc, name),
    }
    true
}

// ---------------------------------------------------------------------------
// Judging and measures

fn lost_of(game: &GameState, sc: &Scene) -> usize {
    sc.mine.len() - sc.mine_alive(game).len()
}

pub fn judge(rules: &CompiledRuleset, game: &GameState, sc: &Scene, rel: u32) -> Option<bool> {
    match sc.fam {
        Fam::AirThreat => return air::judge(rules, game, sc, rel),
        Fam::NavalApproach => return sea::judge(rules, game, sc, rel),
        _ => {}
    }
    let keep = get_tiles(&sc.info, "keep");
    if keep.iter().any(|k| owner_of_city(game, *k) != Some(sc.learner)) {
        return Some(false);
    }
    let alive = sc.mine_alive(game).len();
    let need = 4;
    if alive < need {
        return Some(false);
    }
    let spawned = sc.fam != Fam::SecondFront || sc.info.contains_key("spawned");
    let force_dead = sc.theirs_alive(game).iter().all(|(p, _)| p.role != "force");
    if (force_dead && spawned) || rel >= sc.rounds {
        return Some(true);
    }
    None
}

/// The numbers beside the win: rounds of warning, of heeding, the cost of looking.
pub fn measures(sc: &Scene, rel: u32, m: &mut BTreeMap<String, f64>) {
    let num = |k: &str| sc.info.get(k).and_then(|v| v.parse::<f64>().ok());
    m.insert("seen".into(), num("first_seen").unwrap_or(-1.0));
    m.insert("strike".into(), num("strike").unwrap_or(-1.0));
    m.insert("heeded".into(), num("heeded").unwrap_or(-1.0));
    if let (Some(s), Some(k)) = (num("first_seen"), num("strike")) {
        m.insert("warning".into(), k - s);
    }
    if let (Some(h), Some(k)) = (num("heeded"), num("strike")) {
        m.insert("lead".into(), k - h);
    }
    m.insert("diverted".into(), num("diverted").unwrap_or(0.0));
    m.insert("look_rounds".into(), f64::from(rel));
}

mod air {
    use super::*;

    /// The era's anti-air unit: the mobile SAM (atomic era on), else the anti-aircraft gun (modern era).
    pub fn aa_kind(rules: &CompiledRuleset, era: u8) -> Option<UnitKindId> {
        let by = |key: &str| super::super::lab::kind_by_key(rules, key).filter(|k| super::super::lab::era_of(rules, *k).is_some_and(|e| e <= era));
        by("core:unit/mobile-sam").or_else(|| by("core:unit/anti-aircraft-gun"))
    }

    /// A foot kind of the era that cannot intercept aircraft (the era's melee draw also holds the gun and the SAM).
    fn ground_kind(p: &mut Painter) -> Option<UnitKindId> {
        for _ in 0..40 {
            let k = p.kind(Arm::Melee)?;
            if p.rules.units[k.0 as usize].combat.air_intercept_range == 0 {
                return Some(k);
            }
        }
        None
    }

    /// The scene (offsets from the centre, drawn mirrored): our forward city D (0, 0) with 1-2 fighters, the anti-air unit and 3 foot units
    /// ("the army", fortified); our city C (-8, 0) with a garrison; the enemy's air base B (-11, 0), 3 tiles from C, with 3-4 bombers that
    /// hold until round `go` and then strike C every round.
    pub fn build(p: &mut Painter, lab: &Lab, instance: usize, attempt: u32) -> Result<Scene, String> {
        let mut s = blank(Fam::AirThreat, lab, instance, attempt);
        let (me, foe) = (lab.learner, lab.enemy);
        let flip = p.d.chance(50);
        s.info.insert("flip".into(), u8::from(flip).to_string());
        let c_t = at(p, flip, (-10, 0))?;
        p.city(me, c_t, 4, false);
        let gk = ground_kind(p).ok_or("no melee")?;
        let g = p.put(me, c_t, gk, "garrison");
        s.mine.push(g);
        let d_t = at(p, flip, (0, 0))?;
        p.city(me, d_t, 3, false);
        for _ in 0..p.d.range(1, 2) {
            let k = p.kind(Arm::Fighter).ok_or("no fighter")?;
            let piece = p.put(me, d_t, k, "fighter");
            s.mine.push(piece);
        }
        let aa = aa_kind(p.rules, lab.era).ok_or("no anti-air unit")?;
        let spots = p.spots(d_t, 1, 4)?;
        let piece = p.put(me, spots[0], aa, "aa");
        s.mine.push(piece);
        for t in &spots[1..4] {
            let k = ground_kind(p).ok_or("no melee")?;
            let piece = p.put(me, *t, k, "army");
            s.mine.push(piece);
        }
        let b_t = at(p, flip, (-13, 0))?;
        p.city(foe, b_t, 4, false);
        for _ in 0..p.d.range(3, 3) {
            let k = p.kind(Arm::Bomber).ok_or("no bomber")?;
            let piece = p.put(foe, b_t, k, "force");
            s.theirs.push(piece);
        }
        put_tiles(&mut s.info, "keep", &[c_t]);
        s.info.insert("go".into(), 4.to_string());
        s.info.insert("fort".into(), d_t.0.to_string());
        s.target = Some(c_t);
        s.hold = Some(c_t);
        s.refuge = Some(c_t);
        s.centre = d_t;
        Ok(s)
    }

    /// The bombers hold in their base until round `go`, then strike our city every round (a bomber below 40 hit points mends).
    pub fn enemy(rules: &CompiledRuleset, game: &mut GameState, sc: &mut Scene) {
        let (me, foe) = (sc.enemy, sc.learner);
        let rel = rel_round(game, sc);
        if !sc.info.contains_key("first_seen") {
            let view = game.player_view(rules, sc.learner);
            if force_alive(game, sc).iter().any(|(_, c)| view.sees_unit(c)) {
                sc.info.insert("first_seen".into(), rel.to_string());
            }
        }
        let go: u32 = sc.info.get("go").and_then(|v| v.parse().ok()).unwrap_or(0);
        let keep = get_tiles(&sc.info, "keep");
        let sc2 = sc.clone();
        for p in &sc2.theirs {
            let Some(c) = act::alive(game, p, me) else { continue };
            if rel >= go && !hp_below(&c, 40) {
                if let Some((_, t)) = act::best_blow(game, rules, me, p.h, &keep, Nerve::Reckless) {
                    if act::strike(game, rules, me, p.h, t) && !sc.info.contains_key("strike") {
                        sc.info.insert("strike".into(), rel.to_string());
                    }
                }
            }
            act::stand(game, rules, p.h);
        }
        let _ = foe;
    }

    fn hp_below(c: &UnitCore, pct: u16) -> bool {
        u32::from(c.hp) * 100 < u32::from(kernel::world::unit::UNIT_MAX_HP) * u32::from(pct)
    }

    pub fn teacher(rules: &CompiledRuleset, game: &mut GameState, sc: &mut Scene, name: &str) {
        let look = name.contains("watch");
        let me = sc.learner;
        let rel = rel_round(game, sc);
        let keep = get_tiles(&sc.info, "keep");
        let c_t = keep[0];
        let seen = seen_threats(rules, game, sc);
        let mine_tiles: Vec<TileId> = sc.mine_alive(game).iter().filter(|(p, _)| p.role != "garrison").map(|(_, c)| c.tile).collect();
        let gaze: Vec<&UnitCore> = if look { seen.iter().collect() } else { seen.iter().filter(|f| mine_tiles.iter().any(|t| dist(game, *t, f.tile) <= TUNNEL_N)).collect() };
        if !gaze.is_empty() && !sc.info.contains_key("heeded") {
            sc.info.insert("heeded".into(), rel.to_string());
        }
        let heeded = sc.info.contains_key("heeded");
        let friends: Vec<classic_rules::ids::UnitHandle> = sc.mine.iter().map(|p| p.h).collect();
        let none = |_: &GameState| Vec::<TileId>::new();
        let mut away = 0usize;
        for p in sc.mine.clone() {
            let Some(c) = act::alive(game, &p, me) else { continue };
            match p.role {
                "fighter" => {
                    if heeded && c.tile != c_t {
                        let city = game.view().auth.indexes().city_on_tile(c_t);
                        if let Some(city) = city {
                            act::try_cmd(game, rules, me, Command::Rebase { unit: p.h, city });
                        }
                        away += 1;
                    } else {
                        let _ = act::try_cmd(game, rules, me, Command::Intercept { unit: p.h });
                        if heeded {
                            away += 1;
                        }
                    }
                }
                "aa" if heeded => {
                    away += 1;
                    act::unit_turn(game, rules, me, sc.enemy, p.h, &Aim::To(c_t), Nerve::Careful, &none, &friends);
                }
                "garrison" | "aa" | "army" => {
                    let _ = act::fortify(game, rules, me, p.h);
                    act::stand(game, rules, p.h);
                }
                _ => act::stand(game, rules, p.h),
            }
        }
        let best: usize = sc.info.get("diverted").and_then(|v| v.parse().ok()).unwrap_or(0);
        sc.info.insert("diverted".into(), best.max(away).to_string());
    }

    fn city_hp_share(rules: &CompiledRuleset, game: &GameState, t: TileId) -> Option<u32> {
        let auth = game.view().auth;
        let c = auth.indexes().city_on_tile(t).and_then(|c| auth.cities().get(c))?;
        let max = i64::from(classic_rules::combat::city::max_hit_points(auth, rules, c));
        Some(((max - i64::from(c.damage)).max(0) * 100 / max.max(1)) as u32)
    }

    /// Every bomber dead within the rounds, our city at 75 % of its hit points or more, at most 1 of the scene's units lost.
    pub fn judge(rules: &CompiledRuleset, game: &GameState, sc: &Scene, rel: u32) -> Option<bool> {
        let bombers = sc.theirs_alive(game).into_iter().filter(|(p, _)| p.role == "force").count();
        let share = sc.target.and_then(|t| city_hp_share(rules, game, t)).unwrap_or(100);
        let lost = sc.mine.len() - sc.mine_alive(game).len();
        if lost >= 2 {
            Some(false)
        } else if bombers == 0 || rel >= sc.rounds {
            Some(bombers == 0 && share >= 75)
        } else {
            None
        }
    }
}

mod sea {
    use super::*;
    use std::collections::{BTreeSet, VecDeque};

    /// A band of shallow sea: columns `x0..=x1` from the centre, `ry` rows each way; islets where the water would be deep.
    fn paint_sea(p: &mut Painter, x0: i64, x1: i64, ry: i64) -> BTreeSet<u32> {
        let sea: Vec<TileId> = (x0..=x1).flat_map(|dx| (-ry..=ry).map(move |dy| (dx, dy))).filter_map(|(dx, dy)| p.at(dx, dy)).collect();
        let coast = p.lab.grounds.coast;
        p.paint(&sea, coast);
        let open = p.lab.grounds.open;
        for &t in &sea {
            let map = p.game.view().auth.map();
            let deep = map.is_water(t) && map.dimensions.neighbors(t).as_slice().iter().all(|n| map.is_water(*n));
            if deep {
                p.paint(&[t], open);
            }
        }
        let map = p.game.view().auth.map();
        let water: Vec<TileId> = sea.iter().copied().filter(|t| map.is_water(*t)).collect();
        let mut seen: BTreeSet<u32> = BTreeSet::new();
        let mut best: BTreeSet<u32> = BTreeSet::new();
        for &t in &water {
            if seen.contains(&t.0) {
                continue;
            }
            let mut comp: BTreeSet<u32> = BTreeSet::new();
            let mut q = VecDeque::from([t]);
            comp.insert(t.0);
            while let Some(x) = q.pop_front() {
                for n in map.dimensions.neighbors(x).as_slice() {
                    if map.is_water(*n) && comp.insert(n.0) {
                        q.push_back(*n);
                    }
                }
            }
            seen.extend(comp.iter().copied());
            if comp.len() > best.len() {
                best = comp;
            }
        }
        best
    }

    fn water_at(p: &mut Painter, body: &BTreeSet<u32>, anchor: TileId, r: i32, n: usize) -> Result<Vec<TileId>, String> {
        let auth = p.game.view().auth;
        let mut v: Vec<(i32, u64, TileId)> = tiles_within(&p.game, anchor, r)
            .into_iter()
            .filter(|t| body.contains(&t.0) && auth.indexes().units_on(*t).is_empty())
            .map(|t| (dist(&p.game, t, anchor), 0, t))
            .collect();
        for x in &mut v {
            x.1 = p.d.next();
        }
        v.sort();
        if v.len() < n {
            return Err(format!("sea room for {} of {n} ships", v.len()));
        }
        Ok(v.into_iter().take(n).map(|x| x.2).collect())
    }

    /// The sailing distance (tiles through water) from the nearest of `goals` (a land goal counts 0: a unit lands there).
    fn sail_dists(game: &GameState, goals: &[TileId]) -> BTreeMap<u32, i32> {
        let map = game.view().auth.map();
        let mut d = BTreeMap::new();
        let mut q = VecDeque::new();
        for g in goals {
            d.insert(g.0, 0);
            q.push_back(*g);
        }
        while let Some(t) = q.pop_front() {
            let dt = d[&t.0];
            for n in map.dimensions.neighbors(t).as_slice() {
                if d.contains_key(&n.0) || !map.is_water(*n) {
                    continue;
                }
                d.insert(n.0, dt + 1);
                q.push_back(*n);
            }
        }
        d
    }

    fn sail_to(rules: &CompiledRuleset, game: &mut GameState, side: PlayerId, unit: classic_rules::ids::UnitHandle, goals: &[TileId]) {
        let d = sail_dists(game, goals);
        act::move_by(game, rules, side, unit, |_, x, _| d.get(&x.0).copied().unwrap_or(999));
        act::stand(game, rules, unit);
    }

    #[allow(clippy::too_many_arguments)]
    fn engage(rules: &CompiledRuleset, game: &mut GameState, side: PlayerId, foe: PlayerId, unit: classic_rules::ids::UnitHandle, targets: &[TileId], pursue: &[TileId], nerve: Nerve) {
        let Some(me) = act::core_of(game, unit) else { return };
        let reach = act::range_of(rules, me.kind).unwrap_or(1);
        let mut struck = false;
        if let Some((_, t)) = act::best_blow(game, rules, side, unit, targets, nerve) {
            struck = act::strike(game, rules, side, unit, t);
        }
        if !struck && act::approach_strike(game, rules, side, foe, unit, targets, nerve) {
            struck = true;
        }
        if !struck {
            let d = sail_dists(game, pursue);
            act::move_by(game, rules, side, unit, |_, x, _| if reach > 1 { (d.get(&x.0).copied().unwrap_or(999) - reach).abs() } else { d.get(&x.0).copied().unwrap_or(999) });
        }
        act::stand(game, rules, unit);
    }

    /// The land tiles 3 tiles from `h` that touch the sea: the crossing lands out of the city's fire and walks the last 3 tiles.
    fn landing_tiles(game: &GameState, h: TileId) -> Vec<TileId> {
        let map = game.view().auth.map();
        let auth = game.view().auth;
        let ring = 2 as i32;
        tiles_within(game, h, ring)
            .into_iter()
            .filter(|t| dist(game, *t, h) <= ring && dist(game, *t, h) >= 1 && !map.is_water(*t) && auth.indexes().city_on_tile(*t).is_none() && map.dimensions.neighbors(*t).as_slice().iter().any(|n| map.is_water(*n)))
            .collect()
    }

    fn ground_kind_sea(p: &mut Painter) -> Result<UnitKindId, String> {
        p.kind(Arm::Melee).ok_or_else(|| "no melee".to_owned())
    }

    /// The scene (offsets from the centre, drawn mirrored): a sea over columns -6..6, rows +-5; our coastal city H (7, 3) at 3 % without walls;
    /// an islet picket (0, 1); the enemy's empty city Y (0, -6) on the north shore, our 3-4 ships at (0, -3) bombarding it (the operation);
    /// three embarked strong units of the enemy at (-5, 3) sail along row 3 for H (they hold until round `go`).
    pub fn build(p: &mut Painter, lab: &Lab, instance: usize, attempt: u32) -> Result<Scene, String> {
        let mut s = blank(Fam::NavalApproach, lab, instance, attempt);
        let (me, foe) = (lab.learner, lab.enemy);
        let flip = p.d.chance(50);
        s.info.insert("flip".into(), u8::from(flip).to_string());
        let body = paint_sea(p, -6, 6, 5);
        let h_t = at(p, flip, (7, 3))?;
        if p.game.view().auth.map().is_water(h_t) {
            return Err("H is water".into());
        }
        let hc = p.city(me, h_t, 2, false);
        weaken(p, hc, 3);
        let islet = at(p, flip, (0, 1))?;
        let open = p.lab.grounds.open;
        p.paint(&[islet], open);
        p.city(me, islet, 1, false);
        // The operation: a stack of two fortified foot units of the enemy on the north shore (a city there would shoot the ships).
        let y_t = at(p, flip, (0, -6))?;
        if p.game.view().auth.map().is_water(y_t) {
            return Err("the shore is water".into());
        }
        let gk = ground_kind_sea(p)?;
        for t in p.spots(y_t, 1, 2)? {
            if p.game.view().auth.map().is_water(t) {
                return Err("shore stack on water".into());
            }
            let piece = p.put(foe, t, gk, "stack");
            let _ = act::fortify(&mut p.game, p.rules, foe, piece.h);
            s.theirs.push(piece);
        }
        s.target = Some(y_t);
        s.aim_learner = Some(y_t);
        let n = p.d.range(3, 4) as usize;
        let mut kinds = Vec::new();
        let off = p.d.range(0, 1) as usize;
        for i in 0..n {
            kinds.push(p.kind(if (i + off) % 2 == 0 { Arm::NavalRanged } else { Arm::NavalMelee }).ok_or("no ship")?);
        }
        let a = at(p, flip, (0, -4))?;
        let tiles = water_at(p, &body, a, 1, n)?;
        for (t, k) in tiles.iter().zip(&kinds) {
            let piece = p.put(me, *t, *k, "ship");
            s.mine.push(piece);
        }
        let strong = strong_foot(p)?;
        let f = at(p, flip, (-1, 3))?;
        let nf = 7 as usize;
        let ft = water_at(p, &body, f, 2, nf)?;
        for t in ft {
            let piece = p.put(foe, t, strong, "force");
            s.theirs.push(piece);
        }
        // The checks: the crossing is outside the area of the operation, and a picket's eye shows it.
        let ships: Vec<TileId> = s.mine.iter().filter_map(|q| act::core_of(&p.game, q.h).map(|c| c.tile)).collect();
        let row3: Vec<TileId> = (-5..=5).filter_map(|x| at(p, flip, (x, 3)).ok()).collect();
        if ships.iter().any(|t| row3.iter().any(|r| dist(&p.game, *t, *r) <= TUNNEL_N)) {
            return Err("the crossing lies in the area of the ships".into());
        }
        if !row3.iter().any(|t| sees(p, *t)) {
            return Err("no eye sees the crossing".into());
        }
        if landing_tiles(&p.game, h_t).is_empty() {
            return Err("no landing tile beside H".into());
        }
        put_tiles(&mut s.info, "keep", &[h_t]);
        s.info.insert("goal".into(), h_t.0.to_string());
        s.info.insert("go".into(), 0.to_string());
        s.refuge = Some(h_t);
        s.centre = a;
        Ok(s)
    }

    /// The embarked units sail along the crossing to the shore beside H, land, and strike H.
    pub fn enemy(rules: &CompiledRuleset, game: &mut GameState, sc: &mut Scene) {
        let (me, foe) = (sc.enemy, sc.learner);
        let rel = rel_round(game, sc);
        note_signals(rules, game, sc, rel);
        let go: u32 = sc.info.get("go").and_then(|v| v.parse().ok()).unwrap_or(0);
        let h = get_tiles(&sc.info, "keep")[0];
        let shore = landing_tiles(game, h);
        let friends: Vec<classic_rules::ids::UnitHandle> = sc.theirs.iter().map(|p| p.h).collect();
        let ours = move |_: &GameState| vec![h];
        for p in sc.theirs.clone() {
            let Some(c) = act::alive(game, &p, me) else { continue };
            if p.role != "force" {
                let _ = act::fortify(game, rules, me, p.h);
                act::stand(game, rules, p.h);
                continue;
            }
            if rel < go {
                act::stand(game, rules, p.h);
                continue;
            }
            let on_water = game.view().auth.map().is_water(c.tile);
            if act::debug() {
                eprintln!("  sea-force {} at {} water {} moves {} shore {:?} reach {:?}", p.id.0, c.tile.0, on_water, c.moves_left, shore.iter().map(|t| t.0).collect::<Vec<_>>(), act::reachable(game, rules, me, p.h).into_iter().map(|(t, _)| t.0).collect::<Vec<_>>());
            }
            if on_water {
                sail_to(rules, game, me, p.h, &shore);
                if let Some(c2) = act::core_of(game, p.h) {
                    if !game.view().auth.map().is_water(c2.tile) {
                        act::unit_turn(game, rules, me, foe, p.h, &Aim::To(h), Nerve::Reckless, &ours, &friends);
                    }
                }
            } else {
                act::unit_turn(game, rules, me, foe, p.h, &Aim::To(h), Nerve::Reckless, &ours, &friends);
            }
        }
    }

    pub fn teacher(rules: &CompiledRuleset, game: &mut GameState, sc: &mut Scene, name: &str) {
        let look = name.contains("watch");
        let me = sc.learner;
        let rel = rel_round(game, sc);
        let seen = seen_threats(rules, game, sc);
        let ship_tiles: Vec<TileId> = sc.mine_alive(game).iter().map(|(_, c)| c.tile).collect();
        let y = sc.target;
        let gaze: Vec<UnitCore> = if look {
            seen.clone()
        } else {
            seen.iter().filter(|f| ship_tiles.iter().any(|t| dist(game, *t, f.tile) <= TUNNEL_N) || y.is_some_and(|y| dist(game, y, f.tile) <= TUNNEL_N)).cloned().collect()
        };
        if !gaze.is_empty() && !sc.info.contains_key("heeded") {
            sc.info.insert("heeded".into(), rel.to_string());
        }
        let heeded = sc.info.contains_key("heeded");
        let h = get_tiles(&sc.info, "keep")[0];
        let tiles: Vec<TileId> = gaze.iter().map(|c| c.tile).collect();
        let mut order: Vec<Piece> = sc.mine.clone();
        order.sort_by_key(|p| (act::range_of(rules, p.kind).is_none(), p.id.0));
        let mut away = 0usize;
        for p in &order {
            if act::alive(game, p, me).is_none() {
                continue;
            }
            if heeded {
                away += 1;
                if tiles.is_empty() {
                    // The foes left the eyes: sail to the sea beside H and look from there.
                    let map = game.view().auth.map();
                    let near: Vec<TileId> = map.dimensions.neighbors(h).as_slice().iter().copied().filter(|t| map.is_water(*t)).collect();
                    sail_to(rules, game, me, p.h, &near);
                } else {
                    engage(rules, game, me, sc.enemy, p.h, &tiles, &tiles, Nerve::Even);
                }
            } else if let Some(y) = y {
                engage(rules, game, me, sc.enemy, p.h, &[y], &[y], Nerve::Even);
            }
        }
        let best: usize = sc.info.get("diverted").and_then(|v| v.parse().ok()).unwrap_or(0);
        sc.info.insert("diverted".into(), best.max(away).to_string());
    }

    /// H kept and at most 1 ship lost when every embarked unit is dead or the rounds end.
    pub fn judge(_rules: &CompiledRuleset, game: &GameState, sc: &Scene, rel: u32) -> Option<bool> {
        let keep = get_tiles(&sc.info, "keep");
        if keep.iter().any(|k| owner_of_city(game, *k) != Some(sc.learner)) || lost_of(game, sc) >= 2 {
            return Some(false);
        }
        let force_dead = sc.theirs_alive(game).iter().all(|(p, _)| p.role != "force");
        if force_dead || rel >= sc.rounds {
            return Some(true);
        }
        None
    }
}

#[allow(dead_code)]
fn unused(_: Command, _: &dyn Fn(&GameState)) {
    let _ = lost_of;
}
