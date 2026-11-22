//! The situations of wave 3: the adversary groups the waves before skipped (P6 kill-zone, P9
//! sea-hunter, P10 air-nuke, P11 and P12 pests), the air battle (C1), one vote from a diplomatic
//! victory (B10), the rest of the playbook (P4L, P4B, P4W, P1E, P3D, P7C) and the worst combinations of the
//! random mode (R1-R5). Each is a plan (roles and base, both arms) and an edit of one strength,
//! made of the `edit::Change` primitives, plus a scripted adversary (`seat::Script`) where the
//! trick is a plan of play.

use std::collections::BTreeMap;

use classic_rules::ids::{PlayerId, TechId, TileId, UnitKindId};
use classic_rules::rules::content::UnitAbility;
use classic_rules::{CompiledRuleset, GameState};
use kernel::ai::plan::{role_of, AiRole};
use kernel::rules::content::Domain;

use super::edit::{self, Accord, Change};
use super::position::{mix, Position};
use super::scene::{city_tiles, nearest_on_land, Edit, Plan, Roles};
use super::random::Primitive;
use super::seat::Script;
use super::tricks::{at_war, free_tiles, meet, pair_over_sea, seat_and_rival, seat_and_rival_at_peace, top_by, war};
use super::Situation;
use crate::battle::{self, Arm as Kind};
use crate::bench::modules::majors;

fn distance(game: &GameState, a: TileId, b: TileId) -> i32 {
    game.view().auth.map().dimensions.distance(a, b).unwrap_or(i32::MAX)
}

/// The rounds before the World Leader session (`B10`).
const B10_VOTE_IN: u32 = 3;

/// The roles and the base of a wave-3 situation.
pub(super) fn plan(situation: Situation, rules: &CompiledRuleset, position: &Position, instance: usize) -> Result<Plan, String> {
    let game = &position.game;
    let flip = instance % 2 == 1;
    let mut info = BTreeMap::new();
    let (roles, base) = match situation {
        Situation::P6 => {
            let (seat, defender) = seat_and_rival(game, flip, &mut info)?;
            (Roles::Zone { seat: seat.0, defender: defender.0 }, vec![meet(seat, defender), war(seat, defender)])
        }
        Situation::P9 => hunt_plan(game, rules, flip, &mut info)?,
        Situation::P10 | Situation::C1 => {
            let (seat, enemy) = seat_and_rival(game, flip, &mut info)?;
            let mut base = vec![meet(seat, enemy), war(enemy, seat)];
            base.extend(air_techs(game, rules, &[seat, enemy])?);
            (Roles::Air { seat: seat.0, enemy: enemy.0 }, base)
        }
        Situation::P11 => faith_plan(game, &mut info)?,
        Situation::P12 => {
            let seat = top_by(game, |p| game.view().auth.players().techs_of(p).len() as i64);
            let pest = majors(game)
                .into_iter()
                .filter(|p| *p != seat)
                .min_by_key(|p| (nearest_city(game, seat, *p), p.0))
                .ok_or("no rival for the spies")?;
            (Roles::Pest { seat: seat.0, pest: pest.0 }, vec![meet(seat, pest)])
        }
        Situation::B10 => {
            let leader = top_by(game, |p| classic_rules::world::victory::score(game.view().auth, rules, p));
            (Roles::Runaway { leader: leader.0 }, vec![Change::UnitedNations { host: leader.0, rounds: B10_VOTE_IN }])
        }
        Situation::P4L => {
            let (seat, trickster) = seat_and_rival_at_peace(game, flip, &mut info)?;
            if super::seat::luxury_to_sell(game, rules, trickster, seat).is_none() {
                return Err("the trickster holds no luxury the seat lacks".to_owned());
            }
            let base = vec![meet(seat, trickster), Change::Accord { giver: seat.0, receiver: trickster.0, accord: Accord::Friendship }];
            (Roles::Trick { seat: seat.0, trickster: trickster.0 }, base)
        }
        Situation::P4B => {
            let (seat, trickster) = seat_and_rival_at_peace(game, flip, &mut info)?;
            let third = third_of(game, seat, trickster)?;
            info.insert("third".to_owned(), third.0.to_string());
            let base = vec![meet(seat, trickster), meet(seat, third), meet(trickster, third), Change::Accord { giver: seat.0, receiver: trickster.0, accord: Accord::Friendship }];
            (Roles::Trick { seat: seat.0, trickster: trickster.0 }, base)
        }
        Situation::P1E => {
            let (seat, raider) = seat_and_rival(game, flip, &mut info)?;
            (Roles::Raid { seat: seat.0, raider: raider.0 }, vec![meet(seat, raider), war(raider, seat)])
        }
        Situation::P7C => wonder_city_plan(game, rules, &mut info)?,
        Situation::P3D => {
            let (seat, enemy) = seat_and_rival(game, flip, &mut info)?;
            let mut base = vec![meet(seat, enemy)];
            if !at_war(game, seat, enemy) {
                base.push(war(enemy, seat));
            }
            base.push(Change::Accord { giver: enemy.0, receiver: seat.0, accord: Accord::Peace });
            // The war comes the round the treaty the base made ends.
            let mut world = game.clone();
            edit::apply_all(&mut world, rules, &base)?;
            let until = world.view().auth.module().relations().peace_treaty_until(seat, enemy);
            let war_after = until.saturating_sub(position.round) + 1;
            info.insert("treaty_until".to_owned(), until.to_string());
            (Roles::Sneak { seat: seat.0, enemy: enemy.0, war_after }, base)
        }
        Situation::P4W => {
            let (seat, enemy) = seat_and_rival(game, flip, &mut info)?;
            let mut base = vec![meet(seat, enemy)];
            if !at_war(game, seat, enemy) {
                base.push(war(seat, enemy));
            }
            // The seat is winning: half the enemy's army is gone in both arms.
            base.push(super::tricks::disarm(game, rules, enemy, 50, 0));
            (Roles::Trick { seat: seat.0, trickster: enemy.0 }, base)
        }
        Situation::R1 | Situation::R2 | Situation::R3 | Situation::R4 | Situation::R5 => promoted_plan(game, rules, flip, &mut info)?,
        other => return Err(format!("{} is not a situation of wave 3", other.name())),
    };
    Ok(Plan { roles, base, info })
}

/// The edit of a wave-3 situation at `level`.
pub(super) fn edit_of(situation: Situation, level: u8, rules: &CompiledRuleset, position: &Position, plan: &Plan) -> Result<Edit, String> {
    let game = &position.game;
    let salt = mix(position.round as u64 ^ u64::from(level) << 8);
    let l = usize::from(level.min(2));
    let scripted = |changes: Vec<Change>, script: Script| -> Result<Edit, String> { Ok(Edit { changes, gift: None, script: Some(script) }) };
    match (situation, plan.roles) {
        (Situation::P6, Roles::Zone { seat, defender }) => {
            let changes = kill_zone(game, rules, PlayerId(seat), PlayerId(defender), [3, 5, 8][l])?;
            scripted(changes, Script::KillZone { seat: defender, enemy: seat, units: Vec::new() })
        }
        (Situation::P9, Roles::Hunt { seat, hunter }) => {
            let changes = ships(game, rules, plan, PlayerId(hunter), [1, 2, 4][l], salt)?;
            scripted(changes, Script::SeaHunt { seat: hunter, victim: seat, units: Vec::new() })
        }
        (Situation::P10, Roles::Air { seat, enemy }) => {
            let wings = [(2, 0, 0), (4, 2, 0), (4, 2, 1)][l];
            let changes = aircraft(game, rules, plan, PlayerId(enemy), PlayerId(seat), wings)?;
            scripted(changes, Script::AirStrike { seat: enemy, victim: seat, units: Vec::new() })
        }
        (Situation::C1, Roles::Air { seat, enemy }) => {
            let each = [1, 2, 3][l];
            let mut changes = aircraft(game, rules, plan, PlayerId(enemy), PlayerId(seat), (each, each, 0))?;
            changes.extend(aircraft(game, rules, plan, PlayerId(seat), PlayerId(enemy), (each, each, 0))?);
            Ok(Edit { changes, gift: None, script: None })
        }
        (Situation::P11, Roles::Pest { seat, pest }) => {
            let changes = missionaries(game, rules, PlayerId(seat), PlayerId(pest), [2, 4, 6][l], l == 2)?;
            scripted(changes, Script::Faith { seat: pest, victim: seat, units: Vec::new() })
        }
        (Situation::P12, Roles::Pest { seat, pest }) => scripted(vec![Change::Spies { player: pest, count: [1, 2, 3][l] }], Script::Spies { seat: pest, victim: seat }),
        (Situation::B10, Roles::Runaway { leader }) => Ok(Edit { changes: vec![Change::ShortOfVictory { player: leader, short: [3, 2, 1][l] }], gift: None, script: None }),
        (Situation::P4L, Roles::Trick { seat, trickster }) => scripted(Vec::new(), Script::LuxuryThenWar { seat: trickster, victim: seat, percent: [30, 60, 90][l] }),
        (Situation::P4B, Roles::Trick { seat, trickster }) => {
            let third: u8 = plan.info.get("third").and_then(|t| t.parse().ok()).ok_or("the plan names no third nation")?;
            let gold = [300, 600, 1000][l];
            scripted(vec![Change::Treasury { player: trickster, gold: gold as i32 }], Script::BribeThenWar { seat: trickster, victim: seat, third, gold })
        }
        (Situation::P1E, Roles::Raid { seat, raider }) => {
            let changes = super::tricks::raiders(game, rules, PlayerId(seat), PlayerId(raider), [1, 2, 3][l], salt)?;
            scripted(changes, Script::Raid { seat: raider, victim: seat, units: Vec::new() })
        }
        (Situation::P3D, Roles::Sneak { seat, enemy, war_after }) => {
            let mut changes = super::tricks::at_border(game, rules, PlayerId(seat), PlayerId(enemy), [4, 7, 10][l], salt)?;
            changes.push(Change::Later { after: war_after, then: Box::new(war(PlayerId(enemy), PlayerId(seat))) });
            Ok(Edit { changes, gift: None, script: None })
        }
        (Situation::P4W, Roles::Trick { seat, trickster }) => scripted(Vec::new(), Script::PeaceFishing { seat: trickster, victim: seat, every: [3, 2, 1][l] }),
        (Situation::P7C, Roles::Lost { taker, capital, .. }) => Ok(Edit { changes: vec![Change::CaptureCity { city: capital, conqueror: taker }], gift: None, script: None }),
        (Situation::R1 | Situation::R2 | Situation::R3 | Situation::R4 | Situation::R5, _) => promoted_edit(situation, l, rules, position, plan, salt),
        (s, roles) => Err(format!("{} has no edit for {roles:?}", s.name())),
    }
}

// ---------------------------------------------------------------------------
// Reading the position

/// The distance between the nearest cities of the two (any landmass).
fn nearest_city(game: &GameState, a: PlayerId, b: PlayerId) -> i32 {
    let theirs = city_tiles(game, b);
    city_tiles(game, a).into_iter().flat_map(|t| theirs.iter().map(move |u| (t, *u))).map(|(t, u)| distance(game, t, u)).min().unwrap_or(i32::MAX)
}

/// A third nation for the bribe (`P4B`): at peace with the seat, the nearest to it but the trickster.
fn third_of(game: &GameState, seat: PlayerId, trickster: PlayerId) -> Result<PlayerId, String> {
    majors(game)
        .into_iter()
        .filter(|p| *p != seat && *p != trickster && !at_war(game, *p, seat) && !at_war(game, *p, trickster))
        .min_by_key(|p| (nearest_city(game, seat, *p), p.0))
        .ok_or_else(|| "no third nation at peace with both".to_owned())
}

/// The technologies the nations need for aircraft (the first technology that unlocks an air unit
/// with a ranged strength, and its prerequisites), each nation's missing ones in the order of the
/// tree.
fn air_techs(game: &GameState, rules: &CompiledRuleset, nations: &[PlayerId]) -> Result<Vec<Change>, String> {
    let flight = (0..rules.units.len())
        .map(|i| &rules.units[i])
        .filter(|d| d.domain == Domain::Air && d.ranged_strength > 0)
        .filter_map(|d| d.required_tech)
        .min_by_key(|t| (rules.technologies[t.0 as usize].era, t.0))
        .ok_or("the rules hold no aircraft")?;
    let mut changes = Vec::new();
    for nation in nations {
        let known = game.view().auth.players().techs_of(*nation).to_vec();
        let techs = closure(rules, flight, &known);
        if !techs.is_empty() {
            changes.push(Change::Discover { player: nation.0, techs });
        }
    }
    Ok(changes)
}

/// `tech` and its unknown prerequisites, prerequisites first.
fn closure(rules: &CompiledRuleset, tech: TechId, known: &[TechId]) -> Vec<u16> {
    let mut wanted: Vec<TechId> = Vec::new();
    let mut stack = vec![tech];
    while let Some(t) = stack.pop() {
        if known.contains(&t) || wanted.contains(&t) {
            continue;
        }
        wanted.push(t);
        stack.extend(rules.technologies[t.0 as usize].requires.iter().copied());
    }
    wanted.sort_by_key(|t| (rules.technologies[t.0 as usize].era, t.0));
    wanted.into_iter().map(|t| t.0).collect()
}

/// `P9`: the seat and the hunter (the nearest pair over the sea, else the nearest pair), the war,
/// and the seat's army at sea by its coastal city nearest to the hunter (both arms).
fn hunt_plan(game: &GameState, rules: &CompiledRuleset, flip: bool, info: &mut BTreeMap<String, String>) -> Result<(Roles, Vec<Change>), String> {
    let (a, b) = match pair_over_sea(game) {
        Some((a, b, _)) => (a, b),
        None => seat_and_rival(game, false, info)?,
    };
    let (seat, hunter) = if flip { (b, a) } else { (a, b) };
    let map = game.view().auth.map();
    let hunters = city_tiles(game, hunter);
    let to_hunter = |t: TileId| hunters.iter().map(|c| distance(game, *c, t)).min().unwrap_or(i32::MAX);
    let coast = city_tiles(game, seat)
        .into_iter()
        .filter(|c| map.dimensions.neighbors(*c).into_iter().any(|t| map.is_water(t)))
        .min_by_key(|c| (to_hunter(*c), c.0))
        .ok_or("the seat has no coastal city")?;
    let mut water: Vec<(i32, u32)> = (0..map.tile_count() as u32)
        .map(TileId)
        .filter(|t| map.is_water(*t) && (1..=2).contains(&distance(game, coast, *t)) && game.view().auth.indexes().units_on(*t).is_empty())
        .map(|t| (to_hunter(t), t.0))
        .collect();
    water.sort_unstable();
    let kinds = edit::kinds_of(game, rules, seat);
    let melee = *kinds.get(&Kind::Melee).ok_or("the seat has no melee unit")?;
    let order = [Kind::Melee, Kind::Ranged, Kind::Melee, Kind::Mounted];
    if water.len() < order.len() {
        return Err(format!("only {} free water tiles by the seat's coast", water.len()));
    }
    let mut base = vec![meet(seat, hunter), war(hunter, seat)];
    for (arm, (_, tile)) in order.iter().zip(&water) {
        let kind = kinds.get(arm).copied().unwrap_or(melee);
        base.push(Change::Spawn { player: seat.0, tile: *tile, kind: kind.0 });
    }
    info.insert("coast".to_owned(), coast.0.to_string());
    Ok((Roles::Hunt { seat: seat.0, hunter: hunter.0 }, base))
}

/// `P11`: the pest is a nation with a religion of its own; the seat the nation with the most cities
/// that do not follow it (ties: the nearest, then lower ids).
fn faith_plan(game: &GameState, info: &mut BTreeMap<String, String>) -> Result<(Roles, Vec<Change>), String> {
    let auth = game.view().auth;
    let all = majors(game);
    let mut best: Option<(usize, i32, PlayerId, PlayerId)> = None;
    for pest in all.iter().copied() {
        let Some(religion) = auth.module().religion().player(pest).religion else { continue };
        let follows = |h: &classic_rules::ids::CityHandle| classic_rules::derived::religion::city_majority(auth, *h) == Some(classic_rules::world::religion::Creed::Religion(religion));
        for seat in all.iter().copied().filter(|s| *s != pest) {
            let open = auth.indexes().cities_of(seat).iter().filter(|h| !follows(h)).count();
            let d = nearest_city(game, pest, seat);
            if open > 0 && best.is_none_or(|(bo, bd, ..)| (open, -d) > (bo, -bd)) {
                best = Some((open, d, pest, seat));
            }
        }
    }
    let (open, d, pest, seat) = best.ok_or("no nation has a religion that some other nation's cities do not follow")?;
    info.insert("pair_distance".to_owned(), d.to_string());
    info.insert("cities_not_following".to_owned(), open.to_string());
    Ok((Roles::Pest { seat: seat.0, pest: pest.0 }, vec![meet(seat, pest)]))
}

/// `P7C`: the city with the most world wonders (ties: the lower id) and the nation nearest to it on
/// its landmass; the base is their war.
fn wonder_city_plan(game: &GameState, rules: &CompiledRuleset, info: &mut BTreeMap<String, String>) -> Result<(Roles, Vec<Change>), String> {
    let auth = game.view().auth;
    let all = majors(game);
    let (count, city, victim, tile) = auth
        .cities()
        .iter()
        .filter(|(_, c)| all.contains(&c.owner))
        .map(|(_, c)| (c.buildings.iter().filter(|b| rules.buildings[b.0 as usize].wonder).count(), c.id.0, c.owner, c.tile))
        .filter(|(n, ..)| *n > 0)
        .max_by_key(|(n, id, ..)| (*n, std::cmp::Reverse(*id)))
        .ok_or("no city holds a world wonder")?;
    let taker = all
        .into_iter()
        .filter(|p| *p != victim)
        .filter_map(|p| nearest_on_land(game, p, tile).map(|d| (d, p)))
        .min_by_key(|(d, p)| (*d, p.0))
        .map(|(_, p)| p)
        .ok_or("no nation on the wonder city's landmass")?;
    info.insert("wonders".to_owned(), count.to_string());
    Ok((Roles::Lost { victim: victim.0, taker: taker.0, capital: city }, vec![meet(victim, taker), war(taker, victim)]))
}

// ---------------------------------------------------------------------------
// Unit kinds from the rules' data

/// The kinds `player` may field by its technologies and nation (placed, not trained: resources and
/// buildings are not asked) that `keep` accepts.
fn fieldable(game: &GameState, rules: &CompiledRuleset, player: PlayerId, keep: impl Fn(UnitKindId) -> bool) -> Vec<UnitKindId> {
    let auth = game.view().auth;
    let known = auth.players().techs_of(player);
    let nation = auth.players().nation_of(player);
    (0..rules.units.len() as u16)
        .map(UnitKindId)
        .filter(|&kind| {
            let def = &rules.units[kind.0 as usize];
            !def.unbuildable
                && def.required_tech.is_none_or(|t| known.contains(&t))
                && def.obsolete_tech.is_none_or(|t| !known.contains(&t))
                && def.nation.is_none_or(|n| n == nation)
                && keep(kind)
        })
        .collect()
}

/// The strongest of `kinds` by its ranged strength, else its strength (ties: the dearer, then the
/// lower id).
fn strongest(rules: &CompiledRuleset, kinds: &[UnitKindId]) -> Option<UnitKindId> {
    kinds.iter().copied().max_by_key(|k| {
        let d = &rules.units[k.0 as usize];
        (d.ranged_strength.max(d.strength), d.cost, std::cmp::Reverse(k.0))
    })
}

/// The strongest warship `player` may field, else the cheapest warship of the rules.
fn warship(game: &GameState, rules: &CompiledRuleset, player: PlayerId) -> Option<UnitKindId> {
    let naval = |k: UnitKindId| rules.units[k.0 as usize].domain == Domain::Sea && rules.units[k.0 as usize].strength.max(rules.units[k.0 as usize].ranged_strength) > 0;
    let own = fieldable(game, rules, player, naval);
    if let Some(best) = strongest(rules, &own) {
        return Some(best);
    }
    (0..rules.units.len() as u16).map(UnitKindId).filter(|k| naval(*k) && !rules.units[k.0 as usize].unbuildable).min_by_key(|k| (rules.units[k.0 as usize].cost, k.0))
}

/// The bomber, the fighter and the nuclear weapon of `player`: its strongest aircraft of each wing
/// by its technologies (the nuclear weapon: the cheapest of the rules).
fn wings(game: &GameState, rules: &CompiledRuleset, player: PlayerId) -> (Option<UnitKindId>, Option<UnitKindId>, Option<UnitKindId>) {
    let air = |k: UnitKindId| rules.units[k.0 as usize].domain == Domain::Air;
    let combat = |k: UnitKindId| rules.units[k.0 as usize].combat;
    let bombers = fieldable(game, rules, player, |k| air(k) && combat(k).nuke_level == 0 && combat(k).air_intercept_range == 0 && !combat(k).suicide);
    let fighters = fieldable(game, rules, player, |k| air(k) && combat(k).air_intercept_range > 0);
    let nuke = (0..rules.units.len() as u16).map(UnitKindId).filter(|k| air(*k) && combat(*k).nuke_level > 0).min_by_key(|k| (rules.units[k.0 as usize].cost, k.0));
    (strongest(rules, &bombers), strongest(rules, &fighters), nuke)
}

/// The cheapest unit kind with `ability` (a missionary, a prophet).
fn with_ability(rules: &CompiledRuleset, ability: UnitAbility, without: Option<UnitAbility>) -> Option<UnitKindId> {
    (0..rules.units.len() as u16)
        .map(UnitKindId)
        .filter(|k| classic_rules::derived::religion::has_ability(rules, *k, ability) && without.is_none_or(|w| !classic_rules::derived::religion::has_ability(rules, *k, w)))
        .min_by_key(|k| (rules.units[k.0 as usize].cost, k.0))
}

/// The cheapest worker kind of the rules (the bait of the kill-zone).
fn worker_kind(rules: &CompiledRuleset) -> Option<UnitKindId> {
    (0..rules.units.len() as u16).map(UnitKindId).filter(|k| role_of(rules, *k) == AiRole::Worker && rules.units[k.0 as usize].domain == Domain::Land).min_by_key(|k| (rules.units[k.0 as usize].cost, k.0))
}

// ---------------------------------------------------------------------------
// Builders

/// `P6`: `n` units of the defender's own kinds on the free tiles 1-2 from its city nearest to the
/// seat, the melee third on the tiles nearest to the seat and the ranged behind, and a worker (the
/// bait) three tiles out, towards the seat. The bait is spawned last.
fn kill_zone(game: &GameState, rules: &CompiledRuleset, seat: PlayerId, defender: PlayerId, n: usize) -> Result<Vec<Change>, String> {
    let front = battle::front_city(game, seat, defender).ok_or("the defender has no city on the seat's landmass")?;
    let seats = city_tiles(game, seat);
    let to_seat = |t: TileId| seats.iter().map(|c| distance(game, *c, t)).min().unwrap_or(i32::MAX);
    let map = game.view().auth.map();
    let mut posts: Vec<TileId> = free_tiles(game, rules, 0, |t| map.land_area(t) == map.land_area(front) && (1..=2).contains(&distance(game, front, t)));
    posts.sort_by_key(|t| (to_seat(*t), t.0));
    if posts.len() < n {
        return Err(format!("only {} free tiles by the defender's front city for {n} units", posts.len()));
    }
    let kinds = edit::kinds_of(game, rules, defender);
    let melee = *kinds.get(&Kind::Melee).ok_or("the defender has no melee unit")?;
    let ranged = kinds.get(&Kind::Ranged).or_else(|| kinds.get(&Kind::Siege)).copied().unwrap_or(melee);
    let line = n.div_ceil(3);
    let mut changes: Vec<Change> = posts.iter().take(n).enumerate().map(|(i, t)| Change::Spawn { player: defender.0, tile: t.0, kind: if i < line { melee.0 } else { ranged.0 } }).collect();
    let bait_tile = free_tiles(game, rules, 0, |t| map.land_area(t) == map.land_area(front) && distance(game, front, t) == 3 && to_seat(t) < to_seat(front))
        .into_iter()
        .min_by_key(|t| (to_seat(*t), t.0));
    if let (Some(tile), Some(worker)) = (bait_tile, worker_kind(rules)) {
        changes.push(Change::Spawn { player: defender.0, tile: tile.0, kind: worker.0 });
    }
    Ok(changes)
}

/// `P9`: `n` warships of the hunter on the free water 2-4 tiles from the seat's army at sea, on
/// its water, drawn by `salt`.
fn ships(game: &GameState, rules: &CompiledRuleset, plan: &Plan, hunter: PlayerId, n: usize, salt: u64) -> Result<Vec<Change>, String> {
    let army: Vec<TileId> = plan.base.iter().filter_map(|c| if let Change::Spawn { tile, .. } = c { Some(TileId(*tile)) } else { None }).collect();
    let first = *army.first().ok_or("the plan holds no army at sea")?;
    let kind = warship(game, rules, hunter).ok_or("the rules hold no warship")?;
    let map = game.view().auth.map();
    let mut water: Vec<(u64, TileId)> = (0..map.tile_count() as u32)
        .map(TileId)
        .filter(|t| map.is_water(*t) && map.water_area(*t) == map.water_area(first) && !army.contains(t) && game.view().auth.indexes().units_on(*t).is_empty())
        .filter(|t| army.iter().map(|a| distance(game, *a, *t)).min().is_some_and(|d| (2..=4).contains(&d)))
        .map(|t| (mix(u64::from(t.0) ^ salt), t))
        .collect();
    water.sort_unstable();
    if water.len() < n {
        return Err(format!("only {} free water tiles near the army for {n} ships", water.len()));
    }
    Ok(water.into_iter().take(n).map(|(_, t)| Change::Spawn { player: hunter.0, tile: t.0, kind: kind.0 }).collect())
}

/// `P10`, `C1`: (bombers, fighters, nuclear weapons) of `owner` in its city nearest to `target`.
fn aircraft(game: &GameState, rules: &CompiledRuleset, plan: &Plan, owner: PlayerId, target: PlayerId, (bombers, fighters, nukes): (usize, usize, usize)) -> Result<Vec<Change>, String> {
    // The base's technologies make the aircraft fieldable: read the kinds on the world with them.
    let mut world = game.clone();
    edit::apply_all(&mut world, rules, &plan.base)?;
    let (bomber, fighter, nuke) = wings(&world, rules, owner);
    let targets = city_tiles(game, target);
    let base = city_tiles(game, owner).into_iter().min_by_key(|c| (targets.iter().map(|t| distance(game, *c, *t)).min().unwrap_or(i32::MAX), c.0)).ok_or("the owner has no city")?;
    let mut changes = Vec::new();
    for (kind, count) in [(bomber, bombers), (fighter, fighters), (nuke, nukes)] {
        if count == 0 {
            continue;
        }
        let kind = kind.ok_or("the owner may field no aircraft of a wing the level needs")?;
        changes.extend((0..count).map(|_| Change::Spawn { player: owner.0, tile: base.0, kind: kind.0 }));
    }
    Ok(changes)
}

/// `P11`: `n` missionaries of the pest (and its great prophet with `prophet`) on free land next to
/// the seat's cities, those that do not follow the pest's religion first, one unit a city in turn.
fn missionaries(game: &GameState, rules: &CompiledRuleset, seat: PlayerId, pest: PlayerId, n: usize, prophet: bool) -> Result<Vec<Change>, String> {
    let missionary = with_ability(rules, UnitAbility::SpreadReligion, Some(UnitAbility::FoundReligion)).ok_or("the rules hold no missionary")?;
    let great = with_ability(rules, UnitAbility::FoundReligion, None);
    let pests = city_tiles(game, pest);
    let auth = game.view().auth;
    let religion = auth.module().religion().player(pest).religion;
    let follows = |c: TileId| {
        let majority = auth.indexes().city_on_tile(c).and_then(|h| classic_rules::derived::religion::city_majority(auth, h));
        religion.is_some_and(|r| majority == Some(classic_rules::world::religion::Creed::Religion(r)))
    };
    let mut cities = city_tiles(game, seat);
    // The cities that do not follow the pest's religion first, the nearest to the pest first.
    cities.sort_by_key(|c| (follows(*c), pests.iter().map(|p| distance(game, *c, *p)).min().unwrap_or(i32::MAX), c.0));
    let mut kinds = vec![missionary; n];
    if let (true, Some(g)) = (prophet, great) {
        kinds.push(g);
    }
    let mut taken: Vec<TileId> = Vec::new();
    let mut changes = Vec::new();
    for (i, kind) in kinds.into_iter().enumerate() {
        let city = cities.get(i % cities.len().max(1)).copied().ok_or("the seat has no city")?;
        let tile = free_tiles(game, rules, i as u64, |t| distance(game, city, t) == 1 && !taken.contains(&t)).into_iter().next().ok_or("no free tile next to the seat's city")?;
        taken.push(tile);
        changes.push(Change::Spawn { player: pest.0, tile: tile.0, kind: kind.0 });
    }
    Ok(changes)
}

// ---------------------------------------------------------------------------
// R1-R5: the worst combinations of the random mode, named

/// The parties of a promoted combination: the seat (the instance's draw), its two nearest rivals and
/// the rival with the most score of the others (the one a gift makes a runaway).
struct Parties {
    seat: PlayerId,
    near: PlayerId,
    second: PlayerId,
    leader: PlayerId,
}

fn parties(game: &GameState, rules: &CompiledRuleset, flip: bool) -> Result<Parties, String> {
    let all = majors(game);
    let (a, b) = seat_and_rival(game, false, &mut BTreeMap::new())?;
    let seat = if flip { b } else { a };
    let mut rivals: Vec<PlayerId> = all.iter().copied().filter(|p| *p != seat).collect();
    rivals.sort_by_key(|p| (nearest_city(game, seat, *p), p.0));
    let (near, second) = match rivals.as_slice() {
        [n, s, ..] => (*n, *s),
        _ => return Err("fewer than three nations".to_owned()),
    };
    let leader = top_by(game, |p| if p == seat || p == near || p == second { i64::MIN } else { classic_rules::world::victory::score(game.view().auth, rules, p) });
    Ok(Parties { seat, near, second, leader: if leader == seat { second } else { leader } })
}

fn promoted_plan(game: &GameState, rules: &CompiledRuleset, flip: bool, info: &mut BTreeMap<String, String>) -> Result<(Roles, Vec<Change>), String> {
    let p = parties(game, rules, flip)?;
    info.insert("parties".to_owned(), format!("{} {} {}", p.near.0, p.second.0, p.leader.0));
    Ok((Roles::Seat { seat: p.seat.0 }, Vec::new()))
}

/// The primitives of a promoted combination at level `l` (`random::Primitive`), the parties fixed
/// by rule: the seat's nearest rival takes and grabs, the second declares war (and takes in R2), the
/// leader of the others gets the gift.
fn promoted_primitives(situation: Situation, l: usize, p: &Parties) -> Vec<Primitive> {
    let seize = |taker: PlayerId, cities: usize| Primitive::Seize { taker, cities };
    match situation {
        // deplete+riches+seize
        Situation::R1 => vec![Primitive::Deplete, seize(p.near, [1, 2, 3][l]), Primitive::Riches { rival: p.leader, percent: [50, 100, 200][l] }],
        // seize+seize: two fronts
        Situation::R2 => vec![seize(p.near, [1, 1, 2][l]), seize(p.second, [1, 2, 3][l])],
        // boost+grab+war
        Situation::R3 => vec![Primitive::Grab { rival: p.near }, Primitive::Boost { nation: p.leader, percent: [25, 50, 100][l] }, Primitive::War { enemy: p.second }],
        // bankrupt+seize+war
        Situation::R4 => vec![Primitive::Bankrupt, Primitive::War { enemy: p.second }, seize(p.near, [1, 2, 3][l])],
        // bankrupt+deplete+seize
        _ => vec![Primitive::Bankrupt, Primitive::Deplete, seize(p.near, [1, 2, 3][l])],
    }
}

fn promoted_edit(situation: Situation, l: usize, rules: &CompiledRuleset, position: &Position, plan: &Plan, salt: u64) -> Result<Edit, String> {
    let Roles::Seat { seat } = plan.roles else { return Err("a promoted combination judges one seat".to_owned()) };
    let numbers: Vec<u8> = plan.info.get("parties").map(|t| t.split(' ').filter_map(|x| x.parse().ok()).collect()).unwrap_or_default();
    let [near, second, leader] = numbers[..] else { return Err("the plan names no parties".to_owned()) };
    let p = Parties { seat: PlayerId(seat), near: PlayerId(near), second: PlayerId(second), leader: PlayerId(leader) };
    let mut edit = Edit::default();
    for primitive in promoted_primitives(situation, l, &p) {
        super::random::add(rules, position, p.seat, primitive, salt, &mut edit)?;
    }
    Ok(edit)
}
