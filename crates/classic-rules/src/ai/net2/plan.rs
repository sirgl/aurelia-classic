//! The contract of `net2` (work/new-net-design-2026-11-22.md §6): every type the parts read or write.
//!
//! | part | writes | reads |
//! |---|---|---|
//! | A inputs (`inputs`, `rival_inputs`) | `Inputs` (`SeatInputs`, `RivalInputs`, `CityInputs`), its own `InputMemory` | the observation, the brief, `AiState` |
//! | B core (`concepts`, `levers`, `weights`, `site_head`) | `Plan2` (`strategy_net::Plan` with the intent, the 15 prices, the city target; `Net2Plan`), `CoreMemory` | `Inputs` |
//! | C war executors | — | `Net2Plan::war`, `guard_of`, `escorts`, `generals` |
//! | D diplomacy executors | — | `Net2Plan::trust`, `denies_help`, `makes_peace_with`, `cs_gold_share`, `embargo` |
//! | E wiring, sites | `TurnBrief::net2`, `SiteInputs` | `net2::think`, `site_head::score`, `Net2Plan::menace_of`, `Net2Plan::tall_share` |
//!
//! Scale: thousandths (`Milli`, 1000 = 1.0), 0…1000 unless a field says otherwise. Every input is read
//! from the seat's own observation and memory (fog), never from the world.
//!
//! Extension rule (the offensive outputs of the design's addendum will add war kinds, intent variants and
//! levers): a reader asks through `Net2Plan`'s and `WarKind`'s methods (`war_on`, `passes_odds_at_war`,
//! `trust`, …) or matches with a `_` arm, never exhaustively on a variant list, and a new lever is a new
//! field with a `Default` that is today's behaviour. The enums are `#[non_exhaustive]` for the readers
//! outside this crate (the bench).
//!
//! `Net2Plan::default()` is today's behaviour: no war order, no peace wish, every rival trusted, help
//! denied to nobody, no guard, the city-state reserve as today, no embargo, the escorts and the
//! generals as today. An executor that finds `brief.net2` None, or the default, plays as today.

use crate::ai::strategy_net::weights::fe;
use crate::ai::strategy_net::{Milli, Plan};
use crate::ids::{EntityId, PlayerId};

/// The most rivals read per turn (met majors, by id).
pub const MAX_RIVALS: usize = 7;

// ---- layer 0: the inputs (design §2) ----

/// The seat's new inputs (§2.2, G1–G10), 0…1000 each.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SeatInputs {
    /// G1: our original capital's holder is not us (0/1000).
    pub capital_lost: Milli,
    /// G2: own cities lost in the last 20 paced rounds, 0…3 → 0…1000.
    pub cities_lost: Milli,
    /// G3: the longest current war, paced rounds since `war_since`, 0…30 → 0…1000.
    pub war_rounds: Milli,
    /// G4: cities captured − cities lost in the current wars, −2…+2 → 0…1000 (500 = even; 500 at peace).
    pub war_gain: Milli,
    /// G5: happiness 10 paced rounds ago − now, 0…10 → 0…1000.
    pub unhappy_trend: Milli,
    /// G6: the rules' embarkation is known (0/1000).
    pub embark: Milli,
    /// G7: enemy air units seen (`air::enemy_air_seen`), 0…4 → 0…1000.
    pub enemy_air: Milli,
    /// G8: treasury over `MEDIUM_GIFT` (500), 0…3 → 0…1000.
    pub gold_stock: Milli,
    /// G9: city-states allied to us over 4, 0…1000.
    pub cs_allies: Milli,
    /// G10: city-states where our influence trails the leader's by < 30 points, 0…4 → 0…1000.
    pub cs_contested: Milli,
}

/// The inputs of one met major rival (§2.3, R1–R24 and the two facts the war order needs).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RivalInputs {
    pub rival: PlayerId,
    /// R1: our nearest city to its nearest seen city: 1000 at ≤ 5 tiles, 0 at ≥ 15.
    pub near: Milli,
    /// R2: `Routes::stage` from our nearest city reaches its nearest seen city within `BORDER_REACH` (0/1000).
    pub land: Milli,
    /// R3: not `land`, its nearest seen city coastal, one of ours coastal within `DECISIVE_RANGE`, and `embark` (0/1000).
    pub sea: Milli,
    /// R4: its soldiers by the fixed estimator (§2.4) over its + ours: 500 level, 0 no army.
    pub soldiers: Milli,
    /// R5: its military power sighted in the last 10 paced rounds (decayed max) over itself + ours.
    pub seen_army: Milli,
    /// R6: the share of its seen cities last seen with no military unit on or next to them.
    pub open_cities: Milli,
    /// R7: its share of the technology tree (`threat.rs`).
    pub tree: Milli,
    /// R8: its lead over us, a tenth of the tree → 1000 (`threat.rs`).
    pub lead: Milli,
    /// R9: its science over its + ours (500 level, `threat.rs`).
    pub pace: Milli,
    /// R10: its culture share against ours.
    pub culture: Milli,
    /// R11: at peace, its military power inside our borders or within `THREAT_RADIUS` of our cities over our power there.
    pub stack: Milli,
    /// R12: `relation.warmonger` / `DENOUNCE_WARMONGER` (250), clamped.
    pub warmonger: Milli,
    /// R13: wars it declared on us within 30 paced rounds of a deal or while friends, 0…2 → 0…1000.
    pub betrayed: Milli,
    /// R14: it denounced us (0/1000).
    pub denounced_us: Milli,
    /// R15: friends (0/1000).
    pub friend: Milli,
    /// R16: a defensive pact (0/1000).
    pub pact: Milli,
    /// R17: at war with us (0/1000).
    pub at_war: Milli,
    /// R18: paced rounds since `war_since`, 0…30 → 0…1000 (0 at peace).
    pub war_rounds: Milli,
    /// R19: it holds our original capital (0/1000).
    pub holds_capital: Milli,
    /// R20: our lost cities it holds, 0…2 → 0…1000.
    pub holds_cities: Milli,
    /// R21: its cities founded within 4 tiles of ours in 40 paced rounds + 500 a citadel by our land, clamped.
    pub grab: Milli,
    /// R22: it is at war with a friend or pact partner of ours (0/1000; 0 while the public war list is unknown).
    pub ally_hit: Milli,
    /// R23: our free power within 6 tiles of its nearest seen city over that + its sighted power there.
    pub local: Milli,
    /// R24: its score over its + ours.
    pub score: Milli,
    /// A retake case on it runs (`retake::Memory`, 0/1000): no peace and no end of a war order meanwhile.
    pub retake_case: Milli,
    /// We captured one of its cities in the last 10 paced rounds (0/1000): a war that pays runs on.
    pub captured_recent: Milli,
}

impl RivalInputs {
    /// A rival with every input 0 (a stranger far away).
    pub fn stranger(rival: PlayerId) -> Self {
        Self {
            rival,
            near: 0,
            land: 0,
            sea: 0,
            soldiers: 500,
            seen_army: 0,
            open_cities: 0,
            tree: 0,
            lead: 0,
            pace: 500,
            culture: 0,
            stack: 0,
            warmonger: 0,
            betrayed: 0,
            denounced_us: 0,
            friend: 0,
            pact: 0,
            at_war: 0,
            war_rounds: 0,
            holds_capital: 0,
            holds_cities: 0,
            grab: 0,
            ally_hit: 0,
            local: 500,
            score: 500,
            retake_case: 0,
            captured_recent: 0,
        }
    }
}

/// The inputs of the guard of one own city (§4.5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CityInputs {
    /// The city (`OwnCity::id`).
    pub city: EntityId,
    /// The owner of the strongest foreign stack near the city (`defence::hostile_units`); None: no stack.
    pub stack_owner: Option<PlayerId>,
    /// The city is in danger (`defence::Watch::danger`, 0/1000).
    pub danger: Milli,
    /// Rivals with a seen city within 8 tiles of this one.
    pub rivals_near: Vec<PlayerId>,
    /// Founded less than 20 paced rounds ago (0/1000).
    pub young: Milli,
}

/// Everything `net2` reads in a turn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Inputs {
    /// The carried features 0–53 of `strategy_net::features` (54–67 are not read by `net2`).
    pub situation: [Milli; fe::N],
    pub seat: SeatInputs,
    /// At most `MAX_RIVALS`, by id.
    pub rivals: Vec<RivalInputs>,
    pub cities: Vec<CityInputs>,
}

/// The inputs of the site head for one candidate tile (§3.4, S1–S9). Points are the planner's site points.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SiteInputs {
    /// S1: the fitted site net's answer in points (`site_net::planner_score`).
    pub pop50: i64,
    /// S2: the heuristic land score (`SiteContext::score`), points.
    pub land: i64,
    /// S3: `distance_terms(own, foreign)`, points.
    pub dist: i64,
    /// S4: max(0, 7 − tiles to the nearest seen rival city).
    pub rival_gap: i64,
    /// S5: `menace` of that rival (`Net2Plan::menace_of`), 0…1000.
    pub rival_menace: Milli,
    /// S6: hostile military units in sight within 6 tiles.
    pub army: i64,
    /// S7: turns of travel.
    pub travel: i64,
    /// S8: ≥ 3 from every city and within 5 of two of ours (0/1000).
    pub fill: Milli,
    /// S9: the style share of tall (`Net2Plan::tall_share`), 0…1000.
    pub tall: Milli,
}

// ---- the outputs (design §4.4–§4.7) ----

/// Why a war is fought (§4.4); each kind has its budget (§3.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum WarKind {
    /// A weak, reachable neighbour: the cheap city.
    Prey,
    /// Our capital or cities in its hands.
    Retake,
    /// The runaway, where it is reachable and beatable.
    Spoiler,
    /// It struck a friend or a pact partner.
    Duty,
    /// A grabber's forward city or citadel, while it is weak.
    Border,
    /// Any of the above but a retake, over water (staging by sea, escorts).
    Overseas,
}

impl WarKind {
    pub const ALL: [WarKind; 6] = [WarKind::Prey, WarKind::Retake, WarKind::Spoiler, WarKind::Duty, WarKind::Border, WarKind::Overseas];

    /// The order's budget in Standard rounds (§3.3; paced by the caller).
    pub fn budget(self) -> u32 {
        match self {
            WarKind::Retake => 20,
            WarKind::Prey => 25,
            WarKind::Spoiler => 30,
            WarKind::Duty => 15,
            WarKind::Border => 15,
            WarKind::Overseas => 30,
        }
    }

    /// A kind that passes the war machine's odds gate at war like `staked` (`operations.rs:1773`).
    pub fn passes_odds_at_war(self) -> bool {
        matches!(self, WarKind::Retake | WarKind::Spoiler | WarKind::Duty)
    }

    pub fn name(self) -> &'static str {
        match self {
            WarKind::Prey => "prey",
            WarKind::Retake => "retake",
            WarKind::Spoiler => "spoiler",
            WarKind::Duty => "duty",
            WarKind::Border => "border",
            WarKind::Overseas => "overseas",
        }
    }
}

/// A war order: one at a time (§4.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WarOrder {
    pub rival: PlayerId,
    pub kind: WarKind,
    /// The round the order started.
    pub since: u32,
    /// Its budget in paced rounds (`WarKind::budget` × pace).
    pub budget: u32,
    /// Its war value this turn (≥ 350 while kept).
    pub value: Milli,
}

impl WarOrder {
    /// The rounds left of the budget at `round`.
    pub fn rounds_left(&self, round: u32) -> u32 {
        (self.since + self.budget).saturating_sub(round)
    }
}

/// The war the seat wants.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum WarPlan {
    /// No offensive war: the war machine picks no target of its own.
    #[default]
    NoWar,
    Order(WarOrder),
}

impl WarPlan {
    pub fn order(&self) -> Option<&WarOrder> {
        match self {
            WarPlan::NoWar => None,
            WarPlan::Order(order) => Some(order),
        }
    }
}

/// Whether a rival gets our help (§4.6).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum Help {
    #[default]
    Give,
    /// The runaway: no friendship, research agreement, pact, open borders, strategic copy.
    Deny,
}

/// The rival the congress proposal and vote embargo (§4.6).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum Embargo {
    #[default]
    Nobody,
    On(PlayerId),
}

/// The ships an operation sails with (§4.4).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum Escorts {
    /// `fleet.rs` as today.
    #[default]
    AsToday,
    /// An `Overseas` order: this many warships escort the landing.
    Ships(u32),
}

/// What the Great Generals do (§4.4).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum Generals {
    /// `great_people.rs` as today.
    #[default]
    AsToday,
    /// No war order and not at war: the generals stay in a city.
    Hold,
}

/// The guard level of one own city, 0…1000 (§4.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CityGuard {
    pub city: EntityId,
    pub level: Milli,
}

/// The guard from which a city may buy a defender at peace (§4.5).
pub const GUARD_PURCHASE: Milli = 600;

/// The rival concepts of one rival (§4.2), each 0…1000, and what follows from them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RivalRead {
    pub rival: PlayerId,
    pub weak: Milli,
    pub reach: Milli,
    pub menace: Milli,
    pub distrust: Milli,
    pub prey: Milli,
    pub runaway: Milli,
    pub spoiler: Milli,
    pub retake: Milli,
    pub duty: Milli,
    pub grabber: Milli,
    /// AND(grabber, weak): the value of a `Border` war.
    pub border: Milli,
    /// max of the war kinds (§4.4).
    pub war_value: Milli,
    /// The argmax kind (Overseas when not a retake and no land route).
    pub war_kind: WarKind,
    /// 1000 − distrust.
    pub trust: Milli,
    pub help: Help,
}

/// What the executors read (`TurnBrief::net2`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Net2Plan {
    pub round: u32,
    pub war: WarPlan,
    /// Rivals at war with us we want peace with (§4.4).
    pub peace_with: Vec<PlayerId>,
    /// Per met major rival, by id.
    pub rivals: Vec<RivalRead>,
    /// Per own city.
    pub guard: Vec<CityGuard>,
    /// The share of the gold for city-states, 0…1000 (0: today's reserve and ceiling).
    pub cs_gold_share: Milli,
    pub embargo: Embargo,
    pub escorts: Escorts,
    pub generals: Generals,
    /// The style share of tall (the site head's S9).
    pub tall_share: Milli,
}

/// The trust of a rival without a reading (a city-state, a stranger): a stranger is not yet a liar.
pub const TRUST_UNKNOWN: Milli = 1000;

/// The trust below which items that flow later are worth nothing and no open borders pass (§4.6).
pub const TRUST_MIN: Milli = 500;

impl Net2Plan {
    pub fn rival(&self, rival: PlayerId) -> Option<&RivalRead> {
        self.rivals.iter().find(|r| r.rival == rival)
    }

    /// The war order on `rival`, if the order names it.
    pub fn war_on(&self, rival: PlayerId) -> Option<&WarOrder> {
        self.war.order().filter(|o| o.rival == rival)
    }

    /// `trust[r]`, 0…1000 (`TRUST_UNKNOWN` without a reading).
    pub fn trust(&self, rival: PlayerId) -> Milli {
        match self.rival(rival) {
            Some(read) => read.trust,
            None => TRUST_UNKNOWN,
        }
    }

    /// `deny_help[r]` (§4.6).
    pub fn denies_help(&self, rival: PlayerId) -> bool {
        self.rival(rival).is_some_and(|r| r.help == Help::Deny)
    }

    /// `peace_with ∋ r` (§4.4).
    pub fn makes_peace_with(&self, rival: PlayerId) -> bool {
        self.peace_with.contains(&rival)
    }

    /// `guard[c]` (0 for a city without a reading).
    pub fn guard_of(&self, city: EntityId) -> Milli {
        match self.guard.iter().find(|g| g.city == city) {
            Some(g) => g.level,
            None => 0,
        }
    }

    /// `menace[r]` for the site head's S5 (0 without a reading).
    pub fn menace_of(&self, rival: PlayerId) -> Milli {
        match self.rival(rival) {
            Some(read) => read.menace,
            None => 0,
        }
    }
}

/// The answer of `net2::think`: the strategist's plan (the intent's style, aim and posture, the 15
/// price multipliers, the city target; read by the economy heads as today, `TurnBrief::strategy`) and
/// the levers (`TurnBrief::net2`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan2 {
    pub strategy: Plan,
    pub net2: Net2Plan,
}

// ---- the memory between turns (design §3.3: `NET2_SECTION` inside `strategy_net::Memory`) ----

/// A war order that ran out of budget without paying: no new order on its rival until `until`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpentWar {
    pub rival: PlayerId,
    pub until: u32,
}

/// The core's memory (part B): the held war order and the spent ones.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CoreMemory {
    pub war: WarPlan,
    pub spent: Vec<SpentWar>,
}

/// `net2`'s memory: the core's and the inputs' (part A's own type, `inputs::InputMemory`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Memory2 {
    pub core: CoreMemory,
    pub inputs: super::inputs::InputMemory,
}
