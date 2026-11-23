//! What scripted seats do with a scene's units: the primitives (strike by the rules' previews, step,
//! stand), the competent scripts the references are made of, the scripted humans of the trick scenes,
//! and the passive and random controls. Every choice is a function of the game (ties by tile and id),
//! so a script plays the same instance the same way.

use classic_rules::combat::damage::Roll;
use classic_rules::combat::exchange::{plan_attack, plan_attack_from, AttackKind};
use classic_rules::ids::{PlayerId, TileId, UnitHandle, UnitKindId};
use classic_rules::{ClassicGame, Command, CompiledRuleset, GameState};
use kernel::world::unit::UnitCore;

use super::lab::{dist, Draw};
use super::Piece;

pub fn core_of(game: &GameState, h: UnitHandle) -> Option<UnitCore> {
    game.view().auth.units().get(h).map(|u| *u.core)
}

/// The piece's unit if it is still the same unit and `side`'s.
pub fn alive(game: &GameState, p: &Piece, side: PlayerId) -> Option<UnitCore> {
    core_of(game, p.h).filter(|c| c.id == p.id && c.owner == side)
}

pub fn range_of(rules: &CompiledRuleset, kind: UnitKindId) -> Option<i32> {
    classic_rules::ai::combat::ranged_range(rules, kind)
}

pub fn moves_of(rules: &CompiledRuleset, kind: UnitKindId) -> i32 {
    rules.units.get(kind.0 as usize).map_or(1, |d| i32::from(d.movement.max(1)))
}

pub fn is_civilian(rules: &CompiledRuleset, kind: UnitKindId) -> bool {
    let d = &rules.units[kind.0 as usize];
    d.strength == 0 && d.ranged_strength == 0
}

pub fn try_cmd(game: &mut GameState, rules: &CompiledRuleset, side: PlayerId, cmd: Command) -> bool {
    if !game.accepts_order(rules, side, &cmd) {
        return false;
    }
    execute(game, rules, side, cmd)
}

/// Executes an accepted command; the army net's recorder sees it before and after (when it records).
fn execute(game: &mut GameState, rules: &CompiledRuleset, side: PlayerId, cmd: Command) -> bool {
    let pending = crate::army_net::record::before(game, rules, side, &cmd);
    let fact = crate::army_net::observe::log_before(game, rules, side, &cmd);
    let ok = game.execute(rules, cmd).is_ok();
    crate::army_net::observe::log_after(game, rules, fact, ok);
    crate::army_net::record::after(game, pending, ok);
    ok
}

/// The unit's turn is over: the seat's mind plays the rest of the seat and must not move it.
pub fn stand(game: &mut GameState, rules: &CompiledRuleset, h: UnitHandle) {
    crate::army_net::record::done(game, rules, h);
    if let Some(core) = game.world_mut().units.get_mut_core(h) {
        core.moves_left = 0;
    }
}

/// The defence percent of a tile for `side` (the rules' number).
pub fn defence(game: &GameState, rules: &CompiledRuleset, t: TileId, side: PlayerId) -> i64 {
    classic_rules::combat::terrain::tile_defense_percent(game.view().auth, rules, t, side)
}

/// The expected exchange of `unit` striking `target` (from where it stands, or from `from`):
/// (damage dealt, damage taken, the defender's hit points).
pub fn preview(game: &GameState, rules: &CompiledRuleset, unit: UnitHandle, from: Option<TileId>, target: TileId) -> Option<(i64, i64, i64)> {
    let me = core_of(game, unit)?;
    let how = if range_of(rules, me.kind).is_some() { AttackKind::Ranged } else { AttackKind::Melee };
    let plan = match from {
        Some(f) => plan_attack_from(game.view(), rules, unit, f, target, how).ok()?,
        None => plan_attack(game.view(), rules, unit, target, how).ok()?,
    };
    Some((i64::from(plan.damage_to_defender(Roll::Expected)), i64::from(plan.damage_to_attacker(Roll::Expected)), i64::from(plan.defender_hp)))
}

/// The attack command of `unit` on the unit or city on `target`.
pub fn attack_cmd(game: &GameState, rules: &CompiledRuleset, unit: UnitHandle, target: TileId) -> Option<Command> {
    let me = core_of(game, unit)?;
    let auth = game.view().auth;
    if range_of(rules, me.kind).is_some() {
        return Some(Command::RangedAttack { unit, target });
    }
    if let Some(city) = auth.indexes().city_on_tile(target) {
        return Some(Command::AttackCity { attacker: unit, city });
    }
    let defender = auth.indexes().units_on(target).iter().copied().find(|h| auth.units().get(*h).is_some_and(|u| u.core.owner != me.owner))?;
    Some(Command::AttackUnit { attacker: unit, defender })
}

/// Strikes `target`; a siege unit that must set up does so first. True when the strike was made.
pub fn strike(game: &mut GameState, rules: &CompiledRuleset, side: PlayerId, unit: UnitHandle, target: TileId) -> bool {
    let Some(cmd) = attack_cmd(game, rules, unit, target) else { return false };
    if game.accepts_order(rules, side, &cmd) {
        return execute(game, rules, side, cmd);
    }
    if matches!(cmd, Command::RangedAttack { .. }) && try_cmd(game, rules, side, Command::SetUpRangedAttack { unit }) {
        return try_cmd(game, rules, side, cmd);
    }
    false
}

/// How a script picks its blows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Nerve {
    /// Only a kill, or a blow that deals at least twice what it takes.
    Careful,
    /// A kill, or a blow that deals at least what it takes and does not kill the striker.
    Even,
    /// Any blow the rules accept.
    Reckless,
}

/// The tiles of the units of `foe` (and its cities' tiles when `cities`).
pub fn foe_tiles(game: &GameState, foe: PlayerId, only: Option<&[Piece]>, cities: bool) -> Vec<TileId> {
    let auth = game.view().auth;
    let mut v: Vec<TileId> = match only {
        Some(ps) => ps.iter().filter_map(|p| alive(game, p, foe)).map(|c| c.tile).collect(),
        None => auth.indexes().units_of(foe).iter().filter_map(|h| auth.units().get(*h)).map(|u| u.core.tile).collect(),
    };
    if cities {
        v.extend(auth.indexes().cities_of(foe).iter().filter_map(|c| auth.cities().get(*c)).map(|c| c.tile));
    }
    v.sort_by_key(|t| t.0);
    v.dedup();
    v
}

/// The best blow of `unit` from where it stands on one of `targets`, by the previews: a kill first,
/// then the wounded (least hit points left after the blow), melee targets before ranged ones (MT4:
/// the units that capture), a city last unless `city_first`.
pub fn best_blow(game: &GameState, rules: &CompiledRuleset, side: PlayerId, unit: UnitHandle, targets: &[TileId], nerve: Nerve) -> Option<(i64, TileId)> {
    let me = core_of(game, unit)?;
    if me.moves_left == 0 {
        return None;
    }
    let auth = game.view().auth;
    let reach = range_of(rules, me.kind).unwrap_or(1);
    let mut best: Option<(i64, TileId)> = None;
    for &t in targets {
        if dist(game, me.tile, t) > reach {
            continue;
        }
        let Some(cmd) = attack_cmd(game, rules, unit, t) else { continue };
        let ok = game.accepts_order(rules, side, &cmd) || matches!(cmd, Command::RangedAttack { .. }) && game.accepts_order(rules, side, &Command::SetUpRangedAttack { unit });
        if !ok {
            continue;
        }
        let Some((dealt, taken, hp)) = preview(game, rules, unit, None, t) else { continue };
        let kills = dealt >= hp;
        let city = auth.indexes().city_on_tile(t).is_some();
        let fine = match nerve {
            Nerve::Careful => kills || (dealt >= 2 * taken && taken < i64::from(me.hp) / 2),
            Nerve::Even => kills || (dealt >= taken && taken < i64::from(me.hp)),
            Nerve::Reckless => true,
        };
        if !fine {
            continue;
        }
        let capturer = auth.indexes().units_on(t).iter().filter_map(|h| auth.units().get(*h)).any(|u| range_of(rules, u.core.kind).is_none() && !is_civilian(rules, u.core.kind));
        let score = if kills { 10_000 } else { 0 } + (100 - (hp - dealt).max(0)) * 10 + if capturer { 50 } else { 0 } + if city { 20 } else { 0 } - taken;
        if best.is_none_or(|b| score > b.0) {
            best = Some((score, t));
        }
    }
    best
}

/// The reachable tiles of `unit` this turn with the moves left after the step (its own tile too).
pub fn reachable(game: &GameState, rules: &CompiledRuleset, side: PlayerId, unit: UnitHandle) -> Vec<(TileId, u16)> {
    let mut v = game.player_view(rules, side).own_reachable(unit);
    if let Some(c) = core_of(game, unit) {
        if !v.iter().any(|(t, _)| *t == c.tile) {
            v.push((c.tile, c.moves_left));
        }
    }
    v.sort_by_key(|(t, _)| t.0);
    v
}

/// A tile a unit may end on: no other unit of a military kind there (a civilian may share with one
/// military unit), no city of another player.
pub fn free_for(game: &GameState, rules: &CompiledRuleset, unit: UnitHandle, t: TileId) -> bool {
    let auth = game.view().auth;
    let Some(me) = core_of(game, unit) else { return false };
    if me.tile == t {
        return true;
    }
    let civ = is_civilian(rules, me.kind);
    let others: Vec<UnitCore> = auth.indexes().units_on(t).iter().filter_map(|h| auth.units().get(*h)).map(|u| *u.core).collect();
    if others.iter().any(|o| o.owner != me.owner) {
        return false;
    }
    others.iter().all(|o| is_civilian(rules, o.kind) != civ)
}

/// Moves `unit` to the reachable tile with the lowest `key` (its own tile counts: no move when it is
/// best). Returns whether it moved.
pub fn move_by<K: Ord>(game: &mut GameState, rules: &CompiledRuleset, side: PlayerId, unit: UnitHandle, key: impl Fn(&GameState, TileId, u16) -> K) -> bool {
    let Some(me) = core_of(game, unit) else { return false };
    if me.moves_left == 0 {
        return false;
    }
    let mut opts: Vec<(K, u32, TileId)> = reachable(game, rules, side, unit).into_iter().filter(|(t, _)| free_for(game, rules, unit, *t)).map(|(t, left)| (key(game, t, left), t.0, t)).collect();
    opts.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
    for (_, _, t) in opts {
        if t == me.tile {
            return false;
        }
        if try_cmd(game, rules, side, Command::MoveUnit { unit, destination: t }) {
            return true;
        }
    }
    false
}

/// The distance from `t` to the nearest of `tiles` (99 without).
pub fn nearest(game: &GameState, t: TileId, tiles: &[TileId]) -> i32 {
    tiles.iter().map(|x| dist(game, t, *x)).min().unwrap_or(99)
}

/// Tiles from which a foe could strike `t` next turn: within the foe's move plus its reach.
pub fn threat_on(game: &GameState, rules: &CompiledRuleset, foe: PlayerId, t: TileId) -> i32 {
    let auth = game.view().auth;
    auth.indexes()
        .units_of(foe)
        .iter()
        .filter_map(|h| auth.units().get(*h))
        .filter(|u| !is_civilian(rules, u.core.kind))
        .filter(|u| dist(game, u.core.tile, t) <= moves_of(rules, u.core.kind) + range_of(rules, u.core.kind).unwrap_or(1))
        .count() as i32
}

/// Fortify where the rules let it (else nothing).
pub fn fortify(game: &mut GameState, rules: &CompiledRuleset, side: PlayerId, unit: UnitHandle) -> bool {
    try_cmd(game, rules, side, Command::Fortify { unit })
}

// ---------------------------------------------------------------------------
// The competent tactician the references are made of

/// Where a unit of a script goes when it does not strike.
#[derive(Debug, Clone)]
pub enum Aim {
    /// Stay on (or go to) this tile and fortify.
    Hold(TileId),
    /// Close on the nearest foe.
    Close,
    /// Go to this tile.
    To(TileId),
    /// Keep away: the tile farthest from the foes (toward `home` on a tie).
    Away(TileId),
    /// Keep the foes at the edge of the unit's range, never within their reach (kiting).
    Kite,
    /// Stand on the tile of (or next to) this piece, between it and the foes.
    Guard(UnitHandle),
    /// Ranged support: within range of the nearest foe, behind a friendly melee unit when one is near.
    Support,
}

/// One unit's turn of a script: strike (before and, for hit-and-run units, after moving), else move by
/// the aim, strike from the new tile, fortify when holding.
pub fn unit_turn(game: &mut GameState, rules: &CompiledRuleset, side: PlayerId, foe: PlayerId, unit: UnitHandle, aim: &Aim, nerve: Nerve, targets: &dyn Fn(&GameState) -> Vec<TileId>, friends: &[UnitHandle]) -> u32 {
    let Some(me) = core_of(game, unit) else { return 0 };
    if me.owner != side || me.moves_left == 0 {
        return 0;
    }
    let mut made = 0;
    let ranged = range_of(rules, me.kind).is_some();
    let civ = is_civilian(rules, me.kind);
    if debug() {
        let ts = targets(game);
        let blows: Vec<String> = ts.iter().map(|t| format!("{}:{:?}/{:?}", t.0, preview(game, rules, unit, None, *t), attack_cmd(game, rules, unit, *t).map(|c| game.accepts_order(rules, side, &c)))).collect();
        let reach: Vec<u32> = reachable(game, rules, side, unit).iter().map(|(t, _)| t.0).collect();
        eprintln!("  unit {} kind {} at {} hp {} moves {} aim {:?} targets {} reach {:?}", me.id.0, rules.units[me.kind.0 as usize].name, me.tile.0, me.hp, me.moves_left, aim, blows.join(" "), reach);
    }
    // Strike first from where the unit stands.
    if !civ {
        if let Some((_, t)) = best_blow(game, rules, side, unit, &targets(game), nerve) {
            if strike(game, rules, side, unit, t) {
                made += 1;
            }
        }
    }
    // Move to strike: a tile reached this turn with moves left from which a fine blow lands (the
    // safest such tile; a hit-and-run unit steps back after it).
    if made == 0 && !civ && !matches!(aim, Aim::Hold(_) | Aim::Away(_)) {
        if approach_strike(game, rules, side, foe, unit, &targets(game), nerve) {
            made += 1;
        }
    }
    let Some(me) = core_of(game, unit).filter(|c| c.owner == side) else { return made };
    if me.moves_left > 0 && !(made > 0 && !matches!(aim, Aim::Kite)) {
        let foes = targets(game);
        let reach = range_of(rules, me.kind).unwrap_or(1);
        let moved = match aim {
            Aim::Hold(t) => {
                let t = *t;
                move_by(game, rules, side, unit, |g, x, _| (dist(g, x, t), -defence(g, rules, x, side)))
            }
            Aim::To(t) => {
                let t = *t;
                move_by(game, rules, side, unit, |g, x, _| (dist(g, x, t), threat_on(g, rules, foe, x)))
            }
            Aim::Close => move_by(game, rules, side, unit, |g, x, _| {
                let d = nearest(g, x, &foes);
                // Melee: next to a foe it can strike (a kill or an even blow) if any; else close in.
                (if ranged { (d - reach).abs() } else { (d - 1).max(0) }, -defence(g, rules, x, side))
            }),
            Aim::Away(home) => {
                let home = *home;
                move_by(game, rules, side, unit, |g, x, _| (threat_on(g, rules, foe, x), -nearest(g, x, &foes), dist(g, x, home)))
            }
            Aim::Kite => move_by(game, rules, side, unit, |g, x, _| {
                // Out of every foe's reach first, then as near the edge of our range as that allows.
                let d = nearest(g, x, &foes);
                (threat_on(g, rules, foe, x), (d - reach - 1).abs(), -defence(g, rules, x, side))
            }),
            Aim::Guard(ward) => {
                let ward_tile = core_of(game, *ward).map_or(me.tile, |c| c.tile);
                move_by(game, rules, side, unit, |g, x, _| (dist(g, x, ward_tile), -nearest(g, x, &foes).min(3), -defence(g, rules, x, side)))
            }
            Aim::Support => {
                let melee: Vec<TileId> = friends.iter().filter_map(|h| core_of(game, *h)).filter(|c| c.owner == side && range_of(rules, c.kind).is_none() && !is_civilian(rules, c.kind)).map(|c| c.tile).collect();
                move_by(game, rules, side, unit, |g, x, _| {
                    let d = nearest(g, x, &foes);
                    let in_range = d <= reach;
                    let exposed = d <= 1;
                    let cover = nearest(g, x, &melee) <= 1;
                    (exposed, !in_range, !cover, threat_on(g, rules, foe, x), (d - reach).abs(), -defence(g, rules, x, side))
                })
            }
        };
        if moved {
            made += 1;
        }
        if !civ {
            if let Some((_, t)) = best_blow(game, rules, side, unit, &targets(game), nerve) {
                if strike(game, rules, side, unit, t) {
                    made += 1;
                }
            }
        }
        // Hit and run: a unit with moves after its strike steps back out of reach.
        if let Some(c) = core_of(game, unit).filter(|c| c.owner == side && c.moves_left > 0) {
            if matches!(aim, Aim::Kite) {
                let foes = targets(game);
                if move_by(game, rules, side, unit, |g, x, _| (threat_on(g, rules, foe, x), (nearest(g, x, &foes) - 4).abs(), -defence(g, rules, x, side))) {
                    made += 1;
                }
            } else if let Aim::Hold(t) = aim {
                if c.tile == *t && fortify(game, rules, side, unit) {
                    made += 1;
                }
            }
        }
    }
    stand(game, rules, unit);
    made
}

// ---------------------------------------------------------------------------
// The controls

/// Passive: every scene unit fortifies where it stands (or does nothing), all game long.
pub fn passive_turn(game: &mut GameState, rules: &CompiledRuleset, side: PlayerId, units: &[Piece]) -> u32 {
    let mut made = 0;
    for p in units {
        if alive(game, p, side).is_some() {
            if fortify(game, rules, side, p.h) {
                made += 1;
            }
            stand(game, rules, p.h);
        }
    }
    made
}

/// Random: every scene unit does one thing drawn uniformly among standing, fortifying, a move to a
/// reachable tile and a strike the rules accept (the draw seeded by the instance and the round).
pub fn random_turn(game: &mut GameState, rules: &CompiledRuleset, side: PlayerId, units: &[Piece], d: &Draw) -> u32 {
    let mut made = 0;
    for p in units {
        let Some(me) = alive(game, p, side) else { continue };
        let reach = range_of(rules, me.kind).unwrap_or(1);
        let mut options: Vec<Command> = vec![Command::Fortify { unit: p.h }];
        for (t, _) in reachable(game, rules, side, p.h) {
            if t != me.tile && free_for(game, rules, p.h, t) {
                options.push(Command::MoveUnit { unit: p.h, destination: t });
            }
        }
        let auth = game.view().auth;
        let mut targets: Vec<TileId> = (0..auth.map().tile_count() as u32).map(TileId).filter(|t| dist(game, me.tile, *t) <= reach && *t != me.tile).collect();
        targets.retain(|t| auth.indexes().units_on(*t).iter().filter_map(|h| auth.units().get(*h)).any(|u| u.core.owner != side) || auth.indexes().city_on_tile(*t).and_then(|c| auth.cities().get(c)).is_some_and(|c| c.owner != side));
        for t in targets {
            if let Some(cmd) = attack_cmd(game, rules, p.h, t) {
                if game.accepts_order(rules, side, &cmd) {
                    options.push(cmd);
                }
            }
        }
        let pick = (d.next() % (options.len() as u64 + 1)) as usize;
        if pick < options.len() && try_cmd(game, rules, side, options[pick].clone()) {
            made += 1;
        }
        stand(game, rules, p.h);
    }
    made
}

pub fn debug() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("TRAINERS_DEBUG").is_some())
}

/// Moves `unit` to a reachable tile (moves left there) from which a fine blow lands on one of `targets`,
/// and strikes. The tile: the least threatened, then the best blow. True when it struck.
pub fn approach_strike(game: &mut GameState, rules: &CompiledRuleset, side: PlayerId, foe: PlayerId, unit: UnitHandle, targets: &[TileId], nerve: Nerve) -> bool {
    let Some(me) = core_of(game, unit) else { return false };
    let reach = range_of(rules, me.kind).unwrap_or(1);
    let mut best: Option<((i32, i64, u32), TileId, TileId)> = None;
    for (from, left) in reachable(game, rules, side, unit) {
        if left == 0 || from == me.tile || !free_for(game, rules, unit, from) {
            continue;
        }
        for &t in targets {
            if dist(game, from, t) > reach {
                continue;
            }
            let Some((dealt, taken, hp)) = preview(game, rules, unit, Some(from), t) else { continue };
            let kills = dealt >= hp;
            let fine = match nerve {
                Nerve::Careful => kills || (dealt >= 2 * taken && taken < i64::from(me.hp) / 2),
                Nerve::Even => kills || (dealt >= taken && taken < i64::from(me.hp)),
                Nerve::Reckless => true,
            };
            if !fine {
                continue;
            }
            let score = if kills { 10_000 } else { 0 } + (100 - (hp - dealt).max(0)) * 10 - taken;
            let key = (threat_on(game, rules, foe, from), -score, from.0);
            if best.as_ref().is_none_or(|b| key < b.0) {
                best = Some((key, from, t));
            }
        }
    }
    let Some((_, from, t)) = best else { return false };
    if !try_cmd(game, rules, side, Command::MoveUnit { unit, destination: from }) {
        return false;
    }
    strike(game, rules, side, unit, t)
}
