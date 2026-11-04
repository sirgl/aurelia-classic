//! RON schema of effects (mirror of classic_rules::rules::source::SourceEffect;
//! classic-rules stays free of serde, parsing lives here — platform/structure.md §28–29).
//!
//! ```ron
//! (scope: City, selector: SourceCity, modifier: AddYield(yield_kind: Output, amount: 2))
//! (scope: City, selector: EventCity, trigger: OnCityFounded,
//!  modifier: FreeBuilding("core:building/monument"), duration: Once)
//! ```
//!
//! `scope`, `selector` and `modifier` are required. `condition` (Always),
//! `trigger` (Passive), `duration` (WhileSourceExists) and `stacking`
//! (Stack) default to the plain passive effect; unknown fields are errors,
//! so a misspelt field never silently becomes a default.

use classic_rules::classic::effects::{ClassicCondition, ClassicSourceModifier};
use classic_rules::classic::effects::{
    CityRelation, CityStateStat, ClassicCost, ClassicCount, ClassicGrant, ClassicScope, ClassicSelector,
    ClassicTrigger, ClassicUnitAbility, ClassicUnitStat, ClassicYield, GreatPersonKind, ReligionAbility,
    ReligionStat, TourismCondition, TourismStat,
};
use classic_rules::rules::content::{Domain, UnitRole};
use classic_rules::rules::effects::{
    CityCombatStat, CombatContext, CostTarget, Count, Duration, Grant, PerCount, PlotFilter, Scope,
    Selector, Stacking, Trigger, UnitAbility, UnitStat, YieldKind,
};

/// A fieldless RON enum mirroring a classic-rules enum variant by variant. The
/// words after `module` are the rules module's (`Core::Module(Module::Word)`):
/// RON names them like the kernel's words.
macro_rules! ron_mirror {
    ($ron:ident => $core:ident { $($variant:ident),* $(,)? } words { $($word:ident = $value:path),* $(,)? }) => {
        #[derive(Debug, Clone, Copy, Deserialize)]
        enum $ron {
            $($variant,)*
            $($word,)*
        }

        impl From<$ron> for $core {
            fn from(value: $ron) -> Self {
                match value {
                    $($ron::$variant => $core::$variant,)*
                    $($ron::$word => $value,)*
                }
            }
        }
    };
    ($ron:ident => $core:ident { $($variant:ident),* $(,)? }) => {
        ron_mirror!($ron => $core { $($variant),* } module Never {});
    };
    ($ron:ident => $core:ident { $($variant:ident),* $(,)? } module $module:ident { $($word:ident),* $(,)? }) => {
        #[derive(Debug, Clone, Copy, Deserialize)]
        enum $ron {
            $($variant,)*
            $($word,)*
        }

        impl From<$ron> for $core {
            fn from(value: $ron) -> Self {
                match value {
                    $($ron::$variant => $core::$variant,)*
                    $($ron::$word => $core::Module($module::$word),)*
                }
            }
        }
    };
}
ron_mirror!(RonCombatContext => CombatContext {
    Any, Attack, Defense, VsCity, AttackOpen, AttackRough, DefenseOpen, DefenseRough,
    RangedAttackOpen, RangedAttackRough, DefenseVsRanged, RangedAttack, VsWounded, VsFortified,
    FriendlyLand, OutsideFriendlyLand, AttackFriendlyLand, Flanking, AdjacentFriendly,
    NearCapital, EmbarkedDefense, Interception, AirSweep,
});
ron_mirror!(RonUnitStat => UnitStat {
    Sight, EmbarkedSight, Range, AttacksPerTurn, Cargo, InterceptChance, InterceptionsPerTurn,
    EvasionPercent, InterceptionDamageTakenPercent, ParadropRange, NavalMoves, ExperiencePercent,
    WithdrawChance, CityAttackPlunderPercent, HealEnemyLand, HealNeutralLand, HealFriendlyLand,
    HealAdjacent, HealOnKill, EnemyAuraPercent, AuraRange, AdjacentDamageOnAttack,
    CapitalDefenseFalloff, EmbarkedMoves, WoundedDamagePercent,
} module ClassicUnitStat { GreatGeneralPointsPercent, StackedGeneralPercent, MaintenancePercent, ExperienceNeededPercent, GeneralAuraPercent, TradeMissionGoldPercent, TradeMissionInfluencePercent, AdjacentDamageChance, GoldenAgePointsOnKillPercent, RivalLandStrengthLossPercent });
ron_mirror!(RonUnitAbility => UnitAbility {
    MoveAfterAttacking, AlwaysHeal, Embarkation, EmbarkedAllWater, MoveImpassable,
    RangedSupportFire, HillsDoubleMove,
    DoubleMove, Impassable, InstaHeal, Amphibious, RiverCrossing, IndirectFire,
    HealOutsideFriendly, AirSweep, RivalTerritory, RoughTerrainEndsTurn, IgnoreTerrainCost, FlatMovementCost, Hovering, NoCapture, OnlyDefensive,
    NoDefensiveBonus, MustSetUp, FreePillageMoves, CityAttackOnly,
    CaptureDefeatedEnemy, HeavyCharge, Sapper, IgnoreZoneOfControl, EmbarkedToLandFlatCost,
    Invisible, SeeInvisible, HillsIgnoreTerrainCost, CrossMountains, RoadInFriendlyLand,
} module ClassicUnitAbility { GreatGeneral, GreatAdmiral, HealOnKillNotBarbarians, PostCombatPromotions, ExoticCargo, LendsMovementToGeneral, ChoosesRuinsReward });
ron_mirror!(RonCityStateStat => CityStateStat {
    RestingPoint, DecayPercent, GoldGiftPercent, BullyScorePercent, UnitGiftInfluence,
    FriendBonusPercent, FriendshipRate, AnnexCityStates, RestingPointSameReligion, AfraidInfluence,
    TradeRouteInfluence, UnitGiftRatePercent, AllyResourceBonus, AllyGreatPeople, AllyScience,
    GreatPersonGiftInfluence, TradeMissionGoldPercent,
});
ron_mirror!(RonReligionAbility => ReligionAbility { ConvertsBarbarians, FaithPurchaseAllGreatPeople });
ron_mirror!(RonCityCombatStat => CityCombatStat {
    Defense, MaxHitPoints, DefensePercent, GarrisonRangedStrikePercent, NukeDamagePercent,
    AirCapacity, RangedStrikePercent,
});
ron_mirror!(RonCityRelation => CityRelation { Own, Enemy });
ron_mirror!(RonRole => UnitRole {
    Civilian, SpaceshipPart, Recon, Melee, Archer, Mounted, Siege,
    Gun, Armor, NavalMelee, NavalRanged, Submarine, Carrier, Fighter, Bomber, Helicopter, Missile,
} words {
    GreatPerson = classic_rules::classic::content::GREAT_PERSON_ROLE,
    Religious = classic_rules::classic::content::RELIGIOUS_ROLE,
});
ron_mirror!(RonDomain => Domain { Land, Sea, Air });
use classic_rules::rules::source::{
    SourceCondition, SourceEffect, SourceModifier, SourceProductionFilter, StableContentKey,
    SourceForbidden,
};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RonEffect {
    scope: RonScope,
    selector: RonSelector,
    #[serde(default)]
    condition: RonCondition,
    #[serde(default)]
    second_condition: RonCondition,
    #[serde(default)]
    trigger: RonTrigger,
    modifier: RonModifier,
    #[serde(default)]
    duration: RonDuration,
    #[serde(default)]
    stacking: RonStacking,
}

#[derive(Debug, Deserialize)]
enum RonScope {
    Player,
    City,
    Unit,
    Tile,
    Religion,
}

#[derive(Debug, Deserialize)]
enum RonSelector {
    Owner,
    SourceCity,
    OwnerCities,
    EventCity,
    SourceUnit,
    OwnerUnits,
    EventUnit,
    SourceCityTiles,
    OwnerCityTiles,
    Capital,
    FollowingCities,
    FollowingCityTiles,
    HolyCity,
    Founder,
    OwnReligion,
}

#[derive(Debug, Default, Deserialize)]
enum RonCondition {
    #[default]
    Always,
    HasTechnology(String),
    LacksTechnology(String),
    MinPopulation(u32),
    HasBuilding(String),
    TileImprovement(String),
    TileResource(String),
    TileFeature(String),
    TileTerrain(String),
    TileTerrainUntil(String, String),
    OpponentRole(RonRole),
    OpponentDomain(RonDomain),
    OpponentClassOf(String),
    OpponentIsBarbarian,
    OpponentIsCityState,
    OpponentHasMoreCities,
    UnitDomain(RonDomain),
    UnitRole(RonRole),
    MinFollowers(u32),
    EventNearSourceCity(u8),
    NearFollowingCity(RonCityRelation),
    TilePlot(RonPlotFilter),
    TileResourceClass(crate::ron_types::RonResourceClass),
    TileFeatureUnimproved(String),
    CityAdjacentUnimprovedFeature { feature: String, at_least: u8 },
    Producing(RonProductionFilter),
    GoldenAge,
    CityGarrisoned,
    CityCoastal,
    LearnedTechnology(String),
    CityOnRiver,
    AtPeace,
    ConnectedToCapital,
    GreatPersonIs(RonGreatPerson),
    GreatGeneralEarned,
    NearOwnImprovement { improvement: String, range: u8 },
}

#[derive(Debug, Deserialize)]
enum RonProductionFilter {
    Units,
    UnitDomain(RonDomain),
    UnitRole(RonRole),
    UnitClassOf(String),
    MilitaryUnits,
    SpaceshipParts,
    Buildings,
    WorldWonders {
        #[serde(default)]
        before_era: Option<String>,
    },
    BuildingClassOf(String),
    BuiltInCapital,
}

fn production_filter(f: RonProductionFilter) -> SourceProductionFilter {
    match f {
        RonProductionFilter::Units => SourceProductionFilter::Units,
        RonProductionFilter::UnitDomain(d) => SourceProductionFilter::UnitDomain(d.into()),
        RonProductionFilter::UnitRole(r) => SourceProductionFilter::UnitRole(r.into()),
        RonProductionFilter::UnitClassOf(k) => SourceProductionFilter::UnitClassOf(key(k)),
        RonProductionFilter::MilitaryUnits => SourceProductionFilter::MilitaryUnits,
        RonProductionFilter::SpaceshipParts => SourceProductionFilter::SpaceshipParts,
        RonProductionFilter::Buildings => SourceProductionFilter::Buildings,
        RonProductionFilter::WorldWonders { before_era } => SourceProductionFilter::WorldWonders {
            before_era: before_era.map(key),
        },
        RonProductionFilter::BuildingClassOf(k) => SourceProductionFilter::BuildingClassOf(key(k)),
        RonProductionFilter::BuiltInCapital => SourceProductionFilter::BuiltInCapital,
    }
}

ron_mirror!(RonPlotFilter => PlotFilter { River, Sea, Lake, SeaResource, NaturalWonder });

#[derive(Debug, Default, Deserialize)]
enum RonTrigger {
    #[default]
    Passive,
    OnBuilt,
    OnTurnStart,
    OnTechnologyUnlocked,
    OnCityFounded,
    OnUnitKilled,
    OnUnitTrained,
    OnAdopt,
    OnNaturalWonderFoundFirst,
    OnNaturalWonderFound,
    OnCityConquered,
    OnEraEntered,
    OnLongCountCycle,
    OnGreatPersonExpended,
    OnCityConverted,
    OnReligionSpread,
    OnGreatPersonBorn,
}

#[derive(Debug, Default, Deserialize)]
enum RonDuration {
    #[default]
    WhileSourceExists,
    Once,
    Turns(u16),
}

#[derive(Debug, Default, Deserialize)]
enum RonStacking {
    #[default]
    Stack,
    Unique,
}

#[derive(Debug, Clone, Copy, Deserialize)]
enum RonYield {
    Output,
    Food,
    Production,
    Gold,
    Science,
    Culture,
    Faith,
}

#[derive(Debug, Clone, Copy, Deserialize)]
enum RonCostTarget {
    Units,
    Buildings,
    Technologies,
    TilePurchase,
    Hurry,
    BorderGrowth,
    BorderGrowthExponent,
    Policies,
    PolicyCityPenalty,
    FaithPurchase,
    UnitUpgrade,
    UnitPurchase,
    BuildingPurchase,
}

#[derive(Debug, Clone, Copy, Deserialize)]
enum RonGreatPerson {
    Artist,
    Engineer,
    Merchant,
    Scientist,
    Writer,
    Musician,
    General,
    Admiral,
    Prophet,
}

#[derive(Debug, Deserialize)]
enum RonModifier {
    AddYield {
        yield_kind: RonYield,
        amount: i32,
    },
    AddYieldPercent {
        yield_kind: RonYield,
        percent: i32,
    },
    MultiplyYield {
        yield_kind: RonYield,
        percent: i32,
    },
    ModifyGrowth {
        percent: i32,
    },
    ModifyCost {
        target: RonCostTarget,
        percent: i32,
    },
    AddYieldPerPopulation {
        yield_kind: RonYield,
        hundredths: i32,
    },
    ModifyCombat {
        context: RonCombatContext,
        percent: i32,
    },
    ModifyMovement {
        amount: i16,
    },
    AddHappiness {
        amount: i32,
    },
    AddGreatPersonPoints {
        kind: RonGreatPerson,
        amount: i32,
    },
    UnlockUnit(String),
    UnlockBuilding(String),
    FreeUnit {
        unit: String,
        count: u8,
    },
    FreeBuilding(String),
    AddPromotion(String),
    AddStat {
        stat: crate::stats::RonStat,
        amount: i32,
    },
    ModifyUnitStat {
        stat: RonUnitStat,
        amount: i32,
    },
    GrantAbility(RonUnitAbility),
    PostCombatPromotion(String),
    PassivePromotion(String),
    ModifyCityCombat {
        stat: RonCityCombatStat,
        amount: i32,
    },
    GrantExperience(u16),
    ModifyCityState {
        stat: RonCityStateStat,
        amount: i32,
    },
    FreePolicies {
        count: u8,
    },
    FreeBuildingInCities {
        building: String,
        count: u8,
    },
    AddYieldPerFollowingCity {
        yield_kind: RonYield,
        amount: i32,
        #[serde(default)]
        foreign_only: bool,
    },
    AddYieldPerFollowers {
        yield_kind: RonYield,
        per: u16,
        #[serde(default)]
        foreign_only: bool,
    },
    AddHappinessPerFollowingCity {
        hundredths: i32,
    },
    ModifyReligion {
        stat: RonReligionStat,
        percent: i32,
    },
    UnlockFaithBuilding(String),
    UnlockFaithUnits(String),
    AllowReformationBelief,
    ModifyTourism {
        stat: RonTourismStat,
        amount: i32,
    },
    TourismToward {
        when: RonTourismCondition,
        percent: i32,
    },
    AddYieldPerCount {
        yield_kind: RonYield,
        amount: i32,
        per: RonPerCount,
    },
    AddYieldPercentPerCount {
        yield_kind: RonYield,
        percent: i32,
        per: RonPerCount,
    },
    AddHappinessPerCount {
        amount: i32,
        per: RonPerCount,
    },
    YieldFromKill {
        yield_kind: RonYield,
        percent: i32,
    },
    Grant(RonGrant),
    Forbid(RonForbidden),
    GrantSpies {
        count: u8,
    },
    PromoteSpies {
        ranks: u8,
    },
    GrantReligionAbility(RonReligionAbility),
    ExtraFoundingBelief,
    SecondReligionPantheon,
    InstantTourism {
        amount: u32,
    },
    GrantExperiencePerGreatWork(u16),
}

#[derive(Debug, Clone, Copy, Deserialize)]
enum RonGrant {
    FreeTechnologies(u8),
    FreeGreatPeople(u8),
    LongCountGreatPerson,
    ConquestTechnology,
    GoldenAge { turns: u16 },
    MilitaryUnitCopies,
    Population(u8),
}

impl From<RonGrant> for Grant {
    fn from(g: RonGrant) -> Self {
        match g {
            RonGrant::FreeTechnologies(n) => Grant::FreeTechnologies(n),
            RonGrant::FreeGreatPeople(n) => Grant::Module(ClassicGrant::FreeGreatPeople(n)),
            RonGrant::LongCountGreatPerson => Grant::Module(ClassicGrant::LongCountGreatPerson),
            RonGrant::ConquestTechnology => Grant::ConquestTechnology,
            RonGrant::GoldenAge { turns } => Grant::Module(ClassicGrant::GoldenAge { turns }),
            RonGrant::MilitaryUnitCopies => Grant::MilitaryUnitCopies,
            RonGrant::Population(n) => Grant::Population(n),
        }
    }
}

#[derive(Debug, Deserialize)]
enum RonForbidden {
    UnitClassOf(String),
    Annexing,
}

ron_mirror!(RonCount => Count { Population, WorldWonders, ConnectedCities } module ClassicCount {
    Policies, MajorityFollowers, ReligionsPresent, PeacefulForeignFollowers,
    OtherReligionFollowers, TradePartners, CityStateRoutes,
});

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields)]
struct RonPerCount {
    count: RonCount,
    #[serde(default = "one")]
    every: u16,
    #[serde(default)]
    max: Option<i32>,
}

fn one() -> u16 {
    1
}

impl From<RonPerCount> for PerCount {
    fn from(p: RonPerCount) -> Self {
        PerCount {
            count: p.count.into(),
            every: p.every,
            max: p.max,
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize)]
enum RonTourismStat {
    Flat,
    Percent,
    GreatWorksPercent,
    LandmarksPercent,
    PerFaithBuilding,
    Spread,
    ThemingPercent,
    GreatWorkCulture,
}

#[derive(Debug, Clone, Copy, Deserialize)]
enum RonTourismCondition {
    SharedReligion,
    TradeRoute,
    OpenBorders,
    SameIdeology,
    LessHappy,
    CommonEnemy,
}

#[derive(Debug, Clone, Copy, Deserialize)]
enum RonReligionStat {
    PressureOut,
    SpreadDistance,
    MissionaryStrength,
    ProphetStrength,
    ProphetCost,
    MissionaryCost,
    FriendlyCityStatePressure,
    OtherReligionErosion,
    PressureRetention,
}

pub(crate) fn effects(src: Vec<RonEffect>) -> Vec<SourceEffect> {
    src.into_iter().map(effect).collect()
}

fn effect(e: RonEffect) -> SourceEffect {
    SourceEffect {
        scope: scope(e.scope),
        selector: selector(e.selector),
        condition: condition(e.condition),
        second_condition: condition(e.second_condition),
        trigger: trigger(e.trigger),
        modifier: modifier(e.modifier),
        duration: match e.duration {
            RonDuration::WhileSourceExists => Duration::WhileSourceExists,
            RonDuration::Once => Duration::Once,
            RonDuration::Turns(n) => Duration::Turns(n),
        },
        stacking: match e.stacking {
            RonStacking::Stack => Stacking::Stack,
            RonStacking::Unique => Stacking::Unique,
        },
    }
}

fn key(s: String) -> StableContentKey {
    StableContentKey::new(s)
}

fn scope(s: RonScope) -> Scope {
    match s {
        RonScope::Player => Scope::Player,
        RonScope::City => Scope::City,
        RonScope::Unit => Scope::Unit,
        RonScope::Tile => Scope::Tile,
        RonScope::Religion => Scope::Module(ClassicScope::Religion),
    }
}

fn selector(s: RonSelector) -> Selector {
    match s {
        RonSelector::Owner => Selector::Owner,
        RonSelector::SourceCity => Selector::SourceCity,
        RonSelector::OwnerCities => Selector::OwnerCities,
        RonSelector::EventCity => Selector::EventCity,
        RonSelector::SourceUnit => Selector::SourceUnit,
        RonSelector::OwnerUnits => Selector::OwnerUnits,
        RonSelector::EventUnit => Selector::EventUnit,
        RonSelector::SourceCityTiles => Selector::SourceCityTiles,
        RonSelector::OwnerCityTiles => Selector::OwnerCityTiles,
        RonSelector::Capital => Selector::Capital,
        RonSelector::FollowingCities => Selector::Module(ClassicSelector::FollowingCities),
        RonSelector::FollowingCityTiles => Selector::Module(ClassicSelector::FollowingCityTiles),
        RonSelector::HolyCity => Selector::Module(ClassicSelector::HolyCity),
        RonSelector::Founder => Selector::Module(ClassicSelector::Founder),
        RonSelector::OwnReligion => Selector::Module(ClassicSelector::OwnReligion),
    }
}

fn condition(c: RonCondition) -> SourceCondition {
    match c {
        RonCondition::Always => SourceCondition::Always,
        RonCondition::HasTechnology(k) => SourceCondition::HasTechnology(key(k)),
        RonCondition::LacksTechnology(k) => SourceCondition::LacksTechnology(key(k)),
        RonCondition::MinPopulation(n) => SourceCondition::MinPopulation(n),
        RonCondition::HasBuilding(k) => SourceCondition::HasBuilding(key(k)),
        RonCondition::TileImprovement(k) => SourceCondition::TileImprovement(key(k)),
        RonCondition::TileResource(k) => SourceCondition::TileResource(key(k)),
        RonCondition::TileFeature(k) => SourceCondition::TileFeature(key(k)),
        RonCondition::TileTerrain(k) => SourceCondition::TileTerrain(key(k)),
        RonCondition::TileTerrainUntil(k, t) => SourceCondition::TileTerrainUntil(key(k), key(t)),
        RonCondition::OpponentRole(r) => SourceCondition::OpponentRole(r.into()),
        RonCondition::OpponentDomain(d) => SourceCondition::OpponentDomain(d.into()),
        RonCondition::OpponentClassOf(k) => SourceCondition::OpponentClassOf(key(k)),
        RonCondition::OpponentIsBarbarian => SourceCondition::Module(ClassicCondition::OpponentIsBarbarian),
        RonCondition::OpponentIsCityState => SourceCondition::Module(ClassicCondition::OpponentIsCityState),
        RonCondition::OpponentHasMoreCities => SourceCondition::Module(ClassicCondition::OpponentHasMoreCities),
        RonCondition::UnitDomain(d) => SourceCondition::UnitDomain(d.into()),
        RonCondition::UnitRole(r) => SourceCondition::UnitRole(r.into()),
        RonCondition::MinFollowers(n) => SourceCondition::Module(ClassicCondition::MinFollowers(n)),
        RonCondition::EventNearSourceCity(n) => SourceCondition::EventNearSourceCity(n),
        RonCondition::NearFollowingCity(r) => SourceCondition::Module(ClassicCondition::NearFollowingCity(r.into())),

        RonCondition::TilePlot(p) => SourceCondition::TilePlot(p.into()),
        RonCondition::TileResourceClass(c) => SourceCondition::TileResourceClass(c.into()),
        RonCondition::TileFeatureUnimproved(k) => SourceCondition::TileFeatureUnimproved(key(k)),
        RonCondition::CityAdjacentUnimprovedFeature { feature, at_least } => {
            SourceCondition::CityAdjacentUnimprovedFeature {
                feature: key(feature),
                at_least,
            }
        }
        RonCondition::Producing(f) => SourceCondition::Producing(production_filter(f)),
        RonCondition::GoldenAge => SourceCondition::Module(ClassicCondition::GoldenAge),
        RonCondition::GreatGeneralEarned => SourceCondition::Module(ClassicCondition::GreatGeneralEarned),
        RonCondition::NearOwnImprovement { improvement, range } => {
            SourceCondition::NearOwnImprovement { improvement: key(improvement), range }
        }
        RonCondition::CityGarrisoned => SourceCondition::CityGarrisoned,
        RonCondition::CityCoastal => SourceCondition::CityCoastal,
        RonCondition::LearnedTechnology(k) => SourceCondition::LearnedTechnology(key(k)),
        RonCondition::CityOnRiver => SourceCondition::CityOnRiver,
        RonCondition::AtPeace => SourceCondition::AtPeace,
        RonCondition::ConnectedToCapital => SourceCondition::ConnectedToCapital,
        RonCondition::GreatPersonIs(g) => SourceCondition::Module(ClassicCondition::GreatPersonIs(great_person(g))),
    }
}

fn trigger(t: RonTrigger) -> Trigger {
    match t {
        RonTrigger::Passive => Trigger::Passive,
        RonTrigger::OnBuilt => Trigger::OnBuilt,
        RonTrigger::OnTurnStart => Trigger::OnTurnStart,
        RonTrigger::OnTechnologyUnlocked => Trigger::OnTechnologyUnlocked,
        RonTrigger::OnCityFounded => Trigger::OnCityFounded,
        RonTrigger::OnUnitKilled => Trigger::OnUnitKilled,
        RonTrigger::OnUnitTrained => Trigger::OnUnitTrained,
        RonTrigger::OnAdopt => Trigger::Module(ClassicTrigger::OnAdopt),
        RonTrigger::OnNaturalWonderFoundFirst => Trigger::Module(ClassicTrigger::OnNaturalWonderFoundFirst),
        RonTrigger::OnNaturalWonderFound => Trigger::Module(ClassicTrigger::OnNaturalWonderFound),
        RonTrigger::OnCityConquered => Trigger::OnCityConquered,
        RonTrigger::OnEraEntered => Trigger::OnEraEntered,
        RonTrigger::OnLongCountCycle => Trigger::Module(ClassicTrigger::OnLongCountCycle),
        RonTrigger::OnGreatPersonExpended => Trigger::Module(ClassicTrigger::OnGreatPersonExpended),
        RonTrigger::OnCityConverted => Trigger::Module(ClassicTrigger::OnCityConverted),
        RonTrigger::OnReligionSpread => Trigger::Module(ClassicTrigger::OnReligionSpread),
        RonTrigger::OnGreatPersonBorn => Trigger::Module(ClassicTrigger::OnGreatPersonBorn),
    }
}

fn yield_kind(y: RonYield) -> YieldKind {
    match y {
        RonYield::Output => YieldKind::Output,
        RonYield::Food => YieldKind::Food,
        RonYield::Production => YieldKind::Production,
        RonYield::Gold => YieldKind::Gold,
        RonYield::Science => YieldKind::Science,
        RonYield::Culture => YieldKind::Culture,
        RonYield::Faith => YieldKind::Module(ClassicYield::Faith),
    }
}

fn cost_target(t: RonCostTarget) -> CostTarget {
    match t {
        RonCostTarget::Units => CostTarget::Units,
        RonCostTarget::Buildings => CostTarget::Buildings,
        RonCostTarget::Technologies => CostTarget::Technologies,
        RonCostTarget::TilePurchase => CostTarget::TilePurchase,
        RonCostTarget::Hurry => CostTarget::Hurry,
        RonCostTarget::BorderGrowth => CostTarget::BorderGrowth,
        RonCostTarget::BorderGrowthExponent => CostTarget::BorderGrowthExponent,
        RonCostTarget::Policies => CostTarget::Module(ClassicCost::Policies),
        RonCostTarget::PolicyCityPenalty => CostTarget::Module(ClassicCost::PolicyCityPenalty),
        RonCostTarget::FaithPurchase => CostTarget::Module(ClassicCost::FaithPurchase),
        RonCostTarget::UnitUpgrade => CostTarget::UnitUpgrade,
        RonCostTarget::UnitPurchase => CostTarget::UnitPurchase,
        RonCostTarget::BuildingPurchase => CostTarget::BuildingPurchase,
    }
}

fn great_person(g: RonGreatPerson) -> GreatPersonKind {
    match g {
        RonGreatPerson::Artist => GreatPersonKind::Artist,
        RonGreatPerson::Engineer => GreatPersonKind::Engineer,
        RonGreatPerson::Merchant => GreatPersonKind::Merchant,
        RonGreatPerson::Scientist => GreatPersonKind::Scientist,
        RonGreatPerson::Writer => GreatPersonKind::Writer,
        RonGreatPerson::Musician => GreatPersonKind::Musician,
        RonGreatPerson::General => GreatPersonKind::General,
        RonGreatPerson::Admiral => GreatPersonKind::Admiral,
        RonGreatPerson::Prophet => GreatPersonKind::Prophet,
    }
}

fn modifier(m: RonModifier) -> SourceModifier {
    match m {
        RonModifier::AddYield {
            yield_kind: y,
            amount,
        } => SourceModifier::AddYield {
            yield_kind: yield_kind(y),
            amount,
        },
        RonModifier::AddYieldPercent {
            yield_kind: y,
            percent,
        } => SourceModifier::AddYieldPercent {
            yield_kind: yield_kind(y),
            percent,
        },
        RonModifier::MultiplyYield {
            yield_kind: y,
            percent,
        } => SourceModifier::MultiplyYield {
            yield_kind: yield_kind(y),
            percent,
        },
        RonModifier::ModifyGrowth { percent } => SourceModifier::ModifyGrowth { percent },
        RonModifier::ModifyCost { target, percent } => SourceModifier::ModifyCost {
            target: cost_target(target),
            percent,
        },
        RonModifier::AddYieldPerPopulation {
            yield_kind: y,
            hundredths,
        } => SourceModifier::AddYieldPerPopulation {
            yield_kind: yield_kind(y),
            hundredths,
        },
        RonModifier::ModifyCombat { context, percent } => SourceModifier::ModifyCombat {
            context: context.into(),
            percent,
        },
        RonModifier::ModifyMovement { amount } => SourceModifier::ModifyMovement { amount },
        RonModifier::AddHappiness { amount } => SourceModifier::Module(ClassicSourceModifier::AddHappiness { amount }),
        RonModifier::AddGreatPersonPoints { kind, amount } => {
            SourceModifier::Module(ClassicSourceModifier::AddGreatPersonPoints {
                kind: great_person(kind),
                amount,
            })
        }
        RonModifier::UnlockUnit(k) => SourceModifier::UnlockUnit(key(k)),
        RonModifier::UnlockBuilding(k) => SourceModifier::UnlockBuilding(key(k)),
        RonModifier::FreeUnit { unit, count } => SourceModifier::FreeUnit {
            unit: key(unit),
            count,
        },
        RonModifier::FreeBuilding(k) => SourceModifier::FreeBuilding(key(k)),
        RonModifier::AddPromotion(k) => SourceModifier::AddPromotion(key(k)),
        RonModifier::AddStat { stat, amount } => SourceModifier::AddStat {
            stat: stat.into(),
            amount,
        },
        RonModifier::ModifyUnitStat { stat, amount } => SourceModifier::ModifyUnitStat {
            stat: stat.into(),
            amount,
        },
        RonModifier::GrantAbility(a) => SourceModifier::GrantAbility(a.into()),
        RonModifier::PostCombatPromotion(k) => SourceModifier::PostCombatPromotion(key(k)),
        RonModifier::PassivePromotion(k) => SourceModifier::PassivePromotion(key(k)),
        RonModifier::ModifyCityCombat { stat, amount } => SourceModifier::ModifyCityCombat {
            stat: stat.into(),
            amount,
        },
        RonModifier::GrantExperience(amount) => SourceModifier::GrantExperience(amount),
        RonModifier::ModifyCityState { stat, amount } => SourceModifier::Module(ClassicSourceModifier::ModifyCityState {
            stat: stat.into(),
            amount,
        }),
        RonModifier::FreePolicies { count } => SourceModifier::Module(ClassicSourceModifier::FreePolicies { count }),
        RonModifier::FreeBuildingInCities { building, count } => {
            SourceModifier::FreeBuildingInCities {
                building: key(building),
                count,
            }
        }
        RonModifier::AddYieldPerFollowingCity {
            yield_kind: y,
            amount,
            foreign_only,
        } => SourceModifier::Module(ClassicSourceModifier::AddYieldPerFollowingCity {
            yield_kind: yield_kind(y),
            amount,
            foreign_only,
        }),
        RonModifier::AddYieldPerFollowers {
            yield_kind: y,
            per,
            foreign_only,
        } => SourceModifier::Module(ClassicSourceModifier::AddYieldPerFollowers {
            yield_kind: yield_kind(y),
            per,
            foreign_only,
        }),
        RonModifier::AddHappinessPerFollowingCity { hundredths } => {
            SourceModifier::Module(ClassicSourceModifier::AddHappinessPerFollowingCity { hundredths })
        }
        RonModifier::ModifyReligion { stat, percent } => SourceModifier::Module(ClassicSourceModifier::ModifyReligion {
            stat: religion_stat(stat),
            percent,
        }),
        RonModifier::UnlockFaithBuilding(k) => SourceModifier::Module(ClassicSourceModifier::UnlockFaithBuilding(key(k))),
        RonModifier::UnlockFaithUnits(k) => SourceModifier::Module(ClassicSourceModifier::UnlockFaithUnits(key(k))),
        RonModifier::AllowReformationBelief => SourceModifier::Module(ClassicSourceModifier::AllowReformationBelief),
        RonModifier::ModifyTourism { stat, amount } => SourceModifier::Module(ClassicSourceModifier::ModifyTourism {
            stat: tourism_stat(stat),
            amount,
        }),
        RonModifier::TourismToward { when, percent } => SourceModifier::Module(ClassicSourceModifier::TourismToward {
            when: tourism_condition(when),
            percent,
        }),
        RonModifier::AddYieldPerCount { yield_kind: y, amount, per } => SourceModifier::AddYieldPerCount {
            yield_kind: yield_kind(y),
            amount,
            per: per.into(),
        },
        RonModifier::AddYieldPercentPerCount { yield_kind: y, percent, per } => {
            SourceModifier::AddYieldPercentPerCount {
                yield_kind: yield_kind(y),
                percent,
                per: per.into(),
            }
        }
        RonModifier::AddHappinessPerCount { amount, per } => SourceModifier::Module(ClassicSourceModifier::AddHappinessPerCount {
            amount,
            per: per.into(),
        }),
        RonModifier::YieldFromKill { yield_kind: y, percent } => SourceModifier::YieldFromKill {
            yield_kind: yield_kind(y),
            percent,
        },
        RonModifier::Grant(g) => SourceModifier::Grant(g.into()),
        RonModifier::Forbid(RonForbidden::UnitClassOf(k)) => {
            SourceModifier::Forbid(SourceForbidden::UnitClassOf(key(k)))
        }
        RonModifier::Forbid(RonForbidden::Annexing) => SourceModifier::Forbid(SourceForbidden::Annexing),
        RonModifier::GrantSpies { count } => SourceModifier::Module(ClassicSourceModifier::GrantSpies { count }),
        RonModifier::PromoteSpies { ranks } => SourceModifier::Module(ClassicSourceModifier::PromoteSpies { ranks }),
        RonModifier::GrantReligionAbility(a) => SourceModifier::Module(ClassicSourceModifier::GrantReligionAbility(a.into())),
        RonModifier::ExtraFoundingBelief => SourceModifier::Module(ClassicSourceModifier::ExtraFoundingBelief),
        RonModifier::SecondReligionPantheon => SourceModifier::Module(ClassicSourceModifier::SecondReligionPantheon),
        RonModifier::InstantTourism { amount } => SourceModifier::Module(ClassicSourceModifier::InstantTourism { amount }),
        RonModifier::GrantExperiencePerGreatWork(amount) => SourceModifier::Module(ClassicSourceModifier::GrantExperiencePerGreatWork(amount)),
    }
}

fn tourism_stat(s: RonTourismStat) -> TourismStat {
    match s {
        RonTourismStat::Flat => TourismStat::Flat,
        RonTourismStat::Percent => TourismStat::Percent,
        RonTourismStat::GreatWorksPercent => TourismStat::GreatWorksPercent,
        RonTourismStat::LandmarksPercent => TourismStat::LandmarksPercent,
        RonTourismStat::PerFaithBuilding => TourismStat::PerFaithBuilding,
        RonTourismStat::Spread => TourismStat::Spread,
        RonTourismStat::ThemingPercent => TourismStat::ThemingPercent,
        RonTourismStat::GreatWorkCulture => TourismStat::GreatWorkCulture,
    }
}

fn tourism_condition(c: RonTourismCondition) -> TourismCondition {
    match c {
        RonTourismCondition::SharedReligion => TourismCondition::SharedReligion,
        RonTourismCondition::TradeRoute => TourismCondition::TradeRoute,
        RonTourismCondition::OpenBorders => TourismCondition::OpenBorders,
        RonTourismCondition::SameIdeology => TourismCondition::SameIdeology,
        RonTourismCondition::LessHappy => TourismCondition::LessHappy,
        RonTourismCondition::CommonEnemy => TourismCondition::CommonEnemy,
    }
}

fn religion_stat(s: RonReligionStat) -> ReligionStat {
    match s {
        RonReligionStat::PressureOut => ReligionStat::PressureOut,
        RonReligionStat::SpreadDistance => ReligionStat::SpreadDistance,
        RonReligionStat::MissionaryStrength => ReligionStat::MissionaryStrength,
        RonReligionStat::ProphetStrength => ReligionStat::ProphetStrength,
        RonReligionStat::ProphetCost => ReligionStat::ProphetCost,
        RonReligionStat::MissionaryCost => ReligionStat::MissionaryCost,
        RonReligionStat::FriendlyCityStatePressure => ReligionStat::FriendlyCityStatePressure,
        RonReligionStat::OtherReligionErosion => ReligionStat::OtherReligionErosion,
        RonReligionStat::PressureRetention => ReligionStat::PressureRetention,
    }
}
