//! Effect text of the classic words (`classic_rules::classic::effects`): the
//! kernel's describer (`describe`) asks it for every `Module(..)` word, one
//! function per family. The sentences are messages of `classic-pedia-describe.ftl`
//! (keys `classic.pedia.describe-classic.*`).

use classic_rules::classic::effects::{
    CityRelation, CityStateStat, ClassicCondition, ClassicCost, ClassicCount, ClassicGrant, ClassicModifier,
    ClassicSelector, ClassicTrigger, ClassicUnitAbility, ClassicUnitStat, ClassicYield, GreatPersonKind,
    ReligionAbility, ReligionStat, TourismCondition, TourismStat,
};
use classic_rules::classic::tables::LeagueReward;
use classic_rules::rules::compiled::CompiledRuleset;
use i18n::{t, tf};

use super::{hundredths_text, name_of, per_count, signed, yield_name};
use crate::ui::pedia::{PediaRef, Table};

pub(super) fn trigger(trigger: ClassicTrigger) -> &'static str {
    match trigger {
        ClassicTrigger::OnAdopt => t!("classic.pedia.describe-classic.trigger-on-adopt"),
        ClassicTrigger::OnNaturalWonderFoundFirst => t!("classic.pedia.describe-classic.trigger-wonder-found-first"),
        ClassicTrigger::OnNaturalWonderFound => t!("classic.pedia.describe-classic.trigger-wonder-found"),
        ClassicTrigger::OnLongCountCycle => t!("classic.pedia.describe-classic.trigger-long-count"),
        ClassicTrigger::OnGreatPersonExpended => t!("classic.pedia.describe-classic.trigger-great-person-expended"),
        ClassicTrigger::OnCityConverted => t!("classic.pedia.describe-classic.trigger-city-converted"),
        ClassicTrigger::OnReligionSpread => t!("classic.pedia.describe-classic.trigger-religion-spread"),
        ClassicTrigger::OnGreatPersonBorn => t!("classic.pedia.describe-classic.trigger-great-person-born"),
    }
}

pub(super) fn selector(selector: ClassicSelector) -> &'static str {
    match selector {
        ClassicSelector::FollowingCities => t!("classic.pedia.describe-classic.selector-following-cities"),
        ClassicSelector::FollowingCityTiles => t!("classic.pedia.describe-classic.selector-following-city-tiles"),
        ClassicSelector::HolyCity => t!("classic.pedia.describe-classic.selector-holy-city"),
        ClassicSelector::Founder => t!("classic.pedia.describe-classic.selector-founder"),
        ClassicSelector::OwnReligion => t!("classic.pedia.describe-classic.selector-own-religion"),
    }
}

pub(super) fn condition(condition: ClassicCondition) -> String {
    match condition {
        ClassicCondition::OpponentIsBarbarian => t!("classic.pedia.describe-classic.condition-barbarian").into(),
        ClassicCondition::OpponentIsCityState => t!("classic.pedia.describe-classic.condition-city-state").into(),
        ClassicCondition::OpponentHasMoreCities => t!("classic.pedia.describe-classic.condition-more-cities").into(),
        ClassicCondition::MinFollowers(n) => tf!("classic.pedia.describe-classic.condition-min-followers", n = n),
        ClassicCondition::NearFollowingCity(CityRelation::Own) => t!("classic.pedia.describe-classic.condition-near-own-following-city").into(),
        ClassicCondition::NearFollowingCity(CityRelation::Enemy) => t!("classic.pedia.describe-classic.condition-near-enemy-following-city").into(),
        ClassicCondition::GoldenAge => t!("classic.pedia.describe-classic.condition-golden-age").into(),
        ClassicCondition::GreatGeneralEarned => t!("classic.pedia.describe-classic.condition-great-general-earned").into(),
        ClassicCondition::GreatPersonIs(kind) => tf!("classic.pedia.describe-classic.condition-great-person-is", kind = great_person(kind)),
    }
}

pub(super) fn modifier(rules: &CompiledRuleset, modifier: ClassicModifier) -> String {
    match modifier {
        ClassicModifier::AddHappiness { amount } => tf!("classic.pedia.describe-classic.modifier-happiness", amount = signed(amount)),
        ClassicModifier::AddGreatPersonPoints { kind, amount } => {
            tf!("classic.pedia.describe-classic.modifier-great-person-points", amount = signed(amount), kind = great_person(kind))
        }
        ClassicModifier::FreePolicies { count } => tf!("classic.pedia.describe-classic.modifier-free-policies", count = count),
        ClassicModifier::AddYieldPerFollowingCity { yield_kind, amount, foreign_only } => tf!(
            "classic.pedia.describe-classic.modifier-yield-per-following-city",
            amount = signed(amount),
            yield = yield_name(yield_kind),
            foreign = foreign_only
        ),
        ClassicModifier::AddYieldPerFollowers { yield_kind, per, foreign_only } => tf!(
            "classic.pedia.describe-classic.modifier-yield-per-followers",
            yield = yield_name(yield_kind),
            per = per,
            foreign = foreign_only
        ),
        ClassicModifier::AddHappinessPerFollowingCity { hundredths } => {
            tf!("classic.pedia.describe-classic.modifier-happiness-per-following-city", amount = hundredths_text(hundredths))
        }
        ClassicModifier::ModifyReligion { stat, percent } => {
            tf!("classic.pedia.describe-classic.modifier-religion", percent = signed(percent), stat = religion_stat(stat))
        }
        ClassicModifier::UnlockFaithBuilding(b) => {
            tf!("classic.pedia.describe-classic.modifier-faith-building", name = name_of(rules, PediaRef::new(Table::Building, b.0)))
        }
        ClassicModifier::UnlockFaithUnits(era) => {
            tf!("classic.pedia.describe-classic.modifier-faith-units", era = name_of(rules, PediaRef::new(Table::Era, era.0)))
        }
        ClassicModifier::AllowReformationBelief => t!("classic.pedia.describe-classic.modifier-reformation-belief").into(),
        ClassicModifier::ModifyTourism { stat, amount } => tourism(stat, amount),
        ClassicModifier::TourismToward { when, percent } => {
            tf!("classic.pedia.describe-classic.modifier-tourism-toward", percent = signed(percent), when = tourism_condition(when))
        }
        ClassicModifier::ModifyCityState { stat, amount } => city_state_phrase(stat, amount),
        ClassicModifier::AddHappinessPerCount { amount, per } => {
            tf!("classic.pedia.describe-classic.modifier-happiness-per-count", amount = signed(amount), per = per_count(per))
        }
        ClassicModifier::GrantSpies { count } => tf!("classic.pedia.describe-classic.modifier-grant-spies", count = count),
        ClassicModifier::PromoteSpies { ranks } => tf!("classic.pedia.describe-classic.modifier-promote-spies", ranks = ranks),
        ClassicModifier::GrantReligionAbility(ability) => religion_ability(ability).into(),
        ClassicModifier::ExtraFoundingBelief => t!("classic.pedia.describe-classic.modifier-extra-founding-belief").into(),
        ClassicModifier::SecondReligionPantheon => t!("classic.pedia.describe-classic.modifier-second-religion-pantheon").into(),
        ClassicModifier::InstantTourism { amount } => tf!("classic.pedia.describe-classic.modifier-instant-tourism", amount = amount),
        ClassicModifier::GrantExperiencePerGreatWork(amount) => {
            tf!("classic.pedia.describe-classic.modifier-experience-per-great-work", amount = amount)
        }
    }
}

pub(super) fn grant(grant: ClassicGrant) -> String {
    match grant {
        ClassicGrant::FreeGreatPeople(n) => tf!("classic.pedia.describe-classic.grant-free-great-people", n = n),
        ClassicGrant::LongCountGreatPerson => t!("classic.pedia.describe-classic.grant-long-count-great-person").into(),
        ClassicGrant::GoldenAge { turns } => tf!("classic.pedia.describe-classic.grant-golden-age", turns = turns),
    }
}

pub(super) fn unit_stat(stat: ClassicUnitStat, amount: i32) -> String {
    let a = signed(amount);
    match stat {
        ClassicUnitStat::GreatGeneralPointsPercent => tf!("classic.pedia.describe-classic.unit-stat-general-points", amount = a),
        ClassicUnitStat::StackedGeneralPercent => tf!("classic.pedia.describe-classic.unit-stat-stacked-general", amount = a),
        ClassicUnitStat::MaintenancePercent => tf!("classic.pedia.describe-classic.unit-stat-maintenance", amount = a),
        ClassicUnitStat::ExperienceNeededPercent => tf!("classic.pedia.describe-classic.unit-stat-experience-needed", amount = a),
        ClassicUnitStat::GeneralAuraPercent => tf!("classic.pedia.describe-classic.unit-stat-general-aura", amount = a),
        ClassicUnitStat::TradeMissionGoldPercent => tf!("classic.pedia.describe-classic.unit-stat-mission-gold", amount = a),
        ClassicUnitStat::TradeMissionInfluencePercent => tf!("classic.pedia.describe-classic.unit-stat-mission-influence", amount = a),
        ClassicUnitStat::AdjacentDamageChance => tf!("classic.pedia.describe-classic.unit-stat-adjacent-damage-chance", amount = amount),
        ClassicUnitStat::RivalLandStrengthLossPercent => t!("classic.pedia.describe-classic.unit-stat-rival-land-loss").into(),
        ClassicUnitStat::GoldenAgePointsOnKillPercent => t!("classic.pedia.describe-classic.unit-stat-golden-age-on-kill").into(),
    }
}

pub(super) fn ability(ability: ClassicUnitAbility) -> &'static str {
    match ability {
        ClassicUnitAbility::GreatGeneral => t!("classic.pedia.describe-classic.ability-great-general"),
        ClassicUnitAbility::PostCombatPromotions => t!("classic.pedia.describe-classic.ability-post-combat-promotions"),
        ClassicUnitAbility::LendsMovementToGeneral => t!("classic.pedia.describe-classic.ability-lends-movement"),
        ClassicUnitAbility::ChoosesRuinsReward => t!("classic.pedia.describe-classic.ability-ruins-reward"),
        ClassicUnitAbility::ExoticCargo => t!("classic.pedia.describe-classic.ability-exotic-cargo"),
        ClassicUnitAbility::GreatAdmiral => t!("classic.pedia.describe-classic.ability-great-admiral"),
        ClassicUnitAbility::HealOnKillNotBarbarians => t!("classic.pedia.describe-classic.ability-heal-not-barbarians"),
    }
}

pub(super) fn yield_kind_name(kind: ClassicYield) -> &'static str {
    match kind {
        ClassicYield::Faith => t!("classic.pedia.describe.yield-faith"),
    }
}

pub(super) fn cost_name(target: ClassicCost) -> &'static str {
    match target {
        ClassicCost::Policies => t!("classic.pedia.describe-classic.cost-policies"),
        ClassicCost::PolicyCityPenalty => t!("classic.pedia.describe-classic.cost-policy-city-penalty"),
        ClassicCost::FaithPurchase => t!("classic.pedia.describe-classic.cost-faith-purchase"),
    }
}

/// The name of a classic count, in the form that goes with the number `n`.
pub(super) fn count_name(count: ClassicCount, n: impl Into<i64> + Copy) -> String {
    match count {
        ClassicCount::Policies => tf!("classic.pedia.describe-classic.count-policies", n = n.into()),
        ClassicCount::MajorityFollowers => tf!("classic.pedia.describe-classic.count-majority-followers", n = n.into()),
        ClassicCount::ReligionsPresent => tf!("classic.pedia.describe-classic.count-religions-present", n = n.into()),
        ClassicCount::PeacefulForeignFollowers => tf!("classic.pedia.describe-classic.count-peaceful-foreign-followers", n = n.into()),
        ClassicCount::OtherReligionFollowers => tf!("classic.pedia.describe-classic.count-other-religion-followers", n = n.into()),
        ClassicCount::TradePartners => tf!("classic.pedia.describe-classic.count-trade-partners", n = n.into()),
        ClassicCount::CityStateRoutes => tf!("classic.pedia.describe-classic.count-city-state-routes", n = n.into()),
    }
}

#[allow(unreachable_patterns)]
pub(super) fn modifier_ref(modifier: ClassicModifier) -> Option<PediaRef> {
    match modifier {
        ClassicModifier::UnlockFaithBuilding(b) => Some(PediaRef::new(Table::Building, b.0)),
        ClassicModifier::UnlockFaithUnits(era) => Some(PediaRef::new(Table::Era, era.0)),
        _ => None,
    }
}

/// One reward of a World Congress project.
pub fn league_reward(rules: &CompiledRuleset, reward: &LeagueReward) -> String {
    match *reward {
        LeagueReward::Happiness(n) => tf!("classic.pedia.describe-classic.league-happiness", n = n),
        LeagueReward::FreePolicies(n) => tf!("classic.pedia.describe-classic.league-free-policies", n = n),
        LeagueReward::GoldenAgePoints(n) => tf!("classic.pedia.describe-classic.league-golden-age-points", n = n),
        LeagueReward::CultureBoostTurns(n) => tf!("classic.pedia.describe-classic.league-culture-boost", n = n),
        LeagueReward::TourismBoostTurns(n) => tf!("classic.pedia.describe-classic.league-tourism-boost", n = n),
        LeagueReward::CityStateInfluence(n) => tf!("classic.pedia.describe-classic.league-city-state-influence", n = n),
        LeagueReward::ResearchTurns(n) => tf!("classic.pedia.describe-classic.league-research-turns", n = n),
        LeagueReward::FreeUnit(u) => {
            tf!("classic.pedia.describe-classic.league-free-unit", name = name_of(rules, PediaRef::new(Table::Unit, u.0)))
        }
        LeagueReward::Building(b) => {
            tf!("classic.pedia.describe-classic.league-building", name = name_of(rules, PediaRef::new(Table::Building, b.0)))
        }
    }
}

pub(super) fn city_state_phrase(stat: CityStateStat, amount: i32) -> String {
    use CityStateStat as S;
    let a = signed(amount);
    match stat {
        S::RestingPoint => tf!("classic.pedia.describe-classic.city-state-resting-point", amount = a),
        S::DecayPercent => tf!("classic.pedia.describe-classic.city-state-decay", amount = a),
        S::GoldGiftPercent => tf!("classic.pedia.describe-classic.city-state-gold-gift", amount = a),
        S::BullyScorePercent => tf!("classic.pedia.describe-classic.city-state-bully-score", amount = a),
        S::UnitGiftInfluence => tf!("classic.pedia.describe-classic.city-state-unit-gift-influence", amount = a),
        S::FriendBonusPercent => tf!("classic.pedia.describe-classic.city-state-friend-bonus", amount = a),
        S::FriendshipRate => tf!("classic.pedia.describe-classic.city-state-friendship-rate", amount = a, decay = signed(-(amount / 2))),
        S::AnnexCityStates => t!("classic.pedia.describe-classic.city-state-annex").into(),
        S::RestingPointSameReligion => tf!("classic.pedia.describe-classic.city-state-resting-point-religion", amount = a),
        S::AfraidInfluence => tf!("classic.pedia.describe-classic.city-state-afraid-influence", amount = a),
        S::TradeRouteInfluence => tf!("classic.pedia.describe-classic.city-state-trade-route-influence", amount = a),
        S::UnitGiftRatePercent => tf!("classic.pedia.describe-classic.city-state-unit-gift-rate", amount = amount),
        S::AllyResourceBonus => t!("classic.pedia.describe-classic.city-state-ally-resources").into(),
        S::AllyGreatPeople => t!("classic.pedia.describe-classic.city-state-ally-great-people").into(),
        S::AllyScience => t!("classic.pedia.describe-classic.city-state-ally-science").into(),
        S::GreatPersonGiftInfluence => tf!("classic.pedia.describe-classic.city-state-great-person-gift", amount = a),
        S::TradeMissionGoldPercent => tf!("classic.pedia.describe-classic.city-state-mission-gold", amount = a),
    }
}

pub fn great_person(kind: GreatPersonKind) -> &'static str {
    match kind {
        GreatPersonKind::Artist => t!("classic.pedia.describe-classic.great-artist"),
        GreatPersonKind::Engineer => t!("classic.pedia.describe-classic.great-engineer"),
        GreatPersonKind::Merchant => t!("classic.pedia.describe-classic.great-merchant"),
        GreatPersonKind::Scientist => t!("classic.pedia.describe-classic.great-scientist"),
        GreatPersonKind::Writer => t!("classic.pedia.describe-classic.great-writer"),
        GreatPersonKind::Musician => t!("classic.pedia.describe-classic.great-musician"),
        GreatPersonKind::General => t!("classic.pedia.describe-classic.great-general"),
        GreatPersonKind::Admiral => t!("classic.pedia.describe-classic.great-admiral"),
        GreatPersonKind::Prophet => t!("classic.pedia.describe-classic.great-prophet"),
    }
}

pub(super) fn religion_stat(stat: ReligionStat) -> &'static str {
    match stat {
        ReligionStat::PressureOut => t!("classic.pedia.describe-classic.religion-pressure-out"),
        ReligionStat::SpreadDistance => t!("classic.pedia.describe-classic.religion-spread-distance"),
        ReligionStat::MissionaryStrength => t!("classic.pedia.describe-classic.religion-missionary-strength"),
        ReligionStat::ProphetStrength => t!("classic.pedia.describe-classic.religion-prophet-strength"),
        ReligionStat::ProphetCost => t!("classic.pedia.describe-classic.religion-prophet-cost"),
        ReligionStat::MissionaryCost => t!("classic.pedia.describe-classic.religion-missionary-cost"),
        ReligionStat::FriendlyCityStatePressure => t!("classic.pedia.describe-classic.religion-friendly-city-state-pressure"),
        ReligionStat::OtherReligionErosion => t!("classic.pedia.describe-classic.religion-other-erosion"),
        ReligionStat::PressureRetention => t!("classic.pedia.describe-classic.religion-pressure-retention"),
    }
}

pub(super) fn religion_ability(ability: ReligionAbility) -> &'static str {
    use ReligionAbility as A;
    match ability {
        A::ConvertsBarbarians => t!("classic.pedia.describe-classic.religion-converts-barbarians"),
        A::FaithPurchaseAllGreatPeople => t!("classic.pedia.describe-classic.religion-faith-great-people"),
    }
}

pub(super) fn tourism(stat: TourismStat, amount: i32) -> String {
    let a = signed(amount);
    match stat {
        TourismStat::Flat => tf!("classic.pedia.describe-classic.tourism-flat", amount = a),
        TourismStat::Percent => tf!("classic.pedia.describe-classic.tourism-percent", amount = a),
        TourismStat::GreatWorksPercent => tf!("classic.pedia.describe-classic.tourism-great-works", amount = a),
        TourismStat::LandmarksPercent => tf!("classic.pedia.describe-classic.tourism-landmarks", amount = a),
        TourismStat::PerFaithBuilding => tf!("classic.pedia.describe-classic.tourism-per-faith-building", amount = a),
        TourismStat::Spread => tf!("classic.pedia.describe-classic.tourism-spread", amount = a),
        TourismStat::ThemingPercent => tf!("classic.pedia.describe-classic.tourism-theming", amount = a),
        TourismStat::GreatWorkCulture => tf!("classic.pedia.describe-classic.tourism-great-work-culture", amount = a),
    }
}

pub(super) fn tourism_condition(when: TourismCondition) -> &'static str {
    match when {
        TourismCondition::SharedReligion => t!("classic.pedia.describe-classic.tourism-shared-religion"),
        TourismCondition::TradeRoute => t!("classic.pedia.describe-classic.tourism-trade-route"),
        TourismCondition::OpenBorders => t!("classic.pedia.describe-classic.tourism-open-borders"),
        TourismCondition::SameIdeology => t!("classic.pedia.describe-classic.tourism-same-ideology"),
        TourismCondition::LessHappy => t!("classic.pedia.describe-classic.tourism-less-happy"),
        TourismCondition::CommonEnemy => t!("classic.pedia.describe-classic.tourism-common-enemy"),
    }
}
