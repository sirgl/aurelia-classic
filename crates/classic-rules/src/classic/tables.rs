//! The classic tables of the compiled rules (platform/PLATFORM.md stage 4): policies
//! and their branches, beliefs and religions, projects, city-states and
//! great people. The kernel's `CompiledRuleset` holds them as the module's
//! part (`CompiledRuleset::module`) and never reads their rows.

use crate::classic::Classic;
use crate::rules::tables::ProjectFacts;
use crate::classic::effects::GreatPersonKind;
use crate::ids::{BuildingKindId, ImprovementId, PolicyId, TechId, UnitKindId};
use crate::rules::content::{
    BeliefCategory, CityStateTypeId, EraId, InertEffect, PolicyBranchId, PolicyBranchKind,
    PolicyRole, ProjectId, ProjectKind, SpecialistId,
};
use crate::classic::compile_tables;
use crate::classic::effects::ClassicSource;
use crate::rules::compile::{RulesError, SourceKeys};
use crate::rules::compiled::CompiledRuleset;
use crate::rules::effects::EffectSet;
use crate::rules::fingerprint::Fingerprint;
use crate::rules::content::{ContentTable, TableTag};
use crate::rules::key_index::{KeyIndex, TableKeys};
use crate::rules::source::{
    SourceBelief, SourceCityState, SourceCityStateType, SourceGreatPerson, SourcePersonality, SourcePolicy,
    SourcePolicyBranch, SourceProject, SourceReligion,
};
use crate::rules::source::SourceRules;
use crate::rules::tables::ModuleTables;

/// Social policy (adoption arrives with the policies system).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PolicyDefinition {
    pub key: String,
    pub effects: EffectSet,
    // ---- content details ----
    pub name: String,
    pub branch: Option<PolicyBranchId>,
    pub role: PolicyRole,
    pub prerequisites: Vec<PolicyId>,
    pub grid: (u8, u8),
    /// The help text (en_US), for the Civics screen.
    pub text: String,
    pub inert: Vec<InertEffect>,
}

/// Religious belief (adoption arrives with the religion system).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BeliefDefinition {
    pub key: String,
    pub effects: EffectSet,
    // ---- content details ----
    pub name: String,
    pub category: Option<BeliefCategory>,
    /// The help text (en_US), for the religion screen.
    pub text: String,
    pub inert: Vec<InertEffect>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ProjectDefinition {
    pub key: String,
    pub name: String,
    pub kind: ProjectKind,
    pub cost: u32,
    pub unbuildable: bool,
    pub required_tech: Option<TechId>,
    pub prerequisite_projects: Vec<(ProjectId, u16)>,
    pub max_global_instances: u16,
    pub max_team_instances: u16,
    pub victory: Option<String>,
    /// Completing it lets its owner build nuclear weapons (the Manhattan
    /// Project).
    pub allows_nukes: bool,
    /// World Congress project data (Some only for `ProjectKind::LeagueProject`).
    pub league: Option<LeagueProject>,
    pub inert: Vec<InertEffect>,
}

/// A World Congress international project (classic/docs/classic/rules.md): the
/// members contribute production through its city process; at completion
/// each member is rewarded by its share.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct LeagueProject {
    /// Production of one nation's share at Standard speed (× the game
    /// speed's production percent): the project costs this × the nations of the game.
    pub cost_per_player: u32,
    /// A member must know it before the project may be proposed.
    pub tech_any_member: Option<TechId>,
    /// Rewards of tiers 1, 2 and 3; a member gets
    /// the rewards of its tier and of every tier below.
    pub tiers: [Vec<LeagueReward>; 3],
}

/// One reward of an international project.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LeagueReward {
    /// Permanent happiness.
    Happiness(u16),
    /// Free social policies.
    FreePolicies(u8),
    /// Points toward the next golden age.
    GoldenAgePoints(u32),
    /// Culture +100 % for this many turns.
    CultureBoostTurns(u16),
    /// Tourism +100 % for this many turns.
    TourismBoostTurns(u16),
    /// Influence with every city-state the member met, once.
    CityStateInfluence(u16),
    /// The science of this many of the member's last turns into its
    /// research.
    ResearchTurns(u16),
    /// A unit in the member's capital (a Great Scientist).
    FreeUnit(UnitKindId),
    /// A building in the member's capital (the International Space Station
    /// wonder).
    Building(BuildingKindId),
}

/// Share of the per-player cost a contribution needs for tiers 1 and 2: 40 % and 90 %. Own design
/// (Unciv has no league projects): a member that gives less than two fifths of its share gets nothing.
pub const LEAGUE_TIER_PERCENTS: [u32; 2] = [40, 90];

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PolicyBranchDefinition {
    pub key: String,
    pub name: String,
    pub kind: PolicyBranchKind,
    pub era_prerequisite: Option<EraId>,
    pub opener: Option<PolicyId>,
    pub finisher: Option<PolicyId>,
    pub mutually_exclusive_with: Vec<PolicyBranchId>,
    pub locked_without_religion: bool,
    pub first_adopter_free_policies: u8,
    pub second_adopter_free_policies: u8,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ReligionDefinition {
    pub key: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CityStateTypeDefinition {
    pub key: String,
    pub name: String,
    pub friends_threshold: i32,
    pub allies_threshold: i32,
    pub minor_trait: Option<crate::rules::content::CityStateTrait>,
    pub bonuses: Vec<CityStateBonus>,
    pub inert: Vec<InertEffect>,
}

/// A friend or ally bonus of a city-state type (from `era` on).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CityStateBonus {
    pub level: crate::rules::content::CityStateLevel,
    pub kind: crate::rules::content::CityStateBonusKind,
    pub era: Option<EraId>,
    pub amount: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CityStateDefinition {
    pub key: String,
    pub name: String,
    pub city_state_type: CityStateTypeId,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct GreatPersonDefinition {
    pub key: String,
    pub kind: GreatPersonKind,
    pub unit: UnitKindId,
    pub specialist: Option<SpecialistId>,
    pub improvements: Vec<ImprovementId>,
    /// What makes it born: its specialists' points, combat experience or
    /// faith.
    pub points_source: crate::rules::content::PointsSource,
    pub inert: Vec<InertEffect>,
}

/// The classic tables as content writes them (`SourceRules::module`).
#[derive(Debug, Clone, Default)]
pub struct ClassicSourceTables {
    pub policies: Vec<SourcePolicy>,
    pub beliefs: Vec<SourceBelief>,
    pub projects: Vec<SourceProject>,
    pub policy_branches: Vec<SourcePolicyBranch>,
    pub religions: Vec<SourceReligion>,
    pub city_state_types: Vec<SourceCityStateType>,
    pub city_states: Vec<SourceCityState>,
    pub great_people: Vec<SourceGreatPerson>,
    pub personalities: Vec<SourcePersonality>,
}

/// The classic tables a reference may point into, by the tags they had
/// among the kernel's tables before the split (`ContentTable::tag`).
/// Projects are a kernel table (`ContentTable::Project`) whose rows the
/// module holds.
pub const POLICIES: ContentTable = ContentTable::Module(TableTag::new(8));
pub const BELIEFS: ContentTable = ContentTable::Module(TableTag::new(9));
pub const POLICY_BRANCHES: ContentTable = ContentTable::Module(TableTag::new(14));
pub const RELIGIONS: ContentTable = ContentTable::Module(TableTag::new(15));
pub const CITY_STATE_TYPES: ContentTable = ContentTable::Module(TableTag::new(16));
pub const CITY_STATES: ContentTable = ContentTable::Module(TableTag::new(17));
pub const GREAT_PEOPLE: ContentTable = ContentTable::Module(TableTag::new(19));

/// The classic tables of the compiled rules (`CompiledRuleset::module`).
#[derive(Debug, Clone, Default)]
pub struct ClassicTables {
    pub policies: Vec<PolicyDefinition>,
    pub beliefs: Vec<BeliefDefinition>,
    pub projects: Vec<ProjectDefinition>,
    pub policy_branches: Vec<PolicyBranchDefinition>,
    pub religions: Vec<ReligionDefinition>,
    pub city_state_types: Vec<CityStateTypeDefinition>,
    pub city_states: Vec<CityStateDefinition>,
    pub great_people: Vec<GreatPersonDefinition>,
    /// The leader's personality of every nation, by nation
    /// index (`Personality::NEUTRAL` where the content names none). AI
    /// data, not a rule: it stays out of the ruleset fingerprint, so saves
    /// keep loading whatever the personalities.
    pub personalities: Vec<crate::classic::content::Personality>,
}

impl ClassicTables {
    /// The personality of nation `nation`'s leader.
    pub fn personality(&self, nation: u8) -> crate::classic::content::Personality {
        self.personalities.get(usize::from(nation)).copied().unwrap_or(crate::classic::content::Personality::NEUTRAL)
    }
}

impl ModuleTables<Classic> for ClassicTables {
    type Source = ClassicSourceTables;
    type Rows = crate::classic::rows::ClassicRows;

    fn merge_source(source: &mut ClassicSourceTables, other: ClassicSourceTables) {
        source.policies.extend(other.policies);
        source.beliefs.extend(other.beliefs);
        source.projects.extend(other.projects);
        source.policy_branches.extend(other.policy_branches);
        source.religions.extend(other.religions);
        source.city_state_types.extend(other.city_state_types);
        source.city_states.extend(other.city_states);
        source.great_people.extend(other.great_people);
        source.personalities.extend(other.personalities);
    }

    fn source_keys(s: &ClassicSourceTables) -> Vec<(ContentTable, TableKeys)> {
        vec![
            (POLICIES, TableKeys::of(s.policies.iter().map(|x| &x.id), "policy")),
            (BELIEFS, TableKeys::of(s.beliefs.iter().map(|x| &x.id), "belief")),
            (ContentTable::Project, TableKeys::of(s.projects.iter().map(|x| &x.id), "project")),
            (POLICY_BRANCHES, TableKeys::of(s.policy_branches.iter().map(|x| &x.id), "policy-branch")),
            (RELIGIONS, TableKeys::of(s.religions.iter().map(|x| &x.id), "religion")),
            (CITY_STATE_TYPES, TableKeys::of(s.city_state_types.iter().map(|x| &x.id), "city-state-type")),
            (CITY_STATES, TableKeys::of(s.city_states.iter().map(|x| &x.id), "city-state")),
            (GREAT_PEOPLE, TableKeys::of(s.great_people.iter().map(|x| &x.id), "great-person")),
        ]
    }
    type SourceRoles = crate::classic::roles::ClassicSourceRoles;
    type Roles = crate::classic::roles::ClassicRoles;

    fn merge_roles(roles: &mut Self::SourceRoles, other: Self::SourceRoles) {
        roles.merge(other)
    }

    fn compile_roles(source: &Self::SourceRoles, ix: &KeyIndex, rules: &CompiledRuleset) -> Result<Self::Roles, RulesError> {
        crate::classic::roles::compile_roles(source, ix, rules)
    }

    fn compile(
        source: &SourceRules,
        keys: &SourceKeys,
        ix: &KeyIndex,
        rules: &CompiledRuleset,
    ) -> Result<Self, RulesError> {
        compile_tables::compile(source, keys, ix, rules)
    }

    fn check(rules: &CompiledRuleset) -> Result<(), RulesError> {
        compile_tables::check(rules)
    }

    fn attach_effects(
        source: &SourceRules,
        keys: &SourceKeys,
        ix: &KeyIndex,
        rules: &mut CompiledRuleset,
    ) -> Result<(), RulesError> {
        compile_tables::attach_effects(source, keys, ix, rules)
    }

    fn effect_set(&self, source: ClassicSource) -> Option<&EffectSet> {
        match source {
            ClassicSource::Policy(id) => self.policies.get(id.0 as usize).map(|d| &d.effects),
            ClassicSource::Belief(id) => self.beliefs.get(id.0 as usize).map(|d| &d.effects),
        }
    }

    fn effect_sets(&self) -> Vec<(ClassicSource, &EffectSet)> {
        compile_tables::effect_sets_of(self)
    }

    /// The places the tables held in the fingerprint before the split.
    fn hash_after(&self, kernel_table: &str, f: &mut Fingerprint) {
        match kernel_table {
            "promotions" => {
                f.table("policies", &self.policies, |d| &d.key);
                f.table("beliefs", &self.beliefs, |d| &d.key);
            }
            "natural_wonders" => {
                f.table("projects", &self.projects, |d| &d.key);
                f.table("policy_branches", &self.policy_branches, |d| &d.key);
                f.table("religions", &self.religions, |d| &d.key);
                f.table("city_state_types", &self.city_state_types, |d| &d.key);
                f.table("city_states", &self.city_states, |d| &d.key);
            }
            "specialists" => f.table("great_people", &self.great_people, |d| &d.key),
            _ => {}
        }
    }

    fn project(&self, id: ProjectId) -> Option<ProjectFacts> {
        let def = self.projects.get(id.0 as usize)?;
        Some(ProjectFacts {
            cost: def.cost,
            unbuildable: def.unbuildable || def.kind == crate::rules::content::ProjectKind::LeagueProject,
            contributed: def.league.is_some(),
        })
    }

    fn projects_of_tech(&self, tech: crate::ids::TechId) -> Vec<ProjectId> {
        let unlocked = self.projects.iter().enumerate().filter(|(_, p)| p.required_tech == Some(tech));
        unlocked.map(|(i, _)| ProjectId(i as u16)).collect()
    }
}
