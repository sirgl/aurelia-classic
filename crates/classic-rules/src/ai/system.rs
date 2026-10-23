//! Plug-in point of the per-system AI (PLAN M6 item 7: "each M4 system
//! comes with its AI consumer"). The rules module brings one advisor per
//! game system of its own (`RulesModule::advisors`); the registry asks
//! each one at its moments of an AI turn, from the full observation of that
//! moment, and the driver executes the proposals like any other decision
//! (traced, validated by the core, rejections counted).

use crate::ai::planner::TurnBrief;
use crate::ai::state::AiState;
use crate::command::Command;
use crate::knowledge::observation::AIObservation;
use crate::rules::CompiledRuleset;

/// A game system of the module that brings its own AI decisions.
pub type AdvisorSystem = crate::classic::ClassicSystem;

/// What a system advisor may read: the player's observation, the brief of
/// its turn, the AI memory (read only) and the rules.
pub struct AdvisorInput<'o, 'a> {
    pub observation: &'o AIObservation<'a>,
    pub brief: &'o TurnBrief,
    pub state: &'o AiState,
    pub rules: &'o CompiledRuleset,
}

/// One command a system wants, with its priority (higher first).
#[derive(Debug, Clone)]
pub struct Proposal {
    pub priority: i32,
    pub command: Command,
}

/// The AI of one game system. Implementations are stateless and
/// deterministic: the same input gives the same proposals.
pub trait SystemAdvisor {
    /// Commands before the units move (ranged strikes, city strikes,
    /// stances): from the observation at the start of the turn.
    fn start_of_turn(&self, _input: &AdvisorInput<'_, '_>) -> Vec<Proposal> {
        Vec::new()
    }

    /// Commands after the units moved and the cities chose their
    /// production (policies, beliefs, purchases with faith, upgrades).
    fn end_of_turn(&self, _input: &AdvisorInput<'_, '_>) -> Vec<Proposal> {
        Vec::new()
    }
}

/// When in a turn the advisors are asked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Moment {
    StartOfTurn,
    EndOfTurn,
}

/// One advisor per game system, in system order.
pub struct AdvisorRegistry {
    advisors: Vec<(AdvisorSystem, Box<dyn SystemAdvisor>)>,
}

impl std::fmt::Debug for AdvisorRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "AdvisorRegistry({:?})", self.systems())
    }
}

impl Default for AdvisorRegistry {
    fn default() -> Self {
        Self::standard()
    }
}

impl AdvisorRegistry {
    /// The advisors of the game module.
    pub fn standard() -> Self {
        let mut registry = Self { advisors: Vec::new() };
        for (system, advisor) in crate::classic::advisors::advisors() {
            registry.register(system, advisor);
        }
        registry
    }

    /// Replace the advisor of `system` (a module plugs in here).
    pub fn register(&mut self, system: AdvisorSystem, advisor: Box<dyn SystemAdvisor>) {
        self.advisors.retain(|(s, _)| *s != system);
        self.advisors.push((system, advisor));
        self.advisors.sort_by_key(|(s, _)| *s);
    }

    pub fn systems(&self) -> Vec<AdvisorSystem> {
        self.advisors.iter().map(|(s, _)| *s).collect()
    }

    /// Every proposal of every system, by priority (stable: system order
    /// on a tie).
    pub fn propose_all(
        &self,
        input: &AdvisorInput<'_, '_>,
        moment: Moment,
    ) -> Vec<(AdvisorSystem, Proposal)> {
        let mut out = Vec::new();
        let _moment = prof::span_debug("classic/advisors/", &moment);
        for (system, advisor) in &self.advisors {
            let _span = prof::span_debug("classic/advisor/", system);
            let proposals = match moment {
                Moment::StartOfTurn => advisor.start_of_turn(input),
                Moment::EndOfTurn => advisor.end_of_turn(input),
            };
            for proposal in proposals {
                out.push((*system, proposal));
            }
        }
        out.sort_by_key(|(system, p)| (std::cmp::Reverse(p.priority), *system));
        out
    }
}
