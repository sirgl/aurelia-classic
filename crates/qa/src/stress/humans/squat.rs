//! Squat (MO5, D14): at war, the human's units walk to the seat's luxury and strategic tiles, pillage
//! the improvement and stand on the tile, fortified, for as long as they live: the resource is cut and
//! no worker can repair it. They attack nothing.

use std::collections::BTreeMap;

use classic_rules::ids::{PlayerId, TileId};
use classic_rules::{Command, CompiledRuleset, GameState};
use kernel::rules::content::ResourceClass;
use serde::{Deserialize, Serialize};

use super::army;
use super::{scripted, Cast, Order, Trick};
use crate::stress::edit::Change;
use crate::stress::position::Position;
use crate::stress::scene::{nearest_pair, Edit, Plan, Roles};
use crate::stress::seat::Seat;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Squat {
    pub seat: u8,
    pub victim: u8,
    #[serde(default)]
    pub units: Vec<u64>,
}

pub struct Play {
    order: Squat,
    /// The tile each unit goes for, by its id.
    posts: BTreeMap<u64, TileId>,
    pillaged: u32,
    /// Unit-rounds a squatter stood on a resource tile of the seat.
    squat_rounds: u32,
    /// The script's turns played.
    turns: u32,
}

impl Play {
    pub fn new(order: Squat) -> Self {
        Play { order, posts: BTreeMap::new(), pillaged: 0, squat_rounds: 0, turns: 0 }
    }

    /// The seat's luxury and strategic tiles no squatter holds or goes for, nearest to `from` first.
    fn free_targets(&self, game: &GameState, rules: &CompiledRuleset, from: TileId) -> Vec<TileId> {
        let taken: Vec<TileId> = self.posts.values().copied().collect();
        let mut tiles: Vec<(i32, u32)> = resource_tiles(game, rules, PlayerId(self.order.victim))
            .into_iter()
            .filter(|t| !taken.contains(t))
            .map(|t| (army::distance(game, from, t), t.0))
            .collect();
        tiles.sort_unstable();
        tiles.into_iter().map(|(_, t)| TileId(t)).collect()
    }

    /// One squatter's turn: on its tile it pillages and stands; else it walks there.
    fn squat(&mut self, game: &mut GameState, rules: &CompiledRuleset, id: u64, budget: &mut usize) {
        let human = PlayerId(self.order.seat);
        let Some(s) = army::soldiers(game, rules, human, &[id]).into_iter().next() else { return };
        let still_theirs = |game: &GameState, t: TileId| game.view().auth.map().owner(t).flatten() == Some(PlayerId(self.order.victim));
        let post = match self.posts.get(&id).copied().filter(|t| still_theirs(game, *t)) {
            Some(post) => post,
            None => {
                self.posts.remove(&id);
                let Some(next) = self.free_targets(game, rules, s.tile).into_iter().next() else { return };
                self.posts.insert(id, next);
                next
            }
        };
        if s.tile != post {
            army::walk(game, rules, s.handle, post, budget);
        }
        let Some((here, moves)) = game.view().auth.units().get(s.handle).map(|u| (u.core.tile, u.core.moves_left)) else { return };
        if here != post {
            return;
        }
        let map = game.view().auth.map();
        if moves > 0 && map.improvement(here).flatten().is_some() && !map.pillaged(here) && *budget > 0 {
            *budget -= 1;
            if game.execute(rules, Command::Pillage { unit: s.handle }).is_ok() {
                self.pillaged += 1;
            }
        }
    }
}

impl Seat for Play {
    fn player(&self) -> PlayerId {
        PlayerId(self.order.seat)
    }

    fn play(&mut self, game: &mut GameState, rules: &CompiledRuleset) {
        let human = PlayerId(self.order.seat);
        self.turns += 1;
        army::refuse_peace(game, rules, human);
        let mut budget = army::BUDGET;
        for id in self.order.units.clone() {
            self.squat(game, rules, id, &mut budget);
        }
        let seat_tiles = resource_tiles(game, rules, PlayerId(self.order.victim));
        let standing = army::soldiers(game, rules, human, &self.order.units).iter().filter(|s| seat_tiles.contains(&s.tile)).count();
        self.squat_rounds += standing as u32;
        army::hold_all(game, rules, human, &self.order.units);
    }

    fn counts(&self) -> BTreeMap<String, f64> {
        // The unit-rounds on the seat's tiles as a share of what the squatters could hold.
        let could = (self.order.units.len() as u32 * self.turns).max(1);
        BTreeMap::from([
            ("script_pillaged".to_owned(), f64::from(self.pillaged)),
            ("script_squat_rounds".to_owned(), f64::from(self.squat_rounds)),
            ("script_squat_share".to_owned(), f64::from(self.squat_rounds) / f64::from(could)),
        ])
    }
}

/// The tiles of `player`'s land, not a city, with a luxury or strategic resource.
pub(super) fn resource_tiles(game: &GameState, rules: &CompiledRuleset, player: PlayerId) -> Vec<TileId> {
    let auth = game.view().auth;
    let map = auth.map();
    (0..map.tile_count() as u32)
        .map(TileId)
        .filter(|t| map.owner(*t).flatten() == Some(player) && auth.indexes().city_on_tile(*t).is_none())
        .filter(|t| map.resource(*t).flatten().and_then(|r| rules.resources.get(r.0 as usize)).is_some_and(|d| matches!(d.class, Some(ResourceClass::Luxury | ResourceClass::Strategic))))
        .collect()
}

// ---------------------------------------------------------------------------
// The scene

/// The nearest pair at war (the base), the seat holding a luxury or strategic tile.
pub(super) fn plan(rules: &CompiledRuleset, position: &Position, instance: usize) -> Result<Plan, String> {
    let game = &position.game;
    let (a, b, d) = nearest_pair(game).ok_or("no two nations share a landmass")?;
    let (seat, human) = if instance % 2 == 1 { (b, a) } else { (a, b) };
    if resource_tiles(game, rules, seat).is_empty() {
        return Err("the seat holds no luxury or strategic tile".to_owned());
    }
    let base = vec![Change::Meet { a: seat.0, b: human.0 }, Change::War { a: human.0, b: seat.0 }];
    let info = BTreeMap::from([("pair_distance".to_owned(), d.to_string())]);
    Ok(Plan { roles: Roles::Human(Cast { trick: Trick::Squat, seat: seat.0, human: human.0 }), base, info })
}

/// Two, three or five squatters (the human's best melee kind) raised at home nearest to the seat's
/// resource tile nearest to it.
pub(super) fn edit_of(cast: Cast, level: usize, rules: &CompiledRuleset, position: &Position) -> Result<Edit, String> {
    let game = &position.game;
    let (seat, human) = (PlayerId(cast.seat), PlayerId(cast.human));
    let home = crate::battle::front_city(game, seat, human).ok_or("the human has no city on the seat's landmass")?;
    let map = game.view().auth.map();
    let target = resource_tiles(game, rules, seat)
        .into_iter()
        .filter(|t| map.land_area(*t) == map.land_area(home))
        .min_by_key(|t| (army::distance(game, home, *t), t.0))
        .ok_or("no resource tile of the seat on the human's landmass")?;
    let (kinds, _) = army::army_kinds(game, rules, human)?;
    let n = [2, 3, 5][level];
    let tiles = army::home_tiles(game, rules, human, target);
    if tiles.len() < n {
        return Err(format!("only {} free tiles at home for {n} squatters", tiles.len()));
    }
    let changes = tiles.into_iter().take(n).map(|t| Change::Spawn { player: human.0, tile: t.0, kind: kinds.melee.0 }).collect();
    Ok(scripted(changes, Order::Squat(Squat { seat: human.0, victim: seat.0, units: Vec::new() })))
}
