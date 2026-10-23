//! Named versions of the classic-ruleset AI (platform/docs/ai-bench.md). The game plays
//! `Standard`; the AI bench seats the others against it
//! (`kernel::ai::seats::Seated`). A change of the AI is developed as a new
//! variant next to `Standard`; once it wins on the bench, it becomes the
//! standard and the old one stays as a named variant while the bench needs
//! it.
//!
//! The steps of ru/PLAN.en.md "M6.2. A whole AI", each on the previous:
//! `Legacy` is the M6 planners (the game's AI until 16 October, then named
//! `baseline`); `Tactics` decides the military units near an enemy as one
//! front (`ai::front`, classic/docs/ai-tactics.md); `Operations` fights its wars by
//! operations: an army formed for one city, built by the cities and
//! launched when assembled (`ai::operations`, classic/docs/ai-operations.md);
//! `Allocator` spends production, gold and research by one allocator
//! (`ai::allocator`, classic/docs/ai-allocator.md; named `economy`, then the
//! game's AI `standard`); `Leaders` is `Allocator` played by its leaders:
//! their personalities and a grand strategy that pursues a path to victory
//! (`ai::grand_strategy`, classic/docs/ai-leaders.md, step 5), and peace by value
//! (`ai::peace`, step 6); `Defence` is `Leaders` that values keeping its
//! cities (`ai::defence`, classic/docs/ai-defence.md, step 6 «defence»). `Direct`
//! is `Defence` whose allocator prices gold by its best use and happiness
//! by its effects (`ai::value::PriceModel::Uses`, classic/docs/ai-allocator.md
//! "Gold and happiness"; named `defence-econ2`, then the game's AI
//! `standard`, until the science economy): it prices a beaker at the score
//! of the technologies and orders what the rules refuse. `Standard` is
//! `Direct` with the science economy (classic/docs/ai-allocator.md "Science"): it
//! orders only what the rules accept (`Sound`) and prices a beaker by what
//! the technologies it buys open (`SciencePricing::Reach`; named `reach`
//! until it became the game's AI); `AllocatorEcon2` and `LeadersEcon2` are `Allocator` and `Leaders`
//! with those prices. `AllocatorPeace` is `Allocator` without new wars,
//! the control of its wars; `LeadersAlone` is `Leaders` without peace by
//! value, its control. The instruments of the bench itself are built on
//! `Standard`: `Nudged` plays as well but differently (the noise floor),
//! `OneCity` is known to be weaker (the bench sees a difference).

use crate::ai::grand_strategy::Stakes;
use crate::ai::operations::WarGate;
use crate::ai::peace::PeaceMaking;
use crate::ai::planner::{AiSettings, Decision, TurnBrief};
use crate::ai::trace::Reason;
use crate::ai::value::{PriceModel, Prices, SciencePricing};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Variant {
    /// The game's AI, with the heavy arms since 23 October (missiles and
    /// nuclear weapons, carriers, paratrooper jumps, the war case that counts
    /// them: classic/docs/ai-strikes.md). The civilizations' leaders (personalities, a grand
    /// strategy, peace by deal) defending their cities by value, with gold
    /// worth its best use, happiness worth its effects (M6.2 steps 4–6) and
    /// a beaker worth what the technologies it buys open (the science
    /// economy, classic/docs/ai-allocator.md "Science").
    #[default]
    Standard,
    /// `Standard` before the science economy: a beaker at the score of the
    /// technologies, orders the rules refuse kept; the game's AI until 19
    /// October.
    Direct,
    /// `Operations` whose production, gold, research and decision on war
    /// one allocator values in points of the final score (M6.2 step 4,
    /// classic/docs/ai-allocator.md); the game's AI before `Standard`.
    Allocator,
    /// `Allocator` that starts no war operation and leaves every war after
    /// its minimum length: the control that tells the allocator's economy
    /// from its wars.
    AllocatorPeace,
    /// `Allocator` whose gold is worth its best use and whose happiness is
    /// worth its effects (`PriceModel::Uses`).
    AllocatorEcon2,
    /// `Allocator` played by the civilizations' leaders: each one's
    /// personality chooses among near-equal options, its grand strategy
    /// pursues a path to victory (M6.2 step 5), and its wars with
    /// civilizations end by peace deals it values (step 6). With every
    /// personality neutral it plays as `Allocator` but for captures that
    /// win the game and how its wars end.
    Leaders,
    /// `Leaders` who end their wars alone, as `Allocator` does (`Leaders`
    /// before step 6): the control of peace by value.
    LeadersAlone,
    /// `Leaders` with the prices of `AllocatorEcon2`.
    LeadersEcon2,
    /// `Leaders` that defends by value: the cities in danger, guards sent,
    /// operations recalled and defenders built while they save more than
    /// they cost (M6.2 step 6); `Standard` without its prices.
    Defence,
    /// `Direct` that orders only what the rules accept: no building without
    /// its resource, no unit without its policy (the half of the science
    /// economy that is a repair; the control of the other half).
    Sound,
    /// The M6 planners, the game's AI before `Allocator`.
    Legacy,
    /// `Legacy` with coordinated combat: focus fire, sieges gathered out of
    /// the city's strike, the wounded rotated out (M6.2 step 2).
    Tactics,
    /// `Tactics` with war operations: armies sized for their target,
    /// mustered, built by the cities, marching together; a war only when
    /// an operation can take its city in time (M6.2 step 3).
    Operations,
    /// `Standard` that plans its investments toward an aim (classic/docs/ai-investment.md):
    /// every leader weighs the spaceship at least as the score, the chance of
    /// it grows with the economy it already has (no chicken and egg), a game
    /// without a time victory is judged over an open horizon, and a beaker
    /// is also worth the progress it makes toward the aim.
    Planner,
    /// `Standard` before the open horizon: the rounds left are counted to
    /// the nominal end of the game even when no time victory ends it there
    /// (`value::horizon`); the control of the open horizon, and the AI of the
    /// game until 20 October.
    Closed,
    /// `Standard` without work boats: the land workers improve only the
    /// land (the game's AI before 21 October, the control of
    /// `ai::sea_work`).
    Landwork,
    /// `Previous` with missiles and nuclear weapons, built and fired, and
    /// counted by the war case (`ai::strikes`, classic/docs/ai-strikes.md):
    /// one third of the heavy arms of `Standard`, for attribution.
    Strikes,
    /// `Previous` with paratroopers that jump to their army's staging tile
    /// (`ai::operations`, classic/docs/ai-strikes.md "Landings"): another third.
    Landings,
    /// The game's AI before the heavy arms (23 October): `Standard` without
    /// missiles, nuclear weapons, carriers and paratrooper jumps; the control
    /// of `ai::strikes`, `ai::carrier` and the war case with weapons.
    Previous,
    /// `Standard` that sends its orders unchecked, as before 23 October: the
    /// control of `ai::legal` (a refused production order is chosen again,
    /// a unit order or an advisor's proposal the rules refuse still goes to
    /// the game).
    Unchecked,
    /// `Standard` without its air arm: aircraft stay in their cities
    /// (the game's AI before 21 October, the control of `ai::air`).
    NoAir,
    /// `Standard` scoring fewer candidates (sites, jobs, targets) per
    /// decision: most games diverge from the standard's early, the
    /// strength stays (the bench's noise floor, platform/docs/ai-bench.md).
    Nudged,
    /// `Standard` that founds no city after its capital: known to be
    /// weaker (the bench's calibration).
    OneCity,
}

/// Candidates per decision of `Nudged` (the game's budget: 48). A smaller
/// nudge diverges too few games (measured on `Legacy`: one node less per
/// path search changes none of the standard suite's 80, one candidate less
/// 32, this 73).
const NUDGED_CANDIDATES: u32 = 40;

impl Variant {
    /// The game's AI first.
    pub const ALL: [Variant; 23] = [
        Variant::Standard,
        Variant::Previous,
        Variant::Strikes,
        Variant::Landings,
        Variant::Unchecked,
        Variant::Landwork,
        Variant::NoAir,
        Variant::Planner,
        Variant::Closed,
        Variant::Direct,
        Variant::Sound,
        Variant::Allocator,
        Variant::AllocatorPeace,
        Variant::AllocatorEcon2,
        Variant::Leaders,
        Variant::LeadersAlone,
        Variant::LeadersEcon2,
        Variant::Defence,
        Variant::Legacy,
        Variant::Tactics,
        Variant::Operations,
        Variant::Nudged,
        Variant::OneCity,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Variant::Standard => "standard",
            Variant::Planner => "planner",
            Variant::Landwork => "landwork",
            Variant::Unchecked => "unchecked",
            Variant::Strikes => "strikes",
            Variant::Landings => "landings",
            Variant::Previous => "previous",
            Variant::NoAir => "no-air",
            Variant::Closed => "closed",
            Variant::Allocator => "allocator",
            Variant::AllocatorPeace => "allocator-peace",
            Variant::AllocatorEcon2 => "allocator-econ2",
            Variant::Leaders => "leaders",
            Variant::LeadersAlone => "leaders-alone",
            Variant::LeadersEcon2 => "leaders-econ2",
            Variant::Defence => "defence",
            Variant::Direct => "direct",
            Variant::Sound => "sound",
            Variant::Legacy => "legacy",
            Variant::Tactics => "tactics",
            Variant::Operations => "operations",
            Variant::Nudged => "nudged",
            Variant::OneCity => "one-city",
        }
    }

    pub fn from_name(name: &str) -> Option<Variant> {
        Variant::ALL.into_iter().find(|v| v.name() == name)
    }

    /// `Standard` or a bench instrument built on it.
    fn is_standard(self) -> bool {
        matches!(self, Variant::Standard | Variant::Planner | Variant::Landwork | Variant::NoAir | Variant::Closed | Variant::Direct | Variant::Sound | Variant::Nudged | Variant::OneCity | Variant::Unchecked | Variant::Strikes | Variant::Landings | Variant::Previous)
    }

    /// One allocator spends its cities' production and its gold.
    pub(crate) fn allocates(self) -> bool {
        !matches!(self, Variant::Legacy | Variant::Tactics | Variant::Operations)
    }

    /// Its wars are fought by operations (`ai::operations`).
    pub(crate) fn operates(self) -> bool {
        !matches!(self, Variant::Legacy | Variant::Tactics)
    }

    /// Its military units near an enemy decide as one front (`ai::front`).
    pub(crate) fn fights_as_front(self) -> bool {
        self != Variant::Legacy
    }

    /// Its leaders have personalities and a grand strategy.
    pub(crate) fn has_leaders(self) -> bool {
        self.defends() || matches!(self, Variant::Leaders | Variant::LeadersAlone | Variant::LeadersEcon2)
    }

    /// It values keeping its cities (`ai::defence`).
    pub(crate) fn defends(self) -> bool {
        self.is_standard() || self == Variant::Defence
    }

    /// How its allocator prices gold and happiness.
    pub fn price_model(self) -> PriceModel {
        if self.is_standard() || matches!(self, Variant::AllocatorEcon2 | Variant::LeadersEcon2) {
            PriceModel::Uses
        } else {
            PriceModel::Tiles
        }
    }

    /// It orders only what the rules accept (`TurnBrief::sound_orders`).
    pub fn sound_orders(self) -> bool {
        matches!(self, Variant::Standard | Variant::Planner | Variant::Landwork | Variant::NoAir | Variant::Closed | Variant::Sound | Variant::Nudged | Variant::OneCity | Variant::Unchecked | Variant::Strikes | Variant::Landings | Variant::Previous)
    }

    /// Every order it sends passed the rules' own check (`ai::legal`): all
    /// the variants built on `Standard` but `Unchecked`, its control, and the
    /// ones before the science economy.
    pub fn checks_orders(self) -> bool {
        self.sound_orders() && self != Variant::Unchecked
    }

    /// It builds and fires missiles and nuclear weapons (`ai::strikes`).
    pub fn strikes(self) -> bool {
        matches!(self, Variant::Standard | Variant::Strikes)
    }

    /// It builds and sails carriers for the air arm and the weapons
    /// (`ai::carrier`).
    pub fn carriers(self) -> bool {
        self == Variant::Standard
    }

    /// Its paratroopers jump to their army's staging tile
    /// (`ai::operations`).
    pub fn landings(self) -> bool {
        matches!(self, Variant::Standard | Variant::Landings)
    }

    /// How its allocator prices a beaker.
    pub fn science_pricing(self) -> SciencePricing {
        if self == Variant::Planner {
            SciencePricing::Aim
        } else if self == Variant::Closed {
            SciencePricing::ReachClosed
        } else if matches!(self, Variant::Standard | Variant::Landwork | Variant::NoAir | Variant::Nudged | Variant::OneCity | Variant::Unchecked | Variant::Strikes | Variant::Landings | Variant::Previous) {
            SciencePricing::Reach
        } else {
            SciencePricing::Direct
        }
    }

    /// Its cities build work boats for the sea deposits in the borders
    /// (`ai::sea_work`): the game's AI and the variants built on it, but
    /// not their control `Landwork`.
    pub fn sea_work(self) -> bool {
        matches!(self, Variant::Standard | Variant::Planner | Variant::Closed | Variant::NoAir | Variant::Unchecked | Variant::Strikes | Variant::Landings | Variant::Previous)
    }

    /// It builds and commands aircraft (`ai::air`): the game's AI and the
    /// variants built on it, but not their control `NoAir`.
    pub fn air_arm(self) -> bool {
        matches!(self, Variant::Standard | Variant::Planner | Variant::Closed | Variant::Nudged | Variant::OneCity | Variant::Unchecked | Variant::Strikes | Variant::Landings | Variant::Previous)
    }

    /// It plans toward an aim (`ai::grand_strategy`, classic/docs/ai-investment.md).
    pub fn plans(self) -> bool {
        self == Variant::Planner
    }

    /// How it ends its wars with civilizations: the leaders by deals they
    /// value (`ai::peace`), their control and the variants before them
    /// alone.
    pub fn peace_making(self) -> PeaceMaking {
        if self.has_leaders() && self != Variant::LeadersAlone {
            PeaceMaking::ByDeal
        } else {
            PeaceMaking::Alone
        }
    }

    /// How its operations are admitted; `prices` are the allocator's,
    /// `stakes` the leader's.
    pub(crate) fn war_gate<'p>(self, prices: Option<&'p Prices>, stakes: &'p Stakes) -> WarGate<'p> {
        match (self, prices) {
            (Variant::Operations, _) => WarGate::Orders,
            (Variant::AllocatorPeace, _) | (_, None) => WarGate::Never,
            (_, Some(prices)) => WarGate::Value(prices, stakes),
        }
    }

    /// The settings its decisions read, when they differ from the game's.
    pub(crate) fn settings(self, settings: &AiSettings) -> Option<AiSettings> {
        (self == Variant::Nudged).then(|| {
            let mut nudged = settings.clone();
            nudged.budget.candidates = NUDGED_CANDIDATES;
            nudged
        })
    }

    /// Its change of a unit decision.
    pub(crate) fn adjust(self, decision: Decision, brief: &TurnBrief) -> Decision {
        let settles = decision.reason == Reason::FoundCity && !brief.cities.is_empty();
        if self == Variant::OneCity && settles {
            return Decision::refuse(decision.actor, Reason::Idle);
        }
        decision
    }
}
