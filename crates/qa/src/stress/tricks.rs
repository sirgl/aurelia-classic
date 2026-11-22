//! The situations of wave 2: the human tricks (`P*`, work/human-playbook-triage.md) and the rest of
//! the catalogue (work/ai-scenarios.html). Each is a plan (roles and base, both arms) and an edit of
//! one strength, made of the same `edit::Change` primitives as wave 1, plus a scripted adversary
//! (`seat::Script`) where the trick is a plan of play, not a change of the world.

use std::collections::BTreeMap;

use classic_rules::ids::{BuildingKindId, PlayerId, ProductionItem, TileId};
use kernel::rules::content::ResourceClass;
use classic_rules::{ClassicGame, CompiledRuleset, GameState};

use super::edit::{self, Accord, Change, Gift, Yield};
use super::position::{mix, Position};
use super::scene::{city_tiles, invaders, nearest_on_land, nearest_pair, Edit, Plan, Roles};
use super::seat::Script;
use super::Situation;
use crate::battle::{self, Arm as Kind};
use crate::bench::modules::majors;

pub(super) fn distance(game: &GameState, a: TileId, b: TileId) -> i32 {
    game.view().auth.map().dimensions.distance(a, b).unwrap_or(i32::MAX)
}

pub(super) fn at_war(game: &GameState, a: PlayerId, b: PlayerId) -> bool {
    game.view().auth.diplomacy().at_war(a, b)
}

pub(super) fn meet(a: PlayerId, b: PlayerId) -> Change {
    Change::Meet { a: a.0, b: b.0 }
}

pub(super) fn war(by: PlayerId, on: PlayerId) -> Change {
    Change::War { a: by.0, b: on.0 }
}

/// The nearest pair on one landmass, the seat and the other by `flip`.
pub(super) fn seat_and_rival(game: &GameState, flip: bool, info: &mut BTreeMap<String, String>) -> Result<(PlayerId, PlayerId), String> {
    let (a, b, d) = nearest_pair(game).ok_or("no two nations share a landmass")?;
    info.insert("pair_distance".to_owned(), d.to_string());
    Ok(if flip { (b, a) } else { (a, b) })
}

/// The nearest pair at peace on one landmass (`B2`, `B5`, `P3` need a war that starts now).
pub(super) fn seat_and_rival_at_peace(game: &GameState, flip: bool, info: &mut BTreeMap<String, String>) -> Result<(PlayerId, PlayerId), String> {
    let (a, b) = seat_and_rival(game, flip, info)?;
    if at_war(game, a, b) {
        return Err("the nearest pair is at war already".to_owned());
    }
    Ok((a, b))
}

/// The two nations whose cities are nearest of the pairs that share no landmass (`A6`, `C2`).
pub(super) fn pair_over_sea(game: &GameState) -> Option<(PlayerId, PlayerId, i32)> {
    let all = majors(game);
    let mut best: Option<(i32, PlayerId, PlayerId)> = None;
    for a in &all {
        for b in all.iter().filter(|b| b.0 > a.0) {
            let shares = city_tiles(game, *a).into_iter().any(|t| nearest_on_land(game, *b, t).is_some());
            if shares {
                continue;
            }
            let near = city_tiles(game, *a).into_iter().flat_map(|t| city_tiles(game, *b).into_iter().map(move |u| (t, u))).map(|(t, u)| distance(game, t, u)).min();
            if let Some(d) = near.filter(|d| best.is_none_or(|(bd, ..)| *d < bd)) {
                best = Some((d, *a, *b));
            }
        }
    }
    best.map(|(d, a, b)| (a, b, d))
}

/// The nation of the instance: the majors in id order, one picked by the instance's draw.
pub(super) fn picked(game: &GameState, instance: usize) -> Result<PlayerId, String> {
    let all = majors(game);
    if all.is_empty() {
        return Err("no nations".to_owned());
    }
    Ok(all[(mix(instance as u64 ^ 0x5ea7) % all.len() as u64) as usize])
}

/// The living nation with the most of `key` (ties: the lower id).
pub(super) fn top_by(game: &GameState, key: impl Fn(PlayerId) -> i64) -> PlayerId {
    majors(game).into_iter().max_by_key(|p| (key(*p), std::cmp::Reverse(p.0))).unwrap_or(PlayerId(0))
}

/// The roles and the base of a wave-2 situation.
pub(super) fn plan(situation: Situation, rules: &CompiledRuleset, position: &Position, instance: usize) -> Result<Plan, String> {
    let game = &position.game;
    let flip = instance % 2 == 1;
    let mut info = BTreeMap::new();
    let (roles, base) = match situation {
        Situation::P3 => {
            let (seat, enemy) = seat_and_rival_at_peace(game, flip, &mut info)?;
            let base = vec![meet(seat, enemy), Change::Accord { giver: seat.0, receiver: enemy.0, accord: Accord::OpenBorders }];
            (Roles::Sneak { seat: seat.0, enemy: enemy.0, war_after: P3_WAR_AFTER }, base)
        }
        Situation::B2 => {
            let (seat, enemy) = seat_and_rival_at_peace(game, flip, &mut info)?;
            (Roles::Sneak { seat: seat.0, enemy: enemy.0, war_after: 0 }, vec![meet(seat, enemy)])
        }
        Situation::B5 => {
            let (seat, enemy) = seat_and_rival_at_peace(game, flip, &mut info)?;
            let base = vec![meet(seat, enemy), Change::Accord { giver: seat.0, receiver: enemy.0, accord: Accord::Friendship }];
            (Roles::Sneak { seat: seat.0, enemy: enemy.0, war_after: 0 }, base)
        }
        Situation::P2 => {
            let (seat, enemy) = seat_and_rival(game, flip, &mut info)?;
            (Roles::Invasion { seat: seat.0, enemy: enemy.0 }, vec![meet(seat, enemy), war(enemy, seat)])
        }
        Situation::P7 => snipe_plan(game, rules, &mut info)?,
        Situation::P5 => {
            let (seat, grabber) = seat_and_rival_at_peace(game, flip, &mut info)?;
            let tile = forward_tile(game, rules, seat, grabber, position.round)?;
            (Roles::Grab { seat: seat.0, grabber: grabber.0, tile: tile.0 }, vec![meet(seat, grabber)])
        }
        Situation::P1 => {
            let (seat, raider) = seat_and_rival(game, flip, &mut info)?;
            (Roles::Raid { seat: seat.0, raider: raider.0 }, vec![meet(seat, raider), war(raider, seat)])
        }
        Situation::P4 => {
            let (seat, trickster) = seat_and_rival_at_peace(game, flip, &mut info)?;
            let base = vec![meet(seat, trickster), Change::Accord { giver: seat.0, receiver: trickster.0, accord: Accord::Friendship }];
            (Roles::Trick { seat: seat.0, trickster: trickster.0 }, base)
        }
        Situation::P8 => {
            let leader = top_by(game, |p| classic_rules::world::victory::score(game.view().auth, rules, p));
            (Roles::Runaway { leader: leader.0 }, Vec::new())
        }
        Situation::A2 => {
            let leader = top_by(game, |p| edit::military_units(game, rules, p).len() as i64);
            (Roles::Runaway { leader: leader.0 }, Vec::new())
        }
        Situation::A3 => {
            let leader = top_by(game, |p| i64::from(game.ai_observation(rules, p).economy.income.culture));
            (Roles::Runaway { leader: leader.0 }, Vec::new())
        }
        Situation::A4 => {
            let leader = top_by(game, |p| i64::from(game.ai_observation(rules, p).economy.income.gold));
            (Roles::Runaway { leader: leader.0 }, Vec::new())
        }
        Situation::A6 | Situation::C2 => {
            let (a, b, d) = pair_over_sea(game).ok_or("every pair of nations shares a landmass")?;
            info.insert("pair_distance".to_owned(), d.to_string());
            let (weak, near) = if flip { (b, a) } else { (a, b) };
            let base = if situation == Situation::C2 { vec![meet(near, weak), war(near, weak)] } else { Vec::new() };
            (Roles::Neighbour { weak: weak.0, near: near.0 }, base)
        }
        Situation::A7 | Situation::B7 => (Roles::Weak { seat: picked(game, instance)?.0 }, Vec::new()),
        Situation::B6 => {
            let seat = top_by(game, |p| resource_tiles(game, rules, p, ResourceClass::Strategic).len() as i64);
            if resource_tiles(game, rules, seat, ResourceClass::Strategic).is_empty() {
                return Err("no nation holds a strategic resource".to_owned());
            }
            (Roles::Weak { seat: seat.0 }, Vec::new())
        }
        Situation::D2 => {
            let (seat, enemy) = seat_and_rival_at_peace(game, flip, &mut info)?;
            let ally = majors(game)
                .into_iter()
                .filter(|p| *p != seat && *p != enemy && !at_war(game, *p, seat))
                .min_by_key(|p| (city_tiles(game, seat).into_iter().filter_map(|t| nearest_on_land(game, *p, t)).min().unwrap_or(i32::MAX), p.0))
                .ok_or("no third nation for the pact")?;
            let base = vec![meet(seat, enemy), meet(seat, ally), meet(ally, enemy), Change::Accord { giver: seat.0, receiver: ally.0, accord: Accord::DefensivePact }];
            (Roles::Ally { seat: seat.0, ally: ally.0, enemy: enemy.0 }, base)
        }
        Situation::D3 => {
            let (seat, enemy) = seat_and_rival(game, flip, &mut info)?;
            (Roles::Losing { seat: seat.0, enemy: enemy.0 }, vec![meet(seat, enemy), war(enemy, seat)])
        }
        other => return Err(format!("{} is a situation of wave 1", other.name())),
    };
    Ok(Plan { roles, base, info })
}

/// The round of the window the sneak stack declares its war (`P3`): the stack stands at peace
/// for the rounds before it.
const P3_WAR_AFTER: u32 = 4;

/// The edit of a wave-2 situation at `level`.
pub(super) fn edit_of(situation: Situation, level: u8, rules: &CompiledRuleset, position: &Position, plan: &Plan) -> Result<Edit, String> {
    let game = &position.game;
    let salt = mix(position.round as u64 ^ u64::from(level) << 8);
    let l = usize::from(level.min(2));
    let only = |changes: Vec<Change>| -> Result<Edit, String> { Ok(Edit { changes, gift: None, script: None }) };
    match (situation, plan.roles) {
        (Situation::P3, Roles::Sneak { seat, enemy, war_after }) => {
            let mut changes = invaders(game, rules, PlayerId(seat), PlayerId(enemy), [4, 7, 10][l], salt)?;
            changes.push(Change::Later { after: war_after, then: Box::new(war(PlayerId(enemy), PlayerId(seat))) });
            only(changes)
        }
        (Situation::B2 | Situation::B5, Roles::Sneak { seat, enemy, .. }) => {
            let mut changes = at_border(game, rules, PlayerId(seat), PlayerId(enemy), [4, 7, 10][l], salt)?;
            changes.push(war(PlayerId(enemy), PlayerId(seat)));
            only(changes)
        }
        (Situation::P2, Roles::Invasion { seat, enemy }) => only(near_capital(game, rules, PlayerId(seat), PlayerId(enemy), [2, 3, 5][l], salt)?),
        (Situation::P7, Roles::Snipe { sniper, building, .. }) => only(vec![Change::Wonder { player: sniper, building }]),
        (Situation::P5, Roles::Grab { seat, grabber, tile }) => only(grab(game, rules, PlayerId(seat), PlayerId(grabber), TileId(tile), l)),
        (Situation::P1, Roles::Raid { seat, raider }) => {
            let changes = raiders(game, rules, PlayerId(seat), PlayerId(raider), [2, 3, 5][l], salt)?;
            Ok(Edit { changes, gift: None, script: Some(Script::Raid { seat: raider, victim: seat, units: Vec::new() }) })
        }
        (Situation::P4, Roles::Trick { seat, trickster }) => Ok(Edit { changes: Vec::new(), gift: None, script: Some(Script::SellThenWar { seat: trickster, victim: seat, percent: [30, 60, 90][l] }) }),
        (Situation::P8, Roles::Runaway { leader }) => {
            let changes = vec![Change::Treasury { player: leader, gold: [500, 1000, 2000][l] }];
            Ok(Edit { changes, gift: None, script: Some(Script::Sweep { seat: leader, percent: 50 }) })
        }
        (Situation::A2, Roles::Runaway { leader }) => only(army_copies(game, rules, PlayerId(leader), [50, 100, 200][l], salt)),
        (Situation::A3, Roles::Runaway { leader }) => Ok(Edit { changes: Vec::new(), gift: Some(Gift { player: leader, percent: [25, 50, 100][l], yield_kind: Yield::Culture }), script: None }),
        (Situation::A4, Roles::Runaway { leader }) => Ok(Edit { changes: Vec::new(), gift: Some(Gift { player: leader, percent: [50, 100, 200][l], yield_kind: Yield::Gold }), script: None }),
        (Situation::A6 | Situation::C2, Roles::Neighbour { weak, .. }) => only(vec![disarm(game, rules, PlayerId(weak), [50, 75, 100][l], salt)]),
        (Situation::A7, Roles::Weak { seat }) => only(vec![disarm(game, rules, PlayerId(seat), [33, 50, 75][l], salt)]),
        (Situation::B6, Roles::Weak { seat }) => only(vec![deplete(game, rules, PlayerId(seat), ResourceClass::Strategic, [1, 2, usize::MAX][l])]),
        (Situation::B7, Roles::Weak { seat }) => {
            let drain = Change::Treasury { player: seat, gold: i32::MIN };
            only(vec![drain, deplete(game, rules, PlayerId(seat), ResourceClass::Luxury, [1, 2, usize::MAX][l])])
        }
        (Situation::D2, Roles::Ally { seat, enemy, .. }) => only(vec![war(PlayerId(enemy), PlayerId(seat))]),
        (Situation::D3, Roles::Losing { seat, enemy }) => {
            let mut changes = losing_cities(game, rules, PlayerId(seat), PlayerId(enemy), 1 + l)?;
            changes.push(disarm(game, rules, PlayerId(seat), [50, 75, 90][l], salt));
            only(changes)
        }
        (s, roles) => Err(format!("{} has no edit for {roles:?}", s.name())),
    }
}

// ---------------------------------------------------------------------------
// Builders

/// `percent` of the military units of `player` removed (by the draw of each unit's id).
pub(super) fn disarm(game: &GameState, rules: &CompiledRuleset, player: PlayerId, percent: u64, salt: u64) -> Change {
    let ids = edit::military_units(game, rules, player).into_iter().filter(|id| percent >= 100 || mix(*id ^ salt) % 100 < percent).collect();
    Change::RemoveUnits { ids }
}

/// Free tiles a unit can stand on, drawn by `salt`, that `keep` accepts.
pub(super) fn free_tiles(game: &GameState, rules: &CompiledRuleset, salt: u64, keep: impl Fn(TileId) -> bool) -> Vec<TileId> {
    let auth = game.view().auth;
    let mut tiles: Vec<(u64, TileId)> = (0..auth.map().tile_count() as u32)
        .map(TileId)
        .filter(|&t| auth.indexes().units_on(t).is_empty() && auth.indexes().city_on_tile(t).is_none() && battle::standable(rules, game, t) && keep(t))
        .map(|t| (mix(u64::from(t.0) ^ salt), t))
        .collect();
    tiles.sort_unstable();
    tiles.into_iter().map(|(_, t)| t).collect()
}

/// The enemy's melee, ranged, melee, mounted, ... on `tiles`.
pub(super) fn army_on(game: &GameState, rules: &CompiledRuleset, enemy: PlayerId, tiles: &[TileId], n: usize) -> Result<Vec<Change>, String> {
    let kinds = edit::kinds_of(game, rules, enemy);
    let melee = *kinds.get(&Kind::Melee).ok_or("the enemy has no melee unit")?;
    let pick = |arm: Kind| kinds.get(&arm).copied().or_else(|| battle::fallback(arm).and_then(|f| kinds.get(&f).copied())).unwrap_or(melee);
    let order = [Kind::Melee, Kind::Ranged, Kind::Melee, Kind::Mounted];
    if tiles.len() < n {
        return Err(format!("only {} free tiles for {n} units", tiles.len()));
    }
    Ok(tiles.iter().take(n).enumerate().map(|(i, t)| Change::Spawn { player: enemy.0, tile: t.0, kind: pick(order[i % order.len()]).0 }).collect())
}

/// `n` enemy units just outside `seat`'s land, 2 to 4 tiles from its cities on their landmass (an
/// army at the border).
pub(super) fn at_border(game: &GameState, rules: &CompiledRuleset, seat: PlayerId, enemy: PlayerId, n: usize, salt: u64) -> Result<Vec<Change>, String> {
    let map = game.view().auth.map();
    let cities = city_tiles(game, seat);
    let tiles = free_tiles(game, rules, salt, |t| {
        map.owner(t).flatten() != Some(seat) && cities.iter().any(|c| map.land_area(*c) == map.land_area(t) && (2..=4).contains(&distance(game, *c, t)))
    });
    army_on(game, rules, enemy, &tiles, n)
}

/// `n` enemy units 1 to 3 tiles from `seat`'s capital (an early rush).
pub(super) fn near_capital(game: &GameState, rules: &CompiledRuleset, seat: PlayerId, enemy: PlayerId, n: usize, salt: u64) -> Result<Vec<Change>, String> {
    let auth = game.view().auth;
    let capital = classic_rules::combat::city::capital_of(auth, rules, seat).and_then(|c| auth.cities().get(c)).map(|c| c.tile).ok_or("the seat has no capital")?;
    let map = auth.map();
    let tiles = free_tiles(game, rules, salt, |t| map.land_area(t) == map.land_area(capital) && (1..=3).contains(&distance(game, capital, t)));
    army_on(game, rules, enemy, &tiles, n)
}

/// `n` fast units of the raider (its mounted kind, else melee) in the seat's countryside, 2 to 3
/// tiles from its cities.
pub(super) fn raiders(game: &GameState, rules: &CompiledRuleset, seat: PlayerId, raider: PlayerId, n: usize, salt: u64) -> Result<Vec<Change>, String> {
    let kinds = edit::kinds_of(game, rules, raider);
    let kind = kinds.get(&Kind::Mounted).or_else(|| kinds.get(&Kind::Melee)).copied().ok_or("the raider has no land unit")?;
    let map = game.view().auth.map();
    let cities = city_tiles(game, seat);
    let tiles = free_tiles(game, rules, salt, |t| map.owner(t).flatten() == Some(seat) && cities.iter().map(|c| distance(game, *c, t)).min().is_some_and(|d| (2..=3).contains(&d)));
    if tiles.len() < n {
        return Err(format!("only {} free tiles in the seat's land for {n} raiders", tiles.len()));
    }
    Ok(tiles.iter().take(n).map(|t| Change::Spawn { player: raider.0, tile: t.0, kind: kind.0 }).collect())
}

/// Copies of `percent` of the leader's land military units (each copy of a unit's kind) on free
/// tiles of its land 1 to 3 tiles from its cities; as many as the tiles allow.
pub(super) fn army_copies(game: &GameState, rules: &CompiledRuleset, leader: PlayerId, percent: usize, salt: u64) -> Vec<Change> {
    let auth = game.view().auth;
    let kinds: Vec<u16> = edit::military_units(game, rules, leader)
        .into_iter()
        .filter_map(|id| auth.units().handle_of(kernel::ids::EntityId(id)).and_then(|h| auth.units().get(h)).map(|u| u.core.kind))
        .filter(|k| rules.units[k.0 as usize].domain == kernel::rules::content::Domain::Land)
        .map(|k| k.0)
        .collect();
    let wanted = kinds.len() * percent / 100;
    let map = auth.map();
    let cities = city_tiles(game, leader);
    let tiles = free_tiles(game, rules, salt, |t| map.owner(t).flatten() == Some(leader) && cities.iter().map(|c| distance(game, *c, t)).min().is_some_and(|d| (1..=3).contains(&d)));
    (0..wanted).zip(tiles).map(|(i, t)| Change::Spawn { player: leader.0, tile: t.0, kind: kinds[i % kinds.len()] }).collect()
}

/// The tiles of `player`'s land that hold a resource of `class`, by resource.
pub(super) fn resource_tiles(game: &GameState, rules: &CompiledRuleset, player: PlayerId, class: ResourceClass) -> BTreeMap<u16, Vec<u32>> {
    let map = game.view().auth.map();
    let mut by: BTreeMap<u16, Vec<u32>> = BTreeMap::new();
    for t in (0..map.tile_count() as u32).map(TileId) {
        if map.owner(t).flatten() != Some(player) {
            continue;
        }
        if let Some(resource) = map.resource(t).flatten() {
            if rules.resources[resource.0 as usize].class == Some(class) {
                by.entry(resource.0).or_default().push(t.0);
            }
        }
    }
    by
}

/// The resources of `class` in `player`'s land gone: the `kinds` kinds it holds most of.
pub(super) fn deplete(game: &GameState, rules: &CompiledRuleset, player: PlayerId, class: ResourceClass, kinds: usize) -> Change {
    let mut by: Vec<(usize, u16, Vec<u32>)> = resource_tiles(game, rules, player, class).into_iter().map(|(r, t)| (t.len(), r, t)).collect();
    by.sort_by_key(|(n, r, _)| (std::cmp::Reverse(*n), *r));
    Change::Deplete { tiles: by.into_iter().take(kinds).flat_map(|(_, _, t)| t).collect() }
}

/// The cities of `seat` nearest to `enemy`, the capital last, taken by `enemy`.
pub(super) fn losing_cities(game: &GameState, rules: &CompiledRuleset, seat: PlayerId, enemy: PlayerId, count: usize) -> Result<Vec<Change>, String> {
    let auth = game.view().auth;
    let capital = classic_rules::combat::city::capital_of(auth, rules, seat).and_then(|c| auth.cities().get(c)).map(|c| c.id.0);
    let mut cities: Vec<(bool, i32, u64)> = auth
        .indexes()
        .cities_of(seat)
        .iter()
        .filter_map(|h| auth.cities().get(*h))
        .map(|c| (Some(c.id.0) == capital, nearest_on_land(game, enemy, c.tile).unwrap_or(i32::MAX), c.id.0))
        .collect();
    if cities.len() <= count {
        return Err(format!("the seat holds {} cities, the war takes {count}", cities.len()));
    }
    cities.sort_unstable();
    Ok(cities.into_iter().take(count).map(|(_, _, city)| Change::CaptureCity { city, conqueror: enemy.0 }).collect())
}

/// `P7`: the city of any nation whose head order is a world wonder nobody holds, with the most
/// production stocked in it; the sniper is the nation with the most score but the seat.
pub(super) fn snipe_plan(game: &GameState, rules: &CompiledRuleset, info: &mut BTreeMap<String, String>) -> Result<(Roles, Vec<Change>), String> {
    let auth = game.view().auth;
    let built: Vec<BuildingKindId> = auth.cities().iter().flat_map(|(_, c)| c.buildings.iter().copied()).filter(|b| rules.buildings[b.0 as usize].wonder).collect();
    let all = majors(game);
    let best = auth
        .cities()
        .iter()
        .filter(|(_, c)| all.contains(&c.owner))
        .filter_map(|(_, c)| match c.queue.first() {
            Some(ProductionItem::Building(b)) if rules.buildings[b.0 as usize].wonder && !built.contains(b) => Some((c.production, c.id.0, c.owner, *b)),
            _ => None,
        })
        .max_by_key(|(stock, id, ..)| (*stock, std::cmp::Reverse(*id)))
        .ok_or("no city builds a world wonder")?;
    let (stock, city, seat, building) = best;
    let sniper = top_by(game, |p| if p == seat { i64::MIN } else { classic_rules::world::victory::score(auth, rules, p) });
    info.insert("stock".to_owned(), stock.to_string());
    info.insert("wonder".to_owned(), rules.buildings[building.0 as usize].key.clone());
    Ok((Roles::Snipe { seat: seat.0, sniper: sniper.0, city, building: building.0 }, Vec::new()))
}

/// `P5`: the tile of the forward city: free land no nation owns, 3 to 4 tiles from the seat's
/// city nearest to the grabber, on its landmass, nearest to the grabber's cities.
pub(super) fn forward_tile(game: &GameState, rules: &CompiledRuleset, seat: PlayerId, grabber: PlayerId, round: u32) -> Result<TileId, String> {
    let map = game.view().auth.map();
    let grabber_cities = city_tiles(game, grabber);
    let front = city_tiles(game, seat).into_iter().min_by_key(|c| (nearest_on_land(game, grabber, *c).unwrap_or(i32::MAX), c.0)).ok_or("the seat has no city")?;
    let every_city: Vec<TileId> = game.view().auth.cities().iter().map(|(_, c)| c.tile).collect();
    let tiles = free_tiles(game, rules, u64::from(round), |t| {
        map.owner(t).flatten().is_none()
            && map.land_area(t) == map.land_area(front)
            && (3..=4).contains(&distance(game, front, t))
            && every_city.iter().all(|c| distance(game, *c, t) >= 3)
    });
    tiles
        .into_iter()
        .min_by_key(|t| (grabber_cities.iter().map(|c| distance(game, *c, *t)).min().unwrap_or(i32::MAX), t.0))
        .ok_or_else(|| "no free tile for a forward city at the seat's border".to_owned())
}

/// `P5` at `level`: the forward city; with one and two citadels on the seat's tiles next to it
/// (nearest to the seat's city first); the strongest level adds the grabber's war.
pub(super) fn grab(game: &GameState, rules: &CompiledRuleset, seat: PlayerId, grabber: PlayerId, tile: TileId, level: usize) -> Vec<Change> {
    let mut changes = vec![Change::Found { player: grabber.0, tile: tile.0 }];
    let citadel = rules.improvements.iter().position(|d| d.culture_bomb_radius > 0);
    let map = game.view().auth.map();
    let cities = city_tiles(game, seat);
    let mut near: Vec<(i32, u32)> = map
        .dimensions
        .neighbors(tile)
        .into_iter()
        .filter(|t| map.land_area(*t).is_some() && game.view().auth.indexes().city_on_tile(*t).is_none())
        .map(|t| (cities.iter().map(|c| distance(game, *c, t)).min().unwrap_or(i32::MAX), t.0))
        .collect();
    near.sort_unstable();
    if let Some(citadel) = citadel {
        let count = [0, 1, 2][level];
        // Two citadels next to each other take overlapping rings: the second is the next tile but one.
        for (_, t) in near.iter().step_by(2).take(count) {
            changes.push(Change::Improve { player: grabber.0, tile: *t, improvement: citadel as u16 });
        }
    }
    if level == 2 {
        changes.push(war(grabber, seat));
    }
    changes
}
