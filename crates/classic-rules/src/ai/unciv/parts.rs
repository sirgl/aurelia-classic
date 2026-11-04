// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
//
// Derived from Unciv (https://github.com/yairm210/Unciv), tag 4.22.10, MPL-2.0: the
// families of core/src/com/unciv/logic/automation/ the sibling files port, played one by
// one inside the classic AI.

//! Unciv's modules inside `standard` (`UncivPlay::Parts`, the variants
//! `std-unciv-*`, classic/docs/ai-unciv.md "Unciv's modules inside standard"):
//! one decision family of Unciv's AI in place of `standard`'s, everything else
//! `standard`'s unchanged. A family is the orders it gives (`strips` drops
//! `standard`'s, `opening` and `end_of_turn` add Unciv's) and the units it
//! commands (`commands`, `decide`):
//!
//! | Family | Unciv's | `standard`'s orders dropped |
//! |---|---|---|
//! | research | `civ::research` (the free technology too) | `ChooseTechnology`, `PlanResearch`, `ChooseFreeTechnology` |
//! | policies | `civ::adopt_policy` (an ideology: `ai::policies::policy_command`) | `AdoptPolicy`, `UnlockPolicyBranch`, `ChooseIdeology` |
//! | settle | the settlers (`civilian::automate_settler`), `construction::train_settler` | the settlers' orders; the allocator wants no settler (`TurnBrief::unciv`) |
//! | construction | `construction::automate_cities`, trade units by `ai::trade` (as `unciv`) | production orders but the settlers' and the system advisors' |
//! | workers | workers and work boats (`worker::automate_worker`) | their orders |
//! | gold | `gold::use_gold` | `HurryProduction`, `PurchaseTile`, `GiftGold`, `BuyoutCityState`, `UpgradeUnit`; the allocator buys nothing (`TurnBrief::unciv`) |
//! | religion | `religion::spend_faith`, `choose_beliefs`, the prophets, missionaries and inquisitors | `FoundPantheon`, `AddReformationBelief`, `PurchaseWithFaith`, those units' orders |
//!
//! The units Unciv commands see as far as in `unciv` (`UNCIV_RADIUS`); every
//! other unit keeps `standard`'s sight and order.
//!
//! The proof that a hybrid is `standard` but for its family: with `DRY_ENV` set
//! the families decide as usual (the turn's sight, the forces seen, the path
//! searches) and their decisions are dropped; the games' hashes are then
//! `standard`'s (classic/docs/ai-unciv.md "Unciv's modules inside standard").

use crate::ai::planner::{Decision, PlanTools, UnitView};
use crate::ai::trace::{Actor, Reason};
use crate::command::Command;
use crate::ids::{UnitHandle, UnitKindId};
use crate::knowledge::observation::AIObservation;
use crate::knowledge::public::PlayerQueries;
use crate::rules::CompiledRuleset;
use crate::world::player::{ClassicKind, PlayerKind};

use super::sight::{Kind, Unit};
use super::{civ, civilian, construction, diplomacy, gold, personality, religion, worker, Memo, UncivMind};

/// Dev-only: a hybrid whose families decide but whose decisions are dropped
/// (`standard` plays everything): the proof of the plumbing by result hash.
pub const DRY_ENV: &str = "AURELIA_UNCIV_DRY";

/// The decision families of Unciv's AI a hybrid plays inside `standard`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct UncivParts {
    /// Research: the technology when the queue is empty, the free technology.
    pub research: bool,
    /// Policy branches and policies.
    pub policies: bool,
    /// Where the settlers found cities and when the cities train settlers.
    pub settle: bool,
    /// What the cities build (the settlers aside).
    pub construction: bool,
    /// The workers' and work boats' jobs.
    pub workers: bool,
    /// The gold: purchases, upgrades, city-state gifts, tiles.
    pub gold: bool,
    /// Faith, beliefs and the religious units.
    pub religion: bool,
}

impl UncivParts {
    pub const NONE: UncivParts = UncivParts { research: false, policies: false, settle: false, construction: false, workers: false, gold: false, religion: false };

    pub fn any(self) -> bool {
        self != UncivParts::NONE
    }

    /// How many families.
    pub fn count(self) -> usize {
        [self.research, self.policies, self.settle, self.construction, self.workers, self.gold, self.religion].into_iter().filter(|b| *b).count()
    }
}

impl UncivMind {
    /// The families this mind carries inside `standard`.
    pub fn parts(&self) -> UncivParts {
        self.play.parts()
    }

    /// Dev-only: the families decide, `standard` plays (as with `DRY_ENV`).
    pub fn set_dry(&mut self) {
        self.dry = self.parts().any();
    }

    /// The families whose decisions are played: none in a dry run (`DRY_ENV`).
    pub fn played_parts(&self) -> UncivParts {
        if self.dry { UncivParts::NONE } else { self.parts() }
    }

    /// The unit's sight is `unciv`'s (`UNCIV_RADIUS`): a family commands it.
    pub fn part_sight(&self, rules: &CompiledRuleset, kind: UnitKindId) -> bool {
        !self.dry && self.part_commands(rules, kind)
    }

    /// Does one of the families command a unit of this kind (a settler, a
    /// worker or work boat, a religious unit; never a military unit)?
    pub fn part_commands(&self, rules: &CompiledRuleset, kind: UnitKindId) -> bool {
        let parts = self.parts();
        let Some(def) = rules.units.get(kind.0 as usize) else {
            return false;
        };
        if !parts.any() || (Kind { rules, kind }).is_military() {
            return false;
        }
        (parts.settle && def.founder) || (parts.workers && def.builder) || (parts.religion && religion::religious_kind(rules, kind))
    }

    /// A decision of `standard`'s that the families replace: an order of a
    /// family's own (research, policy, purchase, faith, production) or of a
    /// unit a family commands.
    /// `kind_of` gives the kind of an own unit.
    pub fn strips(&self, rules: &CompiledRuleset, kind_of: &dyn Fn(UnitHandle) -> Option<UnitKindId>, decision: &Decision) -> bool {
        let parts = self.parts();
        let Some(command) = decision.command.as_ref().filter(|_| parts.any() && !self.dry) else {
            return false;
        };
        use Command as C;
        match command {
            C::ChooseTechnology { .. } | C::PlanResearch { .. } | C::ChooseFreeTechnology { .. } => return parts.research,
            C::AdoptPolicy { .. } | C::UnlockPolicyBranch { .. } | C::ChooseIdeology { .. } => return parts.policies,
            C::HurryProduction { .. } | C::PurchaseTile { .. } | C::GiftGold { .. } | C::BuyoutCityState { .. } | C::UpgradeUnit { .. } => return parts.gold,
            C::FoundPantheon { .. } | C::AddReformationBelief { .. } | C::PurchaseWithFaith { .. } => return parts.religion,
            // The settlers stay the allocator's, the advisors' orders (the World Congress's
            // process, the religion's shrine) theirs.
            C::EnqueueProduction { .. } | C::ClearProduction { .. } | C::SetProcess { .. } => {
                return parts.construction && !matches!(decision.reason, Reason::ProduceSettler | Reason::SystemAdvisor);
            }
            _ => {}
        }
        let Some(unit) = crate::knowledge::actions::acting_unit(command) else {
            return false;
        };
        kind_of(unit).is_some_and(|kind| self.part_commands(rules, kind))
    }

    /// The families' orders at the start of the turn, in `unciv`'s order
    /// (`full_opening`): faith, policies, research, gold, beliefs.
    pub fn part_opening(&self, obs: &AIObservation<'_>, rules: &CompiledRuleset, state: &crate::ai::state::AiState) -> Vec<Decision> {
        let parts = self.parts();
        let mut out = Vec::new();
        if !parts.any() {
            return out;
        }
        let personality = personality::UncivPersonality::of(rules, obs.economy.nation);
        let major = obs.view.own_kind() == PlayerKind::Major;
        if major && parts.construction {
            // Unciv's production arms when the enemies seen outweigh its force.
            self.note_forces(obs, rules);
        }
        if major && parts.religion {
            out.extend(religion::spend_faith(obs, rules));
        }
        if major && parts.policies {
            out.extend(civ::adopt_policy(obs, rules, state, &personality));
        }
        if parts.research {
            out.extend(civ::research(obs, rules));
        }
        if parts.gold {
            out.extend(gold::use_gold(obs, rules, &personality));
        }
        if major && parts.religion {
            out.extend(religion::choose_beliefs(obs, rules));
        }
        if self.dry {
            out.clear();
        }
        out
    }

    /// A unit's decision by the family that commands it; None for the others.
    pub fn part_decide(&self, view: &UnitView<'_, '_>, tools: &mut PlanTools<'_>) -> Option<Decision> {
        let decision = self.part_decision(view, tools);
        decision.filter(|_| !self.dry)
    }

    fn part_decision(&self, view: &UnitView<'_, '_>, tools: &mut PlanTools<'_>) -> Option<Decision> {
        let rules = tools.rules;
        let kind = view.local.unit.kind;
        if !self.part_commands(rules, kind) {
            return None;
        }
        let sight = self.sight_of(view.local.player)?;
        let unit = Unit::new(view, &sight);
        let parts = self.parts();
        let def = rules.units.get(kind.0 as usize)?;
        if parts.workers && def.builder {
            return Some(worker::automate_worker(&unit, tools));
        }
        if parts.religion {
            if let Some(decision) = religion::automate_religious_unit(&unit, tools) {
                return Some(decision);
            }
        }
        let mut memo = Memo::new(view.local.unit.id);
        civilian::automate_civilian(&unit, &mut memo, tools)
    }

    /// The families' orders at the end of the turn: Unciv's production of the
    /// cities with an empty queue (and those in danger), its trade units; its
    /// settler (`trainSettler`).
    pub fn part_end_of_turn(&self, obs: &AIObservation<'_>, rules: &CompiledRuleset) -> Vec<Decision> {
        let parts = self.parts();
        let mut out = Vec::new();
        if !(parts.construction || parts.settle) || obs.view.own_kind() == PlayerKind::Module(ClassicKind::Barbarian) {
            return out;
        }
        let personality = personality::UncivPersonality::of(rules, obs.economy.nation);
        let empire = construction::Empire::new(obs, rules, personality, super::afraid_of_barbarians(obs));
        if parts.construction {
            let own_force = diplomacy::own_force(obs, rules);
            out.extend(construction::automate_cities(&empire, self.enemy_force(obs), own_force));
        }
        if parts.settle {
            out.extend(construction::train_settler(&empire));
        }
        if parts.construction {
            out.extend(crate::ai::trade::production_commands(obs, rules).into_iter().map(|c| Decision::act(Actor::Player, Reason::ProduceTradeUnit, c)));
        }
        if self.dry {
            out.clear();
        }
        out
    }
}
