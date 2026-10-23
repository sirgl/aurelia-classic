//! The classic words of the effect vocabulary (`rules::vocabulary`): what
//! the module adds to the kernel's families, the classic-only enums its words
//! carry (great people, city-states, religion, tourism) and the module's
//! indexes over its words. Every word keeps the tag it had in the closed
//! vocabulary (see the classification in `rules::vocabulary`).

use crate::classic::Classic;
use std::hash::{Hash, Hasher};

use crate::ids::{BeliefId, BuildingKindId, PolicyId};
use crate::rules::source::StableContentKey;
use crate::rules::content::EraId;
use crate::classic::compile_effects::{self as compile, per_count};
use crate::rules::compile::RulesError;
use crate::rules::compile_effects::{all_effect_sets, SourceKind};
use crate::rules::key_index::KeyIndex;
use crate::rules::effects::{
    CompiledEffect, Condition, Count, Modifier, PerCount, Scope, Selector, YieldKind,
};
use crate::rules::CompiledRuleset;
use crate::rules::vocabulary::{
    hash_tag, ConditionWord, CostWord, CountWord, GrantWord, ModifierWord, SelectorWord,
    SourceWord, TableWord, TriggerWord, Vocabulary, Word, YieldWord,
};

/// Great person kinds: the kernel's `rules::stats` still names them (its
/// closed stat vocabulary), so the enum lives there until stats split.
pub use crate::classic::content::{GreatPersonKind, GREAT_PERSON_KINDS};

/// What a policy, a belief or a trait changes in a civilization's dealings
/// with city-states (classic/docs/classic/rules.md). Read by
/// `crate::systems::city_states`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(test, derive(serde::Deserialize))]
pub enum CityStateStat {
    /// Resting point of the influence (Consulates).
    RestingPoint,
    /// Percent of the influence decay (Patronage).
    DecayPercent,
    /// Percent of the influence of gold gifts.
    GoldGiftPercent,
    /// Percent of the military part of the bully score (Gunboat Diplomacy).
    BullyScorePercent,
    /// Influence per military unit gifted (Arsenal of Democracy).
    UnitGiftInfluence,
    /// Percent of the food, culture and faith of friends and allies
    /// (Siam).
    FriendBonusPercent,
    /// Slower decay (half) and faster recovery (full) of the influence
    /// (Greece).
    FriendshipRate,
    /// May buy out an allied city-state (Austria).
    AnnexCityStates,
    /// Resting point of the influence with the city-states whose capital
    /// follows the religion the player founded (Papal Primacy; rules.md).
    RestingPointSameReligion,
    /// Influence per turn with each city-state that is afraid of the player
    /// (Gunboat Diplomacy; §4.4).
    AfraidInfluence,
    /// Influence per turn with each city-state a trade route connects the
    /// player with (Treaty Organization).
    TradeRouteInfluence,
    /// Percent faster militaristic unit gifts while the player and the
    /// city-state share an enemy (United Front; §4.8).
    UnitGiftRatePercent,
    /// Flag: allied city-states give twice their resources and their
    /// luxuries 50 % more happiness (Cultural Diplomacy; §4.7).
    AllyResourceBonus,
    /// Flag: allied city-states give great people now and then
    /// (Patronage finisher; §4.7).
    AllyGreatPeople,
    /// Flag: the player gets a quarter of its allied city-states' science
    /// (Scholasticism; §4.7).
    AllyScience,
    /// Influence of a great person gifted to a city-state (the unit is used
    /// up; Sweden; §4.11).
    GreatPersonGiftInfluence,
    /// Percent of the gold of a great merchant's trade mission
    /// (Entrepreneurship; §4.11).
    TradeMissionGoldPercent,
}

pub const CITY_STATE_STATS: usize = 17;

impl CityStateStat {
    /// Position in declaration order (`CITY_STATE_STATS` slots).
    pub fn index(self) -> usize {
        self as usize
    }
}

/// A number of a founded religion that beliefs change (percent).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(test, derive(serde::Deserialize))]
pub enum ReligionStat {
    /// Pressure its cities exert on their neighbours (`religion.pressure_out`).
    PressureOut,
    /// Distance its pressure reaches (`religion.spread_distance`).
    SpreadDistance,
    /// Strength of its missionaries (`missionary.strength`).
    MissionaryStrength,
    /// Strength of its prophets (`prophet.strength`).
    ProphetStrength,
    /// Faith of its founder's next prophet (`prophet.faith_threshold`).
    ProphetCost,
    /// Faith price of its missionaries and inquisitors
    /// (`faith_purchase.missionary_inquisitor.cost`).
    MissionaryCost,
    /// Pressure on the cities of city-states that are friends or allies of
    /// its founder (Religious Unity).
    FriendlyCityStatePressure,
    /// Percent of a missionary's pressure taken from the other religions of
    /// the city, in proportion to their pressure
    /// (Evangelism).
    OtherReligionErosion,
    /// Percent of its pressure a city keeps when an inquisitor or a prophet
    /// of another religion removes it (Unity of the Prophets).
    PressureRetention,
}

pub const RELIGION_STATS: usize = 9;

impl ReligionStat {
    pub fn index(self) -> usize {
        match self {
            ReligionStat::PressureOut => 0,
            ReligionStat::SpreadDistance => 1,
            ReligionStat::MissionaryStrength => 2,
            ReligionStat::ProphetStrength => 3,
            ReligionStat::ProphetCost => 4,
            ReligionStat::MissionaryCost => 5,
            ReligionStat::FriendlyCityStatePressure => 6,
            ReligionStat::OtherReligionErosion => 7,
            ReligionStat::PressureRetention => 8,
        }
    }
}

/// A rule of a founded religion that a belief switches on (Reformation).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(test, derive(serde::Deserialize))]
pub enum ReligionAbility {
    /// Its missionaries convert the barbarian land units next to the city
    /// they spread into (Heathen Conversion).
    ConvertsBarbarians,
    /// Its founder may buy every kind of great person with faith from the
    /// Industrial era without the policy branches
    /// (To the Glory of God).
    FaithPurchaseAllGreatPeople,
}

pub const RELIGION_ABILITIES: usize = 2;

impl ReligionAbility {
    pub fn index(self) -> usize {
        match self {
            ReligionAbility::ConvertsBarbarians => 0,
            ReligionAbility::FaithPurchaseAllGreatPeople => 1,
        }
    }
}

/// A tourism number that policies, buildings, wonders, beliefs and
/// technologies change (classic/docs/classic/rules.md).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(test, derive(serde::Deserialize))]
pub enum TourismStat {
    /// City: flat tourism (Eiffel Tower, Mughal Fort, Religious Art).
    Flat,
    /// City: percent of the city's tourism (Media Culture with a Broadcast
    /// Tower).
    Percent,
    /// City: percent of the tourism of its great works (Hotel, Airport,
    /// National Visitor Center).
    GreatWorksPercent,
    /// City: percent of the culture of its wonders, natural wonders and
    /// improvements that becomes tourism.
    LandmarksPercent,
    /// City: tourism per faith-only building (Sacred Sites).
    PerFaithBuilding,
    /// Player: percent of the tourism of all cities that a Great Firewall
    /// nullifies (the Internet).
    Spread,
    /// Player: percent of the theming bonuses (Aesthetics finisher); City (the
    /// capital): France's, applied after the player's.
    ThemingPercent,
    /// Player: culture per great work (Creative Expression).
    GreatWorkCulture,
}

impl TourismStat {
    /// The scopes the stat belongs to.
    pub fn fits(self, scope: Scope) -> bool {
        match self {
            TourismStat::Spread | TourismStat::GreatWorkCulture => scope == Scope::Player,
            TourismStat::ThemingPercent => matches!(scope, Scope::Player | Scope::City),
            _ => scope == Scope::City,
        }
    }
}

/// When a tourism percent toward another civilization applies
/// (rules.md, tourism multiplier).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(test, derive(serde::Deserialize))]
pub enum TourismCondition {
    /// Our majority religion is the target's majority religion.
    SharedReligion,
    /// A trade route connects us.
    TradeRoute,
    /// The target opened its borders to us.
    OpenBorders,
    /// We follow the same ideology.
    SameIdeology,
    /// We are happier than the target.
    LessHappy,
    /// Per third civilization both of us are at war with.
    CommonEnemy,
}

pub const TOURISM_CONDITIONS: usize = 6;

impl TourismCondition {
    pub fn index(self) -> usize {
        match self {
            TourismCondition::SharedReligion => 0,
            TourismCondition::TradeRoute => 1,
            TourismCondition::OpenBorders => 2,
            TourismCondition::SameIdeology => 3,
            TourismCondition::LessHappy => 4,
            TourismCondition::CommonEnemy => 5,
        }
    }
}

/// Whose city a belief condition looks at, seen from the unit's owner.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(test, derive(serde::Deserialize))]
pub enum CityRelation {
    Own,
    /// A city of a player at war with the unit's owner.
    Enemy,
}

/// The classic words of `Scope` (tags in `rules::vocabulary`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(test, derive(serde::Deserialize))]
pub enum ClassicScope {
    /// A founded religion (its missionaries, pressure, prophets).
    Religion,
}


/// The classic words of `Selector` (tags in `rules::vocabulary`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(test, derive(serde::Deserialize))]
pub enum ClassicSelector {
    /// City scope, beliefs: the cities whose majority follows the belief
    /// (its religion, or the owner's pantheon).
    FollowingCities,
    /// Tile scope, beliefs: tiles worked by the following cities.
    FollowingCityTiles,
    /// City scope, beliefs: the holy city of the belief's religion.
    HolyCity,
    /// Player scope, beliefs: the founder of the belief's religion.
    Founder,
    /// Religion scope, beliefs: the belief's religion itself.
    OwnReligion,
}


/// The classic words of `Trigger` (tags in `rules::vocabulary`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(test, derive(serde::Deserialize))]
pub enum ClassicTrigger {
    /// The owner adopts the source policy: openers when
    /// their branch is unlocked, finishers when it is completed, tenets.
    OnAdopt,
    /// The owner is the first major civilization to find a natural wonder
    /// (Reward: gold).
    OnNaturalWonderFoundFirst,
    /// The owner finds a natural wonder another civilization found before
    /// (Reward: gold).
    OnNaturalWonderFound,
    /// Start of the owner's turn when the game year crosses the end of a
    /// Maya Long Count cycle (a baktun, `world::calendar`).
    OnLongCountCycle,
    /// A great person of the owner is used up by its action (a great work, a
    /// bulb, a hurry, a golden age, an improvement, a citadel, a concert, a
    /// prophet's founding, enhancing or last spread). Belief sources: the
    /// beliefs of the religion the owner founded (Reliquary). That a prophet
    /// who founds or enhances a religion counts is an own rule, unverified.
    OnGreatPersonExpended,
    /// A city first converts to the source belief's religion: its majority
    /// becomes the religion, once per city.
    /// Fires for the religion's founder; belief sources only (Initiation
    /// Rites).
    OnCityConverted,
    /// A missionary or a prophet spreads the source belief's religion into
    /// a city, before its pressure lands. Fires
    /// for the unit's owner; belief sources only (Interfaith Dialogue).
    OnReligionSpread,
    /// A great person unit of the owner appears, however it came (birth,
    /// faith, a gift); the unit is the event unit (`Condition::GreatPersonIs`
    /// reads it).
    OnGreatPersonBorn,
}


/// The classic words of `Condition` (tags in `rules::vocabulary`). Content
/// writes them as they are (they hold no references).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(test, derive(serde::Deserialize))]
pub enum ClassicCondition {
    /// Unit scope, combat: the opponent is a barbarian.
    OpponentIsBarbarian,
    /// Unit scope, combat: the opponent (unit or city) is a city-state's
    /// (Mongolia).
    OpponentIsCityState,
    /// Unit scope, combat: the opponent's owner has more cities than the
    /// unit's (Ethiopia).
    OpponentHasMoreCities,
    /// City or Tile scope: the city has at least this many followers of its
    /// majority religion.
    MinFollowers(u32),
    /// Unit scope, situational: the city whose territory holds the battle
    /// tile (combat) or the unit's tile (healing) is the unit owner's own
    /// (or an enemy's, at war) and its majority follows the owner's creed —
    /// the religion it founded, or (own city) its pantheon. Combat bonuses in
    /// friendly and enemy cities and healing in friendly cities, of beliefs
    /// (rules.md).
    NearFollowingCity(CityRelation),
    /// Any scope: the owner is in a golden age.
    GoldenAge,
    /// Player scope, `OnGreatPersonBorn` only: the great person that
    /// appeared is of this kind (tourism on a great person's appearance).
    GreatPersonIs(GreatPersonKind),
    /// The owner has earned a great general (Carthage).
    GreatGeneralEarned,
}


/// The classic words of `Modifier` (tags in `rules::vocabulary`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClassicModifier {
    AddHappiness {
        amount: i32,
    },
    AddGreatPersonPoints {
        kind: GreatPersonKind,
        amount: i32,
    },
    /// Flat change of a city-state number of the target player (Player).
    ModifyCityState {
        stat: CityStateStat,
        amount: i32,
    },
    /// Instant: the target player may adopt `count` policies for free
    /// (the `grant.policy` effect).
    FreePolicies { count: u8 },
    /// Flat yield per city following the source belief's religion (Church
    /// Property; Pilgrimage with `foreign_only`).
    AddYieldPerFollowingCity {
        yield_kind: YieldKind,
        amount: i32,
        foreign_only: bool,
    },
    /// Flat yield per `per` followers of the source belief's religion (Tithe;
    /// World Church with `foreign_only`).
    AddYieldPerFollowers {
        yield_kind: YieldKind,
        per: u16,
        foreign_only: bool,
    },
    /// Happiness per city following the religion, in hundredths (Ceremonial
    /// Burial: 50 = +1 per 2 cities).
    AddHappinessPerFollowingCity { hundredths: i32 },
    /// Additive percent of a number of the religion.
    ModifyReligion { stat: ReligionStat, percent: i32 },
    /// The following city may buy this building with faith (Cathedrals ...).
    UnlockFaithBuilding(BuildingKindId),
    /// The following city may buy land units of this era with faith (Holy
    /// Warriors, Religious Fervor).
    UnlockFaithUnits(EraId),
    /// The founder of a religion may add a Reformation belief (Piety's
    /// Reformation policy).
    AllowReformationBelief,
    /// A tourism number (City or Player scope by `stat`).
    ModifyTourism { stat: TourismStat, amount: i32 },
    /// Percent of the player's tourism toward a civilization when `when`
    /// holds for that civilization (Player scope).
    TourismToward { when: TourismCondition, percent: i32 },
    /// Happiness per counted thing (Aristocracy: +1 per 10 citizens of a
    /// city; Prora: +1 per 2 policies). Becomes `AddHappiness`.
    AddHappinessPerCount { amount: i32, per: PerCount },
    /// Instant: the target player gains `count` spies at its starting rank
    /// (on completion: National Intelligence Agency).
    GrantSpies { count: u8 },
    /// Instant: every living spy of the target player rises `ranks` ranks,
    /// at most Special Agent (National Intelligence Agency).
    PromoteSpies { ranks: u8 },
    /// A rule of the religion (Religion scope; Reformation beliefs).
    GrantReligionAbility(ReligionAbility),
    /// The player chooses one more belief when it founds its religion: a
    /// pantheon, founder, follower or enhancer belief (Byzantium). Player scope.
    ExtraFoundingBelief,
    /// The player's cities with a majority religion also get the pantheon
    /// belief of their second religion (Piety's Religious Tolerance). Player
    /// scope.
    SecondReligionPantheon,
    /// Instant, Player scope: the target player's cultural influence on
    /// every other living civilization it met grows by `amount` (Futurism;
    /// rules "Culture" §1.5).
    InstantTourism { amount: u32 },
    /// Instant, Unit scope (a unit just trained): experience for each Great
    /// Work in the city that trained it (Royal Library).
    GrantExperiencePerGreatWork(u16),
}


/// `ClassicModifier` as content writes it: references by key (the compiler
/// resolves them, `classic::compile_effects`).
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(test, derive(serde::Deserialize))]
pub enum ClassicSourceModifier {
    AddHappiness {
        amount: i32,
    },
    AddGreatPersonPoints {
        kind: GreatPersonKind,
        amount: i32,
    },
    ModifyCityState {
        stat: CityStateStat,
        amount: i32,
    },
    FreePolicies {
        count: u8,
    },
    AddYieldPerFollowingCity {
        yield_kind: YieldKind,
        amount: i32,
        foreign_only: bool,
    },
    AddYieldPerFollowers {
        yield_kind: YieldKind,
        per: u16,
        foreign_only: bool,
    },
    AddHappinessPerFollowingCity {
        hundredths: i32,
    },
    ModifyReligion {
        stat: ReligionStat,
        percent: i32,
    },
    UnlockFaithBuilding(StableContentKey),
    UnlockFaithUnits(StableContentKey),
    AllowReformationBelief,
    ModifyTourism {
        stat: TourismStat,
        amount: i32,
    },
    TourismToward {
        when: TourismCondition,
        percent: i32,
    },
    AddHappinessPerCount {
        amount: i32,
        per: PerCount,
    },
    GrantSpies {
        count: u8,
    },
    PromoteSpies {
        ranks: u8,
    },
    GrantReligionAbility(ReligionAbility),
    ExtraFoundingBelief,
    SecondReligionPantheon,
    InstantTourism {
        amount: u32,
    },
    GrantExperiencePerGreatWork(u16),
}

/// The classic words of `UnitStat` (tags in `rules::vocabulary`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(test, derive(serde::Deserialize))]
pub enum ClassicUnitStat {
    /// Great general points from this unit's combat, percent.
    GreatGeneralPointsPercent,
    /// Extra combat percent for the unit a great general stands with
    /// (read from the general).
    StackedGeneralPercent,
    /// Percent added to the gold maintenance of the unit (Germany's land
    /// units, the Ottomans' ships, the Zulu's melee, Nationalism).
    MaintenancePercent,
    /// Percent added to the experience the unit needs for its next
    /// promotion (the Zulu).
    ExperienceNeededPercent,
    /// Percent added to the combat bonus of the owner's great generals and
    /// admirals (China; read from the owner).
    GeneralAuraPercent,
    /// Percent added to the gold of the unit's trade mission
    /// (the Merchant of Venice).
    TradeMissionGoldPercent,
    /// Percent added to the influence of the unit's trade mission.
    TradeMissionInfluencePercent,
    /// Chance in percent that the adjacent-damage stat applies after the
    /// unit attacks; without it the damage always applies.
    AdjacentDamageChance,
    /// Percent of a killed unit's strength the owner earns as golden age
    /// points.
    GoldenAgePointsOnKillPercent,
    /// Percent of its original religious strength the unit loses for each
    /// turn it ends in rival land (the Unwelcome Evangelist of the
    /// missionary).
    RivalLandStrengthLossPercent,
}


/// The classic words of `UnitAbility` (tags in `rules::vocabulary`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(test, derive(serde::Deserialize))]
pub enum ClassicUnitAbility {
    /// The unit is a great general: aura for own land units.
    GreatGeneral,
    /// The unit is a great admiral: aura for own naval units.
    GreatAdmiral,
    /// Healing on a kill does not apply against barbarians.
    HealOnKillNotBarbarians,
    /// The promotion that carries it is replaced by a random one of its
    /// candidates after the unit's first combat (Mystic Blade).
    PostCombatPromotions,
    /// The unit carries a cargo it sells once next to a foreign land
    /// («Exotic Cargo»: Portugal's Nau).
    ExoticCargo,
    /// A Great General stacked with the unit at the start of the turn moves
    /// at the unit's rate (the Hakkapeliitta).
    LendsMovementToGeneral,
    /// The unit takes the best of the offers of Ancient Ruins it enters
    /// («Native Tongue»: the Shoshone Pathfinder).
    ChoosesRuinsReward,
}


/// The classic words of `YieldKind` (tags in `rules::vocabulary`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(test, derive(serde::Deserialize))]
pub enum ClassicYield {
    Faith,
}


/// The classic words of `CostTarget` (tags in `rules::vocabulary`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(test, derive(serde::Deserialize))]
pub enum ClassicCost {
    /// Culture cost of the next social policy of the target player
    /// (`policy.cost`; never below -75 %).
    Policies,
    /// Percent of the per-city policy cost penalty of the target player
    /// (`policy.cost_per_city`).
    PolicyCityPenalty,
    /// Faith price of the target player's faith purchases
    /// (`faith_purchase.cost`, Mandate of Heaven).
    FaithPurchase,
}


/// The classic words of `Count` (tags in `rules::vocabulary`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(test, derive(serde::Deserialize))]
pub enum ClassicCount {
    /// Player: the policies it adopted, openers and finishers included
    /// (Prora: +1 happiness per 2 policies).
    Policies,
    /// City: the followers of its majority creed (Religious Community).
    MajorityFollowers,
    /// City: the founded religions with at least one follower in it (Candi).
    ReligionsPresent,
    /// Player: the followers of the religion it founded in the cities of
    /// other players it is not at war with (Peace Loving).
    PeacefulForeignFollowers,
    /// Player, read on the city of an `OnReligionSpread` event: the
    /// followers of the religions other than the spreading one, when the
    /// city's majority is another creed (Interfaith Dialogue).
    OtherReligionFollowers,
    /// City or Player: the other civilizations and city-states a trade route
    /// connects the (city's) owner with, either way (Morocco).
    TradePartners,
    /// City or Player: the (city's) owner's trade routes to city-states
    /// (Hanse).
    CityStateRoutes,
}


/// The classic words of `Grant` (tags in `rules::vocabulary`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(test, derive(serde::Deserialize))]
pub enum ClassicGrant {
    /// Player: great people of the player's choice (buildings and policies).
    FreeGreatPeople(u8),
    /// Player: one great person of a kind the Long Count has not given yet
    /// (Maya Long Count; all given: any kind again).
    LongCountGreatPerson,
    /// Player: a golden age of `turns`, lengthened by the player's
    /// `GoldenAgeLengthPercent` (buildings with the standard length, policies
    /// with their own).
    GoldenAge { turns: u16 },
}


/// The classic words of `SourceRef` (tags in `rules::vocabulary`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClassicSource {
    Policy(PolicyId),
    Belief(BeliefId),
}


/// `Hash` of a fieldless word: its tag, as the closed enum hashed it.
macro_rules! hash_as_tag {
    ($($word:ty),*) => {$(
        impl Hash for $word {
            fn hash<H: Hasher>(&self, state: &mut H) {
                hash_tag(self.tag(), state);
            }
        }
    )*};
}

hash_as_tag!(
    ClassicScope, ClassicSelector, ClassicTrigger, ClassicUnitStat, ClassicUnitAbility, ClassicYield, ClassicCost,
    ClassicCount
);

impl Word for ClassicScope {
    fn tag(self) -> u8 {
        4
    }
}

impl Word for ClassicSelector {
    fn tag(self) -> u8 {
        match self {
            ClassicSelector::FollowingCities => 10,
            ClassicSelector::FollowingCityTiles => 11,
            ClassicSelector::HolyCity => 12,
            ClassicSelector::Founder => 13,
            ClassicSelector::OwnReligion => 14,
        }
    }
}

impl SelectorWord<Classic> for ClassicSelector {
    fn scope(self) -> Scope {
        match self {
            ClassicSelector::FollowingCities | ClassicSelector::HolyCity => Scope::City,
            ClassicSelector::FollowingCityTiles => Scope::Tile,
            ClassicSelector::Founder => Scope::Player,
            ClassicSelector::OwnReligion => Scope::Module(ClassicScope::Religion),
        }
    }
}

impl Word for ClassicTrigger {
    fn tag(self) -> u8 {
        match self {
            ClassicTrigger::OnAdopt => 7,
            ClassicTrigger::OnNaturalWonderFoundFirst => 8,
            ClassicTrigger::OnNaturalWonderFound => 9,
            ClassicTrigger::OnLongCountCycle => 12,
            ClassicTrigger::OnGreatPersonExpended => 13,
            ClassicTrigger::OnCityConverted => 14,
            ClassicTrigger::OnReligionSpread => 15,
            ClassicTrigger::OnGreatPersonBorn => 16,
        }
    }
}

impl TriggerWord for ClassicTrigger {
    fn has_event_unit(self) -> bool {
        self == ClassicTrigger::OnGreatPersonBorn
    }
}

impl Word for ClassicCondition {
    fn tag(self) -> u8 {
        match self {
            ClassicCondition::OpponentIsBarbarian => 13,
            ClassicCondition::OpponentIsCityState => 32,
            ClassicCondition::OpponentHasMoreCities => 33,
            ClassicCondition::MinFollowers(_) => 16,
            ClassicCondition::NearFollowingCity(_) => 18,
            ClassicCondition::GoldenAge => 24,
            ClassicCondition::GreatPersonIs(_) => 31,
            ClassicCondition::GreatGeneralEarned => 35,
        }
    }
}

impl Hash for ClassicCondition {
    fn hash<H: Hasher>(&self, state: &mut H) {
        hash_tag(self.tag(), state);
        match self {
            ClassicCondition::MinFollowers(n) => n.hash(state),
            ClassicCondition::NearFollowingCity(relation) => relation.hash(state),
            ClassicCondition::GreatPersonIs(kind) => kind.hash(state),
            ClassicCondition::OpponentIsBarbarian
            | ClassicCondition::OpponentIsCityState
            | ClassicCondition::OpponentHasMoreCities
            | ClassicCondition::GreatGeneralEarned
            | ClassicCondition::GoldenAge => {}
        }
    }
}

impl ConditionWord for ClassicCondition {
    fn is_opponent(self) -> bool {
        matches!(
            self,
            ClassicCondition::OpponentIsBarbarian | ClassicCondition::OpponentIsCityState | ClassicCondition::OpponentHasMoreCities
        )
    }

    fn is_situational(self) -> bool {
        self.is_opponent() || matches!(self, ClassicCondition::NearFollowingCity(_))
    }

    fn is_territory(self) -> bool {
        matches!(self, ClassicCondition::NearFollowingCity(_))
    }
}

impl Word for ClassicModifier {
    fn tag(self) -> u8 {
        match self {
            ClassicModifier::AddHappiness { .. } => 8,
            ClassicModifier::AddGreatPersonPoints { .. } => 9,
            ClassicModifier::ModifyCityState { .. } => 22,
            ClassicModifier::FreePolicies { .. } => 23,
            ClassicModifier::AddYieldPerFollowingCity { .. } => 25,
            ClassicModifier::AddYieldPerFollowers { .. } => 26,
            ClassicModifier::AddHappinessPerFollowingCity { .. } => 27,
            ClassicModifier::ModifyReligion { .. } => 28,
            ClassicModifier::UnlockFaithBuilding(_) => 29,
            ClassicModifier::UnlockFaithUnits(_) => 30,
            ClassicModifier::AllowReformationBelief => 31,
            ClassicModifier::ModifyTourism { .. } => 32,
            ClassicModifier::TourismToward { .. } => 33,
            ClassicModifier::AddHappinessPerCount { .. } => 36,
            ClassicModifier::GrantSpies { .. } => 40,
            ClassicModifier::PromoteSpies { .. } => 41,
            ClassicModifier::GrantReligionAbility(_) => 42,
            ClassicModifier::ExtraFoundingBelief => 43,
            ClassicModifier::SecondReligionPantheon => 44,
            ClassicModifier::InstantTourism { .. } => 45,
            ClassicModifier::GrantExperiencePerGreatWork(_) => 46,
        }
    }
}

impl Hash for ClassicModifier {
    fn hash<H: Hasher>(&self, state: &mut H) {
        hash_tag(self.tag(), state);
        match *self {
            ClassicModifier::AddHappiness { amount } => amount.hash(state),
            ClassicModifier::AddGreatPersonPoints { kind, amount } => (kind, amount).hash(state),
            ClassicModifier::ModifyCityState { stat, amount } => (stat, amount).hash(state),
            ClassicModifier::FreePolicies { count } | ClassicModifier::GrantSpies { count } => {
                count.hash(state)
            }
            ClassicModifier::AddYieldPerFollowingCity { yield_kind, amount, foreign_only } => {
                (yield_kind, amount, foreign_only).hash(state)
            }
            ClassicModifier::AddYieldPerFollowers { yield_kind, per, foreign_only } => {
                (yield_kind, per, foreign_only).hash(state)
            }
            ClassicModifier::AddHappinessPerFollowingCity { hundredths } => hundredths.hash(state),
            ClassicModifier::ModifyReligion { stat, percent } => (stat, percent).hash(state),
            ClassicModifier::UnlockFaithBuilding(building) => building.hash(state),
            ClassicModifier::UnlockFaithUnits(era) => era.hash(state),
            ClassicModifier::ModifyTourism { stat, amount } => (stat, amount).hash(state),
            ClassicModifier::TourismToward { when, percent } => (when, percent).hash(state),
            ClassicModifier::AddHappinessPerCount { amount, per } => (amount, per).hash(state),
            ClassicModifier::PromoteSpies { ranks } => ranks.hash(state),
            ClassicModifier::GrantReligionAbility(ability) => ability.hash(state),
            ClassicModifier::InstantTourism { amount } => amount.hash(state),
            ClassicModifier::GrantExperiencePerGreatWork(amount) => amount.hash(state),
            ClassicModifier::AllowReformationBelief
            | ClassicModifier::ExtraFoundingBelief
            | ClassicModifier::SecondReligionPantheon => {}
        }
    }
}

impl ModifierWord for ClassicModifier {
    fn is_instant(self) -> bool {
        matches!(
            self,
            ClassicModifier::FreePolicies { .. }
                | ClassicModifier::GrantSpies { .. }
                | ClassicModifier::PromoteSpies { .. }
                | ClassicModifier::InstantTourism { .. }
                | ClassicModifier::GrantExperiencePerGreatWork(_)
        )
    }
}

impl Word for ClassicUnitStat {
    fn tag(self) -> u8 {
        match self {
            ClassicUnitStat::GreatGeneralPointsPercent => 22,
            ClassicUnitStat::StackedGeneralPercent => 23,
            ClassicUnitStat::MaintenancePercent => 27,
            ClassicUnitStat::ExperienceNeededPercent => 28,
            ClassicUnitStat::GeneralAuraPercent => 29,
            ClassicUnitStat::TradeMissionGoldPercent => 30,
            ClassicUnitStat::TradeMissionInfluencePercent => 31,
            ClassicUnitStat::AdjacentDamageChance => 32,
            ClassicUnitStat::GoldenAgePointsOnKillPercent => 33,
            ClassicUnitStat::RivalLandStrengthLossPercent => 34,
        }
    }
}

impl Word for ClassicUnitAbility {
    fn tag(self) -> u8 {
        match self {
            ClassicUnitAbility::GreatGeneral => 6,
            ClassicUnitAbility::GreatAdmiral => 7,
            ClassicUnitAbility::HealOnKillNotBarbarians => 8,
            ClassicUnitAbility::PostCombatPromotions => 36,
            ClassicUnitAbility::ExoticCargo => 37,
            ClassicUnitAbility::LendsMovementToGeneral => 41,
            ClassicUnitAbility::ChoosesRuinsReward => 42,
        }
    }
}

impl Word for ClassicYield {
    fn tag(self) -> u8 {
        6
    }
}

impl YieldWord for ClassicYield {
    /// The faith pool (`world::religion`).
    fn has_player_stock(self) -> bool {
        true
    }

    fn is_player_yield(self) -> bool {
        true
    }
}

impl Word for ClassicCost {
    fn tag(self) -> u8 {
        match self {
            ClassicCost::Policies => 7,
            ClassicCost::PolicyCityPenalty => 8,
            ClassicCost::FaithPurchase => 9,
        }
    }
}

impl CostWord for ClassicCost {
    fn is_player_cost(self) -> bool {
        true
    }
}

impl Word for ClassicCount {
    fn tag(self) -> u8 {
        match self {
            ClassicCount::Policies => 1,
            ClassicCount::MajorityFollowers => 4,
            ClassicCount::ReligionsPresent => 5,
            ClassicCount::PeacefulForeignFollowers => 6,
            ClassicCount::OtherReligionFollowers => 7,
            ClassicCount::TradePartners => 8,
            ClassicCount::CityStateRoutes => 9,
        }
    }
}

impl CountWord<Classic> for ClassicCount {
    fn fits(self, scope: Scope) -> bool {
        match self {
            ClassicCount::MajorityFollowers | ClassicCount::ReligionsPresent => scope == Scope::City,
            ClassicCount::Policies
            | ClassicCount::PeacefulForeignFollowers
            | ClassicCount::OtherReligionFollowers => scope == Scope::Player,
            ClassicCount::TradePartners | ClassicCount::CityStateRoutes => {
                matches!(scope, Scope::City | Scope::Player)
            }
        }
    }
}

impl ClassicCount {
    /// The count is read on the city of an `OnReligionSpread` event, not on
    /// the target (the compiler allows it only with that trigger).
    pub fn needs_spread_event(self) -> bool {
        self == ClassicCount::OtherReligionFollowers
    }

    /// The count changes when a trade route starts or ends.
    pub fn follows_trade_routes(self) -> bool {
        matches!(self, ClassicCount::TradePartners | ClassicCount::CityStateRoutes)
    }
}

impl Word for ClassicGrant {
    fn tag(self) -> u8 {
        match self {
            ClassicGrant::FreeGreatPeople(_) => 1,
            ClassicGrant::LongCountGreatPerson => 2,
            ClassicGrant::GoldenAge { .. } => 4,
        }
    }
}

impl Hash for ClassicGrant {
    fn hash<H: Hasher>(&self, state: &mut H) {
        hash_tag(self.tag(), state);
        match self {
            ClassicGrant::FreeGreatPeople(n) => n.hash(state),
            ClassicGrant::GoldenAge { turns } => turns.hash(state),
            ClassicGrant::LongCountGreatPerson => {}
        }
    }
}

impl GrantWord<Classic> for ClassicGrant {
    fn scope(self) -> Scope {
        Scope::Player
    }
}

/// The classic tables whose rows carry effects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClassicTable {
    Policy,
    Belief,
}

impl TableWord for ClassicTable {
    /// A belief's city: the following city nearest to the event.
    fn has_source_city(self) -> bool {
        self == ClassicTable::Belief
    }
}

impl Word for ClassicSource {
    fn tag(self) -> u8 {
        match self {
            ClassicSource::Policy(_) => 1,
            ClassicSource::Belief(_) => 2,
        }
    }
}

impl SourceWord for ClassicSource {
    fn row(self) -> u16 {
        match self {
            ClassicSource::Policy(id) => id.0,
            ClassicSource::Belief(id) => id.0,
        }
    }

    fn from_parts(tag: u8, row: u16) -> Option<Self> {
        match tag {
            1 => Some(ClassicSource::Policy(PolicyId(row))),
            2 => Some(ClassicSource::Belief(BeliefId(row))),
            _ => None,
        }
    }
}

impl Hash for ClassicSource {
    fn hash<H: Hasher>(&self, state: &mut H) {
        hash_tag(self.tag(), state);
        match self {
            ClassicSource::Policy(id) => id.hash(state),
            ClassicSource::Belief(id) => id.hash(state),
        }
    }
}

/// The classic words of the effect vocabulary.
#[derive(Debug, Clone, Copy)]
pub struct ClassicVocabulary;

impl Vocabulary<Classic> for ClassicVocabulary {
    type Scope = ClassicScope;
    type Selector = ClassicSelector;
    type Trigger = ClassicTrigger;
    type Condition = ClassicCondition;
    type Modifier = ClassicModifier;
    type UnitStat = ClassicUnitStat;
    type UnitAbility = ClassicUnitAbility;
    type Stat = crate::classic::stats::ClassicStat;
    type Yield = ClassicYield;
    type Cost = ClassicCost;
    type Count = ClassicCount;
    type Grant = ClassicGrant;
    type Table = ClassicTable;
    type Source = ClassicSource;
    type SourceModifier = ClassicSourceModifier;
    type SourceCondition = ClassicCondition;
    type Index = ClassicIndex;

    const TRIGGERS: &'static [ClassicTrigger] = &[
        ClassicTrigger::OnAdopt,
        ClassicTrigger::OnNaturalWonderFoundFirst,
        ClassicTrigger::OnNaturalWonderFound,
        ClassicTrigger::OnLongCountCycle,
        ClassicTrigger::OnGreatPersonExpended,
        ClassicTrigger::OnCityConverted,
        ClassicTrigger::OnReligionSpread,
        ClassicTrigger::OnGreatPersonBorn,
    ];
    const YIELDS: &'static [ClassicYield] = &[ClassicYield::Faith];
    const COSTS: &'static [ClassicCost] = &[ClassicCost::Policies, ClassicCost::PolicyCityPenalty, ClassicCost::FaithPurchase];
    const UNIT_STATS: &'static [ClassicUnitStat] =
        &[
            ClassicUnitStat::GreatGeneralPointsPercent,
            ClassicUnitStat::StackedGeneralPercent,
            ClassicUnitStat::MaintenancePercent,
            ClassicUnitStat::ExperienceNeededPercent,
            ClassicUnitStat::GeneralAuraPercent,
            ClassicUnitStat::TradeMissionGoldPercent,
            ClassicUnitStat::TradeMissionInfluencePercent,
            ClassicUnitStat::AdjacentDamageChance,
            ClassicUnitStat::GoldenAgePointsOnKillPercent,
            ClassicUnitStat::RivalLandStrengthLossPercent,
        ];

    fn index(rules: &CompiledRuleset) -> ClassicIndex {
        ClassicIndex::of(rules)
    }

    fn resolve_modifier(ix: &KeyIndex, source: &ClassicSourceModifier) -> Result<ClassicModifier, RulesError> {
        compile::resolve_modifier(ix, source)
    }

    fn resolve_condition(_ix: &KeyIndex, source: &ClassicCondition) -> Result<ClassicCondition, RulesError> {
        Ok(*source)
    }

    fn selector_fits(selector: ClassicSelector, kind: SourceKind, effect: &CompiledEffect) -> bool {
        compile::selector_fits(selector, kind, effect)
    }

    fn condition_fits(condition: ClassicCondition, kind: SourceKind, effect: &CompiledEffect) -> bool {
        compile::condition_fits(condition, kind, effect)
    }

    fn modifier_fits_scope(modifier: ClassicModifier, scope: Scope) -> bool {
        compile::modifier_fits_scope(modifier, scope)
    }

    fn trigger_fits(trigger: ClassicTrigger, kind: SourceKind) -> bool {
        compile::trigger_fits(trigger, kind)
    }

    fn check_effect(kind: SourceKind, effect: &CompiledEffect) -> Result<(), RulesError> {
        compile::check_effect(kind, effect)
    }
}

/// The classic indexes over its words, built once by the compiler: what the
/// religion, tourism and trade code looks up instead of scanning effects.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct ClassicIndex {
    /// Any belief has passive City- or Tile-scope effects (else cities do
    /// not look up their religion when evaluating modifiers).
    pub has_passive_belief: bool,
    /// Any belief has passive Unit-scope effects (else unit modifiers do
    /// not look up their owner's creed).
    pub has_passive_unit_belief: bool,
    /// Index = `BuildingKindId`: a city with this building may make
    /// tourism without great works (its own tourism effects, a faith-only
    /// building for Sacred Sites, a building a tourism effect asks for).
    pub tourism_buildings: Vec<bool>,
    /// A City-scope tourism effect of a non-building source needs no
    /// building: every city may make tourism.
    pub tourism_everywhere: bool,
    /// Policies whose passive effects give the pantheon of a city's second
    /// religion (`SecondReligionPantheon`); a city looks for its second
    /// religion only when its owner adopted one of them.
    pub second_pantheon_policies: Vec<PolicyId>,
    /// Some passive effect counts trade routes (`ClassicCount::follows_trade_routes`):
    /// a route that starts or ends refreshes both owners' cities.
    pub trade_counts: bool,
}

impl ClassicIndex {
    fn of(rules: &CompiledRuleset) -> ClassicIndex {
        let beliefs = &rules.module.beliefs;
        let mut index = ClassicIndex {
            has_passive_belief: beliefs
                .iter()
                .any(|b| !b.effects.passive_city.is_empty() || !b.effects.passive_tile.is_empty()),
            has_passive_unit_belief: beliefs.iter().any(|b| !b.effects.passive_unit.is_empty()),
            ..ClassicIndex::default()
        };
        index.index_tourism(rules);
        for (i, p) in rules.module.policies.iter().enumerate() {
            let second_pantheon = Modifier::Module(ClassicModifier::SecondReligionPantheon);
            if p.effects.passive_player.iter().any(|e| e.modifier == second_pantheon) {
                index.second_pantheon_policies.push(PolicyId(i as u16));
            }
        }
        index.trade_counts = all_effect_sets(rules).iter().any(|s| {
            let passive = [&s.passive_player, &s.passive_city];
            passive.into_iter().flatten().any(|e| {
                let count = per_count(&e.modifier).map(|per| per.count);
                matches!(count, Some(Count::Module(count)) if count.follows_trade_routes())
            })
        });
        index
    }

    /// Which buildings let a city make tourism without great works.
    fn index_tourism(&mut self, rules: &CompiledRuleset) {
        let base_tourism = |e: &CompiledEffect| {
            matches!(
                e.modifier,
                Modifier::Module(ClassicModifier::ModifyTourism { stat, .. })
                    if !matches!(stat, TourismStat::Percent | TourismStat::ThemingPercent)
                        && e.scope == Scope::City
            )
        };
        let mut flags: Vec<bool> = rules
            .buildings
            .iter()
            .map(|b| {
                b.effects.passive_city.iter().any(base_tourism)
                    || b.unbuildable && b.module.faith_cost.is_some()
            })
            .collect();
        for set in all_effect_sets(rules) {
            for effect in set.passive_city.iter().filter(|e| base_tourism(e)) {
                // Per faith building and per great work need those already.
                let counted = matches!(
                    effect.modifier,
                    Modifier::Module(ClassicModifier::ModifyTourism {
                        stat: TourismStat::PerFaithBuilding | TourismStat::GreatWorksPercent,
                        ..
                    })
                );
                match effect.condition {
                    Condition::HasBuilding(b) => flags[b.0 as usize] = true,
                    _ if effect.selector != Selector::SourceCity && !counted => {
                        self.tourism_everywhere = true
                    }
                    _ => {}
                }
            }
        }
        self.tourism_buildings = flags;
    }
}
