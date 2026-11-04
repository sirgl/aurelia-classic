//! AI (platform/structure.md §23, platform/project.md §28, PLAN M6): reads only the player's
//! observation, writes only commands, keeps its memory in `state::AiState`.
//!
//! - `state` — the saved memory (strategy, unit tasks, reservations,
//!   relations, seen cities);
//! - `driver` — the step-wise turn machine with deterministic budgets;
//! - `planner*` — observation → decisions (map, strategy, research, cities,
//!   settling, workers, units); `tactics` — combat choices; `front` —
//!   the coordinated combat (classic/docs/ai-tactics.md); `operations` — the war
//!   armies (classic/docs/ai-operations.md);
//! - `battlefield`, `roles`, `plan_search`, `tactical` — the tactical layer of
//!   the variant `tactical`: a battlefield model with a threat map, roles by
//!   unit characteristics, a two-ply beam search and one plan a turn
//!   (classic/docs/ai-tactical.md);
//! - `value`, `allocator` — the score-valued spending of production and
//!   gold (classic/docs/ai-allocator.md);
//! - `grand_strategy`, `taste` — the leaders' paths to victory and their
//!   choice among near-equal options (`leaders` and the game's AI,
//!   `standard`, classic/docs/ai-leaders.md); `peace` — their wars end by deals
//!   valued in the same measure; `capital_search` — where their scouts
//!   look for the rival capitals they value;
//! - `defence` — the cities in danger and the answers worth their loss
//!   (the game's AI, `standard`, classic/docs/ai-defence.md);
//! - `legal` — every order passes the rules' own check before it is chosen
//!   and again when it is sent (classic/docs/ai-legal-orders.md);
//! - `strikes`, `carrier` — missiles, nuclear weapons and carriers of the
//!   variants `strikes` and `heavy` (classic/docs/ai-strikes.md);
//! - `system` — the plug-in point of the per-system advisors (M4 mechanics);
//! - `trace` — reasons and counters;
//! - `net` — the one integer inference of the learned nets (`danger_net`,
//!   `site_net`: a site's population at +50, classic/docs/ai-site.md;
//!   `choice_net`: what a city builds and what a player researches, by
//!   `choice`'s features of the allocator's best candidates,
//!   classic/docs/ai-choice.md);
//! - `variant` — named versions of the AI: the game's `standard` and the
//!   others the AI bench seats against it (platform/docs/ai-bench.md);
//! - `greedy` — the old greedy planner, a command script of pinned tests;
//! - `citizens`, `improvements`, `great_people` — economy decisions (M4-A,
//!   see README.md).
//! - `combat`, `diplomacy`, `barbarians`, `city_states`, `congress`, `espionage`, `victory` — decision functions of the M4
//!   mechanics over the observation (README.md lists them).

pub mod air;
pub mod allocator;
pub mod army_fit;
pub mod barbarians;
pub mod books;
pub mod battlefield;
pub mod camps;
pub mod capital_search;
pub mod carrier;
pub mod choice;
pub mod choice_net;
#[cfg(feature = "experimental-ai")]
mod choice_weights;
pub mod citizens;
pub mod city_states;
pub mod combat;
pub mod congress;
pub mod conquest;
pub mod danger;
pub mod danger_net;
mod danger_weights;
pub mod defence;
pub mod diplomacy;
pub mod espionage;
pub mod driver;
pub mod fleet;
pub mod genome;
pub mod front;
pub mod economy;
pub mod effects;
pub mod grand_strategy;
pub mod grants;
pub mod great_people;
pub mod greedy;
pub mod happiness;
pub mod improvements;
pub mod infamy;
pub mod legal;
pub mod net;
#[cfg(feature = "experimental-ai")]
pub mod net_e2e;
pub mod operations;
pub mod pantheon;
pub mod peace;
pub mod plan_search;
pub mod planner;
pub mod planner_cities;
pub mod planner_map;
pub mod planner_research;
pub mod planner_settle;
pub mod planner_strategy;
pub mod planner_units;
pub mod planner_value;
pub mod planner_workers;
pub mod raid;
pub mod policies;
pub mod policy_branches;
pub mod promo_fit;
pub mod religion;
pub mod religion_fix;
pub mod roles;
pub mod science_levers;
pub mod scouting;
pub mod sea_work;
pub mod siege_tactics;
pub mod siege_walls;
pub mod site;
pub mod site_net;
#[cfg(feature = "experimental-ai")]
mod site_weights;
pub mod territory;
pub mod tourism;
pub mod trade;
pub mod unciv;
pub mod unit_abilities;
pub mod war_probe;
pub use kernel::ai::{state, AiPlayers};
pub mod strategy_net;
#[cfg(feature = "experimental-ai")]
pub mod unit_net;
pub mod strikes;
pub mod system;
pub mod taste;
pub mod tactical;
pub mod tactics;
pub mod tall;
pub mod trace;
pub mod value;
pub mod variant;
pub mod victory;
pub mod weak_fighter;

pub use driver::{AiDriver, DriverPhase, StepOutcome};
pub use greedy::{
    diplomacy_commands, war_declarations, GreedyPlanner, NullPlanner, Planner,
    PEACE_AFTER_UNSEEN_ROUNDS,
};
pub use planner::{AiSettings, DecisionBudget};
pub use state::{AiState, AiStates, Strategy, UnitTask};
