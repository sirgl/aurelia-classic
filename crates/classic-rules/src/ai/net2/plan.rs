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
use crate::ids::{EntityId, PlayerId, ResourceId, TileId};

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
    /// G11 (offence addendum §2.1): the warmonger an average met major holds of us, / 250.
    pub own_infamy: Milli,
    /// G12: friendships, research agreements and pacts with majors other than the order's target, / 4.
    pub ties_at_risk: Milli,
    /// G13: luxury kinds with ≥ 2 copies + strategic copies beyond the queue, / 3.
    pub spare_copies: Milli,
    /// G14: the World Leader within the alliances we can buy before a session within 5 paced rounds.
    pub vote_reach: Milli,
    /// G15: own workers < cities / 2, early (0/1000).
    pub worker_need: Milli,
    /// The space race is open to us and a part is missing (0/1000; `weights::g::SPACE_RACE`).
    pub space_race: Milli,
    /// The copies the missing parts need beyond the free ones, 0…3 → 0…1000 (`weights::g::PARTS_SHORT`).
    pub parts_short: Milli,
    /// The new cities the rules' size model pays for a real tall seat (`tall_real::SizeModel::more`), 250 a city (4 → 1000;
    /// `weights::g::SIZE_MODEL`); 0 for another seat. Set after the intent (`net2::cap_cities`), read by `CITY_CAP` only.
    pub size_model: Milli,
    /// A spaceship part of ours stands outside the capital (0/1000; `weights::g::PART_OUT`).
    pub part_out: Milli,
    /// The opening's cities the size model does not found, 250 a city (`weights::g::OPENING`; `net2::opening`); 0 unless
    /// the seat plays `early-growth` (`TurnBrief::early_growth`). Set after the intent, read by `CITY_CAP` only.
    pub opening: Milli,
    /// The opening's cities still to found, the model's among them, 250 a city (`weights::g::OPENING_GAP`;
    /// `net2::opening_gap`); 0 unless `early-growth`. Set after the intent, read by `EXPANSION_TILT` only.
    pub opening_gap: Milli,
    /// Luxury kinds lying unconnected on our tiles beyond our workers, 500 a kind (`weights::g::LUX_OPEN`;
    /// `net2::lux_open`); 0 unless `early-growth`. Read by `WORKER_TILT` only.
    pub lux_open: Milli,
}

/// One strategic resource the missing spaceship parts need (the hold's quantity, `Net2Plan::space_hold`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpaceNeed {
    pub resource: ResourceId,
    /// Copies the parts not yet built or walking need.
    pub need: i32,
    /// Copies free now (`resources.available`).
    pub free: i32,
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
    /// R11: at peace and at war, its military power inside our borders or within `THREAT_RADIUS` of our cities over our power there.
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
    /// R25 (offence addendum §2.2): its seen cities near ours first seen < 20 paced rounds ago with ≤ 3 citizens, share.
    pub young: Milli,
    /// R26: its seen-city growth over 40 paced rounds minus ours, −2…+4 → 0…1000 (333 = even).
    pub spread: Milli,
    /// R27: its spaceship progress.
    pub space: Milli,
    /// R28: its visible luxury or strategic tiles we lack within 2 of our border, 0…2 → 0…1000.
    pub lux_near: Milli,
    /// R29: it grants us entry (0/1000).
    pub entry: Milli,
    /// R30: our era ahead of its (0/1000).
    pub era_ahead: Milli,
    /// R31: city-states allied to it / 3.
    pub cs_held: Milli,
    /// An operation of ours against it forms or marches (0/1000): an order is not dropped under its army.
    pub operating: Milli,
    /// R32 (GAP.md): the league elects a World Leader and its delegates on the Congress screen reach the votes
    /// needed by themselves (0/1000): it wins the vote unless a tie or a higher candidate stops it.
    pub alone: Milli,
    /// `science-flow`: the rounds our gold has waited for its research agreement unsigned (`science::read_waits`; 0 for
    /// every other variant).
    pub chest_wait: Milli,
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
            young: 0,
            spread: 333,
            space: 0,
            lux_near: 0,
            entry: 0,
            era_ahead: 0,
            cs_held: 0,
            operating: 0,
            alone: 0,
            chest_wait: 0,
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

/// The inputs of a hire (offence addendum §2.3): an ordered pair of met majors, `hired` ≠ `target`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PairInputs {
    pub hired: PlayerId,
    pub target: PlayerId,
    /// P1: 1000 at ≤ 6 tiles between their nearest seen cities, 0 at ≥ 16.
    pub near: Milli,
    /// P2: the hired's soldiers over both (500 level).
    pub matched: Milli,
    /// P3: they are at war (0/1000).
    pub at_war: Milli,
    /// P4: the hired refused us within 30 paced rounds (0/1000).
    pub refused: Milli,
}

/// The inputs of one seen rival city within `DECISIVE_RANGE` (offence addendum §2.4, T1–T6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AimInputs {
    pub rival: PlayerId,
    pub city: TileId,
    pub capital: Milli,
    pub young: Milli,
    pub lux: Milli,
    pub holy: Milli,
    pub worth: Milli,
    pub ring: Milli,
}

/// The inputs of one known city of a met major within `DECISIVE_RANGE` of ours (T1–T4, 0…1000): how easy
/// it is to take, read by `weights::EASE` (WAR-LEVER.md §2). The war order aims at its rival's easiest one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TargetInputs {
    pub rival: PlayerId,
    pub city: TileId,
    /// T1: its citizens, 2…14 → 0…1000.
    pub size: Milli,
    /// T2: the military power seen on it or within `DEFENCE_RADIUS` over that and our military power (0: none seen).
    pub garrison: Milli,
    /// T3: our nearest city to it, R1's ramp: 1000 at ≤ 5 tiles, 0 at ≥ 15.
    pub near: Milli,
    /// T4: its estimated strength (`operations::estimate_target`: the base, the citizens, the technology and the
    /// walls the rules give by the technologies we know) over that and our military power.
    pub walls: Milli,
}

/// The most targets read per turn (the nearest first).
pub const MAX_TARGETS: usize = 40;

/// Everything `net2` reads in a turn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Inputs {
    /// The carried features 0–53 of `strategy_net::features` (54–67 are not read by `net2`).
    pub situation: [Milli; fe::N],
    pub seat: SeatInputs,
    /// At most `MAX_RIVALS`, by id.
    pub rivals: Vec<RivalInputs>,
    pub cities: Vec<CityInputs>,
    /// Every ordered pair of met majors (the hire).
    pub pairs: Vec<PairInputs>,
    /// The seen rival cities a war may aim at, at most 40.
    pub aims: Vec<AimInputs>,
    /// The known rival cities within reach and how easy each is to take, at most `MAX_TARGETS`.
    pub targets: Vec<TargetInputs>,
    /// The strategic resources the missing spaceship parts need, while the race is open (empty else).
    pub space: Vec<SpaceNeed>,
    /// The capital's tile, where the parts are installed (`victory::capital_tile`; the tile `capital_gate` keeps).
    pub capital: Option<crate::ids::TileId>,
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
    /// S10 (offence addendum §2.5): kinds we lack in the site's radius within 3 of a rival's border, / 2.
    pub claim: Milli,
    /// S11 (`early-growth`): the land score of the site's tiles within the work radius of an own city (their
    /// `tile_score`, the radius weight of the land score), points; 0 when `Growth::shared` is 0.
    pub shared: i64,
    /// S12: `Growth::shared`, the weight of S11 (thousandths).
    pub shared_weight: Milli,
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

    /// Retake and Duty wait only the rules' treaty (`PEACE_DURATION`), not `PEACE_COOLDOWN_ROUNDS`.
    pub fn waits_own_cooldown(self) -> bool {
        !matches!(self, WarKind::Retake | WarKind::Duty)
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

/// Why a war order aims at its city (offence addendum §3.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum AimWhy {
    Capital,
    Young,
    Resource,
    Holy,
    Worth,
    Parts,
    /// The rival's city that is easiest to take (`weights::EASE`: small, bare, near, weak walls).
    Ease,
}

/// The city a war order aims at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WarAim {
    pub city: TileId,
    pub why: AimWhy,
}

/// How a war order opens (offence addendum §3.3, MO3).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum Opening {
    #[default]
    Plain,
    /// Stage inside the target's land under its entry, declare when assembled (by `until` at the latest).
    Staged { since: u32, until: u32 },
}

/// Where a war order stands: its budget is the war's, from the declaration (§3.3: a war that has not paid by
/// its budget ends), and the army it waits for has the war machine's forming time (WAR-LEVER.md §2).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum OrderPhase {
    /// At peace with its rival: the army forms, at most `weights::gate::PREPARE_ROUNDS` paced rounds from `since`.
    #[default]
    Preparing,
    /// At war with its rival since this round.
    Fighting { since: u32 },
}

/// A war order: one at a time (§4.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WarOrder {
    pub rival: PlayerId,
    pub kind: WarKind,
    /// The round the order started.
    pub since: u32,
    /// The war's budget in paced rounds (`WarKind::budget` × pace), from the declaration (`OrderPhase::Fighting`).
    pub budget: u32,
    pub phase: OrderPhase,
    /// Its war value this turn (≥ 350 while kept).
    pub value: Milli,
    /// The named city (None: any city of the rival, as the war machine picks today).
    pub aim: Option<WarAim>,
    pub opening: Opening,
    /// The war machine pillages the order's enemy where no capture fits (`raid_war`).
    pub raid: bool,
}

impl WarOrder {
    /// The round its war began (None: it prepares).
    pub fn war_since(&self) -> Option<u32> {
        match self.phase {
            OrderPhase::Preparing => None,
            OrderPhase::Fighting { since } => Some(since),
        }
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

/// The seat's ballot in the World Leader vote (GAP.md: the congress part).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum Ballot {
    /// For ourselves.
    #[default]
    Own,
    /// Against this rival: the delegates go to the strongest other candidate, so that a tie or a higher one stops it.
    Block(PlayerId),
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

/// A hire: `hired` is paid to declare on `target` (offence addendum §3.3, D4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HireOrder {
    pub hired: PlayerId,
    pub target: PlayerId,
    pub gold: u32,
    pub gold_per_turn: u32,
    pub since: u32,
}

/// Spare copies sold for gold a turn to the order's target until the declaration (D1/D2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MilkOrder {
    pub target: PlayerId,
    pub until: u32,
}

/// The final city-state sweep before a congress session (CS6/W2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CsSweep {
    pub until: u32,
}

/// The offensive levers (offence addendum §3.3); `Default` is the design's play (`net2-defence`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Offence {
    pub hire: Option<HireOrder>,
    pub milk: Option<MilkOrder>,
    pub sell_spares: bool,
    pub ask_entry: Option<PlayerId>,
    pub tourism_entry: bool,
    pub cs_sweep: Option<CsSweep>,
    pub ban_luxury: Option<ResourceId>,
    pub coup_floor: Option<u8>,
    pub bully_worker: Option<PlayerId>,
    pub faith_target: Option<PlayerId>,
    pub citadel_at: Option<TileId>,
    pub liberate: bool,
}

/// The guard level of one own city, 0…1000 (§4.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CityGuard {
    pub city: EntityId,
    pub level: Milli,
}

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
    /// Its World Leader vote is to be blocked (`weights::RIVAL` `block`).
    pub block: Milli,
    /// AND(grabber, weak): the value of a `Border` war.
    pub border: Milli,
    /// A runaway we cannot beat ourselves (offence addendum §3.2; 0 until the offence is built).
    pub brake: Milli,
    /// max of the war kinds (§4.4).
    pub war_value: Milli,
    /// The argmax kind (Overseas when not a retake and no land route).
    pub war_kind: WarKind,
    /// 1000 − distrust.
    pub trust: Milli,
    pub help: Help,
    /// Part A's estimate of its soldiers (§2.4), for the war machine's `known_power`.
    pub soldiers: Soldiers,
}

/// A rival's soldiers as part A's estimator reads them (§2.4, `inputs::SoldierRead`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Soldiers {
    /// No estimate this turn (the read has not run, or the rival is not in it).
    #[default]
    Unread,
    /// The estimated count and its doubt: the width of its range over its size, thousandths (0: exact).
    Estimate { count: i64, doubt: Milli },
}

/// The doubt up to which the war machine counts the estimate instead of its army per city (§2.4 step 4).
pub const SOLDIERS_DOUBT_MAX: Milli = 300;

// ---- the parts of `net2` (net2/GAP.md: the ablation) ----

/// A part of `net2`: the levers that one bench variant `net2-no-<part>` gives back to `nets-x-site`'s play
/// (the ablation that finds where the real victories go). Every reader of the plan names its part
/// (`operations::with_net2`); `net2::think` reads the parts that live inside the net.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Part {
    /// The intent's tilt (every term of `weights::TILT` but the war order's) and the tall first.
    Strategy,
    /// The war order: `net2` orders no war and the war machine picks as `nets-x-site`; with it go the order's
    /// posture terms, the retake margin and the soldiers estimate in `known_power`.
    War,
    /// Trust, the help denied and the refusals that follow, the price of a third-party war, no denunciation.
    Deals,
    /// The peace wish (`Net2Plan::peace_with`).
    Peace,
    /// The city-state gold share.
    CityStates,
    /// The congress: the embargo, the World Leader ballot and the stances.
    Congress,
    /// The site head.
    Sites,
    /// The escorts of an overseas order and the Great Generals held.
    Fleet,
    /// The guard levels of the cities and the rules' zone damage in the danger read.
    Guard,
    /// The tilt of the military price.
    Economy,
}

impl Part {
    pub const ALL: [Part; 10] = [Part::Strategy, Part::War, Part::Deals, Part::Peace, Part::CityStates, Part::Congress, Part::Sites, Part::Fleet, Part::Guard, Part::Economy];

    pub fn name(self) -> &'static str {
        match self {
            Part::Strategy => "strategy",
            Part::War => "war",
            Part::Deals => "deals",
            Part::Peace => "peace",
            Part::CityStates => "cs",
            Part::Congress => "congress",
            Part::Sites => "sites",
            Part::Fleet => "fleet",
            Part::Guard => "guard",
            Part::Economy => "economy",
        }
    }
}

/// The parts of `net2` a seat plays.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Parts {
    #[default]
    All,
    /// Every part but one (`net2-no-<part>`).
    Without(Part),
    /// No part: every reader plays as `nets-x-site` (`net2-off`, the check of the ablation itself).
    Nothing,
}

impl Parts {
    pub fn plays(self, part: Part) -> bool {
        match self {
            Parts::All => true,
            Parts::Without(off) => off != part,
            Parts::Nothing => false,
        }
    }
}

/// What the executors read (`TurnBrief::net2`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Net2Plan {
    /// The parts that play: a reader of a part that does not finds no plan (`operations::with_net2`).
    pub parts: Parts,
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
    /// The World Leader ballot.
    pub ballot: Ballot,
    pub escorts: Escorts,
    pub generals: Generals,
    /// The style share of tall (the site head's S9).
    pub tall_share: Milli,
    pub offence: Offence,
    /// Every known city of the order's rival with its ease (`weights::EASE`), the easiest (the aim) first
    /// (`net2/orders.rs`): what the order's war promises when no operation runs yet, read by the peace answer; when
    /// the aimed city has just been taken the next one prices the war that goes on (CHAIN.md D8, HD8 #1 r114).
    pub order_eases: Vec<(crate::ids::TileId, Milli)>,
    /// Copies of a strategic resource held for the missing spaceship parts (`weights::PART_HOLD` × the copies they
    /// need): no other unit is built, bought or upgraded into with them (the production planner, the upgrades, the
    /// armory read `held`).
    pub space_hold: Vec<(ResourceId, i32)>,
    /// The research stays on the spaceship's technologies and their prerequisites while one of them is available
    /// (`weights::SPACE_RESEARCH` at least `gate::SPACE_RESEARCH`; the research head, `strategy_net::heads::research`,
    /// reads it).
    pub space_research: bool,
    /// The capital is kept for the spaceship's parts (`weights::CAPITAL_GATE` at least `gate::CAPITAL_GATE`): no own
    /// civilian parks in it, the civilians in it walk out around the units in the way, and the caravans in it or at its
    /// gate go to another city (`planner_workers::park` and `victory::Walk::Gated` read it). The capital's tile when
    /// kept, else None.
    pub capital_gate: Option<crate::ids::TileId>,
    /// `science-flow`'s levers (`net2::science`; all off for every other variant).
    pub science: Science,
    /// `early-growth`'s levers (`net2::growth`; all off for every other variant).
    pub growth: Growth,
}

/// The early growth's levers (`weights::GARRISON`, `weights::BUY_CHEST`, `weights::site::SHARED`), read by the city
/// planner's garrison, the allocator's gold and the site head. Off (empty, zeros) unless the seat plays `early-growth`
/// (`TurnBrief::early_growth`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Growth {
    /// The garrison lever plays (`early-growth`): only the cities in `garrisons` build their flat garrison.
    pub plays: bool,
    /// Own cities whose garrison is built (`weights::GARRISON` at least `gate::GARRISON`). A city not read (founded
    /// after the plan) waits for the next turn's reading: `planner_cities::garrison_order` orders none there; a
    /// threat is the defence watch's (`defence_orders`).
    pub garrisons: Vec<EntityId>,
    /// Gold the tiles and the gifts leave for a growth purchase (`weights::BUY_CHEST` × the hurry price of the cheapest
    /// growth item heading a queue; `allocator::spend_gold`).
    pub chest: u32,
    /// The site head's weight of the land a site shares with an own city (`weights::site::SHARED`, thousandths of
    /// the land score's weight of those tiles); 0: no term.
    pub shared: Milli,
    /// The jobs the workers have ahead (`net2::growth::jobs_ahead`): the tiles the cities work now and will work next
    /// with a job the known technologies allow; the allocator's worker counts at least these (0: its own count).
    pub jobs_ahead: u32,
}

/// The science flow's levers (`weights::SCIENTISTS`, `weights::AGREEMENT`), read by the citizens' executor, the diplomacy
/// and the allocator's gold. Off (zeros, empty) unless the seat plays `science-flow` (`TurnBrief::science_flow`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Science {
    /// The share of the science specialists' slots the cities fill (thousandths; `citizens::science_specialist_commands`).
    pub scientists: Milli,
    /// The rivals a research agreement with pays our race (`weights::AGREEMENT` ≥ `gate::AGREEMENT`): the friendship
    /// and the agreement with them are proposed and taken though the rival runs away (not the war order's target).
    pub ties: Vec<Tie>,
    /// Gold kept for the agreements the ties still lack (the rules' cost each; `allocator::spend_gold` does not spend it).
    pub chest: u32,
}

/// A rival of the science ties and the cadence of its ladder (embassies, friendship, the agreement).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tie {
    pub rival: PlayerId,
    /// Rounds between two offers of the ladder (`science::cadence` of the tie's `weights::AGREEMENT` level, at least 1):
    /// the diplomacy offers it in the rounds where (round + our id) is a multiple of it.
    pub every: u32,
}

impl Tie {
    /// The ladder is offered to the tie in `round` by `player`.
    pub fn asks_in(&self, round: u32, player: PlayerId) -> bool {
        (round + u32::from(player.0)) % self.every.max(1) == 0
    }
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

    /// The copies of `resource` held for the spaceship's parts (0 without a hold).
    pub fn held(&self, resource: ResourceId) -> i32 {
        self.space_hold.iter().filter(|(r, _)| *r == resource).map(|(_, n)| *n).sum()
    }

    /// `deny_help[r]` (§4.6).
    pub fn denies_help(&self, rival: PlayerId) -> bool {
        self.rival(rival).is_some_and(|r| r.help == Help::Deny)
    }

    /// `denies_help` for one item: the friendship and the research agreement with a rival of the science ties
    /// (`Science::ties`) are given though it runs away.
    pub fn denies_item(&self, rival: PlayerId, item: crate::world::relations::DealItem) -> bool {
        use crate::world::relations::DealItem;
        if matches!(item, DealItem::Friendship | DealItem::ResearchAgreement) && self.ties_with(rival) {
            return false;
        }
        self.denies_help(rival)
    }

    /// A research agreement with `rival` pays our race (`Science::ties`).
    pub fn ties_with(&self, rival: PlayerId) -> bool {
        self.tie(rival).is_some()
    }

    /// The tie with `rival` (`Science::ties`), if any.
    pub fn tie(&self, rival: PlayerId) -> Option<&Tie> {
        self.science.ties.iter().find(|t| t.rival == rival)
    }

    /// `peace_with ∋ r` (§4.4).
    pub fn makes_peace_with(&self, rival: PlayerId) -> bool {
        self.peace_with.contains(&rival)
    }

    /// `guard[c]` (0 for a city without a reading).
    /// `early-growth`: the city's garrison waits (`Growth::garrisons`).
    pub fn garrison_waits(&self, city: EntityId) -> bool {
        self.growth.plays && !self.growth.garrisons.contains(&city)
    }

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
    /// Targets hired against: no new hire on one until `until`.
    pub hires: Vec<SpentWar>,
}

/// `net2`'s memory: the core's, the inputs' (part A's own type, `inputs::InputMemory`) and the science flow's.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Memory2 {
    pub core: CoreMemory,
    pub inputs: super::inputs::InputMemory,
    /// `science-flow`'s own; empty for every other variant, whose bytes it leaves as they were (`memory.rs`).
    pub science: ScienceMemory,
}

/// What `science-flow` keeps between turns: how long its gold has waited for each partner's research agreement.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScienceMemory {
    /// The round of the last turn written (a second pass in one round adds nothing).
    pub round: u32,
    pub waits: Vec<ChestWait>,
}

/// The rounds the chest held a partner's agreement's gold without the partner signing (`weights::AGREEMENT`'s
/// `r::CHEST_WAIT`). Kept while the friendship lasts; an agreement signed or the friendship gone starts it again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChestWait {
    pub rival: PlayerId,
    pub rounds: u32,
}
