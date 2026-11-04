//! Every hand weight of the strategy net, each with the reason it has its
//! value (classic/docs/ai-strategy-net.md; the Russian page
//! work/strategy-net-weights.html prints these tables). Nothing here was
//! trained: the numbers are written from the rules and from how the game is
//! played. All values are thousandths (`Milli`: 1000 = 1.0) unless a
//! constant says otherwise.
//!
//! Layers: the situation (`f`: features 0…1, computed by `features`), the
//! concepts (`k`: `CONCEPTS`), the intent (`STYLE`, `AIM`, `POSTURE`
//! logits, softmax; `CITY_ROWS`, `HORIZON_*`), the prices (`PRICE_ROWS`,
//! `GATES`: deviations of each price from the allocator's), the goals
//! (`goal`), the heads (`head`) and the effect classifier (`effect`).

use super::Milli;

/// An input of a neuron: a feature of the situation or a concept.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Input {
    F(usize),
    K(usize),
}

/// A neuron's activation: clamped to 0…1, or a linear logit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Act {
    Unit,
    Linear,
}

/// One weighted input and why it has that weight.
#[derive(Debug, Clone, Copy)]
pub struct Term {
    pub input: Input,
    pub w: Milli,
    pub why: &'static str,
}

/// A neuron: bias + Σ w·x, then its activation.
#[derive(Debug, Clone, Copy)]
pub struct Neuron {
    pub name: &'static str,
    pub bias: Milli,
    pub why: &'static str,
    pub terms: &'static [Term],
    pub act: Act,
}

const fn f(i: usize, w: Milli, why: &'static str) -> Term {
    Term { input: Input::F(i), w, why }
}

const fn k(i: usize, w: Milli, why: &'static str) -> Term {
    Term { input: Input::K(i), w, why }
}

// ---- layer 0: the situation (`features`) ----

/// The features, 0…1 each (`features::situation` computes them; the
/// scale of each is its doc line in `FEATURES`).
pub mod fe {
    pub const CAP_FOOD: usize = 0;
    pub const CAP_PROD: usize = 1;
    pub const CAP_LUX: usize = 2;
    pub const ROOM: usize = 3;
    pub const SITES_FOOD: usize = 4;
    pub const RIVAL_NEAR: usize = 5;
    pub const RIVAL_KNOWN: usize = 6;
    pub const BARBS: usize = 7;
    pub const RUINS: usize = 8;
    pub const CARAVAN_FOOD: usize = 9;
    pub const CITY_STATES: usize = 10;
    pub const CS_NEAR: usize = 11;
    pub const COASTAL: usize = 12;
    pub const ERA: usize = 13;
    pub const PROGRESS: usize = 14;
    pub const CITIES: usize = 15;
    pub const CITIZENS: usize = 16;
    pub const SCIENCE: usize = 17;
    pub const PRODUCTION: usize = 18;
    pub const GOLD: usize = 19;
    pub const AT_WAR: usize = 20;
    pub const FORCE: usize = 21;
    pub const THREAT: usize = 22;
    pub const HAPPY: usize = 23;
    pub const UNHAPPY: usize = 24;
    pub const FAITH: usize = 25;
    pub const RELIGION: usize = 26;
    pub const TOURISM: usize = 27;
    pub const TECH_GAP: usize = 28;
    pub const TECH_LEAD: usize = 29;
    pub const SCORE_GAP: usize = 30;
    pub const WONDERS_TAKEN: usize = 31;
    pub const OUR_WONDERS: usize = 32;
    pub const RIVALS: usize = 33;
    pub const T_EXPANSION: usize = 34;
    pub const T_WONDERS: usize = 35;
    pub const T_SCIENCE: usize = 36;
    pub const T_CULTURE: usize = 37;
    pub const T_GOLD: usize = 38;
    pub const T_RELIGION: usize = 39;
    pub const T_AGGRESSION: usize = 40;
    pub const T_WAR_TOLERANCE: usize = 41;
    pub const T_DIPLOMACY: usize = 42;
    pub const UNIQUE_MILITARY: usize = 43;
    pub const IDEOLOGY: usize = 44;
    pub const ROUTES_LEFT: usize = 45;
    pub const N: usize = 46;
}

/// Each feature's name and scale (0 … 1).
pub const FEATURES: [(&str, &str); fe::N] = [
    ("cap_food", "mean food of the capital's 12 best tiles: 1.2 → 0, 3.0 → 1"),
    ("cap_prod", "mean production of those tiles: 0.5 → 0, 2.0 → 1"),
    ("cap_luxury", "luxury resources in the capital's reach: 3 → 1"),
    ("room", "free good city sites within 10 tiles of the capital, 4 apart: 6 → 1"),
    ("sites_food", "mean food of the best tiles of the 4 best free sites, as cap_food"),
    ("rival_near", "nearest known city of a rival nation: 7 tiles → 1, 18 → 0, none known → 0"),
    ("rival_known", "a rival nation's city is known"),
    ("barbarians", "camps within reach × 0.35 + barbarian units near the cities × 0.2"),
    ("ruins", "unclaimed ruins known within 12 tiles of the capital × 0.25"),
    ("caravan_food", "food the internal food routes of the cities and planned sites carry: 8 food → 1"),
    ("city_states", "city-states met: 4 → 1"),
    ("city_states_near", "met city-states with a known city within 12 tiles of the capital: 2 → 1"),
    ("coastal", "the capital is on the coast"),
    ("era", "our era over the last era"),
    ("progress", "the round over the last round"),
    ("cities", "own cities: 8 → 1"),
    ("citizens", "mean citizens a city: 10 → 1"),
    ("science", "beakers a citizen: 1.5 → 1"),
    ("production", "production a city: 15 → 1"),
    ("gold", "gold income: −5 → 0, +30 → 1"),
    ("at_war", "at war with a nation"),
    ("force", "own military power over own plus the hostile power in sight (1: nothing hostile seen)"),
    ("threat", "share of the cities with hostile power near (the defence watch)"),
    ("happy", "excess happiness: −10 → 0, +10 → 1"),
    ("unhappy", "unhappiness: 0 → 0, −10 → 1"),
    ("faith", "faith a turn: 8 → 1"),
    ("religion", "a religion founded"),
    ("tourism_pressure", "public opinion of our ideology: content 0, murmurs 0.4, unrest 0.7, upheaval 1"),
    ("tech_gap", "eras behind the most advanced rival met: 2 → 1"),
    ("tech_lead", "eras ahead of the most advanced rival met: 2 → 1"),
    ("score_gap", "(best rival score − ours) / ours: +100 % → 1"),
    ("wonders_taken", "world wonders built anywhere over all world wonders"),
    ("our_wonders", "our world wonders: 4 → 1"),
    ("rivals", "nations met: 4 → 1"),
    ("trait_expansion", "the leader's personality from the rules: expansion 0…10"),
    ("trait_wonders", "personality: wonders"),
    ("trait_science", "personality: science"),
    ("trait_culture", "personality: culture"),
    ("trait_gold", "personality: gold"),
    ("trait_religion", "personality: religion"),
    ("trait_aggression", "personality: aggression"),
    ("trait_war_tolerance", "personality: war tolerance"),
    ("trait_diplomacy", "personality: diplomacy"),
    ("unique_military", "unique military units of the first two eras in the nation's rules"),
    ("ideology", "an ideology chosen"),
    ("routes_left", "trade routes it may still start: 3 → 1"),
];

// ---- layer 1: concepts ----

/// The concepts, in the order they are computed (a concept may read the
/// earlier ones).
pub mod kc {
    pub const GOOD_LAND: usize = 0;
    pub const ROOM_TO_EXPAND: usize = 1;
    pub const TALL_GROWTH: usize = 2;
    pub const DANGER: usize = 3;
    pub const WARLIKE: usize = 4;
    pub const EARLY: usize = 5;
    pub const STRIKE_WINDOW: usize = 6;
    pub const WONDER_CHANCE: usize = 7;
    pub const SCIENCE_LEAD: usize = 8;
    pub const CULTURE_PRESSURE: usize = 9;
    pub const HAPPINESS_CRISIS: usize = 10;
    pub const CITY_STATE_PLAY: usize = 11;
    pub const RELIGIOUS_PLAY: usize = 12;
    pub const N: usize = 13;
}

use fe::*;
use kc::*;

/// The concept neurons: AND(a, b) is relu(a + b − 1), NOT a is a negative
/// weight, a threshold is the bias; every concept is clamped to 0…1.
pub const CONCEPTS: [Neuron; kc::N] = [
    Neuron {
        name: "good_land",
        bias: -100,
        why: "a middling capital is not good land",
        act: Act::Unit,
        terms: &[
            f(CAP_FOOD, 550, "the food of the capital's best tiles is growth itself"),
            f(CAP_PROD, 250, "hammers build the growth buildings and the wonders"),
            f(SITES_FOOD, 150, "fertile land around: the next cities grow too"),
            f(CAP_LUX, 200, "luxuries keep a big city content"),
        ],
    },
    Neuron {
        name: "room_to_expand",
        bias: -150,
        why: "one free site is not room",
        act: Act::Unit,
        terms: &[
            f(ROOM, 1300, "free good sites within 10 tiles are the room itself"),
            f(RIVAL_NEAR, -600, "a rival close takes the sites first and threatens the settlers"),
            f(BARBS, -250, "camps around make every settler an escort job"),
            f(SITES_FOOD, 200, "fertile sites are worth the walk"),
            f(T_EXPANSION, 200, "a leader who likes new cities sees room sooner"),
        ],
    },
    Neuron {
        name: "tall_growth",
        bias: 0,
        why: "AND(good land, NOT room) = relu(good_land − room)",
        act: Act::Unit,
        terms: &[
            k(GOOD_LAND, 1000, "AND: good land …"),
            k(ROOM_TO_EXPAND, -1000, "… and NOT room to expand"),
            f(CARAVAN_FOOD, 400, "internal food routes feed a few big cities"),
        ],
    },
    Neuron {
        name: "danger",
        bias: -150,
        why: "nothing hostile is no danger; a neighbour alone is not yet one",
        act: Act::Unit,
        terms: &[
            f(RIVAL_NEAR, 250, "a rival city a few tiles away can strike at once"),
            f(BARBS, 400, "camps send raiders at the cities"),
            f(AT_WAR, 600, "a war is danger now"),
            f(THREAT, 900, "cities with hostile power around are the danger itself"),
            f(FORCE, -400, "our army against what is in sight lowers it"),
            f(RIVALS, 100, "more nations met, more chances of a war"),
        ],
    },
    Neuron {
        name: "warlike",
        bias: -550,
        why: "a neutral leader (5 of 10) is not warlike",
        act: Act::Unit,
        terms: &[
            f(T_AGGRESSION, 1000, "the leader's appetite for conquest, from the rules"),
            f(UNIQUE_MILITARY, 400, "an early unique unit is a window the nation is built to use"),
            f(T_WAR_TOLERANCE, 300, "a leader who minds conquest's unrest little"),
        ],
    },
    Neuron {
        name: "early",
        bias: 1000,
        why: "the game's start is the earliest",
        act: Act::Unit,
        terms: &[
            f(ERA, -2500, "every era closes the window; by the medieval era it is shut"),
            f(PROGRESS, -1500, "the share of the game gone"),
        ],
    },
    Neuron {
        name: "strike_window",
        bias: -2600,
        why: "AND of three (warlike, rival near, early): only all three together open the window; two of three stay shut",
        act: Act::Unit,
        terms: &[
            k(WARLIKE, 1250, "AND: a leader who wants it …"),
            f(RIVAL_NEAR, 1250, "… a rival capital within reach …"),
            k(EARLY, 1250, "… while its walls and army are weak"),
            f(FORCE, 100, "an army already at hand opens it a little sooner"),
        ],
    },
    Neuron {
        name: "wonder_chance",
        bias: 100,
        why: "a small base chance of winning a race",
        act: Act::Unit,
        terms: &[
            f(CAP_PROD, 700, "the capital's hammers decide a wonder race"),
            f(T_WONDERS, 500, "a leader who loves wonders"),
            f(WONDERS_TAKEN, -600, "the more already built, the fewer left and the more contested"),
            k(DANGER, -400, "a threatened empire builds walls, not wonders"),
            f(RIVALS, -150, "every rival met is a competitor in the race"),
        ],
    },
    Neuron {
        name: "science_lead",
        bias: 300,
        why: "level with the rivals is a modest lead",
        act: Act::Unit,
        terms: &[
            f(TECH_LEAD, 800, "eras ahead of the best rival"),
            f(TECH_GAP, -700, "eras behind it"),
            f(SCIENCE, 300, "beakers a citizen: the engine of a lead"),
            f(T_SCIENCE, 200, "a scientific leader"),
        ],
    },
    Neuron {
        name: "culture_pressure",
        bias: -100,
        why: "murmurs alone are little pressure",
        act: Act::Unit,
        terms: &[
            f(TOURISM, 1200, "public opinion against our ideology is the pressure itself"),
            f(IDEOLOGY, 100, "only an ideology can be pressed"),
        ],
    },
    Neuron {
        name: "happiness_crisis",
        bias: -100,
        why: "a point below zero is not yet a crisis",
        act: Act::Unit,
        terms: &[f(UNHAPPY, 1600, "five below zero is most of a crisis (growth −75 %, settlers soon stop)")],
    },
    Neuron {
        name: "city_state_play",
        bias: -300,
        why: "a few city-states far away are no plan",
        act: Act::Unit,
        terms: &[
            f(CITY_STATES, 500, "city-states met"),
            f(CS_NEAR, 400, "near ones are cheap to befriend and to protect"),
            f(T_DIPLOMACY, 500, "a diplomatic leader"),
            f(GOLD, 200, "gold buys their friendship"),
        ],
    },
    Neuron {
        name: "religious_play",
        bias: -300,
        why: "a leader of the middle without a religion has no use for faith policies",
        act: Act::Unit,
        terms: &[
            f(RELIGION, 600, "a religion founded: its beliefs make the faith policies pay"),
            f(T_RELIGION, 600, "a pious leader (the nation's leaning, from the rules)"),
            f(FAITH, 300, "faith already flowing"),
        ],
    },
];

// ---- layer 1: the intent (logits, softmax) ----

pub const STYLES: [&str; 3] = ["tall", "wide", "conquest"];
pub const AIMS: [&str; 4] = ["science", "culture", "domination", "diplomacy"];
pub const POSTURES: [&str; 4] = ["peace", "defence", "prepare", "war"];

pub const STYLE: [Neuron; 3] = [
    Neuron {
        name: "style_tall",
        bias: -200,
        why: "no tilt without a reason",
        act: Act::Linear,
        terms: &[
            k(TALL_GROWTH, 2500, "the tall concept itself"),
            k(GOOD_LAND, 800, "good land rewards growing big"),
            k(SCIENCE_LEAD, 500, "a lead is kept by a compact empire"),
            k(ROOM_TO_EXPAND, -600, "room left empty is lost to tall play"),
            f(T_SCIENCE, 600, "a scientific leader"),
            f(BARBS, 400, "barbarian pressure: few cities behind the capital's defence, no settlers to escort"),
            f(CS_NEAR, 300, "city-states next door take the land a wide empire would settle; their gifts feed a few big cities"),
        ],
    },
    Neuron {
        name: "style_wide",
        bias: -600,
        why: "tall is the stronger play under these rules (the science per research cost of few cities): wide needs a reason",
        act: Act::Linear,
        terms: &[
            k(ROOM_TO_EXPAND, 1500, "room is what wide play needs"),
            f(T_EXPANSION, 800, "a leader who likes new cities"),
            f(HAPPY, 600, "happiness headroom pays for new cities"),
            f(CAP_LUX, 300, "luxuries carry more cities"),
            k(DANGER, -800, "settlers die in danger"),
            k(HAPPINESS_CRISIS, -1500, "an unhappy empire cannot found"),
            f(RUINS, 300, "ruins near home: the scouts' gifts pay an early expansion"),
        ],
    },
    Neuron {
        name: "style_conquest",
        bias: -900,
        why: "conquest is the exception, not the default",
        act: Act::Linear,
        terms: &[
            k(STRIKE_WINDOW, 3000, "the early strike window"),
            k(WARLIKE, 1500, "a warlike leader"),
            f(FORCE, 800, "an army that outweighs what it sees"),
            f(SCORE_GAP, 500, "behind on score: taking cities closes it"),
            f(AT_WAR, 500, "already at war"),
            k(HAPPINESS_CRISIS, -800, "conquered cities add unhappiness"),
        ],
    },
];

pub const AIM: [Neuron; 4] = [
    Neuron {
        name: "aim_science",
        bias: 300,
        why: "the science victory is the project's goal: it leads on a tie",
        act: Act::Linear,
        terms: &[
            k(SCIENCE_LEAD, 1500, "a lead in technology is half the spaceship"),
            f(T_SCIENCE, 1000, "a scientific leader"),
            k(TALL_GROWTH, 700, "tall growth turns into beakers"),
            f(ERA, 500, "late in the game the spaceship comes into reach"),
        ],
    },
    Neuron {
        name: "aim_culture",
        bias: -300,
        why: "culture needs a reason",
        act: Act::Linear,
        terms: &[
            f(T_CULTURE, 1200, "a cultural leader"),
            k(WONDER_CHANCE, 1000, "wonders hold great works and tourism"),
            f(OUR_WONDERS, 500, "wonders already ours"),
        ],
    },
    Neuron {
        name: "aim_domination",
        bias: -700,
        why: "domination needs a warlike reason",
        act: Act::Linear,
        terms: &[
            k(WARLIKE, 1800, "a warlike leader"),
            k(STRIKE_WINDOW, 1000, "a window to take a capital early"),
            f(FORCE, 500, "an army that outweighs the neighbours"),
        ],
    },
    Neuron {
        name: "aim_diplomacy",
        bias: -700,
        why: "diplomacy needs city-states and gold",
        act: Act::Linear,
        terms: &[
            k(CITY_STATE_PLAY, 1800, "city-states to win over"),
            f(T_DIPLOMACY, 800, "a diplomatic leader"),
            f(GOLD, 500, "gold buys their votes"),
        ],
    },
];

pub const POSTURE: [Neuron; 4] = [
    Neuron {
        name: "posture_peace",
        bias: 1000,
        why: "peace is the default",
        act: Act::Linear,
        terms: &[
            k(DANGER, -1500, "danger ends the peace of mind"),
            k(STRIKE_WINDOW, -1200, "a window to strike is not a time to sit"),
            f(AT_WAR, -2000, "a war is not peace"),
        ],
    },
    Neuron {
        name: "posture_defence",
        bias: -600,
        why: "defence needs a threat",
        act: Act::Linear,
        terms: &[
            k(DANGER, 1800, "danger is the reason to defend"),
            f(THREAT, 1000, "cities with hostile power near"),
            f(FORCE, -500, "a weak army defends rather than attacks"),
        ],
    },
    Neuron {
        name: "posture_prepare",
        bias: -800,
        why: "preparing a war needs a reason",
        act: Act::Linear,
        terms: &[
            k(STRIKE_WINDOW, 2500, "the window: build the army that uses it"),
            k(WARLIKE, 600, "a warlike leader prepares in any case"),
            f(RIVAL_NEAR, 400, "a target within reach"),
            f(FORCE, -400, "the army is not yet strong enough to strike"),
        ],
    },
    Neuron {
        name: "posture_war",
        bias: -1200,
        why: "war is the last step",
        act: Act::Linear,
        terms: &[
            f(AT_WAR, 3000, "a war under way is fought"),
            k(STRIKE_WINDOW, 800, "the window open"),
            f(FORCE, 300, "an army strong enough (a war needs a reason first)"),
        ],
    },
];

/// A choice of the intent changes only when the new one's share leads the
/// held one's by more than this (thousandths: 15 %).
pub const HYSTERESIS: Milli = 1150;

/// The style is chosen once (classic/docs/ai-strategy-net.md, "The style
/// lock"): it is locked when the first branch may be opened after the first
/// survey of the surroundings (`LOCK_MIN_ROUNDS` Standard rounds), or at
/// `LOCK_MAX_ROUNDS` at the latest. A locked style is computed from the
/// situation at the lock with only the events (`STYLE_EVENTS`) taken from
/// the turn, so the empire's own growth (its cities filling the room, its
/// happiness spent on them) never moves it; another style replaces it only
/// when it leads by `STYLE_LOCK` (50 %), or by `HYSTERESIS` in a war for
/// survival (at war and a threatened city or danger above `SURVIVAL_*`).
pub const LOCK_MIN_ROUNDS: u32 = 20;
pub const LOCK_MAX_ROUNDS: u32 = 40;
pub const STYLE_LOCK: Milli = 1500;
pub const STYLE_EVENTS: [usize; 4] = [AT_WAR, THREAT, FORCE, UNHAPPY];
pub const SURVIVAL_THREAT: Milli = 300;
pub const SURVIVAL_DANGER: Milli = 600;

/// More cities by style: (per free site, base, why). A style's row is
/// weighed by its share; `CITY_*` below adjust the sum.
pub const CITY_ROWS: [(Milli, Milli, &str); 3] = [
    (500, 0, "tall takes the best few sites: half of the free ones (the book keeps four cities)"),
    (1000, 1000, "wide takes every good site and one beyond what it sees"),
    (500, 0, "conquest founds a few and takes the rest"),
];
/// Fewer new cities per unit of the happiness crisis.
pub const CITY_CRISIS: Milli = -6000;
/// Fewer new cities per unit of danger.
pub const CITY_DANGER: Milli = -1000;
/// The plan keeps at least this many cities while sites are free.
pub const CITY_FLOOR: u32 = 4;
/// The ceiling of the city count: the rules' city model (`tall::city_target`:
/// the count with the most science per research cost) plus this many per unit
/// of the wide and the conquest shares (thousandths of a city).
pub const CITY_CEILING_WIDE: Milli = 1000;
pub const CITY_CEILING_CONQUEST: Milli = 500;

/// The horizon H in Standard rounds: base and each term's weight (rounds
/// per unit of a share or a concept).
pub const HORIZON_BASE: Milli = 60;
pub const HORIZON_TERMS: [(&str, Milli, &str); 5] = [
    ("style_tall", 50, "tall play invests in growth that pays late"),
    ("aim_science", 30, "the spaceship is a long project"),
    ("style_conquest", -25, "conquest lives from the next few wars"),
    ("danger", -30, "danger shortens every plan"),
    ("posture_war", -20, "a war is fought now"),
];
pub const HORIZON_MIN: i64 = 20;
pub const HORIZON_MAX: i64 = 150;

// ---- layer 2: prices ----

/// The price dimensions: a multiplier of the allocator's price each
/// (`value::Prices`), 1000 = the allocator's own. Food is not here: its
/// price comes from the economy cell's growth formula over H.
pub mod pd {
    pub const SCIENCE: usize = 0;
    pub const PRODUCTION: usize = 1;
    pub const GOLD: usize = 2;
    pub const CULTURE: usize = 3;
    pub const FAITH: usize = 4;
    pub const HAPPINESS: usize = 5;
    pub const MILITARY: usize = 6;
    pub const WONDER: usize = 7;
    pub const EXPANSION: usize = 8;
    pub const TRADE: usize = 9;
    pub const GREAT_PEOPLE: usize = 10;
    pub const TOURISM: usize = 11;
    pub const WORKER: usize = 12;
    pub const CITIZEN: usize = 13;
    pub const FOOD: usize = 14;
    pub const N: usize = 15;
}

pub const PRICES: [&str; pd::N] = [
    "science", "production", "gold", "culture", "faith", "happiness", "military", "wonder", "expansion", "trade", "great_people", "tourism", "worker", "citizen", "food",
];

/// The rows of the price layer: the intent's shares (3 styles, 4 aims, 4
/// postures) and the two gates (danger, happiness crisis).
pub const ROWS: [&str; 13] = [
    "style_tall", "style_wide", "style_conquest", "aim_science", "aim_culture", "aim_domination", "aim_diplomacy", "posture_peace", "posture_defence", "posture_prepare",
    "posture_war", "gate_danger", "gate_crisis",
];

/// (row, price, deviation from the allocator's price per unit of the row,
/// why). The multiplier of a price is 1000 + Σ row × deviation.
pub const PRICE_ROWS: &[(usize, usize, Milli, &str)] = {
    use pd::*;
    &[
        (0, SCIENCE, 500, "a tall empire turns its citizens into beakers"),
        (0, CULTURE, 200, "few cities make policies cheap: culture buys more"),
        (0, WONDER, 400, "a tall capital is a wonder factory"),
        (0, EXPANSION, -700, "a city beyond the plan's count costs science and policy culture"),
        (0, TRADE, 400, "internal food routes are the tall growth engine"),
        (0, GREAT_PEOPLE, 400, "the specialists of big cities"),
        (0, CITIZEN, 500, "the big city's citizens are the point"),
        (0, HAPPINESS, 100, "big cities need content citizens"),
        (0, MILITARY, -200, "a small army defends a compact land"),
        (1, EXPANSION, 400, "every good site taken is land, citizens and score (the research cost per city tempers it)"),
        (1, HAPPINESS, 500, "each city costs happiness: it is the wide limit"),
        (1, PRODUCTION, 200, "many cities build many things"),
        (1, GOLD, 200, "roads and upkeep of many cities"),
        (1, WORKER, 400, "many cities need many improvements"),
        (1, WONDER, -300, "wonders compete with settlers for hammers"),
        (2, MILITARY, 1500, "the army is the plan"),
        (2, PRODUCTION, 400, "the army is built of hammers"),
        (2, HAPPINESS, 400, "conquered cities are unhappy"),
        (2, WONDER, -500, "hammers go to the army"),
        (2, SCIENCE, -200, "military technologies only"),
        (2, CULTURE, -200, "culture waits for the peace"),
        (2, GREAT_PEOPLE, -200, "great generals come from the fights, not from points"),
        (2, EXPANSION, -200, "cities are taken, not founded"),
        (3, SCIENCE, 800, "the spaceship is beakers"),
        (3, GREAT_PEOPLE, 300, "great scientists are bulbs of beakers"),
        (3, PRODUCTION, 100, "the spaceship parts are hammers"),
        (3, CULTURE, -100, "culture is a side line"),
        (4, CULTURE, 800, "culture is the aim"),
        (4, TOURISM, 1500, "tourism wins the culture victory"),
        (4, WONDER, 600, "wonders hold great works and bring tourism"),
        (4, GREAT_PEOPLE, 300, "great artists, writers and musicians make great works"),
        (4, FAITH, 200, "faith buys the culture buildings of a religion"),
        (5, MILITARY, 800, "the army takes the capitals"),
        (5, PRODUCTION, 200, "the army is built of hammers"),
        (5, WONDER, -200, "wonders are a luxury of peace"),
        (6, GOLD, 800, "gold buys city-state alliances and votes"),
        (6, TRADE, 300, "routes to city-states win their friendship"),
        (7, MILITARY, -300, "peace keeps the army small"),
        (8, MILITARY, 500, "defenders and walls"),
        (8, PRODUCTION, 200, "defenders are built of hammers"),
        (8, WONDER, -200, "a wonder in danger may be lost"),
        (9, MILITARY, 1000, "the army to strike with"),
        (9, PRODUCTION, 300, "the army is built of hammers"),
        (9, WONDER, -400, "hammers go to the army"),
        (9, EXPANSION, -200, "settlers wait for the war"),
        (10, MILITARY, 1600, "a war under way is won by units"),
        (10, PRODUCTION, 400, "units are hammers"),
        (10, WONDER, -600, "no wonders at war"),
        (10, EXPANSION, -400, "no settlers on a front"),
        (10, HAPPINESS, 200, "war weariness and captured cities"),
        (11, MILITARY, 800, "danger makes every unit worth more"),
        (11, PRODUCTION, 200, "the answer is built of hammers"),
        (11, WONDER, -300, "a wonder in danger may be lost"),
        (11, EXPANSION, -500, "settlers die in danger"),
        (11, GOLD, 200, "gold buys a defender at once"),
        (12, HAPPINESS, 2500, "a crisis stops growth and settling: happiness first"),
        (12, EXPANSION, -900, "no city is founded at very unhappy"),
        (12, CITIZEN, -400, "a new citizen deepens the crisis"),
        (12, WONDER, -200, "wonders wait for content citizens"),
    ]
};

/// The share of the written deviations a price takes (thousandths): the
/// allocator's prices are already the score's, the intent tilts them (at
/// the full deviations the net bought science buildings over food and gold
/// and grew 21 % fewer citizens: dev bench, offsets 120–125).
pub const PRICE_GAIN: Milli = 500;

/// The least and the most a multiplier may be (thousandths).
pub const PRICE_MIN: Milli = 100;
pub const PRICE_MAX: Milli = 6000;
/// The food price from the cell, as a multiplier of the allocator's, is
/// kept within these bounds (thousandths). The floor: the cell knows no
/// happiness the empire will build, so its mood caps growth too soon; below
/// 0.7 of the allocator's price (whose happiness outlook does) the net
/// starved its own growth (bench: 21 % fewer citizens at the end).
pub const FOOD_MIN: Milli = 700;
pub const FOOD_MAX: Milli = 4000;

// ---- the economy cell ----

pub mod cell {
    /// Coarse steps of the cell over the horizon H.
    pub const STEPS: i64 = 10;
    /// The weights a citizen's tile is chosen by (per unit of a yield):
    /// food, production, gold, science, culture, faith. Food first (a city
    /// grows from it), then hammers and beakers (the citizen AI's order).
    pub const TILE_WEIGHTS: [i64; 6] = [3, 2, 1, 2, 1, 1];
    /// The growth an unhappy empire keeps (percent): unhappy −75 %, very
    /// unhappy (−10) none (the rules' moods).
    pub const UNHAPPY_GROWTH: i64 = 25;
    /// Food a turn a city keeps above what it eats before its citizens
    /// prefer hammers and beakers to food.
    pub const FED_SURPLUS: i64 = 2;
    pub const VERY_UNHAPPY: i64 = -10;
    /// A production percent toward some items counts this share of the
    /// city's production when the planner has no share of its own.
    pub const PRODUCING_SHARE: i64 = 30;
    /// A garrison is assumed in this share of the cities (a garrison
    /// condition counts this much).
    pub const GARRISON_SHARE: i64 = 50;
    /// The yields of a city centre are at least these (food, production).
    pub const CENTRE_FOOD: i64 = 2;
    pub const CENTRE_PRODUCTION: i64 = 1;
}

// ---- the goals (the pointer) ----

pub mod goal {
    /// Goals chosen at most, and at least while any is worth something.
    pub const MAX: usize = 5;
    pub const MIN: usize = 3;
    /// Key buildings the plan holds beside those goals (they are cheap
    /// commitments the allocator makes anyway: they must not crowd out the
    /// wonders, branches and technologies).
    pub const MAX_BUILDINGS: usize = 2;
    /// A goal beyond the minimum stops the pointer when its rate is below
    /// this share of the first goal's rate (thousandths).
    pub const STOP_SHARE: i64 = 200;
    /// Candidates the pointer keeps after a first screening by their value
    /// alone.
    pub const CANDIDATES: usize = 24;
    /// A world wonder's chance to be ours (thousandths): base, per rival
    /// met in or past its era, per Standard round to get it, per unit of the
    /// wonder-chance concept, bounds.
    pub const CHANCE_BASE: i64 = 850;
    pub const CHANCE_PER_RIVAL: i64 = -120;
    pub const CHANCE_PER_ROUND: i64 = -8;
    pub const CHANCE_CONCEPT: i64 = 200;
    pub const CHANCE_MIN: i64 = 100;
    pub const CHANCE_MAX: i64 = 950;
    /// A technology that opens a project (the spaceship's) is worth this
    /// many world wonders' points times the science aim's share.
    pub const PROJECT_TECH_WONDERS: i64 = 3;
    /// The goals are reviewed every this many Standard rounds (and when a
    /// goal is lost, the cities change or a war starts or ends).
    pub const REVIEW_ROUNDS: u32 = 5;
    /// The land is surveyed again every this many Standard rounds.
    pub const SURVEY_ROUNDS: u32 = 10;
}

// ---- the policy branches: their character and their fit ----

/// A policy branch's character is read from the rules' data (`branches`):
/// each effect of its policies goes to one character by its kind, each
/// policy counts once; the character is the shares (thousandths). The fit
/// of a branch to the intent multiplies its value in the policy head and in
/// the goal pointer: 1 + Σ share × Σ input × weight. The rows are the
/// owner's play, taken as given: the growth branch for few strong cities
/// (defence and science), the expansion branch for many cities and for
/// conquest, the war branch only at war, the science branch almost always,
/// the gold branch rarely, the faith branch by the nation and its religion.
pub mod branch {
    use super::Milli;

    /// The characters.
    pub mod ch {
        /// Few strong cities: growth, the capital, free buildings in the
        /// first cities, border growth.
        pub const TALL: usize = 0;
        /// Many cities: settlers, workers, per-city yields and happiness,
        /// the policy cost of a city.
        pub const WIDE: usize = 1;
        /// Fighting units: combat, promotions, experience, kills, generals.
        pub const WAR: usize = 2;
        /// The cities' defence: garrisons, city combat.
        pub const DEFENCE: usize = 3;
        pub const SCIENCE: usize = 4;
        pub const GOLD: usize = 5;
        pub const FAITH: usize = 6;
        pub const CULTURE: usize = 7;
        pub const DIPLOMACY: usize = 8;
        pub const N: usize = 9;
    }

    pub const CHARACTERS: [&str; ch::N] = ["tall", "wide", "war", "defence", "science", "gold", "faith", "culture", "diplomacy"];

    /// An input of the fit: always 1, the held style (1 or 0), an aim's or
    /// a posture's share, a concept.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum FitInput {
        Bias,
        Style(usize),
        Aim(usize),
        Posture(usize),
        Concept(usize),
    }

    use super::kc::*;
    use ch::*;
    use FitInput::*;

    /// (input, character, weight per unit of the input and of the
    /// character's share, why).
    pub const FIT: &[(FitInput, usize, Milli, &str)] = &[
        (Style(0), TALL, 1500, "tall: the growth branch is the play of few strong cities"),
        (Style(1), TALL, -500, "wide: its capital-and-growth policies serve few cities"),
        (Style(2), TALL, -500, "conquest: the same"),
        (Style(1), WIDE, 1500, "wide: the expansion branch is the play of many cities"),
        (Style(2), WIDE, 1200, "conquest: the flexible branch of many cities, captured ones too"),
        (Style(0), WIDE, -500, "tall: settlers and per-city bonuses the plan does not use"),
        (Style(0), DEFENCE, 500, "tall: few cities are defended, not multiplied"),
        (Concept(DANGER), DEFENCE, 500, "danger: the cities' defence pays"),
        (Posture(3), WAR, 1500, "at war the war branch pays (outside war it is shut: `WAR_GATE`)"),
        (Bias, SCIENCE, 1500, "the science branch is almost a default: every victory needs technologies"),
        (Aim(0), SCIENCE, 500, "the science aim"),
        (Style(0), SCIENCE, 300, "tall: few big cities make their science in the capital's buildings"),
        (Bias, GOLD, -600, "the gold branch is rare: the science and the growth branches pay more"),
        (Aim(3), GOLD, 600, "the diplomatic aim buys city-states with gold"),
        (Bias, FAITH, -500, "faith policies need a religion to pay"),
        (Concept(RELIGIOUS_PLAY), FAITH, 1500, "a religion founded, a pious nation: the faith branch"),
        (Aim(1), CULTURE, 1000, "the culture aim"),
        (Bias, DIPLOMACY, -300, "city-state policies need city-states"),
        (Aim(3), DIPLOMACY, 1000, "the diplomatic aim"),
        (Concept(CITY_STATE_PLAY), DIPLOMACY, 500, "city-states met and near"),
    ];

    /// The bounds of the fit (thousandths).
    pub const FIT_MIN: Milli = 100;
    pub const FIT_MAX: Milli = 4000;
    /// A branch whose largest character is war is opened and continued only
    /// in the war posture (the owner's play: "the war branch only at war").
    pub const WAR_GATE: bool = true;
    /// The fit of the chosen ideology's tenets (it was chosen for the
    /// plan): on par with the style's branch, below the science branch.
    pub const IDEOLOGY_FIT: Milli = 2000;
}

// ---- the heads ----

pub mod head {
    /// Every head divides its score by (turns + TURNS_PAD).
    pub const TURNS_PAD: i64 = 2;
    /// Technologies: the share of a goal's worth carried back to each
    /// technology of its missing prerequisites (by the share of the path's
    /// cost the technology is), thousandths. 5×: a technology's own value
    /// (`planner_value::tech_value`) counts every item it unlocks in every
    /// city to the end of the game, a goal one item; 5 puts a goal's path on
    /// the same scale (measured: own values ~2000 points, goals 100–400).
    pub const BEELINE: i64 = 5000;
    /// A technology on the paths of two or more goals: its goal value × (1
    /// + this).
    pub const SHARED_BONUS: i64 = 300;
    /// The target technology is kept unless another leads its path by this
    /// much (thousandths: 15 %).
    pub const KEEP_MARGIN: i64 = 1150;
    /// Policies: a new branch is opened only when its step leads the best
    /// started branch's by this much (thousandths: 50 %), so a branch is
    /// finished first (its finisher, and the policy cost that grows with
    /// every adoption whatever the branch; traced games opened three
    /// branches at 10 %).
    pub const OPEN_MARGIN: i64 = 1500;
    /// A goal branch's steps are worth this much more (thousandths).
    pub const GOAL_BRANCH: i64 = 500;
    /// Policies: a branch is scored by its fit (`branch::FIT`) × (1 + this
    /// × its step's value in the cell over the best candidate's): the
    /// owner's play is the key, the cell breaks the near ties (the cell
    /// sees little of the growth and science branches: border growth,
    /// free buildings, a science percent while content, free technologies).
    pub const VALUE_WEIGHT: i64 = 500;
    /// The ideology is switched at unrest (the rules force it at upheaval)
    /// when the culture-pressure concept is above this.
    pub const SWITCH_PRESSURE: i64 = 600;
    /// Production: a goal's item takes this share of the goal's worth
    /// beside its own value (thousandths).
    pub const GOAL_ITEM: i64 = 500;
    /// Production: a military unit returns this share of its hammers'
    /// worth at the military price's excess over the allocator's, while the
    /// army is short of what it sees (thousandths).
    pub const MILITARY_RETURN: i64 = 1000;
    /// The own correction of the production head by kind of order
    /// (thousandths of the order's value), by style: (style, kind, value,
    /// why). Kinds: 0 settler, 1 worker, 2 work boat, 3 trade unit, 4
    /// military unit, 5 building, 6 world wonder, 7 national wonder.
    pub const KIND_ROWS: &[(usize, usize, i64, &str)] = &[
        (0, 6, 150, "a tall capital races for wonders"),
        (0, 3, 150, "tall play feeds its cities by caravans"),
        (1, 1, 150, "a wide empire needs its workers early"),
        (1, 0, 100, "wide play founds while sites are free"),
        (2, 4, 200, "conquest keeps a standing army"),
        (2, 5, -100, "buildings wait for the conquest"),
    ];
}

// ---- the effect classifier (what an effect is worth to us, beyond the
// yields the cell counts) ----

pub mod effect {
    /// A great person point is worth a technology's score points over this
    /// many points (a great person is about a technology).
    pub const POINTS_PER_PERSON: i64 = 100;
    /// A golden age adds this percent of the empire's production and gold.
    pub const GOLDEN_AGE_PERCENT: i64 = 20;
    /// A combat effect (a promotion, a combat or unit stat) is worth this
    /// many hammers per city at the production price, scaled by the
    /// military price.
    pub const COMBAT_HAMMERS: i64 = 20;
    /// A city-state effect is worth this much gold a turn per city-state
    /// met, at the gold price.
    pub const CITY_STATE_GOLD: i64 = 1;
    /// A cost cut of `p` % is worth p % of the yield it saves: border growth
    /// (culture), tile purchase (gold, this share of the income), the
    /// policy penalty of a city (culture × cities over this).
    pub const TILE_GOLD_SHARE: i64 = 30;
    pub const POLICY_PENALTY_CITIES: i64 = 10;
    /// A tourism percent counts on the empire's culture a turn.
    pub const TOURISM_ON_CULTURE: i64 = 100;
    /// An effect the classifier has no model of is worth nothing.
    pub const UNKNOWN: i64 = 0;
}

/// Every hand weight, counted: the concepts' and the intent's terms and
/// biases, the price rows, the city and horizon rows, the classifier's and
/// the heads' constants.
pub fn count() -> usize {
    let neurons = CONCEPTS.iter().chain(&STYLE).chain(&AIM).chain(&POSTURE).map(|n| n.terms.len() + 1).sum::<usize>();
    let cities = CITY_ROWS.len() * 2 + 5;
    let horizon = HORIZON_TERMS.len() + 1;
    let goals = 10;
    let heads = 9 + head::KIND_ROWS.len();
    let classifier = 8;
    let cell = 6 + 4;
    // The lock (margin, two rounds, two survival bounds) and the branch fit.
    let lock = 5;
    let fit = branch::FIT.len() + 4;
    neurons + PRICE_ROWS.len() + cities + horizon + goals + heads + classifier + cell + 2 + lock + fit
}
