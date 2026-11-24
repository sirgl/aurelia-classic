//! The naval and air trainers (work/nets/TRAINERS-PLAN.md, lane "Sea and air"): four families on the sea and in
//! the air, each with the scene, the win rule, a scripted enemy, the right teacher, a naive one and (by the
//! suite's own controls) the passive and random plays.
//!
//! WIN RULES (fixed before the gate's runs; "lost" = our scene units dead or captured, "killed" = the enemy's
//! combat units dead; a round is a game round):
//! - naval-duel: a sea of shallow water (islets where the water would be deep: every ship of every era sails
//!   everywhere), our port city on the west shore, 4 warships of ours against 3 of the enemy's of the same era
//!   and mix, a scripted hunter (a strike at even odds or better, else it closes on the nearest of ours). WIN:
//!   every enemy ship sunk within 10 rounds with at most 1 of ours lost. Ships heal only in their own borders
//!   and only when they did not move (the rules' heal), so a ship below 40 hit points pulled back to the port
//!   heals while the rest keep firing.
//! - landing-escort: 3-4 land units of ours embarked mid-sea, 4 warships of ours escorting, 2 enemy ships
//!   hunting; the far shore is 7 tiles away. WIN: every land unit stands on the far shore within 10 rounds
//!   (the embarked move 2 tiles a round; the hunters 4 to 6).
//! - air-intercept (modern and atomic eras): our city (garrison, 1-2 fighters, an anti-air unit 3 tiles behind it)
//!   faces an enemy air base 3 tiles off with 3-4 bombers that strike our city every round (a bomber below 40
//!   hit points mends in its base). WIN: every bomber dead, the city holds at least 75 percent of its hit points
//!   and at most 1 of our units lost, within 8 rounds. Fighters sent to strike the base die there.
//! - air-recon (modern and atomic eras): 2-3 enemy melee units stand fortified 5 tiles from our city, out of its
//!   sight; our city holds 3-4 bombers and a scout (the era's scout, a cheap unit). An air strike needs its
//!   target in sight. WIN: 2 of the enemy units killed within 6 rounds.
//!   The information era is skipped for both air families: the stealth bomber's range and sight make the
//!   scene decide nothing (the passive and the blind plays win there).
//!
//! THE TEACHERS: the right ones are `ref-sea-focus` (focus fire; the wounded pulled back to the port to heal),
//! `ref-escort` (the escorts screen and strike the hunters first, the land units wait out of the hunters' reach
//! until they are dead), `ref-intercept` (fighters on interception duty, the anti-air unit brought into the city),
//! `ref-recon` (the scout walks until the target is in sight, then the bombers strike). The naive ones are
//! `ref-sea-solo` (every ship fights the nearest foe alone, no retreat), `ref-rush` (everybody makes for the
//! far shore at once), `ref-sortie` (the fighters strike the enemy's base instead of covering the city, the
//! anti-air unit stays behind), `ref-blind` (the bombers wait for a target nobody shows them).
//! Any other `ref-` name plays the naive style of the family.

use classic_rules::ids::{PlayerId, TileId, UnitKindId};
use classic_rules::{ClassicGame, Command, CompiledRuleset, GameState};
use kernel::world::unit::{Stance, UNIT_MAX_HP};

use super::act::{self, Aim, Nerve};
use super::families::{blank, kinds_label, Fam, Painter};
use super::lab::{dist, Arm, EraKinds, Lab};
use super::{memory, Piece, Scene};

pub fn is_sea_air(f: Fam) -> bool {
    matches!(f, Fam::NavalDuel | Fam::LandingEscort | Fam::AirIntercept | Fam::AirRecon)
}

pub fn rounds(f: Fam) -> u32 {
    match f {
        Fam::NavalDuel => 10,
        Fam::LandingEscort => 10,
        Fam::AirIntercept => 8,
        _ => 6,
    }
}

pub fn solvers(f: Fam) -> &'static [&'static str] {
    match f {
        Fam::NavalDuel => &["ref-sea-focus"],
        Fam::LandingEscort => &["ref-escort"],
        Fam::AirIntercept => &["ref-intercept"],
        _ => &["ref-recon"],
    }
}

pub fn tricks(f: Fam) -> &'static [(&'static str, char)] {
    match f {
        Fam::NavalDuel => &[("MT4", 'U'), ("MT7", 'U')],
        Fam::LandingEscort => &[("N2", 'U')],
        Fam::AirIntercept => &[("A1", 'D')],
        _ => &[("A1", 'U')],
    }
}

pub fn task(f: Fam) -> &'static str {
    match f {
        Fam::NavalDuel => "ships against ships: focus fire, the wounded pulled back to heal",
        Fam::LandingEscort => "land units embarked cross to a shore under escort; enemy ships hunt them",
        Fam::AirIntercept => "enemy bombers strike our city from their air base: fighters on interception duty keep the city hp; fighters sent out die",
        _ => "an air strike on a target in the fog: reveal it first, then strike",
    }
}

fn embark_known(rules: &CompiledRuleset, era: u8) -> bool {
    rules.technologies.iter().any(|t| t.allows_embarking && t.content_era.and_then(|e| rules.eras.get(e.0 as usize)).is_some_and(|e| e.index <= era))
}

/// Whether the family can be drawn in an era.
pub fn claims(f: Fam, rules: &CompiledRuleset, k: &EraKinds) -> bool {
    let ships = k.has(Arm::NavalMelee) || k.has(Arm::NavalRanged);
    match f {
        Fam::NavalDuel => ships,
        Fam::LandingEscort => ships && embark_known(rules, k.era) && k.has(Arm::Melee),
        Fam::AirIntercept => k.era <= 6 && k.has(Arm::Fighter) && k.has(Arm::Bomber) && k.has(Arm::Melee) && aa_kind(rules, k.era).is_some(),
        _ => k.era <= 6 && k.has(Arm::Bomber) && k.has(Arm::Melee),
    }
}

/// The era's anti-air unit: the mobile SAM (atomic era on), else the anti-aircraft gun (modern era).
fn aa_kind(rules: &CompiledRuleset, era: u8) -> Option<UnitKindId> {
    let by = |key: &str| super::lab::kind_by_key(rules, key).filter(|k| super::lab::era_of(rules, *k).is_some_and(|e| e <= era));
    by("core:unit/mobile-sam").or_else(|| by("core:unit/anti-aircraft-gun"))
}

// ---------------------------------------------------------------------------
// Building

/// A band of shallow sea: columns `x0..=x1` from the centre, `ry` rows each way; islets where the water would be deep.
fn paint_sea(p: &mut Painter, x0: i64, x1: i64, ry: i64) -> std::collections::BTreeSet<u32> {
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
    // The islets can close a pocket of water: ships are drawn only in the largest connected body.
    let map = p.game.view().auth.map();
    let water: Vec<TileId> = sea.iter().copied().filter(|t| map.is_water(*t)).collect();
    let mut seen: std::collections::BTreeSet<u32> = std::collections::BTreeSet::new();
    let mut best: std::collections::BTreeSet<u32> = std::collections::BTreeSet::new();
    for &t in &water {
        if seen.contains(&t.0) {
            continue;
        }
        let mut comp: std::collections::BTreeSet<u32> = std::collections::BTreeSet::new();
        let mut q = std::collections::VecDeque::from([t]);
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

/// `n` free water tiles within `r` of `anchor`, nearest first with ties drawn.
fn water_at(p: &mut Painter, body: &std::collections::BTreeSet<u32>, anchor: TileId, r: i32, n: usize) -> Result<Vec<TileId>, String> {
    let auth = p.game.view().auth;
    let mut v: Vec<(i32, u64, TileId)> = super::lab::tiles_within(&p.game, anchor, r)
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

/// A foot kind of the era that cannot intercept aircraft (the era's melee draw also holds the anti-aircraft gun and the SAM).
fn ground_kind(p: &mut Painter, arm: Arm) -> Option<UnitKindId> {
    for _ in 0..40 {
        let k = p.kind(arm)?;
        if p.rules.units[k.0 as usize].combat.air_intercept_range == 0 {
            return Some(k);
        }
    }
    None
}

fn put_all(p: &mut Painter, side: PlayerId, tiles: &[TileId], kinds: &[UnitKindId], role: &'static str, into: &mut Vec<Piece>) {
    for (t, k) in tiles.iter().zip(kinds) {
        let piece = p.put(side, *t, *k, role);
        into.push(piece);
    }
}

pub fn build(p: &mut Painter, fam: Fam, lab: &Lab, instance: usize, attempt: u32) -> Result<Scene, String> {
    let rules = p.rules;
    let mut s = blank(fam, lab, instance, attempt);
    let (me, foe) = (lab.learner, lab.enemy);
    let at = |p: &Painter, dx: i64, dy: i64| p.at(dx, dy).ok_or_else(|| "off the map".to_owned());
    match fam {
        Fam::NavalDuel => {
            let body = paint_sea(p, -6, 7, 5);
            let port = at(p, -7, 0)?;
            if p.game.view().auth.map().is_water(port) {
                return Err("the port is water".into());
            }
            p.city(me, port, 3, false);
            s.refuge = Some(port);
            let n = 4usize;
            let m = 3usize;
            let mk = |p: &mut Painter, count: usize, off: usize| -> Result<Vec<UnitKindId>, String> {
                (0..count).map(|i| p.kind(if (i + off) % 2 == 0 { Arm::NavalRanged } else { Arm::NavalMelee }).ok_or_else(|| "no ship".to_owned())).collect()
            };
            let off = p.d.range(0, 1) as usize;
            let (mine_k, theirs_k) = (mk(p, n, off)?, mk(p, m, off)?);
            let a = at(p, -3, 0)?;
            let b = at(p, 3, 0)?;
            let (mt, tt) = (water_at(p, &body, a, 2, n)?, water_at(p, &body, b, 2, m)?);
            put_all(p, me, &mt, &mine_k, "ship", &mut s.mine);
            put_all(p, foe, &tt, &theirs_k, "foe", &mut s.theirs);
        }
        Fam::LandingEscort => {
            // The sea is 7 columns wide; the landing is the first land column east of it.
            let half = 6 as i64;
            let body = paint_sea(p, -half, half, 5);
            let shore = half + 1;
            s.info.insert("shore".into(), shore.to_string());
            let landing = at(p, shore, 0)?;
            if p.game.view().auth.map().is_water(landing) {
                return Err("the landing is water".into());
            }
            s.target = Some(landing);
            s.refuge = Some(at(p, -half - 1, 0)?);
            let n = p.d.range(3, 4) as usize;
            let tk: Vec<UnitKindId> = (0..n).map(|i| p.kind(if i % 2 == 0 { Arm::Melee } else { Arm::Ranged }).ok_or("no land kind".to_owned())).collect::<Result<_, _>>()?;
            let off = p.d.range(0, 1) as usize;
            let esc: Vec<UnitKindId> = (0..4).map(|i| p.kind(if (i + off) % 2 == 0 { Arm::NavalRanged } else { Arm::NavalMelee }).ok_or("no ship".to_owned())).collect::<Result<_, _>>()?;
            let hun: Vec<UnitKindId> = (0..2).map(|i| p.kind(if i % 3 == 1 { Arm::NavalRanged } else { Arm::NavalMelee }).ok_or("no ship".to_owned())).collect::<Result<_, _>>()?;
            let (ta, ea, ha) = (at(p, -half + 5 as i64, 0)?, at(p, -half + 1 as i64, 0)?, at(p, 5 as i64, 0)?);
            let tt = water_at(p, &body, ta, 2, n)?;
            put_all(p, me, &tt, &tk, "transport", &mut s.mine);
            let et = water_at(p, &body, ea, 2, esc.len())?;
            put_all(p, me, &et, &esc, "ship", &mut s.mine);
            let ht = water_at(p, &body, ha, 2, hun.len())?;
            put_all(p, foe, &ht, &hun, "foe", &mut s.theirs);
            s.info.insert("need".into(), n.to_string());
        }
        Fam::AirIntercept => {
            // Our city and the enemy's air base 3 tiles apart: the base's sight (3) shows our city, the city's strike (2)
            // does not reach it. The bombers strike the city (and whatever of ours stands out).
            let home = at(p, -2, 0)?;
            p.city(me, home, 4, false);
            s.refuge = Some(home);
            s.target = Some(home);
            let gk = ground_kind(p, Arm::Melee).ok_or("no melee")?;
            let g = p.put(me, home, gk, "garrison");
            s.mine.push(g);
            for _ in 0..p.d.range(1, 2) {
                let k = p.kind(Arm::Fighter).ok_or("no fighter")?;
                let piece = p.put(me, home, k, "fighter");
                s.mine.push(piece);
            }
            // The anti-air unit starts 3 tiles behind the city: out of reach of the strikes' tile until it is brought up.
            let aa = aa_kind(rules, lab.era).ok_or("no anti-air unit")?;
            let behind = at(p, -5, p.d.range(-1, 1))?;
            let piece = p.put(me, behind, aa, "aa");
            s.mine.push(piece);
            let base = at(p, 1, 0)?;
            p.city(foe, base, 4, false);
            for _ in 0..p.d.range(3, 4) {
                let k = p.kind(Arm::Bomber).ok_or("no bomber")?;
                let piece = p.put(foe, base, k, "bomber");
                s.theirs.push(piece);
            }
            s.hold = Some(home);
        }
        _ => {
            // air-recon: our base with bombers and a scout; a fortified stack out of the base's sight.
            let home = at(p, -4, 0)?;
            p.city(me, home, 4, false);
            s.refuge = Some(home);
            let gk = ground_kind(p, Arm::Melee).ok_or("no melee")?;
            let g = p.put(me, home, gk, "garrison");
            s.mine.push(g);
            let scout = super::lab::kind_by_key(rules, "core:unit/scout").or_else(|| p.kind(Arm::Melee)).ok_or("no scout kind")?;
            let piece = p.put(me, home, scout, "scout");
            s.mine.push(piece);
            for _ in 0..p.d.range(3, 4) {
                let k = p.kind(Arm::Bomber).ok_or("no bomber")?;
                let piece = p.put(me, home, k, "bomber");
                s.mine.push(piece);
            }
            let anchor = at(p, 1, p.d.range(-1, 1))?;
            for t in p.spots(anchor, 1, p.d.range(2, 3) as usize)? {
                let k = ground_kind(p, Arm::Melee).ok_or("no melee")?;
                let piece = p.put(foe, t, k, "foe");
                let _ = act::fortify(&mut p.game, rules, foe, piece.h);
                s.theirs.push(piece);
            }
            s.hold = Some(anchor);
        }
    }
    s.info.insert("mine".into(), kinds_label(rules, &s.mine));
    s.info.insert("theirs".into(), kinds_label(rules, &s.theirs));
    Ok(s)
}

// ---------------------------------------------------------------------------
// The teachers

fn handles(ps: &[Piece]) -> Vec<classic_rules::ids::UnitHandle> {
    ps.iter().map(|p| p.h).collect()
}

/// The tiles of the enemy's pieces that are combat units.
fn foes_of(rules: &CompiledRuleset, game: &GameState, sc: &Scene) -> Vec<TileId> {
    let mut v: Vec<TileId> = sc.theirs_alive(game).into_iter().filter(|(p, _)| !act::is_civilian(rules, p.kind)).map(|(_, c)| c.tile).collect();
    v.sort_by_key(|t| t.0);
    v.dedup();
    v
}

fn nearest_foe(game: &GameState, from: TileId, foes: &[TileId]) -> Option<TileId> {
    foes.iter().copied().min_by_key(|t| (dist(game, from, *t), t.0))
}

/// The sailing distance (tiles through water) from the nearest of `goals` (land goals count as 0: a ship enters a coastal
/// city, a land unit lands): the islets of the sea make the straight distance a poor guide.
fn sail_dists(game: &GameState, goals: &[TileId]) -> std::collections::BTreeMap<u32, i32> {
    let map = game.view().auth.map();
    let mut d = std::collections::BTreeMap::new();
    let mut q = std::collections::VecDeque::new();
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

/// One unit's turn of a retreat or a crossing: a kill or a good blow first (`nerve`), then the step that ends nearest
/// the goal by sailing distance, then the end of the turn.
#[allow(clippy::too_many_arguments)]
fn sail_to(rules: &CompiledRuleset, game: &mut GameState, side: PlayerId, unit: classic_rules::ids::UnitHandle, goals: &[TileId], nerve: Nerve, targets: &[TileId]) {
    if let Some((_, t)) = act::best_blow(game, rules, side, unit, targets, nerve) {
        act::strike(game, rules, side, unit, t);
    }
    let d = sail_dists(game, goals);
    if act::debug() {
        let me = act::core_of(game, unit);
        let reach: Vec<(u32, i32)> = act::reachable(game, rules, side, unit).into_iter().map(|(t, _)| (t.0, d.get(&t.0).copied().unwrap_or(999))).collect();
        eprintln!("  sail {:?} moves {:?} reach {:?}", me.map(|m| m.tile.0), me.map(|m| (m.moves_left, m.moves_max)), reach);
    }
    act::move_by(game, rules, side, unit, |_, x, _| d.get(&x.0).copied().unwrap_or(999));
    act::stand(game, rules, unit);
}

/// One ship's turn of a fight: a fine blow from where it stands (`nerve`), else a step and a blow from a tile it reaches
/// (the rules' reachable tiles: the islets are no obstacle to it), else a step toward the foe nearest by sailing
/// distance (a shooter stops at its range), then the end of the turn. `targets`: what it may strike; `pursue`: what it closes on.
#[allow(clippy::too_many_arguments)]
fn engage(rules: &CompiledRuleset, game: &mut GameState, side: PlayerId, foe: PlayerId, unit: classic_rules::ids::UnitHandle, targets: &[TileId], pursue: &[TileId], nerve: Nerve) {
    let Some(me) = act::core_of(game, unit) else { return };
    let reach = act::range_of(rules, me.kind).unwrap_or(1);
    let mut struck = false;
    if act::debug() {
        eprintln!("  engage {} at {} moves {} targets {:?} pursue {:?}", rules.units[me.kind.0 as usize].name, me.tile.0, me.moves_left, targets.iter().map(|t| t.0).collect::<Vec<_>>(), pursue.iter().map(|t| t.0).collect::<Vec<_>>());
    }
    if let Some((_, t)) = act::best_blow(game, rules, side, unit, targets, nerve) {
        struck = act::strike(game, rules, side, unit, t);
    }
    if !struck && act::approach_strike(game, rules, side, foe, unit, targets, nerve) {
        struck = true;
    }
    if !struck {
        let d = sail_dists(game, pursue);
        if act::debug() {
            let r: Vec<(u32, i32)> = act::reachable(game, rules, side, unit).into_iter().map(|(t, _)| (t.0, d.get(&t.0).copied().unwrap_or(999))).collect();
            eprintln!("    step reach {r:?}");
        }
        act::move_by(game, rules, side, unit, |_, x, _| if reach > 1 { (d.get(&x.0).copied().unwrap_or(999) - reach).abs() } else { d.get(&x.0).copied().unwrap_or(999) });
    }
    act::stand(game, rules, unit);
}

fn hp_below(c: &kernel::world::unit::UnitCore, pct: u16) -> bool {
    u32::from(c.hp) * 100 < u32::from(UNIT_MAX_HP) * u32::from(pct)
}

/// The turn of `name` for the learner's pieces; false when `scene` is not one of this module's.
pub fn reference_turn(rules: &CompiledRuleset, game: &mut GameState, scene: &mut Scene, name: &str) -> bool {
    if !is_sea_air(scene.fam) {
        return false;
    }
    match (scene.fam, name) {
        (Fam::NavalDuel, "ref-sea-focus") => sea_focus(rules, game, scene),
        (Fam::NavalDuel, _) => sea_solo(rules, game, scene),
        (Fam::LandingEscort, "ref-escort") => landing(rules, game, scene, true),
        (Fam::LandingEscort, _) => landing(rules, game, scene, false),
        (Fam::AirIntercept, "ref-intercept") => air_intercept(rules, game, scene, true),
        (Fam::AirIntercept, _) => air_intercept(rules, game, scene, false),
        (Fam::AirRecon, "ref-recon") => air_recon(rules, game, scene, true),
        (_, _) => air_recon(rules, game, scene, false),
    }
    true
}

/// The ships in a script's order: shooters first, then the rest (ties by id).
fn ships_order(rules: &CompiledRuleset, sc: &Scene) -> Vec<Piece> {
    let mut order: Vec<Piece> = sc.mine.clone();
    order.sort_by_key(|p| (act::range_of(rules, p.kind).is_none(), p.id.0));
    order
}

/// Focus fire with the wounded pulled back to the port to heal.
fn sea_focus(rules: &CompiledRuleset, game: &mut GameState, scene: &Scene) {
    let (me, foe) = (scene.learner, scene.enemy);
    let sc = scene.clone();
    let friends = handles(&sc.mine);
    let port = sc.refuge.unwrap_or(sc.centre);
    let foes = |g: &GameState| foes_of(rules, g, &sc);
    let order = ships_order(rules, &sc);
    // Who retreats this turn (decided on the hit points at the turn's start): below 55 %, and rests until 85 %.
    let retreating: Vec<Piece> = order
        .iter()
        .filter(|p| act::alive(game, p, me).is_some_and(|c| hp_below(&c, 40) || (hp_below(&c, 80) && dist(game, c.tile, port) <= 1)))
        .cloned()
        .collect();
    for (ix, p) in order.iter().enumerate() {
        let Some(c) = act::alive(game, p, me) else { continue };
        let ranged = act::range_of(rules, c.kind).is_some();
        if retreating.iter().any(|q| q.id == p.id) {
            if dist(game, c.tile, port) <= 1 {
                // At the port and nobody near: rest (no move, no shot: the rules heal a ship that did not move).
                act::stand(game, rules, p.h);
            } else {
                sail_to(rules, game, me, p.h, &[port], Nerve::Careful, &foes(game));
            }
            continue;
        }
        let to_act: Vec<classic_rules::ids::UnitHandle> = order[ix..].iter().filter(|q| act::alive(game, q, me).is_some() && !retreating.iter().any(|r| r.id == q.id)).map(|q| q.h).collect();
        let focus = act::focus_target(game, rules, me, &to_act, &foes(game));
        let focused = |_: &GameState| focus.into_iter().collect::<Vec<TileId>>();
        let targets: &dyn Fn(&GameState) -> Vec<TileId> = if focus.is_some() { &focused } else { &foes };
        let nerve = Nerve::Even;
        let all = foes(game);
        let strike_at: Vec<TileId> = focus.into_iter().collect();
        engage(rules, game, me, foe, p.h, if focus.is_some() { &strike_at } else { &all }, &all, nerve);
        let _ = (ranged, &friends);
    }
}

/// The naive duel: every ship fights the foe nearest to it alone and never pulls back.
fn sea_solo(rules: &CompiledRuleset, game: &mut GameState, scene: &Scene) {
    let (me, foe) = (scene.learner, scene.enemy);
    let sc = scene.clone();
    for p in ships_order(rules, &sc) {
        let Some(c) = act::alive(game, &p, me) else { continue };
        let all = foes_of(rules, game, &sc);
        let nearest: Vec<TileId> = nearest_foe(game, c.tile, &all).into_iter().collect();
        engage(rules, game, me, foe, p.h, &nearest, &nearest, Nerve::Even);
    }
}

fn landed(game: &GameState, sc: &Scene, c: &kernel::world::unit::UnitCore) -> bool {
    let w = i64::from(game.view().auth.map().dimensions.width);
    let shore: i64 = sc.info.get("shore").and_then(|s| s.parse().ok()).unwrap_or(9);
    let dx = i64::from(c.tile.0) % w - i64::from(sc.centre.0) % w;
    dx >= shore && !game.view().auth.map().is_water(c.tile)
}

fn landing(rules: &CompiledRuleset, game: &mut GameState, scene: &Scene, escort: bool) {
    let (me, foe) = (scene.learner, scene.enemy);
    let sc = scene.clone();
    let friends = handles(&sc.mine);
    let goal = sc.target.unwrap_or(sc.centre);
    let home = sc.refuge.unwrap_or(sc.centre);
    let foes = |g: &GameState| foes_of(rules, g, &sc);
    // The landing tiles: the first land column east of the sea.
    let shore: Vec<TileId> = {
        let w = i64::from(game.view().auth.map().dimensions.width);
        let wanted: i64 = sc.info.get("shore").and_then(|s| s.parse().ok()).unwrap_or(9);
        (0..game.view().auth.map().tile_count() as u32).map(TileId).filter(|t| i64::from(t.0) % w - i64::from(sc.centre.0) % w == wanted && !game.view().auth.map().is_water(*t) && dist(game, *t, sc.centre) <= 12).collect()
    };
    let rel = game.view().auth.turn().round.saturating_sub(sc.start_round);
    let mut order: Vec<Piece> = sc.mine.clone();
    order.sort_by_key(|p| (p.role != "ship", act::range_of(rules, p.kind).is_none(), p.id.0));
    for (ix, p) in order.iter().enumerate() {
        let Some(c) = act::alive(game, p, me) else { continue };
        let ranged = act::range_of(rules, c.kind).is_some();
        if !escort {
            // Everybody rushes the far shore at once.
            if landed(game, &sc, &c) || p.role == "ship" {
                if p.role == "ship" {
                    act::unit_turn(game, rules, me, foe, p.h, &Aim::To(goal), Nerve::Even, &foes, &friends);
                } else {
                    act::stand(game, rules, p.h);
                }
            } else {
                sail_to(rules, game, me, p.h, &shore, Nerve::Even, &foes(game));
            }
            continue;
        }
        if p.role == "ship" {
            let to_act: Vec<classic_rules::ids::UnitHandle> = order[ix..].iter().filter(|q| q.role == "ship" && act::alive(game, q, me).is_some()).map(|q| q.h).collect();
            let focus = act::focus_target(game, rules, me, &to_act, &foes(game));
            let focused = |_: &GameState| focus.into_iter().collect::<Vec<TileId>>();
            let targets: &dyn Fn(&GameState) -> Vec<TileId> = if focus.is_some() { &focused } else { &foes };
            // The escorts go for the hunters (the nearest one of them when none can be struck this turn).
            act::unit_turn(game, rules, me, foe, p.h, if ranged { &Aim::Support } else { &Aim::Close }, Nerve::Even, targets, &friends);
            continue;
        }
        // A land unit: waits out of the hunters' reach while any hunter lives (and the round limit is far), else crosses.
        let hunters = foes(game);
        if landed(game, &sc, &c) {
            act::stand(game, rules, p.h);
        } else if hunters.is_empty() || rel + 4 >= sc.rounds {
            sail_to(rules, game, me, p.h, &shore, Nerve::Even, &hunters);
        } else {
            // Wait where the hunters cannot reach this turn (no move at all when none can); else step out of their reach.
            let cur = c.tile;
            let hs = hunters.clone();
            act::move_by(game, rules, me, p.h, |g, x, _| (act::threat_on(g, rules, foe, x), x != cur, -act::nearest(g, x, &hs).min(8), dist(g, x, home)));
            act::stand(game, rules, p.h);
        }
    }
}

fn air_intercept(rules: &CompiledRuleset, game: &mut GameState, scene: &Scene, right: bool) {
    let (me, foe) = (scene.learner, scene.enemy);
    let sc = scene.clone();
    let friends = handles(&sc.mine);
    let none = |_: &GameState| Vec::<TileId>::new();
    for p in &sc.mine {
        let Some(c) = act::alive(game, p, me) else { continue };
        match p.role {
            "fighter" if right => {
                // Interception duty: the fighter must not move or strike this turn (a unit that moved cannot intercept).
                let _ = act::try_cmd(game, rules, me, Command::Intercept { unit: p.h });
            }
            "fighter" => {
                // The naive sortie: strike the enemy's base and spotter, any blow the rules accept.
                let targets: Vec<TileId> = act::foe_tiles(game, foe, None, true);
                if let Some((_, t)) = act::best_blow(game, rules, me, p.h, &targets, Nerve::Reckless) {
                    act::strike(game, rules, me, p.h, t);
                }
                act::stand(game, rules, p.h);
            }
            "aa" if right => {
                // The anti-air unit goes into the city (it intercepts what strikes within 2 tiles of it).
                let home = sc.refuge.unwrap_or(sc.centre);
                act::unit_turn(game, rules, me, foe, p.h, &Aim::To(home), Nerve::Careful, &none, &friends);
            }
            _ => {
                act::stand(game, rules, p.h);
                let _ = c;
            }
        }
    }
}

fn air_recon(rules: &CompiledRuleset, game: &mut GameState, scene: &Scene, right: bool) {
    let (me, foe) = (scene.learner, scene.enemy);
    let sc = scene.clone();
    let friends = handles(&sc.mine);
    let none = |_: &GameState| Vec::<TileId>::new();
    let mut order: Vec<Piece> = sc.mine.clone();
    order.sort_by_key(|p| (match p.role { "scout" => 0, "bomber" => 1, _ => 2 }, p.id.0));
    for p in &order {
        let Some(c) = act::alive(game, p, me) else { continue };
        let seen: Vec<TileId> = memory::seen_foes(rules, game, me, foe).into_iter().map(|u| u.tile).collect();
        match p.role {
            "scout" if right => {
                if seen.is_empty() {
                    // Walk toward the stack until it is in sight.
                    let goal = sc.hold.unwrap_or(sc.centre);
                    act::unit_turn(game, rules, me, foe, p.h, &Aim::To(goal), Nerve::Careful, &none, &friends);
                } else {
                    act::stand(game, rules, p.h);
                }
            }
            "bomber" => {
                if let Some((_, t)) = act::best_blow(game, rules, me, p.h, &seen, Nerve::Reckless) {
                    act::strike(game, rules, me, p.h, t);
                }
                act::stand(game, rules, p.h);
            }
            _ => {
                act::stand(game, rules, p.h);
                let _ = c;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// The enemy

/// The scripted enemy of the air scenes (the sea scenes use the scripted hunter); false: not handled here.
pub fn enemy_turn(rules: &CompiledRuleset, game: &mut GameState, scene: &mut Scene) -> bool {
    if !is_sea_air(scene.fam) {
        return false;
    }
    let (me, foe) = (scene.enemy, scene.learner);
    let sc = scene.clone();
    if matches!(sc.fam, Fam::NavalDuel | Fam::LandingEscort) {
        // The scripted hunter of the sea: a blow at even odds or better from where it stands or a tile it reaches, else it
        // closes on the nearest of ours by sailing distance (a shooter stops at its range).
        let mut order: Vec<Piece> = sc.theirs.clone();
        order.sort_by_key(|p| (act::range_of(rules, p.kind).is_none(), p.id.0));
        for p in &order {
            if act::alive(game, p, me).is_none() {
                continue;
            }
            let ours: Vec<TileId> = act::foe_tiles(game, foe, Some(&sc.mine), false);
            engage(rules, game, me, foe, p.h, &ours, &ours, Nerve::Even);
        }
        return true;
    }
    for p in &sc.theirs {
        let Some(c) = act::alive(game, p, me) else { continue };
        if p.role == "bomber" && !hp_below(&c, 40) {
            let ours: Vec<TileId> = sc.mine_alive(game).into_iter().filter(|(q, _)| q.role != "garrison").map(|(_, u)| u.tile).collect();
            if act::debug() {
                for t in &ours {
                    let cmd = Command::RangedAttack { unit: p.h, target: *t };
                    eprintln!("  bomber {} -> {} d{} accepts {:?} preview {:?}", c.tile.0, t.0, dist(game, c.tile, *t), game.accepts_order(rules, me, &cmd), act::preview(game, rules, p.h, None, *t));
                }
            }
            if let Some((_, t)) = act::best_blow(game, rules, me, p.h, &ours, Nerve::Reckless) {
                act::strike(game, rules, me, p.h, t);
            }
        }
        act::stand(game, rules, p.h);
    }
    true
}

// ---------------------------------------------------------------------------
// Judging

/// The city's hit points, percent of its maximum.
fn city_hp_share(rules: &CompiledRuleset, game: &GameState, t: TileId) -> Option<u32> {
    let auth = game.view().auth;
    let c = auth.indexes().city_on_tile(t).and_then(|c| auth.cities().get(c))?;
    let max = i64::from(classic_rules::combat::city::max_hit_points(auth, rules, c));
    Some(((max - i64::from(c.damage)).max(0) * 100 / max.max(1)) as u32)
}

fn lost_of(game: &GameState, sc: &Scene) -> usize {
    sc.mine.len() - sc.mine_alive(game).len()
}

pub fn judge(rules: &CompiledRuleset, game: &GameState, sc: &Scene, rel: u32) -> Option<bool> {
    let limit = rel >= sc.rounds;
    let theirs = sc.theirs_alive(game).into_iter().filter(|(p, _)| !act::is_civilian(rules, p.kind)).count();
    let killed = sc.theirs.len() - sc.theirs_alive(game).len();
    let lost = lost_of(game, sc);
    match sc.fam {
        Fam::NavalDuel => {
            if theirs == 0 && lost <= 1 {
                Some(true)
            } else if lost >= 2 || limit {
                Some(false)
            } else {
                None
            }
        }
        Fam::LandingEscort => {
            let need: usize = sc.info.get("need").and_then(|s| s.parse().ok()).unwrap_or(2);
            let ok = sc.mine_alive(game).into_iter().filter(|(p, c)| p.role == "transport" && landed(game, sc, c)).count();
            let alive = sc.mine_alive(game).into_iter().filter(|(p, _)| p.role == "transport").count();
            if ok >= need {
                Some(true)
            } else if alive < need || limit {
                Some(false)
            } else {
                None
            }
        }
        Fam::AirIntercept => {
            // Our city's hit points kept when the bombers are all dead or the rounds end.
            let bombers = sc.theirs_alive(game).into_iter().filter(|(p, _)| p.role == "bomber").count();
            let share = sc.target.and_then(|t| city_hp_share(rules, game, t)).unwrap_or(100);
            if lost >= 2 {
                Some(false)
            } else if bombers == 0 || limit {
                Some(bombers == 0 && share >= 75)
            } else {
                None
            }
        }
        _ => {
            if killed >= 2 {
                Some(true)
            } else if limit {
                Some(false)
            } else {
                None
            }
        }
    }
}

/// Whether a scene piece of `side` stands on interception duty (the scene rule leaves such a piece alone: a unit
/// that fortified, shot or ended its turn by hand cannot intercept).
pub fn on_duty(c: &kernel::world::unit::UnitCore) -> bool {
    c.stance == Stance::Intercept
}
