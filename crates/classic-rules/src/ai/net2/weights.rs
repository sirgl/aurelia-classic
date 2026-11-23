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
//! | `T` | one known rival city's target inputs T1–T4 (`t`) | part A |
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
    T(usize),
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
    /// max(0, c − x).
    Below(Milli),
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
    pub const OWN_INFAMY: usize = 10;
    pub const TIES_AT_RISK: usize = 11;
    /// The space race is open to us: the parts' prerequisite projects are complete or their technologies known, and a
    /// part of the ship is still missing (0/1000).
    pub const SPACE_RACE: usize = 12;
    /// The copies of strategic resources the missing parts need beyond the free ones, 0…3 → 0…1000.
    pub const PARTS_SHORT: usize = 13;
    /// The new cities the rules' size model pays for a real tall seat, 250 a city (set after the intent; `CITY_CAP` only).
    pub const SIZE_MODEL: usize = 14;
    /// A spaceship part of ours stands outside the capital, the tile it is installed on (0/1000).
    pub const PART_OUT: usize = 15;
    pub const N: usize = 16;
    pub const NAMES: [&str; N] = ["capital_lost", "cities_lost", "war_rounds", "war_gain", "unhappy_trend", "embark", "enemy_air", "gold_stock", "cs_allies", "cs_contested", "own_infamy", "ties_at_risk", "space_race", "parts_short", "size_model", "part_out"];
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
    /// The largest `ease` of its targets (`EASE`): the core's aggregate of the T rows, before the rival concepts.
    pub const SOFT: usize = 26;
    /// An operation of ours against it forms or marches (0/1000).
    pub const OPERATING: usize = 27;
    /// Its delegates reach the World Leader's votes by themselves (0/1000).
    pub const ALONE: usize = 28;
    /// The city-states allied to it, / 3 (R31).
    pub const CS_HELD: usize = 29;
    pub const N: usize = 30;
    pub const NAMES: [&str; N] = [
        "near", "land", "sea", "soldiers", "seen_army", "open_cities", "tree", "lead", "pace", "culture", "stack", "warmonger", "betrayed", "denounced_us", "friend", "pact", "at_war",
        "war_rounds", "holds_capital", "holds_cities", "grab", "ally_hit", "local", "score", "retake_case", "captured_recent", "soft", "operating", "alone", "cs_held",
    ];
}

/// The new seat concepts (`SEAT` in order).
pub mod s {
    pub const WAR_WEARY: usize = 0;
    pub const CAPITAL_FALL: usize = 1;
    pub const CITY_STATE_ROOM: usize = 2;
    pub const AIR_NEED: usize = 3;
    pub const INFAMY_COST: usize = 4;
    pub const LATE: usize = 5;
    pub const N: usize = 6;
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
    pub const BLOCK: usize = 12;
    pub const N: usize = 13;
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
    pub const STYLE_WIDE: usize = 1;
    pub const STYLE_CONQUEST: usize = 2;
    pub const AIM_SCIENCE: usize = 3;
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

/// One known rival city's target inputs (`plan::TargetInputs` in order).
pub mod t {
    pub const SIZE: usize = 0;
    pub const GARRISON: usize = 1;
    pub const NEAR: usize = 2;
    pub const WALLS: usize = 3;
    pub const N: usize = 4;
    pub const NAMES: [&str; N] = ["size", "garrison", "near", "walls"];
}

use Src::{A, C, F, G, K, Q, R, S, T, Y};

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
    Neuron {
        name: "infamy_cost",
        bias: -100,
        why: "the offence addendum §3.1: a seat that never declared pays nothing",
        join: UNIT,
        terms: &[
            t(G(g::OWN_INFAMY), 1000, "1000 = every scripted AI that met us denounces us already (250 points, `ai/diplomacy.rs`); 400 = it distrusts us and refuses friendship, pacts and agreements"),
            t(G(g::TIES_AT_RISK), 600, "the friendships, agreements and pacts the next declaration blocks for about 30 rounds"),
        ],
    },
    Neuron {
        name: "late",
        bias: -1800,
        why: "a city taken before the last two fifths of the game pays its worth long enough",
        join: UNIT,
        terms: &[t(F(fe::PROGRESS), 3000, "0 at 60 % of the game, 1000 at 93 %: the war case prices a city by the rounds left (`city_worth`); nets-x-site opened 63 of its 68 wars before round 200 of 330 (WAR-LEVER.md §4)")],
    },
];

// ---- the target: how easy one known rival city is to take (WAR-LEVER.md §2) ----

/// The ease of one target (the T rows): the war order aims at its rival's easiest city, and `soft` (the largest)
/// is the rival's prey term.
pub const EASE: Neuron = Neuron {
    name: "ease",
    bias: -2200,
    why: "a city is no target by default: it takes a small, near city with weak defences (the old size gate, at most 8 citizens, sits at size 500); −2000 → −2300 raised the bar of a soft city (at −2000 net2 declared 1.49× nets-x-site's wars, GAP.md); R4: at −2150 the wars were 1.30× on the tuning offsets and 1.32× on the acceptance's (gate 1.3), A5's `nobody_attacked` 0.856, at −2200 0.864 with the wars 1.26×; the whole games do not separate −2150 from −2200, so R5 takes −2200 back: the wars' gate, not the A5 rate, binds",
    join: UNIT,
    terms: &[
        ts(T(t::SIZE), Shape::Not, 1000, "a small city: the learned capture chance falls 0.066 a citizen (`OPEN_CHANCE_WEIGHTS`), as much as a tile"),
        ts(T(t::GARRISON), Shape::Not, 800, "no defenders seen: the army meets only the walls"),
        t(T(t::NEAR), 1000, "a near city: the chance falls 0.067 a tile, as much as a citizen"),
        ts(T(t::WALLS), Shape::Not, 600, "the city's own strength against our army: the siege's other half"),
    ],
};

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
            t(R(r::FRIEND), 0, "read, no weight: a friendship binds nothing in these rules (no backstab memory, WAR-LEVER.md K3); at −300 a friend's army of 3+3 at our border read distrust 0 (tricks HD2 #1) and its 4–10 units two tiles from our city at peace menace 142–313 (HMO3 #1)"),
        ],
    },
    Neuron {
        name: "menace",
        bias: -300,
        why: "a few units passing are no menace",
        join: UNIT,
        terms: &[
            t(R(r::STACK), 1500, "a stack inside our borders at peace is the sneak attack (P3, MO3, B2), and at war the attack itself (capitals: read at peace only, the menace fell to 0 on the declaration)"),
            t(Q(q::DISTRUST), 400, "distrust raises it"),
            t(R(r::FRIEND), 0, "read, no weight: a friend's stack inside our borders is the same sneak attack (HMO3 #1: 4–10 units two tiles from our city, menace 142–313 with −300); a friendship binds nothing in these rules"),
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
            t(R(r::SOFT), 1200, "a soft city within reach is the prey even when the nation's army is level (WAR-LEVER.md K1: the cities the old war machine took profitably)"),
            t(K(kc::WARLIKE), 400, "a warlike leader"),
            t(R(r::GRAB), 300, "a grabber pays twice"),
            t(R(r::FRIEND), -600, "a friend's yields end with the war, but the rules add no backstab memory: a price, not a veto (WAR-LEVER.md K3)"),
            t(R(r::PACT), -1200, "never a pact partner"),
            t(K(kc::HAPPINESS_CRISIS), -700, "not in a crisis: conquered cities deepen it"),
            t(K(kc::DANGER), -500, "not under threat elsewhere"),
            t(S(s::WAR_WEARY), -600, "not when tired"),
            t(S(s::INFAMY_COST), -900, "the declaration's cost in ties (offence addendum §3.2)"),
            ts(S(s::INFAMY_COST), Shape::Times(R(r::AT_WAR)), 900, "at war with it the declaration is paid"),
            t(S(s::LATE), -1500, "a conquest late in the game does not pay (the war case's worth runs over the rounds left)"),
            t(R(r::CS_HELD), -600, "its allied city-states join a war declared on it (`minor_allies_join_war`): three allies make it a war on four (casebook, offset 1783: a spoiler war pulled in 3 nations and 5 city-states)"),
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
            t(S(s::INFAMY_COST), -900, "the declaration's cost in ties (offence addendum §3.2)"),
            ts(S(s::INFAMY_COST), Shape::Times(R(r::AT_WAR)), 900, "at war with it the declaration is paid"),
            t(R(r::CS_HELD), -600, "its allied city-states join a war declared on it (`minor_allies_join_war`), and a runaway holds many"),
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
    Neuron {
        name: "block",
        bias: -500,
        why: "no block by default: a block gives our delegates, and the leaders' bonus of a failed vote (`LEADERS_BONUS_ON_FAIL`), to another nation",
        join: UNIT,
        terms: &[t(
            R(r::ALONE),
            1500,
            "a rival with the votes of its own wins unless a tie or a higher candidate stops it; one short wins nothing, since every seat votes for itself, and the block then only elects the candidate we pooled on (GAP.md: 4 of 200 games)",
        )],
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
    (Table::Style, 2, t(A(a::ONE), -400, "conquest: −900 → −1300, balances the prey term: conquest without prey is the 12.4 % schedule. It is the cause of net2's fewer cities (R4: it moves the warlike leaders' lock to tall, and a tall seat is held to the rules' city model, `tall_real`: Rome, archipelago 1810002, 5 cities against 10 without it; founded 5.92 against nets-x-site's 6.34, 6.39 against 6.25 at 0), but at 0 the share fell 53.8 → 48.5 % on offsets 1810–1811 (nets-x-site's diplomatic victories 11 → 20, ours 21 → 14): kept; R5 made the tall seat's cap the net's (`CITY_CAP`), and founding more on the tall seats lost the science road (R5 tuning: 9.0 cities, 35.9 %), so the cap reads the conquest share only by half its sites")),
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
        t(C(c::DANGER), 0, "read, no weight: the watch's saving already prices its own danger, and 800 multiplied it by 1.6 on 43 % of the cities in the first 70 rounds (4 % more army, 5 % less production at round 66, GAP.md); the guard adds what the watch does not see"),
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

/// The copies of the strategic resources the missing spaceship parts need that no other unit may take (`Net2Plan::
/// space_hold`: the level × the copies the missing parts need). Why (capital-fix FIX.md Part 2, Portugal, fractal
/// 1783007 P3): 12 aluminium, 5 in installed parts and 6 in helicopter gunships, the last part could not be queued
/// from r314 to r330 and the seat that knew every technology won by time.
pub const PART_HOLD: Neuron = Neuron {
    name: "part_hold",
    bias: -500,
    why: "no hold outside the space race: aluminium is the modern army's",
    join: UNIT,
    terms: &[
        t(G(g::SPACE_RACE), 1500, "in the race every copy a missing part needs is the ship's: a unit can be built later, the last part cannot without its copy"),
        t(G(g::PARTS_SHORT), 300, "the copies are short already: none of them goes to a unit"),
        t(K(kc::DANGER), -600, "a city in danger may spend some of them on its defenders"),
    ],
};

/// The research on the spaceship's way (`Net2Plan::space_research`). Why (R5 traces, two late science victories):
/// Ottomans, continents 1810002 (won r~306), and England, pangaea 1810001 (won r~330), both researched after robotics
/// four technologies outside the ship's closure (nuclear fusion, globalization, the internet, lasers: 34 100 of the
/// rules' beakers, 17–20 rounds) before mobile tactics and particle physics, the engine's: the review names no
/// spaceship goal (`goal 0` from robotics on) and every one of them scored the floor (`own 5`), so the lowest id won.
/// 75 of the 81 technologies are on the ship's way: the lever changes the order of the last few only.
pub const SPACE_RESEARCH: Neuron = Neuron {
    name: "space_research",
    bias: -1000,
    why: "no seat turns its research to the ship by default: globalization's delegates and the internet's tourism are other roads",
    join: UNIT,
    terms: &[
        t(Y(y::AIM_SCIENCE), 2000, "the science aim: the ship is the victory it plays for (0.75 alone opens it; England 1810001 held 0.95 at robotics)"),
        t(F(fe::SPACE_OWN), 1000, "the way to the ship already gone (its technologies' beakers known, its parts built; 0.6 at robotics): with aim 0.4 it opens at 0.7, a diplomatic seat (aim 0.2) never"),
    ],
};

/// The capital is kept for the spaceship's parts (`Net2Plan::capital_gate`): no own civilian waits in it while a part
/// is outside, and the civilians standing in it walk out around the units in the way (the executors
/// `planner_workers::park` and `victory::walked_orders` only read the lever). Why (space-parts SPACE.md, P1 offsets
/// 2200–2201 replayed with `AURELIA_SPACE_TRACE`): a part is a civilian and a tile holds one civilian; a jobless
/// worker parked in the capital (`park`: the nearest city with no civilian), the space plan sent it out, and the
/// worker's own turn parked it back in the same turn (Korea, pangaea 2201008: the engine 2 tiles off from r317 to the
/// time victory at r330; Celts, continents 2201005: 18 rounds). Or the worker's march out was stopped by the waiting
/// parts themselves (the rules' march does not go around units: Denmark, continents 2200002, a booster and the stasis
/// chamber one tile off from r305 until a diplomatic victory at r312). 9 of the 29 ships built and not won stood still
/// so (34 rounds), and 29 of the 140 ships that won (57 rounds). The executor walks the civilians off the tiles next
/// to the capital as well (its gate): a worker sent to the nearest free tile parked at the gate and shut a part out
/// as before (tuning offsets 2620–2621, Brazil pangaea 2620001, Morocco pangaea 2621017).
pub const CAPITAL_GATE: Neuron = Neuron {
    name: "capital_gate",
    bias: -500,
    why: "no part outside the capital, no gate: a city is where a jobless civilian is safe from capture",
    join: UNIT,
    terms: &[t(G(g::PART_OUT), 1500, "a part enters only a capital with no civilian in it, and the ship is the victory: the capital is the part's tile")],
};

/// The new cities a real tall seat founds (`Intent::more_cities` of a `tall_real` seat, thousandths of a city, rounded
/// half up; the executors `tall_real::adjust` and `Prices::city_cap` only read the target). The rules' size model
/// (`tall_real::SizeModel::more`) is one input among the land, the leader's tilt, the rivals and the mood. R4's case:
/// a warlike leader held to tall by the conquest bias kept 2 cities from r55 to r130 (Rome, archipelago 1810002; 5 at
/// the end against 10 under conquest, score 863 against 1014). Tuning rounds on offsets 1810–1811 (R4 there: 53.5 %):
/// without the model (the sites, a floor of four, the count, the happiness below zero) 9.0 cities a seat against
/// nets-x-site's 6.3, 35.9 %, science victories 23 / 59, risings 25 / 0; the model with the conquest and wide shares at
/// 6000 a site 7.6 cities, 45.2 %; the model alone 5.8 cities, 49.7 %, real victories 95 / 97; this form 5.9 cities,
/// 52.0 %, real 113 / 103, science 77 / 70. The model's happiness floor (6 points left after the city) is what keeps a
/// tall empire growing: it stays, and the net adds only the founding of a warlike seat's conquest share.
pub const CITY_CAP: Neuron = Neuron {
    name: "city_cap",
    bias: 0,
    why: "the model's count unless a reason moves it",
    join: LINEAR,
    terms: &[
        t(G(g::SIZE_MODEL), 4000, "the rules' size model: the new cities whose beakers pay the research and policy cost they add, the empire content after them, a city a city (R5 first form without it: 9.0 cities, science victories 23 / 59)"),
        ts(Y(y::STYLE_CONQUEST), Shape::Times(F(fe::ROOM)), 3000, "the leader's tilt: a warlike leader moved to tall by the conquest bias founds half its conquest share of the free sites (6 → 1000) on top (Rome 1810002, r55: 0.40 of 4 sites, 0.8 more); a pure tall seat (0.05) stays at the model; at 6000 with the wide share's 6000 (R5 tuning) 7.6 cities, 45.2 %"),
        ts(Y(y::STYLE_WIDE), Shape::Times(F(fe::ROOM)), 0, "read, no weight: the wide share is not the tilt's (the bias moved conquest, not wide); at 6000 it founded on most tall seats (R5 tuning: 7.6 cities, 45.2 %)"),
        ts(F(fe::CITIES), Shape::Above(375), -4000, "the count now: beyond three cities (8 → 1000) half a city less a city held: the tilt's founding is a city or two, not the wide play"),
        ts(F(fe::HAPPY), Shape::Below(500), -8000, "the happiness headroom: an unhappy empire founds none, a city less per 2.5 points below zero (a city and its citizen cost about 3.4)"),
        t(K(kc::HAPPINESS_CRISIS), -6000, "in a crisis the rules cut growth: no city (`CITY_CRISIS`)"),
        t(A(a::MAX_MENACE), -2000, "rivals' pressure: a stack at our border takes the settler and the young city first (MO8)"),
        ts(F(fe::PROGRESS), Shape::Above(600), -5000, "late a new city has no rounds to grow its citizens: the model's horizon refused every city in 1326 of 3178 turns of r198–263 (R4 traces); the tilt's founding stops too (−1.6 cities at 93 % of the game)"),
    ],
};

// ---- the offence addendum (work/new-net-offence-addendum-2026-11-22.md §3): declared, not yet built ----

/// The offence is off: `net2` plays as the addendum's control `net2-defence` (every `Offence` field and
/// the `WarOrder` aim, opening and raid at their defaults). The offence's levers read this switch.
pub const OFFENCE: bool = false;

const fn declared(name: &'static str, why: &'static str) -> Neuron {
    Neuron { name, bias: 0, why, join: UNIT, terms: &[] }
}

/// The offence's seat concepts (addendum §3.1) not built yet: no terms, each fires 0; not in `neurons()` (`infamy_cost`
/// is built: `SEAT`).
pub const OFFENCE_SEAT: [Neuron; 2] = [
    declared("isolation", "unit(1000 − 1200·max reach − 600·max menace − 400·max(runaway·reach)): the minimal army is safe"),
    declared("sweep", "unit(−600 + 1200·G14 + 600·Y(aim_diplomacy) + 400·G8): the city-state sweep before a session"),
];

/// The offence's rival (and pair) concepts (addendum §3.2): no terms yet, each fires 0; not in `neurons()`.
pub const OFFENCE_RIVAL: [Neuron; 5] = [
    declared("brake", "unit(−200 + 1200·runaway − 1000·spoiler): a runaway we cannot beat ourselves"),
    declared("hire", "per pair h, t: unit(−900 + 1200·brake_t + 800·[order on t] + 600·P1 + 600·(P2 − 500) − 1200·runaway_h − 800·P4 − 1000·P3 − 500·(1000 − G8))"),
    declared("milk", "unit(−1200 + 1000·[order on r, at peace, posture prepare] + 800·G13)"),
    declared("stage", "unit(−700 + 1000·[order on r, Prey/Border, at peace] + 400·R1 − 600·menace − 400·distrust)"),
    declared("citadel", "unit(−600 + 700·T6(aim) + 800·R28 + 400·[order on r and at war]); 0 at peace"),
];

/// The thresholds of the levers: (value, why).
pub mod gate {
    use super::Milli;
    pub const WAR_START: (Milli, &str) = (550, "a war order starts on a clear reason: a target must not flicker");
    pub const WAR_KEEP: (Milli, &str) = (350, "the order is kept below its start (hysteresis) while the reason lasts");
    pub const PEACE: (Milli, &str) = (500, "the peace wish wins over the war's inertia");
    pub const DENY_HELP: (Milli, &str) = (500, "a rival more likely than not to run away gets no help (lane A T4/T1: science wins 0.311 → 0.256, no war cost)");
    pub const EMBARGO: (Milli, &str) = (600, "the embargo, the cheap brake, only on a clear runaway (W2, VP #8)");
    pub const BLOCK: (Milli, &str) = (500, "the ballot goes against a rival only when it would win the vote");
    pub const SPACE_RESEARCH: (Milli, &str) = (500, "the research keeps to the ship's way when the science aim and the way gone say so more likely than not");
    pub const CAPITAL_GATE: (Milli, &str) = (500, "the capital is kept for the parts when a part outside it says so more likely than not");
    /// Ships escorting an `Overseas` order (`fleet::FLEET_SIZE`).
    pub const ESCORT_SHIPS: (u32, &str) = (2, "the landing needs ships that shell the coast (`fleet.rs` 1-10)");
    /// Standard rounds after a war order spent its budget without paying before another order on that rival.
    pub const SPENT_ROUNDS: (u32, &str) = (20, "the war that did not pay ends, and is not restarted at once");
    /// Standard rounds a war order may wait for its army at peace before it is spent.
    pub const PREPARE_ROUNDS: (u32, &str) = (30, "the war machine's `FORM_LIMIT`: an army that has not marched by then does not");
}

/// The site head (§3.4): site = clamp(S1, S2 − BOUND, S2 + BOUND) + S3 − GAP·S4·(1 + S5/1000) − ARMY·S6 − TRAVEL·S7 + FILL·S8·S9/10⁶.
pub mod site {
    pub const BOUND: (i64, &str) = (169, "`bounded_net`: one citizen; outside it the net reads off its data");
    pub const GAP: (i64, &str) = (30, "`RIVAL_PENALTY`: the forward city is the one the next war takes first; up to twice next to a menacing rival");
    pub const ARMY: (i64, &str) = (60, "`ARMY_PENALTY`: an army 4–6 tiles off reaches the settler in a turn or two");
    pub const TRAVEL: (i64, &str) = (40, "`TRAVEL_COST` 30 + `NET_TRAVEL_RISK` 10");
    pub const FILL: (i64, &str) = (0, "read, no weight: a gap inside our land stays ours, the outward site is the one a rival takes; at 25 the term chose a gap over a better outward site in 242 of 5633 settler decisions, mostly at 2-5 cities, and without the head net2 took +4.1, +4.2 and +2.1 points (GAP.md)");
}

/// Every neuron of `net2`'s own tables.
pub fn neurons() -> Vec<&'static Neuron> {
    let mut out: Vec<&'static Neuron> = SEAT.iter().chain(RIVAL.iter()).collect();
    out.extend([&PEACE, &GUARD, &MILITARY, &CS_SHARE, &EASE, &PART_HOLD, &CITY_CAP, &SPACE_RESEARCH, &CAPITAL_GATE]);
    out
}

/// The hand weights of `net2`'s own: every term, every bias read, the tilt, the gates and the site head.
pub fn count() -> usize {
    let mut n = 0;
    for neuron in neurons() {
        n += neuron.terms.len() + usize::from(matches!(neuron.join, Join::Sum(_)));
    }
    n + TILT.len() + 11 + 5
}
