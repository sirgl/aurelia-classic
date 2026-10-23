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
//! - `variant` — named versions of the AI: the game's `standard` and the
//!   others the AI bench seats against it (platform/docs/ai-bench.md);
//! - `greedy` — the old greedy planner, a command script of pinned tests;
//! - `citizens`, `improvements`, `great_people` — economy decisions (M4-A,
//!   see README.md).
//! - `combat`, `diplomacy`, `barbarians`, `city_states`, `congress`, `espionage`, `victory` — decision functions of the M4
//!   mechanics over the observation (README.md lists them).

pub mod air;
pub mod allocator;
pub mod barbarians;
pub mod capital_search;
pub mod carrier;
pub mod citizens;
pub mod city_states;
pub mod combat;
pub mod congress;
pub mod defence;
pub mod diplomacy;
pub mod espionage;
pub mod driver;
pub mod fleet;
pub mod genome;
pub mod front;
pub mod economy;
pub mod grand_strategy;
pub mod grants;
pub mod great_people;
pub mod greedy;
pub mod happiness;
pub mod improvements;
pub mod infamy;
pub mod legal;
pub mod operations;
pub mod pantheon;
pub mod peace;
pub mod planner;
pub mod planner_cities;
pub mod planner_map;
pub mod planner_research;
pub mod planner_settle;
pub mod planner_strategy;
pub mod planner_units;
pub mod planner_value;
pub mod planner_workers;
pub mod policies;
pub mod policy_branches;
pub mod religion;
pub mod scouting;
pub mod sea_work;
pub mod tourism;
pub mod trade;
pub use kernel::ai::{state, AiPlayers};
pub mod strikes;
pub mod system;
pub mod taste;
pub mod tactics;
pub mod trace;
pub mod value;
pub mod variant;
pub mod victory;

pub use driver::{AiDriver, DriverPhase, StepOutcome};
pub use greedy::{
    diplomacy_commands, war_declarations, GreedyPlanner, NullPlanner, Planner,
    PEACE_AFTER_UNSEEN_ROUNDS,
};
pub use planner::{AiSettings, DecisionBudget};
pub use state::{AiState, AiStates, Strategy, UnitTask};
