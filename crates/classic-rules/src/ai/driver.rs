//! The classic-ruleset AI players: the kernel's turn driver (`kernel::ai::driver`)
//! deciding with the classic planners ([`ClassicMind`]) of the game's version of
//! the AI (`ai::variant`).

pub use kernel::ai::driver::RunReport;
pub use kernel::ai::{DriverPhase, StepOutcome};

use kernel::ai::mind::{AiMind, TurnKind, TurnStart};
use kernel::ai::plan::AiSettings;

use crate::ai::defence;
use crate::ai::capital_search::CapitalSearch;
use crate::ai::genome::Genome;
use crate::ai::grand_strategy::{self, Stakes};
use crate::ai::operations;
use crate::ai::peace::PeaceMaking;
use crate::ai::planner::{self, Decision, PlanTools, TurnBrief, UnitView};
use crate::ai::state::AiState;
use crate::ai::system::{AdvisorRegistry, Moment};
use crate::ai::trace::{Actor, Reason};
use crate::ai::variant::Variant;
use crate::classic::Classic;
use crate::command::Command;
use crate::world::player::ClassicKind;
use crate::world::player::PlayerKind;
use crate::game::ClassicGame;
use crate::ids::{PlayerId, UnitHandle};
use crate::knowledge::observation::{AIObservation, LocalObservation};
use crate::knowledge::public::PlayerQueries;
use crate::rules::CompiledRuleset;
use crate::world::state::GameState;

/// The AI of every non-human player of one classic game.
pub type AiDriver = kernel::ai::driver::AiDriver<Classic, ClassicMind>;

/// The classic planners (`ai::planner*`), the advisors of its systems and the
/// version of the AI that decides (`ai::variant`): the game plays
/// [`Variant::Standard`], the AI bench seats the others.
#[derive(Debug)]
pub struct ClassicMind {
    advisors: AdvisorRegistry,
    variant: Variant,
    /// Dev-only: the tuned knobs this mind decides with (`ai::genome`).
    genome: Option<Genome>,
}

/// The game's AI: the default variant with its own advisors (its diplomacy
/// makes peace the variant's way).
impl Default for ClassicMind {
    fn default() -> Self {
        Self::new(Variant::default())
    }
}

impl ClassicMind {
    pub fn new(variant: Variant) -> Self {
        let mut advisors = AdvisorRegistry::default();
        let diplomacy = crate::ai::diplomacy::DiplomacyAdvisor { peace: variant.peace_making() };
        advisors.register(crate::classic::ClassicSystem::Diplomacy, Box::new(diplomacy));
        Self { advisors, variant, genome: None }
    }

    /// The mind of `variant` deciding with the knobs of `genome` (dev-only,
    /// `ai::genome`); every other mind keeps the constants.
    pub fn with_genome(variant: Variant, genome: Genome) -> Self {
        Self { genome: Some(genome), ..Self::new(variant) }
    }

    pub fn variant(&self) -> Variant {
        self.variant
    }

    /// The system advisors (a module plugs in its own here).
    pub fn advisors_mut(&mut self) -> &mut AdvisorRegistry {
        &mut self.advisors
    }
}

impl AiMind<Classic> for ClassicMind {
    type Brief = TurnBrief;
    /// The whole-turn observation: everything the planners may read.
    type Sight<'g> = AIObservation<'g>;
    /// The observation around one unit.
    type Near<'g> = LocalObservation<'g>;

    fn sense<'g>(&self, game: &'g GameState, rules: &'g CompiledRuleset, player: PlayerId) -> AIObservation<'g> {
        game.ai_observation(rules, player)
    }

    fn sense_unit<'g>(
        &self,
        game: &'g GameState,
        rules: &'g CompiledRuleset,
        settings: &AiSettings,
        player: PlayerId,
        unit: UnitHandle,
    ) -> Option<LocalObservation<'g>> {
        let radius = settings.budget.local_radius;
        let (local, _) = game.ai_local_observation(rules, player, unit, radius)?;
        // An aircraft looks as far as it can strike from its base (`ai::air`).
        let reach = crate::ai::air::radius(rules, local.unit.kind, radius);
        if reach == radius {
            return Some(local);
        }
        game.ai_local_observation(rules, player, unit, reach).map(|(wide, _)| wide)
    }

    /// The barbarians play their orders (`ai::barbarians`); a city-state has
    /// no strategy of its own; a civilization plays the whole turn, its
    /// war operations first (`ai::operations`).
    fn start_turn(&self, sight: &AIObservation<'_>, rules: &CompiledRuleset, _player: PlayerId, state: &mut AiState) -> TurnStart<Classic, TurnBrief> {
        let _genome = self.genome.map(Genome::enter);
        let (mut start, stakes) = self.begin_turn(sight, rules, state, self.variant.allocates());
        if !self.variant.operates() || start.kind != TurnKind::Full {
            return start;
        }
        let mut routes = operations::Routes::new(sight.view);
        let gate = self.variant.war_gate(start.brief.prices.as_ref(), &stakes);
        let _span = prof::span("classic/operations_plan");
        let decisions = operations::plan(&start.brief, state, rules, &mut routes, gate);
        start.opening.splice(0..0, decisions);
        start
    }

    /// The strategy review knows nothing of the dangers: a defending AI
    /// arms again after it (`defence::arm`).
    fn plan_strategic(&self, brief: &TurnBrief, state: &mut AiState, rules: &CompiledRuleset) -> Vec<Decision> {
        let _genome = self.genome.map(Genome::enter);
        let mut out = if self.variant.operates() {
            operations_strategic(brief, state, rules, self.variant.peace_making())
        } else {
            planner::plan_strategic(brief, state, rules)
        };
        if let Some(watch) = &brief.defence {
            let _span = prof::span("classic/defence_arm");
            out.extend(defence::arm(brief, state, rules, watch));
        }
        out
    }

    fn decide_unit(
        &self,
        near: &LocalObservation<'_>,
        _player: PlayerId,
        _unit: UnitHandle,
        brief: &TurnBrief,
        state: &mut AiState,
        tools: &mut PlanTools<'_>,
    ) -> Option<Decision> {
        let _genome = self.genome.map(Genome::enter);
        let decision = match self.variant.settings(tools.settings) {
            Some(settings) => {
                let mut tools = PlanTools { rules: tools.rules, settings: &settings, scratch: tools.scratch, counters: tools.counters };
                self.decide(near, brief, state, &mut tools)
            }
            None => self.decide(near, brief, state, tools),
        };
        let decision = self.variant.adjust(decision, brief);
        Some(if brief.checks_orders { crate::ai::legal::unit_order(near, state, decision) } else { decision })
    }

    /// The last look before an order is sent: the game's own check against
    /// the game as it is now (`ai::legal`).
    fn permits(&self, game: &GameState, rules: &CompiledRuleset, player: PlayerId, command: &Command) -> bool {
        !self.variant.checks_orders() || game.accepts_order(rules, player, command)
    }

    fn plan_end_of_turn(
        &self,
        sight: &AIObservation<'_>,
        _player: PlayerId,
        brief: &TurnBrief,
        state: &AiState,
        tools: &mut PlanTools<'_>,
    ) -> Vec<Decision> {
        let _genome = self.genome.map(Genome::enter);
        let allocate = self.variant.allocates();
        match self.variant.settings(tools.settings) {
            Some(settings) => {
                let mut tools = PlanTools { rules: tools.rules, settings: &settings, scratch: tools.scratch, counters: tools.counters };
                self.end_of_turn(sight, brief, state, &mut tools, allocate)
            }
            None => self.end_of_turn(sight, brief, state, tools, allocate),
        }
    }
}

impl ClassicMind {
    /// The start of a player's turn; with `priced` the brief carries the
    /// allocator's prices of the turn (`ai::value`). A leader's prices are
    /// its own, and its stakes in a war are read when it may start one
    /// (`ai::grand_strategy`); the others' stakes are neutral. A defending
    /// AI sees its cities in danger and answers them first (`ai::defence`).
    fn begin_turn(
        &self,
        observation: &AIObservation<'_>,
        rules: &CompiledRuleset,
        state: &mut AiState,
        priced: bool,
    ) -> (TurnStart<Classic, TurnBrief>, Stakes) {
        let mut brief = {
            let _span = prof::span("classic/observe_brief");
            planner::observe(observation, state, rules)
        };
        brief.sound_orders = self.variant.sound_orders();
        brief.checks_orders = self.variant.checks_orders();
        brief.strikes = self.variant.strikes();
        brief.landings = self.variant.landings();
        brief.carriers = self.variant.carriers();
        if brief.checks_orders {
            brief.researchable = crate::ai::legal::researchable(observation, &brief, rules);
        }
        brief.plans = self.variant.plans();
        brief.air = self.variant.air_arm();
        if brief.strikes {
            brief.armory = crate::ai::strikes::armory(observation, &brief, rules, crate::ai::genome::get(crate::ai::genome::Knob::SuccessPercent));
        }
        if priced {
            let _span = prof::span("classic/allocator_prices");
            let prices = crate::ai::value::Prices::of_with(observation, rules, self.variant.price_model(), self.variant.science_pricing());
            if self.variant.sea_work() {
                brief.sea_jobs = crate::ai::sea_work::sea_values(observation, &brief, rules, &prices);
            }
            brief.prices = Some(prices);
        }
        let kind = observation.view.own_kind();
        let major = kind == PlayerKind::Major;
        let stakes = if self.variant.has_leaders() && major {
            lead(observation, &mut brief, state, rules)
        } else {
            Stakes::neutral()
        };
        let mut defended = Vec::new();
        if self.variant.defends() && major {
            let _span = prof::span("classic/defence_watch_respond");
            let mut watch = defence::watch(observation, &brief, state, rules);
            defended = defence::respond(observation, &brief, state, rules, &mut watch);
            brief.defence = Some(watch);
        }
        let units = brief.units.iter().map(|u| (u.id, u.handle, u.kind)).collect();
        let cities = brief.cities.len() as u32;
        let (kind, opening) = if kind == PlayerKind::Module(ClassicKind::Barbarian) {
            (TurnKind::Scripted, barbarian_decisions(observation, rules))
        } else {
            let mut opening = planner::advise(observation, &brief, state, rules, &self.advisors, Moment::StartOfTurn);
            opening.splice(0..0, defended);
            let kind = if matches!(kind, PlayerKind::Module(ClassicKind::CityState(_))) { TurnKind::NoStrategy } else { TurnKind::Full };
            (kind, opening)
        };
        (TurnStart { brief, kind, units, cities, opening }, stakes)
    }

    /// A unit's decision from one local observation: the front decides for
    /// its members near an enemy (`ai::front`), the planners for everything
    /// else.
    fn decide(&self, local: &LocalObservation<'_>, brief: &TurnBrief, state: &mut AiState, tools: &mut PlanTools<'_>) -> Decision {
        let view = UnitView { local, brief };
        let front = if self.variant.fights_as_front() {
            let _span = prof::span("classic/front_decide");
            crate::ai::front::decide(&view, state, tools)
        } else {
            None
        };
        front.unwrap_or_else(|| {
            let _span = prof::span("classic/planner_units_decide");
            crate::ai::planner_units::decide_unit(&view, state, tools)
        })
    }

    /// Production and the advisors at the end of the turn; with `allocate`
    /// the allocator chooses the valued orders and spends the gold
    /// (`ai::allocator`).
    fn end_of_turn(
        &self,
        observation: &AIObservation<'_>,
        brief: &TurnBrief,
        state: &AiState,
        tools: &mut PlanTools<'_>,
        allocate: bool,
    ) -> Vec<Decision> {
        let allocator = allocate.then(|| {
            let _span = prof::span("classic/allocator_new");
            crate::ai::allocator::Allocator::new(observation, brief, state, tools.rules)
        });
        planner::plan_end_of_turn(observation, brief, state, tools, &self.advisors, allocator.as_ref())
    }
}

/// The strategic phase of an AI with operations: the strategy review,
/// diplomacy without its wars (the operations, planned at the start of the
/// turn, declare them), peace in wars no operation fights, research (by
/// value per beaker when the allocator prices the turn). An AI that makes
/// peace by deal makes it alone only with city-states.
fn operations_strategic(brief: &TurnBrief, state: &mut AiState, rules: &CompiledRuleset, peace: PeaceMaking) -> Vec<Decision> {
    use crate::ai::{planner_research, planner_strategy};
    let alone = |decision: &Decision| match (peace, &decision.command) {
        (PeaceMaking::ByDeal, Some(Command::MakePeace { enemy, .. })) => brief.relation(*enemy).is_none_or(|r| r.kind != PlayerKind::Major),
        _ => true,
    };
    let mut out = Vec::new();
    {
        let _span = prof::span("classic/strategy_review");
        out.extend(planner_strategy::review_strategy(brief, state));
    }
    {
        let _span = prof::span("classic/diplomacy_relations");
        out.extend(planner_strategy::plan_relations(brief, state, brief.dims).into_iter().filter(alone));
    }
    {
        let _span = prof::span("classic/idle_wars_peace");
        let peace = operations::idle_wars(brief, state, &out);
        out.extend(peace.into_iter().filter(alone));
    }
    let _span = prof::span("classic/research_choice");
    let research = match &brief.prices {
        Some(prices) => planner_research::plan_research_valued(brief, rules, prices),
        None => planner_research::plan_research(brief, state.strategy.current, rules),
    };
    out.extend(research);
    out
}

/// A civilization's leader at the start of its turn: its path and prices
/// (`ai::grand_strategy`), the search for the capitals it values
/// (`ai::capital_search`), and its stakes in a war when it may start one or
/// fights one (a capture that wins the game forms longer and gathers its
/// army from farther, `ai::operations`).
fn lead(observation: &AIObservation<'_>, brief: &mut TurnBrief, state: &mut AiState, rules: &CompiledRuleset) -> Stakes {
    let personality = rules.module.personality(brief.civ);
    {
        let _span = prof::span("classic/grand_strategy");
        grand_strategy::turn(observation, brief, state, rules, &personality);
    }
    {
        let _span = prof::span("classic/capital_search");
        brief.search = CapitalSearch::of(observation, brief, state, &personality);
    }
    if !state.operations().is_empty() || operations::in_season(brief) {
        let _span = prof::span("classic/war_stakes");
        Stakes::of(observation, brief, state, &personality)
    } else {
        Stakes::neutral()
    }
}

/// The barbarian player's orders as decisions of their units.
fn barbarian_decisions(observation: &AIObservation<'_>, rules: &CompiledRuleset) -> Vec<Decision> {
    crate::ai::barbarians::barbarian_orders(observation, rules)
        .into_iter()
        .map(|command| Decision::act(Actor::Player, Reason::SystemAdvisor, command))
        .collect()
}
