//! The character of a policy branch, read from the rules' data, and its fit
//! to the intent (`weights::branch`). No branch is named: each effect of a
//! branch's policies goes to one character by its kind in the rules — the
//! capital and growth are "tall", settlers, workers and per-city bonuses
//! "wide", combat, promotions and kills "war", garrisons and city combat
//! "defence", and the yields, great person kinds, faith, tourism and
//! city-state numbers their own — and each policy counts once. The fit
//! multiplies a branch's value in the policy head and in the goal pointer;
//! a branch whose largest character is war is shut outside the war posture.

use super::cell::effects_of;
use super::layers::ONE;
use super::weights::branch::{ch, FitInput, FIT, FIT_MAX, FIT_MIN, WAR_GATE};
use super::{Intent, Milli};
use crate::classic::content::GreatPersonKind;
use crate::classic::effects::{ClassicCost, ClassicGrant, ClassicModifier};
use crate::classic::stats::{PlayerRule, SocietyStat};
use crate::ids::{BuildingKindId, UnitKindId};
use crate::rules::content::{PolicyBranchId, PolicyBranchKind};
use crate::rules::effects::{CompiledEffect, Condition, CostTarget, Count, Grant, Modifier, ProductionFilter, Selector, YieldKind};
use crate::rules::stats::{ClassicStat, Stat, TradeStat};
use crate::rules::CompiledRuleset;

/// The character of a great person kind.
fn person(kind: GreatPersonKind) -> Option<usize> {
    match kind {
        GreatPersonKind::Scientist => Some(ch::SCIENCE),
        GreatPersonKind::Merchant => Some(ch::GOLD),
        GreatPersonKind::Prophet => Some(ch::FAITH),
        GreatPersonKind::General | GreatPersonKind::Admiral => Some(ch::WAR),
        GreatPersonKind::Writer | GreatPersonKind::Artist | GreatPersonKind::Musician => Some(ch::CULTURE),
        GreatPersonKind::Engineer => None,
    }
}

/// The character of a unit given or favoured: a founder or a builder is
/// many cities, a great person its kind's, a fighter war.
fn unit(rules: &CompiledRuleset, unit: UnitKindId) -> Option<usize> {
    let def = rules.units.get(usize::from(unit.0))?;
    if def.founder || def.builder {
        return Some(ch::WIDE);
    }
    if let Some(g) = rules.module.great_people.iter().find(|g| g.unit == unit) {
        return person(g.kind);
    }
    (def.strength > 0).then_some(ch::WAR)
}

/// The character of a building a policy speeds: the most frequent of its
/// own effects' (one level: its own production conditions are not
/// followed).
fn building(rules: &CompiledRuleset, building: BuildingKindId) -> Option<usize> {
    let def = rules.buildings.get(usize::from(building.0))?;
    let mut count = [0u32; ch::N];
    for e in effects_of(&def.effects) {
        if let Some(c) = effect_at(rules, e, false) {
            count[c] += 1;
        }
    }
    let best = (0..ch::N).max_by_key(|c| (count[*c], std::cmp::Reverse(*c)))?;
    (count[best] > 0).then_some(best)
}

/// The character of a yield.
fn yield_of(kind: YieldKind) -> Option<usize> {
    match kind {
        YieldKind::Food => Some(ch::TALL),
        YieldKind::Science => Some(ch::SCIENCE),
        YieldKind::Gold => Some(ch::GOLD),
        YieldKind::Culture => Some(ch::CULTURE),
        YieldKind::Module(crate::classic::effects::ClassicYield::Faith) => Some(ch::FAITH),
        YieldKind::Production | YieldKind::Output => None,
    }
}

fn stat(stat: Stat) -> Option<usize> {
    match stat {
        Stat::FoodKept => Some(ch::TALL),
        Stat::WorkerSpeedPercent => Some(ch::WIDE),
        Stat::RouteMaintenancePercent => Some(ch::GOLD),
        Stat::SpecialistYield { yield_kind, .. } | Stat::AnySpecialistYield(yield_kind) => yield_of(yield_kind),
        Stat::Module(s) => match s {
            ClassicStat::GreatPersonRate(kind) => person(kind),
            ClassicStat::HappinessPerCity | ClassicStat::CityCountUnhappinessPercent => Some(ch::WIDE),
            ClassicStat::PopulationUnhappinessPercent => Some(ch::TALL),
            ClassicStat::Trade(TradeStat::InternalPercent) => Some(ch::TALL),
            ClassicStat::Trade(TradeStat::Routes | TradeStat::RangeTiles(_) | TradeStat::RangePercent(_) | TradeStat::RoutesPercent) => None,
            ClassicStat::Trade(_) | ClassicStat::ConnectionGoldPercent => Some(ch::GOLD),
            ClassicStat::Rule(PlayerRule::GarrisonFreeMaintenance) => Some(ch::DEFENCE),
            ClassicStat::Rule(PlayerRule::SeeBarbarianCamps) | ClassicStat::PlunderGoldPercent => Some(ch::WAR),
            ClassicStat::Society(SocietyStat::ScienceWhileHappyPercent) | ClassicStat::ResearchAgreementPercent | ClassicStat::GreatScientistSciencePercent => Some(ch::SCIENCE),
            ClassicStat::Society(SocietyStat::WonderCulturePercent | SocietyStat::HappinessToCulturePercent) => Some(ch::CULTURE),
            _ => None,
        },
        _ => None,
    }
}

fn module(rules: &CompiledRuleset, m: ClassicModifier) -> Option<usize> {
    match m {
        ClassicModifier::AddHappinessPerCount { per, .. } => match per.count {
            Count::Population => Some(ch::TALL),
            Count::ConnectedCities => Some(ch::WIDE),
            _ => None,
        },
        ClassicModifier::AddGreatPersonPoints { kind, .. } => person(kind),
        ClassicModifier::ModifyCityState { .. } => Some(ch::DIPLOMACY),
        ClassicModifier::FreePolicies { .. } | ClassicModifier::ModifyTourism { .. } | ClassicModifier::TourismToward { .. } | ClassicModifier::InstantTourism { .. } => Some(ch::CULTURE),
        ClassicModifier::AddYieldPerFollowingCity { .. }
        | ClassicModifier::AddYieldPerFollowers { .. }
        | ClassicModifier::AddHappinessPerFollowingCity { .. }
        | ClassicModifier::ModifyReligion { .. }
        | ClassicModifier::UnlockFaithBuilding(_)
        | ClassicModifier::UnlockFaithUnits(_)
        | ClassicModifier::AllowReformationBelief
        | ClassicModifier::GrantReligionAbility(_)
        | ClassicModifier::ExtraFoundingBelief
        | ClassicModifier::SecondReligionPantheon => Some(ch::FAITH),
        _ => {
            let _ = rules;
            None
        }
    }
}

/// The character of one effect (None: it serves every play alike —
/// happiness, production, a golden age — or the classifier has no model of
/// it). `follow` lets a production condition read the building it speeds.
fn effect_at(rules: &CompiledRuleset, e: &CompiledEffect, follow: bool) -> Option<usize> {
    if let Modifier::FreeUnit { unit: u, .. } = e.modifier {
        return unit(rules, u);
    }
    if let Condition::Producing(filter) = e.condition {
        return match filter {
            ProductionFilter::UnitClassOf(u) => unit(rules, u),
            ProductionFilter::MilitaryUnits | ProductionFilter::UnitRole(_) => Some(ch::WAR),
            // A wonder race is the capital's: few strong cities.
            ProductionFilter::WorldWonders { .. } => Some(ch::TALL),
            ProductionFilter::BuildingClassOf(b) if follow => building(rules, b),
            _ => None,
        };
    }
    if matches!(e.condition, Condition::CityGarrisoned) {
        return Some(ch::DEFENCE);
    }
    let capital = matches!(e.selector, Selector::Capital);
    match e.modifier {
        Modifier::ModifyCombat { .. }
        | Modifier::ModifyUnitStat { .. }
        | Modifier::ModifyMovement { .. }
        | Modifier::PassivePromotion(_)
        | Modifier::PostCombatPromotion(_)
        | Modifier::AddPromotion(_)
        | Modifier::GrantExperience(_)
        | Modifier::YieldFromKill { .. } => Some(ch::WAR),
        Modifier::ModifyCityCombat { .. } => Some(ch::DEFENCE),
        Modifier::ModifyGrowth { .. } | Modifier::FreeBuildingInCities { .. } => Some(ch::TALL),
        Modifier::ModifyCost { target, .. } => match target {
            CostTarget::UnitUpgrade => Some(ch::WAR),
            CostTarget::Technologies => Some(ch::SCIENCE),
            CostTarget::BorderGrowth | CostTarget::BorderGrowthExponent => Some(ch::TALL),
            CostTarget::Module(ClassicCost::PolicyCityPenalty) => Some(ch::WIDE),
            CostTarget::Module(ClassicCost::Policies) => Some(ch::CULTURE),
            CostTarget::Module(ClassicCost::FaithPurchase) => Some(ch::FAITH),
            CostTarget::Hurry | CostTarget::UnitPurchase | CostTarget::BuildingPurchase | CostTarget::TilePurchase => Some(ch::GOLD),
            CostTarget::Units | CostTarget::Buildings => None,
        },
        // A yield in the capital is the tall play's whatever the yield; a
        // flat culture or production in every city always is the wide one's
        // (it grows with the cities).
        Modifier::AddYield { .. } | Modifier::AddYieldPercent { .. } | Modifier::AddYieldPerPopulation { .. } | Modifier::AddYieldPerCount { .. } | Modifier::AddYieldPercentPerCount { .. }
            if capital =>
        {
            Some(ch::TALL)
        }
        Modifier::AddYield { yield_kind: YieldKind::Culture | YieldKind::Production, .. } if matches!(e.selector, Selector::OwnerCities) && matches!(e.condition, Condition::Always) => Some(ch::WIDE),
        Modifier::AddYield { yield_kind, .. }
        | Modifier::AddYieldPercent { yield_kind, .. }
        | Modifier::AddYieldPerPopulation { yield_kind, .. }
        | Modifier::AddYieldPerCount { yield_kind, .. }
        | Modifier::AddYieldPercentPerCount { yield_kind, .. } => yield_of(yield_kind),
        Modifier::AddStat { stat: s, .. } if capital => stat(s).map(|_| ch::TALL),
        Modifier::AddStat { stat: s, .. } => stat(s),
        Modifier::Grant(Grant::FreeTechnologies(_)) => Some(ch::SCIENCE),
        Modifier::Grant(Grant::Module(ClassicGrant::FreeGreatPeople(_) | ClassicGrant::GoldenAge { .. })) => None,
        Modifier::Module(m) => module(rules, m),
        _ => None,
    }
}

/// The character of one effect.
pub fn effect(rules: &CompiledRuleset, e: &CompiledEffect) -> Option<usize> {
    effect_at(rules, e, true)
}

/// The character of a branch: the shares (thousandths) of its policies'
/// characters, each policy once (split over its effects' characters).
pub fn character(rules: &CompiledRuleset, branch: PolicyBranchId) -> [Milli; ch::N] {
    let mut sum = [0i64; ch::N];
    for def in rules.module.policies.iter().filter(|p| p.branch == Some(branch)) {
        let found: Vec<usize> = effects_of(&def.effects).filter_map(|e| effect(rules, e)).collect();
        for c in &found {
            sum[*c] += ONE / found.len() as i64;
        }
    }
    let total: i64 = sum.iter().sum();
    if total > 0 {
        for v in sum.iter_mut() {
            *v = *v * ONE / total;
        }
    }
    sum
}

/// The largest character of a branch (None: nothing classified).
pub fn dominant(character: &[Milli; ch::N]) -> Option<usize> {
    let best = (0..ch::N).max_by_key(|c| (character[*c], std::cmp::Reverse(*c)))?;
    (character[best] > 0).then_some(best)
}

/// A normal branch (not an ideology).
pub fn is_normal(rules: &CompiledRuleset, branch: PolicyBranchId) -> bool {
    rules.module.policy_branches.get(usize::from(branch.0)).is_some_and(|d| d.kind != PolicyBranchKind::Ideology)
}

fn input(intent: &Intent, i: FitInput) -> Milli {
    match i {
        FitInput::Bias => ONE,
        FitInput::Style(s) => {
            if intent.style_pick == s {
                ONE
            } else {
                0
            }
        }
        FitInput::Aim(a) => intent.aim[a],
        FitInput::Posture(p) => intent.posture[p],
        FitInput::Concept(k) => intent.concepts[k],
    }
}

/// The fit of a branch of `character` to the intent: a multiplier of its
/// value (thousandths), None when the branch is shut (the war gate).
pub fn fit(intent: &Intent, character: &[Milli; ch::N]) -> Option<Milli> {
    if WAR_GATE && dominant(character) == Some(ch::WAR) && intent.posture_pick != 3 {
        return None;
    }
    let mut f = ONE;
    for (i, c, w, _) in FIT {
        f += character[*c] * input(intent, *i) / ONE * w / ONE;
    }
    Some(f.clamp(FIT_MIN, FIT_MAX))
}

/// The fit of a branch by its id.
pub fn branch_fit(rules: &CompiledRuleset, intent: &Intent, branch: PolicyBranchId) -> Option<Milli> {
    fit(intent, &character(rules, branch))
}
