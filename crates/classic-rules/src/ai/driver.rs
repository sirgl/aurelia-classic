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
    /// The variant by its name when the mind plays it as another: a book or the alias
    /// `siege-walls` as `PrePuppet`, the alias `next-w` as `Standard` (the thirteenth
    /// promotion); None: `variant` (or the flags' variant, `variant()`).
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
    /// `Variant::Sharp`: the mind plays `Standard` (`variant`) with the strategy net's sharp choice
    /// and inputs (`TurnBrief::sharp`). Set with the next three for the game's AI since the
    /// thirteenth promotion (`Variant::Standard`, as `Variant::NextW`); `Variant::PreNext` plays
    /// `Standard`'s switches without any of them.
    sharp: bool,
    /// `Variant::FieldCraft`: the mind plays `Standard` (`variant`) with the flag (`TurnBrief::field_craft`).
    field: bool,
    /// `Variant::RaidWar`: the mind plays `Standard` (`variant`) with the flag (`TurnBrief::raid_war`).
    raidw: bool,
    /// `Variant::TallReal`: the mind plays `Sharp` (`sharp` set) whose tall style reaches for the
    /// science victory (`TurnBrief::tall_real`, `ai::tall_real`).
    tall_real: bool,
    /// `Variant::Next`: the mind plays `TallReal` (`tall_real` set) with the review's fixes
    /// (`TurnBrief::next`).
    next: bool,
    /// `Variant::WonderCancel` (1) and `WonderRace` (2): `Sharp` with the wonder-race switch
    /// (`TurnBrief::wonder_race`, `ai::wonder_race`).
    wonder_race: u8,
    /// `Variant::WarPays`: the mind plays `WarOpen` (`variant`) with the flag (`TurnBrief::war_pays`).
    pays: bool,
    /// `Variant::TacticianFit`: `pays` with the flag (`TurnBrief::tactician_fit`).
    fit: bool,
    /// `Variant::WarMuster`: `pays` with the muster fix (`TurnBrief::muster_fix`), scripted tactics.
    muster: bool,
    /// `Variant::SiegeCall`: `muster` with the call at the walls (`TurnBrief::siege_call`).
    call: bool,
    /// `Variant::Retake2`: `call` with the retake of a lost city (`TurnBrief::retake`).
    retake2: bool,
    /// `Variant::VoteGuard`: the mind plays `Standard` (`next-w`'s flags set) and takes the alliances
    /// of a rival that would win the World Leader vote (`TurnBrief::vote_guard`, `ai::victory_watch`).
    vote_guard: bool,
    /// `Variant::Landless`: the mind plays `Standard` with the goal of a player without a city
    /// (`TurnBrief::landless`).
    landless: bool,
    /// `Variant::LandlessPlan`: the mind plays `Standard` with the goal of a player without a city
    /// given to its strategist (`TurnBrief::landless_plan`).
    landless_plan: bool,
    /// The siege layer's mends on every turn, cities or not (`TurnBrief::siege_mend`, `mended`): the tests' switch.
    siege_mend: bool,
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
        // `sharp` is `Standard` in every switch but its strategy net (`TurnBrief::sharp`).
        // `tall-real` is `sharp` with its tall play (`TurnBrief::tall_real`).
        // `next` is `tall-real` with the review's fixes (`TurnBrief::next`).
        // `standard` (since the thirteenth promotion) and its alias `next-w` are `next` with the
        // wonder rule (`TurnBrief::wonder_race` = 1).
        // `next-war` is `standard` (the flags of `next-w` below) with the war of `siege-call` (its
        // flags and the switches of `WarOpen`, the variant `siege-call` plays).
        let requested = variant;
        // `next-war-retake` is `next-war` with the retake of `retake2` (its flag on top of the war of
        // `siege-call`).
        let (flags, war) = match requested {
            Variant::NextWar => (Variant::Standard, Variant::SiegeCall),
            Variant::NextWarRetake => (Variant::Standard, Variant::Retake2),
            // `next-x` (the joint build) is `next-war-retake` with the guard of `vote-guard` and the plan of
            // `landless-plan` (both below).
            Variant::NextX => (Variant::Standard, Variant::Retake2),
            _ => (requested, requested),
        };
        let joint = requested == Variant::NextX;
        // `vote-guard` is `standard` (the flags of `next-w`) with the guard on.
        let vote_guard = flags.vote_guard() || joint;
        // `landless` is `Standard` in every switch but its own (`TurnBrief::landless`).
        let landless = flags.landless();
        // `landless-plan` likewise (`TurnBrief::landless_plan`).
        let landless_plan = flags.landless_plan() || joint;
        let flags = if landless || landless_plan { Variant::Standard } else { flags };
        let next = flags.next() || vote_guard;
        let tall_real = flags.tall_real() || next;
        let wonder_race = flags.wonder_race().max(u8::from(vote_guard));
        let sharp = flags.sharp() || tall_real || wonder_race > 0;
        // `field-craft` and `raid-war` are `Standard` in every switch but their own (`TurnBrief::field_craft`,
        // `TurnBrief::raid_war`): the mind plays `PreNext`, as on their branch.
        let field = flags.field_craft();
        let raidw = flags.raid_war();
        // `tactician-fit` is `WarPays` in every switch but its own (`TurnBrief::tactician_fit`).
        let fit = war.tactician_fit();
        // `war-muster` is `WarPays` in every switch but the muster fix (`TurnBrief::muster_fix`).
        // `siege-call` is `WarMuster` with the call at the walls (`TurnBrief::siege_call`).
        // `retake2` is `SiegeCall` with the retake of a lost city (`TurnBrief::retake`).
        let retake2 = war.retake2();
        let call = war.siege_call() || retake2;
        let muster = war.war_muster() || call;
        // `war-pays` is `WarOpen` in every switch but its own (`TurnBrief::war_pays`).
        let pays = war.war_pays() || fit || muster;
        let variant = if pays {
            Variant::WarOpen
        } else if field || raidw {
            Variant::PreNext
        } else if sharp {
            Variant::Standard
        } else {
            variant
        };
        let mut advisors = AdvisorRegistry::default();
        // Every mind makes peace with a nation by a deal it values (`ai::peace`): the rules allow
        // no other. The alias `siege-walls` asks for embassies as `PrePuppet` (the standard of
        // the seventh promotion) does.
        let embassies = if variant == Variant::SiegeWalls { Variant::PrePuppet } else { variant }.values_embassies();
        advisors.register(crate::classic::ClassicSystem::Diplomacy, Box::new(crate::ai::diplomacy::DiplomacyAdvisor { embassies }));
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
        // The alias `next-w` plays as `Standard` under its name, `next-war` as `Standard` with the
        // war of `siege-call` under its own, `next-war-retake` as `Standard` with the war of
        // `retake2` under its own, `next-x` as `next-war-retake` with the guard and the landless
        // plan under its own.
        let named = named.or(matches!(requested, Variant::NextW | Variant::NextWar | Variant::NextWarRetake | Variant::NextX).then_some(requested));
        Self { advisors, variant, genome: None, humans: Vec::new(), walks: Default::default(), choice: None, #[cfg(feature = "experimental-ai")] net, book, named, walls, conquest: None, unciv, tall: None, sharp, tall_real, next, wonder_race, pays, fit, muster, call, retake2, vote_guard, field, raidw, landless, landless_plan, siege_mend: false }
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
    /// measured (its embassies; peace is every mind's, `ai::peace`). The books as measured
    /// are `book@pre-puppet` for a duel book (`new`) and `tall-book@pre-tall`. None: `book` is
    /// no league book, or `base` is one, the net or
    /// Unciv's AI.
    pub fn book_on(book: Variant, base: Variant) -> Option<Self> {
        if !Self::rebases(book) || Self::rebases(base) || base.net() || base.unciv().any() {
            return None;
        }
        // A base gives its switches, not the flags of the game's AI since the thirteenth
        // promotion: a book on `standard` plays on `pre-next`, as before the promotion.
        let base = if base == Variant::Standard { Variant::PreNext } else { base };
        let embassies = crate::ai::diplomacy::DiplomacyAdvisor { embassies: book.values_embassies() };
        let mut mind = if book.book().is_some() {
            // `new` plays the book on `PrePuppet`: the base and its siege are `base`'s instead.
            let walls = crate::ai::siege_walls::SiegeWalls::new().joined(base.siege_join());
            Self { variant: base, walls: Some(Box::new(walls)), ..Self::new(book) }
        } else {
            Self { named: Some(book), tall: Some((book.tall(), book.building_order())), ..Self::new(base) }
        };
        mind.advisors.register(crate::classic::ClassicSystem::Diplomacy, Box::new(embassies));
        Some(mind)
    }

    /// The mind of `variant` (normally `Variant::Net`) whose net choices
    /// come from `net` instead of the weights file `AURELIA_NET_WEIGHTS`
    /// names.
    #[cfg(feature = "experimental-ai")]
    pub fn with_net(variant: Variant, net: NetPlayer) -> Self {
        let mut advisors = AdvisorRegistry::default();
        advisors.register(crate::classic::ClassicSystem::Diplomacy, Box::new(crate::ai::diplomacy::DiplomacyAdvisor { embassies: variant.values_embassies() }));
        // The net plays on `NET_BASE`: its siege too.
        let walls = variant.siege_walls().then(|| Box::new(crate::ai::siege_walls::SiegeWalls::new().joined(variant.siege_join())));
        Self { advisors, variant, genome: None, humans: Vec::new(), walks: Default::default(), choice: None, net: Some(Box::new(net)), book: None, named: None, walls, conquest: None, unciv: None, tall: None, sharp: false, tall_real: false, next: false, wonder_race: 0, pays: false, fit: false, muster: false, call: false, retake2: false, vote_guard: false, field: false, raidw: false, landless: false, landless_plan: false, siege_mend: false }
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

    /// This mind with the siege layer's mends on every turn, with cities or without (`TurnBrief::siege_mend`): the
    /// tests that the mends hold for an army with cities, and a later measure of them for `standard`. No variant
    /// plays it.
    pub fn mended(self) -> Self {
        Self { siege_mend: true, ..self }
    }

    /// The mind of `variant` deciding with the knobs of `genome` (dev-only,
    /// `ai::genome`); every other mind keeps the constants.
    pub fn with_genome(variant: Variant, genome: Genome) -> Self {
        Self { genome: Some(genome), ..Self::new(variant) }
    }

    pub fn variant(&self) -> Variant {
        // The joint build's candidate carries the flags below under its own name.
        if self.named == Some(Variant::NextX) {
            return Variant::NextX;
        }
        if self.landless {
            return Variant::Landless;
        }
        if self.landless_plan {
            return Variant::LandlessPlan;
        }
        if self.vote_guard {
            return Variant::VoteGuard;
        }
        if self.next {
            return match self.wonder_race {
                // The game's AI since the thirteenth promotion, or its alias `next-w`.
                1 => self.named.unwrap_or(Variant::Standard),
                2 => Variant::NextRace,
                _ => Variant::Next,
            };
        }
        if self.tall_real {
            return Variant::TallReal;
        }
        match self.wonder_race {
            1 => return Variant::WonderCancel,
            2 => return Variant::WonderRace,
            _ => {}
        }
        if self.sharp {
            return Variant::Sharp;
        }
        if self.field {
            return Variant::FieldCraft;
        }
        if self.raidw {
            return Variant::RaidWar;
        }
        if self.fit {
            return Variant::TacticianFit;
        }
        if self.retake2 {
            return Variant::Retake2;
        }
        if self.call {
            return Variant::SiegeCall;
        }
        if self.muster {
            return Variant::WarMuster;
        }
        if self.pays {
            return Variant::WarPays;
        }
        self.named.unwrap_or(self.variant)
    }

    /// The mind retakes a lost city (`TurnBrief::retake`, `ai::retake`): `retake2` and
    /// `next-war-retake`.
    pub fn retakes(&self) -> bool {
        self.retake2
    }

    /// The mind makes the call at the walls (`TurnBrief::siege_call`): `siege-call`'s line, `next-war-retake` and `next-x`.
    pub fn calls_at_walls(&self) -> bool {
        self.call
    }

    /// The mind plans the war of a player with no city (`TurnBrief::landless_plan`, with the mends of
    /// `TurnBrief::siege_mend` it brings): `landless-plan` and `next-x`.
    pub fn plans_landless(&self) -> bool {
        self.landless_plan
    }

    /// Its tactical and operations layer is the unit net's: `unit-net`, `hybrid`, and
    /// `tactician-fit` (`TurnBrief::tactician_fit`: the unit net with its fitted numbers,
    /// `unit_net::fit`).
    #[cfg(feature = "experimental-ai")]
    fn unit_net_on(&self) -> bool {
        self.variant.unit_net() || self.fit
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
        if self.unit_net_on() {
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
        if start.brief.landless_planned() && state.operations().is_empty() {
            // A player with units and no city: the city its units take is the strategist's target (`ai::landless`;
            // `operations::start` looks for one only while no operation runs).
            // `TurnBrief::siege_mend`: not a city its siege saw it could not take.
            let beyond = match &self.walls {
                Some(walls) if start.brief.siege_mend => walls.beyond_reach(&start.brief),
                _ => Vec::new(),
            };
            start.brief.landless_goal = crate::ai::landless::goal(sight, &start.brief, state, rules, &beyond);
        }
        if start.brief.siege_mend && start.brief.landless_planned() {
            // The target of its operation seen now is weighed again on what the player sees: before its war by the goal's
            // estimate (`ai::landless::takes`), at war by the siege's (`SiegeWalls::beyond_reach`).
            let beyond = self.walls.as_ref().map(|w| w.beyond_reach(&start.brief)).unwrap_or_default();
            let brief = &start.brief;
            let seen = |t: crate::ids::TileId| sight.visible_foreign_cities.iter().any(|c| c.tile == t);
            start.brief.landless_untaken = state
                .operations()
                .iter()
                .find(|o| if brief.at_war(o.enemy) { beyond.contains(&o.target) } else { seen(o.target) && !crate::ai::landless::takes(sight, brief, state, rules, o.target) })
                .map(|o| o.target);
        }
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
        if self.unit_net_on() {
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
            operations_strategic(brief, state, rules)
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
        if self.pays {
            brief.war.siege = crate::ai::operations::SiegeFixes::PAYS;
        }
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
        brief.war_open = self.variant.war_open();
        brief.sharp = self.sharp;
        brief.field_craft = self.field;
        brief.raid_war = self.raidw;
        brief.landless = self.landless;
        brief.landless_plan = self.landless_plan;
        if brief.landless_planned() {
            // The brief's relations are the nations' (city-states have their own system, `ai::system`): a player
            // with no city takes a city-state's city as well, and its strategist must see that war (`operations`).
            brief.relations = observation.relations.iter().filter(|r| r.kind != PlayerKind::Module(ClassicKind::Barbarian)).copied().collect();
        }
        brief.siege_mend = self.siege_mend || brief.landless_planned();
        brief.tall_real = self.tall_real;
        brief.next = self.next;
        brief.vote_guard = self.vote_guard;
        brief.wonder_race = self.wonder_race;
        // `tall-real` finishes its spaceship by the space plan where the science victory fits the
        // game (`victory::space_plan`, `ai::tall_real`), on every seat; `next` too: the plan on its
        // tall seats only measured worse (pace against `sharp`, offsets 200+201: 65.7 % against
        // 70.5 % on every seat).
        let space_plan = self.tall_real && crate::ai::tall_real::science_game(observation, rules);
        brief.space_finish |= space_plan;
        brief.war_pays = self.pays;
        brief.tactician_fit = self.fit;
        brief.muster_fix = self.fit || self.muster;
        brief.war_muster = self.muster;
        brief.retake = self.retake2;
        brief.siege_call = if self.call { crate::ai::planner::CallFixes::DEFAULT } else { crate::ai::planner::CallFixes::NONE };
        brief.met_eras = observation.view.met_eras();
        if brief.war_open {
            brief.scores = observation.view.race().scores;
        }
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
            if self.variant.space_finish() || space_plan {
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
            // `next`: an alliance's delegates toward the World Leader vote count in its gifts.
            if self.next {
                prices.delegate = crate::ai::city_states::delegate_worth(observation);
            }
            // `vote-guard`: what losing the game to a rival's vote costs, once a rival can win it.
            if self.vote_guard {
                prices.guard = crate::ai::victory_watch::guard_prize(observation);
                if observation.round % 10 == 0 {
                    crate::ai::strategy_net::trace::readings(observation, rules);
                }
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
        // The lost cities worth retaking, by the turn's prices and stakes (the value of a city is the strategist's,
        // `retake::pays`). Before the defence answers its dangers, as the retake always was: the defence sends the
        // free units to guards (measured: the retaking falls from 40 to 22 percent of the positions when it comes first).
        if self.retake2 {
            brief.retaking = crate::ai::retake::cases_remembered(observation, &brief, state, rules, &stakes);
        }
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
                let unit_part = if self.unit_net_on() {
                    let (strategy, unit) = crate::ai::unit_net::memory::split(&state.mind_memory);
                    state.mind_memory = strategy;
                    Some(unit)
                } else {
                    None
                };
                let plan = crate::ai::strategy_net::think(observation, &brief, state, rules, &prices, &stakes);
                #[cfg(feature = "experimental-ai")]
                if let Some(unit) = unit_part {
                    state.mind_memory = crate::ai::unit_net::memory::join(&state.mind_memory, &unit);
                }
                if let Some(p) = brief.prices.as_mut() {
                    p.city_cap = Some(plan.city_target);
                }
                brief.tall_cities = Some(plan.city_target);
                // `tall-real`'s tall play orders the science buildings first (`tall::BuildingOrder::Science`).
                if plan.intent.tall_real {
                    brief.building_order = crate::ai::tall::BuildingOrder::Science;
                }
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
        // `tall-real`'s tall play founds no city beyond its target (`tall_real::adjust`).
        let decision = if self.tall_real { crate::ai::tall_real::adjust(decision, brief) } else { decision };
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
/// value per beaker when the allocator prices the turn). Peace alone only
/// with a city-state: with a nation it is a deal (`ai::peace`).
fn operations_strategic(brief: &TurnBrief, state: &mut AiState, rules: &CompiledRuleset) -> Vec<Decision> {
    use crate::ai::{planner_research, planner_strategy};
    let mut out = Vec::new();
    {
        let _span = prof::span("classic/strategy_review");
        out.extend(planner_strategy::review_strategy(brief, state));
    }
    {
        let _span = prof::span("classic/diplomacy_relations");
        out.extend(planner_strategy::plan_relations(brief, state, brief.dims));
    }
    {
        let _span = prof::span("classic/idle_wars_peace");
        let peace = operations::idle_wars(brief, state, &out);
        out.extend(peace);
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

#[cfg(test)]
mod sharp_tests {
    use super::*;

    /// `sharp` is `Standard` in every switch but its strategy net: the mind plays `Standard` with
    /// the flag (`TurnBrief::sharp`) and names itself `sharp`; no other variant has the flag of its
    /// own (the game's AI carries it since the thirteenth promotion,
    /// `the_game_plays_next_w_and_pre_next_is_the_standard_before`).
    #[test]
    fn sharp_plays_standard_with_its_flag() {
        let mind = ClassicMind::new(Variant::Sharp);
        assert_eq!((mind.variant, mind.sharp, mind.variant()), (Variant::Standard, true, Variant::Sharp));
        for v in [Variant::PreNext, Variant::WarOpen, Variant::StrategyNet] {
            let other = ClassicMind::new(v);
            assert_eq!((other.variant(), other.sharp), (v, false));
        }
        let flagged: Vec<Variant> = Variant::ALL.into_iter().filter(|v| v.sharp()).collect();
        assert_eq!(flagged, [Variant::Sharp]);
        assert_eq!(Variant::from_name("sharp"), cfg!(feature = "experimental-ai").then_some(Variant::Sharp));
    }

    /// `next-x` (the joint build) is `next-war-retake` (the flags of `next-w`, the war of `retake2`) with the
    /// guard of `vote-guard` and the plan of `landless-plan`, and none of the measured candidates beside them
    /// (`landless`'s own goal, `field-craft`, `raid-war`, the unit net of `tactician-fit`); it names itself
    /// `next-x`, and each accepted variant keeps its own mind.
    #[test]
    fn next_x_carries_every_accepted_flag() {
        let next = |m: &ClassicMind| (m.sharp, m.tall_real, m.next, m.wonder_race);
        let war = |m: &ClassicMind| (m.variant, m.pays, m.fit, m.muster, m.call, m.retake2);
        let rest = |m: &ClassicMind| (m.vote_guard, m.landless, m.landless_plan, m.field, m.raidw, m.siege_mend);
        let x = ClassicMind::new(Variant::NextX);
        let (retake, guard, plan) = (ClassicMind::new(Variant::NextWarRetake), ClassicMind::new(Variant::VoteGuard), ClassicMind::new(Variant::LandlessPlan));
        assert_eq!((next(&x), war(&x)), (next(&retake), war(&retake)));
        assert_eq!(next(&x), next(&guard));
        assert_eq!(next(&x), next(&plan));
        assert_eq!(rest(&x), (true, false, true, false, false, false));
        assert!(x.retakes() && guard.vote_guard && plan.landless_plan && !retake.vote_guard && !retake.landless_plan);
        assert_eq!((x.variant(), retake.variant(), guard.variant(), plan.variant()), (Variant::NextX, Variant::NextWarRetake, Variant::VoteGuard, Variant::LandlessPlan));
        assert_eq!(Variant::from_name("next-x"), cfg!(feature = "experimental-ai").then_some(Variant::NextX));
    }

    /// `landless` is `Standard` in every switch (the flags of `next-w` included) but for its own flag
    /// (`TurnBrief::landless`); no other variant has the flag, and `Standard` does not carry it.
    #[test]
    fn landless_plays_standard_with_its_flag() {
        let flags = |m: &ClassicMind| (m.sharp, m.tall_real, m.next, m.wonder_race, m.pays, m.fit, m.field, m.raidw);
        let (landless, standard) = (ClassicMind::new(Variant::Landless), ClassicMind::new(Variant::Standard));
        assert_eq!((landless.variant, flags(&landless), landless.landless, landless.variant()), (Variant::Standard, flags(&standard), true, Variant::Landless));
        assert!(!standard.landless && standard.variant() == Variant::Standard);
        let flagged: Vec<Variant> = Variant::ALL.into_iter().filter(|v| v.landless()).collect();
        assert_eq!(flagged, [Variant::Landless]);
        assert_eq!(Variant::from_name("landless"), cfg!(feature = "experimental-ai").then_some(Variant::Landless));
        let plan = ClassicMind::new(Variant::LandlessPlan);
        assert_eq!((plan.variant, flags(&plan), plan.landless, plan.landless_plan, plan.variant()), (Variant::Standard, flags(&standard), false, true, Variant::LandlessPlan));
        assert!(!standard.landless_plan);
        let planned: Vec<Variant> = Variant::ALL.into_iter().filter(|v| v.landless_plan()).collect();
        assert_eq!(planned, [Variant::LandlessPlan]);
        assert_eq!(Variant::from_name("landless-plan"), cfg!(feature = "experimental-ai").then_some(Variant::LandlessPlan));
    }

    /// `tall-real` is `sharp` with its tall play: the mind plays `Standard` with both flags and
    /// names itself `tall-real`; no other variant has the flag.
    #[test]
    fn tall_real_plays_sharp_with_its_flag() {
        let mind = ClassicMind::new(Variant::TallReal);
        assert_eq!((mind.variant, mind.sharp, mind.tall_real, mind.variant()), (Variant::Standard, true, true, Variant::TallReal));
        assert!(!ClassicMind::new(Variant::Sharp).tall_real);
        let flagged: Vec<Variant> = Variant::ALL.into_iter().filter(|v| v.tall_real()).collect();
        assert_eq!(flagged, [Variant::TallReal]);
        assert_eq!(Variant::from_name("tall-real"), cfg!(feature = "experimental-ai").then_some(Variant::TallReal));
    }

    /// `next` is `tall-real` with the review's fixes: the mind plays `Standard` with the three
    /// flags and names itself `next`; `tall-real` and `sharp` do not carry its flag.
    #[test]
    fn next_plays_tall_real_with_its_flag() {
        let mind = ClassicMind::new(Variant::Next);
        assert_eq!((mind.variant, mind.sharp, mind.tall_real, mind.next, mind.variant()), (Variant::Standard, true, true, true, Variant::Next));
        assert!(!ClassicMind::new(Variant::TallReal).next && !ClassicMind::new(Variant::Sharp).next);
        let flagged: Vec<Variant> = Variant::ALL.into_iter().filter(|v| v.next()).collect();
        assert_eq!(flagged, [Variant::Standard, Variant::Next, Variant::NextW, Variant::NextRace]);
        assert_eq!(Variant::from_name("next"), cfg!(feature = "experimental-ai").then_some(Variant::Next));
    }

    /// `next-race` is `next` with the wonder-race rule and price, `next-w` with the rule only:
    /// they carry `next`'s flags and name themselves; `next` carries no wonder-race switch.
    #[test]
    fn next_w_is_next_with_the_wonder_race() {
        for (variant, race) in [(Variant::Next, 0), (Variant::NextW, 1), (Variant::NextRace, 2)] {
            let mind = ClassicMind::new(variant);
            assert_eq!((mind.variant, mind.sharp, mind.tall_real, mind.next, mind.wonder_race, mind.variant()), (Variant::Standard, true, true, true, race, variant));
        }
        assert!(!ClassicMind::new(Variant::WonderRace).next);
        assert_eq!(ClassicMind::new(Variant::WonderRace).variant(), Variant::WonderRace);
        assert_eq!(Variant::from_name("next-race"), cfg!(feature = "experimental-ai").then_some(Variant::NextRace));
        assert_eq!(Variant::from_name("next-w"), cfg!(feature = "experimental-ai").then_some(Variant::NextW));
    }

    /// The thirteenth promotion (11 November): the game's AI is the mind of `next-w` — `Standard`'s
    /// switches with the flags of `sharp`, `tall-real`, `next` and the wonder rule — and names
    /// itself `standard`; `next-w` stays as its alias with the same mind; `pre-next` is the standard
    /// before it: `Standard`'s switches without a flag (bench only).
    #[test]
    fn the_game_plays_next_w_and_pre_next_is_the_standard_before() {
        let flags = |m: &ClassicMind| (m.sharp, m.tall_real, m.next, m.wonder_race);
        let (standard, alias, before) = (ClassicMind::new(Variant::Standard), ClassicMind::new(Variant::NextW), ClassicMind::new(Variant::PreNext));
        assert_eq!((standard.variant, flags(&standard), standard.variant()), (Variant::Standard, (true, true, true, 1), Variant::Standard));
        assert_eq!((alias.variant, flags(&alias), alias.variant()), (Variant::Standard, (true, true, true, 1), Variant::NextW));
        assert_eq!((before.variant, flags(&before), before.variant()), (Variant::PreNext, (false, false, false, 0), Variant::PreNext));
        assert_eq!(flags(&ClassicMind::default()), flags(&standard), "the game's AI");
        // Every other variant keeps its flags: none but the line of `sharp` (the flags `new` reads).
        let line = [Variant::Standard, Variant::NextW, Variant::Sharp, Variant::TallReal, Variant::Next, Variant::WonderCancel, Variant::WonderRace, Variant::NextRace];
        for v in Variant::ALL.into_iter().filter(|v| !line.contains(v)) {
            assert_eq!((v.sharp(), v.tall_real(), v.next(), v.wonder_race()), (false, false, false, 0), "{}", v.name());
        }
        // A book on `standard` plays on `pre-next`, as before the promotion.
        let tall = ClassicMind::book_on(Variant::TallBook, Variant::Standard).unwrap();
        assert_eq!((tall.variant, flags(&tall)), (Variant::PreNext, (false, false, false, 0)));
        let duel = ClassicMind::book_on(Variant::RushSiege, Variant::Standard).unwrap();
        assert_eq!((duel.variant, flags(&duel)), (Variant::PreNext, (false, false, false, 0)));
        assert_eq!(Variant::from_name("pre-next"), cfg!(feature = "experimental-ai").then_some(Variant::PreNext));
    }

    /// `next-war` is the game's AI (the flags of `next-w`) with the war of `siege-call`: its flags
    /// and the switches of `WarOpen`, as `siege-call` plays them; it names itself `next-war`, and
    /// `siege-call` keeps its own mind (no flag of `next-w`: it plays on `pre-next`).
    #[test]
    fn next_war_is_standard_with_the_war_of_siege_call() {
        let next = |m: &ClassicMind| (m.sharp, m.tall_real, m.next, m.wonder_race);
        let war = |m: &ClassicMind| (m.variant, m.pays, m.fit, m.muster, m.call);
        let (mind, standard, call) = (ClassicMind::new(Variant::NextWar), ClassicMind::new(Variant::Standard), ClassicMind::new(Variant::SiegeCall));
        assert_eq!(next(&mind), next(&standard));
        assert_eq!(war(&mind), war(&call));
        assert_eq!(war(&call), (Variant::WarOpen, true, false, true, true));
        assert_eq!(next(&call), (false, false, false, 0));
        assert_eq!((mind.variant(), call.variant(), standard.variant()), (Variant::NextWar, Variant::SiegeCall, Variant::Standard));
        assert_eq!(Variant::from_name("next-war"), cfg!(feature = "experimental-ai").then_some(Variant::NextWar));
    }

    /// `next-war` and `next-war-retake` are the game's AI in everything but the war (the review of
    /// stage 1): the mind's flags of `next-w`, the parts the mind builds (siege, peace, book, nets,
    /// Unciv), and every switch the mind reads from its variant (`WarOpen` for them, `Standard` for
    /// the game's AI) but the war's: `war_open` and the war case of `war_tuning`. The war flags
    /// (`pays`, `fit`, `muster`, `call`, `retake2`) are the war line's. A switch that one of them
    /// reads differently from `Standard` fails here (a new switch joins the list); the others
    /// (`settings`, `adjust`, `war_gate`) read the variant's `base()`, `Standard`'s for `WarOpen`
    /// through `scripted()`.
    #[test]
    fn next_war_variants_are_standard_but_for_the_war() {
        macro_rules! read {
            ($v:expr; $($f:ident),* $(,)?) => { vec![$((stringify!($f), format!("{:?}", $v.$f()))),*] };
        }
        let switches = |m: &ClassicMind| {
            let v = m.variant;
            let tuning = crate::ai::operations::WarTuning { siege: crate::ai::operations::SiegeFixes::NONE, ..v.war_tuning() };
            let mut out = read!(v; allocates, operates, fights_as_front, has_leaders, defends, growth_policy, content_founding, true_effects, effect_checks, priced_stops, science_levers, tactical, danger_net, weak_fighter, fight_fix, siege_tactics, choice_mode, site_net, space_finish, unit_abilities, religion_fix, tall, building_order, price_model, sound_orders, checks_orders, strikes, carriers, landings, science_pricing, sea_work, air_arm, plans, values_embassies, siege_walls, siege_join, workers_fit, conquest, army_fit, camps, trade_fit, no_idle_cities, scout_early, cs_gift_cap, strategy_net, unit_net, fog_march, unciv, net, book);
            out.push(("war_tuning without the war case", format!("{tuning:?}")));
            out.push(("flags of next-w", format!("{:?}", (m.sharp, m.tall_real, m.next, m.wonder_race))));
            out.push(("parts", format!("{:?}", (m.walls.is_some(), m.unciv.is_some(), m.book.is_some(), m.tall, m.conquest, m.choice, m.genome.is_some()))));
            #[cfg(feature = "experimental-ai")]
            out.push(("nets", format!("{:?}", (m.net.is_some(), m.unit_net_on()))));
            out
        };
        let standard = ClassicMind::new(Variant::Standard);
        for v in [Variant::NextWar, Variant::NextWarRetake, Variant::NextX] {
            let mind = ClassicMind::new(v);
            for ((name, a), (_, b)) in switches(&mind).into_iter().zip(switches(&standard)) {
                assert_eq!(a, b, "{}: {name}", v.name());
            }
            assert!(mind.variant.war_open() && !standard.variant.war_open() && mind.pays && mind.call && !standard.pays, "{}: the war is the war line's", v.name());
        }
    }

    /// `war-pays` is `WarOpen` in every switch but its own flag (`TurnBrief::war_pays`); no other
    /// variant has the flag, and `sharp` and `war-open` keep theirs.
    #[test]
    fn war_pays_plays_war_open_with_its_flag() {
        let mind = ClassicMind::new(Variant::WarPays);
        assert_eq!((mind.variant, mind.pays, mind.sharp, mind.variant()), (Variant::WarOpen, true, false, Variant::WarPays));
        let open = ClassicMind::new(Variant::WarOpen);
        assert_eq!((open.variant(), open.pays), (Variant::WarOpen, false));
        let flagged: Vec<Variant> = Variant::ALL.into_iter().filter(|v| v.war_pays()).collect();
        assert_eq!(flagged, [Variant::WarPays]);
        // An experimental war variant like the rest of its line (`Variant::compiled`; until stage 2 of
        // `next-war` it was named in every build).
        assert_eq!(Variant::from_name("war-pays"), cfg!(feature = "experimental-ai").then_some(Variant::WarPays));
        assert!(mind.variant.war_open() && !Variant::WarPays.war_open());
    }

    /// `tactician-fit` is `WarPays` in every switch but its own flag (`TurnBrief::tactician_fit`); no
    /// other variant has the flag, and `war-pays` keeps its own.
    #[test]
    fn tactician_fit_plays_war_pays_with_its_flag() {
        let mind = ClassicMind::new(Variant::TacticianFit);
        assert_eq!((mind.variant, mind.pays, mind.fit, mind.sharp, mind.variant()), (Variant::WarOpen, true, true, false, Variant::TacticianFit));
        let pays = ClassicMind::new(Variant::WarPays);
        assert_eq!((pays.variant(), pays.fit), (Variant::WarPays, false));
        let flagged: Vec<Variant> = Variant::ALL.into_iter().filter(|v| v.tactician_fit()).collect();
        assert_eq!(flagged, [Variant::TacticianFit]);
        assert_eq!(Variant::from_name("tactician-fit"), cfg!(feature = "experimental-ai").then_some(Variant::TacticianFit));
        #[cfg(feature = "experimental-ai")]
        assert!(mind.unit_net_on() && !pays.unit_net_on() && !ClassicMind::new(Variant::Standard).unit_net_on());
    }

    /// `war-muster` is `WarPays` with the muster fix and the scripted tactics: no unit net, no
    /// `tactician_fit` flag; no other variant has the muster flag but it and `tactician-fit`.
    #[test]
    fn war_muster_plays_war_pays_with_the_muster_fix_only() {
        let mind = ClassicMind::new(Variant::WarMuster);
        assert_eq!((mind.variant, mind.pays, mind.fit, mind.muster, mind.variant()), (Variant::WarOpen, true, false, true, Variant::WarMuster));
        let flagged: Vec<Variant> = Variant::ALL.into_iter().filter(|v| v.war_muster()).collect();
        assert_eq!(flagged, [Variant::WarMuster]);
        assert!(!ClassicMind::new(Variant::WarPays).muster);
        #[cfg(feature = "experimental-ai")]
        assert!(!mind.unit_net_on());
    }

    /// `retake2` is `SiegeCall` with the retake of a lost city: the flag of no other variant, and a mind
    /// that is not `retake2` has no case (`TurnBrief::retaking` stays empty).
    #[test]
    fn retake2_plays_siege_call_with_the_retake_only() {
        let mind = ClassicMind::new(Variant::Retake2);
        assert_eq!((mind.variant, mind.pays, mind.muster, mind.call, mind.retake2, mind.variant()), (Variant::WarOpen, true, true, true, true, Variant::Retake2));
        let flagged: Vec<Variant> = Variant::ALL.into_iter().filter(|v| v.retake2()).collect();
        assert_eq!(flagged, [Variant::Retake2]);
        let call = ClassicMind::new(Variant::SiegeCall);
        assert!(call.call && !call.retake2);
        assert!(!ClassicMind::new(Variant::WarPays).retake2);
    }

    /// `next-war-retake` is `next-war` (the flags of `next-w` and the war of `siege-call`) with the
    /// retake of `retake2`; it names itself `next-war-retake`, and `retake2` keeps its own mind (no
    /// flag of `next-w`: it plays on `pre-next`).
    #[test]
    fn next_war_retake_is_next_war_with_the_retake_of_retake2() {
        let next = |m: &ClassicMind| (m.sharp, m.tall_real, m.next, m.wonder_race);
        let war = |m: &ClassicMind| (m.variant, m.pays, m.fit, m.muster, m.call, m.retake2);
        let (mind, next_war, retake) = (ClassicMind::new(Variant::NextWarRetake), ClassicMind::new(Variant::NextWar), ClassicMind::new(Variant::Retake2));
        assert_eq!(next(&mind), next(&next_war));
        assert_eq!(war(&mind), war(&retake));
        assert_eq!(war(&next_war), (Variant::WarOpen, true, false, true, true, false));
        assert_eq!(next(&retake), (false, false, false, 0));
        assert!(mind.retakes() && retake.retakes() && !next_war.retakes());
        assert_eq!((mind.variant(), retake.variant()), (Variant::NextWarRetake, Variant::Retake2));
        assert_eq!(Variant::from_name("next-war-retake"), cfg!(feature = "experimental-ai").then_some(Variant::NextWarRetake));
    }
}
