//! RON mirror of `classic_rules::rules::stats::SourceStat` (the stats of the
//! economy and society systems that `AddStat` effects add to).

use classic_rules::rules::stats::{
    PlayerRule, SourceStat, TradeDomain, TradeStat, SocietyStat, EspionageStat, LeagueStat,
};
use serde::Deserialize;

use crate::ron_types::{key, opt_key, RonGreatPersonKind, RonYieldKind};

#[derive(Debug, Deserialize)]
pub(crate) enum RonStat {
    GreatPeopleRate,
    GreatPersonRate(RonGreatPersonKind),
    FoodKept,
    SpecialistYield {
        #[serde(default)]
        specialist: Option<String>,
        yield_kind: RonYieldKind,
    },
    HappinessPerCity,
    HappinessPerLuxury,
    NaturalWonderHappinessPercent,
    CityCountUnhappinessPercent,
    PopulationUnhappinessPercent,
    GoldenAgeLengthPercent,
    Trade(RonTradeStat),
    PlunderGoldPercent,
    CapturedGoldPercent,
    ResourceCopies(String),
    ResourceQuantityPercent(String),
    StrategicQuantityPercent,
    ExtraLuxuryCopies,
    WorkerSpeedPercent,
    RouteMaintenancePercent,
    Rule(RonPlayerRule),
    Society(RonSocietyStat),
    Espionage(RonEspionageStat),
    League(RonLeagueStat),
    AnySpecialistYield(RonYieldKind),
    ConnectionGoldPercent,
    ResearchAgreementPercent,
    GreatScientistSciencePercent,
    GreatPeopleRatePerFriend,
    FreeMaintenanceUnits,
    LandBarbarianConversionPercent,
}

#[derive(Debug, Clone, Copy, Deserialize)]
pub(crate) enum RonEspionageStat {
    EnemyStealPercent,
    StealPercent,
    CatchPercent,
    RigElectionPercent,
    ExtraSpies,
    StartingRank,
    SpyPressure,
}

impl From<RonEspionageStat> for EspionageStat {
    fn from(s: RonEspionageStat) -> Self {
        match s {
            RonEspionageStat::EnemyStealPercent => EspionageStat::EnemyStealPercent,
            RonEspionageStat::StealPercent => EspionageStat::StealPercent,
            RonEspionageStat::CatchPercent => EspionageStat::CatchPercent,
            RonEspionageStat::RigElectionPercent => EspionageStat::RigElectionPercent,
            RonEspionageStat::ExtraSpies => EspionageStat::ExtraSpies,
            RonEspionageStat::StartingRank => EspionageStat::StartingRank,
            RonEspionageStat::SpyPressure => EspionageStat::SpyPressure,
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize)]
pub(crate) enum RonLeagueStat {
    Delegates,
    DelegatesPerDiplomat,
}

impl From<RonLeagueStat> for LeagueStat {
    fn from(s: RonLeagueStat) -> Self {
        match s {
            RonLeagueStat::Delegates => LeagueStat::Delegates,
            RonLeagueStat::DelegatesPerDiplomat => LeagueStat::DelegatesPerDiplomat,
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize)]
pub(crate) enum RonSocietyStat {
    SpecialistFoodPercent,
    SpecialistUnhappinessPercent,
    NoOccupiedUnhappiness,
    WonderCulturePercent,
    HappinessToCulturePercent,
    ScienceWhileHappyPercent,
    LuxuryRetentionPercent,
    RazeSpeedPercent,
    FoundingTerritoryTiles,
    CapitalScienceBuildingBoost,
    UniqueLuxuryCities,
    UniqueLuxuryQuantity,
    UniqueLuxuryNewLandmass,
}

impl From<RonSocietyStat> for SocietyStat {
    fn from(s: RonSocietyStat) -> Self {
        match s {
            RonSocietyStat::SpecialistFoodPercent => SocietyStat::SpecialistFoodPercent,
            RonSocietyStat::SpecialistUnhappinessPercent => SocietyStat::SpecialistUnhappinessPercent,
            RonSocietyStat::NoOccupiedUnhappiness => SocietyStat::NoOccupiedUnhappiness,
            RonSocietyStat::WonderCulturePercent => SocietyStat::WonderCulturePercent,
            RonSocietyStat::HappinessToCulturePercent => SocietyStat::HappinessToCulturePercent,
            RonSocietyStat::ScienceWhileHappyPercent => SocietyStat::ScienceWhileHappyPercent,
            RonSocietyStat::LuxuryRetentionPercent => SocietyStat::LuxuryRetentionPercent,
            RonSocietyStat::RazeSpeedPercent => SocietyStat::RazeSpeedPercent,
            RonSocietyStat::FoundingTerritoryTiles => SocietyStat::FoundingTerritoryTiles,
            RonSocietyStat::CapitalScienceBuildingBoost => SocietyStat::CapitalScienceBuildingBoost,
            RonSocietyStat::UniqueLuxuryCities => SocietyStat::UniqueLuxuryCities,
            RonSocietyStat::UniqueLuxuryQuantity => SocietyStat::UniqueLuxuryQuantity,
            RonSocietyStat::UniqueLuxuryNewLandmass => SocietyStat::UniqueLuxuryNewLandmass,
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize)]
pub(crate) enum RonPlayerRule {
    GarrisonFreeMaintenance,
    FreeHillsRouteMaintenance,
    SpaceshipPartPurchase,
    SpaceshipPartHurry,
    SeeBarbarianCamps,
}

impl From<RonPlayerRule> for PlayerRule {
    fn from(r: RonPlayerRule) -> Self {
        match r {
            RonPlayerRule::GarrisonFreeMaintenance => PlayerRule::GarrisonFreeMaintenance,
            RonPlayerRule::FreeHillsRouteMaintenance => PlayerRule::FreeHillsRouteMaintenance,
            RonPlayerRule::SpaceshipPartPurchase => PlayerRule::SpaceshipPartPurchase,
            RonPlayerRule::SpaceshipPartHurry => PlayerRule::SpaceshipPartHurry,
            RonPlayerRule::SeeBarbarianCamps => PlayerRule::SeeBarbarianCamps,
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize)]
pub(crate) enum RonTradeDomain {
    Land,
    Sea,
}

impl From<RonTradeDomain> for TradeDomain {
    fn from(d: RonTradeDomain) -> Self {
        match d {
            RonTradeDomain::Land => TradeDomain::Land,
            RonTradeDomain::Sea => TradeDomain::Sea,
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize)]
pub(crate) enum RonTradeStat {
    Routes,
    RangeTiles(RonTradeDomain),
    RangePercent(RonTradeDomain),
    RouteGold(RonTradeDomain),
    TargetGold,
    RecipientGold,
    InternalPercent,
    RoutesPercent,
    ResourceGoldPercent,
    IncomingRouteGold,
    CityStateRouteGold,
    SharedIdeologyRouteGold,
    ReligiousPressurePercent,
}

impl From<RonTradeStat> for TradeStat {
    fn from(t: RonTradeStat) -> Self {
        match t {
            RonTradeStat::Routes => TradeStat::Routes,
            RonTradeStat::RangeTiles(d) => TradeStat::RangeTiles(d.into()),
            RonTradeStat::RangePercent(d) => TradeStat::RangePercent(d.into()),
            RonTradeStat::RouteGold(d) => TradeStat::RouteGold(d.into()),
            RonTradeStat::TargetGold => TradeStat::TargetGold,
            RonTradeStat::RecipientGold => TradeStat::RecipientGold,
            RonTradeStat::InternalPercent => TradeStat::InternalPercent,
            RonTradeStat::RoutesPercent => TradeStat::RoutesPercent,
            RonTradeStat::ResourceGoldPercent => TradeStat::ResourceGoldPercent,
            RonTradeStat::IncomingRouteGold => TradeStat::IncomingRouteGold,
            RonTradeStat::CityStateRouteGold => TradeStat::CityStateRouteGold,
            RonTradeStat::SharedIdeologyRouteGold => TradeStat::SharedIdeologyRouteGold,
            RonTradeStat::ReligiousPressurePercent => TradeStat::ReligiousPressurePercent,
        }
    }
}

impl From<RonStat> for SourceStat {
    fn from(s: RonStat) -> Self {
        match s {
            RonStat::GreatPeopleRate => SourceStat::Module(classic_rules::rules::stats::ClassicStat::GreatPeopleRate),
            RonStat::GreatPersonRate(kind) => SourceStat::Module(classic_rules::rules::stats::ClassicStat::GreatPersonRate(kind.into())),
            RonStat::FoodKept => SourceStat::FoodKept,
            RonStat::HappinessPerCity => SourceStat::Module(classic_rules::rules::stats::ClassicStat::HappinessPerCity),
            RonStat::HappinessPerLuxury => SourceStat::Module(classic_rules::rules::stats::ClassicStat::HappinessPerLuxury),
            RonStat::NaturalWonderHappinessPercent => SourceStat::Module(classic_rules::rules::stats::ClassicStat::NaturalWonderHappinessPercent),
            RonStat::CityCountUnhappinessPercent => SourceStat::Module(classic_rules::rules::stats::ClassicStat::CityCountUnhappinessPercent),
            RonStat::PopulationUnhappinessPercent => SourceStat::Module(classic_rules::rules::stats::ClassicStat::PopulationUnhappinessPercent),
            RonStat::GoldenAgeLengthPercent => SourceStat::Module(classic_rules::rules::stats::ClassicStat::GoldenAgeLengthPercent),
            RonStat::Trade(t) => SourceStat::Module(classic_rules::rules::stats::ClassicStat::Trade(t.into())),
            RonStat::PlunderGoldPercent => SourceStat::Module(classic_rules::rules::stats::ClassicStat::PlunderGoldPercent),
            RonStat::CapturedGoldPercent => SourceStat::Module(classic_rules::rules::stats::ClassicStat::CapturedGoldPercent),

            RonStat::ResourceCopies(k) => SourceStat::ResourceCopies(key(k)),
            RonStat::ResourceQuantityPercent(k) => SourceStat::ResourceQuantityPercent(key(k)),
            RonStat::StrategicQuantityPercent => SourceStat::StrategicQuantityPercent,
            RonStat::ExtraLuxuryCopies => SourceStat::ExtraLuxuryCopies,
            RonStat::WorkerSpeedPercent => SourceStat::WorkerSpeedPercent,
            RonStat::RouteMaintenancePercent => SourceStat::RouteMaintenancePercent,
            RonStat::Rule(rule) => SourceStat::Module(classic_rules::rules::stats::ClassicStat::Rule(rule.into())),
            RonStat::Society(s) => SourceStat::Module(classic_rules::rules::stats::ClassicStat::Society(s.into())),
            RonStat::Espionage(e) => SourceStat::Module(classic_rules::rules::stats::ClassicStat::Espionage(e.into())),
            RonStat::League(l) => SourceStat::Module(classic_rules::rules::stats::ClassicStat::League(l.into())),
            RonStat::AnySpecialistYield(y) => SourceStat::AnySpecialistYield(y.into()),
            RonStat::ConnectionGoldPercent => SourceStat::Module(classic_rules::rules::stats::ClassicStat::ConnectionGoldPercent),
            RonStat::ResearchAgreementPercent => SourceStat::Module(classic_rules::rules::stats::ClassicStat::ResearchAgreementPercent),
            RonStat::GreatScientistSciencePercent => SourceStat::Module(classic_rules::rules::stats::ClassicStat::GreatScientistSciencePercent),
            RonStat::GreatPeopleRatePerFriend => SourceStat::Module(classic_rules::rules::stats::ClassicStat::GreatPeopleRatePerFriend),
            RonStat::FreeMaintenanceUnits => SourceStat::Module(classic_rules::rules::stats::ClassicStat::FreeMaintenanceUnits),
            RonStat::LandBarbarianConversionPercent => {
                SourceStat::Module(classic_rules::rules::stats::ClassicStat::LandBarbarianConversionPercent)
            }
            RonStat::SpecialistYield {
                specialist,
                yield_kind,
            } => SourceStat::SpecialistYield {
                specialist: opt_key(specialist),
                yield_kind: yield_kind.into(),
            },
        }
    }
}
