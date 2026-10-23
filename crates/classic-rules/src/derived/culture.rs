//! Culture and social policy formulas (classic/docs/classic/rules.md): one set
//! of read-only functions for the turn resolution, validation, the HUD and
//! the AI (platform/structure.md §36: the client never re-derives game numbers).
//! Nothing here is stored: every answer is a function of the authoritative
//! world and the rules.

use crate::world::player::player_era;
use crate::classic::effects::ClassicCost;
use crate::effects::player::player_modifiers;
use crate::ids::{PlayerId, PolicyId};
use crate::rules::content::{
    BuildingAbility, PolicyBranchId, PolicyBranchKind, PolicyRole,
};
use crate::rules::effects::CostTarget;
use crate::rules::CompiledRuleset;
use crate::world::culture::PlayerCulture;
use crate::world::size::WorldSize;
use crate::world::state::AuthWorld;

/// Culture cost of the first policy. Unciv: PolicyManager.getPolicyCultureCost, equal.
pub const POLICY_CULTURE_COST_BASE: i64 = 25;
/// Policies paid so far are multiplied by this before the exponent. Unciv:
/// PolicyManager.getPolicyCultureCost, equal.
pub const POLICY_CULTURE_COST_PER_POLICY: u64 = 3;
/// Exponent of the policy cost curve (double precision, the power not
/// truncated). Unciv: PolicyManager.getPolicyCultureCost, equal.
pub const POLICY_CULTURE_COST_EXPONENT: f64 = 2.01;
/// Largest total discount of the policy cost (percent, negative). Own value
/// (no Unciv equivalent: Unciv multiplies its policy-cost uniques without a floor).
pub const POLICY_COST_MAX_DISCOUNT_PERCENT: i32 = -75;
/// The policy cost is rounded to the nearest whole culture and then down to a
/// multiple of this. Unciv: PolicyManager.getPolicyCultureCost, equal.
pub const POLICY_CULTURE_COST_ROUND_TO: i64 = 5;

/// Culture cost of a city's next border tile, the factor of
/// `factor × (claimed + offset)^exponent`. Unciv:
/// CityExpansionManager.getCultureToNextTile, equal (6).
pub const CULTURE_TO_NEXT_TILE_FACTOR: f64 = 6.0;
/// Added to the tiles claimed beyond the first ring before the exponent.
/// Unciv: CityExpansionManager.getCultureToNextTile, equal (1.4813).
pub const CULTURE_TO_NEXT_TILE_OFFSET: f64 = 1.4813;
/// Exponent of the border tile cost curve. Unciv:
/// CityExpansionManager.getCultureToNextTile, equal (1.3).
pub const CULTURE_TO_NEXT_TILE_EXPONENT: f64 = 1.3;
/// Largest total discount of the border tile cost (percent, negative). Own
/// value (no Unciv equivalent: Unciv multiplies its border-growth uniques
/// without a floor).
pub const CULTURE_TO_NEXT_TILE_MAX_DISCOUNT_PERCENT: i32 = -85;


/// The order of an era (0 for a ruleset without eras).
pub fn era_index(rules: &CompiledRuleset, era: crate::rules::content::EraId) -> u8 {
    rules.eras.get(era.0 as usize).map(|e| e.index).unwrap_or(0)
}



/// Cities that count for the policy and research cost: the player's
/// non-puppet cities now (may be 0). Unciv: PolicyManager.getPolicyCultureCost,
/// TechManager.costOfTech (`cities.count { !it.isPuppet }`), equal.
pub fn effective_cities(auth: &AuthWorld, player: PlayerId) -> u32 {
    auth.indexes()
        .cities_of(player)
        .iter()
        .filter_map(|c| auth.cities().get(*c))
        .filter(|c| c.status != crate::world::city::CityStatus::Puppet)
        .count() as u32
}

/// Culture cost of the next policy of `player` (rules.md).
pub fn next_policy_cost(auth: &AuthWorld, rules: &CompiledRuleset, player: PlayerId) -> u32 {
    let mods = player_modifiers(auth, rules, player);
    let inputs = PolicyCostInputs {
        paid_policies: auth.module().culture().of(player).paid_policies(),
        cities: effective_cities(auth, player),
        per_city_percent: crate::classic::world_size::policy_cost_per_city(WorldSize::of(auth.map().dimensions)),
        per_city_discount: mods.cost_percent[CostTarget::Module(ClassicCost::PolicyCityPenalty).index()],
        cost_percent: mods.cost_percent[CostTarget::Module(ClassicCost::Policies).index()],
        difficulty_percent: auth.settings().handicap(rules, player).module.policy_percent,
    };
    policy_cost(inputs)
}

/// Inputs of the policy cost formula.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PolicyCostInputs {
    /// Policies owned minus the free ones received.
    pub paid_policies: u32,
    /// Non-puppet cities (`effective_cities`).
    pub cities: u32,
    /// Hundredths of a percent added to the cost per city beyond the first
    /// (depends on the map size: 1000 = 10 %).
    pub per_city_percent: i64,
    /// Discount of the per-city percent (Representation −33).
    pub per_city_discount: i32,
    /// Sum of the policy cost modifiers (floored at −75).
    pub cost_percent: i32,
    /// The player's difficulty policy percent (the default level 100; an AI
    /// 100: Unciv applies the difficulty's policy cost to humans only).
    pub difficulty_percent: i32,
}

/// The next policy's culture at Standard speed. Unciv:
/// PolicyManager.getPolicyCultureCost, equal: `25 + (3 n)^2.01`, times the
/// cost and difficulty percents, times `1 + per-city × (cities − 1)` (the
/// per-city percent lowered by Representation-like discounts), rounded to
/// the nearest whole and then down to a multiple of 5.
pub fn policy_cost(input: PolicyCostInputs) -> u32 {
    let exponent_base = f64::from(input.paid_policies) * POLICY_CULTURE_COST_PER_POLICY as f64;
    let mut cost = POLICY_CULTURE_COST_BASE as f64 + crate::numeric::pow(exponent_base, POLICY_CULTURE_COST_EXPONENT);
    let mut per_city = input.per_city_percent as f64 / 10_000.0;
    if input.per_city_discount != 0 {
        per_city *= f64::from(100 + input.per_city_discount) / 100.0;
    }
    let city_modifier = per_city * (f64::from(input.cities) - 1.0);
    let cost_percent = input.cost_percent.max(POLICY_COST_MAX_DISCOUNT_PERCENT);
    cost *= f64::from(100 + cost_percent) / 100.0;
    cost *= f64::from(input.difficulty_percent.max(1)) / 100.0;
    let mut cost = (cost * (1.0 + city_modifier) + 0.5).floor() as i64;
    cost -= cost % POLICY_CULTURE_COST_ROUND_TO;
    cost.clamp(0, i64::from(u32::MAX)) as u32
}

/// Culture for the next border tile of a city that has claimed `level`
/// tiles beyond its first ring. Unciv: CityExpansionManager.getCultureToNextTile,
/// equal: `6 (level + 1.4813)^1.3`, times the border cost percent, rounded
/// to the nearest whole. `exponent_percent`: the percent change of the
/// exponent (own extension, no Unciv equivalent; no content of this ruleset
/// uses it); `cost_percent`: the sum of the border culture cost modifiers.
pub fn border_cost(level: u32, exponent_percent: i32, cost_percent: i32) -> u32 {
    let mut exponent = CULTURE_TO_NEXT_TILE_EXPONENT;
    if exponent_percent != 0 {
        exponent = exponent * f64::from(100 + exponent_percent) / 100.0;
    }
    let mut cost = CULTURE_TO_NEXT_TILE_FACTOR
        * crate::numeric::pow(f64::from(level) + CULTURE_TO_NEXT_TILE_OFFSET, exponent);
    if cost_percent != 0 {
        let m = cost_percent.max(CULTURE_TO_NEXT_TILE_MAX_DISCOUNT_PERCENT);
        cost = cost * f64::from(100 + m) / 100.0;
    }
    ((cost + 0.5).floor() as i64).max(1) as u32
}

/// How a policy (or a branch) would be paid for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Payment {
    Culture(u32),
    FreePolicy,
    FreeTenet,
}

/// Why a policy cannot be adopted now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolicyBlock {
    UnknownPlayer,
    UnknownPolicy,
    /// Openers and finishers come with their branch, not by choice.
    NotChoosable,
    BranchLocked,
    AlreadyAdopted,
    MissingPrerequisite,
    /// A tenet of another ideology than the player's.
    OtherIdeology,
    /// The tier rule: more tenets of the tier below are needed.
    TierLocked,
    NotEnoughCulture,
}

impl PolicyBlock {
    pub const ALL: [PolicyBlock; 9] = [
        PolicyBlock::UnknownPlayer,
        PolicyBlock::UnknownPolicy,
        PolicyBlock::NotChoosable,
        PolicyBlock::BranchLocked,
        PolicyBlock::AlreadyAdopted,
        PolicyBlock::MissingPrerequisite,
        PolicyBlock::OtherIdeology,
        PolicyBlock::TierLocked,
        PolicyBlock::NotEnoughCulture,
    ];

    pub fn reason(self) -> &'static str {
        match self {
            PolicyBlock::UnknownPlayer => "unknown player",
            PolicyBlock::UnknownPolicy => "unknown policy",
            PolicyBlock::NotChoosable => "comes with its branch",
            PolicyBlock::BranchLocked => "branch not unlocked",
            PolicyBlock::AlreadyAdopted => "already adopted",
            PolicyBlock::MissingPrerequisite => "prerequisite missing",
            PolicyBlock::OtherIdeology => "not your ideology",
            PolicyBlock::TierLocked => "needs three tenets of the tier below",
            PolicyBlock::NotEnoughCulture => "not enough culture",
        }
    }
}

/// Whether `player` may adopt `policy` now, and how it would be paid.
pub fn policy_status(
    auth: &AuthWorld,
    rules: &CompiledRuleset,
    player: PlayerId,
    policy: PolicyId,
) -> Result<Payment, PolicyBlock> {
    if !auth.players().is_alive(player) {
        return Err(PolicyBlock::UnknownPlayer);
    }
    let cost = next_policy_cost(auth, rules, player);
    policy_status_at(rules, auth.module().culture().of(player), policy, cost)
}

/// `policy_status` with the next policy cost already known.
fn policy_status_at(
    rules: &CompiledRuleset,
    state: &PlayerCulture,
    policy: PolicyId,
    cost: u32,
) -> Result<Payment, PolicyBlock> {
    let def = rules
        .module.policies
        .get(policy.0 as usize)
        .ok_or(PolicyBlock::UnknownPolicy)?;
    let branch = def.branch.ok_or(PolicyBlock::NotChoosable)?;
    let tier = match def.role {
        PolicyRole::Policy => None,
        PolicyRole::Tenet(tier) => Some(tier),
        PolicyRole::Opener | PolicyRole::Finisher => return Err(PolicyBlock::NotChoosable),
    };
    if state.has_policy(policy) {
        return Err(PolicyBlock::AlreadyAdopted);
    }
    if tier.is_some() && state.ideology != Some(branch) {
        return Err(PolicyBlock::OtherIdeology);
    }
    if !state.has_branch(branch) {
        return Err(PolicyBlock::BranchLocked);
    }
    for required in &def.prerequisites {
        if !state.has_policy(*required) {
            return Err(PolicyBlock::MissingPrerequisite);
        }
    }
    if let Some(tier) = tier {
        if !tier_open(rules, state, branch, tier) {
            return Err(PolicyBlock::TierLocked);
        }
        if state.free_tenets > 0 {
            return Ok(Payment::FreeTenet);
        }
    }
    payment(state, cost).ok_or(PolicyBlock::NotEnoughCulture)
}

/// Tenets of the tier below a tier needs before its first tenet may be taken: three. Own design
/// (Unciv has no ideologies): the first adopter's free tenets open the second tier at once, the
/// second tier's three open the third.
pub const TENETS_TO_OPEN_NEXT_TIER: u32 = 3;

/// A tenet of tier L > 1 may be taken once the player holds [`TENETS_TO_OPEN_NEXT_TIER`] tenets of
/// tier L − 1.
fn tier_open(
    rules: &CompiledRuleset,
    state: &PlayerCulture,
    branch: PolicyBranchId,
    tier: u8,
) -> bool {
    if tier <= 1 {
        return true;
    }
    tenets_of_tier(rules, state, branch, tier - 1) >= TENETS_TO_OPEN_NEXT_TIER
}

/// Tenets of `tier` of `branch` the player adopted.
pub fn tenets_of_tier(
    rules: &CompiledRuleset,
    state: &PlayerCulture,
    branch: PolicyBranchId,
    tier: u8,
) -> u32 {
    let mut count = 0;
    for p in &state.policies {
        let Some(def) = rules.module.policies.get(p.0 as usize) else {
            continue;
        };
        if def.branch == Some(branch) && def.role == PolicyRole::Tenet(tier) {
            count += 1;
        }
    }
    count
}

/// A free policy first, else the culture cost if the pool covers it.
fn payment(state: &PlayerCulture, cost: u32) -> Option<Payment> {
    if state.free_policies > 0 {
        return Some(Payment::FreePolicy);
    }
    if state.culture >= cost {
        Some(Payment::Culture(cost))
    } else {
        None
    }
}

/// Why a branch cannot be unlocked now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BranchBlock {
    UnknownPlayer,
    UnknownBranch,
    /// Ideologies are chosen, not unlocked with culture.
    Ideology,
    AlreadyUnlocked,
    EraTooEarly,
    /// A mutually exclusive branch is unlocked.
    Excluded,
    NotEnoughCulture,
}

impl BranchBlock {
    pub const ALL: [BranchBlock; 7] = [
        BranchBlock::UnknownPlayer,
        BranchBlock::UnknownBranch,
        BranchBlock::Ideology,
        BranchBlock::AlreadyUnlocked,
        BranchBlock::EraTooEarly,
        BranchBlock::Excluded,
        BranchBlock::NotEnoughCulture,
    ];

    pub fn reason(self) -> &'static str {
        match self {
            BranchBlock::UnknownPlayer => "unknown player",
            BranchBlock::UnknownBranch => "unknown branch",
            BranchBlock::Ideology => "ideologies are chosen",
            BranchBlock::AlreadyUnlocked => "already unlocked",
            BranchBlock::EraTooEarly => "era too early",
            BranchBlock::Excluded => "excluded by another branch",
            BranchBlock::NotEnoughCulture => "not enough culture",
        }
    }
}

/// Whether `player` may unlock `branch` now (rules.md).
pub fn branch_status(
    auth: &AuthWorld,
    rules: &CompiledRuleset,
    player: PlayerId,
    branch: PolicyBranchId,
) -> Result<Payment, BranchBlock> {
    if !auth.players().is_alive(player) {
        return Err(BranchBlock::UnknownPlayer);
    }
    let cost = next_policy_cost(auth, rules, player);
    let era = player_era(auth, rules, player);
    branch_status_at(rules, auth.module().culture().of(player), branch, cost, era)
}

/// `branch_status` with the next policy cost and the player's era known.
fn branch_status_at(
    rules: &CompiledRuleset,
    state: &PlayerCulture,
    branch: PolicyBranchId,
    cost: u32,
    player_era: u8,
) -> Result<Payment, BranchBlock> {
    let def = rules
        .module.policy_branches
        .get(branch.0 as usize)
        .ok_or(BranchBlock::UnknownBranch)?;
    if def.kind == PolicyBranchKind::Ideology {
        return Err(BranchBlock::Ideology);
    }
    if state.has_branch(branch) {
        return Err(BranchBlock::AlreadyUnlocked);
    }
    if let Some(era) = def.era_prerequisite {
        if player_era < era_index(rules, era) {
            return Err(BranchBlock::EraTooEarly);
        }
    }
    if def
        .mutually_exclusive_with
        .iter()
        .any(|b| state.has_branch(*b))
    {
        return Err(BranchBlock::Excluded);
    }
    payment(state, cost).ok_or(BranchBlock::NotEnoughCulture)
}

/// Why an ideology cannot be chosen now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdeologyBlock {
    UnknownPlayer,
    NotAnIdeology,
    AlreadyChosen,
    /// Neither past the ideology era nor enough triggering buildings.
    TooEarly,
}

impl IdeologyBlock {
    pub const ALL: [IdeologyBlock; 4] = [
        IdeologyBlock::UnknownPlayer,
        IdeologyBlock::NotAnIdeology,
        IdeologyBlock::AlreadyChosen,
        IdeologyBlock::TooEarly,
    ];

    pub fn reason(self) -> &'static str {
        match self {
            IdeologyBlock::UnknownPlayer => "unknown player",
            IdeologyBlock::NotAnIdeology => "not an ideology",
            IdeologyBlock::AlreadyChosen => "ideology already chosen",
            IdeologyBlock::TooEarly => "needs a later era or more factories",
        }
    }
}

/// Whether `player` may choose `ideology` now; Ok holds the free tenets it
/// would get (first adopter 2, second 1, rules.md).
pub fn ideology_status(
    auth: &AuthWorld,
    rules: &CompiledRuleset,
    player: PlayerId,
    ideology: PolicyBranchId,
) -> Result<u8, IdeologyBlock> {
    if !auth.players().is_alive(player) {
        return Err(IdeologyBlock::UnknownPlayer);
    }
    let def = rules
        .module.policy_branches
        .get(ideology.0 as usize)
        .ok_or(IdeologyBlock::NotAnIdeology)?;
    if def.kind != PolicyBranchKind::Ideology {
        return Err(IdeologyBlock::NotAnIdeology);
    }
    if auth.module().culture().of(player).ideology.is_some() {
        return Err(IdeologyBlock::AlreadyChosen);
    }
    if !time_to_choose_ideology(auth, rules, player, def.era_prerequisite) {
        return Err(IdeologyBlock::TooEarly);
    }
    let free = match auth.module().culture().adopters_of(ideology) {
        0 => def.first_adopter_free_policies,
        1 => def.second_adopter_free_policies,
        _ => 0,
    };
    Ok(free)
}

/// The player may choose an ideology: past the ideology era (Industrial), or
/// owns enough buildings that trigger the choice (2 Factories).
pub fn time_to_choose_ideology(
    auth: &AuthWorld,
    rules: &CompiledRuleset,
    player: PlayerId,
    era: Option<crate::rules::content::EraId>,
) -> bool {
    if let Some(era) = era {
        if player_era(auth, rules, player) > era_index(rules, era) {
            return true;
        }
    }
    for (i, def) in rules.buildings.iter().enumerate() {
        for ability in &def.module.abilities {
            let BuildingAbility::TriggersIdeologyChoice(needed) = *ability else {
                continue;
            };
            let owned = buildings_owned(auth, player, crate::ids::BuildingKindId(i as u16));
            if needed > 0 && owned >= u32::from(needed) {
                return true;
            }
        }
    }
    false
}

/// How many of the player's cities have the building.
pub fn buildings_owned(
    auth: &AuthWorld,
    player: PlayerId,
    building: crate::ids::BuildingKindId,
) -> u32 {
    let mut count = 0;
    for handle in auth.indexes().cities_of(player) {
        if let Some(core) = auth.cities().get(*handle) {
            if core.buildings.contains(&building) {
                count += 1;
            }
        }
    }
    count
}

/// The branch is complete: every choosable policy of it is adopted.
pub fn branch_complete(
    rules: &CompiledRuleset,
    state: &PlayerCulture,
    branch: PolicyBranchId,
) -> bool {
    let mut any = false;
    for (i, def) in rules.module.policies.iter().enumerate() {
        if def.branch != Some(branch) || def.role != PolicyRole::Policy {
            continue;
        }
        any = true;
        if !state.has_policy(PolicyId(i as u16)) {
            return false;
        }
    }
    any
}

/// What a player knows of its own culture and policies (HUD, Civics
/// screen, AI observation): one evaluation of the cost, then every branch,
/// policy and ideology checked against it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CultureOverview {
    /// Culture in the pool.
    pub culture: u32,
    /// Culture the cities add next turn.
    pub per_turn: u32,
    pub lifetime: u64,
    pub next_policy_cost: u32,
    pub free_policies: u16,
    pub free_tenets: u16,
    pub policies: Vec<PolicyId>,
    pub branches: Vec<PolicyBranchId>,
    pub ideology: Option<PolicyBranchId>,
    /// Branches the player may unlock now.
    pub unlockable: Vec<PolicyBranchId>,
    /// Policies and tenets the player may adopt now, table order.
    pub adoptable: Vec<PolicyId>,
    /// Ideologies the player may choose now, with the free tenets each
    /// would give.
    pub ideologies: Vec<(PolicyBranchId, u8)>,
}

/// `per_turn`: the culture of the player's cities this turn (the caller
/// has the city incomes; `culture_per_turn` computes them).
pub fn culture_overview(
    auth: &AuthWorld,
    rules: &CompiledRuleset,
    player: PlayerId,
    per_turn: u32,
) -> CultureOverview {
    let state = auth.module().culture().of(player);
    let cost = next_policy_cost(auth, rules, player);
    let era = player_era(auth, rules, player);
    let alive = auth.players().is_alive(player);
    let mut out = CultureOverview {
        culture: state.culture,
        per_turn,
        lifetime: state.lifetime,
        next_policy_cost: cost,
        free_policies: state.free_policies,
        free_tenets: state.free_tenets,
        policies: state.policies.clone(),
        branches: state.branches.clone(),
        ideology: state.ideology,
        ..CultureOverview::default()
    };
    if !alive {
        return out;
    }
    for i in 0..rules.module.policy_branches.len() {
        let branch = PolicyBranchId(i as u16);
        if branch_status_at(rules, state, branch, cost, era).is_ok() {
            out.unlockable.push(branch);
        }
        if let Ok(free) = ideology_status(auth, rules, player, branch) {
            out.ideologies.push((branch, free));
        }
    }
    for i in 0..rules.module.policies.len() {
        let policy = PolicyId(i as u16);
        if policy_status_at(rules, state, policy, cost).is_ok() {
            out.adoptable.push(policy);
        }
    }
    out
}

/// Status of every policy for `player` (index = PolicyId), one cost
/// evaluation for all: what the Civics screen shows on each card.
pub fn policy_statuses(
    auth: &AuthWorld,
    rules: &CompiledRuleset,
    player: PlayerId,
) -> Vec<Result<Payment, PolicyBlock>> {
    if !auth.players().is_alive(player) {
        return vec![Err(PolicyBlock::UnknownPlayer); rules.module.policies.len()];
    }
    let state = auth.module().culture().of(player);
    let cost = next_policy_cost(auth, rules, player);
    (0..rules.module.policies.len())
        .map(|i| policy_status_at(rules, state, PolicyId(i as u16), cost))
        .collect()
}

/// Status of every branch for `player` (index = PolicyBranchId); for an
/// ideology, `ideology_status` answers instead.
pub fn branch_statuses(
    auth: &AuthWorld,
    rules: &CompiledRuleset,
    player: PlayerId,
) -> Vec<Result<Payment, BranchBlock>> {
    if !auth.players().is_alive(player) {
        return vec![Err(BranchBlock::UnknownPlayer); rules.module.policy_branches.len()];
    }
    let state = auth.module().culture().of(player);
    let cost = next_policy_cost(auth, rules, player);
    let era = player_era(auth, rules, player);
    (0..rules.module.policy_branches.len())
        .map(|i| branch_status_at(rules, state, PolicyBranchId(i as u16), cost, era))
        .collect()
}

/// Culture the player's cities make in a turn (the pool's income).
pub fn culture_per_turn(auth: &AuthWorld, rules: &CompiledRuleset, player: PlayerId) -> u32 {
    let mut total = 0u32;
    for handle in auth.indexes().cities_of(player) {
        if let Some(core) = auth.cities().get(*handle) {
            total = total
                .saturating_add(crate::derived::income::city_income(auth, rules, core).culture);
        }
    }
    total
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cost(paid: u32, cities: u32) -> u32 {
        policy_cost(PolicyCostInputs {
            paid_policies: paid,
            cities,
            per_city_percent: 1000,
            per_city_discount: 0,
            cost_percent: 0,
            difficulty_percent: 100,
        })
    }

    #[test]
    fn policy_cost_matches_the_rules_table() {
        // Unciv: PolicyManager.getPolicyCultureCost, Standard, default
        // difficulty (25 + (3n)^2.01, × 1.3 for 4 cities, rounded, down to 5).
        let one_city = [25, 30, 60, 105, 170, 255, 355, 480, 620];
        for (n, expected) in one_city.iter().enumerate() {
            assert_eq!(cost(n as u32, 1), *expected, "1 city, n={n}");
        }
        let four_cities = [30, 40, 80, 140, 220, 330, 465, 620];
        for (n, expected) in four_cities.iter().enumerate() {
            assert_eq!(cost(n as u32, 4), *expected, "4 cities, n={n}");
        }
    }

    #[test]
    fn policy_cost_modifiers_apply_in_order() {
        // Representation: per-city penalty 10 % × 67 % = 6.7 %.
        let with_discount = policy_cost(PolicyCostInputs {
            paid_policies: 1,
            cities: 4,
            per_city_percent: 1000,
            per_city_discount: -33,
            cost_percent: 0,
            difficulty_percent: 100,
        });
        // 34.1 × (1 + 3 × 6.7 %) = 40.95 → 41 → 40.
        assert_eq!(with_discount, 40);
        // The discount is floored at −75 %.
        let floor = policy_cost(PolicyCostInputs {
            paid_policies: 0,
            cities: 1,
            per_city_percent: 1000,
            per_city_discount: 0,
            cost_percent: -90,
            difficulty_percent: 100,
        });
        assert_eq!(floor, 5, "25 × 25 % = 6.25 → 6 → 5");
    }

    /// A difficulty percent of 75 applies before the rounding down to 5
    /// (Unciv: human players only; the AI default level carries 100).
    #[test]
    fn a_difficulty_percent_scales_a_policy() {
        let ai = |paid, cities| {
            policy_cost(PolicyCostInputs {
                paid_policies: paid,
                cities,
                per_city_percent: 1000,
                per_city_discount: 0,
                cost_percent: 0,
                difficulty_percent: 75,
            })
        };
        // n = 0: 25 × 75 % = 18.75 → 19 → 15; n = 3, 4 cities: 107.8 × 75 % × 1.3 = 105.1 → 105.
        assert_eq!(ai(0, 1), 15);
        assert_eq!(ai(3, 4), 105);
    }

    #[test]
    fn border_cost_matches_the_rules_table() {
        // Unciv: CityExpansionManager.getCultureToNextTile, round(6 (t + 1.4813)^1.3).
        let table = [10, 20, 30, 42, 55, 68, 82, 97, 112, 127, 143];
        for (level, expected) in table.iter().enumerate() {
            assert_eq!(border_cost(level as u32, 0, 0), *expected, "level {level}");
        }
        assert_eq!(border_cost(20, 0, 0), 323);
        assert_eq!(border_cost(30, 0, 0), 532);
        // The kernel's table (used by the AI's estimates) is the same curve.
        for level in 0..60 {
            assert_eq!(border_cost(level, 0, 0), crate::world::city::culture_cost(level), "level {level}");
        }
        // An exponent −25 %: level 2: 6 × 3.4813^0.975 = 20.2 → 20.
        assert_eq!(border_cost(2, -25, 0), 20);
        // A −25 % cost (Unciv's Tradition): level 1: 19.6 × 75 % = 14.7 → 15.
        assert_eq!(border_cost(1, 0, -25), 15);
        // Level 0, −50 %: 9.9997 × 50 % → 5.
        assert_eq!(border_cost(0, 0, -50), 5);
        // Floored at −85 %: 9.9997 × 15 % = 1.49996 → 1.
        assert_eq!(border_cost(0, 0, -99), 1);
    }
}

/// The culture and faith requirements of a building (wonders): the
/// policy branch it needs is unlocked, a holy-city building goes into a
/// holy city.
pub fn building_culture_status(
    auth: &AuthWorld,
    rules: &CompiledRuleset,
    city: crate::ids::CityHandle,
    building: crate::ids::BuildingKindId,
) -> Result<(), &'static str> {
    let Some(def) = rules.buildings.get(building.0 as usize) else {
        return Ok(());
    };
    let Some(core) = auth.cities().get(city) else {
        return Ok(());
    };
    if let Some(branch) = def.module.requires_policy_branch {
        if !auth.module().culture().of(core.owner).has_branch(branch) {
            return Err("needs its policy branch");
        }
    }
    if def.module.requires_holy_city && auth.module().religion().holy_city_of(city).is_none() {
        return Err("only a holy city builds it");
    }
    Ok(())
}
