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
    /// The smallest term, linear (the bias is not read): the first of several limits.
    Min,
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
    /// The opening's cities a real tall seat still lacks beyond the size model's new ones, 250 a city (`OPENING`;
    /// `early-growth` only, else 0; set after the intent).
    pub const OPENING: usize = 16;
    /// The opening's cities a real tall seat still lacks, the model's new ones among them, 250 a city (`OPENING_ROOM`;
    /// `early-growth` only, else 0; set after the intent).
    pub const OPENING_GAP: usize = 17;
    /// Luxury kinds the empire lacks lying on its own tiles without their improvement, beyond its workers (alive or
    /// queued), 500 a kind (`net2::growth::lux_open`; `early-growth` only, else 0).
    pub const LUX_OPEN: usize = 18;
    pub const N: usize = 19;
    pub const NAMES: [&str; N] = ["capital_lost", "cities_lost", "war_rounds", "war_gain", "unhappy_trend", "embark", "enemy_air", "gold_stock", "cs_allies", "cs_contested", "own_infamy", "ties_at_risk", "space_race", "parts_short", "size_model", "part_out", "opening", "opening_gap", "lux_open"];
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
    /// `science-flow`: the rounds our gold has waited for its research agreement unsigned, over
    /// `gate::CHEST_WAIT_ROUNDS` (`science::read_waits`; 0 for every other variant).
    pub const CHEST_WAIT: usize = 30;
    pub const N: usize = 31;
    pub const NAMES: [&str; N] = [
        "near", "land", "sea", "soldiers", "seen_army", "open_cities", "tree", "lead", "pace", "culture", "stack", "warmonger", "betrayed", "denounced_us", "friend", "pact", "at_war",
        "war_rounds", "holds_capital", "holds_cities", "grab", "ally_hit", "local", "score", "retake_case", "captured_recent", "soft", "operating", "alone", "cs_held",
        "chest_wait",
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
    /// `layout-defence`: the city holds our palace (0/1000; `hold::rows`, 0 for every other variant).
    pub const CAPITAL: usize = 4;
    /// `layout-defence`: the largest `FRONT` of a rival on the city (`hold::rows`, 0 for every other variant).
    pub const FRONT: usize = 5;
    /// `layout-defence`: the watch's threat at the city over the threat and its planned guard (`defence::Danger`;
    /// 500: level, 0 without a danger; `hold::rows`, 0 for every other variant).
    pub const ALARM: usize = 6;
    pub const N: usize = 7;
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
        t(G(g::OPENING), 4000, "the opening's cities beyond the model's, a city a city (`early-growth`, `OPENING`): the model keeps 6 points of happiness after a city (`tall_real::CONTENT_FLOOR`), which held science-flow's seats at 2 cities in 4909 of 6650 turns of r1–100 (look: 2.57 cities at r80, the 3rd city at r80 and the 4th at r108 for the science winners)"),
    ],
};

// ---- the science flow (work/nets/science-flow/SCIENCE-FLOW.md): played by `science-flow` only (`TurnBrief::science_flow`) ----

/// The share of its science specialists' slots a seat fills (`Science::scientists`; the executor
/// `citizens::science_specialist_commands` keeps each city's food at `gate::SCIENTIST_FOOD`). Why (the look, 80 self-play
/// games of `standard`, Quick): the science winners entered the Modern era with 2 scientists in 11 slots and 25 food a
/// turn to spare, the Atomic era with 9 in 16; small-continents 1 p4 went from 185 to 376 beakers when its cities took 9
/// scientists (r170→r180) and back to 341 when they left them for food tiles (r210, food +100). A scientist is 3 beakers,
/// 2 more with Secularism (taken by every seat at r~160), times the city's science percents, and 3 points of a great
/// scientist; the rules' default weights rank it level with a 3-food tile.
pub const SCIENTISTS: Neuron = Neuron {
    name: "scientists",
    bias: -900,
    why: "early the citizens stay on the tiles: a growing citizen is a beaker and more (ai-growth.md); the level only adds slots to what the rules' weights fill themselves",
    join: UNIT,
    terms: &[
        ts(F(fe::PROGRESS), Shape::Above(300), 2500, "the later the game, the fewer rounds a new citizen pays back its food (0.6 of the game: +750, 0.75: +1125); a human fills the slots of the universities and schools late whatever its aim"),
        t(Y(y::AIM_SCIENCE), 600, "the science aim: the ship is beakers, and a filled slot is beakers now and great scientists later (sf1: the aim's share is 0.35 at the median mid-game, so the aim tilts and does not gate)"),
        t(F(fe::AT_WAR), -500, "at war the cities work the hammers"),
    ],
};

/// A research agreement with one rival pays our race (`Science::ties`): the friendship and the agreement with it are
/// proposed and taken though it runs away (`Net2Plan::denies_item`), the ladder to it is offered at the level's cadence
/// (`science::cadence`: every round at 1000, the diplomacy's own `PROPOSAL_INTERVAL` at the gate), and the gold it costs is
/// kept (`Science::chest`) until the partner lets it wait too long (`r::CHEST_WAIT`). Why (the look): at the Modern
/// era a seat met 3.45 majors and had 0.38 agreements; of its 1.55 friends 1.17 had none: 0.36 our gold short (the allocator spent it; treasury 425 and +121 a turn at the Modern era),
/// 0.26 theirs, 0.58 nothing in the rules; and the runaway, whom `net2` denies every help (`gate::DENY_HELP`), is the
/// partner with the most science (`net2-no-deals`, the denial off: 0.75 agreements at the Modern era, 1.06 at r300
/// against 0.41, 42 science victories in 80 self-play games against 29). The rules pay min(both sciences over the deal)
/// / 3 × 50 % at its end: with a partner as fast as us, a sixth of our science a turn for ~280 gold at Quick, 4 beakers
/// a gold, where a bought building is about one. Equal beakers to both shorten a faster rival's lead in rounds: for a
/// seat that races the runaway it is a good partner; for one that does not race it, the beakers are its victory.
pub const AGREEMENT: Neuron = Neuron {
    name: "agreement",
    bias: 600,
    why: "an agreement is 4 beakers a gold: a seat at peace signs one with every friend, as a human does",
    join: UNIT,
    terms: &[
        t(Y(y::AIM_SCIENCE), 400, "the science aim: the beakers are the victory it plays for"),
        ts(R(r::PACE), Shape::Below(500), -1200, "a slower partner pays a smaller pool (the smaller science): at a quarter less −300"),
        t(Q(q::DISTRUST), -1000, "a distrusted rival breaks the friendship or comes with an army (D12)"),
        t(Q(q::MENACE), -600, "its army at our border: a friendship feeds the sneak attack"),
        t(R(r::AT_WAR), -2000, "no deal in a war"),
        t(Q(q::RUNAWAY), -800, "the runaway wins with the beakers: a seat that does not race it gives it none (`gate::DENY_HELP`)"),
        ts(Q(q::RUNAWAY), Shape::Times(Y(y::AIM_SCIENCE)), 800, "a seat that races it gains as many beakers and shortens its lead in rounds: the denial lifts with the aim"),
        t(R(r::CHEST_WAIT), -600, "a partner that let our gold wait this long does not sign (0.26 of the friends without an agreement lacked their own gold, which a few rounds of income bring): a level friend (740) frees the chest after 8 rounds, about three offers at its cadence; a sure tie (1000) after 17"),
    ],
};

// ---- the early growth (work/nets/early-growth): played by `early-growth` only (`TurnBrief::early_growth`) ----

/// The tilt of the expansion price's multiplier while the opening lacks cities (`price_tilt[pd::EXPANSION]`; the
/// strategist's production head multiplies a settler's value by it). Why (look 3610–3614, the opening's cap on): the
/// allocator picked the settler and the head replaced it by a granary (52 times by r60), a library (39), stone works
/// (10); a tall seat's expansion price is ×0.45–0.72 (the tall row's −700, the defence posture), so a city the cap
/// allowed at 2 cities waited 32 rounds (median) for its settler, at 3 cities 40.
pub const EXPANSION_TILT: Neuron = Neuron {
    name: "expansion_tilt",
    bias: 0,
    why: "no tilt without an opening city to found: the tall row prices a city beyond the plan's count",
    join: LINEAR,
    terms: &[t(G(g::OPENING_GAP), 6000, "an opening city to found lifts the price by 1.5 (×0.5 → ×2.0): the settler the allocator ranks first is the head's pick before a granary or a library, which a 2-hammer city builds anyway and the new city builds too")],
};

/// The tilt of the worker price's multiplier while a luxury we lack lies unimproved on our tiles
/// (`price_tilt[pd::WORKER]`). Why (look 3610–3614): the first worker came at r46 (median; quartiles 40–51) while the
/// seats held 1.9 luxury kinds unconnected inside their borders at r40; a luxury kind is 4 points of happiness here
/// (7.8 points of 2.0 kinds at r80), a city and its citizen cost about 3.3, so the 4th city waited on the happiness
/// (3413 turns at 3 cities with no city allowed, excess 1.3); the head replaced the allocator's worker by a granary 21
/// times, a lighthouse 16, stone works 14, the Great Library 11 by r60.
pub const WORKER_TILT: Neuron = Neuron {
    name: "worker_tilt",
    bias: 0,
    why: "no tilt without a luxury waiting for its worker: the allocator's worker value counts the worked tiles",
    join: LINEAR,
    terms: &[t(G(g::LUX_OPEN), 4000, "a luxury kind waiting lifts the price by 2 (×1 → ×3): its connection is a city's happiness, which the worked-tile value does not see")],
};

/// The opening's row of the seat (`net2::growth::opening_row`), read by `OPENING_ROOM` and `OPENING` through `Src::C`.
pub mod go {
    /// Own cities, 250 a city.
    pub const CITIES: usize = 0;
    /// Free sites (`fe::ROOM` × `strategy_net::features::ROOM_SITES`), 250 a site.
    pub const SITES: usize = 1;
    /// The cities the excess happiness pays for, each with its first citizen at the empire's rates now
    /// (`value::Unhappiness::of`), 250 a city.
    pub const CONTENT: usize = 2;
    /// The seat plays the real tall style and `net2`'s strategy part (0/1000).
    pub const TALL: usize = 3;
    /// The size model's new cities (`Intent::more_cities`), 250 a city.
    pub const MODEL: usize = 4;
    /// `OPENING_ROOM`'s output (set after it fires; read by `OPENING`).
    pub const ROOM: usize = 5;
    pub const N: usize = 6;
}

/// The opening's cities a real tall seat may still found, 250 a city (`g::OPENING_GAP`, `OPENING`'s input): the first
/// of its limits.
pub const OPENING_ROOM: Neuron = Neuron {
    name: "opening_room",
    bias: 0,
    why: "the founding stops at the first of its limits",
    join: Join::Min,
    terms: &[
        ts(C(go::CITIES), Shape::Below(1000), 1000, "up to four cities: science-flow's seats at 3–4 cities at r80 made 82–83 beakers a turn at r132 against 72 at 2 cities and won by science in 10–12 % of the games against 8 % (230 seats against 233; 5 cities: 5 seats, no reading; runs/derive-e1-sf.md)"),
        t(C(go::SITES), 1000, "a city a free site: the opening founds no city the land has no site for"),
        t(C(go::CONTENT), 1000, "a city the happiness pays for (excess ≥ 0 after it and its first citizen; below 0 the rules slow every city's growth): the size model's floor of 6 points held science-flow's seats at 2 cities in 4909 of 6650 turns of r1–100 (look)"),
        t(C(go::TALL), 1000, "a real tall seat with the strategy part only: 1000 never binds, 0 is no opening (the other seats' count is the strategist's)"),
    ],
};

/// The opening's cities beyond the size model's new ones, 250 a city, up to four (`g::OPENING`, read by `CITY_CAP`).
pub const OPENING: Neuron = Neuron {
    name: "opening",
    bias: 0,
    why: "no city beyond the model without the opening's room",
    join: UNIT,
    terms: &[t(C(go::ROOM), 1000, "the opening's room"), t(C(go::MODEL), -1000, "less the model's own new cities: `CITY_CAP` reads them at `g::SIZE_MODEL`")],
};

/// The early growth's garrison row of one own city (`net2::growth::garrison_row`), read by `GARRISON` through `Src::C`
/// (its own frame: `GUARD`'s `c` row is not read here). A new reason to keep the garrison is a new slot and a term.
pub mod gc {
    /// The city's production is below the mean of the empire's cities (0/1000).
    pub const LOW_PRODUCTION: usize = 0;
    /// A barbarian camp the player knows within `camps::CAMP_REACH` of the city (0/1000).
    pub const CAMP_NEAR: usize = 1;
    /// `c::STACK_MENACE`: the menace of the owner of the strongest stack near the city.
    pub const STACK_MENACE: usize = 2;
    /// `c::DANGER`: the defence watch sees the city in danger (0/1000).
    pub const DANGER: usize = 3;
    /// `c::DISTRUST_NEAR`: the largest distrust of the rivals with a city within 8 tiles.
    pub const DISTRUST_NEAR: usize = 4;
    /// At war with a major (0/1000).
    pub const AT_WAR: usize = 5;
    /// The city is the capital (0/1000).
    pub const CAPITAL: usize = 6;
    pub const N: usize = 7;
}

/// The early growth's purchase row of the seat (`net2::growth::buy_row`), read by `BUY_CHEST` through `Src::C`.
pub mod gb {
    /// A head the chest may buy (`CHEST_ITEM`) heads a queue with `gate::CHEST_TURNS` or more turns left (0/1000).
    pub const WAITING: usize = 0;
    /// At war with a major (0/1000).
    pub const AT_WAR: usize = 1;
    pub const N: usize = 2;
}

/// A queue head's row (`net2::growth::item_row`), read by `CHEST_ITEM` through `Src::C`.
pub mod gi {
    /// A builder of the land (0/1000): the jobs ahead and the luxuries waiting are land tiles.
    pub const WORKER: usize = 0;
    /// A settler (0/1000).
    pub const SETTLER: usize = 1;
    /// A building, not a wonder, with an effect that yields science (0/1000).
    pub const SCIENCE: usize = 2;
    pub const N: usize = 3;
}

/// The seat's worker row (`net2::growth::jobs_ahead`), read by `WORKER_SHORT` through `Src::C`.
pub mod gw {
    /// Own cities, 1000 a city.
    pub const CITIES: usize = 0;
    /// Builders of the land alive or queued, 1000 a worker.
    pub const WORKERS: usize = 1;
    pub const N: usize = 2;
}

/// A city's jobs row (`net2::growth::jobs_ahead`), read by `JOBS_CAP` through `Src::C`.
pub mod gj {
    /// The city's citizens, 1000 a citizen.
    pub const CITIZENS: usize = 0;
    pub const N: usize = 1;
}

/// Whether a city builds its flat garrison (`planner_cities::garrison_order`: one military unit in every city from
/// the first contact), 0…1000, at least `gate::GARRISON` to build it (`Growth::garrison_waits` below it). Why
/// (science-flow's self-play, seeds 3700–3716, runs/derive-e1-sf.md): the young non-capital cities after Writing and
/// before their library (r ≤ 135) were below the empire's mean production in 44793 of 46672 rounds; there a military
/// unit or a scout headed the queue in 49.6 % of the rounds, 70.3 % of it ordered by the flat garrison, at 3 hammers
/// (a spearman is 12 rounds); the library came 34 rounds after the founding or Writing and 18 % had none by r135
/// (look). The defence watch, which answers a seen threat (507 of 554 of its orders were against barbarians, look),
/// stays as it is.
pub const GARRISON: Neuron = Neuron {
    name: "garrison",
    bias: 1000,
    why: "a city keeps today's garrison: one unit from the first contact",
    join: UNIT,
    terms: &[
        t(C(gc::LOW_PRODUCTION), -1000, "a young city below the empire's mean production builds its infrastructure first: half its rounds went to a military head, 70 % of it the flat garrison; at or above the mean 7.7 % (1879 rounds)"),
        t(C(gc::CAMP_NEAR), 1000, "a camp within reach spawns raiders every few rounds (`camps::LOSS_PER_CAMP_ROUND`): the garrison stays"),
        t(C(gc::STACK_MENACE), 1000, "a rival's stack near the city"),
        t(C(gc::DANGER), 1000, "the watch sees a threat: the city is not left empty"),
        t(C(gc::DISTRUST_NEAR), 700, "a distrusted neighbour within 8 tiles (above 715 distrust the garrison stays)"),
        t(C(gc::AT_WAR), 1000, "at war with a major every city keeps its garrison"),
        t(C(gc::CAPITAL), 1000, "the capital keeps its garrison: with the wait on every city early-growth lost 4 capitals against science-flow's 3 in E3 (offsets 3722–3723, 400 games); a capital was below the mean in 696 of its 64800 rounds (r ≤ 135), the only rounds the wait reached it"),
        t(S(s::CAPITAL_FALL), 1000, "after a fall the rest must hold"),
    ],
};

/// Whether a queue head may spend the growth chest (`Growth::chest_heads`), 0…1000, at least `gate::CHEST_ITEM`. A
/// wonder has no gold price under the rules (Unciv's `hurryCostModifier` −1: not for sale), so it never meets the chest.
pub const CHEST_ITEM: Neuron = Neuron {
    name: "chest_item",
    bias: 0,
    why: "no other head spends the chest: the tiles' and the other heads' gold stays theirs",
    join: UNIT,
    terms: &[
        t(C(gi::WORKER), 1000, "the first worker came at r46 (median; quartiles 40–51) while 1.9 luxury kinds lay unconnected at r40 (look)"),
        t(C(gi::SETTLER), 1000, "a city the cap allowed waited 32 rounds (median) for its settler at 2 cities, 40 at 3 (look)"),
        t(C(gi::SCIENCE), 1000, "a library's hurry offer was worth more a gold than a tile's (ratio median 1853 against 1195), but its price (300) was above the spendable gold in 3706 of 4591 offers (look)"),
    ],
};

/// The share of the hurry price of the cheapest chest head (`CHEST_ITEM`) that the tiles and the gifts leave in the
/// treasury (`Growth::chest`), 0…1000. Why (look 3610–3614 with the gold trace): by r135 the seats bought 2533 tiles
/// (21 a seat, 54 gold each) and 4 libraries; the tiles took the gold (spendable median 40 above a reserve of 105–130)
/// every turn before it reached a library (300), a worker (220) or a settler.
pub const BUY_CHEST: Neuron = Neuron {
    name: "buy_chest",
    bias: 0,
    why: "no chest without a growth item waiting: the tiles keep the gold",
    join: UNIT,
    terms: &[
        t(C(gb::WAITING), 1000, "a library, a worker or a settler at a queue's head: the gold waits for it, the tiles after"),
        t(C(gb::AT_WAR), -1000, "at war the gold is the defenders' (`defence::affordable_now`)"),
        t(S(s::CAPITAL_FALL), -1000, "after a fall the gold holds the rest"),
    ],
};

/// The empire lacks builders of the land (0…1000; at least `gate::WORKER_SHORT` the allocator's worker counts the jobs
/// ahead, `Growth::jobs_ahead`). Why (look 3610–3611): the worked tiles alone held the first worker out of the
/// allocator's options, 0 jobs in 34 of 74 decisions before it.
pub const WORKER_SHORT: Neuron = Neuron {
    name: "worker_short",
    bias: 0,
    why: "no city without its worker, no jobs ahead",
    join: UNIT,
    terms: &[
        t(C(gw::CITIES), 1000, "a worker a city: with the jobs ahead counted for any number of workers the workers a city at r120 went from 0.95 to 1.41 and 23 % of them stood idle (look, round 2)"),
        t(C(gw::WORKERS), -1000, "a worker alive or queued covers a city's jobs"),
    ],
};

/// The tiles of a city whose jobs the workers count ahead, thousandths of a tile (`Growth::jobs_ahead`: the reading
/// counts the city's owned unimproved land tiles with a job up to it).
pub const JOBS_CAP: Neuron = Neuron {
    name: "jobs_cap",
    bias: 2000,
    why: "two citizens ahead: a young city grows from 1 to 3 citizens in 12 rounds (median of 1199 cities) and a seat's first worker finishes its second improvement 14 rounds after its birth (median of 480 seats; runs/derive-e1-sf.md): the jobs it starts now are done when the city works those tiles",
    join: LINEAR,
    terms: &[t(C(gj::CITIZENS), 1000, "a tile a citizen: the tiles the city works now")],
};

// ---- the hold of the cities (work/nets/layout-defence): played by `layout-defence` only (`TurnBrief::layout_defence`) ----

/// The front one rival makes on one own city before its army is in sight (`hold::rows`): the rival's inputs with R1
/// `near` read from this city (its nearest known city to this one, 1000 at 5 tiles, 0 at 15) and its concepts. Why
/// (the look: 11 capitals of `standard` lost at Standard speed, offsets 3340–3361, and 3 more on 4260–4261): the defence
/// answers the enemy it sees within 5 tiles, and the stacks of 10–19 units came into that ring 3–8 rounds before the
/// fall, most of them 5–12 rounds after the declaration; the guard then held 0.6–1.0 of the threat and lost it in the
/// siege (pangaea 3342019 p5: guard 91 against 92 at r151, 57 against 66 at r156, the capital lost at r157). What made
/// those wars was on the map long before: a land neighbour, near, with an army level with ours or above.
pub const FRONT: Neuron = Neuron {
    name: "front",
    bias: -600,
    why: "a neighbour at peace with an army below ours is no front: its stack needs rounds of building we would see",
    join: UNIT,
    terms: &[
        ts(R(r::NEAR), Shape::Times(R(r::LAND)), 1000, "a rival's city near this one by land: its army walks 10 tiles in 5 rounds, the warning the defence had (3342019 p0: near 500 by land at r140, the war at r150)"),
        ts(R(r::NEAR), Shape::Times(R(r::SEA)), 300, "over water a landing takes the turns and the ships the rival must first build: a third of the land road"),
        ts(R(r::SOLDIERS), Shape::Above(400), 1500, "its soldiers estimate above 0.4 of both armies: level (0.5) adds 150, 0.7 adds 450 (in the last round of the 11 traced falls the threat at the walls was 1.0–3.9 times the guard)"),
        t(Q(q::DISTRUST), 500, "a distrusted neighbour is the one that declares"),
        t(Q(q::MENACE), 600, "its stack at our border at peace is the war before the declaration (3342019: stack 222 inside our borders 10 rounds before it)"),
        t(R(r::AT_WAR), 800, "at war a near land rival is the front itself: every traced capital fell to a declared war"),
        t(R(r::PACT), -1200, "a pact partner's army is ours to call, not a front"),
    ],
};

/// The hold of one own city (`Net2Plan::hold`): how many holders it keeps beyond the standing garrison
/// (`gate::HOLD_ONE`, `HOLD_TWO`, `HOLD_THREE`). The executors read the count only: the city's garrison
/// (`planner_units::garrison_wanted_at`), the defenders the city switches its production to while it is short of them
/// and the watch reads a danger there, and the gold kept from the other purchases for buying one (`Hold::chest`). Why
/// (the look: 17 capitals of `standard` lost at Standard speed, offsets 4260–4264, and 10 more on 4100–4102): the guard
/// within 3 tiles was weaker than the stack in 17 of 17 (median 0.28 of it), one unit stood in the city in 14 of 17;
/// the watch read the danger at the capital 10 rounds or more before the fall in 16 of 17, net2's guard level at its
/// top, and nothing answered it: 100 gold or more left in 12 of 17, nothing bought in the last 10 rounds in 10 of 17,
/// tiles bought in 5 (pangaea 4261012 p0: 376 gold on 4 tiles and one unit while the capital's guard of 41 met 117).
/// The defence's own price (`defence::defender_net`) pays no defender once the stack is three times the guard: the
/// count must be set while the danger is young, from the city's place and the alarm, not from the odds at the walls.
pub const HOLD: Neuron = Neuron {
    name: "hold",
    bias: -300,
    why: "an interior city at peace keeps the standing garrison",
    join: UNIT,
    terms: &[
        t(C(c::CAPITAL), 400, "the capital is the stake: its loss is the city and the game's share of it (`defence::capital_share`); the traced seats kept 0–3 cities after it"),
        t(C(c::FRONT), 600, "a city on a front holds more, but alone a front makes no holder outside the capital: the alarm does (`DANGER`)"),
        ts(C(c::CAPITAL), Shape::Times(C(c::FRONT)), 200, "the capital on a front first: at a front of 0.55 (a land neighbour at peace with a level army 5–6 tiles off) one holder more, at a full front (at war) two"),
        ts(C(c::ALARM), Shape::Above(250), 3200, "the alarm: the threat a third of the threat and the guard holds nothing more, level (0.5) 800 more: a city one, the capital two, three from 0.56. A scout or two in the ring is no alarm (the watch read a danger in 46 % of the seat-turns of the tuning games, most at a threat of 4–14 against a guard of 50–90: at a flat 800 the cities held 0.41 extra holders on the mean). 16 of 17 falls had the alarm 10 rounds or more ahead, time for 2–4 defenders from the city's own production (fractal 4101012 p1: a stack of 20 units 4–8 tiles from the capital at peace for 15 rounds, read at a stranger's half, 57 against 89, 0.39: one more; 0.56 and up: three)"),
        t(S(s::CAPITAL_FALL), 600, "after a fall the rest must hold: what is left is a few cities (0–3 in the traced falls)"),
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
    pub const EMBARGO: (Milli, &str) = (600, "the congress embargo cuts the runaway's trade routes and deals for a proposal's price, no war: it is proposed and voted only on a rival whose runaway is clearly above even, so a level race is not braked (W2)");
    pub const BLOCK: (Milli, &str) = (500, "the ballot goes against a rival only when it would win the vote");
    pub const SPACE_RESEARCH: (Milli, &str) = (500, "the research keeps to the ship's way when the science aim and the way gone say so more likely than not");
    pub const CAPITAL_GATE: (Milli, &str) = (500, "the capital is kept for the parts when a part outside it says so more likely than not");
    pub const AGREEMENT: (Milli, &str) = (500, "a tie when the agreement pays more likely than not (`weights::AGREEMENT`)");
    /// The rounds between two offers of a tie's ladder at the gate (`science::cadence`; 1 at a level of 1000).
    pub const LADDER_ROUNDS: (u32, &str) = (5, "a tie at the gate asks no more often than any deal (`diplomacy::PROPOSAL_INTERVAL`); a sure one asks every round, so an agreement ended or a friendship made waits no interval for the next step (the look: 0.58 of a friend a turn without an agreement nothing in the rules blocked)");
    /// The Standard rounds of an unsigned chest at which `r::CHEST_WAIT` is full (`science::read_waits`).
    pub const CHEST_WAIT_ROUNDS: (u32, &str) = (20, "four proposal intervals: past them a partner's refusal is not its gold, which a few rounds of its income bring");
    /// Food a turn a city keeps when it takes a science specialist (`citizens::science_specialist_commands`).
    pub const SCIENTIST_FOOD: (i32, &str) = (2, "two food a turn: the city still grows, slower, and never starves for a slot");
    /// A city's flat garrison is built from this `weights::GARRISON` level.
    pub const GARRISON: (Milli, &str) = (500, "the garrison when the threats outweigh the low production");
    /// Turns a growth item still needs for the chest to wait for it (`weights::gb::WAITING`).
    pub const CHEST_TURNS: (u32, &str) = (4, "an item done in a few turns comes without the gold: the gold waits for the heads it saves the most rounds on");
    /// A queue head spends the growth chest from this `weights::CHEST_ITEM` level.
    pub const CHEST_ITEM: (Milli, &str) = (500, "a head the chest is for, more likely than not");
    /// The jobs ahead are counted from this `weights::WORKER_SHORT` level.
    pub const WORKER_SHORT: (Milli, &str) = (500, "a city without its worker, more likely than not");
    /// `layout-defence`: the hold level from which a city keeps one holder beyond the standing garrison (`weights::HOLD`).
    pub const HOLD_ONE: (Milli, &str) = (500, "one more holder when the hold says so more likely than not");
    /// `layout-defence`: the hold level from which a city keeps two.
    pub const HOLD_TWO: (Milli, &str) = (850, "two more only on a clear front: the capital at war with a near land rival, or a capital in the watch");
    /// `layout-defence`: the hold level at which a city keeps three (the top of the level).
    pub const HOLD_THREE: (Milli, &str) = (1000, "three more only at the top: a city in the watch on the front of a war (the traced stacks were 8–32 units against 3–9)");
    /// Ships escorting an `Overseas` order (`fleet::FLEET_SIZE`).
    pub const ESCORT_SHIPS: (u32, &str) = (2, "the landing needs ships that shell the coast (`fleet.rs` 1-10)");
    /// Standard rounds after a war order spent its budget without paying before another order on that rival.
    pub const SPENT_ROUNDS: (u32, &str) = (20, "the war that did not pay ends, and is not restarted at once");
    /// Standard rounds a war order may wait for its army at peace before it is spent.
    pub const PREPARE_ROUNDS: (u32, &str) = (30, "the war machine's `FORM_LIMIT`: an army that has not marched by then does not");
}

/// The site head (§3.4): site = clamp(S1, S2 − BOUND, S2 + BOUND) + S3 − GAP·S4·(1 + S5/1000) − ARMY·S6 − TRAVEL·S7 + FILL·S8·S9/10⁶
/// − S11·S12/1000 (S12 = SHARED for `early-growth`, else 0).
pub mod site {
    pub const BOUND: (i64, &str) = (169, "`bounded_net`: one citizen; outside it the net reads off its data");
    pub const GAP: (i64, &str) = (30, "`RIVAL_PENALTY`: the forward city is the one the next war takes first; up to twice next to a menacing rival");
    pub const ARMY: (i64, &str) = (60, "`ARMY_PENALTY`: an army 4–6 tiles off reaches the settler in a turn or two");
    pub const TRAVEL: (i64, &str) = (40, "`TRAVEL_COST` 30 + `NET_TRAVEL_RISK` 10");
    /// S12 of `early-growth` (`Growth::shared`), thousandths of S11.
    pub const SHARED: (i64, &str) = (750, "three quarters of the land a site shares with an own city: science-flow's non-capital cities founded by r60 had 3.65 citizens 30 rounds later at 3 tiles from an own city (14 shared tiles of 36), 3.62 at 4 (9), 4.30 at 5 (4), 4.94 at 6+ (1), the later ones 4.42, 4.73, 5.56, 5.89 (1135 cities); the least squares of the share of citizens lost on the share of tiles shared, by founding half, is 0.76 (95 % CI 0.44–0.99, games resampled; tools/derive.py, runs/derive-e1-sf.md)");
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
    // `early-growth`'s own neurons (read outside `neurons()`).
    for neuron in [&EXPANSION_TILT, &WORKER_TILT, &OPENING_ROOM, &OPENING, &GARRISON, &CHEST_ITEM, &BUY_CHEST, &WORKER_SHORT, &JOBS_CAP] {
        n += usize::from(neuron.bias != 0);
        n += neuron.terms.len();
    }
    // The gates read by `neurons()`' levers (11) and the growth's (`gate::GARRISON`, `CHEST_TURNS`, `CHEST_ITEM`,
    // `WORKER_SHORT`), the site head (5) and its shared land (`site::SHARED`).
    n + TILT.len() + 15 + 6
}
