//! What happened while an arm was played: the events of every round, read from the world (cities
//! that changed hands, treaties) and from the AI's decision trace (who declared war, who denounced,
//! who bought). The watch only reads.

use std::collections::BTreeMap;

use classic_rules::classic::Classic;
use classic_rules::ids::PlayerId;
use classic_rules::world::congress::ResolutionKind;
use classic_rules::world::victory::VictoryKind;
use classic_rules::{ClassicGame, Command, GameState};
use kernel::ai::trace::{DecisionTrace, Outcome};
use kernel::ids::EntityId;

use crate::bench::modules::majors;

/// A round's event between two nations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Event {
    pub round: u32,
    pub by: PlayerId,
    pub on: PlayerId,
}

/// A city that changed hands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Capture {
    pub round: u32,
    pub city: u64,
    pub from: PlayerId,
    pub to: PlayerId,
}

/// A congress proposal sent in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Proposal {
    pub round: u32,
    pub by: PlayerId,
    pub kind: ResolutionKind,
    pub choice: u32,
}

/// Units put in by the edit that the watch follows (B1's invaders).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tracked {
    pub id: u64,
    pub hp: u16,
}

#[derive(Debug, Clone, Default)]
pub struct Watch {
    seen_round: u32,
    owners: BTreeMap<u64, PlayerId>,
    pact_until: BTreeMap<(u8, u8), u32>,
    pub captures: Vec<Capture>,
    pub declared: Vec<Event>,
    pub denounced: Vec<Event>,
    pub peaces: Vec<Event>,
    /// Research agreements that began (`by` < `on`: the pair).
    pub pacts: Vec<Event>,
    pub purchases: Vec<(u32, PlayerId)>,
    /// Attacks whose target's owner could be read after the round.
    pub strikes: Vec<Event>,
    pub proposals: Vec<Proposal>,
    pub victory: Option<(u32, PlayerId, VictoryKind)>,
    tracked: Vec<Tracked>,
    /// The first round a followed unit was hurt or dead.
    pub first_hurt: Option<u32>,
    /// Followed units dead (or gone) now.
    pub tracked_dead: u32,
    /// The congress host after each round.
    pub hosts: Vec<(u32, Option<PlayerId>, bool)>,
}

impl Watch {
    /// A watch of a game at the start of round `start`; it follows the units `tracked`.
    pub fn new(game: &GameState, tracked: &[u64]) -> Self {
        let auth = game.view().auth;
        let start = auth.turn().round;
        let mut watch = Watch { seen_round: start.saturating_sub(1), ..Watch::default() };
        watch.owners = auth.cities().iter().map(|(_, c)| (c.id.0, c.owner)).collect();
        for pair in pairs(game) {
            watch.pact_until.insert((pair.0 .0, pair.1 .0), auth.module().relations().research_agreement_until(pair.0, pair.1));
        }
        watch.tracked = tracked
            .iter()
            .filter_map(|id| {
                let handle = auth.units().handle_of(EntityId(*id))?;
                Some(Tracked { id: *id, hp: auth.units().get(handle)?.core.hp })
            })
            .collect();
        watch
    }

    /// Reads the world and the trace after a round: everything the trace holds of rounds not read yet.
    pub fn observe(&mut self, game: &GameState, trace: &DecisionTrace<Classic>) {
        let round = game.view().auth.turn().round;
        self.read_cities(game, round);
        self.read_pacts(game, round);
        self.read_trace(game, trace);
        self.read_tracked(game, round);
        self.read_host(game, round);
        if self.victory.is_none() {
            self.victory = game.outcome().map(|o| (o.round, o.winner, o.kind));
        }
    }

    fn read_cities(&mut self, game: &GameState, round: u32) {
        let auth = game.view().auth;
        for (_, city) in auth.cities().iter() {
            match self.owners.insert(city.id.0, city.owner) {
                Some(before) if before != city.owner => self.captures.push(Capture { round, city: city.id.0, from: before, to: city.owner }),
                _ => {}
            }
        }
    }

    fn read_pacts(&mut self, game: &GameState, round: u32) {
        let relations = game.view().auth.module().relations();
        for (a, b) in pairs(game) {
            let until = relations.research_agreement_until(a, b);
            let before = self.pact_until.insert((a.0, b.0), until).unwrap_or(0);
            if until > before {
                self.pacts.push(Event { round, by: a, on: b });
            }
        }
    }

    fn read_trace(&mut self, game: &GameState, trace: &DecisionTrace<Classic>) {
        let mut newest = self.seen_round;
        for entry in trace.entries().filter(|e| e.round > self.seen_round && e.outcome == Outcome::Applied) {
            newest = newest.max(entry.round);
            let Some(command) = &entry.command else { continue };
            let by = entry.player;
            let round = entry.round;
            match command {
                Command::DeclareWar { enemy, .. } => self.declared.push(Event { round, by, on: *enemy }),
                Command::Denounce { target, .. } => self.denounced.push(Event { round, by, on: *target }),
                Command::MakePeace { enemy, .. } => self.peaces.push(Event { round, by, on: *enemy }),
                Command::HurryProduction { .. } | Command::PurchaseTile { .. } => self.purchases.push((round, by)),
                Command::ProposeResolution { kind, choice, .. } => self.proposals.push(Proposal { round, by, kind: *kind, choice: *choice }),
                Command::AttackCity { city, .. } => {
                    if let Some(owner) = game.view().auth.cities().get(*city).map(|c| c.owner) {
                        self.strikes.push(Event { round, by, on: owner });
                    }
                }
                Command::RangedAttack { target, .. } => {
                    let auth = game.view().auth;
                    let owner = auth
                        .indexes()
                        .city_on_tile(*target)
                        .and_then(|c| auth.cities().get(c))
                        .map(|c| c.owner)
                        .or_else(|| auth.indexes().units_on(*target).first().and_then(|u| auth.units().get(*u)).map(|u| u.core.owner));
                    if let Some(owner) = owner {
                        self.strikes.push(Event { round, by, on: owner });
                    }
                }
                Command::AttackUnit { defender, .. } => {
                    if let Some(owner) = game.view().auth.units().get(*defender).map(|u| u.core.owner) {
                        self.strikes.push(Event { round, by, on: owner });
                    }
                }
                _ => {}
            }
        }
        self.seen_round = newest;
    }

    fn read_tracked(&mut self, game: &GameState, round: u32) {
        let auth = game.view().auth;
        let mut dead = 0;
        for unit in &self.tracked {
            let hp = auth.units().handle_of(EntityId(unit.id)).and_then(|h| auth.units().get(h)).map(|u| u.core.hp);
            match hp {
                None => dead += 1,
                Some(hp) if hp < unit.hp && self.first_hurt.is_none() => self.first_hurt = Some(round),
                Some(_) => {}
            }
        }
        if dead > 0 && self.first_hurt.is_none() {
            self.first_hurt = Some(round);
        }
        self.tracked_dead = dead;
    }

    fn read_host(&mut self, game: &GameState, round: u32) {
        let auth = game.view().auth;
        let congress = auth.module().congress();
        if congress.founded() {
            let host = congress.host();
            let alive = host.is_some_and(|h| auth.players().is_alive(h));
            self.hosts.push((round, host, alive));
        }
    }

    /// One line about the round just read (`STRESS_TRACE`).
    pub fn line(&self, game: &GameState) -> String {
        format!(
            "r{} captures {} wars declared {} denounced {} pacts {} purchases {} strikes {} followed dead {} victory {:?}",
            game.view().auth.turn().round,
            self.captures.len(),
            self.declared.len(),
            self.denounced.len(),
            self.pacts.len(),
            self.purchases.len(),
            self.strikes.len(),
            self.tracked_dead,
            self.victory
        )
    }

    /// The followed units (B1).
    pub fn followed(&self) -> usize {
        self.tracked.len()
    }
}

/// Every pair of living major nations, the lower id first.
fn pairs(game: &GameState) -> Vec<(PlayerId, PlayerId)> {
    let all = majors(game);
    all.iter().flat_map(|a| all.iter().filter(move |b| b.0 > a.0).map(move |b| (*a, *b))).collect()
}
