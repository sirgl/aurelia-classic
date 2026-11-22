//! The scripted fighters of wave 3: the kill-zone (P6), the sea-hunter (P9) and the air striker
//! (P10). Each plays only its own units (the AI of the seat leaves them alone, `Seat::holds`) and
//! counts what they did.

use std::collections::BTreeMap;

use classic_rules::ids::{PlayerId, TileId, UnitHandle};
use classic_rules::{Command, CompiledRuleset, GameState};
use kernel::ai::plan::{role_of, AiRole};
use kernel::rules::content::Domain;

use super::{distance, handle, orders_one_of, Seat};

/// Hit points at or below which a melee unit of the kill-zone finishes an enemy next to it.
const FINISH_HP: u16 = 50;

/// Attack attempts on the targets of one unit in one turn.
const TRIES: usize = 3;

/// The living units of `owner` on the tiles `keep` accepts, by (distance from `from`, hit points,
/// id): the nearest and most wounded first.
fn units_of(game: &GameState, owner: PlayerId, from: TileId, keep: impl Fn(TileId) -> bool) -> Vec<(UnitHandle, TileId)> {
    let auth = game.view().auth;
    let mut out: Vec<(i32, u16, u64, UnitHandle, TileId)> = auth
        .indexes()
        .units_of(owner)
        .iter()
        .filter_map(|h| auth.units().get(*h).map(|u| (*h, u)))
        .filter(|(_, u)| keep(u.core.tile))
        .map(|(h, u)| (distance(game, from, u.core.tile), u.core.hp, u.core.id.0, h, u.core.tile))
        .collect();
    out.sort_unstable_by_key(|(d, hp, id, ..)| (*d, *hp, *id));
    out.into_iter().map(|(.., h, t)| (h, t)).collect()
}

/// The range of a unit that shoots (None: a melee unit or a civilian).
fn range_of(rules: &CompiledRuleset, game: &GameState, unit: UnitHandle) -> Option<i32> {
    let kind = game.view().auth.units().get(unit)?.core.kind;
    classic_rules::ai::combat::ranged_range(rules, kind)
}

fn alive(game: &GameState, unit: UnitHandle) -> bool {
    game.view().auth.units().get(unit).is_some()
}

fn tile_of(game: &GameState, unit: UnitHandle) -> Option<TileId> {
    game.view().auth.units().get(unit).map(|u| u.core.tile)
}

/// Fires at the first of `targets` the rules accept; true when the target died.
fn shoot(game: &mut GameState, rules: &CompiledRuleset, unit: UnitHandle, targets: &[(UnitHandle, TileId)]) -> Option<bool> {
    for (target, tile) in targets.iter().take(TRIES) {
        if game.execute(rules, Command::RangedAttack { unit, target: *tile }).is_ok() {
            return Some(!alive(game, *target));
        }
    }
    None
}

/// Attacks the first of `targets` the rules accept; true when the target died.
fn strike(game: &mut GameState, rules: &CompiledRuleset, unit: UnitHandle, targets: &[(UnitHandle, TileId)]) -> Option<bool> {
    for (target, _) in targets.iter().take(TRIES) {
        if game.execute(rules, Command::AttackUnit { attacker: unit, defender: *target }).is_ok() {
            return Some(!alive(game, *target));
        }
    }
    None
}

// ---------------------------------------------------------------------------
// P6 kill-zone

pub(super) struct KillZone {
    seat: PlayerId,
    enemy: PlayerId,
    /// The units and the tile each holds (where it stood at the start).
    posts: Vec<(u64, Option<TileId>)>,
    held: Vec<u64>,
    kills: u32,
    shots: u32,
    bait: Option<u64>,
}

impl KillZone {
    pub(super) fn new(seat: PlayerId, enemy: PlayerId, units: Vec<u64>) -> Self {
        KillZone { seat, enemy, posts: units.iter().map(|id| (*id, None)).collect(), held: units, kills: 0, shots: 0, bait: None }
    }

    /// The posts and the bait, read at the first turn of the script.
    fn read_posts(&mut self, game: &GameState, rules: &CompiledRuleset) {
        for (id, post) in &mut self.posts {
            let Some(unit) = handle(game, *id).and_then(|h| game.view().auth.units().get(h)) else { continue };
            *post = Some(unit.core.tile);
            if role_of(rules, unit.core.kind) != AiRole::Military {
                self.bait = Some(*id);
            }
        }
    }

    fn guard(&mut self, game: &mut GameState, rules: &CompiledRuleset, unit: UnitHandle, post: TileId) {
        let Some((here, kind)) = game.view().auth.units().get(unit).map(|u| (u.core.tile, u.core.kind)) else { return };
        let enemy = self.enemy;
        let mut fought = false;
        if let Some(range) = range_of(rules, game, unit) {
            let targets = units_of(game, enemy, here, |t| distance(game, here, t) <= range);
            let mut by_hp = targets;
            by_hp.sort_by_key(|(h, _)| game.view().auth.units().get(*h).map_or(u16::MAX, |u| u.core.hp));
            if let Some(killed) = shoot(game, rules, unit, &by_hp) {
                self.shots += 1;
                self.kills += u32::from(killed);
                fought = true;
            }
        } else if role_of(rules, kind) == AiRole::Military {
            let wounded = |t: TileId| distance(game, here, t) == 1 && game.view().auth.indexes().city_on_tile(t).is_none();
            let targets: Vec<(UnitHandle, TileId)> = units_of(game, enemy, here, wounded)
                .into_iter()
                .filter(|(h, _)| game.view().auth.units().get(*h).is_some_and(|u| u.core.hp <= FINISH_HP))
                .collect();
            if let Some(killed) = strike(game, rules, unit, &targets) {
                self.shots += 1;
                self.kills += u32::from(killed);
                fought = true;
            }
        }
        if tile_of(game, unit).is_some_and(|t| t != post) {
            let _ = game.execute(rules, Command::MarchTo { unit, destination: post });
            return;
        }
        if !fought {
            let _ = game.execute(rules, Command::Fortify { unit });
        }
    }
}

impl Seat for KillZone {
    fn player(&self) -> PlayerId {
        self.seat
    }

    fn play(&mut self, game: &mut GameState, rules: &CompiledRuleset) {
        if self.posts.iter().all(|(_, post)| post.is_none()) {
            self.read_posts(game, rules);
        }
        for (id, post) in self.posts.clone() {
            let (Some(unit), Some(post)) = (handle(game, id), post) else { continue };
            if Some(id) == self.bait || game.view().auth.units().get(unit).is_none_or(|u| u.core.owner != self.seat) {
                continue;
            }
            self.guard(game, rules, unit, post);
        }
    }

    fn counts(&self) -> BTreeMap<String, f64> {
        BTreeMap::from([("script_kills".to_owned(), f64::from(self.kills)), ("script_shots".to_owned(), f64::from(self.shots))])
    }

    fn holds(&self, game: &GameState, command: &Command) -> bool {
        orders_one_of(game, command, &self.held)
    }
}

// ---------------------------------------------------------------------------
// P9 sea-hunter

pub(super) struct SeaHunt {
    seat: PlayerId,
    victim: PlayerId,
    ships: Vec<u64>,
    sunk: u32,
    shots: u32,
}

impl SeaHunt {
    pub(super) fn new(seat: PlayerId, victim: PlayerId, ships: Vec<u64>) -> Self {
        SeaHunt { seat, victim, ships, sunk: 0, shots: 0 }
    }

    /// One ship's turn: at the prey in reach, else towards the nearest prey and at it.
    fn hunt(&mut self, game: &mut GameState, rules: &CompiledRuleset, ship: UnitHandle) {
        let Some(here) = tile_of(game, ship) else { return };
        let water = |t: TileId| game.view().auth.map().is_water(t);
        let prey = units_of(game, self.victim, here, water);
        let Some(&(_, nearest)) = prey.first() else { return };
        let reach = range_of(rules, game, ship).unwrap_or(1);
        if distance(game, here, nearest) > reach {
            self.sail_to(game, rules, ship, here, nearest);
        }
        let Some(now) = tile_of(game, ship) else { return };
        let in_reach: Vec<(UnitHandle, TileId)> = units_of(game, self.victim, now, |t| game.view().auth.map().is_water(t) && distance(game, now, t) <= reach);
        let result = match range_of(rules, game, ship) {
            Some(_) => shoot(game, rules, ship, &in_reach),
            None => strike(game, rules, ship, &in_reach),
        };
        if let Some(killed) = result {
            self.shots += 1;
            self.sunk += u32::from(killed);
        }
    }
}

impl SeaHunt {
    /// Sails to the free water next to `prey` nearest to the ship (an occupied tile is no goal).
    fn sail_to(&self, game: &mut GameState, rules: &CompiledRuleset, ship: UnitHandle, here: TileId, prey: TileId) {
        let map = game.view().auth.map();
        let mut beside: Vec<(i32, u32)> = map
            .dimensions
            .neighbors(prey)
            .into_iter()
            .filter(|t| map.is_water(*t) && game.view().auth.indexes().units_on(*t).is_empty())
            .map(|t| (distance(game, here, t), t.0))
            .collect();
        beside.sort_unstable();
        for (_, tile) in beside.into_iter().take(TRIES) {
            if game.execute(rules, Command::MarchTo { unit: ship, destination: TileId(tile) }).is_ok() {
                return;
            }
        }
    }
}

impl Seat for SeaHunt {
    fn player(&self) -> PlayerId {
        self.seat
    }

    fn play(&mut self, game: &mut GameState, rules: &CompiledRuleset) {
        for id in self.ships.clone() {
            let Some(ship) = handle(game, id) else { continue };
            if game.view().auth.units().get(ship).is_some_and(|u| u.core.owner == self.seat) {
                self.hunt(game, rules, ship);
            }
        }
    }

    fn counts(&self) -> BTreeMap<String, f64> {
        BTreeMap::from([("script_sunk".to_owned(), f64::from(self.sunk)), ("script_shots".to_owned(), f64::from(self.shots))])
    }

    fn holds(&self, game: &GameState, command: &Command) -> bool {
        orders_one_of(game, command, &self.ships)
    }
}

// ---------------------------------------------------------------------------
// P10 air-nuke

pub(super) struct AirStrike {
    seat: PlayerId,
    victim: PlayerId,
    planes: Vec<u64>,
    strikes: u32,
    kills: u32,
    nukes: u32,
}

/// What an aircraft of the striker is for, from the rules' data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Wing {
    /// A nuclear weapon (`nuke_level`).
    Nuke,
    /// An aircraft that cannot intercept: it strikes.
    Bomber,
    /// An aircraft with an interception range: it guards.
    Fighter,
}

fn wing(rules: &CompiledRuleset, game: &GameState, unit: UnitHandle) -> Option<Wing> {
    let kind = game.view().auth.units().get(unit)?.core.kind;
    let def = &rules.units[kind.0 as usize];
    if def.domain != Domain::Air {
        return None;
    }
    if def.combat.nuke_level > 0 {
        return Some(Wing::Nuke);
    }
    Some(if def.combat.air_intercept_range > 0 { Wing::Fighter } else { Wing::Bomber })
}

impl AirStrike {
    pub(super) fn new(seat: PlayerId, victim: PlayerId, planes: Vec<u64>) -> Self {
        AirStrike { seat, victim, planes, strikes: 0, kills: 0, nukes: 0 }
    }

    /// The victim's cities in range of `from`, the largest first.
    fn cities_in(&self, game: &GameState, from: TileId, range: i32) -> Vec<TileId> {
        let auth = game.view().auth;
        let mut cities: Vec<(u32, TileId)> = auth
            .indexes()
            .cities_of(self.victim)
            .iter()
            .filter_map(|h| auth.cities().get(*h))
            .filter(|c| distance(game, from, c.tile) <= range)
            .map(|c| (c.population, c.tile))
            .collect();
        cities.sort_unstable_by_key(|(pop, t)| (std::cmp::Reverse(*pop), t.0));
        cities.into_iter().map(|(_, t)| t).collect()
    }

    fn bomb(&mut self, game: &mut GameState, rules: &CompiledRuleset, plane: UnitHandle) {
        let (Some(here), Some(range)) = (tile_of(game, plane), range_of(rules, game, plane)) else { return };
        let mut targets = units_of(game, self.victim, here, |t| distance(game, here, t) <= range);
        // The strongest unit first: what a human bombs before the battle.
        targets.sort_by_key(|(h, _)| std::cmp::Reverse(game.view().auth.units().get(*h).map_or(0, |u| rules.units[u.core.kind.0 as usize].strength)));
        if let Some(killed) = shoot(game, rules, plane, &targets) {
            self.strikes += 1;
            self.kills += u32::from(killed);
            return;
        }
        for city in self.cities_in(game, here, range).into_iter().take(TRIES) {
            if game.execute(rules, Command::RangedAttack { unit: plane, target: city }).is_ok() {
                self.strikes += 1;
                return;
            }
        }
    }

    fn nuke(&mut self, game: &mut GameState, rules: &CompiledRuleset, unit: UnitHandle) {
        let Some(here) = tile_of(game, unit) else { return };
        let kind = game.view().auth.units().get(unit).map(|u| u.core.kind);
        let range = kind.map_or(0, |k| i32::from(rules.units[k.0 as usize].range));
        for city in self.cities_in(game, here, range).into_iter().take(TRIES) {
            if game.execute(rules, Command::Nuke { unit, target: city }).is_ok() {
                self.nukes += 1;
                return;
            }
        }
    }
}

impl Seat for AirStrike {
    fn player(&self) -> PlayerId {
        self.seat
    }

    fn play(&mut self, game: &mut GameState, rules: &CompiledRuleset) {
        for id in self.planes.clone() {
            let Some(plane) = handle(game, id) else { continue };
            match wing(rules, game, plane) {
                Some(Wing::Nuke) => self.nuke(game, rules, plane),
                Some(Wing::Bomber) => self.bomb(game, rules, plane),
                Some(Wing::Fighter) => {
                    let _ = game.execute(rules, Command::Intercept { unit: plane });
                }
                None => {}
            }
        }
    }

    fn counts(&self) -> BTreeMap<String, f64> {
        BTreeMap::from([
            ("script_strikes".to_owned(), f64::from(self.strikes)),
            ("script_kills".to_owned(), f64::from(self.kills)),
            ("script_nukes".to_owned(), f64::from(self.nukes)),
        ])
    }

    fn holds(&self, game: &GameState, command: &Command) -> bool {
        orders_one_of(game, command, &self.planes)
    }
}
