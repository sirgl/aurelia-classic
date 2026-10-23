//! The classic role bindings (`ModuleTables::Roles`): the order that breaks
//! ties between policy branches of the AI, the ideology it picks in each
//! situation, the branch that opens buying each great person kind with
//! faith, and the ideology public opinion prefers on a tie. They hash last
//! in the ruleset's roles, where they were before the split.

use kernel::rules::roles::bind;

use crate::rules::compile::RulesError;
use crate::rules::content::{PolicyBranchId, PolicyBranchKind};
use crate::rules::key_index::KeyIndex;
use crate::rules::source::StableContentKey;
use crate::rules::stats::{GreatPersonKind, GREAT_PERSON_KINDS};
use crate::rules::CompiledRuleset;

/// The situation in which the AI picks an ideology (`ai::policies`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum IdeologyCase {
    /// An empire of many cities.
    Wide,
    AtWar,
    AtPeace,
}


/// The classic role bindings as the content writes them.
#[derive(Debug, Clone, Default)]
pub struct ClassicSourceRoles {
    /// The order that breaks a tie between policy branches the AI values
    /// alike (`ai::policy_branches`).
    pub ai_branches: Vec<StableContentKey>,
    /// The ideology the AI picks in each situation.
    pub ai_ideologies: Vec<(IdeologyCase, StableContentKey)>,
    /// The branch whose completion lets a player buy a great person kind
    /// with faith (rules.md).
    pub faith_great_people: Vec<(GreatPersonKind, StableContentKey)>,
    /// (ideology, the ideology its public opinion prefers on a tie).
    pub ideology_ties: Vec<(StableContentKey, StableContentKey)>,
}

impl ClassicSourceRoles {
    /// Take the bindings `other` writes (a ruleset split over files).
    pub fn merge(&mut self, other: ClassicSourceRoles) {
        self.ai_branches.extend(other.ai_branches);
        self.ai_ideologies.extend(other.ai_ideologies);
        self.faith_great_people.extend(other.faith_great_people);
        self.ideology_ties.extend(other.ideology_ties);
    }
}

/// The classic role bindings resolved to rows (field order: the fingerprint).
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct ClassicRoles {
    ai_branches: Vec<PolicyBranchId>,
    /// By `IdeologyCase as usize`.
    ai_ideologies: [Option<PolicyBranchId>; 3],
    /// By `GreatPersonKind as usize`.
    faith_branches: [Option<PolicyBranchId>; GREAT_PERSON_KINDS],
    ideology_ties: Vec<(PolicyBranchId, PolicyBranchId)>,
}

impl ClassicRoles {
    /// The order that breaks a tie between policy branches of equal value.
    pub fn ai_branches(&self) -> &[PolicyBranchId] {
        &self.ai_branches
    }

    pub fn ai_ideology(&self, case: IdeologyCase) -> Option<PolicyBranchId> {
        self.ai_ideologies[case as usize]
    }

    /// The branch whose completion lets a player buy `kind` with faith.
    pub fn faith_branch(&self, kind: GreatPersonKind) -> Option<PolicyBranchId> {
        self.faith_branches[kind as usize]
    }

    /// The ideology public opinion prefers when `ideology` ties another.
    pub fn ideology_tie(&self, ideology: PolicyBranchId) -> Option<PolicyBranchId> {
        self.ideology_ties.iter().find(|(from, _)| *from == ideology).map(|(_, to)| *to)
    }
}

/// The policy branch bindings: AI orders name normal branches (once each),
/// AI picks and ties name ideologies, each great person kind one branch.
pub(crate) fn compile_roles(
    source: &ClassicSourceRoles,
    ix: &KeyIndex,
    rules: &CompiledRuleset,
) -> Result<ClassicRoles, RulesError> {
    let mut roles = ClassicRoles::default();
    let branch = |key: &StableContentKey, kind: Option<PolicyBranchKind>| -> Result<PolicyBranchId, RulesError> {
        let id = PolicyBranchId(ix.id(crate::classic::tables::POLICY_BRANCHES, &key.0)?);
        match kind {
            Some(kind) if rules.module.policy_branches.get(id.0 as usize).map(|b| b.kind) != Some(kind) => {
                Err(RulesError::Inconsistent(format!("{} is not a {kind:?} branch", key.0)))
            }
            _ => Ok(id),
        }
    };
    let order = |keys: &[StableContentKey]| -> Result<Vec<PolicyBranchId>, RulesError> {
        let mut out: Vec<PolicyBranchId> = Vec::with_capacity(keys.len());
        for key in keys {
            let id = branch(key, Some(PolicyBranchKind::Normal))?;
            if out.contains(&id) {
                return Err(RulesError::Inconsistent(format!("{} listed twice", key.0)));
            }
            out.push(id);
        }
        Ok(out)
    };
    roles.ai_branches = order(&source.ai_branches)?;
    for (case, key) in &source.ai_ideologies {
        let id = branch(key, Some(PolicyBranchKind::Ideology))?;
        bind(&mut roles.ai_ideologies[*case as usize], id, &format!("{case:?}"))?;
    }
    for (kind, key) in &source.faith_great_people {
        let id = branch(key, None)?;
        bind(&mut roles.faith_branches[*kind as usize], id, &format!("faith {kind:?}"))?;
    }
    for (from, to) in &source.ideology_ties {
        let pair = (branch(from, Some(PolicyBranchKind::Ideology))?, branch(to, Some(PolicyBranchKind::Ideology))?);
        if roles.ideology_ties.iter().any(|(f, _)| *f == pair.0) {
            return Err(RulesError::Inconsistent(format!("{} ties twice", from.0)));
        }
        roles.ideology_ties.push(pair);
    }
    Ok(roles)
}

