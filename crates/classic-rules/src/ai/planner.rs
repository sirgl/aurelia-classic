//! The AI planner (PLAN M6, R-07 AI §3): a simple sequential pipeline with
//! deterministic scoring and stable tie-breaks — no behavior tree, rule
//! engine or search over cloned worlds.
//!
//! The kernel's driver (`kernel::ai::driver`) calls it through the classic
//! mind (`ai::driver::ClassicMind`) at decision boundaries:
//! - `observe`: the full observation of the player's turn → memory update
//!   (contacts, threat, seen cities, dead units' tasks) and an owned
//!   `TurnBrief` of the turn;
//! - `plan_strategic`: strategy review, diplomacy, research (brief + memory
//!   only);
//! - `decide_unit`: one command (or none) for one unit from a fresh local
//!   observation around it;
//! - `plan_cities`: production and the per-system advisors, from a fresh
//!   full observation at the end of the turn.
//!
//! Everything reads `AIObservation`/`LocalObservation` (never the truth) and
//! writes only `AiState`; the world changes only through returned commands.

pub use kernel::ai::plan::{kind_strength, role_of, unit_power, AiRole, AiSettings, DecisionBudget, UnitSupply};

use crate::ai::state::AiState;
use crate::ai::system::{AdvisorInput, AdvisorRegistry, Moment};
use crate::ai::trace::{Actor, Reason};
use crate::classic::Classic;
use crate::ids::{CityHandle, EntityId, PlayerId, ProductionItem, TechId, TileId, UnitHandle, UnitKindId};
use crate::knowledge::observation::{AIObservation, LocalObservation, Relation};
use crate::math::MapDimensions;
use crate::rules::CompiledRuleset;

/// One decision: a command with its reason, or a reason for none.
pub type Decision = kernel::ai::plan::Decision<Classic>;

/// Everything a decision may use besides the observation and the memory.
pub type PlanTools<'a> = kernel::ai::plan::PlanTools<'a, Classic>;

/// The memory update and the brief of a player's turn (`Observe`).
pub fn observe(obs: &AIObservation<'_>, state: &mut AiState, rules: &CompiledRuleset) -> TurnBrief {
    crate::ai::planner_strategy::remember(obs, rules, state);
    let mut brief = TurnBrief::from_observation(obs, rules);
    if needs_territory(&brief, state) {
        let (edges, own_tiles) = territory_edges(obs);
        brief.territory = edges;
        brief.own_tiles = own_tiles;
    }
    brief
}

/// The known territory of the others matters at a strategy review and in a
/// war without a known enemy city.
fn needs_territory(brief: &TurnBrief, state: &AiState) -> bool {
    let review = brief.round >= state.strategy.next_review_round;
    let searching = state.relations().iter().any(|m| {
        let city_known = m
            .objective
            .is_some_and(|o| o.kind == crate::ai::state::ObjectiveKind::City);
        m.at_war && !city_known
    });
    review || searching
}

/// The known edge of one other player's territory: its known tile nearest
/// to our cities, preferring tiles next to unknown ones (going there shows
/// more of the territory).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TerritoryEdge {
    pub owner: PlayerId,
    pub tile: TileId,
    pub distance: i32,
    /// Known tiles of this player.
    pub known_tiles: u32,
}

/// One pass over the known map: every other player's territory edge and
/// the count of our own tiles.
fn territory_edges(obs: &AIObservation<'_>) -> (Vec<TerritoryEdge>, u32) {
    let dims = obs.view.dimensions();
    let cities: Vec<TileId> = obs.own_cities.iter().map(|c| c.tile).collect();
    let mut edges: Vec<(TerritoryEdge, bool)> = Vec::new();
    let mut own_tiles = 0;
    for index in 0..dims.tile_count() as u32 {
        let tile = TileId(index);
        if !obs.view.is_known(tile) {
            continue;
        }
        let known = obs.view.tile(tile);
        let Some(owner) = known.snapshot().and_then(|s| s.owner) else {
            continue;
        };
        if owner == obs.player {
            own_tiles += 1;
            continue;
        }
        let distance = cities
            .iter()
            .filter_map(|c| dims.distance(*c, tile))
            .min()
            .unwrap_or(i32::MAX);
        let frontier = crate::ai::planner_map::is_frontier(&obs.view, tile);
        note_edge(&mut edges, owner, tile, distance, frontier);
    }
    (edges.into_iter().map(|(edge, _)| edge).collect(), own_tiles)
}

fn note_edge(
    edges: &mut Vec<(TerritoryEdge, bool)>,
    owner: PlayerId,
    tile: TileId,
    distance: i32,
    frontier: bool,
) {
    let candidate = TerritoryEdge {
        owner,
        tile,
        distance,
        known_tiles: 1,
    };
    let Some(at) = edges.iter().position(|(e, _)| e.owner == owner) else {
        edges.push((candidate, frontier));
        return;
    };
    let (current, current_frontier) = edges[at];
    let better = (!frontier, distance, tile) < (!current_frontier, current.distance, current.tile);
    let known_tiles = current.known_tiles + 1;
    edges[at] = if better {
        (candidate, frontier)
    } else {
        (current, current_frontier)
    };
    edges[at].0.known_tiles = known_tiles;
}

/// Strategy review, diplomacy and research of a turn (`PlanStrategic`):
/// the brief and the memory only.
pub fn plan_strategic(
    brief: &TurnBrief,
    state: &mut AiState,
    rules: &CompiledRuleset,
) -> Vec<Decision> {
    let mut out = Vec::new();
    out.extend(crate::ai::planner_strategy::review_strategy(brief, state));
    out.extend(crate::ai::planner_strategy::plan_diplomacy(
        brief, state, brief.dims,
    ));
    out.extend(crate::ai::planner_research::plan_research(
        brief,
        state.strategy.current,
        rules,
    ));
    out
}

/// Production of every city and the proposals of the system advisors, from
/// a fresh full observation at the end of the turn.
pub fn plan_end_of_turn(
    obs: &AIObservation<'_>,
    brief: &TurnBrief,
    state: &AiState,
    tools: &mut PlanTools<'_>,
    advisors: &AdvisorRegistry,
    allocator: Option<&crate::ai::allocator::Allocator>,
) -> Vec<Decision> {
    let mut out = {
        let _span = prof::span("classic/plan_cities");
        crate::ai::planner_cities::plan_cities_with(obs, brief, state, tools, allocator)
    };
    out.extend(advise(
        obs,
        brief,
        state,
        tools.rules,
        advisors,
        Moment::EndOfTurn,
    ));
    out
}

/// The system advisors' commands for one moment of the turn, as decisions.
pub fn advise(
    obs: &AIObservation<'_>,
    brief: &TurnBrief,
    state: &AiState,
    rules: &CompiledRuleset,
    advisors: &AdvisorRegistry,
    moment: Moment,
) -> Vec<Decision> {
    let input = AdvisorInput {
        observation: obs,
        brief,
        state,
        rules,
    };
    advisors
        .propose_all(&input, moment)
        .into_iter()
        .map(|(_, proposal)| Decision::act(Actor::Player, Reason::SystemAdvisor, proposal.command))
        .collect()
}

/// One own city in the brief of a turn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CityBrief {
    pub handle: CityHandle,
    pub id: EntityId,
    pub tile: TileId,
    pub population: u32,
    pub production: u32,
    pub queue: Vec<ProductionItem>,
    pub hp: u16,
}

/// One own unit in the brief of a turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnitBrief {
    pub handle: UnitHandle,
    pub id: EntityId,
    pub tile: TileId,
    pub kind: UnitKindId,
    pub role: AiRole,
    pub hp: u16,
}

/// A foreign unit seen at the start of the turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sighting {
    pub tile: TileId,
    pub owner: PlayerId,
    pub power: u32,
    pub military: bool,
}

/// The owned summary of the full observation at the start of a player's
/// turn: strategic decisions read it, unit decisions read it beside their
/// fresh local observation.
#[derive(Debug, Clone)]
pub struct TurnBrief {
    pub player: PlayerId,
    pub nation: u8,
    pub round: u32,
    /// The game's speed: gates and horizons in rounds read it (Standard: the constants themselves).
    pub speed: kernel::rules::speeds::SpeedValues,
    pub dims: MapDimensions,
    pub cities: Vec<CityBrief>,
    pub units: Vec<UnitBrief>,
    pub treasury: u32,
    pub gold_income: i32,
    pub science: i32,
    /// Sorted.
    pub techs: Vec<TechId>,
    pub research_queue: Vec<TechId>,
    /// The technologies the rules let the player research now (their own
    /// check, `ChooseTechnology`), sorted; None when the variant asks the
    /// brief's own reading (`sound_orders` off, or a queue not empty).
    pub researchable: Option<Vec<TechId>>,
    /// Every order is checked with the rules' own check before it is chosen
    /// or sent (`ai::legal`, `Variant::checks_orders`).
    pub checks_orders: bool,
    /// The AI commands and builds missiles and nuclear weapons
    /// (`ai::strikes`, `Variant::strikes`).
    pub strikes: bool,
    /// The weapons the player has or could build, as the chance of an
    /// operation each gives (`strikes::armory`; empty without `strikes`).
    pub armory: Vec<crate::ai::strikes::Armed>,
    /// Paratroopers jump to their army's staging tile (`Variant::landings`).
    pub landings: bool,
    /// The AI builds and sails carriers (`ai::carrier`, `Variant::carriers`).
    pub carriers: bool,
    pub relations: Vec<Relation>,
    pub sightings: Vec<Sighting>,
    /// Sum of strength × health of own military units.
    pub military_power: u32,
    pub total_population: u32,
    /// Excess happiness of the empire now (`derived::happiness`).
    pub happiness: i32,
    /// Excess happiness of the turn's snapshot (the rules read it).
    pub turn_happiness: i32,
    /// Known territory of the others (filled at reviews and in wars without
    /// a known enemy city, else empty) and the count of our own tiles.
    pub territory: Vec<TerritoryEdge>,
    pub own_tiles: u32,
    /// Share of the map the player has explored, in percent.
    pub known_percent: u32,
    /// World wonders built anywhere, sorted (every player hears of them).
    pub built_wonders: Vec<crate::ids::BuildingKindId>,
    /// The allocator's prices of the turn (`ai::value`), for the variants
    /// that value by them; None for the others.
    pub prices: Option<crate::ai::value::Prices>,
    /// Orders are only those the rules accept (a building's resources, a
    /// unit's policy; `Variant::sound_orders`).
    pub sound_orders: bool,
    /// The leader plans its investments toward an aim (`Variant::plans`).
    pub plans: bool,
    /// The variant has an air arm: it builds and commands aircraft
    /// (`ai::air`, `Variant::air_arm`).
    pub air: bool,
    /// What each water deposit a work boat could improve is worth, best
    /// first (`ai::sea_work`); empty for the variants without work boats.
    pub sea_jobs: Vec<crate::ai::value::Centi>,
    /// The cities in danger this turn (`ai::defence`), for the variants
    /// that defend by value; None for the others.
    pub defence: Option<crate::ai::defence::Watch>,
    /// Where a leader's scouts look for the rival capitals it values
    /// (`ai::capital_search`); None for the others.
    pub search: Option<crate::ai::capital_search::CapitalSearch>,
    /// The leader's personality (`ai::taste`); neutral for the variants
    /// without leaders.
    pub personality: crate::rules::content::Personality,
    /// The changes of the war logic the variant carries (`operations::WarTuning`).
    pub war: crate::ai::operations::WarTuning,
    /// Where the cities stop growing for happiness (`Variant::growth_policy`).
    pub growth_policy: crate::ai::citizens::GrowthPolicy,
    /// The players that play themselves (`AiMind::note_humans`).
    pub humans: Vec<PlayerId>,
    /// The tactical layer's theatre and plan of the turn (`ai::tactical`),
    /// for the variants that fight with it.
    pub tactics: Option<crate::ai::tactical::Tactics>,
    /// The front and the tactical search rank the places of a unit by the
    /// danger net's expected loss (`ai::danger_net`, `Variant::danger_net`).
    pub danger_net: bool,
    /// The sight the danger of a unit decision reads beyond its local
    /// observation (`danger::Sight`), taken at the start of the turn with
    /// `danger_net`; None without it.
    pub danger: Option<crate::ai::danger::Sight>,
    /// The fixes of the combat defects the variant carries
    /// (`combat::FightFix`, `Variant::fight_fix`); with any, `danger` is
    /// taken too.
    pub fight: crate::ai::combat::FightFix,
    /// The siege-tactics switches the variant carries
    /// (`siege_tactics::SiegeTactics`, `Variant::siege_tactics`); with any,
    /// `danger` is taken too.
    pub siege: crate::ai::siege_tactics::SiegeTactics,
    /// The own stops the AI replaces by the price or risk of the rules
    /// (`planner_cities::PricedStops`, `Variant::priced_stops`).
    pub priced_stops: crate::ai::planner_cities::PricedStops,
    /// The settlers value a site worth a city by the site net
    /// (`ai::site_net`, `Variant::site_net`).
    pub site_net: bool,
    /// Which orders are judged by their effect (`ai::effects`,
    /// `Variant::effect_checks`).
    pub effects: crate::ai::effects::EffectChecks,
    /// The science levers priced by what they do (`ai::science_levers`,
    /// `Variant::science_levers`).
    pub levers: crate::ai::science_levers::ScienceLevers,
    /// The spaceship is built where it is finished soonest
    /// (`victory::space_plan`, `Variant::space_finish`).
    pub space_finish: bool,
    /// The units use the abilities the rules give them where they pay
    /// (`ai::unit_abilities`, `Variant::unit_abilities`).
    pub abilities: bool,
    /// The religion is priced by the rules' formulas (`ai::religion_fix`,
    /// `Variant::religion_fix`).
    pub religion_fix: bool,
    /// The cities order the unit kind that fits what it is for, by the rules' combat
    /// against the foes seen, and armies take ranged escorts (`ai::army_fit`,
    /// `Variant::army_fit`).
    pub army_fit: bool,
    /// Free military units clear the barbarian camps near the cities when it pays
    /// (`ai::camps`, `Variant::camps`).
    pub camps: bool,
    /// Its armies march through the land they have never seen
    /// (`planner_units::fog_approach`, `Variant::fog_march`).
    pub fog_march: bool,
    /// The tall play the variant carries (`ai::tall`, `Variant::tall`), the
    /// cities it keeps and what a site meeting a science building's condition
    /// adds (`tall::turn_site_bonuses`); Off, None and empty for the others.
    pub tall: crate::ai::tall::Tall,
    pub tall_cities: Option<u32>,
    pub tall_sites: Vec<(crate::ids::BuildingKindId, i32)>,
    /// The science buildings a city orders first (`tall::BuildingOrder`).
    pub building_order: crate::ai::tall::BuildingOrder,
    /// How a captured city is decided (`ai::conquest`, `Variant::conquest`).
    pub conquest: crate::ai::conquest::Conquest,
    /// How production and research choose among the allocator's best
    /// (`ai::choice`: its own choice, exploration, the choice net).
    pub choice: crate::ai::choice::ChoiceMode,
    /// How its workers are wanted and kept (`planner_workers::WorkersFit`,
    /// `Variant::workers_fit`); none of the switches for the others.
    pub workers: crate::ai::planner_workers::WorkersFit,
    /// A city with nothing within the allocator's limits takes the best process or
    /// the best order without the limits (`Variant::no_idle_cities`).
    pub no_idle_cities: bool,
    /// The first recon unit is built when the ruins it expects are worth it
    /// (`Variant::scout_early`, `scouting::recon_worth`).
    pub scout_early: bool,
    /// Trade routes as `trade-fit` plays them (`ai::trade`, `Variant::trade_fit`):
    /// any city may take a trade unit, routes priced by what they deliver and kept
    /// safe, stuck trade units rehomed, route slots worth their route in research.
    pub trade_fit: bool,
    /// The decision families Unciv's AI plays in place of the classic ones
    /// (`ai::unciv::parts`, `Variant::unciv`): with `settle` the allocator orders
    /// no settler, with `gold` it buys nothing.
    pub unciv: crate::ai::unciv::UncivParts,
    /// The strategy net's plan of the turn (`ai::strategy_net`, `Variant::strategy_net`):
    /// research, policies and the choice among the allocator's best orders follow it.
    /// None for the others.
    pub strategy: Option<std::sync::Arc<crate::ai::strategy_net::Plan>>,
    /// The unit net's turn (`ai::unit_net`, `Variant::unit_net`): the commander's
    /// operations, orders and plan; its tactician decides the units of a theatre.
    /// None for the others.
    #[cfg(feature = "experimental-ai")]
    pub unit_net: Option<std::sync::Arc<crate::ai::unit_net::UnitNetTurn>>,
}

/// Explored tiles of the map, in percent.
fn known_percent(view: &crate::knowledge::view::PlayerView<'_>) -> u32 {
    let tiles = view.tile_count().max(1);
    let known = (0..tiles as u32).filter(|t| view.is_known(TileId(*t))).count();
    (known * 100 / tiles) as u32
}

impl TurnBrief {
    pub fn from_observation(obs: &AIObservation<'_>, rules: &CompiledRuleset) -> Self {
        let units: Vec<UnitBrief> = obs.own_units.iter().map(|u| unit_brief(rules, u)).collect();
        let military_power = units
            .iter()
            .filter(|u| u.role == AiRole::Military)
            .map(|u| unit_power(rules, u.kind, u.hp))
            .sum();
        let mut techs = obs.economy.techs.clone();
        techs.sort_by_key(|t| t.0);
        Self {
            player: obs.player,
            nation: obs.economy.nation,
            round: obs.round,
            speed: obs.speed,
            dims: obs.view.dimensions(),
            cities: obs.own_cities.iter().map(city_brief).collect(),
            units,
            treasury: obs.economy.treasury,
            gold_income: obs.economy.income.gold,
            science: obs.economy.income.science,
            techs,
            research_queue: obs.economy.research_queue.clone(),
            researchable: None,
            checks_orders: false,
            strikes: false,
            armory: Vec::new(),
            landings: false,
            carriers: false,
            // Nations only: barbarians are always at war, city-states
            // have their own system (`ai::system`).
            relations: obs
                .relations
                .iter()
                .filter(|r| r.kind == crate::world::player::PlayerKind::Major)
                .copied()
                .collect(),
            sightings: sightings(obs, rules),
            military_power,
            total_population: obs.own_cities.iter().map(|c| c.population).sum(),
            happiness: obs.economy.happiness.excess(),
            turn_happiness: obs.economy.turn_happiness,
            territory: Vec::new(),
            own_tiles: 0,
            known_percent: known_percent(&obs.view),
            built_wonders: obs.built_wonders.clone(),
            prices: None,
            sound_orders: false,
            plans: false,
            air: false,
            sea_jobs: Vec::new(),
            defence: None,
            search: None,
            personality: crate::rules::content::Personality::NEUTRAL,
            war: crate::ai::operations::WarTuning::NONE,
            growth_policy: crate::ai::citizens::GrowthPolicy::STANDARD,
            humans: Vec::new(),
            tactics: None,
            danger_net: false,
            danger: None,
            fight: crate::ai::combat::FightFix::NONE,
            siege: crate::ai::siege_tactics::SiegeTactics::NONE,
            priced_stops: crate::ai::planner_cities::PricedStops::NONE,
            site_net: false,
            effects: crate::ai::effects::EffectChecks::NONE,
            levers: crate::ai::science_levers::ScienceLevers::NONE,
            space_finish: false,
            abilities: false,
            religion_fix: false,
            army_fit: false,
            camps: false,
            fog_march: false,
            tall: crate::ai::tall::Tall::Off,
            tall_cities: None,
            tall_sites: Vec::new(),
            building_order: crate::ai::tall::BuildingOrder::Off,
            conquest: crate::ai::conquest::Conquest::Threshold,
            choice: crate::ai::choice::ChoiceMode::Off,
            workers: crate::ai::planner_workers::WorkersFit::NONE,
            no_idle_cities: false,
            scout_early: false,
            trade_fit: false,
            unciv: crate::ai::unciv::UncivParts::NONE,
            strategy: None,
            #[cfg(feature = "experimental-ai")]
            unit_net: None,
        }
    }

    pub fn knows(&self, tech: TechId) -> bool {
        self.techs.binary_search_by_key(&tech.0, |t| t.0).is_ok()
    }

    pub fn relation(&self, other: PlayerId) -> Option<&Relation> {
        self.relations.iter().find(|r| r.other == other)
    }

    pub fn at_war(&self, other: PlayerId) -> bool {
        self.relation(other).is_some_and(|r| r.at_war)
    }

    /// The own city on `tile` is in danger this turn (`ai::defence`).
    pub fn endangered(&self, tile: TileId) -> bool {
        self.defence.as_ref().is_some_and(|watch| watch.danger(tile).is_some())
    }

    pub fn count_role(&self, role: AiRole) -> usize {
        self.units.iter().filter(|u| u.role == role).count()
    }

    /// The own city nearest to `tile` (stable: lower id on a tie).
    pub fn nearest_city(
        &self,
        dims: crate::math::MapDimensions,
        tile: TileId,
    ) -> Option<&CityBrief> {
        let distance = |c: &&CityBrief| dims.distance(c.tile, tile).unwrap_or(i32::MAX);
        self.cities.iter().min_by_key(|c| (distance(c), c.id))
    }
}

fn unit_brief(rules: &CompiledRuleset, u: &crate::knowledge::observation::OwnUnit) -> UnitBrief {
    UnitBrief {
        handle: u.handle,
        id: u.id,
        tile: u.tile,
        kind: u.kind,
        role: role_of(rules, u.kind),
        hp: u.hp,
    }
}

fn city_brief(c: &crate::knowledge::observation::OwnCity) -> CityBrief {
    CityBrief {
        handle: c.handle,
        id: c.id,
        tile: c.tile,
        population: c.population,
        production: c.income.production.max(0) as u32,
        queue: c.queue.clone(),
        hp: c.hp,
    }
}

fn sightings(obs: &AIObservation<'_>, rules: &CompiledRuleset) -> Vec<Sighting> {
    obs.visible_foreign_units
        .iter()
        .map(|u| Sighting {
            tile: u.tile,
            owner: u.owner,
            power: unit_power(rules, u.kind, u.hp),
            military: role_of(rules, u.kind) == AiRole::Military,
        })
        .collect()
}


/// The local observation of a unit decision with the brief of its turn.
pub struct UnitView<'o, 'a> {
    pub local: &'o LocalObservation<'a>,
    pub brief: &'o TurnBrief,
}
