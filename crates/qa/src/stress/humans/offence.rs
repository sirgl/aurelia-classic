//! The offence: the human tricks turned round. The AI seat judged is the one that COULD play the
//! trick; the situation hands it the opportunity a strong human would make for himself (an army next
//! to an early capital, open borders, raiders beside the rival's workers, great generals on the
//! border, a fresh rival city next door, gold before a congress session) and its own AI plays every
//! seat. No script: the question is whether the AI takes the opportunity (behaviour) and gains by it
//! against the control without it (outcome). `Cast.human` is the rival here.

use std::collections::BTreeMap;

use classic_rules::ids::{PlayerId, TileId, UnitKindId};
use classic_rules::world::player::ClassicPlayers;
use classic_rules::{CompiledRuleset, GameState};

use super::army;
use super::{Cast, Trick};
use crate::bench::modules::majors;
use crate::stress::edit::{Accord, Change};
use crate::stress::position::{mix, Position};
use crate::stress::scene::{city_tiles, nearest_pair, Edit, Plan, Roles};

fn plain(changes: Vec<Change>) -> Edit {
    Edit { changes, gift: None, script: None }
}

fn score(game: &GameState, rules: &CompiledRuleset, p: PlayerId) -> i64 {
    classic_rules::world::victory::score(game.view().auth, rules, p)
}

/// The roles and the base of an offence trick: `seat` (judged) could play it on `rival`.
pub(super) fn plan(trick: Trick, rules: &CompiledRuleset, position: &Position, instance: usize) -> Result<Plan, String> {
    let game = &position.game;
    let mut info = BTreeMap::new();
    let (seat, rival, base) = match trick {
        Trick::OffLobby | Trick::OffPoach => rich_pair(trick, game, rules)?,
        _ => {
            let (a, b, d) = nearest_pair(game).ok_or("no two nations share a landmass")?;
            info.insert("pair_distance".to_owned(), d.to_string());
            let (seat, rival) = if instance % 2 == 1 { (b, a) } else { (a, b) };
            let at_war = game.view().auth.diplomacy().at_war(seat, rival);
            let mut base = vec![Change::Meet { a: seat.0, b: rival.0 }];
            match trick {
                Trick::OffRaid | Trick::OffVulture => base.push(Change::War { a: seat.0, b: rival.0 }),
                _ if at_war => return Err("the nearest pair is at war already".to_owned()),
                Trick::OffSneak => base.push(Change::Accord { giver: rival.0, receiver: seat.0, accord: Accord::OpenBorders }),
                _ => {}
            }
            (seat, rival, base)
        }
    };
    Ok(Plan { roles: Roles::Human(Cast { trick, seat: seat.0, human: rival.0 }), base, info })
}

/// The lobby: the seat is second by score, the rival the leader (the congress founded). The poach:
/// the rival holds the most city-state allies, the seat is the one with the most score of the rest.
fn rich_pair(trick: Trick, game: &GameState, rules: &CompiledRuleset) -> Result<(PlayerId, PlayerId, Vec<Change>), String> {
    let mut by_score = majors(game);
    by_score.sort_by_key(|p| (std::cmp::Reverse(score(game, rules, *p)), p.0));
    if trick == Trick::OffLobby {
        if !game.view().auth.module().congress().founded() {
            return Err("the congress is not founded".to_owned());
        }
        let [leader, second, ..] = by_score.as_slice() else { return Err("fewer than two nations".to_owned()) };
        return Ok((*second, *leader, Vec::new()));
    }
    let auth = game.view().auth;
    let states = auth.module().city_states();
    let allies = |p: PlayerId| (0..auth.players().count() as u8).filter(|c| auth.players().is_city_state(PlayerId(*c)) && states.ally(PlayerId(*c)) == Some(p)).count();
    let rival = by_score.iter().copied().max_by_key(|p| (allies(*p), std::cmp::Reverse(p.0))).filter(|p| allies(*p) > 0).ok_or("no nation has a city-state ally")?;
    let seat = by_score.iter().copied().find(|p| *p != rival).ok_or("no other nation")?;
    Ok((seat, rival, Vec::new()))
}

/// The opportunity at `level`.
pub(super) fn edit_of(cast: Cast, level: usize, rules: &CompiledRuleset, position: &Position) -> Result<Edit, String> {
    let game = &position.game;
    let (seat, rival) = (PlayerId(cast.seat), PlayerId(cast.human));
    let auth = game.view().auth;
    match cast.trick {
        Trick::OffRush => {
            let capital = classic_rules::combat::city::capital_of(auth, rules, rival).and_then(|c| auth.cities().get(c)).map(|c| c.tile).ok_or("the rival has no capital")?;
            army_at_home(game, rules, seat, capital, [(2, 1), (3, 2), (4, 2)][level])
        }
        Trick::OffSneak => {
            let front = crate::battle::front_city(game, seat, rival).ok_or("the rival has no city on the seat's landmass")?;
            army_at_home(game, rules, seat, front, [(2, 2), (3, 4), (5, 5)][level])
        }
        Trick::OffRaid => Ok(plain(raiders(game, rules, seat, rival, [2, 3, 5][level], mix(u64::from(position.round) ^ level as u64))?)),
        Trick::OffCitadel => generals(game, rules, seat, rival, level),
        Trick::OffVulture => vulture(game, rules, seat, rival, level, position.round),
        _ => Ok(plain(vec![Change::Treasury { player: seat.0, gold: [500, 1000, 2000][level] }])),
    }
}

/// `(ranged, melee)` units of the seat at home nearest to `target` (the ranged arm's technology
/// given when it has none: the units a human would have built).
fn army_at_home(game: &GameState, rules: &CompiledRuleset, seat: PlayerId, target: TileId, shape: (usize, usize)) -> Result<Edit, String> {
    let (kinds, techs) = army::army_kinds(game, rules, seat)?;
    let mut changes = Vec::new();
    if !techs.is_empty() {
        changes.push(Change::Discover { player: seat.0, techs });
    }
    changes.extend(super::campaign::raise(game, rules, seat, target, kinds, shape)?);
    Ok(plain(changes))
}

/// `n` fast units of the seat (its mounted kind, else melee) in the rival's countryside, 2 to 3 tiles
/// from its cities, the tiles drawn by `salt`.
fn raiders(game: &GameState, rules: &CompiledRuleset, seat: PlayerId, rival: PlayerId, n: usize, salt: u64) -> Result<Vec<Change>, String> {
    let (kinds, _) = army::army_kinds(game, rules, seat)?;
    let map = game.view().auth.map();
    let cities = city_tiles(game, rival);
    let mut tiles: Vec<(u64, u32)> = (0..map.tile_count() as u32)
        .map(TileId)
        .filter(|t| map.owner(*t).flatten() == Some(rival) && army::free_tile(game, rules, *t))
        .filter(|t| cities.iter().map(|c| army::distance(game, *c, *t)).min().is_some_and(|d| (2..=3).contains(&d)))
        .map(|t| (mix(u64::from(t.0) ^ salt), t.0))
        .collect();
    tiles.sort_unstable();
    if tiles.len() < n {
        return Err(format!("only {} free tiles in the rival's land for {n} raiders", tiles.len()));
    }
    Ok(tiles.into_iter().take(n).map(|(_, t)| Change::Spawn { player: seat.0, tile: t, kind: kinds.mounted.0 }).collect())
}

/// One great general of the seat on its land nearest to the rival's city nearest to it (two at the
/// medium level; the strong level adds the seat's war on the rival).
fn generals(game: &GameState, rules: &CompiledRuleset, seat: PlayerId, rival: PlayerId, level: usize) -> Result<Edit, String> {
    let general = (0..rules.units.len()).find(|i| rules.units[*i].key.ends_with("unit/great-general")).map(|i| UnitKindId(i as u16)).ok_or("the rules hold no great general")?;
    let target = crate::battle::front_city(game, seat, rival).ok_or("the rival has no city on the seat's landmass")?;
    let map = game.view().auth.map();
    let mut tiles: Vec<(i32, u32)> = (0..map.tile_count() as u32)
        .map(TileId)
        .filter(|t| map.owner(*t).flatten() == Some(seat) && army::free_tile(game, rules, *t) && map.land_area(*t) == map.land_area(target))
        .map(|t| (army::distance(game, t, target), t.0))
        .collect();
    tiles.sort_unstable();
    let count = [1, 2, 2][level];
    if tiles.len() < count {
        return Err("no free tile of the seat's land for a general".to_owned());
    }
    let mut changes: Vec<Change> = tiles.into_iter().take(count).map(|(_, t)| Change::Spawn { player: seat.0, tile: t, kind: general.0 }).collect();
    if level == 2 {
        changes.push(Change::War { a: seat.0, b: rival.0 });
    }
    Ok(plain(changes))
}

/// The rival founds a city on free land 3 to 4 tiles from the seat's city nearest to it (at war: the
/// base); the medium and strong levels add two and four units of the seat at home next to it.
fn vulture(game: &GameState, rules: &CompiledRuleset, seat: PlayerId, rival: PlayerId, level: usize, round: u32) -> Result<Edit, String> {
    let front = crate::battle::front_city(game, rival, seat).ok_or("the seat has no city on the rival's landmass")?;
    let map = game.view().auth.map();
    let every_city: Vec<TileId> = game.view().auth.cities().iter().map(|(_, c)| c.tile).collect();
    let rivals = city_tiles(game, rival);
    let tile = (0..map.tile_count() as u32)
        .map(TileId)
        .filter(|t| map.owner(*t).flatten().is_none() && map.land_area(*t) == map.land_area(front) && army::free_tile(game, rules, *t))
        .filter(|t| (3..=4).contains(&army::distance(game, front, *t)) && every_city.iter().all(|c| army::distance(game, *c, *t) >= 3))
        .min_by_key(|t| (rivals.iter().map(|c| army::distance(game, *c, *t)).min().unwrap_or(i32::MAX), mix(u64::from(t.0) ^ u64::from(round))))
        .ok_or("no free tile for the rival's new city")?;
    let mut changes = vec![Change::Found { player: rival.0, tile: tile.0 }];
    if level > 0 {
        let (kinds, _) = army::army_kinds(game, rules, seat)?;
        changes.extend(super::campaign::raise(game, rules, seat, tile, kinds, [(0, 0), (1, 1), (2, 2)][level])?);
    }
    Ok(plain(changes))
}
