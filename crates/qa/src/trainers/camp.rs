//! The camp trainers (Roman, 24 Nov: "an attack on a barbarian camp, a trainer, and the same with a neutral's units
//! beside it that can take it: teach the army to act so that OUR unit takes the camp").
//!
//! THE RULES AS FOUND (`classic-rules/src/systems/barbarians.rs`; Unciv's `BarbarianManager` and `BarbarianEncampment`):
//! a camp is an improvement on a tile the barbarians own no city on; a camp spawns a defender on its own tile when no
//! barbarian military unit stands ON it (the countdown `spawn_in`, 8 + 0..4 turns, scaled by the speed and the level),
//! else, from round 10 on, a wandering unit beside it (unless more than two barbarians stand within 4 tiles). A camp is
//! CLEARED by whoever ENTERS its tile (any unit of a player that is not the barbarians: `UnitEntered` /
//! `clear_camp_at`; also by a paradrop): the camp is gone, a major gets `CAMP_GOLD` (25, scaled by the speed's gold
//! percent, Songhai triple), the clearing is remembered 30 rounds (`barbarians().cleared()`: tile, player, round).
//! Killing the defenders does not clear it by itself: a MELEE unit that kills the last defender on a tile advances into
//! it (rules §2.3, `combat::advance`, which enters the tile and so clears the camp with the blow); a RANGED kill leaves
//! the tile empty and a unit has to walk onto it. So the capture is a RACE for the tile in time: the keeper (the unit on
//! the camp) dies, and whoever strikes the last blow in melee, or enters the empty tile first (in the same turn, or in a
//! later one), takes it; a camp left empty at the barbarians' turn with its countdown out spawns a new defender.
//!
//! The scene (`families::build` -> `build`): the CAMP LAB (`lab::build_camp_lab`: the lab with the barbarians on and a
//! second nation, the neutral, at peace with the learner and (as always) at war with the barbarians; the barbarian
//! player is the scene's `enemy`, the neutral's units are `Scene::third`). The barbarians' own turn places new camps
//! in the fog every second turn and gives each a defender: those are taken off at once (`clean`), the scene camp
//! lives by the rules (its countdown, its spawns: a unit it spawns joins the scene's barbarian pieces, `adopt`).
//!
//! WIN RULES (fixed before the first run, `GATES.md`; "lost" = our scene units dead or captured):
//! - camp: our unit has entered the camp within 12 rounds, with at most 1 of ours lost. The camp has 2-3 defenders
//!   (a fortified melee unit of the era on the camp tile, 1-2 more beside it: ranged, melee or mounted; the camp tile
//!   is on hills three times in ten); ours are 3-4 units of the era (melee, ranged, melee or mounted, ranged), 5-6
//!   tiles from it. The defenders are a scripted human: the one on the camp never leaves it, the others strike what
//!   comes within three tiles of the camp (at even odds or better) and else keep to the camp.
//! - camp-race: the same, plus the neutral's 3 units of the era (a melee, a ranged and a mounted unit) 4-5 tiles from the
//!   camp on the other side (ours are 5-6: the neutral stands one tile nearer, a "similar distance" that leaves the
//!   careful teacher late), a scripted human: its blows go to the unit ON the camp while one stands there (it knows the
//!   camp is cleared by entering it), and its unit nearest the camp enters it at the first chance of its turn. WIN: OUR
//!   unit enters the camp within 12 rounds with at most 2 of ours lost. LOSS: the neutral's unit enters first, the cap
//!   breaks, or the rounds end. Why 2: the stake is a camp (25 gold, a spawner removed), and a race is only won by a force
//!   that spends units; at most 2 of 3-4 is the most that still leaves a force, and the passive and careful controls lose
//!   it anyway. The neutral's units are at peace with ours: the rules refuse every strike between them; our units may
//!   stand in the neutral's path (a military unit cannot stop on, or pass through, a tile of another player).
//!   (The counts are what the first measurement left: with 2 neutral units, or the neutral 5-6 tiles out, the careful
//!   teacher won the race more than 40 % of the time on the development instances 0-39.)
//!
//! THE REFERENCES: `ref-camp` is the careful teacher: its ranged units go first and shoot the weakest defender they
//! reach, its melee wait three tiles from the defenders until the keeper (the unit on the camp) is at 30 % or below, or
//! 6 rounds have passed, then strike at even odds or better and keep away when wounded below 40 %; the unit that can
//! reach the empty camp (healthy: at least half) enters it. `ref-camp-race` races: every blow goes to the keeper while
//! the units' expected blows add up to its hit points (the capturer, the quickest healthy non-ranged unit, keeps its move
//! to enter and acts last, striking only to kill), else no blow at the keeper (a blow the neutral, who moves after us,
//! would finish: it waits two tiles out, the neutral's blows wear the keeper down and we strike first in the next round),
//! and the capturer enters in the same turn the keeper dies. `ref-camp` on camp-race is the careful control (no race).

use classic_rules::ids::{PlayerId, TileId};
use classic_rules::{ClassicGame, Command, CompiledRuleset, GameState};
use kernel::world::unit::UnitCore;

use super::act::{self, Aim, Nerve};
use super::families::{blank, Fam, Painter};
use super::lab::{dist, tiles_within, Arm, Lab};
use super::{Piece, Scene};

pub fn is_camp(f: Fam) -> bool {
    matches!(f, Fam::Camp | Fam::CampRace)
}

pub fn rounds(_f: Fam) -> u32 {
    12
}

/// The units of ours the win rule lets us lose.
pub fn loss_cap(f: Fam) -> usize {
    match f {
        Fam::Camp => 1,
        _ => 2,
    }
}

pub fn solvers(f: Fam) -> &'static [&'static str] {
    match f {
        Fam::Camp => &["ref-camp"],
        _ => &["ref-camp-race", "ref-camp"],
    }
}

pub fn task(f: Fam) -> &'static str {
    match f {
        Fam::Camp => "clear a barbarian camp (kill its defenders, then walk onto the tile) with at most 1 loss",
        _ => "take a barbarian camp before a neutral's units at peace with us do (at most 2 lost)",
    }
}

/// The scene's numbers, tuned on the development instances 0-39 (the gate in `GATES.md` says how): the camp tile is on
/// hills this often (percent), the neutral's units start this many tiles nearer to the camp than ours do ("a similar
/// distance": the neutral's anchor is 4-5 tiles out, ours 5-6; the units stand on and around the anchor), the neutral has
/// this many units.
const HILLS_PERCENT: u64 = 30;
const NEUTRAL_NEARER: i64 = 1;
const NEUTRAL_UNITS: usize = 3;
/// `ref-camp`'s patience: its melee wait three tiles from the defenders until the keeper is at this share of its hit
/// points (percent) or below, for at most this many rounds.
const PATIENT_KEEPER_PERCENT: i64 = 30;
const PATIENT_ROUNDS: u32 = 6;
/// A unit below this share of its hit points (percent) keeps away from the defenders and strikes only to kill.
const WOUNDED_PERCENT: u16 = 40;

pub fn camp_tile(sc: &Scene) -> TileId {
    sc.hold.unwrap_or(sc.centre)
}

/// Whether the camp improvement still stands.
pub fn standing(game: &GameState, sc: &Scene) -> bool {
    let t = camp_tile(sc);
    game.view().auth.module().barbarians().camps().iter().any(|c| c.tile == t)
}

/// Who cleared the camp (the rules' record).
pub fn cleared_by(game: &GameState, sc: &Scene) -> Option<(PlayerId, u32)> {
    let t = camp_tile(sc);
    game.view().auth.module().barbarians().cleared().iter().find(|c| c.tile == t).map(|c| (c.player, c.round))
}

/// A barbarian military unit stands on the camp tile.
pub fn guarded(game: &GameState, rules: &CompiledRuleset, sc: &Scene) -> bool {
    let auth = game.view().auth;
    auth.indexes().units_on(camp_tile(sc)).iter().filter_map(|h| auth.units().get(*h)).any(|u| u.core.owner == sc.enemy && !act::is_civilian(rules, u.core.kind))
}

/// The neutral's pieces alive: (piece, core).
pub fn third_alive(game: &GameState, sc: &Scene) -> Vec<(Piece, UnitCore)> {
    match sc.neutral {
        Some(n) => sc.third.iter().filter_map(|p| act::alive(game, p, n).map(|c| (p.clone(), c))).collect(),
        None => Vec::new(),
    }
}

/// The barbarian units a camp spawned (the scene's own pieces are known): they join the scene's pieces.
pub fn adopt(game: &GameState, sc: &mut Scene) {
    let auth = game.view().auth;
    let known: std::collections::BTreeSet<_> = sc.theirs.iter().map(|p| p.id).collect();
    let mut fresh: Vec<Piece> = auth
        .indexes()
        .units_of(sc.enemy)
        .iter()
        .filter_map(|h| auth.units().get(*h).map(|u| (*h, *u.core)))
        .filter(|(_, c)| !known.contains(&c.id))
        .map(|(h, c)| Piece { h, id: c.id, kind: c.kind, role: "foe" })
        .collect();
    fresh.sort_by_key(|p| p.id.0);
    sc.theirs.extend(fresh);
}

/// The closed arena of a camp scene: the camps the barbarians' turn placed in the fog, and the defender each of them
/// spawned, are taken off (the scene's own camp lives by the rules).
pub fn clean(rules: &CompiledRuleset, game: &mut GameState, sc: &Scene) {
    let camp = camp_tile(sc);
    let strays: Vec<TileId> = game.view().auth.module().barbarians().camps().iter().map(|c| c.tile).filter(|t| *t != camp).collect();
    for t in strays {
        let on: Vec<classic_rules::ids::UnitHandle> = game.view().auth.indexes().units_on(t).to_vec();
        for h in on {
            if act::core_of(game, h).is_some_and(|c| c.owner == sc.enemy) {
                game.debug_remove_unit(rules, h);
            }
        }
        game.debug_remove_camp(rules, t);
    }
}

/// Called after every seat's script: the first time the camp is seen cleared, what ours had lost by then.
pub fn mark(game: &GameState, sc: &mut Scene) {
    if cleared_by(game, sc).is_some() && !sc.info.contains_key("lost_at_clear") {
        let lost = sc.mine.len() - sc.mine_alive(game).len();
        sc.info.insert("lost_at_clear".into(), lost.to_string());
    }
}

/// Peace between the learner and the neutral holds for the whole play (the neutral's mind may declare a war).
pub fn keep_world(rules: &CompiledRuleset, game: &mut GameState, sc: &mut Scene) {
    let Some(n) = sc.neutral else { return };
    if game.view().auth.diplomacy().at_war(sc.learner, n) {
        game.debug_force_peace(rules, sc.learner, n);
        let k = sc.info.get("peace_forced").and_then(|v| v.parse::<u32>().ok()).unwrap_or(0) + 1;
        sc.info.insert("peace_forced".into(), k.to_string());
    }
}

fn kind_of_arm(p: &mut Painter, arm: Arm) -> Result<classic_rules::ids::UnitKindId, String> {
    p.kind(arm).ok_or_else(|| format!("no {} kind", arm.name()))
}

pub(super) fn build(p: &mut Painter, fam: Fam, lab: &Lab, instance: usize, attempt: u32) -> Result<Scene, String> {
    let rules = p.rules;
    if !super::scene_rule::on() {
        return Err("the camp families need the scene rule (the default)".into());
    }
    let neutral = if fam == Fam::CampRace { Some(lab.neutral.ok_or("no neutral in the lab")?) } else { None };
    let mut s = blank(fam, lab, instance, attempt);
    s.neutral = neutral;
    let (me, foe) = (lab.learner, lab.enemy);
    let at = |p: &Painter, dx: i64, dy: i64| p.at(dx, dy).ok_or_else(|| "off the map".to_owned());
    // Our side is west or east of the camp (drawn), the neutral's the other.
    let sx: i64 = if p.d.chance(50) { 1 } else { -1 };
    let camp = at(p, 0, 0)?;
    let n_def = p.d.range(2, 3) as usize;
    let n_mine = n_def + 1;
    let n_neutral = NEUTRAL_UNITS;
    let d_us = p.d.range(5, 6);
    let hills = p.d.chance(HILLS_PERCENT);
    // Ground: the camp's tile (hills three times in ten) and clutter away from it.
    if hills {
        p.paint(&[camp], lab.grounds.hills);
    }
    p.clutter(p.d.range(4, 8) as usize, &[camp]);
    if !p.standable(camp) {
        return Err("the camp's tile is not standable".into());
    }
    // The defenders: the camp's tile, then beside it.
    let first = kind_of_arm(p, Arm::Melee)?;
    let guard = p.put(foe, camp, first, "foe");
    s.theirs.push(guard.clone());
    let _ = act::fortify(&mut p.game, rules, foe, guard.h);
    let mut ring: Vec<TileId> = tiles_within(&p.game, camp, 1).into_iter().filter(|t| *t != camp && p.free(*t)).collect();
    p.d.shuffle(&mut ring);
    for (i, t) in ring.into_iter().take(n_def - 1).enumerate() {
        let arm = match (i, p.d.chance(60)) {
            (0, true) => Arm::Ranged,
            (0, false) => Arm::Melee,
            (_, true) => Arm::Mounted,
            _ => Arm::Melee,
        };
        let k = kind_of_arm(p, arm)?;
        let piece = p.put(foe, t, k, "foe");
        let _ = act::fortify(&mut p.game, rules, foe, piece.h);
        s.theirs.push(piece);
    }
    if s.theirs.len() < n_def {
        return Err("no room for the defenders".into());
    }
    // Ours.
    let arms = [Arm::Melee, Arm::Ranged, if p.d.chance(50) { Arm::Mounted } else { Arm::Melee }, Arm::Ranged];
    let anchor = at(p, -sx * d_us, p.d.range(-1, 1))?;
    let mut ks = Vec::new();
    for a in arms.iter().take(n_mine) {
        ks.push(kind_of_arm(p, *a)?);
    }
    for (k, t) in ks.iter().zip(p.spots(anchor, 1, n_mine)?) {
        let role = if act::range_of(rules, *k).is_some() { "ranged" } else { "melee" };
        let piece = p.put(me, t, *k, role);
        s.mine.push(piece);
    }
    // The neutral's: the same distance from the other side.
    if let Some(n) = neutral {
        let anchor = at(p, sx * (d_us - NEUTRAL_NEARER), p.d.range(-1, 1))?;
        let arms = [Arm::Melee, Arm::Ranged, Arm::Mounted];
        let mut ks = Vec::new();
        for a in arms.iter().take(n_neutral) {
            ks.push(kind_of_arm(p, *a)?);
        }
        for (k, t) in ks.iter().zip(p.spots(anchor, 1, n_neutral)?) {
            let role = if act::range_of(rules, *k).is_some() { "ranged" } else { "melee" };
            let piece = p.put(n, t, *k, role);
            s.third.push(piece);
        }
    }
    // The camp itself (last: a terrain change clears an improvement).
    p.game.debug_place_camp(rules, camp);
    if p.game.view().auth.module().barbarians().camps().len() != 1 {
        return Err("the camp was not placed".into());
    }
    s.hold = Some(camp);
    s.info.insert("camp".into(), camp.0.to_string());
    s.info.insert("hills".into(), hills.to_string());
    let near = |ps: &[Piece], side: PlayerId, g: &GameState| ps.iter().filter_map(|q| act::alive(g, q, side)).map(|c| dist(g, c.tile, camp)).min().unwrap_or(0);
    s.info.insert("d_us".into(), near(&s.mine, me, &p.game).to_string());
    if let Some(n) = neutral {
        s.info.insert("d_neutral".into(), near(&s.third, n, &p.game).to_string());
    }
    Ok(s)
}

/// The barbarians' turn: adopt what the camp spawned; the unit on the camp holds it; the others strike what comes
/// within three tiles of the camp (at even odds or better), else keep to the camp.
pub fn enemy_turn(rules: &CompiledRuleset, game: &mut GameState, sc: &mut Scene) {
    adopt(game, sc);
    let (me, foe) = (sc.enemy, sc.learner);
    let camp = camp_tile(sc);
    let sc2 = sc.clone();
    let friends: Vec<classic_rules::ids::UnitHandle> = sc2.theirs.iter().map(|p| p.h).collect();
    let targets = move |g: &GameState| -> Vec<TileId> {
        let mut v: Vec<TileId> = sc2.mine_alive(g).iter().map(|(_, c)| c.tile).collect();
        v.extend(third_alive(g, &sc2).iter().map(|(_, c)| c.tile));
        v.sort_by_key(|t| t.0);
        v.dedup();
        v
    };
    for p in sc.theirs.clone() {
        let Some(c) = act::alive(game, &p, me) else { continue };
        let ranged = act::range_of(rules, c.kind).is_some();
        let near = targets(game).iter().any(|t| dist(game, *t, camp) <= 3);
        let aim = if c.tile == camp {
            Aim::Hold(camp)
        } else if near && !ranged {
            Aim::Close
        } else if near {
            Aim::Support
        } else {
            Aim::To(camp)
        };
        act::unit_turn(game, rules, me, foe, p.h, &aim, Nerve::Even, &targets, &friends);
    }
}

/// The units of `pieces` (side `side`) that can reach the empty camp this turn, nearest first.
fn can_enter(game: &GameState, rules: &CompiledRuleset, side: PlayerId, pieces: &[Piece], camp: TileId) -> Vec<Piece> {
    let mut v: Vec<(i32, u64, Piece)> = pieces
        .iter()
        .filter_map(|p| act::alive(game, p, side).map(|c| (p, c)))
        .filter(|(p, c)| c.moves_left > 0 && act::reachable(game, rules, side, p.h).iter().any(|(t, left)| *t == camp && *left <= c.moves_left) && act::free_for(game, rules, p.h, camp))
        .map(|(p, c)| (dist(game, c.tile, camp), p.id.0, p.clone()))
        .collect();
    v.sort_by_key(|x| (x.0, x.1));
    v.into_iter().map(|x| x.2).collect()
}

/// A unit of `pieces` enters the camp when it is empty and one reaches it; true when one did.
fn enter(game: &mut GameState, rules: &CompiledRuleset, side: PlayerId, pieces: &[Piece], sc: &Scene, healthy: bool) -> bool {
    let camp = camp_tile(sc);
    if !standing(game, sc) || guarded(game, rules, sc) {
        return false;
    }
    for p in can_enter(game, rules, side, pieces, camp) {
        let hp = act::core_of(game, p.h).map_or(0, |c| c.hp);
        if healthy && hp < kernel::world::unit::UNIT_MAX_HP / 2 {
            continue;
        }
        if act::try_cmd(game, rules, side, Command::MoveUnit { unit: p.h, destination: camp }) {
            act::stand(game, rules, p.h);
            return true;
        }
    }
    false
}

/// The neutral's turn (a scripted human): its nearest unit enters the cleared camp at the first chance; else its
/// ranged units shoot and its melee strike the defenders at even odds or better, closing in.
pub fn neutral_turn(rules: &CompiledRuleset, game: &mut GameState, sc: &mut Scene) {
    let Some(n) = sc.neutral else { return };
    let foe = sc.enemy;
    let friends: Vec<classic_rules::ids::UnitHandle> = sc.third.iter().map(|p| p.h).collect();
    // It knows the trick: the camp is cleared by entering it, so its blows go to the unit on the camp while one stands there.
    let camp = camp_tile(sc);
    let targets = move |g: &GameState| -> Vec<TileId> {
        let all = act::foe_tiles(g, foe, None, false);
        if all.contains(&camp) {
            vec![camp]
        } else {
            all
        }
    };
    let mut order: Vec<Piece> = sc.third.clone();
    order.sort_by_key(|p| (u8::from(act::range_of(rules, p.kind).is_none()), p.id.0));
    let pieces = sc.third.clone();
    for p in order {
        if enter(game, rules, n, &pieces, sc, false) {
            continue;
        }
        let Some(c) = act::alive(game, &p, n) else { continue };
        let ranged = act::range_of(rules, c.kind).is_some();
        act::unit_turn(game, rules, n, foe, p.h, if ranged { &Aim::Support } else { &Aim::Close }, Nerve::Even, &targets, &friends);
    }
    let _ = enter(game, rules, n, &pieces, sc, false);
}

/// The scene rule's part of the neutral's turn (promotions, shots, stand and fortify), as `scene_rule::turn` for a seat.
pub fn neutral_rule(rules: &CompiledRuleset, game: &mut GameState, sc: &Scene) {
    let Some(n) = sc.neutral else { return };
    let foes: Vec<TileId> = act::foe_tiles(game, sc.enemy, None, false);
    for p in &sc.third {
        let Some(c) = act::alive(game, p, n) else { continue };
        if let Some(promotion) = super::scene_rule::promotion_pick(game, rules, &c) {
            act::try_cmd(game, rules, n, Command::Promote { unit: p.h, promotion });
        }
    }
    for p in &sc.third {
        if act::alive(game, p, n).is_none() {
            continue;
        }
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
}

/// The references: `ref-camp` (careful: no race) and `ref-camp-race`.
pub fn reference_turn(rules: &CompiledRuleset, game: &mut GameState, sc: &mut Scene, name: &str) {
    let race = name == "ref-camp-race";
    let (me, foe) = (sc.learner, sc.enemy);
    let camp = camp_tile(sc);
    adopt(game, sc);
    let friends: Vec<classic_rules::ids::UnitHandle> = sc.mine.iter().map(|p| p.h).collect();
    let mine = sc.mine.clone();
    // What the units strike: ref-camp the barbarians' units (the weakest first, by the blows' previews); ref-camp-race
    // the unit ON the camp while one stands there and the unit can reach it (the camp is cleared by entering it: the
    // others do not matter), else what it can kill or trade with at even odds.
    let all_foes = move |g: &GameState| act::foe_tiles(g, foe, None, false);
    // The capturer of the race: the quickest healthy non-ranged unit (its moves, then the lowest id); it acts last and strikes only to kill.
    let capturer: Option<Piece> = if race {
        mine.iter()
            .filter_map(|p| act::alive(game, p, me).map(|c| (p, c)))
            .filter(|(p, c)| act::range_of(rules, p.kind).is_none() && c.hp >= kernel::world::unit::UNIT_MAX_HP * 6 / 10)
            .max_by_key(|(p, _)| (act::moves_of(rules, p.kind), std::cmp::Reverse(p.id.0)))
            .map(|(p, _)| p.clone())
    } else {
        None
    };
    let mut order: Vec<Piece> = mine.clone();
    order.sort_by_key(|p| (u8::from(capturer.as_ref().is_some_and(|c| c.id == p.id)), u8::from(act::range_of(rules, p.kind).is_none()), p.id.0));
    let home = sc.centre;
    let hurt = sc.theirs_alive(game).iter().any(|(_, c)| c.hp < kernel::world::unit::UNIT_MAX_HP * 9 / 10);
    // The race's commit: go for the keeper (the unit on the camp) only when the blows of this turn's units (the capturer
    // keeps its move to enter) add up to its hit points; else leave it alone, so that the neutral, who moves after us,
    // cannot finish what we began, and be first at it in the next turn (the neutral's blows wear it down for us).
    let keeper_hp = {
        let auth = game.view().auth;
        auth.indexes().units_on(camp).iter().filter_map(|h| auth.units().get(*h)).find(|u| u.core.owner == foe).map(|u| i64::from(u.core.hp))
    };
    let commit = race
        && keeper_hp.is_some_and(|hp| {
            let dealt: i64 = mine.iter().filter(|p| capturer.as_ref().is_none_or(|c| c.id != p.id)).filter_map(|p| act::reach_blow(game, rules, me, p.h, camp)).map(|(d, _)| d).sum();
            dealt >= hp
        });
    let hold_off = race && keeper_hp.is_some() && !commit;
    for p in order.clone() {
        // The camp is empty and a unit reaches it: in.
        if enter(game, rules, me, &mine, sc, !race) {
            return;
        }
        let Some(c) = act::alive(game, &p, me) else { continue };
        let ranged = act::range_of(rules, c.kind).is_some();
        let wounded = c.hp < kernel::world::unit::UNIT_MAX_HP * WOUNDED_PERCENT / 100;
        let is_capturer = capturer.as_ref().is_some_and(|q| q.id == p.id);
        if is_capturer {
            // Fresh and held back: a blow only where it kills; else close to the camp (within one move of it).
            if let Some((score, t)) = act::best_blow(game, rules, me, p.h, &act::foe_tiles(game, foe, None, false), Nerve::Reckless) {
                if score >= 10_000 {
                    let _ = act::strike(game, rules, me, p.h, t);
                }
            }
            if act::core_of(game, p.h).is_some_and(|u| u.owner == me && u.moves_left > 0) {
                let foes = act::foe_tiles(game, foe, None, false);
                let rivals: Vec<TileId> = third_alive(game, sc).iter().map(|(_, u)| u.tile).collect();
                act::move_by(game, rules, me, p.h, |g, x, _| {
                    let d = dist(g, x, camp);
                    (d.max(2) - 2, act::nearest(g, x, &foes) <= 1 && d > 1, act::nearest(g, x, &rivals), d)
                });
            }
            act::stand(game, rules, p.h);
            continue;
        }
        // The careful teacher is patient: its melee wait three tiles from the defenders until the shooters have drawn blood.
        let near_foe = act::nearest(game, c.tile, &all_foes(game));
        let rel = game.view().auth.turn().round.saturating_sub(sc.start_round);
        let softened = hurt && keeper_hp.is_none_or(|hp| hp <= i64::from(kernel::world::unit::UNIT_MAX_HP) * PATIENT_KEEPER_PERCENT / 100);
        let patient = !race && !ranged && rel < PATIENT_ROUNDS && !softened && near_foe <= 3;
        if hold_off {
            // No blow at the keeper: a blow elsewhere at even odds, then to two tiles from the camp.
            let others: Vec<TileId> = all_foes(game).into_iter().filter(|t| *t != camp).collect();
            if let Some((_, t)) = act::best_blow(game, rules, me, p.h, &others, Nerve::Even) {
                let _ = act::strike(game, rules, me, p.h, t);
            }
            if act::core_of(game, p.h).is_some_and(|u| u.owner == me && u.moves_left > 0) {
                let foes = all_foes(game);
                act::move_by(game, rules, me, p.h, |g, x, _| {
                    let d = dist(g, x, camp);
                    (d.max(if ranged { 3 } else { 2 }) - if ranged { 3 } else { 2 }, act::nearest(g, x, &foes) <= 1 && d > 1, d)
                });
            }
            act::stand(game, rules, p.h);
            continue;
        }
        let keeper = race && all_foes(game).contains(&camp) && act::reach_blow(game, rules, me, p.h, camp).is_some();
        let nerve = if keeper {
            Nerve::Reckless
        } else if wounded || patient {
            Nerve::Careful
        } else {
            Nerve::Even
        };
        let aim = if wounded && !race {
            Aim::Away(home)
        } else if patient {
            Aim::Hold(c.tile)
        } else if ranged {
            Aim::Support
        } else {
            Aim::Close
        };
        let targets = move |g: &GameState| -> Vec<TileId> { if keeper { vec![camp] } else { all_foes(g) } };
        act::unit_turn(game, rules, me, foe, p.h, &aim, nerve, &targets, &friends);
    }
    let _ = enter(game, rules, me, &mine, sc, !race);
}

/// The verdict at a round's end.
pub fn judge(rules: &CompiledRuleset, game: &GameState, sc: &mut Scene, rel: u32) -> Option<bool> {
    let _ = rules;
    mark(game, sc);
    let by = cleared_by(game, sc);
    // What counts is what was lost when the camp was cleared (the units that fall in the turns after it do not).
    let lost = match (by, sc.info.get("lost_at_clear").and_then(|v| v.parse::<usize>().ok())) {
        (Some(_), Some(l)) => l,
        _ => sc.mine.len() - sc.mine_alive(game).len(),
    };
    let cap = loss_cap(sc.fam);
    if lost > cap && sc.cap_broken.is_none() {
        sc.cap_broken = Some(rel);
    }
    if let Some((who, _)) = by {
        if sc.taken.is_none() && who == sc.learner {
            sc.taken = Some(rel);
        }
        if Some(who) == sc.neutral {
            sc.info.insert("lost_to".into(), "neutral".into());
        }
    }
    if matches!(by, Some((who, _)) if Some(who) == sc.neutral) {
        Some(false)
    } else if sc.cap_broken.is_some() {
        Some(false)
    } else if matches!(by, Some((who, _)) if who == sc.learner) {
        Some(true)
    } else if sc.mine_alive(game).is_empty() || rel >= sc.rounds {
        Some(false)
    } else {
        None
    }
}
