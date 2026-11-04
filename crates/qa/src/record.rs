//! The recording mind: the classic AI's decisions written to a JSON Lines
//! file with what the AI saw and what came of each command, a dataset for
//! training a policy later. The recording changes nothing of the game: the
//! wrapper delegates every call of the AI contract (`kernel::ai::mind`) to
//! the mind it wraps and only writes what passes through; the world hash of
//! a recorded game equals the hash of the plain one.
//!
//! One JSON object a line, its kind in the field `t`:
//! - `game`: the spec of the game, first;
//! - `sight`: the whole-turn observation (`AIObservation`) of one player, at
//!   the start of its turn and again before its cities are planned;
//! - `near`: the observation around one unit (`LocalObservation`) a unit
//!   decision was made from;
//! - `decision`: one decision with the `seq` of the sight or near it was
//!   made from (`observation`; null on a round without sights);
//! - `outcome`: what the driver's trace says became of each decision of a
//!   player's turn, in trace order, after the round;
//! - `end`: the standings at the end.
//!
//! `seq` numbers the sights, nears and decisions in the order they were
//! written; a decision's `observation` is always an earlier `seq`.
//!
//! Outcomes against decisions: within one `(round, player)` the commands are
//! executed in the order they were decided (the strategic and opening ones,
//! then each unit's, then the end of the turn), so the n-th outcome with a
//! command of an actor is the n-th decision with a command of that actor in
//! the same turn; the decision carries its command as the same `{:?}` text.
//! Decisions without a command are traced when planned and carry
//! `no_command`. The trace also holds entries no decision made (the end of
//! the turn, an exhausted budget, an order the mind withheld as
//! `Forbidden`).
//!
//! A decision carries its command twice: as its `{:?}` text (`command`, what
//! the outcome lines match) and as the tree of named values of the command
//! codec (`action`, `PlayerQueries::command_value`: the form a policy reads
//! and writes). The strategic phase has no view to name entities with, so
//! its `action` is null. A near carries the legal commands of its unit
//! (`actions`, `knowledge::actions::unit_actions`, the same tree form), and
//! a unit decision the index of its command in that list (`choice`; null
//! when the kind is not enumerated, when the command is another front
//! member's, or when the list missed it, which `Written::missed` counts).
//! The parts of an observation that are deep module state (culture, faith,
//! tourism, congress, spies, steal rights, deal offers, victories,
//! resources, trade offers) are their `{:?}` text, in one string field
//! each, so nothing is lost.

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::io::{self, BufWriter, Write};
use std::path::Path;

use classic_rules::ai::driver::ClassicMind;
use classic_rules::ai::planner::TurnBrief;
use classic_rules::ai::variant::Variant;
use classic_rules::classic::Classic;
use classic_rules::knowledge::observation::{
    AIObservation, CityStateInfo, LocalObservation, ObservedCity, ObservedRoute, ObservedUnit, OwnCity, OwnEconomy, OwnUnit, Relation,
};
use classic_rules::knowledge::actions::{enumerated, unit_actions};
use classic_rules::knowledge::public::PlayerQueries;
use classic_rules::knowledge::view::{KnownTile, PlayerView};
use classic_rules::{ClassicGame, CompiledRuleset, GameState};
use kernel::ai::mind::{AiMind, TurnStart};
use kernel::ai::plan::{AiSettings, Decision, PlanTools};
use kernel::ai::state::AiState;
use kernel::ai::trace::{Actor, Outcome, TraceEntry};
use kernel::ai::DriverPhase;
use kernel::ids::{CityHandle, PlayerId, TileId, UnitHandle};
use kernel::replay::codec::Value;
use kernel::world::map::Route;
use serde::Serialize;

use crate::bench::modules::{standing, ClassicBench};
use crate::spec::{size_name, GameSpec};

/// One line of the file.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum Record {
    Game(GameRecord),
    Sight(SightRecord),
    Near(NearRecord),
    Decision(DecisionRecord),
    Outcome(OutcomeRecord),
    End(EndRecord),
}

impl Record {
    pub fn tag(&self) -> &'static str {
        match self {
            Record::Game(_) => "game",
            Record::Sight(_) => "sight",
            Record::Near(_) => "near",
            Record::Decision(_) => "decision",
            Record::Outcome(_) => "outcome",
            Record::End(_) => "end",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct GameRecord {
    pub seed: u64,
    pub map: String,
    pub size: String,
    /// The round of the time victory.
    pub rounds: u32,
    /// Every player of the game, city-states and barbarians included.
    pub players: u32,
    pub variant: String,
}

/// A tile the player knows, as it knows it (`KnownTile`).
#[derive(Debug, Clone, Serialize)]
pub struct TileRecord {
    pub index: u32,
    /// `remembered` or `visible`.
    pub known: &'static str,
    /// The round a remembered tile was last seen (null when visible).
    pub last_seen: Option<u32>,
    pub terrain: u16,
    pub feature: Option<u16>,
    pub resource: Option<u16>,
    pub improvement: Option<u16>,
    /// 0 none, 1 road, 2 railroad.
    pub route: u8,
    pub river: bool,
    pub natural_wonder: Option<u16>,
    pub owner: Option<u8>,
    pub city: bool,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct HandleRecord {
    pub index: u32,
    pub generation: u32,
}

impl From<UnitHandle> for HandleRecord {
    fn from(handle: UnitHandle) -> Self {
        Self { index: handle.index, generation: handle.generation }
    }
}

impl From<CityHandle> for HandleRecord {
    fn from(handle: CityHandle) -> Self {
        Self { index: handle.index, generation: handle.generation }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct OwnUnitRecord {
    pub handle: HandleRecord,
    pub id: u64,
    pub tile: u32,
    pub kind: u16,
    pub hp: u16,
    pub moves_left: u16,
    pub moves_max: u16,
    pub stance: String,
    pub fortified: bool,
    pub can_fortify: bool,
    pub can_attack: bool,
    pub experience: u32,
    pub level: u8,
    pub attacks_made: u8,
    pub promotions: Vec<u16>,
    pub work: Option<String>,
    pub goal: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CityIncomeRecord {
    pub food: i32,
    pub food_eaten: i32,
    pub food_surplus: i32,
    pub production: i32,
    pub gold: i32,
    pub science: i32,
    pub culture: u32,
    pub faith: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct OwnCityRecord {
    pub handle: HandleRecord,
    pub id: u64,
    pub tile: u32,
    pub population: u32,
    pub food: u32,
    pub production: u32,
    pub culture: u32,
    pub damage: u16,
    pub hp: u16,
    pub status: String,
    pub queue: Vec<String>,
    pub buildings: Vec<u16>,
    pub worked: Vec<u32>,
    pub income: CityIncomeRecord,
    pub citizens: String,
    pub process: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct EconomyRecord {
    pub nation: u8,
    pub treasury: u32,
    pub science: u32,
    pub gold_income: i32,
    pub science_income: i32,
    pub culture_income: u32,
    pub faith_income: u32,
    pub techs: Vec<u16>,
    pub research_queue: Vec<u16>,
    pub projects: Vec<u16>,
    /// The excess happiness now, and the whole report as text.
    pub happiness: i32,
    pub happiness_report: String,
    pub turn_happiness: i32,
    pub golden_age_threshold: u32,
    pub resources: String,
    pub trade_routes_left: u32,
    pub trade_offers: String,
    pub free_technology_options: Vec<u16>,
    pub free_great_person_options: String,
    pub may_annex: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct RelationRecord {
    pub other: u8,
    pub met_round: Option<u32>,
    pub last_seen_round: Option<u32>,
    pub at_war: bool,
    pub they_grant_entry: bool,
    pub we_grant_entry: bool,
    pub kind: String,
    pub peace_treaty_until: u32,
    pub war_since: u32,
    pub our_embassy: bool,
    pub their_embassy: bool,
    pub defensive_pact: bool,
    pub research_agreement: bool,
    pub friends: bool,
    pub we_denounced: bool,
    pub they_denounced: bool,
    pub warmonger: i32,
    pub can_open_borders: bool,
    pub pacts_known: u8,
}

#[derive(Debug, Clone, Serialize)]
pub struct CityStateRecord {
    pub player: u8,
    pub minor_trait: Option<String>,
    pub influence: i32,
    pub resting: i32,
    pub friends: bool,
    pub ally: Option<u8>,
    pub at_war: bool,
    pub pledged: bool,
    pub can_bully_gold: bool,
    pub bully_gold: u32,
    pub can_buyout: bool,
    pub quests: Vec<String>,
    pub capital: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ForeignUnitRecord {
    pub handle: HandleRecord,
    pub tile: u32,
    pub owner: u8,
    pub kind: u16,
    pub hp: u16,
}

#[derive(Debug, Clone, Serialize)]
pub struct ForeignCityRecord {
    pub handle: HandleRecord,
    pub tile: u32,
    pub owner: u8,
    pub population: u32,
    pub hp: u16,
}

#[derive(Debug, Clone, Serialize)]
pub struct RouteRecord {
    pub owner: u8,
    pub tile: u32,
    pub sea: bool,
}

/// What the player saw of the whole world (`AIObservation`).
#[derive(Debug, Clone, Serialize)]
pub struct SightRecord {
    pub seq: u64,
    pub round: u32,
    pub player: u8,
    /// The player's kind: a nation, a city-state or the barbarians.
    pub kind: String,
    pub map_width: u32,
    pub map_height: u32,
    /// The known tiles only.
    pub tiles: Vec<TileRecord>,
    pub own_units: Vec<OwnUnitRecord>,
    pub own_cities: Vec<OwnCityRecord>,
    pub economy: EconomyRecord,
    pub visible_foreign_units: Vec<ForeignUnitRecord>,
    pub visible_foreign_cities: Vec<ForeignCityRecord>,
    pub relations: Vec<RelationRecord>,
    pub city_states: Vec<CityStateRecord>,
    pub built_wonders: Vec<u16>,
    pub offers_to_us: String,
    pub culture: String,
    pub faith: String,
    pub tourism: String,
    pub congress: String,
    pub spies: String,
    pub steal: String,
    pub victories: String,
}

/// What the player saw around one unit (`LocalObservation`).
#[derive(Debug, Clone, Serialize)]
pub struct NearRecord {
    pub seq: u64,
    pub round: u32,
    pub player: u8,
    pub unit: OwnUnitRecord,
    pub radius: i32,
    /// The known tiles of the disc around the unit.
    pub tiles: Vec<TileRecord>,
    pub own_units: Vec<OwnUnitRecord>,
    pub own_cities: Vec<ForeignCityRecord>,
    pub foreign_units: Vec<ForeignUnitRecord>,
    pub foreign_cities: Vec<ForeignCityRecord>,
    pub foreign_routes: Vec<RouteRecord>,
    pub relations: Vec<RelationRecord>,
    /// The commands the rules accept for the unit now, in the enumerator's
    /// order (`knowledge::actions`), each as its tree of named values.
    pub actions: Vec<serde_json::Value>,
}

/// Who decided: `player`, `unit` or `city`, with the entity's id.
#[derive(Debug, Clone, Serialize)]
pub struct ActorRecord {
    pub kind: &'static str,
    pub id: Option<u64>,
}

impl From<Actor> for ActorRecord {
    fn from(actor: Actor) -> Self {
        match actor {
            Actor::Player => Self { kind: "player", id: None },
            Actor::Unit(id) => Self { kind: "unit", id: Some(id.0) },
            Actor::City(id) => Self { kind: "city", id: Some(id.0) },
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct DecisionRecord {
    pub seq: u64,
    pub round: u32,
    pub player: u8,
    /// `opening`, `strategic`, `unit` or `end_of_turn`.
    pub phase: &'static str,
    /// The `seq` of the sight or near the decision was made from.
    pub observation: Option<u64>,
    pub actor: ActorRecord,
    pub reason: String,
    pub command: Option<String>,
    /// The command as its tree of named values (null: no command, or the
    /// strategic phase).
    pub action: Option<serde_json::Value>,
    /// The index of the command in the near's `actions` (unit decisions of
    /// an enumerated kind only).
    pub choice: Option<usize>,
}

#[derive(Debug, Clone, Serialize)]
pub struct OutcomeRecord {
    pub round: u32,
    pub player: u8,
    /// The entry's place in the trace of the player's turn, from 0.
    pub index: u32,
    pub actor: ActorRecord,
    pub reason: String,
    pub command: Option<String>,
    /// `applied`, `rejected` or `no_command`.
    pub outcome: &'static str,
    /// Why the game rejected the command.
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PlayerEndRecord {
    pub player: u8,
    pub major: bool,
    pub alive: bool,
    pub score: i64,
    pub cities: u32,
    pub population: u32,
    pub techs: u32,
    pub military: u32,
    pub tiles: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct EndRecord {
    pub round: u32,
    pub winner: Option<u8>,
    pub victory: Option<&'static str>,
    pub players: Vec<PlayerEndRecord>,
}

fn route_code(route: Route) -> u8 {
    match route {
        Route::None => 0,
        Route::Road => 1,
        Route::Railroad => 2,
    }
}

/// The tile as the player knows it (None: unknown).
fn tile_record(known: KnownTile, index: u32) -> Option<TileRecord> {
    let (known, last_seen, s) = match known {
        KnownTile::Unknown => return None,
        KnownTile::Remembered { snapshot, last_seen_round } => ("remembered", Some(last_seen_round), snapshot),
        KnownTile::Visible(snapshot) => ("visible", None, snapshot),
    };
    Some(TileRecord {
        index,
        known,
        last_seen,
        terrain: s.terrain.0,
        feature: s.feature.map(|f| f.0),
        resource: s.resource.map(|r| r.0),
        improvement: s.improvement.map(|i| i.0),
        route: route_code(s.route),
        river: s.river,
        natural_wonder: s.natural_wonder.map(|w| w.0),
        owner: s.owner.map(|p| p.0),
        city: s.city,
    })
}

fn own_unit_record(u: &OwnUnit) -> OwnUnitRecord {
    OwnUnitRecord {
        handle: u.handle.into(),
        id: u.id.0,
        tile: u.tile.0,
        kind: u.kind.0,
        hp: u.hp,
        moves_left: u.moves_left,
        moves_max: u.moves_max,
        stance: format!("{:?}", u.stance),
        fortified: u.fortified,
        can_fortify: u.can_fortify,
        can_attack: u.can_attack,
        experience: u.experience,
        level: u.level,
        attacks_made: u.attacks_made,
        promotions: u.promotions.iter().map(|p| p.0).collect(),
        work: u.work.map(|w| format!("{w:?}")),
        goal: u.goal.map(|t| t.0),
    }
}

fn own_city_record(c: &OwnCity) -> OwnCityRecord {
    OwnCityRecord {
        handle: c.handle.into(),
        id: c.id.0,
        tile: c.tile.0,
        population: c.population,
        food: c.food,
        production: c.production,
        culture: c.culture,
        damage: c.damage,
        hp: c.hp,
        status: format!("{:?}", c.status),
        queue: c.queue.iter().map(|item| format!("{item:?}")).collect(),
        buildings: c.buildings.iter().map(|b| b.0).collect(),
        worked: c.worked.iter().map(|t| t.0).collect(),
        income: CityIncomeRecord {
            food: c.income.food,
            food_eaten: c.income.food_eaten,
            food_surplus: c.income.food_surplus,
            production: c.income.production,
            gold: c.income.gold,
            science: c.income.science,
            culture: c.income.culture,
            faith: c.income.faith,
        },
        citizens: format!("{:?}", c.citizens),
        process: c.process.map(|p| format!("{p:?}")),
    }
}

fn economy_record(e: &OwnEconomy) -> EconomyRecord {
    EconomyRecord {
        nation: e.nation,
        treasury: e.treasury,
        science: e.science,
        gold_income: e.income.gold,
        science_income: e.income.science,
        culture_income: e.income.culture,
        faith_income: e.income.faith,
        techs: e.techs.iter().map(|t| t.0).collect(),
        research_queue: e.research_queue.iter().map(|t| t.0).collect(),
        projects: e.projects.clone(),
        happiness: e.happiness.excess(),
        happiness_report: format!("{:?}", e.happiness),
        turn_happiness: e.turn_happiness,
        golden_age_threshold: e.golden_age_threshold,
        resources: format!("{:?}", e.resources),
        trade_routes_left: e.trade_routes_left,
        trade_offers: format!("{:?}", e.trade_offers),
        free_technology_options: e.free_technology_options.iter().map(|t| t.0).collect(),
        free_great_person_options: format!("{:?}", e.free_great_person_options),
        may_annex: e.may_annex,
    }
}

fn relation_record(r: &Relation) -> RelationRecord {
    RelationRecord {
        other: r.other.0,
        met_round: r.contact.met_round,
        last_seen_round: r.contact.last_seen_round,
        at_war: r.at_war,
        they_grant_entry: r.they_grant_entry,
        we_grant_entry: r.we_grant_entry,
        kind: format!("{:?}", r.kind),
        peace_treaty_until: r.peace_treaty_until,
        war_since: r.war_since,
        our_embassy: r.our_embassy,
        their_embassy: r.their_embassy,
        defensive_pact: r.defensive_pact,
        research_agreement: r.research_agreement,
        friends: r.friends,
        we_denounced: r.we_denounced,
        they_denounced: r.they_denounced,
        warmonger: r.warmonger,
        can_open_borders: r.can_open_borders,
        pacts_known: r.pacts_known,
    }
}

fn city_state_record(c: &CityStateInfo) -> CityStateRecord {
    CityStateRecord {
        player: c.player.0,
        minor_trait: c.minor_trait.map(|t| format!("{t:?}")),
        influence: c.influence,
        resting: c.resting,
        friends: c.friends,
        ally: c.ally.map(|p| p.0),
        at_war: c.at_war,
        pledged: c.pledged,
        can_bully_gold: c.can_bully_gold,
        bully_gold: c.bully_gold,
        can_buyout: c.can_buyout,
        quests: c.quests.iter().map(|q| format!("{q:?}")).collect(),
        capital: c.capital.map(|t| t.0),
    }
}

fn foreign_unit_record(u: &ObservedUnit) -> ForeignUnitRecord {
    ForeignUnitRecord { handle: u.handle.into(), tile: u.tile.0, owner: u.owner.0, kind: u.kind.0, hp: u.hp }
}

fn foreign_city_record(c: &ObservedCity) -> ForeignCityRecord {
    ForeignCityRecord { handle: c.handle.into(), tile: c.tile.0, owner: c.owner.0, population: c.population, hp: c.hp }
}

fn route_record(r: &ObservedRoute) -> RouteRecord {
    RouteRecord { owner: r.owner.0, tile: r.tile.0, sea: r.sea }
}

fn sight_record(seq: u64, sight: &AIObservation<'_>) -> SightRecord {
    let tiles = (0..sight.view.tile_count() as u32)
        .filter_map(|i| tile_record(sight.view.tile(TileId(i)), i))
        .collect();
    SightRecord {
        seq,
        round: sight.round,
        player: sight.player.0,
        kind: format!("{:?}", sight.view.own_kind()),
        map_width: sight.map_width,
        map_height: sight.map_height,
        tiles,
        own_units: sight.own_units.iter().map(own_unit_record).collect(),
        own_cities: sight.own_cities.iter().map(own_city_record).collect(),
        economy: economy_record(&sight.economy),
        visible_foreign_units: sight.visible_foreign_units.iter().map(foreign_unit_record).collect(),
        visible_foreign_cities: sight.visible_foreign_cities.iter().map(foreign_city_record).collect(),
        relations: sight.relations.iter().map(relation_record).collect(),
        city_states: sight.city_states.iter().map(city_state_record).collect(),
        built_wonders: sight.built_wonders.iter().map(|b| b.0).collect(),
        offers_to_us: format!("{:?}", sight.offers_to_us),
        culture: format!("{:?}", sight.culture),
        faith: format!("{:?}", sight.faith),
        tourism: format!("{:?}", sight.tourism),
        congress: format!("{:?}", sight.congress),
        spies: format!("{:?}", sight.spies),
        steal: format!("{:?}", sight.steal),
        victories: format!("{:?}", sight.victories),
    }
}

/// The command codec's tree as JSON: records are objects with their variant
/// under `kind`, lists are arrays, a gone handle is null.
pub fn json(value: &Value) -> serde_json::Value {
    match value {
        Value::Null => serde_json::Value::Null,
        Value::Bool(b) => (*b).into(),
        Value::Int(i) => (*i).into(),
        Value::Uint(u) => (*u).into(),
        Value::List(items) => items.iter().map(json).collect(),
        Value::Record { name, fields } => {
            let mut object = serde_json::Map::with_capacity(fields.len() + 1);
            object.insert("kind".to_owned(), (*name).into());
            for (field, value) in fields {
                object.insert((*field).to_owned(), json(value));
            }
            serde_json::Value::Object(object)
        }
    }
}

fn near_record(seq: u64, near: &LocalObservation<'_>, actions: &[Value]) -> NearRecord {
    let dims = near.view.dimensions();
    let tiles = classic_rules::math::tiles_in_hex_radius(&dims, near.unit.tile, near.radius)
        .into_iter()
        .filter_map(|t| tile_record(near.view.tile(t), t.0))
        .collect();
    NearRecord {
        seq,
        round: near.round,
        player: near.player.0,
        unit: own_unit_record(&near.unit),
        radius: near.radius,
        tiles,
        own_units: near.own_units.iter().map(own_unit_record).collect(),
        own_cities: near.own_cities.iter().map(foreign_city_record).collect(),
        foreign_units: near.foreign_units.iter().map(foreign_unit_record).collect(),
        foreign_cities: near.foreign_cities.iter().map(foreign_city_record).collect(),
        foreign_routes: near.foreign_routes.iter().map(route_record).collect(),
        relations: near.relations.iter().map(relation_record).collect(),
        actions: actions.iter().map(json).collect(),
    }
}

/// The outcome line of one trace entry.
pub fn outcome_record(entry: &TraceEntry<Classic>, index: u32) -> OutcomeRecord {
    let (outcome, error) = match &entry.outcome {
        Outcome::Applied => ("applied", None),
        Outcome::Rejected(error) => ("rejected", Some(format!("{error:?}"))),
        Outcome::NoCommand => ("no_command", None),
    };
    OutcomeRecord {
        round: entry.round,
        player: entry.player.0,
        index,
        actor: entry.actor.into(),
        reason: format!("{:?}", entry.reason),
        command: entry.command.as_ref().map(|c| format!("{c:?}")),
        outcome,
        error,
    }
}

/// The end line: every player's standing.
pub fn end_record(game: &GameState, rules: &CompiledRuleset) -> EndRecord {
    let players = game.view().auth.players();
    let outcome = game.outcome();
    EndRecord {
        round: game.view().auth.turn().round,
        winner: outcome.as_ref().map(|o| o.winner.0),
        victory: outcome.as_ref().map(|o| crate::spec::victory_name(o.kind)),
        players: (0..players.count() as u8)
            .map(PlayerId)
            .map(|player| {
                let s = standing::<ClassicBench>(game, rules, player);
                PlayerEndRecord {
                    player: player.0,
                    major: players.is_major(player),
                    alive: players.is_alive(player),
                    score: s.score,
                    cities: s.cities,
                    population: s.population,
                    techs: s.techs,
                    military: s.military,
                    tiles: s.tiles,
                }
            })
            .collect(),
    }
}

/// The file being written: the lines and bytes so far, and the first
/// write error (the mind's methods cannot return one; `flush` does).
struct Sink {
    out: BufWriter<Box<dyn Write>>,
    lines: BTreeMap<&'static str, u64>,
    bytes: u64,
    error: Option<io::Error>,
}

impl Sink {
    fn write(&mut self, record: &Record) {
        if self.error.is_some() {
            return;
        }
        let result = serde_json::to_vec(record)
            .map_err(io::Error::from)
            .and_then(|mut line| {
                line.push(b'\n');
                self.bytes += line.len() as u64;
                self.out.write_all(&line)
            });
        if let Err(error) = result {
            self.error = Some(error);
            return;
        }
        *self.lines.entry(record.tag()).or_default() += 1;
    }

    fn flush(&mut self) -> io::Result<()> {
        if let Some(error) = self.error.take() {
            self.error = Some(io::Error::new(error.kind(), error.to_string()));
            return Err(error);
        }
        self.out.flush()
    }
}

/// How much the recording wrote.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Written {
    /// Lines by tag.
    pub lines: BTreeMap<&'static str, u64>,
    pub bytes: u64,
    /// Unit decisions of an enumerated kind whose command the legal list did
    /// not hold (a gap of the enumerator; 0 is the invariant).
    pub missed: u64,
}

/// A mind whose sights, nears and decisions are written as they pass
/// through; every call is delegated to `P` unchanged. The driver takes the
/// mind by value: write the lines of your own (`write`) and flush through
/// `AiDriver::mind_mut`.
pub struct Recording<P> {
    mind: P,
    sink: RefCell<Sink>,
    seq: Cell<u64>,
    /// The `seq` of the last sight written for the turn in progress (None:
    /// a round without sights).
    sight: Cell<Option<u64>>,
    /// Full sights only on every K-th round.
    every: u32,
    missed: Cell<u64>,
}

impl<P: std::fmt::Debug> std::fmt::Debug for Recording<P> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Recording").field("mind", &self.mind).field("seq", &self.seq.get()).field("every", &self.every).finish_non_exhaustive()
    }
}

impl<P> Recording<P> {
    /// Record `mind` into `out`, sights on every `every`-th round (1: all).
    pub fn new(mind: P, out: Box<dyn Write>, every: u32) -> Self {
        Self {
            mind,
            sink: RefCell::new(Sink { out: BufWriter::new(out), lines: BTreeMap::new(), bytes: 0, error: None }),
            seq: Cell::new(0),
            sight: Cell::new(None),
            every: every.max(1),
            missed: Cell::new(0),
        }
    }

    pub fn to_file(mind: P, path: &Path, every: u32) -> io::Result<Self> {
        let file = std::fs::File::create(path)?;
        Ok(Self::new(mind, Box::new(file), every))
    }

    pub fn mind(&self) -> &P {
        &self.mind
    }

    /// Write one line of your own (the game, the outcomes, the end).
    pub fn write(&self, record: &Record) {
        self.sink.borrow_mut().write(record);
    }

    /// Write out what is buffered; the first write error, if any.
    pub fn flush(&self) -> io::Result<()> {
        self.sink.borrow_mut().flush()
    }

    pub fn written(&self) -> Written {
        let sink = self.sink.borrow();
        Written { lines: sink.lines.clone(), bytes: sink.bytes, missed: self.missed.get() }
    }

    fn next_seq(&self) -> u64 {
        let seq = self.seq.get() + 1;
        self.seq.set(seq);
        seq
    }

    fn records_round(&self, round: u32) -> bool {
        round % self.every == 0
    }

    /// A sight of the turn in progress: written on a recorded round, and
    /// the decisions of the turn refer to it.
    fn note_sight(&self, sight: &AIObservation<'_>) {
        if !self.records_round(sight.round) {
            self.sight.set(None);
            return;
        }
        let seq = self.next_seq();
        self.write(&Record::Sight(sight_record(seq, sight)));
        self.sight.set(Some(seq));
    }

    /// The near and the legal commands of its unit, which the unit's
    /// decision is then placed in.
    fn note_near(&self, near: &LocalObservation<'_>) -> (u64, Vec<Value>) {
        let seq = self.next_seq();
        let actions: Vec<Value> = unit_actions(near).iter().map(|c| near.view.command_value(c)).collect();
        self.write(&Record::Near(near_record(seq, near, &actions)));
        (seq, actions)
    }

    /// `view` names the entities of the commands' trees (None: the strategic
    /// phase, which has no view); `actions` is the near's legal list of a
    /// unit decision with the entity id of the near's unit. A front may give
    /// the command of another member (`ai::front`): that command is not the
    /// near unit's to list, and is neither placed nor missed.
    fn note_decisions(
        &self,
        round: u32,
        player: PlayerId,
        phase: &'static str,
        observation: Option<u64>,
        view: Option<&PlayerView<'_>>,
        actions: Option<(&[Value], u64)>,
        decisions: &[Decision<Classic>],
    ) {
        for decision in decisions {
            let seq = self.next_seq();
            let tree = match (&decision.command, view) {
                (Some(command), Some(view)) => Some(view.command_value(command)),
                _ => None,
            };
            let own = |tree: &Value, unit: u64| ["unit", "settler", "attacker"].iter().find_map(|f| tree.field(f)) == Some(&Value::Uint(unit));
            let choice = match (&decision.command, &tree, actions) {
                (Some(command), Some(tree), Some((actions, unit))) if enumerated(command) && own(tree, unit) => {
                    let at = actions.iter().position(|a| a == tree);
                    if at.is_none() {
                        self.missed.set(self.missed.get() + 1);
                    }
                    at
                }
                _ => None,
            };
            self.write(&Record::Decision(DecisionRecord {
                seq,
                round,
                player: player.0,
                phase,
                observation,
                actor: decision.actor.into(),
                reason: format!("{:?}", decision.reason),
                command: decision.command.as_ref().map(|c| format!("{c:?}")),
                action: tree.as_ref().map(json),
                choice,
            }));
        }
    }
}

impl<P> Drop for Recording<P> {
    fn drop(&mut self) {
        let _ = self.sink.get_mut().flush();
    }
}

/// The brief of the classic planners carries the round and the player the
/// strategic decisions are made for.
impl<P> AiMind<Classic> for Recording<P>
where
    P: for<'g> AiMind<Classic, Brief = TurnBrief, Sight<'g> = AIObservation<'g>, Near<'g> = LocalObservation<'g>>,
{
    type Brief = TurnBrief;
    type Sight<'g> = AIObservation<'g>;
    type Near<'g> = LocalObservation<'g>;

    fn note_humans(&mut self, humans: &[PlayerId]) {
        self.mind.note_humans(humans);
    }

    fn sense<'g>(&self, game: &'g GameState, rules: &'g CompiledRuleset, player: PlayerId) -> AIObservation<'g> {
        self.mind.sense(game, rules, player)
    }

    fn sense_unit<'g>(
        &self,
        game: &'g GameState,
        rules: &'g CompiledRuleset,
        settings: &AiSettings,
        player: PlayerId,
        unit: UnitHandle,
    ) -> Option<LocalObservation<'g>> {
        self.mind.sense_unit(game, rules, settings, player, unit)
    }

    fn start_turn(&self, sight: &AIObservation<'_>, rules: &CompiledRuleset, player: PlayerId, state: &mut AiState) -> TurnStart<Classic, TurnBrief> {
        self.note_sight(sight);
        let start = self.mind.start_turn(sight, rules, player, state);
        self.note_decisions(sight.round, player, "opening", self.sight.get(), Some(&sight.view), None, &start.opening);
        start
    }

    fn order_units(&self, brief: &TurnBrief, rules: &CompiledRuleset, order: &mut Vec<(classic_rules::ids::EntityId, classic_rules::ids::UnitHandle)>) {
        self.mind.order_units(brief, rules, order);
    }

    fn plan_strategic(&self, brief: &TurnBrief, state: &mut AiState, rules: &CompiledRuleset) -> Vec<Decision<Classic>> {
        let decisions = self.mind.plan_strategic(brief, state, rules);
        self.note_decisions(brief.round, brief.player, "strategic", self.sight.get(), None, None, &decisions);
        decisions
    }

    fn decide_unit(
        &self,
        near: &LocalObservation<'_>,
        player: PlayerId,
        unit: UnitHandle,
        brief: &TurnBrief,
        state: &mut AiState,
        tools: &mut PlanTools<'_, Classic>,
    ) -> Option<Decision<Classic>> {
        let (seq, actions) = self.note_near(near);
        let decision = self.mind.decide_unit(near, player, unit, brief, state, tools);
        if let Some(decision) = &decision {
            self.note_decisions(near.round, player, "unit", Some(seq), Some(&near.view), Some((&actions, near.unit.id.0)), std::slice::from_ref(decision));
        }
        decision
    }

    fn permits(&self, game: &GameState, rules: &CompiledRuleset, player: PlayerId, command: &classic_rules::Command) -> bool {
        self.mind.permits(game, rules, player, command)
    }

    fn plan_end_of_turn(
        &self,
        sight: &AIObservation<'_>,
        player: PlayerId,
        brief: &TurnBrief,
        state: &AiState,
        tools: &mut PlanTools<'_, Classic>,
    ) -> Vec<Decision<Classic>> {
        self.note_sight(sight);
        let decisions = self.mind.plan_end_of_turn(sight, player, brief, state, tools);
        self.note_decisions(sight.round, player, "end_of_turn", self.sight.get(), Some(&sight.view), None, &decisions);
        decisions
    }
}

/// The driver of a recorded game.
pub type RecordingDriver = kernel::ai::driver::AiDriver<Classic, Recording<ClassicMind>>;

/// A new game of `spec` (the bench's setup: `ClassicBench::new_game`).
pub fn new_game(rules: &CompiledRuleset, spec: &GameSpec) -> GameState {
    let (config, options) = spec.scenario(rules);
    classic_rules::scenario::generate_new_game_with(rules, &config, None, options)
}

/// What a recorded game came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Summary {
    /// The last round played.
    pub rounds: u32,
    pub written: Written,
    pub hash: u64,
}

/// Plays `spec` to its end (or `spec.victories.max_rounds`) with the
/// variant's mind recorded into `out`, outcome lines after every round, the
/// end line last. The file is flushed after every round and at the end.
pub fn record_game(rules: &CompiledRuleset, spec: &GameSpec, variant: Variant, every: u32, out: Box<dyn Write>) -> io::Result<Summary> {
    let mut game = new_game(rules, spec);
    let players = game.view().auth.players().count() as u32;
    let recording = Recording::new(ClassicMind::new(variant), out, every);
    recording.write(&Record::Game(GameRecord {
        seed: spec.seed,
        map: spec.script.key().to_owned(),
        size: size_name(spec.size).to_owned(),
        rounds: spec.victories.max_rounds,
        players,
        variant: variant.name().to_owned(),
    }));
    let settings = AiSettings::for_rules(rules, &spec.difficulty);
    let mut driver = RecordingDriver::with_mind(settings, Vec::new(), recording);
    driver.set_trace_capacity(usize::MAX);
    let last = spec.victories.max_rounds;
    while !game.is_over() && game.view().auth.turn().round <= last {
        let report = driver.run(&mut game, rules, u32::MAX);
        if !report.round_done && !game.is_over() {
            return Err(io::Error::other(format!("round {} waits for a human", game.view().auth.turn().round)));
        }
        write_outcomes(&mut driver);
        driver.mind_mut().flush()?;
    }
    let recording = driver.mind_mut();
    recording.write(&Record::End(end_record(&game, rules)));
    recording.flush()?;
    let written = recording.written();
    debug_assert_eq!(driver.phase(), DriverPhase::Idle);
    Ok(Summary { rounds: game.view().auth.turn().round, written, hash: kernel::replay::hash::authoritative_hash(&game) })
}

/// The outcome lines of the round's trace, indexed within each player's
/// turn; the trace starts afresh for the next round.
fn write_outcomes(driver: &mut RecordingDriver) {
    let mut turn: Option<(u32, PlayerId)> = None;
    let mut index = 0;
    let records: Vec<Record> = driver
        .trace()
        .entries()
        .map(|entry| {
            if turn != Some((entry.round, entry.player)) {
                turn = Some((entry.round, entry.player));
                index = 0;
            }
            let record = Record::Outcome(outcome_record(entry, index));
            index += 1;
            record
        })
        .collect();
    let recording = driver.mind_mut();
    for record in &records {
        recording.write(record);
    }
    driver.set_trace_capacity(usize::MAX);
}
