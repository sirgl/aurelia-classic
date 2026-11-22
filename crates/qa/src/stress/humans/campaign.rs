//! An army of the human that takes the seat's cities: the early rush (MO1), the sneak attack under
//! open borders (MO3) and the vulture that takes every new city (MO8). One state machine; the three
//! differ only in what starts the war (`Trigger`).
//!
//! The army is raised in the human's own land, nearest to the seat (the edit: the units a human
//! would have built for the trick), and played by the script from the first round: it walks, declares,
//! shoots from range and takes the city with a melee unit, then goes on to the next city.

use std::collections::BTreeMap;

use classic_rules::ids::{PlayerId, TileId};
use classic_rules::{CompiledRuleset, GameState};
use serde::{Deserialize, Serialize};

use super::army::{self, ArmyKinds, Tally};
use super::{scripted, Cast, Order, Trick};
use crate::stress::edit::{Accord, Change};
use crate::stress::position::Position;
use crate::stress::scene::{nearest_pair, Edit, Plan, Roles};
use crate::stress::seat::Seat;

/// The rounds the sneak army may walk at peace before it strikes from where it stands.
const MAX_APPROACH_ROUNDS: u32 = 8;

/// What starts the war.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Trigger {
    /// War at the first turn; the capital is the first target (MO1).
    AtOnce,
    /// The army walks in at peace under open borders and declares war when every unit stands within
    /// two tiles of the target city, or after `MAX_APPROACH_ROUNDS`; it strikes the same turn (MO3).
    InPosition,
    /// The army waits at home; the first city the seat founds after the start is the target, and the
    /// war is declared on it (MO8). After a capture it waits again.
    NewCity,
}

/// The script's data: `seat` plays it against `victim` with `units`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Campaign {
    pub seat: u8,
    pub victim: u8,
    pub trigger: Trigger,
    #[serde(default)]
    pub units: Vec<u64>,
}

/// Where the campaign stands.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Stage {
    /// Nothing read yet: the first turn decides.
    Start,
    /// Walking at peace to the target city, `rounds` so far.
    Approach { target: TileId, rounds: u32 },
    /// Waiting at home; the seat's cities when the wait began.
    Wait { known: Vec<u64> },
    /// At war on the city on `target`.
    Siege { target: TileId },
    /// The army is gone, or nothing is left to take.
    Done,
}

pub struct Play {
    order: Campaign,
    stage: Stage,
    round: u32,
    war_round: Option<u32>,
    tally: Tally,
    capital_taken: bool,
    new_cities_taken: u32,
}

impl Play {
    pub fn new(order: Campaign) -> Self {
        Play { order, stage: Stage::Start, round: 0, war_round: None, tally: Tally::default(), capital_taken: false, new_cities_taken: 0 }
    }

    fn human(&self) -> PlayerId {
        PlayerId(self.order.seat)
    }

    fn victim(&self) -> PlayerId {
        PlayerId(self.order.victim)
    }

    fn cities_of_victim(game: &GameState, victim: PlayerId) -> Vec<u64> {
        let auth = game.view().auth;
        let mut ids: Vec<u64> = auth.indexes().cities_of(victim).iter().filter_map(|h| auth.cities().get(*h)).map(|c| c.id.0).collect();
        ids.sort_unstable();
        ids
    }

    fn declare(&mut self, game: &mut GameState, rules: &CompiledRuleset) -> bool {
        let at_war = army::war(game, rules, self.human(), self.victim());
        if at_war && self.war_round.is_none() {
            self.war_round = Some(self.round);
        }
        at_war
    }

    /// The first turn: the trigger decides the stage.
    fn begin(&mut self, game: &mut GameState, rules: &CompiledRuleset) -> Stage {
        let target = match self.order.trigger {
            Trigger::AtOnce => classic_rules::combat::city::capital_of(game.view().auth, rules, self.victim()).and_then(|c| game.view().auth.cities().get(c)).map(|c| c.tile),
            Trigger::InPosition => army::nearest_city(game, rules, self.human(), self.victim(), &self.order.units),
            Trigger::NewCity => return Stage::Wait { known: Self::cities_of_victim(game, self.victim()) },
        };
        let Some(target) = target else { return Stage::Done };
        if self.order.trigger != Trigger::AtOnce {
            return Stage::Approach { target, rounds: 0 };
        }
        if self.declare(game, rules) {
            Stage::Siege { target }
        } else {
            Stage::Done
        }
    }

    /// Walking in at peace; at war with a strike the same turn once in position.
    fn approach(&mut self, game: &mut GameState, rules: &CompiledRuleset, target: TileId, rounds: u32, budget: &mut usize) -> Stage {
        let army = army::soldiers(game, rules, self.human(), &self.order.units);
        let ready = army.iter().all(|s| army::distance(game, s.tile, target) <= 2);
        if ready || rounds >= MAX_APPROACH_ROUNDS {
            if !self.declare(game, rules) {
                return Stage::Done;
            }
            return self.siege(game, rules, target, budget);
        }
        for s in &army {
            let ring = if s.range.is_some() { 2 } else { 1 };
            army::take_post(game, rules, s.handle, target, ring, budget);
        }
        army::hold_all(game, rules, self.human(), &self.order.units);
        Stage::Approach { target, rounds: rounds + 1 }
    }

    /// Waiting at home for a new city of the seat.
    fn wait(&mut self, game: &mut GameState, rules: &CompiledRuleset, known: Vec<u64>, budget: &mut usize) -> Stage {
        let auth = game.view().auth;
        let new_city = auth.indexes().cities_of(self.victim()).iter().filter_map(|h| auth.cities().get(*h)).filter(|c| !known.contains(&c.id.0)).map(|c| c.tile).next();
        let Some(target) = new_city else {
            army::hold_all(game, rules, self.human(), &self.order.units);
            return Stage::Wait { known };
        };
        if !self.declare(game, rules) {
            army::hold_all(game, rules, self.human(), &self.order.units);
            return Stage::Wait { known };
        }
        self.siege(game, rules, target, budget)
    }

    /// One turn of the siege; after a capture the next target (or the wait of the vulture).
    fn siege(&mut self, game: &mut GameState, rules: &CompiledRuleset, target: TileId, budget: &mut usize) -> Stage {
        let owner = game.view().auth.indexes().city_on_tile(target).and_then(|h| game.view().auth.cities().get(h)).map(|c| (c.owner, c.original_capital && c.original_owner == self.victim()));
        match owner {
            Some((owner, _)) if owner == self.victim() => {
                let before = self.tally.captured;
                self.tally.add(army::siege(game, rules, self.human(), self.victim(), &self.order.units, target, budget));
                if self.tally.captured > before {
                    return self.after_capture(game, rules, target);
                }
                Stage::Siege { target }
            }
            _ => self.after_capture(game, rules, target),
        }
    }

    fn after_capture(&mut self, game: &mut GameState, rules: &CompiledRuleset, taken: TileId) -> Stage {
        let city = game.view().auth.indexes().city_on_tile(taken).and_then(|h| game.view().auth.cities().get(h));
        if let Some(c) = city.filter(|c| c.owner == self.human()) {
            if c.original_capital && c.original_owner == self.victim() {
                self.capital_taken = true;
            }
            if self.order.trigger == Trigger::NewCity {
                self.new_cities_taken += 1;
            }
        }
        army::hold_all(game, rules, self.human(), &self.order.units);
        match self.order.trigger {
            Trigger::NewCity => Stage::Wait { known: Self::cities_of_victim(game, self.victim()) },
            _ => match army::nearest_city(game, rules, self.human(), self.victim(), &self.order.units) {
                Some(next) => Stage::Siege { target: next },
                None => Stage::Done,
            },
        }
    }
}

impl Seat for Play {
    fn player(&self) -> PlayerId {
        self.human()
    }

    fn play(&mut self, game: &mut GameState, rules: &CompiledRuleset) {
        self.round += 1;
        army::refuse_peace(game, rules, self.human());
        if army::soldiers(game, rules, self.human(), &self.order.units).is_empty() {
            self.stage = Stage::Done;
            return;
        }
        let mut budget = army::BUDGET;
        let stage = std::mem::replace(&mut self.stage, Stage::Done);
        self.stage = match stage {
            Stage::Start => match self.begin(game, rules) {
                Stage::Siege { target } => self.siege(game, rules, target, &mut budget),
                Stage::Approach { target, rounds } => self.approach(game, rules, target, rounds, &mut budget),
                Stage::Wait { known } => self.wait(game, rules, known, &mut budget),
                other => other,
            },
            Stage::Approach { target, rounds } => self.approach(game, rules, target, rounds, &mut budget),
            Stage::Wait { known } => self.wait(game, rules, known, &mut budget),
            Stage::Siege { target } => self.siege(game, rules, target, &mut budget),
            Stage::Done => {
                army::hold_all(game, rules, self.human(), &self.order.units);
                Stage::Done
            }
        };
    }

    fn counts(&self) -> BTreeMap<String, f64> {
        let flag = |b: bool| f64::from(u8::from(b));
        BTreeMap::from([
            ("script_war".to_owned(), flag(self.war_round.is_some())),
            ("script_war_round".to_owned(), f64::from(self.war_round.unwrap_or(0))),
            ("script_shots".to_owned(), f64::from(self.tally.shots)),
            ("script_kills".to_owned(), f64::from(self.tally.kills)),
            ("script_captured".to_owned(), f64::from(self.tally.captured)),
            ("script_capital_taken".to_owned(), flag(self.capital_taken)),
            ("script_new_cities_taken".to_owned(), f64::from(self.new_cities_taken)),
        ])
    }
}

// ---------------------------------------------------------------------------
// The scene

/// The nearest pair on one landmass: the seat and the human by `flip`; the sneak attack and the
/// vulture need them at peace.
pub(super) fn plan(trick: Trick, _rules: &CompiledRuleset, position: &Position, instance: usize) -> Result<Plan, String> {
    let game = &position.game;
    let (a, b, d) = nearest_pair(game).ok_or("no two nations share a landmass")?;
    let (seat, human) = if instance % 2 == 1 { (b, a) } else { (a, b) };
    let at_war = game.view().auth.diplomacy().at_war(seat, human);
    if at_war && trick != Trick::Rush {
        return Err("the nearest pair is at war already".to_owned());
    }
    let mut base = vec![Change::Meet { a: seat.0, b: human.0 }];
    if trick == Trick::Sneak {
        base.push(Change::Accord { giver: seat.0, receiver: human.0, accord: Accord::OpenBorders });
    }
    let info = BTreeMap::from([("pair_distance".to_owned(), d.to_string())]);
    Ok(Plan { roles: Roles::Human(Cast { trick, seat: seat.0, human: human.0 }), base, info })
}

/// The army of the human at `level`, raised at home nearest to the seat's target.
pub(super) fn edit_of(cast: Cast, level: usize, rules: &CompiledRuleset, position: &Position) -> Result<Edit, String> {
    let game = &position.game;
    let (seat, human) = (PlayerId(cast.seat), PlayerId(cast.human));
    let auth = game.view().auth;
    let target = match cast.trick {
        Trick::Rush => classic_rules::combat::city::capital_of(auth, rules, seat).and_then(|c| auth.cities().get(c)).map(|c| c.tile),
        _ => crate::battle::front_city(game, human, seat),
    }
    .ok_or("the seat has no city to aim at")?;
    let (kinds, techs) = army::army_kinds(game, rules, human)?;
    let (shape, trigger) = match cast.trick {
        Trick::Rush => ([(2, 1), (3, 2), (4, 2)][level], Trigger::AtOnce),
        Trick::Sneak => ([(2, 2), (3, 4), (5, 5)][level], Trigger::InPosition),
        _ => ([(1, 2), (2, 3), (3, 4)][level], Trigger::NewCity),
    };
    let mut changes = Vec::new();
    if !techs.is_empty() {
        changes.push(Change::Discover { player: human.0, techs });
    }
    changes.extend(raise(game, rules, human, target, kinds, shape)?);
    let order = Campaign { seat: human.0, victim: seat.0, trigger, units: Vec::new() };
    Ok(scripted(changes, Order::Campaign(order)))
}

/// `(ranged, melee)` units of the human on its land nearest to `target`; every third melee unit is
/// mounted.
pub(super) fn raise(game: &GameState, rules: &CompiledRuleset, human: PlayerId, target: TileId, kinds: ArmyKinds, (ranged, melee): (usize, usize)) -> Result<Vec<Change>, String> {
    let tiles = army::home_tiles(game, rules, human, target);
    let wanted = ranged + melee;
    if tiles.len() < wanted {
        return Err(format!("only {} free tiles at home for {wanted} units", tiles.len()));
    }
    let mut order = vec![kinds.ranged.0; ranged];
    order.extend((0..melee).map(|i| if i % 3 == 2 { kinds.mounted.0 } else { kinds.melee.0 }));
    Ok(tiles.into_iter().zip(order).map(|(tile, kind)| Change::Spawn { player: human.0, tile: tile.0, kind }).collect())
}
