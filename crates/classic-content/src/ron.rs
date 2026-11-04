// RON schema of game content (§28/29): a verbatim mirror of Source*.
// The v1 fields are mandatory (except Option): we do not hide defaults in content files;
// a typo must fail the parser, not silently become zero.
// Classic-ruleset fields default to "not declared" (old content parses as it was).
// An unknown field is a parse error. Keys are strings "core:unit/settler".
//
// One file is part of a rule set: any tables, the rest are empty
// (the classic set is laid out by category in classic/assets/rules/classic/*.ron).

use crate::effects::{effects, RonEffect};
use crate::ron_types::{
    inert, key, key_count, key_counts, keys, opt_key, RonBeliefCategory, RonBuildingAbility,
    RonBuildingKind, RonUnitAbility, RonDomain, RonGreatPersonKind, RonImprovementKind, RonInert,
    RonPolicyBranchKind, RonCityStateBonusKind, RonCityStateLevel, RonCityStateTrait,
    RonFeatureRemoval, RonPlacement, RonPolicyRole, RonProjectKind, RonResourceClass,
    RonStartRegions, RonTechYield, RonUnitRole, RonYields,
};
use classic_rules::rules::source::{
    SourceBelief, SourceBuilding, SourceCityState, SourceCityStateType, SourceNation, SourceEra,
    SourceFeature, SourceGreatPerson, SourceImprovement, SourceLeagueProject, SourceLeagueReward,
    SourceNaturalWonder, SourcePolicy, SourcePolicyBranch, SourceProject, SourcePromotion,
    SourceReligion, SourceResource, SourceRules, SourceSpecialist, SourceTechYield,
    SourceTechnology, SourceTerrain, SourceUnit, SourceWonderProduction, StartBias,
};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RonUnit {
    id: String,
    movement: u16,
    strength: u16,
    #[serde(default)]
    founder: bool,
    #[serde(default)]
    builder: bool,
    cost: u32,
    #[serde(default)]
    effects: Vec<RonEffect>,
    required_tech: Option<String>,
    #[serde(default)]
    name: String,
    #[serde(default)]
    class: Option<String>,
    #[serde(default)]
    role: Option<RonUnitRole>,
    #[serde(default)]
    domain: RonDomain,
    #[serde(default)]
    ranged_strength: u16,
    #[serde(default)]
    range: u8,
    #[serde(default)]
    sight: u8,
    #[serde(default)]
    faith_cost: Option<u32>,
    #[serde(default)]
    unbuildable: bool,
    #[serde(default)]
    obsolete_tech: Option<String>,
    #[serde(default)]
    required_resources: Vec<(String, u16)>,
    #[serde(default)]
    upgrades_to: Vec<String>,
    #[serde(default)]
    free_promotions: Vec<String>,
    #[serde(default)]
    era: Option<String>,
    #[serde(default)]
    nation: Option<String>,
    #[serde(default)]
    replaces: Option<String>,
    #[serde(default)]
    requires_buildings: Vec<String>,
    #[serde(default)]
    combat: crate::ron_types::RonUnitCombat,
    #[serde(default)]
    required_project: Option<String>,
    #[serde(default)]
    spaceship_project: Option<String>,
    #[serde(default)]
    required_policy: Option<String>,
    #[serde(default)]
    builds: Vec<String>,
    #[serde(default)]
    builds_roads: bool,
    #[serde(default)]
    abilities: Vec<RonUnitAbility>,
    #[serde(default)]
    inert: Vec<RonInert>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RonBuilding {
    id: String,
    cost: u32,
    #[serde(default)]
    effects: Vec<RonEffect>,
    required_tech: Option<String>,
    #[serde(default)]
    wonder: bool,
    #[serde(default)]
    name: String,
    #[serde(default)]
    kind: RonBuildingKind,
    #[serde(default)]
    class: Option<String>,
    #[serde(default)]
    faith_cost: Option<u32>,
    #[serde(default)]
    unbuildable: bool,
    #[serde(default)]
    maintenance: u32,
    #[serde(default)]
    obsolete_tech: Option<String>,
    #[serde(default)]
    era: Option<String>,
    #[serde(default)]
    hurry_cost_modifier: i32,
    #[serde(default)]
    conquest_survival_pct: u8,
    #[serde(default)]
    never_capture: bool,
    #[serde(default)]
    specialist_slots: Option<(String, u16)>,
    #[serde(default)]
    great_work_slots: Option<(String, u16)>,
    #[serde(default)]
    theming: Vec<crate::ron_types::RonThemingBonus>,
    #[serde(default)]
    requires_policy_branch: Option<String>,
    #[serde(default)]
    requires_holy_city: bool,
    #[serde(default)]
    faith_purchase_needs_belief: bool,
    #[serde(default)]
    consumes_resources: Vec<(String, u16)>,
    #[serde(default)]
    requires_buildings: Vec<String>,
    #[serde(default)]
    requires_buildings_in_empire: Vec<String>,
    #[serde(default)]
    placement: crate::ron_types::RonBuildingPlacement,
    #[serde(default)]
    cost_per_city_pct: i32,
    #[serde(default)]
    max_start_era: Option<String>,
    #[serde(default)]
    free_start_era: Option<String>,
    #[serde(default)]
    nation: Option<String>,
    #[serde(default)]
    replaces: Option<String>,
    #[serde(default)]
    abilities: Vec<RonBuildingAbility>,
    #[serde(default)]
    inert: Vec<RonInert>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RonTechnology {
    id: String,
    /// Era for visualisation (None = no column caption).
    #[serde(default)]
    era_name: Option<String>,
    #[serde(default)]
    unlocks_buildings: Vec<String>,
    cost: u32,
    #[serde(default)]
    requires: Vec<String>,
    #[serde(default)]
    effects: Vec<RonEffect>,
    #[serde(default)]
    name: String,
    #[serde(default)]
    era: Option<String>,
    #[serde(default)]
    grid: (u8, u8),
    #[serde(default)]
    unlocks_units: Vec<String>,
    #[serde(default)]
    unlocks_improvements: Vec<String>,
    #[serde(default)]
    unlocks_projects: Vec<String>,
    #[serde(default)]
    unlocks_actions: Vec<String>,
    #[serde(default)]
    repeatable: bool,
    #[serde(default)]
    allows_embarking: bool,
    #[serde(default)]
    embarked_all_water: bool,
    #[serde(default)]
    allows_embassy: bool,
    #[serde(default)]
    allows_open_borders: bool,
    #[serde(default)]
    allows_defensive_pact: bool,
    #[serde(default)]
    allows_research_agreement: bool,
    #[serde(default)]
    allows_world_congress: bool,
    #[serde(default)]
    triggers_archaeological_sites: bool,
    #[serde(default)]
    map_visible: bool,
    #[serde(default)]
    bridge_building: bool,
    #[serde(default)]
    road_move_change: i16,
    #[serde(default)]
    inert: Vec<RonInert>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RonImprovement {
    id: String,
    #[serde(default)]
    work_turns: u16,
    #[serde(default)]
    food_bonus: i32,
    #[serde(default)]
    prod_bonus: i32,
    #[serde(default)]
    river_food: i32,
    required_tech: Option<String>,
    #[serde(default)]
    name: String,
    #[serde(default)]
    kind: RonImprovementKind,
    #[serde(default)]
    unbuildable: bool,
    #[serde(default)]
    yields: RonYields,
    #[serde(default)]
    work_time: u32,
    #[serde(default)]
    build_action: Option<String>,
    #[serde(default)]
    valid_terrains: Vec<String>,
    #[serde(default)]
    valid_features: Vec<String>,
    #[serde(default)]
    resources: Vec<String>,
    #[serde(default)]
    fresh_water_yields: Vec<RonTechYield>,
    #[serde(default)]
    defense_pct: i32,
    #[serde(default)]
    pillage_gold: u32,
    #[serde(default)]
    culture_bomb_radius: u8,
    #[serde(default)]
    nearby_enemy_damage: u16,
    #[serde(default)]
    culture_per_era: u16,
    #[serde(default)]
    nation: Option<String>,
    #[serde(default)]
    created_by_great_person: bool,
    #[serde(default)]
    placement: RonPlacement,
    #[serde(default)]
    adjacent_mountain_yields: RonYields,
    #[serde(default)]
    removes_resource: bool,
    #[serde(default)]
    adjacent_same_culture: u8,
    #[serde(default)]
    city_state_luxury_copies: u8,
    #[serde(default)]
    inert: Vec<RonInert>,
}

/// Tile feature (forest): a permanent bonus and chopping by a worker.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RonFeature {
    id: String,
    #[serde(default)]
    chop_turns: u16,
    #[serde(default)]
    prod_bonus: i32,
    #[serde(default)]
    chop_grant: u32,
    #[serde(default)]
    name: String,
    #[serde(default)]
    yields: RonYields,
    #[serde(default)]
    yield_not_additive: bool,
    #[serde(default)]
    movement_cost: u8,
    #[serde(default)]
    defense_pct: i32,
    #[serde(default)]
    impassable: bool,
    #[serde(default)]
    rough: bool,
    #[serde(default)]
    fresh_water: bool,
    #[serde(default)]
    no_improvement: bool,
    #[serde(default)]
    requires_river: bool,
    #[serde(default)]
    requires_flatlands: bool,
    #[serde(default)]
    see_through: u8,
    #[serde(default)]
    valid_terrains: Vec<String>,
    #[serde(default)]
    unremovable: bool,
    #[serde(default)]
    removal: Option<RonFeatureRemoval>,
    #[serde(default)]
    inert: Vec<RonInert>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RonResource {
    id: String,
    #[serde(default)]
    food_bonus: i32,
    #[serde(default)]
    prod_bonus: i32,
    #[serde(default)]
    gold_bonus: u32,
    #[serde(default)]
    requires_improvement: Option<String>,
    required_tech: Option<String>,
    #[serde(default)]
    terrains: Vec<u16>,
    density: u8,
    #[serde(default)]
    sea_density: u8,
    #[serde(default)]
    name: String,
    #[serde(default)]
    class: Option<RonResourceClass>,
    #[serde(default)]
    yields: RonYields,
    #[serde(default)]
    happiness: i32,
    #[serde(default)]
    improvements: Vec<String>,
    #[serde(default)]
    tradeable_with_tech: Option<String>,
    #[serde(default)]
    valid_terrains: Vec<String>,
    #[serde(default)]
    valid_features: Vec<String>,
    #[serde(default)]
    hills: bool,
    #[serde(default)]
    flatlands: bool,
    #[serde(default = "yes")]
    in_standard_game: bool,
    #[serde(default)]
    only_city_states: bool,
    #[serde(default)]
    nation: Option<String>,
    #[serde(default)]
    quantities: Vec<u8>,
    #[serde(default)]
    improved_yields: Vec<(String, RonYields)>,
    #[serde(default)]
    revealed_by_policy: Option<String>,
    wonder_production: Option<RonWonderProduction>,
    #[serde(default)]
    inert: Vec<RonInert>,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RonPolicy {
    id: String,
    #[serde(default)]
    effects: Vec<RonEffect>,
    #[serde(default)]
    name: String,
    #[serde(default)]
    branch: Option<String>,
    #[serde(default)]
    role: RonPolicyRole,
    #[serde(default)]
    prerequisites: Vec<String>,
    #[serde(default)]
    grid: (u8, u8),
    #[serde(default)]
    text: String,
    #[serde(default)]
    inert: Vec<RonInert>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RonBelief {
    id: String,
    #[serde(default)]
    effects: Vec<RonEffect>,
    #[serde(default)]
    name: String,
    #[serde(default)]
    category: Option<RonBeliefCategory>,
    #[serde(default)]
    text: String,
    #[serde(default)]
    inert: Vec<RonInert>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RonPromotion {
    id: String,
    #[serde(default)]
    effects: Vec<RonEffect>,
    #[serde(default)]
    name: String,
    #[serde(default)]
    requires: Vec<String>,
    #[serde(default)]
    requires_any_of: Vec<String>,
    #[serde(default)]
    choosable: bool,
    #[serde(default)]
    lost_with_upgrade: bool,
    #[serde(default)]
    required_tech: Option<String>,
    #[serde(default)]
    unit_roles: Vec<RonUnitRole>,
    #[serde(default)]
    civilian_units: Vec<String>,
    #[serde(default)]
    inert: Vec<RonInert>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RonColor {
    r: u8,
    g: u8,
    b: u8,
}

/// Nation (classic/assets/rules/classic/nations.ron; v1: nations.ron).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RonNation {
    id: String,
    name: String,
    leader: String,
    color: RonColor,
    /// "none" | "coastal" (an unknown value is an honest parse error).
    start_bias: String,
    /// Unique abilities.
    #[serde(default)]
    effects: Vec<RonEffect>,
    #[serde(default)]
    adjective: String,
    #[serde(default)]
    ability_name: String,
    #[serde(default)]
    unique_units: Vec<String>,
    #[serde(default)]
    unique_buildings: Vec<String>,
    #[serde(default)]
    unique_improvements: Vec<String>,
    #[serde(default)]
    start_regions: RonStartRegions,
    #[serde(default)]
    free_techs: Vec<String>,
    #[serde(default)]
    free_units: Vec<(String, u16)>,
    /// The nation's own unit for a starting slot (the Shoshone's Pathfinder
    /// in the defense slot), `(Defense, "core:unit/pathfinder")`.
    #[serde(default)]
    start_units: Vec<(crate::ron_roles::RonStartingUnit, String)>,
    #[serde(default)]
    preferred_religions: Vec<String>,
    #[serde(default)]
    inert: Vec<RonInert>,
    #[serde(default)]
    city_names: Vec<String>,
    #[serde(default)]
    spy_names: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct RonNations {
    nations: Vec<RonNation>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RonEra {
    id: String,
    name: String,
    index: u8,
    starting_defense_units: u8,
    starting_worker_units: u8,
    starting_explore_units: u8,
    starting_gold: u32,
    research_agreement_cost: u32,
    embarked_unit_defense: u16,
    faith_cost_multiplier: u32,
}

/// A game speed (speeds.ron, from Unciv's Speeds.json).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RonSpeed {
    id: String,
    name: String,
    index: u8,
    pace_percent: u32,
    production_percent: u32,
    gold_percent: u32,
    science_percent: u32,
    culture_percent: u32,
    faith_percent: u32,
    improvement_percent: u32,
    barbarian_percent: u32,
    gold_gift_percent: u32,
    tribute_interval_times10: u32,
    golden_age_percent: u32,
    adjacent_city_pressure: u32,
    peace_rounds: u32,
    deal_rounds: u32,
    start_year: i32,
    calendar: Vec<(u32, u32)>,
}

fn speed(s: RonSpeed) -> Result<classic_rules::rules::speeds::SourceSpeed, crate::error::ContentError> {
    use classic_rules::rules::speeds::{SpeedValues, SPEED_STRETCHES};
    if s.calendar.len() > SPEED_STRETCHES {
        return Err(crate::error::ContentError::Parse(format!("{}: more than {SPEED_STRETCHES} calendar stretches", s.id)));
    }
    let mut calendar = [(0, 0); SPEED_STRETCHES];
    calendar[..s.calendar.len()].copy_from_slice(&s.calendar);
    Ok(classic_rules::rules::speeds::SourceSpeed {
        id: Some(classic_rules::rules::source::StableContentKey(s.id)),
        name: s.name,
        index: s.index,
        values: SpeedValues {
            pace_percent: s.pace_percent,
            production_percent: s.production_percent,
            gold_percent: s.gold_percent,
            science_percent: s.science_percent,
            culture_percent: s.culture_percent,
            faith_percent: s.faith_percent,
            improvement_percent: s.improvement_percent,
            barbarian_percent: s.barbarian_percent,
            gold_gift_percent: s.gold_gift_percent,
            tribute_interval_times10: s.tribute_interval_times10,
            golden_age_percent: s.golden_age_percent,
            adjacent_city_pressure: s.adjacent_city_pressure,
            peace_rounds: s.peace_rounds,
            deal_rounds: s.deal_rounds,
            start_year: s.start_year,
            calendar,
        },
    })
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RonHandicap {
    id: String,
    name: String,
    index: u8,
    happiness_default: i32,
    extra_happiness_per_luxury: i32,
    num_cities_unhappiness_mod: i32,
    population_unhappiness_mod: i32,
    production_free_units: i32,
    production_free_units_per_city: i32,
    production_free_units_population_percent: i32,
    gold_free_units: i32,
    route_cost_percent: i32,
    unit_cost_percent: i32,
    building_cost_percent: i32,
    research_percent: i32,
    policy_percent: i32,
    ai_unhappiness_percent: i32,
    ai_growth_percent: i32,
    ai_train_percent: i32,
    ai_world_train_percent: i32,
    ai_construct_percent: i32,
    ai_world_construct_percent: i32,
    ai_create_percent: i32,
    ai_world_create_percent: i32,
    ai_building_cost_percent: i32,
    ai_unit_cost_percent: i32,
    ai_unit_supply_percent: i32,
    ai_unit_upgrade_percent: i32,
    ai_per_era_modifier: i32,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RonTerrain {
    id: String,
    name: String,
    #[serde(default)]
    yields: RonYields,
    #[serde(default)]
    hills_yields: RonYields,
    movement_cost: u8,
    #[serde(default)]
    impassable: bool,
    #[serde(default)]
    defense_pct: i32,
    #[serde(default)]
    water: bool,
    #[serde(default)]
    can_found_city: bool,
    #[serde(default)]
    map_code: Option<u16>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RonNaturalWonder {
    id: String,
    name: String,
    #[serde(default)]
    yields: RonYields,
    #[serde(default)]
    impassable: bool,
    #[serde(default)]
    movement_cost: u8,
    #[serde(default)]
    defense_pct: i32,
    #[serde(default)]
    happiness: i32,
    #[serde(default)]
    first_finder_gold: u32,
    #[serde(default)]
    fresh_water: bool,
    #[serde(default)]
    valid_terrains: Vec<String>,
    #[serde(default)]
    adjacent_unit_free_promotion: Option<String>,
    #[serde(default)]
    occurrence_frequency: u16,
    #[serde(default)]
    rough: bool,
    #[serde(default)]
    no_improvement: bool,
    #[serde(default)]
    placement: crate::natural_wonders::RonPlacement,
    #[serde(default)]
    inert: Vec<RonInert>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RonProject {
    id: String,
    name: String,
    kind: RonProjectKind,
    cost: u32,
    #[serde(default)]
    unbuildable: bool,
    required_tech: Option<String>,
    #[serde(default)]
    prerequisite_projects: Vec<(String, u16)>,
    #[serde(default)]
    max_global_instances: u16,
    #[serde(default)]
    max_team_instances: u16,
    #[serde(default)]
    victory: Option<String>,
    #[serde(default)]
    allows_nukes: bool,
    #[serde(default)]
    league: Option<RonLeagueProject>,
    #[serde(default)]
    inert: Vec<RonInert>,
}

/// `classic_rules::rules::source::SourceLeagueProject`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RonLeagueProject {
    cost_per_player: u32,
    #[serde(default)]
    tech_any_member: Option<String>,
    tiers: Vec<Vec<RonLeagueReward>>,
}

#[derive(Debug, Deserialize)]
enum RonLeagueReward {
    Happiness(u16),
    FreePolicies(u8),
    GoldenAgePoints(u32),
    CultureBoostTurns(u16),
    TourismBoostTurns(u16),
    CityStateInfluence(u16),
    ResearchTurns(u16),
    FreeUnit(String),
    Building(String),
}

impl From<RonLeagueReward> for SourceLeagueReward {
    fn from(r: RonLeagueReward) -> Self {
        match r {
            RonLeagueReward::Happiness(n) => SourceLeagueReward::Happiness(n),
            RonLeagueReward::FreePolicies(n) => SourceLeagueReward::FreePolicies(n),
            RonLeagueReward::GoldenAgePoints(n) => SourceLeagueReward::GoldenAgePoints(n),
            RonLeagueReward::CultureBoostTurns(n) => SourceLeagueReward::CultureBoostTurns(n),
            RonLeagueReward::TourismBoostTurns(n) => SourceLeagueReward::TourismBoostTurns(n),
            RonLeagueReward::CityStateInfluence(n) => SourceLeagueReward::CityStateInfluence(n),
            RonLeagueReward::ResearchTurns(n) => SourceLeagueReward::ResearchTurns(n),
            RonLeagueReward::FreeUnit(k) => SourceLeagueReward::FreeUnit(key(k)),
            RonLeagueReward::Building(k) => SourceLeagueReward::Building(key(k)),
        }
    }
}

fn league_project(l: RonLeagueProject) -> SourceLeagueProject {
    SourceLeagueProject {
        cost_per_player: l.cost_per_player,
        tech_any_member: opt_key(l.tech_any_member),
        tiers: l
            .tiers
            .into_iter()
            .map(|tier| tier.into_iter().map(SourceLeagueReward::from).collect())
            .collect(),
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RonPolicyBranch {
    id: String,
    name: String,
    kind: RonPolicyBranchKind,
    #[serde(default)]
    era_prerequisite: Option<String>,
    #[serde(default)]
    opener: Option<String>,
    #[serde(default)]
    finisher: Option<String>,
    #[serde(default)]
    mutually_exclusive_with: Vec<String>,
    #[serde(default)]
    locked_without_religion: bool,
    #[serde(default)]
    first_adopter_free_policies: u8,
    #[serde(default)]
    second_adopter_free_policies: u8,
}

/// A leader's personality (classic/docs/ai-leaders.md); `why` is for the reader.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RonPersonality {
    nation: String,
    expansion: u8,
    wonders: u8,
    science: u8,
    culture: u8,
    gold: u8,
    religion: u8,
    aggression: u8,
    war_tolerance: u8,
    diplomacy: u8,
    #[allow(dead_code)]
    why: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RonReligion {
    id: String,
    name: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RonCityStateType {
    id: String,
    name: String,
    friends_threshold: i32,
    allies_threshold: i32,
    #[serde(default)]
    minor_trait: Option<RonCityStateTrait>,
    #[serde(default)]
    bonuses: Vec<RonCityStateBonus>,
    #[serde(default)]
    inert: Vec<RonInert>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RonCityStateBonus {
    level: RonCityStateLevel,
    kind: RonCityStateBonusKind,
    #[serde(default)]
    era: Option<String>,
    amount: i32,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RonCityState {
    id: String,
    name: String,
    city_state_type: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RonSpecialist {
    id: String,
    name: String,
    #[serde(default)]
    yields: RonYields,
    #[serde(default)]
    great_person_points: u16,
    #[serde(default)]
    great_person: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RonGreatPerson {
    id: String,
    kind: RonGreatPersonKind,
    unit: String,
    #[serde(default)]
    specialist: Option<String>,
    #[serde(default)]
    improvements: Vec<String>,
    #[serde(default)]
    points_source: crate::ron_types::RonPointsSource,
    #[serde(default)]
    inert: Vec<RonInert>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RonRules {
    #[serde(default)]
    units: Vec<RonUnit>,
    #[serde(default)]
    buildings: Vec<RonBuilding>,
    #[serde(default)]
    technologies: Vec<RonTechnology>,
    #[serde(default)]
    improvements: Vec<RonImprovement>,
    #[serde(default)]
    features: Vec<RonFeature>,
    #[serde(default)]
    resources: Vec<RonResource>,
    #[serde(default)]
    policies: Vec<RonPolicy>,
    #[serde(default)]
    beliefs: Vec<RonBelief>,
    #[serde(default)]
    promotions: Vec<RonPromotion>,
    #[serde(default)]
    nations: Vec<RonNation>,
    #[serde(default)]
    eras: Vec<RonEra>,
    #[serde(default)]
    terrains: Vec<RonTerrain>,
    #[serde(default)]
    natural_wonders: Vec<RonNaturalWonder>,
    #[serde(default)]
    projects: Vec<RonProject>,
    #[serde(default)]
    policy_branches: Vec<RonPolicyBranch>,
    #[serde(default)]
    religions: Vec<RonReligion>,
    #[serde(default)]
    city_state_types: Vec<RonCityStateType>,
    #[serde(default)]
    city_states: Vec<RonCityState>,
    #[serde(default)]
    specialists: Vec<RonSpecialist>,
    #[serde(default)]
    great_people: Vec<RonGreatPerson>,
    #[serde(default)]
    handicaps: Vec<RonHandicap>,
    #[serde(default)]
    speeds: Vec<RonSpeed>,
    #[serde(default)]
    personalities: Vec<RonPersonality>,
    #[serde(default)]
    roles: crate::ron_roles::RonRoles,
}

/// Parse RON text into SourceRules (semantics are in compile, §29).
/// A parse error is ContentError::Parse with a position from ron (we do not stay silent).
pub fn parse_ron(text: &str) -> Result<SourceRules, crate::error::ContentError> {
    let ron: RonRules =
        ron::de::from_str(text).map_err(|e| crate::error::ContentError::Parse(e.to_string()))?;
    let mut nations = Vec::with_capacity(ron.nations.len());
    for c in ron.nations {
        nations.push(nation(c)?);
    }
    Ok(SourceRules {
        units: ron.units.into_iter().map(unit).collect(),
        buildings: ron.buildings.into_iter().map(building).collect(),
        technologies: ron.technologies.into_iter().map(technology).collect(),
        improvements: ron.improvements.into_iter().map(improvement).collect(),
        features: ron.features.into_iter().map(feature).collect(),
        resources: ron.resources.into_iter().map(resource).collect(),
        promotions: ron.promotions.into_iter().map(promotion).collect(),
        nations,
        eras: ron.eras.into_iter().map(era).collect(),
        terrains: ron.terrains.into_iter().map(terrain).collect(),
        natural_wonders: ron.natural_wonders.into_iter().map(natural_wonder).collect(),
        specialists: ron.specialists.into_iter().map(specialist).collect(),
        handicaps: ron.handicaps.into_iter().map(handicap).collect(),
        speeds: ron.speeds.into_iter().map(speed).collect::<Result<_, _>>()?,
        module: classic_rules::classic::tables::ClassicSourceTables {
            policies: ron.policies.into_iter().map(policy).collect(),
            beliefs: ron.beliefs.into_iter().map(belief).collect(),
            projects: ron.projects.into_iter().map(project).collect(),
            policy_branches: ron.policy_branches.into_iter().map(policy_branch).collect(),
            religions: ron.religions.into_iter().map(religion).collect(),
            city_state_types: ron.city_state_types.into_iter().map(city_state_type).collect(),
            city_states: ron.city_states.into_iter().map(city_state).collect(),
            great_people: ron.great_people.into_iter().map(great_person).collect(),
            personalities: ron.personalities.into_iter().map(personality).collect(),
        },
        roles: crate::ron_roles::roles(ron.roles),
    })
}

fn handicap(h: RonHandicap) -> classic_rules::rules::source::SourceHandicap {
    let values = classic_rules::rules::source::HandicapValues {
        module: classic_rules::classic::rows::HandicapFields {
            happiness_default: h.happiness_default,
            extra_happiness_per_luxury: h.extra_happiness_per_luxury,
            num_cities_unhappiness_mod: h.num_cities_unhappiness_mod,
            population_unhappiness_mod: h.population_unhappiness_mod,
            policy_percent: h.policy_percent,
            ai_unhappiness_percent: h.ai_unhappiness_percent,
        },
        production_free_units: h.production_free_units,
        production_free_units_per_city: h.production_free_units_per_city,
        production_free_units_population_percent: h.production_free_units_population_percent,
        gold_free_units: h.gold_free_units,
        route_cost_percent: h.route_cost_percent,
        unit_cost_percent: h.unit_cost_percent,
        building_cost_percent: h.building_cost_percent,
        research_percent: h.research_percent,
        ai_growth_percent: h.ai_growth_percent,
        ai_train_percent: h.ai_train_percent,
        ai_world_train_percent: h.ai_world_train_percent,
        ai_construct_percent: h.ai_construct_percent,
        ai_world_construct_percent: h.ai_world_construct_percent,
        ai_create_percent: h.ai_create_percent,
        ai_world_create_percent: h.ai_world_create_percent,
        ai_building_cost_percent: h.ai_building_cost_percent,
        ai_unit_cost_percent: h.ai_unit_cost_percent,
        ai_unit_supply_percent: h.ai_unit_supply_percent,
        ai_unit_upgrade_percent: h.ai_unit_upgrade_percent,
        ai_per_era_modifier: h.ai_per_era_modifier,
    };
    classic_rules::rules::source::SourceHandicap {
        id: Some(key(h.id)),
        name: h.name,
        index: h.index,
        values,
    }
}

/// Parse RON text into nations (v1 nations.ron: `(nations: [...])`).
/// An unfamiliar start_bias is ContentError::Parse, not a silent "none".
pub fn parse_nations(text: &str) -> Result<Vec<SourceNation>, crate::error::ContentError> {
    let ron: RonNations =
        ron::de::from_str(text).map_err(|e| crate::error::ContentError::Parse(e.to_string()))?;
    let mut out = Vec::with_capacity(ron.nations.len());
    for c in ron.nations {
        out.push(nation(c)?);
    }
    Ok(out)
}

fn unit(u: RonUnit) -> SourceUnit {
    SourceUnit {
        id: Some(key(u.id)),
        movement: u.movement,
        strength: u.strength,
        founder: u.founder,
        builder: u.builder,
        cost: u.cost,
        effects: effects(u.effects),
        required_tech: opt_key(u.required_tech),
        name: u.name,
        class: opt_key(u.class),
        role: u.role.map(Into::into),
        domain: u.domain.into(),
        ranged_strength: u.ranged_strength,
        range: u.range,
        sight: u.sight,
        module: classic_rules::classic::rows::SourceUnitFields {
            faith_cost: u.faith_cost,
            abilities: u.abilities.into_iter().map(Into::into).collect(),
            required_policy: opt_key(u.required_policy),
            builds: keys(u.builds),
            builds_roads: u.builds_roads,
        },
        unbuildable: u.unbuildable,
        obsolete_tech: opt_key(u.obsolete_tech),
        required_resources: key_counts(u.required_resources),
        upgrades_to: keys(u.upgrades_to),
        free_promotions: keys(u.free_promotions),
        era: opt_key(u.era),
        nation: opt_key(u.nation),
        replaces: opt_key(u.replaces),
        requires_buildings: keys(u.requires_buildings),
        combat: u.combat.into(),
        required_project: u.required_project.map(key),
        spaceship_project: u.spaceship_project.map(key),
        inert: inert(u.inert),
    }
}

fn building(b: RonBuilding) -> SourceBuilding {
    SourceBuilding {
        id: Some(key(b.id)),
        cost: b.cost,
        effects: effects(b.effects),
        required_tech: opt_key(b.required_tech),
        wonder: b.wonder,
        name: b.name,
        kind: b.kind.into(),
        class: opt_key(b.class),
        module: classic_rules::classic::rows::SourceBuildingFields {
            faith_cost: b.faith_cost,
            great_work_slots: key_count(b.great_work_slots),
            theming: b.theming.into_iter().map(Into::into).collect(),
            requires_policy_branch: opt_key(b.requires_policy_branch),
            requires_holy_city: b.requires_holy_city,
            faith_purchase_needs_belief: b.faith_purchase_needs_belief,
            abilities: b.abilities.into_iter().map(Into::into).collect(),
        },
        unbuildable: b.unbuildable,
        maintenance: b.maintenance,
        obsolete_tech: opt_key(b.obsolete_tech),
        era: opt_key(b.era),
        hurry_cost_modifier: b.hurry_cost_modifier,
        conquest_survival_pct: b.conquest_survival_pct,
        never_capture: b.never_capture,
        specialist_slots: key_count(b.specialist_slots),
        consumes_resources: key_counts(b.consumes_resources),
        requires_buildings: keys(b.requires_buildings),
        requires_buildings_in_empire: keys(b.requires_buildings_in_empire),
        cost_per_city_pct: b.cost_per_city_pct,
        placement: b.placement.into(),
        max_start_era: opt_key(b.max_start_era),
        free_start_era: opt_key(b.free_start_era),
        nation: opt_key(b.nation),
        replaces: opt_key(b.replaces),
        inert: inert(b.inert),
    }
}

fn technology(t: RonTechnology) -> SourceTechnology {
    SourceTechnology {
        id: Some(key(t.id)),
        era_name: t.era_name,
        unlocks_buildings: keys(t.unlocks_buildings),
        cost: t.cost,
        requires: keys(t.requires),
        effects: effects(t.effects),
        name: t.name,
        era: opt_key(t.era),
        grid: t.grid,
        unlocks_units: keys(t.unlocks_units),
        unlocks_improvements: keys(t.unlocks_improvements),
        unlocks_projects: keys(t.unlocks_projects),
        unlocks_actions: keys(t.unlocks_actions),
        repeatable: t.repeatable,
        allows_embarking: t.allows_embarking,
        embarked_all_water: t.embarked_all_water,
        allows_embassy: t.allows_embassy,
        allows_open_borders: t.allows_open_borders,
        allows_defensive_pact: t.allows_defensive_pact,
        allows_research_agreement: t.allows_research_agreement,
        module: classic_rules::classic::rows::TechnologyFields {
            allows_world_congress: t.allows_world_congress,
            triggers_archaeological_sites: t.triggers_archaeological_sites,
        },
        map_visible: t.map_visible,
        bridge_building: t.bridge_building,
        road_move_change: t.road_move_change,
        inert: inert(t.inert),
    }
}

fn improvement(i: RonImprovement) -> SourceImprovement {
    let fresh_water_yields = i
        .fresh_water_yields
        .into_iter()
        .map(|y| SourceTechYield {
            tech: key(y.tech),
            yield_kind: y.yield_kind.into(),
            amount: y.amount,
            fresh_water: y.fresh_water.into(),
        })
        .collect();
    SourceImprovement {
        id: Some(key(i.id)),
        work_turns: i.work_turns,
        food_bonus: i.food_bonus,
        prod_bonus: i.prod_bonus,
        river_food: i.river_food,
        required_tech: opt_key(i.required_tech),
        name: i.name,
        kind: i.kind.into(),
        unbuildable: i.unbuildable,
        yields: i.yields.into(),
        work_time: i.work_time,
        build_action: opt_key(i.build_action),
        valid_terrains: keys(i.valid_terrains),
        valid_features: keys(i.valid_features),
        resources: keys(i.resources),
        fresh_water_yields,
        defense_pct: i.defense_pct,
        pillage_gold: i.pillage_gold,
        culture_bomb_radius: i.culture_bomb_radius,
        nearby_enemy_damage: i.nearby_enemy_damage,
        culture_per_era: i.culture_per_era,
        nation: opt_key(i.nation),
        module: classic_rules::classic::rows::ImprovementFields {
            created_by_great_person: i.created_by_great_person,
            city_state_luxury_copies: i.city_state_luxury_copies,
        },
        placement: i.placement.into(),
        adjacent_mountain_yields: i.adjacent_mountain_yields.into(),
        removes_resource: i.removes_resource,
        adjacent_same_culture: i.adjacent_same_culture,
        inert: inert(i.inert),
    }
}

fn feature(f: RonFeature) -> SourceFeature {
    SourceFeature {
        id: Some(key(f.id)),
        chop_turns: f.chop_turns,
        prod_bonus: f.prod_bonus,
        chop_grant: f.chop_grant,
        name: f.name,
        yields: f.yields.into(),
        yield_not_additive: f.yield_not_additive,
        movement_cost: f.movement_cost,
        defense_pct: f.defense_pct,
        impassable: f.impassable,
        rough: f.rough,
        fresh_water: f.fresh_water,
        no_improvement: f.no_improvement,
        requires_river: f.requires_river,
        requires_flatlands: f.requires_flatlands,
        see_through: f.see_through,
        valid_terrains: keys(f.valid_terrains),
        unremovable: f.unremovable,
        removal: f.removal.map(Into::into),
        inert: inert(f.inert),
    }
}

fn resource(r: RonResource) -> SourceResource {
    SourceResource {
        id: Some(key(r.id)),
        food_bonus: r.food_bonus,
        prod_bonus: r.prod_bonus,
        gold_bonus: r.gold_bonus,
        requires_improvement: opt_key(r.requires_improvement),
        required_tech: opt_key(r.required_tech),
        terrains: r.terrains,
        density: r.density,
        sea_density: r.sea_density,
        name: r.name,
        class: r.class.map(Into::into),
        yields: r.yields.into(),
        module: classic_rules::classic::rows::SourceResourceFields {
            happiness: r.happiness,
            only_city_states: r.only_city_states,
            revealed_by_policy: opt_key(r.revealed_by_policy),
        },
        improvements: keys(r.improvements),
        tradeable_with_tech: opt_key(r.tradeable_with_tech),
        valid_terrains: keys(r.valid_terrains),
        valid_features: keys(r.valid_features),
        hills: r.hills,
        flatlands: r.flatlands,
        in_standard_game: r.in_standard_game,
        nation: opt_key(r.nation),
        quantities: r.quantities,
        improved_yields: r
            .improved_yields
            .into_iter()
            .map(|(k, y)| (key(k), y.into()))
            .collect(),
        wonder_production: r.wonder_production.map(|w| SourceWonderProduction {
            percent: w.percent,
            before_era: opt_key(w.before_era),
        }),
        inert: inert(r.inert),
    }
}

/// Marble: `wonder_production: Some((percent: 15, before_era: Some("core:era/medieval")))`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RonWonderProduction {
    percent: i32,
    #[serde(default)]
    before_era: Option<String>,
}

fn policy(p: RonPolicy) -> SourcePolicy {
    SourcePolicy {
        id: Some(key(p.id)),
        effects: effects(p.effects),
        name: p.name,
        branch: opt_key(p.branch),
        role: p.role.into(),
        prerequisites: keys(p.prerequisites),
        grid: p.grid,
        text: p.text,
        inert: inert(p.inert),
    }
}

fn belief(b: RonBelief) -> SourceBelief {
    SourceBelief {
        id: Some(key(b.id)),
        effects: effects(b.effects),
        name: b.name,
        category: b.category.map(Into::into),
        text: b.text,
        inert: inert(b.inert),
    }
}

fn promotion(p: RonPromotion) -> SourcePromotion {
    SourcePromotion {
        id: Some(key(p.id)),
        effects: effects(p.effects),
        name: p.name,
        requires: keys(p.requires),
        requires_any_of: keys(p.requires_any_of),
        choosable: p.choosable,
        lost_with_upgrade: p.lost_with_upgrade,
        required_tech: opt_key(p.required_tech),
        unit_roles: p.unit_roles.into_iter().map(Into::into).collect(),
        civilian_units: keys(p.civilian_units),
        inert: inert(p.inert),
    }
}

fn nation(c: RonNation) -> Result<SourceNation, crate::error::ContentError> {
    let start_bias = match c.start_bias.as_str() {
        "none" => StartBias::None,
        "coastal" => StartBias::Coastal,
        other => {
            return Err(crate::error::ContentError::Parse(format!(
                "unknown start_bias {:?} (expected \"none\"|\"coastal\")",
                other
            )));
        }
    };
    Ok(SourceNation {
        id: Some(key(c.id)),
        name: c.name,
        leader: c.leader,
        color: (c.color.r, c.color.g, c.color.b),
        start_bias,
        effects: effects(c.effects),
        adjective: c.adjective,
        ability_name: c.ability_name,
        unique_units: keys(c.unique_units),
        unique_buildings: keys(c.unique_buildings),
        unique_improvements: keys(c.unique_improvements),
        start_regions: c.start_regions.into(),
        free_techs: keys(c.free_techs),
        free_units: key_counts(c.free_units),
        start_units: c.start_units.into_iter().map(|(slot, unit)| (slot.into(), key(unit))).collect(),
        module: classic_rules::classic::rows::SourceNationFields {
            preferred_religions: keys(c.preferred_religions),
            spy_names: c.spy_names,
        },
        inert: inert(c.inert),
        city_names: c.city_names,
    })
}

fn era(e: RonEra) -> SourceEra {
    SourceEra {
        id: Some(key(e.id)),
        name: e.name,
        index: e.index,
        starting_defense_units: e.starting_defense_units,
        starting_worker_units: e.starting_worker_units,
        starting_explore_units: e.starting_explore_units,
        starting_gold: e.starting_gold,
        research_agreement_cost: e.research_agreement_cost,
        embarked_unit_defense: e.embarked_unit_defense,
        module: classic_rules::classic::rows::EraFields {
            faith_cost_multiplier: e.faith_cost_multiplier,
        },
    }
}

fn terrain(t: RonTerrain) -> SourceTerrain {
    SourceTerrain {
        id: Some(key(t.id)),
        name: t.name,
        yields: t.yields.into(),
        hills_yields: t.hills_yields.into(),
        movement_cost: t.movement_cost,
        impassable: t.impassable,
        defense_pct: t.defense_pct,
        water: t.water,
        can_found_city: t.can_found_city,
        map_code: t.map_code,
    }
}

fn natural_wonder(w: RonNaturalWonder) -> SourceNaturalWonder {
    SourceNaturalWonder {
        id: Some(key(w.id)),
        name: w.name,
        yields: w.yields.into(),
        impassable: w.impassable,
        movement_cost: w.movement_cost,
        defense_pct: w.defense_pct,
        module: classic_rules::classic::rows::NaturalWonderFields {
            happiness: w.happiness,
        },
        first_finder_gold: w.first_finder_gold,
        fresh_water: w.fresh_water,
        valid_terrains: keys(w.valid_terrains),
        adjacent_unit_free_promotion: opt_key(w.adjacent_unit_free_promotion),
        occurrence_frequency: w.occurrence_frequency,
        rough: w.rough,
        no_improvement: w.no_improvement,
        placement: w.placement.into(),
        inert: inert(w.inert),
    }
}

fn project(p: RonProject) -> SourceProject {
    SourceProject {
        id: Some(key(p.id)),
        name: p.name,
        kind: Some(p.kind.into()),
        cost: p.cost,
        unbuildable: p.unbuildable,
        required_tech: opt_key(p.required_tech),
        prerequisite_projects: key_counts(p.prerequisite_projects),
        max_global_instances: p.max_global_instances,
        max_team_instances: p.max_team_instances,
        victory: p.victory,
        allows_nukes: p.allows_nukes,
        league: p.league.map(league_project),
        inert: inert(p.inert),
    }
}

fn policy_branch(b: RonPolicyBranch) -> SourcePolicyBranch {
    SourcePolicyBranch {
        id: Some(key(b.id)),
        name: b.name,
        kind: Some(b.kind.into()),
        era_prerequisite: opt_key(b.era_prerequisite),
        opener: opt_key(b.opener),
        finisher: opt_key(b.finisher),
        mutually_exclusive_with: keys(b.mutually_exclusive_with),
        locked_without_religion: b.locked_without_religion,
        first_adopter_free_policies: b.first_adopter_free_policies,
        second_adopter_free_policies: b.second_adopter_free_policies,
    }
}

fn personality(p: RonPersonality) -> classic_rules::rules::source::SourcePersonality {
    classic_rules::rules::source::SourcePersonality {
        nation: key(p.nation),
        personality: classic_rules::rules::content::Personality {
            expansion: p.expansion,
            wonders: p.wonders,
            science: p.science,
            culture: p.culture,
            gold: p.gold,
            religion: p.religion,
            aggression: p.aggression,
            war_tolerance: p.war_tolerance,
            diplomacy: p.diplomacy,
        },
    }
}

fn religion(r: RonReligion) -> SourceReligion {
    SourceReligion {
        id: Some(key(r.id)),
        name: r.name,
    }
}

fn city_state_type(t: RonCityStateType) -> SourceCityStateType {
    SourceCityStateType {
        id: Some(key(t.id)),
        name: t.name,
        friends_threshold: t.friends_threshold,
        allies_threshold: t.allies_threshold,
        minor_trait: t.minor_trait.map(Into::into),
        bonuses: t
            .bonuses
            .into_iter()
            .map(|b| classic_rules::rules::source::SourceCityStateBonus {
                level: b.level.into(),
                kind: b.kind.into(),
                era: b.era.map(key),
                amount: b.amount,
            })
            .collect(),
        inert: inert(t.inert),
    }
}

fn city_state(c: RonCityState) -> SourceCityState {
    SourceCityState {
        id: Some(key(c.id)),
        name: c.name,
        city_state_type: Some(key(c.city_state_type)),
    }
}

fn specialist(s: RonSpecialist) -> SourceSpecialist {
    SourceSpecialist {
        id: Some(key(s.id)),
        name: s.name,
        yields: s.yields.into(),
        module: classic_rules::classic::rows::SourceSpecialistFields {
            great_person_points: s.great_person_points,
            great_person: opt_key(s.great_person),
        },
    }
}

fn great_person(g: RonGreatPerson) -> SourceGreatPerson {
    SourceGreatPerson {
        id: Some(key(g.id)),
        kind: Some(g.kind.into()),
        unit: Some(key(g.unit)),
        specialist: opt_key(g.specialist),
        improvements: keys(g.improvements),
        points_source: g.points_source.into(),
        inert: inert(g.inert),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn garbage_fails_loudly() {
        assert!(parse_ron("not ron at all (((").is_err());
        // An unknown field is an error too (typos do not slip through).
        assert!(parse_ron("(units: [(idd: \"x\")])").is_err());
        let misspelt = "(units: [(id: \"core:unit/x\", movement: 2, strength: 1, cost: 5, \
                        required_tech: None, sigth: 2)])";
        assert!(parse_ron(misspelt).is_err(), "misspelt optional field");
    }

    #[test]
    fn minimal_unit_parses() {
        let rules = parse_ron(
            "(units: [(id: \"core:unit/scout\", movement: 3, strength: 2, founder: false, builder: false, cost: 20, effects: [])])",
        )
        .unwrap();
        assert_eq!(rules.units.len(), 1);
        assert_eq!(rules.units[0].id.as_ref().unwrap().0, "core:unit/scout");
        assert_eq!(rules.units[0].cost, 20);
        assert!(!rules.units[0].founder);
    }
}
