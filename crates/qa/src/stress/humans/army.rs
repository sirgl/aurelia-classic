//! The moves of a human army, shared by the scripts that fight (`campaign`, `bait`, `squat`). Every
//! order goes through `GameState::execute` (the rules validate it, as they validate a human's click);
//! a refused order is a step the unit does not make. At the end of its part of the turn a script holds
//! every unit it plays (`hold`): the seat's AI plays the rest of the seat and never moves them.
//!
//! The play is a strong human's simple one: ranged units shoot the city from their range and never
//! stand next to it, melee units close in only when the city is low and take it, wounded units step
//! back and rest, nothing attacks a unit that is stronger than itself.

use classic_rules::ids::{CityHandle, PlayerId, TileId, UnitHandle, UnitKindId};
use classic_rules::world::relations::{DealItem, PeaceRefusal};
use classic_rules::{ClassicGame, Command, CompiledRuleset, GameState};
use kernel::ai::plan::{role_of, AiRole};
use kernel::ids::EntityId;

use crate::battle::{self, Arm};

/// Orders one script may send in one turn (a guard against a plan that loops).
pub const BUDGET: usize = 240;

/// A unit below this share of its hit points steps back and rests.
const WOUNDED_PERCENT: u16 = 40;

/// The melee units close in on a city at or below this share of its hit points.
const CLOSE_IN_PERCENT: u16 = 50;

/// A unit of the script's army as the world holds it now.
#[derive(Debug, Clone, Copy)]
pub struct Soldier {
    pub id: u64,
    pub handle: UnitHandle,
    pub tile: TileId,
    pub hp: u16,
    pub moves: u16,
    /// The range of a ranged unit; none for a unit that fights in melee.
    pub range: Option<i32>,
}

impl Soldier {
    fn wounded(&self) -> bool {
        self.hp * 100 < WOUNDED_PERCENT * kernel::world::unit::UNIT_MAX_HP
    }
}

/// What the army did in one turn, added up by the script.
#[derive(Debug, Clone, Copy, Default)]
pub struct Tally {
    pub shots: u32,
    pub kills: u32,
    pub captured: u32,
}

impl Tally {
    pub fn add(&mut self, other: Tally) {
        self.shots += other.shots;
        self.kills += other.kills;
        self.captured += other.captured;
    }
}

pub fn distance(game: &GameState, a: TileId, b: TileId) -> i32 {
    game.view().auth.map().dimensions.distance(a, b).unwrap_or(i32::MAX)
}

/// The living units of `ids` that `owner` still holds.
pub fn soldiers(game: &GameState, rules: &CompiledRuleset, owner: PlayerId, ids: &[u64]) -> Vec<Soldier> {
    let auth = game.view().auth;
    let mut out = Vec::new();
    for id in ids {
        let Some(handle) = auth.units().handle_of(EntityId(*id)) else { continue };
        let Some(unit) = auth.units().get(handle) else { continue };
        if unit.core.owner != owner {
            continue;
        }
        let range = classic_rules::ai::combat::ranged_range(rules, unit.core.kind);
        out.push(Soldier { id: *id, handle, tile: unit.core.tile, hp: unit.core.hp, moves: unit.core.moves_left, range });
    }
    out
}

/// The unit stands for the rest of the turn (fortified when it did not move): the seat's AI does not
/// move it, and a unit that did not move heals as a resting unit heals.
pub fn hold(game: &mut GameState, rules: &CompiledRuleset, unit: UnitHandle) {
    let unmoved = game.view().auth.units().get(unit).is_some_and(|u| !u.core.has_moved() && !u.core.stance.is_fortified());
    if unmoved {
        let _ = game.execute(rules, Command::Fortify { unit });
    }
    game.debug_hold_unit(rules, unit);
}

/// Holds every unit of `ids` that `owner` still holds.
pub fn hold_all(game: &mut GameState, rules: &CompiledRuleset, owner: PlayerId, ids: &[u64]) {
    for s in soldiers(game, rules, owner, ids) {
        hold(game, rules, s.handle);
    }
}

/// Walks `unit` towards `to` while it has moves and the way is open; true when it stands there.
pub fn walk(game: &mut GameState, rules: &CompiledRuleset, unit: UnitHandle, to: TileId, budget: &mut usize) -> bool {
    while *budget > 0 {
        let Some((here, moves)) = game.view().auth.units().get(unit).map(|u| (u.core.tile, u.core.moves_left)) else { return false };
        if here == to {
            return true;
        }
        if moves == 0 {
            return false;
        }
        *budget -= 1;
        let command = if distance(game, here, to) == 1 { Command::MoveUnit { unit, destination: to } } else { Command::MarchTo { unit, destination: to } };
        if game.execute(rules, command).is_err() {
            return false;
        }
        let moved = game.view().auth.units().get(unit).is_some_and(|u| u.core.tile != here);
        if !moved {
            return false;
        }
    }
    false
}

/// A free tile a unit of `owner` may stand on: land, no unit, no city.
fn free(game: &GameState, rules: &CompiledRuleset, tile: TileId) -> bool {
    let auth = game.view().auth;
    auth.indexes().units_on(tile).is_empty() && auth.indexes().city_on_tile(tile).is_none() && battle::standable(rules, game, tile)
}

/// The tiles at exactly `ring` from `centre` that are free, nearest to `from` first (ties: the lower
/// id), the hills first among equals (a human takes the high ground).
pub fn posts(game: &GameState, rules: &CompiledRuleset, centre: TileId, ring: i32, from: TileId) -> Vec<TileId> {
    let map = game.view().auth.map();
    let mut tiles: Vec<(i32, u8, u32)> = (0..map.tile_count() as u32)
        .map(TileId)
        .filter(|t| distance(game, centre, *t) == ring && free(game, rules, *t) && map.land_area(*t) == map.land_area(centre))
        .map(|t| (distance(game, from, t), u8::from(!kernel::navigation::terrain::is_hills(map, t)), t.0))
        .collect();
    tiles.sort_unstable();
    tiles.into_iter().map(|(_, _, t)| TileId(t)).collect()
}

/// Walks `unit` to the nearest free post at `ring` from `centre`; true when it stands there.
pub fn take_post(game: &mut GameState, rules: &CompiledRuleset, unit: UnitHandle, centre: TileId, ring: i32, budget: &mut usize) -> bool {
    let Some(here) = game.view().auth.units().get(unit).map(|u| u.core.tile) else { return false };
    if distance(game, here, centre) == ring {
        return true;
    }
    for post in posts(game, rules, centre, ring, here).into_iter().take(3) {
        if walk(game, rules, unit, post, budget) {
            return true;
        }
        let moved = game.view().auth.units().get(unit).is_some_and(|u| u.core.tile != here);
        if moved {
            return false;
        }
    }
    false
}

/// The city's hit points as a percent of its maximum.
pub fn city_percent(game: &GameState, rules: &CompiledRuleset, city: CityHandle) -> u16 {
    let auth = game.view().auth;
    let Some(core) = auth.cities().get(city) else { return 0 };
    let max = classic_rules::combat::city::max_hit_points(auth, rules, core).max(1);
    (u32::from(classic_rules::combat::city::hit_points(auth, rules, core)) * 100 / u32::from(max)) as u16
}

/// The enemy's military units within `reach` of `tile`, the weakest first.
fn enemies_near(game: &GameState, rules: &CompiledRuleset, enemy: PlayerId, tile: TileId, reach: i32) -> Vec<(u16, UnitHandle, TileId)> {
    let auth = game.view().auth;
    let mut out: Vec<(u16, UnitHandle, TileId)> = auth
        .indexes()
        .units_of(enemy)
        .iter()
        .filter_map(|h| auth.units().get(*h).map(|u| (*h, u)))
        .filter(|(_, u)| matches!(role_of(rules, u.core.kind), AiRole::Military | AiRole::Naval) && distance(game, u.core.tile, tile) <= reach)
        .filter(|(_, u)| auth.indexes().city_on_tile(u.core.tile).is_none())
        .map(|(h, u)| (u.core.hp, h, u.core.tile))
        .collect();
    out.sort_by_key(|(hp, h, t)| (*hp, t.0, h.index));
    out
}

/// One ranged shot of `unit` at `target`; a kill when a unit was there and is gone.
fn shoot(game: &mut GameState, rules: &CompiledRuleset, unit: UnitHandle, target: TileId, victim: Option<UnitHandle>, budget: &mut usize, tally: &mut Tally) -> bool {
    if *budget == 0 {
        return false;
    }
    *budget -= 1;
    if game.execute(rules, Command::RangedAttack { unit, target }).is_err() {
        return false;
    }
    tally.shots += 1;
    if victim.is_some_and(|v| game.view().auth.units().get(v).is_none()) {
        tally.kills += 1;
    }
    true
}

/// A melee attack of `unit` on the unit `defender`; a kill when it is gone.
fn strike(game: &mut GameState, rules: &CompiledRuleset, unit: UnitHandle, defender: UnitHandle, budget: &mut usize, tally: &mut Tally) -> bool {
    if *budget == 0 {
        return false;
    }
    *budget -= 1;
    if game.execute(rules, Command::AttackUnit { attacker: unit, defender }).is_err() {
        return false;
    }
    if game.view().auth.units().get(defender).is_none() {
        tally.kills += 1;
    }
    true
}

/// A ranged unit's turn against `city` of `enemy`: shoot the city from its range (walking to a post
/// at that range first), else the weakest enemy unit in range; a wounded one steps back out of the
/// city's reach. A unit that did not act is held by the caller.
fn ranged_turn(game: &mut GameState, rules: &CompiledRuleset, s: Soldier, enemy: PlayerId, city: TileId, budget: &mut usize, tally: &mut Tally) {
    let range = s.range.unwrap_or(1);
    let strike_reach = classic_rules::combat::city::CITY_ATTACK_RANGE;
    if s.wounded() {
        if distance(game, s.tile, city) <= strike_reach {
            take_post(game, rules, s.handle, city, strike_reach + 1, budget);
        }
        return;
    }
    if distance(game, s.tile, city) > range {
        take_post(game, rules, s.handle, city, range, budget);
    }
    let Some((here, moves)) = game.view().auth.units().get(s.handle).map(|u| (u.core.tile, u.core.moves_left)) else { return };
    if moves == 0 {
        return;
    }
    if distance(game, here, city) <= range && shoot(game, rules, s.handle, city, None, budget, tally) {
        return;
    }
    if let Some((_, victim, tile)) = enemies_near(game, rules, enemy, here, range).first().copied() {
        shoot(game, rules, s.handle, tile, Some(victim), budget, tally);
    }
}

/// A melee unit's turn against `city`: take it when it is low (walking next to it first), else wait
/// two tiles out; strike an enemy unit next to it that is weaker than itself; a wounded one rests.
fn melee_turn(game: &mut GameState, rules: &CompiledRuleset, s: Soldier, enemy: PlayerId, city: TileId, budget: &mut usize, tally: &mut Tally) {
    let Some(handle) = game.view().auth.indexes().city_on_tile(city) else { return };
    if s.wounded() {
        return;
    }
    if city_percent(game, rules, handle) <= CLOSE_IN_PERCENT {
        take_post(game, rules, s.handle, city, 1, budget);
        let at = game.view().auth.units().get(s.handle).map(|u| (u.core.tile, u.core.moves_left));
        if let Some((tile, moves)) = at {
            if moves > 0 && distance(game, tile, city) == 1 && *budget > 0 {
                *budget -= 1;
                let before = game.view().auth.cities().get(handle).map(|c| c.owner);
                if game.execute(rules, Command::AttackCity { attacker: s.handle, city: handle }).is_ok() {
                    let after = game.view().auth.cities().get(handle).map(|c| c.owner);
                    if after != before {
                        tally.captured += 1;
                    }
                }
            }
        }
        return;
    }
    let weaker = enemies_near(game, rules, enemy, s.tile, 1).into_iter().find(|(hp, ..)| *hp < s.hp);
    if let Some((_, defender, _)) = weaker {
        strike(game, rules, s.handle, defender, budget, tally);
        return;
    }
    if distance(game, s.tile, city) > 2 {
        take_post(game, rules, s.handle, city, 2, budget);
    }
}

/// One turn of the army `ids` of `owner` on the city on `city` of `enemy`: the ranged units first
/// (they bring the city down), then the melee; every unit is held at the end.
pub fn siege(game: &mut GameState, rules: &CompiledRuleset, owner: PlayerId, enemy: PlayerId, ids: &[u64], city: TileId, budget: &mut usize) -> Tally {
    let mut tally = Tally::default();
    let army = soldiers(game, rules, owner, ids);
    for s in army.iter().filter(|s| s.range.is_some()) {
        ranged_turn(game, rules, *s, enemy, city, budget, &mut tally);
    }
    for s in army.iter().filter(|s| s.range.is_none()) {
        melee_turn(game, rules, *s, enemy, city, budget, &mut tally);
    }
    hold_all(game, rules, owner, ids);
    tally
}

/// Every unit of `ids` that stands next to a weaker enemy unit outside a city strikes it (ranged
/// units shoot the weakest in range): what a human does to a unit that came out.
pub fn pick_off(game: &mut GameState, rules: &CompiledRuleset, owner: PlayerId, enemy: PlayerId, ids: &[u64], budget: &mut usize) -> Tally {
    let mut tally = Tally::default();
    for s in soldiers(game, rules, owner, ids) {
        if s.moves == 0 || s.wounded() {
            continue;
        }
        let reach = s.range.unwrap_or(1);
        let Some((hp, victim, tile)) = enemies_near(game, rules, enemy, s.tile, reach).first().copied() else { continue };
        match s.range {
            Some(_) => {
                shoot(game, rules, s.handle, tile, Some(victim), budget, &mut tally);
            }
            None if hp < s.hp => {
                strike(game, rules, s.handle, victim, budget, &mut tally);
            }
            None => {}
        }
    }
    tally
}

/// Refuses every peace offered to `human` (a human in the middle of a campaign does not take it; the
/// seat's AI would answer it later in the turn).
pub fn refuse_peace(game: &mut GameState, rules: &CompiledRuleset, human: PlayerId) {
    let offers: Vec<u32> = game.view().auth.module().relations().offers().iter().filter(|o| o.to == human && o.items.iter().any(|(_, i)| *i == DealItem::Peace)).map(|o| o.id).collect();
    for offer in offers {
        let _ = game.execute(rules, Command::RespondDeal { player: human, offer, accept: false, why: PeaceRefusal::Unsaid });
    }
}

/// Declares the war of `human` on `victim` unless they are at war; true when they are at war.
pub fn war(game: &mut GameState, rules: &CompiledRuleset, human: PlayerId, victim: PlayerId) -> bool {
    if !game.view().auth.diplomacy().at_war(human, victim) {
        let _ = game.execute(rules, Command::DeclareWar { player: human, enemy: victim });
    }
    game.view().auth.diplomacy().at_war(human, victim)
}

/// The city of `victim` nearest to the army (ties: the lower tile id); none when it holds no city.
pub fn nearest_city(game: &GameState, rules: &CompiledRuleset, owner: PlayerId, victim: PlayerId, ids: &[u64]) -> Option<TileId> {
    let army: Vec<TileId> = soldiers(game, rules, owner, ids).iter().map(|s| s.tile).collect();
    let auth = game.view().auth;
    let map = auth.map();
    auth.indexes()
        .cities_of(victim)
        .iter()
        .filter_map(|h| auth.cities().get(*h))
        .filter(|c| army.iter().any(|t| map.land_area(*t) == map.land_area(c.tile)))
        .map(|c| (army.iter().map(|t| distance(game, *t, c.tile)).min().unwrap_or(i32::MAX), c.tile.0))
        .min()
        .map(|(_, t)| TileId(t))
}

/// The kinds of an army of `player`: its best melee, ranged and mounted units by its technologies
/// (a missing arm stands in by the next, `battle::fallback`). The ranged arm, when the player cannot
/// build one, is the cheapest ranged unit of the rules with the technology it needs (the rusher
/// researched it): `(kinds, techs to give)`.
pub fn army_kinds(game: &GameState, rules: &CompiledRuleset, player: PlayerId) -> Result<(ArmyKinds, Vec<u16>), String> {
    let kinds = super::super::edit::kinds_of(game, rules, player);
    let melee = *kinds.get(&Arm::Melee).ok_or("the human has no melee unit")?;
    let mounted = kinds.get(&Arm::Mounted).copied().unwrap_or(melee);
    if let Some(ranged) = kinds.get(&Arm::Ranged).copied() {
        return Ok((ArmyKinds { melee, ranged, mounted }, Vec::new()));
    }
    let nation = game.view().auth.players().nation_of(player);
    let cheapest = (0..rules.units.len())
        .map(|i| UnitKindId(i as u16))
        .filter(|&k| {
            let def = &rules.units[k.0 as usize];
            def.domain == kernel::rules::content::Domain::Land && !def.unbuildable && def.nation.is_none_or(|n| n == nation) && battle::kind_arm(rules, k) == Some(Arm::Ranged) && role_of(rules, k) == AiRole::Military
        })
        .min_by_key(|k| (rules.units[k.0 as usize].cost, k.0))
        .ok_or("the rules hold no ranged unit")?;
    let techs = rules.units[cheapest.0 as usize].required_tech.map(|t| vec![t.0]).unwrap_or_default();
    Ok((ArmyKinds { melee, ranged: cheapest, mounted }, techs))
}

#[derive(Debug, Clone, Copy)]
pub struct ArmyKinds {
    pub melee: UnitKindId,
    pub ranged: UnitKindId,
    pub mounted: UnitKindId,
}

/// Free tiles of `player`'s land (and the unowned land within two tiles of its cities) on the
/// landmass of `toward`, nearest to `toward` first: where its army is raised.
pub fn home_tiles(game: &GameState, rules: &CompiledRuleset, player: PlayerId, toward: TileId) -> Vec<TileId> {
    let auth = game.view().auth;
    let map = auth.map();
    let cities: Vec<TileId> = auth.indexes().cities_of(player).iter().filter_map(|h| auth.cities().get(*h)).map(|c| c.tile).collect();
    let mut tiles: Vec<(i32, u32)> = (0..map.tile_count() as u32)
        .map(TileId)
        .filter(|t| map.land_area(*t) == map.land_area(toward) && free(game, rules, *t))
        .filter(|t| {
            let owner = map.owner(*t).flatten();
            owner == Some(player) || (owner.is_none() && cities.iter().any(|c| distance(game, *c, *t) <= 2))
        })
        .map(|t| (distance(game, t, toward), t.0))
        .collect();
    tiles.sort_unstable();
    tiles.into_iter().map(|(_, t)| TileId(t)).collect()
}
