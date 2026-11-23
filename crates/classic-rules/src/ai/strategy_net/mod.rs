//! The strategy net (`strategy-net`, classic/docs/ai-strategy-net.md): a
//! hand-written network whose neurons are concepts of the game and whose
//! weights are written from the rules and from how the game is played — no
//! training. It decides the research, the policies and, among the
//! allocator's best orders, the production of `standard`.
//!
//! ```text
//! layer 0  situation   ~46 numbers 0…1 from the observation and the rules (`features`)
//! layer 1  concepts    12 neurons: AND = relu(a+b−1), NOT = a negative weight (`weights::CONCEPTS`)
//!          intent      style (tall / wide / conquest), victory aim, posture: softmax,
//!                      held unless the new choice leads by 15 %; the style locked once
//!                      (50 %, events only); more cities; horizon H
//! layer 2  prices      15 shadow prices as multipliers of the allocator's (`weights::PRICE_ROWS`),
//!                      gated by danger and a happiness crisis; food by the cell's growth formula
//! cell     economy     the empire run forward in 10 steps over H by the rules' growth and tile
//!                      yields (`cell`): what a plan of wonders, buildings and policies is worth
//! pointer  goals       3–5 goals one at a time: (V(plan + goal) − V(plan)) × chance / (turns + 2)
//! heads    decisions   research (goal value carried back through the tech tree), policies,
//!                      production among the allocator's top 4: prices · effects / (turns + 2)
//! ```
//!
//! Everything is integer (thousandths) and deterministic. The intent, the
//! goals and the land survey are kept between turns in the player's AI
//! memory (`AiState::mind_memory`, unused by every other scripted variant).
//! No content is named: conditions such as "desert tiles" are the rules'
//! effect conditions evaluated on our own tiles.

pub mod beliefs;
pub mod branches;
pub mod cell;
pub mod effects;
pub mod features;
pub mod goals;
pub mod heads;
pub mod layers;
pub mod model;
pub mod sharp;
pub mod threat;
pub mod trace;
pub mod weights;
pub mod wishes;

use crate::ai::state::AiState;
use crate::ai::value::Prices;
use crate::ids::{BuildingKindId, PolicyId, TechId};
use crate::knowledge::observation::AIObservation;
use crate::rules::content::PolicyBranchId;
use crate::rules::CompiledRuleset;
use layers::{fire, hold, softmax, ONE};
use weights::{fe, kc, pd};

/// Thousandths: 1000 is 1.0.
pub type Milli = i64;

/// The intent of a turn (layer 1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Intent {
    pub concepts: [Milli; kc::N],
    /// Softmax shares of the styles, aims and postures.
    pub style: [Milli; 3],
    pub aim: [Milli; 4],
    pub posture: [Milli; 4],
    /// The choices held after the hysteresis (the style after its lock).
    pub style_pick: usize,
    pub aim_pick: usize,
    pub posture_pick: usize,
    /// New cities the plan wants beyond those it has.
    pub more_cities: u32,
    /// The horizon H in rounds of the game's speed.
    pub horizon: u32,
    /// The style is locked (`weights::LOCK_MIN_ROUNDS`).
    pub locked: bool,
    /// The intent of `sharp` (`Net::sharp`): its prices follow the held choice and its cell runs on
    /// its own scale (`sharp::rows`, `CellPrices::sharp`).
    pub sharp: bool,
    /// `tall-real`'s tall play (`ai::tall_real::plays`): the held style tall where the science
    /// victory fits the game; its aim is science and its cities the rules' size model's.
    pub tall_real: bool,
    /// The tall play takes the science branch whole before anything else once it is open
    /// (`heads::choose_policy`): `tall-real`'s only (`next` measured no gain from it).
    pub science_first: bool,
}

/// Which net thinks: `standard`'s, with `war-open`'s window and terms (`open`), with `sharp`'s
/// choice and inputs (`sharp`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Net {
    pub open: bool,
    pub sharp: bool,
    /// `war-pays`: the window of any era needs a war that pays (`fe::PAYS`), it is a condition, not a term;
    /// an operation forming does not tilt the posture (`weights::open::POSTURE`: it cost a point, see the doc).
    pub pays: bool,
    /// `strategist-x`: the threat of the rival running away (`threat::assess`, 0…1000), a term of the
    /// posture (`weights::threat::POSTURE`); 0 in every other net.
    pub threat: Milli,
    /// `strategist-x`: the wide style needs a strong reason (`weights::style::WIDE`); false elsewhere.
    pub tall_first: bool,
}

impl Net {
    /// `war-pays` leaves the opportunity out of the style and the aim (`weights::open::STYLE`, `AIM`,
    /// `STYLE_EVENTS`); see classic/docs/ai-war-pays.md for the measurement.
    pub fn pays_no_style(self) -> bool {
        self.pays && PAYS_NO_STYLE
    }
}

/// `war-pays`: no opportunity in the style and the aim (see `Net::pays_no_style`).
const PAYS_NO_STYLE: bool = true;

/// The choices held from the last turn.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Held {
    pub style: Option<usize>,
    pub aim: Option<usize>,
    pub posture: Option<usize>,
    /// The style's lock: the situation it was chosen in.
    pub lock: Option<StyleLock>,
}

/// The style chosen once: the round and the situation of the lock. The
/// style is computed from this situation with only the events
/// (`weights::STYLE_EVENTS`) taken from the turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StyleLock {
    pub round: u32,
    pub situation: [Milli; fe::N],
}

/// A war for survival: at war, and a city threatened or the danger high.
pub fn survival(features: &[Milli; fe::N], concepts: &[Milli; kc::N]) -> bool {
    features[fe::AT_WAR] > 0 && (features[fe::THREAT] >= weights::SURVIVAL_THREAT || concepts[kc::DANGER] >= weights::SURVIVAL_DANGER)
}

/// The concepts of a situation (`weights::CONCEPTS` in order).
pub fn concepts(features: &[Milli; fe::N]) -> [Milli; kc::N] {
    concepts_of(features, false)
}

/// The concepts; with `open` (`war-open`) the strike window is the larger of the early one and
/// the window of any era (`weights::open::WINDOW`). No later concept reads the window.
pub fn concepts_of(features: &[Milli; fe::N], open: bool) -> [Milli; kc::N] {
    concepts_net(features, Net { open, sharp: false, pays: false, threat: 0, tall_first: false })
}

/// The concepts of a net: `concepts_of`, with `sharp`'s terms (`sharp::w::CONCEPTS`) when `sharp`.
pub fn concepts_net(features: &[Milli; fe::N], net: Net) -> [Milli; kc::N] {
    let open = net.open;
    let mut out = [0; kc::N];
    for (i, neuron) in weights::CONCEPTS.iter().enumerate() {
        out[i] = if net.sharp { sharp::fire_with(neuron, sharp::w::CONCEPTS, i, features, &out) } else { fire(neuron, features, &out) };
        if open && i == kc::STRIKE_WINDOW && (!net.pays || features[fe::PAYS] > 0) {
            out[i] = out[i].max(fire(&weights::open::WINDOW, features, &out));
        }
    }
    out
}

/// The logits of a table of the intent, with `war-open`'s terms (`weights::open`) when `extra`
/// is given.
fn logits_of(table: &[weights::Neuron], extra: Option<&[(usize, weights::Term)]>, features: &[Milli; fe::N], concepts: &[Milli; kc::N]) -> Vec<Milli> {
    let mut out: Vec<Milli> = table.iter().map(|n| fire(n, features, concepts)).collect();
    for (at, term) in extra.unwrap_or_default() {
        let x = match term.input {
            weights::Input::F(i) => features[i],
            weights::Input::K(i) => concepts[i],
        };
        out[*at] += term.w * x / ONE;
    }
    out
}

/// Layer 1: the intent of a situation; `held` the choices of the last
/// turn, `cities` the cities now, `rounds_left` and `pace` (percent) the
/// game's.
pub fn intent(features: &[Milli; fe::N], held: Held, cities: u32, rounds_left: u32, pace: u32) -> Intent {
    intent_of(features, held, cities, rounds_left, pace, false)
}

/// `intent`; with `open` the net of `war-open` (`weights::open`): the window of any era, the
/// war machine's operation in the posture, the opportunity in the style and the aim.
pub fn intent_of(features: &[Milli; fe::N], held: Held, cities: u32, rounds_left: u32, pace: u32, open: bool) -> Intent {
    intent_net(features, held, cities, rounds_left, pace, Net { open, sharp: false, pays: false, threat: 0, tall_first: false })
}

/// `intent_of` for `war-pays`: `war-open`'s net whose window of any era needs a war that pays.
pub fn intent_pays(features: &[Milli; fe::N], held: Held, cities: u32, rounds_left: u32, pace: u32) -> Intent {
    intent_net(features, held, cities, rounds_left, pace, Net { open: true, sharp: false, pays: true, threat: 0, tall_first: false })
}

/// The logits of a table with `sharp`'s terms added (`sharp::w`), and its bias for `at`.
fn sharpened(mut logits: Vec<Milli>, table: &[(usize, weights::Term)], features: &[Milli; fe::N], concepts: &[Milli; kc::N]) -> Vec<Milli> {
    for (at, term) in table {
        let x = match term.input {
            weights::Input::F(i) => features[i],
            weights::Input::K(i) => concepts[i],
        };
        logits[*at] += term.w * x / ONE;
    }
    logits
}

/// The style logits of `sharp`: its terms and the tall bias.
fn sharp_style(logits: Vec<Milli>, features: &[Milli; fe::N], concepts: &[Milli; kc::N]) -> Vec<Milli> {
    let mut out = sharpened(logits, sharp::w::STYLE, features, concepts);
    out[0] += sharp::w::TALL_BIAS;
    out
}

/// Additions to the intent's logits before the temperature and the softmax: `net2`'s terms
/// (`ai::net2`); `Tilt::NONE` for the strategist, whose intent it leaves bit for bit.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Tilt {
    pub style: [Milli; 3],
    pub aim: [Milli; 4],
    pub posture: [Milli; 4],
}

impl Tilt {
    pub const NONE: Tilt = Tilt { style: [0; 3], aim: [0; 4], posture: [0; 4] };
}

/// The intent's logits before the temperature and the softmax, the concepts they read and the
/// style's hysteresis margin (the lock's or the plain one).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Logits {
    pub concepts: [Milli; kc::N],
    pub style: [Milli; 3],
    pub aim: [Milli; 4],
    pub posture: [Milli; 4],
    pub style_margin: Milli,
}

/// `intent_of` for a net (`Net`): `sharp`'s concepts, terms and temperatures when `sharp`.
pub fn intent_net(features: &[Milli; fe::N], held: Held, cities: u32, rounds_left: u32, pace: u32, net: Net) -> Intent {
    intent_tilted(features, held, cities, rounds_left, pace, net, &Tilt::NONE)
}

/// `intent_net` with `tilt` added to the logits (`net2`).
pub fn intent_tilted(features: &[Milli; fe::N], held: Held, cities: u32, rounds_left: u32, pace: u32, net: Net, tilt: &Tilt) -> Intent {
    intent_from(&logits_net(features, &held, net), features, held, cities, rounds_left, pace, net, tilt)
}

/// The style's logits: a locked style is the lock's situation with the turn's events; with its margin.
fn style_logits(features: &[Milli; fe::N], concepts: &[Milli; kc::N], held: &Held, net: Net) -> ([Milli; 3], Milli) {
    let open = net.open;
    let extra_style = |table: &'static [(usize, weights::Term)]| (open && !net.pays_no_style()).then_some(table);
    let (situation, lock_concepts, margin) = match &held.lock {
        Some(lock) => {
            let at_lock = lock_situation(lock, features, net);
            // The window of any era is a posture's reason, not a style's: a locked style weighs
            // the opportunity itself (`weights::open::STYLE`), not the window it opens for a few
            // reviews (with the window the conquest style held 35 % of the traced reviews and its
            // prices cost the economy; tuning offsets 200–201).
            let lock_concepts = concepts_net(&at_lock, Net { open: false, sharp: net.sharp, pays: false, threat: 0, tall_first: false });
            let margin = if survival(features, concepts) { weights::HYSTERESIS } else { weights::STYLE_LOCK };
            (at_lock, lock_concepts, margin)
        }
        None => (*features, *concepts, weights::HYSTERESIS),
    };
    let mut logits = logits_of(&weights::STYLE, extra_style(weights::open::STYLE), &situation, &lock_concepts);
    if net.sharp {
        logits = sharp_style(logits, &situation, &lock_concepts);
    }
    if net.tall_first {
        logits[1] += weights::style::WIDE.0;
    }
    (<[Milli; 3]>::try_from(logits).expect("three styles"), margin)
}

/// The situation of a style's lock with the turn's events.
fn lock_situation(lock: &StyleLock, features: &[Milli; fe::N], net: Net) -> [Milli; fe::N] {
    let mut at_lock = lock.situation;
    for e in weights::STYLE_EVENTS {
        at_lock[e] = features[e];
    }
    if net.open && !net.pays_no_style() {
        for e in weights::open::STYLE_EVENTS {
            at_lock[e] = features[e];
        }
    }
    if net.sharp {
        for e in sharp::w::STYLE_EVENTS {
            at_lock[e] = features[e];
        }
    }
    at_lock
}

/// The intent's logits of a net (before the temperatures and the softmax).
pub fn logits_net(features: &[Milli; fe::N], held: &Held, net: Net) -> Logits {
    let open = net.open;
    let concepts = concepts_net(features, net);
    let extra_style = |table: &'static [(usize, weights::Term)]| (open && !net.pays_no_style()).then_some(table);
    let extra_posture = |table: &'static [(usize, weights::Term)]| (open && !net.pays).then_some(table);
    let (style, style_margin) = style_logits(features, &concepts, held, net);
    let mut aim_logits = logits_of(&weights::AIM, extra_style(weights::open::AIM), features, &concepts);
    if net.sharp {
        aim_logits = sharpened(aim_logits, sharp::w::AIM, features, &concepts);
    }
    // The model of the rivals (`weights::rival`, every weight 0 until trained).
    let aim_logits = sharpened(aim_logits, weights::rival::AIM, features, &concepts);
    let mut posture_logits = sharpened(logits_of(&weights::POSTURE, extra_posture(weights::open::POSTURE), features, &concepts), weights::rival::POSTURE, features, &concepts);
    for (at, w, _) in weights::threat::POSTURE {
        posture_logits[*at] += w * net.threat / ONE;
    }
    let aim = <[Milli; 4]>::try_from(aim_logits).expect("four aims");
    let posture = <[Milli; 4]>::try_from(posture_logits).expect("four postures");
    Logits { concepts, style, aim, posture, style_margin }
}

/// `logits` plus `tilt`, element by element.
fn tilted<const N: usize>(logits: &[Milli; N], tilt: &[Milli; N]) -> [Milli; N] {
    let mut out = *logits;
    for (o, t) in out.iter_mut().zip(tilt) {
        *o += t;
    }
    out
}

/// The intent of logits: the softmaxes (with `sharp`'s temperatures), the held picks, the cities
/// wanted and the horizon.
#[allow(clippy::too_many_arguments)]
pub fn intent_from(logits: &Logits, features: &[Milli; fe::N], held: Held, cities: u32, rounds_left: u32, pace: u32, net: Net, tilt: &Tilt) -> Intent {
    let temper = |logits: [Milli; 3], t: Milli| if net.sharp { sharp::tempered(&logits, t) } else { logits };
    let temper4 = |logits: [Milli; 4], t: Milli| if net.sharp { sharp::tempered(&logits, t) } else { logits };
    let concepts = logits.concepts;
    let style = softmax(&temper(tilted(&logits.style, &tilt.style), sharp::w::STYLE_TEMPERATURE));
    let aim = softmax(&temper4(tilted(&logits.aim, &tilt.aim), sharp::w::AIM_TEMPERATURE));
    let posture = softmax(&temper4(tilted(&logits.posture, &tilt.posture), sharp::w::POSTURE_TEMPERATURE));
    let style_pick = hold(&style, held.style, logits.style_margin);
    let aim_pick = hold(&aim, held.aim, weights::HYSTERESIS);
    let posture_pick = hold(&posture, held.posture, weights::HYSTERESIS);
    let more_cities = more_cities_of(&style, &concepts, features, cities);
    let horizon = horizon_of(&style, &aim, &posture, &concepts, features, rounds_left, pace, net);
    Intent { concepts, style, aim, posture, style_pick, aim_pick, posture_pick, more_cities, horizon, locked: held.lock.is_some(), sharp: net.sharp, tall_real: false, science_first: false }
}

/// More cities: each style's rows weighed by its share.
fn more_cities_of(style: &[Milli; 3], concepts: &[Milli; kc::N], features: &[Milli; fe::N], cities: u32) -> u32 {
    let room = features[fe::ROOM] * 6; // free sites, thousandths
    let mut more: Milli = 0;
    for (s, (per_site, base, _)) in weights::CITY_ROWS.iter().enumerate() {
        more += style[s] * (room * per_site / ONE + base) / ONE;
    }
    more += concepts[kc::HAPPINESS_CRISIS] * weights::CITY_CRISIS / ONE;
    more += concepts[kc::DANGER] * weights::CITY_DANGER / ONE;
    let sites = (room / ONE) as u32;
    let mut more_cities = (more.max(0) + ONE / 2) as u64 / ONE as u64;
    let floor = weights::CITY_FLOOR.min(cities + sites).saturating_sub(cities);
    if concepts[kc::HAPPINESS_CRISIS] < ONE / 2 {
        more_cities = more_cities.max(u64::from(floor));
    }
    more_cities as u32
}

/// The horizon H in rounds of the game's speed.
#[allow(clippy::too_many_arguments)]
fn horizon_of(style: &[Milli; 3], aim: &[Milli; 4], posture: &[Milli; 4], concepts: &[Milli; kc::N], features: &[Milli; fe::N], rounds_left: u32, pace: u32, net: Net) -> u32 {
    let share = |name: &str| -> Milli {
        match name {
            "style_tall" => style[0],
            "style_conquest" => style[2],
            "aim_science" => aim[0],
            "posture_war" => posture[3],
            "danger" => concepts[kc::DANGER],
            _ => 0,
        }
    };
    let mut h = weights::HORIZON_BASE * ONE;
    for (name, w, _) in weights::HORIZON_TERMS {
        h += w * share(name);
    }
    if net.sharp {
        for (feature, w, _) in sharp::w::HORIZON {
            h += w * features[*feature];
        }
    }
    let h = (h / ONE).clamp(weights::HORIZON_MIN, weights::HORIZON_MAX);
    (h * i64::from(pace.max(1)) / 100).clamp(1, i64::from(rounds_left.max(1))) as u32
}

/// The 13 rows of the price layer: the 11 shares and the 2 gates.
pub fn rows(intent: &Intent) -> [Milli; 13] {
    let mut r = [0; 13];
    r[0..3].copy_from_slice(&intent.style);
    r[3..7].copy_from_slice(&intent.aim);
    r[7..11].copy_from_slice(&intent.posture);
    r[11] = intent.concepts[kc::DANGER];
    r[12] = intent.concepts[kc::HAPPINESS_CRISIS];
    r
}

/// Layer 2: the price multipliers of an intent (food 1000: the cell sets
/// it).
pub fn price_mult(intent: &Intent) -> [Milli; pd::N] {
    price_mult_tilted(intent, &[0; pd::N])
}

/// `price_mult` with `tilt` added to each multiplier before the bounds (`net2`'s price terms).
pub fn price_mult_tilted(intent: &Intent, tilt: &[Milli; pd::N]) -> [Milli; pd::N] {
    let rows = if intent.sharp { sharp::rows(intent) } else { rows(intent) };
    let mut mult = [ONE; pd::N];
    for (row, price, w, _) in weights::PRICE_ROWS {
        mult[*price] += rows[*row] * w / ONE * weights::PRICE_GAIN / ONE;
    }
    for (m, t) in mult.iter_mut().zip(tilt) {
        *m += t;
    }
    for m in mult.iter_mut() {
        *m = (*m).clamp(weights::PRICE_MIN, weights::PRICE_MAX);
    }
    mult[pd::FOOD] = ONE;
    mult
}

/// The allocator's prices the net multiplies (centi-points of the score,
/// a yield a turn kept to the last round; `value::Prices`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Base {
    pub rounds_left: i64,
    pub food: i64,
    pub production: i64,
    pub gold: i64,
    pub science: i64,
    pub culture: i64,
    pub faith: i64,
    pub happiness: i64,
    /// A new city founded now.
    pub new_city: i64,
    pub citizen: i64,
    pub wonder: i64,
    pub tech: i64,
}

impl Base {
    pub fn of(prices: &Prices) -> Self {
        Self {
            rounds_left: i64::from(prices.rounds_left.max(1)),
            food: prices.food,
            production: prices.production,
            gold: prices.gold,
            science: prices.science,
            culture: prices.culture,
            faith: prices.faith,
            happiness: prices.happiness,
            new_city: prices.new_city(0),
            citizen: prices.score.citizen,
            wonder: prices.score.wonder,
            tech: prices.score.tech,
        }
    }

    /// The allocator's price of a yield slot (food, production, gold,
    /// science, culture, faith).
    pub fn slot(&self, slot: usize) -> i64 {
        [self.food, self.production, self.gold, self.science, self.culture, self.faith][slot]
    }
}

/// The multiplier of a yield slot.
pub fn slot_mult(mult: &[Milli; pd::N], slot: usize) -> Milli {
    [mult[pd::FOOD], mult[pd::PRODUCTION], mult[pd::GOLD], mult[pd::SCIENCE], mult[pd::CULTURE], mult[pd::FAITH]][slot]
}

/// A goal of the plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GoalKind {
    /// A world wonder in the city of this id.
    Wonder { building: BuildingKindId, city: u64 },
    /// A building in every city that lacks it.
    Building(BuildingKindId),
    /// A policy branch opened (or finished, if open).
    Branch(PolicyBranchId),
    /// A technology that opens a project.
    Tech(TechId),
}

/// A chosen goal: what it is worth within the horizon (centi-points), its
/// rate (worth per round, the pointer's key), the rounds to get it, and the
/// technologies still missing on its way (prerequisites first).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Goal {
    pub kind: GoalKind,
    pub worth: i64,
    pub rate: i64,
    pub turns: i64,
    pub path: Vec<TechId>,
}

/// The net's answer of a turn, carried by the brief (`TurnBrief::strategy`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    pub round: u32,
    pub features: [Milli; fe::N],
    pub intent: Intent,
    /// Price multipliers (thousandths of the allocator's prices).
    pub mult: [Milli; pd::N],
    pub goals: Vec<Goal>,
    /// The goal technology the research holds (the end of a goal's path).
    pub target: Option<TechId>,
    /// The cities the plan keeps (the allocator founds up to them).
    pub city_target: u32,
    /// Per city id: the cell's mean yields over H per yield slot, as
    /// thousandths of today's (a percent bonus is worth this much more).
    pub growth: Vec<(u64, [Milli; 6])>,
    /// The net of `war-open` (`TurnBrief::war_open`): its army is the war machine's.
    pub war_open: bool,
    /// `strategist-x`: the wishes to the planners (the threat); empty elsewhere.
    pub wishes: wishes::Wishes,
}

impl Plan {
    /// A goal's item: the goal whose wonder or building it is.
    pub fn goal_of_building(&self, building: BuildingKindId) -> Option<&Goal> {
        self.goals.iter().find(|g| match g.kind {
            GoalKind::Wonder { building: b, .. } | GoalKind::Building(b) => b == building,
            _ => false,
        })
    }

    pub fn goal_branch(&self, branch: PolicyBranchId) -> Option<&Goal> {
        self.goals.iter().find(|g| g.kind == GoalKind::Branch(branch))
    }

    /// The growth factor of a city's yield slot (1000 when unknown).
    pub fn growth_of(&self, city: u64, slot: usize) -> Milli {
        self.growth.iter().find(|(id, _)| *id == city).map_or(ONE, |(_, g)| g[slot])
    }
}

// ---- the memory between turns ----

/// What the net keeps between turns (`AiState::mind_memory`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Memory {
    pub held: Held,
    /// The land survey (`features::Survey`) and its round.
    pub survey: Option<features::Survey>,
    /// The last review of the goals.
    pub review: Option<Review>,
    pub target: Option<TechId>,
    /// `sharp`'s own (`sharp::Memory`): written after the rest, only when there is one, so the
    /// other variants' memory keeps its bytes.
    pub sharp: Option<sharp::Memory>,
    /// `retake2`'s own (`retake::Memory`): a section before `sharp`'s, only when there is one.
    pub retake: Option<crate::ai::retake::Memory>,
    /// The samples of the rivals' screens (`ai::rival_model::Book`): written before `sharp`'s
    /// section (after the retake's) behind its marker (`RIVAL_SECTION`), only when there is one.
    pub rival_book: crate::ai::rival_model::Book,
    /// `net2`'s own (`ai::net2::plan::Memory2`): a section after the rivals' book and before `sharp`'s
    /// (`NET2_SECTION`), only when there is one, so the other variants' memory keeps its bytes.
    pub net2: Option<crate::ai::net2::plan::Memory2>,
}

/// A review of the goals and of the cell's prices.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Review {
    pub round: u32,
    pub cities: u32,
    pub at_war: bool,
    pub food: Milli,
    pub goals: Vec<Goal>,
    pub growth: Vec<(u64, [Milli; 6])>,
}

/// The memory's format version (the first byte).
const MEMORY_VERSION: u8 = 0x5B;
/// The markers of `sharp`'s section at the memory's end (`Memory::sharp`): `sharp`'s two numbers,
/// `next`'s four (`sharp::Memory::rivals` and `fits` too). The section's length follows from its
/// marker (`section_len`).
const SHARP_SECTION: u8 = 0x5C;
const NEXT_SECTION: u8 = 0x5D;
/// The marker of `retake2`'s section (a length of two bytes, then its body) before `sharp`'s. It was
/// 0x5D on the retake line, a marker `next` took for its section on the thirteenth promotion's line:
/// a mind with both sections (`next-war-retake`) needs two markers.
const RETAKE_SECTION: u8 = 0x5E;
/// The marker of the rivals' book (`Memory::rival_book`), after the retake's and before `sharp`'s section (0x5E was its marker on its own branch, which the retake line took first).
const RIVAL_SECTION: u8 = 0x5F;
/// The marker of `net2`'s section (a length of two bytes, then its body, `ai::net2::memory`), after the
/// rivals' book and before `sharp`'s; the memory's encode is its only writer.
const NET2_SECTION: u8 = 0x60;

/// The bytes of a section of `marker`: the marker and its numbers, eight bytes each (`next`'s:
/// the rivals and the science victory's fit too, −1 for none).
fn section_len(marker: u8) -> Option<usize> {
    let numbers = match marker {
        SHARP_SECTION => 2,
        NEXT_SECTION => 4,
        _ => return None,
    };
    Some(1 + numbers * std::mem::size_of::<i64>())
}

struct Writer(Vec<u8>);

impl Writer {
    fn i64(&mut self, v: i64) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn u32(&mut self, v: u32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn opt(&mut self, v: Option<usize>) {
        self.0.push(v.map_or(u8::MAX, |v| v as u8));
    }
}

struct Reader<'a>(&'a [u8]);

impl Reader<'_> {
    fn take<const N: usize>(&mut self) -> Option<[u8; N]> {
        let (head, rest) = self.0.split_first_chunk::<N>()?;
        self.0 = rest;
        Some(*head)
    }
    fn i64(&mut self) -> Option<i64> {
        self.take::<8>().map(i64::from_le_bytes)
    }
    fn u32(&mut self) -> Option<u32> {
        self.take::<4>().map(u32::from_le_bytes)
    }
    fn u8(&mut self) -> Option<u8> {
        self.take::<1>().map(|b| b[0])
    }
    fn opt(&mut self) -> Option<Option<usize>> {
        self.u8().map(|b| (b != u8::MAX).then_some(usize::from(b)))
    }
}

impl Memory {
    pub fn encode(&self) -> Vec<u8> {
        let mut w = Writer(vec![MEMORY_VERSION]);
        w.opt(self.held.style);
        w.opt(self.held.aim);
        w.opt(self.held.posture);
        match &self.held.lock {
            Some(l) => {
                w.0.push(1);
                w.u32(l.round);
                for v in &l.situation[..fe::BASE_N] {
                    w.i64(*v);
                }
            }
            None => w.0.push(0),
        }
        w.0.push(u8::from(self.target.is_some()));
        w.u32(self.target.map_or(0, |t| u32::from(t.0)));
        match &self.survey {
            Some(s) => {
                w.0.push(1);
                w.u32(s.round);
                w.u32(s.cities);
                for v in s.values {
                    w.i64(v);
                }
            }
            None => w.0.push(0),
        }
        match &self.review {
            Some(r) => {
                w.0.push(1);
                w.u32(r.round);
                w.u32(r.cities);
                w.0.push(u8::from(r.at_war));
                w.i64(r.food);
                w.u32(r.goals.len() as u32);
                for g in &r.goals {
                    let (kind, a, b) = match g.kind {
                        GoalKind::Wonder { building, city } => (0u8, u64::from(building.0), city),
                        GoalKind::Building(b) => (1, u64::from(b.0), 0),
                        GoalKind::Branch(b) => (2, u64::from(b.0), 0),
                        GoalKind::Tech(t) => (3, u64::from(t.0), 0),
                    };
                    w.0.push(kind);
                    w.i64(a as i64);
                    w.i64(b as i64);
                    w.i64(g.worth);
                    w.i64(g.rate);
                    w.i64(g.turns);
                    w.u32(g.path.len() as u32);
                    for t in &g.path {
                        w.u32(u32::from(t.0));
                    }
                }
                w.u32(r.growth.len() as u32);
                for (id, g) in &r.growth {
                    w.i64(*id as i64);
                    for v in g {
                        w.i64(*v);
                    }
                }
            }
            None => w.0.push(0),
        }
        if let Some(m) = &self.retake {
            let body = m.encode();
            w.0.push(RETAKE_SECTION);
            w.0.extend_from_slice(&(body.len() as u16).to_le_bytes());
            w.0.extend_from_slice(&body);
        }
        if !self.rival_book.samples.is_empty() {
            w.0.push(RIVAL_SECTION);
            self.rival_book.encode(&mut w.0);
        }
        if let Some(m) = &self.net2 {
            let body = m.encode();
            w.0.push(NET2_SECTION);
            w.0.extend_from_slice(&(body.len() as u16).to_le_bytes());
            w.0.extend_from_slice(&body);
        }
        if let Some(m) = &self.sharp {
            let next = m.rivals.is_some() || m.fits.is_some();
            w.0.push(if next { NEXT_SECTION } else { SHARP_SECTION });
            w.i64(m.ratio);
            w.i64(m.trend);
            if next {
                w.i64(m.rivals.unwrap_or(-1));
                w.i64(m.fits.map_or(-1, i64::from));
            }
        }
        w.0
    }

    /// The memory of `bytes` (a default one for anything else: another
    /// mind's bytes, an older version, nothing).
    pub fn decode(bytes: &[u8]) -> Memory {
        Self::read(bytes).unwrap_or_default()
    }

    fn read(bytes: &[u8]) -> Option<Memory> {
        let mut r = Reader(bytes);
        if r.u8()? != MEMORY_VERSION {
            return None;
        }
        let (style, aim, posture) = (r.opt()?, r.opt()?, r.opt()?);
        let lock = if r.u8()? == 1 {
            let round = r.u32()?;
            let mut situation = [0; fe::N];
            for v in situation[..fe::BASE_N].iter_mut() {
                *v = r.i64()?;
            }
            Some(StyleLock { round, situation })
        } else {
            None
        };
        let held = Held { style, aim, posture, lock };
        let has_target = r.u8()? == 1;
        let target = r.u32()?;
        let target = has_target.then_some(TechId(target as u16));
        let survey = if r.u8()? == 1 {
            let round = r.u32()?;
            let cities = r.u32()?;
            let mut values = [0; features::SURVEYED];
            for v in values.iter_mut() {
                *v = r.i64()?;
            }
            Some(features::Survey { round, cities, values })
        } else {
            None
        };
        let review = if r.u8()? == 1 {
            let round = r.u32()?;
            let cities = r.u32()?;
            let at_war = r.u8()? == 1;
            let food = r.i64()?;
            let n = r.u32()? as usize;
            let mut goals = Vec::with_capacity(n.min(16));
            for _ in 0..n {
                let kind = r.u8()?;
                let a = r.i64()?;
                let b = r.i64()?;
                let kind = match kind {
                    0 => GoalKind::Wonder { building: BuildingKindId(a as u16), city: b as u64 },
                    1 => GoalKind::Building(BuildingKindId(a as u16)),
                    2 => GoalKind::Branch(PolicyBranchId(a as u16)),
                    _ => GoalKind::Tech(TechId(a as u16)),
                };
                let worth = r.i64()?;
                let rate = r.i64()?;
                let turns = r.i64()?;
                let len = r.u32()? as usize;
                let mut path = Vec::with_capacity(len.min(64));
                for _ in 0..len {
                    path.push(TechId(r.u32()? as u16));
                }
                goals.push(Goal { kind, worth, rate, turns, path });
            }
            let n = r.u32()? as usize;
            let mut growth = Vec::with_capacity(n.min(64));
            for _ in 0..n {
                let id = r.i64()? as u64;
                let mut g = [0; 6];
                for v in g.iter_mut() {
                    *v = r.i64()?;
                }
                growth.push((id, g));
            }
            Some(Review { round, cities, at_war, food, goals, growth })
        } else {
            None
        };
        // `retake2`'s section: its marker, a length and its body; one that does not read is none.
        let retake = if r.0.first() == Some(&RETAKE_SECTION) {
            r.u8()?;
            let len = usize::from(u16::from_le_bytes(r.take::<2>()?));
            let (body, rest) = r.0.split_at_checked(len)?;
            r.0 = rest;
            crate::ai::retake::Memory::decode(body)
        } else {
            None
        };
        // The rivals' book behind its marker.
        let mut rival_book = crate::ai::rival_model::Book::default();
        if r.0.first() == Some(&RIVAL_SECTION) {
            // A cut or spoiled book is no book (like a cut `sharp` section): the rest of the memory stays.
            match crate::ai::rival_model::Book::decode(&r.0[1..]) {
                Some((book, read)) => {
                    rival_book = book;
                    r.0 = &r.0[1 + read..];
                }
                None => r.0 = &[],
            }
        }
        // `net2`'s section: its marker, a length and its body; one that does not read is none.
        let net2 = if r.0.first() == Some(&NET2_SECTION) {
            r.u8()?;
            let len = usize::from(u16::from_le_bytes(r.take::<2>()?));
            let (body, rest) = r.0.split_at_checked(len)?;
            r.0 = rest;
            crate::ai::net2::plan::Memory2::decode(body)
        } else {
            None
        };
        // `sharp`'s section: its marker and numbers, the memory's last bytes (`section_len`).
        let sharp = match r.0.first().copied() {
            Some(marker) if section_len(marker) == Some(r.0.len()) => {
                r.u8()?;
                let (ratio, trend) = (r.i64()?, r.i64()?);
                let (rivals, fits) = if marker == NEXT_SECTION { (Some(r.i64()?).filter(|n| *n >= 0), Some(r.i64()?).filter(|f| *f >= 0).map(|f| f == 1)) } else { (None, None) };
                Some(sharp::Memory { ratio, trend, rivals, fits })
            }
            _ => None,
        };
        Some(Memory { held, survey, review, target, sharp, retake, rival_book, net2 })
    }
}

// ---- the turn ----

/// A goal still to pursue: a wonder nobody built, a building some city
/// lacks, a branch not finished, a technology not known.
fn goal_alive(obs: &AIObservation<'_>, rules: &CompiledRuleset, goal: &Goal) -> bool {
    match goal.kind {
        GoalKind::Wonder { building, city } => {
            !obs.built_wonders.contains(&building) && obs.own_cities.iter().any(|c| c.id.0 == city && !c.buildings.contains(&building))
        }
        GoalKind::Building(b) => obs.own_cities.iter().any(|c| !c.buildings.contains(&b)),
        GoalKind::Branch(branch) => !branch_complete(rules, &obs.culture.policies, branch),
        GoalKind::Tech(t) => !obs.economy.techs.contains(&t),
    }
}

/// Every ordinary policy and the finisher of `branch` adopted.
pub fn branch_complete(rules: &CompiledRuleset, policies: &[PolicyId], branch: PolicyBranchId) -> bool {
    rules
        .module
        .policies
        .iter()
        .enumerate()
        .filter(|(_, p)| p.branch == Some(branch) && p.role != crate::rules::content::PolicyRole::Opener)
        .all(|(i, _)| policies.contains(&PolicyId(i as u16)))
}

/// What a turn of the net reads: the observation, the brief, the rules and the allocator's prices.
#[derive(Clone, Copy)]
pub struct Seat<'a, 'o> {
    pub obs: &'a AIObservation<'o>,
    pub brief: &'a crate::ai::planner::TurnBrief,
    pub rules: &'a CompiledRuleset,
    pub prices: &'a Prices,
}

impl Seat<'_, '_> {
    fn cities(&self) -> u32 {
        self.obs.own_cities.len() as u32
    }

    fn pace(&self) -> u32 {
        self.obs.speed.pace_percent.max(1)
    }

    /// `rounds` Standard rounds at the game's pace (at least 1).
    fn paced(&self, rounds: u32) -> u32 {
        (rounds * self.pace() / 100).max(1)
    }
}

/// The situation of a turn (layer 0), the model of the rivals it read and whether the land was surveyed.
#[derive(Debug, Clone)]
pub struct Situation {
    pub features: [Milli; fe::N],
    pub rivals: crate::ai::rival_model::Rivals,
    pub survey_due: bool,
}

/// The net's turn for a nation: the situation, the intent (held by the
/// memory's hysteresis), the prices, and every few rounds the goals; the
/// memory is written back. `prices` are the allocator's of the turn.
/// `stakes`: the leader's in a war (`grand_strategy::Stakes`), read by `war-open` alone.
pub fn think(obs: &AIObservation<'_>, brief: &crate::ai::planner::TurnBrief, state: &mut AiState, rules: &CompiledRuleset, prices: &Prices, stakes: &crate::ai::grand_strategy::Stakes) -> Plan {
    let seat = Seat { obs, brief, rules, prices };
    let mut memory = Memory::decode(&state.mind_memory);
    let situation = situation(seat, state, stakes, &mut memory);
    // `strategist-x`: the rival running away (`threat`), a term of the posture; tall first.
    let parts = brief.strategist;
    let threat = if parts.threat { threat::assess(&situation.rivals) } else { None };
    let net = Net { open: brief.war_open, sharp: brief.sharp, pays: brief.war_pays, threat: threat.map_or(0, |t| t.level), tall_first: parts.tall_first };
    let intent = intent_step(seat, &mut memory, &situation, net, &Tilt::NONE);
    plan_with_intent(seat, state, memory, &situation, intent, &[0; pd::N], wishes::Wishes { threat })
}

/// Layer 0 of a turn: the land survey when due, the features, the model of the rivals (its book
/// sampled when due), all kept in `memory`.
pub fn situation(seat: Seat<'_, '_>, state: &AiState, stakes: &crate::ai::grand_strategy::Stakes, memory: &mut Memory) -> Situation {
    let Seat { obs, brief, rules, prices } = seat;
    let survey_due = survey_step(seat, state, memory);
    let survey = memory.survey.expect("surveyed above");
    let mut features = features::situation(obs, brief, rules, &survey);
    if brief.war_open {
        // `war-open`: what the war machine sees (`features::open`).
        features::open(obs, brief, state, rules, prices, stakes, &mut features);
    }
    if brief.sharp {
        // `sharp`: the features on scales that keep moving and its new inputs (`sharp::features`).
        sharp::features(obs, brief, rules, state, &mut memory.sharp, survey_due, &mut features);
    }
    // The model of the rivals from the public screens (`ai::rival_model`): inputs of every variant's
    // net, read with weight 0 (`weights::rival`); the book samples the screens for the trends.
    let rivals = crate::ai::rival_model::estimate(obs, rules, &memory.rival_book);
    features::rivals(&rivals, &mut features);
    if memory.rival_book.due(obs.round, seat.pace()) {
        memory.rival_book.record(obs.player, &rivals, seat.pace());
    }
    Situation { features, rivals, survey_due }
}

/// The land survey, every `SURVEY_ROUNDS` paced rounds or when the city count changes: whether it ran.
fn survey_step(seat: Seat<'_, '_>, state: &AiState, memory: &mut Memory) -> bool {
    let round = seat.obs.round;
    let cities = seat.cities();
    let due = memory.survey.as_ref().is_none_or(|s| round >= s.round + seat.paced(weights::goal::SURVEY_ROUNDS) || s.cities != cities);
    if due {
        let mut survey = features::survey(seat.obs, seat.rules, state);
        if seat.brief.sharp {
            sharp::survey(seat.obs, state, &mut survey);
        }
        memory.survey = Some(survey);
    }
    due
}

/// Layer 1 of a turn: the intent of the situation with `tilt` on its logits, the style's lock, the
/// tall play; the picks are held in `memory`.
pub fn intent_step(seat: Seat<'_, '_>, memory: &mut Memory, situation: &Situation, net: Net, tilt: &Tilt) -> Intent {
    let obs = seat.obs;
    let round = obs.round;
    let mut intent = intent_tilted(&situation.features, memory.held, seat.cities(), seat.prices.rounds_left, seat.pace(), net, tilt);
    let lock = lock_step(seat, memory.held.lock, &situation.features, &mut intent);
    if let Some(from) = memory.held.style.filter(|s| *s != intent.style_pick) {
        trace::style_change(obs.player.0, round, from, &intent);
    }
    tall_real_step(seat, memory, lock.is_some(), &mut intent);
    memory.held = Held { style: Some(intent.style_pick), aim: Some(intent.aim_pick), posture: Some(intent.posture_pick), lock };
    intent
}

/// The style is locked when the first branch may be opened after the
/// first survey of the surroundings, at the latest `LOCK_MAX_ROUNDS`.
fn lock_step(seat: Seat<'_, '_>, held: Option<StyleLock>, situation: &[Milli; fe::N], intent: &mut Intent) -> Option<StyleLock> {
    if held.is_some() {
        return held;
    }
    let (obs, rules) = (seat.obs, seat.rules);
    let round = obs.round;
    let opened = obs.culture.branches.iter().any(|b| branches::is_normal(rules, *b));
    let may_open = obs.culture.unlockable.iter().any(|b| branches::is_normal(rules, *b));
    if round >= seat.paced(weights::LOCK_MAX_ROUNDS) || round >= seat.paced(weights::LOCK_MIN_ROUNDS) && (opened || may_open) {
        intent.locked = true;
        trace::lock(obs.player.0, round, intent);
        return Some(StyleLock { round, situation: *situation });
    }
    None
}

/// `tall-real`: a tall seat where the science victory fits the game aims at it
/// (`ai::tall_real`); its city count is the rules' size model's.
/// `next`: whether the science victory fits the seat's game, judged once at the lock
/// (`tall_real::science_fits`) and kept in `sharp`'s memory.
fn tall_real_step(seat: Seat<'_, '_>, memory: &mut Memory, locked: bool, intent: &mut Intent) {
    let Seat { obs, brief, rules, prices } = seat;
    let next_fit = brief.next;
    let fits = if next_fit {
        let held = memory.sharp.and_then(|m| m.fits);
        let fits = held.or_else(|| locked.then(|| crate::ai::tall_real::science_fits(obs, rules, prices.rounds_left)));
        if let Some(m) = memory.sharp.as_mut() {
            m.fits = fits;
        }
        Some(fits)
    } else {
        None
    };
    if brief.tall_real && crate::ai::tall_real::plays(obs, rules, intent.style_pick, fits) {
        intent.tall_real = true;
        // `next` values the science branch by the net like any other: forcing it whole first
        // measured no better (pace against `sharp`, offsets 200+201: 64.9 % without, 65.7 % with).
        intent.science_first = !brief.next;
        intent.aim_pick = weights::AIM_SCIENCE;
        let target = crate::ai::tall_real::city_target(obs, rules, prices.rounds_left, next_fit);
        intent.more_cities = target.saturating_sub(seat.cities());
    }
}

/// Layer 2 and the goals of a turn: the prices of the intent with `price_tilt` added, the goals'
/// review when due, the research target, the city target; the memory is written back to `state`.
pub fn plan_with_intent(seat: Seat<'_, '_>, state: &mut AiState, mut memory: Memory, situation: &Situation, intent: Intent, price_tilt: &[Milli; pd::N], wishes: wishes::Wishes) -> Plan {
    let obs = seat.obs;
    let round = obs.round;
    let mut mult = price_mult_tilted(&intent, price_tilt);
    let review_due = review_step(seat, &mut memory, &intent, &mut mult, situation.features[fe::AT_WAR] > 0);
    let review = memory.review.clone().unwrap_or_default();
    if review.food > 0 {
        mult[pd::FOOD] = review.food;
    }
    let target = research_target(obs, memory.target, &review);
    memory.target = target;
    state.mind_memory = memory.encode();
    let city_target = city_target_of(seat, &intent);
    let plan = Plan { round, features: situation.features, intent, mult, goals: review.goals.clone(), target, city_target, growth: review.growth.clone(), war_open: seat.brief.war_open, wishes };
    trace::turn(obs.player.0, &plan, review_due, seat.rules);
    if review_due {
        trace::rivals(obs.player.0, &situation.rivals);
    }
    if review_due {
        trace::diplo(obs, state, &plan.intent);
    }
    plan
}

/// The goals' review, every `REVIEW_ROUNDS` paced rounds, on a change of the city count or of the
/// war, or when a goal is lost: whether it was due.
fn review_step(seat: Seat<'_, '_>, memory: &mut Memory, intent: &Intent, mult: &mut [Milli; pd::N], at_war: bool) -> bool {
    let (obs, rules) = (seat.obs, seat.rules);
    let round = obs.round;
    let cities = seat.cities();
    let lost = memory.review.as_ref().is_some_and(|r| r.goals.iter().any(|g| !goal_alive(obs, rules, g)));
    let review_due = memory.review.as_ref().is_none_or(|r| round >= r.round + seat.paced(weights::goal::REVIEW_ROUNDS) || r.cities != cities || r.at_war != at_war) || lost;
    let base = Base::of(seat.prices);
    if review_due && !obs.own_cities.is_empty() {
        let model = model::Model::of_observation(obs, rules);
        let review = goals::review(&model, rules, intent, mult, &base, round, at_war);
        memory.review = Some(review);
    }
    review_due
}

/// The research target: the held one while its goal stays, else the
/// first goal's; another goal's replaces it when its rate leads by 15 %.
fn research_target(obs: &AIObservation<'_>, held_target: Option<TechId>, review: &Review) -> Option<TechId> {
    let goal_end = |g: &Goal| g.path.last().copied();
    let held = held_target.and_then(|t| review.goals.iter().find(|g| goal_end(g) == Some(t)).filter(|_| !obs.economy.techs.contains(&t)));
    let best = review.goals.iter().filter(|g| goal_end(g).is_some()).max_by_key(|g| g.rate);
    match (held, best) {
        (Some(h), Some(b)) if b.rate * ONE > h.rate * weights::head::KEEP_MARGIN => goal_end(b),
        (Some(h), _) => goal_end(h),
        (None, Some(b)) => goal_end(b),
        (None, None) => None,
    }
}

/// The rules' city model is the ceiling (`brief.tall_cities`: the count with
/// the most science per research cost); wide and conquest play may pass it.
fn city_target_of(seat: Seat<'_, '_>, intent: &Intent) -> u32 {
    let cities = seat.cities();
    let mut city_target = cities + intent.more_cities;
    if let Some(model) = seat.brief.tall_cities.filter(|_| !intent.tall_real) {
        let allowance = (intent.style[1] * weights::CITY_CEILING_WIDE + intent.style[2] * weights::CITY_CEILING_CONQUEST) / ONE / ONE;
        city_target = city_target.min(model + allowance as u32).max(cities);
    }
    city_target
}
