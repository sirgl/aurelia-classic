//! Bait (MT2, MT3): at war, the human leaves a worker (MT3) or a weak unit (MT2) two tiles in front of
//! the seat's city, inside the city's reach, with a force hidden three to four tiles out, beyond the
//! city's strike. Whatever comes out of the city for the bait, or for the force, is picked off; once
//! the city stands empty or low the force takes it.
//!
//! The units of the edit are the baits first, then the force (`Bait::baits` of them are baits).

use std::collections::BTreeMap;

use classic_rules::ids::{PlayerId, TileId, UnitKindId};
use classic_rules::{CompiledRuleset, GameState};
use kernel::ai::plan::{role_of, AiRole};
use serde::{Deserialize, Serialize};

use super::army::{self, Tally};
use super::{scripted, Cast, Order, Trick};
use crate::stress::edit::Change;
use crate::stress::position::Position;
use crate::stress::scene::{nearest_pair, Edit, Plan, Roles};
use crate::stress::seat::Seat;

/// The city falls to the force at or below this share of its hit points even with a garrison.
const ASSAULT_PERCENT: u16 = 50;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Bait {
    pub seat: u8,
    pub victim: u8,
    /// The city the bait is laid before.
    pub city: u32,
    /// How many of `units` (the first) are baits.
    pub baits: usize,
    #[serde(default)]
    pub units: Vec<u64>,
}

pub struct Play {
    order: Bait,
    taken: Vec<u64>,
    tally: Tally,
    assault: bool,
}

impl Play {
    pub fn new(order: Bait) -> Self {
        Play { order, taken: Vec::new(), tally: Tally::default(), assault: false }
    }

    fn baits(&self) -> &[u64] {
        &self.order.units[..self.order.baits.min(self.order.units.len())]
    }

    fn force(&self) -> &[u64] {
        &self.order.units[self.order.baits.min(self.order.units.len())..]
    }

    /// A bait no longer the human's (captured or killed) is counted once.
    fn count_taken(&mut self, game: &GameState, rules: &CompiledRuleset) {
        let human = PlayerId(self.order.seat);
        let alive: Vec<u64> = army::soldiers(game, rules, human, self.baits()).iter().map(|s| s.id).collect();
        let lost: Vec<u64> = self.baits().iter().copied().filter(|id| !alive.contains(id) && !self.taken.contains(id)).collect();
        self.taken.extend(lost);
    }

    /// The city stands empty of military units, or low.
    fn open(game: &GameState, rules: &CompiledRuleset, city: TileId) -> bool {
        let auth = game.view().auth;
        let Some(handle) = auth.indexes().city_on_tile(city) else { return false };
        let owner = auth.cities().get(handle).map(|c| c.owner);
        let garrison = auth.indexes().units_on(city).iter().filter_map(|h| auth.units().get(*h)).any(|u| Some(u.core.owner) == owner && role_of(rules, u.core.kind) == AiRole::Military);
        !garrison || army::city_percent(game, rules, handle) <= ASSAULT_PERCENT
    }
}

impl Seat for Play {
    fn player(&self) -> PlayerId {
        PlayerId(self.order.seat)
    }

    fn play(&mut self, game: &mut GameState, rules: &CompiledRuleset) {
        let (human, victim, city) = (PlayerId(self.order.seat), PlayerId(self.order.victim), TileId(self.order.city));
        army::refuse_peace(game, rules, human);
        self.count_taken(game, rules);
        let mut budget = army::BUDGET;
        let force = self.force().to_vec();
        self.tally.add(army::pick_off(game, rules, human, victim, &force, &mut budget));
        let still_theirs = game.view().auth.indexes().city_on_tile(city).and_then(|h| game.view().auth.cities().get(h)).is_some_and(|c| c.owner == victim);
        if still_theirs && (self.assault || Self::open(game, rules, city)) {
            self.assault = true;
            self.tally.add(army::siege(game, rules, human, victim, &force, city, &mut budget));
        }
        army::hold_all(game, rules, human, &self.order.units);
    }

    fn counts(&self) -> BTreeMap<String, f64> {
        BTreeMap::from([
            ("script_bait_taken".to_owned(), self.taken.len() as f64),
            ("script_lured".to_owned(), f64::from(self.tally.kills)),
            ("script_captured".to_owned(), f64::from(self.tally.captured)),
            ("script_assault".to_owned(), f64::from(u8::from(self.assault))),
        ])
    }
}

// ---------------------------------------------------------------------------
// The scene

/// The nearest pair at war (the base: both arms have the war).
pub(super) fn plan(_rules: &CompiledRuleset, position: &Position, instance: usize) -> Result<Plan, String> {
    let game = &position.game;
    let (a, b, d) = nearest_pair(game).ok_or("no two nations share a landmass")?;
    let (seat, human) = if instance % 2 == 1 { (b, a) } else { (a, b) };
    let base = vec![Change::Meet { a: seat.0, b: human.0 }, Change::War { a: human.0, b: seat.0 }];
    let info = BTreeMap::from([("pair_distance".to_owned(), d.to_string())]);
    Ok(Plan { roles: Roles::Human(Cast { trick: Trick::Bait, seat: seat.0, human: human.0 }), base, info })
}

/// The baits two tiles before the seat's city nearest to the human, the force three and four tiles
/// out beside them. Mild: a worker and three units; medium: a weak unit and four; strong: both baits
/// and six.
pub(super) fn edit_of(cast: Cast, level: usize, rules: &CompiledRuleset, position: &Position) -> Result<Edit, String> {
    let game = &position.game;
    let (seat, human) = (PlayerId(cast.seat), PlayerId(cast.human));
    let city = crate::battle::front_city(game, human, seat).ok_or("the seat has no city on the human's landmass")?;
    let home = crate::battle::front_city(game, seat, human).ok_or("the human has no city on the seat's landmass")?;
    let worker = cheapest(game, rules, human, AiRole::Worker).ok_or("the human can build no worker")?;
    let weak = cheapest(game, rules, human, AiRole::Military).ok_or("the human can build no unit")?;
    let baits: Vec<UnitKindId> = [vec![worker], vec![weak], vec![worker, weak]][level].clone();
    let (kinds, techs) = army::army_kinds(game, rules, human)?;
    let (ranged, melee) = [(1, 2), (2, 2), (3, 3)][level];
    let bait_tiles = army::posts(game, rules, city, 2, home);
    if bait_tiles.len() < baits.len() {
        return Err("no free tile two tiles before the city".to_owned());
    }
    let mut force_tiles = army::posts(game, rules, city, 3, home);
    force_tiles.extend(army::posts(game, rules, city, 4, home));
    force_tiles.sort_by_key(|t| (army::distance(game, *t, bait_tiles[0]), t.0));
    if force_tiles.len() < ranged + melee {
        return Err("no room for the force beyond the city's strike".to_owned());
    }
    let mut changes = Vec::new();
    if !techs.is_empty() {
        changes.push(Change::Discover { player: human.0, techs });
    }
    for (tile, kind) in bait_tiles.iter().zip(&baits) {
        changes.push(Change::Spawn { player: human.0, tile: tile.0, kind: kind.0 });
    }
    let mut force = vec![kinds.ranged; ranged];
    force.extend(vec![kinds.melee; melee]);
    for (tile, kind) in force_tiles.iter().zip(force) {
        changes.push(Change::Spawn { player: human.0, tile: tile.0, kind: kind.0 });
    }
    let order = Bait { seat: human.0, victim: seat.0, city: city.0, baits: baits.len(), units: Vec::new() };
    Ok(scripted(changes, Order::Bait(order)))
}

/// The cheapest land unit of `role` that `player` can build by its technologies and nation.
fn cheapest(game: &GameState, rules: &CompiledRuleset, player: PlayerId, role: AiRole) -> Option<UnitKindId> {
    let auth = game.view().auth;
    let known = auth.players().techs_of(player);
    let nation = auth.players().nation_of(player);
    (0..rules.units.len())
        .map(|i| UnitKindId(i as u16))
        .filter(|&k| {
            let def = &rules.units[k.0 as usize];
            def.domain == kernel::rules::content::Domain::Land
                && !def.unbuildable
                && role_of(rules, k) == role
                && def.required_tech.is_none_or(|t| known.contains(&t))
                && def.obsolete_tech.is_none_or(|t| !known.contains(&t))
                && def.nation.is_none_or(|n| n == nation)
                && (role != AiRole::Military || crate::battle::kind_arm(rules, k).is_some())
        })
        .min_by_key(|k| (rules.units[k.0 as usize].cost, k.0))
}
