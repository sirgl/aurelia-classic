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
#[cfg(feature = "experimental-ai")]
use crate::ai::net_e2e::mind::NetPlayer;
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
    /// The players that play themselves (`AiMind::note_humans`): no new
    /// aggressive aim targets them.
    humans: Vec<PlayerId>,
    /// The walks sent in the current turns, judged by their effect at the
    /// unit's next decision (`ai::effects`, `EffectChecks::moves`).
    walks: crate::ai::effects::Walks,
    /// Dev-only: a choice mode instead of the variant's (`ai::choice`; the
    /// dataset tool's exploration).
    choice: Option<crate::ai::choice::ChoiceMode>,
    /// `Variant::Net`: the end-to-end net that makes the unit commands,
    /// research, production and policies (`ai::net_e2e::mind`).
    #[cfg(feature = "experimental-ai")]
    net: Option<Box<NetPlayer>>,
    /// A league opponent's book (`ai::books`): the mind plays `PrePuppet` with it.
    book: Option<Box<crate::ai::books::BookPlayer>>,
    /// The variant by its name when the mind plays it as `PrePuppet`: a book, or
    /// the alias `siege-walls`; None: `variant`.
    named: Option<Variant>,
    /// The siege of walled cities (`ai::siege_walls`): `standard` since the seventh
    /// promotion, its alias `siege-walls`, `net` on it, and the books.
    walls: Option<Box<crate::ai::siege_walls::SiegeWalls>>,
    /// Dev-only: the decision of a captured city this mind takes instead of its
    /// variant's (`with_conquest`; the bench's `book@puppet-all`).
    conquest: Option<crate::ai::conquest::Conquest>,
    /// Unciv's AI (`ai::unciv`): `unciv` and `unciv-military`.
    unciv: Option<Box<crate::ai::unciv::UncivMind>>,
    /// Dev-only: the tall book's play and building order on another base
    /// (`book_on`: `tall-book@standard`); None: the variant's.
    tall: Option<(crate::ai::tall::Tall, crate::ai::tall::BuildingOrder)>,
}

/// The environment variable that makes the game's AI another variant
/// (dev-only: `AURELIA_AI_VARIANT=net` plays the net in the game window and
/// the observer; an unknown name is an error). Read only by the experimental
/// AI (`experimental-ai`): the game ignores it.
#[cfg(feature = "experimental-ai")]
pub const VARIANT_ENV: &str = "AURELIA_AI_VARIANT";

/// The game's AI: the default variant with its own advisors (its diplomacy
/// makes peace the variant's way), or the variant `AURELIA_AI_VARIANT`
/// names.
impl Default for ClassicMind {
    fn default() -> Self {
        #[cfg(feature = "experimental-ai")]
        let variant = match std::env::var(VARIANT_ENV) {
            Ok(name) if !name.is_empty() => Variant::from_name(&name).unwrap_or_else(|| panic!("{VARIANT_ENV}={name}: no such AI variant")),
            _ => Variant::default(),
        };
        #[cfg(not(feature = "experimental-ai"))]
        let variant = Variant::default();
        Self::new(variant)
    }
}

impl ClassicMind {
    pub fn new(variant: Variant) -> Self {
        let mut advisors = AdvisorRegistry::default();
        // The alias `siege-walls` plays as `PrePuppet` (the standard of the seventh promotion) in
        // everything, its peace too (before the seventh promotion it made peace as a variant
        // without leaders: `PeaceMaking::Alone`).
        let peace = if variant == Variant::SiegeWalls { Variant::PrePuppet } else { variant }.peace_making();
        let diplomacy = crate::ai::diplomacy::DiplomacyAdvisor { peace };
        advisors.register(crate::classic::ClassicSystem::Diplomacy, Box::new(diplomacy));
        #[cfg(feature = "experimental-ai")]
        let net = variant.net().then(|| Box::new(NetPlayer::from_env().unwrap_or_else(|e| panic!("{e}"))));
        let book = variant.book().map(|b| Box::new(crate::ai::books::BookPlayer::new(b)));
        // A book and the alias `siege-walls` play as `PrePuppet` under their names: the standard
        // they were measured on, which annexes a captured city of 4+ citizens (since the eighth
        // promotion `Standard` puppets it).
        let named = (book.is_some() || variant == Variant::SiegeWalls).then_some(variant);
        let walls = (book.is_some() || variant.siege_walls()).then(|| Box::new(crate::ai::siege_walls::SiegeWalls::new().joined(variant.siege_join())));
        let unciv = variant.unciv().any().then(|| Box::new(crate::ai::unciv::UncivMind::new(variant.unciv())));
        let variant = if named.is_some() { Variant::PrePuppet } else { variant };
        Self { advisors, variant, genome: None, humans: Vec::new(), walks: Default::default(), choice: None, #[cfg(feature = "experimental-ai")] net, book, named, walls, conquest: None, unciv, tall: None }
    }

    /// A league book that may play on another base (`book_on`): a duel book
    /// (`Variant::book`) or a tall book (`Tall::Book` and its attribution probes).
    pub fn rebases(book: Variant) -> bool {
        book.book().is_some() || book.tall().book_cities() || book.tall().book_policies()
    }

    /// Dev-only: the league book `book` played on `base` (the bench's `book@base`,
    /// classic/docs/ai-books.md «Re-based on the 2 November standard»): every switch of
    /// `base`, its siege (`Variant::siege_join`) and its decision of a captured city, with
    /// the book on top: a duel book's plan (`ai::books`) or the tall book's cities, policies,
    /// sites and building order (`Tall::Book`). The diplomacy stays the book's own, as
    /// measured. The books as measured are `book@pre-puppet` for a duel book (`new`) and
    /// `tall-book@pre-tall`. None: `book` is no league book, or `base` is one, the net or
    /// Unciv's AI.
    pub fn book_on(book: Variant, base: Variant) -> Option<Self> {
        if !Self::rebases(book) || Self::rebases(base) || base.net() || base.unciv().any() {
            return None;
        }
        let peace = crate::ai::diplomacy::DiplomacyAdvisor { peace: book.peace_making() };
        let mut mind = if book.book().is_some() {
            // `new` plays the book on `PrePuppet`: the base and its siege are `base`'s instead.
            let walls = crate::ai::siege_walls::SiegeWalls::new().joined(base.siege_join());
            Self { variant: base, walls: Some(Box::new(walls)), ..Self::new(book) }
        } else {
            Self { named: Some(book), tall: Some((book.tall(), book.building_order())), ..Self::new(base) }
        };
        mind.advisors.register(crate::classic::ClassicSystem::Diplomacy, Box::new(peace));
        Some(mind)
    }

    /// The mind of `variant` (normally `Variant::Net`) whose net choices
    /// come from `net` instead of the weights file `AURELIA_NET_WEIGHTS`
    /// names.
    #[cfg(feature = "experimental-ai")]
    pub fn with_net(variant: Variant, net: NetPlayer) -> Self {
        let mut advisors = AdvisorRegistry::default();
        let diplomacy = crate::ai::diplomacy::DiplomacyAdvisor { peace: variant.peace_making() };
        advisors.register(crate::classic::ClassicSystem::Diplomacy, Box::new(diplomacy));
        // The net plays on `NET_BASE`: its siege too.
        let walls = variant.siege_walls().then(|| Box::new(crate::ai::siege_walls::SiegeWalls::new().joined(variant.siege_join())));
        Self { advisors, variant, genome: None, humans: Vec::new(), walks: Default::default(), choice: None, net: Some(Box::new(net)), book: None, named: None, walls, conquest: None, unciv: None, tall: None }
    }

    /// The net's part (`Variant::Net`), if any.
    #[cfg(feature = "experimental-ai")]
    pub fn net(&self) -> Option<&NetPlayer> {
        self.net.as_deref()
    }

    /// The mind of `variant` choosing its production and research by
    /// `choice` (dev-only, `ai::choice`: the dataset's exploration).
    pub fn with_choice(variant: Variant, choice: crate::ai::choice::ChoiceMode) -> Self {
        Self { choice: Some(choice), ..Self::new(variant) }
    }

    /// This mind deciding a captured city by `conquest` instead of its variant's rule
    /// (dev-only, `ai::conquest`: a book that puppets, classic/docs/ai-conquest-payoff.md).
    pub fn with_conquest(self, conquest: crate::ai::conquest::Conquest) -> Self {
        Self { conquest: Some(conquest), ..self }
    }

    /// This mind with Unciv's families inside `standard` deciding but not played
    /// (dev-only, `ai::unciv::parts::DRY_ENV`: the proof that a hybrid is `standard`
    /// elsewhere).
    pub fn dry_unciv(mut self) -> Self {
        if let Some(unciv) = self.unciv.as_mut() {
            unciv.set_dry();
        }
        self
    }

    /// The mind of `variant` deciding with the knobs of `genome` (dev-only,
    /// `ai::genome`); every other mind keeps the constants.
    pub fn with_genome(variant: Variant, genome: Genome) -> Self {
        Self { genome: Some(genome), ..Self::new(variant) }
    }

    pub fn variant(&self) -> Variant {
        self.named.unwrap_or(self.variant)
    }

    /// A league opponent's book, if any (`ai::books`).
    pub fn book(&self) -> Option<&crate::ai::books::BookPlayer> {
        self.book.as_deref()
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

    fn note_humans(&mut self, humans: &[PlayerId]) {
        self.humans = humans.to_vec();
    }

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
        let mut radius = settings.budget.local_radius;
        #[cfg(feature = "experimental-ai")]
        if self.variant.unit_net() {
            // The unit net's tactician and its "who is next" see the whole theatre (`ai::unit_net`).
            radius = radius.max(crate::ai::unit_net::weights::fields::SIGHT);
        }
        if self.unciv.as_ref().is_some_and(|u| u.play().on()) {
            // Unciv's threat queries look around the tiles a unit reaches (`ai::unciv`).
            radius = radius.max(crate::ai::unciv::UNCIV_RADIUS);
        }
        let (local, _) = game.ai_local_observation(rules, player, unit, radius)?;
        if radius < crate::ai::unciv::UNCIV_RADIUS && self.unciv.as_ref().is_some_and(|u| u.part_sight(rules, local.unit.kind)) {
            // A unit one of Unciv's families commands looks as far as in `unciv` (`ai::unciv::parts`).
            return game.ai_local_observation(rules, player, unit, crate::ai::unciv::UNCIV_RADIUS).map(|(wide, _)| wide);
        }
        // An aircraft looks as far as it can strike from its base (`ai::air`).
        let reach = crate::ai::air::radius(rules, local.unit.kind, radius);
        if reach == radius {
            return Some(local);
        }
        game.ai_local_observation(rules, player, unit, reach).map(|(wide, _)| wide)
    }

    /// The barbarians play their orders (`ai::barbarians`); a city-state has
    /// no strategy of its own; a nation plays the whole turn, its
    /// war operations first (`ai::operations`).
    fn start_turn(&self, sight: &AIObservation<'_>, rules: &CompiledRuleset, _player: PlayerId, state: &mut AiState) -> TurnStart<Classic, TurnBrief> {
        let _genome = self.genome.map(Genome::enter);
        if self.variant.effect_checks().moves {
            self.walks.begin_turn(sight.player);
        }
        let (mut start, stakes) = self.begin_turn(sight, rules, state, self.variant.allocates());
        if let Some(unciv) = self.unciv.as_ref().filter(|u| u.full() && start.kind != TurnKind::Scripted) {
            // `unciv` plays the whole turn: Unciv's opening in place of the classic one (`ai::unciv`).
            unciv.begin_turn(sight, state);
            start.opening = unciv.full_opening(sight, rules, state, &start.brief);
            return start;
        }
        if let Some(unciv) = self.unciv.as_ref().filter(|_| start.kind != TurnKind::Scripted) {
            // Unciv's part replaces the orders of its units and the city strikes (`ai::unciv`).
            unciv.begin_turn(sight, state);
            start.opening.retain(|d| !unciv.replaces(rules, &sight.own_units, d));
            start.opening.splice(0..0, unciv.opening(sight, rules));
        }
        if self.variant.weak_fighter().is_some() {
            // A control fighter alone decides whether its land units strike.
            start.opening.retain(|d| !d.command.as_ref().is_some_and(|c| crate::ai::weak_fighter::strips(rules, &sight.own_units, c)));
        }
        #[cfg(feature = "experimental-ai")]
        if let Some(net) = &self.net {
            if net.begin(sight, rules, state) {
                let opening = std::mem::take(&mut start.opening);
                start.opening = net.replace_scripted(sight.player, opening, false, Some(sight), rules);
            }
        }
        if !self.variant.operates() || start.kind != TurnKind::Full {
            self.unciv_parts_opening(&mut start, sight, rules, state);
            return start;
        }
        let mut routes = operations::Routes::new(sight.view);
        let gate = self.variant.war_gate(start.brief.prices.as_ref(), &stakes);
        // A book's attack is its own operation: `standard` starts none while it plays.
        let gate = if self.book.as_ref().is_some_and(|b| b.holds_war(sight.player)) { operations::WarGate::Never } else { gate };
        let _span = prof::span("classic/operations_plan");
        let mut decisions = operations::plan(&start.brief, state, rules, &mut routes, gate);
        if let Some(book) = &self.book {
            let turn = book.start_turn(sight, &start.brief, state, rules, &mut routes, self.walls.as_deref());
            decisions.splice(0..0, turn.decisions);
            start.opening = book.hold_peace(sight.player, std::mem::take(&mut start.opening));
        }
        #[cfg(feature = "experimental-ai")]
        let decisions = match &self.net {
            Some(net) => net.replace_scripted(sight.player, decisions, false, Some(sight), rules),
            None => decisions,
        };
        start.opening.splice(0..0, decisions);
        if let Some(walls) = &self.walls {
            // The siege plans give their groups' shots (`ai::siege_walls`).
            let stripped = walls.plan_turn(sight, &start.brief, state, rules);
            start.opening.retain(|d| !d.command.as_ref().is_some_and(|c| stripped.strips(c)));
        }
        if let Some(unciv) = &self.unciv {
            // The operations' orders of Unciv's units are Unciv's too.
            start.opening.retain(|d| !unciv.replaces(rules, &sight.own_units, d));
        }
        #[cfg(feature = "experimental-ai")]
        if self.variant.unit_net() {
            // The commander's turn (`ai::unit_net`): its operations, orders and plan; the
            // tactician moves and strikes with the units of its theatres, not the advisors.
            let _span = prof::span("classic/unit_net_begin");
            let turn = crate::ai::unit_net::begin(sight, &start.brief, state, rules);
            start.opening.retain(|d| !d.command.as_ref().is_some_and(|c| crate::ai::unit_net::owns_command(&turn, sight, rules, c)));
            start.brief.unit_net = Some(std::sync::Arc::new(turn));
        }
        if self.variant.tactical() {
            let _span = prof::span("classic/tactical_observe");
            let tactics = crate::ai::tactical::Tactics::observe(sight, &start.brief, state, rules);
            // The plan shoots and sets up for its members: not the advisors' shots.
            start.opening.retain(|d| {
                !matches!(&d.command, Some(Command::RangedAttack { unit, .. } | Command::SetUpRangedAttack { unit }) if tactics.commands(*unit))
            });
            start.brief.tactics = Some(tactics);
        }
        self.unciv_parts_opening(&mut start, sight, rules, state);
        start
    }

    /// Unciv's units move in Unciv's priority order (`ai::unciv`); the others
    /// in the driver's.
    fn order_units(&self, brief: &TurnBrief, rules: &CompiledRuleset, order: &mut Vec<(crate::ids::EntityId, UnitHandle)>) {
        if let Some(unciv) = &self.unciv {
            unciv.order_units(rules, brief.player, order);
        }
    }

    /// The unit net's commander picks the next unit while a theatre is in
    /// contact (`ai::unit_net::next_unit`); every other mind keeps the queue.
    #[cfg(feature = "experimental-ai")]
    fn picks_next(&self, brief: &TurnBrief) -> bool {
        brief.unit_net.as_ref().is_some_and(|t| t.contact)
    }

    #[cfg(feature = "experimental-ai")]
    fn next_unit(&self, near: &LocalObservation<'_>, brief: &TurnBrief, state: &AiState, remaining: &[(crate::ids::EntityId, UnitHandle)]) -> Option<crate::ids::EntityId> {
        let _span = prof::span("classic/unit_net_next");
        crate::ai::unit_net::next_unit(near, brief, state, near.view.rules(), remaining)
    }

    /// The strategy review knows nothing of the dangers: a defending AI
    /// arms again after it (`defence::arm`).
    fn plan_strategic(&self, brief: &TurnBrief, state: &mut AiState, rules: &CompiledRuleset) -> Vec<Decision> {
        let _genome = self.genome.map(Genome::enter);
        if self.unciv.as_ref().is_some_and(|u| u.full()) {
            // `unciv` researches and makes war and peace in its opening.
            return Vec::new();
        }
        let mut out = if self.variant.operates() {
            operations_strategic(brief, state, rules, self.variant.peace_making())
        } else {
            planner::plan_strategic(brief, state, rules)
        };
        if let Some(watch) = &brief.defence {
            let _span = prof::span("classic/defence_arm");
            out.extend(defence::arm(brief, state, rules, watch));
        }
        if let Some(unciv) = self.unciv.as_ref().filter(|u| u.parts().any()) {
            // Unciv's families replace `standard`'s orders of theirs (`ai::unciv::parts`).
            out.retain(|d| !unciv.strips(rules, &|h| brief.units.iter().find(|u| u.handle == h).map(|u| u.kind), d));
        }
        let out = match &self.book {
            Some(book) => book.strategic(brief, rules, out),
            None => out,
        };
        #[cfg(feature = "experimental-ai")]
        let out = match &self.net {
            Some(net) => net.replace_scripted(brief.player, out, true, None, rules),
            None => out,
        };
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
        #[cfg(feature = "experimental-ai")]
        if let Some(net) = &self.net {
            let rules = tools.rules;
            let decided = if net.records() {
                // DAgger: the scripted mind's decision is the label.
                let mut expert = || self.decide_checked(near, brief, state, tools);
                net.decide_unit(near, rules, &mut expert)
            } else {
                net.decide_unit(near, rules, &mut || unreachable!("asked only while recording"))
            };
            if let Some(decision) = decided {
                return Some(decision);
            }
        }
        // A promotion before the unit moves and strikes (`ai::promo_fit`).
        if let Some(decision) = crate::ai::promo_fit::before_unit(&UnitView { local: near, brief }, brief.fight.promo) {
            return Some(decision);
        }
        #[cfg(feature = "experimental-ai")]
        if brief.unit_net.is_some() {
            // The unit net's tactician decides the units of its theatres (`ai::unit_net`).
            let view = UnitView { local: near, brief };
            if let Some(decision) = crate::ai::unit_net::decide(&view, state, tools) {
                return Some(decision);
            }
        }
        if let Some(unciv) = self.unciv.as_ref().filter(|u| u.full()) {
            let view = UnitView { local: near, brief };
            if let Some(decision) = unciv.decide_full(&view, state, tools) {
                return Some(decision);
            }
            // A Brave New World unit Unciv's AI does not know: `standard`'s.
            return Some(self.decide_checked(near, brief, state, tools));
        }
        if let Some(unciv) = self.unciv.as_ref().filter(|u| u.parts().any()) {
            // A settler, worker or religious unit of one of Unciv's families (`ai::unciv::parts`).
            let view = UnitView { local: near, brief };
            if let Some(decision) = unciv.part_decide(&view, tools) {
                return Some(decision);
            }
        }
        if let Some(unciv) = &self.unciv {
            // The mustering of an operation before its war stays `standard`'s (its war is
            // declared when the army has gathered); everything else of the unit is Unciv's.
            let mustering = state.task_of(near.unit.id).is_some_and(|t| match t.task {
                crate::ai::state::UnitTask::Operation { target } => state.operation(target).is_some_and(|o| o.phase == kernel::ai::state::OperationPhase::Forming),
                _ => false,
            });
            if !mustering {
                let view = UnitView { local: near, brief };
                if let Some(decision) = unciv.decide(&view, state, tools) {
                    return Some(decision);
                }
            }
        }
        if let Some(walls) = &self.walls {
            let view = UnitView { local: near, brief };
            if let Some(decision) = walls.decide(&view, state, tools) {
                return Some(decision);
            }
        }
        if let Some(book) = &self.book {
            let view = UnitView { local: near, brief };
            if let Some(decision) = book.assault(&view, state, tools) {
                return Some(decision);
            }
        }
        if self.book.is_none() && self.walls.is_some() {
            // The siege press of the game's AI (a book presses its own attack above).
            let view = UnitView { local: near, brief };
            if let Some(decision) = crate::ai::books::press_siege(&view, state, tools) {
                return Some(decision);
            }
        }
        if brief.effects.moves {
            let mut dropped = 0;
            let decision = self.walks.judged(near, state, &mut dropped, |state| self.decide_checked(near, brief, state, tools));
            tools.counters.tasks_dropped += dropped;
            return Some(decision);
        }
        Some(self.decide_checked(near, brief, state, tools))
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
        let rules = tools.rules;
        if let Some(unciv) = self.unciv.as_ref().filter(|u| u.full() && sight.view.own_kind() != PlayerKind::Module(ClassicKind::Barbarian)) {
            return unciv.full_end_of_turn(sight, rules, state, brief);
        }
        let scripted = match self.variant.settings(tools.settings) {
            Some(settings) => {
                let mut tools = PlanTools { rules: tools.rules, settings: &settings, scratch: tools.scratch, counters: tools.counters };
                self.end_of_turn(sight, brief, state, &mut tools, allocate)
            }
            None => self.end_of_turn(sight, brief, state, tools, allocate),
        };
        let scripted = match &self.book {
            Some(book) => book.end_of_turn(sight, brief, state, rules, scripted),
            None => scripted,
        };
        let scripted = match &self.unciv {
            Some(unciv) => scripted.into_iter().filter(|d| !unciv.replaces(rules, &sight.own_units, d)).collect(),
            None => scripted,
        };
        let scripted = match self.unciv.as_ref().filter(|u| u.parts().any() && sight.view.own_kind() != PlayerKind::Module(ClassicKind::Barbarian)) {
            // Unciv's families: `standard`'s orders of theirs dropped, Unciv's production and settlers added.
            Some(unciv) => {
                let kind_of = |h| sight.own_units.iter().find(|u| u.handle == h).map(|u| u.kind);
                let mut kept: Vec<Decision> = scripted.into_iter().filter(|d| !unciv.strips(rules, &kind_of, d)).collect();
                kept.extend(unciv.part_end_of_turn(sight, rules));
                kept
            }
            None => scripted,
        };
        #[cfg(feature = "experimental-ai")]
        let scripted = match &self.net {
            Some(net) => net.end_of_turn(sight, rules, scripted),
            None => scripted,
        };
        scripted
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
        brief.war = self.variant.war_tuning();
        brief.growth_policy = self.variant.growth_policy();
        brief.humans = self.humans.clone();
        if brief.checks_orders {
            brief.researchable = crate::ai::legal::researchable(observation, &brief, rules);
        }
        brief.plans = self.variant.plans();
        brief.air = self.variant.air_arm();
        brief.danger_net = self.variant.danger_net();
        brief.priced_stops = self.variant.priced_stops();
        brief.site_net = self.variant.site_net();
        brief.effects = self.variant.effect_checks();
        brief.levers = self.variant.science_levers();
        brief.space_finish = self.variant.space_finish();
        brief.abilities = self.variant.unit_abilities();
        brief.religion_fix = self.variant.religion_fix();
        brief.army_fit = self.variant.army_fit();
        brief.camps = self.variant.camps();
        brief.fog_march = self.variant.fog_march();
        brief.trade_fit = self.variant.trade_fit();
        (brief.tall, brief.building_order) = self.tall.unwrap_or_else(|| (self.variant.tall(), self.variant.building_order()));
        brief.conquest = self.conquest.unwrap_or_else(|| self.variant.conquest());
        brief.unciv = self.unciv.as_ref().map_or(crate::ai::unciv::UncivParts::NONE, |u| u.played_parts());
        brief.choice = self.choice.unwrap_or_else(|| self.variant.choice_mode());
        brief.workers = self.variant.workers_fit();
        brief.fight = self.variant.fight_fix();
        brief.no_idle_cities = self.variant.no_idle_cities();
        brief.scout_early = self.variant.scout_early();
        brief.siege = self.variant.siege_tactics();
        if brief.danger_net || brief.fight.any() || brief.siege.any() {
            brief.danger = Some(crate::ai::danger::Sight::take(observation, rules, state.seen_cities()));
        }
        if brief.strikes {
            brief.armory = crate::ai::strikes::armory(observation, &brief, rules, crate::ai::genome::get(crate::ai::genome::Knob::SuccessPercent));
        }
        if priced {
            let _span = prof::span("classic/allocator_prices");
            let mut prices = crate::ai::value::Prices::of_switches(observation, rules, self.variant.price_model(), self.variant.science_pricing(), self.variant.true_effects(), self.variant.cs_gift_cap());
            prices.unrest_steps = brief.war.unrest;
            prices.content_founding = self.variant.content_founding();
            if self.variant.space_finish() {
                prices.space = Some(crate::ai::value::SpaceDemand::of(observation, rules));
            }
            if brief.tall.plays() || brief.tall.book_cities() {
                let target = match brief.tall {
                    tall if tall.book_cities() => crate::ai::tall::BOOK_CITIES,
                    _ => crate::ai::tall::city_target(observation, prices.rounds_left),
                };
                prices.city_cap = Some(target);
                brief.tall_cities = Some(target);
                brief.tall_sites = crate::ai::tall::turn_site_bonuses(observation, rules, brief.tall, prices.rounds_left);
            }
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
        if brief.camps && major {
            crate::ai::camps::assign(observation, &brief, state, rules);
        }
        if self.variant.strategy_net() && major && !observation.own_cities.is_empty() {
            // The strategy net's plan (`ai::strategy_net`): its city count caps the founding.
            if let Some(prices) = brief.prices {
                let _span = prof::span("classic/strategy_net");
                // `hybrid` keeps the unit net's memory beside the strategy net's (`unit_net::memory`).
                #[cfg(feature = "experimental-ai")]
                let unit_part = if self.variant.unit_net() {
                    let (strategy, unit) = crate::ai::unit_net::memory::split(&state.mind_memory);
                    state.mind_memory = strategy;
                    Some(unit)
                } else {
                    None
                };
                let plan = crate::ai::strategy_net::think(observation, &brief, state, rules, &prices);
                #[cfg(feature = "experimental-ai")]
                if let Some(unit) = unit_part {
                    state.mind_memory = crate::ai::unit_net::memory::join(&state.mind_memory, &unit);
                }
                if let Some(p) = brief.prices.as_mut() {
                    p.city_cap = Some(plan.city_target);
                }
                brief.tall_cities = Some(plan.city_target);
                brief.strategy = Some(std::sync::Arc::new(plan));
            }
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

    /// Unciv's families inside `standard` at the start of a turn (`ai::unciv::parts`):
    /// `standard`'s orders of theirs dropped, Unciv's (research, policies, gold,
    /// faith) first.
    fn unciv_parts_opening(&self, start: &mut TurnStart<Classic, TurnBrief>, sight: &AIObservation<'_>, rules: &CompiledRuleset, state: &AiState) {
        let Some(unciv) = self.unciv.as_ref().filter(|u| u.parts().any() && start.kind != TurnKind::Scripted) else {
            return;
        };
        let kind_of = |h| sight.own_units.iter().find(|u| u.handle == h).map(|u| u.kind);
        start.opening.retain(|d| !unciv.strips(rules, &kind_of, d));
        start.opening.splice(0..0, unciv.part_opening(sight, rules, state));
    }

    /// A unit's decision as the variant changes it, checked by the rules
    /// when the variant checks its orders (`ai::legal`).
    fn decide_checked(&self, near: &LocalObservation<'_>, brief: &TurnBrief, state: &mut AiState, tools: &mut PlanTools<'_>) -> Decision {
        let decision = match self.variant.settings(tools.settings) {
            Some(settings) => {
                let mut tools = PlanTools { rules: tools.rules, settings: &settings, scratch: tools.scratch, counters: tools.counters };
                self.decide(near, brief, state, &mut tools)
            }
            None => self.decide(near, brief, state, tools),
        };
        let decision = self.variant.adjust(decision, brief);
        // The tall book on another base keeps exactly its cities (`book_on`).
        let decision = match (self.tall, self.named) {
            (Some(_), Some(book)) => book.adjust(decision, brief),
            _ => decision,
        };
        if brief.checks_orders { crate::ai::legal::unit_order(near, state, decision) } else { decision }
    }

    /// A unit's decision from one local observation: the front decides for
    /// its members near an enemy (`ai::front`), the planners for everything
    /// else.
    fn decide(&self, local: &LocalObservation<'_>, brief: &TurnBrief, state: &mut AiState, tools: &mut PlanTools<'_>) -> Decision {
        let view = UnitView { local, brief };
        if let Some(control) = self.variant.weak_fighter().and_then(|f| crate::ai::weak_fighter::decide(f, &view)) {
            return control;
        }
        let tactical = if self.variant.tactical() {
            let _span = prof::span("classic/tactical_decide");
            crate::ai::tactical::decide(&view, state, tools)
        } else {
            None
        };
        let front = if tactical.is_some() {
            tactical
        } else if self.variant.fights_as_front() {
            let _span = prof::span("classic/front_decide");
            crate::ai::front::decide(&view, state, tools)
        } else {
            None
        };
        let front = front.or_else(|| if brief.abilities { crate::ai::unit_abilities::sale(&view) } else { None });
        let decision = front.unwrap_or_else(|| {
            let _span = prof::span("classic/planner_units_decide");
            crate::ai::planner_units::decide_unit(&view, state, tools)
        });
        let decision = crate::ai::siege_tactics::settle(&view, state, decision);
        crate::ai::siege_tactics::dig_in(&view, decision)
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

/// A nation's leader at the start of its turn: its path and prices
/// (`ai::grand_strategy`), the search for the capitals it values
/// (`ai::capital_search`), and its stakes in a war when it may start one or
/// fights one (a capture that wins the game forms longer and gathers its
/// army from farther, `ai::operations`).
fn lead(observation: &AIObservation<'_>, brief: &mut TurnBrief, state: &mut AiState, rules: &CompiledRuleset) -> Stakes {
    let personality = rules.module.personality(brief.nation);
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
