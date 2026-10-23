//! How much the AI wants a piece of content (buildings, units,
//! improvements, technologies) under its strategy: a weighted sum of the
//! content's typed effects and yields (weights by strategy).
//! This is the AI's planning model of value, not a copy of any game
//! formula: the costs and yields themselves come from the rules data.

use crate::classic::effects::{ClassicGrant, ClassicModifier, ClassicTrigger, ClassicYield};
use crate::ai::state::Strategy;
use crate::ids::{BuildingKindId, ImprovementId, ProductionItem, ResourceId, TechId, UnitKindId};
use crate::rules::effects::{CompiledEffect, EffectSet, Modifier, YieldKind};
use crate::rules::CompiledRuleset;

/// Strategy weights per kind of benefit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Weights {
    pub food: i32,
    pub production: i32,
    pub gold: i32,
    pub science: i32,
    pub culture: i32,
    pub faith: i32,
    pub growth: i32,
    pub military: i32,
    pub happiness: i32,
}

/// Added to the gold weight at a deficit.
pub const DEFICIT_GOLD_WEIGHT: i32 = 4;
/// Added to the happiness weight while the empire is unhappy.
pub const UNHAPPY_HAPPINESS_WEIGHT: i32 = 6;

impl Weights {
    pub fn of(strategy: Strategy) -> Self {
        match strategy {
            Strategy::Expand => Self::new([4, 3, 1, 1, 2, 1, 3, 1, 2]),
            Strategy::Stabilize => Self::new([3, 3, 2, 3, 2, 1, 2, 1, 2]),
            Strategy::Defend => Self::new([1, 3, 1, 1, 1, 1, 1, 4, 1]),
            Strategy::Attack => Self::new([1, 3, 1, 1, 1, 1, 1, 4, 1]),
            Strategy::Recover => Self::new([2, 3, 1, 1, 1, 1, 2, 3, 2]),
        }
    }

    /// The strategy's weights for an empire's state: gold weighs more at a
    /// deficit (buildings that earn gold first, upkeep counts more) and
    /// happiness weighs more while the empire is unhappy (the rules: growth
    /// −75 %, no new city when very unhappy). Without this the AI kept
    /// adding upkeep at a deficit and stayed unhappy in a third of its
    /// turns (full-game harness), growing slowly.
    pub fn for_empire(strategy: Strategy, gold_income: i32, excess_happiness: i32) -> Self {
        let mut weights = Self::of(strategy);
        if gold_income < 0 {
            weights.gold += DEFICIT_GOLD_WEIGHT;
        }
        if excess_happiness < 0 {
            weights.happiness += UNHAPPY_HAPPINESS_WEIGHT;
        }
        weights
    }

    fn new(w: [i32; 9]) -> Self {
        Self {
            food: w[0],
            production: w[1],
            gold: w[2],
            science: w[3],
            culture: w[4],
            faith: w[5],
            growth: w[6],
            military: w[7],
            happiness: w[8],
        }
    }

    pub fn yield_weight(&self, kind: YieldKind) -> i32 {
        match kind {
            YieldKind::Output => self.production + self.gold,
            YieldKind::Food => self.food,
            YieldKind::Production => self.production,
            YieldKind::Gold => self.gold,
            YieldKind::Science => self.science,
            YieldKind::Culture => self.culture,
            YieldKind::Module(ClassicYield::Faith) => self.faith,
        }
    }
}

/// Value of one effect (points; 10 ≈ one yield per turn in one city).
pub(crate) fn effect_value(effect: &CompiledEffect, w: &Weights, cities: i32) -> i32 {
    if let Some(value) = occasional_grant_value(effect, w) {
        return value;
    }
    let reach = if effect.selector.is_owner_wide() {
        cities.max(1)
    } else {
        1
    };
    // A percent toward some items only counts while the city builds them.
    if let crate::rules::effects::Condition::Producing(filter) = effect.condition {
        return production_percent_value(effect, filter, w) * reach;
    }
    let value = match effect.modifier {
        Modifier::AddYield { yield_kind, amount } => w.yield_weight(yield_kind) * amount * 10,
        Modifier::AddYieldPercent {
            yield_kind,
            percent,
        } => w.yield_weight(yield_kind) * percent / 2,
        Modifier::MultiplyYield {
            yield_kind,
            percent,
        } => w.yield_weight(yield_kind) * (percent - 100) / 2,
        Modifier::AddYieldPerPopulation {
            yield_kind,
            hundredths,
        } => w.yield_weight(yield_kind) * hundredths / 5,
        Modifier::ModifyGrowth { percent } => w.growth * percent / 2,
        Modifier::ModifyCost { percent, .. } => w.production * (-percent) / 2,
        Modifier::ModifyCombat { percent, .. } => w.military * percent / 2,
        Modifier::ModifyMovement { amount } => w.military * i32::from(amount) * 5,
        Modifier::Module(ClassicModifier::AddHappiness { amount }) => w.happiness * amount * 10,
        Modifier::Module(ClassicModifier::AddGreatPersonPoints { amount, .. }) => w.culture * amount * 3,
        Modifier::FreeUnit { count, .. } => w.military * i32::from(count) * 20,
        Modifier::FreeBuilding(_) => w.production * 30,
        Modifier::AddPromotion(_) => w.military * 10,
        Modifier::UnlockUnit(_) | Modifier::UnlockBuilding(_) => 5,
        Modifier::AddStat { stat, amount } => stat_value(stat, amount, w),
        Modifier::ModifyUnitStat { amount, .. } => w.military * amount.clamp(-10, 10),
        Modifier::GrantAbility(_)
        | Modifier::PostCombatPromotion(_)
        | Modifier::PassivePromotion(_) => w.military * 10,
        Modifier::ModifyCityCombat { amount, .. } => w.military * amount.clamp(-100, 100) / 10,
        Modifier::GrantExperience(xp) => w.military * i32::from(xp) / 2,
        Modifier::Module(ClassicModifier::ModifyCityState { stat, amount }) => city_state_value(stat, amount, w),
        // Track B (culture, faith): valued by the system advisors
        // (ai::policies, ai::religion), a flat token here.
        Modifier::Module(ClassicModifier::FreePolicies { count }) => w.culture * i32::from(count) * 30,
        Modifier::FreeBuildingInCities { count, .. } => w.production * i32::from(count) * 20,
        Modifier::Module(ClassicModifier::AddYieldPerFollowingCity {
            yield_kind, amount, ..
        }) => w.yield_weight(yield_kind) * amount * 10,
        Modifier::Module(ClassicModifier::AddYieldPerFollowers { yield_kind, .. }) => w.yield_weight(yield_kind) * 5,
        Modifier::Module(ClassicModifier::AddHappinessPerFollowingCity { hundredths }) => w.happiness * hundredths / 10,
        Modifier::Module(ClassicModifier::ModifyReligion { .. })
        | Modifier::Module(ClassicModifier::UnlockFaithBuilding(_))
        | Modifier::Module(ClassicModifier::UnlockFaithUnits(_))
        | Modifier::Module(ClassicModifier::AllowReformationBelief) => 5,
        Modifier::Module(ClassicModifier::ModifyTourism { amount, .. }) => w.culture * amount / 2,
        Modifier::Module(ClassicModifier::TourismToward { percent, .. }) => w.culture * percent / 4,
        // Per-count modifiers: valued at a typical count of a few steps.
        Modifier::AddYieldPerCount {
            yield_kind,
            amount,
            per,
        } => w.yield_weight(yield_kind) * per.scale(amount, 3 * u32::from(per.every)) * 10,
        Modifier::AddYieldPercentPerCount {
            yield_kind,
            percent,
            per,
        } => w.yield_weight(yield_kind) * per.scale(percent, 3 * u32::from(per.every)) / 2,
        Modifier::Module(ClassicModifier::AddHappinessPerCount { amount, per }) => {
            w.happiness * per.scale(amount, 3 * u32::from(per.every)) * 10
        }
        // A kill of a typical unit (strength ~15) now and then: worth more
        // to an army that fights.
        Modifier::YieldFromKill { yield_kind, percent } => {
            w.yield_weight(yield_kind) * percent * w.military / 20
        }
        Modifier::Grant(grant) => grant_value(grant, w),
        // A ban costs the options it closes (Venice: no settlers, no annexing).
        Modifier::Forbid(crate::rules::effects::Forbidden::UnitClassOf(_)) => -w.growth * 20,
        Modifier::Forbid(crate::rules::effects::Forbidden::Annexing) => -w.military * 5,
        // A spy steals technologies (science) for the rest of the game; a
        // rank is a quarter faster theft and a better counter-spy.
        Modifier::Module(ClassicModifier::GrantSpies { count }) => w.science * i32::from(count) * SPY_VALUE,
        Modifier::Module(ClassicModifier::PromoteSpies { ranks }) => w.science * i32::from(ranks) * SPY_RANK_VALUE,
        // Reformation rules: a missionary converting barbarians, great
        // people for faith (worth a great person now and then).
        Modifier::Module(ClassicModifier::GrantReligionAbility(_)) => w.culture * 10,
        // One more belief at founding: roughly a follower belief.
        Modifier::Module(ClassicModifier::ExtraFoundingBelief) => w.culture * 20,
        // A second pantheon in the cities with two religions (owner-wide).
        Modifier::Module(ClassicModifier::SecondReligionPantheon) => w.happiness * 3,
        // Influence on every civilization met: a step toward a culture win.
        Modifier::Module(ClassicModifier::GrantExperiencePerGreatWork(xp)) => w.military * i32::from(xp) / 4,
        Modifier::Module(ClassicModifier::InstantTourism { amount }) => w.culture * (amount.min(10_000) as i32) / 25,
    };
    value * reach
}

/// Value of a production percent toward some items: the share of the time
/// a city builds them (a guess per filter) of a production percent.
fn production_percent_value(
    effect: &CompiledEffect,
    filter: crate::rules::effects::ProductionFilter,
    w: &Weights,
) -> i32 {
    use crate::rules::effects::ProductionFilter as F;
    let Modifier::AddYieldPercent { percent, .. } = effect.modifier else {
        return 0;
    };
    // Share of the production that goes to such items, in percent.
    let share = match filter {
        F::Units | F::Buildings | F::BuiltInCapital => 50,
        F::MilitaryUnits | F::UnitDomain(_) => 30,
        F::WorldWonders { .. } => 20,
        F::UnitRole(_) | F::UnitClassOf(_) | F::SpaceshipParts | F::BuildingClassOf(_) => 10,
    };
    // The military weight values military production, the culture weight
    // wonders, production the rest.
    let weight = match filter {
        F::MilitaryUnits | F::UnitDomain(_) | F::UnitRole(_) => w.military.max(w.production),
        F::WorldWonders { .. } => w.culture.max(w.production),
        _ => w.production,
    };
    weight * percent * share / 200
}

/// Value of an instant grant (points as `effect_value`).
fn grant_value(grant: crate::rules::effects::Grant, w: &Weights) -> i32 {
    use crate::rules::effects::Grant;
    match grant {
        // A technology is a few turns of research.
        Grant::FreeTechnologies(n) => w.science * i32::from(n) * 40,
        Grant::Module(ClassicGrant::FreeGreatPeople(n)) => (w.science + w.production) * i32::from(n) * 20,
        Grant::Module(ClassicGrant::LongCountGreatPerson) => (w.science + w.production) * 20,
        Grant::ConquestTechnology => w.military * 10,
        Grant::Module(ClassicGrant::GoldenAge { turns }) => (w.production + w.gold + w.culture) * i32::from(turns) * 2,
        Grant::MilitaryUnitCopies => w.military * 60,
        Grant::Population(n) => w.growth * i32::from(n) * 20,
    }
}

/// A lump sum paid now and then (a great person expended, a city
/// converted, a religion spread): worth about a tenth of the sum per turn,
/// once for the player (None: not such a grant).
fn occasional_grant_value(effect: &CompiledEffect, w: &Weights) -> Option<i32> {
    use crate::rules::effects::Trigger;
    let occasional = matches!(
        effect.trigger,
        Trigger::Module(ClassicTrigger::OnGreatPersonExpended) | Trigger::Module(ClassicTrigger::OnCityConverted) | Trigger::Module(ClassicTrigger::OnReligionSpread)
    );
    if !occasional {
        return None;
    }
    let value = match effect.modifier {
        Modifier::AddYield { yield_kind, amount } => w.yield_weight(yield_kind) * amount,
        Modifier::AddYieldPerCount { yield_kind, amount, per } => {
            w.yield_weight(yield_kind) * per.scale(amount, 3 * u32::from(per.every))
        }
        _ => 0,
    };
    Some(value)
}

/// Value of a city-state number: influence a turn, flags of allied
/// city-states' gifts, the gold of trade missions.
fn city_state_value(stat: crate::classic::effects::CityStateStat, amount: i32, w: &Weights) -> i32 {
    use crate::classic::effects::CityStateStat;
    match stat {
        CityStateStat::AfraidInfluence | CityStateStat::TradeRouteInfluence => {
            w.culture * amount.clamp(-20, 20) * 2
        }
        CityStateStat::UnitGiftRatePercent => w.military * amount.clamp(-100, 100) / 10,
        CityStateStat::AllyResourceBonus => w.happiness * 20,
        CityStateStat::AllyGreatPeople => w.culture * 30,
        CityStateStat::AllyScience => w.science * 20,
        CityStateStat::GreatPersonGiftInfluence => w.culture * amount.clamp(0, 200) / 10,
        CityStateStat::TradeMissionGoldPercent => w.gold * amount.clamp(-100, 200) / 5,
        CityStateStat::RestingPoint
        | CityStateStat::RestingPointSameReligion
        | CityStateStat::DecayPercent
        | CityStateStat::GoldGiftPercent
        | CityStateStat::BullyScorePercent
        | CityStateStat::UnitGiftInfluence
        | CityStateStat::FriendBonusPercent
        | CityStateStat::FriendshipRate
        | CityStateStat::AnnexCityStates => w.culture * amount.clamp(-100, 100) / 5,
    }
}

/// Value of a stat (rules::stats): great people rates, kept food, yields
/// of specialists.
fn stat_value(stat: crate::rules::stats::Stat, amount: i32, w: &Weights) -> i32 {
    use crate::rules::stats::{Stat, TradeStat};
    match stat {
        Stat::Module(crate::rules::stats::ClassicStat::GreatPeopleRate) | Stat::Module(crate::rules::stats::ClassicStat::GreatPersonRate(_)) => w.culture * amount / 2,
        Stat::FoodKept => w.growth * amount / 2,
        Stat::SpecialistYield { yield_kind, .. } => w.yield_weight(yield_kind) * amount * 5,
        // Once per city with a specialist.
        Stat::AnySpecialistYield(yield_kind) => w.yield_weight(yield_kind) * amount * 5,
        Stat::Module(crate::rules::stats::ClassicStat::HappinessPerCity) | Stat::Module(crate::rules::stats::ClassicStat::HappinessPerLuxury) => w.happiness * amount * 10,
        Stat::Module(crate::rules::stats::ClassicStat::NaturalWonderHappinessPercent) | Stat::Module(crate::rules::stats::ClassicStat::GoldenAgeLengthPercent) => w.happiness * amount / 10,
        // Negative percents reduce unhappiness: worth happiness.
        Stat::Module(crate::rules::stats::ClassicStat::CityCountUnhappinessPercent) | Stat::Module(crate::rules::stats::ClassicStat::PopulationUnhappinessPercent) => -w.happiness * amount / 2,
        // One more route is a few gold a turn for 30 turns.
        Stat::Module(crate::rules::stats::ClassicStat::Trade(TradeStat::Routes)) => w.gold * amount * 30,
        // Venice: twice the routes, about four more mid-game.
        Stat::Module(crate::rules::stats::ClassicStat::Trade(TradeStat::RoutesPercent)) => w.gold * amount * 6 / 5,
        Stat::Module(crate::rules::stats::ClassicStat::Trade(TradeStat::ResourceGoldPercent)) => w.gold * amount / 20,
        Stat::Module(crate::rules::stats::ClassicStat::Trade(TradeStat::IncomingRouteGold)) => w.gold * amount * 5,
        Stat::Module(crate::rules::stats::ClassicStat::Trade(TradeStat::CityStateRouteGold | TradeStat::SharedIdeologyRouteGold)) => {
            w.gold * amount * 3
        }
        Stat::Module(crate::rules::stats::ClassicStat::Trade(TradeStat::ReligiousPressurePercent)) => w.faith * amount / 10,
        Stat::Module(crate::rules::stats::ClassicStat::Trade(_)) => w.gold * amount,
        // Plunder is gold now and then, for an army that takes it.
        Stat::Module(crate::rules::stats::ClassicStat::PlunderGoldPercent) => w.gold * w.military * amount / 50,
        // The captor's share: a small loss for the owner.
        Stat::Module(crate::rules::stats::ClassicStat::CapturedGoldPercent) => -w.gold * amount / 50,

        // A copy of a strategic resource is units; of a luxury, happiness.
        Stat::ResourceCopies(_) => STRATEGIC_RESOURCE_WEIGHT * w.military.max(1) * amount * 5,
        Stat::ResourceQuantityPercent(_) | Stat::StrategicQuantityPercent => {
            STRATEGIC_RESOURCE_WEIGHT * w.military.max(1) * amount / 10
        }
        Stat::ExtraLuxuryCopies => w.gold * amount * 20,
        // Faster workers: improvements sooner in every city.
        Stat::WorkerSpeedPercent => (w.food + w.production) * amount / 4,
        // Negative percents save road gold.
        Stat::RouteMaintenancePercent => -w.gold * amount / 10,
        Stat::Module(crate::rules::stats::ClassicStat::Rule(rule)) => rule_value(rule, w),
        Stat::Module(crate::rules::stats::ClassicStat::Society(s)) => society_value(s, amount, w),
        Stat::Module(crate::rules::stats::ClassicStat::Espionage(e)) => espionage_stat_value(e, amount, w),
        // A delegate is a step toward the World Leader vote and a voice on
        // every resolution.
        Stat::Module(crate::rules::stats::ClassicStat::League(crate::rules::stats::LeagueStat::Delegates)) => (w.culture + w.gold) * amount * 10,
        Stat::Module(crate::rules::stats::ClassicStat::League(crate::rules::stats::LeagueStat::DelegatesPerDiplomat)) => (w.culture + w.gold) * amount * 5,
        Stat::Module(crate::rules::stats::ClassicStat::ConnectionGoldPercent) => w.gold * amount / 2,
        Stat::Module(crate::rules::stats::ClassicStat::ResearchAgreementPercent) | Stat::Module(crate::rules::stats::ClassicStat::GreatScientistSciencePercent) => w.science * amount / 5,
        Stat::Module(crate::rules::stats::ClassicStat::GreatPeopleRatePerFriend) => w.culture * amount / 2,
        Stat::Module(crate::rules::stats::ClassicStat::FreeMaintenanceUnits) => w.gold * amount,
        // A chance of a free unit and 25 gold per camp fight.
        Stat::Module(crate::rules::stats::ClassicStat::LandBarbarianConversionPercent) => w.gold * amount / 10,
    }
}

/// Value of a society stat (rules::stats::SocietyStat).
fn society_value(stat: crate::rules::stats::SocietyStat, amount: i32, w: &Weights) -> i32 {
    use crate::rules::stats::SocietyStat;
    match stat {
        // Negative percents: specialists eat less, make less unhappiness.
        SocietyStat::SpecialistFoodPercent => -w.food * amount / 5,
        SocietyStat::SpecialistUnhappinessPercent => -w.happiness * amount / 5,
        SocietyStat::NoOccupiedUnhappiness => w.happiness * 30,
        SocietyStat::WonderCulturePercent => w.culture * amount / 5,
        SocietyStat::HappinessToCulturePercent => w.culture * amount / 2,
        SocietyStat::ScienceWhileHappyPercent => w.science * amount * 2,
        SocietyStat::LuxuryRetentionPercent => w.happiness * amount / 10,
        SocietyStat::RazeSpeedPercent => w.military * amount / 20,
        SocietyStat::FoundingTerritoryTiles => w.growth * amount * 5,
        SocietyStat::CapitalScienceBuildingBoost => w.science * 30,
        SocietyStat::UniqueLuxuryCities => w.happiness * amount * 20,
        SocietyStat::UniqueLuxuryQuantity => w.gold * amount * 5,
        SocietyStat::UniqueLuxuryNewLandmass => 0,
    }
}

/// Value of an exception to a rule (`PlayerRule`).
fn rule_value(rule: crate::rules::stats::PlayerRule, w: &Weights) -> i32 {
    use crate::rules::stats::PlayerRule;
    match rule {
        // A few gold a turn per garrisoned city.
        PlayerRule::GarrisonFreeMaintenance => w.gold * 20 + w.military * 10,
        PlayerRule::FreeHillsRouteMaintenance => w.gold * 10,
        // The space race: parts bought or hurried.
        PlayerRule::SpaceshipPartPurchase | PlayerRule::SpaceshipPartHurry => w.science * 30,
        // Seeing new barbarian camps: a little safety.
        PlayerRule::SeeBarbarianCamps => w.military * 2,
    }
}

/// Points of one spy and of one spy rank (see `stat_value`).
const SPY_VALUE: i32 = 30;
const SPY_RANK_VALUE: i32 = 15;

/// Value of an espionage stat: theft is science, defence keeps the
/// science lead (a lower enemy steal rate is worth as much as a higher own
/// one), rigged elections are city-state influence (culture weight).
fn espionage_stat_value(stat: crate::rules::stats::EspionageStat, amount: i32, w: &Weights) -> i32 {
    use crate::rules::stats::EspionageStat;
    match stat {
        EspionageStat::EnemyStealPercent => -w.science * amount / 5,
        EspionageStat::StealPercent => w.science * amount / 5,
        EspionageStat::CatchPercent => w.science * amount / 10,
        EspionageStat::RigElectionPercent => w.culture * amount / 10,
        EspionageStat::ExtraSpies => w.science * amount * SPY_VALUE,
        EspionageStat::StartingRank => w.science * amount * SPY_RANK_VALUE,
        EspionageStat::SpyPressure => w.faith * amount * 5,
    }
}

/// Value of one World Congress project reward (points, as `effect_value`).
pub fn league_reward_value(reward: &crate::classic::tables::LeagueReward, w: &Weights, cities: i32) -> i32 {
    use crate::classic::tables::LeagueReward;
    match *reward {
        LeagueReward::Happiness(n) => w.happiness * i32::from(n) * 10,
        LeagueReward::FreePolicies(n) => w.culture * i32::from(n) * 30,
        LeagueReward::GoldenAgePoints(n) => w.happiness * (n.min(10_000) as i32) / 50,
        // +100 % culture or tourism for n turns: about the empire's
        // culture of those turns.
        LeagueReward::CultureBoostTurns(n) | LeagueReward::TourismBoostTurns(n) => {
            w.culture * i32::from(n) * cities.max(1)
        }
        LeagueReward::CityStateInfluence(n) => w.culture * i32::from(n) / 2,
        LeagueReward::ResearchTurns(n) => w.science * i32::from(n) * cities.max(1) * 3,
        LeagueReward::FreeUnit(_) => w.science * 40,
        LeagueReward::Building(_) => w.production * 30 + 20 * w.culture,
    }
}

/// What a World Congress project is worth to a member that contributes a
/// full share (tiers 1 and 2).
pub fn league_project_value(rules: &CompiledRuleset, project: crate::rules::content::ProjectId, w: &Weights, cities: i32) -> i32 {
    let Some(league) = rules.module.projects.get(project.0 as usize).and_then(|p| p.league.as_ref()) else {
        return 0;
    };
    league.tiers[..2]
        .iter()
        .flatten()
        .map(|r| league_reward_value(r, w, cities))
        .sum()
}

/// Value of a source's effects (passive and triggered).
pub fn effects_value(set: &EffectSet, w: &Weights, cities: i32) -> i32 {
    let passive = set
        .passive_player
        .iter()
        .chain(&set.passive_city)
        .chain(&set.passive_unit)
        .chain(&set.passive_tile);
    let triggered = set.triggered.iter().flatten();
    passive
        .chain(triggered)
        .map(|e| effect_value(e, w, cities))
        .sum()
}

/// Value of a building in one city: its effects minus its upkeep.
/// Gold a building adds (flat and percent effects on gold, its own city).
pub fn building_gold(rules: &CompiledRuleset, kind: crate::ids::BuildingKindId) -> i32 {
    let Some(def) = rules.buildings.get(kind.0 as usize) else {
        return 0;
    };
    def.effects
        .passive_city
        .iter()
        .chain(&def.effects.passive_player)
        .map(|e| match e.modifier {
            Modifier::AddYield { yield_kind: YieldKind::Gold, amount } => amount,
            Modifier::AddYieldPercent { yield_kind: YieldKind::Gold, percent } => percent.signum(),
            _ => 0,
        })
        .sum()
}

pub fn building_value(
    rules: &CompiledRuleset,
    kind: BuildingKindId,
    w: &Weights,
    cities: i32,
) -> i32 {
    let Some(def) = rules.buildings.get(kind.0 as usize) else {
        return 0;
    };
    // Upkeep at half a yield: it is paid from the empire's surplus, and
    // `planner_cities` builds no new upkeep at a deficit. A full yield
    // made a Shrine (1 faith, 1 gold) worth nothing under the real
    // maintenance, and no AI founded a pantheon.
    let upkeep = def.maintenance as i32 * 5 * w.gold.max(1);
    let wonder = if def.wonder { 20 * w.culture } else { 0 };
    effects_value(&def.effects, w, cities) + wonder - upkeep
}

/// Value of an improvement on a tile without it: its yields (resource
/// unlocks are counted where a resource is known).
pub fn improvement_value(rules: &CompiledRuleset, kind: ImprovementId, w: &Weights) -> i32 {
    let Some(def) = rules.improvements.get(kind.0 as usize) else {
        return 0;
    };
    let y = def.yields;
    (w.food * y.food + w.production * y.production + w.gold * y.gold) * 10
}

/// Value of what a resource gives once improved.
pub fn resource_value(rules: &CompiledRuleset, resource: ResourceId, w: &Weights) -> i32 {
    let Some(def) = rules.resources.get(resource.0 as usize) else {
        return 0;
    };
    let improved = def.improved_yields.first().map(|(_, y)| *y).unwrap_or_default();
    let gold = def.yields.gold + improved.gold;
    let base = w.food * (def.yields.food + improved.food)
        + w.production * (def.yields.production + improved.production);
    // A strategic resource is worth its units (iron, horses: the army).
    let strategic = def.class == Some(crate::rules::content::ResourceClass::Strategic);
    let army = if strategic { STRATEGIC_RESOURCE_WEIGHT * w.military.max(1) } else { 0 };
    (base + w.gold * gold + w.happiness * def.module.happiness + army) * 10
}

/// Yield-equivalents of connecting a strategic resource, per military weight.
pub const STRATEGIC_RESOURCE_WEIGHT: i32 = 3;

/// Combat weight of a unit kind (melee or ranged strength).
pub fn unit_strength(rules: &CompiledRuleset, kind: UnitKindId) -> i32 {
    match rules.units.get(kind.0 as usize) {
        Some(def) => i32::from(def.strength.max(def.ranged_strength)),
        None => 0,
    }
}

/// Value of a technology: what it unlocks and its own effects.
pub fn tech_value(
    rules: &CompiledRuleset,
    tech: TechId,
    w: &Weights,
    cities: i32,
    best_strength: i32,
) -> i32 {
    let Some(def) = rules.technologies.get(tech.0 as usize) else {
        return 0;
    };
    let buildings: i32 = def
        .unlocks_buildings
        .iter()
        .map(|b| building_value(rules, *b, w, 1).max(0) * cities.max(1) / 2)
        .sum();
    let units: i32 = def
        .unlocks_units
        .iter()
        .map(|u| (unit_strength(rules, *u) - best_strength).max(0) * w.military * 5)
        .sum();
    let improvements: i32 = def
        .unlocks_improvements
        .iter()
        .map(|i| improvement_value(rules, *i, w).max(0) * cities.max(1) / 2)
        .sum();
    // Faster roads and bridges move armies and workers; a revealed map
    // shows where to settle and whom to meet.
    let roads = if def.bridge_building || def.road_move_change != 0 { 10 * w.military.max(1) } else { 0 };
    let map = if def.map_visible { 20 } else { 0 };
    buildings + units + improvements + effects_value(&def.effects, w, cities) + roads + map + 10
}

/// Production cost of an item before city modifiers (content cost).
pub fn base_cost(rules: &CompiledRuleset, item: ProductionItem) -> u32 {
    match item {
        ProductionItem::Unit(kind) => rules
            .units
            .get(kind.0 as usize)
            .map(|u| u.cost)
            .unwrap_or(0),
        ProductionItem::Building(kind) => rules
            .buildings
            .get(kind.0 as usize)
            .map(|b| b.cost)
            .unwrap_or(0),
        ProductionItem::Project(id) => rules
            .module.projects
            .get(id.0 as usize)
            .map(|p| p.cost)
            .unwrap_or(0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Gold weighs more at a deficit, happiness while unhappy; a healthy
    /// empire keeps its strategy's weights.
    #[test]
    fn weights_follow_the_treasury_and_the_mood() {
        let base = Weights::of(Strategy::Expand);
        assert_eq!(Weights::for_empire(Strategy::Expand, 5, 3), base);
        let broke = Weights::for_empire(Strategy::Expand, -2, 3);
        assert_eq!(broke.gold, base.gold + DEFICIT_GOLD_WEIGHT);
        let unhappy = Weights::for_empire(Strategy::Expand, 5, -1);
        assert_eq!(unhappy.happiness, base.happiness + UNHAPPY_HAPPINESS_WEIGHT);
        assert_eq!(unhappy.gold, base.gold);
    }
}
