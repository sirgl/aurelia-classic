//! Every hand weight of `net2` (work/new-net-design-2026-11-22.md §3–§4), each with the reason it has
//! its value. Nothing here was trained. Values are thousandths (`Milli`: 1000 = 1.0).
//!
//! A neuron is `bias + (Σ w·shape(x)) / 1000` (one division: the float twin matches it within a
//! thousandth), then its activation. The layers a term may read:
//!
//! | source | what | computed by |
//! |---|---|---|
//! | `F` | the carried features (`strategy_net::weights::fe`) | `strategy_net::situation` |
//! | `K` | the 13 carried concepts (`strategy_net::weights::kc`) | `strategy_net::concepts_net` |
//! | `G` | the seat inputs G1–G10 (`g`) | part A |
//! | `S` | the new seat concepts (`s`, `SEAT`) | here |
//! | `R` | one rival's inputs R1–R24 (`r`) | part A |
//! | `Q` | one rival's concepts (`q`, `RIVAL`), the earlier ones of the same rival | here |
//! | `A` | the aggregates over the rivals and the war order (`a`) | `levers` |
//! | `Y` | the intent's shares: 3 styles, 4 aims, 4 postures (`y`) | `strategy_net::intent_from` |
//! | `C` | one city's guard inputs (`c`) | `levers` |
//!
//! Deviations from the design's formulas, each a defect of the class "a war kind opens without the
//! thing it is about": `prey` and `duty` need the reach (the design's would order a war on a disarmed
//! nation or an ally's attacker on another continent: `weak` 1000 alone gave 700), `retake` needs a
//! lost city in the rival's hands (the design's gave 600 to any war fought with local strength). The
//! worked cases of §4.8 keep their verdicts (`tests`).

use crate::ai::strategy_net::weights::{fe, kc, Act};
use crate::ai::strategy_net::Milli;

/// Where a term's value comes from (the table in the module's doc).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Src {
    F(usize),
    K(usize),
    G(usize),
    S(usize),
    R(usize),
    Q(usize),
    A(usize),
    Y(usize),
    C(usize),
}

/// How a term reads its source `x` (0…1000).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    Plain,
    /// 1000 − x.
    Not,
    /// max(0, x − c).
    Above(Milli),
    /// x − c (signed).
    Centered(Milli),
    /// x · y / 1000.
    Times(Src),
}

/// How a neuron combines its terms.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Join {
    /// bias + Σ, then the activation.
    Sum(Act),
    /// The largest term, clamped to 0…1000 (the bias is not read).
    Max,
}

/// One weighted input and why it has that weight.
#[derive(Debug, Clone, Copy)]
pub struct Term {
    pub src: Src,
    pub shape: Shape,
    pub w: Milli,
    pub why: &'static str,
}

/// A neuron of `net2`.
#[derive(Debug, Clone, Copy)]
pub struct Neuron {
    pub name: &'static str,
    pub bias: Milli,
    pub why: &'static str,
    pub join: Join,
    pub terms: &'static [Term],
}

const fn t(src: Src, w: Milli, why: &'static str) -> Term {
    Term { src, shape: Shape::Plain, w, why }
}

const fn ts(src: Src, shape: Shape, w: Milli, why: &'static str) -> Term {
    Term { src, shape, w, why }
}

const UNIT: Join = Join::Sum(Act::Unit);
const LINEAR: Join = Join::Sum(Act::Linear);

/// The seat inputs (`plan::SeatInputs` in order).
pub mod g {
    pub const CAPITAL_LOST: usize = 0;
    pub const CITIES_LOST: usize = 1;
    pub const WAR_ROUNDS: usize = 2;
    pub const WAR_GAIN: usize = 3;
    pub const UNHAPPY_TREND: usize = 4;
    pub const EMBARK: usize = 5;
    pub const ENEMY_AIR: usize = 6;
    pub const GOLD_STOCK: usize = 7;
    pub const CS_ALLIES: usize = 8;
    pub const CS_CONTESTED: usize = 9;
    pub const N: usize = 10;
    pub const NAMES: [&str; N] = ["capital_lost", "cities_lost", "war_rounds", "war_gain", "unhappy_trend", "embark", "enemy_air", "gold_stock", "cs_allies", "cs_contested"];
}

/// A rival's inputs (`plan::RivalInputs` in order).
pub mod r {
    pub const NEAR: usize = 0;
    pub const LAND: usize = 1;
    pub const SEA: usize = 2;
    pub const SOLDIERS: usize = 3;
    pub const SEEN_ARMY: usize = 4;
    pub const OPEN_CITIES: usize = 5;
    pub const TREE: usize = 6;
    pub const LEAD: usize = 7;
    pub const PACE: usize = 8;
    pub const CULTURE: usize = 9;
    pub const STACK: usize = 10;
    pub const WARMONGER: usize = 11;
    pub const BETRAYED: usize = 12;
    pub const DENOUNCED_US: usize = 13;
    pub const FRIEND: usize = 14;
    pub const PACT: usize = 15;
    pub const AT_WAR: usize = 16;
    pub const WAR_ROUNDS: usize = 17;
    pub const HOLDS_CAPITAL: usize = 18;
    pub const HOLDS_CITIES: usize = 19;
    pub const GRAB: usize = 20;
    pub const ALLY_HIT: usize = 21;
    pub const LOCAL: usize = 22;
    pub const SCORE: usize = 23;
    pub const RETAKE_CASE: usize = 24;
    pub const CAPTURED_RECENT: usize = 25;
    pub const N: usize = 26;
    pub const NAMES: [&str; N] = [
        "near", "land", "sea", "soldiers", "seen_army", "open_cities", "tree", "lead", "pace", "culture", "stack", "warmonger", "betrayed", "denounced_us", "friend", "pact", "at_war",
        "war_rounds", "holds_capital", "holds_cities", "grab", "ally_hit", "local", "score", "retake_case", "captured_recent",
    ];
}

/// The new seat concepts (`SEAT` in order).
pub mod s {
    pub const WAR_WEARY: usize = 0;
    pub const CAPITAL_FALL: usize = 1;
    pub const CITY_STATE_ROOM: usize = 2;
    pub const AIR_NEED: usize = 3;
    pub const N: usize = 4;
}

/// A rival's concepts (`RIVAL` in order; a concept reads only earlier ones).
pub mod q {
    pub const WEAK: usize = 0;
    pub const REACH: usize = 1;
    pub const DISTRUST: usize = 2;
    pub const MENACE: usize = 3;
    pub const RUNAWAY: usize = 4;
    pub const PREY: usize = 5;
    pub const SPOILER: usize = 6;
    pub const RETAKE: usize = 7;
    pub const DUTY: usize = 8;
    pub const GRABBER: usize = 9;
    pub const BORDER: usize = 10;
    pub const WAR_VALUE: usize = 11;
    pub const N: usize = 12;
    /// The war kinds' values in `plan::WarKind`'s order of the argmax (Prey, Retake, Spoiler, Duty, Border).
    pub const KINDS: [usize; 5] = [PREY, RETAKE, SPOILER, DUTY, BORDER];
}

/// The aggregates over the rivals and the war order (`levers::aggregates`).
pub mod a {
    /// 1000: a term on it is a bias change.
    pub const ONE: usize = 0;
    pub const MAX_PREY: usize = 1;
    pub const MAX_MENACE: usize = 2;
    /// The order's value while at peace with its rival (0 else).
    pub const ORDER_AT_PEACE: usize = 3;
    /// The order's value while at war with its rival (0 else).
    pub const ORDER_AT_WAR: usize = 4;
    /// 1000 when there is no order and no war.
    pub const NO_ORDER: usize = 5;
    /// The order's value (0 without one).
    pub const ORDER_VALUE: usize = 6;
    pub const N: usize = 7;
    pub const NAMES: [&str; N] = ["one", "max_prey", "max_menace", "order_at_peace", "order_at_war", "no_order", "order_value"];
}

/// The intent's shares (`Y`): the styles, the aims, the postures.
pub mod y {
    pub const STYLE_TALL: usize = 0;
    pub const AIM_DIPLOMACY: usize = 6;
    pub const POSTURE_PREPARE: usize = 9;
    pub const N: usize = 11;
}

/// One city's guard inputs (`levers::city_row`).
pub mod c {
    /// The menace of the owner of the strongest stack near the city (0: none).
    pub const STACK_MENACE: usize = 0;
    pub const DANGER: usize = 1;
    /// The largest distrust of the rivals with a city within 8 tiles.
    pub const DISTRUST_NEAR: usize = 2;
    pub const YOUNG: usize = 3;
    pub const N: usize = 4;
}

use Src::{A, C, F, G, K, Q, R, S, Y};

// ---- §4.1 seat concepts ----

pub const SEAT: [Neuron; s::N] = [
    Neuron {
        name: "war_weary",
        bias: -400,
        why: "a short war is no weariness",
        join: UNIT,
        terms: &[
            t(G(g::WAR_ROUNDS), 1200, "a war past 30 rounds without an end is the rebels' road (F3)"),
            t(G(g::UNHAPPY_TREND), 900, "falling happiness is the first sign"),
            t(K(kc::HAPPINESS_CRISIS), 600, "a crisis: the rules cut growth"),
            ts(F(fe::FORCE), Shape::Not, 500, "our force low against what we see: we are losing"),
            ts(G(g::WAR_GAIN), Shape::Above(500), -1600, "a war that captures (gain above even) pays its unrest"),
        ],
    },
    Neuron {
        name: "capital_fall",
        bias: -200,
        why: "one lost city far from home is not a fall",
        join: UNIT,
        terms: &[
            t(G(g::CAPITAL_LOST), 1200, "a lost capital is the worst state the exam measures (B4 seat alive 0.48 at L2)"),
            t(G(g::CITIES_LOST), 400, "a run of lost cities is the same danger"),
        ],
    },
    Neuron {
        name: "city_state_room",
        bias: -300,
        why: "a little gold and no contested city-state is no diplomatic road",
        join: UNIT,
        terms: &[
            t(G(g::GOLD_STOCK), 600, "gold in the treasury buys the alliances (F2)"),
            t(G(g::CS_CONTESTED), 500, "city-states a little behind the leader are cheap flips"),
            t(K(kc::CITY_STATE_PLAY), 400, "the leader's and the map's city-state play"),
            t(K(kc::TALL_GROWTH), 300, "the diplomatic winners are tall (~6 allies, F2)"),
            t(G(g::CS_ALLIES), 0, "read, no weight: §4.1 names no term; the trainer's to set"),
        ],
    },
    Neuron {
        name: "air_need",
        bias: -500,
        why: "no fighters without enemy air",
        join: UNIT,
        terms: &[
            t(G(g::ENEMY_AIR), 1000, "enemy air seen is the only reason for fighters (A1 trick)"),
            t(F(fe::AT_WAR), 400, "at war the bombers come to us"),
        ],
    },
];

// ---- §4.2 rival concepts ----

pub const RIVAL: [Neuron; q::N] = [
    Neuron {
        name: "weak",
        bias: 1000,
        why: "1000 − 2·soldiers: a level army (500) → 0, no army → 1000",
        join: UNIT,
        terms: &[
            t(R(r::SOLDIERS), -2000, "its estimated soldiers over its and ours"),
            t(R(r::OPEN_CITIES), 300, "cities seen without a garrison"),
            ts(R(r::SEEN_ARMY), Shape::Above(500), -2000, "an army we saw above level beats the estimate"),
        ],
    },
    Neuron {
        name: "reach",
        bias: 0,
        why: "the larger of the land and the sea reach",
        join: Join::Max,
        terms: &[
            ts(R(r::LAND), Shape::Times(R(r::NEAR)), 1000, "a city we can walk to, by its nearness"),
            ts(R(r::SEA), Shape::Times(R(r::NEAR)), 600, "over water at 0.6: a landing costs turns and units (the `Overseas` kind)"),
        ],
    },
    Neuron {
        name: "distrust",
        bias: -500,
        why: "a stranger is not yet a liar",
        join: UNIT,
        terms: &[
            t(R(r::STACK), 700, "its army inside our borders: the army before the war (P4 sells then declares)"),
            t(R(r::WARMONGER), 500, "a warmonger"),
            t(R(r::BETRAYED), 900, "a past betrayal is the best predictor (D1–D6, P4)"),
            t(R(r::DENOUNCED_US), 500, "it denounced us"),
            t(R(r::NEAR), 300, "only a neighbour can strike"),
            ts(R(r::SOLDIERS), Shape::Above(500), 800, "stronger than us"),
            t(R(r::PACT), -600, "a pact partner is bound"),
            t(R(r::FRIEND), -300, "a friend is bound"),
        ],
    },
    Neuron {
        name: "menace",
        bias: -300,
        why: "a few units passing are no menace",
        join: UNIT,
        terms: &[
            t(R(r::STACK), 1500, "a stack inside our borders at peace is the sneak attack (P3, MO3, B2)"),
            t(Q(q::DISTRUST), 400, "distrust raises it"),
            t(R(r::FRIEND), -300, "a friend's stack less"),
        ],
    },
    Neuron {
        name: "runaway",
        bias: -1000,
        why: "a rival level with us is no threat: the race is open (`weights::threat::BIAS`)",
        join: UNIT,
        terms: &[
            t(R(r::TREE), 1200, "its share of the tree is its road to the spaceship"),
            t(R(r::LEAD), 1000, "a tenth of the tree ahead of us"),
            ts(R(r::PACE), Shape::Centered(500), 1200, "it researches faster than we do: the lead grows"),
            ts(R(r::CULTURE), Shape::Above(600), 800, "a culture share above 0.6 is the tourism road (A3, C3)"),
        ],
    },
    Neuron {
        name: "prey",
        bias: -1700,
        why: "no war by default (F1); −1700 makes it AND(weak, reach): weakness alone (1600) does not open it",
        join: UNIT,
        terms: &[
            t(Q(q::WEAK), 1600, "the disarmed neighbour is the cheap city (A5, MO11, MO13)"),
            t(Q(q::REACH), 1600, "AND: a city we can reach (a disarmed nation over the ocean is no prey)"),
            t(K(kc::WARLIKE), 400, "a warlike leader"),
            t(R(r::GRAB), 300, "a grabber pays twice"),
            t(R(r::FRIEND), -1000, "never a friend"),
            t(R(r::PACT), -1200, "never a pact partner"),
            t(K(kc::HAPPINESS_CRISIS), -700, "not in a crisis: conquered cities deepen it"),
            t(K(kc::DANGER), -500, "not under threat elsewhere"),
            t(S(s::WAR_WEARY), -600, "not when tired"),
        ],
    },
    Neuron {
        name: "spoiler",
        bias: -1400,
        why: "the war on a runaway only when it is reachable and beatable locally (F3: as a posture term it opened wars on everyone)",
        join: UNIT,
        terms: &[
            t(Q(q::RUNAWAY), 1000, "the rival about to win"),
            t(Q(q::REACH), 700, "reachable"),
            t(Q(q::WEAK), 600, "beatable"),
            t(R(r::AT_WAR), 400, "already at war with it: continue"),
            ts(R(r::LOCAL), Shape::Centered(500), 1000, "our force where the war would be, against its"),
        ],
    },
    Neuron {
        name: "retake",
        bias: -900,
        why: "nothing of ours in its hands is nothing to retake: −900 keeps a war with local strength alone shut",
        join: UNIT,
        terms: &[
            t(R(r::HOLDS_CAPITAL), 1800, "our capital in its hands is the first war aim (B4, F7)"),
            t(R(r::HOLDS_CITIES), 1200, "our cities too: two of them open it with local strength even"),
            t(R(r::LOCAL), 600, "local force decides"),
            t(R(r::AT_WAR), 300, "the war is already on"),
            t(S(s::WAR_WEARY), -500, "tiredness lowers it, but a capital outweighs it"),
        ],
    },
    Neuron {
        name: "duty",
        bias: -1100,
        why: "no ally struck, no duty; −1100 makes it AND(ally struck, reach)",
        join: UNIT,
        terms: &[
            t(R(r::ALLY_HIT), 1300, "a pact partner or a friend attacked (D1, D2: ally struck 4 %)"),
            t(Q(q::REACH), 800, "AND: strike the attacker where we can reach"),
            t(Q(q::WEAK), 400, "a weak attacker is a cheap duty"),
            t(S(s::WAR_WEARY), -500, "not when tired"),
        ],
    },
    Neuron {
        name: "grabber",
        bias: -500,
        why: "a city far off is no grab",
        join: UNIT,
        terms: &[
            t(R(r::GRAB), 1200, "a forward city or a citadel on our land (P5, MO2, MO4)"),
            t(R(r::NEAR), 300, "it sits next to us"),
        ],
    },
    Neuron {
        name: "border",
        bias: -1000,
        why: "AND(grabber, weak): the forward city is taken back only from a weak grabber, else guarded",
        join: UNIT,
        terms: &[t(Q(q::GRABBER), 1000, "AND: a grabber …"), t(Q(q::WEAK), 1000, "… that is weak")],
    },
    Neuron {
        name: "war_value",
        bias: 0,
        why: "the strongest reason for a war on this rival (§4.4)",
        join: Join::Max,
        terms: &[
            t(Q(q::PREY), 1000, "prey"),
            t(Q(q::RETAKE), 1000, "retake"),
            t(Q(q::SPOILER), 1000, "spoiler"),
            t(Q(q::DUTY), 1000, "duty"),
            t(Q(q::BORDER), 1000, "border"),
        ],
    },
];

// ---- §4.3 the intent's tilt ----

/// The logits' tables a tilt term adds to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Table {
    Style,
    Aim,
    Posture,
}

/// Terms added to the carried logits (`strategy_net::Tilt`): (table, index, term). The old threat term of
/// the posture (prepare +2000, peace −1200) is removed: `net2` thinks with the threat 0.
pub const TILT: &[(Table, usize, Term)] = &[
    (Table::Style, 1, t(A(a::ONE), -1200, "wide: −600 → −1800, the tall first (`weights::style::WIDE`) folded into the bias (F1, F2)")),
    (Table::Style, 2, t(A(a::ONE), -400, "conquest: −900 → −1300, balances the prey term: conquest without prey is the 12.4 % schedule")),
    (Table::Style, 2, t(A(a::MAX_PREY), 1500, "conquest is the play only next to a disarmed, reachable neighbour")),
    (Table::Aim, 3, t(S(s::CITY_STATE_ROOM), 800, "diplomacy: the winners are tall seats with gold and allies (F2)")),
    (Table::Posture, 0, t(S(s::WAR_WEARY), 1200, "peace: tiredness is the reason to end wars (D3, F3)")),
    (Table::Posture, 1, t(A(a::MAX_MENACE), 1500, "defence: a stack at the border is defence, not war (P3, B2)")),
    (Table::Posture, 1, t(S(s::CAPITAL_FALL), 1200, "defence: after a capital falls the rest must hold (B4)")),
    (Table::Posture, 2, t(A(a::ORDER_AT_PEACE), 1800, "prepare: the army is built for a named target only")),
    (Table::Posture, 3, t(A(a::ORDER_AT_WAR), 2000, "war: a war under way with a reason is fought")),
    (Table::Posture, 2, t(A(a::NO_ORDER), -3000, "prepare: no offensive posture without a target (one brain)")),
    (Table::Posture, 3, t(A(a::NO_ORDER), -3000, "war: no offensive posture without a target (one brain)")),
];

// ---- §4.4–§4.7 the levers ----

/// The peace wish with a rival at war with us and without an order on it (§4.4).
pub const PEACE: Neuron = Neuron {
    name: "peace",
    bias: -300,
    why: "a war is not ended for nothing",
    join: UNIT,
    terms: &[
        t(S(s::WAR_WEARY), 1000, "tired: end it"),
        ts(Q(q::WAR_VALUE), Shape::Not, 600, "no reason left for this war"),
        t(R(r::RETAKE_CASE), -1500, "never while retaking"),
    ],
};

/// The guard of one own city (§4.5).
pub const GUARD: Neuron = Neuron {
    name: "guard",
    bias: -200,
    why: "a quiet city keeps today's saving",
    join: UNIT,
    terms: &[
        t(C(c::STACK_MENACE), 1200, "the answer to a stack near the city"),
        t(C(c::DANGER), 800, "the defence watch's danger"),
        t(S(s::CAPITAL_FALL), 600, "after a fall the rest must hold"),
        t(C(c::DISTRUST_NEAR), 400, "a distrusted neighbour within 8 tiles"),
        t(C(c::YOUNG), 300, "new cities are the cheap targets (MO8)"),
    ],
};

/// The tilt of the military price's multiplier, added to the carried row (§4.5).
pub const MILITARY: Neuron = Neuron {
    name: "military_tilt",
    bias: 0,
    why: "the carried row prices the posture; these terms build before the war, not after (P3)",
    join: LINEAR,
    terms: &[
        t(A(a::MAX_MENACE), 800, "a stack at the border: defenders now"),
        ts(A(a::ORDER_VALUE), Shape::Times(Y(y::POSTURE_PREPARE)), 1000, "the named target's army while preparing"),
        t(S(s::CAPITAL_FALL), 700, "after a fall: the army that holds and retakes"),
        t(S(s::AIR_NEED), 400, "fighters against the enemy air"),
        t(S(s::WAR_WEARY), -600, "a tired empire builds no more army"),
    ],
};

/// The share of the gold for city-states (§4.7).
pub const CS_SHARE: Neuron = Neuron {
    name: "cs_gold_share",
    bias: -200,
    why: "the reserve as today without a reason",
    join: UNIT,
    terms: &[
        t(Y(y::AIM_DIPLOMACY), 900, "the diplomatic aim buys the votes"),
        t(Y(y::STYLE_TALL), 600, "the tall seat's gold is its diplomatic road (F2)"),
        t(G(g::GOLD_STOCK), 400, "gold in the treasury"),
        t(G(g::CS_CONTESTED), 400, "cheap flips"),
        t(K(kc::DANGER), -800, "in danger the gold buys defenders"),
        t(F(fe::AT_WAR), -600, "at war the gold buys units"),
    ],
};

/// The thresholds of the levers: (value, why).
pub mod gate {
    use super::Milli;
    pub const WAR_START: (Milli, &str) = (550, "a war order starts on a clear reason: a target must not flicker");
    pub const WAR_KEEP: (Milli, &str) = (350, "the order is kept below its start (hysteresis) while the reason lasts");
    pub const PEACE: (Milli, &str) = (500, "the peace wish wins over the war's inertia");
    pub const DENY_HELP: (Milli, &str) = (500, "a rival more likely than not to run away gets no help (lane A T4/T1: science wins 0.311 → 0.256, no war cost)");
    pub const EMBARGO: (Milli, &str) = (600, "the embargo, the cheap brake, only on a clear runaway (W2, VP #8)");
    /// Ships escorting an `Overseas` order (`fleet::FLEET_SIZE`).
    pub const ESCORT_SHIPS: (u32, &str) = (2, "the landing needs ships that shell the coast (`fleet.rs` 1-10)");
    /// Standard rounds after a war order spent its budget without paying before another order on that rival.
    pub const SPENT_ROUNDS: (u32, &str) = (20, "the war that did not pay ends, and is not restarted at once");
}

/// The site head (§3.4): site = clamp(S1, S2 − BOUND, S2 + BOUND) + S3 − GAP·S4·(1 + S5/1000) − ARMY·S6 − TRAVEL·S7 + FILL·S8·S9/10⁶.
pub mod site {
    pub const BOUND: (i64, &str) = (169, "`bounded_net`: one citizen; outside it the net reads off its data");
    pub const GAP: (i64, &str) = (30, "`RIVAL_PENALTY`: the forward city is the one the next war takes first; up to twice next to a menacing rival");
    pub const ARMY: (i64, &str) = (60, "`ARMY_PENALTY`: an army 4–6 tiles off reaches the settler in a turn or two");
    pub const TRAVEL: (i64, &str) = (40, "`TRAVEL_COST` 30 + `NET_TRAVEL_RISK` 10");
    pub const FILL: (i64, &str) = (25, "small: a tall seat closes its own gaps (M1) before walking far");
}

/// Every neuron of `net2`'s own tables.
pub fn neurons() -> Vec<&'static Neuron> {
    let mut out: Vec<&'static Neuron> = SEAT.iter().chain(RIVAL.iter()).collect();
    out.extend([&PEACE, &GUARD, &MILITARY, &CS_SHARE]);
    out
}

/// The hand weights of `net2`'s own: every term, every bias read, the tilt, the gates and the site head.
pub fn count() -> usize {
    let mut n = 0;
    for neuron in neurons() {
        n += neuron.terms.len() + usize::from(matches!(neuron.join, Join::Sum(_)));
    }
    n + TILT.len() + 7 + 5
}
