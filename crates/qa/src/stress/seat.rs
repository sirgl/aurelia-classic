//! The scripted adversary (`Seat`): one seat whose trick is played by a small explicit plan, the
//! rest of that seat and every other seat played by the game's AI as usual. At the start of the
//! seat's turn, before its AI looks at the world, the play loop (`play::play_arm`) hands the world
//! to the script, which plays its part through `GameState::execute` (the boundary a human's commands
//! go through: the rules validate every one). Then the AI plays the seat's turn: the units the
//! script spent stay spent.
//!
//! A script is data (`Script`, kept in the saved scene) and a state machine (`Seat`, one
//! implementation per adversary group of work/human-playbook-triage.md).

use std::collections::BTreeMap;

use classic_rules::ids::{PlayerId, TileId, UnitHandle};
use classic_rules::world::player::ClassicPlayers;
use classic_rules::world::relations::DealItem;
use classic_rules::{Command, CompiledRuleset, GameState};
use kernel::ai::plan::{role_of, AiRole};
use kernel::ids::EntityId;
use serde::{Deserialize, Serialize};

mod deals;
mod pests;
mod war;

pub(crate) use deals::luxury_to_sell;

/// Commands one script may send in one turn (a guard against a plan that loops).
const COMMANDS_A_TURN: usize = 120;

/// A scripted seat.
pub trait Seat {
    /// The seat the script plays.
    fn player(&self) -> PlayerId;
    /// Plays the script's part of the seat's turn (the seat is the active player).
    fn play(&mut self, game: &mut GameState, rules: &CompiledRuleset);
    /// What the script counted (merged into the arm's measures as `script_*`).
    fn counts(&self) -> BTreeMap<String, f64>;
    /// Whether `command` of the seat's AI orders what the script plays (its units, its spies):
    /// the play loop refuses it, so the AI leaves the script's part alone (wave 3).
    fn holds(&self, _game: &GameState, _command: &Command) -> bool {
        false
    }
}

/// The command orders one of the units `held` (persistent ids).
fn orders_one_of(game: &GameState, command: &Command, held: &[u64]) -> bool {
    let Some(unit) = classic_rules::knowledge::actions::acting_unit(command) else { return false };
    game.view().auth.units().get(unit).is_some_and(|u| held.contains(&u.core.id.0))
}

/// The handle of a living unit by its persistent id.
fn handle(game: &GameState, id: u64) -> Option<UnitHandle> {
    game.view().auth.units().handle_of(EntityId(id))
}

/// The adversary of a scene, as data.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "script", rename_all = "snake_case")]
pub enum Script {
    /// P1 raid-hunt: the `units` of `seat` hunt `victim`'s civilians (capture) and pillage its
    /// improved tiles; they never attack a city or a combat unit.
    Raid {
        seat: u8,
        victim: u8,
        /// The units the edit spawned (`with_units`).
        #[serde(default)]
        units: Vec<u64>,
    },
    /// P4 deal-cycles: `seat` sells open borders to `victim` for its gold (the price
    /// halves each round it is refused, from `percent` of the victim's treasury), and declares war
    /// the round after the victim paid.
    SellThenWar { seat: u8, victim: u8, percent: u32 },
    /// P8 alliance-sweep: every round `seat` gifts `percent` of its treasury to the city-state it is
    /// nearest to winning as an ally (the most influence of those it is not the ally of).
    Sweep { seat: u8, percent: u32 },
    /// P6 kill-zone: the `units` of `seat` hold their tiles by its city nearest to `enemy`: the
    /// ranged ones shoot the most wounded enemy in range, the melee ones finish a wounded enemy next
    /// to them and go back to their tile, none advances; a civilian among them is the bait and
    /// never moves. The seat's AI leaves them alone.
    KillZone {
        seat: u8,
        enemy: u8,
        #[serde(default)]
        units: Vec<u64>,
    },
    /// P9 sea-hunter: the ships `units` of `seat` hunt `victim`'s units on the water (embarked
    /// land units and ships): shoot or ram the nearest, else sail to it.
    SeaHunt {
        seat: u8,
        victim: u8,
        #[serde(default)]
        units: Vec<u64>,
    },
    /// P10 air-nuke: the aircraft `units` of `seat` strike `victim` every round from their city:
    /// bombers the victim's strongest unit in range (else its city), fighters stand on interception,
    /// a nuclear weapon falls on the victim's largest city in range.
    AirStrike {
        seat: u8,
        victim: u8,
        #[serde(default)]
        units: Vec<u64>,
    },
    /// P11 pests (faith): the missionaries and prophets `units` of `seat` walk to `victim`'s cities
    /// that do not follow the seat's religion and spread it there.
    Faith {
        seat: u8,
        victim: u8,
        #[serde(default)]
        units: Vec<u64>,
    },
    /// P12 pests (spies): the spies of `seat` go to `victim`'s largest cities, one a city, and
    /// steal a technology whenever the seat has the right to.
    Spies { seat: u8, victim: u8 },
    /// P4L: `seat`, a friend, sells `victim` a luxury it lacks for a lump sum of `percent` of its
    /// treasury (halved each round it is refused) and declares war the round after the sale (the
    /// gold stays, the deal ends with the war).
    LuxuryThenWar { seat: u8, victim: u8, percent: u32 },
    /// P4B: `seat` pays `victim` (a friend) `gold` to declare war on `third`, then declares war on
    /// `victim` once it is at war with `third`.
    BribeThenWar { seat: u8, victim: u8, third: u8, gold: u32 },
    /// P4W white-peace fishing (trick D8): `seat`, losing its war on `victim`, offers white peace
    /// every `every` rounds until the victim takes it.
    PeaceFishing { seat: u8, victim: u8, every: u32 },
    /// A human trick (`humans::Order`).
    Human(super::humans::Order),
}

impl Script {
    /// The script with the units the edit spawned: a raid, a kill-zone, a hunt, a strike and a
    /// faith script play them.
    pub fn with_units(self, spawned: &[u64]) -> Script {
        let units = spawned.to_vec();
        match self {
            Script::Raid { seat, victim, .. } => Script::Raid { seat, victim, units },
            Script::KillZone { seat, enemy, .. } => Script::KillZone { seat, enemy, units },
            Script::SeaHunt { seat, victim, .. } => Script::SeaHunt { seat, victim, units },
            Script::AirStrike { seat, victim, .. } => Script::AirStrike { seat, victim, units },
            Script::Faith { seat, victim, .. } => Script::Faith { seat, victim, units },
            Script::Human(order) => Script::Human(order.with_units(spawned)),
            other => other,
        }
    }

    /// The state machine of the script.
    pub fn start(&self) -> Box<dyn Seat> {
        let p = |x: &u8| PlayerId(*x);
        match self {
            Script::Raid { seat, victim, units } => Box::new(Raid { seat: p(seat), victim: p(victim), units: units.clone(), captured: 0, pillaged: 0 }),
            Script::SellThenWar { seat, victim, percent } => Box::new(SellThenWar { seat: p(seat), victim: p(victim), percent: *percent, stage: Stage::Offer, offers: 0 }),
            Script::Sweep { seat, percent } => Box::new(Sweep { seat: p(seat), percent: *percent, gifted: 0 }),
            Script::KillZone { seat, enemy, units } => Box::new(war::KillZone::new(p(seat), p(enemy), units.clone())),
            Script::SeaHunt { seat, victim, units } => Box::new(war::SeaHunt::new(p(seat), p(victim), units.clone())),
            Script::AirStrike { seat, victim, units } => Box::new(war::AirStrike::new(p(seat), p(victim), units.clone())),
            Script::Faith { seat, victim, units } => Box::new(pests::Faith::new(p(seat), p(victim), units.clone())),
            Script::Spies { seat, victim } => Box::new(pests::Spies::new(p(seat), p(victim))),
            Script::LuxuryThenWar { seat, victim, percent } => Box::new(deals::Betrayal::new(p(seat), p(victim), deals::Bait::Luxury { percent: *percent })),
            Script::BribeThenWar { seat, victim, third, gold } => Box::new(deals::Betrayal::new(p(seat), p(victim), deals::Bait::Bribe { third: p(third), gold: *gold })),
            Script::PeaceFishing { seat, victim, every } => Box::new(deals::PeaceFishing::new(p(seat), p(victim), *every)),
            Script::Human(order) => order.start(),
        }
    }
}

fn distance(game: &GameState, a: TileId, b: TileId) -> i32 {
    game.view().auth.map().dimensions.distance(a, b).unwrap_or(i32::MAX)
}

// ---------------------------------------------------------------------------
// P1 raid-hunt

struct Raid {
    seat: PlayerId,
    victim: PlayerId,
    units: Vec<u64>,
    captured: u32,
    pillaged: u32,
}

/// What a raider goes for.
#[derive(Debug, Clone, Copy)]
enum Prey {
    /// A civilian of the victim (a capture by moving onto it).
    Civilian(TileId),
    /// An improved, unpillaged tile of the victim.
    Improvement(TileId),
}

impl Prey {
    fn tile(self) -> TileId {
        match self {
            Prey::Civilian(t) | Prey::Improvement(t) => t,
        }
    }
}

impl Raid {
    fn prey(&self, game: &GameState, rules: &CompiledRuleset) -> Vec<Prey> {
        let auth = game.view().auth;
        let map = auth.map();
        let mut prey: Vec<Prey> = auth
            .indexes()
            .units_of(self.victim)
            .iter()
            .filter_map(|h| auth.units().get(*h))
            .filter(|u| matches!(role_of(rules, u.core.kind), AiRole::Worker | AiRole::Settler | AiRole::Civilian))
            .filter(|u| auth.indexes().units_on(u.core.tile).iter().filter_map(|h| auth.units().get(*h)).all(|o| role_of(rules, o.core.kind) != AiRole::Military))
            .filter(|u| auth.indexes().city_on_tile(u.core.tile).is_none())
            .map(|u| Prey::Civilian(u.core.tile))
            .collect();
        for t in (0..map.tile_count() as u32).map(TileId) {
            let improved = map.improvement(t).flatten().is_some() && !map.pillaged(t);
            if improved && map.owner(t).flatten() == Some(self.victim) && auth.indexes().city_on_tile(t).is_none() && auth.indexes().units_on(t).is_empty() {
                prey.push(Prey::Improvement(t));
            }
        }
        prey
    }

    /// One unit's turn: pillage where it stands, else step towards the nearest prey.
    fn hunt(&mut self, game: &mut GameState, rules: &CompiledRuleset, unit: UnitHandle, budget: &mut usize) {
        while *budget > 0 {
            let auth = game.view().auth;
            let Some((moves, here)) = auth.units().get(unit).map(|u| (u.core.moves_left, u.core.tile)) else { return };
            if moves == 0 {
                return;
            }
            let map = auth.map();
            if map.owner(here).flatten() == Some(self.victim) && map.improvement(here).flatten().is_some() && !map.pillaged(here) {
                *budget -= 1;
                if game.execute(rules, Command::Pillage { unit }).is_ok() {
                    self.pillaged += 1;
                    continue;
                }
                return;
            }
            let prey = self.prey(game, rules);
            let Some(target) = prey.iter().min_by_key(|p| (distance(game, here, p.tile()), p.tile().0)).copied() else { return };
            let before = game.view().auth.indexes().units_of(self.victim).len();
            *budget -= 1;
            let command = if distance(game, here, target.tile()) == 1 { Command::MoveUnit { unit, destination: target.tile() } } else { Command::MarchTo { unit, destination: target.tile() } };
            if game.execute(rules, command).is_err() {
                return;
            }
            let after = game.view().auth.indexes().units_of(self.victim).len();
            if matches!(target, Prey::Civilian(_)) && after < before {
                self.captured += 1;
            }
            if game.view().auth.units().get(unit).is_some_and(|u| u.core.tile == here) {
                return;
            }
        }
    }
}

impl Seat for Raid {
    fn player(&self) -> PlayerId {
        self.seat
    }

    fn play(&mut self, game: &mut GameState, rules: &CompiledRuleset) {
        let mut budget = COMMANDS_A_TURN;
        for id in self.units.clone() {
            let Some(unit) = game.view().auth.units().handle_of(EntityId(id)) else { continue };
            if game.view().auth.units().get(unit).is_some_and(|u| u.core.owner == self.seat) {
                self.hunt(game, rules, unit, &mut budget);
            }
        }
    }

    fn counts(&self) -> BTreeMap<String, f64> {
        BTreeMap::from([("script_captured".to_owned(), f64::from(self.captured)), ("script_pillaged".to_owned(), f64::from(self.pillaged))])
    }
}

// ---------------------------------------------------------------------------
// P4 deal-cycles: sell, then war

/// Where the trick stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stage {
    /// Offering the sale (each round, at a lower price after a refusal).
    Offer,
    /// The victim paid: war at the seat's next turn.
    Paid,
    /// War declared: the script is done.
    War,
}

struct SellThenWar {
    seat: PlayerId,
    victim: PlayerId,
    percent: u32,
    stage: Stage,
    offers: u32,
}

impl Seat for SellThenWar {
    fn player(&self) -> PlayerId {
        self.seat
    }

    fn play(&mut self, game: &mut GameState, rules: &CompiledRuleset) {
        match self.stage {
            Stage::Offer => {
                let victim_gold = game.view().auth.players().treasury_of(self.victim);
                let price = (u64::from(victim_gold) * u64::from(self.percent) / 100 >> self.offers.min(6)) as u32;
                if price < 10 {
                    return;
                }
                self.offers += 1;
                let items = vec![(self.seat, DealItem::OpenBorders), (self.victim, DealItem::Gold(price))];
                let _ = game.execute(rules, Command::ProposeDeal { from: self.seat, to: self.victim, items });
                self.stage = Stage::Paid;
            }
            Stage::Paid => {
                let open = game.view().auth.module().relations().open_borders_until(self.seat, self.victim) > game.view().auth.turn().round;
                if !open {
                    // Refused (or lapsed): offer again at half the price.
                    self.stage = Stage::Offer;
                    self.play(game, rules);
                    return;
                }
                if game.execute(rules, Command::DeclareWar { player: self.seat, enemy: self.victim }).is_ok() {
                    self.stage = Stage::War;
                }
            }
            Stage::War => {}
        }
    }

    fn counts(&self) -> BTreeMap<String, f64> {
        let paid = matches!(self.stage, Stage::War);
        BTreeMap::from([("script_offers".to_owned(), f64::from(self.offers)), ("script_sold_then_war".to_owned(), f64::from(u8::from(paid)))])
    }
}

// ---------------------------------------------------------------------------
// P8 alliance-sweep

struct Sweep {
    seat: PlayerId,
    percent: u32,
    gifted: u32,
}

impl Seat for Sweep {
    fn player(&self) -> PlayerId {
        self.seat
    }

    fn play(&mut self, game: &mut GameState, rules: &CompiledRuleset) {
        let auth = game.view().auth;
        let players = auth.players();
        let states = auth.module().city_states();
        let target = (0..players.count() as u8)
            .map(PlayerId)
            .filter(|p| players.is_city_state(*p) && players.is_alive(*p) && states.ally(*p) != Some(self.seat))
            .filter(|p| auth.knowledge().get(usize::from(self.seat.0)).is_some_and(|k| k.contact(*p).met_round.is_some()))
            .max_by_key(|p| (states.influence(*p, self.seat), std::cmp::Reverse(p.0)));
        let Some(city_state) = target else { return };
        let gold = players.treasury_of(self.seat) * self.percent / 100;
        if gold < 50 {
            return;
        }
        if game.execute(rules, Command::GiftGold { player: self.seat, city_state, gold }).is_ok() {
            self.gifted += gold;
        }
    }

    fn counts(&self) -> BTreeMap<String, f64> {
        BTreeMap::from([("script_gifted".to_owned(), f64::from(self.gifted))])
    }
}
