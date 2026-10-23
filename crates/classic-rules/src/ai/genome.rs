//! The genome of the heuristic AI (classic/docs/ai-fitness.md, «Genome»): a small,
//! named set of the AI's own numeric weights and thresholds that a
//! development tool can override for one mind, so an optimiser can tune
//! them against the bench's fitness.
//!
//! Dev-only and off by default. Without a genome every knob reads the
//! constant it is named after (the default below *is* that constant), so
//! the game plays exactly as before: no genome, no change, the same world
//! hashes. A mind that carries a genome ([`ClassicMind::with_genome`]) makes it
//! the active one for the span of each of its own decisions and puts the
//! previous one back afterwards, so a game can seat minds with different
//! genomes (`kernel::ai::seats::Seated`) and the minds of other games on
//! other threads are never touched.
//!
//! What the knobs are not: a price, a cap or a multiplier of the rules for
//! the AI. They are the AI's valuations and patience (how long a plan is
//! kept, how a policy branch is weighed, what a tile or a technology is
//! worth to the allocator); the rules every player plays under are the
//! same. No knob names a technology, a building, a belief or a
//! civilization.
//!
//! [`ClassicMind::with_genome`]: crate::ai::driver::ClassicMind::with_genome

use std::cell::Cell;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use crate::ai::{
    allocator, defence, grand_strategy, operations, planner_settle, policy_branches, taste, value,
};

/// One tunable number of the AI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Spec {
    pub knob: Knob,
    /// The JSON key.
    pub name: &'static str,
    /// The constant the game plays with.
    pub default: i64,
    /// The range an optimiser may search; a genome is clamped into it.
    pub min: i64,
    pub max: i64,
    pub group: &'static str,
    /// What it is, in a line.
    pub doc: &'static str,
}

macro_rules! knobs {
    ($( $variant:ident, $name:literal, $group:literal, $default:expr, $min:literal, $max:literal, $doc:literal; )+) => {
        /// The knobs, in the order of [`SPECS`].
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum Knob { $( $variant ),+ }

        /// Every knob with its default, range and meaning.
        pub const SPECS: &[Spec] = &[
            $( Spec { knob: Knob::$variant, name: $name, default: $default as i64, min: $min, max: $max, group: $group, doc: $doc } ),+
        ];
    };
}

knobs! {
    BandPercent, "band_percent", "leaders", taste::BAND_PERCENT, 0, 30,
        "options within this percent of the best value are tied; a leader takes the one it loves";
    SwitchPercent, "switch_percent", "leaders", grand_strategy::SWITCH_PERCENT, 100, 220,
        "another victory path replaces the current one only when worth this percent of it";
    ReviewRounds, "review_rounds", "leaders", grand_strategy::REVIEW_ROUNDS, 4, 30,
        "rounds between two reviews of the victory path";
    PathYieldPercent, "path_yield_percent", "leaders", grand_strategy::PATH_YIELD_PERCENT, 100, 300,
        "price of the chosen path's yield, percent";
    ScorePower, "score_power", "leaders", grand_strategy::SCORE_POWER, 1, 8,
        "power of the scores in the chance of the time victory (a lead counts more than its share)";
    PlanLimit, "plan_limit", "war", operations::PLAN_LIMIT, 10, 90,
        "rounds a war plan waits for its treasury chest";
    PlanKeepPercent, "plan_keep_percent", "war", operations::PLAN_KEEP_PERCENT, 30, 100,
        "a war plan is dropped when its target is worth less than this percent of its value at the choice";
    PlanSwitchPercent, "plan_switch_percent", "war", operations::PLAN_SWITCH_PERCENT, 100, 250,
        "another war target replaces the plan only when worth this percent of it";
    SuccessPercent, "success_percent", "war", operations::SUCCESS_PERCENT, 10, 80,
        "expected chance an operation takes its city, percent (war appetite)";
    LossPercent, "loss_percent", "war", operations::LOSS_PERCENT, 20, 100,
        "share of the army an operation expects to lose, percent";
    RivalPercent, "rival_percent", "war", operations::RIVAL_PERCENT, 0, 100,
        "share of a captured city's points that counts against the best rival, percent";
    StrangerPercent, "stranger_percent", "war", defence::STRANGER_PERCENT, 0, 100,
        "how hostile an untrusted stranger's army is taken to be in a danger estimate, percent";
    MaxSettlers, "max_settlers", "economy", allocator::MAX_SETTLERS, 1, 6,
        "most settlers the allocator keeps wanted at once";
    JobsPerWorker, "jobs_per_worker", "economy", allocator::JOBS_PER_WORKER, 2, 8,
        "tile jobs one worker is counted to serve";
    ImprovementYields, "improvement_yields", "economy", allocator::IMPROVEMENT_YIELDS, 50, 400,
        "yield a finished tile job adds, as percent of the price of a food or hammer";
    WonderMinChance, "wonder_min_chance", "economy", allocator::WONDER_MIN_CHANCE, 0, 100,
        "lowest chance, percent, a wonder is still counted ours however long it takes";
    FirstWorkerFactor, "first_worker_factor", "economy", allocator::FIRST_WORKER_FACTOR, 1, 6,
        "how much the first worker counts once a settler is on the way";
    TilePrice, "tile_price", "prices", value::TILE_PRICE, 30, 200,
        "gold a bought tile is taken to cost on average";
    CitizenRounds, "citizen_rounds", "prices", value::CITIZEN_ROUNDS, 10, 60,
        "rounds a new city is taken to need for one more citizen";
    OpeningCities, "opening_cities", "prices", value::OPENING_CITIES, 1, 6,
        "cities of the opening: their citizens are valued as the empire's growth";
    HappinessComfort, "happiness_comfort", "prices", value::HAPPINESS_COMFORT, 2, 12,
        "excess happiness at which one more point stops being worth a citizen";
    TechRounds, "tech_rounds", "prices", value::TECH_ROUNDS, 4, 20,
        "rounds one technology is taken to take, when valuing what research buys";
    SettleTravelCost, "settle_travel_cost", "settling", planner_settle::TRAVEL_COST, 10, 80,
        "points a settler's turn of walking costs a candidate site";
    SettleDistancePenalty, "settle_distance_penalty", "settling", planner_settle::DISTANCE_PENALTY, 10, 80,
        "points per tile a site costs beyond the free distance from the empire's cities";
    SettleFreeDistance, "settle_free_distance", "settling", planner_settle::FREE_DISTANCE, 2, 7,
        "tiles from the nearest own city a site is free of the distance penalty";
    BranchExpansionPercent, "branch_expansion_percent", "policies", policy_branches::EXPANSION_PERCENT, 100, 800,
        "the wish for expansion when free sites are in reach, percent (weighs a policy branch)";
    BranchMilitaryAtWar, "branch_military_at_war", "policies", policy_branches::MILITARY_AT_WAR, 0, 900,
        "percent the wish for an army adds while at war (weighs a policy branch)";
    BranchHappinessPerCity, "branch_happiness_per_city", "policies", policy_branches::HAPPINESS_PER_CITY, 0, 100,
        "percent the wish for happiness grows with each city beyond the free ones";
    BranchSettlerPoints, "branch_settler_points", "policies", policy_branches::SETTLER, 20, 150,
        "points a free settler is worth in a policy branch";
    BranchTechnologyPoints, "branch_technology_points", "policies", policy_branches::TECHNOLOGY, 20, 150,
        "points a free technology is worth in a policy branch";
}

/// Number of knobs.
pub const COUNT: usize = SPECS.len();

type Values = [i64; COUNT];

// The active genome of this thread: the values of the mind that is
// deciding, or none.
thread_local! {
    static ACTIVE: Cell<Option<&'static Values>> = const { Cell::new(None) };
}

/// The value of a knob: the active genome's, else the constant it is named
/// after.
#[inline]
pub fn get(knob: Knob) -> i64 {
    ACTIVE.with(|active| match active.get() {
        Some(values) => values[knob as usize],
        None => SPECS[knob as usize].default,
    })
}

/// The knob as the unsigned count the constant it replaces was.
#[inline]
pub fn get_u32(knob: Knob) -> u32 {
    get(knob) as u32
}

#[inline]
pub fn get_i32(knob: Knob) -> i32 {
    get(knob) as i32
}

#[inline]
pub fn get_usize(knob: Knob) -> usize {
    get(knob) as usize
}

/// A set of knob values (cheap to copy: the values live for the whole
/// process; a tool makes a handful of genomes).
#[derive(Debug, Clone, Copy)]
pub struct Genome(&'static Values);

impl Genome {
    /// The defaults with `pairs` (key, value) overriding; values are
    /// rounded and clamped into each knob's range. An unknown key is an
    /// error, so a typo never silently tunes nothing.
    pub fn from_pairs(pairs: &[(String, f64)]) -> Result<Genome, String> {
        let mut values: Values = std::array::from_fn(|i| SPECS[i].default);
        for (key, raw) in pairs {
            let spec = SPECS
                .iter()
                .find(|s| s.name == key)
                .ok_or_else(|| format!("unknown knob {key}"))?;
            if !raw.is_finite() {
                return Err(format!("knob {key} is not a finite number"));
            }
            values[spec.knob as usize] = (raw.round() as i64).clamp(spec.min, spec.max);
        }
        Ok(Genome(Box::leak(Box::new(values))))
    }

    /// The value of one knob in this genome.
    pub fn value(&self, knob: Knob) -> i64 {
        self.0[knob as usize]
    }

    /// Makes this genome the active one until the returned scope ends.
    pub fn enter(self) -> Scope {
        Scope {
            previous: ACTIVE.with(|active| active.replace(Some(self.0))),
        }
    }

    /// The knobs that differ from the defaults, as (name, value).
    pub fn changes(&self) -> Vec<(&'static str, i64)> {
        SPECS
            .iter()
            .filter(|s| self.0[s.knob as usize] != s.default)
            .map(|s| (s.name, self.0[s.knob as usize]))
            .collect()
    }
}

/// Puts the previously active genome back when dropped.
#[derive(Debug)]
pub struct Scope {
    previous: Option<&'static Values>,
}

impl Drop for Scope {
    fn drop(&mut self) {
        ACTIVE.with(|active| active.set(self.previous));
    }
}

/// Genomes by handle: how a bench names the mind of a variant that carries
/// one (`standard@handle`, `ClassicBench::mind`).
fn registry() -> &'static Mutex<HashMap<String, Genome>> {
    static REGISTRY: OnceLock<Mutex<HashMap<String, Genome>>> = OnceLock::new();
    REGISTRY.get_or_init(Default::default)
}

/// Remember `genome` under `handle` for the rest of the process.
pub fn register(handle: &str, genome: Genome) {
    registry()
        .lock()
        .expect("the genome registry is never held across a panic")
        .insert(handle.to_owned(), genome);
}

/// The genome registered under `handle`.
pub fn lookup(handle: &str) -> Option<Genome> {
    registry()
        .lock()
        .expect("the genome registry is never held across a panic")
        .get(handle)
        .copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_is_in_the_order_of_the_enum_with_unique_names_and_defaults_inside_ranges() {
        for (i, spec) in SPECS.iter().enumerate() {
            assert_eq!(spec.knob as usize, i, "{} is out of order", spec.name);
            assert!(
                spec.min <= spec.default && spec.default <= spec.max,
                "{} default {} outside {}..{}",
                spec.name,
                spec.default,
                spec.min,
                spec.max
            );
            assert_eq!(
                SPECS.iter().filter(|s| s.name == spec.name).count(),
                1,
                "{} twice",
                spec.name
            );
        }
        assert!(COUNT <= 30, "a genome stays small");
    }

    #[test]
    fn without_a_genome_a_knob_is_its_constant_and_a_scope_restores_it() {
        assert_eq!(get(Knob::PlanLimit), operations::PLAN_LIMIT as i64);
        let genome = Genome::from_pairs(&[
            ("plan_limit".to_owned(), 55.4),
            ("band_percent".to_owned(), 999.0),
        ])
        .expect("known knobs");
        {
            let _scope = genome.enter();
            assert_eq!(get(Knob::PlanLimit), 55);
            assert_eq!(get(Knob::BandPercent), 30, "clamped into the range");
            assert_eq!(get(Knob::SwitchPercent), grand_strategy::SWITCH_PERCENT);
        }
        assert_eq!(get(Knob::PlanLimit), operations::PLAN_LIMIT as i64);
    }

    #[test]
    fn an_unknown_key_is_an_error() {
        assert!(Genome::from_pairs(&[("plan_limt".to_owned(), 1.0)]).is_err());
    }
}
