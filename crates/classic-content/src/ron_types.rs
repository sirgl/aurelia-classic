//! RON mirrors of the classic content vocabulary (`classic_rules::rules::content`):
//! classic-rules has no serde, parsing lives here (platform/structure.md §28–29). Unknown
//! variants and fields are parse errors.

use classic_rules::rules::content::{
    BeliefCategory, BuildingAbility, BuildingKind, CargoKind, CityStateBonusKind, CityStateLevel,
    CityStateTrait, Domain, FreshWater, ImprovementKind, ImprovementPlacement, InertAmount,
    PointsSource, PolicyBranchKind, PolicyRole, ProjectKind, ResourceClass, SourceFeatureRemoval,
    SourceInertEffect, SourceUnitCombat, StartRegion, StartRegions, ThemingBonus, UnitAbility,
    CivilianPriority, SpecialUnit,
    UnitRole, Yields,
};
use classic_rules::classic::content;
use classic_rules::rules::effects::YieldKind;
use classic_rules::classic::effects::{ClassicYield, GreatPersonKind};
use classic_rules::rules::source::{KeyCount, StableContentKey};
use serde::Deserialize;

/// A fieldless RON enum and its conversion into the classic-rules enum.
macro_rules! mirror_enum {
    ($ron:ident => $core:ident { $($variant:ident),* $(,)? } words { $($word:ident = $value:path),* $(,)? }) => {
        #[derive(Debug, Clone, Copy, Deserialize)]
        pub(crate) enum $ron {
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
        mirror_enum!($ron => $core { $($variant),* } module Never {});
    };
    ($ron:ident => $core:ident { $($variant:ident),* $(,)? } module $module:ident { $($word:ident),* $(,)? }) => {
        #[derive(Debug, Clone, Copy, Deserialize)]
        pub(crate) enum $ron {
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
mirror_enum!(RonUnitRole => UnitRole {
    Civilian, SpaceshipPart, Recon, Melee, Archer, Mounted, Siege,
    Gun, Armor, NavalMelee, NavalRanged, Submarine, Carrier, Fighter, Bomber, Helicopter, Missile,
} words { GreatPerson = content::GREAT_PERSON_ROLE, Religious = content::RELIGIOUS_ROLE });
mirror_enum!(RonDomain => Domain { Land, Sea, Air });
mirror_enum!(RonCityStateTrait => CityStateTrait {
    Cultured, Militaristic, Maritime, Mercantile, Religious,
});
mirror_enum!(RonCityStateLevel => CityStateLevel { Friends, Allies });
mirror_enum!(RonCityStateBonusKind => CityStateBonusKind {
    CapitalFood, AllCitiesFood, Culture, Faith, Happiness, UnitGiftTurns, UnitGiftRandom,
});
mirror_enum!(RonBuildingKind => BuildingKind {
    Regular, Palace, Unique, WorldWonder, NationalWonder,
} words { Religious = content::RELIGIOUS_BUILDING, Guild = content::GUILD });
mirror_enum!(RonResourceClass => ResourceClass { Bonus, Strategic, Luxury, Artifact });

/// Where a great person's points come from (default: specialists).
#[derive(Debug, Clone, Copy, Default, Deserialize)]
pub(crate) enum RonPointsSource {
    #[default]
    Specialists,
    CombatExperience,
    Faith,
}

impl From<RonPointsSource> for PointsSource {
    fn from(s: RonPointsSource) -> Self {
        match s {
            RonPointsSource::Specialists => PointsSource::Specialists,
            RonPointsSource::CombatExperience => PointsSource::CombatExperience,
            RonPointsSource::Faith => PointsSource::Faith,
        }
    }
}
mirror_enum!(RonImprovementKind => ImprovementKind {
    Regular, Unique, AncientRuins,
} words { GreatPerson = content::GREAT_PERSON_IMPROVEMENT, BarbarianCamp = content::BARBARIAN_CAMP });
mirror_enum!(RonProjectKind => ProjectKind { Project, LeagueProject });
mirror_enum!(RonPolicyBranchKind => PolicyBranchKind { Normal, Ideology });
mirror_enum!(RonBeliefCategory => BeliefCategory {
    Pantheon, Founder, Follower, Enhancer, Reformation,
});
mirror_enum!(RonStartRegion => StartRegion { Grass, Plains, Desert, Tundra, Hills, Forest, Jungle });
mirror_enum!(RonFreshWater => FreshWater { Any, Required, Absent });
mirror_enum!(RonYieldKind => YieldKind { Output, Food, Production, Gold, Science, Culture } module ClassicYield { Faith });
mirror_enum!(RonGreatPersonKind => GreatPersonKind {
    Artist, Engineer, Merchant, Scientist, Writer, Musician, General, Admiral, Prophet,
});

// The enum comes from `mirror_enum!`, which cannot mark a default variant.
#[allow(clippy::derivable_impls)]
impl Default for RonDomain {
    fn default() -> Self {
        RonDomain::Land
    }
}

// The enum comes from `mirror_enum!`, which cannot mark a default variant.
#[allow(clippy::derivable_impls)]
impl Default for RonBuildingKind {
    fn default() -> Self {
        RonBuildingKind::Regular
    }
}

// The enum comes from `mirror_enum!`, which cannot mark a default variant.
#[allow(clippy::derivable_impls)]
impl Default for RonImprovementKind {
    fn default() -> Self {
        RonImprovementKind::Regular
    }
}

#[derive(Debug, Clone, Copy, Default, Deserialize)]
pub(crate) enum RonPolicyRole {
    #[default]
    Policy,
    Tenet(u8),
    Opener,
    Finisher,
}

impl From<RonPolicyRole> for PolicyRole {
    fn from(value: RonPolicyRole) -> Self {
        match value {
            RonPolicyRole::Policy => PolicyRole::Policy,
            RonPolicyRole::Tenet(tier) => PolicyRole::Tenet(tier),
            RonPolicyRole::Opener => PolicyRole::Opener,
            RonPolicyRole::Finisher => PolicyRole::Finisher,
        }
    }
}

/// Typed unit ability (`classic_rules::rules::content::UnitAbility`).
#[derive(Debug, Clone, Copy, Deserialize)]
pub(crate) enum RonUnitAbility {
    FoundReligion,
    SpreadReligion,
    ReligionSpreads(u8),
    ReligiousStrength(u16),
    RemoveHeresy,
    ProhibitsSpread,
    RequiresEnhancedReligion,
    OneShotTourism(u16),
    OneShotTourismPercentOthers(u16),
    CultureTurnsToCount(u16),
    Excavates,
    Food,
    GoldenAgeTurns(u16),
    BeakersTurns(u16),
    BaseHurry(u32),
    HurryMultiplier(u32),
    BaseGold(u32),
    GoldPerEra(u32),
    NoMaintenance,
    Trade,
    FaithPurchaseNeedsBelief,
    Mechanized,
    CivilianAttackPriority(RonCivilianPriority),
    Conscription(u8),
    Special(RonSpecialUnit),
    AirSlots(u8),
    WorkRate(u16),
    MoveAfterPurchase,
    NoBadGoodies,
    PurchaseOnly,
    BuyCityState,
    FoundAbroad,
    ExoticGoods(u8),
}

#[derive(Debug, Clone, Copy, Deserialize)]
pub(crate) enum RonCivilianPriority {
    Low,
    HighEarlyGameOnly,
    High,
}

#[derive(Debug, Clone, Copy, Deserialize)]
pub(crate) enum RonSpecialUnit {
    People,
    Stealth,
}

impl From<RonUnitAbility> for UnitAbility {
    fn from(value: RonUnitAbility) -> Self {
        match value {
            RonUnitAbility::FoundReligion => UnitAbility::FoundReligion,
            RonUnitAbility::SpreadReligion => UnitAbility::SpreadReligion,
            RonUnitAbility::ReligionSpreads(n) => UnitAbility::ReligionSpreads(n),
            RonUnitAbility::ReligiousStrength(n) => UnitAbility::ReligiousStrength(n),
            RonUnitAbility::RemoveHeresy => UnitAbility::RemoveHeresy,
            RonUnitAbility::ProhibitsSpread => UnitAbility::ProhibitsSpread,
            RonUnitAbility::RequiresEnhancedReligion => UnitAbility::RequiresEnhancedReligion,
            RonUnitAbility::OneShotTourism(n) => UnitAbility::OneShotTourism(n),
            RonUnitAbility::OneShotTourismPercentOthers(n) => {
                UnitAbility::OneShotTourismPercentOthers(n)
            }
            RonUnitAbility::CultureTurnsToCount(n) => UnitAbility::CultureTurnsToCount(n),
            RonUnitAbility::Excavates => UnitAbility::Excavates,
            RonUnitAbility::Food => UnitAbility::Food,
            RonUnitAbility::GoldenAgeTurns(n) => UnitAbility::GoldenAgeTurns(n),
            RonUnitAbility::BeakersTurns(n) => UnitAbility::BeakersTurns(n),
            RonUnitAbility::BaseHurry(n) => UnitAbility::BaseHurry(n),
            RonUnitAbility::HurryMultiplier(n) => UnitAbility::HurryMultiplier(n),
            RonUnitAbility::BaseGold(n) => UnitAbility::BaseGold(n),
            RonUnitAbility::GoldPerEra(n) => UnitAbility::GoldPerEra(n),
            RonUnitAbility::NoMaintenance => UnitAbility::NoMaintenance,
            RonUnitAbility::Trade => UnitAbility::Trade,
            RonUnitAbility::FaithPurchaseNeedsBelief => UnitAbility::FaithPurchaseNeedsBelief,
            RonUnitAbility::Mechanized => UnitAbility::Mechanized,
            RonUnitAbility::CivilianAttackPriority(p) => UnitAbility::CivilianAttackPriority(match p {
                RonCivilianPriority::Low => CivilianPriority::Low,
                RonCivilianPriority::HighEarlyGameOnly => CivilianPriority::HighEarlyGameOnly,
                RonCivilianPriority::High => CivilianPriority::High,
            }),
            RonUnitAbility::Conscription(n) => UnitAbility::Conscription(n),
            RonUnitAbility::Special(s) => UnitAbility::Special(match s {
                RonSpecialUnit::People => SpecialUnit::People,
                RonSpecialUnit::Stealth => SpecialUnit::Stealth,
            }),
            RonUnitAbility::AirSlots(n) => UnitAbility::AirSlots(n),
            RonUnitAbility::WorkRate(n) => UnitAbility::WorkRate(n),
            RonUnitAbility::MoveAfterPurchase => UnitAbility::MoveAfterPurchase,
            RonUnitAbility::NoBadGoodies => UnitAbility::NoBadGoodies,
            RonUnitAbility::PurchaseOnly => UnitAbility::PurchaseOnly,
            RonUnitAbility::BuyCityState => UnitAbility::BuyCityState,
            RonUnitAbility::FoundAbroad => UnitAbility::FoundAbroad,
            RonUnitAbility::ExoticGoods(n) => UnitAbility::ExoticGoods(n),
        }
    }
}

/// Typed building ability (`classic_rules::rules::content::BuildingAbility`).
#[derive(Debug, Clone, Copy, Deserialize)]
pub(crate) enum RonBuildingAbility {
    TriggersIdeologyChoice(u16),
    FreeGreatWork,
    NullifyInfluence,
    MissionaryExtraSpreads(u8),
    ReligiousPressure(u16),
    FoodTradeRoutes,
    ProductionTradeRoutes,
    CounterEspionage,
    AffectSpiesNow,
    BorderObstacle,
    Airlift,
    AllowsRangeStrike,
}

/// A theming bonus (`classic_rules::rules::content::ThemingBonus`).
#[derive(Debug, Clone, Copy, Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub(crate) struct RonThemingBonus {
    bonus: u16,
    same_era: bool,
    unique_eras: bool,
    must_be_art: bool,
    must_be_artifact: bool,
    must_be_equal_art_artifact: bool,
    requires_owner: bool,
    requires_any_but_owner: bool,
    requires_same_player: bool,
    requires_unique_players: bool,
}

impl From<RonThemingBonus> for ThemingBonus {
    fn from(t: RonThemingBonus) -> Self {
        ThemingBonus {
            bonus: t.bonus,
            same_era: t.same_era,
            unique_eras: t.unique_eras,
            must_be_art: t.must_be_art,
            must_be_artifact: t.must_be_artifact,
            must_be_equal_art_artifact: t.must_be_equal_art_artifact,
            requires_owner: t.requires_owner,
            requires_any_but_owner: t.requires_any_but_owner,
            requires_same_player: t.requires_same_player,
            requires_unique_players: t.requires_unique_players,
        }
    }
}

impl From<RonBuildingAbility> for BuildingAbility {
    fn from(value: RonBuildingAbility) -> Self {
        match value {
            RonBuildingAbility::TriggersIdeologyChoice(n) => BuildingAbility::TriggersIdeologyChoice(n),
            RonBuildingAbility::FreeGreatWork => BuildingAbility::FreeGreatWork,
            RonBuildingAbility::NullifyInfluence => BuildingAbility::NullifyInfluence,
            RonBuildingAbility::MissionaryExtraSpreads(n) => BuildingAbility::MissionaryExtraSpreads(n),
            RonBuildingAbility::ReligiousPressure(n) => BuildingAbility::ReligiousPressure(n),
            RonBuildingAbility::FoodTradeRoutes => BuildingAbility::FoodTradeRoutes,
            RonBuildingAbility::ProductionTradeRoutes => BuildingAbility::ProductionTradeRoutes,
            RonBuildingAbility::CounterEspionage => BuildingAbility::CounterEspionage,
            RonBuildingAbility::AffectSpiesNow => BuildingAbility::AffectSpiesNow,
            RonBuildingAbility::BorderObstacle => BuildingAbility::BorderObstacle,
            RonBuildingAbility::Airlift => BuildingAbility::Airlift,
            RonBuildingAbility::AllowsRangeStrike => BuildingAbility::AllowsRangeStrike,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RonYields {
    #[serde(default)]
    food: i32,
    #[serde(default)]
    production: i32,
    #[serde(default)]
    gold: i32,
    #[serde(default)]
    science: i32,
    #[serde(default)]
    culture: i32,
    #[serde(default)]
    faith: i32,
}

impl From<RonYields> for Yields {
    fn from(y: RonYields) -> Self {
        Yields {
            food: y.food,
            production: y.production,
            gold: y.gold,
            science: y.science,
            culture: y.culture,
            module: y.faith,
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RonStartRegions {
    #[serde(default)]
    prefer: Vec<RonStartRegion>,
    #[serde(default)]
    avoid: Vec<RonStartRegion>,
    #[serde(default)]
    along_ocean: bool,
}

impl From<RonStartRegions> for StartRegions {
    fn from(r: RonStartRegions) -> Self {
        StartRegions {
            prefer: r.prefer.into_iter().map(Into::into).collect(),
            avoid: r.avoid.into_iter().map(Into::into).collect(),
            along_ocean: r.along_ocean,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, Deserialize)]
pub(crate) enum RonInertAmount {
    #[default]
    None,
    Flag,
    Int(i64),
    Hundredths(i64),
}

impl From<RonInertAmount> for InertAmount {
    fn from(a: RonInertAmount) -> Self {
        match a {
            RonInertAmount::None => InertAmount::None,
            RonInertAmount::Flag => InertAmount::Flag,
            RonInertAmount::Int(v) => InertAmount::Int(v),
            RonInertAmount::Hundredths(v) => InertAmount::Hundredths(v),
        }
    }
}

/// Inert effect (see `classic_rules::rules::content`).
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RonInert {
    needs_mechanic: String,
    modifier: String,
    scope: String,
    selector: String,
    trigger: String,
    #[serde(default)]
    amount: RonInertAmount,
    #[serde(default)]
    refs: Vec<String>,
}

pub(crate) fn inert(src: Vec<RonInert>) -> Vec<SourceInertEffect> {
    let mut out = Vec::with_capacity(src.len());
    for e in src {
        out.push(SourceInertEffect {
            needs_mechanic: e.needs_mechanic,
            modifier: e.modifier,
            scope: e.scope,
            selector: e.selector,
            trigger: e.trigger,
            amount: e.amount.into(),
            refs: keys(e.refs),
        });
    }
    out
}

/// Yield a technology adds to an improvement under a fresh water condition.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RonTechYield {
    pub tech: String,
    pub yield_kind: RonYieldKind,
    pub amount: i32,
    pub fresh_water: RonFreshWater,
}

pub(crate) fn key(s: String) -> StableContentKey {
    StableContentKey::new(s)
}

pub(crate) fn opt_key(s: Option<String>) -> Option<StableContentKey> {
    s.map(key)
}

pub(crate) fn keys(src: Vec<String>) -> Vec<StableContentKey> {
    src.into_iter().map(key).collect()
}

/// `("core:resource/iron", 1)` pairs.
pub(crate) fn key_counts(src: Vec<(String, u16)>) -> Vec<KeyCount> {
    src.into_iter()
        .map(|(k, count)| KeyCount { key: key(k), count })
        .collect()
}

pub(crate) fn key_count(src: Option<(String, u16)>) -> Option<KeyCount> {
    src.map(|(k, count)| KeyCount { key: key(k), count })
}

/// Improvement placement flags (see `classic_rules::rules::content::ImprovementPlacement`).
#[derive(Debug, Clone, Copy, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RonPlacement {
    #[serde(default)]
    hills_makes_valid: bool,
    #[serde(default)]
    fresh_water_makes_valid: bool,
    #[serde(default)]
    requires_flatlands_or_fresh_water: bool,
    #[serde(default)]
    requires_feature: bool,
    #[serde(default)]
    buildable_on_resources: bool,
    #[serde(default)]
    water: bool,
    #[serde(default)]
    coastal: bool,
    #[serde(default)]
    no_two_adjacent: bool,
    #[serde(default)]
    adjacent_luxury: bool,
    #[serde(default)]
    in_adjacent_friendly: bool,
    #[serde(default)]
    outside_borders: bool,
    #[serde(default)]
    ignore_ownership: bool,
    #[serde(default)]
    only_city_state_territory: bool,
    #[serde(default)]
    requires_improvement: bool,
    #[serde(default)]
    prompt_when_complete: bool,
    #[serde(default)]
    destroyed_when_pillaged: bool,
}

impl From<RonPlacement> for ImprovementPlacement {
    fn from(p: RonPlacement) -> Self {
        ImprovementPlacement {
            hills_makes_valid: p.hills_makes_valid,
            fresh_water_makes_valid: p.fresh_water_makes_valid,
            requires_flatlands_or_fresh_water: p.requires_flatlands_or_fresh_water,
            requires_feature: p.requires_feature,
            buildable_on_resources: p.buildable_on_resources,
            water: p.water,
            coastal: p.coastal,
            no_two_adjacent: p.no_two_adjacent,
            adjacent_luxury: p.adjacent_luxury,
            in_adjacent_friendly: p.in_adjacent_friendly,
            outside_borders: p.outside_borders,
            ignore_ownership: p.ignore_ownership,
            only_host_land: p.only_city_state_territory,
            requires_improvement: p.requires_improvement,
            prompt_when_complete: p.prompt_when_complete,
            destroyed_when_pillaged: p.destroyed_when_pillaged,
        }
    }
}

/// Building placement (see `classic_rules::rules::content::SourceBuildingPlacement`).
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RonBuildingPlacement {
    #[serde(default)]
    water: bool,
    #[serde(default)]
    river: bool,
    #[serde(default)]
    mountain: bool,
    #[serde(default)]
    nearby_mountain: bool,
    #[serde(default)]
    fresh_water: bool,
    #[serde(default)]
    flat: bool,
    #[serde(default)]
    nearby_terrain: Option<String>,
    #[serde(default)]
    prohibited_city_terrain: Option<String>,
    #[serde(default)]
    local_resources_any: Vec<String>,
    #[serde(default)]
    local_resources_all: Vec<String>,
    #[serde(default)]
    exclusive_group: Option<u8>,
    #[serde(default)]
    capital: bool,
    #[serde(default)]
    allows_water_routes: bool,
}

impl From<RonBuildingPlacement> for classic_rules::rules::content::SourceBuildingPlacement {
    fn from(p: RonBuildingPlacement) -> Self {
        let key = |s: String| classic_rules::rules::source::StableContentKey(s);
        Self {
            water: p.water,
            river: p.river,
            mountain: p.mountain,
            nearby_mountain: p.nearby_mountain,
            fresh_water: p.fresh_water,
            flat: p.flat,
            nearby_terrain: p.nearby_terrain.map(key),
            prohibited_city_terrain: p.prohibited_city_terrain.map(key),
            local_resources_any: p.local_resources_any.into_iter().map(key).collect(),
            local_resources_all: p.local_resources_all.into_iter().map(key).collect(),
            exclusive_group: p.exclusive_group,
            capital: p.capital,
            allows_water_routes: p.allows_water_routes,
        }
    }
}

/// Feature removal build (see `classic_rules::rules::content::SourceFeatureRemoval`).
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RonFeatureRemoval {
    #[serde(default)]
    tech: Option<String>,
    work_time: u32,
    #[serde(default)]
    production: u32,
}

impl From<RonFeatureRemoval> for SourceFeatureRemoval {
    fn from(r: RonFeatureRemoval) -> Self {
        SourceFeatureRemoval {
            tech: opt_key(r.tech),
            work_time: r.work_time,
            production: r.production,
        }
    }
}

mirror_enum!(RonCargoKind => CargoKind { Aircraft, Missile });

/// Combat abilities of a unit (`classic_rules::rules::content::SourceUnitCombat`).
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RonUnitCombat {
    #[serde(default)]
    capture: Option<String>,
    #[serde(default)]
    ignore_building_defense: bool,
    #[serde(default)]
    immobile: bool,
    #[serde(default)]
    air_intercept_range: u8,
    #[serde(default)]
    suicide: bool,
    #[serde(default)]
    nuke_level: u8,
    #[serde(default)]
    ranged_only_in_domain: bool,
    #[serde(default)]
    carries: Option<RonCargoKind>,
    #[serde(default)]
    carried_as: Option<RonCargoKind>,
    #[serde(default)]
    pillage: bool,
}

impl From<RonUnitCombat> for SourceUnitCombat {
    fn from(c: RonUnitCombat) -> Self {
        SourceUnitCombat {
            capture: c.capture.map(StableContentKey::new),
            ignore_building_defense: c.ignore_building_defense,
            immobile: c.immobile,
            air_intercept_range: c.air_intercept_range,
            suicide: c.suicide,
            nuke_level: c.nuke_level,
            ranged_only_in_domain: c.ranged_only_in_domain,
            carries: c.carries.map(Into::into),
            carried_as: c.carried_as.map(Into::into),
            pillage: c.pillage,
        }
    }
}
