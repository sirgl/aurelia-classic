//! What happened while an arm was played: the events of every round, read from the world (cities
//! that changed hands, treaties) and from the AI's decision trace (who declared war, who denounced,
//! who bought). The watch only reads.

use std::collections::BTreeMap;

use classic_rules::classic::Classic;
use classic_rules::ids::PlayerId;
use classic_rules::world::congress::ResolutionKind;
use classic_rules::world::victory::VictoryKind;
use classic_rules::ids::ProductionItem;
use classic_rules::{ClassicGame, Command, CompiledRuleset, GameState};
use kernel::ai::plan::{role_of, AiRole};
use kernel::ai::trace::{DecisionTrace, Outcome};
use kernel::ids::EntityId;
use kernel::rules::content::Domain;

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

/// A nation after a round.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sample {
    pub round: u32,
    pub player: PlayerId,
    /// Military units (any domain).
    pub military: u32,
    /// Workers, settlers and other civilians.
    pub civilians: u32,
    pub treasury: u32,
    /// The excess happiness of the nation's last turn (below 0: unhappy).
    pub happiness: i32,
    /// Tiles of its land whose improvement is pillaged.
    pub pillaged: u32,
    /// Aircraft (air domain units, missiles included).
    pub air: u32,
    /// Land units standing on water (embarked).
    pub at_sea: u32,
}

/// A city's head order and stock after a round.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Head {
    pub round: u32,
    pub item: Option<ProductionItem>,
    pub stock: u32,
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
    /// The unit kinds that are not combat units (`AiRole`): what a sample counts as civilians.
    civilian_kinds: Vec<u16>,
    /// The unit kinds of the air domain.
    air_kinds: Vec<u16>,
    /// The unit kinds of the land domain.
    land_kinds: Vec<u16>,
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
    /// Every major nation after each round.
    pub samples: Vec<Sample>,
    /// Gold gifts to city-states (round, giver).
    pub gifts: Vec<(u32, PlayerId)>,
    /// The head order of every city after each round, by city id.
    pub heads: BTreeMap<u64, Vec<Head>>,
    /// The pairs of majors at war after each round (the lower id first).
    pub wars: Vec<(u32, Vec<(u8, u8)>)>,
    /// The military units of each major alive after the last round read (id -> owner).
    military: BTreeMap<u64, PlayerId>,
    /// Military units each major lost (gone between two rounds: killed, captured, disbanded).
    pub losses: BTreeMap<u8, u32>,
    /// Orders of aircraft (strikes, sweeps, interceptions) by player, applied.
    pub air_orders: Vec<(u32, PlayerId)>,
    /// Interception orders by player, applied.
    pub interceptions: Vec<(u32, PlayerId)>,
    /// Faith answers by player, applied: removing heresy, a faith purchase, a spread.
    pub faith_orders: Vec<(u32, PlayerId)>,
    /// Spies a player sent to one of its own cities (counter-intelligence), applied.
    pub spies_home: Vec<(u32, PlayerId)>,
}

impl Watch {
    /// A watch of a game at the start of round `start`; it follows the units `tracked`.
    pub fn new(game: &GameState, rules: &CompiledRuleset, tracked: &[u64]) -> Self {
        let auth = game.view().auth;
        let start = auth.turn().round;
        let mut watch = Watch { seen_round: start.saturating_sub(1), ..Watch::default() };
        watch.civilian_kinds = (0..rules.units.len() as u16).filter(|k| role_of(rules, classic_rules::ids::UnitKindId(*k)) != AiRole::Military && role_of(rules, classic_rules::ids::UnitKindId(*k)) != AiRole::Naval).collect();
        watch.air_kinds = (0..rules.units.len() as u16).filter(|k| rules.units[usize::from(*k)].domain == Domain::Air).collect();
        watch.land_kinds = (0..rules.units.len() as u16).filter(|k| rules.units[usize::from(*k)].domain == Domain::Land).collect();
        watch.owners = auth.cities().iter().map(|(_, c)| (c.id.0, c.owner)).collect();
        watch.military = military_of(game, &watch.civilian_kinds);
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
        self.read_samples(game, round);
        self.read_losses(game);
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
                Command::GiftGold { .. } => self.gifts.push((round, by)),
                Command::Intercept { .. } => {
                    self.interceptions.push((round, by));
                    self.air_orders.push((round, by));
                }
                Command::AirSweep { .. } | Command::Nuke { .. } => self.air_orders.push((round, by)),
                Command::RemoveHeresy { .. } | Command::PurchaseWithFaith { .. } | Command::SpreadReligion { .. } => self.faith_orders.push((round, by)),
                Command::MoveSpy { player, city: Some(city), .. } => {
                    if game.view().auth.cities().get(*city).is_some_and(|c| c.owner == *player) {
                        self.spies_home.push((round, by));
                    }
                }
                Command::ProposeResolution { kind, choice, .. } => self.proposals.push(Proposal { round, by, kind: *kind, choice: *choice }),
                Command::AttackCity { city, .. } => {
                    if let Some(owner) = game.view().auth.cities().get(*city).map(|c| c.owner) {
                        self.strikes.push(Event { round, by, on: owner });
                    }
                }
                Command::RangedAttack { unit, target, .. } => {
                    let auth = game.view().auth;
                    if auth.units().get(*unit).is_some_and(|u| self.air_kinds.contains(&u.core.kind.0)) {
                        self.air_orders.push((round, by));
                    }
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

    fn read_samples(&mut self, game: &GameState, round: u32) {
        let auth = game.view().auth;
        let map = auth.map();
        let mut pillaged: BTreeMap<u8, u32> = BTreeMap::new();
        for t in (0..map.tile_count() as u32).map(classic_rules::ids::TileId) {
            if map.pillaged(t) {
                if let Some(owner) = map.owner(t).flatten() {
                    *pillaged.entry(owner.0).or_default() += 1;
                }
            }
        }
        for player in majors(game) {
            let (mut military, mut civilians, mut air, mut at_sea) = (0, 0, 0, 0);
            for unit in auth.indexes().units_of(player).iter().filter_map(|h| auth.units().get(*h)) {
                if self.civilian_kinds.contains(&unit.core.kind.0) {
                    civilians += 1;
                } else {
                    military += 1;
                }
                air += u32::from(self.air_kinds.contains(&unit.core.kind.0));
                at_sea += u32::from(self.land_kinds.contains(&unit.core.kind.0) && map.is_water(unit.core.tile));
            }
            let happiness = classic_rules::derived::happiness::excess_snapshot(auth, player);
            let treasury = auth.players().treasury_of(player);
            self.samples.push(Sample { round, player, military, civilians, treasury, happiness, pillaged: pillaged.get(&player.0).copied().unwrap_or(0), air, at_sea });
        }
        for (_, city) in auth.cities().iter() {
            self.heads.entry(city.id.0).or_default().push(Head { round, item: city.queue.first().copied(), stock: city.production });
        }
        let at_war = pairs(game).into_iter().filter(|(a, b)| auth.diplomacy().at_war(*a, *b)).map(|(a, b)| (a.0, b.0)).collect();
        self.wars.push((round, at_war));
    }

    /// The military units gone since the last round, counted against their owners.
    fn read_losses(&mut self, game: &GameState) {
        let now = military_of(game, &self.civilian_kinds);
        for (id, owner) in &self.military {
            if !now.contains_key(id) {
                *self.losses.entry(owner.0).or_default() += 1;
            }
        }
        self.military = now;
    }

    /// The samples of one nation, in round order.
    pub fn of(&self, player: u8) -> impl Iterator<Item = &Sample> {
        self.samples.iter().filter(move |s| s.player.0 == player)
    }

    /// Rounds after which the two were at war.
    pub fn war_rounds(&self, a: u8, b: u8) -> Vec<u32> {
        let pair = (a.min(b), a.max(b));
        self.wars.iter().filter(|(_, w)| w.contains(&pair)).map(|(r, _)| *r).collect()
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

/// The military units of the majors (any kind that is not a civilian), by id.
fn military_of(game: &GameState, civilian_kinds: &[u16]) -> BTreeMap<u64, PlayerId> {
    let auth = game.view().auth;
    let mut out = BTreeMap::new();
    for player in majors(game) {
        for unit in auth.indexes().units_of(player).iter().filter_map(|h| auth.units().get(*h)) {
            if !civilian_kinds.contains(&unit.core.kind.0) {
                out.insert(unit.core.id.0, player);
            }
        }
    }
    out
}

/// Every pair of living major nations, the lower id first.
fn pairs(game: &GameState) -> Vec<(PlayerId, PlayerId)> {
    let all = majors(game);
    all.iter().flat_map(|a| all.iter().filter(move |b| b.0 > a.0).map(move |b| (*a, *b))).collect()
}
