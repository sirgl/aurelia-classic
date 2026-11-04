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

pub mod branches;
pub mod cell;
pub mod effects;
pub mod features;
pub mod goals;
pub mod heads;
pub mod layers;
pub mod model;
pub mod trace;
pub mod weights;

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
}

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
    let mut out = [0; kc::N];
    for (i, neuron) in weights::CONCEPTS.iter().enumerate() {
        out[i] = fire(neuron, features, &out);
    }
    out
}

/// Layer 1: the intent of a situation; `held` the choices of the last
/// turn, `cities` the cities now, `rounds_left` and `pace` (percent) the
/// game's.
pub fn intent(features: &[Milli; fe::N], held: Held, cities: u32, rounds_left: u32, pace: u32) -> Intent {
    let concepts = concepts(features);
    let logits = |table: &[weights::Neuron]| table.iter().map(|n| fire(n, features, &concepts)).collect::<Vec<_>>();
    // A locked style is the lock's situation with the turn's events.
    let (style, margin) = match &held.lock {
        Some(lock) => {
            let mut at_lock = lock.situation;
            for e in weights::STYLE_EVENTS {
                at_lock[e] = features[e];
            }
            let lock_concepts = self::concepts(&at_lock);
            let style_logits: Vec<Milli> = weights::STYLE.iter().map(|n| fire(n, &at_lock, &lock_concepts)).collect();
            let margin = if survival(features, &concepts) { weights::HYSTERESIS } else { weights::STYLE_LOCK };
            (softmax(&<[Milli; 3]>::try_from(style_logits).expect("three styles")), margin)
        }
        None => (softmax(&<[Milli; 3]>::try_from(logits(&weights::STYLE)).expect("three styles")), weights::HYSTERESIS),
    };
    let aim = softmax(&<[Milli; 4]>::try_from(logits(&weights::AIM)).expect("four aims"));
    let posture = softmax(&<[Milli; 4]>::try_from(logits(&weights::POSTURE)).expect("four postures"));
    let style_pick = hold(&style, held.style, margin);
    let aim_pick = hold(&aim, held.aim, weights::HYSTERESIS);
    let posture_pick = hold(&posture, held.posture, weights::HYSTERESIS);
    // More cities: each style's rows weighed by its share.
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
    // The horizon.
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
    let h = (h / ONE).clamp(weights::HORIZON_MIN, weights::HORIZON_MAX);
    let horizon = (h * i64::from(pace.max(1)) / 100).clamp(1, i64::from(rounds_left.max(1))) as u32;
    Intent { concepts, style, aim, posture, style_pick, aim_pick, posture_pick, more_cities: more_cities as u32, horizon, locked: held.lock.is_some() }
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
    let rows = rows(intent);
    let mut mult = [ONE; pd::N];
    for (row, price, w, _) in weights::PRICE_ROWS {
        mult[*price] += rows[*row] * w / ONE * weights::PRICE_GAIN / ONE;
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
                for v in l.situation {
                    w.i64(v);
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
            for v in situation.iter_mut() {
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
        Some(Memory { held, survey, review, target })
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

/// The net's turn for a nation: the situation, the intent (held by the
/// memory's hysteresis), the prices, and every few rounds the goals; the
/// memory is written back. `prices` are the allocator's of the turn.
pub fn think(obs: &AIObservation<'_>, brief: &crate::ai::planner::TurnBrief, state: &mut AiState, rules: &CompiledRuleset, prices: &Prices) -> Plan {
    let mut memory = Memory::decode(&state.mind_memory);
    let round = obs.round;
    let cities = obs.own_cities.len() as u32;
    let pace = obs.speed.pace_percent.max(1);
    let paced = |rounds: u32| (rounds * pace / 100).max(1);
    let survey_due = memory.survey.as_ref().is_none_or(|s| round >= s.round + paced(weights::goal::SURVEY_ROUNDS) || s.cities != cities);
    if survey_due {
        memory.survey = Some(features::survey(obs, rules, state));
    }
    let survey = memory.survey.expect("surveyed above");
    let situation = features::situation(obs, brief, rules, &survey);
    let rounds_left = prices.rounds_left;
    let mut intent = intent(&situation, memory.held, cities, rounds_left, pace);
    // The style is locked when the first branch may be opened after the
    // first survey of the surroundings, at the latest `LOCK_MAX_ROUNDS`.
    let mut lock = memory.held.lock;
    if lock.is_none() {
        let opened = obs.culture.branches.iter().any(|b| branches::is_normal(rules, *b));
        let may_open = obs.culture.unlockable.iter().any(|b| branches::is_normal(rules, *b));
        if round >= paced(weights::LOCK_MAX_ROUNDS) || round >= paced(weights::LOCK_MIN_ROUNDS) && (opened || may_open) {
            lock = Some(StyleLock { round, situation });
            intent.locked = true;
            trace::lock(obs.player.0, round, &intent);
        }
    }
    if let Some(from) = memory.held.style.filter(|s| *s != intent.style_pick) {
        trace::style_change(obs.player.0, round, from, &intent);
    }
    memory.held = Held { style: Some(intent.style_pick), aim: Some(intent.aim_pick), posture: Some(intent.posture_pick), lock };
    let mut mult = price_mult(&intent);
    let at_war = situation[fe::AT_WAR] > 0;
    let lost = memory.review.as_ref().is_some_and(|r| r.goals.iter().any(|g| !goal_alive(obs, rules, g)));
    let review_due = memory.review.as_ref().is_none_or(|r| round >= r.round + paced(weights::goal::REVIEW_ROUNDS) || r.cities != cities || r.at_war != at_war) || lost;
    let base = Base::of(prices);
    if review_due && !obs.own_cities.is_empty() {
        let model = model::Model::of_observation(obs, rules);
        let review = goals::review(&model, rules, &intent, &mut mult, &base, round, at_war);
        memory.review = Some(review);
    }
    let review = memory.review.clone().unwrap_or_default();
    if review.food > 0 {
        mult[pd::FOOD] = review.food;
    }
    // The research target: the held one while its goal stays, else the
    // first goal's; another goal's replaces it when its rate leads by 15 %.
    let goal_end = |g: &Goal| g.path.last().copied();
    let held = memory.target.and_then(|t| review.goals.iter().find(|g| goal_end(g) == Some(t)).filter(|_| !obs.economy.techs.contains(&t)));
    let best = review.goals.iter().filter(|g| goal_end(g).is_some()).max_by_key(|g| g.rate);
    let target = match (held, best) {
        (Some(h), Some(b)) if b.rate * ONE > h.rate * weights::head::KEEP_MARGIN => goal_end(b),
        (Some(h), _) => goal_end(h),
        (None, Some(b)) => goal_end(b),
        (None, None) => None,
    };
    memory.target = target;
    state.mind_memory = memory.encode();
    // The rules' city model is the ceiling (`brief.tall_cities`: the count with
    // the most science per research cost); wide and conquest play may pass it.
    let mut city_target = cities + intent.more_cities;
    if let Some(model) = brief.tall_cities {
        let allowance = (intent.style[1] * weights::CITY_CEILING_WIDE + intent.style[2] * weights::CITY_CEILING_CONQUEST) / ONE / ONE;
        city_target = city_target.min(model + allowance as u32).max(cities);
    }
    let plan = Plan { round, features: situation, intent, mult, goals: review.goals.clone(), target, city_target, growth: review.growth.clone() };
    trace::turn(obs.player.0, &plan, review_due, rules);
    plan
}
