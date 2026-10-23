//! The classic tables as content writes them (`classic::tables::ClassicSourceTables`):
//! policies and their branches, beliefs and religions, projects, city-states
//! and great people.

use crate::classic::content::{BeliefCategory, GreatPersonKind, PolicyBranchKind, PolicyRole, ProjectKind};
use crate::module::RulesModule;
use crate::rules::content::SourceInertEffect;
use crate::rules::source::{KeyCount, StableContentKey};

/// Social policy. Adoption state arrives with the policies system; the table
/// and its effects are compiled and validated now.
#[derive(Debug, Clone, Default)]
pub struct SourcePolicy<M: RulesModule> {
    pub id: Option<StableContentKey>,
    pub effects: Vec<kernel::rules::source::SourceEffect<M>>,
    // ---- content details ----
    pub name: String,
    pub branch: Option<StableContentKey>,
    pub role: PolicyRole,
    pub prerequisites: Vec<StableContentKey>,
    pub grid: (u8, u8),
    pub text: String,
    pub inert: Vec<SourceInertEffect>,
}

/// Religious belief. Adoption state arrives with the religion system;
/// the table and its effects are compiled and validated now.
#[derive(Debug, Clone, Default)]
pub struct SourceBelief<M: RulesModule> {
    pub id: Option<StableContentKey>,
    pub effects: Vec<kernel::rules::source::SourceEffect<M>>,
    // ---- content details ----
    pub name: String,
    pub category: Option<BeliefCategory>,
    pub text: String,
    pub inert: Vec<SourceInertEffect>,
}

/// A World Congress project as written in content (`compiled::LeagueProject`).
#[derive(Debug, Clone, Default)]
pub struct SourceLeagueProject {
    pub cost_per_player: u32,
    pub tech_any_member: Option<StableContentKey>,
    /// Exactly three tiers (the compiler checks).
    pub tiers: Vec<Vec<SourceLeagueReward>>,
}

/// A reward of a World Congress project as written in content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceLeagueReward {
    Happiness(u16),
    FreePolicies(u8),
    GoldenAgePoints(u32),
    CultureBoostTurns(u16),
    TourismBoostTurns(u16),
    CityStateInfluence(u16),
    ResearchTurns(u16),
    FreeUnit(StableContentKey),
    Building(StableContentKey),
}

/// Project: built like a building, but not a city building.
#[derive(Debug, Clone, Default)]
pub struct SourceProject {
    pub id: Option<StableContentKey>,
    pub name: String,
    pub kind: Option<ProjectKind>,
    /// Production cost (0 for World Congress projects: contributed to).
    pub cost: u32,
    /// Not built with production (spaceship thresholds: counted from the
    /// spaceship part units).
    pub unbuildable: bool,
    pub required_tech: Option<StableContentKey>,
    pub prerequisite_projects: Vec<KeyCount>,
    pub max_global_instances: u16,
    pub max_team_instances: u16,
    /// Victory it counts for (catalog name, e.g. "space").
    pub victory: Option<String>,
    /// Completing it allows nuclear weapons (Manhattan Project).
    pub allows_nukes: bool,
    /// World Congress project data.
    pub league: Option<SourceLeagueProject>,
    pub inert: Vec<SourceInertEffect>,
}

/// Policy branch or ideology.
#[derive(Debug, Clone, Default)]
pub struct SourcePolicyBranch {
    pub id: Option<StableContentKey>,
    pub name: String,
    pub kind: Option<PolicyBranchKind>,
    pub era_prerequisite: Option<StableContentKey>,
    pub opener: Option<StableContentKey>,
    pub finisher: Option<StableContentKey>,
    pub mutually_exclusive_with: Vec<StableContentKey>,
    pub locked_without_religion: bool,
    pub first_adopter_free_policies: u8,
    pub second_adopter_free_policies: u8,
}

/// The personality of a civilization's leader (classic/docs/ai-leaders.md).
#[derive(Debug, Clone)]
pub struct SourcePersonality {
    pub civ: StableContentKey,
    pub personality: crate::classic::content::Personality,
}

#[derive(Debug, Clone, Default)]
pub struct SourceReligion {
    pub id: Option<StableContentKey>,
    pub name: String,
}

/// City-state type: friend and ally bonuses are inert effects.
#[derive(Debug, Clone, Default)]
pub struct SourceCityStateType {
    pub id: Option<StableContentKey>,
    pub name: String,
    pub friends_threshold: i32,
    pub allies_threshold: i32,
    /// None: content without city-state mechanics (legacy fixtures).
    pub minor_trait: Option<crate::rules::content::CityStateTrait>,
    pub bonuses: Vec<SourceCityStateBonus>,
    pub inert: Vec<SourceInertEffect>,
}

/// One friend or ally bonus of a city-state type; `era`: from this era on
/// (the latest one reached applies).
#[derive(Debug, Clone)]
pub struct SourceCityStateBonus {
    pub level: crate::rules::content::CityStateLevel,
    pub kind: crate::rules::content::CityStateBonusKind,
    pub era: Option<StableContentKey>,
    pub amount: i32,
}

#[derive(Debug, Clone, Default)]
pub struct SourceCityState {
    pub id: Option<StableContentKey>,
    pub name: String,
    pub city_state_type: Option<StableContentKey>,
}

/// Great person: the unit, its kind and its specialist.
#[derive(Debug, Clone, Default)]
pub struct SourceGreatPerson {
    pub id: Option<StableContentKey>,
    pub kind: Option<GreatPersonKind>,
    pub unit: Option<StableContentKey>,
    pub specialist: Option<StableContentKey>,
    /// Improvements it can create.
    pub improvements: Vec<StableContentKey>,
    pub points_source: crate::rules::content::PointsSource,
    pub inert: Vec<SourceInertEffect>,
}
